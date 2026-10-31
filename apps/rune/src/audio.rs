//! Native dictation and speech. Every take retains its owner and generation;
//! changing a chat cannot insert a late transcript into another draft.
use crate::{
    composer::DraftScope,
    gateway::{
        Connection, VoiceEngine, VoiceStatus,
        audio::{AudioEvent, AudioGateway, AudioStream},
    },
};
use rune::Waker;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};
mod device;
#[cfg(target_os = "linux")]
mod dsp;
pub mod live;
mod live_protocol;
#[cfg(not(target_os = "linux"))]
mod live_web;
#[cfg(target_os = "linux")]
mod native_live;
mod progress;
mod speech;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Starting,
    Listening,
    Transcribing,
    Thinking,
    Speaking,
}
#[derive(Default)]
pub struct AudioView {
    pub status: Status,
    pub level: f32,
    pub elapsed: f32,
    pub partial: String,
    pub error: Option<String>,
    pub muted: bool,
    pub voice_active: bool,
    pub waiting_for_hermes: bool,
    pub engine: Option<VoiceEngine>,
    pub preferred_engine: Option<VoiceEngine>,
    pub live_available: bool,
}
impl AudioView {
    fn end_voice(&mut self) {
        self.voice_active = false;
        self.waiting_for_hermes = false;
        self.engine = self.preferred_engine.or(self.engine);
    }
    fn confirmed_engine(&mut self, engine: VoiceEngine) {
        self.preferred_engine = Some(engine);
        if !self.voice_active {
            self.engine = Some(engine);
        }
    }
}
#[derive(Clone, Debug)]
pub enum Update {
    Changed {
        scope: DraftScope,
    },
    Transcript {
        scope: DraftScope,
        text: String,
        submit: bool,
        voice_context: Option<String>,
    },
    Interrupt {
        scope: DraftScope,
    },
    Stopped {
        scope: DraftScope,
    },
}
struct Task {
    id: u64,
    controls: Arc<device::Controls>,
    started: Instant,
}
enum Reply {
    Status(DraftScope, u64, Status),
    Partial(DraftScope, u64, String),
    Transcript(DraftScope, u64, String, bool),
    Error(DraftScope, u64, String),
    SpeechDone(DraftScope, u64),
    Engine(DraftScope, u64, VoiceStatus),
    Warning(DraftScope, u64, String),
    Meter(DraftScope, u64),
    EngineError(DraftScope, u64, String),
}
pub struct Audio {
    views: HashMap<DraftScope, AudioView>,
    tasks: HashMap<DraftScope, Task>,
    connections: HashMap<DraftScope, Connection>,
    sequence: u64,
    replies: Sender<Reply>,
    receive: Receiver<Reply>,
    waker: Option<Waker>,
    live: HashMap<DraftScope, live::Live>,
    live_progress: HashMap<DraftScope, progress::Progress>,
    engine_pending: HashMap<(u64, String, crate::source::ProfileIntent), u64>,
}
impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}
impl Audio {
    pub fn new() -> Self {
        let (replies, receive) = mpsc::channel();
        Self {
            views: HashMap::new(),
            tasks: HashMap::new(),
            connections: HashMap::new(),
            sequence: 0,
            replies,
            receive,
            waker: None,
            live: HashMap::new(),
            live_progress: HashMap::new(),
            engine_pending: HashMap::new(),
        }
    }
    pub fn init(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }
    pub fn view(&self, scope: &DraftScope) -> Option<&AudioView> {
        self.views.get(scope)
    }
    pub fn start_dictation(
        &mut self,
        connection: Connection,
        scope: DraftScope,
    ) -> Result<(), String> {
        self.capture(connection, scope, false, false)
    }
    pub fn start_wake(&mut self, connection: Connection, scope: DraftScope) -> Result<(), String> {
        self.capture(connection, scope, false, true)
    }
    pub fn start_conversation(
        &mut self,
        connection: Connection,
        scope: DraftScope,
        engine: VoiceEngine,
        history: serde_json::Value,
    ) -> Result<(), String> {
        let waker = self
            .waker
            .clone()
            .ok_or("Audio transport is not initialized")?;
        if !self.live.is_empty()
            || self
                .tasks
                .iter()
                .any(|(_, t)| !t.controls.stop.load(Ordering::Acquire))
        {
            return Err(
                "Finish the current recording or playback before starting voice chat".into(),
            );
        }
        let view = self.views.entry(scope.clone()).or_default();
        view.voice_active = true;
        view.engine = Some(engine);
        view.preferred_engine = Some(engine);
        self.connections.insert(scope.clone(), connection.clone());
        if engine == VoiceEngine::GptLive {
            let session = match live::Live::start(connection, scope.clone(), history, waker) {
                Ok(session) => session,
                Err(error) => {
                    let view = self.views.get_mut(&scope).unwrap();
                    view.end_voice();
                    view.status = Status::Idle;
                    view.error = Some(error.clone());
                    return Err(error);
                }
            };
            self.live.insert(scope.clone(), session);
            self.views.get_mut(&scope).unwrap().status = Status::Starting;
            return Ok(());
        }
        let result = self.capture(connection, scope.clone(), true, true);
        if let Err(error) = &result {
            let view = self.views.get_mut(&scope).unwrap();
            view.end_voice();
            view.status = Status::Idle;
            view.error = Some(error.clone());
        }
        result
    }
    fn capture(
        &mut self,
        connection: Connection,
        scope: DraftScope,
        submit: bool,
        vad: bool,
    ) -> Result<(), String> {
        if !self.live.is_empty()
            || self
                .tasks
                .iter()
                .any(|(_, t)| !t.controls.stop.load(Ordering::Acquire))
        {
            return Err("A microphone recording or playback is already active".into());
        }
        let waker = self
            .waker
            .clone()
            .ok_or("Audio transport is not initialized")?;
        self.sequence += 1;
        let id = self.sequence;
        let controls = Arc::new(device::Controls::default());
        let view = self.views.entry(scope.clone()).or_default();
        view.status = Status::Starting;
        view.partial.clear();
        view.error = None;
        view.elapsed = 0.0;
        view.level = 0.0;
        view.muted = false;
        if submit {
            view.voice_active = true;
            view.engine = Some(VoiceEngine::Chained);
        }
        self.tasks.insert(
            scope.clone(),
            Task {
                id,
                controls: controls.clone(),
                started: Instant::now(),
            },
        );
        self.connections.insert(scope.clone(), connection.clone());
        let replies = self.replies.clone();
        std::thread::spawn(move || {
            let notify = |r| {
                if replies.send(r).is_ok() {
                    waker();
                }
            };
            let result = record(connection, &scope, id, &controls, vad, &notify);
            match result {
                Ok(Some(text)) => notify(Reply::Transcript(scope, id, text, submit)),
                Ok(None) => {}
                Err(error) => notify(Reply::Error(scope, id, error)),
            }
        });
        Ok(())
    }
    pub fn stop_dictation(&mut self, scope: &DraftScope) -> Result<(), String> {
        let task = self
            .tasks
            .get(scope)
            .ok_or("No recording is active in this conversation")?;
        task.controls.stop.store(true, Ordering::Release);
        if let Some(view) = self.views.get_mut(scope) {
            view.status = Status::Transcribing;
        }
        Ok(())
    }
    pub fn mute(&mut self, scope: &DraftScope, muted: bool) {
        if let Some(task) = self.tasks.get(scope) {
            task.controls.muted.store(muted, Ordering::Release);
        }
        if let Some(session) = self.live.get_mut(scope) {
            session.mute(muted);
        }
        if let Some(view) = self.views.get_mut(scope) {
            view.muted = muted;
        }
    }
    pub fn cancel(&mut self, scope: &DraftScope) {
        self.live_progress.remove(scope);
        if let Some(task) = self.tasks.remove(scope) {
            task.controls.cancelled.store(true, Ordering::Release);
            task.controls.stop.store(true, Ordering::Release);
        }
        if let Some(mut live) = self.live.remove(scope) {
            live.close();
        }
        if let Some(view) = self.views.get_mut(scope) {
            view.end_voice();
            view.status = Status::Idle;
            view.level = 0.0;
            view.partial.clear();
        }
    }
    pub fn close_connection(&mut self, connection: u64) {
        self.engine_pending
            .retain(|(epoch, _, _), _| *epoch != connection);
        let scopes: Vec<_> = self
            .views
            .keys()
            .filter(|s| s.connection == connection)
            .cloned()
            .collect();
        for scope in scopes {
            self.cancel(&scope);
            self.views.remove(&scope);
            self.connections.remove(&scope);
        }
    }
    pub fn rehome(&mut self, origin: &DraftScope, target: DraftScope) {
        // Device/audio ownership stays profile-bound; only adoption of the
        // user's new draft may change its stored conversation identity.
        if origin.connection != target.connection
            || origin.profile != target.profile
            || origin.profile_intent != target.profile_intent
        {
            return;
        }
        if let Some(view) = self.views.remove(origin) {
            self.views.insert(target.clone(), view);
        }
        if let Some(connection) = self.connections.remove(origin) {
            self.connections.insert(target.clone(), connection);
        }
        if let Some(task) = self.tasks.remove(origin) {
            self.tasks.insert(target.clone(), task);
        }
        if let Some(progress) = self.live_progress.remove(origin) {
            self.live_progress.insert(target.clone(), progress);
        }
        if let Some(live) = self.live.remove(origin) {
            self.live.insert(target, live);
        }
        // Captured profile/connection stays immutable; future transcript events
        // are projected through the map's adopted stored scope.
    }
    pub fn set_engine(
        &mut self,
        connection: Connection,
        scope: DraftScope,
        engine: VoiceEngine,
    ) -> Result<(), String> {
        let waker = self
            .waker
            .clone()
            .ok_or("Audio transport is not initialized")?;
        self.sequence += 1;
        let id = self.sequence;
        self.engine_pending.insert(
            (
                scope.connection,
                scope.profile.clone(),
                scope.profile_intent,
            ),
            id,
        );
        let replies = self.replies.clone();
        std::thread::spawn(move || {
            let result = AudioGateway::set_engine_scoped(
                &connection,
                &scope.profile,
                scope.profile_intent,
                engine,
            );
            let reply = match result {
                Ok(status) => Reply::Engine(scope, id, status),
                Err(e) => Reply::EngineError(scope, id, e),
            };
            if replies.send(reply).is_ok() {
                waker();
            }
        });
        Ok(())
    }
    pub fn speak(
        &mut self,
        connection: Connection,
        scope: DraftScope,
        text: String,
    ) -> Result<(), String> {
        if let Some(live) = self.live.get_mut(&scope) {
            return live.speak(&text);
        }
        if !self.live.is_empty() {
            return Err("Stop the active voice call before playing another conversation".into());
        }
        if text.trim().is_empty() {
            return self.resume_listening(scope);
        }
        if let Some(old) = self.tasks.remove(&scope) {
            old.controls.cancelled.store(true, Ordering::Release);
            old.controls.stop.store(true, Ordering::Release);
        }
        let waker = self
            .waker
            .clone()
            .ok_or("Audio transport is not initialized")?;
        self.sequence += 1;
        let id = self.sequence;
        let controls = Arc::new(device::Controls::default());
        self.tasks.insert(
            scope.clone(),
            Task {
                id,
                controls: controls.clone(),
                started: Instant::now(),
            },
        );
        self.views.entry(scope.clone()).or_default().status = Status::Speaking;
        let replies = self.replies.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut gateway = AudioGateway::connect_scoped(
                    &connection,
                    &scope.profile,
                    scope.profile_intent,
                )?;
                let lease = format!("desktop:rune:tts:{}", uuid::Uuid::new_v4());
                let _ = gateway.lease(false, &lease, true);
                let result = synthesize_and_play(&mut gateway, &text, &controls);
                let _ = gateway.lease(false, &lease, false);
                result
            })();
            let reply = match result {
                Ok(()) => Reply::SpeechDone(scope, id),
                Err(error) => Reply::Error(scope, id, error),
            };
            if replies.send(reply).is_ok() {
                waker();
            }
        });
        Ok(())
    }
    pub fn complete_live(
        &mut self,
        scope: &DraftScope,
        text: &str,
        failed: bool,
    ) -> Result<(), String> {
        self.finish_live(scope, text, failed)
    }
    pub fn think(&mut self, scope: &DraftScope, text: &str) -> Result<(), String> {
        if let Some(live) = self.live.get_mut(scope) {
            live.think(text)?;
        }
        Ok(())
    }
    pub fn resume_listening(&mut self, scope: DraftScope) -> Result<(), String> {
        if !self.views.get(&scope).is_some_and(|v| v.voice_active) {
            return Ok(());
        }
        let connection = self
            .connections
            .get(&scope)
            .cloned()
            .ok_or("The voice conversation lost its connection")?;
        self.capture(connection, scope, true, true)
    }
    pub fn stop_turn(&mut self, scope: &DraftScope) -> Result<(), String> {
        if let Some(live) = self.live.get_mut(scope) {
            return live.stop_turn();
        }
        self.stop_dictation(scope)
    }
    pub fn poll(&mut self) -> Vec<Update> {
        let mut updates = vec![];
        let mut resume = vec![];
        let mut closed_live = vec![];
        for reply in self.receive.try_iter().collect::<Vec<_>>() {
            if let Reply::Engine(scope, id, status) = &reply {
                let owner = (
                    scope.connection,
                    scope.profile.clone(),
                    scope.profile_intent,
                );
                if self.engine_pending.get(&owner) != Some(id) {
                    continue;
                }
                self.engine_pending.remove(&owner);
                let mut scopes: Vec<_> = self
                    .views
                    .keys()
                    .filter(|s| {
                        s.connection == scope.connection
                            && s.profile == scope.profile
                            && s.profile_intent == scope.profile_intent
                    })
                    .cloned()
                    .collect();
                if scopes.is_empty() {
                    scopes.push(scope.clone());
                }
                for scope in scopes {
                    let view = self.views.entry(scope.clone()).or_default();
                    view.confirmed_engine(status.engine);
                    view.live_available = status.live_available;
                    view.error = None;
                    updates.push(Update::Changed { scope });
                }
                continue;
            }
            if let Reply::EngineError(scope, id, error) = &reply {
                let owner = (
                    scope.connection,
                    scope.profile.clone(),
                    scope.profile_intent,
                );
                if self.engine_pending.get(&owner) != Some(id) {
                    continue;
                }
                self.engine_pending.remove(&owner);
                self.views.entry(scope.clone()).or_default().error = Some(error.clone());
                updates.push(Update::Changed {
                    scope: scope.clone(),
                });
                continue;
            }
            let (origin, id) = match &reply {
                Reply::Status(s, i, _)
                | Reply::Partial(s, i, _)
                | Reply::Transcript(s, i, _, _)
                | Reply::Error(s, i, _)
                | Reply::SpeechDone(s, i)
                | Reply::Engine(s, i, _)
                | Reply::Warning(s, i, _)
                | Reply::Meter(s, i)
                | Reply::EngineError(s, i, _) => (s.clone(), *i),
            };
            let Some(scope) = self
                .tasks
                .iter()
                .find(|(scope, task)| {
                    task.id == id
                        && scope.connection == origin.connection
                        && scope.profile == origin.profile
                        && scope.profile_intent == origin.profile_intent
                })
                .map(|(scope, _)| scope.clone())
            else {
                continue;
            };
            let view = self.views.entry(scope.clone()).or_default();
            match reply {
                Reply::Status(_, _, status) => {
                    view.status = status;
                    if status == Status::Listening {
                        if let Some(task) = self.tasks.get_mut(&scope) {
                            task.started = Instant::now();
                        }
                    }
                }
                Reply::Partial(_, _, text) => view.partial = text,
                Reply::Transcript(_, _, text, submit) => {
                    view.partial.clear();
                    view.level = 0.0;
                    view.status = if submit {
                        Status::Thinking
                    } else {
                        Status::Idle
                    };
                    if submit && stop_word(&text) {
                        view.end_voice();
                        view.status = Status::Idle;
                        updates.push(Update::Stopped {
                            scope: scope.clone(),
                        });
                    } else if !text.trim().is_empty() {
                        updates.push(Update::Transcript {
                            scope: scope.clone(),
                            text,
                            submit,
                            voice_context: None,
                        });
                    } else if submit {
                        resume.push(scope.clone());
                    } else {
                        view.error = Some("No speech detected. Try recording again".into());
                    }
                }
                Reply::Error(_, _, error) => {
                    view.status = Status::Idle;
                    view.end_voice();
                    view.error = Some(error);
                    view.level = 0.0;
                }
                Reply::SpeechDone(_, _) => {
                    view.status = Status::Idle;
                    if view.voice_active {
                        resume.push(scope.clone());
                    }
                }
                Reply::Engine(_, _, status) => {
                    view.confirmed_engine(status.engine);
                    view.live_available = status.live_available;
                    view.error = None;
                }
                Reply::Warning(_, _, warning) => view.error = Some(warning),
                Reply::Meter(_, _) => {}
                Reply::EngineError(_, _, _) => {}
            }
            updates.push(Update::Changed { scope });
        }
        for (scope, task) in &self.tasks {
            if let Some(view) = self.views.get_mut(scope) {
                if view.status == Status::Listening {
                    view.elapsed = task.started.elapsed().as_secs_f32();
                    view.level = f32::from_bits(task.controls.level.load(Ordering::Relaxed));
                }
            }
        }
        for (scope, session) in &mut self.live {
            for event in session.poll() {
                match event {
                    live::Event::Ready => {
                        self.views.entry(scope.clone()).or_default().status = Status::Listening;
                    }
                    live::Event::Level(level) => {
                        self.views.entry(scope.clone()).or_default().level = level
                    }
                    live::Event::Partial(text) => {
                        self.views.entry(scope.clone()).or_default().partial = text
                    }
                    live::Event::Delegation { text, context } => {
                        self.live_progress.remove(scope);
                        let view = self.views.entry(scope.clone()).or_default();
                        view.status = Status::Thinking;
                        view.waiting_for_hermes = true;
                        updates.push(Update::Interrupt {
                            scope: scope.clone(),
                        });
                        updates.push(Update::Transcript {
                            scope: scope.clone(),
                            text,
                            submit: true,
                            voice_context: Some(context),
                        });
                    }
                    live::Event::Speaking(speaking) => {
                        let view = self.views.entry(scope.clone()).or_default();
                        view.status = if speaking {
                            Status::Speaking
                        } else if view.waiting_for_hermes {
                            Status::Thinking
                        } else {
                            Status::Listening
                        };
                    }
                    live::Event::Error(error) => {
                        let view = self.views.entry(scope.clone()).or_default();
                        view.error = Some(error);
                        view.status = Status::Idle;
                        view.end_voice();
                        closed_live.push(scope.clone());
                    }
                    live::Event::Warning(warning) => {
                        self.views.entry(scope.clone()).or_default().error = Some(warning)
                    }
                    live::Event::Closed => {
                        let view = self.views.entry(scope.clone()).or_default();
                        view.status = Status::Idle;
                        view.end_voice();
                        updates.push(Update::Stopped {
                            scope: scope.clone(),
                        });
                        closed_live.push(scope.clone());
                    }
                }
                updates.push(Update::Changed {
                    scope: scope.clone(),
                });
            }
        }
        for scope in closed_live {
            self.live_progress.remove(&scope);
            self.live.remove(&scope);
        }
        for scope in resume {
            self.tasks.remove(&scope);
            if let Err(e) = self.resume_listening(scope.clone()) {
                self.views.entry(scope).or_default().error = Some(e);
            }
        }
        updates
    }
}
fn record(
    connection: Connection,
    scope: &DraftScope,
    id: u64,
    controls: &Arc<device::Controls>,
    vad: bool,
    notify: &impl Fn(Reply),
) -> Result<Option<String>, String> {
    let capture = device::Capture::open(controls.clone(), vad)?;
    if controls.cancelled.load(Ordering::Acquire) {
        return capture_cancelled(controls);
    }
    notify(Reply::Status(scope.clone(), id, Status::Listening));
    let mut gateway =
        AudioGateway::connect_scoped(&connection, &scope.profile, scope.profile_intent)?;
    let lease = format!("desktop:rune:stt:{}", uuid::Uuid::new_v4());
    match gateway.lease(true, &lease, true) {
        Ok(Some(warning)) => notify(Reply::Warning(scope.clone(), id, warning)),
        Err(warning) => notify(Reply::Warning(scope.clone(), id, warning)),
        _ => {}
    }
    let result = (|| {
        let mut stream = gateway.stream(AudioStream::Transcribe, capture.rate).ok();
        let mut cursor = 0;
        while !controls.stop.load(Ordering::Acquire) {
            notify(Reply::Meter(scope.clone(), id));
            if controls.cancelled.load(Ordering::Acquire) {
                return capture_cancelled(controls);
            }
            if let Some(socket) = &mut stream {
                let bytes = capture.pcm_from(&mut cursor)?;
                for chunk in bytes.chunks(64 * 1024) {
                    if socket.push_pcm(chunk).is_err() {
                        stream = None;
                        break;
                    }
                }
            }
            if let Some(socket) = &mut stream {
                match socket.poll(Duration::from_millis(80)) {
                    Ok(Some(AudioEvent::Partial(text))) => {
                        notify(Reply::Partial(scope.clone(), id, text))
                    }
                    Ok(Some(AudioEvent::Error(_))) | Err(_) => stream = None,
                    _ => {}
                }
            } else {
                std::thread::sleep(Duration::from_millis(80));
            }
        }
        if controls.cancelled.load(Ordering::Acquire) {
            return capture_cancelled(controls);
        }
        capture.wait_stopped()?;
        notify(Reply::Status(scope.clone(), id, Status::Transcribing));
        if let Some(socket) = &mut stream {
            let bytes = capture.pcm_from(&mut cursor)?;
            let mut failed = false;
            for chunk in bytes.chunks(64 * 1024) {
                if socket.push_pcm(chunk).is_err() {
                    failed = true;
                    break;
                }
            }
            if !failed && socket.finish().is_ok() {
                let deadline = Instant::now() + Duration::from_secs(180);
                while Instant::now() < deadline && !controls.cancelled.load(Ordering::Acquire) {
                    match socket.poll(Duration::from_millis(100)) {
                        Ok(Some(AudioEvent::Final(text))) if !text.is_empty() => {
                            return Ok(Some(text));
                        }
                        Ok(Some(AudioEvent::Final(_) | AudioEvent::Error(_))) | Err(_) => break,
                        Ok(Some(AudioEvent::Partial(text))) => {
                            notify(Reply::Partial(scope.clone(), id, text))
                        }
                        _ => {}
                    }
                }
            }
        }
        if controls.cancelled.load(Ordering::Acquire) {
            return capture_cancelled(controls);
        }
        let wav = capture.wav()?;
        gateway
            .transcribe(&wav, "audio/wav")
            .map(|t| Some(t.transcript))
    })();
    controls.stop.store(true, Ordering::Release);
    let _ = gateway.lease(true, &lease, false);
    result
}
fn capture_cancelled(controls: &Arc<device::Controls>) -> Result<Option<String>, String> {
    if controls.failed.load(Ordering::Acquire) {
        Err("The microphone disconnected or stopped unexpectedly".into())
    } else {
        Ok(None)
    }
}

fn synthesize_and_play(
    gateway: &mut AudioGateway,
    text: &str,
    controls: &Arc<device::Controls>,
) -> Result<(), String> {
    if controls.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    if let Ok(mut stream) = gateway.stream(AudioStream::Speak, 0) {
        if stream.push_text(text).and_then(|_| stream.finish()).is_ok() {
            let deadline = Instant::now() + Duration::from_secs(180);
            let mut format = None;
            let mut playback: Option<device::Playback> = None;
            let mut played = false;
            while Instant::now() < deadline {
                if controls.stop.load(Ordering::Acquire) {
                    let _ = stream.stop();
                    if let Some(output) = &mut playback {
                        output.stop();
                    }
                    return Ok(());
                }
                match stream.poll(Duration::from_millis(100)) {
                    Ok(Some(AudioEvent::Start {
                        sample_rate,
                        channels,
                    })) => format = Some((sample_rate, channels)),
                    Ok(Some(AudioEvent::Pcm(bytes))) => {
                        let (rate, channels) =
                            format.ok_or("Speech PCM arrived before its audio format")?;
                        if playback.is_none() {
                            playback = Some(device::Playback::open()?);
                        }
                        playback
                            .as_mut()
                            .unwrap()
                            .append_pcm(rate, channels, &bytes)?;
                        played = true;
                    }
                    Ok(Some(AudioEvent::End)) => {
                        if let Some(output) = &mut playback {
                            output.wait(controls);
                            return Ok(());
                        }
                        break;
                    }
                    Ok(Some(AudioEvent::Fallback)) if !played => break,
                    Ok(Some(AudioEvent::Error(error))) => {
                        if played {
                            return Err(error);
                        }
                        break;
                    }
                    Err(error) => {
                        if played {
                            return Err(error);
                        }
                        break;
                    }
                    _ => {}
                }
            }
            if played {
                return Err("The speech stream did not finish; playback was stopped".into());
            }
        }
    }
    if controls.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let speech = gateway.speak(text)?;
    if controls.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    device::play(speech.bytes, controls)
}

fn stop_word(text: &str) -> bool {
    matches!(
        text.trim()
            .trim_matches(|c: char| c.is_ascii_punctuation())
            .to_lowercase()
            .as_str(),
        "stop" | "cancel" | "goodbye" | "never mind" | "стоп" | "хватит" | "отмена" | "до свидания"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(session: Option<&str>) -> DraftScope {
        DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 7,
            profile: "voice-owner".into(),
            session: session.map(str::to_owned),
        }
    }
    #[test]
    fn failed_voice_initialization_never_claims_an_active_microphone() {
        let mut audio = Audio::new();
        let owner = scope(Some("stored"));
        let connection = Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        };
        assert!(
            audio
                .start_conversation(
                    connection.clone(),
                    owner.clone(),
                    VoiceEngine::GptLive,
                    serde_json::json!([])
                )
                .is_err()
        );
        assert!(audio.view(&owner).is_none());
        audio.init(Arc::new(|| {}));
        assert!(
            audio
                .start_conversation(
                    connection,
                    owner.clone(),
                    VoiceEngine::GptLive,
                    serde_json::json!({})
                )
                .is_err()
        );
        let view = audio.view(&owner).unwrap();
        assert_eq!(view.status, Status::Idle);
        assert!(!view.voice_active);
        assert!(view.error.as_deref().unwrap().contains("history"));
        assert!(audio.live.is_empty());
        assert!(audio.tasks.is_empty());
    }
    #[test]
    fn adoption_follows_task_generation_without_aliasing_a_future_new_draft() {
        let mut audio = Audio::new();
        let origin = scope(None);
        let target = scope(Some("stored"));
        audio.views.insert(origin.clone(), AudioView::default());
        audio.tasks.insert(
            origin.clone(),
            Task {
                id: 1,
                controls: Arc::new(device::Controls::default()),
                started: Instant::now(),
            },
        );
        audio.rehome(&origin, target.clone());
        audio.tasks.insert(
            origin.clone(),
            Task {
                id: 2,
                controls: Arc::new(device::Controls::default()),
                started: Instant::now(),
            },
        );
        audio.views.insert(origin.clone(), AudioView::default());
        audio
            .replies
            .send(Reply::Status(origin.clone(), 1, Status::Thinking))
            .unwrap();
        let updates = audio.poll();
        assert!(
            updates
                .iter()
                .any(|u| matches!(u,Update::Changed{scope}if scope==&target))
        );
        assert_eq!(audio.view(&target).unwrap().status, Status::Thinking);
        assert_eq!(audio.view(&origin).unwrap().status, Status::Idle);
        audio.cancel(&target);
        audio
            .replies
            .send(Reply::Partial(origin.clone(), 1, "stale".into()))
            .unwrap();
        assert!(audio.poll().is_empty());
        assert!(audio.view(&origin).unwrap().partial.is_empty());
    }
    #[test]
    fn changing_the_profile_engine_does_not_replace_or_stop_a_live_recording() {
        let mut audio = Audio::new();
        let owner = scope(Some("stored"));
        let controls = Arc::new(device::Controls::default());
        audio.views.insert(
            owner.clone(),
            AudioView {
                status: Status::Listening,
                voice_active: true,
                engine: Some(VoiceEngine::Chained),
                ..Default::default()
            },
        );
        audio.tasks.insert(
            owner.clone(),
            Task {
                id: 5,
                controls: controls.clone(),
                started: Instant::now(),
            },
        );
        audio.engine_pending.insert(
            (
                7,
                "voice-owner".into(),
                crate::source::ProfileIntent::Backend,
            ),
            6,
        );
        audio
            .replies
            .send(Reply::Engine(
                owner.clone(),
                6,
                VoiceStatus {
                    engine: VoiceEngine::GptLive,
                    live_available: true,
                },
            ))
            .unwrap();
        audio.poll();
        assert_eq!(audio.tasks.get(&owner).unwrap().id, 5);
        assert!(!controls.stop.load(Ordering::Acquire));
        assert_eq!(audio.view(&owner).unwrap().status, Status::Listening);
        assert_eq!(
            audio.view(&owner).unwrap().engine,
            Some(VoiceEngine::Chained)
        );
        assert_eq!(
            audio.view(&owner).unwrap().preferred_engine,
            Some(VoiceEngine::GptLive)
        );
        audio.cancel(&owner);
        assert_eq!(
            audio.view(&owner).unwrap().engine,
            Some(VoiceEngine::GptLive)
        );
    }
    #[test]
    fn opposite_intents_do_not_share_engine_replies_or_rehome() {
        let mut audio = Audio::new();
        let backend = scope(None);
        let mut native = backend.clone();
        native.profile_intent = crate::source::ProfileIntent::NativeSelf;
        audio.views.insert(backend.clone(), AudioView::default());
        audio.views.insert(native.clone(), AudioView::default());
        audio
            .engine_pending
            .insert((7, "voice-owner".into(), backend.profile_intent), 3);
        audio
            .engine_pending
            .insert((7, "voice-owner".into(), native.profile_intent), 4);
        audio
            .replies
            .send(Reply::Engine(
                backend.clone(),
                3,
                VoiceStatus {
                    engine: VoiceEngine::Chained,
                    live_available: false,
                },
            ))
            .unwrap();
        audio.poll();
        assert_eq!(
            audio.view(&backend).unwrap().preferred_engine,
            Some(VoiceEngine::Chained)
        );
        assert_eq!(audio.view(&native).unwrap().preferred_engine, None);
        audio.rehome(&backend, native.clone());
        assert!(audio.view(&backend).is_some());
        audio.cancel(&native);
        assert_eq!(
            audio.view(&backend).unwrap().preferred_engine,
            Some(VoiceEngine::Chained)
        );
    }
}
