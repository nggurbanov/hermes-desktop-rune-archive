//! A delayed tab gesture resolves its captured source/profile/session, never a reused index.
use super::*;
impl Viewer {
    fn captured_tab_index(&self, owner: &DraftScope) -> Option<usize> {
        if owner.connection != self.connection_epoch {
            return None;
        }
        let stored = owner.session.as_deref()?;
        self.tabs.iter().position(|tab| {
            tab.profile == owner.profile
                && tab.profile_intent == owner.profile_intent
                && tab.session == stored
        })
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
            profile_intent: crate::source::ProfileIntent::Backend,
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

#[cfg(test)]
mod intent_tests {
    use super::*;
    use crate::source::ProfileIntent;
    #[test]
    fn captured_tab_lookup_distinguishes_selected_from_backend_with_same_id() {
        let mut viewer = Viewer::empty();
        viewer.connection_epoch = 7;
        viewer.tabs = [ProfileIntent::Backend, ProfileIntent::NativeSelf]
            .into_iter()
            .map(|profile_intent| ConversationTab {
                profile: "default".into(),
                profile_intent,
                session: "same".into(),
                title: "chat".into(),
                bot: false,
            })
            .collect();
        let raw = DraftScope {
            draft_origin: None,
            connection: 7,
            profile: "default".into(),
            profile_intent: ProfileIntent::Backend,
            session: Some("same".into()),
        };
        let selected = DraftScope {
            profile_intent: ProfileIntent::NativeSelf,
            ..raw.clone()
        };
        assert_eq!(viewer.captured_tab_index(&raw), Some(0));
        assert_eq!(viewer.captured_tab_index(&selected), Some(1));
        viewer.tabs.remove(1);
        assert_eq!(viewer.captured_tab_index(&selected), None);
    }
}

#[cfg(test)]
mod selected_target_tests {
    use super::*;
    use crate::{
        gateway::prepared::{BackendTarget, SelectedContext, WirePolicy},
        source::{ProfileIntent, RouteOwner},
    };
    #[test]
    fn equal_named_raw_target_keeps_a_separate_selected_tab_and_editor_owner() {
        let mut s = Viewer::empty();
        crate::app::reader_scene(&mut s);
        s.draft_storage_available = false;
        let profiles = crate::gateway::route::ProfilePolicy::new(
            "default",
            crate::gateway::route::RouteOptions {
                profile_remote_override: true,
                backend_profile: Some("dixie".into()),
                override_kind: crate::gateway::route::OverrideKind::Url,
                ..Default::default()
            },
        )
        .unwrap();
        let policy = WirePolicy::token(profiles, Default::default())
            .unwrap()
            .with_selected(
                SelectedContext::new("default", BackendTarget::Named("dixie".into())).unwrap(),
            )
            .unwrap();
        s.connection = Some(Connection::Routed {
            actual: Box::new(Connection::Url {
                endpoint: "https://private.fixture.invalid".into(),
                token: Some("private".into()),
            }),
            policy: std::sync::Arc::new(policy),
            owner: std::sync::Arc::new(RouteOwner::legacy("private-route").unwrap()),
        });
        s.connection_epoch = windows::source_epoch();
        s.profile = "default".into();
        s.profile_intent = ProfileIntent::NativeSelf;
        s.selected_session = Some("same".into());
        s.tabs = vec![ConversationTab {
            profile: "default".into(),
            profile_intent: ProfileIntent::NativeSelf,
            session: "same".into(),
            title: "Selected".into(),
            bot: false,
        }];
        s.init_editors(std::sync::Arc::new(|| {}));
        assert!(s.ensure_editor());
        let selected = s.composer_scope();
        s.composer
            .draft_mut(selected.clone())
            .text
            .replace_text("selected draft");
        s.chrome.writing = true;
        s.open_target(
            ConversationTarget::Session {
                profile: "default".into(),
                profile_intent: ProfileIntent::Backend,
                id: "same".into(),
                title: "Raw".into(),
            },
            true,
        );
        assert_eq!(s.composer_scope(), selected);
        assert_eq!(s.tabs.len(), 1);
        assert!(s.client_error.is_some());
        s.chrome.writing = false;
        s.client_error = None;
        s.open_target(
            ConversationTarget::Session {
                profile: "default".into(),
                profile_intent: ProfileIntent::Backend,
                id: "same".into(),
                title: "Raw".into(),
            },
            true,
        );
        assert_eq!(s.composer_scope().profile_intent, ProfileIntent::Backend);
        assert_eq!(s.tabs.len(), 2);
        assert_eq!(s.active_tab().unwrap().title, "Raw");
        s.activate_tab(0);
        assert_eq!(s.composer_scope(), selected);
        assert_eq!(s.active_tab().unwrap().title, "Selected");
        assert_eq!(
            s.composer.draft(&selected).unwrap().text.text,
            "selected draft"
        );
    }
}
