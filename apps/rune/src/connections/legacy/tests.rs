use super::*;
use serde_json::json;
fn absent() -> ScopeVersion {
    ScopeVersion::Absent { revision: None }
}
fn remote(url: &str) -> Global {
    Global {
        mode: Mode::Remote,
        remote: Some(Remote::new(url.into())),
        ..Global::default()
    }
}
fn context() -> SelectionContext {
    SelectionContext {
        environment: Environment {
            url: None,
            token: None,
            generation: "fixture-env".into(),
        },
        primary: None,
        primary_profile: Some("default".into()),
        primary_remote_active: true,
        isolated_backend: false,
        local_home: None,
    }
}
fn put_global(store: &mut Store, value: Global) {
    let expected = store.scope_version(&Scope::Global);
    store
        .apply(&Scope::Global, &expected, ValueEdit::Global(value))
        .unwrap();
}
fn put_profile(store: &mut Store, name: &str, value: Profile) {
    let scope = Scope::Profile(name.into());
    let expected = store.scope_version(&scope);
    store
        .apply(&scope, &expected, ValueEdit::Profile(value))
        .unwrap();
}
fn stamp(version: ScopeVersion) -> SlotStamp {
    match version {
        ScopeVersion::Present(s) => s,
        _ => panic!("expected present route"),
    }
}
fn ssh(host: &str) -> InstalledSsh {
    InstalledSsh {
        host: host.into(),
        user: String::new(),
        port: 22,
        key_path: None,
        remote_hermes_path: Some("/external/hermes".into()),
        remote_profile: None,
        credential: None,
        other: Unknown::new(),
    }
}
#[test]
fn per_scope_cas_keeps_independent_edits_but_rejects_same_scope_stale_forms() {
    let mut store = Store::default();
    let initial = store.scope_version(&Scope::Global);
    put_profile(
        &mut store,
        "research",
        Profile::Remote(Remote::new("https://research.invalid/".into())),
    );
    store
        .apply(
            &Scope::Global,
            &initial,
            ValueEdit::Global(remote("https://global.invalid/")),
        )
        .unwrap();
    assert_eq!(store.snapshot().profiles.len(), 1);
    let stable = serde_json::to_value(&store).unwrap();
    assert!(
        store
            .apply(
                &Scope::Global,
                &initial,
                ValueEdit::Global(Global::default())
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(&store).unwrap(), stable);
}
#[test]
fn deleted_slot_tombstone_fences_absent_create_delete_aba_and_new_slot_identity() {
    let mut store = Store::default();
    let scope = Scope::Profile("research".into());
    let first_absent = store.scope_version(&scope);
    put_profile(
        &mut store,
        "research",
        Profile::Remote(Remote::new("https://a.invalid/".into())),
    );
    let first = stamp(store.scope_version(&scope));
    let version = store.scope_version(&scope);
    let deleted = store.apply(&scope, &version, ValueEdit::Remove).unwrap();
    assert_ne!(deleted, first_absent);
    assert!(
        store
            .apply(
                &scope,
                &first_absent,
                ValueEdit::Profile(Profile::Inherit {
                    saved_ssh: None,
                    other: Unknown::new()
                })
            )
            .is_err()
    );
    store
        .apply(
            &scope,
            &deleted,
            ValueEdit::Profile(Profile::Inherit {
                saved_ssh: None,
                other: Unknown::new(),
            }),
        )
        .unwrap();
    assert_ne!(stamp(store.scope_version(&scope)).id, first.id);
}
#[test]
fn resolution_ignores_unrelated_and_cosmetic_changes_but_observes_own_inherit_and_auth_alias() {
    let mut store = Store::default();
    put_global(&mut store, remote("https://global.invalid/"));
    let ctx = context();
    let inherited = store.select("research", &ctx).unwrap().version;
    put_profile(
        &mut store,
        "other",
        Profile::Remote(Remote::new("https://other.invalid/".into())),
    );
    let old = stamp(store.scope_version(&Scope::Global));
    let mut global = store.snapshot().global;
    global.remote.as_mut().unwrap().name = Some("Renamed only".into());
    global.remote.as_mut().unwrap().remote_profile = Some("ignored-global-alias".into());
    global.ssh = Some(ssh("dormant.invalid"));
    put_global(&mut store, global);
    let edited = stamp(store.scope_version(&Scope::Global));
    assert_ne!(old.edit_revision, edited.edit_revision);
    assert_eq!(old.route_revision, edited.route_revision);
    assert!(store.validates_resolution(&inherited, &ctx));
    put_profile(
        &mut store,
        "research",
        Profile::Inherit {
            saved_ssh: Some(ssh("dormant.invalid")),
            other: Unknown::new(),
        },
    );
    assert!(!store.validates_resolution(&inherited, &ctx));
    let explicit = store.select("research", &ctx).unwrap();
    assert!(explicit.own_entry);
    assert_eq!(explicit.origin, Origin::Global);
    let mut selected = Remote::new("https://profile.invalid/".into());
    selected.remote_profile = Some("backend-research".into());
    put_profile(&mut store, "research", Profile::Remote(selected));
    let own = store.select("research", &ctx).unwrap();
    assert_eq!(own.backend_profile.as_deref(), Some("backend-research"));
    put_global(&mut store, remote("https://new-global.invalid/"));
    assert!(store.validates_resolution(&own.version, &ctx));
    let mut selected = match store.snapshot().profiles["research"].value.clone() {
        Profile::Remote(r) => r,
        _ => unreachable!(),
    };
    selected.remote_profile = Some("default".into());
    put_profile(&mut store, "research", Profile::Remote(selected));
    assert!(!store.validates_resolution(&own.version, &ctx));
}
#[test]
fn selection_preserves_override_priority_and_never_falls_back_from_selected_errors() {
    let mut store = Store::default();
    put_global(&mut store, remote("https://global.invalid/"));
    let mut ctx = context();
    ctx.environment.url = Some("https://environment.invalid/".into());
    assert!(store.select("research", &ctx).is_err());
    put_profile(
        &mut store,
        "research",
        Profile::Remote(Remote::new("https://profile.invalid/".into())),
    );
    assert_eq!(
        store.select("research", &ctx).unwrap().origin,
        Origin::Profile
    );
    let scope = Scope::Profile("research".into());
    let expected = store.scope_version(&scope);
    store.apply(&scope, &expected, ValueEdit::Remove).unwrap();
    ctx.environment.url = None;
    put_global(
        &mut store,
        Global {
            mode: Mode::Ssh,
            ssh: Some(ssh("")),
            ..Global::default()
        },
    );
    assert!(store.select("research", &ctx).is_err());
    put_global(&mut store, remote("https://global.invalid/"));
    put_profile(&mut store, "research", Profile::Ssh(ssh("")));
    assert_eq!(
        store.select("research", &ctx).unwrap().origin,
        Origin::Global
    );
}
#[test]
fn native_unknown_fields_and_unsupported_config_survive_but_future_schema_refuses_writes() {
    let mut store = Store::default();
    let mut global = remote("https://global.invalid/");
    global.other.insert("global-future".into(), json!({"x":1}));
    global
        .remote
        .as_mut()
        .unwrap()
        .other
        .insert("transport-future".into(), json!(2));
    global.remote.as_mut().unwrap().auth_mode = AuthMode::Oauth;
    put_global(&mut store, global);
    let mut source = ssh("external.invalid");
    source.other.insert("ssh-future".into(), json!(["a"]));
    let cleared = Profile::inherit_from(Some(&Profile::Ssh(source.clone())));
    assert!(
        matches!(cleared, Profile::Inherit {saved_ssh: Some(ref kept), ..} if kept.remote_hermes_path.as_deref() == Some("/external/hermes") && kept.host == "external.invalid")
    );
    put_profile(
        &mut store,
        "dormant",
        Profile::Inherit {
            saved_ssh: Some(source),
            other: BTreeMap::from([("profile-future".into(), json!(true))]),
        },
    );
    store.other.insert("store-future".into(), json!("kept"));
    store
        .global
        .as_mut()
        .unwrap()
        .other
        .insert("slot-future".into(), json!(3));
    let bytes = serde_json::to_vec(&store).unwrap();
    let decoded: Store = serde_json::from_slice(&bytes).unwrap();
    decoded.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&store).unwrap()
    );
    store.version = 2;
    let before = serde_json::to_value(&store).unwrap();
    assert!(
        store
            .apply(
                &Scope::Global,
                &store.scope_version(&Scope::Global),
                ValueEdit::Remove
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(&store).unwrap(), before);
}
#[test]
fn all_credential_references_include_dormant_blocks_without_exposing_them_in_debug() {
    let mut store = Store::default();
    let remote_ref = CredentialRef::fresh();
    let ssh_ref = CredentialRef::fresh();
    let mut global = Global::default();
    global.remote = Some(Remote::new("https://dormant.invalid/".into()));
    global.remote.as_mut().unwrap().credential = Some(remote_ref.clone());
    let mut dormant = ssh("dormant.invalid");
    dormant.credential = Some(ssh_ref.clone());
    global.ssh = Some(dormant);
    put_global(&mut store, global);
    let references = store.credential_refs();
    assert_eq!(references.len(), 2);
    assert!(references.contains(remote_ref.id()));
    assert!(references.contains(ssh_ref.id()));
    let debug = format!("{:?}", store.snapshot());
    assert!(!debug.contains(remote_ref.id()));
    assert!(!debug.contains(ssh_ref.id()));
    let before = serde_json::to_value(&store).unwrap();
    assert!(
        store
            .apply(
                &Scope::Profile("bad/name".into()),
                &absent(),
                ValueEdit::Profile(Profile::Remote(Remote::new(
                    "https://valid.invalid/".into()
                )))
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(&store).unwrap(), before);
    assert!(
        store
            .apply(
                &Scope::Global,
                &store.scope_version(&Scope::Global),
                ValueEdit::Global(remote("https://gateway.invalid/?token=plaintext-fixture"))
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(&store).unwrap(), before);
}

#[test]
fn captured_environment_has_stable_namespace_without_serializing_credentials() {
    let a = Environment::captured(
        Some("wss://fixture.invalid/base/api/ws".into()),
        Some("fixture-secret-a".into()),
    );
    let b = Environment::captured(
        Some("https://fixture.invalid/base/".into()),
        Some("fixture-secret-a".into()),
    );
    let c = Environment::captured(
        Some("https://fixture.invalid/base/".into()),
        Some("fixture-secret-b".into()),
    );
    assert_eq!(a.generation, b.generation);
    assert_ne!(a.generation, c.generation);
    assert!(!a.generation.contains("fixture-secret"));
    assert_eq!(
        Environment::captured(None, None).generation,
        Environment::captured(None, Some("unused-without-url".into())).generation
    );
}

#[test]
fn changing_profile_modes_preserves_extension_fields_and_dormant_ssh_without_retaining_active_auth()
{
    let mut ssh = InstalledSsh::new("private-ssh.invalid".into());
    ssh.other
        .insert("future-extension".into(), json!({"nested": [1,2]}));
    let previous = Profile::Ssh(ssh.clone());
    let inherited = Profile::inherit_from(Some(&previous));
    assert_eq!(inherited.saved_ssh(), Some(&ssh));
    let mut remote = Remote::new("https://private.invalid/".into());
    remote.other.insert("new-extension".into(), json!(true));
    let remote = Profile::remote_from(Some(&inherited), remote);
    let mut store = Store::default();
    put_profile(&mut store, "research", previous);
    put_profile(&mut store, "research", remote.clone());
    let json = serde_json::to_value(&store.snapshot().profiles["research"].value).unwrap();
    assert_eq!(json["future-extension"], json!({"nested": [1,2]}));
    assert_eq!(json["new-extension"], json!(true));
    let next = Profile::ssh_from(Some(&remote), InstalledSsh::new("new.invalid".into()));
    put_profile(&mut store, "research", next);
    let json = serde_json::to_value(&store.snapshot().profiles["research"].value).unwrap();
    assert_eq!(json["host"], "new.invalid");
    assert_eq!(json["future-extension"], json!({"nested": [1,2]}));
    assert_eq!(json["new-extension"], json!(true));
    assert!(store.credential_refs().is_empty());
}
