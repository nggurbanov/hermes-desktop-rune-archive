use super::*;
use serde_json::json;
use std::sync::{Arc, Barrier};
const ONE: &str = "11111111-1111-4111-8111-111111111111";
const TWO: &str = "22222222-2222-4222-8222-222222222222";
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-startup-default-{}", uuid::Uuid::new_v4())))
    }
    fn path(&self) -> PathBuf {
        self.0.join("startup.json")
    }
    fn load(&self) -> Startup {
        Startup::load_at(self.path())
    }
    fn write(&self, value: &Value) {
        super::super::private_write(&self.path(), &serde_json::to_vec(value).unwrap()).unwrap();
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
fn route(id: Option<&str>, profile: &str) -> Route {
    Route::new(id.map(str::to_owned), profile.to_owned()).unwrap()
}
fn observe(store: &mut Startup) -> Result<bool, String> {
    store.next_read = Instant::now();
    store.poll()
}
#[test]
fn logical_routes_validate_exact_profile_names_and_distinguish_all_default_states_after_restart() {
    let root = Workspace::new();
    let mut store = root.load();
    assert_eq!(store.get().unwrap(), None);
    assert!(!root.path().exists());
    for value in [
        route(None, "work"),
        route(Some("local"), "work"),
        route(Some(ONE), "work"),
    ] {
        store.set(value.clone()).unwrap();
        assert_eq!(root.load().get().unwrap(), Some(value));
    }
    for profile in [
        "",
        " Work",
        "work ",
        "Work",
        "_work",
        "-work",
        "work/path",
        "work.x",
        "работа",
    ] {
        assert!(Route::new(None, profile.into()).is_err(), "{profile:?}");
    }
    assert!(Route::new(None, "a".repeat(64)).is_ok());
    assert!(Route::new(None, "a".repeat(65)).is_err());
    assert!(Route::new(None, "0_a-b".into()).is_ok());
    for id in [
        "",
        " local",
        "remote-work",
        "http://fixture.invalid?token=private",
    ] {
        assert!(Route::new(Some(id.into()), "work".into()).is_err());
    }
    assert!(serde_json::from_value::<Route>(json!({"profile":"work"})).is_err());
    assert!(
        serde_json::from_value::<Route>(json!({"connectionId":true,"profile":"work"})).is_err()
    );
    assert_eq!(
        serde_json::to_value(route(None, "work")).unwrap(),
        json!({"connectionId":null,"profile":"work"})
    );
    store.clear().unwrap();
    assert_eq!(root.load().get().unwrap(), None);
}
#[test]
fn explicit_same_value_set_and_clear_win_over_foreign_writes_without_losing_unknown_fields() {
    let root = Workspace::new();
    root.write(&json!({"version":1,"revision":7,"extension":{"keep":[1,2]},"defaultRoute":{"connectionId":ONE,"profile":"work","futureRoute":{"keep":true}}}));
    let mut stale = root.load();
    let mut other = root.load();
    other.set(route(Some(TWO), "other")).unwrap();
    stale.set(route(Some(ONE), "work")).unwrap();
    assert_eq!(root.load().get().unwrap(), Some(route(Some(ONE), "work")));
    assert_eq!(root.saved()["extension"], json!({"keep":[1,2]}));
    assert_eq!(root.saved()["defaultRoute"]["futureRoute"]["keep"], true);
    stale.clear().unwrap();
    other.set(route(Some(TWO), "other")).unwrap();
    stale.clear().unwrap();
    assert!(root.saved()["defaultRoute"].is_null());
    assert_eq!(root.saved()["extension"], json!({"keep":[1,2]}));
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
fn acknowledged_profile_changes_and_forgets_compare_the_latest_source_and_profile() {
    let root = Workspace::new();
    let mut current = root.load();
    current.set(route(Some(ONE), "work")).unwrap();
    let mut stale = root.load();
    current.set(route(Some(TWO), "work")).unwrap();
    stale
        .after_profile(
            Some(ONE.into()),
            "work".into(),
            Some("renamed".into()),
            true,
        )
        .unwrap();
    assert_eq!(stale.get().unwrap(), Some(route(Some(TWO), "work")));
    stale.after_forget(ONE.into(), true).unwrap();
    assert_eq!(root.load().get().unwrap(), Some(route(Some(TWO), "work")));
    stale
        .after_profile(Some(TWO.into()), "other".into(), None, true)
        .unwrap();
    assert_eq!(root.load().get().unwrap(), Some(route(Some(TWO), "work")));
    stale
        .after_profile(
            Some(TWO.into()),
            "work".into(),
            Some("renamed".into()),
            true,
        )
        .unwrap();
    assert_eq!(
        root.load().get().unwrap(),
        Some(route(Some(TWO), "renamed"))
    );
    stale
        .after_profile(Some(TWO.into()), "renamed".into(), None, true)
        .unwrap();
    assert_eq!(root.load().get().unwrap(), None);
    stale.set(route(Some(ONE), "work")).unwrap();
    stale.after_forget(ONE.into(), true).unwrap();
    assert_eq!(root.load().get().unwrap(), None);
}
#[test]
fn legacy_null_source_and_registered_local_are_not_interchangeable_and_failed_events_do_nothing() {
    let root = Workspace::new();
    let mut store = root.load();
    store.set(route(None, "work")).unwrap();
    let before = std::fs::read(root.path()).unwrap();
    store
        .after_profile(None, "work".into(), Some("renamed".into()), false)
        .unwrap();
    store.after_forget("local".into(), false).unwrap();
    assert_eq!(std::fs::read(root.path()).unwrap(), before);
    store
        .after_profile(
            Some("local".into()),
            "work".into(),
            Some("renamed".into()),
            true,
        )
        .unwrap();
    store.after_forget("local".into(), true).unwrap();
    assert_eq!(store.get().unwrap(), Some(route(None, "work")));
    store
        .after_profile(None, "work".into(), Some("renamed".into()), true)
        .unwrap();
    assert_eq!(store.get().unwrap(), Some(route(None, "renamed")));
    store.set(route(Some("local"), "work")).unwrap();
    store
        .after_profile(None, "work".into(), None, true)
        .unwrap();
    assert_eq!(store.get().unwrap(), Some(route(Some("local"), "work")));
}
#[test]
fn conditional_rename_and_explicit_new_default_are_atomic_across_concurrent_writers() {
    let root = Workspace::new();
    let mut seed = root.load();
    for _ in 0..8 {
        seed.set(route(Some(ONE), "work")).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let mut rename = root.load();
        let mut choice = root.load();
        let a = barrier.clone();
        let left = std::thread::spawn(move || {
            a.wait();
            rename
                .after_profile(
                    Some(ONE.into()),
                    "work".into(),
                    Some("renamed".into()),
                    true,
                )
                .unwrap();
        });
        let right = std::thread::spawn(move || {
            barrier.wait();
            choice.set(route(Some(TWO), "work")).unwrap();
        });
        left.join().unwrap();
        right.join().unwrap();
        assert_eq!(root.load().get().unwrap(), Some(route(Some(TWO), "work")));
    }
}
#[test]
fn other_windows_refresh_only_the_authoritative_mirror_and_preserve_failed_read_errors() {
    let root = Workspace::new();
    let mut observer = root.load();
    let mut writer = root.load();
    writer.set(route(Some(ONE), "work")).unwrap();
    assert_eq!(observer.get().unwrap(), None);
    assert!(observe(&mut observer).unwrap());
    assert_eq!(observer.get().unwrap(), Some(route(Some(ONE), "work")));
    assert!(!observe(&mut observer).unwrap());
    root.write(&json!({"version":8,"defaultRoute":null,"future":true}));
    assert!(observe(&mut observer).is_err());
    assert!(observer.error().is_some());
    assert_eq!(observer.get().unwrap(), Some(route(Some(ONE), "work")));
    root.write(&json!({"version":1,"revision":50,"defaultRoute":null}));
    assert!(observe(&mut observer).unwrap());
    assert_eq!(observer.get().unwrap(), None);
    assert!(observer.error().is_none());
}
#[test]
fn invalid_initial_state_is_not_absence_and_failed_intent_flushes_only_after_valid_repair() {
    for invalid in [
        b"invalid json".as_slice(),
        br#"{"version":9,"future":true}"#.as_slice(),
        br#"{"version":1,"defaultRoute":{"profile":"work"}}"#.as_slice(),
    ] {
        let root = Workspace::new();
        super::super::private_write(&root.path(), invalid).unwrap();
        let mut store = root.load();
        assert!(store.get().is_err());
        store.flush().unwrap();
        assert_eq!(std::fs::read(root.path()).unwrap(), invalid);
        let wanted = route(Some(ONE), "work");
        assert!(store.set(wanted.clone()).is_err());
        assert!(store.wake_after().is_some());
        assert!(store.get().is_err());
        assert_eq!(std::fs::read(root.path()).unwrap(), invalid);
        root.write(&json!({"version":1,"extension":{"keep":true},"defaultRoute":null}));
        store.flush().unwrap();
        assert_eq!(store.get().unwrap(), Some(wanted.clone()));
        assert_eq!(root.load().get().unwrap(), Some(wanted));
        assert_eq!(root.saved()["extension"]["keep"], true);
        assert!(store.error().is_none());
    }
}
#[test]
fn failed_pending_maintenance_cannot_clear_a_newer_foreign_default_after_repair() {
    let root = Workspace::new();
    let mut store = root.load();
    store.set(route(Some(ONE), "work")).unwrap();
    let valid = std::fs::read(root.path()).unwrap();
    std::fs::write(root.path(), b"invalid json").unwrap();
    assert!(store.after_forget(ONE.into(), true).is_err());
    assert_eq!(store.get().unwrap(), Some(route(Some(ONE), "work")));
    std::fs::write(root.path(), valid).unwrap();
    let mut other = root.load();
    other.set(route(Some(TWO), "work")).unwrap();
    store.flush().unwrap();
    assert_eq!(store.get().unwrap(), Some(route(Some(TWO), "work")));
    assert_eq!(root.load().get().unwrap(), Some(route(Some(TWO), "work")));
}
#[test]
fn failed_explicit_intents_keep_the_previous_default_and_latest_local_choice_retries() {
    let root = Workspace::new();
    let mut store = root.load();
    store.set(route(Some(ONE), "work")).unwrap();
    let valid = std::fs::read(root.path()).unwrap();
    std::fs::remove_file(root.path()).unwrap();
    std::fs::create_dir(root.path()).unwrap();
    assert!(store.set(route(Some(TWO), "other")).is_err());
    assert!(store.clear().is_err());
    assert_eq!(store.get().unwrap(), Some(route(Some(ONE), "work")));
    assert!(store.error().is_some());
    std::fs::remove_dir(root.path()).unwrap();
    std::fs::write(root.path(), valid).unwrap();
    store.retry_at = Some(Instant::now());
    assert!(store.poll().unwrap());
    assert_eq!(root.load().get().unwrap(), None);
    assert!(store.error().is_none());
}
#[test]
fn snakecase_aliases_are_read_and_unknown_route_fields_survive_successful_rename() {
    let root = Workspace::new();
    root.write(&json!({"version":1,"default_route":{"connection_id":ONE,"profile":"work","future":{"kept":true}},"unrelated":[1,2]}));
    let mut store = root.load();
    assert_eq!(store.get().unwrap(), Some(route(Some(ONE), "work")));
    store
        .after_profile(
            Some(ONE.into()),
            "work".into(),
            Some("renamed".into()),
            true,
        )
        .unwrap();
    let saved = root.saved();
    assert_eq!(saved["defaultRoute"]["connectionId"], ONE);
    assert_eq!(saved["defaultRoute"]["future"]["kept"], true);
    assert_eq!(saved["unrelated"], json!([1, 2]));
    assert!(saved.get("default_route").is_none());
    assert!(saved["defaultRoute"].get("connection_id").is_none());
}
