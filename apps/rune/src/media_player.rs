//! Owned native audio previews. Metadata reads never open an output device.
mod budget;
mod decode;
#[cfg(test)]
mod malformed_tests;
mod output;
mod save;
mod tempo;
#[cfg(test)]
pub(crate) mod tests;
mod worker;
use crate::{
    composer::DraftScope,
    gateway::{Connection, stream::Cancel},
    preview::Target,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
pub type PlayerId = u64;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Loading,
    Paused,
    Buffering,
    Playing,
    Ended,
    Error,
}
#[derive(Clone)]
pub struct Snapshot {
    pub id: PlayerId,
    pub target: Target,
    pub state: State,
    pub paused: bool,
    pub duration: Option<f64>,
    pub position: f64,
    pub seekable: bool,
    pub rate: f32,
    pub volume: f32,
    pub muted: bool,
    pub error: Option<String>,
    pub saving: bool,
    pub save_progress: u64,
    pub save_total: Option<u64>,
    pub save_error: Option<String>,
    pub notice: Option<String>,
}
#[derive(Clone, Debug)]
pub enum Action {
    Play,
    Pause,
    Seek(f64),
    Rate(f32),
    Volume(f32),
    Mute(bool),
    Retry,
    SaveOriginal(std::path::PathBuf),
    SaveDialogError,
}
#[derive(Clone)]
struct Controls {
    paused: bool,
    seek: Option<f64>,
    rate: f32,
    volume: f32,
    muted: bool,
    generation: u64,
    revision: u64,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            paused: true,
            seek: None,
            rate: 1.,
            volume: 1.,
            muted: false,
            generation: 0,
            revision: 0,
        }
    }
}
struct Shared {
    controls: Mutex<Controls>,
    wake: Condvar,
    snapshot: Mutex<Snapshot>,
    revision: AtomicU64,
    observed: AtomicBool,
    cancel: Cancel,
    source_cancel: Mutex<Option<Cancel>>,
    save_cancel: Mutex<Option<Cancel>>,
    ring: Mutex<output::Ring>,
    waker: rune::Waker,
}
impl Shared {
    fn update(&self, change: impl FnOnce(&mut Snapshot)) {
        let mut snapshot = self.snapshot.lock().unwrap();
        change(&mut snapshot);
        self.revision.fetch_add(1, Ordering::Release);
        drop(snapshot);
        (self.waker)();
    }
    fn playback_update(&self, generation: u64, change: impl FnOnce(&mut Snapshot)) {
        let controls = self.controls.lock().unwrap();
        if controls.generation == generation && !self.cancel.cancelled() {
            self.update(change);
        }
    }
    fn playback_progress(&self, generation: u64, change: impl FnOnce(&mut Snapshot)) {
        let controls = self.controls.lock().unwrap();
        if controls.generation != generation || self.cancel.cancelled() {
            return;
        }
        let mut snapshot = self.snapshot.lock().unwrap();
        let old = snapshot.state;
        change(&mut snapshot);
        let notify = old != snapshot.state || self.observed.load(Ordering::Acquire);
        self.revision.fetch_add(1, Ordering::Release);
        drop(snapshot);
        if notify {
            (self.waker)();
        }
    }
    fn restart(&self, controls: &Controls) {
        if let Some(source) = self.source_cancel.lock().unwrap().as_ref() {
            source.cancel();
        }
        let mut ring = self.ring.lock().unwrap();
        let rate = ring.rate;
        let channels = ring.channels;
        ring.reset(controls.seek.unwrap_or(0.), rate, channels);
        ring.paused = true;
        drop(ring);
        self.update(|s| {
            s.state = State::Loading;
            s.position = controls.seek.unwrap_or(0.);
            s.rate = controls.rate;
            s.paused = controls.paused;
            s.error = None;
        });
    }
    fn close(&self) {
        self.cancel.cancel();
        if let Some(source) = self.source_cancel.lock().unwrap().as_ref() {
            source.cancel();
        }
        if let Some(save) = self.save_cancel.lock().unwrap().as_ref() {
            save.cancel();
        }
        self.ring.lock().unwrap().closed = true;
        self.wake.notify_all();
    }
}
struct Player {
    connection: Connection,
    shared: Arc<Shared>,
    snapshot: Snapshot,
    revision: u64,
}
pub struct MediaPlayers {
    players: HashMap<PlayerId, Player>,
    sequence: u64,
    waker: Option<rune::Waker>,
}
impl Default for MediaPlayers {
    fn default() -> Self {
        Self::new()
    }
}
impl MediaPlayers {
    pub fn new() -> Self {
        Self {
            players: HashMap::new(),
            sequence: 0,
            waker: None,
        }
    }
    pub fn init(&mut self, waker: rune::Waker) {
        self.waker = Some(waker);
    }
    pub fn open(&mut self, connection: Connection, target: Target) -> Result<PlayerId, String> {
        if target.kind != crate::preview::Kind::Audio {
            return Err("This player opens audio files".into());
        }
        let waker = self
            .waker
            .clone()
            .ok_or("Audio previews are not initialized")?;
        if self.players.len() >= 8 {
            return Err("Close an audio preview before opening another".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Audio preview identifier exhausted")?;
        let id = self.sequence;
        let snapshot = Snapshot {
            id,
            target: target.clone(),
            state: State::Loading,
            paused: true,
            duration: None,
            position: 0.,
            seekable: false,
            rate: 1.,
            volume: 1.,
            muted: false,
            error: None,
            saving: false,
            save_progress: 0,
            save_total: None,
            save_error: None,
            notice: None,
        };
        let shared = Arc::new(Shared {
            controls: Mutex::new(Controls::default()),
            wake: Condvar::new(),
            snapshot: Mutex::new(snapshot.clone()),
            revision: AtomicU64::new(1),
            observed: AtomicBool::new(true),
            cancel: Cancel::default(),
            source_cancel: Mutex::new(None),
            save_cancel: Mutex::new(None),
            ring: Mutex::new(output::Ring::new()),
            waker,
        });
        let child = shared.clone();
        let worker_connection = connection.clone();
        std::thread::Builder::new()
            .name("hermes-audio-preview".into())
            .spawn(move || worker::run(worker_connection, target, child))
            .map_err(|_| "The audio preview worker could not start")?;
        self.players.insert(
            id,
            Player {
                connection,
                shared,
                snapshot,
                revision: 0,
            },
        );
        Ok(id)
    }
    pub fn observe(&mut self, id: PlayerId, observed: bool) {
        if let Some(player) = self.players.get(&id) {
            player.shared.observed.store(observed, Ordering::Release);
        }
    }
    pub fn get(&self, id: PlayerId) -> Option<&Snapshot> {
        self.players.get(&id).map(|p| &p.snapshot)
    }
    pub fn act(&mut self, id: PlayerId, action: Action) {
        let Some(player) = self.players.get(&id) else {
            return;
        };
        let shared = &player.shared;
        if let Action::SaveOriginal(destination) = action {
            if shared.snapshot.lock().unwrap().saving {
                return;
            }
            shared.update(|s| {
                s.saving = true;
                s.save_error = None;
                s.notice = None;
                s.save_progress = 0;
                s.save_total = None;
            });
            let connection = player.connection.clone();
            let target = player.snapshot.target.clone();
            let shared = shared.clone();
            std::thread::spawn(move || {
                let result = save::original(connection, target, &destination, shared.clone());
                shared.update(|s| {
                    s.saving = false;
                    match result {
                        Ok(()) => s.notice = Some("Original saved".into()),
                        Err(error) => s.save_error = Some(error),
                    }
                });
            });
            return;
        }
        if matches!(action, Action::SaveDialogError) {
            shared.update(|s| s.save_error = Some("The save dialog is unavailable".into()));
            return;
        }
        let mut c = shared.controls.lock().unwrap();
        c.revision = c.revision.wrapping_add(1);
        match action {
            Action::Play => {
                c.paused = false;
                shared.update(|s| s.paused = false);
            }
            Action::Pause => {
                c.paused = true;
                shared.ring.lock().unwrap().paused = true;
                shared.update(|s| {
                    s.paused = true;
                    if matches!(s.state, State::Playing | State::Buffering) {
                        s.state = State::Paused;
                    }
                });
            }
            Action::Seek(value) if value.is_finite() && player.snapshot.seekable => {
                c.seek = Some(
                    value
                        .max(0.)
                        .min(player.snapshot.duration.unwrap_or(f64::MAX)),
                );
                c.generation = c.generation.wrapping_add(1);
                shared.restart(&c);
            }
            Action::Rate(value) if value.is_finite() && (0.25..=4.).contains(&value) => {
                if c.rate != value {
                    c.rate = value;
                    if player.snapshot.seekable {
                        c.seek = Some(shared.ring.lock().unwrap().position);
                        c.generation = c.generation.wrapping_add(1);
                        shared.restart(&c);
                    }
                }
            }
            Action::Volume(value) if value.is_finite() => c.volume = value.clamp(0., 1.),
            Action::Mute(value) => c.muted = value,
            Action::Retry => {
                c.generation = c.generation.wrapping_add(1);
                c.paused = true;
                c.seek = None;
                shared.restart(&c);
            }
            _ => {}
        }
        drop(c);
        shared.wake.notify_all();
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for player in self.players.values_mut() {
            let revision = player.shared.revision.load(Ordering::Acquire);
            if revision != player.revision {
                player.snapshot = player.shared.snapshot.lock().unwrap().clone();
                player.revision = revision;
                changed = true;
            }
        }
        changed
    }
    pub fn close(&mut self, id: PlayerId) {
        if let Some(player) = self.players.remove(&id) {
            player.shared.close();
        }
    }
    pub fn close_owner(&mut self, owner: &DraftScope) {
        self.close_where(|s| &s.target.owner == owner);
    }
    pub fn close_profile(&mut self, connection: u64, profile: &str) {
        self.close_where(|s| {
            s.target.owner.connection == connection && s.target.owner.profile == profile
        });
    }
    pub fn close_connection(&mut self, connection: u64) {
        self.close_where(|s| s.target.owner.connection == connection);
    }
    fn close_where(&mut self, predicate: impl Fn(&Snapshot) -> bool) {
        let ids: Vec<_> = self
            .players
            .iter()
            .filter_map(|(id, p)| predicate(&p.snapshot).then_some(*id))
            .collect();
        for id in ids {
            self.close(id);
        }
    }
}
impl Drop for MediaPlayers {
    fn drop(&mut self) {
        for player in self.players.values() {
            player.shared.close();
        }
    }
}
