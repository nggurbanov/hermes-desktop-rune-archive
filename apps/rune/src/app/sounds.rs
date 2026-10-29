use super::*;
impl Viewer {
    pub fn sound_action(&mut self, action: crate::sounds::Action) {
        self.sounds.act(action, !self.fixture);
        self.notifications.muted = self.sounds.muted();
    }
    pub(super) fn poll_sounds(&mut self) -> bool {
        let changed = self.sounds.poll();
        self.notifications.muted = self.sounds.muted();
        changed
    }
    pub(super) fn bind_sounds(&mut self, connection: &Connection) {
        match crate::client::connection_key(connection) {
            Ok(principal) => self.sounds.bind_principal(principal),
            Err(error) => {
                self.sounds.bind_principal(String::new());
                self.client_error = Some(error);
            }
        }
    }
}
