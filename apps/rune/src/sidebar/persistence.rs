//! Sidebar writers merge their own edits; a failed load never grants write access.
use super::{Preferences, SidebarState};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) const KNOWN_FIELDS: &[&str] = &[
    "grouping",
    "show_all_sessions",
    "ordering",
    "collapsed",
    "all_profiles",
    "archived",
    "cards",
    "preview",
    "tokens",
    "cost",
    "profile",
    "rail",
    "status",
    "profiles",
    "projects",
];

#[derive(Default)]
pub(super) struct Storage {
    path: Option<PathBuf>,
    baseline: Preferences,
    writable: bool,
    retry_at: Option<Instant>,
    error: Option<String>,
    touched: BTreeSet<String>,
}

impl Preferences {
    fn validate(&self) -> Result<(), String> {
        for name in ["version", "schema_version"] {
            if self.other.get(name).is_some_and(|v| v.as_u64() != Some(1)) {
                return Err(
                    "Saved sidebar preferences use an unsupported schema; their file was preserved"
                        .into(),
                );
            }
        }
        Ok(())
    }
}

impl SidebarState {
    pub fn load_preferences(&mut self) {
        match crate::client::state_dir() {
            Ok(path) => self.load_preferences_at(path.join("sidebar.json")),
            Err(error) => {
                self.preferences_storage = Storage {
                    error: Some(error.clone()),
                    ..Default::default()
                };
                self.notice = Some(error);
            }
        }
    }

    fn load_preferences_at(&mut self, path: PathBuf) {
        let mut storage = Storage {
            path: Some(path.clone()),
            ..Default::default()
        };
        match crate::client::store::load::<Preferences>(&path, Preferences::validate) {
            Ok(prefs) => {
                storage.baseline = prefs.clone();
                storage.writable = true;
                if self.notice == self.preferences_storage.error {
                    self.notice = None;
                }
                self.prefs = prefs;
            }
            Err(error) => {
                storage.error = Some(error.clone());
                self.notice = Some(error);
            }
        }
        self.preferences_storage = storage;
    }

    pub(super) fn save_preferences(&mut self, fields: &[&str]) {
        if self.demo || self.partition_preferences {
            return;
        }
        self.preferences_storage
            .touched
            .extend(fields.iter().map(|field| (*field).to_owned()));
        let _ = self.flush_storage();
    }

    /// Force pending local intents before close; an untouched unreadable file is
    /// preserved without preventing its window from closing.
    pub fn flush_storage(&mut self) -> Result<(), String> {
        if self.demo
            || self.partition_preferences
            || (self.preferences_storage.touched.is_empty()
                && self.prefs == self.preferences_storage.baseline)
        {
            return Ok(());
        }
        if !self.preferences_storage.writable {
            let error = self.preferences_storage.error.clone().unwrap_or_else(|| {
                "Sidebar preferences were not loaded; their file was preserved".into()
            });
            self.notice = Some(error.clone());
            return Err(error);
        }
        self.write_preferences();
        match &self.preferences_storage.error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn write_preferences(&mut self) -> bool {
        let before = self.notice.clone();
        let storage = &mut self.preferences_storage;
        let result = storage
            .path
            .as_ref()
            .ok_or_else(|| "The sidebar preferences directory is unavailable".to_owned())
            .and_then(|path| {
                crate::client::store::save_patch_forced(
                    path,
                    &storage.baseline,
                    &self.prefs,
                    &storage
                        .touched
                        .iter()
                        .map(|field| vec![field.clone()])
                        .collect::<Vec<_>>(),
                    Preferences::validate,
                )
            });
        match result {
            Ok(merged) => {
                let changed = merged != self.prefs;
                // The controls read prefs directly. Adopt the merged snapshot and
                // its baseline together so a later edit cannot echo stale fields.
                storage.baseline = merged.clone();
                self.prefs = merged;
                storage.retry_at = None;
                storage.touched.clear();
                if self.notice == storage.error {
                    self.notice = None;
                }
                storage.error = None;
                changed || before != self.notice
            }
            Err(error) => {
                storage.retry_at = Some(Instant::now() + Duration::from_secs(2));
                storage.error = Some(error.clone());
                self.notice = Some(error);
                before != self.notice
            }
        }
    }

    pub fn poll_storage(&mut self) -> bool {
        if self.demo
            || !self.preferences_storage.writable
            || !self
                .preferences_storage
                .retry_at
                .is_some_and(|at| at <= Instant::now())
        {
            return false;
        }
        self.write_preferences()
    }

    pub fn storage_wake_after(&self) -> Option<Duration> {
        (!self.demo && self.preferences_storage.writable)
            .then_some(self.preferences_storage.retry_at)
            .flatten()
            .map(|at| at.saturating_duration_since(Instant::now()))
    }
}

#[cfg(test)]
mod tests;
