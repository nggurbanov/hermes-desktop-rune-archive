use super::*;
const LIMIT: usize = 5 * 1024 * 1024;
fn data_url(value: &Value) -> Result<std::sync::Arc<str>, CallError> {
    let data = value
        .as_str()
        .or(value["data_url"].as_str())
        .or(value["dataUrl"].as_str())
        .ok_or("The stored attachment returned no bytes")?;
    let (head, encoded) = data
        .split_once(',')
        .ok_or("The stored attachment returned invalid data")?;
    if !head.starts_with("data:") || !head.ends_with(";base64") || data.len() > LIMIT * 2 {
        return Err(
            "The stored attachment has an unsupported encoding or exceeds the safe replay limit"
                .into(),
        );
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "The stored attachment encoding is invalid")?;
    if bytes.len() > LIMIT {
        return Err("This stored attachment exceeds this gateway's 5 MiB replay limit. History has not been changed".into());
    }
    Ok(data.into())
}
impl Worker {
    pub(super) fn prepare_replay(
        &mut self,
        entry: &mut Prepared,
        journal: &Journal,
        snapshot: &Snapshot,
    ) -> Result<(), CallError> {
        let Some(bundle) = entry.input.replay.clone() else {
            return Ok(());
        };
        bundle.verify(&self.connection, &self.scope)?;
        if !snapshot.canonical {
            return Err("This gateway does not support safe canonical multimodal replay. History has not been changed".into());
        }
        if bundle.attachments.len() > 16 {
            return Err("This turn has too many attachments to safely replay at once".into());
        }
        if entry.text.is_none() {
            entry.text = Some(bundle.transport.clone());
            journal.save(entry)?;
        }
        // Freeze every byte before a rewind. A failed fetch/size/version check
        // cannot destroy a turn and leave its attachments unrecoverable.
        for index in 0..bundle.attachments.len() {
            if entry.input.replay.as_ref().unwrap().attachments[index]
                .frozen
                .is_none()
            {
                let attachment = &bundle.attachments[index];
                let value = if attachment.source.starts_with("data:") {
                    json!(attachment.source)
                } else {
                    self.transport()?
                        .replay_media(&attachment.source, &snapshot.stored)?
                };
                let frozen = data_url(&value)?;
                if attachment.image && !frozen.starts_with("data:image/") {
                    return Err("A stored image no longer contains image bytes. History has not been changed".into());
                }
                entry.input.replay.as_mut().unwrap().attachments[index].frozen = Some(frozen);
                journal.save(entry)?;
            }
        }
        let total: usize = entry
            .input
            .replay
            .as_ref()
            .unwrap()
            .attachments
            .iter()
            .filter_map(|a| a.frozen.as_ref())
            .map(|v| v.len())
            .sum();
        if total > 64 * 1024 * 1024 {
            return Err("This turn exceeds the 64 MiB frozen replay budget. Its history has not been changed".into());
        }
        if entry.input.rewind.as_ref().is_some_and(|r| r.restore_only) {
            return Ok(());
        }
        for index in entry.replay_attached..bundle.attachments.len() {
            let attachment = entry.input.replay.as_ref().unwrap().attachments[index].clone();
            let bytes = attachment
                .frozen
                .as_ref()
                .ok_or("A restored attachment has no frozen bytes")?
                .clone();
            if attachment.image {
                let value = self
                    .transport()?
                    .upload_image(bytes.to_string(), &attachment.name)?;
                entry.images.push(value);
            } else {
                let value = self.transport()?.call(
                    "file.attach",
                    json!({"session_id":snapshot.runtime,"name":attachment.name,"data_url":bytes}),
                )?;
                if value["attached"] != true {
                    return Err(
                        "The agent refused the restored file. History has not been changed".into(),
                    );
                }
                let reference = value["ref_text"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or("The restored file has no acknowledged reference")?
                    .to_owned();
                let text = entry.text.as_deref().unwrap_or("");
                entry.text = Some(format!(
                    "{reference}{}{}",
                    if text.is_empty() { "" } else { "\n\n" },
                    text
                ));
                entry.replay_refs.push(reference);
            }
            entry.replay_attached = index + 1;
            journal.save(entry)?;
        }
        Ok(())
    }
}
