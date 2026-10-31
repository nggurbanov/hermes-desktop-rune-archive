//! Explicit, revision-fenced OAuth token installation for a Legacy URL.
use super::*;
use crate::connections::cloud::Tokens;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub(super) enum InstallFailure {
    Cancelled,
    Timeout,
    Stale,
    Store,
    Recovery,
    Held,
}

fn live(
    generation: &oauth_login::LoginGeneration,
    deadline: Instant,
) -> Result<(), InstallFailure> {
    if !generation.active() {
        Err(InstallFailure::Cancelled)
    } else if Instant::now() >= deadline {
        Err(InstallFailure::Timeout)
    } else {
        Ok(())
    }
}

impl Core {
    pub(in crate::connections) fn install_cloud_tokens(
        &mut self,
        scope: Scope,
        expected: domain::ScopeVersion,
        tokens: Tokens,
    ) -> Result<domain::SaveReceipt, String> {
        self.install_cloud_tokens_guarded(
            scope,
            expected,
            tokens,
            &oauth_login::LoginGeneration::new(),
            Instant::now() + std::time::Duration::from_secs(360),
        )
        .map_err(|_| "The OAuth credential could not be installed".into())
    }

    pub(super) fn install_cloud_tokens_guarded(
        &mut self,
        scope: Scope,
        expected: domain::ScopeVersion,
        tokens: Tokens,
        generation: &oauth_login::LoginGeneration,
        deadline: Instant,
    ) -> Result<domain::SaveReceipt, InstallFailure> {
        loop {
            generation.wait_ready(deadline)?;
            match self.install_cloud_tokens_once(
                scope.clone(),
                expected.clone(),
                &tokens,
                generation,
                deadline,
            ) {
                Err(InstallFailure::Held) => generation.wait_ready(deadline)?,
                result => return result,
            }
        }
    }

    fn install_cloud_tokens_once(
        &mut self,
        scope: Scope,
        expected: domain::ScopeVersion,
        tokens: &Tokens,
        generation: &oauth_login::LoginGeneration,
        deadline: Instant,
    ) -> Result<domain::SaveReceipt, InstallFailure> {
        live(generation, deadline)?;
        if tokens.access_token.is_empty() {
            return Err(InstallFailure::Store);
        }
        let encoded = serde_json::to_string(&tokens).map_err(|_| InstallFailure::Store)?;
        let _transaction = self
            .metadata
            .transaction()
            .map_err(|_| InstallFailure::Store)?;
        self.refresh_registry().map_err(|_| InstallFailure::Store)?;
        let mut store = self.registry.legacy.clone().unwrap_or_default();
        store.validate().map_err(|_| InstallFailure::Store)?;
        if store.scope_version(&scope) != expected {
            return Err(InstallFailure::Stale);
        }
        let mut value = match &scope {
            Scope::Global => {
                let global = &store.global.as_ref().ok_or(InstallFailure::Stale)?.value;
                if !matches!(global.mode, domain::Mode::Remote | domain::Mode::Cloud) {
                    return Err(InstallFailure::Stale);
                }
                ValueEdit::Global(global.clone())
            }
            Scope::Profile(profile) => ValueEdit::Profile(
                store
                    .profiles
                    .get(profile)
                    .ok_or(InstallFailure::Stale)?
                    .value
                    .clone(),
            ),
        };
        let remote = remote_in_edit(&mut value).ok_or(InstallFailure::Stale)?;
        if remote.kind != domain::RemoteKind::Url
            || remote.auth_mode != domain::AuthMode::Oauth
            || remote.url.trim().is_empty()
        {
            return Err(InstallFailure::Stale);
        }
        crate::connections::cloud::Endpoint::new(&remote.url, &[])
            .map_err(|_| InstallFailure::Stale)?;
        live(generation, deadline)?;
        let reference = CredentialRef::fresh();
        remote.oauth_credential = Some(reference.clone());
        let version = store
            .apply(&scope, &expected, value)
            .map_err(|_| InstallFailure::Stale)?;
        let mut next = self.registry.clone();
        next.version = 2;
        next.legacy = Some(store);
        let reachable = next.credential_refs().map_err(|_| InstallFailure::Store)?;
        for old in self
            .registry
            .credential_refs()
            .map_err(|_| InstallFailure::Store)?
        {
            if !reachable.contains(&old) && !next.cleanup.contains(&old) {
                next.cleanup.push(old);
            }
        }
        let mut stage = self.registry.clone();
        stage.cleanup.push(reference.id().into());
        self.metadata
            .save(&stage)
            .map_err(|_| InstallFailure::Store)?;
        self.registry = stage.clone();
        if self.secrets.set(reference.id(), &encoded).is_err() {
            let rollback =
                self.rollback_legacy_secret(&reference, "Credential store failed".into());
            return Err(if rollback == "Credential store failed" {
                InstallFailure::Store
            } else {
                InstallFailure::Recovery
            });
        }
        // Claim before publishing: cancellation never waits for storage, and a
        // winning commit cannot subsequently be reported as cancelled. Until
        // this claim the fresh reference is unreachable and its cleanup intent
        // is durable, so rollback never has to undo published metadata.
        if let Err(reason) = live(generation, deadline) {
            let rollback = self.rollback_legacy_secret(&reference, "Login cancelled".into());
            return Err(if rollback == "Login cancelled" {
                reason
            } else {
                InstallFailure::Recovery
            });
        }
        if let Err(reason) = generation.claim_publication(deadline) {
            let rollback = self.rollback_legacy_secret(&reference, "Login cancelled".into());
            return Err(if rollback == "Login cancelled" {
                reason
            } else {
                InstallFailure::Recovery
            });
        }
        next.cleanup.retain(|id| id != reference.id());
        if self.metadata.save(&next).is_err() {
            let rollback =
                self.rollback_legacy_secret(&reference, "Credential publication failed".into());
            return Err(if rollback == "Credential publication failed" {
                InstallFailure::Store
            } else {
                InstallFailure::Recovery
            });
        }
        self.registry = next;
        let warning = self.cleanup();
        Ok(domain::SaveReceipt {
            scope,
            version,
            snapshot: self.registry.legacy.as_ref().unwrap().snapshot(),
            warning,
        })
    }
}
