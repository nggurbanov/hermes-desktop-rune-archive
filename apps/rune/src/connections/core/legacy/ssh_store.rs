//! Private credentials and cross-process ownership holds for an SSH generation.
use super::*;
use crate::connections::installed_ssh::{
    CredentialState, CredentialStore, LifetimeLease, StoredCredential,
};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) struct SshCredentialStore {
    root: PathBuf,
    secrets: Mutex<Box<dyn Secrets>>,
    held: Mutex<HashMap<String, File>>,
    // Revalidate an attachment independently of cleanup of its existing generation.
    validate: Arc<dyn Fn() -> Result<crate::client::FileLock, String> + Send + Sync>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Record {
    nonce: String,
    spawn_token: String,
    served_token: Option<String>,
}
impl SshCredentialStore {
    pub(super) fn new(
        root: PathBuf,
        secrets: Box<dyn Secrets>,
        validate: Arc<dyn Fn() -> Result<crate::client::FileLock, String> + Send + Sync>,
    ) -> Self {
        Self {
            root,
            secrets: Mutex::new(secrets),
            held: Mutex::new(HashMap::new()),
            validate,
        }
    }
    fn id(owner: &str) -> Result<String, String> {
        if owner.len() != 32
            || !owner
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Invalid SSH owner".into());
        }
        Ok(format!("legacy-ssh-{owner}"))
    }
    fn open_lock(&self, owner: &str, kind: &str) -> Result<File, String> {
        Self::id(owner)?;
        fs::create_dir_all(&self.root).map_err(|_| "SSH ownership directory unavailable")?;
        let metadata =
            fs::symlink_metadata(&self.root).map_err(|_| "SSH ownership directory unavailable")?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("SSH ownership directory unavailable".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != rustix::process::geteuid().as_raw() {
                return Err("SSH ownership directory belongs to another user".into());
            }
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
                .map_err(|_| "SSH ownership directory unavailable")?;
        }
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600)
                .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
        }
        let file = opts
            .open(self.root.join(format!("{owner}.{kind}.lock")))
            .map_err(|_| "SSH ownership lock unavailable")?;
        let metadata = file
            .metadata()
            .map_err(|_| "SSH ownership lock unavailable")?;
        if !metadata.is_file() {
            return Err("SSH ownership lock unavailable".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != rustix::process::geteuid().as_raw() {
                return Err("SSH ownership lock belongs to another user".into());
            }
            if metadata.mode() & 0o077 != 0 {
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|_| "SSH ownership lock unavailable")?;
            }
        }
        Ok(file)
    }
    fn read(&self, owner: &str) -> Result<Option<Record>, String> {
        let id = Self::id(owner)?;
        self.secrets
            .lock()
            .map_err(|_| "SSH credential store unavailable")?
            .get_optional(&id)?
            .map(|text| {
                serde_json::from_str::<Record>(&text)
                    .map_err(|_| "SSH credential needs repair".into())
            })
            .transpose()
    }
    fn write(&self, owner: &str, record: &Record) -> Result<(), String> {
        let id = Self::id(owner)?;
        let text = serde_json::to_string(record).map_err(|_| "SSH credential encoding failed")?;
        self.secrets
            .lock()
            .map_err(|_| "SSH credential store unavailable")?
            .set(&id, &text)
    }
    fn matches(record: &Record, nonce: &str, token: &str) -> bool {
        record.nonce == nonce && record.spawn_token == token
    }
    fn claim_file(&self, owner: &str) -> Result<File, String> {
        let file = self.open_lock(owner, "bootstrap")?;
        file.try_lock()
            .map_err(|_| "Another SSH operation owns this generation")?;
        Ok(file)
    }
    fn with_held<T>(
        &self,
        owner: &str,
        nonce: &str,
        token: &str,
        f: impl FnOnce(&Record) -> Result<T, String>,
    ) -> Result<T, String> {
        let held = self.held.lock().map_err(|_| "SSH ownership lock failed")?;
        if !held.contains_key(owner) {
            return Err("SSH generation is not claimed".into());
        }
        let record = self.read(owner)?.ok_or("SSH credential is missing")?;
        if !Self::matches(&record, nonce, token) {
            return Err("SSH generation changed".into());
        }
        f(&record)
    }
}
struct SshLifetime(File);
impl LifetimeLease for SshLifetime {
    fn try_last(&mut self) -> Result<bool, String> {
        self.0.unlock().map_err(|_| "SSH lifetime lock failed")?;
        match self.0.try_lock() {
            Ok(()) => Ok(true),
            Err(std::fs::TryLockError::WouldBlock) => Ok(false),
            Err(_) => Err("SSH lifetime lock failed".into()),
        }
    }
}
impl CredentialStore for SshCredentialStore {
    fn load(&self, owner: &str) -> Result<CredentialState, String> {
        let _route_guard = (self.validate)()?;
        let lock = self.open_lock(owner, "bootstrap")?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(CredentialState::Pending),
            Err(_) => return Err("SSH ownership lock failed".into()),
        }
        let record = self.read(owner)?;
        lock.unlock().map_err(|_| "SSH ownership lock failed")?;
        Ok(match record {
            Some(r) => CredentialState::Ready(StoredCredential {
                nonce: r.nonce,
                spawn_token: r.spawn_token,
                served_token: r.served_token,
            }),
            None => CredentialState::Missing,
        })
    }
    fn reserve(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        let file = self.claim_file(owner)?;
        let _route_guard = (self.validate)()?;
        if self.read(owner)?.is_some() {
            return Err("SSH generation already exists".into());
        }
        self.write(
            owner,
            &Record {
                nonce: nonce.into(),
                spawn_token: token.into(),
                served_token: None,
            },
        )?;
        self.held
            .lock()
            .map_err(|_| "SSH ownership lock failed")?
            .insert(owner.into(), file);
        Ok(())
    }
    fn claim(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        let file = self.claim_file(owner)?;
        let record = self.read(owner)?.ok_or("SSH credential is missing")?;
        if !Self::matches(&record, nonce, token) {
            return Err("SSH generation changed".into());
        }
        self.held
            .lock()
            .map_err(|_| "SSH ownership lock failed")?
            .insert(owner.into(), file);
        Ok(())
    }
    fn finish(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        self.with_held(owner, nonce, token, |_| Ok(()))?;
        self.held
            .lock()
            .map_err(|_| "SSH ownership lock failed")?
            .remove(owner)
            .ok_or("SSH generation is not claimed")?
            .unlock()
            .map_err(|_| "SSH ownership lock failed".to_owned())
    }
    fn activate(
        &self,
        owner: &str,
        nonce: &str,
        token: &str,
    ) -> Result<Box<dyn LifetimeLease>, String> {
        let file = self.open_lock(owner, "lifetime")?;
        file.try_lock_shared()
            .map_err(|_| "SSH generation is closing")?;
        let _route_guard = (self.validate)()?;
        self.with_held(owner, nonce, token, |_| Ok(()))?;
        self.finish(owner, nonce, token)?;
        Ok(Box::new(SshLifetime(file)))
    }
    fn reconcile(&self, owner: &str, nonce: &str, token: &str, served: &str) -> Result<(), String> {
        self.with_held(owner, nonce, token, |record| {
            let _route_guard = (self.validate)()?;
            self.write(
                owner,
                &Record {
                    nonce: record.nonce.clone(),
                    spawn_token: record.spawn_token.clone(),
                    served_token: Some(served.into()),
                },
            )
        })
    }
    fn abort(&self, owner: &str, nonce: &str, token: &str) -> Result<(), String> {
        let mut held = self.held.lock().map_err(|_| "SSH ownership lock failed")?;
        let temporary = if held.contains_key(owner) {
            None
        } else {
            Some(self.claim_file(owner)?)
        };
        let record = self.read(owner)?.ok_or("SSH credential is missing")?;
        if !Self::matches(&record, nonce, token) {
            return Err("SSH generation changed".into());
        }
        let id = Self::id(owner)?;
        self.secrets
            .lock()
            .map_err(|_| "SSH credential store unavailable")?
            .delete(&id)?;
        if let Some(file) = held.remove(owner).or(temporary) {
            file.unlock().map_err(|_| "SSH ownership lock failed")?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Command, Stdio},
    };
    fn private_bus() {
        let home = std::env::var("HOME").unwrap();
        let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
        let root = std::path::Path::new(&home).parent().unwrap();
        assert!(
            root.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("rune-private-credentials-")
        );
        assert_eq!(bus, format!("unix:path={}", root.join("bus").display()));
    }
    fn store(root: &std::path::Path) -> SshCredentialStore {
        let registry = root.join("registry.json");
        SshCredentialStore::new(
            root.join("locks"),
            Box::new(SystemSecrets),
            Arc::new(move || super::super::super::registry::transaction(&registry)),
        )
    }
    #[test]
    #[ignore = "requires isolated private Secret Service"]
    fn independent_process_lifetime_and_exact_generation_cleanup() {
        private_bus();
        let owner = "0123456789abcdef0123456789abcdef";
        if let Ok(root) = std::env::var("RUNE_SSH_STORE_CHILD") {
            let store = store(std::path::Path::new(&root));
            store.claim(owner, "nonce", "spawn").unwrap();
            let _lease = store.activate(owner, "nonce", "spawn").unwrap();
            println!("CHILD_READY");
            std::io::stdout().flush().unwrap();
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
            return;
        }
        let root =
            std::env::temp_dir().join(format!("rune-ssh-store-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let store = store(&root);
        store.reserve(owner, "nonce", "spawn").unwrap();
        store.reconcile(owner, "nonce", "spawn", "served").unwrap();
        let mut own = store.activate(owner, "nonce", "spawn").unwrap();
        let exe = std::env::current_exe().unwrap();
        let mut child = Command::new(exe).args(["connections::core::legacy::ssh_store::tests::independent_process_lifetime_and_exact_generation_cleanup", "--exact", "--ignored", "--nocapture"])
            .env("RUNE_SSH_STORE_CHILD", &root).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            assert!(output.read_line(&mut line).unwrap() > 0);
            if line.contains("CHILD_READY") {
                break;
            }
        }
        store.claim(owner, "nonce", "spawn").unwrap();
        assert!(!own.try_last().unwrap());
        store.finish(owner, "nonce", "spawn").unwrap();
        assert!(store.abort(owner, "wrong", "spawn").is_err());
        match store.load(owner).unwrap() {
            CredentialState::Ready(record) => {
                assert_eq!(record.served_token.as_deref(), Some("served"))
            }
            _ => panic!("expected ready credential"),
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        store.claim(owner, "nonce", "spawn").unwrap();
        let mut final_hold = store.activate(owner, "nonce", "spawn").unwrap();
        let stale = SshCredentialStore::new(
            root.join("locks"),
            Box::new(SystemSecrets),
            Arc::new(|| Err("route retired".into())),
        );
        assert!(stale.load(owner).is_err());
        assert!(stale.reserve(owner, "later", "later-token").is_err());
        // Retiring settings cannot prevent exact-generation cleanup.
        stale.claim(owner, "nonce", "spawn").unwrap();
        assert!(final_hold.try_last().unwrap());
        stale.abort(owner, "nonce", "spawn").unwrap();
        assert!(matches!(
            store.load(owner).unwrap(),
            CredentialState::Missing
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
