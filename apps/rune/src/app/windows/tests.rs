use super::*;
use crate::client::windows::SourceDescriptor;
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-window-consumer-{}", uuid::Uuid::new_v4())))
    }
    fn store(&self) -> WindowStore {
        WindowStore::at(self.0.clone())
    }
    fn viewer(&self, profile: &str, session: Option<&str>) -> Viewer {
        let mut s = Viewer::empty();
        let route = WindowRoute {
            profile: profile.into(),
            session: session.map(str::to_owned),
            ..Default::default()
        };
        s.window.lease = Some(
            self.store()
                .mint_peer(BootstrapKind::PeerInherited, route)
                .unwrap(),
        );
        s.profile = profile.into();
        s.selected_session = session.map(str::to_owned);
        s.connection = Some(connection("one"));
        s.connection_form = false;
        s.init_window_storage();
        s.window.started = true;
        s
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn connection(token: &str) -> Connection {
    Connection::Url {
        endpoint: "https://window-fixture.invalid/hermes".into(),
        token: Some(token.into()),
    }
}
#[test]
fn mounted_windows_keep_distinct_source_incarnations_actual_inputs_and_presentation() {
    let root = Workspace::new();
    let mut a = root.viewer("research", Some("same-session"));
    let mut b = root.viewer("research", Some("same-session"));
    let first = a.composer_scope();
    let second = b.composer_scope();
    assert_ne!(first.connection, second.connection);
    assert_ne!(a.composer.editor_id(), b.composer.editor_id());
    a.composer
        .draft_mut(first.clone())
        .text
        .replace_text("left unsent");
    b.composer
        .draft_mut(second.clone())
        .text
        .replace_text("right unsent");
    a.theme = Some(true);
    b.theme = Some(false);
    a.save_window_record(false).unwrap();
    b.save_window_record(false).unwrap();
    let rows = root.store().records().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| r.editor == *a.composer.editor_id()
        && r.presentation.as_ref().unwrap().theme == Some(true)));
    assert!(rows.iter().any(|r| r.editor == *b.composer.editor_id()
        && r.presentation.as_ref().unwrap().theme == Some(false)));
    b.composer.draft_mut(second).text.insert(" edited");
    assert_eq!(a.composer.draft(&first).unwrap().text.text, "left unsent");
    let serialized = std::fs::read_to_string(root.0.join("windows.json")).unwrap();
    assert!(!serialized.contains("\"token\""));
    assert!(!serialized.contains("left unsent"));
    assert!(matches!(
        rows[0].route.source,
        Some(SourceDescriptor::Remote {
            credential_required: true,
            ..
        })
    ));
}
#[test]
fn an_explicit_new_window_keeps_its_captured_source_profile_and_presentation_after_navigation() {
    let root = Workspace::new();
    let mut s = root.viewer("research", None);
    s.theme = Some(true);
    let principal = crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap();
    s.request_new_window();
    assert_eq!(s.window.requested.len(), 1);
    s.profile = "archive".into();
    s.use_connection(connection("two"));
    s.theme = Some(false);
    let request = s.window.requested.pop_front().unwrap();
    assert_eq!(request.route.profile, "research");
    assert_eq!(request.presentation.theme, Some(true));
    let Some(Source::Direct(captured)) = request.source else {
        panic!("URL source was captured directly")
    };
    assert_eq!(crate::client::connection_key(&captured).unwrap(), principal);
    assert_ne!(
        crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap(),
        principal
    );
}
#[test]
fn closed_window_records_preserve_other_live_windows_and_canonical_bot_identity() {
    let root = Workspace::new();
    let mut a = root.viewer("research", None);
    let mut b = root.viewer("archive", Some("archive-chat"));
    a.bot_empty = true;
    a.save_window_record(true).unwrap();
    b.save_window_record(false).unwrap();
    let id = a.window.lease.as_ref().unwrap().id().clone();
    let rows = root.store().records().unwrap();
    let closed = rows.iter().find(|r| r.id == id).unwrap();
    assert!(closed.closed);
    assert!(closed.route.canonical_bot);
    assert!(closed.route.session.is_none());
    assert!(rows.iter().any(|r| r.id != id
        && !r.closed
        && r.route.profile == "archive"
        && r.route.session.as_deref() == Some("archive-chat")));
}
#[test]
fn hydrating_restored_window_presentation_is_idempotent_and_does_not_toggle_its_panes() {
    let mut s = Viewer::empty();
    let mut prefs = crate::client::Preferences::default();
    prefs.sidebar_open = false;
    prefs.inspector_open = true;
    prefs.sidebar_width = 290.;
    prefs.theme = Some(true);
    s.hydrate_client_preferences(prefs.clone());
    s.hydrate_client_preferences(prefs);
    assert!(!s.sidebar.is_open());
    assert!(s.inspector.is_open());
    assert_eq!(s.sidebar.width(), 290.);
    assert_eq!(s.theme, Some(true));
}

#[test]
fn normal_close_preserves_an_untouched_unavailable_presentation_but_vetoes_a_local_edit() {
    let root = Workspace::new();
    let mut s = root.viewer("research", None);
    s.preferences_writable = false;
    s.preferences_read_only_baseline = Some(s.client_preferences_snapshot());
    assert!(may_close(&mut s, &mut Tx::default()));
    s.theme = Some(true);
    assert!(!may_close(&mut s, &mut Tx::default()));
    assert!(
        s.client_error
            .as_deref()
            .unwrap()
            .contains("could not be saved")
    );
    assert_eq!(s.theme, Some(true));
    assert!(!root.store().records().unwrap()[0].closed);
    s.theme = s.preferences_read_only_baseline.as_ref().unwrap().theme;
    assert!(may_close(&mut s, &mut Tx::default()));
}

#[test]
fn a_pending_window_write_respects_backoff_and_close_forces_the_latest_presentation() {
    let root = Workspace::new();
    let mut s = root.viewer("research", None);
    s.save_window_record(false).unwrap();
    let path = root.0.join("windows.json");
    let valid = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"invalid window state").unwrap();
    s.theme = Some(true);
    s.poll_windows(&mut Tx::default());
    assert!(s.window.lease.as_ref().unwrap().pending().is_some());
    assert!(s.window.lease.as_ref().unwrap().wake_after().is_some());
    std::fs::write(&path, &valid).unwrap();
    s.theme = Some(false);
    s.poll_windows(&mut Tx::default());
    assert_eq!(std::fs::read(&path).unwrap(), valid);
    s.preferences_writable = false;
    s.preferences_read_only_baseline = Some(s.client_preferences_snapshot());
    assert!(may_close(&mut s, &mut Tx::default()));
    let row = &root.store().records().unwrap()[0];
    assert_eq!(row.presentation.as_ref().unwrap().theme, Some(false));
    assert!(s.window.lease.as_ref().unwrap().pending().is_none());
}

#[test]
fn an_explicit_launch_profile_keeps_the_restored_source_without_its_old_conversation() {
    let mut s = Viewer::empty();
    s.profile = "research".into();
    s.exact_connection_profile = true;
    let source = SourceDescriptor::from_connection(&connection("launch")).unwrap();
    s.restore_launch_route(WindowRoute {
        source: Some(source.clone()),
        profile: "archive".into(),
        session: Some("old-archive-chat".into()),
        canonical_bot: false,
    });
    let target = s.window.boot_target.as_ref().unwrap();
    assert_eq!(s.profile, "research");
    assert_eq!(target.profile, "research");
    assert_eq!(target.source, Some(source));
    assert!(target.session.is_none());
    assert!(!target.canonical_bot);
    s.window.boot_target = None;
    s.connection = Some(connection("explicit"));
    s.restore_launch_route(WindowRoute {
        profile: "archive".into(),
        ..Default::default()
    });
    assert!(s.window.boot_target.is_none());
    assert_eq!(s.profile, "research");
}
#[test]
fn an_ordinary_restored_launch_keeps_its_exact_profile_and_conversation() {
    let mut s = Viewer::empty();
    s.restore_launch_route(WindowRoute {
        profile: "archive".into(),
        session: Some("stored".into()),
        ..Default::default()
    });
    assert_eq!(s.profile, "archive");
    assert!(s.exact_connection_profile);
    assert_eq!(
        s.window.boot_target.as_ref().unwrap().session.as_deref(),
        Some("stored")
    );
}
