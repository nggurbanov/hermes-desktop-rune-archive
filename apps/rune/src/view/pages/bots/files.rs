use super::owns;
use crate::{app::Viewer, pages::bots::Action};
use rune::{El, FileDialogOptions};
use rune_kit::{Button, ButtonVariant, Kit};
pub(super) fn avatar(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let epoch = s.bots.editor_epoch;
    Button::new("Upload avatar")
        .variant(ButtonVariant::Ghost)
        .disabled(s.bots.busy || s.bots.normalizing || s.bots.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !owns(s, &owner, epoch) {
                return;
            }
            let owner = owner.clone();
            tx.open_file(
                FileDialogOptions {
                    title: "Choose bot avatar".into(),
                    ..Default::default()
                }
                .filter("Avatar images", &["png", "jpg", "jpeg", "webp", "gif"]),
                move |s: &mut Viewer, result, tx| {
                    if !owns(s, &owner, epoch) {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            s.bots_action(Action::AvatarPath(path));
                            tx.instant();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            s.bots.error = Some("The avatar file chooser is unavailable".into())
                        }
                    }
                },
            );
        })
        .build(k)
}
pub(super) fn import(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let epoch = s.bots.editor_epoch;
    Button::new("Choose local archive")
        .variant(ButtonVariant::Ghost)
        .disabled(s.bots.busy || s.bots.demo || !s.is_local_connection())
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !owns(s, &owner, epoch) || !s.is_local_connection() {
                return;
            }
            let owner = owner.clone();
            tx.open_file(
                FileDialogOptions {
                    title: "Import Hermes profile".into(),
                    ..Default::default()
                }
                .filter("Hermes profile", &["tar.gz", "tgz", "gz"]),
                move |s: &mut Viewer, result, tx| {
                    if !owns(s, &owner, epoch) || !s.is_local_connection() {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            s.bots.archive_path.replace_text(path.to_string_lossy());
                            s.bots_action(Action::Import);
                            tx.instant();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            s.bots.error = Some("The archive file chooser is unavailable".into())
                        }
                    }
                },
            );
        })
        .build(k)
}
pub(super) fn export(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let epoch = s.bots.editor_epoch;
    let file = s.bots.selected.clone().unwrap_or_else(|| "profile".into());
    Button::new("Choose local save path")
        .variant(ButtonVariant::Ghost)
        .disabled(s.bots.busy || s.bots.demo || !s.is_local_connection())
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !owns(s, &owner, epoch) || !s.is_local_connection() {
                return;
            }
            let owner = owner.clone();
            tx.save_file(
                FileDialogOptions {
                    title: "Export Hermes profile".into(),
                    file_name: Some(format!("{file}.tar.gz")),
                    ..Default::default()
                }
                .filter("Hermes profile", &["tar.gz", "tgz"]),
                move |s: &mut Viewer, result, tx| {
                    if !owns(s, &owner, epoch) || !s.is_local_connection() {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            s.bots.export_path.replace_text(path.to_string_lossy());
                            s.bots_action(Action::Export);
                            tx.instant();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            s.bots.error = Some("The archive save chooser is unavailable".into())
                        }
                    }
                },
            );
        })
        .build(k)
}
pub(super) fn download_archive(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let epoch = s.bots.editor_epoch;
    let file = s.bots.selected.clone().unwrap_or_else(|| "profile".into());
    Button::new("Download archive to this device")
        .variant(ButtonVariant::Ghost)
        .disabled(s.bots.busy || s.bots.demo || s.bots.exported_archive.is_none())
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !owns(s, &owner, epoch) {
                return;
            }
            let owner = owner.clone();
            tx.save_file(
                FileDialogOptions {
                    title: "Save exported Hermes profile".into(),
                    file_name: Some(format!("{file}.tar.gz")),
                    ..Default::default()
                }
                .filter("Hermes profile", &["tar.gz", "tgz"]),
                move |s: &mut Viewer, result, tx| {
                    if !owns(s, &owner, epoch) {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            s.bots_action(Action::DownloadArchive(path));
                            tx.instant();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            s.bots.error = Some("The archive save chooser is unavailable".into())
                        }
                    }
                },
            );
        })
        .build(k)
}
