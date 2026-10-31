//! App ownership contracts use private preferences and validated route receipts, never a gateway.
use super::*;
use crate::client::startup::Startup;
use serde_json::json;
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("hermes-route-owner-{}", uuid::Uuid::new_v4())))
    }
    fn path(&self) -> PathBuf {
        self.0.join("startup.json")
    }
    fn store(&self) -> Startup {
        Startup::load_at(self.path())
    }
    fn viewer(&self) -> (Viewer, Receiver<Request>) {
        let mut s = Viewer::empty();
        s.connection = Some(connection("current"));
        s.connection_form = false;
        s.connection_epoch = 37;
        s.profile = "work".into();
        s.selected_session = Some("stored-current".into());
        s.status = "Connected".into();
        s.messages = vec![HistoryMessage {
            role: "assistant".into(),
            content: "Current conversation history".into(),
            ..Default::default()
        }];
        s.tabs = vec![
            ConversationTab {
                profile_intent: crate::source::ProfileIntent::Backend,
                profile: "work".into(),
                session: "stored-current".into(),
                title: "Current".into(),
                bot: false,
            },
            ConversationTab {
                profile_intent: crate::source::ProfileIntent::Backend,
                profile: "work".into(),
                session: "stored-other".into(),
                title: "Other".into(),
                bot: false,
            },
        ];
        s.profiles = ["work", "archive"]
            .into_iter()
            .map(|name| Profile {
                name: name.into(),
                label: name.into(),
                description: String::new(),
                canonical_session: None,
            })
            .collect();
        s.device_startup = Some(self.store());
        s.routing.next_observe = Some(Instant::now() + Duration::from_secs(30));
        let scope = s.composer_scope();
        s.composer
            .draft_mut(scope)
            .text
            .replace_text("Unsent current input");
        let (send, requests) = mpsc::channel();
        let (_, receive) = mpsc::channel();
        s.network = Some(Network { send, receive });
        // No setup/init_editors: these unbound controllers cannot access a system draft store.
        (s, requests)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn connection(host: &str) -> Connection {
    Connection::Url {
        endpoint: format!("https://{host}.route-fixture.invalid/hermes"),
        token: Some("synthetic-principal".into()),
    }
}
fn principal(s: &Viewer) -> String {
    crate::client::connection_key(s.connection.as_ref().unwrap()).unwrap()
}
fn default_route() -> Route {
    Route::new(
        Some("11111111-1111-4111-8111-111111111111".into()),
        "archive".into(),
    )
    .unwrap()
}
fn validated_reply(s: &mut Viewer, profile: &str) -> mpsc::Sender<Result<Connection, String>> {
    let (send, receive) = mpsc::channel();
    s.routing.sequence += 1;
    s.routing.pending = Some(Pending {
        id: s.routing.sequence,
        target: Route::new(None, profile.into()).unwrap(),
        project_target: None,
        owner: s.route_owner(),
        reason: Reason::NewSession,
        stage: Stage::Direct(receive),
        destination: None,
        restore: None,
    });
    send
}
fn view(s: &Viewer) -> (DraftScope, String, EditorId, Vec<String>, Option<String>) {
    let scope = s.composer_scope();
    (
        scope.clone(),
        principal(s),
        s.composer.editor_id().clone(),
        s.messages.iter().map(|m| m.content.clone()).collect(),
        s.composer.draft(&scope).map(|d| d.text.text.clone()),
    )
}
#[test]
fn delayed_activation_cannot_replace_a_newer_read_or_new_conversation_owner() {
    for navigation in [false, true] {
        let root = Workspace::new();
        let (mut s, requests) = root.viewer();
        let old = s.composer_scope();
        let reply = validated_reply(&mut s, "archive");
        if navigation {
            s.open_session("newer-conversation".into());
            let scope = s.composer_scope();
            s.composer
                .draft_mut(scope)
                .text
                .replace_text("New owner input");
            assert!(
                matches!(requests.recv().unwrap().command,Command::History(profile,id) if profile=="work" && id=="newer-conversation")
            );
        } else {
            s.dispatch(Command::Sessions("work".into()));
            assert!(
                matches!(requests.recv().unwrap().command,Command::Sessions(profile) if profile=="work")
            );
        }
        let latest = view(&s);
        let sent = reply.send(Ok(connection("delayed")));
        assert_eq!(
            sent.is_err(),
            navigation,
            "navigation retires its old receiver immediately"
        );
        s.poll_device_routes(&mut Tx::default());
        assert_eq!(view(&s), latest);
        assert_eq!(
            s.composer.draft(&old).unwrap().text.text,
            "Unsent current input"
        );
        assert!(!s.device_route_pending());
        assert!(
            requests.try_recv().is_err(),
            "the stale receipt must not dispatch a connect or history read"
        );
    }
}
#[test]
fn an_explicit_current_workspace_draft_bypasses_the_global_device_default() {
    let root = Workspace::new();
    let mut store = root.store();
    store.set(default_route()).unwrap();
    let (mut s, requests) = root.viewer();
    let old = s.composer_scope();
    let source = principal(&s);
    s.new_draft_on_current_source();
    assert_eq!(principal(&s), source);
    assert_eq!(s.connection_epoch, old.connection);
    assert_eq!(s.profile, "work");
    assert!(s.selected_session.is_none());
    assert!(s.messages.is_empty());
    assert_eq!(
        s.composer.draft(&old).unwrap().text.text,
        "Unsent current input"
    );
    assert_eq!(s.default_profile_route(), Some(default_route()));
    assert!(!s.device_route_pending());
    assert!(
        s.client_error.is_none(),
        "an unavailable global source must not block an explicit workspace action"
    );
    assert!(requests.try_recv().is_err());
}
#[test]
fn observing_and_clearing_a_device_default_does_not_navigate_an_existing_conversation() {
    let root = Workspace::new();
    let (mut s, requests) = root.viewer();
    let current = view(&s);
    let mut other_window = root.store();
    other_window.set(default_route()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while s.default_profile_route() != Some(default_route()) {
        assert!(
            Instant::now() < deadline,
            "the private default mirror did not refresh"
        );
        s.poll_device_routes(&mut Tx::default());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(view(&s), current);
    assert!(!s.device_route_pending());
    s.clear_default_profile();
    assert_eq!(root.store().get().unwrap(), None);
    assert_eq!(view(&s), current);
    assert!(requests.try_recv().is_err());
}
#[test]
fn a_file_becoming_dirty_during_activation_keeps_its_source_profile_and_both_inputs() {
    let root = Workspace::new();
    let (mut s, requests) = root.viewer();
    let current = view(&s);
    let reply = validated_reply(&mut s, "archive");
    let owner = s.composer_scope();
    s.files.fixture(owner);
    let editor = s.files.editor.as_mut().unwrap();
    editor.text.insert("Unsaved file edit");
    assert!(editor.dirty());
    let file_text = editor.text.text.clone();
    reply.send(Ok(connection("validated-target"))).unwrap();
    s.poll_device_routes(&mut Tx::default());
    assert_eq!(view(&s), current);
    assert_eq!(s.files.editor.as_ref().unwrap().text.text, file_text);
    assert!(s.files.editor.as_ref().unwrap().dirty());
    assert!(
        s.client_error.is_some(),
        "the blocked activation must be visible"
    );
    assert!(!s.device_route_pending());
    assert!(requests.try_recv().is_err());
}
#[test]
fn unavailable_or_future_default_preferences_are_not_interpreted_as_no_default() {
    for unavailable in [true, false] {
        let root = Workspace::new();
        let (mut s, requests) = root.viewer();
        if unavailable {
            s.device_startup = None;
        } else {
            crate::client::private_write(
                &root.path(),
                &serde_json::to_vec(&json!({"version":9,"future":{"keep":true}})).unwrap(),
            )
            .unwrap();
            s.device_startup = Some(root.store());
        }
        let current = view(&s);
        s.new_draft();
        assert_eq!(
            view(&s),
            current,
            "a failed preference read cannot clear the current session/input"
        );
        assert!(s.client_error.is_some());
        assert!(!s.device_route_pending());
        assert!(requests.try_recv().is_err());
        if !unavailable {
            assert!(root.store().get().is_err());
        }
    }
}
#[test]
fn an_accepted_same_source_fresh_intent_preserves_stored_tabs_and_their_unsent_inputs() {
    let root = Workspace::new();
    let (mut s, requests) = root.viewer();
    let original = s.composer_scope();
    let source = principal(&s);
    let other = DraftScope {
        session: Some("stored-other".into()),
        draft_origin: None,
        ..original.clone()
    };
    s.composer
        .draft_mut(other.clone())
        .text
        .replace_text("Other tab input");
    let fresh = DraftScope {
        session: None,
        ..original.clone()
    };
    s.composer
        .draft_mut(fresh.clone())
        .text
        .replace_text("Existing fresh input");
    let tabs = s
        .tabs
        .iter()
        .map(|t| (t.profile.clone(), t.session.clone()))
        .collect::<Vec<_>>();
    let reply = validated_reply(&mut s, "work");
    reply.send(Ok(connection("current"))).unwrap();
    s.poll_device_routes(&mut Tx::default());
    let created = s.composer_scope();
    assert_eq!(created.connection, fresh.connection);
    assert_eq!(created.profile, fresh.profile);
    assert_eq!(created.profile_intent, fresh.profile_intent);
    assert!(created.session.is_none());
    assert_ne!(created.draft_origin, fresh.draft_origin);
    assert_eq!(principal(&s), source);
    assert_eq!(
        s.composer.draft(&original).unwrap().text.text,
        "Unsent current input"
    );
    assert_eq!(
        s.composer.draft(&other).unwrap().text.text,
        "Other tab input"
    );
    assert_eq!(
        s.composer.draft(&fresh).unwrap().text.text,
        "Existing fresh input"
    );
    assert_eq!(
        s.tabs
            .iter()
            .map(|t| (t.profile.clone(), t.session.clone()))
            .collect::<Vec<_>>(),
        tabs
    );
    assert!(s.messages.is_empty());
    assert!(!s.device_route_pending());
    assert!(
        requests.try_recv().is_err(),
        "same-source acceptance does not reconnect the gateway"
    );
}

#[test]
fn typing_into_the_same_actual_editor_retires_a_delayed_activation_without_losing_any_input() {
    let root = Workspace::new();
    let (mut s, requests) = root.viewer();
    let scope = s.composer_scope();
    let editor = s.composer.editor_id().clone();
    let other = DraftScope {
        session: Some("stored-other".into()),
        draft_origin: None,
        ..scope.clone()
    };
    s.composer
        .draft_mut(other.clone())
        .text
        .replace_text("Other stored input");
    let reply = validated_reply(&mut s, "archive");
    // This is the same callback returned to the actual Rune composer input,
    // with no session, editor, page, source or read-intent change.
    s.edit_editor_text(&editor, &scope)
        .insert(" + typed while the target was opening");
    assert_eq!(s.composer_scope(), scope);
    assert_eq!(s.composer.editor_id(), &editor);
    let edited = view(&s);
    reply.send(Ok(connection("validated-target"))).unwrap();
    s.poll_device_routes(&mut Tx::default());
    assert_eq!(view(&s), edited);
    assert_eq!(
        s.composer.draft(&other).unwrap().text.text,
        "Other stored input"
    );
    assert!(!s.device_route_pending());
    assert!(
        requests.try_recv().is_err(),
        "typing after the activation intent must prevent a stale Connect command"
    );
}

#[test]
fn private_resolver_receipts_activate_selected_route_through_the_existing_intent_fence() {
    use crate::connections::legacy::{Environment, ResolveRequest, SelectionContext};
    use crate::source::{ProfileIntent, RouteDoor};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tungstenite::Message;

    let root = Workspace::new();
    std::fs::create_dir_all(&root.0).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let server_address = listener.local_addr().unwrap();
    let endpoint = format!("http://{server_address}/");
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let server = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        while !done.load(Ordering::Acquire) {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("private listener: {error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            loop {
                match socket.read() {
                    Ok(Message::Text(text)) => {
                        let request: serde_json::Value = serde_json::from_str(&text).unwrap();
                        let value = match request["method"].as_str().unwrap() {
                            "profiles.list" => {
                                json!({"profiles":[{"name":"work","path":"/private/work"}],"install_id":"private"})
                            }
                            "runtime.describe" => {
                                assert_eq!(request["params"], json!({}));
                                json!({"profile_id":"/private/work","instance_id":"private-instance","authority_epoch":1})
                            }
                            method => panic!("unexpected private method {method}"),
                        };
                        socket
                            .send(Message::Text(
                                json!({"jsonrpc":"2.0","id":request["id"],"result":value})
                                    .to_string()
                                    .into(),
                            ))
                            .unwrap();
                    }
                    Err(tungstenite::Error::Io(error))
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) =>
                    {
                        if done.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        }
    });
    let resolver = crate::connections::private_test_resolver(root.0.join("connections.json"));
    let (mut s, requests) = root.viewer();
    let request = ResolveRequest {
        door: RouteDoor::Legacy,
        desktop_profile: "work".into(),
        context: SelectionContext {
            environment: Environment::captured(
                Some(endpoint.clone()),
                Some("private-token".into()),
            ),
            primary: None,
            primary_profile: None,
            primary_remote_active: false,
            isolated_backend: false,
            local_home: None,
        },
        primary: None,
        expected: None,
        expected_namespace: None,
        intent: 1,
    };
    let ticket = resolver.resolve_legacy(request, vec![]).unwrap();
    let retained_resolver = resolver.clone();
    s.queue_route_stage(
        Route::new(None, "work".into()).unwrap(),
        Reason::NewSession,
        Stage::Legacy {
            ticket,
            resolver,
            committing: false,
            expected_namespace: None,
        },
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.routing.pending.is_some() && Instant::now() < deadline {
        s.poll_device_routes(&mut Tx::default());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(s.routing.pending.is_none(), "route did not finish");
    assert!(s.client_error.is_none(), "{:?}", s.client_error);
    assert_eq!(s.profile_intent, ProfileIntent::NativeSelf);
    assert!(matches!(
        s.connection.as_ref().unwrap().route_owner().unwrap().door(),
        RouteDoor::Legacy
    ));
    let read = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(read.profile_intent, ProfileIntent::NativeSelf);
    assert!(matches!(read.command, Command::ConnectExact(_, ref profile) if profile == "work"));
    let prior = s.composer_scope();
    let mismatched = "a-stale-route-namespace".to_owned();
    let reopen = ResolveRequest {
        door: RouteDoor::Legacy,
        desktop_profile: "work".into(),
        context: SelectionContext {
            environment: Environment::captured(Some(endpoint), Some("private-token".into())),
            primary: None,
            primary_profile: None,
            primary_remote_active: false,
            isolated_backend: false,
            local_home: None,
        },
        primary: None,
        expected: None,
        expected_namespace: Some(mismatched.clone()),
        intent: 2,
    };
    let ticket = retained_resolver.resolve_legacy(reopen, vec![]).unwrap();
    s.queue_route_stage(
        Route::new(None, "work".into()).unwrap(),
        Reason::NewSession,
        Stage::Legacy {
            ticket,
            resolver: retained_resolver.clone(),
            committing: false,
            expected_namespace: Some(mismatched),
        },
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.routing.pending.is_some() && Instant::now() < deadline {
        s.poll_device_routes(&mut Tx::default());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(s.routing.pending.is_none());
    assert_eq!(s.composer_scope(), prior);
    assert!(
        s.client_error
            .as_deref()
            .is_some_and(|error| error.contains("namespace")),
        "{:?}",
        s.client_error
    );
    assert!(requests.try_recv().is_err());

    let namespace = s
        .connection
        .as_ref()
        .unwrap()
        .route_owner()
        .unwrap()
        .namespace()
        .to_owned();
    let (mut cold, cold_requests) = root.viewer();
    let captured = crate::client::windows::WindowRoute {
        draft_origin: None,
        source: Some(crate::client::windows::SourceDescriptor::Legacy {
            namespace: Some(namespace.clone()),
        }),
        profile: "work".into(),
        route_profile: None,
        profile_intent: ProfileIntent::NativeSelf,
        session: Some("same".into()),
        title: Some("Restored".into()),
        canonical_bot: false,
    };
    let reopen = ResolveRequest {
        door: RouteDoor::Legacy,
        desktop_profile: "work".into(),
        context: SelectionContext {
            environment: Environment::captured(
                Some(format!("http://{}/", server_address)),
                Some("private-token".into()),
            ),
            primary: None,
            primary_profile: None,
            primary_remote_active: false,
            isolated_backend: false,
            local_home: None,
        },
        primary: None,
        expected: None,
        expected_namespace: Some(namespace.clone()),
        intent: 1,
    };
    let ticket = retained_resolver.resolve_legacy(reopen, vec![]).unwrap();
    cold.queue_route_stage(
        Route::new(None, "work".into()).unwrap(),
        Reason::Initial,
        Stage::Legacy {
            ticket,
            resolver: retained_resolver.clone(),
            committing: false,
            expected_namespace: Some(namespace.clone()),
        },
        None,
    );
    cold.routing.pending.as_mut().unwrap().restore = Some(captured);
    let deadline = Instant::now() + Duration::from_secs(5);
    while cold.routing.pending.is_some() && Instant::now() < deadline {
        cold.poll_device_routes(&mut Tx::default());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(cold.routing.pending.is_none());
    assert!(cold.client_error.is_none(), "{:?}", cold.client_error);
    let connect = cold_requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(connect.profile_intent, ProfileIntent::NativeSelf);
    cold.apply(Reply {
        intent: cold.intent,
        result: Ok(Update::Connected {
            profiles: vec![],
            profile: "work".into(),
            sessions: vec![],
        }),
    });
    cold.poll_windows(&mut Tx::default());
    let history = cold_requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(history.profile_intent, ProfileIntent::NativeSelf);
    assert!(
        matches!(history.command, Command::History(ref profile, ref id) if profile == "work" && id == "same")
    );

    let (mut raw, raw_requests) = root.viewer();
    let raw_route = crate::client::windows::WindowRoute {
        draft_origin: None,
        source: Some(crate::client::windows::SourceDescriptor::Legacy {
            namespace: Some(namespace.clone()),
        }),
        profile: "default".into(),
        route_profile: Some("work".into()),
        profile_intent: ProfileIntent::Backend,
        session: Some("same".into()),
        title: Some("Raw restored".into()),
        canonical_bot: false,
    };
    let mut old_raw = raw_route.clone();
    old_raw.route_profile = None;
    assert!(
        raw.queue_window_restoration(old_raw)
            .unwrap_err()
            .contains("no Desktop routing profile")
    );
    assert!(!raw.device_route_pending());
    let reopen = ResolveRequest {
        door: RouteDoor::Legacy,
        desktop_profile: "work".into(),
        context: SelectionContext {
            environment: Environment::captured(
                Some(format!("http://{}/", server_address)),
                Some("private-token".into()),
            ),
            primary: None,
            primary_profile: None,
            primary_remote_active: false,
            isolated_backend: false,
            local_home: None,
        },
        primary: None,
        expected: None,
        expected_namespace: Some(namespace.clone()),
        intent: 1,
    };
    let ticket = retained_resolver.resolve_legacy(reopen, vec![]).unwrap();
    raw.queue_route_stage(
        Route::new(None, "work".into()).unwrap(),
        Reason::Initial,
        Stage::Legacy {
            ticket,
            resolver: retained_resolver,
            committing: false,
            expected_namespace: Some(namespace),
        },
        None,
    );
    raw.routing.pending.as_mut().unwrap().restore = Some(raw_route.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while raw.routing.pending.is_some() && Instant::now() < deadline {
        raw.poll_device_routes(&mut Tx::default());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(raw.routing.pending.is_none());
    assert!(raw.client_error.is_none(), "{:?}", raw.client_error);
    let connect = raw_requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(connect.profile_intent, ProfileIntent::Backend);
    assert!(
        matches!(connect.command, Command::ConnectExact(_, ref profile) if profile == "default")
    );
    raw.apply(Reply {
        intent: raw.intent,
        result: Ok(Update::Connected {
            profiles: vec![Profile {
                name: "default".into(),
                label: "Raw".into(),
                description: String::new(),
                canonical_session: None,
            }],
            profile: "default".into(),
            sessions: vec![],
        }),
    });
    raw.poll_windows(&mut Tx::default());
    let history = raw_requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(history.profile_intent, ProfileIntent::Backend);
    assert!(
        matches!(history.command, Command::History(ref profile, ref id) if profile == "default" && id == "same")
    );
    stop.store(true, Ordering::Release);
    drop(s);
    server.join().unwrap();
}

#[test]
fn delayed_project_target_does_not_cross_source_on_activation() {
    let root = Workspace::new();
    let (mut viewer, _requests) = root.viewer();
    let original = viewer.connection.as_ref().unwrap();
    let target = crate::projects::DraftTarget {
        owner: crate::projects::Owner {
            session: crate::source::SessionOwner::with_intent(
                crate::source::SourceStamp::from_connection(original).unwrap(),
                "work",
                crate::source::ProfileIntent::Backend,
            ),
            epoch: viewer.connection_epoch,
        },
        cwd: crate::projects::CwdChoice::Path("/source-a-project".into()),
        explicit: true,
    };
    let reply = validated_reply(&mut viewer, "archive");
    viewer.set_device_project_target(Some(target));
    reply.send(Ok(connection("source-b"))).unwrap();
    viewer.poll_device_routes(&mut Tx::default());
    let scope = viewer.composer_scope();
    assert_ne!(
        viewer
            .composer
            .draft(&scope)
            .and_then(|draft| draft.cwd.as_deref()),
        Some("/source-a-project")
    );
    assert!(
        viewer
            .compose_error
            .as_deref()
            .is_some_and(|error| error.contains("different or retired source"))
    );
}
