//! Saved sources can be retained without changing the foreground connection.
use super::*;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
struct State {
    id: String,
    revision: String,
    connection: Connection,
    profiles: Vec<crate::gateway::Profile>,
    alive: AtomicBool,
    error: Mutex<Option<String>>,
}
#[derive(Clone)]
pub struct Lease(Arc<State>);
#[derive(Clone)]
pub struct LeaseGuard(Weak<State>);
impl LeaseGuard {
    pub fn is_valid(&self) -> bool {
        self.0
            .upgrade()
            .is_some_and(|state| state.alive.load(Ordering::Acquire))
    }
}
impl std::fmt::Debug for LeaseGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceLeaseGuard")
            .field("valid", &self.is_valid())
            .finish()
    }
}

impl Lease {
    pub fn guard(&self) -> LeaseGuard {
        LeaseGuard(Arc::downgrade(&self.0))
    }
    pub fn connection(&self) -> &Connection {
        &self.0.connection
    }
    pub fn profiles(&self) -> &[crate::gateway::Profile] {
        &self.0.profiles
    }
    pub fn source_id(&self) -> &str {
        &self.0.id
    }
    pub fn revision(&self) -> &str {
        &self.0.revision
    }
    pub fn is_valid(&self) -> bool {
        self.0.alive.load(Ordering::Acquire)
    }
    pub fn error(&self) -> Option<String> {
        self.0.error.lock().unwrap().clone()
    }
}
impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceLease")
            .field("source", &self.source_id())
            .field("revision", &self.revision())
            .field("valid", &self.is_valid())
            .finish()
    }
}
pub(super) struct Entry {
    state: Weak<State>,
    tunnel: Option<Box<dyn core::Tunnel>>,
}
impl Entry {
    pub fn new(
        id: String,
        revision: String,
        connection: Connection,
        profiles: Vec<crate::gateway::Profile>,
        tunnel: Option<Box<dyn core::Tunnel>>,
    ) -> (Self, Lease) {
        let state = Arc::new(State {
            id,
            revision,
            connection,
            profiles,
            alive: AtomicBool::new(true),
            error: Mutex::new(None),
        });
        (
            Self {
                state: Arc::downgrade(&state),
                tunnel,
            },
            Lease(state),
        )
    }
    pub fn acquire(&self) -> Option<Lease> {
        self.state
            .upgrade()
            .filter(|state| state.alive.load(Ordering::Acquire))
            .map(Lease)
    }
    pub fn referenced(&self) -> bool {
        self.state.strong_count() > 0
    }
    pub fn exited(&mut self) -> Option<String> {
        self.tunnel.as_mut()?.exited()
    }
    pub fn fail(&mut self, error: &str) {
        if let Some(state) = self.state.upgrade() {
            let mut reason = state.error.lock().unwrap();
            if state.alive.load(Ordering::Acquire) {
                *reason = Some(error.into());
                state.alive.store(false, Ordering::Release);
            }
        }
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.close();
        }
    }
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.fail("The saved-source lease was released");
    }
}
#[derive(Clone)]
pub struct Resolver {
    send: Sender<Work>,
}
impl Resolver {
    pub(super) fn new(send: Sender<Work>) -> Self {
        Self { send }
    }
    pub fn resolve(
        &self,
        id: impl Into<String>,
        expected_revision: Option<String>,
    ) -> Result<Receiver<Result<Lease, String>>, String> {
        let id = id.into();
        uuid::Uuid::parse_str(&id).map_err(|_| "The saved-source identity is invalid")?;
        let (send, receive) = mpsc::channel();
        self.send
            .send(Work::Resolve {
                id,
                expected_revision,
                reply: send,
            })
            .map_err(|_| "The saved connection service is unavailable")?;
        Ok(receive)
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    #[test]
    fn notification_guard_does_not_retain_source_and_cannot_revive_on_replacement() {
        let connection = Connection::Url {
            endpoint: "http://127.0.0.1:49191".into(),
            token: None,
        };
        let (mut entry, lease) = Entry::new(
            "source".into(),
            "revision".into(),
            connection.clone(),
            vec![],
            None,
        );
        let guard = lease.guard();
        assert!(guard.is_valid());
        entry.fail("retired");
        assert!(!guard.is_valid());
        let (_replacement, new_lease) =
            Entry::new("source".into(), "revision".into(), connection, vec![], None);
        assert!(new_lease.guard().is_valid());
        assert!(!guard.is_valid());
        let guard = new_lease.guard();
        drop(new_lease);
        assert!(!guard.is_valid());
    }
}
