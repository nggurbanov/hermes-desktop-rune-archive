use crate::{
    app::Viewer,
    pages::settings::{
        Action,
        models::{Action as ModelAction, Models},
    },
};
use rune::{El, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, TextInput};
fn action(s: &Viewer, label: &str, a: ModelAction, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.settings.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection != owner.connection
                || now.profile != owner.profile
                || s.settings.scope_profile != scope
            {
                return;
            }
            s.settings_action(Action::Model(a.clone()));
            tx.instant();
        })
        .build(k)
}
fn input(
    s: &Viewer,
    label: &str,
    value: &TextEdit,
    access: impl Fn(&mut Models) -> &mut TextEdit + 'static,
    main: bool,
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
                .disabled(s.settings.models.busy || s.settings.demo)
                .on_edit(move |s: &mut Viewer| {
                    let now = s.composer_scope();
                    if now.connection != owner.connection
                        || now.profile != owner.profile
                        || s.settings.scope_profile != scope
                    {
                        return s.settings.edit_mut("");
                    }
                    access(&mut s.settings.models)
                })
                .on_change(move |s: &mut Viewer, _| {
                    s.settings_action(Action::Model(if main {
                        ModelAction::Edited
                    } else {
                        ModelAction::TaskEdited
                    }))
                })
                .build(k),
        )
}
pub(super) fn rows(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let m = &s.settings.models;
    let mut rows = vec![
        row()
            .w_full()
            .center_y()
            .gap(12.0)
            .child(
                text("Main model")
                    .font(k.ty.body_medium)
                    .color(k.c.text_base),
            )
            .child(
                text(&m.current)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap()
                    .grow(1.0),
            )
            .child(action(
                s,
                "Refresh model catalog",
                ModelAction::Load,
                m.loading || m.busy,
                k,
            )),
    ];
    if let Some(error) = &m.error {
        rows.push(
            text(error)
                .role("alert")
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(notice) = &m.notice {
        rows.push(text(notice).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if m.confirm.is_some() {
        rows.push(
            col()
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(12.0)
                .fill(k.c.layer_01)
                .child(
                    text(&m.confirm_message)
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .wrap(),
                )
                .child(
                    row()
                        .gap(8.0)
                        .child(action(
                            s,
                            "Confirm model selection",
                            ModelAction::Confirm,
                            m.busy,
                            k,
                        ))
                        .child(action(s, "Cancel", ModelAction::Cancel, m.busy, k)),
                ),
        );
    }
    if m.dirty() {
        rows.push(action(
            s,
            "Revert model edits",
            ModelAction::Revert,
            m.busy,
            k,
        ));
    }
    let options = m.providers.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    let provider = Select::new("settings-main-provider", &options)
        .current(Some(&m.provider.text))
        .width(220.0)
        .open(m.open.as_deref() == Some("provider"))
        .disabled(m.busy || s.settings.demo)
        .on_toggle(|s: &mut Viewer| {
            s.settings.models.open = if s.settings.models.open.as_deref() == Some("provider") {
                None
            } else {
                Some("provider".into())
            }
        })
        .on_close(|s: &mut Viewer| s.settings.models.open = None)
        .on_choose(move |s: &mut Viewer, value, tx| {
            let now = s.composer_scope();
            if now.connection != owner.connection
                || now.profile != owner.profile
                || s.settings.scope_profile != scope
            {
                return;
            }
            s.settings_action(Action::Model(ModelAction::Provider(value.into())));
            tx.instant();
        })
        .build(k, layer);
    rows.push(
        row()
            .w_full()
            .gap(10.0)
            .center_y()
            .py(10.0)
            .child(
                col()
                    .gap(4.0)
                    .child(text("Provider").font(k.ty.small).color(k.c.text_muted))
                    .child(provider),
            )
            .child(input(s, "Model", &m.model, |m| &mut m.model, true, k))
            .child(action(s, "Apply main model", ModelAction::Main, m.busy, k)),
    );
    if let Some(provider) = m.providers.iter().find(|p| p.id == m.provider.text) {
        rows.push(
            text(format!(
                "{}{}",
                provider.label,
                if provider.available {
                    ""
                } else {
                    " · Connect this provider in Providers & endpoints first"
                }
            ))
            .font(k.ty.small)
            .color(k.c.text_muted)
            .wrap(),
        );
        if !provider.models.is_empty() {
            let owner = s.composer_scope();
            let scope = s.settings.scope_profile.clone();
            rows.push(
                Select::new("settings-main-model-catalog", &provider.models)
                    .current(Some(&m.model.text))
                    .width(360.0)
                    .open(m.open.as_deref() == Some("model"))
                    .disabled(m.busy || s.settings.demo)
                    .on_toggle(|s: &mut Viewer| {
                        s.settings.models.open =
                            if s.settings.models.open.as_deref() == Some("model") {
                                None
                            } else {
                                Some("model".into())
                            }
                    })
                    .on_close(|s: &mut Viewer| s.settings.models.open = None)
                    .on_choose(move |s: &mut Viewer, value, tx| {
                        let now = s.composer_scope();
                        if now.connection != owner.connection
                            || now.profile != owner.profile
                            || s.settings.scope_profile != scope
                        {
                            return;
                        }
                        s.settings_action(Action::Model(ModelAction::MainModel(value.into())));
                        tx.instant();
                    })
                    .build(k, layer),
            );
        }
    }
    rows.push(input(
        s,
        "Endpoint URL (optional)",
        &m.url,
        |m| &mut m.url,
        true,
        k,
    ));
    rows.push(
        text("Auxiliary models")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(18.0),
    );
    rows.push(text("Task routes are independent of the main model. Leave the model empty and use auto to inherit. Reasoning left blank inherits the main setting.").font(k.ty.small).color(k.c.text_muted).wrap());
    for task in &m.tasks {
        let id = task.id.clone();
        let a = id.clone();
        let b = id.clone();
        let c = id.clone();
        let d = id.clone();
        rows.push(
            col()
                .w_full()
                .gap(8.0)
                .py(14.0)
                .child(text(&task.label).font(k.ty.ui_medium).color(k.c.text_base))
                .children((!task.hint.is_empty()).then(|| {
                    text(&task.hint)
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .wrap()
                }))
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .child(input(
                            s,
                            "Provider",
                            &task.provider,
                            move |m| m.task_edit(&a, "provider"),
                            false,
                            k,
                        ))
                        .child(input(
                            s,
                            "Model",
                            &task.model,
                            move |m| m.task_edit(&b, "model"),
                            false,
                            k,
                        ))
                        .child(input(
                            s,
                            "Reasoning effort",
                            &task.effort,
                            move |m| m.task_edit(&c, "effort"),
                            false,
                            k,
                        )),
                )
                .child(input(
                    s,
                    "Endpoint URL (optional)",
                    &task.url,
                    move |m| m.task_edit(&d, "url"),
                    false,
                    k,
                ))
                .child(
                    row()
                        .gap(8.0)
                        .child(action(
                            s,
                            "Apply task route",
                            ModelAction::Task(id.clone(), false),
                            m.busy,
                            k,
                        ))
                        .child(action(
                            s,
                            "Use main model",
                            ModelAction::Task(id, true),
                            m.busy,
                            k,
                        )),
                ),
        );
    }
    rows
}
