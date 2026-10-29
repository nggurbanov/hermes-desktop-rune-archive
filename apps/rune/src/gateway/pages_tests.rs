use super::*;
use crate::pages::PageRequest;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
};
fn request(stream: &mut TcpStream) -> (Url, Value) {
    let mut reader = BufReader::new(stream);
    let mut lines = vec![];
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
        lines.push(line);
    }
    let url = Url::parse(&format!(
        "http://fixture{}",
        lines[0].split_whitespace().nth(1).unwrap()
    ))
    .unwrap();
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).unwrap();
    (
        url,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}
#[test]
fn workspace_writes_preserve_prefix_and_owner_and_do_not_echo_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/hermes", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ws = tungstenite::accept(stream).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let (url, body) = request(&mut stream);
        assert_eq!(url.path(), "/hermes/api/config");
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["profile"], "work");
        assert_eq!(query["token"], "private & token");
        assert_eq!(body, json!({"changes":{"voice.speed":1.25}}));
        write!(stream,"HTTP/1.1 403 Forbidden\r\nContent-Length: 23\r\nConnection: close\r\n\r\nprivate & token refused").unwrap();
    });
    let mut g = Gateway::connect(&Connection::Url {
        endpoint,
        token: Some("private & token".into()),
    })
    .unwrap();
    let error = g
        .page_request(
            "work",
            &PageRequest::post("/api/config", json!({"changes":{"voice.speed":1.25}})),
        )
        .unwrap_err();
    assert!(error.contains("403"));
    assert!(!error.contains("private"));
    assert!(
        g.page_request(
            "work",
            &PageRequest::get("/api/config").query("token", "injected")
        )
        .is_err()
    );
    assert!(
        g.page_request(
            "work",
            &PageRequest::post("/api/config?profile=other", json!({}))
        )
        .is_err()
    );
    assert!(
        g.page_request("work", &PageRequest::rpc("vault.list", Value::Null))
            .is_err()
    );
    server.join().unwrap();
}
#[test]
fn page_rpc_mutations_are_explicit_and_step_up_events_arrive_before_settlement() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let first = super::tests::request(&mut ws, "vault.add", "work");
        assert_eq!(first["params"]["secret"], "synthetic");
        super::tests::answer(&mut ws, &first, json!({"ok":false,"error":"locked"}));
        let step = super::tests::request(&mut ws, "billing.step_up", "work");
        ws.send(Message::Text(json!({"type":"billing.step_up.verification","payload":{"verification_url":"https://example.com/device","user_code":"ABCD"}}).to_string().into())).unwrap();
        super::tests::answer(&mut ws, &step, json!({"ok":true,"granted":true}));
    });
    let mut g = Gateway::connect(&Connection::Url {
        endpoint,
        token: None,
    })
    .unwrap();
    assert_eq!(
        g.page_request(
            "work",
            &PageRequest::rpc("vault.add", json!({"secret":"synthetic","profile":"wrong"}))
        )
        .unwrap()["error"],
        "locked"
    );
    let mut events = vec![];
    assert_eq!(
        g.page_request_events(
            "work",
            &PageRequest::rpc("billing.step_up", json!({})),
            |e| events.push(e)
        )
        .unwrap()["granted"],
        true
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["payload"]["user_code"], "ABCD");
    assert!(
        g.page_request("work", &PageRequest::rpc("terminal.execute", json!({})))
            .is_err()
    );
    server.join().unwrap();
}
#[test]
fn downloads_stream_beyond_json_limit_and_interrupted_transfers_preserve_destination() {
    for interrupted in [false, true] {
        let dir = std::env::temp_dir().join(format!("hermes-download-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("report.bin");
        std::fs::write(&path, b"original").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ws = tungstenite::accept(stream).unwrap();
            let (mut http, _) = listener.accept().unwrap();
            let (url, _) = request(&mut http);
            assert_eq!(url.path(), "/api/fs/download");
            let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(query["profile"], "research");
            assert_eq!(query["session_id"], "owner");
            let size = MAX_BODY + 1024;
            write!(
                http,
                "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            if interrupted {
                http.write_all(b"partial").unwrap();
            } else {
                http.write_all(&vec![7; size]).unwrap();
            }
        });
        let mut g = Gateway::connect(&Connection::Url {
            endpoint,
            token: None,
        })
        .unwrap();
        let result = g.page_request(
            "research",
            &PageRequest::get("/api/fs/download")
                .query("path", "/remote/report.bin")
                .query("session_id", "owner")
                .download_to(path.clone()),
        );
        server.join().unwrap();
        if interrupted {
            assert!(result.is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"original");
        } else {
            assert_eq!(result.unwrap()["downloaded"], true);
            assert_eq!(
                std::fs::metadata(&path).unwrap().len(),
                (MAX_BODY + 1024) as u64
            );
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn plugin_upload_stream_is_multipart_binary_and_keeps_authenticated_owner() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/prefix", listener.local_addr().unwrap());
    let bytes = vec![0, 255, 13, 10, 34, 127];
    let expected = bytes.clone();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ws = tungstenite::accept(stream).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut header = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
            header.push_str(&line);
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        drop(reader);
        assert!(
            header.starts_with("POST /prefix/api/plugins/upload?profile=research&token=fixture")
        );
        assert!(
            header
                .to_lowercase()
                .contains("content-type: multipart/form-data; boundary=")
        );
        assert!(body.windows(expected.len()).any(|s| s == expected));
        assert!(String::from_utf8_lossy(&body).contains("name=\"file\"; filename=\"archive.bin\""));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{{\"ok\":true}}"
        )
        .unwrap();
    });
    let mut gateway = Gateway::connect(&Connection::Url {
        endpoint,
        token: Some("fixture".into()),
    })
    .unwrap();
    let result = gateway
        .page_request(
            "research",
            &PageRequest::post("/api/plugins/upload", Value::Null).upload(
                "archive.bin",
                "application/octet-stream",
                bytes,
            ),
        )
        .unwrap();
    assert_eq!(result["ok"], true);
    server.join().unwrap();
}

#[test]
fn messaging_test_preserves_business_diagnostic_while_other_refusals_fail_closed() {
    for (path, diagnostic) in [
        ("/api/messaging/platforms/telegram/test", true),
        ("/api/config", false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let expected = path.to_owned();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ws = tungstenite::accept(stream).unwrap();
            let (mut http, _) = listener.accept().unwrap();
            let (url, _) = request(&mut http);
            assert_eq!(url.path(), expected);
            assert!(
                url.query_pairs()
                    .any(|(k, v)| k == "profile" && v == "research")
            );
            let body = r#"{"ok":false,"message":"Credentials need attention","state":"error"}"#;
            write!(
                http,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let mut g = Gateway::connect(&Connection::Url {
            endpoint,
            token: None,
        })
        .unwrap();
        let response = g.page_request("research", &PageRequest::post(path, json!({})));
        if diagnostic {
            assert_eq!(response.unwrap()["message"], "Credentials need attention");
        } else {
            assert!(response.is_err());
        }
        server.join().unwrap();
    }
}
