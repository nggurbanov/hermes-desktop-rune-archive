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
                profile: "work".into(),
                session: "stored-current".into(),
                title: "Current".into(),
                bot: false,
            },
            ConversationTab {
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
        owner: s.route_owner(),
        reason: Reason::NewSession,
        stage: Stage::Direct(receive),
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
    assert_eq!(s.composer_scope(), fresh);
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
