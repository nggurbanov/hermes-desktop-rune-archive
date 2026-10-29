use super::*;
impl Messaging {
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        if op.starts_with("messaging.telegram.") {
            return self.telegram_reply(op, result);
        }
        if op == "messaging.restart.status" {
            return self.restart_reply(result);
        }
        if op == "messaging.mutation" {
            return self.mutation_reply(result);
        }
        let key = op.trim_start_matches("messaging.");
        self.read_pending.remove(key);
        self.loading = !self.read_pending.is_empty();
        match result {
            Err(e) => {
                self.error = Some(e);
            }
            Ok(v) => match op {
                "messaging.platforms" => {
                    let Some(platforms) = v["platforms"].as_array() else {
                        self.error = Some("The agent returned no platform list".into());
                        return vec![];
                    };
                    self.platforms = platforms.clone();
                    if !self
                        .platforms
                        .iter()
                        .any(|p| p["id"].as_str() == Some(&self.selected))
                    {
                        self.selected = self
                            .platforms
                            .first()
                            .and_then(|p| p["id"].as_str())
                            .unwrap_or("")
                            .into();
                    }
                }
                "messaging.pairing" => self.pairing = v,
                "messaging.webhooks" => self.webhooks = v,
                _ => {}
            },
        }
        vec![]
    }
    fn mutation_reply(&mut self, result: Result<Value, String>) -> Vec<PageEffect> {
        self.busy = false;
        let Some(pending) = self.pending.take() else {
            return vec![];
        };
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                if matches!(pending, Pending::TelegramCancel) {
                    self.clear_telegram();
                    self.telegram_error = Some(format!(
                        "Setup closed locally; the agent could not confirm cancellation. The link will expire. {e}"
                    ));
                } else if matches!(pending, Pending::Restart) {
                    self.restart_needed = true;
                    self.error = Some(format!("The restart request outcome is unconfirmed. {e}"));
                    return self.start_restart_watch("gateway-restart".into(), None, false);
                } else {
                    self.error = Some(e);
                }
                return vec![];
            }
        };
        if let Pending::Test(platform) = pending {
            if !v["ok"].is_boolean() || !v["message"].is_string() {
                self.error = Some("The platform returned an invalid connection-test result".into());
            } else {
                let mut result = v;
                result["platform"] = json!(platform);
                self.test_result = Some(result);
            }
            return self.load();
        }
        if v["ok"] != true {
            self.error = Some(
                v["error"]
                    .as_str()
                    .or(v["message"].as_str())
                    .unwrap_or("The agent did not acknowledge this messaging change")
                    .into(),
            );
            return vec![];
        }
        match pending {
            Pending::Credentials {
                platform,
                sent,
                clear,
            } => {
                for (key, value) in sent {
                    if self.edits.get(&key).is_some_and(|e| e.text.trim() == value) {
                        self.edits.remove(&key);
                    }
                }
                for key in clear {
                    let id = format!("{platform}:{key}");
                    self.clear_fields.remove(&id);
                    if self
                        .edits
                        .get(&id)
                        .is_some_and(|e| e.text.trim().is_empty())
                    {
                        self.edits.remove(&id);
                    }
                }
                self.changed_adapters(&v);
            }
            Pending::Toggle => self.changed_adapters(&v),
            Pending::Pairing => self.notice = Some("Messaging access updated".into()),
            Pending::WebhookCreate(sent) => {
                self.new_secret = v["secret"].as_str().map(str::to_owned);
                if self.webhook_draft() == sent {
                    self.clear_webhook();
                }
                self.notice =
                    Some("Webhook created. Copy its one-time secret before leaving.".into());
            }
            Pending::Webhook => self.notice = Some("Webhook subscription updated".into()),
            Pending::TelegramCancel => {
                self.clear_telegram();
                self.telegram_error = None;
                self.notice = Some("Telegram setup cancelled".into());
            }
            Pending::TelegramApply => {
                self.clear_telegram();
                self.notice = Some("Telegram bot saved to this profile".into());
                if v["restart_started"] == true {
                    self.restart_needed = true;
                    let name = v["restart_action"]
                        .as_str()
                        .unwrap_or("gateway-restart")
                        .to_owned();
                    let mut effects = self.load();
                    effects.extend(self.start_restart_watch(name, v["restart_pid"].as_u64(), true));
                    return effects;
                }
                if v["needs_restart"] == true {
                    self.restart_needed = true;
                    self.confirm = Some(Action::Restart);
                    if let Some(error) = v["restart_error"].as_str() {
                        self.error = Some(format!(
                            "Bot saved; gateway restart was not started: {error}"
                        ));
                    }
                }
            }
            Pending::Restart => {
                let Some(name) = v["name"].as_str().filter(|n| !n.is_empty()) else {
                    self.restart_needed = true;
                    self.error = Some("The agent did not return a restart receipt".into());
                    return vec![];
                };
                self.restart_needed = true;
                return self.start_restart_watch(name.into(), v["pid"].as_u64(), true);
            }
            Pending::Test(_) => unreachable!(),
        }
        self.load()
    }
    fn changed_adapters(&mut self, v: &Value) {
        if v["hot_served"] == true {
            self.notice = Some("Saved. The served profile reloaded its adapters.".into());
            self.next_refresh = self.clock + 4.0;
        } else {
            self.restart_needed = true;
            self.notice = Some(
                "Saved. Restart the shared gateway to activate these adapter settings.".into(),
            );
        }
    }
}
