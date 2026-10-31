//! Scoped OAuth token record. Metadata contains only a credential reference.
use super::*;
use crate::connections::cloud::{Error, TokenStore, Tokens};
use std::path::PathBuf;

pub(super) struct ScopedTokenStore {
    registry_path: PathBuf,
    scope: Scope,
    slot_id: String,
    route_revision: String,
    reference: String,
    secrets: Box<dyn Secrets>,
}
impl ScopedTokenStore {
    pub(super) fn new(
        registry_path: PathBuf,
        scope: Scope,
        stamp: &domain::SlotStamp,
        reference: &CredentialRef,
        secrets: Box<dyn Secrets>,
    ) -> Self {
        Self {
            registry_path,
            scope,
            slot_id: stamp.id.clone(),
            route_revision: stamp.route_revision.clone(),
            reference: reference.id().into(),
            secrets,
        }
    }
    fn locked<T>(
        &mut self,
        operation: impl FnOnce(&mut dyn Secrets, &str) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let _guard =
            super::super::registry::transaction(&self.registry_path).map_err(|_| Error::Store)?;
        let mut metadata = JsonMetadata(self.registry_path.clone());
        let registry = metadata.load().map_err(|_| Error::Store)?;
        let store = registry.legacy.as_ref().ok_or(Error::Store)?;
        let valid_stamp = matches!(store.scope_version(&self.scope), domain::ScopeVersion::Present(stamp)
            if stamp.id == self.slot_id && stamp.route_revision == self.route_revision);
        let remote = super::remote_in_store(store, &self.scope).ok_or(Error::Store)?;
        if !valid_stamp
            || remote.auth_mode != domain::AuthMode::Oauth
            || remote.oauth_credential.as_ref().map(CredentialRef::id)
                != Some(self.reference.as_str())
        {
            return Err(Error::Store);
        }
        operation(self.secrets.as_mut(), &self.reference)
    }
}
impl TokenStore for ScopedTokenStore {
    fn load(&mut self) -> Result<Option<Tokens>, Error> {
        self.locked(|secrets, reference| {
            secrets
                .get_optional(reference)
                .map_err(|_| Error::Store)?
                .map(|value| serde_json::from_str(&value).map_err(|_| Error::Store))
                .transpose()
        })
    }
    fn save(&mut self, tokens: &Tokens) -> Result<(), Error> {
        if tokens.access_token.is_empty() {
            return Err(Error::InvalidResponse);
        }
        let encoded = serde_json::to_string(tokens).map_err(|_| Error::Store)?;
        self.locked(|secrets, reference| secrets.set(reference, &encoded).map_err(|_| Error::Store))
    }
    fn compare_and_swap(
        &mut self,
        expected: &Tokens,
        replacement: Option<&Tokens>,
    ) -> Result<bool, Error> {
        if replacement.is_some_and(|tokens| tokens.access_token.is_empty()) {
            return Err(Error::InvalidResponse);
        }
        self.locked(|secrets, reference| {
            let Some(value) = secrets.get_optional(reference).map_err(|_| Error::Store)? else {
                return Ok(false);
            };
            let current: Tokens = serde_json::from_str(&value).map_err(|_| Error::Store)?;
            if &current != expected {
                return Ok(false);
            }
            match replacement {
                Some(tokens) => secrets
                    .set(
                        reference,
                        &serde_json::to_string(tokens).map_err(|_| Error::Store)?,
                    )
                    .map_err(|_| Error::Store)?,
                None => secrets.delete(reference).map_err(|_| Error::Store)?,
            }
            Ok(true)
        })
    }
}
