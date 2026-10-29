use super::*;
use serde::{Deserialize, Serialize};
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
#[derive(Serialize, Deserialize)]
pub(super) struct Launch {
    transport: Transport,
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
            Connection::Saved { .. } => unreachable!(),
        };
        Self {
            transport,
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
        match &self.transport {
            Transport::Local { home } => Connection::Local { home: home.clone() },
            Transport::Url { endpoint, token } => Connection::Url {
                endpoint: endpoint.clone(),
                token: token.clone(),
            },
        }
    }
    pub fn synthetic() -> Self {
        Self {
            transport: Transport::Url {
                endpoint: "http://127.0.0.1:1".into(),
                token: None,
            },
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
