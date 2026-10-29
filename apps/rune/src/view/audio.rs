//! Visible recording state follows a confirmed native device and scoped transport.
use crate::{app::Viewer, audio::Status};
use rune::{El, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit};
pub fn panel(s: &Viewer, k: &Kit) -> El {
    let scope = s.composer_scope();
    let Some(view) = s.audio.view(&scope) else {
        return el().h(0.0);
    };
    let mut panel = col().w_full().px(20.0).gap(6.0);
    if let Some(error) = &view.error {
        panel = panel.child(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if view.status != Status::Idle {
        let label = match view.status {
            Status::Idle => "Idle",
            Status::Starting => "Opening microphone…",
            Status::Listening if s.wake_enabled => "Listening for hey hermes",
            Status::Listening => "Listening",
            Status::Transcribing => "Transcribing…",
            Status::Thinking => "Hermes is thinking",
            Status::Speaking => "Speaking",
        };
        panel = panel.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text(format!("{label} · {:.0}s", view.elapsed))
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                )
                .child(
                    el().w(90.0)
                        .h(3.0)
                        .radius(1.5)
                        .fill(k.c.border_muted)
                        .child(
                            el().w(90.0 * view.level.clamp(0.0, 1.0))
                                .h(3.0)
                                .fill(k.c.text_muted),
                        ),
                )
                .child(el().grow(1.0))
                .children(
                    (view.voice_active && view.status == Status::Listening).then(|| {
                        Button::new("Finish turn")
                            .variant(ButtonVariant::GhostMuted)
                            .on_press(|s: &mut Viewer| s.finish_voice_turn())
                            .build(k)
                    }),
                )
                .children(view.voice_active.then(|| {
                    let owner = scope.clone();
                    let muted = view.muted;
                    Button::new(if muted { "Unmute" } else { "Mute" })
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(move |s: &mut Viewer| s.audio.mute(&owner, !muted))
                        .build(k)
                }))
                .child(
                    Button::new("Cancel audio")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut Viewer| s.end_voice_chat())
                        .build(k),
                ),
        );
        if !view.partial.is_empty() {
            panel = panel.child(
                text(&view.partial)
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .wrap(),
            );
        }
    }
    panel
}
