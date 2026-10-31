//! Search is owned by the mounted editor as well as the conversation scope.
use crate::{
    app::Viewer,
    chat::{
        projection::{self, Part},
        search::{self, Fragment, FragmentKind, Owner},
    },
    pages::Route,
};
use rune::{Key, KeyEvent, Tx};

impl Viewer {
    pub(crate) fn update_chat_media(&mut self, update: impl FnOnce(&mut crate::chat::Chat)) {
        let owner = self.chat_search_owner();
        let sources = |chat: &crate::chat::Chat| {
            chat.media
                .keys()
                .filter(|(scope, _)| scope == &owner.scope)
                .map(|(_, source)| source.clone())
                .collect::<std::collections::HashSet<_>>()
        };
        let before = self
            .chat
            .searches
            .views
            .contains_key(&owner)
            .then(|| sources(&self.chat));
        update(&mut self.chat);
        if before.is_some_and(|before| before != sources(&self.chat)) {
            self.refresh_chat_search();
        }
    }
    pub(crate) fn retire_chat_search_reveals(&mut self) {
        self.chat.searches.retire_pending();
    }
    pub fn chat_search_owner(&self) -> Owner {
        Owner {
            scope: self.composer_scope(),
            editor: self.composer.editor_id().clone(),
        }
    }
    pub fn chat_edit_key(&self) -> String {
        format!(
            "chat-edit-input-{}-{:?}",
            self.composer.editor_id().as_str(),
            self.composer_scope()
        )
    }
    pub fn chat_history_key(&self) -> String {
        format!("history-{}", self.composer.editor_id().as_str())
    }
    pub fn refresh_chat_search(&mut self) {
        let owner = self.chat_search_owner();
        if !self.chat.searches.views.contains_key(&owner) {
            return;
        }
        let messages = projection::project(&self.messages);
        let mut parts = Vec::new();
        for message in &messages {
            if let Some((_, edit)) = self
                .chat
                .views
                .get(&owner.scope)
                .and_then(|view| view.edit.as_ref())
                .filter(|(row, _)| Some(*row) == message.row)
            {
                parts.push((
                    message.key.clone(),
                    "edit".into(),
                    edit.display().0,
                    FragmentKind::Literal,
                ));
                continue;
            }
            for (i, part) in message.parts.iter().enumerate() {
                let key = format!("{}-part-{i}", message.key);
                let id = format!("part-{i}");
                let mut add = |part: String, text: String, kind: FragmentKind| {
                    parts.push((message.key.clone(), part, text, kind));
                };
                match part {
                    Part::Text(text) => {
                        let display = crate::chat::media_tags::display_text(&message.role, text);
                        for (start, label) in
                            crate::view::transcript::markdown_audio_labels(&display)
                        {
                            add(
                                format!("{id}-audio-{start}-label"),
                                label,
                                FragmentKind::Literal,
                            );
                        }
                        add(id, display, FragmentKind::MarkdownAudio);
                    }
                    Part::Reasoning(text)
                        if crate::view::transcript::disclosure_open(
                            self,
                            &owner.scope,
                            &key,
                            true,
                        ) =>
                    {
                        add(id, text.clone(), FragmentKind::Markdown)
                    }
                    Part::Tool {
                        name, args, output, ..
                    } => {
                        let open = crate::view::transcript::disclosure_open(
                            self,
                            &owner.scope,
                            &key,
                            false,
                        );
                        add(
                            format!("{id}-title"),
                            search::disclosure_label(
                                &search::tool_caption(name, output.is_some()),
                                open,
                            ),
                            FragmentKind::Literal,
                        );
                        if open {
                            add(format!("{id}-args"), args.clone(), FragmentKind::Literal);
                            if let Some(text) = output {
                                add(id, text.clone(), FragmentKind::Markdown);
                            }
                        }
                    }
                    Part::Image { source, label } => {
                        let loaded = self
                            .chat
                            .media
                            .contains_key(&(owner.scope.clone(), source.clone()));
                        add(
                            format!("{id}-label"),
                            search::image_label(label, loaded),
                            FragmentKind::Literal,
                        );
                    }
                    Part::File { label, .. } => add(
                        format!("{id}-label"),
                        search::file_label(label),
                        FragmentKind::Literal,
                    ),
                    _ => {}
                }
            }
        }
        if let Some(live) = self.conversation.view(&owner.scope) {
            use crate::view::transcript::{
                disclosure_open, live_tool_args, live_tool_key, live_tool_output, live_tool_title,
                live_turn_key,
            };
            let turn = live_turn_key(live);
            let reasoning_key = format!("{turn}-reasoning");
            if !live.reasoning.trim().is_empty()
                && disclosure_open(self, &owner.scope, &reasoning_key, true)
            {
                parts.push((
                    turn,
                    "reasoning".into(),
                    live.reasoning.clone(),
                    FragmentKind::Markdown,
                ));
            }
            for tool in &live.activities {
                let key = live_tool_key(live, tool);
                let open = disclosure_open(self, &owner.scope, &key, false);
                parts.push((
                    key.clone(),
                    "title".into(),
                    search::disclosure_label(&live_tool_title(tool), open),
                    FragmentKind::Literal,
                ));
                if open {
                    parts.push((
                        key.clone(),
                        "args".into(),
                        live_tool_args(tool),
                        FragmentKind::Literal,
                    ));
                    parts.push((
                        key,
                        "output".into(),
                        live_tool_output(tool).into(),
                        FragmentKind::Markdown,
                    ));
                }
            }
        }
        let fragments: Vec<_> = parts
            .iter()
            .map(|(message, part, text, kind)| Fragment {
                message,
                part,
                text,
                kind: kind.clone(),
            })
            .collect();
        let bridge = self
            .conversation
            .view(&owner.scope)
            .and_then(|live| live.search_reasoning_bridge.as_ref())
            .and_then(|(from, row)| {
                let message = messages.iter().find(|message| message.row == Some(*row))?;
                let mut reasoning = message
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(_, part)| matches!(part, Part::Reasoning(_)));
                let (index, _) = reasoning.next()?;
                if reasoning.next().is_some() {
                    return None;
                }
                Some((from.clone(), message.key.clone(), format!("part-{index}")))
            });
        let tool_bridge = self.conversation.view(&owner.scope).and_then(|live| {
            let (_, row) = live.search_reasoning_bridge.as_ref()?;
            let message = messages.iter().find(|message| message.row == Some(*row))?;
            Some((live.search_tool_bridge.clone(), message))
        });
        let view = self.chat.searches.view(owner);
        view.pending = None;
        view.revision = view.revision.wrapping_add(1);
        if let Some((tool_bridges, message)) = tool_bridge {
            for (from, id) in tool_bridges {
                let mut destinations =
                    message.parts.iter().enumerate().filter(
                        |(_, part)| matches!(part, Part::Tool { id: held, .. } if held == &id),
                    );
                if let Some((index, _)) = destinations.next() {
                    if destinations.next().is_none() {
                        for (old, suffix) in
                            [("title", "-title"), ("args", "-args"), ("output", "")]
                        {
                            view.search.rekey_current(
                                &from,
                                old,
                                &message.key,
                                &format!("part-{index}{suffix}"),
                            );
                        }
                    }
                }
            }
        }
        if let Some((from, message, part)) = bridge {
            view.search
                .rekey_current(&from, "reasoning", &message, &part);
        }
        view.search.set_query(view.input.text.clone(), &fragments);
    }
    pub fn reveal_chat_search(&mut self, forward: Option<bool>, tx: &mut Tx) {
        self.refresh_chat_search();
        let owner = self.chat_search_owner();
        let history = self.chat_history_key();
        let view = self.chat.searches.view(owner.clone());
        if let Some(forward) = forward {
            view.search.step(forward);
        }
        view.pending = view
            .search
            .current()
            .cloned()
            .map(|current| search::PendingReveal {
                owner,
                query: view.input.text.clone(),
                current,
                revision: view.revision,
                mounted_frame: false,
            });
        if let Some(pending) = &view.pending {
            tx.scroll_into_view_in(history, format!("message-{}", pending.current.message));
            tx.wake_after(std::time::Duration::ZERO);
        }
        tx.instant();
    }
    pub(crate) fn poll_chat_search_reveal(&mut self, tx: &mut Tx) -> bool {
        let owner = self.chat_search_owner();
        // Retire requests for unmounted editors and scopes before examining the current one.
        for (held, view) in &mut self.chat.searches.views {
            if held != &owner {
                view.pending = None;
            }
        }
        let history = self.chat_history_key();
        let Some(view) = self.chat.searches.views.get_mut(&owner) else {
            return false;
        };
        let Some(mut pending) = view.pending.take() else {
            return false;
        };
        if self.route != Route::Chat
            || self.connection_form
            || !view.open
            || pending.owner != owner
            || pending.query != view.input.text
            || pending.revision != view.revision
            || view.search.current() != Some(&pending.current)
        {
            return false;
        }
        if !pending.mounted_frame {
            pending.mounted_frame = true;
            view.pending = Some(pending);
            tx.wake_after(std::time::Duration::ZERO);
            return true;
        }
        let hit = pending.current;
        let Some((first, last)) = hit.bytes.first().zip(hit.bytes.last()) else {
            return false;
        };
        let key = format!(
            "search-{}-{}-{}-{}",
            hit.message, hit.part, first.start, last.end
        );
        tx.scroll_highlight_into_view_all_in(history, key);
        tx.instant();
        true
    }
    pub fn chat_search_key(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        self.chat_search_key_inner(event, tx, false)
    }
    pub fn chat_search_input_key(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        self.chat_search_key_inner(event, tx, true)
    }
    fn chat_search_key_inner(&mut self, event: &KeyEvent, tx: &mut Tx, search_input: bool) -> bool {
        if self.route != Route::Chat || self.connection_form {
            return false;
        }
        let focused = tx.focused();
        let in_chat = search_input
            || (focused == Some(self.chat_edit_key().as_str())
                && self
                    .chat
                    .views
                    .get(&self.composer_scope())
                    .is_some_and(|view| view.edit.is_some()))
            || matches!(focused, Some("composer-input"))
            || focused == Some(self.chat_history_key().as_str());
        if !in_chat {
            return false;
        }
        let owner = self.chat_search_owner();
        if event.is_command('f') && !event.mods.shift && !event.mods.alt {
            let view = self.chat.searches.view(owner);
            view.open = true;
            view.input.focused = true;
            self.reveal_chat_search(None, tx);
            return true;
        }
        let open = self
            .chat
            .searches
            .views
            .get(&owner)
            .is_some_and(|view| view.open);
        if !open {
            return false;
        }
        if event.is_command('g') && !event.mods.alt || (search_input && event.key == Key::Enter) {
            self.reveal_chat_search(Some(!event.mods.shift), tx);
            return true;
        }
        if event.key == Key::Escape && search_input {
            let view = self.chat.searches.view(owner);
            view.open = false;
            view.pending = None;
            view.input.focused = false;
            tx.blur();
            tx.instant();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{chat::Action, gateway::HistoryMessage};
    use rune::{Cx, El, Env, Mods, Runtime, TextStyle, col, text_input};
    use serde_json::json;

    fn view(s: &Viewer, _: &mut Cx<Viewer>) -> El {
        let font = TextStyle::new(14., 400.);
        col()
            .size(500., 300.)
            .child(
                text_input(&s.search, font)
                    .key("composer-input")
                    .size(200., 30.)
                    .on_edit(|s: &mut Viewer| &mut s.search),
            )
            .child(
                text_input(&s.connection_url, font)
                    .key("chat-edit-input-foreign-editor")
                    .size(200., 30.)
                    .on_edit(|s: &mut Viewer| &mut s.connection_url),
            )
            .child(crate::view::transcript::search_bar(
                s,
                &rune_theme_opencode::kit(true),
            ))
    }
    fn click(rt: &mut Runtime<Viewer>, key: &str) {
        let r = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap()
            .rect;
        rt.pointer_down(r[0] + 5., r[1] + 5.);
        rt.pointer_up(r[0] + 5., r[1] + 5.);
        rt.frame(0.);
    }
    fn key(rt: &mut Runtime<Viewer>, key: Key, shift: bool) {
        rt.key(KeyEvent {
            key,
            mods: Mods {
                shift,
                ..Default::default()
            },
        });
        rt.frame(0.);
    }
    #[test]
    fn edit_preedit_display_change_reaches_search() {
        let mut s = Viewer::empty();
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id": 7, "role":"user", "content":""}),
        )];
        let owner = s.chat_search_owner();
        s.chat.searches.view(owner.clone()).open = true;
        s.chat.searches.view(owner.clone()).input.text = "漢".into();
        s.refresh_chat_search();
        let mut rt = Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            Env::new(true, 1.),
        );
        rt.frame(0.);
        rt.update(|s, _| s.chat_action(owner.scope.clone(), Action::Edit(7)));
        rt.frame(0.);
        let edit_key = rt.state.chat_edit_key();
        click(&mut rt, &edit_key);
        rt.ime(rune::Ime::Preedit("漢".into(), None));
        rt.frame(0.);
        assert_eq!(
            rt.state.chat.searches.views[&owner].search.matches[0].bytes,
            vec![0..3]
        );
        assert!(rt.find("search-row-7-edit-0-3").is_some());
        assert_eq!(
            rt.state.chat.views[&owner.scope]
                .edit
                .as_ref()
                .unwrap()
                .1
                .range(),
            (0, 0)
        );
        rt.ime(rune::Ime::Preedit(String::new(), None));
        rt.frame(0.);
        assert!(
            rt.state.chat.searches.views[&owner]
                .search
                .matches
                .is_empty()
        );
        rt.ime(rune::Ime::Preedit("漢".into(), None));
        rt.frame(0.);
        rt.ime(rune::Ime::Commit("漢".into()));
        rt.frame(0.);
        assert_eq!(
            rt.state.chat.searches.views[&owner].search.matches[0].bytes,
            vec![0..3]
        );
        assert_eq!(
            rt.state.chat.views[&owner.scope]
                .edit
                .as_ref()
                .unwrap()
                .1
                .range(),
            (3, 3)
        );
    }
    #[test]
    fn edit_search_tracks_runtime_text_and_transitions() {
        let mut s = Viewer::empty();
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id": 7, "role":"user", "content":"old [label](hidden-destination)"}),
        )];
        let owner = s.chat_search_owner();
        s.chat.searches.view(owner.clone()).open = true;
        s.chat.searches.view(owner.clone()).input.text = "needle".into();
        s.refresh_chat_search();
        let mut rt = Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            Env::new(true, 1.),
        );
        rt.frame(0.);
        rt.update(|s, _| s.chat_action(owner.scope.clone(), Action::Edit(7)));
        rt.frame(0.);
        let edit_key = rt.state.chat_edit_key();
        click(&mut rt, &edit_key);
        key(&mut rt, Key::Command('a'), false);
        key(&mut rt, Key::Text("İ needle".into()), false);
        assert_eq!(rt.state.chat.searches.views[&owner].search.matches.len(), 1);
        rt.paste(" needle");
        rt.frame(0.);
        assert_eq!(
            rt.state.chat.searches.views[&owner]
                .search
                .matches
                .iter()
                .map(|m| m.bytes.clone())
                .collect::<Vec<_>>(),
            vec![vec![3..9], vec![10..16]]
        );
        rt.invalidate();
        rt.frame(0.);
        assert!(rt.find("search-row-7-edit-3-9").is_some());
        rt.update(|s, _| s.chat_action(owner.scope.clone(), Action::SaveEdit));
        rt.frame(0.);
        assert_eq!(rt.state.chat.searches.views[&owner].search.matches.len(), 2);
        rt.update(|s, _| s.chat_action(owner.scope.clone(), Action::Cancel));
        rt.frame(0.);
        assert!(
            rt.state.chat.searches.views[&owner]
                .search
                .matches
                .is_empty()
        );
        rt.state.chat.searches.view(owner.clone()).input.text = "hidden-destination".into();
        rt.state.refresh_chat_search();
        assert!(
            rt.state.chat.searches.views[&owner]
                .search
                .matches
                .is_empty()
        );
        rt.update(|s, _| s.chat_action(owner.scope.clone(), Action::Edit(7)));
        rt.frame(0.);
        assert_eq!(rt.state.chat.searches.views[&owner].search.matches.len(), 1);
        let edit_key = rt.state.chat_edit_key();
        click(&mut rt, &edit_key);
        key(&mut rt, Key::Command('a'), false);
        key(&mut rt, Key::Text("replacement".into()), false);
        assert!(
            rt.state.chat.searches.views[&owner]
                .search
                .matches
                .is_empty()
        );
        rt.update(|s, _| s.chat_action(owner.scope.clone(), Action::Cancel));
        rt.frame(0.);
        assert!(
            rt.state.chat.searches.views[&owner]
                .search
                .matches
                .is_empty()
        );
    }
    #[test]
    fn edit_search_scope_retains_independent_queries_and_hits() {
        let mut s = Viewer::empty();
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id": 7, "role":"user", "content":"same message"}),
        )];
        s.draft_origin = Some(crate::composer::DraftOrigin::new());
        let fresh = s.chat_search_owner();
        s.chat.searches.view(fresh.clone()).input.text = "same".into();
        s.refresh_chat_search();
        s.chat_action(fresh.scope.clone(), Action::Edit(7));
        assert_eq!(s.chat.searches.views[&fresh].search.matches.len(), 1);
        s.draft_origin = Some(crate::composer::DraftOrigin::new());
        let next_draft = s.chat_search_owner();
        assert_ne!(fresh, next_draft);
        s.chat.searches.view(next_draft.clone()).input.text = "missing".into();
        s.refresh_chat_search();
        assert!(s.chat.searches.views[&next_draft].search.matches.is_empty());
        s.selected_session = Some("same-session".into());
        let session = s.chat_search_owner();
        s.chat.searches.view(session.clone()).input.text = "same".into();
        s.chat_action(session.scope.clone(), Action::Edit(7));
        s.profile_intent = crate::source::ProfileIntent::NativeSelf;
        let bot = s.chat_search_owner();
        s.chat.searches.view(bot.clone()).input.text = "missing".into();
        s.chat_action(bot.scope.clone(), Action::Edit(7));
        s.connection_epoch += 1;
        let other_source = s.chat_search_owner();
        s.chat.searches.view(other_source.clone()).input.text = "same".into();
        s.chat_action(other_source.scope.clone(), Action::Edit(7));
        assert_ne!(session, bot);
        assert_ne!(bot, other_source);
        assert_eq!(s.chat.searches.views[&session].search.matches.len(), 1);
        assert!(s.chat.searches.views[&bot].search.matches.is_empty());
        assert_eq!(s.chat.searches.views[&other_source].search.matches.len(), 1);
        s.connection_epoch -= 1;
        s.profile_intent = crate::source::ProfileIntent::Backend;
        assert_eq!(s.chat_search_owner(), session);
        s.chat_action(session.scope.clone(), Action::Cancel);
        assert_eq!(s.chat.searches.views[&session].search.matches.len(), 1);
        assert_eq!(
            s.chat.searches.views[&session].search.matches[0].part,
            "part-0"
        );
        assert_eq!(s.chat.searches.views[&fresh].search.matches[0].part, "edit");
    }
    #[test]
    fn media_membership_refreshes_active_literal_label_ranges() {
        use crate::{app::Viewer, gateway::HistoryMessage};
        let mut viewer = Viewer::empty();
        viewer.messages = vec![HistoryMessage::from_row(&serde_json::json!({
            "row_id": 9, "role": "assistant", "content": "", "parts": [
                {"type":"image", "url":"https://hidden.invalid/a", "name":"photo.png"}
            ]
        }))];
        let owner = viewer.chat_search_owner();
        viewer.chat.searches.view(owner.clone()).input.text = "photo".into();
        viewer.refresh_chat_search();
        assert_eq!(
            viewer.chat.searches.views[&owner].search.matches[0].bytes,
            vec![5..10]
        );
        let key = (owner.scope.clone(), "https://hidden.invalid/a".to_string());
        viewer.update_chat_media(|chat| {
            chat.media
                .insert(key.clone(), crate::chat::search::synthetic_media());
        });
        assert_eq!(viewer.chat.searches.views[&owner].search.matches.len(), 1);
        assert_eq!(
            viewer.chat.searches.views[&owner].search.matches[0].bytes,
            vec![0..5]
        );
        viewer.update_chat_media(|chat| {
            chat.media.remove(&key);
        });
        assert_eq!(
            viewer.chat.searches.views[&owner].search.matches[0].bytes,
            vec![5..10]
        );
    }
    #[test]
    fn literal_title_and_argument_highlights_are_mounted() {
        let mut s = Viewer::empty();
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id": 7, "role":"assistant", "content":"", "parts":[
                {"type":"tool-call", "name":"tool_unique", "input":"_literal*<İ>_", "output":"result"},
                {"type":"file", "path":"/hidden/path", "name":"report_unique.pdf"}
            ]}),
        )];
        let owner = s.chat_search_owner();
        s.chat.searches.view(owner.clone()).open = true;
        s.chat.searches.view(owner.clone()).input.text = "tool_unique".into();
        s.refresh_chat_search();
        let hit = s.chat.searches.views[&owner].search.current().unwrap();
        let mark = format!(
            "search-{}-{}-{}-{}",
            hit.message, hit.part, hit.bytes[0].start, hit.bytes[0].end
        );
        let mut rt = Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            Env::new(true, 1.),
        );
        rt.invalidate();
        rt.frame(0.);
        assert!(rt.find(&mark).is_some());
        let scope = rt.state.composer_scope();
        rt.state
            .chat_action(scope, Action::SetDisclosure("row-7-part-0".into(), true));
        rt.state.chat.searches.view(owner.clone()).input.text = "_literal*<".into();
        rt.state.refresh_chat_search();
        let hit = rt.state.chat.searches.views[&owner]
            .search
            .current()
            .unwrap();
        let mark = format!(
            "search-{}-{}-{}-{}",
            hit.message, hit.part, hit.bytes[0].start, hit.bytes[0].end
        );
        rt.invalidate();
        rt.frame(0.);
        assert!(rt.find(&mark).is_some());
        rt.state.chat.searches.view(owner.clone()).input.text = "report_unique.pdf".into();
        rt.state.refresh_chat_search();
        let hit = rt.state.chat.searches.views[&owner]
            .search
            .current()
            .unwrap();
        let mark = format!(
            "search-{}-{}-{}-{}",
            hit.message, hit.part, hit.bytes[0].start, hit.bytes[0].end
        );
        rt.invalidate();
        rt.frame(0.);
        assert!(rt.find(&mark).is_some());
    }
    #[test]
    fn visible_live_fragments_have_mounted_highlights_and_refresh_with_fixture_updates() {
        let mut s = Viewer::empty();
        let owner = s.chat_search_owner();
        let fixture = |reason: &str, output: &str| {
            json!({
                "running": true, "execution_generation": 12,
                "reasoning": reason,
                "activities": [{"id":"call-a", "state":"complete", "name":"terminal", "arguments":"_arg*needle_", "result":output}]
            })
        };
        s.conversation
            .fixture(
                owner.scope.clone(),
                &fixture("reason needle", "output needle"),
            )
            .unwrap();
        let live = s.conversation.view(&owner.scope).unwrap();
        let reasoning_key = format!("{}-reasoning", crate::view::transcript::live_turn_key(live));
        let tool_key = crate::view::transcript::live_tool_key(live, &live.activities[0]);
        s.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure(reasoning_key.clone(), true),
        );
        s.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure(tool_key.clone(), true),
        );
        s.chat.searches.view(owner.clone()).open = true;
        s.chat.searches.view(owner.clone()).input.text = "needle".into();
        s.refresh_chat_search();
        assert_eq!(s.chat.searches.views[&owner].search.matches.len(), 3);
        let marks: Vec<_> = s.chat.searches.views[&owner]
            .search
            .matches
            .iter()
            .map(|hit| {
                format!(
                    "search-{}-{}-{}-{}",
                    hit.message,
                    hit.part,
                    hit.bytes[0].start,
                    hit.bytes.last().unwrap().end
                )
            })
            .collect();
        let mut rt = Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            Env::new(true, 1.),
        );
        rt.frame(0.);
        for mark in marks {
            assert!(rt.find(&mark).is_some(), "missing {mark}");
        }
        rt.state
            .chat_action(owner.scope.clone(), Action::SetDisclosure(tool_key, false));
        assert_eq!(rt.state.chat.searches.views[&owner].search.matches.len(), 1);
        rt.state
            .conversation
            .fixture(
                owner.scope.clone(),
                &fixture("reason needle needle", "output needle"),
            )
            .unwrap();
        rt.state.refresh_chat_search();
        assert_eq!(rt.state.chat.searches.views[&owner].search.matches.len(), 2);
        rt.state.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure(reasoning_key, false),
        );
        assert!(
            rt.state.chat.searches.views[&owner]
                .search
                .matches
                .is_empty()
        );
    }
    #[test]
    fn reused_tool_id_in_an_earlier_turn_does_not_hide_live_output() {
        let mut s = Viewer::empty();
        let owner = s.chat_search_owner();
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id": 4, "role":"assistant", "content":"", "tool_calls":[{"id":"same", "function":{"name":"terminal", "arguments":"old"}}]}),
        )];
        s.conversation.fixture(owner.scope.clone(), &json!({"running":true,"execution_generation":2,"activities":[{"id":"same","name":"terminal","arguments":"live_unique"}]})).unwrap();
        let live = s.conversation.view(&owner.scope).unwrap();
        let key = crate::view::transcript::live_tool_key(live, &live.activities[0]);
        s.chat_action(owner.scope.clone(), Action::SetDisclosure(key, true));
        s.chat.searches.view(owner.clone()).input.text = "live_unique".into();
        s.refresh_chat_search();
        assert_eq!(s.chat.searches.views[&owner].search.matches.len(), 1);
    }
    #[test]
    fn receipt_row_reconciles_tool_hit_without_selecting_earlier_reused_id() {
        let mut s = Viewer::empty();
        let owner = s.chat_search_owner();
        s.conversation.fixture(owner.scope.clone(), &json!({"execution_generation":2,"activities":[{"id":"same","name":"terminal","arguments":"needle"}]})).unwrap();
        let live = s.conversation.view(&owner.scope).unwrap();
        let turn = crate::view::transcript::live_turn_key(live);
        let tool = crate::view::transcript::live_tool_key(live, &live.activities[0]);
        s.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure(tool.clone(), true),
        );
        s.chat.searches.view(owner.clone()).input.text = "needle".into();
        s.refresh_chat_search();
        assert_eq!(
            s.chat.searches.views[&owner]
                .search
                .current()
                .unwrap()
                .message,
            tool
        );
        s.messages = vec![4, 8].into_iter().map(|row| HistoryMessage::from_row(&json!({"row_id":row,"role":"assistant","content":"","tool_calls":[{"id":"same","function":{"name":"terminal","arguments":"needle"}}]}))).collect();
        s.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure("row-4-part-0".into(), true),
        );
        s.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure("row-8-part-0".into(), true),
        );
        s.conversation.fixture_search_bridge(&owner.scope, turn, 8);
        s.refresh_chat_search();
        assert_eq!(
            s.chat.searches.views[&owner]
                .search
                .current()
                .unwrap()
                .message,
            "row-8"
        );
    }
    #[test]
    fn queued_media_delta_polls_through_viewer_and_refreshes_mounted_search() {
        let mut s = Viewer::empty();
        s.selected_session = Some("fixture".into());
        let owner = s.chat_search_owner();
        s.conversation
            .fixture(owner.scope.clone(), &json!({"execution_generation":2}))
            .unwrap();
        s.chat.searches.view(owner.clone()).input.text = "needle".into();
        s.chat.searches.view(owner.clone()).open = true;
        let mut rt = Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            Env::new(true, 1.),
        );
        let mut offset = 0;
        for (seq, text) in [(1, r#"MEDIA:"/private/needle"#), (2, r#".txt""#)] {
            let event = crate::gateway::conversation::Event {
                session: owner.scope.session.clone().unwrap_or_default(),
                kind: "message.delta".into(),
                payload: json!({"text":text,"turn_id":"t","segment":0,"offset":offset}),
                epoch: None,
                sequence: Some(seq),
                generation: Some(2),
                authority: None,
                admission: None,
            };
            offset += text.encode_utf16().count();
            rt.state
                .conversation
                .fixture_queue_event(owner.scope.clone(), event);
            assert!(rt.state.poll_conversations());
            rt.invalidate();
            rt.frame(0.);
        }
        let raw = &rt
            .state
            .conversation
            .view(&owner.scope)
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content;
        assert_eq!(raw, r#"MEDIA:"/private/needle.txt""#);
        let search = &rt.state.chat.searches.views[&owner].search;
        assert_eq!(search.matches.len(), 1);
        let hit = search.current().unwrap();
        let mark = format!(
            "search-{}-{}-{}-{}",
            hit.message,
            hit.part,
            hit.bytes[0].start,
            hit.bytes.last().unwrap().end
        );
        assert!(rt.find(&mark).is_some());
    }
    #[test]
    fn active_query_tracks_live_events_and_error_output() {
        let mut s = Viewer::empty();
        let owner = s.chat_search_owner();
        s.conversation
            .fixture(owner.scope.clone(), &json!({"execution_generation":2}))
            .unwrap();
        s.chat.searches.view(owner.clone()).input.text = "needle".into();
        let event = |kind: &str, seq, payload| crate::gateway::conversation::Event {
            session: owner.scope.session.clone().unwrap_or_default(),
            kind: kind.into(),
            payload,
            epoch: None,
            sequence: Some(seq),
            generation: Some(2),
            authority: None,
            admission: None,
        };
        assert!(s.conversation.fixture_event(
            &owner.scope,
            &event("reasoning.delta", 1, json!({"text":"reason needle"}))
        ));
        let key = format!(
            "{}-reasoning",
            crate::view::transcript::live_turn_key(s.conversation.view(&owner.scope).unwrap())
        );
        s.chat_action(owner.scope.clone(), Action::SetDisclosure(key, true));
        assert_eq!(s.chat.searches.views[&owner].search.matches.len(), 1);
        assert!(s.conversation.fixture_event(&owner.scope, &event("tool.complete", 2, json!({"tool_call_id":"call", "name":"terminal", "arguments":"needle", "error":"needle"}))));
        let live = s.conversation.view(&owner.scope).unwrap();
        let key = crate::view::transcript::live_tool_key(live, &live.activities[0]);
        s.chat_action(owner.scope.clone(), Action::SetDisclosure(key, true));
        assert_eq!(s.chat.searches.views[&owner].search.matches.len(), 3);
    }
    #[test]
    fn mounted_message_editor_ctrl_f_opens_owned_search() {
        let mut s = Viewer::empty();
        s.connection_form = false;
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id": 7, "role":"user", "content":"needle"}),
        )];
        let owner = s.chat_search_owner();
        let mut rt = Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            Env::new(true, 1.),
        );
        rt.on_key = Some(|s, event, tx| s.chat_search_key(event, tx));
        rt.frame(0.);
        rt.update(|s, _| s.chat_action(owner.scope.clone(), Action::Edit(7)));
        rt.frame(0.);
        let edit_key = rt.state.chat_edit_key();
        click(&mut rt, &edit_key);
        key(&mut rt, Key::Command('f'), false);
        assert!(
            rt.state
                .chat
                .searches
                .views
                .get(&owner)
                .is_some_and(|v| v.open)
        );
    }
    #[test]
    fn focused_editor_controls_search_and_other_editors_keep_shortcuts() {
        let mut s = Viewer::empty();
        s.connection_form = false;
        s.messages = vec![
            crate::gateway::HistoryMessage::text("assistant", "needle needle"),
            HistoryMessage::from_row(&json!({"row_id": 7, "role": "user", "content": "other"})),
        ];
        s.chat_action(s.composer_scope(), Action::Edit(7));
        let mut rt = Runtime::new(s, view, (500., 300.), Env::new(true, 1.));
        rt.on_key = Some(|s, event, tx| s.chat_search_key(event, tx));
        rt.frame(0.);
        click(&mut rt, "chat-edit-input-foreign-editor");
        key(&mut rt, Key::Command('f'), false);
        assert!(rt.state.chat.searches.views.is_empty());
        click(&mut rt, "composer-input");
        key(&mut rt, Key::Command('f'), false);
        let owner = rt.state.chat_search_owner();
        assert!(rt.state.chat.searches.views[&owner].open);
        key(&mut rt, Key::Text("needle".into()), false);
        assert_eq!(rt.state.chat.searches.views[&owner].search.matches.len(), 2);
        key(&mut rt, Key::Command('g'), false);
        assert_eq!(rt.state.chat.searches.views[&owner].search.ordinal(), 2);
        key(&mut rt, Key::Command('g'), true);
        assert_eq!(rt.state.chat.searches.views[&owner].search.ordinal(), 1);
        key(&mut rt, Key::Escape, false);
        assert!(!rt.state.chat.searches.views[&owner].open);
    }
    #[test]
    fn real_scope_and_editor_identity_isolate_searches() {
        let mut s = Viewer::empty();
        let a = s.chat_search_owner();
        s.chat.searches.view(a.clone()).input.text = "first".into();
        s.connection_epoch += 1;
        let b = s.chat_search_owner();
        assert_ne!(a, b);
        s.connection_epoch -= 1;
        s.composer = crate::composer::Composer::new();
        let c = s.chat_search_owner();
        assert_ne!(a, c);
        assert!(s.chat.searches.views.get(&b).is_none());
        assert!(s.chat.searches.views.get(&c).is_none());
        assert_eq!(s.chat.searches.views[&a].input.text, "first");
    }
}

#[cfg(test)]
mod literal_inventory_tests {
    use super::*;
    use crate::{chat::Action, gateway::HistoryMessage};
    use serde_json::json;

    fn fixture() -> Viewer {
        let mut s = Viewer::empty();
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id": 7, "role":"assistant", "content":"duplicate_marker", "parts":[
                {"type":"tool-call", "name":"tool_unique", "input":"_literal*<İ>_", "output":"result"},
                {"type":"file", "path":"/hidden/private/path", "name":"report_unique.pdf"},
                {"type":"image", "url":"https://hidden.example/image", "name":"image_unique.png"},
                {"type":"text", "text":"duplicate_marker"}
            ]}),
        )];
        let owner = s.chat_search_owner();
        s.chat.searches.view(owner).open = true;
        s
    }
    fn query(s: &mut Viewer, q: &str) -> usize {
        let owner = s.chat_search_owner();
        s.chat.searches.view(owner.clone()).input.text = q.into();
        s.refresh_chat_search();
        s.chat.searches.views[&owner].search.matches.len()
    }
    #[test]
    fn assistant_media_search_uses_visible_normalized_text() {
        let mut s = Viewer::empty();
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id":7,"role":"assistant","content":"é before\nMEDIA:/private/a.wav\nafter [ordinary](https://example.invalid/a.wav)"}),
        )];
        let owner = s.chat_search_owner();
        s.chat.searches.view(owner.clone()).open = true;
        assert_eq!(query(&mut s, "a.wav"), 1);
        assert_eq!(query(&mut s, "MEDIA"), 0);
        assert_eq!(query(&mut s, "private"), 0);
        assert_eq!(query(&mut s, "ordinary"), 1);
        assert_eq!(query(&mut s, "before"), 1);
        assert_eq!(
            s.chat.searches.views[&owner]
                .search
                .current()
                .unwrap()
                .bytes[0],
            3..9
        );
    }

    #[test]
    fn markdown_audio_labels_are_excluded_but_surrounding_text_and_table_links_remain() {
        let mut s = Viewer::empty();
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id":7,"role":"assistant","content":
                "é before [hiddenlabel](/tmp/a.wav) after [ordinary](https://example.test/a.mp3) ` [code](/tmp/c.wav) `\n\n| head |\n| --- |\n| [tablelabel](/tmp/t.wav) |"
            }),
        )];
        let owner = s.chat_search_owner();
        s.chat.searches.view(owner.clone()).open = true;
        assert_eq!(query(&mut s, "hiddenlabel"), 0);
        assert_eq!(query(&mut s, "a.wav"), 1);
        assert_eq!(query(&mut s, "before"), 1);
        let hit = s.chat.searches.views[&owner].search.current().unwrap();
        assert_eq!(hit.bytes[0], 3..9);
        assert_eq!(query(&mut s, "ordinary"), 1);
        assert_eq!(query(&mut s, "code"), 1);
        assert_eq!(query(&mut s, "tablelabel"), 1);
        assert_eq!(query(&mut s, "before"), 1);
        let hit = s.chat.searches.views[&owner].search.current().unwrap();
        let key = format!(
            "search-{}-{}-{}-{}",
            hit.message,
            hit.part,
            hit.bytes[0].start,
            hit.bytes.last().unwrap().end
        );
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        rt.frame(0.);
        rt.frame(0.);
        assert!(mark_is_visible(&rt, &key));
    }

    #[test]
    fn expanded_tool_arguments_are_literal_and_closed_arguments_are_hidden() {
        let mut s = fixture();
        let scope = s.composer_scope();
        s.chat_action(
            scope.clone(),
            Action::SetDisclosure("row-7-part-0".into(), true),
        );
        assert_eq!(query(&mut s, "_literal*<"), 1);
        s.chat_action(scope, Action::SetDisclosure("row-7-part-0".into(), false));
        assert_eq!(query(&mut s, "_literal*<"), 0);
    }
    #[test]
    fn displayed_fragments_follow_disclosure_and_exclude_hidden_targets() {
        let mut s = fixture();
        let scope = s.composer_scope();
        s.chat_action(
            scope.clone(),
            Action::SetDisclosure("row-7-part-0".into(), false),
        );
        assert_eq!(query(&mut s, "duplicate_marker"), 1);
        assert_eq!(query(&mut s, "tool_unique"), 1);
        assert_eq!(query(&mut s, "report_unique.pdf"), 1);
        assert_eq!(query(&mut s, "image_unique.png"), 1);
        assert_eq!(query(&mut s, "hidden"), 0);
        assert_eq!(query(&mut s, "_literal*<"), 0);
        s.chat_action(
            scope.clone(),
            Action::SetDisclosure("row-7-part-0".into(), true),
        );
        assert_eq!(query(&mut s, "_literal*<"), 1);
        assert_eq!(query(&mut s, "i\u{307}"), 1);
        let owner = s.chat_search_owner();
        let hit = s.chat.searches.views[&owner].search.current().unwrap();
        assert_eq!(hit.part, "part-0-args");
        assert_eq!(query(&mut s, "tool_unique"), 1);
        s.chat_action(scope, Action::SetDisclosure("row-7-part-0".into(), false));
        assert_eq!(query(&mut s, "_literal*<"), 0);
    }
    // Insert into app/conversation/search.rs's existing test module after source grant.
    fn mark_is_visible(rt: &rune::Runtime<Viewer>, key: &str) -> bool {
        rt.elements().iter().any(|el| {
            el.role == "mark"
                && el.key.as_deref() == Some(key)
                && el.visible[2] > 0.0
                && el.visible[3] > 0.0
        })
    }

    #[test]
    fn navigating_to_second_match_reveals_the_actual_mark() {
        let mut s = Viewer::empty();
        s.connection_form = false;
        let body = format!(
            "needle\n\n{}\n\nneedle\n\n{}",
            "filler words ".repeat(1200),
            "trailing words ".repeat(1200)
        );
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id":7,"role":"assistant","content":body}),
        )];
        let owner = s.chat_search_owner();
        let v = s.chat.searches.view(owner.clone());
        v.open = true;
        v.input.text = "needle".into();
        s.refresh_chat_search();
        let hit = s.chat.searches.views[&owner].search.matches[1].clone();
        let key = format!(
            "search-{}-{}-{}-{}",
            hit.message,
            hit.part,
            hit.bytes.first().unwrap().start,
            hit.bytes.last().unwrap().end
        );
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::search_test_timeline,
            (800., 600.),
            rune::Env::new(true, 1.),
        );
        rt.poll = Some(|s, _, tx| s.poll_chat_search_reveal(tx));
        rt.frame(1.0);
        rt.update(|s, tx| tx.scroll_to_y(s.chat_history_key(), 0.0));
        rt.frame(1.0);
        assert!(
            !mark_is_visible(&rt, &key),
            "fixture must start with second match offscreen"
        );
        rt.update(|s, tx| s.reveal_chat_search(Some(true), tx));
        rt.frame(1.0);
        rt.frame(1.0);
        assert!(
            mark_is_visible(&rt, &key),
            "selected mark must intersect history clip"
        );
    }

    #[test]
    fn deep_tool_hit_intersects_nested_and_short_history_clips() {
        let mut s = Viewer::empty();
        s.connection_form = false;
        let mut rows: Vec<_> = (1..=70)
            .map(|row| {
                HistoryMessage::from_row(
                    &json!({"row_id":row,"role":"assistant","content":"earlier filler"}),
                )
            })
            .collect();
        let output = format!(
            "{}\n\nunique **nee**dle\n\n{}",
            "deep filler ".repeat(1200),
            "trailing ".repeat(500)
        );
        rows.push(HistoryMessage::from_row(
            &json!({"row_id":71,"role":"assistant","content":"",
        "parts":[{"type":"tool-call","name":"terminal","input":"args","output":output}]}),
        ));
        s.messages = rows;
        let owner = s.chat_search_owner();
        s.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure("row-71-part-0".into(), true),
        );
        let v = s.chat.searches.view(owner.clone());
        v.open = true;
        v.input.text = "needle".into();
        s.refresh_chat_search();
        let hit = s.chat.searches.views[&owner]
            .search
            .current()
            .unwrap()
            .clone();
        assert!(hit.bytes.len() > 1, "fixture must cross Markdown markup");
        let key = format!(
            "search-{}-{}-{}-{}",
            hit.message,
            hit.part,
            hit.bytes.first().unwrap().start,
            hit.bytes.last().unwrap().end
        );
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::short_search_test_timeline,
            (800., 180.),
            rune::Env::new(true, 1.),
        );
        rt.poll = Some(|s, _, tx| s.poll_chat_search_reveal(tx));
        rt.frame(1.0);
        rt.update(|s, tx| tx.scroll_to_y(s.chat_history_key(), 0.0));
        rt.frame(1.0);
        assert!(
            !mark_is_visible(&rt, &key),
            "fixture must start with row offscreen"
        );
        rt.update(|s, tx| s.reveal_chat_search(None, tx));
        rt.frame(1.0);
        rt.frame(1.0);
        rt.frame(1.0);
        assert!(mark_is_visible(&rt, &key), "mark must survive both clips");
    }

    #[test]
    fn pending_reveal_is_retired_on_owner_or_query_change() {
        let mut s = Viewer::empty();
        s.connection_form = false;
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id":7,"role":"assistant","content":"needle"}),
        )];
        let owner = s.chat_search_owner();
        let view = s.chat.searches.view(owner.clone());
        view.open = true;
        view.input.text = "needle".into();
        let mut tx = Tx::default();
        s.reveal_chat_search(None, &mut tx);
        assert!(s.chat.searches.views[&owner].pending.is_some());
        s.connection_epoch += 1;
        s.poll_chat_search_reveal(&mut Tx::default());
        assert!(s.chat.searches.views[&owner].pending.is_none());
        s.connection_epoch -= 1;
        s.reveal_chat_search(None, &mut Tx::default());
        s.chat.searches.view(owner.clone()).input.text = "different".into();
        s.poll_chat_search_reveal(&mut Tx::default());
        assert!(s.chat.searches.views[&owner].pending.is_none());
    }

    #[test]
    fn scoped_ancestor_reveal_keeps_other_tree_and_nearest_api_unchanged() {
        fn scene(_: &(), _: &mut rune::Cx<()>) -> rune::El {
            let tree = |key: &str| {
                let content = format!("{}needle{}", "line\n".repeat(50), "\nline".repeat(20));
                let start = content.find("needle").unwrap();
                rune::col()
                    .key(key.to_owned())
                    .size(200., 120.)
                    .scroll(rune::ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(
                        rune::col()
                            .w_full()
                            .child(rune::el().h(240.))
                            .child(
                                rune::col()
                                    .key("inner")
                                    .w_full()
                                    .h(360.)
                                    .scroll(rune::ScrollSpec {
                                        stick_bottom: false,
                                    })
                                    .child(
                                        rune::text(content)
                                            .w_full()
                                            .font(rune::TextStyle::new(14., 400.))
                                            .text_highlights(vec![
                                                rune::TextHighlight::new(
                                                    start..start + 6,
                                                    rune::Color::WHITE,
                                                )
                                                .key("needle"),
                                            ]),
                                    ),
                            )
                            .child(rune::el().h(240.)),
                    )
            };
            rune::row().child(tree("left")).child(tree("right"))
        }
        let mut rt = rune::Runtime::new((), scene, (400., 120.), rune::Env::new(true, 1.));
        rt.frame(1.0);
        rt.update(|_, tx| tx.scroll_into_view_in("right", "needle"));
        rt.frame(1.0);
        let marks = |rt: &rune::Runtime<()>| {
            rt.elements()
                .into_iter()
                .filter(|e| e.role == "mark" && e.key.as_deref() == Some("needle"))
                .collect::<Vec<_>>()
        };
        let nearest = marks(&rt);
        assert_eq!(nearest.len(), 2);
        assert_eq!(nearest[1].visible[3], 0., "ordinary API stays nearest-only");
        rt.update(|_, tx| tx.scroll_highlight_into_view_all_in("right", "needle"));
        rt.frame(1.0);
        let all = marks(&rt);
        assert_eq!(all[0].visible[3], 0., "other scope must not scroll");
        assert!(
            all[1].visible[2] > 0. && all[1].visible[3] > 0.,
            "target must intersect every clip: {:?}",
            all[1]
        );
    }

    #[test]
    fn live_reasoning_hit_has_a_row_and_is_revealed() {
        let mut s = Viewer::empty();
        s.connection_form = false;
        let owner = s.chat_search_owner();
        let reason = format!(
            "{}needle{}",
            "filler ".repeat(1200),
            "trailing ".repeat(500)
        );
        s.conversation
            .fixture(
                owner.scope.clone(),
                &json!({"running":true,"execution_generation":12,"reasoning":reason}),
            )
            .unwrap();
        let live = s.conversation.view(&owner.scope).unwrap();
        let row = crate::view::transcript::live_turn_key(live);
        s.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure(format!("{row}-reasoning"), true),
        );
        let view = s.chat.searches.view(owner.clone());
        view.open = true;
        view.input.text = "needle".into();
        s.refresh_chat_search();
        let hit = s.chat.searches.views[&owner]
            .search
            .current()
            .unwrap()
            .clone();
        let key = format!(
            "search-{}-{}-{}-{}",
            hit.message,
            hit.part,
            hit.bytes[0].start,
            hit.bytes.last().unwrap().end
        );
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::short_search_test_timeline,
            (800., 180.),
            rune::Env::new(true, 1.),
        );
        rt.poll = Some(|s, _, tx| s.poll_chat_search_reveal(tx));
        rt.frame(1.0);
        rt.update(|s, tx| s.reveal_chat_search(None, tx));
        for _ in 0..4 {
            rt.frame(1.0);
        }
        assert!(rt.find(&format!("message-{row}")).is_some());
        assert!(mark_is_visible(&rt, &key));
    }

    #[test]
    fn editable_hit_reveals_without_moving_selection() {
        let mut s = Viewer::empty();
        s.connection_form = false;
        let body = format!("{}needle{}", "line\n".repeat(50), "\nline".repeat(20));
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id":7,"role":"user","content":body}),
        )];
        let owner = s.chat_search_owner();
        s.chat_action(owner.scope.clone(), Action::Edit(7));
        let before = s.chat.views[&owner.scope].edit.as_ref().unwrap().1.range();
        let view = s.chat.searches.view(owner.clone());
        view.open = true;
        view.input.text = "needle".into();
        s.refresh_chat_search();
        let hit = s.chat.searches.views[&owner]
            .search
            .current()
            .unwrap()
            .clone();
        let key = format!(
            "search-{}-{}-{}-{}",
            hit.message,
            hit.part,
            hit.bytes[0].start,
            hit.bytes.last().unwrap().end
        );
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::short_search_test_timeline,
            (800., 180.),
            rune::Env::new(true, 1.),
        );
        rt.poll = Some(|s, _, tx| s.poll_chat_search_reveal(tx));
        rt.frame(1.0);
        rt.update(|s, tx| s.reveal_chat_search(None, tx));
        for _ in 0..4 {
            rt.frame(1.0);
        }
        assert!(mark_is_visible(&rt, &key));
        assert_eq!(
            rt.state.chat.views[&owner.scope]
                .edit
                .as_ref()
                .unwrap()
                .1
                .range(),
            before
        );
    }
    #[test]
    fn live_tool_hit_has_stable_row_and_is_revealed() {
        let mut s = Viewer::empty();
        s.connection_form = false;
        let owner = s.chat_search_owner();
        let output = format!(
            "{}needle{}",
            "filler ".repeat(1200),
            "trailing ".repeat(500)
        );
        s.conversation.fixture(owner.scope.clone(), &json!({"running":true,"execution_generation":12,"activities":[{"id":"call-a","name":"terminal","result":output}]})).unwrap();
        let live = s.conversation.view(&owner.scope).unwrap();
        let row = crate::view::transcript::live_tool_key(live, &live.activities[0]);
        s.chat_action(
            owner.scope.clone(),
            Action::SetDisclosure(row.clone(), true),
        );
        let view = s.chat.searches.view(owner.clone());
        view.open = true;
        view.input.text = "needle".into();
        s.refresh_chat_search();
        let hit = s.chat.searches.views[&owner]
            .search
            .current()
            .unwrap()
            .clone();
        let key = format!(
            "search-{}-{}-{}-{}",
            hit.message,
            hit.part,
            hit.bytes[0].start,
            hit.bytes.last().unwrap().end
        );
        let mut rt = rune::Runtime::new(
            s,
            crate::view::transcript::short_search_test_timeline,
            (800., 180.),
            rune::Env::new(true, 1.),
        );
        rt.poll = Some(|s, _, tx| s.poll_chat_search_reveal(tx));
        rt.frame(1.0);
        rt.update(|s, tx| s.reveal_chat_search(None, tx));
        for _ in 0..4 {
            rt.frame(1.0);
        }
        assert!(rt.find(&format!("message-{row}")).is_some());
        assert!(mark_is_visible(&rt, &key));
    }
    #[test]
    fn real_route_and_editor_roundtrips_retire_pending_without_losing_query() {
        let mut s = Viewer::empty();
        crate::app::reader_scene(&mut s);
        s.draft_storage_available = false;
        s.connection_form = false;
        s.selected_session = Some("fixture-a".into());
        s.init_editors(std::sync::Arc::new(|| {}));
        assert!(s.ensure_editor());
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id":7,"role":"assistant","content":"needle"}),
        )];
        let owner = s.chat_search_owner();
        let view = s.chat.searches.view(owner.clone());
        view.open = true;
        view.input.text = "needle".into();
        s.reveal_chat_search(None, &mut Tx::default());
        assert!(s.chat.searches.views[&owner].pending.is_some());
        assert!(s.ensure_editor());
        assert!(
            s.chat.searches.views[&owner].pending.is_some(),
            "same-owner ensure must not cancel"
        );
        s.open_route(crate::pages::Route::Settings);
        s.open_route(crate::pages::Route::Chat);
        assert!(
            s.chat.searches.views[&owner].pending.is_none(),
            "route roundtrip must cancel synchronously"
        );
        assert_eq!(s.chat.searches.views[&owner].input.text, "needle");
        s.reveal_chat_search(None, &mut Tx::default());
        assert!(s.chat.searches.views[&owner].pending.is_some());
        s.open_session("fixture-b".into());
        s.open_session("fixture-a".into());
        assert!(
            s.chat.searches.views[&owner].pending.is_none(),
            "editor roundtrip must not restore pending"
        );
        assert_eq!(s.chat.searches.views[&owner].input.text, "needle");
    }
    #[test]
    fn source_roundtrip_retires_pending_without_polling() {
        let mut s = Viewer::empty();
        crate::app::reader_scene(&mut s);
        s.draft_storage_available = false;
        s.connection_form = false;
        let connection = |name: &str| crate::gateway::Connection::Url {
            endpoint: format!("https://{name}.fixture.invalid"),
            token: Some("synthetic".into()),
        };
        s.connection = Some(connection("a"));
        s.connection_epoch = crate::app::windows::source_epoch();
        s.selected_session = Some("same".into());
        s.init_editors(std::sync::Arc::new(|| {}));
        assert!(s.ensure_editor());
        s.messages = vec![HistoryMessage::from_row(
            &json!({"row_id":7,"role":"assistant","content":"needle"}),
        )];
        let owner = s.chat_search_owner();
        let view = s.chat.searches.view(owner.clone());
        view.open = true;
        view.input.text = "needle".into();
        s.reveal_chat_search(None, &mut Tx::default());
        assert!(s.chat.searches.views[&owner].pending.is_some());
        assert!(s.use_connection_for_profile(connection("b"), Some("default".into())));
        assert!(s.use_connection_for_profile(connection("a"), Some("default".into())));
        assert!(s.chat.searches.views[&owner].pending.is_none());
        assert_eq!(s.chat.searches.views[&owner].input.text, "needle");
    }
}
