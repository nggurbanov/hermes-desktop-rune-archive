use super::*;
fn platforms() -> Messaging {
    let mut m = Messaging::new();
    m.profile = "work".into();
    m.fixture();
    m.demo = false;
    m
}
#[test]
fn sparse_credentials_compare_baseline_and_focusing_does_not_dirty() {
    let mut m = platforms();
    m.edits
        .insert("telegram:TELEGRAM_BOT_TOKEN".into(), TextEdit::new(""));
    m.edits.insert(
        "telegram:TELEGRAM_ALLOWED_USERS".into(),
        TextEdit::new("123456"),
    );
    assert!(!m.credentials_dirty());
    assert!(m.act(Action::Save, "work").is_empty());
    m.edits
        .get_mut("telegram:TELEGRAM_ALLOWED_USERS")
        .unwrap()
        .replace_text("");
    let e = m.act(Action::Save, "work");
    assert_eq!(e[0].1.body["env"], json!({}));
    assert_eq!(e[0].1.body["clear_env"], json!(["TELEGRAM_ALLOWED_USERS"]));
    assert_eq!(e[0].1.profile.as_deref(), Some("work"));
    m.apply("messaging.pairing", Err("read failed".into()));
    assert!(m.busy);
}
#[test]
fn test_result_false_is_diagnostic_and_preserves_all_drafts() {
    let mut m = platforms();
    m.edits.insert(
        "telegram:TELEGRAM_BOT_TOKEN".into(),
        TextEdit::new("new-secret"),
    );
    m.act(Action::Test, "work");
    m.act(Action::Select("discord".into()), "work");
    m.apply(
        "messaging.mutation",
        Ok(json!({"ok":false,"message":"Token revoked","state":"invalid"})),
    );
    assert_eq!(m.test_result.as_ref().unwrap()["platform"], "telegram");
    assert_eq!(m.test_result.as_ref().unwrap()["ok"], false);
    assert_eq!(m.edits["telegram:TELEGRAM_BOT_TOKEN"].text, "new-secret");
    assert!(!m.restart_needed);
}
#[test]
fn credential_ack_only_removes_exact_sent_snapshot_and_clear_is_explicit() {
    let mut m = platforms();
    m.edits
        .insert("telegram:TELEGRAM_BOT_TOKEN".into(), TextEdit::new("sent"));
    m.act(Action::Save, "work");
    m.edits
        .get_mut("telegram:TELEGRAM_BOT_TOKEN")
        .unwrap()
        .replace_text("newer");
    m.apply(
        "messaging.mutation",
        Ok(json!({"ok":true,"hot_served":true})),
    );
    assert_eq!(m.edits["telegram:TELEGRAM_BOT_TOKEN"].text, "newer");
    assert!(!m.restart_needed);
    m.act(Action::Clear("TELEGRAM_BOT_TOKEN".into()), "work");
    let e = m.act(Action::Save, "work");
    assert_eq!(e[0].1.body["clear_env"], json!(["TELEGRAM_BOT_TOKEN"]));
}
#[test]
fn telegram_auto_poll_is_singleflight_transient_retry_and_validated_owner() {
    let mut m = Messaging::new();
    m.profile = "bot".into();
    let e = m.act(Action::StartTelegram, "bot");
    assert_eq!(e[0].1.body, json!({}));
    m.apply("messaging.telegram.start",Ok(json!({"pairing_id":"abc","deep_link":"https://t.me/setup","qr_payload":"https://t.me/setup"})));
    assert!(m.poll(1.1).is_empty());
    let e = m.poll(1.3);
    assert_eq!(e[0].1.profile.as_deref(), Some("bot"));
    assert!(m.poll(2.0).is_empty());
    m.apply(
        "messaging.telegram.status.abc",
        Err("temporary network failure".into()),
    );
    assert_eq!(m.onboarding["deep_link"], "https://t.me/setup");
    assert!(m.poll(3.9).is_empty());
    assert_eq!(m.poll(4.1).len(), 1);
    m.apply(
        "messaging.telegram.status.abc",
        Ok(json!({"status":"ready","owner_user_id":"123456"})),
    );
    assert_eq!(m.allowed.text, "123456");
    m.allowed.replace_text("bad");
    assert!(m.act(Action::ApplyTelegram, "bot").is_empty());
    m.allowed.replace_text("123456,123456 987654");
    let e = m.act(Action::ApplyTelegram, "bot");
    assert_eq!(e[0].1.body["allowed_user_ids"], json!(["123456", "987654"]));
    m.apply("messaging.mutation", Err("unconfirmed".into()));
    assert!(m.pairing_id.is_some());
    assert_eq!(m.allowed.text, "123456,123456 987654");
}
#[test]
fn stale_telegram_status_cannot_overwrite_new_flow_and_cancel_failure_closes_locally() {
    let mut m = Messaging::new();
    m.apply("messaging.telegram.start", Ok(json!({"pairing_id":"new"})));
    m.apply(
        "messaging.telegram.status.old",
        Ok(json!({"status":"ready","owner_user_id":"111"})),
    );
    assert!(m.allowed.text.is_empty());
    m.act(Action::CancelTelegram, "default");
    m.apply("messaging.mutation", Err("offline".into()));
    assert!(m.pairing_id.is_none());
    assert!(m.telegram_error.as_ref().unwrap().contains("locally"));
}
#[test]
fn telegram_apply_watches_existing_restart_and_never_posts_second_restart() {
    let mut m = Messaging::new();
    m.profile = "bot".into();
    m.apply("messaging.telegram.start", Ok(json!({"pairing_id":"abc"})));
    m.apply(
        "messaging.telegram.status.abc",
        Ok(json!({"status":"ready","owner_user_id":"123"})),
    );
    m.act(Action::ApplyTelegram, "bot");
    let e=m.apply("messaging.mutation",Ok(json!({"ok":true,"platform":"telegram","needs_restart":true,"restart_started":true,"restart_action":"gateway-restart","restart_pid":9})));
    assert!(
        e.iter()
            .all(|(_, r)| !matches!(r.method, super::super::PageMethod::Post))
    );
    assert!(m.restart_needed);
    assert!(m.pairing_id.is_none());
    let e = m.apply(
        "messaging.restart.status",
        Ok(json!({"name":"gateway-restart","running":false,"exit_code":1,"lines":["failed"]})),
    );
    assert!(e.is_empty());
    assert!(m.restart_needed);
    assert!(m.take_reconnect_followthrough());
    assert!(m.restart.as_ref().unwrap().error.is_some());
}
#[test]
fn expired_flow_and_cancel_during_start_are_recoverable() {
    let mut m = Messaging::new();
    m.apply(
        "messaging.telegram.start",
        Ok(json!({"pairing_id":"x","expires_at":"2000-01-01T00:00:00Z"})),
    );
    assert!(m.poll(1.0).is_empty());
    assert!(m.pairing_id.is_none());
    m.act(Action::StartTelegram, "default");
    m.act(Action::CancelTelegram, "default");
    let e = m.apply(
        "messaging.telegram.start",
        Ok(json!({"pairing_id":"cancel"})),
    );
    assert_eq!(e[0].1.path, "/api/messaging/telegram/onboarding/cancel");
    assert!(matches!(e[0].1.method, super::super::PageMethod::Delete));
}
#[test]
fn webhook_request_and_pairing_keep_profile_and_required_contract() {
    let mut m = Messaging::new();
    m.profile = "work".into();
    let e = m.act(
        Action::Approve("telegram".into(), "request".into()),
        "wrong",
    );
    assert_eq!(e[0].1.body["profile"], "work");
    assert_eq!(e[0].1.body["request_id"], "request");
    m.apply("messaging.mutation", Ok(json!({"ok":true})));
    m.webhook_name.replace_text("repo");
    m.webhook_skills.replace_text("review, report");
    m.webhook_chat.replace_text("channel");
    m.webhook_deliver_only = true;
    let e = m.act(Action::CreateWebhook, "work");
    assert_eq!(e[0].1.body["skills"], json!(["review", "report"]));
    assert_eq!(e[0].1.body["deliver_only"], true);
    m.apply(
        "messaging.mutation",
        Ok(json!({"ok":true,"secret":"one-time"})),
    );
    assert!(!m.webhook_dirty());
    assert_eq!(m.new_secret.as_deref(), Some("one-time"));
}

#[test]
fn platform_filter_never_promotes_configuration_to_runtime_readiness() {
    for state in [
        "pending_restart",
        "gateway_stopped",
        "retrying",
        "connecting",
        "needs_setup",
        "unknown",
    ] {
        assert_eq!(
            platform_group(&json!({"enabled":true,"configured":true,"state":state})),
            "setup"
        );
    }
    assert_eq!(
        platform_group(&json!({"enabled":true,"configured":false,"state":"connected"})),
        "ready"
    );
    for state in ["fatal", "startup_failed"] {
        assert_eq!(
            platform_group(&json!({"enabled":true,"state":state})),
            "issues"
        );
    }
    assert_eq!(
        platform_group(&json!({"enabled":false,"state":"connected"})),
        "disabled"
    );
    assert_eq!(
        platform_group(&json!({"enabled":true,"state":"disabled"})),
        "disabled"
    );
}
