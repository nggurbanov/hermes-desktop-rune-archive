use super::*;
use rune::{App, AppSpec, Chrome, Key, KeyEvent, Tx};
use std::io::BufReader;
pub(crate) struct WindowState {
    pub title: String,
    pub goal: String,
    pub profile: String,
    pub child: String,
    pub theme: Option<bool>,
    pub projection: worker::Projection,
    pub chat: crate::chat::Chat,
    pub audio: crate::audio::Audio,
    pub reading: Option<String>,
    pub(crate) files: super::media::Files,
    epoch: u64,
    ready_images: usize,
    connection: Connection,
    watch: Option<worker::Watch>,
    waker: Option<Waker>,
    fixture: bool,
}
impl WindowState {
    fn new(launch: process::Launch) -> Self {
        let connection = launch.connection();
        let fixture = launch.fixture;
        let mut state = Self {
            title: launch.title,
            goal: launch.goal,
            profile: launch.profile,
            child: launch.child,
            theme: launch.theme,
            projection: worker::Projection::default(),
            chat: crate::chat::Chat::new(),
            audio: crate::audio::Audio::new(),
            reading: None,
            files: super::media::Files::new(),
            epoch: launch.epoch,
            ready_images: 0,
            connection,
            watch: None,
            waker: None,
            fixture,
        };
        if fixture {
            state.projection=worker::Projection {activities:vec![],messages:vec![crate::gateway::HistoryMessage::text("user",&state.goal),crate::gateway::HistoryMessage::text("assistant","I checked all 80 candidates across the eight scenarios.\n\n**16 images analyzed**, six source files reviewed. The strongest proposals match both the brief and the reference images.\n\nI am comparing the remaining variants and recording the differences.")],reasoning:"Compare each candidate with the task constraints, then verify the source before summarizing.".into(),tools:vec!["Analyzed 16 images".into(),"Explored 6 files".into()],running:true,connected:true,error:None};
        }
        state
    }
    pub fn owner(&self) -> DraftScope {
        DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: self.epoch,
            profile: self.profile.clone(),
            session: Some(self.child.clone()),
        }
    }
    pub fn toggle(&mut self, key: String) {
        let owner = self.owner();
        let expanded = &mut self.chat.presentation(owner).expanded;
        if !expanded.remove(&key) {
            expanded.insert(key);
        }
    }
    pub fn image(&mut self, source: String) {
        let owner = self.owner();
        let preview = &mut self.chat.presentation(owner).preview;
        if preview.as_ref() == Some(&source) {
            *preview = None;
        } else {
            *preview = Some(source);
        }
    }
    pub fn retry_image(&mut self, source: String) {
        self.chat
            .retry_media(self.connection.clone(), self.owner(), source);
    }
    pub fn read(&mut self, key: String, text: String) {
        if self.fixture {
            return;
        }
        if self.reading.is_some() {
            self.audio.cancel(&self.owner());
            self.reading = None;
            return;
        }
        match self
            .audio
            .speak(self.connection.clone(), self.owner(), text)
        {
            Ok(()) => self.reading = Some(key),
            Err(error) => self.files.error = Some(error),
        }
    }
    pub fn transfer(
        &mut self,
        expected: DraftScope,
        source: String,
        label: String,
        destination: Option<std::path::PathBuf>,
    ) {
        if expected != self.owner() || self.fixture {
            return;
        }
        if let Err(error) = self.files.start(
            self.connection.clone(),
            expected,
            source,
            label,
            destination,
        ) {
            self.files.error = Some(error);
        }
    }
    pub fn retry(&mut self) {
        if self.fixture {
            return;
        }
        if let Some(waker) = self.waker.clone() {
            self.watch = Some(worker::start(
                self.connection.clone(),
                self.profile.clone(),
                self.child.clone(),
                waker,
            ));
            self.projection.error = None;
        }
    }
}
fn default_state() -> WindowState {
    WindowState::new(process::Launch::synthetic())
}
fn setup(state: &mut WindowState, waker: Waker) {
    state.chat.init(waker.clone());
    state.audio.init(waker.clone());
    state.files.init(waker.clone());
    state.waker = Some(waker);
    state.retry();
}
fn poll(state: &mut WindowState, _now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some(watch) = &state.watch {
        for projection in watch.receive.try_iter() {
            state.projection = projection;
            changed = true;
        }
    }
    changed |= !state.chat.poll().is_empty();
    let owner = state.owner();
    let sources = crate::chat::projection::project(&state.projection.messages)
        .into_iter()
        .flat_map(|message| message.parts)
        .filter_map(|part| match part {
            crate::chat::projection::Part::Image { source, .. } => Some(source),
            _ => None,
        })
        .collect();
    state
        .chat
        .prepare_media(state.connection.clone(), owner.clone(), sources);
    let ready = state
        .chat
        .media
        .values()
        .filter(|media| media.image.as_ref().is_some_and(|image| image.is_ready()))
        .count();
    if ready != state.ready_images {
        state.ready_images = ready;
        changed = true;
    }
    changed |= state.files.poll(&owner);
    if let Some(url) = state.files.open_url.take() {
        if tx.open_url(&url).is_err() {
            state.files.error = Some("The native application could not open this file".into());
        }
        changed = true;
    }
    if !state.audio.poll().is_empty() {
        changed = true;
        if let Some(view) = state.audio.view(&owner) {
            if let Some(error) = &view.error {
                state.files.error = Some(error.clone());
            }
            if view.status == crate::audio::Status::Idle {
                state.reading = None;
            }
        }
    }
    changed
}
fn close(state: &mut WindowState, _tx: &mut Tx) {
    state.audio.close_connection(state.epoch);
    state.watch = None;
}
fn key(_state: &mut WindowState, event: &KeyEvent, tx: &mut Tx) -> bool {
    if event.key == Key::Command('w') {
        tx.close_window();
        return true;
    }
    false
}
pub fn run_if_window_mode() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--check-subagent-windows") {
        if let Err(error) = super::proof::run(&args) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return true;
    }
    let synthetic = args.iter().any(|arg| arg == "--subagent-scene");
    if !synthetic && !args.iter().any(|arg| arg == process::ARG) {
        return false;
    }
    let mut input = BufReader::new(std::io::stdin());
    let launch = if synthetic {
        process::Launch::synthetic()
    } else {
        match process::frame(&mut input)
            .ok()
            .flatten()
            .filter(|v| v["type"] == "launch")
            .and_then(|v| serde_json::from_value::<process::Launch>(v["launch"].clone()).ok())
        {
            Some(value) if !value.child.trim().is_empty() && !value.profile.trim().is_empty() => {
                value
            }
            _ => return true,
        }
    };
    let spec = AppSpec {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(key),
        min_size: Some((380.0, 400.0)),
        ..AppSpec::new(
            "Hermes · Subagent",
            (560.0, 800.0),
            default_state,
            crate::view::subagents::window,
        )
    };
    let application = App::new(spec)
        .on_close(close)
        .init(move || WindowState::new(launch))
        .chrome(Chrome {
            full_size_content: true,
            traffic_lights: Some((16.0, 16.0)),
            drag_regions: true,
        });
    let handle = application.handle();
    if !synthetic {
        std::thread::spawn(move || {
            loop {
                match process::frame(&mut input) {
                    Ok(Some(value)) => match value["type"].as_str() {
                        Some("focus") => {
                            handle.open();
                            handle.update(|_, _| process::receipt("focus", None));
                        }
                        Some("inspect") => {
                            handle.update(|state,_|{
                            let parts:Vec<_>=crate::chat::projection::project(&state.projection.messages).into_iter().flat_map(|message|message.parts).collect();
                            process::report(serde_json::json!({"type":"inspection","images_ready":state.ready_images,"image_parts":parts.iter().filter(|part|matches!(part,crate::chat::projection::Part::Image{..})).count(),"file_parts":parts.iter().filter(|part|matches!(part,crate::chat::projection::Part::File{..})).count(),"tool_parts":parts.iter().filter(|part|matches!(part,crate::chat::projection::Part::Tool{..})).count()+state.projection.activities.len(),"media_error":state.chat.media.values().any(|media|media.error.is_some()),"expanded_tools":state.chat.views.get(&state.owner()).map(|view|view.expanded.iter().filter(|key|key.starts_with("live-tool-")).count()).unwrap_or(0)}));
                        });
                        }
                        Some("theme") => {
                            let dark = value.get("dark").and_then(Value::as_bool);
                            handle.update(move |state, tx| {
                                state.theme = dark;
                                tx.instant();
                                process::receipt("theme", dark);
                            });
                        }
                        Some("close") => {
                            handle.close();
                            break;
                        }
                        _ => {}
                    },
                    _ => {
                        handle.close();
                        break;
                    }
                }
            }
        });
    }
    application
        .on_ready(move |_| {
            if !synthetic {
                process::ready();
            }
        })
        .run();
    true
}
