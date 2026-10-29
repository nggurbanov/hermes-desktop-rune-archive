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
        return crate::view::connections::panel(s, k, layer);
    }
    let mut body = col()
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
                    text("These preferences apply to this Hermes app on this device.")
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
                        .on_toggle(|s: &mut Viewer| s.sidebar.toggle())
                        .build(k),
                )
                .child(
                    Switch::new(s.inspector.is_open())
                        .label("Show tools panel")
                        .on_toggle(|s: &mut Viewer| s.inspector.toggle())
                        .build(k),
                );
        }
        "shortcuts" => {
            body = body.child(
                text("Keyboard shortcuts")
                    .font(k.ty.body_medium)
                    .color(k.c.text_base),
            );
            for (command, keys) in [
                ("New conversation", "⌘ / Ctrl + N"),
                ("Settings", "⌘ / Ctrl + ,"),
                ("Command Center", "⌘ / Ctrl + K"),
                ("Toggle sidebar", "⌘ / Ctrl + B"),
                ("Toggle tools panel", "⌘ / Ctrl + J"),
                ("Send message", "Enter"),
                ("New line", "Shift + Enter"),
                ("Choose model", "⌘ / Ctrl + 1 … 9 while picker is open"),
                ("Dismiss menu", "Escape"),
            ] {
                body = body.child(
                    row()
                        .w_full()
                        .gap(20.0)
                        .child(text(command).font(k.ty.ui).color(k.c.text_base).grow(1.0))
                        .child(text(keys).font(k.ty.ui).color(k.c.text_muted)),
                );
            }
        }
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
