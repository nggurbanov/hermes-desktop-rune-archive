use super::*;
pub(super) fn users(input: &str) -> Result<Vec<String>, String> {
    let mut users = vec![];
    for id in input
        .split([',', '\n', ' ', '\t'])
        .filter(|s| !s.is_empty())
    {
        if !(3..=20).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("{id} is not a Telegram user ID. Use 3–20 digits."));
        }
        if !users.iter().any(|v| v == id) {
            users.push(id.to_owned());
        }
    }
    if users.is_empty() {
        Err("Add at least one allowed Telegram user ID".into())
    } else {
        Ok(users)
    }
}
impl Messaging {
    pub(super) fn start_telegram(&mut self) -> Vec<PageEffect> {
        if self.pairing_id.is_some() {
            return vec![];
        }
        self.busy = true;
        self.cancel_after_start = false;
        self.telegram_error = None;
        let mut body = json!({});
        if !self.bot_name.text.trim().is_empty() {
            body["bot_name"] = json!(self.bot_name.text.trim());
        }
        vec![(
            "messaging.telegram.start".into(),
            Req::post("/api/messaging/telegram/onboarding/start", body).profile(&self.profile),
        )]
    }
    pub(super) fn telegram_request(&mut self) -> Vec<PageEffect> {
        if self.telegram_polling
            || self
                .pending
                .as_ref()
                .is_some_and(|p| matches!(p, Pending::TelegramApply | Pending::TelegramCancel))
        {
            return vec![];
        }
        let Some(id) = &self.pairing_id else {
            return vec![];
        };
        self.telegram_polling = true;
        vec![(
            format!("messaging.telegram.status.{id}"),
            Req::get(format!(
                "/api/messaging/telegram/onboarding/{}",
                segment(id)
            ))
            .profile(&self.profile),
        )]
    }
    pub(super) fn poll_telegram(&mut self) -> Vec<PageEffect> {
        if self.pairing_id.is_none() {
            return vec![];
        }
        if self
            .telegram_expires
            .is_some_and(|t| t <= chrono::Utc::now().timestamp())
            && !self.busy
        {
            self.clear_telegram();
            self.telegram_error =
                Some("This Telegram setup link expired. Start again to get a new QR code.".into());
            self.redraw = true;
            return vec![];
        }
        if self.onboarding["status"] != "ready" && self.clock >= self.telegram_next {
            return self.telegram_request();
        }
        vec![]
    }
    pub fn telegram_remaining(&self) -> Option<i64> {
        self.telegram_expires
            .map(|t| (t - chrono::Utc::now().timestamp()).max(0))
    }
    pub(super) fn set_expiry(&mut self, v: &Value) {
        if let Some(s) = v["expires_at"].as_str() {
            self.telegram_expires = chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|t| t.timestamp());
        } else if let Some(t) = v["expires_at"].as_i64() {
            self.telegram_expires = Some(t);
        }
    }
    pub(super) fn clear_telegram(&mut self) {
        self.pairing_id = None;
        self.onboarding = Value::Null;
        self.qr = None;
        self.allowed.replace_text("");
        self.telegram_expires = None;
        self.telegram_polling = false;
    }
    pub(super) fn telegram_reply(&mut self, op: &str, r: Result<Value, String>) -> Vec<PageEffect> {
        if op.ends_with("start") {
            self.busy = false;
            let v = match r {
                Ok(v) => v,
                Err(e) => {
                    self.telegram_error = Some(e);
                    return vec![];
                }
            };
            let Some(id) = v["pairing_id"].as_str().filter(|id| !id.is_empty()) else {
                self.telegram_error = Some("The agent returned no Telegram setup receipt".into());
                return vec![];
            };
            self.pairing_id = Some(id.into());
            self.set_expiry(&v);
            self.onboarding = v;
            self.update_qr();
            self.telegram_next = self.clock + 1.2;
            if self.cancel_after_start {
                self.cancel_after_start = false;
                let p = self.profile.clone();
                return self.act(Action::CancelTelegram, &p);
            }
        } else {
            if let Some(id) = op.strip_prefix("messaging.telegram.status.") {
                if self.pairing_id.as_deref() != Some(id) {
                    return vec![];
                }
            }
            self.telegram_polling = false;
            self.telegram_next = self.clock + 2.0;
            match r {
                Err(e) => {
                    let lower = e.to_lowercase();
                    if lower.contains("410") {
                        self.clear_telegram();
                        self.telegram_error = Some(
                            "This setup link is no longer available. Start Telegram setup again."
                                .into(),
                        );
                    } else {
                        self.telegram_error =
                            Some(format!("Status check failed; retrying automatically. {e}"));
                    }
                }
                Ok(v) => {
                    if self.pairing_id.is_none() {
                        return vec![];
                    }
                    if !matches!(v["status"].as_str(), Some("waiting" | "ready")) {
                        self.telegram_error =
                            Some("The agent returned an unknown Telegram setup state".into());
                        return vec![];
                    }
                    self.telegram_error = None;
                    self.set_expiry(&v);
                    if v["status"] == "ready" && self.allowed.text.is_empty() {
                        if let Some(id) = v["owner_user_id"].as_str() {
                            if users(id).is_ok() {
                                self.allowed.replace_text(id);
                            }
                        }
                    }
                    if let Some(fields) = v.as_object() {
                        for (k, v) in fields {
                            self.onboarding[k] = v.clone();
                        }
                    }
                }
            }
        }
        vec![]
    }
    pub(super) fn start_restart_watch(
        &mut self,
        name: String,
        pid: Option<u64>,
        accepted: bool,
    ) -> Vec<PageEffect> {
        self.restart = Some(Restart {
            name,
            pid,
            accepted,
            running: true,
            finished: false,
            error: None,
            lines: vec![],
            exit: None,
            polling: false,
            polls: 0,
            next: self.clock,
        });
        self.poll_restart()
    }
    pub(super) fn poll_restart(&mut self) -> Vec<PageEffect> {
        let Some(r) = &mut self.restart else {
            return vec![];
        };
        if r.finished || r.polling || self.clock < r.next {
            return vec![];
        }
        r.polling = true;
        vec![(
            "messaging.restart.status".into(),
            Req::get(format!("/api/actions/{}/status", segment(&r.name)))
                .query("lines", 120)
                .profile(&self.profile),
        )]
    }
    pub(super) fn restart_reply(&mut self, result: Result<Value, String>) -> Vec<PageEffect> {
        let Some(r) = &mut self.restart else {
            return vec![];
        };
        r.polling = false;
        r.polls += 1;
        r.next = self.clock + 1.2;
        match result {
            Err(e) => {
                r.error = Some(format!("Waiting for the restarting gateway: {e}"));
            }
            Ok(v) => {
                if v["name"].as_str().is_some_and(|name| name != r.name)
                    || !v["running"].is_boolean()
                {
                    r.error = Some("The restart status receipt is invalid".into());
                } else {
                    r.error = None;
                    r.running = v["running"] == true;
                    r.exit = v["exit_code"].as_i64();
                    r.lines = v["lines"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                    if !r.running && r.exit.is_some() {
                        r.finished = true;
                        self.reconnect_followthrough = true;
                        if r.accepted && r.exit == Some(0) {
                            self.restart_needed = false;
                            self.notice = Some(
                                "The gateway restart completed. Refreshing platform status.".into(),
                            );
                            return self.load();
                        }
                        self.restart_needed = true;
                        r.error = Some(if r.accepted {
                            format!(
                                "Gateway restart failed with exit code {}. Retry or inspect its log.",
                                r.exit.unwrap()
                            )
                        } else {
                            "A previous restart log cannot confirm this request. Reconnect and check gateway status before retrying.".into()
                        });
                    }
                }
            }
        }
        if r.polls >= 18 && !r.finished {
            r.finished = true;
            self.restart_needed = true;
            self.reconnect_followthrough = true;
            r.error=Some("The restart outcome could not be confirmed. Reconnect and inspect gateway status before retrying.".into());
        }
        vec![]
    }
}
