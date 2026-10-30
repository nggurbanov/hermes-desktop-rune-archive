mod app;
mod audio;
mod background;
mod chat;
mod client;
mod client_settings;
mod client_status;
mod composer;
mod connections;
mod conversation;
mod decks;
mod desktop_window;
mod directories;
mod files;
mod gateway;
mod identities;
mod image_previews;
mod layout;
mod media;
mod media_player;
mod media_preview_proof;
mod native_fs;
mod notifications;
mod oauth;
mod operations;
mod pages;
mod plugin_host;
mod preview;
mod review;
mod sidebar;
mod sounds;
mod subagents;
mod terminal;
mod view;
mod voice;

fn main() {
    if media_preview_proof::run_if_proof_mode() {
        return;
    }
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
    desktop_window::application(None).run();
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
