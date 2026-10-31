use super::*;
use rune::{Cx, El, Env, Runtime};
use std::sync::Arc;

fn view(v: &Viewer, _: &mut Cx<Viewer>) -> El {
    let mut layers = Vec::new();
    let body = crate::view::projects::navigation(
        v,
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
    rune::col().size(700., 700.).child(body).children(layers)
}
#[test]
fn local_menu_offers_add_folder() {
    let mut v = Viewer::empty();
    v.connection = Some(Connection::Local {
        home: "/tmp".into(),
    });
    v.status = "Connected".into();
    v.profile = "work".into();
    v.connection_epoch = 1;
    v.fleet_state.projects_network.init(Arc::new(|| {}));
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
        Ok(serde_json::json!({"projects":[{"id":"same","label":"Original"}]}))
    ));
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    rt.frame(1.);
    let (x, y) = rt.find("project-header-same").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1.);
    let (x, y) = rt.find("project-actions-same").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.);
    assert!(
        rt.find("project-actions-same-item1").is_some(),
        "missing Add folder menu action"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn local_presenter_writes_each_path_and_refreshes_tree() {
    use crate::gateway::{
        BackendRootProof,
        prepared::{BackendTarget, SelectedContext, WirePolicy},
        route,
    };
    use rune::file_dialog::TestPresenter;
    use std::{
        cell::RefCell,
        future::poll_fn,
        io::{BufRead, BufReader, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
        rc::Rc,
        task::Poll,
        time::{Duration, Instant},
    };
    struct Home(std::path::PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let home = Home(std::env::temp_dir().join(format!("rune-folders-{}", std::process::id())));
    std::fs::create_dir(&home.0).unwrap();
    std::fs::set_permissions(&home.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = std::fs::canonicalize(&home.0).unwrap();
    let control = UnixListener::bind(path.join("gateway.sock")).unwrap();
    std::fs::set_permissions(
        path.join("gateway.sock"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let profile_id = path.to_string_lossy().into_owned();
    let peer_id = profile_id.clone();
    let control_thread = std::thread::spawn(move || {
        for _ in 0..3 {
            for verb in ["identify", "session-ticket"] {
                let (mut stream, _) = control.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["verb"], verb);
                let result = if verb == "identify" {
                    serde_json::json!({"pid":std::process::id(),"runtime_protocol":1,"state":"ready","authority_epoch":7,"instance_id":"fixture","capabilities":["session-authority-v1"],"api_origin":origin,"served_profiles":[{"home":peer_id,"profile_id":peer_id}]})
                } else {
                    serde_json::json!({"profile_id":peer_id,"instance_id":"fixture","ticket":"folder-test"})
                };
                writeln!(
                    stream,
                    "{}",
                    serde_json::json!({"protocol":1,"id":1,"ok":true,"result":result})
                )
                .unwrap();
            }
        }
    });
    let peer_id = profile_id.clone();
    let peer = std::thread::spawn(move || {
        for index in 0..3 {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut ws = tungstenite::accept_hdr(
                stream,
                |req: &tungstenite::handshake::server::Request,
                 mut res: tungstenite::handshake::server::Response| {
                    assert_eq!(
                        req.headers()["Sec-WebSocket-Protocol"],
                        "hermes-gateway-v1, hermes-gateway-ticket.folder-test"
                    );
                    res.headers_mut().insert(
                        "Sec-WebSocket-Protocol",
                        "hermes-gateway-v1".parse().unwrap(),
                    );
                    Ok(res)
                },
            )
            .unwrap();
            let read = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>| -> serde_json::Value {
                serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap()
            };
            let describe = read(&mut ws);
            assert_eq!(describe["method"], "runtime.describe");
            ws.send(tungstenite::Message::Text(serde_json::json!({"jsonrpc":"2.0","id":describe["id"],"result":{"profile_id":peer_id,"instance_id":"fixture","authority_epoch":7}}).to_string().into())).unwrap();
            let req = read(&mut ws);
            let result = if index < 2 {
                assert_eq!(req["method"], "projects.add_folder");
                assert_eq!(
                    req["params"],
                    serde_json::json!({"id":"same","path":if index == 0 {"/A"} else {"/B"},"is_primary":false})
                );
                serde_json::json!({"project":{"id":"same"}})
            } else {
                assert_eq!(req["method"], "projects.tree");
                serde_json::json!({"projects":[{"id":"same","label":"Refreshed"}]})
            };
            ws.send(tungstenite::Message::Text(
                serde_json::json!({"jsonrpc":"2.0","id":req["id"],"result":result})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        }
    });
    let policy = WirePolicy::native(
        route::ProfilePolicy::new("default", route::RouteOptions::default()).unwrap(),
    )
    .with_selected(
        SelectedContext::new(
            "default",
            BackendTarget::Root(BackendRootProof {
                profile: None,
                profile_id,
                instance_id: "fixture".into(),
                authority_epoch: 7,
            }),
        )
        .unwrap(),
    )
    .unwrap();
    let connection = Connection::Routed {
        actual: Box::new(Connection::Local { home: path }),
        policy: Arc::new(policy),
        owner: Arc::new(crate::source::RouteOwner::legacy("folder-test").unwrap()),
    };
    let mut fixture = super::create::integration_tests::NativeSelfViewer::new(connection);
    let v = &mut fixture.viewer;
    v.fleet_state.projects_network.init(Arc::new(|| {}));
    v.fleet_state.project_folders_network.init(Arc::new(|| {}));
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
        Ok(serde_json::json!({"projects":[{"id":"same","label":"Original"}]}))
    ));
    let viewer = std::mem::replace(v, Viewer::empty());
    let mut rt = Runtime::new(viewer, view, (700., 700.), Env::new(true, 1.));
    let click = |rt: &mut Runtime<Viewer>, key: &str| {
        rt.frame(1.);
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.);
    };
    rt.frame(1.);
    let (x, y) = rt.find("project-header-same").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1.);
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item1");
    click(&mut rt, "project-folders-choose");
    rt.update(|v, tx| {
        v.poll_tools(1., tx);
    });
    let mut presenter = TestPresenter::new(Arc::new(|| {}));
    let result = Rc::new(RefCell::new(None));
    let input = result.clone();
    let (_, kind, _) = presenter.open(
        &mut rt,
        poll_fn(move |_| match input.borrow_mut().take() {
            Some(v) => Poll::Ready(v),
            None => Poll::Pending,
        }),
    );
    assert_eq!(kind, rune::file_dialog::FileDialogKind::Folders);
    *result.borrow_mut() = Some(Ok(Some(vec!["/A".into(), "/B".into(), "/A".into()])));
    assert!(presenter.answer(&mut rt));
    rt.frame(1.);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !rt
        .state
        .projects_state()
        .tree
        .as_ref()
        .is_some_and(|t| t.projects.iter().any(|p| p.label == "Refreshed"))
    {
        rt.state.poll_projects();
        assert!(
            Instant::now() < deadline,
            "timed out: {:?}",
            rt.state.project_folders_error()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    control_thread.join().unwrap();
    peer.join().unwrap();
}

#[test]
fn stale_presenter_callback_cannot_write_to_replacement_project() {
    use rune::file_dialog::TestPresenter;
    use std::{cell::RefCell, future::poll_fn, rc::Rc, task::Poll};
    let mut v = Viewer::empty();
    v.connection = Some(Connection::Local {
        home: "/tmp".into(),
    });
    v.status = "Connected".into();
    v.profile = "work".into();
    v.connection_epoch = 1;
    let (owner, binding) = v.project_owner().unwrap();
    v.fleet_state.projects.bind(owner.clone());
    let (ticket, _) = v
        .fleet_state
        .projects
        .begin(LoadKind::Tree, &binding)
        .unwrap();
    assert!(v.fleet_state.projects.finish(
        ticket,
        &binding,
        Ok(serde_json::json!({"projects":[{"id":"same","label":"A"}]}))
    ));
    let generation = v.projects_state().generation();
    v.project_folders_open(owner, Scope::All, generation, "same".into());
    v.project_folders_choose();
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    rt.update(|v, tx| {
        v.poll_tools(1., tx);
    });
    let mut presenter = TestPresenter::new(Arc::new(|| {}));
    let result = Rc::new(RefCell::new(None));
    let input = result.clone();
    let _ = presenter.open(
        &mut rt,
        poll_fn(move |_| match input.borrow_mut().take() {
            Some(v) => Poll::Ready(v),
            None => Poll::Pending,
        }),
    );
    rt.state.connection_epoch += 1;
    let (owner, binding) = rt.state.project_owner().unwrap();
    rt.state.fleet_state.projects.bind(owner);
    let (ticket, _) = rt
        .state
        .fleet_state
        .projects
        .begin(LoadKind::Tree, &binding)
        .unwrap();
    assert!(rt.state.fleet_state.projects.finish(
        ticket,
        &binding,
        Ok(serde_json::json!({"projects":[{"id":"same","label":"B"}]}))
    ));
    *result.borrow_mut() = Some(Ok(Some(vec!["/A".into()])));
    assert!(presenter.answer(&mut rt));
    rt.frame(1.);
    assert!(rt.state.fleet_state.project_folders_pending.is_none());
    assert!(rt.state.project_folders_error().is_none());
    assert!(rt.state.project_folders_result().is_none());
    assert_eq!(
        rt.state.projects_state().tree.as_ref().unwrap().projects[0].label,
        "B"
    );
}

#[test]
fn remote_menu_offers_add_folder() {
    let mut v = Viewer::empty();
    v.connection = Some(Connection::Url {
        endpoint: "http://127.0.0.1:1".into(),
        token: None,
    });
    v.status = "Connected".into();
    v.profile = "work".into();
    v.connection_epoch = 1;
    v.fleet_state.projects_network.init(Arc::new(|| {}));
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
        Ok(serde_json::json!({"projects":[{"id":"same","label":"Original"}]}))
    ));
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    rt.frame(1.);
    let (x, y) = rt.find("project-header-same").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1.);
    let (x, y) = rt.find("project-actions-same").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.);
    assert!(rt.find("project-actions-same-item1").is_some());
}

#[test]
fn remote_runtime_picker_reads_and_writes_captured_source() {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let peer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        for index in 0..4 {
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("peer accept {index}: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut ws = tungstenite::accept_hdr(
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
            .unwrap();
            if index < 2 {
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(e) => panic!("HTTP accept {index}: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(
                    line.starts_with(if index == 0 {
                        "GET /api/fs/default-cwd?"
                    } else {
                        "GET /api/fs/list?"
                    }),
                    "{line}"
                );
                assert!(line.contains("profile=work"), "{line}");
                if index == 1 {
                    assert!(line.contains("path=%2Fremote"), "{line}");
                }
                let mut headers = String::new();
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" {
                        break;
                    }
                    headers.push_str(&h);
                }
                assert!(line.contains("token=fixture"), "{line}");
                let body = if index == 0 {
                    r#"{"cwd":"/remote"}"#
                } else {
                    r#"{"entries":[]}"#
                };
                write!(reader.get_mut(), "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            } else {
                let req: serde_json::Value =
                    serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
                let result = if index == 2 {
                    assert_eq!(req["method"], "projects.add_folder");
                    assert_eq!(
                        req["params"],
                        serde_json::json!({"profile":"work","id":"same","path":"/remote","is_primary":false})
                    );
                    serde_json::json!({"project":{"id":"same"}})
                } else {
                    assert_eq!(req["method"], "projects.tree");
                    serde_json::json!({"projects":[{"id":"same","label":"Refreshed"}]})
                };
                ws.send(tungstenite::Message::Text(
                    serde_json::json!({"jsonrpc":"2.0","id":req["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            }
        }
    });
    let mut v = Viewer::empty();
    v.connection = Some(Connection::Url {
        endpoint,
        token: Some("fixture".into()),
    });
    v.status = "Connected".into();
    v.profile = "work".into();
    v.connection_epoch = 1;
    let page_owner = (v.connection_epoch, v.profile.clone(), v.profile_intent);
    v.page_accepted_owner = Some(page_owner.clone());
    v.page_loaded_owner = Some(page_owner);
    v.page_network.init(Arc::new(|| {}));
    v.fleet_state.projects_network.init(Arc::new(|| {}));
    v.fleet_state.project_folders_network.init(Arc::new(|| {}));
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
        Ok(serde_json::json!({"projects":[{"id":"same","label":"Original"}]}))
    ));
    fn remote_view(v: &Viewer, cx: &mut Cx<Viewer>) -> El {
        view(v, cx).children(crate::view::directories::panel(
            v,
            &rune_theme_opencode::kit(true),
        ))
    }
    let mut rt = Runtime::new(v, remote_view, (700., 700.), Env::new(true, 1.));
    let click = |rt: &mut Runtime<Viewer>, key: &str| {
        rt.invalidate();
        rt.frame(1.);
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.);
    };
    rt.frame(1.);
    let (x, y) = rt.find("project-header-same").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1.);
    click(&mut rt, "project-actions-same");
    click(&mut rt, "project-actions-same-item1");
    click(&mut rt, "project-folders-choose");
    rt.update(|v, tx| {
        v.poll_tools(1., tx);
    });
    assert!(
        rt.file_dialog().is_none(),
        "remote picker requested a native dialog"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while rt.state.directories.loading {
        let replies = rt.state.page_network.poll(&rt.state.composer_scope());
        for reply in replies {
            rt.state.apply_page_result(&reply.operation, reply.result);
        }
        assert!(
            Instant::now() < deadline,
            "picker timeout: {:?}",
            rt.state.directories.error
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        rt.state.directories.path.text, "/remote",
        "picker error: {:?}",
        rt.state.directories.error
    );
    rt.invalidate();
    rt.frame(1.);
    let item = rt
        .elements()
        .into_iter()
        .find(|e| {
            e.role == "button" && e.label.as_deref() == Some("Use this directory") && !e.exiting
        })
        .unwrap();
    let (x, y) = (
        item.rect[0] + item.rect[2] / 2.,
        item.rect[1] + item.rect[3] / 2.,
    );
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.);
    while !rt
        .state
        .projects_state()
        .tree
        .as_ref()
        .is_some_and(|t| t.projects.iter().any(|p| p.label == "Refreshed"))
    {
        rt.state.poll_projects();
        assert!(
            Instant::now() < deadline,
            "write timeout: {:?}",
            rt.state.project_folders_error()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    peer.join().unwrap();
}

#[test]
fn stale_remote_picker_callbacks_cannot_target_replacement() {
    for action in [
        "Use this directory",
        "Cancel",
        "↑ Parent directory",
        "▸ child",
        "edit",
        "submit",
    ] {
        let mut v = Viewer::empty();
        v.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: Some("a".into()),
        });
        v.status = "Connected".into();
        v.profile = "work".into();
        v.connection_epoch = 1;
        fn bind(v: &mut Viewer) {
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
                Ok(serde_json::json!({"projects":[{"id":"same","label":"Project"}]}))
            ));
        }
        fn pick(v: &mut Viewer) {
            let owner = v.projects_state().owner.clone().unwrap();
            v.project_folders_open(
                owner,
                v.projects_state().scope.clone(),
                v.projects_state().generation(),
                "same".into(),
            );
            v.project_folders_choose();
            let c = v.fleet_state.project_folders_choose.take().unwrap();
            v.directories.open(
                c.request_scope.clone(),
                false,
                "/same/path".into(),
                crate::directories::Purpose::ProjectFolders(c.dialog, c.token),
            );
            v.directories.loading = false;
            v.directories.entries = vec![crate::files::Entry {
                name: "child".into(),
                path: "/same/path/child".into(),
                directory: true,
                ignored: false,
            }];
        }
        fn panel(v: &Viewer, _: &mut Cx<Viewer>) -> El {
            rune::col()
                .size(700., 700.)
                .children(crate::view::directories::panel(
                    v,
                    &rune_theme_opencode::kit(true),
                ))
        }
        bind(&mut v);
        pick(&mut v);
        let mut rt = Runtime::new(v, panel, (700., 700.), Env::new(true, 1.));
        rt.frame(1.);
        let point = if action == "edit" || action == "submit" {
            let p = rt.find("input").unwrap();
            rt.pointer_down(p.0, p.1);
            rt.pointer_up(p.0, p.1);
            rt.frame(1.);
            p
        } else {
            let e = rt
                .elements()
                .into_iter()
                .find(|e| e.role == "button" && e.label.as_deref() == Some(action) && !e.exiting)
                .unwrap();
            (e.rect[0] + e.rect[2] / 2., e.rect[1] + e.rect[3] / 2.)
        };
        if action != "edit" && action != "submit" {
            rt.pointer_down(point.0, point.1);
        }
        rt.state.project_folders_cancel();
        rt.state.connection = Some(Connection::Url {
            endpoint: "http://127.0.0.1:2".into(),
            token: Some("b".into()),
        });
        rt.state.connection_epoch = 2;
        bind(&mut rt.state);
        pick(&mut rt.state);
        if action == "edit" {
            rt.paste("foreign");
        } else if action == "submit" {
            rt.key(rune::KeyEvent {
                key: rune::Key::Enter,
                mods: rune::Mods::default(),
            });
        } else {
            rt.pointer_up(point.0, point.1);
        }
        rt.frame(1.);
        assert_eq!(rt.state.directories.path.text, "/same/path", "{action}");
        assert!(!rt.state.directories.loading, "{action}");
        assert!(!rt.state.project_folders_busy(), "{action}");
        assert!(rt.state.project_folders_visible(), "{action}");
    }
}

#[test]
fn partial_failure_and_lost_reply_do_not_replay_confirmed_paths() {
    use std::{
        net::TcpListener,
        sync::mpsc,
        time::{Duration, Instant},
    };
    for lost in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (event_tx, event_rx) = mpsc::channel();
        let peer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            for index in 0..3 {
                let stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(e) => panic!("peer {index}: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut ws = tungstenite::accept(stream).unwrap();
                let req: serde_json::Value =
                    serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
                event_tx.send(req.clone()).unwrap();
                if index == 1 && lost {
                    drop(ws);
                    continue;
                }
                let response = if index == 1 {
                    serde_json::json!({"jsonrpc":"2.0","id":req["id"],"error":{"code":5063,"message":"rejected"}})
                } else {
                    let result = if index == 0 {
                        serde_json::json!({"project":{"id":"same"}})
                    } else {
                        serde_json::json!({"projects":[{"id":"same","label":"Refreshed","path":"/A"}]})
                    };
                    serde_json::json!({"jsonrpc":"2.0","id":req["id"],"result":result})
                };
                ws.send(tungstenite::Message::Text(response.to_string().into()))
                    .unwrap();
            }
            std::thread::sleep(Duration::from_millis(100));
            assert!(listener.accept().is_err(), "unexpected replay");
        });
        let mut v = Viewer::empty();
        v.connection = Some(Connection::Url {
            endpoint,
            token: Some("fixture".into()),
        });
        v.status = "Connected".into();
        v.profile = "work".into();
        v.connection_epoch = 1;
        v.fleet_state.projects_network.init(Arc::new(|| {}));
        v.fleet_state.project_folders_network.init(Arc::new(|| {}));
        let (owner, binding) = v.project_owner().unwrap();
        v.fleet_state.projects.bind(owner.clone());
        let (ticket, _) = v
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        assert!(v.fleet_state.projects.finish(
            ticket,
            &binding,
            Ok(serde_json::json!({"projects":[{"id":"same","label":"Original"}]}))
        ));
        v.project_folders_open(
            owner,
            Scope::All,
            v.projects_state().generation(),
            "same".into(),
        );
        v.project_folders_choose();
        let capture = v.fleet_state.project_folders_choose.take().unwrap();
        v.project_folders_return(capture, vec!["/A".into(), "/B".into(), "/A".into()]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while v.project_folders_busy()
            || !v
                .projects_state()
                .tree
                .as_ref()
                .is_some_and(|t| t.projects.iter().any(|p| p.label == "Refreshed"))
        {
            v.poll_projects();
            assert!(
                Instant::now() < deadline,
                "receipt timeout: {:?}",
                v.project_folders_error()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(v.project_folders_error().unwrap().contains("uncertain"));
        let requests: Vec<_> = (0..3)
            .map(|_| event_rx.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        assert_eq!(requests[0]["method"], "projects.add_folder");
        assert_eq!(requests[0]["params"]["path"], "/A");
        assert_eq!(requests[1]["method"], "projects.add_folder");
        assert_eq!(requests[1]["params"]["path"], "/B");
        assert_eq!(requests[2]["method"], "projects.tree");
        peer.join().unwrap();
    }
}

#[test]
fn held_write_survives_close_and_source_switch_without_touching_equal_id() {
    use std::{
        net::TcpListener,
        sync::mpsc,
        time::{Duration, Instant},
    };
    let a = TcpListener::bind("127.0.0.1:0").unwrap();
    let b = TcpListener::bind("127.0.0.1:0").unwrap();
    b.set_nonblocking(true).unwrap();
    let a_url = format!("http://{}", a.local_addr().unwrap());
    let b_url = format!("http://{}", b.local_addr().unwrap());
    let (seen_tx, seen_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let (stream, _) = a.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let req: serde_json::Value =
            serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(req["method"], "projects.add_folder");
        assert_eq!(req["params"]["path"], "/A");
        seen_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        ws.send(tungstenite::Message::Text(
            serde_json::json!({"jsonrpc":"2.0","id":req["id"],"result":{"project":{"id":"same"}}})
                .to_string()
                .into(),
        ))
        .unwrap();
    });
    let mut v = Viewer::empty();
    v.connection = Some(Connection::Url {
        endpoint: a_url,
        token: Some("fixture".into()),
    });
    v.status = "Connected".into();
    v.profile = "work".into();
    v.connection_epoch = 1;
    v.fleet_state.projects_network.init(Arc::new(|| {}));
    v.fleet_state.project_folders_network.init(Arc::new(|| {}));
    fn bind(v: &mut Viewer, label: &str) -> Owner {
        let (owner, binding) = v.project_owner().unwrap();
        v.fleet_state.projects.bind(owner.clone());
        let (ticket, _) = v
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        assert!(v.fleet_state.projects.finish(
            ticket,
            &binding,
            Ok(serde_json::json!({"projects":[{"id":"same","label":label}]}))
        ));
        owner
    }
    let owner = bind(&mut v, "A");
    v.project_folders_open(
        owner,
        Scope::All,
        v.projects_state().generation(),
        "same".into(),
    );
    v.project_folders_choose();
    let capture = v.fleet_state.project_folders_choose.take().unwrap();
    v.project_folders_return(capture, vec!["/A".into()]);
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    v.project_folders_cancel();
    v.connection = Some(Connection::Url {
        endpoint: b_url,
        token: Some("other".into()),
    });
    v.connection_epoch = 2;
    bind(&mut v, "B");
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while v.project_folders_busy() {
        v.poll_project_folders();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        v.project_folders_result()
            .unwrap()
            .contains("previous project source")
    );
    assert_eq!(
        v.projects_state().tree.as_ref().unwrap().projects[0].label,
        "B"
    );
    assert!(b.accept().is_err(), "write or refresh reached B");
    peer.join().unwrap();
}

#[test]
fn held_remote_listing_cannot_replace_new_source_picker() {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::mpsc,
        time::{Duration, Instant},
    };
    let a = TcpListener::bind("127.0.0.1:0").unwrap();
    let a_url = format!("http://{}", a.local_addr().unwrap());
    let b = TcpListener::bind("127.0.0.1:0").unwrap();
    b.set_nonblocking(true).unwrap();
    let b_url = format!("http://{}", b.local_addr().unwrap());
    let (seen_tx, seen_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        for index in 0..2 {
            let (stream, _) = a.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let _ws = tungstenite::accept_hdr(
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
            .unwrap();
            let (stream, _) = a.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(
                line.starts_with(if index == 0 {
                    "GET /api/fs/default-cwd?"
                } else {
                    "GET /api/fs/list?"
                }),
                "{line}"
            );
            assert!(
                line.contains("profile=work") && line.contains("token=fixture"),
                "{line}"
            );
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            if index == 1 {
                seen_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            let body = if index == 0 {
                r#"{"cwd":"/A"}"#
            } else {
                r#"{"entries":[{"name":"foreign","path":"/A/foreign","isDirectory":true}]}"#
            };
            write!(reader.get_mut(),"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        }
    });
    let mut v = Viewer::empty();
    v.connection = Some(Connection::Url {
        endpoint: a_url,
        token: Some("fixture".into()),
    });
    v.status = "Connected".into();
    v.profile = "work".into();
    v.connection_epoch = 1;
    let page_owner = (v.connection_epoch, v.profile.clone(), v.profile_intent);
    v.page_accepted_owner = Some(page_owner.clone());
    v.page_loaded_owner = Some(page_owner);
    v.page_network.init(Arc::new(|| {}));
    v.fleet_state.projects_network.init(Arc::new(|| {}));
    fn bind(v: &mut Viewer) -> Owner {
        let (owner, binding) = v.project_owner().unwrap();
        v.fleet_state.projects.bind(owner.clone());
        let (ticket, _) = v
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        assert!(v.fleet_state.projects.finish(
            ticket,
            &binding,
            Ok(serde_json::json!({"projects":[{"id":"same","label":"Project"}]}))
        ));
        owner
    }
    let owner = bind(&mut v);
    v.project_folders_open(
        owner,
        Scope::All,
        v.projects_state().generation(),
        "same".into(),
    );
    v.project_folders_choose();
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    rt.update(|v, tx| {
        v.poll_tools(1., tx);
    });
    assert!(rt.file_dialog().is_none());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let replies = rt.state.page_network.poll(&rt.state.composer_scope());
        for reply in replies {
            rt.state.apply_page_result(&reply.operation, reply.result);
        }
        if seen_rx.try_recv().is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "held list not reached: {:?}",
            rt.state.directories.error
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    rt.state.project_folders_cancel();
    rt.state.connection = Some(Connection::Url {
        endpoint: b_url,
        token: Some("other".into()),
    });
    rt.state.connection_epoch = 2;
    let owner = bind(&mut rt.state);
    rt.state.project_folders_open(
        owner,
        Scope::All,
        rt.state.projects_state().generation(),
        "same".into(),
    );
    rt.state.project_folders_choose();
    let c = rt.state.fleet_state.project_folders_choose.take().unwrap();
    rt.state.directories.open(
        c.request_scope.clone(),
        false,
        "/B".into(),
        crate::directories::Purpose::ProjectFolders(c.dialog, c.token),
    );
    rt.state.directories.loading = false;
    release_tx.send(()).unwrap();
    peer.join().unwrap();
    for _ in 0..50 {
        let replies = rt.state.page_network.poll(&rt.state.composer_scope());
        for reply in replies {
            rt.state.apply_page_result(&reply.operation, reply.result);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(rt.state.directories.path.text, "/B");
    assert!(rt.state.directories.entries.is_empty());
    assert!(rt.state.project_folders_visible());
    assert!(b.accept().is_err(), "stale listing touched B");
}
