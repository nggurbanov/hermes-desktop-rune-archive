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
    let purpose = d.purpose;
    let edit = owner.clone();
    let submit = owner.clone();
    panel = panel.child(
        TextInput::new(&d.path)
            .width(None)
            .on_edit(move |s: &mut Viewer| {
                let current = s.directory_target_current(&edit, purpose);
                s.directories.edit(&edit, current)
            })
            .on_submit(move |s: &mut Viewer, _| {
                if !s.directory_target_current(&submit, purpose) {
                    return;
                }
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
    let parent = if matches!(
        d.purpose,
        crate::directories::Purpose::Projects(..) | crate::directories::Purpose::ProjectFolders(..)
    ) {
        let path = d.path.text.trim_end_matches('/');
        if path.starts_with('/') {
            path.rsplit_once('/').map(|(p, _)| {
                if p.is_empty() {
                    "/".to_owned()
                } else {
                    p.to_owned()
                }
            })
        } else {
            None
        }
    } else {
        std::path::Path::new(&d.path.text)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
    };
    if let Some(parent) = parent {
        let captured = owner.clone();
        list = list.child(
            Button::new("↑ Parent directory")
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut Viewer| {
                    if s.directory_target_current(&captured, purpose) {
                        s.directory_load(parent.clone());
                    }
                })
                .build(k),
        );
    }
    for entry in &d.entries {
        let path = entry.path.clone();
        let captured = owner.clone();
        list = list.child(
            Button::new(format!("▸ {}", entry.name))
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut Viewer| {
                    if s.directory_target_current(&captured, purpose) {
                        s.directory_load(path.clone());
                    }
                })
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
    let use_owner = owner.clone();
    let cancel_owner = owner.clone();
    Some(
        panel.child(list).child(
            row()
                .gap(8.)
                .child(
                    Button::new("Use this directory")
                        .disabled(d.loading || d.error.is_some())
                        .on_press(move |s: &mut Viewer| {
                            if s.directory_target_current(&use_owner, purpose) {
                                s.choose_directory();
                            }
                        })
                        .build(k),
                )
                .child(
                    Button::new("Cancel")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(move |s: &mut Viewer| {
                            if !s.directory_target_current(&cancel_owner, purpose) {
                                return;
                            }
                            match s.directories.purpose {
                                crate::directories::Purpose::Projects(dialog, pick) => {
                                    s.project_directory_return(dialog, pick, None)
                                }
                                crate::directories::Purpose::ProjectFolders(dialog, pick) => {
                                    s.project_folders_directory_return(dialog, pick, None)
                                }
                                _ => {}
                            }
                            s.invalidate_page_requests("directories.");
                            s.directories.owner = None;
                        })
                        .build(k),
                ),
        ),
    )
}
