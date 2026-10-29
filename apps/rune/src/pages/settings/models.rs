//! Main/auxiliary assignment uses the guarded model API, not generic config writes.
use crate::pages::{PageEffect, PageRequest};
use rune::TextEdit;
use serde_json::{Value, json};
#[derive(Clone)]
pub struct Provider {
    pub id: String,
    pub label: String,
    pub models: Vec<String>,
    pub url: String,
    pub available: bool,
}
#[derive(Clone)]
pub struct Task {
    pub id: String,
    pub label: String,
    pub hint: String,
    pub provider: TextEdit,
    pub model: TextEdit,
    pub effort: TextEdit,
    pub url: TextEdit,
    original: Value,
}
impl Task {
    pub fn dirty(&self) -> bool {
        self.provider.text != self.original["provider"].as_str().unwrap_or("auto")
            || self.model.text != self.original["model"].as_str().unwrap_or("")
            || self.effort.text != self.original["reasoning_effort"].as_str().unwrap_or("")
            || self.url.text != self.original["base_url"].as_str().unwrap_or("")
    }
}
pub struct Models {
    pub providers: Vec<Provider>,
    pub tasks: Vec<Task>,
    pub provider: TextEdit,
    pub model: TextEdit,
    pub url: TextEdit,
    pub current: String,
    pub loading: bool,
    pub busy: bool,
    pub open: Option<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub confirm: Option<Value>,
    pub confirm_message: String,
    last: Option<Value>,
    edited: bool,
    acknowledged_task: Option<String>,
    detached: TextEdit,
}
impl Default for Models {
    fn default() -> Self {
        Self::new()
    }
}
impl Models {
    pub fn new() -> Self {
        Self {
            providers: vec![],
            tasks: vec![],
            provider: TextEdit::new(""),
            model: TextEdit::new(""),
            url: TextEdit::new(""),
            current: String::new(),
            loading: false,
            busy: false,
            open: None,
            error: None,
            notice: None,
            confirm: None,
            confirm_message: String::new(),
            last: None,
            edited: false,
            acknowledged_task: None,
            detached: TextEdit::new(""),
        }
    }
    pub fn task_edit(&mut self, id: &str, field: &str) -> &mut TextEdit {
        match self.tasks.iter_mut().find(|t| t.id == id) {
            Some(t) => match field {
                "provider" => &mut t.provider,
                "model" => &mut t.model,
                "effort" => &mut t.effort,
                _ => &mut t.url,
            },
            None => &mut self.detached,
        }
    }
    pub fn dirty(&self) -> bool {
        self.edited || self.tasks.iter().any(Task::dirty)
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        vec![
            (
                "settings.models.options".into(),
                PageRequest::get("/api/model/options").query("explicit_only", "1"),
            ),
            (
                "settings.models.auxiliary".into(),
                PageRequest::get("/api/model/auxiliary"),
            ),
        ]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                self.loading = false;
                if op == "settings.models.apply" {
                    self.busy = false;
                }
                self.error = Some(e);
                return vec![];
            }
        };
        match op {
            "settings.models.options" => {
                self.loading = false;
                if !value["providers"].is_array() {
                    self.error = Some("Invalid model catalog response".into());
                    return vec![];
                }
                self.providers = value["providers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| {
                        Some(Provider {
                            id: v["slug"].as_str()?.into(),
                            label: v["name"].as_str().unwrap_or("").into(),
                            models: v["models"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect(),
                            url: v["api_url"].as_str().unwrap_or("").into(),
                            available: v["authenticated"].as_bool() != Some(false),
                        })
                    })
                    .collect();
            }
            "settings.models.auxiliary" => {
                self.loading = false;
                if !value["tasks"].is_array() || !value["main"].is_object() {
                    self.error = Some("Invalid auxiliary models response".into());
                    return vec![];
                }
                let main = &value["main"];
                let provider = main["provider"].as_str().unwrap_or("");
                let model = main["model"].as_str().unwrap_or("");
                self.current = format!("{provider} · {model}");
                if !self.edited {
                    self.provider.replace_text(provider);
                    self.model.replace_text(model);
                    self.url.replace_text(
                        self.providers
                            .iter()
                            .find(|p| p.id == provider)
                            .map(|p| p.url.as_str())
                            .unwrap_or(""),
                    );
                }
                let previous = std::mem::take(&mut self.tasks);
                let acknowledged = self.acknowledged_task.take();
                self.tasks = value["tasks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| {
                        let id = v["task"].as_str()?;
                        if let Some(draft) = previous.iter().find(|t| {
                            t.id == id && t.dirty() && acknowledged.as_deref() != Some(id)
                        }) {
                            return Some(draft.clone());
                        }
                        Some(Task {
                            id: v["task"].as_str()?.into(),
                            label: v["label"]
                                .as_str()
                                .or_else(|| v["task"].as_str())
                                .unwrap_or("")
                                .into(),
                            hint: v["hint"].as_str().unwrap_or("").into(),
                            provider: TextEdit::new(v["provider"].as_str().unwrap_or("auto")),
                            model: TextEdit::new(v["model"].as_str().unwrap_or("")),
                            effort: TextEdit::new(v["reasoning_effort"].as_str().unwrap_or("")),
                            url: TextEdit::new(v["base_url"].as_str().unwrap_or("")),
                            original: v.clone(),
                        })
                    })
                    .collect();
            }
            "settings.models.apply" => {
                self.busy = false;
                if value["confirm_required"].as_bool() == Some(true) {
                    self.confirm = self.last.take();
                    self.confirm_message = value["confirm_message"]
                        .as_str()
                        .unwrap_or("Apply this model selection?")
                        .into();
                    return vec![];
                }
                if value["ok"].as_bool() != Some(true) {
                    self.error = Some("The agent did not confirm the model assignment".into());
                    return vec![];
                }
                if self.last.as_ref().is_some_and(|v| v["scope"] == "main") {
                    self.edited = false;
                }
                self.acknowledged_task = self
                    .last
                    .take()
                    .and_then(|v| v["task"].as_str().map(str::to_owned));
                self.notice = value["notice"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| Some("Model assignment saved".into()));
                let stale = value["stale_aux"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v["task"].as_str())
                    .collect::<Vec<_>>();
                if !stale.is_empty() {
                    self.notice = Some(format!(
                        "Model saved. These auxiliary tasks still use their pinned routes: {}",
                        stale.join(", ")
                    ));
                }
                let mut requests = self.load();
                requests.push(("settings.config".into(), PageRequest::get("/api/config")));
                return requests;
            }
            _ => {}
        }
        vec![]
    }
    pub fn act(&mut self, action: Action) -> Vec<PageEffect> {
        if self.busy {
            return vec![];
        }
        self.error = None;
        match action {
            Action::Load => self.load(),
            Action::Revert => {
                self.edited = false;
                self.confirm = None;
                self.tasks.clear();
                self.load()
            }
            Action::Provider(provider) => {
                self.provider.replace_text(&provider);
                self.model.replace_text("");
                self.url.replace_text(
                    self.providers
                        .iter()
                        .find(|p| p.id == provider)
                        .map(|p| p.url.as_str())
                        .unwrap_or(""),
                );
                self.open = None;
                self.edited = true;
                vec![]
            }
            Action::MainModel(model) => {
                self.model.replace_text(model);
                self.open = None;
                self.edited = true;
                vec![]
            }
            Action::Edited => {
                self.edited = true;
                vec![]
            }
            Action::TaskEdited => vec![],
            Action::Main => {
                if self.provider.text.trim().is_empty() || self.model.text.trim().is_empty() {
                    self.error = Some("Choose a provider and model".into());
                    return vec![];
                }
                if self
                    .providers
                    .iter()
                    .find(|p| p.id == self.provider.text)
                    .is_some_and(|p| !p.available)
                {
                    self.error =
                        Some("Connect this provider in Providers & endpoints first".into());
                    return vec![];
                }
                let mut body = json!({"scope":"main","provider":self.provider.text.trim(),"model":self.model.text.trim()});
                if !self.url.text.trim().is_empty() {
                    body["base_url"] = json!(self.url.text.trim());
                }
                self.send(body)
            }
            Action::Task(id, reset) => {
                let Some(task) = self.tasks.iter().find(|t| t.id == id) else {
                    return vec![];
                };
                let provider = if reset {
                    "auto"
                } else {
                    task.provider.text.trim()
                };
                let model = if reset { "" } else { task.model.text.trim() };
                let effort = task.effort.text.trim();
                if !effort.is_empty()
                    && !matches!(
                        effort,
                        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
                    )
                {
                    self.error = Some(
                        "Choose a supported reasoning effort or leave it blank to inherit".into(),
                    );
                    return vec![];
                }
                let mut body = json!({"scope":"auxiliary","task":id,"provider":if provider.is_empty(){"auto"}else{provider},"model":model,"reasoning_effort":if reset||effort.is_empty(){Value::Null}else{json!(effort)}});
                if !reset && !task.url.text.trim().is_empty() {
                    body["base_url"] = json!(task.url.text.trim());
                }
                self.send(body)
            }
            Action::Confirm => {
                if let Some(mut body) = self.confirm.take() {
                    body["confirm_expensive_model"] = json!(true);
                    self.send(body)
                } else {
                    vec![]
                }
            }
            Action::Cancel => {
                self.confirm = None;
                vec![]
            }
        }
    }
    fn send(&mut self, body: Value) -> Vec<PageEffect> {
        self.busy = true;
        self.last = Some(body.clone());
        vec![(
            "settings.models.apply".into(),
            PageRequest::post("/api/model/set", body),
        )]
    }
}
#[derive(Clone)]
pub enum Action {
    Load,
    Revert,
    Provider(String),
    MainModel(String),
    Edited,
    TaskEdited,
    Main,
    Task(String, bool),
    Confirm,
    Cancel,
}

/// These assignments need the API's provider validation and confirmation guard.
pub fn assignment_key(key: &str) -> bool {
    matches!(
        key,
        "model" | "model.default" | "model.provider" | "model.base_url" | "model.api_key"
    ) || key.starts_with("auxiliary.")
        && (key.ends_with(".provider")
            || key.ends_with(".model")
            || key.ends_with(".base_url")
            || key.ends_with(".reasoning_effort"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn models() -> Models {
        let mut m = Models::new();
        m.apply("settings.models.options",Ok(json!({"providers":[{"slug":"sample","name":"Sample","authenticated":true,"models":["a","b"]}]})));
        m.apply("settings.models.auxiliary",Ok(json!({"main":{"provider":"sample","model":"a"},"tasks":[{"task":"plugin.task","label":"Plugin task","provider":"auto","model":"","reasoning_effort":null,"base_url":""},{"task":"compression","provider":"sample","model":"b","reasoning_effort":"high","base_url":""}]})));
        m
    }
    #[test]
    fn main_assignment_waits_for_guard_and_reuses_exact_body() {
        let mut m = models();
        m.model.replace_text("b");
        let effects = m.act(Action::Main);
        assert_eq!(effects[0].1.path, "/api/model/set");
        let original = effects[0].1.body.clone();
        assert!(
            m.apply(
                "settings.models.apply",
                Ok(json!({"confirm_required":true,"confirm_message":"Training tier"}))
            )
            .is_empty()
        );
        assert!(m.notice.is_none());
        assert_eq!(m.confirm.as_ref(), Some(&original));
        let confirmed = m.act(Action::Confirm);
        let mut expected = original;
        expected["confirm_expensive_model"] = json!(true);
        assert_eq!(confirmed[0].1.body, expected);
        assert!(
            m.apply("settings.models.apply", Ok(json!({"ok":true})))
                .iter()
                .any(|(op, _)| op == "settings.models.auxiliary")
        );
    }
    #[test]
    fn auxiliary_readback_preserves_other_dirty_plugin_routes() {
        let mut m = models();
        m.tasks[0].model.replace_text("plugin-route");
        m.tasks[1].effort.replace_text("");
        let request = m.act(Action::Task("compression".into(), false));
        assert_eq!(request[0].1.body["reasoning_effort"], Value::Null);
        m.apply("settings.models.apply", Ok(json!({"ok":true})));
        m.apply("settings.models.auxiliary",Ok(json!({"main":{"provider":"sample","model":"a"},"tasks":[{"task":"plugin.task","provider":"auto","model":"","base_url":""},{"task":"compression","provider":"sample","model":"b","reasoning_effort":null,"base_url":""}]})));
        assert_eq!(m.tasks[0].model.text, "plugin-route");
        assert!(m.tasks[0].dirty());
        assert!(!m.tasks[1].dirty());
        let reset = m.act(Action::Task("plugin.task".into(), true));
        assert_eq!(
            reset[0].1.body,
            json!({"scope":"auxiliary","task":"plugin.task","provider":"auto","model":"","reasoning_effort":null})
        );
    }
    #[test]
    fn read_errors_do_not_unlock_an_inflight_assignment() {
        let mut m = models();
        m.act(Action::Main);
        m.apply("settings.models.options", Err("Unavailable".into()));
        assert!(m.busy);
        assert!(m.act(Action::Main).is_empty());
    }
}
