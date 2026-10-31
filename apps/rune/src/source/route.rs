//! Requested authority and a resolved dependency namespace are not transport provenance.
use super::{SourceId, SourceVersion};
use crate::gateway::Connection;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteDoor {
    Legacy,
    Saved { id: String, revision: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RouteOwner {
    door: RouteDoor,
    namespace: String,
}
impl RouteOwner {
    pub(crate) fn legacy(namespace: impl Into<String>) -> Result<Self, String> {
        Self::new(RouteDoor::Legacy, namespace.into())
    }
    pub(crate) fn saved(
        id: impl Into<String>,
        revision: impl Into<String>,
        namespace: impl Into<String>,
    ) -> Result<Self, String> {
        let id = id.into();
        let revision = revision.into();
        uuid::Uuid::parse_str(&id).map_err(|_| "Invalid requested saved source")?;
        uuid::Uuid::parse_str(&revision).map_err(|_| "Invalid requested source revision")?;
        Self::new(RouteDoor::Saved { id, revision }, namespace.into())
    }
    fn new(door: RouteDoor, namespace: String) -> Result<Self, String> {
        if namespace.is_empty() || namespace.len() > 1024 || namespace.chars().any(char::is_control)
        {
            return Err("Invalid resolved route namespace".into());
        }
        Ok(Self { door, namespace })
    }
    pub(crate) fn door(&self) -> &RouteDoor {
        &self.door
    }
    pub(crate) fn namespace(&self) -> &str {
        &self.namespace
    }
    pub(crate) fn registry_identity(&self) -> Option<(&str, &str)> {
        match &self.door {
            RouteDoor::Saved { id, revision } => Some((id, revision)),
            RouteDoor::Legacy => None,
        }
    }
    pub(crate) fn source_version(&self) -> SourceVersion {
        match &self.door {
            RouteDoor::Legacy => SourceVersion {
                id: SourceId::Legacy,
                revision: self.namespace.clone(),
            },
            RouteDoor::Saved { id, revision } => SourceVersion {
                id: SourceId::Saved(id.clone()),
                revision: revision.clone(),
            },
        }
    }
    pub(crate) fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(self).expect("route identity serializes");
        format!("{:x}", Sha256::digest(bytes))
    }
}

impl Connection {
    pub(crate) fn route_owner(&self) -> Option<&RouteOwner> {
        match self {
            Self::Routed { owner, .. } => Some(owner.as_ref()),
            Self::Saved { inner, .. } => inner.route_owner(),
            _ => None,
        }
    }
}
