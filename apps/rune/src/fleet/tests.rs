use super::*;
use crate::{
    gateway::{
        Connection,
        roster::{Identity, Read},
    },
    source::SourceStamp,
};
use std::sync::{Condvar, atomic::AtomicUsize};
use worker::{Job, Outcome, Runner};

fn descriptor(id: &str, revision: &str, kind: Kind, order: usize) -> Descriptor {
    Descriptor {
        version: SourceVersion {
            id: SourceId::Saved(id.into()),
            revision: revision.into(),
        },
        label: id.into(),
        kind,
        registration_order: order,
    }
}
fn profiles(names: &[&str]) -> Vec<RosterProfile> {
    names
        .iter()
        .map(|name| RosterProfile {
            name: (*name).into(),
            metadata: ProfileMetadata::default(),
            canonical_session: None,
        })
        .collect()
}
fn success(job: &Job, names: &[&str], install: Option<&str>) -> Outcome {
    let connection = Connection::Saved {
        id: match &job.descriptor.version.id {
            SourceId::Saved(id) => id.clone(),
            _ => unreachable!(),
        },
        revision: job.descriptor.version.revision.clone(),
        inner: Box::new(Connection::Url {
            endpoint: "http://fixture.invalid".into(),
            token: None,
        }),
    };
    Outcome::Gateway {
        binding: None,
        stamp: SourceStamp::from_connection(&connection).unwrap(),
        read: Read {
            profiles: profiles(names),
            identity: if job.identity {
                Identity::Observed(install.map(str::to_owned))
            } else {
                Identity::NotRequested
            },
        },
    }
}
fn state(rows: Vec<Descriptor>) -> State {
    let mut state = State::default();
    state.update_registry(rows, None);
    state
}

#[test]
fn partial_retry_is_event_driven_and_forced_churn_queues_only_one_followup() {
    let now = Instant::now();
    let mut state = state(vec![descriptor("one", "v1", Kind::Remote, 0)]);
    let first = state.refresh(Reason::Mount, now);
    assert_eq!(first.len(), 1);
    for _ in 0..8 {
        assert!(state.refresh(Reason::GatewayOpened, now).is_empty());
    }
    let followup = state.complete(&first[0], Outcome::Failed("offline".into()), now);
    assert_eq!(
        followup.len(),
        1,
        "forced recovery must not turn focus churn into many reads"
    );
    state.complete(&followup[0], Outcome::Failed("offline".into()), now);
    assert!(
        state
            .refresh(Reason::Focus, now + Duration::from_secs(4))
            .is_empty()
    );
    let retry = state.refresh(Reason::Visible, now + Duration::from_secs(5));
    assert_eq!(retry.len(), 1);
    state.complete(
        &retry[0],
        success(&retry[0], &["default", "research"], None),
        now + Duration::from_secs(5),
    );
    assert!(
        state
            .refresh(Reason::Focus, now + Duration::from_secs(64))
            .is_empty()
    );
    assert_eq!(
        state
            .refresh(Reason::Focus, now + Duration::from_secs(65))
            .len(),
        1
    );
}

#[test]
fn failed_remote_keeps_names_but_failed_local_does_not_and_unknown_url_invents_nothing() {
    let now = Instant::now();
    let mut state = state(vec![
        descriptor("remote", "1", Kind::Remote, 0),
        descriptor("local", "1", Kind::Local, 1),
        descriptor("unknown", "1", Kind::Remote, 2),
    ]);
    let jobs = state.refresh(Reason::Mount, now);
    for job in &jobs {
        let result = if job.descriptor.label == "unknown" {
            Outcome::Failed("offline".into())
        } else {
            success(job, &["default", "research"], None)
        };
        state.complete(job, result, now);
    }
    let jobs = state.refresh(Reason::Explicit, now);
    for job in &jobs {
        state.complete(job, Outcome::Failed("sleeping".into()), now);
    }
    let row = |name: &str| {
        state
            .snapshot
            .sources
            .iter()
            .find(|s| s.descriptor.label == name)
            .unwrap()
    };
    assert_eq!(
        row("remote")
            .profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["default", "research"]
    );
    assert!(row("local").profiles.is_empty());
    assert!(row("unknown").profiles.is_empty());
    assert_eq!(state.snapshot.agents.len(), 2);
}

#[test]
fn changed_registry_owner_discards_delayed_profiles_identity_and_old_cache() {
    let now = Instant::now();
    let original = descriptor("same", "first", Kind::Remote, 0);
    let mut state = state(vec![original]);
    let jobs = state.refresh(Reason::Mount, now);
    state.update_registry(
        vec![descriptor("same", "replacement", Kind::Remote, 0)],
        None,
    );
    state.complete(
        &jobs[0],
        success(&jobs[0], &["foreign"], Some("old-install")),
        now,
    );
    assert!(state.snapshot.agents.is_empty());
    assert_eq!(
        state.snapshot.sources[0].descriptor.version.revision,
        "replacement"
    );
    assert!(state.snapshot.sources[0].install_id.is_none());
}

#[test]
fn backend_dedup_uses_known_identity_then_primary_priority_before_handles() {
    let now = Instant::now();
    let a = descriptor("url", "1", Kind::Remote, 0);
    let b = descriptor("ssh", "1", Kind::Ssh, 1);
    let c = descriptor("other", "1", Kind::Remote, 2);
    let mut state = state(vec![a.clone(), b.clone(), c.clone()]);
    let jobs = state.refresh(Reason::Mount, now);
    for job in &jobs {
        state.complete(
            job,
            success(
                job,
                &["research"],
                if job.descriptor.label == "other" {
                    None
                } else {
                    Some("one-install")
                },
            ),
            now,
        );
    }
    assert_eq!(state.snapshot.agents.len(), 2);
    assert!(
        state
            .snapshot
            .agents
            .iter()
            .any(|a| a.source.id == b.version.id && a.handle == "research-ssh")
    );
    state.update_registry(vec![a.clone(), b, c], Some(a.version.id.clone()));
    assert_eq!(state.snapshot.agents.len(), 2);
    assert!(
        state
            .snapshot
            .agents
            .iter()
            .any(|row| row.source.id == a.version.id && row.handle == "research-url")
    );
    assert!(
        state
            .snapshot
            .agents
            .iter()
            .any(|row| row.handle == "research-other")
    );
}

#[test]
fn identity_positive_negative_ttls_and_transient_failure_do_not_flap_dedup() {
    let now = Instant::now();
    let mut state = state(vec![descriptor("one", "1", Kind::Remote, 0)]);
    let first = state.refresh(Reason::Mount, now);
    assert!(first[0].identity);
    state.complete(
        &first[0],
        success(&first[0], &["default"], Some("install")),
        now,
    );
    let next = state.refresh(Reason::Explicit, now + Duration::from_secs(299));
    assert!(!next[0].identity);
    let mut result = success(&next[0], &["default"], None);
    if let Outcome::Gateway { read, .. } = &mut result {
        read.identity = Identity::NotRequested;
    }
    state.complete(&next[0], result, now + Duration::from_secs(299));
    let next = state.refresh(Reason::Explicit, now + Duration::from_secs(300));
    assert!(next[0].identity);
    let mut result = success(&next[0], &["default"], None);
    if let Outcome::Gateway { read, .. } = &mut result {
        read.identity = Identity::Unavailable;
    }
    state.complete(&next[0], result, now + Duration::from_secs(300));
    assert_eq!(
        state.snapshot.sources[0].install_id.as_deref(),
        Some("install")
    );
    let next = state.refresh(Reason::Explicit, now + Duration::from_secs(301));
    state.complete(
        &next[0],
        success(&next[0], &["default"], None),
        now + Duration::from_secs(301),
    );
    let next = state.refresh(Reason::Explicit, now + Duration::from_secs(360));
    assert!(!next[0].identity);
    state.complete(
        &next[0],
        success(&next[0], &["default"], None),
        now + Duration::from_secs(360),
    );
    assert!(state.refresh(Reason::Explicit, now + Duration::from_secs(361))[0].identity);
}

#[test]
fn ssh_inventory_is_not_gateway_proof_and_never_seen_ssh_has_no_fabricated_profile() {
    let now = Instant::now();
    let mut state = state(vec![descriptor("box", "1", Kind::Ssh, 0)]);
    let jobs = state.refresh(Reason::Mount, now);
    state.complete(&jobs[0], Outcome::Inventory(Err("unavailable".into())), now);
    assert!(state.snapshot.agents.is_empty());
    assert_eq!(state.snapshot.sources[0].status, Status::ConnectOnDemand);
    assert!(state.snapshot.sources[0].evidence.is_none());
    let jobs = state.refresh(Reason::Explicit, now + Duration::from_secs(59));
    assert!(!jobs[0].inventory);
    state.complete(&jobs[0], Outcome::Deferred, now + Duration::from_secs(59));
    let jobs = state.refresh(Reason::Explicit, now + Duration::from_secs(60));
    assert!(jobs[0].inventory);
    state.complete(
        &jobs[0],
        Outcome::Inventory(Ok(inventory::ssh::Inventory {
            profiles: vec!["default".into(), "research".into()],
            install_id: None,
        })),
        now + Duration::from_secs(60),
    );
    assert_eq!(
        state.snapshot.sources[0].evidence,
        Some(Evidence::SshInventory)
    );
    assert!(state.snapshot.sources[0].principal_stamp.is_none());
    assert!(state.rows.values().next().unwrap().binding.is_none());
    assert!(!state.refresh(Reason::Explicit, now + Duration::from_secs(3600))[0].inventory);
}

struct FakeRunner {
    calls: AtomicUsize,
    gate: (Mutex<bool>, Condvar),
}
impl Runner for FakeRunner {
    fn run(&self, job: &Job) -> Outcome {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut allowed = self.gate.0.lock().unwrap();
        while !*allowed {
            allowed = self.gate.1.wait(allowed).unwrap();
        }
        success(job, &["default", "research"], None)
    }
}
#[test]
fn actual_worker_cache_survives_opener_drop_without_retaining_an_idle_service_forever() {
    let mut opener = Fleet::default();
    opener.update_registry(vec![descriptor("source", "1", Kind::Remote, 0)], None);
    let runner = Arc::new(FakeRunner {
        calls: AtomicUsize::new(0),
        gate: (Mutex::new(false), Condvar::new()),
    });
    opener.service.start(runner.clone());
    opener.refresh(Reason::Mount);
    let mut peer = opener.clone();
    let weak = Arc::downgrade(&opener.service);
    drop(opener);
    *runner.gate.0.lock().unwrap() = true;
    runner.gate.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        peer.poll();
        if !peer.snapshot().refreshing {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(peer.snapshot().agents.len(), 2);
    assert_eq!(runner.calls.load(Ordering::Relaxed), 1);
    assert!(
        !peer
            .snapshot()
            .agents
            .iter()
            .any(|a| a.profile == "invented")
    );
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(2);
    while weak.upgrade().is_some() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn last_ui_drop_kills_and_reaps_owned_inventory_even_while_a_cache_handle_survives() {
    use std::{
        path::PathBuf,
        process::{Command, Stdio},
    };
    struct ChildRunner {
        children: Arc<inventory::ssh::Children>,
        pid_file: PathBuf,
    }
    impl Runner for ChildRunner {
        fn run(&self, job: &Job) -> Outcome {
            // A private fake SSH executable: one shell process, no sleep child,
            // no remote connection, and no user process lookup/termination.
            let mut command = Command::new("sh");
            command
                .args([
                    "-c",
                    "printf '%s' $$ > \"$1\"; while :; do :; done",
                    "fixture",
                ])
                .arg(&self.pid_file)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            Outcome::Inventory(inventory::ssh::read_command(
                command,
                Instant::now() + Duration::from_secs(30),
                job.cancelled.clone(),
                &self.children,
            ))
        }
    }
    let private =
        std::env::temp_dir().join(format!("hermes-inventory-child-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&private).unwrap();
    let pid_file = private.join("pid");
    let mut ui = Fleet::default();
    ui.register(Arc::new(|| {}));
    ui.update_registry(vec![descriptor("box", "1", Kind::Ssh, 0)], None);
    ui.service.start(Arc::new(ChildRunner {
        children: ui.service.children.clone(),
        pid_file: pid_file.clone(),
    }));
    let cache_only = ui.clone();
    let mut peer_ui = ui.clone();
    peer_ui.register(Arc::new(|| {}));
    ui.refresh(Reason::Mount);
    let limit = Instant::now() + Duration::from_secs(2);
    let pid = loop {
        if let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok())
        {
            break pid;
        }
        assert!(Instant::now() < limit);
        std::thread::sleep(Duration::from_millis(2));
    };
    let process = PathBuf::from(format!("/proc/{pid}"));
    assert!(process.exists());
    drop(ui);
    assert!(
        process.exists(),
        "a surviving peer must keep the shared inventory alive"
    );
    peer_ui.detach();
    peer_ui.detach();
    assert!(
        !process.exists(),
        "last UI Drop returned with an owned inventory child still alive"
    );
    assert!(cache_only.service.children.is_empty());
    drop(cache_only);
    std::fs::remove_dir_all(private).unwrap();
}

#[test]
fn active_direct_alias_only_maps_by_proven_backend_and_the_actual_profile() {
    let now = Instant::now();
    let saved = descriptor("registered", "1", Kind::Remote, 0);
    let binding = Binding::new(
        Connection::Url {
            endpoint: "http://current.invalid".into(),
            token: Some("fixture".into()),
        },
        None,
    )
    .unwrap();
    let direct = binding.stamp().version().clone();
    let mut state = state(vec![saved.clone()]);
    let profile = Profile {
        name: "research".into(),
        label: "Current title".into(),
        description: "Current description".into(),
        canonical_session: None,
    };
    state.seed_current(0, binding.clone(), &[profile], None);
    let jobs = state.refresh(Reason::Mount, now);
    for job in &jobs {
        let outcome = if job.descriptor.version == direct {
            Outcome::Gateway {
                binding: Some(binding.clone()),
                stamp: binding.stamp().clone(),
                read: Read {
                    profiles: profiles(&["research", "current-only"]),
                    identity: Identity::Observed(Some("same-install".into())),
                },
            }
        } else {
            success(job, &["research"], Some("same-install"))
        };
        state.complete(job, outcome, now);
    }
    assert_eq!(
        state.snapshot.canonical_source(&direct, "research"),
        Some(saved.version)
    );
    assert_eq!(
        state.snapshot.canonical_source(&direct, "current-only"),
        Some(direct.clone())
    );
    assert_eq!(state.snapshot.canonical_source(&direct, "not-served"), None);
    let mut unknown = state.snapshot.clone();
    for source in &mut unknown.sources {
        source.install_id = None;
    }
    assert_eq!(
        unknown.canonical_source(&direct, "research"),
        None,
        "missing identity cannot prove an alias"
    );
}

#[test]
fn sole_window_parked_direct_source_survives_b_seed_and_transfers_back_without_a_fake_success() {
    let a = Binding::new(
        Connection::Url {
            endpoint: "http://a.invalid".into(),
            token: Some("private-a".into()),
        },
        None,
    )
    .unwrap();
    let b = Binding::new(
        Connection::Url {
            endpoint: "http://b.invalid".into(),
            token: Some("private-b".into()),
        },
        None,
    )
    .unwrap();
    let a_version = a.stamp().version().clone();
    let b_version = b.stamp().version().clone();
    let mut fleet = Fleet::default();
    fleet.register(Arc::new(|| {}));
    fleet.update_registry(vec![], None);
    let profile = Profile {
        name: "research".into(),
        label: "A research".into(),
        description: String::new(),
        canonical_session: None,
    };
    fleet.seed_current(a.clone(), &[profile.clone()]).unwrap();
    // A prior failed refresh must stay failed: retention is ownership, not a new gateway read.
    {
        let mut state = fleet.service.state.lock().unwrap();
        state.rows.get_mut(&a_version).unwrap().source.status =
            Status::Unavailable("sleeping".into());
    }
    let retained = fleet.retain_source(a.clone(), None).unwrap();
    fleet.seed_current(b, &[profile]).unwrap();
    assert!(retained.validate().is_ok());
    assert_eq!(fleet.binding(&a_version).unwrap().stamp(), a.stamp());
    let source = fleet
        .snapshot()
        .sources
        .iter()
        .find(|s| s.descriptor.version == a_version)
        .unwrap();
    assert_eq!(source.status, Status::Unavailable("sleeping".into()));
    assert_eq!(source.profiles.len(), 1);
    fleet.adopt_source(&retained).unwrap();
    drop(retained);
    assert_eq!(fleet.binding(&a_version).unwrap().stamp(), a.stamp());
    assert!(fleet.binding(&b_version).is_none());
    assert!(
        fleet
            .snapshot()
            .sources
            .iter()
            .any(|s| s.descriptor.version == a_version)
    );
}
#[test]
fn retention_without_cached_metadata_proves_only_binding_and_expires_on_last_real_ui_detach() {
    let binding = Binding::new(
        Connection::Url {
            endpoint: "http://actual.invalid".into(),
            token: Some("private".into()),
        },
        None,
    )
    .unwrap();
    let version = binding.stamp().version().clone();
    let mut fleet = Fleet::default();
    let retained = fleet.retain_source(binding.clone(), None).unwrap();
    assert!(retained.validate().is_ok());
    assert!(fleet.snapshot().sources.is_empty());
    assert!(fleet.snapshot().agents.is_empty());
    fleet.adopt_source(&retained).unwrap();
    drop(retained);
    assert_eq!(fleet.binding(&version).unwrap().stamp(), binding.stamp());
    assert!(
        fleet.snapshot().sources.is_empty(),
        "adoption must not fabricate profile metadata or gateway readiness"
    );
    let parked = fleet.retain_source(binding, None).unwrap();
    fleet.register(Arc::new(|| {}));
    let cache_only = fleet.clone();
    fleet.detach();
    assert!(parked.validate().is_err());
    assert!(cache_only.binding(&version).is_none());
    assert!(cache_only.snapshot().agents.is_empty());
    assert!(
        fleet.adopt_source(&parked).is_err(),
        "a closed UI's ownership cannot revive through a cache handle"
    );
    let mut closing_ui = Fleet::default();
    closing_ui.register(Arc::new(|| {}));
    let binding = Binding::new(
        Connection::Url {
            endpoint: "http://drop-owner.invalid".into(),
            token: None,
        },
        None,
    )
    .unwrap();
    let version = binding.stamp().version().clone();
    let held = closing_ui.retain_source(binding, None).unwrap();
    let cache_only = closing_ui.clone();
    drop(closing_ui);
    assert!(
        held.validate().is_err(),
        "last UI Drop must invalidate retained guards too"
    );
    assert!(cache_only.binding(&version).is_none());
}
#[test]
fn a_retention_guard_cannot_be_adopted_by_an_unrelated_fleet_service() {
    let binding = Binding::new(
        Connection::Url {
            endpoint: "http://actual.invalid".into(),
            token: None,
        },
        None,
    )
    .unwrap();
    let mut first = Fleet::default();
    let guard = first.retain_source(binding, None).unwrap();
    let mut other = Fleet::default();
    assert!(other.adopt_source(&guard).is_err());
    assert!(other.snapshot().sources.is_empty());
}
