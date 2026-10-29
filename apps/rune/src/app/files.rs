use super::*;
use crate::{
    files::Action,
    pages::{PageRequest, Route},
};
impl Viewer {
    pub fn files_action(&mut self, action: Action) {
        if matches!(action, Action::Open(_))
            && !self
                .files
                .editor
                .as_ref()
                .is_some_and(|e| e.dirty() || e.busy)
        {
            self.invalidate_page_requests("files.editor.read");
            self.invalidate_page_requests("files.editor.media");
        }
        let open = matches!(action, Action::Open(_));
        let effects = self.files.action(action);
        if open && self.files.editor.is_some() {
            self.open_route(Route::FilePreview);
        }
        self.page_requests(effects);
    }
    pub fn poll_files(&mut self, now: f64) -> bool {
        if !(self.pane_shown("files") || self.route == Route::FilePreview) {
            return false;
        }
        let scope = self.composer_scope();
        if self.fixture {
            if self.files.owner.is_none() {
                self.files.fixture(scope);
                return true;
            }
            return false;
        }
        if self.connection.is_none() {
            return false;
        }
        let cwd = self
            .selected_session
            .as_ref()
            .and_then(|id| self.sidebar_state.find(&self.profile, id))
            .and_then(|row| row.cwd.clone());
        if self.files.owner.as_ref() != Some(&scope) {
            self.invalidate_page_requests("files.tree.");
            self.invalidate_page_requests("files.cwd");
            let effects = self.files.bind(scope, self.is_local_connection(), cwd);
            self.page_requests(effects);
            return true;
        }
        let effects = self.files.tick(now);
        let changed = !effects.is_empty();
        self.page_requests(effects);
        changed
    }
    pub fn save_file_copy(&mut self, owner: DraftScope, path: PathBuf, contents: String) {
        if self.fixture || owner.connection != self.connection_epoch {
            return;
        }
        if crate::native_fs::save_copy(&path, contents.as_bytes()).is_err() {
            self.files.error = Some("The edited file copy could not be saved".into());
        } else {
            self.files.notice = Some("Edited file copied to the selected destination".into());
        }
    }
    pub fn download_selected_file(
        &mut self,
        owner: DraftScope,
        source: String,
        destination: PathBuf,
    ) {
        if self.fixture
            || owner.connection != self.connection_epoch
            || owner.profile != self.profile
        {
            self.files.error = Some("The file owner changed while the dialog was open".into());
            return;
        }
        let mut request = PageRequest::get("/api/fs/download")
            .profile(&owner.profile)
            .query("path", source)
            .download_to(destination)
            .timeout_secs(3600);
        if let Some(id) = owner.session {
            request = request.query("session_id", id);
        }
        self.page_request("files.download", request);
    }
    pub fn quote_file(&mut self) {
        if let Some(editor) = &self.files.editor {
            let selected = editor.text.selected();
            let content = if selected.is_empty() {
                &editor.text.text
            } else {
                selected
            };
            let contents = format!("\nFile: {}\n```\n{}\n```\n", editor.path, content);
            let scope = self.composer_scope();
            self.composer.draft_mut(scope).text.insert(&contents);
            self.open_route(Route::Chat);
        }
    }
}
