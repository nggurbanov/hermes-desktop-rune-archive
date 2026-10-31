use super::*;
use crate::{
    connections::legacy::{Environment, ResolveRequest, SelectionContext},
    gateway::prepared::BackendTarget,
    source::{Binding, RouteDoor, SourceId},
};
use serde_json::{Value, json};
use std::{
    net::TcpListener,
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tungstenite::{
    Message,
    handshake::server::{Request, Response},
};
struct Server {
    endpoint: String,
    stop: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
    calls: Arc<Mutex<Vec<Value>>>,
    epoch: Arc<std::sync::atomic::AtomicU64>,
}
impl Server {
    fn new() -> Self {
        Self::with_access(true)
    }
    fn with_access(access: bool) -> Self {
        let epoch = Arc::new(std::sync::atomic::AtomicU64::new(7));
        let runtime_epoch = epoch.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/hermes/", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let finished = stop.clone();
        let calls = Arc::new(Mutex::new(vec![]));
        let observed = calls.clone();
        let task = thread::spawn(move || {
            while !finished.load(Ordering::Acquire) {
                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("private accept: {e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut ws =
                    tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                        let url =
                            url::Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
                        assert_eq!(url.path(), "/hermes/api/ws");
                        assert_eq!(
                            url.query_pairs().find(|(k, _)| k == "token").unwrap().1,
                            "fixture-token"
                        );
                        if access {
                            assert_eq!(request.headers()["x-private-access"], "fixture-access");
                        } else {
                            assert!(!request.headers().contains_key("x-private-access"));
                        }
                        Ok(response)
                    })
                    .unwrap();
                loop {
                    if finished.load(Ordering::Acquire) {
                        break;
                    }
                    let message = match ws.read() {
                        Ok(Message::Text(text)) => text,
                        Ok(Message::Close(_))
                        | Err(tungstenite::Error::ConnectionClosed)
                        | Err(tungstenite::Error::AlreadyClosed) => break,
                        Err(tungstenite::Error::Io(e))
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            continue;
                        }
                        Err(tungstenite::Error::Protocol(_)) => break,
                        Ok(_) => continue,
                        Err(e) => panic!("private WS: {e}"),
                    };
                    let request: Value = serde_json::from_str(&message).unwrap();
                    observed.lock().unwrap().push(request.clone());
                    let value = match request["method"].as_str().unwrap() {
                        "profiles.list" => {
                            json!({"profiles":[{"name":"backend-home","path":"/private/backend-home"},{"name":"declared-alias","path":"/private/declared-alias"}],"install_id":"private-install"})
                        }
                        "runtime.describe" => {
                            assert_eq!(request["params"], json!({}));
                            json!({"profile_id":"/private/backend-home","instance_id":"fixture-instance","authority_epoch":runtime_epoch.load(Ordering::Acquire)})
                        }
                        other => panic!("unexpected private method {other}"),
                    };
                    if ws
                        .send(Message::Text(
                            json!({"jsonrpc":"2.0","id":request["id"],"result":value})
                                .to_string()
                                .into(),
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
        Self {
            endpoint,
            stop,
            task: Some(task),
            calls,
            epoch,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            task.join().unwrap();
        }
    }
}
fn context() -> SelectionContext {
    SelectionContext {
        environment: Environment::captured(None, None),
        primary: None,
        primary_profile: Some("default".into()),
        primary_remote_active: false,
        isolated_backend: false,
        local_home: None,
    }
}
fn request(profile: &str) -> ResolveRequest {
    ResolveRequest {
        door: RouteDoor::Legacy,
        desktop_profile: profile.into(),
        context: context(),
        primary: None,
        expected: None,
        expected_namespace: None,
        intent: 17,
    }
}
fn configure(core: &mut Core, server: &Server, scope: Scope, alias: Option<&str>) {
    let mut value = Remote::new(server.endpoint.clone());
    value.remote_profile = alias.map(str::to_owned);
    let snapshot = core.legacy_snapshot().unwrap();
    let expected = snapshot.scope_version(&scope);
    let value = match scope {
        Scope::Global => ValueEdit::Global(Global {
            mode: domain::Mode::Remote,
            remote: Some(value),
            ..Default::default()
        }),
        Scope::Profile(_) => ValueEdit::Profile(Profile::Remote(value)),
    };
    core.save_legacy(Edit {
        scope,
        expected,
        value,
        auth: AuthEdit {
            token: SecretEdit::Set("fixture-token".into()),
            headers: BTreeMap::from([(
                "x-private-access".into(),
                SecretEdit::Set("fixture-access".into()),
            )]),
        },
    })
    .unwrap();
}
#[test]
fn url_resolution_preserves_alias_door_and_exact_raw_proof_without_invented_profile() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let server = Server::new();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        None,
    );
    let resolved = core
        .resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert_eq!(
        resolved.lease.connection().route_owner().unwrap(),
        resolved.connection.route_owner().unwrap()
    );
    assert!(
        matches!(resolved.connection.wire_policy().unwrap().selected().unwrap().target(),BackendTarget::Root(root) if root.profile_id=="/private/backend-home")
    );
    assert_eq!(
        resolved
            .backend_proof
            .profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["backend-home", "declared-alias"]
    );
    assert!(
        !resolved
            .backend_proof
            .profiles
            .iter()
            .any(|p| p.name == "desktop-alias")
    );
    assert_eq!(resolved.lease.registry_identity(), None);
    assert_eq!(
        resolved
            .connection
            .route_owner()
            .unwrap()
            .source_version()
            .id,
        SourceId::Legacy
    );
    assert!(resolved.matched_saved.is_none());
    core.commit_legacy(resolved, None, &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert!(fixture.load().selected.is_none());
    assert_eq!(
        server
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == "runtime.describe")
            .count(),
        1
    );
}
#[test]
fn explicit_backend_alias_is_validated_and_stale_namespace_never_publishes_a_receipt() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let server = Server::new();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        Some("declared-alias"),
    );
    let first = core
        .resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert!(
        matches!(first.connection.wire_policy().unwrap().selected().unwrap().target(),BackendTarget::Named(name) if name=="declared-alias")
    );
    let namespace = first
        .connection
        .route_owner()
        .unwrap()
        .namespace()
        .to_owned();
    let mut reopen = request("desktop-alias");
    reopen.expected_namespace = Some(namespace.clone());
    let same = core
        .resolve_legacy(reopen, &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert_eq!(
        same.connection.route_owner().unwrap().namespace(),
        namespace
    );
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        Some("backend-home"),
    );
    let mut stale = request("desktop-alias");
    stale.expected_namespace = Some(namespace);
    assert!(
        core.resolve_legacy(stale, &Arc::new(AtomicBool::new(true)))
            .is_err()
    );
    assert!(!first.lease.is_valid());
}
#[test]
fn selective_route_watchdog_and_acceptance_fence_reject_only_changed_effective_dependencies() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let server = Server::new();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        None,
    );
    let resolved = core
        .resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    configure(&mut core, &server, Scope::Profile("unrelated".into()), None);
    core.poll_leases();
    assert!(resolved.lease.is_valid());
    let before = fs::read(&fixture.path).unwrap();
    assert!(
        core.commit_legacy(
            resolved,
            Some("other-namespace"),
            &Arc::new(AtomicBool::new(true))
        )
        .is_err()
    );
    assert_eq!(before, fs::read(&fixture.path).unwrap());
    let resolved = core
        .resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        Some("declared-alias"),
    );
    core.poll_leases();
    assert!(!resolved.lease.is_valid());
    assert!(
        core.commit_legacy(resolved, None, &Arc::new(AtomicBool::new(true)))
            .is_err()
    );
}
#[test]
fn cancelled_and_unsupported_selected_routes_do_not_try_other_sources_or_create_leases() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let server = Server::new();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        None,
    );
    assert!(
        core.resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(false)))
            .is_err()
    );
    assert!(server.calls.lock().unwrap().is_empty());
    core.ssh_binary = fixture.root.join("no-ssh-executable");
    let scope = Scope::Profile("desktop-alias".into());
    let expected = core.legacy_snapshot().unwrap().scope_version(&scope);
    core.save_legacy(Edit {
        scope,
        expected,
        value: ValueEdit::Profile(Profile::Ssh(domain::InstalledSsh::new(
            "unused.invalid".into(),
        ))),
        auth: AuthEdit::default(),
    })
    .unwrap();
    assert!(
        core.resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
            .unwrap_err()
            .contains("SSH")
    );
    assert!(server.calls.lock().unwrap().is_empty());
    assert!(core.route_leases.is_empty());
}
#[test]
fn request_retains_two_original_actors_without_cycle_until_final_request_and_still_obeys_retirement()
 {
    let fixture = Private::new();
    let mut core = fixture.core();
    let server = Server::new();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        None,
    );
    let resolved = core
        .resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    let one = crate::connections::legacy::testing::Rig::new();
    let two = crate::connections::legacy::testing::Rig::new();
    let one_state = one.state.clone();
    let two_state = two.state.clone();
    let a = one.resolver();
    let b = two.resolver();
    let binding = Binding::new(resolved.connection, resolved.lease.clone().into())
        .unwrap()
        .retaining_origins(vec![a.clone(), a.clone(), b.clone()]);
    assert_eq!(binding.origins().len(), 2);
    let request = binding
        .request(crate::pages::PageRequest::get("/api/config").profile("desktop-alias"))
        .unwrap();
    drop(binding);
    drop(one);
    drop(two);
    drop(a);
    drop(b);
    thread::sleep(Duration::from_millis(250));
    assert!(!one_state.worker_exited());
    assert!(!two_state.worker_exited());
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        Some("declared-alias"),
    );
    core.poll_leases();
    assert!(!resolved.lease.is_valid());
    drop(request);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !(one_state.worker_exited() && two_state.worker_exited()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(one_state.worker_exited());
    assert!(two_state.worker_exited());
}

fn saved(fixture: &Private, server: &Server, local: bool) -> Saved {
    let row = Saved {
        id: uuid::Uuid::new_v4().to_string(),
        revision: uuid::Uuid::new_v4().to_string(),
        label: "Private Saved Door".into(),
        destination: if local {
            Destination::Local {
                home: fixture.root.join("existing-agent"),
            }
        } else {
            Destination::Remote {
                url: server.endpoint.clone(),
            }
        },
        credential: (!local).then(|| "private-saved-credential".into()),
        other: Default::default(),
        destination_other: Default::default(),
    };
    let mut registry = fixture.load();
    registry.rows.push(row.clone());
    fs::write(&fixture.path, registry::encode(&registry).unwrap()).unwrap();
    fixture
        .secrets
        .values
        .lock()
        .unwrap()
        .insert("private-saved-credential".into(), "fixture-token".into());
    row
}
#[test]
fn same_home_fresh_authority_gets_a_new_full_lease_while_durable_namespace_stays_stable() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let server = Server::new();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        None,
    );
    let first = core
        .resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    let original_namespace = first
        .connection
        .route_owner()
        .unwrap()
        .namespace()
        .to_owned();
    server.epoch.store(8, Ordering::Release);
    let second = core
        .resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert_eq!(
        second.connection.route_owner().unwrap().namespace(),
        original_namespace
    );
    assert_ne!(
        crate::client::connection_key(first.lease.connection()).unwrap(),
        crate::client::connection_key(second.lease.connection()).unwrap()
    );
    assert_eq!(first.root_proof.authority_epoch, 7);
    assert_eq!(second.root_proof.authority_epoch, 8);
    assert!(first.lease.is_valid());
    assert!(second.lease.is_valid());
    let mut stale = crate::gateway::Gateway::connect(first.lease.connection()).unwrap();
    assert!(
        stale
            .ensure_profile_scope(&crate::gateway::prepared::ProfileProvenance::NativeSelf)
            .is_err()
    );
    drop(stale);
    let mut fresh = crate::gateway::Gateway::connect(second.lease.connection()).unwrap();
    fresh
        .ensure_profile_scope(&crate::gateway::prepared::ProfileProvenance::NativeSelf)
        .unwrap();
    drop(fresh);
    assert!(server.calls.lock().unwrap().iter().all(|r| matches!(
        r["method"].as_str(),
        Some("profiles.list" | "runtime.describe")
    )));
}
#[test]
fn saved_local_override_keeps_requested_raw_door_and_never_selects_actual_remote_provenance() {
    let fixture = Private::new();
    let server = Server::new();
    let row = saved(&fixture, &server, true);
    let mut core = fixture.core();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        None,
    );
    let mut open = request("desktop-alias");
    open.door = RouteDoor::Saved {
        id: row.id.clone(),
        revision: row.revision.clone(),
    };
    let resolved = core
        .resolve_legacy(open, &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert_eq!(
        resolved
            .connection
            .route_owner()
            .unwrap()
            .registry_identity(),
        Some((row.id.as_str(), row.revision.as_str()))
    );
    assert_eq!(
        resolved.lease.registry_identity(),
        Some((row.id.as_str(), row.revision.as_str()))
    );
    assert!(resolved.lease.actual_saved_identity().is_none());
    assert!(resolved.matched_saved.is_none());
    let previous = fixture.load();
    core.commit_legacy(resolved, None, &Arc::new(AtomicBool::new(true)))
        .unwrap();
    let persisted = fixture.load();
    assert_eq!(persisted.last_used.as_deref(), Some(row.id.as_str()));
    assert_eq!(persisted.last_profiles[&row.id], "desktop-alias");
    assert_eq!(persisted.selected, previous.selected);
    assert_eq!(persisted.primary, previous.primary);
    // A global URL cannot redirect the registered Local door after its own override is removed.
    configure(&mut core, &server, Scope::Global, None);
    let scope = Scope::Profile("desktop-alias".into());
    let expected = core.legacy_snapshot().unwrap().scope_version(&scope);
    core.save_legacy(Edit {
        scope,
        expected,
        value: ValueEdit::Remove,
        auth: AuthEdit::default(),
    })
    .unwrap();
    let calls = server.calls.lock().unwrap().len();
    let mut open = request("desktop-alias");
    open.door = RouteDoor::Saved {
        id: row.id,
        revision: row.revision,
    };
    assert!(
        core.resolve_legacy(open, &Arc::new(AtomicBool::new(true)))
            .is_err()
    );
    assert_eq!(server.calls.lock().unwrap().len(), calls);
}
#[test]
fn native_saved_remote_uses_exact_old_connection_lease_and_raw_backend_selector() {
    let fixture = Private::new();
    let server = Server::with_access(false);
    let row = saved(&fixture, &server, false);
    let mut core = fixture.core();
    // Neither global nor profile Legacy URL may replace an explicit non-Local Saved door.
    configure(&mut core, &server, Scope::Global, None);
    configure(
        &mut core,
        &server,
        Scope::Profile("backend-home".into()),
        None,
    );
    let mut open = request("backend-home");
    open.door = RouteDoor::Saved {
        id: row.id.clone(),
        revision: row.revision.clone(),
    };
    let resolved = core
        .resolve_legacy(open, &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert!(
        matches!(&resolved.connection,Connection::Saved{id,revision,..} if id==&row.id&&revision==&row.revision)
    );
    assert!(resolved.connection.wire_policy().is_none());
    assert!(resolved.connection.route_owner().is_none());
    let old = core.resolve_lease(&row.id, Some(&row.revision)).unwrap();
    assert!(std::ptr::eq(old.connection(), resolved.lease.connection()));
    core.commit_legacy(resolved, None, &Arc::new(AtomicBool::new(true)))
        .unwrap();
    let mut invalid = request("desktop-only-alias");
    invalid.door = RouteDoor::Saved {
        id: row.id,
        revision: row.revision,
    };
    assert!(
        core.resolve_legacy(invalid, &Arc::new(AtomicBool::new(true)))
            .unwrap_err()
            .contains("selected backend profile")
    );
}
#[test]
fn registry_primary_accepts_only_actual_saved_ancestry_not_same_home_or_logical_door() {
    let fixture = Private::new();
    let server = Server::new();
    let row = saved(&fixture, &server, false);
    let mut core = fixture.core();
    configure(
        &mut core,
        &server,
        Scope::Profile("desktop-alias".into()),
        None,
    );
    let known = core
        .resolve_legacy(request("desktop-alias"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    let mut registry = fixture.load();
    registry.primary = Some(row.id.clone());
    fs::write(&fixture.path, registry::encode(&registry).unwrap()).unwrap();
    let raw = Connection::Saved {
        id: row.id.clone(),
        revision: row.revision.clone(),
        inner: Box::new(Connection::Url {
            endpoint: server.endpoint.clone(),
            token: Some("fixture-token".into()),
        }),
    };
    let (_raw_entry, raw_lease) = crate::connections::leases::Entry::new(
        row.id.clone(),
        row.revision.clone(),
        raw.clone(),
        known.backend_proof.profiles.clone(),
        None,
    );
    let captured = Connection::Routed {
        actual: Box::new(raw.transport().clone()),
        policy: known.connection.wire_policy().unwrap().clone(),
        owner: Arc::new(crate::source::RouteOwner::legacy("prior-logical-legacy").unwrap()),
    };
    let (_captured_entry, captured_lease) = crate::connections::leases::Entry::routed(
        captured.clone(),
        known.backend_proof.profiles.clone(),
        Some(raw_lease.clone()),
        None,
    )
    .unwrap();
    let capture = crate::connections::legacy::CapturedPrimary {
        connection: captured.clone(),
        lease: Some(captured_lease),
        backend_proof: known.backend_proof.clone(),
        root_proof: known.root_proof.clone(),
    };
    let mut open = request("backend-home");
    open.context.primary_remote_active = true;
    open.primary = Some(capture.clone());
    let resolved = core
        .resolve_legacy(open, &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert_eq!(
        resolved.lease.actual_saved_identity(),
        Some((row.id.as_str(), row.revision.as_str()))
    );
    assert_eq!(
        resolved
            .connection
            .route_owner()
            .unwrap()
            .registry_identity(),
        None
    );
    assert_eq!(
        resolved.matched_saved.unwrap().id,
        SourceId::Saved(row.id.clone())
    );
    server.epoch.store(8, Ordering::Release);
    let mut stale = request("backend-home");
    stale.context.primary_remote_active = true;
    stale.primary = Some(capture);
    assert!(
        core.resolve_legacy(stale, &Arc::new(AtomicBool::new(true)))
            .unwrap_err()
            .contains("runtime authority changed")
    );
    server.epoch.store(7, Ordering::Release);
    let other_id = uuid::Uuid::new_v4().to_string();
    let other_revision = uuid::Uuid::new_v4().to_string();
    let other_raw = Connection::Saved {
        id: other_id.clone(),
        revision: other_revision.clone(),
        inner: Box::new(raw.transport().clone()),
    };
    let (_other_entry, other) =
        crate::connections::leases::Entry::new(other_id, other_revision, other_raw, vec![], None);
    let (_wrong_entry, wrong) =
        crate::connections::leases::Entry::routed(captured.clone(), vec![], Some(other), None)
            .unwrap();
    let calls = server.calls.lock().unwrap().len();
    let mut wrong_open = request("backend-home");
    wrong_open.context.primary_remote_active = true;
    wrong_open.primary = Some(crate::connections::legacy::CapturedPrimary {
        connection: captured,
        lease: Some(wrong),
        backend_proof: known.backend_proof,
        root_proof: known.root_proof,
    });
    assert!(
        core.resolve_legacy(wrong_open, &Arc::new(AtomicBool::new(true)))
            .unwrap_err()
            .contains("selected registry source")
    );
    assert_eq!(server.calls.lock().unwrap().len(), calls);
}

#[test]
fn cancellation_at_the_final_native_saved_storage_guard_cannot_publish_a_successful_receipt() {
    struct CancelAfterProof {
        actual: JsonMetadata,
        calls: Arc<Mutex<Vec<Value>>>,
        alive: Arc<AtomicBool>,
    }
    impl Metadata for CancelAfterProof {
        fn transaction(&mut self) -> Result<Option<crate::client::FileLock>, String> {
            let guard = self.actual.transaction()?;
            if self
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|call| call["method"] == "runtime.describe")
            {
                self.alive.store(false, Ordering::Release);
            }
            Ok(guard)
        }
        fn load(&mut self) -> Result<Registry, String> {
            self.actual.load()
        }
        fn save(&mut self, value: &Registry) -> Result<(), String> {
            self.actual.save(value)
        }
    }
    let fixture = Private::new();
    let server = Server::with_access(false);
    let row = saved(&fixture, &server, false);
    let alive = Arc::new(AtomicBool::new(true));
    let mut core = fixture.core();
    core.metadata = Box::new(CancelAfterProof {
        actual: JsonMetadata(fixture.path.clone()),
        calls: server.calls.clone(),
        alive: alive.clone(),
    });
    let mut target = request("backend-home");
    target.door = RouteDoor::Saved {
        id: row.id,
        revision: row.revision,
    };
    let before = fs::read(&fixture.path).unwrap();
    let result = core.resolve_legacy(target, &alive);
    assert!(
        server
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call["method"] == "runtime.describe")
    );
    assert!(!alive.load(Ordering::Acquire));
    assert!(
        result.unwrap_err().contains("superseded"),
        "Cancellation after evidence but before publication must invalidate the receipt"
    );
    assert_eq!(fs::read(&fixture.path).unwrap(), before);
}

#[test]
fn cold_registry_primary_is_resolved_and_proved_without_a_live_viewer() {
    let fixture = Private::new();
    let server = Server::with_access(false);
    let row = saved(&fixture, &server, false);
    let mut registry = fixture.load();
    registry.primary = Some(row.id.clone());
    fs::write(&fixture.path, registry::encode(&registry).unwrap()).unwrap();
    let mut core = fixture.core();
    let resolved = core
        .resolve_legacy(request("backend-home"), &Arc::new(AtomicBool::new(true)))
        .unwrap();
    assert_eq!(
        resolved.lease.actual_saved_identity(),
        Some((row.id.as_str(), row.revision.as_str()))
    );
    assert_eq!(resolved.root_proof.profile_id, "/private/backend-home");
    assert!(matches!(
        resolved.connection.route_owner().unwrap().door(),
        RouteDoor::Legacy
    ));
    core.commit_legacy(resolved, None, &Arc::new(AtomicBool::new(true)))
        .unwrap();
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires isolated private Secret Service"]
fn installed_ssh_route_proves_same_socket_and_keeps_lease_until_retirement() {
    let home = std::env::var("HOME").unwrap();
    let root = std::path::Path::new(&home).parent().unwrap();
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("rune-private-credentials-")
    );
    assert_eq!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
        format!("unix:path={}", root.join("bus").display())
    );
    let fake = crate::connections::installed_ssh::test_fixture::Fixture::new();
    let private = Private::new();
    let mut core = Core::system(private.path.clone());
    core.ssh_binary = fake.ssh_binary.clone();
    let spec = &fake.spec;
    let mut ssh = domain::InstalledSsh::new(spec.host.clone());
    ssh.user = spec.user.clone();
    ssh.port = spec.port;
    ssh.remote_hermes_path = spec.remote_hermes_path.clone();
    ssh.remote_profile = spec.remote_profile.clone();
    let scope = Scope::Profile("default".into());
    core.save_legacy(Edit {
        scope,
        expected: absent(),
        value: ValueEdit::Profile(Profile::Ssh(ssh)),
        auth: AuthEdit::default(),
    })
    .unwrap();
    let alive = Arc::new(AtomicBool::new(true));
    let resolved = core.resolve_legacy(request("default"), &alive).unwrap();
    let resolved = core.commit_legacy(resolved, None, &alive).unwrap();
    assert!(resolved.lease.is_valid());
    let installation = super::super::installation::load_or_create(&private.path).unwrap();
    let owner = crate::connections::installed_ssh::ownership_id(&installation, "default").unwrap();
    let credential_id = format!("legacy-ssh-{owner}");
    let generation = fake.generation(&owner).unwrap();
    assert!(
        SystemSecrets
            .get_optional(&credential_id)
            .unwrap()
            .is_some()
    );
    let calls = fake.calls();
    let roster = calls
        .iter()
        .find(|(_, call)| call["method"] == "profiles.list")
        .unwrap();
    let runtime = calls
        .iter()
        .find(|(_, call)| call["method"] == "runtime.describe")
        .unwrap();
    assert_eq!(roster.0, runtime.0);
    assert_eq!(runtime.1["params"], json!({}));
    let prepared = crate::gateway::prepared::PreparedTransport::from_connection(
        &resolved.connection,
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    let url = prepared.base().join("api/profiles").unwrap();
    let parts = prepared
        .prepare_http_method(
            reqwest::Method::GET,
            url.clone(),
            None,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    assert!(parts.url.query_pairs().all(|(key, _)| key != "token"));
    let token = match resolved.connection.transport() {
        Connection::Url {
            token: Some(token), ..
        } => token,
        _ => panic!("expected private SSH endpoint"),
    };
    assert_eq!(
        parts.headers.get("x-hermes-session-token").unwrap(),
        token.as_str()
    );
    let roster = crate::gateway::roster::read(
        &resolved.connection,
        Instant::now() + Duration::from_secs(5),
        false,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(
        roster
            .profiles
            .iter()
            .any(|profile| profile.name == "default")
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .unwrap();
    for response in [
        client.get(url.clone()).send(),
        client
            .get(url.clone())
            .header("x-hermes-session-token", "wrong-private-token")
            .send(),
        client.get(url.clone()).query(&[("token", token)]).send(),
    ] {
        assert!(response.map(|r| !r.status().is_success()).unwrap_or(true));
    }
    drop(resolved);
    core.poll_route_leases();
    drop(core);
    assert!(
        SystemSecrets
            .get_optional(&credential_id)
            .unwrap()
            .is_none()
    );
    assert!(generation.files_retired().unwrap());
    assert!(!generation.running().unwrap());
    drop(fake);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires isolated private Secret Service"]
fn installed_ssh_cancellation_and_route_edits_fence_inflight_proof_and_cleanup() {
    let home = std::env::var("HOME").unwrap();
    let root = std::path::Path::new(&home).parent().unwrap();
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("rune-private-credentials-")
    );
    assert_eq!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
        format!("unix:path={}", root.join("bus").display())
    );
    for cancel in [true, false] {
        let fake = crate::connections::installed_ssh::test_fixture::Fixture::new();
        let private = Private::new();
        let mut core = Core::system(private.path.clone());
        core.ssh_binary = fake.ssh_binary.clone();
        let mut ssh = domain::InstalledSsh::new(fake.spec.host.clone());
        ssh.user = fake.spec.user.clone();
        ssh.port = fake.spec.port;
        ssh.remote_hermes_path = fake.spec.remote_hermes_path.clone();
        ssh.remote_profile = fake.spec.remote_profile.clone();
        let scope = Scope::Profile("default".into());
        let saved = core
            .save_legacy(Edit {
                scope: scope.clone(),
                expected: absent(),
                value: ValueEdit::Profile(Profile::Ssh(ssh)),
                auth: AuthEdit::default(),
            })
            .unwrap();
        let installation = super::super::installation::load_or_create(&private.path).unwrap();
        let owner =
            crate::connections::installed_ssh::ownership_id(&installation, "default").unwrap();
        let credential_id = format!("legacy-ssh-{owner}");
        let alive = Arc::new(AtomicBool::new(true));
        fake.proof_barrier.arm();
        let worker_alive = alive.clone();
        let worker =
            std::thread::spawn(move || core.resolve_legacy(request("default"), &worker_alive));
        assert!(fake.proof_barrier.wait_entered(Duration::from_secs(8)));
        let generation = fake.generation(&owner).unwrap();
        assert!(
            SystemSecrets
                .get_optional(&credential_id)
                .unwrap()
                .is_some()
        );
        if cancel {
            alive.store(false, Ordering::Release);
        } else {
            let mut writer = Core::system(private.path.clone());
            writer
                .save_legacy(edit(
                    scope,
                    saved.version,
                    "https://private-changed.invalid/",
                    SecretEdit::Clear,
                ))
                .unwrap();
        }
        fake.proof_barrier.release();
        assert!(worker.join().unwrap().is_err());
        assert!(
            SystemSecrets
                .get_optional(&credential_id)
                .unwrap()
                .is_none()
        );
        assert!(generation.files_retired().unwrap());
        assert!(!generation.running().unwrap());
        drop(fake);
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires isolated private Secret Service"]
fn generic_oauth_route_uses_its_keyring_record_for_ticket_and_same_socket_proof() {
    use std::io::{Read, Write};
    let home = std::env::var("HOME").unwrap();
    let root = std::path::Path::new(&home).parent().unwrap();
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("rune-private-credentials-")
    );
    assert_eq!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
        format!("unix:path={}", root.join("bus").display())
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut mint, _) = listener.accept().unwrap();
        mint.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut bytes = [0u8; 8192];
        let count = mint.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..count]).to_ascii_lowercase();
        assert!(request.starts_with("post /api/auth/ws-ticket "));
        assert!(request.contains("authorization: bearer private-access"));
        assert!(request.contains("x-private-access: fixture-header"));
        let body = r#"{"ticket":"private-once"}"#;
        write!(
            mint,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        drop(mint);
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept_hdr(
            stream,
            |request: &tungstenite::handshake::server::Request,
             response: tungstenite::handshake::server::Response| {
                assert_eq!(request.uri().query(), Some("ticket=private-once"));
                assert_eq!(
                    request.headers().get("x-private-access").unwrap(),
                    "fixture-header"
                );
                Ok(response)
            },
        )
        .unwrap();
        let mut calls = Vec::new();
        for _ in 0..2 {
            let message = socket.read().unwrap().into_text().unwrap();
            let request: Value = serde_json::from_str(&message).unwrap();
            let result = match request["method"].as_str().unwrap() {
                "profiles.list" => {
                    json!({"profiles":[{"name":"default","path":"/private/oauth-home"}],"install_id":"private-oauth-install"})
                }
                "runtime.describe" => {
                    assert_eq!(request["params"], json!({}));
                    json!({"profile_id":"/private/oauth-home","instance_id":"private-oauth-instance","authority_epoch":1})
                }
                _ => panic!("unexpected private gateway request"),
            };
            calls.push(request["method"].as_str().unwrap().to_owned());
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
        calls
    });
    let fixture = Private::new();
    let mut core = Core::system(fixture.path.clone());
    let scope = Scope::Profile("default".into());
    let mut settings = edit(scope.clone(), absent(), &endpoint, SecretEdit::Clear);
    if let ValueEdit::Profile(Profile::Remote(remote)) = &mut settings.value {
        remote.auth_mode = domain::AuthMode::Oauth;
    }
    settings.auth.headers.insert(
        "x-private-access".into(),
        SecretEdit::Set("fixture-header".into()),
    );
    let saved = core.save_legacy(settings).unwrap();
    core.install_cloud_tokens(
        scope,
        saved.version,
        crate::connections::cloud::Tokens {
            access_token: "private-access".into(),
            refresh_token: "private-refresh".into(),
            expires_at: i64::MAX,
            provider: String::new(),
            user_id: String::new(),
        },
    )
    .unwrap();
    let alive = Arc::new(AtomicBool::new(true));
    let resolved = core.resolve_legacy(request("default"), &alive).unwrap();
    let resolved = core.commit_legacy(resolved, None, &alive).unwrap();
    assert!(resolved.lease.is_valid());
    assert_eq!(
        server.join().unwrap(),
        ["profiles.list", "runtime.describe"]
    );
    assert!(
        !fs::read_to_string(&fixture.path)
            .unwrap()
            .contains("private-refresh")
    );
}
