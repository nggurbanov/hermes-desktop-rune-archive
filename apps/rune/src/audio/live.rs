//! A real GPT-Live session. Linux uses native ICE/DTLS/SRTP and Opus because
//! its system WebKit may not implement RTCPeerConnection.
use crate::{composer::DraftScope, gateway::Connection};
use rune::Waker;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::Ordering,
    mpsc::{self, Receiver, Sender},
};
#[derive(Debug)]
pub enum Event {
    Ready,
    Partial(String),
    Delegation { text: String, context: String },
    Speaking(bool),
    Level(f32),
    Error(String),
    Warning(String),
    Closed,
}
pub struct Live {
    send: Sender<Value>,
    receive: Receiver<Event>,
    controls: Option<Arc<super::device::Controls>>,
}
impl Live {
    pub fn start(
        connection: Connection,
        scope: DraftScope,
        history: Value,
        waker: Waker,
    ) -> Result<Self, String> {
        if !history.is_array() {
            return Err("GPT-Live needs conversation history".into());
        }
        let (send, commands) = mpsc::channel::<Value>();
        let (events, receive) = mpsc::channel();
        #[cfg(target_os = "linux")]
        let controls = Some(super::native_live::spawn(
            connection, scope, history, commands, events, waker,
        ));
        #[cfg(not(target_os = "linux"))]
        let controls = {
            super::live_web::spawn(connection, scope, history, commands, events, waker);
            None
        };
        Ok(Self {
            send,
            receive,
            controls,
        })
    }
    pub fn poll(&mut self) -> Vec<Event> {
        self.receive.try_iter().collect()
    }
    pub fn mute(&mut self, muted: bool) {
        if let Some(controls) = &self.controls {
            controls.muted.store(muted, Ordering::Release);
        }
        let _ = self.send.send(json!({"type":"mute","muted":muted}));
    }
    pub fn speak(&mut self, text: &str) -> Result<(), String> {
        self.send
            .send(json!({"type":"speak","text":text}))
            .map_err(|_| "The voice session has ended".into())
    }
    pub fn think(&mut self, text: &str) -> Result<(), String> {
        self.send
            .send(json!({"type":"think","text":text}))
            .map_err(|_| "The voice session has ended".into())
    }
    pub fn stop_turn(&mut self) -> Result<(), String> {
        self.send
            .send(json!({"type":"stop-turn"}))
            .map_err(|_| "The voice session has ended".into())
    }
    pub fn close(&mut self) {
        if let Some(controls) = &self.controls {
            controls.stop.store(true, Ordering::Release);
        }
        let _ = self.send.send(json!({"type":"close"}));
    }
}
impl Drop for Live {
    fn drop(&mut self) {
        self.close();
    }
}
