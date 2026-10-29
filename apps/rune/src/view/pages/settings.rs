//! Hermes Settings composed from native OpenCode fields and server schema.
mod billing;
mod delegation;
mod files;
mod local_models;
mod memory;
mod moa;
mod models;
mod plugins;
mod providers;
mod vault;
mod web_access;
use crate::{
    app::Viewer,
    pages::settings::{Action, Field},
};
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextArea, TextInput};
use std::time::Duration;

fn button(label: &str, action: impl Fn(&mut Viewer) + 'static, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .on_press(action)
        .build(k)
}
fn owns(s: &Viewer, owner: &crate::composer::DraftScope, scope: &Option<String>) -> bool {
    let now = s.composer_scope();
    now.connection == owner.connection
        && now.profile == owner.profile
        && s.settings.scope_profile == *scope
}
fn owned_action(
    s: &Viewer,
    label: &str,
    a: impl Fn(&mut Viewer) + 'static,
    disabled: bool,
    k: &Kit,
) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled)
        .on_press(move |s: &mut Viewer| {
            if owns(s, &owner, &scope) {
                a(s);
            }
        })
        .build(k)
}
pub fn page(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let state = &s.settings;
    let config_section = !matches!(
        state.section.as_str(),
        "keys"
            | "providers"
            | "vault"
            | "plugins"
            | "web-access"
            | "billing"
            | "local-models"
            | "delegation"
    );
    let mut nav = col()
        .w(190.0)
        .min_w(160.0)
        .gap(3.0)
        .px(12.0)
        .py(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    nav = nav.child(
        text("Settings")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .px(8.0)
            .pb(14.0),
    );
    for (id, label) in state.sections() {
        let selected = state.section == id;
        nav = nav.child(
            button(
                &label,
                move |s| s.settings_action(Action::Section(id.clone())),
                k,
            )
            .w_full()
            .justify_start()
            .fill(if selected {
                k.c.layer_01
            } else {
                rune::Color::TRANSPARENT
            }),
        );
    }
    if matches!(
        state.section.as_str(),
        "appearance" | "shortcuts" | "about" | "connections"
    ) {
        return row()
            .key("settings-page")
            .w_full()
            .h_full()
            .min_h(0.0)
            .child(nav)
            .child(crate::view::client::page(s, k, layer).grow(1.0).min_w(0.0));
    }
    let options: Vec<String> = std::iter::once("Current conversation".into())
        .chain(s.profiles.iter().map(|p| p.name.clone()))
        .collect();
    let scope_picker = Select::new("settings-profile-scope", &options)
        .current(Some(
            state
                .scope_profile
                .as_deref()
                .unwrap_or("Current conversation"),
        ))
        .width(220.0)
        .open(state.open_select.as_deref() == Some("__scope"))
        .disabled(state.demo || state.blocks_owner_change())
        .on_toggle(|s: &mut Viewer| {
            s.settings.open_select = if s.settings.open_select.as_deref() == Some("__scope") {
                None
            } else {
                Some("__scope".into())
            }
        })
        .on_close(|s: &mut Viewer| s.settings.open_select = None)
        .on_choose(|s: &mut Viewer, value, tx| {
            s.settings.open_select = None;
            s.settings_action(Action::Scope(if value == "Current conversation" {
                None
            } else {
                Some(value.into())
            }));
            tx.instant();
        })
        .build(k, layer);
    let mut toolbar = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            TextInput::new(&state.search)
                .placeholder("Search settings…")
                .width(None)
                .on_edit(|s: &mut Viewer| &mut s.settings.search)
                .build(k)
                .grow(1.0)
                .min_w(120.0)
                .key("settings-search"),
        )
        .child(
            Button::new("Refresh")
                .variant(ButtonVariant::Ghost)
                .disabled(state.loading || state.saving || state.dirty_count() > 0)
                .on_press(|s: &mut Viewer| {
                    let action = match s.settings.section.as_str() {
                        "keys" => Action::EnvLoad,
                        "plugins" => Action::PluginLoad,
                        "delegation" => {
                            Action::Delegation(crate::pages::settings::delegation::Action::Load)
                        }
                        "local-models" => {
                            Action::LocalModels(crate::pages::settings::local_models::Action::Load)
                        }
                        "billing" => Action::Billing(crate::pages::settings::billing::Action::Load),
                        "web-access" => {
                            Action::Web(crate::pages::settings::web_access::Action::Load)
                        }
                        "vault" => Action::Vault(crate::pages::settings::vault::Action::Load),
                        "memory" => Action::Memory(crate::pages::settings::memory::Action::Load),
                        "providers" => {
                            Action::Provider(crate::pages::settings::providers::Action::Load)
                        }
                        _ => Action::Load,
                    };
                    s.settings_action(action);
                })
                .build(k)
                .key("settings-refresh"),
        )
        .children(config_section.then(|| {
            Button::new("Save")
                .disabled(state.demo || state.saving || state.dirty_count() == 0)
                .on_press(|s: &mut Viewer| s.settings_action(Action::Save))
                .build(k)
                .key("settings-save")
        }));
    if config_section {
        toolbar = toolbar
            .child(files::import(s, k))
            .child(files::export(s, k));
    }
    let mut body = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .gap(14.0)
        .px(28.0)
        .py(20.0)
        .child(toolbar);
    let status = if state.saving {
        "Saving…".to_owned()
    } else if state.dirty_count() > 0 {
        format!("{} unsaved changes", state.dirty_count())
    } else {
        state
            .notice
            .clone()
            .unwrap_or_else(|| "Changes save automatically".into())
    };
    if state.section == "local-models" {
        body = body.child(
            text(format!("Active agent profile: {}", s.profile))
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    } else {
        body = body.child(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .child(text("Agent profile").font(k.ty.small).color(k.c.text_muted))
                .child(scope_picker),
        );
    }
    if config_section {
        body = body.child(
            row()
                .w_full()
                .center_y()
                .child(text(status).font(k.ty.small).color(k.c.text_muted))
                .child(el().grow(1.0))
                .child(button(
                    "Revert unsaved",
                    |s| s.settings_action(Action::Discard),
                    k,
                ))
                .child(button(
                    "Reset to defaults",
                    |s| s.settings_action(Action::Defaults),
                    k,
                )),
        );
    }
    if state.demo {
        body = body.child(
            text("Demonstration data · backend writes are disabled")
                .font(k.ty.small)
                .color(k.c.text_faint),
        );
    }
    if let Some(error) = &state.error {
        body = body.child(
            text(error)
                .role("alert")
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(confirm) = &state.confirm {
        let message = if confirm == "toolsets" {
            "Disable every toolset? The agent will lose access to tools until you enable them again."
        } else if confirm == "defaults" {
            "Reset settings to the agent defaults? This replaces your current settings and saves the defaults."
        } else {
            "Remove this credential from the selected agent profile?"
        };
        let id = confirm.clone();
        body = body.child(
            col()
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(12.0)
                .fill(k.c.layer_01)
                .child(text(message).font(k.ty.ui).color(k.c.text_base).wrap())
                .child(
                    row()
                        .gap(8.0)
                        .child(button(
                            "Confirm",
                            move |s| s.settings_action(Action::Confirm(id.clone())),
                            k,
                        ))
                        .child(button(
                            "Cancel",
                            |s| s.settings_action(Action::CancelConfirm),
                            k,
                        )),
                ),
        );
    }
    let mut fields = col()
        .w_full()
        .gap(2.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .grow(1.0)
        .min_h(0.0);
    if state.section == "providers" {
        fields = fields.children(providers::rows(s, k));
    } else if state.section == "delegation" {
        fields = fields.children(delegation::rows(s, k));
    } else if state.section == "local-models" {
        fields = fields.children(local_models::rows(s, k));
    } else if state.section == "billing" {
        fields = fields.children(billing::rows(s, k));
    } else if state.section == "web-access" {
        fields = fields.children(web_access::rows(s, k));
    } else if state.section == "plugins" {
        fields = fields.children(plugins::rows(s, k));
    } else if state.section == "vault" {
        fields = fields.children(vault::rows(s, k, layer));
    } else if state.section == "keys" {
        fields = fields.children(env_rows(s, k));
    } else if state.loading && state.fields.is_empty() {
        fields = fields.child(
            text("Loading settings from the selected agent…")
                .font(k.ty.ui)
                .color(k.c.text_muted),
        );
    } else {
        if state.section == "model" && state.search.text.trim().is_empty() {
            fields = fields.children(models::rows(s, k, layer));
            fields = fields.children(moa::rows(s, k, layer));
            fields = fields.child(
                text("Other model settings")
                    .font(k.ty.body_medium)
                    .color(k.c.text_base)
                    .pt(18.0),
            );
        }
        if state.section == "memory" && state.search.text.trim().is_empty() {
            fields = fields.children(memory::rows(s, k, layer));
        }
        let tool = match state.section.as_str() {
            "workspace" => Some(("Terminal backend readiness", "terminal")),
            "browser" => Some(("Browser connection & profile consent", "browser")),
            "advanced" => Some(("Computer Use health & permissions", "computer_use")),
            _ => None,
        };
        if let Some((label, name)) = tool {
            fields = fields.child(owned_action(
                s,
                label,
                move |s| s.settings_action(Action::Tool(name.into())),
                state.demo,
                k,
            ));
        }
        for field in state.visible_fields() {
            fields = fields.child(field_row(s, field, k, layer));
        }
        if state.visible_fields().is_empty() {
            fields = fields.child(
                text("No matching settings")
                    .font(k.ty.ui)
                    .color(k.c.text_muted),
            );
        }
    }
    body = body.child(fields);
    row()
        .key("settings-page")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(nav)
        .child(body)
}

fn field_row(s: &Viewer, field: &Field, k: &Kit, layer: &mut Vec<El>) -> El {
    let owner = s.composer_scope();
    let settings_scope = s.settings.scope_profile.clone();
    let key = field.key.clone();
    let label = field.label.clone();
    let heading = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(
            text(&field.label)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .wrap(),
        )
        .children((!field.description.is_empty()).then(|| {
            text(&field.description)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap()
        }))
        .child(text(&field.key).font(k.ty.small).color(k.c.text_faint));
    let mut line = row()
        .key(format!("setting-{}", field.key))
        .w_full()
        .min_w(0.0)
        .gap(24.0)
        .px(4.0)
        .py(13.0)
        .center_y()
        .child(heading.clone());
    if field.key == "fallback_providers" {
        return fallback_rows(s, heading, k);
    }
    if field.kind == "boolean" {
        let next = field.editor.text != "true";
        return line.child(
            Switch::new(!next)
                .label(label)
                .hide_label()
                .on_toggle_tx(move |s: &mut Viewer, tx| {
                    if !owns(s, &owner, &settings_scope) {
                        return;
                    }
                    s.settings_action(Action::Value(key.clone(), serde_json::Value::Bool(next)));
                    tx.wake_after(Duration::from_millis(560));
                })
                .build(k),
        );
    }
    if !field.options.is_empty() && !field.free_input {
        let choose_key = key.clone();
        let close_key = key.clone();
        let options = field.options.clone();
        return line.child(
            Select::new(format!("settings-select-{}", field.key), &options)
                .current(Some(&field.editor.text))
                .placeholder("Not set")
                .width(270.0)
                .open(s.settings.open_select.as_ref() == Some(&field.key))
                .on_toggle(move |s: &mut Viewer| {
                    s.settings.open_select = if s.settings.open_select.as_ref() == Some(&key) {
                        None
                    } else {
                        Some(key.clone())
                    };
                })
                .on_close(move |s: &mut Viewer| {
                    if s.settings.open_select.as_ref() == Some(&close_key) {
                        s.settings.open_select = None;
                    }
                })
                .on_choose(move |s: &mut Viewer, value, tx| {
                    if !owns(s, &owner, &settings_scope) {
                        return;
                    }
                    s.settings_action(Action::Value(
                        choose_key.clone(),
                        serde_json::Value::String(value.into()),
                    ));
                    tx.wake_after(Duration::from_millis(560));
                    tx.instant();
                })
                .build(k, layer),
        );
    }
    let long = matches!(field.kind.as_str(), "text" | "object")
        || field.editor.text.len() > 100
        || field.key == "fallback_providers";
    let edit_key = key.clone();
    if long {
        let editor = TextArea::new(&field.editor)
            .width(None)
            .placeholder("Not set")
            .rows(5)
            .max_height(140.0)
            .on_edit(move |s: &mut Viewer| {
                let scope = s.composer_scope();
                if scope.connection != owner.connection
                    || scope.profile != owner.profile
                    || s.settings.scope_profile != settings_scope
                {
                    return s.settings.edit_mut("");
                }
                s.settings.edit_mut(&edit_key)
            })
            .on_change(|s: &mut Viewer, tx| {
                s.settings_action(Action::Edited);
                tx.wake_after(Duration::from_millis(560));
            })
            .build(k)
            .key(format!("settings-input-{}", field.key))
            .w_full();
        return col().w_full().py(10.0).gap(8.0).child(line).child(editor);
    }
    line = line.child(
        TextInput::new(&field.editor)
            .placeholder(if field.kind == "list" {
                "Comma separated"
            } else {
                "Not set"
            })
            .width(Some(270.0))
            .on_edit(move |s: &mut Viewer| {
                let scope = s.composer_scope();
                if scope.connection != owner.connection
                    || scope.profile != owner.profile
                    || s.settings.scope_profile != settings_scope
                {
                    return s.settings.edit_mut("");
                }
                s.settings.edit_mut(&edit_key)
            })
            .on_change(|s: &mut Viewer, tx| {
                s.settings_action(Action::Edited);
                tx.wake_after(Duration::from_millis(560));
            })
            .build(k)
            .key(format!("settings-input-{}", field.key)),
    );
    if field.free_input {
        line = line.help(format!(
            "Suggestions: {}. Custom values are accepted.",
            field.options.join(", ")
        ));
    }
    if field.key == "tts.elevenlabs.voice_id" {
        let mut choices = col().gap(6.0).w(270.0);
        let labels = s
            .settings
            .voice_catalog
            .iter()
            .map(|(id, label)| format!("{label} · {id}"))
            .collect::<Vec<_>>();
        if !labels.is_empty() {
            let lookup = s
                .settings
                .voice_catalog
                .iter()
                .map(|(id, label)| (format!("{label} · {id}"), id.clone()))
                .collect::<std::collections::BTreeMap<_, _>>();
            let current = s
                .settings
                .voice_catalog
                .get(&field.editor.text)
                .map(|label| format!("{label} · {}", field.editor.text));
            let owner = s.composer_scope();
            let scope = s.settings.scope_profile.clone();
            choices = choices.child(
                Select::new("settings-elevenlabs-voices", &labels)
                    .current(current.as_deref())
                    .placeholder("Choose an account voice")
                    .width(270.0)
                    .open(s.settings.open_select.as_deref() == Some("__voices"))
                    .disabled(s.settings.demo || s.settings.voice_loading)
                    .on_toggle(|s: &mut Viewer| {
                        s.settings.open_select =
                            if s.settings.open_select.as_deref() == Some("__voices") {
                                None
                            } else {
                                Some("__voices".into())
                            }
                    })
                    .on_close(|s: &mut Viewer| s.settings.open_select = None)
                    .on_choose(move |s: &mut Viewer, value, tx| {
                        if owns(s, &owner, &scope) {
                            if let Some(id) = lookup.get(value) {
                                s.settings_action(Action::Value(
                                    "tts.elevenlabs.voice_id".into(),
                                    serde_json::json!(id),
                                ));
                                tx.wake_after(Duration::from_millis(560));
                                tx.instant();
                            }
                        }
                    })
                    .build(k, layer),
            );
        }
        choices = choices.child(owned_action(
            s,
            if s.settings.voice_loading {
                "Loading voices…"
            } else {
                "Refresh account voices"
            },
            |s| s.settings_action(Action::VoicesLoad),
            s.settings.voice_loading || s.settings.demo,
            k,
        ));
        return col()
            .w_full()
            .gap(4.0)
            .child(line)
            .child(row().w_full().child(el().grow(1.0)).child(choices));
    }
    line
}
fn env_rows(s: &Viewer, k: &Kit) -> Vec<El> {
    let state = &s.settings;
    let mut rows = vec![
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                text("API keys & environment")
                    .font(k.ty.body_medium)
                    .color(k.c.text_base),
            )
            .child(el().grow(1.0))
            .child(button("Refresh", |s| s.settings_action(Action::EnvLoad), k)),
    ];
    rows.push(text("Keys stay on the selected agent. Values are hidden until explicitly revealed; channel-managed credentials live in Messaging.").font(k.ty.small).color(k.c.text_muted).wrap());
    for field in &state.env {
        let query = state.search.text.to_lowercase();
        if !query.is_empty()
            && !format!("{} {} {}", field.key, field.category, field.description)
                .to_lowercase()
                .contains(&query)
        {
            continue;
        }
        let owner = s.composer_scope();
        let scope = s.settings.scope_profile.clone();
        let key = field.key.clone();
        let save_key = key.clone();
        let clear_key = key.clone();
        let reveal_key = key.clone();
        let hide_key = key.clone();
        let revealed = field.revealed.is_some();
        let mut input = TextInput::new(&field.editor)
            .placeholder(if field.is_set {
                "Replace saved value"
            } else {
                "Not set"
            })
            .width(None)
            .on_edit(move |s: &mut Viewer| {
                if !owns(s, &owner, &scope) {
                    return s.settings.edit_mut("");
                }
                s.settings.env_edit_mut(&key)
            });
        if field.password {
            input = input.secure();
        }
        let mut item = col()
            .w_full()
            .gap(7.0)
            .py(12.0)
            .child(
                row()
                    .w_full()
                    .child(text(&field.key).font(k.ty.ui_medium).color(k.c.text_base))
                    .child(el().grow(1.0))
                    .child(
                        text(if field.is_set {
                            "Configured"
                        } else {
                            "Not set"
                        })
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                    ),
            )
            .child(
                text(&field.description)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap(),
            );
        if let Some(value) = &field.revealed {
            item = item.child(
                text(value)
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .selectable(true)
                    .wrap(),
            );
        }
        let actions = row()
            .w_full()
            .gap(8.0)
            .child(input.build(k).grow(1.0).min_w(0.0))
            .child(owned_action(
                s,
                "Save",
                move |s| s.settings_action(Action::EnvSave(save_key.clone())),
                state.demo || state.env_saving.is_some() || field.editor.text.is_empty(),
                k,
            ))
            .child(owned_action(
                s,
                if revealed { "Hide" } else { "Reveal" },
                move |s| {
                    s.settings_action(if revealed {
                        Action::EnvHide(hide_key.clone())
                    } else {
                        Action::EnvReveal(reveal_key.clone())
                    })
                },
                state.demo || state.env_saving.is_some() || !field.is_set,
                k,
            ))
            .child(owned_action(
                s,
                "Remove",
                move |s| s.settings_action(Action::EnvDelete(clear_key.clone())),
                state.demo || state.env_saving.is_some() || !field.is_set,
                k,
            ));
        rows.push(item.child(actions));
    }
    let key_owner = s.composer_scope();
    let value_owner = key_owner.clone();
    let key_scope = s.settings.scope_profile.clone();
    let value_scope = key_scope.clone();
    let new_key = state.env_key.text.clone();
    rows.push(
        col()
            .w_full()
            .gap(8.0)
            .pt(20.0)
            .child(
                text("Add environment variable")
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
            )
            .child(
                row()
                    .w_full()
                    .gap(8.0)
                    .child(
                        TextInput::new(&state.env_key)
                            .placeholder("VARIABLE_NAME")
                            .width(Some(220.0))
                            .on_edit(move |s: &mut Viewer| {
                                if !owns(s, &key_owner, &key_scope) {
                                    return s.settings.edit_mut("");
                                }
                                &mut s.settings.env_key
                            })
                            .build(k),
                    )
                    .child(
                        TextInput::new(&state.env_value)
                            .secure()
                            .placeholder("Value")
                            .width(None)
                            .on_edit(move |s: &mut Viewer| {
                                if !owns(s, &value_owner, &value_scope) {
                                    return s.settings.edit_mut("");
                                }
                                &mut s.settings.env_value
                            })
                            .build(k)
                            .grow(1.0)
                            .min_w(0.0),
                    )
                    .child(owned_action(
                        s,
                        "Add",
                        move |s| s.settings_action(Action::EnvSave(new_key.clone())),
                        state.demo || state.env_saving.is_some() || state.env_key.text.is_empty(),
                        k,
                    )),
            ),
    );
    rows
}

fn fallback_rows(s: &Viewer, heading: El, k: &Kit) -> El {
    let mut list = col().w_full().gap(8.0).py(12.0).child(heading);
    for (index, entry) in s.settings.fallback_rows.iter().enumerate() {
        let scope = s.settings.scope_profile.clone();
        let model_scope = scope.clone();
        let remove_scope = scope.clone();
        let owner = s.composer_scope();
        let model_owner = owner.clone();
        let remove_owner = owner.clone();
        list = list.child(
            row()
                .w_full()
                .min_w(0.0)
                .gap(8.0)
                .center_y()
                .child(
                    text((index + 1).to_string())
                        .font(k.ty.small)
                        .color(k.c.text_faint)
                        .w(20.0),
                )
                .child(
                    TextInput::new(&entry.provider)
                        .placeholder("Provider")
                        .width(None)
                        .on_edit(move |s: &mut Viewer| {
                            let now = s.composer_scope();
                            if now.connection != owner.connection
                                || now.profile != owner.profile
                                || s.settings.scope_profile != scope
                            {
                                return s.settings.edit_mut("");
                            }
                            s.settings.fallback_edit_mut(index, true)
                        })
                        .on_change(|s: &mut Viewer, tx| {
                            s.settings_action(Action::FallbackEdited);
                            tx.wake_after(Duration::from_millis(560));
                        })
                        .build(k)
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(
                    TextInput::new(&entry.model)
                        .placeholder("Model")
                        .width(None)
                        .on_edit(move |s: &mut Viewer| {
                            let now = s.composer_scope();
                            if now.connection != model_owner.connection
                                || now.profile != model_owner.profile
                                || s.settings.scope_profile != model_scope
                            {
                                return s.settings.edit_mut("");
                            }
                            s.settings.fallback_edit_mut(index, false)
                        })
                        .on_change(|s: &mut Viewer, tx| {
                            s.settings_action(Action::FallbackEdited);
                            tx.wake_after(Duration::from_millis(560));
                        })
                        .build(k)
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(
                    Button::new("Remove")
                        .variant(ButtonVariant::Ghost)
                        .on_press_tx(move |s: &mut Viewer, tx| {
                            if !owns(s, &remove_owner, &remove_scope) {
                                return;
                            }
                            s.settings_action(Action::FallbackRemove(index));
                            tx.wake_after(Duration::from_millis(560));
                        })
                        .build(k),
                ),
        );
    }
    list.child(button(
        "Add fallback model",
        |s| s.settings_action(Action::FallbackAdd),
        k,
    ))
}
