use super::*;
use rune::{Cx, El, Env, Runtime};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

fn view(v: &Viewer, _: &mut Cx<Viewer>) -> El {
    let mut layers = Vec::new();
    let kit = rune_theme_opencode::kit(true);
    if let Some(f) = v.project_create_form() {
        crate::view::project_create::modal::<Viewer>(
            crate::view::project_create::Snapshot {
                target: f.target(),
                name: &f.name,
                idea: &f.idea,
                folders: &f.folders,
                error: f.error.as_deref(),
                pending: v.project_create_busy(),
                generating: v.project_idea_busy(),
                uncertain: f.uncertain,
                templates: v.project_create_templates(),
            },
            &kit,
            &mut layers,
            "",
        );
    }
    rune::col().size(700., 700.).children(layers)
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
    v.fleet_state.project_create_network.init(Arc::new(|| {}));
    v.fleet_state.project_idea_network.init(Arc::new(|| {}));
    let (owner, binding) = v.project_owner().unwrap();
    v.fleet_state.projects.bind(owner);
    let (ticket, _) = v
        .fleet_state
        .projects
        .begin(LoadKind::Tree, &binding)
        .unwrap();
    assert!(
        v.fleet_state
            .projects
            .finish(ticket, &binding, Ok(json!({"projects":[]})))
    );
    v
}
fn open(v: &mut Viewer, name: &str, idea: &str) {
    let owner = v.projects_state().owner.clone().unwrap();
    v.create_action(Action::Open(crate::view::project_create::OpenTarget {
        owner,
        scope: v.projects_state().scope.clone(),
        generation: v.projects_state().generation(),
        focus_return: String::new(),
    }));
    let form = v.fleet_state.project_create.as_mut().unwrap();
    form.name.replace_text(name);
    form.idea.replace_text(idea);
    form.folders.push("/fixture/project".into());
}
fn click(rt: &mut Runtime<Viewer>, key: &str) {
    // Polls and fixture setup mutate state outside Runtime input dispatch.
    rt.invalidate();
    rt.frame(1.);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.);
}
fn wait(v: &mut Viewer, predicate: impl Fn(&Viewer) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate(v) {
        v.poll_project_create();
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
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
fn read(socket: &mut tungstenite::WebSocket<std::net::TcpStream>) -> Value {
    serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap()
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
fn closed_dialog_retains_create_receipt_without_entering_project() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut socket = accept(&listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.create");
        assert_eq!(
            request["params"],
            json!({"profile":"work","name":"First","folders":["/fixture/project"],"use":true})
        );
        seen_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        reply(
            &mut socket,
            &request,
            json!({"project":{"id":"same","name":"First","primary_path":"/fixture/project"}}),
        );
    });
    let mut v = viewer(endpoint);
    open(&mut v, "First", "");
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-create-submit");
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    click(&mut rt, "project-create-cancel");
    assert!(rt.state.project_create_busy());
    release_tx.send(()).unwrap();
    wait(&mut rt.state, |v| !v.project_create_busy());
    assert_eq!(rt.state.projects_state().scope, Scope::All);
    assert!(
        rt.state
            .projects_state()
            .tree
            .as_ref()
            .unwrap()
            .projects
            .is_empty()
    );
    assert!(
        rt.state
            .project_create_result()
            .unwrap()
            .contains("previous")
    );
    peer.join().unwrap();
}

#[test]
fn uncertain_create_requires_explicit_retry_and_preserves_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (requests_tx, requests_rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut socket = accept(&listener);
            let request = read(&mut socket);
            requests_tx.send(request).unwrap();
            drop(socket);
        }
    });
    let mut v = viewer(endpoint);
    open(&mut v, "First", "Keep this idea");
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-create-submit");
    let first = requests_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    wait(&mut rt.state, |v| !v.project_create_busy());
    let form = rt.state.project_create_form().unwrap();
    assert!(form.uncertain);
    assert_eq!(form.name.text, "First");
    assert_eq!(form.idea.text, "Keep this idea");
    assert_eq!(form.folders, ["/fixture/project"]);
    for _ in 0..5 {
        rt.state.poll_project_create();
        rt.frame(1.);
    }
    assert!(
        requests_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
    click(&mut rt, "project-create-retry");
    let second = requests_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first["method"], "projects.create");
    assert_eq!(first["params"], second["params"]);
    wait(&mut rt.state, |v| !v.project_create_busy());
    assert!(rt.state.project_create_form().unwrap().uncertain);
    peer.join().unwrap();
}

#[test]
fn create_dispatches_while_sessionless_generation_is_held() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (events_tx, events_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut generation = accept(&listener);
        let request = read(&mut generation);
        assert_eq!(request["method"], "llm.oneshot");
        assert_eq!(request["params"]["input"], "Project name: First");
        assert!(request["params"].get("session_id").is_none());
        events_tx.send("generation").unwrap();
        let mut create = accept(&listener);
        let create_request = read(&mut create);
        assert_eq!(create_request["method"], "projects.create");
        assert_eq!(create_request["params"]["name"], "First");
        events_tx.send("create").unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        reply(
            &mut generation,
            &request,
            json!({"text":"late generated idea"}),
        );
        events_tx.send("released").unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        reply(
            &mut create,
            &create_request,
            json!({"project":{"id":"same","name":"First"}}),
        );
    });
    let mut v = viewer(endpoint);
    open(&mut v, "First", "Manual idea");
    let target = v.project_create_form().unwrap().target();
    v.create_action(Action::Generate(target));
    events_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-create-submit");
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "create"
    );
    assert!(rt.state.project_create_busy());
    release_tx.send(()).unwrap();
    events_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    wait(&mut rt.state, |v| !v.project_idea_busy());
    assert_eq!(
        rt.state.project_create_form().unwrap().idea.text,
        "Manual idea"
    );
    release_tx.send(()).unwrap();
    wait(&mut rt.state, |v| !v.project_create_busy());
    peer.join().unwrap();
}

#[test]
fn idea_write_terminal_does_not_consume_next_create_receipt() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (events_tx, events_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut first = accept(&listener);
        let a = read(&mut first);
        assert_eq!(a["method"], "projects.create");
        assert_eq!(
            a["params"],
            json!({"profile":"work","name":"First","folders":["/fixture/project"],"use":true})
        );
        events_tx.send("create-a").unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        reply(
            &mut first,
            &a,
            json!({"project":{"id":"a","name":"First","primary_path":"/authoritative/a"}}),
        );

        // Every PageNetwork REST operation first establishes its authenticated gateway socket.
        let _writer_socket = accept(&listener);
        let (mut http, _) = listener.accept().unwrap();
        http.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reader = BufReader::new(&mut http);
        let mut lines = Vec::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().unwrap();
            }
            lines.push(line);
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        drop(reader);
        let mut words = lines[0].split_whitespace();
        assert_eq!(words.next(), Some("POST"));
        let url = url::Url::parse(&format!("http://fixture{}", words.next().unwrap())).unwrap();
        assert_eq!(url.path(), "/api/fs/write-text");
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query.get("profile").map(String::as_str), Some("work"));
        assert_eq!(query.get("token").map(String::as_str), Some("fixture"));
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"path":"/authoritative/a/IDEA.md","content":"Idea A\n"})
        );
        events_tx.send("writer-a").unwrap();

        let mut second = accept(&listener);
        let b = read(&mut second);
        assert_eq!(b["method"], "projects.create");
        assert_eq!(
            b["params"],
            json!({"profile":"work","name":"Second","folders":["/fixture/project"],"use":true})
        );
        events_tx.send("create-b").unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        write!(
            http,
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        events_tx.send("writer-released").unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        reply(
            &mut second,
            &b,
            json!({"project":{"id":"b","name":"Second","primary_path":"/authoritative/b"}}),
        );
        let mut entered = accept(&listener);
        let request = read(&mut entered);
        assert_eq!(request["method"], "projects.project_sessions");
        assert_eq!(
            request["params"],
            json!({"profile":"work","project_id":"b"})
        );
        reply(
            &mut entered,
            &request,
            json!({"project":{"id":"b","label":"Entered B"}}),
        );
        let mut tree = accept(&listener);
        let request = read(&mut tree);
        assert_eq!(request["method"], "projects.tree");
        assert_eq!(
            request["params"],
            json!({"profile":"work","preview_limit":3})
        );
        reply(
            &mut tree,
            &request,
            json!({"projects":[{"id":"b","label":"Authoritative B"}]}),
        );
        events_tx.send("refresh").unwrap();
        listener.set_nonblocking(true).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    });
    let mut v = viewer(endpoint);
    v.fleet_state.projects_network.init(Arc::new(|| {}));
    let (wake_tx, wake_rx) = mpsc::channel();
    v.fleet_state.project_create_network.init(Arc::new(move || {
        let _ = wake_tx.send(());
    }));
    open(&mut v, "First", "Idea A");
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-create-submit");
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "create-a"
    );
    click(&mut rt, "project-create-cancel");
    release_tx.send(()).unwrap();
    wake_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    wait(&mut rt.state, |v| !v.project_create_busy());
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "writer-a"
    );
    open(&mut rt.state, "Second", "");
    click(&mut rt, "project-create-submit");
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "create-b"
    );
    assert!(rt.state.project_create_busy());
    release_tx.send(()).unwrap();
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "writer-released"
    );
    wake_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    rt.state.poll_project_create();
    assert!(
        rt.state.project_create_busy(),
        "writer failure must not settle B"
    );
    assert_eq!(rt.state.project_create_form().unwrap().name.text, "Second");
    assert_eq!(rt.state.projects_state().scope, Scope::All);
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !rt.state.projects_state().tree.as_ref().is_some_and(|t| {
        t.projects
            .iter()
            .any(|p| p.id == "b" && p.label == "Authoritative B")
    }) {
        rt.state.poll_projects();
        assert!(Instant::now() < deadline, "timed out waiting for B refresh");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!rt.state.project_create_busy());
    assert_eq!(rt.state.projects_state().scope, Scope::Project("b".into()));
    assert_eq!(
        rt.state.projects_state().entered.as_ref().unwrap().label,
        "Entered B"
    );
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "refresh"
    );
    for _ in 0..5 {
        rt.state.poll_projects();
    }
    release_tx.send(()).unwrap();
    peer.join().unwrap();
}

fn picker_view(v: &Viewer, _: &mut Cx<Viewer>) -> El {
    let kit = rune_theme_opencode::kit(true);
    rune::col()
        .size(700., 700.)
        .children(crate::view::directories::panel(v, &kit))
}

// Construct the remote picker without dispatching its initial listing to any service.
fn fixture_picker(v: &mut Viewer) -> PickerTarget {
    let target = v.project_create_form().unwrap().target();
    v.create_action(Action::Choose(target));
    let pick = v.fleet_state.project_create_choose.take().unwrap();
    let effects = v.directories.open(
        pick.request_scope.clone(),
        false,
        "/same/path".into(),
        crate::directories::Purpose::Projects(pick.token.dialog, pick.token.pick),
    );
    assert_eq!(effects.len(), 1);
    v.fleet_state.project_create_remote = Some(pick.clone());
    v.directories.loading = false;
    v.directories.entries = vec![crate::files::Entry {
        name: "child".into(),
        path: "/same/path/child".into(),
        directory: true,
        ignored: false,
    }];
    pick
}

fn picker_button(rt: &Runtime<Viewer>, label: &str) -> (f32, f32) {
    let item = rt
        .elements()
        .into_iter()
        .find(|e| e.role == "button" && e.label.as_deref() == Some(label) && !e.exiting)
        .unwrap_or_else(|| panic!("missing picker button {label}"));
    (
        item.rect[0] + item.rect[2] / 2.,
        item.rect[1] + item.rect[3] / 2.,
    )
}

#[test]
fn mounted_remote_picker_callbacks_cannot_act_on_reopened_picker() {
    for replace_owner in [false, true] {
        for action in [
            "Use this directory",
            "Cancel",
            "↑ Parent directory",
            "▸ child",
            "edit",
            "submit",
        ] {
            let mut v = viewer("http://127.0.0.1:1".into());
            open(&mut v, "A", "idea A");
            let a = fixture_picker(&mut v);
            let mut rt = Runtime::new(v, picker_view, (700., 700.), Env::new(true, 1.));
            rt.frame(1.);
            let point = if action == "edit" || action == "submit" {
                let p = rt.find("input").expect("mounted A path input");
                rt.pointer_down(p.0, p.1);
                rt.pointer_up(p.0, p.1);
                rt.frame(1.);
                assert!(
                    rt.elements()
                        .iter()
                        .any(|e| e.key.as_deref() == Some("input") && e.focused)
                );
                p
            } else {
                picker_button(&rt, action)
            };
            // The last committed scene and its handlers still belong to A. Closing and reopening
            // state between host input and the next frame models a state change before queued input
            // is dispatched; Runtime processes this queue before committing the replacement scene.
            if action != "edit" && action != "submit" {
                rt.pointer_down(point.0, point.1);
            }
            let old = rt.state.project_create_form().unwrap().target();
            rt.state.create_action(Action::Cancel(old));
            if replace_owner {
                rt.state.connection = Some(Connection::Url {
                    endpoint: "http://127.0.0.1:2".into(),
                    token: Some("other".into()),
                });
                rt.state.profile = "other-profile".into();
                rt.state.profile_intent = crate::source::ProfileIntent::NativeSelf;
                rt.state.connection_epoch = 2;
                let (owner, _) = rt.state.project_owner().unwrap();
                rt.state.fleet_state.projects.bind(owner);
            }
            open(&mut rt.state, "B", "idea B");
            let b = fixture_picker(&mut rt.state);
            assert_ne!(a.token.dialog, b.token.dialog);
            assert_ne!(a.token.pick, b.token.pick);
            assert_eq!(a.owner == b.owner, !replace_owner);
            assert_eq!(a.request_scope == b.request_scope, !replace_owner);
            assert_eq!(
                b.request_scope.profile,
                if replace_owner {
                    "other-profile"
                } else {
                    "work"
                }
            );
            assert_eq!(
                b.request_scope.profile_intent,
                if replace_owner {
                    crate::source::ProfileIntent::NativeSelf
                } else {
                    crate::source::ProfileIntent::Backend
                }
            );
            assert_eq!(b.owner.epoch, if replace_owner { 2 } else { 1 });
            assert_eq!(a.request_scope.profile, "work");
            assert_eq!(a.owner.epoch, 1);
            assert_eq!(rt.state.directories.path.text, "/same/path");
            assert!(
                rt.state
                    .fleet_state
                    .project_create_remote
                    .as_ref()
                    .unwrap()
                    .token
                    == b.token
            );
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
            let form = rt.state.project_create_form().unwrap();
            assert_eq!(form.name.text, "B", "{action}");
            assert_eq!(form.idea.text, "idea B", "{action}");
            assert_eq!(form.folders, ["/fixture/project"], "{action}");
            assert_eq!(form.active_pick, Some(b.token), "{action}");
            assert_eq!(
                rt.state.directories.owner.as_ref(),
                Some(&b.request_scope),
                "{action}"
            );
            assert_eq!(rt.state.directories.path.text, "/same/path", "{action}");
            assert!(!rt.state.directories.loading, "{action} queued a browse");
            assert!(!rt.state.project_create_busy(), "{action} queued Create");
            // After B is actually committed, the same mounted view must still accept B's path.
            let p = picker_button(&rt, "Use this directory");
            rt.pointer_down(p.0, p.1);
            rt.pointer_up(p.0, p.1);
            rt.frame(1.);
            assert!(
                rt.state.directories.owner.is_none(),
                "current B Use: {action}"
            );
            assert_eq!(
                rt.state.project_create_form().unwrap().folders,
                ["/fixture/project", "/same/path"]
            );
        }
    }
}

#[test]
fn retained_create_and_failed_idea_write_stay_on_original_source_with_equal_id() {
    let a_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let b_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    b_listener.set_nonblocking(true).unwrap();
    let a_endpoint = format!("http://{}", a_listener.local_addr().unwrap());
    let b_endpoint = format!("http://{}", b_listener.local_addr().unwrap());
    let (event_tx, event_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut socket = accept(&a_listener);
        let request = read(&mut socket);
        assert_eq!(request["method"], "projects.create");
        assert_eq!(
            request["params"],
            json!({"profile":"work","name":"First","folders":["/fixture/project"],"use":true})
        );
        event_tx.send("create").unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        reply(
            &mut socket,
            &request,
            json!({"project":{"id":"same","name":"A","primary_path":"/authoritative/a"}}),
        );
        let _writer_socket = accept(&a_listener);
        let (mut http, _) = a_listener.accept().unwrap();
        http.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reader = BufReader::new(&mut http);
        let mut lines = Vec::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().unwrap();
            }
            lines.push(line);
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        drop(reader);
        let mut words = lines[0].split_whitespace();
        assert_eq!(words.next(), Some("POST"));
        let url = url::Url::parse(&format!("http://fixture{}", words.next().unwrap())).unwrap();
        assert_eq!(url.path(), "/api/fs/write-text");
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query.get("profile").map(String::as_str), Some("work"));
        assert_eq!(query.get("token").map(String::as_str), Some("fixture"));
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"path":"/authoritative/a/IDEA.md","content":"Idea A\n"})
        );
        event_tx.send("writer").unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        write!(
            http,
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        event_tx.send("failed").unwrap();
        a_listener.set_nonblocking(true).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            matches!(a_listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
        );
    });
    let mut v = viewer(a_endpoint);
    open(&mut v, "First", "Idea A");
    let mut rt = Runtime::new(v, view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-create-submit");
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "create"
    );
    rt.state.connection = Some(Connection::Url {
        endpoint: b_endpoint,
        token: Some("fixture".into()),
    });
    rt.state.profile = "other".into();
    rt.state.connection_epoch = 2;
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
        Ok(json!({"projects":[{"id":"same","label":"B"}]}))
    ));
    release_tx.send(()).unwrap();
    wait(&mut rt.state, |v| !v.project_create_busy());
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "writer"
    );
    assert_eq!(rt.state.projects_state().scope, Scope::All);
    assert_eq!(
        rt.state.projects_state().tree.as_ref().unwrap().projects[0].label,
        "B"
    );
    assert!(rt.state.fleet_state.project_create_refresh.is_none());
    release_tx.send(()).unwrap();
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "failed"
    );
    for _ in 0..10 {
        rt.state.poll_project_create();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        rt.state.projects_state().tree.as_ref().unwrap().projects[0].label,
        "B"
    );
    assert!(
        rt.state
            .project_create_result()
            .unwrap()
            .contains("previous source")
    );
    assert!(matches!(b_listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    release_tx.send(()).unwrap();
    peer.join().unwrap();
}

/// A routed NativeSelf viewer with an explicitly owned registry entry and actor.
/// Keep this fixture alive while its Viewer or acquired bindings are in use.
pub(in crate::app::fleet::projects) struct NativeSelfViewer {
    pub(in crate::app::fleet::projects) viewer: Viewer,
    _lease: crate::connections::TestRoutedLease,
    _actor: crate::connections::TestLegacyRig,
}
impl NativeSelfViewer {
    pub(in crate::app::fleet::projects) fn new(connection: Connection) -> Self {
        let actor = crate::connections::TestLegacyRig::new();
        let lease = crate::connections::TestRoutedLease::new(connection.clone());
        let mut viewer = Viewer::empty();
        viewer.connection = Some(connection);
        viewer.status = "Connected".into();
        viewer.profile = "work".into();
        viewer.profile_intent = crate::source::ProfileIntent::NativeSelf;
        viewer.connection_epoch = 41;
        viewer.retain_window_route(actor.resolver(), lease.lease.clone());
        assert!(viewer.client_error.is_none());
        Self {
            viewer,
            _lease: lease,
            _actor: actor,
        }
    }
}

#[test]
fn native_self_viewer_retains_routed_binding_for_root_project_reads() {
    use crate::gateway::{
        BackendRootProof,
        prepared::{BackendTarget, SelectedContext, WirePolicy},
        route,
    };
    use std::collections::BTreeMap;
    use tungstenite::{
        Message,
        handshake::server::{Request, Response},
    };
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
        owner: Arc::new(crate::source::RouteOwner::legacy("projects-private-test").unwrap()),
    };
    let stamp = crate::source::SourceStamp::from_connection(&connection).unwrap();
    let server = std::thread::spawn(move || {
        for epoch in [7, 8] {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut socket =
                tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                    assert_eq!(request.headers()["x-access-fixture"], "private-access");
                    let url = url::Url::parse(&format!("http://private{}", request.uri())).unwrap();
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
            socket.send(Message::Text(json!({"jsonrpc":"2.0","id":describe["id"],"result":{"profile_id":"/private/root","instance_id":"fixture-instance","authority_epoch":epoch}}).to_string().into())).unwrap();
            if epoch == 7 {
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
    let mut fixture = NativeSelfViewer::new(connection);
    let (owner, binding) = fixture.viewer.project_owner().unwrap();
    assert_eq!(owner.session.source, stamp);
    assert_eq!(
        owner.session.profile_intent,
        crate::source::ProfileIntent::NativeSelf
    );
    assert_eq!(owner.epoch, 41);
    assert!(binding.lease().unwrap().is_valid());
    fixture.viewer.fleet_state.projects.bind(owner);
    let mut network = crate::pages::PageNetwork::default();
    network.init(Arc::new(|| {}));
    for valid in [true, false] {
        let (ticket, request) = fixture
            .viewer
            .fleet_state
            .projects
            .begin(LoadKind::Tree, &binding)
            .unwrap();
        let generation = fixture.viewer.fleet_state.projects.generation();
        let scope = fixture.viewer.composer_scope();
        network
            .request(&scope, binding.connection().clone(), "projects", request)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            if let Some(reply) = network.poll(&scope).into_iter().find(|r| !r.progress) {
                break reply.result;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        };
        if valid {
            assert!(result.is_ok());
        } else {
            assert!(result.as_ref().unwrap_err().contains("authority changed"));
        }
        assert!(
            fixture
                .viewer
                .fleet_state
                .projects
                .finish(ticket, &binding, result)
        );
        assert_eq!(fixture.viewer.fleet_state.projects.generation(), generation);
    }
    server.join().unwrap();
}

fn native_create_view(v: &Viewer, _: &mut Cx<Viewer>) -> El {
    let mut layers = Vec::new();
    let body = crate::view::sidebar::sessions(
        v,
        crate::view::palette::pal(true),
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

fn mounted_native_create(valid: bool) {
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
        owner: Arc::new(crate::source::RouteOwner::legacy("projects-private-test").unwrap()),
    };
    let stamp = crate::source::SourceStamp::from_connection(&connection).unwrap();
    let (event_tx, event_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        for index in 0..if valid { 3 } else { 1 } {
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
            event_tx.send("describe").unwrap();
            reply(
                &mut socket,
                &describe,
                json!({"profile_id":"/private/root","instance_id":"fixture-instance","authority_epoch":if valid {7} else {8}}),
            );
            if !valid {
                assert!(socket.read().is_err(), "mismatch must prevent project RPC");
                continue;
            }
            let request = read(&mut socket);
            match index {
                0 => {
                    assert_eq!(request["method"], "projects.create");
                    assert_eq!(
                        request["params"],
                        json!({"name":"Mounted","folders":["/first","/second"],"use":true})
                    );
                    event_tx.send("create").unwrap();
                    reply(
                        &mut socket,
                        &request,
                        json!({"project":{"id":"native","name":"Mounted","primary_path":"/first"}}),
                    );
                }
                1 => {
                    assert_eq!(request["method"], "projects.project_sessions");
                    assert_eq!(request["params"], json!({"project_id":"native"}));
                    reply(
                        &mut socket,
                        &request,
                        json!({"project":{"id":"native","label":"Entered Native"}}),
                    );
                }
                _ => {
                    assert_eq!(request["method"], "projects.tree");
                    assert_eq!(request["params"], json!({"preview_limit":3}));
                    reply(
                        &mut socket,
                        &request,
                        json!({"active_id":"native","projects":[{"id":"native","label":"Authoritative Native"}]}),
                    );
                }
            }
        }
        listener.set_nonblocking(true).unwrap();
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    });
    let NativeSelfViewer {
        mut viewer,
        _lease,
        _actor,
    } = NativeSelfViewer::new(connection);
    viewer.fleet_state.projects_network.init(Arc::new(|| {}));
    viewer
        .fleet_state
        .project_create_network
        .init(Arc::new(|| {}));
    viewer
        .fleet_state
        .project_idea_network
        .init(Arc::new(|| {}));
    let (owner, binding) = viewer.project_owner().unwrap();
    assert_eq!(owner.session.source, stamp);
    assert_eq!(
        owner.session.profile_intent,
        crate::source::ProfileIntent::NativeSelf
    );
    assert_eq!(owner.session.profile, "work");
    assert_eq!(owner.epoch, 41);
    assert!(binding.lease().unwrap().is_valid());
    viewer.fleet_state.projects.bind(owner.clone());
    let (ticket, _) = viewer
        .fleet_state
        .projects
        .begin(LoadKind::Tree, &binding)
        .unwrap();
    assert!(
        viewer
            .fleet_state
            .projects
            .finish(ticket, &binding, Ok(json!({"projects":[]})))
    );
    viewer.sidebar_state.prefs.grouping = crate::sidebar::Grouping::Project;
    let generation = viewer.projects_state().generation();
    let mut rt = Runtime::new(viewer, native_create_view, (700., 700.), Env::new(true, 1.));
    click(&mut rt, "project-create-header");
    let form = rt.state.project_create_form().unwrap();
    assert_eq!(form.owner, owner);
    assert_eq!(form.scope, Scope::All);
    assert_eq!(form.generation, generation);
    assert_eq!(
        form.request_scope.profile_intent,
        crate::source::ProfileIntent::NativeSelf
    );
    assert!(form.binding.lease().unwrap().is_valid());
    click(&mut rt, "project-create-name");
    rt.paste("Mounted");
    rt.frame(1.);
    assert_eq!(rt.state.project_create_form().unwrap().name.text, "Mounted");
    click(&mut rt, "project-create-choose");
    let pick = rt.state.fleet_state.project_create_choose.take().unwrap();
    assert_eq!(pick.owner, owner);
    assert_eq!(
        pick.token.dialog,
        rt.state.project_create_form().unwrap().dialog
    );
    rt.state
        .project_create_pick_return(pick, vec!["/first".into(), "/second".into()]);
    rt.invalidate();
    rt.frame(1.);
    let form = rt.state.project_create_form().unwrap();
    assert_eq!(form.folders, ["/first", "/second"]);
    assert!(form.request().is_ok());
    let labels: Vec<_> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.text.or(e.label))
        .collect();
    assert!(labels.iter().any(|s| s.contains("/first")));
    assert!(labels.iter().any(|s| s.contains("/second")));
    assert!(labels.iter().any(|s| s.contains("Primary")));
    click(&mut rt, "project-create-submit");
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "describe"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        rt.state.poll_projects();
        if valid {
            if rt
                .state
                .projects_state()
                .tree
                .as_ref()
                .is_some_and(|t| t.projects.iter().any(|p| p.label == "Authoritative Native"))
            {
                break;
            }
        } else if !rt.state.project_create_busy() {
            break;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
    if valid {
        assert_eq!(
            event_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "create"
        );
        assert_eq!(rt.state.projects_state().owner.as_ref(), Some(&owner));
        assert_eq!(
            rt.state.projects_state().scope,
            Scope::Project("native".into())
        );
        assert_eq!(
            rt.state.projects_state().entered.as_ref().unwrap().label,
            "Entered Native"
        );
        let tree = rt.state.projects_state().tree.as_ref().unwrap();
        assert_eq!(tree.active_id.as_deref(), Some("native"));
        assert_eq!(tree.projects.len(), 1);
        assert_eq!(tree.projects[0].id, "native");
    } else {
        let form = rt.state.project_create_form().unwrap();
        assert!(form.error.as_ref().unwrap().contains("authority changed"));
        assert!(form.uncertain);
        assert_eq!(rt.state.projects_state().owner.as_ref(), Some(&owner));
        assert_eq!(rt.state.projects_state().scope, Scope::All);
        assert!(
            rt.state
                .projects_state()
                .tree
                .as_ref()
                .unwrap()
                .projects
                .is_empty()
        );
    }
    server.join().unwrap();
    drop((_lease, _actor));
}

#[test]
fn mounted_native_self_create_accepts_root_and_refreshes() {
    mounted_native_create(true);
}

#[test]
fn mounted_native_self_create_rejects_changed_root_authority() {
    mounted_native_create(false);
}

#[path = "presenter_tests.rs"]
mod presenter_tests;
