//! Subagent roster and native spectator content, using the same Rune theme.
mod transcript;
use crate::{app::Viewer, subagents::window::WindowState};
use rune::{Cx, El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit};

pub fn strip(state: &Viewer, kit: &Kit) -> El {
    let scope = state.composer_scope();
    let rows = state.subagents.rows(&scope);
    let mut content = col().key("subagent-status").w_full().px(16.0).gap(2.0);
    if let Some(error) = &state.subagents.error {
        content = content.child(
            row()
                .w_full()
                .gap(8.0)
                .child(
                    text(error)
                        .role("alert")
                        .font(kit.ty.small)
                        .color(kit.c.danger.fg)
                        .wrap()
                        .grow(1.0),
                )
                .child(
                    Button::new("Dismiss")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut Viewer| s.subagents.error = None)
                        .build(kit),
                ),
        );
    }
    if rows.is_empty() {
        return content;
    }
    let expanded = state.subagents.expanded.contains(&scope);
    let owner = scope.clone();
    content = content.child(
        Button::new(format!(
            "{} {}{}",
            if expanded { "⌄" } else { "›" },
            rows.len(),
            if rows.len() == 1 {
                " Subagent"
            } else {
                " Subagents"
            }
        ))
        .variant(ButtonVariant::GhostMuted)
        .on_press(move |s: &mut Viewer| {
            if !s.subagents.expanded.remove(&owner) {
                s.subagents.expanded.insert(owner.clone());
            }
        })
        .build(kit)
        .key("subagents-toggle")
        .justify_start(),
    );
    if expanded {
        let mut list = col()
            .key("subagent-roster")
            .w_full()
            .max_h(180.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            });
        for (index, child) in rows.iter().enumerate() {
            let id = child.id.clone();
            let owner = scope.clone();
            let title = child.goal.clone();
            let activity = if child.terminal() && !child.summary.is_empty() {
                child.summary.clone()
            } else {
                child.activity.clone()
            };
            let status = child.status.clone();
            list = list.child(
                Button::new(&title)
                    .variant(ButtonVariant::Ghost)
                    .label_with(move |font, ink| {
                        row()
                            .w_full()
                            .min_w(0.0)
                            .gap(10.0)
                            .center_y()
                            .child(
                                col()
                                    .min_w(0.0)
                                    .grow(1.0)
                                    .gap(2.0)
                                    .child(text(&title).font(font).color(ink).truncate())
                                    .child(
                                        text(&activity)
                                            .font(rune::TextStyle::new(11.0, 400.0))
                                            .color(ink)
                                            .truncate(),
                                    ),
                            )
                            .child(
                                text(&status)
                                    .font(rune::TextStyle::new(11.0, 400.0))
                                    .color(ink),
                            )
                            .child(text("↗").color(ink))
                    })
                    .disabled(child.session.is_none())
                    .help(if child.session.is_some() {
                        "Open this subagent in its own window"
                    } else {
                        "Waiting for the child session"
                    })
                    .on_press(move |s: &mut Viewer| {
                        if s.composer_scope() != owner {
                            return;
                        }
                        let Some(connection) = s.current_connection().cloned() else {
                            return;
                        };
                        s.subagents.error =
                            s.subagents.open(&connection, &owner, &id, s.theme).err();
                    })
                    .build(kit)
                    .key(format!("subagent-open-{index}")),
            );
        }
        content = content.child(list);
    }
    content
}

pub fn window(state: &WindowState, cx: &mut Cx<WindowState>) -> El {
    let kit = rune_theme_opencode::kit(state.theme.unwrap_or(cx.system_dark()));
    let stack = transcript::stack(state, kit);
    col()
        .key("subagent-window")
        .role("window")
        .label("Hermes subagent spectator")
        .w_full()
        .h_full()
        .fill(kit.c.bg_deep)
        .child(
            row()
                .key("subagent-titlebar")
                .h(48.0)
                .w_full()
                .px(18.0)
                .gap(10.0)
                .center_y()
                .window_drag()
                .child(text(&state.title).font(kit.ty.small).color(kit.c.text_base))
                .child(el().grow(1.0))
                .child(
                    Button::new("Close")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press_tx(|_: &mut WindowState, tx| tx.close_window())
                        .build(kit)
                        .key("subagent-close"),
                ),
        )
        .children(state.projection.error.as_ref().map(|error| {
            row()
                .key("subagent-error")
                .w_full()
                .px(18.0)
                .py(10.0)
                .gap(8.0)
                .child(
                    text(error)
                        .role("alert")
                        .font(kit.ty.small)
                        .color(kit.c.danger.fg)
                        .wrap()
                        .grow(1.0),
                )
                .child(
                    Button::new("Retry")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut WindowState| s.retry())
                        .build(kit)
                        .key("subagent-retry"),
                )
        }))
        .children(state.files.error.as_ref().map(|error| {
            row()
                .key("subagent-media-error")
                .w_full()
                .px(18.0)
                .py(8.0)
                .gap(8.0)
                .child(
                    text(error)
                        .role("alert")
                        .font(kit.ty.small)
                        .color(kit.c.danger.fg)
                        .wrap()
                        .grow(1.0),
                )
                .child(
                    Button::new("Dismiss")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut WindowState| s.files.error = None)
                        .build(kit),
                )
        }))
        .children(state.files.notice.as_ref().map(|notice| {
            text(notice)
                .key("subagent-media-notice")
                .font(kit.ty.small)
                .color(kit.c.text_muted)
                .px(18.0)
                .py(8.0)
        }))
        .children(state.files.busy().then(|| {
            text("Preparing file…")
                .key("subagent-media-busy")
                .font(kit.ty.small)
                .color(kit.c.text_muted)
                .px(18.0)
                .py(8.0)
        }))
        .child(
            col()
                .key("subagent-history")
                .role("log")
                .label("Subagent conversation history")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec { stick_bottom: true })
                .child(stack),
        )
        .child(
            row()
                .key("subagent-statusbar")
                .w_full()
                .min_h(32.0)
                .px(16.0)
                .gap(8.0)
                .center_y()
                .border(1.0, kit.c.border_muted)
                .child(
                    text(&state.profile)
                        .font(kit.ty.small)
                        .color(kit.c.text_muted)
                        .truncate()
                        .shrink(),
                )
                .child(el().grow(1.0))
                .child(
                    text(if state.projection.running {
                        "Working"
                    } else if state.projection.connected {
                        "Connected"
                    } else {
                        "Disconnected"
                    })
                    .font(kit.ty.small)
                    .color(kit.c.text_muted),
                )
                .child(text("Read only").font(kit.ty.small).color(kit.c.text_faint)),
        )
}
