use crate::{
    app::Viewer,
    pages::settings::{
        Action,
        vault::{self, Action as VaultAction, Vault},
    },
};
use rune::{El, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextInput};
fn action(s: &Viewer, label: &str, a: VaultAction, disabled: bool, k: &Kit) -> El {
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
                s.settings_action(Action::Vault(a.clone()));
                tx.instant();
            }
        })
        .build(k)
}
fn input(
    s: &Viewer,
    label: &str,
    value: &TextEdit,
    secure: bool,
    access: impl Fn(&mut Vault) -> &mut TextEdit + 'static,
    k: &Kit,
) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    let mut input = TextInput::new(value)
        .width(None)
        .disabled(s.settings.vault.busy || s.settings.demo)
        .on_edit(move |s: &mut Viewer| {
            let now = s.composer_scope();
            if now.connection != owner.connection
                || now.profile != owner.profile
                || s.settings.scope_profile != scope
            {
                return s.settings.edit_mut("");
            }
            access(&mut s.settings.vault)
        });
    if secure {
        input = input.secure();
    }
    col()
        .w_full()
        .gap(5.0)
        .child(text(label).font(k.ty.small_medium).color(k.c.text_muted))
        .child(input.build(k))
}
fn select(
    s: &Viewer,
    id: &str,
    current: &str,
    values: &[&str],
    a: fn(String) -> VaultAction,
    k: &Kit,
    layer: &mut Vec<El>,
) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    let open_id = id.to_owned();
    let close_id = id.to_owned();
    let options = values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
    Select::new(format!("vault-{id}"), &options)
        .current(Some(current))
        .width(240.0)
        .open(s.settings.vault.open.as_deref() == Some(id))
        .disabled(s.settings.vault.busy || s.settings.demo)
        .on_toggle(move |s: &mut Viewer| {
            s.settings.vault.open = if s.settings.vault.open.as_ref() == Some(&open_id) {
                None
            } else {
                Some(open_id.clone())
            }
        })
        .on_close(move |s: &mut Viewer| {
            if s.settings.vault.open.as_ref() == Some(&close_id) {
                s.settings.vault.open = None;
            }
        })
        .on_choose(move |s: &mut Viewer, v, tx| {
            let now = s.composer_scope();
            if now.connection == owner.connection
                && now.profile == owner.profile
                && s.settings.scope_profile == scope
            {
                s.settings_action(Action::Vault(a(v.into())));
                tx.instant();
            }
        })
        .build(k, layer)
}
pub(super) fn rows(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let v = &s.settings.vault;
    let mut out=vec![row().w_full().gap(8.0).center_y().child(text("Credential vault").font(k.ty.body_medium).color(k.c.text_base).grow(1.0)).child(action(s,"Refresh",VaultAction::Load,v.loading||v.busy,k)).child(action(s,"Add credential",VaultAction::Add,v.busy,k)),text("Vault entries belong to the selected connection and agent profile. The list exposes metadata; saved passwords and payment details stay on the agent.").font(k.ty.small).color(k.c.text_muted).wrap()];
    if let Some(e) = &v.error {
        out.push(
            text(e)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if let Some(n) = &v.notice {
        out.push(text(n).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if v.loading && v.items.is_empty() {
        out.push(
            text("Loading vault metadata…")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    } else if v.items.is_empty() {
        out.push(
            text("No vault credentials yet")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .py(12.0),
        );
    }
    for item in &v.items {
        let query = s.settings.search.text.to_lowercase();
        if !query.is_empty()
            && !format!(
                "{} {} {} {}",
                item.label, item.origin, item.identifier, item.kind
            )
            .to_lowercase()
            .contains(&query)
        {
            continue;
        }
        let mut line = row().w_full().gap(10.0).py(12.0).center_y().child(
            col()
                .grow(1.0)
                .gap(5.0)
                .child(
                    text(format!(
                        "{} · {}{}",
                        item.label,
                        item.kind,
                        if item.otp { " · 2FA" } else { "" }
                    ))
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
                )
                .child(
                    text(format!(
                        "{}{}{} · {}",
                        item.identifier,
                        if item.identifier.is_empty() {
                            ""
                        } else {
                            " · "
                        },
                        item.origin,
                        item.created
                    ))
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap(),
                ),
        );
        if item.backend == "local" {
            line = line.child(action(
                s,
                "Delete",
                VaultAction::Delete(item.id.clone()),
                v.busy,
                k,
            ));
        } else {
            line = line.child(text(&item.backend).font(k.ty.small).color(k.c.text_muted));
        }
        out.push(line);
    }
    if let Some(id) = &v.confirm_delete {
        let label = v
            .items
            .iter()
            .find(|i| &i.id == id)
            .map(|i| i.label.as_str())
            .unwrap_or("this credential");
        out.push(
            col()
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(12.0)
                .fill(k.c.layer_01)
                .child(
                    text(format!(
                        "Permanently delete {label} from this agent's vault?"
                    ))
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .wrap(),
                )
                .child(
                    row()
                        .gap(8.0)
                        .child(action(
                            s,
                            "Delete credential",
                            VaultAction::ConfirmDelete,
                            v.busy,
                            k,
                        ))
                        .child(action(s, "Cancel", VaultAction::CancelDelete, v.busy, k)),
                ),
        );
    }
    if v.adding {
        let mut form = col()
            .w_full()
            .gap(12.0)
            .px(14.0)
            .py(14.0)
            .fill(k.c.layer_01)
            .child(
                text("Add vault credential")
                    .font(k.ty.body_medium)
                    .color(k.c.text_base),
            )
            .child(select(
                s,
                "kind",
                &v.kind,
                &["login", "payment", "address"],
                VaultAction::Kind,
                k,
                layer,
            ))
            .child(input(s, "Label", &v.label, false, |v| &mut v.label, k))
            .child(input(
                s,
                "Allowed website origin",
                &v.origin,
                false,
                |v| &mut v.origin,
                k,
            ));
        if v.kind == "login" {
            form = form.child(select(
                s,
                "identifier-type",
                &v.identifier_type,
                &["email", "phone", "username"],
                VaultAction::IdentifierType,
                k,
                layer,
            ));
        }
        for f in vault::fields(&v.kind) {
            let key = f.key;
            form = form.child(input(
                s,
                f.label,
                &v.inputs[f.key],
                f.secure,
                move |v| v.edit(key),
                k,
            ));
        }
        out.push(
            form.child(
                row()
                    .gap(8.0)
                    .child(action(
                        s,
                        if v.busy {
                            "Saving…"
                        } else {
                            "Save encrypted credential"
                        },
                        VaultAction::Save,
                        v.busy,
                        k,
                    ))
                    .child(action(s, "Cancel", VaultAction::Dismiss, v.busy, k)),
            ),
        );
    }
    out.push(
        text("Password manager sources")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(18.0),
    );
    for source in v.sources.iter().filter(|s| s.needs_unlock) {
        let name = source.name.clone();
        let owner = s.composer_scope();
        let scope = s.settings.scope_profile.clone();
        let next = !source.enabled;
        let mut line = row().w_full().gap(10.0).py(12.0).center_y().child(
            col()
                .grow(1.0)
                .gap(4.0)
                .child(
                    text(&source.label)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                )
                .child(
                    text(if !source.installed {
                        "Not detected on the agent"
                    } else if !source.enabled {
                        "Disabled"
                    } else if source.unlocked {
                        "Unlocked"
                    } else {
                        "Locked"
                    })
                    .font(k.ty.small)
                    .color(k.c.text_muted),
                ),
        );
        if source.installed {
            line = line.child(
                Switch::new(source.enabled)
                    .disabled(v.busy || s.settings.demo)
                    .on_toggle(move |s: &mut Viewer| {
                        let now = s.composer_scope();
                        if now.connection == owner.connection
                            && now.profile == owner.profile
                            && s.settings.scope_profile == scope
                        {
                            s.settings_action(Action::Vault(VaultAction::Toggle(
                                name.clone(),
                                next,
                            )));
                        }
                    })
                    .build(k),
            );
            if source.enabled {
                line = line.child(action(
                    s,
                    if source.unlocked { "Lock" } else { "Unlock" },
                    if source.unlocked {
                        VaultAction::Lock(source.name.clone())
                    } else {
                        VaultAction::Unlock(source.name.clone())
                    },
                    v.busy,
                    k,
                ));
            }
        }
        out.push(line);
    }
    if let Some(name) = &v.unlock {
        let label = v
            .sources
            .iter()
            .find(|s| &s.name == name)
            .map(|s| s.label.as_str())
            .unwrap_or(name);
        out.push(
            col()
                .w_full()
                .gap(10.0)
                .px(14.0)
                .py(14.0)
                .fill(k.c.layer_01)
                .child(
                    text(format!("Unlock {label}"))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                )
                .child(input(
                    s,
                    "Master password",
                    &v.password,
                    true,
                    |v| &mut v.password,
                    k,
                ))
                .child(
                    row()
                        .gap(8.0)
                        .child(action(
                            s,
                            "Unlock source",
                            VaultAction::SubmitUnlock,
                            v.busy,
                            k,
                        ))
                        .child(action(s, "Cancel", VaultAction::Dismiss, v.busy, k)),
                ),
        );
    }
    out
}
