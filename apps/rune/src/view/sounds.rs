use crate::{
    app::Viewer,
    sounds::{Action, PRESETS},
};
use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Select, Switch};
pub fn rows(s: &Viewer, k: &Kit, layer: &mut Vec<El>) -> El {
    let state = &s.sounds;
    let options = PRESETS
        .iter()
        .map(|(_, name)| name.to_string())
        .collect::<Vec<_>>();
    let selected = PRESETS
        .iter()
        .find(|(id, _)| *id == state.prefs.preset)
        .map(|(_, name)| *name)
        .unwrap_or(PRESETS[1].1);
    let mut body=col().key("completion-sound-settings").w_full().gap(9.).pt(12.)
        .child(text("Sounds").font(k.ty.body_medium).color(k.c.text_base))
        .child(Switch::new(state.prefs.muted).label("Mute app sounds").on_toggle(|s:&mut Viewer|s.sound_action(Action::Mute(!s.sounds.muted()))).build(k).key("completion-sound-mute"))
        .child(text("Silences automatic completion cues and notification sounds on this device. You can still preview a sound below.").font(k.ty.small).color(k.c.text_muted).wrap())
        .child(text("Completion sound").font(k.ty.ui_medium).color(k.c.text_base))
        .child(row().w_full().flex_wrap().gap(8.)
            .child(Select::new("completion-sound-preset",&options).current(Some(selected)).open(state.open).on_toggle(|s:&mut Viewer|s.sounds.open=!s.sounds.open).on_close(|s:&mut Viewer|s.sounds.open=false).on_choose(|s:&mut Viewer,value,_|{s.sounds.open=false;if let Some((id,_))=PRESETS.iter().find(|(_,name)|*name==value){s.sound_action(Action::Preset(*id));}}).build(k,layer))
            .child(Button::new(if state.previewing{"Playing…"}else{"Preview"}).variant(ButtonVariant::Ghost).disabled(!state.initialized||state.prefs.preset==0||state.previewing).on_press(|s:&mut Viewer|s.sound_action(Action::Preview)).build(k).key("completion-sound-preview")))
        .child(text("Plays when an agent finishes a reply. None turns the cue off.").font(k.ty.small).color(k.c.text_muted).wrap());
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
        body = body.child(text(notice).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    body
}
