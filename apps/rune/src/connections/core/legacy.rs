//! Legacy writes share the Saved registry's OS transaction and credential ledger.
use super::super::legacy::{
    self as domain, AuthBundle, AuthEdit, CredentialRef, Edit, Global, Profile, Remote, Scope,
    SecretEdit, ValueEdit,
};
use super::*;
mod cloud_install;
mod cloud_runtime;
mod cloud_store;
mod installation;
mod materialize;
pub(crate) mod oauth_login;
mod oauth_route;
mod pool;
mod resolve;
mod ssh_adapter;
mod ssh_store;
#[cfg(test)]
pub(crate) mod testing;
pub(super) use pool::RouteEntry;
impl Core {
    pub(in crate::connections) fn legacy_snapshot(&mut self) -> Result<domain::Snapshot, String> {
        self.refresh_registry()?;
        Ok(self
            .registry
            .legacy
            .as_ref()
            .cloned()
            .unwrap_or_default()
            .snapshot())
    }
    pub(in crate::connections) fn save_legacy(
        &mut self,
        mut edit: Edit,
    ) -> Result<domain::SaveReceipt, String> {
        let _transaction = self.metadata.transaction()?;
        self.refresh_registry()?;
        let store = self.registry.legacy.clone().unwrap_or_default();
        // Validate the exact scope and stale form before keychain or probe work.
        store.validate()?;
        edit.scope.validate()?;
        if store.scope_version(&edit.scope) != edit.expected {
            return Err(
                "These Legacy settings changed in another window; reopen them before saving".into(),
            );
        }
        normalize_edit(&mut edit)?;
        let previous = remote_in_store(&store, &edit.scope).cloned();
        if let Some(remote) = remote_in_edit(&mut edit.value) {
            remote.oauth_credential = previous
                .as_ref()
                .filter(|old| {
                    old.url == remote.url
                        && old.auth_mode == domain::AuthMode::Oauth
                        && remote.auth_mode == domain::AuthMode::Oauth
                })
                .and_then(|old| old.oauth_credential.clone());
        }
        let mut validation = store.clone();
        validation.apply(&edit.scope, &edit.expected, edit.value.clone())?;
        let provisional = self.prepare_legacy_auth(&mut edit, previous.as_ref())?;
        if let Some(remote) = remote_in_edit(&mut edit.value) {
            if previous
                .as_ref()
                .is_some_and(|old| old.credential != remote.credential)
            {
                remote.oauth_credential = None;
            }
        }
        let mut next = self.registry.clone();
        let mut updated = store;
        let version = updated.apply(&edit.scope, &edit.expected, edit.value)?;
        next.version = 2;
        next.legacy = Some(updated);
        let reachable = next.credential_refs()?;
        for old in self.registry.credential_refs()? {
            if !reachable.contains(&old) && !next.cleanup.contains(&old) {
                next.cleanup.push(old);
            }
        }
        if let Some((reference, encoded)) = &provisional {
            // A recoverable unreachable-reference intent precedes all secret
            // creation. A crash/write failure cannot lose its cleanup identity.
            let mut stage = self.registry.clone();
            if !stage.cleanup.iter().any(|id| id == reference.id()) {
                stage.cleanup.push(reference.id().into());
            }
            self.metadata.save(&stage)?;
            self.registry = stage;
            if let Err(error) = self.secrets.set(reference.id(), encoded) {
                return Err(self.rollback_legacy_secret(reference, error));
            }
            next.cleanup.retain(|id| id != reference.id());
        }
        if let Err(error) = self.metadata.save(&next) {
            return Err(match &provisional {
                Some((reference, _)) => self.rollback_legacy_secret(reference, error),
                None => error,
            });
        }
        self.registry = next;
        let warning = self.cleanup();
        Ok(domain::SaveReceipt {
            scope: edit.scope,
            version,
            snapshot: self.registry.legacy.as_ref().unwrap().snapshot(),
            warning,
        })
    }
    fn rollback_legacy_secret(&mut self, reference: &CredentialRef, error: String) -> String {
        if self.secrets.delete(reference.id()).is_err() {
            return format!(
                "{error}. The provisional credential remains in the durable cleanup ledger"
            );
        }
        self.registry.cleanup.retain(|id| id != reference.id());
        if self.metadata.save(&self.registry).is_err() {
            return format!("{error}. Credential cleanup will be retried from the durable ledger");
        }
        error
    }
    fn prepare_legacy_auth(
        &mut self,
        edit: &mut Edit,
        previous: Option<&Remote>,
    ) -> Result<Option<(CredentialRef, String)>, String> {
        let Some(remote) = remote_in_edit(&mut edit.value) else {
            if !edit.auth.unchanged() {
                return Err("This Legacy scope does not contain a URL credential target".into());
            }
            return Ok(None);
        };
        let previous_ref = previous.and_then(|r| r.credential.as_ref());
        if previous.is_some_and(|p| p.url != remote.url) && previous_ref.is_some() {
            let kept_header = previous.unwrap().header_names.iter().any(|name| {
                edit.auth
                    .headers
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .is_none_or(|(_, v)| matches!(v, SecretEdit::Keep))
            });
            if matches!(edit.auth.token, SecretEdit::Keep) || kept_header {
                return Err("The Legacy destination changed. Re-enter or clear its token and access headers before saving".into());
            }
        }
        if edit.auth.unchanged() {
            remote.credential = previous_ref.cloned();
            remote.token_present = previous.is_some_and(Remote::has_token);
            remote.header_names = previous.map(|r| r.header_names.clone()).unwrap_or_default();
            return Ok(None);
        }
        let complete_replacement = !matches!(edit.auth.token, SecretEdit::Keep)
            && previous.is_none_or(|old| {
                old.header_names.iter().all(|name| {
                    edit.auth
                        .headers
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(name))
                        .is_some_and(|(_, change)| !matches!(change, SecretEdit::Keep))
                })
            });
        let mut bundle = match previous_ref {
            Some(reference) if !complete_replacement => {
                AuthBundle::decode(&self.secrets.get(reference.id())?)?
            }
            _ => AuthBundle::empty(),
        };
        if previous.is_some_and(|p| p.url != remote.url)
            && bundle.headers.keys().any(|name| {
                edit.auth
                    .headers
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .is_none_or(|(_, change)| matches!(change, SecretEdit::Keep))
            })
        {
            return Err("The Legacy destination changed. Re-enter or clear every stored access header before saving".into());
        }
        let before = bundle.encode()?;
        match &edit.auth.token {
            SecretEdit::Keep => {}
            SecretEdit::Clear => bundle.token = None,
            SecretEdit::Set(value) => {
                let value = value.trim();
                bundle.token = (!value.is_empty()).then(|| value.into());
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for (name, change) in &edit.auth.headers {
            let name = name.trim().to_ascii_lowercase();
            if !seen.insert(name.clone()) {
                return Err("A Legacy access header was authored more than once".into());
            }
            match change {
                SecretEdit::Keep => {}
                SecretEdit::Clear => {
                    bundle.headers.remove(&name);
                }
                SecretEdit::Set(value) => {
                    bundle.headers.insert(name, value.clone());
                }
            }
        }
        bundle.normalize()?;
        remote.header_names = bundle.headers.keys().cloned().collect();
        remote.token_present = bundle.token.is_some();
        if bundle.is_empty() {
            remote.credential = None;
            return Ok(None);
        }
        let encoded = bundle.encode()?;
        if encoded == before {
            remote.credential = previous_ref.cloned();
            return Ok(None);
        }
        let reference = CredentialRef::fresh();
        remote.credential = Some(reference.clone());
        Ok(Some((reference, encoded)))
    }
}
impl Registry {
    pub(in crate::connections) fn credential_refs(
        &self,
    ) -> Result<std::collections::BTreeSet<String>, String> {
        let mut refs = self
            .rows
            .iter()
            .filter_map(|r| r.credential.clone())
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(legacy) = &self.legacy {
            legacy.validate()?;
            refs.extend(legacy.credential_refs());
        }
        Ok(refs)
    }
}
fn remote_in_store<'a>(store: &'a domain::Store, scope: &Scope) -> Option<&'a Remote> {
    match scope {
        Scope::Global => store.global.as_ref().and_then(|s| s.value.remote.as_ref()),
        Scope::Profile(p) => store.profiles.get(p).and_then(|s| match &s.value {
            Profile::Remote(r) => Some(r),
            _ => None,
        }),
    }
}
fn remote_in_edit(value: &mut ValueEdit) -> Option<&mut Remote> {
    match value {
        ValueEdit::Global(Global { remote, .. }) => remote.as_mut(),
        ValueEdit::Profile(Profile::Remote(r)) => Some(r),
        _ => None,
    }
}
fn normalize_edit(edit: &mut Edit) -> Result<(), String> {
    if let Some(remote) = remote_in_edit(&mut edit.value) {
        if !remote.url.trim().is_empty() {
            let (url, embedded) = crate::gateway::endpoint_url(remote.url.trim())?;
            remote.url = url.into();
            if let Some(token) = embedded {
                if !matches!(edit.auth.token, SecretEdit::Keep) {
                    return Err("Enter the Legacy token in one place: its field or URL".into());
                }
                edit.auth.token = SecretEdit::Set(token);
            }
        }
        remote.remote_profile = domain::normalize_alias(remote.remote_profile.as_deref());
    }
    Ok(())
}
#[cfg(test)]
mod tests;
