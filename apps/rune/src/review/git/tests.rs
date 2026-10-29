use super::*;
fn repo() -> (std::path::PathBuf, Git) {
    let path = std::env::temp_dir().join(format!(
        "hermes-review-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    let git = Git::new(path.to_str().unwrap()).unwrap();
    git.bytes(&["init", "-b", "main"]).unwrap();
    git.bytes(&["config", "user.name", "Native test"]).unwrap();
    git.bytes(&["config", "user.email", "native@example.invalid"])
        .unwrap();
    std::fs::write(path.join("file.txt"), "old\n").unwrap();
    git.bytes(&["add", "--", "file.txt"]).unwrap();
    git.bytes(&["commit", "-m", "Initial"]).unwrap();
    let git = Git::new(path.to_str().unwrap()).unwrap();
    (path, git)
}
#[test]
fn real_git_stage_unstage_diff_and_confirmed_restore_use_only_temporary_repository() {
    let (path, g) = repo();
    std::fs::write(path.join("file.txt"), "new\nline\n").unwrap();
    std::fs::write(path.join("fresh.txt"), "fresh\n").unwrap();
    let list = status::review_list(&g, &json!({"scope":"uncommitted"})).unwrap();
    assert_eq!(list["files"].as_array().unwrap().len(), 2);
    let diff = status::diff(&g, &json!({"file":"file.txt","scope":"uncommitted"})).unwrap();
    assert!(diff["diff"].as_str().unwrap().contains("+new"));
    status::mutate(&g, "review/stage", &json!({"file":"file.txt"})).unwrap();
    assert!(
        status::changed(&g)
            .unwrap()
            .iter()
            .any(|f| f.x != b' ' && f.path == "file.txt")
    );
    status::mutate(&g, "review/unstage", &json!({"file":"file.txt"})).unwrap();
    status::mutate(&g, "review/revert", &json!({"file":"file.txt"})).unwrap();
    assert_eq!(
        std::fs::read_to_string(path.join("file.txt")).unwrap(),
        "old\n"
    );
    assert!(path.join("fresh.txt").exists());
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn real_git_commit_mirrors_stage_all_only_when_index_is_empty() {
    let (path, g) = repo();
    std::fs::write(path.join("file.txt"), "changed\n").unwrap();
    std::fs::write(path.join("fresh.txt"), "fresh\n").unwrap();
    status::mutate(&g, "review/stage", &json!({"file":"file.txt"})).unwrap();
    let result = shipping::commit(&g, &json!({"message":"Change tracked","push":false})).unwrap();
    assert_eq!(result["committed"], true);
    assert!(
        status::changed(&g)
            .unwrap()
            .iter()
            .any(|f| f.path == "fresh.txt" && f.x == b'?')
    );
    shipping::commit(&g, &json!({"message":"Add fresh","push":false})).unwrap();
    assert!(status::changed(&g).unwrap().is_empty());
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn worktree_creation_and_removal_preserve_the_branch_and_never_publish() {
    let (path, g) = repo();
    let result = worktrees::add(
        &g,
        &json!({"name":"Task","branch":"feature/test","base":"main"}),
    )
    .unwrap();
    let created = result["path"].as_str().unwrap();
    assert!(Path::new(created).is_dir());
    let trees = worktrees::list(&g).unwrap();
    assert!(
        trees["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["branch"] == "feature/test")
    );
    worktrees::remove(&g, &json!({"worktreePath":created,"force":false})).unwrap();
    assert!(!Path::new(created).exists());
    assert!(g.ok(&["show-ref", "--verify", "--quiet", "refs/heads/feature/test"]));
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn changed_paths_with_spaces_and_renames_use_nul_record_framing() {
    let (path, g) = repo();
    g.bytes(&["mv", "file.txt", "new name.txt"]).unwrap();
    let rows = status::changed(&g).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "new name.txt");
    assert_eq!(rows[0].x, b'R');
    assert!(file(&json!({"file":"../outside"})).is_err());
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn confirmed_restore_discards_staged_additions_and_restores_both_sides_of_a_rename() {
    let (path, g) = repo();
    g.bytes(&["mv", "file.txt", "renamed.txt"]).unwrap();
    std::fs::write(
        path.join("added.txt"),
        "new
",
    )
    .unwrap();
    status::mutate(&g, "review/stage", &json!({"file":"added.txt"})).unwrap();
    status::mutate(&g, "review/revert", &json!({"file":"renamed.txt"})).unwrap();
    assert!(path.join("file.txt").exists());
    assert!(!path.join("renamed.txt").exists());
    status::mutate(&g, "review/revert", &json!({"file":"added.txt"})).unwrap();
    assert!(!path.join("added.txt").exists());
    assert!(status::changed(&g).unwrap().is_empty());
    std::fs::remove_dir_all(path).unwrap();
}
