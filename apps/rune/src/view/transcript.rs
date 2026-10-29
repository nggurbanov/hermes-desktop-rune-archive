//! Adapted from anomalyco/rune examples/opencode/src/view.rs at
//! 3f70cd6b675c89a8a7b64fc09b1bf2b455e0ad1a: avatar, timeline,
//! user_bubble, markdown_view and composer_dock.
//! Copyright (c) 2026 Christopher Langton. MIT licensed.
//!
//! The layout/text/Markdown builders are reused; Hermes supplies stored history.
//! Sending is outside this reader's scope, so the composer has no fake handler.

use rune::{El, GrowthSpec, ScrollSpec, TextStyle, col, el, row, text};
use rune_kit::{Kit, Markdown, MarkdownStyle};

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
        .find(|p| p.name == s.profile)
        .map(|p| p.label.as_str())
        .unwrap_or("Hermes");
    for (index, message) in s.messages.iter().enumerate() {
        let key = format!("message-{index}-{}", message.role);
        let content = if message.role == "user" {
            user_bubble(p, &key, &message.content)
        } else {
            col()
                .key(format!("body-{key}"))
                .w_full()
                .gap(8.0)
                .child(
                    text(if message.role == "assistant" {
                        assistant
                    } else {
                        &message.role
                    })
                    .font(UI)
                    .color(p.faint),
                )
                .child(markdown_view(p, &message.content))
        };
        stack = stack.child(
            col()
                .key(key)
                .w_full()
                .max_w(CONTENT_W)
                .pb(20.0)
                .child(content),
        );
    }
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

fn user_bubble(p: &Pal, key: &str, body: &str) -> El {
    row().w_full().justify_end().pt(8.0).pb(16.0).child(
        col()
            .max_w(460.0)
            .shrink()
            .px(12.0)
            .py(8.0)
            .radius(10.0)
            .fill(p.layer2)
            .child(
                text(body.to_owned())
                    .key(format!("text-{key}"))
                    .font(BUBBLE)
                    .color(p.text)
                    .wrap()
                    .selectable(true),
            ),
    )
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
