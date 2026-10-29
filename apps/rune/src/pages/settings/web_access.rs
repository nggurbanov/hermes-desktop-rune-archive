//! Web access distinguishes saved configuration from what the owner serves now.
use crate::pages::{PageEffect, PageRequest};
use rune::TextEdit;
use serde_json::{Value, json};
pub struct WebAccess {
    pub enabled: bool,
    pub mode: String,
    pub draft_mode: String,
    pub public_url: TextEdit,
    pub port: u16,
    pub lan: String,
    pub signed_in: bool,
    pub gateway_state: String,
    pub gateway_url: String,
    pub gateway_reason: String,
    pub loading: bool,
    pub busy: bool,
    pub waiting: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    saved_public: String,
    clock: f64,
    next_poll: f64,
    deadline: f64,
    polling: bool,
    restart_pid: Option<u64>,
}
impl WebAccess {
    pub fn new() -> Self {
        Self {
            enabled: false,
            mode: String::new(),
            draft_mode: "lan".into(),
            public_url: TextEdit::new(""),
            port: 0,
            lan: String::new(),
            signed_in: false,
            gateway_state: String::new(),
            gateway_url: String::new(),
            gateway_reason: String::new(),
            loading: false,
            busy: false,
            waiting: false,
            loaded: false,
            error: None,
            notice: None,
            saved_public: String::new(),
            clock: 0.0,
            next_poll: 0.0,
            deadline: 0.0,
            polling: false,
            restart_pid: None,
        }
    }
    pub fn dirty(&self) -> bool {
        self.loaded
            && (self.public_url.text != self.saved_public
                || self.draft_mode
                    != if self.mode == "public" {
                        "public"
                    } else {
                        "lan"
                    })
    }
    pub fn phase(&self) -> &str {
        if !self.enabled {
            if self.gateway_state == "serving" {
                "pending_off"
            } else {
                "off"
            }
        } else if self.gateway_state == "serving"
            && url::Url::parse(&self.gateway_url)
                .is_ok_and(|u| u.port().is_some_and(|p| p == self.port))
        {
            "serving"
        } else if self.gateway_state == "unavailable" {
            "unavailable"
        } else {
            "pending_on"
        }
    }
    pub fn address(&self) -> String {
        if self.mode == "local" {
            if self.gateway_url.is_empty() {
                format!("http://127.0.0.1:{}", self.port)
            } else {
                self.gateway_url.clone()
            }
        } else if if self.phase() == "off" {
            &self.draft_mode
        } else {
            &self.mode
        } == "public"
        {
            self.public_url.text.trim().into()
        } else if self.lan.is_empty() {
            String::new()
        } else {
            format!("http://{}:{}", self.lan, self.port)
        }
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        self.polling = true;
        vec![(
            "settings.web.status".into(),
            PageRequest::get("/api/webapp-access/status"),
        )]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                self.polling = false;
                self.loading = false;
                if op == "settings.web.write" {
                    self.busy = false;
                }
                if !self.waiting {
                    self.error = Some(e);
                }
                self.next_poll = self.clock + 1.5;
                return vec![];
            }
        };
        self.polling = false;
        self.loading = false;
        if op == "settings.web.write" {
            self.busy = false;
        }
        let Some(port) = v["port"].as_u64().filter(|p| *p > 0 && *p <= 65535) else {
            self.error = Some("Invalid web access status".into());
            if op == "settings.web.write" {
                self.busy = false;
            }
            return vec![];
        };
        if !v["enabled"].is_boolean() || !v["gateway"].is_object() {
            self.error = Some("Invalid web access status".into());
            return vec![];
        }
        let mode = v["mode"].as_str().unwrap_or("");
        let public = v["public_url"].as_str().unwrap_or("");
        if !self.loaded || mode != self.mode || public != self.saved_public {
            self.draft_mode = if mode == "public" { "public" } else { "lan" }.into();
            self.public_url.replace_text(public);
        }
        self.mode = mode.into();
        self.saved_public = public.into();
        self.enabled = v["enabled"].as_bool().unwrap();
        self.port = port as u16;
        self.lan = v["lan_address"].as_str().unwrap_or("").into();
        self.signed_in = v["nous_signed_in"].as_bool().unwrap_or(false);
        self.gateway_state = v["gateway"]["state"]
            .as_str()
            .unwrap_or("unavailable")
            .into();
        self.gateway_url = v["gateway"]["url"].as_str().unwrap_or("").into();
        self.gateway_reason = v["gateway"]["reason"].as_str().unwrap_or("").into();
        self.loaded = true;
        if op == "settings.web.write" {
            self.busy = false;
            if v["restart_started"].as_bool() == Some(true) {
                self.waiting = true;
                self.deadline = self.clock + 90.0;
                self.restart_pid = v["restart_pid"].as_u64();
                self.notice =
                    Some("Configuration saved. Waiting for the gateway to apply it…".into());
            } else {
                self.notice=Some(if v["restart_busy"].as_bool()==Some(true){"Configuration saved. A running conversation prevented the gateway restart; restart when it is idle."}else{"Configuration saved. Restart the gateway to apply it."}.into());
            }
        }
        if self.waiting && op == "settings.web.status" {
            if self.restart_pid.is_some()
                && v["restart_failure"]["pid"].as_u64() == self.restart_pid
            {
                self.waiting = false;
                self.error = Some(format!(
                    "Gateway restart failed: {}",
                    v["restart_failure"]["reason"]
                        .as_str()
                        .unwrap_or("Unknown failure")
                ));
            } else if !matches!(self.phase(), "pending_on" | "pending_off") {
                self.waiting = false;
                self.notice = Some(
                    if self.phase() == "serving" {
                        "Web access is serving the saved configuration"
                    } else if self.phase() == "unavailable" {
                        "The gateway applied the configuration but web access is unavailable"
                    } else {
                        "Web access stopped"
                    }
                    .into(),
                );
            }
        }
        self.next_poll = self.clock + if self.waiting { 1.5 } else { 10.0 };
        vec![]
    }
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.busy || self.waiting {
            return vec![];
        }
        self.error = None;
        match a {
            Action::Load => self.load(),
            Action::Mode(mode) => {
                if matches!(mode.as_str(), "lan" | "public") && self.phase() == "off" {
                    self.draft_mode = mode;
                }
                vec![]
            }
            Action::Revert => {
                self.public_url.replace_text(&self.saved_public);
                self.draft_mode = if self.mode == "public" {
                    "public"
                } else {
                    "lan"
                }
                .into();
                vec![]
            }
            Action::Run(kind) => {
                if !self.loaded {
                    return vec![];
                }
                let body = if kind == "start" {
                    if !self.signed_in {
                        self.error = Some("Sign in to Nous before enabling web access".into());
                        return vec![];
                    }
                    if self.draft_mode == "lan" && self.lan.is_empty() {
                        self.error = Some("The agent did not report a LAN address".into());
                        return vec![];
                    }
                    if self.draft_mode == "public"
                        && !url::Url::parse(self.public_url.text.trim()).is_ok_and(|u| {
                            u.scheme() == "https"
                                && u.host_str().is_some()
                                && u.username().is_empty()
                                && u.password().is_none()
                        })
                    {
                        self.error = Some(
                            "Enter a valid HTTPS public URL without embedded credentials".into(),
                        );
                        return vec![];
                    }
                    json!({"mode":self.draft_mode,"public_url":if self.draft_mode=="public"{json!(self.public_url.text.trim())}else{Value::Null}})
                } else if matches!(kind.as_str(), "stop" | "restart") {
                    json!({})
                } else {
                    return vec![];
                };
                self.busy = true;
                vec![(
                    "settings.web.write".into(),
                    PageRequest::post(format!("/api/webapp-access/{kind}"), body)
                        .timeout_secs(if kind == "start" { 35 * 60 } else { 30 }),
                )]
            }
        }
    }
    pub fn poll(&mut self, now: f64, visible: bool) -> Vec<PageEffect> {
        self.clock = now;
        if self.waiting && now >= self.deadline {
            self.waiting = false;
            self.error=Some("Gateway restart has not been confirmed. The saved configuration remains; refresh before retrying.".into());
        }
        if (self.waiting || visible && self.loaded)
            && !self.busy
            && !self.polling
            && now >= self.next_poll
        {
            return self.load();
        }
        vec![]
    }
    pub fn next_wake(&self, visible: bool) -> Option<std::time::Duration> {
        ((self.waiting || visible && self.loaded) && !self.busy && !self.polling)
            .then(|| std::time::Duration::from_secs_f64((self.next_poll - self.clock).max(0.01)))
    }
}
#[derive(Clone)]
pub enum Action {
    Load,
    Mode(String),
    Revert,
    Run(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(enabled: bool, state: &str) -> Value {
        json!({"enabled":enabled,"mode":"lan","public_url":"","port":8080,"lan_address":"192.0.2.1","nous_signed_in":true,"gateway":{"state":state,"url":if state=="serving"{json!("http://192.0.2.1:8080")}else{Value::Null}}})
    }
    #[test]
    fn saved_web_config_does_not_claim_serving_until_restart_readback() {
        let mut w = WebAccess::new();
        w.apply("settings.web.status", Ok(value(false, "off")));
        let e = w.act(Action::Run("start".into()));
        assert_eq!(e[0].1.timeout, 2100);
        let mut v = value(true, "off");
        v["restart_started"] = json!(true);
        v["restart_pid"] = json!(42);
        w.apply("settings.web.write", Ok(v));
        assert!(w.waiting);
        assert_eq!(w.phase(), "pending_on");
        w.apply("settings.web.status", Ok(value(true, "serving")));
        assert!(!w.waiting);
        assert_eq!(w.phase(), "serving");
    }
    #[test]
    fn restart_failure_is_bound_to_the_acknowledged_pid() {
        let mut w = WebAccess::new();
        w.apply("settings.web.status", Ok(value(false, "off")));
        w.act(Action::Run("start".into()));
        let mut v = value(true, "off");
        v["restart_started"] = json!(true);
        v["restart_pid"] = json!(42);
        w.apply("settings.web.write", Ok(v));
        let mut v = value(true, "off");
        v["restart_failure"] = json!({"pid":42,"reason":"Active conversation"});
        w.apply("settings.web.status", Ok(v));
        assert!(!w.waiting);
        assert!(w.error.as_ref().unwrap().contains("Active conversation"));
        assert_eq!(w.phase(), "pending_on");
    }
}
