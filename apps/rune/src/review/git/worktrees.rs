use super::*;
pub fn list(g: &Git) -> Result<Value, String> {
    if !g.is_repo {
        return Ok(json!({"worktrees":[]}));
    }
    let bytes = g.bytes(&["worktree", "list", "--porcelain", "-z"])?;
    let mut rows = vec![];
    let mut current = Value::Null;
    for line in bytes.split(|b| *b == 0) {
        let line = std::str::from_utf8(line).map_err(|_| "A worktree has a non-UTF-8 path")?;
        if let Some(path) = line.strip_prefix("worktree ") {
            if current.is_object() {
                rows.push(current);
            }
            current = json!({"path":path,"branch":null,"isMain":rows.is_empty(),"detached":false,"locked":false});
        } else if current.is_object() {
            if let Some(branch) = line.strip_prefix("branch ") {
                current["branch"] = json!(branch.trim_start_matches("refs/heads/"));
            } else if line == "detached" {
                current["detached"] = json!(true);
            } else if line.starts_with("locked") {
                current["locked"] = json!(true);
            }
        }
    }
    if current.is_object() {
        rows.push(current);
    }
    Ok(json!({"worktrees":rows}))
}
pub fn branches(g: &Git, bases: bool) -> Result<Value, String> {
    if !g.is_repo {
        return Ok(json!({"branches":[]}));
    }
    let locals = g.text(&[
        "for-each-ref",
        "--format=%(refname:short)",
        "--sort=-committerdate",
        "refs/heads",
    ])?;
    let remotes = g.text(&[
        "for-each-ref",
        "--format=%(refname:short)",
        "--sort=-committerdate",
        "refs/remotes",
    ])?;
    let local = locals
        .lines()
        .filter(|s| !s.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let trees = list(g)?;
    let trunk = g.default_branch();
    let remote_default = g.optional(&[
        "symbolic-ref",
        "--quiet",
        "--short",
        "refs/remotes/origin/HEAD",
    ]);
    let mut rows = vec![];
    for (is_remote, names) in [(false, locals.as_str()), (true, remotes.as_str())] {
        for name in names
            .lines()
            .filter(|n| !n.is_empty() && !n.ends_with("/HEAD"))
        {
            if is_remote
                && !bases
                && name
                    .split_once('/')
                    .is_some_and(|(_, short)| local.contains(short))
            {
                continue;
            }
            let tree = trees["worktrees"]
                .as_array()
                .and_then(|t| t.iter().find(|t| t["branch"] == name));
            rows.push(json!({"name":name,"isRemote":is_remote,"isDefault":if bases{remote_default.as_deref()==Some(name)||!is_remote&&trunk.as_deref()==Some(name)}else{!is_remote&&trunk.as_deref()==Some(name)},"checkedOut":!is_remote&&tree.is_some(),"worktreePath":tree.map(|t|t["path"].clone())}));
        }
    }
    Ok(json!({"branches":rows}))
}
fn sanitize(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        let c = if c.is_whitespace() { '-' } else { c };
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-') {
            if (c == '-' && out.ends_with('-'))
                || (c == '/' && out.ends_with('/'))
                || (c == '.' && out.ends_with('.'))
            {
                continue;
            }
            out.push(c);
        }
    }
    out.trim_matches(['-', '.', '/']).into()
}
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "work".into()
    } else {
        out.into()
    }
}
fn root(g: &Git) -> Result<PathBuf, String> {
    let trees = list(g)?;
    Ok(trees["worktrees"]
        .as_array()
        .and_then(|r| r.first())
        .and_then(|r| r["path"].as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| g.cwd.clone()))
}
fn unique(root: &Path, name: &str) -> Result<PathBuf, String> {
    for n in 1..1000 {
        let path = root.join(if n == 1 {
            name.into()
        } else {
            format!("{name}-{n}")
        });
        if !path
            .try_exists()
            .map_err(|_| "The worktree destination could not be checked")?
        {
            return Ok(path);
        }
    }
    Err("No free worktree folder name is available".into())
}
fn fetch(g: &Git, remote: &str, branch: &str) -> bool {
    if !g.ref_valid(branch) || remote.is_empty() || remote.starts_with('-') {
        return false;
    }
    g.ok(&[
        "fetch",
        remote,
        &format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}"),
    ])
}
pub fn switch(g: &Git, args: &Value) -> Result<Value, String> {
    if !g.is_repo {
        return Ok(json!({"branch":null}));
    }
    let branch = sanitize(&text(args, "branch"));
    if !g.ref_valid(&branch) {
        return Err("Choose a valid branch name".into());
    }
    g.bytes(&["switch", "--", &branch])?;
    Ok(json!({"branch":branch}))
}
pub fn add(g: &Git, args: &Value) -> Result<Value, String> {
    if !g.is_repo {
        g.bytes(&["init"])?;
    }
    if g.head().is_none() {
        g.bytes(&[
            "-c",
            "user.email=hermes@localhost",
            "-c",
            "user.name=Hermes",
            "commit",
            "--allow-empty",
            "-m",
            "Initial commit",
        ])?;
    }
    let main = root(g)?;
    let g = Git::new(main.to_str().ok_or("The main worktree path is not UTF-8")?)?;
    let existing = sanitize(&text(args, "existingBranch"));
    if !existing.is_empty() {
        let trees = list(&g)?;
        if let Some(row) = trees["worktrees"]
            .as_array()
            .and_then(|t| t.iter().find(|t| t["branch"] == existing))
        {
            return Ok(json!({"path":row["path"],"branch":existing,"repoRoot":main}));
        }
        let mut remote = existing
            .split_once('/')
            .filter(|_| {
                g.ok(&[
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/remotes/{existing}"),
                ])
            })
            .map(|(r, _)| r.to_owned());
        let mut fetched = false;
        if remote.is_none()
            && !g.ok(&[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{existing}"),
            ])
        {
            if let Some((r, b)) = existing.split_once('/') {
                if g.ok(&["remote", "get-url", r]) && fetch(&g, r, b) {
                    remote = Some(r.into());
                    fetched = true;
                }
            }
        }
        let branch = if let Some(remote) = &remote {
            existing
                .strip_prefix(&format!("{remote}/"))
                .unwrap_or(&existing)
                .to_owned()
        } else {
            existing.clone()
        };
        if remote.is_none() && g.default_branch().as_deref() == Some(&branch) {
            g.bytes(&["switch", "--", &branch])?;
            return Ok(json!({"path":main,"branch":branch,"repoRoot":main}));
        }
        let path = unique(&main.join(".worktrees"), &slug(&branch))?;
        let destination = path.to_str().ok_or("The worktree path is not UTF-8")?;
        if let Some(remote) = remote {
            fetched = fetched || fetch(&g, &remote, &branch);
            if !g.ok(&[
                "worktree",
                "add",
                "--track",
                "-b",
                &branch,
                destination,
                &existing,
            ]) {
                g.bytes(&["worktree", "add", "-b", &branch, destination, &existing])?;
                if fetched {
                    g.bytes(&["remote", "set-branches", "--add", &remote, &branch])?;
                    g.bytes(&["branch", &format!("--set-upstream-to={existing}"), &branch])?;
                }
            }
        } else {
            g.bytes(&["worktree", "add", destination, &branch])?;
        }
        return Ok(json!({"path":path,"branch":branch,"repoRoot":main}));
    }
    let name = text(args, "name");
    let name = if name.is_empty() {
        format!(
            "work-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    } else {
        name
    };
    let slug = slug(&name);
    let requested = sanitize(&text(args, "branch"));
    let branch = if requested.is_empty() {
        format!("hermes/{slug}")
    } else {
        requested
    };
    if !g.ref_valid(&branch) {
        return Err("Choose a valid worktree branch name".into());
    }
    let path = unique(&main.join(".worktrees"), &slug)?;
    let destination = path.to_str().ok_or("The worktree path is not UTF-8")?;
    let base = text(args, "base");
    let mut argv = vec!["worktree", "add", "-b", &branch, destination];
    if !base.is_empty() {
        if base.starts_with('-') || base.contains('\0') {
            return Err("Choose a valid worktree base".into());
        }
        if let Some(remote_branch) = base.strip_prefix("origin/") {
            fetch(&g, "origin", remote_branch);
            argv.push("--no-track");
        }
        argv.push(&base);
    }
    if let Err(error) = g.bytes(&argv) {
        if g.ok(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ]) && !path.exists()
        {
            g.bytes(&["worktree", "add", destination, &branch])?;
        } else {
            return Err(error);
        }
    }
    Ok(json!({"path":path,"branch":branch,"repoRoot":main}))
}
pub fn remove(g: &Git, args: &Value) -> Result<Value, String> {
    let requested = text(args, "worktreePath");
    if requested.is_empty() {
        return Err("Choose a worktree to remove".into());
    }
    let trees = list(g)?;
    let Some(row) = trees["worktrees"]
        .as_array()
        .and_then(|r| r.iter().find(|r| r["path"] == requested))
    else {
        return Err("Refresh the registered worktrees before removing this path".into());
    };
    if row["isMain"] == true {
        return Err("The main checkout cannot be removed as a worktree".into());
    }
    let main = root(g)?;
    let g = Git::new(main.to_str().ok_or("The main worktree path is not UTF-8")?)?;
    let mut argv = vec!["worktree", "remove"];
    if args["force"] == true {
        argv.push("--force");
    }
    argv.push(&requested);
    g.bytes(&argv)?;
    Ok(json!({"removed":requested,"repoRoot":main}))
}
