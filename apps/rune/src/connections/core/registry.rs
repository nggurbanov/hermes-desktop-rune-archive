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
        self.active_owner.is_some() || self.active.is_some() || !self.leases.is_empty()
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
                if let Some(mut active) = self.active.take() {
                    active.tunnel.close();
                }
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
        if let Some(mut active) = self.active.take() {
            active.tunnel.close();
        }
        if let Some((id, revision)) = owner {
            self.outbox.push_back((id, revision, error.into()));
        }
    }
}
