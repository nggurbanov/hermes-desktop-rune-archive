use super::*;
use crate::gateway::Connection;
fn root() -> PathBuf {
    std::env::temp_dir().join(format!("hermes-profile-patches-{}", uuid::Uuid::new_v4()))
}
fn connection(token: &str) -> Connection {
    Connection::Url {
        endpoint: "https://same-host.invalid/hermes".into(),
        token: Some(token.into()),
    }
}
#[test]
fn stale_profile_writers_preserve_other_principals_fields_and_explicit_removals() {
    let directory = root();
    let path = directory.join("profile-preferences.json");
    let one = connection("one-private-principal");
    let two = connection("two-private-principal");
    let original =
        json!({"version":1,"mode":"light","profileColor":"blue","future":{"retained":true}});
    let mut seed = ProfilePreferences::load_at(path.clone()).unwrap();
    seed.assign(&one, "bot", original.clone()).unwrap();
    let mut left = ProfilePreferences::load_at(path.clone()).unwrap();
    let mut right = ProfilePreferences::load_at(path.clone()).unwrap();
    let mut first = original.clone();
    first["mode"] = json!("dark");
    left.assign(&one, "bot", first.clone()).unwrap();
    let mut second = original.clone();
    second["profileColor"] = json!("orange");
    right.assign(&one, "bot", second.clone()).unwrap();
    right
        .assign(&two, "bot", json!({"version":1,"mode":"system"}))
        .unwrap();
    let merged = ProfilePreferences::load_at(path.clone()).unwrap();
    let value = merged.get(&one, "bot").unwrap();
    assert_eq!(value["mode"], "dark");
    assert_eq!(value["profileColor"], "orange");
    assert_eq!(value["future"]["retained"], true);
    assert_eq!(merged.get(&two, "bot").unwrap()["mode"], "system");
    left.remove(&one, "bot").unwrap();
    second["profileColor"] = json!("green");
    right.assign(&two, "other", second).unwrap();
    let saved = ProfilePreferences::load_at(path.clone()).unwrap();
    assert!(saved.get(&one, "bot").is_none());
    assert!(saved.get(&two, "bot").is_some());
    assert!(saved.get(&two, "other").is_some());
    // This writer never loaded the other principal's overlay. An explicit
    // removal still removes the latest matching key rather than silently no-op.
    left.remove(&two, "bot").unwrap();
    assert!(
        ProfilePreferences::load_at(path.clone())
            .unwrap()
            .get(&two, "bot")
            .is_none()
    );
    assert!(
        ProfilePreferences::load_at(path.clone())
            .unwrap()
            .get(&two, "other")
            .is_some()
    );
    let raw = std::fs::read_to_string(path).unwrap();
    assert!(!raw.contains("private-principal"));
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn invalid_disk_state_is_preserved_and_pending_overlay_retries_after_repair() {
    let directory = root();
    let path = directory.join("profile-preferences.json");
    let c = connection("retry-principal");
    let mut prefs = ProfilePreferences::load_at(path.clone()).unwrap();
    prefs
        .assign(&c, "bot", json!({"version":1,"mode":"light"}))
        .unwrap();
    let valid = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"invalid existing state").unwrap();
    assert!(
        prefs
            .assign(&c, "bot", json!({"version":1,"mode":"dark"}))
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid existing state");
    assert_eq!(prefs.get(&c, "bot").unwrap()["mode"], "dark");
    assert!(prefs.wake_after().is_some());
    assert!(prefs.storage_error().is_some());
    assert!(ProfilePreferences::load_at(path.clone()).is_err());
    std::fs::write(&path, valid).unwrap();
    prefs.retry_at = Some(Instant::now());
    assert!(prefs.poll_storage().unwrap());
    assert!(prefs.wake_after().is_none());
    assert!(prefs.storage_error().is_none());
    assert_eq!(
        ProfilePreferences::load_at(path)
            .unwrap()
            .get(&c, "bot")
            .unwrap()["mode"],
        "dark"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn close_flush_allows_untouched_unavailable_profiles_but_retains_unsaved_edits() {
    let directory = root();
    let path = directory.join("profile-preferences.json");
    let invalid = b"invalid profile preferences";
    crate::client::private_write(&path, invalid).unwrap();
    // Viewer retains the default unavailable controller after a failed load.
    assert!(ProfilePreferences::load_at(path.clone()).is_err());
    let mut prefs = ProfilePreferences::default();
    prefs.flush_storage().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), invalid);
    let c = connection("close-principal");
    assert!(prefs.assign(&c, "bot", json!({"mode":"dark"})).is_err());
    assert!(prefs.flush_storage().is_err());
    assert_eq!(prefs.get(&c, "bot").unwrap()["mode"], "dark");
    assert_eq!(std::fs::read(&path).unwrap(), invalid);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn close_flush_retries_profile_edits_and_explicit_removals_immediately_after_repair() {
    let directory = root();
    let path = directory.join("profile-preferences.json");
    let one = connection("close-one");
    let two = connection("close-two");
    let mut other = ProfilePreferences::load_at(path.clone()).unwrap();
    other.assign(&one, "bot", json!({"mode":"light"})).unwrap();
    let mut stale = ProfilePreferences::load_at(path.clone()).unwrap();
    other.assign(&two, "bot", json!({"mode":"system"})).unwrap();
    other
        .assign(&two, "keep", json!({"extension":true}))
        .unwrap();
    let valid = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"invalid profile preferences").unwrap();
    assert!(stale.assign(&one, "bot", json!({"mode":"dark"})).is_err());
    assert!(stale.remove(&two, "bot").is_err());
    assert!(stale.wake_after().is_some());
    assert!(stale.flush_storage().is_err());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"invalid profile preferences"
    );
    std::fs::write(&path, valid).unwrap();
    stale.flush_storage().unwrap();
    let loaded = ProfilePreferences::load_at(path).unwrap();
    assert_eq!(loaded.get(&one, "bot").unwrap()["mode"], "dark");
    assert!(loaded.get(&two, "bot").is_none());
    assert_eq!(loaded.get(&two, "keep").unwrap()["extension"], true);
    assert!(stale.wake_after().is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unavailable_overlay_adoption_is_not_an_edit_but_explicit_same_value_assignment_is() {
    let directory = root();
    let path = directory.join("profile-preferences.json");
    let invalid = b"invalid profile preferences";
    crate::client::private_write(&path, invalid).unwrap();
    let mut prefs = ProfilePreferences::load_at(path.clone()).unwrap_or_default();
    let c = connection("inherited-principal");
    let fallback = json!({"version":1,"skin":"opencode","profileColor":null,"mode":"light"});
    prefs.adopt_fallback(&c, "bot", fallback.clone()).unwrap();
    for _ in 0..3 {
        prefs
            .remember_implicit(&c, "bot", fallback.clone())
            .unwrap();
    }
    prefs.flush_storage().unwrap();
    assert!(prefs.get(&c, "bot").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), invalid);
    assert!(prefs.assign(&c, "bot", fallback.clone()).is_err());
    assert!(prefs.flush_storage().is_err());
    assert_eq!(prefs.get(&c, "bot"), Some(&fallback));
    assert_eq!(std::fs::read(&path).unwrap(), invalid);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn read_only_fallbacks_belong_to_each_principal_and_profile_and_never_absorb_edits() {
    let mut prefs = ProfilePreferences::default();
    let one = connection("fallback-one");
    let two = connection("fallback-two");
    let light = json!({"mode":"light"});
    let dark = json!({"mode":"dark"});
    prefs.adopt_fallback(&one, "bot", light.clone()).unwrap();
    prefs.adopt_fallback(&two, "bot", dark.clone()).unwrap();
    prefs.adopt_fallback(&one, "other", dark.clone()).unwrap();
    // Repeated owner restoration cannot redefine the original inherited state.
    prefs.adopt_fallback(&one, "bot", dark.clone()).unwrap();
    prefs.remember_implicit(&one, "bot", light).unwrap();
    prefs.remember_implicit(&two, "bot", dark.clone()).unwrap();
    prefs
        .remember_implicit(&one, "other", dark.clone())
        .unwrap();
    prefs.flush_storage().unwrap();
    // Changing the mode via the implicit appearance recorder is a real edit.
    assert!(prefs.remember_implicit(&one, "bot", dark.clone()).is_err());
    assert_eq!(prefs.get(&one, "bot"), Some(&dark));
    assert!(prefs.get(&two, "bot").is_none());
    assert!(prefs.get(&one, "other").is_none());
    assert!(prefs.flush_storage().is_err());
    prefs.adopt_fallback(&one, "bot", dark.clone()).unwrap();
    assert_eq!(prefs.get(&one, "bot"), Some(&dark));
    assert!(prefs.flush_storage().is_err());
}

#[test]
fn explicit_removal_is_not_suppressed_by_an_unavailable_overlay_fallback() {
    let mut prefs = ProfilePreferences::default();
    let c = connection("removed-fallback");
    let fallback = json!({"mode":"system"});
    prefs.adopt_fallback(&c, "bot", fallback.clone()).unwrap();
    assert!(prefs.remove(&c, "bot").is_err());
    prefs.remember_implicit(&c, "bot", fallback).unwrap();
    assert!(prefs.flush_storage().is_err());
}
