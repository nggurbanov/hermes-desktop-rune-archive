use super::*;
use crate::{
    gateway::{Gateway, roster, stream},
    pages::PageRequest,
};
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, atomic::AtomicBool},
    thread::{self, JoinHandle},
    time::Duration,
};
use tungstenite::handshake::server::{Request, Response};

fn owned_websocket(
    listener: &TcpListener,
    name: &str,
    path: &str,
) -> tungstenite::WebSocket<TcpStream> {
    tungstenite::accept_hdr(accept(listener), |request: &Request, response: Response| {
        let url = Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
        assert_eq!(url.path(), path);
        let pairs = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(pairs["token"], format!("session-{name}"));
        assert_eq!(pairs["profile"], "work");
        assert_eq!(
            request.headers()["x-access-fixture"],
            format!("access-{name}")
        );
        assert!(!request.headers().contains_key("x-hermes-session-token"));
        Ok(response)
    })
    .unwrap()
}

fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "The expected private request never arrived"
                );
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("Private fixture accept failed: {error}"),
        }
    }
}
fn listener() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}
fn connection(listener: &TcpListener, name: &str) -> Connection {
    Connection::Routed {
        owner: Arc::new(crate::source::RouteOwner::legacy(format!("private-test-{name}")).unwrap()),
        actual: Box::new(Connection::Url {
            endpoint: format!("http://{}/hermes/", listener.local_addr().unwrap()),
            token: Some(format!("session-{name}")),
        }),
        policy: Arc::new(
            WirePolicy::token(
                ProfilePolicy::identity(),
                BTreeMap::from([("X-Access-Fixture".into(), format!("access-{name}"))]),
            )
            .unwrap(),
        ),
    }
}
fn websocket(listener: &TcpListener, name: &str) -> tungstenite::WebSocket<TcpStream> {
    tungstenite::accept_hdr(accept(listener), |request: &Request, response: Response| {
        let url = Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
        assert_eq!(url.path(), "/hermes/api/ws");
        assert_eq!(
            url.query_pairs().find(|(key, _)| key == "token").unwrap().1,
            format!("session-{name}")
        );
        assert_eq!(
            request.headers()["x-access-fixture"],
            format!("access-{name}")
        );
        assert!(!request.headers().contains_key("x-hermes-session-token"));
        Ok(response)
    })
    .unwrap()
}
fn request(stream: &mut TcpStream, name: &str) -> (String, Url, Vec<u8>) {
    let mut reader = BufReader::new(stream);
    let mut lines = Vec::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
        lines.push(line);
    }
    let mut words = lines[0].split_whitespace();
    let method = words.next().unwrap().to_owned();
    let url = Url::parse(&format!("http://fixture{}", words.next().unwrap())).unwrap();
    let headers = lines[1..]
        .iter()
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(headers["x-hermes-session-token"], format!("session-{name}"));
    assert_eq!(headers["x-access-fixture"], format!("access-{name}"));
    assert!(!url.query_pairs().any(|(key, _)| key == "token"));
    assert!(!headers.contains_key("authorization"));
    let mut body = vec![0; length];
    use std::io::Read;
    reader.read_exact(&mut body).unwrap();
    (method, url, body)
}
fn reply(stream: &mut TcpStream, status: &str, headers: &str, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
}
fn fixture(name: &'static str) -> (Connection, JoinHandle<()>) {
    let listener = listener();
    let connection = connection(&listener, name);
    let task = thread::spawn(move || {
        let _socket = websocket(&listener, name);
        let mut config = accept(&listener);
        let (method, url, body) = request(&mut config, name);
        assert_eq!(method, "POST");
        assert_eq!(url.path(), "/hermes/api/config");
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "profile")
                .unwrap()
                .1,
            "work"
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            json!({"changes":{"note":"work","nested":{"profile":"literal-user-text"}}})
        );
        reply(
            &mut config,
            "200 OK",
            "Content-Type: application/json\r\n",
            json!({"ok":true,"source":name}).to_string().as_bytes(),
        );
        let mut history = accept(&listener);
        let (method, url, _) = request(&mut history, name);
        assert_eq!(method, "GET");
        assert_eq!(url.path(), "/hermes/api/sessions/same-session/messages");
        assert!(
            url.query_pairs()
                .any(|(key, value)| key == "profile" && value == "work")
        );
        reply(
            &mut history,
            "200 OK",
            "Content-Type: application/json\r\n",
            json!({"profile":"work","session_id":"same-session","messages":[{"role":"assistant","text":name}]}).to_string().as_bytes(),
        );
        let mut media = accept(&listener);
        let (_, url, _) = request(&mut media, name);
        assert_eq!(url.path(), "/hermes/api/fs/download");
        let query = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(query["profile"], "work");
        assert_eq!(query["session_id"], "same-session");
        assert_eq!(query["path"], "/same-file.png");
        reply(
            &mut media,
            "200 OK",
            "Content-Type: image/png\r\n",
            name.as_bytes(),
        );
        let mut roster = accept(&listener);
        let (method, url, _) = request(&mut roster, name);
        assert_eq!(method, "GET");
        assert_eq!(url.path(), "/hermes/api/profiles");
        reply(
            &mut roster,
            "200 OK",
            "Content-Type: application/json\r\n",
            json!({"profiles":[{"name":"work","display_name":name}]})
                .to_string()
                .as_bytes(),
        );
    });
    (connection, task)
}

#[test]
fn actual_workspace_media_and_passive_roster_keep_each_captured_private_transport() {
    let (a, server_a) = fixture("source-a");
    let (b, server_b) = fixture("source-b");
    for (connection, name) in [(&a, "source-a"), (&b, "source-b")] {
        let mut gateway = Gateway::connect(connection).unwrap();
        let config = gateway
            .page_request(
                "work",
                &PageRequest::post(
                    "/api/config",
                    json!({"changes":{"note":"work","nested":{"profile":"literal-user-text"}}}),
                ),
            )
            .unwrap();
        assert_eq!(config["source"], name);
        assert_eq!(
            gateway.history("work", "same-session").unwrap().messages[0].content,
            name
        );
        let mut bytes = Vec::new();
        let media = gateway
            .stream_file(
                "work",
                &stream::Request::download(
                    "/same-file.png".into(),
                    Some("same-session".into()),
                    1024,
                ),
                &stream::Cancel::default(),
                &mut bytes,
            )
            .unwrap();
        assert_eq!(media.status, 200);
        assert_eq!(bytes, name.as_bytes());
        let roster = roster::read(
            connection,
            Instant::now() + Duration::from_secs(3),
            false,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(
            roster
                .profiles
                .iter()
                .map(|profile| profile.name.as_str())
                .collect::<Vec<_>>(),
            ["default", "work"]
        );
        assert_eq!(roster.profiles[1].gateway_profile().label, name);
    }
    server_a.join().unwrap();
    server_b.join().unwrap();
}

#[test]
fn authenticated_workspace_media_and_roster_never_follow_a_foreign_redirect() {
    let foreign = listener();
    let target = format!("http://{}/steal", foreign.local_addr().unwrap());
    let source = listener();
    let connection = connection(&source, "redirect");
    let task = thread::spawn(move || {
        let _socket = websocket(&source, "redirect");
        for path in [
            "/hermes/api/config",
            "/hermes/api/fs/download",
            "/hermes/api/profiles",
        ] {
            let mut stream = accept(&source);
            let (_, url, _) = request(&mut stream, "redirect");
            assert_eq!(url.path(), path);
            reply(
                &mut stream,
                "302 Found",
                &format!("Location: {target}\r\n"),
                b"",
            );
        }
    });
    let mut gateway = Gateway::connect(&connection).unwrap();
    assert!(
        gateway
            .page_request("work", &PageRequest::get("/api/config"))
            .unwrap_err()
            .contains("302")
    );
    let mut bytes = Vec::new();
    let media = gateway
        .stream_file(
            "work",
            &stream::Request::download("/a.png".into(), None, 1024),
            &stream::Cancel::default(),
            &mut bytes,
        )
        .unwrap();
    assert_eq!(media.status, 302);
    assert!(bytes.is_empty());
    assert!(
        roster::read(
            &connection,
            Instant::now() + Duration::from_secs(3),
            false,
            Arc::new(AtomicBool::new(false))
        )
        .err()
        .unwrap()
        .contains("302")
    );
    task.join().unwrap();
    assert!(
        matches!(foreign.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}

#[test]
fn actual_audio_terminal_and_sdk_sockets_use_the_same_captured_auth_envelope() {
    use crate::gateway::{
        audio::{AudioGateway, AudioStream},
        plugin::sockets::SocketBroker,
        terminal::HostTerminal,
    };
    let source = listener();
    let connection = connection(&source, "sockets");
    let task = thread::spawn(move || {
        let _audio_root = websocket(&source, "sockets");
        let mut lease = accept(&source);
        let (_, url, body) = request(&mut lease, "sockets");
        assert_eq!(url.path(), "/hermes/api/audio/tts-lease");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            json!({"lease":"private-wire-fixture","active":true})
        );
        reply(
            &mut lease,
            "200 OK",
            "Content-Type: application/json\r\n",
            b"{\"ok\":true}",
        );
        let mut speech = owned_websocket(&source, "sockets", "/hermes/api/audio/speak-stream");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&speech.read().unwrap().into_text().unwrap())
                .unwrap(),
            json!({"text":"private wire payload only"})
        );
        let _terminal_root = websocket(&source, "sockets");
        let mut terminal = owned_websocket(&source, "sockets", "/hermes/api/host-terminal");
        assert_eq!(
            terminal.read().unwrap().into_text().unwrap(),
            "private terminal fixture"
        );
        let _sdk_root = websocket(&source, "sockets");
        let mut sdk = owned_websocket(&source, "sockets", "/hermes/api/plugins/wire/events");
        sdk.send(tungstenite::Message::Text("private sdk fixture".into()))
            .unwrap();
        assert_eq!(
            sdk.read().unwrap().into_text().unwrap(),
            "private sdk acknowledgement"
        );
        sdk.close(None).unwrap();
    });
    let mut audio = AudioGateway::connect(&connection, "work").unwrap();
    assert_eq!(
        audio.lease(false, "private-wire-fixture", true).unwrap(),
        None
    );
    let mut speech = audio.stream(AudioStream::Speak, 0).unwrap();
    speech.push_text("private wire payload only").unwrap();
    let mut terminal =
        HostTerminal::connect(&connection, "work", "/private-fixture", 80, 24, None, false)
            .unwrap();
    terminal.write(b"private terminal fixture").unwrap();
    let mut broker = SocketBroker::default();
    let prepared = broker.prepare("work", "wire", "/events").unwrap();
    let id = prepared["receipt"].as_str().unwrap();
    broker.open(id, connection).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut received = false;
    while Instant::now() < deadline && !received {
        for event in broker.poll() {
            assert_ne!(event["event"], "error", "{event}");
            if event["event"] == "message" {
                assert_eq!(event["data"]["text"], "private sdk fixture");
                received = true;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(
        received,
        "The actual SDK namespace socket did not deliver its frame"
    );
    broker
        .command(id, json!({"text":"private sdk acknowledgement"}))
        .unwrap();
    task.join().unwrap();
}

#[test]
fn raw_backend_profile_proof_does_not_infer_or_relabel_an_active_root() {
    for active in [None, Some(json!("dixie")), Some(json!(17))] {
        let source = listener();
        let connection = connection(&source, "backend-proof");
        let returned = active.clone();
        let task = thread::spawn(move || {
            let mut socket = websocket(&source, "backend-proof");
            let request = crate::gateway::tests::request(&mut socket, "profiles.list", "default");
            let mut value = json!({"profiles":[{"name":"default"},{"name":"dixie"}]} );
            if let Some(active) = returned {
                value["active"] = active;
            }
            crate::gateway::tests::answer(&mut socket, &request, value);
        });
        let mut gateway = Gateway::connect(&connection).unwrap();
        let proof = gateway.backend_profiles();
        match active {
            Some(serde_json::Value::Number(_)) => assert!(
                proof
                    .err()
                    .unwrap()
                    .contains("active backend profile evidence")
            ),
            active => {
                let proof = proof.unwrap();
                assert_eq!(
                    proof
                        .profiles
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>(),
                    ["default", "dixie"]
                );
                assert_eq!(
                    proof.active,
                    active.and_then(|value| value.as_str().map(str::to_owned))
                );
            }
        }
        task.join().unwrap();
    }
}

#[test]
fn explicit_same_socket_root_evidence_keeps_unmapped_home_and_rejects_malformed_authority() {
    let rows = json!([
        {"name":"default","path":"/remote/root"},
        {"name":"dixie","path":"/remote/dixie"}
    ]);
    let description =
        json!({"profile_id":"/remote/root","instance_id":"private-runtime","authority_epoch":7});
    let mut unmapped = description.clone();
    unmapped["profile_id"] = json!("/remote/custom");
    let mut windows = description.clone();
    windows["profile_id"] = json!("C:\\Private\\Hermes");
    let mut relative = description.clone();
    relative["profile_id"] = json!("default");
    let mut bad_epoch = description.clone();
    bad_epoch["authority_epoch"] = json!(-1);
    let mut bad_instance = description.clone();
    bad_instance["instance_id"] = json!("bad\ninstance");
    for (rows, described, expected) in [
        (rows.clone(), description.clone(), Ok(Some("default"))),
        (rows.clone(), unmapped, Ok(None)),
        (
            json!([{"name":"default","path":"/remote/root"},{"name":"dixie","path":"/remote/root"}]),
            description,
            Ok(None),
        ),
        (
            json!([{"name":"dixie","path":"C:\\Private\\Hermes"}]),
            windows,
            Ok(Some("dixie")),
        ),
        (rows.clone(), relative, Err(())),
        (rows.clone(), bad_epoch, Err(())),
        (rows, bad_instance, Err(())),
    ] {
        let source = listener();
        let connection = connection(&source, "root-proof");
        let expected_description = described.clone();
        let task = thread::spawn(move || {
            let mut socket = websocket(&source, "root-proof");
            let request = crate::gateway::tests::request(&mut socket, "profiles.list", "default");
            crate::gateway::tests::answer(
                &mut socket,
                &request,
                json!({"profiles":rows,"active":"dixie"}),
            );
            let request: serde_json::Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(request["method"], "runtime.describe");
            assert_eq!(
                request["params"],
                json!({}),
                "Root authority must not inherit a profile selector"
            );
            crate::gateway::tests::answer(&mut socket, &request, described);
        });
        let mut gateway = Gateway::connect(&connection).unwrap();
        assert!(
            gateway.backend_root_proof().is_err(),
            "Unread profile paths are not root-name evidence"
        );
        let inventory = gateway.backend_profiles().unwrap();
        assert_eq!(inventory.active.as_deref(), Some("dixie"));
        assert_eq!(inventory.homes.len(), inventory.profiles.len());
        let proof = gateway.backend_root_proof();
        match expected {
            Err(()) => assert!(proof.is_err()),
            Ok(expected) => {
                let proof = proof.unwrap();
                assert_eq!(proof.profile.as_deref(), expected);
                assert_eq!(
                    proof.profile_id,
                    expected_description["profile_id"].as_str().unwrap()
                );
                assert_eq!(proof.instance_id, "private-runtime");
                assert_eq!(proof.authority_epoch, 7);
            }
        }
        task.join().unwrap();
    }
}
