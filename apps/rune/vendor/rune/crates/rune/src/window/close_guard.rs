//! Local close can retain a dirty window; global Quit remains explicitly separate.
use super::*;
impl<S: 'static> Host<S> {
    pub(super) fn allow_local_close(&mut self) -> bool {
        let Some(guard) = self.on_close_requested else {
            return true;
        };
        let mut allowed = true;
        self.rt.update(|state, tx| allowed = guard(state, tx));
        self.clipboard.flush(&mut self.rt);
        if !allowed {
            if self.rt.shell_request == Some(Shell::Close) {
                self.rt.shell_request = None;
            }
            if let Some(window) = &self.win {
                window.window.request_redraw();
            }
        }
        allowed
    }
}
