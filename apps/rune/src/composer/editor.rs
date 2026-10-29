//! A mounted input has its own editor even when another input shows the same session.
use super::{Composer, Draft, DraftScope, TextEdit};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EditorId(String);
impl EditorId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        uuid::Uuid::parse_str(value)
            .map(|id| Self(id.to_string()))
            .map_err(|_| "Invalid editor identity".into())
    }
}
impl Default for EditorId {
    fn default() -> Self {
        Self::new()
    }
}
impl TryFrom<String> for EditorId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<EditorId> for String {
    fn from(value: EditorId) -> Self {
        value.0
    }
}
impl Composer {
    pub fn with_editor(editor: EditorId) -> Self {
        let mut composer = Self::new();
        composer.editor = editor;
        composer
    }
    pub fn inherit_catalog_from(&mut self, source: &Composer) {
        self.catalog = source.catalog.clone();
        self.catalog_owner = source.catalog_owner.clone();
    }
    pub fn editor_id(&self) -> &EditorId {
        &self.editor
    }
    pub fn fork_editor(&self, scope: &DraftScope) -> Self {
        let mut editor = Self::new();
        editor.catalog = self.catalog.clone();
        editor.catalog_owner = self.catalog_owner.clone();
        editor.favorites = self.favorites.clone();
        if let Some(source) = self.draft(scope) {
            let draft = Draft {
                text: TextEdit::new(source.text.text.clone()),
                model: source.model.clone(),
                reasoning: source.reasoning.clone(),
                service_tier: source.service_tier.clone(),
                approval_mode: source.approval_mode.clone(),
                cwd: source.cwd.clone(),
                cwd_explicit: source.cwd_explicit,
                session_policy: source.session_policy.clone(),
                model_changed: source.model_changed,
                reasoning_changed: source.reasoning_changed,
                service_tier_changed: source.service_tier_changed,
                approval_changed: source.approval_changed,
                ..Draft::default()
            };
            editor.drafts.insert(scope.clone(), draft);
            editor.storage_revision = 1;
        }
        editor
    }
}
