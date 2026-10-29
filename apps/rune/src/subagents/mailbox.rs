//! A lagging spectator retains one latest display snapshot, never a transcript backlog.
use super::worker::Projection;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[cfg(test)]
use std::{sync::mpsc::RecvTimeoutError, time::Duration};
struct State {
    value: Option<Projection>,
    closed: bool,
}
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    receiver_alive: AtomicBool,
}
pub(super) struct Sender {
    shared: Arc<Shared>,
}
pub(super) struct Receiver {
    shared: Arc<Shared>,
}
pub(super) fn channel() -> (Sender, Receiver) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            value: None,
            closed: false,
        }),
        ready: Condvar::new(),
        receiver_alive: AtomicBool::new(true),
    });
    (
        Sender {
            shared: shared.clone(),
        },
        Receiver { shared },
    )
}
impl Sender {
    pub fn send(&self, value: Projection) -> bool {
        if !self.shared.receiver_alive.load(Ordering::Acquire) {
            return false;
        }
        let old = {
            let mut state = self.shared.state.lock().unwrap();
            state.value.replace(value)
        };
        // Free the old transcript outside the lock, so the UI can take the latest immediately.
        drop(old);
        self.shared.ready.notify_one();
        true
    }
}
impl Drop for Sender {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock().unwrap();
        state.closed = true;
        self.shared.ready.notify_one();
    }
}
impl Receiver {
    pub fn try_iter(&self) -> std::option::IntoIter<Projection> {
        self.shared.state.lock().unwrap().value.take().into_iter()
    }
    #[cfg(test)]
    pub fn recv_timeout(&self, duration: Duration) -> Result<Projection, RecvTimeoutError> {
        let state = self.shared.state.lock().unwrap();
        let (mut state, _) = self
            .shared
            .ready
            .wait_timeout_while(state, duration, |state| {
                state.value.is_none() && !state.closed
            })
            .unwrap();
        match state.value.take() {
            Some(value) => Ok(value),
            None if state.closed => Err(RecvTimeoutError::Disconnected),
            None => Err(RecvTimeoutError::Timeout),
        }
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        self.shared.receiver_alive.store(false, Ordering::Release);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::HistoryMessage;
    #[test]
    fn cosmetic_backlog_coalesces_and_the_last_terminal_or_error_snapshot_is_never_lost() {
        for error in [None, Some("The child failed".to_owned())] {
            let (sender, receiver) = channel();
            for index in 0..64 {
                assert!(sender.send(Projection {
                    messages: vec![HistoryMessage::text(
                        "assistant",
                        &format!("Progress {index}")
                    )],
                    running: true,
                    ..Default::default()
                }));
            }
            assert!(sender.send(Projection {
                messages: vec![HistoryMessage::text("assistant", "Final child response")],
                running: false,
                error: error.clone(),
                connected: true,
                ..Default::default()
            }));
            let frames: Vec<_> = receiver.try_iter().collect();
            assert_eq!(
                frames.len(),
                1,
                "A lagging window must retain one display snapshot"
            );
            assert_eq!(frames[0].messages[0].content, "Final child response");
            assert!(!frames[0].running);
            assert_eq!(frames[0].error, error);
        }
    }
    #[test]
    fn detach_stops_publication_and_worker_exit_releases_a_waiting_reader() {
        let (sender, receiver) = channel();
        drop(receiver);
        assert!(!sender.send(Projection::default()));
        let (sender, receiver) = channel();
        drop(sender);
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Err(RecvTimeoutError::Disconnected)
        ));
    }
}
