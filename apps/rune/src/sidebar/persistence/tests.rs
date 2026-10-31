use super::*;
use crate::sidebar::{Action, Grouping, Ordering};
use serde_json::{Value, json};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-sidebar-writers-{}", uuid::Uuid::new_v4())))
    }
    fn path(&self) -> PathBuf {
        self.0.join("sidebar.json")
    }
    fn write(&self, bytes: &[u8]) {
        crate::client::private_write(&self.path(), bytes).unwrap();
    }
    fn state(&self) -> SidebarState {
        let mut state = SidebarState::new();
        state.load_preferences_at(self.path());
        state
    }
    fn saved(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.path()).unwrap()).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stale_sidebar_controllers_merge_filters_and_grouping_and_adopt_the_saved_baseline() {
    let root = Workspace::new();
    let mut a = root.state();
    let mut b = root.state();
    a.action(Action::Status("Working".into()));
    b.action(Action::Grouping(Grouping::Project));
    assert!(b.prefs.status.contains("Working"));
    assert!(b.prefs.grouping == Grouping::Project);
    a.action(Action::Profile("research".into()));
    assert!(a.prefs.grouping == Grouping::Project);
    b.action(Action::Ordering(Ordering::Cost));
    a.action(Action::Toggle("tokens"));
    let saved = root.saved();
    assert_eq!(saved["grouping"], "project");
    assert_eq!(saved["ordering"], "cost");
    assert_eq!(saved["status"], json!(["Working"]));
    assert_eq!(saved["profiles"], json!(["research"]));
    assert_eq!(saved["tokens"], true);
    assert!(a.prefs.ordering == Ordering::Cost);
    assert!(a.storage_wake_after().is_none());
    assert!(!a.poll_storage());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&root.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn reset_and_same_value_selection_are_explicit_known_field_intents_preserving_extensions() {
    let root = Workspace::new();
    root.write(br#"{"version":1,"extension":{"first":1,"second":2}}"#);
    let mut a = root.state();
    let mut b = root.state();
    b.action(Action::Grouping(Grouping::Status));
    b.action(Action::Toggle("cards"));
    b.prefs.other.get_mut("extension").unwrap()["second"] = json!(3);
    b.save_preferences(&[]);
    a.action(Action::ResetView);
    let saved = root.saved();
    assert_eq!(saved["grouping"], "date");
    assert_eq!(saved["cards"], false);
    assert_eq!(saved["version"], 1);
    assert_eq!(saved["extension"], json!({"first":1,"second":3}));
    assert_eq!(a.prefs.other["extension"], json!({"first":1,"second":3}));
    b.action(Action::Grouping(Grouping::Profile));
    // a still displays its default selection. Selecting it is a new intent.
    a.action(Action::Grouping(Grouping::Date));
    assert_eq!(root.saved()["grouping"], "date");
    assert_eq!(root.saved()["extension"], json!({"first":1,"second":3}));
}

#[test]
fn malformed_future_and_unreadable_initial_files_stay_read_only_after_later_repair() {
    let invalid = [
        b"{".as_slice(),
        b"[]".as_slice(),
        b"null".as_slice(),
        br#"{"grouping":"future"}"#.as_slice(),
        br#"{"version":2,"extension":{"keep":true}}"#.as_slice(),
        br#"{"schema_version":"next"}"#.as_slice(),
    ];
    for bytes in invalid {
        let root = Workspace::new();
        root.write(bytes);
        let mut state = root.state();
        assert!(state.notice.is_some());
        state.action(Action::Grouping(Grouping::Project));
        state.action(Action::ResetView);
        assert_eq!(std::fs::read(root.path()).unwrap(), bytes);
        assert!(state.storage_wake_after().is_none());
        assert!(!state.poll_storage());
        root.write(br#"{"extension":{"repaired":true}}"#);
        let repaired = std::fs::read(root.path()).unwrap();
        state.action(Action::Toggle("cards"));
        assert_eq!(std::fs::read(root.path()).unwrap(), repaired);
        assert!(!state.poll_storage());
        // Only a new successful explicit load can grant write access.
        state.load_preferences_at(root.path());
        state.action(Action::Grouping(Grouping::Profile));
        assert_eq!(root.saved()["grouping"], "profile");
        assert_eq!(root.saved()["extension"], json!({"repaired":true}));
    }
    let root = Workspace::new();
    std::fs::create_dir_all(root.path()).unwrap();
    let mut state = root.state();
    state.action(Action::Toggle("cards"));
    std::fs::remove_dir(root.path()).unwrap();
    root.write(b"{}");
    state.action(Action::Toggle("tokens"));
    assert_eq!(std::fs::read(root.path()).unwrap(), b"{}");
    assert!(state.storage_wake_after().is_none());
}

#[test]
fn failed_writes_retain_all_intents_and_retry_after_repair_without_losing_other_changes() {
    let root = Workspace::new();
    let mut a = root.state();
    let mut b = root.state();
    std::fs::create_dir_all(root.path()).unwrap();
    a.action(Action::Toggle("cards"));
    a.action(Action::Grouping(Grouping::Date));
    assert!(a.notice.is_some());
    assert!(a.storage_wake_after().is_some());
    assert!(!a.poll_storage());
    std::fs::remove_dir(root.path()).unwrap();
    b.action(Action::Grouping(Grouping::Status));
    b.action(Action::Ordering(Ordering::Cost));
    a.preferences_storage.retry_at = Some(Instant::now());
    assert!(a.poll_storage());
    assert!(a.notice.is_none());
    assert!(a.storage_wake_after().is_none());
    assert_eq!(root.saved()["cards"], true);
    assert_eq!(root.saved()["grouping"], "date");
    assert_eq!(root.saved()["ordering"], "cost");
    assert!(a.prefs.ordering == Ordering::Cost);
    // A later save must diff from the merged snapshot, not reapply old values.
    b.action(Action::Status("Working".into()));
    a.action(Action::Profile("research".into()));
    assert_eq!(root.saved()["status"], json!(["Working"]));
    assert_eq!(root.saved()["profiles"], json!(["research"]));
}

#[test]
fn writes_validate_a_newer_schema_before_replacing_it_and_demo_never_persists() {
    let root = Workspace::new();
    let mut state = root.state();
    let future = br#"{"version":3,"extension":{"keep":true}}"#;
    root.write(future);
    state.action(Action::Toggle("cards"));
    assert_eq!(std::fs::read(root.path()).unwrap(), future);
    assert!(state.storage_wake_after().is_some());
    root.write(br#"{"extension":{"repaired":true},"ordering":"cost"}"#);
    state.preferences_storage.retry_at = Some(Instant::now());
    assert!(state.poll_storage());
    assert_eq!(root.saved()["cards"], true);
    assert_eq!(root.saved()["ordering"], "cost");
    assert_eq!(root.saved()["extension"], json!({"repaired":true}));
    let demo_root = Workspace::new();
    let mut demo = demo_root.state();
    demo.demo = true;
    demo.action(Action::Toggle("cards"));
    demo.action(Action::ResetView);
    assert!(!demo_root.path().exists());
    assert!(!demo.poll_storage());
    assert!(demo.storage_wake_after().is_none());
}

#[test]
fn close_flush_preserves_untouched_read_only_sidebar_but_reports_local_intents() {
    for bytes in [
        b"invalid sidebar".as_slice(),
        br#"{"version":9,"future":true}"#.as_slice(),
    ] {
        let root = Workspace::new();
        root.write(bytes);
        let mut state = root.state();
        assert!(state.notice.is_some());
        state.flush_storage().unwrap();
        assert_eq!(std::fs::read(root.path()).unwrap(), bytes);
        state.action(Action::Grouping(Grouping::Project));
        assert!(state.flush_storage().is_err());
        assert!(state.prefs.grouping == Grouping::Project);
        assert_eq!(std::fs::read(root.path()).unwrap(), bytes);
    }
}

#[test]
fn close_flush_retries_sidebar_intents_immediately_after_repair() {
    let root = Workspace::new();
    let mut state = root.state();
    let invalid = br#"{"version":9,"future":true}"#;
    root.write(invalid);
    state.action(Action::Grouping(Grouping::Date));
    state.action(Action::Toggle("cards"));
    assert!(state.storage_wake_after().is_some());
    assert!(state.flush_storage().is_err());
    assert_eq!(std::fs::read(root.path()).unwrap(), invalid);
    root.write(br#"{"grouping":"status","ordering":"cost","extension":{"keep":true}}"#);
    state.flush_storage().unwrap();
    let saved = root.saved();
    assert_eq!(saved["grouping"], "date");
    assert_eq!(saved["cards"], true);
    assert_eq!(saved["ordering"], "cost");
    assert_eq!(saved["extension"]["keep"], true);
    assert!(state.storage_wake_after().is_none());
}

#[test]
fn show_all_compatibility_merge_reload_and_reset() {
    let root = Workspace::new();
    root.write(br#"{"grouping":"project","extension":{"keep":true}}"#);
    let mut a = root.state();
    let mut b = root.state();
    assert!(!a.prefs.show_all_sessions);
    a.action(Action::Toggle("show_all_sessions"));
    b.action(Action::Toggle("tokens"));
    let reloaded = root.state();
    assert!(reloaded.prefs.show_all_sessions);
    assert!(reloaded.prefs.tokens);
    assert_eq!(root.saved()["extension"]["keep"], true);
    b.action(Action::ResetView);
    let reloaded = root.state();
    assert!(!reloaded.prefs.show_all_sessions);
    assert!(!reloaded.prefs.tokens);
    assert_eq!(root.saved()["extension"]["keep"], true);
}
