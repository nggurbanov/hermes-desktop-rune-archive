//! Native owned audio controls, shared by Files, Artifacts and transcript attachments.
use crate::{
    app::Viewer,
    media_player::{Action, MediaPlayers, PlayerId, State},
};
use rune::{El, FileDialogOptions, Key, Phase, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit};

fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.).floor() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
pub fn panel(s: &Viewer, id: PlayerId, k: &Kit) -> El {
    content(&s.media_players, id, k, |s: &mut Viewer| {
        &mut s.media_players
    })
    .child(
        Button::new("Close preview")
            .variant(ButtonVariant::GhostMuted)
            .on_press(move |s: &mut Viewer| {
                if s.audio_preview == Some(id) {
                    s.close_audio_preview();
                }
            })
            .build(k)
            .key("audio-close"),
    )
}
fn action<S: 'static>(
    label: &str,
    key: &str,
    id: PlayerId,
    action: Action,
    disabled: bool,
    k: &Kit,
    access: fn(&mut S) -> &mut MediaPlayers,
) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .disabled(disabled)
        .on_press_tx(move |s: &mut S, tx| {
            access(s).act(id, action.clone());
            tx.instant();
        })
        .build(k)
        .key(key)
}
pub fn content<S: 'static>(
    players: &MediaPlayers,
    id: PlayerId,
    k: &Kit,
    access: fn(&mut S) -> &mut MediaPlayers,
) -> El {
    content_layout(players, id, k, access, false)
}
pub fn inline(
    players: &MediaPlayers,
    id: PlayerId,
    k: &Kit,
    access: fn(&mut Viewer) -> &mut MediaPlayers,
) -> El {
    content_layout(players, id, k, access, true)
}
fn content_layout<S: 'static>(
    players: &MediaPlayers,
    id: PlayerId,
    k: &Kit,
    access: fn(&mut S) -> &mut MediaPlayers,
    inline: bool,
) -> El {
    let mut body = col()
        .key("native-audio-preview")
        .role("group")
        .label("Audio preview")
        .w_full()
        .min_h(0.)
        .min_w(0.)
        .padding(if inline { 8. } else { 24. })
        .gap(if inline { 8. } else { 16. });
    if !inline {
        body = body.h_full().scroll(rune::ScrollSpec {
            stick_bottom: false,
        });
    }
    let Some(player) = players.get(id) else {
        return body.child(
            text("This audio preview is closed")
                .font(k.ty.ui)
                .color(k.c.text_muted),
        );
    };
    body = body
        .child(
            text(&player.target.label)
                .font(k.ty.body_medium)
                .color(k.c.text_base)
                .wrap(),
        )
        .child(
            text(format!(
                "{} · {}",
                player.target.owner.profile,
                player
                    .target
                    .owner
                    .session
                    .as_deref()
                    .unwrap_or("Profile files")
            ))
            .font(k.ty.small)
            .color(k.c.text_muted)
            .key("audio-owner"),
        );
    let state = match player.state {
        State::Loading => "Reading audio metadata…",
        State::Paused => "Paused",
        State::Buffering => "Buffering…",
        State::Playing => "Playing",
        State::Ended => "Finished",
        State::Error => "Playback unavailable",
    };
    body = body.child(
        text(state)
            .font(k.ty.ui)
            .color(k.c.text_muted)
            .key("audio-state"),
    );
    if let Some(error) = &player.error {
        body = body.child(
            text(error)
                .role("alert")
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    let duration = player.duration.unwrap_or(0.);
    let ratio = if duration > 0. {
        (player.position / duration).clamp(0., 1.) as f32
    } else {
        0.
    };
    let seekable = player.seekable;
    let position = player.position;
    let progress = el()
        .key("audio-seek")
        .label("Audio playback position")
        .role("slider")
        .focusable(seekable)
        .w_full()
        .h(28.)
        .center_y()
        .on_drag(move |s: &mut S, drag, tx| {
            if seekable && drag.phase == Phase::Up && !drag.cancelled() && drag.w > 0. {
                access(s).act(
                    id,
                    Action::Seek((drag.x / drag.w).clamp(0., 1.) as f64 * duration),
                );
                tx.instant();
            }
        })
        .on_key(move |s: &mut S, event, tx| {
            if !seekable {
                return false;
            }
            let next = match event.key {
                Key::Left => (position - 5.).max(0.),
                Key::Right => (position + 5.).min(duration),
                Key::Home => 0.,
                Key::End => duration,
                _ => return false,
            };
            access(s).act(id, Action::Seek(next));
            tx.instant();
            true
        })
        .child(
            el().w_full()
                .h(4.)
                .radius(2.)
                .fill(k.c.border_muted)
                .child(el().w_frac(ratio).h(4.).radius(2.).fill(k.c.text_base)),
        );
    body = body.child(progress).child(
        row()
            .w_full()
            .child(
                text(clock(position))
                    .font(k.ty.mono)
                    .color(k.c.text_base)
                    .key("audio-position"),
            )
            .child(el().grow(1.))
            .child(
                text(
                    player
                        .duration
                        .map(clock)
                        .unwrap_or_else(|| "Duration unknown".into()),
                )
                .font(k.ty.mono)
                .color(k.c.text_muted)
                .key("audio-duration"),
            ),
    );
    let paused = player.paused;
    let controls = row()
        .w_full()
        .flex_wrap()
        .center_y()
        .gap(8.)
        .child(action(
            if paused { "Play" } else { "Pause" },
            "audio-play-pause",
            id,
            if paused { Action::Play } else { Action::Pause },
            player.state == State::Error,
            k,
            access,
        ))
        .child(action(
            "−5 s",
            "audio-back",
            id,
            Action::Seek((position - 5.).max(0.)),
            !seekable,
            k,
            access,
        ))
        .child(action(
            "+5 s",
            "audio-forward",
            id,
            Action::Seek(position + 5.),
            !seekable,
            k,
            access,
        ))
        .child(action(
            if player.muted { "Unmute" } else { "Mute" },
            "audio-mute",
            id,
            Action::Mute(!player.muted),
            false,
            k,
            access,
        ))
        .child(action(
            "−",
            "audio-volume-down",
            id,
            Action::Volume(player.volume - 0.1),
            player.volume <= 0.,
            k,
            access,
        ))
        .child(
            text(format!("{}%", (player.volume * 100.).round() as u32))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .key("audio-volume"),
        )
        .child(action(
            "+",
            "audio-volume-up",
            id,
            Action::Volume(player.volume + 0.1),
            player.volume >= 1.,
            k,
            access,
        ));
    body = body.child(controls);
    let mut rates = row()
        .key("audio-rates")
        .w_full()
        .flex_wrap()
        .center_y()
        .gap(4.)
        .child(text("Speed").font(k.ty.small).color(k.c.text_muted));
    for speed in [0.25f32, 0.5, 0.75, 1., 1.25, 1.5, 2., 3., 4.] {
        rates = rates.child(
            Button::new(format!("{speed}×"))
                .variant(if (player.rate - speed).abs() < 0.001 {
                    ButtonVariant::Neutral
                } else {
                    ButtonVariant::GhostMuted
                })
                .on_press(move |s: &mut S| access(s).act(id, Action::Rate(speed)))
                .build(k)
                .key(format!("audio-rate-{speed}")),
        );
    }
    body = body.child(rates);
    if !seekable && player.state != State::Loading {
        body = body.child(
            text("This source streams sequentially; seeking is unavailable.")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap(),
        );
    }
    if player.state == State::Error {
        body = body.child(action(
            "Retry",
            "audio-retry",
            id,
            Action::Retry,
            false,
            k,
            access,
        ));
    }
    if player.saving {
        body = body.child(
            text(format!(
                "Saving original: {} KB",
                player.save_progress / 1024
            ))
            .font(k.ty.small)
            .color(k.c.text_muted)
            .key("audio-save-progress"),
        );
    }
    if let Some(error) = &player.save_error {
        body = body.child(
            text(error)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(notice) = &player.notice {
        body = body.child(
            text(notice)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .key("audio-notice"),
        );
    }
    let label = player.target.label.clone();
    body.child(
        Button::new(if player.saving {
            "Saving…"
        } else {
            "Save original…"
        })
        .variant(ButtonVariant::GhostMuted)
        .disabled(player.saving)
        .on_press_tx(move |_: &mut S, tx| {
            tx.save_file(
                FileDialogOptions {
                    title: "Save original audio".into(),
                    file_name: Some(label.clone()),
                    ..Default::default()
                },
                move |s: &mut S, result, _| match result {
                    Ok(Some(path)) => access(s).act(id, Action::SaveOriginal(path)),
                    Err(_) => access(s).act(id, Action::SaveDialogError),
                    _ => {}
                },
            )
        })
        .build(k)
        .key("audio-save"),
    )
}
