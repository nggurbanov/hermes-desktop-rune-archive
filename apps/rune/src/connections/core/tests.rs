use super::*;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone)]
struct FakeMetadata {
    data: Arc<Mutex<Registry>>,
    fail: Arc<AtomicBool>,
}
impl Metadata for FakeMetadata {
    fn transaction(&mut self) -> Result<Option<crate::client::FileLock>, String> {
        Ok(None)
    }
    fn load(&mut self) -> Result<Registry, String> {
        Ok(self.data.lock().unwrap().clone())
    }
    fn save(&mut self, value: &Registry) -> Result<(), String> {
        if self.fail.load(Ordering::Acquire) {
            return Err("synthetic atomic commit refused".into());
        }
        *self.data.lock().unwrap() = value.clone();
        Ok(())
    }
}
#[derive(Clone, Default)]
struct FakeSecrets(
    Arc<Mutex<HashMap<String, String>>>,
    Arc<Mutex<Vec<&'static str>>>,
);
impl Secrets for FakeSecrets {
    fn get(&mut self, id: &str) -> Result<String, String> {
        self.1.lock().unwrap().push("get");
        self.0
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("missing credential".into())
    }
    fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        self.1.lock().unwrap().push("set");
        self.0.lock().unwrap().insert(id.into(), value.into());
        Ok(())
    }
    fn delete(&mut self, id: &str) -> Result<(), String> {
        self.1.lock().unwrap().push("delete");
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}
#[derive(Clone, Default)]
struct FakeProbe(Arc<Mutex<Vec<Connection>>>);
impl Probe for FakeProbe {
    fn check(&mut self, connection: &Connection) -> Result<Vec<crate::gateway::Profile>, String> {
        self.0.lock().unwrap().push(connection.clone());
        Ok(vec![])
    }
}
struct FakeTunnel {
    closed: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
}
impl Tunnel for FakeTunnel {
    fn exited(&mut self) -> Option<String> {
        self.exited
            .load(Ordering::Acquire)
            .then(|| "synthetic SSH tunnel exited".into())
    }
    fn close(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}
#[derive(Clone, Default)]
struct FakeTunnels {
    calls: Arc<Mutex<Vec<Ssh>>>,
    closed: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
    exits: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
    fail: Arc<AtomicBool>,
}
impl Tunnels for FakeTunnels {
    fn open(&mut self, spec: &Ssh) -> Result<Box<dyn Tunnel>, String> {
        self.calls.lock().unwrap().push(spec.clone());
        if self.fail.load(Ordering::Acquire) {
            return Err("synthetic fixed port collision".into());
        }
        let closed = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        self.closed.lock().unwrap().push(closed.clone());
        self.exits.lock().unwrap().push(exited.clone());
        Ok(Box::new(FakeTunnel { closed, exited }))
    }
}
fn core() -> (Core, FakeMetadata, FakeSecrets, FakeProbe, FakeTunnels) {
    let metadata = FakeMetadata {
        data: Arc::new(Mutex::new(Registry::empty())),
        fail: Arc::new(AtomicBool::new(false)),
    };
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    (
        Core {
            registry: Registry::empty(),
            secrets: Box::new(secrets.clone()),
            metadata: Box::new(metadata.clone()),
            probe: Box::new(probe.clone()),
            tunnels: Box::new(tunnels.clone()),
            active: None,
            active_owner: None,
            outbox: std::collections::VecDeque::new(),
            leases: std::collections::BTreeMap::new(),
            primary_lease: None,
        },
        metadata,
        secrets,
        probe,
        tunnels,
    )
}
fn remote(id: Option<String>, label: &str, token: SecretChange) -> Input {
    Input {
        id,
        expected_revision: None,
        label: label.into(),
        destination: Destination::Remote {
            url: "https://fixture.invalid/prefix".into(),
        },
        secret: token,
    }
}
fn remote_edit(saved: &Saved, label: &str, token: SecretChange) -> Input {
    Input {
        id: Some(saved.id.clone()),
        expected_revision: Some(saved.revision.clone()),
        label: label.into(),
        destination: saved.destination.clone(),
        secret: token,
    }
}
#[test]
fn token_commit_is_transactional_and_metadata_never_contains_credentials() {
    let (mut core, metadata, secrets, _, _) = core();
    core.save(remote(
        None,
        "Original",
        SecretChange::Set("synthetic private token".into()),
    ))
    .unwrap();
    let old = core.registry.rows[0].clone();
    assert!(old.has_credential());
    assert!(
        !serde_json::to_string(&*metadata.data.lock().unwrap())
            .unwrap()
            .contains("synthetic private token")
    );
    core.save(remote_edit(&old, "Label only", SecretChange::Keep))
        .unwrap();
    assert_eq!(core.registry.rows[0].revision, old.revision);
    assert_eq!(core.registry.rows[0].credential, old.credential);
    metadata.fail.store(true, Ordering::Release);
    assert!(
        core.save(remote_edit(
            &old,
            "Must not commit",
            SecretChange::Set("new synthetic token".into())
        ))
        .is_err()
    );
    let map = secrets.0.lock().unwrap();
    assert_eq!(map.len(), 1);
    assert_eq!(
        map.get(old.credential.as_ref().unwrap()).unwrap(),
        "synthetic private token"
    );
    drop(map);
    assert_eq!(core.registry.rows[0].label, "Label only");
    assert_eq!(metadata.data.lock().unwrap().rows[0].label, "Label only");
    assert!(core.forget(&old.id).is_err());
    assert!(
        secrets
            .0
            .lock()
            .unwrap()
            .contains_key(old.credential.as_ref().unwrap())
    );
}
#[test]
fn ssh_reuses_fixed_port_only_for_same_owner_revision_and_never_falls_back_after_collision() {
    let (mut core, metadata, _, probe, tunnels) = core();
    let spec = Ssh {
        host: "fixture-ssh".into(),
        user: "test-user".into(),
        port: 22,
        key_file: None,
        remote_port: 9119,
        local_port: 0,
        path: "/hermes".into(),
    };
    core.save(Input {
        id: None,
        expected_revision: None,
        label: "SSH fixture".into(),
        destination: Destination::Ssh(spec),
        secret: SecretChange::Set("ssh gateway token".into()),
    })
    .unwrap();
    let old = core.registry.rows[0].clone();
    let Destination::Ssh(saved) = &old.destination else {
        panic!()
    };
    assert_ne!(saved.local_port, 0);
    core.select(&old.id).unwrap();
    core.select(&old.id).unwrap();
    assert_eq!(tunnels.calls.lock().unwrap().len(), 1);
    let actual = probe.0.lock().unwrap().last().unwrap().clone();
    let Connection::Saved {
        id,
        revision,
        inner,
    } = actual
    else {
        panic!()
    };
    assert_eq!(id, old.id);
    assert_eq!(revision, old.revision);
    assert!(
        matches!(*inner,Connection::Url{ref endpoint,ref token}if endpoint==&format!("http://127.0.0.1:{}/hermes",saved.local_port)&&token.as_deref()==Some("ssh gateway token"))
    );
    let mut moved = saved.clone();
    moved.host = "other-ssh-host".into();
    core.save(Input {
        id: Some(old.id.clone()),
        expected_revision: Some(old.revision.clone()),
        label: "Moved".into(),
        destination: Destination::Ssh(moved),
        secret: SecretChange::Set("other gateway token".into()),
    })
    .unwrap();
    let changed = core.registry.rows[0].clone();
    assert_ne!(changed.revision, old.revision);
    let Destination::Ssh(changed_spec) = &changed.destination else {
        panic!()
    };
    assert_eq!(changed_spec.local_port, saved.local_port);
    tunnels.fail.store(true, Ordering::Release);
    let before = probe.0.lock().unwrap().len();
    assert!(core.select(&old.id).is_err());
    assert_eq!(
        probe.0.lock().unwrap().len(),
        before,
        "Failed forwarding never probes a different listener or local agent"
    );
    assert!(tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
    assert_eq!(
        metadata.data.lock().unwrap().rows[0].revision,
        changed.revision
    );
    let lost = core.poll_tunnel().unwrap();
    assert_eq!(lost.0, old.id);
    assert_eq!(lost.1, old.revision);
}
#[test]
fn pasted_token_is_extracted_into_keyring_and_auth_rotation_changes_owner_identity() {
    let (mut core, metadata, secrets, probe, _) = core();
    let mut input = remote(None, "Pasted", SecretChange::Keep);
    input.destination = Destination::Remote {
        url: "https://fixture.invalid/prefix/api/ws?token=pasted-secret".into(),
    };
    core.save(input).unwrap();
    let old = core.registry.rows[0].clone();
    let Destination::Remote { url } = old.destination.clone() else {
        panic!()
    };
    assert_eq!(url, "https://fixture.invalid/prefix/");
    assert!(
        !serde_json::to_string(&*metadata.data.lock().unwrap())
            .unwrap()
            .contains("pasted-secret")
    );
    core.select(&old.id).unwrap();
    assert!(
        matches!(probe.0.lock().unwrap()[0].transport(),Connection::Url{token,..}if token.as_deref()==Some("pasted-secret"))
    );
    core.save(remote_edit(
        &old,
        "Rotated",
        SecretChange::Set("rotated-secret".into()),
    ))
    .unwrap();
    assert_ne!(core.registry.rows[0].revision, old.revision);
    assert!(
        !secrets
            .0
            .lock()
            .unwrap()
            .contains_key(old.credential.as_ref().unwrap())
    );
}

#[test]
fn connecting_once_keeps_credentials_in_memory_without_touching_metadata_or_keychain() {
    let (mut core, metadata, secrets, probe, _) = core();
    let result = core
        .execute(Command::ConnectOnce(remote(
            None,
            "One time",
            SecretChange::Set("session-only-synthetic-token".into()),
        )))
        .unwrap();
    assert!(
        matches!(result,Outcome::Ready(_,Connection::Url{token:Some(ref token),..})if token=="session-only-synthetic-token")
    );
    assert!(metadata.data.lock().unwrap().rows.is_empty());
    assert!(secrets.0.lock().unwrap().is_empty());
    assert_eq!(probe.0.lock().unwrap().len(), 1);
}

fn lease_ssh(label: &str, host: &str) -> Input {
    Input {
        id: None,
        expected_revision: None,
        label: label.into(),
        destination: Destination::Ssh(Ssh {
            host: host.into(),
            user: "fixture".into(),
            port: 22,
            key_file: None,
            remote_port: 9119,
            local_port: 0,
            path: "/hermes".into(),
        }),
        secret: SecretChange::Set("synthetic SSH token".into()),
    }
}
#[test]
fn resolving_a_saved_source_preserves_the_foreground_and_fences_credential_revision() {
    let (mut core, metadata, _, probe, _) = core();
    core.save(remote(
        None,
        "Foreground",
        SecretChange::Set("first synthetic token".into()),
    ))
    .unwrap();
    let first = core.registry.rows[0].clone();
    core.select(&first.id).unwrap();
    core.save(remote(
        None,
        "Background",
        SecretChange::Set("second synthetic token".into()),
    ))
    .unwrap();
    let second = core.registry.rows[1].clone();
    let lease = core
        .resolve_lease(&second.id, Some(&second.revision))
        .unwrap();
    assert_eq!(
        metadata.data.lock().unwrap().selected.as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(core.active_owner.as_ref().unwrap().0, first.id);
    assert!(lease.is_valid());
    core.save(remote_edit(
        &second,
        "Background",
        SecretChange::Set("rotated synthetic token".into()),
    ))
    .unwrap();
    core.poll_leases();
    assert!(!lease.is_valid());
    assert!(lease.error().is_some());
    let calls = probe.0.lock().unwrap().len();
    assert!(
        core.resolve_lease(&second.id, Some(&second.revision))
            .is_err()
    );
    assert_eq!(probe.0.lock().unwrap().len(), calls);
}
#[test]
fn ssh_leases_reuse_their_own_forward_and_close_only_after_the_last_reference() {
    let (mut core, metadata, _, _, tunnels) = core();
    core.save(lease_ssh("Foreground", "first.fixture.invalid"))
        .unwrap();
    let first = core.registry.rows[0].clone();
    core.select(&first.id).unwrap();
    core.save(lease_ssh("Background", "second.fixture.invalid"))
        .unwrap();
    let second = core.registry.rows[1].clone();
    let lease = core
        .resolve_lease(&second.id, Some(&second.revision))
        .unwrap();
    let retained = core
        .resolve_lease(&second.id, Some(&second.revision))
        .unwrap();
    assert_eq!(tunnels.calls.lock().unwrap().len(), 2);
    assert_eq!(core.active.as_ref().unwrap().id, first.id);
    assert_eq!(
        metadata.data.lock().unwrap().selected.as_deref(),
        Some(first.id.as_str())
    );
    drop(lease);
    core.poll_leases();
    assert!(!tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire));
    drop(retained);
    core.poll_leases();
    assert!(tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire));
    assert!(!tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
}
#[test]
fn verified_lease_activation_commits_before_releasing_the_previous_foreground() {
    let (mut core, metadata, _, _, tunnels) = core();
    core.save(lease_ssh("Foreground", "first.fixture.invalid"))
        .unwrap();
    let first = core.registry.rows[0].clone();
    core.select(&first.id).unwrap();
    core.save(lease_ssh("Candidate", "second.fixture.invalid"))
        .unwrap();
    let second = core.registry.rows[1].clone();
    let lease = core
        .resolve_lease(&second.id, Some(&second.revision))
        .unwrap();
    metadata.fail.store(true, Ordering::Release);
    assert!(core.commit_lease(lease.clone()).is_err());
    assert_eq!(core.active.as_ref().unwrap().id, first.id);
    assert_eq!(
        metadata.data.lock().unwrap().selected.as_deref(),
        Some(first.id.as_str())
    );
    assert!(!tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
    assert!(lease.is_valid());
    metadata.fail.store(false, Ordering::Release);
    core.commit_lease(lease.clone()).unwrap();
    drop(lease);
    core.poll_leases();
    assert_eq!(
        metadata.data.lock().unwrap().selected.as_deref(),
        Some(second.id.as_str())
    );
    assert!(tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
    assert!(!tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire));
    core.forget(&second.id).unwrap();
    core.poll_leases();
    assert!(tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire));
    assert!(core.primary_lease.is_none());
}

#[test]
fn retaining_the_foreground_ssh_source_preserves_its_existing_socket_endpoint_across_switches() {
    let (mut core, _, _, _, tunnels) = core();
    core.save(lease_ssh("First", "first.fixture.invalid"))
        .unwrap();
    let first = core.registry.rows[0].clone();
    let Outcome::Ready(_, old_connection) = core.select(&first.id).unwrap() else {
        panic!()
    };
    let lease = core
        .resolve_lease(&first.id, Some(&first.revision))
        .unwrap();
    assert_eq!(
        crate::client::connection_key(&old_connection).unwrap(),
        crate::client::connection_key(lease.connection()).unwrap()
    );
    assert_eq!(tunnels.calls.lock().unwrap().len(), 1);
    assert!(core.active.is_none());
    core.save(lease_ssh("Second", "second.fixture.invalid"))
        .unwrap();
    let second = core.registry.rows[1].clone();
    let candidate = core
        .resolve_lease(&second.id, Some(&second.revision))
        .unwrap();
    core.commit_lease(candidate.clone()).unwrap();
    core.poll_leases();
    assert!(!tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
    core.select(&first.id).unwrap();
    assert_eq!(tunnels.calls.lock().unwrap().len(), 2);
    core.select(&second.id).unwrap();
    drop(lease);
    core.poll_leases();
    assert!(tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
    assert!(!tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire));
}

fn retained_worker(
    core: Core,
) -> (
    Sender<Work>,
    Resolver,
    Receiver<Reply>,
    Receiver<()>,
    std::thread::JoinHandle<()>,
) {
    let (send, requests) = mpsc::channel();
    let (replies, receive) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let resolver = Resolver::new(send.clone());
    let worker = std::thread::spawn(move || {
        super::super::worker::run(Ok(core), requests, replies, Arc::new(|| {}));
        let _ = finished.send(());
    });
    (send, resolver, receive, done, worker)
}
fn resolved(resolver: &Resolver, row: &Saved) -> Lease {
    resolver
        .resolve(row.id.clone(), Some(row.revision.clone()))
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap()
}

#[test]
fn retained_resolver_outlives_the_ui_reply_sink_and_last_sender_retires_its_ssh_lease() {
    let (mut core, _, _, _, tunnels) = core();
    core.save(lease_ssh("Retained peer", "retained.fixture.invalid"))
        .unwrap();
    let row = core.registry.rows[0].clone();
    let (send, resolver, ui, done, worker) = retained_worker(core);
    drop(ui);
    // This queued UI request has no reply receiver. It must not own the resolver's life.
    send.send(Work::Ui(Request {
        id: 10,
        command: Command::Load,
    }))
    .unwrap();
    let lease = resolved(&resolver, &row);
    assert!(lease.is_valid());
    assert!(!tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
    let other = resolver.clone();
    drop(send);
    drop(resolver);
    assert!(done.try_recv().is_err());
    let same = resolved(&other, &row);
    assert_eq!(
        crate::client::connection_key(lease.connection()).unwrap(),
        crate::client::connection_key(same.connection()).unwrap()
    );
    assert_eq!(tunnels.calls.lock().unwrap().len(), 1);
    drop(other);
    done.recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    worker.join().unwrap();
    assert!(!lease.is_valid());
    assert!(!same.is_valid());
    assert!(lease.error().is_some());
    assert!(tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
}

#[test]
fn retained_worker_keeps_its_watchdog_and_resolves_other_sources_after_lost_ui_delivery() {
    let (mut core, _, _, _, tunnels) = core();
    core.save(lease_ssh("Foreground", "lost.fixture.invalid"))
        .unwrap();
    let first = core.registry.rows[0].clone();
    core.select(&first.id).unwrap();
    core.save(lease_ssh("Other peer", "other.fixture.invalid"))
        .unwrap();
    let second = core.registry.rows[1].clone();
    let (send, resolver, ui, done, worker) = retained_worker(core);
    drop(ui);
    let failed = resolved(&resolver, &first);
    tunnels.exits.lock().unwrap()[0].store(true, Ordering::Release);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while failed.is_valid() {
        assert!(
            std::time::Instant::now() < deadline,
            "retained tunnel watchdog did not run"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    wait_until(
        || tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire),
        "failed retained tunnel did not close",
    );
    assert!(failed.error().is_some());
    let working = resolved(&resolver, &second);
    assert!(working.is_valid());
    assert_eq!(tunnels.calls.lock().unwrap().len(), 2);
    assert!(!tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire));
    drop(send);
    drop(resolver);
    done.recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    worker.join().unwrap();
    assert!(!working.is_valid());
    assert!(tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire));
}

struct RegistryFile(PathBuf);
impl RegistryFile {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-shared-registry-{}", uuid::Uuid::new_v4())))
    }
    fn path(&self) -> PathBuf {
        self.0.join("connections.json")
    }
}
impl Drop for RegistryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn json_core(
    path: PathBuf,
    secrets: &FakeSecrets,
    probe: &FakeProbe,
    tunnels: &FakeTunnels,
) -> Core {
    Core {
        registry: Registry::empty(),
        secrets: Box::new(secrets.clone()),
        metadata: Box::new(JsonMetadata(path)),
        probe: Box::new(probe.clone()),
        tunnels: Box::new(tunnels.clone()),
        active: None,
        active_owner: None,
        outbox: std::collections::VecDeque::new(),
        leases: std::collections::BTreeMap::new(),
        primary_lease: None,
    }
}
fn wait_until(mut predicate: impl FnMut() -> bool, message: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    while !predicate() {
        assert!(std::time::Instant::now() < deadline, "{message}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn a_second_registry_writer_retires_old_peer_leases_without_another_ui_or_resolver_request() {
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut writer = json_core(file.path(), &secrets, &probe, &tunnels);
    writer
        .execute(Command::Save(lease_ssh("Shared", "shared.fixture.invalid")))
        .unwrap();
    let row = writer.registry.rows[0].clone();
    let reader = json_core(file.path(), &secrets, &probe, &tunnels);
    let (send, resolver, ui, done, worker) = retained_worker(reader);
    drop(ui);
    let old = resolved(&resolver, &row);
    let old_guard = old.guard();
    let count = probe.0.lock().unwrap().len();
    writer
        .execute(Command::Save(Input {
            id: Some(row.id.clone()),
            expected_revision: Some(row.revision.clone()),
            label: row.label.clone(),
            destination: row.destination.clone(),
            secret: SecretChange::Set("rotated-registry-fixture-token".into()),
        }))
        .unwrap();
    let changed = writer.registry.rows[0].clone();
    assert_ne!(changed.revision, row.revision);
    wait_until(
        || !old_guard.is_valid() && tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire),
        "metadata refresh did not retire the old peer lease/tunnel",
    );
    assert!(tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
    assert_eq!(
        probe.0.lock().unwrap().len(),
        count,
        "registry observation cannot probe an agent"
    );
    assert!(
        resolver
            .resolve(row.id.clone(), Some(row.revision.clone()))
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .is_err()
    );
    assert_eq!(probe.0.lock().unwrap().len(), count);
    let current = resolved(&resolver, &changed);
    assert!(current.is_valid());
    assert!(!old_guard.is_valid());
    assert_eq!(tunnels.calls.lock().unwrap().len(), 2);
    // A later independent writer removes the same identity, again without a request.
    writer.execute(Command::Forget(changed.id)).unwrap();
    wait_until(
        || !current.is_valid() && tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire),
        "metadata refresh did not retire the removed peer source/tunnel",
    );
    assert!(tunnels.closed.lock().unwrap()[1].load(Ordering::Acquire));
    drop(send);
    drop(resolver);
    done.recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    worker.join().unwrap();
}
#[test]
fn malformed_or_future_registry_retires_sources_preserves_bytes_and_repair_never_revives_old_guards()
 {
    for invalid in [
        b"invalid registry".as_slice(),
        br#"{"version":9,"rows":[],"selected":null,"cleanup":[]}"#.as_slice(),
    ] {
        let file = RegistryFile::new();
        let secrets = FakeSecrets::default();
        let probe = FakeProbe::default();
        let tunnels = FakeTunnels::default();
        let mut writer = json_core(file.path(), &secrets, &probe, &tunnels);
        writer
            .execute(Command::Save(lease_ssh(
                "Original",
                "original.fixture.invalid",
            )))
            .unwrap();
        let row = writer.registry.rows[0].clone();
        let valid = std::fs::read(file.path()).unwrap();
        let reader = json_core(file.path(), &secrets, &probe, &tunnels);
        let (send, resolver, ui, done, worker) = retained_worker(reader);
        drop(ui);
        let old = resolved(&resolver, &row);
        let guard = old.guard();
        let count = probe.0.lock().unwrap().len();
        crate::client::private_write(&file.path(), invalid).unwrap();
        wait_until(
            || !guard.is_valid() && tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire),
            "invalid registry did not retire the retained source/tunnel",
        );
        assert_eq!(std::fs::read(file.path()).unwrap(), invalid);
        assert!(old.error().is_some());
        assert!(tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
        assert_eq!(probe.0.lock().unwrap().len(), count);
        // An explicit write cannot silently replace this unsupported registry either.
        assert!(
            writer
                .execute(Command::Save(lease_ssh(
                    "Must not replace",
                    "blocked.fixture.invalid"
                )))
                .is_err()
        );
        assert_eq!(std::fs::read(file.path()).unwrap(), invalid);
        assert!(
            resolver
                .resolve(row.id.clone(), Some(row.revision.clone()))
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap()
                .is_err()
        );
        crate::client::private_write(&file.path(), &valid).unwrap();
        let repaired = resolved(&resolver, &row);
        assert!(repaired.is_valid());
        assert!(
            !guard.is_valid(),
            "reusing id/revision must not revive an expired lease incarnation"
        );
        drop(send);
        drop(resolver);
        done.recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        worker.join().unwrap();
        assert!(!repaired.is_valid());
    }
}

#[test]
fn concurrent_json_registry_add_edit_and_forget_preserve_other_rows_and_credential_references() {
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut seed = json_core(file.path(), &secrets, &probe, &tunnels);
    for name in ["Edit", "Forget", "Untouched"] {
        seed.execute(Command::Save(remote(
            None,
            name,
            SecretChange::Set(format!("fixture-token-{name}")),
        )))
        .unwrap();
    }
    let initial = seed.registry.clone();
    let edit = initial.rows[0].clone();
    let forget = initial.rows[1].clone();
    let untouched = initial.rows[2].clone();
    let commands = [
        Command::Save(remote(
            None,
            "Added",
            SecretChange::Set("added-fixture-token".into()),
        )),
        Command::Save(remote_edit(
            &edit,
            "Edited",
            SecretChange::Set("edited-fixture-token".into()),
        )),
        Command::Forget(forget.id.clone()),
    ];
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let (completed, done) = mpsc::channel();
    let before = std::fs::read(file.path()).unwrap();
    let secret_before = secrets.0.lock().unwrap().clone();
    // An actual independently opened OS guard protects the entire mutation, including secrets.
    let held = super::registry::transaction(&file.path()).unwrap();
    let mut workers = vec![];
    for command in commands {
        let mut writer = json_core(file.path(), &secrets, &probe, &tunnels);
        writer.registry = initial.clone();
        let barrier = barrier.clone();
        let completed = completed.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let result = writer.execute(command).map(|_| ());
            completed.send(result.clone()).unwrap();
            result
        }));
    }
    barrier.wait();
    std::thread::sleep(std::time::Duration::from_millis(40));
    assert!(done.try_recv().is_err());
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
    assert_eq!(*secrets.0.lock().unwrap(), secret_before);
    drop(held);
    for _ in 0..3 {
        done.recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap();
    }
    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    let final_registry = JsonMetadata(file.path()).load().unwrap();
    assert_eq!(final_registry.rows.len(), 3);
    assert!(!final_registry.rows.iter().any(|r| r.id == forget.id));
    let updated = final_registry
        .rows
        .iter()
        .find(|r| r.id == edit.id)
        .unwrap();
    assert_eq!(updated.label, "Edited");
    assert_ne!(updated.revision, edit.revision);
    let retained = final_registry
        .rows
        .iter()
        .find(|r| r.id == untouched.id)
        .unwrap();
    assert_eq!(retained.credential, untouched.credential);
    assert!(final_registry.rows.iter().any(|r| r.label == "Added"));
    let keys: std::collections::HashSet<_> = secrets.0.lock().unwrap().keys().cloned().collect();
    let references: std::collections::HashSet<_> = final_registry
        .rows
        .iter()
        .filter_map(|r| r.credential.clone())
        .collect();
    assert_eq!(keys, references);
    assert!(!keys.contains(edit.credential.as_ref().unwrap()));
    assert!(!keys.contains(forget.credential.as_ref().unwrap()));
    assert!(probe.0.lock().unwrap().is_empty());
    assert!(tunnels.calls.lock().unwrap().is_empty());
}

struct PausedProbe {
    base: FakeProbe,
    start: Option<Sender<()>>,
    resume: Option<Receiver<()>>,
}
impl Probe for PausedProbe {
    fn check(&mut self, connection: &Connection) -> Result<Vec<crate::gateway::Profile>, String> {
        if let Some(start) = self.start.take() {
            start.send(()).map_err(|_| "fixture probe owner closed")?;
            self.resume
                .take()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(3))
                .map_err(|_| "fixture probe was not released")?;
        }
        self.base.check(connection)
    }
}
#[test]
fn resolver_holds_the_registry_transaction_through_probe_before_a_second_writer_rotates_credentials()
 {
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut writer = json_core(file.path(), &secrets, &probe, &tunnels);
    writer
        .execute(Command::Save(lease_ssh(
            "Probe source",
            "probe.fixture.invalid",
        )))
        .unwrap();
    let row = writer.registry.rows[0].clone();
    let before = std::fs::read(file.path()).unwrap();
    let (started, entering) = mpsc::channel();
    let (resume, release) = mpsc::channel();
    let mut reader = json_core(file.path(), &secrets, &probe, &tunnels);
    reader.probe = Box::new(PausedProbe {
        base: probe.clone(),
        start: Some(started),
        resume: Some(release),
    });
    let (send, resolver, ui, done, worker) = retained_worker(reader);
    drop(ui);
    let result = resolver
        .resolve(row.id.clone(), Some(row.revision.clone()))
        .unwrap();
    entering
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    let (finish, finished) = mpsc::channel();
    let previous = row.clone();
    let writer = std::thread::spawn(move || {
        let result = writer
            .execute(Command::Save(Input {
                id: Some(previous.id),
                expected_revision: Some(previous.revision),
                label: previous.label,
                destination: previous.destination,
                secret: SecretChange::Set("new-probed-source-fixture-token".into()),
            }))
            .map(|_| ());
        finish.send(result.clone()).unwrap();
        result
    });
    std::thread::sleep(std::time::Duration::from_millis(40));
    assert!(finished.try_recv().is_err());
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
    assert!(
        secrets
            .0
            .lock()
            .unwrap()
            .contains_key(row.credential.as_ref().unwrap())
    );
    resume.send(()).unwrap();
    let lease = result
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap();
    finished
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap();
    writer.join().unwrap().unwrap();
    let current = JsonMetadata(file.path()).load().unwrap();
    assert_ne!(current.rows[0].revision, row.revision);
    assert!(
        !secrets
            .0
            .lock()
            .unwrap()
            .contains_key(row.credential.as_ref().unwrap())
    );
    wait_until(
        || !lease.is_valid() && tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire),
        "rotated probed lease did not retire",
    );
    drop(send);
    drop(resolver);
    done.recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    worker.join().unwrap();
}

#[test]
fn stale_anonymous_editor_keeps_its_form_and_cannot_replace_another_windows_destination() {
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut writer = json_core(file.path(), &secrets, &probe, &tunnels);
    writer
        .execute(Command::Save(remote(None, "Anonymous", SecretChange::Keep)))
        .unwrap();
    let original = writer.registry.rows[0].clone();
    let (send, resolver, receive, done, worker) =
        retained_worker(json_core(file.path(), &secrets, &probe, &tunnels));
    let mut ui = Connections::new();
    ui.network = Some(Network { send, receive });
    ui.load();
    wait_until(
        || {
            ui.poll();
            !ui.busy
        },
        "initial registry did not load",
    );
    ui.action(Action::Edit(original.id.clone()));
    ui.editor
        .as_mut()
        .unwrap()
        .label
        .replace_text("Unsaved label");

    let mut other = Editor::existing(&original);
    other
        .url
        .replace_text("https://other.fixture.invalid/hermes/");
    writer
        .execute(Command::Save(other.input().unwrap()))
        .unwrap();
    let current = writer.registry.rows[0].clone();
    assert_ne!(current.revision, original.revision);
    assert!(!current.has_credential());
    let committed = std::fs::read(file.path()).unwrap();

    for supplied_token in ["", "must-not-be-stored"] {
        ui.editor
            .as_mut()
            .unwrap()
            .token
            .replace_text(supplied_token);
        ui.action(Action::Save);
        wait_until(
            || {
                ui.poll();
                !ui.busy
            },
            "stale save did not complete",
        );
        assert!(
            ui.error
                .as_deref()
                .is_some_and(|error| error.contains("changed in another window"))
        );
        let retained = ui
            .editor
            .as_ref()
            .expect("rejected save lost the entered form");
        assert_eq!(retained.label.text, "Unsaved label");
        assert_eq!(
            retained.url.text.as_str(),
            match &original.destination {
                Destination::Remote { url } => url.as_str(),
                _ => unreachable!(),
            }
        );
        assert_eq!(retained.token.text, supplied_token);
        assert_eq!(std::fs::read(file.path()).unwrap(), committed);
        assert!(secrets.0.lock().unwrap().is_empty());
        assert!(
            secrets.1.lock().unwrap().is_empty(),
            "stale save accessed the keychain"
        );
        assert!(probe.0.lock().unwrap().is_empty());
        assert!(tunnels.calls.lock().unwrap().is_empty());
    }

    // Explicitly reload/reopen the latest row, then reapply the retained label.
    ui.action(Action::Reload);
    wait_until(
        || {
            ui.poll();
            !ui.busy
        },
        "updated registry did not load",
    );
    ui.action(Action::Edit(current.id.clone()));
    ui.editor
        .as_mut()
        .unwrap()
        .label
        .replace_text("Unsaved label");
    ui.action(Action::Save);
    wait_until(
        || {
            ui.poll();
            !ui.busy
        },
        "current revision did not save",
    );
    assert!(ui.error.is_none());
    assert!(ui.editor.is_none());
    let stored = JsonMetadata(file.path()).load().unwrap();
    assert_eq!(stored.rows[0].label, "Unsaved label");
    assert_eq!(stored.rows[0].destination, current.destination);
    assert_eq!(stored.rows[0].revision, current.revision);
    assert!(secrets.1.lock().unwrap().is_empty());
    drop(ui);
    drop(resolver);
    done.recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    worker.join().unwrap();
}

#[test]
fn saved_edits_require_the_captured_revision_and_a_current_capture_can_rotate_credentials() {
    let (mut core, metadata, secrets, probe, tunnels) = core();
    core.execute(Command::Save(remote(None, "Current", SecretChange::Keep)))
        .unwrap();
    let row = core.registry.rows[0].clone();
    let before = serde_json::to_vec(&*metadata.data.lock().unwrap()).unwrap();
    let mut missing = remote(
        Some(row.id.clone()),
        "Uncaptured",
        SecretChange::Set("must-not-be-stored".into()),
    );
    missing.destination = Destination::Remote {
        url: "invalid URL".into(),
    };
    let error = core
        .execute(Command::Save(missing))
        .err()
        .expect("an uncaptured edit was accepted");
    assert!(error.contains("Reopen"), "unexpected error: {error}");
    assert_eq!(
        serde_json::to_vec(&*metadata.data.lock().unwrap()).unwrap(),
        before
    );
    assert!(secrets.1.lock().unwrap().is_empty());
    assert!(probe.0.lock().unwrap().is_empty());
    assert!(tunnels.calls.lock().unwrap().is_empty());

    let mut current = Editor::existing(&row);
    current.label.replace_text("Authenticated current");
    current.token.replace_text("current-revision-token");
    core.execute(Command::Save(current.input().unwrap()))
        .unwrap();
    let saved = &core.registry.rows[0];
    assert_eq!(saved.id, row.id);
    assert_ne!(saved.revision, row.revision);
    assert_eq!(saved.label, "Authenticated current");
    assert_eq!(saved.destination, row.destination);
    assert_eq!(
        secrets
            .0
            .lock()
            .unwrap()
            .get(saved.credential.as_ref().unwrap())
            .map(String::as_str),
        Some("current-revision-token")
    );
    assert_eq!(*secrets.1.lock().unwrap(), vec!["set"]);
}
