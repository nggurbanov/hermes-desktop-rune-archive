//! Project navigation belongs to a proved source and profile, never to a project id alone.
use crate::{
    gateway::prepared::ProfileProvenance,
    pages::PageRequest,
    source::{Binding, ProfileIntent, SessionOwner},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub session: SessionOwner,
    pub epoch: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub projects: Vec<Project>,
    #[serde(default, rename = "active_id")]
    pub active_id: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub label: String,
    pub path: Option<String>,
    #[serde(default)]
    pub is_no_project: bool,
    #[serde(default)]
    pub is_auto: bool,
    #[serde(default)]
    pub last_active: Option<f64>,
    #[serde(default)]
    pub repos: Vec<Repo>,
    #[serde(default)]
    pub preview_sessions: Vec<Value>,
    #[serde(default)]
    pub session_ids: Vec<String>,
    #[serde(default)]
    pub session_count: u64,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub id: String,
    pub label: String,
    pub path: Option<String>,
    #[serde(default)]
    pub groups: Vec<Lane>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lane {
    pub id: String,
    pub label: String,
    pub path: Option<String>,
    #[serde(default)]
    pub sessions: Vec<Value>,
}
impl Project {
    pub fn root(&self) -> Option<&str> {
        self.path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .or_else(|| {
                self.repos
                    .iter()
                    .filter_map(|r| r.path.as_deref())
                    .map(str::trim)
                    .find(|p| !p.is_empty())
            })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Home,
    Project(String),
}
impl Default for Scope {
    fn default() -> Self {
        Self::All
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CwdChoice {
    Unspecified,
    Detached,
    Path(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftTarget {
    pub owner: Owner,
    pub cwd: CwdChoice,
    pub explicit: bool,
}
impl DraftTarget {
    /// Insert only fields that the actual session.create contract accepts.
    pub fn apply(&self, params: &mut Value) {
        if let Some(object) = params.as_object_mut() {
            object.remove("cwd");
            object.remove("cwd_explicit");
            if let CwdChoice::Path(path) = &self.cwd {
                object.insert("cwd".into(), json!(path));
                object.insert("cwd_explicit".into(), json!(self.explicit));
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadKind {
    Tree,
    Entered(String),
    Overview(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    owner: Owner,
    generation: u64,
    kind: LoadKind,
}
impl Ticket {
    pub fn kind(&self) -> &LoadKind {
        &self.kind
    }
}
#[derive(Default)]
pub struct State {
    pub owner: Option<Owner>,
    pub scope: Scope,
    pub tree: Option<Tree>,
    pub entered: Option<Project>,
    pub expanded: std::collections::HashMap<String, Project>,
    pub revealed: std::collections::HashMap<String, usize>,
    pub error: Option<String>,
    pub loading: bool,
    generation: u64,
    pub show_all_sessions: bool,
}
impl State {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn bind(&mut self, owner: Owner) {
        if self.owner.as_ref() != Some(&owner) {
            self.retire();
            self.owner = Some(owner);
        }
    }
    pub fn retire(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.owner = None;
        self.scope = Scope::All;
        self.tree = None;
        self.entered = None;
        self.expanded.clear();
        self.revealed.clear();
        self.error = None;
        self.loading = false;
    }
    pub fn select(&mut self, scope: Scope) {
        if self.scope != scope {
            self.generation = self.generation.wrapping_add(1);
            self.scope = scope;
            self.entered = None;
            self.error = None;
            self.loading = false;
        }
    }
    pub fn begin(
        &mut self,
        kind: LoadKind,
        binding: &Binding,
    ) -> Result<(Ticket, PageRequest), String> {
        let owner = self.owner.clone().ok_or("Project owner is unavailable")?;
        binding.validate()?;
        if binding.stamp() != &owner.session.source {
            return Err("Project source changed".into());
        }
        if let LoadKind::Entered(id) = &kind {
            if self.scope != Scope::Project(id.clone())
                && !(self.scope == Scope::Home && id == "__no_project__")
            {
                return Err("Project is no longer entered".into());
            }
        }
        if matches!(&kind, LoadKind::Overview(_)) && self.scope != Scope::All {
            return Err("Project overview is no longer visible".into());
        }
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.error = None;
        let (method, body) = match &kind {
            LoadKind::Tree => (
                "projects.tree",
                json!({"profile": owner.session.profile, "preview_limit": if self.show_all_sessions { 2000 } else { 3 }}),
            ),
            LoadKind::Entered(id) | LoadKind::Overview(id) => (
                "projects.project_sessions",
                json!({"profile": owner.session.profile, "project_id": id}),
            ),
        };
        let provenance = match owner.session.profile_intent {
            ProfileIntent::Backend => ProfileProvenance::Backend(owner.session.profile.clone()),
            ProfileIntent::NativeSelf => ProfileProvenance::NativeSelf,
        };
        let request = binding.request(
            PageRequest::rpc(method, body)
                .profile(&owner.session.profile)
                .provenance(provenance),
        )?;
        Ok((
            Ticket {
                owner,
                generation: self.generation,
                kind,
            },
            request,
        ))
    }
    pub fn finish(
        &mut self,
        ticket: Ticket,
        binding: &Binding,
        result: Result<Value, String>,
    ) -> bool {
        if binding.validate().is_err()
            || binding.stamp() != &ticket.owner.session.source
            || self.owner.as_ref() != Some(&ticket.owner)
            || self.generation != ticket.generation
        {
            return false;
        }
        self.loading = false;
        let result = result.and_then(|value| match &ticket.kind {
            LoadKind::Tree => serde_json::from_value::<Tree>(value)
                .map(|tree| {
                    self.tree = Some(tree);
                })
                .map_err(|_| "Invalid project tree".to_owned()),
            LoadKind::Entered(id) | LoadKind::Overview(id) => {
                let project = value
                    .get("project")
                    .filter(|v| !v.is_null())
                    .ok_or("Project no longer exists")?;
                let project: Project = serde_json::from_value(project.clone())
                    .map_err(|_| "Invalid project details")?;
                if &project.id != id {
                    return Err("Project response did not match the request".into());
                }
                match &ticket.kind {
                    LoadKind::Entered(_) => self.entered = Some(project),
                    LoadKind::Overview(_) => {
                        self.revealed.insert(id.clone(), 50);
                        self.expanded.insert(id.clone(), project);
                    }
                    LoadKind::Tree => unreachable!(),
                }
                Ok(())
            }
        });
        self.error = result.err();
        true
    }
    /// Called at draft creation, with any clicked cwd already captured by the caller.
    pub fn capture(
        &self,
        explicit: CwdChoice,
        profile_default: Option<&str>,
    ) -> Result<DraftTarget, String> {
        let owner = self.owner.clone().ok_or("Project owner is unavailable")?;
        let (cwd, is_explicit) = match explicit {
            CwdChoice::Path(path) => (CwdChoice::Path(path), true),
            CwdChoice::Detached => (CwdChoice::Detached, false),
            CwdChoice::Unspecified => match &self.scope {
                Scope::Home => (CwdChoice::Detached, false),
                Scope::Project(id) => {
                    let project = self
                        .tree
                        .as_ref()
                        .and_then(|t| t.projects.iter().find(|p| &p.id == id))
                        .ok_or("Project is not loaded")?;
                    match project.root() {
                        Some(path) => (CwdChoice::Path(path.into()), true),
                        None => (
                            profile_default
                                .filter(|p| !p.trim().is_empty())
                                .map(|p| CwdChoice::Path(p.into()))
                                .unwrap_or(CwdChoice::Unspecified),
                            false,
                        ),
                    }
                }
                Scope::All => (
                    profile_default
                        .filter(|p| !p.trim().is_empty())
                        .map(|p| CwdChoice::Path(p.into()))
                        .unwrap_or(CwdChoice::Unspecified),
                    false,
                ),
            },
        };
        Ok(DraftTarget {
            owner,
            cwd,
            explicit: is_explicit,
        })
    }
}
#[cfg(test)]
mod tests;
