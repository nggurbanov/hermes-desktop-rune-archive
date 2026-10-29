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
struct FakeSecrets(Arc<Mutex<HashMap<String, String>>>);
impl Secrets for FakeSecrets {
    fn get(&mut self, id: &str) -> Result<String, String> {
        self.0
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("missing credential".into())
    }
    fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(id.into(), value.into());
        Ok(())
    }
    fn delete(&mut self, id: &str) -> Result<(), String> {
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
}
impl Tunnel for FakeTunnel {
    fn exited(&mut self) -> Option<String> {
        None
    }
    fn close(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}
#[derive(Clone, Default)]
struct FakeTunnels {
    calls: Arc<Mutex<Vec<Ssh>>>,
    closed: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
    fail: Arc<AtomicBool>,
}
impl Tunnels for FakeTunnels {
    fn open(&mut self, spec: &Ssh) -> Result<Box<dyn Tunnel>, String> {
        self.calls.lock().unwrap().push(spec.clone());
        if self.fail.load(Ordering::Acquire) {
            return Err("synthetic fixed port collision".into());
        }
        let closed = Arc::new(AtomicBool::new(false));
        self.closed.lock().unwrap().push(closed.clone());
        Ok(Box::new(FakeTunnel { closed }))
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
        label: label.into(),
        destination: Destination::Remote {
            url: "https://fixture.invalid/prefix".into(),
        },
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
    core.save(remote(
        Some(old.id.clone()),
        "Label only",
        SecretChange::Keep,
    ))
    .unwrap();
    assert_eq!(core.registry.rows[0].revision, old.revision);
    assert_eq!(core.registry.rows[0].credential, old.credential);
    metadata.fail.store(true, Ordering::Release);
    assert!(
        core.save(remote(
            Some(old.id.clone()),
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
    core.save(remote(
        Some(old.id.clone()),
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
    core.save(remote(
        Some(second.id.clone()),
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
