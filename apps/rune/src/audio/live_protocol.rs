//! The same GPT-Live data-channel contract for native and WebView media.
use super::live::Event;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
#[derive(Default)]
pub(super) struct Protocol {
    pub closed: bool,
    pub started: bool,
    pub closing: bool,
    pub delegation: Option<String>,
    fragments: Vec<(String, String, u64)>,
    seen: HashSet<String>,
    utterance: String,
    last_user: Option<Instant>,
    sequence: u64,
}
impl Protocol {
    pub fn vendor(&mut self, event: &Value) -> Vec<Event> {
        if self.closed {
            return vec![];
        }
        let mut events = vec![];
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "session.started" => {
                self.started = true;
                events.push(Event::Ready);
            }
            "session.input_transcript.delta" | "session.output_transcript.delta" => {
                let user = event["type"] == "session.input_transcript.delta";
                let speaker = if user { "user" } else { "assistant" };
                let text = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                if user {
                    self.utterance.push_str(&text);
                    self.last_user = Some(Instant::now());
                }
                self.fragments.push((
                    speaker.into(),
                    text.clone(),
                    event.get("end_ms").and_then(Value::as_u64).unwrap_or(0),
                ));
                if self.fragments.len() > 2000 {
                    self.fragments.drain(..500);
                }
                events.push(Event::Partial(text));
            }
            "session.delegation.created" if !self.closing => {
                let Some(id) = event
                    .pointer("/delegation/id")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                else {
                    return vec![Event::Error(
                        "GPT-Live delegated a task without an identity".into(),
                    )];
                };
                if !self.seen.insert(id.into()) {
                    return vec![];
                }
                self.delegation = Some(id.into());
                let (text, context) = delegation_text(&self.fragments);
                if super::stop_word(&text) {
                    self.closed = true;
                    events.push(Event::Closed);
                } else {
                    events.push(Event::Delegation { text, context });
                }
            }
            "error" => {
                if event.pointer("/error/code").and_then(Value::as_str)
                    != Some("context_injection_incomplete")
                {
                    events.push(Event::Warning(
                        event
                            .pointer("/error/message")
                            .and_then(Value::as_str)
                            .unwrap_or("GPT-Live error")
                            .into(),
                    ));
                }
            }
            "session.closed" => {
                self.closed = true;
                events.push(Event::Closed);
            }
            _ => {}
        }
        events
    }
    pub fn command(&mut self, command: &Value) -> Vec<Value> {
        let kind = command.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "speak" | "think" => {
                let text = command
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let chunks = if kind == "think" {
                    let mut units = 0;
                    let text: String = text
                        .chars()
                        .take_while(|character| {
                            units += character.len_utf16();
                            units <= 1400
                        })
                        .collect();
                    if text.is_empty() { vec![] } else { vec![text] }
                } else {
                    commentary_chunks(&text, 1400)
                };
                chunks
                    .into_iter()
                    .map(|chunk| {
                        self.append(
                            if kind == "speak" {
                                "session.commentary.append"
                            } else {
                                "session.thinking.append"
                            },
                            chunk,
                            self.delegation.clone(),
                        )
                    })
                    .collect()
            }
            "mute" => {
                self.sequence += 1;
                vec![
                    json!({"type":if command["muted"]==true{"session.input_audio.mute"}else{"session.input_audio.unmute"},"event_id":format!("mute-{}",self.sequence)}),
                ]
            }
            "stop-turn" => vec![self.append(
                "session.instructions.append",
                "The user has finished speaking. Respond now to what they said.".into(),
                None,
            )],
            "close" => {
                self.closing = true;
                vec![json!({"type":"session.close"})]
            }
            _ => vec![],
        }
    }
    fn append(&mut self, kind: &str, content: String, id: Option<String>) -> Value {
        self.sequence += 1;
        json!({"type":kind,"content":content,"delegation_id":id,"event_id":format!("hermes-voice-{}",self.sequence)})
    }
    pub fn stop_phrase(&mut self) -> bool {
        if self
            .last_user
            .is_some_and(|time| time.elapsed() > Duration::from_millis(1500))
        {
            self.last_user = None;
            let stopped = super::stop_word(&self.utterance);
            self.utterance.clear();
            return stopped;
        }
        false
    }
}
// Vendor limits count UTF-16 units. Keep sentences together when possible,
// and split long words only at whole Unicode scalar boundaries.
fn commentary_chunks(text: &str, limit: usize) -> Vec<String> {
    let mut sentences = vec![];
    let mut start = 0;
    let mut previous = None;
    for (index, character) in text.char_indices() {
        if character == ' ' && previous.is_some_and(|c| matches!(c, '.' | '!' | '?')) {
            sentences.push(&text[start..index]);
            start = index + 1;
        }
        previous = Some(character);
    }
    if start < text.len() {
        sentences.push(&text[start..]);
    }
    let mut chunks = vec![];
    let mut current = String::new();
    for sentence in sentences {
        if sentence.encode_utf16().count() > limit {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let mut units = 0;
            for character in sentence.chars() {
                if units + character.len_utf16() > limit && !current.is_empty() {
                    chunks.push(std::mem::take(&mut current));
                    units = 0;
                }
                current.push(character);
                units += character.len_utf16();
            }
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
        } else {
            if current.encode_utf16().count()
                + usize::from(!current.is_empty())
                + sentence.encode_utf16().count()
                > limit
            {
                chunks.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(sentence);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
fn delegation_text(fragments: &[(String, String, u64)]) -> (String, String) {
    let last = fragments.last().map(|f| f.2).unwrap_or(0);
    let floor = last.saturating_sub(300_000);
    let mut turns: Vec<(String, String)> = vec![];
    for (speaker, text, _) in fragments.iter().filter(|f| f.2 >= floor).skip(
        fragments
            .iter()
            .filter(|f| f.2 >= floor)
            .count()
            .saturating_sub(80),
    ) {
        if let Some(last) = turns.last_mut().filter(|t| &t.0 == speaker) {
            last.1.push_str(text);
        } else {
            turns.push((speaker.clone(), text.clone()));
        }
    }
    let text = turns
        .iter()
        .rev()
        .find(|t| t.0 == "user")
        .map(|t| t.1.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let context = turns
        .iter()
        .map(|(speaker, text)| {
            format!(
                "{}: {}",
                if speaker == "user" {
                    "User"
                } else {
                    "Voice assistant"
                },
                text.split_whitespace().collect::<Vec<_>>().join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    (
        if text.is_empty() {
            context
                .chars()
                .rev()
                .take(400)
                .collect::<String>()
                .chars()
                .rev()
                .collect()
        } else {
            text
        },
        context,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commentary_enforces_vendor_limit_for_long_unicode_words_and_sentences() {
        let text = format!("A short sentence. {} And another!", "🦀".repeat(1600));
        let mut state = Protocol::default();
        let frames = state.command(&json!({"type":"speak","text":text}));
        assert!(frames.len() >= 4);
        assert_eq!(frames[0]["content"], "A short sentence.");
        assert!(
            frames
                .iter()
                .all(|f| f["content"].as_str().unwrap().encode_utf16().count() <= 1400)
        );
        let joined = frames
            .iter()
            .map(|f| f["content"].as_str().unwrap())
            .collect::<String>();
        assert_eq!(joined.chars().filter(|c| *c == '🦀').count(), 1600);
        assert_eq!(state.command(&json!({"type":"think","text":text})).len(), 1);
    }
    #[test]
    fn delegation_identity_context_and_commentary_remain_bound_to_the_same_vendor_turn() {
        let mut state = Protocol::default();
        for (speaker, text) in [
            ("input", "Find "),
            ("input", "Hermes"),
            ("output", "I will check."),
        ] {
            state.vendor(&json!({"type":format!("session.{speaker}_transcript.delta"),"delta":text,"end_ms":1}));
        }
        let event = json!({"type":"session.delegation.created","delegation":{"id":"task-1"}});
        let events = state.vendor(&event);
        assert!(
            matches!(&events[0],Event::Delegation{text,context}if text=="Find Hermes"&&context=="User: Find Hermes\nVoice assistant: I will check.")
        );
        assert!(state.vendor(&event).is_empty());
        let frames = state.command(&json!({"type":"speak","text":"Found it"}));
        assert_eq!(frames[0]["type"], "session.commentary.append");
        assert_eq!(frames[0]["delegation_id"], "task-1");
        state.command(&json!({"type":"close"}));
        assert!(
            state
                .vendor(&json!({"type":"session.delegation.created","delegation":{"id":"late"}}))
                .is_empty()
        );
    }
}
