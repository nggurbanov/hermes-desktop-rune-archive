//! Compact model summary and the single next-message settings popover.
use super::{Viewer, draft};
use crate::composer::{DraftScope, Popup};
use rune::{Color, El, Icon, ScrollSpec, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant, Kit, Switch};
use rune_theme_opencode::icons as ic;

static SLIDERS: Icon = Icon {
    name: "model-settings",
    view: 24.0,
    body: r#"<path d="M4 6h16M4 12h16M4 18h16M15 3v6M9 9v6M15 15v6" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#,
};
static LOCK: Icon = Icon {
    name: "manual-approval",
    view: 24.0,
    body: r#"<rect x="5" y="10" width="14" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M8 10V6a4 4 0 0 1 8 0v4" fill="none" stroke="currentColor" stroke-width="1.7"/>"#,
};
static SMART: Icon = Icon {
    name: "smart-approval",
    view: 24.0,
    body: r#"<path d="m12 3 2.3 6.7L21 12l-6.7 2.3L12 21l-2.3-6.7L3 12l6.7-2.3L12 3ZM20 2v4m-2-2h4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>"#,
};
static SHIELD_OFF: Icon = Icon {
    name: "full-access",
    view: 24.0,
    body: r#"<path d="m9 4 3-2 8 4v6c0 2-.6 4-1.7 5.5M15.8 20A17 17 0 0 1 12 22c-6-3-8-6-8-10V6l1.3-.7M3 2l19 20" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub(super) fn trigger(k: &Kit) -> El {
    IconButton::new(&SLIDERS)
        .variant(IconButtonVariant::Neutral)
        .label("Model settings")
        .help("Reasoning, service tier and permissions")
        .on_press(|s: &mut Viewer| s.open_composer(Popup::Settings))
        .build(k)
        .key("model-settings-trigger")
        .size(32.0, 32.0)
        .radius(16.0)
}

pub(super) fn model_trigger(s: &Viewer, k: &Kit) -> El {
    let scope = s.composer_scope();
    let current = draft(s);
    let label = s
        .composer
        .selected_model(&scope)
        .map(|(_, m)| m.label.as_str())
        .or_else(|| s.composer.model_name(&scope))
        .unwrap_or("Select model");
    let compact = label
        .strip_prefix("GPT-")
        .or_else(|| label.strip_prefix("gpt-"))
        .unwrap_or(label)
        .replace('-', " ");
    let summary = match current.reasoning.as_deref() {
        Some(value) => format!("{compact} {}", effort_label(value)),
        None => compact,
    };
    let mut help = format!("{label}\nChoose a model for the next message");
    if current.model_changed || current.session_policy.deferred.is_some() {
        help.push_str("\nThis selection is for the next message.");
        if let Some(reported) = &current.session_policy.model {
            help.push_str(&format!("\nAgent-reported model: {reported}"));
        }
    } else if !current.reasoning_changed {
        if let Some(wire) = &current.session_policy.reasoning_wire {
            help.push_str(&format!("\nResolved reasoning effort: {wire}"));
        }
    }
    Button::new(&summary)
        .variant(ButtonVariant::GhostMuted)
        .label_with(move |font, ink| {
            row()
                .min_w(0.0)
                .gap(6.0)
                .center_y()
                .child(icon(&ic::BRAIN, 18.0).color(ink))
                .child(text(&summary).font(font).color(ink).truncate().shrink())
        })
        .on_press(|s: &mut Viewer| s.open_composer(Popup::Models))
        .build(k)
        .key("model-picker-trigger")
        .min_w(0.0)
        .max_w(240.0)
        .shrink()
        .help(help)
}

fn effort_label(value: &str) -> &str {
    match value {
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        "ultra" => "Ultra",
        "none" => "Thinking off",
        _ => value,
    }
}
fn heading(label: &str, k: &Kit) -> El {
    text(label)
        .font(k.ty.small_medium)
        .color(k.c.text_muted)
        .px(8.0)
        .py(6.0)
}
fn divider(k: &Kit) -> El {
    el().w_full().h(1.0).my(6.0).fill(k.c.border_muted)
}
fn choice(label: &str, key: String, selected: bool, detail: Option<&str>, k: &Kit) -> El {
    let label = label.to_owned();
    let height = if detail.is_some() { 46.0 } else { 26.0 };
    let detail = detail.map(str::to_owned);
    let small = k.ty.small;
    let muted = k.c.text_muted;
    Button::new(&label)
        .variant(ButtonVariant::GhostMuted)
        .label_with(move |font, ink| {
            let mut content = col()
                .w_full()
                .gap(4.0)
                .child(text(&label).font(font).color(ink));
            if let Some(detail) = detail {
                content = content.child(text(detail).font(small).color(muted).wrap());
            }
            content
        })
        .build(k)
        .key(key)
        .min_h(height)
        .w_full()
        .justify_start()
        .role("option")
        .focus_selected(selected)
        .fill(if selected {
            k.c.bg_base
        } else {
            Color::TRANSPARENT
        })
}

pub(super) fn menu(s: &Viewer, scope: &DraftScope, k: &Kit) -> El {
    let current = draft(s);
    let caps = s
        .composer
        .selected_model(scope)
        .and_then(|(_, m)| m.capabilities.as_ref());
    let mut menu = col()
        .w(340.0)
        .padding(8.0)
        .gap(2.0)
        .min_h(0.0)
        .max_h(650.0)
        .shrink()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(heading("Reasoning", k));
    for value in [
        None,
        Some("minimal"),
        Some("low"),
        Some("medium"),
        Some("high"),
        Some("xhigh"),
        Some("max"),
        Some("ultra"),
        Some("none"),
    ] {
        if value.is_some()
            && !caps.is_some_and(|c| {
                c.reasoning == Some(true)
                    && (value != Some("none") || c.can_disable_reasoning == Some(true))
            })
        {
            continue;
        }
        let owner = scope.clone();
        menu = menu.child(
            choice(
                value.map(effort_label).unwrap_or("Agent default"),
                format!("effort-{}", value.unwrap_or("default")),
                current.reasoning.as_deref() == value,
                None,
                k,
            )
            .on_press(move |s: &mut Viewer| {
                s.composer
                    .choose_reasoning(owner.clone(), value.map(str::to_owned));
            }),
        );
    }
    menu = menu.child(divider(k)).child(heading("Service Tier", k));
    for (value, label, detail) in [
        (None, "Agent default", None),
        (Some("normal"), "Standard", None),
        (
            Some("priority"),
            "Fast",
            Some("Faster responses, increased usage"),
        ),
        (Some("ultrafast"), "Ultra Fast", None),
    ] {
        if !value.is_none_or(|v| {
            v == "normal"
                || caps.is_some_and(|c| {
                    if v == "priority" {
                        c.fast == Some(true)
                    } else {
                        c.ultrafast == Some(true)
                    }
                })
        }) {
            continue;
        }
        let owner = scope.clone();
        menu = menu.child(
            choice(
                label,
                format!("tier-{}", value.unwrap_or("default")),
                current.service_tier.as_deref() == value,
                detail,
                k,
            )
            .on_press(move |s: &mut Viewer| {
                s.composer
                    .choose_service_tier(owner.clone(), value.map(str::to_owned));
            }),
        );
    }
    menu.child(divider(k)).child(permissions(s, scope, k))
}

fn permissions(s: &Viewer, scope: &DraftScope, k: &Kit) -> El {
    let value = s
        .composer
        .draft(scope)
        .and_then(|d| d.approval_mode.as_deref());
    let mut segments = row()
        .key("permissions-selector")
        .w_full()
        .gap(2.0)
        .padding(3.0)
        .radius(9.0)
        .fill(k.c.layer_02);
    let mut selected = (
        "Agent default",
        "Use the connected agent’s approval policy.",
    );
    for (mode, label, detail, glyph) in [
        (
            None,
            "Agent default",
            "Use the connected agent’s approval policy.",
            &ic::STATUS,
        ),
        (
            Some("manual"),
            "Manual",
            "Use manual approval checks for this agent profile.",
            &LOCK,
        ),
        (
            Some("smart"),
            "Smart",
            "Let Hermes evaluate approvals for this agent profile.",
            &SMART,
        ),
        (
            Some("off"),
            "Off",
            "Disable approval checks for this agent profile.",
            &SHIELD_OFF,
        ),
    ] {
        if mode == value {
            selected = (label, detail);
        }
        let owner = scope.clone();
        segments = segments.child(
            IconButton::new(glyph)
                .variant(IconButtonVariant::GhostMuted)
                .label(label)
                .help(detail)
                .on_press(move |s: &mut Viewer| {
                    s.composer
                        .choose_approval(owner.clone(), mode.map(str::to_owned));
                })
                .build(k)
                .key(format!("approval-{}", mode.unwrap_or("default")))
                .grow(1.0)
                .focus_selected(mode == value)
                .fill(if mode == value {
                    k.c.bg_base
                } else {
                    Color::TRANSPARENT
                }),
        );
    }
    col()
        .w_full()
        .gap(8.0)
        .child(heading("Permissions", k))
        .child(
            text("Profile approval policy")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .px(8.0),
        )
        .child(segments)
        .child(
            text(selected.0)
                .key("selected-permission")
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .px(8.0),
        )
        .child(
            text(selected.1)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap()
                .px(8.0)
                .pb(8.0),
        )
        .child(divider(k))
        .child(conversation_bypass(s, scope, k))
}

fn conversation_bypass(s: &Viewer, scope: &DraftScope, k: &Kit) -> El {
    let reported = s
        .composer
        .draft(scope)
        .and_then(|draft| draft.session_policy.yolo);
    let owner = scope.clone();
    let enabled = s.can_set_conversation_bypass(scope);
    col()
        .w_full()
        .gap(6.0)
        .px(8.0)
        .pb(8.0)
        .child(
            Switch::new(reported.unwrap_or(false))
                .label("Bypass approvals for this conversation")
                .disabled(reported.is_none() || !enabled)
                .on_toggle(move |s: &mut Viewer| {
                    s.set_conversation_bypass(owner.clone(), !reported.unwrap_or(false))
                })
                .build(k)
                .key("conversation-approval-bypass"),
        )
        .child(
            text(if reported.is_none() {
                "Bypass status is unavailable until this conversation is attached."
            } else {
                "Separate from the profile policy. The agent reports the effective bypass state."
            })
            .font(k.ty.small)
            .color(k.c.text_muted)
            .wrap(),
        )
}
