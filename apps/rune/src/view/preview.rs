//! Shared Rune preview toolbar/canvas for Files, Artifacts and isolated native acceptance.
use crate::{
    app::Viewer,
    preview::{Action, Service},
};
use rune::{Cursor, El, FileDialogOptions, Key, PointerButton, SurfaceFit, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit};
pub fn panel(s: &Viewer, k: &Kit) -> El {
    content(&s.preview, k, |s: &mut Viewer| &mut s.preview)
}
fn button<S: 'static>(
    label: &str,
    key: &str,
    id: u64,
    action: Action,
    k: &Kit,
    access: fn(&mut S) -> &mut Service,
) -> El {
    Button::new(label)
        .variant(ButtonVariant::GhostMuted)
        .on_press_tx(move |s: &mut S, tx| {
            access(s).action(id, action.clone());
            tx.instant();
        })
        .build(k)
        .key(key)
}
pub fn content<S: 'static>(service: &Service, k: &Kit, access: fn(&mut S) -> &mut Service) -> El {
    let mut body = col()
        .key("native-file-preview")
        .role("group")
        .label("File preview")
        .w_full()
        .h_full()
        .min_w(0.)
        .min_h(0.)
        .fill(k.c.bg_deep);
    let Some(document) = service.active() else {
        return body.child(
            text("Choose a file to preview")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .padding(24.),
        );
    };
    let id = document.id;
    let mut toolbar = row().w_full().min_h(42.).center_y().gap(4.).px(10.).child(
        text(&document.target.label)
            .font(k.ty.small_medium)
            .color(k.c.text_base)
            .grow(1.)
            .min_w(0.)
            .help(&document.target.label),
    );
    if document.image.is_some() {
        toolbar = toolbar
            .child(
                button("−", "preview-zoom-out", id, Action::Zoom(0.8), k, access).label("Zoom out"),
            )
            .child(
                text(format!(
                    "{}%",
                    (document.camera.scale * 100.).round() as u32
                ))
                .key("preview-scale")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .w(48.),
            )
            .child(
                button("+", "preview-zoom-in", id, Action::Zoom(1.25), k, access).label("Zoom in"),
            )
            .child(button("Fit", "preview-fit", id, Action::Fit, k, access))
            .child(button(
                "100%",
                "preview-actual",
                id,
                Action::ActualSize,
                k,
                access,
            ));
    }
    let label = document
        .target
        .label
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("download")
        .to_owned();
    toolbar = toolbar.child(
        Button::new(if document.saving {
            "Saving…"
        } else {
            "Save original…"
        })
        .variant(ButtonVariant::GhostMuted)
        .disabled(document.original.is_none() || document.saving)
        .on_press_tx(move |_: &mut S, tx| {
            tx.save_file(
                FileDialogOptions {
                    title: "Save original file".into(),
                    file_name: Some(label.clone()),
                    ..Default::default()
                },
                move |s: &mut S, result, _| match result {
                    Ok(Some(path)) => access(s).action(id, Action::SaveOriginal(path)),
                    Err(_) => access(s).action(id, Action::SaveDialogError),
                    _ => {}
                },
            );
        })
        .build(k)
        .key("preview-save"),
    );
    let source = match &document.target.source {
        crate::preview::Source::AgentPath(path) => path.as_str(),
        crate::preview::Source::RemoteUrl(_) => "Remote attachment",
        crate::preview::Source::Inline { .. } => "Inline attachment",
    };
    body = body.child(toolbar).child(
        row()
            .w_full()
            .gap(12.)
            .px(12.)
            .py(6.)
            .child(
                text(format!(
                    "{} · {}",
                    document.target.owner.profile,
                    document
                        .target
                        .owner
                        .session
                        .as_deref()
                        .unwrap_or("Profile files")
                ))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .key("preview-owner"),
            )
            .child(
                text(source)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .grow(1.)
                    .min_w(0.)
                    .help(source),
            ),
    );
    if let Some(error) = &document.error {
        body =
            body.child(
                row()
                    .w_full()
                    .px(12.)
                    .py(8.)
                    .gap(8.)
                    .child(
                        text(error)
                            .role("alert")
                            .font(k.ty.small)
                            .color(k.c.danger.fg)
                            .wrap()
                            .grow(1.),
                    )
                    .children(
                        document.target.kind.available().then(|| {
                            button("Retry", "preview-retry", id, Action::Retry, k, access)
                        }),
                    ),
            );
    }
    if let Some(notice) = &document.notice {
        body = body.child(
            text(notice)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .px(12.)
                .py(6.),
        );
    }
    if document.loading {
        return body.child(
            text("Reading the original from its owning agent…")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .padding(24.),
        );
    }
    let mut canvas = el()
        .key("preview-canvas")
        .role("image")
        .label(format!("Preview: {}", document.target.label))
        .w_full()
        .grow(1.)
        .min_h(80.)
        .min_w(0.)
        .clip_rounded(0.)
        .cursor(Cursor::Grab)
        .pointer_buttons([PointerButton::Primary, PointerButton::Middle])
        .on_layout(move |s: &mut S, rect| {
            if let Some(d) = access(s).active_mut(id) {
                d.camera.layout(rect);
            }
        })
        .on_wheel(move |s: &mut S, e, tx| {
            if let Some(d) = access(s).active_mut(id) {
                d.camera.wheel(-e.dy, [e.x, e.y]);
                tx.instant();
            }
            true
        })
        .on_magnify(move |s: &mut S, e, tx| {
            if let Some(d) = access(s).active_mut(id) {
                d.camera.zoom(1. + e.delta, Some([e.x, e.y]));
                tx.instant();
            }
            true
        })
        .on_smart_magnify(move |s: &mut S, x, y, tx| {
            if let Some(d) = access(s).active_mut(id) {
                d.camera.smart_zoom([x, y]);
                tx.instant();
            }
            true
        })
        .on_drag(move |s: &mut S, e, tx| {
            if let Some(d) = access(s).active_mut(id) {
                d.camera.drag(e.phase, e.dx, e.dy);
                tx.instant();
            }
        })
        .on_key(move |s: &mut S, key, tx| {
            let action = match &key.key {
                Key::Text(t) if t == "+" || t == "=" => Action::Zoom(1.25),
                Key::Text(t) if t == "-" => Action::Zoom(0.8),
                Key::Text(t) if t == "0" => Action::Fit,
                Key::Text(t) if t == "1" => Action::ActualSize,
                _ => return false,
            };
            access(s).action(id, action);
            tx.instant();
            true
        });
    if let Some(image) = &document.image {
        let [x, y, w, h] = document.camera.bounds();
        if image.is_ready() {
            canvas = canvas.child(
                el().pixels_fit(image.pixels(), SurfaceFit::Fill)
                    .absolute(x, y)
                    .w(w)
                    .h(h),
            );
        } else {
            canvas = canvas.child(
                text("Decoding image…")
                    .font(k.ty.ui)
                    .color(k.c.text_muted)
                    .padding(24.),
            );
        }
    }
    let dimensions = document.camera.image;
    let detail = if dimensions[0] > 0 {
        format!(
            "{} × {} · Scroll or pinch to zoom · Drag to pan",
            dimensions[0], dimensions[1]
        )
    } else {
        String::new()
    };
    body.child(canvas).child(
        row()
            .w_full()
            .px(12.)
            .py(6.)
            .gap(8.)
            .child(text(detail).font(k.ty.small).color(k.c.text_muted).grow(1.))
            .children(document.original.as_ref().map(|original| {
                text(format!("{} KB", original.size.div_ceil(1024)))
                    .font(k.ty.small)
                    .color(k.c.text_muted)
            })),
    )
}
