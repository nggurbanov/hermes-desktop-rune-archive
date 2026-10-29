//! Native Hermes reader. The transcript and composer rendering reuse OpenCode's
//! Rune example at 3f70cd6b675c89a8a7b64fc09b1bf2b455e0ad1a; see the modules.
//! Hermes owns the pane composition, connection/profile identities and actions.

mod audio;
pub(crate) mod client;
pub(crate) mod client_settings;
pub(crate) mod composer;
pub(crate) mod connections;
mod conversation;
mod decks;
mod directories;
pub(crate) mod files;
pub(crate) mod layout;
pub(crate) mod operations;
pub(crate) mod pages;
mod palette;
pub(crate) mod review;
mod shell;
mod sidebar;
mod status;
pub(crate) mod subagents;
pub(crate) mod terminal;
mod transcript;

use rune::{Cx, El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Kit, TextInput};
use rune_theme_opencode::icons as ic;

use crate::app::{SidebarTab, Viewer};
use palette::{Pal, pal};

pub fn app(s: &Viewer, cx: &mut Cx<Viewer>) -> El {
    let system_dark = cx.system_dark();
    s.client_settings.system_dark.set(system_dark);
    let dark = s.theme.unwrap_or(system_dark);
    let local_kit = s.client_settings.theme_kit(rune_theme_opencode::kit(dark));
    let k = &local_kit;
    let p = pal(dark);
    let mut layer = Vec::new();
    let main = if s.connection_form {
        connection_form(s, k)
    } else if s.route != crate::pages::Route::Chat {
        pages::page(s, k, &mut layer)
    } else {
        workspace(s, p, k, &mut layer)
    };
    let sidebar = match s.sidebar_tab {
        SidebarTab::Sessions => sidebar::sessions(s, p, k, &mut layer),
        SidebarTab::Bots => shell::bots_sidebar(s, p, k),
    };
    let split = shell::layout(s, cx, main, sidebar, k);
    let menu = shell::context_menu(s, k, &mut layer);
    status::overlays(s, k, &mut layer);
    if let Some(picker) = directories::panel(s, k) {
        layer.push(picker);
    }
    if s.health_open {
        layer.push(
            col()
                .key("health-popup")
                .absolute(24.0, 64.0)
                .w(580.0)
                .max_h(640.0)
                .fill(k.c.bg_deep)
                .radius(10.0)
                .border(1.0, k.c.border_muted)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    Button::new("Close health")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut Viewer| s.health_open = false)
                        .build(k),
                )
                .child(operations::health(s, k, &mut Vec::new())),
        );
    }
    col()
        .key("hermes-window")
        .role("window")
        .label("Hermes Desktop")
        .w_full()
        .h_full()
        .fill(k.c.bg_deep)
        .children(s.client_error.as_ref().map(|error| {
            row()
                .w_full()
                .gap(12.0)
                .px(16.0)
                .py(8.0)
                .fill(k.c.danger.bg)
                .child(
                    text(error)
                        .role("alert")
                        .font(k.ty.small)
                        .color(k.c.danger.fg)
                        .wrap()
                        .grow(1.0),
                )
                .child(
                    Button::new("Dismiss")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut Viewer| s.client_error = None)
                        .build(k),
                )
        }))
        .children(s.client_notice.as_ref().map(|notice| {
            row()
                .w_full()
                .px(16.0)
                .py(8.0)
                .gap(12.0)
                .child(
                    text(notice)
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap()
                        .grow(1.0),
                )
                .child(
                    Button::new("Dismiss")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut Viewer| s.client_notice = None)
                        .build(k),
                )
        }))
        .child(split.grow(1.0).min_h(0.0))
        .children((!s.simple_mode && s.show_statusbar).then(|| status::bar(s, k)))
        .child(menu)
        .child(
            el().key("overlay-layer")
                .role("none")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .instant_changes()
                .children(layer),
        )
}

fn workspace(s: &Viewer, p: &Pal, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut body = col()
        .key("workspace")
        .instant_changes()
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(shell::conversation_context(s, p, k));
    if let Some(error) = &s.error {
        body = body.child(
            col()
                .key("connection-error")
                .role("alert")
                .w_full()
                .px(24.0)
                .py(12.0)
                .gap(4.0)
                .fill(k.c.danger.bg)
                .child(
                    text("Could not load Hermes")
                        .font(k.ty.ui_medium)
                        .color(k.c.danger.fg),
                )
                .child(
                    text(error)
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_muted)
                        .selectable(true)
                        .wrap(),
                )
                .child(
                    Button::new("Retry connection")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut Viewer| s.reconnect())
                        .build(k)
                        .key("retry-connection"),
                ),
        );
    }
    let history = if s.history_loading {
        empty(
            "Loading conversation…",
            "Your conversation will appear here.",
            k,
        )
    } else if s.error.is_some() && s.messages.is_empty() {
        empty(
            "Conversation unavailable",
            "Reconnect or change the connection to continue.",
            k,
        )
    } else if s.bot_empty {
        empty(
            "No Bot Chat yet",
            "Send a message to start this bot’s own conversation.",
            k,
        )
    } else if s.selected_session.is_none() {
        empty(
            "Choose a conversation",
            "Select an existing chat or write a message to start a new conversation.",
            k,
        )
    } else if s.messages.is_empty() {
        empty(
            "No messages yet",
            "This conversation has no stored messages.",
            k,
        )
    } else {
        transcript::timeline(s, p)
    };
    if s.history_truncated {
        body = body.child(
            text(format!("Latest {} messages", s.messages.len()))
                .key("history-truncated")
                .font(k.ty.text_xsmall)
                .color(k.c.text_faint)
                .px(24.0)
                .py(6.0),
        );
    }
    body.child(history.grow(1.0).min_h(0.0))
        .child(conversation::pending(s, k))
        .child(audio::panel(s, k))
        .child(transcript::slash_panel(s, k))
        .child(subagents::strip(s, k))
        .child(composer::dock(s, p, k, layer))
}

fn empty(title: &str, detail: &str, k: &Kit) -> El {
    col()
        .key("empty-history")
        .grow(1.0)
        .w_full()
        .min_h(120.0)
        .center()
        .px(32.0)
        .gap(8.0)
        .child(icon(&ic::STATUS, 24.0).color(k.c.icon_faint))
        .child(text(title).font(k.ty.body_medium).color(k.c.text_base))
        .child(
            text(detail)
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .max_w(330.0)
                .wrap(),
        )
}

fn connection_form(s: &Viewer, k: &Kit) -> El {
    let url = TextInput::new(&s.connection_url)
        .placeholder("ws://host:port/api/ws")
        .width(None)
        .on_edit(|s: &mut Viewer| &mut s.connection_url)
        .on_submit(|s: &mut Viewer, _| s.apply_connection())
        .build(k)
        .key("connection-url")
        .label("Gateway WebSocket URL");
    let token = TextInput::new(&s.connection_token)
        .secure()
        .placeholder("Gateway token")
        .width(None)
        .on_edit(|s: &mut Viewer| &mut s.connection_token)
        .on_submit(|s: &mut Viewer, _| s.apply_connection())
        .build(k)
        .key("connection-token")
        .label("Gateway token");
    let local = TextInput::new(&s.connection_local_home)
        .placeholder("Hermes home directory")
        .width(None)
        .on_edit(|s: &mut Viewer| &mut s.connection_local_home)
        .on_submit(|s: &mut Viewer, _| s.apply_local_connection())
        .build(k)
        .key("connection-local-home")
        .label("Local Hermes home");
    let mut form = col()
        .key("connection-form")
        .role("group")
        .label("Connect to Hermes")
        .w_full()
        .max_w(440.0)
        .gap(12.0)
        .child(
            text("Connect to your Hermes")
                .font(k.ty.body_medium)
                .color(k.c.text_base),
        )
        .child(
            text("Open profiles, sessions and conversation history from an existing agent.")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .wrap(),
        )
        .child(
            Button::new("Saved connections and SSH")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Viewer| {
                    s.connection_form = false;
                    s.open_route(crate::pages::Route::Connections);
                })
                .build(k)
                .key("saved-connections"),
        )
        .child(
            text("Gateway URL")
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .pt(12.0),
        )
        .child(url)
        .child(token)
        .child(
            Button::new("Connect to gateway")
                .variant(ButtonVariant::Contrast)
                .on_press(|s: &mut Viewer| s.apply_connection())
                .build(k)
                .key("connect-url"),
        )
        .child(
            text("On this device")
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .pt(16.0),
        )
        .child(
            text("Connect to an already-running local gateway.")
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted)
                .wrap(),
        )
        .child(local)
        .child(
            Button::new("Connect local Hermes")
                .on_press(|s: &mut Viewer| s.apply_local_connection())
                .build(k)
                .key("connect-local"),
        );
    if let Some(error) = &s.error {
        form = form.child(
            text(error)
                .key("connection-form-error")
                .role("alert")
                .font(k.ty.text_xsmall)
                .color(k.c.danger.fg),
        );
    }
    if s.can_cancel_connection() {
        form = form.child(
            Button::new("Cancel")
                .variant(ButtonVariant::GhostMuted)
                .on_press(|s: &mut Viewer| s.cancel_connection())
                .build(k)
                .key("connection-cancel"),
        );
    }
    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .px(32.0)
        .py(28.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(form)
}
