use super::*;
use crate::connections::cloud::Tokens;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Default)]
struct FakeSecrets {
    values: Arc<Mutex<BTreeMap<String, String>>>,
    fail: Arc<AtomicBool>,
    fail_delete: Arc<AtomicBool>,
    block_set: Arc<AtomicBool>,
    entered_set: Arc<AtomicBool>,
}
impl Secrets for FakeSecrets {
    fn get(&mut self, id: &str) -> Result<String, String> {
        self.values
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("missing".into())
    }
    fn get_optional(&mut self, id: &str) -> Result<Option<String>, String> {
        Ok(self.values.lock().unwrap().get(id).cloned())
    }
    fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        self.entered_set.store(true, Ordering::Release);
        while self.block_set.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.fail.load(Ordering::Acquire) {
            return Err("fixture failure".into());
        }
        self.values.lock().unwrap().insert(id.into(), value.into());
        Ok(())
    }
    fn delete(&mut self, id: &str) -> Result<(), String> {
        if self.fail_delete.load(Ordering::Acquire) {
            return Err("fixture delete failure".into());
        }
        self.values.lock().unwrap().remove(id);
        Ok(())
    }
}
struct Fixture {
    root: PathBuf,
    path: PathBuf,
    secrets: FakeSecrets,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("rune-login-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("registry.json");
        Self {
            root,
            path,
            secrets: FakeSecrets::default(),
        }
    }
    fn core(&self) -> Core {
        let mut core = Core::system(self.path.clone());
        core.secrets = Box::new(self.secrets.clone());
        core
    }
    fn saved(&self, url: &str) -> domain::ScopeVersion {
        let mut remote = Remote::new(url.into());
        remote.auth_mode = domain::AuthMode::Oauth;
        self.core()
            .save_legacy(Edit {
                scope: Scope::Profile("work".into()),
                expected: domain::ScopeVersion::Absent { revision: None },
                value: ValueEdit::Profile(Profile::Remote(remote)),
                auth: AuthEdit::default(),
            })
            .unwrap()
            .version
    }
    fn request(&self, version: domain::ScopeVersion) -> LoginRequest {
        LoginRequest {
            scope: Scope::Profile("work".into()),
            expected: version,
            provider: Some("fixture".into()),
            generation: LoginGeneration::new(),
            deadline: Instant::now() + Duration::from_secs(3),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn server(count: usize) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        for stream in listener.incoming().take(count) {
            let mut stream = stream.unwrap();
            let mut bytes = [0; 8192];
            let n = stream.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..n]);
            let body = if request.starts_with("GET /api/status ") {
                r#"{"auth_flows":["native_pkce"]}"#
            } else if request.starts_with("GET /api/auth/providers ") {
                r#"{"providers":[{"id":"fixture"}]}"#
            } else {
                r#"{"access_token":"test-access","refresh_token":"test-refresh"}"#
            };
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
    });
    (format!("http://{address}"), thread)
}
fn event(attempt: &LoginAttempt) -> LoginEvent {
    let end = Instant::now() + Duration::from_secs(4);
    loop {
        match attempt.poll() {
            Ok(Some(event)) => return event,
            Err(error) => return LoginEvent::Failed(error),
            Ok(None) => {}
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn callback(url: &str, wrong_state: bool) {
    let url = url::Url::parse(url).unwrap();
    let pairs: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    let redirect = url::Url::parse(&pairs["redirect_uri"]).unwrap();
    let state = if wrong_state {
        "wrong"
    } else {
        &pairs["state"]
    };
    let mut stream = TcpStream::connect(("127.0.0.1", redirect.port().unwrap())).unwrap();
    write!(stream, "GET /callback?state={state}&code=test-code HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", redirect.port().unwrap()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with(if wrong_state {
        "HTTP/1.1 400"
    } else {
        "HTTP/1.1 200"
    }));
}
#[test]
fn private_oauth_success_commits_before_receipt() {
    let fixture = Fixture::new();
    let (url, server) = server(3);
    let version = fixture.saved(&url);
    let attempt = LoginAttempt::with_core(fixture.core(), fixture.request(version));
    let LoginEvent::Authorize(url) = event(&attempt) else {
        panic!("expected authorization")
    };
    callback(&url, true);
    callback(&url, false);
    let LoginEvent::Complete(receipt) = event(&attempt) else {
        panic!("expected receipt")
    };
    assert!(matches!(receipt.version, domain::ScopeVersion::Present(_)));
    assert!(
        fixture
            .secrets
            .values
            .lock()
            .unwrap()
            .contains_key(receipt.reference.id())
    );
    assert!(
        !std::fs::read_to_string(&fixture.path)
            .unwrap()
            .contains("test-access")
    );
    server.join().unwrap();
}
#[test]
fn stale_form_and_timeout_fail_without_network_or_secrets() {
    let fixture = Fixture::new();
    let version = fixture.saved("http://127.0.0.1:9");
    let mut request = fixture.request(version);
    request.generation.invalidate();
    let attempt = LoginAttempt::with_core(fixture.core(), request);
    assert!(matches!(
        event(&attempt),
        LoginEvent::Failed(LoginFailure::Cancelled)
    ));
    let mut request = fixture.request(domain::ScopeVersion::Absent { revision: None });
    request.deadline = Instant::now();
    let attempt = LoginAttempt::with_core(fixture.core(), request);
    assert!(matches!(
        event(&attempt),
        LoginEvent::Failed(LoginFailure::Timeout)
    ));
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
}

#[test]
fn changed_persisted_revision_cannot_install() {
    let fixture = Fixture::new();
    let version = fixture.saved("http://127.0.0.1:9");
    let mut core = fixture.core();
    let mut remote = Remote::new("http://127.0.0.1:10".into());
    remote.auth_mode = domain::AuthMode::Oauth;
    core.save_legacy(Edit {
        scope: Scope::Profile("work".into()),
        expected: version.clone(),
        value: ValueEdit::Profile(Profile::Remote(remote)),
        auth: AuthEdit::default(),
    })
    .unwrap();
    let attempt = LoginAttempt::with_core(fixture.core(), fixture.request(version));
    assert!(matches!(
        event(&attempt),
        LoginEvent::Failed(LoginFailure::Stale)
    ));
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
}

#[test]
fn keyring_failure_never_publishes_receipt() {
    let fixture = Fixture::new();
    let (url, server) = server(3);
    let version = fixture.saved(&url);
    fixture.secrets.fail.store(true, Ordering::Release);
    let attempt = LoginAttempt::with_core(fixture.core(), fixture.request(version));
    let LoginEvent::Authorize(url) = event(&attempt) else {
        panic!("expected authorization")
    };
    callback(&url, false);
    assert!(matches!(
        event(&attempt),
        LoginEvent::Failed(LoginFailure::Store)
    ));
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
    assert!(
        !std::fs::read_to_string(&fixture.path)
            .unwrap()
            .contains("test-access")
    );
    server.join().unwrap();
}

#[test]
fn access_headers_require_supported_embedded_authorization() {
    let fixture = Fixture::new();
    let (url, server) = server(2);
    let mut remote = Remote::new(url);
    remote.auth_mode = domain::AuthMode::Oauth;
    let mut auth = AuthEdit::default();
    auth.headers
        .insert("x-access".into(), SecretEdit::Set("private-header".into()));
    let version = fixture
        .core()
        .save_legacy(Edit {
            scope: Scope::Profile("work".into()),
            expected: domain::ScopeVersion::Absent { revision: None },
            value: ValueEdit::Profile(Profile::Remote(remote)),
            auth,
        })
        .unwrap()
        .version;
    let before = fixture.secrets.values.lock().unwrap().len();
    let attempt = LoginAttempt::with_core(fixture.core(), fixture.request(version));
    assert!(matches!(
        event(&attempt),
        LoginEvent::Failed(LoginFailure::EmbeddedRequired)
    ));
    assert_eq!(fixture.secrets.values.lock().unwrap().len(), before);
    server.join().unwrap();
}

#[test]
fn cancellation_during_keyring_write_is_nonblocking_and_retires_owned_slot() {
    let fixture = Fixture::new();
    let (url, server) = server(3);
    let version = fixture.saved(&url);
    fixture.secrets.block_set.store(true, Ordering::Release);
    let attempt = LoginAttempt::with_core(fixture.core(), fixture.request(version.clone()));
    let LoginEvent::Authorize(url) = event(&attempt) else {
        panic!("expected authorization")
    };
    callback(&url, false);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !fixture.secrets.entered_set.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let start = Instant::now();
    attempt.cancel();
    assert!(start.elapsed() < Duration::from_millis(100));
    fixture.secrets.block_set.store(false, Ordering::Release);
    assert!(matches!(
        event(&attempt),
        LoginEvent::Failed(LoginFailure::Cancelled)
    ));
    assert!(matches!(
        attempt.receiver.recv_timeout(Duration::from_secs(2)),
        Ok(LoginEvent::Failed(_))
    ));
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
    let mut core = fixture.core();
    core.refresh_registry().unwrap();
    assert_eq!(
        core.registry
            .legacy
            .as_ref()
            .unwrap()
            .scope_version(&Scope::Profile("work".into())),
        version
    );
    server.join().unwrap();
}

struct DelayedMetadata {
    inner: Box<dyn Metadata>,
    writes: usize,
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}
impl Metadata for DelayedMetadata {
    fn transaction(&mut self) -> Result<Option<crate::client::FileLock>, String> {
        self.inner.transaction()
    }
    fn load(&mut self) -> Result<Registry, String> {
        self.inner.load()
    }
    fn save(&mut self, registry: &Registry) -> Result<(), String> {
        self.writes += 1;
        if self.writes == 2 {
            self.entered.store(true, Ordering::Release);
            while !self.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.inner.save(registry)
    }
}

#[test]
fn commit_claim_wins_cancellation_during_metadata_publication() {
    let fixture = Fixture::new();
    let (url, server) = server(3);
    let version = fixture.saved(&url);
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let mut core = fixture.core();
    let metadata = std::mem::replace(
        &mut core.metadata,
        Box::new(JsonMetadata(fixture.path.clone())),
    );
    core.metadata = Box::new(DelayedMetadata {
        inner: metadata,
        writes: 0,
        entered: entered.clone(),
        release: release.clone(),
    });
    let attempt = LoginAttempt::with_core(core, fixture.request(version.clone()));
    let LoginEvent::Authorize(url) = event(&attempt) else {
        panic!("expected authorization")
    };
    callback(&url, false);
    let end = Instant::now() + Duration::from_secs(3);
    while !entered.load(Ordering::Acquire) {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    attempt.cancel();
    release.store(true, Ordering::Release);
    let LoginEvent::Complete(receipt) = event(&attempt) else {
        panic!("commit must win")
    };
    let mut core = fixture.core();
    core.refresh_registry().unwrap();
    assert_eq!(
        core.registry
            .legacy
            .as_ref()
            .unwrap()
            .scope_version(&Scope::Profile("work".into())),
        receipt.version
    );
    assert_ne!(receipt.version, version);
    assert!(
        fixture
            .secrets
            .values
            .lock()
            .unwrap()
            .contains_key(receipt.reference.id())
    );
    server.join().unwrap();
}

fn delayed_exchange() -> (
    String,
    Receiver<()>,
    mpsc::Sender<()>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (arrived_send, arrived) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        for (index, stream) in listener.incoming().take(3).enumerate() {
            let mut stream = stream.unwrap();
            let mut bytes = [0; 8192];
            let _ = stream.read(&mut bytes).unwrap();
            let body = match index {
                0 => r#"{"auth_flows":["native_pkce"]}"#,
                1 => r#"{"providers":[{"id":"fixture"}]}"#,
                _ => {
                    arrived_send.send(()).unwrap();
                    let _ = wait.recv_timeout(Duration::from_secs(4));
                    r#"{"access_token":"test-access"}"#
                }
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    (format!("http://{address}"), arrived, release, thread)
}

#[test]
fn cancellation_during_exchange_never_installs_tokens() {
    let fixture = Fixture::new();
    let (url, arrived, release, server) = delayed_exchange();
    let version = fixture.saved(&url);
    let attempt = LoginAttempt::with_core(fixture.core(), fixture.request(version));
    let LoginEvent::Authorize(url) = event(&attempt) else {
        panic!("expected authorization")
    };
    callback(&url, false);
    arrived.recv_timeout(Duration::from_secs(3)).unwrap();
    attempt.cancel();
    release.send(()).unwrap();
    assert!(matches!(
        event(&attempt),
        LoginEvent::Failed(LoginFailure::Cancelled)
    ));
    assert!(matches!(
        attempt.receiver.recv_timeout(Duration::from_secs(2)),
        Ok(LoginEvent::Failed(_))
    ));
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
    server.join().unwrap();
}

#[test]
fn token_exchange_obeys_the_attempt_deadline() {
    let fixture = Fixture::new();
    let (url, arrived, release, server) = delayed_exchange();
    let version = fixture.saved(&url);
    let mut request = fixture.request(version);
    request.deadline = Instant::now() + Duration::from_millis(600);
    let attempt = LoginAttempt::with_core(fixture.core(), request);
    let LoginEvent::Authorize(url) = event(&attempt) else {
        panic!("expected authorization")
    };
    callback(&url, false);
    arrived.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        event(&attempt),
        LoginEvent::Failed(LoginFailure::Timeout)
    ));
    release.send(()).unwrap();
    server.join().unwrap();
    assert!(matches!(
        attempt.receiver.recv_timeout(Duration::from_secs(2)),
        Ok(LoginEvent::Failed(_))
    ));
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
}

struct FailPublication {
    inner: Box<dyn Metadata>,
    writes: usize,
}
impl Metadata for FailPublication {
    fn transaction(&mut self) -> Result<Option<crate::client::FileLock>, String> {
        self.inner.transaction()
    }
    fn load(&mut self) -> Result<Registry, String> {
        self.inner.load()
    }
    fn save(&mut self, value: &Registry) -> Result<(), String> {
        self.writes += 1;
        if self.writes == 2 {
            return Err("fixture publication failure".into());
        }
        self.inner.save(value)
    }
}
#[test]
fn failed_publication_and_failed_secret_delete_leave_unreachable_cleanup() {
    use crate::connections::cloud::TokenStore;
    let fixture = Fixture::new();
    let version = fixture.saved("http://127.0.0.1:9");
    fixture.secrets.fail_delete.store(true, Ordering::Release);
    let mut core = fixture.core();
    let metadata = std::mem::replace(
        &mut core.metadata,
        Box::new(JsonMetadata(fixture.path.clone())),
    );
    core.metadata = Box::new(FailPublication {
        inner: metadata,
        writes: 0,
    });
    let tokens = Tokens {
        access_token: "private".into(),
        refresh_token: String::new(),
        expires_at: 0,
        provider: String::new(),
        user_id: String::new(),
    };
    let result = core.install_cloud_tokens_guarded(
        Scope::Profile("work".into()),
        version.clone(),
        tokens,
        &LoginGeneration::new(),
        Instant::now() + Duration::from_secs(3),
    );
    assert!(matches!(
        result,
        Err(super::super::cloud_install::InstallFailure::Recovery)
    ));
    let mut reader = fixture.core();
    reader.refresh_registry().unwrap();
    let registry = &reader.registry;
    assert_eq!(
        registry
            .legacy
            .as_ref()
            .unwrap()
            .scope_version(&Scope::Profile("work".into())),
        version
    );
    assert_eq!(registry.cleanup.len(), 1);
    let id = &registry.cleanup[0];
    assert!(fixture.secrets.values.lock().unwrap().contains_key(id));
    let store = registry.legacy.as_ref().unwrap();
    let domain::ScopeVersion::Present(stamp) = store.scope_version(&Scope::Profile("work".into()))
    else {
        panic!("saved scope")
    };
    let reference: CredentialRef =
        serde_json::from_value(serde_json::json!({"id": id, "version": 1})).unwrap();
    let mut scoped = super::super::cloud_store::ScopedTokenStore::new(
        fixture.path.clone(),
        Scope::Profile("work".into()),
        &stamp,
        &reference,
        Box::new(fixture.secrets.clone()),
    );
    assert!(scoped.load().is_err());
}

#[test]
fn json_metadata_publish_reports_failure_before_rename_and_success_after() {
    let fixture = Fixture::new();
    let mut metadata = JsonMetadata(fixture.path.clone());
    metadata.save(&Registry::empty()).unwrap();
    assert_eq!(metadata.load().unwrap().version, 1);
    let occupied = fixture.root.join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    let mut blocked = JsonMetadata(occupied.clone());
    assert!(blocked.save(&Registry::empty()).is_err());
    assert!(occupied.is_dir());
    assert_eq!(metadata.load().unwrap().version, 1);
}

#[test]
fn real_worker_authorize_callback_and_receipt_drive_saved_settings_state() {
    let fixture = Fixture::new();
    let (url, server) = server(3);
    let version = fixture.saved(&url);
    let mut core = fixture.core();
    let snapshot = core.legacy_snapshot().unwrap();
    let mut settings = crate::legacy_settings::Settings::new();
    settings.scope = Scope::Profile("work".into());
    settings.test_load_snapshot(snapshot);
    let attempt = LoginAttempt::with_core(fixture.core(), fixture.request(version));
    assert!(settings.test_start_attempt(attempt));
    let rig = crate::connections::legacy::testing::Rig::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while settings.authorization_url().is_none() {
        settings.poll(&rig.connections);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let authorize = settings.authorization_url().unwrap().to_owned();
    callback(&authorize, false);
    while settings.login_pending() {
        settings.poll(&rig.connections);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_ne!(settings.notice.as_deref(), Some("Signed in."));
    let mut core = fixture.core();
    let snapshot = core.legacy_snapshot().unwrap();
    assert_ne!(
        snapshot.scope_version(&Scope::Profile("work".into())),
        domain::ScopeVersion::Absent { revision: None }
    );
    server.join().unwrap();
}

#[test]
fn edit_hold_blocks_publication_until_cancel_release_or_deadline() {
    use super::super::cloud_install::InstallFailure;
    let generation = LoginGeneration::new();
    generation.hold();
    let worker = generation.clone();
    let (send, receive) = mpsc::channel();
    let join = std::thread::spawn(move || {
        send.send(worker.wait_ready(Instant::now() + Duration::from_secs(2)))
            .unwrap();
    });
    assert!(matches!(
        receive.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    generation.invalidate();
    assert!(matches!(
        receive.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(InstallFailure::Cancelled)
    ));
    join.join().unwrap();
    let generation = LoginGeneration::new();
    generation.hold();
    assert!(matches!(
        generation.wait_ready(Instant::now() + Duration::from_millis(20)),
        Err(InstallFailure::Timeout)
    ));
    generation.release();
    assert!(
        generation
            .claim_publication(Instant::now() + Duration::from_secs(1))
            .is_ok()
    );
    generation.hold();
    generation.invalidate();
    assert!(!generation.active());
    assert!(matches!(generation.0.load(Ordering::Acquire), COMMITTED));
}

#[test]
fn held_edit_releases_registry_lock_and_revalidates_before_retry() {
    let fixture = Fixture::new();
    let version = fixture.saved("http://127.0.0.1:9");
    fixture.secrets.block_set.store(true, Ordering::Release);
    let generation = LoginGeneration::new();
    let worker_generation = generation.clone();
    let mut core = fixture.core();
    let expected = version.clone();
    let join = std::thread::spawn(move || {
        core.install_cloud_tokens_guarded(
            Scope::Profile("work".into()),
            expected,
            Tokens {
                access_token: "private".into(),
                refresh_token: String::new(),
                expires_at: 0,
                provider: String::new(),
                user_id: String::new(),
            },
            &worker_generation,
            Instant::now() + Duration::from_secs(3),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fixture.secrets.entered_set.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    generation.hold();
    fixture.secrets.block_set.store(false, Ordering::Release);
    let mut other = fixture.core();
    let mut remote = Remote::new("http://127.0.0.1:10".into());
    remote.auth_mode = domain::AuthMode::Oauth;
    other
        .save_legacy(Edit {
            scope: Scope::Profile("work".into()),
            expected: version,
            value: ValueEdit::Profile(Profile::Remote(remote)),
            auth: AuthEdit::default(),
        })
        .unwrap();
    generation.release();
    assert!(matches!(
        join.join().unwrap(),
        Err(super::super::cloud_install::InstallFailure::Stale)
    ));
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
}

#[test]
fn held_ui_without_poll_times_out_without_owning_registry_lock() {
    let fixture = Fixture::new();
    let version = fixture.saved("http://127.0.0.1:9");
    let generation = LoginGeneration::new();
    generation.hold();
    let worker_generation = generation.clone();
    let mut core = fixture.core();
    let join = std::thread::spawn(move || {
        core.install_cloud_tokens_guarded(
            Scope::Profile("work".into()),
            version,
            Tokens {
                access_token: "private".into(),
                refresh_token: String::new(),
                expires_at: 0,
                provider: String::new(),
                user_id: String::new(),
            },
            &worker_generation,
            Instant::now() + Duration::from_millis(200),
        )
    });
    let _guard = super::super::super::registry::transaction(&fixture.path).unwrap();
    assert!(matches!(
        join.join().unwrap(),
        Err(super::super::cloud_install::InstallFailure::Timeout)
    ));
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
}

#[test]
fn focus_hold_during_callback_waits_without_lock_then_completes() {
    let fixture = Fixture::new();
    let (url, server) = server(3);
    let version = fixture.saved(&url);
    let request = fixture.request(version);
    let generation = request.generation.clone();
    let attempt = LoginAttempt::with_core(fixture.core(), request);
    let LoginEvent::Authorize(url) = event(&attempt) else {
        panic!("authorization")
    };
    generation.hold();
    callback(&url, false);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !server.is_finished() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let guard = super::super::super::registry::transaction(&fixture.path).unwrap();
    assert!(matches!(attempt.poll(), Ok(None)));
    drop(guard);
    generation.release();
    assert!(matches!(event(&attempt), LoginEvent::Complete(_)));
    server.join().unwrap();
}
