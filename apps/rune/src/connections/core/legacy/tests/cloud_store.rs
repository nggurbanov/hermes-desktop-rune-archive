use super::super::cloud_store::ScopedTokenStore;
use super::*;
use crate::connections::cloud::{TokenStore, Tokens};
use std::sync::Barrier;

fn tokens(access: &str) -> Tokens {
    Tokens {
        access_token: access.into(),
        refresh_token: format!("refresh-{access}"),
        expires_at: 123,
        provider: "fixture".into(),
        user_id: "private-user".into(),
    }
}
fn setup(fixture: &Private) -> (Core, Scope, domain::SlotStamp, CredentialRef) {
    let mut core = fixture.core();
    let scope = Scope::Profile("work".into());
    let mut settings = edit(
        scope.clone(),
        absent(),
        "https://private.fixture.invalid/",
        SecretEdit::Clear,
    );
    if let ValueEdit::Profile(Profile::Remote(remote)) = &mut settings.value {
        remote.auth_mode = domain::AuthMode::Oauth;
    } else {
        unreachable!()
    }
    let saved = core.save_legacy(settings).unwrap();
    let receipt = core
        .install_cloud_tokens(scope.clone(), saved.version, tokens("initial"))
        .unwrap();
    let stamp = match receipt.version {
        domain::ScopeVersion::Present(stamp) => stamp,
        _ => unreachable!(),
    };
    let registry = fixture.load();
    let reference = remote_in_store(registry.legacy.as_ref().unwrap(), &scope)
        .unwrap()
        .oauth_credential
        .clone()
        .unwrap();
    (core, scope, stamp, reference)
}
#[test]
fn oauth_token_cas_serializes_private_writers_and_never_serializes_secrets_to_metadata() {
    let fixture = Private::new();
    let (_core, scope, stamp, reference) = setup(&fixture);
    let metadata = fs::read_to_string(&fixture.path).unwrap();
    assert!(!metadata.contains("initial") && !metadata.contains("refresh-initial"));
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|name| {
            let mut store = ScopedTokenStore::new(
                fixture.path.clone(),
                scope.clone(),
                &stamp,
                &reference,
                Box::new(fixture.secrets.clone()),
            );
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                store
                    .compare_and_swap(&tokens("initial"), Some(&tokens(name)))
                    .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = writers
        .into_iter()
        .map(|writer| writer.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| **result).count(), 1);
    let mut store = ScopedTokenStore::new(
        fixture.path.clone(),
        scope,
        &stamp,
        &reference,
        Box::new(fixture.secrets.clone()),
    );
    let current = store.load().unwrap().unwrap();
    assert!(current == tokens("one") || current == tokens("two"));
    assert!(store.compare_and_swap(&current, None).unwrap());
    assert!(store.load().unwrap().is_none());
}
#[test]
fn retired_scope_cannot_load_or_overwrite_a_later_oauth_identity() {
    let fixture = Private::new();
    let (mut core, scope, stamp, reference) = setup(&fixture);
    let mut old = ScopedTokenStore::new(
        fixture.path.clone(),
        scope.clone(),
        &stamp,
        &reference,
        Box::new(fixture.secrets.clone()),
    );
    let before = core.legacy_snapshot().unwrap();
    let mut replacement = edit(
        scope.clone(),
        before.scope_version(&scope),
        "https://other.fixture.invalid/",
        SecretEdit::Clear,
    );
    if let ValueEdit::Profile(Profile::Remote(remote)) = &mut replacement.value {
        remote.auth_mode = domain::AuthMode::Oauth;
    }
    core.save_legacy(replacement).unwrap();
    assert!(old.load().is_err());
    assert!(
        old.compare_and_swap(&tokens("initial"), Some(&tokens("stale")))
            .is_err()
    );
    assert!(
        !fixture
            .secrets
            .values
            .lock()
            .unwrap()
            .contains_key(reference.id())
    );
}

#[test]
fn refused_keyring_write_and_stale_login_never_publish_oauth_reference() {
    let fixture = Private::new();
    let mut core = fixture.core();
    let scope = Scope::Profile("work".into());
    let mut settings = edit(
        scope.clone(),
        absent(),
        "https://private.fixture.invalid/",
        SecretEdit::Clear,
    );
    if let ValueEdit::Profile(Profile::Remote(remote)) = &mut settings.value {
        remote.auth_mode = domain::AuthMode::Oauth;
    }
    let saved = core.save_legacy(settings).unwrap();
    fixture.secrets.deny_set.store(true, Ordering::Release);
    assert!(
        core.install_cloud_tokens(
            scope.clone(),
            saved.version.clone(),
            tokens("private-failed")
        )
        .is_err()
    );
    let registry = fixture.load();
    let remote = remote_in_store(registry.legacy.as_ref().unwrap(), &scope).unwrap();
    assert!(remote.oauth_credential.is_none());
    assert!(
        !fs::read_to_string(&fixture.path)
            .unwrap()
            .contains("private-failed")
    );
    fixture.secrets.deny_set.store(false, Ordering::Release);
    let replacement = edit(
        scope.clone(),
        saved.version.clone(),
        "https://new.fixture.invalid/",
        SecretEdit::Clear,
    );
    core.save_legacy(replacement).unwrap();
    let before = fixture.secrets.values.lock().unwrap().len();
    assert!(
        core.install_cloud_tokens(scope, saved.version, tokens("private-stale"))
            .is_err()
    );
    assert_eq!(fixture.secrets.values.lock().unwrap().len(), before);
}

#[test]
fn changing_access_proxy_credentials_retires_the_previous_oauth_identity() {
    let fixture = Private::new();
    let (mut core, scope, stamp, reference) = setup(&fixture);
    let snapshot = core.legacy_snapshot().unwrap();
    let profile = snapshot.profiles.get("work").unwrap().value.clone();
    let edit = Edit {
        scope: scope.clone(),
        expected: snapshot.scope_version(&scope),
        value: ValueEdit::Profile(profile),
        auth: AuthEdit {
            token: SecretEdit::Keep,
            headers: BTreeMap::from([(
                "x-private-access".into(),
                SecretEdit::Set("new-private-header".into()),
            )]),
        },
    };
    core.save_legacy(edit).unwrap();
    let registry = fixture.load();
    assert!(
        remote_in_store(registry.legacy.as_ref().unwrap(), &scope)
            .unwrap()
            .oauth_credential
            .is_none()
    );
    let mut old = ScopedTokenStore::new(
        fixture.path.clone(),
        scope,
        &stamp,
        &reference,
        Box::new(fixture.secrets.clone()),
    );
    assert!(old.load().is_err());
    assert!(
        !fixture
            .secrets
            .values
            .lock()
            .unwrap()
            .contains_key(reference.id())
    );
}

#[test]
#[ignore = "requires the isolated private Secret Service fixture"]
fn system_keyring_installs_rotates_and_retires_scoped_oauth_tokens() {
    let home = std::env::var("HOME").unwrap();
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let root = std::path::Path::new(&home).parent().unwrap();
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("rune-private-credentials-")
    );
    assert_eq!(bus, format!("unix:path={}", root.join("bus").display()));
    let fixture = Private::new();
    let mut core = Core::system(fixture.path.clone());
    let scope = Scope::Profile("private-oauth".into());
    let mut settings = edit(
        scope.clone(),
        absent(),
        "https://private.fixture.invalid/",
        SecretEdit::Clear,
    );
    if let ValueEdit::Profile(Profile::Remote(remote)) = &mut settings.value {
        remote.auth_mode = domain::AuthMode::Oauth;
    }
    let saved = core.save_legacy(settings).unwrap();
    let receipt = core
        .install_cloud_tokens(scope.clone(), saved.version, tokens("private-first"))
        .unwrap();
    let stamp = match receipt.version.clone() {
        domain::ScopeVersion::Present(stamp) => stamp,
        _ => unreachable!(),
    };
    let registry = fixture.load();
    let reference = remote_in_store(registry.legacy.as_ref().unwrap(), &scope)
        .unwrap()
        .oauth_credential
        .clone()
        .unwrap();
    let mut store = ScopedTokenStore::new(
        fixture.path.clone(),
        scope.clone(),
        &stamp,
        &reference,
        Box::new(SystemSecrets),
    );
    assert!(store.load().unwrap() == Some(tokens("private-first")));
    assert!(
        store
            .compare_and_swap(&tokens("private-first"), Some(&tokens("private-second")))
            .unwrap()
    );
    assert!(store.load().unwrap() == Some(tokens("private-second")));
    assert!(
        !fs::read_to_string(&fixture.path)
            .unwrap()
            .contains("private-second")
    );
    let replacement = edit(
        scope,
        receipt.version,
        "https://other.fixture.invalid/",
        SecretEdit::Clear,
    );
    core.save_legacy(replacement).unwrap();
    assert!(store.load().is_err());
    assert!(
        SystemSecrets
            .get_optional(reference.id())
            .unwrap()
            .is_none()
    );
}
