use super::resolve::require_live;
use super::*;
use crate::{
    connections::legacy::{CapturedPrimary, Plan, ResolveRequest, Selected},
    gateway::{
        BackendProfiles, BackendRootProof,
        prepared::{BackendTarget, SelectedContext, WirePolicy},
        route::Backend,
    },
    source::{RouteDoor, RouteOwner, SourceId, SourceVersion},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicBool},
};
pub(super) struct Material {
    pub connection: Connection,
    pub backend_proof: BackendProfiles,
    pub root_proof: BackendRootProof,
    pub parent: Option<Lease>,
    pub matched_saved: Option<SourceVersion>,
    pub tunnel: Option<Box<dyn Tunnel>>,
}
impl Core {
    pub(super) fn materialize_legacy(
        &mut self,
        request: &ResolveRequest,
        plan: &Plan,
        alive: &Arc<AtomicBool>,
    ) -> Result<Material, String> {
        let profiles = plan.profile_policy()?;
        let descriptor = profiles.backend_route("", "");
        let mut captured = None;
        let mut tunnel: Option<Box<dyn Tunnel>> = None;
        let (actual, policy, parent, matched_saved) = match &plan.selected {
            Selected::Remote(remote) => {
                if remote.kind != domain::RemoteKind::Url {
                    return Err(
                        "The managed Hermes Cloud service is not available in this version".into(),
                    );
                }
                if remote.auth_mode == domain::AuthMode::Oauth {
                    let (connection, policy) =
                        self.generic_oauth_transport(remote, plan, profiles.clone())?;
                    (connection, policy, None, None)
                } else {
                    let reference = remote
                        .credential
                        .as_ref()
                        .ok_or("The selected Legacy URL has no saved token; edit its settings")?;
                    let bundle = AuthBundle::decode(&self.secrets.get(reference.id())?)?;
                    let token = bundle
                        .token
                        .filter(|token| !token.trim().is_empty())
                        .ok_or("The selected Legacy URL requires its saved gateway token")?;
                    (
                        Connection::Url {
                            endpoint: remote.url.clone(),
                            token: Some(token),
                        },
                        WirePolicy::token(profiles.clone(), bundle.headers)?,
                        None,
                        None,
                    )
                }
            }
            Selected::Environment { url, token } => (
                Connection::Url {
                    endpoint: url.clone(),
                    token: Some(token.clone()),
                },
                WirePolicy::token(profiles.clone(), BTreeMap::new())?,
                None,
                None,
            ),
            Selected::Ssh(ssh) => {
                let ready = self.open_installed_ssh(ssh, request, plan, alive)?;
                tunnel = Some(Box::new(ready.resource));
                (
                    Connection::Url {
                        endpoint: ready.endpoint,
                        token: Some(ready.token),
                    },
                    WirePolicy::token(profiles.clone(), BTreeMap::new())?,
                    None,
                    None,
                )
            }
            Selected::RegistryPrimary(row) => {
                let primary = request.primary.as_ref().ok_or("The Legacy registry-primary route requires its captured live primary connection")?;
                if primary
                    .lease
                    .as_ref()
                    .and_then(Lease::actual_saved_identity)
                    != Some((row.id.as_str(), row.revision.as_str()))
                {
                    return Err(
                        "The captured primary does not retain the selected registry source".into(),
                    );
                }
                captured = Some(primary);
                captured_transport(primary, profiles.clone())?
            }
            Selected::Local => {
                let use_primary =
                    descriptor.backend == Backend::Primary && request.context.primary_remote_active;
                if use_primary {
                    let primary = request.primary.as_ref().ok_or("This profile inherits the actual remote primary; its captured source is required")?;
                    captured = Some(primary);
                    captured_transport(primary, profiles.clone())?
                } else {
                    let home = request.context.local_home.clone().or_else(|| request.primary.as_ref().and_then(|p| match p.connection.transport() { Connection::Local { home } => Some(home.clone()), _ => None })).ok_or("No explicit existing native Hermes home was captured for this local route")?;
                    (
                        Connection::Local { home },
                        WirePolicy::native(profiles.clone()),
                        None,
                        None,
                    )
                }
            }
        };
        require_live(alive)?;
        let pending_owner = owner(&request.door, plan.version.fingerprint())?;
        let pending = Connection::Routed {
            actual: Box::new(actual.clone()),
            policy: Arc::new(policy.clone()),
            owner: Arc::new(pending_owner),
        };
        let mut gateway = crate::gateway::Gateway::connect(&pending)?;
        let backend_proof = gateway.backend_profiles()?;
        let root_proof = gateway.backend_root_proof()?;
        require_live(alive)?;
        if captured.is_some_and(|capture| {
            capture.root_proof.profile_id != root_proof.profile_id
                || capture.root_proof.instance_id != root_proof.instance_id
                || capture.root_proof.authority_epoch != root_proof.authority_epoch
        }) {
            return Err("The captured primary runtime authority changed while connecting".into());
        }
        let named = plan
            .backend_profile
            .clone()
            .or(descriptor.descriptor_profile);
        let target = match named {
            Some(name) => {
                if backend_proof
                    .profiles
                    .iter()
                    .filter(|row| row.name == name)
                    .count()
                    != 1
                {
                    return Err(
                        "The configured backend profile is missing or ambiguous on this source"
                            .into(),
                    );
                }
                BackendTarget::Named(name)
            }
            None => BackendTarget::Root(root_proof.clone()),
        };
        let selected = SelectedContext::new(&request.desktop_profile, target)?;
        let policy = policy.with_selected(selected)?;
        use sha2::{Digest, Sha256};
        let namespace = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    "hermes-legacy-resolution-v1",
                    plan.version.fingerprint(),
                    &root_proof.profile_id,
                    policy.fingerprint()
                ))
                .expect("route namespace material serializes")
            )
        );
        let connection = Connection::Routed {
            actual: Box::new(actual),
            policy: Arc::new(policy),
            owner: Arc::new(owner(&request.door, namespace)?),
        };
        Ok(Material {
            connection,
            backend_proof,
            root_proof,
            parent,
            matched_saved,
            tunnel,
        })
    }
}
fn owner(door: &RouteDoor, namespace: String) -> Result<RouteOwner, String> {
    match door {
        RouteDoor::Legacy => RouteOwner::legacy(namespace),
        RouteDoor::Saved { id, revision } => RouteOwner::saved(id, revision, namespace),
    }
}
fn captured_transport(
    primary: &CapturedPrimary,
    profiles: crate::gateway::route::ProfilePolicy,
) -> Result<(Connection, WirePolicy, Option<Lease>, Option<SourceVersion>), String> {
    primary.root_proof.validate()?;
    if let Some(lease) = &primary.lease {
        if !lease.is_valid()
            || crate::client::connection_key(lease.connection())?
                != crate::client::connection_key(&primary.connection)?
        {
            return Err(
                "The captured primary lease expired or does not own that connection".into(),
            );
        }
    } else if matches!(
        primary.connection,
        Connection::Saved { .. } | Connection::Routed { .. }
    ) {
        return Err("A captured saved primary needs its original retained lease".into());
    }
    let policy = primary
        .connection
        .wire_policy()
        .map(|p| p.with_profiles(profiles.clone()))
        .unwrap_or_else(|| WirePolicy::native(profiles));
    let matched = primary
        .lease
        .as_ref()
        .and_then(Lease::actual_saved_identity)
        .map(|(id, revision)| SourceVersion {
            id: SourceId::Saved(id.into()),
            revision: revision.into(),
        });
    Ok((
        primary.connection.transport().clone(),
        policy,
        primary.lease.clone(),
        matched,
    ))
}
