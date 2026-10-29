use super::*;
use crate::composer::DraftScope;
use serde_json::json;
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
fn scope(id: &str) -> DraftScope {
    DraftScope {
        connection: 9,
        profile: "alpha".into(),
        session: Some(id.into()),
    }
}
fn sample(kind: Kind, id: &str) -> Notice {
    Notice {
        kind,
        scope: scope(id),
        runtime: format!("runtime-{id}"),
        title: "Test".into(),
        body: "Private fixture".into(),
        global: false,
        tag: None,
        silent: true,
        buttons: vec![Button {
            label: "Approve".into(),
            action: PromptAction {
                prompt_id: "prompt-one".into(),
                execution_generation: 4,
                answer: "once".into(),
            },
        }],
    }
}
pub(super) fn request(client: u64, kind: Kind, id: &str) -> Request {
    Request {
        id: 1,
        client,
        principal: "private-fixture-principal".into(),
        notice: sample(kind, id),
        test: false,
    }
}
fn client(primary: bool) -> (service::Client, mpsc::Receiver<Reply>, Arc<()>) {
    let (replies, receiver) = mpsc::channel();
    let alive = Arc::new(());
    (
        service::Client {
            replies,
            waker: None,
            primary,
            alive: Arc::downgrade(&alive),
        },
        receiver,
        alive,
    )
}
#[test]
fn policy_keeps_attention_and_completion_rules_separate() {
    let now = Instant::now();
    let p = Preferences::default();
    let mut policy = policy::Policy::default();
    policy.active = Some(scope("one"));
    for kind in Kind::ALL {
        let active = sample(kind, "one");
        let other = sample(kind, "two");
        assert!(!policy.allows(&p, &active, now));
        assert_eq!(
            policy.allows(&p, &other, now),
            matches!(kind, Kind::Approval | Kind::Input)
        );
        policy.focused = false;
        assert!(policy.allows(&p, &active, now));
        assert_eq!(
            policy.allows(&p, &other, now),
            matches!(kind, Kind::Approval | Kind::Input)
        );
        let mut global = other.clone();
        global.global = true;
        assert!(policy.allows(&p, &global, now));
        policy.focused = true;
        assert!(!policy.allows(&p, &global, now));
    }
    policy.focused = false;
    let mut other_owner = sample(Kind::TurnDone, "one");
    other_owner.scope.connection += 1;
    assert!(!policy.allows(&p, &other_owner, now));
    other_owner.scope = scope("one");
    other_owner.scope.profile = "beta".into();
    assert!(!policy.allows(&p, &other_owner, now));
    let mut muted = p.clone();
    muted.enabled = false;
    assert!(!policy.allows(&muted, &sample(Kind::Approval, "two"), now));
    muted.enabled = true;
    muted.kinds.insert("approval".into(), false);
    assert!(!policy.allows(&muted, &sample(Kind::Approval, "two"), now));
}
#[test]
fn every_stream_open_resets_only_its_profile_baseline_including_unbound_sessions() {
    let now = Instant::now();
    let mut policy = policy::Policy::default();
    let p = Preferences::default();
    policy.active = Some(scope("one"));
    policy.focused = false;
    let mut unbound = scope("one");
    unbound.session = None;
    policy.opened(&unbound, now);
    assert!(!policy.allows(
        &p,
        &sample(Kind::TurnDone, "one"),
        now + Duration::from_millis(3999)
    ));
    assert!(policy.allows(
        &p,
        &sample(Kind::TurnDone, "one"),
        now + Duration::from_secs(4)
    ));
    policy.opened(&unbound, now + Duration::from_secs(5));
    assert!(!policy.allows(
        &p,
        &sample(Kind::Approval, "two"),
        now + Duration::from_secs(6)
    ));
    let mut other = sample(Kind::Approval, "two");
    other.scope.profile = "beta".into();
    assert!(policy.allows(&p, &other, now + Duration::from_secs(6)));
}
#[test]
fn device_preferences_preserve_unknown_values_and_never_overwrite_invalid_files() {
    let dir = std::env::temp_dir().join(format!(
        "hermes-notifications-prefs-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("notifications.json");
    std::fs::write(
        &path,
        br#"{"version":1,"kinds":{"plugin":false,"future":true},"futureSetting":73}"#,
    )
    .unwrap();
    let mut n = Notifications::new();
    n.load_at(path.clone());
    assert!(n.prefs.kind(Kind::Approval));
    assert!(!n.prefs.kind(Kind::Plugin));
    n.act(Action::Enabled(false), true);
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["futureSetting"], 73);
    assert_eq!(saved["kinds"]["future"], true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    std::fs::write(&path, b"invalid-json").unwrap();
    n.load_at(path.clone());
    n.act(Action::Enabled(true), true);
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid-json");
    assert!(n.error.is_some());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn event_projection_uses_exact_approval_choices_and_only_urgent_credit_notices() {
    let payload = json!({"prompt_id":"approval-one","execution_generation":7,"choices":["once","session","deny"],"command":"make test"});
    let notice = event::notice(
        &scope("one"),
        "approval.request",
        &payload,
        "runtime-one",
        "Fix tests",
    )
    .unwrap();
    assert!(notice.title.contains("Fix tests"));
    assert_eq!(notice.buttons.len(), 2);
    assert_eq!(notice.buttons[0].action.answer, "once");
    assert_eq!(notice.buttons[1].action.answer, "deny");
    assert_eq!(notice.buttons[0].action.execution_generation, 7);
    assert!(
        event::notice(
            &scope("one"),
            "notification.show",
            &json!({"key":"credits.usage","text":"75%"}),
            "runtime-one",
            ""
        )
        .is_none()
    );
    assert!(
        event::notice(
            &scope("one"),
            "notification.show",
            &json!({"key":"credits.window_limit","text":"Access paused"}),
            "runtime-one",
            ""
        )
        .unwrap()
        .global
    );
    let account = event::notice(
        &scope("one"),
        "notification.show",
        &json!({"key":"credits.restored","text":"Access restored"}),
        "runtime-one",
        "",
    )
    .unwrap();
    assert!(account.scope.session.is_none());
    assert!(
        account.runtime.is_empty(),
        "account notice click must not select an arbitrary conversation"
    );
    let peer = event::notice(
        &scope("two"),
        "notification.show",
        &json!({"key":"credits.restored","text":"Access restored"}),
        "runtime-two",
        "",
    )
    .unwrap();
    let left = Request {
        id: 1,
        client: 1,
        principal: "account-fixture".into(),
        notice: account,
        test: false,
    };
    let right = Request {
        id: 2,
        client: 2,
        principal: "account-fixture".into(),
        notice: peer,
        test: false,
    };
    assert_eq!(left.key(), right.key());
    let mut n = Notifications::new();
    n.event(
        &scope("one"),
        "message.complete",
        &json!({"text":"old"}),
        "runtime-one",
        true,
        "",
    );
    assert_eq!(n.sequence, 0);
    assert!(!n.dispatch(sample(Kind::TurnDone, "one")));
}
#[test]
fn daemon_sender_and_origin_client_control_all_actions_and_body_fallbacks() {
    let mut registry = registry::Registry::new();
    let now = Instant::now();
    let (primary, main, _main_life) = client(true);
    let (source, popout, _source_life) = client(false);
    let mut clients = service::Clients::from([(1, primary), (2, source)]);
    registry.insert(
        ":1.10".into(),
        42,
        request(2, Kind::Approval, "one"),
        true,
        &clients,
        now,
    );
    assert!(!registry.receive(
        ":1.11",
        42,
        registry::Signal::Action("0".into()),
        &clients,
        false,
        now
    ));
    assert!(popout.try_recv().is_err());
    assert_eq!(registry.live.len(), 1);
    assert!(!registry.receive(
        ":1.10",
        42,
        registry::Signal::Action("99".into()),
        &clients,
        false,
        now
    ));
    assert!(registry.receive(
        ":1.10",
        42,
        registry::Signal::Action("0".into()),
        &clients,
        false,
        now
    ));
    let Reply::Activated(a) = popout.try_recv().unwrap() else {
        panic!("missing origin action");
    };
    assert_eq!(a.principal, "private-fixture-principal");
    assert_eq!(a.scope, scope("one"));
    assert!(a.action.is_some());
    assert!(main.try_recv().is_err());
    clients.remove(&2);
    registry.insert(
        ":1.10".into(),
        43,
        request(2, Kind::Approval, "one"),
        true,
        &clients,
        now,
    );
    registry.receive(
        ":1.10",
        43,
        registry::Signal::Action("0".into()),
        &clients,
        false,
        now,
    );
    assert!(
        main.try_recv().is_err(),
        "approval action must not migrate to main"
    );
    registry.insert(
        ":1.10".into(),
        44,
        request(2, Kind::TurnDone, "one"),
        true,
        &clients,
        now,
    );
    registry.receive(
        ":1.10",
        44,
        registry::Signal::Token("wayland-token".into()),
        &clients,
        false,
        now,
    );
    registry.receive(
        ":1.10",
        44,
        registry::Signal::Action("default".into()),
        &clients,
        false,
        now,
    );
    let Reply::Activated(a) = main.try_recv().unwrap() else {
        panic!("missing fallback body click");
    };
    assert!(a.action.is_none());
    assert_eq!(a.token.as_deref(), Some("wayland-token"));
}
#[test]
fn same_batch_actions_and_daemon_restart_cannot_reuse_an_old_handler() {
    let mut registry = registry::Registry::new();
    let now = Instant::now();
    let (source, rx, _life) = client(true);
    let clients = service::Clients::from([(1, source)]);
    registry.receive(
        ":1.10",
        5,
        registry::Signal::Action("default".into()),
        &clients,
        true,
        now,
    );
    assert!(registry.insert(
        ":1.10".into(),
        5,
        request(1, Kind::TurnDone, "one"),
        true,
        &clients,
        now
    ));
    assert!(matches!(rx.try_recv(), Ok(Reply::Activated(_))));
    assert!(registry.live.is_empty());
    registry.insert(
        ":1.10".into(),
        6,
        request(1, Kind::TurnDone, "one"),
        true,
        &clients,
        now,
    );
    registry.retire_owner(":1.10");
    registry.insert(
        ":1.11".into(),
        6,
        request(1, Kind::TurnDone, "two"),
        true,
        &clients,
        now,
    );
    registry.receive(
        ":1.10",
        6,
        registry::Signal::Action("default".into()),
        &clients,
        false,
        now,
    );
    assert!(rx.try_recv().is_err());
    assert_eq!(
        registry.expired(now + Duration::from_secs(601)),
        vec![(":1.11".into(), 6)]
    );
    let mut a = request(1, Kind::TurnDone, "one");
    let mut b = a.clone();
    b.notice.scope.session = Some("two".into());
    assert_ne!(a.key(), b.key());
    a.notice.scope.profile = "other".into();
    assert_ne!(a.key(), request(1, Kind::TurnDone, "one").key());
}
