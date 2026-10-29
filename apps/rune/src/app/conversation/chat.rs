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
            Action::Image(source) => {
                let v = self.chat.presentation(owner);
                if v.preview.as_ref() == Some(&source) {
                    v.preview = None;
                } else {
                    v.preview = Some(source);
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
            Action::ChooseSlash(name) => self
                .composer
                .draft_mut(owner)
                .text
                .replace_text(format!("/{name} ")),
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
        let projected = projection::project(&[self.messages[index].clone()]);
        if projected
            .iter()
            .flat_map(|m| &m.parts)
            .any(|p| matches!(p, Part::Image { .. } | Part::File { .. }))
        {
            self.chat.presentation(owner).error=Some("This turn contains attachments. Automatic attachment replay is not ported yet; its history has not been changed.".into());
            return;
        }
        let text = edited.unwrap_or_else(|| self.messages[index].content.clone());
        let later = self.messages[index + 1..]
            .iter()
            .filter(|m| m.role == "user")
            .count();
        let view = self.chat.presentation(owner);
        view.confirmation = Some(Confirmation {
            row,
            kind,
            text,
            later,
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
        let input = SendInput {
            text: confirmation.text.clone(),
            rewind: Some(crate::conversation::Rewrite {
                target: confirmation.row,
                revision: view.revision,
                generation: view.generation,
                restore_only: matches!(confirmation.kind, RewriteKind::Restore),
            }),
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
        match self.conversation.send(connection, owner.clone(), input) {
            Ok(id) => {
                self.policy_sends.insert(id, draft);
                if matches!(confirmation.kind, RewriteKind::Restore) {
                    let draft = self.composer.draft_mut(owner.clone());
                    if draft.text.text.trim().is_empty() {
                        draft.text.replace_text(confirmation.text);
                    }
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
    pub fn poll_chat(&mut self, _tx: &mut Tx) -> bool {
        let scope = self.composer_scope();
        let replies = self.chat.poll();
        let mut changed = !replies.is_empty();
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
                        Ok(value) => view.slash = chat::catalog_rows(&value),
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
                    self.finish_chat_slash(owner, input, result);
                }
                chat::Reply::Media { .. } | chat::Reply::MediaReady => {}
            }
        }
        // Only the foreground transcript queues previews; all results retain
        // their full owner and cannot paint a later profile's same path.
        if !self.fixture && self.route == crate::pages::Route::Chat {
            if let Some(connection) = self.connection.clone() {
                self.chat.recover_branches(&connection, &scope);
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
                    .take(32)
                    .collect();
                for source in sources {
                    if !self
                        .chat
                        .media
                        .contains_key(&(scope.clone(), source.clone()))
                    {
                        changed = true;
                    }
                    self.chat
                        .ensure_media(connection.clone(), scope.clone(), source);
                }
                let slash = self
                    .composer
                    .draft(&scope)
                    .is_some_and(|d| chat::parse_slash(&d.text.text).is_some());
                if slash {
                    self.chat.catalog(connection, scope);
                }
            }
        }
        changed
    }
    pub fn dispatch_chat_slash(&mut self, owner: DraftScope, input: String) {
        let Some((name, arg)) = chat::parse_slash(&input) else {
            return;
        };
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
        let runtime = self.conversation.view(&owner).map(|v| v.runtime.clone());
        match self
            .chat
            .dispatch(connection, owner.clone(), input.clone(), runtime)
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
    ) {
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                let draft = self.composer.draft_mut(owner.clone());
                if draft.text.text.trim().is_empty() {
                    draft.text.replace_text(input);
                }
                self.chat.presentation(owner).error = Some(e);
                return;
            }
        };
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
                let draft = self.composer.draft(&owner).cloned().unwrap_or_default();
                let prompt = SendInput {
                    text: message.into(),
                    display_text: Some(value["display"].as_str().unwrap_or(&input).into()),
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
                match self.conversation.send(connection, owner.clone(), prompt) {
                    Ok(id) => {
                        self.policy_sends.insert(id, draft);
                    }
                    Err(e) => self.chat.presentation(owner).error = Some(e),
                }
            }
            "prefill" => {
                if let Some(message) = value["message"].as_str() {
                    self.composer.draft_mut(owner).text.replace_text(message);
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
