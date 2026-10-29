//! Atomic admission intents in private client state; no credentials or agent files.
use super::{AttachmentInput, SendInput};
use crate::{composer::DraftScope, gateway::Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Input {
    #[serde(default)]
    pub voice_turn: bool,
    #[serde(default)]
    pub voice_context: Option<String>,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub interrupted: bool,
    #[serde(default)]
    pub canonical_bot: bool,
    #[serde(default)]
    pub mode: super::AdmissionMode,
    pub text: String,
    #[serde(default)]
    pub display_text: Option<String>,
    #[serde(default)]
    pub rewind: Option<super::Rewrite>,
    #[serde(default)]
    pub replay: Option<super::ReplayBundle>,
    pub attachments: Vec<AttachmentInput>,
    pub model: Option<(String, String)>,
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
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Prepared {
    pub version: u32,
    pub connection: String,
    pub profile: String,
    pub session: Option<String>,
    pub submission_id: String,
    pub create_id: String,
    pub input: Input,
    pub runtime: Option<String>,
    pub stored: Option<String>,
    pub canonical: Option<bool>,
    pub text: Option<String>,
    pub images: Vec<Value>,
    pub attached: usize,
    pub mutations: Vec<Mutation>,
    pub policy_done: bool,
    pub legacy_attempted: bool,
    #[serde(default)]
    pub attempted: bool,
    #[serde(default)]
    pub replay_attached: usize,
    #[serde(default)]
    pub replay_refs: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Mutation {
    pub operation: String,
    pub params: Value,
    pub complete: bool,
    #[serde(default)]
    pub confirmation: Option<String>,
    #[serde(default)]
    pub refused: bool,
}

impl Prepared {
    pub fn new(
        connection: &Connection,
        scope: &DraftScope,
        input: SendInput,
    ) -> Result<Self, String> {
        Ok(Self {
            version: 1,
            connection: connection_key(connection)?,
            profile: scope.profile.clone(),
            session: scope.session.clone(),
            submission_id: uuid::Uuid::new_v4().to_string(),
            create_id: uuid::Uuid::new_v4().to_string(),
            input: Input {
                voice_turn: input.voice_turn,
                voice_context: input.voice_context,
                surface: input.surface,
                interrupted: input.interrupted,
                canonical_bot: input.canonical_bot,
                mode: input.mode,
                text: input.text,
                display_text: input.display_text,
                rewind: input.rewind,
                replay: input.replay,
                attachments: input.attachments,
                model: input.model.map(|m| (m.provider, m.id)),
                reasoning: input.reasoning,
                service_tier: input.service_tier,
                approval_mode: input.approval_mode,
                model_changed: input.model_changed,
                reasoning_changed: input.reasoning_changed,
                service_tier_changed: input.service_tier_changed,
                approval_changed: input.approval_changed,
                cwd: input.cwd,
                cwd_explicit: input.cwd_explicit,
            },
            runtime: None,
            stored: None,
            canonical: None,
            text: None,
            images: vec![],
            attached: 0,
            mutations: vec![],
            policy_done: false,
            legacy_attempted: false,
            attempted: false,
            replay_attached: 0,
            replay_refs: vec![],
        })
    }
    pub fn verify_owner(&self, connection: &Connection, scope: &DraftScope) -> Result<(), String> {
        let matches_session = self.session == scope.session || self.stored == scope.session;
        if self.version != 1
            || self.connection != connection_key(connection)?
            || self.profile != scope.profile
            || !matches_session
        {
            return Err(
                "The saved send belongs to another connection, profile or conversation".into(),
            );
        }
        Ok(())
    }
}
pub(crate) fn connection_key(connection: &Connection) -> Result<String, String> {
    crate::client::connection_key(connection)
}

#[derive(Clone)]
pub(crate) struct Journal {
    root: PathBuf,
}
impl Journal {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root)
            .map_err(|_| "The prepared-send directory could not be created")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Prepared sends could not be made private")?;
        }
        Ok(Self { root })
    }
    fn path(&self, id: &str, suffix: &str) -> Result<PathBuf, String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid saved submission identity")?;
        Ok(self.root.join(format!("{id}.{suffix}")))
    }
    pub fn save(&self, entry: &Prepared) -> Result<(), String> {
        let path = self.path(&entry.submission_id, "json")?;
        if self.path(&entry.submission_id, "settled")?.exists() {
            return Err("This send has already been settled".into());
        }
        let bytes = serde_json::to_vec(entry).map_err(|_| "Prepared send could not be encoded")?;
        crate::client::private_write(&path, &bytes)
    }
    pub fn load(&self, id: &str) -> Result<Prepared, String> {
        if self.path(id, "settled")?.exists() {
            return Err("This send has already been settled".into());
        }
        let bytes = fs::read(self.path(id, "json")?).map_err(|_| "Saved send could not be read")?;
        let entry: Prepared =
            serde_json::from_slice(&bytes).map_err(|_| "Saved send is invalid")?;
        if entry.submission_id != id {
            return Err("Saved send identity mismatch".into());
        }
        Ok(entry)
    }
    pub fn retire(&self, id: &str) -> Result<(), String> {
        // Commit the tombstone first. A failed deletion cannot resurrect a sent turn.
        crate::client::private_write(&self.path(id, "settled")?, b"settled\n")?;
        match fs::remove_file(self.path(id, "json")?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => {
                Err("The turn was accepted, but its saved payload could not be removed".into())
            }
        }
    }
    pub fn entries(&self) -> Result<Vec<Prepared>, String> {
        let mut entries = vec![];
        for entry in fs::read_dir(&self.root).map_err(|_| "Saved sends could not be listed")? {
            let path = entry
                .map_err(|_| "Saved send directory could not be read")?
                .path();
            if path.extension() != Some(std::ffi::OsStr::new("json")) {
                continue;
            }
            if let Some(id) = path.file_stem().and_then(|v| v.to_str()) {
                if let Ok(entry) = self.load(id) {
                    entries.push(entry);
                }
            }
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("hermes-rune-journal-{}", uuid::Uuid::new_v4()))
    }
    #[test]
    fn journal_preserves_identity_and_owner_without_credentials_and_tombstones_spent_sends() {
        let path = root();
        let journal = Journal::new(path.clone()).unwrap();
        let connection = Connection::Url {
            endpoint: "https://fixture.invalid/api/ws?token=private-token".into(),
            token: None,
        };
        let scope = DraftScope {
            connection: 4,
            profile: "bot".into(),
            session: None,
        };
        let entry = Prepared::new(
            &connection,
            &scope,
            SendInput {
                text: "saved message".into(),
                ..Default::default()
            },
        )
        .unwrap();
        journal.save(&entry).unwrap();
        let loaded = journal.load(&entry.submission_id).unwrap();
        assert_eq!(loaded.create_id, entry.create_id);
        let bytes = fs::read(journal.path(&entry.submission_id, "json").unwrap()).unwrap();
        assert!(!String::from_utf8(bytes).unwrap().contains("private-token"));
        assert!(
            loaded
                .verify_owner(
                    &connection,
                    &DraftScope {
                        profile: "other".into(),
                        ..scope.clone()
                    }
                )
                .is_err()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(journal.path(&entry.submission_id, "json").unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        journal.retire(&entry.submission_id).unwrap();
        assert!(journal.load(&entry.submission_id).is_err());
        assert!(journal.save(&entry).is_err());
        assert!(journal.entries().unwrap().is_empty());
        fs::remove_dir_all(path).unwrap();
    }
}
