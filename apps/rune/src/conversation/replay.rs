//! Frozen original multimodal input; display prose is never a substitute for its payload.
use crate::{
    composer::DraftScope,
    gateway::{Connection, HistoryMessage},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayBundle {
    pub principal: String,
    pub profile: String,
    #[serde(
        default,
        skip_serializing_if = "crate::source::ProfileIntent::is_backend"
    )]
    pub profile_intent: crate::source::ProfileIntent,
    pub session: String,
    pub row: u64,
    pub visible: String,
    pub transport: String,
    pub attachments: Vec<ReplayAttachment>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayAttachment {
    pub source: std::sync::Arc<str>,
    pub name: String,
    pub mime: Option<String>,
    pub image: bool,
    #[serde(default)]
    pub frozen: Option<std::sync::Arc<str>>,
}
impl ReplayBundle {
    pub fn capture(
        connection: &Connection,
        owner: &DraftScope,
        row: &HistoryMessage,
    ) -> Result<Self, String> {
        if row.role != "user" || row.raw["display_kind"] == "hidden" {
            return Err("Only a visible user turn can be replayed".into());
        }
        let id = row
            .row_id()
            .ok_or("Wait until this turn has a durable identity")?;
        let mut parts = vec![];
        let mut text = vec![];
        match row.raw.get("content") {
            Some(Value::String(value)) => text.push(value.clone()),
            Some(Value::Array(rows)) => {
                for value in rows {
                    collect(value, &mut text, &mut parts)?;
                }
            }
            Some(Value::Null) | None => text.push(row.content.clone()),
            _ => {
                return Err(
                    "This gateway's stored message content cannot be safely replayed".into(),
                );
            }
        }
        if let Some(rows) = row.raw["attachments"].as_array() {
            for value in rows {
                collect(value, &mut text, &mut parts)?;
            }
        }
        let mut seen = std::collections::HashSet::new();
        parts.retain(|a| seen.insert((a.source.clone(), a.image)));
        Ok(Self {
            principal: crate::client::connection_key(connection)?,
            profile: owner.profile.clone(),
            profile_intent: owner.profile_intent,
            session: owner
                .session
                .clone()
                .ok_or("Replay needs a stored conversation")?,
            row: id,
            visible: row.content.clone(),
            transport: text.join("\n"),
            attachments: parts,
        })
    }
    pub fn verify(&self, connection: &Connection, owner: &DraftScope) -> Result<(), String> {
        if self.transport.len() > 2 * 1024 * 1024
            || self.attachments.len() > 16
            || self.attachments.iter().any(|a| a.name.len() > 1024)
        {
            return Err("The original input exceeds this gateway’s replay limits. Its history has not been changed.".into());
        }
        if self.principal != crate::client::connection_key(connection)?
            || self.profile != owner.profile
            || self.profile_intent != owner.profile_intent
            || owner.session.as_deref() != Some(self.session.as_str())
            || self.row == 0
        {
            return Err(
                "The restored input belongs to another connection, profile or conversation".into(),
            );
        }
        Ok(())
    }
    pub fn with_text(&self, edited: &str) -> Result<Self, String> {
        let mut next = self.clone();
        if edited == self.visible {
            return Ok(next);
        }
        if self.transport == self.visible {
            next.transport = edited.into();
        } else if !self.visible.is_empty() && self.transport.matches(&self.visible).count() == 1 {
            next.transport = self.transport.replacen(&self.visible, edited, 1);
        } else {
            return Err("The stored display text has no unambiguous editable region in its original input. Its hidden context and history have not been changed.".into());
        }
        next.visible = edited.into();
        Ok(next)
    }
}
fn collect(
    value: &Value,
    text: &mut Vec<String>,
    out: &mut Vec<ReplayAttachment>,
) -> Result<(), String> {
    let kind = value["type"].as_str().unwrap_or("");
    if matches!(kind, "text" | "input_text" | "output_text") {
        text.push(
            value["text"]
                .as_str()
                .ok_or("A stored text part is malformed")?
                .into(),
        );
        return Ok(());
    }
    let mime = value["mime"]
        .as_str()
        .or(value["mime_type"].as_str())
        .or(value["source"]["media_type"].as_str())
        .map(str::to_owned);
    let image = matches!(kind, "image" | "image_url" | "input_image")
        || mime.as_deref().is_some_and(|m| m.starts_with("image/"));
    if !image
        && !matches!(kind, "file" | "file_ref" | "attachment" | "input_file")
        && !(kind.is_empty() && value["path"].is_string())
    {
        return Err(format!(
            "The stored {kind} part is not supported for lossless replay by this gateway"
        ));
    }
    let source = value["path"]
        .as_str()
        .or(value["file_path"].as_str())
        .or(value["url"].as_str())
        .or(value["image_url"]["url"].as_str())
        .or(value["image_url"].as_str())
        .or(value["source"]["url"].as_str())
        .map(str::to_owned)
        .or_else(|| {
            Some(format!(
                "data:{};base64,{}",
                mime.as_ref()?,
                value["source"]["data"].as_str()?
            ))
        })
        .ok_or("The stored attachment has no recoverable source")?;
    if source.trim().is_empty() || source.contains('\0') {
        return Err("The stored attachment address is invalid".into());
    }
    out.push(ReplayAttachment {
        frozen: None,
        source: source.into(),
        name: value["name"]
            .as_str()
            .or(value["filename"].as_str())
            .unwrap_or(if image {
                "Restored image"
            } else {
                "Restored file"
            })
            .into(),
        mime,
        image,
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn connection() -> Connection {
        Connection::Url {
            endpoint: "https://fixture.invalid".into(),
            token: None,
        }
    }
    fn owner() -> DraftScope {
        DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 1,
            profile: "work".into(),
            session: Some("stored".into()),
        }
    }
    #[test]
    fn replay_keeps_images_files_and_hidden_context_while_only_replacing_public_text() {
        let row = HistoryMessage::from_row(
            &json!({"row_id":41,"role":"user","text":"Public request","content":[{"type":"text","text":"private-prefix\nPublic request\nprivate-suffix"},{"type":"image_url","image_url":{"url":"/remote/picture.png"}},{"type":"file","path":"/remote/notes.txt","name":"Notes"}]}),
        );
        let replay = ReplayBundle::capture(&connection(), &owner(), &row).unwrap();
        let edited = replay.with_text("Edited request").unwrap();
        assert_eq!(
            edited.transport,
            "private-prefix\nEdited request\nprivate-suffix"
        );
        assert_eq!(edited.attachments.len(), 2);
        assert_eq!(edited.attachments, replay.attachments);
        assert!(
            replay
                .verify(
                    &connection(),
                    &DraftScope {
                        profile: "other".into(),
                        ..owner()
                    }
                )
                .is_err()
        );
    }
    #[test]
    fn opaque_or_ambiguous_parts_never_get_silently_dropped_on_edit() {
        let row = HistoryMessage::from_row(
            &json!({"row_id":4,"role":"user","text":"display","content":[{"type":"native_assistant","text":"opaque"}]}),
        );
        assert!(ReplayBundle::capture(&connection(), &owner(), &row).is_err());
        let row = HistoryMessage::from_row(
            &json!({"row_id":4,"role":"user","text":"same","content":"same and same"}),
        );
        let replay = ReplayBundle::capture(&connection(), &owner(), &row).unwrap();
        assert!(replay.with_text("new").is_err());
    }
}

#[cfg(test)]
mod profile_intent_tests {
    use super::*;
    use crate::source::ProfileIntent;
    #[test]
    fn replay_keeps_original_owner_intent_across_serialization() {
        let connection = Connection::Url {
            endpoint: "https://fixture.invalid".into(),
            token: None,
        };
        let raw = DraftScope {
            draft_origin: None,
            connection: 1,
            profile: "default".into(),
            profile_intent: ProfileIntent::Backend,
            session: Some("same".into()),
        };
        let selected = DraftScope {
            profile_intent: ProfileIntent::NativeSelf,
            ..raw.clone()
        };
        let row = HistoryMessage::from_row(
            &serde_json::json!({"row_id": 1, "role": "user", "text": "hello", "content": "hello"}),
        );
        let replay = ReplayBundle::capture(&connection, &raw, &row).unwrap();
        let legacy = serde_json::to_value(&replay).unwrap();
        assert!(legacy.get("profile_intent").is_none());
        let replay: ReplayBundle = serde_json::from_value(legacy).unwrap();
        assert!(replay.verify(&connection, &raw).is_ok());
        assert!(replay.verify(&connection, &selected).is_err());
        let selected_replay = ReplayBundle::capture(&connection, &selected, &row).unwrap();
        let restored: ReplayBundle =
            serde_json::from_value(serde_json::to_value(selected_replay).unwrap()).unwrap();
        assert!(restored.verify(&connection, &selected).is_ok());
        assert!(restored.verify(&connection, &raw).is_err());
    }
}
