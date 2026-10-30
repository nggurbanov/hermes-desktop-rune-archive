use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;
fn materialize_with(plan: Plan, secrets: &mut dyn Secrets) -> Result<PassiveSource, String> {
    super::materialize_with_deadline(
        plan,
        secrets,
        Instant::now() + std::time::Duration::from_secs(2),
    )
}
struct Private {
    root: PathBuf,
}
impl Private {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("hermes-passive-registry-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self { root }
    }
    fn path(&self) -> PathBuf {
        self.root.join("connections.json")
    }
    fn write(&self, rows: Vec<Saved>, primary: Option<String>) {
        let registry = Registry {
            rows,
            primary,
            ..Registry::empty()
        };
        fs::write(self.path(), registry::encode(&registry).unwrap()).unwrap();
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[derive(Clone, Default)]
struct SecretFixture {
    values: Arc<Mutex<std::collections::BTreeMap<String, String>>>,
    reads: Arc<AtomicUsize>,
}
impl Secrets for SecretFixture {
    fn get_passive(&mut self, key: &str, _deadline: Instant) -> Result<String, String> {
        self.get(key)
    }
    fn get(&mut self, key: &str) -> Result<String, String> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.values
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or("fixture missing token".into())
    }
    fn set(&mut self, _: &str, _: &str) -> Result<(), String> {
        panic!("passive read wrote a credential")
    }
    fn delete(&mut self, _: &str) -> Result<(), String> {
        panic!("passive read removed a credential")
    }
}
struct ProbeFixture(Arc<AtomicUsize>);
impl Probe for ProbeFixture {
    fn check(&mut self, _: &Connection) -> Result<Vec<crate::gateway::Profile>, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err("fixture explicit probe refused".into())
    }
}
struct TunnelFixture(Arc<AtomicUsize>);
impl Tunnels for TunnelFixture {
    fn open(&mut self, _: &Ssh) -> Result<Box<dyn Tunnel>, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err("passive inventory attempted a forward".into())
    }
}
fn core(private: &Private, secrets: SecretFixture) -> (Core, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let probes = Arc::new(AtomicUsize::new(0));
    let tunnels = Arc::new(AtomicUsize::new(0));
    let mut core = Core::system(private.path());
    core.secrets = Box::new(secrets);
    core.probe = Box::new(ProbeFixture(probes.clone()));
    core.tunnels = Box::new(TunnelFixture(tunnels.clone()));
    (core, probes, tunnels)
}
fn row(destination: Destination, credential: Option<&str>) -> Saved {
    Saved {
        id: uuid::Uuid::new_v4().to_string(),
        revision: uuid::Uuid::new_v4().to_string(),
        label: "Synthetic".into(),
        destination,
        credential: credential.map(str::to_owned),
        other: Default::default(),
        destination_other: Default::default(),
    }
}
fn version(row: &Saved) -> SourceVersion {
    SourceVersion {
        id: SourceId::Saved(row.id.clone()),
        revision: row.revision.clone(),
    }
}
fn ssh() -> Ssh {
    Ssh {
        host: "fixture".into(),
        user: String::new(),
        port: 22,
        key_file: None,
        remote_port: 9119,
        local_port: 49151,
        path: "/gateway-only".into(),
    }
}

#[test]
fn passive_registry_snapshot_never_probes_and_remote_credentials_stay_source_owned() {
    let private = Private::new();
    let a = row(
        Destination::Remote {
            url: "https://a.invalid".into(),
        },
        Some("a"),
    );
    let b = row(
        Destination::Remote {
            url: "https://b.invalid".into(),
        },
        Some("b"),
    );
    private.write(vec![a.clone(), b.clone()], Some(a.id.clone()));
    let mut secrets = SecretFixture::default();
    secrets.values.lock().unwrap().extend([
        ("a".into(), "token-a".into()),
        ("b".into(), "token-b".into()),
    ]);
    let (mut core, probes, tunnels) = core(&private, secrets.clone());
    let a_plan = core.passive_plan(&version(&a)).unwrap();
    let b_plan = core.passive_plan(&version(&b)).unwrap();
    assert_eq!(
        secrets.reads.load(Ordering::Relaxed),
        0,
        "the actor must not unlock credentials"
    );
    for (plan, expected_url, expected_token) in [
        (a_plan, "https://a.invalid", "token-a"),
        (b_plan, "https://b.invalid", "token-b"),
    ] {
        let PassiveSource::Http { connection } = materialize_with(plan, &mut secrets).unwrap()
        else {
            panic!()
        };
        let Connection::Url { endpoint, token } = connection.transport() else {
            panic!()
        };
        assert_eq!(endpoint, expected_url);
        assert_eq!(token.as_deref(), Some(expected_token));
    }
    assert_eq!(probes.load(Ordering::Relaxed), 0);
    assert_eq!(tunnels.load(Ordering::Relaxed), 0);
    assert!(core.active_owner.is_none());
}
#[test]
fn metadata_only_lease_cannot_bypass_the_first_explicit_gateway_probe() {
    let private = Private::new();
    let row = row(
        Destination::Remote {
            url: "https://fixture.invalid".into(),
        },
        None,
    );
    private.write(vec![row.clone()], Some(row.id.clone()));
    let (mut core, probes, tunnels) = core(&private, SecretFixture::default());
    let plan = core.passive_plan(&version(&row)).unwrap();
    let PassiveSource::Http { connection } =
        materialize_with(plan, &mut SecretFixture::default()).unwrap()
    else {
        panic!()
    };
    let lease = core.retain_passive(&version(&row), connection).unwrap();
    assert!(lease.passive_only());
    assert!(lease.profiles().is_empty());
    assert!(Binding::new(lease.connection().clone(), Some(lease.clone())).is_ok());
    assert!(core.resolve_lease(&row.id, Some(&row.revision)).is_err());
    assert_eq!(probes.load(Ordering::Relaxed), 1);
    assert_eq!(tunnels.load(Ordering::Relaxed), 0);
    assert!(core.active_owner.is_none());
}
#[test]
fn undialed_ssh_and_deferred_local_do_not_open_a_forward_or_read_gateway_credentials() {
    let private = Private::new();
    let remote = row(
        Destination::Remote {
            url: "https://active.invalid".into(),
        },
        None,
    );
    let ssh = row(Destination::Ssh(ssh()), Some("must-not-read"));
    let local = row(
        Destination::Local {
            home: private.root.join("absent-agent"),
        },
        None,
    );
    private.write(
        vec![remote.clone(), ssh.clone(), local.clone()],
        Some(remote.id),
    );
    let secrets = SecretFixture::default();
    let (mut core, probes, tunnels) = core(&private, secrets.clone());
    assert!(matches!(
        materialize_with(
            core.passive_plan(&version(&ssh)).unwrap(),
            &mut secrets.clone()
        )
        .unwrap(),
        PassiveSource::SshInventory(_)
    ));
    assert!(matches!(
        core.passive_plan(&version(&local)).unwrap(),
        Plan::DeferredLocal
    ));
    assert_eq!(secrets.reads.load(Ordering::Relaxed), 0);
    assert_eq!(probes.load(Ordering::Relaxed), 0);
    assert_eq!(tunnels.load(Ordering::Relaxed), 0);
    assert!(!private.root.join("absent-agent").exists());
}
#[test]
fn registry_change_between_snapshot_and_lease_receipt_cannot_bind_the_outgoing_account() {
    let private = Private::new();
    let mut row = row(
        Destination::Remote {
            url: "https://old.invalid".into(),
        },
        None,
    );
    private.write(vec![row.clone()], Some(row.id.clone()));
    let (mut core, _, _) = core(&private, SecretFixture::default());
    let old = version(&row);
    let PassiveSource::Http { connection } = materialize_with(
        core.passive_plan(&old).unwrap(),
        &mut SecretFixture::default(),
    )
    .unwrap() else {
        panic!()
    };
    row.revision = uuid::Uuid::new_v4().to_string();
    row.destination = Destination::Remote {
        url: "https://new.invalid".into(),
    };
    private.write(vec![row], None);
    assert!(core.retain_passive(&old, connection).is_err());
    assert!(core.leases.is_empty());
    assert!(core.active_owner.is_none());
}

#[test]
fn shared_fleet_keeps_the_actual_ssh_actor_after_opener_close_then_releases_it_on_last_ui() {
    struct OwnedTunnel(Arc<std::sync::atomic::AtomicBool>);
    impl Tunnel for OwnedTunnel {
        fn exited(&mut self) -> Option<String> {
            None
        }
        fn close(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    fn actor(core: Core) -> (Resolver, std::thread::JoinHandle<()>) {
        let (send, requests) = mpsc::channel();
        let (replies, receive) = mpsc::channel();
        drop(receive);
        let thread = std::thread::spawn(move || {
            crate::connections::worker::run(Ok(core), requests, replies, Arc::new(|| {}))
        });
        (Resolver::new(send), thread)
    }
    let private = Private::new();
    let row = row(Destination::Ssh(ssh()), None);
    private.write(vec![row.clone()], Some(row.id.clone()));
    let (mut first, _, _) = core(&private, SecretFixture::default());
    let (second, _, _) = core(&private, SecretFixture::default());
    let connection = Connection::Saved {
        id: row.id.clone(),
        revision: row.revision.clone(),
        inner: Box::new(Connection::Url {
            endpoint: "http://127.0.0.1:49151".into(),
            token: Some("fixture".into()),
        }),
    };
    let profile = crate::gateway::Profile {
        name: "research".into(),
        label: "Research".into(),
        description: String::new(),
        canonical_session: None,
    };
    let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (entry, lease) = crate::connections::leases::Entry::new(
        row.id.clone(),
        row.revision.clone(),
        connection.clone(),
        vec![profile.clone()],
        Some(Box::new(OwnedTunnel(closed.clone()))),
    );
    first
        .leases
        .insert((row.id.clone(), row.revision.clone()), entry);
    let binding = Binding::new(connection, Some(lease)).unwrap();
    let (origin, first_thread) = actor(first);
    let (other, second_thread) = actor(second);
    let mut opener = crate::fleet::Fleet::default();
    opener.setup(Arc::new(|| {}), origin.clone());
    opener.update_registry(
        vec![crate::fleet::Descriptor::saved(&row, 0)],
        Some(SourceId::Saved(row.id.clone())),
    );
    opener
        .seed_current(binding.clone(), &[profile.clone()])
        .unwrap();
    let mut peer = opener.clone();
    peer.setup(Arc::new(|| {}), other.clone());
    peer.seed_current(binding.clone(), &[profile]).unwrap();
    drop(origin);
    drop(other);
    drop(opener);
    assert!(!closed.load(Ordering::Acquire));
    assert!(binding.validate().is_ok());
    let captured = peer
        .resolver(&version(&row))
        .expect("the actual retained actor must survive the opener");
    let result = captured
        .passive_source(
            &version(&row),
            Instant::now() + std::time::Duration::from_secs(2),
        )
        .unwrap();
    assert!(
        matches!(result,PassiveSource::Attached(ref current) if current.stamp()==binding.stamp()),
        "a new actor would offer SSH inventory instead of the existing forward"
    );
    drop(result);
    drop(captured);
    peer.detach();
    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    while !closed.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(binding.validate().is_err());
    first_thread.join().unwrap();
    second_thread.join().unwrap();
}

fn pending_request_lifetime(invalidate_registry: bool) {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        time::Duration,
    };
    struct OwnedTunnel(Arc<std::sync::atomic::AtomicBool>);
    impl Tunnel for OwnedTunnel {
        fn exited(&mut self) -> Option<String> {
            None
        }
        fn close(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    struct GatewayFixture {
        release: Option<mpsc::Sender<()>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for GatewayFixture {
        fn drop(&mut self) {
            if let Some(release) = self.release.take() {
                let _ = release.send(());
            }
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (posted, request_seen) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let _ws = tungstenite::accept_hdr(
            stream,
            |request: &tungstenite::handshake::server::Request,
             response: tungstenite::handshake::server::Response| {
                let url = url::Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
                assert_eq!(url.path(), "/api/ws");
                assert_eq!(
                    url.query_pairs().find(|(key, _)| key == "token").unwrap().1,
                    "request-owner-token"
                );
                Ok(response)
            },
        )
        .unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut headers = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(size) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = size.trim().parse().unwrap();
            }
            headers.push_str(&line);
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        assert!(headers.starts_with(
            "POST /api/fixture-owned-write?profile=default&token=request-owner-token "
        ));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["title"],
            "original-owner"
        );
        posted.send(()).unwrap();
        let _ = wait.recv_timeout(Duration::from_secs(4));
        let body = "{\"ok\":true}";
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    let mut gateway = GatewayFixture {
        release: Some(release),
        thread: Some(server),
    };
    let private = Private::new();
    let mut row = row(Destination::Ssh(ssh()), None);
    private.write(vec![row.clone()], Some(row.id.clone()));
    let (mut core, _, _) = core(&private, SecretFixture::default());
    let connection = Connection::Saved {
        id: row.id.clone(),
        revision: row.revision.clone(),
        inner: Box::new(Connection::Url {
            endpoint,
            token: Some("request-owner-token".into()),
        }),
    };
    let profile = crate::gateway::Profile {
        name: "default".into(),
        label: "Default".into(),
        description: String::new(),
        canonical_session: None,
    };
    let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (entry, lease) = crate::connections::leases::Entry::new(
        row.id.clone(),
        row.revision.clone(),
        connection.clone(),
        vec![profile.clone()],
        Some(Box::new(OwnedTunnel(closed.clone()))),
    );
    core.leases
        .insert((row.id.clone(), row.revision.clone()), entry);
    core.refresh_registry().unwrap();
    let (send, requests) = mpsc::channel();
    let (replies, receive) = mpsc::channel();
    drop(receive);
    let (finished, done) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        crate::connections::worker::run(Ok(core), requests, replies, Arc::new(|| {}));
        let _ = finished.send(());
    });
    let resolver = Resolver::new(send);
    let binding = Binding::new(connection.clone(), Some(lease.clone()))
        .unwrap()
        .retaining_resolver(resolver.clone());
    let mut fleet = crate::fleet::Fleet::default();
    fleet.setup(Arc::new(|| {}), resolver.clone());
    fleet.update_registry(
        vec![crate::fleet::Descriptor::saved(&row, 0)],
        Some(SourceId::Saved(row.id.clone())),
    );
    fleet.seed_current(binding.clone(), &[profile]).unwrap();
    let cache = fleet.clone();
    let mut network = crate::pages::PageNetwork::default();
    network.init(Arc::new(|| {}));
    let scope = crate::composer::DraftScope {
        connection: 9,
        profile: "default".into(),
        session: Some("original".into()),
    };
    let request = binding
        .request(
            crate::pages::PageRequest::post(
                "/api/fixture-owned-write",
                serde_json::json!({"title":"original-owner"}),
            )
            .timeout_secs(3),
        )
        .unwrap();
    network
        .request(&scope, connection, "owned-write", request)
        .unwrap();
    request_seen.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(network);
    drop(cache);
    drop(fleet);
    drop(binding);
    drop(resolver);
    assert_eq!(
        done.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout),
        "dropping UI/cache must not retire the origin actor while a sent request holds its receipt"
    );
    if invalidate_registry {
        row.revision = uuid::Uuid::new_v4().to_string();
        private.write(vec![row], None);
        let deadline = Instant::now() + Duration::from_secs(2);
        while lease.is_valid() {
            assert!(
                Instant::now() < deadline,
                "an origin keeper must not bypass metadata retirement"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(closed.load(Ordering::Acquire));
        assert!(
            done.try_recv().is_err(),
            "the request still owns the actor, while its stale lease correctly expires"
        );
    } else {
        assert!(lease.is_valid());
        assert!(!closed.load(Ordering::Acquire));
    }
    gateway.release.take().unwrap().send(()).unwrap();
    done.recv_timeout(Duration::from_secs(3)).unwrap();
    worker.join().unwrap();
    assert!(closed.load(Ordering::Acquire));
    assert!(!lease.is_valid());
}
#[test]
fn pending_request_keeps_origin_until_its_final_receipt_after_ui_and_cache_drop() {
    pending_request_lifetime(false);
}
#[test]
fn request_origin_keeper_does_not_override_explicit_registry_retirement() {
    pending_request_lifetime(true);
}
