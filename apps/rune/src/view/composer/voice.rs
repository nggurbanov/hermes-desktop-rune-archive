//! Dictation is separate from the empty-composer voice conversation action.
use crate::{
    app::Viewer,
    composer::{DraftScope, Popup},
    gateway::VoiceEngine,
};
use rune::{El, Icon, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit};
use rune_theme_opencode::icons as ic;
use std::time::Duration;
static MIC: Icon = Icon {
    name: "microphone",
    view: 24.0,
    body: r#"<rect x="9" y="2" width="6" height="13" rx="3" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#,
};
static SPEAK: Icon = Icon {
    name: "spoken-replies-off",
    view: 24.0,
    body: r#"<path d="m11 5-6 4H2v6h3l6 4V5ZM16 9l5 6m0-6-5 6M3 2l19 20" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
static WAKE: Icon = Icon {
    name: "wake-word-off",
    view: 24.0,
    body: r#"<path d="M6 8a7 7 0 0 1 13 3c0 5-5 4-5 8a3 3 0 0 1-6 0M10 9a3 3 0 0 1 6 2M3 3l18 18" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#,
};

static WAVE: Icon = Icon {
    name: "voice-conversation",
    view: 24.0,
    body: r#"<path d="M2 12h3l2-7 4 15 3-17 3 12 2-3h3" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
pub(super) fn fan(s: &Viewer, k: &Kit) -> El {
    let mut fan = el()
        .key("voice-fan")
        .role("button")
        .label("Voice controls")
        .size(28.0, 28.0)
        .focusable(true)
        .focus_ring(2.0, 1.0, k.c.border_focus)
        .on_press(|s: &mut Viewer| s.composer.voice_expanded = true)
        .on_hover_change(|s: &mut Viewer, inside, _| s.composer.voice_expanded = inside)
        .hover_delays(Duration::ZERO, Duration::from_millis(150))
        .child(
            IconButton::new(&MIC)
                .variant(IconButtonVariant::GhostMuted)
                .disabled(true)
                .label("Voice dictation, not yet available")
                .help("Dictation needs native audio capture")
                .build(k)
                .key("voice-dictation"),
        );
    if s.composer.voice_expanded {
        for (key, label, glyph, y) in [
            (
                "voice-speak",
                "Speak replies, not yet available",
                &SPEAK,
                -36.0,
            ),
            (
                "voice-wake",
                "Wake word hey hermes, not yet available",
                &WAKE,
                -72.0,
            ),
        ] {
            fan = fan.child(
                IconButton::new(glyph)
                    .variant(IconButtonVariant::Neutral)
                    .disabled(true)
                    .label(label)
                    .help("Native audio integration is a later increment")
                    .build(k)
                    .key(key)
                    .size(28.0, 28.0)
                    .radius(14.0)
                    .absolute(0.0, y),
            );
        }
    }
    fan
}

pub(super) fn primary(s: &Viewer, k: &Kit, has_payload: bool) -> El {
    if has_payload {
        return IconButton::new(&ic::ARROW_UP)
            .variant(IconButtonVariant::Submit)
            .control_size(ControlSize::Large)
            .disabled(true)
            .label("Send, not yet available")
            .help("Sending and streaming are the next migration increment")
            .build(k)
            .key("composer-send")
            .radius(16.0);
    }
    let scope = s.composer_scope();
    let engine = s.voice.status(&scope).map(|s| s.engine.label());
    row()
        .center_y()
        .gap(2.0)
        .child(
            IconButton::new(&WAVE)
                .variant(IconButtonVariant::Submit)
                .control_size(ControlSize::Large)
                .disabled(true)
                .label("Start voice chat, not yet available")
                .help(
                    engine
                        .map(|s| format!("{s} · Native voice execution is not available yet"))
                        .unwrap_or_else(|| {
                            "Voice chat needs native audio and the conversation pipeline".into()
                        }),
                )
                .build(k)
                .key("voice-chat-start")
                .radius(16.0),
        )
        .child(
            IconButton::new(&ic::CHEVRON_DOWN)
                .variant(IconButtonVariant::GhostMuted)
                .label("Voice chat engine")
                .help("Inspect the connected agent's voice engine; read only")
                .on_press_tx(|s: &mut Viewer, tx| {
                    s.open_composer(Popup::VoiceEngine);
                    tx.instant();
                })
                .build(k)
                .key("voice-engine-trigger"),
        )
}

pub(super) fn engine_menu(s: &Viewer, scope: &DraftScope, k: &Kit) -> El {
    let status = s.voice.status(scope);
    let loading = s.voice.loading(scope);
    let mut menu = col().w(450.0).px(8.0).py(8.0).gap(4.0).child(
        text("Voice chat engine")
            .font(k.ty.small)
            .color(k.c.text_muted)
            .px(10.0)
            .py(6.0),
    );
    if let Some(status) = status {
        for engine in [VoiceEngine::Chained, VoiceEngine::GptLive] {
            let selected = engine == status.engine;
            let detail = match engine {
                VoiceEngine::Chained => "Speech → Hermes turn → spoken reply",
                VoiceEngine::GptLive if status.live_available => {
                    "Available on this agent · Native WebRTC pending"
                }
                VoiceEngine::GptLive => "Unavailable on this agent",
            };
            menu =
                menu.child(
                    row()
                        .key(match engine {
                            VoiceEngine::Chained => "voice-engine-chained",
                            VoiceEngine::GptLive => "voice-engine-gpt-live",
                        })
                        .role("radio")
                        .label(format!(
                            "{}{} · Read only",
                            engine.label(),
                            if selected { " · Selected" } else { "" }
                        ))
                        .w_full()
                        .min_w(0.0)
                        .gap(10.0)
                        .px(10.0)
                        .py(9.0)
                        .center_y()
                        .radius(6.0)
                        .fill(if selected {
                            k.c.bg_base
                        } else {
                            rune::Color::TRANSPARENT
                        })
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(3.0)
                                .child(
                                    text(engine.label())
                                        .font(k.ty.ui_medium)
                                        .color(k.c.text_base)
                                        .wrap(),
                                )
                                .child(text(detail).font(k.ty.small).color(k.c.text_faint).wrap()),
                        )
                        .child(el().size(16.0, 16.0).children(
                            selected.then(|| icon(&ic::CHECK, 16.0).color(k.c.text_base)),
                        )),
                );
        }
        menu=menu.child(text("Current agent setting · Read only. Changing the engine and starting audio are not available yet.")
            .font(k.ty.small).color(k.c.text_muted).wrap().px(10.0).py(6.0));
    } else {
        menu = menu.child(
            text(if loading {
                "Reading voice capabilities…"
            } else {
                "No confirmed voice engine. Connect an agent and refresh."
            })
            .font(k.ty.ui)
            .color(k.c.text_muted)
            .wrap()
            .px(10.0)
            .py(8.0),
        );
    }
    if let Some(error) = s.voice.error(scope) {
        menu = menu.child(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap()
                .px(10.0),
        );
    }
    menu.child(
        Button::new("Refresh voice status")
            .variant(ButtonVariant::GhostMuted)
            .disabled(loading || !s.can_refresh_voice_status())
            .on_press(|s: &mut Viewer| s.reload_voice_status())
            .build(k)
            .key("voice-status-refresh"),
    )
}
