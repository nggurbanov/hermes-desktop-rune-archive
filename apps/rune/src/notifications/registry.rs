//! Origin routing and retention are independent of the native service.
use super::{
    Activation,
    service::Clients,
    types::{Reply, Request},
};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

pub(super) struct Live {
    pub request: Request,
    pub until: Instant,
    pub token: Option<String>,
    pub actions: bool,
}
#[derive(Clone)]
pub(super) enum Signal {
    Action(String),
    Closed,
    Token(String),
}
pub(super) struct Registry {
    pub live: HashMap<(String, u32), Live>,
    early: VecDeque<(String, u32, Signal, Instant)>,
}
impl Registry {
    pub fn new() -> Self {
        Self {
            live: HashMap::new(),
            early: VecDeque::new(),
        }
    }
    pub fn insert(
        &mut self,
        owner: String,
        id: u32,
        request: Request,
        actions: bool,
        clients: &Clients,
        now: Instant,
    ) -> bool {
        self.live.insert(
            (owner.clone(), id),
            Live {
                request,
                until: now + Duration::from_secs(600),
                token: None,
                actions,
            },
        );
        let mut closed = false;
        for (_, _, signal, _) in self
            .early
            .iter()
            .filter(|(sender, signal_id, _, _)| sender == &owner && *signal_id == id)
            .cloned()
            .collect::<Vec<_>>()
        {
            closed |= self.signal(&owner, id, signal, clients);
        }
        self.early
            .retain(|(sender, signal_id, _, _)| sender != &owner || *signal_id != id);
        closed
    }
    pub fn receive(
        &mut self,
        owner: &str,
        id: u32,
        signal: Signal,
        clients: &Clients,
        pending: bool,
        now: Instant,
    ) -> bool {
        if self.live.contains_key(&(owner.into(), id)) {
            return self.signal(owner, id, signal, clients);
        }
        self.early
            .retain(|(_, _, _, at)| now.saturating_duration_since(*at) < Duration::from_secs(5));
        if pending && self.early.len() < 128 {
            self.early.push_back((owner.into(), id, signal, now));
        }
        false
    }
    fn signal(&mut self, owner: &str, id: u32, signal: Signal, clients: &Clients) -> bool {
        let key = (owner.into(), id);
        if let Signal::Token(token) = signal {
            if let Some(live) = self.live.get_mut(&key) {
                live.token = Some(token);
            }
            return false;
        }
        if matches!(signal, Signal::Closed) {
            self.live.remove(&key);
            return false;
        }
        let Signal::Action(action) = signal else {
            return false;
        };
        let Some(live) = self.live.get(&key) else {
            return false;
        };
        let selected = if action == "default" {
            None
        } else {
            if !live.actions {
                return false;
            }
            let Ok(index) = action.parse::<usize>() else {
                return false;
            };
            if index.to_string() != action {
                return false;
            }
            let Some(button) = live.request.notice.buttons.get(index) else {
                return false;
            };
            Some(button.action.clone())
        };
        let live = self.live.remove(&key).unwrap();
        let mut activation = Activation {
            principal: live.request.principal,
            scope: live.request.notice.scope,
            runtime: live.request.notice.runtime,
            action: selected,
            token: live.token,
        };
        if let Some(source) = clients.get(&live.request.client) {
            if source.send(Reply::Activated(activation.clone())) {
                return true;
            }
        }
        // A missing source never lends its runtime action to a different window.
        if activation.action.is_none() {
            activation.action = None;
            if let Some((_, primary)) = clients.iter().find(|(_, client)| client.primary) {
                primary.send(Reply::Activated(activation));
            }
        }
        true
    }
    pub fn retire_owner(&mut self, owner: &str) {
        self.live.retain(|(sender, _), _| sender != owner);
        self.early.retain(|(sender, _, _, _)| sender != owner);
    }
    pub fn expired(&mut self, now: Instant) -> Vec<(String, u32)> {
        let expired = self
            .live
            .iter()
            .filter(|(_, live)| now >= live.until)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in &expired {
            self.live.remove(key);
        }
        expired
    }
}
