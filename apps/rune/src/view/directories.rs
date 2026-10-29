use crate::app::Viewer;
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, TextInput};
pub fn panel(s: &Viewer, k: &Kit) -> Option<El> {
    let owner = s.directories.owner.clone()?;
    let d = &s.directories;
    let mut panel = col()
        .key("directory-picker")
        .pin(Some(80.), Some(70.), Some(80.), Some(70.))
        .padding(24.)
        .gap(12.)
        .fill(k.c.bg_deep)
        .radius(12.)
        .border(1., k.c.border_muted)
        .child(
            text(format!("Choose directory · {}", owner.profile))
                .font(k.ty.body_medium)
                .color(k.c.text_base),
        );
    let edit = owner.clone();
    panel = panel.child(
        TextInput::new(&d.path)
            .width(None)
            .on_edit(move |s: &mut Viewer| s.directories.edit(&edit))
            .on_submit(|s: &mut Viewer, _| {
                let path = s.directories.path.text.clone();
                s.directory_load(path);
            })
            .build(k),
    );
    if let Some(error) = &d.error {
        panel = panel.child(text(error).font(k.ty.small).color(k.c.danger.fg).wrap());
    }
    let mut list = col()
        .w_full()
        .grow(1.)
        .min_h(0.)
        .gap(3.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    if let Some(parent) = std::path::Path::new(&d.path.text).parent() {
        let parent = parent.to_string_lossy().into_owned();
        list = list.child(
            Button::new("↑ Parent directory")
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut Viewer| s.directory_load(parent.clone()))
                .build(k),
        );
    }
    for entry in &d.entries {
        let path = entry.path.clone();
        list = list.child(
            Button::new(format!("▸ {}", entry.name))
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut Viewer| s.directory_load(path.clone()))
                .build(k)
                .w_full()
                .justify_start(),
        );
    }
    if d.loading {
        list = list.child(
            text("Reading directories…")
                .font(k.ty.small)
                .color(k.c.text_muted),
        );
    }
    Some(
        panel.child(list).child(
            row()
                .gap(8.)
                .child(
                    Button::new("Use this directory")
                        .disabled(d.loading || d.error.is_some())
                        .on_press(|s: &mut Viewer| s.choose_directory())
                        .build(k),
                )
                .child(
                    Button::new("Cancel")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(|s: &mut Viewer| {
                            s.invalidate_page_requests("directories.");
                            s.directories.owner = None;
                        })
                        .build(k),
                ),
        ),
    )
}
