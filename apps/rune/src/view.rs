//! Native Hermes reader. The transcript and composer rendering reuse OpenCode's
//! Rune example at 3f70cd6b675c89a8a7b64fc09b1bf2b455e0ad1a; see the modules.
//! Hermes owns the pane composition, connection/profile identities and actions.

mod palette;
mod transcript;

use std::borrow::Cow;
use std::rc::Rc;

use rune::{Cx, El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Sidebar, SidebarRow, SplitView, TextInput, Toolbar};
use rune_theme_opencode::icons as ic;

use crate::app::Viewer;
use palette::{Pal, pal};

pub fn app(s: &Viewer, cx: &mut Cx<Viewer>) -> El {
    let system_dark = cx.system_dark();
    let dark = s.theme.unwrap_or(system_dark);
    let k = rune_theme_opencode::kit(dark);
    let p = pal(dark);
    let title = if s.connection_form {
        "Connect to Hermes".to_owned()
    } else {
        s.selected_session
            .as_deref()
            .and_then(|id| s.sessions.iter().find(|session| session.id == id))
            .map(|session| session.title.clone())
            .unwrap_or_else(|| "Conversations".to_owned())
    };
    let toolbar = Toolbar::new()
        .leading(
            text(title)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate(),
        )
        .trailing(
            Button::new(if dark { "Light" } else { "Dark" })
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut Viewer| s.toggle_theme(system_dark))
                .build(k)
                .key("theme-toggle")
                .label(if dark {
                    "Switch to light theme"
                } else {
                    "Switch to dark theme"
                }),
        )
        .trailing(
            Button::new("Reconnect")
                .disabled(s.loading)
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Viewer| s.reconnect())
                .build(k)
                .key("reconnect"),
        );
    let main = if s.connection_form {
        connection_form(s, k)
    } else {
        workspace(s, p, k)
    };
    let split = SplitView::new(main)
        .sidebar(
            &s.sidebar,
            |s: &mut Viewer| &mut s.sidebar,
            sidebar(s, p, k),
        )
        .sidebar_header(text("Hermes").font(k.ty.ui_medium).color(k.c.text_base))
        .inspector(&s.inspector, |s: &mut Viewer| &mut s.inspector, tooling(k))
        .titlebar_inset(cx.titlebar_inset())
        .reduced_motion(cx.is_reduced_motion())
        .toolbar(toolbar)
        .build(k);
    col()
        .key("hermes-window")
        .role("window")
        .label("Hermes Desktop")
        .w_full()
        .h_full()
        .fill(k.c.bg_deep)
        .child(split.grow(1.0).min_h(0.0))
        .child(status_bar(s, k))
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

fn sidebar(s: &Viewer, p: &Pal, k: &Kit) -> El {
    let mut profiles = col()
        .key("profiles")
        .role("list")
        .label("Agent profiles")
        .focus_group()
        .focus_follows()
        .gap(2.0)
        .w_full();
    for profile in &s.profiles {
        let name = profile.name.clone();
        let label = if profile.label.is_empty() {
            &profile.name
        } else {
            &profile.label
        };
        profiles = profiles.child(
            SidebarRow::new(label)
                .avatar(transcript::avatar(p, label))
                .selected(s.profile == profile.name)
                .on_select(move |s: &mut Viewer, _| s.select_profile(name.clone()))
                .build(k)
                .key(format!("profile-{}", profile.name)),
        );
    }
    let profiles = if s.profiles.is_empty() {
        col().w_full().px(8.0).py(8.0).child(
            text(if s.loading {
                "Loading profiles…"
            } else {
                "No profiles loaded"
            })
            .font(k.ty.text_xsmall)
            .color(k.c.text_faint),
        )
    } else {
        profiles.max_h(160.0).scroll(ScrollSpec {
            stick_bottom: false,
        })
    };
    let filtered_count = filtered_indices(s).len();
    let header = col()
        .w_full()
        .gap(12.0)
        .child(
            col()
                .w_full()
                .gap(4.0)
                .child(
                    text("Profiles")
                        .font(k.ty.small_medium)
                        .color(k.c.text_muted)
                        .px(8.0),
                )
                .child(profiles),
        )
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
                    text("Recent sessions")
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
                let dark = s.theme.unwrap_or(inherited_dark);
                let k = rune_theme_opencode::kit(dark);
                let mut row = SidebarRow::new(&session.title).height(36.0);
                if let Some(date) = session_date(&session.updated) {
                    row = row.detail(date);
                }
                row.selected(s.selected_session.as_deref() == Some(session.id.as_str()))
                    .on_select(move |s: &mut Viewer, _| s.open_session(id.clone()))
                    .build(k)
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
    sidebar
        .footer(
            Button::new("Change connection")
                .icon(&ic::GLOBE)
                .variant(ButtonVariant::GhostMuted)
                .on_press(|s: &mut Viewer| s.change_connection())
                .build(k)
                .key("change-connection"),
        )
        .build(k)
}

fn workspace(s: &Viewer, p: &Pal, k: &Kit) -> El {
    let mut body = col().key("workspace").w_full().h_full().min_h(0.0);
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

fn status_bar(s: &Viewer, k: &Kit) -> El {
    row()
        .key("status-bar")
        .role("status")
        .label(&s.status)
        .h(28.0)
        .w_full()
        .px(14.0)
        .gap(10.0)
        .center_y()
        .fill(k.c.bg_deep)
        .child(
            text(&s.status)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate(),
        )
        .child(el().grow(1.0))
        .child(
            text(&s.endpoint_label)
                .font(k.ty.small)
                .color(k.c.text_faint)
                .truncate(),
        )
        .child(
            text("Read only")
                .font(k.ty.small_medium)
                .color(k.c.text_muted),
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
