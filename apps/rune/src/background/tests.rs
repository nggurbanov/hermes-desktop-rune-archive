use super::*;
use crate::gateway::Connection;
use serde_json::json;
fn scope() -> DraftScope {
    DraftScope {
        draft_origin: None,
        profile_intent: crate::source::ProfileIntent::Backend,
        connection: 7,
        profile: "work".into(),
        session: Some("stored-one".into()),
    }
}
fn connection() -> Binding {
    source("http://127.0.0.1:1".into())
}
fn source(endpoint: String) -> Binding {
    Binding::new(
        Connection::Url {
            endpoint,
            token: None,
        },
        None,
    )
    .unwrap()
}
fn row(id: &str, status: &str, code: Option<i64>) -> Value {
    json!({"session_id":id,"status":status,"command":"npm run build\nignored second line","exit_code":code,"output_tail":"first\nsecond\nthird\nlast\n"})
}
fn state() -> Background {
    let mut b = Background::new();
    b.bind(connection(), scope(), "runtime-one".into());
    b
}
fn reconcile(b: &mut Background, rows: Vec<Value>, now: f64) -> Vec<(String, String)> {
    reconcile::snapshot(
        b.entries.get_mut(&scope()).unwrap(),
        &json!({"processes":rows}),
        now,
    )
}
#[test]
fn only_observed_running_to_exact_exited_transition_emits_once_with_actual_exit_and_tail() {
    let mut b = state();
    assert!(reconcile(&mut b, vec![row("old", "exited", Some(0))], 0.).is_empty());
    assert!(reconcile(&mut b, vec![row("live", "running", None)], 1.).is_empty());
    let done = reconcile(&mut b, vec![row("live", "exited", Some(2))], 2.);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].0, "Background process failed");
    assert_eq!(done[0].1, "npm run build (exit 2)\nsecond\nthird\nlast");
    assert!(reconcile(&mut b, vec![row("live", "exited", Some(2))], 3.).is_empty());
    assert!(reconcile(&mut b, vec![], 4.).is_empty());
    assert!(reconcile(&mut b, vec![row("live", "exited", Some(0))], 5.).is_empty());
    reconcile(&mut b, vec![row("alias", "running", None)], 6.);
    assert!(reconcile(&mut b, vec![row("alias", "completed", Some(0))], 7.).is_empty());
    assert_eq!(b.items(&scope())[0].state, State::Running);
}
#[test]
fn snapshots_keep_order_and_linger_without_resurrection_until_registry_id_disappears() {
    let mut b = state();
    reconcile(
        &mut b,
        vec![row("a", "running", None), row("b", "running", None)],
        0.,
    );
    reconcile(
        &mut b,
        vec![
            row("b", "running", None),
            row("c", "running", None),
            row("a", "exited", Some(0)),
        ],
        1.,
    );
    assert_eq!(
        b.items(&scope())
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    assert_eq!(b.items(&scope())[0].until, Some(5.));
    reconcile(
        &mut b,
        vec![row("a", "exited", Some(0)), row("b", "exited", Some(1))],
        3.,
    );
    assert_eq!(b.items(&scope())[0].until, Some(5.));
    assert_eq!(b.items(&scope())[1].until, Some(15.));
    b.poll(5., &scope(), true);
    assert_eq!(b.items(&scope()).len(), 1);
    reconcile(
        &mut b,
        vec![row("a", "exited", Some(0)), row("b", "exited", Some(1))],
        6.,
    );
    assert_eq!(b.items(&scope()).len(), 1);
    reconcile(&mut b, vec![], 7.);
    reconcile(&mut b, vec![row("a", "running", None)], 8.);
    assert_eq!(b.items(&scope()).len(), 1);
}
#[test]
fn pending_stop_retains_row_on_failure_and_dismisses_only_a_confirmed_reply() {
    let mut b = state();
    reconcile(&mut b, vec![row("a", "running", None)], 0.);
    let (send, receive) = mpsc::sync_channel(32);
    b.send = Some(send);
    b.stop(&scope(), "a");
    let work = receive.try_recv().unwrap();
    assert!(b.items(&scope())[0].stopping);
    b.apply(
        worker::Reply {
            scope: work.scope,
            runtime: work.runtime,
            generation: work.generation,
            id: work.id,
            action: work.action,
            result: Ok(json!({"status":"error","error":"permission refused"})),
        },
        1.,
        &mut vec![],
    );
    assert_eq!(b.items(&scope()).len(), 1);
    assert!(!b.items(&scope())[0].stopping);
    assert!(b.error(&scope()).unwrap().contains("permission refused"));
    b.stop(&scope(), "a");
    let work = receive.try_recv().unwrap();
    b.apply(
        worker::Reply {
            scope: work.scope,
            runtime: work.runtime,
            generation: work.generation,
            id: work.id,
            action: work.action,
            result: Ok(json!({"status":"killed"})),
        },
        2.,
        &mut vec![],
    );
    assert!(b.items(&scope()).is_empty());
    assert!(reconcile(&mut b, vec![row("a", "exited", Some(-9))], 3.).is_empty());
    assert!(b.items(&scope()).is_empty());
}
#[test]
fn rewind_marks_ids_before_kill_and_rejects_old_runtime_readbacks() {
    let mut b = state();
    reconcile(
        &mut b,
        vec![row("live", "running", None), row("done", "exited", Some(0))],
        0.,
    );
    let (send, receive) = mpsc::sync_channel(32);
    b.send = Some(send);
    b.poll(0., &scope(), true);
    let old_read = receive.try_recv().unwrap();
    b.rewind(&scope());
    let kill = receive.try_recv().unwrap();
    assert!(matches!(kill.action, worker::Action::RewindKill(_)));
    assert!(b.items(&scope()).is_empty());
    b.apply(
        worker::Reply {
            scope: old_read.scope,
            runtime: old_read.runtime,
            generation: old_read.generation,
            id: old_read.id,
            action: old_read.action,
            result: Ok(json!({"processes":[row("live","running",None)]})),
        },
        1.,
        &mut vec![],
    );
    assert!(b.items(&scope()).is_empty());
    b.bind(connection(), scope(), "new-runtime".into());
    b.apply(
        worker::Reply {
            scope: kill.scope,
            runtime: kill.runtime,
            generation: kill.generation,
            id: kill.id,
            action: kill.action,
            result: Err("session not found".into()),
        },
        2.,
        &mut vec![],
    );
    assert!(!b.entries[&scope()].gone);
}
#[test]
fn refresh_timers_follow_visibility_known_running_and_actual_reopen_not_plain_remount() {
    let mut b = state();
    let (send, receive) = mpsc::sync_channel(32);
    b.send = Some(send);
    b.poll(0., &scope(), false);
    let work = receive.try_recv().unwrap();
    b.apply(
        worker::Reply {
            scope: work.scope,
            runtime: work.runtime,
            generation: work.generation,
            id: work.id,
            action: work.action,
            result: Ok(json!({"processes":[row("a","running",None)]})),
        },
        0.,
        &mut vec![],
    );
    b.poll(5., &scope(), false);
    assert!(receive.try_recv().is_err());
    b.poll(5., &scope(), true);
    let work = receive.try_recv().unwrap();
    b.apply(
        worker::Reply {
            scope: work.scope,
            runtime: work.runtime,
            generation: work.generation,
            id: work.id,
            action: work.action,
            result: Err("session not found".into()),
        },
        5.,
        &mut vec![],
    );
    b.bind(connection(), scope(), "runtime-one".into());
    b.trigger(&scope(), "runtime-one");
    b.poll(100., &scope(), true);
    assert!(receive.try_recv().is_err());
    b.stream_opened(&scope());
    b.poll(101., &scope(), false);
    assert!(receive.try_recv().is_ok());
}
#[test]
fn only_needed_timers_wake_an_idle_window_and_authority_refusals_do_not_latch_gone() {
    let mut b = state();
    assert!(b.next_wake_after(0., &scope(), true).is_none());
    b.initialized = true;
    assert_eq!(
        b.next_wake_after(0., &scope(), true),
        Some(std::time::Duration::from_millis(100))
    );
    reconcile(&mut b, vec![row("done", "exited", Some(0))], 1.);
    let entry = b.entries.get_mut(&scope()).unwrap();
    entry.refresh = false;
    assert_eq!(
        b.next_wake_after(1., &scope(), false),
        Some(std::time::Duration::from_secs(4))
    );
    b.poll(5., &scope(), false);
    assert!(b.next_wake_after(5., &scope(), false).is_none());
    assert!(gone("Process RPC refused (4001; reason=not_found)"));
    assert!(gone("Workspace RPC refused (4001)"));
    for error in [
        "Process RPC refused (4001; reason=wrong_owner)",
        "Process RPC refused (4001; reason=other)",
        "Workspace RPC refused (4004)",
        "Workspace RPC refused (401)",
        "Gateway read timed out",
        "session_not_owned",
    ] {
        assert!(!gone(error), "{error}");
    }
}
#[test]
fn real_process_worker_sends_owned_snapshot_and_confirmed_kill_to_controlled_websocket_peer() {
    use std::{
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        for expected in ["process.list", "process.kill"] {
            let deadline = Instant::now() + Duration::from_secs(4);
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let raw = socket.read().unwrap();
            let request: Value = serde_json::from_str(raw.to_text().unwrap()).unwrap();
            assert_eq!(request["method"], expected);
            assert_eq!(request["params"]["profile"], "work");
            assert_eq!(request["params"]["session_id"], "runtime-one");
            let result = if expected == "process.list" {
                json!({"processes":[row("owned-process","running",None)]})
            } else {
                assert_eq!(request["params"]["process_id"], "owned-process");
                json!({"status":"killed"})
            };
            socket
                .send(tungstenite::Message::Text(
                    json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
    });
    let mut b = Background::new();
    b.init(std::sync::Arc::new(|| {}));
    b.bind(source(endpoint), scope(), "runtime-one".into());
    let deadline = Instant::now() + Duration::from_secs(4);
    while b.items(&scope()).is_empty() {
        b.poll(0., &scope(), true);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    b.stop(&scope(), "owned-process");
    while !b.items(&scope()).is_empty() {
        b.poll(0.1, &scope(), true);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    server.join().unwrap();
}
#[test]
fn process_rpc_preserves_reason_priority_in_actual_gateway_responses() {
    use crate::{gateway::Gateway, pages::PageRequest};
    use std::{net::TcpListener, thread};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let errors = vec![
        json!({"code":4001,"message":"missing","data":{"reason":"not_found"}}),
        json!({"code":4001,"message":"authority changed","data":{"reason":"wrong_owner"}}),
        json!({"code":4001,"data":{"reason":{"invalid":"untrusted"}}}),
        json!({"code":4001}),
        json!({"code":4004}),
    ];
    let server = thread::spawn(move || {
        for error in errors {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let raw = socket.read().unwrap();
            let request: Value = serde_json::from_str(raw.to_text().unwrap()).unwrap();
            socket
                .send(tungstenite::Message::Text(
                    json!({"jsonrpc":"2.0","id":request["id"],"error":error})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
    });
    for expected in [true, false, false, true, false] {
        let mut gateway = Gateway::connect(&Connection::Url {
            endpoint: endpoint.clone(),
            token: None,
        })
        .unwrap();
        let error = gateway
            .page_request(
                "work",
                &PageRequest::rpc("process.list", json!({"session_id":"runtime-one"})),
            )
            .unwrap_err();
        assert_eq!(gone(&error), expected, "{error}");
    }
    server.join().unwrap();
}
#[test]
fn lost_kill_ack_keeps_the_row_and_later_snapshots_never_retry_the_write() {
    use std::{
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let mut methods = vec![];
        for phase in 0..3 {
            let deadline = Instant::now() + Duration::from_secs(4);
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let raw = socket.read().unwrap();
            let request: Value = serde_json::from_str(raw.to_text().unwrap()).unwrap();
            methods.push(request["method"].as_str().unwrap().to_string());
            if phase == 1 {
                assert_eq!(request["method"], "process.kill");
                continue;
            }
            assert_eq!(request["method"], "process.list");
            let result = json!({"processes":[row("p",if phase==0{"running"}else{"exited"},if phase==0{None}else{Some(0)})]});
            socket
                .send(tungstenite::Message::Text(
                    json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
        methods
    });
    let mut b = Background::new();
    b.init(std::sync::Arc::new(|| {}));
    b.bind(source(endpoint), scope(), "runtime-one".into());
    let deadline = Instant::now() + Duration::from_secs(4);
    while b.items(&scope()).is_empty() {
        b.poll(0., &scope(), true);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    b.stop(&scope(), "p");
    while b.items(&scope()).iter().any(|item| item.stopping) {
        b.poll(0.1, &scope(), true);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(b.items(&scope()).len(), 1);
    assert!(b.error(&scope()).is_some());
    while b.items(&scope())[0].state == State::Running {
        b.poll(6., &scope(), true);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    b.poll(30., &scope(), true);
    assert!(b.items(&scope()).is_empty());
    assert_eq!(
        server.join().unwrap(),
        vec!["process.list", "process.kill", "process.list"]
    );
}
#[test]
fn retired_connection_releases_rows_and_cancels_queued_source_io_without_stopping_any_process() {
    use std::{net::TcpListener, thread, time::Duration};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let mut b = Background::new();
    b.bind(source(endpoint), scope(), "runtime-one".into());
    reconcile(&mut b, vec![row("p", "running", None)], 0.);
    let (send, receive) = mpsc::sync_channel(32);
    b.send = Some(send);
    b.stop(&scope(), "p");
    let work = receive.try_recv().unwrap();
    b.expanded = Some((scope(), "p".into()));
    b.retire_connection(7);
    assert!(b.items(&scope()).is_empty());
    assert!(b.expanded.is_none());
    assert!(!work.active.load(Ordering::Acquire));
    assert!(b.next_wake_after(9., &scope(), true).is_none());
    let (send, _replies) = worker::start(std::sync::Arc::new(|| {}));
    send.send(work)
        .unwrap_or_else(|_| panic!("private background worker closed"));
    thread::sleep(Duration::from_millis(50));
    assert!(
        matches!(listener.accept(),Err(error)if error.kind()==std::io::ErrorKind::WouldBlock),
        "retired source must not receive a queued mutation"
    );
}

#[test]
fn parked_source_reads_keep_their_gateway_and_process_owner_while_another_source_is_visible() {
    use std::{
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };
    fn peer(states: Vec<&'static str>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let join = thread::spawn(move || {
            for state in states {
                let deadline = Instant::now() + Duration::from_secs(4);
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "missing owned process read");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let raw = socket.read().unwrap();
                let request: Value = serde_json::from_str(raw.to_text().unwrap()).unwrap();
                assert_eq!(request["method"], "process.list");
                assert_eq!(request["params"]["profile"], "work");
                assert_eq!(request["params"]["session_id"], "same-runtime");
                let result = json!({"processes":[row("same-process",state,if state=="exited" {Some(0)} else {None})]});
                socket
                    .send(tungstenite::Message::Text(
                        json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                            .to_string()
                            .into(),
                    ))
                    .unwrap();
            }
        });
        (endpoint, join)
    }
    let (a_endpoint, a_peer) = peer(vec!["running", "exited"]);
    let (b_endpoint, b_peer) = peer(vec!["running"]);
    let a = scope();
    let b = DraftScope {
        connection: 8,
        ..a.clone()
    };
    let mut monitor = Background::new();
    monitor.init(Arc::new(|| {}));
    monitor.bind(source(a_endpoint), a.clone(), "same-runtime".into());
    let deadline = Instant::now() + Duration::from_secs(4);
    while monitor.items(&a).is_empty() {
        monitor.poll(0., &a, true);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    monitor.bind(source(b_endpoint), b.clone(), "same-runtime".into());
    monitor.retain_connections(&[a.connection, b.connection]);
    while monitor.items(&b).is_empty() {
        monitor.poll(1., &b, true);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(monitor.items(&a)[0].state, State::Running);
    monitor.trigger(&a, "same-runtime");
    let mut finished = vec![];
    while monitor.items(&a)[0].state == State::Running {
        for update in monitor.poll(1., &b, true) {
            if let Update::Finished { scope, .. } = update {
                finished.push(scope);
            }
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(finished, vec![a.clone()]);
    assert_eq!(monitor.items(&b)[0].state, State::Running);
    monitor.expanded = Some((a.clone(), "same-process".into()));
    monitor.retain_connections(&[b.connection]);
    assert!(monitor.items(&a).is_empty());
    assert!(monitor.expanded.is_none());
    assert_eq!(monitor.items(&b).len(), 1);
    a_peer.join().unwrap();
    b_peer.join().unwrap();
}

#[test]
fn retaining_another_source_cancels_only_the_retired_queued_owner_and_ignores_its_receipt() {
    let a = scope();
    let b = DraftScope {
        connection: 8,
        ..a.clone()
    };
    let mut monitor = state();
    monitor.bind(
        source("http://127.0.0.1:2".into()),
        b.clone(),
        "runtime-two".into(),
    );
    let (send, receive) = mpsc::sync_channel(32);
    monitor.send = Some(send);
    monitor.poll(0., &b, true);
    let requests = receive.try_iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    monitor.retain_connections(&[b.connection]);
    for work in requests {
        if work.scope == a {
            assert!(!work.active.load(Ordering::Acquire));
            let mut updates = vec![];
            monitor.apply(
                worker::Reply {
                    scope: work.scope,
                    runtime: work.runtime,
                    generation: work.generation,
                    id: work.id,
                    action: work.action,
                    result: Ok(json!({"processes":[row("late","running",None)]})),
                },
                1.,
                &mut updates,
            );
            assert!(updates.is_empty());
        } else {
            assert!(work.active.load(Ordering::Acquire));
            assert_eq!(
                work.binding.stamp(),
                source("http://127.0.0.1:2".into()).stamp()
            );
        }
    }
    assert!(monitor.items(&a).is_empty());
    assert!(monitor.entries.contains_key(&b));
}
