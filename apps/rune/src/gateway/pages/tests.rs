use super::*;
use crate::{
    composer::DraftScope,
    gateway::prepared::{BackendTarget, ProfileProvenance, SelectedContext, WirePolicy},
    pages::{PageNetwork, PageRequest},
    source::ProfileIntent,
};
use std::{collections::BTreeMap, net::TcpListener, sync::Arc, thread};
use tungstenite::handshake::server::{Request, Response};

fn connection(listener: &TcpListener, target: BackendTarget) -> Connection {
    let policy = route::ProfilePolicy::new(
        "default",
        route::RouteOptions {
            primary_profile: Some("primary".into()),
            profile_remote_override: true,
            override_kind: route::OverrideKind::Url,
            ..Default::default()
        },
    )
    .unwrap();
    let policy = WirePolicy::token(
        policy,
        BTreeMap::from([("X-Access-Fixture".into(), "private-access".into())]),
    )
    .unwrap()
    .with_selected(SelectedContext::new("default", target).unwrap())
    .unwrap();
    Connection::Routed {
        actual: Box::new(Connection::Url {
            endpoint: format!("http://{}/hermes/", listener.local_addr().unwrap()),
            token: Some("private-session".into()),
        }),
        policy: Arc::new(policy),
        owner: Arc::new(crate::source::RouteOwner::legacy("projects-private-test").unwrap()),
    }
}

fn scope(epoch: u64) -> DraftScope {
    DraftScope {
        draft_origin: None,
        profile_intent: ProfileIntent::Backend,
        connection: epoch,
        profile: "work".into(),
        session: None,
    }
}

fn run(
    network: &mut PageNetwork,
    connection: Connection,
    epoch: u64,
    method: &str,
    body: Value,
    provenance: ProfileProvenance,
) -> Result<Value, String> {
    let scope = scope(epoch);
    network
        .request(
            &scope,
            connection,
            "projects",
            PageRequest::rpc(method, body)
                .profile("work")
                .provenance(provenance)
                .timeout_secs(3),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(reply) = network
            .poll(&scope)
            .into_iter()
            .find(|reply| !reply.progress)
        {
            return reply.result;
        }
        assert!(Instant::now() < deadline, "missing page reply");
        thread::sleep(Duration::from_millis(2));
    }
}

fn server(listener: TcpListener, expected: Vec<(Value, Value)>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for (params, result) in expected {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket =
                tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                    let url = Url::parse(&format!("http://private{}", request.uri())).unwrap();
                    assert_eq!(url.path(), "/hermes/api/ws");
                    assert!(
                        url.query_pairs()
                            .any(|(k, v)| k == "token" && v == "private-session")
                    );
                    assert_eq!(request.headers()["x-access-fixture"], "private-access");
                    Ok(response)
                })
                .unwrap();
            let request: Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(request["method"], params["method"]);
            assert_eq!(request["params"], params["params"]);
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0", "id":request["id"], "result":result})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
    })
}

#[test]
fn projects_page_network_preserves_source_and_backend_profile() {
    let a = TcpListener::bind("127.0.0.1:0").unwrap();
    let b = TcpListener::bind("127.0.0.1:0").unwrap();
    let ca = connection(&a, BackendTarget::Named("selected".into()));
    let cb = connection(&b, BackendTarget::Named("selected".into()));
    let response = |path| json!({"projects":[{"id":"same","path":path}]});
    let sa = server(
        a,
        vec![
            (
                json!({"method":"projects.tree","params":{"profile":"work","preview_limit":3}}),
                response("/source-a"),
            ),
            (
                json!({"method":"projects.project_sessions","params":{"profile":"work","project_id":"same"}}),
                json!({"project":{"id":"same","path":"/source-a"}}),
            ),
        ],
    );
    let sb = server(
        b,
        vec![(
            json!({"method":"projects.tree","params":{"profile":"work","preview_limit":3}}),
            response("/source-b"),
        )],
    );
    let mut network = PageNetwork::default();
    network.init(Arc::new(|| {}));
    let backend = || ProfileProvenance::Backend("work".into());
    let tree = || json!({"profile":"work","preview_limit":3});
    assert_eq!(
        run(
            &mut network,
            ca.clone(),
            1,
            "projects.tree",
            tree(),
            backend()
        )
        .unwrap()["projects"][0]["path"],
        "/source-a"
    );
    assert_eq!(
        run(
            &mut network,
            ca,
            1,
            "projects.project_sessions",
            json!({"profile":"work","project_id":"same"}),
            backend()
        )
        .unwrap()["project"]["path"],
        "/source-a"
    );
    assert_eq!(
        run(&mut network, cb, 2, "projects.tree", tree(), backend()).unwrap()["projects"][0]["path"],
        "/source-b"
    );
    sa.join().unwrap();
    sb.join().unwrap();
}

#[test]
fn projects_page_network_validates_native_root_before_project_rpc() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proof = BackendRootProof {
        profile: None,
        profile_id: "/private/root".into(),
        instance_id: "fixture-instance".into(),
        authority_epoch: 7,
    };
    let connection = connection(&listener, BackendTarget::Root(proof));
    let server = thread::spawn(move || {
        for valid in [true, false] {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket =
                tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                    assert_eq!(request.headers()["x-access-fixture"], "private-access");
                    let url = Url::parse(&format!("http://private{}", request.uri())).unwrap();
                    assert!(
                        url.query_pairs()
                            .any(|(k, v)| k == "token" && v == "private-session")
                    );
                    Ok(response)
                })
                .unwrap();
            let describe: Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(describe["method"], "runtime.describe");
            let epoch = if valid { 7 } else { 8 };
            socket.send(Message::Text(json!({"jsonrpc":"2.0","id":describe["id"],"result":{"profile_id":"/private/root","instance_id":"fixture-instance","authority_epoch":epoch}}).to_string().into())).unwrap();
            if valid {
                let request: Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                assert_eq!(request["method"], "projects.tree");
                assert_eq!(request["params"], json!({"preview_limit":3}));
                socket
                    .send(Message::Text(
                        json!({"jsonrpc":"2.0","id":request["id"],"result":{"projects":[]}})
                            .to_string()
                            .into(),
                    ))
                    .unwrap();
            } else {
                assert!(socket.read().is_err());
            }
        }
    });
    let mut network = PageNetwork::default();
    network.init(Arc::new(|| {}));
    let body = || json!({"profile":"work","preview_limit":3});
    assert!(
        run(
            &mut network,
            connection.clone(),
            1,
            "projects.tree",
            body(),
            ProfileProvenance::NativeSelf
        )
        .is_ok()
    );
    assert!(
        run(
            &mut network,
            connection,
            1,
            "projects.tree",
            body(),
            ProfileProvenance::NativeSelf
        )
        .unwrap_err()
        .contains("authority changed")
    );
    server.join().unwrap();
}

#[test]
fn unsupported_project_rpc_is_rejected_before_rpc_io() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = connection(&listener, BackendTarget::Named("selected".into()));
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        assert!(socket.read().is_err());
    });
    let mut network = PageNetwork::default();
    network.init(Arc::new(|| {}));
    let error = run(
        &mut network,
        connection,
        1,
        "projects.arbitrary",
        json!({"profile":"work"}),
        ProfileProvenance::Backend("work".into()),
    )
    .unwrap_err();
    assert!(error.contains("not part of the native workspace API"));
    server.join().unwrap();
}

#[test]
fn retained_projects_requests_hydrate_tree_and_entered_sessions_through_page_network() {
    use crate::projects::{LoadKind, Owner, Scope, State};
    use crate::source::{Binding, SessionOwner};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("private-session".into()),
    };
    let binding = Binding::new(connection.clone(), None).unwrap();
    let server = thread::spawn(move || {
        for (method, params, result) in [
            (
                "projects.tree",
                json!({"profile":"work","preview_limit":3}),
                json!({"projects":[{"id":"same","label":"Fixture","path":"/private/project"}]}),
            ),
            (
                "projects.project_sessions",
                json!({"profile":"work","project_id":"same"}),
                json!({"project":{"id":"same","label":"Fixture","path":"/private/project","sessions":[]}}),
            ),
        ] {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket =
                tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                    let url = Url::parse(&format!("http://private{}", request.uri())).unwrap();
                    assert_eq!(url.path(), "/api/ws");
                    assert!(
                        url.query_pairs()
                            .any(|(k, v)| k == "token" && v == "private-session")
                    );
                    Ok(response)
                })
                .unwrap();
            let request: Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(request["method"], method);
            assert_eq!(request["params"], params);
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
    });
    let mut state = State::default();
    state.bind(Owner {
        session: SessionOwner::new(binding.stamp().clone(), "work"),
        epoch: 1,
    });
    let mut network = PageNetwork::default();
    network.init(Arc::new(|| {}));
    let scope = scope(1);
    for kind in [LoadKind::Tree, LoadKind::Entered("same".into())] {
        if matches!(kind, LoadKind::Entered(_)) {
            state.select(Scope::Project("same".into()));
        }
        let (ticket, request) = state.begin(kind, &binding).unwrap();
        network
            .request(&scope, connection.clone(), "projects", request)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            if let Some(reply) = network
                .poll(&scope)
                .into_iter()
                .find(|reply| !reply.progress)
            {
                break reply.result;
            }
            assert!(Instant::now() < deadline, "missing retained project reply");
            thread::sleep(Duration::from_millis(2));
        };
        assert!(state.finish(ticket, &binding, result));
        assert!(state.error.is_none());
    }
    assert!(state.tree.is_some());
    assert!(state.entered.is_some());
    server.join().unwrap();
}

#[test]
fn native_root_create_requires_matching_authority_before_rpc() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = connection(
        &listener,
        BackendTarget::Root(BackendRootProof {
            profile: None,
            profile_id: "/private/root".into(),
            instance_id: "fixture-instance".into(),
            authority_epoch: 7,
        }),
    );
    let server = thread::spawn(move || {
        for valid in [true, false] {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket =
                tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                    assert_eq!(request.headers()["x-access-fixture"], "private-access");
                    let url = Url::parse(&format!("http://private{}", request.uri())).unwrap();
                    assert!(
                        url.query_pairs()
                            .any(|(k, v)| k == "token" && v == "private-session")
                    );
                    Ok(response)
                })
                .unwrap();
            let describe: Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(describe["method"], "runtime.describe");
            let epoch = if valid { 7 } else { 8 };
            socket.send(Message::Text(json!({"jsonrpc":"2.0","id":describe["id"],"result":{"profile_id":"/private/root","instance_id":"fixture-instance","authority_epoch":epoch}}).to_string().into())).unwrap();
            if valid {
                let request: Value =
                    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
                assert_eq!(request["method"], "projects.create");
                assert_eq!(
                    request["params"],
                    json!({"name":"First","folders":["/fixture/project"],"use":true})
                );
                socket.send(Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"result":{"project":{"id":"same"}}}).to_string().into())).unwrap();
            } else {
                assert!(socket.read().is_err());
            }
        }
    });
    let mut network = PageNetwork::default();
    network.init(Arc::new(|| {}));
    let body =
        || json!({"profile":"work","name":"First","folders":["/fixture/project"],"use":true});
    assert!(
        run(
            &mut network,
            connection.clone(),
            1,
            "projects.create",
            body(),
            ProfileProvenance::NativeSelf
        )
        .is_ok()
    );
    assert!(
        run(
            &mut network,
            connection,
            1,
            "projects.create",
            body(),
            ProfileProvenance::NativeSelf
        )
        .unwrap_err()
        .contains("authority changed")
    );
    server.join().unwrap();
}
