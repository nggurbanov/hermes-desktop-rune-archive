use super::*;
use crate::{composer::DraftScope, conversation::SendInput, gateway::Connection};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-admission-claim-{}", uuid::Uuid::new_v4())))
    }
    fn journal(&self) -> Journal {
        Journal::new(self.0.clone()).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn prepared() -> Prepared {
    Prepared::new(
        &Connection::Url {
            endpoint: "https://fixture.invalid".into(),
            token: Some("private-fixture-token".into()),
        },
        &DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 7,
            profile: "work".into(),
            session: None,
        },
        SendInput {
            text: "original durable input".into(),
            ..Default::default()
        },
    )
    .unwrap()
}
#[test]
fn concurrent_snapshots_cannot_roll_back_progress_or_retire_a_newer_send() {
    let root = Workspace::new();
    let journal = root.journal();
    let mut entry = prepared();
    journal.save(&entry).unwrap();
    let stale = entry.clone();
    let mut changed_while_queued = entry.clone();
    changed_while_queued.input.text = "different unsaved payload".into();
    assert!(journal.check_current(&changed_while_queued).is_err());
    journal.check_current(&entry).unwrap();
    let first = entry.revision();
    entry.attempted = true;
    journal.save(&entry).unwrap();
    assert!(entry.revision() > first);
    assert!(journal.save(&stale).is_err());
    assert!(journal.retire(&stale).is_err());
    let current = journal.load(&entry.submission_id).unwrap();
    assert!(current.attempted);
    assert_eq!(current.revision(), entry.revision());
    let claim = journal.claim(&entry.submission_id).unwrap();
    assert!(journal.claim(&entry.submission_id).is_err());
    assert!(journal.save(&current).is_err());
    assert!(journal.retire(&current).is_err());
    claim.save(&current).unwrap();
    drop(claim);
    journal.retire(&current).unwrap();
    let path = journal.path(&current.submission_id, "json").unwrap();
    fs::write(path, serde_json::to_vec(&stale).unwrap()).unwrap();
    assert!(journal.load(&current.submission_id).is_err());
    assert!(journal.save(&stale).is_err());
    assert!(journal.entries().unwrap().is_empty());
}
#[test]
fn legacy_revision_zero_migrates_and_failed_writes_do_not_advance_its_stamp() {
    let root = Workspace::new();
    let journal = root.journal();
    let entry = prepared();
    let mut legacy = serde_json::to_value(&entry).unwrap();
    legacy.as_object_mut().unwrap().remove("revision");
    let path = journal.path(&entry.submission_id, "json").unwrap();
    crate::client::private_write(&path, &serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut loaded = journal.load(&entry.submission_id).unwrap();
    assert_eq!(loaded.revision(), 0);
    loaded.attempted = true;
    journal.save(&loaded).unwrap();
    assert_eq!(loaded.revision(), 1);
    let good = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(journal.save(&loaded).is_err());
    assert_eq!(loaded.revision(), 1);
    fs::remove_dir(&path).unwrap();
    fs::write(&path, good).unwrap();
    journal.save(&loaded).unwrap();
    assert_eq!(loaded.revision(), 2);
    let future = br#"{"version":99,"retained":true}"#;
    fs::write(&path, future).unwrap();
    assert!(journal.save(&loaded).is_err());
    assert!(journal.retire(&loaded).is_err());
    assert_eq!(fs::read(&path).unwrap(), future);
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
fn process(root: &Workspace, id: &str, mode: &str) -> Process {
    Process(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "conversation::journal::storage::tests::private_claim_process",
                "--ignored",
                "--nocapture",
            ])
            .env("HERMES_CLAIM_TEST_ROOT", &root.0)
            .env("HERMES_CLAIM_TEST_ID", id)
            .env("HERMES_CLAIM_TEST_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn wait(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !check() {
        assert!(
            Instant::now() < deadline,
            "private claim process did not settle"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn independent_process_claim_is_exclusive_and_a_crash_releases_it_without_pid_guessing() {
    let root = Workspace::new();
    let journal = root.journal();
    let entry = prepared();
    journal.save(&entry).unwrap();
    let mut owner = process(&root, &entry.submission_id, "owner");
    wait(|| {
        assert!(owner.0.try_wait().unwrap().is_none());
        root.0.join("ready").exists()
    });
    let mut contender = process(&root, &entry.submission_id, "contender");
    wait(|| {
        contender.0.try_wait().unwrap().is_some_and(|status| {
            assert!(status.success());
            true
        })
    });
    assert!(root.0.join("contended").exists());
    assert!(journal.claim(&entry.submission_id).is_err());
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    let claim = journal.claim(&entry.submission_id).unwrap();
    let mut current = claim.load(&entry.submission_id).unwrap();
    current.attempted = true;
    claim.save(&current).unwrap();
    assert!(claim.load(&entry.submission_id).unwrap().attempted);
    assert!(claim.save(&entry).is_err());
}
#[test]
#[ignore = "private helper launched by independent_process_claim contract"]
fn private_claim_process() {
    let root =
        PathBuf::from(std::env::var_os("HERMES_CLAIM_TEST_ROOT").expect("private test root"));
    let id = std::env::var("HERMES_CLAIM_TEST_ID").unwrap();
    let journal = Journal::new(root.clone()).unwrap();
    match std::env::var("HERMES_CLAIM_TEST_MODE").unwrap().as_str() {
        "owner" => {
            let _claim = journal.claim(&id).unwrap();
            fs::write(root.join("ready"), b"claimed").unwrap();
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
            panic!("owner should have been killed while holding its claim");
        }
        "contender" => {
            assert!(journal.claim(&id).is_err());
            let entry = journal.load(&id).unwrap();
            assert!(journal.save(&entry).is_err());
            assert!(journal.retire(&entry).is_err());
            fs::write(root.join("contended"), b"exclusive").unwrap();
        }
        _ => panic!("invalid private claim mode"),
    }
}
#[test]
fn a_displayed_confirmation_or_discard_token_cannot_authorize_a_later_revision() {
    let root = Workspace::new();
    let journal = root.journal();
    let mut entry = prepared();
    entry
        .mutations
        .push(crate::conversation::journal::Mutation {
            receipt: None,
            operation: "model".into(),
            params: serde_json::json!({"payload":{"model":"selected"}}),
            complete: false,
            confirmation: Some("first-token".into()),
            refused: false,
        });
    journal.save(&entry).unwrap();
    let shown = entry.revision();
    let (owner, mut newer) = journal.review(&entry.submission_id, shown).unwrap();
    newer.mutations[0].confirmation = Some("newer-token".into());
    owner.save(&newer).unwrap();
    assert!(journal.review(&entry.submission_id, shown).is_err());
    drop(owner);
    assert!(journal.review(&entry.submission_id, shown).is_err());
    let unchanged = journal.load(&entry.submission_id).unwrap();
    assert_eq!(
        unchanged.mutations[0].confirmation.as_deref(),
        Some("newer-token")
    );
    assert!(journal.retire(&entry).is_err());
    let (owner, latest) = journal
        .review(&entry.submission_id, newer.revision())
        .unwrap();
    owner.retire(&latest).unwrap();
}

#[test]
#[cfg(unix)]
fn journal_claim_release_ignores_duplicated_descriptors_after_last_logical_owner() {
    let root = Workspace::new();
    let journal = root.journal();
    let entry = prepared();
    journal.save(&entry).unwrap();
    let claim = journal.claim(&entry.submission_id).unwrap();
    let same_owner = claim.clone();
    let inherited = claim.claim.as_ref().unwrap()._file.try_clone().unwrap();
    drop(claim);
    assert!(journal.claim(&entry.submission_id).is_err());
    drop(same_owner);
    let next_owner = journal.claim(&entry.submission_id).unwrap();
    // A delayed fork/exec child closing its old descriptor cannot release a
    // different owner's newly acquired lock.
    drop(inherited);
    assert!(journal.claim(&entry.submission_id).is_err());
    drop(next_owner);
    journal.retire(&entry).unwrap();
}

#[test]
#[cfg(unix)]
fn journal_transaction_release_ignores_duplicated_descriptors() {
    let root = Workspace::new();
    let journal = root.journal();
    let entry = prepared();
    journal.save(&entry).unwrap();
    let transaction = journal.lock(&entry.submission_id, "state-lock").unwrap();
    let inherited = transaction.try_clone().unwrap();
    drop(transaction);
    assert_eq!(
        journal.load(&entry.submission_id).unwrap().revision(),
        entry.revision()
    );
    drop(inherited);
}

#[test]
fn journal_revision_cannot_be_reused_by_a_different_profile_intent() {
    let root = Workspace::new();
    let journal = root.journal();
    let entry = prepared();
    journal.save(&entry).unwrap();
    let mut other = entry.clone();
    other.profile_intent = crate::source::ProfileIntent::NativeSelf;
    assert!(journal.save(&other).is_err());
    assert!(journal.retire(&other).is_err());
    assert!(
        journal
            .load(&entry.submission_id)
            .unwrap()
            .verify_owner(
                &Connection::Url {
                    endpoint: "https://fixture.invalid".into(),
                    token: Some("private-fixture-token".into())
                },
                &DraftScope {
                    draft_origin: None,
                    connection: 7,
                    profile: "work".into(),
                    profile_intent: crate::source::ProfileIntent::Backend,
                    session: None
                },
            )
            .is_ok()
    );
}
