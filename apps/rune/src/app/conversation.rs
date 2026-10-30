use super::*;
#[path = "conversation/chat.rs"]
mod chat;
#[path = "conversation/policy.rs"]
mod policy;
use crate::conversation::{AttachmentInput, SendInput, Update};
impl Viewer {
    pub fn new_draft(&mut self) {
        self.cancel_device_route();
        let scope = self.composer_scope();
        if self.conversation.view(&scope).is_some_and(|v| v.preparing) {
            self.compose_error = Some("Wait for the prepared send to bind its conversation".into());
            return;
        }
        if self.prepare_window_new_session() {
            return;
        }
        if !self.has_pinned_window_default() && self.prepare_device_new_session() {
            return;
        }
        self.new_draft_on_current_source();
    }
    pub fn new_draft_on_current_source(&mut self) {
        let scope = self.composer_scope();
        if self.conversation.view(&scope).is_some_and(|v| v.preparing) {
            self.compose_error = Some("Wait for the prepared send to bind its conversation".into());
            return;
        }
        self.cancel_device_route();
        self.layout.open("workspace");
        if self.route == crate::pages::Route::Settings {
            self.settings.vault.dismiss();
        }
        self.route = crate::pages::Route::Chat;
        self.selected_session = None;
        self.ensure_editor();
        self.messages.clear();
        self.history_loading = false;
        self.history_truncated = false;
        self.bot_empty = false;
        self.pending_bot = None;
        self.compose_error = None;
        self.composer.close();
    }
    pub fn send_draft(&mut self) {
        if !self.ensure_editor() {
            return;
        }
        self.compose_error = None;
        if let Some(reason) = self.read_only_reason() {
            self.compose_error = Some(reason.into());
            return;
        }
        if !self.can_send() {
            return;
        }
        if self.fixture {
            self.compose_error = Some("Sending is disabled in synthetic scenes".into());
            return;
        }
        let Some(connection) = self.connection.clone() else {
            self.compose_error = Some("Connect a Hermes gateway first".into());
            return;
        };
        let scope = self.composer_scope();
        let Some(draft) = self.composer.draft(&scope).cloned() else {
            return;
        };
        if crate::chat::parse_slash(&draft.text.text).is_some()
            && draft.attachments.is_empty()
            && draft.replay.is_none()
        {
            self.dispatch_chat_slash(scope, draft.text.text.clone());
            return;
        }
        let replay = match draft
            .replay
            .as_ref()
            .map(|r| {
                r.verify(&connection, &scope)
                    .and_then(|_| r.with_text(&draft.text.text))
            })
            .transpose()
        {
            Ok(r) => r,
            Err(error) => {
                self.compose_error = Some(error);
                return;
            }
        };
        let input = SendInput {
            replay,
            policy_stamp: Some(draft.policy_stamp()),
            canonical_bot: self.bot_empty,
            mode: self.admission_mode,
            text: draft.text.text.clone(),
            attachments: draft
                .attachments
                .iter()
                .map(|a| AttachmentInput {
                    path: a.path.clone(),
                    name: a.name.clone(),
                })
                .collect(),
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
        if !self.chat_running() {
            self.capture_review_turn(scope.clone());
        }
        match self.conversation.send(connection, scope.clone(), input) {
            Ok(id) => {
                self.policy_sends.insert(id, draft);
                let draft = self.composer.draft_mut(scope);
                draft.text.replace_text("");
                draft.attachments.clear();
                draft.replay = None;
            }
            Err(error) => self.compose_error = Some(error),
        }
    }
    pub fn stop_conversation(&mut self, scope: DraftScope) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(e) = self.conversation.stop(connection, &scope) {
                self.compose_error = Some(e);
            }
        }
    }
    pub fn retry_send(&mut self, scope: DraftScope, id: String) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(e) = self.conversation.retry(connection, scope, &id) {
                self.compose_error = Some(e);
            }
        }
    }
    pub fn reconnect_conversation(&mut self) {
        if self.fixture {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            let scope = self.composer_scope();
            if let Err(e) = self.conversation.reconnect(connection, &scope) {
                self.compose_error = Some(e);
            }
        }
    }
    pub fn respond_prompt(&mut self, scope: DraftScope, id: String, answer: String) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(e) = self.conversation.respond(connection, &scope, &id, &answer) {
                self.compose_error = Some(e);
            }
        }
    }
    pub fn cancel_admission(&mut self, scope: DraftScope, id: String) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(e) = self.conversation.cancel_admission(connection, &scope, &id) {
                self.compose_error = Some(e);
            }
        }
    }
    pub fn poll_conversations(&mut self) -> bool {
        let updates = self.conversation.poll();
        let mut changed = !updates.is_empty();
        for update in updates {
            match update {
                Update::BackgroundRefresh { scope, runtime } => {
                    self.trigger_background(scope, runtime)
                }
                Update::ProfileApprovalSet { scope, mode } => {
                    self.reconcile_editor_approval(&scope, &mode);
                    self.client_settings.confirm_approval_mode(&scope, &mode);
                    let effects = self.client_settings.refresh();
                    self.page_requests(effects);
                }
                Update::StreamOpened { scope } => {
                    self.notifications.stream_opened(&scope);
                    self.background.stream_opened(&scope);
                }
                Update::NoticeEvent {
                    scope,
                    kind,
                    payload,
                    runtime,
                    replay,
                } => self.notification_event(&scope, &kind, &payload, &runtime, replay),
                Update::AgentEvent {
                    scope,
                    kind,
                    payload,
                } => {
                    self.terminal.agent_event(&scope, &kind, &payload);
                    self.subagents.event(&scope, &kind, &payload);
                    if kind == "session.updated" {
                        self.chat_policy_event(&scope, &payload);
                    }
                }
                Update::Bound { origin, stored } => {
                    let destination = DraftScope {
                        session: Some(stored.clone()),
                        ..origin.clone()
                    };
                    let active = self.composer_scope() == origin;
                    self.chat.rehome(&origin, destination.clone());
                    self.subagents.rehome(&origin, &destination);
                    self.audio.rehome(&origin, destination.clone());
                    self.terminal.rehome(&origin, destination.clone());
                    if let Some(completion) = self.review_completions.remove(&origin) {
                        self.review_completions
                            .insert(destination.clone(), completion);
                    }
                    if self.spoken_scope.as_ref() == Some(&origin) {
                        self.spoken_scope = Some(destination.clone());
                    }
                    if self.voice_interrupted.remove(&origin) {
                        self.voice_interrupted.insert(destination.clone());
                    }
                    if self.audio_owner.as_ref() == Some(&origin) {
                        self.audio_owner = Some(destination.clone());
                    }
                    if let Some(id) = self.voice_sends.remove(&origin) {
                        self.voice_sends.insert(destination.clone(), id);
                    }
                    if let Some(sequence) = self.spoken_completion.remove(&origin) {
                        self.spoken_completion.insert(destination.clone(), sequence);
                    }
                    self.rehome_editors(&origin, destination);
                    if origin.connection == self.connection_epoch {
                        if !self
                            .tabs
                            .iter()
                            .any(|t| t.profile == origin.profile && t.session == stored)
                        {
                            self.tabs.push(ConversationTab {
                                profile: origin.profile.clone(),
                                session: stored.clone(),
                                title: self
                                    .conversation
                                    .view(&DraftScope {
                                        session: Some(stored.clone()),
                                        ..origin.clone()
                                    })
                                    .filter(|v| !v.title.is_empty())
                                    .map(|v| v.title.clone())
                                    .unwrap_or_else(|| "New conversation".into()),
                                bot: self
                                    .conversation
                                    .view(&DraftScope {
                                        session: Some(stored.clone()),
                                        ..origin.clone()
                                    })
                                    .is_some_and(|v| v.canonical_bot),
                            });
                        }
                        if active {
                            self.selected_session = Some(stored);
                            self.history_loading = false;
                            self.bot_empty = false;
                        }
                    }
                }
                Update::Changed { scope } => {
                    if let Some(view) = self.conversation.view(&scope) {
                        self.reconcile_editor_policy(&scope, &view.policy.clone());
                    }
                    if let Some(view) = self.conversation.view(&scope) {
                        let previous = self
                            .review_completions
                            .insert(scope.clone(), view.completion);
                        if previous.is_some_and(|p| view.completion > p)
                            && self.review.owner.as_ref() == Some(&scope)
                        {
                            let effects = self.review.refresh();
                            self.page_requests(effects);
                        }
                    }

                    if scope == self.composer_scope() {
                        if let Some(view) = self.conversation.view(&scope) {
                            self.messages = view.messages.clone();
                            self.history_truncated = view.truncated;
                            self.history_loading = false;
                        }
                    }
                }
                Update::Admitted {
                    scope,
                    submission_id,
                    restored_replay,
                    policy_stamp,
                    policy_fields,
                    ..
                } => {
                    if let Some(replay) = restored_replay {
                        let restored = if let Some(editor) =
                            self.editor_for_policy_mut(&scope, policy_stamp.as_ref())
                        {
                            let draft = editor.draft_mut(scope.clone());
                            if draft.text.text.is_empty()
                                && draft.attachments.is_empty()
                                && draft.replay.is_none()
                            {
                                draft.text.replace_text(replay.visible.clone());
                                draft.replay = Some(replay.clone());
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if !restored {
                            let view = self.chat.presentation(scope.clone());
                            view.restored_candidate = Some(replay);
                            view.notice = Some(
                                "Restored input is saved. Your current draft was preserved.".into(),
                            );
                        }
                    }
                    self.policy_sends.remove(&submission_id);
                    if let Some(stamp) = &policy_stamp {
                        self.settle_editor_policy(&scope, stamp, policy_fields);
                    }
                    if let Some(view) = self.conversation.view(&scope) {
                        self.reconcile_editor_policy(&scope, &view.policy.clone());
                    }
                    self.recovery_scope = None;
                    if scope == self.composer_scope() {
                        self.compose_error = None;
                    }
                }
                Update::Failed { scope, message, .. } => {
                    if scope == self.composer_scope() {
                        self.compose_error = Some(message);
                    }
                }
            }
        }
        if !self.fixture
            && !self.history_loading
            && !self.audio_blocks_send()
            && self.selected_session.is_some()
            && self.read_only_reason().is_none()
        {
            let scope = self.composer_scope();
            if !self
                .retired_profiles
                .contains(&(scope.connection, scope.profile.clone()))
                && !self.chat_attached.contains(&scope)
            {
                if let Some(connection) = self.connection.clone() {
                    self.chat_attached.insert(scope.clone());
                    if let Err(error) = self.conversation.attach(connection, scope) {
                        self.compose_error = Some(error);
                    }
                }
            }
        }
        let scope = self.composer_scope();
        if !self.fixture
            && self.connection.is_some()
            && (self.recovery_scope.as_ref() != Some(&scope) || changed)
        {
            self.recovery_scope = Some(scope.clone());
            self.refresh_recoverable();
        }
        changed |= self.poll_policy();
        changed
    }
}

impl Viewer {
    pub fn can_send(&self) -> bool {
        self.editor_ready()
            && self.window_source_valid()
            && !self.device_route_pending()
            && !self.loading
            && self.error.is_none()
            && !self.audio_blocks_send()
            && self.connection.is_some()
            && !self.fixture
            && !self.history_loading
            && self.read_only_reason().is_none()
            && !self
                .chat
                .views
                .get(&self.composer_scope())
                .is_some_and(|v| v.slash_running || v.slash_intent.is_some())
            && !self
                .conversation
                .view(&self.composer_scope())
                .is_some_and(|v| {
                    v.preparing || v.uncertain.is_some() || v.model_confirmation.is_some()
                })
    }
    pub fn chat_running(&self) -> bool {
        self.conversation
            .view(&self.composer_scope())
            .is_some_and(|v| v.running)
    }
    pub fn confirm_send_model(&mut self, scope: DraftScope, id: String, revision: u64) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self.conversation.confirm_model(
                connection,
                scope.clone(),
                &id,
                self.composer.draft(&scope).map(|d| d.policy_stamp()),
                revision,
            ) {
                self.compose_error = Some(error);
            }
        }
    }
    pub fn ask_discard_send(&mut self, scope: DraftScope, id: String) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = &self.connection {
            match self.conversation.saved_revision(connection, &scope, &id) {
                Ok(revision) => {
                    self.discard_send_confirm = Some(SavedSendConfirmation {
                        scope,
                        id,
                        revision,
                    })
                }
                Err(error) => self.compose_error = Some(error),
            }
        }
    }
    pub fn discard_send(&mut self, scope: DraftScope, id: String, revision: u64) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self
                .conversation
                .discard(&connection, &scope, &id, revision)
            {
                self.compose_error = Some(error);
            } else {
                self.discard_send_confirm = None;
                self.refresh_recoverable();
            }
        }
    }
}

impl Viewer {
    pub fn refresh_recoverable(&mut self) {
        if let Some(connection) = &self.connection {
            match self
                .conversation
                .recoverable(connection, &self.composer_scope())
            {
                Ok(entries) => self.recoverable_sends = entries,
                Err(error) => self.client_error = Some(error),
            }
        }
    }
    pub fn steer_admission(&mut self, scope: DraftScope, id: String) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self.conversation.steer_admission(connection, &scope, &id) {
                self.compose_error = Some(error);
            }
        }
    }
}

impl Viewer {
    pub fn is_local_connection(&self) -> bool {
        self.connection
            .as_ref()
            .is_some_and(|connection| matches!(connection.transport(), Connection::Local { .. }))
    }
}

impl Viewer {
    pub fn open_run_session(
        &mut self,
        profile: String,
        id: String,
        title: String,
        read_only: bool,
    ) {
        let scope = DraftScope {
            connection: self.connection_epoch,
            profile: profile.clone(),
            session: Some(id.clone()),
        };
        if read_only {
            self.read_only_chats.insert(scope,"This scheduled run has no confirmed live owner. Start a new conversation to continue.".into());
        } else {
            self.read_only_chats.remove(&scope);
        }
        self.open_page_session(profile, id, title);
    }
    pub fn read_only_reason(&self) -> Option<&str> {
        self.read_only_chats
            .get(&self.composer_scope())
            .map(String::as_str)
    }
}

impl Viewer {
    pub fn lock_question(
        &mut self,
        scope: DraftScope,
        id: String,
        qid: String,
        answer: Option<String>,
    ) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self
                .conversation
                .lock_clarify(connection, &scope, &id, &qid, answer)
            {
                self.compose_error = Some(error);
            }
        }
    }
    pub fn skip_questions(&mut self, scope: DraftScope, id: String) {
        if self.fixture || scope.connection != self.connection_epoch {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self.conversation.skip_clarify(connection, &scope, &id) {
                self.compose_error = Some(error);
            }
        }
    }
    pub fn question_answer(&self, scope: &DraftScope, id: &str, qid: &str, multi: bool) -> String {
        if multi {
            serde_json::to_string(
                &self
                    .prompt_choices
                    .get(&(scope.clone(), id.into(), qid.into()))
                    .cloned()
                    .unwrap_or_default(),
            )
            .unwrap_or_else(|_| "[]".into())
        } else {
            self.prompt_responses
                .get(&(scope.clone(), serde_json::to_string(&(id, qid)).unwrap()))
                .map(|e| e.text.clone())
                .unwrap_or_default()
        }
    }
}

impl Viewer {
    pub fn is_fixture(&self) -> bool {
        self.fixture
    }
}

impl Viewer {
    pub fn begin_saved_connection_change(&mut self) -> bool {
        if self.allow_owner_change("") {
            self.plugin_hosts.close();
            true
        } else {
            false
        }
    }
    pub fn poll_connections(&mut self) -> bool {
        if self.fixture {
            return false;
        }
        let events = self.connections.poll();
        let changed = !events.is_empty();
        for event in events {
            match event{
   crate::connections::Event::Ready(connection)=>{
    let profile=self.connections.selected_profile().map(str::to_owned);
    let retained=self.connections.active_lease().zip(self.connections.resolver());
    if self.use_connection_for_profile(connection,profile) {
     if let Some((lease,resolver))=retained { self.retain_window_route(resolver,lease); }
    }
    self.connections.note_active_source(self.connection.as_ref());
   },
   crate::connections::Event::Changed=>{},
   crate::connections::Event::Forgot{id}=>{
    if let Some(store)=&mut self.device_startup {
     if let Err(error)=store.after_forget(id,true){self.client_error=Some(error);}
    }
   },
   crate::connections::Event::Lost{id,revision,message}=>{if self.connection.as_ref().is_some_and(|connection|matches!(connection,Connection::Saved{id:current,revision:current_revision,..}if current==&id&&current_revision==&revision)){self.conversation.close_connection(self.connection_epoch);self.chat.close_connection(self.connection_epoch);self.subagents.close_connection(self.connection_epoch);self.audio.close_connection(self.connection_epoch);self.background.retire_connection(self.connection_epoch);self.preview.close_connection(self.connection_epoch);self.media_players.close_connection(self.connection_epoch);self.audio_preview=None;self.error=Some(message);self.status="Disconnected".into();self.loading=false;self.history_loading=false;}}
  }
        }
        changed
    }
}
