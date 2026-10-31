//! Private installed-agent SSH fixture; no user home, keys or services.
use super::Spec;
include!("fixture_constants.rs");
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tungstenite::{
    Message,
    handshake::server::{Request, Response},
};
use uuid::Uuid;

/// A bounded one-shot gate for the authenticated `runtime.describe` reply.
#[derive(Clone, Default)]
pub(crate) struct ProofBarrier(Arc<(Mutex<ProofGate>, Condvar)>);
#[derive(Default)]
struct ProofGate {
    armed: bool,
    entered: bool,
    released: bool,
}
impl ProofBarrier {
    pub(crate) fn arm(&self) {
        let (lock, _) = &*self.0;
        *lock.lock().unwrap() = ProofGate {
            armed: true,
            entered: false,
            released: false,
        };
    }
    pub(crate) fn wait_entered(&self, timeout: Duration) -> bool {
        let (lock, changed) = &*self.0;
        let gate = lock.lock().unwrap();
        changed
            .wait_timeout_while(gate, timeout, |gate| !gate.entered)
            .unwrap()
            .0
            .entered
    }
    pub(crate) fn release(&self) {
        let (lock, changed) = &*self.0;
        lock.lock().unwrap().released = true;
        changed.notify_all();
    }
    fn block_reply(&self) -> bool {
        let (lock, changed) = &*self.0;
        let mut gate = lock.lock().unwrap();
        if !gate.armed {
            return true;
        }
        gate.entered = true;
        changed.notify_all();
        let (mut gate, _) = changed
            .wait_timeout_while(gate, Duration::from_secs(10), |gate| !gate.released)
            .unwrap();
        gate.armed = false;
        gate.released
    }
}

/// Identity captured from the fixture's live ownership record, not process argv.
pub(crate) struct Generation {
    pub pid: u32,
    pub nonce: String,
    start_ticks: u64,
    directory: PathBuf,
}
impl Generation {
    pub(crate) fn running(&self) -> Result<bool, String> {
        std::fs::metadata("/proc/self/stat")
            .map_err(|_| "Fixture process inspection unavailable")?;
        let stat = match std::fs::read_to_string(format!("/proc/{}/stat", self.pid)) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err("Could not inspect fixture process".into()),
        };
        let suffix = stat
            .rsplit_once(')')
            .ok_or("Invalid fixture process state")?
            .1;
        let fields: Vec<_> = suffix.split_whitespace().collect();
        let state = fields.first().ok_or("Invalid fixture process state")?;
        let ticks = fields
            .get(19)
            .ok_or("Invalid fixture process state")?
            .parse::<u64>()
            .map_err(|_| "Invalid fixture process identity")?;
        Ok(*state != "Z" && ticks == self.start_ticks)
    }
    pub(crate) fn files_retired(&self) -> Result<bool, String> {
        for path in [
            self.directory.join("backend.lock.json"),
            self.directory.join(format!("{}.token", self.nonce)),
            self.directory.join(format!("{}.log", self.nonce)),
        ] {
            match std::fs::symlink_metadata(path) {
                Ok(_) => return Ok(false),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("Could not inspect fixture ownership files".into()),
            }
        }
        Ok(true)
    }
}

#[derive(Default)]
struct SessionState {
    running: bool,
    tick: u64,
    sequence: u64,
    generation: u64,
    text: String,
    submission: String,
    terminal_event: Option<(u64, Value)>,
    owner_profile: String,
}
impl SessionState {
    fn snapshot(&self, profile: &str) -> Value {
        json!({"session_id":"live-shared-tip","stored_session_id":"shared-tip","revision":7,"authority_epoch":7,"execution_generation":self.generation,"replay_epoch":"fixture-fleet","last_sequence":self.sequence,"running":self.running,"pending":[],"prompts":[],"messages":[{"row_id":1,"role":"assistant","content":"Distinct private history"}],"info":{"profile_name":profile,"desktop_protocol":"hermes-gateway-v1","model":"fixture-model","provider":"fixture"}})
    }
}

pub(crate) struct Fixture {
    pub root: PathBuf,
    pub ssh_binary: PathBuf,
    pub spec: Spec,
    pub calls: Arc<Mutex<Vec<(u64, Value)>>>,
    pub port: u16,
    pub anonymous_port: u16,
    sessions: Arc<Mutex<SessionState>>,
    anonymous_sessions: Arc<Mutex<SessionState>>,
    pub proof_barrier: ProofBarrier,
    stop: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        let root = std::env::temp_dir().join(format!("rune-ssh-core-fixture-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".hermes")).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let anonymous = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        anonymous.set_nonblocking(true).unwrap();
        let anonymous_port = anonymous.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let proof_barrier = ProofBarrier::default();
        let service_barrier = proof_barrier.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sessions = Arc::new(Mutex::new(SessionState::default()));
        let service_sessions = sessions.clone();
        let anonymous_sessions = Arc::new(Mutex::new(SessionState::default()));
        let service_anonymous_sessions = anonymous_sessions.clone();
        let flag = stop.clone();
        let captured = calls.clone();
        let service_home = root.clone();
        let task = thread::spawn(move || {
            let serial = AtomicU64::new(0);
            let mut children = Vec::new();
            while !flag.load(Ordering::Acquire) {
                for (listener, is_anonymous) in [(&listener, false), (&anonymous, true)] {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let socket_id = serial.fetch_add(1, Ordering::Relaxed) + 1;
                            let flag = flag.clone();
                            let captured = captured.clone();
                            let home = service_home.clone();
                            let barrier = service_barrier.clone();
                            let sessions = if is_anonymous {
                                service_anonymous_sessions.clone()
                            } else {
                                service_sessions.clone()
                            };
                            children.push(thread::spawn(move || {
                                serve(
                                    stream,
                                    socket_id,
                                    &home,
                                    &flag,
                                    &captured,
                                    &barrier,
                                    &sessions,
                                    is_anonymous,
                                )
                            }));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("private fixture accept failed: {error}"),
                    }
                }
            }
            for child in children {
                child.join().unwrap();
            }
        });
        let launcher = root.join("hermes");
        let launcher_program = format!(
            r##"#!/usr/bin/python3
import os,sys,time,re
a=sys.argv[1:]
if a == ['--profile','default','config','path']:
 print(os.path.join(os.environ['HOME'],'.hermes','config.yaml')); sys.exit(0)
if a == ['serve','--help']:
 print('--ssh-session-token-file --ssh-owner-nonce'); sys.exit(0)
if a[:1] == ['--profile'] and len(a) > 2 and a[1] in ('research','dixie'):
 a=a[2:]
if len(a) != 10 or a[:5] != ['serve','--isolated','--host','127.0.0.1','--port'] or a[5] != '0' or a[6] != '--ssh-session-token-file' or a[8] != '--ssh-owner-nonce' or not re.fullmatch(r'[0-9a-f]{{16}}',a[9]):
 sys.exit(64)
expected=os.path.join(os.environ['HOME'],'.hermes','desktop-ssh')+os.sep
if not os.path.realpath(a[7]).startswith(expected) or os.path.basename(a[7]) != a[9]+'.token': sys.exit(64)
print('HERMES_BACKEND_READY port={port}',flush=True)
time.sleep(120)
"##
        );
        std::fs::write(&launcher, launcher_program).unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let bin = root.join("bin");
        std::fs::create_dir(&bin).unwrap();
        let ssh_binary = bin.join("ssh");
        let machine_home = root.join(".hermes");
        let marker = machine_home.join(".hermes-update-in-progress");
        let executable = super::remote_path(launcher.to_str().unwrap()).unwrap();
        let manifest = json!({
            "launcher": launcher, "machine_home": machine_home, "marker": marker, "port": port,
            "config_watchdog": super::remote::watchdog(&format!("env -u HERMES_HOME {executable} --profile default config path")),
            "help_watchdog": super::remote::watchdog(&format!("{executable} serve --help 2>&1")),
            "codes": {
                "gate": include_str!("update_gate.py"),
                "upload": FIXTURE_TOKEN_UPLOAD,
                "discover": FIXTURE_DISCOVER,
                "spawn": include_str!("spawn.py"),
                "cleanup": include_str!("cleanup.py"),
                "supervisor": include_str!("supervisor.py"),
                "reclaim": include_str!("reclaim.py"),
                "publish": include_str!("publish.py"),
                "inspect": include_str!("inspect.py"),
            }
        });
        std::fs::write(
            root.join("ssh-manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(
            root.join("ssh-manifest.json"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        std::fs::write(&ssh_binary, include_str!("fake_ssh.py")).unwrap();
        std::fs::set_permissions(&ssh_binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let spec = Spec {
            host: "private.fixture".into(),
            user: "private".into(),
            port: 2222,
            key_path: None,
            remote_hermes_path: Some(launcher.to_str().unwrap().into()),
            remote_profile: Some("default".into()),
        };
        Self {
            root,
            ssh_binary,
            spec,
            calls,
            port,
            anonymous_port,
            sessions,
            anonymous_sessions,
            proof_barrier,
            stop,
            task: Some(task),
        }
    }
    pub(crate) fn generation(&self, owner_id: &str) -> Result<Generation, String> {
        if owner_id.len() != 32
            || !owner_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Invalid fixture owner".into());
        }
        let directory = self.root.join(".hermes/desktop-ssh").join(owner_id);
        let record: Value = serde_json::from_slice(
            &std::fs::read(directory.join("backend.lock.json"))
                .map_err(|_| "Missing fixture record")?,
        )
        .map_err(|_| "Invalid fixture record")?;
        let pid = record
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or("Invalid fixture pid")?;
        let nonce = record
            .get("spawnNonce")
            .and_then(Value::as_str)
            .filter(|n| {
                n.len() == 16
                    && n.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .ok_or("Invalid fixture nonce")?
            .to_string();
        let start_ticks = record
            .get("startTicks")
            .and_then(Value::as_u64)
            .ok_or("Invalid fixture start ticks")?;
        if record.get("ownershipId").and_then(Value::as_str) != Some(owner_id) {
            return Err("Fixture owner mismatch".into());
        }
        Ok(Generation {
            pid,
            nonce,
            start_ticks,
            directory,
        })
    }
    pub(crate) fn calls(&self) -> Vec<(u64, Value)> {
        self.calls.lock().unwrap().clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.proof_barrier.release();
        if let Some(task) = self.task.take() {
            task.join().unwrap();
        }
        // Test-only fallback: any residual record belongs to this isolated root.
        if let Ok(owners) = std::fs::read_dir(self.root.join(".hermes/desktop-ssh")) {
            for entry in owners.flatten() {
                let record = entry.path().join("backend.lock.json");
                let Ok(bytes) = std::fs::read(record) else {
                    continue;
                };
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                let Some(nonce) = value.get("spawnNonce").and_then(Value::as_str) else {
                    continue;
                };
                let _ = std::process::Command::new("python3")
                    .arg("-c")
                    .arg(include_str!("cleanup.py"))
                    .arg(entry.path())
                    .arg(nonce)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn serve(
    mut stream: TcpStream,
    socket_id: u64,
    home: &Path,
    stop: &AtomicBool,
    calls: &Mutex<Vec<(u64, Value)>>,
    barrier: &ProofBarrier,
    sessions: &Mutex<SessionState>,
    anonymous: bool,
) {
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut first = [0; 4096];
    let n = stream.peek(&mut first).unwrap_or(0);
    if n == 0 {
        return;
    }
    if String::from_utf8_lossy(&first[..n]).starts_with("GET /api/ws")
        || String::from_utf8_lossy(&first[..n]).starts_with("GET /anonymous/api/ws")
    {
        let first_line = String::from_utf8_lossy(&first[..n]);
        let Some(target) = first_line
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|line| line.strip_suffix(" HTTP/1.1"))
        else {
            return;
        };
        let Ok(url) = url::Url::parse(&format!("http://fixture{target}")) else {
            return;
        };
        let tokens: Vec<_> = url
            .query_pairs()
            .filter(|(key, _)| key == "token")
            .map(|(_, value)| value.into_owned())
            .collect();
        if url.path() != "/api/ws"
            || (anonymous && !tokens.is_empty())
            || (!anonymous && tokens.as_slice() != ["fixture-served-token"])
        {
            return;
        }
        let Ok(mut ws) =
            tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                let url = url::Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
                if anonymous {
                    assert!(url.query_pairs().all(|(key, _)| key != "token"));
                } else {
                    assert_eq!(url.path(), "/api/ws");
                    assert_eq!(
                        url.query_pairs().find(|(key, _)| key == "token").unwrap().1,
                        "fixture-served-token"
                    );
                }
                Ok(response)
            })
        else {
            return;
        };
        if ws
            .send(Message::Text(
                json!({"jsonrpc":"2.0","method":"gateway.ready","params":{}})
                    .to_string()
                    .into(),
            ))
            .is_err()
        {
            return;
        }
        let mut subscribed = false;
        let mut delivered_terminal = 0u64;
        let mut subscriber_profile = String::new();
        while !stop.load(Ordering::Acquire) {
            let text = match ws.read() {
                Ok(Message::Text(text)) => text,
                Ok(Message::Close(_))
                | Err(tungstenite::Error::ConnectionClosed)
                | Err(tungstenite::Error::AlreadyClosed) => break,
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if subscribed {
                        let mut state = sessions.lock().unwrap();
                        if state.running && state.owner_profile == subscriber_profile {
                            state.tick += 1;
                            let text = format!("Legacy native tick {}\n", state.tick);
                            let offset = state.text.len();
                            state.text.push_str(&text);
                            state.sequence += 1;
                            let event = json!({"method":"event","params":{"session_id":"live-shared-tip","type":"message.delta","payload":{"text":text,"turn_id":state.submission,"segment":0,"offset":offset},"seq":state.sequence,"replay_epoch":"fixture-fleet","execution_generation":state.generation,"authority_epoch":7}});
                            if ws.send(Message::Text(event.to_string().into())).is_err() {
                                break;
                            }
                        } else if let Some((sequence, event)) = &state.terminal_event {
                            if *sequence > delivered_terminal
                                && state.owner_profile == subscriber_profile
                            {
                                if ws.send(Message::Text(event.to_string().into())).is_err() {
                                    break;
                                }
                                delivered_terminal = *sequence;
                            }
                        }
                    }
                    continue;
                }
                Err(_) => break,
                Ok(_) => continue,
            };
            let request: Value = serde_json::from_str(&text).unwrap();
            calls.lock().unwrap().push((socket_id, request.clone()));
            let result = match request["method"].as_str() {
                Some("profiles.list") => {
                    let root = if anonymous {
                        "/private/anonymous-home"
                    } else {
                        "/private/backend-home"
                    };
                    json!({"profiles":[{"name":"default","path":root},{"name":"dixie","path":"/private/dixie"},{"name":"archive","path":"/private/archive"},{"name":"research","path":"/private/research"}],"active":"default","install_id":"private-install"})
                }
                Some("runtime.describe") => {
                    assert_eq!(request["params"], json!({}));
                    if !barrier.block_reply() {
                        return;
                    }
                    if anonymous {
                        json!({"profile_id":"/private/anonymous-home","instance_id":"fixture-anonymous","authority_epoch":7})
                    } else {
                        json!({"profile_id":"/private/backend-home","instance_id":"fixture-instance","authority_epoch":7})
                    }
                }
                Some("session.create" | "session.resume") => {
                    subscribed = true;
                    subscriber_profile = request["params"]["profile"]
                        .as_str()
                        .unwrap_or("default")
                        .to_owned();
                    let state = sessions.lock().unwrap();
                    delivered_terminal = state
                        .terminal_event
                        .as_ref()
                        .map(|(sequence, _)| *sequence)
                        .unwrap_or(0);
                    state.snapshot(request["params"]["profile"].as_str().unwrap_or("dixie"))
                }
                Some("session.list") => {
                    let profile = request["params"]["profile"].as_str().unwrap_or("default");
                    let state = sessions.lock().unwrap();
                    let rows = if request["params"]["scope"] == "live" {
                        if state.running {
                            json!([{"id":"live-shared-tip","profile":profile,"status":"working","running":true}])
                        } else {
                            json!([])
                        }
                    } else {
                        json!([{"id":"shared-tip","profile":profile,"title":"private shared notes","source":"desktop","last_active":1793310000,"message_count":1,"_lineage_root_id":"shared-root","_lineage_ids":["shared-root","old-tip"],"pinned":false,"unread":false,"cwd":"/private-fixture"}])
                    };
                    json!({"profile":profile,"sessions":rows})
                }
                Some("model.options") => json!({"providers":[],"models":[]}),
                Some("config.get") => json!({"value":null}),
                Some("setup.runtime_check") => json!({"ok":true,"ready":true}),
                Some("profiles.get_asset") => json!({"data_url":null}),
                Some("process.list") => json!({"processes":[]}),
                Some("subagent.list") => json!({"subagents":[]}),
                Some("prompt.submit") => {
                    let mut state = sessions.lock().unwrap();
                    if request["params"]["text"] != "Legacy native synthetic stream"
                        || state.running
                    {
                        return;
                    }
                    let Some(id) = request["params"]["submission_id"].as_str() else {
                        return;
                    };
                    state.submission = id.to_string();
                    state.owner_profile = request["params"]["profile"]
                        .as_str()
                        .unwrap_or("default")
                        .to_owned();
                    subscriber_profile = state.owner_profile.clone();
                    state.terminal_event = None;
                    state.running = true;
                    state.generation += 1;
                    subscribed = true;
                    json!({"status":"streaming","admission_id":format!("admit-{id}"),"execution_generation":state.generation})
                }
                Some("session.interrupt" | "session.stop") => {
                    let mut state = sessions.lock().unwrap();
                    if request["params"]["profile"].as_str().unwrap_or("default")
                        != state.owner_profile
                        || request["params"]["session_id"].as_str() != Some("live-shared-tip")
                    {
                        return;
                    }
                    state.running = false;
                    json!({"interrupted":true})
                }
                _ => return,
            };
            if ws
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .is_err()
            {
                break;
            }
            let event_type = match request["method"].as_str() {
                Some("prompt.submit") => Some("message.start"),
                Some("session.interrupt" | "session.stop") => Some("message.complete"),
                _ => None,
            };
            if let Some(kind) = event_type {
                let mut state = sessions.lock().unwrap();
                state.sequence += 1;
                let payload = if kind == "message.start" {
                    json!({"input_id":state.submission,"admission_id":format!("admit-{}",state.submission)})
                } else {
                    json!({"status":"interrupted","text":state.text,"partial":true})
                };
                let event = json!({"method":"event","params":{"session_id":"live-shared-tip","type":kind,"payload":payload,"seq":state.sequence,"replay_epoch":"fixture-fleet","execution_generation":state.generation,"authority_epoch":7}});
                if kind == "message.complete" {
                    delivered_terminal = state.sequence;
                    state.terminal_event = Some((state.sequence, event.clone()));
                }
                if ws.send(Message::Text(event.to_string().into())).is_err() {
                    break;
                }
            }
        }
        return;
    }
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        request.push_str(&line);
    }
    let Some(target) = request
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("GET "))
        .and_then(|line| line.strip_suffix(" HTTP/1.1"))
    else {
        return;
    };
    let Ok(url) = url::Url::parse(&format!("http://fixture{target}")) else {
        return;
    };
    let authenticated = request
        .to_ascii_lowercase()
        .contains("x-hermes-session-token: fixture-served-token");
    let body = if url.path() == "/api/ssh/ownership" && authenticated && !anonymous {
        let Ok(entries) = std::fs::read_dir(home.join(".hermes/desktop-ssh")) else {
            return;
        };
        let Some(record) = entries
            .flatten()
            .map(|e| e.path().join("backend.lock.json"))
            .find(|p| p.exists())
        else {
            return;
        };
        let Ok(bytes) = std::fs::read(record) else {
            return;
        };
        let Ok(lock) = serde_json::from_slice::<Value>(&bytes) else {
            return;
        };
        json!({"ok":true,"sshOwnerNonce":lock["spawnNonce"],"protocolVersion":1}).to_string()
    } else if url.path() == "/" && !anonymous {
        r#"window.__HERMES_SESSION_TOKEN__ = "fixture-served-token""#.into()
    } else if (anonymous && !authenticated) || (!anonymous && authenticated) {
        let profile = url
            .query_pairs()
            .find(|(k, _)| k == "profile")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_else(|| "default".into());
        let rows = json!([{"id":"shared-tip","profile":profile,"title":"private shared notes","source":"desktop","last_active":1793310000,"message_count":1,"_lineage_root_id":"shared-root","_lineage_ids":["shared-root","old-tip"],"pinned":false,"unread":false,"cwd":"/private-fixture"}]);
        let value = match url.path() {
            "/api/status" => json!({"running":true,"ready":true,"install_id":"private-install"}),
            "/api/profiles" => {
                json!({"profiles":[{"name":"default","path":"/private/backend-home"},{"name":"dixie","path":"/private/dixie"},{"name":"archive","path":"/private/archive"}],"active":"default"})
            }
            "/api/profiles/sessions/sidebar" => {
                json!({"recents":{"sessions":rows},"messaging":{"sessions":[]},"cron":{"sessions":[]}})
            }
            "/api/profiles/sessions" => json!({"sessions":rows}),
            "/api/cron/jobs" => json!({"jobs":[]}),
            "/api/fs/default-cwd" => json!({"cwd":"/private-fixture"}),
            "/api/fs/list" => json!({"entries":[]}),
            "/api/fs/read-text"
                if url
                    .query_pairs()
                    .any(|(k, v)| k == "path" && v.ends_with(".gitignore")) =>
            {
                json!({"exists":false,"content":""})
            }
            "/api/config" => {
                json!({"config":{"model":{"provider":"fixture","default":"fixture-model"},"approvals":{"mode":"smart"}}})
            }
            "/api/config/schema" => json!({"fields":{}}),
            "/api/model/options" => {
                json!({"provider":"fixture","model":"fixture-model","providers":[{"slug":"fixture","name":"Private fixture","authenticated":true,"models":["fixture-model"]}]})
            }
            "/api/model/auxiliary" => {
                json!({"main":{"provider":"fixture","model":"fixture-model"},"tasks":[]})
            }
            "/api/model/moa" => {
                json!({"presets":{},"default_preset":"default","active_preset":"default"})
            }
            "/api/audio/elevenlabs/voices" => json!({"available":false,"voices":[]}),
            "/api/audio/voice-live/status" => json!({"ok":false}),
            "/api/git/review/rev-parse" => json!({"sha":null}),
            "/api/git/review/list" => json!({"files":[],"is_repo":false}),
            "/api/git/review/ship-info" => json!({"ghReady":false}),
            "/api/git/status" => json!({"is_repo":false}),
            "/api/git/branches" | "/api/git/base-branches" => json!({"branches":[]}),
            "/api/git/worktrees" => json!({"worktrees":[]}),
            path if path.starts_with("/api/sessions/") && path.ends_with("/messages") => {
                json!({"profile":profile,"session_id":"shared-tip","messages":[{"row_id":1,"role":"assistant","content":"Distinct private history"}]})
            }
            _ => return,
        };
        value.to_string()
    } else {
        return;
    };
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
}

#[cfg(test)]
mod tests {
    use super::super::{Cancellation, Pool, Ssh};
    use super::*;
    /// Line-oriented control process for the Linux native acceptance harness.
    #[test]
    #[ignore = "launched explicitly by the isolated native acceptance harness"]
    fn native_bridge() {
        let fixture = Fixture::new();
        let mut generations = std::collections::HashMap::<String, Generation>::new();
        let mut forward: Option<Value> = None;
        let mut out = std::io::stdout().lock();
        writeln!(out, "{}", json!({"event":"ready","home":fixture.root,"ssh_binary":fixture.ssh_binary,"host":fixture.spec.host,"user":fixture.spec.user,"port":fixture.spec.port,"launcher":fixture.spec.remote_hermes_path,"anonymous_base":format!("http://127.0.0.1:{}",fixture.anonymous_port)})).unwrap();
        out.flush().unwrap();
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let Ok(command) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let id = &command["id"];
            let result = match command["op"].as_str() {
                Some("hold_proof") => {
                    fixture.proof_barrier.arm();
                    json!({"ok":true})
                }
                Some("wait_proof") => {
                    let ms = command["timeout_ms"].as_u64().unwrap_or(3000).min(8000);
                    json!({"entered":fixture.proof_barrier.wait_entered(Duration::from_millis(ms))})
                }
                Some("release_proof") => {
                    fixture.proof_barrier.release();
                    json!({"ok":true})
                }
                Some("calls") => json!({"calls":fixture.calls()}),
                Some("snapshot") => {
                    let state = fixture.sessions.lock().unwrap();
                    let anonymous = fixture.anonymous_sessions.lock().unwrap();
                    json!({"running":state.running,"sequence":state.sequence,"tick":state.tick,"anonymous":{"running":anonymous.running,"sequence":anonymous.sequence,"tick":anonymous.tick},"calls":fixture.calls()})
                }
                Some("owners") => {
                    let directory = fixture.root.join(".hermes/desktop-ssh");
                    let owners: Vec<String> = std::fs::read_dir(directory)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .filter_map(|e| e.file_name().into_string().ok())
                        .filter(|v| {
                            v.len() == 32
                                && v.bytes()
                                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        })
                        .collect();
                    json!({"owners":owners})
                }
                Some("generation") => {
                    let owner = command["owner"].as_str().unwrap_or("");
                    match fixture.generation(owner) {
                        Ok(g) => {
                            let result = json!({"pid":g.pid,"nonce":g.nonce,"start_ticks":g.start_ticks,"running":g.running()});
                            generations.insert(owner.to_string(), g);
                            result
                        }
                        Err(e) => json!({"error":e}),
                    }
                }
                Some("inspect") => match generations.get(command["owner"].as_str().unwrap_or("")) {
                    Some(g) => {
                        json!({"running":g.running(),"files_retired":g.files_retired(),"lock_exists":g.directory.join("backend.lock.json").exists(),"token_exists":g.directory.join(format!("{}.token",g.nonce)).exists(),"log_exists":g.directory.join(format!("{}.log",g.nonce)).exists()})
                    }
                    None => json!({"error":"generation not captured"}),
                },
                Some("forward") => {
                    match std::fs::read(fixture.root.join("forward.json"))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    {
                        Some(record) => {
                            forward = Some(record.clone());
                            json!({"record":record})
                        }
                        None => json!({"error":"forward not captured"}),
                    }
                }
                Some("inspect_forward") => match &forward {
                    Some(record) => {
                        let running = |item: &Value| {
                            let Some(pid) = item["pid"].as_u64() else {
                                return false;
                            };
                            let Some(ticks) = item["start_ticks"].as_u64() else {
                                return false;
                            };
                            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                            else {
                                return false;
                            };
                            let Some((_, suffix)) = stat.rsplit_once(')') else {
                                return false;
                            };
                            let fields: Vec<_> = suffix.split_whitespace().collect();
                            fields.first() != Some(&"Z")
                                && fields.get(19).and_then(|v| v.parse::<u64>().ok()) == Some(ticks)
                        };
                        let port = record["local_port"].as_u64().unwrap_or(0) as u16;
                        json!({"running":running(record), "helper_running":running(&record["helper"]),
                            "port_accepts":port != 0 && TcpStream::connect_timeout(&std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_millis(100)).is_ok()})
                    }
                    None => json!({"error":"forward not captured"}),
                },
                Some("shutdown") => {
                    writeln!(out, "{}", json!({"id":id,"ok":true})).unwrap();
                    out.flush().unwrap();
                    break;
                }
                _ => json!({"error":"unknown operation"}),
            };
            writeln!(out, "{}", json!({"id":id,"result":result})).unwrap();
            out.flush().unwrap();
        }
    }

    #[test]
    fn fake_ssh_rejects_unexpected_commands() {
        let fixture = Fixture::new();
        let output = std::process::Command::new(&fixture.ssh_binary)
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "-o",
                "ControlPersist=no",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "TCPKeepAlive=yes",
                "-p",
                "2222",
                "-l",
                "private",
                "-T",
                "--",
                "private.fixture",
                "touch forbidden",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(64));
        assert!(!fixture.root.join("forbidden").exists());
    }

    #[test]
    fn interrupt_on_control_socket_completes_every_existing_stream_subscriber() {
        let fixture = Fixture::new();
        let address = format!("ws://127.0.0.1:{}/api/ws", fixture.anonymous_port);
        let (mut stream, _) = tungstenite::connect(address.as_str()).unwrap();
        let _ = stream.read().unwrap();
        let call = |socket: &mut tungstenite::WebSocket<
            tungstenite::stream::MaybeTlsStream<TcpStream>,
        >,
                    id,
                    method,
                    params| {
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            loop {
                let event: Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                if event["id"] == id {
                    break;
                }
            }
        };
        call(
            &mut stream,
            1,
            "session.resume",
            json!({"session_id":"shared-tip","profile":"default"}),
        );
        call(
            &mut stream,
            2,
            "prompt.submit",
            json!({"session_id":"live-shared-tip","profile":"default","text":"Legacy native synthetic stream","submission_id":"private-fanout"}),
        );
        let (mut wrong_subscriber, _) = tungstenite::connect(address.as_str()).unwrap();
        let _ = wrong_subscriber.read().unwrap();
        call(
            &mut wrong_subscriber,
            10,
            "session.resume",
            json!({"session_id":"shared-tip","profile":"wrong-profile"}),
        );
        let auth_address = format!(
            "ws://127.0.0.1:{}/api/ws?token=fixture-served-token",
            fixture.port
        );
        let (mut other_source, _) = tungstenite::connect(auth_address.as_str()).unwrap();
        let _ = other_source.read().unwrap();
        call(
            &mut other_source,
            11,
            "session.resume",
            json!({"session_id":"shared-tip","profile":"default"}),
        );
        call(
            &mut other_source,
            12,
            "prompt.submit",
            json!({"session_id":"live-shared-tip","profile":"default","text":"Legacy native synthetic stream","submission_id":"private-other-source"}),
        );
        for (profile, session) in [
            ("wrong-profile", "live-shared-tip"),
            ("default", "wrong-session"),
        ] {
            let (mut wrong, _) = tungstenite::connect(address.as_str()).unwrap();
            let _ = wrong.read().unwrap();
            wrong.send(Message::Text(json!({"jsonrpc":"2.0","id":99,"method":"session.interrupt","params":{"profile":profile,"session_id":session}}).to_string().into())).unwrap();
            assert!(matches!(wrong.read(), Err(_) | Ok(Message::Close(_))));
            assert!(fixture.anonymous_sessions.lock().unwrap().running);
        }
        let (mut control, _) = tungstenite::connect(address.as_str()).unwrap();
        let _ = control.read().unwrap();
        call(
            &mut control,
            3,
            "session.resume",
            json!({"session_id":"shared-tip","profile":"default"}),
        );
        call(
            &mut control,
            4,
            "session.interrupt",
            json!({"session_id":"live-shared-tip","profile":"default"}),
        );
        if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = stream.get_mut() {
            tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        }
        let first_generation = loop {
            let event: Value = serde_json::from_str(
                &stream
                    .read()
                    .expect("subscriber receives completion")
                    .into_text()
                    .unwrap(),
            )
            .unwrap();
            if event["params"]["type"] == "message.complete" {
                break event["params"]["execution_generation"].as_u64().unwrap();
            }
        };
        call(
            &mut control,
            5,
            "prompt.submit",
            json!({"session_id":"live-shared-tip","profile":"default","text":"Legacy native synthetic stream","submission_id":"private-next-generation"}),
        );
        call(
            &mut control,
            6,
            "session.interrupt",
            json!({"session_id":"live-shared-tip","profile":"default"}),
        );
        loop {
            let event: Value = serde_json::from_str(
                &stream
                    .read()
                    .expect("subscriber receives next completion")
                    .into_text()
                    .unwrap(),
            )
            .unwrap();
            if event["params"]["type"] == "message.complete" {
                assert!(
                    event["params"]["execution_generation"].as_u64().unwrap() > first_generation
                );
                break;
            }
        }
        assert!(!fixture.anonymous_sessions.lock().unwrap().running);
        assert!(fixture.sessions.lock().unwrap().running);
        if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = wrong_subscriber.get_mut() {
            tcp.set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
        }
        assert!(
            matches!(wrong_subscriber.read(), Err(tungstenite::Error::Io(error)) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
        );
    }

    #[test]
    fn anonymous_bridge_streams_and_interrupts() {
        let fixture = Fixture::new();
        let (mut ws, _) =
            tungstenite::connect(format!("ws://127.0.0.1:{}/api/ws", fixture.anonymous_port))
                .unwrap();
        let _: Value =
            serde_json::from_str(ws.read().unwrap().into_text().unwrap().as_ref()).unwrap();
        for (id, method, params) in [
            (
                1,
                "session.resume",
                json!({"session_id":"shared-tip","profile":"default"}),
            ),
            (
                2,
                "prompt.submit",
                json!({"session_id":"live-shared-tip","profile":"default","text":"Legacy native synthetic stream","submission_id":"private-submission"}),
            ),
        ] {
            ws.send(Message::Text(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .unwrap();
            let reply: Value =
                serde_json::from_str(ws.read().unwrap().into_text().unwrap().as_ref()).unwrap();
            assert_eq!(reply["id"], id);
        }
        let start: Value =
            serde_json::from_str(ws.read().unwrap().into_text().unwrap().as_ref()).unwrap();
        assert_eq!(start["params"]["type"], "message.start");
        let delta: Value =
            serde_json::from_str(ws.read().unwrap().into_text().unwrap().as_ref()).unwrap();
        assert_eq!(delta["params"]["type"], "message.delta");
        ws.send(Message::Text(json!({"jsonrpc":"2.0","id":3,"method":"session.interrupt","params":{"profile":"default","session_id":"live-shared-tip"}}).to_string().into())).unwrap();
        let reply: Value =
            serde_json::from_str(ws.read().unwrap().into_text().unwrap().as_ref()).unwrap();
        assert_eq!(reply["id"], 3);
        let done: Value =
            serde_json::from_str(ws.read().unwrap().into_text().unwrap().as_ref()).unwrap();
        assert_eq!(done["params"]["type"], "message.complete");
        assert!(!fixture.anonymous_sessions.lock().unwrap().running);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shared_forward_lives_until_last_resource_then_retires() {
        let fixture = Fixture::new();
        std::fs::write(fixture.root.join("forward-helper"), "").unwrap();
        let ssh = Ssh::new(fixture.ssh_binary.clone(), fixture.spec.clone()).unwrap();
        let pool = Pool::default();
        let store = super::super::engine::test_store();
        let owner = "0123456789abcdef0123456789abcdef";
        let mut first = pool
            .connect_with_store(ssh.clone(), owner, store.clone(), &Cancellation::default())
            .unwrap();
        let mut second = pool
            .connect_with_store(ssh, owner, store, &Cancellation::default())
            .unwrap();
        let record: Value =
            serde_json::from_slice(&std::fs::read(fixture.root.join("forward.json")).unwrap())
                .unwrap();
        let pid = record["pid"].as_u64().unwrap();
        let ticks = record["start_ticks"].as_u64().unwrap();
        let port = record["local_port"].as_u64().unwrap() as u16;
        let helper = record["helper"]["pid"].as_u64().unwrap();
        let helper_ticks = record["helper"]["start_ticks"].as_u64().unwrap();
        // Only runs after assertions/unwind; never substitutes for retirement proof.
        struct Fallback(Vec<(u64, u64)>);
        impl Drop for Fallback {
            fn drop(&mut self) {
                for (pid, ticks) in &self.0 {
                    let _ = std::process::Command::new("python3").args([
                        "-c", "import os,signal,sys\np=int(sys.argv[1]);t=int(sys.argv[2])\ntry:\n f=os.pidfd_open(p);s=open('/proc/%d/stat'%p).read().rsplit(')',1)[1].split()\n if int(s[19])==t and s[0]!='Z':signal.pidfd_send_signal(f,signal.SIGKILL)\n os.close(f)\nexcept ProcessLookupError:pass",
                        &pid.to_string(), &ticks.to_string(),
                    ]).status();
                }
            }
        }
        let _fallback = Fallback(vec![(pid, ticks), (helper, helper_ticks)]);
        let running = |pid, ticks| {
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                return false;
            };
            let fields: Vec<_> = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .collect();
            fields[0] != "Z" && fields[19].parse::<u64>().unwrap() == ticks
        };
        assert_eq!(record["pgid"].as_u64(), Some(pid));
        assert_eq!(record["helper"]["pgid"].as_u64(), Some(pid));
        assert!(running(pid, ticks));
        assert!(running(helper, helper_ticks));
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok());
        first.resource.close().unwrap();
        assert!(running(pid, ticks));
        assert!(running(helper, helper_ticks));
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok());
        second.resource.close().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while running(helper, helper_ticks) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!running(pid, ticks));
        assert!(!running(helper, helper_ticks));
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
    }

    #[test]
    fn fixture_proves_roster_and_runtime_on_one_authenticated_socket() {
        let fixture = Fixture::new();
        let ssh = Ssh::new(fixture.ssh_binary.clone(), fixture.spec.clone()).unwrap();
        let ready = Pool::default()
            .connect_with_store(
                ssh,
                "0123456789abcdef0123456789abcdef",
                super::super::engine::test_store(),
                &Cancellation::default(),
            )
            .unwrap();
        let generation = fixture
            .generation("0123456789abcdef0123456789abcdef")
            .unwrap();
        assert!(generation.running().unwrap());
        let url = format!(
            "ws://127.0.0.1:{}/api/ws?token=fixture-served-token",
            ready.endpoint.rsplit(':').next().unwrap()
        );
        let (mut ws, _) = tungstenite::connect(url.as_str()).unwrap();
        let notification: Value =
            serde_json::from_str(ws.read().unwrap().into_text().unwrap().as_ref()).unwrap();
        assert_eq!(notification["method"], "gateway.ready");
        for (id, method, params) in [
            (1, "profiles.list", json!({"profile":"default"})),
            (2, "runtime.describe", json!({})),
        ] {
            if method == "runtime.describe" {
                fixture.proof_barrier.arm();
            }
            ws.send(Message::Text(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .unwrap();
            if method == "runtime.describe" {
                assert!(fixture.proof_barrier.wait_entered(Duration::from_secs(3)));
                assert_eq!(fixture.calls().len(), 2);
                fixture.proof_barrier.release();
            }
            let reply: Value =
                serde_json::from_str(ws.read().unwrap().into_text().unwrap().as_ref()).unwrap();
            assert_eq!(reply["id"], id);
        }
        let calls = fixture.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, calls[1].0);
        assert_eq!(calls[1].1["params"], json!({}));
        drop(ws);
        drop(ready);
        assert!(generation.files_retired().unwrap());
        assert!(!generation.running().unwrap());
        drop(fixture);
    }
}
