//! Filesystem operations are available only for a confirmed local connection.
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const PREVIEW: usize = 512 * 1024;
const MAX_SOURCE: u64 = 64 * 1024 * 1024;
const NOISE: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".cache",
    ".next",
    ".turbo",
    ".venv",
    "__pycache__",
    "build",
    "dist",
    "node_modules",
    "target",
    "venv",
];
fn requested(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() || value.contains('\0') {
        return Err("Choose a file or directory path".into());
    }
    let lower = value.replace('\\', "/").to_lowercase();
    if lower.starts_with("//?/")
        || lower.starts_with("//./")
        || lower.contains("globalroot/device/")
    {
        return Err("Device paths cannot be opened".into());
    }
    let path = if value.starts_with("file:") {
        url::Url::parse(value)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .ok_or("Invalid file URL")?
    } else if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        PathBuf::from(
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .ok_or("The local home is unavailable")?,
        )
        .join(value.get(2..).unwrap_or(""))
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        return Err("Choose an absolute filesystem path".into());
    }
    Ok(path)
}
fn sensitive(path: &Path) -> bool {
    let path = path.to_string_lossy().replace('\\', "/").to_lowercase();
    let name = path.rsplit('/').next().unwrap_or("");
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    path.contains("/.ssh/")
        || path.contains("/.gnupg/")
        || path.ends_with("/.aws/credentials")
        || name == ".env"
        || name
            .strip_prefix(".env.")
            .is_some_and(|s| !["dist", "example", "sample", "template"].contains(&s))
        || (["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"]
            .iter()
            .any(|key| name == *key || name.starts_with(&format!("{key}.")))
            && !name.ends_with(".pub"))
        || ["kdbx", "p12", "pem", "pfx"].contains(&ext)
        || [".npmrc", ".netrc", ".pypirc"].contains(&name)
}
fn readable(path: &Path) -> Result<(PathBuf, fs::Metadata), String> {
    if sensitive(path) {
        return Err("This sensitive file cannot be previewed".into());
    }
    let real = fs::canonicalize(path).map_err(|_| "The file could not be found or accessed")?;
    if sensitive(&real) {
        return Err("The selected link resolves to a sensitive file".into());
    }
    let metadata = fs::metadata(&real).map_err(|_| "File metadata could not be read")?;
    if !metadata.is_file() {
        return Err("Only regular files can be previewed".into());
    }
    if metadata.len() > MAX_SOURCE {
        return Err("This file is too large for a text preview".into());
    }
    Ok((real, metadata))
}
pub fn execute(route: &str, body: &Value) -> Result<Value, String> {
    let path = requested(body["path"].as_str().unwrap_or(""))?;
    match route {
        "/api/fs/list" => {
            let mut entries = vec![];
            let ignore = gitignore(&path);
            for entry in fs::read_dir(&path).map_err(|_| "The directory could not be read")? {
                let entry = entry.map_err(|_| "A directory entry could not be read")?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if NOISE.contains(&name.as_str()) {
                    continue;
                }
                let directory = fs::metadata(entry.path()).is_ok_and(|m| m.is_dir());
                entries.push(json!({"name":name,"path":entry.path().to_string_lossy(),"isDirectory":directory,"ignored":ignore.matched_path_or_any_parents(entry.path(),directory).is_ignore()}));
            }
            entries.sort_by(|a, b| {
                b["isDirectory"]
                    .as_bool()
                    .cmp(&a["isDirectory"].as_bool())
                    .then_with(|| {
                        a["name"]
                            .as_str()
                            .unwrap_or("")
                            .to_lowercase()
                            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
                    })
            });
            Ok(json!({"entries":entries}))
        }
        "/api/fs/read-data-url" => {
            let (real, metadata) = readable(&path)?;
            if metadata.len() > 32 * 1024 * 1024 {
                return Err("This image is too large for a preview".into());
            }
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let mime = match ext.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "gif" => "image/gif",
                "svg" => "image/svg+xml",
                "bmp" => "image/bmp",
                _ => return Err("This native media preview is unavailable".into()),
            };
            use base64::Engine;
            let bytes = fs::read(real).map_err(|_| "The image could not be read")?;
            Ok(
                json!({"dataUrl":format!("data:{mime};base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes))}),
            )
        }
        "/api/fs/read-text" => {
            let (real, metadata) = readable(&path)?;
            let mut bytes = vec![];
            fs::File::open(real)
                .map_err(|_| "The file could not be opened")?
                .take(PREVIEW as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "The file could not be read")?;
            let binary = bytes.contains(&0);
            Ok(
                json!({"path":path,"text":String::from_utf8_lossy(&bytes),"binary":binary,"byteSize":metadata.len(),"truncated":metadata.len()>PREVIEW as u64,"language":path.extension().and_then(|s|s.to_str()).unwrap_or("text")}),
            )
        }
        "/api/fs/write-text" => {
            let (real, metadata) = readable(&path)?;
            if metadata.len() > PREVIEW as u64 {
                return Err("A truncated preview cannot overwrite the file".into());
            }
            let current = fs::read_to_string(&real).map_err(|_| "The file is not editable text")?;
            let expected = body["expected"]
                .as_str()
                .ok_or("The original file snapshot is missing")?;
            if current != expected {
                return Err(
                    "The file changed on disk. Reload it or save your edits elsewhere".into(),
                );
            }
            let content = body["content"]
                .as_str()
                .ok_or("The edited contents are missing")?;
            atomic_output(&real, content.as_bytes(), Some(metadata.permissions()))?;
            Ok(json!({"ok":true,"path":path}))
        }
        "/api/fs/write-project-idea" => write_project_idea(&path, body),
        "/api/fs/rename" => {
            let name = body["name"].as_str().unwrap_or("").trim();
            if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
                return Err("Choose a single file or folder name".into());
            }
            let parent = path
                .parent()
                .filter(|_| path.file_name().is_some())
                .ok_or("The filesystem root cannot be renamed")?;
            let target = parent.join(name);
            if target
                .try_exists()
                .map_err(|_| "The destination could not be checked")?
            {
                return Err("A file or folder with this name already exists".into());
            }
            fs::rename(path, &target).map_err(|_| "The file or folder could not be renamed")?;
            Ok(json!({"ok":true,"path":target}))
        }
        "/api/fs/trash" => {
            if path.parent().is_none() || path.file_name().is_none() {
                return Err("The filesystem root cannot be moved to Trash".into());
            }
            trash::delete(path).map_err(|_| "The selected file could not be moved to Trash")?;
            Ok(json!({"ok":true}))
        }
        "/api/fs/mkdir" => {
            fs::create_dir(&path).map_err(|_| "The directory could not be created")?;
            Ok(json!({"ok":true,"path":path}))
        }
        _ => Err("This native filesystem operation is unavailable".into()),
    }
}
fn write_project_idea(path: &Path, body: &Value) -> Result<Value, String> {
    if path.file_name().and_then(|n| n.to_str()) != Some("IDEA.md") || sensitive(path) {
        return Err("The project idea destination is invalid".into());
    }
    let content = body["content"].as_str().ok_or("The idea is missing")?;
    if content.encode_utf16().count() > 1_000_000 {
        return Err("The idea is too large".into());
    }
    let parent = fs::canonicalize(path.parent().ok_or("The project folder is invalid")?)
        .map_err(|_| "The project folder could not be accessed")?;
    if sensitive(&parent.join("IDEA.md")) || !parent.is_dir() {
        return Err("The project folder is invalid".into());
    }
    let target = parent.join("IDEA.md");
    let permissions = match fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_SOURCE => Some(meta.permissions()),
        Ok(_) => return Err("The idea destination is not an editable regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("The idea destination could not be checked".into()),
    };
    atomic_output(&target, content.as_bytes(), permissions)?;
    Ok(json!({"ok":true,"path":path}))
}
fn atomic_output(
    path: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
) -> Result<(), String> {
    let parent = path.parent().ok_or("The file destination is invalid")?;
    let temporary = parent.join(format!(".hermes-edit-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| "The edit could not be staged")?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)
                .map_err(|_| "The existing permissions could not be preserved")?;
        }
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "The edit could not be saved")?;
        fs::rename(&temporary, path).map_err(|_| "The edit could not be committed")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub fn save_copy(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Choose an absolute file destination".into());
    }
    let permissions = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Some(meta.permissions()),
        Ok(_) => return Err("Choose a regular file destination".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("The file destination could not be checked".into()),
    };
    atomic_output(path, bytes, permissions)
}
fn gitignore(path: &Path) -> ignore::gitignore::Gitignore {
    let root = path
        .ancestors()
        .find(|p| p.join(".git").exists())
        .unwrap_or(path);
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    let mut dirs: Vec<_> = path
        .ancestors()
        .take_while(|p| p.starts_with(root))
        .collect();
    dirs.reverse();
    for dir in dirs {
        let file = dir.join(".gitignore");
        if file.is_file() {
            builder.add(file);
        }
    }
    builder
        .build()
        .unwrap_or_else(|_| ignore::gitignore::Gitignore::empty())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("hermes-fs-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn edits_verify_current_contents_and_never_replace_from_a_truncated_preview() {
        let dir = dir();
        let path = dir.join("notes.txt");
        fs::write(&path, "old").unwrap();
        assert_eq!(
            execute("/api/fs/read-text", &json!({"path":path})).unwrap()["text"],
            "old"
        );
        fs::write(&path, "external").unwrap();
        assert!(
            execute(
                "/api/fs/write-text",
                &json!({"path":path,"expected":"old","content":"edited"})
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        execute(
            "/api/fs/write-text",
            &json!({"path":path,"expected":"external","content":"edited"}),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "edited");
        fs::write(&path, vec![b'a'; PREVIEW + 1]).unwrap();
        assert_eq!(
            execute("/api/fs/read-text", &json!({"path":path})).unwrap()["truncated"],
            true
        );
        assert!(
            execute(
                "/api/fs/write-text",
                &json!({"path":path,"expected":"edited","content":"short"})
            )
            .is_err()
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn sensitive_paths_links_and_rename_collisions_follow_native_contract() {
        let dir = dir();
        let path = dir.join(".env");
        fs::write(&path, "synthetic").unwrap();
        assert!(execute("/api/fs/read-text", &json!({"path":path})).is_err());
        assert!(!sensitive(&dir.join(".env.example")));
        fs::write(dir.join("other"), "keep").unwrap();
        assert!(execute("/api/fs/rename", &json!({"path":path,"name":"other"})).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, dir.join("public.txt")).unwrap();
            assert!(execute("/api/fs/read-text", &json!({"path":dir.join("public.txt")})).is_err());
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn directory_listing_keeps_dotfiles_and_tags_gitignored_entries() {
        let dir = dir();
        fs::create_dir(dir.join(".git")).unwrap();
        fs::write(dir.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.join("ignored.txt"), "").unwrap();
        fs::write(dir.join(".editorconfig"), "").unwrap();
        let listing = execute("/api/fs/list", &json!({"path":dir})).unwrap();
        let rows = listing["entries"].as_array().unwrap();
        assert_eq!(
            rows.iter().find(|r| r["name"] == "ignored.txt").unwrap()["ignored"],
            true
        );
        assert_eq!(
            rows.iter().find(|r| r["name"] == ".editorconfig").unwrap()["ignored"],
            false
        );
        assert!(!rows.iter().any(|r| r["name"] == ".git"));
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod project_idea_regression {
    use super::*;

    #[test]
    fn local_create_idea_writes_fresh_file() {
        let dir = std::env::temp_dir().join(format!("idea-red-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("IDEA.md");
        execute(
            "/api/fs/write-project-idea",
            &json!({"path": path, "content": "A real idea\n"}),
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"A real idea\n");
        execute(
            "/api/fs/write-project-idea",
            &json!({"path": path, "content": "replacement\n"}),
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"replacement\n");
        assert!(
            execute(
                "/api/fs/write-text",
                &json!({"path": path, "content": "no snapshot"})
            )
            .is_err()
        );
        assert!(
            execute(
                "/api/fs/write-project-idea",
                &json!({"path": dir.join("other.md"), "content": "bad"})
            )
            .is_err()
        );
        assert!(
            execute(
                "/api/fs/write-project-idea",
                &json!({"path": path, "content": "x".repeat(1_000_001)})
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"replacement\n");
    }
}
