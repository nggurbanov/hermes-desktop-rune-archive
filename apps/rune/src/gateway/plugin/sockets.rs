//! Plugin namespace streams are dialed natively; the renderer receives an opaque receipt.
use super::super::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
struct Prepared {
    allowed: std::collections::BTreeSet<String>,
    profile: String,
    path: String,
    at: Instant,
    gateway: bool,
    canonical: bool,
    source: Option<String>,
    connection: Option<Connection>,
    lease: Option<crate::connections::Lease>,
}
struct Control {
    allowed: std::collections::BTreeSet<String>,
    source: Option<String>,
    lease: Option<crate::connections::Lease>,
    profile: String,
    gateway: bool,
    stop: Arc<AtomicBool>,
    send: mpsc::Sender<Value>,
}
impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
pub(crate) struct SocketBroker {
    prepared: BTreeMap<String, Prepared>,
    running: BTreeMap<String, Control>,
    send: mpsc::Sender<Value>,
    receive: mpsc::Receiver<Value>,
}
impl Default for SocketBroker {
    fn default() -> Self {
        let (send, receive) = mpsc::channel();
        Self {
            prepared: BTreeMap::new(),
            running: BTreeMap::new(),
            send,
            receive,
        }
    }
}
impl SocketBroker {
    pub fn prepare(&mut self, profile: &str, plugin: &str, path: &str) -> Result<Value, String> {
        valid_scope(profile)?;
        let path = plugin_path(plugin, path)?;
        self.prepared
            .retain(|_, p| p.at.elapsed() < Duration::from_secs(60));
        if self.prepared.len() + self.running.len() >= 128 {
            return Err("Too many plugin streams are open".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.prepared.insert(
            id.clone(),
            Prepared {
                allowed: [profile.into()].into_iter().collect(),
                profile: profile.into(),
                path,
                at: Instant::now(),
                gateway: false,
                canonical: false,
                source: None,
                connection: None,
                lease: None,
            },
        );
        Ok(json!({"receipt":id,"url":format!("ws://hermes-native-plugin.invalid/{id}")}))
    }
    pub fn prepare_gateway(&mut self, profile: &str, canonical: bool) -> Result<Value, String> {
        valid_scope(profile)?;
        self.prepared
            .retain(|_, p| p.at.elapsed() < Duration::from_secs(60));
        if self.prepared.len() + self.running.len() >= 128 {
            return Err("Too many plugin streams are open".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.prepared.insert(
            id.clone(),
            Prepared {
                allowed: [profile.into()].into_iter().collect(),
                profile: profile.into(),
                path: "/api/ws".into(),
                at: Instant::now(),
                gateway: true,
                canonical,
                source: None,
                connection: None,
                lease: None,
            },
        );
        // These markers select the original SDK adapter, and carry no gateway grant.
        let marker = if canonical {
            format!("?native_dial={id}&ticket=parent-owned")
        } else {
            String::new()
        };
        Ok(json!({"receipt":id,"url":format!("ws://hermes-native-gateway.invalid/{id}{marker}")}))
    }
    pub fn bind_prepared(
        &mut self,
        receipt: &str,
        source: &str,
        connection: Connection,
        lease: Option<crate::connections::Lease>,
    ) -> Result<(), String> {
        let prepared = self
            .prepared
            .get_mut(receipt)
            .ok_or("The plugin stream receipt expired")?;
        prepared.source = Some(source.into());
        prepared.connection = Some(connection);
        prepared.lease = lease;
        Ok(())
    }
    pub fn authorize_profiles(
        &mut self,
        receipt: &str,
        profiles: &std::collections::BTreeSet<String>,
    ) {
        if let Some(prepared) = self.prepared.get_mut(receipt) {
            prepared.allowed = profiles.clone();
        }
    }
    pub fn retain_routes(&mut self, sources: &std::collections::BTreeSet<String>) {
        self.prepared
            .retain(|_, p| p.source.as_ref().is_none_or(|id| sources.contains(id)));
        self.running
            .retain(|_, c| c.source.as_ref().is_none_or(|id| sources.contains(id)));
    }
    pub fn retain_source(
        &mut self,
        source: &str,
        lease: Option<crate::connections::Lease>,
        profiles: &std::collections::BTreeSet<String>,
    ) {
        self.prepared
            .retain(|_, p| p.source.as_deref() != Some(source) || profiles.contains(&p.profile));
        self.running
            .retain(|_, c| c.source.as_deref() != Some(source) || profiles.contains(&c.profile));
        for prepared in self
            .prepared
            .values_mut()
            .filter(|p| p.source.as_deref() == Some(source))
        {
            prepared.lease = lease.clone();
            prepared.allowed = profiles.clone();
        }
        for control in self
            .running
            .values_mut()
            .filter(|c| c.source.as_deref() == Some(source))
        {
            control.lease = lease.clone();
            control.allowed = profiles.clone();
        }
    }
    pub fn open(&mut self, id: &str, connection: Connection) -> Result<Value, String> {
        let prepared = self
            .prepared
            .remove(id)
            .filter(|p| p.at.elapsed() < Duration::from_secs(60))
            .ok_or("The plugin stream receipt expired; reconnect it")?;
        if prepared
            .lease
            .as_ref()
            .is_some_and(|lease| !lease.is_valid())
        {
            return Err("The plugin stream source changed before connecting".into());
        }
        let connection = prepared.connection.clone().unwrap_or(connection);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (send, receive) = mpsc::channel::<Value>();
        let output = self.send.clone();
        let id = id.to_owned();
        self.running.insert(
            id.clone(),
            Control {
                allowed: prepared.allowed.clone(),
                source: prepared.source.clone(),
                lease: prepared.lease.clone(),
                profile: prepared.profile.clone(),
                gateway: prepared.gateway,
                stop,
                send,
            },
        );
        std::thread::spawn(move || {
            let emit = |event: &str, data: Value| {
                let _ = output
                    .send(json!({"type":"plugin-socket","receipt":id,"event":event,"data":data}));
            };
            let result = if prepared.gateway {
                dial_gateway(&connection, &prepared.profile)
            } else {
                dial(&connection, &prepared.profile, &prepared.path)
            };
            let mut socket = match result {
                Ok(s) => s,
                Err(error) => {
                    emit("error", json!({"message":error}));
                    emit(
                        "close",
                        json!({"code":1006,"reason":"The plugin stream could not connect","clean":false}),
                    );
                    return;
                }
            };
            if stopped.load(Ordering::Acquire)
                || prepared
                    .lease
                    .as_ref()
                    .is_some_and(|lease| !lease.is_valid())
            {
                let _ = socket.close(None);
                emit(
                    "close",
                    json!({"code":1006,"reason":"The plugin stream source changed","clean":false}),
                );
                return;
            }
            emit(
                "open",
                json!({"protocol":if prepared.canonical {"hermes-gateway-v1"} else {""}}),
            );
            let mut close_code = 1006;
            let mut close_reason = String::new();
            let mut clean = false;
            'stream: while !stopped.load(Ordering::Acquire)
                && !prepared
                    .lease
                    .as_ref()
                    .is_some_and(|lease| !lease.is_valid())
            {
                if socket_timeout(socket.get_mut(), Duration::from_secs(1)).is_err() {
                    break;
                }
                for command in receive.try_iter() {
                    if command["type"] == "close" {
                        let code = command["code"].as_u64().unwrap_or(1000) as u16;
                        let reason = command["reason"].as_str().unwrap_or("").to_owned();
                        let _ = socket.close(Some(tungstenite::protocol::CloseFrame {
                            code: code.into(),
                            reason: reason.clone().into(),
                        }));
                        close_code = code;
                        close_reason = reason;
                        clean = true;
                        break 'stream;
                    }
                    let frame = if let Some(data) = command["text"].as_str() {
                        Message::Text(data.to_owned().into())
                    } else if let Some(encoded) = command["bytes_base64"].as_str() {
                        use base64::Engine;
                        match base64::engine::general_purpose::STANDARD.decode(encoded) {
                            Ok(bytes) => Message::Binary(bytes.into()),
                            Err(_) => continue,
                        }
                    } else {
                        continue;
                    };
                    if socket.send(frame).is_err() {
                        break 'stream;
                    }
                }
                if socket_timeout(socket.get_mut(), Duration::from_secs(1)).is_err() {
                    break;
                }
                match socket.read() {
                    Ok(Message::Text(text)) => emit("message", json!({"text":text.as_str()})),
                    Ok(Message::Binary(bytes)) => {
                        use base64::Engine;
                        emit(
                            "message",
                            json!({"bytes_base64":base64::engine::general_purpose::STANDARD.encode(bytes)}),
                        );
                    }
                    Ok(Message::Ping(_)) => {
                        if socket.flush().is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(frame) = frame {
                            close_code = frame.code.into();
                            close_reason = frame.reason.to_string();
                        } else {
                            close_code = 1000;
                        }
                        clean = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(error))
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
            }
            if stopped.load(Ordering::Acquire) {
                let _ = socket.close(None);
                close_code = 1000;
                clean = true;
            }
            emit(
                "close",
                json!({"code":close_code,"reason":close_reason,"clean":clean}),
            );
        });
        Ok(json!({"accepted":true}))
    }
    pub fn command(&self, id: &str, mut command: Value) -> Result<Value, String> {
        let control = self.running.get(id).ok_or("The plugin stream is closed")?;
        if control
            .lease
            .as_ref()
            .is_some_and(|lease| !lease.is_valid())
        {
            return Err("The plugin stream source changed; reconnect it".into());
        }
        if control.gateway {
            if let Some(raw) = command["text"].as_str() {
                if let Ok(mut frame) = serde_json::from_str::<Value>(raw) {
                    for owner in [
                        frame["params"]["profile"].as_str(),
                        frame["params"]["ref"]["profile"].as_str(),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if !control.allowed.contains(owner) {
                            return Err("The plugin gateway frame names a profile outside its verified source".into());
                        }
                    }

                    if frame["method"].as_str().is_some_and(|method| {
                        !matches!(method, "client.capabilities" | "ping" | "heartbeat")
                    }) {
                        if frame["params"].is_null() {
                            frame["params"] = json!({});
                        }
                        if let Some(params) = frame["params"].as_object_mut() {
                            params
                                .entry("profile")
                                .or_insert_with(|| control.profile.clone().into());
                            command["text"] = frame.to_string().into();
                        }
                    }
                }
            }
        }
        if command.to_string().len() > 16 * 1024 * 1024 {
            return Err("The plugin stream frame exceeds the size limit".into());
        }
        control
            .send
            .send(command)
            .map_err(|_| "The plugin stream is closed")?;
        Ok(json!({"accepted":true}))
    }
    pub fn sync_profiles(&mut self, profiles: &std::collections::BTreeSet<String>) {
        self.prepared
            .retain(|_, p| p.source.is_some() || profiles.contains(&p.profile));
        self.running
            .retain(|_, s| s.source.is_some() || profiles.contains(&s.profile));
    }
    pub fn poll(&mut self) -> Vec<Value> {
        self.prepared
            .retain(|_, p| !p.lease.as_ref().is_some_and(|lease| !lease.is_valid()));
        self.running
            .retain(|_, c| !c.lease.as_ref().is_some_and(|lease| !lease.is_valid()));
        let values: Vec<_> = self.receive.try_iter().collect();
        for value in &values {
            if value["event"] == "close" {
                self.running
                    .remove(value["receipt"].as_str().unwrap_or_default());
            }
        }
        values
    }
}
fn dial_gateway(
    connection: &Connection,
    profile: &str,
) -> Result<WebSocket<MaybeTlsStream<DeadlineStream>>, String> {
    let mut gateway = Gateway::connect(connection)?;
    #[cfg(target_os = "linux")]
    if matches!(gateway.auth, Auth::Native(_)) && gateway.initial_profile != profile {
        gateway.profiles_until(Instant::now() + Duration::from_secs(30))?;
        let home = gateway
            .profile_homes
            .get(profile)
            .ok_or("The plugin gateway profile is not served")?
            .clone();
        return Gateway::connect(&Connection::Local { home }).map(|gateway| gateway.socket);
    }
    let _ = profile;
    Ok(gateway.socket)
}
fn dial(
    connection: &Connection,
    profile: &str,
    path: &str,
) -> Result<WebSocket<MaybeTlsStream<DeadlineStream>>, String> {
    let mut gateway = Gateway::connect(connection)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    #[cfg(target_os = "linux")]
    if matches!(gateway.auth, Auth::Native(_)) {
        gateway.profiles_until(deadline)?;
    }
    let mut url = gateway
        .base
        .join(path.trim_start_matches('/'))
        .map_err(|_| "The plugin stream route is invalid")?;
    url.set_scheme(if gateway.base.scheme() == "https" {
        "wss"
    } else {
        "ws"
    })
    .map_err(|_| "The plugin stream scheme is invalid")?;
    if url
        .query_pairs()
        .any(|(key, _)| key == "token" || key == "profile")
    {
        return Err("Plugin stream paths cannot override credentials or profile".into());
    }
    url.query_pairs_mut().append_pair("profile", profile);
    #[cfg(target_os = "linux")]
    let home = if matches!(&gateway.auth, Auth::Native(_)) {
        Some(
            gateway
                .profile_homes
                .get(profile)
                .ok_or("The plugin stream profile is not served")?
                .as_path(),
        )
    } else {
        None
    };
    #[cfg(not(target_os = "linux"))]
    let home = None;
    let handshake = gateway.prepared.prepare_ws(
        url,
        home,
        super::super::prepared::WsPurpose::Plugin,
        deadline,
    )?;
    let stream = connect_tcp(&handshake.url)?;
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_BODY))
        .max_frame_size(Some(MAX_BODY));
    tungstenite::client_tls_with_config(handshake.request, stream, Some(config), None)
        .map(|(socket, _)| socket)
        .map_err(|_| "The plugin stream handshake failed (network, TLS or permission)".into())
}
fn plugin_path(plugin: &str, path: &str) -> Result<String, String> {
    if plugin.is_empty()
        || plugin.len() > 128
        || !plugin.as_bytes()[0].is_ascii_alphanumeric()
        || plugin.contains("..")
        || !plugin
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err("Invalid plugin stream namespace".into());
    }
    if !path.starts_with('/') || path.len() > 2048 || path.contains(['#', '\\']) {
        return Err("Invalid plugin stream path".into());
    }
    for segment in path.split('?').next().unwrap_or_default().split('/') {
        let bytes = segment.as_bytes();
        let mut decoded = Vec::new();
        let mut n = 0;
        while n < bytes.len() {
            if bytes[n] == b'%' {
                if n + 2 >= bytes.len() {
                    return Err("Invalid plugin stream path encoding".into());
                }
                let hex = std::str::from_utf8(&bytes[n + 1..n + 3])
                    .ok()
                    .and_then(|s| u8::from_str_radix(s, 16).ok())
                    .ok_or("Invalid plugin stream path encoding")?;
                decoded.push(hex);
                n += 3;
            } else {
                decoded.push(bytes[n]);
                n += 1;
            }
        }
        if decoded == b"." || decoded == b".." || decoded.iter().any(|b| b"/\\%".contains(b)) {
            return Err("The plugin stream path escaped its namespace".into());
        }
    }
    let url = url::Url::parse(&format!("ws://plugin.invalid/api/plugins/{plugin}{path}"))
        .map_err(|_| "Invalid plugin stream path")?;
    if url
        .query_pairs()
        .any(|(key, _)| key == "token" || key == "profile")
    {
        return Err("Plugin stream paths cannot override credentials or profile".into());
    }
    Ok(format!(
        "{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_namespaces_reject_encoded_escapes_and_credential_overrides() {
        assert_eq!(
            plugin_path("proof", "/events?board=one").unwrap(),
            "/api/plugins/proof/events?board=one"
        );
        for path in [
            "/../config",
            "/%2e%2e/config",
            "/part%2fother",
            "/%252e/events",
            "/events?token=other",
            "/events?profile=other",
        ] {
            assert!(plugin_path("proof", path).is_err(), "{path}");
        }
        assert!(plugin_path("../proof", "/events").is_err());
    }
    #[test]
    fn namespace_stream_keeps_its_profile_and_credentials_in_the_native_dial() {
        use std::net::TcpListener;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (verified, receive) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (root, _) = listener.accept().unwrap();
            let root = tungstenite::accept(root).unwrap();
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request, response| {
                    let url =
                        url::Url::parse(&format!("http://synthetic{}", request.uri())).unwrap();
                    assert_eq!(url.path(), "/api/plugins/proof/events");
                    assert!(
                        url.query_pairs()
                            .any(|(key, value)| key == "profile" && value == "research")
                    );
                    assert!(
                        url.query_pairs()
                            .any(|(key, value)| key == "token" && value == "synthetic-token")
                    );
                    Ok(response)
                },
            )
            .unwrap();
            drop(root);
            socket
                .send(Message::Text("{\"item\":\"synthetic\"}".into()))
                .unwrap();
            loop {
                if let Message::Text(text) = socket.read().unwrap() {
                    verified.send(text.to_string()).unwrap();
                    break;
                }
            }
            socket.close(None).unwrap();
        });
        let mut broker = SocketBroker::default();
        let prepared = broker
            .prepare("research", "proof", "/events?room=one")
            .unwrap();
        let receipt = prepared["receipt"].as_str().unwrap();
        assert!(
            prepared["url"]
                .as_str()
                .unwrap()
                .starts_with("ws://hermes-native-plugin.invalid/")
        );
        assert!(!prepared.to_string().contains("synthetic-token"));
        broker
            .open(
                receipt,
                Connection::Url {
                    endpoint: origin,
                    token: Some("synthetic-token".into()),
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut message = false;
        while Instant::now() < deadline && !message {
            for event in broker.poll() {
                assert_ne!(event["event"], "error");
                if event["event"] == "message" {
                    assert_eq!(event["data"]["text"], "{\"item\":\"synthetic\"}");
                    message = true;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(message);
        broker
            .command(receipt, json!({"text":"synthetic acknowledgement"}))
            .unwrap();
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(3)).unwrap(),
            "synthetic acknowledgement"
        );
        server.join().unwrap();
    }
    #[test]
    fn multiplexed_gateway_frames_share_one_retained_socket_until_explicit_close() {
        use std::net::TcpListener;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let mut requests = Vec::new();
            while requests.len() < 2 {
                if let Message::Text(raw) = socket.read().unwrap() {
                    let frame: Value = serde_json::from_str(&raw).unwrap();
                    assert_eq!(frame["params"]["profile"], "research");
                    requests.push(frame);
                }
            }
            for request in requests.into_iter().rev() {
                socket.send(Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"result":{"method":request["method"]}}).to_string().into())).unwrap();
            }
            loop {
                match socket.read() {
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
        });
        let mut broker = SocketBroker::default();
        let prepared = broker.prepare_gateway("research", false).unwrap();
        let receipt = prepared["receipt"].as_str().unwrap();
        broker
            .open(
                receipt,
                Connection::Url {
                    endpoint: origin,
                    token: None,
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut opened = false;
        while Instant::now() < deadline && !opened {
            opened = broker.poll().iter().any(|event| event["event"] == "open");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(opened);
        for (id, method) in [(1, "session.create"), (2, "native.proof")] {
            broker.command(receipt,json!({"text":json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"profile":"research"}}).to_string()})).unwrap();
        }
        let mut ids = Vec::new();
        while Instant::now() < deadline && ids.len() < 2 {
            for event in broker.poll() {
                if event["event"] == "message" {
                    let frame: Value =
                        serde_json::from_str(event["data"]["text"].as_str().unwrap()).unwrap();
                    ids.push(frame["id"].as_u64().unwrap());
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(ids, vec![2, 1]);
        broker
            .command(
                receipt,
                json!({"type":"close","code":1000,"reason":"Synthetic completion"}),
            )
            .unwrap();
        server.join().unwrap();
    }
    #[test]
    fn a_prepared_gateway_route_uses_its_frozen_source_after_the_parent_switches() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (verified, receive) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request, response| {
                    let url =
                        url::Url::parse(&format!("http://synthetic{}", request.uri())).unwrap();
                    assert!(
                        url.query_pairs()
                            .any(|(key, value)| key == "token"
                                && value == "source-b-synthetic-token")
                    );
                    Ok(response)
                },
            )
            .unwrap();
            loop {
                if let Message::Text(raw) = socket.read().unwrap() {
                    let frame: Value = serde_json::from_str(&raw).unwrap();
                    assert_eq!(frame["params"]["profile"], "same-name");
                    verified.send(()).unwrap();
                    break;
                }
            }
            socket.close(None).unwrap();
        });
        let mut broker = SocketBroker::default();
        let prepared = broker.prepare_gateway("same-name", false).unwrap();
        let id = prepared["receipt"].as_str().unwrap();
        broker
            .bind_prepared(
                id,
                "source-b",
                Connection::Url {
                    endpoint,
                    token: Some("source-b-synthetic-token".into()),
                },
                None,
            )
            .unwrap();
        broker
            .open(
                id,
                Connection::Url {
                    endpoint: "http://127.0.0.1:1/ambient-a".into(),
                    token: Some("wrong-a".into()),
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut opened = false;
        while Instant::now() < deadline && !opened {
            for event in broker.poll() {
                assert_ne!(event["event"], "error");
                opened |= event["event"] == "open";
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(opened);
        broker.sync_profiles(&["other-ambient-profile".into()].into_iter().collect());
        broker.command(id,json!({"text":json!({"jsonrpc":"2.0","id":1,"method":"native.proof","params":{}}).to_string()})).unwrap();
        receive.recv_timeout(Duration::from_secs(3)).unwrap();
        server.join().unwrap();
    }
}
