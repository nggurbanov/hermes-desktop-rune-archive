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
