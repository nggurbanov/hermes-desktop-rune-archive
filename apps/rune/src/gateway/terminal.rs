//! Durable remote host shells: every dial keeps the captured gateway/profile,
//! every reconnect attaches the known terminal id, and close needs a receipt.
use super::*;
use crate::pages::PageRequest;
const META: &str = "\0HERMES_TERMINAL_META:";
#[derive(Debug)]
pub enum Frame {
    Meta {
        terminal_id: Option<String>,
        shell: String,
        cwd: Option<String>,
        truncated: bool,
        closed: bool,
    },
    Data(Vec<u8>),
    Closed {
        code: u16,
        reason: String,
    },
}
pub struct HostTerminal {
    socket: WebSocket<MaybeTlsStream<DeadlineStream>>,
}
impl HostTerminal {
    pub fn connect(
        connection: &Connection,
        profile: &str,
        cwd: &str,
        cols: u16,
        rows: u16,
        attach: Option<&str>,
        close: bool,
    ) -> Result<Self, String> {
        valid_scope(profile)?;
        if let Some(id) = attach {
            valid_scope(id)?;
        }
        let mut gateway = Gateway::connect(connection)?;
        #[cfg(target_os = "linux")]
        if matches!(gateway.auth, Auth::Native(_)) {
            gateway.profiles()?;
        }
        let mut url = gateway
            .base
            .join("api/host-terminal")
            .map_err(|_| "Invalid host terminal route")?;
        url.set_scheme(if gateway.base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        })
        .map_err(|_| "Invalid host terminal scheme")?;
        url.query_pairs_mut()
            .append_pair("profile", profile)
            .append_pair("cols", &cols.to_string())
            .append_pair("rows", &rows.to_string());
        if !cwd.is_empty() {
            url.query_pairs_mut().append_pair("cwd", cwd);
        }
        if let Some(id) = attach {
            url.query_pairs_mut().append_pair("attach", id);
        }
        if close {
            if attach.is_none() {
                return Err("A remote close needs its durable shell identity".into());
            }
            url.query_pairs_mut().append_pair("action", "close");
        }
        let mut ticket = None;
        match &gateway.auth {
            Auth::Token(Some(token)) => {
                url.query_pairs_mut().append_pair("token", token);
            }
            Auth::Token(None) => {}
            #[cfg(target_os = "linux")]
            Auth::Native(endpoint) => {
                let home = gateway
                    .profile_homes
                    .get(profile)
                    .ok_or("The terminal profile is not served by this gateway")?;
                ticket = Some(endpoint.ticket(home, "interactive")?);
            }
        }
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| "Invalid host terminal request")?;
        if let Some(ticket) = &ticket {
            request.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                format!("hermes-gateway-v1, hermes-gateway-ticket.{ticket}")
                    .parse()
                    .map_err(|_| "Invalid terminal ticket")?,
            );
        }
        let stream = connect_tcp(&url)?;
        let config = tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(2 * 1024 * 1024))
            .max_frame_size(Some(2 * 1024 * 1024));
        let (socket, response) =
            tungstenite::client_tls_with_config(request, stream, Some(config), None).map_err(
                |_| "The remote terminal connection failed (network, TLS or authentication)",
            )?;
        if ticket.is_some()
            && response
                .headers()
                .get("Sec-WebSocket-Protocol")
                .and_then(|h| h.to_str().ok())
                != Some("hermes-gateway-v1")
        {
            return Err("The remote terminal did not accept private gateway authentication".into());
        }
        Ok(Self { socket })
    }
    pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let frame = match std::str::from_utf8(data) {
            Ok(text) => Message::Text(text.to_owned().into()),
            Err(_) => Message::Binary(data.to_vec().into()),
        };
        socket_timeout(self.socket.get_mut(), Duration::from_secs(5))?;
        self.socket
            .send(frame)
            .map_err(|_| "The remote terminal did not accept input".into())
    }
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        self.write(format!("\x1b[RESIZE:{cols};{rows}]").as_bytes())
    }
    pub fn poll(&mut self) -> Result<Option<Frame>, String> {
        socket_timeout(self.socket.get_mut(), Duration::from_millis(35))?;
        match self.socket.read() {
            Ok(Message::Text(text)) => {
                if let Some(meta) = text.strip_prefix(META) {
                    let value: Value =
                        serde_json::from_str(meta).map_err(|_| "Invalid host terminal metadata")?;
                    Ok(Some(Frame::Meta {
                        terminal_id: value
                            .get("terminalId")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        shell: value
                            .get("shell")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .into(),
                        cwd: value.get("cwd").and_then(Value::as_str).map(str::to_owned),
                        truncated: value["truncated"] == true,
                        closed: value["closed"] == true,
                    }))
                } else {
                    Ok(Some(Frame::Data(text.as_bytes().to_vec())))
                }
            }
            Ok(Message::Binary(bytes)) => Ok(Some(Frame::Data(bytes.to_vec()))),
            Ok(Message::Close(frame)) => Ok(Some(Frame::Closed {
                code: frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(1006),
                reason: frame.map(|f| f.reason.to_string()).unwrap_or_default(),
            })),
            Ok(Message::Ping(data)) => {
                self.socket
                    .send(Message::Pong(data))
                    .map_err(|_| "Remote terminal heartbeat failed")?;
                Ok(None)
            }
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err("The remote terminal connection was interrupted".into()),
        }
    }
    pub fn detach(&mut self) {
        let _ = self.socket.close(None);
    }
}
pub fn stage(
    connection: &Connection,
    profile: &str,
    path: &std::path::Path,
) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|_| "The dropped file is unavailable")?;
    if !metadata.is_file() || metadata.len() > 25 * 1024 * 1024 {
        return Err("Terminal drops require regular files up to 25 MiB".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "The dropped file could not be read")?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("The dropped file has no supported name")?;
    let mut gateway = Gateway::connect(connection)?;
    let result = gateway.page_request(
        profile,
        &PageRequest::post("/api/chat/file-upload", Value::Null)
            .upload(name, "application/octet-stream", bytes)
            .timeout_secs(180),
    )?;
    result
        .get("path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && !s.contains(['\0', '\r', '\n']))
        .map(str::to_owned)
        .ok_or("The remote upload returned no safe path".into())
}
