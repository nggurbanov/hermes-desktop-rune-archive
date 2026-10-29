use super::*;
use crate::{
    files::Action,
    pages::{PageRequest, Route},
};
impl Viewer {
    pub(super) fn poll_preview(&mut self) -> bool {
        let mut changed = self.preview.poll();
        if self.preview.active().is_none() && self.route == Route::FilePreview {
            if let Some(route) = self.preview_return.take() {
                self.route = route;
                self.reveal_pane("workspace");
                changed = true;
            }
        }
        changed
    }
    pub fn files_action(&mut self, action: Action) {
        if let Action::Open(path) = &action {
            let busy = self
                .files
                .editor
                .as_ref()
                .is_some_and(|e| e.dirty() || e.busy);
            if !busy {
                if let Some(kind) =
                    crate::preview::Kind::for_path(path).filter(|kind| kind.available())
                {
                    if let (Some(owner), Some(connection)) =
                        (self.files.owner.clone(), self.connection.clone())
                    {
                        if !self.fixture && owner == self.composer_scope() {
                            self.preview.open(
                                connection,
                                crate::preview::Target {
                                    owner,
                                    source: crate::preview::Source::AgentPath(path.clone()),
                                    label: path.rsplit('/').next().unwrap_or(path).into(),
                                    kind,
                                },
                            );
                            if self.route != Route::FilePreview {
                                self.preview_return = Some(self.route);
                            }
                            self.open_route(Route::FilePreview);
                            return;
                        }
                    }
                }
                self.preview.close();
            }
        }
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

impl Viewer {
    pub fn preview_artifact(&mut self, artifact: crate::pages::artifacts::Artifact) {
        if self.fixture || self.artifacts.demo {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let kind = if artifact.kind == "image" {
            Some(crate::preview::Kind::Image)
        } else {
            crate::preview::Kind::for_path(&artifact.value)
        };
        let Some(kind) = kind.filter(|kind| kind.available()) else {
            return;
        };
        let source = if artifact.value.starts_with("data:") {
            let Some((_, bytes)) = &self.artifacts.download else {
                return;
            };
            crate::preview::Source::Inline {
                mime: self.artifacts.mime.clone(),
                bytes: bytes.clone().into(),
            }
        } else if artifact.value.starts_with("https://") || artifact.value.starts_with("http://") {
            crate::preview::Source::RemoteUrl(artifact.value)
        } else {
            crate::preview::Source::AgentPath(artifact.value)
        };
        self.preview.open(
            connection,
            crate::preview::Target {
                owner: DraftScope {
                    connection: self.connection_epoch,
                    profile: artifact.profile,
                    session: Some(artifact.session),
                },
                source,
                label: artifact.label,
                kind,
            },
        );
        if self.route != Route::FilePreview {
            self.preview_return = Some(self.route);
        }
        self.open_route(Route::FilePreview);
    }
}

#[cfg(test)]
mod preview_return_tests {
    use super::*;
    #[test]
    fn closing_an_image_returns_to_the_existing_artifact_selection_without_reindexing() {
        let mut s = Viewer::empty();
        s.fixture = true;
        s.artifacts.rows.push(crate::pages::artifacts::Artifact {
            kind: "image".into(),
            value: "/agent/source.png".into(),
            label: "Keep this selected image".into(),
            session: "stored".into(),
            profile: "default".into(),
            title: "Original conversation".into(),
            timestamp: 0,
        });
        s.artifacts.selected = Some(0);
        s.preview_return = Some(Route::Artifacts);
        s.route = Route::FilePreview;
        assert!(s.poll_preview());
        assert!(s.route == Route::Artifacts);
        assert_eq!(s.artifacts.selected, Some(0));
        assert_eq!(s.artifacts.rows[0].label, "Keep this selected image");
        assert!(!s.artifacts.loading);
        assert!(!s.poll_preview());
    }
}
