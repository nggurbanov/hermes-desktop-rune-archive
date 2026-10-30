//! Client-owned preferences and private runtime files, never an agent home.
mod file_lock;
pub(crate) use file_lock::FileLock;
pub mod fleet;
pub mod profiles;
pub mod startup;
pub(crate) mod store;
pub mod windows;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    #[serde(flatten)]
    pub other: std::collections::BTreeMap<String, serde_json::Value>,
    pub speak_replies: bool,
    pub layout: crate::layout::Node,
    pub decks: Option<crate::decks::Decks>,
    pub review: serde_json::Value,
    pub show_statusbar: bool,
    pub show_profile_rail: bool,
    pub hidden_status_items: std::collections::BTreeSet<String>,
    pub theme: Option<bool>,
    pub simple_mode: bool,
    pub sidebar_width: f32,
    pub sidebar_open: bool,
    pub inspector_width: f32,
    pub inspector_open: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            other: Default::default(),
            speak_replies: false,
            layout: Default::default(),
            decks: None,
            review: serde_json::Value::Null,
            show_statusbar: true,
            show_profile_rail: true,
            hidden_status_items: default_hidden_status(),
            theme: None,
            simple_mode: false,
            sidebar_width: 237.0,
            sidebar_open: true,
            inspector_width: 300.0,
            inspector_open: false,
        }
    }
}
pub fn state_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")));
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from);
    base.map(|p| p.join("hermes-desktop-rune"))
        .filter(|p| p.is_absolute())
        .ok_or_else(|| "The client state directory is unavailable".into())
}
pub fn private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid client state path")?;
    std::fs::create_dir_all(parent).map_err(|_| "Client state directory could not be created")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Client state privacy could not be established")?;
    }
    let temp = parent.join(format!(
        ".write-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temp)
            .map_err(|_| "Client state file could not be opened")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Client state file could not be saved")?;
        std::fs::rename(&temp, path).map_err(|_| "Client state file could not be committed")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
impl Preferences {
    pub fn load() -> Result<Self, String> {
        Self::load_at(&state_dir()?.join("preferences.json"))
    }
    pub(crate) fn load_at(path: &Path) -> Result<Self, String> {
        let mut p: Self = store::load(path, Self::validate)?;
        p.normalize();
        Ok(p)
    }
    /// `base` is this writer's last successful snapshot, never another window's
    /// live state. The returned value includes the other writers' stored changes.
    pub fn save_patch(&self, base: &Self) -> Result<Self, String> {
        self.save_patch_at(base, &state_dir()?.join("preferences.json"))
    }
    pub(crate) fn save_patch_at(&self, base: &Self, path: &Path) -> Result<Self, String> {
        let mut merged = store::save_patch(path, base, self, Self::validate)?;
        merged.normalize();
        Ok(merged)
    }
    fn validate(&self) -> Result<(), String> {
        for name in ["version", "schema_version"] {
            if self.other.get(name).is_some_and(|v| v.as_u64() != Some(1)) {
                return Err(
                    "Saved client preferences use an unsupported schema; their file was preserved"
                        .into(),
                );
            }
        }
        Ok(())
    }
    fn normalize(&mut self) {
        self.sidebar_width = if self.sidebar_width.is_finite() {
            self.sidebar_width.clamp(190.0, 360.0)
        } else {
            237.0
        };
        self.inspector_width = if self.inspector_width.is_finite() {
            self.inspector_width.clamp(240.0, 520.0)
        } else {
            300.0
        };
    }
}

/// An opaque owner key includes the actual credential principal without storing it.
pub fn connection_key(connection: &crate::gateway::Connection) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let identity = match connection {
        crate::gateway::Connection::Saved {
            id,
            revision,
            inner,
        } => format!("saved:{id}:{revision}:{}", connection_key(inner)?),
        crate::gateway::Connection::Local { home } => format!(
            "local:{}",
            std::fs::canonicalize(home)
                .unwrap_or_else(|_| home.clone())
                .display()
        ),
        crate::gateway::Connection::Url { endpoint, token } => {
            let (url, embedded) = crate::gateway::endpoint_url(endpoint)?;
            format!(
                "url:{}\0{}",
                url,
                token
                    .as_ref()
                    .or(embedded.as_ref())
                    .map(String::as_str)
                    .unwrap_or("")
            )
        }
    };
    Ok(format!("{:x}", Sha256::digest(identity.as_bytes())))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connection_identity_normalizes_transport_aliases_and_fences_credentials() {
        let connection = |endpoint: &str, token: Option<&str>| crate::gateway::Connection::Url {
            endpoint: endpoint.into(),
            token: token.map(str::to_owned),
        };
        let first = connection_key(&connection(
            "https://fixture.invalid/hermes/api/ws?token=one",
            None,
        ))
        .unwrap();
        assert_eq!(
            first,
            connection_key(&connection("wss://fixture.invalid/hermes", Some("one"))).unwrap()
        );
        assert_ne!(
            first,
            connection_key(&connection("https://fixture.invalid/hermes", Some("two"))).unwrap()
        );
        assert_eq!(first.len(), 64);
        assert!(!first.contains("one"));
    }
    #[test]
    fn private_state_replacement_is_atomic_and_uses_private_permissions() {
        let directory =
            std::env::temp_dir().join(format!("hermes-client-{}", uuid::Uuid::new_v4()));
        let file = directory.join("preferences.json");
        private_write(&file, b"old").unwrap();
        private_write(&file, b"new").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}

pub fn default_hidden_status() -> std::collections::BTreeSet<String> {
    [
        "cron",
        "webhooks",
        "session-timer",
        "context",
        "cache",
        "speed",
        "resources",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
