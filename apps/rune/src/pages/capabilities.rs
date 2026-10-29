//! Capabilities owns its selected profile, browsing state and confirmed backend data.
//! Requests are dispatched by the application's owner-fenced page transport.
use crate::pages::{PageEffect, PageRequest, segment};
use rune::TextEdit;
use serde_json::{Map, Value, json};
mod actions;
mod logs;
mod oauth;
pub use logs::{LogSource, McpLogs};
#[cfg(test)]
mod tests;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Skills,
    Plugins,
    Connectors,
    Toolsets,
}
impl Mode {
    pub const ALL: [Self; 4] = [
        Self::Skills,
        Self::Plugins,
        Self::Connectors,
        Self::Toolsets,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Skills => "Skills",
            Self::Plugins => "Plugins",
            Self::Connectors => "Connectors",
            Self::Toolsets => "Tools",
        }
    }
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Selection {
    Skill(String),
    Hub(String),
    Toolset(String),
    Plugin(String),
    Mcp(String),
    Connector(String),
    AddMcp,
    InstallPlugin,
    AdvancedMcp,
    McpCatalog(String),
}
#[derive(Clone)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub description: String,
    pub kind: String,
    pub input: TextEdit,
    pub checked: bool,
    pub choices: Vec<String>,
    pub has_value: bool,
    pub initial: String,
}
#[derive(Clone)]
pub struct Confirmation {
    pub title: String,
    pub description: String,
    pub action: Action,
}
#[derive(Clone)]
pub enum Action {
    Mode(Mode),
    Profile(String),
    Refresh,
    Select(Selection),
    CloseDetail,
    ToggleSkill(String, bool),
    BulkSkills(bool),
    EditSkill(String),
    SaveSkill,
    ArchiveSkill(String),
    SearchHub,
    PreviewHub(String),
    InstallSkill,
    UninstallSkill(String),
    UpdateSkills,
    ToggleToolset(String, bool),
    Provider(String, Option<String>),
    Models(String),
    SelectModel(String),
    SaveToolSecrets,
    PostSetup(String),
    TerminalBackend(String),
    BrowserRealProfile(bool),
    ComputerStatus,
    GrantComputerPermissions,
    OpenSettings(String),
    TogglePlugin(String, bool),
    InspectPlugin,
    InstallPlugin,
    UpdatePlugin(String, bool, bool),
    RemovePlugin(String),
    SavePluginSettings,
    SaveMcp,
    SaveMcpDocument,
    RemoveMcp(String),
    ToggleMcp(String, bool),
    ProbeMcp(String),
    ToggleMcpTool(String),
    InstallMcpCatalog(String),
    ConnectorTools(String),
    ToggleConnector(String, bool),
    ToggleConnectorTool(String),
    Connect(String, bool),
    RemoveAccount(String),
    RenameAccount(String),
    RefreshConnection,
    AnswerConnection(String, bool),
    StopConnection,
    ReconnectAccount(String, String),
    RequestConfirmation(Box<Confirmation>),
    Confirm,
    Cancel,
    PollAction,
    StartMcpOAuth(String),
    CancelMcpOAuth,
    PollMcpOAuth,
    McpLogs(LogSource),
    RefreshMcpLogs,
    HideMcpLogs,
}

pub struct TargetField {
    pub target: String,
    pub name: String,
    pub prompt: String,
    pub required: bool,
    pub secret: bool,
    pub input: TextEdit,
}

pub struct Capabilities {
    pub demo: bool,
    pub profile_menu: bool,
    pub open_field: Option<String>,
    pub mode: Mode,
    pub profile: Option<String>,
    pub query: TextEdit,
    pub skills: Vec<Value>,
    pub official: Vec<Value>,
    pub hub: Vec<Value>,
    pub toolsets: Vec<Value>,
    pub plugins: Vec<Value>,
    pub mcp: Vec<Value>,
    pub mcp_catalog: Vec<Value>,
    pub connectors: Vec<Value>,
    pub connector_catalog: Vec<Value>,
    pub accounts: Vec<Value>,
    pub policy: Value,
    pub selection: Option<Selection>,
    pub detail: Value,
    pub content: TextEdit,
    pub editing: bool,
    pub fields: Vec<Field>,
    pub provider: Option<String>,
    pub tool_models: Value,
    pub terminal: Value,
    pub tool_settings: Value,
    pub computer_status: Value,
    pub source: TextEdit,
    pub hub_source: TextEdit,
    pub alias: TextEdit,
    pub mcp_name: TextEdit,
    pub mcp_url: TextEdit,
    pub mcp_command: TextEdit,
    pub mcp_args: TextEdit,
    pub mcp_env: TextEdit,
    pub mcp_headers: TextEdit,
    pub mcp_config: Map<String, Value>,
    pub mcp_config_loaded: bool,
    pub mcp_document: TextEdit,
    pub mcp_probe: Value,
    pub mcp_oauth: Option<crate::oauth::McpOAuth>,
    pub mcp_logs: McpLogs,
    pub inspection: Value,
    pub scan: Value,
    pub connection_operation: Value,
    pub connection_fields: Vec<TargetField>,
    pub next_poll: f64,
    pub inspected_source: String,
    pub errors: BTreeMap<String, String>,
    pub pending: HashSet<String>,
    pub mutation: bool,
    pub confirm: Option<Confirmation>,
    pub message: Option<String>,
    pub action_name: Option<String>,
    pub action_log: Vec<String>,
    pub action_running: bool,
    bulk: Vec<String>,
    pub editing_key: Option<String>,
    write_context: Option<Action>,
    writing_secret: Option<usize>,
}
impl Default for Capabilities {
    fn default() -> Self {
        Self {
            demo: false,
            profile_menu: false,
            open_field: None,
            mode: Mode::Skills,
            profile: None,
            query: TextEdit::new(""),
            skills: vec![],
            official: vec![],
            hub: vec![],
            toolsets: vec![],
            plugins: vec![],
            mcp: vec![],
            mcp_catalog: vec![],
            connectors: vec![],
            connector_catalog: vec![],
            accounts: vec![],
            policy: Value::Null,
            selection: None,
            detail: Value::Null,
            content: TextEdit::new(""),
            editing: false,
            fields: vec![],
            provider: None,
            tool_models: Value::Null,
            terminal: Value::Null,
            tool_settings: Value::Null,
            computer_status: Value::Null,
            source: TextEdit::new(""),
            hub_source: TextEdit::new("all"),
            alias: TextEdit::new(""),
            mcp_name: TextEdit::new(""),
            mcp_url: TextEdit::new(""),
            mcp_command: TextEdit::new(""),
            mcp_args: TextEdit::new("[]"),
            mcp_env: TextEdit::new("{}"),
            mcp_headers: TextEdit::new("{}"),
            mcp_config: Map::new(),
            mcp_config_loaded: false,
            mcp_document: TextEdit::new("{}"),
            mcp_probe: Value::Null,
            mcp_oauth: None,
            mcp_logs: McpLogs::default(),
            inspection: Value::Null,
            scan: Value::Null,
            connection_operation: Value::Null,
            connection_fields: vec![],
            next_poll: 0.0,
            inspected_source: String::new(),
            errors: BTreeMap::new(),
            pending: HashSet::new(),
            mutation: false,
            confirm: None,
            message: None,
            action_name: None,
            action_log: vec![],
            action_running: false,
            bulk: vec![],
            editing_key: None,
            write_context: None,
            writing_secret: None,
        }
    }
}
impl Capabilities {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn busy(&self) -> bool {
        self.mutation
            || self.action_running
            || self.mcp_oauth.as_ref().is_some_and(|flow| flow.running())
    }
    fn request(&mut self, key: &str, request: PageRequest) -> PageEffect {
        let key = format!("capabilities.{key}");
        self.pending.insert(key.clone());
        self.errors.remove(&key);
        let request = if let Some(profile) = &self.profile {
            request.profile(profile)
        } else {
            request
        };
        (key, request)
    }
    fn write(&mut self, action: Action, request: PageRequest) -> Vec<PageEffect> {
        if self.demo {
            self.message = Some("Demonstration data · writes are disabled".into());
            return vec![];
        }
        if self.busy() {
            return vec![];
        }
        self.mutation = true;
        self.write_context = Some(action);
        self.message = None;
        vec![self.request("write", request)]
    }
    pub fn needs_poll(&self) -> bool {
        !self.demo
            && ((self.mcp_logs.opened && self.mcp_logs.error.is_none())
                || self.action_running
                || self.mcp_oauth.as_ref().is_some_and(|flow| flow.running())
                || (!self.connection_operation.is_null()
                    && !flag(&self.connection_operation, "settled")))
    }
    pub fn tick(&mut self, now: f64) -> Vec<PageEffect> {
        if !self.needs_poll() || now < self.next_poll {
            return vec![];
        }
        self.next_poll = now + 2.0;
        let mut effects = self.tick_mcp_oauth();
        if self.mcp_logs.error.is_none() {
            effects.extend(self.read_mcp_logs());
        }
        if self.action_running
            && !self.pending.contains("capabilities.action-status")
            && !self.errors.contains_key("capabilities.action-status")
        {
            effects.extend(self.act(Action::PollAction));
        }
        if !self.connection_operation.is_null()
            && !flag(&self.connection_operation, "settled")
            && !self.pending.contains("capabilities.connection-operation")
            && !self
                .errors
                .contains_key("capabilities.connection-operation")
        {
            effects.extend(self.act(Action::RefreshConnection));
        }
        effects
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        if self.demo {
            return vec![];
        }
        match self.mode {
            Mode::Skills => vec![
                self.request("skills", PageRequest::get("/api/skills")),
                self.request("official", PageRequest::get("/api/skills/hub/official")),
            ],
            Mode::Toolsets => {
                vec![self.request("toolsets", PageRequest::get("/api/tools/toolsets"))]
            }
            Mode::Plugins => vec![self.request(
                "plugins",
                PageRequest::rpc("plugins.manage", json!({"action":"list"})),
            )],
            Mode::Connectors => vec![
                self.request("mcp", PageRequest::rpc("mcp.servers.list", json!({}))),
                self.request("mcp-catalog", PageRequest::get("/api/mcp/catalog")),
                self.request("mcp-config", PageRequest::get("/api/config")),
                self.request(
                    "connectors",
                    PageRequest::rpc("connectors.list", json!({"owner":{"type":"account"}})),
                ),
                self.request(
                    "connector-catalog",
                    PageRequest::rpc("connectors.catalog", json!({})),
                ),
                self.request(
                    "accounts",
                    PageRequest::rpc("connectors.accounts", json!({})),
                ),
                self.request(
                    "policy",
                    PageRequest::rpc("connectors.policy.get", json!({})),
                ),
            ],
        }
    }
    pub fn act(&mut self, action: Action) -> Vec<PageEffect> {
        if self.demo
            && matches!(
                &action,
                Action::SearchHub
                    | Action::PreviewHub(_)
                    | Action::InspectPlugin
                    | Action::Models(_)
                    | Action::ConnectorTools(_)
                    | Action::ProbeMcp(_)
                    | Action::RefreshConnection
                    | Action::PollAction
                    | Action::ComputerStatus
            )
        {
            return vec![];
        }
        if self.busy()
            && !matches!(
                action,
                Action::PollAction
                    | Action::RefreshConnection
                    | Action::AnswerConnection(..)
                    | Action::StopConnection
                    | Action::Cancel
                    | Action::CancelMcpOAuth
                    | Action::PollMcpOAuth
                    | Action::McpLogs(_)
                    | Action::RefreshMcpLogs
                    | Action::HideMcpLogs
            )
        {
            return vec![];
        }
        match action.clone() {
            Action::McpLogs(source) => self.show_mcp_logs(source),
            Action::RefreshMcpLogs => {
                self.mcp_logs.error = None;
                self.read_mcp_logs()
            }
            Action::HideMcpLogs => {
                self.mcp_logs.reset();
                vec![]
            }
            Action::Mode(mode) => {
                self.mode = mode;
                self.close_detail();
                self.load()
            }
            Action::Profile(profile) => {
                let mode = self.mode;
                let demo = self.demo;
                *self = if demo { Self::fixture() } else { Self::new() };
                self.mode = mode;
                self.profile = Some(profile);
                self.load()
            }
            Action::Refresh => self.load(),
            Action::CloseDetail => {
                self.close_detail();
                vec![]
            }
            Action::Select(selection) => self.select(selection),
            Action::RequestConfirmation(confirmation) => {
                if !self.busy() {
                    self.confirm = Some(*confirmation);
                }
                vec![]
            }
            Action::Confirm => {
                if let Some(c) = self.confirm.take() {
                    self.act(c.action)
                } else {
                    vec![]
                }
            }
            Action::Cancel => {
                self.confirm = None;
                vec![]
            }
            Action::PollAction => {
                let Some(name) = self.action_name.clone() else {
                    return vec![];
                };
                vec![
                    self.request(
                        "action-status",
                        PageRequest::get(&format!("/api/actions/{}/status", segment(&name)))
                            .query("lines", "200"),
                    ),
                ]
            }
            Action::ToggleSkill(..)
            | Action::BulkSkills(..)
            | Action::EditSkill(..)
            | Action::SaveSkill
            | Action::ArchiveSkill(..)
            | Action::SearchHub
            | Action::PreviewHub(..)
            | Action::InstallSkill
            | Action::UninstallSkill(..)
            | Action::UpdateSkills => self.skill_action(action),
            Action::ToggleToolset(..)
            | Action::Provider(..)
            | Action::Models(..)
            | Action::SelectModel(..)
            | Action::SaveToolSecrets
            | Action::PostSetup(..)
            | Action::TerminalBackend(..)
            | Action::BrowserRealProfile(..)
            | Action::ComputerStatus
            | Action::GrantComputerPermissions => self.tool_action(action),
            Action::OpenSettings(_) => vec![],
            Action::StartMcpOAuth(_) | Action::CancelMcpOAuth | Action::PollMcpOAuth => {
                self.mcp_oauth_action(action)
            }
            Action::TogglePlugin(..)
            | Action::InspectPlugin
            | Action::InstallPlugin
            | Action::UpdatePlugin(..)
            | Action::RemovePlugin(..)
            | Action::SavePluginSettings => self.plugin_action(action),
            Action::SaveMcp
            | Action::SaveMcpDocument
            | Action::RemoveMcp(..)
            | Action::ToggleMcp(..)
            | Action::ProbeMcp(..)
            | Action::ToggleMcpTool(..)
            | Action::InstallMcpCatalog(..) => self.mcp_action(action),
            Action::AnswerConnection(..)
            | Action::StopConnection
            | Action::ReconnectAccount(..) => self.connector_action(action),
            Action::ConnectorTools(..)
            | Action::ToggleConnector(..)
            | Action::ToggleConnectorTool(..)
            | Action::Connect(..)
            | Action::RemoveAccount(..)
            | Action::RenameAccount(..)
            | Action::RefreshConnection => self.connector_action(action),
        }
    }

    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        self.pending.remove(op);
        let key = op.strip_prefix("capabilities.").unwrap_or(op);
        if key == "write" {
            self.mutation = false;
        }
        if key.starts_with("mcp-logs.") {
            return self.apply_mcp_logs(op, result);
        }
        if key.starts_with("mcp-oauth-") {
            return self.apply_mcp_oauth(key, result);
        }
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                self.errors.insert(op.into(), e);
                self.bulk.clear();
                self.writing_secret = None;
                return vec![];
            }
        };
        if key == "write" {
            return self.applied_write(value);
        }
        self.errors.remove(op);
        match key {
            "skills" => self.skills = rows(&value, "skills"),
            "official" => self.official = rows(&value, "skills"),
            "hub-search" => self.hub = rows(&value, "results"),
            "skill-content" => self.content.replace_text(string(&value, "content")),
            "skill-edit" => {
                self.content.replace_text(string(&value, "content"));
            }
            "hub-preview" => {
                self.content.replace_text(string(&value, "skill_md"));
                self.detail = value;
            }
            "hub-scan" => self.scan = value,
            "toolsets" => {
                self.toolsets = rows(&value, "toolsets")
                    .into_iter()
                    .filter(|v| toolset_visible(&string(v, "name")))
                    .collect()
            }
            "tool-config" => {
                self.fields = tool_fields(&value);
                self.detail = value;
            }
            "tool-models" => self.tool_models = value,
            "terminal" => self.terminal = value,
            "tool-general-config" => self.tool_settings = json!({"browser":value["browser"]}),
            "computer-status" => self.computer_status = value,
            "plugins" => {
                self.plugins = rows(&value, "plugins")
                    .into_iter()
                    .filter(plugin_visible)
                    .collect();
                if let Some(Selection::Plugin(key)) = self.selection.clone() {
                    return self.select(Selection::Plugin(key));
                }
            }
            "plugin-inspect" => self.inspection = value,
            "mcp" => {
                self.mcp = rows(&value, "servers");
                self.resolve_mcp_setup();
            }
            "mcp-catalog" => {
                self.mcp_catalog = rows(&value, "entries");
                self.resolve_mcp_setup();
            }
            "mcp-config" => {
                // A missing map is a valid empty config; a malformed map cannot authorize replacement.
                if value.get("mcp_servers").is_none_or(Value::is_null)
                    || value.get("mcp_servers").is_some_and(Value::is_object)
                {
                    self.mcp_config = value
                        .get("mcp_servers")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    self.mcp_config_loaded = true;
                    self.mcp_document
                        .replace_text(pretty(&Value::Object(self.mcp_config.clone()), "{}"));
                    if let Some(Selection::Mcp(name)) = self.selection.clone() {
                        self.hydrate_mcp_fields(&name);
                    }
                    self.resolve_mcp_setup();
                } else {
                    self.mcp_config_loaded = false;
                    self.errors.insert(
                        op.into(),
                        "The agent returned an invalid MCP server map. Reload before editing."
                            .into(),
                    );
                }
            }
            "mcp-probe" => self.mcp_probe = value,
            "connectors" => self.connectors = rows(&value, "connectors"),
            "connector-catalog" => self.connector_catalog = rows(&value, "connectors"),
            "accounts" => self.accounts = rows(&value, "accounts"),
            "policy" => self.policy = value,
            "connector-tools" => self.detail = value,
            "connection-operation" => {
                self.connection_fields = connection_fields(&value, &self.connection_fields);
                self.connection_operation = value;
            }
            "action-status" => {
                self.action_running = flag(&value, "running");
                self.action_log = strings(&value["lines"]);
                if !self.action_running {
                    if value.get("exit_code").and_then(Value::as_i64) == Some(0) {
                        self.message = Some("Operation completed.".into());
                        let mut effects = self.load();
                        if let Some(selection) = self.selection.clone() {
                            effects.extend(self.select(selection));
                        }
                        return effects;
                    }
                    self.message = Some("Operation failed. Review the action log.".into());
                }
            }
            _ => {}
        }
        vec![]
    }
    fn applied_write(&mut self, value: Value) -> Vec<PageEffect> {
        let action = self.write_context.take();
        if value.get("consent_required").and_then(Value::as_bool) == Some(true) {
            if let Some(Action::UpdatePlugin(name, _, force)) = action {
                self.confirm = Some(Confirmation {
                    title: "Review plugin capability changes".into(),
                    description: strings(&value["delta_lines"]).join("\n"),
                    action: Action::UpdatePlugin(name, true, force),
                });
            }
            return vec![];
        }
        if flag(&value, "scan_blocked") {
            if string(&value, "scan_verdict") == "caution" {
                if let Some(Action::UpdatePlugin(name, accepted, _)) = action {
                    self.confirm = Some(Confirmation {
                        title: "Plugin scan requires review".into(),
                        description: scan_text(&value),
                        action: Action::UpdatePlugin(name, accepted, true),
                    });
                }
            } else {
                self.message = Some("The security scan blocked this plugin update.".into());
            }
            return vec![];
        }
        if value.get("ok").and_then(Value::as_bool) == Some(false) {
            self.writing_secret = None;
            self.message = Some(string(&value, "message").chars().take(500).collect());
            if self.message.as_ref().is_none_or(String::is_empty) {
                self.message = Some("The backend refused this operation.".into());
            }
            self.bulk.clear();
            return vec![];
        }
        if let Some(index) = self.writing_secret.take() {
            if let Some(field) = self.fields.get_mut(index) {
                field.input.replace_text("");
            }
            return match action {
                Some(Action::SaveToolSecrets) => self.save_next_secret(Action::SaveToolSecrets),
                Some(Action::SavePluginSettings) => self.save_plugin_settings(),
                _ => vec![],
            };
        }
        if let Some(Action::ToggleSkill(_, enabled)) = action {
            if let Some(name) = self.bulk.pop() {
                return self.act(Action::ToggleSkill(name, enabled));
            }
        }
        if let Some(Action::AnswerConnection(name, _)) = &action {
            for field in self
                .connection_fields
                .iter_mut()
                .filter(|f| &f.target == name)
            {
                field.input.replace_text("");
            }
        }
        if value.get("op_id").and_then(Value::as_str).is_some() {
            self.connection_fields = connection_fields(&value, &self.connection_fields);
            self.connection_operation = value;
            return self.act(Action::RefreshConnection);
        }
        let background_name = value
            .get("action")
            .and_then(Value::as_str)
            .filter(|_| flag(&value, "background"))
            .or_else(|| {
                value
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|_| value.get("pid").is_some())
            });
        if let Some(name) = background_name {
            self.action_name = Some(name.into());
            self.action_running = true;
            self.action_log.clear();
            return self.act(Action::PollAction);
        }
        self.message = Some(if flag(&value, "needs_nous_auth") {
            "The provider was selected. Sign in to Nous in Settings → Providers before it can activate.".into()
        } else if flag(&value, "restart_required") {
            "Saved. The agent reports that a restart is required; open conversations may keep their current capabilities.".into()
        } else if flag(&value, "unchanged") {
            "The selected agent already has this configuration.".into()
        } else {
            "Saved on the selected agent. Prompt-affecting changes apply to the next conversation."
                .into()
        });
        self.editing = false;
        let mut effects = self.load();
        if let Some(selection) = self.selection.clone() {
            effects.extend(self.select(selection));
        }
        effects
    }
    fn select(&mut self, selection: Selection) -> Vec<PageEffect> {
        self.close_detail();
        self.selection = Some(selection.clone());
        if self.demo && self.demo_detail(&selection) {
            return vec![];
        }
        match selection {
            Selection::Skill(name) => vec![self.request(
                "skill-content",
                PageRequest::get("/api/skills/content").query("name", &name),
            )],
            Selection::Hub(identifier) => self.act(Action::PreviewHub(identifier)),
            Selection::Toolset(name) => {
                let mut effects = vec![self.request(
                    "tool-config",
                    PageRequest::get(&format!("/api/tools/toolsets/{}/config", segment(&name))),
                )];
                if name == "browser" {
                    effects
                        .push(self.request("tool-general-config", PageRequest::get("/api/config")));
                }
                if name == "computer_use" {
                    effects.extend(self.act(Action::ComputerStatus));
                }
                if name == "terminal" {
                    effects.push(
                        self.request("terminal", PageRequest::get("/api/tools/terminal/backends")),
                    );
                }
                effects
            }
            Selection::Plugin(key) => {
                if let Some(row) = self
                    .plugins
                    .iter()
                    .find(|r| string(r, "key") == key)
                    .cloned()
                {
                    self.fields = plugin_fields(&row);
                    self.detail = row;
                }
                vec![]
            }
            Selection::Mcp(name) => {
                if let Some(config) = self.mcp_config.get(&name) {
                    self.mcp_name.replace_text(&name);
                    self.mcp_url.replace_text(string(config, "url"));
                    self.mcp_command.replace_text(string(config, "command"));
                    self.mcp_args.replace_text(pretty(&config["args"], "[]"));
                    self.mcp_env.replace_text(pretty(&config["env"], "{}"));
                    self.mcp_headers
                        .replace_text(pretty(&config["headers"], "{}"));
                }
                vec![]
            }
            Selection::Connector(slug) => self.act(Action::ConnectorTools(slug)),
            Selection::AddMcp => {
                self.mcp_name.replace_text("");
                self.mcp_url.replace_text("");
                self.mcp_command.replace_text("");
                self.mcp_args.replace_text("[]");
                self.mcp_env.replace_text("{}");
                self.mcp_headers.replace_text("{}");
                vec![]
            }
            Selection::InstallPlugin => {
                self.source.replace_text("");
                self.inspection = Value::Null;
                vec![]
            }
            Selection::AdvancedMcp => vec![],
            Selection::McpCatalog(name) => {
                if let Some(entry) = self.mcp_catalog.iter().find(|e| string(e, "name") == name) {
                    self.detail = entry.clone();
                    self.fields = rows(entry, "required_env")
                        .iter()
                        .map(|env| {
                            let mut input = TextEdit::new("");
                            input.set_secure(true);
                            Field {
                                key: string(env, "name"),
                                label: nonempty_label(env),
                                description: if flag(env, "required") {
                                    "Required".into()
                                } else {
                                    "Optional".into()
                                },
                                kind: "secret".into(),
                                input,
                                checked: false,
                                choices: vec![],
                                has_value: false,
                                initial: String::new(),
                            }
                        })
                        .collect();
                }
                vec![]
            }
        }
    }
    fn close_detail(&mut self) {
        self.mcp_logs.reset();
        self.selection = None;
        self.detail = Value::Null;
        self.content.replace_text("");
        self.editing = false;
        self.editing_key = None;
        self.fields.clear();
        self.tool_models = Value::Null;
        self.terminal = Value::Null;
        self.tool_settings = Value::Null;
        self.computer_status = Value::Null;
        self.confirm = None;
        self.scan = Value::Null;
        self.mcp_probe = Value::Null;
    }
    fn save_next_secret(&mut self, action: Action) -> Vec<PageEffect> {
        if let Some(index) = self
            .fields
            .iter()
            .position(|f| f.kind == "secret" && !f.key.is_empty() && !f.input.text.is_empty())
        {
            let field = &self.fields[index];
            let body = json!({"key":field.key,"value":field.input.text});
            self.writing_secret = Some(index);
            return self.write(action, PageRequest::put("/api/env", body));
        }
        self.message = Some("Credentials saved on the selected agent.".into());
        if let Some(selection) = self.selection.clone() {
            self.select(selection)
        } else {
            vec![]
        }
    }
    fn save_plugin_settings(&mut self) -> Vec<PageEffect> {
        let Some(Selection::Plugin(key)) = self.selection.clone() else {
            return vec![];
        };
        if key.is_empty() || !self.plugins.iter().any(|p| string(p, "key") == key) {
            return vec![];
        }
        let mut values = Map::new();
        for field in &self.fields {
            if field.kind == "secret" {
                continue;
            }
            let current = if field.kind == "boolean" {
                field.checked.to_string()
            } else {
                field.input.text.clone()
            };
            if current == field.initial {
                continue;
            }
            let value = match field.kind.as_str() {
                "boolean" => Value::Bool(field.checked),
                "enum" => {
                    if !field.choices.contains(&current) {
                        self.message = Some(format!(
                            "{} must be one of the available choices",
                            field.label
                        ));
                        return vec![];
                    }
                    Value::String(current)
                }
                "number" => match field
                    .input
                    .text
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                {
                    Some(v) => Value::Number(v),
                    None => {
                        self.message = Some(format!("{} must be a number", field.label));
                        return vec![];
                    }
                },
                "json" => match serde_json::from_str::<Value>(&field.input.text) {
                    Ok(v) if v.is_object() || v.is_array() => v,
                    _ => {
                        self.message = Some(format!(
                            "{} must contain a JSON object or list",
                            field.label
                        ));
                        return vec![];
                    }
                },
                _ => Value::String(field.input.text.clone()),
            };
            values.insert(field.key.clone(), value);
        }
        if self
            .fields
            .iter()
            .any(|f| f.kind == "secret" && !f.key.is_empty() && !f.input.text.is_empty())
        {
            return self.save_next_secret(Action::SavePluginSettings);
        }
        if values.is_empty() {
            self.message = Some("Settings saved.".into());
            return self.load();
        }
        self.write(
            Action::SavePluginSettings,
            PageRequest::rpc(
                "plugins.manage",
                json!({"action":"settings","key":key,"values":values}),
            ),
        )
    }
    fn save_mcp(&mut self) -> Vec<PageEffect> {
        if !self.require_mcp_config() {
            return vec![];
        }
        let name = self.mcp_name.text.trim().to_owned();
        if name.is_empty() {
            self.message = Some("Give the server a name.".into());
            return vec![];
        }
        if matches!(self.selection, Some(Selection::AddMcp)) && self.mcp_config.contains_key(&name)
        {
            self.message = Some(
                "A server with this name already exists. Open it to edit its configuration.".into(),
            );
            return vec![];
        }
        if self
            .mcp
            .iter()
            .any(|server| string(server, "name") == name && string(server, "source") == "plugin")
        {
            self.message = Some(
                "This server is owned by its plugin. Configure it from the Plugins tab.".into(),
            );
            return vec![];
        }
        let args = match serde_json::from_str::<Vec<String>>(&self.mcp_args.text) {
            Ok(v) => v,
            Err(_) => {
                self.message = Some("Arguments must be an array of strings.".into());
                return vec![];
            }
        };
        let env = match serde_json::from_str::<Map<String, Value>>(&self.mcp_env.text) {
            Ok(v) if v.values().all(Value::is_string) => v,
            _ => {
                self.message = Some("Environment must map variable names to strings.".into());
                return vec![];
            }
        };
        let headers = match serde_json::from_str::<Map<String, Value>>(&self.mcp_headers.text) {
            Ok(v) if v.values().all(Value::is_string) => v,
            _ => {
                self.message = Some("Headers must map names to strings.".into());
                return vec![];
            }
        };
        if self.mcp_url.text.trim().is_empty() && self.mcp_command.text.trim().is_empty() {
            self.message = Some("Provide a URL or a command.".into());
            return vec![];
        }
        let mut config = self
            .mcp_config
            .get(&name)
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !config.is_object() {
            self.message =
                Some("The server configuration is invalid. Reload before editing.".into());
            return vec![];
        }
        if !self.mcp_url.text.trim().is_empty() {
            config["url"] = self.mcp_url.text.trim().into();
            config.as_object_mut().unwrap().remove("command");
            config.as_object_mut().unwrap().remove("args");
        } else {
            config["command"] = self.mcp_command.text.trim().into();
            config["args"] = json!(args);
            config.as_object_mut().unwrap().remove("url");
        }
        config["env"] = Value::Object(env);
        config["headers"] = Value::Object(headers);
        let mut servers = self.mcp_config.clone();
        servers.insert(name.clone(), config);
        self.write(
            Action::SaveMcp,
            PageRequest::put("/api/mcp/servers", json!({"servers":servers})),
        )
    }
    fn toggle_mcp_tool(&mut self, name: &str) -> Vec<PageEffect> {
        if !self.require_mcp_config() {
            return vec![];
        }
        let Some(Selection::Mcp(server)) = self.selection.clone() else {
            return vec![];
        };
        let Some(config) = self.mcp_config.get(&server) else {
            return vec![];
        };
        if !config.is_object() {
            self.message =
                Some("The server configuration is invalid. Reload before editing.".into());
            return vec![];
        }
        let next = toggle_tool(config, name);
        let mut servers = self.mcp_config.clone();
        servers.insert(server, next);
        self.write(
            Action::ToggleMcpTool(name.into()),
            PageRequest::put("/api/mcp/servers", json!({"servers":servers})),
        )
    }
    fn toggle_connector_tool(&mut self, name: &str) -> Vec<PageEffect> {
        let Some(Selection::Connector(slug)) = self.selection.clone() else {
            return vec![];
        };
        let revision = member_revision(&self.policy);
        if revision.is_empty() || connector_tool_locked(&self.policy, &slug, name, &self.detail) {
            return vec![];
        }
        let mut disabled = connector_disabled(&self.policy, &slug);
        if disabled.iter().any(|n| n == name) {
            disabled.retain(|n| n != name);
        } else {
            disabled.push(name.into());
        }
        self.write(Action::ToggleConnectorTool(name.into()),PageRequest::rpc("connectors.policy.set",json!({"change":{"type":"tools","connector":slug,"disabled_tools":disabled},"expected_revision":revision})))
    }
    fn hydrate_mcp_fields(&mut self, name: &str) {
        if let Some(config) = self.mcp_config.get(name) {
            self.mcp_name.replace_text(name);
            self.mcp_url.replace_text(string(config, "url"));
            self.mcp_command.replace_text(string(config, "command"));
            self.mcp_args.replace_text(pretty(&config["args"], "[]"));
            self.mcp_env.replace_text(pretty(&config["env"], "{}"));
            self.mcp_headers
                .replace_text(pretty(&config["headers"], "{}"));
        }
    }
    fn resolve_mcp_setup(&mut self) {
        if !self.mcp_config_loaded
            || self.pending.contains("capabilities.mcp-catalog")
            || self.pending.contains("capabilities.mcp")
        {
            return;
        }
        let Some(Selection::Mcp(name)) = self.selection.clone() else {
            return;
        };
        if self.mcp_config.contains_key(&name)
            || self.mcp.iter().any(|row| string(row, "name") == name)
        {
            return;
        }
        if self
            .mcp_catalog
            .iter()
            .any(|row| string(row, "name") == name)
        {
            self.select(Selection::McpCatalog(name));
        }
    }
    fn require_mcp_config(&mut self) -> bool {
        if self.mcp_config_loaded
            && !self.pending.contains("capabilities.mcp-config")
            && !self.errors.contains_key("capabilities.mcp-config")
        {
            return true;
        }
        self.message = Some("Wait for the complete MCP configuration to load, or reload after an error, before replacing it.".into());
        false
    }
    fn demo_detail(&mut self, selection: &Selection) -> bool {
        match selection {
            Selection::Skill(name) => self.content.replace_text(format!("---\nname: {name}\ndescription: A synthetic skill for interface review.\n---\n\n# {name}\n\nTurn the requested goal into clear, practical steps.\n")),
            Selection::Hub(identifier) => {
                self.detail = json!({"name":identifier,"description":"A synthetic official skill preview.","files":["SKILL.md"]});
                self.content.replace_text("# Calendar\n\nRead events and help plan the day.");
                self.scan = json!({"policy":"allow","verdict":"clean","summary":"No findings in this synthetic preview.","findings":[]});
            }
            Selection::Toolset(_) => {
                self.tool_settings=json!({"browser":{"use_real_profile":false}});
                self.computer_status=json!({"platform":"linux","platform_supported":true,"installed":true,"ready":true,"can_grant":false,"version":"Synthetic preview","checks":[]});
                self.detail = json!({"providers":[{"name":"Local","status":"ready","is_active":true,"env_vars":[]}]});
                self.fields = tool_fields(&self.detail);
            }
            Selection::Connector(_) => self.detail = json!({"tools":[{"slug":"calendar_list","name":"List events","description":"Read calendar events.","hints":["read"]},{"slug":"calendar_create","name":"Create event","description":"Add an event to your calendar.","hints":["write"]}]}),
            _ => return false,
        }
        true
    }
    pub fn fixture() -> Self {
        let mut s = Self::new();
        s.demo = true;
        s.skills = rows(
            &json!([{"name":"planning","description":"Turn a goal into a practical plan.","category":"Productivity","enabled":true,"provenance":"agent","usage":12},{"name":"research","description":"Read primary sources and keep useful findings.","category":"Research","enabled":false,"provenance":"hub"}]),
            "",
        );
        s.official = rows(
            &json!({"skills":[{"name":"calendar","identifier":"official/calendar","description":"Work with your calendar.","category":"Productivity","installed":false}]}),
            "skills",
        );
        s.toolsets = rows(
            &json!([{"name":"browser","label":"Browser","description":"Browse and interact with pages.","enabled":true,"configured":true,"tools":["browser_open","browser_click"]},{"name":"terminal","label":"Terminal","description":"Run commands on the agent host.","enabled":true,"configured":true,"tools":["terminal"]}]),
            "",
        );
        s.plugins = rows(
            &json!({"plugins":[{"name":"Notes","key":"notes","description":"Collect notes for this agent.","status":"enabled","source":"user","version":"1.0","settings_schema":[{"key":"daily","label":"Daily notes","type":"boolean","value":true},{"key":"api_key","env":"NOTES_API_KEY","label":"API key","type":"secret","has_value":true}]}]}),
            "plugins",
        );
        s.mcp = rows(
            &json!({"servers":[{"name":"knowledge","transport":"stdio","command":"knowledge-mcp","args":[],"enabled":true,"source":"config"}]}),
            "servers",
        );
        s.connectors = rows(
            &json!({"connectors":[{"connector":"calendar","name":"Calendar","description":"Plan your day with your calendar.","enabled":true,"connected":false}]}),
            "connectors",
        );
        s.connector_catalog = vec![
            json!({"slug":"calendar","name":"Calendar","description":"Plan your day with your calendar."}),
        ];
        s.policy = json!({"layers":[{"kind":"member","revision":"synthetic","body":{"mode":"deny","disabled_connectors":[],"tools":{}}}],"effective":{"disabled_tools":{}}});
        s.mcp_config_loaded = true;
        s.mcp_config.insert(
            "knowledge".into(),
            json!({"command":"knowledge-mcp","args":[]}),
        );
        s.mcp_document
            .replace_text(pretty(&Value::Object(s.mcp_config.clone()), "{}"));
        s
    }
}

impl crate::app::Viewer {
    pub fn open_mcp_settings(&mut self, profile: Option<String>, name: String) {
        if self.capabilities.busy() {
            self.settings.error =
                Some("Finish the current capability operation before opening MCP settings".into());
            return;
        }
        let profile = profile.unwrap_or_else(|| self.profile.clone());
        self.open_route(crate::pages::Route::Capabilities);
        self.invalidate_page_requests("capabilities.");
        let demo = self.capabilities.demo;
        self.capabilities = if demo {
            Capabilities::fixture()
        } else {
            Capabilities::new()
        };
        self.capabilities.profile = Some(profile);
        self.capabilities.mode = Mode::Connectors;
        self.capabilities.selection = Some(Selection::Mcp(name.clone()));
        self.capabilities.mcp_name.replace_text(&name);
        let mut effects = self.capabilities.load();
        if demo {
            effects.extend(self.capabilities.select(Selection::Mcp(name)));
        }
        self.page_requests(effects);
    }
    pub fn open_capabilities_for_profile(&mut self, profile: Option<String>) {
        if self.capabilities.busy() {
            self.settings.error =
                Some("Finish the current capability operation before opening capabilities".into());
            return;
        }
        let profile = profile.unwrap_or_else(|| self.profile.clone());
        self.open_route(crate::pages::Route::Capabilities);
        self.invalidate_page_requests("capabilities.");
        let demo = self.capabilities.demo;
        self.capabilities = if demo {
            Capabilities::fixture()
        } else {
            Capabilities::new()
        };
        self.capabilities.profile = Some(profile);
        self.capabilities.mode = Mode::Toolsets;
        let effects = self.capabilities.load();
        self.page_requests(effects);
    }
    pub fn open_capabilities_plugins(&mut self, profile: Option<String>) {
        if self.capabilities.busy() {
            self.settings.error =
                Some("Finish the current capability operation before opening plugins".into());
            return;
        }
        let profile = profile.unwrap_or_else(|| self.profile.clone());
        self.open_route(crate::pages::Route::Capabilities);
        self.invalidate_page_requests("capabilities.");
        let demo = self.capabilities.demo;
        self.capabilities = if demo {
            Capabilities::fixture()
        } else {
            Capabilities::new()
        };
        self.capabilities.profile = Some(profile);
        self.capabilities.mode = Mode::Plugins;
        let effects = self.capabilities.load();
        self.page_requests(effects);
    }
    pub fn open_tool_settings(&mut self, profile: Option<String>, name: String) {
        if self.capabilities.busy() {
            self.settings.error =
                Some("Finish the current capability operation before opening tool settings".into());
            return;
        }
        let profile = profile.unwrap_or_else(|| self.profile.clone());
        self.open_route(crate::pages::Route::Capabilities);
        self.invalidate_page_requests("capabilities.");
        let demo = self.capabilities.demo;
        self.capabilities = if demo {
            Capabilities::fixture()
        } else {
            Capabilities::new()
        };
        self.capabilities.profile = Some(profile);
        self.capabilities.mode = Mode::Toolsets;
        let mut effects = self.capabilities.load();
        effects.extend(self.capabilities.select(Selection::Toolset(name)));
        self.page_requests(effects);
    }
    pub fn open_agent_plugin_settings(&mut self, profile: Option<String>, key: String) {
        if self.capabilities.busy() {
            self.settings.error = Some(
                "Finish the current capability operation before opening plugin settings".into(),
            );
            return;
        }
        let profile = profile.unwrap_or_else(|| self.profile.clone());
        self.open_route(crate::pages::Route::Capabilities);
        self.invalidate_page_requests("capabilities.");
        let demo = self.capabilities.demo;
        self.capabilities = if demo {
            Capabilities::fixture()
        } else {
            Capabilities::new()
        };
        self.capabilities.profile = Some(profile);
        self.capabilities.mode = Mode::Plugins;
        self.capabilities.selection = Some(Selection::Plugin(key.clone()));
        let mut effects = self.capabilities.load();
        if demo {
            effects.extend(self.capabilities.select(Selection::Plugin(key)));
        }
        self.page_requests(effects);
    }
    pub fn capabilities_action(&mut self, action: Action) {
        if let Action::OpenSettings(section) = &action {
            if self.capabilities.busy() {
                return;
            }
            let profile = self.capabilities.profile.clone();
            self.settings_action(crate::pages::settings::Action::Scope(profile.clone()));
            self.open_route(crate::pages::Route::Settings);
            if self.settings.scope_profile == profile {
                self.settings_action(crate::pages::settings::Action::Section(section.clone()));
            }
            return;
        }
        if matches!(&action, Action::Profile(_)) && !self.capabilities.busy() {
            self.invalidate_page_requests("capabilities.");
        }
        let effects = self.capabilities.act(action);
        self.page_requests(effects);
    }
}
fn nonempty_label(value: &Value) -> String {
    let prompt = string(value, "prompt");
    if prompt.is_empty() {
        string(value, "name")
    } else {
        prompt
    }
}
pub fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}
pub fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}
pub fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}
pub fn rows(value: &Value, key: &str) -> Vec<Value> {
    value
        .as_array()
        .or_else(|| value.get(key).and_then(Value::as_array))
        .map(|a| a.iter().take(10000).cloned().collect())
        .unwrap_or_default()
}
pub fn pretty(value: &Value, default: &str) -> String {
    if value.is_null() {
        default.into()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| default.into())
    }
}
pub fn toolset_visible(name: &str) -> bool {
    !matches!(
        name,
        "discord" | "discord_admin" | "yuanbao" | "context_engine" | "moa"
    )
}
pub fn plugin_visible(row: &Value) -> bool {
    let key = string(row, "key");
    if string(row, "source") == "bundled" {
        return matches!(key.as_str(), "disk-cleanup" | "security-guidance");
    }
    !["dashboard_auth/", "model-providers/", "platforms/"]
        .iter()
        .any(|p| key.starts_with(p))
}
pub fn full_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn tool_fields(value: &Value) -> Vec<Field> {
    let mut seen = HashSet::new();
    rows(value, "providers")
        .iter()
        .flat_map(|p| rows(p, "env_vars"))
        .filter(|v| seen.insert(string(v, "key")))
        .map(|v| Field {
            key: string(&v, "key"),
            label: string(&v, "prompt"),
            description: string(&v, "url"),
            kind: "secret".into(),
            input: {
                let mut input = TextEdit::new("");
                input.set_secure(true);
                input
            },
            checked: false,
            choices: vec![],
            has_value: flag(&v, "is_set"),
            initial: String::new(),
        })
        .collect()
}
fn input_initial(field: &Value) -> String {
    match string(field, "type").as_str() {
        "secret" => String::new(),
        "enum" => field
            .get("value")
            .and_then(Value::as_str)
            .or_else(|| field["choices"].as_array()?.first()?.as_str())
            .unwrap_or_default()
            .into(),
        "string" => string(field, "value"),
        _ => pretty(&field["value"], ""),
    }
}
fn plugin_fields(value: &Value) -> Vec<Field> {
    rows(value, "settings_schema")
        .into_iter()
        .map(|v| {
            let kind = string(&v, "type");
            let mut input = TextEdit::new(input_initial(&v));
            if kind == "secret" {
                input.set_secure(true);
            }
            Field {
                key: if kind == "secret" {
                    string(&v, "env")
                } else {
                    string(&v, "key")
                },
                label: string(&v, "label"),
                description: string(&v, "description"),
                kind,
                input,
                checked: flag(&v, "value"),
                choices: strings(&v["choices"]),
                has_value: flag(&v, "has_value"),
                initial: input_initial(&v),
            }
        })
        .collect()
}
pub fn tool_enabled(config: &Value, name: &str) -> bool {
    let tools = &config["tools"];
    if let Some(include) = tools.get("include").and_then(Value::as_array) {
        return include.iter().any(|v| v.as_str() == Some(name));
    }
    !strings(&tools["exclude"]).iter().any(|v| v == name)
}
pub fn toggle_tool(config: &Value, name: &str) -> Value {
    let mut next = config.clone();
    let mut tools = next
        .get("tools")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let key = if tools.get("include").is_some_and(Value::is_array) {
        "include"
    } else {
        "exclude"
    };
    let mut names = strings(tools.get(key).unwrap_or(&Value::Null));
    if names.iter().any(|v| v == name) {
        names.retain(|v| v != name);
    } else {
        names.push(name.into());
    }
    if key == "exclude" && names.is_empty() {
        tools.remove(key);
    } else {
        tools.insert(key.into(), json!(names));
    }
    if tools.is_empty() {
        next.as_object_mut().unwrap().remove("tools");
    } else {
        next["tools"] = Value::Object(tools);
    }
    next
}
pub fn member_revision(policy: &Value) -> String {
    rows(policy, "layers")
        .iter()
        .find(|l| string(l, "kind") == "member")
        .map(|l| string(l, "revision"))
        .unwrap_or_default()
}
pub fn connector_disabled(policy: &Value, slug: &str) -> Vec<String> {
    rows(policy, "layers")
        .iter()
        .find(|l| string(l, "kind") == "member")
        .map(|l| strings(&l["body"]["tools"][slug]))
        .unwrap_or_default()
}
pub fn connector_locked(policy: &Value, slug: &str) -> bool {
    rows(policy, "layers")
        .iter()
        .filter(|l| string(l, "kind") != "member")
        .any(|l| layer_locks(&l["body"], slug))
}
fn layer_locks(body: &Value, slug: &str) -> bool {
    match string(body, "mode").as_str() {
        "deny-all" => true,
        "allow" => !strings(&body["connectors"]).iter().any(|s| s == slug),
        "deny" => strings(&body["disabled_connectors"])
            .iter()
            .any(|s| s == slug),
        _ => false,
    }
}
pub fn connector_tool_locked(policy: &Value, slug: &str, name: &str, tools: &Value) -> bool {
    if connector_locked(policy, slug) {
        return true;
    }
    let hints = rows(tools, "tools")
        .iter()
        .find(|t| string(t, "slug") == name)
        .map(|t| strings(&t["hints"]))
        .unwrap_or_default();
    rows(policy, "layers")
        .iter()
        .filter(|l| string(l, "kind") != "member")
        .any(|l| {
            let body = &l["body"];
            let enable = strings(&body["tags"]["enable"]);
            strings(&body["tools"][slug]).iter().any(|s| s == name)
                || strings(&body["tags"]["disable"])
                    .iter()
                    .any(|s| hints.contains(s))
                || (!enable.is_empty() && !enable.iter().any(|s| hints.contains(s)))
        })
}
fn connection_fields(value: &Value, previous: &[TargetField]) -> Vec<TargetField> {
    rows(value, "targets")
        .into_iter()
        .flat_map(|target| {
            let name = string(&target, "name");
            rows(&target, "required_env")
                .into_iter()
                .map(move |v| (name.clone(), v))
        })
        .map(|(target, v)| {
            let name = string(&v, "name");
            let secret = flag(&v, "secret");
            let text = previous
                .iter()
                .find(|f| f.target == target && f.name == name)
                .map(|f| f.input.text.clone())
                .unwrap_or_else(|| {
                    if secret {
                        String::new()
                    } else {
                        string(&v, "default")
                    }
                });
            let mut input = TextEdit::new(text);
            if secret {
                input.set_secure(true);
            }
            TargetField {
                target,
                name,
                prompt: string(&v, "prompt"),
                required: flag(&v, "required"),
                secret,
                input,
            }
        })
        .collect()
}

pub fn scan_text(value: &Value) -> String {
    rows(value, "scan_findings")
        .iter()
        .map(|v| {
            format!(
                "{}  {}:{}  {}",
                string(v, "severity"),
                string(v, "file"),
                v["line"],
                string(v, "description")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
