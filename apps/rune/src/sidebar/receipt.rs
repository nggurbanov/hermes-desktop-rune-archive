//! A source namespace owns its pending edit until the exact receipt is reconciled.
use super::SavedReceipt;
use crate::{
    client::FileLock,
    source::{SourceId, SourceStamp},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct Store {
    directory: PathBuf,
    stamp: SourceStamp,
    claim: Option<Arc<FileLock>>,
}
impl Store {
    pub fn new(directory: PathBuf, stamp: SourceStamp) -> Self {
        Self {
            directory,
            stamp,
            claim: None,
        }
    }
    fn path(&self) -> PathBuf {
        let namespace = format!("{:x}", Sha256::digest(self.stamp.storage_key().as_bytes()));
        self.directory
            .join("sidebar-edits")
            .join(format!("{namespace}.json"))
    }
    fn claim(&mut self) -> Result<(), String> {
        if self.claim.is_none() {
            self.claim = Some(Arc::new(lock(&self.path().with_extension("claim"))?));
        }
        Ok(())
    }
    fn read(&self) -> Result<Option<SavedReceipt>, String> {
        let receipt = read(&self.path())?;
        if let Some(receipt) = &receipt {
            self.validate(receipt)?;
        }
        Ok(receipt)
    }
    fn validate(&self, receipt: &SavedReceipt) -> Result<(), String> {
        if receipt.version > 1
            || receipt
                .other
                .get("schema_version")
                .is_some_and(|value| value.as_u64() != Some(1))
        {
            return Err("The pending session edit needs a newer app; its file was retained".into());
        }
        let identity = receipt
            .mutation
            .identity
            .as_ref()
            .and_then(|value| value.as_object());
        if identity.is_none_or(|identity| {
            identity
                .get("request_id")
                .and_then(|value| value.as_str())
                .is_none_or(|id| id.trim().is_empty())
                || identity
                    .get("expected_revision")
                    .and_then(|value| value.as_u64())
                    .is_none()
                || identity
                    .get("expected_generation")
                    .and_then(|value| value.as_u64())
                    .is_none()
        }) || receipt.mutation.session.id.trim().is_empty()
            || !receipt.mutation.delete && !receipt.mutation.payload.is_object()
        {
            return Err(
                "The pending edit has an invalid identity or payload; its file was retained".into(),
            );
        }
        match &receipt.mutation.source {
            Some(source)
                if source.storage_key() == self.stamp.storage_key()
                    && crate::gateway::sidebar::source_tag_matches(
                        receipt.mutation.session.connection_id.as_deref(),
                        source,
                    ) =>
            {
                Ok(())
            }
            None if receipt.owner == self.stamp.principal() => Ok(()),
            _ => Err(
                "The pending edit belongs to a different source namespace; its file was retained"
                    .into(),
            ),
        }
    }
    pub fn load(&mut self) -> Result<Option<SavedReceipt>, String> {
        if let Some(receipt) = self.read()? {
            return Ok(Some(receipt));
        }
        let legacy = self.directory.join("sidebar-edit.json");
        let Some(receipt) = read(&legacy)? else {
            return Ok(None);
        };
        // A legacy opaque owner has no saved-source version: never infer it from a row tag.
        if receipt.owner != self.stamp.principal() {
            return Ok(None);
        }
        if receipt.version > 1 {
            return Err("The legacy pending edit needs a newer app; its file was retained".into());
        }
        let _migration = lock(&self.directory.join("sidebar-edit-migration.claim"))?;
        let Some(mut receipt) = read(&legacy)? else {
            return Ok(None);
        };
        if receipt.owner != self.stamp.principal() || receipt.version > 1 {
            return Err("The legacy pending edit changed during migration; it was retained".into());
        }
        self.claim()?;
        if let Some(existing) = self.read()? {
            self.claim = None;
            return Ok(Some(existing));
        }
        receipt.version = 1;
        receipt
            .other
            .insert("legacy_owner".into(), receipt.owner.clone().into());
        receipt.mutation.source = Some(self.stamp.clone());
        self.validate(&receipt)?;
        write(&self.path(), &Some(&receipt))?;
        // Both copies have the same request identity if this final write fails; the scoped one wins.
        crate::client::private_write(&legacy, b"null\n")?;
        self.claim = None;
        Ok(Some(receipt))
    }
    pub fn begin(&mut self) -> Result<(), String> {
        // Re-read failed/future legacy state too: a failed load never grants write permission.
        let pending = self.load()?;
        self.claim()?;
        if pending.is_some() || self.read()?.is_some() {
            self.claim = None;
            return Err("A session edit is already pending on this source. Refresh and reconcile its original intent.".into());
        }
        Ok(())
    }
    /// False means another owner already reconciled the recovered receipt.
    pub fn save(&mut self, receipt: &SavedReceipt) -> Result<bool, String> {
        let prepared = self.claim.is_some();
        self.claim()?;
        self.validate(receipt)?;
        if let Some(existing) = self.read()? {
            if !same_intent(&existing, receipt) {
                return Err(
                    "Another pending session edit owns this source; its receipt was retained"
                        .into(),
                );
            }
        } else if !prepared {
            self.claim = None;
            return Ok(false);
        }
        write(&self.path(), &Some(receipt))?;
        Ok(true)
    }
    pub fn retire(&mut self, expected: &SavedReceipt) -> Result<(), String> {
        self.claim()?;
        let current = self
            .read()?
            .ok_or("The pending edit receipt disappeared; refresh before reconciling")?;
        if !same_intent(&current, expected) {
            return Err("Another edit replaced the pending receipt; it was retained".into());
        }
        if let Some(owner) = expected
            .other
            .get("legacy_owner")
            .and_then(|value| value.as_str())
        {
            let _migration = lock(&self.directory.join("sidebar-edit-migration.claim"))?;
            let legacy = self.directory.join("sidebar-edit.json");
            if read(&legacy)?
                .as_ref()
                .is_some_and(|receipt| receipt.owner == owner && same_payload(receipt, expected))
            {
                crate::client::private_write(&legacy, b"null\n")?;
            }
        }
        crate::client::private_write(&self.path(), b"null\n")?;
        self.claim = None;
        Ok(())
    }
    pub fn release_unprepared(&mut self) {
        if self.read().ok().flatten().is_none() {
            self.claim = None;
        }
    }
    pub fn worker_claim(&self) -> Option<Arc<FileLock>> {
        self.claim.clone()
    }
}
pub(super) fn can_reconcile(old: &SourceStamp, current: &SourceStamp) -> bool {
    old == current
        || matches!(
            (&old.version().id, &current.version().id),
            (SourceId::Saved(_), SourceId::Saved(_))
        ) && old.storage_key() == current.storage_key()
}
fn same_intent(a: &SavedReceipt, b: &SavedReceipt) -> bool {
    a.mutation
        .source
        .as_ref()
        .zip(b.mutation.source.as_ref())
        .is_some_and(|(a, b)| a.storage_key() == b.storage_key())
        && same_payload(a, b)
}
fn same_payload(a: &SavedReceipt, b: &SavedReceipt) -> bool {
    a.mutation.session.profile() == b.mutation.session.profile()
        && a.mutation.session.root_id() == b.mutation.session.root_id()
        && a.mutation.identity == b.mutation.identity
        && a.mutation.identity.is_some()
        && a.mutation.payload == b.mutation.payload
        && a.mutation.delete == b.mutation.delete
}
fn read(path: &Path) -> Result<Option<SavedReceipt>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Saved session edit is invalid; its file was retained".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Saved session edit could not be read; its file was retained".into()),
    }
}
fn write(path: &Path, receipt: &Option<&SavedReceipt>) -> Result<(), String> {
    let bytes = serde_json::to_vec(receipt).map_err(|_| "The pending edit could not be encoded")?;
    crate::client::private_write(path, &bytes)
}
fn lock(path: &Path) -> Result<FileLock, String> {
    let parent = path.parent().ok_or("Invalid session edit lock path")?;
    std::fs::create_dir_all(parent).map_err(|_| "The session edit directory is unavailable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Session edit privacy could not be established")?;
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file: File = options
        .open(path)
        .map_err(|_| "The session edit lock is unavailable")?;
    file.try_lock()
        .map_err(|_| "Another desktop window owns this session edit; retry after it reconciles")?;
    Ok(FileLock::from_locked(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gateway::{Connection, sidebar::SidebarSession},
        sidebar::Mutation,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    struct Workspace(PathBuf);
    impl Workspace {
        fn new() -> Self {
            Self(
                std::env::temp_dir()
                    .join(format!("hermes-sidebar-receipt-{}", uuid::Uuid::new_v4())),
            )
        }
    }
    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn stamp(name: &str) -> SourceStamp {
        SourceStamp::from_connection(&Connection::Url {
            endpoint: format!("https://{name}.receipt-fixture.invalid"),
            token: Some("synthetic".into()),
        })
        .unwrap()
    }
    fn receipt(source: &SourceStamp) -> SavedReceipt {
        SavedReceipt {
            version: 1,
            owner: source.principal().into(),
            other: BTreeMap::from([("future".into(), json!({"retain":7}))]),
            mutation: Mutation {
                session: SidebarSession {
                    id: "tip".into(),
                    profile: Some("research".into()),
                    _lineage_root_id: Some("root".into()),
                    ..Default::default()
                },
                payload: json!({"pinned":true}),
                delete: false,
                identity: Some(
                    json!({"request_id":"original-request","expected_revision":3,"expected_generation":8}),
                ),
                owner_connection: 1,
                source: Some(source.clone()),
                other: BTreeMap::from([("futureMutation".into(), json!(42))]),
            },
        }
    }
    #[test]
    fn legacy_receipt_migrates_only_to_its_proved_owner_and_keeps_unknown_fields() {
        let root = Workspace::new();
        let source = stamp("a");
        let mut raw = serde_json::to_value(receipt(&source)).unwrap();
        raw.as_object_mut().unwrap().remove("version");
        raw["mutation"].as_object_mut().unwrap().remove("source");
        let bytes = serde_json::to_vec(&raw).unwrap();
        crate::client::private_write(&root.0.join("sidebar-edit.json"), &bytes).unwrap();
        let mut wrong = Store::new(root.0.clone(), stamp("b"));
        assert!(wrong.load().unwrap().is_none());
        assert_eq!(
            std::fs::read(root.0.join("sidebar-edit.json")).unwrap(),
            bytes
        );
        let mut correct = Store::new(root.0.clone(), source.clone());
        let migrated = correct.load().unwrap().unwrap();
        assert_eq!(migrated.mutation.source, Some(source));
        assert_eq!(migrated.other["future"], json!({"retain":7}));
        assert_eq!(migrated.mutation.other["futureMutation"], 42);
        assert_eq!(
            migrated.mutation.identity,
            raw["mutation"]["identity"]
                .as_object()
                .map(|_| raw["mutation"]["identity"].clone())
        );
        // Model an interrupted migration's duplicate legacy copy. Retirement removes it before the scoped receipt.
        crate::client::private_write(&root.0.join("sidebar-edit.json"), &bytes).unwrap();
        correct.retire(&migrated).unwrap();
        assert!(correct.load().unwrap().is_none());
    }
    #[test]
    fn newer_receipt_schema_is_retained_without_owning_or_overwriting_it() {
        let root = Workspace::new();
        let source = stamp("a");
        let mut store = Store::new(root.0.clone(), source.clone());
        let mut raw = serde_json::to_value(receipt(&source)).unwrap();
        raw["version"] = json!(99);
        let bytes = serde_json::to_vec(&raw).unwrap();
        crate::client::private_write(&store.path(), &bytes).unwrap();
        assert!(store.load().is_err());
        assert!(store.begin().is_err());
        assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
        for (field, value) in [
            ("schema_version", json!(2)),
            (
                "mutation",
                json!({"session":{"id":"tip"},"payload":{"pinned":true},"identity":42,"delete":false,"owner_connection":1,"source":source}),
            ),
        ] {
            let mut raw = serde_json::to_value(receipt(&source)).unwrap();
            raw[field] = value;
            let bytes = serde_json::to_vec(&raw).unwrap();
            crate::client::private_write(&store.path(), &bytes).unwrap();
            assert!(store.load().is_err());
            assert!(store.begin().is_err());
            assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
        }
    }
}
