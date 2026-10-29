//! Money mutations retain idempotency and only claim a charge after settlement.
use crate::pages::{PageEffect, PageRequest};
use rune::TextEdit;
use serde_json::{Value, json};
pub struct Billing {
    pub state: Value,
    pub subscription: Value,
    pub loading: bool,
    pub busy: bool,
    pub amount: TextEdit,
    pub threshold: TextEdit,
    pub reload_to: TextEdit,
    pub auto_enabled: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub recovery: String,
    pub portal: String,
    pub preview: Option<Value>,
    pub preview_tier: String,
    pub verification_url: String,
    pub verification_code: String,
    pub stepping_up: bool,
    pub charge_id: Option<String>,
    pub charge_status: String,
    intent: Option<(String, String)>,
    pub pending_keep: Option<bool>,
    keep_after_approval: bool,
    pub confirm_disable: bool,
    clock: f64,
    next_poll: f64,
    deadline: f64,
    polling: bool,
    retry_at: f64,
}
impl Billing {
    pub fn new() -> Self {
        Self {
            state: Value::Null,
            subscription: Value::Null,
            loading: false,
            busy: false,
            amount: TextEdit::new(""),
            threshold: TextEdit::new(""),
            reload_to: TextEdit::new(""),
            auto_enabled: false,
            error: None,
            notice: None,
            recovery: String::new(),
            portal: String::new(),
            preview: None,
            preview_tier: String::new(),
            verification_url: String::new(),
            verification_code: String::new(),
            stepping_up: false,
            charge_id: None,
            charge_status: String::new(),
            intent: None,
            pending_keep: None,
            keep_after_approval: false,
            confirm_disable: false,
            clock: 0.0,
            next_poll: 0.0,
            deadline: 0.0,
            polling: false,
            retry_at: 0.0,
        }
    }
    pub fn blocked(&self) -> bool {
        self.busy || self.stepping_up || self.charge_id.is_some()
    }
    pub fn auto_editable(&self) -> bool {
        self.state["auto_reload"]["enabled"].as_bool() == Some(true)
            && self.state["auto_reload"]["card"]["kind"] != "distinct"
    }
    pub fn auto_dirty(&self) -> bool {
        self.state["auto_reload"].is_object()
            && (self.auto_enabled
                != self.state["auto_reload"]["enabled"]
                    .as_bool()
                    .unwrap_or(false)
                || self.threshold.text != initial_amount(&self.state["auto_reload"], "threshold")
                || self.reload_to.text != initial_amount(&self.state["auto_reload"], "reload_to"))
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        if self.stepping_up {
            return vec![];
        }
        self.loading = true;
        vec![
            (
                "settings.billing.state".into(),
                PageRequest::rpc("billing.state", json!({})),
            ),
            (
                "settings.billing.subscription".into(),
                PageRequest::rpc("subscription.state", json!({})),
            ),
        ]
    }
    fn refused(&mut self, v: &Value) -> String {
        let e = if v["error"].is_object() {
            &v["error"]
        } else {
            v
        };
        let code = e["kind"]
            .as_str()
            .or_else(|| e["error"].as_str())
            .or_else(|| e["code"].as_str())
            .unwrap_or("unknown")
            .to_owned();
        let message = e["message"]
            .as_str()
            .or_else(|| v["message"].as_str())
            .unwrap_or(&code);
        self.error = Some(message.into());
        self.portal = e["portal_url"]
            .as_str()
            .or_else(|| v["portal_url"].as_str())
            .unwrap_or(&self.portal)
            .into();
        self.recovery = e["recovery"]
            .as_str()
            .unwrap_or_else(|| match code.as_str() {
                "insufficient_scope" => "step_up",
                "session_revoked" => "login",
                "remote_spending_revoked" => "reconnect",
                "rate_limited"
                | "temporarily_unavailable"
                | "endpoint_unavailable"
                | "network_error"
                | "stripe_unavailable"
                | "internal_error" => "retry",
                "no_payment_method"
                | "monthly_cap_exceeded"
                | "consent_required"
                | "remote_spending_disabled"
                | "cli_billing_disabled"
                | "auto_top_up_disabled_failures"
                | "org_access_denied"
                | "role_required" => "portal",
                _ => "none",
            })
            .into();
        self.retry_at = self.clock + e["retry_after"].as_f64().unwrap_or(0.0).clamp(0.0, 30.0);
        code
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        if op.ends_with(".event") {
            if !self.stepping_up {
                return vec![];
            }
            if let Ok(v) = result {
                let p = v.get("params").or_else(|| v.get("payload")).unwrap_or(&v);
                let p = p.get("payload").unwrap_or(p);
                if let Some(url) = p["verification_url"].as_str().filter(|u| valid_url(u)) {
                    self.verification_url = url.into();
                    self.verification_code = p["user_code"].as_str().unwrap_or("").into();
                }
            }
            return vec![];
        }
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                match op {
                    "settings.billing.state" | "settings.billing.subscription" => {
                        self.loading = false
                    }
                    "settings.billing.charge-status" => {
                        self.polling = false;
                        self.charge_id = None;
                        self.charge_status = "Unconfirmed".into();
                    }
                    _ => {
                        self.busy = false;
                        self.stepping_up = false;
                    }
                }
                self.error = Some(if op == "settings.billing.charge" {
                    "Payment request is unconfirmed. Retry reuses this request's idempotency key; check the billing portal before starting another amount.".into()
                } else {
                    e
                });
                return vec![];
            }
        };
        if v["ok"].as_bool() != Some(true) {
            let code = self.refused(&v);
            if op == "settings.billing.keep-going"
                && code == "insufficient_scope"
                && self.pending_keep.is_some()
                && !self.keep_after_approval
            {
                self.busy = false;
                return self.start_step_up();
            }
            if matches!(
                op,
                "settings.billing.state" | "settings.billing.subscription"
            ) {
                self.loading = false;
            } else if op == "settings.billing.charge-status" {
                self.polling = false;
                if matches!(self.recovery.as_str(), "retry") && self.clock < self.deadline {
                    self.next_poll = self.retry_at.max(self.clock + 2.0);
                } else {
                    self.charge_id = None;
                    self.charge_status = if matches!(
                        code.as_str(),
                        "insufficient_scope" | "remote_spending_revoked" | "session_revoked"
                    ) {
                        "Unconfirmed"
                    } else {
                        "Status refused"
                    }
                    .into();
                }
            } else {
                self.busy = false;
                self.stepping_up = false;
                if op == "settings.billing.charge"
                    && !matches!(
                        code.as_str(),
                        "endpoint_unavailable"
                            | "rate_limited"
                            | "temporarily_unavailable"
                            | "network_error"
                            | "stripe_unavailable"
                    )
                {
                    self.intent = None;
                }
            }
            return vec![];
        }
        match op {
            "settings.billing.state" => {
                self.loading = false;
                let keep = self.auto_dirty();
                self.portal = v["portal_url"]
                    .as_str()
                    .unwrap_or("https://portal.nousresearch.com/billing")
                    .into();
                if self.amount.text.is_empty() {
                    self.amount.replace_text(
                        v["charge_presets"][0]
                            .as_str()
                            .or_else(|| v["min_usd"].as_str())
                            .unwrap_or(""),
                    );
                }
                if !keep {
                    self.auto_enabled = v["auto_reload"]["enabled"].as_bool().unwrap_or(false);
                    self.threshold
                        .replace_text(&initial_amount(&v["auto_reload"], "threshold"));
                    self.reload_to
                        .replace_text(&initial_amount(&v["auto_reload"], "reload_to"));
                }
                self.state = v;
            }
            "settings.billing.subscription" => {
                self.loading = false;
                if let Some(url) = v["portal_url"].as_str().filter(|url| valid_url(url)) {
                    self.portal = url.into();
                }
                self.subscription = v;
            }
            "settings.billing.charge" => {
                self.busy = false;
                let Some(id) = v["charge_id"].as_str().filter(|s| !s.is_empty()) else {
                    self.error=Some("The payment request has no tracking ID. Check the billing portal before buying again.".into());
                    self.charge_status = "Unconfirmed".into();
                    return vec![];
                };
                self.charge_id = Some(id.into());
                self.charge_status = "Processing".into();
                self.deadline = self.clock + 300.0;
                self.next_poll = self.clock;
                return self.poll(self.clock);
            }
            "settings.billing.charge-status" => {
                self.polling = false;
                match v["status"].as_str() {
                    Some("settled") => {
                        self.charge_id = None;
                        self.intent = None;
                        self.charge_status = "Settled".into();
                        self.notice = Some(format!(
                            "Credits added: ${}",
                            v["amount_usd"].as_str().unwrap_or("")
                        ));
                        return self.load();
                    }
                    Some("failed") => {
                        self.charge_id = None;
                        self.intent = None;
                        self.charge_status = "Failed".into();
                        self.error = Some(format!(
                            "Payment failed: {}",
                            v["reason"].as_str().unwrap_or("processing_error")
                        ));
                    }
                    _ => self.next_poll = self.clock + 2.0,
                }
            }
            "settings.billing.preview" => {
                self.busy = false;
                self.preview = Some(v);
            }
            "settings.billing.step-up" => {
                self.busy = false;
                self.stepping_up = false;
                self.verification_url.clear();
                self.verification_code.clear();
                if v["granted"].as_bool() != Some(true) {
                    self.pending_keep = None;
                    self.error = Some("Remote spending approval was not granted".into());
                    return vec![];
                }
                if let Some(enabled) = self.pending_keep {
                    if !self.keep_after_approval {
                        self.keep_after_approval = true;
                        return self.rpc(
                            "settings.billing.keep-going",
                            "billing.keep_going_on_credits",
                            json!({"enabled":enabled}),
                        );
                    }
                }
                self.notice = Some(
                    "Remote spending approved. Retry the action you wanted to perform.".into(),
                );
                return self.load();
            }
            "settings.billing.auto" => {
                self.busy = false;
                self.state["auto_reload"]["enabled"] = json!(self.auto_enabled);
                self.state["auto_reload"]["threshold_usd"] = json!(self.threshold.text);
                self.state["auto_reload"]["reload_to_usd"] = json!(self.reload_to.text);
                self.notice = Some("Automatic credit reload saved".into());
                return self.load();
            }
            "settings.billing.keep-going" | "settings.billing.plan" | "settings.billing.resume" => {
                self.busy = false;
                self.preview = None;
                if op == "settings.billing.keep-going" {
                    self.pending_keep = None;
                    self.keep_after_approval = false;
                }
                self.notice = Some("Billing change saved".into());
                return self.load();
            }
            _ => {}
        }
        vec![]
    }
    fn rpc(&mut self, op: &str, method: &str, body: Value) -> Vec<PageEffect> {
        self.busy = true;
        vec![(op.into(), PageRequest::rpc(method, body))]
    }
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.blocked() && !matches!(a, Action::CancelStepUp) {
            return vec![];
        }
        self.error = None;
        self.notice = None;
        self.recovery.clear();
        match a {
            Action::Load => self.load(),
            Action::Amount(value) => {
                if self
                    .intent
                    .as_ref()
                    .is_some_and(|(amount, _)| money(amount).ok() != money(&value).ok())
                {
                    self.error=Some("The previous payment request is unconfirmed. Retry its amount or review it in the billing portal.".into());
                    return vec![];
                }
                self.amount.replace_text(value);
                vec![]
            }
            Action::Charge => {
                if !self.state["can_charge"].as_bool().unwrap_or(false)
                    || !self.state["cli_billing_enabled"].as_bool().unwrap_or(false)
                {
                    self.error = Some(
                        "This account does not permit credit purchases from the client".into(),
                    );
                    return vec![];
                }
                if self.clock < self.retry_at {
                    self.error =
                        Some("The billing service asked us to wait before retrying".into());
                    return vec![];
                }
                let amount = match self.checked_amount(&self.amount.text) {
                    Ok(a) => a,
                    Err(e) => {
                        self.error = Some(e);
                        return vec![];
                    }
                };
                if self.intent.as_ref().is_some_and(|(a, _)| a != &amount) {
                    self.error = Some(
                        "Retry the original unconfirmed amount or check the billing portal".into(),
                    );
                    return vec![];
                }
                let key = self
                    .intent
                    .get_or_insert_with(|| (amount.clone(), uuid::Uuid::new_v4().to_string()))
                    .1
                    .clone();
                self.rpc(
                    "settings.billing.charge",
                    "billing.charge",
                    json!({"amount_usd":amount,"idempotency_key":key}),
                )
            }
            Action::AutoEnabled(v) => {
                if !v && self.auto_editable() {
                    self.confirm_disable = true;
                }
                vec![]
            }
            Action::CancelDisable => {
                self.confirm_disable = false;
                vec![]
            }
            Action::ConfirmDisable => {
                self.confirm_disable = false;
                self.auto_enabled = false;
                self.act(Action::AutoSave)
            }
            Action::AutoRevert => {
                self.auto_enabled = self.state["auto_reload"]["enabled"]
                    .as_bool()
                    .unwrap_or(false);
                self.threshold
                    .replace_text(initial_amount(&self.state["auto_reload"], "threshold"));
                self.reload_to
                    .replace_text(initial_amount(&self.state["auto_reload"], "reload_to"));
                vec![]
            }
            Action::AutoSave => {
                if !self.auto_editable() {
                    return vec![];
                }
                let threshold = if self.auto_enabled {
                    self.threshold.text.clone()
                } else {
                    initial_amount(&self.state["auto_reload"], "threshold")
                };
                let amount = if self.auto_enabled {
                    self.reload_to.text.clone()
                } else {
                    initial_amount(&self.state["auto_reload"], "reload_to")
                };
                let checked = if self.auto_enabled {
                    match (
                        self.checked_amount(&threshold),
                        self.checked_amount(&amount),
                    ) {
                        (Ok(a), Ok(b)) if money(&b).unwrap() > money(&a).unwrap() => Ok((a, b)),
                        (Err(e), _) | (_, Err(e)) => Err(e),
                        _ => Err("Reload amount must be greater than the threshold".into()),
                    }
                } else {
                    match (money(&threshold),money(&amount)){(Ok(_),Ok(_))=>Ok((threshold,amount)),_=>Err("The saved automatic reload amounts are unavailable; update this setting in the billing portal".into())}
                };
                let (threshold, amount) = match checked {
                    Ok(v) => v,
                    Err(e) => {
                        self.error = Some(e);
                        return vec![];
                    }
                };
                self.threshold.replace_text(&threshold);
                self.reload_to.replace_text(&amount);
                self.rpc("settings.billing.auto","billing.auto_reload",json!({"enabled":self.auto_enabled,"threshold":threshold,"top_up_amount":amount}))
            }
            Action::KeepGoing(enabled) => {
                if !self.state["keep_going_on_credits"].is_boolean() {
                    return vec![];
                }
                self.pending_keep = Some(enabled);
                self.keep_after_approval = false;
                self.rpc(
                    "settings.billing.keep-going",
                    "billing.keep_going_on_credits",
                    json!({"enabled":enabled}),
                )
            }
            Action::Preview(tier) => {
                if !self.subscription["can_change_plan"]
                    .as_bool()
                    .unwrap_or(false)
                    || !self.subscription["tiers"].as_array().is_some_and(|tiers| {
                        tiers.iter().any(|t| {
                            t["tier_id"] == tier
                                && t["is_enabled"].as_bool() == Some(true)
                                && t["is_current"].as_bool() != Some(true)
                        })
                    })
                {
                    return vec![];
                }
                self.preview = None;
                self.preview_tier = tier.clone();
                self.rpc(
                    "settings.billing.preview",
                    "subscription.preview",
                    json!({"subscription_type_id":tier}),
                )
            }
            Action::ConfirmPlan => {
                if !self
                    .preview
                    .as_ref()
                    .is_some_and(|p| p["effect"] == "scheduled")
                {
                    return vec![];
                }
                self.rpc(
                    "settings.billing.plan",
                    "subscription.change",
                    json!({"subscription_type_id":self.preview_tier}),
                )
            }
            Action::CancelPreview => {
                self.preview = None;
                self.preview_tier.clear();
                vec![]
            }
            Action::Resume => {
                if !self.subscription["can_change_plan"]
                    .as_bool()
                    .unwrap_or(false)
                {
                    return vec![];
                }
                self.rpc("settings.billing.resume", "subscription.resume", json!({}))
            }
            Action::StepUp => self.start_step_up(),
            Action::CancelStepUp => {
                self.stepping_up = false;
                self.busy = false;
                self.pending_keep = None;
                self.verification_url.clear();
                self.verification_code.clear();
                vec![]
            }
        }
    }
    fn start_step_up(&mut self) -> Vec<PageEffect> {
        self.error = None;
        self.recovery.clear();
        self.stepping_up = true;
        self.verification_url.clear();
        self.verification_code.clear();
        self.busy = true;
        vec![(
            "settings.billing.step-up".into(),
            PageRequest::rpc("billing.step_up", json!({})).timeout_secs(180),
        )]
    }
    fn checked_amount(&self, value: &str) -> Result<String, String> {
        let cents = money(value)?;
        if let Some(min) = self.state["min_usd"].as_str().and_then(|s| money(s).ok()) {
            if cents < min {
                return Err(format!(
                    "Minimum purchase is ${}",
                    self.state["min_usd"].as_str().unwrap()
                ));
            }
        }
        if let Some(max) = self.state["max_usd"].as_str().and_then(|s| money(s).ok()) {
            if cents > max {
                return Err(format!(
                    "Maximum purchase is ${}",
                    self.state["max_usd"].as_str().unwrap()
                ));
            }
        }
        Ok(format!("{}.{:02}", cents / 100, cents % 100))
    }
    pub fn poll(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        if self.charge_id.is_some() && now >= self.deadline {
            self.charge_id = None;
            self.polling = false;
            self.charge_status = "Unconfirmed".into();
            self.error=Some("Payment settlement has not been confirmed. Review the billing portal before another purchase.".into());
        }
        if let Some(id) = &self.charge_id {
            if !self.polling && now >= self.next_poll {
                self.polling = true;
                return vec![(
                    "settings.billing.charge-status".into(),
                    PageRequest::rpc("billing.charge_status", json!({"charge_id":id})),
                )];
            }
        }
        vec![]
    }
    pub fn next_wake(&self) -> Option<std::time::Duration> {
        (self.charge_id.is_some() && !self.polling)
            .then(|| std::time::Duration::from_secs_f64((self.next_poll - self.clock).max(0.01)))
    }
}
fn initial_amount(v: &Value, key: &str) -> String {
    v[format!("{key}_usd")]
        .as_str()
        .or_else(|| v[format!("{key}_display")].as_str())
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect()
}
fn money(value: &str) -> Result<u64, String> {
    let text = value.trim();
    let mut p = text.split('.');
    let whole = p.next().unwrap_or("");
    let fraction = p.next().unwrap_or("");
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 2
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || p.next().is_some()
    {
        return Err("Enter a positive dollar amount with at most two decimal places".into());
    }
    let whole = whole
        .parse::<u64>()
        .map_err(|_| "The amount is too large")?;
    let fraction = fraction.parse::<u64>().unwrap_or(0) * if fraction.len() == 1 { 10 } else { 1 };
    whole
        .checked_mul(100)
        .and_then(|n| n.checked_add(fraction))
        .filter(|n| *n > 0)
        .ok_or_else(|| "Enter a positive dollar amount".into())
}
pub fn valid_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        matches!(u.scheme(), "https" | "http")
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}
#[derive(Clone)]
pub enum Action {
    Load,
    Amount(String),
    Charge,
    AutoEnabled(bool),
    AutoSave,
    AutoRevert,
    CancelDisable,
    ConfirmDisable,
    KeepGoing(bool),
    Preview(String),
    ConfirmPlan,
    CancelPreview,
    Resume,
    StepUp,
    CancelStepUp,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Billing {
        let mut b = Billing::new();
        b.apply("settings.billing.state",Ok(json!({"ok":true,"logged_in":true,"can_charge":true,"cli_billing_enabled":true,"min_usd":"5","max_usd":"100","charge_presets":["10"],"auto_reload":{"enabled":true,"threshold_usd":"5","reload_to_usd":"20","card":{"kind":"canonical"}},"keep_going_on_credits":false})));
        b
    }
    #[test]
    fn payment_retries_same_intent_and_only_settlement_claims_credit() {
        let mut b = state();
        let first = b.act(Action::Charge);
        let key = first[0].1.body["idempotency_key"].clone();
        b.apply("settings.billing.charge", Err("Timed out".into()));
        let retry = b.act(Action::Charge);
        assert_eq!(retry[0].1.body["idempotency_key"], key);
        let poll = b.apply(
            "settings.billing.charge",
            Ok(json!({"ok":true,"charge_id":"charge-1"})),
        );
        assert_eq!(poll[0].1.path, "billing.charge_status");
        assert_eq!(poll[0].1.body["charge_id"], "charge-1");
        assert!(b.notice.is_none());
        assert!(b.blocked());
        b.apply(
            "settings.billing.charge-status",
            Ok(json!({"ok":true,"status":"settled","amount_usd":"10"})),
        );
        assert_eq!(b.charge_status, "Settled");
        assert!(b.notice.as_ref().unwrap().contains("Credits added"));
        assert!(b.intent.is_none());
    }
    #[test]
    fn accepted_charge_permission_loss_is_unconfirmed_and_never_resubmitted() {
        let mut b = state();
        b.act(Action::Charge);
        b.apply(
            "settings.billing.charge",
            Ok(json!({"ok":true,"charge_id":"charge"})),
        );
        assert!(
            b.apply(
                "settings.billing.charge-status",
                Ok(json!({"ok":false,"error":"insufficient_scope","message":"Permission changed"}))
            )
            .is_empty()
        );
        assert_eq!(b.charge_status, "Unconfirmed");
        assert!(b.intent.is_some());
        assert!(b.charge_id.is_none());
        assert!(b.notice.is_none());
    }
    #[test]
    fn auto_reload_disable_includes_current_amounts_and_validates_order() {
        let mut b = state();
        assert!(!b.auto_dirty());
        assert!(b.act(Action::AutoEnabled(false)).is_empty());
        assert!(b.confirm_disable);
        let e = b.act(Action::ConfirmDisable);
        assert_eq!(
            e[0].1.body,
            json!({"enabled":false,"threshold":"5","top_up_amount":"20"})
        );
        let mut b = state();
        b.threshold.replace_text("30");
        b.reload_to.replace_text("20");
        assert!(b.act(Action::AutoSave).is_empty());
        assert!(b.error.is_some());
    }
    #[test]
    fn keep_going_step_up_replays_only_exact_requested_switch_once() {
        let mut b = state();
        assert_eq!(
            b.act(Action::KeepGoing(true))[0].1.body,
            json!({"enabled":true})
        );
        let step = b.apply(
            "settings.billing.keep-going",
            Ok(json!({"ok":false,"error":"insufficient_scope"})),
        );
        assert_eq!(step[0].1.path, "billing.step_up");
        b.apply("settings.billing.step-up.event",Ok(json!({"event":"billing.step_up.verification","payload":{"verification_url":"https://example.invalid/device","user_code":"ABCD"}})));
        assert_eq!(b.verification_code, "ABCD");
        let replay = b.apply(
            "settings.billing.step-up",
            Ok(json!({"ok":true,"granted":true})),
        );
        assert_eq!(replay[0].1.path, "billing.keep_going_on_credits");
        assert_eq!(replay[0].1.body, json!({"enabled":true}));
        assert!(
            b.apply(
                "settings.billing.keep-going",
                Ok(json!({"ok":false,"error":"insufficient_scope"}))
            )
            .is_empty()
        );
        assert!(!b.stepping_up);
    }
    #[test]
    fn only_scheduled_quote_can_commit_and_read_errors_do_not_unlock_write() {
        let mut b = state();
        b.preview = Some(json!({"ok":true,"effect":"charge_now"}));
        assert!(b.act(Action::ConfirmPlan).is_empty());
        b.preview = Some(json!({"ok":true,"effect":"scheduled"}));
        b.preview_tier = "quoted-tier".into();
        let e = b.act(Action::ConfirmPlan);
        assert_eq!(e[0].1.body, json!({"subscription_type_id":"quoted-tier"}));
        b.apply("settings.billing.state", Err("Unavailable".into()));
        assert!(b.busy);
    }
}
