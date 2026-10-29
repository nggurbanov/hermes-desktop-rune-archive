//! Messaging credentials, pairing, webhooks and Telegram onboarding.
use super::{PageEffect, PageRequest as Req, segment};
use crate::app::Viewer;
use rune::TextEdit;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
mod actions;
mod replies;
mod telegram;
#[cfg(test)]
mod tests;

pub struct Messaging {
    pub qr: Option<rune::Image>,
    cache: Option<rune::ImageCache>,
    pub profile: String,
    pending: Option<Pending>,
    clear_fields: HashSet<String>,
    pub demo: bool,
    pub test_result: Option<Value>,
    pub restart_needed: bool,
    pub restart: Option<Restart>,
    pub filter: String,
    pub show_advanced: bool,
    pub telegram_polling: bool,
    pub telegram_error: Option<String>,
    telegram_next: f64,
    telegram_expires: Option<i64>,
    cancel_after_start: bool,
    read_pending: HashSet<String>,
    visible: bool,
    next_refresh: f64,
    clock: f64,
    redraw: bool,
    reconnect_followthrough: bool,
    detached: TextEdit,
    pub open_scope: bool,
    pub open_filter: bool,
    pub webhook_description: TextEdit,
    pub webhook_skills: TextEdit,
    pub webhook_chat: TextEdit,
    pub webhook_deliver_only: bool,
    pub platforms: Vec<Value>,
    pub pairing: Value,
    pub webhooks: Value,
    pub selected: String,
    pub tab: usize,
    pub search: TextEdit,
    pub edits: HashMap<String, TextEdit>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub loading: bool,
    pub busy: bool,
    pub confirm: Option<Action>,
    pub pairing_id: Option<String>,
    pub onboarding: Value,
    pub bot_name: TextEdit,
    pub allowed: TextEdit,
    pub webhook_name: TextEdit,
    pub webhook_prompt: TextEdit,
    pub webhook_events: TextEdit,
    pub webhook_delivery: TextEdit,
    pub new_secret: Option<String>,
}
#[derive(Clone)]
pub enum Action {
    Refresh,
    Filter(String),
    Advanced,
    Clear(String),
    Revert,
    RevertWebhook,
    DismissSecret,
    Select(String),
    Tab(usize),
    Save,
    Toggle(bool),
    Test,
    Approve(String, String),
    Revoke(String, String),
    Confirm,
    Cancel,
    StartTelegram,
    PollTelegram,
    ApplyTelegram,
    CancelTelegram,
    CreateWebhook,
    DeleteWebhook(String),
    ToggleWebhook(String, bool),
    EnableWebhooks,
    Restart,
}
impl Default for Messaging {
    fn default() -> Self {
        Self::new()
    }
}
impl Messaging {
    pub fn new() -> Self {
        Self {
            pending: None,
            clear_fields: HashSet::new(),
            demo: false,
            test_result: None,
            restart_needed: false,
            restart: None,
            filter: "all".into(),
            show_advanced: false,
            telegram_polling: false,
            telegram_error: None,
            telegram_next: 0.0,
            telegram_expires: None,
            cancel_after_start: false,
            read_pending: HashSet::new(),
            visible: false,
            next_refresh: 0.0,
            clock: 0.0,
            redraw: false,
            reconnect_followthrough: false,
            detached: TextEdit::new(""),
            qr: None,
            cache: None,
            profile: "default".into(),
            open_scope: false,
            open_filter: false,
            webhook_description: TextEdit::new(""),
            webhook_skills: TextEdit::new(""),
            webhook_chat: TextEdit::new(""),
            webhook_deliver_only: false,
            platforms: vec![],
            pairing: json!({}),
            webhooks: json!({}),
            selected: String::new(),
            tab: 0,
            search: TextEdit::new(""),
            edits: HashMap::new(),
            error: None,
            notice: None,
            loading: false,
            busy: false,
            confirm: None,
            pairing_id: None,
            onboarding: Value::Null,
            bot_name: TextEdit::new(""),
            allowed: TextEdit::new(""),
            webhook_name: TextEdit::new(""),
            webhook_prompt: TextEdit::new(""),
            webhook_events: TextEdit::new(""),
            webhook_delivery: TextEdit::new("local"),
            new_secret: None,
        }
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        if self.demo {
            return vec![];
        }
        self.next_refresh = self.clock + 6.0;
        let mut effects = vec![];
        for (op, path) in [
            ("platforms", "/api/messaging/platforms"),
            ("pairing", "/api/pairing"),
            ("webhooks", "/api/webhooks"),
        ] {
            if self.read_pending.insert(op.into()) {
                effects.push((
                    format!("messaging.{op}"),
                    Req::get(path).profile(&self.profile),
                ));
            }
        }
        self.loading = !self.read_pending.is_empty();
        effects
    }
    pub fn current(&self) -> Option<&Value> {
        self.platforms
            .iter()
            .find(|p| p["id"].as_str() == Some(&self.selected))
    }
    pub fn fixture(&mut self) {
        self.apply("messaging.platforms",Ok(json!({"platforms":[{"id":"telegram","name":"Telegram","description":"Talk to Hermes from Telegram","enabled":true,"configured":true,"state":"connected","env_vars":[{"key":"TELEGRAM_BOT_TOKEN","prompt":"Bot token","is_password":true,"is_set":true,"redacted_value":"••••••"},{"key":"TELEGRAM_ALLOWED_USERS","prompt":"Allowed users","is_list":true,"is_set":true,"value":"123456"}]}]})));
        self.demo = true;
    }
}
impl Viewer {
    pub fn messaging_load(&mut self) -> Vec<PageEffect> {
        let profile = self
            .settings
            .scope_profile
            .clone()
            .unwrap_or_else(|| self.profile.clone());
        if self.messaging.profile != profile {
            if self.messaging.blocks_owner_change() {
                self.messaging.error = Some(
                    "Save credential edits and finish Telegram setup before changing its profile"
                        .into(),
                );
                return vec![];
            }
            self.invalidate_page_requests("messaging.");
            self.messaging.reset();
            self.messaging.profile = profile;
        }
        self.messaging.load()
    }
    pub fn messaging_scope(&mut self, profile: Option<String>) {
        if self.messaging.blocks_owner_change() {
            self.messaging.error = Some(
                "Save credential edits and finish Telegram setup before changing its profile"
                    .into(),
            );
            return;
        }
        self.settings_action(crate::pages::settings::Action::Scope(profile));
        let effects = self.messaging_load();
        self.page_requests(effects);
    }
    pub fn messaging_action(&mut self, action: Action) {
        let profile = self.messaging.profile.clone();
        let effects = self.messaging.act(action, &profile);
        self.page_requests(effects);
    }
}
impl Messaging {
    pub fn blocks_owner_change(&self) -> bool {
        self.busy
            || self.credentials_dirty()
            || self.webhook_dirty()
            || self.pairing_id.is_some()
            || self.restart.as_ref().is_some_and(|r| !r.finished)
    }
    pub fn edit_detached(&mut self) -> &mut TextEdit {
        &mut self.detached
    }
    pub fn credentials_dirty(&self) -> bool {
        !self.clear_fields.is_empty()
            || self.platforms.iter().any(|p| {
                let id = p["id"].as_str().unwrap_or("");
                p["env_vars"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|f| self.field_changed(id, f))
            })
    }
    pub fn field_changed(&self, id: &str, f: &Value) -> bool {
        let key = format!("{id}:{}", f["key"].as_str().unwrap_or(""));
        self.clear_fields.contains(&key)
            || self.edits.get(&key).is_some_and(|e| {
                if f["is_list"] == true {
                    e.text.trim() != f["value"].as_str().unwrap_or("").trim()
                } else {
                    !e.text.trim().is_empty()
                        && (f["is_password"] == true
                            || e.text.trim() != f["value"].as_str().unwrap_or(""))
                }
            })
    }
    pub fn field_cleared(&self, key: &str) -> bool {
        self.clear_fields.contains(key)
    }
    pub fn webhook_dirty(&self) -> bool {
        [
            &self.webhook_name,
            &self.webhook_prompt,
            &self.webhook_events,
            &self.webhook_chat,
            &self.webhook_skills,
            &self.webhook_description,
        ]
        .iter()
        .any(|e| !e.text.is_empty())
            || self.webhook_delivery.text != "local"
            || self.webhook_deliver_only
    }
    pub fn observe(&mut self, visible: bool) {
        if visible && !self.visible {
            self.next_refresh = 0.0;
        }
        self.visible = visible;
    }
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }
    pub fn take_reconnect_followthrough(&mut self) -> bool {
        std::mem::take(&mut self.reconnect_followthrough)
    }
    pub fn poll(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        if self.demo {
            return vec![];
        }
        let mut e = vec![];
        if self.visible && now >= self.next_refresh {
            e.extend(self.load());
        }
        e.extend(self.poll_telegram());
        e.extend(self.poll_restart());
        e
    }
    pub fn wake_after(&self) -> Option<Duration> {
        if self.demo {
            return None;
        }
        if self.visible
            || self.pairing_id.is_some()
            || self.restart.as_ref().is_some_and(|r| !r.finished)
        {
            Some(Duration::from_secs(1))
        } else {
            None
        }
    }
}

impl Messaging {
    pub fn init(&mut self, waker: rune::Waker) {
        self.cache = Some(rune::ImageCache::new(waker));
        self.update_qr();
    }
    pub fn reset(&mut self) {
        let cache = self.cache.take();
        *self = Self::new();
        self.cache = cache;
    }
    fn update_qr(&mut self) {
        self.qr = None;
        let Some(payload) = self.onboarding["qr_payload"].as_str() else {
            return;
        };
        let Ok(qr) = qrcodegen::QrCode::encode_text(payload, qrcodegen::QrCodeEcc::Medium) else {
            self.error = Some("The setup QR payload is too large".into());
            return;
        };
        let size = qr.size();
        let mut svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {} {}'><rect width='100%' height='100%' fill='white'/><path fill='black' d='",
            size + 8,
            size + 8
        );
        for y in 0..size {
            for x in 0..size {
                if qr.get_module(x, y) {
                    svg.push_str(&format!("M{},{}h1v1h-1z", x + 4, y + 4));
                }
            }
        }
        svg.push_str("'/></svg>");
        if let Some(cache) = &self.cache {
            self.qr = Some(cache.load_mime(
                std::sync::Arc::<[u8]>::from(svg.into_bytes()),
                "image/svg+xml",
                240.0,
                1.5,
            ));
        }
    }
}

#[derive(Clone)]
enum Pending {
    Credentials {
        platform: String,
        sent: HashMap<String, String>,
        clear: Vec<String>,
    },
    Toggle,
    Test(String),
    Pairing,
    WebhookCreate(Value),
    Webhook,
    TelegramApply,
    TelegramCancel,
    Restart,
}
pub struct Restart {
    pub name: String,
    pub pid: Option<u64>,
    pub running: bool,
    pub finished: bool,
    pub error: Option<String>,
    pub lines: Vec<String>,
    pub exit: Option<i64>,
    polling: bool,
    polls: u8,
    next: f64,
    accepted: bool,
}

/// Matches the runtime tone contract used by the gateway status menu.
pub fn platform_group(p: &Value) -> &str {
    if p["enabled"] == false || p["state"] == "disabled" {
        "disabled"
    } else {
        match p["state"].as_str() {
            Some("connected") => "ready",
            Some("fatal" | "startup_failed") => "issues",
            _ => "setup",
        }
    }
}
