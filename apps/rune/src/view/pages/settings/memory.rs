use crate::{
    app::Viewer,
    pages::settings::{
        Action,
        memory::{Action as MemoryAction, Field},
    },
};
use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch, TextArea, TextInput};
fn action(s: &Viewer, label: &str, a: MemoryAction, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    let provider = s.settings.memory.provider.clone();
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.settings.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection == owner.connection
                && now.profile == owner.profile
                && s.settings.scope_profile == scope
                && s.settings.memory.provider == provider
            {
                s.settings_action(Action::Memory(a.clone()));
                tx.instant();
            }
        })
        .build(k)
}
fn field(s: &Viewer, f: &Field, k: &Kit, layer: &mut Vec<El>) -> El {
    let m = &s.settings.memory;
    let key = f.key.clone();
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    let provider = m.provider.clone();
    let mut line = col()
        .w_full()
        .gap(6.0)
        .py(9.0)
        .child(
            row()
                .gap(8.0)
                .child(text(&f.label).font(k.ty.ui_medium).color(k.c.text_base))
                .children((f.kind == "secret").then(|| {
                    text(if f.is_set {
                        "Saved on agent"
                    } else {
                        "Not set"
                    })
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                })),
        )
        .children((!f.description.is_empty()).then(|| {
            text(&f.description)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap()
        }));
    if f.kind == "bool" {
        let next = f.editor.text != "true";
        return line.child(
            Switch::new(!next)
                .disabled(m.saving || s.settings.demo)
                .on_toggle(move |s: &mut Viewer| {
                    let now = s.composer_scope();
                    if now.connection == owner.connection
                        && now.profile == owner.profile
                        && s.settings.scope_profile == scope
                        && s.settings.memory.provider == provider
                    {
                        s.settings_action(Action::Memory(MemoryAction::Value(
                            key.clone(),
                            next.to_string(),
                        )));
                    }
                })
                .build(k),
        );
    }
    if f.kind == "select" && !f.options.is_empty() {
        let close_key = key.clone();
        let toggle_key = key.clone();
        return line.child(
            Select::new(format!("memory-provider-{}", f.key), &f.options)
                .current(Some(&f.editor.text))
                .width(350.0)
                .disabled(m.saving || s.settings.demo)
                .open(m.open.as_deref() == Some(&f.key))
                .on_toggle(move |s: &mut Viewer| {
                    s.settings.memory.open = if s.settings.memory.open.as_ref() == Some(&toggle_key)
                    {
                        None
                    } else {
                        Some(toggle_key.clone())
                    }
                })
                .on_close(move |s: &mut Viewer| {
                    if s.settings.memory.open.as_ref() == Some(&close_key) {
                        s.settings.memory.open = None;
                    }
                })
                .on_choose(move |s: &mut Viewer, value, tx| {
                    let now = s.composer_scope();
                    if now.connection == owner.connection
                        && now.profile == owner.profile
                        && s.settings.scope_profile == scope
                        && s.settings.memory.provider == provider
                    {
                        s.settings_action(Action::Memory(MemoryAction::Value(
                            key.clone(),
                            value.into(),
                        )));
                        s.settings.memory.open = None;
                        tx.instant();
                    }
                })
                .build(k, layer),
        );
    }
    if f.kind == "json" {
        return line.child(
            TextArea::new(&f.editor)
                .width(None)
                .rows(5)
                .max_height(150.0)
                .disabled(m.saving || s.settings.demo)
                .on_edit(move |s: &mut Viewer| {
                    let now = s.composer_scope();
                    if now.connection != owner.connection
                        || now.profile != owner.profile
                        || s.settings.scope_profile != scope
                        || s.settings.memory.provider != provider
                    {
                        return s.settings.edit_mut("");
                    }
                    s.settings.memory.edit(&key)
                })
                .build(k),
        );
    }
    let mut input = TextInput::new(&f.editor)
        .placeholder(if f.kind == "secret" && f.is_set {
            "Leave blank to keep saved value"
        } else {
            &f.placeholder
        })
        .width(None)
        .disabled(m.saving || s.settings.demo)
        .on_edit(move |s: &mut Viewer| {
            let now = s.composer_scope();
            if now.connection != owner.connection
                || now.profile != owner.profile
                || s.settings.scope_profile != scope
                || s.settings.memory.provider != provider
            {
                return s.settings.edit_mut("");
            }
            s.settings.memory.edit(&key)
        });
    if f.kind == "secret" {
        input = input.secure();
    }
    line = line.child(input.build(k));
    line
}
pub(super) fn rows(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> Vec<El> {
    let m = &s.settings.memory;
    let mut out=vec![row().w_full().gap(10.0).center_y().child(text("Memory layers").font(k.ty.body_medium).color(k.c.text_base)).child(action(s,"Refresh memory status",MemoryAction::Load,m.loading,k)),text(format!("Saved external provider: {} · Built-in MEMORY.md: {} bytes · USER.md: {} bytes",if m.active.is_empty(){"None"}else{&m.active},m.memory_bytes,m.user_bytes)).font(k.ty.small).color(k.c.text_muted).wrap(),text("Built-in memory and the user profile stay separate from external recall. The saved provider only supplies recall when the agent reports it ready.").font(k.ty.small).color(k.c.text_muted).wrap()];
    for p in &m.providers {
        out.push(
            row()
                .w_full()
                .gap(12.0)
                .py(5.0)
                .child(
                    col()
                        .grow(1.0)
                        .gap(3.0)
                        .child(
                            text(format!("{} · {}", p.name, p.status))
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base),
                        )
                        .child(
                            text(&p.description)
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .wrap(),
                        ),
                )
                .child(action(
                    s,
                    "Configure",
                    MemoryAction::Select(p.name.clone()),
                    m.saving || m.pending || m.dirty(),
                    k,
                )),
        );
    }
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
    if !m.provider.is_empty() {
        out.push(
            text(format!(
                "{} settings",
                if m.label.is_empty() {
                    &m.provider
                } else {
                    &m.label
                }
            ))
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(16.0),
        );
        if m.loading && m.fields.is_empty() {
            out.push(
                text("Loading declared provider settings…")
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            );
        }
        if m.oauth_capable {
            out.push(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(
                        text(if m.pending {
                            "Waiting for consent on the agent…"
                        } else if m.connected {
                            if m.auth == "apikey" {
                                "API key connected"
                            } else {
                                "OAuth connected"
                            }
                        } else {
                            "Not connected"
                        })
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                    )
                    .child(action(
                        s,
                        if m.pending {
                            "Stop waiting"
                        } else if m.connected {
                            "Reconnect"
                        } else {
                            "Connect on agent"
                        },
                        if m.pending {
                            MemoryAction::Cancel
                        } else {
                            MemoryAction::Connect
                        },
                        m.saving,
                        k,
                    )),
            );
        }
        if !m.detail.is_empty() {
            out.push(
                text(&m.detail)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap(),
            );
        }
        let mut group = String::new();
        for f in &m.fields {
            if !f.group.is_empty() && f.group != group {
                group = f.group.clone();
                out.push(
                    text(&group)
                        .font(k.ty.small_medium)
                        .color(k.c.text_muted)
                        .pt(10.0),
                );
            }
            out.push(field(s, f, k, layer));
        }
        if !m.fields.is_empty() {
            out.push(
                row()
                    .gap(8.0)
                    .child(action(
                        s,
                        if m.saving {
                            "Saving…"
                        } else {
                            "Save provider settings"
                        },
                        MemoryAction::Save,
                        m.saving || !m.dirty(),
                        k,
                    ))
                    .child(action(
                        s,
                        "Revert provider changes",
                        MemoryAction::Revert,
                        m.saving || !m.dirty(),
                        k,
                    )),
            );
        }
        if let Ok(url) = url::Url::parse(&m.docs_url) {
            if matches!(url.scheme(), "https" | "http")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
            {
                let url = m.docs_url.clone();
                out.push(
                    Button::new("Provider documentation")
                        .variant(ButtonVariant::Ghost)
                        .on_press_tx(move |s: &mut Viewer, tx| {
                            if tx.open_url(&url).is_err() {
                                s.settings.memory.error =
                                    Some("The provider documentation could not be opened".into());
                            }
                        })
                        .build(k),
                );
            }
        }
    }
    out.push(
        text("Memory & context defaults")
            .font(k.ty.body_medium)
            .color(k.c.text_base)
            .pt(18.0),
    );
    out
}
