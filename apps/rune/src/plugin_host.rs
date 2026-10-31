//! Desktop-plugin compatibility and trusted live voice pages run in an owned WebView process.
mod manager;
mod native;
mod sources;
mod uploads;
pub use manager::{AmbientTarget, HostAction, NavigationTarget, PluginHosts};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
};
const MAX_FRAME: usize = 64 * 1024 * 1024;
pub const CHILD_ARG: &str = "--native-web-host";
#[derive(Clone, Serialize, Deserialize)]
pub struct Document {
    pub title: String,
    pub html: String,
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    #[serde(default)]
    pub allow_media: bool,
}
pub struct HostProcess {
    child: Child,
    input: Arc<Mutex<ChildStdin>>,
    receive: Receiver<Result<Value, String>>,
    closed: bool,
}
impl HostProcess {
    pub fn alive(&mut self) -> bool {
        if self.closed {
            return false;
        }
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => {
                self.closed = true;
                false
            }
            Err(_) => false,
        }
    }
    pub fn send(&mut self, value: Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(&value).map_err(|_| "Invalid host command")?;
        if bytes.len() > MAX_FRAME {
            return Err("Host command exceeds the size limit".into());
        }
        let mut input = self
            .input
            .lock()
            .map_err(|_| "The host command stream is unavailable")?;
        input
            .write_all(&bytes)
            .and_then(|_| input.write_all(b"\n"))
            .and_then(|_| input.flush())
            .map_err(|_| "The host window closed".into())
    }
    pub fn poll(&mut self) -> Vec<Result<Value, String>> {
        let mut events: Vec<_> = self.receive.try_iter().collect();
        if !self.closed {
            if let Ok(Some(_)) = self.child.try_wait() {
                self.closed = true;
                events.push(Ok(host_frame(
                    json!({"type":"closed","reason":"host-exited"}),
                )));
            }
        }
        events
    }
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        let _ = self.send(json!({"type":"close"}));
        for _ in 0..30 {
            if self.child.try_wait().is_ok_and(|status| status.is_some()) {
                self.closed = true;
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.closed = true;
    }
}
impl Drop for HostProcess {
    fn drop(&mut self) {
        self.close();
    }
}
pub fn spawn_document(
    title: impl Into<String>,
    html: impl Into<String>,
    assets: BTreeMap<String, String>,
    allow_media: bool,
) -> Result<HostProcess, String> {
    let executable =
        std::env::current_exe().map_err(|_| "The desktop executable is unavailable")?;
    let mut child = Command::new(executable)
        .arg(CHILD_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "The native web host could not be started")?;
    let input = child
        .stdin
        .take()
        .ok_or("The native host command stream is unavailable")?;
    let output = child
        .stdout
        .take()
        .ok_or("The native host response stream is unavailable")?;
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            match read_frame(&mut reader) {
                Ok(Some(value)) => {
                    if send.send(Ok(host_frame(value))).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = send.send(Err(error));
                    break;
                }
            }
        }
    });
    let mut process = HostProcess {
        child,
        input: Arc::new(Mutex::new(input)),
        receive,
        closed: false,
    };
    let document = Document {
        title: title.into(),
        html: html.into(),
        assets,
        allow_media,
    };
    process.send(json!({"type":"document","document":document}))?;
    Ok(process)
}
pub fn run_if_host_mode() -> bool {
    if !std::env::args().any(|arg| arg == CHILD_ARG) {
        return false;
    }
    if let Err(error) = native::run() {
        native::emit(json!({"type":"error","message":error}));
    }
    true
}
fn read_frame(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut bytes = vec![];
    let read = reader
        .take((MAX_FRAME + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "The host IPC stream could not be read")?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_FRAME || bytes.last() != Some(&b'\n') {
        return Err("The host IPC frame exceeds the size limit".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "The host IPC frame is invalid".into())
}

fn host_frame(mut value: Value) -> Value {
    if value["type"] == "host-web-message" {
        value = value["message"].take();
        if let Some(object) = value.as_object_mut() {
            object.insert("_native_host".into(), false.into());
        }
    } else if let Some(object) = value.as_object_mut() {
        object.insert("_native_host".into(), true.into());
    }
    value
}
#[cfg(test)]
mod origin_tests {
    use super::*;
    #[test]
    fn renderer_cannot_forge_native_window_facts_or_lifecycle() {
        let value = host_frame(
            json!({"type":"host-web-message","message":{"type":"host-window-state","focused":false,"visible":false,"_native_host":true}}),
        );
        assert_eq!(value["_native_host"], false);
        assert_eq!(
            host_frame(json!({"type":"host-window-state","focused":true,"visible":true}))["_native_host"],
            true
        );
    }
}
