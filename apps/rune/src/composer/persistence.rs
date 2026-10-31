//! Scoped drafts commit ordered per-entry patches under a private cross-process lock.
use super::*;
mod model;
mod store;
use model::{Entry, Favorite, Key, Observed, SavedDraft, Slot, Snapshot, State};
pub(super) struct Persistence {
    epoch: u64,
    path: PathBuf,
    migration_owner: EditorId,
    previous: Snapshot,
    observed: Observed,
    pending: Option<(Snapshot, f64)>,
    scanned_revision: Option<u64>,
}
impl Composer {
    pub fn primary() -> Result<Self, String> {
        Self::primary_at(&crate::client::state_dir()?.join("primary-editor.json"))
    }
    pub(crate) fn primary_at(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self::with_editor(store::primary(path)?))
    }
    pub fn editor_for_slot(
        window: &EditorId,
        connection: &Connection,
        scope: &DraftScope,
    ) -> Result<EditorId, String> {
        let path = crate::client::state_dir()?.join("drafts").join(format!(
            "{}.json",
            crate::client::connection_storage_key(connection)?
        ));
        store::slot(
            &path,
            window,
            &Key {
                profile: scope.profile.clone(),
                profile_intent: scope.profile_intent,
                session: scope.session.clone(),
                draft_origin: scope.draft_origin.clone(),
            },
            &legacy_owner()?,
        )
    }
    pub fn rehome_saved_slot(
        window: &EditorId,
        connection: &Connection,
        origin: &DraftScope,
        destination: &DraftScope,
        editor: &EditorId,
    ) -> Result<(), String> {
        let path = crate::client::state_dir()?.join("drafts").join(format!(
            "{}.json",
            crate::client::connection_storage_key(connection)?
        ));
        store::rehome_slot(
            &path,
            window,
            &Key {
                profile: origin.profile.clone(),
                profile_intent: origin.profile_intent,
                session: origin.session.clone(),
                draft_origin: origin.draft_origin.clone(),
            },
            &Key {
                profile: destination.profile.clone(),
                profile_intent: destination.profile_intent,
                session: destination.session.clone(),
                draft_origin: destination.draft_origin.clone(),
            },
            editor,
        )
    }
    pub fn rename_saved_profile(
        connection: &Connection,
        from: &str,
        to: &str,
    ) -> Result<(), String> {
        let path = crate::client::state_dir()?.join("drafts").join(format!(
            "{}.json",
            crate::client::connection_storage_key(connection)?
        ));
        store::rename_profile(&path, from, to)
    }
    pub fn delete_saved_profile(connection: &Connection, profile: &str) -> Result<(), String> {
        let path = crate::client::state_dir()?.join("drafts").join(format!(
            "{}.json",
            crate::client::connection_storage_key(connection)?
        ));
        store::delete_profile(&path, profile)
    }
    /// Copy is durable before source retirement; a failed save restores the actual local input.
    pub fn transfer_draft_to(
        &mut self,
        target: &mut Composer,
        scope: &DraftScope,
    ) -> Result<bool, String> {
        if self.editor == target.editor
            || target.draft(scope).is_some()
            || self.draft(scope).is_none()
        {
            return Ok(false);
        }
        match (&self.persisted, &target.persisted) {
            (Some(a), Some(b)) if a.path == b.path && a.epoch == b.epoch => {}
            (None, None) => {}
            _ => return Err("The editor transfer does not own the same saved source".into()),
        }
        let draft = self.drafts.remove(scope).unwrap();
        target.drafts.insert(scope.clone(), draft);
        target.storage_revision = target.storage_revision.wrapping_add(1);
        if let Err(error) = target.flush_storage() {
            self.drafts
                .insert(scope.clone(), target.drafts.remove(scope).unwrap());
            return Err(error);
        }
        self.storage_revision = self.storage_revision.wrapping_add(1);
        Ok(true)
    }
    pub fn bind_storage(&mut self, connection: &Connection, epoch: u64) -> Result<(), String> {
        let path = crate::client::state_dir()?.join("drafts").join(format!(
            "{}.json",
            crate::client::connection_storage_key(connection)?
        ));
        self.bind_storage_path_owned(path, epoch, legacy_owner()?)
    }
    #[cfg(test)]
    fn bind_storage_path(&mut self, path: PathBuf, epoch: u64) -> Result<(), String> {
        self.bind_storage_path_owned(path, epoch, self.editor.clone())
    }
    fn bind_storage_path_owned(
        &mut self,
        path: PathBuf,
        epoch: u64,
        migration_owner: EditorId,
    ) -> Result<(), String> {
        if self.persisted.as_ref().is_some_and(|p| {
            p.epoch == epoch && p.path == path && p.migration_owner == migration_owner
        }) {
            return Ok(());
        }
        self.flush_storage()?;
        let state = store::load(&path, &migration_owner)?;
        let (snapshot, observed) = state.for_editor(&self.editor);
        for saved in snapshot.drafts.values().cloned() {
            let scope = DraftScope {
                draft_origin: saved.draft_origin.clone(),
                connection: epoch,
                profile: saved.profile.clone(),
                profile_intent: saved.profile_intent,
                session: saved.session.clone(),
            };
            self.drafts.entry(scope).or_insert_with(|| saved.restore());
        }
        self.favorites.extend(
            snapshot
                .favorites
                .iter()
                .cloned()
                .map(|(provider, id)| (epoch, provider, id)),
        );
        self.persisted = Some(Persistence {
            epoch,
            path,
            migration_owner,
            previous: snapshot,
            observed,
            pending: None,
            scanned_revision: None,
        });
        Ok(())
    }
    /// A second stored-session view may start from shared text, never another input's files.
    pub fn seed_stored_draft(&mut self, scope: &DraftScope) -> Result<bool, String> {
        if scope.session.is_none() || self.drafts.contains_key(scope) {
            return Ok(false);
        }
        let Some(p) = self
            .persisted
            .as_ref()
            .filter(|p| p.epoch == scope.connection)
        else {
            return Ok(false);
        };
        let state = store::load(&p.path, &p.migration_owner)?;
        let latest = state
            .entries
            .iter()
            .filter(|entry| {
                entry.profile == scope.profile
                    && entry.profile_intent == scope.profile_intent
                    && entry.session == scope.session
            })
            .max_by_key(|entry| entry.revision);
        let Some(text) = latest
            .and_then(|entry| entry.draft.as_ref())
            .map(|draft| draft.text.clone())
        else {
            return Ok(false);
        };
        if text.is_empty() {
            return Ok(false);
        }
        self.draft_mut(scope.clone()).text = TextEdit::new(text);
        Ok(true)
    }
    fn storage_snapshot(&self, epoch: u64) -> Snapshot {
        Snapshot {
            drafts: self
                .drafts
                .iter()
                .filter(|(scope, _)| scope.connection == epoch)
                .map(|(scope, draft)| {
                    let saved = SavedDraft::capture(scope, draft);
                    (saved.key(), saved)
                })
                .collect(),
            favorites: self
                .favorites
                .iter()
                .filter(|(connection, _, _)| *connection == epoch)
                .map(|(_, provider, id)| (provider.clone(), id.clone()))
                .collect(),
        }
    }
    pub fn flush_storage(&mut self) -> Result<(), String> {
        let Some(p) = &self.persisted else {
            return Ok(());
        };
        let snapshot = self.storage_snapshot(p.epoch);
        if snapshot == p.previous {
            return Ok(());
        }
        let observed = store::commit(
            &p.path,
            &self.editor,
            &p.previous,
            &snapshot,
            &p.observed,
            &p.migration_owner,
        )?;
        let p = self.persisted.as_mut().unwrap();
        p.previous = snapshot;
        p.observed = observed;
        p.pending = None;
        Ok(())
    }
    pub fn poll_storage(&mut self, now: f64) -> Result<Option<std::time::Duration>, String> {
        let Some(epoch) = self.persisted.as_ref().map(|p| p.epoch) else {
            return Ok(None);
        };
        let revision = self.storage_revision;
        if self.persisted.as_ref().unwrap().scanned_revision != Some(revision) {
            let snapshot = self.storage_snapshot(epoch);
            let p = self.persisted.as_mut().unwrap();
            p.scanned_revision = Some(revision);
            if p.pending
                .as_ref()
                .map(|(value, _)| value)
                .unwrap_or(&p.previous)
                != &snapshot
            {
                p.pending = Some((snapshot, now + 0.35));
            }
        }
        let p = self.persisted.as_ref().unwrap();
        let Some((snapshot, due)) = &p.pending else {
            return Ok(None);
        };
        if now < *due {
            return Ok(Some(std::time::Duration::from_secs_f64(
                (*due - now).max(0.001),
            )));
        }
        let result = store::commit(
            &p.path,
            &self.editor,
            &p.previous,
            snapshot,
            &p.observed,
            &p.migration_owner,
        );
        let p = self.persisted.as_mut().unwrap();
        match result {
            Ok(observed) => {
                p.previous = p.pending.take().unwrap().0;
                p.observed = observed;
                Ok(None)
            }
            Err(error) => {
                p.pending.as_mut().unwrap().1 = now + 1.;
                Err(error)
            }
        }
    }
}
fn legacy_owner() -> Result<EditorId, String> {
    crate::client::windows::WindowStore::system()?.legacy_editor_id()
}
impl Drop for Composer {
    fn drop(&mut self) {
        let _ = self.flush_storage();
    }
}
#[cfg(test)]
mod tests;
