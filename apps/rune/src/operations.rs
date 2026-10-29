//! Command Center and gateway recovery use the selected external agent's APIs.
mod actions;
mod files;
mod replies;
#[cfg(test)]
mod tests;
use crate::{
    gateway::sidebar::SidebarSession,
    pages::{PageEffect, PageRequest, segment},
};
pub use actions::Action;
use rune::TextEdit;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Doctor,
    Security,
    Backup,
    Curator,
    Restart,
    Update,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Doctor => "Doctor",
            Self::Security => "Security audit",
            Self::Backup => "Backup",
            Self::Curator => "Skill curator",
            Self::Restart => "Restart gateway",
            Self::Update => "Update Hermes agent",
        }
    }
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Doctor => "/api/ops/doctor",
            Self::Security => "/api/ops/security-audit",
            Self::Backup => "/api/ops/backup",
            Self::Curator => "/api/curator/run",
            Self::Restart => "/api/gateway/restart",
            Self::Update => "/api/hermes/update",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Doctor => "doctor",
            Self::Security => "security-audit",
            Self::Backup => "backup",
            Self::Curator => "curator",
            Self::Restart => "gateway-restart",
            Self::Update => "hermes-update",
        }
    }
}
#[derive(Clone)]
pub struct Task {
    pub kind: Kind,
    pub name: String,
    pub pid: Option<u64>,
    pub action_id: Option<String>,
    pub running: bool,
    pub accepted: bool,
    pub settled: bool,
    pub exit_code: Option<i64>,
    pub receipt: Option<Value>,
    pub lines: Vec<String>,
    pub archive: Option<String>,
    pub error: Option<String>,
    pub polling: bool,
    pub polls: u16,
    pub answered: bool,
    pub recovered: bool,
    pub next: f64,
}
impl Task {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            name: kind.name().into(),
            pid: None,
            action_id: None,
            running: true,
            accepted: false,
            settled: false,
            exit_code: None,
            receipt: None,
            lines: vec![],
            archive: None,
            error: None,
            polling: false,
            polls: 0,
            answered: false,
            recovered: false,
            next: 0.0,
        }
    }
}
#[derive(Clone)]
pub enum Confirmation {
    Action(Kind),
    Memory(String),
}
pub struct Export {
    pub row: SidebarSession,
    pub path: PathBuf,
    pub messages: Vec<Value>,
    pub resolved: String,
    pub offset: usize,
    pub bytes: usize,
    pub pages: usize,
    pub seen: BTreeSet<String>,
}
pub struct Operations {
    pub section: String,
    pub search: TextEdit,
    pub log_search: TextEdit,
    pub log_file: String,
    pub log_level: String,
    pub select: Option<String>,
    pub log_follow: bool,
    pub status: Option<Value>,
    pub setup: Option<Value>,
    pub runtime: Option<Value>,
    pub update: Option<Value>,
    pub logs: Vec<String>,
    pub health_logs: Vec<String>,
    pub health_logs_loading: bool,
    pub health_logs_error: Option<String>,
    pub usage: Option<Value>,
    pub period: u16,
    pub curator: Option<Value>,
    pub memory: Option<Value>,
    pub share: Option<Value>,
    pub task: Option<Task>,
    pub confirm: Option<Confirmation>,
    pub status_loading: bool,
    pub logs_loading: bool,
    pub usage_loading: bool,
    pub maintenance_loading: bool,
    pub update_loading: bool,
    pub starting: bool,
    pub sharing: bool,
    pub curator_busy: bool,
    pub memory_busy: bool,
    pub downloading: bool,
    pub exporting: bool,
    pub saving_file: bool,
    pub system_error: Option<String>,
    pub readiness_error: Option<String>,
    pub logs_error: Option<String>,
    pub usage_error: Option<String>,
    pub maintenance_error: Option<String>,
    pub export_error: Option<String>,
    pub notice: Option<String>,
    pub demo: bool,
    visible: bool,
    health_visible: bool,
    clock: f64,
    next_status: f64,
    next_logs: f64,
    next_health_logs: f64,
    scope_epoch: u64,
    worker: Option<files::Writer>,
    redraw: bool,
    export: Option<Export>,
    pending_kind: Option<Kind>,
    pending_curator: Option<bool>,
    reconnect_followthrough: bool,
    detached: TextEdit,
}
impl Default for Operations {
    fn default() -> Self {
        Self::new()
    }
}
impl Operations {
    pub fn new() -> Self {
        Self {
            section: "sessions".into(),
            search: TextEdit::new(""),
            log_search: TextEdit::new(""),
            log_file: "agent".into(),
            log_level: "ALL".into(),
            select: None,
            log_follow: true,
            status: None,
            setup: None,
            runtime: None,
            update: None,
            logs: vec![],
            health_logs: vec![],
            health_logs_loading: false,
            health_logs_error: None,
            usage: None,
            period: 30,
            curator: None,
            memory: None,
            share: None,
            task: None,
            confirm: None,
            status_loading: false,
            logs_loading: false,
            usage_loading: false,
            maintenance_loading: false,
            update_loading: false,
            starting: false,
            sharing: false,
            curator_busy: false,
            memory_busy: false,
            downloading: false,
            exporting: false,
            saving_file: false,
            system_error: None,
            readiness_error: None,
            logs_error: None,
            usage_error: None,
            maintenance_error: None,
            export_error: None,
            notice: None,
            demo: false,
            visible: false,
            health_visible: false,
            clock: 0.0,
            next_status: 0.0,
            next_logs: 0.0,
            next_health_logs: 0.0,
            scope_epoch: 0,
            worker: None,
            redraw: false,
            export: None,
            pending_kind: None,
            pending_curator: None,
            reconnect_followthrough: false,
            detached: TextEdit::new(""),
        }
    }
    pub fn init(&mut self, waker: rune::Waker) {
        if self.worker.is_none() {
            self.worker = Some(files::Writer::new(waker));
        }
    }
    pub fn reset(&mut self) {
        let worker = self.worker.take();
        let epoch = self.scope_epoch.wrapping_add(1);
        *self = Self::new();
        self.worker = worker;
        self.scope_epoch = epoch;
    }
    pub fn observe(&mut self, center: bool, health: bool) {
        if !self.demo && (center || health) && !self.visible && !self.health_visible {
            self.setup = None;
            self.runtime = None;
            self.readiness_error = None;
        }
        self.visible = center;
        self.health_visible = health;
    }
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }
    pub fn take_reconnect_followthrough(&mut self) -> bool {
        std::mem::take(&mut self.reconnect_followthrough)
    }
    pub fn blocks_owner_change(&self) -> bool {
        self.starting
            || self.sharing
            || self.curator_busy
            || self.memory_busy
            || self.exporting
            || self.saving_file
            || self.downloading
            || self.confirm.is_some()
    }
    pub fn edit_detached(&mut self) -> &mut TextEdit {
        &mut self.detached
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        if self.demo {
            return vec![];
        }
        match self.section.as_str() {
            "system" => self.system_load(),
            "usage" => self.usage_load(),
            "maintenance" => self.maintenance_load(),
            _ => vec![],
        }
    }
    pub fn check_health(&mut self) -> Vec<PageEffect> {
        if self.demo {
            return vec![];
        }
        self.setup = None;
        self.runtime = None;
        self.readiness_error = None;
        self.health_load()
    }
    fn health_load(&mut self) -> Vec<PageEffect> {
        if self.status_loading {
            return vec![];
        }
        self.status_loading = true;
        self.next_status = self.clock + 5.0;
        let mut effects = vec![("ops.status".into(), PageRequest::get("/api/status"))];
        if self.runtime.is_none() && self.setup.is_none() {
            effects.extend([
                (
                    "ops.setup".into(),
                    PageRequest::rpc("setup.status", json!({})),
                ),
                (
                    "ops.runtime".into(),
                    PageRequest::rpc("setup.runtime_check", json!({})),
                ),
            ]);
        }
        effects
    }
    fn health_logs_load(&mut self) -> Vec<PageEffect> {
        if self.health_logs_loading {
            return vec![];
        }
        self.health_logs_loading = true;
        self.next_health_logs = self.clock + 1.5;
        vec![(
            "ops.health-logs".into(),
            PageRequest::get("/api/logs")
                .query("file", "gui")
                .query("lines", 40),
        )]
    }
    fn system_load(&mut self) -> Vec<PageEffect> {
        self.setup = None;
        self.runtime = None;
        let mut e = self.health_load();
        e.extend(self.logs_load());
        e.extend(self.update_load(false));
        e
    }
    fn log_operation(&self) -> String {
        format!("ops.logs.{}.{}", self.log_file, self.log_level)
    }
    fn logs_load(&mut self) -> Vec<PageEffect> {
        if self.logs_loading {
            return vec![];
        }
        self.logs_loading = true;
        self.next_logs = self.clock + 1.5;
        let mut request = PageRequest::get("/api/logs")
            .query("file", self.log_file.clone())
            .query("lines", 200);
        if self.log_level != "ALL" {
            request = request.query("level", self.log_level.clone());
        }
        vec![(self.log_operation(), request)]
    }
    fn usage_load(&mut self) -> Vec<PageEffect> {
        self.usage_loading = true;
        self.usage_error = None;
        vec![(
            format!("ops.usage.{}", self.period),
            PageRequest::get("/api/analytics/usage").query("days", self.period),
        )]
    }
    fn update_load(&mut self, force: bool) -> Vec<PageEffect> {
        if self.update_loading {
            return vec![];
        }
        self.update_loading = true;
        vec![(
            "ops.update-check".into(),
            PageRequest::get("/api/hermes/update/check").query("force", force),
        )]
    }
    fn maintenance_load(&mut self) -> Vec<PageEffect> {
        self.maintenance_loading = true;
        vec![
            ("ops.curator".into(), PageRequest::get("/api/curator")),
            ("ops.memory".into(), PageRequest::get("/api/memory")),
        ]
    }
    fn task_poll(&mut self) -> Vec<PageEffect> {
        let Some(task) = &mut self.task else {
            return vec![];
        };
        if task.polling {
            return vec![];
        }
        task.polling = true;
        vec![(
            "ops.action-status".into(),
            PageRequest::get(format!("/api/actions/{}/status", segment(&task.name)))
                .query("lines", 200),
        )]
    }
    pub fn poll(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        self.redraw |= self.poll_writer();
        if self.demo {
            return vec![];
        }
        let mut e = vec![];
        if (self.visible && self.section == "system" || self.health_visible)
            && now >= self.next_status
        {
            e.extend(self.health_load());
        }
        if self.visible && self.section == "system" && self.log_follow && now >= self.next_logs {
            e.extend(self.logs_load());
        }
        if self.health_visible && now >= self.next_health_logs {
            e.extend(self.health_logs_load());
        }
        if self.task.as_ref().is_some_and(|t| {
            t.running
                && !t.polling
                && now >= t.next
                && t.polls < if t.kind == Kind::Restart { 18 } else { 240 }
        }) {
            e.extend(self.task_poll());
        }
        e
    }
    pub fn wake_after(&self) -> Option<std::time::Duration> {
        let mut times = vec![];
        if self.visible && self.section == "system" || self.health_visible {
            if !self.status_loading {
                times.push(self.next_status);
            }
            if self.visible && self.section == "system" && self.log_follow && !self.logs_loading {
                times.push(self.next_logs);
            }
            if self.health_visible && !self.health_logs_loading {
                times.push(self.next_health_logs);
            }
        }
        if let Some(t) = &self.task {
            if t.running && !t.polling && t.polls < if t.kind == Kind::Restart { 18 } else { 240 } {
                times.push(t.next);
            }
        }
        times
            .into_iter()
            .min_by(f64::total_cmp)
            .map(|n| std::time::Duration::from_secs_f64((n - self.clock).max(0.016)))
    }
    pub fn visible_logs(&self) -> Vec<&str> {
        let q = self.log_search.text.to_lowercase();
        self.logs
            .iter()
            .filter(|l| q.is_empty() || l.to_lowercase().contains(&q))
            .map(String::as_str)
            .collect()
    }
    pub fn ready(&self) -> Option<bool> {
        self.runtime
            .as_ref()
            .and_then(|v| v["ok"].as_bool())
            .or_else(|| {
                self.setup
                    .as_ref()
                    .and_then(|v| v["provider_configured"].as_bool())
            })
    }
    pub fn readiness_reason(&self) -> String {
        if self.ready() == Some(true) {
            return String::new();
        }
        if self.ready().is_none() {
            return self.readiness_error.clone().unwrap_or_else(|| {
                "The agent has not provided an authoritative inference readiness result.".into()
            });
        }
        self.runtime
            .as_ref()
            .and_then(|v| v["error"].as_str())
            .or_else(|| self.setup.as_ref().and_then(|v| v["error"].as_str()))
            .unwrap_or("Configure a provider before sending a message.")
            .into()
    }
    pub fn fixture(&mut self) {
        self.demo = true;
        self.status = Some(
            json!({"version":"0.22.5","gateway_running":true,"gateway_state":"running","active_sessions":3,"gateway_shared_with":["default","research"],"gateway_platforms":{"telegram":{"connected":true,"state":"running"}},"hermes_home":"/sample/agent"}),
        );
        self.runtime = Some(json!({"ok":true,"model":"sample-model","provider":"sample"}));
        self.setup = Some(json!({"provider_configured":true}));
        self.logs = vec![
            "INFO Gateway ready".into(),
            "INFO Profile research connected".into(),
        ];
        self.curator =
            Some(json!({"enabled":true,"paused":false,"interval_hours":12,"last_run_at":null}));
        self.memory = Some(
            json!({"active":"builtin","providers":[],"builtin_files":{"memory":1024,"user":320}}),
        );
        self.usage = Some(
            json!({"period_days":30,"totals":{"total_sessions":12,"total_api_calls":84,"total_input":24000,"total_output":8500,"total_reasoning":2200,"total_cache_read":18000,"total_actual_cost":1.25},"daily":[{"day":"2026-10-27","input_tokens":2000,"output_tokens":800},{"day":"2026-10-28","input_tokens":4000,"output_tokens":1200},{"day":"2026-10-29","input_tokens":3000,"output_tokens":900}],"by_model":[{"model":"sample-model","sessions":12,"api_calls":84,"input_tokens":24000,"output_tokens":8500}],"skills":{"summary":{"distinct_skills_used":2,"total_skill_actions":14,"total_skill_loads":12,"total_skill_edits":2},"top_skills":[{"skill":"research","total_count":8},{"skill":"writing","total_count":6}]}}),
        );
        let args = rune::extra_args();
        if let Some(s) = args
            .windows(2)
            .find(|v| v[0] == "--operations-section")
            .map(|v| v[1].clone())
        {
            self.section = s;
        }
    }
}
