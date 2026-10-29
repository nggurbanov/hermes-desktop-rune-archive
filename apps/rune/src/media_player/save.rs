use super::Shared;
use crate::{
    gateway::{Connection, Gateway, stream},
    preview::{Source, Target},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
struct Progress<'a> {
    file: File,
    shared: &'a Shared,
    count: u64,
    last: Instant,
}
impl Write for Progress<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.shared.cancel.cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Audio save cancelled",
            ));
        }
        let n = self.file.write(bytes)?;
        self.count += n as u64;
        if self.last.elapsed() > Duration::from_millis(100) {
            let count = self.count;
            self.shared.update(|s| s.save_progress = count);
            self.last = Instant::now();
        }
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
pub(super) fn original(
    connection: Connection,
    target: Target,
    destination: &Path,
    shared: Arc<Shared>,
) -> Result<(), String> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("The save destination has no parent directory")?;
    let temporary = parent.join(format!(
        ".hermes-audio-save-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let cancel = stream::Cancel::default();
    *shared.save_cancel.lock().unwrap() = Some(cancel.clone());
    let result = (|| {
        if shared.cancel.cancelled() {
            return Err("Audio save cancelled".into());
        }

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&temporary)
            .map_err(|_| "The audio destination could not be created")?;
        let mut output = Progress {
            file,
            shared: &shared,
            count: 0,
            last: Instant::now(),
        };
        match target.source {
            Source::AgentPath(path) => {
                let request = stream::Request::download(path, target.owner.session, u64::MAX);
                let meta = Gateway::connect(&connection)?.stream_file_events(
                    &target.owner.profile,
                    &request,
                    &cancel,
                    |meta| {
                        if meta.status != 200 {
                            return Err(format!("Audio download refused (HTTP {})", meta.status));
                        }
                        shared.update(|s| s.save_total = meta.content_length);
                        Ok(())
                    },
                    &mut output,
                )?;
                if meta.content_length.is_some_and(|len| len != meta.received) {
                    return Err("The audio download was incomplete".into());
                }
            }
            Source::Inline { bytes, .. } => {
                shared.update(|s| s.save_total = Some(bytes.len() as u64));
                for chunk in bytes.chunks(64 * 1024) {
                    output
                        .write_all(chunk)
                        .map_err(|_| "The audio original could not be saved")?;
                }
            }
            Source::RemoteUrl(url) => {
                let mut source = stream::range::RangeSource::open_external(&url, cancel.clone())?;
                shared.update(|s| s.save_total = source.byte_len());
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let count = source
                        .read(&mut buffer)
                        .map_err(|_| "The external audio original could not be downloaded")?;
                    if count == 0 {
                        break;
                    }
                    output
                        .write_all(&buffer[..count])
                        .map_err(|_| "The external audio original could not be saved")?;
                }
            }
        }
        output
            .file
            .sync_all()
            .map_err(|_| "The saved audio could not be completed")?;
        if shared.cancel.cancelled() {
            return Err("Audio save cancelled".into());
        }
        let count = output.count;
        drop(output);
        fs::rename(&temporary, destination)
            .map_err(|_| "The audio original could not replace the destination")?;
        shared.update(|s| s.save_progress = count);
        Ok(())
    })();
    *shared.save_cancel.lock().unwrap() = None;
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
