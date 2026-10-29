//! Read-only Hermes transport. This viewer never restores, creates or mutates a session.
use std::{
    collections::HashMap,
    fmt,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket, client::IntoClientRequest, stream::MaybeTlsStream};
use url::Url;

mod catalog;
#[cfg(target_os = "linux")]
mod local;
pub use catalog::{CatalogModel, ModelCapabilities, ModelCatalog, ModelProvider};
#[cfg(test)]
mod tests;

const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BODY: usize = 8 * 1024 * 1024;
const HISTORY_LIMIT: usize = 500;

#[derive(Clone)]
pub enum Connection {
    Url {
        endpoint: String,
        token: Option<String>,
    },
    Local {
        home: PathBuf,
    },
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url { token, .. } => f
                .debug_struct("Url")
                .field("endpoint", &"[redacted]")
                .field("has_token", &token.is_some())
                .finish(),
            Self::Local { .. } => f.write_str("Local { home: [private] }"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub label: String,
    pub description: String,
    pub canonical_session: Option<CanonicalSession>,
}
/// The server's named Bot Chat registry row, never a recent-session guess.
#[derive(Clone, Debug)]
pub struct CanonicalSession {
    pub id: String,
    pub resolved_id: String,
    pub preview: String,
}
#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub updated: String,
}
#[derive(Clone, Debug)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
}
#[derive(Clone, Debug)]
pub struct History {
    pub messages: Vec<HistoryMessage>,
    pub has_more: bool,
}

enum Auth {
    Token(Option<String>),
    #[cfg(target_os = "linux")]
    Native(local::Endpoint),
}

pub struct Gateway {
    socket: WebSocket<MaybeTlsStream<DeadlineStream>>,
    base: Url,
    auth: Auth,
    http: reqwest::blocking::Client,
    sequence: u64,
    initial_profile: String,
    profile_homes: HashMap<String, PathBuf>,
    session_targets: HashMap<(String, String), String>,
    canonical_targets: HashMap<(String, String), String>,
}

impl Gateway {
    pub fn connect(connection: &Connection) -> Result<Self, String> {
        let (base, auth, initial_profile, native_ticket) = match connection {
            Connection::Url { endpoint, token } => {
                let (base, url_token) = endpoint_url(endpoint)?;
                (
                    base,
                    Auth::Token(token.clone().or(url_token)),
                    "default".to_owned(),
                    None,
                )
            }
            Connection::Local { home } => {
                #[cfg(target_os = "linux")]
                {
                    let endpoint = local::identify(home)?;
                    let ticket = endpoint.ticket(&endpoint.profile_id, "interactive")?;
                    let base =
                        Url::parse(&endpoint.origin).map_err(|_| "Invalid local gateway origin")?;
                    let profile = endpoint.profile_name();
                    (base, Auth::Native(endpoint), profile, Some(ticket))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = home;
                    return Err("Native local attachment is currently supported on Linux; use a gateway URL".into());
                }
            }
        };
        let mut ws = base.join("api/ws").map_err(|_| "Invalid gateway route")?;
        ws.set_scheme(if base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        })
        .map_err(|_| "Invalid gateway scheme")?;
        if let Auth::Token(Some(token)) = &auth {
            ws.query_pairs_mut().append_pair("token", token);
        }
        let mut request = ws
            .as_str()
            .into_client_request()
            .map_err(|_| "Invalid WebSocket request")?;
        if let Some(ticket) = &native_ticket {
            request.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                format!("hermes-gateway-v1, hermes-gateway-ticket.{ticket}")
                    .parse()
                    .map_err(|_| "Invalid native gateway ticket")?,
            );
        }
        let stream = connect_tcp(&ws)?;
        let config = tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_BODY))
            .max_frame_size(Some(MAX_BODY));
        let (socket, response) =
            tungstenite::client_tls_with_config(request, stream, Some(config), None)
                .map_err(|_| "WebSocket connection failed (network, TLS or authentication)")?;
        if native_ticket.is_some()
            && response
                .headers()
                .get("Sec-WebSocket-Protocol")
                .and_then(|h| h.to_str().ok())
                != Some("hermes-gateway-v1")
        {
            return Err("Gateway did not accept the native protocol".into());
        }
        let mut http = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none());
        #[cfg(target_os = "linux")]
        if matches!(auth, Auth::Native(_)) {
            http = http.no_proxy();
        }
        if base.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }) {
            http = http.no_proxy();
        }
        Ok(Self {
            socket,
            base,
            auth,
            http: http
                .build()
                .map_err(|_| "Could not initialize HTTP transport")?,
            sequence: 0,
            initial_profile,
            profile_homes: HashMap::new(),
            session_targets: HashMap::new(),
            canonical_targets: HashMap::new(),
        })
    }

    pub fn profiles(&mut self) -> Result<Vec<Profile>, String> {
        self.profiles_until(Instant::now() + TIMEOUT)
    }

    fn profiles_until(&mut self, deadline: Instant) -> Result<Vec<Profile>, String> {
        let value = self.rpc_until(
            "profiles.list",
            json!({"profile": self.initial_profile, "include_sessions": true}),
            deadline,
        )?;
        let rows = rows(&value, "profiles")?;
        let mut result = Vec::new();
        let mut homes = HashMap::new();
        let mut canonical_targets = HashMap::new();
        for row in rows {
            let name = required_str(row, "name")?.to_owned();
            valid_scope(&name)?;
            let bot_meta = row.get("ui_meta").and_then(|meta| meta.get("hermes-bots"));
            let label = profile_label(row, bot_meta, &name);
            let description = bot_meta
                .and_then(|meta| nonempty_string(meta, "description"))
                .or_else(|| nonempty_string(row, "description"))
                .unwrap_or_default()
                .to_owned();
            let canonical_session = match row.get("canonical_session") {
                Some(canonical) if !canonical.is_null() => {
                    let id = required_str(canonical, "id")?.to_owned();
                    let resolved_id = required_str(canonical, "resolved_id")?.to_owned();
                    valid_scope(&id)?;
                    valid_scope(&resolved_id)?;
                    canonical_targets.insert((name.clone(), id.clone()), resolved_id.clone());
                    Some(CanonicalSession {
                        id,
                        resolved_id,
                        preview: nonempty_string(canonical, "preview")
                            .unwrap_or_default()
                            .to_owned(),
                    })
                }
                _ => None,
            };
            if let Some(home) = row.get("path").and_then(Value::as_str) {
                homes.insert(name.clone(), PathBuf::from(home));
            }
            result.push(Profile {
                name,
                label,
                description,
                canonical_session,
            });
        }
        self.profile_homes = homes;
        self.canonical_targets = canonical_targets;
        Ok(result)
    }

    pub fn sessions(&mut self, profile: &str) -> Result<Vec<Session>, String> {
        valid_scope(profile)?;
        let value = self.rpc(
            "session.list",
            json!({"profile": profile, "scope": "stored", "limit": 200, "include_hidden": false}),
        )?;
        let rows = rows(&value, "sessions")?;
        let mut result = Vec::new();
        self.session_targets.clear();
        for row in rows {
            let id = row
                .get("session_id")
                .or_else(|| row.get("id"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or("Invalid session identity")?
                .to_owned();
            if let Some(resolved) = row
                .get("resolved_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                self.session_targets
                    .insert((profile.to_owned(), id.clone()), resolved.to_owned());
            }
            // Older clients can leave the named registry chat visible in the
            // database. Keep Bots as its only navigation door without changing
            // the server's durable visibility from this read-only client.
            let canonical_id = self
                .canonical_targets
                .iter()
                .any(|((owner, root), tip)| owner == profile && (&id == root || &id == tip));
            let canonical_title = ["title", "root_title"]
                .into_iter()
                .any(|key| row.get(key).and_then(Value::as_str).map(str::trim) == Some("Bot Chat"));
            if canonical_id || canonical_title {
                continue;
            }
            let updated = ["updated_at", "last_active", "started_at"]
                .into_iter()
                .filter_map(|key| row.get(key))
                .find(|v| !v.is_null())
                .map(display_value)
                .unwrap_or_default();
            result.push(Session {
                id,
                title: row
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                updated,
            });
        }
        Ok(result)
    }

    /// As Electron's initial view, read the latest window without binding a live session.
    /// Probe one earlier row so the UI can disclose truncation rather than inventing history.
    pub fn history(&mut self, profile: &str, session: &str) -> Result<History, String> {
        valid_scope(profile)?;
        valid_scope(session)?;
        let deadline = Instant::now() + TIMEOUT;
        let page = self.history_page(profile, session, HISTORY_LIMIT, 0, deadline)?;
        let message_rows = rows(&page, "messages")?;
        let has_more = if message_rows.len() == HISTORY_LIMIT {
            !rows(
                &self.history_page(profile, session, 1, HISTORY_LIMIT, deadline)?,
                "messages",
            )?
            .is_empty()
        } else {
            false
        };
        let messages = message_rows
            .iter()
            .filter(|row| row.get("display_kind").and_then(Value::as_str) != Some("hidden"))
            .map(|row| {
                Ok(HistoryMessage {
                    role: required_str(row, "role")?.into(),
                    content: message_text(row),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(History { messages, has_more })
    }

    fn history_page(
        &self,
        profile: &str,
        session: &str,
        limit: usize,
        offset: usize,
        deadline: Instant,
    ) -> Result<Value, String> {
        let mut url = self
            .base
            .join("api/sessions/")
            .map_err(|_| "Invalid history route")?;
        url.path_segments_mut()
            .map_err(|_| "Invalid history route")?
            .pop_if_empty()
            .push(session)
            .push("messages");
        url.query_pairs_mut()
            .append_pair("profile", profile)
            .append_pair("limit", &limit.to_string())
            .append_pair("offset", &offset.to_string())
            .append_pair("order", "latest")
            .append_pair("include_compacted", "true")
            .append_pair("inline_images", "false");
        let value = self.get_json(profile, url, deadline, "History")?;
        if value.get("profile").and_then(Value::as_str) != Some(profile) {
            return Err("History profile identity mismatch".into());
        }
        let resolved = self
            .session_targets
            .get(&(profile.into(), session.into()))
            .or_else(|| {
                self.canonical_targets
                    .get(&(profile.into(), session.into()))
            })
            .map(String::as_str)
            .unwrap_or(session);
        if !value
            .get("session_id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == session || id == resolved)
        {
            return Err("History session changed; refresh the session list".into());
        }
        rows(&value, "messages")?;
        Ok(value)
    }

    /// Both reader surfaces use the same profile-bound native HTTP credential
    /// path. A REST fallback never switches to another owner or public token.
    fn get_json(
        &self,
        profile: &str,
        mut url: Url,
        deadline: Instant,
        label: &str,
    ) -> Result<Value, String> {
        let mut headers = reqwest::header::HeaderMap::new();
        match &self.auth {
            Auth::Token(Some(token)) => {
                url.query_pairs_mut().append_pair("token", token);
            }
            Auth::Token(None) => {}
            #[cfg(target_os = "linux")]
            Auth::Native(endpoint) => {
                let home = self
                    .profile_homes
                    .get(profile)
                    .ok_or("Refresh profiles before reading this profile")?;
                let ticket = endpoint.ticket_until(home, "native-http", deadline)?;
                headers.insert(
                    "X-Hermes-Gateway-Ticket",
                    ticket.parse().map_err(|_| "Invalid native HTTP ticket")?,
                );
            }
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| format!("{label} request timed out"))?;
        let response = self
            .http
            .get(url)
            .headers(headers)
            .timeout(remaining)
            .send()
            .map_err(|_| format!("{label} request failed (network, TLS or timeout)"))?;
        if !response.status().is_success() {
            return Err(format!(
                "{label} request refused (HTTP {})",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        response
            .take((MAX_BODY + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| format!("{label} response could not be read"))?;
        if bytes.len() > MAX_BODY {
            return Err(format!("{label} response exceeds the viewer's size limit"));
        }
        serde_json::from_slice(&bytes).map_err(|_| format!("Invalid {label} response"))
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.rpc_until(method, params, Instant::now() + TIMEOUT)
    }

    fn rpc_until(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, String> {
        if !matches!(method, "profiles.list" | "session.list" | "model.options") {
            return Err("This viewer permits read methods only".into());
        }
        self.sequence += 1;
        let id = self.sequence;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Gateway read request timed out")?;
        socket_timeout(self.socket.get_mut(), remaining)?;
        self.socket
            .send(Message::Text(
                json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| "Could not send the gateway read request")?;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("Gateway read request timed out")?;
            socket_timeout(self.socket.get_mut(), remaining)?;
            let message = self
                .socket
                .read()
                .map_err(|_| "Gateway read failed (connection closed or timeout)")?;
            match message {
                Message::Text(text) => {
                    let value: Value =
                        serde_json::from_str(&text).map_err(|_| "Invalid gateway JSON response")?;
                    if value.get("id").and_then(Value::as_u64) != Some(id) {
                        continue;
                    }
                    if let Some(error) = value.get("error") {
                        let code = error.get("code").and_then(Value::as_i64).unwrap_or(-1);
                        return Err(format!("Gateway {method} read refused (RPC {code})"));
                    }
                    return value
                        .get("result")
                        .cloned()
                        .ok_or_else(|| "Gateway response has no result".into());
                }
                Message::Close(_) => return Err("Gateway closed the connection".into()),
                Message::Ping(_) => {
                    self.socket
                        .flush()
                        .map_err(|_| "Gateway heartbeat failed")?;
                }
                _ => {}
            }
        }
    }
}

fn endpoint_url(endpoint: &str) -> Result<(Url, Option<String>), String> {
    let mut url =
        Url::parse(endpoint).map_err(|_| "Enter a valid gateway HTTP(S) or WebSocket URL")?;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Gateway URL must use HTTP(S) or WebSocket without user information or fragment".into(),
        );
    }
    let mut token = None;
    for (name, value) in url.query_pairs() {
        if name != "token" || token.is_some() {
            return Err("Gateway URL query must contain at most one token".into());
        }
        token = Some(value.into_owned());
    }
    url.set_query(None);
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/api/ws").unwrap_or(path);
    let base_path = format!("{}/", path.trim_end_matches('/'));
    url.set_path(&base_path);
    let scheme = if matches!(url.scheme(), "https" | "wss") {
        "https"
    } else {
        "http"
    };
    url.set_scheme(scheme)
        .map_err(|_| "Invalid gateway scheme")?;
    Ok((url, token))
}

fn connect_tcp(url: &Url) -> Result<DeadlineStream, String> {
    let host = url
        .host_str()
        .ok_or("Missing gateway host")?
        .trim_matches(['[', ']'])
        .to_owned();
    let port = url.port_or_known_default().ok_or("Missing gateway port")?;
    let deadline = Instant::now() + TIMEOUT;
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = send.send(
            (host.as_str(), port)
                .to_socket_addrs()
                .map(|a| a.collect::<Vec<_>>()),
        );
    });
    let addresses = receive
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Gateway name lookup timed out")?
        .map_err(|_| "Gateway name lookup failed")?;
    for address in addresses {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Gateway connection timed out")?;
        if let Ok(stream) =
            TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(5)))
        {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("Gateway connection timed out")?;
            stream
                .set_read_timeout(Some(remaining))
                .map_err(|_| "Could not bound gateway reads")?;
            stream
                .set_write_timeout(Some(remaining))
                .map_err(|_| "Could not bound gateway writes")?;
            return Ok(DeadlineStream { stream, deadline });
        }
    }
    Err("Could not connect to the selected gateway".into())
}

struct DeadlineStream {
    stream: TcpStream,
    deadline: Instant,
}
impl DeadlineStream {
    fn remaining(&self) -> std::io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "Gateway I/O deadline")
            })
    }
}
impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(bytes)
    }
}
impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.flush()
    }
}
fn socket_timeout(
    stream: &mut MaybeTlsStream<DeadlineStream>,
    timeout: Duration,
) -> Result<(), String> {
    let stream = match stream {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => &mut stream.sock,
        _ => return Err("Unsupported WebSocket transport".into()),
    };
    stream.deadline = Instant::now() + timeout;
    Ok(())
}

fn rows<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| "Invalid gateway collection response".into())
}
fn nonempty_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Native rows follow Bot Mode's backend title/display-name ladder. Client-side
/// aliases and local metadata belong to a future connection registry, not here.
fn profile_label(row: &Value, bot_meta: Option<&Value>, name: &str) -> String {
    if let Some(label) = bot_meta
        .and_then(|meta| nonempty_string(meta, "title"))
        .or_else(|| nonempty_string(row, "title"))
        .or_else(|| nonempty_string(row, "display_name"))
    {
        return label.to_owned();
    }
    if name.eq_ignore_ascii_case("default") {
        return "Hermes".into();
    }
    let mut word_start = true;
    name.chars()
        .map(|character| {
            let character = if matches!(character, '-' | '_') {
                ' '
            } else {
                character
            };
            let output = if word_start && character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                character
            };
            word_start = !character.is_ascii_alphanumeric() && character != '_';
            output
        })
        .collect::<String>()
        .trim()
        .to_owned()
}
fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Invalid gateway record".into())
}
fn valid_scope(value: &str) -> Result<(), String> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.len() > 1024
        || value.chars().any(char::is_control)
    {
        Err("Invalid profile or session identity".into())
    } else {
        Ok(())
    }
}
fn display_value(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn message_text(value: &Value) -> String {
    if let Some(text) = value.get("display_content").and_then(Value::as_str) {
        return text.into();
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.into();
    }
    match value.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
