use super::*;
use std::io::Read;
#[derive(Clone)]
pub struct Changed {
    pub path: String,
    pub x: u8,
    pub y: u8,
    pub old: Option<String>,
}
pub fn changed(g: &Git) -> Result<Vec<Changed>, String> {
    let data = g.bytes(&["status", "--porcelain=v1", "-z", "--untracked-files=normal"])?;
    let mut fields = data.split(|b| *b == 0);
    let mut out = vec![];
    while let Some(record) = fields.next() {
        if record.len() < 4 {
            continue;
        }
        let x = record[0];
        let y = record[1];
        let path = String::from_utf8(record[3..].to_vec())
            .map_err(|_| "Git contains a filename this desktop cannot represent as UTF-8")?;
        let old = if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            fields
                .next()
                .map(|p| {
                    String::from_utf8(p.to_vec())
                        .map_err(|_| "Git contains a non-UTF-8 original path")
                })
                .transpose()?
        } else {
            None
        };
        out.push(Changed { path, x, y, old });
    }
    Ok(out)
}
fn nums(g: &Git, args: &[&str]) -> Result<BTreeMap<String, (u64, u64)>, String> {
    let mut argv = vec!["diff", "--no-ext-diff", "--no-textconv", "--numstat", "-z"];
    argv.extend_from_slice(args);
    let bytes = g.bytes(&argv)?;
    let mut parts = bytes.split(|b| *b == 0);
    let mut result = BTreeMap::new();
    while let Some(row) = parts.next() {
        let mut fields = row.splitn(3, |b| *b == b'\t');
        let Some(a) = fields.next() else {
            continue;
        };
        let Some(r) = fields.next() else {
            continue;
        };
        let Some(path) = fields.next() else {
            continue;
        };
        let path = if path.is_empty() {
            parts.next();
            parts.next().unwrap_or_default()
        } else {
            path
        };
        let a = std::str::from_utf8(a)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let r = std::str::from_utf8(r)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if !path.is_empty() {
            result.insert(
                String::from_utf8(path.to_vec())
                    .map_err(|_| "Git contains a non-UTF-8 file path")?,
                (a, r),
            );
        }
    }
    Ok(result)
}
fn additions(g: &Git, path: &str) -> u64 {
    let path = g.cwd.join(path);
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 1;
    }
    if !meta.is_file() || meta.len() > 1024 * 1024 {
        return 0;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return 0;
    };
    let mut bytes = vec![];
    if file.take(1024 * 1024 + 1).read_to_end(&mut bytes).is_err()
        || bytes.contains(&0)
        || bytes.len() > 1024 * 1024
    {
        return 0;
    }
    bytes.iter().filter(|b| **b == b'\n').count() as u64
        + u64::from(!bytes.is_empty() && bytes.last() != Some(&b'\n'))
}
fn branch_base(g: &Git) -> Option<String> {
    let mut candidates = vec![];
    if let Some(head) = g.optional(&[
        "symbolic-ref",
        "--quiet",
        "--short",
        "refs/remotes/origin/HEAD",
    ]) {
        candidates.push(head);
    }
    candidates.extend(["origin/main", "origin/master", "main", "master"].map(str::to_owned));
    candidates
        .into_iter()
        .find_map(|r| g.optional(&["merge-base", "HEAD", &r]))
}
pub fn review_list(g: &Git, args: &Value) -> Result<Value, String> {
    if !g.is_repo {
        return Ok(json!({"files":[],"base":null,"is_repo":false}));
    }
    let scope = args["scope"].as_str().unwrap_or("uncommitted");
    let base = if scope == "branch" {
        branch_base(g)
    } else if scope == "lastTurn" {
        args["base"]
            .as_str()
            .filter(|s| !s.starts_with('-') && !s.contains('\0'))
            .map(str::to_owned)
    } else {
        None
    };
    let mut files = vec![];
    if scope == "branch" || scope == "lastTurn" {
        let Some(base) = base.as_ref() else {
            return Ok(json!({"files":[],"base":null,"is_repo":true}));
        };
        let range = if scope == "branch" {
            format!("{base}...HEAD")
        } else {
            base.clone()
        };
        for (path, (added, removed)) in nums(g, &[&range])?.into_iter().take(2000) {
            files.push(
                json!({"path":path,"added":added,"removed":removed,"status":"M","staged":false}),
            );
        }
        if scope == "lastTurn" {
            for f in changed(g)?.into_iter().filter(|f| f.x == b'?') {
                if files.len() >= 2000 {
                    break;
                }
                if !files.iter().any(|r| r["path"] == f.path) {
                    let added = additions(g, &f.path);
                    files.push(json!({"path":f.path,"added":added,"removed":0,"status":"?","staged":false}));
                }
            }
        }
    } else {
        let staged = nums(g, &["--cached"])?;
        let unstaged = nums(g, &[])?;
        for f in changed(g)?.into_iter().take(2000) {
            let a = staged.get(&f.path).copied().unwrap_or_default();
            let b = unstaged.get(&f.path).copied().unwrap_or_default();
            let added = if f.x == b'?' {
                additions(g, &f.path)
            } else {
                a.0.saturating_add(b.0)
            };
            let status = if f.x == b'?' {
                b'?'
            } else if f.x != b' ' {
                f.x
            } else {
                f.y
            };
            files.push(json!({"path":f.path,"added":added,"removed":a.1.saturating_add(b.1),"status":(status as char).to_string(),"staged":f.x!=b' '&&f.x!=b'?'}));
        }
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(json!({"files":files,"base":base,"is_repo":true}))
}
pub fn repo_status(g: &Git) -> Result<Value, String> {
    if !g.is_repo {
        return Ok(Value::Null);
    }
    let rows = changed(g)?;
    let branch = g.branch();
    let mut ahead = 0;
    let mut behind = 0;
    if let Some(v) = g.optional(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) {
        let mut parts = v.split_whitespace().filter_map(|s| s.parse::<u64>().ok());
        ahead = parts.next().unwrap_or(0);
        behind = parts.next().unwrap_or(0);
    }
    let files=rows.iter().map(|f|json!({"path":f.path,"staged":f.x!=b' '&&f.x!=b'?',"unstaged":f.y!=b' '&&f.y!=b'?',"untracked":f.x==b'?',"conflicted":f.x==b'U'||f.y==b'U'})).collect::<Vec<_>>();
    let counts = if g.head().is_some() {
        nums(g, &["HEAD"])?
    } else {
        BTreeMap::new()
    };
    let added = counts.values().map(|v| v.0).sum::<u64>();
    let removed = counts.values().map(|v| v.1).sum::<u64>();
    Ok(
        json!({"branch":branch,"defaultBranch":g.default_branch(),"detached":branch.is_none(),"ahead":ahead,"behind":behind,"staged":files.iter().filter(|f|f["staged"]==true).count(),"unstaged":files.iter().filter(|f|f["unstaged"]==true).count(),"untracked":files.iter().filter(|f|f["untracked"]==true).count(),"conflicted":files.iter().filter(|f|f["conflicted"]==true).count(),"changed":files.len(),"added":added,"removed":removed,"files":files.into_iter().take(200).collect::<Vec<_>>(),"repoRoot":g.cwd.to_string_lossy(),"head":g.head()}),
    )
}
pub fn diff(g: &Git, args: &Value) -> Result<Value, String> {
    if !g.is_repo {
        return Ok(json!({"diff":""}));
    }
    let Some(path) = file(args)? else {
        return Err("Choose a file to review".into());
    };
    let scope = args["scope"].as_str().unwrap_or("uncommitted");
    let mut argv = vec!["diff", "--no-ext-diff", "--no-textconv"];
    let range;
    if scope == "branch" {
        let Some(base) = branch_base(g) else {
            return Ok(json!({"diff":""}));
        };
        range = format!("{base}...HEAD");
        argv.push(&range);
    } else if scope == "lastTurn" {
        let Some(base) = args["base"]
            .as_str()
            .filter(|s| !s.starts_with('-') && !s.contains('\0'))
        else {
            return Ok(json!({"diff":""}));
        };
        argv.push(base);
    } else if args["staged"] == true {
        argv.push("--cached");
    } else if args["scope"].is_null() {
        argv.push("HEAD");
    }
    argv.extend(["--", path]);
    let text = g.output(&argv)?;
    if text.code == 0 && !text.stdout.is_empty() {
        return Ok(json!({"diff":String::from_utf8_lossy(&text.stdout)}));
    }
    if scope == "branch" || scope == "lastTurn" || args["staged"] == true {
        return if text.code == 0 {
            Ok(json!({"diff":""}))
        } else {
            Err(text.stderr)
        };
    }
    let untracked = changed(g)?.iter().any(|f| f.path == path && f.x == b'?');
    if !untracked {
        return Ok(json!({"diff":""}));
    }
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let result = g.output(&[
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-index",
        "--",
        null,
        path,
    ])?;
    if ![0, 1].contains(&result.code) {
        return Err(result.stderr);
    }
    Ok(json!({"diff":String::from_utf8_lossy(&result.stdout)}))
}
pub fn mutate(g: &Git, route: &str, args: &Value) -> Result<Value, String> {
    if !g.is_repo {
        return Err("The selected workspace is not a Git repository".into());
    }
    let path = file(args)?;
    match route {
        "review/stage" => {
            if let Some(path) = path {
                g.bytes(&["add", "--", path])?;
            } else {
                g.bytes(&["add", "-A"])?;
            }
        }
        "review/unstage" => {
            if g.head().is_some() {
                if let Some(path) = path {
                    g.bytes(&["reset", "-q", "HEAD", "--", path])?;
                } else {
                    g.bytes(&["reset", "-q", "HEAD"])?;
                }
            } else if let Some(path) = path {
                g.bytes(&["rm", "--cached", "-r", "--ignore-unmatch", "--", path])?;
            } else {
                g.bytes(&["rm", "--cached", "-r", "--ignore-unmatch", "--", "."])?;
            }
        }
        "review/revert" => {
            let target = path.unwrap_or(".");
            let rows = changed(g)?;
            let tracked = if let Some(path) = path {
                rows.iter().any(|f| f.path == path && f.x != b'?')
            } else {
                rows.iter().any(|f| f.x != b'?')
            };
            if tracked {
                if g.head().is_none() {
                    return Err("There is no committed baseline to restore. Unstage these initial files first.".into());
                }
                let old = path
                    .and_then(|p| rows.iter().find(|f| f.path == p))
                    .and_then(|f| f.old.as_deref());
                let mut argv = vec![
                    "restore",
                    "--source=HEAD",
                    "--staged",
                    "--worktree",
                    "--",
                    target,
                ];
                if let Some(old) = old {
                    argv.push(old);
                }
                g.bytes(&argv)?;
            }
            g.bytes(&["clean", "-fd", "--", target])?;
        }
        _ => return Err("Unsupported Review change".into()),
    }
    Ok(json!({"ok":true}))
}
