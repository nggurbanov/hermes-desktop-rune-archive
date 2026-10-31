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
        search,
    },
    composer::DraftScope,
};
use rune::{El, GrowthSpec, ScrollSpec, TextStyle, auto_grow, col, el, row, text, text_input};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Markdown,
    MarkdownStyle, TextInput,
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

pub(crate) fn live_turn_key(view: &crate::conversation::ConversationView) -> String {
    format!(
        "live-turn-{}-{}",
        view.generation,
        view.active_admission
            .as_deref()
            .or(view.completed_admission.as_deref())
            .unwrap_or("unidentified")
    )
}
pub(crate) fn live_tool_key(
    view: &crate::conversation::ConversationView,
    tool: &serde_json::Value,
) -> String {
    format!(
        "{}-tool-{}",
        live_turn_key(view),
        tool["id"].as_str().unwrap_or("unidentified")
    )
}
pub(crate) fn live_tool_title(tool: &serde_json::Value) -> String {
    format!(
        "{} {}",
        if tool["state"] == "complete" {
            "Completed"
        } else {
            "Running"
        },
        tool["name"].as_str().unwrap_or("Tool")
    )
}
pub(crate) fn live_tool_args(tool: &serde_json::Value) -> String {
    tool.get("arguments")
        .or(tool.get("args"))
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| serde_json::to_string_pretty(v).unwrap_or_default())
        })
        .unwrap_or_default()
}
pub(crate) fn live_tool_output(tool: &serde_json::Value) -> &str {
    tool["result"]
        .as_str()
        .or(tool["output"].as_str())
        .or(tool["text"].as_str())
        .or(tool["error"].as_str())
        .unwrap_or("")
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
                .key(s.chat_edit_key())
                .w_full()
                .color(p.text)
                .caret(p.text, k.selection())
                .text_highlights(literal_marks(s, k, &message.key, "edit"))
                .on_edit(move |s: &mut Viewer| {
                    &mut s.chat.presentation(scope.clone()).edit.as_mut().unwrap().1
                })
                .on_input_display_change(|s: &mut Viewer, _| s.refresh_chat_search());
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
            let message = live_turn_key(view);
            let key = format!("{message}-reasoning");
            stack = stack.child(
                col()
                    .key(format!("message-{message}"))
                    .w_full()
                    .max_w(CONTENT_W)
                    .child(
                        disclosure(
                            s,
                            k,
                            &owner,
                            &key,
                            "Thought",
                            markdown_search_view(s, p, &view.reasoning, &message, "reasoning"),
                        )
                        .w_full()
                        .pb(10.),
                    ),
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
        for tool in &view.activities {
            let key = live_tool_key(view, tool);
            let title = live_tool_title(tool);
            let details = col()
                .w_full()
                .gap(8.)
                .child(
                    text(live_tool_args(tool))
                        .font(CODE)
                        .color(p.muted)
                        .wrap()
                        .selectable(true)
                        .text_highlights(literal_marks(s, k, &key, "args")),
                )
                .child(markdown_search_view(
                    s,
                    p,
                    live_tool_output(tool),
                    &key,
                    "output",
                ));
            stack = stack.child(
                col()
                    .key(format!("message-{key}"))
                    .w_full()
                    .max_w(CONTENT_W)
                    .child(
                        disclosure_search(
                            s,
                            k,
                            &owner,
                            &key,
                            &title,
                            details,
                            Some((&key, "title")),
                        )
                        .w_full()
                        .pb(8.),
                    ),
            );
        }
    }
    stack = stack.child(chat_status(s, k, &owner).max_w(CONTENT_W).w_full());
    col()
        .key(s.chat_history_key())
        .focusable(true)
        .on_key(|s: &mut Viewer, event, tx| s.chat_search_key(event, tx))
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
fn literal_marks(s: &Viewer, k: &Kit, message: &str, part: &str) -> Vec<rune::TextHighlight> {
    let Some(view) = s
        .chat
        .searches
        .views
        .get(&s.chat_search_owner())
        .filter(|v| v.open)
    else {
        return vec![];
    };
    let mut marks = Vec::new();
    for hit in view
        .search
        .matches
        .iter()
        .filter(|h| h.message == message && h.part == part)
    {
        let current = view.search.current() == Some(hit);
        for range in &hit.bytes {
            let bg = if k.dark {
                k.alpha(k.c.warning.icon, if current { 0.40 } else { 0.24 })
            } else if current {
                rune::oklcha(0.82, 0.17, 75.0, 0.58)
            } else {
                rune::oklcha(0.86, 0.15, 85.0, 0.42)
            };
            let mut mark = rune::TextHighlight::new(range.clone(), bg).key(format!(
                "search-{}-{}-{}-{}",
                message, part, range.start, range.end
            ));
            if current {
                mark = mark.ring(
                    if k.dark {
                        k.c.warning.icon
                    } else {
                        rune::oklch(0.66, 0.16, 70.0)
                    },
                    1.0,
                );
            }
            marks.push(mark);
        }
    }
    marks
}
fn search_action(
    s: &Viewer,
    label: &str,
    owner: DraftScope,
    value: Action,
    k: &Kit,
    message: &str,
    part: &str,
) -> El {
    let label = label.to_owned();
    let shown = label.clone();
    let marks = literal_marks(s, k, message, part);
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .label_with(move |font, ink| text(shown).font(font).color(ink).text_highlights(marks))
        .on_press(move |s: &mut Viewer| s.chat_action(owner.clone(), value.clone()))
        .build(k)
}
pub(crate) fn disclosure_open(s: &Viewer, owner: &DraftScope, key: &str, reasoning: bool) -> bool {
    if reasoning && !s.client_settings.show_reasoning() {
        return false;
    }
    s.chat
        .views
        .get(owner)
        .and_then(|v| {
            if v.collapsed.contains(key) {
                Some(false)
            } else if v.expanded.contains(key) {
                Some(true)
            } else {
                None
            }
        })
        .unwrap_or_else(|| s.client_settings.default_open(reasoning, s.simple_mode))
}
fn disclosure(s: &Viewer, k: &Kit, owner: &DraftScope, key: &str, title: &str, content: El) -> El {
    disclosure_search(s, k, owner, key, title, content, None)
}
fn disclosure_search(
    s: &Viewer,
    k: &Kit,
    owner: &DraftScope,
    key: &str,
    title: &str,
    content: El,
    identity: Option<(&str, &str)>,
) -> El {
    let reasoning = title == "Thought";
    if reasoning && !s.client_settings.show_reasoning() {
        return el().h(0.);
    }
    let open = disclosure_open(s, owner, key, reasoning);
    let mut view = col().key(format!("detail-{key}")).w_full().gap(8.);
    let label = search::disclosure_label(title, open);
    let toggle = if let Some((message, part)) = identity {
        search_action(
            s,
            &label,
            owner.clone(),
            Action::SetDisclosure(key.into(), !open),
            k,
            message,
            part,
        )
    } else {
        action(
            &label,
            owner.clone(),
            Action::SetDisclosure(key.into(), !open),
            k,
        )
    };
    view = view.child(toggle.key(format!("toggle-{key}")).justify_start());
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
        Part::Text(body) => markdown_audio_view(
            s,
            p,
            &crate::chat::media_tags::display_text(&message.role, body),
            &message.key,
            &format!("part-{index}"),
        )
        .key(key),
        Part::Reasoning(body) => disclosure(
            s,
            k,
            owner,
            &key,
            "Thought",
            markdown_search_view(s, p, body, &message.key, &format!("part-{index}")),
        ),
        Part::Tool {
            name,
            args,
            output,
            process,
            ..
        } => {
            let mut details = col().w_full().gap(8.).child(
                text(args)
                    .font(CODE)
                    .color(p.muted)
                    .wrap()
                    .selectable(true)
                    .text_highlights(literal_marks(
                        s,
                        k,
                        &message.key,
                        &format!("part-{index}-args"),
                    )),
            );
            if let Some(output) = output {
                details = details.child(markdown_search_view(
                    s,
                    p,
                    output,
                    &message.key,
                    &format!("part-{index}"),
                ));
            }
            if let Some(process) = process {
                details = details.child(action(
                    "Open process in Terminal",
                    owner.clone(),
                    Action::Process(process.clone()),
                    k,
                ));
            }
            disclosure_search(
                s,
                k,
                owner,
                &key,
                &search::tool_caption(name, output.is_some()),
                details,
                Some((&message.key, &format!("part-{index}-title"))),
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
            image.child(search_action(
                s,
                &search::image_label(label, media.is_some()),
                owner.clone(),
                Action::Image(source.clone()),
                k,
                &message.key,
                &format!("part-{index}-label"),
            ))
        }
        Part::File { path, label } => {
            let open = search_action(
                s,
                &search::file_label(label),
                owner.clone(),
                Action::File(path.clone()),
                k,
                &message.key,
                &format!("part-{index}-label"),
            )
            .justify_start();
            if !baseline_audio_extension(path) {
                return open.key(key);
            }
            let identity = (
                owner.clone(),
                s.composer.editor_id().as_str().to_owned(),
                message.key.clone(),
                crate::app::InlineAudioOccurrence::File(index),
                path.clone(),
            );
            let mounted_identity = identity.clone();
            let mounted_label = label.clone();
            let mut body = col()
                .key(format!("{key}-audio-{identity:?}"))
                .w_full()
                .child(open)
                .on_mount_change(move |s: &mut Viewer, mounted| {
                    s.inline_audio_mount(mounted_identity.clone(), mounted_label.clone(), mounted)
                });
            if let Some(id) = s.inline_audio.get(&identity) {
                body = body.child(super::media_player::inline(
                    &s.media_players,
                    *id,
                    k,
                    |s: &mut Viewer| &mut s.media_players,
                ));
            }
            body
        }
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

fn baseline_audio_extension(path: &str) -> bool {
    let extension = path
        .split(['?', '#'])
        .next()
        .unwrap_or(path)
        .rsplit('.')
        .next()
        .unwrap_or("");
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "flac" | "m4a" | "mp3" | "ogg" | "opus" | "wav"
    )
}

pub(crate) fn markdown_audio_path(target: &str) -> Option<String> {
    let path = match url::Url::parse(target) {
        Ok(url) if url.scheme() == "file" => {
            let encoded_path = url.path().to_ascii_lowercase();
            if encoded_path.contains("%2f") || encoded_path.contains("%5c") {
                return None;
            }
            url.to_file_path().ok()?.to_string_lossy().into_owned()
        }
        Err(url::ParseError::RelativeUrlWithoutBase)
            if std::path::Path::new(target).is_absolute() =>
        {
            target.to_owned()
        }
        _ => return None,
    };
    baseline_audio_extension(target).then_some(path)
}

// Only generated #media links opt into external playback; ordinary HTTP links stay links.
fn decoded_media_target(destination: &str) -> Option<String> {
    let encoded = destination.strip_prefix("#media:")?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut iter = encoded.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let hi = (iter.next()? as char).to_digit(16)?;
            let lo = (iter.next()? as char).to_digit(16)?;
            bytes.push((hi * 16 + lo) as u8);
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).ok()
}

fn audio_target(destination: &str) -> Option<(String, bool)> {
    if destination.starts_with("#media:") {
        let target = decoded_media_target(destination)?;
        if !baseline_audio_extension(&target) {
            return None;
        }
        if let Ok(url) = url::Url::parse(&target) {
            if matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
                && url.host_str().is_some()
            {
                return Some((target, true));
            }
        }
        return markdown_audio_path(&target).map(|path| (path, true));
    }
    markdown_audio_path(destination).map(|path| (path, false))
}

fn audio_label(path: &str) -> String {
    let pathname = path.split(['?', '#']).next().unwrap_or(path);
    pathname.rsplit('/').next().unwrap_or(pathname).to_owned()
}

pub(crate) fn markdown_audio_ranges(src: &str) -> Vec<std::ops::Range<usize>> {
    let ranges = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = ranges.clone();
    let _ = Markdown::new(src)
        .embed_links(move |occurrence, _| {
            audio_target(&occurrence.destination)?;
            captured.borrow_mut().push(occurrence.source.clone());
            Some(col())
        })
        .build(&Kit::neutral(false));
    let result = ranges.borrow().clone();
    result
}

pub(crate) fn markdown_audio_labels(src: &str) -> Vec<(usize, String)> {
    let ranges = markdown_audio_ranges(src);
    rune_kit::markdown::link_occurrences_cached(src)
        .into_iter()
        .filter_map(|occurrence| {
            if !ranges.contains(&occurrence.source) {
                return None;
            }
            let (path, _) = audio_target(&occurrence.destination)?;
            Some((occurrence.source.start, audio_label(&path)))
        })
        .collect()
}

fn markdown_audio_view(s: &Viewer, p: &Pal, src: &str, message: &str, part: &str) -> El {
    let kit = Kit::neutral(p.dark);
    let mut embeds = std::collections::HashMap::new();
    for occurrence in rune_kit::markdown::link_occurrences_cached(src) {
        let Some((path, media)) = audio_target(&occurrence.destination) else {
            continue;
        };
        let label = audio_label(&path);
        let identity = (
            s.composer_scope(),
            s.composer.editor_id().as_str().to_owned(),
            message.to_owned(),
            if media {
                crate::app::InlineAudioOccurrence::Media {
                    part: part.to_owned(),
                    source: occurrence.source.clone(),
                }
            } else {
                crate::app::InlineAudioOccurrence::Markdown {
                    part: part.to_owned(),
                    source: occurrence.source.clone(),
                }
            },
            path,
        );
        let mounted_identity = identity.clone();
        let mounted_label = label.clone();
        let marks = literal_marks(
            s,
            &kit,
            message,
            &format!("{part}-audio-{}-label", occurrence.source.start),
        );
        let mut body = col()
            .key(format!("markdown-audio-{identity:?}"))
            .w_full()
            .max_w(448.)
            .child(text(label).text_highlights(marks))
            .on_mount_change(move |s: &mut Viewer, mounted| {
                s.inline_audio_mount(mounted_identity.clone(), mounted_label.clone(), mounted)
            });
        if let Some(id) = s.inline_audio.get(&identity) {
            body = body.child(super::media_player::inline(
                &s.media_players,
                *id,
                &kit,
                |s: &mut Viewer| &mut s.media_players,
            ));
        }
        embeds.insert(occurrence.source, body);
    }
    markdown_search_view_impl(s, p, src, message, part, embeds)
}

fn markdown_search_view(s: &Viewer, p: &Pal, src: &str, message: &str, part: &str) -> El {
    markdown_search_view_impl(s, p, src, message, part, std::collections::HashMap::new())
}

fn markdown_search_view_impl(
    s: &Viewer,
    p: &Pal,
    src: &str,
    message: &str,
    part: &str,
    embeds: std::collections::HashMap<std::ops::Range<usize>, El>,
) -> El {
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
    let marks = s
        .chat
        .searches
        .views
        .get(&s.chat_search_owner())
        .filter(|v| v.open)
        .map(|v| {
            v.search
                .matches
                .iter()
                .filter(|hit| hit.message == message && hit.part == part)
                .map(|hit| {
                    (
                        hit.bytes.clone(),
                        rune_kit::markdown::Highlight::new()
                            .current_match(v.search.current() == Some(hit))
                            .key(format!(
                                "search-{}-{}-{}-{}",
                                message,
                                part,
                                hit.bytes.first().map_or(0, |r| r.start),
                                hit.bytes.last().map_or(0, |r| r.end)
                            )),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Markdown::new(src)
        .embed_links(move |occurrence, _| embeds.get(&occurrence.source).cloned())
        .style(style)
        .highlights(marks)
        .on_link_tx(move |s: &mut Viewer, target, tx| {
            if s.composer_scope() != owner {
                return;
            }
            let decoded = decoded_media_target(target);
            let target = decoded.as_deref().unwrap_or(target);
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

pub fn search_bar(s: &Viewer, k: &Kit) -> El {
    let owner = s.chat_search_owner();
    let Some(view) = s.chat.searches.views.get(&owner).filter(|v| v.open) else {
        return el().h(0.);
    };
    let edit_owner = owner.clone();
    let input = TextInput::new(&view.input)
        .placeholder("Find in conversation")
        .on_edit(move |s: &mut Viewer| &mut s.chat.searches.view(edit_owner.clone()).input)
        .on_change(|s: &mut Viewer, tx| s.reveal_chat_search(None, tx))
        .on_key(|s: &mut Viewer, event, tx| s.chat_search_input_key(event, tx))
        .build(k)
        .key("chat-search-input")
        .w(220.);
    let count = if view.input.text.is_empty() {
        String::new()
    } else {
        format!("{} / {}", view.search.ordinal(), view.search.matches.len())
    };
    row()
        .key("chat-search-bar")
        .w_full()
        .justify_end()
        .center_y()
        .gap(4.)
        .px(24.)
        .py(6.)
        .child(input)
        .child(text(count).font(k.ty.small).color(k.c.text_muted))
        .child(
            Button::new("↑")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .disabled(view.search.matches.is_empty())
                .on_press_tx(|s: &mut Viewer, tx| s.reveal_chat_search(Some(false), tx))
                .build(k)
                .label("Previous match"),
        )
        .child(
            Button::new("↓")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .disabled(view.search.matches.is_empty())
                .on_press_tx(|s: &mut Viewer, tx| s.reveal_chat_search(Some(true), tx))
                .build(k)
                .label("Next match"),
        )
        .child(
            Button::new("×")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .on_press_tx(move |s: &mut Viewer, tx| {
                    let v = s.chat.searches.view(owner.clone());
                    v.open = false;
                    v.input.focused = false;
                    tx.blur();
                    tx.instant();
                })
                .build(k)
                .label("Close search"),
        )
}

#[cfg(test)]
pub(crate) fn search_test_timeline(s: &Viewer, _: &mut rune::Cx<Viewer>) -> El {
    timeline(s, super::palette::pal(true)).size(800., 600.)
}

#[cfg(test)]
pub(crate) fn short_search_test_timeline(s: &Viewer, _: &mut rune::Cx<Viewer>) -> El {
    timeline(s, super::palette::pal(true)).size(800., 180.)
}
