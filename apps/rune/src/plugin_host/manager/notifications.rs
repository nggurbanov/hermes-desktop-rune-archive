use super::*;
use crate::notifications::{Kind, Notice, PluginActivation, PluginButton, PluginTarget};
impl PluginHosts {
    pub fn prepare_native_notification(
        &mut self,
        action: &HostAction,
    ) -> Result<Option<Notice>, String> {
        let running = self.running.as_mut().ok_or("The plugin window closed")?;
        if !super::notifications::same_request_owner(
            &action.payload["native_owner"],
            &running.token,
            &running.key,
            &running.request_source,
            &running.request_profile,
        ) {
            return Err("The plugin notification owner changed".into());
        }
        let Some((focused, visible)) = running.window_facts else {
            return Ok(None);
        };
        if focused && visible || running.opened_at.elapsed() < std::time::Duration::from_secs(4) {
            return Ok(None);
        }
        let source_id = running.request_source.clone();
        let binding = running
            .sources
            .bindings()
            .into_iter()
            .find(|b| b.id == source_id)
            .ok_or("The plugin notification source retired")?;
        if binding.lease.as_ref().is_some_and(|l| !l.is_valid())
            || !binding.profiles.contains(&running.request_profile)
        {
            return Err("The plugin notification source is no longer live".into());
        }
        let title = bounded(&action.payload, "title", 256)?
            .filter(|s| !s.is_empty())
            .ok_or("A native notification needs a title")?;
        let body = bounded(&action.payload, "body", 4000)?.unwrap_or_default();
        let notify_id = bounded(&action.payload, "notifyId", 512)?;
        let tag = bounded(&action.payload, "tag", 256)?.unwrap_or_else(|| "desktop-plugin".into());
        let mut actions = vec![];
        let mut ids = BTreeSet::new();
        if let Some(value) = action.payload.get("actions") {
            let values = value
                .as_array()
                .ok_or("Invalid plugin notification actions")?;
            if values.len() > 8 {
                return Err("A native notification supports at most eight actions".into());
            }
            for value in values {
                let id = bounded(value, "id", 128)?
                    .filter(|s| !s.is_empty())
                    .ok_or("A notification action needs an id")?;
                if !ids.insert(id.clone()) {
                    return Err("Notification action ids must be unique".into());
                }
                let label = bounded(value, "text", 128)?
                    .filter(|s| !s.is_empty())
                    .ok_or("A notification action needs a label")?;
                actions.push(PluginButton {
                    id,
                    label,
                    activate: app_path(value.get("activate"))?,
                });
            }
        }
        let target = PluginTarget {
            lease: binding.lease.as_ref().map(|l| l.guard()),
            window_token: running.token.clone(),
            lifecycle_key: running.key.clone(),
            source_id,
            source_principal: crate::client::connection_key(&binding.connection)?,
            profile: running.request_profile.clone(),
            request_id: action.id.clone(),
            notify_id,
            tag: tag.clone(),
            activate: app_path(action.payload.get("activate"))?,
            actions,
            icon: bounded(&action.payload, "icon", 4096)?
                .filter(|p| std::path::Path::new(p).is_absolute()),
            background: !focused || !visible,
        };
        Ok(Some(Notice {
            plugin: Some(target),
            kind: Kind::Plugin,
            scope: DraftScope {
                session: None,
                ..running.scope.clone()
            },
            runtime: String::new(),
            title,
            body,
            global: true,
            tag: Some(tag),
            silent: action.payload["silent"].as_bool().unwrap_or(false),
            buttons: vec![],
        }))
    }
    fn notification_matches(&mut self, target: &PluginTarget) -> bool {
        let Some(running) = self.running.as_mut() else {
            return false;
        };
        running.sources.bindings().into_iter().any(|binding| {
            origin_matches(
                target,
                &running.token,
                &running.key,
                &running.request_source,
                &running.request_profile,
                &binding,
            )
        })
    }
    pub fn reply_notification(&mut self, target: &PluginTarget, result: Result<bool, String>) {
        if self.notification_matches(target) {
            self.reply_owned(
                &target.window_token,
                &target.request_id,
                result.map(Value::Bool),
            );
        }
    }
    pub fn clear_notification(&mut self, target: &PluginTarget) {
        if self.notification_matches(target) {
            if let Some(running) = self.running.as_mut() {
                let _ = send_cleanup(target, |value| running.process.send(value));
            }
        }
    }
    pub fn activate_notification(
        &mut self,
        activation: &PluginActivation,
        token: Option<&str>,
    ) -> bool {
        let target = &activation.target;
        if !self.notification_matches(target) {
            return false;
        }
        let Some(running) = self.running.as_mut() else {
            return false;
        };
        send_activation(activation, token, |value| running.process.send(value))
    }
}
pub(super) fn same_request_owner(
    owner: &Value,
    window: &str,
    lifecycle: &str,
    source: &str,
    profile: &str,
) -> bool {
    owner["window_token"].as_str() == Some(window)
        && owner["lifecycle_key"].as_str() == Some(lifecycle)
        && owner["connection_id"].as_str() == Some(source)
        && owner["profile"].as_str() == Some(profile)
}
fn bounded(value: &Value, key: &str, limit: usize) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.len() <= limit => Ok(Some(s.clone())),
        _ => Err(format!("Invalid plugin notification {key}")),
    }
}
fn app_path(value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let path = value.as_str().ok_or("Invalid plugin activation path")?;
    let base = path.split('?').next().unwrap_or(path);
    if path.len() > 4096
        || !base.starts_with('/')
        || base.starts_with("//")
        || base.contains(['\\', ':'])
        || base.contains("..")
        || path.chars().any(char::is_control)
    {
        return Err("Unsafe plugin activation path".into());
    }
    Ok(Some(path.into()))
}

fn origin_matches(
    target: &PluginTarget,
    window: &str,
    lifecycle: &str,
    source: &str,
    profile: &str,
    binding: &crate::plugin_host::sources::Binding,
) -> bool {
    target.lease.as_ref().is_none_or(|l| l.is_valid())
        && window == target.window_token
        && lifecycle == target.lifecycle_key
        && source == target.source_id
        && profile == target.profile
        && binding.id == target.source_id
        && binding.profiles.contains(&target.profile)
        && binding.lease.as_ref().is_none_or(|l| l.is_valid())
        && crate::client::connection_key(&binding.connection).as_deref()
            == Ok(target.source_principal.as_str())
}
fn send_cleanup(
    target: &PluginTarget,
    mut send: impl FnMut(Value) -> Result<(), String>,
) -> Result<(), String> {
    if let Some(id) = &target.notify_id {
        send(json!({"type":"plugin-notify-closed","notifyId":id}))?;
    }
    Ok(())
}
fn send_activation(
    activation: &PluginActivation,
    token: Option<&str>,
    mut send: impl FnMut(Value) -> Result<(), String>,
) -> bool {
    let target = &activation.target;
    let activate = if let Some(id) = &activation.action_id {
        let Some(button) = target.actions.iter().find(|b| &b.id == id) else {
            return false;
        };
        button.activate.clone()
    } else {
        target.activate.clone()
    };
    send(json!({"type":"focus","token":token})).is_ok()
        && send(json!({"type":"plugin-notify-activate","notifyId":target.notify_id,"actionId":activation.action_id,"activate":activate,"pluginId":target.tag})).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FakeOwnedHost {
        window: String,
        key: String,
        source: String,
        profile: String,
        binding: crate::plugin_host::sources::Binding,
        sent: Vec<Value>,
    }
    impl FakeOwnedHost {
        fn clear(&mut self, target: &PluginTarget) {
            if origin_matches(
                target,
                &self.window,
                &self.key,
                &self.source,
                &self.profile,
                &self.binding,
            ) {
                send_cleanup(target, |value| {
                    self.sent.push(value);
                    Ok(())
                })
                .unwrap();
            }
        }
        fn activate(&mut self, activation: &PluginActivation) -> bool {
            if !origin_matches(
                &activation.target,
                &self.window,
                &self.key,
                &self.source,
                &self.profile,
                &self.binding,
            ) {
                return false;
            }
            send_activation(activation, Some("activation-token"), |value| {
                self.sent.push(value);
                Ok(())
            })
        }
    }
    #[test]
    fn queued_native_request_cannot_move_to_a_replacement_window_source_or_profile() {
        let owner = json!({"window_token":"window","lifecycle_key":"parent","connection_id":"source","profile":"profile"});
        assert!(same_request_owner(
            &owner, "window", "parent", "source", "profile"
        ));
        for (window, parent, source, profile) in [
            ("replacement", "parent", "source", "profile"),
            ("window", "new-parent", "source", "profile"),
            ("window", "parent", "new-source", "profile"),
            ("window", "parent", "source", "new-profile"),
        ] {
            assert!(!same_request_owner(&owner, window, parent, source, profile));
        }
    }
    #[test]
    fn only_same_live_host_source_can_receive_plugin_callback_and_real_focus_command() {
        let connection = Connection::Url {
            endpoint: "http://127.0.0.1:49191".into(),
            token: None,
        };
        let key = crate::client::connection_key(&connection).unwrap();
        let mut host = FakeOwnedHost {
            window: "window".into(),
            key: "parent".into(),
            source: "source".into(),
            profile: "profile".into(),
            binding: crate::plugin_host::sources::Binding {
                id: "source".into(),
                connection,
                lease: None,
                profiles: BTreeSet::from(["profile".into()]),
            },
            sent: vec![],
        };
        let target = PluginTarget {
            lease: None,
            window_token: "window".into(),
            lifecycle_key: "parent".into(),
            source_id: "source".into(),
            source_principal: key,
            profile: "profile".into(),
            request_id: "ask".into(),
            notify_id: Some("notify-id".into()),
            tag: "plugin".into(),
            activate: Some("/plugin/home".into()),
            actions: vec![PluginButton {
                id: "item".into(),
                label: "Item".into(),
                activate: Some("/plugin/item/1".into()),
            }],
            icon: None,
            background: true,
        };
        for id in 0..140 {
            let mut closed = target.clone();
            closed.notify_id = Some(format!("closed-{id}"));
            host.clear(&closed);
            closed.window_token = "wrong-window".into();
            host.clear(&closed);
        }
        assert_eq!(host.sent.len(), 140);
        assert_eq!(host.sent[139]["notifyId"], "closed-139");
        host.sent.clear();
        let activation = PluginActivation {
            target: target.clone(),
            action_id: Some("item".into()),
        };
        assert!(host.activate(&activation));
        assert_eq!(host.sent.len(), 2);
        assert_eq!(host.sent[0]["type"], "focus");
        assert_eq!(host.sent[1]["notifyId"], "notify-id");
        assert_eq!(host.sent[1]["actionId"], "item");
        assert_eq!(host.sent[1]["activate"], "/plugin/item/1");
        host.sent.clear();
        host.window = "replacement".into();
        assert!(!host.activate(&activation));
        assert!(host.sent.is_empty());
        host.window = "window".into();
        host.key = "new-parent".into();
        assert!(!host.activate(&activation));
        host.key = "parent".into();
        host.binding.profiles.clear();
        assert!(!host.activate(&activation));
        host.binding.profiles.insert("profile".into());
        host.binding.connection = Connection::Url {
            endpoint: "http://127.0.0.1:49192".into(),
            token: None,
        };
        assert!(!host.activate(&activation));
        assert!(host.sent.is_empty());
        assert!(host.sent.is_empty());
    }
}
