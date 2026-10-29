//! Image deliveries keep their gallery; files and links retain location and source-conversation columns.
mod detail;
use crate::{
    app::Viewer,
    pages::artifacts::{Action, Artifact},
};
use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, TextInput};
fn note(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.small).color(k.c.text_muted).wrap()
}
fn btn(s: &Viewer, label: impl Into<String>, action: Action, disabled: bool, k: &Kit) -> El {
    let owner = s.composer_scope();
    let item = match action {
        Action::Select(i) | Action::OpenChat(i) | Action::RetryThumbnail(i) => {
            s.artifacts.rows.get(i).map(|a| (i, a.key()))
        }
        Action::NativeOpen | Action::LoadFile => s
            .artifacts
            .selected
            .and_then(|i| s.artifacts.rows.get(i).map(|a| (i, a.key()))),
        _ => None,
    };
    let live = matches!(
        action,
        Action::Refresh
            | Action::RetryFailed
            | Action::RetrySession(_)
            | Action::RetryThumbnail(_)
            | Action::LoadFile
            | Action::NativeOpen
    );
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(disabled || s.artifacts.demo && live)
        .on_press_tx(move |s: &mut Viewer, tx| {
            let now = s.composer_scope();
            if now.connection != owner.connection || now.profile != owner.profile {
                return;
            }
            if item
                .as_ref()
                .is_some_and(|(i, key)| s.artifacts.rows.get(*i).is_none_or(|a| a.key() != *key))
            {
                return;
            }
            s.artifacts_action(action.clone());
            tx.instant();
        })
        .build(k)
}
fn pagination(s: &Viewer, kind: &str, total: usize, size: usize, page: usize, k: &Kit) -> El {
    let count = total.div_ceil(size).max(1);
    let page = page.min(count - 1);
    let action = |p| {
        if kind == "Images" {
            Action::ImagePage(p)
        } else {
            Action::FilePage(p)
        }
    };
    let mut bar = row()
        .w_full()
        .gap(6.0)
        .center_y()
        .child(note(
            format!(
                "{kind} · {}–{} of {total}",
                if total == 0 { 0 } else { page * size + 1 },
                ((page + 1) * size).min(total)
            ),
            k,
        ))
        .child(el().grow(1.0));
    if count > 1 {
        bar = bar.child(btn(
            s,
            "Previous",
            action(page.saturating_sub(1)),
            page == 0,
            k,
        ));
        for n in 0..count {
            if count <= 7 || n == 0 || n + 1 == count || n.abs_diff(page) <= 1 {
                bar = bar.child(btn(s, format!("{}", n + 1), action(n), false, k).fill(
                    if n == page {
                        k.c.layer_01
                    } else {
                        rune::Color::TRANSPARENT
                    },
                ));
            }
        }
        bar = bar.child(btn(s, "Next", action(page + 1), page + 1 >= count, k));
    }
    bar
}
pub fn page(s: &Viewer, k: &Kit, _: &mut Vec<El>) -> El {
    let state = &s.artifacts;
    let visible = state.visible();
    let images: Vec<_> = visible
        .iter()
        .copied()
        .filter(|&i| state.rows[i].kind == "image")
        .collect();
    let files: Vec<_> = visible
        .iter()
        .copied()
        .filter(|&i| state.rows[i].kind != "image")
        .collect();
    let owner = s.composer_scope();
    let mut controls = row().w_full().flex_wrap().gap(6.0).center_y().child(
        TextInput::new(&state.search)
            .placeholder("Search names, locations and conversations…")
            .width(None)
            .on_edit(|s: &mut Viewer| &mut s.artifacts.search)
            .on_change(move |s: &mut Viewer, _| {
                let now = s.composer_scope();
                if now.connection == owner.connection && now.profile == owner.profile {
                    s.artifacts_action(Action::SearchChanged);
                }
            })
            .build(k)
            .min_w(240.0)
            .grow(1.0),
    );
    for (id, label) in [
        ("all", "All"),
        ("image", "Images"),
        ("file", "Files"),
        ("link", "Links"),
    ] {
        let count = state
            .rows
            .iter()
            .filter(|a| id == "all" || a.kind == id)
            .count();
        controls = controls.child(
            btn(
                s,
                format!("{label} {count}"),
                Action::Filter(id.into()),
                false,
                k,
            )
            .key(format!("artifact-filter-{id}"))
            .fill(if state.filter == id {
                k.c.layer_01
            } else {
                rune::Color::TRANSPARENT
            }),
        );
    }
    controls = controls.child(btn(
        s,
        if state.loading {
            "Indexing…"
        } else {
            "Refresh"
        },
        Action::Refresh,
        state.loading || state.saving,
        k,
    ));
    let mut gallery = col().w_full().gap(12.0);
    if !images.is_empty() {
        gallery = gallery.child(pagination(
            s,
            "Images",
            images.len(),
            24,
            state.image_page,
            k,
        ));
        let mut grid = row().w_full().flex_wrap().gap(12.0);
        for &i in images
            .iter()
            .skip(
                state
                    .image_page
                    .min(images.len().div_ceil(24).saturating_sub(1))
                    * 24,
            )
            .take(24)
        {
            grid = grid.child(image_card(s, i, &state.rows[i], k));
        }
        gallery = gallery.child(grid);
    }
    if !files.is_empty() {
        gallery = gallery.child(pagination(
            s,
            if state.filter == "link" {
                "Links"
            } else {
                "Files and links"
            },
            files.len(),
            100,
            state.file_page,
            k,
        ));
        let mut table = col().w_full().min_w(660.0).gap(1.0).child(
            row()
                .w_full()
                .px(10.0)
                .py(8.0)
                .gap(12.0)
                .fill(k.c.layer_01)
                .child(note("NAME", k).grow(0.32))
                .child(note("LOCATION", k).grow(0.42))
                .child(note("CONVERSATION", k).grow(0.26)),
        );
        for &i in files
            .iter()
            .skip(
                state
                    .file_page
                    .min(files.len().div_ceil(100).saturating_sub(1))
                    * 100,
            )
            .take(100)
        {
            table = table.child(file_row(s, i, &state.rows[i], k));
        }
        gallery = gallery.child(col().w_full().scroll_x().child(table));
    }
    if visible.is_empty() {
        gallery = gallery.child(
            col()
                .w_full()
                .py(48.0)
                .gap(8.0)
                .child(
                    text(if state.loading {
                        "Indexing recent conversations…"
                    } else {
                        "No artifacts match"
                    })
                    .font(k.ty.body_medium)
                    .color(k.c.text_base),
                )
                .child(note(
                    if state.loading {
                        "Successful conversations appear after their full history has been read."
                    } else {
                        "Search by file name, URL, path or the conversation that produced it."
                    },
                    k,
                )),
        );
    }
    let mut content = row().w_full().grow(1.0).min_h(0.0).gap(18.0).child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(gallery),
    );
    if let Some(a) = state.selected.and_then(|i| state.rows.get(i)) {
        content = content
            .child(el().w(1.0).h_full().fill(k.c.border_muted))
            .child(detail::panel(s, a, k));
    }
    let mut page = col()
        .key("artifacts-page")
        .w_full()
        .h_full()
        .min_h(0.0)
        .px(24.0)
        .py(20.0)
        .gap(14.0)
        .child(
            text("Artifacts")
                .font(k.ty.body_medium)
                .color(k.c.text_base),
        )
        .child(controls);
    if state.demo {
        page = page.child(note(
            "Demonstration data · live gateway reads and file operations are disabled",
            k,
        ));
    }
    if let Some(e) = &state.error {
        page = page.child(
            text(e)
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    if let Some(n) = &state.notice {
        page = page.child(note(n, k));
    }
    if !state.failed_sessions.is_empty() {
        let mut failures=col().w_full().gap(4.0).child(row().gap(8.0).child(note(format!("{} conversations could not be indexed. Their partial results were skipped.",state.failures),k).grow(1.0)).child(btn(s,"Retry failed conversations",Action::RetryFailed,state.loading,k)));
        for (i, (session, error)) in state.failed_sessions.iter().enumerate() {
            failures = failures.child(
                row()
                    .w_full()
                    .gap(8.0)
                    .child(
                        note(
                            format!(
                                "{} · {} · {error}",
                                session["profile"].as_str().unwrap_or("default"),
                                session["title"]
                                    .as_str()
                                    .or(session["id"].as_str())
                                    .unwrap_or("Untitled")
                            ),
                            k,
                        )
                        .grow(1.0),
                    )
                    .child(btn(s, "Retry", Action::RetrySession(i), state.loading, k)),
            );
        }
        page = page.child(failures.max_h(160.0).scroll(ScrollSpec {
            stick_bottom: false,
        }));
    }
    page.child(content)
}
fn image_card(s: &Viewer, i: usize, a: &Artifact, k: &Kit) -> El {
    let thumb = s.artifacts.thumbnails.get(&a.key());
    let mut media = col().w_full().h(158.0).center().fill(k.c.layer_01);
    if let Some(image) = thumb
        .and_then(|t| t.image.as_ref())
        .filter(|i| i.error().is_none())
    {
        media = media.child(
            el().w_full()
                .h_full()
                .pixels_fit(image.pixels(), rune::SurfaceFit::Fit),
        );
    } else {
        let error = thumb.and_then(|t| t.error.clone()).or_else(|| {
            thumb
                .and_then(|t| t.image.as_ref())
                .and_then(|i| i.error())
                .map(|e| e.to_string())
        });
        media = media.child(note(
            error.clone().unwrap_or_else(|| {
                if thumb.is_some_and(|t| t.loading) {
                    "Loading image…"
                } else {
                    "Image preview"
                }
                .into()
            }),
            k,
        ));
        if error.is_some() {
            media = media.child(btn(s, "Retry image", Action::RetryThumbnail(i), false, k));
        }
    }
    col()
        .key(format!("artifact-{}", a.key()))
        .w(238.0)
        .grow(1.0)
        .max_w(420.0)
        .min_w(190.0)
        .gap(7.0)
        .border(1.0, k.c.border_muted)
        .radius(8.0)
        .child(media)
        .child(
            col()
                .w_full()
                .px(10.0)
                .pb(10.0)
                .gap(4.0)
                .child(
                    btn(s, &a.label, Action::Select(i), false, k)
                        .key(format!("artifact-select-{i}"))
                        .w_full()
                        .justify_start(),
                )
                .child(note(&a.value, k))
                .child(note(format!("{} · {}", a.profile, a.time()), k))
                .child(btn(s, &a.title, Action::OpenChat(i), false, k).justify_start()),
        )
}
fn file_row(s: &Viewer, i: usize, a: &Artifact, k: &Kit) -> El {
    let copy = a.value.clone();
    row()
        .key(format!("artifact-{}", a.key()))
        .w_full()
        .gap(12.0)
        .px(10.0)
        .py(6.0)
        .center_y()
        .fill(if s.artifacts.selected == Some(i) {
            k.c.layer_01
        } else {
            rune::Color::TRANSPARENT
        })
        .child(
            btn(s, &a.label, Action::Select(i), false, k)
                .grow(0.32)
                .min_w(0.0)
                .justify_start(),
        )
        .child(
            row()
                .grow(0.42)
                .min_w(0.0)
                .gap(4.0)
                .child(note(&a.value, k).grow(1.0).min_w(0.0))
                .child(
                    Button::new("Copy")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press_tx(move |_: &mut Viewer, tx| tx.copy(copy.clone()))
                        .build(k),
                ),
        )
        .child(
            col()
                .grow(0.26)
                .min_w(0.0)
                .gap(2.0)
                .child(btn(s, &a.title, Action::OpenChat(i), false, k).justify_start())
                .child(note(format!("{} · {}", a.profile, a.time()), k)),
        )
}
