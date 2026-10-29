use crate::{
    chat::projection::{self, Message, Part},
    subagents::window::WindowState,
};
use rune::{El, FileDialogOptions, GrowthSpec, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Markdown};
use rune_theme_opencode::icons as ic;
fn expanded(state: &WindowState, key: &str) -> bool {
    state
        .chat
        .views
        .get(&state.owner())
        .is_some_and(|view| view.expanded.contains(key))
}
fn disclosure(state: &WindowState, kit: &Kit, key: &str, title: &str, body: El) -> El {
    let open = expanded(state, key);
    let id = key.to_owned();
    let mut content = col().key(format!("detail-{key}")).w_full().gap(6.0).child(
        Button::new(format!("{} {title}", if open { "⌄" } else { "›" }))
            .variant(ButtonVariant::GhostMuted)
            .on_press(move |s: &mut WindowState| s.toggle(id.clone()))
            .build(kit)
            .key(format!("toggle-{key}"))
            .justify_start(),
    );
    if open {
        content = content.child(body.px(12.0).py(8.0).max_h(320.0).scroll(ScrollSpec {
            stick_bottom: false,
        }));
    }
    content
}
fn native_name(source: &str, label: &str) -> String {
    let candidate = if source.starts_with("data:") && (label == "Image" || label.is_empty()) {
        "attachment"
    } else if label == "Image" || label.is_empty() {
        source
            .rsplit('/')
            .next()
            .unwrap_or(label)
            .split('?')
            .next()
            .unwrap_or(label)
    } else {
        label
    };
    let candidate = candidate.rsplit(['/', '\\']).next().unwrap_or("attachment");
    let mut name: String = candidate
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .take(160)
        .collect();
    if name.is_empty() || name.starts_with("data") {
        name = "attachment".into();
    }
    if !name.contains('.') {
        name.push_str(if source.starts_with("data:image/jpeg;") {
            ".jpg"
        } else if source.starts_with("data:image/webp;") {
            ".webp"
        } else if source.starts_with("data:application/pdf;") {
            ".pdf"
        } else if source.starts_with("data:image/") {
            ".png"
        } else {
            ""
        });
    }
    name
}
fn file_actions(state: &WindowState, kit: &Kit, key: &str, source: &str, label: &str) -> El {
    let open_source = source.to_owned();
    let open_label = native_name(source, label);
    let owner = state.owner();
    let save_source = source.to_owned();
    let save_label = native_name(source, label);
    let save_owner = state.owner();
    let name = native_name(source, label);
    row()
        .key(format!("file-actions-{key}"))
        .w_full()
        .gap(4.0)
        .flex_wrap()
        .child(
            Button::new("Open")
                .variant(ButtonVariant::GhostMuted)
                .disabled(state.files.busy())
                .help(
                    "Download an authenticated private copy and open it in the native application",
                )
                .on_press(move |s: &mut WindowState| {
                    s.transfer(owner.clone(), open_source.clone(), open_label.clone(), None)
                })
                .build(kit)
                .key(format!("open-{key}")),
        )
        .child(
            Button::new("Download…")
                .variant(ButtonVariant::GhostMuted)
                .disabled(state.files.busy())
                .on_press_tx(move |_: &mut WindowState, tx| {
                    let owner = save_owner.clone();
                    let source = save_source.clone();
                    let label = save_label.clone();
                    tx.save_file(
                        FileDialogOptions {
                            title: "Download attachment".into(),
                            file_name: Some(name.clone()),
                            ..Default::default()
                        },
                        move |s: &mut WindowState, result, _| match result {
                            Ok(Some(path)) => {
                                s.transfer(owner.clone(), source.clone(), label.clone(), Some(path))
                            }
                            Ok(None) => {}
                            Err(_) => {
                                s.files.error = Some("The native save dialog is unavailable".into())
                            }
                        },
                    );
                })
                .build(kit)
                .key(format!("download-{key}")),
        )
}
fn part(state: &WindowState, kit: &Kit, key: &str, value: &Part) -> El {
    match value {
        Part::Text(body) => Markdown::new(body).build(kit).selectable(true).key(key),
        Part::Reasoning(body) => disclosure(
            state,
            kit,
            key,
            "Thought",
            Markdown::new(body).build(kit).selectable(true),
        ),
        Part::Tool {
            name, args, output, ..
        } => {
            let mut body = col().w_full().gap(8.0).child(
                text(args)
                    .font(rune::TextStyle::new(12.0, 400.0).mono())
                    .color(kit.c.text_muted)
                    .wrap()
                    .selectable(true),
            );
            if let Some(output) = output {
                let copy = output.clone();
                body = body
                    .child(Markdown::new(output).build(kit).selectable(true))
                    .child(
                        Button::new("Copy output")
                            .variant(ButtonVariant::GhostMuted)
                            .on_press_tx(move |_: &mut WindowState, tx| tx.copy(copy.clone()))
                            .build(kit),
                    );
            }
            disclosure(
                state,
                kit,
                key,
                &format!(
                    "{} {name}",
                    if output.is_some() {
                        "Completed"
                    } else {
                        "Called"
                    }
                ),
                body,
            )
        }
        Part::Image { source, label } => {
            let media = state.chat.media.get(&(state.owner(), source.clone()));
            let mut body = col().key(key).w_full().gap(6.0);
            if let Some(image) = media
                .and_then(|m| m.image.as_ref())
                .filter(|image| image.is_ready() && image.error().is_none())
            {
                let full = state
                    .chat
                    .views
                    .get(&state.owner())
                    .is_some_and(|view| view.preview.as_ref() == Some(source));
                let zoom = source.clone();
                body = body.child(
                    el().key(format!("pixels-{key}"))
                        .role("img")
                        .label(label)
                        .w_full()
                        .h(if full { 480.0 } else { 220.0 })
                        .pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
                        .on_press(move |s: &mut WindowState| s.image(zoom.clone())),
                );
            } else {
                let error = media.and_then(|m| m.error.as_deref()).or_else(|| {
                    media
                        .and_then(|m| m.image.as_ref())
                        .and_then(|image| image.error())
                        .map(|_| "Image could not be decoded")
                });
                body = body.child(
                    text(error.unwrap_or("Loading image…"))
                        .font(kit.ty.small)
                        .color(if error.is_some() {
                            kit.c.danger.fg
                        } else {
                            kit.c.text_muted
                        })
                        .wrap(),
                );
                if media.is_none() {
                    let source = source.clone();
                    body = body.child(
                        Button::new("Load image")
                            .variant(ButtonVariant::GhostMuted)
                            .on_press(move |s: &mut WindowState| s.image(source.clone()))
                            .build(kit)
                            .key(format!("load-{key}")),
                    );
                }
                if error.is_some() {
                    let source = source.clone();
                    body = body.child(
                        Button::new("Retry image")
                            .variant(ButtonVariant::GhostMuted)
                            .on_press(move |s: &mut WindowState| s.retry_image(source.clone()))
                            .build(kit)
                            .key(format!("retry-{key}")),
                    );
                }
            }
            body.child(text(label).font(kit.ty.small).color(kit.c.text_muted))
                .child(file_actions(state, kit, key, source, label))
        }
        Part::File { path, label } => col()
            .key(key)
            .w_full()
            .gap(4.0)
            .child(text(label).font(kit.ty.body).color(kit.c.text_base).wrap())
            .child(file_actions(state, kit, key, path, label)),
    }
}
fn actions(state: &WindowState, kit: &Kit, message: &Message) -> El {
    let copy = message.text.clone();
    let mut buttons = row()
        .key(format!("actions-{}", message.key))
        .w_full()
        .gap(3.0)
        .flex_wrap()
        .center_y()
        .child(
            IconButton::new(&ic::OUTLINE_COPY)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Copy message")
                .help("Copy message")
                .on_press_tx(move |_: &mut WindowState, tx| tx.copy(copy.clone()))
                .build(kit)
                .key(format!("copy-{}", message.key)),
        );
    if message.role == "assistant" && !message.text.trim().is_empty() {
        let key = message.key.clone();
        let body = message.text.clone();
        buttons = buttons.child(
            Button::new(if state.reading.as_ref() == Some(&message.key) {
                "Stop reading"
            } else {
                "Read aloud"
            })
            .variant(ButtonVariant::GhostMuted)
            .disabled(
                state
                    .reading
                    .as_ref()
                    .is_some_and(|key| key != &message.key),
            )
            .on_press(move |s: &mut WindowState| s.read(key.clone(), body.clone()))
            .build(kit)
            .key(format!("read-{}", message.key)),
        );
    }
    buttons
}
pub(super) fn stack(state: &WindowState, kit: &Kit) -> El {
    let mut stack = col()
        .key("subagent-transcript-stack")
        .w_full()
        .gap(20.0)
        .virtualize(80.0)
        .growth(GrowthSpec::default())
        .px(20.0)
        .py(18.0);
    for message in projection::project(&state.projection.messages) {
        let mut body = col().w_full().gap(8.0);
        if message.role == "user" {
            body = body
                .px(12.0)
                .py(10.0)
                .radius(8.0)
                .fill(kit.c.layer_01)
                .border(1.0, kit.c.border_muted);
        } else {
            body = body.child(
                text(if message.role == "assistant" {
                    "Subagent"
                } else {
                    &message.role
                })
                .font(kit.ty.small)
                .color(kit.c.text_muted),
            );
        }
        for (index, value) in message.parts.iter().enumerate() {
            body = body.child(part(
                state,
                kit,
                &format!("{}-part-{index}", message.key),
                value,
            ));
        }
        body = body.child(actions(state, kit, &message));
        stack = stack.child(body.key(format!("subagent-message-{}", message.key)));
    }
    if state.projection.messages.is_empty() {
        stack = stack.child(
            text(if state.projection.connected {
                "No messages in this subagent session yet"
            } else {
                "Loading subagent history…"
            })
            .font(kit.ty.body)
            .color(kit.c.text_muted)
            .wrap(),
        );
    }
    if !state.projection.reasoning.is_empty() {
        stack = stack.child(disclosure(
            state,
            kit,
            "live-reasoning",
            "Thought",
            Markdown::new(&state.projection.reasoning)
                .build(kit)
                .selectable(true),
        ));
    }
    if !state.projection.activities.is_empty() {
        for (index, activity) in state.projection.activities.iter().enumerate() {
            stack = stack.child(part(
                state,
                kit,
                &format!("live-tool-{index}"),
                &projection::activity(activity),
            ));
        }
    } else if !state.projection.tools.is_empty() {
        for (index, tool) in state.projection.tools.iter().enumerate() {
            stack = stack.child(
                text(tool)
                    .key(format!("subagent-tool-{index}"))
                    .font(kit.ty.small)
                    .color(kit.c.text_muted)
                    .wrap()
                    .selectable(true),
            );
        }
    }
    stack
}
