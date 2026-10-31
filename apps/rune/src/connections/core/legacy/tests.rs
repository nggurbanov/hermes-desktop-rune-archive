use super::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
#[derive(Clone, Default)]
struct SecretFixture {
    values: Arc<Mutex<BTreeMap<String, String>>>,
    calls: Arc<Mutex<Vec<&'static str>>>,
    deny_delete: Arc<AtomicBool>,
    deny_set: Arc<AtomicBool>,
}
impl Secrets for SecretFixture {
    fn get(&mut self, id: &str) -> Result<String, String> {
        self.calls.lock().unwrap().push("get");
        self.values
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("fixture credential missing".into())
    }
    fn get_optional(&mut self, id: &str) -> Result<Option<String>, String> {
        self.calls.lock().unwrap().push("get");
        Ok(self.values.lock().unwrap().get(id).cloned())
    }
    fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push("set");
        if self.deny_set.load(Ordering::Acquire) {
            return Err("fixture keychain set refused".into());
        }
        self.values.lock().unwrap().insert(id.into(), value.into());
        Ok(())
    }
    fn delete(&mut self, id: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push("delete");
        if self.deny_delete.load(Ordering::Acquire) {
            return Err("fixture keychain locked".into());
        }
        self.values.lock().unwrap().remove(id);
        Ok(())
    }
}
struct Private {
    root: PathBuf,
    path: PathBuf,
    secrets: SecretFixture,
}
impl Private {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("hermes-legacy-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("connections.json");
        fs::write(&path, registry::encode(&Registry::empty()).unwrap()).unwrap();
        Self {
            root,
            path,
            secrets: SecretFixture::default(),
        }
    }
    fn core(&self) -> Core {
        let mut core = Core::system(self.path.clone());
        core.secrets = Box::new(self.secrets.clone());
        core
    }
    fn load(&self) -> Registry {
        registry::decode(&fs::read(&self.path).unwrap()).unwrap()
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn absent() -> domain::ScopeVersion {
    domain::ScopeVersion::Absent { revision: None }
}
fn edit(scope: Scope, expected: domain::ScopeVersion, url: &str, token: SecretEdit) -> Edit {
    let value = match scope {
        Scope::Global => ValueEdit::Global(Global {
            mode: domain::Mode::Remote,
            remote: Some(Remote::new(url.into())),
            ..Global::default()
        }),
        Scope::Profile(_) => ValueEdit::Profile(Profile::Remote(Remote::new(url.into()))),
    };
    Edit {
        scope,
        expected,
        value,
        auth: AuthEdit {
            token,
            headers: BTreeMap::new(),
        },
    }
}
#[test]
fn real_locked_writers_merge_global_and_profile_without_rewriting_saved_native_v1() {
    let fixture = Private::new();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let mut workers = vec![];
    for scope in [Scope::Global, Scope::Profile("research".into())] {
        let mut core = fixture.core();
        let barrier = barrier.clone();
        workers.push(thread::spawn(move || {
            barrier.wait();
            core.save_legacy(edit(
                scope,
                absent(),
                "https://fixture.invalid/",
                SecretEdit::Clear,
            ))
            .unwrap();
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    let loaded = fixture.load();
    assert_eq!(loaded.version, 2);
    let legacy = loaded.legacy.unwrap();
    assert!(legacy.global.is_some());
    assert!(legacy.profiles.contains_key("research"));
    assert!(fixture.secrets.calls.lock().unwrap().is_empty());
}
#[test]
fn token_headers_are_keyring_only_and_stale_form_fails_before_any_credential_side_effect() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let mut input = edit(
        Scope::Global,
        absent(),
        "https://fixture.invalid/base",
        SecretEdit::Set("private-token-fixture".into()),
    );
    input.auth.headers.insert(
        "CF-Access-Client-Secret".into(),
        SecretEdit::Set("private-header-fixture\r\n".into()),
    );
    let saved = core.save_legacy(input).unwrap();
    let bytes = fs::read(&fixture.path).unwrap();
    let body = String::from_utf8(bytes.clone()).unwrap();
    assert!(!body.contains("private-token-fixture"));
    assert!(!body.contains("private-header-fixture"));
    assert!(body.contains("cf-access-client-secret"));
    let stored = fixture.secrets.values.lock().unwrap();
    assert_eq!(stored.len(), 1);
    let bundle = AuthBundle::decode(stored.values().next().unwrap()).unwrap();
    assert_eq!(bundle.token.as_deref(), Some("private-token-fixture"));
    assert_eq!(
        bundle.headers["cf-access-client-secret"],
        "private-header-fixture"
    );
    drop(stored);
    let before = fixture.secrets.calls.lock().unwrap().len();
    assert!(
        core.save_legacy(edit(
            Scope::Global,
            absent(),
            "https://wrong.invalid/",
            SecretEdit::Set("wrong".into())
        ))
        .is_err()
    );
    assert_eq!(fixture.secrets.calls.lock().unwrap().len(), before);
    assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    let mut unchanged = saved.snapshot.global;
    unchanged.remote.as_mut().unwrap().name = Some("Display edit".into());
    let old_route = match &saved.version {
        domain::ScopeVersion::Present(s) => s.route_revision.clone(),
        _ => unreachable!(),
    };
    let refreshed = core
        .save_legacy(Edit {
            scope: Scope::Global,
            expected: saved.version,
            value: ValueEdit::Global(unchanged),
            auth: AuthEdit::default(),
        })
        .unwrap();
    assert_eq!(fixture.secrets.calls.lock().unwrap().len(), before);
    match refreshed.version {
        domain::ScopeVersion::Present(s) => assert_eq!(s.route_revision, old_route),
        _ => unreachable!(),
    }
}
struct FailingMetadata {
    inner: JsonMetadata,
    writes: usize,
    fail_at: usize,
}
impl Metadata for FailingMetadata {
    fn transaction(&mut self) -> Result<Option<crate::client::FileLock>, String> {
        self.inner.transaction()
    }
    fn load(&mut self) -> Result<Registry, String> {
        self.inner.load()
    }
    fn save(&mut self, value: &Registry) -> Result<(), String> {
        self.writes += 1;
        if self.writes == self.fail_at {
            return Err("fixture metadata commit refused".into());
        }
        self.inner.save(value)
    }
}
#[test]
fn failed_commit_keeps_previous_route_and_durable_cleanup_survives_keychain_repair_and_restart() {
    let fixture = Private::new();
    fixture.secrets.deny_delete.store(true, Ordering::Release);
    let mut core = fixture.core();
    core.metadata = Box::new(FailingMetadata {
        inner: JsonMetadata(fixture.path.clone()),
        writes: 0,
        fail_at: 2,
    });
    assert!(
        core.save_legacy(edit(
            Scope::Global,
            absent(),
            "https://candidate.invalid/",
            SecretEdit::Set("provisional-private-token".into())
        ))
        .unwrap_err()
        .contains("cleanup ledger")
    );
    let old = fixture.load();
    assert_eq!(old.version, 1);
    assert!(old.legacy.is_none());
    assert_eq!(old.cleanup.len(), 1);
    assert_eq!(fixture.secrets.values.lock().unwrap().len(), 1);
    fixture.secrets.deny_delete.store(false, Ordering::Release);
    drop(core);
    let mut repaired = fixture.core();
    repaired.execute(Command::Load).unwrap();
    assert!(fixture.load().cleanup.is_empty());
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
    assert!(fixture.load().legacy.is_none());
}
#[test]
fn native_future_schema_preserves_bytes_and_blocks_saved_and_legacy_writes_and_cleanup() {
    let fixture = Private::new();
    let mut core = fixture.core();
    core.save_legacy(edit(
        Scope::Global,
        absent(),
        "https://fixture.invalid/",
        SecretEdit::Clear,
    ))
    .unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    value["legacy"]["version"] = serde_json::json!(99);
    value["cleanup"] = serde_json::json!(["do-not-delete-fixture"]);
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(&fixture.path, &bytes).unwrap();
    let calls = fixture.secrets.calls.lock().unwrap().len();
    assert!(core.legacy_snapshot().is_err());
    assert!(core.execute(Command::Load).is_err());
    assert!(
        core.save_legacy(edit(
            Scope::Global,
            absent(),
            "https://wrong.invalid/",
            SecretEdit::Set("wrong".into())
        ))
        .is_err()
    );
    assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    assert_eq!(fixture.secrets.calls.lock().unwrap().len(), calls);
}
#[test]
fn cleanup_does_not_delete_reachable_dormant_legacy_bundles_or_saved_tokens() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let saved = core
        .save_legacy(edit(
            Scope::Global,
            absent(),
            "https://fixture.invalid/",
            SecretEdit::Set("still-reachable".into()),
        ))
        .unwrap();
    let reference = fixture
        .secrets
        .values
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let mut global = saved.snapshot.global;
    global.mode = domain::Mode::Local;
    core.save_legacy(Edit {
        scope: Scope::Global,
        expected: saved.version,
        value: ValueEdit::Global(global),
        auth: AuthEdit::default(),
    })
    .unwrap();
    let mut registry = fixture.load();
    registry
        .cleanup
        .extend([reference.clone(), "saved-fixture-token".into()]);
    registry.rows.push(Saved {
        id: uuid::Uuid::new_v4().to_string(),
        revision: uuid::Uuid::new_v4().to_string(),
        label: "Fixture".into(),
        destination: Destination::Remote {
            url: "https://saved.invalid/".into(),
        },
        credential: Some("saved-fixture-token".into()),
        other: Default::default(),
        destination_other: Default::default(),
    });
    fixture
        .secrets
        .values
        .lock()
        .unwrap()
        .insert("saved-fixture-token".into(), "still-saved".into());
    fs::write(&fixture.path, registry::encode(&registry).unwrap()).unwrap();
    let deletes = fixture
        .secrets
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| **c == "delete")
        .count();
    core.execute(Command::Load).unwrap();
    assert!(fixture.load().cleanup.is_empty());
    assert!(
        fixture
            .secrets
            .values
            .lock()
            .unwrap()
            .contains_key(&reference)
    );
    assert_eq!(
        fixture
            .secrets
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| **c == "delete")
            .count(),
        deletes
    );
}
#[test]
fn authored_edits_preserve_native_unknown_fields_through_real_file_transactions() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let saved = core
        .save_legacy(edit(
            Scope::Global,
            absent(),
            "https://fixture.invalid/",
            SecretEdit::Clear,
        ))
        .unwrap();
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    raw["legacy"]["global"]["slot-future"] = serde_json::json!({"v":1});
    raw["legacy"]["global"]["value"]["global-future"] = serde_json::json!(2);
    raw["legacy"]["global"]["value"]["remote"]["transport-future"] = serde_json::json!([3]);
    raw["root-future"] = serde_json::json!(true);
    fs::write(&fixture.path, serde_json::to_vec(&raw).unwrap()).unwrap();
    core.save_legacy(edit(
        Scope::Global,
        saved.version,
        "https://changed.invalid/",
        SecretEdit::Clear,
    ))
    .unwrap();
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    assert_eq!(after["root-future"], true);
    assert_eq!(
        after["legacy"]["global"]["slot-future"],
        serde_json::json!({"v":1})
    );
    assert_eq!(after["legacy"]["global"]["value"]["global-future"], 2);
    assert_eq!(
        after["legacy"]["global"]["value"]["remote"]["transport-future"],
        serde_json::json!([3])
    );
}

#[test]
fn save_ticket_keeps_the_original_worker_until_final_receipt_without_a_ui_sink() {
    let fixture = Private::new();
    let core = fixture.core();
    let (send, requests) = mpsc::channel();
    let (replies, receive) = mpsc::channel();
    drop(receive);
    let (finished, done) = mpsc::channel();
    let worker = thread::spawn(move || {
        crate::connections::worker::run(Ok(core), requests, replies, Arc::new(|| {}));
        finished.send(()).unwrap();
    });
    let resolver = crate::connections::Resolver::new(send);
    let ticket = resolver
        .save_legacy(edit(
            Scope::Global,
            absent(),
            "https://fixture.invalid/",
            SecretEdit::Set("accepted-token".into()),
        ))
        .unwrap();
    drop(resolver);
    let saved = ticket
        .receiver
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap();
    assert!(saved.snapshot.global.remote.as_ref().unwrap().has_token());
    assert!(done.try_recv().is_err());
    assert_eq!(fixture.load().version, 2);
    drop(ticket);
    done.recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    worker.join().unwrap();
}
#[test]
fn an_accepted_queued_save_survives_dropped_receipt_and_ui_handles() {
    let fixture = Private::new();
    let core = fixture.core();
    let (send, requests) = mpsc::channel();
    let (replies, receive) = mpsc::channel();
    drop(receive);
    let resolver = crate::connections::Resolver::new(send);
    let ticket = resolver
        .save_legacy(edit(
            Scope::Global,
            absent(),
            "https://fixture.invalid/",
            SecretEdit::Set("accepted-token".into()),
        ))
        .unwrap();
    drop(ticket);
    drop(resolver);
    let worker = thread::spawn(move || {
        crate::connections::worker::run(Ok(core), requests, replies, Arc::new(|| {}))
    });
    worker.join().unwrap();
    let loaded = fixture.load();
    assert!(
        loaded
            .legacy
            .unwrap()
            .snapshot()
            .global
            .remote
            .unwrap()
            .has_token()
    );
    assert_eq!(fixture.secrets.values.lock().unwrap().len(), 1);
}

#[test]
fn failed_secret_creation_leaves_old_route_and_retries_the_exact_scope_after_repair() {
    let fixture = Private::new();
    fixture.secrets.deny_set.store(true, Ordering::Release);
    let mut core = fixture.core();
    assert!(
        core.save_legacy(edit(
            Scope::Global,
            absent(),
            "https://candidate.invalid/",
            SecretEdit::Set("retry-exact-token".into())
        ))
        .is_err()
    );
    let before = fixture.load();
    assert!(before.legacy.is_none());
    assert!(before.cleanup.is_empty());
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
    fixture.secrets.deny_set.store(false, Ordering::Release);
    let saved = core
        .save_legacy(edit(
            Scope::Global,
            absent(),
            "https://candidate.invalid/",
            SecretEdit::Set("retry-exact-token".into()),
        ))
        .unwrap();
    assert!(saved.snapshot.global.remote.unwrap().has_token());
    assert_eq!(fixture.secrets.values.lock().unwrap().len(), 1);
}

#[test]
fn changing_url_cannot_reuse_a_bundle_header_even_if_display_metadata_omitted_its_name() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let mut input = edit(
        Scope::Global,
        absent(),
        "https://old.invalid/",
        SecretEdit::Set("old-token".into()),
    );
    input.auth.headers.insert(
        "cf-access-client-secret".into(),
        SecretEdit::Set("old-private-header".into()),
    );
    let saved = core.save_legacy(input).unwrap();
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    raw["legacy"]["global"]["value"]["remote"]["header_names"] = serde_json::json!([]);
    let bytes = serde_json::to_vec(&raw).unwrap();
    fs::write(&fixture.path, &bytes).unwrap();
    let result = core.save_legacy(edit(
        Scope::Global,
        saved.version,
        "https://new.invalid/",
        SecretEdit::Set("new-token".into()),
    ));
    let saved = result.unwrap();
    assert!(
        saved
            .snapshot
            .global
            .remote
            .unwrap()
            .header_names
            .is_empty()
    );
    let values = fixture.secrets.values.lock().unwrap();
    assert_eq!(values.len(), 1);
    let bundle = AuthBundle::decode(values.values().next().unwrap()).unwrap();
    assert_eq!(bundle.token.as_deref(), Some("new-token"));
    assert!(
        bundle.headers.is_empty(),
        "complete replacement must never send an undisplayed old header to a new URL"
    );
}

#[test]
fn complete_auth_replacement_repairs_missing_bundle_without_reading_the_old_secret() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let saved = core
        .save_legacy(edit(
            Scope::Global,
            absent(),
            "https://fixture.invalid/",
            SecretEdit::Set("old-token".into()),
        ))
        .unwrap();
    fixture.secrets.values.lock().unwrap().clear();
    let reads = fixture
        .secrets
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| **c == "get")
        .count();
    let repaired = core
        .save_legacy(edit(
            Scope::Global,
            saved.version,
            "https://fixture.invalid/",
            SecretEdit::Set("repaired-token".into()),
        ))
        .expect("complete replacement must not need the unavailable old bundle");
    assert!(repaired.snapshot.global.remote.unwrap().has_token());
    assert_eq!(
        fixture
            .secrets
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| **c == "get")
            .count(),
        reads
    );
    let values = fixture.secrets.values.lock().unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(
        AuthBundle::decode(values.values().next().unwrap())
            .unwrap()
            .token
            .as_deref(),
        Some("repaired-token")
    );
}

#[test]
fn partial_auth_edit_keeps_old_fields_and_refuses_missing_bundle_instead_of_dropping_them() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let saved = core
        .save_legacy(edit(
            Scope::Global,
            absent(),
            "https://fixture.invalid/",
            SecretEdit::Set("keep-this-token".into()),
        ))
        .unwrap();
    let before = fs::read(&fixture.path).unwrap();
    fixture.secrets.values.lock().unwrap().clear();
    let mut input = edit(
        Scope::Global,
        saved.version,
        "https://fixture.invalid/",
        SecretEdit::Keep,
    );
    input.auth.headers.insert(
        "x-access-key".into(),
        SecretEdit::Set("new-header-only".into()),
    );
    assert!(core.save_legacy(input).is_err());
    assert_eq!(fs::read(&fixture.path).unwrap(), before);
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
}

#[path = "tests/resolve.rs"]
mod resolution;

#[path = "tests/cloud_store.rs"]
mod cloud_store;

#[cfg(target_os = "linux")]
#[path = "tests/worker_exit.rs"]
mod worker_exit;
