//! Command Center keeps Hermes' four sections and native OpenCode controls.
mod files;
use crate::{
    app::Viewer,
    operations::{Action, Confirmation, Kind},
};
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, TextInput};
use serde_json::Value;
pub(super) fn owns(s: &Viewer, owner: &crate::composer::DraftScope) -> bool {
    let now = s.composer_scope();
    now.connection == owner.connection && now.profile == owner.profile
}
pub(super) fn action(s: &Viewer, label: &str, a: Action, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let local = matches!(
        a,
        Action::Section(_)
            | Action::LogFile(_)
            | Action::LogLevel(_)
            | Action::UsagePeriod(_)
            | Action::FollowLogs
            | Action::CancelConfirm
    );
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.operations.demo && !local)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if owns(s, &owner) {
                s.operations_action(a.clone());
                tx.instant();
            }
        })
        .build(k)
}
fn heading(value: &str, k: &Kit) -> El {
    text(value)
        .font(k.ty.body_medium)
        .color(k.c.text_base)
        .pt(10.0)
}
fn note(value: impl Into<String>, k: &Kit) -> El {
    text(value.into())
        .font(k.ty.small)
        .color(k.c.text_muted)
        .wrap()
}
fn issue(value: &Option<String>, k: &Kit) -> Vec<El> {
    value
        .iter()
        .map(|v| {
            text(v)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap()
        })
        .collect()
}
fn metric(label: &str, value: impl Into<String>, k: &Kit) -> El {
    row()
        .w_full()
        .gap(20.0)
        .py(6.0)
        .child(note(label, k).grow(1.0))
        .child(
            text(value.into())
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .wrap(),
        )
}
fn value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => "Unavailable".into(),
    }
}
fn count(v: &Value) -> String {
    let Some(n) = v.as_f64() else {
        return "Unavailable".into();
    };
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 10_000.0 {
        format!("{:.1}K", n / 1000.0)
    } else {
        format!("{n:.0}")
    }
}
pub fn page(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let o = &s.operations;
    let mut nav = col()
        .w(180.0)
        .min_w(150.0)
        .px(12.0)
        .py(16.0)
        .gap(4.0)
        .child(heading("Command Center", k));
    for (id, label) in [
        ("sessions", "Sessions"),
        ("system", "System"),
        ("usage", "Usage"),
        ("maintenance", "Maintenance"),
    ] {
        nav = nav.child(
            action(s, label, Action::Section(id.into()), false, k)
                .key(format!("command-center-section-{id}"))
                .w_full()
                .justify_start()
                .fill(if o.section == id {
                    k.c.layer_01
                } else {
                    rune::Color::TRANSPARENT
                }),
        );
    }
    let mut body = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .px(24.0)
        .py(16.0)
        .gap(12.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            row()
                .w_full()
                .center_y()
                .child(
                    heading(
                        match o.section.as_str() {
                            "system" => "System",
                            "usage" => "Usage",
                            "maintenance" => "Maintenance",
                            _ => "Sessions",
                        },
                        k,
                    )
                    .grow(1.0),
                )
                .child(action(s, "Refresh", Action::Refresh, false, k)),
        );
    if o.demo {
        body = body.child(note("Demonstration data · live operations are disabled", k));
    }
    if let Some(n) = &o.notice {
        body = body.child(note(n, k));
    }
    body = body.children(issue(&o.export_error, k));
    let children = match o.section.as_str() {
        "system" => system(s, k, layer),
        "usage" => usage(s, k),
        "maintenance" => maintenance(s, k),
        _ => sessions(s, k),
    };
    body = body.children(children);
    if let Some(confirm) = &o.confirm {
        body = body.child(confirmation(s, confirm, k));
    }
    row()
        .key("command-center")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(nav)
        .child(body)
}
fn sessions(s: &Viewer, k: &Kit) -> Vec<El> {
    let o = &s.operations;
    let owner = s.composer_scope();
    let mut out = vec![
        TextInput::new(&o.search)
            .placeholder("Search sessions by title or ID…")
            .width(None)
            .on_edit(move |s: &mut Viewer| {
                if !owns(s, &owner) {
                    return s.operations.edit_detached();
                }
                &mut s.operations.search
            })
            .build(k),
    ];
    if let Some(crate::sidebar::Confirmation::Session(mutation)) = &s.sidebar_state.confirm {
        let mut confirm = col()
            .w_full()
            .gap(8.0)
            .px(12.0)
            .py(12.0)
            .fill(k.c.layer_01)
            .child(
                text(format!(
                    "Delete {} from profile {}?",
                    mutation.session.title(),
                    mutation.session.profile()
                ))
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .wrap(),
            );
        let mut buttons = row().gap(8.0);
        for (label, a) in [
            ("Delete", crate::sidebar::Action::Confirm),
            ("Cancel", crate::sidebar::Action::Cancel),
        ] {
            let owner = s.composer_scope();
            buttons = buttons.child(
                Button::new(label)
                    .variant(ButtonVariant::Ghost)
                    .disabled(o.demo || s.sidebar_state.mutation_inflight)
                    .on_press_tx(move |s: &mut Viewer, tx| {
                        if owns(s, &owner) {
                            s.sidebar_action(a.clone());
                            tx.instant();
                        }
                    })
                    .build(k),
            );
        }
        confirm = confirm.child(buttons);
        out.push(confirm);
    }
    let q = o.search.text.trim().to_lowercase();
    let mut rows = s.sidebar_state.all_rows();
    let mut seen = std::collections::BTreeSet::new();
    rows.retain(|r| {
        seen.insert(r.key())
            && (q.is_empty()
                || format!("{} {}", r.title(), r.id)
                    .to_lowercase()
                    .contains(&q))
    });
    rows.sort_by(|a, b| b.recency().total_cmp(&a.recency()));
    if rows.is_empty() {
        out.push(note(
            if s.sidebar_state.recents.loading {
                "Loading sessions…"
            } else {
                "No sessions match this search."
            },
            k,
        ));
    }
    for session in rows {
        let p = session.profile().to_owned();
        let id = session.id.clone();
        let unavailable = session.connection_id.is_some();
        let mut buttons = row()
            .gap(5.0)
            .child(action(
                s,
                "Open",
                Action::SessionOpen(p.clone(), id.clone()),
                unavailable,
                k,
            ))
            .child(action(
                s,
                if session.pinned == Some(true) {
                    "Unpin"
                } else {
                    "Pin"
                },
                Action::SessionPin(p.clone(), id.clone(), session.pinned != Some(true)),
                unavailable || s.sidebar_state.mutation_inflight,
                k,
            ))
            .child(files::export_session(s, session.clone(), k))
            .child(action(
                s,
                "Delete",
                Action::SessionDelete(p.clone(), id),
                unavailable || s.sidebar_state.mutation_inflight,
                k,
            ));
        buttons = buttons.min_w(0.0);
        out.push(
            col()
                .w_full()
                .gap(4.0)
                .py(10.0)
                .child(
                    text(session.title())
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .wrap(),
                )
                .child(note(
                    format!(
                        "{} · {} · {} messages · {} tokens · {}",
                        p,
                        session.source.as_deref().unwrap_or("session"),
                        session.message_count,
                        session.tokens(),
                        crate::sidebar::relative_age(session.recency(), s.sidebar_state.now)
                    ),
                    k,
                ))
                .child(note(&session.id, k).selectable(true))
                .child(buttons),
        );
    }
    for error in [
        s.sidebar_state.recents.error.as_ref(),
        s.sidebar_state.messaging.error.as_ref(),
        s.sidebar_state.cron_sessions.error.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        out.push(note(error, k).role("alert"));
    }
    let more = [
        &s.sidebar_state.recents,
        &s.sidebar_state.messaging,
        &s.sidebar_state.cron_sessions,
    ]
    .iter()
    .any(|slice| slice.truncated.values().any(|v| *v));
    if more && q.is_empty() {
        out.push(action(
            s,
            "Load more sessions",
            Action::SessionLoadMore,
            s.sidebar_state.recents.loading
                || s.sidebar_state.messaging.loading
                || s.sidebar_state.cron_sessions.loading,
            k,
        ));
    }
    if o.exporting {
        out.push(note(
            "Exporting the complete stored conversation, including compacted history…",
            k,
        ));
    }
    out
}
fn system(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let o = &s.operations;
    let mut out = issue(&o.system_error, k);
    out.push(health(s, k, layer));
    if let Some(status) = &o.status {
        out.push(metric("Agent version", value(&status["version"]), k));
        out.push(metric(
            "Active sessions",
            count(&status["active_sessions"]),
            k,
        ));
        out.push(
            note(format!("Agent home: {}", value(&status["hermes_home"])), k).selectable(true),
        );
    }
    out.push(
        row()
            .gap(8.0)
            .child(action(
                s,
                "Check agent updates",
                Action::CheckUpdate,
                o.update_loading,
                k,
            ))
            .child(action(
                s,
                "Update external Hermes agent",
                Action::Start(Kind::Update),
                o.starting
                    || o.task.as_ref().is_some_and(|t| t.running)
                    || o.update.as_ref().is_some_and(|u| u["can_apply"] == false),
                k,
            )),
    );
    if let Some(update) = &o.update {
        if let Some(message) = update["message"].as_str() {
            out.push(note(message, k));
        }
        out.push(note(
            format!(
                "Installation: {} · Version: {} · Update: {}",
                value(&update["install_method"]),
                value(&update["current_version"]),
                if update["update_available"] == true {
                    "available"
                } else {
                    "not reported"
                }
            ),
            k,
        ));
        for commit in update["commits"].as_array().into_iter().flatten().take(12) {
            out.push(note(
                format!(
                    "{} {}",
                    commit["sha"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(8)
                        .collect::<String>(),
                    value(&commit["summary"])
                ),
                k,
            ));
        }
    }
    out.push(heading("Logs", k));
    out.push(log_filters(s, k, layer));
    out.extend(issue(&o.logs_error, k));
    out.push(
        TextInput::new(&o.log_search)
            .placeholder("Search within the log tail…")
            .width(None)
            .on_edit(|s: &mut Viewer| &mut s.operations.log_search)
            .build(k),
    );
    out.push(
        row()
            .gap(8.0)
            .child(action(
                s,
                if o.log_follow {
                    "Pause live tail"
                } else {
                    "Resume live tail"
                },
                Action::FollowLogs,
                false,
                k,
            ))
            .child(files::save_logs(s, k)),
    );
    let logs = o.visible_logs();
    out.push(note(
        format!(
            "{} matching lines · {}",
            logs.len(),
            if o.logs_loading {
                "refreshing"
            } else if o.log_follow {
                "live tail"
            } else {
                "paused"
            }
        ),
        k,
    ));
    out.push(
        text(if logs.is_empty() {
            "No log lines in the selected tail.".into()
        } else {
            logs.join("\n")
        })
        .font(k.ty.small)
        .color(k.c.text_base)
        .wrap()
        .selectable(true)
        .w_full()
        .max_h(380.0)
        .scroll(ScrollSpec {
            stick_bottom: o.log_follow,
        })
        .key("command-center-logs"),
    );
    if let Some(task) = &o.task {
        out.push(task_view(s, task, k));
    }
    out
}
fn log_filters(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let o = &s.operations;
    let mut line = row().gap(12.0);
    for (kind, choices, current) in [
        (
            "file",
            vec!["agent", "errors", "gateway", "desktop"],
            o.log_file.as_str(),
        ),
        (
            "level",
            vec!["ALL", "INFO", "WARNING", "ERROR"],
            o.log_level.as_str(),
        ),
    ] {
        let choices = choices.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let owner = s.composer_scope();
        line = line.child(
            Select::new(format!("ops-log-{kind}"), &choices)
                .current(Some(current))
                .width(150.0)
                .open(o.select.as_deref() == Some(kind))
                .on_toggle(move |s: &mut Viewer| {
                    s.operations.select = if s.operations.select.as_deref() == Some(kind) {
                        None
                    } else {
                        Some(kind.into())
                    }
                })
                .on_close(|s: &mut Viewer| s.operations.select = None)
                .on_choose(move |s: &mut Viewer, value, tx| {
                    if owns(s, &owner) {
                        s.operations_action(if kind == "file" {
                            Action::LogFile(value.into())
                        } else {
                            Action::LogLevel(value.into())
                        });
                        tx.instant();
                    }
                })
                .build(k, layer),
        );
    }
    line
}
pub fn health(s: &Viewer, k: &Kit, _layer: &mut Vec<El>) -> El {
    let o = &s.operations;
    let mut pane = col()
        .w_full()
        .gap(7.0)
        .child(heading("Gateway health", k))
        .child(metric("Transport", s.status.clone(), k))
        .child(metric(
            "Inference",
            match o.ready() {
                Some(true) => "Ready",
                Some(false) => "Needs setup",
                None => "Checking / unavailable",
            },
            k,
        ));
    if o.ready() != Some(true) {
        pane = pane.child(note(o.readiness_reason(), k));
    }
    if let Some(status) = &o.status {
        pane = pane.child(metric(
            "Messaging gateway",
            status["gateway_state"]
                .as_str()
                .unwrap_or(if status["gateway_running"] == true {
                    "Running"
                } else {
                    "Stopped"
                }),
            k,
        ));
        if let Some(reason) = status["gateway_exit_reason"].as_str() {
            pane = pane.child(note(reason, k));
        }
        if let Some(age) = status["gateway_heartbeat_stale_s"].as_u64() {
            pane = pane.child(
                note(
                    format!("The gateway heartbeat is stale by {age} seconds"),
                    k,
                )
                .role("alert"),
            );
        }
        for (name, state) in status["gateway_platforms"]
            .as_object()
            .into_iter()
            .flatten()
        {
            pane = pane.child(metric(
                name,
                state["state"].as_str().unwrap_or("Unknown"),
                k,
            ));
            if let Some(e) = state["error_message"].as_str() {
                pane = pane.child(note(e, k));
            }
        }
    }
    if !o.health_logs.is_empty() {
        pane = pane.child(heading("Recent gateway activity", k));
        for line in o.health_logs.iter().rev().take(8).rev() {
            pane = pane.child(note(line, k).selectable(true));
        }
    }
    pane = pane.children(issue(&o.health_logs_error, k));
    pane.child(
        row()
            .gap(8.0)
            .child(action(s, "Reconnect", Action::Reconnect, s.loading, k))
            .child(action(
                s,
                "Open System / all logs",
                Action::OpenSystem,
                s.route == crate::pages::Route::CommandCenter && o.section == "system",
                k,
            )),
    )
    .child(
        row()
            .gap(8.0)
            .child(action(s, "Configure model", Action::OpenSettings, false, k))
            .child(action(
                s,
                "Restart gateway",
                Action::Start(Kind::Restart),
                o.starting || o.task.as_ref().is_some_and(|t| t.running),
                k,
            )),
    )
}
fn usage(s: &Viewer, k: &Kit) -> Vec<El> {
    let o = &s.operations;
    let mut controls = row().gap(8.0);
    for days in [7, 30, 90] {
        controls = controls.child(
            action(
                s,
                &format!("{days} days"),
                Action::UsagePeriod(days),
                o.usage_loading && o.period == days,
                k,
            )
            .fill(if o.period == days {
                k.c.layer_01
            } else {
                rune::Color::TRANSPARENT
            }),
        );
    }
    let mut out = vec![controls];
    out.extend(issue(&o.usage_error, k));
    let Some(v) = &o.usage else {
        out.push(note(
            if o.usage_loading {
                "Loading actual usage…"
            } else {
                "Usage analytics are unavailable. Refresh to try again."
            },
            k,
        ));
        return out;
    };
    for (key, label) in [
        ("total_sessions", "Sessions"),
        ("total_api_calls", "API calls"),
        ("total_input", "Input tokens"),
        ("total_output", "Output tokens"),
        ("total_reasoning", "Reasoning tokens"),
        ("total_cache_read", "Cached input tokens"),
    ] {
        out.push(metric(label, count(&v["totals"][key]), k));
    }
    if let Some(cost) = v["totals"]["total_actual_cost"].as_f64() {
        out.push(metric("Recorded cost", format!("${cost:.4}"), k));
    }
    out.push(heading("Daily token usage", k));
    let days = v["daily"].as_array().cloned().unwrap_or_default();
    let maximum = days
        .iter()
        .map(|d| {
            d["input_tokens"].as_f64().unwrap_or(0.0) + d["output_tokens"].as_f64().unwrap_or(0.0)
        })
        .fold(1.0f64, f64::max);
    for day in &days {
        let input = day["input_tokens"].as_f64().unwrap_or(0.0);
        let output = day["output_tokens"].as_f64().unwrap_or(0.0);
        out.push(
            row()
                .gap(12.0)
                .center_y()
                .child(note(value(&day["day"]), k).w(85.0))
                .child(
                    row()
                        .w(220.0)
                        .h(9.0)
                        .child(
                            el().w((input / maximum * 220.0) as f32)
                                .h(9.0)
                                .fill(k.c.bg_accent),
                        )
                        .child(
                            el().w((output / maximum * 220.0) as f32)
                                .h(9.0)
                                .fill(k.c.text_muted),
                        ),
                )
                .child(note(
                    format!(
                        "{} in / {} out",
                        count(&day["input_tokens"]),
                        count(&day["output_tokens"])
                    ),
                    k,
                )),
        );
    }
    out.push(heading("By model", k));
    for model in v["by_model"].as_array().into_iter().flatten() {
        out.push(
            col()
                .gap(3.0)
                .py(6.0)
                .child(
                    text(value(&model["model"]))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                )
                .child(note(
                    format!(
                        "{} sessions · {} calls · {} input · {} output",
                        count(&model["sessions"]),
                        count(&model["api_calls"]),
                        count(&model["input_tokens"]),
                        count(&model["output_tokens"])
                    ),
                    k,
                )),
        );
    }
    out.push(heading("Top skills", k));
    if let Some(summary) = v["skills"]["summary"].as_object() {
        out.push(note(
            format!(
                "{} distinct skills · {} actions · {} loads · {} edits",
                count(
                    &summary
                        .get("distinct_skills_used")
                        .cloned()
                        .unwrap_or(Value::Null)
                ),
                count(&v["skills"]["summary"]["total_skill_actions"]),
                count(&v["skills"]["summary"]["total_skill_loads"]),
                count(&v["skills"]["summary"]["total_skill_edits"])
            ),
            k,
        ));
    }
    for skill in v["skills"]["top_skills"].as_array().into_iter().flatten() {
        out.push(metric(
            &value(&skill["skill"]),
            count(&skill["total_count"]),
            k,
        ));
    }
    out
}
fn maintenance(s: &Viewer, k: &Kit) -> Vec<El> {
    let o = &s.operations;
    let mut out = issue(&o.maintenance_error, k);
    let running = o.starting || o.task.as_ref().is_some_and(|t| t.running);
    for (kind, detail) in [
        (
            Kind::Doctor,
            "Check the connected agent's installation and runtime.",
        ),
        (
            Kind::Security,
            "Audit the connected agent's security configuration.",
        ),
        (Kind::Backup, "Create a profile backup on the agent's host."),
    ] {
        out.push(
            row()
                .w_full()
                .gap(16.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .gap(4.0)
                        .child(text(kind.label()).font(k.ty.ui_medium).color(k.c.text_base))
                        .child(note(detail, k)),
                )
                .child(action(s, "Run", Action::Start(kind), running, k)),
        );
    }
    out.push(row().w_full().gap(16.0).center_y().child(col().grow(1.0).gap(4.0).child(text("Share diagnostics").font(k.ty.ui_medium).color(k.c.text_base)).child(note("Upload redacted diagnostic reports to the agent's sharing service and receive share links.",k))).child(action(s,if o.sharing{"Uploading…"}else{"Create share links"},Action::DebugShare,o.sharing,k)));
    if let Some(share) = &o.share {
        if let Some(ttl) = share["auto_delete_seconds"].as_u64() {
            out.push(note(
                format!("Shared reports expire after {ttl} seconds"),
                k,
            ));
        }
        out.push(note(if share["redacted"]==true{"The agent reports the diagnostics were redacted."}else{"The sharing result does not confirm redaction. Review the report before sharing its links."},k));
        for (name, url) in share["urls"].as_object().into_iter().flatten() {
            if let Some(url) = url.as_str() {
                out.push(link_row(name, url, k));
            }
        }
        for (name, error) in share["failures"].as_object().into_iter().flatten() {
            out.push(note(format!("{name}: {}", value(error)), k).role("alert"));
        }
    }
    if let Some(task) = &o.task {
        out.push(task_view(s, task, k));
    }
    out.push(heading("Skill curator", k));
    if let Some(c) = &o.curator {
        out.push(metric(
            "State",
            if c["enabled"] != true {
                "Disabled"
            } else if c["paused"] == true {
                "Paused"
            } else {
                "Active"
            },
            k,
        ));
        out.push(metric(
            "Last run",
            c["last_run_at"].as_str().unwrap_or("Never reported"),
            k,
        ));
        out.push(metric("Interval in hours", value(&c["interval_hours"]), k));
        let mut buttons = row().gap(8.0).child(action(
            s,
            "Run now",
            Action::Start(Kind::Curator),
            running,
            k,
        ));
        if c["enabled"] == true {
            buttons = buttons.child(action(
                s,
                if c["paused"] == true {
                    "Resume"
                } else {
                    "Pause"
                },
                Action::ToggleCurator,
                o.curator_busy,
                k,
            ));
        }
        out.push(buttons);
    } else {
        out.push(note(
            if o.maintenance_loading {
                "Loading curator status…"
            } else {
                "Curator status is unavailable. Refresh to retry."
            },
            k,
        ));
    }
    out.push(heading("Memory layers", k));
    if let Some(memory) = &o.memory {
        out.push(metric(
            "Active provider",
            memory["active"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("Built-in memory"),
            k,
        ));
        for (key, label, target) in [
            ("memory", "MEMORY.md", "memory"),
            ("user", "USER.md", "user"),
        ] {
            out.push(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(
                        note(
                            format!("{label} · {} bytes", count(&memory["builtin_files"][key])),
                            k,
                        )
                        .grow(1.0),
                    )
                    .child(action(
                        s,
                        "Reset",
                        Action::ResetMemory(target.into()),
                        o.memory_busy,
                        k,
                    )),
            );
        }
        for provider in memory["providers"].as_array().into_iter().flatten() {
            out.push(metric(
                &value(&provider["name"]),
                value(&provider["status"]),
                k,
            ));
        }
        out.push(action(
            s,
            "Reset both memory files",
            Action::ResetMemory("all".into()),
            o.memory_busy,
            k,
        ));
    } else {
        out.push(note("Memory status is unavailable. Refresh to retry.", k));
    }
    out
}
fn task_view(s: &Viewer, t: &crate::operations::Task, k: &Kit) -> El {
    let mut panel = col()
        .w_full()
        .gap(7.0)
        .py(10.0)
        .child(heading(t.kind.label(), k))
        .child(note(
            if t.running {
                "Running on the agent"
            } else if t.recovered {
                "Gateway responded; previous action registry was replaced"
            } else if t.settled && t.exit_code == Some(0) {
                "Completed"
            } else if t.settled {
                "Finished with issues"
            } else {
                "Outcome not confirmed"
            },
            k,
        ));
    if let Some(e) = &t.error {
        panel = panel.child(note(e, k).role("alert"));
    }
    if let Some(receipt) = &t.receipt {
        for (field, label) in [
            ("outcome", "Update outcome"),
            ("post_version", "Resulting version"),
            ("post_sha", "Resulting revision"),
        ] {
            if receipt[field].is_string() {
                panel = panel.child(metric(label, value(&receipt[field]), k));
            }
        }
        for followup in receipt["followups"].as_array().into_iter().flatten() {
            panel = panel.child(note(
                format!(
                    "{}: {}",
                    value(&followup["step"]),
                    value(&followup["reason"])
                ),
                k,
            ));
        }
        if let Some(a) = receipt["user_action"].as_object() {
            panel = panel.child(note(
                format!(
                    "Required action: {} — {}",
                    value(a.get("step").unwrap_or(&Value::Null)),
                    value(a.get("reason").unwrap_or(&Value::Null))
                ),
                k,
            ));
        }
    }
    panel = panel.child(
        text(t.lines.join("\n"))
            .font(k.ty.small)
            .color(k.c.text_base)
            .wrap()
            .selectable(true)
            .w_full()
            .max_h(240.0)
            .scroll(ScrollSpec { stick_bottom: true }),
    );
    let mut buttons = row().gap(8.0).child(action(
        s,
        "Check action status",
        Action::PollAction,
        t.polling,
        k,
    ));
    if t.running {
        buttons = buttons.child(action(s, "Stop watching", Action::StopWatching, false, k));
    }
    if t.kind == Kind::Backup && t.settled && t.exit_code == Some(0) && t.archive.is_some() {
        buttons = buttons.child(files::download_backup(s, k));
    }
    panel.child(buttons)
}
fn link_row(name: &str, url: &str, k: &Kit) -> El {
    let copy = url.to_owned();
    let open = url.to_owned();
    row()
        .w_full()
        .gap(10.0)
        .center_y()
        .child(note(format!("{name}: {url}"), k).selectable(true).grow(1.0))
        .child(
            Button::new("Copy")
                .variant(ButtonVariant::Ghost)
                .on_press_tx(move |_: &mut Viewer, tx| tx.copy(copy.clone()))
                .build(k),
        )
        .child(
            Button::new("Open")
                .variant(ButtonVariant::Ghost)
                .disabled(
                    !url::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https")),
                )
                .on_press_tx(move |_: &mut Viewer, tx| {
                    let _ = tx.open_url(&open);
                })
                .build(k),
        )
}
fn confirmation(s: &Viewer, c: &Confirmation, k: &Kit) -> El {
    let description = match c {
        Confirmation::Memory(target) => format!(
            "Reset {} on profile {}? This permanently removes the selected built-in memory file contents from its owning agent.",
            match target.as_str() {
                "memory" => "MEMORY.md",
                "user" => "USER.md",
                _ => "MEMORY.md and USER.md",
            },
            s.profile
        ),
        Confirmation::Action(kind) => {
            let profiles = s
                .operations
                .status
                .as_ref()
                .and_then(|v| v["gateway_shared_with"].as_array())
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            format!(
                "{} affects the shared gateway serving {}. Active conversations and messaging connections may be interrupted.",
                kind.label(),
                profiles.join(", ")
            )
        }
    };
    col()
        .w_full()
        .gap(8.0)
        .px(16.0)
        .py(14.0)
        .fill(k.c.layer_01)
        .child(text(description).font(k.ty.ui).color(k.c.text_base).wrap())
        .child(
            row()
                .gap(8.0)
                .child(action(
                    s,
                    "Confirm",
                    Action::Confirm,
                    s.operations.starting || s.operations.memory_busy,
                    k,
                ))
                .child(action(s, "Cancel", Action::CancelConfirm, false, k)),
        )
}
