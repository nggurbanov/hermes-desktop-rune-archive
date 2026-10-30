//! Real private D-Bus fixture. Never connects to the user's keychain/session bus.
use super::*;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
const SESSION: &str = "/org/freedesktop/secrets/session/fixture";
const ITEM_A: &str = "/org/freedesktop/secrets/item/a";
const ITEM_B: &str = "/org/freedesktop/secrets/item/b";
struct Bus {
    process: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut process = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1", "--nopidfile"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("private dbus-daemon");
        let mut address = String::new();
        BufReader::new(process.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        assert!(address.starts_with("unix:"));
        Self {
            process,
            address: address.trim().into(),
        }
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
#[derive(Default)]
struct State {
    mode: AtomicU8,
    selectors: Mutex<Vec<HashMap<String, String>>>,
    opened: AtomicUsize,
    read: AtomicUsize,
    closed: AtomicUsize,
    unlock: AtomicUsize,
}
struct Fake {
    state: Arc<State>,
}
fn object(path: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(path.to_owned()).unwrap()
}
#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl Fake {
    async fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
        let id = attributes.get("username").cloned().unwrap_or_default();
        self.state.selectors.lock().unwrap().push(attributes);
        match self.state.mode.load(Ordering::SeqCst) {
            1 => return (vec![], vec![object(ITEM_A)]),
            2 => tokio::time::sleep(Duration::from_millis(400)).await,
            4 => return (vec![object(ITEM_A), object(ITEM_B)], vec![]),
            _ => {}
        }
        (
            vec![object(if id == "source-b" { ITEM_B } else { ITEM_A })],
            vec![],
        )
    }
    fn open_session(&self, algorithm: &str, input: OwnedValue) -> (OwnedValue, OwnedObjectPath) {
        assert_eq!(algorithm, "plain");
        self.state.opened.fetch_add(1, Ordering::SeqCst);
        (input, object(SESSION))
    }
    async fn get_secrets(
        &self,
        items: Vec<OwnedObjectPath>,
        session: OwnedObjectPath,
    ) -> HashMap<OwnedObjectPath, Secret> {
        self.state.read.fetch_add(1, Ordering::SeqCst);
        assert_eq!(session.as_str(), SESSION);
        assert_eq!(items.len(), 1);
        let mode = self.state.mode.load(Ordering::SeqCst);
        if mode == 3 {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        let item = items[0].clone();
        let token = if item.as_str() == ITEM_B {
            "token-source-b"
        } else {
            "token-source-a"
        };
        HashMap::from([(
            item,
            Secret {
                session: object(if mode == 5 { "/wrong/session" } else { SESSION }),
                parameters: vec![],
                value: token.as_bytes().to_vec(),
                content_type: "text/plain".into(),
            },
        )])
    }
    fn unlock(&self, _items: Vec<OwnedObjectPath>) -> (Vec<OwnedObjectPath>, OwnedObjectPath) {
        self.state.unlock.fetch_add(1, Ordering::SeqCst);
        (vec![], object("/unexpected/prompt"))
    }
}
struct Session {
    state: Arc<State>,
}
#[zbus::interface(name = "org.freedesktop.Secret.Session")]
impl Session {
    fn close(&self) {
        self.state.closed.fetch_add(1, Ordering::SeqCst);
    }
}
struct Harness {
    bus: Bus,
    state: Arc<State>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Harness {
    fn new(mode: u8) -> Self {
        let bus = Bus::new();
        let address = bus.address.clone();
        let state = Arc::new(State::default());
        state.mode.store(mode, Ordering::SeqCst);
        let observed = state.clone();
        let (ready, started) = mpsc::channel();
        let (stop, done) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let _connection = zbus::connection::Builder::address(address.as_str())
                        .unwrap()
                        .name(SERVICE)
                        .unwrap()
                        .serve_at(
                            ROOT,
                            Fake {
                                state: observed.clone(),
                            },
                        )
                        .unwrap()
                        .serve_at(SESSION, Session { state: observed })
                        .unwrap()
                        .build()
                        .await
                        .unwrap();
                    ready.send(()).unwrap();
                    let _ = done.await;
                });
        });
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        Self {
            bus,
            state,
            stop: Some(stop),
            thread: Some(thread),
        }
    }
    fn read(&self, id: &str, budget: Duration) -> Result<String, String> {
        read_at(Some(&self.bus.address), id, Instant::now() + budget)
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[test]
fn unlocked_credentials_are_read_by_exact_source_selector_without_unlock_or_mutation() {
    let fixture = Harness::new(0);
    assert_eq!(
        fixture.read("source-a", Duration::from_secs(2)).unwrap(),
        "token-source-a"
    );
    assert_eq!(
        fixture.read("source-b", Duration::from_secs(2)).unwrap(),
        "token-source-b"
    );
    let selectors = fixture.state.selectors.lock().unwrap();
    assert_eq!(selectors.len(), 2);
    assert_eq!(
        selectors[0],
        HashMap::from([
            ("service".into(), "Hermes Rune".into()),
            ("username".into(), "source-a".into()),
            ("target".into(), "default".into())
        ])
    );
    assert_eq!(selectors[1]["username"], "source-b");
    assert_eq!(fixture.state.opened.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.state.read.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.state.closed.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.state.unlock.load(Ordering::SeqCst), 0);
}
#[test]
fn locked_or_ambiguous_credential_never_opens_a_session_unlocks_or_prompts() {
    for mode in [1, 4] {
        let fixture = Harness::new(mode);
        let error = fixture
            .read("source-a", Duration::from_secs(2))
            .unwrap_err();
        assert!(error.contains(if mode == 1 { "locked" } else { "ambiguous" }));
        assert_eq!(fixture.state.opened.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.state.read.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.state.unlock.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn private_bus_delay_is_bounded_across_search_and_secret_read_without_prompt_fallback() {
    for mode in [2, 3] {
        let fixture = Harness::new(mode);
        let started = Instant::now();
        let result = fixture.read("source-a", Duration::from_millis(100));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_millis(300));
        assert_eq!(fixture.state.unlock.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn wrong_secret_session_is_rejected_and_explicitly_closed() {
    let fixture = Harness::new(5);
    assert!(
        fixture
            .read("source-a", Duration::from_secs(2))
            .unwrap_err()
            .contains("invalid")
    );
    assert_eq!(fixture.state.closed.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.state.unlock.load(Ordering::SeqCst), 0);
}
#[test]
fn absent_private_keychain_owner_does_not_activate_or_call_a_real_session_bus() {
    let bus = Bus::new();
    let error = read_at(
        Some(&bus.address),
        "source-a",
        Instant::now() + Duration::from_secs(1),
    )
    .unwrap_err();
    assert!(error.contains("not running"));
}
