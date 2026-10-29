//! Drafts and model favorites belong to a connection principal, profile and stored session.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct State {
    version: u32,
    drafts: Vec<SavedDraft>,
    favorites: Vec<(String, String)>,
}
#[derive(Serialize, Deserialize)]
struct SavedDraft {
    profile: String,
    session: Option<String>,
    text: String,
    model: Option<(String, String)>,
    reasoning: Option<String>,
    service_tier: Option<String>,
    approval_mode: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    cwd_explicit: bool,
    model_changed: bool,
    reasoning_changed: bool,
    service_tier_changed: bool,
    approval_changed: bool,
    attachments: Vec<PathBuf>,
    #[serde(default)]
    replay: Option<crate::conversation::ReplayBundle>,
}
pub(super) struct Persistence {
    epoch: u64,
    path: PathBuf,
    previous: Vec<u8>,
    pending: Option<(Vec<u8>, f64)>,
    scanned_revision: Option<u64>,
}
impl Composer {
    pub fn bind_storage(&mut self, connection: &Connection, epoch: u64) -> Result<(), String> {
        if self.persisted.as_ref().is_some_and(|s| s.epoch == epoch) {
            return Ok(());
        }
        if let Some(p) = &self.persisted {
            let bytes = self.storage_bytes(p.epoch)?;
            if bytes != p.previous {
                crate::client::private_write(&p.path, &bytes)?;
            }
        }
        let path = crate::client::state_dir()?.join("drafts").join(format!(
            "{}.json",
            crate::client::connection_key(connection)?
        ));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(_) => return Err("Saved drafts could not be read".into()),
        };
        let state: State = if bytes.is_empty() {
            State {
                version: 2,
                ..Default::default()
            }
        } else {
            let state: State = serde_json::from_slice(&bytes)
                .map_err(|_| "Saved drafts are invalid; their file was retained")?;
            if !matches!(state.version, 1 | 2) {
                return Err("Saved drafts use a newer client format".into());
            }
            state
        };
        for saved in state.drafts {
            let scope = DraftScope {
                connection: epoch,
                profile: saved.profile,
                session: saved.session,
            };
            self.drafts.entry(scope).or_insert_with(|| Draft {
                text: TextEdit::new(saved.text),
                model: saved.model.map(|(provider, id)| ModelKey { provider, id }),
                reasoning: saved.reasoning,
                service_tier: saved.service_tier,
                approval_mode: saved.approval_mode,
                cwd: saved.cwd,
                cwd_explicit: saved.cwd_explicit,
                replay: saved.replay,
                model_changed: saved.model_changed,
                reasoning_changed: saved.reasoning_changed,
                service_tier_changed: saved.service_tier_changed,
                approval_changed: saved.approval_changed,
                attachments: saved
                    .attachments
                    .into_iter()
                    .map(|path| DraftAttachment {
                        name: path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("attachment")
                            .into(),
                        path,
                        preview: None,
                    })
                    .collect(),
            });
        }
        self.favorites.extend(
            state
                .favorites
                .into_iter()
                .map(|(provider, id)| (epoch, provider, id)),
        );
        self.persisted = Some(Persistence {
            epoch,
            path,
            previous: bytes,
            pending: None,
            scanned_revision: None,
        });
        Ok(())
    }
    fn storage_bytes(&self, epoch: u64) -> Result<Vec<u8>, String> {
        let mut drafts: Vec<_> = self
            .drafts
            .iter()
            .filter(|(scope, _)| scope.connection == epoch)
            .map(|(scope, draft)| SavedDraft {
                profile: scope.profile.clone(),
                session: scope.session.clone(),
                text: draft.text.text.clone(),
                model: draft
                    .model
                    .as_ref()
                    .map(|m| (m.provider.clone(), m.id.clone())),
                reasoning: draft.reasoning.clone(),
                service_tier: draft.service_tier.clone(),
                approval_mode: draft.approval_mode.clone(),
                cwd: draft.cwd.clone(),
                cwd_explicit: draft.cwd_explicit,
                model_changed: draft.model_changed,
                reasoning_changed: draft.reasoning_changed,
                service_tier_changed: draft.service_tier_changed,
                approval_changed: draft.approval_changed,
                attachments: draft.attachments.iter().map(|a| a.path.clone()).collect(),
                replay: draft.replay.clone(),
            })
            .collect();
        drafts.sort_by(|a, b| (&a.profile, &a.session).cmp(&(&b.profile, &b.session)));
        let mut favorites: Vec<_> = self
            .favorites
            .iter()
            .filter(|(connection, _, _)| *connection == epoch)
            .map(|(_, provider, id)| (provider.clone(), id.clone()))
            .collect();
        favorites.sort();
        serde_json::to_vec(&State {
            version: 2,
            drafts,
            favorites,
        })
        .map_err(|_| "Drafts could not be encoded".into())
    }
    pub fn poll_storage(&mut self, now: f64) -> Result<Option<std::time::Duration>, String> {
        let Some(epoch) = self.persisted.as_ref().map(|p| p.epoch) else {
            return Ok(None);
        };
        let revision = self.storage_revision;
        if self.persisted.as_ref().unwrap().scanned_revision != Some(revision) {
            let bytes = self.storage_bytes(epoch)?;
            let p = self.persisted.as_mut().unwrap();
            p.scanned_revision = Some(revision);
            if p.pending
                .as_ref()
                .map(|(bytes, _)| bytes)
                .unwrap_or(&p.previous)
                != &bytes
            {
                p.pending = Some((bytes, now + 0.35));
            }
        }
        let p = self.persisted.as_mut().unwrap();
        if let Some((_, due)) = &p.pending {
            if now < *due {
                return Ok(Some(std::time::Duration::from_secs_f64(
                    (*due - now).max(0.001),
                )));
            }
        }
        if let Some((bytes, _)) = &p.pending {
            if let Err(error) = crate::client::private_write(&p.path, bytes) {
                p.pending.as_mut().unwrap().1 = now + 1.;
                return Err(error);
            }
            p.previous = bytes.clone();
            p.pending = None;
        }
        Ok(None)
    }
}

impl Drop for Composer {
    fn drop(&mut self) {
        if let Some(p) = &self.persisted {
            if let Ok(bytes) = self.storage_bytes(p.epoch) {
                if bytes != p.previous {
                    let _ = crate::client::private_write(&p.path, &bytes);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_snapshots_preserve_owner_and_choices_without_other_connection_state() {
        let mut composer = Composer::new();
        let scope = DraftScope {
            connection: 3,
            profile: "work".into(),
            session: Some("stored".into()),
        };
        let draft = composer.draft_mut(scope);
        draft.text.replace_text("Do the next step");
        draft.reasoning = Some("high".into());
        draft.reasoning_changed = true;
        draft.attachments.push(DraftAttachment {
            path: PathBuf::from("/missing/kept.pdf"),
            name: "kept.pdf".into(),
            preview: None,
        });
        composer
            .draft_mut(DraftScope {
                connection: 4,
                profile: "other".into(),
                session: None,
            })
            .text
            .replace_text("other private draft");
        composer
            .favorites
            .insert((3, "provider".into(), "model".into()));
        let bytes = composer.storage_bytes(3).unwrap();
        let state: State = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(state.drafts.len(), 1);
        assert_eq!(state.drafts[0].profile, "work");
        assert_eq!(state.drafts[0].text, "Do the next step");
        assert!(state.drafts[0].reasoning_changed);
        assert_eq!(
            state.drafts[0].attachments,
            [PathBuf::from("/missing/kept.pdf")]
        );
        assert_eq!(state.favorites, [("provider".into(), "model".into())]);
        assert!(
            !String::from_utf8(bytes)
                .unwrap()
                .contains("other private draft")
        );
    }
}

#[cfg(test)]
mod write_retry_tests {
    use super::*;
    #[test]
    fn a_failed_private_write_keeps_the_latest_draft_for_retry_without_another_edit() {
        let root =
            std::env::temp_dir().join(format!("hermes-draft-retry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let blocked = root.join("blocked");
        std::fs::write(&blocked, b"file").unwrap();
        let path = blocked.join("drafts.json");
        let mut composer = Composer::new();
        let owner = DraftScope {
            connection: 7,
            profile: "work".into(),
            session: Some("stored".into()),
        };
        composer
            .draft_mut(owner)
            .text
            .replace_text("Keep this after the write failure");
        composer.persisted = Some(Persistence {
            epoch: 7,
            path: path.clone(),
            previous: vec![],
            pending: None,
            scanned_revision: None,
        });
        assert!(composer.poll_storage(0.).unwrap().is_some());
        assert!(composer.poll_storage(0.4).is_err());
        assert!(composer.persisted.as_ref().unwrap().pending.is_some());
        std::fs::remove_file(&blocked).unwrap();
        composer.poll_storage(1.5).unwrap();
        let saved: State = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.version, 2);
        assert_eq!(saved.drafts[0].text, "Keep this after the write failure");
        drop(composer);
        std::fs::remove_dir_all(root).unwrap();
    }
}
