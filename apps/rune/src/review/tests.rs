use super::*;
fn owner() -> DraftScope {
    DraftScope {
        connection: 7,
        profile: "work".into(),
        session: Some("session".into()),
    }
}
fn loaded() -> Review {
    let mut r = Review::new();
    r.bind(owner(), false, Some("/agent/repo".into()));
    r.apply("review.list",Ok(json!({"files":[{"path":"src/a.rs","added":2,"removed":1,"status":"M","staged":false}],"base":null})));
    r.apply("review.status", Ok(json!({"branch":"feature","changed":1})));
    r.apply("review.head", Ok(json!({"sha":"before"})));
    r
}
#[test]
fn remote_reads_and_mutations_retain_explicit_owner_and_never_run_local_git() {
    let mut r = Review::new();
    let e = r.bind(owner(), false, Some("/remote/repo".into()));
    assert!(e.iter().all(
        |(_, req)| req.path.starts_with("/api/git/") && req.profile.as_deref() == Some("work")
    ));
    let mut r = loaded();
    let e = r.act(Action::Stage(Some("src/a.rs".into())));
    assert_eq!(e[0].1.path, "/api/git/review/stage");
    assert_eq!(e[0].1.body["path"], "/agent/repo");
    assert!(r.act(Action::Stage(None)).is_empty());
}
#[test]
fn branch_and_last_turn_scopes_remain_read_only_and_missing_baseline_is_not_guessed() {
    let mut r = loaded();
    let e = r.act(Action::Scope("lastTurn".into()));
    assert_eq!(
        e[0].1.query.iter().find(|(k, _)| k == "scope").unwrap().1,
        "lastTurn"
    );
    assert!(!e[0].1.query.iter().any(|(k, _)| k == "base"));
    assert!(r.act(Action::Stage(None)).is_empty());
    assert!(r.act(Action::Commit(false)).is_empty());
}
#[test]
fn revert_requires_named_confirmation_and_does_not_acknowledge_failure() {
    let mut r = loaded();
    assert!(r.act(Action::AskRevert(Some("src/a.rs".into()))).is_empty());
    let e = r.act(Action::Confirm);
    assert_eq!(e[0].1.path, "/api/git/review/revert");
    r.apply("review.mutation", Ok(json!({"ok":false})));
    assert!(r.error.is_some());
    assert!(!r.busy);
}
#[test]
fn interrupted_commit_retains_draft_and_requires_review_before_retry() {
    let mut r = loaded();
    r.message.replace_text("feat: change");
    r.act(Action::Commit(false));
    let e = r.apply("review.commit", Err("Disconnected".into()));
    assert_eq!(e[0].1.path, "/api/git/review/rev-parse");
    r.apply("review.commit-reconcile", Ok(json!({"sha":"after"})));
    assert!(r.mutation_uncertain);
    assert_eq!(r.message.text, "feat: change");
    assert!(r.error.as_ref().unwrap().contains("HEAD changed"));
    assert!(r.act(Action::Commit(false)).is_empty());
}
#[test]
fn acknowledged_commit_with_failed_push_clears_only_the_committed_draft_and_keeps_failure() {
    let mut r = loaded();
    r.message.replace_text("feat: change");
    r.act(Action::Commit(true));
    r.apply(
        "review.commit",
        Ok(json!({"ok":false,"committed":true,"commit":"after","error":"Push refused"})),
    );
    assert_eq!(r.message.text, "");
    assert_eq!(r.error.as_deref(), Some("Push refused"));
}
#[test]
fn cancelled_stateless_generation_cannot_change_conversation_or_draft() {
    let mut r = loaded();
    r.runtime_session = Some("runtime-owned".into());
    r.act(Action::Generate);
    let e = r.apply(
        "review.context",
        Ok(json!({"diff":"diff","recent":"recent"})),
    );
    assert_eq!(e[0].1.path, "llm.oneshot");
    assert_eq!(e[0].1.body["session_id"], "runtime-owned");
    r.act(Action::CancelGenerate);
    r.apply("review.generate", Ok(json!({"text":"stale"})));
    assert_eq!(r.message.text, "");
}
#[test]
fn per_workspace_turn_baselines_do_not_leak_to_same_path_on_another_gateway() {
    let mut r = loaded();
    r.capture_turn(owner(), false, "/agent/repo".into());
    r.apply("review.capture", Ok(json!({"sha":"turn-base"})));
    assert_eq!(r.turn_base().as_deref(), Some("turn-base"));
    let mut other = owner();
    other.connection = 8;
    r.bind(other, false, Some("/agent/repo".into()));
    assert!(r.turn_base().is_none());
}
