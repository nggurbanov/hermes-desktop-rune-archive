use super::{Patch, Preferences, Reply, bank};
use rune::Waker;
use std::{
    collections::{HashMap, VecDeque},
    io::Cursor,
    path::PathBuf,
    sync::{
        OnceLock, Weak,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    time::{Duration, Instant},
};
pub(super) enum Command {
    Claim {
        client: u64,
        id: u64,
        key: String,
        speech: bool,
    },
    Register {
        client: u64,
        key: String,
        prefs: Preferences,
        path: Option<PathBuf>,
        writable: bool,
        replies: Sender<Reply>,
        waker: Waker,
        alive: Weak<()>,
    },
    Patch {
        client: u64,
        patch: Patch,
        persist: bool,
    },
    Cue {
        client: u64,
        key: String,
        preview: Option<u8>,
    },
    Unregister(u64),
}
struct Client {
    device: String,
    replies: Sender<Reply>,
    waker: Waker,
    alive: Weak<()>,
}
impl Client {
    fn send(&self, reply: Reply) {
        if self.replies.send(reply).is_ok() {
            (self.waker)();
        }
    }
}
struct Device {
    prefs: Preferences,
    path: Option<PathBuf>,
    writable: bool,
}
struct Pending {
    client: u64,
    preview: Option<u8>,
}
pub(super) trait Player {
    fn play(&mut self, preset: u8) -> Result<(), String>;
}
struct NativePlayer {
    stream: Option<rodio::OutputStream>,
}
impl Player for NativePlayer {
    fn play(&mut self, preset: u8) -> Result<(), String> {
        let Some(bytes) = bank::bytes(preset) else {
            return Ok(());
        };
        if self.stream.is_none() {
            self.stream = Some(rodio::OutputStreamBuilder::open_default_stream().map_err(
                |_| "No audio output is available. Choose speakers in system settings.",
            )?);
        }
        let source = rodio::Decoder::try_from(Cursor::new(bytes))
            .map_err(|_| "The completion sound could not be decoded")?;
        let sink = rodio::Sink::connect_new(self.stream.as_ref().unwrap().mixer());
        sink.append(source);
        sink.detach();
        Ok(())
    }
}
static SHARED: OnceLock<SyncSender<Command>> = OnceLock::new();
pub(super) fn shared() -> SyncSender<Command> {
    SHARED
        .get_or_init(|| {
            let (send, receive) = mpsc::sync_channel(32);
            std::thread::Builder::new()
                .name("hermes-completion-sounds".into())
                .spawn(move || run(receive, NativePlayer { stream: None }))
                .expect("completion sound worker");
            send
        })
        .clone()
}
pub(super) struct Mixer<P> {
    player: P,
    clients: HashMap<u64, Client>,
    devices: HashMap<String, Device>,
    claims: HashMap<String, Instant>,
    pending: VecDeque<Pending>,
}
impl<P: Player> Mixer<P> {
    pub fn new(player: P) -> Self {
        Self {
            player,
            clients: HashMap::new(),
            devices: HashMap::new(),
            claims: HashMap::new(),
            pending: VecDeque::new(),
        }
    }
    pub fn command(&mut self, command: Command, now: Instant) {
        match command {
            Command::Register {
                client,
                key,
                prefs,
                path,
                writable,
                replies,
                waker,
                alive,
            } => {
                self.devices.entry(key.clone()).or_insert(Device {
                    prefs,
                    path,
                    writable,
                });
                self.devices.get_mut(&key).unwrap().writable &= writable;
                self.clients.insert(
                    client,
                    Client {
                        device: key.clone(),
                        replies,
                        waker,
                        alive,
                    },
                );
                let prefs = self.devices[&key].prefs.clone();
                self.clients[&client].send(Reply::Preferences {prefs,saved:false,result:if self.devices[&key].writable{Ok(())}else{Err("Sound preferences are read-only because their saved file could not be loaded.".into())}});
            }
            Command::Claim {
                client,
                id,
                key,
                speech,
            } => {
                if !self.clients.contains_key(&client) {
                    return;
                }
                let owns = if speech {
                    self.claim(&key, now, Duration::from_secs(600))
                } else {
                    // The native mixer owns SDK completion audio; the child must not play WebAudio.
                    self.cue(client, key, None, now);
                    false
                };
                self.clients[&client].send(Reply::Claimed { id, owns });
            }
            Command::Unregister(client) => {
                self.clients.remove(&client);
            }
            Command::Patch {
                client,
                patch,
                persist,
            } => self.patch(client, patch, persist),
            Command::Cue {
                client,
                key,
                preview,
            } => self.cue(client, key, preview, now),
        }
    }
    fn patch(&mut self, client: u64, patch: Patch, persist: bool) {
        let Some(key) = self.clients.get(&client).map(|c| c.device.clone()) else {
            return;
        };
        let device = self.devices.get_mut(&key).unwrap();
        match patch {
            Patch::Preset(value) => device.prefs.preset = if value <= 14 { value } else { 1 },
            Patch::Mute(value) => device.prefs.muted = value,
        }
        let result = if persist {
            super::save(device.path.as_deref(), device.writable, &device.prefs)
        } else {
            Ok(())
        };
        for client in self.clients.values().filter(|c| c.device == key) {
            client.send(Reply::Preferences {
                prefs: device.prefs.clone(),
                saved: persist,
                result: result.clone(),
            });
        }
    }
    fn cue(&mut self, client: u64, key: String, preview: Option<u8>, now: Instant) {
        let Some(source) = self.clients.get(&client) else {
            return;
        };
        let prefs = &self.devices[&source.device].prefs;
        if preview == Some(0) || preview.is_none() && (prefs.muted || prefs.preset == 0) {
            return;
        }
        if preview.is_none() && !self.claim(&key, now, Duration::from_secs(1)) {
            return;
        }
        if self.pending.len() >= 32 {
            self.clients[&client].send(Reply::Played {
                preview: preview.is_some(),
                result: Err("The sound queue is busy; this cue was not played.".into()),
            });
            return;
        }
        self.pending.push_back(Pending { client, preview });
    }
    fn claim(&mut self, key: &str, now: Instant, ttl: Duration) -> bool {
        self.claims.retain(|_, until| now < *until);
        if self.claims.contains_key(key) {
            return false;
        }
        self.claims.insert(key.into(), now + ttl);
        true
    }
    pub fn flush(&mut self) {
        self.clients
            .retain(|_, client| client.alive.strong_count() > 0);
        while let Some(pending) = self.pending.pop_front() {
            let Some(source) = self.clients.get(&pending.client) else {
                continue;
            };
            let prefs = &self.devices[&source.device].prefs;
            // Claim and playback are separate steps; mute and the chosen preset can change between them.
            let preset = match pending.preview {
                Some(value) => value,
                None if !prefs.muted => prefs.preset,
                None => continue,
            };
            if preset == 0 {
                continue;
            }
            let result = self.player.play(preset);
            source.send(Reply::Played {
                preview: pending.preview.is_some(),
                result,
            });
        }
        self.devices
            .retain(|key, _| self.clients.values().any(|client| &client.device == key));
    }
}
fn run<P: Player>(receive: Receiver<Command>, player: P) {
    let mut mixer = Mixer::new(player);
    while let Ok(command) = receive.recv() {
        mixer.command(command, Instant::now());
        for command in receive.try_iter().take(63) {
            mixer.command(command, Instant::now());
        }
        // Apply queued preference changes before playback, without requiring an idle gap.
        mixer.flush();
    }
}
