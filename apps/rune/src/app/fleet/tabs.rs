//! A delayed tab gesture resolves its captured source/profile/session, never a reused index.
use super::*;
impl Viewer {
    fn captured_tab_index(&self, owner: &DraftScope) -> Option<usize> {
        if owner.connection != self.connection_epoch {
            return None;
        }
        let stored = owner.session.as_deref()?;
        self.tabs
            .iter()
            .position(|tab| tab.profile == owner.profile && tab.session == stored)
    }
    pub fn activate_tab_owned(&mut self, owner: &DraftScope) {
        if let Some(index) = self.captured_tab_index(owner) {
            self.activate_tab(index);
        }
    }
    pub fn close_tab_owned(&mut self, owner: &DraftScope) {
        if let Some(index) = self.captured_tab_index(owner) {
            self.close_tab(index);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn connection(name: &str) -> Connection {
        Connection::Url {
            endpoint: format!("https://{name}.tab-fixture.invalid"),
            token: Some("synthetic".into()),
        }
    }
    fn tab(title: &str, id: &str) -> ConversationTab {
        ConversationTab {
            profile: "default".into(),
            session: id.into(),
            title: title.into(),
            bot: false,
        }
    }
    fn viewer() -> Viewer {
        let mut s = Viewer::empty();
        crate::app::reader_scene(&mut s);
        s.draft_storage_available = false;
        s.connection = Some(connection("a"));
        s.connection_epoch = windows::source_epoch();
        s.profile = "default".into();
        s.selected_session = Some("same".into());
        s.tabs = vec![tab("A conversation", "same")];
        s.init_editors(std::sync::Arc::new(|| {}));
        assert!(s.ensure_editor());
        s
    }
    fn switch(s: &mut Viewer, source: &str) {
        assert!(s.use_connection_for_profile(connection(source), Some("default".into())));
        assert!(s.ensure_editor());
    }
    #[test]
    fn delayed_a_tab_gestures_cannot_close_or_activate_bs_same_index_and_stored_id() {
        let mut s = viewer();
        let a = s.composer_scope();
        switch(&mut s, "b");
        s.tabs = vec![tab("B conversation", "same")];
        s.selected_session = Some("same".into());
        assert!(s.ensure_editor());
        let b = s.composer_scope();
        let editor = s.composer.editor_id().clone();
        s.composer
            .draft_mut(b.clone())
            .text
            .replace_text("B unsent");
        s.route = crate::pages::Route::Settings;
        let intent = s.intent;
        assert_ne!(a.connection, b.connection);
        s.activate_tab_owned(&a);
        s.close_tab_owned(&a);
        assert!(s.route == crate::pages::Route::Settings);
        assert_eq!(s.intent, intent);
        assert_eq!(s.composer_scope(), b);
        assert_eq!(s.composer.editor_id(), &editor);
        assert_eq!(s.composer.draft(&b).unwrap().text.text, "B unsent");
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.tabs[0].title, "B conversation");
        switch(&mut s, "a");
        assert_eq!(s.composer_scope(), a);
        s.route = crate::pages::Route::Settings;
        s.activate_tab_owned(&a);
        assert!(s.route == crate::pages::Route::Chat);
        assert_eq!(s.tabs[0].title, "A conversation");
        s.close_tab_owned(&a);
        assert!(s.tabs.is_empty());
        assert!(s.selected_session.is_none());
    }
    #[test]
    fn legitimate_tab_gestures_resolve_the_stored_owner_after_reorder_and_ignore_closed_tabs() {
        let mut s = viewer();
        let a = s.composer_scope();
        s.tabs.push(tab("Other conversation", "other"));
        s.tabs.swap(0, 1);
        s.selected_session = Some("other".into());
        assert!(s.ensure_editor());
        s.activate_tab_owned(&a);
        assert_eq!(s.selected_session.as_deref(), Some("same"));
        assert_eq!(s.tabs[0].session, "other");
        s.close_tab_owned(&a);
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.tabs[0].session, "other");
        assert_eq!(s.selected_session.as_deref(), Some("other"));
        s.route = crate::pages::Route::Settings;
        let intent = s.intent;
        s.activate_tab_owned(&a);
        s.close_tab_owned(&a);
        assert!(s.route == crate::pages::Route::Settings);
        assert_eq!(s.intent, intent);
        assert_eq!(s.tabs[0].session, "other");
    }
}
