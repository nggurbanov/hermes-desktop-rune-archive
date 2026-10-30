use super::*;
/// Runtime work stays with its retained source while another source is foreground.
pub(super) struct Workspace {
    binding: Binding,
    retained: crate::fleet::SourceRetention,
    _resolver: Option<crate::connections::Resolver>,
    epoch: u64,
    profile: String,
    selected: Option<String>,
    tabs: Vec<ConversationTab>,
    sessions: Vec<Session>,
    messages: Vec<HistoryMessage>,
    bot_empty: bool,
    truncated: bool,
    all_profiles: bool,
}
impl Workspace {
    fn valid(&self) -> bool {
        self.binding.validate().is_ok() && self.retained.validate().is_ok()
    }
}
impl Viewer {
    pub(in crate::app) fn captured_fleet_binding(
        &self,
        version: &SourceVersion,
        principal: Option<&str>,
    ) -> Option<Binding> {
        let matches = |binding: &Binding| {
            binding.stamp().version() == version
                && principal.is_none_or(|expected| expected == binding.stamp().principal())
                && binding.validate().is_ok()
        };
        self.current_source_binding()
            .ok()
            .filter(&matches)
            .or_else(|| {
                self.fleet_state
                    .workspaces
                    .values()
                    .filter(|w| w.valid())
                    .map(|w| w.binding.clone())
                    .find(&matches)
            })
            .or_else(|| {
                self.fleet_state
                    .partitions
                    .values()
                    .filter(|p| !p.is_retired())
                    .map(|p| p.binding().clone())
                    .find(&matches)
            })
            .or_else(|| self.fleet.binding(version).filter(matches))
    }
    pub(in crate::app) fn binding_for_source_epoch(&self, epoch: u64) -> Option<Binding> {
        if epoch == self.connection_epoch {
            return self.current_source_binding().ok();
        }
        self.fleet_state
            .workspaces
            .values()
            .find(|workspace| workspace.epoch == epoch && workspace.valid())
            .map(|workspace| workspace.binding.clone())
    }
    pub(in crate::app) fn retained_source_epochs(&self) -> Vec<u64> {
        let mut epochs = self
            .fleet_state
            .workspaces
            .values()
            .filter(|workspace| workspace.valid())
            .map(|workspace| workspace.epoch)
            .collect::<Vec<_>>();
        if self.current_source_binding().is_ok() {
            epochs.push(self.connection_epoch);
        }
        epochs.sort_unstable();
        epochs.dedup();
        epochs
    }
    pub(in crate::app) fn inspect_parked_sources(&self) -> Vec<serde_json::Value> {
        self.fleet_state.workspaces.values().map(|workspace| {
            let scope = DraftScope {connection:workspace.epoch, profile:workspace.profile.clone(), session:workspace.selected.clone()};
            let editor = self.inspect_owned_editor(&scope);
            let view = self.conversation.view(&scope);
            serde_json::json!({
                "source":workspace.binding.stamp(), "epoch":workspace.epoch,
                "profile":workspace.profile, "session":workspace.selected,
                "tabs":workspace.tabs.iter().map(|tab|serde_json::json!({"profile":tab.profile,"session":tab.session,"title":tab.title,"bot":tab.bot})).collect::<Vec<_>>(),
                "editor":editor.as_ref().and_then(|e|e.get("editor")),
                "draft":editor.as_ref().and_then(|e|e.get("draft")),
                "running":view.is_some_and(|view|view.running),
                "active_text":view.map(|view|view.active_text.as_str()).unwrap_or("")
            })
        }).collect()
    }
    pub(in crate::app) fn target_foreground_slot(
        &self,
        connection: &Connection,
    ) -> Option<(String, String)> {
        let stamp = SourceStamp::from_connection(connection).ok()?;
        if self
            .connection
            .as_ref()
            .and_then(|c| SourceStamp::from_connection(c).ok())
            .as_ref()
            == Some(&stamp)
        {
            return self.foreground_slot();
        }
        let workspace = self.fleet_state.workspaces.get(&stamp.key())?;
        workspace
            .selected
            .as_ref()
            .map(|id| (workspace.profile.clone(), id.clone()))
    }
    pub(in crate::app) fn park_source_workspace(&mut self) -> Result<bool, String> {
        let Ok(binding) = self.current_source_binding() else {
            return Ok(false);
        };
        self.flush_window_editors()?;
        let resolver = binding
            .resolver()
            .cloned()
            .or_else(|| match binding.connection() {
                Connection::Saved { id, revision, .. } => self.window_route_resolver(id, revision),
                _ => None,
            })
            .or_else(|| self.fleet.resolver(binding.stamp().version()));
        let retained = self
            .fleet
            .retain_source(binding.clone(), resolver.clone())?;
        self.fleet_state.workspaces.insert(
            binding.stamp().key(),
            Workspace {
                binding,
                retained,
                _resolver: resolver,
                epoch: self.connection_epoch,
                profile: self.profile.clone(),
                selected: self.selected_session.clone(),
                tabs: self.tabs.clone(),
                sessions: self.sessions.clone(),
                messages: self.messages.clone(),
                bot_empty: self.bot_empty,
                truncated: self.history_truncated,
                all_profiles: self.sidebar_state.prefs.all_profiles,
            },
        );
        Ok(true)
    }
    pub(in crate::app) fn restore_source_workspace(&mut self, requested: Option<&str>) {
        let Some(stamp) = self
            .connection
            .as_ref()
            .and_then(|c| SourceStamp::from_connection(c).ok())
        else {
            return;
        };
        let Some(workspace) = self.fleet_state.workspaces.remove(&stamp.key()) else {
            return;
        };
        if !workspace.valid() {
            self.retire_source_runtime(workspace.epoch);
            return;
        }
        if let Err(error) = self.fleet.adopt_source(&workspace.retained) {
            self.retire_source_runtime(workspace.epoch);
            self.client_error = Some(error);
            return;
        }
        self.connection_epoch = workspace.epoch;
        self.tabs = workspace.tabs;
        self.sidebar_state.prefs.all_profiles = workspace.all_profiles;
        let profile = requested.unwrap_or(&workspace.profile);
        self.profile = profile.to_owned();
        if profile == workspace.profile {
            self.selected_session = workspace.selected;
            self.sessions = workspace.sessions;
            self.messages = workspace.messages;
            self.bot_empty = workspace.bot_empty;
            self.history_truncated = workspace.truncated;
        }
    }
    pub(in crate::app) fn bind_source_conversation(
        &mut self,
        origin: &DraftScope,
        stored: &str,
        active: bool,
    ) {
        let destination = DraftScope {
            session: Some(stored.into()),
            ..origin.clone()
        };
        let tab = ConversationTab {
            profile: origin.profile.clone(),
            session: stored.into(),
            title: self
                .conversation
                .view(&destination)
                .filter(|view| !view.title.is_empty())
                .map(|view| view.title.clone())
                .unwrap_or_else(|| "New conversation".into()),
            bot: self
                .conversation
                .view(&destination)
                .is_some_and(|view| view.canonical_bot),
        };
        if origin.connection == self.connection_epoch {
            if !self
                .tabs
                .iter()
                .any(|t| t.profile == origin.profile && t.session == stored)
            {
                self.tabs.push(tab);
            }
            if active {
                self.selected_session = Some(stored.into());
                self.history_loading = false;
                self.bot_empty = false;
            }
        } else if let Some(workspace) = self
            .fleet_state
            .workspaces
            .values_mut()
            .find(|workspace| workspace.epoch == origin.connection && workspace.valid())
        {
            if !workspace
                .tabs
                .iter()
                .any(|t| t.profile == origin.profile && t.session == stored)
            {
                workspace.tabs.push(tab);
            }
            // Bind only the parked foreground input which produced this receipt.
            // A new fresh input later gets its own editor; None is never a global alias.
            if workspace.profile == origin.profile && workspace.selected == origin.session {
                workspace.selected = Some(stored.into());
                workspace.bot_empty = false;
            }
        }
    }
    pub(in crate::app) fn update_parked_conversation(&mut self, scope: &DraftScope) {
        let Some(view) = self.conversation.view(scope) else {
            return;
        };
        if let Some(workspace) = self.fleet_state.workspaces.values_mut().find(|workspace| {
            workspace.epoch == scope.connection
                && workspace.profile == scope.profile
                && workspace.selected == scope.session
                && workspace.valid()
        }) {
            workspace.messages = view.messages.clone();
            workspace.truncated = view.truncated;
        }
    }
    pub(in crate::app) fn retained_source_epoch(&self, epoch: u64) -> bool {
        epoch == self.connection_epoch
            || self
                .fleet_state
                .workspaces
                .values()
                .any(|w| w.epoch == epoch && w.valid())
    }
    fn retire_source_runtime(&mut self, epoch: u64) {
        self.preview.close_connection(epoch);
        self.media_players.close_connection(epoch);
        self.chat.close_connection(epoch);
        self.subagents.close_connection(epoch);
        self.conversation.close_connection(epoch);
        self.audio.close_connection(epoch);
        self.terminal.close_connection(epoch);
        self.background.retire_connection(epoch);
    }
    pub(super) fn retire_expired_workspaces(&mut self) -> bool {
        let expired = self
            .fleet_state
            .workspaces
            .iter()
            .filter_map(|(key, w)| (!w.valid()).then_some((key.clone(), w.epoch)))
            .collect::<Vec<_>>();
        for (key, epoch) in &expired {
            self.fleet_state.workspaces.remove(key);
            self.retire_source_runtime(*epoch);
        }
        !expired.is_empty()
    }
}

#[cfg(test)]
#[path = "workspaces/tests.rs"]
mod tests;
