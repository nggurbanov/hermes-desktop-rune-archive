//! A claim spans network side effects; short transactions fence every durable edit.
use super::{Prepared, fs};
use crate::client::FileLock;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(crate) struct Journal {
    root: PathBuf,
    claim: Option<Arc<Claim>>,
}
struct Claim {
    id: String,
    _file: FileLock,
}
impl Journal {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root)
            .map_err(|_| "The prepared-send directory could not be created")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Prepared sends could not be made private")?;
        }
        Ok(Self { root, claim: None })
    }
    pub(super) fn path(&self, id: &str, suffix: &str) -> Result<PathBuf, String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid saved submission identity")?;
        Ok(self.root.join(format!("{id}.{suffix}")))
    }
    fn lock(&self, id: &str, suffix: &str) -> Result<FileLock, String> {
        let mut options = fs::OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(self.path(id, suffix)?)
            .map_err(|_| "Saved send ownership could not be opened")?;
        file.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => "Another Hermes window is handling this saved send. Wait for it to finish, then review or retry.".to_owned(),
            fs::TryLockError::Error(_) => "Saved send ownership could not be locked".to_owned(),
        })?;
        Ok(FileLock::from_locked(file))
    }
    /// This guard can move to a worker. Process exit releases the claim; no timer
    /// or PID heuristic can steal it from a still-running admission.
    pub fn claim(&self, id: &str) -> Result<Self, String> {
        if let Some(claim) = &self.claim {
            return if claim.id == id {
                Ok(self.clone())
            } else {
                Err("The saved send claim belongs to another input".into())
            };
        }
        Ok(Self {
            root: self.root.clone(),
            claim: Some(Arc::new(Claim {
                id: id.into(),
                _file: self.lock(id, "claim-lock")?,
            })),
        })
    }
    pub fn review(&self, id: &str, expected_revision: u64) -> Result<(Self, Prepared), String> {
        let claim = self.claim(id)?;
        let entry = claim.load(id)?;
        if entry.revision() != expected_revision {
            return Err("This saved send changed after it was shown. Review its latest state before confirming or discarding.".into());
        }
        Ok((claim, entry))
    }
    fn read(&self, id: &str) -> Result<Option<Prepared>, String> {
        if self
            .path(id, "settled")?
            .try_exists()
            .map_err(|_| "Saved send settlement could not be checked")?
        {
            return Err("This send has already been settled".into());
        }
        let bytes = match fs::read(self.path(id, "json")?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Saved send could not be read".into()),
        };
        let entry: Prepared = serde_json::from_slice(&bytes)
            .map_err(|_| "Saved send is invalid; its file was preserved")?;
        if entry.submission_id != id || entry.version != 1 {
            return Err(
                "Saved send identity or version is unsupported; its file was preserved".into(),
            );
        }
        Ok(Some(entry))
    }
    fn matches(&self, entry: &Prepared, current: Option<&Prepared>) -> Result<(), String> {
        match current {
            Some(saved) if saved.revision() == entry.revision()
                && saved.create_id == entry.create_id
                && saved.connection == entry.connection
                && saved.profile == entry.profile
                && saved.session == entry.session => Ok(()),
            None if entry.revision() == 0 => Ok(()),
            _ => Err("This saved send changed in another window. Review its latest state before retrying or discarding it.".into()),
        }
    }
    /// Check the queued snapshot before opening a socket or executing a mutation.
    pub fn check_current(&self, entry: &Prepared) -> Result<(), String> {
        let _claim = self.claim(&entry.submission_id)?;
        let _transaction = self.lock(&entry.submission_id, "state-lock")?;
        let current = self.read(&entry.submission_id)?;
        self.matches(entry, current.as_ref())?;
        if let Some(current) = current {
            let queued =
                serde_json::to_value(entry).map_err(|_| "Prepared send could not be encoded")?;
            let durable =
                serde_json::to_value(current).map_err(|_| "Prepared send could not be encoded")?;
            if queued != durable {
                return Err("The queued send differs from its saved payload. Review the original input before retrying.".into());
            }
        }
        Ok(())
    }
    pub fn save(&self, entry: &Prepared) -> Result<(), String> {
        let _claim = self.claim(&entry.submission_id)?;
        let _transaction = self.lock(&entry.submission_id, "state-lock")?;
        self.matches(entry, self.read(&entry.submission_id)?.as_ref())?;
        let next = entry
            .revision()
            .checked_add(1)
            .ok_or("Saved send revision overflow")?;
        let candidate = entry.clone();
        candidate.revision.set(next);
        let bytes =
            serde_json::to_vec(&candidate).map_err(|_| "Prepared send could not be encoded")?;
        crate::client::private_write(&self.path(&entry.submission_id, "json")?, &bytes)?;
        // Cloned snapshots keep their old CAS revision. Only the successful
        // writer advances, including on the queued value handed to its worker.
        entry.revision.set(next);
        Ok(())
    }
    pub fn load(&self, id: &str) -> Result<Prepared, String> {
        let _transaction = self.lock(id, "state-lock")?;
        self.read(id)?
            .ok_or_else(|| "Saved send could not be read".into())
    }
    pub fn retire(&self, entry: &Prepared) -> Result<(), String> {
        let _claim = self.claim(&entry.submission_id)?;
        let _transaction = self.lock(&entry.submission_id, "state-lock")?;
        let current = self.read(&entry.submission_id)?;
        if current.is_none() {
            return Err("This saved send is no longer available".into());
        }
        self.matches(entry, current.as_ref())?;
        // The durable tombstone wins even if deleting the spent payload fails.
        crate::client::private_write(&self.path(&entry.submission_id, "settled")?, b"settled\n")?;
        match fs::remove_file(self.path(&entry.submission_id, "json")?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => {
                Err("The turn was accepted, but its saved payload could not be removed".into())
            }
        }
    }
    pub fn entries(&self) -> Result<Vec<Prepared>, String> {
        let mut entries = vec![];
        for row in fs::read_dir(&self.root).map_err(|_| "Saved sends could not be listed")? {
            let path = row
                .map_err(|_| "Saved send directory could not be read")?
                .path();
            if path.extension() == Some(std::ffi::OsStr::new("json")) {
                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(entry) = self.load(id) {
                        entries.push(entry);
                    }
                }
            }
        }
        Ok(entries)
    }
}
#[cfg(test)]
mod tests;
