//! Existing external-agent provider and OAuth contracts; keys remain on that agent.
use crate::pages::{PageEffect, PageRequest, segment};
use rune::TextEdit;
use serde_json::{Value, json};
#[derive(Clone)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub flow: String,
    pub logged_in: bool,
    pub disconnectable: bool,
    pub command: String,
}
#[derive(Clone)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub current: bool,
    pub has_key: bool,
    pub api_mode: String,
    pub context_length: String,
    pub discover: bool,
}
pub struct Login {
    pub provider: String,
    pub session: String,
    pub url: String,
    pub user_code: String,
    pub code: TextEdit,
    pub next_poll: f64,
    pub interval: f64,
    pub expires: f64,
    pub polling: bool,
}
pub struct Providers {
    pub accounts: Vec<Account>,
    pub endpoints: Vec<Endpoint>,
    pub loading: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub confirm: Option<(String, String)>,
    pub login: Option<Login>,
    pub editing: Option<String>,
    pub name: TextEdit,
    pub url: TextEdit,
    pub model: TextEdit,
    pub api_key: TextEdit,
    pub api_mode: TextEdit,
    pub context_length: TextEdit,
    pub discover: bool,
    pub make_default: bool,
    pub clock: f64,
    starting_provider: Option<String>,
    draft_baseline: Value,
}
impl Default for Providers {
    fn default() -> Self {
        Self::new()
    }
}
impl Providers {
    pub fn new() -> Self {
        Self {
            accounts: vec![],
            endpoints: vec![],
            loading: false,
            busy: false,
            error: None,
            notice: None,
            confirm: None,
            login: None,
            editing: None,
            name: TextEdit::new(""),
            url: TextEdit::new(""),
            model: TextEdit::new(""),
            api_key: TextEdit::new(""),
            api_mode: TextEdit::new(""),
            context_length: TextEdit::new(""),
            discover: true,
            make_default: false,
            clock: 0.0,
            starting_provider: None,
            draft_baseline: json!({"name":"","url":"","model":"","api_key":"","api_mode":"","context_length":"","discover":true,"default":false}),
        }
    }
    fn draft(&self) -> Value {
        json!({"name":self.name.text,"url":self.url.text,"model":self.model.text,"api_key":self.api_key.text,"api_mode":self.api_mode.text,"context_length":self.context_length.text,"discover":self.discover,"default":self.make_default})
    }
    pub fn dirty(&self) -> bool {
        self.draft() != self.draft_baseline
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        self.error = None;
        vec![
            (
                "settings.providers.accounts".into(),
                PageRequest::get("/api/providers/oauth"),
            ),
            (
                "settings.providers.endpoints".into(),
                PageRequest::get("/api/providers/custom-endpoints"),
            ),
        ]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                if !matches!(
                    op,
                    "settings.providers.accounts"
                        | "settings.providers.endpoints"
                        | "settings.providers.poll"
                        | "settings.providers.discard-start"
                ) {
                    self.busy = false;
                }
                self.loading = false;
                if let Some(login) = &mut self.login {
                    login.polling = false;
                    login.next_poll = self.clock + 5.0;
                }
                self.error = Some(e);
                return vec![];
            }
        };
        match op {
            "settings.providers.accounts" => {
                self.loading = false;
                if !value["providers"].is_array() {
                    self.error = Some("Invalid provider account response".into());
                    return vec![];
                }
                self.accounts = value["providers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| {
                        Some(Account {
                            id: v["id"].as_str()?.into(),
                            name: v["name"].as_str().unwrap_or("").into(),
                            flow: v["flow"].as_str().unwrap_or("external").into(),
                            logged_in: v["status"]["logged_in"].as_bool().unwrap_or(false),
                            disconnectable: v["disconnectable"].as_bool().unwrap_or(true),
                            command: v["cli_command"].as_str().unwrap_or("").into(),
                        })
                    })
                    .collect();
            }
            "settings.providers.endpoints" => {
                self.loading = false;
                if !value["endpoints"].is_array() {
                    self.error = Some("Invalid custom endpoint response".into());
                    return vec![];
                }
                self.endpoints = value["endpoints"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| {
                        Some(Endpoint {
                            id: v["id"].as_str()?.into(),
                            name: v["name"].as_str().unwrap_or("").into(),
                            base_url: v["base_url"].as_str().unwrap_or("").into(),
                            model: v["model"].as_str().unwrap_or("").into(),
                            current: v["is_current"].as_bool().unwrap_or(false),
                            has_key: v["has_api_key"].as_bool().unwrap_or(false),
                            api_mode: v["api_mode"].as_str().unwrap_or("").into(),
                            context_length: v["context_length"]
                                .as_u64()
                                .map(|n| n.to_string())
                                .unwrap_or_default(),
                            discover: v["discover_models"].as_bool().unwrap_or(true),
                        })
                    })
                    .collect();
            }
            "settings.providers.save"
            | "settings.providers.delete"
            | "settings.providers.activate"
            | "settings.providers.disconnect" => {
                self.busy = false;
                if value.get("ok").and_then(Value::as_bool) != Some(true)
                    && !value.get("endpoints").is_some_and(Value::is_array)
                {
                    self.error = Some("The agent refused the provider change".into());
                    return vec![];
                }
                self.api_key.replace_text("");
                self.draft_baseline = self.draft();
                self.notice = Some("Provider settings saved".into());
                let mut effects = self.load();
                effects.push(("settings.config".into(), PageRequest::get("/api/config")));
                return effects;
            }
            "settings.providers.validate" => {
                self.busy = false;
                if value["ok"].as_bool() == Some(true) && value["reachable"].as_bool() == Some(true)
                {
                    if let Some(url) = value["resolved_base_url"].as_str() {
                        self.url.replace_text(url);
                    }
                    let models = value["models"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .take(30)
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.notice = Some(if models.is_empty() {
                        "Endpoint is reachable".into()
                    } else {
                        format!("Available models: {models}")
                    });
                } else {
                    self.error = Some(
                        value["message"]
                            .as_str()
                            .unwrap_or("Endpoint validation failed")
                            .into(),
                    );
                }
            }
            "settings.providers.start" => {
                self.busy = false;
                let Some(session) = value["session_id"].as_str() else {
                    self.error = Some("The agent did not return an OAuth session".into());
                    return vec![];
                };
                let provider = self.starting_provider.take().unwrap_or_default();
                let url = value["auth_url"]
                    .as_str()
                    .or_else(|| value["verification_url"].as_str())
                    .unwrap_or("");
                let valid = url::Url::parse(url).is_ok_and(|u| {
                    matches!(u.scheme(), "https" | "http")
                        && u.username().is_empty()
                        && u.password().is_none()
                });
                if !valid {
                    self.error = Some("The agent returned an invalid sign-in URL".into());
                    return vec![(
                        "settings.providers.discard-start".into(),
                        PageRequest::delete(
                            format!("/api/providers/oauth/sessions/{}", segment(session)),
                            Value::Null,
                        ),
                    )];
                }
                let interval = value["poll_interval"]
                    .as_f64()
                    .unwrap_or(3.0)
                    .clamp(1.0, 30.0);
                self.login = Some(Login {
                    provider,
                    session: session.into(),
                    url: url.into(),
                    user_code: value["user_code"].as_str().unwrap_or("").into(),
                    code: TextEdit::new(""),
                    next_poll: self.clock + interval,
                    interval,
                    expires: self.clock
                        + value["expires_in"]
                            .as_f64()
                            .unwrap_or(600.0)
                            .clamp(10.0, 3600.0),
                    polling: false,
                });
            }
            "settings.providers.poll" | "settings.providers.submit" => {
                if op == "settings.providers.submit" {
                    self.busy = false;
                }
                if let Some(login) = &mut self.login {
                    login.polling = false;
                    login.next_poll = self.clock + login.interval;
                }
                match value["status"].as_str() {
                    Some("approved") => {
                        self.login = None;
                        self.notice = Some("Account connected".into());
                        let mut requests = self.load();
                        requests.push(("settings.config".into(), PageRequest::get("/api/config")));
                        return requests;
                    }
                    Some("error" | "expired" | "denied" | "cancelled") => {
                        self.login = None;
                        self.error = Some(
                            value["error_message"]
                                .as_str()
                                .or_else(|| value["message"].as_str())
                                .unwrap_or("Sign-in did not complete")
                                .into(),
                        );
                    }
                    _ => {}
                }
            }
            "settings.providers.cancel" => {
                self.busy = false;
                self.login = None;
                self.notice = Some("Sign-in cancelled".into());
            }
            _ => {}
        }
        vec![]
    }
    pub fn poll(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        if self.busy {
            return vec![];
        }
        if let Some(login) = &self.login {
            if now >= login.expires {
                self.error = Some("Sign-in expired. Start again.".into());
                return self.act(Action::Cancel);
            }
            if !login.polling && now >= login.next_poll {
                return self.act(Action::Poll);
            }
        }
        vec![]
    }
    pub fn next_wake(&self) -> Option<std::time::Duration> {
        self.login
            .as_ref()
            .filter(|l| !l.polling)
            .map(|l| std::time::Duration::from_secs_f64((l.next_poll - self.clock).max(0.01)))
    }
    pub fn act(&mut self, action: Action) -> Vec<PageEffect> {
        if self.busy && !matches!(action, Action::Cancel | Action::Load) {
            return vec![];
        }
        self.error = None;
        match action {
            Action::Load => self.load(),
            Action::Revert => {
                let v = self.draft_baseline.clone();
                self.name.replace_text(v["name"].as_str().unwrap_or(""));
                self.url.replace_text(v["url"].as_str().unwrap_or(""));
                self.model.replace_text(v["model"].as_str().unwrap_or(""));
                self.api_key.replace_text("");
                self.api_mode
                    .replace_text(v["api_mode"].as_str().unwrap_or(""));
                self.context_length
                    .replace_text(v["context_length"].as_str().unwrap_or(""));
                self.discover = v["discover"].as_bool().unwrap_or(true);
                self.make_default = v["default"].as_bool().unwrap_or(false);
                vec![]
            }
            Action::New => {
                self.editing = None;
                self.name.replace_text("");
                self.url.replace_text("");
                self.model.replace_text("");
                self.api_key.replace_text("");
                self.api_mode.replace_text("");
                self.context_length.replace_text("");
                self.discover = true;
                self.make_default = false;
                self.draft_baseline = self.draft();
                vec![]
            }
            Action::Edit(id) => {
                if let Some(e) = self.endpoints.iter().find(|e| e.id == id) {
                    self.editing = Some(id);
                    self.name.replace_text(&e.name);
                    self.url.replace_text(&e.base_url);
                    self.model.replace_text(&e.model);
                    self.api_mode.replace_text(&e.api_mode);
                    self.context_length.replace_text(&e.context_length);
                    self.api_key.replace_text("");
                    self.discover = e.discover;
                    self.make_default = e.current;
                }
                self.draft_baseline = self.draft();
                vec![]
            }
            Action::Save | Action::Validate => {
                let validate = matches!(action, Action::Validate);
                let url = self.url.text.trim();
                if !url::Url::parse(url).is_ok_and(|u| {
                    matches!(u.scheme(), "http" | "https")
                        && u.username().is_empty()
                        && u.password().is_none()
                }) {
                    self.error =
                        Some("Enter a valid HTTP endpoint without embedded credentials".into());
                    return vec![];
                }
                if !validate
                    && (self.name.text.trim().is_empty() || self.model.text.trim().is_empty())
                {
                    self.error = Some("Name and model are required".into());
                    return vec![];
                }
                let mut body = json!({"name":self.name.text.trim(),"base_url":url,"model":self.model.text.trim(),"discover_models":self.discover,"make_default":self.make_default});
                if let Some(id) = &self.editing {
                    body["id"] = json!(id);
                }
                if !self.api_key.text.is_empty() {
                    body["api_key"] = json!(self.api_key.text);
                }
                if !self.api_mode.text.trim().is_empty() {
                    body["api_mode"] = json!(self.api_mode.text.trim());
                }
                if !self.context_length.text.trim().is_empty() {
                    match self.context_length.text.trim().parse::<u64>() {
                        Ok(n) => body["context_length"] = json!(n),
                        Err(_) => {
                            self.error =
                                Some("Context length must be a nonnegative integer".into());
                            return vec![];
                        }
                    }
                }
                self.busy = true;
                vec![(
                    if validate {
                        "settings.providers.validate"
                    } else {
                        "settings.providers.save"
                    }
                    .into(),
                    PageRequest::post(
                        if validate {
                            "/api/providers/custom-endpoints/validate"
                        } else {
                            "/api/providers/custom-endpoints"
                        },
                        body,
                    ),
                )]
            }
            Action::Delete(id) => {
                self.confirm = Some(("delete".into(), id));
                vec![]
            }
            Action::Disconnect(id) => {
                self.confirm = Some(("disconnect".into(), id));
                vec![]
            }
            Action::DismissConfirm => {
                self.confirm = None;
                vec![]
            }
            Action::Confirm => {
                if let Some((kind, id)) = self.confirm.take() {
                    self.busy = true;
                    let (op, path) = if kind == "delete" {
                        (
                            "settings.providers.delete",
                            format!("/api/providers/custom-endpoints/{}", segment(&id)),
                        )
                    } else {
                        (
                            "settings.providers.disconnect",
                            format!("/api/providers/oauth/{}", segment(&id)),
                        )
                    };
                    vec![(op.into(), PageRequest::delete(path, Value::Null))]
                } else {
                    vec![]
                }
            }
            Action::Activate(id) => {
                self.busy = true;
                vec![(
                    "settings.providers.activate".into(),
                    PageRequest::post(
                        format!("/api/providers/custom-endpoints/{}/activate", segment(&id)),
                        json!({}),
                    ),
                )]
            }
            Action::Start(id) => {
                if self.login.is_some() {
                    self.error = Some("Finish or cancel the current sign-in first".into());
                    return vec![];
                }
                self.busy = true;
                self.starting_provider = Some(id.clone());
                vec![(
                    "settings.providers.start".into(),
                    PageRequest::post(
                        format!("/api/providers/oauth/{}/start", segment(&id)),
                        json!({}),
                    ),
                )]
            }
            Action::Poll => {
                if let Some(login) = &mut self.login {
                    login.polling = true;
                    vec![(
                        "settings.providers.poll".into(),
                        PageRequest::get(format!(
                            "/api/providers/oauth/{}/poll/{}",
                            segment(&login.provider),
                            segment(&login.session)
                        )),
                    )]
                } else {
                    vec![]
                }
            }
            Action::Submit => {
                if let Some(login) = &self.login {
                    self.busy = true;
                    vec![(
                        "settings.providers.submit".into(),
                        PageRequest::post(
                            format!("/api/providers/oauth/{}/submit", segment(&login.provider)),
                            json!({"session_id":login.session,"code":login.code.text}),
                        ),
                    )]
                } else {
                    vec![]
                }
            }
            Action::Cancel => {
                if let Some(login) = self.login.take() {
                    self.busy = true;
                    vec![(
                        "settings.providers.cancel".into(),
                        PageRequest::delete(
                            format!("/api/providers/oauth/sessions/{}", segment(&login.session)),
                            Value::Null,
                        ),
                    )]
                } else {
                    vec![]
                }
            }
        }
    }
}
#[derive(Clone)]
pub enum Action {
    Load,
    Revert,
    New,
    Edit(String),
    Save,
    Validate,
    Delete(String),
    Activate(String),
    Disconnect(String),
    Confirm,
    DismissConfirm,
    Start(String),
    Poll,
    Submit,
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_edits_preserve_the_saved_key_unless_the_user_enters_a_replacement() {
        let mut p = Providers::new();
        p.editing = Some("saved/id".into());
        p.name.replace_text("Lab");
        p.url.replace_text("http://example.invalid/v1");
        p.model.replace_text("sample");
        let effects = p.act(Action::Save);
        assert!(effects[0].1.body.get("api_key").is_none());
        assert_eq!(effects[0].1.body["id"], "saved/id");
        p.apply("settings.providers.save", Err("fixture rejection".into()));
        p.api_key.replace_text("new key");
        let effects = p.act(Action::Save);
        assert_eq!(effects[0].1.body["api_key"], "new key");
        assert!(
            p.apply("settings.providers.save", Ok(json!({"ok":false})))
                .is_empty()
        );
        assert!(p.error.is_some());
        assert_eq!(p.api_key.text, "new key");
    }
    #[test]
    fn oauth_uses_the_returned_session_and_checks_status_before_claiming_connection() {
        let mut p = Providers::new();
        let start = p.act(Action::Start("provider/id".into()));
        assert_eq!(start[0].1.path, "/api/providers/oauth/provider%2Fid/start");
        p.apply("settings.providers.start",Ok(json!({"flow":"device_code","session_id":"session/id","verification_url":"https://example.invalid/activate","user_code":"ABCD","poll_interval":3,"expires_in":120})));
        assert_eq!(p.login.as_ref().unwrap().user_code, "ABCD");
        assert!(p.poll(2.0).is_empty());
        let poll = p.poll(3.0);
        assert_eq!(
            poll[0].1.path,
            "/api/providers/oauth/provider%2Fid/poll/session%2Fid"
        );
        assert!(p.poll(4.0).is_empty());
        assert!(
            p.apply("settings.providers.poll", Ok(json!({"status":"pending"})))
                .is_empty()
        );
        assert!(p.login.is_some());
        let done = p.apply("settings.providers.poll", Ok(json!({"status":"approved"})));
        assert!(p.login.is_none());
        assert!(done.iter().any(|(op, _)| op == "settings.config"));
    }
    #[test]
    fn cancelling_sign_in_targets_only_its_exact_backend_session() {
        let mut p = Providers::new();
        p.act(Action::Start("sample".into()));
        p.apply("settings.providers.start",Ok(json!({"flow":"pkce","session_id":"captured/id","auth_url":"https://example.invalid/auth","expires_in":120})));
        let cancel = p.act(Action::Cancel);
        assert!(p.login.is_none());
        assert_eq!(
            cancel[0].1.path,
            "/api/providers/oauth/sessions/captured%2Fid"
        );
        assert!(matches!(
            cancel[0].1.method,
            crate::pages::PageMethod::Delete
        ));
    }
}

#[cfg(test)]
mod independent_read_tests {
    use super::*;
    #[test]
    fn catalog_or_oauth_poll_cannot_unlock_inflight_endpoint_write() {
        let mut p = Providers::new();
        p.name.replace_text("Local");
        p.url.replace_text("https://example.invalid/v1");
        p.model.replace_text("model");
        assert!(!p.act(Action::Save).is_empty());
        p.apply("settings.providers.accounts", Err("Read failed".into()));
        assert!(p.busy);
        p.apply("settings.providers.poll", Ok(json!({"status":"pending"})));
        assert!(p.busy);
        assert!(p.act(Action::Save).is_empty());
    }
    #[test]
    fn invalid_oauth_url_is_rejected_and_session_is_cleaned_up() {
        let mut p = Providers::new();
        p.act(Action::Start("sample".into()));
        let e = p.apply(
            "settings.providers.start",
            Ok(json!({"session_id":"session","auth_url":"file:///tmp/invalid"})),
        );
        assert!(p.login.is_none());
        assert_eq!(e[0].1.path, "/api/providers/oauth/sessions/session");
        assert!(p.error.is_some());
    }
}
