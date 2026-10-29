//! User edits and authority updates have separate identities. A late receipt
//! must not acknowledge a choice made after its captured input.
use super::{Composer, Draft, DraftScope, ModelKey};
use crate::conversation::SessionPolicy;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicyRevisions {
    pub model: u64,
    pub reasoning: u64,
    pub tier: u64,
    pub approval: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyStamp {
    pub draft_id: String,
    pub revisions: PolicyRevisions,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PolicyFields {
    pub model: bool,
    pub reasoning: bool,
    pub tier: bool,
    pub approval: bool,
}
impl Draft {
    pub fn policy_stamp(&self) -> PolicyStamp {
        PolicyStamp {
            draft_id: self.policy_id.clone(),
            revisions: self.policy_revisions,
        }
    }
}
impl Composer {
    pub fn choose_reasoning(&mut self, scope: DraftScope, value: Option<String>) {
        let draft = self.draft_mut(scope);
        draft.reasoning = value;
        draft.reasoning_changed = true;
        draft.policy_revisions.reasoning += 1;
    }
    pub fn choose_service_tier(&mut self, scope: DraftScope, value: Option<String>) {
        let draft = self.draft_mut(scope);
        draft.service_tier = value;
        draft.service_tier_changed = true;
        draft.policy_revisions.tier += 1;
    }
    pub fn choose_approval(&mut self, scope: DraftScope, value: Option<String>) {
        let draft = self.draft_mut(scope);
        draft.approval_mode = value;
        draft.approval_changed = true;
        draft.policy_revisions.approval += 1;
    }
    pub fn settle_policy(
        &mut self,
        scope: &DraftScope,
        stamp: &PolicyStamp,
        fields: PolicyFields,
    ) -> bool {
        let Some(draft) = self.drafts.get_mut(scope) else {
            return false;
        };
        if draft.policy_id != stamp.draft_id {
            return false;
        }
        let mut changed = false;
        for (ack, dirty, now, sent) in [
            (
                fields.model,
                &mut draft.model_changed,
                draft.policy_revisions.model,
                stamp.revisions.model,
            ),
            (
                fields.reasoning,
                &mut draft.reasoning_changed,
                draft.policy_revisions.reasoning,
                stamp.revisions.reasoning,
            ),
            (
                fields.tier,
                &mut draft.service_tier_changed,
                draft.policy_revisions.tier,
                stamp.revisions.tier,
            ),
            (
                fields.approval,
                &mut draft.approval_changed,
                draft.policy_revisions.approval,
                stamp.revisions.approval,
            ),
        ] {
            if ack && now == sent && *dirty {
                *dirty = false;
                changed = true;
            }
        }
        if changed {
            self.storage_revision = self.storage_revision.wrapping_add(1);
        }
        changed
    }
    pub fn reconcile_policy(&mut self, scope: &DraftScope, policy: &SessionPolicy) -> bool {
        let draft = self.drafts.entry(scope.clone()).or_default();
        let mut changed = draft.session_policy != *policy;
        draft.session_policy = policy.clone();
        if !draft.model_changed {
            let model = match (policy.next_provider(), policy.next_model()) {
                (Some(provider), Some(id)) => Some(ModelKey {
                    provider: provider.into(),
                    id: id.into(),
                }),
                _ => None,
            };
            changed |= draft.model != model;
            draft.model = model;
        }
        if !draft.reasoning_changed {
            let value = policy.next_reasoning().map(str::to_owned);
            changed |= draft.reasoning != value;
            draft.reasoning = value;
        }
        if !draft.service_tier_changed {
            let value = policy.next_service_tier().map(str::to_owned).or_else(|| {
                policy
                    .fast
                    .map(|fast| if fast { "priority" } else { "normal" }.into())
            });
            changed |= draft.service_tier != value;
            draft.service_tier = value;
        }
        if !draft.approval_changed {
            let value = policy.approval_mode.clone();
            changed |= draft.approval_mode != value;
            draft.approval_mode = value;
        }
        if changed {
            self.storage_revision = self.storage_revision.wrapping_add(1);
        }
        changed
    }
    pub fn reconcile_approval(&mut self, scope: &DraftScope, mode: &str) -> bool {
        let draft = self.drafts.entry(scope.clone()).or_default();
        let mut changed = draft.session_policy.approval_mode.as_deref() != Some(mode);
        draft.session_policy.approval_mode = Some(mode.into());
        if !draft.approval_changed {
            let value = mode;
            changed |= draft.approval_mode.as_deref() != Some(value);
            draft.approval_mode = Some(value.into());
        }
        if changed {
            self.storage_revision = self.storage_revision.wrapping_add(1);
        }
        changed
    }
    pub fn reconcile_profile_approval(
        &mut self,
        connection: u64,
        profile: &str,
        mode: &str,
    ) -> bool {
        let scopes: Vec<_> = self
            .drafts
            .keys()
            .filter(|s| s.connection == connection && s.profile == profile)
            .cloned()
            .collect();
        let mut changed = false;
        for scope in scopes {
            changed |= self.reconcile_approval(&scope, mode);
        }
        changed
    }
    pub fn effective_model_key<'a>(&'a self, scope: &DraftScope) -> Option<&'a ModelKey> {
        let draft = self.draft(scope)?;
        if draft.model_changed
            || scope.session.is_none()
            || draft.session_policy.next_provider().is_some()
        {
            draft.model.as_ref()
        } else {
            None
        }
    }
    pub fn model_name<'a>(&'a self, scope: &DraftScope) -> Option<&'a str> {
        let draft = self.draft(scope)?;
        if !draft.model_changed && scope.session.is_some() {
            return draft.session_policy.next_model();
        }
        draft.model.as_ref().map(|model| model.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> DraftScope {
        DraftScope {
            connection: 1,
            profile: "default".into(),
            session: Some("s".into()),
        }
    }
    #[test]
    fn late_ack_cannot_clear_a_newer_choice_even_when_its_value_returns_to_a() {
        let mut composer = Composer::new();
        let scope = owner();
        composer.choose_reasoning(scope.clone(), Some("high".into()));
        let sent = composer.draft(&scope).unwrap().policy_stamp();
        composer.choose_reasoning(scope.clone(), Some("low".into()));
        composer.choose_reasoning(scope.clone(), Some("high".into()));
        assert!(!composer.settle_policy(
            &scope,
            &sent,
            PolicyFields {
                reasoning: true,
                ..Default::default()
            }
        ));
        assert!(composer.draft(&scope).unwrap().reasoning_changed);
        let current = composer.draft(&scope).unwrap().policy_stamp();
        assert!(composer.settle_policy(
            &scope,
            &current,
            PolicyFields {
                reasoning: true,
                ..Default::default()
            }
        ));
    }
    #[test]
    fn destination_draft_and_independent_fields_keep_their_own_edits() {
        let mut composer = Composer::new();
        let origin = DraftScope {
            session: None,
            ..owner()
        };
        let target = owner();
        composer.choose_approval(origin.clone(), Some("smart".into()));
        let sent = composer.draft(&origin).unwrap().policy_stamp();
        composer.choose_approval(target.clone(), Some("smart".into()));
        composer.rehome(&origin, target.clone());
        assert!(!composer.settle_policy(
            &target,
            &sent,
            PolicyFields {
                approval: true,
                ..Default::default()
            }
        ));
        let policy = SessionPolicy {
            model: Some("unlisted-model".into()),
            provider: None,
            reasoning: Some("medium".into()),
            yolo: Some(false),
            ..Default::default()
        };
        composer.reconcile_policy(&target, &policy);
        let draft = composer.draft(&target).unwrap();
        assert!(draft.model.is_none());
        assert!(composer.effective_model_key(&target).is_none());
        assert_eq!(composer.model_name(&target), Some("unlisted-model"));
        assert_eq!(draft.approval_mode.as_deref(), Some("smart"));
        assert_eq!(draft.reasoning.as_deref(), Some("medium"));
    }
    #[test]
    fn profile_approval_policy_and_conversation_bypass_are_independent_facts() {
        let mut composer = Composer::new();
        let scope = owner();
        let policy = SessionPolicy {
            approval_mode: Some("manual".into()),
            yolo: Some(true),
            ..Default::default()
        };
        composer.reconcile_policy(&scope, &policy);
        assert_eq!(
            composer.draft(&scope).unwrap().approval_mode.as_deref(),
            Some("manual")
        );
        assert_eq!(
            composer.draft(&scope).unwrap().session_policy.yolo,
            Some(true)
        );
        composer.reconcile_approval(&scope, "smart");
        assert_eq!(
            composer.draft(&scope).unwrap().approval_mode.as_deref(),
            Some("smart")
        );
        assert_eq!(
            composer.draft(&scope).unwrap().session_policy.yolo,
            Some(true)
        );
    }
}
