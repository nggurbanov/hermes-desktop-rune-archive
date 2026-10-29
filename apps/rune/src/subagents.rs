//! Owner-scoped child rosters and separate native Rune spectator windows.
mod mailbox;
mod media;
mod process;
mod proof;
#[cfg(test)]
mod tests;
pub(crate) mod window;
mod worker;
use crate::{
    composer::DraftScope,
    conversation::ConversationView,
    gateway::{Connection, conversation::SessionTransport},
};
use rune::Waker;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};
pub use window::run_if_window_mode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildRow {
    pub id: String,
    pub session: Option<String>,
    pub parent: Option<String>,
    pub goal: String,
    pub status: String,
    pub model: String,
    pub activity: String,
    pub summary: String,
    pub task_index: u64,
}
impl ChildRow {
    pub fn terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "interrupted")
    }
}
#[derive(Default)]
struct Roster {
    runtime: String,
    version: u64,
    rows: Vec<ChildRow>,
    retired: HashSet<String>,
    pending: bool,
    failures: u8,
    next: Option<Instant>,
    conclusion: bool,
}
struct Reply {
    scope: DraftScope,
    runtime: String,
    version: u64,
    result: Result<Value, String>,
}
#[derive(Default)]
pub struct Subagents {
    rosters: HashMap<DraftScope, Roster>,
    pub expanded: HashSet<DraftScope>,
    windows: HashMap<(u64, String, String), process::NativeWindow>,
    tx: Option<Sender<Reply>>,
    rx: Option<Receiver<Reply>>,
    waker: Option<Waker>,
    next_poll: Option<Duration>,
    pub error: Option<String>,
}
impl Subagents {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn init(&mut self, waker: Waker) {
        let (tx, rx) = mpsc::channel();
        self.tx = Some(tx);
        self.rx = Some(rx);
        self.waker = Some(waker);
    }
    pub fn rows(&self, scope: &DraftScope) -> &[ChildRow] {
        self.rosters
            .get(scope)
            .map(|r| r.rows.as_slice())
            .unwrap_or(&[])
    }
    pub fn event(&mut self, scope: &DraftScope, kind: &str, payload: &Value) {
        if kind == "message.start" {
            self.turn_started(scope);
            return;
        }
        if !kind.starts_with("subagent.") {
            return;
        }
        let roster = self.rosters.entry(scope.clone()).or_default();
        roster.version += 1;
        roster.conclusion |= kind == "subagent.complete";
        merge_event(roster, kind, payload);
    }
    pub fn turn_started(&mut self, scope: &DraftScope) {
        let roster = self.rosters.entry(scope.clone()).or_default();
        roster.version += 1;
        for row in &roster.rows {
            if row.terminal() {
                roster.retired.insert(row.id.clone());
            }
        }
        roster.rows.retain(|r| !r.terminal());
    }
    pub fn observe(
        &mut self,
        connection: &Connection,
        scope: DraftScope,
        view: Option<&ConversationView>,
        theme: Option<bool>,
    ) -> bool {
        let changed = self.poll(theme);

        let Some(view) = view.filter(|v| !v.runtime.is_empty()) else {
            return changed;
        };
        let roster = self.rosters.entry(scope.clone()).or_default();
        if roster.runtime != view.runtime {
            roster.runtime = view.runtime.clone();
            roster.version += 1;
            roster.pending = false;
            roster.failures = 0;
            roster.next = None;
        }
        let active = view.running || roster.rows.iter().any(|row| !row.terminal());
        if roster.failures >= 3 || (!active && !roster.conclusion && roster.next.is_some()) {
            return changed;
        }
        if roster.pending {
            return changed;
        }
        self.next_poll = Some(
            roster
                .next
                .map(|next| next.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_secs(5)),
        );
        if roster.next.is_some_and(|v| v > Instant::now()) {
            return changed;
        }
        let (Some(tx), Some(waker)) = (self.tx.clone(), self.waker.clone()) else {
            return changed;
        };
        roster.pending = true;
        roster.next = Some(Instant::now() + Duration::from_secs(5));
        self.next_poll = Some(Duration::from_secs(5));
        let runtime = roster.runtime.clone();
        let version = roster.version;
        let connection = connection.clone();
        std::thread::spawn(move || {
            let result = SessionTransport::connect(&connection, &scope.profile)
                .map_err(|_| "The subagent roster is unavailable".to_owned())
                .and_then(|mut t| {
                    t.call("subagent.list", json!({"session_id":runtime}))
                        .map_err(|e| e.message)
                });
            if tx
                .send(Reply {
                    scope,
                    runtime,
                    version,
                    result,
                })
                .is_ok()
            {
                waker();
            }
        });
        changed
    }
    pub fn poll(&mut self, theme: Option<bool>) -> bool {
        self.next_poll = None;
        let replies: Vec<_> = self
            .rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        let mut changed = false;
        for reply in replies {
            let Some(roster) = self.rosters.get_mut(&reply.scope) else {
                continue;
            };
            if roster.runtime != reply.runtime {
                continue;
            }
            roster.pending = false;
            match reply.result {
                Ok(snapshot) => {
                    roster.failures = 0;
                    if roster.version == reply.version {
                        roster.conclusion = false;
                        let previous = roster.rows.clone();
                        reconcile(roster, &snapshot);
                        changed |= roster.rows != previous;
                    }
                }
                Err(_) => {
                    roster.failures = roster.failures.saturating_add(1);
                    roster.conclusion = true;
                }
            }
        }
        let mut failed = false;
        self.windows.retain(|_, window| {
            window.theme(theme);
            let exited = window.exited();
            failed |= window.startup_failed();
            !exited
        });
        if failed {
            self.error = Some(
                "The native subagent window could not be created. Try opening it again.".into(),
            );
            changed = true;
        }
        changed
    }
    pub fn wake_after(&self) -> Option<Duration> {
        if !self.windows.is_empty() {
            Some(
                self.next_poll
                    .unwrap_or(Duration::from_secs(1))
                    .min(Duration::from_secs(1)),
            )
        } else {
            self.next_poll
        }
    }
    pub fn open(
        &mut self,
        connection: &Connection,
        scope: &DraftScope,
        id: &str,
        theme: Option<bool>,
    ) -> Result<(), String> {
        let row = self
            .rows(scope)
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or("This subagent is no longer in the selected conversation")?;
        let child = row
            .session
            .as_ref()
            .ok_or("This subagent has not published its own session yet")?;
        let key = (scope.connection, scope.profile.clone(), child.clone());
        if let Some(window) = self.windows.get_mut(&key) {
            if !window.exited() {
                window.focus()?;
                return Ok(());
            }
        }
        let launch = process::Launch::new(connection, scope, &row, theme);
        self.windows
            .insert(key, process::NativeWindow::spawn(launch)?);
        Ok(())
    }
    pub fn close_connection(&mut self, epoch: u64) {
        self.rosters.retain(|s, _| s.connection != epoch);
        self.expanded.retain(|s| s.connection != epoch);
        self.windows.retain(|(e, _, _), _| *e != epoch);
    }
    pub fn close_profile(&mut self, epoch: u64, profile: &str) {
        self.rosters
            .retain(|s, _| s.connection != epoch || s.profile != profile);
        self.expanded
            .retain(|s| s.connection != epoch || s.profile != profile);
        self.windows
            .retain(|(e, p, _), _| *e != epoch || p != profile);
    }
    pub fn rehome(&mut self, origin: &DraftScope, target: &DraftScope) {
        let target = target.clone();
        if let Some(roster) = self.rosters.remove(origin) {
            self.rosters.entry(target.clone()).or_insert(roster);
        }
        if self.expanded.remove(origin) {
            self.expanded.insert(target);
        }
    }
    pub fn fixture(&mut self, scope: DraftScope) {
        self.event(&scope, "subagent.start", &json!({"subagent_id":"fixture-child","child_session_id":"fixture-child-session","goal":"Compare the eight proposals and check the source images","model":"GPT-6.1-Sol","task_index":0}));
        self.event(&scope, "subagent.progress", &json!({"subagent_id":"fixture-child","text":"Analyzed 16 images and explored 6 files"}));
        self.expanded.insert(scope);
    }
}
fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}
fn status(value: &Value, kind: &str) -> String {
    match string(value, "status").as_str() {
        "completed" => "completed",
        "failed" | "error" | "timeout" => "failed",
        "interrupted" | "cancelled" | "canceled" => "interrupted",
        _ if kind == "subagent.complete" => "failed",
        "queued" => "queued",
        _ => "running",
    }
    .into()
}
fn merge_event(roster: &mut Roster, kind: &str, payload: &Value) {
    let id = string(payload, "subagent_id");
    if id.is_empty() || roster.retired.contains(&id) {
        return;
    }
    if roster.rows.iter().any(|r| r.id == id && r.terminal()) {
        return;
    }
    let previous = roster.rows.iter().position(|r| r.id == id);
    let mut row = previous
        .map(|i| roster.rows[i].clone())
        .unwrap_or(ChildRow {
            id,
            session: None,
            parent: None,
            goal: "Subagent".into(),
            status: "running".into(),
            model: String::new(),
            activity: String::new(),
            summary: String::new(),
            task_index: 0,
        });
    for (key, target) in [
        ("goal", &mut row.goal),
        ("model", &mut row.model),
        ("summary", &mut row.summary),
    ] {
        let value = string(payload, key);
        if !value.is_empty() {
            *target = value;
        }
    }
    for (key, target) in [
        ("child_session_id", &mut row.session),
        ("parent_id", &mut row.parent),
    ] {
        let value = string(payload, key);
        if !value.is_empty() {
            *target = Some(value);
        }
    }
    row.status = status(payload, kind);
    row.task_index = payload
        .get("task_index")
        .and_then(Value::as_u64)
        .unwrap_or(row.task_index);
    let tool = string(payload, "tool_name");
    let activity = string(payload, "text");
    if !activity.is_empty() {
        row.activity = activity;
    } else if !tool.is_empty() {
        row.activity = tool;
    }
    if row.status == "failed" && row.summary.is_empty() && string(payload, "status") == "timeout" {
        row.summary = format!(
            "Timed out after {}s",
            payload.get("duration_seconds").unwrap_or(&Value::Null)
        );
    }
    if let Some(index) = previous {
        roster.rows[index] = row;
    } else {
        roster.rows.push(row);
    }
}
fn reconcile(roster: &mut Roster, snapshot: &Value) {
    let Some(rows) = snapshot.get("subagents").and_then(Value::as_array) else {
        return;
    };
    let ids: HashSet<_> = rows.iter().map(|r| string(r, "subagent_id")).collect();
    roster
        .rows
        .retain(|row| row.terminal() || ids.contains(&row.id));
    for row in rows {
        merge_event(roster, "subagent.snapshot", row);
    }
    if let Some(rows) = snapshot.get("delegations").and_then(Value::as_array) {
        for row in rows {
            if string(row, "status") != "failed" {
                continue;
            }
            let delegation = string(row, "delegation_id");
            if delegation.is_empty() {
                continue;
            }
            let mut row = row.clone();
            row["subagent_id"] = json!(format!(
                "delegation:{delegation}:{}",
                row.get("task_index").and_then(Value::as_u64).unwrap_or(0)
            ));
            row["summary"] = row.get("error").cloned().unwrap_or(Value::Null);
            merge_event(roster, "subagent.complete", &row);
        }
    }
}
