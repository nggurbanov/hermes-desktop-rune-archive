use super::*;
use rune::FileDialogOptions;
pub(super) fn panel(s: &Viewer, a: &Artifact, k: &Kit) -> El {
    let state = &s.artifacts;
    let copy = a.value.clone();
    let mut pane = col()
        .w(390.0)
        .min_w(240.0)
        .min_h(0.0)
        .gap(10.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(
                    text(&a.label)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .wrap()
                        .grow(1.0),
                )
                .child(btn(s, "Close", Action::ClosePreview, false, k)),
        )
        .child(note(&a.value, k).selectable(true))
        .child(note(
            format!("{} · {} · {}", a.profile, a.title, a.time()),
            k,
        ))
        .child(
            Button::new("Copy location")
                .variant(ButtonVariant::Ghost)
                .on_press_tx(move |_: &mut Viewer, tx| tx.copy(copy.clone()))
                .build(k),
        );
    if a.kind == "image"
        || a.kind == "audio"
        || crate::preview::Kind::for_source(&a.value) == Some(crate::preview::Kind::Audio)
    {
        pane = pane.child(
            btn(
                s,
                if a.kind == "image" {
                    "Open image viewer"
                } else {
                    "Open audio player"
                },
                Action::Inspect,
                state.preview_loading
                    || a.kind == "image"
                        && a.value.starts_with("data:")
                        && state.download.is_none(),
                k,
            )
            .key("artifact-native-preview"),
        );
    }
    if state.preview_loading {
        pane = pane.child(note("Reading this artifact through its owning agent…", k));
    }
    if let Some(error) = &state.preview_error {
        pane = pane
            .child(text(error).font(k.ty.small).color(k.c.danger.fg).wrap())
            .child(btn(s, "Retry preview", Action::LoadFile, false, k));
    }
    if let Some(image) = &state.preview {
        if let Some(error) = image.error() {
            pane = pane.child(note(error.to_string(), k)).child(btn(
                s,
                "Retry image",
                Action::LoadFile,
                false,
                k,
            ));
        } else {
            pane = pane
                .child(
                    row()
                        .gap(5.0)
                        .child(btn(
                            s,
                            "−",
                            Action::Zoom(state.zoom / 1.25),
                            state.zoom <= 0.25,
                            k,
                        ))
                        .child(note(format!("{}%", (state.zoom * 100.0).round()), k))
                        .child(btn(
                            s,
                            "+",
                            Action::Zoom(state.zoom * 1.25),
                            state.zoom >= 4.0,
                            k,
                        ))
                        .child(btn(s, "Fit", Action::Zoom(1.0), false, k)),
                )
                .child(
                    col().w_full().h(370.0).scroll_xy().child(
                        el().w(360.0 * state.zoom)
                            .h(350.0 * state.zoom)
                            .pixels_fit(image.pixels(), rune::SurfaceFit::Fit),
                    ),
                );
        }
    }
    if let Some(content) = &state.preview_text {
        pane = pane.child(
            col()
                .w_full()
                .max_h(480.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    text(content)
                        .font(k.ty.small)
                        .color(k.c.text_base)
                        .wrap()
                        .selectable(true),
                ),
        );
    }
    if a.kind != "image" && a.kind != "link" {
        pane = pane.child(btn(
            s,
            "Preview file",
            Action::LoadFile,
            state.preview_loading,
            k,
        ));
    }
    pane = pane.child(btn(
        s,
        if a.kind == "link" {
            "Open link"
        } else if state.saving {
            "Preparing native copy…"
        } else {
            "Open in default application"
        },
        Action::NativeOpen,
        state.saving,
        k,
    ));
    if !a.value.starts_with("http://") && !a.value.starts_with("https://") {
        let artifact = a.clone();
        let owner = s.composer_scope();
        let bytes = state.download.as_ref().map(|(_, bytes)| bytes.clone());
        pane = pane.child(
            Button::new(if state.saving {
                "Saving…"
            } else {
                "Save file…"
            })
            .disabled(state.saving || state.demo)
            .on_press_tx(move |s: &mut Viewer, tx| {
                let artifact = artifact.clone();
                let owner = owner.clone();
                let bytes = bytes.clone();
                if s.composer_scope().connection != owner.connection {
                    return;
                }
                tx.save_file(
                    FileDialogOptions {
                        title: "Save artifact".into(),
                        file_name: Some(artifact.label.clone()),
                        ..Default::default()
                    },
                    move |s: &mut Viewer, result, _| {
                        if s.composer_scope().connection != owner.connection
                            || s.profile != owner.profile
                        {
                            return;
                        }
                        match result {
                            Ok(Some(path)) => {
                                if let Some(bytes) = bytes.clone() {
                                    s.artifacts.write_bytes(path, bytes, false);
                                } else {
                                    s.download_artifact(artifact.clone(), path, owner.clone());
                                }
                            }
                            Ok(None) => {}
                            Err(_) => {
                                s.artifacts.error =
                                    Some("The native save dialog is unavailable".into())
                            }
                        }
                    },
                );
            })
            .build(k),
        );
    }
    pane
}
