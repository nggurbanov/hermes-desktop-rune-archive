//! File trees and text previews keep the read's profile/session through save and download.
mod ignore;
use crate::{
    composer::DraftScope,
    pages::{PageEffect, PageRequest},
};
use rune::TextEdit;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    #[serde(rename = "isDirectory")]
    pub directory: bool,
    #[serde(default)]
    pub ignored: bool,
}
#[derive(Default)]
pub struct Folder {
    pub entries: Vec<Entry>,
    pub loading: bool,
    pub error: Option<String>,
}
pub struct Editor {
    pub owner: DraftScope,
    pub path: String,
    pub text: TextEdit,
    pub original: String,
    saving: Option<String>,
    pub binary: bool,
    pub truncated: bool,
    pub size: u64,
    pub loading: bool,
    pub busy: bool,
    pub error: Option<String>,
}
impl Editor {
    pub fn dirty(&self) -> bool {
        self.text.text != self.original
    }
    pub fn writable(&self) -> bool {
        !self.binary && !self.truncated && !self.loading && !self.busy
    }
}
pub struct Files {
    pub owner: Option<DraftScope>,
    pub local: bool,
    pub root: String,
    pub path: TextEdit,
    pub search: TextEdit,
    pub folders: BTreeMap<String, Folder>,
    pub expanded: BTreeSet<String>,
    pub editor: Option<Editor>,
    pub loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub rename: Option<Entry>,
    pub name: TextEdit,
    pub trash: Option<Entry>,
    pub show_ignored: bool,
    pub mutating: bool,
    next_refresh: f64,
    clock: f64,
    ignore_files: BTreeMap<String, String>,
    ignore_next: BTreeMap<String, f64>,
    safe_images: Option<crate::image_previews::SafeImages>,
    image_job: Option<(u64, DraftScope, String)>,
    pub preview: Option<rune::Image>,
}
#[derive(Clone)]
pub enum Action {
    Refresh,
    Root(String),
    Toggle(String),
    Open(String),
    Save,
    Discard,
    ReloadEditor,
    AskRename(Entry),
    Rename,
    AskTrash(Entry),
    Trash,
    Cancel,
    ShowIgnored,
}
impl Default for Files {
    fn default() -> Self {
        Self::new()
    }
}
impl Files {
    pub fn new() -> Self {
        Self {
            owner: None,
            local: false,
            root: String::new(),
            path: TextEdit::new(""),
            search: TextEdit::new(""),
            folders: BTreeMap::new(),
            expanded: BTreeSet::new(),
            editor: None,
            loading: false,
            error: None,
            notice: None,
            rename: None,
            name: TextEdit::new(""),
            trash: None,
            show_ignored: false,
            mutating: false,
            next_refresh: 0.0,
            clock: 0.,
            ignore_files: Default::default(),
            ignore_next: Default::default(),
            safe_images: None,
            image_job: None,
            preview: None,
        }
    }
    pub fn init(&mut self, waker: rune::Waker) {
        self.safe_images = Some(crate::image_previews::SafeImages::new(waker));
    }
    pub fn reset(&mut self) {
        if let Some(images) = &mut self.safe_images {
            images.cancel_all();
        }
        let images = self.safe_images.take();
        *self = Self::new();
        self.safe_images = images;
    }
    pub fn poll_images(&mut self) -> bool {
        let results = self
            .safe_images
            .as_mut()
            .map(|images| images.poll())
            .unwrap_or_default();
        let mut changed = false;
        for decoded in results {
            if !self
                .image_job
                .as_ref()
                .is_some_and(|job| job.0 == decoded.id)
            {
                continue;
            }
            let (_, owner, path) = self.image_job.take().unwrap();
            if let Some(editor) = self
                .editor
                .as_mut()
                .filter(|e| e.owner == owner && e.path == path)
            {
                editor.loading = false;
                changed = true;
                match decoded.result {
                    Ok(image) => self.preview = Some(image),
                    Err(error) => editor.error = Some(error),
                }
            }
        }
        changed
    }
    pub fn blocks_owner_change(&self) -> bool {
        self.mutating
            || self.editor.as_ref().is_some_and(|e| e.dirty() || e.busy)
            || self.rename.is_some()
            || self.trash.is_some()
    }
    pub fn bind(&mut self, scope: DraftScope, local: bool, cwd: Option<String>) -> Vec<PageEffect> {
        if self.owner.as_ref() == Some(&scope) {
            return vec![];
        }
        if self
            .owner
            .as_ref()
            .is_some_and(|old| old.connection != scope.connection || old.profile != scope.profile)
        {
            self.reset();
        }
        self.owner = Some(scope);
        self.local = local;
        self.folders.clear();
        self.expanded.clear();
        self.root = cwd.unwrap_or_default();
        self.path.replace_text(&self.root);
        self.load()
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.error = None;
        if self.root.is_empty() {
            self.loading = true;
            return vec![(
                "files.cwd".into(),
                self.scoped(PageRequest::get("/api/fs/default-cwd")),
            )];
        }
        self.folder(self.root.clone())
    }
    fn scoped(&self, mut request: PageRequest) -> PageRequest {
        if let Some(owner) = &self.owner {
            request = request.profile(&owner.profile);
            if let Some(id) = &owner.session {
                request = request.query("session_id", id);
            }
        }
        request
    }
    fn read_request(&self, route: &str, path: &str, owner: &DraftScope) -> PageRequest {
        if self.local {
            PageRequest::local_fs(route, json!({"path":path})).profile(&owner.profile)
        } else {
            let mut request = PageRequest::get(route)
                .query("path", path)
                .profile(&owner.profile);
            if let Some(id) = &owner.session {
                request = request.query("session_id", id);
            }
            request
        }
    }
    fn folder(&mut self, path: String) -> Vec<PageEffect> {
        let Some(owner) = &self.owner else {
            return vec![];
        };
        self.expanded.insert(path.clone());
        self.folders.entry(path.clone()).or_default().loading = true;
        let request = self.read_request("/api/fs/list", &path, owner);
        vec![(
            format!("files.tree.{}", super::pages::segment(&path)),
            request,
        )]
    }
    pub fn action(&mut self, action: Action) -> Vec<PageEffect> {
        self.error = None;
        match action {
            Action::Refresh => {
                self.folders.clear();
                self.expanded.clear();
                self.ignore_files.clear();
                self.ignore_next.clear();
                self.load()
            }
            Action::Root(path) => {
                if path.trim().is_empty() {
                    return vec![];
                }
                self.root = path;
                self.path.replace_text(&self.root);
                self.folders.clear();
                self.expanded.clear();
                self.ignore_files.clear();
                self.ignore_next.clear();
                self.load()
            }
            Action::Toggle(path) => {
                if self.expanded.remove(&path) {
                    vec![]
                } else if self.folders.contains_key(&path) {
                    self.expanded.insert(path);
                    vec![]
                } else {
                    self.folder(path)
                }
            }
            Action::Open(path) => {
                if self.editor.as_ref().is_some_and(|e| e.dirty() || e.busy) {
                    self.error = Some(
                        "Save or discard the current edits before opening another file".into(),
                    );
                    return vec![];
                }
                let Some(owner) = self.owner.clone() else {
                    return vec![];
                };
                self.preview = None;
                self.image_job = None;
                if let Some(images) = &mut self.safe_images {
                    images.cancel_all();
                }
                let image = std::path::Path::new(&path)
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| {
                        ["png", "jpg", "jpeg", "webp", "gif", "svg", "bmp"]
                            .contains(&s.to_lowercase().as_str())
                    });
                let request = self.read_request(
                    if image {
                        "/api/fs/read-data-url"
                    } else {
                        "/api/fs/read-text"
                    },
                    &path,
                    &owner,
                );
                self.editor = Some(Editor {
                    owner,
                    path,
                    text: TextEdit::new(""),
                    original: String::new(),
                    saving: None,
                    binary: false,
                    truncated: false,
                    size: 0,
                    loading: true,
                    busy: false,
                    error: None,
                });
                vec![(
                    if image {
                        "files.editor.media"
                    } else {
                        "files.editor.read"
                    }
                    .into(),
                    request,
                )]
            }
            Action::ReloadEditor => {
                let Some(editor) = &self.editor else {
                    return vec![];
                };
                if editor.busy || editor.dirty() {
                    self.error = Some("Save or discard the current edits before reloading".into());
                    return vec![];
                }
                let request = self.read_request("/api/fs/read-text", &editor.path, &editor.owner);
                self.editor.as_mut().unwrap().loading = true;
                vec![("files.editor.read".into(), request)]
            }
            Action::Discard => {
                if let Some(editor) = &mut self.editor {
                    if !editor.busy {
                        editor.text.replace_text(&editor.original);
                    }
                }
                vec![]
            }
            Action::Save => {
                let Some(editor) = &self.editor else {
                    return vec![];
                };
                if !editor.writable() || !editor.dirty() {
                    return vec![];
                }
                let request = self.read_request("/api/fs/read-text", &editor.path, &editor.owner);
                let editor = self.editor.as_mut().unwrap();
                editor.busy = true;
                editor.error = None;
                editor.saving = Some(editor.text.text.clone());
                vec![("files.editor.check".into(), request)]
            }
            Action::AskRename(entry) => {
                if self.local && !self.mutating {
                    if self.editor.as_ref().is_some_and(|e| e.dirty() || e.busy) {
                        self.error = Some("Save or discard edits before renaming files".into());
                        return vec![];
                    }
                    self.name.replace_text(&entry.name);
                    self.rename = Some(entry);
                }
                vec![]
            }
            Action::Rename => {
                let Some(entry) = self.rename.as_ref() else {
                    return vec![];
                };
                if !self.local || self.mutating {
                    return vec![];
                }
                let Some(owner) = &self.owner else {
                    return vec![];
                };
                self.mutating = true;
                vec![(
                    "files.rename".into(),
                    PageRequest::local_fs(
                        "/api/fs/rename",
                        json!({"path":entry.path,"name":self.name.text.trim()}),
                    )
                    .profile(&owner.profile),
                )]
            }
            Action::AskTrash(entry) => {
                if self.local && !self.mutating {
                    if self.editor.as_ref().is_some_and(|e| e.dirty() || e.busy) {
                        self.error =
                            Some("Save or discard edits before moving files to Trash".into());
                        return vec![];
                    }
                    self.rename = None;
                    self.trash = Some(entry);
                }
                vec![]
            }
            Action::Trash => {
                let Some(entry) = self.trash.as_ref() else {
                    return vec![];
                };
                if !self.local || self.mutating {
                    return vec![];
                }
                let Some(owner) = &self.owner else {
                    return vec![];
                };
                self.mutating = true;
                vec![(
                    "files.trash".into(),
                    PageRequest::local_fs("/api/fs/trash", json!({"path":entry.path}))
                        .profile(&owner.profile),
                )]
            }
            Action::Cancel => {
                if self.mutating {
                    return vec![];
                }
                self.rename = None;
                self.trash = None;
                vec![]
            }
            Action::ShowIgnored => {
                self.show_ignored = !self.show_ignored;
                vec![]
            }
        }
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        if op.starts_with("files.ignore.") {
            self.ignore_reply(op, result);
            return vec![];
        }
        if let Some(encoded) = op.strip_prefix("files.tree.") {
            let path = decode(encoded);
            let folder = self.folders.entry(path.clone()).or_default();
            folder.loading = false;
            match result {
                Ok(v) => match serde_json::from_value(v["entries"].clone()) {
                    Ok(entries) => {
                        folder.entries = entries;
                        folder.error = v["error"].as_str().map(str::to_owned);
                    }
                    Err(_) => {
                        folder.error =
                            Some("The agent returned an invalid directory listing".into())
                    }
                },
                Err(error) => folder.error = Some(error),
            }
            return self.ignore_request(&path);
        }
        let value = match result {
            Ok(v) => v,
            Err(error) => {
                if op.starts_with("files.editor.") {
                    if let Some(editor) = &mut self.editor {
                        editor.loading = false;
                        editor.busy = false;
                        editor.error = Some(error);
                        editor.saving = None;
                    }
                } else {
                    self.loading = false;
                    self.mutating = false;
                    self.error = Some(error);
                }
                return vec![];
            }
        };
        match op {
            "files.cwd" => {
                self.loading = false;
                let Some(cwd) = value["cwd"].as_str().filter(|s| !s.is_empty()) else {
                    self.error = Some("Choose a directory to browse files".into());
                    return vec![];
                };
                self.root = cwd.into();
                self.path.replace_text(cwd);
                return self.folder(cwd.into());
            }
            "files.editor.media" => {
                self.image_job = None;
                if let Some(editor) = &mut self.editor {
                    editor.loading = false;
                    editor.binary = true;
                }
                let data = value
                    .as_str()
                    .or(value["dataUrl"].as_str())
                    .or(value["data_url"].as_str())
                    .ok_or("The agent returned no image preview".to_owned())
                    .and_then(crate::chat::media::decode_data_url);
                match data {
                    Ok((mime, bytes)) => {
                        if let Some(editor) = &mut self.editor {
                            editor.size = bytes.len() as u64;
                            match self
                                .safe_images
                                .as_mut()
                                .ok_or("The image decoder is unavailable".into())
                                .and_then(|images| {
                                    images.request(&mime, std::sync::Arc::from(bytes), 1400)
                                }) {
                                Ok(id) => {
                                    editor.loading = true;
                                    self.image_job =
                                        Some((id, editor.owner.clone(), editor.path.clone()));
                                }
                                Err(error) => editor.error = Some(error),
                            }
                        }
                    }
                    Err(error) => {
                        if let Some(editor) = &mut self.editor {
                            editor.error = Some(error);
                        }
                    }
                }
            }
            "files.editor.read" => {
                if let Some(editor) = &mut self.editor {
                    editor.loading = false;
                    editor.busy = false;
                    editor.error = None;
                    editor.binary = value["binary"].as_bool().unwrap_or(false);
                    editor.truncated = value["truncated"].as_bool().unwrap_or(false);
                    editor.size = value["byteSize"].as_u64().unwrap_or(0);
                    if let Some(text) = value["text"].as_str() {
                        editor.original = text.into();
                        editor.text.replace_text(text);
                    } else {
                        editor.error = Some("The agent returned no text preview".into());
                    }
                }
            }
            "files.editor.check" => {
                let Some(editor) = &mut self.editor else {
                    return vec![];
                };
                if value["text"].as_str() != Some(&editor.original)
                    || value["binary"] == true
                    || value["truncated"] == true
                {
                    editor.busy = false;
                    editor.saving = None;
                    editor.error = Some(
                        "The file changed on disk. Reload it or save your edits elsewhere".into(),
                    );
                    return vec![];
                }
                let Some(content) = editor.saving.clone() else {
                    return vec![];
                };
                let mut body = json!({"path":editor.path,"content":content});
                if let Some(id) = &editor.owner.session {
                    body["session_id"] = json!(id);
                }
                let request = if self.local {
                    body["expected"] = json!(editor.original);
                    PageRequest::local_fs("/api/fs/write-text", body)
                } else {
                    PageRequest::post("/api/fs/write-text", body)
                }
                .profile(&editor.owner.profile);
                return vec![("files.editor.write".into(), request)];
            }
            "files.editor.write" => {
                if let Some(editor) = &mut self.editor {
                    editor.busy = false;
                    if let Some(saved) = editor.saving.take() {
                        editor.original = saved;
                    }
                    editor.error = None;
                    self.notice = Some("File saved".into());
                }
                return self.load();
            }
            "files.rename" | "files.trash" => {
                self.mutating = false;
                let affected = self
                    .rename
                    .as_ref()
                    .or(self.trash.as_ref())
                    .map(|e| e.path.clone());
                if let Some(editor) = &mut self.editor {
                    if affected.as_deref().is_some_and(|p| {
                        editor.path == p || editor.path.starts_with(&format!("{p}/"))
                    }) {
                        if op == "files.trash" {
                            self.editor = None;
                        } else if let (Some(from), Some(to)) = (affected, value["path"].as_str()) {
                            editor.path = format!("{to}{}", &editor.path[from.len()..]);
                        }
                    }
                }
                self.rename = None;
                self.trash = None;
                self.notice = Some(
                    if op == "files.trash" {
                        "Moved to Trash"
                    } else {
                        "Renamed"
                    }
                    .into(),
                );
                return self.load();
            }
            "files.download" => self.notice = Some("File downloaded".into()),
            _ => {}
        }
        vec![]
    }
    pub fn visible(&self, path: &str) -> Vec<&Entry> {
        let query = self.search.text.to_lowercase();
        let ignored = self.remote_ignore(path);
        self.folders
            .get(path)
            .into_iter()
            .flat_map(|folder| folder.entries.iter())
            .filter(|entry| {
                (self.show_ignored
                    || !(entry.ignored
                        || ignored.as_ref().is_some_and(|rules| {
                            rules
                                .matched_path_or_any_parents(
                                    entry.path.replace('\\', "/"),
                                    entry.directory,
                                )
                                .is_ignore()
                        })))
                    && (query.is_empty()
                        || entry.name.to_lowercase().contains(&query)
                        || entry.directory)
            })
            .collect()
    }
    pub fn tick(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        if now < self.next_refresh || self.loading || self.mutating || self.root.is_empty() {
            return vec![];
        }
        self.next_refresh = now + 4.0;
        let paths: Vec<_> = self
            .expanded
            .iter()
            .filter(|p| !self.folders.get(*p).is_some_and(|f| f.loading))
            .cloned()
            .collect();
        paths.into_iter().flat_map(|p| self.folder(p)).collect()
    }
    pub fn fixture(&mut self, scope: DraftScope) {
        self.owner = Some(scope);
        self.local = false;
        self.root = "/sample/project".into();
        self.path.replace_text(&self.root);
        self.expanded.insert(self.root.clone());
        self.apply("files.tree.%2Fsample%2Fproject",Ok(json!({"entries":[{"name":"notes","path":"/sample/project/notes","isDirectory":true},{"name":"README.md","path":"/sample/project/README.md","isDirectory":false}]})));
        self.action(Action::Open("/sample/project/README.md".into()));
        self.apply("files.editor.read",Ok(json!({"path":"/sample/project/README.md","text":"# Project notes\n\nKeep the conversation at the center of the workspace.\n","byteSize":82})));
    }
}
fn decode(value: &str) -> String {
    url::form_urlencoded::parse(format!("path={}", value.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> DraftScope {
        DraftScope {
            connection: 1,
            profile: "work".into(),
            session: Some("stored".into()),
        }
    }
    #[test]
    fn asynchronous_images_keep_exact_editor_owner_path_and_cancel_retired_scope() {
        use base64::Engine;
        let mut files = Files::new();
        files.init(std::sync::Arc::new(|| {}));
        files.bind(scope(), false, Some("/work".into()));
        let first=br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="200" height="100" fill="#a52323"/></svg>"##;
        let second=br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="#239a82"/></svg>"##;
        files.action(Action::Open("/work/old.png".into()));
        files.apply("files.editor.media",Ok(json!({"data_url":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(first))})));
        files.action(Action::Open("/work/new.png".into()));
        files.apply("files.editor.media",Ok(json!({"data_url":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(second))})));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while files.preview.is_none() && std::time::Instant::now() < deadline {
            files.poll_images();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            files.preview.as_ref().unwrap().wait().unwrap(),
            [1400, 1400]
        );
        assert_eq!(files.editor.as_ref().unwrap().path, "/work/new.png");
        assert_eq!(files.editor.as_ref().unwrap().owner, scope());
        files.action(Action::Open("/work/retired.png".into()));
        files.apply("files.editor.media",Ok(json!({"data_url":format!("data:image/svg+xml;base64,{}",base64::engine::general_purpose::STANDARD.encode(first))})));
        files.reset();
        for _ in 0..100 {
            files.poll_images();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(files.preview.is_none());
        assert!(files.editor.is_none());
        assert!(files.image_job.is_none());
    }
    #[test]
    fn remote_save_keeps_read_owner_and_refuses_stale_disk_without_erasing_edits() {
        let mut state = Files::new();
        state.bind(scope(), false, Some("/work".into()));
        state.action(Action::Open("/work/a.txt".into()));
        state.apply("files.editor.read", Ok(json!({"text":"old"})));
        state.editor.as_mut().unwrap().text.replace_text("edited");
        let request = state.action(Action::Save);
        assert_eq!(request[0].1.profile.as_deref(), Some("work"));
        assert!(
            request[0]
                .1
                .query
                .iter()
                .any(|(k, v)| k == "session_id" && v == "stored")
        );
        assert!(
            state
                .apply("files.editor.check", Ok(json!({"text":"external"})))
                .is_empty()
        );
        assert_eq!(state.editor.as_ref().unwrap().text.text, "edited");
        assert!(state.editor.as_ref().unwrap().dirty());
    }
    #[test]
    fn save_write_and_read_errors_do_not_cross_local_remote_or_truncated_boundaries() {
        let mut state = Files::new();
        state.bind(scope(), false, Some("/work".into()));
        state.action(Action::Open("/work/a.txt".into()));
        state.apply(
            "files.editor.read",
            Ok(json!({"text":"old","truncated":true})),
        );
        state.editor.as_mut().unwrap().text.replace_text("edited");
        assert!(state.action(Action::Save).is_empty());
        state.editor.as_mut().unwrap().truncated = false;
        state.action(Action::Save);
        let effects = state.apply("files.editor.check", Ok(json!({"text":"old"})));
        assert_eq!(effects[0].1.body["session_id"], "stored");
        assert_eq!(effects[0].1.profile.as_deref(), Some("work"));
        assert!(matches!(
            effects[0].1.method,
            crate::pages::PageMethod::Post
        ));
        state.apply("files.cwd", Err("read".into()));
        assert!(state.editor.as_ref().unwrap().busy);
        assert!(
            state
                .action(Action::AskTrash(Entry {
                    name: "a".into(),
                    path: "/work/a".into(),
                    directory: false,
                    ignored: false,
                }))
                .is_empty()
        );
        assert!(state.trash.is_none());
    }
    #[test]
    fn save_ack_keeps_edits_made_during_write_and_dirty_reload_is_refused() {
        let mut state = Files::new();
        state.bind(scope(), true, Some("/work".into()));
        state.action(Action::Open("/work/a.txt".into()));
        state.apply("files.editor.read", Ok(json!({"text":"old"})));
        state.editor.as_mut().unwrap().text.replace_text("captured");
        assert!(state.action(Action::ReloadEditor).is_empty());
        assert_eq!(state.editor.as_ref().unwrap().text.text, "captured");
        state.action(Action::Save);
        let write = state.apply("files.editor.check", Ok(json!({"text":"old"})));
        assert_eq!(write[0].1.body["content"], "captured");
        state.editor.as_mut().unwrap().text.replace_text("newer");
        state.apply("files.editor.write", Ok(json!({"ok":true})));
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(editor.original, "captured");
        assert_eq!(editor.text.text, "newer");
        assert!(editor.dirty());
    }
}
