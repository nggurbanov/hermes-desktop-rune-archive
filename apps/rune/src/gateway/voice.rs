//! Read the owning agent's voice engine. No configuration or audio writes.
use super::{Gateway, TIMEOUT, Value, valid_scope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceEngine {
    Chained,
    GptLive,
}

impl VoiceEngine {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chained => "Speech-to-text + Hermes voice",
            Self::GptLive => "GPT-Live (full-duplex, delegates to Hermes)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceStatus {
    pub engine: VoiceEngine,
    pub live_available: bool,
}

impl VoiceStatus {
    pub fn from_value(value: &Value) -> Result<Option<Self>, String> {
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            return Ok(None);
        }
        let engine = match value.get("mode").and_then(Value::as_str) {
            Some("chained") => VoiceEngine::Chained,
            Some("gpt-live") => VoiceEngine::GptLive,
            _ => return Err("The agent returned an unsupported voice engine".into()),
        };
        let live_available = value
            .get("available")
            .and_then(Value::as_bool)
            .ok_or("Invalid voice availability response")?;
        Ok(Some(Self {
            engine,
            live_available,
        }))
    }
}

impl Gateway {
    pub fn voice_status(&mut self, profile: &str) -> Result<Option<VoiceStatus>, String> {
        valid_scope(profile)?;
        let deadline = std::time::Instant::now() + TIMEOUT;
        #[cfg(target_os = "linux")]
        if matches!(&self.auth, super::Auth::Native(_)) && !self.profile_homes.contains_key(profile)
        {
            self.profiles_until(deadline)?;
        }
        let mut url = self
            .base
            .join("api/audio/voice-live/status")
            .map_err(|_| "Invalid voice status route")?;
        url.query_pairs_mut().append_pair("profile", profile);
        VoiceStatus::from_value(&self.get_json(profile, url, deadline, "Voice status")?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::{
        Connection,
        tests::{http_answer, http_request},
    };
    use serde_json::json;
    use std::{net::TcpListener, thread};

    #[test]
    fn voice_status_never_invents_a_supported_or_selected_engine() {
        assert_eq!(VoiceStatus::from_value(&json!({"ok":false})).unwrap(), None);
        assert!(
            VoiceStatus::from_value(&json!({"ok":true,"mode":"future","available":true})).is_err()
        );
        assert!(
            VoiceStatus::from_value(&json!({"ok":true,"mode":"chained","available":"yes"}))
                .is_err()
        );
        let status =
            VoiceStatus::from_value(&json!({"ok":true,"mode":"gpt-live","available":false}))
                .unwrap()
                .unwrap();
        assert_eq!(status.engine, VoiceEngine::GptLive);
        assert!(
            !status.live_available,
            "saved selection and current availability are distinct"
        );
    }

    #[test]
    fn voice_status_is_an_authenticated_same_owner_get_without_settings_writes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/prefix", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _socket = tungstenite::accept(stream).unwrap();
            let (mut http, _) = listener.accept().unwrap();
            let header = http_request(&mut http);
            assert!(header.starts_with("GET "));
            let path = header.split_whitespace().nth(1).unwrap();
            let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
            assert_eq!(url.path(), "/prefix/api/audio/voice-live/status");
            let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(params.get("profile").unwrap(), "work");
            assert_eq!(params.get("token").unwrap(), "synthetic voice token");
            http_answer(
                &mut http,
                json!({"ok":true,"mode":"chained","available":true,"api_key":"discarded synthetic key"}),
            );
        });
        let mut gateway = Gateway::connect(&Connection::Url {
            endpoint,
            token: Some("synthetic voice token".into()),
        })
        .unwrap();
        let status = gateway.voice_status("work").unwrap().unwrap();
        assert_eq!(status.engine, VoiceEngine::Chained);
        assert!(!format!("{status:?}").contains("synthetic key"));
        server.join().unwrap();
    }
}
