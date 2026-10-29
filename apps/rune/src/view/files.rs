//! Native file navigation and the original preview/read-before-save contract.
use crate::{
    app::Viewer,
    files::{Action, Entry},
};
use rune::{El, FileDialogOptions, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, Kit, TextArea, TextInput};
fn action(label: impl Into<String>, value: Action, disabled: bool, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .disabled(disabled)
        .on_press(move |s: &mut Viewer| s.files_action(value.clone()))
        .build(k)
}
fn note(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.small).color(k.c.text_muted).wrap()
}
pub fn panel(s: &Viewer, k: &Kit) -> El {
    let state = &s.files;
    let mut body = col()
        .key("files")
        .role("group")
        .label("Files")
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(6.0)
        .fill(k.c.bg_deep)
        .child(
            row()
                .w_full()
                .h(36.0)
                .px(8.0)
                .center_y()
                .child(
                    text("Files")
                        .font(k.ty.small_medium)
                        .color(k.c.text_base)
                        .grow(1.0),
                )
                .child(action("↻", Action::Refresh, state.loading, k).key("files-refresh")),
        )
        .child(
            Button::new("Browse directory…")
                .variant(ButtonVariant::GhostMuted)
                .disabled(s.is_fixture())
                .on_press(|s: &mut Viewer| s.open_directory(crate::directories::Purpose::Files))
                .build(k),
        )
        .child(
            TextInput::new(&state.path)
                .width(None)
                .placeholder("Directory path")
                .on_edit(|s: &mut Viewer| &mut s.files.path)
                .on_submit(|s: &mut Viewer, _| {
                    let path = s.files.path.text.clone();
                    s.files_action(Action::Root(path));
                })
                .build(k)
                .key("files-root"),
        )
        .child(
            TextInput::new(&state.search)
                .width(None)
                .placeholder("Find a file")
                .on_edit(|s: &mut Viewer| &mut s.files.search)
                .build(k),
        )
        .child(
            Checkbox::new(state.show_ignored)
                .label("Show ignored files")
                .on_toggle(|s: &mut Viewer| s.files_action(Action::ShowIgnored))
                .build(k),
        );
    if let Some(error) = &state.error {
        body = body.child(note(error, k).role("alert"));
    }
    let mut tree = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec {
        stick_bottom: false,
    });
    if state.root.is_empty() {
        tree = tree.child(note(
            if state.loading {
                "Reading the workspace directory…"
            } else {
                "Choose a directory to browse this agent’s files."
            },
            k,
        ));
    } else {
        tree = tree.child(folder(s, &state.root, 0, k));
    }
    body.child(tree).child(dialogs(s, k))
}
fn folder(s: &Viewer, path: &str, depth: usize, k: &Kit) -> El {
    let state = &s.files;
    let mut list = col().w_full().gap(1.0);
    if depth > 40 {
        return list.child(note("Folder depth limit reached", k));
    }
    if let Some(folder) = state.folders.get(path) {
        if let Some(error) = &folder.error {
            list = list.child(note(error, k));
        }
        if folder.loading {
            list = list.child(note("Loading…", k));
        }
    }
    for entry in state.visible(path) {
        list = list.child(entry_row(s, entry, depth, k));
        if entry.directory && state.expanded.contains(&entry.path) {
            list = list.child(folder(s, &entry.path, depth + 1, k));
        }
    }
    list
}
fn entry_row(s: &Viewer, entry: &Entry, depth: usize, k: &Kit) -> El {
    let state = &s.files;
    let mut line = row()
        .w_full()
        .min_h(28.0)
        .pl(6.0 + depth as f32 * 10.0)
        .center_y();
    line = line.child(
        action(
            format!(
                "{} {}",
                if entry.directory {
                    if state.expanded.contains(&entry.path) {
                        "▾"
                    } else {
                        "▸"
                    }
                } else {
                    "·"
                },
                entry.name
            ),
            if entry.directory {
                Action::Toggle(entry.path.clone())
            } else {
                Action::Open(entry.path.clone())
            },
            false,
            k,
        )
        .grow(1.0)
        .min_w(0.0)
        .justify_start()
        .help(&entry.path)
        .key(format!("file-row-{}", crate::pages::segment(&entry.path))),
    );
    if state.local {
        line = line.child(action("…", Action::AskRename(entry.clone()), false, k));
    }
    line
}
fn dialogs(s: &Viewer, k: &Kit) -> El {
    let state = &s.files;
    let mut body = col().w_full().gap(6.0).padding(8.0);
    if let Some(entry) = &state.rename {
        let id = entry.path.clone();
        body = body
            .child(note(format!("Rename {}", entry.name), k))
            .child(
                TextInput::new(&state.name)
                    .width(None)
                    .on_edit(|s: &mut Viewer| &mut s.files.name)
                    .build(k),
            )
            .child(
                row()
                    .gap(4.0)
                    .child(action("Rename", Action::Rename, state.mutating, k))
                    .child(action("Cancel", Action::Cancel, state.mutating, k)),
            )
            .child(action(
                "Move to Trash…",
                Action::AskTrash(entry.clone()),
                false,
                k,
            ));
        let value = id;
        body = body.child(
            Button::new("Copy path")
                .variant(ButtonVariant::GhostMuted)
                .on_press_tx(move |_: &mut Viewer, tx| tx.copy(value.clone()))
                .build(k),
        );
    }
    if let Some(entry) = &state.trash {
        body = body
            .child(note(
                format!("Move {} to the device’s Trash?", entry.path),
                k,
            ))
            .child(
                row()
                    .gap(4.0)
                    .child(action("Move to Trash", Action::Trash, state.mutating, k))
                    .child(action("Keep", Action::Cancel, state.mutating, k)),
            );
    }
    body
}
pub fn preview(s: &Viewer, k: &Kit) -> El {
    let state = &s.files;
    let Some(editor) = &state.editor else {
        return col()
            .padding(24.0)
            .child(note("Select a file from the Files panel.", k));
    };
    let mut body = col()
        .key("file-preview")
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(12.0)
        .padding(20.0)
        .child(
            text(&editor.path)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .wrap()
                .selectable(true),
        )
        .child(note(
            format!(
                "{} · {} bytes{}",
                editor.owner.profile,
                editor.size,
                if editor.dirty() {
                    " · Unsaved edits"
                } else {
                    ""
                }
            ),
            k,
        ));
    if let Some(error) = &editor.error {
        body = body.child(note(error, k).role("alert"));
    }
    if let Some(notice) = &state.notice {
        body = body.child(note(notice, k));
    }
    let mut toolbar = row()
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            action(
                if editor.busy { "Saving…" } else { "Save" },
                Action::Save,
                !editor.writable() || !editor.dirty(),
                k,
            )
            .key("file-save"),
        )
        .child(action(
            "Discard edits",
            Action::Discard,
            editor.busy || !editor.dirty(),
            k,
        ))
        .child(action("Reload", Action::ReloadEditor, editor.busy, k));
    let owner = editor.owner.clone();
    let contents = editor.text.text.clone();
    let name = editor
        .path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("edited.txt")
        .to_owned();
    toolbar = toolbar.child(
        Button::new("Save a copy…")
            .variant(ButtonVariant::Ghost)
            .disabled(editor.binary || editor.loading)
            .on_press_tx(move |_: &mut Viewer, tx| {
                let owner = owner.clone();
                let contents = contents.clone();
                tx.save_file(
                    FileDialogOptions {
                        title: "Save file copy".into(),
                        file_name: Some(name.clone()),
                        ..Default::default()
                    },
                    move |s: &mut Viewer, result, _| match result {
                        Ok(Some(path)) => s.save_file_copy(owner.clone(), path, contents.clone()),
                        Ok(None) => {}
                        Err(_) => s.files.error = Some("The save dialog is unavailable".into()),
                    },
                );
            })
            .build(k),
    );
    let owner = editor.owner.clone();
    let source = editor.path.clone();
    let name = source
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("download")
        .to_owned();
    toolbar = toolbar
        .child(
            Button::new("Download…")
                .variant(ButtonVariant::Ghost)
                .on_press_tx(move |_: &mut Viewer, tx| {
                    let owner = owner.clone();
                    let source = source.clone();
                    tx.save_file(
                        FileDialogOptions {
                            title: "Download file".into(),
                            file_name: Some(name.clone()),
                            ..Default::default()
                        },
                        move |s: &mut Viewer, result, _| match result {
                            Ok(Some(path)) => {
                                s.download_selected_file(owner.clone(), source.clone(), path)
                            }
                            Ok(None) => {}
                            Err(_) => s.files.error = Some("The save dialog is unavailable".into()),
                        },
                    );
                })
                .build(k),
        )
        .child(el().grow(1.0))
        .child(
            Button::new("Quote in chat")
                .variant(ButtonVariant::Ghost)
                .disabled(editor.binary || editor.loading)
                .on_press(|s: &mut Viewer| s.quote_file())
                .build(k),
        );
    body = body.child(toolbar);
    if editor.loading {
        return body.child(note("Reading the file from its owner…", k));
    }
    if let Some(image) = &state.preview {
        body = body.child(
            el().pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
                .w_full()
                .grow(1.)
                .min_h(120.),
        );
    }
    if editor.binary {
        return body.child(note(
            "This is a binary file. Download it to open it in a device application.",
            k,
        ));
    }
    if editor.truncated {
        body = body.child(note(
            "This is a partial preview. It cannot overwrite the complete file.",
            k,
        ));
    }
    body.child(
        TextArea::new(&editor.text)
            .width(None)
            .on_edit(|s: &mut Viewer| {
                &mut s.files.editor.as_mut().expect("mounted file preview").text
            })
            .build(k)
            .grow(1.0)
            .min_h(100.0),
    )
}
