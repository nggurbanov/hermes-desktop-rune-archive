use super::*;
impl Viewer {
    pub fn open_subagent(&mut self, id: String) {
        if self.fixture {
            return;
        }
        if let Some(connection) = self.connection.as_ref() {
            if let Err(error) =
                self.subagents
                    .open(connection, &self.composer_scope(), &id, self.theme)
            {
                self.client_error = Some(error);
            }
        }
    }
    pub fn open_chat_file(&mut self, owner: DraftScope, path: String) {
        if owner != self.composer_scope() {
            self.client_error = Some("The file belongs to a different conversation".into());
            return;
        }
        if self.files.blocks_owner_change() {
            self.client_error =
                Some("Save or discard the file editor before opening another file".into());
            return;
        }
        let effects = self
            .files
            .bind(owner, self.is_local_connection(), self.workspace_cwd());
        self.page_requests(effects);
        self.reveal_pane("files");
        self.files_action(crate::files::Action::Open(path));
    }
    pub fn reveal_chat_process(&mut self, owner: DraftScope, id: String) {
        if owner != self.composer_scope() {
            self.client_error = Some("The process belongs to a different conversation".into());
            return;
        }
        self.reveal_pane("terminal");
        self.terminal.open_process(&owner, &id);
    }
    pub fn workspace_cwd(&self) -> Option<String> {
        let scope = self.composer_scope();
        if scope.session.is_none() {
            if let Some(cwd) = self.composer.draft(&scope).and_then(|d| d.cwd.clone()) {
                return Some(cwd);
            }
        }
        if let Some(cwd) = self.conversation.view(&scope).and_then(|v| v.cwd.clone()) {
            return Some(cwd);
        }
        self.workspace_cwds.get(&scope).cloned().or_else(|| {
            scope
                .session
                .as_ref()
                .and_then(|id| self.sidebar_state.find(&scope.profile, id))
                .and_then(|s| s.cwd.clone())
        })
    }
    pub(super) fn poll_tools(&mut self, now: f64, tx: &mut Tx) -> bool {
        let artifacts_changed = self.artifacts.poll();
        if let Some(url) = self.artifacts.take_open_url() {
            if url::Url::parse(&url).is_ok_and(|u| {
                matches!(u.scheme(), "http" | "https")
                    || u.scheme() == "file" && u.to_file_path().is_ok_and(|p| p.is_absolute())
            }) {
                if tx.open_url(&url).is_err() {
                    self.artifacts.error =
                        Some("The artifact could not be opened in its native application".into());
                }
            }
        }
        let effects = self.review.poll(now);
        if self
            .directories
            .owner
            .as_ref()
            .is_some_and(|o| *o != self.composer_scope())
        {
            self.directories.owner = None;
            self.invalidate_page_requests("directories.");
        }
        let mut changed = artifacts_changed
            | !effects.is_empty()
            | self.review.take_redraw()
            | self.terminal.poll();
        self.page_requests(effects);
        self.consume_review(tx);
        if self.fixture {
            return changed;
        }
        if let Some(connection) = self.connection.clone() {
            let scope = self.composer_scope();
            let cwd = self.workspace_cwd();
            if self.tool_owner.as_ref() != Some(&(scope.clone(), cwd.clone())) {
                let ssh = if let Connection::Saved { id, revision, .. } = &connection {
                    self.connections
                        .rows
                        .iter()
                        .find(|r| r.id == *id && r.revision == *revision)
                        .and_then(|r| match &r.destination {
                            crate::connections::Destination::Ssh(ssh) => Some(ssh.clone()),
                            _ => None,
                        })
                } else {
                    None
                };
                if let Err(error) =
                    self.terminal
                        .set_owner(connection, scope.clone(), cwd.clone(), ssh)
                {
                    self.client_error = Some(error);
                }
                self.invalidate_page_requests("terminal.processes");
                self.process_owner = None;
                let effects =
                    self.review
                        .bind(scope.clone(), self.is_local_connection(), cwd.clone());
                self.page_requests(effects);
                self.tool_owner = Some((scope, cwd));
                changed = true;
            }
        }
        let scope = self.composer_scope();
        let cwd = self.workspace_cwd();
        if !self.review.blocks_owner_change()
            && (self.review.owner.as_ref() != Some(&scope)
                || self.review.root != cwd.clone().unwrap_or_default())
        {
            let effects = self.review.bind(scope, self.is_local_connection(), cwd);
            changed |= !effects.is_empty();
            self.page_requests(effects);
        }
        if self.pane_shown("terminal") && self.process_owner.is_none() && now >= self.next_process {
            let scope = self.composer_scope();
            if let Some(request) = self.conversation.process_request(&scope) {
                self.process_owner = Some(scope);
                self.next_process = now + 2.;
                self.page_request("terminal.processes", request);
                changed = true;
            }
        }
        changed
    }
}

impl Viewer {
    fn consume_review(&mut self, tx: &mut Tx) {
        if let Some(prefs) = self.review.take_preferences() {
            self.review_preferences = prefs;
        }
        if self.review.take_close() {
            self.layout.close("review");
        }
        if let Some(url) = self.review.take_url() {
            if url::Url::parse(&url).is_ok_and(|u| matches!(u.scheme(), "http" | "https")) {
                if tx.open_url(&url).is_err() {
                    self.review.error = Some("The pull request link could not be opened".into());
                }
            }
        }
        if let Some((owner, path)) = self.review.take_workspace() {
            self.change_workspace(owner, path);
        }
        if let Some(request) = self.review.take_file_request() {
            if request.owner != self.composer_scope() {
                self.review.error = Some("The reviewed conversation changed".into());
            } else if self.files.blocks_owner_change() {
                self.review.error =
                    Some("Save or discard the file editor before opening another file".into());
            } else {
                let effects = self.files.bind(
                    request.owner.clone(),
                    self.is_local_connection(),
                    Some(request.root.clone()),
                );
                self.page_requests(effects);
                if self.files.root != request.root {
                    self.files_action(crate::files::Action::Root(request.root.clone()));
                }
                self.reveal_pane("files");
                if request.in_tree {
                    let mut parents = vec![];
                    let mut dir = std::path::Path::new(&request.path).parent();
                    while let Some(path) = dir {
                        if !path.starts_with(&request.root) {
                            break;
                        }
                        parents.push(path.to_string_lossy().into_owned());
                        dir = path.parent();
                    }
                    parents.reverse();
                    for path in parents {
                        if !self.files.expanded.contains(&path) {
                            self.files_action(crate::files::Action::Toggle(path));
                        }
                    }
                } else {
                    self.files_action(crate::files::Action::Open(request.path));
                }
            }
        }
        if let Some((owner, cwd, prompt)) = self.review.take_agent_ship() {
            if self.fixture || owner != self.composer_scope() || self.read_only_reason().is_some() {
                self.review.error = Some("The reviewed conversation is no longer available".into());
                return;
            }
            if let Some(connection) = self.connection.clone() {
                let draft = self.composer.draft(&owner).cloned().unwrap_or_default();
                let input = crate::conversation::SendInput {
                    text: prompt,
                    cwd: Some(cwd),
                    cwd_explicit: true,
                    model: draft.model.clone(),
                    reasoning: draft.reasoning.clone(),
                    service_tier: draft.service_tier.clone(),
                    approval_mode: draft.approval_mode.clone(),
                    model_changed: draft.model_changed,
                    reasoning_changed: draft.reasoning_changed,
                    service_tier_changed: draft.service_tier_changed,
                    approval_changed: draft.approval_changed,
                    ..Default::default()
                };
                match self.conversation.send(connection, owner, input) {
                    Ok(id) => {
                        self.policy_sends.insert(id, draft);
                    }
                    Err(error) => self.review.error = Some(error),
                }
            }
        }
    }
    pub fn change_workspace(&mut self, owner: DraftScope, path: String) {
        if self.fixture || owner != self.composer_scope() {
            return;
        }
        if self.review.blocks_owner_change() {
            self.client_error =
                Some("Finish the Review change before changing the working directory".into());
            return;
        }
        if owner.session.is_none() {
            let draft = self.composer.draft_mut(owner.clone());
            draft.cwd = Some(path.clone());
            draft.cwd_explicit = true;
            self.workspace_cwds.insert(owner, path);
            self.workspace_menu = false;
            return;
        }
        if let Some(connection) = self.connection.clone() {
            if let Err(error) = self.conversation.set_cwd(connection, &owner, path) {
                self.client_error = Some(error);
            }
        }
    }
}

impl Viewer {
    pub fn capture_review_turn(&mut self, scope: DraftScope) {
        let cwd = if scope == self.composer_scope() {
            self.workspace_cwd()
        } else {
            self.workspace_cwds.get(&scope).cloned().or_else(|| {
                scope
                    .session
                    .as_ref()
                    .and_then(|id| self.sidebar_state.find(&scope.profile, id))
                    .and_then(|r| r.cwd.clone())
            })
        };
        if let Some(cwd) = cwd {
            let effects = self
                .review
                .capture_turn(scope, self.is_local_connection(), cwd);
            self.page_requests(effects);
        }
    }
}
