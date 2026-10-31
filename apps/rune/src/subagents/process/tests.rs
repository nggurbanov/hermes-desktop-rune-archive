use super::*;
use crate::gateway::{
    prepared::WirePolicy,
    route::{OverrideKind, ProfilePolicy, RouteOptions, WireScope},
};
use crate::source::{RouteDoor, RouteOwner};
use std::{collections::BTreeMap, io::Cursor, net::TcpListener, sync::Arc, thread};
use tungstenite::{
    Message,
    handshake::server::{Request, Response},
};

fn launch(connection: &Connection) -> Launch {
    Launch::new(
        connection,
        &DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 31,
            profile: "work".into(),
            session: Some("parent-session".into()),
        },
        &ChildRow {
            id: "child-task".into(),
            session: Some("child-session".into()),
            parent: Some("parent-session".into()),
            goal: "Inspect only".into(),
            status: "running".into(),
            model: "".into(),
            activity: "".into(),
            summary: "".into(),
            task_index: 0,
        },
        Some(true),
    )
}
fn roundtrip(launch: Launch) -> Launch {
    let mut pipe = Vec::new();
    send(&mut pipe, &json!({"type":"launch","launch":launch})).unwrap();
    let decoded = frame(&mut Cursor::new(pipe)).unwrap().unwrap();
    serde_json::from_value(decoded["launch"].clone()).unwrap()
}
fn routed(endpoint: String, profiles: ProfilePolicy, saved_door: bool) -> Connection {
    let owner = if saved_door {
        RouteOwner::saved(
            "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222",
            "captured-namespace",
        )
        .unwrap()
    } else {
        RouteOwner::legacy("captured-namespace").unwrap()
    };
    Connection::Routed {
        actual: Box::new(Connection::Url {
            endpoint,
            token: Some("private-parent-token".into()),
        }),
        policy: Arc::new(
            WirePolicy::token(
                profiles,
                BTreeMap::from([("X-Access-Fixture".into(), "private-proxy-secret".into())]),
            )
            .unwrap(),
        ),
        owner: Arc::new(owner),
    }
}
#[test]
fn private_launch_roundtrip_preserves_full_policy_and_requested_owner_without_saved_provenance() {
    let profiles = ProfilePolicy::new(
        "work",
        RouteOptions {
            profile_remote_override: true,
            backend_profile: Some("backend-work".into()),
            override_kind: OverrideKind::Url,
            ..Default::default()
        },
    )
    .unwrap();
    for saved_door in [false, true] {
        let original = routed(
            "http://127.0.0.1:1/hermes/".into(),
            profiles.clone(),
            saved_door,
        );
        let wrapped = Connection::Saved {
            id: "33333333-3333-4333-8333-333333333333".into(),
            revision: "44444444-4444-4444-8444-444444444444".into(),
            inner: Box::new(original.clone()),
        };
        let restored = roundtrip(launch(&wrapped)).connection();
        assert_eq!(
            restored.wire_policy().map(|p| p.fingerprint()),
            original.wire_policy().map(|p| p.fingerprint()),
            "The spectator lost its captured authentication/profile policy"
        );
        assert_eq!(
            restored.route_owner().map(|o| o.fingerprint()),
            original.route_owner().map(|o| o.fingerprint())
        );
        assert!(matches!(restored, Connection::Routed { .. }));
        let inherited = restored.wire_policy().unwrap().profiles();
        assert_eq!(
            inherited.rpc_scope(Some("work")).unwrap(),
            WireScope::Named("backend-work".into())
        );
        assert_eq!(
            inherited
                .rest_self_query("work", "GET", "/api/config?profile=work")
                .unwrap(),
            "/api/config?profile=backend-work"
        );
        assert_eq!(
            matches!(
                restored.route_owner().unwrap().door(),
                RouteDoor::Saved { .. }
            ),
            saved_door
        );
    }
}
#[test]
fn private_launch_roundtrip_reaches_only_strict_authenticated_readonly_spectator_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
            let url = url::Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
            assert_eq!(url.path(), "/hermes/api/ws");
            assert_eq!(
                url.query_pairs()
                    .filter(|(key, _)| key == "token")
                    .map(|(_, value)| value.into_owned())
                    .collect::<Vec<_>>(),
                ["private-parent-token"]
            );
            assert_eq!(
                request.headers().get("x-access-fixture").unwrap(),
                "private-proxy-secret"
            );
            assert!(!request.headers().contains_key("x-hermes-session-token"));
            Ok(response)
        })
        .unwrap();
        let request: Value =
            serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(request["method"], "session.resume");
        assert_eq!(request["params"]["profile"], "work");
        assert_eq!(request["params"]["session_id"], "child-session");
        assert_eq!(request["params"]["lazy"], true);
        assert_eq!(request["params"]["defer_history"], false);
        assert!(request["params"].get("prompt").is_none());
        ws.send(Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"result":{"session_id":"live-child","stored_session_id":"child-session","messages":[{"role":"assistant","content":"Read-only child history"}],"running":false,"info":{"profile_name":"work"}}}).to_string().into())).unwrap();
        assert!(
            matches!(ws.read(), Err(_) | Ok(Message::Close(_))),
            "The spectator must close without an agent command"
        );
    });
    let original = routed(
        format!("http://{address}/hermes/"),
        ProfilePolicy::identity(),
        true,
    );
    let inherited = roundtrip(launch(&original));
    let mut reader =
        SessionTransport::connect(&inherited.connection(), &inherited.profile).unwrap();
    let snapshot = reader.watch(&inherited.child).unwrap();
    assert_eq!(snapshot.stored, "child-session");
    assert_eq!(snapshot.messages[0].content, "Read-only child history");
    drop(reader);
    server.join().unwrap();
}

#[test]
fn invalid_inherited_route_is_refused_during_launch_decode() {
    let profiles = ProfilePolicy::new(
        "work",
        RouteOptions {
            profile_remote_override: true,
            backend_profile: Some("backend-work".into()),
            override_kind: OverrideKind::Url,
            ..Default::default()
        },
    )
    .unwrap();
    let value = serde_json::to_value(launch(&routed(
        "http://127.0.0.1:1/".into(),
        profiles,
        true,
    )))
    .unwrap();
    for target in [
        "route-version",
        "policy-version",
        "profile-version",
        "owner-id",
        "owner-revision",
        "namespace",
        "header",
        "profile",
        "unknown",
    ] {
        let mut invalid = value.clone();
        match target {
            "route-version" => invalid["route"]["version"] = json!(2),
            "policy-version" => invalid["route"]["policy"]["version"] = json!(2),
            "profile-version" => invalid["route"]["policy"]["profiles"]["version"] = json!(2),
            "owner-id" => invalid["route"]["owner"]["door"]["id"] = json!("not-a-source-id"),
            "owner-revision" => {
                invalid["route"]["owner"]["door"]["revision"] = json!("not-a-revision")
            }
            "namespace" => invalid["route"]["owner"]["namespace"] = json!("wrong\nnamespace"),
            "header" => {
                invalid["route"]["policy"]["headers"]["authorization"] =
                    json!("must-not-replace-auth")
            }
            "profile" => {
                invalid["route"]["policy"]["profiles"]["legacy"]["desktop"] = json!("../foreign")
            }
            "unknown" => invalid["route"]["future_transport"] = json!(true),
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<Launch>(invalid).is_err(),
            "Invalid inherited {target} must not fall back to a plain transport"
        );
    }
}

#[test]
fn plain_launches_remain_compatible_and_routed_local_retains_native_policy() {
    for actual in [
        Connection::Url {
            endpoint: "http://127.0.0.1:1/".into(),
            token: Some("private-parent-token".into()),
        },
        Connection::Local {
            home: PathBuf::from("/private-fixture-home"),
        },
    ] {
        let value = serde_json::to_value(launch(&actual)).unwrap();
        assert!(value.get("route").is_none());
        let inherited = roundtrip(serde_json::from_value(value).unwrap()).connection();
        match (actual, &inherited) {
            (
                Connection::Url { endpoint, token },
                Connection::Url {
                    endpoint: next,
                    token: next_token,
                },
            ) => {
                assert_eq!(&endpoint, next);
                assert_eq!(&token, next_token);
            }
            (Connection::Local { home }, Connection::Local { home: next }) => {
                assert_eq!(&home, next)
            }
            _ => panic!("A plain launch changed transport kind"),
        }
        assert!(inherited.wire_policy().is_none());
        assert!(inherited.route_owner().is_none());
    }
    let original = Connection::Routed {
        actual: Box::new(Connection::Local {
            home: PathBuf::from("/private-fixture-home"),
        }),
        policy: Arc::new(WirePolicy::native(
            ProfilePolicy::new("work", RouteOptions::default()).unwrap(),
        )),
        owner: Arc::new(RouteOwner::legacy("captured-local-namespace").unwrap()),
    };
    let inherited = roundtrip(launch(&original)).connection();
    assert!(
        matches!(inherited.transport(),Connection::Local {home} if home==&PathBuf::from("/private-fixture-home"))
    );
    assert_eq!(
        inherited.wire_policy().unwrap().fingerprint(),
        original.wire_policy().unwrap().fingerprint()
    );
    assert_eq!(
        inherited.route_owner().unwrap().fingerprint(),
        original.route_owner().unwrap().fingerprint()
    );
}
