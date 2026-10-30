//! Passive fleet facts are source-owned and independent of foreground navigation.
use crate::{
    connections::{Kind, Resolver, Saved},
    gateway::Profile,
    source::{Binding, SourceId, SourceStamp, SourceVersion},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
mod cache;
#[path = "inventory/mod.rs"]
mod inventory;
mod worker;
pub(crate) use crate::gateway::roster::{ProfileMetadata, RosterProfile};
use cache::State;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Descriptor {
    pub version: SourceVersion,
    pub label: String,
    pub kind: Kind,
    pub registration_order: usize,
}
impl Descriptor {
    pub fn saved(saved: &Saved, registration_order: usize) -> Self {
        Self {
            version: SourceVersion {
                id: SourceId::Saved(saved.id.clone()),
                revision: saved.revision.clone(),
            },
            label: saved.label.clone(),
            kind: saved.kind(),
            registration_order,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Ready,
    ConnectOnDemand,
    Unavailable(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Evidence {
    Gateway,
    SshInventory,
    CurrentSeed,
}
#[derive(Clone, Debug)]
pub(crate) struct Source {
    pub descriptor: Descriptor,
    pub status: Status,
    pub evidence: Option<Evidence>,
    pub install_id: Option<String>,
    pub principal_stamp: Option<SourceStamp>,
    pub profiles: Vec<RosterProfile>,
    pub inventory_error: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct Agent {
    pub source: SourceVersion,
    pub profile: String,
    pub target_profile: String,
    pub handle: String,
    pub metadata: ProfileMetadata,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    pub revision: u64,
    pub sources: Vec<Source>,
    pub agents: Vec<Agent>,
    pub refreshing: bool,
    pub error: Option<String>,
}
impl Snapshot {
    /// A display alias only. Source bindings/actions keep their original owner.
    pub fn canonical_source(
        &self,
        version: &SourceVersion,
        profile: &str,
    ) -> Option<SourceVersion> {
        let actual = self
            .sources
            .iter()
            .find(|source| &source.descriptor.version == version)?;
        if !actual.profiles.iter().any(|row| row.name == profile) {
            return None;
        }
        if self
            .agents
            .iter()
            .any(|agent| &agent.source == version && agent.profile == profile)
        {
            return Some(version.clone());
        }
        let install = actual.install_id.as_ref().filter(|id| !id.is_empty())?;
        self.agents
            .iter()
            .find(|agent| {
                agent.profile == profile
                    && self.sources.iter().any(|source| {
                        source.descriptor.version == agent.source
                            && source.install_id.as_ref() == Some(install)
                    })
            })
            .map(|agent| agent.source.clone())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    Mount,
    Focus,
    Visible,
    RegistryChanged,
    GatewayOpened,
    Explicit,
}
impl Reason {
    fn forced(self) -> bool {
        matches!(
            self,
            Self::RegistryChanged | Self::GatewayOpened | Self::Explicit
        )
    }
}

struct Service {
    state: Mutex<State>,
    send: Mutex<Option<mpsc::Sender<worker::Job>>>,
    wakers: Mutex<BTreeMap<u64, rune::Waker>>,
    sequence: AtomicU64,
    subscribers: AtomicUsize,
    children: Arc<inventory::ssh::Children>,
}
impl Service {
    fn wake(&self) {
        let wakers = self
            .wakers
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for waker in wakers {
            waker();
        }
    }
    fn dispatch(self: &Arc<Self>, jobs: Vec<worker::Job>) {
        for job in jobs {
            let failure = self
                .send
                .lock()
                .unwrap()
                .as_ref()
                .map(|send| send.send(job.clone()))
                .unwrap_or_else(|| Err(mpsc::SendError(job.clone())));
            if failure.is_err() {
                let followup = self.state.lock().unwrap().complete(
                    &job,
                    worker::Outcome::Failed("The fleet worker is unavailable".into()),
                    Instant::now(),
                );
                if !followup.is_empty() {
                    self.dispatch(followup);
                }
            }
        }
        self.wake();
    }
    fn start(self: &Arc<Self>, runner: Arc<dyn worker::Runner>) {
        let mut sender = self.send.lock().unwrap();
        if sender.is_some() {
            return;
        }
        let (send, receive) = mpsc::channel::<worker::Job>();
        *sender = Some(send);
        let receive = Arc::new(Mutex::new(receive));
        for _ in 0..4 {
            let receive = receive.clone();
            let service = Arc::downgrade(self);
            let runner = runner.clone();
            std::thread::spawn(move || {
                loop {
                    let job = match receive.lock().unwrap().recv() {
                        Ok(job) => job,
                        Err(_) => break,
                    };
                    let Some(service) = service.upgrade() else {
                        break;
                    };
                    let outcome = runner.run(&job);
                    let followup =
                        service
                            .state
                            .lock()
                            .unwrap()
                            .complete(&job, outcome, Instant::now());
                    service.dispatch(followup);
                }
            });
        }
    }
}

/// An actual parked source owner; this never registers a native UI subscriber.
pub(crate) struct SourceRetention {
    service: Weak<Service>,
    token: u64,
    stamp: SourceStamp,
}
impl SourceRetention {
    pub fn stamp(&self) -> &SourceStamp {
        &self.stamp
    }
    pub fn validate(&self) -> Result<(), String> {
        let service = self
            .service
            .upgrade()
            .ok_or("The retained fleet service closed")?;
        service
            .state
            .lock()
            .unwrap()
            .retained_binding(self.token, &self.stamp)
            .map(|_| ())
    }
}
impl Drop for SourceRetention {
    fn drop(&mut self) {
        if let Some(service) = self.service.upgrade() {
            service.state.lock().unwrap().release_retained(self.token);
            service.wake();
        }
    }
}

pub(crate) struct Fleet {
    service: Arc<Service>,
    owner: u64,
    registered: bool,
    resolver: Option<Resolver>,
    snapshot: Snapshot,
}
impl Default for Fleet {
    fn default() -> Self {
        Self {
            service: Arc::new(Service {
                state: Mutex::new(State::default()),
                send: Mutex::new(None),
                wakers: Mutex::new(BTreeMap::new()),
                sequence: AtomicU64::new(1),
                subscribers: AtomicUsize::new(0),
                children: Arc::new(inventory::ssh::Children::default()),
            }),
            owner: 0,
            registered: false,
            resolver: None,
            snapshot: Snapshot::default(),
        }
    }
}
impl Clone for Fleet {
    fn clone(&self) -> Self {
        Self {
            owner: self.service.sequence.fetch_add(1, Ordering::Relaxed),
            registered: false,
            resolver: None,
            service: self.service.clone(),
            snapshot: self.snapshot.clone(),
        }
    }
}
impl Drop for Fleet {
    fn drop(&mut self) {
        self.detach();
    }
}
impl Fleet {
    /// Called by an accepted native close, and again safely from Drop.
    pub fn detach(&mut self) {
        self.service.wakers.lock().unwrap().remove(&self.owner);
        self.service
            .state
            .lock()
            .unwrap()
            .remove_current(self.owner);
        if std::mem::replace(&mut self.registered, false)
            && self.service.subscribers.fetch_sub(1, Ordering::AcqRel) == 1
        {
            self.service.state.lock().unwrap().shutdown();
            self.service.send.lock().unwrap().take();
            self.service.children.shutdown();
        }
        self.resolver = None;
    }
    fn register(&mut self, waker: rune::Waker) {
        if !self.registered {
            self.service.subscribers.fetch_add(1, Ordering::AcqRel);
            self.registered = true;
        }
        self.service
            .wakers
            .lock()
            .unwrap()
            .insert(self.owner, waker);
    }
    pub fn setup(&mut self, waker: rune::Waker, resolver: Resolver) {
        self.register(waker);
        self.resolver = Some(resolver.clone());
        self.service.state.lock().unwrap().resolver = Some(resolver);
        self.service.children.reopen();
        self.service.start(Arc::new(worker::System {
            children: self.service.children.clone(),
        }));
    }
    pub fn update_registry(
        &mut self,
        descriptors: Vec<Descriptor>,
        primary: Option<SourceId>,
    ) -> bool {
        let changed = self
            .service
            .state
            .lock()
            .unwrap()
            .update_registry(descriptors, primary);
        if changed {
            self.service.wake();
        }
        self.poll() || changed
    }
    pub fn registry_unavailable(&mut self, error: String) {
        self.service.state.lock().unwrap().unavailable(error);
        self.service.wake();
        self.poll();
    }
    pub fn seed_current(&mut self, binding: Binding, profiles: &[Profile]) -> Result<(), String> {
        self.seed_current_owned(binding, profiles, self.resolver.clone())
    }
    pub fn seed_current_from(
        &mut self,
        binding: Binding,
        profiles: &[Profile],
        resolver: Resolver,
    ) -> Result<(), String> {
        self.seed_current_owned(binding, profiles, Some(resolver))
    }
    fn seed_current_owned(
        &mut self,
        binding: Binding,
        profiles: &[Profile],
        resolver: Option<Resolver>,
    ) -> Result<(), String> {
        binding.validate()?;
        self.service
            .state
            .lock()
            .unwrap()
            .seed_current(self.owner, binding, profiles, resolver);
        self.service.wake();
        self.poll();
        Ok(())
    }
    pub fn retain_source(
        &mut self,
        binding: Binding,
        resolver: Option<Resolver>,
    ) -> Result<SourceRetention, String> {
        binding.validate()?;
        let stamp = binding.stamp().clone();
        let token = self.service.sequence.fetch_add(1, Ordering::Relaxed);
        self.service
            .state
            .lock()
            .unwrap()
            .retain_source(token, binding, resolver);
        Ok(SourceRetention {
            service: Arc::downgrade(&self.service),
            token,
            stamp,
        })
    }
    pub fn adopt_source(&mut self, retained: &SourceRetention) -> Result<(), String> {
        if !retained.service.ptr_eq(&Arc::downgrade(&self.service)) {
            return Err("The retained source belongs to another fleet service".into());
        }
        self.service.state.lock().unwrap().adopt_source(
            self.owner,
            retained.token,
            &retained.stamp,
        )?;
        self.service.wake();
        self.poll();
        Ok(())
    }
    pub fn binding(&self, version: &SourceVersion) -> Option<Binding> {
        self.service.state.lock().unwrap().binding(version)
    }
    pub fn resolver(&self, version: &SourceVersion) -> Option<Resolver> {
        self.service.state.lock().unwrap().resolver(version)
    }
    pub fn refresh(&mut self, reason: Reason) {
        let jobs = self
            .service
            .state
            .lock()
            .unwrap()
            .refresh(reason, Instant::now());
        self.service.dispatch(jobs);
        self.poll();
    }
    pub fn poll(&mut self) -> bool {
        let mut state = self.service.state.lock().unwrap();
        state.retire_invalid();
        if state.snapshot.revision == self.snapshot.revision {
            return false;
        }
        self.snapshot = state.snapshot.clone();
        true
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    /// A wake only drains in-flight work. TTL expiry never schedules a network read.
    pub fn wake(&self) -> Option<Duration> {
        self.snapshot
            .refreshing
            .then_some(Duration::from_millis(200))
    }
}
#[cfg(test)]
mod tests;
