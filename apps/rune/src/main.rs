mod app;
mod composer;
mod gateway;
mod view;
mod voice;

use app::Viewer;
use rune::{AppSpec, Chrome};

fn main() {
    if std::env::args().any(|arg| arg == "--check-connection") {
        if let Err(error) = check_connection() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let spec = AppSpec {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(app::setup),
        poll: Some(app::poll),
        on_key: Some(app::on_key),
        scenes: &[
            ("reader", app::reader_scene),
            ("bots", app::bots_scene),
            ("layout", app::layout_scene),
            ("composer", app::composer_scene),
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
        ..AppSpec::new(
            "Hermes Desktop Rune",
            (1440.0, 900.0),
            Viewer::new,
            view::app,
        )
    };
    let application = rune::App::new(spec).chrome(Chrome {
        full_size_content: true,
        traffic_lights: Some((16.0, 16.0)),
        drag_regions: true,
    });
    let handle = application.handle();
    application
        .on_ready(move |_| {
            handle.update(|s, tx| {
                s.begin();
                tx.instant();
            })
        })
        .run();
}

/// A read-only acceptance probe. Never prints endpoint credentials or conversation data.
fn check_connection() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let value = |name: &str| {
        args.iter()
            .position(|s| s == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let connection = if let Some(home) = value("--local-home") {
        gateway::Connection::Local { home: home.into() }
    } else if let Some(endpoint) = value("--gateway") {
        gateway::Connection::Url {
            endpoint,
            token: std::env::var("HERMES_GATEWAY_TOKEN").ok(),
        }
    } else {
        return Err("Choose --local-home PATH or --gateway URL".into());
    };
    let mut gateway = gateway::Gateway::connect(&connection)?;
    let model_counts = if args.iter().any(|arg| arg == "--check-models") {
        let profile = value("--profile").unwrap_or_else(|| "default".into());
        let catalog = gateway.model_catalog(&profile)?;
        Some((
            catalog.providers.len(),
            catalog
                .providers
                .iter()
                .map(|p| p.models.len())
                .sum::<usize>(),
        ))
    } else {
        None
    };
    let voice_status = if args.iter().any(|arg| arg == "--check-voice") {
        let profile = value("--profile").unwrap_or_else(|| "default".into());
        gateway.voice_status(&profile)?
    } else {
        None
    };
    let profiles = gateway.profiles()?;
    let profile = value("--profile").unwrap_or_else(|| "default".into());
    let sessions = gateway.sessions(&profile)?;
    let check_bot = args.iter().any(|arg| arg == "--check-bot");
    let history = if check_bot {
        let chat = profiles
            .iter()
            .find(|p| p.name == profile)
            .and_then(|p| p.canonical_session.as_ref())
            .ok_or("No existing Bot Chat for the selected profile")?;
        Some(gateway.history(&profile, &chat.id)?)
    } else {
        sessions
            .first()
            .map(|s| gateway.history(&profile, &s.id))
            .transpose()?
    };
    println!(
        "{}",
        serde_json::json!({ "connected": true, "profiles": profiles.len(),
        "sessions": sessions.len(), "history_messages": history.as_ref().map(|h| h.messages.len()),
        "earlier_history": history.as_ref().map(|h| h.has_more), "read_only": true,
        "canonical_bot_history": check_bot,
        "model_providers": model_counts.map(|counts| counts.0),
        "models": model_counts.map(|counts| counts.1),
        "voice_chat_engine": voice_status.as_ref().map(|s| s.engine.label()),
        "gpt_live_available": voice_status.as_ref().map(|s| s.live_available) })
    );
    Ok(())
}
