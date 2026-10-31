//! Explicit generic URL OAuth attempts. The UI only polls messages; all I/O stays here.
use super::*;
use crate::connections::cloud::{Endpoint, Error, Login};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

// A cancellation is a nonblocking atomic operation. Publication wins only if it
// claims COMMITTED before cancellation; otherwise the worker retires its own slot.
const ACTIVE: u8 = 0;
const CANCELLED: u8 = 1;
const COMMITTED: u8 = 2;
const HELD: u8 = 3;
#[derive(Clone)]
pub(crate) struct LoginGeneration(Arc<AtomicU8>);
impl LoginGeneration {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicU8::new(ACTIVE)))
    }
    pub(crate) fn invalidate(&self) {
        let _ = self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                matches!(state, ACTIVE | HELD).then_some(CANCELLED)
            });
    }
    pub(crate) fn hold(&self) {
        let _ = self
            .0
            .compare_exchange(ACTIVE, HELD, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(crate) fn release(&self) {
        let _ = self
            .0
            .compare_exchange(HELD, ACTIVE, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(super) fn active(&self) -> bool {
        matches!(self.0.load(Ordering::Acquire), ACTIVE | HELD)
    }
    pub(super) fn wait_ready(
        &self,
        deadline: Instant,
    ) -> Result<(), super::cloud_install::InstallFailure> {
        loop {
            if Instant::now() >= deadline {
                return Err(super::cloud_install::InstallFailure::Timeout);
            }
            match self.0.load(Ordering::Acquire) {
                ACTIVE => return Ok(()),
                HELD => std::thread::sleep(Duration::from_millis(1)),
                _ => return Err(super::cloud_install::InstallFailure::Cancelled),
            }
        }
    }
    pub(super) fn claim_publication(
        &self,
        deadline: Instant,
    ) -> Result<(), super::cloud_install::InstallFailure> {
        if Instant::now() >= deadline {
            return Err(super::cloud_install::InstallFailure::Timeout);
        }
        match self
            .0
            .compare_exchange(ACTIVE, COMMITTED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(HELD) => Err(super::cloud_install::InstallFailure::Held),
            Err(_) => Err(super::cloud_install::InstallFailure::Cancelled),
        }
    }
}

pub(crate) struct LoginRequest {
    pub scope: Scope,
    pub expected: domain::ScopeVersion,
    pub provider: Option<String>,
    pub generation: LoginGeneration,
    pub deadline: Instant,
}

pub(crate) struct LoginReceipt {
    pub scope: Scope,
    pub version: domain::ScopeVersion,
    pub(in crate::connections) reference: CredentialRef,
}
pub(crate) enum LoginEvent {
    /// The caller chooses how to open this explicit URL. No browser is opened here.
    Authorize(String),
    Complete(LoginReceipt),
    Failed(LoginFailure),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginFailure {
    Cancelled,
    Timeout,
    EmbeddedRequired,
    Unavailable,
    Stale,
    Store,
    Recovery,
}
pub(crate) struct LoginAttempt {
    generation: LoginGeneration,
    receiver: Receiver<LoginEvent>,
    deadline: Instant,
}
#[cfg(test)]
impl LoginEvent {
    pub(crate) fn test_complete(scope: Scope, version: domain::ScopeVersion) -> Self {
        Self::Complete(LoginReceipt {
            scope,
            version,
            reference: CredentialRef::fresh(),
        })
    }
}
impl LoginAttempt {
    #[cfg(test)]
    pub(crate) fn test_channel() -> (Self, mpsc::Sender<LoginEvent>) {
        let (send, receiver) = mpsc::channel();
        (
            Self {
                generation: LoginGeneration::new(),
                receiver,
                deadline: Instant::now() + Duration::from_secs(3),
            },
            send,
        )
    }

    pub(crate) fn start(path: PathBuf, request: LoginRequest) -> Self {
        Self::with_core(Core::system(path), request)
    }
    fn with_core(core: Core, request: LoginRequest) -> Self {
        let generation = request.generation.clone();
        let deadline = request.deadline;
        let (send, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = run(core, request, &send);
            if let Err(error) = result {
                let _ = send.send(LoginEvent::Failed(error));
            }
        });
        Self {
            generation,
            receiver,
            deadline,
        }
    }
    pub(crate) fn poll(&self) -> Result<Option<LoginEvent>, LoginFailure> {
        if Instant::now() >= self.deadline && self.generation.active() {
            self.generation.invalidate();
            return Err(LoginFailure::Timeout);
        }
        if self.generation.0.load(Ordering::Acquire) == CANCELLED {
            return Err(LoginFailure::Cancelled);
        }
        match self.receiver.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(LoginFailure::Unavailable),
        }
    }
    pub(crate) fn poll_terminal(&self) -> Option<LoginEvent> {
        match self.receiver.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(LoginEvent::Failed(LoginFailure::Unavailable)),
        }
    }
    #[cfg(test)]
    pub(crate) fn test_is_held(&self) -> bool {
        self.generation.0.load(Ordering::Acquire) == HELD
    }
    pub(crate) fn hold(&self) {
        self.generation.hold();
    }
    pub(crate) fn release(&self) {
        self.generation.release();
    }
    pub(crate) fn cancel(&self) {
        self.generation.invalidate();
    }
}
impl Drop for LoginAttempt {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn active(request: &LoginRequest) -> Result<(), LoginFailure> {
    if !request.generation.active() {
        return Err(LoginFailure::Cancelled);
    }
    if Instant::now() >= request.deadline {
        return Err(LoginFailure::Timeout);
    }
    Ok(())
}
fn map_error(error: Error) -> LoginFailure {
    match error {
        Error::Cancelled => LoginFailure::Cancelled,
        Error::Timeout => LoginFailure::Timeout,
        Error::EmbeddedRequired => LoginFailure::EmbeddedRequired,
        Error::Store => LoginFailure::Store,
        _ => LoginFailure::Unavailable,
    }
}
fn run(
    mut core: Core,
    request: LoginRequest,
    send: &mpsc::Sender<LoginEvent>,
) -> Result<(), LoginFailure> {
    active(&request)?;
    core.refresh_registry().map_err(|_| LoginFailure::Store)?;
    let store = core.registry.legacy.as_ref().ok_or(LoginFailure::Stale)?;
    if store.scope_version(&request.scope) != request.expected {
        return Err(LoginFailure::Stale);
    }
    let remote = remote_in_store(store, &request.scope)
        .ok_or(LoginFailure::Stale)?
        .clone();
    if remote.kind != domain::RemoteKind::Url || remote.auth_mode != domain::AuthMode::Oauth {
        return Err(LoginFailure::Stale);
    }
    let headers = match &remote.credential {
        Some(reference) => AuthBundle::decode(
            &core
                .secrets
                .get(reference.id())
                .map_err(|_| LoginFailure::Store)?,
        )
        .map_err(|_| LoginFailure::Store)?
        .headers
        .into_iter()
        .collect::<Vec<_>>(),
        None => vec![],
    };
    let endpoint = Endpoint::new(&remote.url, &headers).map_err(map_error)?;
    let status = endpoint.status(request.deadline).map_err(map_error)?;
    active(&request)?;
    let providers = endpoint.providers(request.deadline).map_err(map_error)?;
    if let Some(provider) = &request.provider {
        if !providers
            .get("providers")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("id").and_then(serde_json::Value::as_str) == Some(provider)
                })
            })
        {
            return Err(LoginFailure::Unavailable);
        }
    }
    active(&request)?;
    let mut login =
        Login::begin(&endpoint, &status, request.provider.as_deref()).map_err(map_error)?;
    send.send(LoginEvent::Authorize(login.authorize_url.clone()))
        .map_err(|_| LoginFailure::Cancelled)?;
    let tokens = loop {
        active(&request)?;
        if let Some(result) = login.poll_until(request.deadline) {
            break result.map_err(map_error)?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    active(&request)?;
    let receipt = core
        .install_cloud_tokens_guarded(
            request.scope.clone(),
            request.expected,
            tokens,
            &request.generation,
            request.deadline,
        )
        .map_err(|error| match error {
            super::cloud_install::InstallFailure::Cancelled => LoginFailure::Cancelled,
            super::cloud_install::InstallFailure::Timeout => LoginFailure::Timeout,
            super::cloud_install::InstallFailure::Stale => LoginFailure::Stale,
            super::cloud_install::InstallFailure::Store => LoginFailure::Store,
            super::cloud_install::InstallFailure::Recovery => LoginFailure::Recovery,
            super::cloud_install::InstallFailure::Held => LoginFailure::Unavailable,
        })?;
    let reference = remote_in_store(
        core.registry.legacy.as_ref().ok_or(LoginFailure::Store)?,
        &request.scope,
    )
    .and_then(|remote| remote.oauth_credential.clone())
    .ok_or(LoginFailure::Store)?;
    let _ = send.send(LoginEvent::Complete(LoginReceipt {
        scope: receipt.scope,
        version: receipt.version,
        reference,
    }));
    Ok(())
}

#[cfg(test)]
#[path = "oauth_login/tests.rs"]
mod tests;
