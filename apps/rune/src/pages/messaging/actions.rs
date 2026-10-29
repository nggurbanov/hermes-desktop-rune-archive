use super::*;
impl Messaging {
    pub fn act(&mut self, action: Action, _profile: &str) -> Vec<PageEffect> {
        let preview = matches!(
            action,
            Action::Select(_)
                | Action::Tab(_)
                | Action::Filter(_)
                | Action::Advanced
                | Action::Cancel
                | Action::DismissSecret
        );
        if self.demo && !preview {
            self.notice = Some("Demonstration data · live messaging writes are disabled".into());
            return vec![];
        }
        if self.busy && !preview && !matches!(action, Action::CancelTelegram | Action::PollTelegram)
        {
            return vec![];
        }
        self.error = None;
        let (pending, request) = match action {
            Action::Refresh => return self.load(),
            Action::Filter(f) => {
                self.filter = f;
                return vec![];
            }
            Action::Advanced => {
                self.show_advanced = !self.show_advanced;
                return vec![];
            }
            Action::Select(id) => {
                self.selected = id;
                self.test_result = None;
                return vec![];
            }
            Action::Tab(tab) => {
                self.tab = tab;
                return vec![];
            }
            Action::Revert => {
                let prefix = format!("{}:", self.selected);
                self.edits.retain(|k, _| !k.starts_with(&prefix));
                self.clear_fields.retain(|k| !k.starts_with(&prefix));
                return vec![];
            }
            Action::RevertWebhook => {
                self.clear_webhook();
                return vec![];
            }
            Action::DismissSecret => {
                self.new_secret = None;
                return vec![];
            }
            Action::Clear(key) => {
                let id = format!("{}:{key}", self.selected);
                if self.clear_fields.contains(&id) {
                    self.clear_fields.remove(&id);
                } else {
                    self.clear_fields.insert(id);
                }
                return vec![];
            }
            Action::Save => {
                let mut env = serde_json::Map::new();
                let mut sent = HashMap::new();
                let mut clear = vec![];
                if let Some(p) = self.current() {
                    for f in p["env_vars"].as_array().into_iter().flatten() {
                        let key = f["key"].as_str().unwrap_or("");
                        let id = format!("{}:{key}", self.selected);
                        if !self.field_changed(&self.selected, f) {
                            continue;
                        }
                        let value = self.edits.get(&id).map(|e| e.text.trim()).unwrap_or("");
                        if self.clear_fields.contains(&id)
                            || f["is_list"] == true && value.is_empty()
                        {
                            clear.push(key.to_owned());
                        } else if !value.is_empty() {
                            env.insert(key.into(), json!(value));
                            sent.insert(id, value.to_owned());
                        }
                    }
                }
                if env.is_empty() && clear.is_empty() {
                    return vec![];
                }
                (
                    Pending::Credentials {
                        platform: self.selected.clone(),
                        sent,
                        clear: clear.clone(),
                    },
                    Req::put(
                        format!("/api/messaging/platforms/{}", segment(&self.selected)),
                        json!({"env":env,"clear_env":clear}),
                    ),
                )
            }
            Action::Toggle(enabled) => (
                Pending::Toggle,
                Req::put(
                    format!("/api/messaging/platforms/{}", segment(&self.selected)),
                    json!({"enabled":enabled}),
                ),
            ),
            Action::Test => (
                Pending::Test(self.selected.clone()),
                Req::post(
                    format!("/api/messaging/platforms/{}/test", segment(&self.selected)),
                    json!({}),
                ),
            ),
            Action::Approve(platform, id) => {
                if id.is_empty() {
                    self.error = Some("The pairing request has no request ID".into());
                    return vec![];
                }
                (
                    Pending::Pairing,
                    Req::post(
                        "/api/pairing/approve",
                        json!({"platform":platform,"request_id":id,"profile":self.profile}),
                    ),
                )
            }
            Action::Revoke(_, _) | Action::DeleteWebhook(_) | Action::Restart => {
                self.confirm = Some(action);
                return vec![];
            }
            Action::Cancel => {
                self.confirm = None;
                return vec![];
            }
            Action::Confirm => match self.confirm.take() {
                Some(Action::Revoke(platform, id)) => (
                    Pending::Pairing,
                    Req::post(
                        "/api/pairing/revoke",
                        json!({"platform":platform,"user_id":id,"profile":self.profile}),
                    ),
                ),
                Some(Action::DeleteWebhook(name)) => (
                    Pending::Webhook,
                    Req::delete(format!("/api/webhooks/{}", segment(&name)), Value::Null),
                ),
                Some(Action::Restart) => {
                    if self.restart.as_ref().is_some_and(|r| !r.finished) {
                        return vec![];
                    }
                    (
                        Pending::Restart,
                        Req::post("/api/gateway/restart", json!({})),
                    )
                }
                _ => return vec![],
            },
            Action::StartTelegram => return self.start_telegram(),
            Action::PollTelegram => return self.telegram_request(),
            Action::ApplyTelegram => {
                let Some(id) = &self.pairing_id else {
                    return vec![];
                };
                if self.onboarding["status"] != "ready" {
                    self.error = Some("Finish linking this Telegram bot before applying it".into());
                    return vec![];
                }
                let users = match telegram::users(&self.allowed.text) {
                    Ok(u) => u,
                    Err(e) => {
                        self.error = Some(e);
                        return vec![];
                    }
                };
                (
                    Pending::TelegramApply,
                    Req::post(
                        format!("/api/messaging/telegram/onboarding/{}/apply", segment(id)),
                        json!({"allowed_user_ids":users,"profile":self.profile}),
                    ),
                )
            }
            Action::CancelTelegram => {
                if self.busy && self.pairing_id.is_none() {
                    self.cancel_after_start = true;
                    return vec![];
                }
                if self.pending.is_some() {
                    if self.pairing_id.is_none() {
                        self.cancel_after_start = true;
                    }
                    return vec![];
                }
                let Some(id) = self.pairing_id.clone() else {
                    return vec![];
                };
                (
                    Pending::TelegramCancel,
                    Req::delete(
                        format!("/api/messaging/telegram/onboarding/{}", segment(&id)),
                        Value::Null,
                    ),
                )
            }
            Action::CreateWebhook => {
                let name = self.webhook_name.text.trim();
                if name.is_empty() {
                    self.error = Some("Enter a webhook name".into());
                    return vec![];
                }
                let body = self.webhook_draft();
                (
                    Pending::WebhookCreate(body.clone()),
                    Req::post("/api/webhooks", body),
                )
            }
            Action::ToggleWebhook(name, enabled) => (
                Pending::Webhook,
                Req::put(
                    format!("/api/webhooks/{}/enabled", segment(&name)),
                    json!({"enabled":enabled}),
                ),
            ),
            Action::EnableWebhooks => (
                Pending::Toggle,
                Req::post("/api/webhooks/enable", json!({})),
            ),
        };
        self.pending = Some(pending);
        self.busy = true;
        vec![("messaging.mutation".into(), request.profile(&self.profile))]
    }
    pub(super) fn webhook_draft(&self) -> Value {
        let list = |s: &str| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        json!({"name":self.webhook_name.text.trim(),"prompt":self.webhook_prompt.text,"events":list(&self.webhook_events.text),"deliver":self.webhook_delivery.text,"deliver_chat_id":self.webhook_chat.text.trim(),"deliver_only":self.webhook_deliver_only,"description":self.webhook_description.text.trim(),"skills":list(&self.webhook_skills.text)})
    }
    pub(super) fn clear_webhook(&mut self) {
        for e in [
            &mut self.webhook_name,
            &mut self.webhook_prompt,
            &mut self.webhook_events,
            &mut self.webhook_chat,
            &mut self.webhook_skills,
            &mut self.webhook_description,
        ] {
            e.replace_text("");
        }
        self.webhook_delivery.replace_text("local");
        self.webhook_deliver_only = false;
    }
}
