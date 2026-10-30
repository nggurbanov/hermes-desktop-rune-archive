//! Device-wide logical defaults, independent of window presentation and authenticated leases.
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    fs::OpenOptions,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const REFRESH: Duration = Duration::from_secs(1);
/// `None` is the legacy profile route; the explicit registered `local` id is distinct.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Route {
    #[serde(rename = "connectionId")]
    pub connection_id: Option<String>,
    pub profile: String,
}
impl Route {
    pub fn new(connection_id: Option<String>, profile: String) -> Result<Self, String> {
        let route = Self {
            connection_id,
            profile,
        };
        route.validate()?;
        Ok(route)
    }
    fn validate(&self) -> Result<(), String> {
        valid_profile(&self.profile)?;
        if let Some(id) = &self.connection_id {
            valid_source(id)?;
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for Route {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Fields {
            // A missing connectionId is invalid; it must explicitly be null or an id.
            #[serde(rename = "connectionId", alias = "connection_id")]
            connection_id: Value,
            profile: String,
        }
        let fields = Fields::deserialize(deserializer)?;
        let id = match fields.connection_id {
            Value::Null => None,
            Value::String(id) => Some(id),
            _ => return Err(D::Error::custom("Invalid default profile source")),
        };
        Self::new(id, fields.profile).map_err(D::Error::custom)
    }
}
fn valid_profile(profile: &str) -> Result<(), String> {
    let bytes = profile.as_bytes();
    let first = bytes
        .first()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !first
        || bytes.len() > 64
        || !bytes
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
    {
        return Err("Invalid default profile name".into());
    }
    Ok(())
}
fn valid_source(id: &str) -> Result<(), String> {
    if id != "local" && uuid::Uuid::parse_str(id).is_err() {
        return Err("Invalid default profile source identity".into());
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Document {
    version: u32,
    revision: u64,
    #[serde(rename = "defaultRoute", alias = "default_route")]
    default_route: Value,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            default_route: Value::Null,
            other: BTreeMap::new(),
        }
    }
}
impl Document {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(
                "Saved startup preferences require a newer Hermes app; their file was retained"
                    .into(),
            );
        }
        self.route().map(|_| ())
    }
    fn route(&self) -> Result<Option<Route>, String> {
        if self.default_route.is_null() {
            Ok(None)
        } else {
            serde_json::from_value(self.default_route.clone())
                .map(Some)
                .map_err(|_| {
                    "The saved default profile route is invalid; its file was retained".into()
                })
        }
    }
    fn set_route(&mut self, route: Option<&Route>) {
        self.default_route = match route {
            None => Value::Null,
            Some(route) => {
                let mut value = self.default_route.as_object().cloned().unwrap_or_default();
                value.remove("connection_id");
                value.insert(
                    "connectionId".into(),
                    route
                        .connection_id
                        .clone()
                        .map_or(Value::Null, Value::String),
                );
                value.insert("profile".into(), Value::String(route.profile.clone()));
                Value::Object(value)
            }
        };
    }
}
enum Intent {
    Set(Option<Route>),
    Profile {
        connection_id: Option<String>,
        old: String,
        new: Option<String>,
    },
    Forget(String),
}
impl Intent {
    fn apply(&self, document: &mut Document) -> Result<bool, String> {
        match self {
            Self::Set(route) => {
                document.set_route(route.as_ref());
                Ok(true) // Explicit choices win even if this window displayed the same value.
            }
            Self::Profile {
                connection_id,
                old,
                new,
            } => {
                let Some(mut route) = document.route()? else {
                    return Ok(false);
                };
                if route.connection_id != *connection_id || route.profile != *old {
                    return Ok(false);
                }
                match new {
                    Some(new) => {
                        route.profile = new.clone();
                        document.set_route(Some(&route));
                    }
                    None => document.set_route(None),
                }
                Ok(true)
            }
            Self::Forget(id) => {
                if document
                    .route()?
                    .is_some_and(|route| route.connection_id.as_deref() == Some(id))
                {
                    document.set_route(None);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }
}
/// The mirror changes on observation only; consumers decide when to navigate.
/// Failed writes retain intent but never publish an uncommitted default.
pub struct Startup {
    path: PathBuf,
    mirror: Option<Document>,
    pending: VecDeque<Intent>,
    error: Option<String>,
    retry_at: Option<Instant>,
    next_read: Instant,
}
impl Startup {
    pub fn system() -> Result<Self, String> {
        Ok(Self::load_at(super::state_dir()?.join("startup.json")))
    }
    pub fn load_at(path: PathBuf) -> Self {
        let (mirror, error) = match read(&path) {
            Ok(document) => (Some(document), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            path,
            mirror,
            pending: VecDeque::new(),
            error,
            retry_at: None,
            next_read: Instant::now() + REFRESH,
        }
    }
    pub fn get(&self) -> Result<Option<Route>, String> {
        self.mirror
            .as_ref()
            .ok_or_else(|| {
                self.error
                    .clone()
                    .unwrap_or_else(|| "Startup preferences are unavailable".into())
            })?
            .route()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn set(&mut self, route: Route) -> Result<(), String> {
        route.validate()?;
        self.pending.clear();
        self.pending.push_back(Intent::Set(Some(route)));
        self.flush()
    }
    pub fn clear(&mut self) -> Result<(), String> {
        self.pending.clear();
        self.pending.push_back(Intent::Set(None));
        self.flush()
    }
    /// Apply only an acknowledged mutation; compare its owner with the latest disk route.
    pub fn after_profile(
        &mut self,
        connection_id: Option<String>,
        old: String,
        new: Option<String>,
        succeeded: bool,
    ) -> Result<(), String> {
        if !succeeded {
            return Ok(());
        }
        valid_profile(&old)?;
        if let Some(new) = &new {
            valid_profile(new)?;
        }
        if let Some(id) = &connection_id {
            valid_source(id)?;
        }
        self.pending.push_back(Intent::Profile {
            connection_id,
            old,
            new,
        });
        self.flush()
    }
    pub fn after_forget(&mut self, id: String, succeeded: bool) -> Result<(), String> {
        if !succeeded {
            return Ok(());
        }
        valid_source(&id)?;
        self.pending.push_back(Intent::Forget(id));
        self.flush()
    }
    /// Retry pending intent immediately; an untouched invalid store has nothing to flush.
    pub fn flush(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        match self.commit() {
            Ok(document) => {
                self.mirror = Some(document);
                self.pending.clear();
                self.error = None;
                self.retry_at = None;
                self.next_read = Instant::now() + REFRESH;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.clone());
                let retry = Instant::now() + REFRESH;
                self.retry_at = Some(retry);
                self.next_read = retry;
                Err(error)
            }
        }
    }
    fn commit(&self) -> Result<Document, String> {
        let _transaction = lock(&self.path)?;
        let mut document = read(&self.path)?;
        let mut changed = false;
        for intent in &self.pending {
            changed |= intent.apply(&mut document)?;
        }
        if changed {
            document.revision = document
                .revision
                .checked_add(1)
                .ok_or("Startup preference revision is exhausted")?;
            let bytes = serde_json::to_vec_pretty(&document)
                .map_err(|_| "Startup preferences could not be encoded")?;
            super::private_write(&self.path, &bytes)?;
        }
        Ok(document)
    }
    pub fn poll(&mut self) -> Result<bool, String> {
        let now = Instant::now();
        if !self.pending.is_empty() && self.retry_at.is_none_or(|at| at <= now) {
            self.flush()?;
            return Ok(true);
        }
        if now < self.next_read {
            return Ok(false);
        }
        self.next_read = now + REFRESH;
        match read(&self.path) {
            Ok(document) => {
                let changed = self.error.is_some()
                    || self.mirror.as_ref().is_none_or(|old| {
                        old.revision != document.revision
                            || old.default_route != document.default_route
                    });
                self.mirror = Some(document);
                if self.pending.is_empty() {
                    self.error = None;
                }
                Ok(changed)
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
    pub fn wake_after(&self) -> Option<Duration> {
        let at = self
            .retry_at
            .map_or(self.next_read, |retry| retry.min(self.next_read));
        Some(at.saturating_duration_since(Instant::now()))
    }
}
fn read(path: &Path) -> Result<Document, String> {
    if !path.is_absolute() {
        return Err("Startup preferences require an absolute client path".into());
    }
    super::store::load(path, Document::validate)
}
fn lock(path: &Path) -> Result<super::FileLock, String> {
    if !path.is_absolute() {
        return Err("Startup preferences require an absolute client path".into());
    }
    let parent = path.parent().ok_or("Invalid startup preferences path")?;
    std::fs::create_dir_all(parent)
        .map_err(|_| "Startup preferences directory could not be created")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Startup preference privacy could not be established")?;
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path.with_extension("lock"))
        .map_err(|_| "Startup preference lock could not be opened")?;
    file.lock()
        .map_err(|_| "Startup preferences could not be locked")?;
    Ok(super::FileLock::from_locked(file))
}
#[cfg(test)]
mod tests;
