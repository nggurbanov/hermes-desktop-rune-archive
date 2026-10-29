use super::*;
use std::path::Path;
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("hermes-editors-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("drafts.json")
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn scope(epoch: u64, session: Option<&str>) -> DraftScope {
    DraftScope {
        connection: epoch,
        profile: "work".into(),
        session: session.map(str::to_owned),
    }
}
fn bind(id: EditorId, path: &Path, epoch: u64) -> Composer {
    let mut c = Composer::with_editor(id);
    c.bind_storage_path(path.to_owned(), epoch).unwrap();
    c
}
fn save(c: &mut Composer, now: f64) {
    assert!(c.poll_storage(now).unwrap().is_some());
    assert!(c.poll_storage(now + 0.4).unwrap().is_none());
}
fn load(id: &EditorId, path: &Path, epoch: u64) -> Composer {
    bind(id.clone(), path, epoch)
}
fn state(path: &Path) -> State {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
#[test]
fn fresh_and_same_session_editors_keep_independent_text_attachments_and_policy() {
    let mut first = Composer::new();
    let fresh = scope(1, None);
    first
        .draft_mut(fresh.clone())
        .text
        .replace_text("fresh one");
    first
        .draft_mut(fresh.clone())
        .attachments
        .push(DraftAttachment {
            path: "/private/one.pdf".into(),
            name: "one.pdf".into(),
            preview: None,
        });
    first.choose_reasoning(fresh.clone(), Some("high".into()));
    let mut second = first.fork_editor(&fresh);
    assert_ne!(first.editor_id(), second.editor_id());
    assert_eq!(second.draft(&fresh).unwrap().text.text, "fresh one");
    assert!(second.draft(&fresh).unwrap().attachments.is_empty());
    assert!(second.draft(&fresh).unwrap().replay.is_none());
    assert_ne!(
        first.draft(&fresh).unwrap().policy_id,
        second.draft(&fresh).unwrap().policy_id
    );
    second
        .draft_mut(fresh.clone())
        .text
        .replace_text("fresh two");
    second.choose_reasoning(fresh.clone(), Some("low".into()));
    assert_eq!(first.draft(&fresh).unwrap().text.text, "fresh one");
    assert_eq!(
        first.draft(&fresh).unwrap().reasoning.as_deref(),
        Some("high")
    );
    let existing = scope(1, Some("same"));
    first.rehome(&fresh, existing.clone());
    second.rehome(&fresh, existing.clone());
    second
        .draft_mut(existing.clone())
        .text
        .replace_text("second existing");
    assert_eq!(first.draft(&existing).unwrap().text.text, "fresh one");
    let wrong = first.draft(&existing).unwrap().policy_stamp();
    assert!(!second.settle_policy(
        &existing,
        &wrong,
        PolicyFields {
            reasoning: true,
            ..Default::default()
        }
    ));
}
#[test]
fn two_editors_merge_their_entries_and_restore_exact_drafts_under_new_live_epochs() {
    let files = Files::new();
    let path = files.path();
    let mut first = bind(EditorId::new(), &path, 1);
    let mut second = bind(EditorId::new(), &path, 2);
    let a = scope(1, None);
    let b = scope(2, None);
    first.draft_mut(a.clone()).text.replace_text("first unsent");
    first.choose_reasoning(a.clone(), Some("high".into()));
    first
        .draft_mut(a.clone())
        .attachments
        .push(DraftAttachment {
            path: "/missing/kept.pdf".into(),
            name: "kept.pdf".into(),
            preview: None,
        });
    second.draft_mut(b).text.replace_text("second unsent");
    let stamp = first.draft(&a).unwrap().policy_stamp();
    save(&mut first, 0.);
    save(&mut second, 1.);
    let restored = load(first.editor_id(), &path, 90);
    let other = load(second.editor_id(), &path, 91);
    let draft = restored.draft(&scope(90, None)).unwrap();
    assert_eq!(draft.text.text, "first unsent");
    assert_eq!(draft.policy_stamp(), stamp);
    assert!(draft.reasoning_changed);
    assert_eq!(
        draft.attachments[0].path,
        PathBuf::from("/missing/kept.pdf")
    );
    assert_eq!(
        other.draft(&scope(91, None)).unwrap().text.text,
        "second unsent"
    );
    assert!(other.draft(&scope(90, None)).is_none());
    assert_eq!(state(&path).entries.len(), 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[test]
fn a_stale_same_editor_clear_cannot_replace_newer_saved_text() {
    let files = Files::new();
    let path = files.path();
    let id = EditorId::new();
    let mut first = bind(id.clone(), &path, 1);
    let owner = scope(1, Some("same"));
    first
        .draft_mut(owner.clone())
        .text
        .replace_text("submitted text");
    save(&mut first, 0.);
    let mut stale = bind(id.clone(), &path, 2);
    first
        .draft_mut(owner)
        .text
        .replace_text("newer unsent text");
    save(&mut first, 1.);
    stale
        .draft_mut(scope(2, Some("same")))
        .text
        .replace_text("");
    stale.poll_storage(2.).unwrap();
    assert!(stale.poll_storage(2.4).is_err());
    assert!(stale.persisted.as_ref().unwrap().pending.is_some());
    assert_eq!(
        load(&id, &path, 3)
            .draft(&scope(3, Some("same")))
            .unwrap()
            .text
            .text,
        "newer unsent text"
    );
}
#[test]
fn tombstones_survive_an_unrelated_stale_snapshot_and_reject_resurrection() {
    let files = Files::new();
    let path = files.path();
    let id = EditorId::new();
    let mut first = bind(id.clone(), &path, 1);
    first
        .draft_mut(scope(1, Some("removed")))
        .text
        .replace_text("never resurrect this");
    save(&mut first, 0.);
    let mut stale = bind(id.clone(), &path, 2);
    first.delete_profile(1, "work");
    save(&mut first, 1.);
    stale
        .draft_mut(scope(2, Some("unrelated")))
        .text
        .replace_text("keep another entry");
    save(&mut stale, 2.);
    let saved = state(&path);
    assert!(
        saved
            .entries
            .iter()
            .any(|e| e.session.as_deref() == Some("removed") && e.draft.is_none())
    );
    let restored = load(&id, &path, 3);
    assert!(restored.draft(&scope(3, Some("removed"))).is_none());
    assert_eq!(
        restored
            .draft(&scope(3, Some("unrelated")))
            .unwrap()
            .text
            .text,
        "keep another entry"
    );
    stale
        .draft_mut(scope(2, Some("removed")))
        .text
        .insert(" stale edit");
    stale.poll_storage(3.).unwrap();
    assert!(stale.poll_storage(3.4).is_err());
}
#[test]
fn favorite_patches_keep_other_windows_choices_and_revision_fences() {
    let files = Files::new();
    let path = files.path();
    let mut a = bind(EditorId::new(), &path, 1);
    let mut b = bind(EditorId::new(), &path, 2);
    a.toggle_favorite(1, "p", "one");
    b.toggle_favorite(2, "p", "two");
    save(&mut a, 0.);
    save(&mut b, 1.);
    let mut stale = bind(EditorId::new(), &path, 3);
    a.toggle_favorite(1, "p", "one");
    save(&mut a, 2.);
    a.toggle_favorite(1, "p", "one");
    save(&mut a, 3.);
    stale.toggle_favorite(3, "p", "one");
    stale.poll_storage(4.).unwrap();
    assert!(stale.poll_storage(4.4).is_err());
    let saved = state(&path);
    assert_eq!(
        saved.favorites,
        vec![("p".into(), "one".into()), ("p".into(), "two".into())]
    );
}
#[test]
fn legacy_formats_migrate_once_without_lending_attachments_to_a_second_editor() {
    for version in [1, 2, 3] {
        let files = Files::new();
        let path = files.path();
        let mut source = Composer::new();
        let owner = scope(1, Some("saved"));
        source
            .draft_mut(owner.clone())
            .text
            .replace_text("old text");
        source.choose_approval(owner.clone(), Some("smart".into()));
        source
            .draft_mut(owner.clone())
            .attachments
            .push(DraftAttachment {
                path: "/old/input.svg".into(),
                name: "input.svg".into(),
                preview: None,
            });
        let saved = SavedDraft::capture(&owner, source.draft(&owner).unwrap());
        let stamp = saved.policy_id.clone();
        let old = State {
            version,
            drafts: vec![saved],
            favorites: vec![("p".into(), "m".into())],
            ..Default::default()
        };
        crate::client::private_write(&path, &serde_json::to_vec(&old).unwrap()).unwrap();
        let migrated = bind(EditorId::new(), &path, 2);
        let second = bind(EditorId::new(), &path, 3);
        let d = migrated.draft(&scope(2, Some("saved"))).unwrap();
        assert_eq!(d.text.text, "old text");
        assert_eq!(d.policy_id, stamp);
        assert_eq!(d.attachments.len(), 1);
        assert!(second.draft(&scope(3, Some("saved"))).is_none());
        assert!(second.is_favorite(3, "p", "m"));
        assert_eq!(state(&path).version, 4);
    }
}
#[test]
fn invalid_and_future_files_are_preserved_even_after_failed_bind_drop() {
    for bytes in [b"not json".as_slice(), b"{\"version\":999}", b""] {
        let files = Files::new();
        let path = files.path();
        std::fs::write(&path, bytes).unwrap();
        let mut composer = Composer::new();
        composer
            .draft_mut(scope(1, None))
            .text
            .replace_text("unsaved text");
        assert!(composer.bind_storage_path(path.clone(), 1).is_err());
        drop(composer);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
#[test]
fn write_failure_retries_the_latest_snapshot_without_another_edit() {
    let files = Files::new();
    let blocked = files.0.join("blocked");
    std::fs::write(&blocked, b"file").unwrap();
    let path = blocked.join("drafts.json");
    let mut c = Composer::new();
    c.draft_mut(scope(1, None))
        .text
        .replace_text("latest unsaved text");
    c.persisted = Some(Persistence {
        epoch: 1,
        path: path.clone(),
        previous: Snapshot::default(),
        observed: Observed::default(),
        pending: None,
        scanned_revision: None,
    });
    c.poll_storage(0.).unwrap();
    assert!(c.poll_storage(0.4).is_err());
    assert!(c.persisted.as_ref().unwrap().pending.is_some());
    std::fs::remove_file(&blocked).unwrap();
    c.poll_storage(1.5).unwrap();
    assert_eq!(
        load(c.editor_id(), &path, 2)
            .draft(&scope(2, None))
            .unwrap()
            .text
            .text,
        "latest unsaved text"
    );
}
#[test]
fn rebinding_flushes_only_its_principal_and_retains_other_live_epochs_in_memory() {
    let files = Files::new();
    let first = files.path();
    let second = files.0.join("other.json");
    let id = EditorId::new();
    let mut c = bind(id.clone(), &first, 1);
    c.draft_mut(scope(1, None))
        .text
        .replace_text("first source");
    c.bind_storage_path(second.clone(), 2).unwrap();
    c.draft_mut(scope(2, None))
        .text
        .replace_text("second source");
    save(&mut c, 0.);
    assert_eq!(
        load(&id, &first, 3)
            .draft(&scope(3, None))
            .unwrap()
            .text
            .text,
        "first source"
    );
    assert_eq!(
        load(&id, &second, 4)
            .draft(&scope(4, None))
            .unwrap()
            .text
            .text,
        "second source"
    );
    assert_eq!(state(&first).entries.len(), 1);
    assert_eq!(state(&second).entries.len(), 1);
}
#[test]
fn primary_editor_identity_is_stable_and_corruption_is_never_replaced() {
    let files = Files::new();
    let path = files.0.join("primary.json");
    let id = store::primary(&path).unwrap();
    assert_eq!(store::primary(&path).unwrap(), id);
    std::fs::write(&path, b"bad identity").unwrap();
    assert!(store::primary(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"bad identity");
}
#[test]
fn a_new_stored_session_view_seeds_text_but_not_local_files_or_fresh_chat_text() {
    let files = Files::new();
    let path = files.path();
    let mut source = bind(EditorId::new(), &path, 1);
    let owner = scope(1, Some("shared"));
    source
        .draft_mut(owner.clone())
        .text
        .replace_text("shared stash text");
    source
        .draft_mut(owner.clone())
        .attachments
        .push(DraftAttachment {
            path: "/private/one.pdf".into(),
            name: "one.pdf".into(),
            preview: None,
        });
    save(&mut source, 0.);
    let mut next = bind(EditorId::new(), &path, 2);
    let other = scope(2, Some("shared"));
    assert!(next.seed_stored_draft(&other).unwrap());
    assert_eq!(next.draft(&other).unwrap().text.text, "shared stash text");
    assert!(next.draft(&other).unwrap().attachments.is_empty());
    assert_ne!(
        next.draft(&other).unwrap().policy_id,
        source.draft(&owner).unwrap().policy_id
    );
    assert!(!next.seed_stored_draft(&scope(2, None)).unwrap());
    assert!(!next.seed_stored_draft(&scope(99, Some("shared"))).unwrap());
    source.delete_profile(1, "work");
    save(&mut source, 1.);
    let mut after_clear = bind(EditorId::new(), &path, 3);
    assert!(
        !after_clear
            .seed_stored_draft(&scope(3, Some("shared")))
            .unwrap()
    );
    assert_eq!(
        next.draft(&other).unwrap().text.text,
        "shared stash text",
        "another view clearing the stash never clears a live local input"
    );
}
#[test]
fn a_cross_process_transaction_keeps_both_fresh_editors() {
    let files = Files::new();
    let ids = [EditorId::new(), EditorId::new()];
    let mut children = vec![];
    for id in &ids {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "composer::persistence::tests::private_process_writer",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("RUNE_DRAFT_PROCESS_TEST_DIR", &files.0)
                .env("RUNE_DRAFT_PROCESS_TEST_EDITOR", id.as_str())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !ids.iter().all(|id| files.0.join(id.as_str()).exists()) {
        if std::time::Instant::now() > deadline {
            for child in &mut children {
                let _ = child.kill();
            }
            panic!("private writer children did not become ready");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    std::fs::write(files.0.join("go"), b"ready").unwrap();
    for mut child in children {
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                panic!("private writer child timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let saved = state(&files.path());
    assert_eq!(saved.entries.len(), 2);
    assert_ne!(saved.entries[0].revision, saved.entries[1].revision);
    for id in &ids {
        let restored = load(id, &files.path(), 8);
        assert_eq!(
            restored.draft(&scope(8, None)).unwrap().text.text,
            id.as_str()
        );
    }
}
#[test]
#[ignore = "private child entry point, exercised by a_cross_process_transaction_keeps_both_fresh_editors"]
fn private_process_writer() {
    let path = PathBuf::from(
        std::env::var_os("RUNE_DRAFT_PROCESS_TEST_DIR").expect("private parent test directory"),
    );
    let id = EditorId::parse(&std::env::var("RUNE_DRAFT_PROCESS_TEST_EDITOR").unwrap()).unwrap();
    let mut composer = bind(id.clone(), &path.join("drafts.json"), 1);
    std::fs::write(path.join(id.as_str()), b"ready").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !path.join("go").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "private writer parent did not release gate"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    composer
        .draft_mut(scope(1, None))
        .text
        .replace_text(id.as_str());
    save(&mut composer, 0.);
}
#[test]
fn migration_transfers_the_actual_input_only_after_the_target_is_durable() {
    let files = Files::new();
    let path = files.path();
    let mut primary = bind(EditorId::new(), &path, 1);
    let owner = scope(1, Some("session"));
    primary
        .draft_mut(owner.clone())
        .text
        .replace_text("keep my actual draft");
    primary
        .draft_mut(owner.clone())
        .attachments
        .push(DraftAttachment {
            path: "/saved/one.bin".into(),
            name: "one.bin".into(),
            preview: None,
        });
    save(&mut primary, 0.);
    let stamp = primary.draft(&owner).unwrap().policy_stamp();
    let mut target = bind(EditorId::new(), &path, 1);
    assert!(primary.transfer_draft_to(&mut target, &owner).unwrap());
    assert!(primary.draft(&owner).is_none());
    assert_eq!(target.draft(&owner).unwrap().policy_stamp(), stamp);
    let restored = load(target.editor_id(), &path, 2);
    assert_eq!(
        restored
            .draft(&scope(2, Some("session")))
            .unwrap()
            .attachments[0]
            .path,
        PathBuf::from("/saved/one.bin")
    );
    // The old durable copy may still exist until its delayed clear; no crash gap loses the input.
    assert!(
        state(&path)
            .entries
            .iter()
            .any(|e| &e.editor == target.editor_id() && e.draft.is_some())
    );
    save(&mut primary, 1.);
    assert!(
        state(&path)
            .entries
            .iter()
            .any(|e| &e.editor == primary.editor_id() && e.draft.is_none())
    );
}
#[test]
fn slot_identity_survives_profile_rename_rehome_and_delete_recreate() {
    let files = Files::new();
    let path = files.path();
    let window = EditorId::new();
    let fresh = Key {
        profile: "old".into(),
        session: None,
    };
    let id = store::slot(&path, &window, &fresh).unwrap();
    assert_eq!(store::slot(&path, &window, &fresh).unwrap(), id);
    let stored = Key {
        profile: "old".into(),
        session: Some("saved".into()),
    };
    store::rehome_slot(&path, &window, &fresh, &stored, &id).unwrap();
    assert_eq!(store::slot(&path, &window, &stored).unwrap(), id);
    assert_ne!(store::slot(&path, &window, &fresh).unwrap(), id);
    store::rename_profile(&path, "old", "renamed").unwrap();
    let renamed = Key {
        profile: "renamed".into(),
        session: Some("saved".into()),
    };
    assert_eq!(store::slot(&path, &window, &renamed).unwrap(), id);
    store::delete_profile(&path, "renamed").unwrap();
    assert_ne!(store::slot(&path, &window, &renamed).unwrap(), id);
}
