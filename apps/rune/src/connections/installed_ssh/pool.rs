//! Coalesce simultaneous attempts by the full effective SSH identity and owner_id.
use super::{
    Cancellation, Ssh,
    engine::{self, CredentialStore, Ready},
};
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, Weak},
    time::Duration,
};

#[derive(Default, Clone)]
pub(crate) struct Pool(Arc<Mutex<HashMap<String, Entry>>>);
#[derive(Default)]
struct Entry {
    ready: Weak<Mutex<engine::Live>>,
    flight: Option<Arc<Flight>>,
}
struct Flight {
    state: Mutex<State>,
    changed: Condvar,
    cancel: Cancellation,
}
struct State {
    waiters: usize,
    result: Option<Result<Ready, String>>,
}
impl Pool {
    #[cfg(all(test, unix))]
    pub(crate) fn connect(
        &self,
        ssh: Ssh,
        owner_id: &str,
        cancel: &Cancellation,
    ) -> Result<Ready, String> {
        self.connect_with_store(ssh, owner_id, engine::test_store(), cancel)
    }
    pub(crate) fn connect_with_store(
        &self,
        ssh: Ssh,
        owner_id: &str,
        store: Arc<dyn CredentialStore>,
        cancel: &Cancellation,
    ) -> Result<Ready, String> {
        if cancel.cancelled() {
            return Err("SSH operation cancelled".into());
        }
        let effective = ssh.effective_identity(cancel)?;
        let key = engine::ownership(&ssh, &effective, owner_id);
        let mut entries = self.0.lock().map_err(|_| "SSH connection pool failed")?;
        let entry = entries.entry(key.clone()).or_default();
        // A returned Ready's resource is the only strong owner of its lifetime.
        if let Some(live) = entry.ready.upgrade() {
            if let Ok(ready) = engine::from_live(live) {
                if cancel.cancelled() {
                    return Err("SSH operation cancelled".into());
                }
                return Ok(ready);
            }
            entry.ready = Weak::new();
        }
        let (flight, start) = if let Some(flight) = &entry.flight {
            (flight.clone(), false)
        } else {
            let flight = Arc::new(Flight {
                state: Mutex::new(State {
                    waiters: 0,
                    result: None,
                }),
                changed: Condvar::new(),
                cancel: Cancellation::default(),
            });
            entry.flight = Some(flight.clone());
            (flight, true)
        };
        let mut state = flight
            .state
            .lock()
            .map_err(|_| "SSH connection pool failed")?;
        state.waiters += 1;
        drop(entries);
        if start {
            let table = self.0.clone();
            let runner = flight.clone();
            let owner_id = owner_id.to_string();
            std::thread::spawn(move || {
                let result =
                    engine::connect_prepared(ssh, &owner_id, store, &effective, &runner.cancel);
                if let Ok(mut entries) = table.lock() {
                    if let Some(entry) = entries.get_mut(&key) {
                        if entry
                            .flight
                            .as_ref()
                            .is_some_and(|f| Arc::ptr_eq(f, &runner))
                        {
                            if let Ok(ready) = &result {
                                if let Some(live) = &ready.resource.0 {
                                    entry.ready = Arc::downgrade(live);
                                }
                            }
                            entry.flight = None;
                        }
                    }
                }
                if let Ok(mut state) = runner.state.lock() {
                    state.result = Some(result);
                    runner.changed.notify_all();
                }
            });
        }
        loop {
            if cancel.cancelled() {
                state.waiters -= 1;
                if state.waiters == 0 {
                    if state.result.is_none() {
                        flight.cancel.cancel();
                    }
                    while state.result.is_none() {
                        state = flight
                            .changed
                            .wait(state)
                            .map_err(|_| "SSH connection pool failed")?;
                    }
                    // Dropping the final Ready waits for its exact owned cleanup.
                    drop(state.result.take());
                }
                return Err("SSH operation cancelled".into());
            }
            if let Some(result) = &state.result {
                let result = result.clone();
                state.waiters -= 1;
                if state.waiters == 0 {
                    drop(state.result.take());
                }
                return result;
            }
            state = flight
                .changed
                .wait_timeout(state, Duration::from_millis(50))
                .map_err(|_| "SSH connection pool failed")?
                .0;
        }
    }
}
