//! Real socket contracts against a private synthetic gateway, never an agent.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
};
use tungstenite::{Message, WebSocket};

pub(super) fn request(ws: &mut WebSocket<TcpStream>, method: &str) -> Value {
    let frame: Value = serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
    assert_eq!(frame["method"], method);
    assert_eq!(frame["params"]["profile"], "work");
    frame
}
pub(super) fn answer(ws: &mut WebSocket<TcpStream>, request: &Value, result: Value) {
    ws.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":result})
            .to_string()
            .into(),
    ))
    .unwrap();
}
pub(super) fn snapshot(running: bool, seq: u64) -> Value {
    json!({"session_id":"live","stored_session_id":"stored","running":running,"revision":2,"execution_generation":if running{1}else{0},"authority_epoch":7,"replay_epoch":"fixture-epoch","last_sequence":seq,"messages":[],"prompts":[],"pending":[],"info":{"profile_name":"work","profile_id":"fixture-profile","desktop_protocol":"hermes-gateway-v1"}})
}
pub(super) fn history(listener: &TcpListener, messages: Value) {
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut headers = String::new();
    let mut reader = BufReader::new(&mut stream);
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line.is_empty() {
            break;
        }
        headers.push_str(&line);
    }
    assert!(headers.starts_with("GET /api/sessions/stored/messages?"));
    assert!(headers.contains("profile=work"));
    assert!(headers.contains("token=synthetic-token"));
    let body = json!({"profile":"work","session_id":"stored","messages":messages}).to_string();
    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
}
fn event(ws: &mut WebSocket<TcpStream>, seq: u64, kind: &str, payload: Value) {
    ws.send(Message::Text(json!({"jsonrpc":"2.0","method":"event","params":{"type":kind,"session_id":"live","seq":seq,"replay_epoch":"fixture-epoch","authority_epoch":7,"execution_generation":1,"admission_id":"owner-admission","payload":payload}}).to_string().into())).unwrap();
}
pub(super) fn worker(connection: Connection, scope: DraftScope) -> (Worker, Receiver<Reply>) {
    let (replies, receive) = mpsc::channel();
    let mut transport = SessionTransport::connect(&connection, &scope.profile).unwrap();
    // Exercise the canonical wire on a synthetic token-authenticated socket.
    // Production selects it only after the native subprotocol handshake.
    transport.canonical = true;
    (
        Worker {
            connection,
            scope: scope.clone(),
            replies,
            waker: Arc::new(|| {}),
            transport: Some(transport),
            stored: scope.session,
            epoch: String::new(),
            last: 0,
            reconnect_at: Instant::now(),
            reconnect_attempts: 0,
            want_reconnect: false,
        },
        receive,
    )
}

#[test]
fn real_gui_create_files_permission_readback_admission_and_stream_are_one_owned_cycle() {
    let root = std::env::temp_dir().join(format!("hermes-rune-cycle-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("notes.txt");
    std::fs::write(&file, b"synthetic attachment").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let create = request(&mut ws, "session.create");
        assert_eq!(create["params"]["source"], "gui");
        assert!(create["params"]["request_id"].as_str().is_some());
        answer(&mut ws, &create, snapshot(false, 0));
        history(&listener, json!([]));
        let set = request(&mut ws, "config.set");
        assert_eq!(set["params"]["key"], "approvals.mode");
        assert_eq!(set["params"]["value"], "smart");
        answer(
            &mut ws,
            &set,
            json!({"key":"approvals.mode","value":"smart"}),
        );
        let get = request(&mut ws, "config.get");
        assert_eq!(get["params"]["key"], "approvals.mode");
        answer(&mut ws, &get, json!({"value":"smart"}));
        let upload = request(&mut ws, "file.attach");
        assert!(upload["params"].get("path").is_none());
        assert!(
            upload["params"]["data_url"]
                .as_str()
                .unwrap()
                .starts_with("data:application/octet-stream;base64,")
        );
        answer(
            &mut ws,
            &upload,
            json!({"attached":true,"ref_text":"@file:notes.txt"}),
        );
        let submit = request(&mut ws, "prompt.submit");
        assert_eq!(submit["params"]["text"], "@file:notes.txt\n\nHello");
        assert_eq!(submit["params"]["session_id"], "live");
        assert_eq!(submit["params"]["queued"], true);
        assert_ne!(submit["params"]["submission_id"], "owner-admission");
        // Interleaved notification BEFORE the ACK must survive the RPC wait.
        event(&mut ws, 1, "message.start", json!({}));
        answer(
            &mut ws,
            &submit,
            json!({"admission_id":"owner-admission","ref":{"profile_id":"fixture-profile","session_id":"live"},"status":"started","sequence":1,"execution_generation":1,"authority_epoch":7}),
        );
        event(
            &mut ws,
            2,
            "message.delta",
            json!({"text":"😀","turn_id":"turn","segment":0,"offset":0}),
        );
        event(
            &mut ws,
            2,
            "message.delta",
            json!({"text":"duplicate","turn_id":"turn","segment":0,"offset":0}),
        );
        event(
            &mut ws,
            3,
            "message.delta",
            json!({"text":" Hello","turn_id":"turn","segment":0,"offset":2}),
        );
        event(
            &mut ws,
            4,
            "message.complete",
            json!({"text":"😀 Hello","status":"complete"}),
        );
        let resume = request(&mut ws, "session.resume");
        assert_eq!(resume["params"]["session_id"], "stored");
        answer(&mut ws, &resume, snapshot(false, 4));
        history(
            &listener,
            json!([{"role":"user","content":"Hello"},{"role":"assistant","content":"😀 Hello"}]),
        );
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: None,
    };
    let prepared = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "Hello".into(),
            attachments: vec![AttachmentInput {
                path: file,
                name: "notes.txt".into(),
            }],
            approval_mode: Some("smart".into()),
            approval_changed: true,
            ..Default::default()
        },
    )
    .unwrap();
    let id = prepared.submission_id.clone();
    let journal = Journal::new(root.join("journal")).unwrap();
    journal.save(&prepared).unwrap();
    let (mut worker, replies) = worker(connection, scope.clone());
    worker.submit(prepared, &journal).unwrap();
    assert!(journal.load(&id).is_err());
    for _ in 0..5 {
        let event = worker
            .transport
            .as_mut()
            .unwrap()
            .read_event()
            .unwrap()
            .unwrap();
        worker.event(event).unwrap();
    }
    let mut manager = Conversation::new();
    for reply in replies.try_iter() {
        manager.replies.send(reply).unwrap();
    }
    let updates = manager.poll();
    assert!(
        updates
            .iter()
            .any(|u| matches!(u,Update::Bound{origin,..}if origin.session.is_none()))
    );
    let stored = DraftScope {
        session: Some("stored".into()),
        ..scope
    };
    let view = manager.view(&stored).unwrap();
    assert!(!view.running);
    assert_eq!(view.messages.len(), 2);
    assert_eq!(view.messages[1].content, "😀 Hello");
    assert!(
        manager
            .view(&DraftScope {
                connection: 1,
                profile: "other".into(),
                session: Some("stored".into())
            })
            .is_none()
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_resume_never_falls_back_to_create_or_prompt() {
    let root = std::env::temp_dir().join(format!("hermes-rune-resume-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: None,
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        assert_eq!(resume["params"]["session_id"], "missing");
        ws.send(Message::Text(
            json!({"jsonrpc":"2.0","id":resume["id"],"error":{"code":4004,"message":"not_found"}})
                .to_string()
                .into(),
        ))
        .unwrap();
        ws.get_mut()
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        assert!(
            ws.read().is_err(),
            "No create/submit follows a refused explicit resume"
        );
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("missing".into()),
    };
    let prepared = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "do not send".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let journal = Journal::new(root.clone()).unwrap();
    journal.save(&prepared).unwrap();
    let (mut worker, _) = worker(connection, scope);
    let error = worker.submit(prepared.clone(), &journal).unwrap_err();
    assert!(!error.uncertain);
    assert_eq!(
        journal.load(&prepared.submission_id).unwrap().create_id,
        prepared.create_id
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn uncertain_ack_retries_the_same_admission_without_creating_a_second_chat_or_bubble() {
    let root = std::env::temp_dir().join(format!("hermes-rune-retry-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let create = request(&mut ws, "session.create");
        answer(&mut ws, &create, snapshot(false, 0));
        history(&listener, json!([]));
        let submit = request(&mut ws, "prompt.submit");
        let id = submit["params"]["submission_id"].clone();
        drop(ws); // accepted by owner, ACK lost
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        assert_eq!(resume["params"]["session_id"], "stored");
        answer(&mut ws, &resume, snapshot(false, 4));
        history(
            &listener,
            json!([{"role":"user","content":"Hello"},{"role":"assistant","content":"Already answered"}]),
        );
        let retry = request(&mut ws, "prompt.submit");
        assert_eq!(retry["params"]["submission_id"], id);
        answer(
            &mut ws,
            &retry,
            json!({"admission_id":"same-owner-admission","ref":{"profile_id":"fixture-profile","session_id":"live"},"status":"terminal","sequence":1,"execution_generation":1,"authority_epoch":7}),
        );
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: None,
    };
    let entry = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "Hello".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let journal = Journal::new(root.clone()).unwrap();
    journal.save(&entry).unwrap();
    let (mut first, _) = worker(connection.clone(), scope.clone());
    assert!(first.submit(entry.clone(), &journal).unwrap_err().uncertain);
    let retained = journal.load(&entry.submission_id).unwrap();
    assert!(retained.attempted);
    assert_eq!(retained.create_id, entry.create_id);
    drop(first);
    let stored = DraftScope {
        session: Some("stored".into()),
        ..scope
    };
    let (mut second, replies) = worker(connection, stored.clone());
    second.submit(retained, &journal).unwrap();
    let mut manager = Conversation::new();
    for reply in replies.try_iter() {
        manager.replies.send(reply).unwrap();
    }
    manager.poll();
    let view = manager.view(&stored).unwrap();
    assert_eq!(view.messages.len(), 2);
    assert_eq!(view.messages[0].content, "Hello");
    assert!(view.admissions.is_empty());
    assert!(journal.load(&entry.submission_id).is_err());
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bot_registry_failure_is_not_permission_to_create_a_fork() {
    let root =
        std::env::temp_dir().join(format!("hermes-rune-bot-registry-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: None,
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let lookup = request(&mut ws, "session.list");
        assert_eq!(lookup["params"]["title"], "Bot Chat");
        assert_eq!(lookup["params"]["include_hidden"], true);
        ws.send(Message::Text(json!({"jsonrpc":"2.0","id":lookup["id"],"error":{"code":4009,"message":"profile_warming_up"}}).to_string().into())).unwrap();
        ws.get_mut()
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        assert!(
            ws.read().is_err(),
            "A failed named registry read must never mint another Bot Chat"
        );
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: None,
    };
    let entry = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "Hello bot".into(),
            canonical_bot: true,
            ..Default::default()
        },
    )
    .unwrap();
    let journal = Journal::new(root.clone()).unwrap();
    journal.save(&entry).unwrap();
    let (mut worker, _) = worker(connection, scope);
    assert!(!worker.submit(entry, &journal).unwrap_err().uncertain);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn composer_model_pick_retries_only_one_typed_fence_refusal_with_new_journaled_identity() {
    let root = std::env::temp_dir().join(format!("hermes-rune-fence-{}", uuid::Uuid::new_v4()));
    let journal = Journal::new(root.clone()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(false, 0));
        let first = request(&mut ws, "session.mutate");
        assert_eq!(first["params"]["expected_revision"], 2);
        ws.send(Message::Text(json!({"jsonrpc":"2.0","id":first["id"],"error":{"code":4001,"message":"stale_generation","data":{"reason":"stale_generation","revision":6,"execution_generation":3}}}).to_string().into())).unwrap();
        let second = request(&mut ws, "session.mutate");
        assert_ne!(
            second["params"]["request_id"],
            first["params"]["request_id"]
        );
        assert_eq!(second["params"]["expected_revision"], 6);
        assert_eq!(second["params"]["expected_generation"], 3);
        assert_eq!(second["params"]["payload"], first["params"]["payload"]);
        answer(
            &mut ws,
            &second,
            json!({"session_id":"live","revision":7,"status":"applied"}),
        );
    });
    let (mut worker, _) = worker(connection.clone(), scope.clone());
    let snapshot = worker.transport().unwrap().resume("stored").unwrap();
    let mut entry = Prepared::new(&connection, &scope, SendInput::default()).unwrap();
    let result = worker
        .mutate(
            &mut entry,
            &journal,
            snapshot,
            "model",
            json!({"model":"picked","provider":"custom"}),
        )
        .unwrap();
    assert_eq!(result.revision, 7);
    assert!(entry.mutations[0].complete);
    let saved = journal.load(&entry.submission_id).unwrap();
    assert_eq!(saved.mutations[0].params["expected_revision"], 6);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ambiguous_model_pick_keeps_exact_fences_and_request_identity_for_explicit_retry() {
    let root = std::env::temp_dir().join(format!(
        "hermes-rune-ambiguous-fence-{}",
        uuid::Uuid::new_v4()
    ));
    let journal = Journal::new(root.clone()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(false, 0));
        let mutation = request(&mut ws, "session.mutate");
        drop(ws);
        mutation["params"].clone()
    });
    let (mut worker, _) = worker(connection.clone(), scope.clone());
    let snapshot = worker.transport().unwrap().resume("stored").unwrap();
    let mut entry = Prepared::new(&connection, &scope, SendInput::default()).unwrap();
    let error = worker
        .mutate(
            &mut entry,
            &journal,
            snapshot,
            "model",
            json!({"model":"picked","provider":"custom"}),
        )
        .err()
        .unwrap();
    assert!(error.uncertain);
    let wire = server.join().unwrap();
    let saved = journal.load(&entry.submission_id).unwrap();
    let mut retained = saved.mutations[0].params.clone();
    retained["profile"] = json!("work");
    assert_eq!(retained, wire);
    assert!(!saved.mutations[0].refused);
    assert!(!saved.mutations[0].complete);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn workspace_change_is_session_owned_cas_and_publishes_normalized_readback_without_a_prompt() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        let mut initial = snapshot(false, 0);
        initial["info"]["cwd"] = json!("/old");
        answer(&mut ws, &resume, initial);
        history(&listener, json!([]));
        let mutation = request(&mut ws, "session.mutate");
        assert_eq!(mutation["params"]["operation"], "workspace");
        assert_eq!(mutation["params"]["expected_revision"], 2);
        assert_eq!(mutation["params"]["expected_generation"], 0);
        assert_eq!(mutation["params"]["request_id"], "retained-folder-intent");
        assert_eq!(mutation["params"]["payload"]["cwd"], "/requested/../new");
        answer(
            &mut ws,
            &mutation,
            json!({"session_id":"live","operation":"workspace","revision":3,"cwd":"/new"}),
        );
        let read = request(&mut ws, "session.resume");
        let mut current = snapshot(false, 0);
        current["info"]["cwd"] = json!("/new");
        current["revision"] = json!(3);
        answer(&mut ws, &read, current);
        history(&listener, json!([]));
    });
    let (mut worker, replies) = worker(connection, scope);
    let cwd = worker
        .change_cwd(super::super::workspace::Intent {
            path: "/requested/../new".into(),
            request_id: "retained-folder-intent".into(),
            stored: "stored".into(),
            runtime: "live".into(),
            revision: 2,
            generation: 0,
            authority: 7,
        })
        .unwrap();
    assert_eq!(cwd, "/new");
    let snapshots = replies
        .try_iter()
        .filter_map(|reply| {
            if let Reply::Snapshot { snapshot, .. } = reply {
                Some(snapshot)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[1].info["cwd"], "/new");
    assert_eq!(snapshots[1].metadata["revision"], 3);
    server.join().unwrap();
}

#[test]
fn confirmed_restore_uses_durable_rewind_and_never_submits_a_new_user_prompt() {
    let root = std::env::temp_dir().join(format!("hermes-rewind-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(false, 0));
        history(
            &listener,
            json!([{"role":"user","row_id":91,"text":"old question"}]),
        );
        let rewind = request(&mut ws, "session.mutate");
        assert_eq!(rewind["params"]["operation"], "rewind");
        assert_eq!(rewind["params"]["payload"], json!({"target_message_id":91}));
        assert_eq!(rewind["params"]["expected_revision"], 2);
        assert_eq!(rewind["params"]["expected_generation"], 0);
        answer(
            &mut ws,
            &rewind,
            json!({"session_id":"live","operation":"rewind","revision":3}),
        );
        let readback = request(&mut ws, "session.resume");
        let mut current = snapshot(false, 0);
        current["revision"] = json!(3);
        answer(&mut ws, &readback, current);
        history(&listener, json!([]));
        ws.get_mut()
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        assert!(
            ws.read().is_err(),
            "Restore only must never submit a prompt, create a session or mutate model policy"
        );
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let prepared = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "old question".into(),
            model: Some(ModelKey {
                provider: "fixture".into(),
                id: "pending-choice".into(),
            }),
            model_changed: true,
            reasoning: Some("high".into()),
            reasoning_changed: true,
            rewind: Some(Rewrite {
                target: 91,
                revision: 2,
                generation: 0,
                restore_only: true,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let journal = Journal::new(root.clone()).unwrap();
    journal.save(&prepared).unwrap();
    let id = prepared.submission_id.clone();
    let (mut worker, replies) = worker(connection, scope);
    worker.submit(prepared, &journal).unwrap();
    assert!(journal.load(&id).is_err());
    assert!(
        replies
            .try_iter()
            .any(|r| matches!(r,Reply::Admitted{status,..}if status=="restored"))
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn lost_rewind_receipt_retries_exact_identity_and_fences_even_after_revision_advanced() {
    let root = std::env::temp_dir().join(format!("hermes-rewind-lost-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(false, 0));
        let first = request(&mut ws, "session.mutate");
        drop(ws);
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        let mut current = snapshot(false, 0);
        current["revision"] = json!(3);
        answer(&mut ws, &resume, current.clone());
        let retry = request(&mut ws, "session.mutate");
        assert_eq!(
            first["params"], retry["params"],
            "Lost rewind must never mint a new request or change its CAS fences"
        );
        answer(
            &mut ws,
            &retry,
            json!({"session_id":"live","operation":"rewind","revision":3}),
        );
        let readback = request(&mut ws, "session.resume");
        answer(&mut ws, &readback, current);
        history(&listener, json!([]));
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let journal = Journal::new(root.clone()).unwrap();
    let mut entry = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "replacement".into(),
            rewind: Some(Rewrite {
                target: 91,
                revision: 2,
                generation: 0,
                restore_only: false,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let id = entry.submission_id.clone();
    let (mut first, _) = worker(connection.clone(), scope.clone());
    let snap = first.transport().unwrap().resume("stored").unwrap();
    assert!(
        first
            .prepare_rewind(&mut entry, &journal, snap)
            .err()
            .unwrap()
            .uncertain
    );
    let mut saved = journal.load(&id).unwrap();
    assert!(!saved.mutations[0].refused);
    assert!(!saved.mutations[0].complete);
    let (mut retry, _) = worker(connection, scope);
    let snap = retry.transport().unwrap().resume("stored").unwrap();
    retry.prepare_rewind(&mut saved, &journal, snap).unwrap();
    assert!(journal.load(&id).unwrap().mutations[0].complete);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn multimodal_edit_stages_exact_files_and_images_before_rewind_then_admits_one_owned_turn() {
    let root = std::env::temp_dir().join(format!("hermes-multimodal-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(false, 0));
        history(&listener, json!([]));
        let (mut http, _) = listener.accept().unwrap();
        http.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
        let mut reader = BufReader::new(&mut http);
        let mut headers = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().unwrap();
            }
            headers.push_str(&line);
        }
        assert!(headers.starts_with("POST /api/chat/image-upload?"));
        assert!(headers.contains("profile=work"));
        let mut body = vec![0; length];
        std::io::Read::read_exact(&mut reader, &mut body).unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data_url"], "data:image/png;base64,aW1hZ2U=");
        let response = json!({"path":"/staged/image.png","mime_type":"image/png"}).to_string();
        write!(http,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
        drop(http);
        let file = request(&mut ws, "file.attach");
        assert_eq!(
            file["params"]["data_url"],
            "data:text/plain;base64,bm90ZXM="
        );
        answer(
            &mut ws,
            &file,
            json!({"attached":true,"ref_text":"@file:staged-notes.txt"}),
        );
        let rewind = request(&mut ws, "session.mutate");
        assert_eq!(rewind["params"]["operation"], "rewind");
        assert_eq!(rewind["params"]["payload"]["target_message_id"], 91);
        answer(
            &mut ws,
            &rewind,
            json!({"session_id":"live","operation":"rewind","revision":3}),
        );
        let readback = request(&mut ws, "session.resume");
        let mut current = snapshot(false, 0);
        current["revision"] = json!(3);
        answer(&mut ws, &readback, current);
        history(&listener, json!([]));
        let submit = request(&mut ws, "prompt.submit");
        assert_eq!(
            submit["params"]["text"],
            "@file:staged-notes.txt\n\nEdited public request"
        );
        assert_eq!(
            submit["params"]["api_text"],
            "@file:staged-notes.txt\n\nprivate-prefix\nEdited public request\nprivate-suffix"
        );
        assert_eq!(
            submit["params"]["attachments"],
            json!([{"path":"/staged/image.png","mime":"image/png"}])
        );
        answer(
            &mut ws,
            &submit,
            json!({"ref":{"session_id":"live"},"admission_id":"admitted","status":"queued","execution_generation":0}),
        );
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let row = HistoryMessage::from_row(
        &json!({"role":"user","row_id":91,"text":"Public request","content":[{"type":"text","text":"private-prefix\nPublic request\nprivate-suffix"},{"type":"image_url","image_url":{"url":"data:image/png;base64,aW1hZ2U="}},{"type":"file","path":"data:text/plain;base64,bm90ZXM=","name":"Notes"}]}),
    );
    let replay = ReplayBundle::capture(&connection, &scope, &row)
        .unwrap()
        .with_text("Edited public request")
        .unwrap();
    let prepared = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "Edited public request".into(),
            replay: Some(replay),
            rewind: Some(Rewrite {
                target: 91,
                revision: 2,
                generation: 0,
                restore_only: false,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let journal = Journal::new(root.clone()).unwrap();
    journal.save(&prepared).unwrap();
    let (mut worker, _) = worker(connection, scope);
    worker.submit(prepared, &journal).unwrap();
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_multimodal_bytes_fail_before_any_rewind_or_admission() {
    let root = std::env::temp_dir().join(format!(
        "hermes-multimodal-refused-{}",
        uuid::Uuid::new_v4()
    ));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(false, 0));
        history(&listener, json!([]));
        ws.get_mut()
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        assert!(
            ws.read().is_err(),
            "Unrecoverable media cannot be followed by a history mutation or admission"
        );
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let row = HistoryMessage::from_row(
        &json!({"role":"user","row_id":91,"content":[{"type":"text","text":"Keep me"},{"type":"image_url","image_url":"data:image/png;base64,%%%"}]}),
    );
    let replay = ReplayBundle::capture(&connection, &scope, &row).unwrap();
    let prepared = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "Keep me".into(),
            replay: Some(replay),
            rewind: Some(Rewrite {
                target: 91,
                revision: 2,
                generation: 0,
                restore_only: false,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let id = prepared.submission_id.clone();
    let journal = Journal::new(root.clone()).unwrap();
    journal.save(&prepared).unwrap();
    let (mut worker, _) = worker(connection, scope);
    assert!(worker.submit(prepared, &journal).is_err());
    assert!(journal.load(&id).unwrap().mutations.is_empty());
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn multimodal_restore_returns_frozen_original_input_without_upload_or_prompt() {
    let root = std::env::temp_dir().join(format!("hermes-restore-media-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        answer(&mut ws, &resume, snapshot(false, 0));
        history(&listener, json!([]));
        let rewind = request(&mut ws, "session.mutate");
        assert_eq!(rewind["params"]["payload"]["target_message_id"], 91);
        answer(
            &mut ws,
            &rewind,
            json!({"session_id":"live","operation":"rewind","revision":3}),
        );
        let read = request(&mut ws, "session.resume");
        let mut current = snapshot(false, 0);
        current["revision"] = json!(3);
        answer(&mut ws, &read, current);
        history(&listener, json!([]));
        ws.get_mut()
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        assert!(ws.read().is_err());
    });
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let row = HistoryMessage::from_row(
        &json!({"role":"user","row_id":91,"content":[{"type":"text","text":"Original"},{"type":"image_url","image_url":"data:image/png;base64,aW1hZ2U="},{"type":"file","path":"data:text/plain;base64,bm90ZXM=","name":"Notes"}]}),
    );
    let replay = ReplayBundle::capture(&connection, &scope, &row).unwrap();
    let entry = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "Original".into(),
            replay: Some(replay),
            rewind: Some(Rewrite {
                target: 91,
                revision: 2,
                generation: 0,
                restore_only: true,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let journal = Journal::new(root.clone()).unwrap();
    journal.save(&entry).unwrap();
    let (mut worker, replies) = worker(connection, scope);
    worker.submit(entry, &journal).unwrap();
    let bundle = replies
        .try_iter()
        .find_map(|reply| {
            if let Reply::Admitted {
                restored_replay, ..
            } = reply
            {
                restored_replay
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(bundle.attachments.len(), 2);
    assert!(
        bundle
            .attachments
            .iter()
            .all(|a| a.frozen.as_deref() == Some(a.source.as_ref()))
    );
    assert_eq!(bundle.visible, "Original");
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn persisted_policy_stamp_defers_reasoning_and_never_acknowledges_a_newer_a_b_a_choice() {
    let root = std::env::temp_dir().join(format!("hermes-policy-stamp-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let mut composer = crate::composer::Composer::new();
    composer.choose_reasoning(scope.clone(), Some("high".into()));
    let stamp = composer.draft(&scope).unwrap().policy_stamp();
    let prepared = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "test message".into(),
            reasoning: Some("high".into()),
            reasoning_changed: true,
            policy_stamp: Some(stamp.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let journal = Journal::new(root.clone()).unwrap();
    journal.save(&prepared).unwrap();
    let prepared = journal.load(&prepared.submission_id).unwrap();
    assert_eq!(prepared.input.policy_stamp.as_ref(), Some(&stamp));
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        let mut info = snapshot(true, 0);
        info["info"]["model"] = json!("running-model");
        info["info"]["provider"] = json!("running-provider");
        info["info"]["reasoning_effort"] = json!("low");
        answer(&mut ws, &resume, info);
        history(&listener, json!([]));
        let mutation = request(&mut ws, "session.mutate");
        assert_eq!(mutation["params"]["operation"], "reasoning");
        assert_eq!(mutation["params"]["payload"], json!({"effort":"high"}));
        answer(
            &mut ws,
            &mutation,
            json!({"session_id":"live","operation":"reasoning","status":"deferred","revision":2,"execution_generation":1,"effort":"high"}),
        );
        let first_submit = request(&mut ws, "prompt.submit");
        drop(ws);
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        let mut info = snapshot(true, 0);
        info["info"]["model"] = json!("running-model");
        info["info"]["provider"] = json!("running-provider");
        info["info"]["reasoning_effort"] = json!("low");
        answer(&mut ws, &resume, info);
        history(&listener, json!([]));
        // A completed deferred mutation is projected from its retained receipt;
        // only the exact unknown prompt is retried, never the policy mutation.
        let submit = request(&mut ws, "prompt.submit");
        assert_eq!(submit["params"], first_submit["params"]);
        answer(
            &mut ws,
            &submit,
            json!({"admission_id":"queued-policy","ref":{"profile_id":"fixture-profile","session_id":"live"},"status":"queued","sequence":1,"execution_generation":null,"authority_epoch":7}),
        );
    });
    let submission = prepared.submission_id.clone();
    let (mut first, _) = worker(connection.clone(), scope.clone());
    assert!(first.submit(prepared, &journal).unwrap_err().uncertain);
    let recovered = journal.load(&submission).unwrap();
    assert_eq!(
        recovered.mutations[0].receipt.as_ref().unwrap()["status"],
        "deferred"
    );
    let (mut worker, replies) = worker(connection, scope.clone());
    worker.submit(recovered, &journal).unwrap();
    // The original receipt has arrived in the worker queue, but a later input owns the UI now.
    composer.choose_reasoning(scope.clone(), Some("low".into()));
    composer.choose_reasoning(scope.clone(), Some("high".into()));
    let mut manager = Conversation::new();
    for reply in replies.try_iter() {
        manager.replies.send(reply).unwrap();
    }
    let updates = manager.poll();
    let policy = &manager.view(&scope).unwrap().policy;
    assert_eq!(policy.reasoning.as_deref(), Some("low"));
    assert_eq!(policy.next_reasoning(), Some("high"));
    assert_eq!(policy.model.as_deref(), Some("running-model"));
    let mut observed = false;
    for update in updates {
        if let Update::Admitted {
            policy_stamp,
            policy_fields,
            ..
        } = update
        {
            observed = true;
            assert_eq!(policy_stamp.as_ref(), Some(&stamp));
            assert!(policy_fields.reasoning);
            assert!(!composer.settle_policy(&scope, policy_stamp.as_ref().unwrap(), policy_fields));
        }
    }
    assert!(observed);
    composer.reconcile_policy(&scope, policy);
    assert!(composer.draft(&scope).unwrap().reasoning_changed);
    assert_eq!(
        composer.draft(&scope).unwrap().reasoning.as_deref(),
        Some("high")
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_session_info_updates_clean_fields_without_inventing_a_missing_provider() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let resume = request(&mut ws, "session.resume");
        let mut initial = snapshot(true, 0);
        initial["info"]["model"] = json!("old");
        initial["info"]["provider"] = json!("old-route");
        initial["info"]["reasoning_effort"] = json!("low");
        answer(&mut ws, &resume, initial);
        history(&listener, json!([]));
        event(
            &mut ws,
            1,
            "session.info",
            json!({"model":"unlisted-current","reasoning_effort":"ultra","reasoning_effort_wire":"max","service_tier":"normal","yolo":false}),
        );
    });
    let (mut worker, replies) = worker(connection, scope.clone());
    worker.attach().unwrap();
    let event = worker.transport().unwrap().read_event().unwrap().unwrap();
    worker.event(event).unwrap();
    let mut conversation = Conversation::new();
    for reply in replies.try_iter() {
        conversation.replies.send(reply).unwrap();
    }
    conversation.poll();
    let policy = &conversation.view(&scope).unwrap().policy;
    assert_eq!(policy.model.as_deref(), Some("unlisted-current"));
    assert!(policy.provider.is_none());
    assert_eq!(policy.reasoning_wire.as_deref(), Some("max"));
    assert!(policy.approval_mode.is_none());
    let mut composer = crate::composer::Composer::new();
    composer.choose_approval(scope.clone(), Some("smart".into()));
    composer.reconcile_policy(&scope, policy);
    assert_eq!(composer.model_name(&scope), Some("unlisted-current"));
    assert!(composer.effective_model_key(&scope).is_none());
    assert_eq!(
        composer.draft(&scope).unwrap().reasoning.as_deref(),
        Some("ultra")
    );
    assert!(composer.draft(&scope).unwrap().approval_changed);
    server.join().unwrap();
}
