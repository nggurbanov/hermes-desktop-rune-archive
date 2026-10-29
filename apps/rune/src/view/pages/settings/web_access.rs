use crate::{
    app::Viewer,
    pages::settings::{Action, web_access::Action as WebAction},
};
use rune::{El, row, text};
use rune_kit::{Button, ButtonVariant, Kit, TextInput};
fn action(s: &Viewer, label: &str, a: WebAction, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.settings.demo || !s.is_local_connection())
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection == owner.connection
                && now.profile == owner.profile
                && s.settings.scope_profile == scope
            {
                s.settings_action(Action::Web(a.clone()));
                tx.instant();
            }
        })
        .build(k)
}
pub(super) fn rows(s: &Viewer, k: &Kit) -> Vec<El> {
    let w = &s.settings.web;
    let mut out = vec![
        text("Web access")
            .font(k.ty.body_medium)
            .color(k.c.text_base),
        text("Serve this agent's web app to other devices using Nous sign-in.")
            .font(k.ty.small)
            .color(k.c.text_muted)
            .wrap(),
    ];
    if !s.is_local_connection() && !s.settings.demo {
        out.push(text("Web access is managed from the local agent connection. Connect locally to configure this machine.").font(k.ty.ui).color(k.c.text_muted).wrap());
        return out;
    }
    if let Some(e) = &w.error {
        out.push(
            text(e)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if let Some(n) = &w.notice {
        out.push(text(n).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if !w.loaded {
        out.push(action(
            s,
            if w.loading {
                "Loading web access…"
            } else {
                "Retry web access status"
            },
            WebAction::Load,
            w.loading,
            k,
        ));
        return out;
    }
    let locked = w.busy || w.waiting;
    let phase = w.phase();
    out.push(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                text(if w.busy {
                    "Applying configuration…"
                } else if w.waiting {
                    "Waiting for gateway restart…"
                } else {
                    match phase {
                        "serving" => "Running",
                        "off" => "Stopped",
                        "unavailable" => "Unavailable",
                        _ => "Restart required",
                    }
                })
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .grow(1.0),
            )
            .child(action(
                s,
                "Refresh status",
                WebAction::Load,
                locked || w.loading,
                k,
            )),
    );
    if w.mode != "local" {
        out.push(
            row()
                .gap(8.0)
                .child(
                    action(
                        s,
                        "Local network",
                        WebAction::Mode("lan".into()),
                        locked || phase != "off",
                        k,
                    )
                    .fill(if w.draft_mode == "lan" {
                        k.c.layer_01
                    } else {
                        rune::Color::TRANSPARENT
                    }),
                )
                .child(
                    action(
                        s,
                        "Public HTTPS URL",
                        WebAction::Mode("public".into()),
                        locked || phase != "off",
                        k,
                    )
                    .fill(if w.draft_mode == "public" {
                        k.c.layer_01
                    } else {
                        rune::Color::TRANSPARENT
                    }),
                ),
        );
        if w.draft_mode == "public" {
            let owner = s.composer_scope();
            let scope = s.settings.scope_profile.clone();
            out.push(
                TextInput::new(&w.public_url)
                    .placeholder("https://hermes.example.com")
                    .width(None)
                    .disabled(locked || phase != "off" || s.settings.demo)
                    .on_edit(move |s: &mut Viewer| {
                        let now = s.composer_scope();
                        if now.connection != owner.connection
                            || now.profile != owner.profile
                            || s.settings.scope_profile != scope
                        {
                            return s.settings.edit_mut("");
                        }
                        &mut s.settings.web.public_url
                    })
                    .build(k),
            );
        } else {
            out.push(text(if w.lan.is_empty(){"No LAN address was reported by the agent."}else{"LAN access uses plain HTTP. Devices on the network can read traffic, including sign-in."}).font(k.ty.small).color(k.c.text_muted).wrap());
        }
    }
    let address = w.address();
    if !address.is_empty() {
        out.push(
            text(&address)
                .font(k.ty.ui)
                .color(k.c.text_base)
                .selectable(true)
                .wrap(),
        );
        if phase == "serving" {
            if let Ok(url) = url::Url::parse(&address) {
                if matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                {
                    let owner = s.composer_scope();
                    let scope = s.settings.scope_profile.clone();
                    out.push(
                        Button::new("Open web app")
                            .variant(ButtonVariant::Ghost)
                            .on_press_tx(move |s: &mut Viewer, tx| {
                                let now = s.composer_scope();
                                if now.connection == owner.connection
                                    && now.profile == owner.profile
                                    && s.settings.scope_profile == scope
                                    && tx.open_url(&address).is_err()
                                {
                                    s.settings.web.error =
                                        Some("The web app could not be opened".into());
                                }
                            })
                            .build(k),
                    );
                }
            }
        }
    }
    if !w.gateway_reason.is_empty() {
        out.push(
            text(&w.gateway_reason)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
        );
    }
    if !w.signed_in {
        out.push(
            text("Sign in to Nous in Providers & endpoints before enabling web access.")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
        );
    }
    let (label, kind) = match phase {
        "off" => ("Start web access", "start"),
        "pending_on" | "pending_off" => ("Restart gateway", "restart"),
        _ => ("Stop web access", "stop"),
    };
    out.push(
        row()
            .gap(8.0)
            .child(action(
                s,
                label,
                WebAction::Run(kind.into()),
                locked
                    || phase == "off"
                        && (!w.signed_in || w.draft_mode == "lan" && w.lan.is_empty()),
                k,
            ))
            .child(action(
                s,
                "Revert address changes",
                WebAction::Revert,
                locked || !w.dirty(),
                k,
            )),
    );
    out
}
