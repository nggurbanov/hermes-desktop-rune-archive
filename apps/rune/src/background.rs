//! Background process status follows process.list snapshots, independently of terminal visibility.
mod reconcile;
#[cfg(test)]
mod tests;
mod worker;
use crate::{composer::DraftScope, source::Binding};
use rune::Waker;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Done,
    Failed,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub output: String,
    pub exit_code: Option<i64>,
    pub state: State,
    pub stopping: bool,
    pub until: Option<f64>,
}
struct Entry {
    binding: Binding,
    active: Arc<AtomicBool>,
    runtime: String,
    generation: u64,
    items: Vec<Item>,
    dismissed: HashSet<String>,
    reading: Option<u64>,
    kills: HashMap<u64, String>,
    refresh: bool,
    next_read: f64,
    gone: bool,
    pub error: Option<String>,
}
pub enum Update {
    Snapshot {
        scope: DraftScope,
        value: Value,
    },
    Finished {
        scope: DraftScope,
        runtime: String,
        title: String,
        body: String,
    },
    Changed,
    RuntimeGone {
        scope: DraftScope,
        runtime: String,
    },
}
pub struct Background {
    entries: HashMap<DraftScope, Entry>,
    serial: u64,
    requests: u64,
    send: Option<mpsc::SyncSender<worker::Work>>,
    replies: Option<mpsc::Receiver<worker::Reply>>,
    pub initialized: bool,
    pub expanded: Option<(DraftScope, String)>,
    pub revision: u64,
}
impl Default for Background {
    fn default() -> Self {
        Self::new()
    }
}
impl Background {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            serial: 0,
            requests: 0,
            send: None,
            replies: None,
            initialized: false,
            expanded: None,
            revision: 0,
        }
    }
    pub fn fixture(&mut self, scope: DraftScope) {
        self.bind(
            Binding::new(
                crate::gateway::Connection::Url {
                    endpoint: "http://127.0.0.1:1".into(),
                    token: None,
                },
                None,
            )
            .expect("fixture source is valid"),
            scope.clone(),
            "fixture-background".into(),
        );
        let entry = self.entries.get_mut(&scope).unwrap();
        let rows = serde_json::json!({"processes":[
            {"session_id":"build","status":"running","command":"Build the desktop app","output_tail":"Compiling the native workspace…"},
            {"session_id":"tests","status":"running","command":"Run the test suite"},
            {"session_id":"lint","status":"running","command":"Check formatting"}
        ]});
        reconcile::snapshot(entry, &rows, 0.);
        let rows = serde_json::json!({"processes":[
            {"session_id":"build","status":"running","command":"Build the desktop app","output_tail":"Compiling the native workspace…\nLinking Hermes…"},
            {"session_id":"tests","status":"exited","exit_code":0,"command":"Run the test suite","output_tail":"All checks passed."},
            {"session_id":"lint","status":"exited","exit_code":1,"command":"Check formatting","output_tail":"Two files need formatting."}
        ]});
        reconcile::snapshot(entry, &rows, 0.);
        self.expanded = Some((scope, "build".into()));
        self.revision += 1;
    }
    pub fn init(&mut self, waker: Waker) {
        if self.initialized {
            return;
        }
        let (send, replies) = worker::start(waker);
        self.send = Some(send);
        self.replies = Some(replies);
        self.initialized = true;
    }
    pub fn bind(&mut self, binding: Binding, scope: DraftScope, runtime: String) {
        if runtime.is_empty() || scope.session.is_none() || binding.validate().is_err() {
            return;
        }
        if self
            .entries
            .get(&scope)
            .is_some_and(|e| e.runtime == runtime && e.binding.stamp() == binding.stamp())
        {
            return;
        }
        self.serial += 1;
        if let Some(old) = self.entries.get(&scope) {
            old.active.store(false, Ordering::Release);
        }
        self.entries.insert(
            scope,
            Entry {
                binding,
                active: Arc::new(AtomicBool::new(true)),
                runtime,
                generation: self.serial,
                items: vec![],
                dismissed: HashSet::new(),
                reading: None,
                kills: HashMap::new(),
                refresh: true,
                next_read: 0.,
                gone: false,
                error: None,
            },
        );
        self.revision += 1;
    }
    pub fn trigger(&mut self, scope: &DraftScope, runtime: &str) {
        if let Some(entry) = self
            .entries
            .get_mut(scope)
            .filter(|e| e.runtime == runtime && !e.gone)
        {
            entry.refresh = true;
        }
    }
    pub fn stream_opened(&mut self, scope: &DraftScope) {
        for (owner, entry) in &mut self.entries {
            if owner.connection == scope.connection && owner.profile == scope.profile {
                entry.gone = false;
                entry.refresh = true;
            }
        }
    }
    pub fn retain_connections(&mut self, connections: &[u64]) {
        self.entries.retain(|scope, entry| {
            let keep = connections.contains(&scope.connection) && entry.binding.validate().is_ok();
            if !keep {
                entry.active.store(false, Ordering::Release);
            }
            keep
        });
        if self
            .expanded
            .as_ref()
            .is_some_and(|(scope, _)| !self.entries.contains_key(scope))
        {
            self.expanded = None;
        }
    }
    pub fn retire_profile(&mut self, connection: u64, profile: &str) {
        self.entries.retain(|scope, entry| {
            let keep = scope.connection != connection || scope.profile != profile;
            if !keep {
                entry.active.store(false, Ordering::Release);
            }
            keep
        });
    }
    pub fn retire_connection(&mut self, connection: u64) {
        self.entries.retain(|scope, entry| {
            let keep = scope.connection != connection;
            if !keep {
                entry.active.store(false, Ordering::Release);
            }
            keep
        });
        if self
            .expanded
            .as_ref()
            .is_some_and(|(scope, _)| scope.connection == connection)
        {
            self.expanded = None;
        }
        self.revision += 1;
    }
    pub fn items(&self, scope: &DraftScope) -> &[Item] {
        self.entries
            .get(scope)
            .map(|e| e.items.as_slice())
            .unwrap_or(&[])
    }
    pub fn error(&self, scope: &DraftScope) -> Option<&str> {
        self.entries.get(scope).and_then(|e| e.error.as_deref())
    }
    pub fn toggle_output(&mut self, scope: DraftScope, id: String) {
        let key = (scope, id);
        self.expanded = if self.expanded.as_ref() == Some(&key) {
            None
        } else {
            Some(key)
        };
        self.revision += 1;
    }
    pub fn dismiss(&mut self, scope: &DraftScope, id: &str) {
        if let Some(entry) = self.entries.get_mut(scope)
            && entry
                .items
                .iter()
                .any(|item| item.id == id && item.state != State::Running)
        {
            entry.dismissed.insert(id.into());
            entry.items.retain(|item| item.id != id);
            self.revision += 1;
        }
    }
    pub fn stop(&mut self, scope: &DraftScope, id: &str) {
        let Some(entry) = self.entries.get_mut(scope) else {
            return;
        };
        if entry.gone {
            entry.dismissed.insert(id.into());
            entry.items.retain(|item| item.id != id);
            self.revision += 1;
            return;
        }
        if !entry
            .items
            .iter()
            .any(|item| item.id == id && item.state == State::Running && !item.stopping)
        {
            return;
        }
        self.requests += 1;
        let request = self.requests;
        let work = worker::Work {
            scope: scope.clone(),
            runtime: entry.runtime.clone(),
            generation: entry.generation,
            id: request,
            binding: entry.binding.clone(),
            active: entry.active.clone(),
            action: worker::Action::Kill(id.into()),
        };
        if self
            .send
            .as_ref()
            .is_some_and(|send| send.try_send(work).is_ok())
        {
            entry.kills.insert(request, id.into());
            entry
                .items
                .iter_mut()
                .filter(|item| item.id == id)
                .for_each(|item| item.stopping = true);
            entry.error = None;
        } else {
            entry.error = Some(
                "The process stop request could not be queued. The process is still shown.".into(),
            );
        }
        self.revision += 1;
    }
    /// A rewind abandons its timeline; dismiss before the asynchronous kill can return.
    pub fn rewind(&mut self, scope: &DraftScope) {
        let Some(entry) = self.entries.get_mut(scope) else {
            return;
        };
        let running = entry
            .items
            .iter()
            .filter(|item| item.state == State::Running)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        entry
            .dismissed
            .extend(entry.items.iter().map(|item| item.id.clone()));
        entry.items.clear();
        entry.reading = None;
        entry.refresh = true;
        self.revision += 1;
        if !entry.gone {
            for process in running {
                self.requests += 1;
                let work = worker::Work {
                    scope: scope.clone(),
                    runtime: entry.runtime.clone(),
                    generation: entry.generation,
                    id: self.requests,
                    binding: entry.binding.clone(),
                    active: entry.active.clone(),
                    action: worker::Action::RewindKill(process),
                };
                if self
                    .send
                    .as_ref()
                    .is_some_and(|send| send.try_send(work).is_ok())
                {
                } else {
                    entry.error = Some(
                        "Some abandoned background processes could not be queued for stopping."
                            .into(),
                    );
                }
            }
        }
    }
    pub fn poll(
        &mut self,
        now: f64,
        visible_scope: &DraftScope,
        host_visible: bool,
    ) -> Vec<Update> {
        let mut updates = vec![];
        let replies = self
            .replies
            .as_ref()
            .map(|rx| rx.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for reply in replies {
            self.apply(reply, now, &mut updates);
        }
        for (scope, entry) in &mut self.entries {
            let expired = entry
                .items
                .iter()
                .filter(|item| item.until.is_some_and(|until| now >= until))
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            if !expired.is_empty() {
                entry.dismissed.extend(expired.iter().cloned());
                entry.items.retain(|item| !expired.contains(&item.id));
                self.revision += 1;
                updates.push(Update::Changed);
            }
            let safety = host_visible
                && scope == visible_scope
                && entry.items.iter().any(|item| item.state == State::Running)
                && now >= entry.next_read;
            if entry.gone || entry.reading.is_some() || !entry.refresh && !safety {
                continue;
            }
            self.requests += 1;
            let work = worker::Work {
                scope: scope.clone(),
                runtime: entry.runtime.clone(),
                generation: entry.generation,
                id: self.requests,
                binding: entry.binding.clone(),
                active: entry.active.clone(),
                action: worker::Action::Read,
            };
            if self
                .send
                .as_ref()
                .is_some_and(|send| send.try_send(work).is_ok())
            {
                entry.reading = Some(self.requests);
                entry.refresh = false;
                entry.next_read = now + 5.;
            }
        }
        updates
    }
    pub fn next_wake_after(
        &self,
        now: f64,
        visible_scope: &DraftScope,
        host_visible: bool,
    ) -> Option<std::time::Duration> {
        let mut delay: Option<f64> = None;
        for (scope, entry) in &self.entries {
            for until in entry.items.iter().filter_map(|item| item.until) {
                let wait = (until - now).max(0.001);
                delay = Some(delay.map_or(wait, |old| old.min(wait)));
            }
            if entry.gone || entry.reading.is_some() || !self.initialized {
                continue;
            }
            let running = host_visible
                && scope == visible_scope
                && entry.items.iter().any(|item| item.state == State::Running);
            let wait = if entry.refresh {
                Some(0.1)
            } else if running {
                Some((entry.next_read - now).max(0.1))
            } else {
                None
            };
            if let Some(wait) = wait {
                delay = Some(delay.map_or(wait, |old| old.min(wait)));
            }
        }
        delay.map(std::time::Duration::from_secs_f64)
    }
    fn apply(&mut self, reply: worker::Reply, now: f64, updates: &mut Vec<Update>) {
        let Some(entry) = self
            .entries
            .get_mut(&reply.scope)
            .filter(|e| e.runtime == reply.runtime && e.generation == reply.generation)
        else {
            return;
        };
        match reply.action {
            worker::Action::Read => {
                if entry.reading != Some(reply.id) {
                    return;
                }
                entry.reading = None;
                match reply.result {
                    Ok(value) => {
                        let result = reconcile::snapshot(entry, &value, now);
                        for (title, body) in result {
                            updates.push(Update::Finished {
                                scope: reply.scope.clone(),
                                runtime: entry.runtime.clone(),
                                title,
                                body,
                            });
                        }
                        updates.push(Update::Snapshot {
                            scope: reply.scope,
                            value,
                        });
                    }
                    Err(error) => {
                        if gone(&error) {
                            entry.gone = true;
                            updates.push(Update::RuntimeGone {
                                scope: reply.scope.clone(),
                                runtime: entry.runtime.clone(),
                            });
                        }
                    }
                }
            }
            worker::Action::Kill(process) => {
                if entry.kills.remove(&reply.id).as_deref() != Some(process.as_str()) {
                    return;
                }
                entry
                    .items
                    .iter_mut()
                    .filter(|item| item.id == process)
                    .for_each(|item| item.stopping = false);
                match reply.result{
                    Ok(value)if kill_confirmed(&value)=>{entry.dismissed.insert(process.clone());entry.items.retain(|item|item.id!=process);entry.error=None;},
                    Err(error)if gone(&error)=>{entry.gone=true;entry.dismissed.insert(process.clone());entry.items.retain(|item|item.id!=process);},
                    Ok(value)=>entry.error=Some(value["error"].as_str().unwrap_or("The gateway did not confirm stopping this process. It is still shown.").chars().take(500).collect()),
                    Err(error)=>entry.error=Some(format!("The process could not be stopped: {}",error.chars().take(400).collect::<String>())),
                }
            }
            worker::Action::RewindKill(_) => {
                if let Err(error) = reply.result
                    && gone(&error)
                {
                    entry.gone = true;
                }
            }
        }
        self.revision += 1;
        updates.push(Update::Changed);
    }
}
fn kill_confirmed(value: &Value) -> bool {
    matches!(
        value["status"].as_str(),
        Some("killed" | "already_exited" | "not_found")
    ) || value["ok"].as_bool() == Some(true)
}
fn gone(error: &str) -> bool {
    if let Some((_, reason)) = error.split_once("; reason=") {
        return reason.trim_end_matches(')').trim() == "not_found";
    }
    if error == "Workspace RPC refused (4001)" || error == "Process RPC refused (4001)" {
        return true;
    }
    let value = error
        .trim()
        .trim_start_matches("Error:")
        .trim()
        .to_ascii_lowercase();
    matches!(
        value.as_str(),
        "session not found"
            | "session not found."
            | "session not found!"
            | "4001 session not found"
            | "4001: session not found"
    )
}

impl Drop for Background {
    fn drop(&mut self) {
        for entry in self.entries.values() {
            entry.active.store(false, Ordering::Release);
        }
    }
}
