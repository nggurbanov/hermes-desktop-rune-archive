//! Real private D-Bus, never the user's desktop bus or notification daemon.
use super::*;
use super::{service::Command, types::Reply};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Child, Command as Process, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc as channel;
use zbus::{Connection, zvariant::OwnedValue};
const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
struct Bus {
    process: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut process = Process::new("dbus-daemon")
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
#[derive(Clone, Debug)]
struct Observed {
    title: String,
    body: String,
    silent: bool,
    desktop_entry: Option<String>,
    actions: Vec<String>,
}
#[derive(Default)]
struct FakeState {
    seen: Mutex<Vec<Observed>>,
    closed: Mutex<Vec<u32>>,
    mode: AtomicU8,
    actions: AtomicBool,
    sequence: AtomicU32,
}
#[derive(Clone)]
struct Fake {
    state: Arc<FakeState>,
}
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Fake {
    fn get_capabilities(&self) -> Vec<String> {
        if self.state.actions.load(Ordering::SeqCst) {
            vec!["actions".into(), "body".into(), "sound".into()]
        } else {
            vec!["body".into()]
        }
    }
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        _app: &str,
        _replace: u32,
        _icon: &str,
        title: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        _expire: i32,
        #[zbus(connection)] connection: &Connection,
    ) -> u32 {
        self.state.seen.lock().unwrap().push(Observed {
            title: title.into(),
            body: body.into(),
            silent: hints
                .get("suppress-sound")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false),
            desktop_entry: hints
                .get("desktop-entry")
                .and_then(|value| <&str>::try_from(value).ok())
                .map(str::to_owned),
            actions,
        });
        let id = self.state.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        match self.state.mode.load(Ordering::SeqCst) {
            1 => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            2 => return 0,
            3 => {
                connection
                    .emit_signal(
                        None::<&str>,
                        PATH,
                        SERVICE,
                        "ActionInvoked",
                        &(id, "default"),
                    )
                    .await
                    .unwrap();
            }
            _ => {}
        }
        id
    }
    fn close_notification(&self, id: u32) {
        self.state.closed.lock().unwrap().push(id);
    }
}
async fn server(bus: &Bus, actions: bool, mode: u8) -> (Connection, Arc<FakeState>) {
    let state = Arc::new(FakeState::default());
    state.actions.store(actions, Ordering::SeqCst);
    state.mode.store(mode, Ordering::SeqCst);
    let connection = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(SERVICE)
        .unwrap()
        .serve_at(
            PATH,
            Fake {
                state: state.clone(),
            },
        )
        .unwrap()
        .build()
        .await
        .unwrap();
    (connection, state)
}
struct Harness {
    sender: channel::Sender<Command>,
    task: tokio::task::JoinHandle<()>,
    life: Arc<()>,
    first: mpsc::Receiver<Reply>,
    second: mpsc::Receiver<Reply>,
}
impl Harness {
    async fn new(bus: &Bus) -> Self {
        let (sender, receiver) = channel::channel(32);
        let config = linux::Config {
            address: Some(bus.address.clone()),
            timeout: Duration::from_millis(150),
            cooldown: Duration::from_millis(600),
        };
        let task = tokio::spawn(linux::run_with(receiver, config));
        let life = Arc::new(());
        let (tx1, first) = mpsc::channel();
        let (tx2, second) = mpsc::channel();
        sender
            .send(Command::Register {
                client: 1,
                replies: tx1,
                waker: None,
                primary: true,
                alive: Arc::downgrade(&life),
            })
            .await
            .unwrap_or_else(|_| panic!("private broker request channel closed"));
        sender
            .send(Command::Register {
                client: 2,
                replies: tx2,
                waker: None,
                primary: false,
                alive: Arc::downgrade(&life),
            })
            .await
            .unwrap_or_else(|_| panic!("private broker request channel closed"));
        Self {
            sender,
            task,
            life,
            first,
            second,
        }
    }
    async fn show(&self, id: u64, client: u64, session: &str) {
        let mut request = super::tests::request(client, Kind::Approval, session);
        request.id = id;
        self.sender.send(Command::Show(request)).await.unwrap();
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn receive(rx: &mpsc::Receiver<Reply>, predicate: impl Fn(&Reply) -> bool) -> Vec<Reply> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut replies = vec![];
    loop {
        while let Ok(reply) = rx.try_recv() {
            let done = predicate(&reply);
            replies.push(reply);
            if done {
                return replies;
            }
        }
        assert!(
            Instant::now() < deadline,
            "private notification broker did not report its result"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
fn delivered(reply: &Reply, id: u64, ok: bool) -> bool {
    matches!(reply, Reply::Delivered { id: got, result } if *got == id && result.is_ok() == ok)
}
async fn signal(connection: &Connection, id: u32, key: &str) {
    connection
        .emit_signal(None::<&str>, PATH, SERVICE, "ActionInvoked", &(id, key))
        .await
        .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_daemon_delivery_shared_dedup_silent_and_approval_origin() {
    let bus = Bus::new();
    let (daemon, state) = server(&bus, true, 0).await;
    let h = Harness::new(&bus).await;
    h.show(11, 2, "one").await;
    h.show(12, 1, "one").await;
    receive(&h.second, |reply| delivered(reply, 11, true)).await;
    receive(&h.first, |reply| delivered(reply, 12, true)).await;
    let seen = state.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "one native delivery across two windows");
    assert_eq!(seen[0].title, "Test");
    assert_eq!(seen[0].body, "Private fixture");
    assert!(seen[0].silent);
    assert_eq!(
        seen[0].desktop_entry.as_deref(),
        Some("io.github.nggurbanov.hermes-desktop-rune")
    );
    assert_eq!(seen[0].actions, ["default", "View", "0", "Approve"]);
    let imposter = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    signal(&imposter, 1, "0").await;
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(
        h.second.try_recv().is_err(),
        "another D-Bus peer cannot invoke an action"
    );
    signal(&daemon, 1, "0").await;
    let replies = receive(&h.second, |reply| matches!(reply, Reply::Activated(_))).await;
    let activation = replies
        .into_iter()
        .find_map(|r| {
            if let Reply::Activated(a) = r {
                Some(a)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(activation.scope.session.as_deref(), Some("one"));
    assert_eq!(activation.action.unwrap().answer, "once");
    assert!(
        h.first.try_recv().is_err(),
        "runtime action stays with its originating window"
    );
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(*state.closed.lock().unwrap(), vec![1]);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_daemon_same_batch_click_and_reused_id_after_restart() {
    let bus = Bus::new();
    let (daemon, state) = server(&bus, true, 3).await;
    let h = Harness::new(&bus).await;
    h.show(1, 1, "one").await;
    let replies = receive(&h.first, |reply| delivered(reply, 1, true)).await;
    let mut clicked = replies
        .iter()
        .any(|reply| matches!(reply, Reply::Activated(_)));
    if !clicked {
        clicked = receive(&h.first, |reply| matches!(reply, Reply::Activated(_)))
            .await
            .iter()
            .any(|reply| matches!(reply, Reply::Activated(_)));
    }
    assert!(clicked, "action before Notify reply must not disappear");
    state.mode.store(0, Ordering::SeqCst);
    h.show(2, 1, "old").await;
    receive(&h.first, |reply| delivered(reply, 2, true)).await;
    daemon.release_name(SERVICE).await.unwrap();
    let (replacement, replacement_state) = server(&bus, true, 0).await;
    h.show(3, 1, "new").await;
    receive(&h.first, |reply| delivered(reply, 3, true)).await;
    assert_eq!(replacement_state.sequence.load(Ordering::SeqCst), 1);
    signal(&daemon, 1, "default").await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        h.first.try_recv().is_err(),
        "old unique sender cannot invoke the replacement daemon's id"
    );
    signal(&replacement, 1, "default").await;
    let replies = receive(&h.first, |reply| matches!(reply, Reply::Activated(_))).await;
    let session = replies.into_iter().find_map(|r| {
        if let Reply::Activated(a) = r {
            a.scope.session
        } else {
            None
        }
    });
    assert_eq!(session.as_deref(), Some("new"));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_daemon_timeout_is_truthful_not_replayed_and_new_owner_escapes_cooldown() {
    let bus = Bus::new();
    let (daemon, state) = server(&bus, true, 1).await;
    let h = Harness::new(&bus).await;
    h.show(1, 1, "one").await;
    let replies = receive(&h.first, |reply| delivered(reply, 1, false)).await;
    assert!(replies.iter().any(|reply| matches!(reply, Reply::Delivered { result: Err(message), .. } if message.contains("unconfirmed"))));
    h.show(2, 1, "two").await;
    receive(&h.first, |reply| delivered(reply, 2, false)).await;
    assert_eq!(
        state.seen.lock().unwrap().len(),
        1,
        "timeout/cooldown never resends an uncertain delivery"
    );
    daemon.release_name(SERVICE).await.unwrap();
    let (_new, replacement) = server(&bus, false, 0).await;
    h.show(3, 1, "three").await;
    receive(&h.first, |reply| delivered(reply, 3, true)).await;
    assert!(replacement.seen.lock().unwrap()[0].actions.is_empty());
    assert_eq!(state.seen.lock().unwrap().len(), 1);
    assert_eq!(Arc::strong_count(&h.life), 1);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_daemon_rejects_invalid_ids_and_shares_pending_failure() {
    let bus = Bus::new();
    let (_daemon, state) = server(&bus, true, 2).await;
    let h = Harness::new(&bus).await;
    h.show(1, 1, "one").await;
    h.show(2, 2, "one").await;
    receive(&h.first, |reply| delivered(reply, 1, false)).await;
    receive(&h.second, |reply| delivered(reply, 2, false)).await;
    assert_eq!(
        state.seen.lock().unwrap().len(),
        1,
        "duplicates share failure rather than fabricated delivery success"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_plugin_delivery_waits_for_daemon_and_routes_typed_callback() {
    let bus = Bus::new();
    let (daemon, state) = server(&bus, true, 0).await;
    let h = Harness::new(&bus).await;
    let mut notifications = Notifications::new();
    notifications.client = 37;
    notifications.started = true;
    notifications.test_service = Some(h.sender.clone());
    notifications.bind_connection("private-fixture-principal".into());
    h.sender
        .send(Command::Register {
            client: 37,
            replies: notifications.sender.clone(),
            waker: None,
            primary: false,
            alive: Arc::downgrade(&notifications.alive),
        })
        .await
        .unwrap();
    let mut notice = super::tests::request(37, Kind::Plugin, "ignored").notice;
    notice.kind = Kind::Plugin;
    notice.global = true;
    notice.buttons.clear();
    notice.plugin = Some(PluginTarget {
        lease: None,
        window_token: "window-one".into(),
        lifecycle_key: "private-fixture-principal".into(),
        source_id: "source".into(),
        source_principal: "source-principal".into(),
        profile: "default".into(),
        request_id: "request-uuid".into(),
        notify_id: Some("plugin:callback".into()),
        tag: "plugin".into(),
        activate: Some("/plugin/home".into()),
        actions: vec![PluginButton {
            id: "open".into(),
            label: "Open item".into(),
            activate: Some("/plugin/item/1".into()),
        }],
        icon: None,
        background: true,
    });
    assert!(
        notifications.focused,
        "parent is focused; child background fact must still allow delivery"
    );
    assert!(notifications.dispatch_plugin(notice.clone()).is_some());
    assert!(
        notifications.take_plugin_deliveries().is_empty(),
        "queued is not daemon accepted"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    let delivery = loop {
        notifications.poll();
        let deliveries = notifications.take_plugin_deliveries();
        if let Some(delivery) = deliveries.into_iter().next() {
            break delivery;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert!(delivery.result.unwrap());
    assert_eq!(delivery.request_id, "request-uuid");
    assert_eq!(
        state.seen.lock().unwrap()[0].actions,
        ["default", "View", "0", "Open item"]
    );
    signal(&daemon, 1, "0").await;
    let activation = loop {
        let actions = notifications.poll();
        if let Some(action) = actions.into_iter().next() {
            break action;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert!(activation.action.is_none());
    let plugin = activation.plugin.unwrap();
    assert_eq!(plugin.action_id.as_deref(), Some("open"));
    assert_eq!(plugin.target.window_token, "window-one");
    assert_eq!(plugin.target.notify_id.as_deref(), Some("plugin:callback"));
    // The original SDK must receive false while a notice is suppressed, without retaining callbacks.
    assert!(
        notifications.dispatch_plugin(notice).is_none(),
        "same child/plugin within1s is suppressed"
    );
    h.sender
        .send(Command::Unregister { client: 37 })
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_plugin_timeout_is_reported_as_failed_delivery() {
    let bus = Bus::new();
    let (_daemon, state) = server(&bus, true, 1).await;
    let h = Harness::new(&bus).await;
    let mut request = super::tests::request(2, Kind::Plugin, "plugin");
    request.notice.buttons.clear();
    request.notice.plugin = Some(PluginTarget {
        lease: None,
        window_token: "child".into(),
        lifecycle_key: request.principal.clone(),
        source_id: "source".into(),
        source_principal: "route".into(),
        profile: "default".into(),
        request_id: "request".into(),
        notify_id: Some("callbacks".into()),
        tag: "plugin".into(),
        activate: None,
        actions: vec![],
        icon: None,
        background: true,
    });
    h.sender.send(Command::Show(request)).await.unwrap();
    receive(&h.second, |reply| delivered(reply, 1, false)).await;
    assert_eq!(
        state.seen.lock().unwrap().len(),
        1,
        "no automatic native delivery retry"
    );
}
