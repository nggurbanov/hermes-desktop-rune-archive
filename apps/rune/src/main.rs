mod app;
mod audio;
mod chat;
mod client;
mod client_settings;
mod client_status;
mod composer;
mod connections;
mod conversation;
mod decks;
mod directories;
mod files;
mod gateway;
mod identities;
mod layout;
mod media;
mod native_fs;
mod notifications;
mod oauth;
mod operations;
mod pages;
mod plugin_host;
mod preview;
mod review;
mod sidebar;
mod subagents;
mod terminal;
mod view;
mod voice;

use app::Viewer;
use rune::{AppSpec, Chrome};

fn main() {
    if preview::run_if_proof_mode() {
        return;
    }
    if subagents::run_if_window_mode() {
        return;
    }
    if plugin_host::run_if_host_mode() {
        return;
    }
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
        .on_ime(view::terminal::on_ime)
        .on_focus(app::on_focus)
        .chrome(Chrome {
            full_size_content: true,
            traffic_lights: Some((16.0, 16.0)),
            drag_regions: true,
        });
    let handle = application.handle();
    let native_window = handle.clone();
    application
        .on_ready(move |_| {
            let native_window = native_window.clone();
            handle.update(move |s, tx| {
                s.native_window = Some(native_window);
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
    let mut workspace_reads = serde_json::Map::new();
    if args.iter().any(|arg| arg == "--check-pages") {
        let profile = value("--profile").unwrap_or_else(|| "default".into());
        for (name, path) in [
            ("settings_schema", "/api/config/schema"),
            ("settings_config", "/api/config"),
            ("capabilities_toolsets", "/api/tools/toolsets"),
            ("messaging_platforms", "/api/messaging/platforms"),
            ("scheduled_jobs", "/api/cron/jobs"),
        ] {
            let response = gateway.page_request(&profile, &pages::PageRequest::get(path));
            workspace_reads.insert(name.into(),serde_json::json!({"ok":response.is_ok(),"items":response.as_ref().ok().and_then(|v|v.get("fields").or(v.get("platforms")).or(v.get("jobs")).or(v.get("toolsets"))).map(|v|v.as_array().map(Vec::len).or_else(||v.as_object().map(serde_json::Map::len)))}));
        }
    }
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
        "gpt_live_available": voice_status.as_ref().map(|s| s.live_available), "workspace_reads":workspace_reads })
    );
    Ok(())
}
