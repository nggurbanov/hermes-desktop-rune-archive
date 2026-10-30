//! Saved agent destinations and a typed, private connection editor.
use crate::{
    app::Viewer,
    connections::{Action, Editor, Kind, LaunchMode},
};
use rune::{El, ScrollSpec, TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, Kit, Switch, TextInput};
fn action(
    label: impl Into<String>,
    key: impl Into<String>,
    action: Action,
    busy: bool,
    k: &Kit,
) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .disabled(busy)
        .on_press_tx(move |s: &mut Viewer, tx| {
            s.connections_action(action.clone());
            tx.instant();
        })
        .build(k)
        .key(key)
}
fn note(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.small).color(k.c.text_muted).wrap()
}
fn input(
    s: &Viewer,
    key: &str,
    label: &str,
    placeholder: &str,
    read: fn(&Editor) -> &TextEdit,
    write: fn(&mut Editor) -> &mut TextEdit,
    secure: bool,
    k: &Kit,
) -> El {
    let editor = s
        .connections
        .editor
        .as_ref()
        .expect("mounted connection editor");
    let field = TextInput::new(read(editor))
        .placeholder(placeholder)
        .width(None)
        .on_edit(move |s: &mut Viewer| {
            write(
                s.connections
                    .editor
                    .as_mut()
                    .expect("mounted connection editor"),
            )
        });
    let field = if secure { field.secure() } else { field };
    col()
        .w_full()
        .gap(6.)
        .child(text(label).font(k.ty.ui_medium).color(k.c.text_base))
        .child(field.build(k).key(key).label(label))
}
pub fn panel(s: &Viewer, k: &Kit, _layer: &mut Vec<El>) -> El {
    let state = &s.connections;
    let busy = state.busy;
    let policy_disabled =
        busy || state.editor.is_some() || s.is_fixture() || !state.registry_loaded();
    let mut destinations = col()
        .w(300.)
        .min_h(0.)
        .gap(12.)
        .child(
            text("Your Hermes agents")
                .font(k.ty.body_medium)
                .color(k.c.text_base),
        )
        .child(note(
            "Connect to an existing agent on this device, a remote gateway or through SSH.",
            k,
        ));
    for row_data in &state.rows {
        let id = row_data.id.clone();
        let detail = row_data.summary();
        let primary = state.primary() == Some(id.as_str());
        destinations = destinations.child(
            col()
                .w_full()
                .gap(3.)
                .child(
                    action(
                        format!(
                            "{}{}{}",
                            row_data.label,
                            if state.selected.as_deref() == Some(&id) {
                                " · selected"
                            } else {
                                ""
                            },
                            if primary { " · primary" } else { "" }
                        ),
                        format!("saved-connect-{id}"),
                        Action::Select(id.clone()),
                        busy,
                        k,
                    )
                    .w_full()
                    .justify_start(),
                )
                .child(note(detail, k))
                .child(
                    row()
                        .gap(6.)
                        .child(action(
                            "Edit",
                            format!("saved-edit-{id}"),
                            Action::Edit(id.clone()),
                            busy,
                            k,
                        ))
                        .child(action(
                            "Forget",
                            format!("saved-forget-{id}"),
                            Action::AskForget(id.clone()),
                            busy,
                            k,
                        ))
                        .child(action(
                            if primary { "Primary" } else { "Make primary" },
                            format!("saved-primary-{id}"),
                            Action::MakePrimary(id.clone()),
                            policy_disabled || primary,
                            k,
                        )),
                )
                .pb(12.),
        );
    }
    if state.rows.is_empty() {
        destinations = destinations.child(note("No saved connections yet", k));
    }
    destinations = destinations.child(
        row()
            .gap(8.)
            .child(action(
                "Add connection",
                "saved-add",
                Action::Add(Kind::Local),
                busy,
                k,
            ))
            .child(action(
                "Reload",
                "saved-reload",
                Action::Reload,
                busy || state.editor.is_some(),
                k,
            )),
    );
    let last_used = state.launch_mode() == LaunchMode::LastUsed;
    destinations = destinations.child(
        col()
            .w_full()
            .pt(10.)
            .gap(10.)
            .child(
                text("App startup")
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
            )
            .child(
                Switch::new(last_used)
                    .label("Open last used agent")
                    .disabled(policy_disabled)
                    .on_toggle_tx(move |s: &mut Viewer, tx| {
                        s.connections_action(Action::LaunchMode(if last_used {
                            LaunchMode::Primary
                        } else {
                            LaunchMode::LastUsed
                        }));
                        tx.instant();
                    })
                    .build(k)
                    .key("startup-last-used"),
            )
            .child(note(
                "When off, opens the primary agent. A default profile takes priority.",
                k,
            )),
    );
    let device_default = s.default_profile_route();
    let default_error = s.device_default_error();
    let summary = match &device_default {
        Some(route) => {
            let source = match route.connection_id.as_deref() {
                None => "Profile connection",
                Some("local") => state
                    .rows
                    .iter()
                    .find(|row| row.kind() == Kind::Local)
                    .map(|row| row.label.as_str())
                    .unwrap_or("This device"),
                Some(id) => state
                    .rows
                    .iter()
                    .find(|row| row.id == id)
                    .map(|row| row.label.as_str())
                    .unwrap_or("Saved agent unavailable"),
            };
            format!("{} · {source}", route.profile)
        }
        None if default_error.is_some() => "Default profile unavailable".into(),
        None => "No default profile".into(),
    };
    let mut default = col()
        .w_full()
        .pt(10.)
        .gap(8.)
        .child(
            text("Default profile")
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        )
        .child(note(summary, k).key("device-default-summary"))
        .child(note("Used for app startup and new conversations.", k));
    if device_default.is_some() {
        default = default.child(
            Button::new("Clear default")
                .variant(ButtonVariant::Ghost)
                .disabled(policy_disabled)
                .on_press_tx(|s: &mut Viewer, tx| {
                    s.clear_default_profile();
                    tx.instant();
                })
                .build(k)
                .key("device-default-clear"),
        );
    }
    if let Some(error) = default_error {
        default = default.child(
            text(error)
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap()
                .role("alert")
                .key("device-default-error"),
        );
    }
    destinations = destinations.child(default);
    if !state.registry_loaded() && !s.is_fixture() {
        destinations =
            destinations.child(note("Reload connections to change startup preferences.", k));
    }
    if let Some(id) = &state.forget {
        let name = state
            .rows
            .iter()
            .find(|r| &r.id == id)
            .map(|r| r.label.as_str())
            .unwrap_or("this connection");
        destinations=destinations.child(col().gap(10.).px(12.).py(12.).child(text(format!("Forget {name}?" )).font(k.ty.ui_medium).color(k.c.text_base)).child(note("This removes its saved address and OS keychain credential. The agent remains on its host.",k)).child(row().gap(8.).child(action("Forget connection","saved-forget-confirm",Action::ConfirmForget,busy,k)).child(action("Keep","saved-forget-cancel",Action::CancelForget,busy,k))));
    }
    let mut detail = col().grow(1.).min_w(0.).max_w(620.).gap(18.);
    if let Some(editor) = &state.editor {
        detail = detail.child(
            text(if editor.id.is_some() {
                "Edit connection"
            } else {
                "Add a connection"
            })
            .font(k.ty.body_medium)
            .color(k.c.text_base),
        );
        let mut kinds = row().gap(6.);
        for (kind, name) in [
            (Kind::Local, "This device"),
            (Kind::Remote, "Remote URL"),
            (Kind::Ssh, "SSH tunnel"),
        ] {
            kinds = kinds.child(
                Button::new(name)
                    .variant(if kind == editor.kind {
                        ButtonVariant::Contrast
                    } else {
                        ButtonVariant::Ghost
                    })
                    .disabled(busy)
                    .on_press(move |s: &mut Viewer| s.connections_action(Action::Kind(kind)))
                    .build(k)
                    .key(format!("saved-kind-{name}")),
            );
        }
        detail = detail.child(kinds);
        detail = detail.child(input(
            s,
            "saved-label",
            "Name",
            "Personal Hermes",
            |e| &e.label,
            |e| &mut e.label,
            false,
            k,
        ));
        match editor.kind {
            Kind::Local => {
                detail = detail
                    .child(input(
                        s,
                        "saved-home",
                        "Agent home",
                        "Absolute directory of the existing agent",
                        |e| &e.home,
                        |e| &mut e.home,
                        false,
                        k,
                    ))
                    .child(note(
                        "Uses the running gateway's private local attachment.",
                        k,
                    ));
            }
            Kind::Remote => {
                detail = detail.child(input(
                    s,
                    "saved-url",
                    "Gateway URL",
                    "https://agent.example.com",
                    |e| &e.url,
                    |e| &mut e.url,
                    false,
                    k,
                ));
            }
            Kind::Ssh => {
                detail=detail.child(note("Uses your OpenSSH agent/config and known_hosts to forward an existing gateway's HTTP port.",k)).child(input(s,"saved-ssh-host","SSH host","Hostname, IP address or SSH config alias",|e|&e.ssh_host,|e|&mut e.ssh_host,false,k)).child(input(s,"saved-ssh-user","SSH user","Leave blank to use SSH config",|e|&e.ssh_user,|e|&mut e.ssh_user,false,k)).child(row().gap(12.).child(input(s,"saved-ssh-port","SSH port","22",|e|&e.ssh_port,|e|&mut e.ssh_port,false,k).grow(1.)).child(input(s,"saved-remote-port","Gateway port on agent host","9119",|e|&e.remote_port,|e|&mut e.remote_port,false,k).grow(1.))).child(input(s,"saved-ssh-key","Private key file (optional)","Absolute path; blank uses SSH agent/config",|e|&e.key_file,|e|&mut e.key_file,false,k)).child(input(s,"saved-ssh-path","Gateway path prefix (optional)","/hermes",|e|&e.ssh_path,|e|&mut e.ssh_path,false,k));
            }
        }
        if editor.kind != Kind::Local {
            detail = detail
                .child(input(
                    s,
                    "saved-token",
                    "Gateway token",
                    if editor.had_token {
                        "Leave blank to keep the saved token"
                    } else {
                        "Optional for gateways without token authentication"
                    },
                    |e| &e.token,
                    |e| &mut e.token,
                    true,
                    k,
                ))
                .child(note(
                    "Tokens are stored in the operating system's credential store.",
                    k,
                ));
            if editor.had_token {
                detail = detail.child(
                    Checkbox::new(editor.clear_token)
                        .label("Remove the saved token")
                        .on_toggle(|s: &mut Viewer| {
                            if let Some(editor) = &mut s.connections.editor {
                                editor.clear_token = !editor.clear_token;
                            }
                        })
                        .build(k)
                        .key("saved-clear-token"),
                );
            }
        }
        if editor.kind != Kind::Ssh {
            detail = detail.child(action(
                "Connect without saving",
                "saved-connect-once",
                Action::ConnectOnce,
                busy,
                k,
            ));
        }
        detail = detail.child(
            row()
                .gap(8.)
                .child(
                    Button::new(if busy { "Saving…" } else { "Save connection" })
                        .variant(ButtonVariant::Contrast)
                        .disabled(busy)
                        .on_press(|s: &mut Viewer| s.connections_action(Action::Save))
                        .build(k)
                        .key("saved-save"),
                )
                .child(action(
                    "Cancel",
                    "saved-edit-cancel",
                    Action::Cancel,
                    busy,
                    k,
                )),
        );
    } else {
        detail=detail.child(text("Choose where Hermes runs").font(k.ty.body_medium).color(k.c.text_base)).child(note("Saved connections keep their own profiles, conversations and credentials. Select an agent to connect, or add another destination.",k)).child(action("Add a local agent","saved-add-local",Action::Add(Kind::Local),busy,k)).child(action("Add a remote gateway","saved-add-remote",Action::Add(Kind::Remote),busy,k)).child(action("Connect through SSH","saved-add-ssh",Action::Add(Kind::Ssh),busy,k));
    }
    if let Some(error) = &state.error {
        detail = detail.child(
            text(error)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap()
                .role("alert")
                .key("saved-error"),
        );
    }
    if let Some(notice) = &state.notice {
        detail = detail.child(note(notice, k).key("saved-notice"));
    }
    if busy {
        detail = detail.child(note("Connecting…", k));
    }
    row()
        .w_full()
        .h_full()
        .min_h(0.)
        .gap(32.)
        .px(28.)
        .py(28.)
        .child(destinations.scroll(ScrollSpec {
            stick_bottom: false,
        }))
        .child(detail.scroll(ScrollSpec {
            stick_bottom: false,
        }))
}
