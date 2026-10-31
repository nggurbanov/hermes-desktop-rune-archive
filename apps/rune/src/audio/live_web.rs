//! Platforms with a system WebRTC implementation reuse the same vendor
//! protocol as native media; this page never receives gateway credentials.
use super::{live::Event, live_protocol::Protocol};
use crate::{
    composer::DraftScope,
    gateway::{Connection, audio::AudioGateway},
};
use rune::Waker;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};
pub(super) fn spawn(
    connection: Connection,
    scope: DraftScope,
    history: Value,
    commands: Receiver<Value>,
    events: Sender<Event>,
    waker: Waker,
) {
    std::thread::spawn(move || {
        let emit = |event| {
            if events.send(event).is_ok() {
                waker();
            }
        };
        let result = (|| -> Result<(), String> {
            let mut host = crate::plugin_host::spawn_document(
                "Hermes voice",
                include_str!("live-host.html"),
                BTreeMap::new(),
                true,
            )?;
            let (answer_send, answers) = mpsc::channel();
            let mut offered = false;
            let mut protocol = Protocol::default();
            loop {
                for command in commands.try_iter() {
                    for frame in protocol.command(&command) {
                        host.send(json!({"type":"vendor","frame":frame}))?;
                    }
                    if command["type"] == "close" {
                        host.send(json!({"type":"close"}))?;
                        host.close();
                        emit(Event::Closed);
                        return Ok(());
                    }
                }
                for answer in answers.try_iter() {
                    let answer: Result<crate::gateway::audio::LiveAnswer, String> = answer;
                    let answer = answer?;
                    host.send(
                        json!({"type":"answer","sdp":answer.sdp,"session_id":answer.session_id}),
                    )?;
                }
                for event in host.poll() {
                    let event = event?;
                    match event.get("type").and_then(Value::as_str).unwrap_or("") {
                        "offer" => {
                            if offered {
                                return Err("The voice host produced a second offer".into());
                            }
                            offered = true;
                            let sdp = event
                                .get("sdp")
                                .and_then(Value::as_str)
                                .ok_or("The voice host returned no SDP offer")?
                                .to_owned();
                            let connection = connection.clone();
                            let profile = scope.profile.clone();
                            let intent = scope.profile_intent;
                            let history = history.clone();
                            let send = answer_send.clone();
                            std::thread::spawn(move || {
                                let result =
                                    AudioGateway::connect_scoped(&connection, &profile, intent)
                                        .and_then(|mut gateway| gateway.live_answer(&sdp, history));
                                let _ = send.send(result);
                            });
                        }
                        "vendor" => {
                            for event in protocol.vendor(&event["frame"]) {
                                emit(event);
                            }
                            if protocol.closed {
                                host.close();
                                return Ok(());
                            }
                        }
                        "speaking" => emit(Event::Speaking(event["speaking"] == true)),
                        "warning" => emit(Event::Warning(
                            event
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("Voice warning")
                                .into(),
                        )),
                        "error" => {
                            return Err(event
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("Voice device failed")
                                .into());
                        }
                        "closed" => {
                            host.close();
                            emit(Event::Closed);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                if protocol.stop_phrase() {
                    host.send(json!({"type":"close"}))?;
                    host.close();
                    emit(Event::Closed);
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        })();
        if let Err(error) = result {
            emit(Event::Error(error));
        }
    });
}
