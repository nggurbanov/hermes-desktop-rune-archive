//! Durable, owner-scoped conversations. A lost ACK is a retained intent, never
//! permission to create another session or submit another user turn.
use crate::{
    composer::{DraftScope, ModelKey},
    gateway::{Connection, HistoryMessage},
};
use rune::Waker;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

mod clarify;
mod history;
mod journal;
mod projection;
mod worker;
mod workspace;
pub(crate) use journal::{Journal, Prepared};

#[derive(Clone, Debug, Default)]
pub struct SendInput {
    pub canonical_bot: bool,
    pub voice_turn: bool,
    pub voice_context: Option<String>,
    pub surface: Option<String>,
    pub interrupted: bool,
    pub mode: AdmissionMode,
    pub text: String,
    pub display_text: Option<String>,
    pub rewind: Option<Rewrite>,
    pub attachments: Vec<AttachmentInput>,
    pub model: Option<ModelKey>,
    pub reasoning: Option<String>,
    pub service_tier: Option<String>,
    pub approval_mode: Option<String>,
    pub model_changed: bool,
    pub reasoning_changed: bool,
    pub service_tier_changed: bool,
    pub approval_changed: bool,
    pub cwd: Option<String>,
    pub cwd_explicit: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rewrite {
    pub target: u64,
    pub revision: u64,
    pub generation: u64,
    pub restore_only: bool,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AdmissionMode {
    #[default]
    Queue,
    Steer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachmentInput {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PendingPrompt {
    pub kind: String,
    pub prompt_id: String,
    pub execution_generation: u64,
    #[serde(default, deserialize_with = "clarify::choices")]
    pub choices: Vec<String>,
    pub command: Option<String>,
    pub description: Option<String>,
    pub question: Option<String>,
    #[serde(default)]
    pub multi_select: bool,
    #[serde(default)]
    pub questions: Vec<ClarifyQuestion>,
    #[serde(default, deserialize_with = "clarify::answers")]
    pub answers: std::collections::BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub server_request: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ClarifyQuestion {
    pub qid: String,
    pub question: String,
    #[serde(default, deserialize_with = "clarify::choices")]
    pub choices: Vec<String>,
    #[serde(default)]
    pub multi_select: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub struct PendingAdmission {
    pub admission_id: String,
    #[serde(default)]
    pub input_id: String,
    #[serde(default)]
    pub text: String,
    pub status: String,
    pub execution_generation: Option<u64>,
    #[serde(default)]
    pub controls: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ModelConfirmation {
    pub submission_id: String,
    pub message: String,
}

#[derive(Default)]
pub struct ConversationView {
    pub messages: Vec<HistoryMessage>,
    pub running: bool,
    pub completion: u64,
    pub completed_text: Option<String>,
    pub completed_input: Option<String>,
    pub completed_status: Option<String>,
    pub completed_admission: Option<String>,
    pub active_input: Option<String>,
    pub active_admission: Option<String>,
    pub active_generation: Option<u64>,
    pub active_text: String,
    pub(crate) admission_inputs: HashMap<String, String>,
    pub preparing: bool,
    pub workspace_pending: bool,
    pub workspace_uncertain: bool,
    pub error: Option<String>,
    pub uncertain: Option<String>,
    pub model_confirmation: Option<ModelConfirmation>,
    pub canonical_bot: bool,
    pub title: String,
    pub cwd: Option<String>,
    pub metadata: Value,
    pub prompts: Vec<PendingPrompt>,
    pub admissions: Vec<PendingAdmission>,
    pub reasoning: String,
    pub tools: Vec<String>,
    pub activities: Vec<Value>,
    pub truncated: bool,
    pub history_cursor: usize,
    pub history_loading: bool,
    pub generation: u64,
    pub authority: u64,
    pub revision: u64,
    pub(crate) canonical: bool,
    pub(crate) runtime: String,
    pub(crate) stored: String,
    pub(crate) replay_epoch: String,
    pub(crate) last_sequence: u64,
    pub(crate) delta: Option<projection::Delta>,
    pub(crate) submitted: std::collections::HashSet<String>,
}

#[derive(Clone, Debug)]
pub enum Update {
    AgentEvent {
        scope: DraftScope,
        kind: String,
        payload: Value,
    },
    Bound {
        origin: DraftScope,
        stored: String,
    },
    Changed {
        scope: DraftScope,
    },
    Admitted {
        scope: DraftScope,
        submission_id: String,
        text: String,
    },
    Failed {
        scope: DraftScope,
        message: String,
        uncertain: bool,
    },
}

pub struct Conversation {
    workspaces: HashMap<DraftScope, workspace::Intent>,
    workers: HashMap<DraftScope, Sender<worker::Command>>,
    aliases: HashMap<DraftScope, DraftScope>,
    retired_profiles: std::collections::HashSet<(u64, String)>,
    views: HashMap<DraftScope, ConversationView>,
    replies: Sender<worker::Reply>,
    receive: Receiver<worker::Reply>,
    waker: Option<Waker>,
}
impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}
impl Conversation {
    pub fn new() -> Self {
        let (replies, receive) = mpsc::channel();
        Self {
            workspaces: HashMap::new(),
            workers: HashMap::new(),
            aliases: HashMap::new(),
            retired_profiles: std::collections::HashSet::new(),
            views: HashMap::new(),
            replies,
            receive,
            waker: None,
        }
    }
    pub fn init(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }
    fn key(&self, scope: &DraftScope) -> DraftScope {
        self.aliases
            .get(scope)
            .cloned()
            .unwrap_or_else(|| scope.clone())
    }
    pub fn view(&self, scope: &DraftScope) -> Option<&ConversationView> {
        self.views.get(&self.key(scope))
    }
    pub fn attach(&mut self, connection: Connection, scope: DraftScope) -> Result<(), String> {
        if scope.session.is_none() {
            return Err("Choose a stored conversation before attaching".into());
        }
        let key = self.key(&scope);
        if self.workers.contains_key(&key) {
            return Ok(());
        }
        let worker = self.worker(connection, key)?;
        worker
            .send(worker::Command::Attach)
            .map_err(|_| "Conversation worker is unavailable".into())
    }
    pub fn close_profile(&mut self, connection: u64, profile: &str) {
        self.retired_profiles.insert((connection, profile.into()));
        self.workspaces
            .retain(|scope, _| !(scope.connection == connection && scope.profile == profile));
        self.workers
            .retain(|scope, _| !(scope.connection == connection && scope.profile == profile));
        self.views
            .retain(|scope, _| !(scope.connection == connection && scope.profile == profile));
        self.aliases
            .retain(|scope, _| !(scope.connection == connection && scope.profile == profile));
    }
    pub fn blocks_profile_retirement(&self, connection: u64, profile: Option<&str>) -> bool {
        self.views.iter().any(|(scope, view)| {
            scope.connection == connection
                && profile.is_none_or(|profile| scope.profile == profile)
                && (view.preparing
                    || view.workspace_pending
                    || view.workspace_uncertain
                    || view.uncertain.is_some())
        })
    }
    pub fn load_earlier(
        &mut self,
        connection: Connection,
        scope: &DraftScope,
    ) -> Result<(), String> {
        let key = self.key(scope);
        let view = self
            .views
            .get(&key)
            .ok_or("Attach this conversation before loading earlier messages")?;
        if view.history_loading || !view.truncated {
            return Ok(());
        }
        let stored = view.stored.clone();
        let cursor = view.history_cursor;
        let worker = self.worker(connection, key.clone())?;
        worker
            .send(worker::Command::Older { stored, cursor })
            .map_err(|_| "The conversation reader stopped")?;
        self.views.get_mut(&key).unwrap().history_loading = true;
        Ok(())
    }
    pub fn profile_readback(&mut self, connection: u64, profile: &str) {
        self.retired_profiles.remove(&(connection, profile.into()));
    }
    pub fn process_request(&self, scope: &DraftScope) -> Option<crate::pages::PageRequest> {
        let view = self.view(scope)?;
        if view.runtime.is_empty() {
            return None;
        }
        Some(
            crate::pages::PageRequest::rpc(
                "process.list",
                serde_json::json!({"session_id":view.runtime}),
            )
            .profile(&scope.profile)
            .timeout_secs(10),
        )
    }
    pub fn reconnect(&mut self, connection: Connection, scope: &DraftScope) -> Result<(), String> {
        self.retired_profiles
            .remove(&(scope.connection, scope.profile.clone()));
        let key = self.key(scope);
        if key.session.is_none() {
            return Err("Choose a stored conversation before reconnecting".into());
        }
        if let Some(worker) = self.workers.get(&key) {
            if worker.send(worker::Command::Attach).is_ok() {
                return Ok(());
            }
        }
        self.workers.remove(&key);
        self.worker(connection, key)?
            .send(worker::Command::Attach)
            .map_err(|_| "Conversation worker is unavailable".into())
    }
    pub fn close_connection(&mut self, connection: u64) {
        for scope in self
            .workers
            .keys()
            .chain(self.views.keys())
            .filter(|scope| scope.connection == connection)
        {
            self.retired_profiles
                .insert((connection, scope.profile.clone()));
        }
        self.workspaces
            .retain(|scope, _| scope.connection != connection);
        self.workers
            .retain(|scope, _| scope.connection != connection);
        self.views.retain(|scope, _| scope.connection != connection);
        self.aliases
            .retain(|scope, _| scope.connection != connection);
    }
    /// A synthetic projection exercises the native controls without a socket,
    /// journal entry, submitted prompt or agent mutation.
    pub fn fixture(&mut self, scope: DraftScope, value: &Value) -> Result<(), String> {
        let messages = value
            .get("messages")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().map(HistoryMessage::from_row).collect())
            .unwrap_or_default();
        let prompts = serde_json::from_value(
            value
                .get("prompts")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .map_err(|_| "Invalid synthetic prompts")?;
        let admissions = serde_json::from_value(
            value
                .get("pending")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .map_err(|_| "Invalid synthetic admissions")?;
        self.views.insert(
            scope.clone(),
            ConversationView {
                messages,
                prompts,
                admissions,
                running: value.get("running") == Some(&Value::Bool(true)),
                preparing: value.get("preparing") == Some(&Value::Bool(true)),
                generation: value
                    .get("execution_generation")
                    .and_then(Value::as_u64)
                    .unwrap_or(1),
                stored: scope.session.clone().unwrap_or_default(),
                runtime: scope.session.clone().unwrap_or_default(),
                canonical: true,
                reasoning: value["reasoning"].as_str().unwrap_or("").into(),
                tools: value["tools"]
                    .as_array()
                    .map(|rows| {
                        rows.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
                activities: value["activities"].as_array().cloned().unwrap_or_default(),
                ..Default::default()
            },
        );
        Ok(())
    }
    pub fn send(
        &mut self,
        connection: Connection,
        scope: DraftScope,
        input: SendInput,
    ) -> Result<String, String> {
        validate(&input)?;
        if self
            .retired_profiles
            .contains(&(scope.connection, scope.profile.clone()))
        {
            return Err(
                "This profile is being changed; wait for its registry readback before sending"
                    .into(),
            );
        }
        let key = self.key(&scope);
        if self
            .views
            .get(&key)
            .is_some_and(|s| s.preparing || s.uncertain.is_some() || s.workspace_uncertain)
        {
            return Err("Resolve the prepared send before sending another message".into());
        }
        let journal = Journal::new(crate::client::state_dir()?.join("submissions"))?;
        let prepared = Prepared::new(&connection, &scope, input)?;
        let id = prepared.submission_id.clone();
        journal.save(&prepared)?;
        let worker = self.worker(connection, key.clone())?;
        self.views.entry(key.clone()).or_default().preparing = true;
        if worker.send(worker::Command::Send(prepared)).is_err() {
            let view = self.views.entry(key).or_default();
            view.preparing = false;
            view.uncertain = Some(id.clone());
            return Err(
                "Conversation worker is unavailable; the send is saved for explicit retry".into(),
            );
        }
        Ok(id)
    }
    pub fn retry(
        &mut self,
        connection: Connection,
        scope: DraftScope,
        submission_id: &str,
    ) -> Result<(), String> {
        let key = self.key(&scope);
        if self.views.get(&key).is_some_and(|v| v.preparing) {
            return Err("A send is already being prepared".into());
        }
        let journal = Journal::new(crate::client::state_dir()?.join("submissions"))?;
        let entry = journal.load(submission_id)?;
        entry.verify_owner(&connection, &scope)?;
        if entry.mutations.iter().any(|m| m.confirmation.is_some()) {
            return Err("Confirm or cancel the guarded model selection first".into());
        }
        if entry.legacy_attempted {
            return Err("The legacy gateway cannot prove whether this turn was sent. Read the transcript before discarding the saved send; retrying could duplicate it".into());
        }
        let worker = self.worker(connection, key.clone())?;
        self.views.entry(key).or_default().preparing = true;
        worker
            .send(worker::Command::Send(entry))
            .map_err(|_| "Conversation worker is unavailable".into())
    }
    pub fn confirm_model(
        &mut self,
        connection: Connection,
        scope: DraftScope,
        id: &str,
    ) -> Result<(), String> {
        let journal = Journal::new(crate::client::state_dir()?.join("submissions"))?;
        let mut entry = journal.load(id)?;
        entry.verify_owner(&connection, &scope)?;
        let mutation = entry
            .mutations
            .iter_mut()
            .find(|m| m.confirmation.is_some())
            .ok_or("This model confirmation is no longer available")?;
        let token = mutation.confirmation.take().unwrap();
        mutation.params["request_id"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
        mutation.params["payload"]["confirm"] = serde_json::json!(token);
        journal.save(&entry)?;
        let key = self.key(&scope);
        let worker = self.worker(connection, key.clone())?;
        let view = self.views.entry(key).or_default();
        view.model_confirmation = None;
        view.preparing = true;
        worker
            .send(worker::Command::Send(entry))
            .map_err(|_| "Conversation worker is unavailable".into())
    }
    pub fn steer_admission(
        &mut self,
        connection: Connection,
        scope: &DraftScope,
        id: &str,
    ) -> Result<(), String> {
        let key = self.key(scope);
        let view = self
            .views
            .get(&key)
            .ok_or("Attach this conversation first")?;
        let admission = view
            .admissions
            .iter()
            .find(|a| a.admission_id == id && a.status == "queued")
            .ok_or("The queued turn is no longer available to steer")?;
        if !admission.controls.is_empty() && !admission.controls.iter().any(|c| c == "steer") {
            return Err("The agent does not offer steering for this queued turn".into());
        }
        self.control(
            connection,
            key,
            worker::Control::Admission {
                stored: view.stored.clone(),
                runtime: view.runtime.clone(),
                id: id.into(),
                resolve: false,
                steer: true,
                generation: view.generation,
            },
        )
    }
    /// Recovery is display-only. Nothing in the journal is an automatic outbox.
    pub fn recoverable(
        &self,
        connection: &Connection,
        scope: &DraftScope,
    ) -> Result<Vec<(String, String)>, String> {
        let journal = Journal::new(crate::client::state_dir()?.join("submissions"))?;
        Ok(journal
            .entries()?
            .into_iter()
            .filter(|e| e.verify_owner(connection, scope).is_ok())
            .map(|e| {
                (
                    e.submission_id,
                    e.input.display_text.unwrap_or(e.input.text),
                )
            })
            .collect())
    }
    pub fn discard(
        &mut self,
        connection: &Connection,
        scope: &DraftScope,
        submission_id: &str,
    ) -> Result<(), String> {
        let journal = Journal::new(crate::client::state_dir()?.join("submissions"))?;
        let entry = journal.load(submission_id)?;
        entry.verify_owner(connection, scope)?;
        journal.retire(submission_id)?;
        if let Some(view) = self.views.get_mut(&self.key(scope)) {
            if view.uncertain.as_deref() == Some(submission_id) {
                view.uncertain = None;
                view.error = None;
            }
        }
        Ok(())
    }
    pub fn stop(&mut self, connection: Connection, scope: &DraftScope) -> Result<(), String> {
        let key = self.key(scope);
        let view = self
            .views
            .get(&key)
            .ok_or("Attach this conversation before stopping it")?;
        if !view.running {
            return Err("This conversation has no running turn".into());
        }
        self.control(
            connection,
            key,
            worker::Control::Stop {
                runtime: view.runtime.clone(),
                stored: view.stored.clone(),
                generation: view.generation,
                authority: view.authority,
            },
        )
    }
    pub fn respond(
        &mut self,
        connection: Connection,
        scope: &DraftScope,
        prompt_id: &str,
        answer: &str,
    ) -> Result<(), String> {
        let key = self.key(scope);
        let view = self
            .views
            .get(&key)
            .ok_or("Attach this conversation before responding")?;
        let prompt = view
            .prompts
            .iter()
            .find(|p| p.prompt_id == prompt_id && p.execution_generation == view.generation)
            .ok_or("This prompt is stale; refresh the conversation")?;
        if prompt.kind == "clarify" && !prompt.questions.is_empty() {
            return Err("Answer the individual questions on this clarification card".into());
        }
        if prompt.kind == "clarify" && prompt.multi_select && !answer.is_empty() {
            let values: Vec<String> = serde_json::from_str(answer)
                .map_err(|_| "Choose the multiple-choice answers as a list")?;
            if values.is_empty() {
                return Err("Choose an answer or skip the question".into());
            }
        }
        if prompt.kind == "approval" && !prompt.choices.iter().any(|choice| choice == answer) {
            return Err("Choose one of this approval's offered responses".into());
        }
        let control = worker::Control::Respond {
            runtime: view.runtime.clone(),
            stored: view.stored.clone(),
            generation: prompt.execution_generation,
            authority: view.authority,
            prompt_id: prompt.prompt_id.clone(),
            kind: prompt.kind.clone(),
            answer: answer.into(),
        };
        if !view.canonical {
            return self
                .workers
                .get(&key)
                .ok_or("The prompt connection is unavailable")?
                .send(worker::Command::Control(control))
                .map_err(|_| "The prompt connection is unavailable".into());
        }
        self.control(connection, key, control)
    }
    pub fn cancel_admission(
        &mut self,
        connection: Connection,
        scope: &DraftScope,
        id: &str,
    ) -> Result<(), String> {
        let key = self.key(scope);
        let view = self
            .views
            .get(&key)
            .ok_or("Attach this conversation first")?;
        let admission = view
            .admissions
            .iter()
            .find(|a| a.admission_id == id)
            .ok_or("The queued turn is no longer available")?;
        self.control(
            connection,
            key,
            worker::Control::Admission {
                stored: view.stored.clone(),
                runtime: view.runtime.clone(),
                id: id.into(),
                resolve: admission.status == "unknown",
                steer: false,
                generation: admission.execution_generation.unwrap_or(view.generation),
            },
        )
    }
    fn control(
        &self,
        connection: Connection,
        scope: DraftScope,
        control: worker::Control,
    ) -> Result<(), String> {
        if self
            .retired_profiles
            .contains(&(scope.connection, scope.profile.clone()))
        {
            return Err(
                "This profile is being changed; its controls are unavailable until readback".into(),
            );
        }
        let waker = self
            .waker
            .clone()
            .ok_or("Conversation transport is not initialized")?;
        worker::control(connection, scope, control, self.replies.clone(), waker);
        Ok(())
    }
    fn worker(
        &mut self,
        connection: Connection,
        key: DraftScope,
    ) -> Result<Sender<worker::Command>, String> {
        if self
            .retired_profiles
            .contains(&(key.connection, key.profile.clone()))
        {
            return Err(
                "This profile is being changed; refresh its registry before reconnecting".into(),
            );
        }
        if let Some(worker) = self.workers.get(&key) {
            return Ok(worker.clone());
        }
        let waker = self
            .waker
            .clone()
            .ok_or("Conversation transport is not initialized")?;
        let send = worker::spawn(connection, key.clone(), self.replies.clone(), waker);
        self.workers.insert(key, send.clone());
        Ok(send)
    }
    pub fn poll(&mut self) -> Vec<Update> {
        let mut updates = vec![];
        for reply in self.receive.try_iter().take(4096).collect::<Vec<_>>() {
            let reply_owner = match &reply {
                worker::Reply::Older { origin, .. }
                | worker::Reply::Workspace { origin, .. }
                | worker::Reply::Snapshot { origin, .. }
                | worker::Reply::Admitted { origin, .. }
                | worker::Reply::Event { origin, .. }
                | worker::Reply::Failed { origin, .. }
                | worker::Reply::Recovered { origin, .. }
                | worker::Reply::ModelConfirmation { origin, .. }
                | worker::Reply::ClarifyLocked { origin, .. }
                | worker::Reply::ControlSettled { origin } => {
                    (origin.connection, origin.profile.clone())
                }
            };
            if self.retired_profiles.contains(&reply_owner) {
                continue;
            }
            match reply {
                worker::Reply::Older {
                    origin,
                    cursor,
                    result,
                } => {
                    let key = self.key(&origin);
                    if let Some(view) = self.views.get_mut(&key) {
                        view.history_loading = false;
                        match result {
                            Ok(history) if view.history_cursor == cursor => {
                                view.messages = history::older(
                                    std::mem::take(&mut view.messages),
                                    history.messages,
                                );
                                view.history_cursor = history.cursor;
                                view.truncated = history.has_more;
                            }
                            Ok(_) => {}
                            Err(e) => view.error = Some(e.message),
                        }
                        updates.push(Update::Changed { scope: key });
                    }
                }
                worker::Reply::Workspace { origin, result } => {
                    updates.push(self.workspace_result(origin, result))
                }
                worker::Reply::Snapshot {
                    origin,
                    snapshot,
                    history,
                } => {
                    let scope = DraftScope {
                        session: Some(snapshot.stored.clone()),
                        ..origin.clone()
                    };
                    if origin != scope {
                        if let Some(worker) = self.workers.remove(&origin) {
                            self.workers.insert(scope.clone(), worker);
                        }
                        if let Some(view) = self.views.remove(&origin) {
                            self.views.insert(scope.clone(), view);
                        }
                    } else if origin != scope {
                        self.aliases.insert(origin.clone(), scope.clone());
                    }
                    let folder_confirmed =
                        self.views.get(&scope).is_some_and(|view| {
                            view.workspace_uncertain && !view.workspace_pending
                        }) && self.workspaces.get(&scope).is_some_and(|intent| {
                            snapshot.info.get("cwd").and_then(Value::as_str)
                                == Some(intent.path.as_str())
                        });
                    if folder_confirmed {
                        self.workspaces.remove(&scope);
                    }
                    let view = self.views.entry(scope.clone()).or_default();
                    view.install(snapshot, history);
                    if folder_confirmed {
                        view.workspace_uncertain = false;
                    }
                    updates.push(Update::Bound {
                        origin,
                        stored: scope.session.clone().unwrap(),
                    });
                    updates.push(Update::Changed { scope });
                }
                worker::Reply::Admitted {
                    origin,
                    id,
                    text,
                    status,
                    generation,
                    admission,
                    replayed,
                    user_row,
                } => {
                    let view = self.views.entry(origin.clone()).or_default();
                    view.preparing = false;
                    view.uncertain = None;
                    view.model_confirmation = None;
                    view.error = None;
                    if !matches!(status.as_str(), "voice_stopped" | "restored")
                        && !(replayed && status == "terminal")
                        && view.submitted.insert(id.clone())
                    {
                        view.messages.push(HistoryMessage {
                            role: "user".into(),
                            content: text.clone(),
                            raw: serde_json::json!({"input_id":id,"row_id":user_row}),
                        });
                    }
                    if status == "started" {
                        view.running = true;
                    }
                    if let Some(generation) = generation {
                        view.generation = generation;
                    }
                    if let Some(admission) = &admission {
                        view.admission_inputs.insert(admission.clone(), id.clone());
                    }
                    if let Some(admission_id) = admission
                        .filter(|_| matches!(status.as_str(), "queued" | "started" | "unknown"))
                    {
                        if !view
                            .admissions
                            .iter()
                            .any(|a| a.admission_id == admission_id)
                        {
                            view.admissions.push(PendingAdmission {
                                admission_id,
                                input_id: id.clone(),
                                text: text.clone(),
                                status: status.clone(),
                                execution_generation: generation,
                                controls: vec![],
                            });
                        }
                    }
                    let scope = DraftScope {
                        session: Some(view.stored.clone()),
                        ..origin
                    };
                    updates.push(Update::Admitted {
                        scope: scope.clone(),
                        submission_id: id,
                        text,
                    });
                    updates.push(Update::Changed { scope });
                }
                worker::Reply::Event { origin, event } => {
                    let view = self.views.entry(origin.clone()).or_default();
                    if (matches!(
                        event.kind.as_str(),
                        "agent.terminal.output" | "terminal.close" | "message.start"
                    ) || event.kind.starts_with("subagent."))
                        && event.session == view.runtime
                        && event
                            .epoch
                            .as_ref()
                            .is_none_or(|epoch| epoch == &view.replay_epoch)
                        && event.authority.is_none_or(|epoch| epoch >= view.authority)
                        && event.sequence.is_none_or(|seq| seq > view.last_sequence)
                    {
                        updates.push(Update::AgentEvent {
                            scope: DraftScope {
                                session: Some(view.stored.clone()),
                                ..origin.clone()
                            },
                            kind: event.kind.clone(),
                            payload: event.payload.clone(),
                        });
                    }
                    if view.apply_event(&event) {
                        updates.push(Update::Changed {
                            scope: DraftScope {
                                session: Some(view.stored.clone()),
                                ..origin
                            },
                        });
                    }
                }
                worker::Reply::Failed {
                    origin,
                    id,
                    message,
                    uncertain,
                } => {
                    let view = self.views.entry(origin.clone()).or_default();
                    if id.is_some() {
                        view.preparing = false;
                    }
                    view.error = Some(message.clone());
                    if id.is_some() {
                        view.uncertain = id;
                    }
                    let scope = DraftScope {
                        session: (!view.stored.is_empty())
                            .then(|| view.stored.clone())
                            .or(origin.session.clone()),
                        ..origin
                    };
                    updates.push(Update::Failed {
                        scope,
                        message,
                        uncertain,
                    });
                }
                worker::Reply::ClarifyLocked {
                    origin,
                    id,
                    qid,
                    answer,
                    remaining,
                } => {
                    let view = self.views.entry(origin.clone()).or_default();
                    if remaining.is_empty() {
                        view.prompts.retain(|p| p.prompt_id != id);
                    } else if let Some(prompt) = view.prompts.iter_mut().find(|p| p.prompt_id == id)
                    {
                        prompt.answers.insert(qid, answer);
                    }
                    updates.push(Update::Changed { scope: origin });
                }
                worker::Reply::Recovered { origin, snapshot } => {
                    let view = self.views.entry(origin.clone()).or_default();
                    view.revision = snapshot.revision;
                    view.cwd = snapshot
                        .info
                        .get("cwd")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    view.metadata = snapshot.metadata;
                    view.prompts = snapshot.prompts;
                    view.admissions = snapshot.pending;
                    view.error = None;
                    for admission in &view.admissions {
                        if !admission.input_id.is_empty() {
                            view.submitted.insert(admission.input_id.clone());
                            view.admission_inputs
                                .insert(admission.admission_id.clone(), admission.input_id.clone());
                        }
                    }
                    updates.push(Update::Changed { scope: origin });
                }
                worker::Reply::ModelConfirmation {
                    origin,
                    id,
                    message,
                } => {
                    let view = self.views.entry(origin.clone()).or_default();
                    view.preparing = false;
                    view.uncertain = Some(id.clone());
                    view.model_confirmation = Some(ModelConfirmation {
                        submission_id: id,
                        message,
                    });
                    updates.push(Update::Changed {
                        scope: DraftScope {
                            session: Some(view.stored.clone()),
                            ..origin
                        },
                    });
                }
                worker::Reply::ControlSettled { origin } => {
                    let scope = DraftScope {
                        session: self
                            .views
                            .get(&origin)
                            .map(|v| v.stored.clone())
                            .or(origin.session.clone()),
                        ..origin
                    };
                    updates.push(Update::Changed { scope });
                }
            }
        }
        updates
    }
}

fn validate(input: &SendInput) -> Result<(), String> {
    if input.text.trim().is_empty()
        && input.attachments.is_empty()
        && !input.rewind.as_ref().is_some_and(|r| r.restore_only)
    {
        return Err("Write a message or attach a file".into());
    }
    if input.text.len() > 2 * 1024 * 1024 {
        return Err("This message is too large; attach it as a file".into());
    }
    if input.reasoning.as_deref().is_some_and(|v| {
        !matches!(
            v,
            "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
        )
    }) {
        return Err("Unsupported reasoning level".into());
    }
    if input
        .service_tier
        .as_deref()
        .is_some_and(|v| !matches!(v, "normal" | "priority" | "ultrafast"))
    {
        return Err("Unsupported service tier".into());
    }
    if input
        .approval_mode
        .as_deref()
        .is_some_and(|v| !matches!(v, "manual" | "smart" | "off"))
    {
        return Err("Unsupported approval mode".into());
    }
    for file in &input.attachments {
        if !file.path.is_absolute() || !file.path.is_file() {
            return Err(format!(
                "Could not read {}. Select the file again",
                file.name
            ));
        }
    }
    Ok(())
}
