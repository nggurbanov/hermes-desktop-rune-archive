//! Audio controls keep the microphone, drafts and agent turn on one conversation owner.
use super::*;
use crate::{
    audio::{Status, Update as AudioUpdate},
    gateway::VoiceEngine,
};
impl Viewer {
    pub fn toggle_dictation(&mut self) {
        if self.fixture {
            return;
        }
        let scope = self.composer_scope();
        if self
            .audio
            .view(&scope)
            .is_some_and(|v| matches!(v.status, Status::Listening | Status::Starting))
        {
            if let Err(error) = self.audio.stop_dictation(&scope) {
                self.compose_error = Some(error);
            }
            return;
        }
        self.wake_enabled = false;
        if let Some(connection) = self.connection.clone() {
            match self.audio.start_dictation(connection, scope.clone()) {
                Ok(()) => self.audio_owner = Some(scope),
                Err(error) => self.compose_error = Some(error),
            }
        }
    }
    pub fn start_voice_chat(&mut self) {
        if self.fixture
            || self.history_loading
            || self.read_only_reason().is_some()
            || self.chat_running()
        {
            return;
        }
        let scope = self.composer_scope();
        let engine = self
            .audio
            .view(&scope)
            .and_then(|v| v.preferred_engine.or(v.engine))
            .or_else(|| self.voice.status(&scope).map(|v| v.engine));
        let Some(engine) = engine else {
            self.reload_voice_status();
            self.compose_error =
                Some("Wait for the agent's voice status before starting voice chat".into());
            return;
        };
        if let Some(connection) = self.connection.clone() {
            self.audio.cancel(&scope);
            self.wake_enabled = false;
            let history = serde_json::json!(
                self.messages
                    .iter()
                    .map(|m| serde_json::json!({"role":m.role,"content":m.content}))
                    .collect::<Vec<_>>()
            );
            match self
                .audio
                .start_conversation(connection, scope.clone(), engine, history)
            {
                Ok(()) => self.audio_owner = Some(scope),
                Err(error) => self.compose_error = Some(error),
            }
        }
    }
    pub fn finish_voice_turn(&mut self) {
        if self.fixture {
            return;
        }
        let scope = self.composer_scope();
        if let Err(error) = self.audio.stop_turn(&scope) {
            self.compose_error = Some(error);
        }
    }
    pub fn end_voice_chat(&mut self) {
        let scope = self.composer_scope();
        self.audio.cancel(&scope);
        self.voice_sends.remove(&scope);
        self.voice_interrupted.remove(&scope);
        self.audio_owner = None;
        self.wake_enabled = false;
    }
    pub fn toggle_spoken_replies(&mut self) {
        self.speak_replies = !self.speak_replies;
        let scope = self.composer_scope();
        if let Some(view) = self.conversation.view(&scope) {
            self.spoken_completion.insert(scope, view.completion);
        }
    }
    pub fn toggle_wake(&mut self) {
        if self.fixture {
            return;
        }
        let scope = self.composer_scope();
        self.wake_enabled = !self.wake_enabled;
        self.audio.cancel(&scope);
        if self.wake_enabled {
            self.start_wake_for(scope);
        } else {
            self.audio_owner = None;
        }
    }
    fn start_wake_for(&mut self, scope: DraftScope) {
        if let Some(connection) = self.connection.clone() {
            match self.audio.start_wake(connection, scope.clone()) {
                Ok(()) => self.audio_owner = Some(scope),
                Err(error) => {
                    self.wake_enabled = false;
                    self.compose_error = Some(error);
                }
            }
        }
    }
    pub fn select_voice_engine(&mut self, scope: DraftScope, engine: VoiceEngine) {
        if self.fixture || scope != self.composer_scope() {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self.audio.set_engine(connection, scope, engine) {
                self.compose_error = Some(error);
            }
        }
    }
    pub fn poll_audio(&mut self) -> bool {
        let scope = self.composer_scope();
        if self
            .audio_owner
            .as_ref()
            .is_some_and(|owner| *owner != scope)
        {
            let old = self.audio_owner.take().unwrap();
            self.audio.cancel(&old);
            self.wake_enabled = false;
        }
        if self.spoken_scope.as_ref() != Some(&scope) {
            let baseline = self
                .conversation
                .view(&scope)
                .map(|v| v.completion)
                .unwrap_or(0);
            self.spoken_completion.insert(scope.clone(), baseline);
            self.spoken_scope = Some(scope.clone());
        }
        let updates = self.audio.poll();
        let mut changed = !updates.is_empty();
        for update in updates {
            match update {
                AudioUpdate::Transcript {
                    scope: owner,
                    text,
                    submit,
                    voice_context,
                } => {
                    if owner != scope {
                        continue;
                    }
                    if self.wake_enabled {
                        let normalized = text.to_lowercase();
                        if [
                            "hey hermes",
                            "hey, hermes",
                            "эй гермес",
                            "эй, гермес",
                            "хей гермес",
                        ]
                        .iter()
                        .any(|wake| normalized.contains(wake))
                        {
                            self.wake_enabled = false;
                            self.audio.cancel(&owner);
                            self.start_voice_chat();
                        } else {
                            self.start_wake_for(owner);
                        }
                        continue;
                    }
                    if submit {
                        self.send_voice_text(owner, text, voice_context);
                    } else {
                        self.composer.draft_mut(owner).text.insert(&text);
                    }
                }
                AudioUpdate::Interrupt { scope: owner } => {
                    if owner == scope && self.conversation.view(&owner).is_some_and(|v| v.running) {
                        self.voice_interrupted.insert(owner.clone());
                        self.stop_conversation(owner);
                    }
                }
                AudioUpdate::Stopped { scope: owner } => {
                    if self.audio_owner.as_ref() == Some(&owner) {
                        self.audio_owner = None;
                    }
                    self.voice_sends.remove(&owner);
                }
                AudioUpdate::Changed { .. } => {}
            }
        }
        if self.wake_enabled
            && self
                .audio
                .view(&scope)
                .is_some_and(|v| v.status == Status::Idle)
        {
            if self
                .audio
                .view(&scope)
                .and_then(|v| v.error.as_deref())
                .is_some_and(|e| !e.starts_with("No speech detected"))
            {
                self.wake_enabled = false;
            } else {
                self.start_wake_for(scope.clone());
            }
        }
        if let Some(view) = self.conversation.view(&scope) {
            let live = self
                .audio
                .view(&scope)
                .is_some_and(|v| v.voice_active && v.engine == Some(VoiceEngine::GptLive));
            if live && view.running {
                if let Some(input) = self.voice_sends.get(&scope).filter(|id| {
                    (!view.canonical || view.active_input.as_ref() == Some(*id))
                        && (!view.canonical || view.active_generation == Some(view.generation))
                }) {
                    let tool = view.tools.last().and_then(|v| v.strip_prefix("Running "));
                    if let Err(error) =
                        self.audio
                            .live_progress(&scope, input, &view.active_text, tool)
                    {
                        self.compose_error = Some(error);
                    }
                }
            }
            let previous = self
                .spoken_completion
                .entry(scope.clone())
                .or_insert(view.completion);
            if view.completion > *previous {
                *previous = view.completion;
                changed = true;
                let text = view.completed_text.clone().unwrap_or_default();
                let voice = self.audio.view(&scope).is_some_and(|v| v.voice_active);
                let live =
                    self.audio.view(&scope).and_then(|v| v.engine) == Some(VoiceEngine::GptLive);
                let voice_matches = self
                    .voice_sends
                    .get(&scope)
                    .is_some_and(|id| view.completed_input.as_ref() == Some(id) || !view.canonical);
                if (voice && voice_matches) || (!voice && self.speak_replies) {
                    if let Some(connection) = self.connection.clone() {
                        let result = if voice && live {
                            self.audio
                                .complete_live(&scope, &text, view.error.is_some())
                        } else {
                            self.audio_owner = Some(scope.clone());
                            self.audio.speak(connection, scope.clone(), text)
                        };
                        self.voice_sends.remove(&scope);
                        if let Err(error) = result {
                            self.compose_error = Some(error);
                        }
                    }
                }
            }
        }
        changed
    }
    fn send_voice_text(&mut self, scope: DraftScope, text: String, context: Option<String>) {
        if self.fixture || scope != self.composer_scope() {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let draft = self.composer.draft(&scope).cloned().unwrap_or_default();
        let surface = context.as_ref().map(|_| "voice-live".to_owned());
        let baseline = self
            .conversation
            .view(&scope)
            .map(|v| v.completion)
            .unwrap_or(0);
        self.spoken_completion.insert(scope.clone(), baseline);
        let input = crate::conversation::SendInput {
            canonical_bot: self.bot_empty,
            voice_turn: true,
            voice_context: context,
            surface,
            text,
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
            interrupted: self.voice_interrupted.remove(&scope),
            ..Default::default()
        };
        if !self.chat_running() {
            self.capture_review_turn(scope.clone());
        }
        match self.conversation.send(connection, scope.clone(), input) {
            Ok(id) => {
                self.policy_sends.insert(id.clone(), draft);
                self.voice_sends.insert(scope, id);
            }
            Err(error) => {
                self.audio.cancel(&scope);
                self.compose_error = Some(error);
            }
        }
    }
    pub fn audio_needs_tick(&self) -> bool {
        self.audio
            .view(&self.composer_scope())
            .is_some_and(|v| v.status != Status::Idle || v.voice_active)
    }
    pub fn audio_blocks_send(&self) -> bool {
        self.audio.view(&self.composer_scope()).is_some_and(|v| {
            matches!(
                v.status,
                Status::Starting | Status::Listening | Status::Transcribing
            )
        })
    }
}
