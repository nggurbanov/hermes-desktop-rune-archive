//! Client-owned drafts and model-picker state. Catalog reads have their own
//! worker so opening a picker cannot cancel a conversation-history request.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    Arc,
    mpsc::{self, Receiver, Sender},
};

use rune::{Image, ImageCache, TextEdit, Waker};

use crate::gateway::{CatalogModel, Connection, Gateway, ModelCatalog, ModelProvider};

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DraftOrigin(String);
impl DraftOrigin {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}
impl Default for DraftOrigin {
    fn default() -> Self {
        Self::new()
    }
}
impl TryFrom<String> for DraftOrigin {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        uuid::Uuid::parse_str(&value)
            .map(|id| Self(id.to_string()))
            .map_err(|_| "Invalid draft origin".into())
    }
}
impl From<DraftOrigin> for String {
    fn from(value: DraftOrigin) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DraftScope {
    pub draft_origin: Option<DraftOrigin>,
    pub connection: u64,
    pub profile: String,
    pub profile_intent: crate::source::ProfileIntent,
    pub session: Option<String>,
}

impl DraftScope {
    fn catalog_owner(&self) -> (u64, String, crate::source::ProfileIntent) {
        (self.connection, self.profile.clone(), self.profile_intent)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ModelKey {
    pub provider: String,
    pub id: String,
}

#[derive(Clone, Debug)]
pub struct DraftAttachment {
    pub path: PathBuf,
    pub name: String,
    pub preview: Option<Image>,
}

#[derive(Clone)]
pub struct Draft {
    pub policy_id: String,
    pub policy_revisions: PolicyRevisions,
    pub session_policy: crate::conversation::SessionPolicy,
    pub text: TextEdit,
    pub model: Option<ModelKey>,
    pub model_changed: bool,
    pub reasoning_changed: bool,
    pub service_tier_changed: bool,
    pub approval_changed: bool,
    pub reasoning: Option<String>,
    pub service_tier: Option<String>,
    pub approval_mode: Option<String>,
    pub cwd: Option<String>,
    pub cwd_explicit: bool,
    pub attachments: Vec<DraftAttachment>,
    pub replay: Option<crate::conversation::ReplayBundle>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            policy_id: uuid::Uuid::new_v4().to_string(),
            policy_revisions: PolicyRevisions::default(),
            session_policy: crate::conversation::SessionPolicy::default(),
            text: TextEdit::new(""),
            model: None,
            model_changed: false,
            reasoning_changed: false,
            service_tier_changed: false,
            approval_changed: false,
            reasoning: None,
            service_tier: None,
            approval_mode: None,
            cwd: None,
            cwd_explicit: false,
            attachments: vec![],
            replay: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popup {
    Models,
    Settings,
    VoiceEngine,
}

mod editor;
pub use editor::EditorId;
mod persistence;
mod policy;
pub use policy::{PolicyFields, PolicyRevisions, PolicyStamp};
pub struct Composer {
    editor: EditorId,
    persisted: Option<persistence::Persistence>,
    storage_revision: u64,
    pub popup: Option<Popup>,
    pub popup_scope: Option<DraftScope>,
    pub search: TextEdit,
    pub provider: Option<String>,
    pub favorites_only: bool,
    pub highlighted: usize,
    pub catalog: Option<ModelCatalog>,
    pub catalog_owner: Option<(u64, String, crate::source::ProfileIntent)>,
    pub catalog_loading: bool,
    pub catalog_error: Option<String>,
    catalog_error_owner: Option<(u64, String, crate::source::ProfileIntent)>,
    pub voice_expanded: bool,
    pub attachment_error: Option<String>,
    drafts: HashMap<DraftScope, Draft>,
    favorites: HashSet<(u64, String, String)>,
    request_id: u64,
    pending: Option<Pending>,
    network: Option<CatalogNetwork>,
    images: Option<ImageCache>,
}

struct CatalogNetwork {
    send: Sender<CatalogRequest>,
    receive: Receiver<CatalogReply>,
}

struct Pending {
    id: u64,
    scope: DraftScope,
}

struct CatalogRequest {
    id: u64,
    connection: Connection,
    scope: DraftScope,
}

struct CatalogReply {
    id: u64,
    scope: DraftScope,
    result: Result<ModelCatalog, String>,
}

impl Default for Composer {
    fn default() -> Self {
        Self {
            editor: EditorId::new(),
            persisted: None,
            storage_revision: 0,
            popup: None,
            popup_scope: None,
            search: TextEdit::new(""),
            provider: None,
            favorites_only: false,
            highlighted: 0,
            catalog: None,
            catalog_owner: None,
            catalog_loading: false,
            catalog_error: None,
            catalog_error_owner: None,
            voice_expanded: false,
            attachment_error: None,
            drafts: HashMap::new(),
            favorites: HashSet::new(),
            request_id: 0,
            pending: None,
            network: None,
            images: None,
        }
    }
}

impl Composer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn draft(&self, scope: &DraftScope) -> Option<&Draft> {
        self.drafts.get(scope)
    }

    pub fn draft_mut(&mut self, scope: DraftScope) -> &mut Draft {
        self.storage_revision = self.storage_revision.wrapping_add(1);
        self.drafts.entry(scope).or_default()
    }

    pub fn catalog_for(&self, scope: &DraftScope) -> Option<&ModelCatalog> {
        (self.catalog_owner.as_ref() == Some(&scope.catalog_owner()))
            .then_some(self.catalog.as_ref())
            .flatten()
    }

    pub fn catalog_error_for(&self, scope: &DraftScope) -> Option<&str> {
        (self.catalog_error_owner.as_ref() == Some(&scope.catalog_owner()))
            .then_some(self.catalog_error.as_deref())
            .flatten()
    }
    pub fn set_catalog_error(&mut self, scope: &DraftScope, error: String) {
        self.catalog_error_owner = Some(scope.catalog_owner());
        self.catalog_error = Some(error);
    }

    pub fn catalog_loading_for(&self, scope: &DraftScope) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|request| request.scope.catalog_owner() == scope.catalog_owner())
    }

    pub fn toggle_popup(&mut self, scope: DraftScope, popup: Popup) {
        if self.popup == Some(popup) && self.popup_scope.as_ref() == Some(&scope) {
            self.close();
            return;
        }
        let changed_scope = self.popup_scope.as_ref() != Some(&scope);
        self.popup = Some(popup);
        self.popup_scope = Some(scope.clone());
        self.voice_expanded = false;
        if popup == Popup::Models {
            self.search.replace_text("");
            self.search.focused = true;
            self.highlighted = 0;
            if changed_scope {
                self.provider = None;
                self.favorites_only = false;
                self.attachment_error = None;
            }
            self.initialize_default_model(&scope);
        }
    }

    pub fn close(&mut self) {
        self.search.focused = false;
        self.popup = None;
        self.popup_scope = None;
        self.voice_expanded = false;
    }

    pub fn dismiss_if_scope_changed(&mut self, scope: &DraftScope) -> bool {
        if self
            .popup_scope
            .as_ref()
            .is_some_and(|owner| owner != scope)
        {
            self.close();
            true
        } else {
            false
        }
    }

    pub fn rehome(&mut self, origin: &DraftScope, destination: DraftScope) {
        if origin == &destination {
            return;
        }
        self.storage_revision = self.storage_revision.wrapping_add(1);
        if let Some(draft) = self.drafts.remove(origin) {
            self.drafts.entry(destination).or_insert(draft);
        }
        self.close();
    }
    pub fn is_favorite(&self, connection: u64, provider: &str, id: &str) -> bool {
        self.favorites
            .contains(&(connection, provider.to_owned(), id.to_owned()))
    }

    pub fn toggle_favorite(&mut self, connection: u64, provider: &str, id: &str) {
        self.storage_revision = self.storage_revision.wrapping_add(1);
        let key = (connection, provider.to_owned(), id.to_owned());
        if !self.favorites.remove(&key) {
            self.favorites.insert(key);
        }
    }

    pub fn selected_model(&self, scope: &DraftScope) -> Option<(&ModelProvider, &CatalogModel)> {
        let key = self.effective_model_key(scope)?;
        find_model(self.catalog_for(scope)?, key)
    }

    pub fn select_model(&mut self, scope: DraftScope, key: ModelKey) -> Result<(), String> {
        let (_, model) = self
            .catalog_for(&scope)
            .and_then(|catalog| find_model(catalog, &key))
            .filter(|(_, model)| model.available)
            .ok_or("This model is unavailable in the current agent catalog")?;
        let capabilities = model.capabilities.clone();
        let draft = self.draft_mut(scope);
        draft.model = Some(key);
        draft.model_changed = true;
        draft.policy_revisions.model += 1;
        if let Some(effort) = draft.reasoning.as_deref() {
            let supported = capabilities.as_ref().is_some_and(|caps| {
                caps.reasoning == Some(true)
                    && (matches!(
                        effort,
                        "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
                    ) || (effort == "none" && caps.can_disable_reasoning == Some(true)))
            });
            if !supported {
                draft.reasoning = None;
                draft.reasoning_changed = true;
                draft.policy_revisions.reasoning += 1;
            }
        }
        if let Some(tier) = draft.service_tier.as_deref() {
            let supported = tier == "normal"
                || capabilities.as_ref().is_some_and(|caps| match tier {
                    "priority" => caps.fast == Some(true),
                    "ultrafast" => caps.ultrafast == Some(true),
                    _ => false,
                });
            if !supported {
                draft.service_tier = None;
                draft.service_tier_changed = true;
                draft.policy_revisions.tier += 1;
            }
        }
        self.close();
        Ok(())
    }

    pub fn add_attachment(&mut self, scope: DraftScope, path: PathBuf) -> Result<(), String> {
        let metadata = path
            .is_absolute()
            .then(|| std::fs::metadata(&path).ok())
            .flatten()
            .filter(|metadata| metadata.is_file());
        let Some(metadata) = metadata else {
            return self.attachment_failure();
        };
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            return self.attachment_failure();
        };
        let preview = path
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "svgz"
                )
            })
            .filter(|_| metadata.len() <= 20 * 1024 * 1024)
            .and(self.images.as_ref())
            .map(|cache| cache.file(&path, 96.0, 1.5));
        let attachments = &mut self.draft_mut(scope).attachments;
        if !attachments.iter().any(|attachment| attachment.path == path) {
            attachments.push(DraftAttachment {
                path,
                name,
                preview,
            });
        }
        self.attachment_error = None;
        Ok(())
    }

    fn attachment_failure(&mut self) -> Result<(), String> {
        let error = "Choose an existing file to attach".to_owned();
        self.attachment_error = Some(error.clone());
        Err(error)
    }

    pub fn remove_attachment(&mut self, scope: &DraftScope, index: usize) -> bool {
        let Some(draft) = self.drafts.get_mut(scope) else {
            return false;
        };
        if index >= draft.attachments.len() {
            return false;
        }
        draft.attachments.remove(index);
        self.storage_revision = self.storage_revision.wrapping_add(1);
        true
    }

    pub fn init_network(&mut self, waker: Waker) {
        if self.images.is_none() {
            self.images = Some(ImageCache::new(waker.clone()));
        }
        if self.network.is_some() {
            return;
        }
        let (send, requests) = mpsc::channel::<CatalogRequest>();
        let (replies, receive) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(mut request) = requests.recv() {
                // Every request carries its own route; only the newest pending
                // catalog read matters. This never touches the history queue.
                for incoming in requests.try_iter() {
                    request = incoming;
                }
                let result = Gateway::connect(&request.connection).and_then(|mut gateway| {
                    let provenance = crate::gateway::audio::provenance(
                        &request.connection,
                        &request.scope.profile,
                        request.scope.profile_intent,
                    )?;
                    gateway.model_catalog_for(&provenance)
                });
                if replies
                    .send(CatalogReply {
                        id: request.id,
                        scope: request.scope,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                waker();
            }
        });
        self.network = Some(CatalogNetwork { send, receive });
    }

    pub fn request_catalog(&mut self, connection: Connection, scope: DraftScope) {
        self.request_id += 1;
        self.catalog_loading = true;
        self.catalog_error = None;
        self.pending = Some(Pending {
            id: self.request_id,
            scope: scope.clone(),
        });
        let request = CatalogRequest {
            id: self.request_id,
            connection,
            scope: scope.clone(),
        };
        if !self
            .network
            .as_ref()
            .is_some_and(|network| network.send.send(request).is_ok())
        {
            self.pending = None;
            self.catalog_loading = false;
            self.set_catalog_error(&scope, "The model catalog worker is unavailable".into());
        }
    }

    pub fn poll(&mut self, current_scope: &DraftScope) -> bool {
        let mut replies = Vec::new();
        if let Some(network) = &self.network {
            while let Ok(reply) = network.receive.try_recv() {
                replies.push(reply);
            }
        }
        let mut changed = self.dismiss_if_scope_changed(current_scope);
        for reply in replies {
            changed |= self.apply_catalog_reply(reply, current_scope);
        }
        changed
    }

    pub fn fixture_catalog(
        &mut self,
        scope: DraftScope,
        value: &serde_json::Value,
    ) -> Result<(), String> {
        let catalog = ModelCatalog::from_value(value)?;
        self.catalog = Some(catalog);
        self.catalog_owner = Some(scope.catalog_owner());
        self.catalog_loading = false;
        self.catalog_error = None;
        if self.images.is_none() {
            self.images = Some(ImageCache::new(Arc::new(|| {})));
        }
        self.initialize_profile_default(&scope);
        if let Some(draft) = self.drafts.get_mut(&scope) {
            if let Some(key) = &draft.model {
                draft.session_policy.model = Some(key.id.clone());
                draft.session_policy.provider = Some(key.provider.clone());
            }
        }
        Ok(())
    }

    fn apply_catalog_reply(&mut self, reply: CatalogReply, current_scope: &DraftScope) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        if pending.id != reply.id || pending.scope.catalog_owner() != reply.scope.catalog_owner() {
            return false;
        }
        self.pending = None;
        self.catalog_loading = false;
        if reply.scope.catalog_owner() != current_scope.catalog_owner() {
            return true;
        }
        match reply.result {
            Ok(catalog) => {
                self.catalog = Some(catalog);
                self.catalog_owner = Some(reply.scope.catalog_owner());
                self.catalog_error = None;
                // Catalogs are profile-scoped; draft editing remains session-
                // scoped. A background reply does not choose another draft's model.
                if reply.scope == *current_scope {
                    self.initialize_default_model(current_scope);
                }
            }
            Err(error) => self.set_catalog_error(&reply.scope, error),
        }
        true
    }

    fn initialize_default_model(&mut self, scope: &DraftScope) {
        if scope.session.is_some() {
            return;
        }
        self.initialize_profile_default(scope);
    }

    fn initialize_profile_default(&mut self, scope: &DraftScope) {
        if self.draft(scope).is_some_and(|draft| draft.model.is_some()) {
            return;
        }
        let Some(catalog) = self.catalog_for(scope) else {
            return;
        };
        let (Some(provider), Some(model)) = (&catalog.current_provider, &catalog.current_model)
        else {
            return;
        };
        let key = resolve_provider(catalog, provider).and_then(|provider| {
            provider
                .models
                .iter()
                .find(|row| row.id == *model && row.available)
                .map(|model| ModelKey {
                    provider: provider.id.clone(),
                    id: model.id.clone(),
                })
        });
        if let Some(key) = key {
            self.draft_mut(scope.clone()).model = Some(key);
        }
    }
}

fn resolve_provider<'a>(catalog: &'a ModelCatalog, name: &str) -> Option<&'a ModelProvider> {
    if let Some(provider) = catalog.providers.iter().find(|p| p.id == name) {
        return Some(provider);
    }
    let mut aliases = catalog.providers.iter().filter(|p| p.matches(name));
    let first = aliases.next()?;
    if aliases.next().is_none() {
        Some(first)
    } else {
        None
    }
}

fn find_model<'a>(
    catalog: &'a ModelCatalog,
    key: &ModelKey,
) -> Option<(&'a ModelProvider, &'a CatalogModel)> {
    let provider = resolve_provider(catalog, &key.provider)?;
    let model = provider.models.iter().find(|model| model.id == key.id)?;
    Some((provider, model))
}

#[cfg(test)]
mod tests;

impl Composer {
    pub fn rename_profile(&mut self, connection: u64, from: &str, to: &str) {
        self.storage_revision = self.storage_revision.wrapping_add(1);
        let keys: Vec<_> = self
            .drafts
            .keys()
            .filter(|scope| {
                scope.connection == connection
                    && scope.profile_intent.is_backend()
                    && scope.profile == from
            })
            .cloned()
            .collect();
        for key in keys {
            if let Some(draft) = self.drafts.remove(&key) {
                self.drafts.insert(
                    DraftScope {
                        profile: to.into(),
                        ..key
                    },
                    draft,
                );
            }
        }
        self.close();
    }
    pub fn delete_profile(&mut self, connection: u64, name: &str) {
        self.storage_revision = self.storage_revision.wrapping_add(1);
        self.drafts.retain(|scope, _| {
            scope.connection != connection
                || !scope.profile_intent.is_backend()
                || scope.profile != name
        });
        self.close();
    }
}
