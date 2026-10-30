//! Durable window identity is separate from an input and from a live source epoch.
mod store;
#[cfg(test)]
mod tests;
use crate::{composer::EditorId, gateway::Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WindowId(String);
impl WindowId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        uuid::Uuid::parse_str(value)
            .map(|id| Self(id.to_string()))
            .map_err(|_| "Invalid desktop window identity".into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Default for WindowId {
    fn default() -> Self {
        Self::new()
    }
}
impl TryFrom<String> for WindowId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<WindowId> for String {
    fn from(value: WindowId) -> Self {
        value.0
    }
}

/// A descriptor can restore a route, never authenticate as a saved principal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceDescriptor {
    Saved {
        id: String,
        revision: String,
    },
    Local {
        home: PathBuf,
    },
    Remote {
        endpoint: String,
        credential_required: bool,
    },
}
impl SourceDescriptor {
    pub fn from_connection(connection: &Connection) -> Result<Self, String> {
        let result = match connection {
            Connection::Saved { id, revision, .. } => Self::Saved {
                id: id.clone(),
                revision: revision.clone(),
            },
            Connection::Local { home } => Self::Local { home: home.clone() },
            Connection::Url { endpoint, token } => {
                let (endpoint, embedded) = crate::gateway::endpoint_url(endpoint)?;
                Self::Remote {
                    endpoint: endpoint.to_string(),
                    credential_required: token.is_some() || embedded.is_some(),
                }
            }
        };
        result.validate()?;
        Ok(result)
    }
    /// Saved routes need the current registry resolver; protected URLs need credentials.
    pub fn connection_without_secret(&self) -> Option<Connection> {
        match self {
            Self::Local { home } => Some(Connection::Local { home: home.clone() }),
            Self::Remote {
                endpoint,
                credential_required: false,
            } => Some(Connection::Url {
                endpoint: endpoint.clone(),
                token: None,
            }),
            _ => None,
        }
    }
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Saved { id, revision } => {
                uuid::Uuid::parse_str(id).map_err(|_| "Invalid saved window source identity")?;
                uuid::Uuid::parse_str(revision)
                    .map_err(|_| "Invalid saved window source revision")?;
            }
            Self::Local { home } if !home.is_absolute() => {
                return Err("Saved local window sources need an absolute home".into());
            }
            Self::Remote { endpoint, .. } => {
                let (url, embedded) = crate::gateway::endpoint_url(endpoint)?;
                if embedded.is_some() || url.as_str() != endpoint {
                    return Err(
                        "Saved window endpoints must be canonical and credential-free".into(),
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapKind {
    PrimaryLaunch,
    PeerInherited,
    ProfilePinned,
    InteractiveSession,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowRoute {
    pub source: Option<SourceDescriptor>,
    pub profile: String,
    pub session: Option<String>,
    pub title: Option<String>,
    pub canonical_bot: bool,
}
impl Default for WindowRoute {
    fn default() -> Self {
        Self {
            source: None,
            profile: "default".into(),
            session: None,
            title: None,
            canonical_bot: false,
        }
    }
}
impl WindowRoute {
    fn validate(&self) -> Result<(), String> {
        if let Some(source) = &self.source {
            source.validate()?;
        }
        if self.profile.is_empty() || self.session.as_ref().is_some_and(String::is_empty) {
            return Err(
                "Saved windows need a profile and a nonempty stored session identity".into(),
            );
        }
        if self.canonical_bot && self.session.is_some() {
            return Err(
                "A Bot Chat window must resolve its named registry instead of pinning a stored id"
                    .into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Geometry {
    /// Native desktop coordinates, matching Rune WindowOptions::position.
    pub position: Option<(i32, i32)>,
    /// Logical content pixels; the window host applies display scale and limits.
    pub size: (u32, u32),
    pub maximized: bool,
}
impl Default for Geometry {
    fn default() -> Self {
        Self {
            position: None,
            size: (1440, 900),
            maximized: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: WindowId,
    pub editor: EditorId,
    pub kind: BootstrapKind,
    pub route: WindowRoute,
    /// Explicit profile-window defaults survive later navigation in that window.
    #[serde(default)]
    pub pinned_route: Option<WindowRoute>,
    #[serde(default)]
    pub geometry: Geometry,
    #[serde(default)]
    pub presentation: Option<super::Preferences>,
    #[serde(default)]
    pub closed: bool,
    pub revision: u64,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}
impl Record {
    fn new(id: WindowId, editor: EditorId, kind: BootstrapKind, route: WindowRoute) -> Self {
        Self {
            id,
            editor,
            kind,
            pinned_route: (kind == BootstrapKind::ProfilePinned).then(|| {
                let mut pinned = route.clone();
                pinned.session = None;
                pinned.title = None;
                pinned.canonical_bot = false;
                pinned
            }),
            route,
            geometry: Geometry::default(),
            presentation: None,
            closed: false,
            revision: 0,
            other: BTreeMap::new(),
        }
    }
    fn validate(&self) -> Result<(), String> {
        self.route.validate()?;
        match (&self.pinned_route, self.kind) {
            (Some(route), BootstrapKind::ProfilePinned) => {
                route.validate()?;
                if route.session.is_some() || route.canonical_bot {
                    return Err("A profile-window default cannot pin a stored conversation".into());
                }
            }
            (None, BootstrapKind::ProfilePinned) | (Some(_), _) => {
                return Err("The saved profile-window default is ambiguous".into());
            }
            _ => {}
        }
        if self.geometry.size.0 == 0 || self.geometry.size.1 == 0 {
            return Err("Saved window dimensions must be positive".into());
        }
        if let Some(presentation) = &self.presentation {
            presentation.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum ClaimError {
    Busy(WindowId),
    Store(String),
}
impl std::fmt::Display for ClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(_) => f.write_str("This desktop window is already open"),
            Self::Store(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for ClaimError {}
impl From<String> for ClaimError {
    fn from(value: String) -> Self {
        Self::Store(value)
    }
}
impl From<&str> for ClaimError {
    fn from(value: &str) -> Self {
        Self::Store(value.into())
    }
}

#[derive(Clone, Debug)]
pub struct WindowStore {
    root: PathBuf,
}
/// Non-cloneable: the file descriptor is the mount's exclusive ownership claim.
#[derive(Debug)]
pub struct WindowLease {
    store: WindowStore,
    record: Record,
    _claim: super::FileLock,
    pending: Option<Record>,
    retry_at: Option<Instant>,
}
impl WindowLease {
    pub fn id(&self) -> &WindowId {
        &self.record.id
    }
    pub fn editor_id(&self) -> &EditorId {
        &self.record.editor
    }
    pub fn record(&self) -> &Record {
        &self.record
    }
    pub fn pending(&self) -> Option<&Record> {
        self.pending.as_ref()
    }
    pub fn wake_after(&self) -> Option<Duration> {
        self.retry_at
            .map(|at| at.saturating_duration_since(Instant::now()))
    }
    pub fn poll_storage(&mut self) -> Result<bool, String> {
        if self.retry_at.is_some_and(|at| at <= Instant::now()) {
            self.write_pending().map(|_| true)
        } else {
            Ok(false)
        }
    }
}
