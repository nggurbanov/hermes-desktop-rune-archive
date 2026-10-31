use super::*;
use crate::{
    gateway::Connection,
    source::{ProfileIntent, SourceStamp},
};

fn owner(home: &str, epoch: u64, intent: ProfileIntent) -> Owner {
    let source = SourceStamp::from_connection(&Connection::Local { home: home.into() }).unwrap();
    Owner {
        session: SessionOwner::with_intent(source, "same", intent),
        epoch,
    }
}
fn binding(home: &str) -> Binding {
    Binding::new(Connection::Local { home: home.into() }, None).unwrap()
}
fn tree(path: &str) -> Value {
    json!({"projects":[{"id":"same", "label":"Project", "path":path, "repos":[]}]})
}
#[test]
fn source_switch_retires_same_id_and_ignores_late_reply() {
    let a = owner("/tmp/project-source-a", 1, ProfileIntent::Backend);
    let b = owner("/tmp/project-source-b", 2, ProfileIntent::Backend);
    let mut state = State::default();
    state.bind(a);
    let (old, request) = state
        .begin(LoadKind::Tree, &binding("/tmp/project-source-a"))
        .unwrap();
    assert_eq!(request.path, "projects.tree");
    assert_eq!(request.body, json!({"profile":"same", "preview_limit":3}));
    state.bind(b.clone());
    let (new, _) = state
        .begin(LoadKind::Tree, &binding("/tmp/project-source-b"))
        .unwrap();
    assert!(!state.finish(old, &binding("/tmp/project-source-a"), Ok(tree("/foreign"))));
    assert!(state.finish(new, &binding("/tmp/project-source-b"), Ok(tree("/correct"))));
    state.select(Scope::Project("same".into()));
    let captured = state
        .capture(CwdChoice::Unspecified, Some("/default"))
        .unwrap();
    assert_eq!(captured.cwd, CwdChoice::Path("/correct".into()));
    assert_eq!(captured.owner, b);
    state.retire();
    assert!(state.capture(CwdChoice::Unspecified, None).is_err());
    assert_eq!(captured.cwd, CwdChoice::Path("/correct".into()));
}
#[test]
fn navigation_retry_and_captured_cwd_obey_precedence() {
    let mut state = State::default();
    state.bind(owner("/tmp/project-source-a", 1, ProfileIntent::Backend));
    let (ticket, _) = state
        .begin(LoadKind::Tree, &binding("/tmp/project-source-a"))
        .unwrap();
    assert!(state.finish(
        ticket,
        &binding("/tmp/project-source-a"),
        Err("offline".into())
    ));
    assert_eq!(state.error.as_deref(), Some("offline"));
    let (ticket, _) = state
        .begin(LoadKind::Tree, &binding("/tmp/project-source-a"))
        .unwrap();
    assert!(state.finish(
        ticket,
        &binding("/tmp/project-source-a"),
        Ok(tree("/project"))
    ));
    state.select(Scope::Home);
    let home = state
        .capture(CwdChoice::Unspecified, Some("/default"))
        .unwrap();
    assert_eq!(home.cwd, CwdChoice::Detached);
    state.select(Scope::Project("same".into()));
    let (ticket, request) = state
        .begin(
            LoadKind::Entered("same".into()),
            &binding("/tmp/project-source-a"),
        )
        .unwrap();
    assert_eq!(request.path, "projects.project_sessions");
    assert_eq!(request.body, json!({"profile":"same", "project_id":"same"}));
    state.select(Scope::All);
    assert!(!state.finish(
        ticket,
        &binding("/tmp/project-source-a"),
        Ok(json!({"project":{"id":"same","label":"Late"}}))
    ));
    let all = state
        .capture(CwdChoice::Unspecified, Some("/default"))
        .unwrap();
    assert_eq!(all.cwd, CwdChoice::Path("/default".into()));
    assert!(!all.explicit);
    state.select(Scope::Project("same".into()));
    let project = state.capture(CwdChoice::Unspecified, None).unwrap();
    let clicked = state
        .capture(CwdChoice::Path("/worktree".into()), None)
        .unwrap();
    let detached = state
        .capture(CwdChoice::Detached, Some("/default"))
        .unwrap();
    let mut body = json!({"source":"desktop"});
    project.apply(&mut body);
    assert_eq!(
        body,
        json!({"source":"desktop","cwd":"/project","cwd_explicit":true})
    );
    let mut body = json!({});
    detached.apply(&mut body);
    assert_eq!(body, json!({}));
    assert_eq!(clicked.cwd, CwdChoice::Path("/worktree".into()));
    state.bind(owner("/tmp/project-source-a", 1, ProfileIntent::NativeSelf));
    assert_eq!(state.scope, Scope::All);
    assert!(state.tree.is_none());
    assert_eq!(project.cwd, CwdChoice::Path("/project".into()));
}

#[test]
fn home_hydration_is_fenced_by_selection_and_epoch() {
    let mut state = State::default();
    let first = owner("/tmp/project-source-a", 1, ProfileIntent::Backend);
    let source = binding("/tmp/project-source-a");
    state.bind(first.clone());
    state.select(Scope::Home);
    let (ticket, request) = state
        .begin(LoadKind::Entered("__no_project__".into()), &source)
        .unwrap();
    assert_eq!(
        request.body,
        json!({"profile":"same", "project_id":"__no_project__"})
    );
    state.bind(owner("/tmp/project-source-a", 2, ProfileIntent::Backend));
    assert!(!state.finish(
        ticket,
        &source,
        Ok(json!({"project":{"id":"__no_project__","label":"Home","isNoProject":true}}))
    ));
    assert!(state.entered.is_none());
    state.bind(first);
    state.select(Scope::Home);
    let (ticket, _) = state
        .begin(LoadKind::Entered("__no_project__".into()), &source)
        .unwrap();
    assert!(state.finish(
        ticket,
        &source,
        Ok(json!({"project":{"id":"__no_project__","label":"Home","isNoProject":true}}))
    ));
    assert!(state.entered.as_ref().unwrap().is_no_project);
}

#[test]
fn project_requests_retain_explicit_profile_provenance() {
    let source = binding("/tmp/project-source-a");
    for (intent, expected) in [
        (
            ProfileIntent::Backend,
            ProfileProvenance::Backend("same".into()),
        ),
        (ProfileIntent::NativeSelf, ProfileProvenance::NativeSelf),
    ] {
        let mut state = State::default();
        state.bind(owner("/tmp/project-source-a", 1, intent));
        let (_, request) = state.begin(LoadKind::Tree, &source).unwrap();
        assert_eq!(request.provenance, Some(expected));
    }
}

#[test]
fn rapid_show_all_tree_requests_fence_older_results() {
    let source = binding("/tmp/project-source-a");
    let mut state = State::default();
    state.bind(owner("/tmp/project-source-a", 1, ProfileIntent::Backend));
    let (old, first) = state.begin(LoadKind::Tree, &source).unwrap();
    state.show_all_sessions = true;
    let (middle, second) = state.begin(LoadKind::Tree, &source).unwrap();
    state.show_all_sessions = false;
    let (latest, third) = state.begin(LoadKind::Tree, &source).unwrap();
    assert_eq!(first.body["preview_limit"], 3);
    assert_eq!(second.body["preview_limit"], 2000);
    assert_eq!(third.body["preview_limit"], 3);
    assert!(!state.finish(middle, &source, Ok(tree("/stale-wide"))));
    assert!(state.finish(latest, &source, Ok(tree("/current"))));
    assert!(!state.finish(old, &source, Ok(tree("/stale-narrow"))));
    assert_eq!(
        state.tree.unwrap().projects[0].path.as_deref(),
        Some("/current")
    );
}
