//! Comfortable client chrome; item preferences do not mutate the agent.
use crate::app::Viewer;
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, Kit};
const ITEMS: &[(&str, &str)] = &[
    ("health", "Backend health"),
    ("workspace", "Workspace"),
    ("approval", "Approval policy"),
    ("terminal", "Terminal"),
    ("cron", "Scheduled jobs"),
    ("webhooks", "Webhooks"),
    ("turn-timer", "Running turn timer"),
    ("session-timer", "Focused session timer"),
    ("context", "Context usage"),
    ("cache", "Prompt cache"),
    ("speed", "Tokens per second"),
];
fn button(label: impl Into<String>, f: impl Fn(&mut Viewer) + 'static, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .on_press(f)
        .build(k)
}
fn duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}
pub fn bar(s: &Viewer, k: &Kit) -> El {
    let mut bar = row()
        .key("status-bar")
        .role("status")
        .label(&s.status)
        .w_full()
        .h(28.)
        .px(8.)
        .gap(5.)
        .center_y()
        .fill(k.c.bg_deep)
        .scroll_x()
        .on_secondary_press(|s: &mut Viewer, _, _| s.chrome.menu = true);
    bar = bar
        .child(
            button("⌘", |s| s.open_route(crate::pages::Route::CommandCenter), k)
                .key("command-center"),
        )
        .child(button(&s.endpoint_label, |s| s.change_connection(), k).key("change-connection"));
    if !super::fleet::rail_visible(s) {
        bar = bar.child(
            button(&s.profile, |s| s.profile_picker = !s.profile_picker, k).key("status-profile"),
        );
    }
    bar = bar.child(button("Reconnect", |s| s.reconnect(), k).key("reconnect"));
    if s.status_item_visible("health") {
        bar = bar.child(
            button(
                match s.operations.ready() {
                    Some(true) => "Backend ready",
                    Some(false) => "Agent needs setup",
                    None => "Backend health",
                },
                |s| s.toggle_health(),
                k,
            )
            .key("backend-health"),
        );
    }
    if s.status_item_visible("workspace") {
        if let Some(cwd) = s.workspace_cwd() {
            let label = std::path::Path::new(&cwd)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&cwd)
                .to_owned();
            bar = bar.child(
                button(
                    format!("⌑ {label}"),
                    |s| {
                        s.workspace_menu = !s.workspace_menu;
                        s.chrome.detail = None;
                        s.chrome.menu = false;
                    },
                    k,
                )
                .help(cwd)
                .key("workspace-menu"),
            );
        }
    }
    if s.status_item_visible("cron") {
        bar = bar.child(button(
            "Jobs",
            |s| s.open_route(crate::pages::Route::Cron),
            k,
        ));
    }
    if s.status_item_visible("webhooks") {
        bar = bar.child(button(
            "Webhooks",
            |s| {
                s.open_route(crate::pages::Route::Messaging);
                s.messaging_action(crate::pages::messaging::Action::Tab(2));
            },
            k,
        ));
    }
    let scope = s.composer_scope();
    let view = s.conversation.view(&scope);
    let usage = view
        .map(|v| {
            v.metadata
                .get("usage")
                .or(v.metadata["info"].get("usage"))
                .or(v.metadata["status"].get("usage"))
                .cloned()
                .unwrap_or_default()
        })
        .unwrap_or_default();
    for (id, label) in [
        (
            "context",
            usage["context_percent"]
                .as_f64()
                .map(|n| format!("Context {n:.0}%"))
                .unwrap_or("Context —".into()),
        ),
        (
            "cache",
            usage["cache_hit_pct"]
                .as_f64()
                .map(|n| format!("Cache {n:.0}%"))
                .unwrap_or("Cache —".into()),
        ),
        (
            "speed",
            usage["avg_tps"]
                .as_f64()
                .map(|n| format!("{n:.1} tok/s"))
                .unwrap_or("Speed —".into()),
        ),
    ] {
        if s.status_item_visible(id) {
            let id = id.to_owned();
            bar = bar.child(button(label, move |s| s.open_chrome_detail(id.clone()), k));
        }
    }
    if s.status_item_visible("turn-timer") {
        if let Some(secs) = s.chrome.elapsed(true) {
            bar = bar.child(
                text(format!("Working {}", duration(secs)))
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            );
        }
    }
    if s.status_item_visible("session-timer") {
        if let Some(secs) = s.chrome.elapsed(false) {
            bar = bar.child(text(duration(secs)).font(k.ty.small).color(k.c.text_muted));
        }
    }
    bar = bar.child(el().grow(1.));
    if s.status_item_visible("approval") {
        bar = bar.child(button(
            s.chrome
                .approval
                .as_deref()
                .map(|m| format!("Policy: {m}"))
                .unwrap_or("Policy —".into()),
            |s| s.open_chrome_detail("approval".into()),
            k,
        ));
    }
    if s.status_item_visible("terminal") {
        bar = bar.child(button(
            "Terminal",
            |s| {
                s.reveal_pane("terminal");
            },
            k,
        ));
    }
    if s.chrome.resources_enabled && s.status_item_visible("resources") {
        bar = bar.child(button(
            "Resources",
            |s| s.open_chrome_detail("resources".into()),
            k,
        ));
    }
    bar.child(
        text(format!("Hermes {}", env!("CARGO_PKG_VERSION")))
            .font(k.ty.small)
            .color(k.c.text_faint),
    )
    .child(button("…", |s| s.chrome.menu = !s.chrome.menu, k).key("status-customize"))
}
pub fn overlays(s: &Viewer, k: &Kit, layer: &mut Vec<El>) {
    if s.chrome.menu {
        let mut menu = popup("Customize statusbar", k);
        for (id, label) in ITEMS.iter().copied().chain(
            s.chrome
                .resources_enabled
                .then_some(("resources", "System resources")),
        ) {
            let key = id.to_owned();
            menu = menu.child(
                Checkbox::new(s.status_item_visible(id))
                    .label(label)
                    .on_toggle(move |s: &mut Viewer| {
                        if !s.hidden_status_items.remove(&key) {
                            s.hidden_status_items.insert(key.clone());
                        }
                    })
                    .build(k),
            );
        }
        menu = menu
            .child(button(
                "Reset statusbar layout",
                |s| s.hidden_status_items = crate::client::default_hidden_status(),
                k,
            ))
            .child(button(
                "Hide statusbar",
                |s| {
                    s.show_statusbar = false;
                    s.chrome.menu = false;
                },
                k,
            ))
            .child(button("Close", |s| s.chrome.menu = false, k));
        layer.push(menu);
    }
    if s.workspace_menu {
        if let Some(cwd) = s.workspace_cwd() {
            let scope = s.composer_scope();
            let view = s.conversation.view(&scope);
            let busy = view.is_some_and(|v| v.running || v.preparing || v.workspace_pending);
            let mut menu = popup("Working directory", k).child(
                text(&cwd)
                    .font(k.ty.small)
                    .color(k.c.text_base)
                    .wrap()
                    .selectable(true),
            );
            let copy = cwd.clone();
            menu = menu.child(
                Button::new("Copy full path")
                    .variant(ButtonVariant::GhostMuted)
                    .on_press_tx(move |_: &mut Viewer, tx| tx.copy(copy.clone()))
                    .build(k),
            );
            if s.is_local_connection() {
                let path = std::path::PathBuf::from(&cwd);
                if path.is_absolute() && path.is_dir() {
                    menu = menu.child(
                        Button::new("Reveal in file manager")
                            .variant(ButtonVariant::GhostMuted)
                            .on_press_tx(move |s: &mut Viewer, tx| {
                                if let Ok(url) = url::Url::from_directory_path(&path) {
                                    if tx.open_url(url.as_str()).is_err() {
                                        s.client_error =
                                            Some("The file manager could not be opened".into());
                                    }
                                }
                            })
                            .build(k),
                    );
                }
            }
            let root = cwd.clone();
            menu = menu.child(button(
                "Reveal in Files",
                move |s| {
                    s.reveal_pane("files");
                    s.files_action(crate::files::Action::Root(root.clone()));
                    s.workspace_menu = false;
                },
                k,
            ));
            let can_change = scope.session.is_none() || view.is_some_and(|v| !v.runtime.is_empty());
            menu = menu.child(
                Button::new("Change working directory…")
                    .variant(ButtonVariant::GhostMuted)
                    .disabled(busy || !can_change || s.is_fixture())
                    .on_press(|s: &mut Viewer| {
                        s.open_directory(crate::directories::Purpose::Workspace);
                        s.workspace_menu = false;
                    })
                    .build(k),
            );
            if let Some(path) = s.conversation.pending_cwd(&scope) {
                let owner = scope.clone();
                menu = menu
                    .child(
                        text(if view.is_some_and(|v| v.workspace_pending) {
                            "Changing directory…"
                        } else {
                            "The directory change needs reconciliation"
                        })
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap(),
                    )
                    .child(
                        Button::new("Retry pending directory change")
                            .variant(ButtonVariant::GhostMuted)
                            .disabled(busy)
                            .on_press(move |s: &mut Viewer| {
                                s.change_workspace(owner.clone(), path.clone())
                            })
                            .build(k),
                    );
            }
            menu = menu.child(button("Close", |s| s.workspace_menu = false, k));
            layer.push(menu);
        }
    }
    if s.profile_picker && !super::fleet::rail_visible(s) {
        layer.push(popup("Choose profile", k).child(super::fleet::picker(s, k, true)));
    }
    if let Some(detail) = &s.chrome.detail {
        let mut body = popup(detail, k);
        if let Some(error) = &s.chrome.error {
            body = body.child(text(error).font(k.ty.small).color(k.c.danger.fg).wrap());
        }
        if detail == "approval" {
            body = body.child(
                text("Profile-wide approval policy")
                    .font(k.ty.ui)
                    .color(k.c.text_base),
            );
            for (mode, label) in [
                ("manual", "Manual · Ask before actions"),
                ("smart", "Smart · Approve routine actions"),
                ("off", "Off · Allow actions without prompts"),
            ] {
                let mode = mode.to_owned();
                body = body.child(
                    Button::new(label)
                        .variant(ButtonVariant::GhostMuted)
                        .disabled(s.chrome.busy || s.is_fixture())
                        .on_press(move |s: &mut Viewer| s.chrome_approval(mode.clone()))
                        .build(k),
                );
            }
        } else {
            let scope = s.composer_scope();
            let usage = s
                .conversation
                .view(&scope)
                .and_then(|v| v.metadata.get("usage").or(v.metadata["info"].get("usage")));
            let value = match detail.as_str() {
                "context" => s.chrome.context.as_ref().or(usage),
                "resources" => s.chrome.hardware.as_ref(),
                _ => usage,
            };
            body = body.children(measurements(value, detail, k));
        }
        body = body.child(button("Close", |s| s.chrome.detail = None, k));
        layer.push(body);
    }
}
fn popup(title: &str, k: &Kit) -> El {
    let height = rune::memo::layout("hermes-window")
        .map(|r| (r[3] - 80.).clamp(160., 610.))
        .unwrap_or(440.);
    col()
        .pin(None, None, Some(12.), Some(40.))
        .w(320.)
        .max_h(height)
        .padding(16.)
        .gap(10.)
        .fill(k.c.bg_deep)
        .radius(10.)
        .border(1., k.c.border_muted)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
}

fn measurements(value: Option<&serde_json::Value>, detail: &str, k: &Kit) -> Vec<El> {
    let Some(value) = value else {
        return vec![
            text("Waiting for measurements from this conversation")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
        ];
    };
    let mut rows = vec![];
    let fields: &[(&str, &str)] = if detail == "resources" {
        &[
            ("gpu_name", "GPU"),
            ("gpu_util_percent", "GPU utilization"),
            ("vram_used_bytes", "VRAM used"),
            ("vram_total_bytes", "VRAM total"),
            ("ram_total_bytes", "RAM total"),
            ("ram_available_bytes", "RAM available"),
        ]
    } else {
        &[
            ("context_used", "Context tokens"),
            ("context_max", "Context limit"),
            ("context_percent", "Context used"),
            ("compressions", "Compressions"),
            ("calls", "Model calls"),
            ("input", "Input tokens"),
            ("output", "Output tokens"),
            ("total", "Total tokens"),
            ("cache_hit_pct", "Prompt cache hit"),
            ("avg_tps", "Tokens per second"),
            ("cost_usd", "Cost, USD"),
        ]
    };
    for (key, label) in fields {
        let v = &value[*key];
        if v.is_null() {
            continue;
        }
        let formatted = if key.ends_with("_bytes") {
            v.as_f64()
                .map(|n| format!("{:.1} GiB", n / 1073741824.))
                .unwrap_or("—".into())
        } else if key.ends_with("percent") || key.ends_with("_pct") {
            v.as_f64().map(|n| format!("{n:.1}%")).unwrap_or("—".into())
        } else if let Some(v) = v.as_str() {
            v.to_owned()
        } else if let Some(n) = v.as_u64() {
            n.to_string()
        } else {
            v.as_f64().map(|n| format!("{n:.2}")).unwrap_or("—".into())
        };
        rows.push(metric(label, &formatted, k));
    }
    for category in value["categories"].as_array().into_iter().flatten() {
        if let Some(label) = category["label"].as_str().or(category["name"].as_str()) {
            let tokens = category["tokens"]
                .as_u64()
                .map(|n| n.to_string())
                .unwrap_or("—".into());
            rows.push(metric(label, &format!("{tokens} tokens"), k));
        }
    }
    if value["uma"] == true {
        rows.push(
            text("This device uses unified memory.")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
        );
    }
    if rows.is_empty() {
        rows.push(
            text("Measurements are not available for this conversation yet")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
        );
    }
    rows
}
fn metric(label: &str, value: &str, k: &Kit) -> El {
    row()
        .w_full()
        .gap(12.)
        .child(
            text(label)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap()
                .grow(1.),
        )
        .child(
            text(value)
                .font(k.ty.small_medium)
                .color(k.c.text_base)
                .wrap(),
        )
}
