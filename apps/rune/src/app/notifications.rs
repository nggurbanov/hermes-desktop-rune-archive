//! Notification callbacks retain their source; a body click cannot execute an action.
use super::*;
use crate::notifications::{Action, Activation};
impl Viewer {
    pub fn notification_action(&mut self, action: Action) {
        self.notifications.act(action, !self.fixture);
    }
    pub(super) fn bind_notifications(&mut self, connection: &Connection) {
        match crate::client::connection_key(connection) {
            Ok(principal) => self.notifications.bind_connection(principal),
            Err(error) => {
                self.notifications.bind_connection(String::new());
                self.client_error = Some(error);
            }
        }
    }
    pub(super) fn notification_event(
        &mut self,
        scope: &DraftScope,
        kind: &str,
        payload: &serde_json::Value,
        runtime: &str,
        replay: bool,
    ) {
        if self.fixture
            || scope.connection != self.connection_epoch
            || self
                .retired_profiles
                .contains(&(scope.connection, scope.profile.clone()))
        {
            return;
        }
        if !replay && kind == "message.complete" {
            self.sounds.complete(scope, runtime);
        }
        self.notifications.set_active(self.composer_scope());
        let title = self
            .conversation
            .view(scope)
            .map(|v| v.title.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                self.profiles
                    .iter()
                    .find(|p| p.name == scope.profile)
                    .map(|p| p.label.clone())
                    .unwrap_or_else(|| "Hermes".into())
            });
        self.notifications
            .event(scope, kind, payload, runtime, replay, &title);
    }
    pub(super) fn poll_notifications(&mut self) -> bool {
        self.notifications.set_active(self.composer_scope());
        if let Some(window) = &self.native_window {
            self.notifications.set_visible(window.is_visible());
        }
        let before = self.notifications.revision;
        let activations = self.notifications.poll();
        let changed = before != self.notifications.revision || !activations.is_empty();
        for activation in activations {
            self.activate_notification(activation);
        }
        changed
    }
    fn activate_notification(&mut self, activation: Activation) {
        self.activate_notification_with(activation, |s, plugin, token| {
            s.activate_plugin_notification(plugin, token)
        });
    }
    fn activate_notification_with(
        &mut self,
        activation: Activation,
        mut plugin_activate: impl FnMut(
            &mut Self,
            &crate::notifications::PluginActivation,
            Option<&str>,
        ) -> bool,
    ) {
        if let Some(plugin) = activation.plugin.as_ref() {
            if activation.principal != plugin.target.lifecycle_key {
                return;
            }
            if plugin_activate(self, plugin, activation.token.as_deref()) {
                return;
            }
            if plugin.action_id.is_none() {
                if let Some(window) = &self.native_window {
                    window.activate(activation.token.unwrap_or_default());
                }
            }
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        if crate::client::connection_key(&connection).as_deref()
            != Ok(activation.principal.as_str())
        {
            return;
        }
        let mut scope = activation.scope;
        if activation.action.is_none() {
            scope.connection = self.connection_epoch;
        }
        if scope.connection != self.connection_epoch
            || self
                .retired_profiles
                .contains(&(scope.connection, scope.profile.clone()))
        {
            return;
        }
        let Some(profile) = self
            .profiles
            .iter()
            .find(|p| p.name == scope.profile)
            .cloned()
        else {
            return;
        };
        if let Some(action) = activation.action {
            let valid = self.conversation.view(&scope).is_some_and(|view| {
                view.runtime == activation.runtime
                    && view.prompts.iter().any(|p| {
                        p.kind == "approval"
                            && p.prompt_id == action.prompt_id
                            && p.execution_generation == action.execution_generation
                            && p.execution_generation == view.generation
                            && p.choices.contains(&action.answer)
                    })
            });
            if !self.fixture && valid {
                if let Err(error) =
                    self.conversation
                        .respond(connection, &scope, &action.prompt_id, &action.answer)
                {
                    self.client_error = Some(error);
                }
            }
        }
        if let Some(id) = scope.session {
            let bot = profile
                .canonical_session
                .as_ref()
                .is_some_and(|chat| chat.id == id);
            let title = self
                .tabs
                .iter()
                .find(|t| t.profile == scope.profile && t.session == id)
                .map(|t| t.title.clone())
                .or_else(|| {
                    self.sidebar_state
                        .find(&scope.profile, &id)
                        .and_then(|r| r.title.clone())
                })
                .unwrap_or(profile.label);
            self.open_conversation(scope.profile, id, title, bot, true);
            self.reveal_pane("workspace");
            self.open_route(crate::pages::Route::Chat);
        }
        if let Some(window) = &self.native_window {
            window.activate(activation.token.unwrap_or_default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn viewer() -> Viewer {
        let mut s = Viewer::empty();
        s.fixture = true;
        s.connection_epoch = 9;
        s.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:49191".into(),
            token: None,
        });
        s.profile = "default".into();
        s.profiles = vec![Profile {
            name: "default".into(),
            label: "Bot identity".into(),
            description: String::new(),
            canonical_session: Some(CanonicalSession {
                id: "bot-chat".into(),
                resolved_id: "bot-chat".into(),
                preview: String::new(),
            }),
        }];
        s.selected_session = Some("foreground".into());
        s.tabs = vec![ConversationTab {
            profile: "default".into(),
            session: "foreground".into(),
            title: "Keep this tab".into(),
            bot: false,
        }];
        s
    }
    fn body(s: &Viewer) -> Activation {
        Activation {
            plugin: None,
            principal: crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap(),
            scope: DraftScope {
                connection: 123,
                profile: "default".into(),
                session: Some("bot-chat".into()),
            },
            runtime: "retired-runtime".into(),
            action: None,
            token: None,
        }
    }
    #[test]
    fn a_body_click_reveals_the_durable_bot_chat_without_replacing_an_unrelated_tab() {
        let mut s = viewer();
        s.activate_notification(body(&s));
        assert_eq!(s.selected_session.as_deref(), Some("bot-chat"));
        assert!(s.tabs.iter().any(|t| t.session == "foreground" && !t.bot));
        assert!(s.tabs.iter().any(|t| t.session == "bot-chat" && t.bot));
        assert_eq!(s.tabs.len(), 2);
    }
    #[test]
    fn a_live_secondary_plugin_uses_its_captured_source_without_main_profile_navigation() {
        let mut s = viewer();
        let primary = crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap();
        let mut activation = body(&s);
        let retained_principal = "retained-plugin-parent".to_string();
        assert_ne!(primary, retained_principal);
        activation.principal = retained_principal.clone();
        activation.scope.profile = "secondary-only".into();
        activation.plugin = Some(crate::notifications::PluginActivation {
            target: crate::notifications::PluginTarget {
                lease: None,
                window_token: "live-child".into(),
                lifecycle_key: retained_principal,
                source_id: "secondary-source".into(),
                source_principal: "secondary-verified-principal".into(),
                profile: "secondary-only".into(),
                request_id: "ask".into(),
                notify_id: Some("closure".into()),
                tag: "plugin".into(),
                activate: Some("/plugin/item".into()),
                actions: vec![],
                icon: None,
                background: true,
            },
            action_id: None,
        });
        let mut received = vec![];
        // The production host performs the exact live token/source/lease check;
        // this transport seam proves the main foreground cannot preempt it.
        s.activate_notification_with(activation.clone(), |_, plugin, _| {
            received.push((
                plugin.target.source_id.clone(),
                plugin.target.profile.clone(),
            ));
            true
        });
        assert_eq!(
            received,
            vec![("secondary-source".into(), "secondary-only".into())]
        );
        assert_eq!(s.selected_session.as_deref(), Some("foreground"));
        assert_eq!(s.tabs.len(), 1);
        activation.principal = "wrong-envelope".into();
        s.activate_notification_with(activation, |_, _, _| {
            panic!("foreign envelope reached the host")
        });
    }
    #[test]
    fn retired_plugin_body_or_button_cannot_execute_a_chat_action_or_navigation() {
        let mut s = viewer();
        for action_id in [None, Some("run".to_string())] {
            let mut activation = body(&s);
            activation.plugin = Some(crate::notifications::PluginActivation {
                target: crate::notifications::PluginTarget {
                    lease: None,
                    window_token: "retired-window".into(),
                    lifecycle_key: activation.principal.clone(),
                    source_id: "retired-source".into(),
                    source_principal: activation.principal.clone(),
                    profile: "default".into(),
                    request_id: "old-request".into(),
                    notify_id: Some("old-callback".into()),
                    tag: "plugin".into(),
                    activate: Some("/plugin/home".into()),
                    actions: vec![],
                    icon: None,
                    background: true,
                },
                action_id,
            });
            s.activate_notification(activation);
            assert_eq!(s.selected_session.as_deref(), Some("foreground"));
            assert_eq!(s.tabs.len(), 1);
        }
    }
    #[test]
    fn a_notification_cannot_cross_a_new_connection_principal_or_retired_profile() {
        let mut s = viewer();
        let mut foreign = body(&s);
        foreign.principal = "another-principal".into();
        s.activate_notification(foreign);
        assert_eq!(s.selected_session.as_deref(), Some("foreground"));
        let retired = body(&s);
        s.retired_profiles.insert((9, "default".into()));
        s.activate_notification(retired);
        assert_eq!(s.selected_session.as_deref(), Some("foreground"));
        assert_eq!(s.tabs.len(), 1);
    }
}
