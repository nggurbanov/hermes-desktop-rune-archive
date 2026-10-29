//! Incremental commentary is exclusively for GPT-Live. Chained TTS still
//! receives only the final, confirmed reply from the owning admission.
use super::*;
#[derive(Default)]
pub(super) struct Progress {
    input: String,
    spoken: usize,
    tool: Option<String>,
    finished: bool,
    text: String,
}
impl Progress {
    fn update(
        &mut self,
        input: &str,
        text: &str,
        tool: Option<&str>,
        flush: bool,
    ) -> Vec<serde_json::Value> {
        if self.input != input {
            *self = Self {
                input: input.into(),
                ..Default::default()
            };
        }
        if self.finished {
            return vec![];
        }
        let mut commands = vec![];
        if let Some(tool) = tool.filter(|value| !value.trim().is_empty()) {
            if self.tool.as_deref() != Some(tool) {
                self.tool = Some(tool.into());
                commands.push(serde_json::json!({"type":"think","text":format!("Hermes is working: {tool}. Not done yet.")}));
            }
        }
        if self.text == text && !flush {
            return commands;
        }
        self.text = text.into();
        let spoken = super::speech::sanitize(text);
        let mut units = 0;
        let mut begin = spoken.len();
        let mut boundary = 0;
        let mut punctuation = false;
        for (offset, character) in spoken.char_indices() {
            if units == self.spoken {
                begin = offset;
            }
            if character.is_whitespace() && punctuation {
                boundary = offset;
            }
            punctuation = matches!(character, '.' | '!' | '?' | '。' | '！' | '？' | '…')
                || (punctuation && matches!(character, '"' | '\'' | '”' | '’' | ')' | ']'));
            units += character.len_utf16();
        }
        let cut = if flush { spoken.len() } else { boundary };
        if cut > begin {
            let text = spoken[begin..cut].trim();
            if !text.is_empty() {
                commands.push(serde_json::json!({"type":"speak","text":text}));
            }
            self.spoken = spoken[..cut].encode_utf16().count();
        }
        if flush {
            self.finished = true;
        }
        commands
    }
}
impl Audio {
    pub fn live_progress(
        &mut self,
        scope: &DraftScope,
        submission_id: &str,
        text: &str,
        tool: Option<&str>,
    ) -> Result<(), String> {
        let live = self
            .live
            .get_mut(scope)
            .ok_or("This conversation has no active GPT-Live session")?;
        let progress = self.live_progress.entry(scope.clone()).or_default();
        for command in progress.update(submission_id, text, tool, false) {
            if command["type"] == "think" {
                live.think(command["text"].as_str().unwrap())?;
            } else {
                live.speak(command["text"].as_str().unwrap())?;
            }
        }
        Ok(())
    }
    pub(super) fn finish_live(
        &mut self,
        scope: &DraftScope,
        text: &str,
        failed: bool,
    ) -> Result<(), String> {
        let live = self
            .live
            .get_mut(scope)
            .ok_or("This conversation has no active GPT-Live session")?;
        let progress = self.live_progress.entry(scope.clone()).or_default();
        if progress.finished {
            return Ok(());
        }
        let input = progress.input.clone();
        let already_spoken = progress.spoken > 0;
        for command in progress.update(&input, if failed { "" } else { text }, None, true) {
            live.speak(command["text"].as_str().unwrap())?;
        }
        if progress.spoken == 0 && !already_spoken {
            live.think(if failed {
                "Hermes could not complete that request."
            } else {
                "Hermes finished that request without a spoken result."
            })?;
        }
        let view = self.views.entry(scope.clone()).or_default();
        view.waiting_for_hermes = false;
        view.status = Status::Listening;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streaming_speaks_only_completed_sentences_and_flushes_one_unspoken_unicode_tail() {
        let mut progress = Progress::default();
        assert!(
            progress
                .update("input-1", "😀 First sentence", None, false)
                .is_empty()
        );
        let first = progress.update(
            "input-1",
            "😀 First sentence. Next tail",
            Some("Search"),
            false,
        );
        assert_eq!(first.len(), 2);
        assert_eq!(first[1]["text"], "First sentence.");
        assert!(
            progress
                .update(
                    "input-1",
                    "😀 First sentence. Next tail",
                    Some("Search"),
                    false
                )
                .is_empty()
        );
        let second = progress.update(
            "input-1",
            "😀 First sentence. Next tail. Final bit",
            None,
            false,
        );
        assert_eq!(second.len(), 1);
        assert_eq!(second[0]["text"], "Next tail.");
        let last = progress.update(
            "input-1",
            "😀 First sentence. Next tail. Final bit",
            None,
            true,
        );
        assert_eq!(last[0]["text"], "Final bit");
        assert!(
            progress
                .update(
                    "input-1",
                    "😀 First sentence. Next tail. Final bit",
                    None,
                    true
                )
                .is_empty()
        );
        assert!(
            progress
                .update("input-2", "Another request. Tail", None, false)
                .iter()
                .any(|c| c["text"] == "Another request.")
        );
    }
}
