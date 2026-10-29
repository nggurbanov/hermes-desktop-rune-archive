//! Managed runtime operations target the active agent profile, including remote hosts.
use crate::pages::{PageEffect, PageRequest, segment};
use rune::TextEdit;
use serde_json::{Value, json};
pub struct LocalModels {
    pub profile: String,
    pub status: Value,
    pub hardware: Value,
    pub catalog: Vec<Value>,
    pub jobs: Vec<Value>,
    pub hits: Vec<Value>,
    pub files: Vec<Value>,
    pub repository: String,
    pub query: TextEdit,
    pub sideload: TextEdit,
    pub backend: TextEdit,
    pub loading: bool,
    pub busy: bool,
    pub searching: bool,
    pub listing: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub confirm_delete: Option<String>,
    clock: f64,
    next_poll: f64,
    polling: bool,
    watching: bool,
    seen_quickstarts: std::collections::BTreeSet<String>,
    chat_ready: Option<String>,
}
impl LocalModels {
    pub fn new() -> Self {
        Self {
            profile: String::new(),
            status: Value::Null,
            hardware: Value::Null,
            catalog: vec![],
            jobs: vec![],
            hits: vec![],
            files: vec![],
            repository: String::new(),
            query: TextEdit::new(""),
            sideload: TextEdit::new(""),
            backend: TextEdit::new(""),
            loading: false,
            busy: false,
            searching: false,
            listing: false,
            error: None,
            notice: None,
            confirm_delete: None,
            clock: 0.0,
            next_poll: 0.0,
            polling: false,
            watching: false,
            seen_quickstarts: std::collections::BTreeSet::new(),
            chat_ready: None,
        }
    }
    pub fn observe_quickstarts(&mut self, watching: bool) {
        if !watching {
            self.seen_quickstarts.clear();
            self.chat_ready = None;
        }
        self.watching = watching;
    }
    pub fn take_chat_ready(&mut self) -> Option<String> {
        self.chat_ready.take()
    }
    fn pin(&self, e: Vec<PageEffect>) -> Vec<PageEffect> {
        e.into_iter()
            .map(|(o, r)| (o, r.profile(&self.profile)))
            .collect()
    }
    pub fn load(&mut self, profile: &str) -> Vec<PageEffect> {
        if self.profile != profile {
            let watching = self.watching;
            *self = Self::new();
            self.watching = watching;
            self.profile = profile.into();
        }
        self.loading = true;
        self.error = None;
        let e = vec![
            (
                "settings.local.status".into(),
                PageRequest::get("/api/local-models/status"),
            ),
            (
                "settings.local.hardware".into(),
                PageRequest::get("/api/local-models/hardware"),
            ),
            (
                "settings.local.catalog".into(),
                PageRequest::get("/api/local-models/catalog"),
            ),
            (
                "settings.local.jobs".into(),
                PageRequest::get("/api/local-models/jobs"),
            ),
        ];
        self.pin(e)
    }
    pub fn active_jobs(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| matches!(j["status"].as_str(), Some("running" | "paused")))
    }
    fn refresh(&self) -> Vec<PageEffect> {
        self.pin(vec![
            (
                "settings.local.status".into(),
                PageRequest::get("/api/local-models/status"),
            ),
            (
                "settings.local.catalog".into(),
                PageRequest::get("/api/local-models/catalog"),
            ),
            (
                "settings.local.jobs".into(),
                PageRequest::get("/api/local-models/jobs"),
            ),
        ])
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let e = self.apply_unscoped(op, result);
        self.pin(e)
    }
    fn apply_unscoped(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                match op {
                    "settings.local.write" | "settings.local.pause" | "settings.local.resume" => {
                        self.busy = false
                    }
                    "settings.local.search" => self.searching = false,
                    "settings.local.files" => self.listing = false,
                    "settings.local.jobs" => self.polling = false,
                    _ => self.loading = false,
                }
                self.error = Some(e);
                return vec![];
            }
        };
        match op {
            "settings.local.status" => {
                self.loading = false;
                if !v["models"].is_array() || !v["runtime_installed"].is_boolean() {
                    self.error = Some("Invalid local runtime status".into());
                    return vec![];
                }
                self.status = v;
            }
            "settings.local.hardware" => self.hardware = v,
            "settings.local.catalog" => {
                self.loading = false;
                if let Some(rows) = v["models"].as_array() {
                    self.catalog = rows.clone();
                } else {
                    self.error = Some("Invalid local model catalog".into());
                }
            }
            "settings.local.jobs" => {
                self.polling = false;
                let Some(jobs) = v["jobs"].as_array() else {
                    self.error = Some("Invalid runtime job list".into());
                    return vec![];
                };
                let changed = self.jobs.iter().any(|old| {
                    old["status"] == "running"
                        && jobs.iter().any(|new| {
                            new["job_id"] == old["job_id"]
                                && matches!(new["status"].as_str(), Some("done" | "error"))
                        })
                });
                if self.watching {
                    for job in jobs {
                        if job["kind"] != "quickstart" {
                            continue;
                        }
                        let Some(id) = job["job_id"].as_str() else {
                            continue;
                        };
                        if job["status"] == "running" {
                            self.seen_quickstarts.insert(id.into());
                        } else if job["status"] == "done" && self.seen_quickstarts.remove(id) {
                            self.chat_ready = Some(self.profile.clone());
                        } else if job["status"] == "error" {
                            self.seen_quickstarts.remove(id);
                        }
                    }
                }
                self.jobs = jobs.clone();
                self.next_poll = self.clock + 2.0;
                if changed {
                    return vec![
                        (
                            "settings.local.status".into(),
                            PageRequest::get("/api/local-models/status"),
                        ),
                        (
                            "settings.local.catalog".into(),
                            PageRequest::get("/api/local-models/catalog"),
                        ),
                    ];
                }
            }
            "settings.local.search" => {
                self.searching = false;
                if let Some(hits) = v["hits"].as_array() {
                    self.hits = hits.clone();
                } else {
                    self.error = Some("Invalid model search response".into());
                }
            }
            "settings.local.files" => {
                self.listing = false;
                if let Some(files) = v["files"].as_array() {
                    self.files = files.clone();
                } else {
                    self.error = Some("Invalid repository file list".into());
                }
            }
            "settings.local.write" => {
                self.busy = false;
                if v["ok"].as_bool() == Some(false) {
                    self.error = Some("The agent refused the local model operation".into());
                    return vec![];
                }
                if let Some(id) = v["job_id"].as_str().filter(|id| !id.is_empty()) {
                    self.notice = Some(format!(
                        "Job started: {id}. Progress is reported by the agent."
                    ));
                    self.next_poll = self.clock;
                } else if v["ok"].as_bool() == Some(true)
                    || v["already_downloaded"].as_bool() == Some(true)
                {
                    self.notice =
                        Some("Local model change confirmed; refreshing runtime status".into());
                } else {
                    self.error = Some("The agent did not acknowledge the runtime operation".into());
                    return vec![];
                }
                return self.refresh();
            }
            "settings.local.pause" | "settings.local.resume" => {
                self.busy = false;
                let flag = if op.ends_with("pause") {
                    "paused"
                } else {
                    "resumed"
                };
                if v["ok"].as_bool() != Some(true) || v[flag].as_bool() != Some(true) {
                    self.error = Some(format!("This job cannot be {flag} in its current phase"));
                    return vec![];
                }
                return vec![(
                    "settings.local.jobs".into(),
                    PageRequest::get("/api/local-models/jobs"),
                )];
            }
            _ => {}
        }
        vec![]
    }
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.busy {
            return vec![];
        }
        self.error = None;
        let e = match a {
            Action::Load => {
                let profile = self.profile.clone();
                self.load(&profile)
            }
            Action::Install => {
                if self.jobs.iter().any(|j| {
                    j["kind"] == "runtime-install"
                        && matches!(j["status"].as_str(), Some("running" | "paused"))
                }) {
                    return vec![];
                }
                self.write("/api/local-models/runtime/install",json!({"backend":if self.backend.text.trim().is_empty(){Value::Null}else{json!(self.backend.text.trim())}}))
            }
            Action::Quickstart => {
                let Some(model) = self.catalog.iter().find(|m| {
                    m["recommended"].as_bool() == Some(true) && m["fits"].as_bool() == Some(true)
                }) else {
                    self.error = Some("No model is recommended for this agent's hardware".into());
                    return vec![];
                };
                let id = model["id"].clone();
                self.write("/api/local-models/quickstart", json!({"model_id":id}))
            }
            Action::Download(id) => {
                if !self
                    .catalog
                    .iter()
                    .any(|m| m["id"] == id && m["fits"].as_bool() == Some(true))
                    || self.status["runtime_installed"].as_bool() != Some(true)
                    || self.jobs.iter().any(|j| {
                        j["kind"] == "model-download"
                            && matches!(j["status"].as_str(), Some("running" | "paused"))
                    })
                {
                    return vec![];
                }
                self.write("/api/local-models/download", json!({"model_id":id}))
            }
            Action::Activate(id) => {
                if !self.status["models"]
                    .as_array()
                    .is_some_and(|m| m.iter().any(|m| m["id"] == id))
                {
                    self.error = Some("This model is not in the downloaded library".into());
                    return vec![];
                }
                if self.jobs.iter().any(|j| {
                    j["kind"] == "model-activate"
                        && matches!(j["status"].as_str(), Some("running" | "paused"))
                }) {
                    return vec![];
                }
                self.write("/api/local-models/activate", json!({"model_id":id}))
            }
            Action::Eject(id) => self.write("/api/local-models/eject", json!({"model_id":id})),
            Action::Delete(id) => {
                if self.status["models"]
                    .as_array()
                    .is_some_and(|m| m.iter().any(|m| m["id"] == id))
                {
                    self.confirm_delete = Some(id);
                }
                vec![]
            }
            Action::CancelDelete => {
                self.confirm_delete = None;
                vec![]
            }
            Action::ConfirmDelete => {
                let Some(id) = self.confirm_delete.take() else {
                    return vec![];
                };
                self.busy = true;
                vec![(
                    "settings.local.write".into(),
                    PageRequest::delete(
                        format!("/api/local-models/models/{}", segment(&id)),
                        Value::Null,
                    ),
                )]
            }
            Action::Server(start) => self.write(
                "/api/local-models/server",
                json!({"action":if start{"start"}else{"stop"}}),
            ),
            Action::Pause(id, pause) => {
                let flag = if pause { "can_pause" } else { "can_resume" };
                if !self
                    .jobs
                    .iter()
                    .any(|j| j["job_id"] == id && j[flag].as_bool() == Some(true))
                {
                    self.error =
                        Some("The download cannot be controlled in its current phase".into());
                    return vec![];
                }
                self.busy = true;
                vec![(
                    if pause {
                        "settings.local.pause"
                    } else {
                        "settings.local.resume"
                    }
                    .into(),
                    PageRequest::post(
                        format!(
                            "/api/local-models/download/{}",
                            if pause { "pause" } else { "resume" }
                        ),
                        json!({"job_id":id}),
                    ),
                )]
            }
            Action::Search => {
                if self.query.text.trim().len() < 2 {
                    self.hits.clear();
                    return vec![];
                }
                self.searching = true;
                vec![(
                    "settings.local.search".into(),
                    PageRequest::get("/api/local-models/search")
                        .query("q", self.query.text.trim())
                        .query("limit", 20),
                )]
            }
            Action::Repository(repo) => {
                self.repository = repo.clone();
                self.files.clear();
                self.listing = true;
                vec![(
                    "settings.local.files".into(),
                    PageRequest::get("/api/local-models/search/files").query("repo", repo),
                )]
            }
            Action::BrowseDownload(repo, paths) => {
                if repo != self.repository {
                    return vec![];
                }
                let Some(group) = self.files.iter().find(|g| g["paths"] == json!(paths)) else {
                    return vec![];
                };
                if paths.is_empty() || group["fit"] == "too-big" {
                    self.error = Some("These files do not fit the agent's hardware".into());
                    return vec![];
                }
                self.write(
                    "/api/local-models/download-browsed",
                    json!({"repo":repo,"paths":paths}),
                )
            }
            Action::Sideload => {
                let path = self.sideload.text.trim().to_owned();
                if path.is_empty() {
                    self.error = Some("Enter the existing GGUF file path on the agent".into());
                    return vec![];
                }
                self.write("/api/local-models/sideload", json!({"path":path}))
            }
        };
        self.pin(e)
    }
    fn write(&mut self, path: &str, body: Value) -> Vec<PageEffect> {
        self.busy = true;
        vec![("settings.local.write".into(), PageRequest::post(path, body))]
    }
    pub fn poll(&mut self, now: f64, visible: bool) -> Vec<PageEffect> {
        self.clock = now;
        if (visible || self.active_jobs())
            && !self.profile.is_empty()
            && !self.polling
            && now >= self.next_poll
        {
            self.next_poll = now + 2.0;
            self.polling = true;
            return self.pin(vec![
                (
                    "settings.local.jobs".into(),
                    PageRequest::get("/api/local-models/jobs"),
                ),
                (
                    "settings.local.status".into(),
                    PageRequest::get("/api/local-models/status"),
                ),
            ]);
        }
        vec![]
    }
    pub fn next_wake(&self, visible: bool) -> Option<std::time::Duration> {
        ((visible || self.active_jobs()) && !self.profile.is_empty() && !self.polling)
            .then(|| std::time::Duration::from_secs_f64((self.next_poll - self.clock).max(0.01)))
    }
}
#[derive(Clone)]
pub enum Action {
    Load,
    Install,
    Quickstart,
    Download(String),
    Activate(String),
    Eject(String),
    Delete(String),
    CancelDelete,
    ConfirmDelete,
    Server(bool),
    Pause(String, bool),
    Search,
    Repository(String),
    BrowseDownload(String, Vec<String>),
    Sideload,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> LocalModels {
        let mut s = LocalModels::new();
        s.profile = "active-agent".into();
        s.apply("settings.local.status",Ok(json!({"runtime_installed":true,"models":[{"id":"downloaded-model"}],"loaded_models":{}})));
        s.apply(
            "settings.local.catalog",
            Ok(json!({"models":[{"id":"catalog-model","recommended":true,"fits":true}]})),
        );
        s
    }
    #[test]
    fn runtime_jobs_and_commands_pin_active_profile_and_distinguish_queued_from_done() {
        let mut s = state();
        let e = s.act(Action::Activate("downloaded-model".into()));
        assert_eq!(e[0].1.path, "/api/local-models/activate");
        assert_eq!(e[0].1.profile.as_deref(), Some("active-agent"));
        let e = s.apply("settings.local.write", Ok(json!({"job_id":"job-1"})));
        assert!(
            e.iter()
                .all(|(_, r)| r.profile.as_deref() == Some("active-agent"))
        );
        assert!(!s.notice.as_ref().unwrap().contains("activated"));
        s.apply(
            "settings.local.jobs",
            Ok(json!({"jobs":[{"job_id":"job-1","kind":"model-activate","status":"running"}]})),
        );
        let e = s.apply(
            "settings.local.jobs",
            Ok(json!({"jobs":[{"job_id":"job-1","kind":"model-activate","status":"done"}]})),
        );
        assert!(e.iter().any(|(o, _)| o == "settings.local.status"));
    }
    #[test]
    fn false_pause_ack_is_failure_and_stale_repo_paths_cannot_download() {
        let mut s = state();
        s.jobs = vec![json!({"job_id":"job","status":"running","can_pause":true})];
        assert_eq!(
            s.act(Action::Pause("job".into(), true))[0].1.body,
            json!({"job_id":"job"})
        );
        s.apply(
            "settings.local.pause",
            Ok(json!({"ok":true,"paused":false})),
        );
        assert!(s.error.is_some());
        s.repository = "owner/current".into();
        s.files = vec![json!({"paths":["model.gguf"],"fit":"fits-gpu"})];
        assert!(
            s.act(Action::BrowseDownload(
                "owner/old".into(),
                vec!["model.gguf".into()]
            ))
            .is_empty()
        );
        assert!(
            s.act(Action::BrowseDownload(
                "owner/current".into(),
                vec!["other.gguf".into()]
            ))
            .is_empty()
        );
        let e = s.act(Action::BrowseDownload(
            "owner/current".into(),
            vec!["model.gguf".into()],
        ));
        assert_eq!(
            e[0].1.body,
            json!({"repo":"owner/current","paths":["model.gguf"]})
        );
    }
    #[test]
    fn file_delete_requires_metadata_match_and_confirmation() {
        let mut s = state();
        assert!(s.act(Action::Delete("unknown".into())).is_empty());
        assert!(s.confirm_delete.is_none());
        assert!(s.act(Action::Delete("downloaded-model".into())).is_empty());
        let e = s.act(Action::ConfirmDelete);
        assert_eq!(e[0].1.path, "/api/local-models/models/downloaded-model");
        assert!(s.busy);
        s.apply("settings.local.hardware", Err("Read failed".into()));
        assert!(s.busy);
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn quickstart_navigation_requires_observed_running_job_and_live_panel() {
        let mut s = LocalModels::new();
        s.load("active-agent");
        s.observe_quickstarts(true);
        let done = json!({"jobs":[{"job_id":"old","kind":"quickstart","status":"done"}]});
        s.apply("settings.local.jobs", Ok(done));
        assert!(s.take_chat_ready().is_none());
        s.apply(
            "settings.local.jobs",
            Ok(json!({"jobs":[{"job_id":"new","kind":"quickstart","status":"running"}]})),
        );
        s.apply(
            "settings.local.jobs",
            Ok(json!({"jobs":[{"job_id":"new","kind":"quickstart","status":"done"}]})),
        );
        assert_eq!(s.take_chat_ready().as_deref(), Some("active-agent"));
        assert!(s.take_chat_ready().is_none());
        s.apply(
            "settings.local.jobs",
            Ok(json!({"jobs":[{"job_id":"gone","kind":"quickstart","status":"running"}]})),
        );
        s.observe_quickstarts(false);
        s.observe_quickstarts(true);
        s.apply(
            "settings.local.jobs",
            Ok(json!({"jobs":[{"job_id":"gone","kind":"quickstart","status":"done"}]})),
        );
        assert!(s.take_chat_ready().is_none());
    }
}
