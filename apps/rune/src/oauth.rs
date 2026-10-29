//! A one-shot client-loopback callback bridge. Tokens and PKCE stay on Hermes.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

pub struct Loopback {
    pub redirect_uri: String,
    expected: Arc<Mutex<Option<String>>>,
    stopped: Arc<AtomicBool>,
    receive: Receiver<Result<Value, String>>,
}
impl Drop for Loopback {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}
impl Loopback {
    pub fn listen() -> Result<Self, String> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| "The local OAuth callback port could not be opened")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "The local OAuth listener could not be configured")?;
        let port = listener
            .local_addr()
            .map_err(|_| "The local OAuth callback address is unavailable")?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");
        let expected = Arc::new(Mutex::new(None::<String>));
        let stopped = Arc::new(AtomicBool::new(false));
        let (send, receive) = mpsc::channel();
        let expected_thread = expected.clone();
        let stopped_thread = stopped.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(360);
            while !stopped_thread.load(Ordering::Acquire) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        if !peer.ip().is_loopback() {
                            continue;
                        }
                        let expected = expected_thread.lock().unwrap().clone();
                        let result = read_callback(&mut stream, port, expected.as_deref());
                        let accepted = result.is_ok();
                        respond(&mut stream, accepted);
                        if let Ok(value) = result {
                            let _ = send.send(Ok(value));
                            return;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(30))
                    }
                    Err(_) => {
                        let _ =
                            send.send(Err("The local OAuth listener stopped unexpectedly".into()));
                        return;
                    }
                }
            }
            if !stopped_thread.load(Ordering::Acquire) {
                let _ = send.send(Err("Timed out waiting for OAuth authorization".into()));
            }
        });
        Ok(Self {
            redirect_uri,
            expected,
            stopped,
            receive,
        })
    }
    pub fn authorize(&mut self, auth_url: &str) -> Result<(), String> {
        let parsed = url::Url::parse(auth_url)
            .map_err(|_| "The agent returned an invalid OAuth authorization URL")?;
        if !matches!(parsed.scheme(), "https" | "http")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err("The agent returned an unsupported OAuth authorization URL".into());
        }
        let values: Vec<_> = parsed.query_pairs().collect();
        let redirects: Vec<_> = values
            .iter()
            .filter(|(key, _)| key == "redirect_uri")
            .collect();
        if redirects.len() != 1 || redirects[0].1 != self.redirect_uri {
            return Err("Update the Hermes backend to support client-local MCP OAuth callbacks. Its redirect URI did not match this desktop.".into());
        }
        let states: Vec<_> = values.iter().filter(|(key, _)| key == "state").collect();
        if states.len() != 1 || states[0].1.is_empty() {
            return Err("The authorization URL did not contain one CSRF state".into());
        }
        *self.expected.lock().unwrap() = Some(states[0].1.to_string());
        Ok(())
    }
    pub fn poll(&mut self) -> Option<Result<Value, String>> {
        self.receive.try_recv().ok()
    }
}
fn read_callback(stream: &mut TcpStream, port: u16, state: Option<&str>) -> Result<Value, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|_| "Invalid callback socket")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|_| "Invalid callback socket")?;
    let mut bytes = vec![];
    let mut buffer = [0u8; 1024];
    loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|_| "Incomplete callback")?;
        if count == 0 {
            return Err("Incomplete callback".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > 8192 {
            return Err("Oversized callback".into());
        }
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&bytes).map_err(|_| "Invalid callback encoding")?;
    let mut lines = request.split("\r\n");
    let line = lines.next().ok_or("Missing callback request")?;
    let parts: Vec<_> = line.split(' ').collect();
    if parts.len() != 3
        || parts[0] != "GET"
        || !parts[1].starts_with('/')
        || parts[1].starts_with("//")
        || !matches!(parts[2], "HTTP/1.1" | "HTTP/1.0")
    {
        return Err("Unsupported callback request".into());
    }
    let expected_host = format!("127.0.0.1:{port}");
    let hosts: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())
        .collect();
    if hosts.len() != 1 || hosts[0] != expected_host {
        return Err("Callback host mismatch".into());
    }
    parse_callback(parts[1], state.ok_or("The OAuth state is not ready")?)
}
fn parse_callback(target: &str, expected: &str) -> Result<Value, String> {
    let parsed = url::Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| "Invalid callback URL")?;
    if parsed.path() != "/callback" || parsed.fragment().is_some() {
        return Err("Unsupported callback path".into());
    }
    let mut values = serde_json::Map::new();
    for (key, value) in parsed.query_pairs() {
        if !matches!(
            key.as_ref(),
            "state" | "code" | "error" | "iss" | "error_description"
        ) {
            continue;
        }
        if values
            .insert(key.into_owned(), Value::String(value.into_owned()))
            .is_some()
        {
            return Err("Duplicate callback parameter".into());
        }
    }
    let state = values
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if expected.is_empty() || !constant_equal(state.as_bytes(), expected.as_bytes()) {
        return Err("Callback state mismatch".into());
    }
    let code = values
        .get("code")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let error = values
        .get("error")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    if code.is_some() == error.is_some() {
        return Err("The callback must contain one authorization result".into());
    }
    values.remove("error_description");
    Ok(Value::Object(values))
}
fn constant_equal(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for (index, byte) in a.iter().enumerate() {
        diff |= usize::from(*byte ^ b.get(index).copied().unwrap_or(0));
    }
    diff == 0
}
fn respond(stream: &mut TcpStream, accepted: bool) {
    let body = if accepted {
        "<!doctype html><meta charset=utf-8><title>Hermes authorization</title><h1>Authorization received</h1><p>Return to Hermes to finish connecting.</p>"
    } else {
        "<!doctype html><meta charset=utf-8><title>Hermes authorization</title><h1>Callback not accepted</h1><p>Return to Hermes and complete the active authorization flow.</p>"
    };
    let status = if accepted {
        "200 OK"
    } else {
        "400 Bad Request"
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

pub struct McpOAuth {
    pub name: String,
    pub session_id: String,
    pub authorization_url: String,
    pub status: String,
    pub error: Option<String>,
    pub listener: Option<Loopback>,
    pub relayed: bool,
    pub cancel_requested: bool,
}
impl McpOAuth {
    pub fn new(name: String) -> Result<Self, String> {
        Ok(Self {
            name,
            session_id: String::new(),
            authorization_url: String::new(),
            status: "starting".into(),
            error: None,
            listener: Some(Loopback::listen()?),
            relayed: false,
            cancel_requested: false,
        })
    }
    pub fn running(&self) -> bool {
        matches!(self.status.as_str(), "starting" | "pending" | "cancelling")
    }
    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.status = "error".into();
        self.listener = None;
    }
    pub fn params(&self) -> Value {
        json!({"name":self.name,"session_id":self.session_id})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_checks_state_path_duplicates_and_relays_the_issuer() {
        assert!(parse_callback("/callback?code=test&state=wrong", "expected").is_err());
        assert!(parse_callback("/other?code=test&state=expected", "expected").is_err());
        assert!(parse_callback("/callback?code=one&code=two&state=expected", "expected").is_err());
        let value = parse_callback(
            "/callback?code=test&state=expected&iss=https%3A%2F%2Fissuer.invalid",
            "expected",
        )
        .unwrap();
        assert_eq!(value["iss"], "https://issuer.invalid");
        assert_eq!(value["code"], "test");
    }
    #[test]
    fn client_redirect_must_match_before_any_browser_is_opened() {
        let mut listener = Loopback::listen().unwrap();
        let state = uuid::Uuid::new_v4().to_string();
        assert!(listener.authorize("https://issuer.invalid/auth?state=test&redirect_uri=http%3A%2F%2F127.0.0.1%3A1%2Fcallback").is_err());
        let mut url = url::Url::parse("https://issuer.invalid/auth").unwrap();
        url.query_pairs_mut()
            .append_pair("state", &state)
            .append_pair("redirect_uri", &listener.redirect_uri);
        assert!(listener.authorize(url.as_str()).is_ok());
        let port = url::Url::parse(&listener.redirect_uri)
            .unwrap()
            .port()
            .unwrap();
        let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(stream,"GET /callback?code=synthetic-code&state={state} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(!response.contains("synthetic-code"));
        let result = listener
            .receive
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(result["code"], "synthetic-code");
    }
}
