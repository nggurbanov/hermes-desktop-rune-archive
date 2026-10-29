use crate::app::Viewer;
use rune::{El, FileDialogOptions};
use rune_kit::{Button, ButtonVariant, Kit};
use std::io::{Read, Write};
fn same(s: &Viewer, owner: &crate::composer::DraftScope, profile: &Option<String>) -> bool {
    let now = s.composer_scope();
    now.connection == owner.connection
        && now.profile == owner.profile
        && s.settings.scope_profile == *profile
}
pub(super) fn import(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let profile = s.settings.scope_profile.clone();
    Button::new("Import")
        .variant(ButtonVariant::Ghost)
        .disabled(s.settings.saving)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !same(s, &owner, &profile) {
                return;
            }
            let owner = owner.clone();
            let profile = profile.clone();
            tx.open_file(
                FileDialogOptions {
                    title: "Import Hermes settings".into(),
                    ..Default::default()
                }
                .filter("JSON settings", &["json"]),
                move |s: &mut Viewer, result, tx| {
                    if !same(s, &owner, &profile) {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            let value = (|| {
                                let mut bytes = Vec::new();
                                std::fs::File::open(path)
                                    .map_err(|_| "The settings file could not be opened")?
                                    .take(8 * 1024 * 1024 + 1)
                                    .read_to_end(&mut bytes)
                                    .map_err(|_| "The settings file could not be read")?;
                                if bytes.len() > 8 * 1024 * 1024 {
                                    return Err("The settings file is too large");
                                }
                                serde_json::from_slice(&bytes)
                                    .map_err(|_| "The settings file is not valid JSON")
                            })();
                            match value {
                                Ok(value) => {
                                    if let Err(error) = s.settings.import_value(value) {
                                        s.settings.error = Some(error);
                                    } else {
                                        tx.wake_after(std::time::Duration::from_millis(560));
                                    }
                                }
                                Err(error) => s.settings.error = Some(error.into()),
                            }
                        }
                        Ok(None) => {}
                        Err(_) => s.settings.error = Some("The file chooser is unavailable".into()),
                    }
                },
            );
        })
        .build(k)
        .key("settings-import")
}
pub(super) fn export(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let profile = s.settings.scope_profile.clone();
    Button::new("Export")
        .variant(ButtonVariant::Ghost)
        .disabled(s.settings.fields.is_empty())
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !same(s, &owner, &profile) {
                return;
            }
            let value = match s.settings.export_value() {
                Ok(value) => value,
                Err(error) => {
                    s.settings.error = Some(error);
                    return;
                }
            };
            let bytes = match serde_json::to_vec_pretty(&value) {
                Ok(bytes) => bytes,
                Err(_) => {
                    s.settings.error = Some("Settings could not be exported".into());
                    return;
                }
            };
            let owner = owner.clone();
            let profile = profile.clone();
            tx.save_file(
                FileDialogOptions {
                    title: "Export Hermes settings".into(),
                    file_name: Some("hermes-config.json".into()),
                    ..Default::default()
                }
                .filter("JSON settings", &["json"]),
                move |s: &mut Viewer, result, _| {
                    if !same(s, &owner, &profile) {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            let mut options = std::fs::OpenOptions::new();
                            options.write(true).create(true).truncate(true);
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::OpenOptionsExt;
                                options.mode(0o600);
                            }
                            let result = options.open(path).and_then(|mut f| f.write_all(&bytes));
                            if result.is_err() {
                                s.settings.error =
                                    Some("The settings file could not be saved".into());
                            } else {
                                s.settings.notice = Some("Settings exported".into());
                            }
                        }
                        Ok(None) => {}
                        Err(_) => s.settings.error = Some("The file chooser is unavailable".into()),
                    }
                },
            );
        })
        .build(k)
        .key("settings-export")
}
