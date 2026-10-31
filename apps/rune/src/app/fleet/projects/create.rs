//! Project creation retains its opening source and write receipt.
use super::*;
use crate::gateway::prepared::ProfileProvenance;
use crate::pages::PageRequest;
use crate::source::ProfileIntent;

/// Both counters must be allocated monotonically by the owning Viewer, not reset on close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickToken {
    pub dialog: u64,
    pub pick: u64,
}

#[derive(Clone)]
pub struct PickerTarget {
    pub token: PickToken,
    pub owner: Owner,
    pub scope: Scope,
    pub generation: u64,
    pub binding: Binding,
    pub request_scope: DraftScope,
}

pub struct Form {
    pub owner: Owner,
    pub scope: Scope,
    pub generation: u64,
    pub dialog: u64,
    pub focus_return: String,
    pub revision: u64,
    pub uncertain: bool,
    pub templates: Vec<usize>,
    pub active_pick: Option<PickToken>,
    pub name: rune::TextEdit,
    pub idea: rune::TextEdit,
    pub folders: Vec<String>,
    pub error: Option<String>,
    pub binding: Binding,
    pub request_scope: DraftScope,
}

impl Form {
    pub fn new(
        owner: Owner,
        scope: Scope,
        generation: u64,
        dialog: u64,
        binding: Binding,
        request_scope: DraftScope,
    ) -> Self {
        let mut name = rune::TextEdit::new("");
        name.focused = true;
        Self {
            owner,
            scope,
            generation,
            dialog,
            focus_return: String::new(),
            revision: 0,
            uncertain: false,
            templates: shuffled_templates(),
            active_pick: None,
            name,
            idea: rune::TextEdit::new(""),
            folders: vec![],
            error: None,
            binding,
            request_scope,
        }
    }
    pub fn start_pick(&mut self, pick: u64) -> PickerTarget {
        let token = PickToken {
            dialog: self.dialog,
            pick,
        };
        self.active_pick = Some(token);
        PickerTarget {
            token,
            owner: self.owner.clone(),
            scope: self.scope.clone(),
            generation: self.generation,
            binding: self.binding.clone(),
            request_scope: self.request_scope.clone(),
        }
    }
    pub fn cancel_pick(&mut self) {
        self.active_pick = None;
    }
    /// Caller also verifies current owner, scope, generation, and source binding before invoking.
    pub fn accept_pick(
        &mut self,
        target: &PickerTarget,
        paths: impl IntoIterator<Item = String>,
    ) -> bool {
        if self.active_pick != Some(target.token)
            || target.token.dialog != self.dialog
            || self.owner != target.owner
            || self.scope != target.scope
            || self.generation != target.generation
            || self.request_scope != target.request_scope
            || target.binding.validate().is_err()
            || self.binding.validate().is_err()
        {
            return false;
        }
        self.active_pick = None;
        let paths: Vec<String> = paths.into_iter().filter(|p| !p.trim().is_empty()).collect();
        if self.name.text.trim().is_empty() {
            if let Some(path) = paths.first() {
                let base = path
                    .trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("");
                self.name.replace_text(base);
            }
        }
        for path in paths {
            if !self.folders.iter().any(|p| p == &path) {
                self.folders.push(path);
            }
        }
        true
    }
    pub fn request(&self) -> Result<PageRequest, String> {
        let name = self.name.text.trim();
        if name.is_empty() || self.folders.is_empty() {
            return Err("Enter a project name and choose at least one folder".into());
        }
        let provenance = match self.owner.session.profile_intent {
            ProfileIntent::Backend => {
                ProfileProvenance::Backend(self.owner.session.profile.clone())
            }
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        self.binding.request(
            PageRequest::rpc(
                "projects.create",
                serde_json::json!({
                    "profile": self.owner.session.profile, "name": name,
                    "folders": self.folders, "use": true
                }),
            )
            .profile(&self.owner.session.profile)
            .provenance(provenance),
        )
    }
}

/// A dispatched request must retain its source lease even after the form is closed.
pub struct Receipt {
    pub owner: Owner,
    pub scope: Scope,
    pub generation: u64,
    pub dialog: u64,
    pub request_scope: DraftScope,
    pub binding: Binding,
    pub idea: String,
}
impl Form {
    pub fn receipt(&self) -> Receipt {
        Receipt {
            owner: self.owner.clone(),
            scope: self.scope.clone(),
            generation: self.generation,
            dialog: self.dialog,
            request_scope: self.request_scope.clone(),
            binding: self.binding.clone(),
            idea: self.idea.text.trim().to_owned(),
        }
    }
}

/// Original writer uses the authoritative primary path, then its first folder.
pub fn idea_write(project: &serde_json::Value, idea: &str) -> Option<(String, String)> {
    let body = idea.trim();
    if body.is_empty() {
        return None;
    }
    let dir = project
        .get("primary_path")
        .and_then(|v| v.as_str())
        .or_else(|| {
            project
                .get("folders")?
                .as_array()?
                .first()?
                .get("path")?
                .as_str()
        })?
        .trim();
    if dir.is_empty() {
        return None;
    }
    Some((
        format!("{}/IDEA.md", dir.trim_end_matches(['/', '\\'])),
        format!("{body}\n"),
    ))
}

use crate::view::project_create::{Action, Field, Host, Target};

pub struct Generation {
    target: Target,
    scope: DraftScope,
    idea: String,
    revision: u64,
}

impl Form {
    pub fn target(&self) -> Target {
        Target {
            owner: self.owner.clone(),
            scope: self.scope.clone(),
            generation: self.generation,
            dialog: self.dialog,
        }
    }
}
impl Viewer {
    pub fn project_create_form(&self) -> Option<&Form> {
        self.fleet_state
            .project_create
            .as_ref()
            .filter(|f| self.create_target_current(&f.target()))
    }
    pub fn project_create_result(&self) -> Option<&str> {
        self.fleet_state.project_create_result.as_deref()
    }
    pub fn project_create_busy(&self) -> bool {
        self.fleet_state.project_create_pending.is_some()
    }
    fn create_target_current(&self, target: &Target) -> bool {
        self.project_owner_is_current(&target.owner)
            && self.fleet_state.projects.scope == target.scope
            && self.fleet_state.projects.generation() == target.generation
            && self
                .fleet_state
                .project_create
                .as_ref()
                .is_some_and(|f| f.target() == *target && f.binding.validate().is_ok())
    }
    pub(in crate::app) fn consume_project_create_picker(&mut self, tx: &mut rune::Tx) {
        let Some(target) = self.fleet_state.project_create_choose.take() else {
            return;
        };
        if !self.create_target_current(&Target {
            owner: target.owner.clone(),
            scope: target.scope.clone(),
            generation: target.generation,
            dialog: target.token.dialog,
        }) {
            return;
        }
        if matches!(
            target.binding.connection().transport(),
            Connection::Local { .. }
        ) {
            tx.open_folders(
                rune::FileDialogOptions::default(),
                move |v: &mut Viewer, result, tx| {
                    match result {
                        Ok(Some(paths)) => v.project_create_pick_return(
                            target,
                            paths
                                .into_iter()
                                .map(|p| p.to_string_lossy().into_owned())
                                .collect(),
                        ),
                        Ok(None) => v.project_create_pick_return(target, vec![]),
                        Err(error) => {
                            if let Some(f) = v
                                .fleet_state
                                .project_create
                                .as_mut()
                                .filter(|f| f.active_pick == Some(target.token))
                            {
                                f.error = Some(error.to_string());
                                f.cancel_pick();
                            }
                        }
                    }
                    tx.instant();
                },
            );
        } else {
            self.invalidate_page_requests("directories.");
            let effects = self.directories.open(
                target.request_scope.clone(),
                false,
                String::new(),
                crate::directories::Purpose::Projects(target.token.dialog, target.token.pick),
            );
            self.fleet_state.project_create_remote = Some(target);
            self.project_directory_requests(effects);
        }
    }
    pub(crate) fn project_directory_target_current(&self, dialog: u64, pick: u64) -> bool {
        self.fleet_state
            .project_create_remote
            .as_ref()
            .is_some_and(|target| {
                target.token == (PickToken { dialog, pick })
                    && self.create_target_current(&Target {
                        owner: target.owner.clone(),
                        scope: target.scope.clone(),
                        generation: target.generation,
                        dialog,
                    })
                    && self
                        .fleet_state
                        .project_create
                        .as_ref()
                        .is_some_and(|f| f.active_pick == Some(target.token))
            })
    }
    pub(crate) fn project_directory_requests(&mut self, effects: Vec<crate::pages::PageEffect>) {
        let Some(target) = self.fleet_state.project_create_remote.clone() else {
            return;
        };
        if !self.create_target_current(&Target {
            owner: target.owner.clone(),
            scope: target.scope.clone(),
            generation: target.generation,
            dialog: target.token.dialog,
        }) {
            return;
        }
        let provenance = match target.owner.session.profile_intent {
            ProfileIntent::Backend => {
                ProfileProvenance::Backend(target.owner.session.profile.clone())
            }
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        for (op, request) in effects {
            match target
                .binding
                .request(request.provenance(provenance.clone()))
            {
                Ok(request) => self.page_request(&op, request),
                Err(error) => {
                    self.directories.error = Some(error);
                    self.directories.loading = false;
                }
            }
        }
    }
    pub(crate) fn project_directory_return(
        &mut self,
        dialog: u64,
        pick: u64,
        path: Option<String>,
    ) {
        let Some(target) = self.fleet_state.project_create_remote.take() else {
            return;
        };
        if target.token != (PickToken { dialog, pick }) {
            self.fleet_state.project_create_remote = Some(target);
            return;
        }
        self.project_create_pick_return(target, path.into_iter().collect());
    }
    fn project_create_pick_return(&mut self, target: PickerTarget, paths: Vec<String>) {
        let t = Target {
            owner: target.owner.clone(),
            scope: target.scope.clone(),
            generation: target.generation,
            dialog: target.token.dialog,
        };
        if self.create_target_current(&t) {
            if let Some(form) = self.fleet_state.project_create.as_mut() {
                form.accept_pick(&target, paths);
            }
        }
    }
    fn project_create_submit(&mut self, target: &Target) {
        if !self.create_target_current(target)
            || self.project_create_busy()
            || self
                .fleet_state
                .project_create
                .as_ref()
                .is_some_and(|f| f.uncertain)
        {
            return;
        }
        let form = self.fleet_state.project_create.as_ref().unwrap();
        let request = form.request();
        let receipt = form.receipt();
        let result = request.and_then(|request| {
            self.fleet_state.project_create_network.request(
                &receipt.request_scope,
                receipt.binding.connection().clone(),
                "project-create",
                request,
            )
        });
        match result {
            Ok(()) => {
                // A submitted idea belongs to the receipt; a late generation cannot edit it.
                self.fleet_state.project_create.as_mut().unwrap().revision += 1;
                self.fleet_state.project_create_pending = Some(receipt);
            }
            Err(error) => self.fleet_state.project_create.as_mut().unwrap().error = Some(error),
        }
    }
    pub(super) fn poll_project_create(&mut self) -> bool {
        let mut changed = self.poll_project_idea();
        let Some(receipt) = self.fleet_state.project_create_pending.take() else {
            if self
                .fleet_state
                .project_create
                .as_ref()
                .is_some_and(|f| !self.create_target_current(&f.target()))
            {
                self.fleet_state.project_create = None;
                self.fleet_state.project_create_choose = None;
                self.fleet_state.project_create_remote = None;
                if matches!(
                    self.directories.purpose,
                    crate::directories::Purpose::Projects(..)
                ) {
                    self.directories.owner = None;
                    self.invalidate_page_requests("directories.");
                }
                changed = true;
            }
            return changed;
        };
        let Some(reply) = self
            .fleet_state
            .project_create_network
            .poll(&receipt.request_scope)
            .into_iter()
            .find(|r| !r.progress && r.operation == "project-create")
        else {
            self.fleet_state.project_create_pending = Some(receipt);
            return changed;
        };
        let current = self.project_owner_is_current(&receipt.owner)
            && self.fleet_state.projects.scope == receipt.scope
            && self.fleet_state.projects.generation() == receipt.generation
            && self
                .fleet_state
                .project_create
                .as_ref()
                .is_some_and(|f| f.dialog == receipt.dialog);
        match reply.result.and_then(|v| {
            v.get("project")
                .filter(|p| p.is_object())
                .cloned()
                .ok_or_else(|| "Create was not confirmed by the agent".to_string())
        }) {
            Ok(project) => {
                self.project_write_idea(&receipt, &project);
                if current {
                    if self
                        .fleet_state
                        .project_create
                        .as_ref()
                        .is_some_and(|f| f.dialog == receipt.dialog)
                    {
                        self.fleet_state.project_create = None;
                    }
                    if let Some(id) = project.get("id").and_then(|v| v.as_str()) {
                        let label = project
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(id)
                            .to_owned();
                        let path = project
                            .get("primary_path")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned);
                        if let Some(tree) = self.fleet_state.projects.tree.as_mut() {
                            tree.projects.retain(|p| p.id != id);
                            tree.projects.push(crate::projects::Project {
                                id: id.to_owned(),
                                label,
                                path,
                                ..Default::default()
                            });
                            tree.active_id = Some(id.to_owned());
                        }
                        self.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Project;
                        self.fleet_state.project_create_refresh = Some(receipt.owner.clone());
                        self.project_select(
                            receipt.owner.clone(),
                            Scope::Project(id.to_owned()),
                            receipt.generation,
                        );
                    }
                } else {
                    self.fleet_state.project_create_result =
                        Some("Project created in the previous source".into());
                }
            }
            Err(error) => {
                let message = format!("Create failed or its result is uncertain: {error}");
                if current
                    && self
                        .fleet_state
                        .project_create
                        .as_ref()
                        .is_some_and(|f| f.dialog == receipt.dialog)
                {
                    let form = self.fleet_state.project_create.as_mut().unwrap();
                    form.uncertain = !error.starts_with("Workspace RPC refused (");
                    form.error = Some(message);
                } else {
                    self.fleet_state.project_create_result = Some(message);
                }
            }
        }
        true
    }
    fn project_write_idea(&mut self, receipt: &Receipt, project: &serde_json::Value) {
        let Some((path, content)) = idea_write(project, &receipt.idea) else {
            return;
        };
        let body = serde_json::json!({"path":path,"content":content});
        let request = if matches!(
            receipt.binding.connection().transport(),
            Connection::Local { .. }
        ) {
            PageRequest::local_fs("/api/fs/write-project-idea", body)
        } else {
            PageRequest::post("/api/fs/write-text", body)
        };
        let provenance = match receipt.owner.session.profile_intent {
            ProfileIntent::Backend => {
                ProfileProvenance::Backend(receipt.owner.session.profile.clone())
            }
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        if let Ok(request) = receipt.binding.request(
            request
                .profile(&receipt.owner.session.profile)
                .provenance(provenance),
        ) {
            let _ = self.fleet_state.project_create_network.request(
                &receipt.request_scope,
                receipt.binding.connection().clone(),
                "project-idea-write",
                request,
            );
        }
    }
    fn poll_project_idea(&mut self) -> bool {
        let Some(pending) = self.fleet_state.project_idea_pending.take() else {
            return false;
        };
        let Some(reply) = self
            .fleet_state
            .project_idea_network
            .poll(&pending.scope)
            .into_iter()
            .find(|r| !r.progress)
        else {
            self.fleet_state.project_idea_pending = Some(pending);
            return false;
        };
        if self.create_target_current(&pending.target) {
            let form = self.fleet_state.project_create.as_mut().unwrap();
            if form.idea.text == pending.idea && form.revision == pending.revision {
                if let Ok(value) = reply.result {
                    if let Some(text) = value
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        form.idea.replace_text(text);
                    }
                }
            }
        }
        true
    }
}
impl Host for Viewer {
    fn create_action(&mut self, action: Action) {
        match action {
            Action::Open(open) => {
                if self.project_create_busy()
                    || !self.project_owner_is_current(&open.owner)
                    || self.fleet_state.projects.scope != open.scope
                    || self.fleet_state.projects.generation() != open.generation
                {
                    return;
                }
                let Some((_, binding)) = self.project_owner() else {
                    return;
                };
                self.fleet_state.project_create_serial += 1;
                let mut form = Form::new(
                    open.owner,
                    open.scope,
                    open.generation,
                    self.fleet_state.project_create_serial,
                    binding,
                    self.composer_scope(),
                );
                form.focus_return = open.focus_return;
                self.fleet_state.project_idea_network.invalidate();
                self.fleet_state.project_idea_pending = None;
                self.fleet_state.project_create = Some(form);
                self.fleet_state.project_create_result = None;
            }
            Action::Choose(target) => {
                if !self.create_target_current(&target) || self.project_create_busy() {
                    return;
                }
                self.fleet_state.project_create_serial += 1;
                self.fleet_state.project_create_choose = Some(
                    self.fleet_state
                        .project_create
                        .as_mut()
                        .unwrap()
                        .start_pick(self.fleet_state.project_create_serial),
                );
            }
            Action::Remove(target, path) => {
                if self.create_target_current(&target) && !self.project_create_busy() {
                    self.fleet_state
                        .project_create
                        .as_mut()
                        .unwrap()
                        .folders
                        .retain(|p| p != &path);
                }
            }
            Action::Edited(target) => {
                if self.create_target_current(&target) {
                    self.fleet_state.project_create.as_mut().unwrap().revision += 1;
                }
            }
            Action::Template(target, index) => {
                if self.create_target_current(&target) && !self.project_create_busy() {
                    if let Some((_, idea)) = template(index) {
                        let form = self.fleet_state.project_create.as_mut().unwrap();
                        form.idea.replace_text(idea);
                        form.revision += 1;
                    }
                }
            }
            Action::Shuffle(target) => {
                if self.create_target_current(&target) {
                    self.fleet_state.project_create.as_mut().unwrap().templates =
                        shuffled_templates();
                }
            }
            Action::Generate(target) => self.project_generate_idea(target),
            Action::Submit(target) => self.project_create_submit(&target),
            Action::Retry(target) => {
                if self.create_target_current(&target) && !self.project_create_busy() {
                    let form = self.fleet_state.project_create.as_mut().unwrap();
                    if form.uncertain {
                        form.uncertain = false;
                        form.error = None;
                        self.project_create_submit(&target);
                    }
                }
            }
            Action::Cancel(target) => {
                if self.create_target_current(&target) {
                    self.fleet_state.project_create = None;
                    self.fleet_state.project_create_choose = None;
                    self.fleet_state.project_create_remote = None;
                    self.invalidate_page_requests("directories.");
                    if matches!(
                        self.directories.purpose,
                        crate::directories::Purpose::Projects(..)
                    ) {
                        self.directories.owner = None;
                    }
                }
            }
        }
    }
    fn create_edit(&mut self, target: &Target, field: Field) -> &mut rune::TextEdit {
        if !self.create_target_current(target) || self.project_create_busy() {
            return &mut self.fleet_state.project_create_detached;
        }
        let form = self.fleet_state.project_create.as_mut().unwrap();
        match field {
            Field::Name => &mut form.name,
            Field::Idea => &mut form.idea,
        }
    }
}

fn template(index: usize) -> Option<(String, String)> {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("idea_templates.json")).ok()?;
    let row = rows.get(index)?;
    Some((
        format!("{} {}", row["emoji"].as_str()?, row["label"].as_str()?),
        row["idea"].as_str()?.to_owned(),
    ))
}
fn shuffled_templates() -> Vec<usize> {
    let mut values: Vec<usize> = (0..18).collect();
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    for i in (1..values.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        values.swap(i, seed as usize % (i + 1));
    }
    values.truncate(6);
    values
}
impl Viewer {
    pub fn project_create_templates(&self) -> Vec<(usize, String)> {
        self.project_create_form()
            .map(|f| {
                f.templates
                    .iter()
                    .filter_map(|&i| template(i).map(|(label, _)| (i, label)))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn project_idea_busy(&self) -> bool {
        self.fleet_state.project_idea_pending.is_some()
    }
    fn project_generate_idea(&mut self, target: Target) {
        if !self.create_target_current(&target) || self.project_idea_busy() {
            return;
        }
        let f = self.fleet_state.project_create.as_ref().unwrap();
        let name = f.name.text.trim();
        let input = if name.is_empty() {
            "Surprise me with a fun project.".to_owned()
        } else {
            format!("Project name: {name}")
        };
        let provenance = match f.owner.session.profile_intent {
            ProfileIntent::Backend => ProfileProvenance::Backend(f.owner.session.profile.clone()),
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        let request = f.binding.request(PageRequest::rpc("llm.oneshot", serde_json::json!({"instructions":"You generate a single, concrete project idea as a short IDEA.md body: a one-line summary, then 3-5 bullet goals. No preamble, no code fences, under 120 words.", "input":input, "temperature":1.0})).profile(&f.owner.session.profile).provenance(provenance));
        let pending = Generation {
            target,
            scope: f.request_scope.clone(),
            idea: f.idea.text.clone(),
            revision: f.revision,
        };
        if let Ok(request) = request {
            if self
                .fleet_state
                .project_idea_network
                .request(
                    &pending.scope,
                    f.binding.connection().clone(),
                    "project-idea-generate",
                    request,
                )
                .is_ok()
            {
                self.fleet_state.project_idea_pending = Some(pending);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn form() -> Form {
        let binding = Binding::new(
            Connection::Local {
                home: "/tmp/project-create-fixture".into(),
            },
            None,
        )
        .unwrap();
        let owner = Owner {
            session: SessionOwner::with_intent(
                binding.stamp().clone(),
                "work",
                ProfileIntent::Backend,
            ),
            epoch: 1,
        };
        Form::new(
            owner,
            Scope::All,
            1,
            1,
            binding,
            DraftScope {
                profile: "work".into(),
                draft_origin: None,
                connection: 1,
                profile_intent: ProfileIntent::Backend,
                session: None,
            },
        )
    }
    #[test]
    fn picks_preserve_order_and_refill_cleared_name_even_when_duplicate() {
        let mut form = form();
        let pick = form.start_pick(1);
        assert!(form.accept_pick(&pick, vec!["/one".into(), "/two".into(), "/one".into()]));
        assert_eq!(form.name.text, "one");
        assert_eq!(form.folders, ["/one", "/two"]);
        form.name.replace_text("");
        let pick = form.start_pick(2);
        assert!(form.accept_pick(&pick, vec!["/two".into()]));
        assert_eq!(form.name.text, "two");
        assert_eq!(form.folders, ["/one", "/two"]);
    }
    #[test]
    fn request_requires_name_and_folder_and_captures_payload() {
        let mut form = form();
        assert!(form.request().is_err());
        form.name.replace_text("  Finch  ");
        let pick = form.start_pick(1);
        form.accept_pick(&pick, vec!["/first".into(), "/second".into()]);
        let request = form.request().unwrap();
        assert_eq!(request.path, "projects.create");
        assert_eq!(
            request.body,
            serde_json::json!({"profile":"work", "name":"Finch", "folders":["/first", "/second"], "use":true})
        );
    }
    #[test]
    fn stale_pick_and_idea_writer_are_fenced() {
        let mut form = form();
        let old = form.start_pick(1);
        let _new = form.start_pick(2);
        assert!(!form.accept_pick(&old, vec!["/old".into()]));
        assert!(form.folders.is_empty());
        assert_eq!(
            idea_write(&serde_json::json!({"primary_path":"/project///"}), " hi \n"),
            Some(("/project/IDEA.md".into(), "hi\n".into()))
        );
        assert_eq!(
            idea_write(&serde_json::json!({"primary_path":"/project"}), " "),
            None
        );
    }
    #[test]
    fn template_data_contains_all_original_choices() {
        assert_eq!((0..18).filter(|&i| template(i).is_some()).count(), 18);
        assert_eq!(template(0).unwrap().0, "🎮 Game jam");
        assert!(template(18).is_none());
    }
}

#[cfg(test)]
#[path = "create_tests.rs"]
pub(super) mod integration_tests;
