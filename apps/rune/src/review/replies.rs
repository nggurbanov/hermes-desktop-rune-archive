use super::*;
fn excluded(path: &str) -> bool {
    path.split(['/', '\\']).any(|p| {
        [
            ".git",
            ".hg",
            ".svn",
            "node_modules",
            "bower_components",
            ".venv",
            "venv",
            "env",
            "__pycache__",
            ".mypy_cache",
            ".pytest_cache",
            ".ruff_cache",
            ".tox",
            ".gradle",
            ".idea",
            "dist",
            "build",
            "out",
            "target",
            "vendor",
            "Pods",
            ".next",
            ".nuxt",
            ".svelte-kit",
            ".output",
            ".turbo",
            ".parcel-cache",
            ".cache",
            ".terraform",
            ".expo",
            ".angular",
            "coverage",
            ".DS_Store",
            "Thumbs.db",
        ]
        .contains(&p)
    })
}
impl Review {
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        if op == "review.capture" {
            if let Some((owner, cwd)) = self.capture.take() {
                if let Ok(v) = result {
                    if let Some(sha) = v.as_str().or(v["sha"].as_str()).filter(|s| !s.is_empty()) {
                        let key = Self::key(&owner, &cwd);
                        self.turn_order.retain(|k| k != &key);
                        self.turn_order.push_back(key.clone());
                        self.turn_bases.insert(key, sha.into());
                        while self.turn_order.len() > 8 {
                            if let Some(old) = self.turn_order.pop_front() {
                                self.turn_bases.remove(&old);
                            }
                        }
                    }
                }
            }
            return vec![];
        }
        let v = match result {
            Ok(v) => v,
            Err(error) => {
                match op {
                    "review.download" => self.downloading = false,
                    "review.list" => self.loading = false,
                    "review.diff" => self.diff_loading = false,
                    "review.context" | "review.generate" => self.generating = false,
                    "review.mutation"
                    | "review.commit"
                    | "review.push"
                    | "review.pr-create"
                    | "review.branch-switch"
                    | "review.add-worktree"
                    | "review.remove-worktree" => self.busy = false,
                    _ => {}
                }
                if [
                    "review.list",
                    "review.diff",
                    "review.status",
                    "review.ship",
                    "review.branches",
                    "review.bases",
                    "review.worktrees",
                    "review.head",
                ]
                .contains(&op)
                {
                    self.read_errors.insert(op.into(), error);
                } else {
                    self.error = Some(error);
                }
                if op == "review.commit" {
                    self.mutation_uncertain = true;
                    return self.request(
                        "review.commit-reconcile",
                        "review/rev-parse",
                        false,
                        json!({"ref":"HEAD"}),
                    );
                }
                return vec![];
            }
        };
        self.read_errors.remove(op);
        match op {
            "review.list" => {
                self.loading = false;
                let Some(files) = v["files"].as_array() else {
                    self.error = Some("The Git host returned an invalid changed-file list".into());
                    return vec![];
                };
                let mut parsed = vec![];
                for row in files.iter().take(2000) {
                    let f: File = match serde_json::from_value(row.clone()) {
                        Ok(f) => f,
                        Err(_) => {
                            self.error = Some("The Git host returned an invalid file row".into());
                            return vec![];
                        }
                    };
                    if !f.path.is_empty() && !excluded(&f.path) {
                        parsed.push(f);
                    }
                }
                self.files = parsed;
                self.is_repo = v["is_repo"].as_bool() != Some(false);
                self.base = v["base"].as_str().map(str::to_owned);
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|p| !self.files.iter().any(|f| &f.path == p))
                {
                    self.selected = None;
                    self.diff.clear();
                    self.prefs_changed = true;
                }
                if self.selected.is_some() {
                    return self.read_diff();
                }
            }
            "review.download" => {
                self.downloading = false;
                if v["ok"] == true && v["downloaded"] == true {
                    self.notice = Some("File downloaded to this device".into());
                } else {
                    self.error = Some("The download was not confirmed".into());
                }
            }
            "review.status" => {
                self.is_repo = v.is_object();
                self.status = if v.is_object() { Some(v) } else { None };
            }
            "review.ship" => {
                if v["ghReady"].is_boolean() {
                    self.ship = Some(v);
                } else {
                    self.error = Some("The Git host returned invalid PR readiness".into());
                }
            }
            "review.diff" => {
                self.diff_loading = false;
                if let Some(diff) = v.as_str().or(v["diff"].as_str()) {
                    self.diff = diff.into();
                } else {
                    self.error = Some("The Git host returned an invalid diff".into());
                }
            }
            "review.head" => self.head = v.as_str().or(v["sha"].as_str()).map(str::to_owned),
            "review.branches" => self.branches = rows(&v, "branches"),
            "review.bases" => self.bases = rows(&v, "branches"),
            "review.worktrees" => self.worktrees = rows(&v, "worktrees"),
            "review.mutation" | "review.push" => {
                self.busy = false;
                if v["ok"] != true {
                    self.error = Some(
                        v["error"]
                            .as_str()
                            .unwrap_or("The Git host did not confirm the change")
                            .into(),
                    );
                    return vec![];
                }
                self.notice = Some("Git operation completed".into());
                return if op == "review.push" {
                    self.load()
                } else {
                    self.refresh()
                };
            }
            "review.commit" => {
                self.busy = false;
                if v["ok"] != true {
                    if v["committed"] == true && v["commit"].is_string() {
                        self.message.replace_text("");
                        self.head = v["commit"].as_str().map(str::to_owned);
                        self.notice =
                            Some("The commit was created, but publishing did not complete".into());
                        self.error = Some(v["error"].as_str().unwrap_or("Push failed").into());
                        return self.load();
                    }
                    self.error = Some(
                        v["error"]
                            .as_str()
                            .unwrap_or("The commit was not confirmed")
                            .into(),
                    );
                    return vec![];
                }
                self.message.replace_text("");
                self.mutation_uncertain = false;
                self.notice = Some("Commit completed".into());
                return self.load();
            }
            "review.commit-reconcile" => {
                let now = v.as_str().or(v["sha"].as_str()).map(str::to_owned);
                self.error=Some(if now!=self.commit_before{"HEAD changed after the interrupted commit request. A commit may already exist; inspect history before allowing another commit."}else{"The commit outcome was not confirmed. Review the operation before allowing another attempt."}.into());
                self.head = now;
                return self.refresh();
            }
            "review.context" => {
                if !self.generating {
                    return vec![];
                }
                let Some(diff) = v["diff"].as_str().filter(|s| !s.trim().is_empty()) else {
                    self.generating = false;
                    self.notice = Some("There is no commit diff to summarize".into());
                    return vec![];
                };
                let Some(owner) = &self.owner else {
                    self.generating = false;
                    return vec![];
                };
                return vec![("review.generate".into(),PageRequest::rpc("llm.oneshot",json!({"template":"commit_message","temperature":0.8,"session_id":self.runtime_session,"variables":{"avoid":self.generation_previous,"diff":diff,"recent_commits":v["recent"]}})).profile(&owner.profile).timeout_secs(180))];
            }
            "review.generate" => {
                if !self.generating {
                    return vec![];
                }
                self.generating = false;
                if let Some(message) = v["text"].as_str().filter(|s| !s.trim().is_empty()) {
                    self.message.replace_text(message.trim());
                } else {
                    self.error = Some(
                        v["error"]
                            .as_str()
                            .unwrap_or("The agent did not return a generated commit message")
                            .into(),
                    );
                }
            }
            "review.pr-create" => {
                self.busy = false;
                let Some(url) = v["url"].as_str().filter(|s| {
                    url::Url::parse(s).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
                }) else {
                    self.error=Some("The PR result did not include a valid URL. Inspect the remote before retrying creation.".into());
                    return vec![];
                };
                self.url = Some(url.into());
                self.notice = Some("Pull request created".into());
                return self.load();
            }
            "review.branch-switch" => {
                self.busy = false;
                if !v["branch"].is_string() {
                    self.error = Some("The Git host did not confirm the branch switch".into());
                    return vec![];
                }
                self.notice = Some("Branch switched".into());
                return self.load();
            }
            "review.add-worktree" => {
                self.busy = false;
                if let Some(path) = v["path"].as_str() {
                    if let Some(owner) = self.owner.clone() {
                        self.workspace = Some((owner, path.into()));
                    }
                    self.name.replace_text("");
                    self.branch.replace_text("");
                    self.notice = Some("Worktree is ready".into());
                    return self.load();
                }
                self.error = Some("The Git host did not confirm the worktree location".into());
            }
            "review.remove-worktree" => {
                self.busy = false;
                let Some(removed) = v["removed"].as_str() else {
                    self.error = Some("The Git host did not confirm worktree removal".into());
                    return vec![];
                };
                if self.root == removed || self.root.starts_with(&format!("{removed}/")) {
                    if let (Some(owner), Some(main)) = (self.owner.clone(), v["repoRoot"].as_str())
                    {
                        self.workspace = Some((owner, main.into()));
                    }
                }
                self.notice = Some("Worktree removed; its branch is preserved".into());
                return self.load();
            }
            _ => {}
        }
        vec![]
    }
}
fn rows(v: &Value, key: &str) -> Vec<Value> {
    v.as_array()
        .or_else(|| v[key].as_array())
        .cloned()
        .unwrap_or_default()
}
