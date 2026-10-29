//! Client-owned preferences and private runtime files, never an agent home.
pub mod profiles;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub speak_replies: bool,
    pub layout: crate::layout::Node,
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
            speak_replies: false,
            layout: Default::default(),
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
        let path = state_dir()?.join("preferences.json");
        match std::fs::read(path) {
            Ok(bytes) => {
                let mut p: Self = serde_json::from_slice(&bytes)
                    .map_err(|_| "Saved client preferences are invalid")?;
                p.sidebar_width = if p.sidebar_width.is_finite() {
                    p.sidebar_width.clamp(190.0, 360.0)
                } else {
                    237.0
                };
                p.inspector_width = if p.inspector_width.is_finite() {
                    p.inspector_width.clamp(240.0, 520.0)
                } else {
                    300.0
                };
                Ok(p)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err("Saved client preferences could not be read".into()),
        }
    }
    pub fn save(&self) -> Result<(), String> {
        private_write(
            &state_dir()?.join("preferences.json"),
            &serde_json::to_vec_pretty(self)
                .map_err(|_| "Client preferences could not be encoded")?,
        )
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
