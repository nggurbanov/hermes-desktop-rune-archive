mod app;
mod gateway;
mod view;

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
    let profiles = gateway.profiles()?;
    let profile = value("--profile").unwrap_or_else(|| "default".into());
    let sessions = gateway.sessions(&profile)?;
    let history = sessions
        .first()
        .map(|s| gateway.history(&profile, &s.id))
        .transpose()?;
    println!(
        "{}",
        serde_json::json!({ "connected": true, "profiles": profiles.len(),
        "sessions": sessions.len(), "history_messages": history.as_ref().map(|h| h.messages.len()),
        "earlier_history": history.as_ref().map(|h| h.has_more), "read_only": true })
    );
    Ok(())
}
