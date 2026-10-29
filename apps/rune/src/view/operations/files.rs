use super::*;
use crate::gateway::sidebar::SidebarSession;
use rune::FileDialogOptions;
pub(super) fn export_session(s: &Viewer, session: SidebarSession, k: &Kit) -> El {
    let owner = s.composer_scope();
    let file = session
        .id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .take(40)
        .collect::<String>();
    Button::new("Export")
        .variant(ButtonVariant::Ghost)
        .disabled(
            s.operations.exporting
                || s.operations.saving_file
                || s.operations.demo
                || session.connection_id.is_some(),
        )
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !owns(s, &owner) {
                return;
            }
            let owner = owner.clone();
            let session = session.clone();
            tx.save_file(
                FileDialogOptions {
                    title: "Export conversation".into(),
                    file_name: Some(format!("session-{file}.json")),
                    ..Default::default()
                }
                .filter("JSON", &["json"]),
                move |s: &mut Viewer, result, tx| {
                    if !owns(s, &owner) {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            s.operations_action(Action::Export(session.clone(), path));
                            tx.instant();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            s.operations.export_error =
                                Some("The native save dialog is unavailable".into())
                        }
                    }
                },
            );
        })
        .build(k)
}
pub(super) fn save_logs(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let file = s.operations.log_file.clone();
    Button::new("Export visible logs")
        .variant(ButtonVariant::Ghost)
        .disabled(s.operations.saving_file || s.operations.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !owns(s, &owner) {
                return;
            }
            let owner = owner.clone();
            tx.save_file(
                FileDialogOptions {
                    title: "Export log tail".into(),
                    file_name: Some(format!("hermes-{file}.log")),
                    ..Default::default()
                }
                .filter("Log", &["log", "txt"]),
                move |s: &mut Viewer, result, tx| {
                    if !owns(s, &owner) {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            s.operations_action(Action::SaveLogs(path));
                            tx.instant();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            s.operations.export_error =
                                Some("The native save dialog is unavailable".into())
                        }
                    }
                },
            );
        })
        .build(k)
}
pub(super) fn download_backup(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    Button::new("Download backup")
        .variant(ButtonVariant::Ghost)
        .disabled(s.operations.downloading || s.operations.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !owns(s, &owner) {
                return;
            }
            let owner = owner.clone();
            tx.save_file(
                FileDialogOptions {
                    title: "Download profile backup".into(),
                    file_name: Some("hermes-backup.tar.gz".into()),
                    ..Default::default()
                }
                .filter("Backup", &["gz", "tgz"]),
                move |s: &mut Viewer, result, tx| {
                    if !owns(s, &owner) {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            s.operations_action(Action::DownloadBackup(path));
                            tx.instant();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            s.operations.export_error =
                                Some("The native save dialog is unavailable".into())
                        }
                    }
                },
            );
        })
        .build(k)
}
