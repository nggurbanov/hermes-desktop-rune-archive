//! Batch questions are server requests, distinct from one-question authority
//! prompts. Preserve qids and confirmed early locks through reconnect.
use super::*;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;

pub(super) fn choices<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let raw = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(raw
        .into_iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .filter(|choice| {
            !choice.trim().is_empty()
                && choice
                    .trim_end_matches("(Recommended)")
                    .trim()
                    .chars()
                    .count()
                    <= 8000
        })
        .collect())
}

pub(super) fn answers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Option<String>>, D::Error> {
    Ok(Option::<BTreeMap<String, Option<String>>>::deserialize(deserializer)?.unwrap_or_default())
}
impl PendingPrompt {
    pub(crate) fn from_server_request(request: &Value, generation: u64) -> Option<Self> {
        let method = request.get("method")?.as_str()?;
        if !matches!(
            method,
            "clarify" | "clarify.request" | "approval" | "approval.request"
        ) {
            return None;
        }
        let id = request.get("id")?.as_str()?;
        let mut params = request.get("params")?.clone();
        let object = params.as_object_mut()?;
        object.insert(
            "kind".into(),
            Value::String(
                if method.starts_with("clarify") {
                    "clarify"
                } else {
                    "approval"
                }
                .into(),
            ),
        );
        object.insert("prompt_id".into(), Value::String(id.into()));
        object.insert("execution_generation".into(), Value::from(generation));
        object.insert("server_request".into(), Value::Bool(true));
        let mut prompt: Self = serde_json::from_value(params).ok()?;
        prompt
            .questions
            .retain(|q| !q.qid.trim().is_empty() && !q.question.trim().is_empty());
        for q in &mut prompt.questions {
            q.multi_select = q.multi_select && !q.choices.is_empty();
        }
        if prompt.kind == "clarify"
            && prompt.questions.is_empty()
            && prompt
                .question
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
        {
            return None;
        }
        Some(prompt)
    }
}
impl Conversation {
    pub fn lock_clarify(
        &mut self,
        connection: Connection,
        scope: &DraftScope,
        id: &str,
        qid: &str,
        answer: Option<String>,
    ) -> Result<(), String> {
        let key = self.key(scope);
        let view = self
            .views
            .get(&key)
            .ok_or("Attach this conversation first")?;
        let prompt = view
            .prompts
            .iter()
            .find(|p| {
                p.prompt_id == id && p.server_request && p.execution_generation == view.generation
            })
            .ok_or("This clarification has expired; refresh the conversation")?;
        let question = prompt
            .questions
            .iter()
            .find(|q| q.qid == qid)
            .ok_or("This question does not belong to the clarification")?;
        validate_answer(question, answer.as_deref())?;
        self.control(
            connection,
            key,
            worker::Control::LockClarify {
                runtime: view.runtime.clone(),
                stored: view.stored.clone(),
                generation: view.generation,
                authority: view.authority,
                id: id.into(),
                qid: Some(qid.into()),
                answer,
                answers: None,
            },
        )
    }
    pub fn skip_clarify(
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
        let prompt = view
            .prompts
            .iter()
            .find(|p| p.prompt_id == id && p.execution_generation == view.generation)
            .ok_or("This clarification has expired")?;
        if !prompt.server_request {
            return self.respond(connection, scope, id, "");
        }
        self.control(
            connection,
            key,
            worker::Control::LockClarify {
                runtime: view.runtime.clone(),
                stored: view.stored.clone(),
                generation: view.generation,
                authority: view.authority,
                id: id.into(),
                qid: None,
                answer: None,
                answers: None,
            },
        )
    }
    pub fn respond_clarify_batch(
        &mut self,
        connection: Connection,
        scope: &DraftScope,
        id: &str,
        answers: BTreeMap<String, Option<String>>,
    ) -> Result<(), String> {
        let key = self.key(scope);
        let view = self
            .views
            .get(&key)
            .ok_or("Attach this conversation first")?;
        let prompt = view
            .prompts
            .iter()
            .find(|p| {
                p.prompt_id == id && p.server_request && p.execution_generation == view.generation
            })
            .ok_or("This clarification has expired")?;
        for (qid, answer) in &answers {
            let question = prompt
                .questions
                .iter()
                .find(|q| &q.qid == qid)
                .ok_or("An answer names a question outside this clarification")?;
            validate_answer(question, answer.as_deref())?;
        }
        self.control(
            connection,
            key,
            worker::Control::LockClarify {
                runtime: view.runtime.clone(),
                stored: view.stored.clone(),
                generation: view.generation,
                authority: view.authority,
                id: id.into(),
                qid: None,
                answer: None,
                answers: Some(answers),
            },
        )
    }
}
fn validate_answer(question: &ClarifyQuestion, answer: Option<&str>) -> Result<(), String> {
    let Some(answer) = answer else {
        return Ok(());
    };
    if answer.len() > 16384 {
        return Err("The clarification answer is too long".into());
    }
    if question.multi_select {
        let values: Vec<String> = serde_json::from_str(answer)
            .map_err(|_| "A multiple-choice answer must be a JSON array of selected labels")?;
        if values.is_empty() || values.iter().any(|v| v.trim().is_empty()) {
            return Err("Choose at least one answer or skip this question".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn server_batch_preserves_qids_null_locks_and_multiple_choice_wire() {
        let request = json!({"id":"srq-batch","method":"clarify","params":{"session_id":"live","questions":[{"qid":"first","question":"Choose tools","choices":["Git (Recommended)","Tests",""],"multi_select":true},{"qid":"second","question":"Anything else?","choices":null}],"answers":{"first":"[\"Git\",\"Tests\"]","second":null}}});
        let prompt = PendingPrompt::from_server_request(&request, 9).unwrap();
        assert!(prompt.server_request);
        assert_eq!(prompt.execution_generation, 9);
        assert_eq!(prompt.questions[0].qid, "first");
        assert_eq!(prompt.questions[0].choices.len(), 2);
        assert!(prompt.questions[1].choices.is_empty());
        assert_eq!(prompt.answers.get("second"), Some(&None));
        assert!(validate_answer(&prompt.questions[0], Some("[\"Git\",\"Tests\"]")).is_ok());
        assert!(validate_answer(&prompt.questions[0], Some("Git, Tests")).is_err());
    }
}
