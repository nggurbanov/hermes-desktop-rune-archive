//! Startup policy changes metadata, never the active workspace or an agent.
use super::*;
use std::time::Duration;

fn local(label: &str) -> Input {
    Input {
        id: None,
        expected_revision: None,
        label: label.into(),
        destination: Destination::Local {
            home: std::env::temp_dir(),
        },
        secret: SecretChange::Keep,
    }
}
fn profile(name: &str) -> crate::gateway::Profile {
    crate::gateway::Profile {
        name: name.into(),
        label: name.into(),
        description: String::new(),
        canonical_session: None,
    }
}
struct NamedProbe {
    base: FakeProbe,
    profiles: Vec<crate::gateway::Profile>,
}
impl Probe for NamedProbe {
    fn check(&mut self, connection: &Connection) -> Result<Vec<crate::gateway::Profile>, String> {
        self.base.check(connection)?;
        Ok(self.profiles.clone())
    }
}
fn profiles(core: &mut Core, probe: &FakeProbe, names: &[&str]) {
    core.probe = Box::new(NamedProbe {
        base: probe.clone(),
        profiles: names.iter().map(|name| profile(name)).collect(),
    });
}
fn ui_worker(
    core: Core,
) -> (
    Connections,
    Resolver,
    Receiver<()>,
    std::thread::JoinHandle<()>,
) {
    let (send, resolver, receive, done, worker) = retained_worker(core);
    let mut ui = Connections::new();
    ui.network = Some(Network::new(send, receive));
    ui.load();
    wait_until(
        || {
            ui.poll();
            !ui.busy
        },
        "registry did not load",
    );
    assert!(ui.error.is_none());
    (ui, resolver, done, worker)
}
fn finish(
    ui: Connections,
    resolver: Resolver,
    done: Receiver<()>,
    worker: std::thread::JoinHandle<()>,
) {
    drop(ui);
    drop(resolver);
    done.recv_timeout(Duration::from_secs(3)).unwrap();
    worker.join().unwrap();
}

#[test]
fn legacy_selected_migrates_without_activating_and_policy_writes_preserve_opaque_fields() {
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut seed = json_core(file.path(), &secrets, &probe, &tunnels);
    seed.execute(Command::Save(remote(
        None,
        "Old selected",
        SecretChange::Keep,
    )))
    .unwrap();
    let selected = seed.registry.rows[0].clone();
    seed.execute(Command::Save(local("This device"))).unwrap();
    let local = seed.registry.rows[1].clone();
    let mut raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(file.path()).unwrap()).unwrap();
    let object = raw.as_object_mut().unwrap();
    for name in ["primary", "lastUsed", "launchMode", "lastProfiles"] {
        object.remove(name);
    }
    object.insert("selected".into(), serde_json::json!(selected.id));
    object.insert(
        "future_policy".into(),
        serde_json::json!({"nested":["retained", 7]}),
    );
    raw["rows"][0]["future_row"] = serde_json::json!({"opaque": true});
    raw["rows"][0]["destination"]["future_transport"] = serde_json::json!([1, 2, 3]);
    crate::client::private_write(&file.path(), &serde_json::to_vec(&raw).unwrap()).unwrap();
    let original = std::fs::read(file.path()).unwrap();
    let mut core = json_core(file.path(), &secrets, &probe, &tunnels);
    let Outcome::Registry(visible, _) = core.execute(Command::Load).unwrap() else {
        panic!()
    };
    assert!(
        visible.selected.is_none(),
        "a stored last selection became this worker's active source"
    );
    assert_eq!(visible.primary.as_deref(), Some(selected.id.as_str()));
    assert_eq!(visible.last_used.as_deref(), Some(selected.id.as_str()));
    assert_eq!(visible.launch_mode, LaunchMode::Primary);
    assert_eq!(
        std::fs::read(file.path()).unwrap(),
        original,
        "merely loading rewrote migration state"
    );
    core.execute(Command::MakePrimary(local.id.clone()))
        .unwrap();
    core.execute(Command::LaunchMode(LaunchMode::LastUsed))
        .unwrap();
    let mut editor = Editor::existing(&selected);
    editor.label.replace_text("Edited after migration");
    core.execute(Command::Save(editor.input().unwrap()))
        .unwrap();
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(file.path()).unwrap()).unwrap();
    assert_eq!(stored["future_policy"], raw["future_policy"]);
    assert_eq!(
        stored["rows"][0]["future_row"],
        raw["rows"][0]["future_row"]
    );
    assert_eq!(
        stored["rows"][0]["destination"]["future_transport"],
        raw["rows"][0]["destination"]["future_transport"]
    );
    let restarted = JsonMetadata(file.path()).load().unwrap();
    assert_eq!(restarted.primary.as_deref(), Some(local.id.as_str()));
    assert_eq!(restarted.candidate().unwrap().id, selected.id);
    assert!(secrets.1.lock().unwrap().is_empty());
    assert!(probe.0.lock().unwrap().is_empty());
    assert!(tunnels.calls.lock().unwrap().is_empty());
}

#[test]
fn primary_last_used_and_confirmed_profile_are_distinct_and_forget_uses_local_fallback() {
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut core = json_core(file.path(), &secrets, &probe, &tunnels);
    profiles(&mut core, &probe, &["default", "research"]);
    core.execute(Command::Save(local("This device"))).unwrap();
    let local = core.registry.rows[0].clone();
    core.execute(Command::Save(remote(None, "Primary", SecretChange::Keep)))
        .unwrap();
    let primary = core.registry.rows[1].clone();
    core.execute(Command::Save(remote(None, "Last used", SecretChange::Keep)))
        .unwrap();
    let last = core.registry.rows[2].clone();
    core.execute(Command::MakePrimary(primary.id.clone()))
        .unwrap();
    let lease = core.resolve_lease(&last.id, Some(&last.revision)).unwrap();
    assert_eq!(
        core.registry.last_used.as_deref(),
        Some(local.id.as_str()),
        "resolution was mistaken for activation"
    );
    core.execute(Command::CommitLease(lease.clone(), Some("research".into())))
        .unwrap();
    assert_eq!(core.registry.primary.as_deref(), Some(primary.id.as_str()));
    assert_eq!(core.registry.last_used.as_deref(), Some(last.id.as_str()));
    assert_eq!(
        core.registry
            .last_profiles
            .get(&last.id)
            .map(String::as_str),
        Some("research")
    );
    assert_eq!(core.registry.candidate().unwrap().id, primary.id);
    core.execute(Command::LaunchMode(LaunchMode::LastUsed))
        .unwrap();
    let candidate = core.registry.candidate().unwrap();
    assert_eq!(candidate.id, last.id);
    assert_eq!(candidate.revision, last.revision);
    assert_eq!(candidate.profile, "research");
    core.execute(Command::MakePrimary(local.id.clone()))
        .unwrap();
    assert_eq!(core.registry.launch_mode, LaunchMode::LastUsed);
    assert_eq!(core.registry.candidate().unwrap().id, last.id);
    core.execute(Command::MakePrimary(primary.id.clone()))
        .unwrap();
    let before = std::fs::read(file.path()).unwrap();
    assert!(
        core.execute(Command::CommitLease(lease, Some("missing".into())))
            .is_err()
    );
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
    core.execute(Command::Forget(primary.id.clone())).unwrap();
    assert_eq!(core.registry.primary.as_deref(), Some(local.id.as_str()));
    assert_eq!(core.registry.last_used.as_deref(), Some(last.id.as_str()));
    core.execute(Command::Forget(last.id.clone())).unwrap();
    assert_eq!(core.registry.last_used.as_deref(), Some(local.id.as_str()));
    assert_eq!(core.registry.candidate().unwrap().id, local.id);
    assert!(!core.registry.last_profiles.contains_key(&last.id));
    let before = std::fs::read(file.path()).unwrap();
    assert!(core.execute(Command::Forget(local.id.clone())).is_err());
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
    assert!(
        core.execute(Command::Forget("already-absent".into()))
            .is_ok()
    );
}

#[test]
fn missing_last_used_falls_back_to_primary_and_no_local_source_is_ever_invented() {
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut core = json_core(file.path(), &secrets, &probe, &tunnels);
    core.execute(Command::Save(remote(None, "First", SecretChange::Keep)))
        .unwrap();
    let first = core.registry.rows[0].clone();
    core.execute(Command::Save(remote(None, "Second", SecretChange::Keep)))
        .unwrap();
    let second = core.registry.rows[1].clone();
    assert_eq!(core.registry.primary.as_deref(), Some(first.id.as_str()));
    let mut raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(file.path()).unwrap()).unwrap();
    raw["lastUsed"] = serde_json::json!("removed-source");
    raw["launchMode"] = serde_json::json!("last-used");
    crate::client::private_write(&file.path(), &serde_json::to_vec(&raw).unwrap()).unwrap();
    core.execute(Command::Load).unwrap();
    assert_eq!(core.registry.candidate().unwrap().id, first.id);
    core.execute(Command::Forget(first.id)).unwrap();
    assert!(core.registry.primary.is_none());
    assert!(core.registry.candidate().is_none());
    assert_eq!(core.registry.rows.len(), 1);
    assert_eq!(core.registry.rows[0].id, second.id);
    assert_eq!(core.registry.rows[0].kind(), Kind::Remote);
    core.execute(Command::MakePrimary(second.id.clone()))
        .unwrap();
    assert_eq!(core.registry.candidate().unwrap().id, second.id);
    assert!(probe.0.lock().unwrap().is_empty());
}

#[test]
fn profile_memory_remaps_only_the_confirmed_matching_source_revision_and_keeps_missing_profile_errors()
 {
    let (mut core, metadata, secrets, probe, _) = core();
    core.execute(Command::Save(remote(None, "One", SecretChange::Keep)))
        .unwrap();
    let one = core.registry.rows[0].clone();
    core.execute(Command::Save(remote(None, "Two", SecretChange::Keep)))
        .unwrap();
    let two = core.registry.rows[1].clone();
    for row in [&one, &two] {
        core.execute(Command::RememberProfile {
            id: row.id.clone(),
            revision: row.revision.clone(),
            profile: "research".into(),
        })
        .unwrap();
    }
    core.execute(Command::ProfileChanged {
        id: one.id.clone(),
        revision: one.revision.clone(),
        old: "research".into(),
        new: Some("renamed".into()),
    })
    .unwrap();
    assert_eq!(
        core.registry.last_profiles.get(&one.id).map(String::as_str),
        Some("renamed")
    );
    assert_eq!(
        core.registry.last_profiles.get(&two.id).map(String::as_str),
        Some("research")
    );
    let before = serde_json::to_vec(&*metadata.data.lock().unwrap()).unwrap();
    assert!(
        core.execute(Command::RememberProfile {
            id: one.id.clone(),
            revision: uuid::Uuid::new_v4().to_string(),
            profile: "stale".into()
        })
        .is_err()
    );
    assert_eq!(
        serde_json::to_vec(&*metadata.data.lock().unwrap()).unwrap(),
        before
    );
    assert!(probe.0.lock().unwrap().is_empty());
    assert!(secrets.1.lock().unwrap().is_empty());
    assert!(
        core.execute(Command::Select(one.id.clone())).is_err(),
        "a missing remembered profile silently fell back to default"
    );
    assert_eq!(
        serde_json::to_vec(&*metadata.data.lock().unwrap()).unwrap(),
        before
    );
    core.execute(Command::ProfileChanged {
        id: one.id.clone(),
        revision: one.revision,
        old: "renamed".into(),
        new: None,
    })
    .unwrap();
    core.execute(Command::Select(one.id.clone())).unwrap();
    assert_eq!(
        core.registry.last_profiles.get(&one.id).map(String::as_str),
        Some("default")
    );
    assert_eq!(
        core.registry.last_profiles.get(&two.id).map(String::as_str),
        Some("research")
    );
}

#[test]
fn another_workers_policy_is_observed_without_navigation_editor_loss_or_network_activity() {
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut writer = json_core(file.path(), &secrets, &probe, &tunnels);
    writer
        .execute(Command::Save(remote(None, "One", SecretChange::Keep)))
        .unwrap();
    let one = writer.registry.rows[0].clone();
    writer
        .execute(Command::Save(remote(None, "Two", SecretChange::Keep)))
        .unwrap();
    let two = writer.registry.rows[1].clone();
    let (mut ui, resolver, done, worker) =
        ui_worker(json_core(file.path(), &secrets, &probe, &tunnels));
    assert!(ui.registry_loaded());
    assert!(ui.selected.is_none());
    ui.action(Action::Select(one.id.clone()));
    wait_until(
        || {
            ui.poll();
            !ui.busy
        },
        "initial saved source did not activate",
    );
    let foreground = ui
        .active_lease()
        .expect("Saved Ready lacked its immediate source lease");
    assert_eq!(foreground.source_id(), one.id);
    ui.action(Action::Edit(one.id.clone()));
    ui.editor
        .as_mut()
        .unwrap()
        .label
        .replace_text("Uncommitted label");
    writer
        .execute(Command::MakePrimary(two.id.clone()))
        .unwrap();
    writer
        .execute(Command::LaunchMode(LaunchMode::LastUsed))
        .unwrap();
    writer
        .execute(Command::RememberProfile {
            id: two.id.clone(),
            revision: two.revision,
            profile: "research".into(),
        })
        .unwrap();
    let before = std::fs::read(file.path()).unwrap();
    let calls = probe.0.lock().unwrap().len();
    ui.observe_registry();
    assert!(!ui.busy);
    let mut navigations = 0;
    wait_until(
        || {
            for event in ui.poll() {
                if matches!(event, Event::Ready(_)) {
                    navigations += 1;
                }
            }
            !ui.observing
        },
        "metadata observation did not reply",
    );
    assert_eq!(navigations, 0);
    assert_eq!(ui.primary(), Some(two.id.as_str()));
    assert_eq!(ui.last_used(), Some(one.id.as_str()));
    assert_eq!(ui.launch_mode(), LaunchMode::LastUsed);
    assert_eq!(ui.selected.as_deref(), Some(one.id.as_str()));
    assert_eq!(ui.selected_profile(), Some("default"));
    assert_eq!(ui.remembered_profile(&two.id), Some("research"));
    assert_eq!(ui.startup_candidate().unwrap().unwrap().id, one.id);
    assert_eq!(ui.editor.as_ref().unwrap().label.text, "Uncommitted label");
    assert!(foreground.is_valid());
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
    assert_eq!(probe.0.lock().unwrap().len(), calls);
    assert!(secrets.1.lock().unwrap().is_empty());
    assert!(tunnels.calls.lock().unwrap().is_empty());
    finish(ui, resolver, done, worker);
}

#[test]
fn invalid_future_registry_or_launch_mode_refuses_policy_and_preserves_the_file_and_form() {
    for invalid in [
        b"not JSON".as_slice(),
        br#"{"version":9,"rows":[],"selected":null}"#.as_slice(),
        br#"{"version":1,"rows":[],"selected":null,"launchMode":"future-policy"}"#.as_slice(),
    ] {
        let file = RegistryFile::new();
        let secrets = FakeSecrets::default();
        let probe = FakeProbe::default();
        let tunnels = FakeTunnels::default();
        let mut writer = json_core(file.path(), &secrets, &probe, &tunnels);
        writer
            .execute(Command::Save(remote(None, "One", SecretChange::Keep)))
            .unwrap();
        let row = writer.registry.rows[0].clone();
        let valid = std::fs::read(file.path()).unwrap();
        let (mut ui, resolver, done, worker) =
            ui_worker(json_core(file.path(), &secrets, &probe, &tunnels));
        ui.action(Action::Edit(row.id.clone()));
        ui.editor
            .as_mut()
            .unwrap()
            .label
            .replace_text("Keep this entered form");
        crate::client::private_write(&file.path(), invalid).unwrap();
        for command in [
            Command::MakePrimary(row.id),
            Command::LaunchMode(LaunchMode::LastUsed),
        ] {
            assert!(writer.execute(command).is_err());
            assert_eq!(std::fs::read(file.path()).unwrap(), invalid);
        }
        ui.observe_registry();
        wait_until(
            || {
                ui.poll();
                !ui.observing
            },
            "invalid metadata observation did not finish",
        );
        assert!(ui.startup_candidate().is_err());
        assert!(ui.error.is_some());
        assert_eq!(
            ui.editor.as_ref().unwrap().label.text,
            "Keep this entered form"
        );
        assert_eq!(std::fs::read(file.path()).unwrap(), invalid);
        assert!(secrets.1.lock().unwrap().is_empty());
        assert!(probe.0.lock().unwrap().is_empty());
        assert!(tunnels.calls.lock().unwrap().is_empty());
        crate::client::private_write(&file.path(), &valid).unwrap();
        ui.observe_registry();
        wait_until(
            || {
                ui.poll();
                !ui.observing
            },
            "repaired metadata observation did not finish",
        );
        assert!(ui.startup_candidate().is_ok());
        assert!(ui.error.is_none());
        assert_eq!(
            ui.editor.as_ref().unwrap().label.text,
            "Keep this entered form"
        );
        finish(ui, resolver, done, worker);
    }
}

#[test]
fn local_forget_emits_no_success_and_only_a_successful_explicit_forget_emits_its_identity() {
    let (mut core, _, _, _, _) = core();
    core.execute(Command::Save(local("Local"))).unwrap();
    let local = core.registry.rows[0].clone();
    core.execute(Command::Save(remote(None, "Remote", SecretChange::Keep)))
        .unwrap();
    let remote = core.registry.rows[1].clone();
    let (mut ui, resolver, done, worker) = ui_worker(core);
    let mut forgotten = vec![];
    for id in [&local.id, &remote.id] {
        ui.action(Action::AskForget(id.clone()));
        ui.action(Action::ConfirmForget);
        wait_until(
            || {
                for event in ui.poll() {
                    if let Event::Forgot { id } = event {
                        forgotten.push(id);
                    }
                }
                !ui.busy
            },
            "forget did not complete",
        );
        if id == &local.id {
            assert!(ui.error.is_some());
            assert!(forgotten.is_empty());
        } else {
            assert!(ui.error.is_none());
        }
    }
    assert_eq!(forgotten, vec![remote.id]);
    assert_eq!(ui.rows.len(), 1);
    assert_eq!(ui.rows[0].id, local.id);
    finish(ui, resolver, done, worker);
}

#[test]
fn concurrent_policy_profile_and_row_writers_preserve_each_others_changes_under_the_same_os_guard()
{
    let file = RegistryFile::new();
    let secrets = FakeSecrets::default();
    let probe = FakeProbe::default();
    let tunnels = FakeTunnels::default();
    let mut seed = json_core(file.path(), &secrets, &probe, &tunnels);
    seed.execute(Command::Save(local("Local"))).unwrap();
    seed.execute(Command::Save(remote(
        None,
        "Target",
        SecretChange::Set("existing-reference".into()),
    )))
    .unwrap();
    let target = seed.registry.rows[1].clone();
    let commands = [
        Command::MakePrimary(target.id.clone()),
        Command::LaunchMode(LaunchMode::LastUsed),
        Command::RememberProfile {
            id: target.id.clone(),
            revision: target.revision.clone(),
            profile: "research".into(),
        },
        Command::Save(remote_edit(&target, "Renamed label", SecretChange::Keep)),
    ];
    let before = std::fs::read(file.path()).unwrap();
    let keys = secrets.0.lock().unwrap().clone();
    let key_calls = secrets.1.lock().unwrap().len();
    let held = super::super::registry::transaction(&file.path()).unwrap();
    let start = Arc::new(std::sync::Barrier::new(5));
    let (finished, done) = mpsc::channel();
    let mut workers = vec![];
    for command in commands {
        let mut writer = json_core(file.path(), &secrets, &probe, &tunnels);
        let start = start.clone();
        let finished = finished.clone();
        workers.push(std::thread::spawn(move || {
            start.wait();
            let result = writer.execute(command).map(|_| ());
            finished.send(result.clone()).unwrap();
            result
        }));
    }
    start.wait();
    std::thread::sleep(Duration::from_millis(40));
    assert!(done.try_recv().is_err());
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
    assert_eq!(*secrets.0.lock().unwrap(), keys);
    assert_eq!(secrets.1.lock().unwrap().len(), key_calls);
    drop(held);
    for _ in 0..4 {
        done.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
    }
    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    let stored = JsonMetadata(file.path()).load().unwrap();
    assert_eq!(stored.primary.as_deref(), Some(target.id.as_str()));
    assert_eq!(stored.launch_mode, LaunchMode::LastUsed);
    assert_eq!(
        stored.last_profiles.get(&target.id).map(String::as_str),
        Some("research")
    );
    assert_eq!(stored.rows[1].label, "Renamed label");
    assert_eq!(stored.rows[1].credential, target.credential);
    assert_eq!(*secrets.0.lock().unwrap(), keys);
    assert!(probe.0.lock().unwrap().is_empty());
    assert!(tunnels.calls.lock().unwrap().is_empty());
}

struct TargetPausedProbe {
    base: FakeProbe,
    target: String,
    start: Option<Sender<()>>,
    resume: Option<Receiver<()>>,
}
impl Probe for TargetPausedProbe {
    fn check(&mut self, connection: &Connection) -> Result<Vec<crate::gateway::Profile>, String> {
        if matches!(connection, Connection::Saved { id, .. } if id == &self.target) {
            if let Some(start) = self.start.take() {
                start.send(()).map_err(|_| "fixture probe owner closed")?;
                self.resume
                    .take()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .map_err(|_| "fixture probe was not released")?;
            }
        }
        self.base.check(connection)
    }
}

#[test]
fn a_cancelled_or_dropped_route_ticket_cannot_write_registry_or_retire_the_outgoing_ssh_lease() {
    for cancel_by_drop in [false, true] {
        let file = RegistryFile::new();
        let secrets = FakeSecrets::default();
        let probe = FakeProbe::default();
        let tunnels = FakeTunnels::default();
        let mut core = json_core(file.path(), &secrets, &probe, &tunnels);
        core.execute(Command::Save(lease_ssh(
            "Outgoing",
            "outgoing.fixture.invalid",
        )))
        .unwrap();
        let old = core.registry.rows[0].clone();
        core.execute(Command::Save(lease_ssh("Target", "target.fixture.invalid")))
            .unwrap();
        let target = core.registry.rows[1].clone();
        let Outcome::Ready(_, _, Some(outgoing)) =
            core.execute(Command::Select(old.id.clone())).unwrap()
        else {
            panic!()
        };
        let target_lease = core
            .resolve_lease(&target.id, Some(&target.revision))
            .unwrap();
        let before = std::fs::read(file.path()).unwrap();
        let (started, entering) = mpsc::channel();
        let (resume, release) = mpsc::channel();
        core.probe = Box::new(TargetPausedProbe {
            base: probe.clone(),
            target: target.id.clone(),
            start: Some(started),
            resume: Some(release),
        });
        let (mut ui, resolver, done, worker) = ui_worker(core);
        ui.note_active_source(Some(outgoing.connection()));
        let mut ticket = Some(
            ui.commit_route(target_lease.clone(), "default".into(), 73)
                .unwrap(),
        );
        entering.recv_timeout(Duration::from_secs(3)).unwrap();
        if cancel_by_drop {
            drop(ticket.take());
        } else {
            ticket.as_ref().unwrap().cancel();
        }
        resume.send(()).unwrap();
        if let Some(ticket) = ticket {
            let receipt = ticket
                .receiver
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            assert_eq!(receipt.intent, 73);
            assert_eq!(receipt.profile, "default");
            assert!(receipt.result.err().unwrap().contains("superseded"));
        }
        // This observation is queued after the request, including when its receiver was dropped.
        ui.observe_registry();
        let mut automatic_ready = 0;
        wait_until(
            || {
                for event in ui.poll() {
                    if matches!(event, Event::Ready(_)) {
                        automatic_ready += 1;
                    }
                }
                !ui.observing
            },
            "route cancellation did not settle",
        );
        assert_eq!(std::fs::read(file.path()).unwrap(), before);
        assert_eq!(automatic_ready, 0);
        assert_eq!(ui.selected.as_deref(), Some(old.id.as_str()));
        assert!(outgoing.is_valid());
        assert!(target_lease.is_valid());
        assert!(!tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));

        let accepted = ui
            .commit_route(target_lease.clone(), "default".into(), 74)
            .unwrap();
        let receipt = accepted
            .receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_eq!(receipt.intent, 74);
        let connection = receipt.result.unwrap();
        let stored = JsonMetadata(file.path()).load().unwrap();
        assert_eq!(stored.last_used.as_deref(), Some(target.id.as_str()));
        assert_eq!(
            stored.last_profiles.get(&target.id).map(String::as_str),
            Some("default")
        );
        assert_eq!(
            stored.selected.as_deref(),
            Some(old.id.as_str()),
            "preparing a receipt promoted service foreground"
        );
        assert!(outgoing.is_valid());
        assert!(!tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
        assert_eq!(ui.selected.as_deref(), Some(old.id.as_str()));
        for event in ui.poll() {
            assert!(!matches!(event, Event::Ready(_)));
        }
        ui.note_active_source(Some(&connection));
        assert_eq!(ui.selected.as_deref(), Some(target.id.as_str()));
        // Only acceptance releases the old service reference; a peer's own reference survives it.
        ui.release_foreground(&old.id, &old.revision);
        ui.observe_registry();
        wait_until(
            || {
                ui.poll();
                !ui.observing
            },
            "foreground release did not settle",
        );
        assert!(outgoing.is_valid());
        assert!(!tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
        drop(outgoing);
        wait_until(
            || tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire),
            "released unreferenced outgoing tunnel did not close",
        );
        assert!(target_lease.is_valid());
        finish(ui, resolver, done, worker);
        assert!(!target_lease.is_valid());
    }
}

#[test]
fn saved_ready_carries_its_already_open_lease_and_keeps_an_outgoing_peer_alive_across_selection() {
    let (mut core, _, _, _, tunnels) = core();
    core.execute(Command::Save(lease_ssh("One", "one.fixture.invalid")))
        .unwrap();
    let one = core.registry.rows[0].clone();
    core.execute(Command::Save(lease_ssh("Two", "two.fixture.invalid")))
        .unwrap();
    let two = core.registry.rows[1].clone();
    let (mut ui, resolver, done, worker) = ui_worker(core);
    assert!(
        tunnels.calls.lock().unwrap().is_empty(),
        "loading startup metadata opened a source"
    );
    ui.action(Action::Select(one.id.clone()));
    wait_until(
        || {
            ui.poll();
            !ui.busy
        },
        "first saved activation did not finish",
    );
    let outgoing = ui
        .active_lease()
        .expect("Ready did not provide a saved source lease");
    assert_eq!(outgoing.source_id(), one.id);
    assert_eq!(tunnels.calls.lock().unwrap().len(), 1);
    ui.action(Action::Select(two.id.clone()));
    wait_until(
        || {
            ui.poll();
            !ui.busy
        },
        "second saved activation did not finish",
    );
    let current = ui.active_lease().unwrap();
    assert_eq!(current.source_id(), two.id);
    assert_eq!(ui.selected.as_deref(), Some(two.id.as_str()));
    assert_eq!(ui.last_used(), Some(two.id.as_str()));
    assert_eq!(ui.primary(), Some(one.id.as_str()));
    assert_eq!(tunnels.calls.lock().unwrap().len(), 2);
    assert!(outgoing.is_valid());
    assert!(!tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire));
    drop(outgoing);
    wait_until(
        || tunnels.closed.lock().unwrap()[0].load(Ordering::Acquire),
        "outgoing tunnel was not released with its last lease",
    );
    assert!(current.is_valid());
    finish(ui, resolver, done, worker);
    assert!(!current.is_valid());
}
