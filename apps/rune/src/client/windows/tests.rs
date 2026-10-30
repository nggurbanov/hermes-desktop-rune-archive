use super::*;
use serde_json::json;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-window-owners-{}", uuid::Uuid::new_v4())))
    }
    fn store(&self) -> WindowStore {
        WindowStore::at(self.0.clone())
    }
    fn document(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.0.join("windows.json")).unwrap()).unwrap()
    }
    fn write(&self, value: &Value) {
        crate::client::private_write(
            &self.0.join("windows.json"),
            &serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn route(profile: &str) -> WindowRoute {
    WindowRoute {
        profile: profile.into(),
        ..Default::default()
    }
}
#[test]
fn primary_claim_preserves_legacy_editor_while_peers_have_distinct_durable_inputs() {
    let root = Workspace::new();
    let legacy = crate::composer::Composer::primary_at(&root.0.join("primary-editor.json"))
        .unwrap()
        .editor_id()
        .clone();
    let store = root.store();
    let primary = store.claim_primary().unwrap();
    let id = primary.id().clone();
    assert_eq!(primary.editor_id(), &legacy);
    assert_eq!(primary.record().geometry.size, (1440, 900));
    assert!(matches!(store.claim_primary(), Err(ClaimError::Busy(busy)) if busy == id));
    let peer = store
        .mint_peer(BootstrapKind::PeerInherited, route("research"))
        .unwrap();
    assert_ne!(primary.id(), peer.id());
    assert_ne!(primary.editor_id(), peer.editor_id());
    assert!(matches!(store.restore(peer.id()), Err(ClaimError::Busy(_))));
    let peer_id = peer.id().clone();
    let editor = peer.editor_id().clone();
    drop(peer);
    let restored = store.restore(&peer_id).unwrap();
    assert_eq!(restored.editor_id(), &editor);
    assert_eq!(restored.record().route.profile, "research");
    drop(primary);
    assert_eq!(store.claim_primary().unwrap().id(), &id);
    // Once the durable window registry exists, it is the migration authority.
    let invalid_legacy = b"retained malformed legacy identity";
    std::fs::write(root.0.join("primary-editor.json"), invalid_legacy).unwrap();
    assert_eq!(store.legacy_editor_id().unwrap(), legacy);
    assert_eq!(
        std::fs::read(root.0.join("primary-editor.json")).unwrap(),
        invalid_legacy
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.0.join("windows.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(root.0.join("window-claims"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}
#[test]
fn descriptors_strip_credentials_and_never_restore_a_protected_url_as_anonymous() {
    let mut route = route("research");
    let connection = Connection::Url {
        endpoint: "wss://fixture.invalid/prefix/api/ws?token=embedded-private".into(),
        token: Some("explicit-private".into()),
    };
    let source = SourceDescriptor::from_connection(&connection).unwrap();
    assert_eq!(
        source,
        SourceDescriptor::Remote {
            endpoint: "https://fixture.invalid/prefix/".into(),
            credential_required: true
        }
    );
    assert!(source.connection_without_secret().is_none());
    route.source = Some(source);
    let root = Workspace::new();
    let store = root.store();
    let peer = store
        .mint_peer(BootstrapKind::PeerInherited, route)
        .unwrap();
    let saved = Connection::Saved {
        id: uuid::Uuid::new_v4().to_string(),
        revision: uuid::Uuid::new_v4().to_string(),
        inner: Box::new(Connection::Url {
            endpoint: "http://127.0.0.1:41379".into(),
            token: Some("forward-private".into()),
        }),
    };
    let source = SourceDescriptor::from_connection(&saved).unwrap();
    assert!(matches!(source, SourceDescriptor::Saved { .. }));
    let mut next = peer.record().clone();
    next.route.source = Some(source);
    let mut peer = peer;
    peer.save(next).unwrap();
    let raw = std::fs::read_to_string(root.0.join("windows.json")).unwrap();
    for forbidden in [
        "embedded-private",
        "explicit-private",
        "forward-private",
        "41379",
    ] {
        assert!(!raw.contains(forbidden));
    }
    assert!(
        SourceDescriptor::from_connection(&Connection::Local {
            home: "relative-agent".into()
        })
        .is_err()
    );
    let public = SourceDescriptor::from_connection(&Connection::Url {
        endpoint: "http://fixture.invalid".into(),
        token: None,
    })
    .unwrap();
    assert!(matches!(
        public.connection_without_secret(),
        Some(Connection::Url { token: None, .. })
    ));
}
#[test]
fn different_window_updates_preserve_siblings_extensions_and_exact_mount_revision() {
    let root = Workspace::new();
    let store = root.store();
    let mut first = store.claim_primary().unwrap();
    let mut second = store
        .mint_peer(BootstrapKind::ProfilePinned, route("second"))
        .unwrap();
    let mut document = root.document();
    document["future_extension"] = json!({"keep":true});
    document["records"][first.id().as_str()]["future_window"] = json!("retained");
    root.write(&document);
    let stale = first.record().clone();
    let mut next = stale.clone();
    next.geometry.position = Some((32, 64));
    next.geometry.size = (1100, 700);
    next.presentation = Some(crate::client::Preferences {
        theme: Some(true),
        ..Default::default()
    });
    first.save(next).unwrap();
    let mut external = root.document();
    external["records"][first.id().as_str()]["future_window"] = json!("newer retained");
    root.write(&external);
    let mut next = first.record().clone();
    next.geometry.maximized = true;
    first.save(next).unwrap();
    let mut next = second.record().clone();
    next.route.profile = "changed-second".into();
    second.save(next).unwrap();
    assert!(first.save(stale).is_err());
    let current = root.document();
    assert_eq!(current["future_extension"]["keep"], true);
    let a = &current["records"][first.id().as_str()];
    let b = &current["records"][second.id().as_str()];
    assert_eq!(a["future_window"], "newer retained");
    assert_eq!(a["geometry"]["size"], json!([1100, 700]));
    assert_eq!(a["presentation"]["theme"], true);
    assert_eq!(b["route"]["profile"], "changed-second");
    let id = second.id().clone();
    second.close().unwrap();
    assert!(
        store
            .records()
            .unwrap()
            .iter()
            .find(|r| r.id == id)
            .unwrap()
            .closed
    );
    let restored = store.restore(&id).unwrap();
    assert!(!restored.record().closed);
}
#[test]
fn corrupt_or_newer_windows_and_legacy_id_are_retained_without_random_fallback() {
    for bytes in [
        b"invalid".as_slice(),
        b"[]".as_slice(),
        br#"{"version":2,"revision":0,"primary":null,"records":{}}"#.as_slice(),
    ] {
        let root = Workspace::new();
        crate::client::private_write(&root.0.join("windows.json"), bytes).unwrap();
        assert!(matches!(
            root.store().claim_primary(),
            Err(ClaimError::Store(_))
        ));
        assert!(
            root.store()
                .mint_peer(BootstrapKind::PeerInherited, route("default"))
                .is_err()
        );
        assert_eq!(std::fs::read(root.0.join("windows.json")).unwrap(), bytes);
        assert!(!root.0.join("primary-editor.json").exists());
    }
    let root = Workspace::new();
    let legacy = br#"{"version":2,"identity":"future"}"#;
    crate::client::private_write(&root.0.join("primary-editor.json"), legacy).unwrap();
    assert!(root.store().claim_primary().is_err());
    assert_eq!(
        std::fs::read(root.0.join("primary-editor.json")).unwrap(),
        legacy
    );
    assert!(!root.0.join("windows.json").exists());
    assert!(
        WindowStore::at(PathBuf::from("relative-state"))
            .claim_primary()
            .is_err()
    );
}
#[test]
fn failed_window_save_keeps_pending_changes_for_retry_and_merges_new_peer_records() {
    let root = Workspace::new();
    let store = root.store();
    let mut first = store.claim_primary().unwrap();
    let good = std::fs::read(root.0.join("windows.json")).unwrap();
    std::fs::remove_file(root.0.join("windows.json")).unwrap();
    std::fs::create_dir(root.0.join("windows.json")).unwrap();
    let mut next = first.record().clone();
    next.geometry.size = (1210, 710);
    assert!(first.save(next).is_err());
    assert_eq!(first.record().geometry.size, (1440, 900));
    assert_eq!(first.pending().unwrap().geometry.size, (1210, 710));
    assert!(first.wake_after().is_some());
    assert!(!first.poll_storage().unwrap());
    std::fs::remove_dir(root.0.join("windows.json")).unwrap();
    std::fs::write(root.0.join("windows.json"), good).unwrap();
    let other = store
        .mint_peer(BootstrapKind::PeerInherited, route("new-peer"))
        .unwrap();
    first.retry_at = Some(Instant::now());
    assert!(first.poll_storage().unwrap());
    assert!(first.pending().is_none());
    assert!(first.wake_after().is_none());
    assert_eq!(first.record().geometry.size, (1210, 710));
    assert!(store.records().unwrap().iter().any(|r| &r.id == other.id()));
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|s| s.is_none()) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
#[test]
fn actual_other_process_cannot_mount_the_primary_and_crash_releases_its_claim() {
    let root = Workspace::new();
    let mut child = Process(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "client::windows::tests::window_owner_process",
                "--ignored",
                "--nocapture",
            ])
            .env("HERMES_RUNE_WINDOW_OWNER_ROOT", &root.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let ready = root.0.join("owner.ready");
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "window owner child failed to become ready"
        );
        assert!(child.0.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    let state: Value = serde_json::from_slice(&std::fs::read(ready).unwrap()).unwrap();
    let id = WindowId::parse(state["id"].as_str().unwrap()).unwrap();
    let editor = EditorId::parse(state["editor"].as_str().unwrap()).unwrap();
    let store = root.store();
    assert!(matches!(store.claim_primary(), Err(ClaimError::Busy(busy)) if busy == id));
    let peer = store
        .mint_peer(BootstrapKind::PeerInherited, route("parallel"))
        .unwrap();
    assert_ne!(peer.editor_id(), &editor);
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    let primary = store.claim_primary().unwrap();
    assert_eq!(primary.id(), &id);
    assert_eq!(primary.editor_id(), &editor);
    assert!(store.records().unwrap().iter().any(|r| &r.id == peer.id()));
}
#[test]
#[ignore = "private entry point exercised by actual_other_process contract"]
fn window_owner_process() {
    let root = PathBuf::from(std::env::var_os("HERMES_RUNE_WINDOW_OWNER_ROOT").unwrap());
    let window = WindowStore::at(root.clone()).claim_primary().unwrap();
    crate::client::private_write(
        &root.join("owner.ready"),
        &serde_json::to_vec(&json!({"id":window.id(),"editor":window.editor_id()})).unwrap(),
    )
    .unwrap();
    let mut byte = [0u8];
    let _ = std::io::stdin().read(&mut byte);
}

#[test]
fn pinned_defaults_survive_navigation_and_session_bootstrap_allows_a_later_fresh_chat() {
    let root = Workspace::new();
    let store = root.store();
    let mut pinned = store
        .mint_peer(BootstrapKind::ProfilePinned, route("pinned-profile"))
        .unwrap();
    let mut next = pinned.record().clone();
    next.route = route("navigated-profile");
    pinned.save(next).unwrap();
    assert_eq!(
        pinned.record().pinned_route.as_ref().unwrap().profile,
        "pinned-profile"
    );
    assert_eq!(pinned.record().route.profile, "navigated-profile");
    let mut next = pinned.record().clone();
    next.pinned_route = Some(route("cannot-reinterpret-the-boot-default"));
    assert!(pinned.save(next).is_err());
    assert!(
        store
            .mint_peer(BootstrapKind::InteractiveSession, route("no-session"))
            .is_err()
    );
    let mut seed = route("session-profile");
    seed.session = Some("stored-conversation".into());
    let mut session = store
        .mint_peer(BootstrapKind::InteractiveSession, seed)
        .unwrap();
    let mut next = session.record().clone();
    next.route.session = None;
    session.save(next).unwrap();
    let id = session.id().clone();
    drop(session);
    let restored = store.restore(&id).unwrap();
    assert!(restored.record().route.session.is_none());
    assert_eq!(restored.record().kind, BootstrapKind::InteractiveSession);
}

#[test]
#[cfg(unix)]
fn window_claim_release_ignores_duplicated_descriptors() {
    let root = Workspace::new();
    let store = root.store();
    let primary = store.claim_primary().unwrap();
    let id = primary.id().clone();
    let inherited = primary._claim.try_clone().unwrap();
    drop(primary);
    let next = store.claim_primary().unwrap();
    assert_eq!(next.id(), &id);
    drop(inherited);
    assert!(matches!(store.claim_primary(), Err(ClaimError::Busy(_))));
    drop(next);
    assert_eq!(store.claim_primary().unwrap().id(), &id);
}
