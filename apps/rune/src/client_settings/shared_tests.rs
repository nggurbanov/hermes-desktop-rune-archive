use super::*;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-display-writers-{}", uuid::Uuid::new_v4())))
    }
    fn path(&self) -> PathBuf {
        self.0.join("display.json")
    }
    fn load(&self) -> ClientSettings {
        let mut client = ClientSettings::new();
        client.load_at(self.path());
        client
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn independent_display_and_shortcut_writers_rebase_their_successful_baselines() {
    let root = Workspace::new();
    crate::client::private_write(&root.path(), br#"{"version":1,"bindings":{"session.new":["ctrl+f7"],"plugin.unloaded":["ctrl+f8"]},"future":{"keep":true}}"#).unwrap();
    let mut display = root.load();
    let mut bindings = root.load();
    display.act(Action::Scale(150), true);
    bindings.set_binding("nav.settings".into(), Some("ctrl+f6".into()), true);
    assert_eq!(
        bindings.prefs.chat_scale, 150,
        "the writer adopts the merged display value"
    );
    display.act(Action::Reset("session.new".into()), true);
    assert_eq!(display.prefs.bindings["nav.settings"], vec!["ctrl+f6"]);
    assert!(!display.prefs.bindings.contains_key("session.new"));
    bindings.act(Action::Bubble(35), true);
    assert!(
        !bindings.prefs.bindings.contains_key("session.new"),
        "a later stale writer does not resurrect a reset binding"
    );
    let saved = root.load();
    assert!(saved.error.is_none());
    assert_eq!(saved.prefs.chat_scale, 150);
    assert_eq!(saved.prefs.bubble_transparency, 35);
    assert_eq!(saved.prefs.other["future"], json!({"keep":true}));
    assert_eq!(saved.prefs.bindings["plugin.unloaded"], vec!["ctrl+f8"]);
}

#[test]
fn a_failed_save_retains_the_local_diff_and_retry_merges_other_successful_writes() {
    let root = Workspace::new();
    crate::client::private_write(&root.path(), br#"{"version":1,"future":{"keep":1}}"#).unwrap();
    let mut client = root.load();
    let backup = root.0.with_extension("backup");
    std::fs::rename(&root.0, &backup).unwrap();
    std::fs::write(&root.0, b"blocks the directory").unwrap();
    client.act(Action::Scale(175), true);
    assert!(client.error.is_some());
    assert_eq!(client.prefs.chat_scale, 175);
    assert_eq!(client.saved.chat_scale, 110);
    assert!(client.preferences_wake_after().is_some());
    std::fs::remove_file(&root.0).unwrap();
    std::fs::rename(&backup, &root.0).unwrap();
    let mut other = root.load();
    other.set_binding("nav.settings".into(), Some("ctrl+f6".into()), true);
    client.retry_at = Some(std::time::Instant::now());
    assert!(client.poll_preferences());
    assert!(client.error.is_none());
    assert_eq!(client.prefs.chat_scale, 175);
    assert_eq!(client.prefs.bindings["nav.settings"], vec!["ctrl+f6"]);
    assert_eq!(client.saved, client.prefs);
    assert!(client.preferences_wake_after().is_none());
    assert_eq!(root.load().prefs, client.prefs);
}

#[test]
fn an_invalid_or_future_file_arriving_after_load_is_preserved_and_pending_edits_can_retry() {
    let root = Workspace::new();
    let mut client = root.load();
    let future = br#"{"version":8,"future":{"keep":true}}"#;
    crate::client::private_write(&root.path(), future).unwrap();
    client.act(Action::Scale(150), true);
    assert_eq!(std::fs::read(root.path()).unwrap(), future);
    assert!(client.error.is_some());
    assert_eq!(client.prefs.chat_scale, 150);
    crate::client::private_write(&root.path(), b"bad json").unwrap();
    client.retry_at = Some(std::time::Instant::now());
    assert!(client.poll_preferences());
    assert_eq!(std::fs::read(root.path()).unwrap(), b"bad json");
    assert_eq!(client.prefs.chat_scale, 150);
    crate::client::private_write(&root.path(), br#"{"version":1,"technical_tools":true}"#).unwrap();
    client.retry_at = Some(std::time::Instant::now());
    assert!(client.poll_preferences());
    assert!(client.error.is_none());
    assert!(client.prefs.technical_tools);
    assert_eq!(client.prefs.chat_scale, 150);
}

#[test]
fn explicit_default_choices_and_resets_apply_even_when_the_local_snapshot_was_already_default() {
    let root = Workspace::new();
    let mut stale = root.load();
    let mut other = root.load();
    other.act(Action::Scale(150), true);
    stale.act(Action::Scale(110), true);
    assert_eq!(root.load().prefs.chat_scale, 110);
    let mut stale_reset = root.load();
    other.set_binding("nav.settings".into(), Some("ctrl+f6".into()), true);
    other
        .prefs
        .bindings
        .insert("plugin.new".into(), vec!["ctrl+f8".into()]);
    other.act(Action::Bubble(20), true);
    stale_reset.act(Action::ResetBindings, true);
    let saved = root.load();
    assert!(!saved.prefs.bindings.contains_key("nav.settings"));
    assert_eq!(saved.prefs.bindings["plugin.new"], vec!["ctrl+f8"]);
    assert_eq!(saved.prefs.bubble_transparency, 20);
    let mut reset_one = root.load();
    other.set_binding("session.new".into(), Some("ctrl+f7".into()), true);
    reset_one.act(Action::Reset("session.new".into()), true);
    assert!(!root.load().prefs.bindings.contains_key("session.new"));
    let mut clear = root.load();
    other.set_binding("session.new".into(), Some("ctrl+f6".into()), true);
    clear.act(Action::Clear("session.new".into()), true);
    assert_eq!(
        root.load().prefs.bindings["session.new"],
        Vec::<String>::new()
    );
}

#[test]
fn failed_explicit_unchanged_intent_survives_until_retry_without_overwriting_other_fields() {
    let root = Workspace::new();
    let mut client = root.load();
    let mut other = root.load();
    other.act(Action::Scale(150), true);
    let backup = root.0.with_extension("backup");
    std::fs::rename(&root.0, &backup).unwrap();
    std::fs::write(&root.0, b"blocked").unwrap();
    client.act(Action::Scale(110), true);
    assert!(client.error.is_some());
    assert!(client.touched.contains(&vec!["chat_scale".into()]));
    std::fs::remove_file(&root.0).unwrap();
    std::fs::rename(&backup, &root.0).unwrap();
    other.act(Action::Bubble(45), true);
    client.retry_at = Some(std::time::Instant::now());
    assert!(client.poll_preferences());
    assert_eq!(client.prefs.chat_scale, 110);
    assert_eq!(client.prefs.bubble_transparency, 45);
    assert!(client.touched.is_empty());
    assert_eq!(root.load().prefs, client.prefs);
}
