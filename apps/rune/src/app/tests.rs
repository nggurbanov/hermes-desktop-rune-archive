use super::*;

#[test]
fn stale_history_cannot_replace_a_different_profile_or_session() {
    let mut s = Viewer::empty();
    s.intent = 4;
    s.profile = "work".into();
    s.selected_session = Some("new".into());
    s.apply(Reply {
        intent: 3,
        result: Ok(Update::History {
            profile: "default".into(),
            session: "old".into(),
            history: History {
                messages: vec![HistoryMessage {
                    role: "assistant".into(),
                    content: "stale".into(),
                }],
                has_more: false,
            },
        }),
    });
    assert!(s.messages.is_empty());
    s.apply(Reply {
        intent: 4,
        result: Ok(Update::History {
            profile: "default".into(),
            session: "new".into(),
            history: History {
                messages: vec![HistoryMessage {
                    role: "assistant".into(),
                    content: "wrong owner".into(),
                }],
                has_more: false,
            },
        }),
    });
    assert!(s.messages.is_empty());
}

#[test]
fn profile_change_clears_previous_history_before_any_network_reply() {
    let mut s = Viewer::empty();
    reader_scene(&mut s);
    assert!(!s.messages.is_empty());
    s.select_profile("research".into());
    assert!(s.messages.is_empty());
    assert!(s.selected_session.is_none());
    assert!(s.sessions.is_empty());
}

#[test]
fn connection_form_never_accepts_credentials_inside_the_visible_url() {
    let mut s = Viewer::empty();
    s.connection_url
        .replace_text("https://user:private@example.com/?token=secret");
    s.apply_connection();
    assert!(s.connection.is_none());
    assert!(s.connection_form);
    assert!(!s.error.as_ref().unwrap().contains("private"));
    assert!(!s.error.as_ref().unwrap().contains("secret"));
}

#[test]
fn tokenized_cli_urls_are_split_before_filling_the_visible_url_field() {
    let (connection, visible) = url_connection(
        "https://host.example/hermes/api/ws?token=private%20key",
        None,
    )
    .unwrap();
    assert_eq!(visible, "https://host.example/hermes/api/ws");
    assert!(!visible.contains("private"));
    match connection {
        Connection::Url { endpoint, token } => {
            assert_eq!(endpoint, visible);
            assert_eq!(token.as_deref(), Some("private key"));
        }
        _ => panic!("expected remote connection"),
    }
    assert!(url_connection("https://user:private@host.example/", None).is_err());
}

#[test]
fn queued_reads_coalesce_without_losing_the_last_connection_barrier() {
    let local = || Connection::Local {
        home: "/fixture".into(),
    };
    let queue = coalesce(
        Request {
            intent: 1,
            command: Command::History("old".into(), "a".into()),
        },
        [
            Request {
                intent: 2,
                command: Command::Connect(local(), "default".into()),
            },
            Request {
                intent: 3,
                command: Command::Sessions("old".into()),
            },
            Request {
                intent: 4,
                command: Command::Connect(local(), "work".into()),
            },
            Request {
                intent: 5,
                command: Command::History("work".into(), "b".into()),
            },
            Request {
                intent: 6,
                command: Command::History("work".into(), "c".into()),
            },
        ],
    );
    assert_eq!(
        queue.iter().map(|r| r.intent).collect::<Vec<_>>(),
        vec![4, 6]
    );
    assert!(matches!(&queue[0].command, Command::Connect(_, p) if p == "work"));
    assert!(matches!(&queue[1].command, Command::History(p, id) if p == "work" && id == "c"));
}

#[test]
fn reconnect_profile_fallback_does_not_adopt_an_old_session_id() {
    let mut s = Viewer::empty();
    s.intent = 1;
    s.profile = "removed-profile".into();
    s.selected_session = Some("shared".into());
    s.messages.push(HistoryMessage {
        role: "assistant".into(),
        content: "old owner".into(),
    });
    s.apply(Reply {
        intent: 1,
        result: Ok(Update::Connected {
            profiles: vec![Profile {
                name: "default".into(),
                label: "Default".into(),
            }],
            profile: "default".into(),
            sessions: vec![Session {
                id: "shared".into(),
                title: "Other owner".into(),
                updated: String::new(),
            }],
        }),
    });
    assert!(s.selected_session.is_none());
    assert!(s.messages.is_empty());
}
