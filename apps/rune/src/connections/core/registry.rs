//! Saved-source leases observe local metadata without probing agents or credentials.
use super::*;
use std::{
    fs::OpenOptions,
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn transaction(path: &Path) -> Result<crate::client::FileLock, String> {
    let parent = path
        .parent()
        .ok_or("Invalid saved connection registry path")?;
    std::fs::create_dir_all(parent)
        .map_err(|_| "The connection state directory could not be created")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Saved connection privacy could not be established")?;
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
        .map_err(|_| "The saved connection transaction could not be opened")?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(crate::client::FileLock::from_locked(file)),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err("Another window is updating saved connections; try again".into());
            }
            Err(std::fs::TryLockError::Error(_)) => {
                return Err("Saved connections could not be locked".into());
            }
        }
    }
}
impl Core {
    pub(in crate::connections) fn has_sources(&self) -> bool {
        self.active_owner.is_some() || !self.leases.is_empty()
    }
    pub(in crate::connections) fn refresh_registry(&mut self) -> Result<(), String> {
        let registry = match self.metadata.load() {
            Ok(registry) => registry,
            Err(error) => {
                self.retire_registry_sources(&error);
                return Err(error);
            }
        };
        self.registry = registry;
        self.poll_leases();
        if let Some((id, revision)) = self.active_owner.clone() {
            if !self
                .registry
                .rows
                .iter()
                .any(|r| r.id == id && r.revision == revision)
            {
                self.active_owner = None;
                self.primary_lease = None;
                self.outbox.push_back((
                    id,
                    revision,
                    "The saved source changed or was removed".into(),
                ));
            }
        }
        Ok(())
    }
    fn retire_registry_sources(&mut self, error: &str) {
        let owner = self.active_owner.take();
        self.primary_lease = None;
        for entry in self.leases.values_mut() {
            entry.fail(error);
        }
        self.leases.clear();
        if let Some((id, revision)) = owner {
            self.outbox.push_back((id, revision, error.into()));
        }
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<Registry, String> {
    let raw: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "The saved connection registry is invalid")?;
    let mut registry: Registry = serde_json::from_value(raw.clone())
        .map_err(|_| "The saved connection registry is invalid")?;
    if registry.version != 1 {
        return Err("This saved connection registry requires a newer Hermes app".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for row in &registry.rows {
        uuid::Uuid::parse_str(&row.id).map_err(|_| "A saved connection has an invalid identity")?;
        uuid::Uuid::parse_str(&row.revision)
            .map_err(|_| "A saved connection has an invalid owner revision")?;
        if !seen.insert(&row.id) {
            return Err("The saved registry contains duplicate connection identities".into());
        }
    }
    if registry
        .last_profiles
        .values()
        .any(|profile| !super::super::startup::valid_profile(profile))
    {
        return Err("A saved connection has an invalid remembered profile".into());
    }
    if let Some(rows) = raw["rows"].as_array() {
        for row in &mut registry.rows {
            if let Some(destination) = rows
                .iter()
                .find(|raw| raw["id"].as_str() == Some(&row.id))
                .and_then(|raw| raw["destination"].as_object())
            {
                row.destination_other = destination
                    .iter()
                    .filter(|(key, _)| !destination_keys(&row.destination).contains(&key.as_str()))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
            }
        }
    }
    registry.normalize(raw.get("primary").is_none());
    Ok(registry)
}
pub(super) fn encode(registry: &Registry) -> Result<Vec<u8>, String> {
    let mut value =
        serde_json::to_value(registry).map_err(|_| "Connections could not be encoded")?;
    if let Some(rows) = value["rows"].as_array_mut() {
        for raw in rows {
            if let Some(row) = registry
                .rows
                .iter()
                .find(|row| raw["id"].as_str() == Some(&row.id))
            {
                if let Some(destination) = raw["destination"].as_object_mut() {
                    destination.extend(row.destination_other.clone());
                }
            }
        }
    }
    serde_json::to_vec_pretty(&value).map_err(|_| "Connections could not be encoded".into())
}
fn destination_keys(destination: &Destination) -> &'static [&'static str] {
    match destination {
        Destination::Local { .. } => &["kind", "home"],
        Destination::Remote { .. } => &["kind", "url"],
        Destination::Ssh(_) => &[
            "kind",
            "host",
            "user",
            "port",
            "key_file",
            "remote_port",
            "local_port",
            "path",
        ],
    }
}
