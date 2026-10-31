use crate::app::Viewer;
use crate::pages::*;
use serde_json::Value;
impl Viewer {
    pub fn page_request(&mut self, operation: &str, request: PageRequest) {
        let error = if self.fixture {
            Some("Backend operations are disabled in synthetic scenes".into())
        } else if self.connection.is_some() && !self.page_owner_ready() {
            Some("Wait for the selected agent and profile to finish connecting".into())
        } else if let Some(connection) = self.connection.clone() {
            let request = if request.provenance.is_none()
                && request.profile.is_none()
                && request.connection.is_none()
                && request.source_lease.is_none()
                && self.profile_intent == crate::source::ProfileIntent::NativeSelf
            {
                request.provenance(crate::gateway::prepared::ProfileProvenance::NativeSelf)
            } else {
                request
            };
            let request = if request.connection.is_none()
                && request.source_lease.is_none()
                && (matches!(connection, crate::gateway::Connection::Saved { .. })
                    || connection.route_owner().is_some())
            {
                self.current_source_binding()
                    .and_then(|binding| binding.request(request))
            } else {
                Ok(request)
            };
            match request {
                Ok(request) => self
                    .page_network
                    .request(&self.composer_scope(), connection, operation, request)
                    .err(),
                Err(error) => Some(error),
            }
        } else {
            Some("Connect a Hermes gateway before using this page".into())
        };
        if let Some(error) = error {
            self.apply_page_result(operation, Err(error));
        }
    }
    pub fn page_requests(&mut self, effects: Vec<PageEffect>) {
        for (op, request) in effects {
            self.page_request(&op, request);
        }
    }
    pub fn invalidate_page_requests(&mut self, prefix: &str) {
        self.page_network.invalidate_prefix(prefix);
    }
    pub fn open_route(&mut self, route: Route) {
        self.cancel_device_route();
        if self.connections.busy && route != Route::Chat {
            self.client_error = Some("Wait for the connection attempt to finish".into());
            return;
        }
        if self.route != route {
            self.retire_chat_search_reveals();
        }
        self.ensure_page_owner();
        if self.route == Route::Settings && route != Route::Settings {
            self.settings.vault.dismiss();
        }
        self.layout.open("workspace");
        self.route = route;
        self.settings_local_navigation();
        self.composer.close();
        self.reload_page_route(route);
    }
    fn reload_page_route(&mut self, route: Route) {
        if route != Route::Chat {
            if self.fixture {
                match route {
                    Route::CommandCenter => self.operations.fixture(),
                    Route::Settings => self.settings.fixture(),
                    Route::Bots => self.bots.fixture(),
                    Route::Connections => self.connections.fixture(),
                    Route::Capabilities => {
                        self.capabilities = crate::pages::capabilities::Capabilities::fixture()
                    }
                    Route::Cron => self.cron.fixture(),
                    Route::Messaging => self.messaging.fixture(),
                    Route::Artifacts => self.artifacts.fixture(),
                    _ => {}
                }
            } else if self.page_owner_ready() {
                let requests = match route {
                    Route::CommandCenter => self.operations.load(),
                    Route::Settings => self.settings.load(),
                    Route::Bots => self.bots.load(),
                    Route::Capabilities => self.capabilities.load(),
                    Route::Cron => self.cron.load(),
                    Route::Messaging => self.messaging_load(),
                    Route::Artifacts => self.artifacts.load(),
                    _ => vec![],
                };
                self.page_requests(requests);
            }
        }
    }
    pub fn apply_page_result(&mut self, operation: &str, result: Result<Value, String>) {
        if operation == "files.cwd" {
            if let (Some(owner), Ok(v)) = (&self.files.owner, &result) {
                if let Some(cwd) = v["cwd"].as_str() {
                    self.workspace_cwds.insert(owner.clone(), cwd.into());
                }
            }
        }
        let successful = result.is_ok();
        let effects = if operation.starts_with("client-display.") {
            self.client_settings.apply(operation, result);
            self.refresh_chat_search();
            vec![]
        } else if operation.starts_with("chrome.") {
            self.chrome.apply(operation, result)
        } else if operation.starts_with("ops.") {
            self.operations.apply(operation, result)
        } else if operation.starts_with("identities.") {
            self.identities.apply(operation, result)
        } else if operation.starts_with("directories.") {
            self.directories.apply(operation, result)
        } else if operation.starts_with("review.") {
            self.review.apply(operation, result)
        } else if operation.starts_with("files.") {
            self.files.apply(operation, result)
        } else if operation.starts_with("bots.") {
            let effects = self.bots.apply(operation, result);
            self.reconcile_bots();
            effects
        } else if operation.starts_with("plugin-host.") {
            self.plugin_hosts.apply(operation, result);
            vec![]
        } else if operation.starts_with("sidebar.") {
            self.sidebar_state.apply(operation, result)
        } else if operation.starts_with("settings.") {
            self.settings.apply(operation, result)
        } else if operation.starts_with("capabilities.") {
            self.capabilities.apply(operation, result)
        } else if operation.starts_with("cron.") {
            self.cron.apply(operation, result)
        } else if operation.starts_with("messaging.") {
            self.messaging.apply(operation, result)
        } else if operation.starts_with("artifacts.") {
            self.artifacts.apply(operation, result)
        } else {
            vec![]
        };
        if successful
            && matches!(operation, "settings.config" | "settings.readback")
            && self
                .settings
                .scope_profile
                .as_ref()
                .is_none_or(|p| p == &self.profile)
        {
            self.client_settings
                .apply_config(self.settings.confirmed_config());
        }
        self.settings_local_navigation();
        if operation.starts_with("directories.") {
            match self.directories.purpose {
                crate::directories::Purpose::Projects(..) => {
                    self.project_directory_requests(effects)
                }
                crate::directories::Purpose::ProjectFolders(..) => {
                    self.project_folders_directory_requests(effects)
                }
                _ => self.page_requests(effects),
            }
        } else {
            self.page_requests(effects);
        }
        if self.messaging.take_reconnect_followthrough() {
            self.reconnect();
        }
        if self.operations.take_reconnect_followthrough() {
            self.reconnect();
        }
    }
}

impl Viewer {
    pub(crate) fn page_connection_accepted(&self) -> bool {
        self.connection.is_some()
            && !self.loading
            && self.page_accepted_owner.as_ref()
                == Some(&(
                    self.composer_scope().connection,
                    self.profile.clone(),
                    self.profile_intent,
                ))
    }
    fn page_owner_ready(&self) -> bool {
        self.fixture
            || (self.page_connection_accepted()
                && self.page_loaded_owner.as_ref()
                    == Some(&(
                        self.composer_scope().connection,
                        self.profile.clone(),
                        self.profile_intent,
                    )))
    }
    pub fn ensure_page_owner(&mut self) -> bool {
        let owner = (
            self.composer_scope().connection,
            self.profile.clone(),
            self.profile_intent,
        );
        let changed = self.page_owner.as_ref() != Some(&owner);
        if changed {
            if let (Some((epoch, profile, profile_intent)), Some(connection)) =
                (self.page_owner.clone(), self.page_connection.clone())
            {
                let scope = crate::composer::DraftScope {
                    draft_origin: None,
                    profile_intent,
                    connection: epoch,
                    profile: profile.clone(),
                    session: None,
                };
                for (op, mut request) in self.settings.cancel_owned() {
                    if request.profile.is_none() && request.provenance.is_none() {
                        request = if profile_intent == crate::source::ProfileIntent::NativeSelf {
                            request
                                .provenance(crate::gateway::prepared::ProfileProvenance::NativeSelf)
                        } else {
                            request.profile(&profile)
                        };
                    }
                    if self
                        .page_network
                        .request(
                            &scope,
                            connection.clone(),
                            &format!("cleanup.{op}"),
                            request,
                        )
                        .is_err()
                    {
                        self.client_error =
                            Some("The previous sign-in cleanup could not be queued".into());
                    }
                }
            }
            self.page_owner = Some(owner.clone());
            self.page_loaded_owner = None;
            self.page_connection = self.connection.clone();
            self.page_network.invalidate();
            self.settings = crate::pages::settings::Settings::new();
            self.files.reset();
            self.restore_profile_appearance();
            self.review.reset();
            self.review.hydrate_preferences(&self.review_preferences);
            self.tool_owner = None;
            self.operations.reset();
            self.chrome.reset();
            self.workspace_menu = false;
            self.directories = Default::default();
            self.health_open = false;
            self.bots.reset();
            self.bots_revision = 0;
            self.capabilities = crate::pages::capabilities::Capabilities::new();
            self.cron = crate::pages::cron::Cron::new();
            self.messaging.reset();
            self.artifacts.reset();
            self.sidebar_state.reset_data();
            if let Some(connection) = &self.connection {
                self.sidebar_state
                    .bind_connection(connection, self.composer_scope().connection);
                if !self.fixture && self.draft_storage_available {
                    if let Err(error) = self
                        .composer
                        .bind_storage(connection, self.composer_scope().connection)
                    {
                        self.client_error = Some(error);
                    }
                }
            }
            if self.fixture {
                self.chrome.fixture();
                self.seed_sidebar_fixture();
                self.page_loaded_owner = Some(owner.clone());
            }
        }
        // Installing a route does not prove that the agent still serves its profile.
        // The foreground worker's correlated receipt enables the deferred reads.
        if !self.fixture
            && self.page_connection_accepted()
            && self.page_loaded_owner.as_ref() != Some(&owner)
        {
            self.page_loaded_owner = Some(owner);
            let effects = self.client_settings.bind(self.composer_scope());
            self.page_requests(effects);
            let effects = self.identities.bind(self.composer_scope().connection);
            self.page_requests(effects);
            let effects = self.chrome.load();
            self.page_requests(effects);
            let effects = self.operations.check_health();
            self.page_requests(effects);
            let effects = self.sidebar_state.load(&self.profile);
            self.page_requests(effects);
            return true;
        }
        changed
    }
    pub fn poll_pages(&mut self, now: f64) -> bool {
        let mut changed = self.ensure_page_owner();
        if changed && self.route != Route::Chat {
            self.reload_page_route(self.route);
        }
        let replies = self.page_network.poll(&self.composer_scope());
        changed |= !replies.is_empty();
        for reply in replies {
            let operation = if reply.progress {
                format!("{}.event", reply.operation)
            } else {
                reply.operation
            };
            self.apply_page_result(&operation, reply.result);
        }
        if !self.fixture && self.page_owner_ready() {
            if self.connection.is_some() {
                let effects = self.client_settings.poll(now);
                changed |= !effects.is_empty();
                self.page_requests(effects);
            }
            if self.connection.is_some() && self.sidebar.is_open() {
                let effects = self.sidebar_state.tick(now);
                changed |= !effects.is_empty();
                self.page_requests(effects);
            }
            let effects = self.bots.poll(now);
            changed |= !effects.is_empty() | self.bots.take_redraw();
            self.page_requests(effects);
            let effects = self.chrome.poll(
                now,
                self.show_statusbar && !self.simple_mode && self.status_item_visible("resources"),
            );
            changed |= !effects.is_empty();
            self.page_requests(effects);
            let effects = self.messaging.poll(now);
            changed |= !effects.is_empty() | self.messaging.take_redraw();
            self.page_requests(effects);
            if self.messaging.take_reconnect_followthrough() {
                self.reconnect();
            }
            let effects = self.operations.poll(now);
            changed |= !effects.is_empty() | self.operations.take_redraw();
            self.page_requests(effects);
            let effects = self.settings.poll(now);
            changed |= !effects.is_empty();
            self.page_requests(effects);
            if self.route == Route::Capabilities && self.capabilities.needs_poll() {
                let effects = self.capabilities.tick(now);
                changed |= !effects.is_empty();
                self.page_requests(effects);
            }
            if self.route == Route::Cron {
                let effects = self.cron.tick();
                changed |= !effects.is_empty();
                self.page_requests(effects);
            }
        }
        changed
    }
    pub fn open_page_session(&mut self, profile: String, id: String, title: String) {
        self.open_page_session_owned(profile, id, title, crate::source::ProfileIntent::Backend);
    }
    pub(in crate::app) fn open_page_session_owned(
        &mut self,
        profile: String,
        id: String,
        title: String,
        profile_intent: crate::source::ProfileIntent,
    ) {
        self.route = Route::Chat;
        self.open_target(
            super::ConversationTarget::Session {
                profile,
                id,
                title,
                profile_intent,
            },
            false,
        );
    }
}

impl Viewer {
    pub fn allow_owner_change(&mut self, profile: &str) -> bool {
        if profile == self.profile {
            return true;
        }
        let blocked = self
            .conversation
            .blocks_profile_retirement(self.composer_scope().connection, None)
            || self
                .chat
                .blocks_profile_retirement(self.composer_scope().connection, None)
            || self.review.blocks_owner_change()
            || self.chrome.writing
            || self.operations.blocks_owner_change()
            || self.files.blocks_owner_change()
            || self.connections.busy
            || self.bots.blocks_owner_change()
            || self.sidebar_state.blocks_owner_change()
            || self.settings.blocks_owner_change()
            || self.messaging.blocks_owner_change()
            || self.artifacts.blocks_owner_change()
            || self.capabilities.busy();
        if blocked {
            self.client_error=Some("Finish or reconcile the pending settings/session change before switching profiles or gateways".into());
            false
        } else {
            true
        }
    }
    fn seed_sidebar_fixture(&mut self) {
        self.sidebar_state.fixture();
        self.sidebar_state.owner_profile = self.profile.clone();
        let rows=self.sessions.iter().enumerate().map(|(index,session)|serde_json::json!({"id":session.id,"profile":self.profile,"title":session.title,"source":"desktop","last_active":self.sidebar_state.now-(index as f64+1.0)*3600.0,"message_count":8,"pinned":index==0})).collect::<Vec<_>>();
        if !rows.is_empty() {
            let _ = self
                .sidebar_state
                .apply("sidebar.recents", Ok(serde_json::json!({"sessions":rows})));
        }
    }
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    use crate::app::{Profile, Reply, Update};
    use std::{
        io::{BufRead, BufReader, Write},
        net::{TcpListener, TcpStream},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    struct Wire {
        endpoint: String,
        calls: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
        directory: std::path::PathBuf,
    }
    impl Wire {
        fn new() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let calls = Arc::new(Mutex::new(vec![]));
            let stop = Arc::new(AtomicBool::new(false));
            let recorded = calls.clone();
            let stopping = stop.clone();
            let worker = thread::spawn(move || {
                let mut workers = vec![];
                while !stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let recorded = recorded.clone();
                            workers.push(thread::spawn(move || serve(stream, recorded)));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2))
                        }
                        Err(error) => panic!("private page fixture: {error}"),
                    }
                }
                for worker in workers {
                    worker.join().unwrap();
                }
            });
            Self {
                endpoint,
                calls,
                stop,
                worker: Some(worker),
                directory: std::env::temp_dir()
                    .join(format!("hermes-page-ready-{}", uuid::Uuid::new_v4())),
            }
        }
        fn viewer(&self) -> Viewer {
            let mut s = Viewer::empty();
            s.fixture = false;
            s.connection = Some(crate::gateway::Connection::Url {
                endpoint: self.endpoint.clone(),
                token: None,
            });
            s.connection_epoch = 42;
            s.profile = "research".into();
            s.status = "Connecting…".into();
            s.loading = true;
            s.draft_storage_available = false;
            s.sidebar_state.bind_connection_at(
                s.connection.as_ref().unwrap(),
                42,
                self.directory.clone(),
            );
            s.page_network.init(Arc::new(|| {}));
            s
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }
    impl Drop for Wire {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let result = self.worker.take().unwrap().join();
            if !thread::panicking() {
                result.unwrap();
            }
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
    fn serve(mut stream: TcpStream, calls: Arc<Mutex<Vec<String>>>) {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buffer = [0; 1024];
        let size = stream.peek(&mut buffer).unwrap();
        if String::from_utf8_lossy(&buffer[..size]).contains("Upgrade: websocket") {
            let Ok(mut socket) = tungstenite::accept(stream) else {
                return;
            };
            if let Ok(tungstenite::Message::Text(text)) = socket.read() {
                let frame: Value = serde_json::from_str(text.as_ref()).unwrap();
                let method = frame["method"].as_str().unwrap();
                assert_eq!(frame["params"]["profile"], "research");
                calls.lock().unwrap().push(format!("RPC {method}"));
                let result = match method {
                    "profiles.list" => serde_json::json!({"profiles":[]}),
                    "config.get" => serde_json::json!({"value":"smart"}),
                    "session.list" => {
                        assert_eq!(frame["params"]["scope"], "live");
                        serde_json::json!({"sessions":[]})
                    }
                    "setup.status" | "setup.runtime_check" => {
                        serde_json::json!({"ok":true,"ready":true})
                    }
                    _ => panic!("unexpected page RPC {method}"),
                };
                let _ = socket.send(tungstenite::Message::Text(
                    serde_json::json!({"jsonrpc":"2.0","id":frame["id"],"result":result})
                        .to_string()
                        .into(),
                ));
            }
            return;
        }
        let mut reader = BufReader::new(&mut stream);
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        let target = first.split_whitespace().nth(1).unwrap();
        assert!(first.starts_with("GET "));
        let url = url::Url::parse(&format!("http://fixture{target}")).unwrap();
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "profile")
                .unwrap()
                .1,
            "research"
        );
        calls.lock().unwrap().push(format!("GET {}", url.path()));
        let result = match url.path() {
            "/api/config" => serde_json::json!({"config":{"approvals":{"mode":"smart"}}}),
            "/api/status" => serde_json::json!({"running":true,"ready":true}),
            "/api/profiles/sessions/sidebar" => serde_json::json!({"recents":{"sessions":[{"id":"notes","profile":"research","title":"Verified profile notes","source":"desktop","message_count":1}]},"messaging":{"sessions":[]},"cron":{"sessions":[]}}),
            "/api/cron/jobs" => serde_json::json!({"jobs":[]}),
            "/api/cron/blueprints" => serde_json::json!({"blueprints":[]}),
            "/api/cron/delivery-targets" => serde_json::json!({"targets":[]}),
            "/api/model/options" => serde_json::json!({"provider":"fixture","model":"fixture-model","providers":[]}),
            _ => panic!("unexpected page path {}", url.path()),
        }.to_string();
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{result}",result.len()).unwrap();
    }
    fn accepted(s: &mut Viewer) {
        s.apply(Reply {
            intent: s.intent,
            result: Ok(Update::Connected {
                profiles: vec![Profile {
                    name: "research".into(),
                    label: "Research".into(),
                    description: String::new(),
                    canonical_session: None,
                }],
                profile: "research".into(),
                sessions: vec![],
            }),
        });
    }
    #[test]
    fn connecting_owner_defers_wire_reads_and_initializes_same_owner_after_acceptance() {
        let wire = Wire::new();
        let mut s = wire.viewer();
        s.open_route(Route::Cron);
        for now in [1., 20., 60.] {
            s.poll_pages(now);
        }
        thread::sleep(Duration::from_millis(150));
        assert!(
            wire.calls().is_empty(),
            "Unaccepted source/profile reached page wire: {:?}",
            wire.calls()
        );
        assert_eq!(
            s.page_owner,
            Some((42, "research".into(), crate::source::ProfileIntent::Backend))
        );
        accepted(&mut s);
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            s.poll_pages(61.);
            if s.sidebar_state.find("research", "notes").is_some()
                && wire.calls().contains(&"RPC session.list".into())
                && wire.calls().contains(&"GET /api/model/options".into())
            {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            s.sidebar_state.find("research", "notes").is_some(),
            "Same owner never initialized after accepted receipt: {:?}",
            wire.calls()
        );
        assert!(wire.calls().contains(&"GET /api/config".into()));
        assert!(wire.calls().contains(&"RPC session.list".into()));
        assert!(s.route == Route::Cron);
        assert!(
            wire.calls().contains(&"GET /api/model/options".into()),
            "Deferred current route did not load after acceptance"
        );
        let count = wire
            .calls()
            .iter()
            .filter(|path| path.as_str() == "GET /api/config")
            .count();
        s.history_loading = true;
        for _ in 0..5 {
            s.poll_pages(61.);
        }
        thread::sleep(Duration::from_millis(50));
        assert_eq!(
            wire.calls()
                .iter()
                .filter(|path| path.as_str() == "GET /api/config")
                .count(),
            count,
            "Accepted owner initialized more than once"
        );
        // A history failure does not revoke an already accepted agent/profile.
        s.apply(Reply {
            intent: s.intent,
            result: Err("History is temporarily unavailable".into()),
        });
        s.page_request("client-display.config", PageRequest::get("/api/config"));
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until
            && wire
                .calls()
                .iter()
                .filter(|path| path.as_str() == "GET /api/config")
                .count()
                == count
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            wire.calls()
                .iter()
                .filter(|path| path.as_str() == "GET /api/config")
                .count(),
            count + 1,
            "History error blocked the accepted owner's settings"
        );
    }
    #[test]
    fn rejected_exact_owner_blocks_automatic_and_explicit_page_wire() {
        let wire = Wire::new();
        let mut s = wire.viewer();
        s.apply(Reply {
            intent: s.intent,
            result: Err("The requested profile is no longer served by this agent".into()),
        });
        s.open_route(Route::Cron);
        for now in [1., 20., 60.] {
            s.poll_pages(now);
        }
        s.page_request("client-display.config", PageRequest::get("/api/config"));
        thread::sleep(Duration::from_millis(150));
        assert!(
            wire.calls().is_empty(),
            "Rejected exact source/profile reached page wire: {:?}",
            wire.calls()
        );
        assert_eq!(
            s.error.as_deref(),
            Some("The requested profile is no longer served by this agent")
        );
        assert!(s.sidebar_state.find("research", "notes").is_none());
    }
    #[test]
    fn page_acceptance_does_not_cross_equal_named_profile_intents() {
        let wire = Wire::new();
        let mut s = wire.viewer();
        accepted(&mut s);
        assert!(s.page_connection_accepted());
        s.ensure_page_owner();
        s.profile_intent = crate::source::ProfileIntent::NativeSelf;
        assert!(!s.page_connection_accepted());
        assert!(s.ensure_page_owner());
        assert!(
            !s.can_remember_profile_preferences()
                || s.page_owner.as_ref().unwrap().2 == s.profile_intent
        );
        assert!(wire.calls().is_empty());
    }
}
