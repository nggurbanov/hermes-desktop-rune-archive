use super::*;
use std::{net::TcpListener, sync::Arc, thread};
use tungstenite::Message;
fn scope(epoch: u64, profile: &str) -> DraftScope {
    DraftScope {
        connection: epoch,
        profile: profile.into(),
        session: Some("parent".into()),
    }
}
#[test]
fn terminal_children_retire_without_reviving_on_late_events_or_rosters() {
    let owner = scope(1, "work");
    let mut state = Subagents::new();
    state.event(
        &owner,
        "subagent.start",
        &json!({"subagent_id":"a","child_session_id":"child-a","goal":"Inspect images"}),
    );
    state.event(
        &owner,
        "subagent.complete",
        &json!({"subagent_id":"a","status":"completed","summary":"All checked"}),
    );
    state.event(
        &owner,
        "subagent.start",
        &json!({"subagent_id":"a","status":"running"}),
    );
    assert_eq!(state.rows(&owner)[0].status, "completed");
    state.turn_started(&owner);
    state.event(
        &owner,
        "subagent.progress",
        &json!({"subagent_id":"a","text":"late"}),
    );
    reconcile(
        state.rosters.get_mut(&owner).unwrap(),
        &json!({"subagents":[{"subagent_id":"a","status":"running"}]}),
    );
    assert!(state.rows(&owner).is_empty());
}
#[test]
fn child_rosters_never_cross_connection_profile_or_parent_and_retirement_detaches_only_scope() {
    let one = scope(1, "work");
    let two = scope(1, "personal");
    let three = scope(2, "work");
    let mut state = Subagents::new();
    for owner in [&one, &two, &three] {
        state.event(
            owner,
            "subagent.start",
            &json!({"subagent_id":"same","child_session_id":"same-child"}),
        );
    }
    state.close_profile(1, "work");
    assert!(state.rows(&one).is_empty());
    assert_eq!(state.rows(&two).len(), 1);
    assert_eq!(state.rows(&three).len(), 1);
    state.close_connection(1);
    assert!(state.rows(&two).is_empty());
    assert_eq!(state.rows(&three).len(), 1);
}
#[test]
fn roster_read_cannot_overwrite_newer_stream_and_unknown_completion_is_terminal() {
    let mut roster = Roster::default();
    merge_event(
        &mut roster,
        "subagent.start",
        &json!({"subagent_id":"a","goal":"Inspect"}),
    );
    merge_event(
        &mut roster,
        "subagent.complete",
        &json!({"subagent_id":"a","status":"unexpected"}),
    );
    assert_eq!(roster.rows[0].status, "failed");
    reconcile(
        &mut roster,
        &json!({"subagents":[{"subagent_id":"a","status":"running"}]}),
    );
    assert_eq!(roster.rows[0].status, "failed");
    let owner = scope(1, "work");
    let mut state = Subagents::new();
    state.init(Arc::new(|| {}));
    roster.runtime = "live".into();
    roster.version = 3;
    roster.pending = true;
    state.rosters.insert(owner.clone(), roster);
    state
        .tx
        .as_ref()
        .unwrap()
        .send(Reply {
            scope: owner.clone(),
            runtime: "live".into(),
            version: 2,
            result: Ok(json!({"subagents":[]})),
        })
        .unwrap();
    state.poll(None);
    assert_eq!(state.rows(&owner).len(), 1);
}
fn snapshot() -> Value {
    json!({"session_id":"live-child","stored_session_id":"child","running":true,"revision":2,"execution_generation":1,"authority_epoch":7,"replay_epoch":"child-epoch","last_sequence":0,"messages":[{"role":"user","content":"Inspect the images"}],"prompts":[],"pending":[],"info":{"profile_name":"work","desktop_protocol":"hermes-gateway-v1"}})
}
#[test]
fn actual_socket_spectator_attaches_lazily_streams_and_closes_without_any_agent_action() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let request: Value =
            serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(request["method"], "session.resume");
        assert_eq!(request["params"]["session_id"], "child");
        assert_eq!(request["params"]["profile"], "work");
        assert_eq!(request["params"]["lazy"], true);
        assert_eq!(request["params"]["defer_history"], false);
        assert_ne!(request["params"]["omit_messages"], true);
        ws.send(Message::Text(
            json!({"jsonrpc":"2.0","id":request["id"],"result":snapshot()})
                .to_string()
                .into(),
        ))
        .unwrap();
        for (seq, kind, payload) in [
            (
                1,
                "message.delta",
                json!({"text":"Checked all images","turn_id":"t","segment":0,"offset":0}),
            ),
            (
                2,
                "reasoning.delta",
                json!({"text":"Verify each reference"}),
            ),
            (
                3,
                "tool.complete",
                json!({"name":"read_file","tool_name":"read_file"}),
            ),
            (4, "message.complete", json!({"status":"completed"})),
        ] {
            ws.send(Message::Text(json!({"method":"event","params":{"session_id":"live-child","type":kind,"replay_epoch":"child-epoch","seq":seq,"execution_generation":1,"authority_epoch":7,"payload":payload}}).to_string().into())).unwrap();
        }
        let frame = ws.read();
        assert!(
            frame.is_err() || matches!(frame, Ok(Message::Close(_))),
            "Closing a spectator must not send a gateway action"
        );
    });
    let watch = worker::start(
        Connection::Url {
            endpoint: format!("http://{addr}"),
            token: Some("synthetic-token".into()),
        },
        "work".into(),
        "child".into(),
        Arc::new(|| {}),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut finished = false;
    while Instant::now() < deadline {
        let result = watch
            .receive
            .recv_timeout(Duration::from_millis(200))
            .unwrap();
        if !result.running
            && result
                .messages
                .iter()
                .any(|m| m.content.contains("Checked all images"))
        {
            assert_eq!(result.reasoning, "Verify each reference");
            assert!(!result.tools.is_empty());
            finished = true;
            break;
        }
    }
    assert!(finished);
    drop(watch);
    server.join().unwrap();
}
#[test]
fn watch_refuses_a_different_stored_child() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let request: Value =
            serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
        let mut reply = snapshot();
        reply["stored_session_id"] = json!("other-child");
        ws.send(Message::Text(
            json!({"jsonrpc":"2.0","id":request["id"],"result":reply})
                .to_string()
                .into(),
        ))
        .unwrap();
    });
    let mut transport = SessionTransport::connect(
        &Connection::Url {
            endpoint: format!("http://{addr}"),
            token: None,
        },
        "work",
    )
    .unwrap();
    assert!(
        transport
            .watch("child")
            .err()
            .unwrap()
            .message
            .contains("different stored")
    );
    server.join().unwrap();
}

#[test]
fn a_settled_roster_stops_gateway_polling_and_hidden_chats_stop_their_timer() {
    let listener = Arc::new(TcpListener::bind("127.0.0.1:0").unwrap());
    let addr = listener.local_addr().unwrap();
    let peer = listener.clone();
    let server = thread::spawn(move || {
        let (stream, _) = peer.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let request: Value =
            serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(request["method"], "subagent.list");
        assert_eq!(request["params"]["session_id"], "live-parent");
        assert_eq!(request["params"]["profile"], "work");
        ws.send(Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"result":{"subagents":[{"subagent_id":"done","status":"completed","child_session_id":"child"}]}}).to_string().into())).unwrap();
    });
    let owner = scope(1, "work");
    let connection = Connection::Url {
        endpoint: format!("http://{addr}"),
        token: None,
    };
    let mut view = ConversationView::default();
    view.runtime = "live-parent".into();
    let mut manager = Subagents::new();
    manager.init(Arc::new(|| {}));
    manager.observe(&connection, owner.clone(), Some(&view), None);
    server.join().unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while manager.rows(&owner).is_empty() && Instant::now() < until {
        manager.poll(None);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(manager.rows(&owner)[0].status, "completed");
    manager.rosters.get_mut(&owner).unwrap().next = Some(Instant::now() - Duration::from_secs(1));
    manager.observe(&connection, owner.clone(), Some(&view), None);
    assert!(manager.wake_after().is_none());
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "A settled child roster must not reconnect just because five seconds elapsed"
    );
    manager.event(
        &owner,
        "subagent.start",
        &json!({"subagent_id":"live","status":"running"}),
    );
    manager.next_poll = Some(Duration::from_secs(5));
    manager.poll(None);
    assert!(
        manager.wake_after().is_none(),
        "A hidden chat must not retain its roster wake timer"
    );
}
