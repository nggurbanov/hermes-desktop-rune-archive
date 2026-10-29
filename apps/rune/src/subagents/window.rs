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
    pub show_reasoning: bool,
    pub show_tools: bool,
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
            show_reasoning: false,
            show_tools: true,
            connection,
            watch: None,
            waker: None,
            fixture,
        };
        if fixture {
            state.projection=worker::Projection {messages:vec![crate::gateway::HistoryMessage::text("user",&state.goal),crate::gateway::HistoryMessage::text("assistant","I checked all 80 candidates across the eight scenarios.\n\n**16 images analyzed**, six source files reviewed. The strongest proposals match both the brief and the reference images.\n\nI am comparing the remaining variants and recording the differences.")],reasoning:"Compare each candidate with the task constraints, then verify the source before summarizing.".into(),tools:vec!["Analyzed 16 images".into(),"Explored 6 files".into()],running:true,connected:true,error:None};
        }
        state
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
    state.waker = Some(waker);
    state.retry();
}
fn poll(state: &mut WindowState, _now: f64, _tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some(watch) = &state.watch {
        for projection in watch.receive.try_iter() {
            state.projection = projection;
            changed = true;
        }
    }
    changed
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
