use super::*;
use crate::{
    gateway::Connection,
    source::{SessionOwner, SourceStamp},
};
struct Workspace(std::path::PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-sidebar-source-{}", uuid::Uuid::new_v4())))
    }
    fn state(&self, connection: &Connection, epoch: u64) -> SidebarState {
        let mut state = SidebarState::new();
        state.bind_connection_at(connection, epoch, self.0.clone());
        state.owner_profile = "research".into();
        state
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn connection(name: &str) -> Connection {
    Connection::Url {
        endpoint: format!("https://{name}.sidebar-fixture.invalid"),
        token: Some("synthetic".into()),
    }
}
fn saved(revision: &str, port: u16) -> Connection {
    Connection::Saved {
        id: "11111111-1111-4111-8111-111111111111".into(),
        revision: revision.into(),
        inner: Box::new(Connection::Url {
            endpoint: format!("http://127.0.0.1:{port}"),
            token: Some("synthetic".into()),
        }),
    }
}
fn rows() -> Value {
    json!({"sessions":[{"id":"tip","profile":"research","_lineage_root_id":"root","_lineage_ids":["old-tip"],"message_count":2,"unread":true}]})
}
#[test]
fn colliding_rows_runtime_aliases_menus_and_canonical_ids_stay_with_their_fetch_source() {
    let root = Workspace::new();
    let mut a = root.state(&connection("a"), 11);
    let mut b = root.state(&connection("b"), 12);
    a.apply("sidebar.recents", Ok(rows()));
    b.apply("sidebar.recents", Ok(rows()));
    let ar = a.recents.rows[0].clone();
    let br = b.recents.rows[0].clone();
    assert_ne!(ar.key(), br.key());
    assert!(a.find_owned(&br.owner().unwrap(), "old-tip").is_none());
    assert!(b.find_owned(&ar.owner().unwrap(), "root").is_none());
    a.apply(
        "sidebar.live.research",
        Ok(json!({"sessions":[{"id":"old-tip","status":"waiting"}]})),
    );
    assert!(a.status(&ar) == Status::NeedsInput);
    assert!(b.status(&br) == Status::Unread);
    assert!(a.status(&br) == Status::Unread);
    a.apply(
        "sidebar.live.research",
        Ok(json!({"sessions":[{"id":"old-tip","profile":"foreign","status":"working"}]})),
    );
    assert!(a.status(&ar) == Status::NeedsInput);
    assert!(a.errors.contains_key("sidebar.live.research"));
    a.set_canonical_profiles(&[crate::gateway::Profile {
        name: "research".into(),
        label: "Research".into(),
        description: String::new(),
        canonical_session: Some(crate::gateway::CanonicalSession {
            id: "root".into(),
            resolved_id: "tip".into(),
            preview: String::new(),
        }),
    }]);
    assert!(a.is_canonical(&ar));
    assert!(!a.is_canonical(&br));
    assert!(!b.is_canonical(&br));
    assert_ne!(
        a.group_key("recents", "Profile", "research"),
        b.group_key("recents", "Profile", "research")
    );
    assert_ne!(
        a.group_key("recents", "Profile", "research"),
        a.group_key("messaging:telegram", "Profile", "research")
    );
    let before = ar.key();
    a.apply("sidebar.recents",Ok(json!({"sessions":[{"id":"forged","profile":"research","connection_id":"another-source"}]})));
    assert_eq!(a.recents.rows[0].key(), before);
    assert!(a.recents.error.is_some());
}
#[test]
fn compression_survivors_forks_resets_and_aliases_cannot_cross_source_namespaces() {
    let root = Workspace::new();
    let mut a = root.state(&connection("a"), 21);
    let mut b = root.state(&connection("b"), 22);
    a.apply("sidebar.recents",Ok(json!({"sessions":[{"id":"root","profile":"research","message_count":1,"last_active":1},{"id":"tip","profile":"research","_lineage_root_id":"root","_lineage_ids":["root"],"parent_session_id":"root","last_active":2,"message_count":2},{"id":"fork","profile":"research","_branched_from":"root","last_active":3,"message_count":1},{"id":"reset","profile":"research","parent_session_id":"tip","_reset_from":"tip","last_active":4,"message_count":1}]})));
    b.apply("sidebar.recents",Ok(json!({"sessions":[{"id":"root","profile":"research","last_active":5,"message_count":1},{"id":"orphan","profile":"research","_branched_from":"tip","last_active":6,"message_count":1}]})));
    let all: Vec<_> = a.all_rows().into_iter().chain(b.all_rows()).collect();
    let projected = lineage_rows(all);
    assert_eq!(projected.len(), 5);
    assert!(
        !projected
            .iter()
            .any(|row| row.id == "root" && row.provenance == a.source)
    );
    assert!(
        projected
            .iter()
            .any(|row| row.id == "tip" && row.provenance == a.source)
    );
    assert!(
        projected
            .iter()
            .any(|row| row.id == "root" && row.provenance == b.source)
    );
    assert_eq!(a.branch_depth(&a.find("research", "fork").unwrap()), 1);
    assert_eq!(a.branch_depth(&a.find("research", "reset").unwrap()), 0);
    assert_eq!(b.branch_depth(&b.find("research", "orphan").unwrap()), 0);
}
#[test]
fn source_claims_keep_uncertain_edits_exact_across_other_sources_and_another_window() {
    let root = Workspace::new();
    let mut a = root.state(&connection("a"), 31);
    a.apply("sidebar.recents", Ok(rows()));
    assert_eq!(
        a.action(Action::Pin("research".into(), "tip".into(), true))
            .len(),
        1
    );
    let send =
        a.accept_snapshot(json!({"exists":true,"runtime_revision":7,"runtime_generation":4}));
    assert_eq!(send.len(), 1);
    let payload = send[0].1.body.clone();
    a.apply(
        "sidebar.mutation",
        Err("Network timeout; outcome unknown".into()),
    );
    let mut peer = root.state(&connection("a"), 32);
    assert!(peer.mutation.is_some());
    assert!(peer.action(Action::RetryMutation).is_empty());
    assert!(peer.notice.as_ref().unwrap().contains("Another desktop"));
    let mut b = root.state(&connection("b"), 33);
    b.apply("sidebar.recents", Ok(rows()));
    assert_eq!(
        b.action(Action::Unread("research".into(), "tip".into(), false))
            .len(),
        1
    );
    assert_eq!(
        b.accept_snapshot(json!({"exists":true,"runtime_revision":2,"runtime_generation":1}))
            .len(),
        1
    );
    drop(a);
    // A dispatched request retains the claim after its UI owner has gone away.
    assert!(send[0].1.edit_claim.is_some());
    assert!(peer.action(Action::RetryMutation).is_empty());
    drop(send);
    let retry = peer.action(Action::RetryMutation);
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].1.body, payload);
    assert_eq!(peer.mutation.as_ref().unwrap().source, peer.source);
}
#[test]
fn explicit_retry_reconciles_the_same_saved_version_after_forward_recreation_only() {
    let root = Workspace::new();
    let mut old = root.state(&saved("original", 19001), 41);
    old.apply("sidebar.recents", Ok(rows()));
    assert_eq!(
        old.action(Action::Pin("research".into(), "tip".into(), true))
            .len(),
        1
    );
    let send =
        old.accept_snapshot(json!({"exists":true,"runtime_revision":5,"runtime_generation":8}));
    let payload = send[0].1.body.clone();
    drop(old);
    drop(send);
    let mut reopened = root.state(&saved("original", 19002), 42);
    assert!(reopened.mutation.is_some());
    assert_ne!(reopened.mutation.as_ref().unwrap().source, reopened.source);
    let retry = reopened.action(Action::RetryMutation);
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].1.body, payload);
    assert_eq!(reopened.mutation.as_ref().unwrap().source, reopened.source);
    let rotated = root.state(&saved("new-credentials", 19002), 43);
    assert!(rotated.mutation.is_none());
}
#[test]
fn partition_preference_mirrors_do_not_write_the_global_preference_file() {
    let root = Workspace::new();
    let mut state = root.state(&connection("a"), 51);
    state.set_partition_preferences(&Preferences::default());
    state.action(Action::Grouping(Grouping::Profile));
    state.action(Action::Collapse("owned-group".into()));
    state.flush_storage().unwrap();
    assert!(!root.0.join("sidebar.json").exists());
    assert!(state.prefs.grouping == Grouping::Profile);
    assert!(state.prefs.collapsed.contains("owned-group"));
    let owner = SessionOwner::new(
        SourceStamp::from_connection(&connection("a")).unwrap(),
        "research",
    );
    assert_eq!(state.owner("research"), Some(owner));
}

#[test]
fn retired_source_clears_active_history_but_keeps_exact_recovery_and_worker_claim() {
    let root = Workspace::new();
    let source_connection = connection("retired");
    let binding = crate::source::Binding::new(source_connection.clone(), None).unwrap();
    let mut source = crate::sidebar::fleet::SourceSidebar::new_at(
        binding.clone(),
        61,
        "research".into(),
        std::sync::Arc::new(|| {}),
        root.0.clone(),
    )
    .unwrap();
    source.state_mut().apply("sidebar.recents", Ok(rows()));
    let snapshots = source
        .state_mut()
        .action(Action::Pin("research".into(), "tip".into(), true));
    assert!(snapshots[0].1.edit_claim.is_some());
    let sends = source
        .state_mut()
        .accept_snapshot(json!({"exists":true,"runtime_revision":3,"runtime_generation":2}));
    let request = binding.request(sends[0].1.clone()).unwrap();
    assert!(request.edit_claim.is_some());
    assert_eq!(
        SourceStamp::from_connection(request.connection.as_ref().unwrap()).unwrap(),
        *source.stamp()
    );
    source.retire("The saved source was removed".into());
    assert!(source.is_retired());
    assert!(source.state().retired);
    assert!(source.state().all_rows().is_empty());
    assert!(source.state().blocks_owner_change());
    assert!(source.state().mutation.as_ref().unwrap().identity.is_some());
    assert_eq!(
        source.state().notice.as_deref(),
        Some("The saved source was removed")
    );
    let mut reopened = root.state(&source_connection, 62);
    assert!(reopened.action(Action::RetryMutation).is_empty());
    drop(source);
    drop(snapshots);
    drop(sends);
    assert!(reopened.action(Action::RetryMutation).is_empty());
    drop(request);
    let retry = reopened.action(Action::RetryMutation);
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].1.body["expected_revision"], 3);
    assert_eq!(retry[0].1.body["expected_generation"], 2);
}

#[test]
fn recovery_cannot_resurrect_an_edit_already_confirmed_by_another_window() {
    let root = Workspace::new();
    let endpoint = connection("confirmed");
    let mut owner = root.state(&endpoint, 71);
    owner.apply("sidebar.recents", Ok(rows()));
    drop(owner.action(Action::Pin("research".into(), "tip".into(), true)));
    let send =
        owner.accept_snapshot(json!({"exists":true,"runtime_revision":7,"runtime_generation":2}));
    let mut peer = root.state(&endpoint, 72);
    assert!(peer.mutation.is_some());
    owner.apply("sidebar.mutation", Ok(json!({"ok":true})));
    drop(send);
    assert!(peer.action(Action::RetryMutation).is_empty());
    assert!(peer.mutation.is_none());
    assert!(peer.recovery.is_none());
    assert!(
        peer.notice
            .as_deref()
            .unwrap()
            .contains("already reconciled")
    );
    assert!(root.state(&endpoint, 73).mutation.is_none());
}
