use super::*;
use crate::connections::legacy::testing::Rig;
use std::{
    fs,
    time::{Duration, Instant},
};
fn settle(settings: &mut Settings, connections: &Connections) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while settings.loading() || settings.pending() {
        settings.poll(connections);
        assert!(
            Instant::now() < deadline,
            "Private settings actor did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn load(rig: &Rig) -> Settings {
    let mut settings = Settings::new();
    settings.ensure_loaded(&rig.connections);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    assert!(settings.form.is_some());
    settings
}
fn change(settings: &mut Settings, connections: &Connections, change: Change) {
    let generation = settings.form.as_ref().unwrap().generation;
    settings.action(Action::Form { generation, change }, connections);
}
fn input(settings: &mut Settings, field: Field, value: &str) {
    let generation = settings.form.as_ref().unwrap().generation;
    settings.edit_mut(generation, field).replace_text(value);
}
fn url(settings: &mut Settings, connections: &Connections, value: &str) {
    change(settings, connections, Change::Mode(FormMode::Url));
    input(settings, Field::Url, value);
}
#[test]
fn actual_form_save_persists_secret_references_only_and_snapshot_never_loads_values() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    url(
        &mut settings,
        &rig.connections,
        "https://private-gateway.invalid/hermes/",
    );
    change(
        &mut settings,
        &rig.connections,
        Change::Token(SecretChoice::Set),
    );
    input(&mut settings, Field::Token, "private-form-token");
    change(&mut settings, &rig.connections, Change::AddHeader);
    let id = settings.form.as_ref().unwrap().headers[0].id;
    input(&mut settings, Field::HeaderName(id), "X-Access-Fixture");
    input(&mut settings, Field::HeaderValue(id), "private-form-header");
    change(&mut settings, &rig.connections, Change::Save);
    assert!(settings.pending());
    assert!(settings.notice.is_none());
    assert!(settings.can_close().is_err());
    settings.action(
        Action::Scope(Scope::Profile("other".into())),
        &rig.connections,
    );
    assert!(matches!(settings.scope, Scope::Global));
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    assert_eq!(settings.notice.as_deref(), Some("Saved."));
    assert!(!settings.dirty());
    let form = settings.form.as_ref().unwrap();
    assert!(form.token_stored);
    assert!(form.headers[0].stored);
    assert!(form.field(Field::Token).unwrap().text.is_empty());
    assert!(
        form.field(Field::HeaderValue(form.headers[0].id))
            .unwrap()
            .text
            .is_empty()
    );
    let stored = rig.state.read_json().to_string();
    assert!(!stored.contains("private-form-token"));
    assert!(!stored.contains("private-form-header"));
    assert!(stored.contains("private-gateway.invalid"));
    assert_eq!(rig.state.credential_count(), 1);
    let before = rig.state.secret_operations();
    settings.action(Action::Reload, &rig.connections);
    settle(&mut settings, &rig.connections);
    assert_eq!(
        rig.state.secret_operations(),
        before,
        "Opening settings must never fetch plaintext credentials"
    );
}
#[test]
fn failed_commit_keeps_authored_values_and_retries_the_original_cas() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    url(
        &mut settings,
        &rig.connections,
        "https://private-gateway.invalid/",
    );
    change(
        &mut settings,
        &rig.connections,
        Change::Token(SecretChoice::Set),
    );
    input(&mut settings, Field::Token, "retained-after-failure");
    rig.state.fail_saves(true);
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.as_deref().unwrap().contains("refused"));
    assert!(settings.notice.is_none());
    assert!(settings.dirty());
    assert_eq!(
        settings
            .form
            .as_ref()
            .unwrap()
            .field(Field::Token)
            .unwrap()
            .text,
        "retained-after-failure"
    );
    settings.action(
        Action::Scope(Scope::Profile("other".into())),
        &rig.connections,
    );
    assert!(matches!(settings.scope, Scope::Global));
    assert!(settings.can_close().is_err());
    rig.state.fail_saves(false);
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    assert_eq!(settings.notice.as_deref(), Some("Saved."));
    assert!(!settings.dirty());
}
#[test]
fn stale_form_is_rejected_before_any_secret_operation_and_unrelated_profile_save_merges() {
    let rig = Rig::new();
    let mut first = load(&rig);
    let mut stale = load(&rig);
    let mut unrelated = load(&rig);
    unrelated.action(
        Action::Scope(Scope::Profile("research".into())),
        &rig.connections,
    );
    settle(&mut unrelated, &rig.connections);
    url(&mut first, &rig.connections, "https://first.invalid/");
    change(&mut first, &rig.connections, Change::Save);
    settle(&mut first, &rig.connections);
    assert!(first.error.is_none());
    url(&mut stale, &rig.connections, "https://stale.invalid/");
    change(
        &mut stale,
        &rig.connections,
        Change::Token(SecretChoice::Set),
    );
    input(&mut stale, Field::Token, "must-not-reach-keychain");
    let before = rig.state.secret_operations();
    change(&mut stale, &rig.connections, Change::Save);
    settle(&mut stale, &rig.connections);
    assert!(stale.error.as_deref().unwrap().contains("another window"));
    assert_eq!(rig.state.secret_operations(), before);
    assert!(stale.dirty());
    assert_eq!(
        stale
            .form
            .as_ref()
            .unwrap()
            .field(Field::Token)
            .unwrap()
            .text,
        "must-not-reach-keychain"
    );
    url(
        &mut unrelated,
        &rig.connections,
        "https://research.invalid/",
    );
    change(&mut unrelated, &rig.connections, Change::Save);
    settle(&mut unrelated, &rig.connections);
    assert!(unrelated.error.is_none());
    let json = rig.state.read_json();
    assert_eq!(
        json["legacy"]["global"]["value"]["remote"]["url"],
        "https://first.invalid/"
    );
    assert_eq!(
        json["legacy"]["profiles"]["research"]["value"]["url"],
        "https://research.invalid/"
    );
}
#[test]
fn late_reads_and_callbacks_cannot_overwrite_edits_or_another_scope() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    let old = settings.snapshot.as_ref().unwrap().clone();
    let old_generation = settings.form.as_ref().unwrap().generation;
    url(&mut settings, &rig.connections, "https://authored.invalid/");
    let (sender, receiver) = std::sync::mpsc::channel();
    settings.read = Some(Read {
        generation: settings.generation,
        scope: Scope::Global,
        receiver,
    });
    sender.send(Ok(old.clone())).unwrap();
    settings.poll(&rig.connections);
    assert_eq!(
        settings
            .form
            .as_ref()
            .unwrap()
            .field(Field::Url)
            .unwrap()
            .text,
        "https://authored.invalid/"
    );
    settings.action(Action::Discard, &rig.connections);
    settle(&mut settings, &rig.connections);
    settings.action(
        Action::Scope(Scope::Profile("research".into())),
        &rig.connections,
    );
    settle(&mut settings, &rig.connections);
    let (sender, receiver) = std::sync::mpsc::channel();
    settings.read = Some(Read {
        generation: settings.generation,
        scope: Scope::Global,
        receiver,
    });
    sender.send(Ok(old)).unwrap();
    settings.poll(&rig.connections);
    assert_eq!(settings.scope, Scope::Profile("research".into()));
    settings.error = Some("Current scope error".into());
    settings.action(
        Action::Form {
            generation: old_generation,
            change: Change::Mode(FormMode::Cloud),
        },
        &rig.connections,
    );
    assert_eq!(settings.error.as_deref(), Some("Current scope error"));
    settings
        .edit_mut(old_generation, Field::Url)
        .replace_text("wrong source");
    assert!(
        settings
            .form
            .as_ref()
            .unwrap()
            .field(Field::Url)
            .unwrap()
            .text
            .is_empty()
    );
}
#[test]
fn accepted_save_keeps_real_actor_until_receipt_after_opener_drops() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    url(&mut settings, &rig.connections, "https://retained.invalid/");
    change(&mut settings, &rig.connections, Change::Save);
    assert!(settings.pending());
    let state = rig.state.clone();
    drop(rig.connections);
    settle(&mut settings, &Connections::new());
    assert_eq!(settings.notice.as_deref(), Some("Saved."));
    assert_eq!(
        state.read_json()["legacy"]["global"]["value"]["remote"]["url"],
        "https://retained.invalid/"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while !state.worker_exited() {
        assert!(
            Instant::now() < deadline,
            "A completed ticket leaked the actor"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn corrupt_or_future_registry_does_not_become_a_successful_empty_form() {
    for contents in ["{bad-json", r#"{"version":999,"future":"retain-me"}"#] {
        let rig = Rig::new();
        fs::write(rig.state.path(), contents).unwrap();
        let mut settings = Settings::new();
        settings.ensure_loaded(&rig.connections);
        settle(&mut settings, &rig.connections);
        assert!(settings.form.is_none());
        assert!(settings.error.is_some());
        assert!(settings.notice.is_none());
        assert_eq!(fs::read_to_string(rig.state.path()).unwrap(), contents);
    }
}
#[test]
fn dormant_ssh_and_unknown_fields_survive_real_profile_mode_changes() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    settings.action(
        Action::Scope(Scope::Profile("research".into())),
        &rig.connections,
    );
    settle(&mut settings, &rig.connections);
    change(&mut settings, &rig.connections, Change::Mode(FormMode::Ssh));
    input(&mut settings, Field::Host, "private-host.invalid");
    input(&mut settings, Field::HermesPath, "/opt/hermes");
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    let mut json = rig.state.read_json();
    json["legacy"]["profiles"]["research"]["value"]["future-profile-field"] =
        serde_json::json!({"keep":true});
    fs::write(rig.state.path(), serde_json::to_vec(&json).unwrap()).unwrap();
    settings.action(Action::Reload, &rig.connections);
    settle(&mut settings, &rig.connections);
    change(
        &mut settings,
        &rig.connections,
        Change::Mode(FormMode::Inherit),
    );
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    let value = rig.state.read_json();
    assert_eq!(
        value["legacy"]["profiles"]["research"]["value"]["saved_ssh"]["host"],
        "private-host.invalid"
    );
    assert_eq!(
        value["legacy"]["profiles"]["research"]["value"]["future-profile-field"]["keep"],
        true
    );
    change(&mut settings, &rig.connections, Change::Mode(FormMode::Ssh));
    assert_eq!(
        settings
            .form
            .as_ref()
            .unwrap()
            .field(Field::HermesPath)
            .unwrap()
            .text,
        "/opt/hermes"
    );
}

#[test]
fn headers_are_renamed_and_removed_with_explicit_secret_intents() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    url(&mut settings, &rig.connections, "https://headers.invalid/");
    change(
        &mut settings,
        &rig.connections,
        Change::Token(SecretChoice::Set),
    );
    input(&mut settings, Field::Token, "kept-gateway-token");
    change(&mut settings, &rig.connections, Change::AddHeader);
    let id = settings.form.as_ref().unwrap().headers[0].id;
    input(&mut settings, Field::HeaderName(id), "X-Old-Access");
    input(&mut settings, Field::HeaderValue(id), "old-access-value");
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    let id = settings.form.as_ref().unwrap().headers[0].id;
    input(&mut settings, Field::HeaderName(id), "X-New-Access");
    let before = rig.state.secret_operations();
    change(&mut settings, &rig.connections, Change::Save);
    assert!(!settings.pending());
    assert!(settings.error.as_deref().unwrap().contains("renamed"));
    assert_eq!(rig.state.secret_operations(), before);
    change(
        &mut settings,
        &rig.connections,
        Change::HeaderSecret(id, SecretChoice::Set),
    );
    input(&mut settings, Field::HeaderValue(id), "new-access-value");
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    let json = rig.state.read_json();
    assert_eq!(
        json["legacy"]["global"]["value"]["remote"]["header_names"],
        serde_json::json!(["x-new-access"])
    );
    assert!(settings.form.as_ref().unwrap().token_stored);
    let id = settings.form.as_ref().unwrap().headers[0].id;
    change(&mut settings, &rig.connections, Change::RemoveHeader(id));
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    assert!(settings.form.as_ref().unwrap().headers.is_empty());
    assert!(settings.form.as_ref().unwrap().token_stored);
    change(
        &mut settings,
        &rig.connections,
        Change::Token(SecretChoice::Clear),
    );
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    assert!(settings.error.is_none());
    assert!(!settings.form.as_ref().unwrap().token_stored);
    assert_eq!(rig.state.credential_count(), 0);
}

#[test]
fn oauth_ui_captures_saved_form_and_rejects_late_events_after_edit_and_scope_change() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    url(&mut settings, &rig.connections, "http://127.0.0.1:9");
    change(
        &mut settings,
        &rig.connections,
        Change::Auth(AuthMode::Oauth),
    );
    change(&mut settings, &rig.connections, Change::Login);
    assert!(!settings.login_pending()); // unsaved edits cannot start a login
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    let form = settings.form.as_ref().unwrap();
    let (attempt, send) = crate::connections::LoginAttempt::test_channel();
    settings.login = Some(Login {
        scope: settings.scope.clone(),
        generation: form.generation,
        expected: form.expected(),
        profile_name: settings.profile_name.text.clone(),
        attempt,
        authorize_url: None,
    });
    send.send(crate::connections::LoginEvent::Authorize(
        "http://127.0.0.1/authorize".into(),
    ))
    .ok();
    assert!(settings.poll(&rig.connections));
    assert!(settings.authorization_url().is_some());
    let generation = settings.form.as_ref().unwrap().generation;
    settings
        .edit_mut(generation, Field::Url)
        .replace_text("http://127.0.0.1:10");
    assert!(
        settings.authorization_url().is_none(),
        "changed forms cannot open a stale authorization URL even before poll"
    );
    settings.poll(&rig.connections);
    assert!(settings.authorization_url().is_none());
    let version = settings.form.as_ref().unwrap().expected();
    send.send(crate::connections::LoginEvent::test_complete(
        Scope::Global,
        version,
    ))
    .ok();
    settings.poll(&rig.connections);
    assert!(settings.dirty());
    assert_ne!(settings.notice.as_deref(), Some("Signed in."));
    settings.action(Action::Discard, &rig.connections);
    settle(&mut settings, &rig.connections);
    let form = settings.form.as_ref().unwrap();
    let (attempt, send) = crate::connections::LoginAttempt::test_channel();
    settings.login = Some(Login {
        scope: settings.scope.clone(),
        generation: form.generation,
        expected: form.expected(),
        profile_name: settings.profile_name.text.clone(),
        attempt,
        authorize_url: None,
    });
    settings.action(
        Action::Scope(Scope::Profile("other".into())),
        &rig.connections,
    );
    send.send(crate::connections::LoginEvent::Authorize(
        "http://127.0.0.1/late".into(),
    ))
    .ok();
    settings.poll(&rig.connections);
    assert!(settings.authorization_url().is_none());
    assert_eq!(settings.scope, Scope::Profile("other".into()));
}

#[test]
fn oauth_receipt_is_confirmed_against_the_reloaded_scope_version() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    let snapshot = settings.snapshot.as_ref().unwrap().clone();
    let version = snapshot.scope_version(&settings.scope);
    let (send, receiver) = std::sync::mpsc::channel();
    settings.read = Some(Read {
        generation: settings.generation,
        scope: settings.scope.clone(),
        receiver,
    });
    settings.login_receipt = Some((settings.scope.clone(), version.clone()));
    send.send(Ok(snapshot.clone())).unwrap();
    settings.poll(&rig.connections);
    assert_eq!(settings.notice.as_deref(), Some("Signed in."));
    let (send, receiver) = std::sync::mpsc::channel();
    settings.read = Some(Read {
        generation: settings.generation,
        scope: settings.scope.clone(),
        receiver,
    });
    settings.login_receipt = Some((
        settings.scope.clone(),
        crate::connections::legacy::ScopeVersion::Absent {
            revision: Some("different".into()),
        },
    ));
    settings.notice = None;
    send.send(Ok(snapshot)).unwrap();
    settings.poll(&rig.connections);
    assert_ne!(settings.notice.as_deref(), Some("Signed in."));
    assert!(
        settings
            .error
            .as_deref()
            .unwrap()
            .contains("changed after sign-in")
    );
}

#[test]
fn source_epoch_change_invalidates_pending_authorization_and_late_completion() {
    let rig = Rig::new();
    let mut settings = load(&rig);
    url(&mut settings, &rig.connections, "http://127.0.0.1:9");
    change(
        &mut settings,
        &rig.connections,
        Change::Auth(AuthMode::Oauth),
    );
    change(&mut settings, &rig.connections, Change::Save);
    settle(&mut settings, &rig.connections);
    settings.observe_source(7);
    let form = settings.form.as_ref().unwrap();
    let (attempt, send) = crate::connections::LoginAttempt::test_channel();
    settings.login = Some(Login {
        scope: settings.scope.clone(),
        generation: form.generation,
        expected: form.expected(),
        profile_name: settings.profile_name.text.clone(),
        attempt,
        authorize_url: None,
    });
    send.send(crate::connections::LoginEvent::Authorize(
        "http://127.0.0.1/authorize".into(),
    ))
    .ok();
    settings.poll(&rig.connections);
    assert!(settings.authorization_url().is_some());
    settings.observe_source(7);
    assert!(settings.authorization_url().is_some());
    settings.observe_source(8);
    assert!(settings.authorization_url().is_none());
    let version = settings.form.as_ref().unwrap().expected();
    send.send(crate::connections::LoginEvent::test_complete(
        Scope::Global,
        version,
    ))
    .ok();
    settings.poll(&rig.connections);
    assert_ne!(settings.notice.as_deref(), Some("Signed in."));
}
#[test]
fn runtime_focus_loss_does_not_cancel_sign_in_but_subsequent_edit_does() {
    use rune::{Cx, El, Env, Runtime, TextStyle, col, el, text_input};
    struct State {
        settings: Settings,
        attempt: Option<crate::connections::LoginAttempt>,
        connections: Connections,
    }
    fn view(s: &State, _: &mut Cx<State>) -> El {
        let form = s.settings.form.as_ref().unwrap();
        let generation = form.generation;
        col()
            .size(300., 100.)
            .child(
                text_input(form.field(Field::Url).unwrap(), TextStyle::new(14., 400.))
                    .key("url")
                    .size(300., 40.)
                    .on_edit(move |s: &mut State| s.settings.edit_mut(generation, Field::Url)),
            )
            .child(
                el().key("sign-in")
                    .size(100., 40.)
                    .on_press_tx(|s: &mut State, tx| {
                        tx.blur();
                        assert!(s.settings.test_start_attempt(s.attempt.take().unwrap()));
                    }),
            )
    }
    for mutation in 0..3 {
        let rig = Rig::new();
        let mut settings = load(&rig);
        url(&mut settings, &rig.connections, "http://127.0.0.1:9");
        change(
            &mut settings,
            &rig.connections,
            Change::Auth(AuthMode::Oauth),
        );
        change(&mut settings, &rig.connections, Change::Save);
        settle(&mut settings, &rig.connections);
        let (attempt, _sender) = crate::connections::LoginAttempt::test_channel();
        let mut rt = Runtime::new(
            State {
                settings,
                attempt: Some(attempt),
                connections: rig.connections,
            },
            view,
            (300., 100.),
            Env::new(true, 1.),
        );
        rt.frame(0.);
        let (x, y) = rt.find("url").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.);
        let (x, y) = rt.find("sign-in").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.);
        assert!(
            rt.state.settings.login_pending(),
            "focus loss must not cancel a just-started login"
        );
        rt.update(|s, _| {
            s.settings.poll(&s.connections);
        });
        let (x, y) = rt.find("url").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.);
        rt.update(|s, _| {
            s.settings.poll(&s.connections);
        });
        assert!(
            rt.state.settings.login_pending(),
            "focus and selection must retain login"
        );
        rt.key(rune::KeyEvent {
            key: rune::Key::Left,
            mods: Default::default(),
        });
        rt.frame(0.);
        rt.update(|s, _| {
            s.settings.poll(&s.connections);
        });
        assert!(
            rt.state.settings.login_pending(),
            "caret movement must retain login"
        );
        rt.key(rune::KeyEvent {
            key: rune::Key::Left,
            mods: rune::Mods {
                shift: true,
                ..Default::default()
            },
        });
        rt.frame(0.);
        rt.update(|s, _| {
            s.settings.poll(&s.connections);
        });
        assert!(
            rt.state.settings.login_pending(),
            "selection must retain login"
        );
        rt.key(rune::KeyEvent {
            key: rune::Key::Tab,
            mods: Default::default(),
        });
        rt.frame(0.);
        rt.update(|s, _| {
            s.settings.poll(&s.connections);
        });
        assert!(
            rt.state.settings.login_pending(),
            "tab navigation must retain login"
        );
        let (x, y) = rt.find("url").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.);
        rt.update(|s, _| {
            s.settings.poll(&s.connections);
        });
        assert!(rt.state.settings.login_pending());
        match mutation {
            0 => rt.key(rune::KeyEvent {
                key: rune::Key::Text("x".into()),
                mods: Default::default(),
            }),
            1 => rt.paste("paste"),
            _ => rt.ime(rune::Ime::Commit("ime".into())),
        }
        rt.frame(0.);
        assert!(
            rt.state
                .settings
                .login
                .as_ref()
                .unwrap()
                .attempt
                .test_is_held(),
            "mutation must fence publication before poll"
        );
        rt.update(|s, _| {
            s.settings.poll(&s.connections);
        });
        assert!(
            !rt.state.settings.login_pending(),
            "text mutation must invalidate login"
        );
    }
}
