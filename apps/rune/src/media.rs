//! Private native-open copies belong to this process, never to a remote path's spelling.
use std::path::{Path, PathBuf};
pub struct TempMedia {
    directory: Option<PathBuf>,
}
impl TempMedia {
    pub fn new() -> Self {
        Self { directory: None }
    }
    pub fn reserve(&mut self, label: &str) -> Result<PathBuf, String> {
        if self.directory.is_none() {
            let path = std::env::temp_dir()
                .join(format!("hermes-media-{}", uuid::Uuid::new_v4().simple()));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(&path)
                .map_err(|_| "A private media directory could not be created")?;
            self.directory = Some(path);
        }
        let name = label.rsplit(['/', '\\']).next().unwrap_or("artifact");
        let name: String = name
            .chars()
            .filter(|c| {
                !c.is_control()
                    && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            })
            .take(180)
            .collect();
        let name = if name.trim_matches('.').is_empty() {
            "artifact"
        } else {
            &name
        };
        Ok(self
            .directory
            .as_ref()
            .unwrap()
            .join(format!("{}-{name}", uuid::Uuid::new_v4().simple())))
    }
    pub fn contains(&self, path: &Path) -> bool {
        self.directory
            .as_ref()
            .is_some_and(|d| path.parent() == Some(d.as_path()))
    }
}
impl Drop for TempMedia {
    fn drop(&mut self) {
        if let Some(path) = &self.directory {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}
pub fn local_file_url(path: &str) -> Result<String, String> {
    if path.starts_with("file:") {
        let url = url::Url::parse(path).map_err(|_| "Invalid local file URL")?;
        let path = url
            .to_file_path()
            .map_err(|_| "This file URL is not local to this operating system")?;
        if !path.is_absolute() {
            return Err("The local file path is not absolute".into());
        }
        return url::Url::from_file_path(path)
            .map(|u| u.into())
            .map_err(|_| "Invalid local file path".into());
    }
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("This artifact path must be resolved by its owning agent".into());
    }
    url::Url::from_file_path(path)
        .map(|u| u.into())
        .map_err(|_| "Invalid local file path".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_media_paths_never_reuse_remote_path_and_drop_cleans() {
        let path;
        {
            let mut t = TempMedia::new();
            path = t.reserve("../../payload.png").unwrap();
            assert!(t.contains(&path));
            assert_eq!(path.extension().unwrap(), "png");
            assert_ne!(path, t.reserve("../../payload.png").unwrap());
            std::fs::write(&path, b"fixture").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(path.parent().unwrap())
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
            }
        }
        assert!(!path.exists());
    }
    #[test]
    fn native_file_urls_never_expand_remote_home_or_relative_working_directory() {
        assert!(local_file_url("~/a.txt").is_err());
        assert!(local_file_url("./a.txt").is_err());
        assert!(local_file_url("../a.txt").is_err());
        assert_eq!(
            local_file_url("/tmp/a b.txt").unwrap(),
            "file:///tmp/a%20b.txt"
        );
    }
}

pub struct FileWriter {
    send: std::sync::mpsc::Sender<(u64, PathBuf, Vec<u8>)>,
    receive: std::sync::mpsc::Receiver<(u64, Result<PathBuf, String>)>,
    next: u64,
}
impl FileWriter {
    pub fn new(waker: rune::Waker) -> Self {
        let (send, tasks) = std::sync::mpsc::channel::<(u64, PathBuf, Vec<u8>)>();
        let (results, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Ok((id, path, bytes)) = tasks.recv() {
                let result = write_private(&path, &bytes).map(|_| path);
                if results.send((id, result)).is_err() {
                    break;
                }
                waker();
            }
        });
        Self {
            send,
            receive,
            next: 0,
        }
    }
    pub fn write(&mut self, path: PathBuf, bytes: Vec<u8>) -> Result<u64, String> {
        self.next = self.next.wrapping_add(1);
        let id = self.next;
        self.send
            .send((id, path, bytes))
            .map_err(|_| "The native media writer stopped")?;
        Ok(id)
    }
    pub fn poll(&mut self) -> Vec<(u64, Result<PathBuf, String>)> {
        self.receive.try_iter().collect()
    }
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or("The destination has no parent directory")?;
    let temporary = parent.join(format!(
        ".hermes-media-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|_| "The native media copy could not be created")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "The native media copy could not be written")?;
        std::fs::rename(&temporary, path)
            .map_err(|_| "The native media copy could not replace the selected destination")
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result.map_err(str::to_owned)
}
