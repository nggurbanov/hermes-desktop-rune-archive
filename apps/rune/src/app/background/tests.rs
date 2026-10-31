use super::*;
fn connection(name: &str) -> Connection {
    Connection::Url {
        endpoint: format!("https://{name}.background-fixture.invalid"),
        token: None,
    }
}
fn parked_viewer() -> (Viewer, DraftScope, DraftScope) {
    let mut s = Viewer::empty();
    crate::app::reader_scene(&mut s);
    s.draft_storage_available = false;
    s.connection = Some(connection("a"));
    s.connection_epoch = windows::source_epoch();
    s.profile = "default".into();
    s.selected_session = Some("same".into());
    s.tabs.clear();
    s.init_editors(std::sync::Arc::new(|| {}));
    assert!(s.ensure_editor());
    let a = s.composer_scope();
    s.conversation
        .fixture(a.clone(), &serde_json::json!({"running":true}))
        .unwrap();
    assert!(s.use_connection_for_profile(connection("b"), Some("default".into())));
    assert!(s.ensure_editor());
    let b = s.composer_scope();
    assert_ne!(a.connection, b.connection);
    assert!(s.binding_for_source_epoch(a.connection).is_some());
    assert!(s.retained_source_epochs().contains(&a.connection));
    s.fixture = false;
    (s, a, b)
}
#[test]
fn a_retained_source_stream_can_trigger_its_monitor_after_b_becomes_foreground() {
    let (mut s, a, b) = parked_viewer();
    let before = s.background.revision;
    s.trigger_background(a.clone(), "wrong-runtime".into());
    assert_eq!(s.background.revision, before);
    s.trigger_background(a.clone(), "same".into());
    assert!(s.background.revision > before);
    assert_eq!(s.composer_scope(), b);
    assert_eq!(
        crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap(),
        crate::client::connection_key(&connection("b")).unwrap()
    );
    let before = s.background.revision;
    s.retired_profiles.insert((a.connection, a.profile.clone()));
    s.trigger_background(a, "same".into());
    assert_eq!(s.background.revision, before);
}

#[test]
fn parked_runtime_reconnect_failure_stays_with_a_while_current_failure_uses_current_client_error() {
    let (mut s, a, b) = parked_viewer();
    s.client_error = Some("Existing foreground B issue".into());
    s.apply_background_update(crate::background::Update::RuntimeGone {
        scope: a.clone(),
        runtime: "same".into(),
    });
    assert_eq!(
        s.client_error.as_deref(),
        Some("Existing foreground B issue")
    );
    assert_eq!(
        s.chat.views[&a].error.as_deref(),
        Some("Conversation transport is not initialized")
    );
    assert!(!s.chat.views.contains_key(&b));
    let current = DraftScope {
        session: Some("current-b".into()),
        draft_origin: None,
        ..b
    };
    s.conversation
        .fixture(current.clone(), &serde_json::json!({}))
        .unwrap();
    s.apply_background_update(crate::background::Update::RuntimeGone {
        scope: current,
        runtime: "current-b".into(),
    });
    assert_eq!(
        s.client_error.as_deref(),
        Some("Conversation transport is not initialized")
    );
}
