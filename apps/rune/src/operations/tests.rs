use super::*;
#[test]
fn read_only_health_requires_inference_truth_and_never_autostarts_service() {
    let mut o = Operations::new();
    o.observe(false, true);
    let e = o.poll(1.0);
    assert!(
        e.iter()
            .all(|(_, r)| !matches!(r.method, crate::pages::PageMethod::Post))
    );
    o.apply("ops.status", Ok(json!({"gateway_running":true})));
    assert_eq!(o.ready(), None);
    o.apply("ops.setup", Ok(json!({"provider_configured":true})));
    o.apply("ops.runtime", Ok(json!({"ok":false,"error":"No model"})));
    assert_eq!(o.ready(), Some(false));
    assert_eq!(o.readiness_reason(), "No model");
}
#[test]
fn maintenance_starts_once_requires_receipt_and_never_confuses_read_with_completion() {
    let mut o = Operations::new();
    let e = o.act(Action::Start(Kind::Doctor));
    assert_eq!(e[0].1.path, "/api/ops/doctor");
    assert!(o.act(Action::Start(Kind::Doctor)).is_empty());
    let e = o.apply(
        "ops.action-start",
        Ok(json!({"ok":true,"name":"doctor","pid":8})),
    );
    assert!(e[0].1.path.ends_with("doctor/status"));
    o.apply(
        "ops.action-status",
        Ok(json!({"name":"doctor","pid":8,"running":false,"exit_code":null,"lines":[]})),
    );
    assert!(!o.task.as_ref().unwrap().settled);
    o.act(Action::PollAction);
    o.apply(
        "ops.action-status",
        Ok(json!({"name":"doctor","pid":8,"running":false,"exit_code":0,"lines":["ok"]})),
    );
    assert!(o.task.as_ref().unwrap().settled);
}
#[test]
fn shared_restart_is_confirmed_and_refused_recovery_window_never_claims_restart() {
    let mut o = Operations::new();
    o.status = Some(json!({"gateway_running":true,"gateway_shared_with":["default","research"]}));
    assert!(o.act(Action::Start(Kind::Restart)).is_empty());
    assert!(o.confirm.is_some());
    o.act(Action::Confirm);
    o.apply(
        "ops.action-start",
        Ok(json!({"ok":true,"name":"gateway-restart","pid":8})),
    );
    for n in 0..18 {
        o.apply("ops.action-status", Err("Connection refused".into()));
        if n < 17 {
            assert!(o.task.as_ref().unwrap().running);
        }
    }
    assert!(!o.task.as_ref().unwrap().settled);
    assert!(
        o.task
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("No replacement")
    );
    assert!(o.take_reconnect_followthrough());
}
#[test]
fn update_receipt_identity_and_partial_outcome_are_authoritative() {
    let mut o = Operations::new();
    o.act(Action::Start(Kind::Update));
    o.apply(
        "ops.action-start",
        Ok(json!({"ok":true,"name":"hermes-update","pid":8,"action_id":"owned"})),
    );
    o.apply("ops.action-status",Ok(json!({"name":"hermes-update","running":false,"exit_code":0,"action_id":"other","receipt":{"outcome":"success"}})));
    assert!(!o.task.as_ref().unwrap().settled);
    assert!(o.task.as_ref().unwrap().error.is_some());
}
#[test]
fn log_filters_and_usage_periods_ignore_obsolete_results() {
    let mut o = Operations::new();
    let request = o.act(Action::LogFile("gateway".into()));
    assert!(!request[0].1.query.iter().any(|(k, _)| k == "level"));
    o.apply("ops.logs.agent.ALL", Ok(json!({"lines":["old"]})));
    assert!(o.logs.is_empty());
    o.apply(
        "ops.logs.gateway.ALL",
        Ok(json!({"lines":["INFO ready","ERROR failed"]})),
    );
    o.log_search.replace_text("error");
    assert_eq!(o.visible_logs(), vec!["ERROR failed"]);
    o.act(Action::UsagePeriod(7));
    o.apply("ops.usage.30", Ok(json!({"totals":{},"daily":[]})));
    assert!(o.usage.is_none());
}
#[test]
fn memory_reset_and_curator_pause_require_exact_acknowledgement() {
    let mut o = Operations::new();
    assert!(o.act(Action::ResetMemory("memory".into())).is_empty());
    let e = o.act(Action::Confirm);
    assert_eq!(e[0].1.body, json!({"target":"memory"}));
    o.apply(
        "ops.memory-reset",
        Ok(json!({"ok":false,"deleted":["MEMORY.md"]})),
    );
    assert!(o.maintenance_error.is_some());
    o.curator = Some(json!({"enabled":true,"paused":false}));
    o.act(Action::ToggleCurator);
    o.apply("ops.curator-pause", Ok(json!({"ok":true,"paused":false})));
    assert!(o.maintenance_error.as_ref().unwrap().contains("pause"));
}
#[test]
fn transcript_export_pages_include_compacted_history_and_reject_repeated_pages() {
    let mut o = Operations::new();
    let row = SidebarSession {
        id: "session".into(),
        profile: Some("owner".into()),
        ..Default::default()
    };
    let e = o.act(Action::Export(
        row,
        PathBuf::from("/not-written/export.json"),
    ));
    assert_eq!(e[0].1.profile.as_deref(), Some("owner"));
    assert!(
        e[0].1
            .query
            .iter()
            .any(|(k, v)| k == "include_compacted" && v == "true")
    );
    let page = json!({"session_id":"session","profile":"owner","messages":[{"id":1,"content":"synthetic"}],"pagination":{"limit":1}});
    let e = o.apply("ops.export-page", Ok(page.clone()));
    assert!(e[0].1.query.iter().any(|(k, v)| k == "offset" && v == "1"));
    assert!(o.apply("ops.export-page", Ok(page)).is_empty());
    assert!(o.export_error.as_ref().unwrap().contains("repeated"));
    assert!(!o.exporting);
}

#[test]
fn lost_start_cannot_adopt_a_previous_completed_action_as_success() {
    let mut o = Operations::new();
    o.act(Action::Start(Kind::Backup));
    o.apply("ops.action-start", Err("Disconnected".into()));
    o.act(Action::PollAction);
    o.apply(
        "ops.action-status",
        Ok(json!({"name":"backup","running":false,"exit_code":0,"lines":["old backup complete"]})),
    );
    assert!(!o.task.as_ref().unwrap().settled);
    assert!(
        o.task
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("cannot confirm")
    );
}

#[test]
fn hidden_center_stops_status_and_log_tails_and_authoritative_readiness_is_not_reprobed_each_tick()
{
    let mut o = Operations::new();
    o.observe(false, false);
    assert!(o.poll(50.0).is_empty());
    o.observe(false, true);
    o.poll(51.0);
    o.apply("ops.status", Ok(json!({"gateway_running":true})));
    o.apply("ops.runtime", Ok(json!({"ok":true})));
    o.apply("ops.setup", Ok(json!({"provider_configured":true})));
    o.apply("ops.health-logs", Ok(json!({"lines":[]})));
    let e = o.poll(60.0);
    assert!(
        e.iter()
            .all(|(_, r)| r.path != "setup.runtime_check" && r.path != "setup.status")
    );
    o.observe(false, false);
    o.apply("ops.status", Ok(json!({"gateway_running":true})));
    o.apply("ops.health-logs", Ok(json!({"lines":[]})));
    assert!(o.poll(100.0).is_empty());
}
