//! Legacy routing form, built from the same Rune/OpenCode controls as Settings.
use crate::{
    connections::legacy::{AuthMode, Scope},
    legacy_settings::{Action, Change, Field, FormMode, SecretChoice, Settings},
};
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, Kit, TextInput};

pub(crate) trait Host: 'static {
    fn legacy_settings(&self) -> &Settings;
    fn legacy_settings_mut(&mut self) -> &mut Settings;
    fn legacy_settings_action(&mut self, action: Action);
    fn legacy_settings_connect(&mut self, generation: u64);
}
fn note(value: impl Into<String>, k: &Kit) -> El {
    text(value).font(k.ty.small).color(k.c.text_muted).wrap()
}
fn action<H: Host>(
    label: &str,
    key: impl Into<String>,
    action: Action,
    disabled: bool,
    selected: bool,
    k: &Kit,
) -> El {
    Button::new(label)
        .variant(if selected {
            ButtonVariant::Neutral
        } else {
            ButtonVariant::Ghost
        })
        .disabled(disabled)
        .on_press_tx(move |host: &mut H, tx| {
            host.legacy_settings_action(action.clone());
            tx.instant();
        })
        .build(k)
        .key(key)
}
fn owned<H: Host>(
    host: &H,
    label: &str,
    key: impl Into<String>,
    change: Change,
    disabled: bool,
    selected: bool,
    k: &Kit,
) -> El {
    let state = host.legacy_settings();
    let generation = state
        .form
        .as_ref()
        .map(|f| f.generation)
        .unwrap_or_default();
    action::<H>(
        label,
        key,
        Action::Form { generation, change },
        disabled || state.pending() || state.loading(),
        selected,
        k,
    )
}
fn field<H: Host>(
    host: &H,
    field: Field,
    label: &str,
    placeholder: &str,
    secure: bool,
    disabled: bool,
    k: &Kit,
) -> El {
    let state = host.legacy_settings();
    let form = state.form.as_ref().unwrap();
    let generation = form.generation;
    let mut input = TextInput::new(form.field(field).unwrap())
        .placeholder(placeholder)
        .width(None)
        .disabled(disabled || state.pending() || state.loading())
        .on_edit(move |host: &mut H| host.legacy_settings_mut().edit_mut(generation, field));
    if secure {
        input = input.secure();
    }
    col()
        .w_full()
        .min_w(0.)
        .gap(6.)
        .child(text(label).font(k.ty.ui_medium).color(k.c.text_base))
        .child(
            input
                .build(k)
                .label(label)
                .key(format!("legacy-field-{}", field_key(field))),
        )
}
fn field_key(field: Field) -> String {
    match field {
        Field::Url => "url".into(),
        Field::RemoteProfile => "remote-profile".into(),
        Field::Org => "org".into(),
        Field::CloudName => "cloud-name".into(),
        Field::Token => "token".into(),
        Field::Host => "ssh-host".into(),
        Field::User => "ssh-user".into(),
        Field::Port => "ssh-port".into(),
        Field::KeyPath => "ssh-key-path".into(),
        Field::HermesPath => "ssh-hermes-path".into(),
        Field::SshProfile => "ssh-profile".into(),
        Field::HeaderName(id) => format!("header-name-{id}"),
        Field::HeaderValue(id) => format!("header-value-{id}"),
    }
}
fn secret_choices<H: Host>(
    host: &H,
    key: &str,
    current: SecretChoice,
    change: impl Fn(SecretChoice) -> Change,
    k: &Kit,
) -> El {
    let mut choices = row().gap(6.);
    for (name, choice) in [
        ("Keep", SecretChoice::Keep),
        ("Set", SecretChoice::Set),
        ("Clear", SecretChoice::Clear),
    ] {
        choices = choices.child(owned(
            host,
            name,
            format!("legacy-{key}-{}", name.to_ascii_lowercase()),
            change(choice),
            false,
            current == choice,
            k,
        ));
    }
    choices
}
pub(crate) fn panel<H: Host>(host: &H, k: &Kit, _layer: &mut Vec<El>) -> El {
    let state = host.legacy_settings();
    let busy = state.pending() || state.loading();
    let global = matches!(state.scope, Scope::Global);
    let mut body = col()
        .key("legacy-routing-settings")
        .w_full()
        .h_full()
        .min_w(0.)
        .min_h(0.)
        .padding(28.)
        .gap(20.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    body = body
        .child(
            text("Legacy routing")
                .font(k.ty.body_medium)
                .color(k.c.text_base),
        )
        .child(note(
            "Choose the global connection or override a single desktop profile.",
            k,
        ));
    body = body.child(
        row()
            .gap(8.)
            .child(action::<H>(
                "Global",
                "legacy-scope-global",
                Action::Scope(Scope::Global),
                busy,
                global,
                k,
            ))
            .child(action::<H>(
                "Profile",
                "legacy-scope-profile",
                Action::EditProfile,
                busy || state.profile_name.text.trim().is_empty(),
                !global,
                k,
            )),
    );
    let generation = state.generation();
    body = body.child(
        col()
            .w_full()
            .gap(6.)
            .child(
                text("Desktop profile")
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
            )
            .child(
                row()
                    .gap(8.)
                    .child(
                        TextInput::new(&state.profile_name)
                            .placeholder("Profile name")
                            .width(None)
                            .disabled(busy)
                            .on_edit(move |host: &mut H| {
                                host.legacy_settings_mut().profile_edit_mut(generation)
                            })
                            .build(k)
                            .label("Desktop profile name")
                            .key("legacy-profile-name")
                            .grow(1.)
                            .min_w(0.),
                    )
                    .child(action::<H>(
                        "Edit profile",
                        "legacy-profile-open",
                        Action::EditProfile,
                        busy || state.profile_name.text.trim().is_empty(),
                        false,
                        k,
                    )),
            ),
    );
    let mut profiles = col().w_full().gap(4.);
    for name in state.profile_names() {
        let selected = state.scope == Scope::Profile(name.clone());
        profiles = profiles.child(action::<H>(
            &name,
            format!("legacy-profile-{name}"),
            Action::Scope(Scope::Profile(name.clone())),
            busy,
            selected,
            k,
        ));
    }
    body = body.child(profiles);
    if let Some(error) = &state.error {
        body = body.child(
            text(error)
                .font(k.ty.ui)
                .color(k.c.danger.fg)
                .wrap()
                .role("alert")
                .key("legacy-error"),
        );
    }
    if let Some(notice) = &state.notice {
        body = body.child(note(notice, k).key("legacy-notice"));
    }
    if let Some(form) = &state.form {
        let title = match &state.scope {
            Scope::Global => "Global connection".to_owned(),
            Scope::Profile(profile) => format!("Profile: {profile}"),
        };
        body = body.child(text(title).font(k.ty.ui_medium).color(k.c.text_base));
        let mut modes = row().gap(8.);
        for (label, mode) in [
            (if global { "Local" } else { "Inherit" }, FormMode::Inherit),
            ("URL", FormMode::Url),
            ("SSH", FormMode::Ssh),
        ] {
            modes = modes.child(owned(
                host,
                label,
                format!("legacy-mode-{}", label.to_ascii_lowercase()),
                Change::Mode(mode),
                false,
                form.mode == mode,
                k,
            ));
        }
        body = body.child(modes);
        if form.mode == FormMode::Cloud {
            body = body.child(note(
                "This saved Hermes Cloud connection is preserved, but the managed Cloud service is not available in this version.",
                k,
            ));
        }
        match form.mode {
            FormMode::Inherit => {
                body = body.child(note(
                    if global {
                        "Use an existing Hermes agent on this device."
                    } else {
                        "Use the global connection. Stored SSH settings are kept."
                    },
                    k,
                ))
            }
            FormMode::Url | FormMode::Cloud => {
                body = body.child(field(
                    host,
                    Field::Url,
                    "Gateway URL",
                    "https://gateway.example",
                    false,
                    false,
                    k,
                ));
                if form.mode == FormMode::Cloud {
                    body = body
                        .child(field(
                            host,
                            Field::CloudName,
                            "Cloud name",
                            "",
                            false,
                            false,
                            k,
                        ))
                        .child(field(host, Field::Org, "Organization", "", false, false, k));
                }
                body = body.child(field(
                    host,
                    Field::RemoteProfile,
                    "Backend profile (optional)",
                    "Gateway root profile",
                    false,
                    false,
                    k,
                ));
                body = body.child(
                    col()
                        .gap(8.)
                        .child(
                            text("Authentication")
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base),
                        )
                        .child(
                            row()
                                .gap(6.)
                                .child(owned(
                                    host,
                                    "Token",
                                    "legacy-auth-token",
                                    Change::Auth(AuthMode::Token),
                                    false,
                                    form.auth_mode == AuthMode::Token,
                                    k,
                                ))
                                .child(owned(
                                    host,
                                    "OAuth",
                                    "legacy-auth-oauth",
                                    Change::Auth(AuthMode::Oauth),
                                    false,
                                    form.auth_mode == AuthMode::Oauth,
                                    k,
                                )),
                        ),
                );
                if form.mode == FormMode::Url && form.auth_mode == AuthMode::Oauth {
                    body = body.child(note(
                        if state.login_pending() {
                            if state.authorization_url().is_some() {
                                "Continue sign-in in your browser."
                            } else {
                                "Preparing sign-in…"
                            }
                        } else if form.oauth_connected {
                            "Signed in"
                        } else {
                            "Not signed in"
                        },
                        k,
                    ));
                    let mut controls = row().gap(8.).child(owned(
                        host,
                        "Sign in",
                        "legacy-oauth-login",
                        Change::Login,
                        state.dirty() || state.login_pending(),
                        false,
                        k,
                    ));
                    if state.authorization_url().is_some() {
                        controls = controls.child(
                            Button::new("Open sign-in in browser")
                                .variant(ButtonVariant::Neutral)
                                .on_press_tx(|host: &mut H, tx| {
                                    let url = host
                                        .legacy_settings()
                                        .authorization_url()
                                        .map(str::to_owned);
                                    if let Some(url) = url {
                                        if tx.open_url(&url).is_err() {
                                            host.legacy_settings_mut().error = Some(
                                                "The browser could not be opened. Try again."
                                                    .into(),
                                            );
                                        }
                                    }
                                    tx.instant();
                                })
                                .build(k)
                                .key("legacy-oauth-open"),
                        );
                    }
                    if state.login_pending() {
                        controls = controls.child(owned(
                            host,
                            "Cancel sign-in",
                            "legacy-oauth-cancel",
                            Change::CancelLogin,
                            false,
                            false,
                            k,
                        ));
                    }
                    body = body.child(controls);
                }
                if form.auth_mode == AuthMode::Token {
                    body = body.child(
                        col()
                            .gap(8.)
                            .child(
                                text("Gateway token")
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base),
                            )
                            .child(note(
                                if form.token_stored {
                                    "Stored in the credential store"
                                } else {
                                    "No token stored"
                                },
                                k,
                            ))
                            .child(secret_choices(
                                host,
                                "token",
                                form.token_choice,
                                Change::Token,
                                k,
                            )),
                    );
                    if form.token_choice == SecretChoice::Set {
                        body = body.child(field(
                            host,
                            Field::Token,
                            "New token",
                            "Enter token",
                            true,
                            false,
                            k,
                        ));
                    }
                }
                body = body.child(
                    text("Access headers")
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                );
                for header in &form.headers {
                    let id = header.id;
                    let mut item = col()
                        .w_full()
                        .gap(10.)
                        .child(field(
                            host,
                            Field::HeaderName(id),
                            "Header name",
                            "X-Access-Token",
                            false,
                            false,
                            k,
                        ))
                        .child(note(
                            if header.stored {
                                "A value is stored. It is never loaded into this form."
                            } else {
                                "No value stored"
                            },
                            k,
                        ))
                        .child(secret_choices(
                            host,
                            &format!("header-{id}"),
                            header.choice,
                            move |choice| Change::HeaderSecret(id, choice),
                            k,
                        ));
                    if header.choice == SecretChoice::Set {
                        item = item.child(field(
                            host,
                            Field::HeaderValue(id),
                            "Header value",
                            "Enter value",
                            true,
                            false,
                            k,
                        ));
                    }
                    item = item.child(owned(
                        host,
                        "Remove header",
                        format!("legacy-header-remove-{id}"),
                        Change::RemoveHeader(id),
                        false,
                        false,
                        k,
                    ));
                    body = body.child(item.pb(12.));
                }
                body = body.child(owned(
                    host,
                    "Add header",
                    "legacy-header-add",
                    Change::AddHeader,
                    false,
                    false,
                    k,
                ));
            }
            FormMode::Ssh => {
                for (field_id, label, placeholder) in [
                    (Field::Host, "SSH host", "host.example"),
                    (Field::User, "SSH user", "Current user"),
                    (Field::Port, "SSH port", "22"),
                    (
                        Field::KeyPath,
                        "Private key path (optional)",
                        "SSH default keys",
                    ),
                    (
                        Field::HermesPath,
                        "Remote Hermes path (optional)",
                        "Installed Hermes",
                    ),
                    (
                        Field::SshProfile,
                        "Remote profile (optional)",
                        "Desktop profile",
                    ),
                ] {
                    body = body.child(field(host, field_id, label, placeholder, false, false, k));
                }
            }
        }
        body = body.child(
            row()
                .gap(8.)
                .child(owned(
                    host,
                    if state.pending() { "Saving…" } else { "Save" },
                    "legacy-save",
                    Change::Save,
                    false,
                    false,
                    k,
                ))
                .child({
                    let generation = state
                        .form
                        .as_ref()
                        .map(|form| form.generation)
                        .unwrap_or_default();
                    Button::new("Connect")
                        .variant(ButtonVariant::Neutral)
                        .disabled(
                            busy || state.dirty()
                                || state.login_pending()
                                || form.mode == FormMode::Cloud,
                        )
                        .on_press_tx(move |host: &mut H, tx| {
                            host.legacy_settings_connect(generation);
                            tx.instant();
                        })
                        .build(k)
                        .key("legacy-connect")
                })
                .child(action::<H>(
                    "Discard edits",
                    "legacy-discard",
                    Action::Discard,
                    busy || !state.dirty(),
                    false,
                    k,
                ))
                .child(action::<H>(
                    "Reload",
                    "legacy-reload",
                    Action::Reload,
                    busy || state.dirty(),
                    false,
                    k,
                )),
        );
        if state.dirty() {
            body = body.child(note("Unsaved changes", k).key("legacy-dirty"));
        }
        if !global {
            body = body.child(owned(
                host,
                "Remove profile override",
                "legacy-remove-override",
                Change::RemoveOverride,
                !form.has_override() || state.dirty(),
                false,
                k,
            ));
        }
    } else if state.loading() {
        body = body.child(note("Loading routing settings…", k).key("legacy-loading"));
    } else {
        body = body.child(action::<H>(
            "Load routing settings",
            "legacy-load",
            Action::Reload,
            busy,
            false,
            k,
        ));
    }
    body
}
