//! Desktop overlays are scoped by the external gateway principal and profile.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};
#[cfg(test)]
mod tests;
#[derive(Default)]
pub struct ProfilePreferences {
    entries: BTreeMap<String, Value>,
    writable: bool,
    baseline: BTreeMap<String, Value>,
    path: Option<PathBuf>,
    retry_at: Option<Instant>,
    error: Option<String>,
    removals: BTreeSet<Vec<String>>,
    read_only_fallbacks: BTreeMap<String, Value>,
}
impl ProfilePreferences {
    pub fn load() -> Result<Self, String> {
        Self::load_at(super::state_dir()?.join("profile-preferences.json"))
    }
    fn load_at(path: PathBuf) -> Result<Self, String> {
        let entries: BTreeMap<String, Value> = super::store::load(&path, |_| Ok(()))?;
        Ok(Self {
            baseline: entries.clone(),
            entries,
            writable: true,
            path: Some(path),
            retry_at: None,
            error: None,
            removals: BTreeSet::new(),
            read_only_fallbacks: BTreeMap::new(),
        })
    }
    fn key(connection: &crate::gateway::Connection, profile: &str) -> Result<String, String> {
        Ok(format!("{}:{profile}", super::connection_key(connection)?))
    }
    pub fn get(&self, c: &crate::gateway::Connection, p: &str) -> Option<&Value> {
        Self::key(c, p).ok().and_then(|k| self.entries.get(&k))
    }
    // Owner adoption is not an edit. Remember its inherited presentation only
    // while the saved overlays are unavailable; explicit assigns remain pending.
    fn adopt_fallback(
        &mut self,
        c: &crate::gateway::Connection,
        p: &str,
        value: Value,
    ) -> Result<(), String> {
        if !self.writable && self.get(c, p).is_none() {
            self.read_only_fallbacks
                .entry(Self::key(c, p)?)
                .or_insert(value);
        }
        Ok(())
    }
    fn remember_implicit(
        &mut self,
        c: &crate::gateway::Connection,
        p: &str,
        value: Value,
    ) -> Result<(), String> {
        let key = Self::key(c, p)?;
        if !self.writable
            && !self.entries.contains_key(&key)
            && self.read_only_fallbacks.get(&key) == Some(&value)
        {
            return Ok(());
        }
        if self.entries.get(&key) != Some(&value) {
            self.assign(c, p, value)?;
        }
        Ok(())
    }
    pub fn assign(
        &mut self,
        c: &crate::gateway::Connection,
        p: &str,
        value: Value,
    ) -> Result<(), String> {
        self.entries.insert(Self::key(c, p)?, value);
        self.save()
    }
    pub fn remove(&mut self, c: &crate::gateway::Connection, p: &str) -> Result<(), String> {
        let key = Self::key(c, p)?;
        self.entries.remove(&key);
        self.removals.insert(vec![key]);
        self.save()
    }
    pub fn rename(
        &mut self,
        c: &crate::gateway::Connection,
        from: &str,
        to: &str,
    ) -> Result<(), String> {
        if let Some(value) = self.entries.remove(&Self::key(c, from)?) {
            self.entries.insert(Self::key(c, to)?, value);
        }
        self.save()
    }
    fn save(&mut self) -> Result<(), String> {
        if self.entries == self.baseline && self.removals.is_empty() {
            self.retry_at = None;
            self.error = None;
            return Ok(());
        }
        if !self.writable {
            return Err("Profile preferences were not loaded; their file was retained".into());
        }
        let result = self
            .path
            .as_ref()
            .ok_or_else(|| "The profile preferences directory is unavailable".to_owned())
            .and_then(|path| {
                super::store::save_patch_forced(
                    path,
                    &self.baseline,
                    &self.entries,
                    &self.removals.iter().cloned().collect::<Vec<_>>(),
                    |_| Ok(()),
                )
            });
        match result {
            Ok(_) => {
                // Other windows may have their own appearance. Retain our local
                // snapshot as the diff baseline so polling cannot echo theirs back.
                self.baseline = self.entries.clone();
                self.removals.clear();
                self.retry_at = None;
                self.error = None;
                Ok(())
            }
            Err(error) => {
                self.retry_at = Some(Instant::now() + Duration::from_secs(2));
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
    /// Force pending overlays before close, regardless of the retry deadline.
    /// An untouched unavailable store has no local work to lose.
    pub fn flush_storage(&mut self) -> Result<(), String> {
        self.save()
    }
    pub fn poll_storage(&mut self) -> Result<bool, String> {
        if self.retry_at.is_some_and(|at| at <= Instant::now()) {
            self.save().map(|_| true)
        } else {
            Ok(false)
        }
    }
    pub fn storage_error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn wake_after(&self) -> Option<Duration> {
        self.retry_at
            .map(|at| at.saturating_duration_since(Instant::now()))
    }
}
impl crate::app::Viewer {
    pub fn prepare_bot_export(&mut self) {
        let Some(profile) = self.bots.selected.clone() else {
            return;
        };
        let mut overlay = self.profile_overlay(&profile);
        overlay["layoutTree"] = json!(self.layout);
        self.bots.set_export_overlay(profile, overlay);
    }
    pub fn profile_overlay(&self, profile: &str) -> Value {
        let mut value = self
            .current_connection()
            .and_then(|c| self.profile_preferences.get(c, profile))
            .cloned()
            .unwrap_or_else(|| json!({"version":1,"skin":"opencode","profileColor":null}));
        if profile == self.profile {
            value["mode"] = json!(match self.theme {
                Some(true) => "dark",
                Some(false) => "light",
                None => "system",
            });
        }
        value
    }
    pub fn import_profile_overlay(&mut self, profile: &str, value: Value) {
        let Some(connection) = self.current_connection().cloned() else {
            return;
        };
        if value.is_object() {
            if let Some(tree) = value
                .get("layoutTree")
                .and_then(|v| serde_json::from_value::<crate::layout::Node>(v.clone()).ok())
                .and_then(|t| t.normalize())
                .filter(|t| t.panes().iter().any(|p| p == "workspace"))
            {
                self.layout = tree;
            }
            if let Err(error) = self.profile_preferences.assign(&connection, profile, value) {
                self.client_error = Some(error);
            }
            if profile == self.profile {
                self.restore_profile_appearance();
            }
        }
    }
    pub fn restore_profile_appearance(&mut self) {
        let Some(connection) = self.current_connection().cloned() else {
            return;
        };
        if let Some(value) = self.profile_preferences.get(&connection, &self.profile) {
            self.theme = match value["mode"].as_str() {
                Some("dark") => Some(true),
                Some("light") => Some(false),
                Some("system") => None,
                _ => self.theme,
            };
        }
        let fallback = self.profile_overlay(&self.profile);
        if let Err(error) =
            self.profile_preferences
                .adopt_fallback(&connection, &self.profile, fallback)
        {
            self.client_error = Some(error);
        }
    }
    pub fn remember_profile_appearance(&mut self) {
        if self.is_fixture() || !self.can_remember_profile_preferences() {
            return;
        }
        let Some(connection) = self.current_connection().cloned() else {
            return;
        };
        let value = self.profile_overlay(&self.profile);
        if let Err(error) =
            self.profile_preferences
                .remember_implicit(&connection, &self.profile, value)
        {
            self.client_error = Some(error);
        }
    }
}
