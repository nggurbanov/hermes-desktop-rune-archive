//! Native parent for trusted installed JS plugin surfaces.
use super::*;
use serde_json::{Value, json};
impl Viewer {
    fn plugin_snapshot(&self) -> Value {
        let view = self.conversation.view(&self.composer_scope());
        json!({"sound_muted":self.sounds.muted(),"sound_preset":self.sounds.prefs.preset,"profiles_loaded":!self.loading,"profile":self.profile,"profiles":self.profiles.iter().map(|p|json!({"name":p.name,"display_name":p.label,"description":p.description})).collect::<Vec<_>>(),"session":self.selected_session,"runtime_session":view.map(|v|v.runtime.clone()),"cwd":Value::Null,"model":self.composer.draft(&self.composer_scope()).and_then(|d|d.model.as_ref()).map(|m|m.id.clone()),"busy":self.chat_running()})
    }
    pub fn open_gui_plugin(&mut self, source: String, origin: String) {
        self.open_gui_plugin_for(None, source, origin);
    }
    pub fn open_gui_plugin_for(&mut self, profile: Option<String>, source: String, origin: String) {
        if self.fixture {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let scope = self.composer_scope();
        let mut owner = self.plugin_snapshot();
        if let Some(profile) = profile {
            owner["target_profile"] = json!(profile);
            owner["profile"] = json!(profile);
            if profile != self.profile {
                for key in ["session", "runtime_session", "cwd", "model"] {
                    owner[key] = Value::Null;
                }
            }
        }
        self.plugin_hosts.set_resolver(self.connections.resolver());
        let previous = self.plugin_hosts.window_token();
        let result = self.plugin_hosts.launch(
            source,
            origin,
            scope,
            &connection,
            self.connections.active_lease(),
            owner,
        );
        if previous != self.plugin_hosts.window_token() {
            self.invalidate_page_requests("plugin-host.");
        }
        if let Err(error) = result {
            self.client_error = Some(error);
        }
    }
    pub fn poll_plugin_hosts(&mut self, tx: &mut Tx) -> bool {
        if self.fixture {
            return false;
        }
        let Some(connection) = self.connection.clone() else {
            self.plugin_hosts.close();
            return false;
        };
        for answer in self.plugin_dialogs.poll() {
            self.plugin_hosts
                .reply_owned(&answer.token, &answer.id, answer.result);
        }
        for (id, owned) in self.sounds.take_claims() {
            self.plugin_hosts.ambient_reply(id, owned);
        }
        for delivery in self.notifications.take_plugin_deliveries() {
            self.plugin_hosts
                .reply_notification(&delivery.target, delivery.result);
        }
        for target in self.notifications.take_plugin_closed() {
            self.plugin_hosts.clear_notification(&target);
        }
        self.poll_plugin_navigation();
        self.plugin_hosts.sync(self.plugin_snapshot());
        let effects = self.plugin_hosts.poll(&self.composer_scope(), &connection);
        let mut changed = !effects.is_empty();
        self.page_requests(effects);
        for action in self.plugin_hosts.take_actions() {
            changed = true;
            if matches!(action.kind.as_str(), "select-paths" | "select-save-path") {
                if let Some(token) = self.plugin_hosts.window_token() {
                    if let Err(error) = self.plugin_dialogs.open(
                        token,
                        action.id.clone(),
                        action.kind == "select-save-path",
                        action.payload.clone(),
                    ) {
                        self.plugin_hosts.reply(&action.id, Err(error));
                    }
                }
                continue;
            }
            if matches!(
                action.kind.as_str(),
                "open-session" | "new-chat" | "ensure-agent"
            ) {
                self.plugin_navigate(action);
                continue;
            }
            if action.kind == "claim-ambient-cue" {
                match self.plugin_hosts.ambient_target(&action) {
                    Ok(target) => match self.sounds.claim_ambient(
                        &target.principal,
                        &target.profile,
                        &target.key,
                    ) {
                        Ok(id) => self.plugin_hosts.queue_ambient(id, target),
                        Err(_) => self.plugin_hosts.reply(&action.id, Ok(Value::Bool(false))),
                    },
                    Err(_) => self.plugin_hosts.reply(&action.id, Ok(Value::Bool(false))),
                }
                continue;
            }
            if action.kind == "native-notify" {
                match self.plugin_hosts.prepare_native_notification(&action) {
                    Ok(Some(notice)) => {
                        if self.notifications.dispatch_plugin(notice).is_none() {
                            self.plugin_hosts.reply(&action.id, Ok(Value::Bool(false)));
                        }
                    }
                    Ok(None) => self.plugin_hosts.reply(&action.id, Ok(Value::Bool(false))),
                    Err(error) => self.plugin_hosts.reply(&action.id, Err(error)),
                }
                continue;
            }
            let result = match action.kind.as_str() {
                "open-external" => {
                    let value = action
                        .payload
                        .as_str()
                        .or(action.payload["url"].as_str())
                        .unwrap_or("");
                    if url::Url::parse(value)
                        .is_ok_and(|u| matches!(u.scheme(), "http" | "https" | "mailto"))
                    {
                        tx.open_url(value)
                            .map(|_| json!({"ok":true}))
                            .map_err(|_| "The URL could not be opened".into())
                    } else {
                        Err("Unsupported external URL".into())
                    }
                }
                "clipboard" => {
                    let value = action
                        .payload
                        .as_str()
                        .or(action.payload["text"].as_str())
                        .unwrap_or("");
                    tx.copy(value.to_owned());
                    Ok(json!({"ok":true}))
                }
                "notify" => {
                    self.client_notice = action.payload["message"]
                        .as_str()
                        .or(action.payload["title"].as_str())
                        .map(|v| v.chars().take(1000).collect());
                    Ok(json!({"ok":true}))
                }
                "navigate" => {
                    let path = action.payload.as_str().unwrap_or("");
                    let route = match path.split('?').next().unwrap_or(path) {
                        "/settings" => Some(crate::pages::Route::Settings),
                        "/bots" => Some(crate::pages::Route::Bots),
                        "/connections" => Some(crate::pages::Route::Connections),
                        "/command-center" => Some(crate::pages::Route::CommandCenter),
                        "/capabilities" => Some(crate::pages::Route::Capabilities),
                        "/messaging" => Some(crate::pages::Route::Messaging),
                        "/artifacts" => Some(crate::pages::Route::Artifacts),
                        "/cron" => Some(crate::pages::Route::Cron),
                        "/" | "/chat" => Some(crate::pages::Route::Chat),
                        _ => None,
                    };
                    if let Some(route) = route {
                        self.open_route(route);
                        Ok(json!({"ok":true}))
                    } else {
                        Err("This plugin navigation target is unavailable".into())
                    }
                }
                "reveal-path" => {
                    let path = action
                        .payload
                        .as_str()
                        .or(action.payload["path"].as_str())
                        .unwrap_or("");
                    let path = PathBuf::from(path);
                    if self.is_local_connection() && path.is_absolute() && path.exists() {
                        let target = if path.is_dir() {
                            path
                        } else {
                            path.parent().unwrap().to_path_buf()
                        };
                        let url = url::Url::from_file_path(target)
                            .map_err(|_| "The selected folder could not be revealed".to_owned());
                        url.and_then(|url| {
                            tx.open_url(url.as_str())
                                .map(|_| json!({"ok":true}))
                                .map_err(|_| "The native host cannot reveal this folder".into())
                        })
                    } else {
                        Err("Reveal requires an existing path on the local agent's device".into())
                    }
                }
                _ => Err("The plugin requested an unavailable native action".into()),
            };
            if !action.id.is_empty() {
                self.plugin_hosts.reply(&action.id, result);
            }
        }
        if let Some(error) = self.plugin_hosts.error.take() {
            self.client_error = Some(error);
            changed = true;
        }
        changed
    }
}

struct DialogAnswer {
    token: String,
    id: String,
    result: Result<Value, String>,
}
pub(super) struct NativeDialogs {
    send: Sender<DialogAnswer>,
    receive: Receiver<DialogAnswer>,
    waker: Option<Waker>,
    busy: bool,
}
impl NativeDialogs {
    pub fn new() -> Self {
        let (send, receive) = mpsc::channel();
        Self {
            send,
            receive,
            waker: None,
            busy: false,
        }
    }
    pub fn init(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }
    fn poll(&mut self) -> Vec<DialogAnswer> {
        let answers: Vec<_> = self.receive.try_iter().collect();
        if !answers.is_empty() {
            self.busy = false;
        }
        answers
    }
    fn open(
        &mut self,
        token: String,
        id: String,
        save: bool,
        options: Value,
    ) -> Result<(), String> {
        if self.busy {
            return Err("Another native path dialog is open".into());
        }
        let mut dialog = rfd::FileDialog::new();
        if let Some(title) = options["title"].as_str() {
            dialog = dialog.set_title(title);
        }
        if let Some(path) = options["defaultPath"].as_str().filter(|p| !p.is_empty()) {
            let path = PathBuf::from(path);
            if path.is_dir() {
                dialog = dialog.set_directory(path);
            } else {
                if let Some(parent) = path.parent() {
                    dialog = dialog.set_directory(parent);
                }
                if let Some(name) = path.file_name() {
                    dialog = dialog.set_file_name(name.to_string_lossy());
                }
            }
        }
        for filter in options["filters"].as_array().into_iter().flatten() {
            let extensions: Vec<_> = filter["extensions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if !extensions.is_empty() {
                dialog = dialog.add_filter(filter["name"].as_str().unwrap_or("Files"), &extensions);
            }
        }
        let directories = options["directory"]
            .as_bool()
            .or(options["directories"].as_bool())
            .unwrap_or(false);
        let multiple = options["multiple"].as_bool().unwrap_or(true);
        let send = self.send.clone();
        let waker = self.waker.clone();
        self.busy = true;
        std::thread::spawn(move || {
            let result = if save {
                json!(dialog.save_file().map(|p| p.to_string_lossy().into_owned()))
            } else {
                let paths = if directories {
                    if multiple {
                        dialog.pick_folders().unwrap_or_default()
                    } else {
                        dialog.pick_folder().into_iter().collect()
                    }
                } else if multiple {
                    dialog.pick_files().unwrap_or_default()
                } else {
                    dialog.pick_file().into_iter().collect()
                };
                json!(
                    paths
                        .into_iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                )
            };
            let _ = send.send(DialogAnswer {
                token,
                id,
                result: Ok(result),
            });
            if let Some(waker) = waker {
                waker();
            }
        });
        Ok(())
    }
}

enum NavigationStage {
    Hydration,
    Resolving(crate::plugin_host::HostAction),
    Activating {
        action: crate::plugin_host::HostAction,
        target: crate::plugin_host::NavigationTarget,
    },
    Profiles {
        action: crate::plugin_host::HostAction,
        target: crate::plugin_host::NavigationTarget,
    },
}
pub(super) struct PluginNavigation {
    stage: NavigationStage,
    token: String,
    id: String,
    owner: DraftScope,
    intent: u64,
    deadline: std::time::Instant,
}
impl Viewer {
    fn plugin_navigate(&mut self, action: crate::plugin_host::HostAction) {
        let Some(token) = self.plugin_hosts.window_token() else {
            return;
        };
        let target = match self.plugin_hosts.navigation_target(&token, &action.payload) {
            Ok(Some(target)) => target,
            Ok(None) => {
                self.plugin_navigation.push(PluginNavigation {
                    stage: NavigationStage::Resolving(action.clone()),
                    token,
                    id: action.id,
                    owner: self.composer_scope(),
                    intent: self.intent,
                    deadline: std::time::Instant::now() + std::time::Duration::from_secs(60),
                });
                return;
            }
            Err(error) => {
                self.plugin_hosts
                    .reply_owned(&token, &action.id, Err(error));
                return;
            }
        };
        let current_key = self
            .connection
            .as_ref()
            .and_then(|c| crate::client::connection_key(c).ok());
        if current_key.as_ref()
            != crate::client::connection_key(&target.connection)
                .ok()
                .as_ref()
        {
            if action.payload["options"]["refreshInPlace"] == true {
                self.plugin_hosts
                    .reply_owned(&token, &action.id, Ok(json!({"ok":true})));
                return;
            }
            if self.plugin_hosts.navigation_busy()
                || self.connections.busy
                || !self.allow_owner_change("")
            {
                self.plugin_hosts.reply_owned(
                    &token,
                    &action.id,
                    Err("Finish the pending change before switching plugin sources".into()),
                );
                return;
            }
            let Some(lease) = target.lease.clone() else {
                self.plugin_hosts.reply_owned(
                    &token,
                    &action.id,
                    Err("The target source has no verified retained lease".into()),
                );
                return;
            };
            self.connections
                .commit_lease_for(lease, target.profile.clone());
            self.plugin_navigation.push(PluginNavigation {
                stage: NavigationStage::Activating {
                    action: action.clone(),
                    target,
                },
                token,
                id: action.id,
                owner: self.composer_scope(),
                intent: self.intent,
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(90),
            });
            return;
        }
        if self.plugin_hosts.navigation_busy()
            && (self.profile != target.profile
                || current_key.as_ref()
                    != crate::client::connection_key(&target.connection)
                        .ok()
                        .as_ref())
        {
            self.plugin_hosts.reply_owned(
                &token,
                &action.id,
                Err(
                    "Finish the current plugin upload or API operation before switching its owner"
                        .into(),
                ),
            );
            return;
        }
        let profile = target.profile;
        let result = (|| {
            let refresh = action.payload["options"]["refreshInPlace"]
                .as_bool()
                .unwrap_or(false);
            let session = action.payload["stored_session_id"]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned);
            if action.kind == "open-session" && session.is_none() {
                return Err("The plugin supplied no stored session".into());
            }
            if refresh {
                if self.profile == profile && self.selected_session == session {
                    self.reconnect_conversation();
                }
                // A plugin background refresh must preserve the foreground route,
                // focused pane and tab, even when its session remains selected.
                return Ok(self.profile == profile
                    && self.selected_session == session
                    && action.kind == "open-session"
                    && action.payload["options"]["awaitHydration"]
                        .as_bool()
                        .unwrap_or(false));
            }
            if !self.allow_owner_change(&profile) {
                return Err("Finish the pending change before switching plugin agents".into());
            }
            if action.kind == "open-session" {
                let id = session.unwrap();
                let title = action.payload["options"]["tabTitle"]
                    .as_str()
                    .unwrap_or("Conversation")
                    .to_owned();
                self.open_target(
                    ConversationTarget::Session {
                        profile: profile.clone(),
                        profile_intent: crate::source::ProfileIntent::Backend,
                        id,
                        title,
                    },
                    action.payload["options"]["intent"].as_str() == Some("tab"),
                );
                if action.payload["options"]["forceResume"]
                    .as_bool()
                    .unwrap_or(false)
                    || refresh
                {
                    self.reconnect_conversation();
                }
            } else {
                self.select_profile(profile.clone());
                if self.profile != profile {
                    return Err("The agent switch was declined".into());
                }
                if action.kind == "new-chat" {
                    self.new_draft_on_current_source();
                }
            }
            let scope = self.composer_scope();
            self.plugin_hosts.adopt_parent_scope(&token, &scope, None)?;
            Ok(action.kind == "open-session"
                && action.payload["options"]["awaitHydration"]
                    .as_bool()
                    .unwrap_or(false))
        })();
        match result {
            Ok(true) => {
                let ms = action.payload["options"]["hydrationTimeoutMs"]
                    .as_u64()
                    .unwrap_or(60000)
                    .clamp(1000, 600000);
                self.plugin_navigation.push(PluginNavigation {
                    stage: NavigationStage::Hydration,
                    token,
                    id: action.id,
                    owner: self.composer_scope(),
                    intent: self.intent,
                    deadline: std::time::Instant::now() + std::time::Duration::from_millis(ms),
                });
            }
            other => {
                self.plugin_hosts
                    .reply_owned(&token, &action.id, other.map(|_| json!({"ok":true})))
            }
        }
    }
    fn poll_plugin_navigation(&mut self) {
        let mut pending = vec![];
        for mut item in std::mem::take(&mut self.plugin_navigation) {
            if self.plugin_hosts.window_token().as_deref() != Some(&item.token) {
                continue;
            }
            if std::time::Instant::now() >= item.deadline {
                self.plugin_hosts.reply_owned(
                    &item.token,
                    &item.id,
                    Err("The plugin navigation did not finish before its deadline".into()),
                );
                continue;
            }
            match item.stage {
                NavigationStage::Resolving(action) => {
                    if item.owner != self.composer_scope() || item.intent != self.intent {
                        self.plugin_hosts.reply_owned(
                            &item.token,
                            &item.id,
                            Err("The source navigation was superseded".into()),
                        );
                        continue;
                    }
                    match self
                        .plugin_hosts
                        .navigation_target(&item.token, &action.payload)
                    {
                        Ok(Some(_)) => self.plugin_navigate(action),
                        Ok(None) => {
                            item.stage = NavigationStage::Resolving(action);
                            pending.push(item);
                        }
                        Err(error) => {
                            self.plugin_hosts
                                .reply_owned(&item.token, &item.id, Err(error))
                        }
                    }
                }
                NavigationStage::Activating { mut action, target } => {
                    let selected = self.connection.as_ref().is_some_and(|c| {
                        crate::client::connection_key(c).ok()
                            == crate::client::connection_key(&target.connection).ok()
                    });
                    if selected && self.composer_scope().connection != item.owner.connection {
                        let connection = self.connection.clone().unwrap();
                        match self.plugin_hosts.adopt_parent_connection(
                            &item.token,
                            &self.composer_scope(),
                            &connection,
                            &target,
                            false,
                        ) {
                            Ok(()) => {
                                action.payload["native_owner"]["lifecycle_key"] =
                                    crate::client::connection_key(&connection).unwrap().into();
                                item.owner = self.composer_scope();
                                item.intent = self.intent;
                                item.stage = NavigationStage::Profiles { action, target };
                                pending.push(item);
                            }
                            Err(error) => {
                                self.plugin_hosts
                                    .reply_owned(&item.token, &item.id, Err(error))
                            }
                        }
                    } else if item.owner != self.composer_scope() || item.intent != self.intent {
                        self.plugin_hosts.reply_owned(
                            &item.token,
                            &item.id,
                            Err("The source navigation was superseded".into()),
                        );
                    } else if !self.connections.busy {
                        self.plugin_hosts.reply_owned(
                            &item.token,
                            &item.id,
                            Err(self
                                .connections
                                .error
                                .clone()
                                .unwrap_or("The source activation was not accepted".into())),
                        );
                    } else {
                        item.stage = NavigationStage::Activating { action, target };
                        pending.push(item);
                    }
                }
                NavigationStage::Profiles { action, target } => {
                    // The owner's profile may be normalized by the successful initial handshake.
                    if item.owner.connection != self.composer_scope().connection
                        || item.intent != self.intent
                    {
                        self.plugin_hosts.reply_owned(
                            &item.token,
                            &item.id,
                            Err("The source navigation was superseded".into()),
                        );
                    } else if !self.loading {
                        if let Some(error) = &self.error {
                            self.plugin_hosts.reply_owned(
                                &item.token,
                                &item.id,
                                Err(error.clone()),
                            );
                        } else if !self.profiles.iter().any(|p| p.name == target.profile) {
                            self.plugin_hosts.reply_owned(
                                &item.token,
                                &item.id,
                                Err("The source no longer serves that agent profile".into()),
                            );
                        } else {
                            let _ = self.plugin_hosts.adopt_parent_scope(
                                &item.token,
                                &self.composer_scope(),
                                None,
                            );
                            self.plugin_navigate(action);
                        }
                    } else {
                        let _ = self.plugin_hosts.adopt_parent_scope(
                            &item.token,
                            &self.composer_scope(),
                            None,
                        );
                        item.stage = NavigationStage::Profiles { action, target };
                        pending.push(item);
                    }
                }
                NavigationStage::Hydration => {
                    let result =
                        if item.intent != self.intent || item.owner != self.composer_scope() {
                            Some(Err("The session navigation was superseded".into()))
                        } else if !self.history_loading {
                            Some(if let Some(error) = &self.error {
                                Err(error.clone())
                            } else {
                                Ok(json!({"ok":true}))
                            })
                        } else {
                            None
                        };
                    if let Some(result) = result {
                        self.plugin_hosts.reply_owned(&item.token, &item.id, result);
                    } else {
                        item.stage = NavigationStage::Hydration;
                        pending.push(item);
                    }
                }
            }
        }
        self.plugin_navigation.extend(pending);
    }
}

impl Viewer {
    pub(super) fn activate_plugin_notification(
        &mut self,
        activation: &crate::notifications::PluginActivation,
        token: Option<&str>,
    ) -> bool {
        self.plugin_hosts.activate_notification(activation, token)
    }
}
