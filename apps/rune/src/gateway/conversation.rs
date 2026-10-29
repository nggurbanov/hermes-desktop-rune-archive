//! The mutating conversation transport is deliberately separate from the
//! read-only inventory transport. Every call and notification retains its owner.
use super::*;
use crate::conversation::{PendingAdmission, PendingPrompt};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct Event {
    pub session: String,
    pub kind: String,
    pub payload: Value,
    pub epoch: Option<String>,
    pub sequence: Option<u64>,
    pub generation: Option<u64>,
    pub authority: Option<u64>,
    pub admission: Option<String>,
}
impl Event {
    pub fn from_value(value: &Value) -> Option<Self> {
        let params = if value.get("method").and_then(Value::as_str) == Some("event") {
            value.get("params")?
        } else {
            value
        };
        Some(Self {
            session: params.get("session_id")?.as_str()?.into(),
            kind: params.get("type")?.as_str()?.into(),
            payload: params.get("payload").cloned().unwrap_or_else(|| json!({})),
            epoch: params
                .get("replay_epoch")
                .or_else(|| params.get("replayEpoch"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            sequence: params.get("seq").and_then(Value::as_u64),
            generation: params.get("execution_generation").and_then(Value::as_u64),
            authority: params.get("authority_epoch").and_then(Value::as_u64),
            admission: params
                .get("admission_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }
}
#[derive(Clone)]
pub struct Snapshot {
    pub metadata: Value,
    pub runtime: String,
    pub stored: String,
    pub canonical: bool,
    pub revision: u64,
    pub generation: u64,
    pub authority: u64,
    pub running: bool,
    pub epoch: String,
    pub sequence: u64,
    pub prompts: Vec<PendingPrompt>,
    pub pending: Vec<PendingAdmission>,
    pub info: Value,
    pub messages: Vec<HistoryMessage>,
}
impl Snapshot {
    fn parse(value: &Value, profile: &str, canonical: bool) -> Result<Self, String> {
        let runtime = required_str(value, "session_id")?.to_owned();
        valid_scope(&runtime)?;
        let stored = value
            .get("stored_session_id")
            .and_then(Value::as_str)
            .unwrap_or(&runtime)
            .to_owned();
        valid_scope(&stored)?;
        let info = value.get("info").cloned().unwrap_or_else(|| json!({}));
        if canonical && info.get("profile_name").and_then(Value::as_str) != Some(profile) {
            return Err("Session snapshot profile identity mismatch".into());
        }
        if canonical
            && info.get("desktop_protocol").and_then(Value::as_str) != Some("hermes-gateway-v1")
        {
            return Err("The native gateway returned an unsupported session protocol".into());
        }
        let revision = value.get("revision").and_then(Value::as_u64).unwrap_or(0);
        let generation = value
            .get("execution_generation")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if canonical
            && (!value.get("revision").is_some_and(Value::is_u64)
                || !value.get("execution_generation").is_some_and(Value::is_u64))
        {
            return Err("The session snapshot has no execution fence".into());
        }
        let mut prompts: Vec<PendingPrompt> =
            serde_json::from_value(value.get("prompts").cloned().unwrap_or_else(|| json!([])))
                .map_err(|_| "Invalid pending session prompts")?;
        if let Some(requests) = value.get("open_requests").and_then(Value::as_array) {
            for request in requests {
                if let Some(prompt) = PendingPrompt::from_server_request(request, generation) {
                    prompts.retain(|p| p.prompt_id != prompt.prompt_id);
                    prompts.push(prompt);
                }
            }
        }
        let pending =
            serde_json::from_value(value.get("pending").cloned().unwrap_or_else(|| json!([])))
                .map_err(|_| "Invalid pending session admissions")?;
        Ok(Self {
            metadata: value.clone(),
            runtime,
            stored,
            canonical,
            revision,
            generation,
            authority: value
                .get("authority_epoch")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            running: value
                .get("running")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            epoch: value
                .get("replay_epoch")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            sequence: value
                .get("last_sequence")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            prompts,
            pending,
            messages: value
                .get("messages")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter(|row| {
                            row.get("display_kind").and_then(Value::as_str) != Some("hidden")
                        })
                        .map(HistoryMessage::from_row)
                        .collect()
                })
                .unwrap_or_default(),
            info,
        })
    }
}

#[derive(Debug)]
pub struct CallError {
    pub message: String,
    pub code: Option<i64>,
    pub data: Option<Value>,
    pub uncertain: bool,
}
impl From<String> for CallError {
    fn from(message: String) -> Self {
        Self {
            message,
            code: None,
            data: None,
            uncertain: false,
        }
    }
}
impl From<&str> for CallError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}
pub struct SessionTransport {
    gateway: Gateway,
    pub canonical: bool,
    pub profile: String,
    queued: VecDeque<Event>,
    pub snapshot: Option<Snapshot>,
}
impl SessionTransport {
    pub fn connect(connection: &Connection, profile: &str) -> Result<Self, String> {
        valid_scope(profile)?;
        let gateway = Gateway::connect(connection)?;
        #[cfg(target_os = "linux")]
        let canonical = matches!(gateway.auth, Auth::Native(_));
        #[cfg(not(target_os = "linux"))]
        let canonical = false;
        Ok(Self {
            gateway,
            canonical,
            profile: profile.into(),
            queued: VecDeque::new(),
            snapshot: None,
        })
    }
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, CallError> {
        self.call_with_timeout(method, params, TIMEOUT)
    }
    pub fn call_with_timeout(
        &mut self,
        method: &str,
        mut params: Value,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        params
            .as_object_mut()
            .ok_or("Invalid conversation request")?
            .insert("profile".into(), json!(self.profile));
        self.gateway.sequence += 1;
        let id = self.gateway.sequence;
        let timeout = timeout.min(Duration::from_secs(660));
        let deadline = Instant::now() + timeout;
        socket_timeout(self.gateway.socket.get_mut(), timeout)?;
        self.gateway
            .socket
            .send(Message::Text(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| CallError {
                message: "The gateway connection failed while sending; the outcome is unknown"
                    .into(),
                code: None,
                data: None,
                uncertain: true,
            })?;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| CallError {
                    message: format!("{method} timed out; its outcome is unknown"),
                    code: None,
                    data: None,
                    uncertain: true,
                })?;
            socket_timeout(self.gateway.socket.get_mut(), remaining)?;
            let frame = self.read_frame().map_err(|mut e| {
                e.uncertain = true;
                e
            })?;
            let Some(frame) = frame else {
                continue;
            };
            if frame.get("id").and_then(Value::as_u64) == Some(id) && frame.get("method").is_none()
            {
                if let Some(error) = frame.get("error") {
                    let code = error.get("code").and_then(Value::as_i64);
                    let reason = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("request refused");
                    // The server's bounded reason is useful; never expose URL/token/transport frames.
                    return Err(CallError {
                        message: format!(
                            "{method}: {}{}",
                            reason.chars().take(360).collect::<String>(),
                            code.map(|c| format!(" (RPC {c})")).unwrap_or_default()
                        ),
                        code,
                        data: error.get("data").cloned(),
                        uncertain: false,
                    });
                }
                return frame.get("result").cloned().ok_or_else(|| CallError {
                    message: "The gateway acknowledgement is invalid; the outcome is unknown"
                        .into(),
                    code: None,
                    data: None,
                    uncertain: true,
                });
            }
            self.queue_frame(&frame);
        }
    }
    fn read_frame(&mut self) -> Result<Option<Value>, CallError> {
        match self.gateway.socket.read() {
            Ok(Message::Text(text)) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|_| CallError::from("Invalid conversation event")),
            Ok(Message::Close(_)) => Err(CallError::from("The conversation connection closed")),
            Ok(Message::Ping(_)) => {
                self.gateway
                    .socket
                    .flush()
                    .map_err(|_| CallError::from("The conversation heartbeat failed"))?;
                Ok(None)
            }
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err(CallError::from("The conversation connection failed")),
        }
    }
    fn queue_frame(&mut self, frame: &Value) {
        if let Some(event) = Event::from_value(frame) {
            if self
                .snapshot
                .as_ref()
                .is_none_or(|s| s.runtime == event.session)
            {
                self.queued.push_back(event);
            }
            return;
        }
        // Legacy server requests remain request-id owned; a response is sent on
        // this same socket rather than inventing a canonical prompt id.
        let Some(method) = frame.get("method").and_then(Value::as_str) else {
            return;
        };
        if !matches!(
            method,
            "approval.request" | "clarify.request" | "approval" | "clarify"
        ) {
            return;
        }
        let Some(id) = frame.get("id") else {
            return;
        };
        let mut payload = frame.get("params").cloned().unwrap_or_else(|| json!({}));
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "prompt_id".into(),
                Value::String(
                    id.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| id.to_string()),
                ),
            );
            object.insert("server_request_id".into(), id.clone());
            object.insert("server_request".into(), json!(true));
        }
        let session = payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| self.snapshot.as_ref().map(|s| s.runtime.clone()));
        if let Some(session) = session {
            self.queued.push_back(Event {
                session,
                kind: if method.starts_with("approval") {
                    "approval.request"
                } else {
                    "clarify.request"
                }
                .into(),
                payload,
                epoch: None,
                sequence: None,
                generation: None,
                authority: None,
                admission: None,
            });
        }
    }
    pub fn take_queued(&mut self) -> Vec<Event> {
        self.queued.drain(..).collect()
    }
    pub fn read_event(&mut self) -> Result<Option<Event>, CallError> {
        if let Some(event) = self.queued.pop_front() {
            return Ok(Some(event));
        }
        socket_timeout(self.gateway.socket.get_mut(), Duration::from_millis(120))?;
        if let Some(frame) = self.read_frame()? {
            self.queue_frame(&frame);
        }
        Ok(self.queued.pop_front())
    }
    pub fn resume(&mut self, stored: &str) -> Result<Snapshot, CallError> {
        valid_scope(stored)?;
        let value = self.call(
            "session.resume",
            json!({"session_id":stored,"source":"desktop","omit_messages":true}),
        )?;
        let snapshot = Snapshot::parse(&value, &self.profile, self.canonical)?;
        self.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
    /// Attach a spectator without admitting work or building an idle agent.
    pub fn watch(&mut self, stored: &str) -> Result<Snapshot, CallError> {
        valid_scope(stored)?;
        let value = self.call(
            "session.resume",
            json!({"session_id":stored,"source":"desktop","cols":96,"lazy":true,"defer_history":false}),
        )?;
        let snapshot = Snapshot::parse(&value, &self.profile, self.canonical)?;
        if snapshot
            .info
            .get("profile_name")
            .and_then(Value::as_str)
            .is_some_and(|owner| owner != self.profile)
        {
            return Err("The spectator returned a different profile".into());
        }
        if snapshot.stored != stored {
            return Err("The spectator returned a different stored session".into());
        }
        self.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
    pub fn create(&mut self, params: Value) -> Result<Snapshot, CallError> {
        let value = self.call("session.create", params)?;
        let snapshot = Snapshot::parse(&value, &self.profile, self.canonical)?;
        self.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
    pub fn history(&mut self, stored: &str) -> Result<History, CallError> {
        // Resolve native HTTP ownership without calling rpc_until, which is a
        // read-only helper that intentionally discards notifications.
        self.ensure_profiles()?;
        self.gateway
            .history(&self.profile, stored)
            .map_err(Into::into)
    }
    pub fn older_history(&mut self, stored: &str, offset: usize) -> Result<History, CallError> {
        self.ensure_profiles()?;
        self.gateway
            .history_window(&self.profile, stored, offset, Instant::now() + TIMEOUT)
            .map_err(Into::into)
    }
    fn ensure_profiles(&mut self) -> Result<(), CallError> {
        #[cfg(target_os = "linux")]
        if matches!(self.gateway.auth, Auth::Native(_))
            && !self.gateway.profile_homes.contains_key(&self.profile)
        {
            let profiles = self.call("profiles.list", json!({"include_sessions":false}))?;
            let profiles = rows(&profiles, "profiles")?;
            for row in profiles {
                if let (Some(name), Some(path)) = (
                    row.get("name").and_then(Value::as_str),
                    row.get("path").and_then(Value::as_str),
                ) {
                    self.gateway
                        .profile_homes
                        .insert(name.into(), PathBuf::from(path));
                }
            }
            if !self.gateway.profile_homes.contains_key(&self.profile) {
                return Err("The selected profile is not served by this gateway".into());
            }
        }
        Ok(())
    }
    pub fn replay(
        &mut self,
        runtime: &str,
        epoch: &str,
        last: u64,
    ) -> Result<Option<Vec<Event>>, CallError> {
        if !self.canonical {
            return Ok(None);
        }
        let value = self.call(
            "session.events.since",
            json!({"session_id":runtime,"last_sequence":last,"replay_epoch":epoch}),
        )?;
        let same = value
            .get("replay_epoch")
            .or_else(|| value.get("epoch"))
            .and_then(Value::as_str)
            == Some(epoch);
        if !same
            || value.get("snapshot_required") == Some(&Value::Bool(true))
            || value.get("truncated") == Some(&Value::Bool(true))
        {
            return Ok(None);
        }
        let rows = rows(&value, "events")?;
        Ok(Some(
            rows.iter()
                .filter_map(Event::from_value)
                .filter(|e| e.session == runtime)
                .collect(),
        ))
    }
    pub fn replay_media(&mut self, source: &str, stored: &str) -> Result<Value, CallError> {
        self.ensure_profiles()?;
        let source = if source.starts_with("file:") {
            url::Url::parse(source)
                .ok()
                .and_then(|u| u.to_file_path().ok())
                .ok_or("The stored file URL is invalid")?
                .to_string_lossy()
                .into_owned()
        } else {
            source.into()
        };
        let request = if source.starts_with("http://") || source.starts_with("https://") {
            crate::pages::PageRequest::get("/api/media/proxy").query("url", source)
        } else {
            crate::pages::PageRequest::get("/api/fs/read-data-url")
                .query("path", source)
                .query("session_id", stored)
        };
        self.gateway
            .page_request(&self.profile, &request)
            .map_err(CallError::from)
    }
    pub fn upload_image(&mut self, data_url: String, name: &str) -> Result<Value, CallError> {
        self.ensure_profiles()?;
        let request = crate::pages::PageRequest::post(
            "/api/chat/image-upload",
            json!({"data_url":data_url,"filename":name}),
        );
        let value = self.gateway.page_request(&self.profile, &request)?;
        let path = required_str(&value, "path")?;
        let mime = required_str(&value, "mime_type")?;
        if !mime.starts_with("image/") {
            return Err("The gateway did not recognize this image".into());
        }
        Ok(json!({"path":path,"mime":mime}))
    }
    pub fn profile_model(&mut self) -> Result<(String, String), CallError> {
        self.ensure_profiles()?;
        let request =
            crate::pages::PageRequest::get("/api/model/options").query("explicit_only", "1");
        let value = self.gateway.page_request(&self.profile, &request)?;
        let catalog = ModelCatalog::from_value(&value)?;
        Ok((
            catalog
                .current_provider
                .filter(|v| !v.is_empty())
                .ok_or("The profile has no configured provider")?,
            catalog
                .current_model
                .filter(|v| !v.is_empty())
                .ok_or("The profile has no configured model")?,
        ))
    }
    pub fn respond_legacy(&mut self, id: &str, kind: &str, answer: &str) -> Result<(), CallError> {
        let numeric = id.parse::<u64>().ok();
        let id = numeric.map(Value::from).unwrap_or_else(|| json!(id));
        socket_timeout(self.gateway.socket.get_mut(), TIMEOUT)?;
        self.gateway.socket.send(Message::Text(json!({"jsonrpc":"2.0","id":id,"result":if kind=="approval"{json!({"choice":answer})}else{json!({"answer":answer})}}).to_string().into())).map_err(|_|"The prompt response could not be sent")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_requires_real_native_owner_and_generation() {
        let value = json!({"session_id":"live","stored_session_id":"stored","revision":3,"execution_generation":4,"info":{"profile_name":"work","desktop_protocol":"hermes-gateway-v1"}});
        let snapshot = Snapshot::parse(&value, "work", true).unwrap();
        assert_eq!(snapshot.stored, "stored");
        assert_eq!(snapshot.generation, 4);
        assert!(Snapshot::parse(&value, "other", true).is_err());
        let mut no_fence = value.clone();
        no_fence
            .as_object_mut()
            .unwrap()
            .remove("execution_generation");
        assert!(Snapshot::parse(&no_fence, "work", true).is_err());
    }
}
