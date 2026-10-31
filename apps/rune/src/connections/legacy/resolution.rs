use super::{ResolutionVersion, SelectionContext};
use crate::{
    connections::{Lease, Resolver},
    gateway::{BackendProfiles, BackendRootProof, Connection},
    source::{RouteDoor, SourceVersion},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Receiver,
};
#[derive(Clone)]
pub(crate) struct CapturedPrimary {
    pub connection: Connection,
    pub lease: Option<Lease>,
    pub backend_proof: BackendProfiles,
    pub root_proof: BackendRootProof,
}
pub(crate) struct ResolveRequest {
    pub door: RouteDoor,
    pub desktop_profile: String,
    pub context: SelectionContext,
    pub primary: Option<CapturedPrimary>,
    pub expected: Option<ResolutionVersion>,
    pub expected_namespace: Option<String>,
    pub intent: u64,
}
pub(crate) struct ResolvedLegacy {
    pub connection: Connection,
    pub lease: Lease,
    pub version: ResolutionVersion,
    pub matched_saved: Option<SourceVersion>,
    pub desktop_profile: String,
    pub backend_proof: BackendProfiles,
    pub root_proof: BackendRootProof,
    pub(in crate::connections) context: SelectionContext,
    pub(in crate::connections) door: RouteDoor,
}
impl std::fmt::Debug for ResolvedLegacy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedLegacy")
            .field("desktop_profile", &self.desktop_profile)
            .field("version", &self.version)
            .field("matched_saved", &self.matched_saved)
            .finish_non_exhaustive()
    }
}
pub(crate) struct ResolveReceipt {
    pub intent: u64,
    pub desktop_profile: String,
    pub result: Result<ResolvedLegacy, String>,
}
/// Origin senders belong to the caller, never the actor-owned lease graph.
pub(crate) struct ResolveTicket {
    pub receiver: Receiver<ResolveReceipt>,
    pub(in crate::connections) alive: Arc<AtomicBool>,
    pub(in crate::connections) origin: Resolver,
    pub(in crate::connections) primary_origins: Vec<Resolver>,
}
impl ResolveTicket {
    pub(crate) fn cancel(&self) {
        self.alive.store(false, Ordering::Release);
    }
    pub(crate) fn origins(&self) -> Vec<Resolver> {
        let mut origins = vec![self.origin.clone()];
        for origin in &self.primary_origins {
            if !origins.iter().any(|current| current.same_actor(origin)) {
                origins.push(origin.clone());
            }
        }
        origins
    }
}
impl Drop for ResolveTicket {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Resolver {
    pub(crate) fn resolve_legacy(
        &self,
        request: ResolveRequest,
        primary_origins: Vec<Resolver>,
    ) -> Result<ResolveTicket, String> {
        let (reply, receiver) = std::sync::mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        self.send
            .send(crate::connections::Work::ResolveLegacy {
                request,
                alive: alive.clone(),
                reply,
            })
            .map_err(|_| "The connection resolver is unavailable")?;
        Ok(ResolveTicket {
            receiver,
            alive,
            origin: self.clone(),
            primary_origins,
        })
    }
    pub(crate) fn commit_legacy(
        &self,
        resolved: ResolvedLegacy,
        expected_namespace: Option<String>,
        intent: u64,
        primary_origins: Vec<Resolver>,
    ) -> Result<ResolveTicket, String> {
        let (reply, receiver) = std::sync::mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        self.send
            .send(crate::connections::Work::CommitLegacy {
                resolved,
                expected_namespace,
                intent,
                alive: alive.clone(),
                reply,
            })
            .map_err(|_| "The connection resolver is unavailable")?;
        Ok(ResolveTicket {
            receiver,
            alive,
            origin: self.clone(),
            primary_origins,
        })
    }
}
