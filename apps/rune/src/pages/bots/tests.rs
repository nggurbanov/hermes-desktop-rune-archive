use super::*;
fn roster() -> Value {
    json!({"bot_mode_protocol":true,"profiles":[{"name":"default","is_default":true,"description":"Main","ui_meta":{"hermes-bots":{"title":"Hermes","shape":"circle","groups":["room"],"future":true}},"ui_meta_revisions":{"hermes-bots":3}},{"name":"worker","description":"Work","ui_meta":{"hermes-bots":{"shape":"circle"}},"ui_meta_revisions":{"hermes-bots":1}}]})
}
fn loaded() -> Bots {
    let mut b = Bots::new();
    b.apply("bots.list", Ok(roster()));
    b.apply("bots.detail",Ok(json!({"name":"default","description":"Main","soul":"Original persona","model":{"provider":"sample","default":"old-model"},"skills":[{"name":"one","enabled":true},{"name":"two","enabled":true}],"toolsets":[{"name":"terminal","enabled":true},{"name":"web","enabled":true}],"mcp_servers":[]})));
    b
}
#[test]
fn fresh_state_does_not_block_owner_and_new_bot_is_single_flight() {
    let mut b = Bots::new();
    assert!(!b.blocks_owner_change());
    b.apply("bots.list", Ok(roster()));
    assert!(!b.dirty());
    let reads = b.act(Action::New);
    assert!(reads.iter().all(|(_, r)| r.path != "profiles.create"));
    b.name.replace_text("new-bot");
    let e = b.act(Action::Save);
    assert_eq!(e[0].1.path, "profiles.create");
    assert_eq!(e[0].1.body["share_auth"], true);
    assert!(
        e[0].1.body["soul"]
            .as_str()
            .unwrap()
            .contains("persistent named agent")
    );
    assert!(b.act(Action::Save).is_empty());
    assert!(b.take_new_bot_chat().is_none());
}
#[test]
fn independent_sections_advance_only_when_acknowledged_and_model_confirm_retries_only_model() {
    let mut b = loaded();
    b.soul.replace_text("New persona");
    b.provider.replace_text("other");
    b.model.replace_text("costly");
    let e = b.act(Action::Save);
    assert_eq!(e[0].1.body["soul"], "New persona");
    b.apply("bots.configure",Ok(json!({"ok":false,"applied":{"soul":true,"model":false},"confirm_required":true,"confirm_message":"Cost warning"})));
    assert_eq!(b.baseline["soul"], "New persona");
    assert_eq!(b.baseline["model"], "old-model");
    let e = b.act(Action::ConfirmModel);
    assert_eq!(
        e[0].1.body,
        json!({"name":"default","provider":"other","model":"costly","confirm_expensive_model":true})
    );
    b.apply(
        "bots.model-confirm",
        Ok(json!({"ok":true,"applied":{"model":true}})),
    );
    assert_eq!(b.baseline["model"], "costly");
    assert!(!b.dirty());
    assert!(b.take_new_bot_chat().is_none());
}
#[test]
fn appearance_cas_merges_only_user_patch_onto_current_namespace() {
    let mut b = loaded();
    b.color.replace_text("#123456");
    assert_eq!(b.act(Action::Save)[0].1.path, "profiles.list");
    let fresh = json!({"profiles":[{"name":"default","ui_meta":{"hermes-bots":{"title":"Changed elsewhere","shape":"circle","groups":["other-room"],"chat":"obsolete","future":42}},"ui_meta_revisions":{"hermes-bots":8}}]});
    let e = b.apply("bots.meta-read", Ok(fresh.clone()));
    let m = &e[0].1.body;
    assert_eq!(m["ui_meta_expected_revisions"]["hermes-bots"], 8);
    assert_eq!(m["ui_meta"]["hermes-bots"]["title"], "Changed elsewhere");
    assert_eq!(m["ui_meta"]["hermes-bots"]["groups"], json!(["other-room"]));
    assert_eq!(m["ui_meta"]["hermes-bots"]["color"], "#123456");
    assert!(m["ui_meta"]["hermes-bots"].get("chat").is_none());
    let retry=b.apply("bots.meta-write",Ok(json!({"ok":false,"applied":{"ui_meta":false,"ui_meta_conflicts":{"hermes-bots":{"actual":9}}}})));
    assert_eq!(retry[0].1.path, "profiles.list");
    b.apply("bots.meta-read", Ok(fresh.clone()));
    b.apply(
        "bots.meta-write",
        Ok(json!({"ok":false,"applied":{"ui_meta_conflicts":{"hermes-bots":{}}}})),
    );
    b.apply("bots.meta-read", Ok(fresh));
    assert!(
        b.apply(
            "bots.meta-write",
            Ok(json!({"ok":false,"applied":{"ui_meta_conflicts":{"hermes-bots":{}}}}))
        )
        .is_empty()
    );
    assert!(b.dirty());
    assert!(!b.busy);
    assert!(b.error.is_some());
}
#[test]
fn capabilities_emit_changed_sections_and_all_none_toolsets_clear_pin() {
    let mut b = loaded();
    b.toolsets.iter_mut().for_each(|c| c.enabled = false);
    let e = b.act(Action::Save);
    assert_eq!(e[0].1.body, json!({"name":"default","enabled_toolsets":[]}));
    b.apply(
        "bots.configure",
        Ok(json!({"ok":false,"applied":{"toolsets":false}})),
    );
    assert!(b.dirty());
    assert!(b.error.as_ref().unwrap().contains("toolsets"));
    assert!(b.skills.iter().all(|c| c.saved));
}
#[test]
fn rename_uncertainty_uses_surviving_source_and_requires_alias_identity_proof() {
    let mut b = loaded();
    b.act(Action::Select("worker".into()));
    b.apply("bots.detail",Ok(json!({"name":"worker","model":{},"soul":"","skills":[],"toolsets":[],"mcp_servers":[]})));
    b.act(Action::Rename);
    b.rename.replace_text("worker-two");
    let e = b.act(Action::SubmitRename);
    assert_eq!(e[0].1.profile.as_deref(), Some("default"));
    let e = b.apply("bots.rename", Err("Connection lost".into()));
    assert_eq!(e[0].1.profile.as_deref(), Some("default"));
    assert!(
        b.take_changes()
            .iter()
            .any(|c| matches!(c,ProfileChange::Reconcile(n)if n=="worker"))
    );
    b.apply("bots.list",Ok(json!({"profiles":[{"name":"default","is_default":true},{"name":"worker-two","previous_names":["worker"]}]})));
    assert!(
        b.take_changes().iter().any(
            |c| matches!(c,ProfileChange::Renamed{from,to}if from=="worker"&&to=="worker-two")
        )
    );
}
#[test]
fn default_profile_delete_is_refused_and_label_rename_keeps_identity() {
    let mut b = loaded();
    assert!(b.act(Action::Delete).is_empty());
    assert!(b.dialog.is_none());
    b.dialog = Some("delete:default".into());
    assert!(b.act(Action::ConfirmDelete).is_empty());
    b.dialog = None;
    b.act(Action::Rename);
    b.rename.replace_text("Мой Гермес");
    b.act(Action::SubmitRename);
    b.apply("bots.rename", Ok(json!({"ok":true,"name":"default"})));
    let changes = b.take_changes();
    assert!(
        changes
            .iter()
            .any(|c| matches!(c,ProfileChange::Updated(n)if n=="default"))
    );
    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, ProfileChange::Renamed { .. }))
    );
}
#[test]
fn only_genuine_new_bot_requests_intro_after_runtime_readiness() {
    let mut b = loaded();
    b.act(Action::New);
    b.name.replace_text("new-bot");
    b.act(Action::Save);
    b.apply(
        "bots.create",
        Ok(json!({"ok":true,"name":"new-bot","path":"/sample/new-bot"})),
    );
    b.apply(
        "bots.configure",
        Ok(json!({"ok":true,"applied":{"description":true,"soul":true}})),
    );
    b.apply("bots.meta-read",Ok(json!({"profiles":[{"name":"default","is_default":true},{"name":"new-bot","ui_meta":{},"ui_meta_revisions":{"hermes-bots":0}}]})));
    let follow = b.apply(
        "bots.meta-write",
        Ok(json!({"ok":true,"applied":{"ui_meta":true,"ui_meta_revisions":{"hermes-bots":1}}})),
    );
    assert!(
        follow
            .iter()
            .any(|(o, r)| o == "bots.ready" && r.path == "setup.runtime_check")
    );
    assert!(b.take_new_bot_chat().is_none());
    b.apply("bots.ready", Ok(json!({"ok":false,"error":"No model"})));
    let chat = b.take_new_bot_chat().unwrap();
    assert_eq!(chat.name, "new-bot");
    assert!(!chat.kickoff);
    assert!(b.take_new_bot_chat().is_none());
}
#[test]
fn model_reset_is_exact_scoped_command_and_does_not_claim_success_on_refusal() {
    let mut b = loaded();
    b.act(Action::ResetModel);
    let e = b.act(Action::Save);
    assert_eq!(e[0].1.path, "cli.exec");
    assert_eq!(
        e[0].1.body,
        json!({"argv":["--profile","default","config","unset","model"]})
    );
    assert_eq!(e[0].1.profile.as_deref(), Some("default"));
    b.apply("bots.model-unset", Ok(json!({"code":1,"blocked":true})));
    assert_eq!(b.baseline["model"], "old-model");
    assert!(b.dirty());
}

#[test]
fn duplicate_checks_and_copies_appearance_without_legacy_chat_or_intro() {
    let mut b = loaded();
    b.meta["chat"] = json!("legacy-session");
    b.meta["created"] = json!(42);
    let e = b.act(Action::Duplicate);
    assert_eq!(e[0].1.path, "profiles.get_asset");
    assert!(b.act(Action::Discard).is_empty());
    let e = b.apply("bots.duplicate-avatar", Ok(json!({"found":false})));
    assert_eq!(e[0].1.path, "profiles.create");
    assert_eq!(e[0].1.body["clone_from"], "default");
    let e = b.apply("bots.duplicate", Ok(json!({"ok":true,"name":"default-2"})));
    assert_eq!(e[0].1.path, "profiles.list");
    let e=b.apply("bots.meta-read",Ok(json!({"profiles":[{"name":"default-2","ui_meta":{},"ui_meta_revisions":{"hermes-bots":0}}]})));
    let look = &e[0].1.body["ui_meta"]["hermes-bots"];
    assert_eq!(look["title"], "Hermes (copy)");
    assert_eq!(look["groups"], json!(["room"]));
    assert!(look.get("chat").is_none());
    assert!(look.get("created").is_none());
    b.apply(
        "bots.meta-write",
        Ok(json!({"ok":true,"applied":{"ui_meta":true}})),
    );
    assert!(b.take_new_bot_chat().is_none());
    assert!(!b.awaiting_readiness());
}
#[test]
fn changed_model_draft_cannot_reuse_old_confirmation() {
    let mut b = loaded();
    b.model.replace_text("costly");
    b.act(Action::Save);
    b.apply(
        "bots.configure",
        Ok(json!({"ok":false,"applied":{"model":false},"confirm_required":true})),
    );
    b.model.replace_text("different-model");
    assert!(b.act(Action::ConfirmModel).is_empty());
    assert!(b.error.as_deref().unwrap().contains("draft changed"));
    assert!(!b.busy);
}
#[test]
fn incomplete_rename_ack_does_not_migrate_identity() {
    let mut b = loaded();
    b.act(Action::Select("worker".into()));
    b.apply("bots.detail",Ok(json!({"name":"worker","model":{},"soul":"","skills":[],"toolsets":[],"mcp_servers":[]})));
    b.act(Action::Rename);
    b.rename.replace_text("new-worker");
    b.act(Action::SubmitRename);
    b.apply("bots.rename", Ok(json!({"ok":true})));
    assert!(
        !b.take_changes()
            .iter()
            .any(|c| matches!(c, ProfileChange::Renamed { .. }))
    );
    assert_eq!(b.selected.as_deref(), Some("worker"));
}

#[test]
fn legacy_agent_keeps_real_soul_editing_when_advanced_snapshot_is_unavailable() {
    let mut b = loaded();
    let e = b.apply("bots.detail", Err("Method not found".into()));
    assert_eq!(e[0].1.path, "/api/profiles/default/soul");
    assert_eq!(e[0].1.profile.as_deref(), Some("default"));
    b.apply(
        "bots.soul",
        Ok(json!({"content":"Legacy persona","exists":true})),
    );
    assert_eq!(b.soul.text, "Legacy persona");
    assert!(!b.dirty());
    b.soul.replace_text("New legacy persona");
    let e = b.act(Action::Save);
    assert_eq!(e[0].1.path, "/api/profiles/default/soul");
    assert_eq!(e[0].1.body, json!({"content":"New legacy persona"}));
    b.apply("bots.soul-save", Ok(json!({"ok":false})));
    assert!(b.dirty());
    b.act(Action::Save);
    b.apply("bots.soul-save", Ok(json!({"ok":true})));
    assert_eq!(b.baseline["soul"], "New legacy persona");
}

#[test]
fn profile_export_overlay_is_exact_selected_profile_and_uses_desktop_json() {
    let mut b = loaded();
    b.set_export_overlay(
        "default".into(),
        json!({"skin":"opencode","appearanceMode":"dark"}),
    );
    b.act(Action::Export);
    let e = b.act(Action::Export);
    let text = e[0].1.body["extra_files"]["desktop.json"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap()["skin"],
        "opencode"
    );
    b.busy = false;
    b.dialog = None;
    b.set_export_overlay("other".into(), json!({"skin":"foreign"}));
    b.act(Action::Export);
    let e = b.act(Action::Export);
    assert_eq!(e[0].1.body["extra_files"], json!({}));
}
