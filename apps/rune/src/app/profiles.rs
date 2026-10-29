//! Profile changes retire runtimes before a folder may disappear or be renamed.
use super::*;
impl Viewer {
    pub fn prepare_profile_mutation(&mut self, profile: &str) -> bool {
        let blocked = self
            .conversation
            .blocks_profile_retirement(self.composer_scope().connection, Some(profile))
            || self
                .chat
                .blocks_profile_retirement(self.composer_scope().connection, Some(profile))
            || self.review.blocks_owner_change()
            || self.operations.blocks_owner_change()
            || self.files.blocks_owner_change()
            || self.sidebar_state.blocks_owner_change()
            || self.settings.blocks_owner_change()
            || self.capabilities.busy()
            || self.messaging.blocks_owner_change()
            || self.artifacts.blocks_owner_change();
        if blocked {
            self.client_error=Some("Finish the pending settings or session change before renaming or deleting this bot".into());
            return false;
        }
        self.retired_profiles
            .insert((self.connection_epoch, profile.into()));
        self.chat.close_profile(self.connection_epoch, profile);
        self.subagents.close_profile(self.connection_epoch, profile);
        self.conversation
            .close_profile(self.connection_epoch, profile);
        self.terminal.close_profile(self.connection_epoch, profile);
        self.chat_attached
            .retain(|scope| scope.connection != self.connection_epoch || scope.profile != profile);
        if self
            .audio_owner
            .as_ref()
            .is_some_and(|scope| scope.profile == profile)
        {
            let owner = self.audio_owner.take().unwrap();
            self.audio.cancel(&owner);
            self.wake_enabled = false;
        }
        self.plugin_hosts.close();
        true
    }
    pub fn update_profiles_from_bots(&mut self, rows: &[serde_json::Value]) {
        if !self.fixture {
            let effects = self.identities.seed(rows);
            self.page_requests(effects);
        }
        let parsed = rows
            .iter()
            .map(|row| {
                let name = row["name"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .ok_or("The bot roster contains an invalid identity")?
                    .to_owned();
                let meta = row.get("ui_meta").and_then(|m| m.get("hermes-bots"));
                let label = crate::gateway::profile_label(row, meta, &name);
                let canonical_session =
                    if let Some(chat) = row.get("canonical_session").filter(|v| !v.is_null()) {
                        Some(CanonicalSession {
                            id: chat["id"]
                                .as_str()
                                .ok_or("The bot roster contains an invalid Bot Chat")?
                                .into(),
                            resolved_id: chat["resolved_id"]
                                .as_str()
                                .ok_or("The bot roster contains an invalid Bot Chat tip")?
                                .into(),
                            preview: chat["preview"].as_str().unwrap_or("").into(),
                        })
                    } else {
                        None
                    };
                Ok(Profile {
                    name,
                    label,
                    description: row["description"].as_str().unwrap_or("").into(),
                    canonical_session,
                })
            })
            .collect::<Result<Vec<_>, &str>>();
        match parsed {
            Ok(profiles) => {
                self.profiles = profiles;
                for profile in &self.profiles {
                    self.conversation
                        .profile_readback(self.connection_epoch, &profile.name);
                    self.retired_profiles
                        .remove(&(self.connection_epoch, profile.name.clone()));
                }
                if !self.profiles.iter().any(|p| p.name == self.profile) {
                    if let Some(next) = self
                        .profiles
                        .iter()
                        .find(|p| p.name == "default")
                        .or(self.profiles.first())
                        .map(|p| p.name.clone())
                    {
                        self.select_profile(next);
                    }
                }
            }
            Err(error) => self.client_error = Some(error.into()),
        }
    }
    pub fn profile_renamed(&mut self, from: &str, to: &str) {
        for tab in &mut self.tabs {
            if tab.profile == from {
                tab.profile = to.into();
            }
        }
        self.composer
            .rename_profile(self.connection_epoch, from, to);
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self.profile_preferences.rename(&connection, from, to) {
                self.client_error = Some(error);
            }
        }
        if self.profile == from {
            self.profile = to.into();
            self.intent += 1;
            self.history_loading = false;
            self.chat_attached.retain(|scope| scope.profile != from);
        }
        self.retired_profiles
            .remove(&(self.connection_epoch, to.into()));
    }
    pub fn profile_deleted(&mut self, name: &str) {
        self.tabs.retain(|t| t.profile != name);
        self.composer.delete_profile(self.connection_epoch, name);
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self.profile_preferences.remove(&connection, name) {
                self.client_error = Some(error);
            }
        }
        if self.profile == name {
            self.selected_session = None;
            self.messages.clear();
            self.history_loading = false;
            self.intent += 1;
        }
    }
}

impl Viewer {
    pub fn reconcile_bots(&mut self) {
        for change in self.bots.take_changes() {
            match change {
                crate::pages::bots::ProfileChange::Renamed { from, to } => {
                    self.profile_renamed(&from, &to)
                }
                crate::pages::bots::ProfileChange::Deleted(name) => self.profile_deleted(&name),
                crate::pages::bots::ProfileChange::Created(name) => {
                    self.composer.delete_profile(self.connection_epoch, &name);
                    self.tabs.retain(|t| t.profile != name);
                    if let Some(connection) = self.connection.clone() {
                        if let Err(error) = self.profile_preferences.remove(&connection, &name) {
                            self.client_error = Some(error);
                        }
                    }
                    self.client_notice = Some(format!("Bot {name} created"))
                }
                crate::pages::bots::ProfileChange::Updated(_) => {}
                crate::pages::bots::ProfileChange::Reconcile(name) => {
                    self.retired_profiles.insert((self.connection_epoch, name));
                }
            }
        }
        if let Some((profile, overlay)) = self.bots.take_imported_overlay() {
            self.import_profile_overlay(&profile, overlay);
        }
        if let Some(created) = self.bots.take_new_bot_chat() {
            if !self.fixture
                && self.route == crate::pages::Route::Bots
                && self.bots.selected.as_ref() == Some(&created.name)
                && self.bots.editor_epoch == created.editor_epoch
            {
                self.new_bot_launch = Some((created, self.connection_epoch));
            }
        }
        if self.bots.roster_revision != self.bots_revision {
            self.bots_revision = self.bots.roster_revision;
            let rows = self.bots.roster.clone();
            self.update_profiles_from_bots(&rows);
        }
        self.try_launch_new_bot();
    }
}

impl Viewer {
    fn try_launch_new_bot(&mut self) {
        let Some((created, epoch)) = self.new_bot_launch.clone() else {
            return;
        };
        if self.fixture
            || epoch != self.connection_epoch
            || self.route != crate::pages::Route::Bots
            || self.bots.selected.as_ref() != Some(&created.name)
            || self.bots.editor_epoch != created.editor_epoch
        {
            self.new_bot_launch = None;
            return;
        }
        if !self.profiles.iter().any(|p| p.name == created.name) {
            return;
        }
        if !self.allow_owner_change(&created.name) {
            return;
        }
        self.new_bot_launch = None;
        self.open_bot(created.name.clone());
        if created.kickoff {
            self.new_bot_intro = Some((created.name, self.connection_epoch, self.intent));
        }
    }
    pub fn poll_new_bot_intro(&mut self) {
        self.try_launch_new_bot();
        let Some((name, epoch, intent)) = self.new_bot_intro.clone() else {
            return;
        };
        if self.fixture
            || epoch != self.connection_epoch
            || self.profile != name
            || self.intent != intent
        {
            self.new_bot_intro = None;
            return;
        }
        if self.history_loading || self.pending_bot.is_some() {
            return;
        }
        self.new_bot_intro = None;
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let scope = self.composer_scope();
        let input = crate::conversation::SendInput {
            canonical_bot: true,
            text: "Hey, tell me about yourself!".into(),
            ..Default::default()
        };
        if let Err(error) = self.conversation.send(connection, scope, input) {
            self.compose_error = Some(error);
        }
    }
}
