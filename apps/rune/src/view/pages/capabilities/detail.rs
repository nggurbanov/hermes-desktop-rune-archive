use super::{button, confirm, heading, notice};
use crate::{
    app::Viewer,
    pages::capabilities::{Action, Selection, flag, rows, string, strings, tool_enabled},
};
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Kit, Select, Switch, TextArea, TextInput};
use serde_json::Value;

pub(super) fn panel(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let state = &s.capabilities;
    let mut pane = col()
        .w_full()
        .min_w(0.0)
        .gap(12.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(row().w_full().child(el().grow(1.0)).child(button(
            "Close",
            Action::CloseDetail,
            state.busy(),
            k,
        )));
    let Some(selection) = &state.selection else {
        return pane;
    };
    match selection {
        Selection::Skill(name) => {
            let skill = state.skills.iter().find(|v| string(v, "name") == *name);
            pane = pane.child(heading(name, k));
            if let Some(skill) = skill {
                pane = pane.child(notice(string(skill, "description"), k));
                let editable = matches!(string(skill, "provenance").as_str(), "agent" | "external");
                let mut actions = row().gap(8.0);
                if editable {
                    actions = actions
                        .child(button(
                            "Edit skill",
                            Action::EditSkill(name.clone()),
                            state.demo || state.busy(),
                            k,
                        ))
                        .child(button(
                            "Archive",
                            confirm(
                                format!("Archive {name}?"),
                                "The skill will be removed from the agent's active learning index.",
                                Action::ArchiveSkill(name.clone()),
                            ),
                            state.demo || state.busy(),
                            k,
                        ));
                }
                if string(skill, "provenance") == "hub" {
                    actions = actions.child(button(
                        "Uninstall",
                        confirm(
                            format!("Uninstall {name}?"),
                            "This removes the installed Hub skill from the selected profile.",
                            Action::UninstallSkill(name.clone()),
                        ),
                        state.demo || state.busy(),
                        k,
                    ));
                }
                pane = pane.child(actions);
            }
            if state.editing {
                pane = pane
                    .child(
                        TextArea::new(&state.content)
                            .width(None)
                            .rows(20)
                            .on_edit(|s: &mut Viewer| &mut s.capabilities.content)
                            .build(k)
                            .key("skill-content-editor"),
                    )
                    .child(button(
                        "Save skill",
                        Action::SaveSkill,
                        state.demo || state.busy(),
                        k,
                    ));
            } else {
                pane = pane.child(
                    text(&state.content.text)
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .wrap()
                        .selectable(true),
                );
            }
        }
        Selection::Hub(identifier) => {
            pane = pane
                .child(heading(
                    nonempty(string(&state.detail, "name"), identifier),
                    k,
                ))
                .child(notice(string(&state.detail, "description"), k));
            let policy = string(&state.scan, "policy");
            if state.scan.is_null() {
                pane = pane.child(notice("Reading the skill and its security scan…", k));
            } else {
                pane = pane
                    .child(heading(
                        format!("Scan: {}", string(&state.scan, "verdict")),
                        k,
                    ))
                    .child(notice(string(&state.scan, "summary"), k))
                    .child(notice(string(&state.scan, "policy_reason"), k));
                for finding in rows(&state.scan, "findings") {
                    pane = pane.child(notice(
                        format!(
                            "{} · {}:{} · {}",
                            string(&finding, "severity"),
                            string(&finding, "file"),
                            finding["line"],
                            string(&finding, "description")
                        ),
                        k,
                    ));
                }
                let description = format!(
                    "Install {} on this profile?\n{}\n{}",
                    identifier,
                    string(&state.scan, "summary"),
                    string(&state.scan, "policy_reason")
                );
                pane = pane.child(button(
                    if policy == "block" {
                        "Installation blocked"
                    } else {
                        "Install skill"
                    },
                    confirm("Install skill", description, Action::InstallSkill),
                    state.demo || state.busy() || policy == "block",
                    k,
                ));
            }
            pane = pane.child(
                text(&state.content.text)
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .wrap()
                    .selectable(true),
            );
        }
        Selection::Toolset(name) => {
            pane = pane.child(heading(name, k));
            if let Some(tool) = state.toolsets.iter().find(|v| string(v, "name") == *name) {
                pane = pane.child(notice(string(tool, "description"), k));
                for tool in strings(&tool["tools"]) {
                    pane = pane.child(notice(tool, k));
                }
            }
            if name == "vision" {
                pane = pane
                    .child(notice(
                        "The vision model is configured in Settings → Models.",
                        k,
                    ))
                    .child(button(
                        "Configure vision model",
                        Action::OpenSettings("model".into()),
                        state.busy(),
                        k,
                    ));
            }
            if name == "browser" {
                pane=pane.child(heading("Browser profile",k)).child(notice("Use a copy of the agent host's default browser profile, including its cookies and sign-ins. Turning this off removes the snapshot on next use.",k))
                    .child(Switch::new(flag(&state.tool_settings["browser"],"use_real_profile")).label("Use real browser profile").disabled(state.demo||state.busy()||state.tool_settings.is_null()).on_toggle(|s:&mut Viewer| {
                        let enabled=!flag(&s.capabilities.tool_settings["browser"],"use_real_profile");
                        s.capabilities_action(confirm("Browser profile access", "Change whether the agent may copy browser cookies and sign-ins on its host. Applies to new sessions.", Action::BrowserRealProfile(enabled)));
                    }).build(k));
            }
            if name == "computer_use" {
                pane = computer(s, k, pane);
            }
            if name == "tts" || name == "stt" {
                pane = pane.child(button(
                    "Voice and model settings",
                    Action::OpenSettings("voice".into()),
                    state.busy(),
                    k,
                ));
            }
            if name == "terminal" {
                pane = pane.child(heading("Execution backend", k));
                for backend in rows(&state.terminal, "backends") {
                    let label = string(&backend, "label");
                    let name = string(&backend, "name");
                    pane = pane.child(
                        row()
                            .w_full()
                            .gap(8.0)
                            .center_y()
                            .child(
                                col()
                                    .grow(1.0)
                                    .min_w(0.0)
                                    .gap(3.0)
                                    .child(heading(&label, k))
                                    .child(notice(
                                        format!(
                                            "{} · {}",
                                            string(&backend, "status"),
                                            string(&backend, "detail")
                                        ),
                                        k,
                                    )),
                            )
                            .child(button(
                                if flag(&backend, "active") {
                                    "Selected"
                                } else {
                                    "Use"
                                },
                                Action::TerminalBackend(name),
                                state.demo || state.busy() || flag(&backend, "active"),
                                k,
                            )),
                    );
                }
            }
            pane = pane.child(heading("Providers", k));
            for provider in rows(&state.detail, "providers") {
                let provider_name = string(&provider, "name");
                let active = flag(&provider, "is_active");
                let mut actions = row().gap(6.0).children((name != "web").then(|| {
                    button(
                        if active { "Selected" } else { "Use provider" },
                        Action::Provider(provider_name.clone(), None),
                        state.demo || state.busy() || active,
                        k,
                    )
                }));
                if name == "web" {
                    let caps = strings(&provider["capabilities"]);
                    for capability in ["search", "extract"] {
                        if caps.is_empty() || caps.iter().any(|c| c == capability) {
                            actions = actions.child(button(
                                format!("Use for {capability}"),
                                Action::Provider(provider_name.clone(), Some(capability.into())),
                                state.demo || state.busy(),
                                k,
                            ));
                        }
                    }
                }
                if matches!(name.as_str(), "image_gen" | "video_gen") {
                    actions = actions.child(button(
                        "Models",
                        Action::Models(provider_name.clone()),
                        state.busy(),
                        k,
                    ));
                }
                if flag(&provider, "requires_nous_auth")
                    && string(&provider, "status") == "needs_auth"
                {
                    actions = actions.child(button(
                        "Sign in to Nous",
                        Action::OpenSettings("providers".into()),
                        state.busy(),
                        k,
                    ));
                }
                if let Some(key) = provider
                    .get("post_setup")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    actions = actions.child(button(
                        "Run setup",
                        confirm(
                            "Run provider setup?",
                            format!("The selected agent will run setup for {}.", provider_name),
                            Action::PostSetup(key.into()),
                        ),
                        state.demo || state.busy(),
                        k,
                    ));
                }
                pane = pane.child(
                    col()
                        .w_full()
                        .gap(6.0)
                        .py(8.0)
                        .child(heading(&provider_name, k))
                        .child(notice(
                            format!(
                                "{} {}",
                                string(&provider, "status"),
                                string(&provider, "tag")
                            ),
                            k,
                        ))
                        .child(actions),
                );
            }
            if !state.fields.is_empty() {
                pane = pane
                    .child(heading("Credentials", k))
                    .child(button(
                        "Manage stored keys",
                        Action::OpenSettings("keys".into()),
                        state.busy(),
                        k,
                    ))
                    .children(field_rows(s, k, layer))
                    .child(button(
                        "Save credentials",
                        Action::SaveToolSecrets,
                        state.demo
                            || state.busy()
                            || state.fields.iter().all(|f| f.input.text.is_empty()),
                        k,
                    ));
            }
            for model in rows(&state.tool_models, "models") {
                let id = nonempty(string(&model, "id"), model.as_str().unwrap_or_default());
                pane = pane.child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .child(heading(nonempty(string(&model, "display"), &id), k))
                                .child(notice(string(&model, "strengths"), k)),
                        )
                        .child(button(
                            if state.tool_models["current"]
                                .as_str()
                                .or(state.tool_models["default"].as_str())
                                == Some(&id)
                            {
                                "Selected"
                            } else {
                                "Use model"
                            },
                            Action::SelectModel(id.clone()),
                            state.demo
                                || state.busy()
                                || state.tool_models["current"]
                                    .as_str()
                                    .or(state.tool_models["default"].as_str())
                                    == Some(&id),
                            k,
                        )),
                );
            }
        }
        Selection::Plugin(key) => {
            let name = string(&state.detail, "name");
            pane = pane
                .child(heading(&name, k))
                .child(notice(string(&state.detail, "description"), k))
                .child(notice(
                    format!(
                        "{} · {}",
                        string(&state.detail, "version"),
                        string(&state.detail, "source")
                    ),
                    k,
                ));
            if key.is_empty() {
                pane=pane.child(notice("This backend has no canonical plugin key. Update the agent before editing plugin state.",k));
            }
            if flag(&state.detail, "has_desktop_half") {
                pane=pane.child(notice("This package also contains a Desktop UI extension. Native GUI-plugin hosting has not been transferred yet.",k));
            }
            for server in rows(&state.detail, "servers") {
                pane = pane.child(notice(
                    format!(
                        "{} · {}",
                        string(&server, "name"),
                        string(&server, "sentence")
                    ),
                    k,
                ));
            }
            pane = pane.children(field_rows(s, k, layer));
            if !state.fields.is_empty() {
                pane = pane.child(button(
                    "Save plugin settings",
                    Action::SavePluginSettings,
                    state.demo || state.busy() || key.is_empty(),
                    k,
                ));
            }
            let source = string(&state.detail, "source");
            pane=pane.child(row().gap(8.0).child(button("Update",Action::UpdatePlugin(name.clone(),false,false),state.demo||state.busy()||string(&state.detail,"pinned_sha").len()==40,k))
                .child(button("Remove",confirm(format!("Remove {name}?"),"Remove this user-installed agent plugin and its install metadata from the selected profile.",Action::RemovePlugin(name)),state.demo||state.busy()||source=="bundled",k)));
        }
        Selection::InstallPlugin => {
            pane = pane
                .child(heading("Install an agent plugin", k))
                .child(notice(
                    "Inspect the source first. Installation is bound to the exact reviewed commit.",
                    k,
                ))
                .child(
                    TextInput::new(&state.source)
                        .width(None)
                        .placeholder("Repository URL or plugin identifier")
                        .on_edit(|s: &mut Viewer| &mut s.capabilities.source)
                        .build(k)
                        .key("plugin-install-source"),
                )
                .child(button(
                    "Inspect source",
                    Action::InspectPlugin,
                    state.demo
                        || state.busy()
                        || state.pending.contains("capabilities.plugin-inspect"),
                    k,
                ));
            if !state.inspection.is_null() {
                pane = pane.child(notice(
                    format!("Commit: {}", string(&state.inspection, "sha")),
                    k,
                ));
                for warning in strings(&state.inspection["warnings"]) {
                    pane = pane.child(notice(warning, k));
                }
                if flag(&state.inspection, "desktop") {
                    pane=pane.child(notice("Desktop UI half detected. This action installs the agent half; native GUI-plugin hosting is still required for its UI.",k));
                }
                if !flag(&state.inspection, "agent") {
                    pane = pane.child(notice("No agent plugin was found in this source.", k));
                }
                pane = pane.child(button(
                    "Install reviewed commit",
                    confirm(
                        "Install this plugin?",
                        format!(
                            "Source: {}\nCommit: {}\n{}",
                            state.source.text,
                            string(&state.inspection, "sha"),
                            strings(&state.inspection["warnings"]).join("\n")
                        ),
                        Action::InstallPlugin,
                    ),
                    state.demo
                        || state.busy()
                        || !flag(&state.inspection, "ok")
                        || !flag(&state.inspection, "agent"),
                    k,
                ));
            }
        }
        Selection::Mcp(name) => {
            pane = pane.child(heading(name, k));
            let plugin_owned = state
                .mcp
                .iter()
                .any(|v| string(v, "name") == *name && string(v, "source") == "plugin");
            if plugin_owned {
                pane = pane.child(notice(
                    "This server is owned by its plugin. Configure it from the Plugins tab.",
                    k,
                ));
            } else {
                pane = pane.children(mcp_form(s, k)).child(button(
                    "Save server",
                    Action::SaveMcp,
                    state.demo
                        || state.busy()
                        || !state.mcp_config_loaded
                        || state.pending.contains("capabilities.mcp-config"),
                    k,
                ));
            }
            pane = pane.child(
                row()
                    .gap(8.0)
                    .child(button(
                        "Test connection",
                        Action::ProbeMcp(name.clone()),
                        state.demo
                            || state.busy()
                            || state.pending.contains("capabilities.mcp-probe"),
                        k,
                    ))
                    .child(button(
                        "Remove server",
                        confirm(
                            format!("Remove {name}?"),
                            "This removes the server configuration from the selected agent.",
                            Action::RemoveMcp(name.clone()),
                        ),
                        state.demo || state.busy() || plugin_owned,
                        k,
                    )),
            );
            if !state.mcp_probe.is_null() {
                pane = pane
                    .child(heading(
                        if flag(&state.mcp_probe, "ok") {
                            "Connection succeeded"
                        } else {
                            "Connection failed"
                        },
                        k,
                    ))
                    .child(notice(string(&state.mcp_probe, "error"), k));
                if flag(&state.mcp_probe, "oauth_needed") {
                    pane=pane.child(notice("This server requires OAuth. Sign in below to authorize it in your browser.",k));
                }
                for tool in rows(&state.mcp_probe, "tools") {
                    let name = string(&tool, "name");
                    let enabled = state
                        .mcp_config
                        .get(name_of(selection))
                        .is_some_and(|v| tool_enabled(v, &name));
                    pane = pane.child(
                        row()
                            .gap(8.0)
                            .w_full()
                            .center_y()
                            .child(
                                col()
                                    .grow(1.0)
                                    .min_w(0.0)
                                    .child(heading(&name, k))
                                    .child(notice(string(&tool, "description"), k)),
                            )
                            .child(
                                Switch::new(enabled)
                                    .label(format!("Enable {name}"))
                                    .hide_label()
                                    .disabled(state.demo || state.busy() || plugin_owned)
                                    .on_toggle(move |s: &mut Viewer| {
                                        s.capabilities_action(Action::ToggleMcpTool(name.clone()))
                                    })
                                    .build(k),
                            ),
                    );
                }
            }
            if state
                .mcp_config
                .get(name)
                .is_some_and(|config| !string(config, "url").is_empty())
                || flag(&state.mcp_probe, "oauth_needed")
            {
                pane = pane.child(
                    button(
                        "Sign in with OAuth",
                        Action::StartMcpOAuth(name.clone()),
                        state.demo || state.busy(),
                        k,
                    )
                    .key("mcp-oauth-begin"),
                );
                pane = mcp_oauth(s, k, pane, name);
            }
            pane = pane.child(super::logs::panel(s, k));
        }
        Selection::AddMcp => {
            pane=pane.child(heading("Add MCP server",k)).child(notice("Commands execute on the agent host. URLs are reached from that host.",k)).children(mcp_form(s,k))
            .child(button("Add server",confirm("Add MCP server?","The selected agent will save this server configuration. Test it explicitly after saving.",Action::SaveMcp),state.demo||state.busy()||!state.mcp_config_loaded||state.pending.contains("capabilities.mcp-config"),k));
        }
        Selection::AdvancedMcp => {
            pane=pane.child(heading("mcp.json",k)).child(notice("This replaces the complete server map. Deleted entries and removed fields are persisted.",k))
            .child(TextArea::new(&state.mcp_document).width(None).rows(23).on_edit(|s:&mut Viewer|&mut s.capabilities.mcp_document).build(k).key("mcp-json-editor"))
            .child(button("Save complete server map",confirm("Replace MCP server configuration?","Review the complete map before saving. Entries absent from this document will be removed.",Action::SaveMcpDocument),state.demo||state.busy()||!state.mcp_config_loaded||state.pending.contains("capabilities.mcp-config"),k));
        }
        Selection::McpCatalog(name) => {
            pane=pane.child(heading(name,k)).child(notice(string(&state.detail,"description"),k)).child(notice(string(&state.detail,"post_install"),k)).children(field_rows(s,k,layer))
                .child(button("Install and enable",confirm(format!("Install {name}?"),"Install this catalog entry and its dependencies on the selected agent host.",Action::InstallMcpCatalog(name.clone())),state.demo||state.busy(),k));
        }
        Selection::Connector(slug) => {
            pane = connector(s, k, pane, slug);
        }
    }
    pane
}

fn mcp_oauth(s: &Viewer, k: &Kit, mut pane: El, name: &str) -> El {
    let Some(flow) = s
        .capabilities
        .mcp_oauth
        .as_ref()
        .filter(|flow| flow.name == name)
    else {
        return pane;
    };
    pane = pane.child(heading(format!("Authorization: {}", flow.status), k));
    if let Some(error) = &flow.error {
        pane = pane.child(notice(error, k).role("alert"));
    }
    if flow.status == "pending" && !flow.authorization_url.is_empty() {
        let url = flow.authorization_url.clone();
        pane=pane.child(notice("The callback listener runs on this computer. The agent verifies the code and stores the tokens on its own host.",k))
            .child(rune_kit::Button::new("Open authorization in browser").variant(rune_kit::ButtonVariant::Ghost).disabled(s.capabilities.demo).on_press_tx(move|s:&mut Viewer,tx|{if tx.open_url(&url).is_err(){s.capabilities.message=Some("The browser could not be opened. Try again.".into());}}).build(k).key("mcp-oauth-open"));
    }
    if flow.running() {
        pane = pane.child(
            button(
                "Cancel authorization",
                Action::CancelMcpOAuth,
                s.capabilities.demo || flow.status == "cancelling",
                k,
            )
            .key("mcp-oauth-cancel"),
        );
    }
    pane
}
fn computer(s: &Viewer, k: &Kit, mut pane: El) -> El {
    let state = &s.capabilities;
    let status = &state.computer_status;
    pane = pane.child(heading("Computer Use", k)).child(button(
        "Recheck",
        Action::ComputerStatus,
        state.demo || state.busy() || state.pending.contains("capabilities.computer-status"),
        k,
    ));
    if status.is_null() {
        return pane.child(notice("Checking the driver on the agent host…", k));
    }
    if !flag(status, "platform_supported") {
        return pane.child(notice(
            format!(
                "Computer Use is unavailable on {}.",
                string(status, "platform")
            ),
            k,
        ));
    }
    if !flag(status, "installed") {
        return pane.child(notice(
            "Install the cua-driver provider below, then grant its required permissions.",
            k,
        ));
    }
    pane = pane.child(notice(
        format!(
            "{} · {}",
            string(status, "version"),
            if flag(status, "ready") {
                "Ready"
            } else {
                "Not ready"
            }
        ),
        k,
    ));
    if flag(status, "can_grant") {
        pane=pane.child(notice("Permissions attach to CuaDriver on the agent host. Approve its Accessibility and Screen Recording prompts there.",k));
        for (key, label) in [
            ("accessibility", "Accessibility"),
            ("screen_recording", "Screen Recording"),
        ] {
            let value = match status[key].as_bool() {
                Some(true) => "Granted",
                Some(false) => "Not granted",
                None => "Unknown",
            };
            pane = pane.child(notice(format!("{label}: {value}"), k));
        }
        if !flag(status, "ready") {
            pane=pane.child(button("Grant permissions",confirm("Request Computer Use permissions?","The selected agent will launch its driver's permission flow on its host. Approve the prompts there.",Action::GrantComputerPermissions),state.demo||state.busy(),k));
        }
    }
    for check in rows(status, "checks")
        .iter()
        .filter(|c| string(c, "status") != "ok")
    {
        pane = pane.child(notice(
            format!("{}: {}", string(check, "label"), string(check, "message")),
            k,
        ));
    }
    if !string(status, "error").is_empty() {
        pane = pane.child(notice(string(status, "error"), k));
    }
    pane
}
fn field_rows(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    s.capabilities
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let mut item = col()
                .w_full()
                .min_w(0.0)
                .gap(5.0)
                .py(7.0)
                .child(heading(&field.label, k))
                .child(notice(&field.description, k));
            let disabled = s.capabilities.demo || s.capabilities.busy();
            if field.kind == "boolean" {
                item = item.child(
                    Switch::new(field.checked)
                        .label(&field.label)
                        .hide_label()
                        .disabled(disabled)
                        .on_toggle(move |s: &mut Viewer| {
                            s.capabilities.fields[index].checked =
                                !s.capabilities.fields[index].checked;
                        })
                        .build(k),
                );
            } else if field.kind == "enum" && !field.choices.is_empty() {
                let id = format!("capability-field-{index}");
                let toggle = id.clone();
                let close = id.clone();
                item = item.child(
                    Select::new(&id, &field.choices)
                        .current(Some(&field.input.text))
                        .width(250.0)
                        .open(s.capabilities.open_field.as_ref() == Some(&id))
                        .disabled(disabled)
                        .on_toggle(move |s: &mut Viewer| {
                            s.capabilities.open_field =
                                if s.capabilities.open_field.as_ref() == Some(&toggle) {
                                    None
                                } else {
                                    Some(toggle.clone())
                                };
                        })
                        .on_close(move |s: &mut Viewer| {
                            if s.capabilities.open_field.as_ref() == Some(&close) {
                                s.capabilities.open_field = None;
                            }
                        })
                        .on_choose(move |s: &mut Viewer, value, tx| {
                            s.capabilities.fields[index].input.replace_text(value);
                            s.capabilities.open_field = None;
                            tx.instant();
                        })
                        .build(k, layer),
                );
            } else if field.kind == "json" {
                item = item.child(
                    TextArea::new(&field.input)
                        .width(None)
                        .rows(5)
                        .disabled(disabled)
                        .on_edit(move |s: &mut Viewer| &mut s.capabilities.fields[index].input)
                        .build(k),
                );
            } else {
                let mut input = TextInput::new(&field.input)
                    .width(None)
                    .disabled(disabled)
                    .placeholder(if field.kind == "secret" && field.has_value {
                        "Configured · leave blank to keep"
                    } else {
                        "Enter a value"
                    })
                    .on_edit(move |s: &mut Viewer| &mut s.capabilities.fields[index].input);
                if field.kind == "secret" {
                    input = input.secure();
                }
                item = item.child(input.build(k).key(format!("capability-field-{index}")));
            }
            item
        })
        .collect()
}
fn mcp_form(s: &Viewer, k: &Kit) -> Vec<El> {
    let c = &s.capabilities;
    let disabled =
        c.demo || c.busy() || !c.mcp_config_loaded || c.pending.contains("capabilities.mcp-config");
    vec![
        heading("Name", k),
        TextInput::new(&c.mcp_name)
            .width(None)
            .disabled(disabled || matches!(&c.selection, Some(Selection::Mcp(_))))
            .on_edit(|s: &mut Viewer| &mut s.capabilities.mcp_name)
            .build(k),
        heading("Server URL", k),
        TextInput::new(&c.mcp_url)
            .width(None)
            .disabled(disabled)
            .placeholder("https://server.example/mcp")
            .on_edit(|s: &mut Viewer| &mut s.capabilities.mcp_url)
            .build(k),
        notice("Or run a command on the agent host", k),
        TextInput::new(&c.mcp_command)
            .width(None)
            .disabled(disabled)
            .placeholder("Command")
            .on_edit(|s: &mut Viewer| &mut s.capabilities.mcp_command)
            .build(k),
        heading("Arguments (JSON array)", k),
        TextArea::new(&c.mcp_args)
            .width(None)
            .rows(2)
            .disabled(disabled)
            .on_edit(|s: &mut Viewer| &mut s.capabilities.mcp_args)
            .build(k),
        heading("Environment (JSON object)", k),
        TextArea::new(&c.mcp_env)
            .width(None)
            .rows(3)
            .disabled(disabled)
            .on_edit(|s: &mut Viewer| &mut s.capabilities.mcp_env)
            .build(k),
        heading("Headers (JSON object)", k),
        TextArea::new(&c.mcp_headers)
            .width(None)
            .rows(3)
            .disabled(disabled)
            .on_edit(|s: &mut Viewer| &mut s.capabilities.mcp_headers)
            .build(k),
    ]
}
fn connector(s: &Viewer, k: &Kit, mut pane: El, slug: &str) -> El {
    let state = &s.capabilities;
    let connector_row = state
        .connectors
        .iter()
        .find(|v| string(v, "connector") == slug);
    let catalog = state
        .connector_catalog
        .iter()
        .find(|v| string(v, "slug") == slug);
    pane = pane.child(heading(
        catalog
            .map(|v| string(v, "name"))
            .unwrap_or_else(|| slug.into()),
        k,
    ));
    if let Some(catalog) = catalog {
        pane = pane.child(notice(string(catalog, "description"), k));
    }
    let enabled = connector_row.is_some_and(|v| flag(v, "enabled"));
    let owned_slug = slug.to_owned();
    let locked = crate::pages::capabilities::connector_locked(&state.policy, slug);
    pane = pane.child(
        Switch::new(enabled)
            .label("Enable connector")
            .disabled(
                state.demo
                    || state.busy()
                    || locked
                    || crate::pages::capabilities::member_revision(&state.policy).is_empty(),
            )
            .on_toggle(move |s: &mut Viewer| {
                s.capabilities_action(Action::ToggleConnector(owned_slug.clone(), !enabled))
            })
            .build(k),
    );
    if locked {
        pane = pane.child(notice("Disabled by an organization or role policy.", k));
    }
    pane = pane
        .child(
            TextInput::new(&state.alias)
                .width(None)
                .placeholder("Account name (optional for a new account)")
                .on_edit(|s: &mut Viewer| &mut s.capabilities.alias)
                .build(k),
        )
        .child(button(
            "Connect account",
            Action::Connect(slug.into(), false),
            state.demo || state.busy(),
            k,
        ));
    pane = accounts(s, k, pane, slug);
    pane = operation(s, k, pane);
    connector_tools(s, k, pane, slug)
}
fn accounts(s: &Viewer, k: &Kit, mut pane: El, slug: &str) -> El {
    let state = &s.capabilities;
    for account in state
        .accounts
        .iter()
        .filter(|v| string(v, "connector") == slug)
    {
        let id = string(account, "connection_id");
        let label = string(account, "label");
        let retired = flag(account, "disabled");
        pane=pane.child(col().gap(6.0).py(8.0).child(heading(&label,k))
            .child(notice(format!("{} · {}{}",string(account,"status"),string(account,"status_reason"),if retired{" · Replaced account"}else{""}),k))
            .child(row().gap(5.0)
                .child(button("Reconnect",Action::ReconnectAccount(slug.into(),id.clone()),state.demo||state.busy()||retired,k))
                .child(button("Rename",Action::RenameAccount(id.clone()),state.demo||state.busy(),k))
                .child(button("Disconnect",confirm(format!("Disconnect {label}?"),"The account will be removed from this agent's connector configuration.",Action::RemoveAccount(id)),state.demo||state.busy(),k))));
    }
    pane
}
fn operation(s: &Viewer, k: &Kit, mut pane: El) -> El {
    let state = &s.capabilities;
    if state.connection_operation.is_null() {
        return pane;
    }
    let settled = flag(&state.connection_operation, "settled");
    pane = pane.child(heading(
        if settled {
            "Connection request finished"
        } else {
            "Connection request"
        },
        k,
    ));
    for target in rows(&state.connection_operation, "targets") {
        let name = string(&target, "name");
        let resolved = flag(&target, "resolved");
        pane = pane.child(heading(
            format!("{} · {}", name, string(&target, "state")),
            k,
        ));
        for key in ["detail", "instructions", "description", "discovery_error"] {
            let value = string(&target, key);
            if !value.is_empty() {
                pane = pane.child(notice(value, k));
            }
        }
        let link = string(&target, "connect_url");
        if let Ok(url) = url::Url::parse(&link)
            && matches!(url.scheme(), "https" | "http")
        {
            pane = pane.child(
                rune_kit::Button::new("Open authorization link")
                    .variant(rune_kit::ButtonVariant::Ghost)
                    .disabled(state.demo || settled || resolved)
                    .on_press_tx(move |s: &mut Viewer, tx| {
                        if tx.open_url(&link).is_err() {
                            s.capabilities.message =
                                Some("The browser could not be opened. Try again.".into());
                        }
                    })
                    .build(k),
            );
        }
        for (index, field) in state
            .connection_fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.target == name)
        {
            pane = pane.child(heading(
                if field.prompt.is_empty() {
                    &field.name
                } else {
                    &field.prompt
                },
                k,
            ));
            let mut input = TextInput::new(&field.input)
                .width(None)
                .disabled(state.demo || settled || resolved || state.busy())
                .on_edit(move |s: &mut Viewer| &mut s.capabilities.connection_fields[index].input);
            if field.secret {
                input = input.secure();
            }
            pane = pane.child(input.build(k));
        }
        if !settled
            && !resolved
            && matches!(string(&target, "action").as_str(), "install" | "enable")
        {
            pane = pane.child(
                row()
                    .gap(8.0)
                    .child(button(
                        "Approve",
                        confirm(
                            format!("Approve {name}?"),
                            string(&target, "description"),
                            Action::AnswerConnection(name.clone(), true),
                        ),
                        state.demo || state.busy(),
                        k,
                    ))
                    .child(button(
                        "Skip",
                        Action::AnswerConnection(name, false),
                        state.demo || state.busy(),
                        k,
                    )),
            );
        }
    }
    pane = pane.child(button(
        "Refresh connection status",
        Action::RefreshConnection,
        state.demo,
        k,
    ));
    if !settled {
        pane = pane.child(button(
            "Stop waiting",
            Action::StopConnection,
            state.demo || state.busy(),
            k,
        ));
    }
    pane
}
fn connector_tools(s: &Viewer, k: &Kit, mut pane: El, slug: &str) -> El {
    let state = &s.capabilities;
    pane = pane.child(heading("Tools", k));
    let disabled = crate::pages::capabilities::connector_disabled(&state.policy, slug);
    for tool in rows(&state.detail, "tools") {
        let id = string(&tool, "slug");
        let locked = crate::pages::capabilities::connector_tool_locked(
            &state.policy,
            slug,
            &id,
            &state.detail,
        );
        let on = !locked && !disabled.iter().any(|n| n == &id);
        pane = pane.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(heading(string(&tool, "name"), k))
                        .child(notice(string(&tool, "description"), k))
                        .child(notice(
                            if locked {
                                "Managed by organization".into()
                            } else {
                                string(&tool, "facet")
                            },
                            k,
                        )),
                )
                .child(
                    Switch::new(on)
                        .label(format!("Enable {id}"))
                        .hide_label()
                        .disabled(
                            state.demo
                                || state.busy()
                                || locked
                                || crate::pages::capabilities::member_revision(&state.policy)
                                    .is_empty(),
                        )
                        .on_toggle(move |s: &mut Viewer| {
                            s.capabilities_action(Action::ToggleConnectorTool(id.clone()))
                        })
                        .build(k),
                ),
        );
    }
    pane
}

fn name_of(s: &Selection) -> &str {
    if let Selection::Mcp(name) = s {
        name
    } else {
        ""
    }
}
fn nonempty(value: String, fallback: &str) -> String {
    if value.is_empty() {
        fallback.into()
    } else {
        value
    }
}
