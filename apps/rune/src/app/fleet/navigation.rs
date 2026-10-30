//! Fleet actions reuse the device route's resolve/commit and then await its exact owner.
use super::*;

pub(in crate::app) struct Waiting {
    owner: super::super::routing::Owner,
    profile: String,
    destination: Destination,
    replace: Option<(String, String)>,
}
impl Viewer {
    pub(in crate::app) fn cancel_fleet_destination(&mut self) {
        self.fleet_state.destination = None;
    }
    pub(in crate::app) fn begin_fleet_navigation(
        &mut self,
        version: SourceVersion,
        profile: String,
        destination: Destination,
    ) {
        self.cancel_device_route();
        self.fleet_ui.close();
        self.profile_picker = false;
        if !self.fleet_version_exists(&version) {
            self.client_error =
                Some("The clicked source changed or was removed; refresh its profiles".into());
            return;
        }
        let same_source = self.fleet_active_version().as_ref() == Some(&version);
        if same_source
            && self.profile == profile
            && matches!(destination, Destination::ProfilePick)
            && !self.sidebar_state.prefs.all_profiles
        {
            // Re-tapping the live profile keeps its actual conversation and input.
            if self.loading || self.error.is_some() || self.status != "Connected" {
                self.exact_connection_profile = true;
                self.reconnect();
            }
            return;
        }
        if !self.allow_owner_change(&profile) {
            return;
        }
        if same_source {
            if !self.profiles.iter().any(|p| p.name == profile) {
                self.client_error =
                    Some("The requested profile is no longer served by its source".into());
                return;
            }
            let replace = self
                .connection
                .as_ref()
                .and_then(|c| self.target_foreground_slot(c));
            if self.profile == profile
                && !self.loading
                && self.error.is_none()
                && self.status == "Connected"
            {
                self.apply_fleet_destination(profile, destination, replace);
                return;
            }
            if self.profile != profile {
                self.select_profile(profile.clone());
            } else {
                self.reconnect();
            }
            if self.profile != profile {
                return;
            }
            self.queue_fleet_destination(profile, destination, replace);
        } else if let Err(error) = self.queue_fleet_route(version, profile, destination) {
            self.client_error = Some(error);
        }
    }
    pub fn fleet_open_profile(&mut self, version: SourceVersion, profile: String) {
        self.begin_fleet_navigation(version, profile, Destination::ProfilePick);
    }
    pub fn fleet_connect_source(&mut self, version: SourceVersion) {
        let profile = self.fleet_source_profile(&version);
        self.begin_fleet_navigation(version, profile, Destination::ProfilePick);
    }
    pub(in crate::app) fn fleet_source_profile(&self, version: &SourceVersion) -> String {
        if self.fleet_active_version().as_ref() == Some(version) {
            return self.profile.clone();
        }
        let known = self
            .fleet_snapshot()
            .sources
            .iter()
            .find(|s| &s.descriptor.version == version);
        if let SourceId::Saved(id) = &version.id {
            return self
                .connections
                .remembered_profile(id)
                .unwrap_or("default")
                .into();
        }
        if known.is_some_and(|s| s.profiles.iter().any(|p| p.name == "default")) {
            return "default".into();
        }
        known
            .and_then(|s| s.profiles.first())
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "default".into())
    }
    pub(in crate::app) fn queue_fleet_destination(
        &mut self,
        profile: String,
        destination: Destination,
        replace: Option<(String, String)>,
    ) {
        self.selected_session = None;
        self.pending_bot = None;
        if !self.ensure_editor() {
            return;
        }
        self.fleet_state.destination = Some(Waiting {
            owner: self.route_owner(),
            profile,
            destination,
            replace,
        });
    }
    pub(in crate::app) fn fleet_navigation_waiting(&self) -> bool {
        self.fleet_state.destination.is_some()
    }
    pub(in crate::app) fn poll_fleet_destination(&mut self) -> bool {
        let Some(waiting) = self.fleet_state.destination.take() else {
            return false;
        };
        if self.route_owner() != waiting.owner
            || self.profile != waiting.profile
            || self.error.is_some()
        {
            return true;
        }
        if self.loading || self.status != "Connected" {
            self.fleet_state.destination = Some(waiting);
            return false;
        }
        if !self.profiles.iter().any(|p| p.name == waiting.profile) {
            self.client_error =
                Some("The requested profile disappeared before its workspace opened".into());
            return true;
        }
        self.apply_fleet_destination(waiting.profile, waiting.destination, waiting.replace);
        true
    }
    fn apply_fleet_destination(
        &mut self,
        profile: String,
        destination: Destination,
        replace: Option<(String, String)>,
    ) {
        if matches!(
            &destination,
            Destination::Bot | Destination::Session { .. } | Destination::All
        ) {
            self.layout.open("workspace");
            if self.route == crate::pages::Route::Settings {
                self.settings.vault.dismiss();
            }
            self.route = crate::pages::Route::Chat;
        }
        if !matches!(destination, Destination::All) && self.sidebar_state.prefs.all_profiles {
            self.sidebar_action(crate::sidebar::Action::Toggle("all-profiles"));
        }
        match destination {
            Destination::ProfilePick => {
                self.selected_session = None;
                self.pending_bot = None;
                self.ensure_editor();
                self.messages.clear();
                self.history_loading = false;
                self.history_truncated = false;
                self.bot_empty = false;
                self.compose_error = None;
                self.composer.close();
            }
            Destination::Bot => self.open_bot_with_slot(profile, false, replace),
            Destination::Session {
                id,
                title,
                read_only,
            } => {
                if let Some(read_only) = read_only {
                    let scope = DraftScope {
                        connection: self.connection_epoch,
                        profile: profile.clone(),
                        session: Some(id.clone()),
                    };
                    if read_only {
                        self.read_only_chats.insert(scope,"This scheduled run has no confirmed live owner. Start a new conversation to continue.".into());
                    } else {
                        self.read_only_chats.remove(&scope);
                    }
                }
                self.open_conversation_with_slot(profile, id, title, false, false, replace);
            }
            Destination::All => {
                if !self.sidebar_state.prefs.all_profiles {
                    self.sidebar_action(crate::sidebar::Action::Toggle("all-profiles"));
                }
            }
            Destination::Manage(action) => {
                self.open_route(crate::pages::Route::Bots);
                match action {
                    crate::view::fleet::ManageAction::Manage => {}
                    crate::view::fleet::ManageAction::Create => {
                        self.bots_action(crate::pages::bots::Action::New)
                    }
                    crate::view::fleet::ManageAction::Import => {
                        self.bots_action(crate::pages::bots::Action::Import)
                    }
                }
            }
            Destination::Cron(id) => {
                self.open_route(crate::pages::Route::Cron);
                self.cron_action(crate::pages::cron::Action::Select(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn viewer() -> (Viewer, Receiver<Request>) {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Url {
            endpoint: "https://fleet-owner.invalid/hermes".into(),
            token: Some("synthetic".into()),
        });
        s.connection_form = false;
        s.profile = "work".into();
        s.status = "Connected".into();
        s.connection_epoch = 91;
        s.selected_session = Some("stored-current".into());
        s.tabs = vec![ConversationTab {
            profile: "work".into(),
            session: "stored-current".into(),
            title: "Current".into(),
            bot: false,
        }];
        s.profiles = ["work", "archive"]
            .into_iter()
            .map(|name| Profile {
                name: name.into(),
                label: name.into(),
                description: String::new(),
                canonical_session: None,
            })
            .collect();
        let (send, requests) = mpsc::channel();
        let (_, receive) = mpsc::channel();
        s.network = Some(Network { send, receive });
        s.draft_storage_available = false;
        s.init_editors(std::sync::Arc::new(|| {}));
        assert!(s.ensure_editor());
        (s, requests)
    }
    #[test]
    fn changed_profile_pick_waits_for_exact_ack_then_starts_fresh_without_a_bot_read() {
        let (mut s, requests) = viewer();
        let version = s.fleet_active_version().unwrap();
        let work = s.composer_scope();
        let editor = s.composer.editor_id().clone();
        s.composer
            .draft_mut(work.clone())
            .text
            .replace_text("Unsent work input");
        s.route = crate::pages::Route::Settings;
        s.fleet_open_profile(version, "archive".into());
        let requested = requests.recv().unwrap();
        assert!(matches!(requested.command,Command::Sessions(ref p) if p=="archive"));
        assert!(s.fleet_navigation_waiting());
        assert!(!s.poll_fleet_destination());
        assert!(requests.try_recv().is_err());
        s.apply(Reply {
            intent: requested.intent,
            result: Ok(Update::Sessions {
                profile: "archive".into(),
                sessions: vec![],
            }),
        });
        assert!(s.poll_fleet_destination());
        assert!(!s.fleet_navigation_waiting());
        assert!(
            requests.try_recv().is_err(),
            "profile picks never request canonical Bot Chat"
        );
        assert_eq!(s.profile, "archive");
        assert!(s.selected_session.is_none());
        assert!(s.pending_bot.is_none());
        assert!(!s.bot_empty);
        assert_ne!(s.composer.editor_id(), &editor);
        assert_eq!(
            s.inspect_owned_editor(&work).unwrap()["draft"],
            "Unsent work input"
        );
        assert_eq!(s.route, crate::pages::Route::Settings);
        assert_eq!(s.tabs.len(), 1);
    }
    #[test]
    fn current_profile_pick_cancels_pending_intent_and_preserves_the_stored_session_editor_and_page()
     {
        let (mut s, requests) = viewer();
        let version = s.fleet_active_version().unwrap();
        let scope = s.composer_scope();
        let editor = s.composer.editor_id().clone();
        s.composer
            .draft_mut(scope.clone())
            .text
            .replace_text("Stored conversation follow-up");
        s.route = crate::pages::Route::Settings;
        s.fleet_state.destination = Some(Waiting {
            owner: s.route_owner(),
            profile: "work".into(),
            destination: Destination::Bot,
            replace: None,
        });
        s.fleet_open_profile(version, "work".into());
        assert!(!s.fleet_navigation_waiting());
        assert!(
            requests.try_recv().is_err(),
            "the current profile pick must not open Bot Chat"
        );
        assert_eq!(s.composer_scope(), scope);
        assert_eq!(s.composer.editor_id(), &editor);
        assert_eq!(
            s.composer.draft(&scope).unwrap().text.text,
            "Stored conversation follow-up"
        );
        assert_eq!(s.route, crate::pages::Route::Settings);
        assert_eq!(s.tabs.len(), 1);
        assert!(s.pending_bot.is_none());
    }
    #[test]
    fn returning_from_all_profiles_starts_a_fresh_current_profile_without_opening_bot_chat() {
        let (mut s, requests) = viewer();
        let version = s.fleet_active_version().unwrap();
        let previous = s.composer_scope();
        let editor = s.composer.editor_id().clone();
        s.composer
            .draft_mut(previous.clone())
            .text
            .replace_text("Stored input is retained");
        s.sidebar_state.prefs.all_profiles = true;
        s.fleet_open_profile(version, "work".into());
        assert!(!s.sidebar_state.prefs.all_profiles);
        assert!(s.selected_session.is_none());
        assert!(s.pending_bot.is_none());
        assert!(!s.bot_empty);
        assert_ne!(s.composer.editor_id(), &editor);
        assert_eq!(
            s.inspect_owned_editor(&previous).unwrap()["draft"],
            "Stored input is retained"
        );
        assert!(
            requests
                .try_iter()
                .all(|request| !matches!(request.command, Command::Bot(_)))
        );
    }
    fn pending_page_session(route: crate::pages::Route) -> (Viewer, Receiver<Request>, u64) {
        let (mut s, requests) = viewer();
        s.apply(Reply {
            intent: s.intent,
            result: Ok(Update::Sessions {
                profile: "work".into(),
                sessions: vec![],
            }),
        });
        s.open_route(route);
        let version = s.fleet_active_version().unwrap();
        s.begin_fleet_navigation(
            version,
            "archive".into(),
            Destination::Session {
                id: "captured-session".into(),
                title: "Captured archive chat".into(),
                read_only: None,
            },
        );
        let requested = requests
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(matches!(requested.command, Command::Sessions(ref profile) if profile=="archive"));
        assert!(s.fleet_navigation_waiting());
        (s, requests, requested.intent)
    }
    #[test]
    fn internal_page_owner_reload_preserves_pending_session_through_poll_before_exact_ack() {
        for route in [crate::pages::Route::Settings, crate::pages::Route::Cron] {
            let (mut s, requests, intent) = pending_page_session(route);
            let layout = s.layout.clone();
            s.composer.popup = Some(Popup::VoiceEngine);
            for now in [1., 2.] {
                s.poll_pages(now);
                assert!(
                    s.fleet_navigation_waiting(),
                    "internal reload discarded the user's target"
                );
                assert_eq!(s.intent, intent);
                assert_eq!(s.route, route);
                assert_eq!(s.layout, layout);
                assert_eq!(s.composer.popup, Some(Popup::VoiceEngine));
            }
            assert!(requests.try_recv().is_err());
            s.apply(Reply {
                intent,
                result: Ok(Update::Sessions {
                    profile: "archive".into(),
                    sessions: vec![],
                }),
            });
            assert!(s.poll_fleet_destination());
            let history = requests
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            assert!(
                matches!(history.command,Command::History(ref profile,ref id) if profile=="archive" && id=="captured-session")
            );
            assert_eq!(s.route, crate::pages::Route::Chat);
            assert_eq!(s.profile, "archive");
            assert_eq!(s.selected_session.as_deref(), Some("captured-session"));
            s.apply(Reply {
                intent: history.intent,
                result: Ok(Update::Opened {
                    profile: "archive".into(),
                    session: "captured-session".into(),
                    sessions: vec![],
                    history: History {
                        messages: vec![HistoryMessage {
                            role: "assistant".into(),
                            content: "Captured target history".into(),
                            raw: serde_json::Value::Null,
                        }],
                        has_more: false,
                        cursor: 1,
                    },
                }),
            });
            assert_eq!(s.messages[0].content, "Captured target history");
            assert!(!s.fleet_navigation_waiting());
        }
    }
    #[test]
    fn explicit_page_navigation_still_cancels_a_pending_session_after_internal_reload() {
        let (mut s, requests, intent) = pending_page_session(crate::pages::Route::Settings);
        s.poll_pages(1.);
        assert!(s.fleet_navigation_waiting());
        s.open_route(crate::pages::Route::Cron);
        assert!(!s.fleet_navigation_waiting());
        s.apply(Reply {
            intent,
            result: Ok(Update::Sessions {
                profile: "archive".into(),
                sessions: vec![],
            }),
        });
        assert!(!s.poll_fleet_destination());
        assert!(
            requests.try_recv().is_err(),
            "a superseded session must not issue its history read"
        );
        assert_eq!(s.route, crate::pages::Route::Cron);
        assert!(s.selected_session.is_none());
    }
    #[test]
    fn newer_typing_or_same_owner_explicit_pick_cancels_waiting_destination() {
        for explicit in [false, true] {
            let (mut s, requests) = viewer();
            let version = s.fleet_active_version().unwrap();
            s.begin_fleet_navigation(version, "archive".into(), Destination::Bot);
            let requested = requests.recv().unwrap();
            if explicit {
                s.select_profile("archive".into());
            } else {
                let scope = s.composer_scope();
                s.composer
                    .draft_mut(scope)
                    .text
                    .replace_text("Newer archive input");
            }
            s.apply(Reply {
                intent: requested.intent,
                result: Ok(Update::Sessions {
                    profile: "archive".into(),
                    sessions: vec![],
                }),
            });
            s.poll_fleet_destination();
            assert!(!s.fleet_navigation_waiting());
            assert!(
                requests.try_recv().is_err(),
                "a stale intent cannot open Bot Chat"
            );
            if !explicit {
                assert_eq!(
                    s.composer.draft(&s.composer_scope()).unwrap().text.text,
                    "Newer archive input"
                );
            }
        }
    }
    #[test]
    fn stale_clicked_saved_revision_is_rejected_before_any_route_or_gateway_work() {
        let (mut s, requests) = viewer();
        s.connections.fixture();
        let row = &s.connections.rows[0];
        let version = SourceVersion {
            id: SourceId::Saved(row.id.clone()),
            revision: "retired".into(),
        };
        assert!(
            s.queue_fleet_route(version, "archive".into(), Destination::Bot)
                .unwrap_err()
                .contains("changed")
        );
        assert!(!s.device_route_pending());
        assert!(!s.fleet_navigation_waiting());
        assert!(requests.try_recv().is_err());
        assert_eq!(s.profile, "work");
        assert_eq!(s.selected_session.as_deref(), Some("stored-current"));
    }
    #[test]
    fn at_rest_direct_activation_probes_its_captured_binding_and_never_the_current_source() {
        use std::{
            net::TcpListener,
            thread,
            time::{Duration, Instant},
        };
        use tungstenite::{
            Message,
            handshake::server::{Request as WsRequest, Response},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut ws =
                tungstenite::accept_hdr(stream, |request: &WsRequest, response: Response| {
                    let uri = url::Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
                    assert_eq!(uri.path(), "/api/ws");
                    assert_eq!(
                        uri.query_pairs().find(|(k, _)| k == "token").unwrap().1,
                        "captured-fixture-secret"
                    );
                    Ok(response)
                })
                .unwrap();
            let request: serde_json::Value =
                serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(request["method"], "profiles.list");
            ws.send(Message::Text(serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":{"profiles":[{"name":"archive"}]}}).to_string().into())).unwrap();
        });
        let (mut s, requests) = viewer();
        let captured = Connection::Url {
            endpoint: endpoint.clone(),
            token: Some("captured-fixture-secret".into()),
        };
        let binding = Binding::new(captured, None).unwrap();
        let version = binding.stamp().version().clone();
        let roster = [Profile {
            name: "archive".into(),
            label: "Archive".into(),
            description: String::new(),
            canonical_session: None,
        }];
        s.fleet.seed_current(binding, &roster).unwrap();
        assert_ne!(s.fleet_active_version(), Some(version.clone()));
        s.begin_fleet_navigation(version, "archive".into(), Destination::Bot);
        assert!(s.device_route_pending());
        let deadline = Instant::now() + Duration::from_secs(3);
        while s.device_route_pending() && Instant::now() < deadline {
            s.poll_device_routes(&mut Tx::default());
            thread::sleep(Duration::from_millis(5));
        }
        let accepted = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            matches!(accepted.command,Command::ConnectExact(Connection::Url{endpoint:ref actual,token:Some(ref token)},ref profile)if actual==&endpoint&&token=="captured-fixture-secret"&&profile=="archive")
        );
        assert!(s.fleet_navigation_waiting());
        assert_eq!(s.profile, "archive");
        assert!(
            requests.try_recv().is_err(),
            "Bot read waits for the final exact connection receipt"
        );
        server.join().unwrap();
    }
}
