//! Client-owned connection registry. Passwords stay in the OS credential store;
//! connection tests and SSH forwarding attach to existing agents only.
use crate::gateway::Connection;
use rune::{TextEdit, Waker};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};
mod core;
mod leases;
pub use leases::{Lease, Resolver};
pub(crate) mod plugin;
mod ssh;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Local,
    Remote,
    Ssh,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Destination {
    Local { home: PathBuf },
    Remote { url: String },
    Ssh(Ssh),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ssh {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub key_file: Option<PathBuf>,
    pub remote_port: u16,
    pub local_port: u16,
    #[serde(default)]
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Saved {
    pub id: String,
    pub revision: String,
    pub label: String,
    pub destination: Destination,
    #[serde(default)]
    credential: Option<String>,
}
impl Saved {
    pub fn kind(&self) -> Kind {
        match self.destination {
            Destination::Local { .. } => Kind::Local,
            Destination::Remote { .. } => Kind::Remote,
            Destination::Ssh(_) => Kind::Ssh,
        }
    }
    pub fn has_credential(&self) -> bool {
        self.credential.is_some()
    }
    pub fn summary(&self) -> String {
        match &self.destination {
            Destination::Local { home } => home.display().to_string(),
            Destination::Remote { url } => url.clone(),
            Destination::Ssh(ssh) => format!(
                "{}{}{}:{} → 127.0.0.1:{}",
                ssh.user,
                if ssh.user.is_empty() { "" } else { "@" },
                ssh.host,
                ssh.port,
                ssh.remote_port
            ),
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Registry {
    version: u32,
    rows: Vec<Saved>,
    selected: Option<String>,
    #[serde(default)]
    cleanup: Vec<String>,
}
impl Registry {
    fn empty() -> Self {
        Self {
            version: 1,
            ..Default::default()
        }
    }
}

pub struct Editor {
    pub id: Option<String>,
    pub kind: Kind,
    pub label: TextEdit,
    pub home: TextEdit,
    pub url: TextEdit,
    pub token: TextEdit,
    pub clear_token: bool,
    pub had_token: bool,
    pub ssh_host: TextEdit,
    pub ssh_user: TextEdit,
    pub ssh_port: TextEdit,
    pub remote_port: TextEdit,
    pub key_file: TextEdit,
    pub ssh_path: TextEdit,
}
impl Editor {
    fn new(kind: Kind) -> Self {
        Self {
            id: None,
            kind,
            label: TextEdit::new(""),
            home: TextEdit::new(""),
            url: TextEdit::new(""),
            token: TextEdit::new(""),
            clear_token: false,
            had_token: false,
            ssh_host: TextEdit::new(""),
            ssh_user: TextEdit::new(""),
            ssh_port: TextEdit::new("22"),
            remote_port: TextEdit::new("9119"),
            key_file: TextEdit::new(""),
            ssh_path: TextEdit::new(""),
        }
    }
    fn existing(saved: &Saved) -> Self {
        let mut e = Self::new(saved.kind());
        e.id = Some(saved.id.clone());
        e.label.replace_text(saved.label.clone());
        e.had_token = saved.has_credential();
        match &saved.destination {
            Destination::Local { home } => e.home.replace_text(home.to_string_lossy()),
            Destination::Remote { url } => e.url.replace_text(url),
            Destination::Ssh(ssh) => {
                e.ssh_host.replace_text(&ssh.host);
                e.ssh_user.replace_text(&ssh.user);
                e.ssh_port.replace_text(ssh.port.to_string());
                e.remote_port.replace_text(ssh.remote_port.to_string());
                e.key_file.replace_text(
                    ssh.key_file
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
                e.ssh_path.replace_text(&ssh.path);
            }
        }
        e
    }
    fn input(&self) -> Result<Input, String> {
        let label = self.label.text.trim().to_owned();
        if label.is_empty() {
            return Err("Give this connection a name".into());
        }
        let destination = match self.kind {
            Kind::Local => Destination::Local {
                home: PathBuf::from(self.home.text.trim()),
            },
            Kind::Remote => Destination::Remote {
                url: self.url.text.trim().into(),
            },
            Kind::Ssh => Destination::Ssh(Ssh {
                host: self.ssh_host.text.trim().into(),
                user: self.ssh_user.text.trim().into(),
                port: port(&self.ssh_port.text, "SSH port")?,
                remote_port: port(&self.remote_port.text, "Gateway port")?,
                local_port: 0,
                key_file: (!self.key_file.text.trim().is_empty())
                    .then(|| PathBuf::from(self.key_file.text.trim())),
                path: self.ssh_path.text.trim().into(),
            }),
        };
        Ok(Input {
            id: self.id.clone(),
            label,
            destination,
            secret: if !self.token.text.is_empty() {
                SecretChange::Set(self.token.text.clone())
            } else if self.clear_token {
                SecretChange::Clear
            } else {
                SecretChange::Keep
            },
        })
    }
}
fn port(value: &str, name: &str) -> Result<u16, String> {
    value
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| format!("{name} must be between 1 and 65535"))
}
struct Input {
    id: Option<String>,
    label: String,
    destination: Destination,
    secret: SecretChange,
}
enum SecretChange {
    Keep,
    Clear,
    Set(String),
}
#[derive(Clone)]
pub enum Action {
    Add(Kind),
    Edit(String),
    Kind(Kind),
    Save,
    ConnectOnce,
    Cancel,
    Select(String),
    AskForget(String),
    ConfirmForget,
    CancelForget,
    Reload,
}
pub enum Event {
    Ready(Connection),
    Changed,
    Lost {
        id: String,
        revision: String,
        message: String,
    },
}
struct Request {
    id: u64,
    command: Command,
}
enum Command {
    Load,
    Save(Input),
    ConnectOnce(Input),
    Forget(String),
    Select(String),
    CommitLease(Lease, Option<String>),
}
struct Reply {
    id: u64,
    result: Result<Outcome, String>,
}
enum Outcome {
    Registry(Registry, Option<String>),
    Ready(Registry, Connection),
    Lost(String, String, String),
}
enum Work {
    Ui(Request),
    Resolve {
        id: String,
        expected_revision: Option<String>,
        reply: Sender<Result<Lease, String>>,
    },
}
struct Network {
    send: Sender<Work>,
    receive: Receiver<Reply>,
}
pub struct Connections {
    pub rows: Vec<Saved>,
    pub selected: Option<String>,
    pub editor: Option<Editor>,
    pub forget: Option<String>,
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    network: Option<Network>,
    sequence: u64,
    pending: Option<u64>,
    synthetic: bool,
}
impl Default for Connections {
    fn default() -> Self {
        Self::new()
    }
}
impl Connections {
    pub fn new() -> Self {
        Self {
            rows: vec![],
            selected: None,
            editor: None,
            forget: None,
            busy: false,
            error: None,
            notice: None,
            network: None,
            sequence: 0,
            pending: None,
            synthetic: false,
        }
    }
    pub fn init(&mut self, waker: Waker) {
        if self.network.is_some() {
            return;
        }
        let (send, requests) = mpsc::channel::<Work>();
        let (replies, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let path = crate::client::state_dir().map(|p| p.join("connections.json"));
            let mut state = path.map(core::Core::system);
            loop {
                match requests.recv_timeout(std::time::Duration::from_millis(200)) {
                    Ok(Work::Ui(request)) => {
                        let result = match &mut state {
                            Ok(core) => core.execute(request.command),
                            Err(e) => Err(e.clone()),
                        };
                        if replies
                            .send(Reply {
                                id: request.id,
                                result,
                            })
                            .is_err()
                        {
                            break;
                        }
                        waker();
                    }
                    Ok(Work::Resolve {
                        id,
                        expected_revision,
                        reply,
                    }) => {
                        let result = match &mut state {
                            Ok(core) => core.resolve_lease(&id, expected_revision.as_deref()),
                            Err(error) => Err(error.clone()),
                        };
                        let _ = reply.send(result);
                        waker();
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if let Ok(core) = &mut state {
                    if let Some((id, revision, error)) = core.poll_tunnel() {
                        if replies
                            .send(Reply {
                                id: 0,
                                result: Ok(Outcome::Lost(id, revision, error)),
                            })
                            .is_err()
                        {
                            break;
                        }
                        waker();
                    }
                }
            }
        });
        self.network = Some(Network { send, receive });
        self.load();
    }
    pub fn resolver(&self) -> Option<Resolver> {
        self.network
            .as_ref()
            .map(|network| Resolver::new(network.send.clone()))
    }
    pub fn commit_lease(&mut self, lease: Lease) {
        if self.synthetic {
            self.error = Some("Connection operations are disabled in synthetic scenes".into());
            return;
        }
        self.dispatch(Command::CommitLease(lease, None));
    }
    pub fn commit_lease_for(&mut self, lease: Lease, profile: String) {
        if self.synthetic {
            self.error = Some("Connection operations are disabled in synthetic scenes".into());
            return;
        }
        self.dispatch(Command::CommitLease(lease, Some(profile)));
    }
    pub fn load(&mut self) {
        self.dispatch(Command::Load);
    }
    fn dispatch(&mut self, command: Command) {
        self.sequence += 1;
        self.pending = Some(self.sequence);
        self.busy = true;
        self.error = None;
        let request = Request {
            id: self.sequence,
            command,
        };
        if !self
            .network
            .as_ref()
            .is_some_and(|n| n.send.send(Work::Ui(request)).is_ok())
        {
            self.busy = false;
            self.pending = None;
            self.error = Some("The connection worker is unavailable".into());
        }
    }
    pub fn action(&mut self, action: Action) {
        if self.synthetic
            && !matches!(
                action,
                Action::Add(_)
                    | Action::Edit(_)
                    | Action::Kind(_)
                    | Action::Cancel
                    | Action::AskForget(_)
                    | Action::CancelForget
            )
        {
            self.error = Some("Connection operations are disabled in synthetic scenes".into());
            return;
        }
        if self.busy && !matches!(action, Action::CancelForget) {
            return;
        }
        self.error = None;
        self.notice = None;
        match action {
            Action::Add(kind) => {
                self.editor = Some(Editor::new(kind));
                self.forget = None;
            }
            Action::Edit(id) => {
                if let Some(saved) = self.rows.iter().find(|r| r.id == id) {
                    self.editor = Some(Editor::existing(saved));
                    self.forget = None;
                }
            }
            Action::Kind(kind) => {
                if let Some(editor) = &mut self.editor {
                    editor.kind = kind;
                }
            }
            Action::Save => {
                if let Some(editor) = &self.editor {
                    match editor.input() {
                        Ok(input) => self.dispatch(Command::Save(input)),
                        Err(e) => self.error = Some(e),
                    }
                }
            }
            Action::ConnectOnce => {
                if let Some(editor) = &self.editor {
                    match editor.input() {
                        Ok(input) => self.dispatch(Command::ConnectOnce(input)),
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            Action::Cancel => self.editor = None,
            Action::Select(id) => self.dispatch(Command::Select(id)),
            Action::AskForget(id) => self.forget = Some(id),
            Action::CancelForget => self.forget = None,
            Action::ConfirmForget => {
                if let Some(id) = &self.forget {
                    self.dispatch(Command::Forget(id.clone()));
                }
            }
            Action::Reload => self.load(),
        }
    }
    pub fn poll(&mut self) -> Vec<Event> {
        let replies: Vec<_> = self
            .network
            .as_ref()
            .map(|n| n.receive.try_iter().collect())
            .unwrap_or_default();
        let mut events = vec![];
        for reply in replies {
            if let Ok(Outcome::Lost(id, revision, message)) = &reply.result {
                events.push(Event::Lost {
                    id: id.clone(),
                    revision: revision.clone(),
                    message: message.clone(),
                });
                continue;
            }
            if self.pending != Some(reply.id) {
                continue;
            }
            self.pending = None;
            self.busy = false;
            match reply.result {
                Err(error) => self.error = Some(error),
                Ok(Outcome::Registry(registry, notice)) => {
                    self.rows = registry.rows;
                    self.selected = registry.selected;
                    self.notice = notice;
                    self.editor = None;
                    self.forget = None;
                }
                Ok(Outcome::Ready(registry, connection)) => {
                    self.rows = registry.rows;
                    self.selected = registry.selected;
                    self.editor = None;
                    self.forget = None;
                    self.error = None;
                    events.push(Event::Ready(connection));
                }
                Ok(Outcome::Lost(..)) => unreachable!(),
            }
            events.push(Event::Changed);
        }
        events
    }
    pub fn fixture(&mut self) {
        self.synthetic = true;
        self.network = None;
        self.pending = None;
        self.rows = vec![
            Saved {
                id: "synthetic-local".into(),
                revision: "synthetic-revision".into(),
                label: "Local Hermes".into(),
                destination: Destination::Local {
                    home: PathBuf::from("/synthetic/hermes"),
                },
                credential: None,
            },
            Saved {
                id: "synthetic-remote".into(),
                revision: "synthetic-remote-revision".into(),
                label: "Remote agent".into(),
                destination: Destination::Remote {
                    url: "https://agent.example.invalid/".into(),
                },
                credential: Some("synthetic-keyring-reference".into()),
            },
        ];
        self.busy = false;
    }
}
impl crate::app::Viewer {
    pub fn connections_action(&mut self, action: Action) {
        if !self.is_fixture() {
            let guarded = match &action {
                Action::Select(_) | Action::ConnectOnce => true,
                Action::Edit(id) | Action::AskForget(id) => {
                    self.connections.selected.as_deref() == Some(id)
                }
                Action::ConfirmForget => self.connections.forget == self.connections.selected,
                _ => false,
            };
            if guarded && !self.begin_saved_connection_change() {
                return;
            }
        }
        self.connections.action(action);
    }
}
