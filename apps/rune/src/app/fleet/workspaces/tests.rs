use super::*;
fn connection(name: &str) -> Connection {
    Connection::Url {
        endpoint: format!("https://{name}.fixture.invalid/hermes"),
        token: Some(format!("private-{name}")),
    }
}
fn viewer() -> Viewer {
    let mut s = Viewer::empty();
    crate::app::reader_scene(&mut s);
    s.draft_storage_available = false;
    s.connection = Some(connection("a"));
    s.connection_epoch = windows::source_epoch();
    s.profile = "default".into();
    s.selected_session = None;
    s.tabs.clear();
    s.init_editors(std::sync::Arc::new(|| {}));
    assert!(s.ensure_editor());
    s
}
fn switch(s: &mut Viewer, source: &str) {
    assert!(s.use_connection_for_profile(connection(source), Some("default".into())));
    assert!(s.ensure_editor());
}
#[test]
fn same_names_keep_actual_editors_tabs_drafts_and_delayed_file_receipts_per_source() {
    let mut s = viewer();
    s.selected_session = Some("same".into());
    s.tabs.push(ConversationTab {
        profile: "default".into(),
        session: "same".into(),
        title: "A conversation".into(),
        bot: false,
    });
    assert!(s.ensure_editor());
    let a = s.composer_scope();
    let a_editor = s.composer.editor_id().clone();
    s.composer
        .draft_mut(a.clone())
        .text
        .replace_text("A unsent");
    switch(&mut s, "b");
    assert!(s.tabs.is_empty());
    s.selected_session = Some("same".into());
    s.tabs.push(ConversationTab {
        profile: "default".into(),
        session: "same".into(),
        title: "B conversation".into(),
        bot: false,
    });
    assert!(s.ensure_editor());
    let b = s.composer_scope();
    let b_editor = s.composer.editor_id().clone();
    assert_ne!(a.connection, b.connection);
    assert_ne!(a_editor, b_editor);
    s.composer
        .draft_mut(b.clone())
        .text
        .replace_text("B unsent");
    s.edit_editor_text(&a_editor, &a).insert(" stale UI event");
    s.finish_editor_attachment(
        a_editor.clone(),
        a.clone(),
        Err("private chooser failure".into()),
    );
    assert!(s.composer.attachment_error.is_none());
    assert_eq!(s.composer.draft(&b).unwrap().text.text, "B unsent");
    switch(&mut s, "a");
    assert_eq!(s.composer_scope(), a);
    assert_eq!(s.composer.editor_id(), &a_editor);
    assert_eq!(s.tabs.len(), 1);
    assert_eq!(s.tabs[0].title, "A conversation");
    assert_eq!(s.composer.draft(&a).unwrap().text.text, "A unsent");
    assert!(s.composer.attachment_error.is_some());
    switch(&mut s, "b");
    assert_eq!(s.composer_scope(), b);
    assert_eq!(s.composer.editor_id(), &b_editor);
    assert_eq!(s.tabs[0].title, "B conversation");
    assert_eq!(s.composer.draft(&b).unwrap().text.text, "B unsent");
}
#[test]
fn a_fresh_send_binding_while_parked_restores_its_actual_input_and_stored_tab_only() {
    let mut s = viewer();
    let origin = s.composer_scope();
    let editor = s.composer.editor_id().clone();
    s.composer
        .draft_mut(origin.clone())
        .text
        .replace_text("A follow-up");
    switch(&mut s, "b");
    let b = s.composer_scope();
    let b_editor = s.composer.editor_id().clone();
    s.composer
        .draft_mut(b.clone())
        .text
        .replace_text("B untouched");
    let stored = DraftScope {
        session: Some("created-on-a".into()),
        ..origin.clone()
    };
    s.rehome_editors(&origin, stored.clone());
    s.bind_source_conversation(&origin, "created-on-a", false);
    assert_eq!(s.composer_scope(), b);
    assert_eq!(s.composer.editor_id(), &b_editor);
    assert!(s.tabs.is_empty());
    assert_eq!(s.composer.draft(&b).unwrap().text.text, "B untouched");
    switch(&mut s, "a");
    assert_eq!(s.composer_scope(), stored);
    assert_eq!(s.composer.editor_id(), &editor);
    assert_eq!(s.composer.draft(&stored).unwrap().text.text, "A follow-up");
    assert_eq!(s.tabs.len(), 1);
    assert_eq!(s.tabs[0].session, "created-on-a");
    s.new_draft_on_current_source();
    assert!(s.composer_scope().session.is_none());
    assert_ne!(s.composer.editor_id(), &editor);
    assert!(s.composer.draft(&s.composer_scope()).is_none());
}
