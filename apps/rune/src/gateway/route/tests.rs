use super::*;
fn policy(options: RouteOptions) -> ProfilePolicy {
    ProfilePolicy::new("client", options).unwrap()
}
fn override_policy(kind: OverrideKind, backend: Option<&str>) -> ProfilePolicy {
    policy(RouteOptions {
        profile_remote_override: true,
        override_kind: kind,
        backend_profile: backend.map(str::to_owned),
        ..Default::default()
    })
}
fn named(value: &str) -> WireScope {
    WireScope::Named(value.into())
}
fn route(backend: Backend, name: Option<&str>, scoped: bool) -> BackendRoute {
    BackendRoute {
        backend,
        descriptor_profile: name.map(str::to_owned),
        scope_path: scoped,
    }
}
#[test]
fn primary_pin_is_independent_of_global_remote_and_request_scopability() {
    let local = ProfilePolicy::new(
        "primary",
        RouteOptions {
            primary_profile: Some("primary".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        local.backend_route("", ""),
        route(Backend::Primary, None, false)
    );
    assert_eq!(
        local.backend_route("GET", "/api/config"),
        route(Backend::Primary, Some("primary"), true)
    );
    assert_eq!(
        local.backend_route("DELETE", "/api/profiles/other"),
        route(Backend::Primary, None, false)
    );
    assert_eq!(
        local.backend_route("POST", "/api/unknown"),
        route(Backend::Primary, None, false)
    );
    let remote = ProfilePolicy::new(
        "primary",
        RouteOptions {
            primary_profile: Some("primary".into()),
            global_remote: true,
            profile_remote_override: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        remote.backend_route("POST", "/api/unknown"),
        route(Backend::Primary, Some("primary"), true)
    );
    assert_eq!(remote.rpc_scope(None).unwrap(), WireScope::Omit);
    assert_eq!(remote.rpc_scope(Some("primary")).unwrap(), named("primary"));
}
#[test]
fn profile_override_precedes_global_but_remote_primary_and_own_entry_remain_distinct() {
    let scoped = policy(RouteOptions {
        profile_remote_override: true,
        global_remote: true,
        override_kind: OverrideKind::Url,
        ..Default::default()
    });
    assert_eq!(
        scoped.backend_route("GET", "/api/config"),
        route(Backend::Pool, None, false)
    );
    for options in [
        RouteOptions {
            global_remote: true,
            ..Default::default()
        },
        RouteOptions {
            primary_remote_active: true,
            ..Default::default()
        },
    ] {
        assert_eq!(
            policy(options).backend_route("GET", "/api/config"),
            route(Backend::Primary, Some("client"), true)
        );
    }
    let own_local = policy(RouteOptions {
        primary_remote_active: true,
        own_entry: true,
        ..Default::default()
    });
    assert_eq!(
        own_local.backend_route("GET", "/api/config"),
        route(Backend::Pool, None, false)
    );
}
#[test]
fn local_route_families_keep_query_body_and_unscopable_writes_separate() {
    let p = policy(RouteOptions::default());
    for (method, path) in [
        ("PUT", "/api/config/raw"),
        ("POST", "/api/skills/hub/install"),
        ("GET", "/api/actions/install-123/status"),
        ("GET", "/api/sessions/item/messages?limit=4"),
        ("POST", "/api/tools/x"),
        ("DELETE", "/api/webhooks/x"),
        ("POST", "/api/ops/import"),
        ("POST", "/api/gateway/restart"),
    ] {
        assert_eq!(
            p.backend_route(method, path),
            route(Backend::Primary, Some("client"), true),
            "{method} {path}"
        );
    }
    for (method, path) in [
        ("PATCH", "/api/sessions/item"),
        ("POST", "/api/sessions"),
        ("GET", "/api/profiles"),
        ("DELETE", "/api/profiles/target"),
    ] {
        assert_eq!(
            p.backend_route(method, path),
            route(Backend::Primary, None, false),
            "{method} {path}"
        );
    }
    assert_eq!(
        p.backend_route("POST", "/api/unscopable"),
        route(Backend::Pool, None, false)
    );
    assert_eq!(
        p.backend_route("GET", "/api/unscopable"),
        route(Backend::Primary, Some("client"), true)
    );
    let isolated = policy(RouteOptions {
        isolated_backend: true,
        ..Default::default()
    });
    assert_eq!(
        isolated.backend_route("GET", "/api/unscopable"),
        route(Backend::Pool, None, false)
    );
    assert_eq!(
        isolated.backend_route("GET", "/api/config"),
        route(Backend::Primary, Some("client"), true)
    );
}
#[test]
fn explicit_rpc_self_is_translated_while_ambient_pool_and_foreign_selectors_are_preserved() {
    let p = override_policy(OverrideKind::Url, Some("remote-client"));
    assert_eq!(p.rpc_scope(None).unwrap(), WireScope::Omit);
    assert_eq!(p.rpc_scope(Some("client")).unwrap(), named("remote-client"));
    for name in ["all", "__all__", "foreign", "default"] {
        assert_eq!(p.rpc_scope(Some(name)).unwrap(), named(name));
    }
    assert_eq!(p.rpc_scope(Some("")).unwrap(), WireScope::Omit);
    let shared = policy(RouteOptions {
        global_remote: true,
        ..Default::default()
    });
    assert_eq!(shared.rpc_scope(None).unwrap(), named("client"));
    let plain = ProfilePolicy::identity();
    assert_eq!(plain.rpc_scope(None).unwrap(), WireScope::Omit);
    assert_eq!(plain.rpc_scope(Some("client")).unwrap(), named("client"));
}
#[test]
fn rest_translates_only_declared_self_keys_and_preserves_pagination_cas_and_literal_text() {
    let p = override_policy(OverrideKind::Url, Some("remote-client"));
    let path = "/api/cron/jobs?profile=client&limit=50&expected_revision=rev-1&request_id=uuid&text=client#section";
    assert_eq!(
        p.rest_self_query("client", "GET", path).unwrap(),
        "/api/cron/jobs?profile=remote-client&limit=50&expected_revision=rev-1&request_id=uuid&text=client#section"
    );
    assert_eq!(
        p.rest_self_query(
            "client",
            "GET",
            "/api/profiles/sessions/sidebar?recents_profile=client&recents_limit=20&cron_limit=50"
        )
        .unwrap(),
        "/api/profiles/sessions/sidebar?recents_profile=remote-client&recents_limit=20&cron_limit=50"
    );
    for selector in ["all", "foreign", "__all__"] {
        let path = format!("/api/cron/jobs?profile={selector}&text=client");
        assert_eq!(p.rest_self_query("client", "GET", &path).unwrap(), path);
    }
    assert_eq!(
        p.rest_self_query("client", "GET", "/api/other?recents_profile=client")
            .unwrap(),
        "/api/other?recents_profile=client"
    );
    assert_eq!(
        p.rest_self_query("client", "GET", "/api/cron/jobs")
            .unwrap(),
        "/api/cron/jobs"
    );
}
#[test]
fn named_profile_paths_and_method_collisions_never_gain_an_ambient_owner() {
    let p = policy(RouteOptions {
        global_remote: true,
        ..Default::default()
    });
    for (method, path, expected) in [
        ("GET", "/api/config", "/api/config?profile=client"),
        (
            "GET",
            "/api/profiles/active",
            "/api/profiles/active?profile=client",
        ),
        ("PUT", "/api/profiles/active", "/api/profiles/active"),
        ("GET", "/api/profiles/import", "/api/profiles/import"),
        (
            "POST",
            "/api/profiles/import",
            "/api/profiles/import?profile=client",
        ),
        (
            "GET",
            "/api/profiles/sessions/sidebar",
            "/api/profiles/sessions/sidebar?profile=client",
        ),
        ("DELETE", "/api/profiles/named", "/api/profiles/named"),
        (
            "GET",
            "/api/config?profile=foreign",
            "/api/config?profile=foreign",
        ),
    ] {
        assert_eq!(
            p.rest_self_query("client", method, path).unwrap(),
            expected,
            "{method} {path}"
        );
    }
    let local = policy(RouteOptions::default());
    assert_eq!(
        local
            .rest_self_query("client", "PATCH", "/api/sessions/id")
            .unwrap(),
        "/api/sessions/id"
    );
    assert_eq!(
        local
            .rest_self_query("client", "GET", "/api/profiles")
            .unwrap(),
        "/api/profiles"
    );
}
#[test]
fn self_alias_translation_precedes_ambient_insertion_and_deduplicates_only_its_key() {
    let p = policy(RouteOptions {
        global_remote: true,
        backend_profile: Some("remote-client".into()),
        ..Default::default()
    });
    assert_eq!(
        p.rest_self_query(
            "client",
            "GET",
            "/api/profiles/sessions/sidebar?recents_profile=client&foo=client"
        )
        .unwrap(),
        "/api/profiles/sessions/sidebar?recents_profile=remote-client&foo=client"
    );
    assert_eq!(
        p.rest_self_query(
            "client",
            "GET",
            "/api/config?profile=client&profile=foreign&x=1"
        )
        .unwrap(),
        "/api/config?profile=remote-client&x=1"
    );
    assert_eq!(
        p.rest_self_query(
            "client",
            "GET",
            "/api/config?profile=foreign&profile=client"
        )
        .unwrap(),
        "/api/config?profile=foreign&profile=client"
    );
}
#[test]
fn ssh_argv_default_is_omitted_but_explicit_rpc_and_rest_default_remain_named() {
    let ssh = override_policy(OverrideKind::Ssh, Some("default"));
    assert_eq!(ssh.ssh_profile_arg(), None);
    assert_eq!(ssh.rpc_scope(Some("client")).unwrap(), named("default"));
    assert_eq!(ssh.rpc_scope(Some("default")).unwrap(), named("default"));
    assert_eq!(
        ssh.rest_self_query("client", "GET", "/api/cron/jobs?profile=client")
            .unwrap(),
        "/api/cron/jobs?profile=default"
    );
    assert_eq!(ssh.session_aggregate_scope("client").unwrap(), "client");
    let mapped = override_policy(OverrideKind::Ssh, Some("remote-client"));
    assert_eq!(mapped.ssh_profile_arg().as_deref(), Some("remote-client"));
    assert_eq!(
        mapped.session_aggregate_scope("client").unwrap(),
        "remote-client"
    );
    let url = override_policy(OverrideKind::Url, Some("remote-client"));
    let selector = url.session_aggregate_scope("client").unwrap();
    assert_eq!(selector, "client");
    assert_eq!(
        url.rest_self_query(
            "client",
            "GET",
            &format!("/api/sessions?profile={selector}")
        )
        .unwrap(),
        "/api/sessions?profile=remote-client"
    );
    for name in ["foreign", "all"] {
        assert_eq!(ssh.session_aggregate_scope(name).unwrap(), name);
    }
}
#[test]
fn aggregate_compatibility_is_one_scoped_list_get_on_same_selected_override_status_only() {
    let p = override_policy(OverrideKind::Url, Some("remote-client"));
    for status in [400, 404] {
        assert!(p.aggregate_get_retry_unscoped(
            "GET",
            "/api/sessions?profile=client&limit=20",
            status
        ));
        assert!(p.aggregate_get_retry_unscoped(
            "GET",
            "/api/sessions?profile=remote-client",
            status
        ));
    }
    for status in [0, 200, 401, 403, 409, 429, 500, 503] {
        assert!(!p.aggregate_get_retry_unscoped("GET", "/api/sessions?profile=client", status));
    }
    for (method, path) in [
        ("POST", "/api/sessions?profile=client"),
        ("GET", "/api/sessions/id?profile=client"),
        ("GET", "/api/sessions?profile=all"),
        ("GET", "/api/sessions?profile=foreign"),
        ("GET", "/api/sessions"),
    ] {
        assert!(
            !p.aggregate_get_retry_unscoped(method, path, 400),
            "{method} {path}"
        );
    }
    assert!(!ProfilePolicy::identity().aggregate_get_retry_unscoped(
        "GET",
        "/api/sessions?profile=client",
        400
    ));
    assert!(
        !policy(RouteOptions {
            global_remote: true,
            ..Default::default()
        })
        .aggregate_get_retry_unscoped("GET", "/api/sessions?profile=client", 400)
    );
}
#[test]
fn response_owner_is_proved_in_backend_namespace_before_any_desktop_projection() {
    let p = override_policy(OverrideKind::Url, Some("remote-client"));
    assert_eq!(
        p.validate_response_owner(Some("remote-client"), "remote-client", false)
            .unwrap(),
        OwnerEvidence::Named
    );
    assert!(
        p.validate_response_owner(Some("client"), "remote-client", false)
            .is_err()
    );
    assert!(
        p.validate_response_owner(Some("foreign"), "remote-client", true)
            .is_err()
    );
    assert!(
        p.validate_response_owner(None, "remote-client", false)
            .is_err()
    );
    assert_eq!(
        p.validate_response_owner(None, "remote-client", true)
            .unwrap(),
        OwnerEvidence::Unowned
    );
    assert!(p.validate_response_owner(None, "", true).is_err());
}
#[test]
fn scope_selection_never_rewrites_payload_text_or_deep_fields() {
    let p = override_policy(OverrideKind::Url, Some("remote-client"));
    let payload = serde_json::json!({"profile":"client","text":"client","nested":{"profile":"client"},"argv":["client"]});
    let original = payload.clone();
    let wire = p.rpc_scope(payload["profile"].as_str()).unwrap();
    assert_eq!(wire, named("remote-client"));
    assert_eq!(payload, original);
    assert_eq!(payload["text"], "client");
    assert_eq!(payload["nested"]["profile"], "client");
}
#[test]
fn frozen_policy_fingerprint_captures_semantics_and_ignores_rejected_alias_spelling() {
    let a = override_policy(OverrideKind::Url, Some("remote-client"));
    let same = override_policy(OverrideKind::Url, Some(" remote-client "));
    let other = override_policy(OverrideKind::Url, Some("other"));
    assert_eq!(a.fingerprint(), same.fingerprint());
    assert_ne!(a.fingerprint(), other.fingerprint());
    assert_ne!(
        a.fingerprint(),
        override_policy(OverrideKind::Ssh, Some("remote-client")).fingerprint()
    );
    for value in ["bad name", "root", ""] {
        assert_eq!(
            override_policy(OverrideKind::Url, Some(value)),
            override_policy(OverrideKind::Url, None)
        );
    }
    assert!(ProfilePolicy::new("bad profile", RouteOptions::default()).is_err());
    assert!(
        ProfilePolicy::new(
            "client",
            RouteOptions {
                primary_profile: Some("bad pin".into()),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(ProfilePolicy::identity().desktop_profile(), None);
    assert_eq!(a.desktop_profile(), Some("client"));
}

#[test]
fn private_ipc_policy_preserves_mapping_and_rejects_normalisation_or_future_schema() {
    let original = override_policy(OverrideKind::Url, Some("remote-work"));
    let encoded = serde_json::to_value(original.private_ipc_snapshot()).unwrap();
    let restored =
        ProfilePolicy::from_private_ipc(serde_json::from_value(encoded.clone()).unwrap()).unwrap();
    assert_eq!(restored.fingerprint(), original.fingerprint());
    assert_eq!(
        restored.rpc_scope(Some("client")).unwrap(),
        named("remote-work")
    );
    assert_eq!(
        restored
            .rest_self_query("client", "GET", "/api/config?profile=client")
            .unwrap(),
        "/api/config?profile=remote-work"
    );
    for (field, value) in [
        ("desktop", serde_json::json!(" CLIENT ")),
        ("options.backend_profile", serde_json::json!("root")),
        ("options.primary_profile", serde_json::json!(null)),
    ] {
        let mut invalid = encoded.clone();
        if let Some(key) = field.strip_prefix("options.") {
            invalid["legacy"]["options"][key] = value;
        } else {
            invalid["legacy"][field] = value;
        }
        assert!(ProfilePolicy::from_private_ipc(serde_json::from_value(invalid).unwrap()).is_err());
    }
    let mut future = encoded.clone();
    future["version"] = serde_json::json!(2);
    assert!(ProfilePolicy::from_private_ipc(serde_json::from_value(future).unwrap()).is_err());
    let mut unknown = encoded;
    unknown["legacy"]["options"]["future_scope"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProfilePolicySnapshot>(unknown).is_err());
    let identity = ProfilePolicy::from_private_ipc(
        serde_json::from_value(
            serde_json::to_value(ProfilePolicy::identity().private_ipc_snapshot()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(identity, ProfilePolicy::identity());
}

#[test]
fn ambient_primary_or_pool_preserves_absent_empty_and_backend_alias_collisions() {
    let alias = ProfilePolicy::new(
        "default",
        RouteOptions {
            profile_remote_override: true,
            backend_profile: Some("dixie".into()),
            override_kind: OverrideKind::Url,
            ..Default::default()
        },
    )
    .unwrap();
    let pooled = override_policy(OverrideKind::Url, Some("backend-client"));
    for route in [&alias, &pooled, &ProfilePolicy::identity()] {
        assert_eq!(route.ambient_rpc_scope(None).unwrap(), WireScope::Omit);
        for original in [
            "",
            "default",
            "dixie",
            "client",
            "foreign",
            "all",
            "__all__",
            " default ",
        ] {
            assert_eq!(
                route.ambient_rpc_scope(Some(original)).unwrap(),
                named(original)
            );
        }
        assert!(route.ambient_rpc_scope(Some("wrong\nowner")).is_err());
    }
    // An explicitly desktop-owned target and a raw backend selector with the
    // same spelling are different requests, even when both backend rows exist.
    assert_eq!(alias.rpc_scope(Some("default")).unwrap(), named("dixie"));
    assert_eq!(
        alias.ambient_rpc_scope(Some("default")).unwrap(),
        named("default")
    );
    assert_eq!(
        pooled.rpc_scope(Some("client")).unwrap(),
        named("backend-client")
    );
    assert_eq!(
        pooled.ambient_rpc_scope(Some("client")).unwrap(),
        named("client")
    );
}

#[test]
fn ambient_shared_primary_wrapper_overwrites_existing_empty_foreign_or_all_selectors() {
    let global_shared = policy(RouteOptions {
        global_remote: true,
        ..Default::default()
    });
    let inherited_shared = policy(RouteOptions {
        primary_remote_active: true,
        ..Default::default()
    });
    for route in [&global_shared, &inherited_shared] {
        for original in [
            None,
            Some(""),
            Some("foreign"),
            Some("all"),
            Some("default"),
        ] {
            assert_eq!(route.ambient_rpc_scope(original).unwrap(), named("client"));
        }
        // Declared backend/cross-profile intent is not the generic wrapper.
        assert_eq!(route.rpc_scope(Some("foreign")).unwrap(), named("foreign"));
    }
    let own_entry = policy(RouteOptions {
        primary_remote_active: true,
        own_entry: true,
        ..Default::default()
    });
    assert_eq!(own_entry.ambient_rpc_scope(None).unwrap(), WireScope::Omit);
    assert_eq!(
        own_entry.ambient_rpc_scope(Some("foreign")).unwrap(),
        named("foreign")
    );
    let primary = ProfilePolicy::new(
        "default",
        RouteOptions {
            global_remote: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(primary.ambient_rpc_scope(None).unwrap(), WireScope::Omit);
    assert_eq!(primary.ambient_rpc_scope(Some("")).unwrap(), named(""));
    assert_eq!(
        primary.ambient_rpc_scope(Some("foreign")).unwrap(),
        named("foreign")
    );
}
