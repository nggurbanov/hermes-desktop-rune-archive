//! Private portal fixture. Production access remains gated until the network boundary is enforced.
#[cfg(all(test, target_os = "linux"))]
mod tests;

use super::cloud::{Discovery, Endpoint, Error};
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use std::{
    collections::BTreeMap,
    sync::mpsc::{self, Receiver, Sender},
};
use tao::window::Window;
use url::Url;
#[cfg(target_os = "linux")]
use {
    gtk::prelude::*,
    tao::platform::unix::WindowExtUnix,
    webkit2gtk::{CookieManagerExt, WebContextExt, WebViewExt},
};

const ACCESS: &[&str] = &[
    "nas-session",
    "__Host-privy-token",
    "__Secure-privy-token",
    "privy-token",
];
const REFRESH: &[&str] = &["nas-refresh", "privy-session", "privy-refresh-token"];
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Login,
    Renew { force: bool },
    Discover,
}
pub enum Progress {
    Pending,
    Complete,
    Discovery(Discovery),
    Cancelled,
    TimedOut,
}
/// Opaque receipt, scoped to this view's current operation. Results are consumed once.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Receipt {
    source: uuid::Uuid,
    generation: u64,
}
#[cfg(target_os = "linux")]
type Cookies = Vec<(String, String)>;
#[cfg(target_os = "linux")]
enum Reply {
    Cookies(Receipt, Result<Cookies, Error>),
    Discovery(Receipt, Result<Discovery, Error>),
}
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Initial,
    Polling,
    Scoped,
    DiscoveryCookies,
    DiscoveryWorker,
}
#[cfg(target_os = "linux")]
struct Pending {
    receipt: Receipt,
    operation: Operation,
    stage: Stage,
    previous: BTreeMap<String, String>,
    deadline: Option<Instant>,
    next_poll: Instant,
    interval: Duration,
    waiting: bool,
}

/// A source owns one ephemeral jar; each operation gets a disposable view and receipt.
#[cfg(target_os = "linux")]
pub struct PortalView {
    context: Option<webkit2gtk::WebContext>,
    container: gtk::Box,
    view: Option<webkit2gtk::WebView>,
    endpoint: Endpoint,
    root: String,
    discovery: String,
    selected_org: Option<String>,
    pending: Option<Pending>,
    source: uuid::Uuid,
    serial: u64,
    retired: bool,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
}
#[cfg(target_os = "linux")]
impl PortalView {
    pub fn private_fixture(window: &Window, endpoint: Endpoint, base: &str) -> Result<Self, Error> {
        let url = Url::parse(base).map_err(|_| Error::InvalidBase)?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || url.origin().ascii_serialization() != endpoint.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !endpoint.access_headers().is_empty()
        {
            return Err(Error::EmbeddedRequired);
        }
        let path = url.path().trim_end_matches('/').to_owned();
        let mut root = url.clone();
        root.set_path(&format!("{path}/"));
        let mut discovery = url;
        discovery.set_path(&format!("{path}/api/agents"));
        let container = window.default_vbox().ok_or(Error::Transport)?.clone();
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            context: Some(webkit2gtk::WebContext::new_ephemeral()),
            container,
            view: None,
            endpoint,
            root: root.to_string(),
            discovery: discovery.to_string(),
            selected_org: None,
            pending: None,
            source: uuid::Uuid::new_v4(),
            serial: 0,
            retired: false,
            tx,
            rx,
        })
    }
    fn request_cookies(&mut self, url: &str) -> Result<(), Error> {
        let manager = self
            .context
            .as_ref()
            .ok_or(Error::Cancelled)?
            .cookie_manager()
            .ok_or(Error::Transport)?;
        let pending = self.pending.as_mut().ok_or(Error::Cancelled)?;
        pending.waiting = true;
        let receipt = pending.receipt;
        let tx = self.tx.clone();
        manager.cookies(url, None::<&gtk::gio::Cancellable>, move |result| {
            let result = result
                .map(|cookies| {
                    cookies
                        .into_iter()
                        .filter_map(|mut c| {
                            let name = c.name()?.to_string();
                            let value = c.value()?.to_string();
                            (!value.is_empty()).then_some((name, value))
                        })
                        .collect()
                })
                .map_err(|_| Error::Transport);
            let _ = tx.send(Reply::Cookies(receipt, result));
        });
        Ok(())
    }
    fn close_view(&mut self) {
        if let Some(view) = self.view.take() {
            view.stop_loading();
            self.container.remove(&view);
        }
    }
    /// Starts without pumping GTK or waiting for I/O. Drive GTK normally and poll the receipt.
    pub fn begin(&mut self, operation: Operation, now: Instant) -> Result<Receipt, Error> {
        if self.pending.is_some() || self.retired {
            return Err(Error::Cancelled);
        }
        self.serial = self.serial.checked_add(1).ok_or(Error::Cancelled)?;
        let receipt = Receipt {
            source: self.source,
            generation: self.serial,
        };
        let interval = Duration::from_millis(if operation == Operation::Login {
            750
        } else {
            500
        });
        self.pending = Some(Pending {
            receipt,
            operation,
            stage: if operation == Operation::Discover {
                Stage::DiscoveryCookies
            } else {
                Stage::Initial
            },
            previous: BTreeMap::new(),
            deadline: matches!(operation, Operation::Renew { .. })
                .then_some(now + Duration::from_secs(12)),
            next_poll: now,
            interval,
            waiting: false,
        });
        let url = if operation == Operation::Discover {
            self.discovery.clone()
        } else {
            self.root.clone()
        };
        if let Err(e) = self.request_cookies(&url) {
            self.cancel();
            return Err(e);
        }
        Ok(receipt)
    }
    fn cookies_ready(&mut self, cookies: Cookies, now: Instant) -> Result<Option<Progress>, Error> {
        let pending = self.pending.as_mut().ok_or(Error::Cancelled)?;
        pending.waiting = false;
        match pending.stage {
            Stage::Initial => {
                if matches!(pending.operation, Operation::Renew { .. })
                    && !cookies
                        .iter()
                        .any(|(k, _)| ACCESS.contains(&k.as_str()) || REFRESH.contains(&k.as_str()))
                {
                    return Err(Error::Reauthenticate);
                }
                if pending.operation == (Operation::Renew { force: false })
                    && cookies.iter().any(|(k, _)| ACCESS.contains(&k.as_str()))
                {
                    return Ok(Some(Progress::Complete));
                }
                pending.previous = cookies
                    .into_iter()
                    .filter(|(k, _)| ACCESS.contains(&k.as_str()))
                    .collect();
                pending.stage = Stage::Polling;
                let view = webkit2gtk::WebView::with_context(
                    self.context.as_ref().ok_or(Error::Cancelled)?,
                );
                view.connect_create(|_, _| None);
                self.container.pack_start(&view, true, true, 0);
                view.show();
                view.load_uri(&self.root);
                self.view = Some(view);
            }
            Stage::Polling => {
                pending.next_poll = now + pending.interval;
                if cookies
                    .iter()
                    .filter(|(k, _)| ACCESS.contains(&k.as_str()))
                    .any(|(k, v)| pending.previous.get(k) != Some(v))
                {
                    pending.stage = Stage::Scoped;
                    let url = self.discovery.clone();
                    self.request_cookies(&url)?;
                }
            }
            Stage::Scoped => {
                return Ok(Some(Progress::Complete));
            }
            Stage::DiscoveryCookies => {
                if !cookies.iter().any(|(k, _)| ACCESS.contains(&k.as_str())) {
                    return Err(Error::Reauthenticate);
                }
                let cookie = cookies
                    .into_iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                let endpoint = self.endpoint.clone();
                let org = self.selected_org.clone();
                let tx = self.tx.clone();
                let receipt = pending.receipt;
                pending.stage = Stage::DiscoveryWorker;
                pending.waiting = true;
                std::thread::spawn(move || {
                    let result = super::cloud::discover(
                        &endpoint,
                        &super::cloud::Session::Cookie(cookie),
                        org.as_deref(),
                    );
                    let _ = tx.send(Reply::Discovery(receipt, result));
                });
            }
            Stage::DiscoveryWorker => return Err(Error::Cancelled),
        }
        Ok(None)
    }
    /// Returns Pending immediately; only the matching receipt can consume a completion.
    pub fn poll(&mut self, receipt: Receipt, now: Instant) -> Result<Progress, Error> {
        let Some(pending) = self.pending.as_ref() else {
            return Ok(Progress::Cancelled);
        };
        if pending.receipt != receipt {
            return Ok(Progress::Cancelled);
        }
        if pending.deadline.is_some_and(|deadline| now >= deadline) {
            self.cancel();
            return Ok(Progress::TimedOut);
        }
        while let Ok(reply) = self.rx.try_recv() {
            let id = match &reply {
                Reply::Cookies(id, _) | Reply::Discovery(id, _) => *id,
            };
            if id != receipt {
                continue;
            }
            let result = match reply {
                Reply::Cookies(_, result) => {
                    result.and_then(|cookies| self.cookies_ready(cookies, now))
                }
                Reply::Discovery(_, result) => result.map(|v| Some(Progress::Discovery(v))),
            };
            match result {
                Ok(Some(done)) => {
                    self.pending = None;
                    self.close_view();
                    return Ok(done);
                }
                Ok(None) => {}
                Err(e) => {
                    self.cancel();
                    return Err(e);
                }
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.stage == Stage::Polling && !p.waiting && now >= p.next_poll)
        {
            let url = self.root.clone();
            if let Err(e) = self.request_cookies(&url) {
                self.cancel();
                return Err(e);
            }
        }
        Ok(Progress::Pending)
    }
    pub fn select_team(&mut self, organization: Option<String>) {
        self.selected_org = organization;
    }
    pub fn cancel(&mut self) {
        self.pending = None;
        self.close_view();
    }
    pub fn retire(&mut self) {
        self.cancel();
        self.retired = true;
        self.selected_org = None;
        self.context.take();
    }
}
#[cfg(target_os = "linux")]
impl Drop for PortalView {
    fn drop(&mut self) {
        self.retire();
    }
}
