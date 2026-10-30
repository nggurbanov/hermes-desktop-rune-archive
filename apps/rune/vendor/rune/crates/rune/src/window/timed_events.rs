//! Timer-driven native lifecycle changes publish through the same host boundary.
use super::*;
impl<S: 'static> Host<S> {
    pub(super) fn timed_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        #[cfg(target_os = "macos")]
        if let Some(shadow) = self.win.as_ref().and_then(|w| w.shadow.as_ref()) { shadow.turn(); }
        self.meter.turn(matches!(cause, StartCause::ResumeTimeReached { .. } | StartCause::Poll), crate::surface_stats::slept(cause));
        if matches!(cause, StartCause::ResumeTimeReached { .. }) && let Some(w) = &self.win { w.cursor.reassert(); }
        if let Some(w) = &mut self.win && let Some((retry, until)) = w.first_frame.deadline && retry <= Instant::now() {
            if until <= Instant::now() {
                let notified = w.first_frame.notified;
                if w.first_frame.expire() {
                    self.show_cached();
                    self.shell(event_loop);
                    if let Some(w) = &self.win { w.window.request_redraw(); }
                    self.publish();
                } else if !notified {
                    *self.shared.window_error.lock().unwrap() = Some("launcher first frame was not ready within two seconds".into());
                }
            } else {
                // Bounded retries while the frame prepares, whatever stops each one (occlusion,
                // no drawable). A retry left due would turn the loop at once until it gives up.
                w.first_frame.deadline = Some((Instant::now() + Duration::from_millis(16), until));
                w.window.request_redraw();
            }
        }
        let launcher = self.launcher();
        if let Some(w) = &mut self.win && w.retry.is_some_and(|retry| retry <= Instant::now()) {
            w.retry = None;
            // A missed frame marked it occluded; look at the surface again.
            w.occluded = false;
            if launcher || w.visible { w.window.request_redraw(); }
        }
        if let Some(w) = &mut self.win && !w.visible && w.hidden_pace.take_due(Instant::now()) {
            w.window.request_redraw();
        }
        if matches!(cause, StartCause::ResumeTimeReached { .. } | StartCause::Poll)
            && timer_deadline(&self.rt).is_some_and(|deadline| deadline <= Instant::now())
        {
            let hidden = self.win.as_ref().is_none_or(|w| !w.visible);
            if hidden && !(launcher && self.hidden_launcher_renders()) {
                // Hidden or windowless: poll and due timers run, and the view waits for the
                // frame that shows the window. A launcher's cached frame follows, paced.
                let changed = self.rt.hidden_wake();
                self.clipboard.flush(&mut self.rt);
                self.shell(event_loop);
                if changed && launcher { self.refresh_hidden(); }
            // A redraw while the window backs off would draw nothing and leave the timer due.
            } else if self.win.as_ref().is_some_and(|w| (launcher || (w.visible && !w.occluded)) && !w.minimized()
                && !w.backing_off(Instant::now())) {
                // A resting window polls first and draws only for what that changed.
                let rests = self.idle && !composing(self.comp.as_deref()) && self.win.as_ref().is_some_and(Win::rests);
                let frame = match rests.then(|| self.rt.resting_wake()).flatten() {
                    Some(stale) => {
                        self.clipboard.flush(&mut self.rt);
                        self.shell(event_loop);
                        // A close the poll asked for may have hidden the window.
                        let frame = (stale || self.rt.stale()) && self.win.as_ref().is_some_and(|w| w.visible);
                        self.rt.resting_wake_frame(frame);
                        frame
                    }
                    None => true,
                };
                if frame && let Some(w) = &mut self.win {
                    w.quiet = true;
                    w.window.request_redraw();
                }
            } else {
                covered_step(&mut self.rt);
                self.clipboard.flush(&mut self.rt);
                self.shell(event_loop);
            }
        }
    }

}
