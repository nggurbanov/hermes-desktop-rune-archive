use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
};
use tungstenite::handshake::server::{Request, Response};

fn request(socket: &mut WebSocket<TcpStream>, method: &str, profile: &str) -> Value {
    let message = socket.read().unwrap().into_text().unwrap();
    let request: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(request["method"], method);
    assert_eq!(request["params"]["profile"], profile);
    request
}

fn answer(socket: &mut WebSocket<TcpStream>, request: &Value, result: Value) {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0", "id": request["id"], "result": result})
                .to_string()
                .into(),
        ))
        .unwrap();
}

fn http_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut header = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line.is_empty() {
            break;
        }
        header.push_str(&line);
    }
    header
}

fn http_answer(stream: &mut TcpStream, value: Value) {
    let body = value.to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
}

#[test]
fn remote_reads_preserve_scope_identity_and_ignore_unrelated_rpc_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
            let url = Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
            assert_eq!(url.path(), "/api/ws");
            assert_eq!(
                url.query_pairs().find(|(k, _)| k == "token").unwrap().1,
                "fixture & token"
            );
            Ok(response)
        })
        .unwrap();
        let profiles = request(&mut ws, "profiles.list", "default");
        assert_eq!(profiles["params"]["include_sessions"], false);
        ws.send(Message::Text(
            json!({"id": 999, "result": {"profiles":[]}})
                .to_string()
                .into(),
        ))
        .unwrap();
        ws.send(Message::Text(
            json!({"method":"session.changed", "params":{}})
                .to_string()
                .into(),
        ))
        .unwrap();
        answer(
            &mut ws,
            &profiles,
            json!({"profiles":[{"name":"work","display_name":"Work agent"}]}),
        );
        let sessions = request(&mut ws, "session.list", "work");
        assert_eq!(sessions["params"]["scope"], "stored");
        assert!(
            (1..=200).contains(&sessions["params"]["limit"].as_u64().unwrap()),
            "Canonical stored list accepts at most 200 recent sessions"
        );
        assert_eq!(sessions["params"]["include_hidden"], false);
        answer(
            &mut ws,
            &sessions,
            json!({"sessions":[{"session_id":"stored/id", "resolved_id":"tip", "title":"Fixture", "started_at":123}]}),
        );
        let (mut http, _) = listener.accept().unwrap();
        let header = http_request(&mut http);
        assert!(header.starts_with("GET /api/sessions/stored%2Fid/messages?"));
        let target = header.split_whitespace().nth(1).unwrap();
        let url = Url::parse(&format!("http://fixture{target}")).unwrap();
        let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["profile"], "work");
        assert_eq!(params["order"], "latest");
        assert_eq!(params["include_compacted"], "true");
        assert_eq!(params["inline_images"], "false");
        assert_eq!(params["token"], "fixture & token");
        http_answer(
            &mut http,
            json!({"profile":"work","session_id":"tip","messages":[
            {"role":"user","content":[{"type":"text","text":"Hello"},{"type":"image_url","image_url":"not-text"}]},
            {"role":"assistant","text":"Answer","content":"wire content"},
            {"role":"system","text":"hidden","display_kind":"hidden"}]}),
        );
    });
    let connection = Connection::Url {
        endpoint,
        token: Some("fixture & token".into()),
    };
    let mut gateway = Gateway::connect(&connection).unwrap();
    assert_eq!(gateway.profiles().unwrap()[0].label, "Work agent");
    let session = &gateway.sessions("work").unwrap()[0];
    assert_eq!(session.id, "stored/id");
    assert_eq!(session.updated, "123");
    let history = gateway.history("work", &session.id).unwrap();
    assert!(!history.has_more);
    assert_eq!(history.messages.len(), 2);
    assert_eq!(history.messages[0].content, "Hello");
    assert_eq!(history.messages[1].content, "Answer");
    assert!(
        gateway
            .rpc("session.resume", json!({"session_id":"x"}))
            .is_err()
    );
    server.join().unwrap();
}

#[test]
fn history_detects_earlier_rows_and_refuses_cross_profile_responses() {
    for mismatch in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ws = tungstenite::accept(stream).unwrap();
            let (mut first, _) = listener.accept().unwrap();
            let _ = http_request(&mut first);
            let messages = vec![json!({"role":"user","text":"fixture"}); HISTORY_LIMIT];
            http_answer(
                &mut first,
                json!({"profile":if mismatch {"other"} else {"work"},"session_id":"s", "messages":messages}),
            );
            if !mismatch {
                let (mut probe, _) = listener.accept().unwrap();
                let header = http_request(&mut probe);
                assert!(header.contains("limit=1&offset=500"));
                http_answer(
                    &mut probe,
                    json!({"profile":"work","session_id":"s","messages":[{"role":"user","text":"earlier"}]}),
                );
            }
        });
        let mut gateway = Gateway::connect(&Connection::Url {
            endpoint,
            token: None,
        })
        .unwrap();
        let history = gateway.history("work", "s");
        if mismatch {
            assert!(history.unwrap_err().contains("profile identity mismatch"));
        } else {
            assert!(history.unwrap().has_more);
        }
        server.join().unwrap();
    }
}

#[test]
fn credentials_never_escape_debug_url_validation_or_server_errors() {
    let secret = "DO_NOT_DISPLAY_this_token";
    let connection = Connection::Url {
        endpoint: format!("http://user:{secret}@host/api/ws?token={secret}"),
        token: Some(secret.into()),
    };
    assert!(!format!("{connection:?}").contains(secret));
    assert!(
        !Gateway::connect(&connection)
            .err()
            .unwrap()
            .contains(secret)
    );
    for endpoint in [
        format!("file:///{secret}"),
        format!("http://host/#{secret}"),
        format!("http://host/?other={secret}"),
    ] {
        assert!(!endpoint_url(&endpoint).unwrap_err().contains(secret));
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let request = request(&mut ws, "profiles.list", "default");
        ws.send(Message::Text(json!({"id":request["id"],"error":{"code":-32000,"message":secret,"data":{"token":secret}}}).to_string().into())).unwrap();
    });
    let mut gateway = Gateway::connect(&Connection::Url {
        endpoint,
        token: Some(secret.into()),
    })
    .unwrap();
    let error = gateway.profiles().unwrap_err();
    assert!(error.contains("-32000"));
    assert!(!error.contains(secret));
    server.join().unwrap();
}

#[test]
fn endpoint_normalization_preserves_reverse_proxy_prefix_without_credentials() {
    let (base, token) = endpoint_url("wss://host.example/prefix/api/ws?token=fixture").unwrap();
    assert_eq!(base.as_str(), "https://host.example/prefix/");
    assert_eq!(token.as_deref(), Some("fixture"));
    assert_eq!(
        base.join("api/sessions/").unwrap().path(),
        "/prefix/api/sessions/"
    );
}

#[test]
fn history_refuses_another_session_and_does_not_echo_http_error_bodies() {
    for error_status in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ws = tungstenite::accept(stream).unwrap();
            let (mut http, _) = listener.accept().unwrap();
            let _ = http_request(&mut http);
            if error_status {
                let body = "private_fixture_token";
                write!(http, "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            } else {
                http_answer(
                    &mut http,
                    json!({"profile":"work","session_id":"another-session","messages":[]}),
                );
            }
        });
        let mut gateway = Gateway::connect(&Connection::Url {
            endpoint,
            token: Some("private_fixture_token".into()),
        })
        .unwrap();
        let error = gateway.history("work", "requested-session").unwrap_err();
        assert!(!error.contains("private_fixture_token"));
        assert!(error.contains(if error_status {
            "HTTP 401"
        } else {
            "History session changed"
        }));
        server.join().unwrap();
    }
}

#[cfg(target_os = "linux")]
#[path = "tests_local.rs"]
mod native;

#[test]
fn server_display_projection_wins_even_when_it_intentionally_hides_all_text() {
    assert_eq!(
        message_text(
            &json!({"display_content":"Visible reply", "text":"model scaffold", "content":"physical"})
        ),
        "Visible reply"
    );
    assert_eq!(
        message_text(&json!({"display_content":"", "text":"must stay hidden"})),
        ""
    );
    assert_eq!(
        message_text(&json!({"display_content":null, "text":"Legacy projection"})),
        "Legacy projection"
    );
}
