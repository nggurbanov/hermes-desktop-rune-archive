//! Canonical slash mutations retain their exact request and fences across lost replies.
use super::*;
use crate::gateway::conversation::{CallError, SessionTransport};
use serde::{Deserialize, Serialize};
use serde_json::json;
#[derive(Clone, Serialize, Deserialize)]
pub struct SlashIntent {
    pub id: String,
    pub principal: String,
    pub profile: String,
    pub stored: Option<String>,
    pub input: String,
    pub runtime: Option<String>,
    pub revision: Option<u64>,
    pub generation: Option<u64>,
    pub method: Option<String>,
    pub params: Value,
    pub attempted: bool,
    #[serde(default)]
    pub policy: crate::conversation::SendInput,
}
impl SlashIntent {
    fn path(&self) -> Result<std::path::PathBuf, String> {
        uuid::Uuid::parse_str(&self.id).map_err(|_| "Invalid retained command identity")?;
        Ok(crate::client::state_dir()?
            .join("slash-actions")
            .join(format!("{}.json", self.id)))
    }
    fn save(&self) -> Result<(), String> {
        crate::client::private_write(
            &self.path()?,
            &serde_json::to_vec(self).map_err(|_| "The command could not be retained")?,
        )
    }
    pub fn settle(&self, value: &Value) -> Result<(), String> {
        crate::client::private_write(
            &self.path()?.with_extension("receipt"),
            &serde_json::to_vec(value).map_err(|_| "The command receipt could not be encoded")?,
        )
    }
    pub fn retryable(&self) -> bool {
        self.method.as_deref() == Some("session.mutate")
    }
    fn verify(&self, connection: &Connection, owner: &DraftScope) -> Result<(), String> {
        if self.principal != crate::client::connection_key(connection)?
            || self.profile != owner.profile
            || self.stored != owner.session
        {
            return Err("This command belongs to another source or conversation".into());
        }
        Ok(())
    }
}
impl Chat {
    pub fn dispatch(
        &mut self,
        connection: Connection,
        owner: DraftScope,
        input: String,
        fence: Option<(String, u64, u64)>,
        policy: crate::conversation::SendInput,
    ) -> Result<(), String> {
        let view = self.views.entry(owner.clone()).or_default();
        if view.slash_running || view.slash_intent.is_some() {
            return Err("Resolve the retained command before sending another one".into());
        }
        let (runtime, revision, generation) = fence
            .map(|(r, v, g)| (Some(r), Some(v), Some(g)))
            .unwrap_or_default();
        let intent = SlashIntent {
            id: uuid::Uuid::new_v4().to_string(),
            principal: crate::client::connection_key(&connection)?,
            profile: owner.profile.clone(),
            stored: owner.session.clone(),
            input,
            runtime,
            revision,
            generation,
            method: None,
            params: Value::Null,
            attempted: false,
            policy,
        };
        self.start_slash(connection, owner, intent)
    }
    pub fn start_slash(
        &mut self,
        connection: Connection,
        owner: DraftScope,
        intent: SlashIntent,
    ) -> Result<(), String> {
        intent.verify(&connection, &owner)?;
        let waker = self
            .waker
            .clone()
            .ok_or("The command worker is not initialized")?;
        let view = self.views.entry(owner.clone()).or_default();
        if view.slash_running {
            return Err("Wait for the command to finish".into());
        }
        if intent.attempted && !intent.retryable() {
            return Err("The legacy command has an unknown outcome. Read its effects and acknowledge it before issuing another command".into());
        }
        intent.save()?;
        view.slash_running = true;
        view.error = None;
        view.slash_intent = Some(intent.clone());
        let catalog = view.slash_catalog.clone();
        let send = self.send.clone();
        std::thread::spawn(move || {
            let mut intent = intent;
            let result = (|| {
                let mut transport = SessionTransport::connect(&connection, &owner.profile)?;
                execute(&mut transport, &mut intent, &catalog, &|intent| {
                    intent.save()
                })
            })();
            let result = match result {
                Ok(value) => {
                    if value["confirm_required"] == true
                        || value["status"] == "confirmation_required"
                    {
                        Ok(value)
                    } else {
                        intent
                            .settle(&value)
                            .map_err(|message| CallError {
                                message,
                                code: None,
                                data: None,
                                uncertain: true,
                            })
                            .map(|_| value)
                    }
                }
                Err(error) if !error.uncertain => match intent.settle(&json!({"refused":true})) {
                    Ok(()) => Err(error),
                    Err(message) => Err(CallError {
                        message,
                        code: None,
                        data: None,
                        uncertain: true,
                    }),
                },
                other => other,
            };
            let input = intent.input.clone();
            if send
                .send(Reply::Slash {
                    owner,
                    input,
                    intent,
                    result,
                })
                .is_ok()
            {
                waker();
            }
        });
        Ok(())
    }
    pub fn recover_slash(&mut self, connection: &Connection, owner: &DraftScope) {
        let view = self.views.entry(owner.clone()).or_default();
        if view.slash_recovered {
            return;
        }
        view.slash_recovered = true;
        let Ok(root) = crate::client::state_dir().map(|r| r.join("slash-actions")) else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.take(1000).flatten() {
            let path = entry.path();
            if path.extension() != Some(std::ffi::OsStr::new("json"))
                || path.with_extension("receipt").exists()
            {
                continue;
            }
            let Some(intent) = std::fs::read(path)
                .ok()
                .and_then(|b| serde_json::from_slice::<SlashIntent>(&b).ok())
            else {
                continue;
            };
            if intent.verify(connection, owner).is_ok() {
                view.slash_intent = Some(intent);
                view.error=Some("An unfinished slash command is retained. Resolve its outcome before changing this profile.".into());
                break;
            }
        }
    }
}
fn missing(error: &CallError) -> bool {
    error.code == Some(-32601) && !error.uncertain
}
fn unknown(message: &str) -> CallError {
    CallError {
        message: message.into(),
        code: None,
        data: None,
        uncertain: true,
    }
}
pub(super) fn desktop_refusal(catalog: &Value, name: &str, arg: &str) -> Option<String> {
    let meta = &catalog["commands"][name];
    let fallback: Value = serde_json::from_str(include_str!(
        "../../../desktop/src/lib/desktop-slash-registry.json"
    ))
    .unwrap_or_default();
    let disposition = meta["desktop"]
        .as_str()
        .or_else(|| fallback[format!("/{name}")].as_str());
    if catalog["skills"].get(name).is_some() {
        return None;
    }
    if let Some(kind) =
        disposition.filter(|s| matches!(*s, "terminal" | "messaging" | "advanced" | "hidden"))
    {
        return Some(format!(
            "/{name} belongs to the {kind} interface and cannot execute from this composer"
        ));
    }
    if let Some(allowed) = meta["desktop_subcommands"].as_array() {
        if !arg.is_empty()
            && !allowed
                .iter()
                .any(|v| v.as_str() == arg.split_whitespace().next())
        {
            return Some(format!(
                "This /{name} subcommand is managed outside the desktop composer"
            ));
        }
    }
    None
}
fn directive(name: &str, arg: &str) -> Option<(String, Value)> {
    match name {
        "model" if !arg.is_empty() && !arg.starts_with('-') => {
            let words: Vec<_> = arg.split_whitespace().collect();
            if words.len() == 1 {
                Some(("model".into(), json!({"model":arg})))
            } else if words.len() >= 3
                && words[1] == "--provider"
                && words.len() <= 4
                && words.get(3).is_none_or(|v| *v == "--session")
            {
                Some((
                    "model".into(),
                    json!({"model":words[0],"provider":words[2]}),
                ))
            } else {
                None
            }
        }
        "compress" | "compact" => Some((
            "compress".into(),
            if arg.is_empty() {
                json!({})
            } else {
                json!({"focus":arg})
            },
        )),
        "branch" | "fork" => Some((
            "branch".into(),
            if arg.is_empty() {
                json!({})
            } else {
                json!({"title":arg})
            },
        )),
        "title" if !arg.is_empty() => Some(("rename".into(), json!({"title":arg}))),
        "reasoning" => {
            let words: Vec<_> = arg.split_whitespace().collect();
            let global = words
                .iter()
                .any(|v| matches!(*v, "--global" | "-g" | "global"));
            let effort = words
                .iter()
                .filter(|v| !matches!(**v, "--session" | "-s" | "session"))
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            if !global
                && matches!(
                    effort.as_str(),
                    "none"
                        | "false"
                        | "disabled"
                        | "minimal"
                        | "low"
                        | "medium"
                        | "high"
                        | "xhigh"
                        | "max"
                        | "ultra"
                )
            {
                Some(("reasoning".into(), json!({"effort":effort})))
            } else {
                None
            }
        }
        _ => None,
    }
}
fn execute(
    transport: &mut SessionTransport,
    intent: &mut SlashIntent,
    catalog: &Value,
    persist: &impl Fn(&SlashIntent) -> Result<(), String>,
) -> Result<Value, CallError> {
    if intent.attempted {
        if !intent.retryable() {
            return Err(unknown(
                "This command's outcome is unknown; automatic replay is not supported by its gateway",
            ));
        }
        return mutate(transport, intent, persist);
    }
    let mut catalog = catalog.clone();
    if catalog.is_null() {
        catalog = transport
            .call("commands.catalog", json!({"session_id":intent.runtime}))
            .unwrap_or(Value::Null);
    }
    let mut command = intent.input.clone();
    let mut seen = HashSet::new();
    for _ in 0..8 {
        let (name, arg) = parse_slash(&command).ok_or("Invalid slash command")?;
        if !seen.insert(name.clone()) {
            return Err("A slash alias cycle was refused before executing again".into());
        }
        if let Some(refusal) = desktop_refusal(&catalog, &name, &arg) {
            return Err(refusal.into());
        }
        if let Some(canonical) = catalog["canon"]
            .get(&name)
            .or_else(|| catalog["canon"].get(format!("/{name}")))
            .and_then(Value::as_str)
            .filter(|v| v.trim_start_matches('/') != name)
        {
            command = format!(
                "/{}{}{}",
                canonical.trim_start_matches('/'),
                if arg.is_empty() { "" } else { " " },
                arg
            );
            continue;
        }
        if transport.canonical {
            if let Some((operation, payload)) = directive(&name, &arg) {
                let stored = intent
                    .stored
                    .as_deref()
                    .ok_or("Choose an existing conversation for this session command")?;
                let snapshot = transport.resume(stored)?;
                if intent
                    .runtime
                    .as_ref()
                    .is_some_and(|id| id != &snapshot.runtime)
                {
                    return Err(
                        "This command's runtime owner changed before it was submitted".into(),
                    );
                }
                intent.runtime = Some(snapshot.runtime.clone());
                intent.method = Some("session.mutate".into());
                intent.params = json!({"session_id":snapshot.runtime,"request_id":intent.id,"expected_revision":intent.revision.unwrap_or(snapshot.revision),"expected_generation":intent.generation.unwrap_or(snapshot.generation),"operation":operation,"payload":payload});
                persist(intent)?;
                return mutate(transport, intent, persist);
            }
        }
        if name == "reasoning" && !arg.is_empty() {
            let mut scope = None;
            let mut words = vec![];
            for word in arg.split_whitespace() {
                match word.to_lowercase().as_str() {
                    "--global" | "-g" | "global" => scope = Some("global"),
                    "--session" | "-s" | "session" => scope = Some("session"),
                    _ => words.push(word),
                }
            }
            let mut params =
                json!({"key":"reasoning","value":words.join(" "),"session_id":intent.runtime});
            if let Some(scope) = scope {
                params["scope"] = json!(scope);
            }
            return unretained(transport, intent, persist, "config.set", params);
        }
        if intent.runtime.is_none() {
            return unretained(
                transport,
                intent,
                persist,
                "command.dispatch",
                json!({"name":name,"arg":arg}),
            );
        }
        let params = json!({"session_id":intent.runtime,"command":command});
        intent.method = Some("slash.exec".into());
        intent.params = params.clone();
        intent.attempted = true;
        persist(intent)?;
        let value = match transport.call("slash.exec", params) {
            Ok(value) => value,
            Err(error) if missing(&error) => {
                intent.method = Some("command.dispatch".into());
                intent.params = json!({"name":name,"arg":arg,"session_id":intent.runtime});
                persist(intent)?;
                transport.call("command.dispatch", intent.params.clone())?
            }
            Err(error) => return Err(error),
        };
        if value["type"] == "alias" {
            let target = value["target"]
                .as_str()
                .filter(|v| !v.trim().is_empty())
                .ok_or("The alias has no target")?;
            command = format!(
                "/{}{}{}",
                target.trim_start_matches('/'),
                if arg.is_empty() { "" } else { " " },
                arg
            );
            intent.attempted = false;
            intent.method = None;
            intent.params = Value::Null;
            persist(intent)?;
            continue;
        }
        return Ok(value);
    }
    Err("The slash alias chain exceeds the supported depth".into())
}
fn unretained(
    transport: &mut SessionTransport,
    intent: &mut SlashIntent,
    persist: &impl Fn(&SlashIntent) -> Result<(), String>,
    method: &str,
    params: Value,
) -> Result<Value, CallError> {
    intent.method = Some(method.into());
    intent.params = params.clone();
    intent.attempted = true;
    persist(intent)?;
    transport.call(method, params)
}
fn mutate(
    transport: &mut SessionTransport,
    intent: &mut SlashIntent,
    persist: &impl Fn(&SlashIntent) -> Result<(), String>,
) -> Result<Value, CallError> {
    intent.attempted = true;
    persist(intent)?;
    let timeout = if intent.params["operation"] == "compress" {
        std::time::Duration::from_secs(660)
    } else {
        std::time::Duration::from_secs(20)
    };
    let value = transport.call_with_timeout("session.mutate", intent.params.clone(), timeout)?;
    if value["session_id"] != intent.params["session_id"] {
        return Err(unknown("The command receipt belongs to another session"));
    }
    if value["confirm_required"] == true || value["status"] == "confirmation_required" {
        return Ok(value);
    }
    if !value["revision"].is_u64() {
        return Err(unknown(
            "The command mutation receipt has no matching session and revision",
        ));
    }
    let operation = intent.params["operation"].as_str().unwrap_or("command");
    Ok(
        json!({"type":"exec","output":format!("{operation}: completed"),"mutation":value,"operation":operation}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::{TcpListener, TcpStream},
        thread,
    };
    use tungstenite::{Message, WebSocket};
    fn request(ws: &mut WebSocket<TcpStream>, method: &str) -> Value {
        let value: Value = serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(value["method"], method);
        assert_eq!(value["params"]["profile"], "work");
        value
    }
    fn answer(ws: &mut WebSocket<TcpStream>, frame: &Value, value: Value) {
        ws.send(Message::Text(
            json!({"jsonrpc":"2.0","id":frame["id"],"result":value})
                .to_string()
                .into(),
        ))
        .unwrap();
    }
    fn intent(input: &str) -> SlashIntent {
        SlashIntent {
            id: uuid::Uuid::new_v4().to_string(),
            principal: "fixture".into(),
            profile: "work".into(),
            stored: Some("stored".into()),
            runtime: Some("live".into()),
            revision: Some(2),
            generation: Some(0),
            input: input.into(),
            method: None,
            params: Value::Null,
            attempted: false,
            policy: Default::default(),
        }
    }
    fn snapshot() -> Value {
        json!({"session_id":"live","stored_session_id":"stored","revision":2,"execution_generation":0,"info":{"profile_name":"work","desktop_protocol":"hermes-gateway-v1"}})
    }
    #[test]
    fn canonical_policy_lost_ack_retries_the_exact_mutation_and_original_fences() {
        let root =
            std::env::temp_dir().join(format!("hermes-slash-contract-{}", uuid::Uuid::new_v4()));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let connection = Connection::Url {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            token: None,
        };
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let resume = request(&mut ws, "session.resume");
            answer(&mut ws, &resume, snapshot());
            let first = request(&mut ws, "session.mutate");
            assert_eq!(first["params"]["operation"], "reasoning");
            assert_eq!(first["params"]["payload"], json!({"effort":"high"}));
            drop(ws);
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let second = request(&mut ws, "session.mutate");
            assert_eq!(first["params"], second["params"]);
            answer(
                &mut ws,
                &second,
                json!({"session_id":"live","operation":"reasoning","revision":3}),
            );
        });
        let path = root.join("retained.json");
        let persist = |intent: &SlashIntent| {
            crate::client::private_write(&path, &serde_json::to_vec(intent).unwrap())
        };
        let mut pending = intent("/reasoning high");
        let mut transport = SessionTransport::connect(&connection, "work").unwrap();
        transport.canonical = true;
        assert!(
            execute(&mut transport, &mut pending, &json!({}), &persist)
                .unwrap_err()
                .uncertain
        );
        let mut recovered: SlashIntent =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut transport = SessionTransport::connect(&connection, "work").unwrap();
        transport.canonical = true;
        let result = execute(&mut transport, &mut recovered, &json!({}), &persist).unwrap();
        assert_eq!(result["operation"], "reasoning");
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn aliases_preserve_multiline_arguments_and_cycles_do_not_execute_again() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let connection = Connection::Url {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            token: None,
        };
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let first = request(&mut ws, "slash.exec");
            answer(&mut ws, &first, json!({"type":"alias","target":"next"}));
            let second = request(&mut ws, "slash.exec");
            assert_eq!(second["params"]["command"], "/next first\n\n  second");
            answer(&mut ws, &second, json!({"type":"alias","target":"alias"}));
            ws.get_mut()
                .set_read_timeout(Some(std::time::Duration::from_millis(200)))
                .unwrap();
            assert!(ws.read().is_err());
        });
        let mut transport = SessionTransport::connect(&connection, "work").unwrap();
        let mut command = intent("/alias first\n\n  second");
        let error = execute(&mut transport, &mut command, &json!({}), &|_| Ok(())).unwrap_err();
        assert!(error.message.contains("cycle"));
        server.join().unwrap();
    }
    #[test]
    fn model_confirmation_receipt_must_match_the_captured_session_before_showing_it() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let connection = Connection::Url {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            token: None,
        };
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let frame = request(&mut ws, "session.mutate");
            answer(
                &mut ws,
                &frame,
                json!({"session_id":"other","status":"confirmation_required","confirm":"wrong-owner-token"}),
            );
        });
        let mut transport = SessionTransport::connect(&connection, "work").unwrap();
        let mut command = intent("/model guarded");
        command.method = Some("session.mutate".into());
        command.params = json!({"session_id":"live","operation":"model","request_id":command.id,"expected_revision":2,"expected_generation":0,"payload":{"model":"guarded"}});
        let error = mutate(&mut transport, &mut command, &|_| Ok(())).unwrap_err();
        assert!(error.uncertain);
        assert!(error.message.contains("another session"));
        server.join().unwrap();
    }
}
