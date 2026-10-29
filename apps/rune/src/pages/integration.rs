use crate::app::Viewer;
use crate::pages::*;
use serde_json::Value;
impl Viewer {
    pub fn page_request(&mut self, operation: &str, request: PageRequest) {
        let error = if self.fixture {
            Some("Backend operations are disabled in synthetic scenes".into())
        } else if let Some(connection) = self.connection.clone() {
            self.page_network
                .request(&self.composer_scope(), connection, operation, request)
                .err()
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
        if self.connections.busy && route != Route::Chat {
            self.client_error = Some("Wait for the connection attempt to finish".into());
            return;
        }
        self.ensure_page_owner();
        if self.route == Route::Settings && route != Route::Settings {
            self.settings.vault.dismiss();
        }
        self.layout.open("workspace");
        self.route = route;
        self.settings_local_navigation();
        self.composer.close();
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
            } else {
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
        let effects = if operation.starts_with("chrome.") {
            self.chrome.apply(operation, result)
        } else if operation == "terminal.processes" {
            if let Some(owner) = self.process_owner.take() {
                if let Ok(value) = result {
                    self.terminal.sync_processes(&owner, &value);
                }
            }
            vec![]
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
        self.settings_local_navigation();
        self.page_requests(effects);
        if self.messaging.take_reconnect_followthrough() {
            self.reconnect();
        }
        if self.operations.take_reconnect_followthrough() {
            self.reconnect();
        }
    }
}

impl Viewer {
    pub fn ensure_page_owner(&mut self) -> bool {
        let owner = (self.composer_scope().connection, self.profile.clone());
        if self.page_owner.as_ref() == Some(&owner) {
            return false;
        }
        if let (Some((epoch, profile)), Some(connection)) =
            (self.page_owner.clone(), self.page_connection.clone())
        {
            let scope = crate::composer::DraftScope {
                connection: epoch,
                profile: profile.clone(),
                session: None,
            };
            for (op, mut request) in self.settings.cancel_owned() {
                if request.profile.is_none() {
                    request = request.profile(&profile);
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
        self.page_owner = Some(owner);
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
            if !self.fixture {
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
        } else if self.connection.is_some() {
            let effects = self.identities.bind(self.composer_scope().connection);
            self.page_requests(effects);
            let effects = self.chrome.load();
            self.page_requests(effects);
            let effects = self.operations.check_health();
            self.page_requests(effects);
            let effects = self.sidebar_state.load(&self.profile);
            self.page_requests(effects);
        }
        true
    }
    pub fn poll_pages(&mut self, now: f64) -> bool {
        let mut changed = self.ensure_page_owner();
        if changed && self.route != Route::Chat {
            self.open_route(self.route);
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
        if !self.fixture {
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
        self.route = Route::Chat;
        self.open_target(
            super::ConversationTarget::Session { profile, id, title },
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
