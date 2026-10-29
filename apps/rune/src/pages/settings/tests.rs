use super::*;
fn loaded() -> Settings {
    let mut s = Settings::new();
    s.apply("settings.schema",Ok(json!({"fields":{
        "agent.max_turns":{"type":"number"},"memory.memory_enabled":{"type":"boolean"},
        "optional_missing":{"type":"string"},"toolsets":{"type":"list"},"fallback_providers":{"type":"list"}
    }})));
    s.apply("settings.config",Ok(json!({"agent":{"max_turns":90,"api_max_retries":5},"memory":{"memory_enabled":true},"toolsets":["terminal","memory"],"fallback_providers":[{"provider":"local","model":"fallback","base_url":"http://example.invalid/private","key_env":"LOCAL_KEY"}]})));
    s
}
#[test]
fn schema_can_arrive_before_config_without_stale_empty_fields_or_spurious_writes() {
    let mut s = loaded();
    assert_eq!(s.value("agent.max_turns"), json!(90));
    assert_eq!(s.dirty_count(), 0);
    assert!(s.save(false).is_empty());
    assert!(s.fields.iter().any(|f| f.key == "optional_missing"));
}
#[test]
fn saves_are_sparse_serialized_and_readback_preserves_a_newer_dirty_edit() {
    let mut s = loaded();
    s.set_value("agent.max_turns", json!(80), true);
    let effects = s.save(false);
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0].1.body,
        json!({"config":{"agent":{"max_turns":80}}})
    );
    s.set_value("agent.max_turns", json!(70), true);
    assert!(s.save(false).is_empty());
    let readback = s.apply("settings.save", Ok(json!({"ok":true})));
    assert_eq!(readback[0].0, "settings.readback");
    s.apply(
        "settings.readback",
        Ok(json!({"agent":{"max_turns":80,"api_max_retries":6},"toolsets":["terminal","memory"]})),
    );
    assert_eq!(s.value("agent.max_turns"), json!(70));
    assert_eq!(s.value("agent.api_max_retries"), json!(6));
    assert_eq!(
        s.save(false)[0].1.body,
        json!({"config":{"agent":{"max_turns":70}}})
    );
}
#[test]
fn invalid_number_stays_in_the_draft_during_refresh_and_never_reaches_a_write() {
    let mut s = loaded();
    s.edit_mut("agent.max_turns").replace_text("not a number");
    s.changed();
    s.apply("settings.readback", Ok(json!({"agent":{"max_turns":50}})));
    assert_eq!(s.edit_mut("agent.max_turns").text, "not a number");
    assert!(s.save(false).is_empty());
    assert!(s.error.is_some());
}
#[test]
fn clearing_all_toolsets_requires_explicit_confirmation() {
    let mut s = loaded();
    s.set_value("toolsets", json!([]), true);
    assert!(s.save(false).is_empty());
    assert_eq!(s.confirm.as_deref(), Some("toolsets"));
    assert_eq!(s.save(true)[0].1.body, json!({"config":{"toolsets":[]}}));
}
#[test]
fn fallback_editor_retains_routing_keys_and_excludes_incomplete_rows() {
    let mut s = loaded();
    assert_eq!(s.fallback_rows.len(), 1);
    s.fallback_rows[0].model.replace_text("changed");
    s.fallback_rows.push(FallbackRow::empty());
    s.sync_fallback();
    let effects = s.save(false);
    let rows = &effects[0].1.body["config"]["fallback_providers"];
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["model"], "changed");
    assert_eq!(rows[0]["base_url"], "http://example.invalid/private");
    assert_eq!(rows[0]["key_env"], "LOCAL_KEY");
}
#[test]
fn environment_previews_are_never_written_back_as_credentials() {
    let mut s = Settings::new();
    s.apply("settings.env",Ok(json!({"MODEL_KEY":{"is_set":true,"is_password":true,"redacted_value":"«redacted: fake»"},"CHANNEL_KEY":{"channel_managed":true}})));
    assert_eq!(s.env.len(), 1);
    assert!(s.env[0].editor.text.is_empty());
    assert!(s.env[0].revealed.is_none());
    s.env[0].editor.replace_text("explicitly entered value");
    let effects = s.env_write("MODEL_KEY".into(), false);
    assert_eq!(
        effects[0].1.body,
        json!({"key":"MODEL_KEY","value":"explicitly entered value"})
    );
}

#[test]
fn settings_scope_pins_reads_writes_readback_credentials_and_oauth_cleanup() {
    let mut s = loaded();
    s.scope_profile = Some("other".into());
    assert!(
        s.load()
            .iter()
            .all(|(_, r)| r.profile.as_deref() == Some("other"))
    );
    s.set_value("agent.max_turns", json!(12), true);
    assert_eq!(s.save(false)[0].1.profile.as_deref(), Some("other"));
    assert_eq!(
        s.apply("settings.save", Ok(json!({"ok":true})))[0]
            .1
            .profile
            .as_deref(),
        Some("other")
    );
    assert_eq!(s.env_load()[0].1.profile.as_deref(), Some("other"));
    s.providers.act(providers::Action::Start("sample".into()));
    s.providers.apply("settings.providers.start",Ok(json!({"session_id":"owned","flow":"pkce","auth_url":"https://example.invalid/sign-in","expires_in":60})));
    let cleanup = s.cancel_owned();
    assert_eq!(cleanup[0].1.profile.as_deref(), Some("other"));
    assert_eq!(cleanup[0].1.path, "/api/providers/oauth/sessions/owned");
}

#[test]
fn a_read_failure_does_not_unlock_an_inflight_config_write() {
    let mut s = loaded();
    s.set_value("agent.max_turns", json!(11), true);
    assert_eq!(s.save(false).len(), 1);
    s.apply("settings.readback", Err("read failed".into()));
    assert!(s.saving);
    assert!(s.save(false).is_empty());
}

#[test]
fn import_is_a_sparse_merge_and_does_not_clear_omitted_keys() {
    let mut s = loaded();
    s.import_value(json!({"agent":{"max_turns":22},"new_setting":{"enabled":true}}))
        .unwrap();
    let effects = s.save(false);
    assert_eq!(
        effects[0].1.body,
        json!({"config":{"agent":{"max_turns":22},"new_setting":{"enabled":true}}})
    );
    assert_eq!(s.export_value().unwrap()["agent"]["api_max_retries"], 5);
    assert!(s.import_value(json!([1, 2, 3])).is_err());
}

#[test]
fn active_agent_runtime_scope_cannot_be_overwritten_by_settings_override() {
    let mut s = Settings::new();
    s.scope_profile = Some("settings-other".into());
    let reads = s.local_models.load("active-agent");
    let effects = s.pin(reads);
    assert!(
        effects
            .iter()
            .all(|(_, r)| r.profile.as_deref() == Some("active-agent"))
    );
}

#[test]
fn voice_catalog_is_scoped_and_refresh_never_replaces_custom_draft() {
    let mut s = Settings::new();
    s.scope_profile = Some("voice-owner".into());
    s.apply(
        "settings.config",
        Ok(json!({"tts":{"elevenlabs":{"voice_id":"custom"}}})),
    );
    s.edit_mut("tts.elevenlabs.voice_id")
        .replace_text("private-clone");
    let effects = s.load();
    assert!(
        effects
            .iter()
            .any(|(op, r)| op == "settings.voices" && r.profile.as_deref() == Some("voice-owner"))
    );
    s.apply(
        "settings.voices",
        Ok(json!({"available":true,"voices":[{"voice_id":"one","label":"Friendly"}]})),
    );
    assert_eq!(s.voice_catalog["one"], "Friendly");
    assert_eq!(s.edit_mut("tts.elevenlabs.voice_id").text, "private-clone");
    s.apply("settings.voices", Err("No key".into()));
    assert!(s.voice_catalog.is_empty());
    assert_eq!(s.edit_mut("tts.elevenlabs.voice_id").text, "private-clone");
}
