//! Native Hermes reader. The transcript and composer rendering reuse OpenCode's
//! Rune example at 3f70cd6b675c89a8a7b64fc09b1bf2b455e0ad1a; see the modules.
//! Hermes owns the pane composition, connection/profile identities and actions.

mod palette;
mod shell;
mod transcript;

use std::borrow::Cow;
use std::rc::Rc;

use rune::{Cx, El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Sidebar, SidebarRow, TextInput};
use rune_theme_opencode::icons as ic;

use crate::app::{ConversationTarget, SidebarTab, Viewer};
use palette::{Pal, pal};

pub fn app(s: &Viewer, cx: &mut Cx<Viewer>) -> El {
    let system_dark = cx.system_dark();
    let dark = s.theme.unwrap_or(system_dark);
    let k = rune_theme_opencode::kit(dark);
    let p = pal(dark);
    let main = if s.connection_form {
        connection_form(s, k)
    } else {
        workspace(s, p, k)
    };
    let sidebar = match s.sidebar_tab {
        SidebarTab::Sessions => sessions_sidebar(s, p, k),
        SidebarTab::Bots => shell::bots_sidebar(s, p, k),
    };
    let split = shell::layout(s, cx, main, sidebar, tooling(k), k);
    let mut layer = Vec::new();
    let menu = shell::context_menu(s, k, &mut layer);
    col()
        .key("hermes-window")
        .role("window")
        .label("Hermes Desktop")
        .w_full()
        .h_full()
        .fill(k.c.bg_deep)
        .child(split.grow(1.0).min_h(0.0))
        .children((!s.simple_mode).then(|| shell::status_bar(s, k)))
        .child(menu)
        .children(layer)
}

fn filtered_indices(s: &Viewer) -> Vec<usize> {
    let query = s.search.text.trim().to_lowercase();
    s.sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| query.is_empty() || session.title.to_lowercase().contains(&query))
        .map(|(index, _)| index)
        .collect()
}

/// The gateway also accepts numeric/legacy timestamps; only a known ISO date
/// is useful as compact sidebar metadata without guessing its unit or timezone.
fn session_date(value: &str) -> Option<&str> {
    let date = value.get(..10)?;
    date.bytes()
        .enumerate()
        .all(|(index, byte)| {
            if index == 4 || index == 7 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
        .then_some(date)
}

fn sessions_sidebar(s: &Viewer, p: &Pal, k: &Kit) -> El {
    let filtered_count = filtered_indices(s).len();
    let header = col()
        .w_full()
        .gap(12.0)
        .child(shell::sessions_nav(s, k))
        .child(
            TextInput::new(&s.search)
                .placeholder("Search recent sessions")
                .leading_icon(&ic::MAGNIFYING_GLASS)
                .width(None)
                .on_edit(|s: &mut Viewer| &mut s.search)
                .build(k)
                .key("session-search")
                .label("Search recent sessions"),
        )
        .child(
            row()
                .w_full()
                .px(8.0)
                .center_y()
                .child(
                    text("Sessions")
                        .font(k.ty.small_medium)
                        .color(k.c.text_muted),
                )
                .child(el().grow(1.0))
                .child(
                    text(if s.search.text.trim().is_empty() {
                        s.sessions.len().to_string()
                    } else {
                        format!("{filtered_count} of {}", s.sessions.len())
                    })
                    .font(k.ty.small)
                    .color(k.c.text_faint),
                ),
        );
    let indices = Rc::new(filtered_indices(s));
    let inherited_dark = p.dark;
    let keys = indices.clone();
    let rows = indices.clone();
    let mut sidebar = Sidebar::new()
        .label("Conversation sessions")
        .header_padding(8.0, 8.0)
        .header(header)
        .row_height(36.0)
        .virtual_rows::<Viewer>(
            indices.len(),
            move |s, index| Cow::Owned(format!("session-{}", s.sessions[keys[index]].id)),
            move |s, index| {
                let session = &s.sessions[rows[index]];
                let id = session.id.clone();
                let menu_target = ConversationTarget::Session {
                    profile: s.profile.clone(),
                    id: id.clone(),
                    title: session.title.clone(),
                };
                let dark = s.theme.unwrap_or(inherited_dark);
                let k = rune_theme_opencode::kit(dark);
                let mut row = SidebarRow::new(&session.title).height(36.0);
                if let Some(date) = session_date(&session.updated) {
                    row = row.detail(date);
                }
                row.selected(s.selected_session.as_deref() == Some(session.id.as_str()))
                    .on_select(move |s: &mut Viewer, _| s.open_session(id.clone()))
                    .build(k)
                    .on_secondary_press(move |s: &mut Viewer, point, _| {
                        s.row_menu = Some((menu_target.clone(), point))
                    })
                    .key(format!("session-{}", session.id))
            },
        );
    if indices.is_empty() {
        sidebar = sidebar.child(
            text(if s.loading {
                "Loading sessions…"
            } else if !s.search.text.trim().is_empty() {
                "No matching sessions"
            } else {
                "No sessions in this profile"
            })
            .font(k.ty.text_xsmall)
            .color(k.c.text_faint)
            .px(8.0)
            .py(12.0),
        );
    }
    sidebar.footer(shell::profile_rail(s, p, k)).build(k)
}

fn workspace(s: &Viewer, p: &Pal, k: &Kit) -> El {
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
            "This bot has no existing Bot Chat. Starting its conversation will be available in the sending increment.",
            k,
        )
    } else if s.selected_session.is_none() {
        empty(
            "Choose a conversation",
            "Select a profile and a session to read its history.",
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
        .child(transcript::readonly_composer(p))
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

fn tooling(k: &Kit) -> El {
    col()
        .key("tooling")
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(1.0)
        .fill(k.c.border_muted)
        .child(
            row()
                .key("upper-tools")
                .w_full()
                .h(0.0)
                .grow(1.6)
                .min_h(0.0)
                .gap(1.0)
                .child(
                    tool_panel(
                        "review",
                        "Review",
                        "Review is unavailable in this viewer.",
                        &ic::BRANCH,
                        k,
                    )
                    .w(0.0)
                    .grow(1.0)
                    .min_w(0.0),
                )
                .child(
                    tool_panel(
                        "files",
                        "Files",
                        "File browsing is unavailable in this viewer.",
                        &ic::FOLDER,
                        k,
                    )
                    .w(0.0)
                    .grow(1.2)
                    .min_w(0.0),
                ),
        )
        .child(
            tool_panel(
                "terminal",
                "Terminal",
                "Terminal access is unavailable in this viewer.",
                &ic::CONSOLE,
                k,
            )
            .h(0.0)
            .grow(1.0)
            .min_h(0.0),
        )
}

fn tool_panel(key: &str, label: &str, detail: &str, glyph: &'static rune::Icon, k: &Kit) -> El {
    col()
        .key(key)
        .role("group")
        .label(label)
        .w_full()
        .h_full()
        .fill(k.c.bg_deep)
        .child(
            row()
                .w_full()
                .h(36.0)
                .px(12.0)
                .gap(6.0)
                .center_y()
                .child(icon(glyph, 14.0).color(k.c.icon_muted))
                .child(
                    text(label)
                        .font(k.ty.text_xsmall_medium)
                        .color(k.c.text_muted),
                ),
        )
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .px(12.0)
                .py(16.0)
                .gap(8.0)
                .child(
                    text(detail)
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_faint)
                        .wrap(),
                ),
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
