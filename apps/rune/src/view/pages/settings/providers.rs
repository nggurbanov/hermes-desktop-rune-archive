use crate::{
    app::Viewer,
    pages::settings::{
        Action,
        providers::{Action as ProviderAction, Providers},
    },
};
use rune::{El, TextEdit, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Switch, TextInput};
fn action(s: &Viewer, label: &str, a: ProviderAction, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let settings_scope = s.settings.scope_profile.clone();
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.settings.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection != owner.connection
                || now.profile != owner.profile
                || s.settings.scope_profile != settings_scope
            {
                return;
            }
            s.settings_action(Action::Provider(a.clone()));
            tx.instant();
        })
        .build(k)
}
fn input(
    s: &Viewer,
    label: &str,
    field: fn(&mut Providers) -> &mut TextEdit,
    value: &TextEdit,
    password: bool,
    k: &Kit,
) -> El {
    let owner = s.composer_scope();
    let settings_scope = s.settings.scope_profile.clone();
    let mut input = TextInput::new(value)
        .placeholder(label)
        .width(None)
        .disabled(s.settings.providers.busy)
        .on_edit(move |s: &mut Viewer| {
            let now = s.composer_scope();
            if now.connection != owner.connection
                || now.profile != owner.profile
                || s.settings.scope_profile != settings_scope
            {
                return s.settings.edit_mut("");
            }
            field(&mut s.settings.providers)
        });
    if password {
        input = input.secure();
    }
    col()
        .w_full()
        .gap(5.0)
        .child(text(label).font(k.ty.small_medium).color(k.c.text_muted))
        .child(input.build(k))
}
pub(super) fn rows(s: &Viewer, k: &Kit) -> Vec<El> {
    let p = &s.settings.providers;
    let mut rows = vec![
        row()
            .w_full()
            .center_y()
            .child(
                text("Provider accounts")
                    .font(k.ty.body_medium)
                    .color(k.c.text_base),
            )
            .child(el().grow(1.0))
            .child(action(
                s,
                "Refresh",
                ProviderAction::Load,
                p.busy || p.loading,
                k,
            )),
    ];
    if let Some(error) = &p.error {
        rows.push(
            text(error)
                .role("alert")
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(notice) = &p.notice {
        rows.push(text(notice).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if let Some((kind, _)) = &p.confirm {
        rows.push(
            col()
                .w_full()
                .gap(8.0)
                .py(12.0)
                .child(
                    text(if kind == "delete" {
                        "Delete this custom endpoint?"
                    } else {
                        "Disconnect this provider account?"
                    })
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
                )
                .child(
                    row()
                        .gap(8.0)
                        .child(action(s, "Confirm", ProviderAction::Confirm, p.busy, k))
                        .child(action(
                            s,
                            "Cancel",
                            ProviderAction::DismissConfirm,
                            p.busy,
                            k,
                        )),
                ),
        );
    }
    if p.loading && p.accounts.is_empty() {
        rows.push(
            text("Loading provider accounts…")
                .font(k.ty.ui)
                .color(k.c.text_muted),
        );
    }
    for account in &p.accounts {
        let mut item = col().w_full().gap(5.0).py(10.0).child(
            row()
                .w_full()
                .center_y()
                .gap(12.0)
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(
                            text(&account.name)
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base),
                        )
                        .child(
                            text(if account.logged_in {
                                "Connected"
                            } else {
                                "Not connected"
                            })
                            .font(k.ty.small)
                            .color(k.c.text_muted),
                        ),
                )
                .child(action(
                    s,
                    if account.logged_in {
                        "Sign in again"
                    } else {
                        "Sign in"
                    },
                    ProviderAction::Start(account.id.clone()),
                    p.busy || p.login.is_some() || account.flow == "external",
                    k,
                ))
                .child(action(
                    s,
                    "Disconnect",
                    ProviderAction::Disconnect(account.id.clone()),
                    p.busy
                        || !account.logged_in
                        || !account.disconnectable
                        || account.flow == "external",
                    k,
                )),
        );
        if account.flow == "external" && !account.command.is_empty() {
            item = item.child(
                text(format!(
                    "Authenticate on the agent host: {}",
                    account.command
                ))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .selectable(true)
                .wrap(),
            );
        }
        rows.push(item);
    }
    if let Some(login) = &p.login {
        let url = login.url.clone();
        let owner = s.composer_scope();
        let settings_scope = s.settings.scope_profile.clone();
        let mut status = col()
            .w_full()
            .gap(8.0)
            .px(12.0)
            .py(14.0)
            .fill(k.c.layer_01)
            .child(
                text("Complete sign-in in your browser")
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
            );
        if !login.user_code.is_empty() {
            status = status.child(
                text(format!("Device code: {}", login.user_code))
                    .font(k.ty.body_medium)
                    .color(k.c.text_base)
                    .selectable(true),
            );
        }
        status=status.child(row().gap(8.0).child(Button::new("Open sign-in page").on_press_tx(move|s:&mut Viewer,tx|{
   let now=s.composer_scope();if now.connection!=owner.connection||now.profile!=owner.profile||s.settings.scope_profile!=settings_scope{return;}
   if tx.open_url(&url).is_err(){s.settings.providers.error=Some("The browser could not be opened. Copy the sign-in address below.".into());}
  }).build(k)).child(action(s,"Check sign-in",ProviderAction::Poll,p.busy||login.polling,k)).child(action(s,"Cancel sign-in",ProviderAction::Cancel,false,k)))
   .child(text(&login.url).font(k.ty.small).color(k.c.text_muted).selectable(true).wrap());
        if login.user_code.is_empty() {
            let owner = s.composer_scope();
            let settings_scope = s.settings.scope_profile.clone();
            status = status
                .child(
                    TextInput::new(&login.code)
                        .secure()
                        .placeholder("Paste authorization code")
                        .width(None)
                        .on_edit(move |s: &mut Viewer| {
                            let now = s.composer_scope();
                            if now.connection != owner.connection
                                || now.profile != owner.profile
                                || s.settings.scope_profile != settings_scope
                            {
                                return s.settings.edit_mut("");
                            }
                            if s.settings.providers.login.is_none() {
                                return s.settings.edit_mut("");
                            }
                            &mut s.settings.providers.login.as_mut().unwrap().code
                        })
                        .build(k),
                )
                .child(action(
                    s,
                    "Submit code",
                    ProviderAction::Submit,
                    p.busy || login.code.text.trim().is_empty(),
                    k,
                ));
        }
        rows.push(status);
    }
    rows.push(
        row()
            .w_full()
            .center_y()
            .pt(22.0)
            .child(
                text("Custom endpoints")
                    .font(k.ty.body_medium)
                    .color(k.c.text_base),
            )
            .child(el().grow(1.0))
            .child(action(s, "New endpoint", ProviderAction::New, p.busy, k)),
    );
    for endpoint in &p.endpoints {
        rows.push(
            col().w_full().gap(4.0).py(10.0).child(
                row()
                    .w_full()
                    .gap(8.0)
                    .center_y()
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .child(
                                text(format!(
                                    "{}{}",
                                    endpoint.name,
                                    if endpoint.current { " · Default" } else { "" }
                                ))
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .wrap(),
                            )
                            .child(
                                text(format!(
                                    "{} · {}{}",
                                    endpoint.base_url,
                                    endpoint.model,
                                    if endpoint.has_key {
                                        " · API key configured"
                                    } else {
                                        ""
                                    }
                                ))
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .wrap(),
                            ),
                    )
                    .child(action(
                        s,
                        "Edit",
                        ProviderAction::Edit(endpoint.id.clone()),
                        p.busy,
                        k,
                    ))
                    .child(action(
                        s,
                        "Use",
                        ProviderAction::Activate(endpoint.id.clone()),
                        p.busy || endpoint.current,
                        k,
                    ))
                    .child(action(
                        s,
                        "Delete",
                        ProviderAction::Delete(endpoint.id.clone()),
                        p.busy,
                        k,
                    )),
            ),
        );
    }
    let mut form = col().w_full().gap(12.0).pt(20.0).child(
        text(if p.editing.is_some() {
            "Edit endpoint"
        } else {
            "Add endpoint"
        })
        .font(k.ty.ui_medium)
        .color(k.c.text_base),
    );
    for (label, field, value, secret) in [
        (
            "Name",
            (|p: &mut Providers| &mut p.name) as fn(&mut Providers) -> &mut TextEdit,
            &p.name,
            false,
        ),
        (
            "Base URL",
            (|p: &mut Providers| &mut p.url) as fn(&mut Providers) -> &mut TextEdit,
            &p.url,
            false,
        ),
        (
            "Model",
            (|p: &mut Providers| &mut p.model) as fn(&mut Providers) -> &mut TextEdit,
            &p.model,
            false,
        ),
        (
            "API key (blank preserves the saved key)",
            (|p: &mut Providers| &mut p.api_key) as fn(&mut Providers) -> &mut TextEdit,
            &p.api_key,
            true,
        ),
        (
            "API mode (blank uses agent detection)",
            (|p: &mut Providers| &mut p.api_mode) as fn(&mut Providers) -> &mut TextEdit,
            &p.api_mode,
            false,
        ),
        (
            "Context length (optional)",
            (|p: &mut Providers| &mut p.context_length) as fn(&mut Providers) -> &mut TextEdit,
            &p.context_length,
            false,
        ),
    ] {
        form = form.child(input(s, label, field, value, secret, k));
    }
    form = form
        .child(
            row()
                .gap(20.0)
                .child(
                    Switch::new(p.discover)
                        .label("Discover models")
                        .disabled(p.busy)
                        .on_toggle(|s: &mut Viewer| {
                            s.settings.providers.discover = !s.settings.providers.discover
                        })
                        .build(k),
                )
                .child(
                    Switch::new(p.make_default)
                        .label("Use as default")
                        .disabled(p.busy)
                        .on_toggle(|s: &mut Viewer| {
                            s.settings.providers.make_default = !s.settings.providers.make_default
                        })
                        .build(k),
                ),
        )
        .child(
            row()
                .gap(8.0)
                .child(action(
                    s,
                    "Validate endpoint",
                    ProviderAction::Validate,
                    p.busy,
                    k,
                ))
                .child(action(s, "Save endpoint", ProviderAction::Save, p.busy, k)),
        );
    rows.push(form);
    if p.dirty() {
        rows.push(action(
            s,
            "Revert endpoint changes",
            ProviderAction::Revert,
            p.busy,
            k,
        ));
    }
    rows
}
