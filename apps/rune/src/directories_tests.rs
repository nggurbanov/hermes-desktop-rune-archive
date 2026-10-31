use super::*;

fn existing_listing() -> Directories {
    let mut directories = Directories::default();
    directories.apply(
        "directories.list",
        Ok(json!({"entries": [
            {"name": "folder", "path": "/server/folder", "isDirectory": true},
            {"name": "file", "path": "/server/file", "isDirectory": false}
        ]})),
    );
    assert_eq!(directories.entries.len(), 1);
    assert_eq!(directories.entries[0].path, "/server/folder");
    directories
}

#[test]
fn structured_listing_error_clears_stale_entries() {
    let mut directories = existing_listing();
    directories.loading = true;
    directories.apply(
        "directories.list",
        Ok(json!({"entries": [], "error": "EACCES"})),
    );
    assert!(!directories.loading);
    assert!(directories.entries.is_empty());
    assert_eq!(directories.error.as_deref(), Some("EACCES"));
}

#[test]
fn malformed_listing_clears_stale_entries() {
    let mut directories = existing_listing();
    directories.loading = true;
    directories.apply(
        "directories.list",
        Ok(json!({"entries": [{"name": "broken"}]})),
    );
    assert!(!directories.loading);
    assert!(directories.entries.is_empty());
    assert_eq!(
        directories.error.as_deref(),
        Some("The agent did not return a directory listing")
    );
}
