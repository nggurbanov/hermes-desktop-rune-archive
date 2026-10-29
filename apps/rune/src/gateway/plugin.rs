//! A plugin's event tap is an authenticated, persistent owner-bound socket.
use super::*;
pub(crate) mod sockets;
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
pub struct PluginFeed {
    stop: Arc<AtomicBool>,
    receive: mpsc::Receiver<Result<Value, String>>,
    responses: mpsc::Sender<Value>,
    profiles: Arc<std::sync::RwLock<BTreeSet<String>>>,
}
impl Drop for PluginFeed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl PluginFeed {
    pub fn start(
        connection: Connection,
        profiles: BTreeSet<String>,
        connection_id: String,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (send, receive) = mpsc::channel();
        let (responses, replies) = mpsc::channel::<Value>();
        let profiles = Arc::new(std::sync::RwLock::new(profiles));
        let served = profiles.clone();
        std::thread::spawn(move || {
            let mut attempts = 0u32;
            while !stopped.load(Ordering::Acquire) {
                let mut gateway = match Gateway::connect(&connection) {
                    Ok(gateway) => gateway,
                    Err(_) => {
                        let _ = send.send(Err("The plugin event connection is unavailable".into()));
                        if !pause(&stopped, attempts) {
                            break;
                        }
                        attempts = (attempts + 1).min(4);
                        continue;
                    }
                };
                attempts = 0;
                let _ = send.send(Ok(json!({"type":"native.plugin.state","state":"open"})));
                let mut pending =
                    std::collections::BTreeMap::<String, (Value, std::time::Instant)>::new();
                while !stopped.load(Ordering::Acquire) {
                    if socket_timeout(gateway.socket.get_mut(), Duration::from_secs(1)).is_err() {
                        break;
                    }
                    pending.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(600));
                    for reply in replies.try_iter() {
                        let token = reply["token"].as_str().unwrap_or_default();
                        if let Some((id, _)) = pending.remove(token) {
                            let frame = if reply.get("error").is_some() {
                                json!({"jsonrpc":"2.0","id":id,"error":reply["error"]})
                            } else {
                                json!({"jsonrpc":"2.0","id":id,"result":reply["result"]})
                            };
                            if gateway
                                .socket
                                .send(Message::Text(frame.to_string().into()))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    if socket_timeout(gateway.socket.get_mut(), Duration::from_secs(1)).is_err() {
                        break;
                    }
                    match gateway.socket.read() {
                        Ok(Message::Text(raw)) => {
                            let Ok(frame) = serde_json::from_str::<Value>(&raw) else {
                                continue;
                            };
                            let profiles = served.read().unwrap();
                            if let Some(event) = normalize_event(
                                &frame,
                                &gateway.initial_profile,
                                &profiles,
                                &connection_id,
                            ) {
                                if send.send(Ok(event)).is_err() {
                                    return;
                                }
                            } else if (frame["id"].is_string() || frame["id"].is_number())
                                && frame["method"].is_string()
                            {
                                let owner = frame["params"]["profile"]
                                    .as_str()
                                    .or(frame["params"]["ref"]["profile"].as_str())
                                    .unwrap_or(&gateway.initial_profile);
                                if profiles.contains(owner) {
                                    let token = uuid::Uuid::new_v4().to_string();
                                    pending.insert(
                                        token.clone(),
                                        (frame["id"].clone(), std::time::Instant::now()),
                                    );
                                    let request = json!({"type":"native.plugin.request","request":{"token":token,"id":frame["id"],"method":frame["method"],"params":frame["params"],"profile":owner,"connectionId":connection_id}});
                                    if send.send(Ok(request)).is_err() {
                                        return;
                                    }
                                } else {
                                    let _=gateway.socket.send(Message::Text(json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32601,"message":"The request owner is not served by this plugin window"}}).to_string().into()));
                                }
                            }
                        }
                        Ok(Message::Ping(_)) => {
                            if gateway.socket.flush().is_err() {
                                break;
                            }
                        }
                        Ok(Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(error))
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => break,
                    }
                }
                if !stopped.load(Ordering::Acquire) {
                    let _ = send.send(Err(
                        "The plugin event stream disconnected; reconnecting".into()
                    ));
                    if !pause(&stopped, attempts) {
                        break;
                    }
                }
            }
        });
        Self {
            stop,
            receive,
            responses,
            profiles,
        }
    }
    pub fn update_profiles(&self, profiles: BTreeSet<String>) {
        *self.profiles.write().unwrap() = profiles;
    }
    pub fn respond(&self, response: Value) -> Result<(), String> {
        self.responses
            .send(response)
            .map_err(|_| "The plugin event connection is closed".into())
    }
    pub fn close(&self) {
        self.stop.store(true, Ordering::Release);
    }
    pub fn poll(&mut self) -> Vec<Result<Value, String>> {
        self.receive.try_iter().collect()
    }
}
fn pause(stop: &AtomicBool, attempt: u32) -> bool {
    for _ in 0..(10u32 << attempt.min(4)) {
        if stop.load(Ordering::Acquire) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}
pub(crate) fn normalize_event(
    frame: &Value,
    launch_profile: &str,
    profiles: &BTreeSet<String>,
    connection_id: &str,
) -> Option<Value> {
    let mut event = if frame["method"] == "event" {
        frame.get("params")?.clone()
    } else {
        frame.clone()
    };
    event["type"].as_str()?;
    let owner = event["profile"]
        .as_str()
        .or(event["payload"]["profile"].as_str())
        .or(event["ref"]["profile"].as_str())
        .unwrap_or(launch_profile);
    if !profiles.contains(owner) {
        return None;
    }
    let owner = owner.to_owned();
    event["profile"] = owner.into();
    event["connectionId"] = connection_id.into();
    Some(event)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_owners_follow_the_socket_launch_or_explicit_profile_and_never_an_unserved_target() {
        let profiles = ["research".into()].into_iter().collect();
        assert!(normalize_event(&json!({"method":"event","params":{"type":"message.delta","payload":{"text":"hidden"}}}),"default",&profiles,"source").is_none());
        let event=normalize_event(&json!({"method":"event","params":{"type":"message.delta","profile":"research","payload":{"text":"visible"}}}),"default",&profiles,"source").unwrap();
        assert_eq!(event["profile"], "research");
        assert_eq!(event["connectionId"], "source");
        assert_eq!(event["payload"]["text"], "visible");
    }
    #[test]
    fn persistent_plugin_socket_delivers_owned_events_and_answers_only_its_request_receipt() {
        use std::net::TcpListener;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (verified, receipt) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            for frame in [
                json!({"jsonrpc":"2.0","method":"event","params":{"type":"message.delta","profile":"other","payload":{"text":"outside"}}}),
                json!({"jsonrpc":"2.0","method":"event","params":{"type":"message.delta","profile":"research","session_id":"synthetic","seq":1,"payload":{"text":"visible"}}}),
                json!({"jsonrpc":"2.0","id":"synthetic-request","method":"proof.input","params":{"profile":"research","question":"Synthetic question"}}),
            ] {
                socket
                    .send(Message::Text(frame.to_string().into()))
                    .unwrap();
            }
            loop {
                if let Message::Text(raw) = socket.read().unwrap() {
                    let reply: Value = serde_json::from_str(&raw).unwrap();
                    verified.send(reply).unwrap();
                    break;
                }
            }
        });
        let mut feed = PluginFeed::start(
            Connection::Url {
                endpoint: origin,
                token: Some("synthetic-token".into()),
            },
            ["research".into()].into_iter().collect(),
            "synthetic-owner".into(),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let mut visible = false;
        let mut replied = false;
        while std::time::Instant::now() < deadline && (!visible || !replied) {
            for event in feed.poll().into_iter().flatten() {
                if event["type"] == "message.delta" {
                    assert_eq!(event["profile"], "research");
                    assert_eq!(event["connectionId"], "synthetic-owner");
                    visible = event["payload"]["text"] == "visible";
                }
                if event["type"] == "native.plugin.request" {
                    assert_eq!(event["request"]["profile"], "research");
                    feed.respond(json!({"token":"not-a-receipt","result":{"forged":true}}))
                        .unwrap();
                    feed.respond(
                        json!({"token":event["request"]["token"],"result":{"accepted":true}}),
                    )
                    .unwrap();
                    replied = true;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(visible && replied);
        let reply = receipt.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(reply["id"], "synthetic-request");
        assert_eq!(reply["result"], json!({"accepted":true}));
        feed.close();
        server.join().unwrap();
    }
}
