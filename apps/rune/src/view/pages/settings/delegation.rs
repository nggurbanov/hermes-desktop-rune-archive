use crate::{
    app::Viewer,
    pages::settings::{
        Action,
        delegation::{Action as DelegationAction, FIELDS},
    },
};
use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Switch, TextInput};
use serde_json::Value;
fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn action(s: &Viewer, label: &str, a: DelegationAction, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.settings.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection == owner.connection
                && now.profile == owner.profile
                && s.settings.scope_profile == scope
            {
                s.settings_action(Action::Delegation(a.clone()));
                tx.instant();
            }
        })
        .build(k)
}
pub(super) fn rows(s: &Viewer, k: &Kit) -> Vec<El> {
    let d = &s.settings.delegation;
    let data = &d.data;
    let mut out=vec![row().w_full().gap(8.0).center_y().child(text("Subagent routing").font(k.ty.body_medium).color(k.c.text_base).grow(1.0)).child(action(s,"Refresh routing",DelegationAction::Load,d.loading||d.busy,k)),text("Choose whether subagents inherit their model, select a named model route, or run with an allowed bot identity. Backend availability comes from the agent.").font(k.ty.small).color(k.c.text_muted).wrap()];
    if let Some(e) = &d.error {
        out.push(
            text(e)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if let Some(n) = &d.notice {
        out.push(text(n).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if !data.is_object() {
        out.push(
            text(if d.loading {
                "Loading delegation backends…"
            } else {
                "Delegation routing was not supplied by this agent. Refresh to retry."
            })
            .font(k.ty.small)
            .color(k.c.text_muted),
        );
        return out;
    }
    if let Some(reason) = data["blocked_reason"].as_str().filter(|s| !s.is_empty()) {
        out.push(
            text(format!("Subagents are blocked: {reason}"))
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    for backend in data["backends"].as_array().into_iter().flatten() {
        let name = string(backend, "name");
        let current = data["configured"] == name;
        out.push(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .py(12.0)
                .child(
                    col()
                        .grow(1.0)
                        .gap(4.0)
                        .child(
                            text(format!(
                                "{}{}",
                                string(backend, "label"),
                                if current { " · In use" } else { "" }
                            ))
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base),
                        )
                        .child(
                            text(string(backend, "description"))
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .wrap(),
                        )
                        .children((!string(backend, "reason").is_empty()).then(|| {
                            text(string(backend, "reason"))
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .wrap()
                        })),
                )
                .child(action(
                    s,
                    "Use backend",
                    DelegationAction::Backend(name.into()),
                    d.busy || current || backend["available"].as_bool() != Some(true) || d.dirty(),
                    k,
                )),
        );
    }
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    out.push(
        Button::new("Manage delegation plugins")
            .variant(ButtonVariant::Ghost)
            .disabled(d.busy || s.settings.demo)
            .on_press(move |s: &mut Viewer| {
                let now = s.composer_scope();
                if now.connection == owner.connection
                    && now.profile == owner.profile
                    && s.settings.scope_profile == scope
                {
                    s.settings_action(Action::PluginsManager);
                }
            })
            .build(k),
    );
    if data["configured"] == "model_map" {
        out.push(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .pt(16.0)
                .child(
                    text("Named model routes")
                        .font(k.ty.body_medium)
                        .color(k.c.text_base)
                        .grow(1.0),
                )
                .child(action(
                    s,
                    "Add route",
                    DelegationAction::New,
                    d.busy || d.editing,
                    k,
                )),
        );
        for route in data["model_map"]["options"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let name = string(route, "name");
            out.push(
                col()
                    .w_full()
                    .gap(6.0)
                    .py(12.0)
                    .child(
                        text(format!(
                            "{}{}",
                            name,
                            if data["model_map"]["default"] == name {
                                " · Default"
                            } else {
                                ""
                            }
                        ))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                    )
                    .child(
                        text(string(route, "description"))
                            .font(k.ty.small)
                            .color(k.c.text_muted)
                            .wrap(),
                    )
                    .child(
                        text(format!(
                            "{} · {}{}{}",
                            route["provider"]
                                .as_str()
                                .or_else(|| route["base_url"].as_str())
                                .unwrap_or(""),
                            string(route, "model"),
                            route["reasoning_effort"]
                                .as_str()
                                .map(|e| format!(" · {e}"))
                                .unwrap_or_default(),
                            route["service_tier"]
                                .as_str()
                                .map(|e| format!(" · {e}"))
                                .unwrap_or_default()
                        ))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap(),
                    )
                    .child(
                        row()
                            .gap(8.0)
                            .child(action(
                                s,
                                "Edit route",
                                DelegationAction::Edit(name.into()),
                                d.busy || d.editing,
                                k,
                            ))
                            .child(action(
                                s,
                                "Make default",
                                DelegationAction::DefaultRoute(name.into()),
                                d.busy || d.editing || data["model_map"]["default"] == name,
                                k,
                            ))
                            .child(action(
                                s,
                                "Delete route",
                                DelegationAction::Delete(name.into()),
                                d.busy || d.editing,
                                k,
                            )),
                    ),
            );
        }
        if let Some(name) = &d.confirm_delete {
            out.push(col().w_full().gap(8.0).px(12.0).py(12.0).fill(k.c.layer_01).child(text(format!("Delete route {name}? If it is the default, the next available route becomes default.")).font(k.ty.ui).color(k.c.text_base).wrap()).child(row().gap(8.0).child(action(s,"Delete route",DelegationAction::ConfirmDelete,d.busy,k)).child(action(s,"Keep route",DelegationAction::CancelDelete,d.busy,k))));
        }
        if d.editing {
            let mut form = col()
                .w_full()
                .gap(10.0)
                .px(14.0)
                .py(14.0)
                .fill(k.c.layer_01)
                .child(
                    text(if d.original.is_some() {
                        "Edit model route"
                    } else {
                        "New model route"
                    })
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
                );
            for (key, label) in FIELDS {
                let owner = s.composer_scope();
                let scope = s.settings.scope_profile.clone();
                form = form.child(
                    col()
                        .w_full()
                        .gap(4.0)
                        .child(text(label).font(k.ty.small).color(k.c.text_muted))
                        .child(
                            TextInput::new(&d.fields[key])
                                .width(None)
                                .disabled(d.busy || s.settings.demo)
                                .on_edit(move |s: &mut Viewer| {
                                    let now = s.composer_scope();
                                    if now.connection != owner.connection
                                        || now.profile != owner.profile
                                        || s.settings.scope_profile != scope
                                    {
                                        return s.settings.edit_mut("");
                                    }
                                    s.settings.delegation.edit(key)
                                })
                                .build(k),
                        ),
                );
            }
            out.push(form.child(text("Blank reasoning inherits. Service tier: normal, fast, or ultrafast. Existing endpoint, key and fallback routing details are retained.").font(k.ty.small).color(k.c.text_muted).wrap()).child(row().gap(8.0).child(action(s,"Save route",DelegationAction::SaveRoute,d.busy,k)).child(action(s,"Cancel",DelegationAction::Cancel,d.busy,k))));
        }
    }
    if data["configured"] == "profile_routes" {
        out.push(
            text("Allowed profile identities")
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .pt(16.0),
        );
        for p in &d.profiles {
            let owner = s.composer_scope();
            let scope = s.settings.scope_profile.clone();
            let name = p.name.clone();
            let next = !p.enabled;
            let desc_name = p.name.clone();
            let desc_owner = owner.clone();
            let desc_scope = scope.clone();
            let mut line = col().w_full().gap(7.0).py(10.0).child(
                row()
                    .w_full()
                    .gap(10.0)
                    .center_y()
                    .child(
                        Switch::new(p.enabled)
                            .disabled(d.busy || s.settings.demo || !p.available && !p.enabled)
                            .on_toggle(move |s: &mut Viewer| {
                                let now = s.composer_scope();
                                if now.connection == owner.connection
                                    && now.profile == owner.profile
                                    && s.settings.scope_profile == scope
                                {
                                    s.settings_action(Action::Delegation(
                                        DelegationAction::ProfileToggle(name.clone(), next),
                                    ));
                                }
                            })
                            .build(k),
                    )
                    .child(
                        text(format!(
                            "{}{}{}",
                            p.name,
                            if !p.available {
                                " · Missing profile"
                            } else {
                                ""
                            },
                            if d.profile_default.as_deref() == Some(&p.name) {
                                " · Default"
                            } else {
                                ""
                            }
                        ))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .grow(1.0),
                    )
                    .child(action(
                        s,
                        "Make default",
                        DelegationAction::ProfileDefault(p.name.clone()),
                        d.busy || !p.enabled || d.profile_default.as_deref() == Some(&p.name),
                        k,
                    )),
            );
            if p.enabled {
                line = line.child(
                    TextInput::new(&p.description)
                        .placeholder("Describe when a subagent should use this identity")
                        .width(None)
                        .disabled(d.busy || s.settings.demo)
                        .on_edit(move |s: &mut Viewer| {
                            let now = s.composer_scope();
                            if now.connection != desc_owner.connection
                                || now.profile != desc_owner.profile
                                || s.settings.scope_profile != desc_scope
                            {
                                return s.settings.edit_mut("");
                            }
                            s.settings.delegation.profile_edit(&desc_name)
                        })
                        .build(k),
                );
            }
            out.push(line);
        }
        out.push(
            row()
                .gap(8.0)
                .child(action(
                    s,
                    "Save identity descriptions",
                    DelegationAction::SaveProfiles,
                    d.busy || !d.dirty(),
                    k,
                ))
                .child(action(
                    s,
                    "Revert identity changes",
                    DelegationAction::RevertProfiles,
                    d.busy || !d.dirty(),
                    k,
                )),
        );
    }
    out
}
