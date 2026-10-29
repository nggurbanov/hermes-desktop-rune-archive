//! Native Capabilities retains four modes and a profile-local master/detail workflow.
mod desktop;
mod detail;
mod logs;
use crate::{
    app::Viewer,
    pages::capabilities::{
        Action, Capabilities, Confirmation, Mode, Selection, flag, rows, string, strings,
    },
};
use rune::{Color, El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextInput};
use serde_json::Value;

pub(super) fn button(label: impl Into<String>, action: Action, disabled: bool, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled)
        .on_press_tx(move |s: &mut Viewer, tx| {
            s.capabilities_action(action.clone());
            tx.instant();
        })
        .build(k)
}
pub(super) fn notice(label: impl Into<String>, k: &Kit) -> El {
    text(label).font(k.ty.small).color(k.c.text_muted).wrap()
}
pub(super) fn heading(label: impl Into<String>, k: &Kit) -> El {
    text(label).font(k.ty.ui_medium).color(k.c.text_base)
}
pub(super) fn confirm(
    title: impl Into<String>,
    description: impl Into<String>,
    action: Action,
) -> Action {
    Action::RequestConfirmation(Box::new(Confirmation {
        title: title.into(),
        description: description.into(),
        action,
    }))
}

pub fn page(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let state = &s.capabilities;
    let mut header = row()
        .w_full()
        .center_y()
        .gap(10.0)
        .child(
            text("Capabilities")
                .font(k.ty.body_medium)
                .color(k.c.text_base),
        )
        .child(el().grow(1.0));
    if s.profiles.len() > 1 {
        let options: Vec<String> = s.profiles.iter().map(|p| p.name.clone()).collect();
        header = header.child(notice("Configuring profile", k)).child(
            Select::new("capabilities-profile", &options)
                .current(Some(state.profile.as_deref().unwrap_or(&s.profile)))
                .width(170.0)
                .open(state.profile_menu)
                .disabled(state.busy())
                .on_toggle(|s: &mut Viewer| {
                    s.capabilities.profile_menu = !s.capabilities.profile_menu
                })
                .on_close(|s: &mut Viewer| s.capabilities.profile_menu = false)
                .on_choose(|s: &mut Viewer, v, tx| {
                    s.capabilities_action(Action::Profile(v.into()));
                    tx.instant();
                })
                .build(k, layer),
        );
    }
    header = header
        .child(button("Refresh", Action::Refresh, state.busy(), k).key("capabilities-refresh"));
    let mut tabs = row().gap(5.0).w_full();
    for mode in Mode::ALL {
        tabs = tabs.child(
            button(mode.label(), Action::Mode(mode), state.busy(), k)
                .key(format!("capabilities-tab-{}", mode.label().to_lowercase()))
                .fill(if state.mode == mode {
                    k.c.layer_01
                } else {
                    Color::TRANSPARENT
                }),
        );
    }
    let search = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            TextInput::new(&state.query)
                .placeholder("Search capabilities…")
                .width(None)
                .on_edit(|s: &mut Viewer| &mut s.capabilities.query)
                .on_submit(|s: &mut Viewer, _| {
                    if s.capabilities.mode == Mode::Skills {
                        s.capabilities_action(Action::SearchHub);
                    }
                })
                .build(k)
                .grow(1.0)
                .min_w(140.0)
                .key("capabilities-search"),
        )
        .children((state.mode == Mode::Skills).then(|| {
            button(
                "Search Skills Hub",
                Action::SearchHub,
                state.busy() || state.demo,
                k,
            )
            .key("capabilities-search-hub")
        }))
        .children((state.mode == Mode::Plugins).then(|| {
            button(
                "Install from source",
                Action::Select(Selection::InstallPlugin),
                state.busy(),
                k,
            )
        }))
        .children((state.mode == Mode::Connectors).then(|| {
            button(
                "Add MCP server",
                Action::Select(Selection::AddMcp),
                state.busy(),
                k,
            )
        }));
    let mut body = col()
        .key("capabilities-page")
        .role("group")
        .label("Capabilities")
        .w_full()
        .h_full()
        .min_h(0.0)
        .px(24.0)
        .py(20.0)
        .gap(14.0)
        .child(header)
        .child(tabs)
        .child(search);
    if state.mode == Mode::Plugins {
        body = body.child(
            row()
                .gap(8.0)
                .child(desktop::bots(s, k))
                .child(desktop::installed(s, k))
                .child(desktop::open(s, k)),
        );
    }
    if state.demo {
        body = body.child(notice(
            "Demonstration data · backend writes are disabled",
            k,
        ));
    }
    if state.busy() {
        body = body.child(notice(
            if state.action_running {
                "An operation is running on the selected agent."
            } else {
                "Applying changes to the selected agent…"
            },
            k,
        ));
    }
    if let Some(message) = &state.message {
        body = body.child(notice(message, k).key("capabilities-notice"));
    }
    for (op, error) in &state.errors {
        body = body.child(
            text(format!(
                "{}: {}",
                op.trim_start_matches("capabilities."),
                error
            ))
            .role("alert")
            .font(k.ty.small)
            .color(k.c.danger.fg)
            .wrap(),
        );
    }
    if let Some(c) = &state.confirm {
        body = body.child(
            col()
                .gap(8.0)
                .w_full()
                .px(12.0)
                .py(12.0)
                .fill(k.c.layer_01)
                .child(heading(&c.title, k))
                .child(notice(&c.description, k))
                .child(
                    row()
                        .gap(8.0)
                        .child(
                            button("Confirm", Action::Confirm, state.busy(), k)
                                .key("capabilities-confirm"),
                        )
                        .child(button("Cancel", Action::Cancel, false, k)),
                ),
        );
    }
    let mut content = row().grow(1.0).min_h(0.0).w_full().gap(20.0);
    let list = list(s, k).grow(1.0).min_w(260.0).min_h(0.0).h_full();
    content = content.child(if state.selection.is_some() {
        list.max_w(400.0)
    } else {
        list
    });
    if state.selection.is_some() {
        content = content.child(
            detail::panel(s, k, layer)
                .grow(1.1)
                .min_w(280.0)
                .h_full()
                .min_h(0.0),
        );
    }
    body = body.child(content);
    if state.action_name.is_some() {
        body = body
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(notice(
                        if state.action_running {
                            "Backend action in progress"
                        } else {
                            "Last backend action"
                        },
                        k,
                    ))
                    .child(button("Refresh action log", Action::PollAction, false, k)),
            )
            .child(
                text(state.action_log.join("\n"))
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap()
                    .max_h(130.0)
                    .scroll(ScrollSpec { stick_bottom: true }),
            );
    }
    body
}

fn list(s: &Viewer, k: &Kit) -> El {
    let state = &s.capabilities;
    let mut list = col().w_full().gap(4.0).scroll(ScrollSpec {
        stick_bottom: false,
    });
    let query = state.query.text.trim().to_lowercase();
    match state.mode {
        Mode::Skills => {
            list=list.child(row().w_full().gap(8.0).center_y().child(heading("Installed",k)).child(el().grow(1.0))
                .child(button("Enable all",Action::BulkSkills(true),state.demo||state.busy()||state.skills.is_empty(),k))
                .child(button("Disable all",confirm("Disable every skill?","This applies to the whole selected profile, including skills outside the search results.",Action::BulkSkills(false)),state.demo||state.busy()||state.skills.is_empty(),k)))
                .child(button("Update installed Hub skills",confirm("Update installed skills?","The agent will check the configured sources and update installed Hub skills.",Action::UpdateSkills),state.demo||state.busy(),k));
            for skill in state.skills.iter().filter(|v| matches_query(v, &query)) {
                let name = string(skill, "name");
                let enabled = flag(skill, "enabled");
                let title = title_row(
                    &name,
                    &string(skill, "description"),
                    &format!(
                        "{} · {}",
                        string(skill, "category"),
                        string(skill, "provenance")
                    ),
                    Selection::Skill(name.clone()),
                    state,
                    k,
                )
                .child(
                    Switch::new(enabled)
                        .label(format!("Enable {name}"))
                        .hide_label()
                        .disabled(state.demo || state.busy())
                        .on_toggle(move |s: &mut Viewer| {
                            s.capabilities_action(Action::ToggleSkill(name.clone(), !enabled))
                        })
                        .build(k),
                );
                list = list.child(title);
            }
            if state.skills.is_empty() {
                list = list.child(empty_or_loading(
                    state,
                    "skills",
                    "No installed skills. Browse the available skills below or search the Hub.",
                    k,
                ));
            }
            list = list.child(heading("Available to install", k).pt(16.0));
            for item in state
                .official
                .iter()
                .filter(|v| !flag(v, "installed") && matches_query(v, &query))
            {
                let id = string(item, "identifier");
                list = list.child(title_row(
                    &string(item, "name"),
                    &string(item, "description"),
                    &string(item, "category"),
                    Selection::Hub(id),
                    state,
                    k,
                ));
            }
            if !state.hub.is_empty() {
                list = list.child(heading("Skills Hub results", k).pt(16.0));
            }
            for item in state.hub.iter().filter(|v| matches_query(v, &query)) {
                let id = string(item, "identifier");
                list = list.child(title_row(
                    &string(item, "name"),
                    &string(item, "description"),
                    &string(item, "source"),
                    Selection::Hub(id),
                    state,
                    k,
                ));
            }
        }
        Mode::Toolsets => {
            for item in state.toolsets.iter().filter(|v| matches_query(v, &query)) {
                let name = string(item, "name");
                let enabled = flag(item, "enabled");
                let label = nonempty(string(item, "label"), &name);
                list = list.child(
                    title_row(
                        &label,
                        &string(item, "description"),
                        if flag(item, "configured") {
                            "Configured"
                        } else {
                            "Needs setup"
                        },
                        Selection::Toolset(name.clone()),
                        state,
                        k,
                    )
                    .child(
                        Switch::new(enabled)
                            .label(format!("Enable {label}"))
                            .hide_label()
                            .disabled(state.demo || state.busy())
                            .on_toggle(move |s: &mut Viewer| {
                                s.capabilities_action(Action::ToggleToolset(name.clone(), !enabled))
                            })
                            .build(k),
                    ),
                );
            }
            if state.toolsets.is_empty() {
                list = list.child(empty_or_loading(
                    state,
                    "toolsets",
                    "No toolsets were returned by this agent.",
                    k,
                ));
            }
        }
        Mode::Plugins => {
            for item in state.plugins.iter().filter(|v| matches_query(v, &query)) {
                let key = string(item, "key");
                let name = string(item, "name");
                let enabled = string(item, "status") == "enabled";
                list = list.child(
                    title_row(
                        &name,
                        &string(item, "description"),
                        &format!("{} · {}", string(item, "version"), string(item, "source")),
                        Selection::Plugin(key.clone()),
                        state,
                        k,
                    )
                    .child(
                        Switch::new(enabled)
                            .label(format!("Enable {name}"))
                            .hide_label()
                            .disabled(state.demo || state.busy() || key.is_empty())
                            .on_toggle(move |s: &mut Viewer| {
                                s.capabilities_action(Action::TogglePlugin(key.clone(), !enabled))
                            })
                            .build(k),
                    ),
                );
            }
            if state.plugins.is_empty() {
                list = list.child(empty_or_loading(
                    state,
                    "plugins",
                    "No manageable agent plugins are installed. Install a source to add one.",
                    k,
                ));
            }
        }
        Mode::Connectors => {
            list = list.child(heading("Connected services", k));
            let joined = joined_connectors(state);
            for item in joined.iter().filter(|v| matches_query(v, &query)) {
                let slug = string(item, "connector");
                let catalog = state
                    .connector_catalog
                    .iter()
                    .find(|v| string(v, "slug") == slug);
                let title = catalog
                    .map(|v| string(v, "name"))
                    .unwrap_or_else(|| slug.clone());
                let description = catalog
                    .map(|v| string(v, "description"))
                    .unwrap_or_default();
                let status = if flag(item, "connected") {
                    "Connected"
                } else {
                    "Not connected"
                };
                list = list.child(title_row(
                    &title,
                    &description,
                    status,
                    Selection::Connector(slug),
                    state,
                    k,
                ));
            }
            if state.connectors.is_empty() {
                list=list.child(empty_or_loading(state,"connectors","No connected services were returned. Sign in through the agent's account flow to discover hosted connectors.",k));
            }
            list = list.child(heading("Local and remote MCP servers", k).pt(16.0));
            for item in state.mcp.iter().filter(|v| matches_query(v, &query)) {
                let name = string(item, "name");
                let enabled = flag(item, "enabled");
                let source = string(item, "source");
                list = list.child(
                    title_row(
                        &name,
                        &nonempty(string(item, "url"), &string(item, "command")),
                        &format!("{} · {}", string(item, "transport"), source),
                        Selection::Mcp(name.clone()),
                        state,
                        k,
                    )
                    .child(
                        Switch::new(enabled)
                            .label(format!("Enable {name}"))
                            .hide_label()
                            .disabled(state.demo || state.busy() || source == "plugin")
                            .on_toggle(move |s: &mut Viewer| {
                                s.capabilities_action(Action::ToggleMcp(name.clone(), !enabled))
                            })
                            .build(k),
                    ),
                );
            }
            if state.mcp.is_empty() {
                list = list.child(empty_or_loading(
                    state,
                    "mcp",
                    "No MCP servers are configured. Add a server by URL or command.",
                    k,
                ));
            }
            list = list.child(button(
                "Advanced mcp.json",
                Action::Select(Selection::AdvancedMcp),
                state.busy(),
                k,
            ));
            list = list.child(heading("MCP catalog", k).pt(16.0));
            for item in state
                .mcp_catalog
                .iter()
                .filter(|v| matches_query(v, &query) && !flag(v, "installed"))
            {
                let name = string(item, "name");
                let needs_keys = !rows(item, "required_env").is_empty();
                list = list.child(
                    row()
                        .w_full()
                        .gap(10.0)
                        .px(10.0)
                        .py(10.0)
                        .center_y()
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(4.0)
                                .child(heading(&name, k))
                                .child(notice(string(item, "description"), k)),
                        )
                        .child(button(
                            if needs_keys { "Configure" } else { "Install" },
                            if needs_keys {
                                Action::Select(Selection::McpCatalog(name.clone()))
                            } else {
                                confirm(
                                    format!("Install {name}?"),
                                    string(item, "post_install"),
                                    Action::InstallMcpCatalog(name),
                                )
                            },
                            state.demo || state.busy(),
                            k,
                        )),
                );
            }
        }
    }
    list
}
fn title_row(
    title: &str,
    description: &str,
    meta: &str,
    selection: Selection,
    state: &Capabilities,
    k: &Kit,
) -> El {
    let selected = state.selection.as_ref() == Some(&selection);
    let key = match &selection {
        Selection::Skill(name) => format!("capability-skill-{name}"),
        Selection::Hub(name) => format!("capability-hub-{name}"),
        Selection::Toolset(name) => format!("capability-tool-{name}"),
        Selection::Plugin(name) => format!("capability-plugin-{name}"),
        Selection::Mcp(name) => format!("capability-mcp-{name}"),
        Selection::Connector(name) => format!("capability-connector-{name}"),
        _ => "capability-action".into(),
    };
    row()
        .key(key)
        .w_full()
        .min_w(0.0)
        .min_h(66.0)
        .gap(10.0)
        .px(10.0)
        .py(11.0)
        .radius(7.0)
        .center_y()
        .role("button")
        .label(title)
        .focusable(true)
        .focus_ring(2.0, 1.0, k.c.border_focus)
        .fill(if selected {
            k.c.layer_01
        } else {
            Color::TRANSPARENT
        })
        .hover(k.c.hover)
        .on_press(move |s: &mut Viewer| s.capabilities_action(Action::Select(selection.clone())))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(heading(title, k).truncate())
                .child(notice(description, k))
                .child(notice(meta, k)),
        )
}
fn empty_or_loading(state: &Capabilities, key: &str, empty: &str, k: &Kit) -> El {
    notice(
        if state.pending.contains(&format!("capabilities.{key}")) {
            "Loading from the selected agent…"
        } else {
            empty
        },
        k,
    )
    .py(12.0)
}
fn matches_query(value: &Value, query: &str) -> bool {
    query.is_empty()
        || [
            "name",
            "label",
            "description",
            "category",
            "connector",
            "slug",
        ]
        .iter()
        .any(|key| string(value, key).to_lowercase().contains(query))
        || strings(&value["tools"])
            .iter()
            .any(|v| v.to_lowercase().contains(query))
}
fn nonempty(value: String, fallback: &str) -> String {
    if value.is_empty() {
        fallback.into()
    } else {
        value
    }
}

fn joined_connectors(state: &Capabilities) -> Vec<Value> {
    let mut rows = state.connectors.clone();
    for catalog in &state.connector_catalog {
        let slug = string(catalog, "slug");
        if !rows.iter().any(|v| string(v, "connector") == slug) {
            rows.push(serde_json::json!({"connector":slug,"name":string(catalog,"name"),"description":string(catalog,"description"),"enabled":false,"connected":false}));
        }
    }
    rows
}
