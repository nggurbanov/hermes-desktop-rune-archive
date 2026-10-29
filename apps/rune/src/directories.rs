//! Directory browsing is routed to the owning agent, never a remote-to-local fallback.
use crate::{
    composer::DraftScope,
    pages::{PageEffect, PageRequest},
};
use rune::TextEdit;
use serde_json::{Value, json};
#[derive(Clone)]
pub enum Purpose {
    Workspace,
    Files,
}
pub struct Directories {
    pub owner: Option<DraftScope>,
    pub purpose: Purpose,
    pub path: TextEdit,
    pub entries: Vec<crate::files::Entry>,
    pub local: bool,
    pub loading: bool,
    pub error: Option<String>,
    detached: TextEdit,
}
impl Default for Directories {
    fn default() -> Self {
        Self {
            owner: None,
            purpose: Purpose::Files,
            path: TextEdit::new(""),
            entries: vec![],
            local: false,
            loading: false,
            error: None,
            detached: TextEdit::new(""),
        }
    }
}
impl Directories {
    pub fn open(
        &mut self,
        owner: DraftScope,
        local: bool,
        path: String,
        purpose: Purpose,
    ) -> Vec<PageEffect> {
        self.owner = Some(owner);
        self.local = local;
        self.path.replace_text(path);
        self.purpose = purpose;
        self.load()
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        let Some(owner) = &self.owner else {
            return vec![];
        };
        self.loading = true;
        self.error = None;
        let route = if self.path.text.trim().is_empty() {
            "/api/fs/default-cwd"
        } else {
            "/api/fs/list"
        };
        let mut request = if self.local && route != "/api/fs/default-cwd" {
            PageRequest::local_fs(route, json!({"path":self.path.text.trim()}))
        } else {
            let mut r = PageRequest::get(route);
            if route == "/api/fs/list" {
                r = r.query("path", self.path.text.trim());
            }
            r
        }
        .profile(&owner.profile);
        if let Some(id) = &owner.session {
            request = request.query("session_id", id);
        }
        vec![(
            if route == "/api/fs/list" {
                "directories.list"
            } else {
                "directories.cwd"
            }
            .into(),
            request,
        )]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        self.loading = false;
        match result {
            Ok(v) if op == "directories.cwd" => {
                if let Some(cwd) = v["cwd"].as_str() {
                    self.path.replace_text(cwd);
                    return self.load();
                }
                self.error = Some("Choose an absolute directory on this agent".into());
            }
            Ok(v) => {
                match serde_json::from_value::<Vec<crate::files::Entry>>(v["entries"].clone()) {
                    Ok(entries) => {
                        self.entries = entries.into_iter().filter(|e| e.directory).collect()
                    }
                    Err(_) => {
                        self.error = Some("The agent did not return a directory listing".into())
                    }
                }
            }
            Err(error) => {
                self.error = Some(error);
                self.entries.clear();
            }
        }
        vec![]
    }
    pub fn edit(&mut self, owner: &DraftScope) -> &mut TextEdit {
        if self.owner.as_ref() == Some(owner) {
            &mut self.path
        } else {
            &mut self.detached
        }
    }
}
impl crate::app::Viewer {
    pub fn open_directory(&mut self, purpose: Purpose) {
        let scope = self.composer_scope();
        let path = match purpose {
            Purpose::Workspace => self.workspace_cwd(),
            Purpose::Files => Some(self.files.root.clone()),
        }
        .unwrap_or_default();
        self.invalidate_page_requests("directories.");
        let effects = self
            .directories
            .open(scope, self.is_local_connection(), path, purpose);
        self.page_requests(effects);
    }
    pub fn directory_load(&mut self, path: String) {
        if self.directories.owner.as_ref() != Some(&self.composer_scope()) {
            return;
        }
        self.invalidate_page_requests("directories.");
        self.directories.path.replace_text(path);
        let effects = self.directories.load();
        self.page_requests(effects);
    }
    pub fn choose_directory(&mut self) {
        let Some(owner) = self.directories.owner.clone() else {
            return;
        };
        if owner != self.composer_scope() || self.directories.loading {
            return;
        }
        let path = self.directories.path.text.trim().to_owned();
        if path.is_empty() || self.directories.error.is_some() {
            return;
        }
        match self.directories.purpose {
            Purpose::Workspace => self.change_workspace(owner, path),
            Purpose::Files => self.files_action(crate::files::Action::Root(path)),
        }
        self.directories.owner = None;
    }
}
