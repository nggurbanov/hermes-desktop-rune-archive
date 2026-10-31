use super::*;
use crate::composer::{DraftAttachment, PolicyFields, Popup};
fn viewer() -> Viewer {
    let mut s = Viewer::empty();
    crate::app::reader_scene(&mut s);
    s.init_editors(std::sync::Arc::new(|| {}));
    s
}
fn next(s: &mut Viewer) {
    s.open_target(
        ConversationTarget::Session {
            profile_intent: crate::source::ProfileIntent::Backend,
            profile: "default".into(),
            id: "sample-notes".into(),
            title: "Other chat".into(),
        },
        true,
    );
}
#[test]
fn tabs_keep_their_actual_editor_text_files_settings_and_popup_state() {
    let mut s = viewer();
    let first = s.composer_scope();
    let id = s.composer.editor_id().clone();
    s.composer
        .draft_mut(first.clone())
        .text
        .replace_text("first unsent");
    s.composer
        .draft_mut(first.clone())
        .attachments
        .push(DraftAttachment {
            path: "/private/first.bin".into(),
            name: "first.bin".into(),
            preview: None,
        });
    s.composer
        .choose_reasoning(first.clone(), Some("high".into()));
    s.composer.toggle_popup(first.clone(), Popup::Models);
    s.composer.search.replace_text("first search");
    next(&mut s);
    let second = s.composer_scope();
    let second_id = s.composer.editor_id().clone();
    assert_ne!(id, second_id);
    assert!(s.composer.draft(&second).is_none());
    assert!(s.composer.popup.is_none());
    s.composer
        .draft_mut(second.clone())
        .text
        .replace_text("second unsent");
    s.composer
        .choose_reasoning(second.clone(), Some("low".into()));
    s.activate_tab(0);
    assert_eq!(s.composer.editor_id(), &id);
    let restored = s.composer.draft(&first).unwrap();
    assert_eq!(restored.text.text, "first unsent");
    assert_eq!(restored.attachments.len(), 1);
    assert_eq!(restored.reasoning.as_deref(), Some("high"));
    assert_eq!(s.composer.popup, Some(Popup::Models));
    assert_eq!(s.composer.search.text, "first search");
    s.activate_tab(1);
    assert_eq!(s.composer.editor_id(), &second_id);
    assert_eq!(
        s.composer.draft(&second).unwrap().text.text,
        "second unsent"
    );
}
#[test]
fn a_file_dialog_return_targets_its_original_editor_after_switch_or_tab_close() {
    let mut s = viewer();
    let first = s.composer_scope();
    let id = s.composer.editor_id().clone();
    s.composer
        .draft_mut(first.clone())
        .text
        .replace_text("first draft");
    let path =
        std::env::temp_dir().join(format!("hermes-editor-file-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&path, b"owned fixture").unwrap();
    next(&mut s);
    let other = s.composer_scope();
    s.composer
        .draft_mut(other.clone())
        .text
        .replace_text("other draft");
    s.finish_editor_attachment(id.clone(), first.clone(), Ok(Some(path.clone())));
    assert!(s.composer.draft(&other).unwrap().attachments.is_empty());
    assert_eq!(
        s.editor_mut(&id, &first)
            .unwrap()
            .draft(&first)
            .unwrap()
            .attachments[0]
            .path,
        path
    );
    s.close_tab(0);
    assert_eq!(s.composer_scope(), other);
    s.finish_editor_attachment(id.clone(), first.clone(), Err("late dialog failure".into()));
    assert!(s.composer.attachment_error.is_none());
    assert!(
        s.editor_mut(&id, &first)
            .unwrap()
            .attachment_error
            .is_some()
    );
    std::fs::remove_file(path).unwrap();
}
#[test]
fn late_policy_ack_and_live_truth_update_inactive_inputs_without_acknowledging_newer_edits() {
    let mut s = viewer();
    let first = s.composer_scope();
    let id = s.composer.editor_id().clone();
    s.composer
        .choose_reasoning(first.clone(), Some("high".into()));
    let old = s.composer.draft(&first).unwrap().policy_stamp();
    next(&mut s);
    let other = s.composer_scope();
    s.composer
        .draft_mut(other.clone())
        .text
        .replace_text("other draft");
    let source = s.editor_mut(&id, &first).unwrap();
    source.choose_reasoning(first.clone(), Some("low".into()));
    source.choose_reasoning(first.clone(), Some("high".into()));
    assert!(!s.settle_editor_policy(
        &first,
        &old,
        PolicyFields {
            reasoning: true,
            ..Default::default()
        }
    ));
    let current = s
        .editor_mut(&id, &first)
        .unwrap()
        .draft(&first)
        .unwrap()
        .policy_stamp();
    assert!(s.settle_editor_policy(
        &first,
        &current,
        PolicyFields {
            reasoning: true,
            ..Default::default()
        }
    ));
    let policy = crate::conversation::SessionPolicy {
        reasoning: Some("medium".into()),
        ..Default::default()
    };
    s.reconcile_editor_policy(&first, &policy);
    assert_eq!(
        s.editor_mut(&id, &first)
            .unwrap()
            .draft(&first)
            .unwrap()
            .reasoning
            .as_deref(),
        Some("medium")
    );
    assert_eq!(s.composer.draft(&other).unwrap().text.text, "other draft");
    assert!(
        s.composer.draft(&first).is_none(),
        "a background policy event must not create a shadow draft in the foreground controller"
    );
}
#[test]
fn a_bound_fresh_editor_keeps_identity_and_delayed_dialog_alias() {
    let mut s = viewer();
    s.new_draft();
    let fresh = s.composer_scope();
    let id = s.composer.editor_id().clone();
    s.composer
        .draft_mut(fresh.clone())
        .text
        .replace_text("fresh input");
    let stored = DraftScope {
        session: Some("new-stored".into()),
        draft_origin: None,
        ..fresh.clone()
    };
    s.rehome_editors(&fresh, stored.clone());
    s.selected_session = stored.session.clone();
    assert!(s.ensure_editor());
    assert_eq!(s.composer.editor_id(), &id);
    s.new_draft();
    assert_ne!(s.composer.editor_id(), &id);
    s.finish_editor_attachment(id.clone(), fresh.clone(), Err("late picker".into()));
    assert!(s.composer.attachment_error.is_none());
    assert!(
        s.editor_mut(&id, &stored)
            .unwrap()
            .attachment_error
            .is_some()
    );
}

#[test]
fn delayed_text_input_stays_with_its_built_editor_and_cannot_edit_a_replacement_or_retired_profile()
{
    let mut s = viewer();
    let first = s.composer_scope();
    let editor = s.composer.editor_id().clone();
    s.editors.blocked = true;
    assert!(s.ensure_editor());
    assert!(s.editor_ready());
    s.composer
        .draft_mut(first.clone())
        .text
        .replace_text("original");
    next(&mut s);
    let other = s.composer_scope();
    s.composer
        .draft_mut(other.clone())
        .text
        .replace_text("new input");
    s.edit_editor_text(&editor, &first).insert(" late");
    assert_eq!(s.composer.draft(&other).unwrap().text.text, "new input");
    let captured = s
        .editor_mut(&editor, &first)
        .unwrap()
        .draft(&first)
        .unwrap()
        .text
        .text
        .clone();
    assert!(captured.contains("late"));
    assert!(s.prepare_profile_mutation(&first.profile));
    s.edit_editor_text(&editor, &first)
        .replace_text("retired event");
    assert_eq!(s.composer.draft(&other).unwrap().text.text, "new input");
    assert_eq!(
        s.editor_mut(&editor, &first)
            .unwrap()
            .draft(&first)
            .unwrap()
            .text
            .text,
        captured
    );
}
