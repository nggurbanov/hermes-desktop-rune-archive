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
    passive_only: bool,
    actual_parent: Option<Lease>,
    alive: AtomicBool,
    error: Mutex<Option<String>>,
}
#[derive(Clone)]
pub struct Lease(Arc<State>);
#[derive(Clone)]
pub struct LeaseGuard(Weak<State>);
impl State {
    fn valid(&self) -> bool {
        self.alive.load(Ordering::Acquire)
            && self.actual_parent.as_ref().is_none_or(Lease::is_valid)
    }
}
impl LeaseGuard {
    pub fn is_valid(&self) -> bool {
        self.0.upgrade().is_some_and(|state| state.valid())
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
    pub(crate) fn passive_only(&self) -> bool {
        self.0.passive_only
    }
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
        self.0.valid()
    }
    pub(crate) fn route_owner(&self) -> Option<&crate::source::RouteOwner> {
        self.0.connection.route_owner()
    }
    pub(crate) fn registry_identity(&self) -> Option<(&str, &str)> {
        match self.route_owner() {
            Some(owner) => owner.registry_identity(),
            None => Some((&self.0.id, &self.0.revision)),
        }
    }
    /// Actual Saved provenance follows retained transport ancestry, never the
    /// requested logical door or a matching destination/backend installation.
    pub(in crate::connections) fn actual_saved_identity(&self) -> Option<(&str, &str)> {
        if self.route_owner().is_some() {
            self.0
                .actual_parent
                .as_ref()
                .and_then(Lease::actual_saved_identity)
        } else {
            match &self.0.connection {
                Connection::Saved { id, revision, .. } => Some((id, revision)),
                _ => None,
            }
        }
    }
    pub fn error(&self) -> Option<String> {
        self.0
            .error
            .lock()
            .unwrap()
            .clone()
            .or_else(|| self.0.actual_parent.as_ref().and_then(Lease::error))
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
        Self::create(id, revision, connection, profiles, tunnel, false)
    }
    fn create(
        id: String,
        revision: String,
        connection: Connection,
        profiles: Vec<crate::gateway::Profile>,
        tunnel: Option<Box<dyn core::Tunnel>>,
        passive_only: bool,
    ) -> (Self, Lease) {
        let state = Arc::new(State {
            id,
            revision,
            connection,
            profiles,
            passive_only,
            actual_parent: None,
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
    pub(in crate::connections) fn routed(
        connection: Connection,
        profiles: Vec<crate::gateway::Profile>,
        actual_parent: Option<Lease>,
        tunnel: Option<Box<dyn core::Tunnel>>,
    ) -> Result<(Self, Lease), String> {
        let owner = connection
            .route_owner()
            .ok_or("A resolved route needs its requested authority")?;
        if actual_parent
            .as_ref()
            .is_some_and(|parent| !parent.is_valid())
        {
            return Err("The captured primary source expired before route publication".into());
        }
        let (id, revision) = match owner.registry_identity() {
            Some((id, revision)) => (id.to_owned(), revision.to_owned()),
            None => ("legacy".into(), owner.namespace().to_owned()),
        };
        // The parent is an older immutable lease, never a sender or this new
        // allocation; construction can only form an acyclic ownership chain.
        let state = Arc::new(State {
            id,
            revision,
            connection,
            profiles,
            passive_only: false,
            actual_parent,
            alive: AtomicBool::new(true),
            error: Mutex::new(None),
        });
        Ok((
            Self {
                state: Arc::downgrade(&state),
                tunnel,
            },
            Lease(state),
        ))
    }
    pub fn passive(id: String, revision: String, connection: Connection) -> (Self, Lease) {
        Self::create(id, revision, connection, vec![], None, true)
    }
    pub fn acquire(&self) -> Option<Lease> {
        self.state
            .upgrade()
            .filter(|state| state.valid())
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
    pub(super) send: Sender<Work>,
    pub(super) identity: Arc<super::shutdown::Owner>,
}
impl Resolver {
    pub(super) fn new(send: Sender<Work>) -> Self {
        Self {
            identity: Arc::new(super::shutdown::Owner { send: send.clone() }),
            send,
        }
    }
    pub(crate) fn same_actor(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
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

/// Owns the same registry entry that keeps a routed lease alive in production.
#[cfg(test)]
pub(crate) struct TestRoutedLease {
    _entry: Entry,
    pub(crate) lease: Lease,
}
#[cfg(test)]
impl TestRoutedLease {
    pub(crate) fn new(connection: Connection) -> Self {
        let (entry, lease) = Entry::routed(connection, vec![], None, None).unwrap();
        Self {
            _entry: entry,
            lease,
        }
    }
}
