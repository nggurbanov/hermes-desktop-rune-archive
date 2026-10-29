//! Original Hermes completion cues, rendered offline and played by a shared native worker.
mod bank;
#[cfg(test)]
mod tests;
mod worker;
use crate::composer::DraftScope;
pub use bank::PRESETS;
use rune::Waker;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};
static CLIENT: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub preset: u8,
    pub muted: bool,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            preset: 1,
            muted: false,
            other: BTreeMap::new(),
        }
    }
}
impl Preferences {
    fn normalize(&mut self) {
        if self.preset > 14 {
            self.preset = 1;
        }
    }
}
#[derive(Clone, Copy)]
pub enum Action {
    Preset(u8),
    Mute(bool),
    Preview,
}
#[derive(Clone, Copy)]
pub(super) enum Patch {
    Preset(u8),
    Mute(bool),
}
pub(super) enum Reply {
    Claimed {
        id: u64,
        owns: bool,
    },
    Preferences {
        prefs: Preferences,
        saved: bool,
        result: Result<(), String>,
    },
    Played {
        preview: bool,
        result: Result<(), String>,
    },
}
pub struct CompletionSounds {
    pub prefs: Preferences,
    pub open: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub previewing: bool,
    pub revision: u64,
    pub initialized: bool,
    claims: Vec<(u64, bool)>,
    next_claim: u64,
    client: u64,
    principal: String,
    path: Option<PathBuf>,
    writable: bool,
    alive: Arc<()>,
    sender: mpsc::Sender<Reply>,
    replies: mpsc::Receiver<Reply>,
    native: Option<mpsc::SyncSender<worker::Command>>,
}
impl Default for CompletionSounds {
    fn default() -> Self {
        Self::new()
    }
}
impl CompletionSounds {
    pub fn new() -> Self {
        let (sender, replies) = mpsc::channel();
        Self {
            prefs: Preferences::default(),
            open: false,
            error: None,
            notice: None,
            previewing: false,
            revision: 0,
            initialized: false,
            claims: vec![],
            next_claim: 0,
            client: CLIENT.fetch_add(1, Ordering::Relaxed),
            principal: String::new(),
            path: None,
            writable: true,
            alive: Arc::new(()),
            sender,
            replies,
            native: None,
        }
    }
    pub fn load(&mut self) {
        match crate::client::state_dir() {
            Ok(path) => self.load_at(path.join("sounds.json")),
            Err(error) => {
                self.error = Some(error);
                self.writable = false;
            }
        }
    }
    fn load_at(&mut self, path: PathBuf) {
        self.path = Some(path.clone());
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(mut prefs) if prefs.version == 1 => {
                    prefs.normalize();
                    self.prefs = prefs;
                }
                _ => {
                    self.error=Some("Saved sound preferences are invalid or newer than this app; the file is preserved.".into());
                    self.writable = false;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                self.error = Some(
                    "Saved sound preferences could not be read; the file is preserved.".into(),
                );
                self.writable = false;
            }
        }
    }
    pub fn init(&mut self, waker: Waker) {
        if self.initialized {
            return;
        }
        self.native.get_or_insert_with(worker::shared);
        let command = worker::Command::Register {
            client: self.client,
            key: self
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("unsaved:{}", self.client)),
            prefs: self.prefs.clone(),
            path: self.path.clone(),
            writable: self.writable,
            replies: self.sender.clone(),
            waker,
            alive: Arc::downgrade(&self.alive),
        };
        self.initialized = self.enqueue(command);
    }
    pub fn bind_principal(&mut self, principal: String) {
        self.principal = principal;
    }
    pub fn muted(&self) -> bool {
        self.prefs.muted
    }
    pub fn complete(&mut self, scope: &DraftScope, runtime: &str) -> bool {
        if !self.initialized
            || self.prefs.muted
            || self.prefs.preset == 0
            || self.principal.is_empty()
            || runtime.is_empty()
        {
            return false;
        }
        let key =
            serde_json::to_string(&(&self.principal, &scope.profile, format!("sound:{runtime}")))
                .expect("sound owner key");
        self.enqueue(worker::Command::Cue {
            client: self.client,
            key,
            preview: None,
        })
    }
    pub fn claim_ambient(
        &mut self,
        principal: &str,
        profile: &str,
        key: &str,
    ) -> Result<u64, String> {
        if !self.initialized {
            return Err("The native sound service is unavailable".into());
        }
        if principal.is_empty() || profile.is_empty() || key.is_empty() || key.len() > 512 {
            return Err("The ambient cue key is invalid".into());
        }
        if !key.starts_with("sound:") && !key.starts_with("speak:") {
            return Err("This ambient cue namespace is unavailable".into());
        }
        let speech = key.starts_with("speak:");
        let canonical = serde_json::to_string(&(principal, profile, key))
            .map_err(|_| "The ambient cue key is invalid")?;
        self.next_claim += 1;
        let id = self.next_claim;
        if !self.enqueue(worker::Command::Claim {
            client: self.client,
            id,
            key: canonical,
            speech,
        }) {
            return Err("The ambient ownership request could not be queued".into());
        }
        Ok(id)
    }
    pub fn take_claims(&mut self) -> Vec<(u64, bool)> {
        std::mem::take(&mut self.claims)
    }
    pub fn act(&mut self, action: Action, persist: bool) {
        self.revision += 1;
        self.notice = None;
        let previous = (self.prefs.preset, self.prefs.muted);
        let patch = match action {
            Action::Preview => {
                if self.prefs.preset == 0 {
                    return;
                }
                if !self.initialized {
                    self.notice = Some("Audio previews are unavailable in this scene.".into());
                    return;
                }
                if self.previewing {
                    return;
                }
                self.previewing = true;
                if !self.enqueue(worker::Command::Cue {
                    client: self.client,
                    key: String::new(),
                    preview: Some(self.prefs.preset),
                }) {
                    self.previewing = false;
                }
                return;
            }
            Action::Preset(value) => {
                self.prefs.preset = if value <= 14 { value } else { 1 };
                Patch::Preset(self.prefs.preset)
            }
            Action::Mute(value) => {
                self.prefs.muted = value;
                Patch::Mute(value)
            }
        };
        if self.initialized {
            if !self.enqueue(worker::Command::Patch {
                client: self.client,
                patch,
                persist,
            }) {
                self.prefs.preset = previous.0;
                self.prefs.muted = previous.1;
            }
        } else if persist {
            self.error = save(self.path.as_deref(), self.writable, &self.prefs).err();
        }
    }
    fn enqueue(&mut self, command: worker::Command) -> bool {
        if self
            .native
            .as_ref()
            .is_some_and(|sender| sender.try_send(command).is_ok())
        {
            return true;
        }
        self.error = Some("The sound queue is busy; this request was not submitted.".into());
        self.revision += 1;
        false
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for reply in self.replies.try_iter() {
            changed = true;
            self.revision += 1;
            match reply {
                Reply::Claimed { id, owns } => self.claims.push((id, owns)),
                Reply::Preferences {
                    prefs,
                    saved,
                    result,
                } => {
                    self.prefs = prefs;
                    match result {
                        Ok(()) => {
                            self.error = None;
                            if saved {
                                self.notice = Some("Saved on this device".into());
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Reply::Played { preview, result } => {
                    if preview {
                        self.previewing = false;
                    }
                    match result {
                        Ok(()) => {
                            self.error = None;
                            if preview {
                                self.notice = Some("Preview is playing.".into());
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }
        changed
    }
}
fn save(path: Option<&std::path::Path>, writable: bool, prefs: &Preferences) -> Result<(), String> {
    if !writable {
        return Err(
            "Sound preferences are read-only because their saved file could not be loaded.".into(),
        );
    }
    let bytes =
        serde_json::to_vec_pretty(prefs).map_err(|_| "Sound preferences could not be encoded")?;
    crate::client::private_write(
        path.ok_or("The client state directory is unavailable")?,
        &bytes,
    )
}
impl Drop for CompletionSounds {
    fn drop(&mut self) {
        if let Some(sender) = &self.native {
            let _ = sender.try_send(worker::Command::Unregister(self.client));
        }
    }
}
