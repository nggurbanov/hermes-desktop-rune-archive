//! Device routing edits remain owned by this window across agent navigation.
use super::*;

impl crate::view::legacy_settings::Host for Viewer {
    fn legacy_settings(&self) -> &crate::legacy_settings::Settings {
        &self.legacy_settings
    }
    fn legacy_settings_mut(&mut self) -> &mut crate::legacy_settings::Settings {
        &mut self.legacy_settings
    }
    fn legacy_settings_action(&mut self, action: crate::legacy_settings::Action) {
        self.legacy_settings.observe_source(self.connection_epoch);
        self.legacy_settings.action(action, &self.connections);
    }
    fn legacy_settings_connect(&mut self, generation: u64) {
        if self.fixture
            || !self.legacy_routing_settings
            || self.route != crate::pages::Route::Settings
            || self.settings.section != "connections"
            || self.legacy_settings.loading()
            || self.legacy_settings.form.as_ref().is_none_or(|form| {
                form.generation != generation
                    || form.mode == crate::legacy_settings::FormMode::Cloud
            })
        {
            return;
        }
        if let Err(error) = self.legacy_settings.can_close() {
            self.legacy_settings.error = Some(error);
            return;
        }
        let profile = match &self.legacy_settings.scope {
            crate::connections::legacy::Scope::Global => self.profile.clone(),
            crate::connections::legacy::Scope::Profile(profile) => profile.clone(),
        };
        if let Err(error) = self.connect_legacy_settings(profile) {
            self.legacy_settings.error = Some(error);
        }
    }
}
impl Viewer {
    pub fn set_legacy_routing_settings(&mut self, open: bool) {
        if !open {
            if let Err(error) = self.legacy_settings.can_close() {
                self.client_error = Some(error);
                return;
            }
        }
        if !open {
            self.legacy_settings.cancel_login();
        }
        self.legacy_routing_settings = open;
        if open && !self.fixture {
            self.legacy_settings.ensure_loaded(&self.connections);
        }
    }
    pub(super) fn poll_legacy_settings(&mut self, tx: &mut Tx) -> bool {
        if self.fixture {
            return false;
        }
        self.legacy_settings.observe_source(self.connection_epoch);
        if self.legacy_routing_settings
            && self.route == crate::pages::Route::Settings
            && self.settings.section == "connections"
        {
            self.legacy_settings.ensure_loaded(&self.connections);
        }
        if !self.legacy_routing_settings
            || self.route != crate::pages::Route::Settings
            || self.settings.section != "connections"
        {
            self.legacy_settings.cancel_login();
        }
        let changed = self.legacy_settings.poll(&self.connections);
        if self.legacy_settings.loading()
            || self.legacy_settings.pending()
            || self.legacy_settings.login_work_pending()
        {
            tx.wake_after(std::time::Duration::from_millis(40));
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connections::legacy::testing::Rig,
        legacy_settings::{Action, Change, FormMode},
        view::legacy_settings::Host,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn connect_requires_the_current_clean_saved_form_before_queuing_a_route() {
        let Rig { connections, state } = Rig::new();
        let mut s = Viewer::empty();
        s.connections = connections;
        s.routing.test_environment = Some(crate::connections::legacy::Environment::captured(
            None, None,
        ));
        s.route = crate::pages::Route::Settings;
        s.settings.section = "connections".into();
        s.legacy_routing_settings = true;
        s.connection_local_home.replace_text(
            state
                .path()
                .parent()
                .unwrap()
                .join("private-agent")
                .to_str()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while !s.connections.registry_loaded() {
            s.connections.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        s.legacy_settings.ensure_loaded(&s.connections);
        while s.legacy_settings.loading() {
            s.legacy_settings.poll(&s.connections);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let generation = s.legacy_settings.form.as_ref().unwrap().generation;
        Host::legacy_settings_connect(&mut s, generation.wrapping_add(1));
        assert!(!s.device_route_pending());
        s.legacy_settings.action(
            Action::Form {
                generation,
                change: Change::Mode(FormMode::Url),
            },
            &s.connections,
        );
        assert!(s.legacy_settings.dirty());
        Host::legacy_settings_connect(&mut s, generation);
        assert!(!s.device_route_pending());
        assert!(s.legacy_settings.error.is_some());
        s.legacy_settings.action(Action::Discard, &s.connections);
        while s.legacy_settings.loading() {
            s.legacy_settings.poll(&s.connections);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let generation = s.legacy_settings.form.as_ref().unwrap().generation;
        Host::legacy_settings_connect(&mut s, generation);
        assert!(s.device_route_pending());
        assert!(s.legacy_settings.error.is_none());
        s.cancel_device_route();
    }
    #[test]
    fn switching_source_synchronously_retires_the_authorization_action() {
        use crate::{connections::legacy::AuthMode, legacy_settings::Field};
        let rig = Rig::new();
        let mut viewer = Viewer::empty();
        viewer.connections = rig.connections;
        viewer.legacy_settings.ensure_loaded(&viewer.connections);
        let deadline = Instant::now() + Duration::from_secs(3);
        while viewer.legacy_settings.loading() {
            viewer.legacy_settings.poll(&viewer.connections);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let generation = viewer.legacy_settings.form.as_ref().unwrap().generation;
        for change in [Change::Mode(FormMode::Url), Change::Auth(AuthMode::Oauth)] {
            viewer
                .legacy_settings
                .action(Action::Form { generation, change }, &viewer.connections);
        }
        viewer
            .legacy_settings
            .edit_mut(generation, Field::Url)
            .replace_text("http://127.0.0.1:9");
        viewer.legacy_settings.action(
            Action::Form {
                generation,
                change: Change::Save,
            },
            &viewer.connections,
        );
        while viewer.legacy_settings.pending() {
            viewer.legacy_settings.poll(&viewer.connections);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let (attempt, send) = crate::connections::LoginAttempt::test_channel();
        assert!(viewer.legacy_settings.test_start_attempt(attempt));
        send.send(crate::connections::LoginEvent::Authorize(
            "http://127.0.0.1/authorize".into(),
        ))
        .ok();
        viewer.legacy_settings.poll(&viewer.connections);
        assert!(viewer.legacy_settings.authorization_url().is_some());
        let before = viewer.connection_epoch;
        viewer.switching_window_source();
        assert_eq!(viewer.connection_epoch, before);
        assert!(viewer.legacy_settings.authorization_url().is_none());
    }
}
