//! Adapted from anomalyco/rune examples/opencode/src/view.rs at
//! 3f70cd6b675c89a8a7b64fc09b1bf2b455e0ad1a: avatar, timeline,
//! user_bubble, markdown_view and composer_dock.
//! Copyright (c) 2026 Christopher Langton. MIT licensed.
//!
//! The layout/text/Markdown builders are reused; Hermes supplies stored history.
//! Conversation workers supply live messages; native actions retain their original owners.

use crate::{
    chat::{
        Action, RewriteKind,
        projection::{self, Message, Part},
    },
    composer::DraftScope,
};
use rune::{El, GrowthSpec, ScrollSpec, TextStyle, auto_grow, col, el, row, text, text_input};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Markdown, MarkdownStyle,
};
use rune_theme_opencode::icons as ic;

use super::palette::Pal;
use crate::app::Viewer;

const UI: TextStyle = TextStyle::new(13.0, 440.0).lh(16.0).tracking(-0.04);
const AVATAR: TextStyle = TextStyle::new(11.0, 530.0).lh(16.0);
const MD: TextStyle = TextStyle::new(14.0, 400.0).lh(22.4);
const BUBBLE: TextStyle = TextStyle::new(14.0, 400.0).lh(21.0);
const H1: TextStyle = TextStyle::new(17.0, 600.0).lh(24.0);
const H2: TextStyle = TextStyle::new(15.0, 600.0).lh(24.0);
const H3: TextStyle = TextStyle::new(13.0, 600.0).lh(20.0);
const CODE: TextStyle = TextStyle::new(13.0, 400.0).mono().lh(20.0);
const CONTENT_W: f32 = 696.0;

pub fn avatar(p: &Pal, name: &str) -> El {
    let k = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) as usize
        % 8;
    let (bg, border) = p.avatars[k];
    let letter = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or("?".into(), |c| c.to_uppercase().to_string());
    el().size(16.0, 16.0)
        .radius(4.0)
        .fill(bg)
        .border(0.5, border)
        .center()
        .child(text(letter).font(AVATAR).color(p.avatar_fg))
}

pub fn timeline(s: &Viewer, p: &Pal) -> El {
    let k = rune_theme_opencode::kit(p.dark);
    let owner = s.composer_scope();
    let mut stack = col()
        .key("timeline")
        .w_full()
        .center()
        .pt(16.0)
        .pb(24.0)
        .virtualize(80.0)
        .growth(GrowthSpec::default());
    let assistant = s
        .profiles
        .iter()
        .find(|profile| profile.name == s.profile)
        .map(|p| p.label.as_str())
        .unwrap_or("Hermes");
    if s.history_truncated {
        let scope = owner.clone();
        let loading = s
            .conversation
            .view(&owner)
            .is_some_and(|v| v.history_loading);
        stack = stack.child(
            Button::new(if loading {
                "Loading earlier messages…"
            } else {
                "Load earlier messages"
            })
            .variant(ButtonVariant::GhostMuted)
            .disabled(loading || s.is_fixture())
            .on_press(move |s: &mut Viewer| s.chat_action(scope.clone(), Action::Earlier))
            .build(k)
            .key("history-earlier"),
        );
    }
    let projected = projection::project(&s.messages);
    for message in projected {
        let key = format!("message-{}", message.key);
        let mut body = col().key(format!("body-{key}")).w_full().gap(8.0);
        if message.role != "user" {
            body = body.child(
                text(if message.role == "assistant" {
                    assistant
                } else {
                    &message.role
                })
                .font(UI)
                .color(p.faint),
            );
        }
        let edit = s
            .chat
            .views
            .get(&owner)
            .and_then(|v| v.edit.as_ref())
            .filter(|(row, _)| Some(*row) == message.row);
        if let Some((_, edit)) = edit {
            let scope = owner.clone();
            let editor = text_input(edit, s.client_settings.prose(BUBBLE))
                .key("chat-edit-input")
                .w_full()
                .color(p.text)
                .caret(p.text, k.selection())
                .on_edit(move |s: &mut Viewer| {
                    &mut s.chat.presentation(scope.clone()).edit.as_mut().unwrap().1
                });
            body = body.child(auto_grow(editor, 2, 10));
            body = body.child(
                row()
                    .gap(6.)
                    .child(
                        action("Review and send", owner.clone(), Action::SaveEdit, k)
                            .key("chat-edit-save"),
                    )
                    .child(action("Cancel", owner.clone(), Action::Cancel, k)),
            );
        } else {
            for (index, part) in message.parts.iter().enumerate() {
                body = body.child(render_part(s, p, k, &owner, &message, index, part));
            }
        }
        body = body.child(message_actions(s, k, &owner, &message));
        if message.role == "user" {
            body = row().w_full().justify_end().child(
                body.max_w(560.)
                    .shrink()
                    .px(14.)
                    .py(10.)
                    .radius(10.)
                    .fill(s.client_settings.bubble_fill(p.layer2))
                    .border(1., p.border_muted),
            );
        }
        stack = stack.child(col().key(key).w_full().max_w(CONTENT_W).pb(20.).child(body));
    }
    if let Some(view) = s.conversation.view(&owner) {
        if !view.reasoning.trim().is_empty() {
            stack = stack.child(
                disclosure(
                    s,
                    k,
                    &owner,
                    "live-reasoning",
                    "Thought",
                    markdown_view(s, p, &view.reasoning),
                )
                .max_w(CONTENT_W)
                .w_full()
                .pb(10.),
            );
        }
        if view.activities.is_empty() && !view.tools.is_empty() {
            let mut details = col().w_full().gap(6.);
            for tool in &view.tools {
                details = details.child(
                    text(tool)
                        .font(s.client_settings.prose(UI))
                        .color(p.muted)
                        .wrap()
                        .selectable(true),
                );
            }
            stack = stack.child(
                disclosure(s, k, &owner, "legacy-tools", "Agent activity", details)
                    .max_w(CONTENT_W)
                    .w_full()
                    .pb(8.),
            );
        }
        for (index, tool) in view.activities.iter().enumerate() {
            let key = format!("live-tool-{index}");
            let title = format!(
                "{} {}",
                if tool["state"] == "complete" {
                    "Completed"
                } else {
                    "Running"
                },
                tool["name"].as_str().unwrap_or("Tool")
            );
            let details = col()
                .w_full()
                .gap(8.)
                .child(
                    text(
                        tool.get("arguments")
                            .or(tool.get("args"))
                            .map(|v| {
                                v.as_str().map(str::to_owned).unwrap_or_else(|| {
                                    serde_json::to_string_pretty(v).unwrap_or_default()
                                })
                            })
                            .unwrap_or_default(),
                    )
                    .font(CODE)
                    .color(p.muted)
                    .wrap()
                    .selectable(true),
                )
                .child(markdown_view(
                    s,
                    p,
                    tool["result"]
                        .as_str()
                        .or(tool["output"].as_str())
                        .or(tool["text"].as_str())
                        .unwrap_or(""),
                ));
            stack = stack.child(
                disclosure(s, k, &owner, &key, &title, details)
                    .max_w(CONTENT_W)
                    .w_full()
                    .pb(8.),
            );
        }
    }
    stack = stack.child(chat_status(s, k, &owner).max_w(CONTENT_W).w_full());
    col()
        .key(format!(
            "history-{}-{}",
            s.profile,
            s.selected_session.as_deref().unwrap_or("draft")
        ))
        .role("log")
        .label("Conversation history")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec { stick_bottom: true })
        .clip_fade(0.0, 8.0, 0.0, 8.0)
        .px(24.0)
        .child(stack)
}

fn action(label: &str, owner: DraftScope, value: Action, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut Viewer| s.chat_action(owner.clone(), value.clone()))
        .build(k)
}
fn disclosure(s: &Viewer, k: &Kit, owner: &DraftScope, key: &str, title: &str, content: El) -> El {
    let reasoning = title == "Thought";
    if reasoning && !s.client_settings.show_reasoning() {
        return el().h(0.);
    }
    let open = s
        .chat
        .views
        .get(owner)
        .map(|v| {
            if v.collapsed.contains(key) {
                Some(false)
            } else if v.expanded.contains(key) {
                Some(true)
            } else {
                None
            }
        })
        .flatten()
        .unwrap_or_else(|| s.client_settings.default_open(reasoning, s.simple_mode));
    let mut view = col().key(format!("detail-{key}")).w_full().gap(8.);
    view = view.child(
        action(
            &format!("{} {title}", if open { "⌄" } else { "›" }),
            owner.clone(),
            Action::SetDisclosure(key.into(), !open),
            k,
        )
        .key(format!("toggle-{key}"))
        .justify_start(),
    );
    if open {
        view = view.child(content.px(12.).py(8.).max_h(360.).scroll(ScrollSpec {
            stick_bottom: false,
        }));
    }
    view
}
fn render_part(
    s: &Viewer,
    p: &Pal,
    k: &Kit,
    owner: &DraftScope,
    message: &Message,
    index: usize,
    part: &Part,
) -> El {
    let key = format!("{}-part-{index}", message.key);
    match part {
        Part::Text(body) => markdown_view(s, p, body).key(key),
        Part::Reasoning(body) => {
            disclosure(s, k, owner, &key, "Thought", markdown_view(s, p, body))
        }
        Part::Tool {
            name,
            args,
            output,
            process,
            ..
        } => {
            let mut details = col()
                .w_full()
                .gap(8.)
                .child(text(args).font(CODE).color(p.muted).wrap().selectable(true));
            if let Some(output) = output {
                details = details.child(markdown_view(s, p, output));
            }
            if let Some(process) = process {
                details = details.child(action(
                    "Open process in Terminal",
                    owner.clone(),
                    Action::Process(process.clone()),
                    k,
                ));
            }
            disclosure(
                s,
                k,
                owner,
                &key,
                &format!(
                    "{} {name}",
                    if output.is_some() {
                        "Completed"
                    } else {
                        "Called"
                    }
                ),
                details,
            )
        }
        Part::Image { source, label } => {
            let media = s.chat.media.get(&(owner.clone(), source.clone()));
            let mut image = col().key(key).w_full().gap(6.);
            if let Some(pixels) = media.and_then(|m| m.image.as_ref()) {
                if pixels.error().is_none() {
                    image = image.child(
                        el().w_full()
                            .h(
                                if s.chat
                                    .views
                                    .get(owner)
                                    .is_some_and(|v| v.preview.as_ref() == Some(source))
                                {
                                    520.
                                } else {
                                    260.
                                },
                            )
                            .pixels_fit(pixels.pixels(), rune::SurfaceFit::Fit),
                    );
                } else {
                    image = image.child(
                        text("Image could not be decoded")
                            .font(k.ty.small)
                            .color(k.c.danger.fg),
                    );
                }
            } else {
                image = image.child(
                    text(
                        media
                            .and_then(|m| m.error.as_deref())
                            .unwrap_or("Loading image…"),
                    )
                    .font(k.ty.small)
                    .color(p.muted)
                    .wrap(),
                );
            }
            if media.is_some_and(|m| {
                m.error.is_some() || m.image.as_ref().is_some_and(|i| i.error().is_some())
            }) {
                image = image.child(action(
                    "Retry image",
                    owner.clone(),
                    Action::RetryMedia(source.clone()),
                    k,
                ));
            }
            image.child(action(
                &if media.is_none() {
                    format!("Load {label}")
                } else {
                    label.clone()
                },
                owner.clone(),
                Action::Image(source.clone()),
                k,
            ))
        }
        Part::File { path, label } => action(
            &format!("↗ {label}"),
            owner.clone(),
            Action::File(path.clone()),
            k,
        )
        .key(key)
        .justify_start(),
    }
}
fn message_actions(s: &Viewer, k: &Kit, owner: &DraftScope, message: &Message) -> El {
    let copy = message.text.clone();
    let mut controls = row()
        .key(format!("actions-{}", message.key))
        .w_full()
        .gap(2.)
        .center_y()
        .flex_wrap();
    controls = controls.child(
        IconButton::new(&ic::OUTLINE_COPY)
            .variant(IconButtonVariant::GhostMuted)
            .label("Copy message")
            .help("Copy message")
            .control_size(ControlSize::Small)
            .on_press_tx(move |_: &mut Viewer, tx| tx.copy(copy.clone()))
            .build(k),
    );
    controls = controls.child(
        action(
            "Quote",
            owner.clone(),
            Action::Quote(message.text.clone()),
            k,
        )
        .key(format!("quote-{}", message.key)),
    );
    if message.role == "assistant" && !message.text.is_empty() {
        controls = controls.child(action(
            "Read aloud",
            owner.clone(),
            Action::Speak(message.text.clone()),
            k,
        ));
    }
    if let Some(row) = message.row {
        let allowed = s
            .conversation
            .view(owner)
            .is_some_and(|v| !v.running && !v.preparing && v.uncertain.is_none() && v.canonical);
        if allowed {
            controls = controls.child(action(
                "Branch",
                owner.clone(),
                Action::Branch(message.through.unwrap_or(row)),
                k,
            ));
            if message.role == "user" {
                controls = controls.child(
                    action("Edit", owner.clone(), Action::Edit(row), k)
                        .key(format!("edit-{}", message.key)),
                );
                controls = controls.child(
                    action("Retry", owner.clone(), Action::Retry(row), k)
                        .key(format!("retry-{}", message.key)),
                );
                controls = controls.child(
                    action("Restore", owner.clone(), Action::Restore(row), k)
                        .key(format!("restore-{}", message.key)),
                );
            }
        }
    }
    if s.client_settings.timestamps() {
        if let Some(timestamp) = message.timestamp.and_then(|value| {
            chrono::DateTime::from_timestamp(
                value.floor() as i64,
                ((value - value.floor()) * 1_000_000_000.) as u32,
            )
        }) {
            controls = controls.child(el().grow(1.)).child(
                text(
                    timestamp
                        .with_timezone(&chrono::Local)
                        .format("%H:%M")
                        .to_string(),
                )
                .font(k.ty.text_xsmall)
                .color(k.c.text_faint),
            );
        }
    }
    controls
}
fn chat_status(s: &Viewer, k: &Kit, owner: &DraftScope) -> El {
    let mut view = col().key("chat-actions-status").w_full().gap(10.);
    let Some(state) = s.chat.views.get(owner) else {
        return view;
    };
    if let Some(confirmation) = &state.confirmation {
        let verb = match confirmation.kind {
            RewriteKind::Edit => "Edit and resend",
            RewriteKind::Retry => "Retry",
            RewriteKind::Restore => "Restore to draft",
        };
        view=view.child(col().w_full().gap(8.).px(12.).py(10.).border(1.,k.c.border_base).radius(8.)
            .child(text(format!("{verb}? This removes this turn and {} later user turn{} from the agent’s history.",confirmation.later,if confirmation.later==1{""}else{"s"})).font(k.ty.small).color(k.c.text_base).wrap())
            .child(row().gap(8.).child(action("Confirm",owner.clone(),Action::Confirm,k)).child(action("Cancel",owner.clone(),Action::Cancel,k))));
    }
    if let Some(error) = &state.error {
        view = view.child(
            text(error)
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if let Some(notice) = &state.notice {
        view = view.child(
            text(notice)
                .font(k.ty.body)
                .color(k.c.text_muted)
                .wrap()
                .selectable(true),
        );
    }
    if let Some((_, message)) = &state.slash_confirmation {
        view = view
            .child(text(message).font(k.ty.small).color(k.c.text_base).wrap())
            .child(
                action(
                    "Confirm model change",
                    owner.clone(),
                    Action::ConfirmSlash,
                    k,
                )
                .key("confirm-slash-model"),
            )
            .child(
                action(
                    "Cancel model change",
                    owner.clone(),
                    Action::ResolveSlashUnknown,
                    k,
                )
                .key("cancel-slash-model"),
            );
    } else if let Some(intent) = &state.slash_intent {
        if !state.slash_running {
            if intent.retryable() || !intent.attempted {
                view = view.child(
                    action(
                        if intent.session_bypass() {
                            "Resolve bypass change"
                        } else {
                            "Retry retained command"
                        },
                        owner.clone(),
                        Action::RetrySlash,
                        k,
                    )
                    .key("retry-slash-command"),
                );
            }
            view = view.child(
                action(
                    "I reviewed the command’s outcome",
                    owner.clone(),
                    Action::ResolveSlashUnknown,
                    k,
                )
                .key("acknowledge-slash-command"),
            );
        }
    }
    if let Some(command) = &state.failed_command {
        let copy = command.clone();
        view = view.child(
            Button::new("Copy failed command")
                .variant(ButtonVariant::GhostMuted)
                .on_press_tx(move |_: &mut Viewer, tx| tx.copy(copy.clone()))
                .build(k),
        );
    }
    if state.prefill_candidate.is_some() {
        view = view.child(action(
            "Append command result to draft",
            owner.clone(),
            Action::AppendPrefill,
            k,
        ));
    }
    if state.restored_candidate.is_some() {
        view = view.child(
            action(
                "Append restored input to draft",
                owner.clone(),
                Action::UseRestored,
                k,
            )
            .key("append-restored-input"),
        );
    }
    if state.branch_busy {
        view = view.child(
            text("Preparing branch…")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    }
    if state.branch.is_some() && state.branch_result.is_none() && !state.branch_busy {
        view = view.child(action(
            "Retry retained branch",
            owner.clone(),
            Action::RetryBranch,
            k,
        ));
    }
    if state.branch_result.is_some() {
        view = view.child(action("Open branch", owner.clone(), Action::OpenBranch, k));
    }
    if state.error.is_some() || state.notice.is_some() {
        view = view.child(action("Dismiss", owner.clone(), Action::Dismiss, k));
    }
    view
}
pub fn slash_panel(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let mut panel = col()
        .key("slash-completions")
        .w_full()
        .max_h(230.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .px(20.)
        .gap(2.);
    let candidates = s.chat_completion_candidates(&owner);
    let Some(state) = s.chat.views.get(&owner) else {
        return el().h(0.);
    };
    if state.slash_loading {
        return panel.child(
            text("Loading commands and skills…")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    }
    for (index, command) in candidates.iter().enumerate() {
        panel = panel.child(
            action(
                &format!(
                    "/{}   {}{}",
                    command.name,
                    command.description,
                    if command.skill { " · Skill" } else { "" }
                ),
                owner.clone(),
                Action::ChooseSlash(command.name.clone()),
                k,
            )
            .justify_start()
            .key(format!("slash-{}", command.name))
            .fill(
                if index
                    == state
                        .slash_highlight
                        .min(candidates.len().saturating_sub(1))
                {
                    k.c.layer_02
                } else {
                    rune::Color::TRANSPARENT
                },
            ),
        );
    }
    panel
}

fn markdown_view(s: &Viewer, p: &Pal, src: &str) -> El {
    let kit = Kit::neutral(p.dark);
    let style = MarkdownStyle::from_kit(&kit)
        .text(p.text)
        .muted(p.muted)
        .link(p.accent)
        .inline_code_bg(p.inline_code_bg)
        .code_block(p.layer1, p.border_muted)
        .quote_rule(p.layer3)
        .rule(p.border)
        .body_font(s.client_settings.prose(MD))
        .heading_font(1, s.client_settings.prose(H1))
        .heading_font(2, s.client_settings.prose(H2))
        .heading_font(3, s.client_settings.prose(H3))
        .code_block_font(s.client_settings.prose(CODE));
    let owner = s.composer_scope();
    Markdown::new(src)
        .style(style)
        .on_link_tx(move |s: &mut Viewer, target, tx| {
            if s.composer_scope() != owner {
                return;
            }
            match url::Url::parse(target) {
                Ok(url)
                    if matches!(url.scheme(), "http" | "https")
                        && url.username().is_empty()
                        && url.password().is_none() =>
                {
                    if tx.open_url(url.as_str()).is_err() {
                        s.compose_error = Some("The link could not be opened".into());
                    }
                }
                Ok(url) if url.scheme() == "file" => {
                    if let Ok(path) = url.to_file_path() {
                        s.open_chat_file(owner.clone(), path.to_string_lossy().into_owned());
                    }
                }
                Err(url::ParseError::RelativeUrlWithoutBase) if !target.starts_with('#') => {
                    let path = if std::path::Path::new(target).is_absolute() {
                        Some(target.to_owned())
                    } else {
                        s.workspace_cwd().map(|cwd| {
                            std::path::Path::new(&cwd)
                                .join(target)
                                .to_string_lossy()
                                .into_owned()
                        })
                    };
                    if let Some(path) = path {
                        s.open_chat_file(owner.clone(), path);
                    }
                }
                _ => {}
            }
        })
        .build(&kit)
        .selectable(true)
}
