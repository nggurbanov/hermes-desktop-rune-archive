use crate::{app::Viewer, pages::settings::Action};
use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit};
fn action(s: &Viewer, label: &str, key: Option<String>, k: &Kit) -> El {
    let owner = s.composer_scope();
    let scope = s.settings.scope_profile.clone();
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(s.settings.plugins.loading || s.settings.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection == owner.connection
                && now.profile == owner.profile
                && s.settings.scope_profile == scope
            {
                s.settings_action(if let Some(key) = &key {
                    Action::PluginOpen(key.clone())
                } else {
                    Action::PluginLoad
                });
                tx.instant();
            }
        })
        .build(k)
}
pub(super) fn rows(s: &Viewer, k: &Kit) -> Vec<El> {
    let p = &s.settings.plugins;
    let mut out=vec![row().w_full().center_y().gap(10.0).child(text("Plugin settings").font(k.ty.body_medium).color(k.c.text_base).grow(1.0)).child(action(s,"Refresh",None,k)),text("Each agent plugin declares its own settings. Open a plugin to edit its native schema form for this profile; credentials are saved separately on the same agent.").font(k.ty.small).color(k.c.text_muted).wrap()];
    if let Some(e) = &p.error {
        out.push(
            text(e)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .role("alert")
                .wrap(),
        );
    }
    if p.loading {
        out.push(
            text("Loading plugin settings…")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    } else if p.rows.is_empty() {
        out.push(text("No installed agent plugins declare settings. Installed plugins can be managed in Capabilities.").font(k.ty.ui).color(k.c.text_muted).wrap().py(12.0));
    }
    for plugin in &p.rows {
        let query = s.settings.search.text.to_lowercase();
        if !query.is_empty()
            && !format!("{} {} {}", plugin.name, plugin.key, plugin.description)
                .to_lowercase()
                .contains(&query)
        {
            continue;
        }
        out.push(
            row()
                .w_full()
                .gap(14.0)
                .center_y()
                .py(13.0)
                .child(
                    col()
                        .grow(1.0)
                        .gap(5.0)
                        .child(
                            text(format!("{} {}", plugin.name, plugin.version))
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base),
                        )
                        .child(
                            text(&plugin.description)
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .wrap(),
                        )
                        .child(
                            text(format!("{} · {} settings", plugin.key, plugin.fields))
                                .font(k.ty.small)
                                .color(k.c.text_faint),
                        ),
                )
                .child(action(s, "Open settings", Some(plugin.key.clone()), k)),
        );
    }
    out
}
