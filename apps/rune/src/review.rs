//! Review follows an explicit conversation/workspace owner; remote Git never runs locally.
mod actions;
mod git;
mod replies;
#[cfg(test)]
mod tests;
use crate::{
    composer::DraftScope,
    pages::{PageEffect, PageRequest},
};
pub use actions::Action;
use rune::TextEdit;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct File {
    pub path: String,
    pub added: u64,
    pub removed: u64,
    pub status: String,
    pub staged: bool,
}
#[derive(Clone)]
pub enum Confirmation {
    Revert(Option<String>),
    RemoveWorktree(String, bool),
}
pub struct ReviewFileRequest {
    pub owner: DraftScope,
    pub root: String,
    pub path: String,
    pub in_tree: bool,
}
pub struct Review {
    pub owner: Option<DraftScope>,
    pub local: bool,
    pub root: String,
    pub scope: String,
    pub base: Option<String>,
    pub head: Option<String>,
    pub files: Vec<File>,
    pub selected: Option<String>,
    pub diff: String,
    pub status: Option<Value>,
    pub ship: Option<Value>,
    pub branches: Vec<Value>,
    pub bases: Vec<Value>,
    pub worktrees: Vec<Value>,
    pub message: TextEdit,
    pub name: TextEdit,
    pub branch: TextEdit,
    pub source: TextEdit,
    pub open: Option<String>,
    pub tree: bool,
    pub collapsed: BTreeSet<String>,
    pub commit_push: bool,
    pub generating: bool,
    pub loading: bool,
    pub diff_loading: bool,
    pub busy: bool,
    pub is_repo: bool,
    pub demo: bool,
    pub read_errors: BTreeMap<String, String>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub confirm: Option<Confirmation>,
    pub runtime_session: Option<String>,
    pub mutation_uncertain: bool,
    pub show_worktrees: bool,
    pub downloading: bool,
    file_request: Option<ReviewFileRequest>,
    close_requested: bool,
    prefs_changed: bool,
    worker: Option<git::Worker>,
    epoch: u64,
    serial: u64,
    pending: BTreeMap<String, u64>,
    turn_bases: BTreeMap<String, String>,
    turn_order: VecDeque<String>,
    capture: Option<(DraftScope, String)>,
    workspace: Option<(DraftScope, String)>,
    agent_ship: Option<(DraftScope, String, String)>,
    url: Option<String>,
    detached: TextEdit,
    redraw: bool,
    generation_previous: String,
    commit_before: Option<String>,
}
impl Default for Review {
    fn default() -> Self {
        Self::new()
    }
}
impl Review {
    pub fn new() -> Self {
        Self {
            owner: None,
            local: false,
            root: String::new(),
            scope: "uncommitted".into(),
            base: None,
            head: None,
            files: vec![],
            selected: None,
            diff: String::new(),
            status: None,
            ship: None,
            branches: vec![],
            bases: vec![],
            worktrees: vec![],
            message: TextEdit::new(""),
            name: TextEdit::new(""),
            branch: TextEdit::new(""),
            source: TextEdit::new(""),
            open: None,
            tree: true,
            collapsed: BTreeSet::new(),
            commit_push: false,
            generating: false,
            loading: false,
            diff_loading: false,
            busy: false,
            is_repo: false,
            demo: false,
            read_errors: BTreeMap::new(),
            error: None,
            notice: None,
            confirm: None,
            runtime_session: None,
            mutation_uncertain: false,
            show_worktrees: false,
            downloading: false,
            file_request: None,
            close_requested: false,
            prefs_changed: false,
            worker: None,
            epoch: 0,
            serial: 0,
            pending: BTreeMap::new(),
            turn_bases: BTreeMap::new(),
            turn_order: VecDeque::new(),
            capture: None,
            workspace: None,
            agent_ship: None,
            url: None,
            detached: TextEdit::new(""),
            redraw: false,
            generation_previous: String::new(),
            commit_before: None,
        }
    }
    pub fn init(&mut self, waker: rune::Waker) {
        if self.worker.is_none() {
            self.worker = Some(git::Worker::new(waker));
        }
    }
    pub fn reset(&mut self) {
        let worker = self.worker.take();
        let epoch = self.epoch.wrapping_add(1);
        let bases = std::mem::take(&mut self.turn_bases);
        let order = std::mem::take(&mut self.turn_order);
        let prefs = self.preferences();
        *self = Self::new();
        self.worker = worker;
        self.epoch = epoch;
        self.turn_bases = bases;
        self.turn_order = order;
        self.hydrate_preferences(&prefs);
    }
    pub fn preferences(&self) -> Value {
        json!({"scope":self.scope,"tree":self.tree,"commit_push":self.commit_push,"selected":self.selected})
    }
    pub fn hydrate_preferences(&mut self, value: &Value) {
        if let Some(scope) = value["scope"]
            .as_str()
            .filter(|v| ["uncommitted", "branch", "lastTurn"].contains(v))
        {
            self.scope = scope.into();
        }
        if let Some(tree) = value["tree"].as_bool() {
            self.tree = tree;
        }
        if let Some(push) = value["commit_push"].as_bool() {
            self.commit_push = push;
        }
        self.selected = value["selected"]
            .as_str()
            .filter(|s| s.len() <= 4096)
            .map(str::to_owned);
        self.prefs_changed = false;
    }
    pub fn take_preferences(&mut self) -> Option<Value> {
        if std::mem::take(&mut self.prefs_changed) {
            Some(self.preferences())
        } else {
            None
        }
    }
    pub fn refresh(&mut self) -> Vec<PageEffect> {
        if self.demo || self.root.is_empty() {
            return vec![];
        }
        self.loading = true;
        let base = if self.scope == "lastTurn" {
            self.turn_base()
        } else {
            None
        };
        let mut e = self.request(
            "review.list",
            "review/list",
            false,
            json!({"scope":self.scope,"base":base}),
        );
        e.extend(self.request("review.status", "status", false, json!({})));
        e.extend(self.request(
            "review.head",
            "review/rev-parse",
            false,
            json!({"ref":"HEAD"}),
        ));
        e
    }
    pub fn can_mutate(&self) -> bool {
        self.scope == "uncommitted"
            && self.is_repo
            && !self.loading
            && !self.read_errors.contains_key("review.list")
            && !self.read_errors.contains_key("review.status")
    }
    pub fn blocks_owner_change(&self) -> bool {
        self.busy
            || self.downloading
            || self.confirm.is_some()
            || !self.message.text.trim().is_empty()
            || self.mutation_uncertain
    }
    pub fn edit_detached(&mut self) -> &mut TextEdit {
        &mut self.detached
    }
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }
    pub fn revision(&self) -> u64 {
        self.epoch
    }
    pub fn take_close(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }
    pub fn take_file_request(&mut self) -> Option<ReviewFileRequest> {
        self.file_request.take()
    }
    pub fn take_workspace(&mut self) -> Option<(DraftScope, String)> {
        self.workspace.take()
    }
    pub fn take_agent_ship(&mut self) -> Option<(DraftScope, String, String)> {
        self.agent_ship.take()
    }
    pub fn take_url(&mut self) -> Option<String> {
        self.url.take()
    }
    pub fn bind(&mut self, owner: DraftScope, local: bool, cwd: Option<String>) -> Vec<PageEffect> {
        let cwd = cwd.unwrap_or_default().trim().to_owned();
        if self.owner.as_ref() == Some(&owner) && self.root == cwd && self.local == local {
            return vec![];
        }
        if self.blocks_owner_change() {
            self.error=Some("Save or discard the current Review draft before changing its conversation or workspace".into());
            return vec![];
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.pending.retain(|op, _| op == "review.capture");
        self.owner = Some(owner);
        self.local = local;
        self.root = cwd;
        self.files.clear();
        self.diff.clear();
        self.selected = None;
        self.status = None;
        self.ship = None;
        self.confirm = None;
        self.loading = false;
        self.diff_loading = false;
        self.error = None;
        self.read_errors.clear();
        self.load()
    }
    fn key(owner: &DraftScope, cwd: &str) -> String {
        format!("{}\0{}\0{}", owner.connection, owner.profile, cwd.trim())
    }
    fn turn_base(&self) -> Option<String> {
        self.owner
            .as_ref()
            .and_then(|o| self.turn_bases.get(&Self::key(o, &self.root)))
            .cloned()
    }
    pub fn capture_turn(&mut self, owner: DraftScope, local: bool, cwd: String) -> Vec<PageEffect> {
        if self.demo || cwd.trim().is_empty() {
            return vec![];
        }
        self.capture = Some((owner.clone(), cwd.clone()));
        self.request_for(
            "review.capture",
            "review/rev-parse",
            false,
            json!({"ref":"HEAD"}),
            owner,
            local,
            cwd,
        )
    }
    fn request_for(
        &mut self,
        op: &str,
        route: &str,
        write: bool,
        mut body: Value,
        owner: DraftScope,
        local: bool,
        cwd: String,
    ) -> Vec<PageEffect> {
        body["path"] = json!(cwd);
        self.serial = self.serial.wrapping_add(1);
        self.pending.insert(op.into(), self.serial);
        if local {
            let Some(w) = &self.worker else {
                self.error = Some("The native Git service is unavailable".into());
                return self.apply(op, Err("The native Git service is unavailable".into()));
            };
            if let Err(e) = w.request(self.epoch, self.serial, op.into(), route.into(), body) {
                return self.apply(op, Err(e));
            }
            vec![]
        } else {
            let mut request = if write {
                PageRequest::post(format!("/api/git/{route}"), body)
            } else {
                let mut req = PageRequest::get(format!("/api/git/{route}"));
                for (k, v) in body.as_object().into_iter().flatten() {
                    if !v.is_null() {
                        req = req.query(
                            k,
                            if let Some(s) = v.as_str() {
                                s.to_owned()
                            } else {
                                v.to_string()
                            },
                        );
                    }
                }
                req
            };
            request = request
                .profile(&owner.profile)
                .timeout_secs(if write { 180 } else { 60 });
            if let Some(session) = owner.session {
                request = request.query("session_id", session);
            }
            vec![(op.into(), request)]
        }
    }
    fn request(&mut self, op: &str, route: &str, write: bool, body: Value) -> Vec<PageEffect> {
        let Some(owner) = self.owner.clone() else {
            return vec![];
        };
        if self.root.is_empty() {
            return vec![];
        }
        self.request_for(op, route, write, body, owner, self.local, self.root.clone())
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        if self.demo || self.root.is_empty() {
            self.loading = false;
            self.is_repo = false;
            return vec![];
        }
        self.loading = true;
        let base = if self.scope == "lastTurn" {
            self.turn_base()
        } else {
            None
        };
        let mut e = self.request(
            "review.list",
            "review/list",
            false,
            json!({"scope":self.scope,"base":base}),
        );
        for (op, route) in [
            ("review.status", "status"),
            ("review.ship", "review/ship-info"),
            ("review.branches", "branches"),
            ("review.bases", "base-branches"),
            ("review.worktrees", "worktrees"),
            ("review.head", "review/rev-parse"),
        ] {
            e.extend(self.request(op, route, false, json!({})));
        }
        e
    }
    fn read_diff(&mut self) -> Vec<PageEffect> {
        let Some(path) = self.selected.clone() else {
            return vec![];
        };
        let staged = self
            .files
            .iter()
            .find(|f| f.path == path)
            .is_some_and(|f| f.staged);
        let base = if self.scope == "lastTurn" {
            self.turn_base()
        } else {
            self.base.clone()
        };
        self.diff_loading = true;
        self.request(
            "review.diff",
            "review/diff",
            false,
            json!({"file":path,"scope":self.scope,"base":base,"staged":staged}),
        )
    }
    pub fn poll(&mut self, _now: f64) -> Vec<PageEffect> {
        let replies = self
            .worker
            .as_ref()
            .map(|w| w.replies())
            .unwrap_or_default();
        let mut e = vec![];
        for (epoch, generation, op, result) in replies {
            if self.pending.get(&op) != Some(&generation)
                || epoch != self.epoch && op != "review.capture"
            {
                continue;
            }
            self.pending.remove(&op);
            self.redraw = true;
            e.extend(self.apply(&op, result));
        }
        e
    }
    pub fn wake_after(&self) -> Option<std::time::Duration> {
        None
    }
    pub fn fixture(&mut self, owner: DraftScope) {
        self.demo = true;
        self.owner = Some(owner);
        self.local = true;
        self.root = "/sample/project".into();
        self.is_repo = true;
        self.head = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into());
        self.files = vec![
            File {
                path: "src/main.rs".into(),
                added: 14,
                removed: 3,
                status: "M".into(),
                staged: false,
            },
            File {
                path: "README.md".into(),
                added: 4,
                removed: 0,
                status: "M".into(),
                staged: true,
            },
        ];
        self.status = Some(
            json!({"branch":"feature/chat","defaultBranch":"main","ahead":1,"behind":0,"changed":2,"staged":1,"unstaged":1,"untracked":0,"conflicted":0}),
        );
        self.ship = Some(json!({"ghReady":false,"pr":null}));
        self.branches = vec![
            json!({"name":"main","isDefault":true,"isRemote":false,"checkedOut":false}),
            json!({"name":"feature/chat","isRemote":false,"checkedOut":true,"worktreePath":"/sample/project"}),
        ];
        self.bases = vec![json!({"name":"main","isDefault":true,"isRemote":false})];
        self.worktrees = vec![
            json!({"path":"/sample/project","branch":"feature/chat","isMain":true,"locked":false}),
            json!({"path":"/sample/project/.worktrees/research","branch":"research","isMain":false,"locked":false}),
        ];
        let args = rune::extra_args();
        self.show_worktrees = args.iter().any(|a| a == "--review-worktrees");
        self.selected = Some("src/main.rs".into());
        self.diff="diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,2 +1,3 @@\n fn main() {\n+    println!(\"Hermes\");\n }\n".into();
    }
}
