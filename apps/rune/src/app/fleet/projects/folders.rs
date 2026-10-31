//! Owner-bound folder picker and sequential metadata writes.
use super::*;
use crate::{gateway::prepared::ProfileProvenance, pages::PageRequest, source::ProfileIntent};
use std::collections::VecDeque;

#[derive(Clone)]
pub struct Capture {
    owner: Owner,
    scope: Scope,
    generation: u64,
    id: String,
    binding: Binding,
    pub(super) request_scope: DraftScope,
    pub(super) dialog: u64,
    pub(super) token: u64,
}
pub struct Pending {
    capture: Capture,
    paths: VecDeque<String>,
}
impl Viewer {
    fn folders_current(&self, c: &Capture) -> bool {
        self.project_owner_is_current(&c.owner)
            && self.fleet_state.projects.scope == c.scope
            && self.fleet_state.projects.generation() == c.generation
            && c.binding.validate().is_ok()
    }
    pub fn project_folders_open(
        &mut self,
        owner: Owner,
        scope: Scope,
        generation: u64,
        id: String,
    ) {
        if self.fleet_state.project_folders_pending.is_some()
            || !self.project_owner_is_current(&owner)
            || self.fleet_state.projects.scope != scope
            || self.fleet_state.projects.generation() != generation
        {
            return;
        }
        let Some((_, binding)) = self.project_owner() else {
            return;
        };
        if !self.fleet_state.projects.tree.as_ref().is_some_and(|t| {
            t.projects
                .iter()
                .any(|p| p.id == id && !p.is_auto && !p.is_no_project)
        }) {
            return;
        }
        self.fleet_state.project_folders_serial += 1;
        self.fleet_state.project_folders = Some(Capture {
            owner,
            scope,
            generation,
            id,
            binding,
            request_scope: self.composer_scope(),
            dialog: self.fleet_state.project_folders_serial,
            token: self.fleet_state.project_folders_serial,
        });
        self.fleet_state.project_folders_error = None;
        self.fleet_state.project_folders_result = None;
    }
    pub fn project_folders_visible(&self) -> bool {
        self.fleet_state
            .project_folders
            .as_ref()
            .is_some_and(|c| self.folders_current(c))
    }
    pub fn project_folders_result(&self) -> Option<&str> {
        self.fleet_state.project_folders_result.as_deref()
    }
    pub fn project_folders_error(&self) -> Option<&str> {
        self.fleet_state.project_folders_error.as_deref()
    }
    pub fn project_folders_busy(&self) -> bool {
        self.fleet_state.project_folders_pending.is_some()
    }
    pub fn project_folders_cancel(&mut self) {
        self.fleet_state.project_folders = None;
        self.fleet_state.project_folders_choose = None;
        if matches!(
            self.directories.purpose,
            crate::directories::Purpose::ProjectFolders(..)
        ) {
            self.directories.owner = None;
            self.invalidate_page_requests("directories.");
        }
    }
    pub fn project_folders_choose(&mut self) {
        if self.project_folders_busy() {
            return;
        }
        let Some(c) = self.fleet_state.project_folders.clone() else {
            return;
        };
        if !self.folders_current(&c) {
            return;
        }
        self.fleet_state.project_folders_serial += 1;
        let mut c = c;
        c.token = self.fleet_state.project_folders_serial;
        self.fleet_state.project_folders = Some(c.clone());
        self.fleet_state.project_folders_choose = Some(c);
    }
    pub(in crate::app) fn consume_project_folders_picker(&mut self, tx: &mut rune::Tx) {
        let Some(c) = self.fleet_state.project_folders_choose.take() else {
            return;
        };
        if !self.folders_current(&c) {
            return;
        }
        if !matches!(c.binding.connection().transport(), Connection::Local { .. }) {
            self.invalidate_page_requests("directories.");
            let effects = self.directories.open(
                c.request_scope.clone(),
                false,
                String::new(),
                crate::directories::Purpose::ProjectFolders(c.dialog, c.token),
            );
            self.project_folders_directory_requests(effects);
            return;
        }
        tx.open_folders(
            rune::FileDialogOptions::default(),
            move |v: &mut Viewer, result, tx| {
                match result {
                    Ok(Some(paths)) => v.project_folders_return(
                        c,
                        paths
                            .into_iter()
                            .map(|p| p.to_string_lossy().into_owned())
                            .collect(),
                    ),
                    Ok(None) => {}
                    Err(e) => {
                        if v.folders_current(&c)
                            && v.fleet_state
                                .project_folders
                                .as_ref()
                                .is_some_and(|f| f.token == c.token)
                        {
                            v.fleet_state.project_folders_error = Some(e.to_string());
                        }
                    }
                }
                tx.instant();
            },
        );
    }
    pub(crate) fn project_folders_directory_current(&self, dialog: u64, pick: u64) -> bool {
        self.fleet_state
            .project_folders
            .as_ref()
            .is_some_and(|c| c.dialog == dialog && c.token == pick && self.folders_current(c))
    }
    pub(crate) fn project_folders_directory_requests(
        &mut self,
        effects: Vec<crate::pages::PageEffect>,
    ) {
        let crate::directories::Purpose::ProjectFolders(dialog, pick) = self.directories.purpose
        else {
            return;
        };
        if !self.project_folders_directory_current(dialog, pick) {
            return;
        }
        let c = self.fleet_state.project_folders.as_ref().unwrap().clone();
        let provenance = match c.owner.session.profile_intent {
            ProfileIntent::Backend => ProfileProvenance::Backend(c.owner.session.profile.clone()),
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        for (op, request) in effects {
            match c.binding.request(request.provenance(provenance.clone())) {
                Ok(request) => self.page_request(&op, request),
                Err(error) => {
                    self.directories.error = Some(error);
                    self.directories.loading = false;
                }
            }
        }
    }
    pub(crate) fn project_folders_directory_return(
        &mut self,
        dialog: u64,
        pick: u64,
        path: Option<String>,
    ) {
        if !self.project_folders_directory_current(dialog, pick) {
            return;
        }
        let c = self.fleet_state.project_folders.as_ref().unwrap().clone();
        if let Some(path) = path {
            self.project_folders_return(c, vec![path]);
        }
    }
    pub(super) fn project_folders_return(&mut self, c: Capture, paths: Vec<String>) {
        if !self.folders_current(&c)
            || !self
                .fleet_state
                .project_folders
                .as_ref()
                .is_some_and(|f| f.token == c.token)
        {
            return;
        }
        let mut existing = std::collections::HashSet::new();
        if let Some(p) = self
            .fleet_state
            .projects
            .tree
            .as_ref()
            .and_then(|t| t.projects.iter().find(|p| p.id == c.id))
        {
            existing.extend(p.path.iter().cloned());
            existing.extend(p.repos.iter().filter_map(|r| r.path.clone()));
        }
        let paths: VecDeque<_> = paths
            .into_iter()
            .filter(|p| !p.trim().is_empty() && existing.insert(p.clone()))
            .collect();
        if paths.is_empty() {
            return;
        }
        self.fleet_state.project_folders_pending = Some(Pending { capture: c, paths });
        self.dispatch_project_folder();
    }
    fn dispatch_project_folder(&mut self) {
        let pending = self.fleet_state.project_folders_pending.as_mut().unwrap();
        let path = pending.paths.pop_front().unwrap();
        let c = &pending.capture;
        let provenance = match c.owner.session.profile_intent {
            ProfileIntent::Backend => ProfileProvenance::Backend(c.owner.session.profile.clone()),
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        let request = c.binding.request(PageRequest::rpc("projects.add_folder", serde_json::json!({"profile":c.owner.session.profile,"id":c.id,"path":path,"is_primary":false})).profile(&c.owner.session.profile).provenance(provenance));
        let result = request.and_then(|r| {
            self.fleet_state.project_folders_network.request(
                &c.request_scope,
                c.binding.connection().clone(),
                "project-add-folder",
                r,
            )
        });
        if let Err(e) = result {
            let pending = self.fleet_state.project_folders_pending.take().unwrap();
            let message = format!(
                "Add folder failed or its result is uncertain: {e}. Refresh the project before trying again."
            );
            let current = self.folders_current(&pending.capture);
            if current {
                self.refresh_project_tree();
            }
            if current && self.fleet_state.project_folders.is_some() {
                self.fleet_state.project_folders_error = Some(message);
            } else {
                self.fleet_state.project_folders_result = Some(message);
            }
        }
    }
    pub(super) fn poll_project_folders(&mut self) -> bool {
        let Some(p) = self.fleet_state.project_folders_pending.take() else {
            if self
                .fleet_state
                .project_folders
                .as_ref()
                .is_some_and(|c| !self.folders_current(c))
            {
                self.project_folders_cancel();
                return true;
            }
            return false;
        };
        let Some(reply) = self
            .fleet_state
            .project_folders_network
            .poll(&p.capture.request_scope)
            .into_iter()
            .find(|r| !r.progress)
        else {
            self.fleet_state.project_folders_pending = Some(p);
            return false;
        };
        let current = self.folders_current(&p.capture);
        let result = reply.result.and_then(|v| {
            v.get("project")
                .filter(|p| p.is_object())
                .map(|_| ())
                .ok_or_else(|| "Add folder was not confirmed by the agent".to_owned())
        });
        match result {
            Ok(()) if !p.paths.is_empty() => {
                self.fleet_state.project_folders_pending = Some(p);
                self.dispatch_project_folder();
            }
            Ok(()) => {
                if current {
                    if self.fleet_state.project_folders.is_some() {
                        self.fleet_state.project_folders = None;
                    } else {
                        self.fleet_state.project_folders_result = Some("Folders added".into());
                    }
                    self.refresh_project_tree();
                } else {
                    self.fleet_state.project_folders_result =
                        Some("Folders added in the previous project source".into());
                }
            }
            Err(e) => {
                if current {
                    self.refresh_project_tree();
                    let message = format!(
                        "Add folder failed or its result is uncertain: {e}. Refresh the project before trying again."
                    );
                    if self.fleet_state.project_folders.is_some() {
                        self.fleet_state.project_folders_error = Some(message);
                    } else {
                        self.fleet_state.project_folders_result = Some(message);
                    }
                } else {
                    self.fleet_state.project_folders_result = Some(format!(
                        "Previous project source add folder failed or is uncertain: {e}"
                    ));
                }
            }
        }
        true
    }
}
