//! Logical source identity is separate from an authenticated runtime incarnation.
use crate::{
    connections::{Lease, Resolver},
    gateway::Connection,
    pages::PageRequest,
};
use serde::{Deserialize, Serialize};
mod profile;
mod route;
pub use profile::ProfileIntent;
pub use route::{RouteDoor, RouteOwner};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SourceId {
    Legacy,
    Saved(String),
    Direct(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceVersion {
    pub id: SourceId,
    pub revision: String,
}

/// Constructed from a bound connection, never from an agent's row.connection_id.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceStamp {
    version: SourceVersion,
    principal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    route_namespace: Option<String>,
}
impl SourceStamp {
    pub fn from_connection(connection: &Connection) -> Result<Self, String> {
        let principal = crate::client::connection_key(connection)?;
        let owner = connection.route_owner();
        if let (Connection::Saved { id, revision, .. }, Some(owner)) = (connection, owner) {
            if owner.registry_identity() != Some((id.as_str(), revision.as_str())) {
                return Err("The requested route conflicts with its saved envelope".into());
            }
        }
        let version = if let Some(owner) = owner {
            owner.source_version()
        } else {
            match connection {
                Connection::Saved { id, revision, .. } => SourceVersion {
                    id: SourceId::Saved(id.clone()),
                    revision: revision.clone(),
                },
                _ => SourceVersion {
                    id: SourceId::Direct(principal.clone()),
                    revision: principal.clone(),
                },
            }
        };
        Ok(Self {
            version,
            principal,
            route_namespace: owner.map(|owner| owner.namespace().to_owned()),
        })
    }
    pub fn version(&self) -> &SourceVersion {
        &self.version
    }
    pub fn principal(&self) -> &str {
        &self.principal
    }
    pub fn key(&self) -> String {
        serde_json::json!([self.version, self.principal]).to_string()
    }
    /// Saved revision changes with credentials/destination, not ephemeral SSH ports.
    pub fn storage_key(&self) -> String {
        match &self.route_namespace {
            Some(namespace) => serde_json::json!([self.version, namespace]).to_string(),
            None => serde_json::json!(self.version).to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionOwner {
    pub source: SourceStamp,
    pub profile: String,
    #[serde(default, skip_serializing_if = "ProfileIntent::is_backend")]
    pub profile_intent: ProfileIntent,
}
impl SessionOwner {
    pub fn new(source: SourceStamp, profile: impl Into<String>) -> Self {
        Self {
            source,
            profile: profile.into(),
            profile_intent: ProfileIntent::Backend,
        }
    }
    pub fn with_intent(
        source: SourceStamp,
        profile: impl Into<String>,
        profile_intent: ProfileIntent,
    ) -> Self {
        Self {
            source,
            profile: profile.into(),
            profile_intent,
        }
    }
    pub fn key(&self) -> String {
        if self.profile_intent.is_backend() {
            serde_json::json!([self.source, self.profile]).to_string()
        } else {
            serde_json::json!([self.source, self.profile, self.profile_intent]).to_string()
        }
    }
    pub fn session(&self, stored_id: impl Into<String>) -> SessionRef {
        SessionRef {
            owner: self.clone(),
            stored_id: stored_id.into(),
        }
    }
    pub fn row_key(&self, lineage_root: &str) -> String {
        if self.profile_intent.is_backend() {
            serde_json::json!([self.source, self.profile, lineage_root]).to_string()
        } else {
            serde_json::json!([self.source, self.profile, lineage_root, self.profile_intent])
                .to_string()
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRef {
    pub owner: SessionOwner,
    pub stored_id: String,
}

/// A saved-source action retains its verified lease through its final receipt.
#[derive(Clone)]
pub struct Binding {
    stamp: SourceStamp,
    connection: Connection,
    lease: Option<Lease>,
    resolver: Option<Resolver>,
    keepers: Vec<Resolver>,
}
impl Binding {
    pub fn new(connection: Connection, lease: Option<Lease>) -> Result<Self, String> {
        let stamp = SourceStamp::from_connection(&connection)?;
        if (matches!(connection, Connection::Saved { .. }) || connection.route_owner().is_some())
            && lease.is_none()
        {
            return Err("A saved source needs its retained connection lease".into());
        }
        let binding = Self {
            stamp,
            connection,
            lease,
            resolver: None,
            keepers: vec![],
        };
        binding.validate()?;
        Ok(binding)
    }
    pub fn resolver(&self) -> Option<&Resolver> {
        self.resolver.as_ref()
    }
    /// The original actor is a lifetime owner, never a route substitution.
    pub fn retaining_resolver(mut self, resolver: Resolver) -> Self {
        if self.resolver.is_none() {
            self.resolver = Some(resolver);
        }
        self
    }
    /// A delegated primary can be owned by another actor than the route settings.
    pub(crate) fn retaining_origins(mut self, origins: Vec<Resolver>) -> Self {
        for origin in origins {
            if self
                .resolver
                .iter()
                .chain(self.keepers.iter())
                .any(|kept| kept.same_actor(&origin))
            {
                continue;
            }
            if self.resolver.is_none() {
                self.resolver = Some(origin);
            } else {
                self.keepers.push(origin);
            }
        }
        self
    }
    pub(crate) fn origins(&self) -> Vec<Resolver> {
        self.resolver
            .iter()
            .chain(self.keepers.iter())
            .cloned()
            .collect()
    }
    pub fn stamp(&self) -> &SourceStamp {
        &self.stamp
    }
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
    pub fn lease(&self) -> Option<&Lease> {
        self.lease.as_ref()
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Some(lease) = &self.lease {
            if !lease.is_valid() || SourceStamp::from_connection(lease.connection())? != self.stamp
            {
                return Err("The captured source lease changed or expired".into());
            }
        }
        Ok(())
    }
    pub fn request(&self, request: PageRequest) -> Result<PageRequest, String> {
        self.validate()?;
        let mut request = match &self.lease {
            Some(lease) => request.lease(lease.clone()),
            None => request.connection(self.connection.clone()),
        };
        request.connection = Some(self.connection.clone());
        request.source_resolver = self.resolver.clone();
        request.source_keepers = self.keepers.clone();
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn saved(id: &str, revision: &str, port: u16) -> Connection {
        Connection::Saved {
            id: id.into(),
            revision: revision.into(),
            inner: Box::new(Connection::Url {
                endpoint: format!("http://127.0.0.1:{port}"),
                token: Some("synthetic".into()),
            }),
        }
    }
    #[test]
    fn colliding_profiles_and_lineage_ids_keep_distinct_source_incarnations() {
        let a = SourceStamp::from_connection(&saved("A", "one", 18001)).unwrap();
        let b = SourceStamp::from_connection(&saved("B", "one", 18001)).unwrap();
        let replacement = SourceStamp::from_connection(&saved("A", "two", 18001)).unwrap();
        let owner = SessionOwner::new(a.clone(), "research");
        assert_ne!(
            owner.row_key("root"),
            SessionOwner::new(b, "research").row_key("root")
        );
        assert_ne!(
            owner.row_key("root"),
            SessionOwner::new(replacement, "research").row_key("root")
        );
        assert_ne!(
            owner.row_key("a\0b"),
            SessionOwner::new(a, "research\0a").row_key("b")
        );
    }
    #[test]
    fn forward_recreation_changes_runtime_fence_but_not_saved_config_namespace() {
        let a = SourceStamp::from_connection(&saved("A", "one", 18001)).unwrap();
        let b = SourceStamp::from_connection(&saved("A", "one", 18002)).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.storage_key(), b.storage_key());
        let rotated = SourceStamp::from_connection(&saved("A", "two", 18002)).unwrap();
        assert_ne!(a.storage_key(), rotated.storage_key());
        assert!(Binding::new(saved("A", "one", 18001), None).is_err());
    }
}

#[cfg(test)]
mod profile_intent_tests {
    use super::*;
    #[test]
    fn selected_owner_is_distinct_without_changing_backend_keys_or_json() {
        let stamp = SourceStamp::from_connection(&Connection::Local {
            home: "/tmp/hermes-owner-fixture".into(),
        })
        .unwrap();
        let backend = SessionOwner::new(stamp.clone(), "default");
        let selected =
            SessionOwner::with_intent(stamp.clone(), "default", ProfileIntent::NativeSelf);
        assert_ne!(backend, selected);
        assert_ne!(backend.key(), selected.key());
        assert_ne!(backend.row_key("same"), selected.row_key("same"));
        assert_eq!(
            backend.key(),
            serde_json::json!([stamp, "default"]).to_string()
        );
        assert_eq!(
            backend.row_key("same"),
            serde_json::json!([stamp, "default", "same"]).to_string()
        );
        let value = serde_json::to_value(&backend).unwrap();
        assert!(value.get("profile_intent").is_none());
        assert_eq!(
            serde_json::from_value::<SessionOwner>(value).unwrap(),
            backend
        );
        assert_eq!(
            serde_json::from_value::<SessionOwner>(serde_json::to_value(&selected).unwrap())
                .unwrap(),
            selected
        );
    }
}
