//! Deferred policy guards use the same retained canonical confirmation path.
use super::*;
use crate::chat::SlashIntent;
use serde_json::{Value, json};
impl Viewer {
    pub fn conversation_bypass_busy(&self, owner: &DraftScope) -> bool {
        self.chat
            .views
            .get(owner)
            .is_some_and(|v| v.slash_running || v.slash_intent.is_some())
            || self
                .conversation
                .view(owner)
                .is_some_and(|v| v.preparing || v.uncertain.is_some())
    }
    pub fn can_set_conversation_bypass(&self, owner: &DraftScope) -> bool {
        !self.fixture
            && owner.session.is_some()
            && owner == &self.composer_scope()
            && owner.connection == self.connection_epoch
            && self.connection.is_some()
            && self.read_only_reason().is_none()
            && !self
                .retired_profiles
                .contains(&(owner.connection, owner.profile.clone()))
            && !self.conversation_bypass_busy(owner)
            && self
                .conversation
                .view(owner)
                .is_some_and(|v| !v.runtime.is_empty() && v.policy.yolo.is_some())
    }
    pub fn set_conversation_bypass(&mut self, owner: DraftScope, enabled: bool) {
        if !self.can_set_conversation_bypass(&owner) {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let Some(view) = self.conversation.view(&owner) else {
            return;
        };
        let Ok(principal) = crate::client::connection_key(&connection) else {
            return;
        };
        let intent = SlashIntent {
            id: uuid::Uuid::new_v4().to_string(),
            principal,
            profile: owner.profile.clone(),
            stored: owner.session.clone(),
            input: format!("/yolo {}", if enabled { "on" } else { "off" }),
            runtime: Some(view.runtime.clone()),
            revision: Some(view.revision),
            generation: Some(view.generation),
            method: Some("config.set".into()),
            params: json!({"session_id":view.runtime,"key":"yolo","value":if enabled {"1"}else{"0"}}),
            attempted: false,
            config_ack: None,
            policy: SendInput {
                policy_stamp: self.composer.draft(&owner).map(|d| d.policy_stamp()),
                ..Default::default()
            },
        };
        if let Err(error) = self.chat.start_slash(connection, owner.clone(), intent) {
            self.chat.presentation(owner).error = Some(error);
        }
    }
    pub(super) fn finish_conversation_bypass(
        &mut self,
        owner: DraftScope,
        intent: SlashIntent,
        result: Result<Value, crate::gateway::conversation::CallError>,
    ) {
        match result {
            Ok(value) => {
                self.chat.presentation(owner.clone()).slash_intent = None;
                if self
                    .conversation
                    .bypass_receipt(&owner, &value["session_bypass"])
                {
                    if let Some(view) = self.conversation.view(&owner) {
                        self.reconcile_editor_policy(&owner, &view.policy.clone());
                    }
                    let view = self.chat.presentation(owner);
                    view.error = value["warning"].as_str().map(str::to_owned);
                    view.notice = view
                        .error
                        .is_none()
                        .then(|| value["output"].as_str().map(str::to_owned))
                        .flatten();
                } else {
                    if let Some(connection) = self.connection.clone() {
                        let _ = self.conversation.reconnect(connection, &owner);
                    }
                    self.chat.presentation(owner).notice=Some("The conversation changed while its bypass state was being confirmed. Reading its current state.".into());
                }
            }
            Err(error) => {
                let view = self.chat.presentation(owner);
                view.slash_intent = error.uncertain.then_some(intent);
                view.error = Some(error.message);
            }
        }
    }
    pub(super) fn poll_policy(&mut self) -> bool {
        let owner = self.composer_scope();
        let Some(mode) = self.client_settings.confirmed_approval_mode(&owner) else {
            return false;
        };
        let mut changed = self.reconcile_editor_approval(&owner, &mode);
        for (scope, policy) in self.conversation.profile_approval(&owner, &mode) {
            changed |= self.reconcile_editor_policy(&scope, &policy);
        }
        changed
    }
    pub(super) fn chat_policy_event(&mut self, owner: &DraftScope, value: &Value) {
        if value["operation"] != "model" {
            return;
        }
        if value["status"] == "refused" {
            self.chat.presentation(owner.clone()).notice = Some(format!(
                "The requested model was not applied: {}",
                value["reason"]
                    .as_str()
                    .unwrap_or("the owner refused the change")
            ));
            return;
        }
        if value["status"] != "confirmation_required" {
            return;
        }
        let Some(token) = value["confirm"].as_str() else {
            return;
        };
        let Some(model) = value["payload"]["model"].as_str() else {
            return;
        };
        let Some(connection) = &self.connection else {
            return;
        };
        let Some(view) = self.conversation.view(owner) else {
            return;
        };
        if self
            .chat
            .views
            .get(owner)
            .is_some_and(|v| v.slash_running || v.slash_intent.is_some())
        {
            return;
        }
        let draft = self.composer.draft(owner).cloned().unwrap_or_default();
        let Ok(principal) = crate::client::connection_key(connection) else {
            return;
        };
        let intent = SlashIntent {
            id: uuid::Uuid::new_v4().to_string(),
            principal,
            profile: owner.profile.clone(),
            stored: owner.session.clone(),
            input: format!("/model {model}"),
            runtime: Some(view.runtime.clone()),
            revision: Some(view.revision),
            generation: Some(view.generation),
            method: Some("session.mutate".into()),
            params: json!({"session_id":view.runtime,"request_id":uuid::Uuid::new_v4().to_string(),"expected_revision":view.revision,"expected_generation":view.generation,"operation":"model","payload":value["payload"]}),
            attempted: false,
            config_ack: None,
            policy: SendInput {
                policy_stamp: Some(draft.policy_stamp()),
                ..Default::default()
            },
        };
        let presentation = self.chat.presentation(owner.clone());
        presentation.slash_intent = Some(intent);
        presentation.slash_confirmation = Some((
            token.into(),
            value["confirm_message"]
                .as_str()
                .unwrap_or("Confirm the deferred model change")
                .into(),
        ));
    }
}
