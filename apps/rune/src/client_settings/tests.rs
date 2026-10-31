use super::*;
#[test]
fn device_changes_survive_restart_without_overwriting_unknown_preferences_or_agent_fonts() {
    let root = std::env::temp_dir().join(format!("hermes-display-{}", uuid::Uuid::new_v4()));
    let path = root.join("display.json");
    crate::client::private_write(
        &path,
        br#"{"version":1,"future":{"keep":true},"bindings":{"plugin.late":["ctrl+f8"]}}"#,
    )
    .unwrap();
    let mut c = ClientSettings::new();
    c.load_at(path.clone());
    c.apply_config(&json!({"desktop":{"font_family":"IBM Plex Mono"}}));
    c.act(Action::Scale(150), true);
    c.act(Action::Bubble(40), true);
    c.act(Action::Density("detailed".into()), true);
    let mut loaded = ClientSettings::new();
    loaded.load_at(path);
    assert_eq!(loaded.prefs.chat_scale, 150);
    assert_eq!(loaded.prefs.bubble_transparency, 40);
    assert_eq!(loaded.prefs.other["future"], json!({"keep":true}));
    assert_eq!(loaded.prefs.bindings["plugin.late"], vec!["ctrl+f8"]);
    assert_eq!(c.config["desktop"]["font_family"], "IBM Plex Mono");
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn invalid_or_future_saved_preferences_are_preserved_when_a_control_changes() {
    let root = std::env::temp_dir().join(format!("hermes-display-{}", uuid::Uuid::new_v4()));
    let path = root.join("display.json");
    let bytes = br#"{"version":9,"future":true}"#;
    crate::client::private_write(&path, bytes).unwrap();
    let mut c = ClientSettings::new();
    c.load_at(path.clone());
    c.act(Action::Scale(150), true);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert!(c.error.is_some());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn clearing_removes_default_dispatch_reset_restores_it_and_conflicts_name_the_other_action() {
    let mut c = ClientSettings::new();
    let key = keys::canonical("mod+n").unwrap();
    assert_eq!(c.command(&key, true), Some("session.new"));
    c.act(Action::Clear("session.new".into()), false);
    assert_eq!(c.command(&key, true), None);
    c.set_binding("nav.settings".into(), Some(key.clone()), false);
    assert_eq!(c.command(&key, true), Some("nav.settings"));
    c.act(Action::Reset("session.new".into()), false);
    assert!(c.conflicts("nav.settings").contains(&"New conversation"));
    assert_eq!(c.command(&key, true), Some("session.new"));
    c.prefs
        .bindings
        .insert("plugin.unloaded".into(), vec!["ctrl+f8".into()]);
    c.act(Action::ResetBindings, false);
    assert_eq!(c.command(&key, true), Some("session.new"));
    assert!(c.prefs.bindings.contains_key("plugin.unloaded"));
    c.set_binding("session.new".into(), Some(key.clone()), false);
    assert!(!c.prefs.bindings.contains_key("session.new"));
}
#[test]
fn physical_shifted_keys_and_typing_safety_do_not_depend_on_emitted_symbol() {
    let e = rune::KeyEvent {
        key: rune::Key::Command('&'),
        mods: rune::Mods {
            cmd: true,
            shift: true,
            ..Default::default()
        },
    };
    let physical = event_chord(&e, Some(rune::PhysicalKeyCode::Digit7)).unwrap();
    assert_eq!(physical, keys::canonical("mod+shift+7").unwrap());
    let c = ClientSettings::new();
    assert_eq!(c.command("shift+x", true), None);
    assert_eq!(c.command("shift+x", false), Some("appearance.toggleMode"));
}
#[test]
fn profile_read_binding_clears_previous_fonts_and_only_restores_confirmed_configuration() {
    let mut c = ClientSettings::new();
    let owner = DraftScope {
        draft_origin: None,
        profile_intent: crate::source::ProfileIntent::Backend,
        connection: 3,
        profile: "first".into(),
        session: Some("a".into()),
    };
    let requests = c.bind(owner.clone());
    assert_eq!(requests[0].1.profile, Some("first".into()));
    c.apply("client-display.config",Ok(json!({"desktop":{"font_family":"IBM Plex Mono"},"display":{"show_reasoning":true,"platforms":{"cli":{"show_reasoning":false}},"timestamps":true}})));
    assert_eq!(c.chat_family(), Family::Mono);
    assert!(!c.show_reasoning());
    assert!(c.timestamps());
    let refresh = c.refresh();
    assert_eq!(refresh.len(), 1);
    assert_eq!(refresh[0].1.profile, Some("first".into()));
    assert_eq!(c.chat_family(), Family::Mono);
    assert_eq!(
        c.refresh().len(),
        1,
        "an acknowledged write must supersede any older in-flight read"
    );
    assert!(
        c.bind(DraftScope {
            session: Some("b".into()),
            draft_origin: None,
            ..owner.clone()
        })
        .is_empty()
    );
    c.bind(DraftScope {
        profile: "second".into(),
        ..owner
    });
    assert_eq!(c.chat_family(), Family::Sans);
    assert!(c.show_reasoning());
    assert!(!c.timestamps());
    c.act(Action::Scale(175), false);
    assert!(c.prose(TextStyle::new(14., 400.)).size > 14.);
    assert!(!c.default_open(true, true));
}

#[test]
fn agent_font_fields_are_clean_until_explicit_selection_and_save_only_the_sparse_profile_key() {
    let mut settings = crate::pages::settings::Settings::new();
    assert!(!settings.config_ready());
    settings.apply("settings.schema", Ok(json!({"fields":{}})));
    assert!(!settings.config_ready());
    settings.apply(
        "settings.config",
        Ok(json!({"model":{"default":"unchanged"}})),
    );
    assert!(settings.config_ready());
    assert_eq!(settings.dirty_count(), 0);
    assert!(settings.changed_values().unwrap().is_empty());
    settings.set_value("desktop.font_family", json!("IBM Plex Mono"), true);
    let changed = settings.changed_values().unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed["desktop.font_family"], "IBM Plex Mono");
    assert_eq!(settings.confirmed_config()["model"]["default"], "unchanged");
}

#[test]
fn close_flush_preserves_untouched_read_only_display_but_reports_unsaved_edits() {
    for bytes in [
        b"invalid display".as_slice(),
        br#"{"version":9,"future":true}"#.as_slice(),
    ] {
        let root =
            std::env::temp_dir().join(format!("hermes-display-close-{}", uuid::Uuid::new_v4()));
        let path = root.join("display.json");
        crate::client::private_write(&path, bytes).unwrap();
        let mut client = ClientSettings::new();
        client.load_at(path.clone());
        assert!(client.error.is_some());
        client.flush_preferences().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        client.act(Action::Scale(150), true);
        assert!(client.flush_preferences().is_err());
        assert_eq!(client.prefs.chat_scale, 150);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn close_flush_retries_pending_display_intent_immediately_after_repair() {
    let root = std::env::temp_dir().join(format!("hermes-display-close-{}", uuid::Uuid::new_v4()));
    let path = root.join("display.json");
    let mut client = ClientSettings::new();
    client.load_at(path.clone());
    let invalid = br#"{"version":9,"keep":true}"#;
    crate::client::private_write(&path, invalid).unwrap();
    // Same-value selection is still an intent against a newer foreign value.
    client.act(Action::Scale(110), true);
    assert!(client.preferences_wake_after().is_some());
    assert!(client.flush_preferences().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), invalid);
    crate::client::private_write(
        &path,
        br#"{"version":1,"chat_scale":175,"bubble_transparency":45,"future":true}"#,
    )
    .unwrap();
    client.flush_preferences().unwrap();
    let mut loaded = ClientSettings::new();
    loaded.load_at(path);
    assert_eq!(loaded.prefs.chat_scale, 110);
    assert_eq!(loaded.prefs.bubble_transparency, 45);
    assert_eq!(loaded.prefs.other["future"], true);
    assert!(client.preferences_wake_after().is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn default_project_directory_is_a_persisted_client_preference() {
    let root =
        std::env::temp_dir().join(format!("hermes-default-project-{}", uuid::Uuid::new_v4()));
    let path = root.join("display.json");
    let mut settings = ClientSettings::new();
    settings.load_at(path.clone());
    settings.act(
        Action::DefaultProjectDir(Some("/private/project".into())),
        true,
    );
    let mut restored = ClientSettings::new();
    restored.load_at(path.clone());
    assert_eq!(
        restored.prefs.default_project_dir.as_deref(),
        Some("/private/project")
    );
    restored.act(Action::DefaultProjectDir(None), true);
    let mut cleared = ClientSettings::new();
    cleared.load_at(path);
    assert_eq!(cleared.prefs.default_project_dir, None);
    std::fs::remove_dir_all(root).unwrap();
}
