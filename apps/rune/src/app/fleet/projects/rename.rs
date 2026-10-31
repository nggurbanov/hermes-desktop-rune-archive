//! A rename holds the source and scope that opened it through its terminal receipt.
use super::*;
use crate::gateway::prepared::ProfileProvenance;
use crate::pages::PageRequest;
use crate::source::ProfileIntent;

#[derive(Clone)]
pub struct MenuTarget {
    pub owner: Owner,
    pub scope: Scope,
    pub generation: u64,
    pub id: String,
    pub point: Option<(f32, f32)>,
    pub binding: Binding,
}
pub struct Form {
    pub owner: Owner,
    pub scope: Scope,
    pub generation: u64,
    pub id: String,
    pub name: rune::TextEdit,
    pub error: Option<String>,
    binding: Binding,
    request_scope: DraftScope,
}
pub struct Receipt {
    owner: Owner,
    scope: Scope,
    generation: u64,
    request_scope: DraftScope,
    _binding: Binding,
}
impl Viewer {
    pub fn project_rename_menu(&self) -> Option<&MenuTarget> {
        self.fleet_state.project_rename_menu.as_ref().filter(|m| {
            self.project_owner_is_current(&m.owner)
                && self.fleet_state.projects.scope == m.scope
                && self.fleet_state.projects.generation() == m.generation
        })
    }
    pub fn project_rename_menu_open(
        &mut self,
        owner: Owner,
        scope: Scope,
        generation: u64,
        id: String,
        point: Option<(f32, f32)>,
    ) {
        if self.project_owner_is_current(&owner)
            && self.fleet_state.projects.scope == scope
            && self.fleet_state.projects.generation() == generation
        {
            let Some((_, binding)) = self.project_owner() else {
                return;
            };
            self.fleet_state.project_rename_menu = Some(MenuTarget {
                binding,
                owner,
                scope,
                generation,
                id,
                point,
            });
        }
    }
    pub fn project_rename_menu_close(&mut self) {
        self.fleet_state.project_rename_menu = None;
    }
    pub fn project_rename_form(&self) -> Option<&Form> {
        self.fleet_state.project_rename.as_ref().filter(|f| {
            self.project_owner_is_current(&f.owner)
                && self.fleet_state.projects.scope == f.scope
                && self.fleet_state.projects.generation() == f.generation
        })
    }
    pub fn project_rename_name_mut(&mut self) -> &mut rune::TextEdit {
        &mut self
            .fleet_state
            .project_rename
            .as_mut()
            .expect("mounted rename form")
            .name
    }
    pub fn project_rename_result(&self) -> Option<&str> {
        self.fleet_state.project_rename_result.as_deref()
    }
    pub fn project_rename_busy(&self) -> bool {
        self.fleet_state.project_rename_pending.is_some()
    }
    pub fn project_rename_open(&mut self, owner: Owner, scope: Scope, generation: u64, id: String) {
        if self.project_rename_busy()
            || !self.project_owner_is_current(&owner)
            || self.fleet_state.projects.scope != scope
            || self.fleet_state.projects.generation() != generation
        {
            return;
        }
        let Some(project) = self.fleet_state.projects.tree.as_ref().and_then(|tree| {
            tree.projects
                .iter()
                .find(|p| p.id == id && !p.is_auto && !p.is_no_project)
        }) else {
            return;
        };
        let name = project.label.clone();
        let Some((_, binding)) = self.project_owner() else {
            return;
        };
        let mut edit = rune::TextEdit::new(name);
        edit.set_selection(0, edit.text.len());
        edit.focused = true;
        self.fleet_state.project_rename = Some(Form {
            owner,
            scope,
            generation,
            id,
            name: edit,
            error: None,
            binding,
            request_scope: self.composer_scope(),
        });
    }
    pub fn project_rename_cancel(&mut self) {
        self.fleet_state.project_rename = None;
    }
    pub fn project_rename_submit(&mut self) {
        if self.project_rename_busy() {
            return;
        }
        let Some(form) = self.fleet_state.project_rename.as_ref() else {
            return;
        };
        let valid = self.project_owner_is_current(&form.owner)
            && self.fleet_state.projects.scope == form.scope
            && self.fleet_state.projects.generation() == form.generation
            && form.binding.validate().is_ok();
        if !valid {
            self.fleet_state.project_rename = None;
            return;
        }
        let name = form.name.text.trim();
        if name.is_empty() {
            return;
        }
        let provenance = match form.owner.session.profile_intent {
            ProfileIntent::Backend => {
                ProfileProvenance::Backend(form.owner.session.profile.clone())
            }
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        let request = form.binding.request(
            PageRequest::rpc(
                "projects.update",
                serde_json::json!({"profile":form.owner.session.profile,"id":form.id,"name":name}),
            )
            .profile(&form.owner.session.profile)
            .provenance(provenance),
        );
        let receipt = Receipt {
            owner: form.owner.clone(),
            scope: form.scope.clone(),
            generation: form.generation,
            request_scope: form.request_scope.clone(),
            _binding: form.binding.clone(),
        };
        let result = request.and_then(|request| {
            self.fleet_state.project_rename_network.request(
                &receipt.request_scope,
                receipt._binding.connection().clone(),
                "project-rename",
                request,
            )
        });
        match result {
            Ok(()) => self.fleet_state.project_rename_pending = Some(receipt),
            Err(error) => self.fleet_state.project_rename.as_mut().unwrap().error = Some(error),
        }
    }
    pub(super) fn poll_project_rename(&mut self) -> bool {
        let Some(receipt) = self.fleet_state.project_rename_pending.take() else {
            if self.fleet_state.project_rename.as_ref().is_some_and(|f| {
                !self.project_owner_is_current(&f.owner)
                    || self.fleet_state.projects.scope != f.scope
                    || self.fleet_state.projects.generation() != f.generation
            }) {
                self.fleet_state.project_rename = None;
                return true;
            }
            return false;
        };
        let Some(reply) = self
            .fleet_state
            .project_rename_network
            .poll(&receipt.request_scope)
            .into_iter()
            .find(|r| !r.progress)
        else {
            self.fleet_state.project_rename_pending = Some(receipt);
            return false;
        };
        let current = self.project_owner_is_current(&receipt.owner)
            && self.fleet_state.projects.scope == receipt.scope
            && self.fleet_state.projects.generation() == receipt.generation;
        let result = reply.result.and_then(|value| {
            value
                .get("project")
                .filter(|v| v.is_object())
                .map(|_| ())
                .ok_or_else(|| "Rename was not confirmed by the agent".to_owned())
        });
        if current {
            match result {
                Ok(()) => {
                    self.fleet_state.project_rename = None;
                    self.refresh_project_tree();
                }
                Err(error) => {
                    let message = format!("Rename failed or its result is uncertain: {error}");
                    if let Some(form) = self.fleet_state.project_rename.as_mut() {
                        form.error = Some(message);
                    } else {
                        self.fleet_state.project_rename_result = Some(message);
                    }
                }
            }
        } else {
            self.fleet_state.project_rename = None;
            self.fleet_state.project_rename_result = Some(match result {
                Ok(()) => "Rename completed in the previous project source".into(),
                Err(error) => {
                    format!("Previous project source rename failed or is uncertain: {error}")
                }
            });
        }
        true
    }
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod tests;
