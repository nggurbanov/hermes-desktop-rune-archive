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
impl Lease {
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
            *state.error.lock().unwrap() = Some(error.into());
            state.alive.store(false, Ordering::Release);
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
