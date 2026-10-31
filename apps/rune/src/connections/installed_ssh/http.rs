//! Proof that a forwarded listener serves the expected dashboard.
use super::Cancellation;
use regex::Regex;
use reqwest::blocking::Client;
use serde_json::Value;
use std::{
    io::Read,
    net::{Ipv4Addr, TcpStream},
    time::Duration,
};
use tungstenite::{client::IntoClientRequest, protocol::Message};
use url::Url;

/// A credential is returned only to the caller that owns the live forward.
/// This type deliberately does not implement Debug or serialization.
pub(super) struct Verified {
    pub token: String,
}

/// Isolated servers must prove their exact spawn nonce over their authenticated
/// endpoint. A canonical server has no app nonce; its live websocket is the
/// service proof and its lifecycle remains externally owned.
pub(super) fn verify(
    port: u16,
    spawn_token: Option<&str>,
    owner_nonce: Option<&str>,
    cancel: &Cancellation,
) -> Result<Verified, String> {
    check_cancel(cancel)?;
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not construct dashboard probe")?;
    let base = format!("http://127.0.0.1:{port}");
    let mut response = client
        .get(format!("{base}/"))
        .send()
        .map_err(|_| "Could not read the forwarded dashboard")?;
    if !response.status().is_success() {
        return Err("Dashboard index rejected the probe".into());
    }
    let mut html = String::new();
    response
        .by_ref()
        .take(1_048_577)
        .read_to_string(&mut html)
        .map_err(|_| "Could not read the dashboard index")?;
    if html.len() > 1_048_576 {
        return Err("Dashboard index exceeded limit".into());
    }
    let regex = Regex::new(r#"window\.__HERMES_SESSION_TOKEN__\s*=\s*("(?:\\.|[^"\\])*")"#)
        .expect("constant token regex");
    let served = regex
        .captures(&html)
        .and_then(|captures| serde_json::from_str::<String>(&captures[1]).ok());
    let token = served
        .filter(|value| !value.is_empty())
        .or_else(|| spawn_token.map(str::to_owned))
        .ok_or("Dashboard did not provide a session credential")?;
    check_cancel(cancel)?;
    if let Some(nonce) = owner_nonce {
        let mut response = client
            .get(format!("{base}/api/ssh/ownership"))
            .header("X-Hermes-Session-Token", &token)
            .send()
            .map_err(|_| "Could not verify dashboard ownership")?;
        if !response.status().is_success() {
            return Err("Dashboard ownership proof was rejected".into());
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|_| "Invalid dashboard ownership proof")?;
        if bytes.len() > 65_536 {
            return Err("Dashboard ownership proof exceeded limit".into());
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid dashboard ownership proof")?;
        if body.get("ok").and_then(Value::as_bool) != Some(true)
            || body.get("sshOwnerNonce").and_then(Value::as_str) != Some(nonce)
            || body.get("protocolVersion").and_then(Value::as_u64) != Some(1)
            || body.get("runtimeIntact").and_then(Value::as_bool) == Some(false)
        {
            return Err("Dashboard ownership proof did not match this generation".into());
        }
    }
    check_cancel(cancel)?;
    let mut url = Url::parse(&format!("ws://127.0.0.1:{port}/api/ws"))
        .map_err(|_| "Invalid dashboard websocket")?;
    url.query_pairs_mut().append_pair("token", &token);
    let tcp = TcpStream::connect_timeout(
        &((Ipv4Addr::LOCALHOST, port).into()),
        Duration::from_secs(2),
    )
    .map_err(|_| "Could not connect to dashboard websocket")?;
    tcp.set_read_timeout(Some(Duration::from_millis(750)))
        .map_err(|_| "Could not bound dashboard websocket")?;
    tcp.set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| "Could not bound dashboard websocket")?;
    let request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "Invalid dashboard websocket request")?;
    let (mut websocket, _) = tungstenite::client(request, tcp)
        .map_err(|_| "Dashboard websocket rejected authentication")?;
    match websocket.read() {
        Ok(Message::Close(_)) => {
            return Err("Dashboard websocket closed during authentication".into());
        }
        Ok(_) => {}
        Err(tungstenite::Error::Io(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        Err(_) => return Err("Dashboard websocket closed during authentication".into()),
    }
    check_cancel(cancel)?;
    Ok(Verified { token })
}
fn check_cancel(cancel: &Cancellation) -> Result<(), String> {
    if cancel.cancelled() {
        Err("SSH operation cancelled".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    fn dashboard(nonce: &'static str) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let thread = thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut first = [0; 4096];
                let length = stream.peek(&mut first).unwrap();
                let first = String::from_utf8_lossy(&first[..length]);
                if first.starts_with("GET /api/ws?") {
                    let mut ws = tungstenite::accept_hdr(stream, |request: &tungstenite::handshake::server::Request, response: tungstenite::handshake::server::Response| {
                        assert!(request.uri().query().unwrap().contains("token=served-token"));
                        Ok(response)
                    }).unwrap();
                    ws.send(Message::Text("ready".into())).unwrap();
                    continue;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    request.push_str(&line);
                }
                let body = if request.starts_with("GET /api/ssh/ownership") {
                    assert!(
                        request
                            .to_ascii_lowercase()
                            .contains("x-hermes-session-token: served-token")
                    );
                    format!(r#"{{"ok":true,"sshOwnerNonce":"{nonce}","protocolVersion":1}}"#)
                } else {
                    r#"<script>window.__HERMES_SESSION_TOKEN__ = "served-token"</script>"#.into()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        (port, thread)
    }
    #[test]
    fn adopts_served_token_and_verifies_nonce_and_websocket() {
        let (port, server) = dashboard("generation-one");
        let proof = verify(
            port,
            Some("spawn-token"),
            Some("generation-one"),
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(proof.token, "served-token");
        server.join().unwrap();
    }
    #[test]
    fn wrong_nonce_is_rejected_before_websocket() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    request.push_str(&line);
                }
                let body = if request.starts_with("GET /api/ssh/ownership") {
                    r#"{"ok":true,"sshOwnerNonce":"other","protocolVersion":1}"#
                } else {
                    r#"window.__HERMES_SESSION_TOKEN__ = "served-token""#
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        assert!(
            verify(
                port,
                Some("spawn-token"),
                Some("generation-one"),
                &Cancellation::default()
            )
            .is_err()
        );
        server.join().unwrap();
    }
}
