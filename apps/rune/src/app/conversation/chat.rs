use super::*;
use crate::chat::{
    self, Action, Confirmation, RewriteKind,
    projection::{self, Part},
};
use rune::{TextEdit, Tx};

impl Viewer {
    pub fn chat_action(&mut self, owner: DraftScope, action: Action) {
        if owner != self.composer_scope() {
            return;
        }
        match action {
            Action::RetryMedia(source) => {
                if let Some(connection) = self.connection.clone() {
                    self.chat.retry_media(connection, owner, source);
                }
            }
            Action::Image(source) => {
                let v = self.chat.presentation(owner);
                if v.preview.as_ref() == Some(&source) {
                    v.preview = None;
                } else {
                    v.preview = Some(source);
                }
            }
            Action::SetDisclosure(key, open) => {
                let v = self.chat.presentation(owner);
                if open {
                    v.collapsed.remove(&key);
                    v.expanded.insert(key);
                } else {
                    v.expanded.remove(&key);
                    v.collapsed.insert(key);
                }
            }
            Action::Expand(key) => {
                let v = self.chat.presentation(owner);
                if !v.expanded.remove(&key) {
                    v.expanded.insert(key);
                }
            }
            Action::Quote(value) => {
                let draft = self.composer.draft_mut(owner);
                let quoted = value
                    .lines()
                    .map(|line| format!("> {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let before = draft.text.text.trim_end();
                draft.text.replace_text(format!(
                    "{before}{}{}\n\n",
                    if before.is_empty() { "" } else { "\n\n" },
                    quoted
                ));
            }
            Action::Speak(value) => {
                if self.fixture {
                    return;
                }
                if let Some(connection) = self.connection.clone() {
                    if let Err(e) = self.audio.speak(connection, owner, value) {
                        self.compose_error = Some(e);
                    }
                }
            }
            Action::File(path) => self.open_chat_file(owner, path),
            Action::Process(id) => self.reveal_chat_process(owner, id),
            Action::Edit(row) => {
                if let Some(message) = self
                    .messages
                    .iter()
                    .find(|m| m.row_id() == Some(row) && m.role == "user")
                {
                    let mut edit = TextEdit::default();
                    edit.replace_text(message.content.clone());
                    self.chat.presentation(owner).edit = Some((row, edit));
                }
            }
            Action::Retry(row) => self.prepare_chat_rewrite(owner, row, RewriteKind::Retry, None),
            Action::Restore(row) => {
                self.prepare_chat_rewrite(owner, row, RewriteKind::Restore, None)
            }
            Action::SaveEdit => {
                if let Some((row, edit)) = self.chat.views.get(&owner).and_then(|v| v.edit.as_ref())
                {
                    self.prepare_chat_rewrite(
                        owner.clone(),
                        *row,
                        RewriteKind::Edit,
                        Some(edit.text.clone()),
                    );
                }
            }
            Action::Cancel => {
                let v = self.chat.presentation(owner);
                v.edit = None;
                v.confirmation = None;
            }
            Action::Confirm => self.confirm_chat_rewrite(owner),
            Action::Branch(row) => self.branch_chat_at(owner, row),
            Action::RetryBranch => {
                if self.fixture {
                    return;
                }
                if let (Some(connection), Some(intent)) = (
                    self.connection.clone(),
                    self.chat.views.get(&owner).and_then(|v| v.branch.clone()),
                ) {
                    if let Err(error) = self.chat.branch(connection, owner.clone(), intent) {
                        self.chat.presentation(owner).error = Some(error);
                    }
                }
            }
            Action::OpenBranch => {
                if let Some((id, title)) = self
                    .chat
                    .views
                    .get(&owner)
                    .and_then(|v| v.branch_result.clone())
                {
                    self.open_page_session(owner.profile, id, title);
                }
            }
            Action::Earlier => {
                if self.fixture {
                    return;
                }
                if let Some(connection) = self.connection.clone() {
                    if let Err(e) = self.conversation.load_earlier(connection, &owner) {
                        self.chat.presentation(owner).error = Some(e);
                    }
                }
            }
            Action::ChooseSlash(name) => {
                let query = format!("/{name} ");
                self.composer
                    .draft_mut(owner.clone())
                    .text
                    .replace_text(query.clone());
                self.chat.presentation(owner).slash_dismissed = Some(query);
            }
            Action::RetrySlash => self.retry_chat_slash(owner, false),
            Action::ConfirmSlash => self.retry_chat_slash(owner, true),
            Action::ResolveSlashUnknown => {
                if let Some(intent) = self
                    .chat
                    .views
                    .get(&owner)
                    .and_then(|v| v.slash_intent.clone())
                {
                    match intent.settle(&serde_json::json!({"acknowledged_unknown":true})) {
                        Ok(()) => {
                            let view = self.chat.presentation(owner);
                            view.slash_intent = None;
                            view.slash_confirmation = None;
                            view.error = None;
                        }
                        Err(error) => self.chat.presentation(owner).error = Some(error),
                    }
                }
            }
            Action::AppendPrefill => {
                if let Some(text) = self
                    .chat
                    .presentation(owner.clone())
                    .prefill_candidate
                    .take()
                {
                    let draft = self.composer.draft_mut(owner);
                    let before = draft.text.text.trim_end();
                    draft.text.replace_text(format!(
                        "{before}{}{}",
                        if before.is_empty() { "" } else { "\n\n" },
                        text
                    ));
                }
            }
            Action::UseRestored => {
                if let Some(mut replay) = self
                    .chat
                    .presentation(owner.clone())
                    .restored_candidate
                    .clone()
                {
                    let draft = self.composer.draft_mut(owner.clone());
                    let before = draft.text.text.clone();
                    let previous = match draft
                        .replay
                        .as_ref()
                        .map(|r| r.with_text(&before))
                        .transpose()
                    {
                        Ok(v) => v,
                        Err(error) => {
                            self.chat.presentation(owner).error = Some(error);
                            return;
                        }
                    };
                    if !before.is_empty() {
                        replay.visible = format!("{before}\n\n{}", replay.visible);
                        replay.transport = format!(
                            "{}\n\n{}",
                            previous
                                .as_ref()
                                .map(|r| r.transport.as_str())
                                .unwrap_or(&before),
                            replay.transport
                        );
                    }
                    if let Some(mut previous) = previous {
                        previous.visible = replay.visible;
                        previous.transport = replay.transport;
                        previous.attachments.extend(replay.attachments);
                        replay = previous;
                    }
                    draft.text.replace_text(replay.visible.clone());
                    draft.replay = Some(replay);
                    self.chat.presentation(owner).restored_candidate = None;
                }
            }
            Action::Dismiss => {
                let v = self.chat.presentation(owner);
                v.error = None;
                v.notice = None;
            }
        }
    }
    fn prepare_chat_rewrite(
        &mut self,
        owner: DraftScope,
        row: u64,
        kind: RewriteKind,
        edited: Option<String>,
    ) {
        let Some(index) = self.messages.iter().position(|m| m.row_id() == Some(row)) else {
            return;
        };
        let target = if self.messages[index].role == "user" {
            Some(index)
        } else {
            (0..index).rev().find(|&i| self.messages[i].role == "user")
        };
        let Some(index) = target else {
            return;
        };
        let Some(row) = self.messages[index].row_id() else {
            self.chat.presentation(owner).error =
                Some("Wait for the user message to be saved before changing its history".into());
            return;
        };
        let text = edited.unwrap_or_else(|| self.messages[index].content.clone());
        let later = self.messages[index + 1..]
            .iter()
            .filter(|m| m.role == "user")
            .count();
        let replay = if let Some(connection) = self.connection.as_ref() {
            match crate::conversation::ReplayBundle::capture(
                connection,
                &owner,
                &self.messages[index],
            )
            .and_then(|r| r.with_text(&text))
            {
                Ok(replay) => Some(replay),
                Err(error) => {
                    self.chat.presentation(owner).error = Some(error);
                    return;
                }
            }
        } else {
            None
        };
        let (revision, generation, authority) = self
            .conversation
            .view(&owner)
            .map(|v| (v.revision, v.generation, v.authority))
            .unwrap_or_default();
        let view = self.chat.presentation(owner);
        view.confirmation = Some(Confirmation {
            row,
            kind,
            text,
            later,
            revision,
            generation,
            authority,
            replay,
        });
    }
    fn confirm_chat_rewrite(&mut self, owner: DraftScope) {
        if self.fixture {
            return;
        }
        let Some(confirmation) = self
            .chat
            .views
            .get(&owner)
            .and_then(|v| v.confirmation.clone())
        else {
            return;
        };
        let Some(view) = self.conversation.view(&owner) else {
            return;
        };
        if view.revision != confirmation.revision
            || view.generation != confirmation.generation
            || view.authority != confirmation.authority
        {
            let state = self.chat.presentation(owner);
            state.confirmation = None;
            state.error=Some("The conversation changed while confirmation was open. Review the current history and confirm its target again.".into());
            return;
        }
        if !view.canonical || view.running || view.preparing || view.uncertain.is_some() {
            self.chat.presentation(owner).error=Some("Stop the turn and resolve pending sends before changing history. A canonical session is required".into());
            return;
        }
        if self.read_only_reason().is_some() {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let draft = self.composer.draft(&owner).cloned().unwrap_or_default();
        let restore_only = matches!(confirmation.kind, RewriteKind::Restore);
        let input = SendInput {
            text: confirmation.text.clone(),
            replay: confirmation.replay,
            rewind: Some(crate::conversation::Rewrite {
                target: confirmation.row,
                revision: confirmation.revision,
                generation: confirmation.generation,
                restore_only: matches!(confirmation.kind, RewriteKind::Restore),
            }),
            policy_stamp: Some(draft.policy_stamp()),
            model: draft.model.clone(),
            reasoning: draft.reasoning.clone(),
            service_tier: draft.service_tier.clone(),
            approval_mode: draft.approval_mode.clone(),
            model_changed: draft.model_changed,
            reasoning_changed: draft.reasoning_changed,
            service_tier_changed: draft.service_tier_changed,
            approval_changed: draft.approval_changed,
            ..Default::default()
        };
        self.rewind_background(&owner);
        match self.conversation.send(connection, owner.clone(), input) {
            Ok(id) => {
                if !restore_only {
                    self.policy_sends.insert(id, draft);
                }

                let v = self.chat.presentation(owner);
                v.confirmation = None;
                v.edit = None;
                v.error = None;
            }
            Err(e) => self.chat.presentation(owner).error = Some(e),
        }
    }
    fn branch_chat_at(&mut self, owner: DraftScope, row: u64) {
        if self.fixture || self.read_only_reason().is_some() {
            return;
        }
        let Some(view) = self.conversation.view(&owner) else {
            return;
        };
        if view.running || view.preparing || view.uncertain.is_some() {
            self.chat.presentation(owner).error =
                Some("Stop the turn and resolve pending sends before branching".into());
            return;
        }
        if !self.messages.iter().any(|m| m.row_id() == Some(row)) {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let intent = match crate::client::connection_key(&connection) {
            Ok(key) => chat::BranchIntent {
                connection: key,
                profile: owner.profile.clone(),
                stored: owner.session.clone().unwrap_or_default(),
                runtime: view.runtime.clone(),
                request_id: uuid::Uuid::new_v4().to_string(),
                revision: view.revision,
                generation: view.generation,
                through: row,
            },
            Err(e) => {
                self.chat.presentation(owner).error = Some(e);
                return;
            }
        };
        if self
            .chat
            .views
            .get(&owner)
            .is_some_and(|v| v.branch.is_some() && v.branch_result.is_none())
        {
            self.chat.presentation(owner).error =
                Some("Resolve the retained branch action before starting another branch".into());
            return;
        }
        if let Err(error) = self.chat.branch(connection, owner.clone(), intent) {
            self.chat.presentation(owner).error = Some(error);
        }
    }
    pub fn poll_chat(&mut self, tx: &mut Tx) -> bool {
        let scope = self.composer_scope();
        let replies = self.chat.poll();
        let changed = !replies.is_empty();
        for reply in replies {
            match reply {
                chat::Reply::Catalog { owner, result } => {
                    if owner.connection != self.connection_epoch
                        || !self.chat.views.contains_key(&owner)
                        || self
                            .retired_profiles
                            .contains(&(owner.connection, owner.profile.clone()))
                    {
                        continue;
                    }
                    let view = self.chat.presentation(owner);
                    view.slash_loading = false;
                    view.slash_loaded = true;
                    match result {
                        Ok(value) => {
                            view.slash = chat::catalog_rows(&value);
                            view.slash_catalog = value;
                        }
                        Err(error) => view.error = Some(error),
                    }
                }
                chat::Reply::Branch { owner, result } => {
                    if owner.connection != self.connection_epoch
                        || !self.chat.views.contains_key(&owner)
                        || self
                            .retired_profiles
                            .contains(&(owner.connection, owner.profile.clone()))
                    {
                        continue;
                    }
                    let view = self.chat.presentation(owner);
                    view.branch_busy = false;
                    match result {
                        Ok(value) => {
                            view.branch_result = Some(value);
                            view.notice = Some("Branch created. Open it when ready.".into());
                        }
                        Err(e) => {
                            if !e.uncertain {
                                view.branch = None;
                            }
                            view.error = Some(e.message);
                        }
                    }
                }
                chat::Reply::Slash {
                    owner,
                    input,
                    intent,
                    result,
                } => {
                    if owner.connection != self.connection_epoch
                        || !self.chat.views.contains_key(&owner)
                        || self
                            .retired_profiles
                            .contains(&(owner.connection, owner.profile.clone()))
                    {
                        continue;
                    }
                    self.chat.presentation(owner.clone()).slash_running = false;
                    if intent.session_bypass() {
                        self.finish_conversation_bypass(owner, intent, result);
                        continue;
                    }
                    let policy = intent.policy.clone();
                    match result {
                        Ok(value)
                            if value["confirm_required"] == true
                                || value["status"] == "confirmation_required" =>
                        {
                            let view = self.chat.presentation(owner);
                            view.slash_intent = Some(intent);
                            view.slash_confirmation = value["confirm"].as_str().map(|token| {
                                (
                                    token.into(),
                                    value["confirm_message"]
                                        .as_str()
                                        .unwrap_or("Confirm this model change")
                                        .into(),
                                )
                            });
                            if view.slash_confirmation.is_none() {
                                view.error=Some("The model confirmation has no token. The retained command needs resolution.".into());
                            }
                        }
                        Ok(value) => {
                            self.chat.presentation(owner.clone()).slash_intent = None;
                            self.finish_chat_slash(owner, input, Ok(value), policy);
                        }
                        Err(error) => {
                            let view = self.chat.presentation(owner.clone());
                            view.slash_intent = if error.uncertain { Some(intent) } else { None };
                            self.finish_chat_slash(owner, input, Err(error.message), policy);
                        }
                    }
                }
                chat::Reply::Completions {
                    owner,
                    query,
                    result,
                } => {
                    if owner.connection == self.connection_epoch
                        && !self
                            .retired_profiles
                            .contains(&(owner.connection, owner.profile.clone()))
                    {
                        self.chat.apply_completions(&owner, &query, result);
                    }
                }
                chat::Reply::Media { .. } | chat::Reply::MediaReady => {}
            }
        }
        // Only the foreground transcript queues previews; all results retain
        // their full owner and cannot paint a later profile's same path.
        if !self.fixture && self.route == crate::pages::Route::Chat {
            if let Some(connection) = self.connection.clone() {
                self.chat.recover_branches(&connection, &scope);
                self.chat.recover_slash(&connection, &scope);
                let sources: Vec<_> = projection::project(&self.messages)
                    .iter()
                    .flat_map(|m| m.parts.iter())
                    .filter_map(|p| {
                        if let Part::Image { source, .. } = p {
                            Some(source.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                self.chat
                    .prepare_media(connection.clone(), scope.clone(), sources);
                let slash = self
                    .composer
                    .draft(&scope)
                    .is_some_and(|d| chat::parse_slash(&d.text.text).is_some());
                if slash {
                    let cwd = self.conversation.view(&scope).and_then(|v| v.cwd.clone());
                    let state = self.chat.presentation(scope.clone());
                    if state.catalog_cwd != cwd {
                        state.catalog_cwd = cwd;
                        state.slash_loaded = false;
                        state.completion_query.clear();
                        state.completion_ready.clear();
                    }
                    self.chat.catalog(connection.clone(), scope.clone());
                    let query = self
                        .composer
                        .draft(&scope)
                        .map(|d| d.text.text.clone())
                        .unwrap_or_default();
                    let runtime = self.conversation.view(&scope).map(|v| v.runtime.clone());
                    if let Some(delay) = self
                        .chat
                        .request_completions(connection, scope, query, runtime)
                    {
                        tx.wake_after(delay);
                    }
                }
            }
        }
        changed
    }
    pub fn dispatch_chat_slash(&mut self, owner: DraftScope, input: String) {
        let Some((name, arg)) = chat::parse_slash(&input) else {
            return;
        };
        if owner.session.is_none() && self.set_draft_command(&owner, &name, &arg) {
            return;
        }
        let route = match name.as_str() {
            "settings" | "config" => Some(crate::pages::Route::Settings),
            "skills" | "tools" | "mcp" => Some(crate::pages::Route::Capabilities),
            "cron" => Some(crate::pages::Route::Cron),
            _ => None,
        };
        if let Some(route) = route {
            self.open_route(route);
            return;
        }
        match name.as_str() {
            "new" | "reset" if arg.is_empty() => {
                self.new_draft();
                return;
            }
            "model" if arg.is_empty() => {
                self.open_composer(crate::composer::Popup::Models);
                return;
            }
            "branch" if arg.is_empty() => {
                if let Some(row) = self.messages.iter().rev().find_map(|m| m.row_id()) {
                    self.branch_chat_at(owner, row);
                }
                return;
            }
            "stop" => {
                self.stop_conversation(owner);
                return;
            }
            "undo" | "retry" => {
                if let Some(row) = self
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "user")
                    .and_then(|m| m.row_id())
                {
                    self.prepare_chat_rewrite(
                        owner,
                        row,
                        if name == "undo" {
                            RewriteKind::Restore
                        } else {
                            RewriteKind::Retry
                        },
                        None,
                    );
                }
                return;
            }
            _ => {}
        }
        if self.fixture {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let fence = self
            .conversation
            .view(&owner)
            .map(|v| (v.runtime.clone(), v.revision, v.generation));
        let draft = self.composer.draft(&owner).cloned().unwrap_or_default();
        let policy = SendInput {
            text: input.clone(),
            policy_stamp: Some(draft.policy_stamp()),
            model: draft.model.clone(),
            reasoning: draft.reasoning.clone(),
            service_tier: draft.service_tier.clone(),
            approval_mode: draft.approval_mode.clone(),
            cwd: draft.cwd.clone(),
            cwd_explicit: draft.cwd_explicit,
            model_changed: draft.model_changed,
            reasoning_changed: draft.reasoning_changed,
            service_tier_changed: draft.service_tier_changed,
            approval_changed: draft.approval_changed,
            ..Default::default()
        };
        match self
            .chat
            .dispatch(connection, owner.clone(), input.clone(), fence, policy)
        {
            Ok(()) => {
                let draft = self.composer.draft_mut(owner);
                if draft.text.text == input {
                    draft.text.replace_text("");
                }
            }
            Err(e) => self.compose_error = Some(e),
        }
    }
    fn finish_chat_slash(
        &mut self,
        owner: DraftScope,
        input: String,
        result: Result<serde_json::Value, String>,
        policy: SendInput,
    ) {
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                let Some(editor) = self.editor_for_policy_mut(&owner, policy.policy_stamp.as_ref())
                else {
                    self.chat.presentation(owner).failed_command = Some(input);
                    return;
                };
                let draft = editor.draft_mut(owner.clone());
                if draft.text.text.trim().is_empty() {
                    draft.text.replace_text(input.clone());
                } else {
                    self.chat.presentation(owner.clone()).failed_command = Some(input);
                }
                self.chat.presentation(owner).error = Some(e);
                return;
            }
        };
        if let Some(child) = value["mutation"]["branched_session_id"].as_str() {
            self.chat.presentation(owner.clone()).branch_result =
                Some((child.into(), "Branched conversation".into()));
        }
        if value.get("mutation").is_some() {
            let mut receipt = value["mutation"].clone();
            if receipt["operation"].is_null() {
                receipt["operation"] = value["operation"].clone();
            }
            let fields = self.conversation.policy_receipt(
                &owner,
                &receipt,
                &value["policy_payload"],
                policy.policy_stamp.clone(),
            );
            if let Some(stamp) = &policy.policy_stamp {
                self.settle_editor_policy(&owner, stamp, fields);
            }
            if let Some(view) = self.conversation.view(&owner) {
                self.reconcile_editor_policy(&owner, &view.policy.clone());
            }
            if let Some(connection) = self.connection.clone() {
                let _ = self.conversation.reconnect(connection, &owner);
            }
        }
        if value["key"] == "reasoning" && matches!(value["value"].as_str(), Some("show" | "hide")) {
            let effects = self.client_settings.refresh();
            self.page_requests(effects);
        }
        match value["type"].as_str().unwrap_or("exec") {
            "send" | "skill" => {
                let Some(message) = value["message"].as_str().filter(|s| !s.trim().is_empty())
                else {
                    self.chat.presentation(owner).error =
                        Some("This command returned no prompt expansion".into());
                    return;
                };
                let Some(connection) = self.connection.clone() else {
                    return;
                };
                let mut sent = self
                    .editor_for_policy_mut(&owner, policy.policy_stamp.as_ref())
                    .and_then(|c| c.draft(&owner))
                    .cloned()
                    .unwrap_or_default();
                if let Some(stamp) = &policy.policy_stamp {
                    sent.policy_id = stamp.draft_id.clone();
                    sent.policy_revisions = stamp.revisions;
                }
                sent.model = policy.model.clone();
                sent.reasoning = policy.reasoning.clone();
                sent.service_tier = policy.service_tier.clone();
                sent.approval_mode = policy.approval_mode.clone();
                let mut prompt = policy;
                prompt.text = message.into();
                prompt.display_text = Some(value["display"].as_str().unwrap_or(&input).into());
                match self.conversation.send(connection, owner.clone(), prompt) {
                    Ok(id) => {
                        self.policy_sends.insert(id, sent);
                    }
                    Err(e) => self.chat.presentation(owner).error = Some(e),
                }
            }
            "prefill" => {
                if let Some(message) = value["message"].as_str() {
                    let Some(editor) =
                        self.editor_for_policy_mut(&owner, policy.policy_stamp.as_ref())
                    else {
                        self.chat.presentation(owner).prefill_candidate = Some(message.into());
                        return;
                    };
                    let draft = editor.draft_mut(owner.clone());
                    if draft.text.text.is_empty() {
                        draft.text.replace_text(message);
                    } else {
                        self.chat.presentation(owner).prefill_candidate = Some(message.into());
                    }
                }
            }
            "alias" => {
                self.chat.presentation(owner).error = Some(
                    "The command returned an unresolved alias. Use its expanded command directly."
                        .into(),
                )
            }
            _ => {
                self.chat.presentation(owner).notice = value["output"]
                    .as_str()
                    .or(value["notice"].as_str())
                    .map(str::to_owned);
            }
        }
    }
}

impl Viewer {
    fn set_draft_command(&mut self, owner: &DraftScope, name: &str, arg: &str) -> bool {
        if !matches!(name, "model" | "reasoning") || arg.is_empty() {
            return false;
        }
        let result = if name == "model" {
            let words: Vec<_> = arg.split_whitespace().collect();
            let provider = if words.len() == 3 && words[1] == "--provider" {
                Some(words[2])
            } else {
                None
            };
            if words.len() != 1 && provider.is_none() {
                Err("Use the model picker for this model option".into())
            } else {
                let matches: Vec<_> = self
                    .composer
                    .catalog_for(owner)
                    .into_iter()
                    .flat_map(|c| c.providers.iter())
                    .filter(|p| provider.is_none_or(|id| id == p.id))
                    .flat_map(|p| {
                        p.models.iter().filter(|m| m.id == words[0]).map(|m| {
                            crate::composer::ModelKey {
                                provider: p.id.clone(),
                                id: m.id.clone(),
                            }
                        })
                    })
                    .collect();
                if matches.len() == 1 {
                    self.composer
                        .select_model(owner.clone(), matches[0].clone())
                } else {
                    Err("Choose one available model and provider from the model picker".into())
                }
            }
        } else {
            let value = arg.trim().to_lowercase();
            if matches!(
                value.as_str(),
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
            ) {
                let supported = self
                    .composer
                    .selected_model(owner)
                    .and_then(|(_, m)| m.capabilities.as_ref())
                    .is_some_and(|c| {
                        c.reasoning == Some(true)
                            && (value != "none" || c.can_disable_reasoning == Some(true))
                    });
                if supported {
                    self.composer.choose_reasoning(owner.clone(), Some(value));
                    Ok(())
                } else {
                    Err("This model does not advertise the requested reasoning setting".into())
                }
            } else {
                Err("Choose an existing conversation for this reasoning command".into())
            }
        };
        match result {
            Ok(()) => self.composer.draft_mut(owner.clone()).text.replace_text(""),
            Err(error) => self.chat.presentation(owner.clone()).error = Some(error),
        };
        true
    }
}

impl Viewer {
    fn retry_chat_slash(&mut self, owner: DraftScope, confirm: bool) {
        if self.fixture || owner != self.composer_scope() || self.read_only_reason().is_some() {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let Some(mut intent) = self
            .chat
            .views
            .get(&owner)
            .and_then(|v| v.slash_intent.clone())
        else {
            return;
        };
        if confirm {
            let current = self.composer.draft(&owner).map(|d| d.policy_stamp());
            let matches = match (intent.policy.policy_stamp.as_ref(), current.as_ref()) {
                (Some(sent), Some(now)) => {
                    sent.draft_id == now.draft_id
                        && sent.revisions.model == now.revisions.model
                        && (intent.params["payload"].get("reasoning").is_none()
                            || sent.revisions.reasoning == now.revisions.reasoning)
                }
                _ => false,
            };
            let same_execution = self.conversation.view(&owner).is_some_and(|v| {
                intent.runtime.as_deref() == Some(v.runtime.as_str())
                    && intent.revision == Some(v.revision)
                    && intent.generation == Some(v.generation)
            });
            if !matches || !same_execution {
                match intent.settle(&serde_json::json!({"cancelled_stale_confirmation":true})) {
                    Ok(()) => {
                        let view = self.chat.presentation(owner);
                        view.slash_intent = None;
                        view.slash_confirmation = None;
                        view.error = Some("The model choice or session changed after this confirmation opened. Submit the current choice again.".into());
                    }
                    Err(error) => self.chat.presentation(owner).error = Some(error),
                }
                return;
            }
            let Some((token, _)) = self
                .chat
                .views
                .get(&owner)
                .and_then(|v| v.slash_confirmation.clone())
            else {
                return;
            };
            intent.params["request_id"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
            intent.params["payload"]["confirm"] = serde_json::json!(token);
            self.chat.presentation(owner.clone()).slash_confirmation = None;
        }
        if let Err(error) = self.chat.start_slash(connection, owner.clone(), intent) {
            self.chat.presentation(owner).error = Some(error);
        }
    }
}

impl Viewer {
    pub fn rich_chat_fixture(&mut self) {
        self.messages = [
            serde_json::json!({"role":"user","row_id":101,"content":"Проверь, что перенесено в Rune, и покажи результат."}),
            serde_json::json!({"role":"assistant","row_id":102,"reasoning":"Сравниваю поведение с оригинальным Hermes и проверяю реальные границы профилей.","tool_calls":[{"id":"sample-read","function":{"name":"terminal","arguments":"{\"command\":\"cargo test --locked\"}"}}]}),
            serde_json::json!({"role":"tool","row_id":103,"tool_call_id":"sample-read","name":"terminal","content":"Targeted checks passed. No live agent prompts were sent."}),
            serde_json::json!({"role":"assistant","row_id":104,"content":"## Перенос чата\n\nИстория, детали вызовов и действия относятся к **этому разговору**. Запросы сохраняют профиль и подключение.\n\n- Код и таблицы отображаются Markdown-компонентом Rune.\n- Историю можно догрузить, не потеряв ранние сообщения.\n- Изменение прошлого требует подтверждения.\n\n```rust\nlet owner = conversation.scope();\n```\n\nЭто синтетический пример для проверки интерфейса, не отчёт о живом запуске.","parts":[{"type":"text","text":"## Перенос чата\n\nИстория, детали вызовов и действия относятся к **этому разговору**. Запросы сохраняют профиль и подключение.\n\n| Возможность | Результат |\n| --- | --- |\n| Markdown | Нативный компонент Rune |\n| История | Догрузка ранних сообщений |\n| Правка прошлого | Явное подтверждение |\n\n```rust\nlet owner = conversation.scope();\n```\n\nЭто синтетический пример для проверки интерфейса."},{"type":"file","path":"/synthetic/report.md","name":"report.md"}]}),
        ].iter().map(crate::gateway::HistoryMessage::from_row).collect();
        let owner = self.composer_scope();
        let value = serde_json::json!({"messages":self.messages.iter().map(|m|m.raw.clone()).collect::<Vec<_>>()});
        let _ = self.conversation.fixture(owner.clone(), &value);
        let view = self.chat.presentation(owner);
        view.expanded.insert("row-102-part-0".into());
        view.expanded.insert("row-102-part-1".into());
        view.slash_loaded = true;
        view.slash = vec![
            chat::SlashRow {
                name: "work".into(),
                description: "Run an installed skill".into(),
                skill: true,
            },
            chat::SlashRow {
                name: "model".into(),
                description: "Choose the next model".into(),
                skill: false,
            },
        ];
    }
}

impl Viewer {
    pub fn chat_completion_key(&mut self, event: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
        if event.mods.shift
            || event.mods.alt
            || event.mods.ctrl
            || event.mods.cmd
            || self.composer.popup.is_some()
        {
            return false;
        }
        let owner = self.composer_scope();
        let candidates = self.chat_completion_candidates(&owner);
        if candidates.is_empty() {
            return false;
        }
        match event.key {
            rune::Key::Down => {
                let state = self.chat.presentation(owner);
                state.slash_highlight = (state.slash_highlight + 1).min(candidates.len() - 1);
            }
            rune::Key::Up => {
                let state = self.chat.presentation(owner);
                state.slash_highlight = state.slash_highlight.saturating_sub(1);
            }
            rune::Key::Tab | rune::Key::Enter => {
                self.chat_complete_submit();
            }
            rune::Key::Escape => {
                let query = self.composer.draft(&owner).map(|d| d.text.text.clone());
                self.chat.presentation(owner).slash_dismissed = query;
            }
            _ => return false,
        }
        tx.instant();
        true
    }
    pub fn chat_completion_candidates(&self, owner: &DraftScope) -> Vec<chat::SlashRow> {
        let Some(draft) = self.composer.draft(owner) else {
            return vec![];
        };
        let Some(state) = self.chat.views.get(owner) else {
            return vec![];
        };
        if state.slash_dismissed.as_deref() == Some(draft.text.text.as_str()) {
            return vec![];
        }
        if state.completion_ready == draft.text.text && !state.completion_items.is_empty() {
            return state.completion_items.clone();
        }
        let Some((name, arg)) = chat::parse_slash(&draft.text.text) else {
            return vec![];
        };
        if !arg.is_empty() {
            return vec![];
        }
        state
            .slash
            .iter()
            .filter(|row| row.name.starts_with(&name))
            .take(30)
            .cloned()
            .collect()
    }
    pub fn chat_complete_submit(&mut self) -> bool {
        let owner = self.composer_scope();
        let candidates = self.chat_completion_candidates(&owner);
        if candidates.is_empty() {
            return false;
        }
        let index = self
            .chat
            .views
            .get(&owner)
            .map_or(0, |v| v.slash_highlight)
            .min(candidates.len() - 1);
        self.chat_action(owner, Action::ChooseSlash(candidates[index].name.clone()));
        true
    }
}

impl Viewer {
    pub fn rich_chat_replay_fixture(&mut self) {
        self.rich_chat_fixture();
        let owner = self.composer_scope();
        let connection = self
            .connection
            .clone()
            .unwrap_or(crate::gateway::Connection::Url {
                endpoint: "https://synthetic.invalid".into(),
                token: None,
            });
        let row = crate::gateway::HistoryMessage::from_row(
            &serde_json::json!({"role":"user","row_id":151,"content":[{"type":"text","text":"Продолжи с исходным изображением и заметками."},{"type":"image_url","name":"Original image","image_url":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="},{"type":"file","path":"data:text/plain;base64,T3JpZ2luYWwgbm90ZXM=","name":"Original notes.txt"}]}),
        );
        if let Ok(mut replay) =
            crate::conversation::ReplayBundle::capture(&connection, &owner, &row)
        {
            for attachment in &mut replay.attachments {
                attachment.frozen = Some(attachment.source.clone());
            }
            let draft = self.composer.draft_mut(owner);
            draft.text.replace_text(replay.visible.clone());
            draft.replay = Some(replay);
        }
    }
}
