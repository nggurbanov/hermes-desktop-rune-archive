use super::*;
#[test]
fn fixtures_never_spawn_or_accept_input_and_hidden_workspaces_keep_tabs() {
    let mut terminal = Terminal::new();
    terminal.fixture();
    assert_eq!(terminal.current().unwrap().tabs.len(), 1);
    terminal.action(Action::Start);
    assert!(terminal.current().unwrap().error.is_some());
    terminal.action(Action::Input(b"no execution\r".to_vec()));
    assert!(
        terminal
            .current()
            .unwrap()
            .active()
            .unwrap()
            .grid
            .screen()
            .contents()
            .contains("cargo check")
    );
}
#[test]
fn restored_remote_identity_stays_detached_until_explicit_attach() {
    let mut original = Terminal::new();
    original.fixture();
    let w = original.current_mut().unwrap();
    let tab = &mut w.tabs[0];
    tab.source = Source::Remote;
    tab.durable = Some("durable-shell".into());
    let saved = original.saved();
    let mut restored = Terminal::new();
    restored.restore(saved);
    let tab = &restored.workspaces.values().next().unwrap().tabs[0];
    assert_eq!(tab.durable.as_deref(), Some("durable-shell"));
    assert_eq!(tab.status, Status::Detached);
    assert!(tab.handle.is_none());
    assert!(tab.grid.screen().contents().is_empty());
}

#[cfg(unix)]
#[test]
fn native_pty_receives_real_input_resize_and_flushes_ansi_output_before_exit() {
    let (send, receive) = mpsc::channel();
    let handle=worker::spawn("native-test".into(),1,worker::Start{target:worker::Target::Test("stty -echo; printf 'READY\\n'; IFS= read -r line; stty size; printf '\\033[31m%s\\033[0m\\n' \"$line\"".into()),profile:"work".into(),cwd:String::new(),rows:24,cols:80,durable:None},send,std::sync::Arc::new(||{}));
    let mut output = Vec::new();
    let mut started = false;
    let mut sent = false;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        assert!(Instant::now() < deadline, "Native PTY did not finish");
        let reply = receive.recv_timeout(Duration::from_millis(500)).unwrap();
        if let Reply::Event { event, .. } = reply {
            match event {
                worker::Event::Started { .. } => started = true,
                worker::Event::Data(bytes, _) => {
                    output.extend(bytes);
                    if output.windows(5).any(|w| w == b"READY") && !sent {
                        assert!(started);
                        handle.send.send(worker::Command::Resize(100, 31)).unwrap();
                        handle
                            .send
                            .send(worker::Command::Input(b"typed-marker\r".to_vec()))
                            .unwrap();
                        sent = true;
                    }
                }
                worker::Event::Exit(code) => {
                    assert_eq!(code, Some(0));
                    break;
                }
                worker::Event::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
    }
    let output = String::from_utf8(output).unwrap();
    assert!(
        output.contains("31 100"),
        "Resize did not reach real PTY: {output:?}"
    );
    assert!(
        output.contains("\x1b[31mtyped-marker\x1b[0m"),
        "Final ANSI output was dropped: {output:?}"
    );
    drop(handle);
}

use std::{
    net::{TcpListener, TcpStream},
    thread,
};
use tungstenite::{Message, WebSocket};
fn host_socket(
    listener: &TcpListener,
    expected_attach: Option<&str>,
    close: bool,
) -> (WebSocket<TcpStream>, WebSocket<TcpStream>) {
    let (gateway, _) = listener.accept().unwrap();
    gateway
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let gateway = tungstenite::accept(gateway).unwrap();
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let socket = tungstenite::accept_hdr(
        stream,
        |request: &tungstenite::handshake::server::Request, response| {
            let url = url::Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
            assert_eq!(url.path(), "/api/host-terminal");
            let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(query.get("profile").map(String::as_str), Some("work"));
            assert_eq!(
                query.get("token").map(String::as_str),
                Some("synthetic-token")
            );
            assert_eq!(query.get("attach").map(String::as_str), expected_attach);
            assert_eq!(
                query.get("action").map(String::as_str),
                if close { Some("close") } else { None }
            );
            Ok(response)
        },
    )
    .unwrap();
    (gateway, socket)
}
fn meta(socket: &mut WebSocket<TcpStream>, value: serde_json::Value) {
    socket
        .send(Message::Text(
            format!("\0HERMES_TERMINAL_META:{value}").into(),
        ))
        .unwrap();
}
fn terminal(connection: Connection) -> Terminal {
    let mut terminal = Terminal::new();
    terminal.waker = Some(std::sync::Arc::new(|| {}));
    terminal.loaded = true;
    terminal
        .set_owner(
            connection,
            DraftScope {
                connection: 8,
                profile: "work".into(),
                session: Some("stored".into()),
            },
            Some("/srv/work".into()),
            None,
        )
        .unwrap();
    terminal
}
fn wait(terminal: &mut Terminal, ready: impl Fn(&Terminal) -> bool) {
    let start = Instant::now();
    loop {
        terminal.poll();
        if ready(terminal) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "Terminal did not reach expected state"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn remote_reconnect_attaches_same_identity_replaces_binary_replay_and_close_requires_receipt() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (_gateway, mut socket) = host_socket(&listener, None, false);
        meta(
            &mut socket,
            serde_json::json!({"terminalId":"shell-owned","shell":"bash","cwd":"/srv/work"}),
        );
        socket
            .send(Message::Binary(b"before\r\n\x1b[6n".to_vec().into()))
            .unwrap();
        drop(socket);
        drop(_gateway);
        let (_gateway, mut socket) = host_socket(&listener, Some("shell-owned"), false);
        meta(
            &mut socket,
            serde_json::json!({"terminalId":"shell-owned","shell":"bash","cwd":"/srv/work","reconnected":true}),
        );
        socket
            .send(Message::Binary(
                b"before\r\nafter\r\n\x1b[6n".to_vec().into(),
            ))
            .unwrap();
        let mut typed = false;
        loop {
            let message = socket.read().unwrap();
            match message {
                Message::Text(text) => {
                    assert!(
                        !text.ends_with('R'),
                        "Replay generated a cursor reply into shell: {text:?}"
                    );
                    if text == "typed\r" {
                        typed = true;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        assert!(typed);
        drop(_gateway);
        let (_gateway, mut socket) = host_socket(&listener, Some("shell-owned"), true);
        meta(
            &mut socket,
            serde_json::json!({"terminalId":"shell-owned","closed":true}),
        );
        socket
            .close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::Normal,
                reason: "closed".into(),
            }))
            .unwrap();
    });
    let mut terminal = terminal(connection);
    terminal.action(Action::Start);
    wait(&mut terminal, |t| {
        t.current()
            .unwrap()
            .active()
            .is_some_and(|tab| tab.grid.screen().contents().contains("after"))
    });
    let tab = terminal.current().unwrap().active().unwrap();
    assert_eq!(tab.durable.as_deref(), Some("shell-owned"));
    assert_eq!(tab.grid.screen().contents().matches("before").count(), 1);
    let id = tab.id.clone();
    terminal.action(Action::Input(b"typed\r".to_vec()));
    terminal.action(Action::Close(id));
    wait(&mut terminal, |t| t.current().unwrap().tabs.is_empty());
    server.join().unwrap();
}
#[test]
fn wrong_close_receipt_keeps_remote_tab_and_durable_owner_for_explicit_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (_gateway, mut socket) = host_socket(&listener, None, false);
        meta(
            &mut socket,
            serde_json::json!({"terminalId":"owned","shell":"bash"}),
        );
        socket.send(Message::Binary(Vec::new().into())).unwrap();
        while !matches!(socket.read(), Ok(Message::Close(_)) | Err(_)) {}
        drop(_gateway);
        let (_gateway, mut socket) = host_socket(&listener, Some("owned"), true);
        meta(
            &mut socket,
            serde_json::json!({"terminalId":"another-shell","closed":true}),
        );
        socket.close(None).unwrap();
    });
    let mut terminal = terminal(connection);
    terminal.action(Action::Start);
    wait(&mut terminal, |t| {
        t.current()
            .unwrap()
            .active()
            .is_some_and(|tab| tab.status == Status::Open)
    });
    let id = terminal.current().unwrap().active().unwrap().id.clone();
    terminal.action(Action::Close(id));
    wait(&mut terminal, |t| {
        t.current()
            .unwrap()
            .active()
            .is_some_and(|tab| tab.status == Status::Error)
    });
    let tab = terminal.current().unwrap().active().unwrap();
    assert_eq!(tab.durable.as_deref(), Some("owned"));
    assert!(tab.error.as_deref().unwrap().contains("not confirmed"));
    server.join().unwrap();
}

#[test]
fn process_mirrors_deduplicate_snapshots_keep_session_routes_and_never_accept_input() {
    let connection = Connection::Url {
        endpoint: "http://127.0.0.1:1".into(),
        token: None,
    };
    let mut terminal = terminal(connection.clone());
    let old = DraftScope {
        connection: 8,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    terminal.agent_event(
        &old,
        "agent.terminal.output",
        &serde_json::json!({"process_id":"process-1","chunk":"first\nsecond\n"}),
    );
    terminal.sync_processes(&old,&serde_json::json!({"processes":[{"session_id":"process-1","command":"synthetic task","output_tail":"first\nsecond\n"}]}));
    let tab = terminal.current().unwrap().active().unwrap();
    assert_eq!(tab.source, Source::Agent);
    assert_eq!(tab.grid.screen().contents().matches("first").count(), 1);
    assert_eq!(tab.grid.screen().cell(1, 0).unwrap().contents(), "s");
    terminal.action(Action::Input(b"must-not-write".to_vec()));
    assert!(
        terminal
            .current()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("read-only")
    );
    terminal
        .set_owner(
            connection,
            DraftScope {
                session: Some("another".into()),
                ..old.clone()
            },
            Some("/other-project".into()),
            None,
        )
        .unwrap();
    terminal.agent_event(
        &old,
        "agent.terminal.output",
        &serde_json::json!({"process_id":"process-1","chunk":"third\n"}),
    );
    assert!(terminal.current().unwrap().tabs.is_empty());
    let original = terminal.session_owners.get(&old).unwrap().clone();
    assert!(
        terminal.workspaces[&original]
            .active()
            .unwrap()
            .grid
            .screen()
            .contents()
            .contains("third")
    );
    let snapshot = terminal.workspaces[&original]
        .active()
        .unwrap()
        .grid
        .screen()
        .contents();
    terminal.close_profile(8, "work");
    terminal.agent_event(
        &old,
        "agent.terminal.output",
        &serde_json::json!({"process_id":"process-1","chunk":"stale"}),
    );
    assert_eq!(
        terminal.workspaces[&original]
            .active()
            .unwrap()
            .grid
            .screen()
            .contents(),
        snapshot
    );
}
#[test]
fn retained_scrollback_search_and_copy_include_earlier_wide_unicode_output() {
    let mut grid = grid::Grid::new(3, 20);
    grid.process(
        "東京 beginning\nline2\nline3\nline4\nline5\n".as_bytes(),
        true,
    );
    assert!(!grid.screen().contents().contains("東京"));
    assert!(grid.find_next("東京").is_some());
    assert_eq!(grid.selected(), "東京");
    grid.select_all();
    let all = grid.selected();
    assert!(all.contains("東京 beginning"));
    assert!(all.contains("line5"));
}
#[test]
fn ssh_terminal_preserves_arguments_and_quotes_remote_paths_without_agent_lifecycle_commands() {
    let config = crate::connections::Ssh {
        host: "example.invalid".into(),
        user: "worker".into(),
        port: 2222,
        key_file: Some(PathBuf::from("/synthetic/key")),
        remote_port: 9119,
        local_port: 49123,
        path: String::new(),
    };
    let connection = Connection::Url {
        endpoint: "http://127.0.0.1:1".into(),
        token: None,
    };
    let start = worker::Start {
        target: worker::Target::Ssh {
            config: config.clone(),
            windows: false,
            connection: connection.clone(),
        },
        profile: "work".into(),
        cwd: "/srv/a'b".into(),
        rows: 24,
        cols: 80,
        durable: None,
    };
    let (command, _, _) = worker::shell_command(&start).unwrap();
    let args = command
        .get_argv()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(args.windows(2).any(|pair| pair == ["-l", "worker"]));
    assert!(args.windows(2).any(|pair| pair == ["-p", "2222"]));
    assert!(args.iter().any(|arg| arg == "BatchMode=yes"));
    assert!(args.last().unwrap().contains("'/srv/a'\\''b'"));
    assert!(
        !args
            .iter()
            .any(|arg| arg.contains("gateway ensure") || arg.contains("hermes update"))
    );
    let mut invalid = config;
    invalid.host = "-oProxyCommand=unsafe".into();
    let mut start = start;
    start.target = worker::Target::Ssh {
        config: invalid,
        windows: false,
        connection,
    };
    assert!(worker::shell_command(&start).is_err());
}

#[test]
fn closing_an_agent_mirror_does_not_resurrect_it_on_poll_but_explicit_open_restores_it() {
    let mut terminal = terminal(Connection::Url {
        endpoint: "http://127.0.0.1:1".into(),
        token: None,
    });
    let scope = DraftScope {
        connection: 8,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let snapshot = serde_json::json!({"processes":[{"session_id":"p","command":"work","output_tail":"retained output\n"}]});
    terminal.sync_processes(&scope, &snapshot);
    let id = terminal.current().unwrap().active().unwrap().id.clone();
    terminal.action(Action::Close(id));
    terminal.sync_processes(&scope, &snapshot);
    terminal.agent_event(
        &scope,
        "agent.terminal.output",
        &serde_json::json!({"process_id":"p","chunk":"late"}),
    );
    assert!(terminal.current().unwrap().tabs.is_empty());
    terminal.open_process(&scope, "p");
    assert!(
        terminal
            .current()
            .unwrap()
            .active()
            .unwrap()
            .grid
            .screen()
            .contents()
            .contains("retained output")
    );
    assert_eq!(
        terminal.current().unwrap().active().unwrap().source,
        Source::Agent
    );
}

#[test]
fn local_restoration_keeps_retained_scrollback_and_ansi_without_starting_a_new_process() {
    let mut terminal = Terminal::new();
    terminal.fixture();
    let tab = terminal.current_mut().unwrap().active_mut().unwrap();
    tab.grid = grid::Grid::new(3, 20);
    tab.grid.process(
        "\x1b[31m東京 beginning\x1b[0m\nline2\nline3\nline4\nline5\n".as_bytes(),
        true,
    );
    let saved = terminal.saved();
    let mut restored = Terminal::new();
    restored.restore(saved);
    let tab = restored
        .workspaces
        .values_mut()
        .next()
        .unwrap()
        .active_mut()
        .unwrap();
    assert!(tab.handle.is_none());
    assert!(tab.grid.all_text().contains("東京 beginning"));
    tab.grid.find_next("東京").unwrap();
    assert_eq!(
        tab.grid.screen().cell(0, 0).unwrap().fgcolor(),
        vt100::Color::Idx(1)
    );
}
