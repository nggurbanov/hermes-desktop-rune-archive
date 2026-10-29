//! Native viewer state. Network replies carry the intent and profile that own them.
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use rune::{TextEdit, Tx, Waker};
use rune_kit::PaneState;

use crate::gateway::{Connection, Gateway, History, HistoryMessage, Profile, Session};

pub struct Viewer {
    pub profiles: Vec<Profile>,
    pub sessions: Vec<Session>,
    pub messages: Vec<HistoryMessage>,
    pub profile: String,
    pub selected_session: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub loading: bool,
    pub history_loading: bool,
    pub history_truncated: bool,
    pub endpoint_label: String,
    pub search: TextEdit,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub theme: Option<bool>,
    pub connection_form: bool,
    pub connection_url: TextEdit,
    pub connection_token: TextEdit,
    pub connection_local_home: TextEdit,
    connection: Option<Connection>,
    intent: u64,
    network: Option<Network>,
    fixture: bool,
}

struct Network {
    send: Sender<Request>,
    receive: Receiver<Reply>,
}
struct Request {
    intent: u64,
    command: Command,
}
enum Command {
    Connect(Connection, String),
    Sessions(String),
    History(String, String),
}
struct Reply {
    intent: u64,
    result: Result<Update, String>,
}
enum Update {
    Connected {
        profiles: Vec<Profile>,
        profile: String,
        sessions: Vec<Session>,
    },
    Sessions {
        profile: String,
        sessions: Vec<Session>,
    },
    History {
        profile: String,
        session: String,
        history: History,
    },
}

impl Viewer {
    fn empty() -> Self {
        Self {
            profiles: vec![],
            sessions: vec![],
            messages: vec![],
            profile: "default".into(),
            selected_session: None,
            status: "Not connected".into(),
            error: None,
            loading: false,
            history_loading: false,
            history_truncated: false,
            endpoint_label: "Connect an agent".into(),
            search: TextEdit::new(""),
            sidebar: PaneState::new(237.0).range(190.0, 360.0),
            inspector: PaneState::new(420.0).range(300.0, 640.0),
            theme: None,
            connection_form: true,
            connection_url: TextEdit::new(""),
            connection_token: TextEdit::new(""),
            connection_local_home: TextEdit::new(default_agent_home().to_string_lossy()),
            connection: None,
            intent: 0,
            network: None,
            fixture: false,
        }
    }

    pub fn new() -> Self {
        let mut s = Self::empty();
        if let Some(path) = argument("--fixture") {
            match std::fs::read_to_string(path)
                .and_then(|v| serde_json::from_str(&v).map_err(std::io::Error::other))
            {
                Ok(value) => s.load_fixture(value),
                Err(_) => s.error = Some("Unable to read the selected fixture".into()),
            }
            return s;
        }
        if let Some(endpoint) = argument("--gateway") {
            let token = std::env::var("HERMES_GATEWAY_TOKEN")
                .ok()
                .filter(|s| !s.is_empty());
            match url_connection(&endpoint, token) {
                Ok((connection, visible_url)) => {
                    s.connection_url.replace_text(visible_url);
                    s.connection = Some(connection);
                }
                Err(error) => s.error = Some(error),
            }
        } else if let Some(home) = argument("--local-home") {
            s.connection_local_home.replace_text(home.clone());
            s.connection = Some(Connection::Local { home: home.into() });
        }
        if let Some(profile) = argument("--profile") {
            s.profile = profile;
        }
        s.connection_form = s.connection.is_none();
        s
    }

    pub fn begin(&mut self) {
        if !self.fixture && self.connection.is_some() {
            self.reconnect();
        }
    }

    pub fn reconnect(&mut self) {
        let Some(connection) = self.connection.clone() else {
            self.change_connection();
            return;
        };
        self.endpoint_label = connection_label(&connection);
        self.loading = true;
        self.error = None;
        self.status = "Connecting…".into();
        self.dispatch(Command::Connect(connection, self.profile.clone()));
    }

    pub fn change_connection(&mut self) {
        self.connection_form = true;
    }
    pub fn can_cancel_connection(&self) -> bool {
        self.connection.is_some() || self.fixture
    }
    pub fn cancel_connection(&mut self) {
        self.connection_token.replace_text("");
        self.connection_form = self.connection.is_none() && !self.fixture;
    }
    pub fn apply_connection(&mut self) {
        if self.fixture {
            self.error = Some("Connections are disabled in fixture mode".into());
            return;
        }
        let endpoint = self.connection_url.text.trim().to_owned();
        let valid = url::Url::parse(&endpoint).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https" | "ws" | "wss")
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
                && u.query().is_none()
                && u.fragment().is_none()
        });
        if !valid {
            self.error = Some("Enter a gateway URL without credentials or query parameters; use the token field for authentication".into());
            return;
        }
        let token =
            (!self.connection_token.text.is_empty()).then(|| self.connection_token.text.clone());
        self.connection_token.replace_text("");
        self.use_connection(Connection::Url { endpoint, token });
    }
    pub fn apply_local_connection(&mut self) {
        if self.fixture {
            self.error = Some("Connections are disabled in fixture mode".into());
            return;
        }
        let home = PathBuf::from(self.connection_local_home.text.trim());
        if !home.is_absolute() {
            self.error = Some("Choose an absolute local Hermes home path".into());
            return;
        }
        self.use_connection(Connection::Local { home });
    }
    fn use_connection(&mut self, connection: Connection) {
        self.connection = Some(connection);
        self.connection_form = false;
        self.profiles.clear();
        self.sessions.clear();
        self.messages.clear();
        self.selected_session = None;
        self.history_truncated = false;
        self.reconnect();
    }
    pub fn toggle_theme(&mut self, system_dark: bool) {
        self.theme = Some(!self.theme.unwrap_or(system_dark));
    }
    pub fn select_profile(&mut self, profile: String) {
        if profile == self.profile {
            return;
        }
        self.profile = profile.clone();
        self.selected_session = None;
        self.messages.clear();
        self.sessions.clear();
        self.history_loading = false;
        self.history_truncated = false;
        self.loading = true;
        self.error = None;
        self.search.replace_text("");
        if self.fixture {
            self.intent += 1;
            self.loading = false;
            return;
        }
        self.dispatch(Command::Sessions(profile));
    }
    pub fn open_session(&mut self, id: String) {
        if self.selected_session.as_ref() == Some(&id) && self.error.is_none() {
            return;
        }
        self.selected_session = Some(id.clone());
        self.messages.clear();
        self.history_loading = true;
        self.history_truncated = false;
        self.error = None;
        if self.fixture {
            self.history_loading = false;
            return;
        }
        self.dispatch(Command::History(self.profile.clone(), id));
    }
    fn dispatch(&mut self, command: Command) {
        self.intent += 1;
        let sent = self.network.as_ref().is_some_and(|n| {
            n.send
                .send(Request {
                    intent: self.intent,
                    command,
                })
                .is_ok()
        });
        if !sent {
            self.loading = false;
            self.history_loading = false;
            self.error = Some("The connection worker is unavailable. Restart the client.".into());
        }
    }
    fn apply(&mut self, reply: Reply) {
        if reply.intent != self.intent {
            return;
        }
        self.loading = false;
        self.history_loading = false;
        match reply.result {
            Err(error) => {
                self.status = "Connection error".into();
                self.error = Some(error);
            }
            Ok(Update::Connected {
                profiles,
                profile,
                sessions,
            }) => {
                let retained = if self.profile == profile {
                    self.selected_session
                        .clone()
                        .filter(|id| sessions.iter().any(|s| &s.id == id))
                } else {
                    None
                };
                self.profiles = profiles;
                self.profile = profile;
                self.sessions = sessions;
                self.selected_session = None;
                self.messages.clear();
                self.error = None;
                self.status = "Connected · Read only".into();
                if let Some(id) = retained {
                    self.open_session(id);
                }
            }
            Ok(Update::Sessions { profile, sessions }) if profile == self.profile => {
                self.sessions = sessions;
                self.status = "Connected · Read only".into();
            }
            Ok(Update::History {
                profile,
                session,
                history,
            }) if profile == self.profile && self.selected_session.as_ref() == Some(&session) => {
                self.messages = history.messages;
                self.history_truncated = history.has_more;
                self.status = "Connected · Read only".into();
                self.error = None;
            }
            Ok(_) => {}
        }
    }
    fn load_fixture(&mut self, v: serde_json::Value) {
        self.fixture = true;
        self.connection_form = false;
        self.endpoint_label = "Sample agent · Fixture".into();
        self.status = "Fixture · Read only".into();
        self.profiles = v["profiles"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| Profile {
                name: p["name"].as_str().unwrap_or("default").into(),
                label: p["label"].as_str().unwrap_or("Default").into(),
            })
            .collect();
        self.profile = v["profile"].as_str().unwrap_or("default").into();
        self.sessions = v["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| Session {
                id: p["id"].as_str().unwrap_or("").into(),
                title: p["title"].as_str().unwrap_or("Untitled").into(),
                updated: String::new(),
            })
            .collect();
        self.selected_session = v["selected_session"].as_str().map(String::from);
        self.messages = v["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| HistoryMessage {
                role: p["role"].as_str().unwrap_or("assistant").into(),
                content: p["content"].as_str().unwrap_or("").into(),
            })
            .collect();
    }
}

pub fn setup(s: &mut Viewer, waker: Waker) {
    if s.fixture {
        return;
    }
    let (send, requests) = mpsc::channel::<Request>();
    let (replies, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut gateway: Option<Gateway> = None;
        let mut pending = VecDeque::new();
        loop {
            let first = match pending.pop_front() {
                Some(request) => request,
                None => match requests.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                },
            };
            pending = coalesce(first, requests.try_iter());
            let request = pending.pop_front().expect("a queued read intent");
            let result = match request.command {
                Command::Connect(connection, requested_profile) => {
                    gateway = None;
                    Gateway::connect(&connection).and_then(|mut g| {
                        let profiles = g.profiles()?;
                        let profile = if profiles.iter().any(|p| p.name == requested_profile) {
                            requested_profile
                        } else {
                            profiles
                                .first()
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| "default".into())
                        };
                        let sessions = g.sessions(&profile)?;
                        gateway = Some(g);
                        Ok(Update::Connected {
                            profiles,
                            profile,
                            sessions,
                        })
                    })
                }
                Command::Sessions(profile) => gateway
                    .as_mut()
                    .ok_or_else(|| "Connect an agent first".into())
                    .and_then(|g| {
                        g.sessions(&profile)
                            .map(|sessions| Update::Sessions { profile, sessions })
                    }),
                Command::History(profile, session) => gateway
                    .as_mut()
                    .ok_or_else(|| "Connect an agent first".into())
                    .and_then(|g| {
                        g.history(&profile, &session)
                            .map(|history| Update::History {
                                profile,
                                session,
                                history,
                            })
                    }),
            };
            if replies
                .send(Reply {
                    intent: request.intent,
                    result,
                })
                .is_err()
            {
                break;
            }
            waker();
        }
    });
    s.network = Some(Network { send, receive });
}

pub fn poll(s: &mut Viewer, _: f64, tx: &mut Tx) -> bool {
    let mut replies = vec![];
    if let Some(network) = &s.network {
        while let Ok(reply) = network.receive.try_recv() {
            replies.push(reply);
        }
    }
    if replies.is_empty() {
        return false;
    }
    for reply in replies {
        s.apply(reply);
    }
    tx.instant();
    true
}

fn argument(key: &str) -> Option<String> {
    let a = rune::extra_args();
    a.iter()
        .position(|v| v == key)
        .and_then(|i| a.get(i + 1))
        .cloned()
}
fn default_agent_home() -> PathBuf {
    std::env::var_os("HERMES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".hermes")
        })
}
fn connection_label(connection: &Connection) -> String {
    match connection {
        Connection::Local { .. } => "Local Hermes".into(),
        Connection::Url { endpoint, .. } => url::Url::parse(endpoint)
            .ok()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_else(|| "Remote Hermes".into()),
    }
}

pub fn reader_scene(s: &mut Viewer) {
    s.load_fixture(
        serde_json::from_str(include_str!("../fixtures/reader.json"))
            .expect("committed reader fixture"),
    );
}
pub fn empty_scene(s: &mut Viewer) {
    reader_scene(s);
    s.sessions.clear();
    s.messages.clear();
    s.selected_session = None;
}
pub fn error_scene(s: &mut Viewer) {
    empty_scene(s);
    s.error = Some("The agent could not be reached. Check the connection and try again.".into());
}

#[cfg(test)]
mod tests;

pub fn on_key(s: &mut Viewer, event: &rune::KeyEvent, _: &mut Tx) -> bool {
    if matches!(&event.key, rune::Key::Escape) && s.connection_form && s.can_cancel_connection() {
        s.cancel_connection();
        true
    } else {
        false
    }
}

// Only viewer reads may coalesce. A future mutation queue needs its own durable ordering.
fn coalesce(first: Request, incoming: impl IntoIterator<Item = Request>) -> VecDeque<Request> {
    let mut pending = VecDeque::new();
    for request in std::iter::once(first).chain(incoming) {
        if matches!(&request.command, Command::Connect(_, _)) {
            pending.clear();
        } else if pending
            .back()
            .is_some_and(|r: &Request| !matches!(&r.command, Command::Connect(_, _)))
        {
            pending.pop_back();
        }
        pending.push_back(request);
    }
    pending
}

fn url_connection(endpoint: &str, token: Option<String>) -> Result<(Connection, String), String> {
    let mut url = url::Url::parse(endpoint).map_err(|_| "Enter a valid gateway URL")?;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Use a gateway URL without user credentials or fragments".into());
    }
    let mut url_token = None;
    for (key, value) in url.query_pairs() {
        if key != "token" || url_token.is_some() {
            return Err("Unsupported gateway URL parameters".into());
        }
        url_token = Some(value.into_owned());
    }
    url.set_query(None);
    let visible = url.to_string();
    Ok((
        Connection::Url {
            endpoint: visible.clone(),
            token: token.or(url_token),
        },
        visible,
    ))
}
