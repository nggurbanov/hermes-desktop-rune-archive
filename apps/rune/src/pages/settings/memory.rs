//! Memory layers and declared provider settings are profile-owned backend surfaces.
use crate::pages::{PageEffect, PageRequest, segment};
use rune::TextEdit;
use serde_json::{Value, json};
#[derive(Clone)]
pub struct Provider {
    pub name: String,
    pub description: String,
    pub status: String,
}
#[derive(Clone)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub description: String,
    pub group: String,
    pub kind: String,
    pub placeholder: String,
    pub options: Vec<String>,
    pub is_set: bool,
    pub editor: TextEdit,
    saved: String,
}
pub struct Memory {
    pub providers: Vec<Provider>,
    pub active: String,
    pub memory_bytes: u64,
    pub user_bytes: u64,
    pub provider: String,
    pub label: String,
    pub docs_url: String,
    pub fields: Vec<Field>,
    pub open: Option<String>,
    pub loading: bool,
    pub saving: bool,
    pub oauth_capable: bool,
    pub connected: bool,
    pub auth: String,
    pub pending: bool,
    pub detail: String,
    pub error: Option<String>,
    pub notice: Option<String>,
    sent: Vec<(String, String)>,
    starting: bool,
    polling: bool,
    clock: f64,
    next_poll: f64,
    deadline: f64,
    detached: TextEdit,
}
impl Memory {
    pub fn new() -> Self {
        Self {
            providers: vec![],
            active: String::new(),
            memory_bytes: 0,
            user_bytes: 0,
            provider: String::new(),
            label: String::new(),
            docs_url: String::new(),
            fields: vec![],
            open: None,
            loading: false,
            saving: false,
            oauth_capable: false,
            connected: false,
            auth: String::new(),
            pending: false,
            detail: String::new(),
            error: None,
            notice: None,
            sent: vec![],
            starting: false,
            polling: false,
            clock: 0.0,
            next_poll: 0.0,
            deadline: 0.0,
            detached: TextEdit::new(""),
        }
    }
    pub fn dirty(&self) -> bool {
        self.fields.iter().any(|f| f.editor.text != f.saved)
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        self.error = None;
        vec![(
            "settings.memory.status".into(),
            PageRequest::get("/api/memory"),
        )]
    }
    fn select(&mut self, provider: String) -> Vec<PageEffect> {
        if self.saving || self.pending || self.dirty() {
            self.error=Some("Save or revert provider settings and finish the connection before changing providers".into());
            return vec![];
        }
        self.provider = provider;
        self.label.clear();
        self.fields.clear();
        self.oauth_capable = false;
        self.connected = false;
        self.auth.clear();
        self.detail.clear();
        self.open = None;
        if self.provider.is_empty()
            || matches!(self.provider.as_str(), "builtin" | "default" | "none")
        {
            return vec![];
        }
        self.loading = true;
        vec![
            (
                "settings.memory.config".into(),
                PageRequest::get(format!(
                    "/api/memory/providers/{}/config",
                    segment(&self.provider)
                ))
                .query("surface", "declared"),
            ),
            (
                "settings.memory.oauth".into(),
                PageRequest::get(format!(
                    "/api/memory/providers/{}/oauth/status",
                    segment(&self.provider)
                )),
            ),
        ]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                if op == "settings.memory.oauth" {
                    self.polling = false;
                    self.oauth_capable = false;
                    if self.pending {
                        self.detail="Connection status is unavailable; retrying until the connection timeout".into();
                    }
                    return vec![];
                }
                if op == "settings.memory.save" {
                    self.saving = false;
                }
                if op == "settings.memory.start" {
                    self.pending = false;
                    self.starting = false;
                }
                self.loading = false;
                self.error = Some(e);
                return vec![];
            }
        };
        match op {
            "settings.memory.status" => {
                self.loading = false;
                if !v["providers"].is_array() {
                    self.error = Some("Invalid memory status response".into());
                    return vec![];
                }
                self.active = v["active"].as_str().unwrap_or("").into();
                self.memory_bytes = v["builtin_files"]["memory"].as_u64().unwrap_or(0);
                self.user_bytes = v["builtin_files"]["user"].as_u64().unwrap_or(0);
                self.providers = v["providers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|p| {
                        Some(Provider {
                            name: p["name"].as_str()?.into(),
                            description: p["description"].as_str().unwrap_or("").into(),
                            status: p["status"].as_str().unwrap_or("unknown").into(),
                        })
                    })
                    .collect();
                if self.provider.is_empty() && !self.active.is_empty() {
                    return self.select(self.active.clone());
                }
            }
            "settings.memory.config" => {
                self.loading = false;
                let Some(fields) = v["fields"].as_array() else {
                    self.error = Some("Invalid declared memory settings response".into());
                    return vec![];
                };
                if v["name"].as_str().is_some_and(|n| n != self.provider) {
                    return vec![];
                }
                let old = std::mem::take(&mut self.fields);
                self.label = v["label"].as_str().unwrap_or(&self.provider).into();
                self.docs_url = v["docs_url"].as_str().unwrap_or("").into();
                self.fields = fields
                    .iter()
                    .filter_map(|f| {
                        let key = f["key"].as_str()?;
                        let kind = f["kind"].as_str().unwrap_or("text");
                        let saved = if kind == "secret" {
                            String::new()
                        } else {
                            f["value"].as_str().unwrap_or("").into()
                        };
                        let editor = old
                            .iter()
                            .find(|o| o.key == key && o.editor.text != o.saved)
                            .map(|o| o.editor.clone())
                            .unwrap_or_else(|| TextEdit::new(&saved));
                        Some(Field {
                            key: key.into(),
                            label: f["label"].as_str().unwrap_or(key).into(),
                            description: f["description"].as_str().unwrap_or("").into(),
                            group: f["group"].as_str().unwrap_or("").into(),
                            kind: kind.into(),
                            placeholder: f["placeholder"].as_str().unwrap_or("").into(),
                            options: f["options"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|o| o["value"].as_str())
                                .map(str::to_owned)
                                .collect(),
                            is_set: f["is_set"].as_bool().unwrap_or(false),
                            editor,
                            saved,
                        })
                    })
                    .collect();
            }
            "settings.memory.save" => {
                self.saving = false;
                if v["ok"].as_bool() != Some(true) {
                    self.error = Some("The agent did not confirm the memory settings save".into());
                    return vec![];
                }
                for (key, value) in self.sent.drain(..) {
                    if let Some(f) = self.fields.iter_mut().find(|f| f.key == key) {
                        if f.kind == "secret" {
                            f.editor.replace_text("");
                            f.saved.clear();
                            f.is_set = true;
                        } else {
                            f.saved = value;
                        }
                    }
                }
                self.notice = Some("Memory provider settings saved".into());
                return vec![
                    (
                        "settings.memory.status".into(),
                        PageRequest::get("/api/memory"),
                    ),
                    (
                        "settings.memory.config".into(),
                        PageRequest::get(format!(
                            "/api/memory/providers/{}/config",
                            segment(&self.provider)
                        ))
                        .query("surface", "declared"),
                    ),
                ];
            }
            "settings.memory.oauth" | "settings.memory.start" => {
                self.polling = false;
                self.starting = false;
                let was_pending = self.pending;
                self.oauth_capable = true;
                self.connected = v["connected"].as_bool().unwrap_or(false);
                self.auth = v["auth"].as_str().unwrap_or("").into();
                self.detail = v["detail"].as_str().unwrap_or("").into();
                self.pending = v["state"] == "pending";
                if self.pending {
                    if !was_pending {
                        self.deadline = self.clock + 120.0;
                    }
                    self.next_poll = self.clock + 1.5;
                } else if self.connected {
                    return self.load();
                } else if v["state"] == "error" {
                    self.error = Some(if self.detail.is_empty() {
                        "Memory provider connection failed".into()
                    } else {
                        self.detail.clone()
                    });
                }
            }
            _ => {}
        }
        vec![]
    }
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        self.error = None;
        match a {
            Action::Load => self.load(),
            Action::Select(p) => self.select(p),
            Action::Value(key, value) => {
                if !self.saving {
                    if let Some(f) = self.fields.iter_mut().find(|f| f.key == key) {
                        f.editor.replace_text(value);
                    }
                }
                vec![]
            }
            Action::Revert => {
                if !self.saving {
                    for f in &mut self.fields {
                        f.editor.replace_text(&f.saved);
                    }
                }
                vec![]
            }
            Action::Save => {
                if self.saving {
                    return vec![];
                }
                let mut values = serde_json::Map::new();
                for f in &self.fields {
                    if f.editor.text == f.saved
                        || f.kind == "secret" && f.editor.text.trim().is_empty()
                    {
                        continue;
                    }
                    if f.kind == "number"
                        && f.editor.text.parse::<f64>().is_ok_and(|v| !v.is_finite())
                        || f.kind == "number" && f.editor.text.parse::<f64>().is_err()
                    {
                        self.error = Some(format!("{} must be a number", f.label));
                        return vec![];
                    }
                    if f.kind == "json" && serde_json::from_str::<Value>(&f.editor.text).is_err() {
                        self.error = Some(format!("{} must be valid JSON", f.label));
                        return vec![];
                    }
                    values.insert(f.key.clone(), json!(f.editor.text));
                }
                if values.is_empty() {
                    return vec![];
                }
                self.sent = values
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap().into()))
                    .collect();
                self.saving = true;
                vec![(
                    "settings.memory.save".into(),
                    PageRequest::put(
                        format!("/api/memory/providers/{}/config", segment(&self.provider)),
                        json!({"values":values}),
                    )
                    .query("surface", "declared"),
                )]
            }
            Action::Connect => {
                if self.pending || !self.oauth_capable {
                    return vec![];
                }
                self.pending = true;
                self.starting = true;
                self.deadline = self.clock + 120.0;
                self.next_poll = self.clock + 1.5;
                vec![(
                    "settings.memory.start".into(),
                    PageRequest::post(
                        format!(
                            "/api/memory/providers/{}/oauth/start",
                            segment(&self.provider)
                        ),
                        json!({}),
                    ),
                )]
            }
            Action::Cancel => {
                self.starting = false;
                self.polling = false;
                self.pending = false;
                self.detail.clear();
                vec![]
            }
        }
    }
    pub fn poll(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        if self.pending && now >= self.deadline {
            self.pending = false;
            self.error = Some("Memory provider connection timed out; retry the connection".into());
        }
        if self.pending && !self.starting && !self.polling && now >= self.next_poll {
            self.polling = true;
            self.next_poll = now + 1.5;
            return vec![(
                "settings.memory.oauth".into(),
                PageRequest::get(format!(
                    "/api/memory/providers/{}/oauth/status",
                    segment(&self.provider)
                )),
            )];
        }
        vec![]
    }
    pub fn next_wake(&self) -> Option<std::time::Duration> {
        self.pending
            .then(|| std::time::Duration::from_secs_f64((self.next_poll - self.clock).max(0.01)))
    }
    pub fn edit(&mut self, key: &str) -> &mut TextEdit {
        self.fields
            .iter_mut()
            .find(|f| f.key == key)
            .map(|f| &mut f.editor)
            .unwrap_or(&mut self.detached)
    }
}
#[derive(Clone)]
pub enum Action {
    Load,
    Select(String),
    Value(String, String),
    Save,
    Revert,
    Connect,
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Memory {
        let mut s = Memory::new();
        s.provider = "sample".into();
        s.apply("settings.memory.config",Ok(json!({"name":"sample","label":"Sample","fields":[{"key":"key","kind":"secret","is_set":true,"value":"masked-preview"},{"key":"url","kind":"text","value":"old"},{"key":"options","kind":"json","value":"{}"}]})));
        s
    }
    #[test]
    fn provider_writes_only_touched_values_and_never_preview_secrets() {
        let mut s = state();
        assert_eq!(s.fields[0].editor.text, "");
        s.edit("url").replace_text("new");
        let e = s.act(Action::Save);
        assert_eq!(e[0].1.body, json!({"values":{"url":"new"}}));
        s.apply("settings.memory.save", Ok(json!({"ok":true})));
        s.edit("key").replace_text("replacement");
        let e = s.act(Action::Save);
        assert_eq!(e[0].1.body, json!({"values":{"key":"replacement"}}));
        s.apply("settings.memory.save", Ok(json!({"ok":true})));
        assert_eq!(s.edit("key").text, "");
        assert!(!s.dirty());
    }
    #[test]
    fn invalid_provider_json_stays_draft_and_never_posts() {
        let mut s = state();
        s.edit("options").replace_text("{broken");
        assert!(s.act(Action::Save).is_empty());
        assert!(s.dirty());
        assert!(!s.saving);
        s.apply(
            "settings.memory.config",
            Ok(json!({"name":"sample","fields":[{"key":"options","kind":"json","value":"{}"}]})),
        );
        assert_eq!(s.edit("options").text, "{broken");
    }
    #[test]
    fn connection_poll_waits_for_start_and_expires_without_success_claim() {
        let mut s = state();
        s.oauth_capable = true;
        s.poll(10.0);
        assert_eq!(
            s.act(Action::Connect)[0].1.path,
            "/api/memory/providers/sample/oauth/start"
        );
        assert!(s.poll(12.0).is_empty());
        s.apply(
            "settings.memory.start",
            Ok(json!({"state":"pending","connected":false})),
        );
        assert_eq!(
            s.poll(14.0)[0].1.path,
            "/api/memory/providers/sample/oauth/status"
        );
        assert!(s.poll(16.0).is_empty());
        s.apply("settings.memory.oauth", Err("Transient".into()));
        assert!(!s.poll(17.0).is_empty());
        s.poll(131.0);
        assert!(!s.pending);
        assert!(!s.connected);
        assert!(s.error.is_some());
    }
}
