use super::*;
fn session(id: &str, profile: &str) -> SidebarSession {
    SidebarSession {
        id: id.into(),
        profile: Some(profile.into()),
        message_count: 1,
        ..Default::default()
    }
}
#[test]
fn independent_slices_keep_pins_outside_the_recent_limit_and_preserve_failed_profile_rows() {
    let mut state = SidebarState::new();
    state.owner_profile = "research".into();
    state.apply("sidebar.batch",Ok(json!({"recents":{"sessions":[{"id":"recent","profile":"research","message_count":2},{"id":"old-pin","profile":"research","pinned":true,"message_count":10}],"profiles_truncated":{"research":true}},"messaging":{"sessions":[{"id":"tg","profile":"research","source":"telegram"}]},"cron":{"sessions":[]}})));
    assert_eq!(state.recents.rows.len(), 2);
    assert_eq!(state.recents.rows[1].pinned, Some(true));
    assert_eq!(state.messaging.rows.len(), 1);
    state.apply("sidebar.batch",Ok(json!({"recents":{"sessions":[],"profiles_failed":{"research":{"failed":true}}},"messaging":{"sessions":[],"failed":true},"cron":{"sessions":[]}})));
    assert_eq!(state.recents.rows.len(), 2);
    assert_eq!(state.messaging.rows.len(), 1);
    assert!(state.recents.error.is_some());
    assert!(state.messaging.error.is_some());
}
#[test]
fn fallback_only_follows_a_missing_constant_batch_endpoint_and_retains_auth_scope() {
    let mut state = SidebarState::new();
    state.owner_profile = "research".into();
    state.prefs.all_profiles = true;
    assert!(
        state
            .apply(
                "sidebar.batch",
                Err("Workspace request refused (HTTP 503)".into())
            )
            .is_empty()
    );
    let effects = state.apply(
        "sidebar.batch",
        Err("Workspace request refused (HTTP 404)".into()),
    );
    assert_eq!(effects.len(), 3);
    for (_, request) in effects {
        assert_eq!(request.path, "/api/profiles/sessions");
        assert_eq!(request.profile.as_deref(), Some("research"));
        assert!(request.query.contains(&("profile".into(), "all".into())));
    }
}
#[test]
fn status_uses_confirmed_runtime_or_scheduler_ownership_instead_of_recency() {
    let mut state = SidebarState::new();
    let mut row = session("tip", "research");
    row.last_active = Local::now().timestamp() as f64;
    assert!(state.status(&row) == Status::Idle);
    row.unread = Some(true);
    assert!(state.status(&row) == Status::Unread);
    row._lineage_ids = Some(vec!["runtime-alias".into()]);
    state.live.insert(
        ("research".into(), "runtime-alias".into()),
        Status::NeedsInput,
    );
    assert!(state.status(&row) == Status::NeedsInput);
    state.live.clear();
    row.scheduler_owned = Some(true);
    assert!(state.status(&row) == Status::Working);
    state
        .live
        .insert(("other".into(), "tip".into()), Status::NeedsInput);
    assert!(state.status(&row) == Status::Working);
}
#[test]
fn filters_and_sorting_use_owner_tokens_and_spend() {
    let mut state = SidebarState::new();
    let mut row = session("one", "research");
    row.title = Some("Find papers".into());
    row.input_tokens = 300;
    row.actual_cost_usd = Some(0.5);
    let other = session("two", "default");
    state.prefs.profiles.insert("research".into());
    assert!(state.matches(&row, "papers"));
    assert!(!state.matches(&other, ""));
    state.prefs.ordering = Ordering::Tokens;
    assert_eq!(state.ordered(vec![&other, &row])[0].id, "one");
    state.prefs.ordering = Ordering::Cost;
    assert_eq!(state.ordered(vec![&other, &row])[0].id, "one");
}
#[test]
fn four_am_rollover_and_month_labels_use_the_local_calendar() {
    let at = |day, hour| {
        Local
            .with_ymd_and_hms(2026, 10, day, hour, 0, 0)
            .single()
            .unwrap()
            .timestamp() as f64
    };
    assert_eq!(calendar_bucket(at(29, 1), at(29, 10)).0, "yesterday");
    assert_eq!(calendar_bucket(at(29, 5), at(29, 10)).0, "today");
    assert_eq!(relative_age(at(29, 9), at(29, 10)), "1h");
    assert_eq!(relative_age(at(29, 11), at(29, 10)), "now");
}
#[test]
fn date_groups_keep_a_recent_burst_together_and_branches_with_the_parent() {
    let now = Local
        .with_ymd_and_hms(2026, 10, 29, 12, 0, 0)
        .single()
        .unwrap()
        .timestamp() as f64;
    let mut parent = session("parent", "research");
    parent.last_active = now - 86400.0 * 20.0;
    let mut child = session("child", "research");
    child.last_active = now;
    child._branched_from = Some("parent".into());
    let mut recent = session("recent", "research");
    recent.last_active = now - 5.0;
    let rows = date_rows(vec![&recent, &child, &parent], now);
    let ids: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            ListRow::Session(r) => Some(r.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["recent", "parent", "child"]);
    let branch = rows
        .iter()
        .position(|r| matches!(r,ListRow::Session(row)if row.id=="child"))
        .unwrap();
    assert!(matches!(&rows[branch-1],ListRow::Session(row)if row.id=="parent"));
}
#[test]
fn invalid_snapshot_never_sends_a_write_and_ambiguous_retries_keep_the_identity() {
    let mut state = SidebarState::new();
    let mutation = Mutation {
        session: session("one", "research"),
        payload: json!({"pinned":true}),
        delete: false,
        identity: None,
        owner_connection: 0,
    };
    state.mutation = Some(mutation.clone());
    assert!(
        state
            .accept_snapshot(json!({"exists":true,"runtime_revision":2,"runtime_generation":null}))
            .is_empty()
    );
    assert!(state.mutation.is_none());
    // Synthetic mode prevents all disk/network mutations; the request projection remains testable.
    state.demo = true;
    state.mutation = Some(mutation);
    let effects =
        state.accept_snapshot(json!({"exists":true,"runtime_revision":2,"runtime_generation":3}));
    assert_eq!(effects.len(), 1);
    let identity = effects[0].1.body["request_id"].clone();
    assert_eq!(effects[0].1.profile.as_deref(), Some("research"));
    assert_eq!(effects[0].1.body["profile"], "research");
    state.apply(
        "sidebar.mutation",
        Err("Workspace request failed (network, TLS or timeout)".into()),
    );
    assert!(state.blocks_owner_change());
    let retry = state.action(Action::RetryMutation);
    assert_eq!(retry[0].1.body["request_id"], identity);
    assert_eq!(retry[0].1.body["expected_revision"], 2);
    assert_eq!(retry[0].1.body["expected_generation"], 3);
}
#[test]
fn resetting_visible_data_keeps_a_pending_mutation_receipt() {
    let mut state = SidebarState::new();
    state.mutation = Some(Mutation {
        session: session("one", "research"),
        payload: json!({"unread":false}),
        delete: false,
        identity: Some(json!({"request_id":"test"})),
        owner_connection: 3,
    });
    state.reset_data();
    assert!(state.blocks_owner_change());
    assert_eq!(state.mutation.as_ref().unwrap().owner_connection, 3);
}

#[test]
fn scheduler_run_verdict_is_conservative_and_prefers_actual_ownership_to_activity() {
    assert!(cron_read_only(
        None,
        Some(false),
        Some(true),
        Some(999.0),
        1000.0
    ));
    assert!(!cron_read_only(
        None,
        Some(true),
        Some(false),
        Some(1.0),
        1000.0
    ));
    assert!(!cron_read_only(
        Some(100.0),
        Some(false),
        None,
        None,
        1000.0
    ));
    assert!(cron_read_only(None, None, None, None, 1000.0));
    assert!(!cron_read_only(None, None, None, Some(900.0), 1000.0));
}
