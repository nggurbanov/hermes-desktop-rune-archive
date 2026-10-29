use crate::{
    app::Viewer,
    pages::settings::{
        Action,
        moa::{Action as MoaAction, Moa, Slot},
    },
};
use rune::{El, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextInput};
fn action(s: &Viewer, label: &str, a: MoaAction, disabled: bool, k: &Kit) -> El {
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
                s.settings_action(Action::Moa(a.clone()));
                tx.instant();
            }
        })
        .build(k)
}
fn input(
    s: &Viewer,
    label: &str,
    value: &TextEdit,
    access: impl Fn(&mut Moa) -> &mut TextEdit + 'static,
    k: &Kit,
) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    col()
        .grow(1.0)
        .min_w(110.0)
        .gap(4.0)
        .child(text(label).font(k.ty.small).color(k.c.text_muted))
        .child(
            TextInput::new(value)
                .width(None)
                .disabled(s.settings.moa.saving || s.settings.demo)
                .on_edit(move |s: &mut Viewer| {
                    let now = s.composer_scope();
                    if now.connection != owner.connection
                        || now.profile != owner.profile
                        || s.settings.scope_profile != scope
                    {
                        return s.settings.edit_mut("");
                    }
                    access(&mut s.settings.moa)
                })
                .on_change(|s: &mut Viewer, _| s.settings_action(Action::Moa(MoaAction::Edited)))
                .build(k),
        )
}
fn slot(s: &Viewer, title: &str, v: &Slot, index: Option<usize>, k: &Kit) -> El {
    let mut r = col()
        .w_full()
        .gap(7.0)
        .py(8.0)
        .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(input(
                    s,
                    "Provider",
                    &v.provider,
                    move |m| m.edit(index, "provider"),
                    k,
                ))
                .child(input(
                    s,
                    "Model",
                    &v.model,
                    move |m| m.edit(index, "model"),
                    k,
                )),
        );
    if let Some(i) = index {
        let owner = s.composer_scope();
        let scope = s.settings.scope_profile.clone();
        let next = !v.enabled;
        r = r.child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    Switch::new(v.enabled)
                        .disabled(s.settings.moa.saving || s.settings.demo)
                        .on_toggle(move |s: &mut Viewer| {
                            let now = s.composer_scope();
                            if now.connection == owner.connection
                                && now.profile == owner.profile
                                && s.settings.scope_profile == scope
                            {
                                s.settings_action(Action::Moa(MoaAction::ReferenceEnabled(
                                    i, next,
                                )));
                            }
                        })
                        .build(k),
                )
                .child(
                    text("Include reference")
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                )
                .child(action(
                    s,
                    "Remove",
                    MoaAction::RemoveReference(i),
                    s.settings.moa.saving || s.settings.moa.references.len() <= 1,
                    k,
                )),
        );
    }
    r
}
pub(super) fn rows(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let m = &s.settings.moa;
    let mut out=vec![text("Mixture of Agents").font(k.ty.body_medium).color(k.c.text_base).pt(18.0),text("Reference models work in parallel; the aggregator combines their responses. Presets retain their own routing and advanced cadence settings.").font(k.ty.small).color(k.c.text_muted).wrap()];
    if let Some(e) = &m.error {
        out.push(
            text(e)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if let Some(n) = &m.notice {
        out.push(text(n).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if m.names.is_empty() {
        out.push(action(
            s,
            if m.loading {
                "Loading presets…"
            } else {
                "Load MoA presets"
            },
            MoaAction::Load,
            m.loading || m.saving,
            k,
        ));
        return out;
    }
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    let picker = Select::new("settings-moa-preset", &m.names)
        .current(Some(&m.selected))
        .width(240.0)
        .open(m.open)
        .disabled(m.saving || s.settings.demo)
        .on_toggle(|s: &mut Viewer| s.settings.moa.open = !s.settings.moa.open)
        .on_close(|s: &mut Viewer| s.settings.moa.open = false)
        .on_choose(move |s: &mut Viewer, value, tx| {
            let now = s.composer_scope();
            if now.connection == owner.connection
                && now.profile == owner.profile
                && s.settings.scope_profile == scope
            {
                s.settings_action(Action::Moa(MoaAction::Select(value.into())));
                tx.instant();
            }
        })
        .build(k, layer);
    out.push(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .py(8.0)
            .child(picker)
            .child(action(s, "Set default", MoaAction::Default, m.saving, k))
            .child(action(
                s,
                "Delete preset",
                MoaAction::Delete,
                m.saving || m.names.len() <= 1,
                k,
            )),
    );
    out.push(
        text(format!(
            "Default: {}{}",
            m.default,
            if m.active.is_empty() {
                String::new()
            } else {
                format!(" · Active: {}", m.active)
            }
        ))
        .font(k.ty.small)
        .color(k.c.text_muted),
    );
    if m.confirm_delete {
        out.push(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    text("Delete this preset?")
                        .font(k.ty.ui)
                        .color(k.c.text_base),
                )
                .child(action(s, "Delete", MoaAction::ConfirmDelete, m.saving, k))
                .child(action(s, "Cancel", MoaAction::CancelDelete, m.saving, k)),
        );
    }
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    let next = !m.enabled;
    out.push(
        row()
            .gap(8.0)
            .center_y()
            .child(
                Switch::new(m.enabled)
                    .disabled(m.saving || s.settings.demo)
                    .on_toggle(move |s: &mut Viewer| {
                        let now = s.composer_scope();
                        if now.connection == owner.connection
                            && now.profile == owner.profile
                            && s.settings.scope_profile == scope
                        {
                            s.settings_action(Action::Moa(MoaAction::Enabled(next)));
                        }
                    })
                    .build(k),
            )
            .child(
                text("Enable this preset")
                    .font(k.ty.ui)
                    .color(k.c.text_base),
            ),
    );
    for (i, v) in m.references.iter().enumerate() {
        out.push(slot(s, &format!("Reference {}", i + 1), v, Some(i), k));
    }
    out.push(action(
        s,
        "Add reference",
        MoaAction::AddReference,
        m.saving,
        k,
    ));
    out.push(slot(s, "Aggregator", &m.aggregator, None, k));
    out.push(
        row()
            .w_full()
            .gap(8.0)
            .child(input(
                s,
                "Reference temperature",
                &m.reference_temperature,
                |m| &mut m.reference_temperature,
                k,
            ))
            .child(input(
                s,
                "Aggregator temperature",
                &m.aggregator_temperature,
                |m| &mut m.aggregator_temperature,
                k,
            ))
            .child(input(
                s,
                "Timeout in seconds (blank: default)",
                &m.timeout,
                |m| &mut m.timeout,
                k,
            )),
    );
    out.push(
        row()
            .gap(8.0)
            .center_y()
            .child(
                text(format!("Degraded reference policy: {}", m.policy))
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            )
            .child(action(
                s,
                "Report missing references",
                MoaAction::Policy("loud".into()),
                m.saving,
                k,
            ))
            .child(action(
                s,
                "Continue silently",
                MoaAction::Policy("silent".into()),
                m.saving,
                k,
            )),
    );
    out.push(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(input(
                s,
                "New preset name",
                &m.new_name,
                |m| &mut m.new_name,
                k,
            ))
            .child(action(
                s,
                "Duplicate as new preset",
                MoaAction::AddPreset,
                m.saving || m.new_name.text.trim().is_empty(),
                k,
            )),
    );
    out.push(
        row()
            .gap(8.0)
            .child(action(
                s,
                if m.saving {
                    "Saving…"
                } else {
                    "Save MoA presets"
                },
                MoaAction::Save,
                m.saving || !m.dirty,
                k,
            ))
            .child(action(
                s,
                "Revert MoA changes",
                MoaAction::Revert,
                m.saving || !m.dirty,
                k,
            )),
    );
    out
}
