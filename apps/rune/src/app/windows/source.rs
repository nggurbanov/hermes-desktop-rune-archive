use super::*;
use crate::client::windows::SourceDescriptor;
use crate::connections::{Lease, Resolver};
#[derive(Clone)]
pub(super) struct Retained {
    pub _resolver: Resolver,
    pub lease: Lease,
}
pub(super) struct Fresh {
    route: WindowRoute,
    owner: super::super::routing::Owner,
    intent: u64,
    loading: bool,
    status: String,
}
const FRESH_CONNECTING: &str = "Connecting to the profile window agent…";
#[derive(Clone)]
pub(super) struct Pinned {
    connection: Connection,
    route: WindowRoute,
    retained: Option<Retained>,
}
pub(super) enum Source {
    Direct(Connection),
    Saved {
        expected_principal: Option<String>,
        resolver: Resolver,
        receive: Receiver<Result<Lease, String>>,
    },
}
impl Source {
    fn restore(descriptor: &SourceDescriptor, resolver: Option<Resolver>) -> Result<Self, String> {
        match descriptor {
            SourceDescriptor::Saved { id, revision } => {
                let resolver = resolver.ok_or("The captured source resolver is unavailable")?;
                let receive = resolver.resolve(id.clone(), Some(revision.clone()))?;
                Ok(Self::Saved { expected_principal: None, resolver, receive })
            }
            _ => descriptor.connection_without_secret().map(Self::Direct).ok_or_else(|| "The profile window's captured source needs credentials. Connect that source before opening a new conversation.".into()),
        }
    }
    pub fn pending(&self) -> bool {
        matches!(self, Self::Saved { .. })
    }
    pub fn capture(
        connection: Option<&Connection>,
        resolver: Option<Resolver>,
    ) -> Result<Option<Self>, String> {
        let Some(connection) = connection else {
            return Ok(None);
        };
        if let Connection::Saved { id, revision, .. } = connection {
            let resolver = resolver.ok_or("The source resolver is unavailable")?;
            let receive = resolver.resolve(id.clone(), Some(revision.clone()))?;
            Ok(Some(Self::Saved {
                expected_principal: Some(crate::client::connection_key(connection)?),
                resolver,
                receive,
            }))
        } else {
            Ok(Some(Self::Direct(connection.clone())))
        }
    }
}
impl Viewer {
    pub(in crate::app) fn start_window_source(&mut self) {
        if self.fixture || self.window.started {
            return;
        }
        if self.window.source.is_none() && self.connection.is_none() {
            let source = self
                .window
                .lease
                .as_ref()
                .and_then(|l| l.record().route.source.clone());
            match source {
                Some(SourceDescriptor::Saved { id, revision }) => {
                    let result = self
                        .connections
                        .resolver()
                        .ok_or("The source resolver is unavailable".to_string())
                        .and_then(|resolver| {
                            let receive = resolver.resolve(id.clone(), Some(revision.clone()))?;
                            Ok(Source::Saved {
                                expected_principal: None,
                                resolver,
                                receive,
                            })
                        });
                    match result {
                        Ok(source) => self.window.source = Some(source),
                        Err(error) => {
                            self.error = Some(error);
                            self.connection_form = true;
                            return;
                        }
                    }
                }
                Some(source) => match source.connection_without_secret() {
                    Some(connection) => self.window.source = Some(Source::Direct(connection)),
                    None => {
                        if let SourceDescriptor::Remote { endpoint, .. } = source {
                            self.connection_url.replace_text(&endpoint);
                        }
                        self.loading = false;
                        self.connection_form = true;
                        self.error=Some("This source needs credentials. Enter them or select its saved connection.".into());
                        return;
                    }
                },
                None => {}
            }
        }
        self.poll_window_source();
        if self.window.source.is_none() && !self.window.started {
            self.window.started = true;
            if self.connection.is_none() {
                self.connection_form = true;
                self.loading = false;
                self.status = "Not connected".into();
            }
            self.begin();
        }
    }
    pub(in crate::app) fn cancel_window_fresh(&mut self) {
        if let Some(fresh) = self.window.pending_fresh.take() {
            self.window.source = None;
            if self.intent == fresh.intent && self.status == FRESH_CONNECTING {
                self.loading = fresh.loading;
                self.status = fresh.status;
            }
        }
    }
    fn fail_window_source(&mut self, message: String, show_form: bool) {
        if self.window.pending_fresh.is_some() {
            self.cancel_window_fresh();
            self.client_error = Some(message);
        } else {
            self.error = Some(message);
            self.loading = false;
            if show_form {
                self.connection_form = true;
            }
        }
    }
    pub(super) fn poll_window_source(&mut self) -> bool {
        if self
            .window
            .pending_fresh
            .as_ref()
            .is_some_and(|fresh| fresh.owner != self.route_owner())
        {
            self.cancel_window_fresh();
            return true;
        }
        let Some(source) = self.window.source.take() else {
            if let Some(retained) = &self.window.retained {
                if !retained.lease.is_valid() {
                    let message = retained
                        .lease
                        .error()
                        .unwrap_or_else(|| "The captured source is no longer available".into());
                    self.window.retained = None;
                    self.window.source_retired = true;
                    self.connection = None;
                    self.connection_form = true;
                    self.conversation.close_connection(self.connection_epoch);
                    self.chat.close_connection(self.connection_epoch);
                    self.subagents.close_connection(self.connection_epoch);
                    self.audio.close_connection(self.connection_epoch);
                    self.terminal.close_connection(self.connection_epoch);
                    self.background.retire_connection(self.connection_epoch);
                    self.preview.close_connection(self.connection_epoch);
                    self.media_players.close_connection(self.connection_epoch);
                    self.audio_preview = None;
                    self.loading = false;
                    self.history_loading = false;
                    self.error = Some(message);
                    self.status = "Disconnected".into();
                    return true;
                }
            }
            return false;
        };
        let (connection, retained) = match source {
            Source::Direct(connection) => (connection, None),
            Source::Saved {
                expected_principal,
                resolver,
                receive,
            } => match receive.try_recv() {
                Ok(Ok(lease)) => {
                    if !lease.is_valid()
                        || expected_principal.as_deref().is_some_and(|expected| {
                            crate::client::connection_key(lease.connection()).as_deref()
                                != Ok(expected)
                        })
                    {
                        self.fail_window_source(
                            "The captured source changed while opening this window".into(),
                            false,
                        );
                        return true;
                    }
                    let connection = lease.connection().clone();
                    (
                        connection,
                        Some(Retained {
                            _resolver: resolver,
                            lease,
                        }),
                    )
                }
                Ok(Err(error)) => {
                    self.fail_window_source(error, true);
                    return true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.window.source = Some(Source::Saved {
                        expected_principal,
                        resolver,
                        receive,
                    });
                    return false;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.fail_window_source("The captured source service stopped".into(), false);
                    return true;
                }
            },
        };
        if let Some(fresh) = self.window.pending_fresh.take() {
            let route = fresh.route;
            if !self.use_connection_for_profile(connection, Some(route.profile.clone())) {
                self.loading = false;
                self.status = "Connection change blocked".into();
                return true;
            }
            self.window.retained = retained;
            self.pin_window_source();
            self.window.boot_target = Some(WindowRoute {
                session: None,
                canonical_bot: false,
                ..route
            });
            self.new_draft_on_current_source();
        } else {
            self.window.retained = retained;
            self.connection = Some(connection);
            self.connection_form = false;
            self.window.started = true;
            self.pin_window_source();
            self.remember_legacy_window_source();
            self.configure_window_inspection();
            self.begin();
        }
        true
    }
    fn pin_window_source(&mut self) {
        if self.window.pinned.is_some() {
            return;
        }
        let route = self
            .window
            .lease
            .as_ref()
            .and_then(|l| l.record().pinned_route.clone());
        if let (Some(route), Some(connection)) = (route, self.connection.clone()) {
            self.window.pinned = Some(Pinned {
                connection,
                route,
                retained: self.window.retained.clone(),
            });
        }
    }
    fn await_window_fresh(&mut self, route: WindowRoute) {
        self.window.pending_fresh = Some(Fresh {
            route,
            owner: self.route_owner(),
            intent: self.intent,
            loading: self.loading,
            status: self.status.clone(),
        });
        self.loading = true;
        self.status = FRESH_CONNECTING.into();
    }
    /// A profile-window default is immutable; an ordinary peer only inherits for boot.
    pub(in crate::app) fn prepare_window_new_session(&mut self) -> bool {
        if self.fixture {
            return false;
        }
        let pin = match self.window.pinned.clone() {
            Some(pin) => pin,
            None => {
                let Some(record) = self.window.lease.as_ref().map(|lease| lease.record()) else {
                    return false;
                };
                if record.kind != BootstrapKind::ProfilePinned {
                    return false;
                }
                let Some(route) = record.pinned_route.clone() else {
                    self.client_error =
                        Some("The profile window's captured route is unavailable".into());
                    return true;
                };
                let Some(descriptor) = route.source.as_ref() else {
                    self.client_error =
                        Some("The profile window's captured source is unavailable".into());
                    return true;
                };
                let matching = self
                    .connection
                    .as_ref()
                    .filter(|connection| {
                        SourceDescriptor::from_connection(connection).ok().as_ref()
                            == Some(descriptor)
                    })
                    .cloned();
                if let Some(connection) = matching {
                    let pin = Pinned {
                        connection,
                        route,
                        retained: self.window.retained.clone(),
                    };
                    self.window.pinned = Some(pin.clone());
                    pin
                } else {
                    if !self.allow_owner_change("") {
                        return true;
                    }
                    let resolver = match descriptor {
                        SourceDescriptor::Saved { id, revision } => {
                            self.window_route_resolver(id, revision)
                        }
                        _ => None,
                    };
                    match Source::restore(descriptor, resolver) {
                        Ok(source) => {
                            self.window.source = Some(source);
                            self.await_window_fresh(route);
                        }
                        Err(error) => self.client_error = Some(error),
                    }
                    return true;
                }
            }
        };
        if self
            .connection
            .as_ref()
            .and_then(|c| crate::client::connection_key(c).ok())
            == crate::client::connection_key(&pin.connection).ok()
        {
            if self.profile != pin.route.profile {
                if !self.allow_owner_change(&pin.route.profile) {
                    return true;
                }
                self.select_profile(pin.route.profile);
            }
            return false;
        }
        if !self.allow_owner_change("") {
            return true;
        }
        let resolver = pin
            .retained
            .as_ref()
            .map(|r| r._resolver.clone())
            .or_else(|| self.connections.resolver());
        match Source::capture(Some(&pin.connection), resolver) {
            Ok(source) => {
                self.window.source = source;
                self.await_window_fresh(pin.route);
            }
            Err(error) => self.client_error = Some(error),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_new_session_uses_profile_pin_but_an_ordinary_peer_only_inherits_for_boot() {
        let connection = Connection::Url {
            endpoint: "https://pin-fixture.invalid".into(),
            token: Some("synthetic".into()),
        };
        let mut pinned = Viewer::empty();
        pinned.connection = Some(connection.clone());
        pinned.profile = "archive".into();
        pinned.selected_session = Some("old-chat".into());
        pinned.window.pinned = Some(Pinned {
            connection: connection.clone(),
            route: WindowRoute {
                profile: "research".into(),
                ..Default::default()
            },
            retained: None,
        });
        pinned.new_draft();
        assert_eq!(pinned.profile, "research");
        assert!(pinned.selected_session.is_none());
        let private =
            std::env::temp_dir().join(format!("hermes-pin-ordinary-{}", uuid::Uuid::new_v4()));
        let mut ordinary = Viewer::empty();
        ordinary.device_startup = Some(crate::client::startup::Startup::load_at(
            private.join("startup.json"),
        ));
        ordinary.connection = Some(connection);
        ordinary.profile = "archive".into();
        ordinary.selected_session = Some("old-chat".into());
        ordinary.new_draft();
        assert_eq!(ordinary.profile, "archive");
        assert!(ordinary.selected_session.is_none());
        drop(ordinary);
        let _ = std::fs::remove_dir_all(private);
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    fn connection(endpoint: &str) -> Connection {
        Connection::Url {
            endpoint: endpoint.into(),
            token: None,
        }
    }
    fn pinned() -> Viewer {
        let mut s = Viewer::empty();
        s.profile = "research".into();
        s.connection = Some(connection("https://temporary.fixture.invalid"));
        s.window.pinned = Some(Pinned {
            connection: connection("https://pinned.fixture.invalid"),
            route: WindowRoute {
                profile: "research".into(),
                ..Default::default()
            },
            retained: None,
        });
        s
    }
    #[test]
    fn a_same_profile_does_not_bypass_the_source_change_guard() {
        let mut s = pinned();
        let before = crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap();
        s.chrome.writing = true;
        assert!(s.prepare_window_new_session());
        assert!(s.window.pending_fresh.is_none());
        assert!(s.window.source.is_none());
        assert!(!s.loading);
        assert!(s.client_error.is_some());
        assert_eq!(
            crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap(),
            before
        );
    }
    #[test]
    fn an_owner_change_rejected_after_resolution_does_not_install_the_pin() {
        let mut s = pinned();
        s.profile = "archive".into();
        let epoch = s.connection_epoch;
        let before = crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap();
        assert!(s.prepare_window_new_session());
        assert!(s.loading);
        s.chrome.writing = true;
        assert!(s.poll_window_source());
        assert_eq!(s.profile, "archive");
        assert_eq!(s.connection_epoch, epoch);
        assert_eq!(
            crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap(),
            before
        );
        assert!(s.window.boot_target.is_none());
        assert!(s.window.pending_fresh.is_none());
        assert!(s.window.source.is_none());
        assert!(s.window.retained.is_none());
        assert!(!s.loading);
        assert_eq!(s.status, "Connection change blocked");
    }
}

#[cfg(test)]
mod owner_tests {
    use super::*;
    struct Workspace(PathBuf);
    impl Workspace {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("hermes-pin-owner-{}", uuid::Uuid::new_v4())))
        }
        fn startup(&self) -> PathBuf {
            self.0.join("startup.json")
        }
        fn viewer(&self) -> (Viewer, Receiver<Request>) {
            self.with_source(connection("pinned"))
        }
        fn with_source(&self, connection: Connection) -> (Viewer, Receiver<Request>) {
            let route = WindowRoute {
                source: Some(SourceDescriptor::from_connection(&connection).unwrap()),
                profile: "research".into(),
                canonical_bot: true,
                ..Default::default()
            };
            let mut s = Viewer::empty();
            s.window.lease = Some(
                WindowStore::at(self.0.clone())
                    .mint_peer(BootstrapKind::ProfilePinned, route)
                    .unwrap(),
            );
            s.connection = Some(connection);
            s.profile = "research".into();
            s.window.started = true;
            s.connection_form = false;
            s.status = "Connected".into();
            s.draft_storage_available = false;
            s.device_startup = Some(crate::client::startup::Startup::load_at(self.startup()));
            s.profiles = profiles();
            s.pin_window_source();
            s.init_editors(std::sync::Arc::new(|| {}));
            let (send, requests) = mpsc::channel();
            let (_, receive) = mpsc::channel();
            s.network = Some(Network { send, receive });
            (s, requests)
        }
    }
    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn connection(host: &str) -> Connection {
        Connection::Url {
            endpoint: format!("https://{host}.pin-owner.fixture.invalid"),
            token: Some("synthetic".into()),
        }
    }
    fn profiles() -> Vec<Profile> {
        ["research", "archive"]
            .into_iter()
            .map(|name| Profile {
                name: name.into(),
                label: name.into(),
                description: String::new(),
                canonical_session: None,
            })
            .collect()
    }
    fn principal(s: &Viewer) -> String {
        crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap()
    }
    #[test]
    fn a_delayed_pinned_fresh_source_cannot_replace_a_newer_session_bot_page_or_editor_owner() {
        for navigation in ["session", "bot", "page", "draft"] {
            let root = Workspace::new();
            let (mut s, requests) = root.viewer();
            assert!(s.use_connection_for_profile(connection("temporary"), Some("archive".into())));
            let request = requests.recv().unwrap();
            s.apply(Reply {
                intent: request.intent,
                result: Ok(Update::Connected {
                    profiles: profiles(),
                    profile: "archive".into(),
                    sessions: vec![],
                }),
            });
            s.selected_session = Some("old-chat".into());
            s.messages = vec![HistoryMessage {
                role: "assistant".into(),
                content: "Old history".into(),
                ..Default::default()
            }];
            assert!(s.ensure_editor());
            let old = s.composer_scope();
            let old_editor = s.composer.editor_id().clone();
            s.composer
                .draft_mut(old.clone())
                .text
                .replace_text("Old unsent input");
            let source = principal(&s);
            s.new_draft();
            assert!(s.window.pending_fresh.is_some());
            assert_eq!(
                principal(&s),
                source,
                "resolution must leave the outgoing source intact"
            );
            match navigation {
                "session" => s.open_session("newer-chat".into()),
                "bot" => s.open_bot("research".into()),
                "page" => s.open_route(crate::pages::Route::Connections),
                "draft" => s.new_draft_on_current_source(),
                _ => unreachable!(),
            }
            assert!(s.ensure_editor());
            let scope = s.composer_scope();
            if scope != old {
                assert_ne!(
                    s.composer.editor_id(),
                    &old_editor,
                    "{navigation}: actual controller ownership must change"
                );
            }
            s.composer
                .draft_mut(scope.clone())
                .text
                .replace_text("New owner input");
            let editor = s.composer.editor_id().clone();
            let page = s.route;
            let messages = s
                .messages
                .iter()
                .map(|m| m.content.clone())
                .collect::<Vec<_>>();
            let _ = requests.try_iter().collect::<Vec<_>>();
            s.poll_window_source();
            assert_eq!(
                principal(&s),
                source,
                "{navigation}: the stale pin stole the source"
            );
            assert_eq!(s.composer_scope(), scope, "{navigation}");
            assert_eq!(s.composer.editor_id(), &editor, "{navigation}");
            assert_eq!(s.route, page, "{navigation}");
            assert_eq!(
                s.messages
                    .iter()
                    .map(|m| m.content.clone())
                    .collect::<Vec<_>>(),
                messages,
                "{navigation}"
            );
            assert_eq!(
                s.composer.draft(&scope).unwrap().text.text,
                "New owner input",
                "{navigation}"
            );
            if scope != old {
                assert_eq!(
                    s.editor_mut(&old_editor, &old)
                        .unwrap()
                        .draft(&old)
                        .unwrap()
                        .text
                        .text,
                    "Old unsent input",
                    "{navigation}"
                );
            }
            assert!(s.window.pending_fresh.is_none());
            assert!(s.window.source.is_none());
            assert!(s.window.boot_target.is_none());
            assert!(
                requests.try_recv().is_err(),
                "{navigation}: stale resolution dispatched a connect"
            );
        }
    }
    #[test]
    fn an_explicit_same_owner_action_immediately_retires_a_pending_profile_pin() {
        for action in ["page", "profile", "session", "fresh"] {
            let root = Workspace::new();
            let (mut s, requests) = root.viewer();
            s.connection = Some(connection("temporary"));
            s.profile = "archive".into();
            s.selected_session = (action != "fresh").then(|| "outgoing-chat".into());
            s.ensure_editor();
            let scope = s.composer_scope();
            let editor = s.composer.editor_id().clone();
            s.composer
                .draft_mut(scope.clone())
                .text
                .replace_text("Current owner input");
            assert!(s.prepare_window_new_session());
            assert!(s.window.pending_fresh.is_some());
            match action {
                "page" => s.open_route(s.route),
                "profile" => s.select_profile(s.profile.clone()),
                "session" => s.open_session("outgoing-chat".into()),
                "fresh" => s.new_draft_on_current_source(),
                _ => unreachable!(),
            }
            assert!(
                s.window.pending_fresh.is_none(),
                "explicit {action} supersedes a pin even when its owner key is unchanged"
            );
            assert!(s.window.source.is_none());
            assert_eq!(s.composer_scope(), scope);
            assert_eq!(s.composer.editor_id(), &editor);
            assert_eq!(
                s.composer.draft(&scope).unwrap().text.text,
                "Current owner input"
            );
            assert!(requests.try_recv().is_err());
        }
    }
    #[test]
    fn a_failed_pin_keeps_the_outgoing_source_input_and_original_error_usable() {
        for old_error in [None, Some("Original outgoing error".to_owned())] {
            let root = Workspace::new();
            let (mut s, requests) = root.viewer();
            assert!(s.use_connection_for_profile(connection("temporary"), Some("archive".into())));
            let request = requests.recv().unwrap();
            s.apply(Reply {
                intent: request.intent,
                result: Ok(Update::Connected {
                    profiles: profiles(),
                    profile: "archive".into(),
                    sessions: vec![],
                }),
            });
            assert!(s.ensure_editor());
            s.error = old_error.clone();
            let source = principal(&s);
            let scope = s.composer_scope();
            let editor = s.composer.editor_id().clone();
            s.composer
                .draft_mut(scope.clone())
                .text
                .replace_text("Outgoing input survives target failure");
            let sendable = s.can_send();
            s.new_draft();
            assert!(s.window.pending_fresh.is_some());
            s.fail_window_source("Saved pinned source is unavailable".into(), true);
            assert_eq!(principal(&s), source);
            assert_eq!(s.composer_scope(), scope);
            assert_eq!(s.composer.editor_id(), &editor);
            assert_eq!(
                s.composer.draft(&scope).unwrap().text.text,
                "Outgoing input survives target failure"
            );
            assert_eq!(s.error, old_error);
            assert_eq!(s.can_send(), sendable);
            assert!(!s.loading);
            assert!(!s.connection_form);
            assert_eq!(s.status, "Connected");
            assert_eq!(
                s.client_error.as_deref(),
                Some("Saved pinned source is unavailable")
            );
            assert!(s.window.pending_fresh.is_none());
            assert!(s.window.source.is_none());
            assert!(requests.try_recv().is_err());
        }
    }
    #[test]
    fn an_unadopted_profile_pin_restores_its_original_source_and_refuses_missing_credentials() {
        for protected in [false, true] {
            let root = Workspace::new();
            let original = Connection::Url {
                endpoint: "https://original.pin-owner.fixture.invalid".into(),
                token: protected.then(|| "synthetic-original-secret".into()),
            };
            let original_key = crate::client::connection_key(&original).unwrap();
            let (mut s, requests) = root.with_source(original);
            let captured = s
                .window
                .lease
                .as_ref()
                .unwrap()
                .record()
                .pinned_route
                .clone();
            s.window.pinned = None;
            assert!(s.use_connection_for_profile(connection("temporary"), Some("archive".into())));
            let request = requests.recv().unwrap();
            s.apply(Reply {
                intent: request.intent,
                result: Ok(Update::Connected {
                    profiles: profiles(),
                    profile: "archive".into(),
                    sessions: vec![],
                }),
            });
            s.selected_session = Some("outgoing-chat".into());
            assert!(s.ensure_editor());
            let outgoing = s.composer_scope();
            let outgoing_editor = s.composer.editor_id().clone();
            let outgoing_key = principal(&s);
            s.composer
                .draft_mut(outgoing.clone())
                .text
                .replace_text("Unsent on temporary source");
            s.new_draft();
            assert_eq!(
                principal(&s),
                outgoing_key,
                "preparation cannot replace the outgoing workspace"
            );
            if protected {
                assert!(
                    s.client_error
                        .as_ref()
                        .is_some_and(|error| error.contains("needs credentials"))
                );
                assert!(s.window.source.is_none());
                assert!(s.window.pending_fresh.is_none());
                assert!(s.window.pinned.is_none());
                assert_eq!(s.composer_scope(), outgoing);
                assert_eq!(s.composer.editor_id(), &outgoing_editor);
                assert!(!s.loading);
                assert!(requests.try_recv().is_err());
            } else {
                assert!(s.window.pending_fresh.is_some());
                assert!(s.poll_window_source());
                assert_eq!(principal(&s), original_key);
                assert_eq!(s.profile, "research");
                assert!(s.selected_session.is_none());
                let pin = s
                    .window
                    .pinned
                    .as_ref()
                    .expect("successful captured-source restoration adopts the immutable pin");
                assert_eq!(
                    crate::client::connection_key(&pin.connection).unwrap(),
                    original_key
                );
                assert_eq!(
                    Some(pin.route.clone()),
                    captured,
                    "the profile pin retains its original normalized target metadata"
                );
                assert!(!pin.route.canonical_bot);
                let request = requests.recv().unwrap();
                assert!(
                    matches!(request.command, Command::ConnectExact(_, ref profile) if profile == "research")
                );
                s.apply(Reply {
                    intent: request.intent,
                    result: Ok(Update::Connected {
                        profiles: profiles(),
                        profile: "research".into(),
                        sessions: vec![],
                    }),
                });
                assert!(s.apply_window_target());
                assert!(
                    s.selected_session.is_none(),
                    "generic New Session must remain fresh after the captured source connects"
                );
                assert!(!s.bot_empty);
                assert!(
                    requests.try_recv().is_err(),
                    "generic New Session cannot dispatch Bot Chat opening"
                );
            }
            assert_eq!(
                s.editor_mut(&outgoing_editor, &outgoing)
                    .unwrap()
                    .draft(&outgoing)
                    .unwrap()
                    .text
                    .text,
                "Unsent on temporary source"
            );
            assert_eq!(
                s.window.lease.as_ref().unwrap().record().pinned_route,
                captured
            );
        }
    }
    #[test]
    fn real_profile_window_metadata_bypasses_unavailable_and_future_device_defaults() {
        for future in [false, true] {
            let root = Workspace::new();
            let (mut s, requests) = root.viewer();
            if future {
                crate::client::private_write(
                    &root.startup(),
                    br#"{"version":9,"future":{"retain":true}}"#,
                )
                .unwrap();
                s.device_startup = Some(crate::client::startup::Startup::load_at(root.startup()));
            } else {
                s.device_startup = None;
            }
            let source = principal(&s);
            s.window.pinned = None;
            assert_eq!(
                s.window.lease.as_ref().unwrap().record().kind,
                BootstrapKind::ProfilePinned
            );
            s.selected_session = Some("before-pin-adoption".into());
            s.new_draft();
            assert_eq!(principal(&s), source);
            assert_eq!(s.profile, "research");
            assert!(s.selected_session.is_none());
            assert!(
                s.client_error.is_none(),
                "profile metadata must bypass a failed device preference read"
            );
            s.pin_window_source();
            s.select_profile("archive".into());
            s.selected_session = Some("after-profile-navigation".into());
            s.new_draft();
            assert_eq!(principal(&s), source);
            assert_eq!(s.profile, "research");
            assert!(s.selected_session.is_none());
            assert!(s.client_error.is_none());
            assert!(
                requests
                    .try_iter()
                    .all(|r| matches!(r.command, Command::Sessions(_)))
            );
            if future {
                assert_eq!(
                    std::fs::read(root.startup()).unwrap(),
                    br#"{"version":9,"future":{"retain":true}}"#
                );
            }
        }
    }
}
