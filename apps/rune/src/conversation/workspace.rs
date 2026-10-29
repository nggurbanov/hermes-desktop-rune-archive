//! A folder change is a retained session mutation, serialized by that session's
//! worker. Lost acknowledgements keep the same CAS identity for explicit retry.
use super::*;
#[derive(Clone)]
pub(crate) struct Intent {
    pub path: String,
    pub request_id: String,
    pub stored: String,
    pub runtime: String,
    pub revision: u64,
    pub generation: u64,
    pub authority: u64,
}
impl Conversation {
    pub fn pending_cwd(&self, scope: &DraftScope) -> Option<String> {
        self.workspaces
            .get(&self.key(scope))
            .map(|intent| intent.path.clone())
    }
    pub fn set_cwd(
        &mut self,
        connection: Connection,
        scope: &DraftScope,
        path: String,
    ) -> Result<(), String> {
        let path = path.trim().to_owned();
        if path.is_empty() || path.len() > 16384 || path.contains(['\0', '\r', '\n']) {
            return Err("Choose a valid working directory on the agent host".into());
        }
        let key = self.key(scope);
        let view = self
            .views
            .get(&key)
            .ok_or("Attach the conversation before changing its working directory")?;
        if view.runtime.is_empty() || view.stored.is_empty() {
            return Err("The conversation has no attached runtime".into());
        }
        if view.running || view.preparing || view.uncertain.is_some() {
            return Err(
                "Finish the current turn or resolve its prepared send before changing folders"
                    .into(),
            );
        }
        let intent = if let Some(intent) = self.workspaces.get(&key) {
            if intent.path != path {
                return Err("Retry the pending folder change or reconnect to read its outcome before choosing another folder".into());
            }
            intent.clone()
        } else {
            Intent {
                path,
                request_id: uuid::Uuid::new_v4().to_string(),
                stored: view.stored.clone(),
                runtime: view.runtime.clone(),
                revision: view.revision,
                generation: view.generation,
                authority: view.authority,
            }
        };
        let worker = self.worker(connection, key.clone())?;
        self.workspaces.insert(key.clone(), intent.clone());
        let view = self.views.get_mut(&key).unwrap();
        view.preparing = true;
        view.workspace_pending = true;
        view.error = None;
        if worker.send(worker::Command::Workspace(intent)).is_err() {
            let view = self.views.get_mut(&key).unwrap();
            view.preparing = false;
            view.workspace_pending = false;
            view.workspace_uncertain = true;
            return Err("The conversation worker stopped; the folder change identity is retained for explicit retry".into());
        }
        Ok(())
    }
    pub(crate) fn workspace_result(
        &mut self,
        origin: DraftScope,
        result: Result<String, crate::gateway::conversation::CallError>,
    ) -> Update {
        let key = self.key(&origin);
        let view = self.views.entry(key.clone()).or_default();
        view.preparing = false;
        view.workspace_pending = false;
        match result {
            Ok(cwd) => {
                self.workspaces.remove(&key);
                view.workspace_uncertain = false;
                view.cwd = Some(cwd.clone());
                if !view.metadata.is_object() {
                    view.metadata = serde_json::json!({});
                }
                if !view.metadata["info"].is_object() {
                    view.metadata["info"] = serde_json::json!({});
                }
                view.metadata["info"]["cwd"] = serde_json::json!(cwd);
                view.error = None;
            }
            Err(error) => {
                view.workspace_uncertain = error.uncertain;
                if !error.uncertain {
                    self.workspaces.remove(&key);
                }
                view.error = Some(error.message);
            }
        }
        Update::Changed { scope: key }
    }
}
