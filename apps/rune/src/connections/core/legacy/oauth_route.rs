//! Retained OAuth transport for an explicitly selected generic URL source.
use super::*;
use crate::{
    connections::legacy::{Origin, Plan},
    gateway::{Connection, prepared::WirePolicy, route::ProfilePolicy},
};

impl Core {
    pub(super) fn generic_oauth_transport(
        &mut self,
        remote: &Remote,
        plan: &Plan,
        profiles: ProfilePolicy,
    ) -> Result<(Connection, WirePolicy), String> {
        if remote.kind != domain::RemoteKind::Url || remote.auth_mode != domain::AuthMode::Oauth {
            return Err("This source is not a generic OAuth URL".into());
        }
        let scope = match plan.origin {
            Origin::Profile => Scope::Profile(plan.desktop_profile.clone()),
            Origin::Global => Scope::Global,
            _ => return Err("The OAuth source has no selected settings owner".into()),
        };
        let store = self
            .registry
            .legacy
            .as_ref()
            .ok_or("The OAuth settings are unavailable")?;
        let stamp = match store.scope_version(&scope) {
            domain::ScopeVersion::Present(stamp) => stamp,
            _ => return Err("The OAuth settings changed before connecting".into()),
        };
        let reference = remote
            .oauth_credential
            .as_ref()
            .ok_or("Sign in to this OAuth gateway before connecting")?;
        let registry_path = self
            .metadata
            .storage_path()
            .ok_or("OAuth needs persistent client state")?
            .to_path_buf();
        let headers = match &remote.credential {
            Some(reference) => AuthBundle::decode(&self.secrets.get(reference.id())?)?
                .headers
                .into_iter()
                .collect::<Vec<_>>(),
            None => vec![],
        };
        let scope_key = match &scope {
            Scope::Global => "global".to_owned(),
            Scope::Profile(profile) => format!("profiles/{profile}"),
        };
        let identity = format!(
            "generic-url-oauth:{scope_key}:{}:{}:{}",
            stamp.id,
            stamp.route_revision,
            reference.id()
        );
        let tokens = super::cloud_store::ScopedTokenStore::new(
            registry_path,
            scope,
            &stamp,
            reference,
            Box::new(SystemSecrets),
        );
        let runtime = super::cloud_runtime::CloudRuntimeAdapter::new(
            &remote.url,
            &headers,
            &identity,
            tokens,
        )?;
        Ok((
            Connection::Url {
                endpoint: remote.url.clone(),
                token: None,
            },
            WirePolicy::cloud(profiles, runtime)?,
        ))
    }
}
