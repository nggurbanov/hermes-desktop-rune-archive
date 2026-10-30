//! All complete Hermes windows share the same application factory and event loop.
use crate::{
    app::{self, Viewer},
    view,
};
use rune::{App, AppSpec, Chrome};

pub fn application(launch: Option<app::windows::Bootstrap>) -> App<Viewer> {
    let spec = AppSpec {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(app::setup),
        poll: Some(app::poll),
        on_key: Some(app::on_key),
        scenes: &[
            ("reader", app::reader_scene),
            ("chat-working", app::working_scene),
            ("chat-decisions", app::decisions_scene),
            ("settings", app::settings_scene),
            ("capabilities", app::capabilities_scene),
            ("messaging", app::messaging_scene),
            ("artifacts", app::artifacts_scene),
            ("cron", app::cron_scene),
            ("bots", app::bots_scene),
            ("bot-manager", app::bots_manager_scene),
            ("connections", app::connections_scene),
            ("command-center", app::operations_scene),
            ("terminal", app::terminal_scene),
            ("review", app::review_scene),
            ("file-preview", app::file_preview_scene),
            ("layout", app::layout_scene),
            ("composer", app::composer_scene),
            ("subagents", app::subagents_scene),
            ("rich-chat", app::rich_chat_scene),
            ("rich-replay", app::rich_replay_scene),
            ("safe-images", app::safe_images_scene),
            ("background-processes", app::background_scene),
            ("models", app::models_scene),
            ("reasoning", app::reasoning_scene),
            ("permissions", app::permissions_scene),
            ("voice", app::voice_scene),
            ("voice-chat", app::voice_chat_scene),
            ("voice-engine", app::voice_engine_scene),
            ("empty", app::empty_scene),
            ("error", app::error_scene),
        ],
        min_size: Some((1024.0, 520.0)),
        ..AppSpec::new("Hermes", (1440.0, 900.0), Viewer::new, view::app)
    };
    let application = rune::App::new(spec)
        .init(move || match launch {
            Some(boot) => Viewer::from_window_bootstrap(boot),
            None => Viewer::new(),
        })
        .on_ime(view::terminal::on_ime)
        .on_focus(app::on_focus)
        .chrome(Chrome {
            full_size_content: true,
            traffic_lights: Some((16.0, 16.0)),
            drag_regions: true,
        });
    let handle = application.handle();
    let ready = handle.clone();
    application
        .on_frame_drawn(app::windows::inspect_native_frame)
        .on_close_requested(app::windows::may_close)
        .on_close(app::windows::closed)
        .on_ready(move |window| {
            let native = window.clone();
            let app_handle = ready.clone();
            handle.update(move |s, tx| {
                s.window_ready(app_handle, native);
                tx.instant();
            });
        })
}
