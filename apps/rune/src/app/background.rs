use super::*;
impl Viewer {
    pub fn trigger_background(&mut self, scope: DraftScope, runtime: String) {
        if self.fixture
            || self
                .retired_profiles
                .contains(&(scope.connection, scope.profile.clone()))
        {
            return;
        }
        if self
            .conversation
            .view(&scope)
            .is_none_or(|view| view.runtime != runtime)
        {
            return;
        }
        if let Some(binding) = self.binding_for_source_epoch(scope.connection) {
            self.background
                .bind(binding, scope.clone(), runtime.clone());
            self.background.trigger(&scope, &runtime);
        }
    }
    pub(super) fn poll_background(&mut self, now: f64, tx: &mut Tx) -> bool {
        if self.fixture {
            return false;
        }
        let retained = self.retained_source_epochs();
        self.background.retain_connections(&retained);
        for (connection, profile) in &self.retired_profiles {
            self.background.retire_profile(*connection, profile);
        }
        let scope = self.composer_scope();
        if let (Some(binding), Some(view)) = (
            self.binding_for_source_epoch(scope.connection),
            self.conversation.view(&scope),
        ) {
            self.background
                .bind(binding, scope.clone(), view.runtime.clone());
        }
        let visible = self.route == crate::pages::Route::Chat
            && self.pane_shown("workspace")
            && self
                .native_window
                .as_ref()
                .is_none_or(|window| window.is_visible());
        let before = self.background.revision;
        let updates = self.background.poll(now, &scope, visible);
        if let Some(delay) = self.background.next_wake_after(now, &scope, visible) {
            tx.wake_after(delay);
        }
        let changed = before != self.background.revision || !updates.is_empty();
        self.notifications.set_active(scope);
        for update in updates {
            self.apply_background_update(update);
        }
        changed
    }
    fn apply_background_update(&mut self, update: crate::background::Update) {
        match update {
            crate::background::Update::Snapshot { scope, value } => {
                self.terminal.sync_processes(&scope, &value)
            }
            crate::background::Update::Finished {
                scope,
                runtime,
                title,
                body,
            } => {
                // The device broker still belongs to the foreground principal.
                // Parked-source delivery needs a source-qualified notification owner.
                if scope.connection == self.connection_epoch {
                    self.notifications.dispatch(crate::notifications::Notice {
                        kind: crate::notifications::Kind::BackgroundDone,
                        scope,
                        runtime,
                        title,
                        body,
                        global: false,
                        tag: None,
                        silent: false,
                        buttons: vec![],
                        plugin: None,
                    });
                }
            }
            crate::background::Update::Changed => {}
            crate::background::Update::RuntimeGone { scope, runtime } => {
                if self
                    .conversation
                    .view(&scope)
                    .is_some_and(|view| view.runtime == runtime)
                    && let Some(binding) = self.binding_for_source_epoch(scope.connection)
                    && let Err(error) = self
                        .conversation
                        .reconnect(binding.connection().clone(), &scope)
                {
                    if scope.connection == self.connection_epoch {
                        self.client_error = Some(error);
                    } else {
                        self.chat.presentation(scope).error = Some(error);
                    }
                }
            }
        }
    }
    pub fn stop_background(&mut self, owner: DraftScope, id: String) {
        if self.fixture
            || owner.connection != self.connection_epoch
            || self
                .retired_profiles
                .contains(&(owner.connection, owner.profile.clone()))
        {
            return;
        }
        self.background.stop(&owner, &id);
    }
    pub fn dismiss_background(&mut self, owner: DraftScope, id: String) {
        self.background.dismiss(&owner, &id);
    }
    pub fn toggle_background_output(&mut self, owner: DraftScope, id: String) {
        self.background.toggle_output(owner, id);
    }
    pub(super) fn rewind_background(&mut self, owner: &DraftScope) {
        if !self.fixture
            && owner.connection == self.connection_epoch
            && !self
                .retired_profiles
                .contains(&(owner.connection, owner.profile.clone()))
        {
            self.background.rewind(owner);
        }
    }
}

#[cfg(test)]
#[path = "background/tests.rs"]
mod tests;
