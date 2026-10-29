//! Device-owned native notifications. Backend events never write agent configuration.
mod event;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(all(test, target_os = "linux"))]
mod linux_tests;
mod policy;
mod registry;
mod service;
#[cfg(test)]
mod tests;
#[cfg(target_os = "linux")]
mod transport;
mod types;

use crate::composer::DraftScope;
pub use policy::Preferences;
use rune::Waker;
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Instant,
};
pub use types::{Action, Activation, Availability, Button, Kind, Notice, PromptAction};
use types::{Reply, Request};

static NEXT_CLIENT: AtomicU64 = AtomicU64::new(1);
pub struct Notifications {
    pub prefs: Preferences,
    pub revision: u64,
    pub availability: Availability,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub testing: bool,
    pub muted: bool,
    client: u64,
    principal: String,
    sequence: u64,
    test_id: Option<u64>,
    policy: policy::Policy,
    visible: bool,
    focused: bool,
    path: Option<PathBuf>,
    writable: bool,
    started: bool,
    alive: Arc<()>,
    waker: Option<Waker>,
    sender: mpsc::Sender<Reply>,
    replies: mpsc::Receiver<Reply>,
}
impl Default for Notifications {
    fn default() -> Self {
        Self::new()
    }
}
impl Notifications {
    pub fn new() -> Self {
        let (sender, replies) = mpsc::channel();
        Self {
            revision: 0,
            prefs: Preferences::default(),
            availability: Availability::NotStarted,
            error: None,
            notice: None,
            testing: false,
            muted: false,
            client: NEXT_CLIENT.fetch_add(1, Ordering::Relaxed),
            principal: String::new(),
            sequence: 0,
            test_id: None,
            policy: policy::Policy::default(),
            visible: true,
            focused: true,
            path: None,
            writable: true,
            started: false,
            alive: Arc::new(()),
            waker: None,
            sender,
            replies,
        }
    }
    pub fn load(&mut self) {
        match crate::client::state_dir() {
            Ok(path) => self.load_at(path.join("notifications.json")),
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
                Ok(prefs) if prefs.version == 1 => self.prefs = prefs,
                _ => {
                    self.error = Some("Saved notification preferences are invalid or newer than this app; the file is preserved.".into());
                    self.writable = false;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                self.error = Some(
                    "Notification preferences could not be read; the file is preserved.".into(),
                );
                self.writable = false;
            }
        }
    }
    pub fn start(&mut self, waker: Waker, primary: bool) {
        if self.started {
            return;
        }
        self.waker = Some(waker.clone());
        self.started = service::send(service::Command::Register {
            client: self.client,
            replies: self.sender.clone(),
            waker: Some(waker),
            primary,
            alive: Arc::downgrade(&self.alive),
        })
        .is_ok();
        self.availability = Availability::Checking;
        if !self.started
            || service::send(service::Command::Probe {
                client: self.client,
            })
            .is_err()
        {
            self.busy();
        }
    }
    pub fn bind_connection(&mut self, principal: String) {
        self.principal = principal;
    }
    pub fn set_active(&mut self, scope: DraftScope) {
        self.policy.active = Some(scope);
    }
    pub fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
        self.policy.focused = focused && self.visible;
    }
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.policy.focused = self.focused && visible;
    }
    pub fn stream_opened(&mut self, scope: &DraftScope) {
        self.policy.opened(scope, Instant::now());
    }
    pub fn event(
        &mut self,
        scope: &DraftScope,
        kind: &str,
        payload: &Value,
        runtime: &str,
        replay: bool,
        title: &str,
    ) {
        if replay {
            return;
        }
        if let Some(notice) = event::notice(scope, kind, payload, runtime, title) {
            self.dispatch(notice);
        }
    }
    /// Also used by client features (background tasks/plugins); no arbitrary command is executable from a body click.
    pub fn dispatch(&mut self, mut notice: Notice) -> bool {
        if !self.started
            || self.principal.is_empty()
            || !self.policy.allows(&self.prefs, &notice, Instant::now())
        {
            return false;
        }
        notice.silent |= self.muted;
        self.submit(notice, false)
    }
    fn submit(&mut self, notice: Notice, test: bool) -> bool {
        self.sequence += 1;
        if test {
            self.test_id = Some(self.sequence);
            self.testing = true;
        }
        if service::send(service::Command::Show(Request {
            id: self.sequence,
            client: self.client,
            principal: self.principal.clone(),
            notice,
            test,
        }))
        .is_err()
        {
            self.testing = false;
            self.test_id = None;
            self.busy();
            return false;
        }
        true
    }
    pub fn act(&mut self, action: Action, persist: bool) {
        self.revision += 1;
        match action {
            Action::Test => {
                self.test();
                return;
            }
            Action::Refresh => {
                if !self.started {
                    if let Some(waker) = self.waker.clone() {
                        self.start(waker, true);
                    }
                } else {
                    self.availability = Availability::Checking;
                    if service::send(service::Command::Probe {
                        client: self.client,
                    })
                    .is_err()
                    {
                        self.busy();
                    }
                }
                return;
            }
            Action::Enabled(value) => self.prefs.enabled = value,
            Action::Kind(kind, value) => {
                self.prefs.kinds.insert(kind.id().into(), value);
            }
        }
        if persist {
            self.save();
        }
    }
    fn test(&mut self) {
        if !self.started {
            self.notice = Some("Native delivery is unavailable in this preview.".into());
            return;
        }
        if self.testing {
            return;
        }
        let scope = self.policy.active.clone().unwrap_or(DraftScope {
            connection: 0,
            profile: "default".into(),
            session: None,
        });
        self.notice = None;
        let _ = self.submit(
            Notice {
                kind: Kind::Plugin,
                scope,
                runtime: String::new(),
                title: "Hermes notifications".into(),
                body: "Native notifications are working. Click to return to Hermes.".into(),
                global: true,
                tag: Some("notification-test".into()),
                silent: self.muted,
                buttons: vec![],
            },
            true,
        );
    }
    fn busy(&mut self) {
        let message = "The native notification queue is full; this notification was not submitted."
            .to_string();
        self.availability = Availability::Unavailable(message.clone());
        self.notice = Some(message);
        self.revision += 1;
    }
    fn save(&mut self) {
        let result = (|| {
            if !self.writable {
                return Err("Notification preferences are read-only because their saved file could not be loaded.".into());
            }
            let path = self
                .path
                .as_ref()
                .ok_or("The client state directory is unavailable")?;
            let bytes = serde_json::to_vec_pretty(&self.prefs)
                .map_err(|_| "Notification preferences could not be encoded")?;
            crate::client::private_write(path, &bytes)
        })();
        match result {
            Ok(()) => {
                self.error = None;
                self.notice = Some("Saved on this device".into());
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub fn poll(&mut self) -> Vec<Activation> {
        let mut activations = vec![];
        for reply in self.replies.try_iter() {
            self.revision += 1;
            match reply {
                Reply::Availability(value) => self.availability = value,
                Reply::Activated(value) => activations.push(value),
                Reply::Delivered { id, result } => {
                    if self.test_id == Some(id) {
                        self.testing = false;
                        self.test_id = None;
                        self.notice = Some(match &result {
                            Ok(()) => "The desktop service accepted the test notification.".into(),
                            Err(error) => error.clone(),
                        });
                    }
                    if let Err(error) = result {
                        self.availability = Availability::Unavailable(error);
                    }
                }
            }
        }
        activations
    }
}
impl Drop for Notifications {
    fn drop(&mut self) {
        if self.started {
            let _ = service::send(service::Command::Unregister {
                client: self.client,
            });
        }
    }
}
