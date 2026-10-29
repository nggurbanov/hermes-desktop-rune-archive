use super::*;

fn state() -> Capabilities {
    let mut state = Capabilities::new();
    state.profile = Some("test-profile".into());
    state
}

fn only(effects: Vec<PageEffect>) -> PageRequest {
    assert_eq!(effects.len(), 1);
    let (operation, request) = effects.into_iter().next().unwrap();
    assert!(operation.starts_with("capabilities."));
    assert_eq!(request.profile.as_deref(), Some("test-profile"));
    request
}

#[test]
fn plugin_toggle_requires_the_canonical_key_and_binds_the_selected_profile() {
    let mut state = state();
    state.plugins = vec![json!({"name":"Notes","key":"community/notes"})];
    assert!(
        state
            .act(Action::TogglePlugin("Notes".into(), true))
            .is_empty()
    );
    let request = only(state.act(Action::TogglePlugin("community/notes".into(), true)));
    assert_eq!(request.path, "plugins.manage");
    assert_eq!(
        request.body,
        json!({"action":"toggle","key":"community/notes","enable":true})
    );
}

#[test]
fn plugin_install_is_bound_to_the_reviewed_source_and_full_sha() {
    let mut state = state();
    state.source.replace_text("https://example.invalid/plugin");
    state.inspected_source = state.source.text.clone();
    state.inspection = json!({"ok":true,"agent":true,"sha":"ab"});
    assert!(state.act(Action::InstallPlugin).is_empty());
    let sha = "a".repeat(40);
    state.inspection["sha"] = sha.clone().into();
    state.source.replace_text("https://example.invalid/another");
    assert!(state.act(Action::InstallPlugin).is_empty());
    state.source.replace_text(state.inspected_source.clone());
    let request = only(state.act(Action::InstallPlugin));
    assert_eq!(request.body["inspected_sha"], sha);
    assert_eq!(request.body["force"], false);
}

#[test]
fn plugin_settings_validate_before_secrets_then_write_only_changed_values_once() {
    let mut state = state();
    state.plugins = vec![
        json!({"key":"notes","settings_schema":[{"key":"token","type":"secret","env":"NOTES_TOKEN"},{"key":"limit","type":"number","value":2},{"key":"unchanged","type":"string","value":"retain"}]}),
    ];
    assert!(
        state
            .act(Action::Select(Selection::Plugin("notes".into())))
            .is_empty()
    );
    state.fields[0].input.replace_text("synthetic-test-token");
    state.fields[1].input.replace_text("invalid");
    assert!(state.act(Action::SavePluginSettings).is_empty());
    assert!(!state.busy());
    state.fields[1].input.replace_text("4");
    let secret = only(state.act(Action::SavePluginSettings));
    assert_eq!(secret.path, "/api/env");
    assert_eq!(secret.body["key"], "NOTES_TOKEN");
    let settings = only(state.apply("capabilities.write", Ok(json!({"ok":true}))));
    assert!(state.fields[0].input.text.is_empty());
    assert_eq!(settings.path, "plugins.manage");
    assert_eq!(settings.body["values"], json!({"limit":4.0}));
    let followup = state.apply("capabilities.write", Ok(json!({"ok":true})));
    assert!(
        followup.iter().all(
            |(_, request)| request.path != "plugins.manage" || request.body["action"] == "list"
        )
    );
    assert!(!state.busy());
}

#[test]
fn failed_secret_write_keeps_the_draft_and_never_sends_settings() {
    let mut state = state();
    state.plugins = vec![
        json!({"key":"notes","settings_schema":[{"key":"token","type":"secret","env":"NOTES_TOKEN"}]}),
    ];
    state.act(Action::Select(Selection::Plugin("notes".into())));
    state.fields[0].input.replace_text("synthetic-test-token");
    only(state.act(Action::SavePluginSettings));
    assert!(
        state
            .apply("capabilities.write", Err("fixture refusal".into()))
            .is_empty()
    );
    assert_eq!(state.fields[0].input.text, "synthetic-test-token");
    assert!(!state.busy());
}

#[test]
fn bulk_skills_changes_the_whole_profile_serially_and_stops_on_error() {
    let mut state = state();
    state.skills = vec![
        json!({"name":"visible","enabled":false}),
        json!({"name":"hidden-by-search","enabled":false}),
        json!({"name":"already-enabled","enabled":true}),
    ];
    state.query.replace_text("visible");
    let first = only(state.act(Action::BulkSkills(true)));
    let second = only(state.apply("capabilities.write", Ok(json!({"ok":true}))));
    let mut changed = vec![
        first.body["name"].as_str().unwrap().to_owned(),
        second.body["name"].as_str().unwrap().to_owned(),
    ];
    changed.sort();
    assert_eq!(changed, ["hidden-by-search", "visible"]);
    assert!(
        state
            .apply("capabilities.write", Err("fixture refusal".into()))
            .is_empty()
    );
    assert!(state.bulk.is_empty());
}

#[test]
fn skill_edit_and_archive_respect_provenance() {
    let mut state = state();
    state.skills = vec![
        json!({"name":"bundled","provenance":"bundled"}),
        json!({"name":"own","provenance":"agent"}),
    ];
    assert!(state.act(Action::EditSkill("bundled".into())).is_empty());
    assert!(state.act(Action::ArchiveSkill("bundled".into())).is_empty());
    let request = only(state.act(Action::EditSkill("own".into())));
    assert_eq!(request.path, "/api/learning/node");
}

#[test]
fn mcp_whitelist_empty_is_preserved_and_denylist_removal_does_not_drop_other_keys() {
    let config = json!({"command":"fixture-mcp","custom":{"retain":true},"tools":{"include":["one"],"exclude":["two"],"vendor":true}});
    let toggled = toggle_tool(&config, "one");
    assert_eq!(toggled["tools"]["include"], json!([]));
    assert!(!tool_enabled(&toggled, "unlisted"));
    assert_eq!(toggled["custom"], config["custom"]);
    assert_eq!(toggled["tools"]["vendor"], true);
    let deny = toggle_tool(
        &json!({"command":"fixture-mcp","tools":{"exclude":["one"],"vendor":true}}),
        "one",
    );
    assert!(deny["tools"].get("exclude").is_none());
    assert_eq!(deny["tools"]["vendor"], true);
}

#[test]
fn mcp_replacement_waits_for_a_confirmed_complete_map_and_retains_unedited_servers() {
    let mut state = state();
    state.mcp_name.replace_text("new-server");
    state.mcp_command.replace_text("fixture-mcp");
    assert!(state.act(Action::SaveMcp).is_empty());
    state.apply(
        "capabilities.mcp-config",
        Ok(json!({"mcp_servers":{"other":{"command":"keep","custom":true}}})),
    );
    let request = only(state.act(Action::SaveMcp));
    assert_eq!(request.path, "/api/mcp/servers");
    assert_eq!(
        request.body["servers"]["other"],
        json!({"command":"keep","custom":true})
    );
    assert_eq!(
        request.body["servers"]["new-server"]["command"],
        "fixture-mcp"
    );
}

#[test]
fn hosted_policy_uses_member_revision_and_keeps_the_member_disabled_tools() {
    let mut state = state();
    state.policy = json!({"effective":{"revision":"not-authoritative","disabled_tools":{"calendar":["org-tool"]}},"layers":[{"kind":"organization","body":{"mode":"deny","tools":{"calendar":["org-tool"]}}},{"kind":"member","revision":"member-revision","body":{"mode":"deny","tools":{"calendar":["member-tool"]}}}]});
    state.selection = Some(Selection::Connector("calendar".into()));
    state.detail = json!({"tools":[{"slug":"new-tool","hints":["read"]}]});
    assert!(
        state
            .act(Action::ToggleConnectorTool("org-tool".into()))
            .is_empty()
    );
    let request = only(state.act(Action::ToggleConnectorTool("new-tool".into())));
    assert_eq!(request.body["expected_revision"], "member-revision");
    assert_eq!(
        request.body["change"]["disabled_tools"],
        json!(["member-tool", "new-tool"])
    );
}

#[test]
fn hosted_connect_omits_empty_alias_and_unnamed_reconnect_addresses_the_account() {
    let mut state = state();
    let request = only(state.act(Action::Connect("calendar".into(), false)));
    assert!(request.body.get("alias").is_none());
    state.apply("capabilities.write", Err("fixture refusal".into()));
    state.accounts = vec![json!({"connection_id":"account-1","alias":""})];
    let request = only(state.act(Action::ReconnectAccount(
        "calendar".into(),
        "account-1".into(),
    )));
    assert_eq!(request.body["connection_id"], "account-1");
    assert!(request.body.get("alias").is_none());
}

#[test]
fn hosted_operation_fields_keep_user_drafts_and_never_seed_secrets_from_backend_defaults() {
    let value = json!({"op_id":"test-op","targets":[{"name":"calendar","state":"needs_input","required_env":[{"name":"TOKEN","required":true,"secret":true,"default":"do-not-copy"},{"name":"ZONE","required":true,"secret":false,"default":"UTC"}]}]});
    let mut fields = connection_fields(&value, &[]);
    assert!(fields[0].input.text.is_empty());
    assert_eq!(fields[1].input.text, "UTC");
    fields[0].input.replace_text("synthetic-token");
    let next = connection_fields(&value, &fields);
    assert_eq!(next[0].input.text, "synthetic-token");
    let mut state = state();
    state.connection_operation = value;
    state.connection_fields = next;
    let request = only(state.act(Action::AnswerConnection("calendar".into(), true)));
    assert_eq!(request.body["result"]["targets"][0]["status"], "approved");
    state.apply("capabilities.write", Ok(json!({"ok":true})));
    assert!(
        state
            .connection_fields
            .iter()
            .all(|f| f.input.text.is_empty())
    );
}

#[test]
fn operation_polling_is_throttled_and_waits_for_manual_retry_after_error() {
    let mut state = state();
    state.action_name = Some("fixture-action".into());
    state.action_running = true;
    assert_eq!(state.tick(10.0).len(), 1);
    assert!(state.tick(11.0).is_empty());
    assert!(state.tick(13.0).is_empty());
    state.apply("capabilities.action-status", Err("fixture refusal".into()));
    assert!(state.tick(15.0).is_empty());
    assert_eq!(state.act(Action::PollAction).len(), 1);
}

#[test]
fn synthetic_pages_browse_without_backend_requests_and_refuse_writes() {
    let mut state = Capabilities::fixture();
    for selection in [
        Selection::Skill("planning".into()),
        Selection::Toolset("browser".into()),
        Selection::Plugin("notes".into()),
        Selection::Connector("calendar".into()),
        Selection::Mcp("knowledge".into()),
    ] {
        assert!(state.act(Action::Select(selection)).is_empty());
    }
    assert!(
        state
            .act(Action::ToggleSkill("planning".into(), false))
            .is_empty()
    );
    assert!(
        state
            .act(Action::Profile("another-fixture".into()))
            .is_empty()
    );
    assert!(state.demo);
}

#[test]
fn browser_consent_is_a_sparse_scoped_patch_and_computer_permissions_require_support() {
    let mut state = state();
    assert!(state.act(Action::BrowserRealProfile(true)).is_empty());
    state.tool_settings = json!({"browser":{"use_real_profile":false,"other":"keep"}});
    let request = only(state.act(Action::BrowserRealProfile(true)));
    assert_eq!(request.path, "/api/config");
    assert_eq!(request.body, json!({"browser":{"use_real_profile":true}}));
    state.apply("capabilities.write", Err("fixture refusal".into()));
    assert!(state.act(Action::GrantComputerPermissions).is_empty());
    state.computer_status = json!({"can_grant":true,"installed":true});
    let request = only(state.act(Action::GrantComputerPermissions));
    assert_eq!(request.path, "/api/tools/computer-use/permissions/grant");
}

#[test]
fn catalog_install_validates_required_fields_and_polls_the_background_action_receipt() {
    let mut state = state();
    state.mcp_catalog = vec![
        json!({"name":"catalog-mcp","required_env":[{"name":"CATALOG_TOKEN","prompt":"Token","required":true}]}),
    ];
    assert!(
        state
            .act(Action::Select(Selection::McpCatalog("catalog-mcp".into())))
            .is_empty()
    );
    assert!(
        state
            .act(Action::InstallMcpCatalog("catalog-mcp".into()))
            .is_empty()
    );
    state.fields[0].input.replace_text("synthetic-token");
    let request = only(state.act(Action::InstallMcpCatalog("catalog-mcp".into())));
    assert_eq!(
        request.body,
        json!({"name":"catalog-mcp","env":{"CATALOG_TOKEN":"synthetic-token"},"enable":true})
    );
    let poll = only(state.apply(
        "capabilities.write",
        Ok(json!({"ok":true,"background":true,"action":"catalog-install"})),
    ));
    assert_eq!(poll.path, "/api/actions/catalog-install/status");
    assert!(state.action_running);
}

#[test]
fn mcp_oauth_uses_a_client_redirect_and_old_backend_mismatch_never_opens_a_browser() {
    let mut state = state();
    let start = only(state.act(Action::StartMcpOAuth("reports".into())));
    assert_eq!(start.path, "mcp.servers.oauth.start");
    assert!(
        start.body["client_redirect_uri"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );
    assert!(state.busy());
    assert!(state.act(Action::Profile("another".into())).is_empty());
    let cancel=only(state.apply("capabilities.mcp-oauth-start",Ok(json!({"ok":true,"session_id":"flow-1","auth_url":"https://issuer.invalid/authorize?state=fixture-state&redirect_uri=http%3A%2F%2F127.0.0.1%3A1%2Fcallback"}))));
    assert_eq!(cancel.path, "mcp.servers.oauth.cancel");
    let flow = state.mcp_oauth.as_ref().unwrap();
    assert_eq!(flow.status, "error");
    assert!(flow.listener.is_none());
    assert!(
        flow.error
            .as_ref()
            .unwrap()
            .contains("Update the Hermes backend")
    );
}

#[test]
fn cancelling_while_oauth_start_is_pending_releases_the_returned_flow_in_its_owner_scope() {
    let mut state = state();
    only(state.act(Action::StartMcpOAuth("reports".into())));
    assert!(state.act(Action::CancelMcpOAuth).is_empty());
    assert!(state.busy());
    let cancel=only(state.apply("capabilities.mcp-oauth-start",Ok(json!({"ok":true,"session_id":"flow-created-after-cancel","auth_url":"https://issuer.invalid/authorize"}))));
    assert_eq!(cancel.path, "mcp.servers.oauth.cancel");
    assert_eq!(cancel.body["session_id"], "flow-created-after-cancel");
    state.apply("capabilities.mcp-oauth-cancel", Ok(json!({"ok":true})));
    assert!(!state.busy());
}
