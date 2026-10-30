//! Device launch policy names logical saved sources; it never opens an agent.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchMode {
    #[default]
    Primary,
    LastUsed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupCandidate {
    pub id: String,
    pub revision: String,
    pub profile: String,
}
/// A coordinator owns its intent and decides whether this receipt may navigate.
pub struct RouteCommit {
    pub intent: u64,
    pub profile: String,
    pub result: Result<Connection, String>,
}
pub struct RouteTicket {
    pub receiver: Receiver<RouteCommit>,
    alive: Arc<AtomicBool>,
}
impl RouteTicket {
    pub fn cancel(&self) {
        self.alive.store(false, Ordering::Release);
    }
}
impl Drop for RouteTicket {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Registry {
    pub(super) fn primary_fallback(&self) -> Option<String> {
        self.rows
            .iter()
            .find(|row| row.kind() == Kind::Local)
            .map(|row| row.id.clone())
    }
    pub(super) fn contains(&self, id: &Option<String>) -> bool {
        id.as_ref()
            .is_some_and(|id| self.rows.iter().any(|row| &row.id == id))
    }
    pub(super) fn normalize(&mut self, legacy: bool) {
        if !self.contains(&self.primary) {
            self.primary = if legacy {
                self.selected
                    .as_ref()
                    .filter(|_| self.contains(&self.selected))
                    .cloned()
                    .or_else(|| self.primary_fallback())
                    .or_else(|| self.rows.first().map(|row| row.id.clone()))
            } else {
                self.primary_fallback()
            };
        }
        if !self.contains(&self.last_used) {
            self.last_used = self.primary.clone();
        }
        self.last_profiles
            .retain(|id, _| self.rows.iter().any(|row| &row.id == id));
    }
    pub(super) fn candidate(&self) -> Option<StartupCandidate> {
        let id = if self.launch_mode == LaunchMode::LastUsed && self.contains(&self.last_used) {
            self.last_used.as_ref()
        } else {
            self.primary.as_ref()
        }?;
        let row = self.rows.iter().find(|row| &row.id == id)?;
        Some(StartupCandidate {
            id: row.id.clone(),
            revision: row.revision.clone(),
            profile: self
                .last_profiles
                .get(id)
                .cloned()
                .unwrap_or_else(|| "default".into()),
        })
    }
}
pub(super) fn valid_profile(profile: &str) -> bool {
    let bytes = profile.as_bytes();
    (1..=64).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'_' | b'-')
        })
}

impl Connections {
    pub fn primary(&self) -> Option<&str> {
        self.registry.primary.as_deref()
    }
    pub fn last_used(&self) -> Option<&str> {
        self.registry.last_used.as_deref()
    }
    pub fn launch_mode(&self) -> LaunchMode {
        self.registry.launch_mode
    }
    pub fn registry_loaded(&self) -> bool {
        self.registry_loaded
    }
    pub fn remembered_profile(&self, id: &str) -> Option<&str> {
        self.registry.last_profiles.get(id).map(String::as_str)
    }
    pub fn selected_profile(&self) -> Option<&str> {
        self.selected
            .as_deref()
            .map(|id| self.remembered_profile(id).unwrap_or("default"))
    }
    /// Selection is metadata only. The application owns activation and explicit defaults.
    pub fn startup_candidate(&self) -> Result<Option<StartupCandidate>, String> {
        if let Some(error) = &self.registry_error {
            return Err(error.clone());
        }
        if !self.registry_loaded {
            return Err("Saved connections have not loaded yet".into());
        }
        Ok(self.registry.candidate())
    }
    /// Call on mount/focus/registry changes. Reading cannot close an entered form or dial a source.
    pub fn observe_registry(&mut self) {
        if self.synthetic || self.observing {
            return;
        }
        match self
            .network
            .as_ref()
            .ok_or("The connection worker is unavailable")
            .and_then(|network| {
                network
                    .send
                    .send(Work::Observe)
                    .map_err(|_| "The connection worker is unavailable")
            }) {
            Ok(()) => self.observing = true,
            Err(error) => self.registry_error = Some(error.into()),
        }
    }
    /// The caller has observed this exact source/profile as Connected, not merely requested it.
    pub fn remember_confirmed_profile(&mut self, id: &str, revision: &str, profile: &str) {
        self.maintain(Command::RememberProfile {
            id: id.into(),
            revision: revision.into(),
            profile: profile.into(),
        });
    }
    /// Call only after a successful source-qualified agent rename/delete response.
    pub fn profile_changed(&mut self, id: &str, revision: &str, old: &str, new: Option<String>) {
        self.maintain(Command::ProfileChanged {
            id: id.into(),
            revision: revision.into(),
            old: old.into(),
            new,
        });
    }
    fn maintain(&mut self, command: Command) {
        if self.synthetic {
            self.error = Some("Connection operations are disabled in synthetic scenes".into());
            return;
        }
        if !self
            .network
            .as_ref()
            .is_some_and(|network| network.send.send(Work::Maintain(command)).is_ok())
        {
            self.error = Some("The connection worker is unavailable".into());
        }
    }
    pub fn commit_route(
        &self,
        lease: Lease,
        profile: String,
        intent: u64,
    ) -> Result<RouteTicket, String> {
        if self.synthetic {
            return Err("Connection operations are disabled in synthetic scenes".into());
        }
        let network = self
            .network
            .as_ref()
            .ok_or("The connection worker is unavailable")?;
        let (reply, receive) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        network
            .send
            .send(Work::CommitRoute {
                lease,
                profile,
                intent,
                alive: alive.clone(),
                reply,
            })
            .map_err(|_| "The connection worker is unavailable")?;
        Ok(RouteTicket {
            receiver: receive,
            alive,
        })
    }
    pub fn active_lease(&self) -> Option<Lease> {
        self.active_lease.clone().filter(Lease::is_valid)
    }
    /// The coordinator calls this only after accepting its correlated route receipt.
    pub fn note_active_source(&mut self, connection: Option<&Connection>) {
        self.active_source = match connection {
            Some(Connection::Saved { id, revision, .. }) => Some((id.clone(), revision.clone())),
            _ => None,
        };
        if self.active_lease.as_ref().is_some_and(|lease| {
            self.active_source.as_ref().is_none_or(|(id, revision)| {
                lease.source_id() != id || lease.revision() != revision
            })
        }) {
            self.active_lease = None;
        }
        self.selected = self.active_source.as_ref().map(|(id, _)| id.clone());
    }
    /// Release only the outgoing service foreground; independent peer leases remain owned.
    pub fn release_foreground(&mut self, id: &str, revision: &str) {
        if let Some(network) = &self.network {
            if network
                .send
                .send(Work::ReleaseForeground {
                    id: id.into(),
                    revision: revision.into(),
                })
                .is_err()
            {
                self.error = Some("The connection worker is unavailable".into());
            }
        }
    }
    pub(super) fn apply_registry(&mut self, registry: Registry) {
        self.rows = registry.rows.clone();
        if self.active_source.as_ref().is_some_and(|(id, revision)| {
            !registry
                .rows
                .iter()
                .any(|row| &row.id == id && &row.revision == revision)
        }) {
            self.active_source = None;
            self.active_lease = None;
        }
        self.selected = self.active_source.as_ref().map(|(id, _)| id.clone());
        self.registry = registry;
        self.registry_loaded = true;
        self.registry_error = None;
    }
    pub(super) fn observe_result(&mut self, result: Result<Registry, String>) {
        match result {
            Ok(registry) => {
                let old_error = self.registry_error.clone();
                self.apply_registry(registry);
                if old_error.is_some() && self.error == old_error {
                    self.error = None;
                }
            }
            Err(error) => {
                self.registry_error = Some(error.clone());
                self.error = Some(error);
            }
        }
    }
}
