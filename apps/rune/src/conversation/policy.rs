//! Owner-observed policy and accepted next-turn picks. Catalogs are not policy.
use crate::composer::{PolicyFields, PolicyStamp};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionPolicy {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub reasoning: Option<String>,
    pub reasoning_wire: Option<String>,
    pub service_tier: Option<String>,
    pub fast: Option<bool>,
    pub approval_mode: Option<String>,
    pub yolo: Option<bool>,
    pub edit_policy: Option<String>,
    pub deferred: Option<DeferredPolicy>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeferredPolicy {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub reasoning: Option<String>,
    pub service_tier: Option<String>,
    pub reasoning_reset: bool,
    pub reasoning_with_model: bool,
    pub stamp: Option<PolicyStamp>,
}

impl SessionPolicy {
    pub fn next_model(&self) -> Option<&str> {
        self.deferred
            .as_ref()
            .and_then(|p| p.model.as_deref())
            .or(self.model.as_deref())
    }
    pub fn next_provider(&self) -> Option<&str> {
        match self.deferred.as_ref().filter(|p| p.model.is_some()) {
            Some(pick) => pick.provider.as_deref(),
            None => self.provider.as_deref(),
        }
    }
    pub fn next_reasoning(&self) -> Option<&str> {
        if self.deferred.as_ref().is_some_and(|p| p.reasoning_reset) {
            return None;
        }
        self.deferred
            .as_ref()
            .and_then(|p| p.reasoning.as_deref())
            .or(self.reasoning.as_deref())
    }
    pub fn next_service_tier(&self) -> Option<&str> {
        self.deferred
            .as_ref()
            .and_then(|p| p.service_tier.as_deref())
            .or(self.service_tier.as_deref())
    }
    pub(crate) fn setting(&mut self, key: &str, value: String, running: bool) {
        if key == "approvals.mode" {
            self.approval_mode = Some(value);
        } else if key == "fast" {
            let value = if value == "fast" {
                "priority".into()
            } else {
                value
            };
            if running {
                self.deferred
                    .get_or_insert_with(DeferredPolicy::default)
                    .service_tier = Some(value);
            } else {
                self.service_tier = Some(value);
            }
        }
    }
    pub(crate) fn from_info(info: &Value) -> Self {
        let mut result = Self::default();
        result.info(info);
        result
    }
    pub(crate) fn info(&mut self, info: &Value) {
        let model = string(info, "model");
        let provider = string(info, "provider");
        let effort_changed = info.get("reasoning_effort").is_some()
            && string(info, "reasoning_effort") != self.reasoning;
        let route_changed = info.get("model").is_some_and(Value::is_null)
            || info.get("provider").is_some_and(Value::is_null)
            || model
                .as_ref()
                .is_some_and(|m| self.model.as_ref() != Some(m))
            || provider
                .as_ref()
                .is_some_and(|p| self.provider.as_ref() != Some(p));
        if route_changed {
            self.provider = None;
        }
        if route_changed || effort_changed {
            self.reasoning_wire = None;
        }
        if info.get("model").is_some_and(Value::is_null) {
            self.model = None;
        }
        if let Some(value) = model {
            self.model = Some(value);
        }
        if let Some(value) = provider {
            self.provider = Some(value);
        }
        for (target, key) in [
            (&mut self.reasoning, "reasoning_effort"),
            (&mut self.reasoning_wire, "reasoning_effort_wire"),
            (&mut self.service_tier, "service_tier"),
            (&mut self.approval_mode, "approval_mode"),
            (&mut self.edit_policy, "edit_approval_policy"),
        ] {
            match info.get(key) {
                Some(Value::Null) => *target = None,
                Some(Value::String(value)) => *target = Some(value.clone()),
                _ => {}
            }
        }
        for (target, key) in [(&mut self.fast, "fast"), (&mut self.yolo, "yolo")] {
            match info.get(key) {
                Some(Value::Null) => *target = None,
                Some(Value::Bool(value)) => *target = Some(*value),
                _ => {}
            }
        }
        if let Some(pick) = self.deferred.as_mut() {
            if pick
                .model
                .as_ref()
                .is_some_and(|m| self.model.as_ref() == Some(m))
                && pick
                    .provider
                    .as_ref()
                    .is_none_or(|p| self.provider.as_ref() == Some(p))
            {
                pick.model = None;
                pick.provider = None;
            }
            if pick
                .reasoning
                .as_ref()
                .is_some_and(|r| self.reasoning.as_ref() == Some(r))
            {
                pick.reasoning = None;
            }
            if pick
                .service_tier
                .as_ref()
                .is_some_and(|r| self.service_tier.as_ref() == Some(r))
            {
                pick.service_tier = None;
            }
            if pick.model.is_none()
                && pick.reasoning.is_none()
                && pick.service_tier.is_none()
                && !pick.reasoning_reset
            {
                self.deferred = None;
            }
        }
    }
    /// Called only after the transport checks the receipt's session identity.
    pub(crate) fn receipt(
        &mut self,
        value: &Value,
        payload: &Value,
        stamp: Option<PolicyStamp>,
    ) -> PolicyFields {
        let operation = value["operation"].as_str().unwrap_or("");
        let status = value["status"].as_str().unwrap_or("");
        if matches!(status, "confirmation_required" | "refused") {
            if operation == "model" {
                if let Some(pick) = self.deferred.as_mut() {
                    pick.model = None;
                    pick.provider = None;
                    if pick.reasoning_with_model {
                        pick.reasoning = None;
                        pick.reasoning_reset = false;
                        pick.reasoning_with_model = false;
                    }
                }
                if self.deferred.as_ref().is_some_and(|p| {
                    p.model.is_none()
                        && p.reasoning.is_none()
                        && p.service_tier.is_none()
                        && !p.reasoning_reset
                }) {
                    self.deferred = None;
                }
                if status == "refused" {
                    self.info(value);
                }
            }
            return PolicyFields::default();
        }
        let mut fields = PolicyFields::default();
        if operation == "model" {
            let model = string(value, "model").or_else(|| {
                (status == "deferred")
                    .then(|| string(payload, "model"))
                    .flatten()
            });
            let provider = string(value, "provider").or_else(|| {
                (status == "deferred")
                    .then(|| string(payload, "provider"))
                    .flatten()
            });
            let reasoning = string(value, "effort").or_else(|| string(payload, "reasoning"));
            fields.model = true;
            fields.reasoning = reasoning.is_some();
            if status == "deferred" {
                let pick = self.deferred.get_or_insert_with(DeferredPolicy::default);
                pick.model = model;
                pick.provider = provider;
                if reasoning.is_some() {
                    pick.reasoning = reasoning;
                    pick.reasoning_with_model = true;
                }
                pick.stamp = stamp;
            } else {
                if model
                    .as_ref()
                    .is_some_and(|m| self.model.as_ref() != Some(m))
                    || provider
                        .as_ref()
                        .is_some_and(|p| self.provider.as_ref().is_some_and(|old| old != p))
                    || reasoning
                        .as_ref()
                        .is_some_and(|r| self.reasoning.as_ref() != Some(r))
                {
                    self.reasoning_wire = None;
                }
                if model.is_some() {
                    self.model = model;
                }
                self.provider = provider;
                if reasoning.is_some() {
                    self.reasoning = reasoning;
                }
                if let Some(pick) = self.deferred.as_mut() {
                    pick.model = None;
                    pick.provider = None;
                    if fields.reasoning {
                        pick.reasoning = None;
                        pick.reasoning_reset = false;
                    }
                }
            }
        } else if operation == "reasoning" {
            // A reset's null does not reveal the profile's resolved effort.
            let effort = string(value, "effort").or_else(|| string(payload, "effort"));
            fields.reasoning = true;
            if status == "deferred" {
                let pick = self.deferred.get_or_insert_with(DeferredPolicy::default);
                pick.reasoning_with_model = false;
                pick.reasoning_reset = effort.is_none();
                pick.reasoning = effort;
                pick.stamp = stamp;
            } else {
                // A reset receipt contains null, not the resolved profile level.
                // Keep the last observed level until the owner's session.info arrives.
                if let Some(effort) = effort {
                    if self.reasoning.as_ref() != Some(&effort) {
                        self.reasoning_wire = None;
                    }
                    self.reasoning = Some(effort);
                }
                if let Some(pick) = self.deferred.as_mut() {
                    pick.reasoning = None;
                    pick.reasoning_reset = false;
                }
            }
        }
        if self.deferred.as_ref().is_some_and(|p| {
            p.model.is_none()
                && p.reasoning.is_none()
                && p.service_tier.is_none()
                && !p.reasoning_reset
        }) {
            self.deferred = None;
        }
        fields
    }
    pub(crate) fn turn_started(&mut self) {
        if let Some(pick) = self.deferred.as_mut() {
            pick.reasoning_reset = false;
        }
    }
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub(crate) fn matches_model_choice(
    captured: Option<&PolicyStamp>,
    current: Option<&PolicyStamp>,
    reasoning: bool,
) -> bool {
    match (captured, current) {
        (Some(sent), Some(now)) => {
            sent.draft_id == now.draft_id
                && sent.revisions.model == now.revisions.model
                && (!reasoning || sent.revisions.reasoning == now.revisions.reasoning)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::{Composer, DraftScope};
    use crate::conversation::{Conversation, ConversationView, Update, worker};
    use crate::gateway::conversation::Event;
    use serde_json::json;
    fn owner() -> DraftScope {
        DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 1,
            profile: "work".into(),
            session: Some("stored".into()),
        }
    }
    #[test]
    fn external_partial_route_never_carries_previous_provider_or_clamp() {
        let mut policy = SessionPolicy::from_info(
            &json!({"model":"old","provider":"custom:a","reasoning_effort":"ultra","reasoning_effort_wire":"max","yolo":false}),
        );
        policy.info(&json!({"model":"new"}));
        assert_eq!(policy.model.as_deref(), Some("new"));
        assert!(policy.provider.is_none());
        assert!(policy.reasoning_wire.is_none());
        assert!(policy.approval_mode.is_none());
        let mut composer = Composer::new();
        let scope = owner();
        composer.choose_service_tier(scope.clone(), Some("priority".into()));
        composer.reconcile_policy(&scope, &policy);
        assert_eq!(composer.model_name(&scope), Some("new"));
        assert!(composer.effective_model_key(&scope).is_none());
        assert!(composer.draft(&scope).unwrap().service_tier_changed);
        policy.info(&json!({"provider":"custom:b","reasoning_effort_wire":"high"}));
        assert_eq!(policy.provider.as_deref(), Some("custom:b"));
        assert_eq!(policy.reasoning_wire.as_deref(), Some("high"));
    }
    #[test]
    fn deferred_pick_preserves_running_route_and_refusal_restores_committed_route() {
        let mut policy = SessionPolicy::from_info(
            &json!({"model":"old","provider":"old-provider","reasoning_effort":"low"}),
        );
        let receipt = json!({"operation":"model","status":"deferred","model":"next"});
        policy.receipt(
            &receipt,
            &json!({"model":"next","provider":"next-provider","reasoning":"ultra"}),
            None,
        );
        policy.info(&json!({"model":"old","provider":"old-provider","reasoning_effort":"low"}));
        assert_eq!(policy.model.as_deref(), Some("old"));
        assert_eq!(policy.next_model(), Some("next"));
        assert_eq!(policy.next_provider(), Some("next-provider"));
        assert_eq!(policy.next_reasoning(), Some("ultra"));
        policy.receipt(&json!({"operation":"model","status":"refused","target_model":"next","model":"old","provider":"old-provider"}),&Value::Null,None);
        assert_eq!(policy.next_model(), Some("old"));
        assert!(policy.deferred.is_none());
        policy.receipt(&receipt, &json!({"model":"next"}), None);
        assert!(policy.next_provider().is_none());
    }
    #[test]
    fn accepted_cues_are_generation_fenced_and_replay_is_explicit() {
        let scope = owner();
        let mut conversation = Conversation::new();
        conversation.views.insert(
            scope.clone(),
            ConversationView {
                runtime: "live".into(),
                stored: "stored".into(),
                replay_epoch: "epoch".into(),
                authority: 7,
                generation: 4,
                canonical: true,
                ..Default::default()
            },
        );
        let send = |seq, generation, replay, payload| worker::Reply::Event {
            origin: scope.clone(),
            replay,
            event: Event {
                session: "live".into(),
                kind: "approval.request".into(),
                payload,
                epoch: Some("epoch".into()),
                sequence: Some(seq),
                generation: Some(generation),
                authority: Some(7),
                admission: None,
            },
        };
        conversation
            .replies
            .send(send(
                100,
                3,
                false,
                json!({"prompt_id":"stale","choices":["allow","deny"]}),
            ))
            .unwrap();
        assert!(conversation.poll().is_empty());
        conversation
            .replies
            .send(send(
                1,
                4,
                false,
                json!({"prompt_id":"actual","choices":["allow","deny"],"command":"test"}),
            ))
            .unwrap();
        let updates = conversation.poll();
        let notices = updates
            .iter()
            .filter_map(|u| {
                if let Update::NoticeEvent {
                    payload, replay, ..
                } = u
                {
                    Some((payload, replay))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(notices.len(), 1);
        assert!(!notices[0].1);
        assert_eq!(notices[0].0["execution_generation"], 4);
        assert_eq!(notices[0].0["choices"], json!(["allow", "deny"]));
        conversation
            .replies
            .send(send(
                1,
                4,
                false,
                json!({"prompt_id":"duplicate","choices":["allow","deny"]}),
            ))
            .unwrap();
        assert!(conversation.poll().is_empty());
        conversation
            .replies
            .send(send(
                2,
                4,
                true,
                json!({"prompt_id":"replayed","choices":["allow","deny"]}),
            ))
            .unwrap();
        assert!(
            conversation
                .poll()
                .iter()
                .any(|u| matches!(u, Update::NoticeEvent { replay: true, .. }))
        );
    }
    #[test]
    fn old_info_replayed_after_snapshot_cannot_regress_current_policy() {
        let mut view = ConversationView {
            runtime: "live".into(),
            stored: "stored".into(),
            replay_epoch: "epoch".into(),
            last_sequence: 3,
            policy_sequence: 8,
            generation: 1,
            authority: 7,
            policy: SessionPolicy::from_info(&json!({"model":"new","provider":"new-route"})),
            ..Default::default()
        };
        assert!(view.apply_event(&Event {
            session: "live".into(),
            kind: "session.info".into(),
            payload: json!({"model":"old","provider":"old-route"}),
            epoch: Some("epoch".into()),
            sequence: Some(4),
            generation: Some(1),
            authority: Some(7),
            admission: None
        }));
        assert_eq!(view.policy.model.as_deref(), Some("new"));
    }
    #[test]
    fn missing_null_and_an_effort_change_have_distinct_authority() {
        let mut policy = SessionPolicy::from_info(
            &json!({"model":"current","provider":"actual","reasoning_effort":"ultra","reasoning_effort_wire":"max","yolo":false}),
        );
        policy.info(&json!({"reasoning_effort":"high"}));
        assert_eq!(policy.provider.as_deref(), Some("actual"));
        assert!(policy.reasoning_wire.is_none());
        policy.info(&json!({"provider":null,"reasoning_effort":"","service_tier":"","yolo":null}));
        assert!(policy.provider.is_none());
        assert!(policy.yolo.is_none());
        assert_eq!(policy.reasoning.as_deref(), Some(""));
        assert_eq!(policy.service_tier.as_deref(), Some(""));
    }
    #[test]
    fn model_confirmation_uses_draft_identity_and_relevant_field_revisions() {
        let mut composer = Composer::new();
        let scope = owner();
        composer.choose_reasoning(scope.clone(), Some("high".into()));
        let sent = composer.draft(&scope).unwrap().policy_stamp();
        composer.choose_reasoning(scope.clone(), Some("low".into()));
        composer.choose_reasoning(scope.clone(), Some("high".into()));
        let now = composer.draft(&scope).unwrap().policy_stamp();
        assert!(!matches_model_choice(Some(&sent), Some(&now), true));
        assert!(matches_model_choice(Some(&sent), Some(&now), false));
        assert!(!matches_model_choice(None, Some(&now), false));
        let mut foreign = now.clone();
        foreign.draft_id = uuid::Uuid::new_v4().to_string();
        assert!(!matches_model_choice(Some(&foreign), Some(&now), false));
    }
    #[test]
    fn reset_receipt_does_not_erase_a_resolved_profile_level_or_wire() {
        let mut policy = SessionPolicy::from_info(
            &json!({"reasoning_effort":"medium","reasoning_effort_wire":"medium"}),
        );
        policy.receipt(
            &json!({"operation":"reasoning","effort":null}),
            &json!({"effort":null}),
            None,
        );
        assert_eq!(policy.reasoning.as_deref(), Some("medium"));
        assert_eq!(policy.reasoning_wire.as_deref(), Some("medium"));
    }
}
