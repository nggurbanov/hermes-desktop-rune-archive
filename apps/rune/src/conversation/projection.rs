use super::*;
use crate::gateway::conversation::{Event, Snapshot};

pub(crate) struct Delta {
    turn: String,
    segment: u64,
    units: usize,
    row: usize,
}
impl ConversationView {
    pub(crate) fn install(&mut self, snapshot: Snapshot, history: crate::gateway::History) {
        let same_authority = self.authority == snapshot.authority && self.stored == snapshot.stored;
        self.canonical = snapshot.canonical;
        self.metadata = snapshot.metadata.clone();
        let deferred = (self.stored == snapshot.stored)
            .then(|| self.policy.deferred.take())
            .flatten();
        self.policy = SessionPolicy::from_info(&snapshot.info);
        self.policy.deferred = deferred;
        self.policy.info(&snapshot.info);
        self.policy_sequence = snapshot.sequence;
        self.title = snapshot
            .info
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into();
        self.canonical_bot = self.title == "Bot Chat";
        self.cwd = snapshot
            .info
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.runtime = snapshot.runtime;
        self.stored = snapshot.stored;
        self.replay_epoch = snapshot.epoch;
        self.last_sequence = snapshot.sequence;
        self.generation = snapshot.generation;
        self.authority = snapshot.authority;
        self.revision = snapshot.revision;
        self.running = snapshot.running;
        self.messages = super::history::tail(
            std::mem::take(&mut self.messages),
            history.messages,
            history.has_more,
            same_authority,
        );
        self.truncated = history.has_more;
        self.history_cursor = history.cursor;
        self.history_loading = false;
        self.prompts = snapshot.prompts;
        self.admissions = snapshot.pending;
        for pending in &self.admissions {
            if !pending.input_id.is_empty() {
                self.submitted.insert(pending.input_id.clone());
                self.admission_inputs
                    .insert(pending.admission_id.clone(), pending.input_id.clone());
            }
        }
        self.active_input = None;
        self.active_admission = None;
        self.active_generation = None;
        self.active_text.clear();
        let active: Vec<_> = self
            .admissions
            .iter()
            .filter(|a| a.status == "started" && a.execution_generation == Some(self.generation))
            .collect();
        if self.running && active.len() == 1 {
            self.active_admission = Some(active[0].admission_id.clone());
            self.active_input =
                (!active[0].input_id.is_empty()).then(|| active[0].input_id.clone());
            self.active_generation = active[0].execution_generation;
        }
        self.delta = None;
        self.reasoning.clear();
        self.tools.clear();
        self.activities.clear();
        self.error = None;
    }
    fn bind_persisted_turn(&mut self, payload: &Value) {
        let Some(receipt) = payload
            .get("persisted_turn")
            .filter(|r| r["complete"] == true)
        else {
            return;
        };
        if let Some(id) = receipt["user_row_id"].as_u64().filter(|id| *id > 0) {
            if let Some(row) = self.messages.iter_mut().rev().find(|m| {
                m.role == "user"
                    && self
                        .completed_input
                        .as_deref()
                        .is_some_and(|input| m.raw["input_id"].as_str() == Some(input))
            }) {
                if !row.raw.is_object() {
                    row.raw = serde_json::json!({});
                }
                row.raw["row_id"] = Value::from(id);
            }
        }
        if let Some(id) = receipt["final_assistant_row_id"]
            .as_u64()
            .filter(|id| *id > 0)
        {
            if let Some(row) = self
                .messages
                .iter_mut()
                .rev()
                .find(|m| m.role == "assistant")
            {
                if !row.raw.is_object() {
                    row.raw = serde_json::json!({});
                }
                row.raw["row_id"] = Value::from(id);
                row.raw["persisted_turn"] = receipt.clone();
            }
        }
    }
    fn bind_active(&mut self, event: &Event, reset: bool) {
        let admission = event.admission.as_ref();
        let generation_changed =
            event.generation.is_some() && event.generation != self.active_generation;
        let owner_changed = admission.is_some() && admission != self.active_admission.as_ref();
        if reset || generation_changed || owner_changed {
            self.active_input = None;
            self.active_admission = None;
            self.active_text.clear();
        }
        if let Some(id) = admission {
            self.active_admission = Some(id.clone());
            self.active_input = self.admission_inputs.get(id).cloned().or_else(|| {
                event
                    .payload
                    .get("input_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        } else if let Some(input) = event.payload.get("input_id").and_then(Value::as_str) {
            self.active_input = Some(input.into());
        }
        if event.generation.is_some() {
            self.active_generation = event.generation;
        }
    }
    pub(crate) fn apply_event(&mut self, event: &Event) -> bool {
        if event.session != self.runtime {
            return false;
        }
        // A terminal error cannot establish a different execution owner. An
        // admission-only failure settles that queue row without halting the
        // currently running turn (the canonical queue cancellation contract).
        if event.kind == "message.error" && self.canonical {
            if event.authority != Some(self.authority) {
                return false;
            }
            match event.generation {
                Some(generation) if generation == self.generation => {}
                None if event
                    .admission
                    .as_ref()
                    .is_some_and(|id| self.admissions.iter().any(|a| &a.admission_id == id)) => {}
                _ => return false,
            }
        }
        if let Some(epoch) = &event.epoch {
            if !self.replay_epoch.is_empty() && epoch != &self.replay_epoch {
                return false;
            }
        }
        if event.authority.is_some_and(|epoch| epoch < self.authority) {
            return false;
        }
        if event
            .generation
            .is_some_and(|generation| generation < self.generation)
        {
            return false;
        }
        if event.sequence.is_some_and(|seq| seq <= self.last_sequence) {
            return false;
        }
        if let Some(authority) = event.authority {
            self.authority = authority;
        }
        if let Some(seq) = event.sequence {
            self.last_sequence = seq;
        }
        if let Some(generation) = event.generation {
            self.generation = generation;
        }
        let p = &event.payload;
        match event.kind.as_str() {
            "session.info" => {
                if event.sequence.is_none_or(|seq| seq >= self.policy_sequence) {
                    self.policy.info(p.get("info").unwrap_or(p));
                    if let Some(seq) = event.sequence {
                        self.policy_sequence = seq;
                    }
                    if !self.metadata.is_object() {
                        self.metadata = serde_json::json!({});
                    }
                    if !self.metadata["info"].is_object() {
                        self.metadata["info"] = serde_json::json!({});
                    }
                    if let Some(info) = p.get("info").unwrap_or(p).as_object() {
                        self.metadata["info"]
                            .as_object_mut()
                            .unwrap()
                            .extend(info.clone());
                    }
                    if let Some(cwd) = p.get("cwd").and_then(Value::as_str) {
                        self.cwd = Some(cwd.into());
                    }
                }
            }
            "message.start" => {
                self.policy.turn_started();
                self.activities.clear();
                self.reasoning.clear();
                self.tools.clear();
                self.bind_active(event, true);
                self.running = true;
                self.reasoning.clear();
                self.tools.clear();
                self.delta = None;
                if p.get("display_kind").and_then(Value::as_str) != Some("hidden")
                    && event
                        .admission
                        .as_ref()
                        .is_some_and(|id| !self.admissions.iter().any(|a| &a.admission_id == id))
                {
                    if let Some(text) = p
                        .get("display_text")
                        .or_else(|| p.get("text"))
                        .and_then(Value::as_str)
                        .filter(|v| !v.is_empty())
                    {
                        self.messages.push(HistoryMessage {
                            role: "user".into(),
                            content: text.into(),
                            raw: p.clone(),
                        });
                    }
                }
            }
            "message.delta" => {
                self.bind_active(event, false);
                self.running = true;
                let Some(text) = p.get("text").and_then(Value::as_str) else {
                    self.delta = None;
                    return true;
                };
                let turn = p.get("turn_id").and_then(Value::as_str).unwrap_or("");
                let segment = p.get("segment").and_then(Value::as_u64).unwrap_or(0);
                let offset = p.get("offset").and_then(Value::as_u64).map(|v| v as usize);
                let same = self
                    .delta
                    .as_ref()
                    .is_some_and(|d| d.turn == turn && d.segment == segment);
                if !same {
                    if offset.is_some_and(|v| v != 0) {
                        self.error = Some(
                            "The response stream has a gap; reconnect to restore the transcript"
                                .into(),
                        );
                        return true;
                    }
                    self.messages.push(HistoryMessage {
                        role: "assistant".into(),
                        content: String::new(),
                        raw: p.clone(),
                    });
                    self.delta = Some(Delta {
                        turn: turn.into(),
                        segment,
                        units: 0,
                        row: self.messages.len() - 1,
                    });
                }
                let delta = self.delta.as_mut().unwrap();
                if let Some(offset) = offset {
                    if offset < delta.units {
                        return true;
                    }
                    if offset != delta.units {
                        self.error = Some(
                            "The response stream has a gap; reconnect to restore the transcript"
                                .into(),
                        );
                        return true;
                    }
                }
                if let Some(row) = self.messages.get_mut(delta.row) {
                    row.content.push_str(text);
                    self.active_text.push_str(text);
                    delta.units += text.encode_utf16().count();
                }
            }
            "message.interim" => {
                self.bind_active(event, false);
                if let Some(text) = p.get("text").and_then(Value::as_str) {
                    if let Some(delta) = self.delta.take() {
                        if let Some(row) = self.messages.get_mut(delta.row) {
                            if let Some(prefix) = self.active_text.strip_suffix(&row.content) {
                                self.active_text = format!("{prefix}{text}");
                            }
                            row.content = text.into();
                        }
                    } else if p.get("already_streamed") != Some(&Value::Bool(true)) {
                        self.messages.push(HistoryMessage {
                            role: "assistant".into(),
                            content: text.into(),
                            raw: p.clone(),
                        });
                        self.active_text.push_str(text);
                    }
                }
            }
            "message.complete" => {
                self.completion = self.completion.saturating_add(1);
                self.completed_admission = event.admission.clone();
                self.completed_input = event
                    .admission
                    .as_ref()
                    .and_then(|id| self.admission_inputs.get(id))
                    .cloned()
                    .or_else(|| p.get("input_id").and_then(Value::as_str).map(str::to_owned));
                self.completed_status = p.get("status").and_then(Value::as_str).map(str::to_owned);
                self.completed_text = p
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|v| !v.trim().is_empty())
                    .map(str::to_owned)
                    .or_else(|| {
                        self.delta
                            .as_ref()
                            .and_then(|d| self.messages.get(d.row))
                            .filter(|m| !m.content.trim().is_empty())
                            .map(|m| m.content.clone())
                    });
                if self
                    .completed_status
                    .as_deref()
                    .is_some_and(|status| status != "complete")
                {
                    self.completed_text = None;
                }
                if let Some(text) = p
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                {
                    if let Some(delta) = self.delta.take() {
                        if let Some(row) = self.messages.get_mut(delta.row) {
                            row.content = text.into();
                        }
                    } else {
                        self.messages.push(HistoryMessage {
                            role: "assistant".into(),
                            content: text.into(),
                            raw: p.clone(),
                        });
                    }
                }
                self.bind_persisted_turn(p);
                self.running = false;
                self.active_input = None;
                self.active_admission = None;
                self.active_generation = None;
                self.active_text.clear();
                self.prompts.clear();
                self.delta = None;
                if let Some(error) = p
                    .get("error")
                    .or_else(|| p.get("failure_reason"))
                    .and_then(Value::as_str)
                {
                    self.error = Some(error.into());
                }
                if let Some(id) = &event.admission {
                    self.admissions.retain(|a| &a.admission_id != id);
                }
            }
            "message.error" => {
                let queued_only =
                    self.canonical && event.generation.is_none() && event.admission.is_some();
                self.completion = self.completion.saturating_add(1);
                self.completed_admission = event.admission.clone();
                self.completed_input = event
                    .admission
                    .as_ref()
                    .and_then(|id| self.admission_inputs.get(id))
                    .cloned()
                    .or_else(|| p.get("input_id").and_then(Value::as_str).map(str::to_owned));
                self.completed_status = Some("error".into());
                self.completed_text = None;
                self.error = p
                    .get("message")
                    .or_else(|| p.get("error"))
                    .and_then(|v| {
                        v.as_str()
                            .or_else(|| v.get("message").and_then(Value::as_str))
                    })
                    .or_else(|| p.get("failure_reason").and_then(Value::as_str))
                    .map(str::to_owned)
                    .or_else(|| Some("The agent could not finish this turn".into()));
                if let Some(id) = &event.admission {
                    self.admissions.retain(|a| &a.admission_id != id);
                }
                if !queued_only {
                    self.running = false;
                    self.active_input = None;
                    self.active_admission = None;
                    self.active_generation = None;
                    self.active_text.clear();
                    self.prompts.clear();
                    self.delta = None;
                }
            }
            "reasoning.delta" | "thinking.delta" => {
                if let Some(text) = p.get("text").and_then(Value::as_str) {
                    self.reasoning.push_str(text);
                }
            }
            "tool.start" | "tool.complete" | "tool.progress" => {
                let name = p.get("name").and_then(Value::as_str).unwrap_or("Tool");
                let detail = p
                    .get("summary")
                    .or_else(|| p.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let state = if event.kind == "tool.complete" {
                    "Completed"
                } else {
                    "Running"
                };
                let id = p
                    .get("tool_call_id")
                    .or_else(|| p.get("id"))
                    .and_then(Value::as_str);
                let position = id.and_then(|id| {
                    self.activities
                        .iter()
                        .position(|v| v["id"].as_str() == Some(id))
                });
                let mut activity = p.clone();
                if !activity.is_object() {
                    activity = serde_json::json!({});
                }
                activity["id"] = id
                    .map(Value::from)
                    .unwrap_or_else(|| Value::from(format!("event-{}", self.last_sequence)));
                activity["state"] = Value::from(if event.kind == "tool.complete" {
                    "complete"
                } else {
                    "running"
                });
                if let Some(i) = position {
                    if let (Some(old), Some(new)) =
                        (self.activities[i].as_object_mut(), activity.as_object())
                    {
                        old.extend(new.clone());
                    }
                } else {
                    self.activities.push(activity);
                }
                self.tools.push(format!(
                    "{state} {name}{}{}",
                    if detail.is_empty() { "" } else { ": " },
                    detail
                ));
                if self.tools.len() > 40 {
                    self.tools.remove(0);
                }
            }
            "approval.request" | "clarify.request" => {
                let mut payload = p.clone();
                let object = payload.as_object_mut();
                if let Some(object) = object {
                    object.entry("kind").or_insert_with(|| {
                        Value::String(
                            if event.kind == "approval.request" {
                                "approval"
                            } else {
                                "clarify"
                            }
                            .into(),
                        )
                    });
                    object.entry("execution_generation").or_insert_with(|| {
                        Value::from(event.generation.unwrap_or(self.generation))
                    });
                    if let Ok(prompt) = serde_json::from_value::<PendingPrompt>(payload) {
                        self.prompts.retain(|row| row.prompt_id != prompt.prompt_id);
                        self.prompts.push(prompt);
                    }
                }
            }
            "request.cancel" => {
                if let Some(id) = p.get("id").and_then(Value::as_str) {
                    self.prompts.retain(|row| row.prompt_id != id);
                }
            }
            "approval.settled" | "clarify.settled" => {
                if let Some(id) = p.get("prompt_id").and_then(Value::as_str) {
                    self.prompts.retain(|row| row.prompt_id != id);
                }
            }
            "session.updated" => {
                let fresh_policy = event.sequence.is_none_or(|seq| seq >= self.policy_sequence)
                    && p["revision"]
                        .as_u64()
                        .is_none_or(|revision| revision >= self.revision);
                if !fresh_policy {
                    return true;
                }
                if fresh_policy {
                    self.policy.receipt(p, &Value::Null, None);
                    if let Some(info) = p.get("info") {
                        self.policy.info(info);
                    }
                    if let Some(seq) = event.sequence {
                        self.policy_sequence = seq;
                    }
                }
                if let Some(info) = p.get("info").and_then(Value::as_object) {
                    if !self.metadata.is_object() {
                        self.metadata = serde_json::json!({});
                    }
                    if !self.metadata["info"].is_object() {
                        self.metadata["info"] = serde_json::json!({});
                    }
                    self.metadata["info"]
                        .as_object_mut()
                        .unwrap()
                        .extend(info.clone());
                }
                if let Some(data) = self.metadata.as_object_mut() {
                    data.insert("last_session_update".into(), p.clone());
                }
                if let Some(cwd) = p.get("cwd").and_then(Value::as_str) {
                    self.cwd = Some(cwd.into());
                }
                if p.get("operation").and_then(Value::as_str) == Some("workspace") {
                    if let Some(cwd) = p.pointer("/payload/cwd").and_then(Value::as_str) {
                        self.cwd = Some(cwd.into());
                    }
                }
                if let Some(revision) = p.get("revision").and_then(Value::as_u64) {
                    self.revision = self.revision.max(revision);
                }
            }
            "error" => {
                self.error = p.get("message").and_then(Value::as_str).map(str::to_owned);
            }
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn event(seq: u64, payload: Value) -> Event {
        Event {
            session: "live".into(),
            kind: "message.delta".into(),
            payload,
            epoch: Some("epoch".into()),
            sequence: Some(seq),
            generation: Some(2),
            admission: None,
            authority: None,
        }
    }
    #[test]
    fn active_progress_binds_only_the_actual_admission_and_drops_interrupted_generation() {
        let mut view = ConversationView {
            runtime: "live".into(),
            replay_epoch: "epoch".into(),
            ..Default::default()
        };
        view.admission_inputs
            .insert("admission-old".into(), "input-old".into());
        view.admission_inputs
            .insert("admission-new".into(), "input-new".into());
        let mut old = event(
            1,
            json!({"text":"Old reply. ","turn_id":"old","segment":0,"offset":0}),
        );
        old.admission = Some("admission-old".into());
        view.apply_event(&old);
        assert_eq!(view.active_input.as_deref(), Some("input-old"));
        assert_eq!(view.active_text, "Old reply. ");
        let mut new = event(
            2,
            json!({"text":"New reply. ","turn_id":"new","segment":0,"offset":0}),
        );
        new.generation = Some(3);
        new.admission = Some("admission-new".into());
        view.apply_event(&new);
        assert_eq!(view.active_input.as_deref(), Some("input-new"));
        assert_eq!(view.active_generation, Some(3));
        assert_eq!(view.active_text, "New reply. ");
        old.sequence = Some(3);
        old.payload["offset"] = json!(11);
        assert!(!view.apply_event(&old));
        assert_eq!(view.active_text, "New reply. ");
        let mut unknown = event(
            4,
            json!({"text":"Unattributed", "turn_id":"x", "segment":0,"offset":0}),
        );
        unknown.generation = Some(4);
        unknown.admission = Some("other-admission".into());
        view.apply_event(&unknown);
        assert!(view.active_input.is_none());
        assert_eq!(view.active_generation, Some(4));
    }
    #[test]
    fn terminal_error_settles_only_its_input_and_preserves_other_running_admissions() {
        let mut view = ConversationView {
            runtime: "live".into(),
            replay_epoch: "epoch".into(),
            canonical: true,
            authority: 7,
            generation: 2,
            running: true,
            ..Default::default()
        };
        view.admission_inputs
            .insert("active".into(), "input-active".into());
        view.admission_inputs
            .insert("queued".into(), "input-queued".into());
        view.admissions.push(PendingAdmission {
            admission_id: "queued".into(),
            input_id: "input-queued".into(),
            text: "next".into(),
            status: "queued".into(),
            execution_generation: None,
            controls: vec![],
        });
        let mut start = event(
            1,
            json!({"text":"Partial response","turn_id":"turn","segment":0,"offset":0}),
        );
        start.authority = Some(7);
        start.admission = Some("active".into());
        assert!(view.apply_event(&start));
        let mut terminal = event(2, json!({"message":"Provider failed"}));
        terminal.kind = "message.error".into();
        terminal.authority = Some(6);
        terminal.admission = Some("active".into());
        assert!(!view.apply_event(&terminal));
        assert_eq!(view.completion, 0);
        assert!(view.running);
        terminal.authority = Some(7);
        terminal.generation = Some(1);
        assert!(!view.apply_event(&terminal));
        terminal.generation = None;
        terminal.admission = Some("queued".into());
        assert!(view.apply_event(&terminal));
        assert!(view.running);
        assert_eq!(view.completed_input.as_deref(), Some("input-queued"));
        assert_eq!(view.active_input.as_deref(), Some("input-active"));
        terminal.sequence = Some(3);
        terminal.generation = Some(2);
        terminal.admission = Some("active".into());
        assert!(view.apply_event(&terminal));
        assert!(!view.running);
        assert_eq!(view.completed_input.as_deref(), Some("input-active"));
        assert_eq!(view.completed_status.as_deref(), Some("error"));
        assert!(view.completed_text.is_none());
        assert_eq!(view.messages[0].content, "Partial response");
        assert!(!view.apply_event(&terminal));
        assert_eq!(view.completion, 2);
    }
    #[test]
    fn stream_is_session_epoch_sequence_and_utf16_offset_owned() {
        let mut view = ConversationView {
            runtime: "live".into(),
            replay_epoch: "epoch".into(),
            ..Default::default()
        };
        assert!(view.apply_event(&event(
            1,
            json!({"text":"😀","turn_id":"t","segment":0,"offset":0})
        )));
        assert!(!view.apply_event(&event(
            1,
            json!({"text":"duplicate","turn_id":"t","segment":0,"offset":0})
        )));
        view.apply_event(&event(
            2,
            json!({"text":" ok","turn_id":"t","segment":0,"offset":2}),
        ));
        assert_eq!(view.messages[0].content, "😀 ok");
        let mut wrong = event(3, json!({"text":"wrong","offset":5}));
        wrong.session = "other".into();
        assert!(!view.apply_event(&wrong));
        wrong.session = "live".into();
        wrong.epoch = Some("other-epoch".into());
        assert!(!view.apply_event(&wrong));
        view.apply_event(&event(
            3,
            json!({"text":"gap","turn_id":"t","segment":0,"offset":99}),
        ));
        assert_eq!(view.messages[0].content, "😀 ok");
        assert!(view.error.as_ref().unwrap().contains("gap"));
    }
}
