//! The requested native export destination stays on a worker and is owner-fenced.
use super::*;
use std::{
    io::Write,
    sync::mpsc::{self, Receiver, Sender},
};
pub enum Payload {
    Text(String),
    Json(Value),
}
pub struct Writer {
    send: Sender<(u64, PathBuf, Payload)>,
    receive: Receiver<(u64, Result<(), String>)>,
}
impl Writer {
    pub fn new(waker: rune::Waker) -> Self {
        let (send, requests) = mpsc::channel::<(u64, PathBuf, Payload)>();
        let (replies, receive) = mpsc::channel();
        std::thread::spawn(move || {
            for (epoch, path, payload) in requests {
                let result = write(path, payload);
                if replies.send((epoch, result)).is_err() {
                    break;
                }
                waker();
            }
        });
        Self { send, receive }
    }
}
fn write(path: PathBuf, payload: Payload) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("The export path has no parent folder")?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = parent.join(format!(".hermes-export-{}-{nonce}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&tmp)
        .map_err(|_| "The export file could not be created")?;
    let result = (|| {
        match payload {
            Payload::Text(text) => file
                .write_all(text.as_bytes())
                .map_err(|_| "The export could not be written")?,
            Payload::Json(value) => serde_json::to_writer_pretty(&mut file, &value)
                .map_err(|_| "The export could not be encoded")?,
        };
        file.sync_all()
            .map_err(|_| "The export could not be flushed")?;
        drop(file);
        std::fs::rename(&tmp, &path).map_err(|_| "The export destination could not be replaced")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
impl Operations {
    pub(super) fn save_file(&mut self, path: PathBuf, payload: Payload) {
        let Some(w) = &self.worker else {
            self.export_error = Some("The native file service is unavailable".into());
            self.exporting = false;
            return;
        };
        if w.send.send((self.scope_epoch, path, payload)).is_err() {
            self.export_error = Some("The native file service stopped".into());
            self.exporting = false;
            return;
        }
        self.saving_file = true;
    }
    pub(super) fn poll_writer(&mut self) -> bool {
        let replies = self
            .worker
            .as_ref()
            .map(|w| w.receive.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut changed = false;
        for (epoch, result) in replies {
            if epoch != self.scope_epoch {
                continue;
            }
            self.saving_file = false;
            self.exporting = false;
            match result {
                Ok(()) => self.notice = Some("Export saved on this device".into()),
                Err(e) => self.export_error = Some(e),
            }
            changed = true;
        }
        changed
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_export_uses_requested_path_and_private_mode() {
        let folder = std::env::temp_dir().join(format!("hermes-ops-export-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("export.json");
        write(path.clone(), Payload::Json(json!({"safe":"synthetic"}))).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap()["safe"],
            "synthetic"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(folder).unwrap();
    }
}
