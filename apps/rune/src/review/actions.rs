use super::*;
#[derive(Clone)]
pub enum Action {
    Refresh,
    Scope(String),
    Tree,
    Collapse(String),
    Select(String),
    ClearSelection,
    Stage(Option<String>),
    Unstage(Option<String>),
    AskRevert(Option<String>),
    Confirm,
    Cancel,
    Commit(bool),
    Push,
    CreatePr,
    Generate,
    CancelGenerate,
    AgentShip,
    SetCommitPush(bool),
    SwitchBranch(String),
    AddWorktree,
    ExistingWorktree(String),
    PickWorktree(String),
    AskRemoveWorktree(String, bool),
    DiscardMessage,
    AcknowledgeCommit,
    ToggleWorktrees,
    Close,
    OpenFile(String, bool),
    Download(String, std::path::PathBuf),
}
impl Review {
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.demo
            && !matches!(
                a,
                Action::Scope(_)
                    | Action::Tree
                    | Action::Collapse(_)
                    | Action::ClearSelection
                    | Action::Cancel
                    | Action::DiscardMessage
                    | Action::SetCommitPush(_)
                    | Action::ToggleWorktrees
                    | Action::Close
            )
        {
            return vec![];
        }
        if self.busy && !matches!(a, Action::Cancel | Action::ClearSelection) {
            return vec![];
        }
        self.error = None;
        let staging = matches!(&a, Action::Stage(_));
        match a {
            Action::Close => {
                if !self.busy {
                    self.close_requested = true;
                }
                vec![]
            }
            Action::ToggleWorktrees => {
                self.show_worktrees = !self.show_worktrees;
                vec![]
            }
            Action::OpenFile(path, in_tree) => {
                if self.files.iter().any(|f| f.path == path) {
                    if let Some(owner) = self.owner.clone() {
                        let root = self
                            .status
                            .as_ref()
                            .and_then(|s| s["repoRoot"].as_str())
                            .unwrap_or(&self.root)
                            .to_owned();
                        let full = format!("{}/{}", root.trim_end_matches(['/', '\\']), path);
                        self.file_request = Some(ReviewFileRequest {
                            owner,
                            root,
                            path: full,
                            in_tree,
                        });
                    }
                }
                vec![]
            }
            Action::Download(path, destination) => {
                if self.downloading || self.local || !self.files.iter().any(|f| f.path == path) {
                    return vec![];
                }
                let Some(owner) = self.owner.clone() else {
                    return vec![];
                };
                self.downloading = true;
                let full = format!("{}/{}", self.root.trim_end_matches(['/', '\\']), path);
                let mut request = PageRequest::get("/api/fs/download")
                    .query("path", full)
                    .profile(&owner.profile)
                    .download_to(destination)
                    .timeout_secs(3600);
                if let Some(session) = owner.session {
                    request = request.query("session_id", session);
                }
                vec![("review.download".into(), request)]
            }
            Action::Refresh => self.load(),
            Action::Scope(scope) => {
                if !["uncommitted", "branch", "lastTurn"].contains(&scope.as_str()) {
                    return vec![];
                }
                self.scope = scope;
                self.selected = None;
                self.diff.clear();
                self.prefs_changed = true;
                self.refresh()
            }
            Action::Tree => {
                self.tree = !self.tree;
                self.prefs_changed = true;
                vec![]
            }
            Action::Collapse(path) => {
                if !self.collapsed.remove(&path) {
                    self.collapsed.insert(path);
                }
                vec![]
            }
            Action::Select(path) => {
                if !self.files.iter().any(|f| f.path == path) {
                    return vec![];
                }
                self.selected = Some(path);
                self.diff.clear();
                self.prefs_changed = true;
                self.read_diff()
            }
            Action::ClearSelection => {
                self.selected = None;
                self.diff.clear();
                self.diff_loading = false;
                self.pending.remove("review.diff");
                self.prefs_changed = true;
                vec![]
            }
            Action::Stage(path) | Action::Unstage(path) => {
                if !self.can_mutate()
                    || path
                        .as_ref()
                        .is_some_and(|p| !self.files.iter().any(|f| f.path == *p))
                {
                    return vec![];
                }
                let route = if staging {
                    "review/stage"
                } else {
                    "review/unstage"
                };
                self.busy = true;
                self.request("review.mutation", route, true, json!({"file":path}))
            }
            Action::AskRevert(path) => {
                if self.can_mutate()
                    && !self.files.is_empty()
                    && !path
                        .as_ref()
                        .is_some_and(|p| !self.files.iter().any(|f| f.path == *p))
                {
                    self.confirm = Some(Confirmation::Revert(path));
                }
                vec![]
            }
            Action::Confirm => match self.confirm.take() {
                Some(Confirmation::Revert(path)) => {
                    if self.scope != "uncommitted" {
                        return vec![];
                    }
                    self.busy = true;
                    self.request(
                        "review.mutation",
                        "review/revert",
                        true,
                        json!({"file":path}),
                    )
                }
                Some(Confirmation::RemoveWorktree(path, force)) => {
                    self.busy = true;
                    self.request(
                        "review.remove-worktree",
                        "worktree/remove",
                        true,
                        json!({"worktreePath":path,"force":force}),
                    )
                }
                None => vec![],
            },
            Action::Cancel => {
                self.confirm = None;
                self.open = None;
                vec![]
            }
            Action::Commit(push) => {
                if !self.can_mutate()
                    || self.message.text.trim().is_empty()
                    || self.files.is_empty()
                    || self.mutation_uncertain
                    || self.generating
                {
                    return vec![];
                }
                self.commit_push = push;
                self.prefs_changed = true;
                self.busy = true;
                self.commit_before = self.head.clone();
                self.request(
                    "review.commit",
                    "review/commit",
                    true,
                    json!({"message":self.message.text.trim(),"push":push}),
                )
            }
            Action::Push => {
                if self.scope != "uncommitted" {
                    return vec![];
                }
                self.busy = true;
                self.request("review.push", "review/push", true, json!({}))
            }
            Action::CreatePr => {
                if self.scope != "uncommitted" {
                    return vec![];
                }
                if let Some(url) = self.ship.as_ref().and_then(|s| s["pr"]["url"].as_str()) {
                    self.url = Some(url.into());
                    return vec![];
                }
                if self.ship.as_ref().is_none_or(|s| s["ghReady"] != true) {
                    self.error = Some(
                        "Install GitHub CLI and sign in on the workspace host before creating a PR"
                            .into(),
                    );
                    return vec![];
                }
                self.busy = true;
                self.request("review.pr-create", "review/create-pr", true, json!({}))
            }
            Action::Generate => {
                if self.generating || self.files.is_empty() {
                    return vec![];
                }
                self.generating = true;
                self.generation_previous = self.message.text.clone();
                self.request("review.context", "review/commit-context", false, json!({}))
            }
            Action::CancelGenerate => {
                self.generating = false;
                self.pending.remove("review.context");
                self.pending.remove("review.generate");
                vec![]
            }
            Action::AgentShip => {
                let Some(owner) = self.owner.clone().filter(|o| o.session.is_some()) else {
                    self.error=Some("Open the conversation that owns this workspace before asking Hermes to ship it".into());
                    return vec![];
                };
                self.agent_ship=Some((owner,self.root.clone(),"Review the current changes, commit them with a clear conventional-commit message, push the branch, and open a pull request.".into()));
                vec![]
            }
            Action::SetCommitPush(push) => {
                self.commit_push = push;
                self.prefs_changed = true;
                vec![]
            }
            Action::SwitchBranch(branch) => {
                if !self.branches.iter().any(|b| b["name"] == branch) {
                    return vec![];
                }
                self.busy = true;
                self.request(
                    "review.branch-switch",
                    "branch/switch",
                    true,
                    json!({"branch":branch}),
                )
            }
            Action::AddWorktree => {
                self.busy = true;
                self.request("review.add-worktree","worktree/add",true,json!({"name":self.name.text.trim(),"branch":self.branch.text.trim(),"base":self.source.text.trim()}))
            }
            Action::ExistingWorktree(branch) => {
                let Some(row) = self.branches.iter().find(|b| b["name"] == branch) else {
                    return vec![];
                };
                if let Some(path) = row["worktreePath"].as_str() {
                    if let Some(owner) = self.owner.clone() {
                        self.workspace = Some((owner, path.into()));
                    }
                    return vec![];
                }
                self.busy = true;
                self.request(
                    "review.add-worktree",
                    "worktree/add",
                    true,
                    json!({"existingBranch":branch}),
                )
            }
            Action::PickWorktree(path) => {
                if self.worktrees.iter().any(|w| w["path"] == path) {
                    if let Some(owner) = self.owner.clone() {
                        self.workspace = Some((owner, path));
                    }
                }
                vec![]
            }
            Action::AskRemoveWorktree(path, force) => {
                if self
                    .worktrees
                    .iter()
                    .any(|w| w["path"] == path && w["isMain"] != true)
                {
                    self.confirm = Some(Confirmation::RemoveWorktree(path, force));
                }
                vec![]
            }
            Action::DiscardMessage => {
                self.generating = false;
                self.pending.remove("review.context");
                self.pending.remove("review.generate");
                self.message.replace_text("");
                self.mutation_uncertain = false;
                vec![]
            }
            Action::AcknowledgeCommit => {
                self.mutation_uncertain = false;
                vec![]
            }
        }
    }
}
impl crate::app::Viewer {
    pub fn review_action(&mut self, a: Action) {
        match &a {
            Action::Scope(_) => {
                self.invalidate_page_requests("review.list");
                self.invalidate_page_requests("review.diff");
            }
            Action::Select(_) | Action::ClearSelection => {
                self.invalidate_page_requests("review.diff")
            }
            Action::CancelGenerate | Action::DiscardMessage => {
                self.invalidate_page_requests("review.context");
                self.invalidate_page_requests("review.generate");
            }
            _ => {}
        }
        if matches!(a, Action::Generate) {
            self.review.runtime_session = self
                .review
                .owner
                .as_ref()
                .and_then(|o| self.conversation.view(o))
                .map(|v| v.runtime.clone());
        }
        let effects = self.review.act(a);
        self.page_requests(effects);
    }
}
