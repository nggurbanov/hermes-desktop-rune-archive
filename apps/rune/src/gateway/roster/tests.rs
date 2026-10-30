use super::*;

#[test]
fn rich_metadata_is_owner_local_and_an_empty_title_is_authoritative() {
    let profiles = parse_profiles(&json!({"profiles":[{"name":"research","display_name":"Research","bot_title":"","ui_meta":{"hermes-bots":{"title":"Old"}},"has_avatar":false}]})).unwrap();
    assert_eq!(profiles[0].name, "default");
    assert_eq!(profiles[1].metadata.title.as_deref(), Some(""));
    assert_eq!(profiles[1].metadata.has_avatar, Some(false));
    assert_eq!(
        profiles[1].metadata.ui_meta.as_ref().unwrap()["hermes-bots"]["title"],
        "Old"
    );
}

#[test]
fn malformed_collection_cannot_become_an_invented_default_agent() {
    assert!(parse_profiles(&json!({"error":"offline"})).is_err());
    assert!(parse_profiles(&json!({"profiles":[{"name":"../other"}]})).is_err());
    assert_eq!(
        parse_profiles(&json!({"profiles":[]})).unwrap()[0].name,
        "default"
    );
}

use std::{
    io::{BufRead, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    thread,
};
fn request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line.is_empty() {
            break;
        }
        request.push_str(&line);
    }
    request
}
fn answer(stream: &mut TcpStream, status: u16, body: Value) {
    let body = body.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}
#[test]
fn actual_http_enumeration_is_read_only_credential_owned_and_preserves_rich_metadata() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/prefix", listener.local_addr().unwrap());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    let fixture = thread::spawn(move || {
        let mut workers = vec![];
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let observed = observed.clone();
            workers.push(thread::spawn(move || {let header=request(&mut stream);assert!(header.starts_with("GET /prefix/api/"));assert!(header.contains("token=source-token"));assert!(!header.contains("/api/ws"));let body=if header.starts_with("GET /prefix/api/profiles?"){json!({"profiles":[{"name":"research","display_name":"Research","bot_title":"","description":"Field notes","ui_meta":{"hermes-bots":{"color":"blue"}},"has_avatar":true,"canonical_session":{"id":"bot","resolved_id":"tip","preview":"Existing relationship"}}]})}else{assert!(header.starts_with("GET /prefix/api/status?"));json!({"install_id":"fixture-install"})};observed.lock().unwrap().push(header);answer(&mut stream,200,body);}));
        }
        for worker in workers {
            worker.join().unwrap();
        }
    });
    let result = read(
        &Connection::Url {
            endpoint,
            token: Some("source-token".into()),
        },
        Instant::now() + Duration::from_secs(2),
        true,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    fixture.join().unwrap();
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert!(matches!(result.identity,Identity::Observed(Some(ref id)) if id=="fixture-install"));
    let profile = result
        .profiles
        .iter()
        .find(|p| p.name == "research")
        .unwrap();
    assert_eq!(profile.metadata.title.as_deref(), Some(""));
    assert_eq!(profile.metadata.has_avatar, Some(true));
    assert_eq!(profile.gateway_profile().description, "Field notes");
    assert_eq!(
        profile
            .gateway_profile()
            .canonical_session
            .unwrap()
            .resolved_id,
        "tip"
    );
}
#[test]
fn slow_identity_does_not_hide_profiles_and_shared_budget_bounds_a_slow_profile() {
    for slow_profiles in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let fixture = thread::spawn(move || {
            let mut workers = vec![];
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                workers.push(thread::spawn(move || {
                    let header = request(&mut stream);
                    let is_profiles = header.starts_with("GET /api/profiles?");
                    if is_profiles == slow_profiles {
                        thread::sleep(Duration::from_millis(400));
                    }
                    answer(
                        &mut stream,
                        200,
                        if is_profiles {
                            json!({"profiles":[{"name":"research"}]})
                        } else {
                            json!({"install_id":"fixture"})
                        },
                    );
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        let started = Instant::now();
        let result = read(
            &Connection::Url {
                endpoint,
                token: None,
            },
            started + Duration::from_millis(250),
            true,
            Arc::new(AtomicBool::new(false)),
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(380),
            "one source exceeded its absolute budget: {elapsed:?}"
        );
        if slow_profiles {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            assert!(result.profiles.iter().any(|p| p.name == "research"));
            assert!(matches!(result.identity, Identity::Unavailable));
        }
        fixture.join().unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn passive_local_roster_uses_only_existing_control_and_native_http_tickets() {
    use std::{
        fs,
        os::unix::{fs::PermissionsExt, net::UnixListener},
    };
    let home = std::env::temp_dir().join(format!("hermes-passive-local-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&home).unwrap();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    let home = fs::canonicalize(home).unwrap();
    let socket = home.join("gateway.sock");
    let control = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let selected = home.clone();
    let private = thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = control.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let result = if index == 0 {
                assert_eq!(request["verb"], "identify");
                json!({"pid":std::process::id(),"runtime_protocol":1,"state":"ready","authority_epoch":1,"instance_id":"fixture-owner","capabilities":["session-authority-v1"],"api_origin":origin,"served_profiles":[{"home":selected,"profile_id":selected}]})
            } else {
                assert_eq!(request["verb"], "session-ticket");
                assert_eq!(request["params"]["purpose"], "native-http");
                assert!(request["params"].get("scope").is_none());
                json!({"profile_id":selected,"instance_id":"fixture-owner","ticket":"fixture-native-http"})
            };
            writeln!(
                stream,
                "{}",
                json!({"protocol":1,"id":1,"ok":true,"result":result})
            )
            .unwrap();
        }
    });
    let public = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let header = request(&mut stream);
        assert!(header.starts_with("GET /api/profiles?profile=default "));
        assert!(!header.contains("token="));
        assert!(
            header
                .to_ascii_lowercase()
                .contains("x-hermes-gateway-ticket: fixture-native-http")
        );
        answer(
            &mut stream,
            200,
            json!({"profiles":[{"name":"default"},{"name":"work"}]}),
        );
    });
    let result = read(
        &Connection::Local { home: home.clone() },
        Instant::now() + Duration::from_secs(2),
        false,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(result.profiles.len(), 2);
    private.join().unwrap();
    public.join().unwrap();
    fs::remove_dir_all(home).unwrap();
}
