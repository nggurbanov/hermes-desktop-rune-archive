//! Preferences owned by the standalone client, independent of agent profiles.
use crate::app::Viewer;
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Switch};
fn choice(label: &str, selected: bool, f: impl Fn(&mut Viewer) + 'static, k: &Kit) -> El {
    Button::new(label)
        .variant(if selected {
            ButtonVariant::Neutral
        } else {
            ButtonVariant::Ghost
        })
        .on_press(f)
        .build(k)
}
pub fn page(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    if s.settings.section == "connections" {
        let tabs = row()
            .w_full()
            .gap(6.)
            .px(20.)
            .pt(16.)
            .child(
                choice(
                    "Saved connections",
                    !s.legacy_routing_settings,
                    |s| s.set_legacy_routing_settings(false),
                    k,
                )
                .key("connections-saved-tab"),
            )
            .child(
                choice(
                    "Profile routing",
                    s.legacy_routing_settings,
                    |s| s.set_legacy_routing_settings(true),
                    k,
                )
                .key("connections-routing-tab"),
            );
        let panel = if s.legacy_routing_settings {
            crate::view::legacy_settings::panel(s, k, layer)
        } else {
            crate::view::connections::panel(s, k, layer)
        };
        return col()
            .key("connection-settings")
            .w_full()
            .h_full()
            .min_h(0.)
            .gap(10.)
            .child(tabs)
            .child(panel.grow(1.).min_h(0.));
    }
    let mut body = col()
        .key("client-preferences")
        .w_full()
        .h_full()
        .gap(20.0)
        .padding(28.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    match s.settings.section.as_str() {
        "appearance" => {
            body = body
                .child(
                    text("Appearance")
                        .font(k.ty.body_medium)
                        .color(k.c.text_base),
                )
                .child(
                    text("Appearance controls apply to this device. Font assignments below use the connected agent profile.")
                        .font(k.ty.ui)
                        .color(k.c.text_muted)
                        .wrap(),
                )
                .child(
                    col()
                        .gap(10.0)
                        .child(text("Theme").font(k.ty.ui_medium).color(k.c.text_base))
                        .child(
                            row()
                                .gap(8.0)
                                .child(choice("System", s.theme.is_none(), |s| s.theme = None, k))
                                .child(choice(
                                    "Light",
                                    s.theme == Some(false),
                                    |s| s.theme = Some(false),
                                    k,
                                ))
                                .child(choice(
                                    "Dark",
                                    s.theme == Some(true),
                                    |s| s.theme = Some(true),
                                    k,
                                )),
                        ),
                )
                .child(
                    Switch::new(s.simple_mode)
                        .label("Simple mode")
                        .on_toggle(|s: &mut Viewer| s.set_simple_mode(!s.simple_mode))
                        .build(k),
                )
                .child(
                    Switch::new(s.show_statusbar)
                        .label("Show statusbar")
                        .on_toggle(|s: &mut Viewer| s.show_statusbar = !s.show_statusbar)
                        .build(k),
                )
                .child(
                    Switch::new(s.show_profile_rail)
                        .label("Show profile rail")
                        .on_toggle(|s: &mut Viewer| s.show_profile_rail = !s.show_profile_rail)
                        .build(k),
                )
                .child(
                    Switch::new(s.sidebar.is_open())
                        .label("Show sidebar")
                        .on_toggle(|s: &mut Viewer| s.toggle_sidebar())
                        .build(k),
                )
                .child(
                    Switch::new(s.inspector.is_open())
                        .label("Show tools panel")
                        .on_toggle(|s: &mut Viewer| s.toggle_all_tools())
                        .build(k),
                );
            body = body.children(crate::view::client_settings::rows(s, k, layer));
        }
        "shortcuts" => return crate::view::client_settings::shortcuts(s, k),
        _ => {
            body=body.child(text("Hermes").font(k.ty.body_medium).color(k.c.text_base))
    .child(text(format!("Standalone desktop · {}",env!("CARGO_PKG_VERSION"))).font(k.ty.ui).color(k.c.text_muted))
    .child(text("Built with Rune and OpenCode UI components. Hermes connects to independently managed local or remote agents.").font(k.ty.ui).color(k.c.text_base).wrap())
    .child(text(format!("Connection: {}\nProfile: {}\nStatus: {}",s.endpoint_label,s.profile,s.status)).font(k.ty.ui).color(k.c.text_muted).wrap().selectable(true))
    .child(text("Third-party notices are included in the application source under apps/rune/THIRD_PARTY_NOTICES.md.").font(k.ty.small).color(k.c.text_faint).wrap());
        }
    }
    body.child(el().grow(1.0))
}
