//! Opening a local desktop extension is an explicit user gesture, scoped at selection.
use super::*;
use rune::FileDialogOptions;
use std::io::Read;

fn same_owner(s: &Viewer, owner: &crate::composer::DraftScope, profile: &Option<String>) -> bool {
    let now = s.composer_scope();
    now.connection == owner.connection
        && now.profile == owner.profile
        && s.capabilities.profile == *profile
}
pub(super) fn open(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let profile = s.capabilities.profile.clone();
    Button::new("Open desktop plugin…")
        .variant(ButtonVariant::Ghost)
        .disabled(s.capabilities.busy() || s.capabilities.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !same_owner(s, &owner, &profile) { return; }
            let owner = owner.clone();
            let profile = profile.clone();
            tx.open_file(
                FileDialogOptions { title: "Open a trusted Hermes desktop plugin".into(), ..Default::default() }
                    .filter("Desktop plugin JavaScript", &["js", "mjs"]),
                move |s: &mut Viewer, result, tx| {
                    if !same_owner(s, &owner, &profile) { return; }
                    match result {
                        Ok(Some(path)) => {
                            let source = (|| {
                                let mut bytes = Vec::new();
                                std::fs::File::open(&path)
                                    .map_err(|_| "The desktop plugin file could not be opened")?
                                    .take(16 * 1024 * 1024 + 1)
                                    .read_to_end(&mut bytes)
                                    .map_err(|_| "The desktop plugin source could not be read")?;
                                if bytes.len() > 16 * 1024 * 1024 { return Err("The desktop plugin source exceeds the size limit"); }
                                String::from_utf8(bytes).map_err(|_| "The desktop plugin is not UTF-8 JavaScript")
                            })();
                            match source {
                                Ok(source) => {
                                    s.open_gui_plugin_for(profile.clone(), source, path.to_string_lossy().into_owned());
                                    tx.wake_after(std::time::Duration::from_millis(100));
                                }
                                Err(error) => s.capabilities.message = Some(error.into()),
                            }
                        }
                        Ok(None) => {}
                        Err(_) => s.capabilities.message = Some("The native file chooser is unavailable".into()),
                    }
                },
            );
        })
        .build(k)
        .key("capabilities-open-desktop-plugin")
        .help("Open trusted ESM JavaScript with the original Hermes React SDK in a separate compatibility window")
}

pub(super) fn installed(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let profile = s.capabilities.profile.clone();
    Button::new("Desktop plugins")
        .variant(ButtonVariant::Ghost)
        .disabled(s.capabilities.busy() || s.capabilities.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !same_owner(s, &owner, &profile) {
                return;
            }
            s.open_gui_plugin_for(
                profile.clone(),
                String::new(),
                "installed-desktop-plugins".into(),
            );
            tx.wake_after(std::time::Duration::from_millis(100));
        })
        .build(k)
        .key("capabilities-installed-desktop-plugins")
        .help(
            "Manage installed desktop extensions, their live UI, enable choices and source reloads",
        )
}

pub(super) fn bots(s: &Viewer, k: &Kit) -> El {
    let owner = s.composer_scope();
    let profile = s.capabilities.profile.clone();
    Button::new("Bot groups & Petdex")
        .variant(ButtonVariant::Ghost)
        .disabled(s.capabilities.busy() || s.capabilities.demo)
        .on_press_tx(move |s: &mut Viewer, tx| {
            if !same_owner(s, &owner, &profile) {
                return;
            }
            s.open_gui_plugin_for(
                profile.clone(),
                "builtin:hermes-bots".into(),
                "builtin:hermes-bots".into(),
            );
            tx.wake_after(std::time::Duration::from_millis(100));
        })
        .build(k)
        .key("capabilities-builtin-bots")
        .help("Open the original bundled Bot Mode, including groups, hidden bots and Petdex")
}
