//! Actual device controls; backend font assignment uses the existing profile-fenced Settings writer.
use crate::{
    app::Viewer,
    client_settings::{Action, COMMANDS, binding_label},
};
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextInput};
fn note(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.small).color(k.c.text_muted).wrap()
}
fn heading(value: &str, k: &Kit) -> El {
    text(value)
        .font(k.ty.body_medium)
        .color(k.c.text_base)
        .pt(12.)
}
fn choose(
    label: impl Into<String>,
    key: impl Into<String>,
    selected: bool,
    action: impl Fn(&mut Viewer) + 'static,
    k: &Kit,
) -> El {
    Button::new(label)
        .variant(if selected {
            ButtonVariant::Neutral
        } else {
            ButtonVariant::Ghost
        })
        .on_press(action)
        .build(k)
        .key(key)
}
fn scale(s: &Viewer, k: &Kit) -> El {
    let mut options = row().w_full().flex_wrap().gap(5.);
    for value in [90, 100, 110, 125, 150, 175] {
        options = options.child(choose(
            format!("{value}%"),
            format!("display-scale-{value}"),
            s.client_settings.prefs.chat_scale == value,
            move |s| s.client_settings_action(Action::Scale(value)),
            k,
        ));
    }
    col()
        .w_full()
        .gap(6.)
        .child(heading("Conversation text size", k))
        .child(note(
            "A reading preference for this device, independent of the agent model and window size.",
            k,
        ))
        .child(options)
}
fn density(s: &Viewer, k: &Kit) -> El {
    let mut choices = row().w_full().flex_wrap().gap(5.);
    for (id, label) in [
        ("compact", "Compact"),
        ("comfortable", "Comfortable"),
        ("detailed", "Detailed"),
    ] {
        choices = choices.child(choose(
            label,
            format!("display-density-{id}"),
            s.client_settings.prefs.session_density == id,
            move |s| s.client_settings_action(Action::Density(id.into())),
            k,
        ));
    }
    col()
        .w_full()
        .gap(6.)
        .child(heading("Session list density", k))
        .child(choices)
}
pub fn rows(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let p = &s.client_settings.prefs;
    let mut out = vec![];
    if let Some(error) = &s.client_settings.error {
        out.push(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    match s.settings.section.as_str() {
        "appearance" => {
            out.push(scale(s, k));
            out.push(density(s, k));
            out.push(fonts(s, k, layer));
        }
        "chat" => {
            out.push(heading("On this device", k));
            out.push(note("Reading preferences below change this app's presentation. Agent display settings follow separately and keep their own save scope.",k));
            out.push(scale(s, k));
            let mut transparency = row().w_full().flex_wrap().gap(5.);
            for value in [0, 25, 50, 75, 100] {
                transparency = transparency.child(choose(
                    format!("{value}%"),
                    format!("display-bubble-{value}"),
                    p.bubble_transparency == value,
                    move |s| s.client_settings_action(Action::Bubble(value)),
                    k,
                ));
            }
            out.push(heading("Your message bubble transparency", k));
            out.push(transparency);
            out.push(
                Switch::new(p.reasoning_collapsed)
                    .label("Collapse reasoning by default")
                    .on_toggle(|s: &mut Viewer| {
                        s.client_settings_action(Action::Reasoning(
                            !s.client_settings.prefs.reasoning_collapsed,
                        ))
                    })
                    .build(k)
                    .key("display-collapse-reasoning"),
            );
            out.push(
                Switch::new(p.technical_tools)
                    .label("Open technical tool details by default")
                    .on_toggle(|s: &mut Viewer| {
                        s.client_settings_action(Action::Technical(
                            !s.client_settings.prefs.technical_tools,
                        ))
                    })
                    .build(k)
                    .key("display-technical-tools"),
            );
            out.push(note("Simple mode rests disclosures closed without changing these saved preferences. An explicit disclosure click keeps its choice for that conversation.",k));
            out.push(
                Button::new("Reset device reading preferences")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut Viewer| s.client_settings_action(Action::ResetDisplay))
                    .build(k)
                    .key("display-reset"),
            );
        }
        "workspace" => {
            out.push(heading("Default project directory", k));
            out.push(note(
                s.client_settings
                    .prefs
                    .default_project_dir
                    .as_deref()
                    .unwrap_or("No default directory"),
                k,
            ));
            out.push(
                row()
                    .w_full()
                    .flex_wrap()
                    .gap(6.)
                    .child(
                        Button::new("Choose default directory")
                            .variant(ButtonVariant::Ghost)
                            .disabled(s.is_fixture())
                            .on_press(|s: &mut Viewer| {
                                s.open_directory(crate::directories::Purpose::DefaultProject)
                            })
                            .build(k),
                    )
                    .child(
                        Button::new("Clear default")
                            .variant(ButtonVariant::Ghost)
                            .disabled(s.client_settings.prefs.default_project_dir.is_none())
                            .on_press(|s: &mut Viewer| {
                                s.client_settings_action(Action::DefaultProjectDir(None))
                            })
                            .build(k),
                    ),
            );
            out.push(note(
                "New conversations use this directory unless a project or Home is selected.",
                k,
            ));
            out.push(heading("Current conversation workspace", k));
            out.push(note(
                format!(
                    "Profile {} · {}",
                    s.profile,
                    s.workspace_cwd()
                        .unwrap_or_else(|| "Agent default directory".into())
                ),
                k,
            ));
            out.push(
                row()
                    .w_full()
                    .flex_wrap()
                    .gap(6.)
                    .child(
                        Button::new("Choose workspace directory")
                            .variant(ButtonVariant::Ghost)
                            .disabled(s.is_fixture())
                            .on_press(|s: &mut Viewer| {
                                s.open_directory(crate::directories::Purpose::Workspace)
                            })
                            .build(k)
                            .key("settings-workspace-directory"),
                    )
                    .child(
                        Button::new("Show Files")
                            .variant(ButtonVariant::Ghost)
                            .on_press(|s: &mut Viewer| s.reveal_pane("files"))
                            .build(k),
                    )
                    .child(
                        Button::new("Show Terminal")
                            .variant(ButtonVariant::Ghost)
                            .on_press(|s: &mut Viewer| s.reveal_pane("terminal"))
                            .build(k),
                    ),
            );
            out.push(note("This directory belongs to the current conversation. Stored conversations use the retained agent workspace change; a new draft stages its directory until send.",k));
            out.push(density(s, k));
        }
        _ => {}
    }
    if let Some(notice) = &s.client_settings.notice {
        out.push(note(notice, k));
    }
    out
}
fn fonts(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut body=col().w_full().gap(8.).child(heading("Fonts on the connected agent profile",k)).child(note("Font assignments retain Hermes's profile-owned configuration. This native build renders Inter, IBM Plex Mono and the installed system monospace face; other saved font names remain intact and use the bundled fallback.",k));
    let profiles = std::iter::once("Current conversation".to_owned())
        .chain(s.profiles.iter().map(|p| p.name.clone()))
        .collect::<Vec<_>>();
    body = body.child(
        Select::new("display-font-profile", &profiles)
            .current(Some(
                s.settings
                    .scope_profile
                    .as_deref()
                    .unwrap_or("Current conversation"),
            ))
            .open(s.client_settings.open.as_deref() == Some("font-profile"))
            .disabled(s.settings.blocks_owner_change() || s.settings.demo)
            .on_toggle(|s: &mut Viewer| s.client_settings.open = Some("font-profile".into()))
            .on_close(|s: &mut Viewer| s.client_settings.open = None)
            .on_choose(|s: &mut Viewer, value, tx| {
                s.client_settings.open = None;
                s.settings_action(crate::pages::settings::Action::Scope(
                    if value == "Current conversation" {
                        None
                    } else {
                        Some(value.into())
                    },
                ));
                tx.instant();
            })
            .build(k, layer),
    );
    for (key, label, mono) in [
        ("desktop.font_family", "Chat and interface font", false),
        ("terminal.font_family", "Terminal font", true),
    ] {
        let Some(field) = s.settings.fields.iter().find(|f| f.key == key) else {
            body = body.child(note("Font configuration is loading from the agent.", k));
            continue;
        };
        let current = if field.editor.text.is_empty() {
            "Default".to_owned()
        } else {
            field.editor.text.clone()
        };
        let mut options = if mono {
            vec![
                "Default".into(),
                "IBM Plex Mono".into(),
                "ui-monospace".into(),
            ]
        } else {
            vec![
                "Default".into(),
                "Inter".into(),
                "IBM Plex Mono".into(),
                "ui-monospace".into(),
            ]
        };
        if !options.contains(&current) {
            options.push(current.clone());
        }
        let owner = s.composer_scope();
        let scope = s.settings.scope_profile.clone();
        let item = key.to_owned();
        let toggle = key.to_owned();
        let open = s.client_settings.open.as_deref() == Some(key);
        body = body.child(note(label, k)).child(
            Select::new(
                format!("display-font-{}", if mono { "terminal" } else { "chat" }),
                &options,
            )
            .current(Some(&current))
            .open(open)
            .disabled(
                !s.settings.config_ready()
                    || s.settings.loading
                    || s.settings.saving
                    || s.settings.demo,
            )
            .on_toggle(move |s: &mut Viewer| s.client_settings.open = Some(toggle.clone()))
            .on_close(|s: &mut Viewer| s.client_settings.open = None)
            .on_choose(move |s: &mut Viewer, value, tx| {
                if s.composer_scope().connection != owner.connection
                    || s.composer_scope().profile != owner.profile
                    || s.settings.scope_profile != scope
                {
                    return;
                }
                s.client_settings.open = None;
                s.settings_action(crate::pages::settings::Action::Value(
                    item.clone(),
                    serde_json::json!(if value == "Default" { "" } else { value }),
                ));
                tx.wake_after(std::time::Duration::from_millis(560));
            })
            .build(k, layer),
        );
    }
    body
}
pub fn shortcuts(s: &Viewer, k: &Kit) -> El {
    let state = &s.client_settings;
    let query = state.search.text.trim().to_lowercase();
    let mut page=col().key("client-keybindings").w_full().h_full().min_h(0.).gap(12.).padding(24.).scroll(ScrollSpec{stick_bottom:false}).child(heading("Keyboard shortcuts",k)).child(note("Click a binding, then press its replacement. Escape cancels; Backspace or Delete unbinds it. Settings are saved on this device. Conflicting chords run the first listed action.",k));
    page = page.child(
        TextInput::new(&state.search)
            .disabled(state.capture.is_some())
            .placeholder("Search actions or shortcuts…")
            .width(None)
            .on_edit(|s: &mut Viewer| &mut s.client_settings.search)
            .build(k)
            .key("keybindings-search"),
    );
    if let Some(error) = &state.error {
        page = page.child(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(notice) = &state.notice {
        page = page.child(note(notice, k));
    }
    if let Some(id) = &state.capture {
        page = page
            .child(note(
                format!(
                    "Listening for {}…",
                    COMMANDS
                        .iter()
                        .find(|c| c.id == id)
                        .map(|c| c.label)
                        .unwrap_or(id)
                ),
                k,
            ))
            .child(
                Button::new("Cancel capture")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut Viewer| s.client_settings.capture = None)
                    .build(k)
                    .key("keybindings-cancel"),
            );
    }
    let mut category = "";
    for command in COMMANDS {
        let combos = state.bindings(command.id);
        let labels = combos
            .iter()
            .map(|c| binding_label(c))
            .collect::<Vec<_>>()
            .join(", ");
        if !query.is_empty()
            && !format!("{} {} {}", command.label, command.id, labels)
                .to_lowercase()
                .contains(&query)
        {
            continue;
        }
        if category != command.category {
            category = command.category;
            page = page.child(heading(category, k));
        }
        let id = command.id;
        let reset = id;
        let clear = id;
        let mut line = row().w_full().flex_wrap().gap(8.).center_y().child(
            text(command.label)
                .font(k.ty.ui)
                .color(k.c.text_base)
                .min_w(170.)
                .grow(1.),
        );
        line = line.child(
            Button::new(if state.capture.as_deref() == Some(id) {
                "Press a shortcut…".into()
            } else if labels.is_empty() {
                "Unbound".into()
            } else {
                labels
            })
            .variant(ButtonVariant::Ghost)
            .on_press_tx(move |s: &mut Viewer, tx| {
                s.client_settings_action(Action::Capture(id.into()));
                tx.blur();
                tx.instant();
            })
            .build(k)
            .key(format!("keybind-capture-{id}")),
        );
        line = line
            .child(
                Button::new("Clear")
                    .disabled(state.capture.is_some())
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(move |s: &mut Viewer| {
                        s.client_settings_action(Action::Clear(clear.into()))
                    })
                    .build(k)
                    .key(format!("keybind-clear-{id}")),
            )
            .child(
                Button::new("Reset")
                    .disabled(state.capture.is_some())
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(move |s: &mut Viewer| {
                        s.client_settings_action(Action::Reset(reset.into()))
                    })
                    .build(k)
                    .key(format!("keybind-reset-{id}")),
            );
        page = page.child(line);
        let conflicts = state.conflicts(id);
        if !conflicts.is_empty() {
            page = page.child(note(
                format!("Also assigned to: {}", conflicts.join(", ")),
                k,
            ));
        }
    }
    page.child(Button::new("Reset all shortcuts").disabled(state.capture.is_some()).variant(ButtonVariant::Ghost).on_press(|s:&mut Viewer|s.client_settings_action(Action::ResetBindings)).build(k).key("keybindings-reset-all")).child(heading("Context-owned keys",k)).child(note("Enter sends, Shift+Enter starts a new line, Escape dismisses the active interaction. Terminal and model-picker keys remain owned by the focused surface. Global OS shortcuts and plugin actions are not available here yet.",k))
}
