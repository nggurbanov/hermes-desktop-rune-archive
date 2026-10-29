use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    },
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hermes-rune-gw-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        mode(&path, 0o700);
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn mode(path: &Path, bits: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(bits)).unwrap();
}
fn identity(home: &Path, origin: &str) -> Value {
    json!({"pid":std::process::id(),"runtime_protocol":1,"state":"ready","authority_epoch":1,
        "instance_id":"fixture-owner","capabilities":["session-authority-v1"],"api_origin":origin,
        "served_profiles":[{"home":home,"profile_id":home}]})
}
fn control_request(stream: &mut std::os::unix::net::UnixStream) -> Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn control_answer(stream: &mut std::os::unix::net::UnixStream, value: Value) {
    writeln!(
        stream,
        "{}",
        json!({"protocol":1,"id":1,"ok":true,"result":value})
    )
    .unwrap();
}

#[test]
fn native_attachment_uses_only_identity_and_purpose_bound_transient_tickets() {
    native_model_catalog(false);
}

#[test]
fn fresh_native_catalog_hydrates_secondary_profile_before_http_fallback() {
    native_model_catalog(true);
}

fn native_model_catalog(fresh: bool) {
    let home = Home::new();
    let work = home.0.join("profiles/work");
    fs::create_dir_all(&work).unwrap();
    mode(&work, 0o700);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let control = UnixListener::bind(home.0.join("gateway.sock")).unwrap();
    mode(&home.0.join("gateway.sock"), 0o600);
    let mut advertised = identity(&home.0, &origin);
    advertised["served_profiles"]
        .as_array_mut()
        .unwrap()
        .push(json!({"home":work,"profile_id":work}));
    let selected_home = home.0.clone();
    let work_home = work.clone();
    let private = thread::spawn(move || {
        for index in 0..if fresh { 3 } else { 4 } {
            let verb = if index == 0 {
                "identify"
            } else {
                "session-ticket"
            };
            let (mut stream, _) = control.accept().unwrap();
            let request = control_request(&mut stream);
            assert_eq!(request["verb"], verb);
            if verb == "identify" {
                control_answer(&mut stream, advertised.clone());
                continue;
            }
            let purpose = request["params"]["purpose"].as_str().unwrap();
            assert!(matches!(purpose, "interactive" | "native-http"));
            assert_eq!(request["params"]["instance_id"], "fixture-owner");
            if purpose == "interactive" {
                assert_eq!(
                    request["params"]["profile_id"],
                    selected_home.to_str().unwrap()
                );
                assert_eq!(request["params"]["scope"], "host");
            } else {
                assert_eq!(request["params"]["profile_id"], work_home.to_str().unwrap());
                assert!(request["params"].get("scope").is_none());
            }
            control_answer(
                &mut stream,
                json!({"profile_id":request["params"]["profile_id"],
                "instance_id":"fixture-owner","ticket":format!("fixture-{purpose}")}),
            );
        }
    });
    let http_work = work.to_string_lossy().into_owned();
    let public = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws =
            tungstenite::accept_hdr(stream, |request: &Request, mut response: Response| {
                assert_eq!(request.uri().path(), "/api/ws");
                assert!(request.uri().query().is_none());
                assert_eq!(
                    request.headers()["Sec-WebSocket-Protocol"],
                    "hermes-gateway-v1, hermes-gateway-ticket.fixture-interactive"
                );
                response.headers_mut().insert(
                    "Sec-WebSocket-Protocol",
                    "hermes-gateway-v1".parse().unwrap(),
                );
                Ok(response)
            })
            .unwrap();
        if fresh {
            let catalog = request(&mut ws, "model.options", "work");
            assert_eq!(catalog["params"]["explicit_only"], true);
            answer(&mut ws, &catalog, json!({"providers":[]}));
        }
        let profiles = request(&mut ws, "profiles.list", "default");
        answer(
            &mut ws,
            &profiles,
            json!({"profiles":[{"name":"work","path":http_work}]}),
        );
        if !fresh {
            let sessions = request(&mut ws, "session.list", "work");
            answer(
                &mut ws,
                &sessions,
                json!({"sessions":[{"session_id":"s","title":"Synthetic"}]}),
            );
            let (mut http, _) = listener.accept().unwrap();
            let header = http_request(&mut http);
            assert!(header.starts_with("GET /api/sessions/s/messages?profile=work&"));
            assert!(!header.split("\r\n").next().unwrap().contains("ticket"));
            assert!(
                header
                    .to_lowercase()
                    .contains("x-hermes-gateway-ticket: fixture-native-http")
            );
            http_answer(
                &mut http,
                json!({"profile":"work","session_id":"s","messages":[{"role":"user","text":"Synthetic"}]}),
            );
            let catalog = request(&mut ws, "model.options", "work");
            assert_eq!(catalog["params"]["explicit_only"], true);
            answer(&mut ws, &catalog, json!({"providers":[]}));
        }
        let (mut http, _) = listener.accept().unwrap();
        let header = http_request(&mut http);
        assert!(header.starts_with("GET /api/model/options?profile=work&explicit_only=1 "));
        assert!(!header.split("\r\n").next().unwrap().contains("ticket"));
        assert!(
            header
                .to_lowercase()
                .contains("x-hermes-gateway-ticket: fixture-native-http")
        );
        http_answer(
            &mut http,
            json!({"providers":[{"slug":"fixture","name":"Fixture","models":["fixture-model"]}]}),
        );
    });
    let mut gateway = Gateway::connect(&Connection::Local {
        home: home.0.clone(),
    })
    .unwrap();
    if !fresh {
        assert_eq!(gateway.profiles().unwrap()[0].name, "work");
        assert_eq!(gateway.sessions("work").unwrap()[0].id, "s");
        assert_eq!(
            gateway.history("work", "s").unwrap().messages[0].content,
            "Synthetic"
        );
    }
    assert_eq!(
        gateway.model_catalog("work").unwrap().providers[0].models[0].id,
        "fixture-model"
    );
    private.join().unwrap();
    public.join().unwrap();
}

#[test]
fn unsafe_homes_and_symlink_sockets_are_refused_without_changing_permissions() {
    let home = Home::new();
    mode(&home.0, 0o770);
    assert!(
        local::identify(&home.0)
            .err()
            .unwrap()
            .contains("permissions")
    );
    use std::os::unix::fs::MetadataExt;
    assert_eq!(fs::metadata(&home.0).unwrap().mode() & 0o777, 0o770);
    mode(&home.0, 0o700);
    let other = Home::new();
    let listener = UnixListener::bind(other.0.join("socket")).unwrap();
    listener.set_nonblocking(true).unwrap();
    symlink(other.0.join("socket"), home.0.join("gateway.sock")).unwrap();
    assert!(
        local::identify(&home.0)
            .err()
            .unwrap()
            .contains("type or owner")
    );
    assert!(listener.accept().is_err());
}

#[test]
fn fallback_socket_pointer_is_bound_to_the_canonical_home_hash() {
    let home = Home::new();
    let scratch = Home::new();
    let digest = format!("{:x}", Sha256::digest(home.0.to_str().unwrap().as_bytes()));
    let target = scratch.0.join(format!("hermes-gw-{}", &digest[..16]));
    fs::create_dir(&target).unwrap();
    mode(&target, 0o700);
    let listener = UnixListener::bind(target.join("control.sock")).unwrap();
    mode(&target.join("control.sock"), 0o600);
    let pointer = home.0.join("gateway.sock.path");
    fs::write(&pointer, target.join("control.sock").to_str().unwrap()).unwrap();
    mode(&pointer, 0o600);
    let advertised = identity(&home.0, "http://127.0.0.1:39343");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert_eq!(control_request(&mut stream)["verb"], "identify");
        control_answer(&mut stream, advertised);
    });
    assert!(local::identify(&home.0).is_ok());
    server.join().unwrap();
    let wrong = scratch.0.join("hermes-gw-0000000000000000");
    fs::create_dir(&wrong).unwrap();
    mode(&wrong, 0o700);
    let _other = UnixListener::bind(wrong.join("control.sock")).unwrap();
    mode(&wrong.join("control.sock"), 0o600);
    fs::write(&pointer, wrong.join("control.sock").to_str().unwrap()).unwrap();
    assert!(
        local::identify(&home.0)
            .err()
            .unwrap()
            .contains("not bound")
    );
}

#[test]
fn native_identity_cannot_name_another_home_or_a_non_loopback_origin() {
    for kind in ["home", "origin", "pid"] {
        let home = Home::new();
        let control = UnixListener::bind(home.0.join("gateway.sock")).unwrap();
        mode(&home.0.join("gateway.sock"), 0o600);
        let mut value = identity(&home.0, "http://127.0.0.1:39343");
        match kind {
            "home" => value["served_profiles"] = json!([{"profile_id":"/other","home":"/other"}]),
            "origin" => value["api_origin"] = json!("http://192.0.2.1:39343"),
            _ => value["pid"] = json!(std::process::id() + 1),
        }
        let server = thread::spawn(move || {
            let (mut stream, _) = control.accept().unwrap();
            let _ = control_request(&mut stream);
            control_answer(&mut stream, value);
        });
        assert!(local::identify(&home.0).is_err());
        server.join().unwrap();
    }
}
