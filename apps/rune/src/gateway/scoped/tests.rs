use super::*;
use crate::{
    gateway::prepared::{SelectedContext, WirePolicy},
    pages::PageRequest,
};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    net::TcpListener,
    sync::{Arc, mpsc},
    thread,
};
use tungstenite::handshake::server::{Request, Response};

fn listener() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}
fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "Missing private request");
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("Private accept: {error}"),
        }
    }
}
fn connection(listener: &TcpListener, target: BackendTarget) -> Connection {
    let alias = match &target {
        BackendTarget::Named(name) => Some(name.clone()),
        BackendTarget::Root(_) => None,
    };
    let policy = route::ProfilePolicy::new(
        "default",
        route::RouteOptions {
            primary_profile: Some("primary".into()),
            profile_remote_override: true,
            backend_profile: alias,
            override_kind: route::OverrideKind::Url,
            ..Default::default()
        },
    )
    .unwrap();
    let policy = WirePolicy::token(
        policy,
        BTreeMap::from([("X-Access-Fixture".into(), "private-access".into())]),
    )
    .unwrap()
    .with_selected(SelectedContext::new("default", target).unwrap())
    .unwrap();
    Connection::Routed {
        actual: Box::new(Connection::Url {
            endpoint: format!("http://{}/hermes/", listener.local_addr().unwrap()),
            token: Some("private-session".into()),
        }),
        policy: Arc::new(policy),
        owner: Arc::new(crate::source::RouteOwner::legacy("private-scope-tests").unwrap()),
    }
}
fn socket(listener: &TcpListener) -> WebSocket<TcpStream> {
    tungstenite::accept_hdr(accept(listener), |request: &Request, response: Response| {
        let url = Url::parse(&format!("http://private{}", request.uri())).unwrap();
        assert_eq!(url.path(), "/hermes/api/ws");
        assert!(
            url.query_pairs()
                .any(|(k, v)| k == "token" && v == "private-session")
        );
        assert_eq!(request.headers()["x-access-fixture"], "private-access");
        Ok(response)
    })
    .unwrap()
}
fn rpc(socket: &mut WebSocket<TcpStream>, method: &str, profile: Option<&str>) -> Value {
    let request: Value =
        serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
    assert_eq!(request["method"], method);
    assert_eq!(
        request["params"].get("profile").and_then(Value::as_str),
        profile
    );
    request
}
fn http(listener: &TcpListener, expected_profile: Option<&str>, body: Value) {
    let mut stream = accept(listener);
    let mut raw = String::new();
    let mut reader = BufReader::new(&mut stream);
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
        raw.push_str(&line);
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).unwrap();
    drop(reader);
    if raw.starts_with("POST ") {
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"changes":{"note":"default"}})
        );
    }
    let lower = raw.to_ascii_lowercase();
    assert!(lower.contains("x-hermes-session-token: private-session\r\n"));
    assert!(lower.contains("x-access-fixture: private-access\r\n"));
    let url = Url::parse(&format!(
        "http://private{}",
        raw.split_whitespace().nth(1).unwrap()
    ))
    .unwrap();
    assert_eq!(
        url.query_pairs()
            .find(|(k, _)| k == "profile")
            .map(|(_, v)| v.into_owned())
            .as_deref(),
        expected_profile
    );
    assert!(!url.query_pairs().any(|(k, _)| k == "token"));
    crate::gateway::tests::http_answer(&mut stream, body);
}
fn root() -> BackendRootProof {
    BackendRootProof {
        profile: None,
        profile_id: "/remote/custom".into(),
        instance_id: "private-runtime".into(),
        authority_epoch: 7,
    }
}

#[test]
fn actual_native_self_and_raw_backend_default_never_share_the_alias_selector() {
    let source = listener();
    let connection = connection(&source, BackendTarget::Named("dixie".into()));
    let server = thread::spawn(move || {
        let mut socket = socket(&source);
        for profile in [
            Some("dixie"),
            Some("default"),
            None,
            Some(""),
            Some("default"),
        ] {
            let request = rpc(&mut socket, "config.get", profile);
            assert_eq!(request["params"]["nested"]["profile"], "default");
            crate::gateway::tests::answer(
                &mut socket,
                &request,
                json!({"profile":profile,"literal":"default"}),
            );
        }
        for profile in ["dixie", "default", "dixie"] {
            http(
                &source,
                Some(profile),
                json!({"profile":profile,"literal":"default"}),
            );
        }
    });
    let mut gateway = Gateway::connect(&connection).unwrap();
    for (provenance, profile) in [
        (ProfileProvenance::NativeSelf, Some("dixie")),
        (
            ProfileProvenance::Backend("default".into()),
            Some("default"),
        ),
        (ProfileProvenance::Ambient(None), None),
        (ProfileProvenance::Ambient(Some("".into())), Some("")),
        (
            ProfileProvenance::Ambient(Some("default".into())),
            Some("default"),
        ),
    ] {
        let mut body = json!({"nested":{"profile":"default"}});
        if let ProfileProvenance::Ambient(Some(value)) = &provenance {
            body["profile"] = value.clone().into();
        }
        let result = gateway
            .page_request(
                "default",
                &PageRequest::plugin_rpc("config.get", body).provenance(provenance),
            )
            .unwrap();
        assert_eq!(result["profile"].as_str(), profile);
        assert_eq!(result["literal"], "default");
    }
    for (provenance, expected) in [
        (ProfileProvenance::NativeSelf, "dixie"),
        (ProfileProvenance::Backend("default".into()), "default"),
    ] {
        let result = gateway
            .page_request(
                "default",
                &PageRequest::get("/api/config").provenance(provenance.clone()),
            )
            .unwrap();
        assert_eq!(result["profile"], expected);
        gateway
            .validate_response_owner(&provenance, &result, "profile", false)
            .unwrap();
    }
    let result = gateway
        .page_request(
            "default",
            &PageRequest::get("/api/config")
                .query("profile", "default")
                .provenance(ProfileProvenance::Ambient(Some("default".into()))),
        )
        .unwrap();
    assert_eq!(result["profile"], "dixie");
    assert!(
        gateway
            .validate_response_owner(
                &ProfileProvenance::NativeSelf,
                &json!({"profile":"default"}),
                "profile",
                false
            )
            .is_err()
    );
    assert!(
        gateway
            .api_url_for(
                &ProfileProvenance::NativeSelf,
                "GET",
                "https://foreign.invalid/api/config?profile=default"
            )
            .is_err()
    );
    server.join().unwrap();
}

#[test]
fn native_session_entry_keeps_backend_receipts_and_raw_default_history_distinct() {
    let source = listener();
    let connection = connection(&source, BackendTarget::Named("dixie".into()));
    let server = thread::spawn(move || {
        for profile in ["dixie", "default"] {
            let mut socket = socket(&source);
            let request = rpc(&mut socket, "session.resume", Some(profile));
            crate::gateway::tests::answer(
                &mut socket,
                &request,
                json!({"session_id":"same","stored_session_id":"same","info":{"profile_name":profile},"messages":[]}),
            );
            http(
                &source,
                Some(profile),
                json!({"profile":profile,"session_id":"same","messages":[{"role":"assistant","text":profile}]}),
            );
        }
    });
    let mut selected =
        conversation::SessionTransport::connect_native_self(&connection, "default").unwrap();
    let snapshot = selected.resume("same").unwrap();
    assert_eq!(snapshot.info["profile_name"], "dixie");
    assert_eq!(
        selected.history("same").unwrap().messages[0].content,
        "dixie"
    );
    let mut raw = conversation::SessionTransport::connect(&connection, "default").unwrap();
    assert_eq!(raw.resume("same").unwrap().info["profile_name"], "default");
    assert_eq!(raw.history("same").unwrap().messages[0].content, "default");
    server.join().unwrap();
}

#[test]
fn fresh_root_scope_verifies_authority_before_any_write_and_does_not_invent_a_name() {
    for changed in [None, Some("home"), Some("instance"), Some("epoch")] {
        let source = listener();
        let proof = root();
        let connection = connection(&source, BackendTarget::Root(proof.clone()));
        let (done, receive) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut socket = socket(&source);
            let request = rpc(&mut socket, "runtime.describe", None);
            assert_eq!(request["params"], json!({}));
            let mut described = json!({"profile_id":proof.profile_id,"instance_id":proof.instance_id,"authority_epoch":proof.authority_epoch});
            match changed {
                Some("home") => described["profile_id"] = json!("/remote/other"),
                Some("instance") => described["instance_id"] = json!("replacement"),
                Some("epoch") => described["authority_epoch"] = json!(8),
                _ => {}
            }
            crate::gateway::tests::answer(&mut socket, &request, described);
            if changed.is_none() {
                for _ in 0..2 {
                    http(
                        &source,
                        None,
                        json!({"ok":true,"profile":"custom","profile_id":"/remote/custom"}),
                    );
                }
            }
            receive.recv_timeout(Duration::from_secs(4)).unwrap();
            assert!(
                matches!(source.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
            );
        });
        let mut gateway = Gateway::connect(&connection).unwrap();
        let result = gateway.page_request(
            "default",
            &PageRequest::post("/api/config", json!({"changes":{"note":"default"}}))
                .provenance(ProfileProvenance::NativeSelf),
        );
        if changed.is_none() {
            let result = result.unwrap();
            assert_eq!(result["profile"], "custom");
            gateway
                .validate_response_owner(&ProfileProvenance::NativeSelf, &result, "profile", false)
                .unwrap();
            assert!(
                gateway
                    .validate_response_owner(
                        &ProfileProvenance::NativeSelf,
                        &json!({"profile_id":"/remote/other"}),
                        "profile",
                        false
                    )
                    .is_err()
            );
            gateway
                .page_request(
                    "default",
                    &PageRequest::get("/api/config").provenance(ProfileProvenance::NativeSelf),
                )
                .unwrap();
            done.send(()).unwrap();
            server.join().unwrap();
        } else {
            done.send(()).unwrap();
            server.join().unwrap();
            assert!(result.err().unwrap().contains("authority changed"));
        }
    }
}

#[test]
fn root_session_epoch_is_distinct_from_runtime_epoch_and_info_stays_backend_owned() {
    let source = listener();
    let connection = connection(&source, BackendTarget::Root(root()));
    let server = thread::spawn(move || {
        let mut socket = socket(&source);
        let request = rpc(&mut socket, "runtime.describe", None);
        assert_eq!(request["params"], json!({}));
        crate::gateway::tests::answer(
            &mut socket,
            &request,
            json!({"profile_id":"/remote/custom","instance_id":"private-runtime","authority_epoch":7}),
        );
        let request = rpc(&mut socket, "session.resume", None);
        crate::gateway::tests::answer(
            &mut socket,
            &request,
            json!({"session_id":"same","stored_session_id":"same","authority_epoch":99,"revision":3,"execution_generation":2,"info":{"profile_name":"custom","profile_id":"/remote/custom"},"messages":[]}),
        );
        socket.send(Message::Text(json!({"method":"event","params":{"session_id":"same","type":"session.info","authority_epoch":99,"payload":{"profile_name":"other","profile_id":"/remote/other"}}}).to_string().into())).unwrap();
    });
    let mut transport =
        conversation::SessionTransport::connect_native_self(&connection, "default").unwrap();
    let snapshot = transport.resume("same").unwrap();
    assert_eq!(snapshot.authority, 99);
    assert_eq!(snapshot.info["profile_name"], "custom");
    assert_eq!(snapshot.info["profile_id"], "/remote/custom");
    assert!(
        transport.read_event().is_err(),
        "A contradictory session.info owner must not enter the event queue"
    );
    assert_eq!(transport.snapshot.unwrap().info["profile_name"], "custom");
    server.join().unwrap();
}

#[test]
fn model_and_voice_requests_keep_explicit_intent_with_identical_window_names() {
    let source = listener();
    let connection = connection(&source, BackendTarget::Named("dixie".into()));
    let server = thread::spawn(move || {
        let mut socket = socket(&source);
        for profile in ["dixie", "default"] {
            let request = rpc(&mut socket, "model.options", Some(profile));
            crate::gateway::tests::answer(
                &mut socket,
                &request,
                json!({"profile":profile,"providers":[{"slug":"fixture","models":[profile]}]}),
            );
        }
        for profile in ["dixie", "default"] {
            http(
                &source,
                Some(profile),
                json!({"profile":profile,"ok":true,"mode":"chained","available":false}),
            );
        }
    });
    let mut gateway = Gateway::connect(&connection).unwrap();
    for (provenance, expected) in [
        (ProfileProvenance::NativeSelf, "dixie"),
        (ProfileProvenance::Backend("default".into()), "default"),
    ] {
        let catalog = gateway.model_catalog_for(&provenance).unwrap();
        assert_eq!(catalog.providers[0].models[0].id, expected);
    }
    for provenance in [
        ProfileProvenance::NativeSelf,
        ProfileProvenance::Backend("default".into()),
    ] {
        assert_eq!(
            gateway
                .voice_status_for(&provenance)
                .unwrap()
                .unwrap()
                .engine,
            VoiceEngine::Chained
        );
    }
    server.join().unwrap();
}

#[test]
fn audio_lease_keeps_the_captured_named_or_raw_owner() {
    use crate::gateway::audio::AudioGateway;
    let source = listener();
    let connection = connection(&source, BackendTarget::Named("dixie".into()));
    let server = thread::spawn(move || {
        for expected in ["dixie", "default"] {
            let _socket = socket(&source);
            let mut stream = accept(&source);
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let url = Url::parse(&format!(
                "http://private{}",
                line.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            assert_eq!(url.path(), "/hermes/api/audio/stt-lease");
            assert_eq!(
                url.query_pairs().find(|(k, _)| k == "profile").unwrap().1,
                expected
            );
            let mut size = 0;
            let mut headers = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    size = value.trim().parse().unwrap();
                }
                headers.push_str(&line.to_ascii_lowercase());
            }
            assert!(headers.contains("x-access-fixture: private-access\r\n"));
            let mut body = vec![0; size];
            reader.read_exact(&mut body).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                json!({"lease":"take", "active":true})
            );
            drop(reader);
            crate::gateway::tests::http_answer(&mut stream, json!({"ok":true,"profile":expected}));
        }
    });
    for intent in [
        crate::source::ProfileIntent::NativeSelf,
        crate::source::ProfileIntent::Backend,
    ] {
        let mut gateway = AudioGateway::connect_scoped(&connection, "default", intent).unwrap();
        gateway.lease(true, "take", true).unwrap();
    }
    server.join().unwrap();
}

#[test]
fn selected_session_list_uses_captured_target_and_rejects_a_conflicting_owner() {
    let source = listener();
    let connection = connection(&source, BackendTarget::Named("dixie".into()));
    let server = thread::spawn(move || {
        let mut socket = socket(&source);
        let selected = rpc(&mut socket, "session.list", Some("dixie"));
        crate::gateway::tests::answer(
            &mut socket,
            &selected,
            json!({"profile":"dixie","sessions":[{"id":"same","profile":"dixie","title":"selected"}]}),
        );
        let raw = rpc(&mut socket, "session.list", Some("default"));
        crate::gateway::tests::answer(
            &mut socket,
            &raw,
            json!({"profile":"default","sessions":[{"id":"same","profile":"default","title":"raw"}]}),
        );
        let bad = rpc(&mut socket, "session.list", Some("dixie"));
        crate::gateway::tests::answer(
            &mut socket,
            &bad,
            json!({"profile":"default","sessions":[]}),
        );
    });
    let mut gateway = Gateway::connect(&connection).unwrap();
    let selected = gateway
        .sessions_for(&ProfileProvenance::NativeSelf, "default")
        .unwrap();
    assert_eq!(selected[0].title, "selected");
    let raw = gateway.sessions("default").unwrap();
    assert_eq!(raw[0].title, "raw");
    assert!(
        gateway
            .sessions_for(&ProfileProvenance::NativeSelf, "default")
            .is_err()
    );
    server.join().unwrap();
}
