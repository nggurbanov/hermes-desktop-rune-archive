//! Desktop overlays are scoped by the external gateway principal and profile.
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Default)]
pub struct ProfilePreferences {
    entries: BTreeMap<String, Value>,
    writable: bool,
}
impl ProfilePreferences {
    pub fn load() -> Result<Self, String> {
        let path = super::state_dir()?.join("profile-preferences.json");
        let entries = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Saved profile preferences are invalid; the file was retained")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(_) => return Err("Profile preferences could not be read".into()),
        };
        Ok(Self {
            entries,
            writable: true,
        })
    }
    fn key(connection: &crate::gateway::Connection, profile: &str) -> Result<String, String> {
        Ok(format!("{}:{profile}", super::connection_key(connection)?))
    }
    pub fn get(&self, c: &crate::gateway::Connection, p: &str) -> Option<&Value> {
        Self::key(c, p).ok().and_then(|k| self.entries.get(&k))
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
        self.entries.remove(&Self::key(c, p)?);
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
    fn save(&self) -> Result<(), String> {
        if !self.writable {
            return Err("Profile preferences were not loaded; their file was retained".into());
        }
        super::private_write(
            &super::state_dir()?.join("profile-preferences.json"),
            &serde_json::to_vec_pretty(&self.entries)
                .map_err(|_| "Profile preferences could not be encoded")?,
        )
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
        if let Some(value) = self
            .current_connection()
            .and_then(|c| self.profile_preferences.get(c, &self.profile))
        {
            self.theme = match value["mode"].as_str() {
                Some("dark") => Some(true),
                Some("light") => Some(false),
                Some("system") => None,
                _ => self.theme,
            };
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
        if self.profile_preferences.get(&connection, &self.profile) != Some(&value) {
            if let Err(error) = self
                .profile_preferences
                .assign(&connection, &self.profile, value)
            {
                self.client_error = Some(error);
            }
        }
    }
}
