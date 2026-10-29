use super::*;
use std::{
    net::{TcpListener, TcpStream},
    thread,
};
use tungstenite::{Message, WebSocket};
fn request(ws: &mut WebSocket<TcpStream>, method: &str) -> Value {
    let value: Value = serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
    assert_eq!(value["method"], method);
    assert_eq!(value["params"]["profile"], "work");
    value
}
fn answer(ws: &mut WebSocket<TcpStream>, request: &Value, value: Value) {
    ws.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":value})
            .to_string()
            .into(),
    ))
    .unwrap();
}
fn snapshot(generation: u64, yolo: bool) -> Value {
    json!({"session_id":"live","stored_session_id":"stored","revision":2,"execution_generation":generation,"running":false,"info":{"profile_name":"work","desktop_protocol":"hermes-gateway-v1","yolo":yolo}})
}
fn intent() -> SlashIntent {
    SlashIntent {
        id: uuid::Uuid::new_v4().to_string(),
        principal: "test".into(),
        profile: "work".into(),
        stored: Some("stored".into()),
        input: "/yolo off".into(),
        runtime: Some("live".into()),
        revision: Some(2),
        generation: Some(0),
        method: Some("config.set".into()),
        params: json!({"session_id":"live","key":"yolo","value":"0"}),
        attempted: false,
        config_ack: None,
        policy: Default::default(),
    }
}
#[test]
fn canonical_bypass_lost_ack_recovers_by_owned_read_without_another_write() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(0, true));
        let set = request(&mut ws, "config.set");
        assert_eq!(
            set["params"],
            json!({"session_id":"live","key":"yolo","value":"0","profile":"work"})
        );
        drop(ws);
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(1, false));
        let get = request(&mut ws, "config.get");
        assert_eq!(get["params"]["key"], "yolo");
        answer(
            &mut ws,
            &get,
            json!({"key":"yolo","value":"0","scope":"session"}),
        );
    });
    let mut pending = intent();
    let mut saved = Vec::new();
    let persisted = std::cell::RefCell::new(Vec::new());
    let persist = |intent: &SlashIntent| {
        *persisted.borrow_mut() = serde_json::to_vec(intent).unwrap();
        Ok(())
    };
    let mut transport = SessionTransport::connect(&connection, "work").unwrap();
    transport.canonical = true;
    assert!(
        execute(&mut transport, &mut pending, &json!({}), &persist)
            .unwrap_err()
            .uncertain
    );
    saved.extend_from_slice(&persisted.borrow());
    let mut recovered: SlashIntent = serde_json::from_slice(&saved).unwrap();
    let mut transport = SessionTransport::connect(&connection, "work").unwrap();
    transport.canonical = true;
    let value = execute(&mut transport, &mut recovered, &json!({}), &persist).unwrap();
    assert_eq!(value["session_bypass"]["reported_yolo"], false);
    assert_eq!(value["session_bypass"]["execution_generation"], 1);
    server.join().unwrap();
}
#[test]
fn legacy_bypass_uses_exact_ack_no_getter_and_preserves_reported_process_override() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: None,
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(0, true));
        let set = request(&mut ws, "config.set");
        assert_eq!(set["params"]["key"], "yolo");
        assert_eq!(set["params"]["value"], "0");
        assert!(set["params"].get("scope").is_none());
        answer(
            &mut ws,
            &set,
            json!({"key":"yolo","value":"0","scope":"session"}),
        );
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(0, true));
    });
    let mut transport = SessionTransport::connect(&connection, "work").unwrap();
    let mut pending = intent();
    let value = execute(&mut transport, &mut pending, &json!({}), &|_| Ok(())).unwrap();
    assert_eq!(value["session_bypass"]["value"], "0");
    assert_eq!(value["session_bypass"]["reported_yolo"], true);
    assert!(value["warning"].as_str().unwrap().contains("still reports"));
    assert_eq!(pending.config_ack.as_ref().unwrap()["scope"], "session");
    server.join().unwrap();
}
