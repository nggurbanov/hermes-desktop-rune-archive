use super::*;
impl Capabilities {
    pub(super) fn skill_action(&mut self, action: Action) -> Vec<PageEffect> {
        match action.clone() {
            Action::ToggleSkill(name, enabled) => self.write(
                action,
                PageRequest::put("/api/skills/toggle", json!({"name":name,"enabled":enabled})),
            ),
            Action::BulkSkills(enabled) => {
                if self.busy() {
                    return vec![];
                }
                self.bulk = self
                    .skills
                    .iter()
                    .filter(|s| flag(s, "enabled") != enabled)
                    .map(|s| string(s, "name"))
                    .collect();
                if let Some(name) = self.bulk.pop() {
                    self.act(Action::ToggleSkill(name, enabled))
                } else {
                    vec![]
                }
            }
            Action::EditSkill(name) => {
                if !self.skills.iter().any(|skill| {
                    string(skill, "name") == name
                        && matches!(string(skill, "provenance").as_str(), "agent" | "external")
                }) {
                    return vec![];
                }
                self.editing = true;
                self.editing_key = Some(name.clone());
                vec![self.request(
                    "skill-edit",
                    PageRequest::get("/api/learning/node").query("id", &name),
                )]
            }
            Action::SaveSkill => {
                let Some(name) = self.editing_key.clone() else {
                    return vec![];
                };
                self.write(
                    action,
                    PageRequest::put(
                        "/api/learning/node",
                        json!({"id":name,"content":self.content.text}),
                    ),
                )
            }
            Action::ArchiveSkill(name) => {
                if !self.skills.iter().any(|skill| {
                    string(skill, "name") == name
                        && matches!(string(skill, "provenance").as_str(), "agent" | "external")
                }) {
                    return vec![];
                }
                self.write(
                    action,
                    PageRequest::delete("/api/learning/node", json!({"id":name})),
                )
            }
            Action::SearchHub => vec![
                self.request(
                    "hub-search",
                    PageRequest::get("/api/skills/hub/search")
                        .query("q", self.query.text.trim())
                        .query("source", self.hub_source.text.trim())
                        .query("limit", "50")
                        .timeout_secs(45),
                ),
            ],
            Action::PreviewHub(identifier) => {
                self.selection = Some(Selection::Hub(identifier.clone()));
                self.scan = Value::Null;
                self.detail = Value::Null;
                vec![
                    self.request(
                        "hub-preview",
                        PageRequest::get("/api/skills/hub/preview")
                            .query("identifier", &identifier)
                            .timeout_secs(45),
                    ),
                    self.request(
                        "hub-scan",
                        PageRequest::get("/api/skills/hub/scan")
                            .query("identifier", &identifier)
                            .timeout_secs(45),
                    ),
                ]
            }
            Action::InstallSkill => {
                let Some(Selection::Hub(identifier)) = self.selection.clone() else {
                    return vec![];
                };
                if !matches!(string(&self.scan, "policy").as_str(), "allow" | "ask") {
                    self.message =
                        Some("Review the completed security scan before installing.".into());
                    return vec![];
                }
                self.write(
                    action,
                    PageRequest::post("/api/skills/hub/install", json!({"identifier":identifier}))
                        .timeout_secs(120),
                )
            }
            Action::UninstallSkill(name) => self.write(
                action,
                PageRequest::post("/api/skills/hub/uninstall", json!({"name":name}))
                    .timeout_secs(120),
            ),
            Action::UpdateSkills => self.write(
                action,
                PageRequest::post("/api/skills/hub/update", json!({})).timeout_secs(120),
            ),
            _ => vec![],
        }
    }
    pub(super) fn tool_action(&mut self, action: Action) -> Vec<PageEffect> {
        match action.clone() {
            Action::ToggleToolset(name, enabled) => self.write(
                action,
                PageRequest::put(
                    &format!("/api/tools/toolsets/{}", segment(&name)),
                    json!({"enabled":enabled}),
                ),
            ),
            Action::Provider(provider, capability) => {
                let Some(Selection::Toolset(name)) = self.selection.clone() else {
                    return vec![];
                };
                let mut body = json!({"provider":provider});
                if let Some(c) = capability {
                    body["capability"] = c.into();
                }
                self.write(
                    action,
                    PageRequest::put(
                        &format!("/api/tools/toolsets/{}/provider", segment(&name)),
                        body,
                    ),
                )
            }
            Action::Models(provider) => {
                let Some(Selection::Toolset(name)) = self.selection.clone() else {
                    return vec![];
                };
                self.provider = Some(provider.clone());
                vec![
                    self.request(
                        "tool-models",
                        PageRequest::get(&format!("/api/tools/toolsets/{}/models", segment(&name)))
                            .query("provider", &provider),
                    ),
                ]
            }
            Action::SelectModel(model) => {
                let Some(Selection::Toolset(name)) = self.selection.clone() else {
                    return vec![];
                };
                self.write(
                    action,
                    PageRequest::put(
                        &format!("/api/tools/toolsets/{}/model", segment(&name)),
                        json!({"model":model,"provider":self.provider}),
                    ),
                )
            }
            Action::SaveToolSecrets => self.save_next_secret(action),
            Action::PostSetup(key) => {
                let Some(Selection::Toolset(name)) = self.selection.clone() else {
                    return vec![];
                };
                self.write(
                    action,
                    PageRequest::post(
                        &format!("/api/tools/toolsets/{}/post-setup", segment(&name)),
                        json!({"key":key}),
                    )
                    .timeout_secs(120),
                )
            }
            Action::BrowserRealProfile(enabled) => {
                if self.tool_settings.is_null() {
                    return vec![];
                }
                self.write(
                    action,
                    PageRequest::put(
                        "/api/config",
                        json!({"browser":{"use_real_profile":enabled}}),
                    ),
                )
            }
            Action::ComputerStatus => vec![self.request(
                "computer-status",
                PageRequest::get("/api/tools/computer-use/status"),
            )],
            Action::GrantComputerPermissions => {
                if !flag(&self.computer_status, "can_grant")
                    || !flag(&self.computer_status, "installed")
                {
                    return vec![];
                }
                self.write(
                    action,
                    PageRequest::post("/api/tools/computer-use/permissions/grant", json!({})),
                )
            }
            Action::TerminalBackend(backend) => self.write(
                action,
                PageRequest::put("/api/tools/terminal/backend", json!({"backend":backend})),
            ),
            _ => vec![],
        }
    }
    pub(super) fn plugin_action(&mut self, action: Action) -> Vec<PageEffect> {
        match action.clone() {
            Action::TogglePlugin(key,enable)=>{if !self.plugins.iter().any(|p|string(p,"key")==key && !key.is_empty()){return vec![];}self.write(action,PageRequest::rpc("plugins.manage",json!({"action":"toggle","key":key,"enable":enable})))},
            Action::InspectPlugin=>{self.inspection=Value::Null;let identifier=self.source.text.trim().to_owned();self.inspected_source=identifier.clone();if identifier.is_empty(){return vec![];}vec![self.request("plugin-inspect",PageRequest::rpc("plugins.manage",json!({"action":"inspect","identifier":identifier})).timeout_secs(120))]},
            Action::InstallPlugin=>{
                let sha=string(&self.inspection,"sha");if !flag(&self.inspection,"ok") || !flag(&self.inspection,"agent") || flag(&self.inspection,"insecure") || !full_sha(&sha) || self.inspected_source!=self.source.text.trim() {self.message=Some("Inspect a source at a full commit SHA before installing.".into());return vec![];}
                self.write(action,PageRequest::rpc("plugins.manage",json!({"action":"install","identifier":self.source.text.trim(),"inspected_sha":sha,"enable":true,"force":false})).timeout_secs(120))
            },
            Action::UpdatePlugin(name,accept_capabilities,force)=>self.write(action,PageRequest::rpc("plugins.manage",json!({"action":"update","name":name,"accept_capabilities":accept_capabilities,"force":force})).timeout_secs(120)),
            Action::RemovePlugin(name)=>self.write(action,PageRequest::rpc("plugins.manage",json!({"action":"remove","name":name})).timeout_secs(120)),
            Action::SavePluginSettings=>self.save_plugin_settings(),
            _ => vec![],
        }
    }
    pub(super) fn mcp_action(&mut self, action: Action) -> Vec<PageEffect> {
        match action.clone() {
            Action::SaveMcp => self.save_mcp(),
            Action::SaveMcpDocument => {
                if !self.require_mcp_config() {
                    return vec![];
                }
                match serde_json::from_str::<Map<String, Value>>(&self.mcp_document.text) {
                    Ok(servers) if servers.values().all(Value::is_object) => self.write(
                        action,
                        PageRequest::put("/api/mcp/servers", json!({"servers":servers})),
                    ),
                    _ => {
                        self.message =
                            Some("Enter an object whose values are MCP server objects.".into());
                        vec![]
                    }
                }
            }
            Action::RemoveMcp(name) => {
                if self.mcp.iter().any(|server| {
                    string(server, "name") == name && string(server, "source") == "plugin"
                }) {
                    return vec![];
                }
                self.write(
                    action,
                    PageRequest::rpc("mcp.servers.remove", json!({"name":name})),
                )
            }
            Action::ToggleMcp(name, enabled) => {
                if self.mcp.iter().any(|server| {
                    string(server, "name") == name && string(server, "source") == "plugin"
                }) {
                    return vec![];
                }
                self.write(
                    action,
                    PageRequest::put(
                        format!("/api/mcp/servers/{}/enabled", segment(&name)),
                        json!({"enabled":enabled}),
                    ),
                )
            }
            Action::ProbeMcp(name) => {
                self.mcp_probe = Value::Null;
                vec![self.request(
                    "mcp-probe",
                    PageRequest::rpc("mcp.servers.test", json!({"name":name})).timeout_secs(60),
                )]
            }
            Action::ToggleMcpTool(tool) => self.toggle_mcp_tool(&tool),
            Action::InstallMcpCatalog(name) => {
                let Some(entry) = self.mcp_catalog.iter().find(|e| string(e, "name") == name)
                else {
                    return vec![];
                };
                let mut env = Map::new();
                for required in rows(entry, "required_env") {
                    let key = string(&required, "name");
                    let text = self
                        .fields
                        .iter()
                        .find(|f| f.key == key)
                        .map(|f| f.input.text.clone())
                        .unwrap_or_default();
                    if flag(&required, "required") && text.is_empty() {
                        self.message = Some(format!("{key} is required"));
                        return vec![];
                    }
                    if !text.is_empty() {
                        env.insert(key, text.into());
                    }
                }
                self.write(
                    action,
                    PageRequest::post(
                        "/api/mcp/catalog/install",
                        json!({"name":name,"env":env,"enable":true}),
                    )
                    .timeout_secs(120),
                )
            }
            _ => vec![],
        }
    }
    pub(super) fn connector_action(&mut self, action: Action) -> Vec<PageEffect> {
        match action.clone() {
            Action::ConnectorTools(slug) => {
                self.selection = Some(Selection::Connector(slug.clone()));
                self.detail = Value::Null;
                vec![self.request(
                    "connector-tools",
                    PageRequest::rpc("connectors.tools", json!({"slug":slug,"refresh":false})),
                )]
            }
            Action::ToggleConnector(slug, enabled) => {
                let revision = member_revision(&self.policy);
                if revision.is_empty() || connector_locked(&self.policy, &slug) {
                    return vec![];
                }
                self.write(action,PageRequest::rpc("connectors.policy.set",json!({"change":{"type":"connector","connector":slug,"enabled":enabled},"expected_revision":revision})))
            }
            Action::ToggleConnectorTool(tool) => self.toggle_connector_tool(&tool),
            Action::Connect(slug, reconnect) => {
                let mut body =
                    json!({"connectors":[slug],"owner":{"type":"account"},"reconnect":reconnect});
                if !self.alias.text.trim().is_empty() {
                    body["alias"] = self.alias.text.trim().into();
                }
                self.write(
                    action,
                    PageRequest::rpc("connectors.connect", body).timeout_secs(60),
                )
            }
            Action::ReconnectAccount(slug, id) => {
                let mut body =
                    json!({"connectors":[slug],"owner":{"type":"account"},"reconnect":true});
                let alias = self
                    .accounts
                    .iter()
                    .find(|a| string(a, "connection_id") == id)
                    .map(|a| string(a, "alias"))
                    .unwrap_or_default();
                if alias.is_empty() {
                    body["connection_id"] = id.into();
                } else {
                    body["alias"] = alias.into();
                }
                self.write(
                    action,
                    PageRequest::rpc("connectors.connect", body).timeout_secs(60),
                )
            }
            Action::AnswerConnection(name, approved) => {
                let mut env = Map::new();
                for field in self.connection_fields.iter().filter(|f| f.target == name) {
                    if approved && field.required && field.input.text.trim().is_empty() {
                        self.message = Some(format!("{} is required", field.name));
                        return vec![];
                    }
                    if !field.input.text.is_empty() {
                        env.insert(field.name.clone(), field.input.text.clone().into());
                    }
                }
                let id = string(&self.connection_operation, "op_id");
                self.write(action,PageRequest::rpc("connection.respond",json!({"owner":{"type":"account"},"op_id":id,"result":{"targets":[{"name":name,"status":if approved{"approved"}else{"skipped"},"env":env}]}})).timeout_secs(120))
            }
            Action::StopConnection => {
                let id = string(&self.connection_operation, "op_id");
                self.write(action,PageRequest::rpc("connection.respond",json!({"owner":{"type":"account"},"op_id":id,"result":{"settled_by":"continue"}})))
            }
            Action::RemoveAccount(id) => self.write(
                action,
                PageRequest::rpc("connectors.accounts.remove", json!({"connection_id":id})),
            ),
            Action::RenameAccount(id) => self.write(
                action,
                PageRequest::rpc(
                    "connectors.accounts.rename",
                    json!({"connection_id":id,"alias":self.alias.text.trim()}),
                ),
            ),
            Action::RefreshConnection => {
                let id = string(&self.connection_operation, "op_id");
                if id.is_empty() {
                    return vec![];
                }
                vec![self.request(
                    "connection-operation",
                    PageRequest::rpc(
                        "connectors.operation.status",
                        json!({"op_id":id,"owner":{"type":"account"}}),
                    ),
                )]
            }
            _ => vec![],
        }
    }
}
