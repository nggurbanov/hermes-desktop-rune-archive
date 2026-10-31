use super::*;
use crate::{
    gateway::prepared::{WirePolicy, WirePolicySnapshot},
    source::{RouteDoor, RouteOwner},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
};
pub const ARG: &str = "--subagent-window";
const FRAME_LIMIT: usize = 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Transport {
    Local {
        home: PathBuf,
    },
    Url {
        endpoint: String,
        token: Option<String>,
    },
}
// Credential material stays on the inherited private control pipe. A malformed
// envelope is rejected while decoding Launch, before a native child can connect.
struct CapturedRoute {
    policy: Arc<WirePolicy>,
    owner: Arc<RouteOwner>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedRouteSnapshot {
    version: u32,
    policy: WirePolicySnapshot,
    owner: RouteOwnerSnapshot,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteOwnerSnapshot {
    door: OwnerDoor,
    namespace: String,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum OwnerDoor {
    Legacy,
    Saved { id: String, revision: String },
}
impl RouteOwnerSnapshot {
    fn capture(owner: &RouteOwner) -> Self {
        let door = match owner.door() {
            RouteDoor::Legacy => OwnerDoor::Legacy,
            RouteDoor::Saved { id, revision } => OwnerDoor::Saved {
                id: id.clone(),
                revision: revision.clone(),
            },
        };
        Self {
            door,
            namespace: owner.namespace().to_owned(),
        }
    }
    fn restore(self) -> Result<RouteOwner, String> {
        match self.door {
            OwnerDoor::Legacy => RouteOwner::legacy(self.namespace),
            OwnerDoor::Saved { id, revision } => RouteOwner::saved(id, revision, self.namespace),
        }
    }
}
impl CapturedRoute {
    fn capture(connection: &Connection) -> Option<Self> {
        match connection {
            Connection::Routed { policy, owner, .. } => Some(Self {
                policy: policy.clone(),
                owner: owner.clone(),
            }),
            Connection::Saved { inner, .. } => Self::capture(inner),
            _ => None,
        }
    }
}
impl Serialize for CapturedRoute {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CapturedRouteSnapshot {
            version: 1,
            policy: self.policy.private_ipc_snapshot(),
            owner: RouteOwnerSnapshot::capture(&self.owner),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for CapturedRoute {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let snapshot = CapturedRouteSnapshot::deserialize(deserializer)?;
        if snapshot.version != 1 {
            return Err(serde::de::Error::custom("Unsupported inherited route"));
        }
        let policy =
            WirePolicy::from_private_ipc(snapshot.policy).map_err(serde::de::Error::custom)?;
        let owner = snapshot.owner.restore().map_err(serde::de::Error::custom)?;
        Ok(Self {
            policy: Arc::new(policy),
            owner: Arc::new(owner),
        })
    }
}
#[derive(Serialize, Deserialize)]
pub(super) struct Launch {
    transport: Transport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    route: Option<CapturedRoute>,
    pub epoch: u64,
    pub profile: String,
    pub child: String,
    pub title: String,
    pub goal: String,
    pub theme: Option<bool>,
    #[serde(default)]
    pub fixture: bool,
}
impl Launch {
    pub fn new(
        connection: &Connection,
        scope: &DraftScope,
        row: &ChildRow,
        theme: Option<bool>,
    ) -> Self {
        let transport = match connection.transport() {
            Connection::Local { home } => Transport::Local { home: home.clone() },
            Connection::Url { endpoint, token } => Transport::Url {
                endpoint: endpoint.clone(),
                token: token.clone(),
            },
            Connection::Saved { .. } | Connection::Routed { .. } => unreachable!(),
        };
        Self {
            transport,
            route: CapturedRoute::capture(connection),
            epoch: scope.connection,
            profile: scope.profile.clone(),
            child: row.session.clone().unwrap_or_default(),
            title: format!("Subagent: {}", row.task_index + 1),
            goal: row.goal.clone(),
            theme,
            fixture: false,
        }
    }
    pub fn connection(&self) -> Connection {
        let actual = match &self.transport {
            Transport::Local { home } => Connection::Local { home: home.clone() },
            Transport::Url { endpoint, token } => Connection::Url {
                endpoint: endpoint.clone(),
                token: token.clone(),
            },
        };
        match &self.route {
            Some(route) => Connection::Routed {
                actual: Box::new(actual),
                policy: route.policy.clone(),
                owner: route.owner.clone(),
            },
            None => actual,
        }
    }
    pub fn synthetic() -> Self {
        Self {
            transport: Transport::Url {
                endpoint: "http://127.0.0.1:1".into(),
                token: None,
            },
            route: None,
            epoch: 1,
            profile: "studio".into(),
            child: "child-session".into(),
            title: "Subagent: 1".into(),
            goal: "Compare the eight proposals and verify the source images.".into(),
            theme: None,
            fixture: true,
        }
    }
}
pub(super) struct NativeWindow {
    child: Child,
    input: ChildStdin,
    theme: Option<bool>,
    closed: bool,
    ready: bool,
    replies: Receiver<Value>,
    pub focus_receipts: u64,
    pub theme_receipt: Option<Value>,
    pub inspection: Option<Value>,
}
impl NativeWindow {
    pub fn spawn(launch: Launch) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|_| "The desktop executable is unavailable")?;
        let mut child = Command::new(exe)
            .arg(ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "The native subagent window could not start")?;
        let Some(mut input) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("The subagent control pipe is unavailable".into());
        };
        if send(&mut input, &json!({"type":"launch","launch":launch})).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("The native subagent window could not attach".into());
        }
        let output = child
            .stdout
            .take()
            .ok_or("The spectator response pipe is unavailable")?;
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || {
            let mut input = BufReader::new(output);
            while let Ok(Some(value)) = frame(&mut input) {
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            theme: launch.theme,
            closed: false,
            ready: false,
            replies,
            focus_receipts: 0,
            theme_receipt: None,
            inspection: None,
        })
    }
    pub fn inspect(&mut self) -> Result<(), String> {
        send(&mut self.input, &json!({"type":"inspect"}))
    }
    pub fn focus(&mut self) -> Result<(), String> {
        send(&mut self.input, &json!({"type":"focus"}))
    }
    pub fn theme(&mut self, theme: Option<bool>) {
        if theme != self.theme {
            self.theme = theme;
            let _ = send(&mut self.input, &json!({"type":"theme","dark":theme}));
        }
    }
    pub fn exited(&mut self) -> bool {
        for value in self.replies.try_iter() {
            if value["type"] == "inspection" {
                self.inspection = Some(value.clone());
            }
            if value["type"] == "ready" {
                self.ready = true;
            }
            if value["type"] == "focus" {
                self.focus_receipts += 1;
            }
            if value["type"] == "theme" {
                self.theme_receipt = Some(value["dark"].clone());
            }
        }
        if !self.closed && self.child.try_wait().is_ok_and(|v| v.is_some()) {
            self.closed = true;
        }
        self.closed
    }
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn startup_failed(&self) -> bool {
        self.closed && !self.ready
    }
}
impl Drop for NativeWindow {
    fn drop(&mut self) {
        if !self.exited() {
            let _ = send(&mut self.input, &json!({"type":"close"}));
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn send(output: &mut impl Write, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Invalid spectator command")?;
    if bytes.len() > FRAME_LIMIT {
        return Err("The spectator command exceeds the size limit".into());
    }
    output
        .write_all(&bytes)
        .and_then(|_| output.write_all(b"\n"))
        .and_then(|_| output.flush())
        .map_err(|_| "The subagent window closed".into())
}
pub(super) fn frame(input: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut bytes = Vec::new();
    let count = input
        .take((FRAME_LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "The spectator control pipe closed")?;
    if count == 0 {
        return Ok(None);
    }
    if count > FRAME_LIMIT || bytes.last() != Some(&b'\n') {
        return Err("Invalid spectator frame".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "Invalid spectator command".into())
}

pub(super) fn ready() {
    let _ = send(&mut std::io::stdout().lock(), &json!({"type":"ready"}));
}

pub(super) fn receipt(kind: &str, dark: Option<bool>) {
    let _ = send(
        &mut std::io::stdout().lock(),
        &json!({"type":kind,"dark":dark}),
    );
}

pub(super) fn report(value: Value) {
    let _ = send(&mut std::io::stdout().lock(), &value);
}

#[cfg(test)]
mod tests;
