//! Client chrome uses confirmed profile policy and the focused runtime's metrics.
use crate::pages::{PageEffect, PageRequest};
use serde_json::{Value, json};
use std::time::Instant;
pub struct ClientStatus {
    pub approval: Option<String>,
    pub busy: bool,
    pub writing: bool,
    pub error: Option<String>,
    pub menu: bool,
    pub detail: Option<String>,
    pub hardware: Option<Value>,
    pub context: Option<Value>,
    pub resources_enabled: bool,
    clock: f64,
    next_hardware: f64,
    hardware_loading: bool,
    context_loading: bool,
    requested: Option<String>,
    focused: Option<crate::composer::DraftScope>,
    focus_started: Instant,
    turn_started: Option<Instant>,
    timer_labels: (Option<u64>, Option<u64>),
}
impl Default for ClientStatus {
    fn default() -> Self {
        Self::new()
    }
}
impl ClientStatus {
    pub fn new() -> Self {
        Self {
            approval: None,
            busy: false,
            writing: false,
            error: None,
            menu: false,
            detail: None,
            hardware: None,
            context: None,
            resources_enabled: rune::extra_args().iter().any(|v| v == "--local"),
            clock: 0.,
            next_hardware: 0.,
            hardware_loading: false,
            context_loading: false,
            requested: None,
            focused: None,
            focus_started: Instant::now(),
            turn_started: None,
            timer_labels: (None, None),
        }
    }
    pub fn reset(&mut self) {
        let enabled = self.resources_enabled;
        *self = Self::new();
        self.resources_enabled = enabled;
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.busy = true;
        vec![(
            "chrome.approval.read".into(),
            PageRequest::rpc("config.get", json!({"key":"approvals.mode"})),
        )]
    }
    pub fn set_approval(&mut self, mode: &str) -> Vec<PageEffect> {
        if self.busy || !["manual", "smart", "off"].contains(&mode) {
            return vec![];
        }
        self.busy = true;
        self.writing = true;
        self.requested = Some(mode.into());
        self.error = None;
        vec![(
            "chrome.approval.write".into(),
            PageRequest::rpc("config.set", json!({"key":"approvals.mode","value":mode})),
        )]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        match op {
            "chrome.approval.read" | "chrome.approval.readback" => {
                self.busy = false;
                self.writing = false;
                match result {
                    Ok(v) => {
                        if let Some(mode) = v["value"]
                            .as_str()
                            .filter(|s| ["manual", "smart", "off"].contains(s))
                        {
                            self.approval = Some(mode.into());
                            self.error = if self
                                .requested
                                .take()
                                .is_some_and(|wanted| wanted != mode)
                            {
                                Some("The agent retained a different approval policy; review the confirmed selection".into())
                            } else {
                                None
                            };
                        } else {
                            self.error =
                                Some("The agent did not confirm its approval policy".into());
                        }
                    }
                    Err(e) => {
                        self.approval = None;
                        self.error = Some(e);
                    }
                }
            }
            "chrome.approval.write" => {
                match result {
                    Err(error) => {
                        self.error = Some(format!("The policy change needs readback: {error}"))
                    }
                    Ok(value) if value["ok"] == false => {
                        self.error = Some("The agent refused the approval-policy change".into())
                    }
                    _ => {}
                }
                return vec![(
                    "chrome.approval.readback".into(),
                    PageRequest::rpc("config.get", json!({"key":"approvals.mode"})),
                )];
            }
            "chrome.hardware" => {
                self.hardware_loading = false;
                match result {
                    Ok(v) => self.hardware = Some(v),
                    Err(_) => self.hardware = None,
                }
            }
            "chrome.context" => {
                self.context_loading = false;
                match result {
                    Ok(v)
                        if v["context_max"].as_f64().unwrap_or(0.) > 0.
                            || v["categories"].as_array().is_some_and(|a| !a.is_empty()) =>
                    {
                        self.context = Some(v)
                    }
                    _ => self.context = None,
                }
            }
            _ => {}
        }
        vec![]
    }
    pub fn observe(&mut self, scope: crate::composer::DraftScope, running: bool) -> bool {
        let changed = self.focused.as_ref() != Some(&scope);
        if changed {
            self.focused = Some(scope);
            self.focus_started = Instant::now();
            self.turn_started = None;
            self.context = None;
            self.context_loading = false;
        }
        if running {
            self.turn_started.get_or_insert_with(Instant::now);
        } else {
            self.turn_started = None;
        }
        changed
    }
    pub fn poll(&mut self, now: f64, resources_shown: bool) -> Vec<PageEffect> {
        self.clock = now;
        if self.resources_enabled
            && resources_shown
            && !self.hardware_loading
            && now >= self.next_hardware
        {
            self.hardware_loading = true;
            self.next_hardware = now + 5.;
            return vec![(
                "chrome.hardware".into(),
                PageRequest::get("/api/local-models/hardware"),
            )];
        }
        vec![]
    }
    pub fn read_context(&mut self, runtime: Option<String>) -> Vec<PageEffect> {
        if self.context_loading {
            return vec![];
        }
        let Some(id) = runtime.filter(|v| !v.is_empty()) else {
            return vec![];
        };
        self.context_loading = true;
        vec![(
            "chrome.context".into(),
            PageRequest::rpc("session.context_breakdown", json!({"session_id":id})),
        )]
    }
    pub fn timers_changed(&mut self, turn: bool, focus: bool) -> bool {
        let labels = (
            turn.then(|| self.elapsed(true)).flatten(),
            focus.then(|| self.elapsed(false)).flatten(),
        );
        let changed = labels != self.timer_labels;
        self.timer_labels = labels;
        changed
    }
    pub fn elapsed(&self, turn: bool) -> Option<u64> {
        if turn {
            self.turn_started.map(|t| t.elapsed().as_secs())
        } else {
            self.focused
                .as_ref()
                .map(|_| self.focus_started.elapsed().as_secs())
        }
    }
    pub fn fixture(&mut self) {
        self.approval = Some("smart".into());
        self.busy = false;
    }
}
impl crate::app::Viewer {
    pub fn chrome_approval(&mut self, mode: String) {
        if self.is_fixture() || self.settings.blocks_owner_change() {
            return;
        }
        let effects = self.chrome.set_approval(&mode);
        self.page_requests(effects);
    }
    pub fn open_chrome_detail(&mut self, name: String) {
        self.chrome.detail = if self.chrome.detail.as_ref() == Some(&name) {
            None
        } else {
            Some(name.clone())
        };
        if name == "approval" && self.chrome.approval.is_none() {
            let effects = self.chrome.load();
            self.page_requests(effects);
        }
        if name == "context" {
            let runtime = self
                .conversation
                .view(&self.composer_scope())
                .map(|v| v.runtime.clone());
            let effects = self.chrome.read_context(runtime);
            self.page_requests(effects);
        }
    }
    pub fn status_item_visible(&self, id: &str) -> bool {
        !self.hidden_status_items.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_policy_is_confirmed_only_after_readback_and_failed_reads_do_not_display_stale_policy()
     {
        let mut state = ClientStatus::new();
        state.apply("chrome.approval.read", Ok(json!({"value":"smart"})));
        state.set_approval("off");
        assert_eq!(state.approval.as_deref(), Some("smart"));
        assert!(state.writing);
        let effects = state.apply("chrome.approval.write", Err("timeout".into()));
        assert_eq!(effects[0].0, "chrome.approval.readback");
        state.apply("chrome.approval.readback", Ok(json!({"value":"off"})));
        assert_eq!(state.approval.as_deref(), Some("off"));
        assert!(!state.writing);
        assert!(state.error.is_none());
        state.load();
        state.apply("chrome.approval.read", Err("offline".into()));
        assert!(state.approval.is_none());
    }
    #[test]
    fn focused_context_is_evicted_on_owner_change_and_resource_reads_are_capability_gated() {
        let mut state = ClientStatus::new();
        state.resources_enabled = false;
        assert!(state.poll(10., true).is_empty());
        state.context = Some(json!({"context_max":100}));
        state.observe(
            crate::composer::DraftScope {
                connection: 1,
                profile: "work".into(),
                session: Some("a".into()),
            },
            false,
        );
        assert!(state.context.is_none());
        state.apply(
            "chrome.context",
            Ok(json!({"context_max":0,"categories":[]})),
        );
        assert!(state.context.is_none());
    }
}
