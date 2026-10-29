use crate::{
    app::Viewer,
    pages::settings::{
        Action,
        billing::{Action as BillingAction, Billing, valid_url},
    },
};
use rune::{El, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Progress, Switch, TextInput};
use serde_json::Value;
fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn action(s: &Viewer, label: &str, a: BillingAction, disabled: bool, k: &Kit) -> El {
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
                s.settings_action(Action::Billing(a.clone()));
                tx.instant();
            }
        })
        .build(k)
}
fn external(s: &Viewer, label: &str, url: &str, k: &Kit) -> Option<El> {
    if !valid_url(url) {
        return None;
    }
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    let url = url.to_owned();
    Some(
        Button::new(label)
            .variant(ButtonVariant::Ghost)
            .on_press_tx(move |s: &mut Viewer, tx| {
                let now = s.composer_scope();
                if now.connection == owner.connection
                    && now.profile == owner.profile
                    && s.settings.scope_profile == scope
                    && tx.open_url(&url).is_err()
                {
                    s.settings.billing.error = Some("The billing page could not be opened".into());
                }
            })
            .build(k),
    )
}
fn input(
    s: &Viewer,
    label: &str,
    value: &TextEdit,
    access: impl Fn(&mut Billing) -> &mut TextEdit + 'static,
    k: &Kit,
) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    col()
        .grow(1.0)
        .min_w(120.0)
        .gap(4.0)
        .child(text(label).font(k.ty.small).color(k.c.text_muted))
        .child(
            TextInput::new(value)
                .width(None)
                .disabled(s.settings.billing.blocked() || s.settings.demo)
                .on_edit(move |s: &mut Viewer| {
                    let now = s.composer_scope();
                    if now.connection != owner.connection
                        || now.profile != owner.profile
                        || s.settings.scope_profile != scope
                    {
                        return s.settings.edit_mut("");
                    }
                    access(&mut s.settings.billing)
                })
                .build(k),
        )
}
fn summary(label: &str, value: &str, k: &Kit) -> El {
    row()
        .w_full()
        .gap(14.0)
        .py(8.0)
        .child(text(label).font(k.ty.ui).color(k.c.text_muted).grow(1.0))
        .child(text(value).font(k.ty.ui_medium).color(k.c.text_base).wrap())
}
pub(super) fn rows(s: &Viewer, k: &Kit) -> Vec<El> {
    let b = &s.settings.billing;
    let state = &b.state;
    let sub = &b.subscription;
    let blocked = b.blocked();
    let mut out = vec![
        row()
            .w_full()
            .center_y()
            .gap(8.0)
            .child(
                text("Billing & subscription")
                    .font(k.ty.body_medium)
                    .color(k.c.text_base)
                    .grow(1.0),
            )
            .child(action(
                s,
                "Refresh billing",
                BillingAction::Load,
                blocked || b.loading,
                k,
            )),
    ];
    if let Some(e) = &b.error {
        out.push(
            text(e)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if let Some(n) = &b.notice {
        out.push(text(n).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if b.stepping_up {
        out.push(text("Approve Remote Spending in your browser. Other billing actions wait for this approval.").font(k.ty.ui).color(k.c.text_muted).wrap());
        if !b.verification_code.is_empty() {
            out.push(
                text(&b.verification_code)
                    .font(k.ty.body_medium)
                    .color(k.c.text_base)
                    .selectable(true),
            );
        }
        if let Some(link) = external(s, "Open spending approval", &b.verification_url, k) {
            out.push(link);
        }
        out.push(action(
            s,
            "Stop waiting for approval",
            BillingAction::CancelStepUp,
            false,
            k,
        ));
    }
    if b.recovery == "step_up" && !b.stepping_up {
        out.push(action(
            s,
            "Approve Remote Spending",
            BillingAction::StepUp,
            blocked,
            k,
        ));
    }
    if !b.portal.is_empty() {
        if let Some(link) = external(
            s,
            if matches!(b.recovery.as_str(), "portal" | "login" | "reconnect") {
                "Resolve in billing portal"
            } else {
                "Open billing portal"
            },
            &b.portal,
            k,
        ) {
            out.push(link);
        }
    }
    if !state.is_object() {
        out.push(
            text(if b.loading {
                "Loading account and subscription…"
            } else {
                "Billing data is unavailable. Refresh to retry."
            })
            .font(k.ty.ui)
            .color(k.c.text_muted),
        );
        return out;
    }
    if state["free_tier_account"].as_bool() == Some(true) {
        out.push(
            text(format!(
                "Nous free tier{}",
                if string(state, "free_tier_model").is_empty() {
                    String::new()
                } else {
                    format!(" · {}", string(state, "free_tier_model"))
                }
            ))
            .font(k.ty.ui)
            .color(k.c.text_base),
        );
        out.push(
            Button::new("Sign in to a Nous account")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Viewer| s.settings_action(Action::Section("providers".into())))
                .build(k),
        );
        return out;
    }
    if state["logged_in"].as_bool() != Some(true) {
        out.push(
            text("Sign in to Nous in Providers & endpoints to see billing for the selected agent.")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .wrap(),
        );
        return out;
    }
    out.push(summary("Account", string(state, "org_name"), k));
    out.push(summary("Role", string(state, "role"), k));
    out.push(summary(
        "Available credits",
        string(state, "balance_display"),
        k,
    ));
    let payment = &state["payment_method"];
    let payment_label = match string(payment, "kind") {
        "card" => format!(
            "{} ••{}{}",
            string(payment, "brand"),
            string(payment, "last4"),
            payment["wallet"]
                .as_str()
                .map(|w| format!(" · {w}"))
                .unwrap_or_default()
        ),
        "link" => format!("Link {}", string(payment, "email")),
        "unknown" => "Saved payment method".into(),
        _ => state["card"]["display"]
            .as_str()
            .or_else(|| state["card"]["masked"].as_str())
            .unwrap_or("No payment method reported")
            .into(),
    };
    out.push(summary("Payment method", &payment_label, k));
    if state["monthly_cap"].is_object() {
        out.push(summary(
            "Monthly spending cap",
            &format!(
                "{} · spent {}",
                string(&state["monthly_cap"], "limit_display"),
                string(&state["monthly_cap"], "spent_display")
            ),
            k,
        ));
    }
    let usage = &state["usage"];
    if usage["available"].as_bool() == Some(true) {
        out.push(
            text("Credit usage")
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .pt(16.0),
        );
        for key in ["plan_bar", "topup_bar"] {
            let bar = &usage[key];
            if !bar.is_object() {
                continue;
            }
            out.push(
                text(format!(
                    "{} · {} remaining of {}",
                    if key == "plan_bar" { "Plan" } else { "Top-up" },
                    string(bar, "remaining_display"),
                    string(bar, "total_display")
                ))
                .font(k.ty.small)
                .color(k.c.text_muted),
            );
            out.push(Progress::new(bar["fill_fraction"].as_f64().map(|v| v as f32)).build(k));
        }
    }
    for gauge in state["plan_usage"]["gauges"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let label = gauge["title"]
            .as_str()
            .or_else(|| gauge["label"].as_str())
            .unwrap_or("Allowance");
        out.push(
            text(format!("{label} · remaining"))
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
        out.push(
            Progress::new(gauge["left_fraction"].as_f64().map(|v| v as f32))
                .show_value()
                .build(k),
        );
    }
    if state["can_charge"].as_bool() == Some(true)
        && state["cli_billing_enabled"].as_bool() == Some(true)
    {
        out.push(
            text("Buy credits")
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .pt(18.0),
        );
        let mut presets = row().w_full().gap(6.0);
        for preset in state["charge_presets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            presets = presets.child(action(
                s,
                &format!("${preset}"),
                BillingAction::Amount(preset.into()),
                blocked,
                k,
            ));
        }
        out.push(presets);
        out.push(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .child(input(s, "Amount in USD", &b.amount, |b| &mut b.amount, k))
                .child(action(s, "Buy credits", BillingAction::Charge, blocked, k)),
        );
        if !b.charge_status.is_empty() {
            out.push(
                text(format!("Payment: {}", b.charge_status))
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            );
        }
    }
    out.push(
        text("Automatic credit reload")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(18.0),
    );
    if b.auto_editable() {
        out.push(
            text(format!(
                "Current: charges {} below {}",
                string(&state["auto_reload"], "reload_to_display"),
                string(&state["auto_reload"], "threshold_display")
            ))
            .font(k.ty.small)
            .color(k.c.text_muted)
            .wrap(),
        );
        out.push(
            row()
                .w_full()
                .gap(10.0)
                .child(input(
                    s,
                    "Balance threshold (USD)",
                    &b.threshold,
                    |b| &mut b.threshold,
                    k,
                ))
                .child(input(
                    s,
                    "Reload amount (USD)",
                    &b.reload_to,
                    |b| &mut b.reload_to,
                    k,
                )),
        );
        out.push(
            row()
                .gap(8.0)
                .child(action(
                    s,
                    "Save automatic reload",
                    BillingAction::AutoSave,
                    blocked || !b.auto_dirty(),
                    k,
                ))
                .child(action(
                    s,
                    "Revert reload changes",
                    BillingAction::AutoRevert,
                    blocked || !b.auto_dirty(),
                    k,
                ))
                .child(action(
                    s,
                    "Turn off automatic reload",
                    BillingAction::AutoEnabled(false),
                    blocked,
                    k,
                )),
        );
        if b.confirm_disable {
            out.push(
                col()
                    .w_full()
                    .gap(8.0)
                    .py(10.0)
                    .child(
                        text("Turn off automatic credit reload?")
                            .font(k.ty.ui)
                            .color(k.c.text_base),
                    )
                    .child(
                        row()
                            .gap(8.0)
                            .child(action(
                                s,
                                "Turn off",
                                BillingAction::ConfirmDisable,
                                blocked,
                                k,
                            ))
                            .child(action(
                                s,
                                "Keep enabled",
                                BillingAction::CancelDisable,
                                blocked,
                                k,
                            )),
                    ),
            );
        }
    } else {
        out.push(
            text(if state["auto_reload"]["enabled"].as_bool() == Some(true) {
                "Automatic reload uses a separate payment method. Manage it in the billing portal."
            } else {
                "Automatic reload is off. Enable it in the billing portal."
            })
            .font(k.ty.small)
            .color(k.c.text_muted)
            .wrap(),
        );
    }
    if let Some(enabled) = state["keep_going_on_credits"].as_bool() {
        let current = b.pending_keep.unwrap_or(enabled);
        let owner = s.composer_scope();
        let scope = s.settings.scope_profile.clone();
        out.push(
            row()
                .gap(10.0)
                .center_y()
                .py(12.0)
                .child(
                    text("Keep going on credits when plan allowance runs out")
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .grow(1.0)
                        .wrap(),
                )
                .child(
                    Switch::new(current)
                        .disabled(blocked || s.settings.demo)
                        .on_toggle(move |s: &mut Viewer| {
                            let now = s.composer_scope();
                            if now.connection == owner.connection
                                && now.profile == owner.profile
                                && s.settings.scope_profile == scope
                            {
                                s.settings_action(Action::Billing(BillingAction::KeepGoing(
                                    !current,
                                )));
                            }
                        })
                        .build(k),
                ),
        );
    }
    if sub.is_object() {
        out.push(
            text("Subscription")
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .pt(18.0),
        );
        let current = &sub["current"];
        out.push(summary(
            "Current plan",
            current["tier_name"].as_str().unwrap_or("Free tier"),
            k,
        ));
        if !string(current, "cycle_ends_at").is_empty() {
            out.push(summary("Cycle ends", string(current, "cycle_ends_at"), k));
        }
        if current["cancel_at_period_end"].as_bool() == Some(true)
            || current["pending_downgrade_tier_name"].is_string()
        {
            out.push(summary(
                "Scheduled change",
                current["pending_downgrade_tier_name"]
                    .as_str()
                    .or_else(|| current["cancellation_effective_display"].as_str())
                    .unwrap_or("Cancellation at period end"),
                k,
            ));
            out.push(action(
                s,
                "Undo scheduled change",
                BillingAction::Resume,
                blocked,
                k,
            ));
        }
        if sub["can_change_plan"].as_bool() == Some(true) && sub["context"] != "team" {
            for tier in sub["tiers"].as_array().into_iter().flatten() {
                let id = string(tier, "tier_id");
                let current = tier["is_current"].as_bool() == Some(true);
                out.push(
                    row()
                        .w_full()
                        .gap(10.0)
                        .center_y()
                        .py(9.0)
                        .child(
                            col()
                                .grow(1.0)
                                .gap(4.0)
                                .child(
                                    text(string(tier, "name"))
                                        .font(k.ty.ui_medium)
                                        .color(k.c.text_base),
                                )
                                .child(
                                    text(format!(
                                        "{} per month{}",
                                        string(tier, "dollars_per_month_display"),
                                        tier["monthly_credits"]
                                            .as_str()
                                            .map(|s| format!(" · ${s} credits"))
                                            .unwrap_or_default()
                                    ))
                                    .font(k.ty.small)
                                    .color(k.c.text_muted),
                                ),
                        )
                        .child(action(
                            s,
                            if current {
                                "Current plan"
                            } else {
                                "Preview plan change"
                            },
                            BillingAction::Preview(id.into()),
                            blocked || current || tier["is_enabled"].as_bool() != Some(true),
                            k,
                        )),
                );
            }
        }
    }
    if let Some(p) = &b.preview {
        let effect = string(p, "effect");
        let mut quote = col()
            .w_full()
            .gap(8.0)
            .px(12.0)
            .py(12.0)
            .fill(k.c.layer_01)
            .child(
                text(format!(
                    "{} · {}",
                    string(p, "target_tier_name"),
                    match effect {
                        "scheduled" => "Changes at period end",
                        "charge_now" => "Paid upgrade",
                        "no_op" => "Already active",
                        _ => "Unavailable",
                    }
                ))
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
            )
            .children((!string(p, "reason").is_empty()).then(|| {
                text(string(p, "reason"))
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap()
            }))
            .children((!string(p, "effective_at").is_empty()).then(|| {
                text(format!("Effective: {}", string(p, "effective_at")))
                    .font(k.ty.small)
                    .color(k.c.text_muted)
            }));
        if effect == "scheduled" {
            quote = quote.child(action(
                s,
                "Confirm scheduled change",
                BillingAction::ConfirmPlan,
                blocked,
                k,
            ));
        } else if effect == "charge_now" {
            if let Some(portal) = external(s, "Complete upgrade in billing portal", &b.portal, k) {
                quote = quote.child(portal);
            }
        }
        out.push(quote.child(action(
            s,
            "Close quote",
            BillingAction::CancelPreview,
            blocked,
            k,
        )));
    }
    out
}
