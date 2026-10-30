//! Read-only replies may settle their original retained source while another is foreground.
use super::*;
impl Viewer {
    fn source_chat_read_allowed(&self, owner: &DraftScope) -> bool {
        self.binding_for_source_epoch(owner.connection).is_some()
            && self.chat.views.contains_key(owner)
            && !self
                .retired_profiles
                .contains(&(owner.connection, owner.profile.clone()))
    }
    pub(in crate::app) fn apply_source_catalog(
        &mut self,
        owner: DraftScope,
        result: Result<serde_json::Value, String>,
    ) {
        if !self.source_chat_read_allowed(&owner) {
            return;
        }
        let view = self.chat.presentation(owner);
        view.slash_loading = false;
        view.slash_loaded = true;
        match result {
            Ok(value) => {
                view.slash = crate::chat::catalog_rows(&value);
                view.slash_catalog = value;
            }
            Err(error) => view.error = Some(error),
        }
    }
    pub(in crate::app) fn apply_source_completions(
        &mut self,
        owner: &DraftScope,
        query: &str,
        result: Result<serde_json::Value, String>,
    ) {
        if self.source_chat_read_allowed(owner) {
            self.chat.apply_completions(owner, query, result);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn parked_slash_catalog_and_completions_settle_only_their_actual_source_and_reject_retirement()
    {
        let mut s = Viewer::empty();
        crate::app::reader_scene(&mut s);
        s.connection = Some(Connection::Url {
            endpoint: "https://a.fixture.invalid".into(),
            token: None,
        });
        s.connection_epoch = windows::source_epoch();
        s.profile = "work".into();
        s.selected_session = Some("same".into());
        let a = s.composer_scope();
        {
            let view = s.chat.presentation(a.clone());
            view.slash_loading = true;
            view.completion_query = "/he".into();
            view.completion_pending = true;
        }
        assert!(s.use_connection_for_profile(
            Connection::Url {
                endpoint: "https://b.fixture.invalid".into(),
                token: None
            },
            Some("work".into())
        ));
        s.selected_session = Some("same".into());
        let b = s.composer_scope();
        s.chat.presentation(b.clone()).slash_catalog = json!({"untouched":"B"});
        let catalog =
            json!({"pairs":[["help","A help"]],"commands":{"help":{"description":"A help"}}});
        s.apply_source_catalog(a.clone(), Ok(catalog.clone()));
        s.apply_source_completions(
            &a,
            "/he",
            Ok(json!({"items":[{"text":"/help","meta":"A completion"}],"replace_from":1})),
        );
        let av = &s.chat.views[&a];
        assert!(av.slash_loaded);
        assert!(!av.slash_loading);
        assert!(!av.completion_pending);
        assert_eq!(av.completion_ready, "/he");
        assert_eq!(av.slash_catalog, catalog);
        assert_eq!(av.slash[0].name, "help");
        assert_eq!(av.completion_items[0].name, "help");
        assert_eq!(av.completion_items[0].description, "A completion");
        assert_eq!(s.chat.views[&b].slash_catalog, json!({"untouched":"B"}));
        assert_eq!(s.composer_scope(), b);
        s.retired_profiles.insert((a.connection, a.profile.clone()));
        s.apply_source_catalog(a.clone(), Err("late stale error".into()));
        assert!(s.chat.views[&a].error.is_none());
        s.apply_source_completions(&a, "/other", Ok(json!({})));
        assert_eq!(s.chat.views[&a].completion_ready, "/he");
    }
}
