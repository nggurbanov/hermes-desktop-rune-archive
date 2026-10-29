//! Scheduled jobs retain their backend owner, editor intent and mutation/readback outcomes.
use crate::{
    app::Viewer,
    gateway::ModelCatalog,
    pages::{PageEffect, PageRequest, segment},
};
use rune::TextEdit;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Job {
    pub id: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub no_agent: bool,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub schedule: Option<Schedule>,
    #[serde(default)]
    pub schedule_display: Option<String>,
    #[serde(default)]
    pub deliver: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub next_run_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Schedule {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub expr: Option<String>,
    #[serde(default)]
    pub display: Option<String>,
}
impl Job {
    pub fn title(&self) -> &str {
        [
            self.name.as_deref(),
            self.prompt.as_deref(),
            self.script.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
        .unwrap_or(&self.id)
    }
    pub fn description(&self) -> &str {
        self.prompt
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(self.script.as_deref())
            .unwrap_or("")
    }
    pub fn state(&self) -> &str {
        self.state
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(if self.enabled == Some(false) {
                "disabled"
            } else {
                "scheduled"
            })
    }
    pub fn script_only(&self) -> bool {
        self.no_agent && self.script.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
    pub fn schedule(&self) -> &str {
        self.schedule
            .as_ref()
            .and_then(|s| s.display.as_deref().or(s.expr.as_deref()))
            .or(self.schedule_display.as_deref())
            .unwrap_or("")
    }
}

/// An overdue next slot is a scheduler fault signal, not a promised future run.
pub fn next_run_overdue(job: &Job, now: i64) -> bool {
    if matches!(job.state(), "paused" | "disabled" | "completed") {
        return false;
    }
    job.next_run_at
        .as_deref()
        .and_then(iso_seconds)
        .is_some_and(|at| now.saturating_sub(at) > 900)
}
fn iso_seconds(iso: &str) -> Option<i64> {
    let number = |a, b| iso.get(a..b)?.parse::<i64>().ok();
    let (year, month, day, hour, minute, second) = (
        number(0, 4)?,
        number(5, 7)?,
        number(8, 10)?,
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
    );
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let suffix = iso.get(19..)?;
    let offset = if suffix.ends_with('Z') {
        0
    } else {
        let at = suffix.find(['+', '-'])?;
        let zone = suffix.get(at..)?;
        let h = zone.get(1..3)?.parse::<i64>().ok()?;
        let m = zone.get(4..6)?.parse::<i64>().ok()?;
        if h > 23 || m > 59 {
            return None;
        }
        (h * 3600 + m * 60) * if zone.starts_with('-') { -1 } else { 1 }
    };
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let shifted = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * shifted + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some(days * 86400 + hour * 3600 + minute * 60 + second - offset)
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct DeliveryTarget {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub home_target_set: bool,
    #[serde(default)]
    pub home_env_var: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct BlueprintField {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub strict: Option<bool>,
    #[serde(default)]
    pub help: String,
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct Blueprint {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub fields: Vec<BlueprintField>,
    #[serde(default)]
    pub plugin: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct Run {
    #[serde(default, alias = "id")]
    pub session_id: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub started_at: Option<f64>,
    #[serde(default)]
    pub last_active: Option<f64>,
    #[serde(default)]
    pub message_count: Option<u64>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub ended_at: Option<f64>,
    #[serde(default)]
    pub scheduler_owned: Option<bool>,
    #[serde(default)]
    pub is_active: Option<bool>,
}

#[derive(Clone)]
pub struct Editor {
    pub job: Option<Job>,
    pub name: TextEdit,
    pub prompt: TextEdit,
    pub schedule: TextEdit,
    pub deliver: String,
    pub provider: String,
    pub model: String,
    pub blueprint: Option<String>,
    pub slots: BTreeMap<String, TextEdit>,
    pub open_select: Option<String>,
    pub error: Option<String>,
}
impl Editor {
    pub fn new(job: Option<Job>) -> Self {
        let value = job.as_ref();
        Self {
            name: TextEdit::new(value.and_then(|j| j.name.as_deref()).unwrap_or("")),
            prompt: TextEdit::new(value.and_then(|j| j.prompt.as_deref()).unwrap_or("")),
            schedule: TextEdit::new(
                value
                    .and_then(|j| {
                        j.schedule
                            .as_ref()
                            .and_then(|s| s.expr.as_deref())
                            .or(j.schedule_display.as_deref())
                    })
                    .unwrap_or("0 9 * * *"),
            ),
            deliver: value
                .and_then(|j| j.deliver.clone())
                .unwrap_or_else(|| "local".into()),
            provider: value.and_then(|j| j.provider.clone()).unwrap_or_default(),
            model: value.and_then(|j| j.model.clone()).unwrap_or_default(),
            job,
            blueprint: None,
            slots: BTreeMap::new(),
            open_select: None,
            error: None,
        }
    }
    pub fn select_blueprint(&mut self, blueprint: Option<&Blueprint>) {
        self.blueprint = blueprint.map(|b| b.key.clone());
        self.slots = blueprint
            .into_iter()
            .flat_map(|b| b.fields.iter())
            .map(|f| {
                let value = f.default.as_deref().unwrap_or("");
                let value = if f.name == "deliver" && (value.is_empty() || value == "origin") {
                    "local"
                } else {
                    value
                };
                (f.name.clone(), TextEdit::new(value))
            })
            .collect();
        self.error = None;
    }
    pub fn payload(&self) -> Result<Value, String> {
        if let Some(blueprint) = &self.blueprint {
            return Ok(
                json!({"blueprint":blueprint,"values":self.slots.iter().map(|(k,v)|(k.clone(),v.text.clone())).collect::<BTreeMap<_,_>>()}),
            );
        }
        let script_only = self.job.as_ref().is_some_and(Job::script_only);
        if self.schedule.text.trim().is_empty() {
            return Err("Enter a schedule".into());
        }
        if !script_only && self.prompt.text.trim().is_empty() {
            return Err("Enter a prompt".into());
        }
        let mut value = json!({"name":self.name.text.trim(),"schedule":self.schedule.text.trim(),"deliver":self.deliver});
        if !script_only || !self.prompt.text.trim().is_empty() {
            value["prompt"] = json!(self.prompt.text.trim());
        }
        if !script_only {
            value["model"] = if self.model.trim().is_empty() {
                Value::Null
            } else {
                json!(self.model.trim())
            };
            value["provider"] = if self.provider.trim().is_empty() {
                Value::Null
            } else {
                json!(self.provider.trim())
            };
        }
        if self.job.is_some() {
            Ok(json!({"updates":value}))
        } else {
            if value["model"].is_null() {
                value.as_object_mut().unwrap().remove("model");
                value.as_object_mut().unwrap().remove("provider");
            }
            Ok(value)
        }
    }
}

#[derive(Default)]
pub struct Cron {
    pub jobs: Vec<Job>,
    pub selected: Option<String>,
    pub query: TextEdit,
    pub editor: Option<Editor>,
    pub delete: Option<String>,
    pub runs: Vec<Run>,
    pub runs_job: Option<String>,
    pub runs_loading: Option<String>,
    pub delivery: Vec<DeliveryTarget>,
    pub blueprints: Vec<Blueprint>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub delivery_error: Option<String>,
    pub blueprints_error: Option<String>,
    pub models_error: Option<String>,
    pub models: Option<ModelCatalog>,
    pub busy: std::collections::BTreeSet<String>,
    pub loaded: bool,
    read_epoch: u64,
    refreshing_after_write: bool,
    pub triggering: std::collections::BTreeSet<String>,
    last_refresh: Option<std::time::Instant>,
}
#[derive(Clone)]
pub enum Action {
    Refresh,
    Select(String),
    Create,
    Edit(String),
    CancelEditor,
    Save,
    PauseResume(String),
    Trigger(String),
    AskDelete(String),
    CancelDelete,
    ConfirmDelete,
    Blueprint(String),
    ToggleDelivery(String),
    Preset(String),
    Model(String, String),
    OpenRun(usize),
}
impl Viewer {
    pub fn cron_action(&mut self, action: Action) {
        if let Action::OpenRun(index) = action {
            if let Some(run) = self
                .cron
                .runs
                .get(index)
                .filter(|r| r.source.as_deref() == Some("cron"))
            {
                self.open_run_session(
                    run.profile.clone().unwrap_or_else(|| self.profile.clone()),
                    run.session_id.clone(),
                    run.title.clone().unwrap_or_else(|| "Scheduled run".into()),
                    self.cron.run_read_only(index),
                );
            }
            return;
        }
        let effects = self.cron.action(action, &self.profile);
        self.page_requests(effects);
    }
}
impl Cron {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fixture(&mut self) {
        self.seed_fixture();
        self.apply("cron.runs.morning-brief", Ok(json!({"runs":[{"session_id":"synthetic-morning-run","source":"cron","title":"Morning brief · today","ended_at":1793250000.0,"scheduler_owned":false,"message_count":4}]})));
        self.loaded = true;
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.read_epoch += 1;
        self.error = None;
        self.loaded = false;
        self.last_refresh = Some(std::time::Instant::now());
        vec![
            (
                format!("cron.jobs.{}", self.read_epoch),
                PageRequest::get("/api/cron/jobs"),
            ),
            (
                "cron.delivery".into(),
                PageRequest::get("/api/cron/delivery-targets"),
            ),
            (
                "cron.blueprints".into(),
                PageRequest::get("/api/cron/blueprints"),
            ),
            (
                "cron.models".into(),
                PageRequest::get("/api/model/options").query("explicit_only", "1"),
            ),
        ]
    }
    pub fn tick(&mut self) -> Vec<PageEffect> {
        if self
            .last_refresh
            .is_none_or(|last| last.elapsed() < std::time::Duration::from_secs(15))
        {
            return vec![];
        }
        self.last_refresh = Some(std::time::Instant::now());
        let mut effects = self.refresh_jobs();
        if let Some(id) = self.selected.clone() {
            effects.push(self.run_request(&id));
        }
        effects
    }
    pub fn run_read_only(&self, index: usize) -> bool {
        let Some(run) = self.runs.get(index) else {
            return true;
        };
        if run.source.as_deref() != Some("cron") {
            return true;
        }
        if run.ended_at.is_some() {
            return false;
        }
        if let Some(owned) = run.scheduler_owned {
            return !owned;
        }
        if let Some(active) = run.is_active {
            return !active;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or_default();
        !run.last_active.is_some_and(|last| now - last < 300.0)
    }
    fn refresh_jobs(&mut self) -> Vec<PageEffect> {
        self.read_epoch += 1;
        vec![(
            format!("cron.jobs.{}", self.read_epoch),
            PageRequest::get("/api/cron/jobs"),
        )]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        if let Some(epoch) = op.strip_prefix("cron.jobs.") {
            if epoch.parse::<u64>().ok() != Some(self.read_epoch) {
                return vec![];
            }
            self.loaded = true;
            match result.and_then(|v| self.apply_jobs(v)) {
                Ok(()) => {
                    self.error = None;
                    self.refreshing_after_write = false;
                }
                Err(e) => {
                    self.error = Some(if self.refreshing_after_write {
                        format!(
                            "The change succeeded, but the jobs list could not be refreshed: {e}"
                        )
                    } else {
                        e
                    })
                }
            }
            return vec![];
        }
        if op == "cron.delivery" {
            match result.and_then(|v| {
                serde_json::from_value(v["targets"].clone())
                    .map_err(|_| "Invalid delivery targets".into())
            }) {
                Ok(targets) => {
                    self.delivery = targets;
                    self.delivery_error = None;
                }
                Err(e) => self.delivery_error = Some(e),
            }
            return vec![];
        }
        if op == "cron.blueprints" {
            match result.and_then(|v| {
                serde_json::from_value(v["blueprints"].clone())
                    .map_err(|_| "Invalid automation blueprints".into())
            }) {
                Ok(blueprints) => {
                    self.blueprints = blueprints;
                    self.blueprints_error = None;
                }
                Err(e) => self.blueprints_error = Some(e),
            }
            return vec![];
        }
        if op == "cron.models" {
            match result.and_then(|v| ModelCatalog::from_value(&v)) {
                Ok(catalog) => {
                    self.models = Some(catalog);
                    self.models_error = None;
                }
                Err(e) => self.models_error = Some(e),
            }
            return vec![];
        }
        if let Some(id) = op.strip_prefix("cron.runs.") {
            if self.selected.as_deref() != Some(id) {
                return vec![];
            }
            self.runs_loading = None;
            match result.and_then(|v| {
                serde_json::from_value(v["runs"].clone()).map_err(|_| "Invalid run history".into())
            }) {
                Ok(runs) => {
                    self.runs = runs;
                    self.runs_job = Some(id.into());
                }
                Err(e) => self.error = Some(e),
            }
            return vec![];
        }
        if let Some(id) = op.strip_prefix("cron.trigger.") {
            self.triggering.remove(id);
        }
        if let Some(id) = op
            .strip_prefix("cron.trigger.")
            .or_else(|| op.strip_prefix("cron.toggle."))
            .or_else(|| op.strip_prefix("cron.delete."))
        {
            self.busy.remove(id);
        }
        if op == "cron.save" {
            self.busy.remove("editor");
        }
        match result {
            Err(e) => {
                if op == "cron.save" {
                    if let Some(editor) = &mut self.editor {
                        editor.error = Some(e.clone());
                    }
                }
                self.error = Some(e);
                vec![]
            }
            Ok(value) => {
                self.notice = Some(
                    if op.starts_with("cron.trigger.") {
                        "Job run completed"
                    } else if op.starts_with("cron.delete.") {
                        "Job deleted"
                    } else {
                        "Scheduled job saved"
                    }
                    .into(),
                );
                if op == "cron.save" {
                    self.editor = None;
                    if let Some(id) = value.get("id").and_then(Value::as_str) {
                        self.selected = Some(id.into());
                    }
                }
                if op.strip_prefix("cron.delete.") == self.delete.as_deref() {
                    self.delete = None;
                }
                self.refreshing_after_write = true;
                let mut next = self.refresh_jobs();
                if let Some(id) = self.selected.clone() {
                    next.push(self.run_request(&id));
                }
                next
            }
        }
    }
    fn run_request(&mut self, id: &str) -> PageEffect {
        self.runs_loading = Some(id.into());
        let request =
            PageRequest::get(format!("/api/cron/jobs/{}/runs", segment(id))).query("limit", "20");
        let request = if let Some(p) = self
            .jobs
            .iter()
            .find(|j| j.id == id)
            .and_then(|j| j.profile.as_deref())
        {
            request.profile(p)
        } else {
            request
        };
        (format!("cron.runs.{id}"), request)
    }
    pub fn action(&mut self, action: Action, profile: &str) -> Vec<PageEffect> {
        self.error = None;
        self.notice = None;
        let is_trigger = matches!(&action, Action::Trigger(_));
        match action {
            Action::Refresh => self.load(),
            Action::Select(id) => {
                self.selected = Some(id.clone());
                self.runs.clear();
                self.runs_job = None;
                vec![self.run_request(&id)]
            }
            Action::Create => {
                if self.busy.contains("editor") {
                    return vec![];
                }
                self.editor = Some(Editor::new(None));
                vec![]
            }
            Action::Edit(id) => {
                if self.busy.contains("editor") {
                    return vec![];
                }
                if let Some(job) = self.jobs.iter().find(|j| j.id == id).cloned() {
                    self.editor = Some(Editor::new(Some(job)));
                }
                vec![]
            }
            Action::CancelEditor => {
                if !self.busy.contains("editor") {
                    self.editor = None;
                }
                vec![]
            }
            Action::AskDelete(id) => {
                self.delete = Some(id);
                vec![]
            }
            Action::CancelDelete => {
                self.delete = None;
                vec![]
            }
            Action::Blueprint(key) => {
                if self.busy.contains("editor") {
                    return vec![];
                }
                let blueprint = self.blueprints.iter().find(|b| b.key == key);
                let mut editor = Editor::new(None);
                editor.select_blueprint(blueprint);
                self.editor = Some(editor);
                vec![]
            }
            Action::ToggleDelivery(target) => {
                self.toggle_delivery(&target);
                vec![]
            }
            Action::Preset(expr) => {
                if let Some(e) = &mut self.editor {
                    e.schedule.replace_text(expr);
                    e.open_select = None;
                }
                vec![]
            }
            Action::Model(provider, model) => {
                if let Some(e) = &mut self.editor {
                    e.provider = provider;
                    e.model = model;
                    e.open_select = None;
                }
                vec![]
            }
            Action::Save => {
                if self.busy.contains("editor") {
                    return vec![];
                }
                let Some(editor) = &mut self.editor else {
                    return vec![];
                };
                let payload = match editor.payload() {
                    Ok(p) => p,
                    Err(e) => {
                        editor.error = Some(e);
                        return vec![];
                    }
                };
                let owner = editor
                    .job
                    .as_ref()
                    .and_then(|j| j.profile.as_deref())
                    .unwrap_or(if profile == "all" { "default" } else { profile });
                let req = if editor.blueprint.is_some() {
                    PageRequest::post("/api/cron/blueprints/instantiate", payload)
                } else if let Some(job) = &editor.job {
                    PageRequest::put(format!("/api/cron/jobs/{}", segment(&job.id)), payload)
                } else {
                    PageRequest::post("/api/cron/jobs", payload)
                }
                .profile(owner);
                self.busy.insert("editor".into());
                self.read_epoch += 1;
                vec![("cron.save".into(), req)]
            }
            Action::PauseResume(id) | Action::Trigger(id) => {
                if self.busy.contains(&id) {
                    return vec![];
                }
                let Some(job) = self.jobs.iter().find(|j| j.id == id) else {
                    return vec![];
                };
                let trigger = is_trigger;
                let verb = if trigger {
                    "trigger"
                } else if matches!(job.state(), "paused" | "disabled") {
                    "resume"
                } else {
                    "pause"
                };
                let req = PageRequest::post(
                    format!("/api/cron/jobs/{}/{verb}", segment(&id)),
                    Value::Null,
                )
                .profile(job.profile.as_deref().unwrap_or(profile));
                let req = if trigger {
                    req.timeout_secs(86400)
                } else {
                    req
                };
                self.busy.insert(id.clone());
                if trigger {
                    self.triggering.insert(id.clone());
                }
                self.read_epoch += 1;
                vec![(
                    format!("cron.{}.{id}", if trigger { "trigger" } else { "toggle" }),
                    req,
                )]
            }
            Action::ConfirmDelete => {
                let Some(id) = self.delete.clone() else {
                    return vec![];
                };
                if self.busy.contains(&id) {
                    return vec![];
                }
                let owner = self
                    .jobs
                    .iter()
                    .find(|j| j.id == id)
                    .and_then(|j| j.profile.as_deref())
                    .unwrap_or(profile);
                let req =
                    PageRequest::delete(format!("/api/cron/jobs/{}", segment(&id)), Value::Null)
                        .profile(owner);
                self.busy.insert(id.clone());
                self.read_epoch += 1;
                vec![(format!("cron.delete.{id}"), req)]
            }
            Action::OpenRun(_) => vec![],
        }
    }
    pub fn selected_job(&self) -> Option<&Job> {
        self.selected
            .as_ref()
            .and_then(|id| self.jobs.iter().find(|j| &j.id == id))
    }
    pub fn visible(&self) -> Vec<&Job> {
        let q = self.query.text.trim().to_lowercase();
        self.jobs
            .iter()
            .filter(|j| {
                q.is_empty()
                    || [j.title(), j.description(), j.state(), j.schedule()]
                        .into_iter()
                        .any(|s| s.to_lowercase().contains(&q))
            })
            .collect()
    }
    pub fn toggle_delivery(&mut self, target: &str) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let mut targets: Vec<_> = editor
            .deliver
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if targets.iter().any(|s| s == target) {
            if targets.len() > 1 {
                targets.retain(|s| s != target);
            }
        } else {
            targets.push(target.into());
        }
        editor.deliver = targets.join(",");
    }
    pub fn apply_jobs(&mut self, value: Value) -> Result<(), String> {
        let jobs: Vec<Job> =
            serde_json::from_value(value).map_err(|_| "Invalid scheduled jobs response")?;
        if jobs.iter().any(|j| j.id.is_empty()) {
            return Err("Scheduled job has no identity".into());
        }
        self.jobs = jobs;
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.jobs.iter().any(|j| &j.id == id))
        {
            self.selected = None;
            self.runs.clear();
            self.runs_job = None;
        }
        Ok(())
    }
    pub fn seed_fixture(&mut self) {
        self.jobs = vec![
            Job {
                id: "morning-brief".into(),
                name: Some("Morning brief".into()),
                prompt: Some("Summarize today's priorities and upcoming appointments.".into()),
                enabled: Some(true),
                schedule: Some(Schedule {
                    expr: Some("0 9 * * 1-5".into()),
                    display: Some("Weekdays at 09:00".into()),
                    ..Default::default()
                }),
                deliver: Some("local".into()),
                ..Default::default()
            },
            Job {
                id: "weekly-review".into(),
                name: Some("Weekly review".into()),
                prompt: Some("Review the week and identify the next steps.".into()),
                state: Some("paused".into()),
                schedule: Some(Schedule {
                    expr: Some("0 17 * * 5".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];
        self.selected = Some("morning-brief".into());
        self.delivery = vec![DeliveryTarget {
            id: "local".into(),
            name: "This desktop".into(),
            home_target_set: true,
            home_env_var: None,
        }];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_only_editor_preserves_its_script_and_ignores_agent_model_override() {
        let job = Job {
            id: "script".into(),
            script: Some("./cleanup.sh".into()),
            no_agent: true,
            model: Some("old-model".into()),
            ..Default::default()
        };
        let editor = Editor::new(Some(job));
        let payload = editor.payload().unwrap();
        assert!(payload["updates"].get("prompt").is_none());
        assert!(payload["updates"].get("model").is_none());
        assert!(payload["updates"].get("script").is_none());
        let mut editor = Editor::new(None);
        assert!(editor.payload().is_err());
        editor.prompt.replace_text("Do the work");
        assert_eq!(editor.payload().unwrap()["prompt"], "Do the work");
    }
    #[test]
    fn blueprint_delivery_uses_desktop_and_last_delivery_target_cannot_be_unchecked() {
        let b = Blueprint {
            key: "brief".into(),
            fields: vec![BlueprintField {
                name: "deliver".into(),
                default: Some("origin".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut editor = Editor::new(None);
        editor.select_blueprint(Some(&b));
        assert_eq!(editor.payload().unwrap()["values"]["deliver"], "local");
        let mut state = Cron {
            editor: Some(Editor::new(None)),
            ..Default::default()
        };
        state.toggle_delivery("local");
        assert_eq!(state.editor.as_ref().unwrap().deliver, "local");
        state.toggle_delivery("telegram");
        state.toggle_delivery("local");
        assert_eq!(state.editor.as_ref().unwrap().deliver, "telegram");
    }
    #[test]
    fn clearing_an_agent_model_override_writes_both_axes_as_null() {
        let job = Job {
            id: "agent".into(),
            prompt: Some("Do work".into()),
            model: Some("old".into()),
            provider: Some("provider".into()),
            ..Default::default()
        };
        let mut editor = Editor::new(Some(job));
        editor.model.clear();
        editor.provider.clear();
        let p = editor.payload().unwrap();
        assert!(p["updates"]["model"].is_null());
        assert!(p["updates"]["provider"].is_null());
    }
}

#[cfg(test)]
mod operation_tests {
    use super::*;
    #[test]
    fn late_job_reads_cannot_undo_a_mutation_and_refresh_failure_keeps_success() {
        let mut state = Cron::new();
        state.fixture();
        let initial = state.load()[0].0.clone();
        let effects = state.action(Action::PauseResume("morning-brief".into()), "research");
        assert_eq!(effects[0].1.profile.as_deref(), Some("research"));
        assert!(effects[0].1.path.ends_with("/pause"));
        state.apply(&initial, Ok(json!([])));
        assert_eq!(state.jobs.len(), 2);
        let refresh = state.apply(&effects[0].0, Ok(json!({"id":"morning-brief"})));
        assert!(state.notice.is_some());
        state.apply(&refresh[0].0, Err("offline".into()));
        assert!(state.notice.is_some());
        assert!(state.error.as_ref().unwrap().contains("change succeeded"));
    }
    #[test]
    fn actions_capture_the_job_owner_and_delete_requires_explicit_confirmation() {
        let mut state = Cron::new();
        state.fixture();
        state.jobs[0].profile = Some("job-owner".into());
        assert!(state.action(Action::ConfirmDelete, "ambient").is_empty());
        state.action(Action::AskDelete("morning-brief".into()), "ambient");
        let effects = state.action(Action::ConfirmDelete, "ambient");
        assert_eq!(effects[0].1.profile.as_deref(), Some("job-owner"));
        assert!(effects[0].1.path.ends_with("morning-brief"));
        assert!(
            state.action(Action::ConfirmDelete, "ambient").is_empty(),
            "duplicate mutation is refused while in flight"
        );
        let mut state = Cron::new();
        state.fixture();
        let effects = state.action(Action::Trigger("morning-brief".into()), "research");
        assert_eq!(effects[0].1.timeout, 86400);
        assert!(state.triggering.contains("morning-brief"));
    }
    #[test]
    fn output_documents_are_not_sessions_and_scheduler_ownership_wins_over_activity() {
        let mut state = Cron::new();
        state.runs = vec![
            Run {
                session_id: "doc".into(),
                source: Some("cron_output".into()),
                ..Default::default()
            },
            Run {
                session_id: "owned".into(),
                source: Some("cron".into()),
                scheduler_owned: Some(true),
                is_active: Some(false),
                ..Default::default()
            },
            Run {
                session_id: "zombie".into(),
                source: Some("cron".into()),
                scheduler_owned: Some(false),
                is_active: Some(true),
                ..Default::default()
            },
        ];
        assert!(state.run_read_only(0));
        assert!(!state.run_read_only(1));
        assert!(state.run_read_only(2));
    }
}

#[cfg(test)]
mod overdue_tests {
    use super::*;
    #[test]
    fn next_run_overdue_respects_grace_timezone_and_paused_state() {
        let at = iso_seconds("2026-10-29T09:00:00+00:00").unwrap();
        assert_eq!(iso_seconds("2026-10-29T11:00:00+02:00"), Some(at));
        assert_eq!(iso_seconds("2026-10-29T09:00:00.123Z"), Some(at));
        let mut job = Job {
            next_run_at: Some("2026-10-29T09:00:00+00:00".into()),
            ..Default::default()
        };
        assert!(!next_run_overdue(&job, at + 900));
        assert!(next_run_overdue(&job, at + 901));
        job.state = Some("paused".into());
        assert!(!next_run_overdue(&job, at + 3600));
    }
}
