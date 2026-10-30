use super::*;
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    path::Path,
};

#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    revision: u64,
    primary: Option<WindowId>,
    records: BTreeMap<WindowId, Record>,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            primary: None,
            records: BTreeMap::new(),
            other: BTreeMap::new(),
        }
    }
}
fn directory(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|_| "Desktop window state directory could not be created")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Desktop window privacy could not be established")?;
    }
    Ok(())
}
fn lock_file(path: &Path) -> Result<File, String> {
    directory(path.parent().ok_or("Invalid desktop window state path")?)?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| "Desktop window ownership file could not be opened".into())
}
impl WindowStore {
    pub fn system() -> Result<Self, String> {
        Ok(Self::at(super::super::state_dir()?))
    }
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }
    fn valid_root(&self) -> Result<(), String> {
        if !self.root.is_absolute() {
            return Err("Desktop window state needs an absolute private directory".into());
        }
        Ok(())
    }
    fn lock(&self) -> Result<super::super::FileLock, String> {
        self.valid_root()?;
        let file = lock_file(&self.root.join("windows.lock"))?;
        file.lock()
            .map_err(|_| "Desktop window state could not be locked")?;
        Ok(super::super::FileLock::from_locked(file))
    }
    fn read(&self) -> Result<Document, String> {
        self.valid_root()?;
        let bytes = match std::fs::read(self.root.join("windows.json")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Document::default()),
            Err(_) => {
                return Err("Desktop window state could not be read; its file was retained".into());
            }
        };
        let document: Document = serde_json::from_slice(&bytes)
            .map_err(|_| "Saved desktop windows are invalid; their file was retained")?;
        if document.version != 1 {
            return Err(
                "Saved desktop windows use an unsupported schema; their file was retained".into(),
            );
        }
        let mut editors = BTreeSet::new();
        for (id, record) in &document.records {
            if id != &record.id
                || record.revision > document.revision
                || !editors.insert(record.editor.as_str())
            {
                return Err(
                    "Saved desktop window owners are ambiguous; their file was retained".into(),
                );
            }
            record.validate()?;
        }
        if let Some(primary) = &document.primary {
            if !document
                .records
                .get(primary)
                .is_some_and(|r| r.kind == BootstrapKind::PrimaryLaunch)
            {
                return Err("Saved primary window is missing; its file was retained".into());
            }
        }
        Ok(document)
    }
    fn write(&self, document: &Document) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(document)
            .map_err(|_| "Desktop window state could not be encoded")?;
        super::super::private_write(&self.root.join("windows.json"), &bytes)
    }
    fn claim(&self, record: Record) -> Result<WindowLease, ClaimError> {
        let claim = lock_file(
            &self
                .root
                .join("window-claims")
                .join(format!("{}.lock", record.id.as_str())),
        )?;
        match claim.try_lock() {
            Ok(()) => Ok(WindowLease {
                store: self.clone(),
                record,
                _claim: super::super::FileLock::from_locked(claim),
                pending: None,
                retry_at: None,
            }),
            Err(std::fs::TryLockError::WouldBlock) => Err(ClaimError::Busy(record.id)),
            Err(std::fs::TryLockError::Error(_)) => Err(ClaimError::Store(
                "Desktop window ownership could not be claimed".into(),
            )),
        }
    }
    pub fn claim_primary(&self) -> Result<WindowLease, ClaimError> {
        let _transaction = self.lock()?;
        let mut document = self.read()?;
        let record = if let Some(id) = &document.primary {
            document
                .records
                .get(id)
                .cloned()
                .ok_or("Saved primary window is missing")?
        } else {
            let editor =
                crate::composer::Composer::primary_at(&self.root.join("primary-editor.json"))?
                    .editor_id()
                    .clone();
            Record::new(
                WindowId::new(),
                editor,
                BootstrapKind::PrimaryLaunch,
                WindowRoute::default(),
            )
        };
        let mut lease = self.claim(record)?;
        lease.record.closed = false;
        lease.record.revision = next_revision(&mut document)?;
        document.primary = Some(lease.record.id.clone());
        document
            .records
            .insert(lease.record.id.clone(), lease.record.clone());
        self.write(&document)?;
        Ok(lease)
    }
    pub fn mint_peer(
        &self,
        kind: BootstrapKind,
        route: WindowRoute,
    ) -> Result<WindowLease, ClaimError> {
        if kind == BootstrapKind::PrimaryLaunch {
            return Err("Use the exclusive primary claim for a launch window".into());
        }
        route.validate()?;
        if kind == BootstrapKind::InteractiveSession
            && route.session.is_none()
            && !route.canonical_bot
        {
            return Err("A session window needs its captured conversation".into());
        }
        let _transaction = self.lock()?;
        let mut document = self.read()?;
        let mut id = WindowId::new();
        while document.records.contains_key(&id) {
            id = WindowId::new();
        }
        let record = Record::new(id, EditorId::new(), kind, route);
        let mut lease = self.claim(record)?;
        lease.record.revision = next_revision(&mut document)?;
        document
            .records
            .insert(lease.record.id.clone(), lease.record.clone());
        self.write(&document)?;
        Ok(lease)
    }
    pub fn restore(&self, id: &WindowId) -> Result<WindowLease, ClaimError> {
        let _transaction = self.lock()?;
        let mut document = self.read()?;
        let record = document
            .records
            .get(id)
            .cloned()
            .ok_or("This saved desktop window no longer exists")?;
        let mut lease = self.claim(record)?;
        lease.record.closed = false;
        lease.record.revision = next_revision(&mut document)?;
        document.records.insert(id.clone(), lease.record.clone());
        self.write(&document)?;
        Ok(lease)
    }
    pub fn legacy_editor_id(&self) -> Result<EditorId, String> {
        let document = self.read()?;
        if let Some(primary) = document.primary {
            return document
                .records
                .get(&primary)
                .map(|record| record.editor.clone())
                .ok_or_else(|| "Saved primary window is missing".into());
        }
        Ok(
            crate::composer::Composer::primary_at(&self.root.join("primary-editor.json"))?
                .editor_id()
                .clone(),
        )
    }
    pub fn records(&self) -> Result<Vec<Record>, String> {
        Ok(self.read()?.records.into_values().collect())
    }
}
impl WindowLease {
    pub fn save(&mut self, next: Record) -> Result<(), String> {
        if next.id != self.record.id
            || next.editor != self.record.editor
            || next.kind != self.record.kind
            || next.pinned_route != self.record.pinned_route
            || next.revision != self.record.revision
        {
            return Err("The window update belongs to another owner or an older mount".into());
        }
        next.validate()?;
        if next == self.record {
            self.pending = None;
            self.retry_at = None;
            return Ok(());
        }
        self.pending = Some(next);
        self.write_pending()
    }
    pub(super) fn write_pending(&mut self) -> Result<(), String> {
        let Some(next) = self.pending.clone() else {
            return Ok(());
        };
        let result = self.commit(next);
        if result.is_ok() {
            self.pending = None;
            self.retry_at = None;
        } else {
            self.retry_at = Some(Instant::now() + Duration::from_secs(2));
        }
        result
    }
    fn commit(&mut self, next: Record) -> Result<(), String> {
        let _transaction = self.store.lock()?;
        let mut document = self.store.read()?;
        let current = document
            .records
            .get_mut(&self.record.id)
            .ok_or("This saved desktop window no longer exists")?;
        if current.revision != self.record.revision {
            return Err("Saved window state changed; the stale update was retained locally".into());
        }
        // Only this owner's presentation changes; preserve newly introduced fields.
        let mut merged = next;
        // Opaque extension fields are not edits from this client's presentation.
        merged.other = current.other.clone();
        merged.revision = next_revision(&mut document)?;
        document.records.insert(merged.id.clone(), merged.clone());
        self.store.write(&document)?;
        self.record = merged;
        Ok(())
    }
    /// A normal close is durable; Drop alone denotes a crash/released mount.
    pub fn close(mut self) -> Result<(), String> {
        let mut next = self.record.clone();
        next.closed = true;
        self.save(next)
    }
}
fn next_revision(document: &mut Document) -> Result<u64, String> {
    document.revision = document
        .revision
        .checked_add(1)
        .ok_or("Saved window revision is exhausted")?;
    Ok(document.revision)
}
