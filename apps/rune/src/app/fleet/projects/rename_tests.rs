use super::*;
use rune::{Cx, El, Env, Key, KeyEvent, Mods, Runtime};
use serde_json::{Value, json};
use std::{
    net::TcpListener,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

fn view(s: &Viewer, _: &mut Cx<Viewer>) -> El {
    let mut layers = Vec::new();
    let body = crate::view::projects::navigation(
        s,
        crate::view::projects::Actions {
            select: Viewer::project_select,
            retry: Viewer::project_retry,
            open_session: Viewer::project_open_session,
            open_preview: Viewer::project_open_preview,
            toggle: Viewer::project_toggle,
            expand: Viewer::project_expand,
            reveal: Viewer::project_reveal,
        },
        &rune_theme_opencode::kit(true),
        &mut layers,
    );
    rune::col().size(700., 700.).child(body).child(
        rune::el()
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .children(layers),
    )
}
fn viewer(endpoint: String) -> Viewer {
    let mut v = Viewer::empty();
    v.connection = Some(Connection::Url {
        endpoint,
        token: Some("fixture".into()),
    });
    v.status = "Connected".into();
    v.profile = "work".into();
    v.connection_epoch = 1;
    v.fleet_state.projects_network.init(Arc::new(|| {}));
    v.fleet_state.project_rename_network.init(Arc::new(|| {}));
    v.fleet_state.project_active_network.init(Arc::new(|| {}));
    seed(&mut v, "Original");
    v
}
fn seed(v: &mut Viewer, label: &str) {
    let (owner, binding) = v.project_owner().unwrap();
    v.fleet_state.projects.bind(owner);
    let (ticket, _) = v
        .fleet_state
        .projects
        .begin(LoadKind::Tree, &binding)
        .unwrap();
    assert!(v.fleet_state.projects.finish(
        ticket,
        &binding,
        Ok(json!({"projects":[{"id":"same","label":label}]}))
    ));
}
fn click(rt: &mut Runtime<Viewer>, key: &str) {
    rt.invalidate();
    rt.frame(1.);
    if key == "project-actions-same" {
        let (x, y) = rt.find("project-header-same").unwrap();
        rt.pointer_move(x, y);
        rt.frame(1.);
    }
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.);
}
fn wait(v: &mut Viewer, predicate: impl Fn(&Viewer) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate(v) {
        v.poll_projects();
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn read(socket: &mut tungstenite::WebSocket<std::net::TcpStream>) -> Value {
    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap()
}
fn accept(listener: &TcpListener) -> tungstenite::WebSocket<std::net::TcpStream> {
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    tungstenite::accept_hdr(
        stream,
        |req: &tungstenite::handshake::server::Request,
         res: tungstenite::handshake::server::Response| {
            assert!(
                req.uri()
                    .query()
                    .unwrap_or_default()
                    .contains("token=fixture")
            );
            Ok(res)
        },
    )
    .unwrap()
}
fn reply(socket: &mut tungstenite::WebSocket<std::net::TcpStream>, request: &Value, result: Value) {
    socket
        .send(tungstenite::Message::Text(
            json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                .to_string()
                .into(),
        ))
        .unwrap();
}
#[test]
fn runtime_rename_dispatches_and_refreshes_authoritative_tree() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.update");
        assert_eq!(
            request["params"],
            json!({"profile":"work","id":"same","name":"New name"})
        );
        reply(
            &mut socket,
            &request,
            json!({"project":{"id":"same","name":"New name"}}),
        );
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.tree");
        reply(
            &mut socket,
            &request,
            json!({"projects":[{"id":"same","label":"New name"}]}),
        );
    });
    let mut rt = Runtime::new(viewer(endpoint), view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item0");
    assert_eq!(
        rt.state.project_rename_form().unwrap().name.text,
        "Original"
    );
    assert_eq!(
        rt.state.project_rename_form().unwrap().name.selected(),
        "Original"
    );
    rt.update(|v, _| v.project_rename_name_mut().replace_text("  New name  "));
    rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    rt.frame(1.);
    wait(&mut rt.state, |v| {
        v.projects_state()
            .tree
            .as_ref()
            .is_some_and(|t| t.projects[0].label == "New name")
    });
    assert!(rt.state.project_rename_form().is_none());
    server.join().unwrap();
}
#[test]
fn scoped_menu_dialog_cancel_and_blank_name() {
    let mut v = viewer("http://127.0.0.1:1".into());
    let project = v.fleet_state.projects.tree.as_ref().unwrap().projects[0].clone();
    v.fleet_state.projects.select(Scope::Project("same".into()));
    v.fleet_state.projects.entered = Some(project);
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    rt.frame(1.);
    let (header_x, header_y) = rt.find("project-header-same").unwrap();
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item0");
    assert_eq!(
        rt.state.project_rename_form().unwrap().name.text,
        "Original"
    );
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("input") && e.focused)
    );
    rt.pointer_down(header_x, header_y);
    rt.pointer_up(header_x, header_y);
    rt.frame(1.);
    assert!(rt.state.project_rename_form().is_some());
    assert_eq!(
        rt.state.projects_state().scope,
        Scope::Project("same".into())
    );
    rt.update(|v, _| v.project_rename_name_mut().replace_text("   "));
    rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    rt.frame(1.);
    assert!(!rt.state.project_rename_busy());
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    rt.frame(1.);
    assert!(rt.state.project_rename_form().is_none());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("project-actions-same") && e.focused),
        "focus returns to project actions"
    );
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item0");
    click(&mut rt, "project-rename-cancel");
    assert!(rt.state.project_rename_form().is_none());
}
#[test]
fn overview_context_menu_opens_rename_dialog() {
    let mut rt = Runtime::new(
        viewer("http://127.0.0.1:1".into()),
        view,
        (700., 700.),
        Env::new(true, 1.),
    );
    rt.frame(1.);
    let (x, y) = rt.find("project-header-same").unwrap();
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    rt.frame(1.);
    click(&mut rt, "project-actions-same-item0");
    assert_eq!(
        rt.state.project_rename_form().unwrap().name.text,
        "Original"
    );
}
#[test]
fn menu_capture_rejects_leave_and_reenter_same_project() {
    let mut v = viewer("http://127.0.0.1:1".into());
    let owner = v.projects_state().owner.clone().unwrap();
    let generation = v.projects_state().generation();
    v.project_rename_menu_open(owner.clone(), Scope::All, generation, "same".into(), None);
    let captured = v.project_rename_menu().unwrap().clone();
    v.fleet_state.projects.select(Scope::Project("same".into()));
    v.fleet_state.projects.select(Scope::All);
    v.project_rename_open(
        captured.owner,
        captured.scope,
        captured.generation,
        captured.id,
    );
    assert!(v.project_rename_form().is_none());
}
#[test]
fn closing_pending_dialog_retains_dispatched_write_and_refresh() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.update");
        seen_tx.send(()).unwrap();
        go_rx.recv().unwrap();
        reply(&mut socket, &request, json!({"project":{"id":"same"}}));
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.tree");
        reply(
            &mut socket,
            &request,
            json!({"projects":[{"id":"same","label":"Saved"}]}),
        );
    });
    let mut rt = Runtime::new(viewer(endpoint), view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item0");
    click(&mut rt, "project-rename-submit");
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    rt.frame(1.);
    assert!(rt.state.project_rename_form().is_none());
    assert!(rt.state.project_rename_busy());
    go_tx.send(()).unwrap();
    wait(&mut rt.state, |v| {
        v.projects_state()
            .tree
            .as_ref()
            .is_some_and(|t| t.projects[0].label == "Saved")
    });
    server.join().unwrap();
}
#[test]
fn stale_before_submit_and_late_receipt_do_not_touch_other_source() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["params"]["id"], "same");
        seen_tx.send(()).unwrap();
        go_rx.recv().unwrap();
        reply(&mut socket, &request, json!({"project":{"id":"same"}}));
    });
    let mut rt = Runtime::new(
        viewer(endpoint.clone()),
        view,
        (700., 700.),
        Env::new(true, 1.),
    );
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item0");
    rt.update(|v, _| {
        v.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: Some("fixture".into()),
        });
        v.connection_epoch += 1;
        seed(v, "Other");
        v.project_rename_submit();
    });
    assert!(!rt.state.project_rename_busy());
    rt.update(|v, _| {
        v.connection = Some(Connection::Url {
            endpoint,
            token: Some("fixture".into()),
        });
        v.connection_epoch += 1;
        seed(v, "Original");
    });
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item0");
    click(&mut rt, "project-rename-submit");
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    rt.update(|v, _| {
        v.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: Some("fixture".into()),
        });
        v.connection_epoch += 1;
        seed(v, "Other");
    });
    go_tx.send(()).unwrap();
    wait(&mut rt.state, |v| !v.project_rename_busy());
    assert_eq!(
        rt.state.projects_state().tree.as_ref().unwrap().projects[0].label,
        "Other"
    );
    assert!(
        rt.state
            .fleet_state
            .project_rename_result
            .as_ref()
            .unwrap()
            .contains("previous")
    );
    server.join().unwrap();
}
#[test]
fn lost_response_keeps_form_without_automatic_duplicate() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.update");
        drop(socket);
        listener.set_nonblocking(true).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    });
    let mut rt = Runtime::new(viewer(endpoint), view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item0");
    rt.update(|v, _| v.project_rename_name_mut().replace_text("Changed"));
    click(&mut rt, "project-rename-submit");
    wait(&mut rt.state, |v| !v.project_rename_busy());
    let form = rt.state.project_rename_form().unwrap();
    assert_eq!(form.name.text, "Changed");
    assert!(form.error.is_some());
    server.join().unwrap();
}

#[test]
fn runtime_project_menu_exposes_set_active() {
    let mut rt = Runtime::new(
        viewer("http://127.0.0.1:1".into()),
        view,
        (700., 700.),
        Env::new(true, 1.),
    );
    click(&mut rt, "project-actions-same");
    assert!(
        rt.find("project-actions-same-item2").is_some(),
        "missing Set active action"
    );
}

#[test]
fn runtime_active_menu_writes_and_refreshes_then_clears() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for active in [Some("same"), None] {
            let mut socket = accept(&listener);
            let request = read(&mut socket);
            assert_eq!(request["method"], "projects.set_active");
            assert_eq!(request["params"], json!({"profile":"work","id":active}));
            reply(&mut socket, &request, json!({"active_id":active}));
            let mut socket = accept(&listener);
            let request = read(&mut socket);
            assert_eq!(request["method"], "projects.tree");
            reply(
                &mut socket,
                &request,
                json!({"active_id":active,"projects":[{"id":"same","label":"Original"}]}),
            );
        }
    });
    let mut rt = Runtime::new(viewer(endpoint), view, (700., 700.), Env::new(true, 1.));
    for active in [Some("same"), None] {
        click(&mut rt, "project-actions-same");
        click(&mut rt, "project-actions-same-item2");
        wait(&mut rt.state, |v| {
            !v.project_active_busy()
                && v.projects_state()
                    .tree
                    .as_ref()
                    .is_some_and(|t| t.active_id.as_deref() == active)
        });
    }
    server.join().unwrap();
}

#[test]
fn runtime_active_late_reply_is_not_published_to_equal_id_other_source() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.set_active");
        seen_tx.send(()).unwrap();
        go_rx.recv().unwrap();
        reply(&mut socket, &request, json!({"active_id":"same"}));
    });
    let mut rt = Runtime::new(viewer(endpoint), view, (700., 700.), Env::new(true, 1.));
    rt.state.draft_origin = Some(crate::composer::DraftOrigin::new());
    let origin = rt.state.draft_origin.clone();
    let old_scope = rt.state.composer_scope();
    let draft = rt.state.composer.draft_mut(old_scope.clone());
    draft.cwd = Some("/captured-A".into());
    draft.cwd_explicit = true;
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item2");
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    rt.update(|v, _| {
        v.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: Some("fixture".into()),
        });
        v.connection_epoch += 1;
        seed(v, "Other");
    });
    go_tx.send(()).unwrap();
    wait(&mut rt.state, |v| !v.project_active_busy());
    assert_eq!(
        rt.state.projects_state().tree.as_ref().unwrap().active_id,
        None
    );
    assert_eq!(rt.state.draft_origin, origin);
    let draft = rt.state.composer.draft(&old_scope).unwrap();
    assert_eq!(draft.cwd.as_deref(), Some("/captured-A"));
    assert!(draft.cwd_explicit);
    assert!(
        rt.state
            .project_active_result()
            .unwrap()
            .contains("previous")
    );
    server.join().unwrap();
}
#[test]
fn runtime_active_lost_reply_is_not_replayed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.set_active");
        drop(socket);
        listener.set_nonblocking(true).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    });
    let mut rt = Runtime::new(viewer(endpoint), view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item2");
    wait(&mut rt.state, |v| !v.project_active_busy());
    assert!(
        rt.state
            .project_active_result()
            .unwrap()
            .contains("uncertain")
    );
    server.join().unwrap();
}

#[test]
fn runtime_active_native_self_proves_root_and_omits_profile() {
    use crate::gateway::{
        BackendRootProof,
        prepared::{BackendTarget, SelectedContext, WirePolicy},
        route,
    };
    use std::collections::BTreeMap;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
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
    .with_selected(
        SelectedContext::new(
            "default",
            BackendTarget::Root(BackendRootProof {
                profile: None,
                profile_id: "/private/root".into(),
                instance_id: "fixture-instance".into(),
                authority_epoch: 7,
            }),
        )
        .unwrap(),
    )
    .unwrap();
    let connection = Connection::Routed {
        actual: Box::new(Connection::Url {
            endpoint: format!("http://{}/hermes/", listener.local_addr().unwrap()),
            token: Some("private-session".into()),
        }),
        policy: Arc::new(policy),
        owner: Arc::new(crate::source::RouteOwner::legacy("active-private-test").unwrap()),
    };
    let server = std::thread::spawn(move || {
        for index in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept_hdr(
                stream,
                |req: &tungstenite::handshake::server::Request,
                 res: tungstenite::handshake::server::Response| {
                    assert_eq!(req.headers()["x-access-fixture"], "private-access");
                    assert!(
                        req.uri()
                            .query()
                            .unwrap_or_default()
                            .contains("token=private-session")
                    );
                    Ok(res)
                },
            )
            .unwrap();
            let describe = read(&mut socket);
            assert_eq!(describe["method"], "runtime.describe");
            reply(
                &mut socket,
                &describe,
                json!({"profile_id":"/private/root","instance_id":"fixture-instance","authority_epoch":7}),
            );
            let request = read(&mut socket);
            if index == 0 {
                assert_eq!(request["method"], "projects.set_active");
                assert_eq!(request["params"], json!({"id":"same"}));
                reply(&mut socket, &request, json!({"active_id":"same"}));
            } else {
                assert_eq!(request["method"], "projects.tree");
                assert_eq!(request["params"], json!({"preview_limit":3}));
                reply(
                    &mut socket,
                    &request,
                    json!({"active_id":"same","projects":[{"id":"same","label":"Original"}]}),
                );
            }
        }
    });
    let mut fixture = super::super::create::integration_tests::NativeSelfViewer::new(connection);
    fixture
        .viewer
        .fleet_state
        .projects_network
        .init(Arc::new(|| {}));
    fixture
        .viewer
        .fleet_state
        .project_active_network
        .init(Arc::new(|| {}));
    seed(&mut fixture.viewer, "Original");
    let viewer = std::mem::replace(&mut fixture.viewer, Viewer::empty());
    let mut rt = Runtime::new(viewer, view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item2");
    wait(&mut rt.state, |v| {
        v.projects_state()
            .tree
            .as_ref()
            .is_some_and(|t| t.active_id.as_deref() == Some("same"))
    });
    server.join().unwrap();
    drop(rt);
    drop(fixture);
}
