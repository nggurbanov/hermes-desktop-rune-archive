use super::*;
pub fn context(g: &Git) -> Result<Value, String> {
    if !g.is_repo {
        return Ok(json!({"diff":"","recent":""}));
    }
    let staged = status::changed(g)?
        .iter()
        .any(|f| f.x != b' ' && f.x != b'?');
    let args = if staged {
        vec!["diff", "--no-ext-diff", "--no-textconv", "--cached"]
    } else if g.head().is_some() {
        vec!["diff", "--no-ext-diff", "--no-textconv", "HEAD"]
    } else {
        vec!["diff", "--no-ext-diff", "--no-textconv", "--cached"]
    };
    let mut diff = g.text(&args)?;
    if diff.chars().count() > 120_000 {
        diff = diff.chars().take(120_000).collect::<String>();
        diff.push_str("\n# diff truncated for commit-message generation\n");
    }
    let untracked = status::changed(g)?
        .into_iter()
        .filter(|f| f.x == b'?')
        .collect::<Vec<_>>();
    if !untracked.is_empty() {
        diff.push_str("\n# New (untracked) files:\n");
        for f in untracked.iter().take(80) {
            diff.push_str(&format!("#   {}\n", f.path));
        }
        if untracked.len() > 80 {
            diff.push_str(&format!("#   ... {} more omitted\n", untracked.len() - 80));
        }
    }
    let recent = g
        .optional(&["log", "-n", "10", "--pretty=format:%s"])
        .unwrap_or_default();
    Ok(json!({"diff":diff,"recent":recent}))
}
pub fn push(g: &Git) -> Result<Value, String> {
    if !g.is_repo {
        return Err("The workspace is not a repository".into());
    }
    if g.ok(&["rev-parse", "--verify", "@{upstream}"]) {
        g.bytes(&["push"])?;
    } else {
        let branch = g
            .branch()
            .ok_or("A detached checkout cannot be pushed without choosing a branch")?;
        if !g.ref_valid(&branch) {
            return Err("The current branch is invalid".into());
        }
        g.bytes(&["push", "-u", "origin", &branch])?;
    }
    Ok(json!({"ok":true}))
}
pub fn commit(g: &Git, args: &Value) -> Result<Value, String> {
    let message = text(args, "message");
    if message.is_empty() {
        return Err("Enter a commit message".into());
    }
    if !g.is_repo {
        return Err("The workspace is not a Git repository".into());
    }
    if !status::changed(g)?
        .iter()
        .any(|f| f.x != b' ' && f.x != b'?')
    {
        g.bytes(&["add", "-A"])?;
    }
    g.bytes(&["commit", "-m", &message])?;
    let head = g.head();
    if args["push"] == true {
        if let Err(error) = push(g) {
            return Ok(
                json!({"ok":false,"committed":true,"commit":head,"error":format!("Commit created; push failed: {error}")}),
            );
        }
    }
    Ok(json!({"ok":true,"committed":true,"commit":head}))
}
fn gh(g: &Git, args: &[&str]) -> Result<process::Output, String> {
    let binary = process::binary("gh")?;
    run_program(&binary, args, &g.cwd)
}
pub fn ship_info(g: &Git) -> Result<Value, String> {
    if !g.is_repo {
        return Ok(json!({"ghReady":false,"pr":null}));
    }
    if !gh(g, &["auth", "status"]).is_ok_and(|r| r.code == 0) {
        return Ok(json!({"ghReady":false,"pr":null}));
    }
    let pr = gh(g, &["pr", "view", "--json", "url,state,number"])?;
    let value = if pr.code == 0 {
        serde_json::from_slice::<Value>(&pr.stdout)
            .ok()
            .filter(|v| v["url"].is_string())
    } else {
        None
    };
    Ok(json!({"ghReady":true,"pr":value}))
}
pub fn create_pr(g: &Git) -> Result<Value, String> {
    if let Err(error) = push(g) {
        let branch = g.branch().ok_or_else(|| error.clone())?;
        let head = g.head().ok_or_else(|| error.clone())?;
        let remote = g.optional(&[
            "ls-remote",
            "--heads",
            "origin",
            &format!("refs/heads/{branch}"),
        ]);
        if !remote
            .as_ref()
            .is_some_and(|s| s.split_whitespace().next() == Some(head.as_str()))
        {
            return Err(format!(
                "The branch was not confirmed on the remote. {error}"
            ));
        }
    }
    let result = gh(g, &["pr", "create", "--fill"])?;
    if result.code != 0 {
        return Err(format!("gh pr create failed: {}", result.stderr));
    }
    let text = String::from_utf8_lossy(&result.stdout);
    let url = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .next_back()
        .unwrap_or("")
        .trim();
    if !url::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https")) {
        return Err("The PR command did not return a valid URL; inspect the remote before retrying creation".into());
    }
    Ok(json!({"url":url}))
}
pub fn pr_list(g: &Git, args: &Value) -> Result<Value, String> {
    let wanted = args["branches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(300)
        .collect::<Vec<_>>();
    let numbers = args["numbers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .filter(|n| *n > 0)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(300)
        .collect::<Vec<_>>();
    if wanted.is_empty() && numbers.is_empty() {
        return Ok(json!({"ghReady":false,"prs":[]}));
    }
    let repo = match gh(
        g,
        &[
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "-q",
            ".nameWithOwner",
        ],
    ) {
        Ok(v) if v.code == 0 => String::from_utf8_lossy(&v.stdout).trim().to_owned(),
        _ => return Ok(json!({"ghReady":false,"prs":[]})),
    };
    let Some((owner, name)) = repo
        .split_once('/')
        .filter(|(o, n)| !o.is_empty() && !n.is_empty())
    else {
        return Ok(json!({"ghReady":false,"prs":[]}));
    };
    let mut chunks = vec![];
    for chunk in wanted.chunks(50) {
        chunks.push((chunk.to_vec(), vec![]));
    }
    for chunk in numbers.chunks(50) {
        chunks.push((vec![], chunk.to_vec()));
    }
    let mut prs = vec![];
    let mut errors = vec![];
    for (branches, numbers) in chunks {
        let mut fields = vec![];
        const NODE: &str = "number state isDraft isCrossRepository title url headRefName";
        for (i, b) in branches.iter().enumerate() {
            fields.push(format!("b{i}: pullRequests(headRefName: {}, first: 5, orderBy: {{field: CREATED_AT, direction: DESC}}) {{nodes {{{NODE}}}}}",serde_json::to_string(b).unwrap()));
        }
        for (i, n) in numbers.iter().enumerate() {
            fields.push(format!("n{i}: pullRequest(number: {n}) {{{NODE}}}"));
        }
        let query = format!(
            "query {{ repository(owner: {}, name: {}) {{{}}} }}",
            serde_json::to_string(owner).unwrap(),
            serde_json::to_string(name).unwrap(),
            fields.join("\n")
        );
        let query = format!("query={query}");
        let result = gh(g, &["api", "graphql", "-f", &query])?;
        if result.code != 0 {
            errors.push(result.stderr);
            continue;
        }
        let data: Value = match serde_json::from_slice(&result.stdout) {
            Ok(v) => v,
            Err(_) => {
                errors.push("The PR query returned malformed data".into());
                continue;
            }
        };
        for (key, value) in data["data"]["repository"].as_object().into_iter().flatten() {
            let node = if key.starts_with('n') {
                Some(value)
            } else {
                value["nodes"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|p| p["isCrossRepository"] == false))
            };
            if let Some(node) = node.filter(|v| v["headRefName"].is_string()) {
                prs.push(json!({"branch":node["headRefName"],"draft":node["isDraft"],"number":node["number"],"state":node["state"].as_str().unwrap_or("").to_lowercase(),"title":node["title"],"url":node["url"]}));
            }
        }
    }
    Ok(json!({"ghReady":true,"prs":prs,"partial":!errors.is_empty(),"errors":errors}))
}
