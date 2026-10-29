//! Authenticated audio transports. A take/voice session owns one captured
//! connection and profile for warm-up, PCM, transcription and release.
use super::*;
use base64::Engine;
use serde::Deserialize;
const MAX_AUDIO: usize = 25 * 1024 * 1024;
const MAX_AUDIO_JSON: usize = 36 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct Transcript {
    pub transcript: String,
    pub provider: Option<String>,
}
#[derive(Clone)]
pub struct Speech {
    pub bytes: Vec<u8>,
    pub mime: String,
}
#[derive(Clone, Debug)]
pub struct LiveAnswer {
    pub session_id: String,
    pub sdp: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioStream {
    Transcribe,
    Speak,
}
#[derive(Debug)]
pub enum AudioEvent {
    Partial(String),
    Final(String),
    Start { sample_rate: u32, channels: u16 },
    Pcm(Vec<u8>),
    End,
    Fallback,
    Error(String),
}

pub struct AudioGateway {
    gateway: Gateway,
    profile: String,
}
impl AudioGateway {
    pub fn connect(connection: &Connection, profile: &str) -> Result<Self, String> {
        valid_scope(profile)?;
        let mut gateway = Gateway::connect(connection)?;
        #[cfg(target_os = "linux")]
        if matches!(gateway.auth, Auth::Native(_)) {
            gateway.profiles()?;
            if !gateway.profile_homes.contains_key(profile) {
                return Err("The audio profile is not served by this gateway".into());
            }
        }
        Ok(Self {
            gateway,
            profile: profile.into(),
        })
    }
    fn json(
        &mut self,
        path: &str,
        body: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        let mut url = self
            .gateway
            .base
            .join(path.trim_start_matches('/'))
            .map_err(|_| "Invalid audio route")?;
        url.query_pairs_mut().append_pair("profile", &self.profile);
        let mut headers = reqwest::header::HeaderMap::new();
        match &self.gateway.auth {
            Auth::Token(Some(token)) => {
                url.query_pairs_mut().append_pair("token", token);
            }
            Auth::Token(None) => {}
            #[cfg(target_os = "linux")]
            Auth::Native(endpoint) => {
                let home = self
                    .gateway
                    .profile_homes
                    .get(&self.profile)
                    .ok_or("Refresh the owning audio profile")?;
                let ticket = endpoint.ticket_until(home, "native-http", deadline)?;
                headers.insert(
                    "X-Hermes-Gateway-Ticket",
                    ticket.parse().map_err(|_| "Invalid private audio ticket")?,
                );
            }
        }
        let mut request = if body.is_some() {
            self.gateway.http.post(url)
        } else {
            self.gateway.http.get(url)
        }
        .headers(headers)
        .timeout(timeout);
        if let Some(body) = body {
            let bytes = serde_json::to_vec(&body).map_err(|_| "Invalid audio request")?;
            if bytes.len() > MAX_AUDIO_JSON {
                return Err("The audio request exceeds the 25 MB recording limit".into());
            }
            request = request
                .header("Content-Type", "application/json")
                .body(bytes);
        }
        let response = request
            .send()
            .map_err(|_| "The audio request failed (network, TLS or timeout)")?;
        let status = response.status();
        let mut bytes = vec![];
        response
            .take((MAX_AUDIO_JSON + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "The audio response could not be read")?;
        if bytes.len() > MAX_AUDIO_JSON {
            return Err("The audio response is too large".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
            format!(
                "The gateway returned invalid audio data (HTTP {})",
                status.as_u16()
            )
        })?;
        if !status.is_success() {
            let detail = value
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("Audio request refused");
            return Err(format!(
                "{} (HTTP {})",
                detail.chars().take(400).collect::<String>(),
                status.as_u16()
            ));
        }
        if value.get("ok") != Some(&Value::Bool(true)) {
            return Err("The gateway did not confirm the audio operation".into());
        }
        Ok(value)
    }
    pub fn lease(&mut self, stt: bool, id: &str, active: bool) -> Result<Option<String>, String> {
        if id.trim().is_empty() {
            return Err("An audio lease needs a client identity".into());
        }
        let value = self.json(
            if stt {
                "/api/audio/stt-lease"
            } else {
                "/api/audio/tts-lease"
            },
            Some(json!({"lease":id,"active":active})),
            Duration::from_secs(180),
        )?;
        Ok(value
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_owned))
    }
    pub fn transcribe(&mut self, bytes: &[u8], mime: &str) -> Result<Transcript, String> {
        if bytes.is_empty() {
            return Err("The recording contains no audio".into());
        }
        if bytes.len() > MAX_AUDIO {
            return Err("The recording exceeds the gateway's 25 MB limit".into());
        }
        if !matches!(
            mime,
            "audio/wav" | "audio/webm" | "audio/ogg" | "audio/mpeg" | "audio/flac" | "audio/mp4"
        ) {
            return Err("Unsupported recording format".into());
        }
        let value=self.json("/api/audio/transcribe",Some(json!({"data_url":format!("data:{mime};base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes)),"mime_type":mime})),Duration::from_secs(180))?;
        serde_json::from_value(value)
            .map_err(|_| "The transcription acknowledgement is invalid".into())
    }
    pub fn speak(&mut self, text: &str) -> Result<Speech, String> {
        if text.trim().is_empty() {
            return Err("There is no text to read aloud".into());
        }
        let value = self.json(
            "/api/audio/speak",
            Some(json!({"text":text})),
            Duration::from_secs(180),
        )?;
        let mime = required_str(&value, "mime_type")?;
        if !mime.starts_with("audio/") {
            return Err("The speech response is not audio".into());
        }
        let url = required_str(&value, "data_url")?;
        let (prefix, data) = url
            .split_once(',')
            .ok_or("The speech data URL is invalid")?;
        if prefix != format!("data:{mime};base64") {
            return Err("The speech data URL has an unexpected format".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| "The speech response is not valid base64")?;
        if bytes.is_empty() || bytes.len() > MAX_AUDIO {
            return Err("The speech response is empty or too large".into());
        }
        Ok(Speech {
            bytes,
            mime: mime.into(),
        })
    }
    pub fn live_answer(&mut self, sdp: &str, history: Value) -> Result<LiveAnswer, String> {
        if sdp.trim().is_empty() || !history.is_array() {
            return Err("GPT-Live needs a real SDP offer and conversation history".into());
        }
        // SDP is byte-exact: trimming the CRLF corrupts the vendor's parser.
        let value = self.json(
            "/api/audio/voice-live/session",
            Some(json!({"sdp":sdp,"history":history})),
            Duration::from_secs(45),
        )?;
        let session = value
            .pointer("/session/id")
            .and_then(Value::as_str)
            .ok_or("GPT-Live returned no session identity")?;
        let sdp = value
            .pointer("/transport/sdp")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .ok_or("GPT-Live returned no SDP answer")?;
        Ok(LiveAnswer {
            session_id: session.into(),
            sdp: sdp.into(),
        })
    }
    pub fn set_engine(
        connection: &Connection,
        profile: &str,
        engine: VoiceEngine,
    ) -> Result<VoiceStatus, String> {
        let mut control = super::conversation::SessionTransport::connect(connection, profile)?;
        control.call("config.set",json!({"key":"voice.voice_chat_mode","value":match engine{VoiceEngine::Chained=>"chained",VoiceEngine::GptLive=>"gpt-live"}})).map_err(|e|e.message)?;
        let mut gateway = Gateway::connect(connection)?;
        let status = gateway
            .voice_status(profile)?
            .ok_or("The agent does not report voice engines")?;
        if status.engine != engine {
            return Err("The agent did not confirm the selected voice engine".into());
        }
        Ok(status)
    }
    pub fn stream(&mut self, kind: AudioStream, sample_rate: u32) -> Result<AudioSocket, String> {
        if kind == AudioStream::Transcribe && !(8000..=48000).contains(&sample_rate) {
            return Err("Unsupported microphone sample rate".into());
        }
        let path = match kind {
            AudioStream::Transcribe => "api/audio/transcribe-stream",
            AudioStream::Speak => "api/audio/speak-stream",
        };
        let mut url = self
            .gateway
            .base
            .join(path)
            .map_err(|_| "Invalid audio stream route")?;
        url.set_scheme(if self.gateway.base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        })
        .map_err(|_| "Invalid audio stream scheme")?;
        url.query_pairs_mut().append_pair("profile", &self.profile);
        let mut ticket = None;
        match &self.gateway.auth {
            Auth::Token(Some(token)) => {
                url.query_pairs_mut().append_pair("token", token);
            }
            Auth::Token(None) => {}
            #[cfg(target_os = "linux")]
            Auth::Native(endpoint) => {
                let home = self
                    .gateway
                    .profile_homes
                    .get(&self.profile)
                    .ok_or("The audio profile is not bound to this gateway")?;
                ticket = Some(endpoint.ticket(home, "interactive")?);
            }
        }
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| "Invalid audio stream request")?;
        if let Some(ticket) = &ticket {
            request.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                format!("hermes-gateway-v1, hermes-gateway-ticket.{ticket}")
                    .parse()
                    .map_err(|_| "Invalid audio stream ticket")?,
            );
        }
        let stream = connect_tcp(&url)?;
        let config = tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_BODY))
            .max_frame_size(Some(MAX_BODY));
        let (socket, response) =
            tungstenite::client_tls_with_config(request, stream, Some(config), None)
                .map_err(|_| "The live audio connection failed (network, TLS or authentication)")?;
        if ticket.is_some()
            && response
                .headers()
                .get("Sec-WebSocket-Protocol")
                .and_then(|h| h.to_str().ok())
                != Some("hermes-gateway-v1")
        {
            return Err("The audio stream did not accept the private gateway protocol".into());
        }
        let mut socket = AudioSocket {
            socket,
            kind,
            ended: false,
        };
        if kind == AudioStream::Transcribe {
            socket.send(json!({"sample_rate":sample_rate}))?;
        }
        Ok(socket)
    }
}

pub struct AudioSocket {
    socket: WebSocket<MaybeTlsStream<DeadlineStream>>,
    kind: AudioStream,
    ended: bool,
}
impl AudioSocket {
    fn send(&mut self, value: Value) -> Result<(), String> {
        socket_timeout(self.socket.get_mut(), Duration::from_secs(5))?;
        self.socket
            .send(Message::Text(value.to_string().into()))
            .map_err(|_| "The audio stream could not send its control frame".into())
    }
    pub fn push_pcm(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.kind != AudioStream::Transcribe
            || self.ended
            || bytes.len() % 2 != 0
            || bytes.len() > 256 * 1024
        {
            return Err("Invalid mono PCM input frame".into());
        }
        socket_timeout(self.socket.get_mut(), Duration::from_secs(5))?;
        self.socket
            .send(Message::Binary(bytes.to_vec().into()))
            .map_err(|_| "The live transcription connection failed".into())
    }
    pub fn push_text(&mut self, text: &str) -> Result<(), String> {
        if self.kind != AudioStream::Speak || self.ended {
            return Err("The speech stream is not accepting text".into());
        }
        self.send(json!({"text":text}))
    }
    pub fn finish(&mut self) -> Result<(), String> {
        if self.ended {
            return Ok(());
        }
        self.ended = true;
        self.send(if self.kind == AudioStream::Transcribe {
            json!({"eos":true})
        } else {
            json!({"done":true})
        })
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.ended = true;
        self.send(json!({"stop":true}))?;
        self.socket
            .close(None)
            .map_err(|_| "The audio stream has already closed".into())
    }
    pub fn poll(&mut self, timeout: Duration) -> Result<Option<AudioEvent>, String> {
        socket_timeout(self.socket.get_mut(), timeout)?;
        match self.socket.read() {
            Ok(Message::Binary(bytes)) if self.kind == AudioStream::Speak => {
                if bytes.len() % 2 != 0 {
                    return Err("The speech stream returned incomplete PCM samples".into());
                }
                Ok(Some(AudioEvent::Pcm(bytes.to_vec())))
            }
            Ok(Message::Text(text)) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|_| "The audio stream returned invalid JSON")?;
                let kind = required_str(&value, "type")?;
                Ok(Some(match kind {
                    "partial" => AudioEvent::Partial(required_str(&value, "text")?.into()),
                    "final" => AudioEvent::Final(
                        value
                            .get("transcript")
                            .and_then(Value::as_str)
                            .ok_or("Invalid final transcription")?
                            .trim()
                            .into(),
                    ),
                    "start" => {
                        let rate = value
                            .get("sample_rate")
                            .and_then(Value::as_u64)
                            .filter(|r| (8000..=192000).contains(r))
                            .ok_or("Unsupported speech sample rate")?;
                        let channels = value
                            .get("channels")
                            .and_then(Value::as_u64)
                            .filter(|v| *v == 1 || *v == 2)
                            .ok_or("Unsupported speech channels")?;
                        AudioEvent::Start {
                            sample_rate: rate as u32,
                            channels: channels as u16,
                        }
                    }
                    "end" => AudioEvent::End,
                    "fallback" => AudioEvent::Fallback,
                    "error" => AudioEvent::Error(
                        value
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("Live audio failed")
                            .into(),
                    ),
                    _ => return Ok(None),
                }))
            }
            Ok(Message::Close(_)) => Err("The audio stream closed before completion".into()),
            Ok(Message::Ping(_)) => {
                self.socket
                    .flush()
                    .map_err(|_| "The audio stream heartbeat failed")?;
                Ok(None)
            }
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err("The audio stream connection failed".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        thread,
    };
    fn http(listener: &TcpListener, path: &str, expected: Value, result: Value) {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        let request = url::Url::parse(&format!(
            "http://fixture{}",
            first.split_whitespace().nth(1).unwrap()
        ))
        .unwrap();
        assert_eq!(request.path(), path);
        assert_eq!(
            request
                .query_pairs()
                .find(|(k, _)| k == "profile")
                .unwrap()
                .1,
            "voice-owner"
        );
        assert_eq!(
            request.query_pairs().find(|(k, _)| k == "token").unwrap().1,
            "synthetic-audio-token"
        );
        let mut size = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(length) = line.to_lowercase().strip_prefix("content-length:") {
                size = length.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; size];
        reader.read_exact(&mut body).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), expected);
        let text = result.to_string();
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",text.len()).unwrap();
    }
    #[test]
    fn audio_requests_keep_owner_and_sdp_bytes_and_release_the_same_lease() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let connection = Connection::Url {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            token: Some("synthetic-audio-token".into()),
        };
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _rpc = tungstenite::accept(stream).unwrap();
            http(
                &listener,
                "/api/audio/stt-lease",
                json!({"lease":"take-1","active":true}),
                json!({"ok":true,"lease":"take-1","active":true}),
            );
            http(
                &listener,
                "/api/audio/transcribe",
                json!({"data_url":"data:audio/wav;base64,AQACAA==","mime_type":"audio/wav"}),
                json!({"ok":true,"transcript":"Привет","provider":"fixture"}),
            );
            http(
                &listener,
                "/api/audio/speak",
                json!({"text":"Привет"}),
                json!({"ok":true,"mime_type":"audio/wav","data_url":"data:audio/wav;base64,AQACAA=="}),
            );
            http(
                &listener,
                "/api/audio/voice-live/session",
                json!({"sdp":"v=0\r\n","history":[]}),
                json!({"ok":true,"session":{"id":"live-fixture"},"transport":{"type":"webrtc","sdp":"answer\r\n"}}),
            );
            http(
                &listener,
                "/api/audio/stt-lease",
                json!({"lease":"take-1","active":false}),
                json!({"ok":true,"lease":"take-1","active":false}),
            );
        });
        let mut gateway = AudioGateway::connect(&connection, "voice-owner").unwrap();
        gateway.lease(true, "take-1", true).unwrap();
        assert_eq!(
            gateway
                .transcribe(&[1, 0, 2, 0], "audio/wav")
                .unwrap()
                .transcript,
            "Привет"
        );
        let speech = gateway.speak("Привет").unwrap();
        assert_eq!(speech.bytes, vec![1, 0, 2, 0]);
        assert_eq!(speech.mime, "audio/wav");
        let answer = gateway.live_answer("v=0\r\n", json!([])).unwrap();
        assert_eq!(answer.sdp, "answer\r\n");
        gateway.lease(true, "take-1", false).unwrap();
        server.join().unwrap();
    }
    #[test]
    fn live_dictation_sends_pcm_and_eos_and_keeps_partials_separate_from_final() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let connection = Connection::Url {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            token: Some("synthetic-audio-token".into()),
        };
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _rpc = tungstenite::accept(stream).unwrap();
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut ws = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request,
                 response: tungstenite::handshake::server::Response| {
                    let url = Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
                    assert_eq!(url.path(), "/api/audio/transcribe-stream");
                    assert_eq!(
                        url.query_pairs().find(|(k, _)| k == "profile").unwrap().1,
                        "voice-owner"
                    );
                    Ok(response)
                },
            )
            .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&ws.read().unwrap().into_text().unwrap()).unwrap(),
                json!({"sample_rate":16000})
            );
            assert_eq!(ws.read().unwrap().into_data().as_ref(), &[1, 0, 2, 0]);
            assert_eq!(
                serde_json::from_str::<Value>(&ws.read().unwrap().into_text().unwrap()).unwrap(),
                json!({"eos":true})
            );
            ws.send(Message::Text(
                json!({"type":"partial","text":"При"}).to_string().into(),
            ))
            .unwrap();
            ws.send(Message::Text(
                json!({"type":"final","transcript":"Привет"})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        });
        let mut gateway = AudioGateway::connect(&connection, "voice-owner").unwrap();
        let mut stream = gateway.stream(AudioStream::Transcribe, 16000).unwrap();
        stream.push_pcm(&[1, 0, 2, 0]).unwrap();
        stream.finish().unwrap();
        assert!(
            matches!(stream.poll(Duration::from_secs(3)).unwrap(),Some(AudioEvent::Partial(text))if text=="При")
        );
        assert!(
            matches!(stream.poll(Duration::from_secs(3)).unwrap(),Some(AudioEvent::Final(text))if text=="Привет")
        );
        server.join().unwrap();
    }
}
