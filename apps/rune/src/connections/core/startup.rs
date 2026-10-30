//! Registry launch choices and confirmed profile memory are pure metadata writes.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
impl Core {
    pub(in crate::connections) fn visible_registry(&self) -> Registry {
        let mut visible = self.registry.clone();
        visible.selected = self
            .active_owner
            .as_ref()
            .filter(|(id, revision)| {
                visible
                    .rows
                    .iter()
                    .any(|row| &row.id == id && &row.revision == revision)
            })
            .map(|(id, _)| id.clone());
        visible
    }
    pub(in crate::connections) fn observe_registry(&mut self) -> Result<Registry, String> {
        self.refresh_registry()?;
        Ok(self.visible_registry())
    }
    pub(super) fn make_primary(&mut self, id: &str) -> Result<Outcome, String> {
        if !self.registry.rows.iter().any(|row| row.id == id) {
            return Err("The primary connection no longer exists".into());
        }
        let mut next = self.registry.clone();
        next.primary = Some(id.into());
        self.write_policy(next)
    }
    pub(super) fn launch_mode(&mut self, mode: LaunchMode) -> Result<Outcome, String> {
        let mut next = self.registry.clone();
        next.launch_mode = mode;
        self.write_policy(next)
    }
    fn write_policy(&mut self, next: Registry) -> Result<Outcome, String> {
        self.metadata.save(&next)?;
        self.registry = next;
        Ok(Outcome::Registry(self.visible_registry(), None))
    }
    fn require_source(&self, id: &str, revision: &str) -> Result<(), String> {
        if !self
            .registry
            .rows
            .iter()
            .any(|row| row.id == id && row.revision == revision)
        {
            return Err("The saved source changed or was removed; refresh its route".into());
        }
        Ok(())
    }
    pub(super) fn remember_profile(
        &mut self,
        id: &str,
        revision: &str,
        profile: &str,
    ) -> Result<Outcome, String> {
        self.require_source(id, revision)?;
        if !super::super::startup::valid_profile(profile) {
            return Err("The confirmed profile name is invalid".into());
        }
        let mut next = self.registry.clone();
        next.last_profiles.insert(id.into(), profile.into());
        self.write_policy(next)
    }
    pub(super) fn profile_changed(
        &mut self,
        id: &str,
        revision: &str,
        old: &str,
        new: Option<&str>,
    ) -> Result<Outcome, String> {
        self.require_source(id, revision)?;
        if !super::super::startup::valid_profile(old)
            || new.is_some_and(|profile| !super::super::startup::valid_profile(profile))
        {
            return Err("The changed profile name is invalid".into());
        }
        if self.registry.last_profiles.get(id).map(String::as_str) != Some(old) {
            return Ok(Outcome::Registry(self.visible_registry(), None));
        }
        let mut next = self.registry.clone();
        match new {
            Some(name) => {
                next.last_profiles.insert(id.into(), name.into());
            }
            None => {
                next.last_profiles.remove(id);
            }
        }
        self.write_policy(next)
    }
    pub(super) fn remember_activation(next: &mut Registry, id: &str, profile: &str) {
        next.last_used = Some(id.into());
        next.last_profiles.insert(id.into(), profile.into());
    }
    pub(super) fn target_profile(
        &self,
        id: &str,
        explicit: Option<&str>,
    ) -> Result<String, String> {
        let profile = explicit
            .or_else(|| self.registry.last_profiles.get(id).map(String::as_str))
            .unwrap_or("default");
        if !super::super::startup::valid_profile(profile) {
            return Err("The target profile name is invalid".into());
        }
        Ok(profile.into())
    }
    pub(super) fn commit_route(
        &mut self,
        lease: Lease,
        profile: &str,
        alive: &AtomicBool,
    ) -> Result<Outcome, String> {
        require_live_route(alive)?;
        self.require_source(lease.source_id(), lease.revision())?;
        if !lease.is_valid() {
            return Err(lease
                .error()
                .unwrap_or_else(|| "The saved-source lease expired".into()));
        }
        let profile = self.target_profile(lease.source_id(), Some(profile))?;
        require_live_route(alive)?;
        let profiles = self.probe.check(lease.connection())?;
        if !profiles.iter().any(|row| row.name == profile) {
            return Err("The selected source no longer serves that agent profile".into());
        }
        require_live_route(alive)?;
        if !lease.is_valid() {
            return Err("The saved-source lease expired while preparing the route".into());
        }
        let mut next = self.registry.clone();
        Self::remember_activation(&mut next, lease.source_id(), &profile);
        // The application has not accepted the receipt yet: outgoing foreground stays owned.
        self.metadata.save(&next)?;
        self.registry = next;
        Ok(Outcome::Ready(
            self.visible_registry(),
            lease.connection().clone(),
            Some(lease),
        ))
    }
    pub(in crate::connections) fn release_foreground(&mut self, id: &str, revision: &str) {
        if self
            .active_owner
            .as_ref()
            .is_none_or(|owner| owner.0 != id || owner.1 != revision)
        {
            return;
        }
        self.active_owner = None;
        self.primary_lease = None;
        self.poll_leases();
    }
}
fn require_live_route(alive: &AtomicBool) -> Result<(), String> {
    if alive.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("The route activation was superseded".into())
    }
}
