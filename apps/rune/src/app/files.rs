use super::*;
use crate::{
    files::Action,
    pages::{PageRequest, Route},
};
impl Viewer {
    pub(super) fn poll_preview(&mut self) -> bool {
        if let Some(id) = self.audio_preview {
            let visible = self.route == Route::FilePreview
                && self.pane_shown("workspace")
                && self.native_window.as_ref().is_none_or(|w| w.is_visible());
            self.media_players.observe(id, visible);
        }
        let mut changed = self.preview.poll() | self.media_players.poll();
        if self
            .audio_preview
            .is_some_and(|id| self.media_players.get(id).is_none())
        {
            self.audio_preview = None;
            changed = true;
        }
        if self.preview.active().is_none()
            && self.audio_preview.is_none()
            && self.route == Route::FilePreview
        {
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
                if let Some(kind) = crate::preview::Kind::for_path(path)
                    .filter(|kind| kind.available() || *kind == crate::preview::Kind::Audio)
                {
                    if let (Some(owner), Some(connection)) =
                        (self.files.owner.clone(), self.connection.clone())
                    {
                        if !self.fixture && owner == self.composer_scope() {
                            self.open_owned_preview(
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
                self.close_audio_preview();
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
        let kind = if artifact.kind == "audio" {
            Some(crate::preview::Kind::Audio)
        } else if artifact.kind == "image" {
            Some(crate::preview::Kind::Image)
        } else {
            crate::preview::Kind::for_source(&artifact.value)
        };
        let Some(kind) =
            kind.filter(|kind| kind.available() || *kind == crate::preview::Kind::Audio)
        else {
            return;
        };
        let source = if artifact.value.starts_with("data:") {
            if kind == crate::preview::Kind::Image
                && let Some((_, bytes)) = &self.artifacts.download
            {
                crate::preview::Source::Inline {
                    mime: self.artifacts.mime.clone(),
                    bytes: bytes.clone().into(),
                }
            } else {
                match crate::chat::media::decode_data_url(&artifact.value) {
                    Ok((mime, bytes)) => crate::preview::Source::Inline {
                        mime,
                        bytes: bytes.into(),
                    },
                    Err(error) => {
                        self.artifacts.preview_error = Some(error);
                        return;
                    }
                }
            }
        } else if artifact.value.starts_with("https://") || artifact.value.starts_with("http://") {
            crate::preview::Source::RemoteUrl(artifact.value)
        } else {
            crate::preview::Source::AgentPath(artifact.value)
        };
        self.open_owned_preview(
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

impl Viewer {
    pub fn close_audio_preview(&mut self) {
        if let Some(id) = self.audio_preview.take() {
            self.media_players.close(id);
        }
    }
    pub fn open_owned_preview(&mut self, connection: Connection, target: crate::preview::Target) {
        self.close_audio_preview();
        if target.kind == crate::preview::Kind::Audio {
            self.preview.close();
            match self.media_players.open(connection, target) {
                Ok(id) => self.audio_preview = Some(id),
                Err(error) => self.client_error = Some(error),
            }
        } else {
            self.preview.open(connection, target);
        }
    }
}

#[cfg(test)]
mod audio_preview_tests {
    use super::*;
    fn audio_target() -> crate::preview::Target {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut wav = hound::WavWriter::new(
                &mut bytes,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 48000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..480 {
                wav.write_sample(0i16).unwrap();
            }
            wav.finalize().unwrap();
        }
        crate::preview::Target {
            owner: DraftScope {
                connection: 9,
                profile: "secondary".into(),
                session: Some("owned-history".into()),
            },
            source: crate::preview::Source::Inline {
                mime: "audio/wav".into(),
                bytes: bytes.into_inner().into(),
            },
            label: "Retained source.wav".into(),
            kind: crate::preview::Kind::Audio,
        }
    }
    #[test]
    fn audio_target_retains_its_owning_session_and_closing_restores_artifact_selection() {
        let mut s = Viewer::empty();
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.route = Route::FilePreview;
        s.preview_return = Some(Route::Artifacts);
        s.artifacts.selected = Some(7);
        s.open_owned_preview(
            Connection::Url {
                endpoint: "http://127.0.0.1:1".into(),
                token: None,
            },
            audio_target(),
        );
        let id = s.audio_preview.unwrap();
        let p = s.media_players.get(id).unwrap();
        assert_eq!(p.target.owner.profile, "secondary");
        assert_eq!(p.target.owner.session.as_deref(), Some("owned-history"));
        assert!(s.preview.active().is_none());
        s.poll_preview();
        assert!(s.route == Route::FilePreview);
        s.close_audio_preview();
        assert!(s.media_players.get(id).is_none());
        s.poll_preview();
        assert!(s.route == Route::Artifacts);
        assert_eq!(s.artifacts.selected, Some(7));
        assert!(!s.artifacts.loading);
    }
    #[test]
    fn an_inline_audio_artifact_opens_without_an_image_download_or_foreground_profile_substitution()
    {
        use base64::Engine;
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        });
        s.media_players.init(std::sync::Arc::new(|| {}));
        let target = audio_target();
        let crate::preview::Source::Inline { bytes, .. } = target.source else {
            unreachable!()
        };
        s.preview_artifact(crate::pages::artifacts::Artifact {
            kind: "file".into(),
            value: format!(
                "data:audio/wav;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            ),
            label: "Inline recording.wav".into(),
            profile: "secondary".into(),
            session: "captured-session".into(),
            title: "Source conversation".into(),
            timestamp: 0,
        });
        let id = s.audio_preview.unwrap();
        let p = s.media_players.get(id).unwrap();
        assert_eq!(p.target.owner.profile, "secondary");
        assert_eq!(p.target.owner.session.as_deref(), Some("captured-session"));
        assert!(s.route == Route::FilePreview);
        assert!(s.artifacts.download.is_none());
        let crate::preview::Source::Inline { bytes: actual, .. } = &p.target.source else {
            unreachable!()
        };
        assert_eq!(&**actual, &*bytes);
        s.close_audio_preview();
    }
    #[test]
    fn replacing_an_audio_preview_retires_the_old_player_before_opening_another() {
        let mut s = Viewer::empty();
        s.media_players.init(std::sync::Arc::new(|| {}));
        let connection = Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        };
        s.open_owned_preview(connection.clone(), audio_target());
        let old = s.audio_preview.unwrap();
        s.open_owned_preview(connection, audio_target());
        let current = s.audio_preview.unwrap();
        assert_ne!(old, current);
        assert!(s.media_players.get(old).is_none());
        assert!(s.media_players.get(current).is_some());
        s.media_players.act(old, crate::media_player::Action::Play);
        assert!(s.media_players.get(old).is_none());
        s.close_audio_preview();
    }
}
