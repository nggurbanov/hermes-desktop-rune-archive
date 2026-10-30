//! Logical source identity is separate from an authenticated runtime incarnation.
use crate::{
    connections::{Lease, Resolver},
    gateway::Connection,
    pages::PageRequest,
};
use serde::{Deserialize, Serialize};

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
}
impl SourceStamp {
    pub fn from_connection(connection: &Connection) -> Result<Self, String> {
        let principal = crate::client::connection_key(connection)?;
        let version = match connection {
            Connection::Saved { id, revision, .. } => SourceVersion {
                id: SourceId::Saved(id.clone()),
                revision: revision.clone(),
            },
            _ => SourceVersion {
                id: SourceId::Direct(principal.clone()),
                revision: principal.clone(),
            },
        };
        Ok(Self { version, principal })
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
        serde_json::json!(self.version).to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionOwner {
    pub source: SourceStamp,
    pub profile: String,
}
impl SessionOwner {
    pub fn new(source: SourceStamp, profile: impl Into<String>) -> Self {
        Self {
            source,
            profile: profile.into(),
        }
    }
    pub fn key(&self) -> String {
        serde_json::json!([self.source, self.profile]).to_string()
    }
    pub fn session(&self, stored_id: impl Into<String>) -> SessionRef {
        SessionRef {
            owner: self.clone(),
            stored_id: stored_id.into(),
        }
    }
    pub fn row_key(&self, lineage_root: &str) -> String {
        serde_json::json!([self.source, self.profile, lineage_root]).to_string()
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
}
impl Binding {
    pub fn new(connection: Connection, lease: Option<Lease>) -> Result<Self, String> {
        let stamp = SourceStamp::from_connection(&connection)?;
        if matches!(connection, Connection::Saved { .. }) && lease.is_none() {
            return Err("A saved source needs its retained connection lease".into());
        }
        let binding = Self {
            stamp,
            connection,
            lease,
            resolver: None,
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
        request.source_resolver = self.resolver.clone();
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
