//! Track connection workers without extending their normal window lifetimes.
use super::{Resolver, Work};
use std::{
    sync::{
        Arc, Mutex, OnceLock, Weak,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

pub(super) struct Owner {
    pub send: Sender<Work>,
}
struct Worker {
    owner: Weak<Owner>,
    finished: Receiver<()>,
}
static WORKERS: OnceLock<Mutex<Vec<Worker>>> = OnceLock::new();
fn workers() -> &'static Mutex<Vec<Worker>> {
    WORKERS.get_or_init(|| Mutex::new(Vec::new()))
}
pub(super) fn track(resolver: &Resolver, finished: Receiver<()>) {
    let mut workers = workers()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    workers.retain(|worker| !matches!(worker.finished.try_recv(), Ok(())));
    workers.push(Worker {
        owner: Arc::downgrade(&resolver.identity),
        finished,
    });
}
/// Called after the desktop event loop returns, never on a peer-window close.
pub(crate) fn drain(timeout: Duration) -> Result<(), String> {
    let pending = std::mem::take(
        &mut *workers()
            .lock()
            .map_err(|_| "Connection shutdown tracking failed")?,
    );
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or("Connection shutdown timeout is invalid")?;
    for worker in &pending {
        if let Some(owner) = worker.owner.upgrade() {
            let _ = owner.send.send(Work::Shutdown);
        }
    }
    for worker in pending {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match worker.finished.recv_timeout(remaining) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("A connection worker stopped without confirming cleanup".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err("Connection cleanup did not finish before shutdown".into());
            }
        }
    }
    Ok(())
}
