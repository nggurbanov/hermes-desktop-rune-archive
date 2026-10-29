//! Device settings are distinct from the connected agent's configuration.
use crate::{
    app::Viewer,
    notifications::{Action, Availability, Kind},
};
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Switch};

pub fn page(s: &Viewer, k: &Kit) -> El {
    let state = &s.notifications;
    let mut body = col()
        .key("native-notifications-settings")
        .w_full()
        .h_full()
        .gap(16.)
        .padding(28.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    body = body.child(text("Notifications").font(k.ty.body_medium).color(k.c.text_base))
        .child(text("These preferences belong to this device. Hermes keeps background conversations quiet and shows notices when your attention is needed.").font(k.ty.ui).color(k.c.text_muted).wrap())
        .child(text(state.availability.label()).key("native-notifications-availability").font(k.ty.small).color(if matches!(state.availability, Availability::Unavailable(_)) { k.c.danger.fg } else { k.c.text_muted }).wrap())
        .child(Switch::new(state.prefs.enabled).label("Enable native notifications").on_toggle(|s: &mut Viewer| s.notification_action(Action::Enabled(!s.notifications.prefs.enabled))).build(k).key("native-notifications-enabled"));
    for kind in Kind::ALL {
        let source_ready = !matches!(kind, Kind::BackgroundDone | Kind::Plugin);
        let enabled = state.prefs.enabled && source_ready;
        body = body.child(
            col()
                .w_full()
                .gap(5.)
                .child(
                    Switch::new(enabled && state.prefs.kind(kind))
                        .label(kind.label())
                        .disabled(!enabled)
                        .on_toggle(move |s: &mut Viewer| {
                            s.notification_action(Action::Kind(
                                kind,
                                !s.notifications.prefs.kind(kind),
                            ))
                        })
                        .build(k)
                        .key(format!("native-notifications-{}", kind.id())),
                )
                .child(
                    text(if source_ready {
                        kind.description()
                    } else if kind == Kind::BackgroundDone {
                        "Background-task alerts are unavailable in this build."
                    } else {
                        "Plugin alerts are unavailable in this build."
                    })
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .wrap(),
                ),
        );
    }
    let controls = row()
        .w_full()
        .flex_wrap()
        .gap(8.)
        .child(
            Button::new(if state.testing {
                "Sending test…"
            } else {
                "Send test notification"
            })
            .disabled(
                state.testing
                    || matches!(
                        state.availability,
                        Availability::NotStarted | Availability::Checking
                    ),
            )
            .on_press(|s: &mut Viewer| s.notification_action(Action::Test))
            .build(k)
            .key("native-notifications-test"),
        )
        .child(
            Button::new("Check desktop service")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Viewer| s.notification_action(Action::Refresh))
                .build(k)
                .key("native-notifications-refresh"),
        );
    body = body.child(controls);
    if let Some(error) = &state.error {
        body = body.child(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(notice) = &state.notice {
        body = body.child(
            text(notice)
                .key("native-notifications-result")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
        );
    }
    body.child(text("Your desktop decides how banners appear, whether they make a sound, and whether notification buttons are available.").font(k.ty.small).color(k.c.text_faint).wrap())
}
