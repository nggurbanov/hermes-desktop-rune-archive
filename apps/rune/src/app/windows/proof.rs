//! Opt-in read-only inspection of real native windows against a private loopback peer.
use super::*;
impl Viewer {
    pub(in crate::app) fn inspect_unhandled_window_key(
        &mut self,
        event: &rune::KeyEvent,
        tx: &rune::Tx,
    ) {
        if self.window.registry.borrow().proof.is_some() {
            let kind = if matches!(event.key, rune::Key::Command(_)) {
                "command"
            } else {
                "other"
            };
            self.window.last_unhandled_key = Some(format!(
                "{kind}: {:?}; focus={:?}",
                event.mods,
                tx.focused()
            ));
        }
    }
    pub(in crate::app) fn configure_window_inspection(&mut self) {
        if self.window.registry.borrow().proof.is_some() {
            return;
        }
        let Some(path) = argument("--peer-window-proof-dir")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        else {
            return;
        };
        let valid = self
            .connection
            .as_ref()
            .is_some_and(|c| match c.transport() {
                Connection::Url { endpoint, .. } => crate::gateway::endpoint_url(endpoint)
                    .is_ok_and(|(u, _)| {
                        u.scheme() == "http"
                            && u.host_str().is_some_and(|h| {
                                h.parse::<std::net::IpAddr>()
                                    .is_ok_and(|ip| ip.is_loopback())
                            })
                    }),
                _ => false,
            });
        if valid {
            self.window.registry.borrow_mut().proof = Some(path);
        }
    }
    pub fn inspect_native_window(&self, cx: &mut rune::Cx<Viewer>) {
        let Some(lease) = &self.window.lease else {
            return;
        };
        let Some(directory) = self.window.registry.borrow().proof.clone() else {
            return;
        };
        let mut geometry = serde_json::Map::new();
        for key in [
            "composer-input",
            "composer-send",
            "composer-stop",
            "attach-file",
            "settings",
            "theme-toggle",
            "model-picker-trigger",
            "model-settings-trigger",
            "timeline",
            "sessions-tab",
            "bots-tab",
            "page-back",
            "hermes-titlebar",
            "reconnect",
            "status-profile",
            "scope-profile-picker",
            "profile-picker",
            "sidebar-session-actions-item6",
            "conversation-actions-item2",
            "hermes-window-actions-item0",
            "hermes-window-actions-item1",
            "hermes-window-actions-item2",
            "hermes-window-actions-item3",
            "hermes-window-actions-item4",
            "change-connection",
            "connection-url",
            "connection-token",
            "connect-url",
            "connection-cancel",
            "profile-picker-toggle",
            "pick-profile-research",
            "pick-profile-archive",
            "bot-research",
            "bot-archive",
            "profile-default",
            "profile-research",
            "profile-archive",
            "pick-profile-default",
            "saved-connections",
            "saved-reload",
            "saved-label",
            "saved-save",
            "compact-profile",
            "collapsed-profile-rail",
            "startup-last-used",
            "device-default-clear",
            "device-default-summary",
            "profile-route-actions-item0",
            "profile-route-actions-item1",
            "profile-route-actions-item2",
        ] {
            if let Some(rect) = cx.layout(key) {
                geometry.insert(key.into(), serde_json::json!(rect));
            }
        }
        for row in &self.sessions {
            let key = format!("session-{}-{}", self.profile, row.id);
            if let Some(rect) = cx.layout(&key) {
                geometry.insert(key, serde_json::json!(rect));
            }
        }
        for row in &self.connections.rows {
            for prefix in [
                "saved-connect",
                "saved-primary",
                "saved-edit",
                "saved-forget",
            ] {
                let key = format!("{prefix}-{}", row.id);
                if let Some(rect) = cx.layout(&key) {
                    geometry.insert(key, serde_json::json!(rect));
                }
            }
        }
        let scope = self.composer_scope();
        let editor_state = self.composer.draft(&scope).map(|draft| {
            serde_json::json!({
                "focused":draft.text.focused,"anchor":draft.text.anchor,"focus":draft.text.focus,
                "selected_bytes":draft.text.selected().len(),"policy_id":draft.policy_id,
            })
        });
        let retired_input_state = serde_json::json!({
            "focused":self.retired_editor_input.focused,
            "anchor":self.retired_editor_input.anchor,"focus":self.retired_editor_input.focus,
            "selected_bytes":self.retired_editor_input.selected().len(),
            "bytes":self.retired_editor_input.text.len(),
        });
        let focused_inputs = ["composer-input", "connection-url", "connection-token"]
            .into_iter()
            .filter(|key| cx.caret(key).is_some())
            .collect::<Vec<_>>();
        let connection_url = url::Url::parse(&self.connection_url.text)
            .ok()
            .filter(|url| url.username().is_empty() && url.password().is_none())
            .map(|mut url| {
                url.set_query(None);
                url.set_fragment(None);
                url.to_string()
            });
        let native=self.window.native.as_ref().and_then(WindowHandle::geometry).map(|f|serde_json::json!({"position":f.position,"inner_size":f.inner_size,"scale":f.scale_factor,"maximized":f.maximized}));
        let snapshot = serde_json::json!({"id":lease.id().as_str(),"editor":self.composer.editor_id().as_str(),"window_editor":lease.editor_id().as_str(),"native_raw_handle":self.window.native.as_ref().and_then(|host|host.with_raw_handle(|handle|format!("{:?}",handle.as_raw())).ok()),"editor_state":editor_state,"retired_input_state":retired_input_state,"focused_inputs":focused_inputs,"last_unhandled_key":self.window.last_unhandled_key,"default_profile":self.default_profile_route(),"route_pending":self.device_route_pending(),"device_default_error":self.device_default_error(),"registry":{"loaded":self.connections.registry_loaded(),"primary":self.connections.primary(),"last_used":self.connections.last_used(),"launch_mode":self.connections.launch_mode(),"busy":self.connections.busy,"error":self.connections.error},"can_send":self.can_send(),"connection_form":self.connection_form,"connection_url":connection_url,"connection_token_set":!self.connection_token.text.is_empty(),"kind":lease.record().kind,"dark":self.theme.unwrap_or(self.client_settings.system_dark.get()),"profile":self.profile,"session":self.selected_session,"source_epoch":scope.connection,"principal":self.connection.as_ref().and_then(|c|crate::client::connection_key(c).ok()),"route":format!("{:?}",self.route),"loading":self.loading||self.history_loading,"error":self.error,"client_error":self.client_error,"compose_error":self.compose_error,"draft":self.composer.draft(&scope).map(|d|d.text.text.as_str()).unwrap_or(""),"running":self.chat_running(),"active_text":self.conversation.view(&scope).map(|v|v.active_text.as_str()).unwrap_or(""),"messages":self.messages.iter().map(|m|serde_json::json!({"role":m.role,"content":m.content})).collect::<Vec<_>>(),"geometry":geometry,"native":native});
        if let Ok(bytes) = serde_json::to_vec(&snapshot) {
            let _ = crate::client::private_write(
                &directory.join(format!("{}.json", lease.id().as_str())),
                &bytes,
            );
        }
    }
}

/// Observe the actually presented native frame, without changing hit testing or focus.
pub(crate) fn inspect_native_frame(runtime: &rune::Runtime<Viewer>) {
    let s = &runtime.state;
    let Some(lease) = &s.window.lease else { return };
    let Some(directory) = s.window.registry.borrow().proof.clone() else {
        return;
    };
    let pointer = runtime.pointer();
    let elements = runtime
        .elements()
        .into_iter()
        .filter(|e| {
            e.key.is_some()
                || pointer.is_some_and(|(x, y)| {
                    let r = e.visible;
                    x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]
                })
        })
        .map(|e| {
            serde_json::json!({
                "key":e.key,"path":e.path,"rect":e.rect,"visible":e.visible,
                "focusable":e.focusable,"focused":e.focused,"inert":e.inert,
                "exiting":e.exiting,"opacity":e.opacity,"hover":e.hover,"press":e.press,
            })
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({"id":lease.id().as_str(),"pointer":pointer,"elements":elements});
    if let Ok(bytes) = serde_json::to_vec(&value) {
        let _ = crate::client::private_write(
            &directory
                .join("frames")
                .join(format!("{}.json", lease.id().as_str())),
            &bytes,
        );
    }
}
