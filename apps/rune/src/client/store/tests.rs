use super::*;
use crate::client::Preferences;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "hermes-preference-writers-{}",
            uuid::Uuid::new_v4()
        )))
    }
    fn path(&self) -> PathBuf {
        self.0.join("preferences.json")
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stale_window_writers_merge_geometry_visibility_nested_changes_and_unknown_fields() {
    let root = Workspace::new();
    let path = root.path();
    let initial = Preferences::default();
    let mut raw = serde_json::to_value(&initial).unwrap();
    raw["layout"]["future_layout"] = json!({"preserved":true});
    raw["review"] = json!({"diff":{"wrap":false,"context":3},"future":true});
    raw["future"] = json!({"keep":1});
    crate::client::private_write(&path, &serde_json::to_vec(&raw).unwrap()).unwrap();
    let base_a = Preferences::load_at(&path).unwrap();
    let base_b = Preferences::load_at(&path).unwrap();
    let mut a = base_a.clone();
    a.sidebar_width = 215.;
    a.theme = Some(false);
    a.review["diff"]["wrap"] = json!(true);
    a.save_patch_at(&base_a, &path).unwrap();
    let mut b = base_b.clone();
    b.show_statusbar = false;
    b.review["diff"]["context"] = json!(8);
    let merged = b.save_patch_at(&base_b, &path).unwrap();
    assert_eq!(merged.sidebar_width, 215.);
    assert_eq!(merged.theme, Some(false));
    assert!(!merged.show_statusbar);
    assert_eq!(
        merged.review,
        json!({"diff":{"wrap":true,"context":8},"future":true})
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["future"], json!({"keep":1}));
    assert_eq!(saved["layout"]["future_layout"], json!({"preserved":true}));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&root.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn nested_removal_is_distinct_from_null_and_keeps_another_writers_new_key() {
    type Map = BTreeMap<String, Value>;
    let root = Workspace::new();
    let path = root.path();
    let base: Map = serde_json::from_value(
        json!({"bindings":{"reset.me":["ctrl+x"],"keep.me":["ctrl+y"]},"extension":{"a":1,"b":2}}),
    )
    .unwrap();
    crate::client::private_write(&path, &serde_json::to_vec(&base).unwrap()).unwrap();
    let mut first = base.clone();
    first.get_mut("bindings").unwrap()["new.binding"] = json!(["ctrl+z"]);
    first.get_mut("extension").unwrap()["a"] = json!(3);
    save_patch(&path, &base, &first, |_| Ok(())).unwrap();
    let mut second = base.clone();
    second
        .get_mut("bindings")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("reset.me");
    second.get_mut("extension").unwrap()["b"] = Value::Null;
    let merged: Map = save_patch(&path, &base, &second, |_| Ok(())).unwrap();
    assert_eq!(
        merged["bindings"],
        json!({"keep.me":["ctrl+y"],"new.binding":["ctrl+z"]})
    );
    assert_eq!(merged["extension"], json!({"a":3,"b":null}));
}

#[test]
fn an_explicit_unchanged_choice_or_reset_overrides_only_its_named_leaf() {
    type Map = BTreeMap<String, Value>;
    let root = Workspace::new();
    let path = root.path();
    let base: Map = serde_json::from_value(json!({"scale":110,"bindings":{}})).unwrap();
    let foreign =
        json!({"scale":150,"bindings":{"known":["ctrl+x"],"plugin.new":["ctrl+y"]},"future":true});
    crate::client::private_write(&path, &serde_json::to_vec(&foreign).unwrap()).unwrap();
    let merged = save_patch_forced(
        &path,
        &base,
        &base,
        &[
            vec!["scale".into()],
            vec!["bindings".into(), "known".into()],
        ],
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(merged["scale"], 110);
    assert_eq!(merged["bindings"], json!({"plugin.new":["ctrl+y"]}));
    assert_eq!(merged["future"], true);
    let preserved = std::fs::read(&path).unwrap();
    assert!(save_patch_forced(&path, &base, &base, &[vec![]], |_| Ok(())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), preserved);
}

#[test]
fn absent_files_use_defaults_and_invalid_or_future_files_are_never_replaced() {
    let root = Workspace::new();
    let path = root.path();
    let base = Preferences::load_at(&path).unwrap();
    assert!(!path.exists());
    assert_eq!(base.sidebar_width, 237.);
    let mut changed = base.clone();
    changed.sidebar_width = 241.;
    for bytes in [
        b"{".as_slice(),
        b"[]",
        br#"{"version":7,"future":{"keep":true}}"#,
        br#"{"schema_version":2}"#,
        br#"{"sidebar_width":"bad"}"#,
    ] {
        crate::client::private_write(&path, bytes).unwrap();
        assert!(Preferences::load_at(&path).is_err());
        assert!(changed.save_patch_at(&base, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    std::fs::remove_file(&path).unwrap();
    let merged = changed.save_patch_at(&base, &path).unwrap();
    assert_eq!(merged.sidebar_width, 241.);
}

#[test]
fn a_defaulted_object_is_complete_before_a_nested_edit_is_applied() {
    let root = Workspace::new();
    let path = root.path();
    crate::client::private_write(&path, br#"{"future":true}"#).unwrap();
    let base = Preferences::load_at(&path).unwrap();
    let mut changed = base.clone();
    if let crate::layout::Node::Split { weights, .. } = &mut changed.layout {
        weights[0] = 0.4;
    } else {
        panic!("default layout has a split");
    }
    let merged = changed.save_patch_at(&base, &path).unwrap();
    assert_eq!(merged.layout, changed.layout);
    assert_eq!(merged.other["future"], true);
}

struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn wait_for(path: &Path, processes: &mut [Process]) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "child did not signal {}",
            path.display()
        );
        for process in processes.iter_mut() {
            assert!(
                process.0.try_wait().unwrap().is_none(),
                "child exited before its handshake"
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn independent_processes_wait_for_the_os_lock_and_merge_their_stale_snapshots() {
    let root = Workspace::new();
    let path = root.path();
    let base = Preferences::default();
    crate::client::private_write(&path, &serde_json::to_vec(&base).unwrap()).unwrap();
    let held = lock(&path).unwrap();
    let mut children: Vec<Process> = ["geometry", "visibility"]
        .into_iter()
        .map(|mode| {
            Process(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "client::store::tests::shared_preferences_process",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("HERMES_RUNE_PREFS_PROCESS_PATH", &path)
                    .env("HERMES_RUNE_PREFS_PROCESS_MODE", mode)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        })
        .collect();
    for mode in ["geometry", "visibility"] {
        wait_for(&root.0.join(format!("{mode}.ready")), &mut children);
    }
    std::thread::sleep(Duration::from_millis(100));
    for process in &mut children {
        assert!(process.0.try_wait().unwrap().is_none());
    }
    assert_eq!(
        Preferences::load_at(&path).unwrap().sidebar_width,
        base.sidebar_width
    );
    drop(held);
    let deadline = Instant::now() + Duration::from_secs(8);
    for process in &mut children {
        loop {
            if let Some(status) = process.0.try_wait().unwrap() {
                assert!(status.success(), "private writer failed: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "private writer did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let saved = Preferences::load_at(&path).unwrap();
    assert_eq!(saved.sidebar_width, 209.);
    assert_eq!(saved.theme, Some(true));
    assert!(!saved.show_profile_rail);
}

#[test]
#[ignore = "private entry point launched by the passing independent_processes contract"]
fn shared_preferences_process() {
    let path = PathBuf::from(
        std::env::var_os("HERMES_RUNE_PREFS_PROCESS_PATH").expect("private test path"),
    );
    let mode = std::env::var("HERMES_RUNE_PREFS_PROCESS_MODE").unwrap();
    let base = Preferences::load_at(&path).unwrap();
    let mut changed = base.clone();
    match mode.as_str() {
        "geometry" => {
            changed.sidebar_width = 209.;
            changed.theme = Some(true);
        }
        "visibility" => changed.show_profile_rail = false,
        _ => panic!("invalid private writer"),
    }
    std::fs::write(
        path.parent().unwrap().join(format!("{mode}.ready")),
        b"ready",
    )
    .unwrap();
    changed.save_patch_at(&base, &path).unwrap();
}
