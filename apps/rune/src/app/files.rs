use super::*;
use crate::{
    files::Action,
    pages::{PageRequest, Route},
};
impl Viewer {
    pub(crate) fn inline_audio_mount(
        &mut self,
        identity: (DraftScope, String, String, InlineAudioOccurrence, String),
        label: String,
        mounted: bool,
    ) -> bool {
        if mounted {
            if self.composer_scope() != identity.0
                || self.composer.editor_id().as_str() != identity.1
                || self.inline_audio.contains_key(&identity)
            {
                return false;
            }
            let Some(connection) = self.connection.clone() else {
                return false;
            };
            let target = crate::preview::Target {
                owner: identity.0.clone(),
                source: if matches!(&identity.3, InlineAudioOccurrence::Media { .. })
                    && url::Url::parse(&identity.4)
                        .is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
                {
                    crate::preview::Source::RemoteUrl(identity.4.clone())
                } else {
                    crate::preview::Source::AgentPath(identity.4.clone())
                },
                label,
                kind: crate::preview::Kind::Audio,
            };
            match self.media_players.open_inline(connection, target) {
                Ok(id) => {
                    self.inline_audio.insert(identity, id);
                    true
                }
                Err(error) => {
                    self.client_error = Some(error);
                    true
                }
            }
        } else if let Some(id) = self.inline_audio.remove(&identity) {
            self.media_players.close(id);
            true
        } else {
            false
        }
    }

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
                    draft_origin: None,
                    profile_intent: crate::source::ProfileIntent::Backend,
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
                draft_origin: None,
                profile_intent: crate::source::ProfileIntent::Backend,
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
    #[test]
    fn transcript_audio_click_uses_owned_loopback_and_independent_players() {
        use std::time::{Duration, Instant};
        let peer =
            crate::media_player::tests::virtual_peer::peer(crate::media_player::tests::wav(1, 1.));
        let mut s = Viewer::empty();
        s.connection = Some(peer.connection.clone());
        s.profile = "sample".into();
        s.selected_session = Some("owned".into());
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.media_players.use_null_output();
        s.messages = vec![crate::gateway::HistoryMessage::from_row(
            &serde_json::json!({
                "row_id": 7, "role": "assistant", "content": [
                    {"type": "file", "path": "/owned/a.wav", "name": "a.wav"},
                    {"type": "file", "path": "/owned/b.wav", "name": "b.wav"}
                ]
            }),
        )];
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        let start = Instant::now();
        loop {
            rt.state.media_players.poll();
            rt.invalidate();
            rt.frame(0.);
            if rt.state.inline_audio.len() == 2
                && rt.state.inline_audio.values().all(|id| {
                    rt.state
                        .media_players
                        .get(*id)
                        .is_some_and(|p| p.state == crate::media_player::State::Paused)
                })
            {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "players did not load"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let a = *rt
            .state
            .inline_audio
            .iter()
            .find(|(key, _)| key.4.ends_with("a.wav"))
            .unwrap()
            .1;
        let b = *rt
            .state
            .inline_audio
            .iter()
            .find(|(key, _)| key.4.ends_with("b.wav"))
            .unwrap()
            .1;
        rt.wheel(400., 300., 10_000.);
        rt.settle_all();
        for _ in 0..5 {
            rt.frame(1. / 60.);
        }
        let button = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("audio-play-pause") && !e.exiting)
            .unwrap();
        let (x, y) = (
            button.rect[0] + button.rect[2] / 2.,
            button.rect[1] + button.rect[3] / 2.,
        );
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.);
        rt.state.media_players.poll();
        assert!(
            !rt.state.media_players.get(a).unwrap().paused,
            "a paused {}, b paused {}",
            rt.state.media_players.get(a).unwrap().paused,
            rt.state.media_players.get(b).unwrap().paused
        );
        assert!(rt.state.media_players.get(b).unwrap().paused);
        assert!(
            peer.requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.contains("a.wav"))
        );
        rt.state.messages.clear();
        rt.invalidate();
        rt.frame(0.);
        assert!(rt.state.media_players.get(a).is_none() && rt.state.media_players.get(b).is_none());
        std::thread::sleep(Duration::from_millis(100));
        rt.state.media_players.poll();
        assert!(
            rt.state.media_players.get(a).is_none() && rt.state.media_players.get(b).is_none(),
            "late worker updates must not resurrect retired players"
        );
    }

    #[test]
    fn inline_audio_survives_overscan_and_retires_when_culled() {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        });
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.messages = (0..100).map(|i| crate::gateway::HistoryMessage::from_row(&serde_json::json!({
            "row_id": i + 1, "role": "assistant", "content": if i == 0 {
                serde_json::json!([{ "type": "file", "path": "/overscan.wav", "name": "overscan" }])
            } else { serde_json::json!(format!("Filler message {i}")) }
        }))).collect();
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        for _ in 0..8 {
            rt.frame(0.);
        }
        rt.wheel(400., 300., 100_000.);
        for _ in 0..8 {
            rt.frame(0.);
        }
        let id = *rt
            .state
            .inline_audio
            .values()
            .next()
            .expect("top audio mounted");
        rt.wheel(400., 300., -650.);
        for _ in 0..8 {
            rt.frame(0.);
        }
        let audio = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("audio-play-pause") && !e.exiting)
            .expect("overscan audio node remains mounted");
        assert!(
            audio.rect[1] + audio.rect[3] < 0.,
            "audio control is offscreen: {:?}",
            audio.rect
        );
        assert_eq!(rt.state.inline_audio.values().copied().next(), Some(id));
        rt.wheel(400., 300., -100_000.);
        for _ in 0..8 {
            rt.frame(0.);
        }
        assert!(
            rt.state.media_players.get(id).is_none(),
            "culled audio retired"
        );
        rt.wheel(400., 300., 100_000.);
        for _ in 0..8 {
            rt.frame(0.);
        }
        let new_id = *rt
            .state
            .inline_audio
            .values()
            .next()
            .expect("audio remounted");
        assert_ne!(id, new_id);
    }

    #[test]
    fn external_media_uses_credential_free_ranged_transport() {
        use std::{
            io::{BufRead, BufReader, Write},
            net::TcpListener,
            sync::{
                Arc, Mutex,
                atomic::{AtomicBool, Ordering},
            },
            time::{Duration, Instant},
        };
        let audio = Arc::new(crate::media_player::tests::wav(1, 1.));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let server = std::thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    headers.push_str(&line);
                }
                recorded
                    .lock()
                    .unwrap()
                    .push((request.clone(), headers.clone()));
                let len = audio.len();
                if request.starts_with("HEAD ") {
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nETag: \"tone\"\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    let range = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("range: bytes=")
                                .map(str::to_owned)
                        })
                        .unwrap();
                    let (a, b) = range.split_once('-').unwrap();
                    let a: usize = a.parse().unwrap();
                    let b: usize = b.trim().parse().unwrap();
                    write!(stream,"HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{len}\r\nETag: \"tone\"\r\nConnection: close\r\n\r\n",b-a+1).unwrap();
                    stream.write_all(&audio[a..=b]).unwrap();
                }
            }
        });
        let peer =
            crate::media_player::tests::virtual_peer::peer(crate::media_player::tests::wav(1, 1.));
        let mut s = Viewer::empty();
        s.connection = Some(peer.connection.clone());
        s.profile = "sample".into();
        s.selected_session = Some("owned".into());
        s.media_players.init(Arc::new(|| {}));
        s.media_players.use_null_output();
        s.messages = vec![crate::gateway::HistoryMessage::from_row(
            &serde_json::json!({"row_id":7,"role":"assistant","content":format!("MEDIA:/owned/a.wav\nMEDIA:http://{address}/public.wav?sig=abc%2Fdef")}),
        )];
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        let start = Instant::now();
        loop {
            rt.state.media_players.poll();
            rt.invalidate();
            rt.frame(0.);
            if rt.state.inline_audio.len() == 2
                && rt.state.inline_audio.values().all(|id| {
                    rt.state
                        .media_players
                        .get(*id)
                        .is_some_and(|p| p.state == crate::media_player::State::Paused)
                })
            {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        let saved = requests.lock().unwrap().clone();
        assert!(
            saved
                .iter()
                .any(|(r, _)| r.starts_with("GET /public.wav?sig=abc%2Fdef "))
        );
        assert!(saved.iter().all(|(r, h)| {
            (r.starts_with("HEAD /public.wav?sig=abc%2Fdef ")
                || r.starts_with("GET /public.wav?sig=abc%2Fdef "))
                && !h.to_ascii_lowercase().lines().any(|line| {
                    line.starts_with("authorization:")
                        || line.starts_with("cookie:")
                        || line.starts_with("x-hermes-gateway-ticket:")
                })
        }));
        assert!(
            peer.requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.contains("a.wav") && r.contains("sample") && r.contains("owned"))
        );
        stop.store(true, Ordering::Release);
        server.join().unwrap();
    }

    #[test]
    fn queued_quoted_media_closure_mounts_and_retains_the_player() {
        use std::time::{Duration, Instant};
        let peer =
            crate::media_player::tests::virtual_peer::peer(crate::media_player::tests::wav(1, 1.));
        let mut s = Viewer::empty();
        s.connection = Some(peer.connection.clone());
        s.profile = "sample".into();
        s.selected_session = Some("owned".into());
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.media_players.use_null_output();
        let owner = s.composer_scope();
        s.conversation
            .fixture(
                owner.clone(),
                &serde_json::json!({"execution_generation":2}),
            )
            .unwrap();
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        let mut offset = 0;
        let mut mounted = None;
        for (seq, text) in [(1, "MEDIA:\"/owned/a.wav"), (2, "\""), (3, "\nprose")] {
            let event = crate::gateway::conversation::Event {
                session: "owned".into(),
                kind: "message.delta".into(),
                payload: serde_json::json!({"text":text,"turn_id":"t","segment":0,"offset":offset}),
                epoch: None,
                sequence: Some(seq),
                generation: Some(2),
                authority: None,
                admission: None,
            };
            offset += text.encode_utf16().count();
            rt.state
                .conversation
                .fixture_queue_event(owner.clone(), event);
            assert!(rt.state.poll_conversations());
            rt.invalidate();
            rt.frame(0.);
            if seq == 1 {
                assert!(rt.state.inline_audio.is_empty());
            }
            if seq == 2 {
                let start = Instant::now();
                while rt.state.inline_audio.is_empty() {
                    rt.state.media_players.poll();
                    rt.invalidate();
                    rt.frame(0.);
                    assert!(start.elapsed() < Duration::from_secs(5));
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            if seq == 2 {
                assert_eq!(rt.state.inline_audio.len(), 1);
                mounted = Some(rt.state.inline_audio.clone());
            }
            if seq == 3 {
                assert_eq!(rt.state.inline_audio, mounted.take().unwrap());
                assert!(
                    rt.elements()
                        .iter()
                        .any(|e| e.key.as_deref() == Some("audio-play-pause") && !e.exiting)
                );
            }
        }
        assert_eq!(
            rt.state
                .conversation
                .view(&owner)
                .unwrap()
                .messages
                .last()
                .unwrap()
                .content,
            "MEDIA:\"/owned/a.wav\"\nprose"
        );
    }

    #[test]
    fn media_lines_mount_independent_players_and_retire_on_removal() {
        use std::time::{Duration, Instant};
        let peer =
            crate::media_player::tests::virtual_peer::peer(crate::media_player::tests::wav(1, 1.));
        let mut s = Viewer::empty();
        s.connection = Some(peer.connection.clone());
        s.profile = "sample".into();
        s.selected_session = Some("owned".into());
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.media_players.use_null_output();
        let make = |body: &str| {
            crate::gateway::HistoryMessage::from_row(&serde_json::json!({
                "row_id": 7, "role": "assistant", "content": [
                    {"type":"text", "text":body}, {"type":"file", "path":"/owned/a.wav", "name":"a.wav"}
                ]
            }))
        };
        let body = "Before\nMEDIA:/owned/a.wav\nMEDIA:/owned/a.wav";
        s.messages = vec![make(body)];
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        let start = Instant::now();
        loop {
            rt.state.media_players.poll();
            rt.invalidate();
            rt.frame(0.);
            if rt.state.inline_audio.len() == 3
                && rt.state.inline_audio.values().all(|id| {
                    rt.state
                        .media_players
                        .get(*id)
                        .is_some_and(|p| p.state == crate::media_player::State::Paused)
                })
            {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "players: {}",
                rt.state.inline_audio.len()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let old = rt.state.inline_audio.clone();
        assert_eq!(
            old.keys()
                .filter(|key| matches!(key.3, InlineAudioOccurrence::Media { .. }))
                .count(),
            2
        );
        rt.update(|s, tx| tx.scroll_to_y(s.chat_history_key(), 0.));
        rt.settle_all();
        for _ in 0..5 {
            rt.frame(1. / 60.);
        }
        let controls: Vec<_> = rt
            .elements()
            .into_iter()
            .filter(|e| e.key.as_deref() == Some("audio-play-pause") && !e.exiting)
            .collect();
        assert_eq!(controls.len(), 3);
        let rect = controls[0].rect;
        rt.pointer_down(rect[0] + rect[2] / 2., rect[1] + rect[3] / 2.);
        rt.pointer_up(rect[0] + rect[2] / 2., rect[1] + rect[3] / 2.);
        rt.frame(0.);
        rt.state.media_players.poll();
        assert_eq!(
            old.values()
                .filter(|id| !rt.state.media_players.get(**id).unwrap().paused)
                .count(),
            1
        );
        rt.state.messages = vec![make(&format!("{body}\nextra"))];
        rt.invalidate();
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio, old);
        rt.state.messages = vec![make("Before\nMEDIA:/owned/a.wav")];
        rt.invalidate();
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio.len(), 2);
        assert_eq!(
            old.values()
                .filter(|id| rt.state.media_players.get(**id).is_none())
                .count(),
            1
        );
        rt.state.messages = vec![make(body)];
        rt.invalidate();
        rt.frame(0.);
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio.len(), 3);
        assert_eq!(
            rt.state
                .inline_audio
                .values()
                .filter(|id| !old.values().any(|previous| previous == *id))
                .count(),
            1
        );
        assert!(
            peer.requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.contains("a.wav") && r.contains("sample") && r.contains("owned"))
        );
    }

    #[test]
    fn markdown_audio_links_mount_independent_controls() {
        use std::time::{Duration, Instant};
        let peer =
            crate::media_player::tests::virtual_peer::peer(crate::media_player::tests::wav(1, 1.));
        let mut s = Viewer::empty();
        s.connection = Some(peer.connection.clone());
        s.profile = "sample".into();
        s.selected_session = Some("owned".into());
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.media_players.use_null_output();
        let body = "Привет **styled** [listen](/owned/a.wav) then [listen](file:///owned/a.wav)";
        let make_message = |text: &str| {
            crate::gateway::HistoryMessage::from_row(&serde_json::json!({
                "row_id": 7, "role": "assistant", "content": [
                    {"type":"text", "text": text}, {"type":"file", "path":"/owned/a.wav", "name":"a.wav"}
                ]
            }))
        };
        s.messages = vec![make_message(body)];
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        let start = Instant::now();
        loop {
            rt.state.media_players.poll();
            rt.invalidate();
            rt.frame(0.);
            if rt.state.inline_audio.len() == 3
                && rt.state.inline_audio.values().all(|id| {
                    rt.state
                        .media_players
                        .get(*id)
                        .is_some_and(|p| p.state == crate::media_player::State::Paused)
                })
            {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "three players did not load: {}",
                rt.state.inline_audio.len()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        rt.update(|s, tx| tx.scroll_to_y(s.chat_history_key(), 0.));
        rt.settle_all();
        for _ in 0..5 {
            rt.frame(1. / 60.);
        }
        let controls: Vec<_> = rt
            .elements()
            .into_iter()
            .filter(|e| e.key.as_deref() == Some("audio-play-pause") && !e.exiting)
            .collect();
        assert_eq!(controls.len(), 3);
        assert!(
            controls
                .windows(2)
                .all(|pair| pair[0].rect[1] < pair[1].rect[1])
        );
        let ids = rt.state.inline_audio.clone();
        assert!(ids.keys().all(|key| key.0.profile == "sample"
            && key.0.session.as_deref() == Some("owned")
            && key.4 == "/owned/a.wav"));
        assert!(
            controls[0].visible[2] > 0. && controls[0].visible[3] > 0.,
            "first control clipped: {:?}",
            controls[0]
        );
        let first = controls[0].rect;
        rt.pointer_down(first[0] + first[2] / 2., first[1] + first[3] / 2.);
        rt.pointer_up(first[0] + first[2] / 2., first[1] + first[3] / 2.);
        rt.frame(0.);
        rt.state.media_players.poll();
        assert_eq!(
            ids.values()
                .filter(|id| !rt.state.media_players.get(**id).unwrap().paused)
                .count(),
            1,
            "controls: {:?}",
            controls
        );
        assert!(
            peer.requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.contains("a.wav") && r.contains("sample") && r.contains("owned"))
        );
        let without_second = body.replace("[listen](file:///owned/a.wav)", "removed");
        rt.state.messages = vec![make_message(&without_second)];
        rt.invalidate();
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio.len(), 2);
        assert_eq!(
            ids.values()
                .filter(|id| rt.state.media_players.get(**id).is_none())
                .count(),
            1
        );
        rt.state.messages = vec![make_message(body)];
        rt.invalidate();
        rt.frame(0.);
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio.len(), 3);
        assert_eq!(
            rt.state
                .inline_audio
                .values()
                .filter(|id| !ids.values().any(|old| old == *id))
                .count(),
            1
        );
        let ids = rt.state.inline_audio.clone();
        rt.state.messages = vec![make_message(&format!("{body} appended"))];
        rt.invalidate();
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio, ids);
        rt.state.messages.insert(
            0,
            crate::gateway::HistoryMessage::from_row(
                &serde_json::json!({"row_id":6,"role":"assistant","content":"earlier"}),
            ),
        );
        rt.invalidate();
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio, ids);
        rt.state.messages.clear();
        rt.invalidate();
        rt.frame(0.);
        assert!(rt.state.inline_audio.is_empty());
        rt.state.messages = vec![make_message(body)];
        rt.invalidate();
        rt.frame(0.);
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio.len(), 3);
        assert!(
            rt.state
                .inline_audio
                .values()
                .all(|id| !ids.values().any(|old| old == id))
        );
    }

    #[test]
    fn empty_markdown_audio_labels_mount_distinct_players() {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        });
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.messages = vec![crate::gateway::HistoryMessage::from_row(
            &serde_json::json!({
                "row_id":7,"role":"assistant","content":"[](/tmp/a.wav) [](/tmp/a.wav)"
            }),
        )];
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        rt.frame(0.);
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio.len(), 2);
    }

    #[test]
    fn markdown_external_and_code_links_do_not_mount_audio() {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        });
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.messages = vec![crate::gateway::HistoryMessage::from_row(
            &serde_json::json!({
                "row_id":7,"role":"assistant","content":"[external](https://example.test/a.mp3) ` [code](/tmp/a.wav) `"
            }),
        )];
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        rt.frame(0.);
        rt.frame(0.);
        assert!(rt.state.inline_audio.is_empty());
    }

    #[test]
    fn transcript_mounts_two_independent_audio_controls() {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        });
        s.media_players.init(std::sync::Arc::new(|| {}));
        s.messages = vec![crate::gateway::HistoryMessage::from_row(
            &serde_json::json!({
                "row_id": 7, "role": "assistant", "content": [
                    {"type": "file", "path": "/a.wav", "name": "A"},
                    {"type": "file", "path": "/b.wav", "name": "B"}
                ]
            }),
        )];
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        rt.frame(0.);
        rt.frame(0.);
        let controls = rt
            .elements()
            .into_iter()
            .filter(|e| e.key.as_deref() == Some("audio-play-pause") && !e.exiting)
            .count();
        assert_eq!(controls, 2, "both attachments need their own inline player");
        let ids: Vec<_> = rt.state.inline_audio.values().copied().collect();
        assert_eq!(ids.len(), 2);
        rt.invalidate();
        rt.frame(0.);
        assert!(
            ids.iter()
                .all(|id| rt.state.inline_audio.values().any(|current| current == id)),
            "rerender retains players"
        );
        let messages = rt.state.messages.clone();
        rt.state.messages.clear();
        rt.invalidate();
        rt.frame(0.);
        assert!(rt.state.inline_audio.is_empty(), "unmount retires players");
        assert!(
            ids.iter()
                .all(|id| rt.state.media_players.get(*id).is_none())
        );
        rt.state.messages = messages;
        rt.invalidate();
        rt.frame(0.);
        rt.frame(0.);
        assert_eq!(rt.state.inline_audio.len(), 2);
        assert!(
            rt.state.inline_audio.values().all(|id| !ids.contains(id)),
            "remount starts new players"
        );
    }
}
