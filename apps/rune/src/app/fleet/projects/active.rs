//! Retain the captured source until the active-project write reaches a terminal receipt.
use super::*;
use crate::{gateway::prepared::ProfileProvenance, pages::PageRequest, source::ProfileIntent};

pub struct Receipt {
    owner: Owner,
    scope: Scope,
    generation: u64,
    request_scope: DraftScope,
    _binding: Binding,
}
impl Viewer {
    pub fn project_set_active(
        &mut self,
        owner: Owner,
        scope: Scope,
        generation: u64,
        id: Option<String>,
        binding: Binding,
    ) {
        if self.fleet_state.project_active_pending.is_some()
            || !self.project_owner_is_current(&owner)
            || self.fleet_state.projects.scope != scope
            || self.fleet_state.projects.generation() != generation
        {
            return;
        }
        if let Some(id) = &id {
            if !self.fleet_state.projects.tree.as_ref().is_some_and(|t| {
                t.projects
                    .iter()
                    .any(|p| p.id == *id && !p.is_auto && !p.is_no_project)
            }) {
                return;
            }
        }
        if binding.stamp() != &owner.session.source || binding.validate().is_err() {
            return;
        }
        let provenance = match owner.session.profile_intent {
            ProfileIntent::Backend => ProfileProvenance::Backend(owner.session.profile.clone()),
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        let request = binding.request(
            PageRequest::rpc(
                "projects.set_active",
                serde_json::json!({"profile":owner.session.profile,"id":id}),
            )
            .profile(&owner.session.profile)
            .provenance(provenance),
        );
        let receipt = Receipt {
            owner,
            scope,
            generation,
            request_scope: self.composer_scope(),
            _binding: binding,
        };
        let result = request.and_then(|request| {
            self.fleet_state.project_active_network.request(
                &receipt.request_scope,
                receipt._binding.connection().clone(),
                "project-set-active",
                request,
            )
        });
        match result {
            Ok(()) => self.fleet_state.project_active_pending = Some(receipt),
            Err(error) => {
                self.fleet_state.project_active_result =
                    Some(format!("Set active failed or is uncertain: {error}"))
            }
        }
    }
    pub fn project_active_busy(&self) -> bool {
        self.fleet_state.project_active_pending.is_some()
    }
    pub fn project_active_result(&self) -> Option<&str> {
        self.fleet_state.project_active_result.as_deref()
    }
    pub(super) fn poll_project_active(&mut self) -> bool {
        let Some(receipt) = self.fleet_state.project_active_pending.take() else {
            return false;
        };
        let Some(reply) = self
            .fleet_state
            .project_active_network
            .poll(&receipt.request_scope)
            .into_iter()
            .find(|r| !r.progress)
        else {
            self.fleet_state.project_active_pending = Some(receipt);
            return false;
        };
        let current = self.project_owner_is_current(&receipt.owner)
            && self.fleet_state.projects.scope == receipt.scope
            && self.fleet_state.projects.generation() == receipt.generation;
        let result = reply.result.and_then(|v| match v.get("active_id") {
            Some(value) if value.is_null() || value.is_string() => Ok(()),
            _ => Err("Active project was not confirmed by the agent".into()),
        });
        if current && result.is_ok() {
            if self.fleet_state.projects_pending.is_some() {
                self.fleet_state.project_active_refresh = Some(receipt.owner.clone());
            } else {
                self.refresh_project_tree();
            }
        }
        self.fleet_state.project_active_result = match result {
            Ok(()) if current => None,
            Ok(()) => Some("Active project changed in the previous project source".into()),
            Err(e) => Some(format!(
                "Set active failed or is uncertain: {e}. Refresh before retrying."
            )),
        };
        true
    }
}
