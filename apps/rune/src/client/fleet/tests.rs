use super::*;
use serde_json::json;
struct Private(std::path::PathBuf);
impl Private {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("hermes-fleet-prefs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self) -> PathBuf {
        self.0.join("fleet.json")
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn stale_windows_preserve_unrelated_source_fields_and_unknown_data() {
    let p = Private::new();
    std::fs::write(
        p.file(),
        json!({"version":1,"future":{"keep":true},"groups":{"source-a":{"alien":42}}}).to_string(),
    )
    .unwrap();
    let mut a = Preferences::at_path(p.file());
    let mut b = Preferences::at_path(p.file());
    a.set_alias("source-a".into(), Some("Desk".into())).unwrap();
    b.set_collapsed("source-b".into(), true).unwrap();
    b.set_color("research".into(), Some(150)).unwrap();
    a.set_profile_order(vec!["research".into(), "archive".into()])
        .unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p.file()).unwrap()).unwrap();
    assert_eq!(saved["future"]["keep"], true);
    assert_eq!(saved["groups"]["source-a"]["alien"], 42);
    assert_eq!(saved["groups"]["source-a"]["alias"], "Desk");
    assert_eq!(saved["groups"]["source-b"]["collapsed"], true);
    a.observe().unwrap();
    assert_eq!(a.profile_hue("research"), Some(150));
    assert!(a.collapsed("source-b"));
}
#[test]
fn explicit_clear_and_false_win_over_a_peers_newer_values() {
    let p = Private::new();
    let mut a = Preferences::at_path(p.file());
    let mut b = Preferences::at_path(p.file());
    b.set_color("same".into(), Some(240)).unwrap();
    b.set_collapsed("a".into(), true).unwrap();
    b.set_alias("a".into(), Some("Other".into())).unwrap();
    a.set_color("same".into(), None).unwrap();
    a.set_collapsed("a".into(), false).unwrap();
    a.set_alias("a".into(), None).unwrap();
    b.observe().unwrap();
    assert!(!b.collapsed("a"));
    assert_eq!(b.group_alias("a"), None);
    assert_ne!(b.profile_hue("same"), Some(240));
}
#[test]
fn future_and_corrupt_files_are_preserved_and_exact_failed_intent_retries() {
    for raw in ["broken", "{\"version\":99,\"future\":7}"] {
        let p = Private::new();
        std::fs::write(p.file(), raw).unwrap();
        let mut prefs = Preferences::at_path(p.file());
        assert!(prefs.error().is_some());
        assert!(
            prefs
                .set_alias("source".into(), Some("Retained".into()))
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(p.file()).unwrap(), raw);
        std::fs::write(p.file(), "{\"version\":1,\"other\":true}").unwrap();
        prefs.flush_storage().unwrap();
        assert_eq!(prefs.group_alias("source"), Some("Retained"));
        assert!(prefs.error().is_none());
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(p.file()).unwrap()).unwrap();
        assert_eq!(saved["other"], true);
    }
}
#[test]
fn name_colors_and_order_are_shared_but_source_groups_are_independent() {
    let mut p = Preferences::default();
    p.set_color("research".into(), Some(30)).unwrap();
    p.set_alias("a".into(), Some("Desk".into())).unwrap();
    p.set_collapsed("a".into(), true).unwrap();
    assert_eq!(p.profile_hue("research"), Some(30));
    assert_eq!(p.group_alias("b"), None);
    assert!(!p.collapsed("b"));
    assert_eq!(p.profile_hue("default"), None);
    p.set_profile_order(vec!["research".into(), "research".into()])
        .unwrap();
    let names = vec!["z".to_owned(), "archive".into(), "research".into()];
    assert_eq!(
        ordered(&names, p.profile_order(), String::as_str),
        vec![&names[2], &names[1], &names[0]]
    );
}
#[test]
fn loading_and_untouched_flush_do_not_create_state() {
    let p = Private::new();
    let mut prefs = Preferences::at_path(p.file());
    prefs.flush_storage().unwrap();
    assert!(!p.file().exists());
    assert!(prefs.set_color("research".into(), Some(360)).is_err());
    assert!(!p.file().exists());
}
#[test]
fn same_group_leaf_edits_do_not_clobber_alias_or_collapse() {
    let p = Private::new();
    let mut a = Preferences::at_path(p.file());
    let mut b = Preferences::at_path(p.file());
    a.set_alias("a".into(), Some("Desk".into())).unwrap();
    b.set_collapsed("a".into(), true).unwrap();
    a.observe().unwrap();
    assert_eq!(a.group_alias("a"), Some("Desk"));
    assert!(a.collapsed("a"));
}

#[test]
fn untouched_flush_keeps_read_error_and_failed_retry_has_a_bounded_wake() {
    let p = Private::new();
    let raw = "{\"version\":99}";
    std::fs::write(p.file(), raw).unwrap();
    let mut prefs = Preferences::at_path(p.file());
    prefs.flush_storage().unwrap();
    assert!(prefs.error().is_some());
    assert_eq!(std::fs::read_to_string(p.file()).unwrap(), raw);
    assert!(prefs.set_collapsed("a".into(), true).is_err());
    assert!(!prefs.poll_storage().unwrap());
    assert!(prefs.wake_after().unwrap() > Duration::from_millis(500));
}

#[test]
fn unavailable_real_store_rejects_mutations_and_retains_the_unsaved_intent() {
    let mut p = Preferences::unavailable("Client state is unavailable".into());
    assert!(p.set_color("research".into(), Some(30)).is_err());
    assert_eq!(p.error(), Some("Client state is unavailable"));
    assert_eq!(p.explicit_profile_hue("research"), None);
    assert!(p.flush_storage().is_err());
    assert_eq!(p.pending.len(), 1);
}

#[test]
fn authored_css_colors_round_trip_exactly_with_legacy_hues_and_explicit_auto() {
    let p = Private::new();
    std::fs::write(
        p.file(),
        json!({"version":1,"profile_colors":{"legacy":150},"unknown":{"keep":true}}).to_string(),
    )
    .unwrap();
    let mut prefs = Preferences::at_path(p.file());
    assert_eq!(
        prefs.explicit_profile_color("legacy").as_deref(),
        Some("hsl(150 68% 58%)")
    );
    for (raw, expected) in [
        ("#12aB34cD", "#12ab34cd"),
        ("#0f08", "#00ff0088"),
        ("rgb(100% 0% 0% / 50%)", "#ff000080"),
        ("rgba(255, 128, 0, 0.5)", "#ff800080"),
        ("hsl(0 68% 58%)", "#dd4b4bff"),
    ] {
        prefs
            .set_profile_color("same".into(), Some(raw.into()))
            .unwrap();
        let saved = Preferences::at_path(p.file());
        assert_eq!(saved.explicit_profile_color("same").as_deref(), Some(raw));
        assert_eq!(
            saved.profile_color("same").unwrap().unwrap().hex(),
            expected
        );
    }
    let multiline = "hsl(
 0 68% 58% / 50%
)";
    prefs
        .set_profile_color("same".into(), Some(multiline.into()))
        .unwrap();
    let saved = Preferences::at_path(p.file());
    assert_eq!(
        saved.explicit_profile_color("same").as_deref(),
        Some(multiline)
    );
    assert_eq!(
        saved.profile_color("same").unwrap().unwrap().hex(),
        "#dd4b4b80"
    );
    let long = format!("var(--{} )", "authored".repeat(256));
    prefs
        .set_profile_color("same".into(), Some(long.clone()))
        .unwrap();
    let saved = Preferences::at_path(p.file());
    assert_eq!(
        saved.explicit_profile_color("same").as_deref(),
        Some(long.as_str())
    );
    assert!(saved.profile_color("same").is_err());
    prefs.set_profile_color("same".into(), None).unwrap();
    assert_eq!(prefs.explicit_profile_color("same"), None);
    assert!(prefs.profile_color("same").unwrap().is_some());
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(p.file()).unwrap()).unwrap();
    assert_eq!(raw["unknown"]["keep"], true);
}
#[test]
fn unsupported_or_overflowing_css_is_retained_without_painting_nan_or_claiming_success() {
    let p = Private::new();
    let mut prefs = Preferences::at_path(p.file());
    for value in [
        "color(display-p3 1 0 0)",
        "hsl(3e38turn 68% 58%)",
        "hsl(3e38rad 68% 58%)",
    ] {
        prefs
            .set_profile_color("same".into(), Some(value.into()))
            .unwrap();
        assert_eq!(prefs.explicit_profile_color("same").as_deref(), Some(value));
        assert!(prefs.profile_color("same").is_err());
    }
    let future = "{\"version\":99}";
    std::fs::write(p.file(), future).unwrap();
    assert!(
        prefs
            .set_profile_color("same".into(), Some("#ABCDEF80".into()))
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(p.file()).unwrap(), future);
    std::fs::write(p.file(), "{\"version\":1,\"keep\":true}").unwrap();
    prefs.flush_storage().unwrap();
    assert_eq!(
        prefs.explicit_profile_color("same").as_deref(),
        Some("#ABCDEF80")
    );
    assert_eq!(
        prefs.profile_color("same").unwrap().unwrap().hex(),
        "#abcdef80"
    );
}
#[test]
fn source_labels_sort_numerically_without_registration_or_selection_dependence() {
    assert!(compare_labels("Gateway2", "Gateway10").is_lt());
    assert!(compare_labels("Gateway0002", "gateway2").is_eq());
    assert!(compare_labels("Agent A", "agent b").is_lt());
    assert!(compare_labels("host9999999999999999999999999999999", "host2").is_gt());
}

#[test]
fn reordering_visible_sources_keeps_hidden_alias_and_offline_slots() {
    let p = Private::new();
    let mut prefs = Preferences::at_path(p.file());
    prefs
        .set_source_order(vec!["a".into(), "hidden".into(), "b".into()])
        .unwrap();
    prefs
        .reorder_sources(vec!["b".into(), "a".into(), "new".into()])
        .unwrap();
    assert_eq!(prefs.source_order(), ["b", "hidden", "a", "new"]);
    prefs.set_source_order(vec![]).unwrap();
    assert!(prefs.source_order().is_empty());
}

#[test]
fn unavailable_untouched_store_allows_close_without_erasing_its_read_error() {
    let mut prefs = Preferences::unavailable("Client state is unavailable".into());
    prefs.flush_storage().unwrap();
    assert_eq!(prefs.error(), Some("Client state is unavailable"));
    assert!(prefs.pending.is_empty());
    assert_eq!(prefs.explicit_profile_color("research"), None);
}
