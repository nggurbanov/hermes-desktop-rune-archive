use super::*;
use crate::gateway::route::{OverrideKind, RouteOptions};
use serde_json::{Value, json};

fn profiles() -> ProfilePolicy {
    ProfilePolicy::new(
        "client",
        RouteOptions {
            primary_profile: Some("primary".into()),
            profile_remote_override: true,
            backend_profile: Some("dixie".into()),
            override_kind: OverrideKind::Url,
            ..Default::default()
        },
    )
    .unwrap()
}
fn token_policy() -> WirePolicy {
    WirePolicy::token(
        profiles(),
        BTreeMap::from([("X-Access-Fixture".into(), "private-access-fixture".into())]),
    )
    .unwrap()
}
fn import(value: Value) -> Result<WirePolicy, String> {
    let snapshot = serde_json::from_value(value).map_err(|_| "Invalid private policy snapshot")?;
    WirePolicy::from_private_ipc(snapshot)
}

#[test]
fn private_roundtrip_keeps_the_complete_auth_and_profile_policy() {
    for policy in [WirePolicy::native(profiles()), token_policy()] {
        let serialized = serde_json::to_vec(&policy.private_ipc_snapshot()).unwrap();
        let imported =
            WirePolicy::from_private_ipc(serde_json::from_slice(&serialized).unwrap()).unwrap();
        assert_eq!(policy.fingerprint(), imported.fingerprint());
        assert_eq!(policy.auth, imported.auth);
        assert_eq!(policy.profiles, imported.profiles);
        assert_eq!(policy.headers, imported.headers);
        let connection = Connection::Routed {
            actual: Box::new(Connection::Url {
                endpoint: "https://gateway.test/hermes/".into(),
                token: Some("private-session-fixture".into()),
            }),
            owner: Arc::new(crate::source::RouteOwner::legacy("private-ipc-roundtrip").unwrap()),
            policy: Arc::new(imported),
        };
        let request = PreparedTransport::from_connection(&connection, Instant::now())
            .unwrap()
            .prepare_http(
                Url::parse("https://gateway.test/hermes/api/status").unwrap(),
                None,
                Instant::now(),
            )
            .unwrap();
        match policy.auth {
            WireAuth::Preserve => {
                assert!(request.headers.is_empty());
                assert!(
                    request
                        .url
                        .query_pairs()
                        .any(|(key, value)| key == "token" && value == "private-session-fixture")
                );
            }
            WireAuth::LegacyToken => {
                assert!(!request.url.query_pairs().any(|(key, _)| key == "token"));
                assert_eq!(
                    request.headers["x-hermes-session-token"],
                    "private-session-fixture"
                );
                assert_eq!(
                    request.headers["x-access-fixture"],
                    "private-access-fixture"
                );
                assert!(request.headers["x-access-fixture"].is_sensitive());
            }
        }
    }
    assert_ne!(
        WirePolicy::native(profiles()).fingerprint(),
        WirePolicy::token(profiles(), BTreeMap::new())
            .unwrap()
            .fingerprint()
    );
}

#[test]
fn private_import_refuses_unknown_or_silently_normalized_auth_material() {
    let snapshot = serde_json::to_value(token_policy().private_ipc_snapshot()).unwrap();
    let mut changed = snapshot.clone();
    changed["version"] = json!(2);
    assert!(import(changed).is_err());
    let mut changed = snapshot.clone();
    changed["unknown"] = json!(true);
    assert!(import(changed).is_err());
    for headers in [
        json!({"X-Access-Fixture":"private-access-fixture"}),
        json!({"authorization":"Bearer private-access-fixture"}),
        json!({"x-access-fixture":" private-access-fixture "}),
        json!({"x-access-fixture":"private\r\n-access-fixture"}),
        json!({"x-access-fixture":""}),
        json!({"x-access-fixture":"one", "X-Access-Fixture":"two"}),
    ] {
        let mut changed = snapshot.clone();
        changed["headers"] = headers;
        assert!(import(changed).is_err());
    }
    let mut changed = snapshot.clone();
    changed["auth"] = json!("preserve");
    assert!(import(changed).is_err());
    let mut changed = snapshot.clone();
    changed["auth"] = json!("oauth");
    assert!(import(changed).is_err());
    let mut changed = snapshot.clone();
    changed["profiles"]["version"] = json!(2);
    assert!(import(changed).is_err());
    let mut changed = snapshot;
    changed["profiles"]["legacy"]["options"]["backend_profile"] = json!("root");
    assert!(import(changed).is_err());
}

#[test]
fn selected_root_storage_identity_is_stable_but_runtime_authority_and_ipc_are_exact() {
    let profiles = ProfilePolicy::new(
        "client",
        RouteOptions {
            profile_remote_override: true,
            override_kind: OverrideKind::Url,
            ..Default::default()
        },
    )
    .unwrap();
    let base = WirePolicy::token(
        profiles.clone(),
        BTreeMap::from([("X-Access-Fixture".into(), "private-access-fixture".into())]),
    )
    .unwrap();
    let policy = |instance: &str, epoch| {
        base.with_selected(
            SelectedContext::new(
                "client",
                BackendTarget::Root(crate::gateway::BackendRootProof {
                    profile: None,
                    profile_id: "/remote/custom".into(),
                    instance_id: instance.into(),
                    authority_epoch: epoch,
                }),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let original = policy("original-runtime", 7);
    let replacement = policy("replacement-runtime", 8);
    assert_eq!(original.fingerprint(), replacement.fingerprint());
    assert_ne!(
        original.runtime_fingerprint(),
        replacement.runtime_fingerprint()
    );
    let snapshot = serde_json::to_value(original.private_ipc_snapshot()).unwrap();
    let restored = import(snapshot.clone()).unwrap();
    assert_eq!(
        restored.runtime_fingerprint(),
        original.runtime_fingerprint()
    );
    match restored.selected().unwrap().target() {
        BackendTarget::Root(proof) => {
            assert_eq!(proof.instance_id, "original-runtime");
            assert_eq!(proof.authority_epoch, 7);
            assert_eq!(proof.profile, None);
        }
        _ => panic!("The private snapshot changed the selected target kind"),
    }
    let rebound = restored.with_profiles(profiles);
    assert!(rebound.selected().is_none());
    assert!(rebound.rpc_scope(&ProfileProvenance::NativeSelf).is_err());
    assert_eq!(rebound.auth, WireAuth::LegacyToken);
    assert_eq!(rebound.headers, base.headers);
    let mut malformed = snapshot;
    malformed["selected"]["target"]["profile_id"] = json!("default");
    assert!(import(malformed).is_err());
}
