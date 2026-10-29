//! Layout controls use the original Hermes preset catalog and native Rune inputs.
use crate::{app::Viewer, decks};
use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, TextInput};
fn label(pane: &str) -> &str {
    match pane {
        "sessions" => "Sessions / Bots",
        "workspace" => "Conversation",
        "files" => "Files",
        "review" => "Review",
        "terminal" => "Terminal",
        _ => pane,
    }
}
pub fn controls(s: &Viewer, k: &Kit) -> El {
    let mut choices = row().key("layout-presets").w_full().flex_wrap().gap(5.);
    for (id, name) in decks::choices(s.simple_mode) {
        let id = (*id).to_owned();
        let selected = s.decks.active == id;
        choices = choices.child(
            Button::new(*name)
                .variant(if selected {
                    ButtonVariant::Neutral
                } else {
                    ButtonVariant::GhostMuted
                })
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut Viewer| s.apply_layout_preset(&id))
                .build(k)
                .key(format!("layout-preset-{}", id_name(name))),
        );
    }
    for saved in &s.decks.saved {
        let select = saved.id.clone();
        let remove = saved.id.clone();
        choices = choices.child(
            row()
                .gap(2.)
                .child(
                    Button::new(&saved.name)
                        .variant(if s.decks.active == saved.id {
                            ButtonVariant::Neutral
                        } else {
                            ButtonVariant::GhostMuted
                        })
                        .control_size(ControlSize::Small)
                        .on_press(move |s: &mut Viewer| s.apply_layout_preset(&select))
                        .build(k)
                        .key(format!("layout-preset-{}", saved.id)),
                )
                .child(
                    Button::new("×")
                        .variant(ButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .help("Delete saved layout; keep the current panes")
                        .on_press(move |s: &mut Viewer| s.delete_layout_preset(&remove))
                        .build(k)
                        .key(format!("layout-delete-{}", saved.id)),
                ),
        );
    }
    let mut reveal = row()
        .key("layout-pane-controls")
        .w_full()
        .flex_wrap()
        .gap(4.);
    for pane in ["sessions", "workspace", "review", "files", "terminal"] {
        reveal = reveal.child(
            Button::new(format!("Show {}", label(pane)))
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .disabled(s.layout.shows(pane, &|p| s.pane_visible(p)))
                .on_press(move |s: &mut Viewer| s.reveal_pane(pane))
                .build(k)
                .key(format!("layout-show-{pane}")),
        );
    }
    col().key("layout-controls").w_full().gap(6.).padding(10.).fill(k.c.layer_01)
        .child(row().w_full().gap(8.).center_y().child(text("Layout").font(k.ty.ui_medium).color(k.c.text_base)).child(choices.grow(1.).min_w(0.)))
        .child(text(if s.simple_mode {"Simple remembers its sidebar position independently from your Advanced layout."}else{"Drag pane tabs to a group or an edge; drag dividers to resize. Saved layouts keep geometry and visibility."}).font(k.ty.small).color(k.c.text_muted).wrap())
        .child(reveal)
        .child(row().w_full().flex_wrap().gap(6.).center_y()
            .child(TextInput::new(&s.layout_name).placeholder("Name this layout").on_edit(|s:&mut Viewer|&mut s.layout_name).build(k).key("layout-preset-name").w(200.))
            .child(Button::new("Save layout").variant(ButtonVariant::GhostMuted).disabled(s.layout_name.text.trim().is_empty()).on_press(|s:&mut Viewer|s.save_layout_preset()).build(k).key("layout-save-preset"))
            .child(Button::new("Reset layout").variant(ButtonVariant::GhostMuted).on_press(|s:&mut Viewer|s.apply_layout_preset(if s.simple_mode{"sidebar-left"}else{"default"})).build(k).key("layout-reset"))
            .child(Button::new("Done").on_press(|s:&mut Viewer|s.arranging=false).build(k).key("layout-done")))
}
fn id_name(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}
