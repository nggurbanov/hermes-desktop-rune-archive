use super::*;
use crate::gateway::conversation::SessionTransport;
use serde::{Deserialize, Serialize};
use serde_json::json;
#[derive(Clone, Debug)]
pub struct SlashRow {
    pub name: String,
    pub description: String,
    pub skill: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct BranchIntent {
    pub connection: String,
    pub profile: String,
    pub stored: String,
    pub runtime: String,
    pub request_id: String,
    pub revision: u64,
    pub generation: u64,
    pub through: u64,
}
impl BranchIntent {
    pub fn path(&self) -> Result<std::path::PathBuf, String> {
        uuid::Uuid::parse_str(&self.request_id)
            .map_err(|_| "The retained branch identity is invalid")?;
        Ok(crate::client::state_dir()?
            .join("chat-actions")
            .join(format!("{}.json", self.request_id)))
    }
    pub fn save(&self) -> Result<(), String> {
        crate::client::private_write(
            &self.path()?,
            &serde_json::to_vec(self).map_err(|_| "The branch action could not be encoded")?,
        )
    }
    pub fn params(&self) -> Value {
        json!({"session_id":self.runtime,"request_id":self.request_id,"expected_revision":self.revision,"expected_generation":self.generation,"operation":"branch","payload":{"through_message_id":self.through}})
    }
}
impl Chat {
    pub fn recover_branches(&mut self, connection: &Connection, owner: &DraftScope) {
        let view = self.views.entry(owner.clone()).or_default();
        if view.recovered {
            return;
        }
        view.recovered = true;
        let Ok(key) = crate::client::connection_key(connection) else {
            return;
        };
        let Ok(root) = crate::client::state_dir().map(|p| p.join("chat-actions")) else {
            return;
        };
        if let Some((intent, receipt)) = recover(&root, &key, owner) {
            view.branch = Some(intent);
            view.branch_result = receipt;
            if view.branch_result.is_none() {
                view.error=Some("An unfinished branch action is saved. Retry its retained identity to resolve its outcome before changing this profile.".into());
            }
        }
    }

    pub fn catalog(&mut self, connection: Connection, owner: DraftScope) {
        let view = self.views.entry(owner.clone()).or_default();
        if view.slash_loaded || view.slash_loading {
            return;
        }
        let Some(waker) = self.waker.clone() else {
            return;
        };
        view.slash_loading = true;
        let send = self.send.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut transport = SessionTransport::connect(&connection, &owner.profile)?;
                transport
                    .call("commands.catalog", json!({"session_id":owner.session}))
                    .map_err(|e| e.message)
            })();
            if send.send(Reply::Catalog { owner, result }).is_ok() {
                waker();
            }
        });
    }
    pub fn dispatch(
        &mut self,
        connection: Connection,
        owner: DraftScope,
        input: String,
        runtime: Option<String>,
    ) -> Result<(), String> {
        let view = self.views.entry(owner.clone()).or_default();
        if view.slash_running {
            return Err("Wait for this slash command to finish".into());
        }
        let waker = self
            .waker
            .clone()
            .ok_or("The command worker is not initialized")?;
        view.slash_running = true;
        let send = self.send.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut transport = SessionTransport::connect(&connection, &owner.profile)?;
                let (name, arg) = parse_slash(&input).ok_or("Invalid slash command")?;
                let target = runtime.or(owner.session.clone());
                let params = json!({"session_id":target,"command":input});
                match transport.call("slash.exec", params) {
                    Ok(v) => Ok(v),
                    Err(e) if e.code == Some(-32601) => transport
                        .call(
                            "command.dispatch",
                            json!({"name":name,"arg":arg,"session_id":target}),
                        )
                        .map_err(|e| e.message),
                    Err(e) => Err(e.message),
                }
            })();
            if send
                .send(Reply::Slash {
                    owner,
                    input,
                    result,
                })
                .is_ok()
            {
                waker();
            }
        });
        Ok(())
    }
    pub fn branch(
        &mut self,
        connection: Connection,
        owner: DraftScope,
        intent: BranchIntent,
    ) -> Result<(), String> {
        if crate::client::connection_key(&connection)? != intent.connection
            || owner.profile != intent.profile
            || owner.session.as_deref() != Some(intent.stored.as_str())
        {
            return Err("This branch belongs to another agent or conversation".into());
        }
        let waker = self
            .waker
            .clone()
            .ok_or("The branch worker is not initialized")?;
        let view = self.views.entry(owner.clone()).or_default();
        if view.branch_busy {
            return Err("A branch is already being prepared".into());
        }
        intent.save()?;
        view.branch = Some(intent.clone());
        view.branch_busy = true;
        view.branch_result = None;
        view.notice = None;
        view.error = None;
        let send = self.send.clone();
        std::thread::spawn(move || {
            let mut result: Result<(String, String), crate::gateway::conversation::CallError> =
                (|| {
                    let mut transport = SessionTransport::connect(&connection, &owner.profile)?;
                    let snapshot = transport.resume(&intent.stored)?;
                    if !snapshot.canonical {
                        return Err(
                            "This agent does not provide the canonical durable branch API".into(),
                        );
                    }
                    if snapshot.runtime != intent.runtime {
                        return Err(
                            "The branch runtime owner changed; its retained identity was not sent"
                                .into(),
                        );
                    }
                    let value = transport.call("session.mutate", intent.params())?;
                    if value["session_id"].as_str() != Some(intent.runtime.as_str())
                        || value["operation"] != "branch"
                        || !value["revision"].is_u64()
                    {
                        return Err(crate::gateway::conversation::CallError{message:"The branch receipt has a different owner; retry the retained action to resolve its outcome".into(),code:None,data:None,uncertain:true});
                    }
                    let child = value["branched_session_id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or_else(||crate::gateway::conversation::CallError{message:"The branch receipt has no child session; retry its retained identity to resolve the outcome".into(),code:None,data:None,uncertain:true})?
                        .to_owned();
                    let title = value["title"]
                        .as_str()
                        .unwrap_or("Branched conversation")
                        .to_owned();
                    // Persist its receipt before retiring the payload. A repeated explicit
                    // retry returns the same child through request_id, never a new branch.
                    crate::client::private_write(
                        &intent.path()?.with_extension("receipt"),
                        &serde_json::to_vec(&json!({"child":child,"title":title}))
                            .map_err(|_| "Branch receipt could not be encoded")?,
                    )
                    .map_err(|message| {
                        crate::gateway::conversation::CallError {
                            message,
                            code: None,
                            data: None,
                            uncertain: true,
                        }
                    })?;
                    Ok((child, title))
                })();
            if result.as_ref().err().is_some_and(|e| !e.uncertain) {
                // A definite pre-commit refusal is a settled outcome, not a
                // replayable request. Its tombstone prevents recovery on restart.
                if let Err(error) = intent.path().and_then(|path| {
                    crate::client::private_write(
                        &path.with_extension("receipt"),
                        b"{\"refused\":true}",
                    )
                }) {
                    result = Err(crate::gateway::conversation::CallError {
                        message: error,
                        code: None,
                        data: None,
                        uncertain: true,
                    });
                }
            }
            if send.send(Reply::Branch { owner, result }).is_ok() {
                waker();
            }
        });
        Ok(())
    }
}
fn recover(
    root: &std::path::Path,
    key: &str,
    owner: &DraftScope,
) -> Option<(BranchIntent, Option<(String, String)>)> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut pending = vec![];
    let mut resolved = vec![];
    for entry in entries.take(1000).flatten() {
        let path = entry.path();
        if path.extension() != Some(std::ffi::OsStr::new("json")) {
            continue;
        }
        let Some(intent) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<BranchIntent>(&bytes).ok())
        else {
            continue;
        };
        if uuid::Uuid::parse_str(&intent.request_id).is_err()
            || intent.through == 0
            || intent.connection != key
            || intent.profile != owner.profile
            || owner.session.as_deref() != Some(intent.stored.as_str())
        {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let receipt_path = path.with_extension("receipt");
        if receipt_path.exists() {
            if let Some(receipt) = std::fs::read(receipt_path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            {
                if let (Some(child), Some(title)) =
                    (receipt["child"].as_str(), receipt["title"].as_str())
                {
                    resolved.push((modified, intent, Some((child.into(), title.into()))));
                }
            }
        } else {
            pending.push((modified, intent, None));
        }
    }
    pending.sort_by_key(|(time, _, _)| *time);
    resolved.sort_by_key(|(time, _, _)| *time);
    pending
        .pop()
        .or_else(|| resolved.pop())
        .map(|(_, intent, receipt)| (intent, receipt))
}
pub fn parse_slash(input: &str) -> Option<(String, String)> {
    let tail = input.strip_prefix('/')?;
    let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
    let name = &tail[..end];
    if name.contains('/') {
        return None;
    }
    Some((name.to_lowercase(), tail[end..].trim().to_owned()))
}
pub fn catalog_rows(value: &Value) -> Vec<SlashRow> {
    let mut rows = vec![];
    let pairs = value["pairs"].as_array().cloned().unwrap_or_default();
    for pair in pairs {
        let Some(name) = pair[0].as_str() else {
            continue;
        };
        let name = name.trim_start_matches('/');
        let disposition = value["commands"][name]["desktop"].as_str().unwrap_or("");
        let skill = value["skills"].get(name).is_some();
        if !skill
            && matches!(
                disposition,
                "hidden" | "terminal" | "terminal-only" | "messaging" | "messaging-only"
            )
        {
            continue;
        }
        rows.push(SlashRow {
            name: name.into(),
            description: pair[1].as_str().unwrap_or("").into(),
            skill,
        });
    }
    rows
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_preserves_extensions_and_multiline_invocation_but_hides_terminal_builtins() {
        let rows = catalog_rows(
            &json!({"pairs":[["local-skill","My skill"],["shell","Terminal only"],["status","Status"]],"commands":{"shell":{"desktop":"terminal"}},"skills":{"local-skill":{}}}),
        );
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["local-skill", "status"]
        );
        assert_eq!(
            parse_slash("/LoCaL-Skill first\n\n  second"),
            Some(("local-skill".into(), "first\n\n  second".into()))
        );
        assert!(parse_slash("/usr/local").is_none());
    }
    #[test]
    fn startup_recovers_only_owned_intents_and_reads_acknowledged_child_receipts() {
        let root =
            std::env::temp_dir().join(format!("hermes-branch-recovery-{}", uuid::Uuid::new_v4()));
        let owner = DraftScope {
            connection: 9,
            profile: "work".into(),
            session: Some("stored".into()),
        };
        let intent = BranchIntent {
            connection: "credential-hash".into(),
            profile: "work".into(),
            stored: "stored".into(),
            runtime: "live".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            revision: 2,
            generation: 3,
            through: 41,
        };
        let path = root.join(format!("{}.json", intent.request_id));
        crate::client::private_write(&path, &serde_json::to_vec(&intent).unwrap()).unwrap();
        let (saved, receipt) = recover(&root, "credential-hash", &owner).unwrap();
        assert_eq!(saved.params(), intent.params());
        assert!(receipt.is_none());
        assert!(recover(&root, "other-backend", &owner).is_none());
        assert!(
            recover(
                &root,
                "credential-hash",
                &DraftScope {
                    profile: "other".into(),
                    ..owner.clone()
                }
            )
            .is_none()
        );
        crate::client::private_write(
            &path.with_extension("receipt"),
            b"{\"child\":\"child-id\",\"title\":\"Branch title\"}",
        )
        .unwrap();
        assert_eq!(
            recover(&root, "credential-hash", &owner).unwrap().1,
            Some(("child-id".into(), "Branch title".into()))
        );
        crate::client::private_write(&path.with_extension("receipt"), b"{\"refused\":true}")
            .unwrap();
        assert!(recover(&root, "credential-hash", &owner).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
