//! Stable client identity for remote SSH ownership, independent of destination.
use std::{fs, io::Read, path::Path};

pub(super) fn load_or_create(registry_path: &Path) -> Result<String, String> {
    let parent = registry_path.parent().ok_or("Invalid client state path")?;
    let path = parent.join("desktop-installation.json");
    let _guard = super::super::registry::transaction(&path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    match options.open(&path) {
        Ok(mut file) => {
            let metadata = file
                .metadata()
                .map_err(|_| "The Desktop installation identity could not be inspected")?;
            if !metadata.is_file() || metadata.len() > 4096 {
                return Err("The Desktop installation identity needs repair".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                if metadata.uid() != rustix::process::geteuid().as_raw() {
                    return Err("The Desktop installation identity belongs to another user".into());
                }
                if metadata.mode() & 0o077 != 0 {
                    file.set_permissions(fs::Permissions::from_mode(0o600))
                        .map_err(
                            |_| "The Desktop installation identity could not be made private",
                        )?;
                }
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|_| "The Desktop installation identity could not be read")?;
            if let Some(id) = parse(&bytes) {
                return Ok(id);
            }
            // Preserve the malformed state; replacing it silently could orphan a remote owner.
            Err("The Desktop installation identity needs repair".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let id = uuid::Uuid::new_v4().to_string();
            let bytes = serde_json::to_vec(&serde_json::json!({"installationId": id}))
                .expect("installation identity serializes");
            crate::client::private_write(&path, &bytes)?;
            Ok(id)
        }
        Err(_) => Err("The Desktop installation identity could not be opened safely".into()),
    }
}
fn parse(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let id = uuid::Uuid::parse_str(value.get("installationId")?.as_str()?).ok()?;
    (id.get_version_num() == 4).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    #[test]
    fn concurrent_private_clients_share_one_stable_installation_identity() {
        let root =
            std::env::temp_dir().join(format!("hermes-installation-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("connections.json");
        let barrier = Arc::new(Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    load_or_create(&path).unwrap()
                })
            })
            .collect();
        let ids: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(ids.iter().all(|id| id == &ids[0]));
        assert_eq!(load_or_create(&path).unwrap(), ids[0]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_identity_is_preserved_and_never_silently_replaced() {
        let root =
            std::env::temp_dir().join(format!("hermes-installation-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("connections.json");
        let identity = root.join("desktop-installation.json");
        fs::write(&identity, b"invalid").unwrap();
        assert!(load_or_create(&path).is_err());
        assert_eq!(fs::read(&identity).unwrap(), b"invalid");
        fs::remove_dir_all(root).unwrap();
    }
}
