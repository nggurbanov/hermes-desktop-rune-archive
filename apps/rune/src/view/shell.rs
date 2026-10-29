//! Hermes shell anatomy, from the retained Electron client:
//! Sessions/Bots share the left zone; profile scope lives at its foot;
//! conversations are workspace tabs. OpenCode supplies controls and tokens.
use rune::{Color, Cursor, Cx, Drag, El, Phase, ScrollSpec, col, el, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Menu, MenuItem,
};
use rune_theme_opencode::icons as ic;

use super::{palette::Pal, transcript};
use crate::app::{ConversationTarget, SidebarTab, Viewer};

const BAND: f32 = 36.0;

pub fn layout(s: &Viewer, cx: &mut Cx<Viewer>, main: El, sidebar: El, tools: El, k: &Kit) -> El {
    let mut header = row()
        .key("hermes-titlebar")
        .w_full()
        .h(BAND)
        .center_y()
        .window_drag()
        .fill(k.c.bg_deep);
    if s.sidebar.is_open() {
        header = header.child(
            row()
                .key("sidebar-titlebar")
                .w(s.sidebar.width())
                .h_full()
                .pl(cx.titlebar_inset() + 8.0)
                .pr(8.0)
                .gap(4.0)
                .center_y()
                .child(sidebar_toggle(k))
                .child(text("Hermes").font(k.ty.ui_medium).color(k.c.text_muted)),
        );
    } else {
        header = header
            .child(el().w(cx.titlebar_inset() + 8.0))
            .child(sidebar_toggle(k));
    }
    header = header
        .child(workspace_tabs(s, k).grow(1.0).min_w(0.0))
        .child(
            row()
                .key("shell-controls")
                .gap(4.0)
                .pr(8.0)
                .center_y()
                .child(
                    Button::new(if s.simple_mode { "Simple" } else { "Advanced" })
                        .variant(ButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .help("Switch Hermes interface mode")
                        .on_press(|s: &mut Viewer| s.set_simple_mode(!s.simple_mode))
                        .build(k)
                        .key("interface-mode"),
                )
                .child(
                    Button::new(if s.theme.unwrap_or(cx.system_dark()) {
                        "Light"
                    } else {
                        "Dark"
                    })
                    .variant(ButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .on_press({
                        let system = cx.system_dark();
                        move |s: &mut Viewer| s.toggle_theme(system)
                    })
                    .build(k)
                    .key("theme-toggle"),
                )
                .child(
                    IconButton::new(&ic::SETTINGS_GEAR)
                        .variant(IconButtonVariant::GhostMuted)
                        .label("Settings, not yet migrated")
                        .help("Settings will be migrated in a later increment")
                        .disabled(true)
                        .build(k)
                        .key("settings"),
                )
                .children(s.simple_mode.then(|| {
                    IconButton::new(&ic::GLOBE)
                        .variant(IconButtonVariant::GhostMuted)
                        .label("Change connection")
                        .on_press(|s: &mut Viewer| s.change_connection())
                        .build(k)
                        .key("change-connection")
                }))
                .child(
                    IconButton::new(&ic::LAYOUT_LEFT)
                        .variant(IconButtonVariant::GhostMuted)
                        .label("Toggle tool panes")
                        .help("Show or hide Review, Files and Terminal")
                        .on_press(|s: &mut Viewer| s.inspector.toggle())
                        .build(k)
                        .key("tooling-toggle"),
                ),
        );
    let mut body = row().key("hermes-panes").w_full().grow(1.0).min_h(0.0);
    if s.sidebar.is_open() {
        body = body.child(
            col()
                .key("sessions-zone")
                .w(s.sidebar.width())
                .h_full()
                .min_h(0.0)
                .fill(k.c.bg_deep)
                .child(
                    row()
                        .key("sidebar-tabs")
                        .role("tablist")
                        .label("Sessions and bots")
                        .w_full()
                        .h(32.0)
                        .px(8.0)
                        .gap(4.0)
                        .center_y()
                        .child(sidebar_tab(
                            "Sessions",
                            "sessions-tab",
                            SidebarTab::Sessions,
                            s,
                            k,
                        ))
                        .child(sidebar_tab("Bots", "bots-tab", SidebarTab::Bots, s, k)),
                )
                .child(sidebar.grow(1.0).min_h(0.0))
                .child(resize_grip(true, k)),
        );
    }
    body = body.child(
        col()
            .key("workspace-zone")
            .w(0.0)
            .grow(1.0)
            .min_w(0.0)
            .h_full()
            .fill(k.c.bg_base)
            .child(main),
    );
    if s.inspector.is_open() {
        body = body.child(
            col()
                .key("tools-zone")
                .w(s.inspector.width())
                .h_full()
                .min_h(0.0)
                .child(tools)
                .child(resize_grip(false, k)),
        );
    }
    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(header)
        .child(el().h(1.0).w_full().fill(k.c.border_muted))
        .child(body)
}

fn sidebar_toggle(k: &Kit) -> El {
    IconButton::new(&ic::SIDEBAR)
        .variant(IconButtonVariant::GhostMuted)
        .label("Toggle sessions and bots")
        .help("Show or hide the left sidebar")
        .on_press(|s: &mut Viewer| s.sidebar.toggle())
        .build(k)
        .key("sidebar-toggle")
}

fn sidebar_tab(label: &str, key: &str, tab: SidebarTab, s: &Viewer, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut Viewer| s.sidebar_tab = tab)
        .build(k)
        .key(key)
        .role("tab")
        .focus_selected(s.sidebar_tab == tab)
        .fill(if s.sidebar_tab == tab {
            k.c.layer_01
        } else {
            Color::TRANSPARENT
        })
}

fn workspace_tabs(s: &Viewer, k: &Kit) -> El {
    let mut tabs = row()
        .key("workspace-tabs")
        .role("tablist")
        .label("Open conversations")
        .h_full()
        .gap(4.0)
        .px(8.0)
        .center_y()
        .scroll_x();
    if s.connection_form {
        return tabs.child(
            text("Connect to Hermes")
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        );
    }
    if s.tabs.is_empty() {
        return tabs.child(text("Hermes").font(k.ty.ui_medium).color(k.c.text_muted));
    }
    for (index, tab) in s.tabs.iter().enumerate() {
        let active = tab.profile == s.profile && s.selected_session.as_ref() == Some(&tab.session);
        let owner = s
            .profiles
            .iter()
            .find(|p| p.name == tab.profile)
            .map(|p| p.label.as_str())
            .unwrap_or(&tab.profile);
        tabs = tabs.child(
            row()
                .key(format!("conversation-tab-{index}"))
                .center_y()
                .h(32.0)
                .radius(6.0)
                .fill(if active {
                    k.c.bg_base
                } else {
                    Color::TRANSPARENT
                })
                .child(
                    Button::new(&tab.title)
                        .variant(ButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .help(format!(
                            "{} · {}",
                            owner,
                            if tab.bot { "Bot Chat" } else { "Session" }
                        ))
                        .on_press(move |s: &mut Viewer| s.activate_tab(index))
                        .build(k)
                        .key(format!("activate-tab-{index}"))
                        .role("tab")
                        .focus_selected(active)
                        .max_w(190.0),
                )
                .child(
                    IconButton::new(&ic::XMARK_SMALL)
                        .variant(IconButtonVariant::GhostMuted)
                        .control_size(ControlSize::Mini)
                        .label(format!("Close {}", tab.title))
                        .on_press(move |s: &mut Viewer| s.close_tab(index))
                        .build(k)
                        .key(format!("close-tab-{index}")),
                ),
        );
    }
    tabs
}

fn resize_grip(leading: bool, k: &Kit) -> El {
    el().key(if leading {
        "sidebar-grip"
    } else {
        "inspector-grip"
    })
    .role("separator")
    .label(if leading {
        "Resize sidebar"
    } else {
        "Resize tools"
    })
    .w(5.0)
    .pin(
        if leading { None } else { Some(0.0) },
        Some(0.0),
        if leading { Some(0.0) } else { None },
        Some(0.0),
    )
    .cursor(Cursor::ResizeColumn)
    .hover(k.c.border_muted)
    .on_drag(move |s: &mut Viewer, d: &mut Drag, tx: &mut rune::Tx| {
        let pane = if leading {
            &mut s.sidebar
        } else {
            &mut s.inspector
        };
        match d.phase {
            Phase::Down => d.data = pane.width(),
            Phase::Move => {
                pane.set_width(d.data + if leading { d.dx } else { -d.dx });
                tx.instant();
            }
            Phase::Up => {
                if d.cancelled() {
                    pane.set_width(d.data);
                }
            }
        }
    })
    .on_double_press(move |s: &mut Viewer| {
        if leading {
            s.sidebar.reset_width();
        } else {
            s.inspector.reset_width();
        }
    })
}

pub fn sessions_nav(s: &Viewer, k: &Kit) -> El {
    let mut nav = col()
        .key("sessions-navigation")
        .role("navigation")
        .label("Hermes navigation")
        .w_full()
        .gap(2.0);
    for (key, label, glyph, advanced) in [
        ("new-session", "New session", &ic::PLUS, false),
        ("capabilities", "Capabilities", &ic::BRAIN, false),
        ("messaging", "Messaging", &ic::GLOBE, false),
        ("artifacts", "Artifacts", &ic::OUTLINE_COPY, true),
        ("cron", "Scheduled jobs", &ic::CHECKLIST, true),
    ] {
        if advanced && s.simple_mode {
            continue;
        }
        nav = nav.child(
            Button::new(label)
                .icon(glyph)
                .variant(ButtonVariant::GhostMuted)
                .disabled(true)
                .help("This section will be migrated in a later increment")
                .build(k)
                .w_full()
                .h(30.0)
                .justify_start()
                .key(format!("nav-{key}")),
        );
    }
    nav
}

pub fn profile_rail(s: &Viewer, p: &Pal, k: &Kit) -> El {
    let mut profiles = row().grow(1.0).min_w(0.0).gap(4.0).center_y().scroll_x();
    for profile in s.profiles.iter().take(12) {
        let name = profile.name.clone();
        profiles = profiles.child(
            el().key(format!("profile-{}", name))
                .role("button")
                .label(format!("Sessions in {}", profile.label))
                .help(&profile.label)
                .focusable(true)
                .focus_ring(2.0, 1.0, k.c.border_focus)
                .size(28.0, 28.0)
                .center()
                .radius(6.0)
                .fill(if s.profile == name {
                    k.c.layer_01
                } else {
                    Color::TRANSPARENT
                })
                .hover(k.c.hover)
                .on_press(move |s: &mut Viewer| s.select_profile(name.clone()))
                .child(transcript::avatar(p, &profile.label)),
        );
    }
    let rail = row()
        .key("profile-rail")
        .role("group")
        .label("Session profile scope")
        .w_full()
        .h(40.0)
        .px(4.0)
        .gap(4.0)
        .center_y()
        .child(profiles)
        .child(
            IconButton::new(&ic::CHEVRON_DOWN)
                .variant(IconButtonVariant::GhostMuted)
                .label("Choose session profile")
                .on_press(|s: &mut Viewer| s.profile_picker = !s.profile_picker)
                .build(k)
                .key("profile-picker-toggle"),
        );
    let mut footer = col().key("profile-footer").w_full();
    if s.profile_picker {
        let mut picker = col()
            .key("profile-picker")
            .w_full()
            .max_h(180.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            });
        for profile in &s.profiles {
            let name = profile.name.clone();
            picker = picker.child(
                Button::new(&profile.label)
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(move |s: &mut Viewer| s.select_profile(name.clone()))
                    .build(k)
                    .key(format!("pick-profile-{}", profile.name)),
            );
        }
        footer = footer.child(picker);
    }
    footer
        .child(el().w_full().h(1.0).fill(k.c.border_muted))
        .child(rail)
}

pub fn bots_sidebar(s: &Viewer, p: &Pal, k: &Kit) -> El {
    let mut roster = col()
        .key("bots-roster")
        .role("list")
        .label("Hermes bots")
        .w_full()
        .gap(4.0)
        .child(
            row()
                .w_full()
                .h(40.0)
                .px(12.0)
                .center_y()
                .child(text("Bots").font(k.ty.ui_medium).color(k.c.text_base))
                .child(el().grow(1.0))
                .child(
                    IconButton::new(&ic::PLUS)
                        .variant(IconButtonVariant::GhostMuted)
                        .disabled(true)
                        .label("New bot, not yet migrated")
                        .help("Creating bots will be migrated in a later increment")
                        .build(k)
                        .key("new-bot"),
                ),
        );
    for bot in &s.profiles {
        let name = bot.name.clone();
        let active = s.bot_is_active(&bot.name);
        let menu_target = ConversationTarget::Bot(name.clone());
        let preview = bot
            .canonical_session
            .as_ref()
            .map(|c| c.preview.as_str())
            .filter(|p| !p.is_empty())
            .unwrap_or(&bot.description);
        roster = roster.child(
            row()
                .key(format!("bot-{}", name))
                .role("listitem")
                .label(format!("Open {} Bot Chat", bot.label))
                .focusable(true)
                .focus_selected(active)
                .focus_ring(2.0, 1.0, k.c.border_focus)
                .w_full()
                .min_h(64.0)
                .px(12.0)
                .py(10.0)
                .gap(10.0)
                .center_y()
                .radius(6.0)
                .fill(if active {
                    k.c.layer_01
                } else {
                    Color::TRANSPARENT
                })
                .hover(k.c.hover)
                .on_press(move |s: &mut Viewer| s.open_bot(name.clone()))
                .on_secondary_press(move |s: &mut Viewer, point, _| {
                    s.row_menu = Some((menu_target.clone(), point))
                })
                .child(transcript::avatar(p, &bot.label).size(28.0, 28.0))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(
                            text(&bot.label)
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate(),
                        )
                        .child(
                            text(if preview.is_empty() {
                                "No Bot Chat yet"
                            } else {
                                preview
                            })
                            .font(k.ty.text_xsmall)
                            .color(k.c.text_faint)
                            .truncate(),
                        ),
                ),
        );
    }
    if s.profiles.is_empty() {
        roster = roster.child(
            text(if s.loading {
                "Loading bots…"
            } else {
                "Connect an agent to see its bots"
            })
            .font(k.ty.ui)
            .color(k.c.text_faint)
            .wrap()
            .px(12.0)
            .py(16.0),
        );
    }
    col()
        .key("bots-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .px(8.0)
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(roster),
        )
        .child(
            row()
                .w_full()
                .h(36.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .child(icon(&ic::GLOBE, 12.0).color(k.c.icon_faint))
                .child(
                    text(&s.endpoint_label)
                        .font(k.ty.small)
                        .color(k.c.text_faint)
                        .truncate(),
                ),
        )
}

pub fn conversation_context(s: &Viewer, p: &Pal, k: &Kit) -> El {
    let bot = s.bot_is_active(&s.profile);
    let profile = s.profiles.iter().find(|p| p.name == s.profile);
    let label = profile.map(|p| p.label.as_str()).unwrap_or("Hermes");
    row()
        .key("conversation-context")
        .w_full()
        .h(48.0)
        .px(24.0)
        .gap(10.0)
        .center_y()
        .child(transcript::avatar(p, label).size(24.0, 24.0))
        .child(text(label).font(k.ty.ui_medium).color(k.c.text_base))
        .child(
            text(if bot { "Bot Chat" } else { "Session" })
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
        .child(el().grow(1.0))
        .child(text("Read only").font(k.ty.small).color(k.c.text_faint))
}

pub fn context_menu(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let Some((target, point)) = s.row_menu.clone() else {
        return el().size(0.0, 0.0);
    };
    let in_place = target.clone();
    Menu::new("conversation-actions")
        .open(true)
        .at(point.x, point.y)
        .on_close(|s: &mut Viewer| s.row_menu = None)
        .item(
            MenuItem::new("Open")
                .on_select(move |s: &mut Viewer| s.open_target(in_place.clone(), false)),
        )
        .item(
            MenuItem::new("Open in new tab")
                .on_select(move |s: &mut Viewer| s.open_target(target.clone(), true)),
        )
        .build(k, layer)
}

pub fn status_bar(s: &Viewer, k: &Kit) -> El {
    row()
        .key("status-bar")
        .role("status")
        .label(&s.status)
        .w_full()
        .h(28.0)
        .px(8.0)
        .gap(10.0)
        .center_y()
        .fill(k.c.bg_deep)
        .child(
            Button::new(&s.endpoint_label)
                .icon(&ic::GLOBE)
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut Viewer| s.change_connection())
                .build(k)
                .key("change-connection"),
        )
        .child(
            Button::new("Reconnect")
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .disabled(s.loading)
                .on_press(|s: &mut Viewer| s.reconnect())
                .build(k)
                .key("reconnect"),
        )
        .child(
            text(&s.status)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate(),
        )
        .child(el().grow(1.0))
        .child(
            text("Hermes · Read only")
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
}
