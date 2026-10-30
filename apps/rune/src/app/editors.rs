//! Tabs retain actual editor controllers; delayed work routes to its captured input.
use super::*;
use crate::composer::{EditorId, PolicyFields, PolicyStamp};
#[derive(Default)]
pub(super) struct Editors {
    base: Option<EditorId>,
    waker: Option<Waker>,
    active: Option<(DraftScope, Option<Connection>)>,
    blocked: bool,
    bindings: HashMap<DraftScope, EditorId>,
    aliases: HashMap<(EditorId, DraftScope), DraftScope>,
    parked: HashMap<EditorId, Parked>,
}
struct Parked {
    scope: Option<DraftScope>,
    connection: Option<Connection>,
    composer: Composer,
}
impl Viewer {
    pub(super) fn flush_window_editors(&mut self) -> Result<(), String> {
        self.composer.flush_storage()?;
        for editor in self.editors.parked.values_mut() {
            editor.composer.flush_storage()?;
        }
        Ok(())
    }
    pub(super) fn init_editors(&mut self, waker: Waker) {
        self.editors.base = Some(self.composer.editor_id().clone());
        self.editors.waker = Some(waker);
        if self.fixture {
            let scope = self.composer_scope();
            self.editors
                .bindings
                .insert(scope.clone(), self.composer.editor_id().clone());
            self.editors.active = Some((scope, self.connection.clone()));
        }
    }
    pub fn editor_ready(&self) -> bool {
        (self.fixture || self.connection.is_some())
            && !self.editors.blocked
            && self
                .editors
                .active
                .as_ref()
                .is_none_or(|(owner, _)| *owner == self.composer_scope())
    }
    pub(super) fn ensure_editor(&mut self) -> bool {
        // A new peer may resolve a saved source after its first frame. Do not
        // bind the editor to a source-less placeholder before that reply.
        if !self.fixture && self.connection.is_none() {
            return false;
        }
        if self.editors.base.is_none() {
            return true;
        }
        let scope = self.composer_scope();
        if self
            .editors
            .active
            .as_ref()
            .is_some_and(|(owner, _)| owner == &scope)
        {
            self.editors.blocked = false;
            return true;
        }
        let mut editors = std::mem::take(&mut self.editors);
        let result = editors.activate(self, &scope);
        editors.blocked = result.is_err();
        self.editors = editors;
        if let Err(error) = result {
            self.client_error = Some(error);
            false
        } else {
            true
        }
    }
    pub fn editor_mut(&mut self, id: &EditorId, owner: &DraftScope) -> Option<&mut Composer> {
        let actual = self
            .editors
            .aliases
            .get(&(id.clone(), owner.clone()))
            .unwrap_or(owner);
        let bound = self.editors.bindings.get(actual) == Some(id);
        if self.composer.editor_id() == id {
            if bound || self.composer.draft(actual).is_some() {
                return Some(&mut self.composer);
            }
        } else if let Some(record) = self.editors.parked.get_mut(id) {
            if record.scope.as_ref() == Some(actual) || record.composer.draft(actual).is_some() {
                return Some(&mut record.composer);
            }
        }
        None
    }
    /// Rune may deliver an input event from the preceding frame after navigation.
    pub fn edit_editor_text(&mut self, id: &EditorId, owner: &DraftScope) -> &mut TextEdit {
        let actual = self
            .editors
            .aliases
            .get(&(id.clone(), owner.clone()))
            .cloned()
            .unwrap_or_else(|| owner.clone());
        if actual.connection != self.connection_epoch
            || self
                .retired_profiles
                .contains(&(actual.connection, actual.profile.clone()))
        {
            return &mut self.retired_editor_input;
        }
        if self.composer.editor_id() == id {
            if actual == self.composer_scope() || self.composer.draft(&actual).is_some() {
                return &mut self.composer.draft_mut(actual).text;
            }
        } else if let Some(record) = self.editors.parked.get_mut(id) {
            if record.scope.as_ref() == Some(&actual) || record.composer.draft(&actual).is_some() {
                return &mut record.composer.draft_mut(actual).text;
            }
        }
        &mut self.retired_editor_input
    }
    pub fn finish_editor_attachment(
        &mut self,
        id: EditorId,
        owner: DraftScope,
        result: Result<Option<PathBuf>, String>,
    ) {
        if owner.connection != self.connection_epoch
            || self
                .retired_profiles
                .contains(&(owner.connection, owner.profile.clone()))
        {
            return;
        }
        let actual = self
            .editors
            .aliases
            .get(&(id.clone(), owner.clone()))
            .cloned()
            .unwrap_or(owner);
        let Some(composer) = self.editor_mut(&id, &actual) else {
            return;
        };
        match result {
            Ok(Some(path)) => {
                let _ = composer.add_attachment(actual, path);
            }
            Ok(None) => {}
            Err(_) => {
                composer.attachment_error =
                    Some("The file chooser is unavailable on this desktop".into())
            }
        }
    }
    pub(super) fn editor_for_policy_mut(
        &mut self,
        owner: &DraftScope,
        stamp: Option<&PolicyStamp>,
    ) -> Option<&mut Composer> {
        let editor = stamp
            .and_then(|stamp| {
                if self
                    .composer
                    .draft(owner)
                    .is_some_and(|d| d.policy_id == stamp.draft_id)
                {
                    Some(self.composer.editor_id().clone())
                } else {
                    self.editors
                        .parked
                        .iter()
                        .find(|(_, p)| {
                            p.composer
                                .draft(owner)
                                .is_some_and(|d| d.policy_id == stamp.draft_id)
                        })
                        .map(|(id, _)| id.clone())
                }
            })
            .or_else(|| {
                stamp
                    .is_none()
                    .then(|| self.editors.bindings.get(owner).cloned())
                    .flatten()
            });
        if let Some(editor) = editor {
            if self.composer.editor_id() == &editor {
                return Some(&mut self.composer);
            }
            return self
                .editors
                .parked
                .get_mut(&editor)
                .map(|p| &mut p.composer);
        }
        if stamp.is_none() && self.editors.active.is_none() {
            Some(&mut self.composer)
        } else {
            None
        }
    }
    pub(super) fn settle_editor_policy(
        &mut self,
        owner: &DraftScope,
        stamp: &PolicyStamp,
        fields: PolicyFields,
    ) -> bool {
        self.editor_for_policy_mut(owner, Some(stamp))
            .is_some_and(|c| c.settle_policy(owner, stamp, fields))
    }
    pub(super) fn reconcile_editor_policy(
        &mut self,
        owner: &DraftScope,
        policy: &crate::conversation::SessionPolicy,
    ) -> bool {
        let mut changed = false;
        if self.composer.draft(owner).is_some() || *owner == self.composer_scope() {
            changed |= self.composer.reconcile_policy(owner, policy)
        }
        for record in self.editors.parked.values_mut() {
            if record.composer.draft(owner).is_some() {
                changed |= record.composer.reconcile_policy(owner, policy)
            }
        }
        changed
    }
    pub(super) fn reconcile_editor_approval(&mut self, owner: &DraftScope, mode: &str) -> bool {
        let mut changed = if *owner == self.composer_scope() {
            self.composer.reconcile_approval(owner, mode)
        } else {
            false
        };
        changed |= self
            .composer
            .reconcile_profile_approval(owner.connection, &owner.profile, mode);
        for record in self.editors.parked.values_mut() {
            changed |=
                record
                    .composer
                    .reconcile_profile_approval(owner.connection, &owner.profile, mode)
        }
        changed
    }
    pub(super) fn rehome_editors(&mut self, origin: &DraftScope, destination: DraftScope) {
        let mut source = None;
        if self.composer.draft(origin).is_some() {
            self.composer.rehome(origin, destination.clone());
            source = Some(self.composer.editor_id().clone());
        }
        for (id, record) in &mut self.editors.parked {
            if record.composer.draft(origin).is_some() {
                record.composer.rehome(origin, destination.clone());
                source = Some(id.clone());
            }
            if record.scope.as_ref() == Some(origin) {
                record.scope = Some(destination.clone());
            }
        }
        if let Some(id) = self.editors.bindings.remove(origin).or(source) {
            self.editors
                .aliases
                .insert((id.clone(), origin.clone()), destination.clone());
            self.editors
                .bindings
                .entry(destination.clone())
                .or_insert(id.clone());
            if !self.fixture && self.draft_storage_available {
                let connection = if self.composer.editor_id() == &id {
                    self.editors.active.as_ref().and_then(|(_, c)| c.clone())
                } else {
                    self.editors
                        .parked
                        .get(&id)
                        .and_then(|p| p.connection.clone())
                };
                if let (Some(base), Some(connection)) = (&self.editors.base, connection) {
                    if let Err(error) =
                        Composer::rehome_saved_slot(base, &connection, origin, &destination, &id)
                    {
                        self.client_error = Some(error);
                    }
                }
            }
        }
        if let Some((scope, _)) = &mut self.editors.active {
            if scope == origin {
                *scope = destination;
            }
        }
    }
    pub(super) fn poll_editors(&mut self, now: f64, tx: &mut Tx) -> bool {
        let mut changed = false;
        for record in self.editors.parked.values_mut() {
            if let Some(scope) = &record.scope {
                changed |= record.composer.poll(scope);
            }
            if !self.fixture {
                match record.composer.poll_storage(now) {
                    Ok(Some(delay)) => tx.wake_after(delay),
                    Err(error) => {
                        self.client_error = Some(error);
                        tx.wake_after(std::time::Duration::from_secs(1));
                    }
                    _ => {}
                }
            }
        }
        changed
    }
    pub(super) fn rename_editor_profile(&mut self, from: &str, to: &str) {
        if !self.fixture && self.draft_storage_available {
            if let Some(connection) = &self.connection {
                if let Err(error) = Composer::rename_saved_profile(connection, from, to) {
                    self.client_error = Some(error);
                }
            }
        }
        self.composer
            .rename_profile(self.connection_epoch, from, to);
        for record in self.editors.parked.values_mut() {
            record
                .composer
                .rename_profile(self.connection_epoch, from, to);
            if let Some(scope) = &mut record.scope {
                if scope.connection == self.connection_epoch && scope.profile == from {
                    scope.profile = to.into();
                }
            }
        }
        let bindings = std::mem::take(&mut self.editors.bindings);
        for (mut key, id) in bindings {
            if key.connection == self.connection_epoch && key.profile == from {
                key.profile = to.into();
            }
            self.editors.bindings.insert(key, id);
        }
        if let Some((scope, _)) = &mut self.editors.active {
            if scope.connection == self.connection_epoch && scope.profile == from {
                scope.profile = to.into();
            }
        }
    }
    pub(super) fn delete_editor_profile(&mut self, name: &str) {
        if !self.fixture && self.draft_storage_available {
            if let Some(connection) = &self.connection {
                if let Err(error) = Composer::delete_saved_profile(connection, name) {
                    self.client_error = Some(error);
                }
            }
        }
        self.composer.delete_profile(self.connection_epoch, name);
        for record in self.editors.parked.values_mut() {
            record.composer.delete_profile(self.connection_epoch, name);
        }
        self.editors
            .bindings
            .retain(|scope, _| scope.connection != self.connection_epoch || scope.profile != name);
        self.editors.aliases.retain(|(_, scope), _| {
            scope.connection != self.connection_epoch || scope.profile != name
        });
    }
}
impl Editors {
    fn activate(&mut self, s: &mut Viewer, scope: &DraftScope) -> Result<(), String> {
        let base = self
            .base
            .get_or_insert_with(|| s.composer.editor_id().clone())
            .clone();
        let id = if let Some(id) = self.bindings.get(scope) {
            id.clone()
        } else if !s.fixture && s.draft_storage_available {
            if let Some(connection) = &s.connection {
                Composer::editor_for_slot(&base, connection, scope)?
            } else {
                return Ok(());
            }
        } else {
            EditorId::new()
        };
        if s.composer.editor_id() == &id {
            self.active = Some((scope.clone(), s.connection.clone()));
            return Ok(());
        }
        let mut next = self
            .parked
            .remove(&id)
            .map(|p| p.composer)
            .unwrap_or_else(|| {
                let mut composer = Composer::with_editor(id.clone());
                composer.inherit_catalog_from(&s.composer);
                composer
            });
        if let Some(waker) = &self.waker {
            next.init_network(waker.clone());
        }
        let prepare = (|| {
            if !s.fixture && s.draft_storage_available {
                if let Some(connection) = &s.connection {
                    if s.composer.editor_id() == &base {
                        s.composer.bind_storage(connection, scope.connection)?;
                    } else if let Some(legacy) = self.parked.get_mut(&base) {
                        legacy.composer.bind_storage(connection, scope.connection)?;
                    }
                    next.bind_storage(connection, scope.connection)?;
                }
            }
            if s.composer.editor_id() == &base {
                s.composer.transfer_draft_to(&mut next, scope)?;
            } else if let Some(legacy) = self.parked.get_mut(&base) {
                legacy.composer.transfer_draft_to(&mut next, scope)?;
            }
            if !s.fixture && s.draft_storage_available {
                next.seed_stored_draft(scope)?;
            }
            Ok::<(), String>(())
        })();
        if let Err(error) = prepare {
            self.parked.insert(
                id,
                Parked {
                    scope: Some(scope.clone()),
                    connection: s.connection.clone(),
                    composer: next,
                },
            );
            return Err(error);
        }
        let old = std::mem::replace(&mut s.composer, next);
        let previous = self.active.take();
        self.parked.insert(
            old.editor_id().clone(),
            Parked {
                scope: previous.as_ref().map(|(scope, _)| scope.clone()),
                connection: previous.and_then(|(_, connection)| connection),
                composer: old,
            },
        );
        self.bindings.insert(scope.clone(), id);
        self.active = Some((scope.clone(), s.connection.clone()));
        Ok(())
    }
}
#[cfg(test)]
#[path = "editors/tests.rs"]
mod tests;
