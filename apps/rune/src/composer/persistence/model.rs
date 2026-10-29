use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub profile: String,
    pub session: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct SavedDraft {
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
    pub policy_revisions: PolicyRevisions,
    pub profile: String,
    pub session: Option<String>,
    pub text: String,
    pub model: Option<(String, String)>,
    pub reasoning: Option<String>,
    pub service_tier: Option<String>,
    pub approval_mode: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub cwd_explicit: bool,
    pub model_changed: bool,
    pub reasoning_changed: bool,
    pub service_tier_changed: bool,
    pub approval_changed: bool,
    pub attachments: Vec<PathBuf>,
    #[serde(default)]
    pub replay: Option<crate::conversation::ReplayBundle>,
}
impl SavedDraft {
    pub fn key(&self) -> Key {
        Key {
            profile: self.profile.clone(),
            session: self.session.clone(),
        }
    }
    pub fn capture(scope: &DraftScope, draft: &Draft) -> Self {
        Self {
            policy_id: draft.policy_id.clone(),
            policy_revisions: draft.policy_revisions,
            profile: scope.profile.clone(),
            session: scope.session.clone(),
            text: draft.text.text.clone(),
            model: draft
                .model
                .as_ref()
                .map(|m| (m.provider.clone(), m.id.clone())),
            reasoning: draft.reasoning.clone(),
            service_tier: draft.service_tier.clone(),
            approval_mode: draft.approval_mode.clone(),
            cwd: draft.cwd.clone(),
            cwd_explicit: draft.cwd_explicit,
            model_changed: draft.model_changed,
            reasoning_changed: draft.reasoning_changed,
            service_tier_changed: draft.service_tier_changed,
            approval_changed: draft.approval_changed,
            attachments: draft.attachments.iter().map(|a| a.path.clone()).collect(),
            replay: draft.replay.clone(),
        }
    }
    pub fn restore(self) -> Draft {
        Draft {
            policy_id: if self.policy_id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                self.policy_id
            },
            policy_revisions: self.policy_revisions,
            session_policy: Default::default(),
            text: TextEdit::new(self.text),
            model: self.model.map(|(provider, id)| ModelKey { provider, id }),
            reasoning: self.reasoning,
            service_tier: self.service_tier,
            approval_mode: self.approval_mode,
            cwd: self.cwd,
            cwd_explicit: self.cwd_explicit,
            model_changed: self.model_changed,
            reasoning_changed: self.reasoning_changed,
            service_tier_changed: self.service_tier_changed,
            approval_changed: self.approval_changed,
            replay: self.replay,
            attachments: self
                .attachments
                .into_iter()
                .map(|path| DraftAttachment {
                    name: path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("attachment")
                        .into(),
                    path,
                    preview: None,
                })
                .collect(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Entry {
    pub editor: EditorId,
    pub profile: String,
    pub session: Option<String>,
    pub revision: u64,
    pub draft: Option<SavedDraft>,
}
impl Entry {
    pub fn key(&self) -> Key {
        Key {
            profile: self.profile.clone(),
            session: self.session.clone(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Favorite {
    pub provider: String,
    pub model: String,
    pub revision: u64,
    pub enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Slot {
    pub window: EditorId,
    pub profile: String,
    pub session: Option<String>,
    pub editor: EditorId,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(super) struct State {
    pub version: u32,
    pub revision: u64,
    pub drafts: Vec<SavedDraft>,
    pub favorites: Vec<(String, String)>,
    pub entries: Vec<Entry>,
    pub favorite_entries: Vec<Favorite>,
    pub owner_slots: Vec<Slot>,
    #[serde(flatten)]
    pub other: std::collections::BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Default, PartialEq)]
pub(super) struct Snapshot {
    pub drafts: HashMap<Key, SavedDraft>,
    pub favorites: HashSet<(String, String)>,
}
#[derive(Default, Clone)]
pub(super) struct Observed {
    pub drafts: HashMap<Key, u64>,
    pub favorites: HashMap<(String, String), u64>,
}
impl State {
    pub fn for_editor(&self, editor: &EditorId) -> (Snapshot, Observed) {
        let mut snapshot = Snapshot::default();
        let mut seen = Observed::default();
        for entry in self.entries.iter().filter(|e| &e.editor == editor) {
            seen.drafts.insert(entry.key(), entry.revision);
            if let Some(draft) = &entry.draft {
                snapshot.drafts.insert(entry.key(), draft.clone());
            }
        }
        for f in &self.favorite_entries {
            let key = (f.provider.clone(), f.model.clone());
            seen.favorites.insert(key.clone(), f.revision);
            if f.enabled {
                snapshot.favorites.insert(key);
            }
        }
        (snapshot, seen)
    }
}
