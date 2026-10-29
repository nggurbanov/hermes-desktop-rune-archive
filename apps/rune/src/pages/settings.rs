//! Schema-driven Settings, with a dirty draft and owner-fenced effects supplied by Pages.
pub mod billing;
pub mod delegation;
mod fallback;
pub mod local_models;
pub mod memory;
pub mod moa;
pub mod models;
pub mod plugins;
pub mod providers;
pub mod vault;
pub mod web_access;
pub use fallback::FallbackRow;

use crate::{
    app::Viewer,
    pages::{PageEffect, PageRequest},
};
use rune::TextEdit;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub description: String,
    pub section: String,
    pub kind: String,
    pub options: Vec<String>,
    pub free_input: bool,
    pub editor: TextEdit,
}
#[derive(Clone)]
pub struct EnvField {
    pub key: String,
    pub description: String,
    pub category: String,
    pub is_set: bool,
    pub password: bool,
    pub editor: TextEdit,
    pub revealed: Option<String>,
}
pub struct Settings {
    pub fields: Vec<Field>,
    pub fallback_rows: Vec<FallbackRow>,
    pub providers: providers::Providers,
    pub models: models::Models,
    pub moa: moa::Moa,
    pub memory: memory::Memory,
    pub vault: vault::Vault,
    pub plugins: plugins::Plugins,
    pub web: web_access::WebAccess,
    pub billing: billing::Billing,
    pub local_models: local_models::LocalModels,
    pub delegation: delegation::Delegation,
    pub voice_catalog: BTreeMap<String, String>,
    pub voice_loading: bool,
    pub section: String,
    pub scope_profile: Option<String>,
    pub search: TextEdit,
    pub open_select: Option<String>,
    pub loading: bool,
    pub saving: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub env: Vec<EnvField>,
    pub env_loading: bool,
    pub env_saving: Option<String>,
    pub env_key: TextEdit,
    pub env_value: TextEdit,
    pub confirm: Option<String>,
    pub demo: bool,
    detached: TextEdit,
    baseline: Value,
    config_ready: bool,
    schema: Value,
    sent: Option<Value>,
    revision: u64,
    clock: f64,
    edited_at: Option<f64>,
}
impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}
impl Settings {
    pub fn new() -> Self {
        Self {
            fields: vec![],
            fallback_rows: vec![],
            providers: providers::Providers::new(),
            models: models::Models::new(),
            moa: moa::Moa::new(),
            memory: memory::Memory::new(),
            vault: vault::Vault::new(),
            plugins: plugins::Plugins::new(),
            web: web_access::WebAccess::new(),
            billing: billing::Billing::new(),
            local_models: local_models::LocalModels::new(),
            delegation: delegation::Delegation::new(),
            voice_catalog: BTreeMap::new(),
            voice_loading: false,
            section: "model".into(),
            scope_profile: None,
            search: TextEdit::new(""),
            open_select: None,
            loading: false,
            saving: false,
            error: None,
            notice: None,
            env: vec![],
            env_loading: false,
            env_saving: None,
            env_key: TextEdit::new(""),
            env_value: TextEdit::new(""),
            confirm: None,
            demo: false,
            detached: TextEdit::new(""),
            baseline: json!({}),
            config_ready: false,
            schema: json!({}),
            sent: None,
            revision: 0,
            clock: 0.0,
            edited_at: None,
        }
    }
    pub fn blocks_owner_change(&self) -> bool {
        self.saving
            || self.dirty_count() > 0
            || self.env_saving.is_some()
            || !self.env_value.text.is_empty()
            || self.env.iter().any(|f| !f.editor.text.is_empty())
            || self.providers.busy
            || self.providers.login.is_some()
            || self.providers.dirty()
            || self.models.busy
            || self.models.dirty()
            || self.moa.saving
            || self.moa.dirty
            || self.memory.saving
            || self.memory.pending
            || self.memory.dirty()
            || self.vault.busy
            || self.vault.has_draft()
            || self.web.busy
            || self.web.waiting
            || self.web.dirty()
            || self.billing.blocked()
            || self.billing.auto_dirty()
            || self.local_models.busy
            || self.delegation.busy
            || self.delegation.dirty()
    }
    fn pin(&self, effects: Vec<PageEffect>) -> Vec<PageEffect> {
        effects
            .into_iter()
            .map(|(op, request)| {
                (
                    op,
                    if request.profile.is_some() {
                        request
                    } else if let Some(profile) = &self.scope_profile {
                        request.profile(profile)
                    } else {
                        request
                    },
                )
            })
            .collect()
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        let e = self.load_unscoped();
        self.pin(e)
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let e = self.apply_unscoped(op, result);
        self.pin(e)
    }
    pub fn save(&mut self, confirmed: bool) -> Vec<PageEffect> {
        let e = self.save_unscoped(confirmed);
        self.pin(e)
    }
    pub fn poll(&mut self, now: f64) -> Vec<PageEffect> {
        let e = self.poll_unscoped(now);
        self.pin(e)
    }
    pub fn env_load(&mut self) -> Vec<PageEffect> {
        let e = self.env_load_unscoped();
        self.pin(e)
    }
    pub fn env_write(&mut self, key: String, delete: bool) -> Vec<PageEffect> {
        let e = self.env_write_unscoped(key, delete);
        self.pin(e)
    }
    pub fn cancel_owned(&mut self) -> Vec<PageEffect> {
        let e = if self.providers.login.is_some() {
            self.providers.act(providers::Action::Cancel)
        } else {
            vec![]
        };
        self.pin(e)
    }
    fn load_unscoped(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        self.error = None;
        let mut effects = self.models.load();
        effects.extend(self.moa.load());
        effects.extend(self.voice_load());
        effects.extend(vec![
            ("settings.config".into(), PageRequest::get("/api/config")),
            (
                "settings.schema".into(),
                PageRequest::get("/api/config/schema"),
            ),
        ]);
        effects
    }
    fn voice_load(&mut self) -> Vec<PageEffect> {
        if self.voice_loading {
            return vec![];
        }
        self.voice_loading = true;
        vec![(
            "settings.voices".into(),
            PageRequest::get("/api/audio/elevenlabs/voices"),
        )]
    }
    fn apply_unscoped(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        if op == "settings.voices" {
            self.voice_loading = false;
            self.voice_catalog.clear();
            if let Ok(v) = result {
                if v["available"].as_bool() == Some(true) {
                    for voice in v["voices"].as_array().into_iter().flatten() {
                        if let Some(id) = voice["voice_id"].as_str().filter(|id| !id.is_empty()) {
                            self.voice_catalog
                                .insert(id.into(), voice["label"].as_str().unwrap_or(id).into());
                        }
                    }
                }
            }
            return vec![];
        }
        if op.starts_with("settings.delegation.") {
            return self.delegation.apply(op, result);
        }
        if op.starts_with("settings.local.") {
            return self.local_models.apply(op, result);
        }
        if op.starts_with("settings.billing.") {
            return self.billing.apply(op, result);
        }
        if op.starts_with("settings.web.") {
            return self.web.apply(op, result);
        }
        if op == "settings.plugins.list" {
            return self.plugins.apply(result);
        }
        if op.starts_with("settings.vault.") {
            return self.vault.apply(op, result);
        }
        if op.starts_with("settings.memory.") {
            return self.memory.apply(op, result);
        }
        if op.starts_with("settings.moa.") {
            return self.moa.apply(op, result);
        }
        if op.starts_with("settings.models.") {
            return self.models.apply(op, result);
        }
        if op.starts_with("settings.providers.") {
            return self.providers.apply(op, result);
        }
        match result {
            Err(error) => {
                match op {
                    "settings.save" => self.saving = false,
                    "settings.config" | "settings.schema" => self.loading = false,
                    "settings.env" => self.env_loading = false,
                    op if op.starts_with("settings.env-") => self.env_saving = None,
                    _ => {}
                }
                self.error = Some(error);
                vec![]
            }
            Ok(value) => match op {
                "settings.config" | "settings.readback" => {
                    self.loading = false;
                    if !value.is_object() {
                        self.error = Some("Invalid settings response".into());
                        return vec![];
                    }
                    let dirty = self.edits();
                    self.baseline = value;
                    self.config_ready = true;
                    self.rebuild();
                    self.restore_edits(dirty);
                    vec![]
                }
                "settings.schema" => {
                    let dirty = self.edits();
                    self.schema = value;
                    self.rebuild();
                    self.restore_edits(dirty);
                    vec![]
                }
                "settings.save" => {
                    self.saving = false;
                    if value.get("ok").and_then(Value::as_bool) != Some(true) {
                        self.error = Some("The agent did not confirm the settings save".into());
                        return vec![];
                    }
                    if let Some(patch) = self.sent.take() {
                        merge(&mut self.baseline, &patch);
                    }
                    self.notice = Some("Saved".into());
                    self.error = None;
                    vec![("settings.readback".into(), PageRequest::get("/api/config"))]
                }
                "settings.env" => {
                    self.env_loading = false;
                    self.env = parse_env(&value);
                    vec![]
                }
                "settings.env-save" | "settings.env-delete" => {
                    self.env_saving = None;
                    self.env_value.replace_text("");
                    if value.get("ok").and_then(Value::as_bool) != Some(true) {
                        self.error = Some("The agent did not confirm the credential change".into());
                        return vec![];
                    }
                    self.env_load()
                }
                "settings.env-reveal" => {
                    self.env_saving = None;
                    if let (Some(key), Some(secret)) = (
                        value.get("key").and_then(Value::as_str),
                        value.get("value").and_then(Value::as_str),
                    ) {
                        if let Some(field) = self.env.iter_mut().find(|f| f.key == key) {
                            field.revealed = Some(secret.into());
                        }
                    }
                    vec![]
                }
                "settings.defaults" => {
                    if !value.is_object() {
                        self.error = Some("Invalid defaults response".into());
                        return vec![];
                    }
                    for field in &mut self.fields {
                        field
                            .editor
                            .replace_text(display(nested(&value, &field.key), &field.kind));
                    }
                    self.changed();
                    vec![]
                }
                _ => vec![],
            },
        }
    }
    fn rebuild(&mut self) {
        let meta: Value = serde_json::from_str(include_str!("settings/metadata.json"))
            .expect("retained settings metadata");
        let mut schemas = self
            .schema
            .get("fields")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for key in ["desktop.font_family", "terminal.font_family"] {
            schemas.entry(key.to_owned()).or_insert_with(||json!({"type":"string","description":"Font family assigned to this agent profile."}));
        }
        let mut flat = BTreeMap::new();
        flatten(&self.baseline, "", &mut flat);
        for (key, value) in flat {
            schemas
                .entry(key)
                .or_insert_with(|| json!({"type":kind(&value)}));
        }
        self.fields = schemas
            .into_iter()
            .filter(|(key, _)| safe_path(key))
            .map(|(key, schema)| {
                let value = nested(&self.baseline, &key);
                let kind = schema
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| kind(value))
                    .to_owned();
                let kind = if key == "fallback_providers" {
                    "structured_list".to_owned()
                } else {
                    kind
                };
                let label = meta["FIELD_LABELS"][&key]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| pretty(key.rsplit('.').next().unwrap_or(&key)));
                let description = meta["FIELD_DESCRIPTIONS"][&key]
                    .as_str()
                    .or_else(|| schema.get("description").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_owned();
                let section =
                    if matches!(key.as_str(), "desktop.font_family" | "terminal.font_family") {
                        "appearance".into()
                    } else {
                        section_for(&key, &schema, &meta)
                    };
                let options = meta["ENUM_OPTIONS"]
                    .get(&key)
                    .or_else(|| schema.get("options"))
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|v| {
                                v.as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| v.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let free_input = meta["FREE_INPUT_KEYS"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(&key)));
                let editor = TextEdit::new(display(value, &kind));
                Field {
                    key,
                    label,
                    description,
                    section,
                    kind,
                    options,
                    free_input,
                    editor,
                }
            })
            .collect();
        self.refresh_fallback();
    }
    fn edits(&self) -> BTreeMap<String, TextEdit> {
        self.fields
            .iter()
            .filter(|f| f.editor.text != display(nested(&self.baseline, &f.key), &f.kind))
            .map(|f| (f.key.clone(), f.editor.clone()))
            .collect()
    }
    fn restore_edits(&mut self, edits: BTreeMap<String, TextEdit>) {
        for f in &mut self.fields {
            if let Some(edit) = edits.get(&f.key) {
                f.editor = edit.clone();
            }
        }
        self.refresh_fallback();
    }
    pub fn edit_mut(&mut self, key: &str) -> &mut TextEdit {
        self.fields
            .iter_mut()
            .find(|f| f.key == key)
            .map(|f| &mut f.editor)
            .unwrap_or(&mut self.detached)
    }
    pub fn env_edit_mut(&mut self, key: &str) -> &mut TextEdit {
        self.env
            .iter_mut()
            .find(|f| f.key == key)
            .map(|f| &mut f.editor)
            .unwrap_or(&mut self.detached)
    }
    pub fn changed(&mut self) {
        self.revision += 1;
        self.edited_at = Some(self.clock);
        self.error = None;
        self.notice = None;
    }
    pub fn set_value(&mut self, key: &str, value: Value, changed: bool) {
        if let Some(field) = self.fields.iter_mut().find(|f| f.key == key) {
            field.editor.replace_text(display(&value, &field.kind));
            if changed {
                self.changed();
            }
        }
        self.open_select = None;
    }
    pub fn changed_values(&self) -> Result<BTreeMap<String, Value>, String> {
        let mut values = BTreeMap::new();
        for field in &self.fields {
            if field.editor.text == display(nested(&self.baseline, &field.key), &field.kind) {
                continue;
            }
            let value = parse(
                &field.editor.text,
                &field.kind,
                nested(&self.baseline, &field.key),
            )
            .map_err(|e| format!("{}: {e}", field.label))?;
            if value != *nested(&self.baseline, &field.key) {
                values.insert(field.key.clone(), value);
            }
        }
        Ok(values)
    }
    pub fn dirty_count(&self) -> usize {
        self.fields
            .iter()
            .filter(|f| display(nested(&self.baseline, &f.key), &f.kind) != f.editor.text)
            .count()
    }
    fn save_unscoped(&mut self, confirmed: bool) -> Vec<PageEffect> {
        if self.saving || self.demo {
            return vec![];
        }
        let values = match self.changed_values() {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e);
                return vec![];
            }
        };
        if values.is_empty() {
            return vec![];
        }
        if !confirmed
            && values
                .get("toolsets")
                .is_some_and(|v| v.as_array().is_some_and(Vec::is_empty))
            && nested(&self.baseline, "toolsets")
                .as_array()
                .is_some_and(|v| !v.is_empty())
        {
            self.confirm = Some("toolsets".into());
            return vec![];
        }
        let mut patch = json!({});
        for (key, value) in values {
            set_nested(&mut patch, &key, value);
        }
        self.saving = true;
        self.sent = Some(patch.clone());
        self.edited_at = None;
        vec![(
            "settings.save".into(),
            PageRequest::put("/api/config", json!({"config":patch})),
        )]
    }
    fn poll_unscoped(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        let mut effects = if self.demo {
            vec![]
        } else {
            {
                let mut e = self.providers.poll(now);
                e.extend(self.memory.poll(now));
                e.extend(self.web.poll(now, self.section == "web-access"));
                e.extend(self.billing.poll(now));
                e.extend(self.local_models.poll(now, self.section == "local-models"));
                e
            }
        };
        if self.edited_at.is_some_and(|t| now - t >= 0.55)
            && !self.saving
            && self.error.is_none()
            && self.confirm.is_none()
        {
            effects.extend(self.save(false));
        }
        effects
    }
    pub fn wake_after(&self) -> Option<std::time::Duration> {
        if self.demo {
            return None;
        }
        let save = self
            .edited_at
            .filter(|_| !self.saving && self.error.is_none() && self.confirm.is_none())
            .map(|at| std::time::Duration::from_secs_f64((0.56 - (self.clock - at)).max(0.001)));
        let provider = self
            .providers
            .next_wake()
            .into_iter()
            .chain(self.memory.next_wake())
            .chain(self.web.next_wake(self.section == "web-access"))
            .chain(self.billing.next_wake())
            .chain(self.local_models.next_wake(self.section == "local-models"))
            .min();
        match (save, provider) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    pub fn config_ready(&self) -> bool {
        self.config_ready
    }
    pub fn confirmed_config(&self) -> &Value {
        &self.baseline
    }
    pub fn export_value(&self) -> Result<Value, String> {
        let mut value = self.baseline.clone();
        for (key, field) in self.changed_values()? {
            set_nested(&mut value, &key, field);
        }
        Ok(value)
    }
    pub fn import_value(&mut self, value: Value) -> Result<(), String> {
        if !value.is_object() {
            return Err("The settings file must contain a JSON object".into());
        }
        let current = self.edits();
        let mut imported = BTreeMap::new();
        flatten(&value, "", &mut imported);
        if imported.keys().any(|k| !safe_path(k)) {
            return Err("The settings file contains an invalid field path".into());
        }
        if !self.schema.get("fields").is_some_and(Value::is_object) {
            self.schema["fields"] = json!({});
        }
        for (key, value) in &imported {
            if self.schema["fields"].get(key).is_none() {
                self.schema["fields"][key] = json!({"type":kind(value)});
            }
        }
        self.rebuild();
        self.restore_edits(current);
        for (key, value) in imported {
            self.set_value(&key, value, false);
        }
        self.refresh_fallback();
        self.changed();
        Ok(())
    }
    pub fn discard(&mut self) {
        for f in &mut self.fields {
            f.editor
                .replace_text(display(nested(&self.baseline, &f.key), &f.kind));
        }
        self.edited_at = None;
        self.error = None;
        self.confirm = None;
        self.refresh_fallback();
    }
    fn env_load_unscoped(&mut self) -> Vec<PageEffect> {
        self.env_loading = true;
        vec![("settings.env".into(), PageRequest::get("/api/env"))]
    }
    fn env_write_unscoped(&mut self, key: String, delete: bool) -> Vec<PageEffect> {
        if self.demo || self.env_saving.is_some() {
            return vec![];
        }
        if key.trim().is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            self.error = Some("Use a valid environment variable name".into());
            return vec![];
        }
        let value = if self.env_key.text == key {
            self.env_value.text.clone()
        } else {
            self.env
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.editor.text.clone())
                .unwrap_or_default()
        };
        self.env_saving = Some(key.clone());
        self.error = None;
        if delete {
            vec![(
                "settings.env-delete".into(),
                PageRequest::delete("/api/env", json!({"key":key})),
            )]
        } else {
            vec![(
                "settings.env-save".into(),
                PageRequest::put("/api/env", json!({"key":key,"value":value})),
            )]
        }
    }
    pub fn visible_fields(&self) -> Vec<&Field> {
        let query = self.search.text.to_lowercase();
        self.fields
            .iter()
            .filter(|f| {
                (query.is_empty() && f.section == self.section
                    || !query.is_empty()
                        && format!("{} {} {}", f.key, f.label, f.description)
                            .to_lowercase()
                            .contains(&query))
                    && self.voice_visible(&f.key)
                    && !models::assignment_key(&f.key)
            })
            .collect()
    }
    fn voice_visible(&self, key: &str) -> bool {
        let parts: Vec<_> = key.split('.').collect();
        if parts.len() < 3 || !matches!(parts[0], "tts" | "stt") {
            return true;
        }
        if parts[0] == "stt" && self.value("stt.enabled") == json!(false) {
            return false;
        }
        let selected = self.value(&format!("{}.provider", parts[0]));
        let provider = selected
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(if parts[0] == "tts" { "edge" } else { "local" });
        parts[1] == provider
    }
    pub fn value(&self, key: &str) -> Value {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .and_then(|f| parse(&f.editor.text, &f.kind, nested(&self.baseline, key)).ok())
            .unwrap_or_else(|| nested(&self.baseline, key).clone())
    }
    pub fn sections(&self) -> Vec<(String, String)> {
        let mut result = vec![
            ("model".into(), "Model".into()),
            ("local-models".into(), "Local models".into()),
            ("delegation".into(), "Subagent routing".into()),
            ("chat".into(), "Chat".into()),
            ("appearance".into(), "Appearance".into()),
            ("connections".into(), "Connections".into()),
            ("workspace".into(), "Workspace".into()),
            ("safety".into(), "Safety".into()),
            ("browser".into(), "Browser".into()),
            ("memory".into(), "Memory & Context".into()),
            ("voice".into(), "Voice".into()),
            ("advanced".into(), "Advanced".into()),
        ];
        for f in &self.fields {
            if !result.iter().any(|(id, _)| *id == f.section) {
                result.push((f.section.clone(), pretty(&f.section)));
            }
        }
        result.push(("providers".into(), "Providers & endpoints".into()));
        result.push(("keys".into(), "API keys & environment".into()));
        result.push(("vault".into(), "Credential vault".into()));
        result.push(("plugins".into(), "Plugins".into()));
        result.push(("web-access".into(), "Web access".into()));
        result.push(("billing".into(), "Billing & subscription".into()));
        result.push(("shortcuts".into(), "Keyboard shortcuts".into()));
        result.push(("about".into(), "About Hermes".into()));
        result
    }
    pub fn fixture(&mut self) {
        self.demo = true;
        self.schema = json!({"fields":{"model":{"type":"string","category":"model"},"agent.max_turns":{"type":"number"},"approvals.mode":{"type":"select"},"memory.memory_enabled":{"type":"boolean"},"memory.provider":{"type":"select","options":["builtin","sample-memory"]},"timezone":{"type":"string"},"voice.voice_chat_mode":{"type":"select"},"stt.enabled":{"type":"boolean"},"stt.provider":{"type":"select"},"tts.provider":{"type":"select"}}});
        self.baseline = json!({"model":"sample-model","agent":{"max_turns":90},"approvals":{"mode":"smart"},"memory":{"memory_enabled":true,"provider":"builtin"},"timezone":"Europe/London","voice":{"voice_chat_mode":"chained"},"stt":{"enabled":true,"provider":"local"},"tts":{"provider":"edge"}});
        self.rebuild();
        self.providers.apply("settings.providers.accounts",Ok(json!({"providers":[{"id":"sample","name":"Sample provider","flow":"pkce","disconnectable":true,"status":{"logged_in":false}}]})));
        self.providers.apply("settings.providers.endpoints",Ok(json!({"endpoints":[{"id":"sample-endpoint","name":"Personal endpoint","base_url":"https://example.invalid/v1","model":"sample-model","has_api_key":true,"discover_models":true,"is_current":true}]})));
        self.moa.apply("settings.moa.load", Ok(json!({"default_preset":"balanced","active_preset":"","presets":{"balanced":{"enabled":true,"reference_models":[{"provider":"sample","model":"sample-model","enabled":true}],"aggregator":{"provider":"sample","model":"sample-model"},"reference_temperature":0.7,"aggregator_temperature":0.4,"reference_timeout":null,"degraded_reference_policy":"loud","fanout":"user_turn"}}})));
        self.models.apply("settings.models.options", Ok(json!({"providers":[{"slug":"sample","name":"Sample provider","models":["sample-model","sample-fast"],"authenticated":true}]})));
        self.models.apply("settings.models.auxiliary", Ok(json!({"main":{"provider":"sample","model":"sample-model"},"tasks":[{"task":"compression","label":"Context compression","hint":"Summarizes long conversations.","provider":"auto","model":"","reasoning_effort":null}]})));
        self.memory.apply("settings.memory.status",Ok(json!({"active":"sample-memory","builtin_files":{"memory":1024,"user":320},"providers":[{"name":"sample-memory","description":"Synthetic memory provider","status":"ready"}]})));
        self.memory.apply("settings.memory.config",Ok(json!({"name":"sample-memory","label":"Sample memory","docs_url":"https://example.invalid/memory","fields":[{"key":"sample_url","label":"Memory endpoint","description":"External memory service endpoint","group":"Connection","kind":"text","value":"https://example.invalid","is_set":true},{"key":"sample_key","label":"API key","kind":"secret","is_set":true,"value":""}]})));
        self.vault.apply("settings.vault.items",Ok(json!({"items":[{"id":"sample","kind":"login","label":"Example account","origin":"https://example.invalid","identifier":"sample@example.invalid","created_at":"2026-01-01","backend":"local","has_otp":true}]})));
        self.vault.apply("settings.vault.sources",Ok(json!({"sources":[{"name":"bitwarden","display_name":"Bitwarden","installed":true,"enabled":true,"unlocked":false,"needs_unlock":true}]})));
        self.plugins.apply(Ok(json!({"plugins":[{"key":"sample-notes","name":"Notes","version":"1.0","description":"Synthetic plugin settings","settings_schema":[{"key":"daily","type":"boolean","value":true}]}]})));
        self.web.apply("settings.web.status",Ok(json!({"enabled":false,"mode":"lan","public_url":"","port":8080,"lan_address":"192.0.2.1","nous_signed_in":true,"gateway":{"state":"off","url":null,"reason":null}})));
        self.billing.apply("settings.billing.state",Ok(json!({"ok":true,"logged_in":true,"org_name":"Sample account","role":"owner","balance_display":"$25","balance_usd":"25","portal_url":"https://example.invalid/billing","can_charge":true,"cli_billing_enabled":true,"charge_presets":["10","25","50"],"min_usd":"5","max_usd":"100","auto_reload":{"enabled":false},"keep_going_on_credits":false,"payment_method":{"kind":"card","brand":"Visa","last4":"0000"}})));
        self.billing.apply("settings.billing.subscription",Ok(json!({"ok":true,"logged_in":true,"can_change_plan":true,"context":"personal","current":{"tier_id":"sample","tier_name":"Sample plan","cycle_ends_at":"2026-11-01"},"tiers":[{"tier_id":"sample","name":"Sample plan","is_current":true,"is_enabled":true,"dollars_per_month_display":"$10"},{"tier_id":"sample-plus","name":"Sample Plus","is_enabled":true,"dollars_per_month_display":"$20"}]})));
        self.local_models.profile = "default".into();
        self.local_models.apply("settings.local.status",Ok(json!({"enabled":true,"runtime_installed":true,"runtime_backend":"cpu","tag":"sample-runtime","configured_tag":"sample-runtime","server_running":false,"models":[{"id":"sample-model","size_label":"2 GB"}],"loaded_models":{},"models_dir":"/sample/models"})));
        self.local_models.apply("settings.local.hardware",Ok(json!({"gpu_name":"Sample GPU","vram_label":"8 GB","uma":false,"gpu_util_percent":0})));
        self.local_models.apply("settings.local.catalog",Ok(json!({"models":[{"id":"sample-model","display_name":"Sample local model","description":"Synthetic local model catalog entry","recommended":true,"fits":true,"downloaded":true,"size_label":"2 GB","native_context_label":"32K","fit_summary":"Fits in GPU memory"}]})));
        self.delegation.apply("settings.delegation.load",Ok(json!({"configured":"model_map","blocked_reason":null,"backends":[{"name":"default","label":"Default","description":"Inherit the main model","available":true},{"name":"model_map","label":"Named model routes","description":"Assign specific model routes to subagents","available":true}],"model_map":{"options":[{"name":"fast","description":"Fast routine work","provider":"sample","model":"sample-fast"}],"default":"fast"},"profile_routes":{"profiles":[],"default":null},"profiles":["default","sample-bot"]})));
        self.env = parse_env(
            &json!({"SAMPLE_API_KEY":{"is_set":true,"is_password":true,"category":"Model providers","description":"A synthetic saved credential. Its value stays on the agent."}}),
        );
        let args = rune::extra_args();
        if let Some(section) = args
            .iter()
            .position(|a| a == "--settings-section")
            .and_then(|i| args.get(i + 1))
        {
            if self.sections().iter().any(|(id, _)| id == section) {
                self.section = section.clone();
            }
        }
        if let Some(dialog) = args
            .iter()
            .position(|a| a == "--settings-dialog")
            .and_then(|i| args.get(i + 1))
        {
            match dialog.as_str() {
                "vault-add" => {
                    self.section = "vault".into();
                    self.vault.act(vault::Action::Add);
                    self.vault.label.replace_text("Example account");
                    self.vault.origin.replace_text("https://example.invalid");
                }
                "vault-unlock" => {
                    self.section = "vault".into();
                    self.vault.act(vault::Action::Unlock("bitwarden".into()));
                }
                "provider-oauth" => {
                    self.section = "providers".into();
                    self.providers
                        .act(providers::Action::Start("sample".into()));
                    self.providers.apply("settings.providers.start",Ok(json!({"session_id":"sample-session","verification_url":"https://example.invalid/device","user_code":"ABCD-EFGH","poll_interval":3,"expires_in":600})));
                }
                "billing-quote" => {
                    self.section = "billing".into();
                    self.billing.preview_tier = "sample".into();
                    self.billing.preview = Some(
                        json!({"ok":true,"effect":"scheduled","target_tier_name":"Sample plan","effective_at":"2026-11-01"}),
                    );
                }
                _ => {}
            }
        }
    }
}

pub enum Action {
    Load,
    Save,
    Edited,
    Discard,
    Section(String),
    Scope(Option<String>),
    Value(String, Value),
    Confirm(String),
    CancelConfirm,
    Defaults,
    EnvLoad,
    EnvSave(String),
    EnvDelete(String),
    EnvReveal(String),
    EnvHide(String),
    FallbackAdd,
    FallbackRemove(usize),
    FallbackEdited,
    Provider(providers::Action),
    Model(models::Action),
    Moa(moa::Action),
    Memory(memory::Action),
    Vault(vault::Action),
    Web(web_access::Action),
    Billing(billing::Action),
    LocalModels(local_models::Action),
    Delegation(delegation::Action),
    PluginsManager,
    Tool(String),
    PluginLoad,
    VoicesLoad,
    PluginOpen(String),
}
impl Viewer {
    pub fn settings_action(&mut self, action: Action) {
        if matches!(&action,Action::Section(section) if section!="shortcuts") {
            self.client_settings.capture = None;
        }
        let effects = match action {
            Action::Load => self.settings.load(),
            Action::VoicesLoad => self.settings.voice_load(),
            Action::Save => self.settings.save(false),
            Action::Edited => {
                self.settings.changed();
                vec![]
            }
            Action::Discard => {
                self.settings.discard();
                vec![]
            }
            Action::Scope(profile) => {
                if self.settings.scope_profile == profile {
                    return;
                }
                if self.settings.blocks_owner_change() {
                    self.settings.error=Some("Save or revert changes and finish sign-in before switching the settings profile".into());
                    vec![]
                } else {
                    self.invalidate_page_requests("settings.");
                    let demo = self.settings.demo;
                    let section = self.settings.section.clone();
                    self.settings = Settings::new();
                    self.settings.section = section;
                    self.settings.scope_profile = profile;
                    if demo {
                        self.settings.fixture();
                        vec![]
                    } else {
                        let mut effects = self.settings.load();
                        if self.settings.section == "providers" {
                            effects.extend(self.settings.providers.load());
                        }
                        if self.settings.section == "delegation" {
                            effects.extend(self.settings.delegation.load());
                        }
                        if self.settings.section == "local-models" {
                            effects.extend(self.settings.local_models.load(&self.profile));
                        }
                        if self.settings.section == "billing" {
                            effects.extend(self.settings.billing.load());
                        }
                        if self.settings.section == "web-access" && self.is_local_connection() {
                            effects.extend(self.settings.web.load());
                        }
                        if self.settings.section == "plugins" {
                            effects.extend(self.settings.plugins.load());
                        }
                        if self.settings.section == "vault" {
                            effects.extend(self.settings.vault.load());
                        }
                        if self.settings.section == "memory" {
                            effects.extend(self.settings.memory.load());
                        }
                        if self.settings.section == "keys" {
                            effects.extend(self.settings.env_load());
                        }
                        effects
                    }
                }
            }
            Action::Section(section) => {
                self.settings
                    .local_models
                    .observe_quickstarts(section == "local-models");
                if section != self.settings.section {
                    self.settings.vault.dismiss();
                }
                self.settings.section = section.clone();
                self.settings.search.replace_text("");
                if self.settings.demo {
                    vec![]
                } else if section == "providers" {
                    self.settings.providers.load()
                } else if section == "delegation" {
                    self.settings.delegation.load()
                } else if section == "local-models" {
                    self.settings.local_models.load(&self.profile)
                } else if section == "billing" {
                    self.settings.billing.load()
                } else if section == "web-access" && self.is_local_connection() {
                    self.settings.web.load()
                } else if section == "plugins" {
                    self.settings.plugins.load()
                } else if section == "vault" {
                    self.settings.vault.load()
                } else if section == "memory" {
                    self.settings.memory.load()
                } else if section == "model" {
                    self.settings.models.load()
                } else if section == "keys" && self.settings.env.is_empty() {
                    self.settings.env_load()
                } else {
                    vec![]
                }
            }
            Action::Value(key, value) => {
                self.settings.set_value(&key, value, true);
                vec![]
            }
            Action::Confirm(kind) => {
                self.settings.confirm = None;
                match kind.as_str() {
                    "toolsets" => self.settings.save(true),
                    "defaults" => vec![(
                        "settings.defaults".into(),
                        PageRequest::get("/api/config/defaults"),
                    )],
                    _ if kind.starts_with("env:") => {
                        self.settings.env_write(kind[4..].into(), true)
                    }
                    _ => vec![],
                }
            }
            Action::CancelConfirm => {
                if self.settings.confirm.as_deref() == Some("toolsets") {
                    let value = nested(&self.settings.baseline, "toolsets").clone();
                    self.settings.set_value("toolsets", value, false);
                }
                self.settings.confirm = None;
                self.settings.edited_at = None;
                vec![]
            }
            Action::Defaults => {
                self.settings.confirm = Some("defaults".into());
                vec![]
            }
            Action::EnvLoad => self.settings.env_load(),
            Action::EnvSave(key) => self.settings.env_write(key, false),
            Action::EnvDelete(key) => {
                self.settings.confirm = Some(format!("env:{key}"));
                vec![]
            }
            Action::EnvReveal(key) => {
                if self.settings.demo {
                    vec![]
                } else {
                    self.settings.env_saving = Some(key.clone());
                    vec![(
                        "settings.env-reveal".into(),
                        PageRequest::post("/api/env/reveal", json!({"key":key})),
                    )]
                }
            }
            Action::FallbackAdd => {
                self.settings.fallback_rows.push(FallbackRow::empty());
                vec![]
            }
            Action::FallbackRemove(index) => {
                if index < self.settings.fallback_rows.len() {
                    self.settings.fallback_rows.remove(index);
                    self.settings.sync_fallback();
                }
                vec![]
            }
            Action::FallbackEdited => {
                self.settings.sync_fallback();
                vec![]
            }
            Action::Delegation(action) => {
                if self.settings.demo {
                    vec![]
                } else {
                    self.settings.delegation.act(action)
                }
            }
            Action::PluginsManager => {
                self.open_capabilities_plugins(self.settings.scope_profile.clone());
                vec![]
            }
            Action::Tool(name) => {
                self.open_tool_settings(self.settings.scope_profile.clone(), name);
                vec![]
            }
            Action::LocalModels(action) => {
                if self.settings.demo {
                    vec![]
                } else {
                    if self.settings.local_models.profile != self.profile {
                        self.settings.local_models.load(&self.profile)
                    } else {
                        self.settings.local_models.act(action)
                    }
                }
            }
            Action::Billing(action) => {
                if self.settings.demo {
                    vec![]
                } else {
                    if matches!(action, billing::Action::CancelStepUp) {
                        self.invalidate_page_requests("settings.billing.step-up");
                    }
                    self.settings.billing.act(action)
                }
            }
            Action::Web(action) => {
                if self.settings.demo || !self.is_local_connection() {
                    vec![]
                } else {
                    self.settings.web.act(action)
                }
            }
            Action::PluginLoad => {
                if self.settings.demo {
                    vec![]
                } else {
                    self.settings.plugins.load()
                }
            }
            Action::PluginOpen(key) => {
                self.open_agent_plugin_settings(self.settings.scope_profile.clone(), key);
                vec![]
            }
            Action::Vault(action) => {
                if self.settings.demo {
                    vec![]
                } else {
                    self.settings.vault.act(action)
                }
            }
            Action::Memory(action) => {
                if self.settings.demo {
                    vec![]
                } else {
                    match &action {
                        memory::Action::Select(_)
                            if !self.settings.memory.saving
                                && !self.settings.memory.pending
                                && !self.settings.memory.dirty() =>
                        {
                            self.invalidate_page_requests("settings.memory.")
                        }
                        memory::Action::Connect => {
                            self.invalidate_page_requests("settings.memory.oauth")
                        }
                        memory::Action::Cancel => {
                            self.invalidate_page_requests("settings.memory.oauth");
                            self.invalidate_page_requests("settings.memory.start");
                        }
                        _ => {}
                    }
                    self.settings.memory.act(action)
                }
            }
            Action::Moa(action) => {
                if self.settings.demo {
                    vec![]
                } else {
                    self.settings.moa.act(action)
                }
            }
            Action::Model(action) => {
                if self.settings.demo {
                    vec![]
                } else {
                    self.settings.models.act(action)
                }
            }
            Action::Provider(action) => {
                if self.settings.demo {
                    vec![]
                } else {
                    if matches!(action, providers::Action::Cancel) {
                        self.invalidate_page_requests("settings.providers.poll");
                    }
                    self.settings.providers.act(action)
                }
            }
            Action::EnvHide(key) => {
                if let Some(f) = self.settings.env.iter_mut().find(|f| f.key == key) {
                    f.revealed = None;
                }
                vec![]
            }
        };
        let effects = self.settings.pin(effects);
        self.page_requests(effects);
    }
}
fn nested<'a>(value: &'a Value, path: &str) -> &'a Value {
    path.split('.')
        .try_fold(value, |v, k| v.get(k))
        .unwrap_or(&Value::Null)
}
fn safe_path(path: &str) -> bool {
    path.split('.')
        .all(|p| !p.is_empty() && !matches!(p, "__proto__" | "prototype" | "constructor"))
}
fn set_nested(root: &mut Value, path: &str, value: Value) {
    let mut cur = root;
    let parts: Vec<_> = path.split('.').collect();
    for key in &parts[..parts.len() - 1] {
        if !cur.get(*key).is_some_and(Value::is_object) {
            cur[*key] = json!({});
        }
        cur = &mut cur[*key];
    }
    cur[parts[parts.len() - 1]] = value;
}
fn merge(base: &mut Value, patch: &Value) {
    if let Some(fields) = patch.as_object() {
        for (k, v) in fields {
            if v.is_object() {
                if !base.get(k).is_some_and(Value::is_object) {
                    base[k] = json!({});
                }
                merge(&mut base[k], v);
            } else {
                base[k] = v.clone();
            }
        }
    }
}
fn kind(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::Array(_) => "list",
        Value::Object(_) => "object",
        _ => "string",
    }
}
fn display(value: &Value, kind: &str) -> String {
    if value.is_null() {
        return String::new();
    }
    match kind {
        "string" | "text" | "select" => value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string()),
        "list"
            if value
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string)) =>
        {
            value
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        }
        _ => value.to_string(),
    }
}
fn parse(text: &str, kind: &str, baseline: &Value) -> Result<Value, String> {
    match kind {
        "structured_list" => serde_json::from_str::<Value>(text)
            .ok()
            .filter(Value::is_array)
            .ok_or_else(|| "Complete the fallback provider/model pairs".into()),
        "boolean" => text
            .parse::<bool>()
            .map(Value::Bool)
            .map_err(|_| "Choose on or off".into()),
        "number" => {
            if text.trim().is_empty() && baseline.is_null() {
                Ok(Value::Null)
            } else {
                serde_json::from_str::<Value>(text)
                    .ok()
                    .filter(Value::is_number)
                    .ok_or_else(|| "Enter a valid number".into())
            }
        }
        "list"
            if baseline
                .as_array()
                .is_some_and(|a| a.iter().any(|v| !v.is_string())) =>
        {
            serde_json::from_str::<Value>(text)
                .ok()
                .filter(Value::is_array)
                .ok_or_else(|| "Enter a valid list".into())
        }
        "list" => Ok(Value::Array(
            text.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.into()))
                .collect(),
        )),
        "object" => serde_json::from_str::<Value>(text)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| "Enter a valid object".into()),
        _ => Ok(Value::String(text.into())),
    }
}
fn flatten(value: &Value, prefix: &str, out: &mut BTreeMap<String, Value>) {
    if let Some(map) = value.as_object() {
        for (k, v) in map {
            let key = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            if v.is_object() {
                flatten(v, &key, out)
            } else {
                out.insert(key, v.clone());
            }
        }
    }
}
fn pretty(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}
fn section_for(key: &str, schema: &Value, meta: &Value) -> String {
    if let Some(sections) = meta["SECTIONS"].as_array() {
        for section in sections {
            if section["keys"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(key)))
            {
                return section["id"].as_str().unwrap_or("advanced").into();
            }
        }
    }
    if key == "model"
        || key.starts_with("model.")
        || key.starts_with("auxiliary.")
        || key.starts_with("delegation.")
    {
        return "model".into();
    }
    if key.starts_with("stt.")
        || key.starts_with("tts.")
        || key.starts_with("voice.")
        || key.starts_with("wake.")
    {
        return "voice".into();
    }
    schema
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or("advanced")
        .into()
}
fn parse_env(value: &Value) -> Vec<EnvField> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.get("channel_managed").and_then(Value::as_bool) != Some(true))
        .map(|(key, v)| EnvField {
            key: key.clone(),
            description: v["description"].as_str().unwrap_or("").into(),
            category: v["provider_label"]
                .as_str()
                .or_else(|| v["category"].as_str())
                .unwrap_or("Other")
                .into(),
            is_set: v["is_set"].as_bool().unwrap_or(false),
            password: v["is_password"].as_bool().unwrap_or(true),
            editor: TextEdit::new(""),
            revealed: None,
        })
        .collect()
}

#[cfg(test)]
mod tests;

impl Viewer {
    /// Called by the root frame/result loop; leaving the panel cancels its navigation intent.
    pub fn settings_local_navigation(&mut self) {
        let watching =
            self.route == crate::pages::Route::Settings && self.settings.section == "local-models";
        self.settings.local_models.observe_quickstarts(watching);
        if let Some(profile) = self.settings.local_models.take_chat_ready() {
            if !self.settings.demo
                && watching
                && self.profile == profile
                && !self.settings.blocks_owner_change()
            {
                self.new_draft();
            }
        }
    }
}
