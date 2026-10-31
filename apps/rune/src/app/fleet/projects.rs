//! Project reads and actions stay bound to the source that produced them.
use super::*;
#[path = "projects/active.rs"]
pub(super) mod active;
#[path = "projects/create.rs"]
pub(super) mod create;
#[path = "projects/folders.rs"]
pub(super) mod folders;
#[path = "projects/rename.rs"]
pub(super) mod rename;
use crate::projects::{CwdChoice, LoadKind, Owner, Scope};
use crate::source::SessionOwner;

impl Viewer {
    pub fn projects_state(&self) -> &crate::projects::State {
        &self.fleet_state.projects
    }

    fn project_owner(&self) -> Option<(Owner, Binding)> {
        if self.status != "Connected" || self.error.is_some() || self.fixture {
            return None;
        }
        let binding = self.current_source_binding().ok()?;
        let owner = Owner {
            session: SessionOwner::with_intent(
                binding.stamp().clone(),
                &self.profile,
                self.profile_intent,
            ),
            epoch: self.connection_epoch,
        };
        Some((owner, binding))
    }

    fn project_owner_is_current(&self, owner: &Owner) -> bool {
        self.project_owner()
            .is_some_and(|(current, _)| current == *owner)
            && self.fleet_state.projects.owner.as_ref() == Some(owner)
    }

    fn project_load(&mut self, kind: LoadKind, binding: Binding) {
        let scope = self.composer_scope();
        self.fleet_state.projects.show_all_sessions = self.sidebar_state.prefs.show_all_sessions;
        match self.fleet_state.projects.begin(kind, &binding) {
            Ok((ticket, request)) => {
                let result = self.fleet_state.projects_network.request(
                    &scope,
                    binding.connection().clone(),
                    "projects",
                    request,
                );
                match result {
                    Ok(()) => self.fleet_state.projects_pending = Some((ticket, binding, scope)),
                    Err(error) => {
                        self.fleet_state
                            .projects
                            .finish(ticket, &binding, Err(error));
                    }
                }
            }
            Err(error) => self.fleet_state.projects.error = Some(error),
        }
    }

    pub(crate) fn refresh_project_tree(&mut self) {
        self.fleet_state.projects_network.invalidate();
        self.fleet_state.projects_pending = None;
        if let Some((owner, binding)) = self.project_owner() {
            self.fleet_state.projects.bind(owner);
            self.project_load(LoadKind::Tree, binding);
        } else {
            self.fleet_state.projects.retire();
        }
    }

    pub(super) fn poll_projects(&mut self) -> bool {
        let rename_changed = self.poll_project_rename()
            | self.poll_project_create()
            | self.poll_project_folders()
            | self.poll_project_active();
        let Some((owner, binding)) = self.project_owner() else {
            if self.fleet_state.projects.owner.is_some() {
                self.fleet_state.projects.retire();
                self.fleet_state.projects_network.invalidate();
                self.fleet_state.projects_pending = None;
                return true;
            }
            return rename_changed;
        };
        let mut changed = rename_changed;
        if self.fleet_state.projects.owner.as_ref() != Some(&owner) {
            self.fleet_state.projects_network.invalidate();
            self.fleet_state.projects_pending = None;
            self.fleet_state.projects.bind(owner);
            self.project_load(LoadKind::Tree, binding);
            changed = true;
        } else if self.fleet_state.projects.show_all_sessions
            != self.sidebar_state.prefs.show_all_sessions
        {
            self.refresh_project_tree();
            changed = true;
        }
        if let Some((ticket, binding, scope)) = self.fleet_state.projects_pending.take() {
            let replies = self.fleet_state.projects_network.poll(&scope);
            if let Some(reply) = replies.into_iter().find(|reply| !reply.progress) {
                let entered = matches!(ticket.kind(), LoadKind::Entered(_));
                let overview = match ticket.kind() {
                    LoadKind::Overview(id) => Some(id.clone()),
                    _ => None,
                };
                let accepted = self
                    .fleet_state
                    .projects
                    .finish(ticket, &binding, reply.result);
                changed |= accepted;
                if accepted
                    && entered
                    && self
                        .fleet_state
                        .project_create_refresh
                        .take()
                        .is_some_and(|owner| self.project_owner_is_current(&owner))
                {
                    self.refresh_project_tree();
                }
                if accepted
                    && self
                        .fleet_state
                        .project_active_refresh
                        .take()
                        .is_some_and(|owner| self.project_owner_is_current(&owner))
                {
                    self.refresh_project_tree();
                }
                if accepted && self.fleet_state.projects.error.is_some() {
                    if let (Some(id), Some(owner)) =
                        (overview, self.fleet_state.projects.owner.clone())
                    {
                        let scope = if id == "__no_project__" {
                            Scope::Home
                        } else {
                            Scope::Project(id)
                        };
                        let generation = self.fleet_state.projects.generation();
                        self.project_select(owner, scope, generation);
                    }
                }
            } else {
                self.fleet_state.projects_pending = Some((ticket, binding, scope));
            }
        }
        changed
    }

    pub fn project_select(&mut self, owner: Owner, scope: Scope, generation: u64) {
        if !self.project_owner_is_current(&owner)
            || self.fleet_state.projects.generation() != generation
        {
            return;
        }
        self.fleet_state.projects_network.invalidate();
        self.fleet_state.projects_pending = None;
        self.fleet_state.project_active_refresh = None;
        self.fleet_state.projects.select(scope.clone());
        if let Scope::Project(id) = &scope {
            let should_activate = self.fleet_state.projects.tree.as_ref().is_some_and(|tree| {
                tree.active_id.as_deref() != Some(id.as_str())
                    && tree
                        .projects
                        .iter()
                        .any(|p| p.id == *id && !p.is_auto && !p.is_no_project)
            });
            if should_activate {
                if let Some((_, binding)) = self.project_owner() {
                    self.project_set_active(
                        owner,
                        scope.clone(),
                        self.fleet_state.projects.generation(),
                        Some(id.clone()),
                        binding,
                    );
                }
            }
        }
        let id = match scope {
            Scope::Project(id) => Some(id),
            Scope::Home => Some("__no_project__".into()),
            Scope::All => None,
        };
        if let Some((_, binding)) = self.project_owner() {
            if let Some(id) = id {
                self.project_load(LoadKind::Entered(id), binding);
            } else if self.fleet_state.projects.tree.is_none() {
                self.project_load(LoadKind::Tree, binding);
            }
        }
    }

    pub fn project_retry(&mut self, owner: Owner, generation: u64) {
        if !self.project_owner_is_current(&owner)
            || self.fleet_state.projects.generation() != generation
        {
            return;
        }
        let kind = match &self.fleet_state.projects.scope {
            Scope::Project(id) if self.fleet_state.projects.tree.is_some() => {
                LoadKind::Entered(id.clone())
            }
            Scope::Home if self.fleet_state.projects.tree.is_some() => {
                LoadKind::Entered("__no_project__".into())
            }
            _ => LoadKind::Tree,
        };
        if let Some((_, binding)) = self.project_owner() {
            self.project_load(kind, binding);
        }
    }

    pub fn project_open_session(
        &mut self,
        owner: Owner,
        scope: Scope,
        generation: u64,
        id: String,
    ) {
        if self.project_owner_is_current(&owner)
            && self.fleet_state.projects.scope == scope
            && self.fleet_state.projects.generation() == generation
            && self
                .fleet_state
                .projects
                .entered
                .as_ref()
                .is_some_and(|project| {
                    let selected = match &self.fleet_state.projects.scope {
                        Scope::Project(selected) => selected == &project.id,
                        Scope::Home => project.is_no_project,
                        Scope::All => false,
                    };
                    selected
                        && project
                            .repos
                            .iter()
                            .flat_map(|repo| &repo.groups)
                            .flat_map(|lane| &lane.sessions)
                            .any(|session| {
                                session.get("id").and_then(|v| v.as_str()) == Some(&id)
                                    && session
                                        .get("profile")
                                        .and_then(|v| v.as_str())
                                        .is_none_or(|profile| profile == owner.session.profile)
                            })
                })
        {
            self.open_session(id);
        }
    }

    pub fn project_expand(&mut self, owner: Owner, scope: Scope, generation: u64, id: String) {
        if !self.project_owner_is_current(&owner)
            || self.fleet_state.projects.scope != scope
            || scope != Scope::All
            || self.fleet_state.projects.generation() != generation
            || !self
                .fleet_state
                .projects
                .tree
                .as_ref()
                .is_some_and(|tree| tree.projects.iter().any(|p| p.id == id))
        {
            return;
        }
        self.fleet_state.projects_network.invalidate();
        self.fleet_state.projects_pending = None;
        if let Some((_, binding)) = self.project_owner() {
            self.project_load(LoadKind::Overview(id), binding);
        }
    }
    pub fn project_reveal(&mut self, owner: Owner, scope: Scope, generation: u64, id: String) {
        if self.project_owner_is_current(&owner)
            && self.fleet_state.projects.scope == scope
            && scope == Scope::All
            && self.fleet_state.projects.generation() == generation
            && self.fleet_state.projects.expanded.contains_key(&id)
        {
            *self.fleet_state.projects.revealed.entry(id).or_insert(50) += 50;
        }
    }

    pub fn project_toggle(&mut self, owner: Owner, scope: Scope, generation: u64, key: String) {
        if self.project_owner_is_current(&owner)
            && self.fleet_state.projects.scope == scope
            && self.fleet_state.projects.generation() == generation
        {
            self.sidebar_action(crate::sidebar::Action::Collapse(key));
        }
    }

    pub fn project_open_preview(
        &mut self,
        owner: Owner,
        scope: Scope,
        generation: u64,
        project_id: String,
        id: String,
    ) {
        if !self.project_owner_is_current(&owner)
            || self.fleet_state.projects.scope != scope
            || scope != Scope::All
            || self.fleet_state.projects.generation() != generation
        {
            return;
        }
        let matches = |session: &&serde_json::Value| {
            session.get("id").and_then(|v| v.as_str()) == Some(id.as_str())
                && session
                    .get("profile")
                    .and_then(|v| v.as_str())
                    .is_none_or(|p| p == owner.session.profile)
        };
        let session = self
            .fleet_state
            .projects
            .tree
            .as_ref()
            .and_then(|tree| {
                tree.projects
                    .iter()
                    .find(|project| project.id == project_id)
            })
            .and_then(|project| {
                project.preview_sessions.iter().find(matches).or_else(|| {
                    self.fleet_state
                        .projects
                        .expanded
                        .get(&project_id)
                        .and_then(|expanded| {
                            expanded
                                .repos
                                .iter()
                                .flat_map(|repo| &repo.groups)
                                .flat_map(|lane| &lane.sessions)
                                .find(matches)
                        })
                })
            });
        let Some(session) = session else {
            return;
        };
        let title = session
            .get("title")
            .and_then(|v| v.as_str())
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("Untitled session")
            .to_owned();
        self.cancel_device_route();
        self.layout.open("workspace");
        if self.route == crate::pages::Route::Settings {
            self.settings.vault.dismiss();
        }
        self.route = crate::pages::Route::Chat;
        self.open_target(
            crate::app::ConversationTarget::Session {
                profile: owner.session.profile,
                profile_intent: owner.session.profile_intent,
                id,
                title,
            },
            false,
        );
    }

    pub(in crate::app) fn project_route_is_scoped(&self) -> bool {
        self.project_owner()
            .is_some_and(|(owner, _)| self.project_owner_is_current(&owner))
            && matches!(self.fleet_state.projects.scope, Scope::Project(_))
    }

    pub(in crate::app) fn project_draft_target(
        &self,
    ) -> Result<Option<crate::projects::DraftTarget>, String> {
        let Some((owner, _)) = self.project_owner() else {
            return Ok(None);
        };
        if !self.project_owner_is_current(&owner) {
            if self.fleet_state.projects.owner.is_some() {
                return Err("The project source changed; wait for projects to reload".into());
            }
            return Ok(Some(crate::projects::DraftTarget {
                owner,
                cwd: self
                    .client_settings
                    .prefs
                    .default_project_dir
                    .as_ref()
                    .filter(|path| !path.trim().is_empty())
                    .map(|path| CwdChoice::Path(path.clone()))
                    .unwrap_or(CwdChoice::Unspecified),
                explicit: false,
            }));
        }
        self.fleet_state
            .projects
            .capture(
                CwdChoice::Unspecified,
                self.client_settings.prefs.default_project_dir.as_deref(),
            )
            .map(Some)
    }

    pub(in crate::app) fn apply_project_draft_target(
        &mut self,
        target: Option<crate::projects::DraftTarget>,
    ) {
        let Some(target) = target else { return };
        if !self
            .project_owner()
            .is_some_and(|(owner, _)| owner == target.owner)
        {
            self.compose_error =
                Some("The captured project belongs to a different or retired source".into());
            return;
        }
        let scope = self.composer_scope();
        if self.composer.draft(&scope).is_some() {
            return;
        }
        let draft = self.composer.draft_mut(scope);
        draft.cwd = match &target.cwd {
            CwdChoice::Path(path) => Some(path.clone()),
            _ => None,
        };
        draft.cwd_explicit = target.explicit;
        self.fleet_state
            .project_drafts
            .insert(draft.policy_id.clone(), target);
    }

    pub(in crate::app) fn capture_project_draft(&mut self) {
        match self.project_draft_target() {
            Ok(target) => self.apply_project_draft_target(target),
            Err(error) => self.compose_error = Some(error),
        }
    }

    pub(in crate::app) fn clear_project_draft_provenance(&mut self, policy_id: &str) {
        self.fleet_state.project_drafts.remove(policy_id);
    }

    pub(in crate::app) fn project_send_cwd(
        &self,
        draft: &crate::composer::Draft,
        canonical_bot: bool,
    ) -> (Option<String>, bool) {
        if canonical_bot
            && self
                .fleet_state
                .project_drafts
                .get(&draft.policy_id)
                .is_some_and(|target| match &target.cwd {
                    CwdChoice::Path(path) => {
                        draft.cwd.as_ref() == Some(path) && draft.cwd_explicit == target.explicit
                    }
                    _ => draft.cwd.is_none(),
                })
        {
            (None, false)
        } else {
            (draft.cwd.clone(), draft.cwd_explicit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn viewer() -> Viewer {
        let mut viewer = Viewer::empty();
        viewer.connection = Some(Connection::Local {
            home: "/tmp/projects-app-source-a".into(),
        });
        viewer.status = "Connected".into();
        viewer.profile = "same".into();
        viewer.connection_epoch = 20;
        let (owner, binding) = viewer.project_owner().unwrap();
        viewer.fleet_state.projects.bind(owner);
        let (ticket, _) = viewer
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        assert!(viewer.fleet_state.projects.finish(
            ticket,
            &binding,
            Ok(json!({
                "projects": [{"id":"same", "label":"One", "path":"/first", "sessionIds":["same"]}]
            }))
        ));
        viewer
    }

    #[test]
    fn old_project_row_cannot_open_after_leaving_and_reentering() {
        let mut viewer = viewer();
        let (owner, binding) = viewer.project_owner().unwrap();
        let scope = Scope::Project("same".into());
        viewer.fleet_state.projects.select(scope.clone());
        let (ticket, _) = viewer
            .fleet_state
            .projects
            .begin(LoadKind::Entered("same".into()), &binding)
            .unwrap();
        assert!(viewer.fleet_state.projects.finish(ticket, &binding, Ok(json!({"project": {
            "id":"same", "label":"One", "repos":[{"id":"r","label":"R", "groups":[{"id":"g", "label":"G", "sessions":[{"id":"same"}]}]}]
        }}))));
        let generation = viewer.fleet_state.projects.generation();
        viewer.fleet_state.projects.select(Scope::All);
        viewer.fleet_state.projects.select(scope.clone());
        viewer.selected_session = Some("current".into());
        viewer.project_open_session(owner, scope, generation, "same".into());
        assert_eq!(viewer.selected_session.as_deref(), Some("current"));
    }

    #[test]
    fn stale_navigation_action_cannot_restore_an_old_selection() {
        let mut viewer = viewer();
        let owner = viewer.fleet_state.projects.owner.clone().unwrap();
        let generation = viewer.fleet_state.projects.generation();
        viewer.fleet_state.projects.select(Scope::Home);
        viewer.project_select(owner.clone(), Scope::Project("same".into()), generation);
        viewer.project_retry(owner, generation);
        assert_eq!(viewer.fleet_state.projects.scope, Scope::Home);
        assert!(!viewer.fleet_state.projects.loading);
    }

    #[test]
    fn captured_target_is_fenced_and_bot_preserves_a_user_folder_change() {
        let mut viewer = viewer();
        viewer
            .fleet_state
            .projects
            .select(Scope::Project("same".into()));
        let target = viewer.project_draft_target().unwrap().unwrap();
        viewer.fleet_state.projects.select(Scope::Home);
        viewer.apply_project_draft_target(Some(target.clone()));
        let scope = viewer.composer_scope();
        let draft = viewer.composer.draft(&scope).unwrap();
        assert_eq!(draft.cwd.as_deref(), Some("/first"));
        assert_eq!(viewer.project_send_cwd(draft, true), (None, false));
        viewer.composer.draft_mut(scope.clone()).cwd = Some("/chosen".into());
        let draft = viewer.composer.draft(&scope).unwrap();
        assert_eq!(
            viewer.project_send_cwd(draft, true),
            (Some("/chosen".into()), true)
        );

        let mut other = self::viewer();
        other.connection = Some(Connection::Local {
            home: "/tmp/projects-app-source-b".into(),
        });
        other.connection_epoch += 1;
        other.apply_project_draft_target(Some(target));
        assert!(other.composer.draft(&other.composer_scope()).is_none());
    }

    #[test]
    fn stale_source_click_is_inert_and_draft_retains_its_first_project() {
        let mut viewer = viewer();
        let owner = viewer.fleet_state.projects.owner.clone().unwrap();
        viewer
            .fleet_state
            .projects
            .select(Scope::Project("same".into()));
        viewer.capture_project_draft();
        let scope = viewer.composer_scope();
        assert_eq!(
            viewer.composer.draft(&scope).unwrap().cwd.as_deref(),
            Some("/first")
        );
        let draft = viewer.composer.draft(&scope).unwrap();
        assert_eq!(
            viewer.project_send_cwd(draft, false),
            (Some("/first".into()), true)
        );
        assert_eq!(viewer.project_send_cwd(draft, true), (None, false));
        viewer.fleet_state.projects.select(Scope::Home);
        viewer.capture_project_draft();
        assert_eq!(
            viewer.composer.draft(&scope).unwrap().cwd.as_deref(),
            Some("/first")
        );
        viewer.connection = Some(Connection::Local {
            home: "/tmp/projects-app-source-b".into(),
        });
        let generation = viewer.fleet_state.projects.generation();
        viewer.project_select(owner, Scope::Project("same".into()), generation);
        assert_eq!(viewer.fleet_state.projects.scope, Scope::Home);
    }
}

#[cfg(test)]
mod draft_origin_tests {
    use super::*;
    #[test]
    fn new_chat_rotates_origin_and_keeps_the_prior_editor_input() {
        let mut viewer = Viewer::empty();
        crate::app::reader_scene(&mut viewer);
        viewer.init_editors(std::sync::Arc::new(|| {}));
        viewer.new_draft_on_current_source();
        let first = viewer.composer_scope();
        let first_editor = viewer.composer.editor_id().clone();
        viewer
            .composer
            .draft_mut(first.clone())
            .text
            .replace_text("kept");
        viewer.new_draft_on_current_source();
        let second = viewer.composer_scope();
        assert_ne!(first.draft_origin, second.draft_origin);
        assert_ne!(first_editor, *viewer.composer.editor_id());
        assert_eq!(
            viewer
                .editor_mut(&first_editor, &first)
                .unwrap()
                .draft(&first)
                .unwrap()
                .text
                .text,
            "kept"
        );
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use std::{
        net::TcpListener,
        sync::{Arc, mpsc},
        time::Duration,
    };

    #[test]
    fn show_all_sidebar_action_refreshes_real_tree_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for limit in [3, 2000, 3] {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let request: serde_json::Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                assert_eq!(request["method"], "projects.tree");
                assert_eq!(
                    request["params"],
                    serde_json::json!({"profile":"work", "preview_limit":limit})
                );
                socket.send(tungstenite::Message::Text(serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "result":{"projects":[{"id":"p", "label":"P", "path":format!("/{limit}")}]}}).to_string().into())).unwrap();
            }
        });
        let mut viewer = Viewer::empty();
        viewer.connection = Some(Connection::Url {
            endpoint,
            token: Some("fixture".into()),
        });
        viewer.connection_epoch = 3;
        viewer.profile = "work".into();
        viewer.status = "Connected".into();
        viewer.fleet_state.projects_network.init(Arc::new(|| {}));
        viewer.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Project;
        viewer.poll_projects();
        for (index, limit) in [3, 2000, 3].into_iter().enumerate() {
            if index > 0 {
                viewer.sidebar_action(crate::sidebar::Action::Toggle("show_all_sessions"));
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                viewer.poll_projects();
                if !viewer.fleet_state.projects.loading
                    && viewer
                        .fleet_state
                        .projects
                        .tree
                        .as_ref()
                        .and_then(|t| t.projects.first())
                        .and_then(|p| p.path.as_deref())
                        == Some(format!("/{limit}").as_str())
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "tree did not arrive: {:?}",
                    viewer.fleet_state.projects.error
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(viewer.sidebar_state.prefs.show_all_sessions, limit == 2000);
        }
        server.join().unwrap();
    }

    #[test]
    fn fresh_project_and_home_send_real_session_create_with_captured_cwd() {
        const CHILD: &str = "HERMES_PROJECT_DISPATCH_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let root = std::env::temp_dir()
                .join(format!("hermes-project-dispatch-{}", uuid::Uuid::new_v4()));
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "app::fleet::projects::dispatch_tests::fresh_project_and_home_send_real_session_create_with_captured_cwd", "--nocapture"])
                .env(CHILD, "1").env("XDG_STATE_HOME", &root).status().unwrap();
            assert!(status.success());
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
        for (project, voice) in [(true, false), (false, false), (true, true)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let (send, recv) = mpsc::channel();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let request: serde_json::Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                send.send(request).unwrap();
                let _ = socket.close(None);
            });
            let mut viewer = Viewer::empty();
            viewer.connection = Some(Connection::Url {
                endpoint,
                token: Some("fixture".into()),
            });
            viewer.connection_epoch = 1;
            viewer.profile = "work".into();
            viewer.status = "Connected".into();
            viewer.loading = false;
            viewer.connection_form = false;
            viewer.conversation.init(Arc::new(|| {}));
            let (owner, binding) = viewer.project_owner().unwrap();
            viewer.fleet_state.projects.bind(owner);
            let (ticket, _) = viewer
                .fleet_state
                .projects
                .begin(LoadKind::Tree, &binding)
                .unwrap();
            assert!(viewer.fleet_state.projects.finish(
                ticket,
                &binding,
                Ok(serde_json::json!({"projects":[{"id":"p","label":"P","path":"/project"}]}))
            ));
            viewer.fleet_state.projects.select(if project {
                Scope::Project("p".into())
            } else {
                Scope::Home
            });
            viewer.client_settings.prefs.default_project_dir = Some("/configured".into());
            viewer.new_draft_on_current_source();
            let scope = viewer.composer_scope();
            viewer.composer.draft_mut(scope).text.replace_text("hello");
            viewer
                .fleet_state
                .projects
                .select(if project { Scope::Home } else { Scope::All });
            if voice {
                viewer.send_voice_text(viewer.composer_scope(), "spoken".into(), None);
            } else {
                viewer.send_draft();
            }
            let request = recv.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(request["method"], "session.create");
            assert_eq!(request["params"]["profile"], "work");
            if project {
                assert_eq!(request["params"]["cwd"], "/project");
                assert_eq!(request["params"]["cwd_explicit"], true);
            } else {
                assert!(request["params"].get("cwd").is_none());
            }
            server.join().unwrap();
        }
    }
    #[test]
    fn slash_expansion_sends_create_on_the_captured_project_source() {
        const CHILD: &str = "HERMES_PROJECT_SLASH_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let root =
                std::env::temp_dir().join(format!("hermes-project-slash-{}", uuid::Uuid::new_v4()));
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "app::fleet::projects::dispatch_tests::slash_expansion_sends_create_on_the_captured_project_source", "--nocapture"])
                .env(CHILD, "1").env("XDG_STATE_HOME", &root).status().unwrap();
            assert!(status.success());
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (send, recv) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let command: serde_json::Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(command["method"], "command.dispatch");
            socket.send(tungstenite::Message::Text(serde_json::json!({"jsonrpc":"2.0","id":command["id"],"result":{"type":"send","message":"expanded"}}).to_string().into())).unwrap();
            drop(socket);
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let request: serde_json::Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            send.send(request).unwrap();
            let _ = socket.close(None);
        });
        let mut viewer = Viewer::empty();
        viewer.connection = Some(Connection::Url {
            endpoint,
            token: Some("fixture".into()),
        });
        viewer.connection_epoch = 3;
        viewer.profile = "work".into();
        viewer.status = "Connected".into();
        viewer.loading = false;
        viewer.connection_form = false;
        viewer.conversation.init(Arc::new(|| {}));
        viewer.chat.init(Arc::new(|| {}));
        let (owner, binding) = viewer.project_owner().unwrap();
        viewer.fleet_state.projects.bind(owner);
        let (ticket, _) = viewer
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        assert!(viewer.fleet_state.projects.finish(
            ticket,
            &binding,
            Ok(serde_json::json!({"projects":[{"id":"p","label":"P","path":"/project"}]}))
        ));
        viewer
            .fleet_state
            .projects
            .select(Scope::Project("p".into()));
        viewer.new_draft_on_current_source();
        let scope = viewer.composer_scope();
        viewer.chat.presentation(scope.clone()).slash_catalog = serde_json::json!({});
        viewer.dispatch_chat_slash(scope, "/example".into());
        viewer.fleet_state.projects.select(Scope::Home);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let request = loop {
            viewer.poll_chat(&mut Tx::default());
            if let Ok(request) = recv.try_recv() {
                break request;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "slash create did not arrive"
            );
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(request["method"], "session.create");
        assert_eq!(request["params"]["profile"], "work");
        assert_eq!(request["params"]["cwd"], "/project");
        assert_eq!(request["params"]["cwd_explicit"], true);
        server.join().unwrap();
    }
    #[test]
    fn bot_open_does_not_adopt_a_fresh_project_and_keeps_explicit_folder() {
        const CHILD: &str = "HERMES_PROJECT_BOT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let root =
                std::env::temp_dir().join(format!("hermes-project-bot-{}", uuid::Uuid::new_v4()));
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "app::fleet::projects::dispatch_tests::bot_open_does_not_adopt_a_fresh_project_and_keeps_explicit_folder", "--nocapture"])
                .env(CHILD, "1").env("XDG_STATE_HOME", &root).status().unwrap();
            assert!(status.success());
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
        for choose in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let (send, recv) = mpsc::channel();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let lookup: serde_json::Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                assert_eq!(lookup["method"], "session.list");
                socket.send(tungstenite::Message::Text(serde_json::json!({"jsonrpc":"2.0","id":lookup["id"],"result":{"sessions":[]}}).to_string().into())).unwrap();
                let create: serde_json::Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                send.send(create).unwrap();
                let _ = socket.close(None);
            });
            let mut viewer = Viewer::empty();
            viewer.connection = Some(Connection::Url {
                endpoint,
                token: Some("fixture".into()),
            });
            viewer.connection_epoch = 5;
            viewer.profile = "work".into();
            viewer.profiles = vec![Profile {
                name: "work".into(),
                label: "Work".into(),
                description: String::new(),
                canonical_session: None,
            }];
            viewer.status = "Connected".into();
            viewer.loading = false;
            viewer.connection_form = false;
            viewer.conversation.init(Arc::new(|| {}));
            viewer.init_editors(Arc::new(|| {}));
            let (send, _receive) = mpsc::channel();
            let (_reply, receive) = mpsc::channel();
            viewer.network = Some(Network { send, receive });
            let (owner, binding) = viewer.project_owner().unwrap();
            viewer.fleet_state.projects.bind(owner);
            let (ticket, _) = viewer
                .fleet_state
                .projects
                .begin(LoadKind::Tree, &binding)
                .unwrap();
            assert!(viewer.fleet_state.projects.finish(
                ticket,
                &binding,
                Ok(serde_json::json!({"projects":[{"id":"p","label":"P","path":"/project"}]}))
            ));
            viewer
                .fleet_state
                .projects
                .select(Scope::Project("p".into()));
            viewer.new_draft_on_current_source();
            let fresh = viewer.composer_scope();
            assert_eq!(
                viewer.composer.draft(&fresh).unwrap().cwd.as_deref(),
                Some("/project")
            );
            viewer.open_bot("work".into());
            viewer.apply(Reply {
                intent: viewer.intent,
                result: Ok(Update::BotOpened {
                    profile: "work".into(),
                    profiles: viewer.profiles.clone(),
                    sessions: vec![],
                    chat: None,
                }),
            });
            let bot = viewer.composer_scope();
            assert_ne!(fresh, bot);
            if choose {
                viewer.change_workspace(bot.clone(), "/project".into());
            }
            viewer
                .composer
                .draft_mut(bot)
                .text
                .replace_text("hello bot");
            viewer.send_draft();
            let create = recv.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(create["method"], "session.create");
            assert_eq!(create["params"]["title"], "Bot Chat");
            if choose {
                assert_eq!(create["params"]["cwd"], "/project");
                assert_eq!(create["params"]["cwd_explicit"], true);
            } else {
                assert!(create["params"].get("cwd").is_none());
            }
            server.join().unwrap();
        }
    }
}
#[cfg(test)]
mod preview_runtime_tests {
    use super::*;
    use rune::{Cx, El, Env, Runtime};
    use serde_json::json;

    fn view(s: &Viewer, _: &mut Cx<Viewer>) -> El {
        rune::col().size(700., 4000.).child(
            crate::view::projects::navigation(
                s,
                crate::view::projects::Actions {
                    select: Viewer::project_select,
                    retry: Viewer::project_retry,
                    open_session: Viewer::project_open_session,
                    open_preview: Viewer::project_open_preview,
                    toggle: Viewer::project_toggle,
                    expand: Viewer::project_expand,
                    reveal: Viewer::project_reveal,
                },
                &rune_theme_opencode::kit(true),
                &mut Vec::new(),
            )
            .h(2000.),
        )
    }
    #[test]
    fn mounted_tree_preview_tracks_response_and_opens_fourth() {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Local {
            home: "/tmp/synthetic-preview-source".into(),
        });
        s.connection_epoch = 1;
        s.profile = "work".into();
        s.status = "Connected".into();
        s.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Project;
        let (send, requests) = std::sync::mpsc::channel();
        let (_reply, receive) = std::sync::mpsc::channel();
        s.network = Some(Network { send, receive });
        let (owner, binding) = s.project_owner().unwrap();
        s.fleet_state.projects.bind(owner);
        let mut rt = Runtime::new(s, view, (700., 700.), Env::new(true, 1.));
        for count in [3, 4, 3] {
            rt.update(|s, _| {
                s.fleet_state.projects.show_all_sessions = count > 3;
                let (ticket, _) = s.fleet_state.projects.begin(LoadKind::Tree, &binding).unwrap();
                let rows: Vec<_> = (1..=count).map(|i| json!({"id":format!("preview-{i}"),"title":format!("Project chat {i}"),"profile":"work"})).collect();
                assert!(s.fleet_state.projects.finish(ticket, &binding, Ok(json!({"projects":[{"id":"p", "label":"Project", "sessionCount":4, "previewSessions":rows}]}))));
            });
            rt.invalidate();
            rt.frame(1.0);
            for i in 1..=count {
                assert!(rt.find(&format!("project-preview-p-preview-{i}")).is_some());
            }
            assert_eq!(rt.find("project-preview-p-preview-4").is_some(), count == 4);
            if count == 4 {
                let (x, y) = rt.find("project-preview-p-preview-4").unwrap();
                rt.pointer_move(x, y);
                rt.frame(0.);
                rt.pointer_down(x, y);
                rt.frame(0.);
                rt.pointer_up(x, y);
                rt.frame(0.);
                assert_eq!(rt.state.selected_session.as_deref(), Some("preview-4"));
                let request = requests.try_recv().unwrap();
                assert!(
                    matches!(request.command, Command::History(ref profile, ref id) if profile == "work" && id == "preview-4")
                );
                assert_eq!(
                    request.profile_intent,
                    crate::source::ProfileIntent::Backend
                );
            }
        }
    }
    #[test]
    fn network_action_mounts_widened_tree_and_opens_new_row() {
        const CHILD: &str = "HERMES_PROJECT_PREVIEW_PRIVATE_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let root =
                std::env::temp_dir().join(format!("project-preview-{}", uuid::Uuid::new_v4()));
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "app::fleet::projects::preview_runtime_tests::network_action_mounts_widened_tree_and_opens_new_row", "--nocapture"])
                .env(CHILD, "1").env("XDG_STATE_HOME", &root).env("XDG_CONFIG_HOME", &root).status().unwrap();
            assert!(status.success());
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        use std::{
            net::TcpListener,
            sync::Arc,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for limit in [3, 2000, 3] {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut socket = tungstenite::accept_hdr(
                    stream,
                    |request: &tungstenite::handshake::server::Request,
                     response: tungstenite::handshake::server::Response| {
                        assert!(
                            request
                                .uri()
                                .query()
                                .unwrap_or_default()
                                .contains("token=fixture")
                        );
                        Ok(response)
                    },
                )
                .unwrap();
                let request: serde_json::Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                assert_eq!(request["method"], "projects.tree");
                assert_eq!(
                    request["params"],
                    json!({"profile":"work", "preview_limit":limit})
                );
                let count = if limit == 2000 { 4 } else { 3 };
                let rows: Vec<_> = (1..=count).map(|i| json!({"id":format!("preview-{i}"),"title":format!("Tree session {i}"),"profile":"work"})).collect();
                socket.send(tungstenite::Message::Text(json!({"jsonrpc":"2.0", "id":request["id"], "result":{"projects":[{"id":"p", "label":"P", "sessionCount":4, "previewSessions":rows}]}}).to_string().into())).unwrap();
            }
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request,
                 response: tungstenite::handshake::server::Response| {
                    assert!(
                        request
                            .uri()
                            .query()
                            .unwrap_or_default()
                            .contains("token=fixture")
                    );
                    Ok(response)
                },
            )
            .unwrap();
            let request: serde_json::Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(request["method"], "projects.project_sessions");
            assert_eq!(
                request["params"],
                json!({"profile":"work", "project_id":"p"})
            );
            let sessions: Vec<_> = (1..=4).map(|i| json!({"id":format!("preview-{i}"), "title":format!("Tree session {i}"), "profile":"work"})).collect();
            socket.send(tungstenite::Message::Text(json!({"jsonrpc":"2.0", "id":request["id"], "result":{"project":{"id":"p", "label":"P", "repos":[{"id":"r", "label":"R", "groups":[{"id":"g", "label":"G", "sessions":sessions}]}]}}}).to_string().into())).unwrap();
        });
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Url {
            endpoint,
            token: Some("fixture".into()),
        });
        s.connection_epoch = 4;
        s.profile = "work".into();
        s.status = "Connected".into();
        s.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Project;
        s.fleet_state.projects_network.init(Arc::new(|| {}));
        let (send, requests) = std::sync::mpsc::channel();
        let (_reply, receive) = std::sync::mpsc::channel();
        s.network = Some(Network { send, receive });
        let mut rt = Runtime::new(
            s,
            crate::view::projects::sidebar_test_view,
            (700., 700.),
            Env::new(true, 1.),
        );
        rt.update(|s, _| {
            s.poll_projects();
        });
        for (index, limit) in [3, 2000, 3].into_iter().enumerate() {
            if index > 0 {
                rt.frame(1.0);
                let (x, y) = rt.find("sessions-filter").unwrap();
                rt.pointer_down(x, y);
                rt.pointer_up(x, y);
                rt.frame(1.0);
                let item = rt
                    .elements()
                    .into_iter()
                    .find(|e| {
                        e.role == "menuitem" && e.label.as_deref() == Some("Show all sessions")
                    })
                    .expect("project grouping checkbox");
                let (x, y) = rt.find(item.key.as_deref().unwrap()).unwrap();
                rt.pointer_down(x, y);
                rt.pointer_up(x, y);
                rt.frame(1.0);
                if rt.state.sidebar_state.filter_open {
                    let (x, y) = rt.find("sessions-filter").unwrap();
                    rt.pointer_down(x, y);
                    rt.pointer_up(x, y);
                    rt.frame(1.0);
                }
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                rt.update(|s, _| {
                    s.poll_projects();
                });
                let count = rt
                    .state
                    .projects_state()
                    .tree
                    .as_ref()
                    .and_then(|t| t.projects.first())
                    .map(|p| p.preview_sessions.len());
                if !rt.state.projects_state().loading
                    && count == Some(if limit == 2000 { 4 } else { 3 })
                {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
            rt.invalidate();
            rt.frame(1.0);
            assert_eq!(
                rt.find("project-preview-p-preview-4").is_some(),
                limit == 2000
            );
            if limit == 2000 {
                let (x, y) = rt.find("project-preview-p-preview-4").unwrap();

                rt.pointer_move(x, y);
                rt.frame(0.);
                rt.pointer_down(x, y);
                rt.frame(0.);
                rt.pointer_up(x, y);
                rt.frame(0.);
                let request = requests.try_recv().unwrap();
                assert!(
                    matches!(request.command, Command::History(ref p, ref id) if p == "work" && id == "preview-4")
                );
            }
        }
        rt.frame(1.0);
        let (x, y) = rt
            .find("project-expand-p")
            .expect("per-project show all action");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            rt.update(|s, _| {
                s.poll_projects();
            });
            if rt.state.projects_state().expanded.contains_key("p") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "inline hydration failed: {:?}",
                rt.state.projects_state().error
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        rt.invalidate();
        rt.frame(1.0);
        assert!(rt.find("project-preview-p-preview-4").is_some());
        assert_eq!(rt.state.projects_state().scope, Scope::All);
        server.join().unwrap();
    }
    #[test]
    fn full_sidebar_uses_tree_not_duplicate_recents_and_menu_is_project_only() {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Local {
            home: "/tmp/synthetic-menu".into(),
        });
        s.connection_epoch = 1;
        s.profile = "work".into();
        s.status = "Connected".into();
        s.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Project;
        s.sidebar_state.recents.loaded = true;
        s.sidebar_state
            .recents
            .rows
            .push(crate::gateway::sidebar::SidebarSession {
                id: "recent-only".into(),
                title: Some("Separate recent".into()),
                profile: Some("work".into()),
                ..Default::default()
            });
        let (owner, binding) = s.project_owner().unwrap();
        s.fleet_state.projects.bind(owner);
        let (ticket, _) = s
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        assert!(s.fleet_state.projects.finish(ticket, &binding, Ok(json!({"projects":[{"id":"p", "label":"P", "sessionCount":1, "previewSessions":[{"id":"tree-only", "title":"Tree row"}]}]}))));
        let mut rt = Runtime::new(
            s,
            crate::view::projects::sidebar_test_view,
            (700., 700.),
            Env::new(true, 1.),
        );
        rt.frame(1.0);
        assert!(rt.find("project-preview-p-tree-only").is_some());
        assert!(rt.find("session-work-recent-only").is_none());
        rt.update(|s, _| s.sidebar_state.filter_open = true);
        rt.frame(1.0);
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.role == "menuitem" && e.label.as_deref() == Some("Show all sessions"))
        );
        rt.update(|s, _| {
            s.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Date;
        });
        rt.frame(1.0);
        assert!(
            !rt.elements()
                .iter()
                .any(|e| e.role == "menuitem" && e.label.as_deref() == Some("Show all sessions"))
        );
        assert!(rt.find("session-work-recent-only").is_some());
    }
    #[test]
    fn expanded_lanes_stay_inline_and_reveal_loaded_rows() {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Local {
            home: "/tmp/synthetic-expanded".into(),
        });
        s.connection_epoch = 2;
        s.profile = "work".into();
        s.status = "Connected".into();
        s.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Project;
        let (owner, binding) = s.project_owner().unwrap();
        s.fleet_state.projects.bind(owner);
        let (ticket, _) = s
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        assert!(s.fleet_state.projects.finish(ticket, &binding, Ok(json!({"projects":[{"id":"p", "label":"P", "sessionCount":55, "previewSessions":[{"id":"preview-1","title":"First"}]}]}))));
        let (ticket, request) = s
            .fleet_state
            .projects
            .begin(LoadKind::Overview("p".into()), &binding)
            .unwrap();
        let _ = request;
        let rows: Vec<_> = (1..=55).map(|i| json!({"id":format!("preview-{i}"), "title":format!("Chat {i}"), "profile":"work", "last_active":56-i})).collect();
        assert!(s.fleet_state.projects.finish(ticket, &binding, Ok(json!({"project":{"id":"p", "label":"P", "repos":[{"id":"r", "label":"R", "groups":[{"id":"g", "label":"G", "sessions":rows}]}]}}))));
        let mut rt = Runtime::new(s, view, (700., 4000.), Env::new(true, 1.));
        rt.frame(0.);
        assert!(rt.find("project-preview-p-preview-50").is_some());
        assert!(rt.find("project-preview-p-preview-51").is_none());
        let owner = rt.state.fleet_state.projects.owner.clone().unwrap();
        let generation = rt.state.fleet_state.projects.generation();
        rt.update(|s, _| s.project_reveal(owner, Scope::All, generation, "p".into()));
        rt.invalidate();
        rt.frame(0.);
        assert!(rt.find("project-preview-p-preview-55").is_some());
        assert_eq!(rt.state.fleet_state.projects.scope, Scope::All);
    }
    #[test]
    fn stale_equal_id_preview_cannot_open_new_source_or_new_generation() {
        let mut s = Viewer::empty();
        s.connection = Some(Connection::Local {
            home: "/tmp/synthetic-preview-a".into(),
        });
        s.connection_epoch = 1;
        s.profile = "work".into();
        s.status = "Connected".into();
        let (owner, binding) = s.project_owner().unwrap();
        s.fleet_state.projects.bind(owner.clone());
        let (ticket, _) = s
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        assert!(s.fleet_state.projects.finish(ticket, &binding, Ok(json!({"projects":[{"id":"p", "label":"P", "previewSessions":[{"id":"same", "title":"A"}]}]}))));
        let generation = s.fleet_state.projects.generation();
        s.selected_session = Some("keep".into());
        s.fleet_state.projects.select(Scope::Home);
        s.fleet_state.projects.select(Scope::All);
        s.project_open_preview(
            owner.clone(),
            Scope::All,
            generation,
            "p".into(),
            "same".into(),
        );
        assert_eq!(s.selected_session.as_deref(), Some("keep"));
        s.connection = Some(Connection::Local {
            home: "/tmp/synthetic-preview-b".into(),
        });
        s.connection_epoch += 1;
        let (other, other_binding) = s.project_owner().unwrap();
        s.fleet_state.projects.bind(other);
        let (ticket, _) = s
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &other_binding)
            .unwrap();
        assert!(s.fleet_state.projects.finish(ticket, &other_binding, Ok(json!({"projects":[{"id":"p", "label":"P", "previewSessions":[{"id":"same", "title":"B"}]}]}))));
        s.project_open_preview(
            owner,
            Scope::All,
            s.fleet_state.projects.generation(),
            "p".into(),
            "same".into(),
        );
        assert_eq!(s.selected_session.as_deref(), Some("keep"));
    }
}

// The fixture server answers the actual project request and records each request's
// authenticated profile and preview limit; Runtime mounts the returned rows.

#[cfg(test)]
#[path = "projects/folders_tests.rs"]
mod folders_tests;
