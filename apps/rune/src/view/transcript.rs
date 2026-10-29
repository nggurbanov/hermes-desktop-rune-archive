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
            let editor = text_input(edit, BUBBLE)
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
                    .fill(p.layer2),
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
                    markdown_view(p, &view.reasoning),
                )
                .max_w(CONTENT_W)
                .w_full()
                .pb(10.),
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
    let open = s
        .chat
        .views
        .get(owner)
        .is_some_and(|v| v.expanded.contains(key));
    let mut view = col().key(format!("detail-{key}")).w_full().gap(8.);
    view = view.child(
        action(
            &format!("{} {title}", if open { "⌄" } else { "›" }),
            owner.clone(),
            Action::Expand(key.into()),
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
        Part::Text(body) => markdown_view(p, body).key(key),
        Part::Reasoning(body) => disclosure(s, k, owner, &key, "Thought", markdown_view(p, body)),
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
                details = details.child(markdown_view(p, output));
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
            image.child(action(
                label,
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
    let Some((name, arg)) = s
        .composer
        .draft(&owner)
        .and_then(|d| crate::chat::parse_slash(&d.text.text))
    else {
        return el().h(0.);
    };
    if !arg.is_empty() {
        return el().h(0.);
    }
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
    for command in state
        .slash
        .iter()
        .filter(|r| r.name.starts_with(&name))
        .take(30)
    {
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
            .key(format!("slash-{}", command.name)),
        );
    }
    panel
}

fn markdown_view(p: &Pal, src: &str) -> El {
    let kit = Kit::neutral(p.dark);
    let style = MarkdownStyle::from_kit(&kit)
        .text(p.text)
        .muted(p.muted)
        .link(p.accent)
        .inline_code_bg(p.inline_code_bg)
        .code_block(p.layer1, p.border_muted)
        .quote_rule(p.layer3)
        .rule(p.border)
        .body_font(MD)
        .heading_font(1, H1)
        .heading_font(2, H2)
        .heading_font(3, H3)
        .code_block_font(CODE);
    Markdown::new(src).style(style).build(&kit).selectable(true)
}
