//! Each verified source owns its sidebar reads and mutation receipts independently of focus.
use super::{Action, Preferences, SidebarState};
use crate::{
    composer::DraftScope,
    gateway::Profile,
    pages::{PageEffect, PageNetwork},
    source::{Binding, SessionOwner, SourceStamp},
};
use rune::Waker;

pub struct SourceSidebar {
    binding: Binding,
    label: String,
    state: SidebarState,
    network: PageNetwork,
    scope: DraftScope,
    retired: bool,
}
impl SourceSidebar {
    pub fn new(
        binding: Binding,
        epoch: u64,
        profile: String,
        waker: Waker,
    ) -> Result<Self, String> {
        Self::new_at(binding, epoch, profile, waker, crate::client::state_dir()?)
    }
    pub fn new_at(
        binding: Binding,
        epoch: u64,
        profile: String,
        waker: Waker,
        directory: std::path::PathBuf,
    ) -> Result<Self, String> {
        binding.validate()?;
        let mut state = SidebarState::new();
        state.partition_preferences = true;
        state.bind_source_at(&binding, epoch, directory)?;
        if state.source_stamp() != Some(binding.stamp()) {
            return Err("The sidebar source binding could not be established".into());
        }
        state.owner_profile = profile;
        if let Some(lease) = binding.lease() {
            state.set_canonical_profiles(lease.profiles());
        }
        let mut network = PageNetwork::default();
        network.init(waker);
        Ok(Self {
            binding,
            label: "Source".into(),
            state,
            network,
            scope: DraftScope {
                draft_origin: None,
                profile_intent: crate::source::ProfileIntent::Backend,
                connection: epoch,
                profile: "default".into(),
                session: None,
            },
            retired: false,
        })
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn set_label(&mut self, label: String) {
        self.label = label;
    }
    pub fn stamp(&self) -> &SourceStamp {
        self.binding.stamp()
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn state(&self) -> &SidebarState {
        &self.state
    }
    pub fn state_mut(&mut self) -> &mut SidebarState {
        &mut self.state
    }
    pub fn owner(&self, profile: &str) -> SessionOwner {
        SessionOwner::new(self.stamp().clone(), profile)
    }
    pub fn set_preferences(&mut self, preferences: &Preferences) {
        let reload = self.state.prefs.all_profiles != preferences.all_profiles
            || self.state.prefs.archived != preferences.archived;
        self.state.set_partition_preferences(preferences);
        if reload && !self.retired {
            let profile = self.state.owner_profile.clone();
            self.load(&profile);
        }
    }
    pub fn is_retired(&self) -> bool {
        self.retired
    }
    pub fn retire(&mut self, reason: String) {
        self.retired = true;
        self.network.invalidate();
        self.state.reset_data();
        self.state.retired = true;
        self.state.mutation_inflight = false;
        if self
            .state
            .mutation
            .as_ref()
            .is_some_and(|edit| edit.identity.is_none())
        {
            self.state.mutation = None;
            if let Some(store) = &mut self.state.receipt_store {
                store.release_unprepared();
            }
        }
        self.state.notice = Some(reason);
    }
    pub fn set_profiles(&mut self, profiles: &[Profile]) {
        self.state.set_canonical_profiles(profiles);
    }
    pub fn blocks_owner_change(&self) -> bool {
        self.state.blocks_owner_change()
    }
    pub fn load(&mut self, profile: &str) {
        if self.retired {
            return;
        }
        let effects = self.state.load(profile);
        self.dispatch(effects);
    }
    pub fn action(&mut self, action: Action) {
        if self.retired {
            return;
        }
        let effects = self.state.action(action);
        self.dispatch(effects);
    }
    fn dispatch(&mut self, effects: Vec<PageEffect>) {
        for (operation, request) in effects {
            let result = self.binding.request(request).and_then(|request| {
                self.network.request(
                    &self.scope,
                    self.binding.connection().clone(),
                    &operation,
                    request,
                )
            });
            if let Err(error) = result {
                let followup = self.state.apply(&operation, Err(error));
                if !followup.is_empty() {
                    self.dispatch(followup);
                }
            }
        }
    }
    pub fn poll(&mut self, now: f64) -> bool {
        if self.retired {
            return false;
        }
        if let Err(error) = self.binding.validate() {
            self.retire(error);
            return true;
        }
        let replies = self.network.poll(&self.scope);
        let changed = !replies.is_empty();
        for reply in replies {
            if !reply.progress {
                let effects = self.state.apply(&reply.operation, reply.result);
                self.dispatch(effects);
            }
        }
        let effects = self.state.tick(now);
        let ticking = !effects.is_empty();
        self.dispatch(effects);
        changed || ticking
    }
}

impl Drop for SourceSidebar {
    fn drop(&mut self) {
        // Queued readonly work has no consumer. Sent writes keep their own lease and claim.
        self.network.invalidate();
    }
}
