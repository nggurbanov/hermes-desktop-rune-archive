use super::*;
use crate::{
    connections::legacy::{ResolutionVersion, ResolveRequest, ResolvedLegacy, SelectionContext},
    source::{RouteDoor, RouteOwner, SourceId, SourceVersion},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn require_live(alive: &AtomicBool) -> Result<(), String> {
    if alive.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("The requested route was superseded".into())
    }
}
impl Core {
    pub(in crate::connections) fn resolve_legacy(
        &mut self,
        mut request: ResolveRequest,
        alive: &Arc<AtomicBool>,
    ) -> Result<ResolvedLegacy, String> {
        require_live(alive)?;
        let (plan, native) = {
            let _transaction = self.metadata.transaction()?;
            self.refresh_registry()?;
            self.refresh_route_context(&mut request.context);
            let store = self.registry.legacy.clone().unwrap_or_default();
            let native = self.native_door(&request.door, &request.desktop_profile, &store)?;
            let plan = if native.is_none() {
                Some(store.select(&request.desktop_profile, &request.context)?)
            } else {
                None
            };
            (plan, native)
        };
        require_live(alive)?;
        if let Some((id, revision)) = native {
            let version =
                ResolutionVersion::native_saved(&request.desktop_profile, &id, &revision)?;
            check_expected(request.expected.as_ref(), &version)?;
            if request.expected_namespace.is_some() {
                return Err("The saved window belonged to a different delegated route".into());
            }
            let lease = self.resolve_lease(&id, Some(&revision))?;
            require_live(alive)?;
            let connection = lease.connection().clone();
            let mut gateway = crate::gateway::Gateway::connect(&connection)?;
            let backend_proof = gateway.backend_profiles()?;
            if backend_proof
                .profiles
                .iter()
                .filter(|row| row.name == request.desktop_profile)
                .count()
                != 1
            {
                return Err("The saved source does not serve the selected backend profile".into());
            }
            let root_proof = gateway.backend_root_proof()?;
            require_live(alive)?;
            let resolved = ResolvedLegacy {
                connection,
                lease,
                version,
                matched_saved: Some(SourceVersion {
                    id: SourceId::Saved(id),
                    revision,
                }),
                desktop_profile: request.desktop_profile,
                backend_proof,
                root_proof,
                context: request.context,
                door: request.door,
            };
            let _transaction = self.metadata.transaction()?;
            self.refresh_registry()?;
            self.validate_legacy_receipt(&resolved)?;
            require_live(alive)?;
            return Ok(resolved);
        }
        let mut plan = plan.expect("a delegated route has its selected plan");
        if request.primary.is_none() {
            if let crate::connections::legacy::Selected::RegistryPrimary(row) = &plan.selected {
                let lease = self.resolve_lease(&row.id, Some(&row.revision))?;
                require_live(alive)?;
                let connection = lease.connection().clone();
                let mut gateway = crate::gateway::Gateway::connect(&connection)?;
                let backend_proof = gateway.backend_profiles()?;
                let root_proof = gateway.backend_root_proof()?;
                require_live(alive)?;
                request.context.primary_profile = root_proof.profile.clone();
                request.context.primary_remote_active = true;
                request.primary = Some(crate::connections::legacy::CapturedPrimary {
                    connection,
                    lease: Some(lease),
                    backend_proof,
                    root_proof,
                });
                let _transaction = self.metadata.transaction()?;
                self.refresh_registry()?;
                self.refresh_route_context(&mut request.context);
                let store = self.registry.legacy.clone().unwrap_or_default();
                let refreshed = store.select(&request.desktop_profile, &request.context)?;
                if refreshed.version.fingerprint() != plan.version.fingerprint() {
                    return Err(
                        "The selected Legacy route changed while proving its primary".into(),
                    );
                }
                plan = refreshed;
            }
        }
        check_expected(request.expected.as_ref(), &plan.version)?;
        let material = self.materialize_legacy(&request, &plan, alive)?;
        let _transaction = self.metadata.transaction()?;
        self.refresh_registry()?;
        require_live(alive)?;
        let mut context = request.context.clone();
        self.refresh_route_context(&mut context);
        let store = self.registry.legacy.clone().unwrap_or_default();
        if !store.validates_resolution(&plan.version, &context)
            || self
                .native_door(&request.door, &request.desktop_profile, &store)?
                .is_some()
        {
            return Err("The selected Legacy route changed while connecting".into());
        }
        let owner = material
            .connection
            .route_owner()
            .ok_or("The resolved route lost its authority")?;
        check_namespace(
            request.expected_namespace.as_deref(),
            Some(owner.namespace()),
        )?;
        let lease = self.retain_route(
            material.connection.clone(),
            material.backend_proof.profiles.clone(),
            plan.version.clone(),
            context.clone(),
            material.parent,
            material.tunnel,
        )?;
        require_live(alive)?;
        Ok(ResolvedLegacy {
            connection: material.connection,
            lease,
            version: plan.version,
            matched_saved: material.matched_saved,
            desktop_profile: request.desktop_profile,
            backend_proof: material.backend_proof,
            root_proof: material.root_proof,
            context,
            door: request.door,
        })
    }
    pub(in crate::connections) fn commit_legacy(
        &mut self,
        resolved: ResolvedLegacy,
        expected_namespace: Option<&str>,
        alive: &AtomicBool,
    ) -> Result<ResolvedLegacy, String> {
        require_live(alive)?;
        let _transaction = self.metadata.transaction()?;
        self.refresh_registry()?;
        self.validate_legacy_receipt(&resolved)?;
        check_namespace(
            expected_namespace,
            resolved.connection.route_owner().map(RouteOwner::namespace),
        )?;
        require_live(alive)?;
        if let RouteDoor::Saved { id, revision } = &resolved.door {
            if !self
                .registry
                .rows
                .iter()
                .any(|r| r.id == *id && r.revision == *revision)
            {
                return Err("The requested saved door changed before acceptance".into());
            }
            let mut next = self.registry.clone();
            Self::remember_activation(&mut next, id, &resolved.desktop_profile);
            require_live(alive)?;
            self.metadata.save(&next)?;
            self.registry = next;
        }
        // Legacy provenance never adopts a matching Saved source's selection.
        Ok(resolved)
    }
    fn validate_legacy_receipt(&self, resolved: &ResolvedLegacy) -> Result<(), String> {
        if !resolved.lease.is_valid()
            || crate::client::connection_key(resolved.lease.connection())?
                != crate::client::connection_key(&resolved.connection)?
        {
            return Err("The captured route lease expired or changed".into());
        }
        let store = self.registry.legacy.clone().unwrap_or_default();
        let native = self.native_door(&resolved.door, &resolved.desktop_profile, &store)?;
        if let Some((id, revision)) = resolved.version.native_identity() {
            if native
                .as_ref()
                .is_none_or(|pair| pair.0 != id || pair.1 != revision)
            {
                return Err(
                    "The saved door acquired a different profile route before acceptance".into(),
                );
            }
        } else {
            if native.is_some() {
                return Err("The delegated profile route was removed before acceptance".into());
            }
            let mut context = resolved.context.clone();
            self.refresh_route_context(&mut context);
            if !store.validates_resolution(&resolved.version, &context) {
                return Err("The Legacy route changed before acceptance".into());
            }
        }
        Ok(())
    }
    pub(super) fn refresh_route_context(&self, context: &mut SelectionContext) {
        context.primary = self
            .registry
            .primary
            .as_ref()
            .and_then(|id| self.registry.rows.iter().find(|row| &row.id == id))
            .cloned();
    }
    fn native_door(
        &self,
        door: &RouteDoor,
        profile: &str,
        store: &domain::Store,
    ) -> Result<Option<(String, String)>, String> {
        domain::Scope::Profile(profile.into()).validate()?;
        let RouteDoor::Saved { id, revision } = door else {
            return Ok(None);
        };
        let row = self
            .registry
            .rows
            .iter()
            .find(|r| r.id == *id && r.revision == *revision)
            .ok_or("The requested saved source changed or was removed")?;
        let eligible_override = store
            .profiles
            .get(profile)
            .is_some_and(|slot| match &slot.value {
                Profile::Remote(r) => !r.url.trim().is_empty(),
                Profile::Ssh(s) => !s.host.trim().is_empty(),
                _ => false,
            });
        Ok(
            (!matches!(row.destination, Destination::Local { .. }) || !eligible_override)
                .then(|| (id.clone(), revision.clone())),
        )
    }
}
fn check_expected(
    expected: Option<&ResolutionVersion>,
    version: &ResolutionVersion,
) -> Result<(), String> {
    if expected.is_some_and(|expected| expected != version) {
        Err("The captured Legacy route changed; reopen it before connecting".into())
    } else {
        Ok(())
    }
}
pub(super) fn check_namespace(expected: Option<&str>, actual: Option<&str>) -> Result<(), String> {
    if expected.is_some_and(|expected| actual != Some(expected)) {
        Err("This window's saved namespace no longer matches its configured route".into())
    } else {
        Ok(())
    }
}
