use super::*;
use rune::FileDialogOptions;
fn copy_button(label: &str, value: String, disabled: bool, k: &Kit) -> El {
    native_button(label)
        .disabled(disabled)
        .on_press_tx(move |_: &mut Viewer, tx| tx.copy(value.clone()))
        .build(k)
        .shrink()
        .max_w(260.0)
}
pub(super) fn actions(s: &Viewer, file: &File, k: &Kit) -> El {
    let r = &s.review;
    let path = file.path.clone();
    let root = r
        .status
        .as_ref()
        .and_then(|s| s["repoRoot"].as_str())
        .unwrap_or(&r.root);
    let full = format!("{}/{}", root.trim_end_matches(['/', '\\']), path);
    let copy = full.clone();
    let relative = path.clone();
    let diff = r.diff.clone();
    let mut buttons = col()
        .w_full()
        .gap(4.0)
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(5.0)
                .child(action(
                    s,
                    "Open file",
                    Action::OpenFile(path.clone(), false),
                    r.busy,
                    k,
                ))
                .child(action(
                    s,
                    "Reveal in sidebar",
                    Action::OpenFile(path.clone(), true),
                    r.busy,
                    k,
                )),
        )
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(5.0)
                .child(copy_button("Copy full path", copy, false, k))
                .child(copy_button("Copy relative path", relative, false, k))
                .child(copy_button("Copy diff", diff, r.diff.is_empty(), k)),
        );
    if r.local {
        let owner = r.owner.clone();
        let epoch = r.revision();
        let path = std::path::PathBuf::from(full);
        buttons = buttons.child(
            native_button("Reveal in file manager")
                .disabled(r.demo)
                .on_press_tx(move |s: &mut Viewer, tx| {
                    if !owns(s, &owner, epoch) || !s.review.local {
                        return;
                    }
                    let target = if path.is_dir() {
                        Some(path.as_path())
                    } else {
                        path.parent()
                    };
                    if let Some(target) = target {
                        if let Ok(url) = url::Url::from_file_path(target) {
                            if tx.open_url(url.as_str()).is_err() {
                                s.review.error =
                                    Some("The file manager could not be opened".into());
                            }
                        }
                    }
                })
                .build(k)
                .shrink()
                .max_w(260.0),
        );
    } else {
        let owner = r.owner.clone();
        let epoch = r.revision();
        let file = file.path.clone();
        let name = file.rsplit('/').next().unwrap_or("file").to_owned();
        buttons = buttons.child(
            native_button("Download remote file")
                .disabled(r.demo || r.downloading)
                .on_press_tx(move |s: &mut Viewer, tx| {
                    if !owns(s, &owner, epoch) {
                        return;
                    }
                    let owner = owner.clone();
                    let file = file.clone();
                    tx.save_file(
                        FileDialogOptions {
                            title: "Download reviewed file".into(),
                            file_name: Some(name.clone()),
                            ..Default::default()
                        },
                        move |s: &mut Viewer, result, tx| {
                            if !owns(s, &owner, epoch) {
                                return;
                            }
                            match result {
                                Ok(Some(path)) => {
                                    s.review_action(Action::Download(file.clone(), path));
                                    tx.instant();
                                }
                                Ok(None) => {}
                                Err(_) => {
                                    s.review.error =
                                        Some("The native save dialog is unavailable".into())
                                }
                            }
                        },
                    );
                })
                .build(k)
                .shrink()
                .max_w(260.0),
        );
    }
    buttons
}
