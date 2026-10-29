use super::*;
#[derive(Clone)]
pub enum Action {
    Load,
    Select(String),
    New,
    Discard,
    Tab(String),
    CloneFrom(Option<String>),
    Toggle(String, String, bool),
    Save,
    ConfirmModel,
    CancelModel,
    ResetModel,
    Rename,
    SubmitRename,
    Delete,
    ConfirmDelete,
    CancelDialog,
    Duplicate,
    Setup,
    Export,
    Import,
    DownloadArchive(std::path::PathBuf),
    AvatarPath(std::path::PathBuf),
    AvatarClear,
    AvatarGenerate,
    CheckReady,
    BlobLock,
    BlobReroll,
    BlobKind(String),
    OpenChat(String),
    OpenCapabilities(String),
    OpenMcp(String),
    ProviderSetup,
}
impl Bots {
    fn survivor(&self) -> Option<&str> {
        self.roster
            .iter()
            .find(|r| r["is_default"].as_bool() == Some(true) || r["name"] == "default")
            .and_then(|r| r["name"].as_str())
    }
    fn roster_effect(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        let request = PageRequest::rpc("profiles.list", json!({"include_sessions":true}));
        vec![(
            "bots.list".into(),
            if let Some(name) = self.survivor() {
                request.profile(name)
            } else {
                request
            },
        )]
    }
    pub(super) fn detail_effects(&mut self, name: &str) -> Vec<PageEffect> {
        self.detail_loading = true;
        self.catalog_loading = true;
        vec![
            (
                "bots.detail".into(),
                PageRequest::rpc("profiles.describe", json!({"name":name})).profile(name),
            ),
            (
                "bots.mcp".into(),
                PageRequest::rpc("mcp.catalog", json!({})).profile(name),
            ),
            (
                "bots.catalog".into(),
                PageRequest::get("/api/model/options")
                    .query("explicit_only", 1)
                    .profile(name),
            ),
            (
                "bots.avatar".into(),
                PageRequest::rpc("profiles.get_asset", json!({"name":name,"asset":"avatar"}))
                    .profile(name),
            ),
            (
                "bots.avatar-probe".into(),
                PageRequest::rpc("image.generate", json!({"probe":true})).profile(name),
            ),
        ]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                match op {
                    "bots.list" => self.loading = false,
                    "bots.detail" => {
                        self.advanced_supported = false;
                        self.notice=Some("This agent offers the basic profile editor. SOUL and appearance remain available; model setup is in Settings.".into());
                        return self.basic_soul_read();
                    }
                    "bots.soul" => self.detail_loading = false,
                    "bots.catalog" => self.catalog_loading = false,
                    "bots.avatar-probe" => {
                        self.can_generate = false;
                        return vec![];
                    }
                    "bots.avatar" | "bots.mcp" => return vec![],
                    _ => {
                        self.busy = false;
                        self.generating = false;
                    }
                }
                self.error = Some(e);
                if matches!(op, "bots.rename" | "bots.delete") {
                    if let Some(intent) = self.pending_mutation.take() {
                        let name = string(&intent, "name").to_owned();
                        self.changes.push(ProfileChange::Reconcile(name));
                        self.reconcile = Some(intent);
                        return self.roster_effect();
                    }
                }
                return vec![];
            }
        };
        match op {
            "bots.list" => {
                self.loading = false;
                let Some(rows) = valid_roster(&v) else {
                    self.error = Some("The agent returned an invalid bot roster".into());
                    return vec![];
                };
                self.roster = rows;
                self.roster_revision = self.roster_revision.wrapping_add(1);
                self.server_protocol = v["bot_mode_protocol"].as_bool() == Some(true);
                if let Some(intent) = self.reconcile.take() {
                    let old = string(&intent, "name");
                    if intent["kind"] == "delete" && !self.roster.iter().any(|r| r["name"] == old) {
                        self.changes.push(ProfileChange::Deleted(old.into()));
                    } else if intent["kind"] == "rename" {
                        if let Some(row) = self.roster.iter().find(|r| {
                            r["previous_names"]
                                .as_array()
                                .is_some_and(|names| names.iter().any(|n| n == old))
                        }) {
                            self.changes.push(ProfileChange::Renamed {
                                from: old.into(),
                                to: string(row, "name").into(),
                            });
                            self.selected = Some(string(row, "name").into());
                        }
                    }
                }
                if self.creating {
                    return vec![];
                }
                let exists = self
                    .selected
                    .as_ref()
                    .is_some_and(|name| self.roster.iter().any(|r| r["name"] == *name));
                if !exists {
                    self.clear_editor();
                    self.selected = self
                        .roster
                        .iter()
                        .find(|r| r["is_default"].as_bool() == Some(true) || r["name"] == "default")
                        .or(self.roster.first())
                        .and_then(|r| r["name"].as_str())
                        .map(str::to_owned);
                    self.seed_selected_metadata();
                    if let Some(name) = self.selected.clone() {
                        return self.detail_effects(&name);
                    }
                } else if !self.dirty() {
                    self.seed_selected_metadata();
                }
            }
            "bots.detail" => {
                self.detail_loading = false;
                self.advanced_supported = true;
                if !v["model"].is_object() {
                    self.advanced_supported = false;
                    self.notice=Some("The agent did not provide an advanced profile snapshot. Basic SOUL editing remains available.".into());
                    return self.basic_soul_read();
                }
                self.seed_detail(&v);
            }
            "bots.soul" => {
                self.detail_loading = false;
                let Some(content) = v["content"].as_str() else {
                    self.error = Some("The agent returned an invalid SOUL document".into());
                    return vec![];
                };
                if self.soul.text == self.baseline["soul"].as_str().unwrap_or("") {
                    self.soul.replace_text(content);
                }
                self.baseline["soul"] = json!(content);
            }
            "bots.soul-save" => {
                self.busy = false;
                if v["ok"].as_bool() != Some(true) {
                    self.error = Some("The agent did not confirm the SOUL change".into());
                    return vec![];
                }
                if let Some(sent) = self.last_configure.take() {
                    self.baseline["soul"] = sent["soul"].clone();
                    self.soul.replace_text(string(&sent, "soul"));
                }
                self.save_applied = true;
                return self.save_look();
            }
            "bots.create-caps" => {
                if self.creating {
                    Self::seed_caps(&mut self.skills, &v["skills"], false);
                    Self::seed_caps(&mut self.toolsets, &v["toolsets"], false);
                    Self::seed_caps(&mut self.mcp, &v["mcp_servers"], true);
                }
            }
            "bots.mcp" => self.merge_mcp_catalog(&v),
            "bots.catalog" => {
                self.catalog_loading = false;
                if let Some(p) = v["providers"].as_array() {
                    self.catalog = p.clone();
                } else {
                    self.error = Some("The profile model catalog is invalid".into());
                }
            }
            "bots.avatar" => {
                if v["found"].as_bool() == Some(true) {
                    if let Some(data) = v["data"]
                        .as_str()
                        .filter(|d| assets::decode_data(d).is_ok())
                    {
                        self.seed_avatar(Some(data.into()));
                    }
                } else if v["found"].as_bool() == Some(false) {
                    self.seed_avatar(None);
                }
            }
            "bots.avatar-probe" => self.can_generate = v["available"].as_bool() == Some(true),
            "bots.avatar-generate" => {
                self.busy = false;
                self.generating = false;
                if v["success"].as_bool() != Some(true) {
                    self.error = Some(
                        v["error"]
                            .as_str()
                            .unwrap_or("Avatar generation failed")
                            .into(),
                    );
                    return vec![];
                }
                let Some(data) = v["image_data"]
                    .as_str()
                    .filter(|d| assets::decode_data(d).is_ok())
                else {
                    self.error=Some("The agent did not return a portable avatar image. Upload an image to use it across devices.".into());
                    return vec![];
                };
                self.set_avatar(Some(data.into()));
                self.notice = Some("Avatar generated. Save the bot to publish it.".into());
            }
            "bots.create" => {
                self.busy = false;
                let Some(wanted) = self.pending_create.take() else {
                    return vec![];
                };
                let actual = v["name"].as_str().unwrap_or("");
                if v["ok"].as_bool() == Some(false) || actual != string(&wanted, "name") {
                    self.error = Some("The agent did not confirm the requested new profile".into());
                    return self.roster_effect();
                }
                self.selected = Some(actual.into());
                self.creating = false;
                self.name.replace_text(actual);
                self.soul
                    .replace_text(wanted["soul"].as_str().unwrap_or(""));
                self.changes.push(ProfileChange::Created(actual.into()));
                self.created_chat_pending = Some(actual.into());
                self.notice = Some(
                    "Bot profile created. Saving the staged identity and capabilities…".into(),
                );
                self.save_applied = true;
                return self.save_configuration(true);
            }
            "bots.duplicate-avatar" => {
                let Some(intent) = self.pending_mutation.as_mut() else {
                    self.busy = false;
                    return vec![];
                };
                if v["ok"].as_bool() == Some(false) || !v["found"].is_boolean() {
                    self.busy = false;
                    self.error = Some(
                        "The source avatar could not be checked; no duplicate profile was created"
                            .into(),
                    );
                    return vec![];
                }
                if v["found"].as_bool() == Some(true) {
                    let Some(data) = v["data"]
                        .as_str()
                        .filter(|d| assets::decode_data(d).is_ok())
                    else {
                        self.busy = false;
                        self.error = Some(
                            "The source avatar was invalid; no duplicate profile was created"
                                .into(),
                        );
                        return vec![];
                    };
                    intent["avatar"] = json!(data);
                } else {
                    intent["avatar"] = Value::Null;
                }
                let body = json!({"name":intent["new_name"],"clone_from":intent["name"],"description":intent["description"]});
                let profile = self.survivor().unwrap_or("default");
                return vec![(
                    "bots.duplicate".into(),
                    PageRequest::rpc("profiles.create", body).profile(profile),
                )];
            }
            "bots.duplicate" => {
                self.busy = false;
                let Some(intent) = self.pending_mutation.take() else {
                    return vec![];
                };
                let actual = string(&v, "name");
                if v["ok"].as_bool() == Some(false) || actual != string(&intent, "new_name") {
                    self.error = Some("The agent did not confirm the duplicate profile".into());
                    return self.roster_effect();
                }
                self.clear_editor();
                self.selected = Some(actual.into());
                self.changes.push(ProfileChange::Created(actual.into()));
                self.name.replace_text(actual);
                self.notice = Some(format!("Duplicated bot: {actual}. Copying its appearance…"));
                let look = intent["look"].clone();
                self.title.replace_text(string(&look, "title"));
                self.shape
                    .replace_text(look["shape"].as_str().unwrap_or("circle"));
                self.color.replace_text(string(&look, "color"));
                self.set_avatar(intent["avatar"].as_str().map(str::to_owned));
                self.save_applied = true;
                return self.metadata_effect(look);
            }
            "bots.rename" => {
                self.busy = false;
                let Some(intent) = self.pending_mutation.take() else {
                    return vec![];
                };
                let old = string(&intent, "name");
                if v["ok"].as_bool() != Some(true)
                    || old != "default" && v["name"].as_str() != intent["new_name"].as_str()
                {
                    self.error =
                        Some("The profile rename was not confirmed; refreshing the roster".into());
                    self.changes.push(ProfileChange::Reconcile(old.into()));
                    self.reconcile = Some(intent);
                    return self.roster_effect();
                }
                self.dialog = None;
                if old == "default" {
                    self.changes.push(ProfileChange::Updated("default".into()));
                } else {
                    let actual = string(&v, "name");
                    self.selected = Some(actual.into());
                    self.name.replace_text(actual);
                    self.changes.push(ProfileChange::Renamed {
                        from: old.into(),
                        to: actual.into(),
                    });
                }
                self.notice = Some("Profile renamed".into());
                return self.roster_effect();
            }
            "bots.delete" => {
                self.busy = false;
                let Some(intent) = self.pending_mutation.take() else {
                    return vec![];
                };
                let name = string(&intent, "name");
                if v["ok"].as_bool() != Some(true) {
                    self.error =
                        Some("Profile deletion was not confirmed; refreshing the roster".into());
                    self.changes.push(ProfileChange::Reconcile(name.into()));
                    self.reconcile = Some(intent);
                    return self.roster_effect();
                }
                self.changes.push(ProfileChange::Deleted(name.into()));
                self.clear_editor();
                self.selected = None;
                self.notice = Some("Bot profile deleted".into());
                return self.roster_effect();
            }
            "bots.model-unset" => {
                self.busy = false;
                if v["blocked"].as_bool() == Some(true) || v["code"].as_i64() != Some(0) {
                    self.error =
                        Some("The agent did not clear this profile's model override".into());
                    return vec![];
                }
                self.baseline["model"] = json!("");
                self.baseline["provider"] = json!("");
                self.save_applied = true;
                return self.save_configuration(false);
            }
            "bots.configure" | "bots.model-confirm" => {
                self.busy = false;
                let Some(sent) = self.last_configure.take() else {
                    return vec![];
                };
                let Some(applied) = v["applied"].as_object() else {
                    self.error =
                        Some("The agent did not report which profile sections were saved".into());
                    return vec![];
                };
                let mut failed = vec![];
                for (section, keys) in [
                    ("description", vec!["description"]),
                    ("soul", vec!["soul"]),
                    ("model", vec!["model", "provider"]),
                    ("skills", vec!["disabled_skills"]),
                    ("toolsets", vec!["enabled_toolsets"]),
                    ("mcp_servers", vec!["enabled_mcp_servers"]),
                ] {
                    if !keys.iter().any(|k| sent.get(k).is_some()) {
                        continue;
                    }
                    if section == "model" && v["confirm_required"].as_bool() == Some(true) {
                        self.confirm_model = Some(
                            json!({"name":sent["name"],"model":sent["model"],"provider":sent["provider"],"confirm_expensive_model":true}),
                        );
                        self.confirm_message = v["confirm_message"]
                            .as_str()
                            .unwrap_or("Confirm this model selection?")
                            .into();
                        continue;
                    }
                    if applied.get(section).and_then(Value::as_bool) != Some(true) {
                        failed.push(section);
                        continue;
                    }
                    self.save_applied = true;
                    match section {
                        "description" => {
                            self.baseline["description"] = sent["description"].clone();
                            self.description
                                .replace_text(sent["description"].as_str().unwrap_or(""));
                        }
                        "soul" => {
                            self.baseline["soul"] = sent["soul"].clone();
                            self.soul.replace_text(sent["soul"].as_str().unwrap_or(""));
                        }
                        "model" => {
                            self.baseline["model"] = sent["model"].clone();
                            self.baseline["provider"] = sent["provider"].clone();
                            self.model
                                .replace_text(sent["model"].as_str().unwrap_or(""));
                            self.provider
                                .replace_text(sent["provider"].as_str().unwrap_or(""));
                        }
                        "skills" => {
                            for c in &mut self.skills {
                                c.saved = c.enabled;
                            }
                        }
                        "toolsets" => {
                            for c in &mut self.toolsets {
                                c.saved = c.enabled;
                            }
                        }
                        _ => {
                            for c in &mut self.mcp {
                                c.saved = c.enabled;
                            }
                        }
                    }
                }
                if !failed.is_empty() {
                    self.error = Some(format!(
                        "These profile sections remain unsaved: {}",
                        failed.join(", ")
                    ));
                }

                return self.save_look();
            }
            "bots.meta-read" => {
                let Some(rows) = valid_roster(&v) else {
                    self.busy = false;
                    self.error = Some("Cannot read the current bot appearance safely".into());
                    return vec![];
                };
                let Some(row) = rows
                    .iter()
                    .find(|r| r["name"].as_str() == self.selected.as_deref())
                else {
                    self.busy = false;
                    self.error = Some("This profile is no longer in the roster".into());
                    return vec![];
                };
                self.read_meta(row);
                return self.meta_write();
            }
            "bots.meta-write" => {
                if v["applied"]["ui_meta_conflicts"]
                    .as_object()
                    .is_some_and(|c| !c.is_empty())
                {
                    if self.cas_attempts < 3 {
                        return vec![(
                            "bots.meta-read".into(),
                            PageRequest::rpc("profiles.list", json!({"include_sessions":false}))
                                .profile(self.survivor().unwrap_or("default")),
                        )];
                    }
                    self.busy = false;
                    self.error=Some("Another client kept changing this bot's appearance. Your edits are still here; refresh and retry.".into());
                    return vec![];
                }
                if v["applied"]["ui_meta"].as_bool() != Some(true) {
                    self.busy = false;
                    self.error = Some("The agent did not save this bot's appearance".into());
                    return vec![];
                }
                if let Some(patch) = self.pending_meta.take() {
                    for key in ["title", "shape", "color"] {
                        if let Some(value) = patch.get(key) {
                            self.baseline[key] = value.clone();
                            match key {
                                "title" => self.title.replace_text(value.as_str().unwrap_or("")),
                                "shape" => {
                                    self.shape.replace_text(value.as_str().unwrap_or("circle"))
                                }
                                _ => self.color.replace_text(value.as_str().unwrap_or("")),
                            }
                        }
                    }
                    if let Some(rev) = v["applied"]["ui_meta_revisions"]["hermes-bots"].as_u64() {
                        self.meta_revision = Some(rev);
                    }
                }
                self.save_applied = true;
                return self.save_avatar();
            }
            "bots.asset-write" => {
                self.busy = false;
                let confirmed = v["ok"].as_bool() != Some(false)
                    && (v["ok"].as_bool() == Some(true)
                        || v["asset"] == "avatar"
                            && (v["size"].as_u64().is_some_and(|n| n > 0)
                                || v["removed"].as_u64().is_some()));
                if !confirmed {
                    self.error = Some("The agent did not confirm the avatar asset change".into());
                    return vec![];
                }
                self.avatar_saved = self.avatar_data.clone();
                self.save_applied = true;
                return self.finish_save();
            }
            "bots.ready" => {
                self.busy = false;
                if let Some(name) = self.created_chat_pending.take() {
                    let kickoff = v["ok"].as_bool() != Some(false);
                    self.new_bot_chat = Some(NewBotChat {
                        name,
                        kickoff,
                        editor_epoch: self.editor_epoch,
                    });
                    if !kickoff {
                        self.notice = Some(
                            "Bot created. Configure its model before starting the introduction."
                                .into(),
                        );
                    }
                }
            }
            "bots.archive-download" => {
                self.busy = false;
                if v["saved"].as_bool() == Some(true) || v["ok"].as_bool() == Some(true) {
                    self.notice = Some("Profile archive downloaded".into());
                } else {
                    self.error = Some("The archive download was not confirmed".into());
                }
            }
            "bots.setup" => {
                self.busy = false;
                let Some(command) = v["command"].as_str() else {
                    self.error = Some("The agent did not return a setup command".into());
                    return vec![];
                };
                self.setup_command = Some(command.into());
            }
            "bots.export" => {
                self.busy = false;
                if v["ok"].as_bool() != Some(true) || !v["archive"].is_string() {
                    self.error = Some("The agent did not confirm the archive export".into());
                    return vec![];
                }
                self.exported_archive = v["archive"].as_str().map(str::to_owned);
                self.dialog = None;
                self.notice = Some("Profile archive exported on the agent".into());
            }
            "bots.import" => {
                self.busy = false;
                if v["ok"].as_bool() != Some(true) || !v["name"].is_string() {
                    self.error = Some("The agent did not confirm the imported profile".into());
                    return vec![];
                }
                let name = string(&v, "name").to_owned();
                self.dialog = None;
                self.clear_editor();
                self.selected = Some(name.clone());
                self.imported_overlay = v["desktop"]
                    .as_object()
                    .map(|o| (name.clone(), Value::Object(o.clone())));
                self.changes.push(ProfileChange::Created(name));
                self.notice=Some("Profile imported. Agent configuration is available; client layout import is separate.".into());
                return self.roster_effect();
            }
            _ => {}
        }
        vec![]
    }
    fn basic_soul_read(&mut self) -> Vec<PageEffect> {
        let Some(name) = self.selected.clone() else {
            return vec![];
        };
        if let Some(row) = self.selected_row().cloned() {
            for key in ["model", "provider"] {
                let Some(value) = row[key].as_str() else {
                    continue;
                };
                let editor = if key == "model" {
                    &mut self.model
                } else {
                    &mut self.provider
                };
                if editor.text == self.baseline[key].as_str().unwrap_or("") {
                    editor.replace_text(value);
                }
                self.baseline[key] = json!(value);
            }
        }
        self.detail_loading = true;
        vec![(
            "bots.soul".into(),
            PageRequest::get(format!("/api/profiles/{}/soul", segment(&name))).profile(&name),
        )]
    }
    fn save_basic(&mut self) -> Vec<PageEffect> {
        if ["description", "model", "provider"]
            .iter()
            .any(|k| self.editor_value(k) != self.baseline[*k].as_str().unwrap_or(""))
        {
            self.error=Some("This agent does not offer advanced profile configuration. Use Settings to change its model; SOUL and appearance can be saved here.".into());
            return vec![];
        }
        if self.soul.text != self.baseline["soul"].as_str().unwrap_or("") {
            let Some(name) = self.selected.clone() else {
                return vec![];
            };
            let soul = self.soul_for(&name, false);
            self.busy = true;
            self.last_configure = Some(json!({"soul":soul}));
            return vec![(
                "bots.soul-save".into(),
                PageRequest::put(
                    format!("/api/profiles/{}/soul", segment(&name)),
                    json!({"content":soul}),
                )
                .profile(&name),
            )];
        }
        self.save_look()
    }
    fn save_configuration(&mut self, all: bool) -> Vec<PageEffect> {
        let Some(name) = self.selected.clone() else {
            return vec![];
        };
        let body = match self.configure_body(&name, all) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e);
                return vec![];
            }
        };
        if body.as_object().is_some_and(|b| b.len() > 1) {
            self.busy = true;
            self.last_configure = Some(body.clone());
            return vec![(
                "bots.configure".into(),
                PageRequest::rpc("profiles.configure", body).profile(&name),
            )];
        }
        self.save_look()
    }
    fn save_look(&mut self) -> Vec<PageEffect> {
        let patch = self.look_patch();
        if patch.as_object().is_some_and(|p| !p.is_empty()) {
            return self.metadata_effect(patch);
        }
        self.save_avatar()
    }
    fn save_avatar(&mut self) -> Vec<PageEffect> {
        if self.avatar_data != self.avatar_saved {
            let Some(name) = &self.selected else {
                return vec![];
            };
            self.busy = true;
            let body = if let Some(data) = &self.avatar_data {
                json!({"name":name,"asset":"avatar","data":data})
            } else {
                json!({"name":name,"asset":"avatar","clear":true})
            };
            return vec![(
                "bots.asset-write".into(),
                PageRequest::rpc("profiles.set_asset", body).profile(name),
            )];
        }
        self.finish_save()
    }
    fn finish_save(&mut self) -> Vec<PageEffect> {
        self.busy = false;
        if self.save_applied {
            if let Some(name) = self.selected.clone() {
                self.changes.push(ProfileChange::Updated(name.clone()));
                self.notice = Some(
                    if self.dirty() {
                        "Saved the acknowledged sections. Pending edits are still shown."
                    } else {
                        "Bot profile saved"
                    }
                    .into(),
                );
                self.save_applied = false;
                let mut e = self.roster_effect();
                e.extend(self.detail_effects(&name));
                if self.created_chat_pending.as_deref() == Some(&name)
                    && self.confirm_model.is_none()
                    && !self.dirty()
                {
                    self.busy = true;
                    e.push((
                        "bots.ready".into(),
                        PageRequest::rpc("setup.runtime_check", json!({})).profile(&name),
                    ));
                }
                return e;
            }
        }
        vec![]
    }
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.busy || self.normalizing && !matches!(a, Action::Discard) {
            return vec![];
        }
        self.error = None;
        match a {
            Action::Load => self.roster_effect(),
            Action::Select(name) => {
                if self.dirty() || self.dialog.is_some() {
                    self.error = Some(
                        "Save or discard this bot's edits before selecting another profile".into(),
                    );
                    return vec![];
                }
                if !self.roster.iter().any(|r| r["name"] == name) {
                    return vec![];
                }
                self.clear_editor();
                self.selected = Some(name.clone());
                self.seed_selected_metadata();
                self.detail_effects(&name)
            }
            Action::New => {
                if self.dirty() || self.dialog.is_some() {
                    self.error =
                        Some("Save or discard your edits before creating another bot".into());
                    return vec![];
                }
                self.begin_new();
                let source = self.survivor().unwrap_or("default").to_owned();
                self.clone_from = Some(source.clone());
                self.new_capabilities(&source)
            }
            Action::Discard => {
                let selected = self.selected.clone();
                self.clear_editor();
                self.selected = selected.or_else(|| self.survivor().map(str::to_owned));
                self.seed_selected_metadata();
                if let Some(name) = self.selected.clone() {
                    self.detail_effects(&name)
                } else {
                    vec![]
                }
            }
            Action::Tab(tab) => {
                self.tab = tab;
                vec![]
            }
            Action::CloneFrom(name) => {
                if !self.creating {
                    return vec![];
                }
                if name
                    .as_ref()
                    .is_some_and(|n| !self.roster.iter().any(|r| r["name"] == *n))
                {
                    return vec![];
                }
                self.clone_from = name.clone();
                self.skills.clear();
                self.toolsets.clear();
                self.mcp.clear();
                let source = name.unwrap_or_else(|| self.survivor().unwrap_or("default").into());
                self.new_capabilities(&source)
            }
            Action::Toggle(kind, name, enabled) => {
                let caps = match kind.as_str() {
                    "skills" => &mut self.skills,
                    "toolsets" => &mut self.toolsets,
                    _ => &mut self.mcp,
                };
                if let Some(c) = caps.iter_mut().find(|c| c.name == name) {
                    if kind == "mcp" && !c.installed && enabled {
                        self.error = Some("Set up this connector before enabling it".into());
                    } else {
                        c.enabled = enabled;
                    }
                }
                vec![]
            }
            Action::Save => {
                self.save_applied = false;
                if self.color.text != self.baseline["color"].as_str().unwrap_or("")
                    && !faces::valid_color(self.color.text.trim())
                {
                    self.error = Some("Use a hexadecimal color or hsl(hue saturation% lightness%), or leave it blank for automatic color".into());
                    return vec![];
                }
                if self.creating {
                    let name = self.name.text.trim().to_owned();
                    if !valid_name(&name) || self.roster.iter().any(|r| r["name"] == name) {
                        self.error=Some("Choose an unused lowercase profile ID (letters, numbers, hyphens or underscores, up to 64 characters)".into());
                        return vec![];
                    }
                    if self.model.text.trim().is_empty() != self.provider.text.trim().is_empty() {
                        self.error = Some(
                            "Choose both model and provider, or leave both blank to inherit".into(),
                        );
                        return vec![];
                    }
                    let body = json!({"name":name,"description":self.description.text.trim(),"clone_from":self.clone_from,"clone_all":self.clone_all,"no_skills":self.no_skills,"share_auth":self.share_auth,"soul":self.soul_for(&name,true)});
                    self.pending_create = Some(body.clone());
                    self.busy = true;
                    return vec![(
                        "bots.create".into(),
                        PageRequest::rpc("profiles.create", body),
                    )];
                }
                let Some(name) = self.selected.clone() else {
                    return vec![];
                };
                if !self.advanced_supported {
                    return self.save_basic();
                }
                let reset = (self.model.text != self.baseline["model"].as_str().unwrap_or("")
                    || self.provider.text != self.baseline["provider"].as_str().unwrap_or(""))
                    && self.model.text.trim().is_empty()
                    && self.provider.text.trim().is_empty();
                if reset {
                    if !valid_name(&name) {
                        self.error = Some(
                            "This legacy profile name cannot use the model-reset command".into(),
                        );
                        return vec![];
                    }
                    self.busy = true;
                    return vec![(
                        "bots.model-unset".into(),
                        PageRequest::rpc(
                            "cli.exec",
                            json!({"argv":["--profile",name,"config","unset","model"]}),
                        )
                        .profile(&name),
                    )];
                }
                self.save_configuration(false)
            }
            Action::ConfirmModel => {
                let Some(body) = self.confirm_model.take() else {
                    return vec![];
                };
                let Some(name) = body["name"].as_str() else {
                    return vec![];
                };
                if self.selected.as_deref() != Some(name) {
                    return vec![];
                }
                if self.model.text.trim() != string(&body, "model")
                    || self.provider.text.trim() != string(&body, "provider")
                {
                    self.confirm_message.clear();
                    self.error = Some(
                        "The model draft changed. Save it again to review a fresh confirmation."
                            .into(),
                    );
                    return vec![];
                }
                self.busy = true;
                self.last_configure = Some(body.clone());
                vec![(
                    "bots.model-confirm".into(),
                    PageRequest::rpc("profiles.configure", body.clone()).profile(name),
                )]
            }
            Action::CancelModel => {
                self.confirm_model = None;
                self.confirm_message.clear();
                vec![]
            }
            Action::ResetModel => {
                self.provider.replace_text("");
                self.model.replace_text("");
                self.confirm_model = None;
                vec![]
            }
            Action::Rename => {
                if self.dirty() {
                    self.error =
                        Some("Save or discard the bot's edits before renaming its profile".into());
                    return vec![];
                }
                let Some(name) = self.selected.clone() else {
                    return vec![];
                };
                self.rename.replace_text(if name == "default" {
                    String::new()
                } else {
                    name.clone()
                });
                self.dialog = Some(format!("rename:{name}"));
                vec![]
            }
            Action::SubmitRename => {
                let Some(name) = self
                    .dialog
                    .as_deref()
                    .and_then(|d| d.strip_prefix("rename:"))
                    .map(str::to_owned)
                else {
                    return vec![];
                };
                let new_name = self.rename.text.trim().to_owned();
                if new_name.is_empty()
                    || name != "default"
                        && (!valid_name(&new_name)
                            || self
                                .roster
                                .iter()
                                .any(|r| r["name"] == new_name && new_name != name))
                {
                    self.error=Some("Choose an unused valid profile ID; the default profile accepts a display label".into());
                    return vec![];
                }
                if name != "default" && new_name == name {
                    self.dialog = None;
                    return vec![];
                }
                self.busy = true;
                self.pending_mutation =
                    Some(json!({"kind":"rename","name":name,"new_name":new_name}));
                vec![(
                    "bots.rename".into(),
                    PageRequest::patch(
                        format!("/api/profiles/{}", segment(&name)),
                        json!({"new_name":new_name}),
                    )
                    .profile(self.survivor().unwrap_or("default")),
                )]
            }
            Action::Delete => {
                if self.dirty() {
                    self.error = Some("Save or discard your edits before deleting this bot".into());
                    return vec![];
                }
                if let Some(name) = self
                    .selected
                    .as_ref()
                    .filter(|n| n.to_lowercase() != "default")
                {
                    self.dialog = Some(format!("delete:{name}"));
                }
                vec![]
            }
            Action::ConfirmDelete => {
                let Some(name) = self
                    .dialog
                    .as_deref()
                    .and_then(|d| d.strip_prefix("delete:"))
                    .filter(|n| n.to_lowercase() != "default")
                    .map(str::to_owned)
                else {
                    self.error = Some("The default profile cannot be deleted".into());
                    return vec![];
                };
                if !self.roster.iter().any(|r| r["name"] == name) {
                    return vec![];
                }
                self.busy = true;
                self.pending_mutation = Some(json!({"kind":"delete","name":name}));
                vec![(
                    "bots.delete".into(),
                    PageRequest::delete(format!("/api/profiles/{}", segment(&name)), Value::Null)
                        .profile(self.survivor().unwrap_or("default")),
                )]
            }
            Action::CancelDialog => {
                self.dialog = None;
                vec![]
            }
            Action::Duplicate => {
                if self.dirty() {
                    self.error = Some("Save or discard edits before duplicating this bot".into());
                    return vec![];
                }
                let Some(base) = self.selected.clone() else {
                    return vec![];
                };
                if !valid_name(&base) {
                    self.error = Some(
                        "Rename this legacy profile to a valid profile ID before duplicating it"
                            .into(),
                    );
                    return vec![];
                }
                let Some(name) = (2..100)
                    .map(|n| {
                        let suffix = format!("-{n}");
                        format!("{}{}", &base[..base.len().min(64 - suffix.len())], suffix)
                    })
                    .find(|n| !self.roster.iter().any(|r| r["name"] == *n))
                else {
                    self.error = Some("No unused duplicate name is available".into());
                    return vec![];
                };
                let mut look = self.meta.clone();
                if let Some(fields) = look.as_object_mut() {
                    fields.remove("chat");
                    fields.remove("created");
                    fields.remove("image");
                }
                look["title"] = json!(if self.title.text.trim().is_empty() {
                    String::new()
                } else {
                    format!("{} (copy)", self.title.text.trim())
                });
                look["shape"] = json!(self.shape.text.trim());
                look["color"] = if self.color.text.trim().is_empty() {
                    Value::Null
                } else {
                    json!(self.color.text.trim())
                };
                self.pending_mutation = Some(
                    json!({"kind":"duplicate","name":base,"new_name":name,"look":look,"description":self.description.text.trim()}),
                );
                self.busy = true;
                vec![(
                    "bots.duplicate-avatar".into(),
                    PageRequest::rpc("profiles.get_asset", json!({"name":base,"asset":"avatar"}))
                        .profile(&base),
                )]
            }
            Action::Setup => {
                let Some(name) = self.selected.clone() else {
                    return vec![];
                };
                self.busy = true;
                self.dialog = Some("setup".into());
                vec![(
                    "bots.setup".into(),
                    PageRequest::get(format!("/api/profiles/{}/setup-command", segment(&name)))
                        .profile(self.survivor().unwrap_or("default")),
                )]
            }
            Action::Export => {
                if self.dialog.as_deref() != Some("export") {
                    self.dialog = Some("export".into());
                    self.export_path.replace_text("");
                    return vec![];
                }
                let Some(name) = self.selected.clone() else {
                    return vec![];
                };
                let extra_files = if let Some((profile, overlay)) = self
                    .export_overlay
                    .as_ref()
                    .filter(|(profile, _)| profile == &name)
                {
                    let _ = profile;
                    json!({"desktop.json":serde_json::to_string(overlay).expect("desktop export overlay")})
                } else {
                    json!({})
                };
                self.busy = true;
                vec![(
                    "bots.export".into(),
                    PageRequest::post(
                        format!("/api/profiles/{}/export", segment(&name)),
                        json!({"output":self.export_path.text.trim(),"extra_files":extra_files}),
                    )
                    .profile(&name)
                    .timeout_secs(120),
                )]
            }
            Action::Import => {
                if self.dialog.as_deref() != Some("import") {
                    self.dialog = Some("import".into());
                    self.archive_path.replace_text("");
                    self.archive_name.replace_text("");
                    return vec![];
                }
                if self.archive_path.text.trim().is_empty() {
                    self.error = Some(
                        "Enter an existing profile archive path on the connected agent".into(),
                    );
                    return vec![];
                }
                let name = self.archive_name.text.trim();
                if !name.is_empty()
                    && (!valid_name(name) || self.roster.iter().any(|r| r["name"] == name))
                {
                    self.error =
                        Some("Choose an unused valid import name, or leave it blank".into());
                    return vec![];
                }
                self.busy = true;
                vec![("bots.import".into(),PageRequest::post("/api/profiles/import",json!({"archive":self.archive_path.text.trim(),"name":if name.is_empty(){Value::Null}else{json!(name)}})).profile(self.survivor().unwrap_or("default")).timeout_secs(120))]
            }
            Action::DownloadArchive(path) => {
                let Some(archive) = self.exported_archive.clone() else {
                    return vec![];
                };
                let Some(name) = self.selected.clone() else {
                    return vec![];
                };
                self.busy = true;
                vec![(
                    "bots.archive-download".into(),
                    PageRequest::get("/api/fs/download")
                        .query("path", archive)
                        .profile(&name)
                        .download_to(path)
                        .timeout_secs(3600),
                )]
            }
            Action::AvatarPath(path) => {
                self.stage_avatar(path);
                vec![]
            }
            Action::AvatarClear => {
                self.set_avatar(None);
                vec![]
            }
            Action::AvatarGenerate => {
                if !self.can_generate {
                    return vec![];
                }
                let name = self
                    .selected
                    .clone()
                    .unwrap_or_else(|| self.survivor().unwrap_or("default").into());
                let title = if self.title.text.trim().is_empty() {
                    &self.name.text
                } else {
                    &self.title.text
                };
                self.generating = true;
                self.busy = true;
                vec![("bots.avatar-generate".into(),PageRequest::rpc("image.generate",json!({"prompt":format!("{} Avatar for an AI agent named {}: centered, bold flat vector style, solid color background, no text.",if self.avatar_prompt.text.trim().is_empty(){self.description.text.trim()}else{self.avatar_prompt.text.trim()},title.trim()),"aspect_ratio":"square"})).profile(&name).timeout_secs(90))]
            }
            Action::BlobLock | Action::BlobReroll => {
                let (seed, kind) = faces::blob_parts(&self.shape.text);
                let next = if matches!(a, Action::BlobLock) && !seed.is_empty() {
                    String::new()
                } else if matches!(a, Action::BlobLock) {
                    if self.name.text.trim().is_empty() {
                        "agent".into()
                    } else {
                        self.name.text.clone()
                    }
                } else {
                    format!(
                        "{:x}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    )
                };
                let next_shape = faces::blob_shape(&next, kind);
                self.shape.replace_text(next_shape);
                self.set_avatar(None);
                vec![]
            }
            Action::BlobKind(kind) => {
                let (seed, _) = faces::blob_parts(&self.shape.text);
                let next_shape = faces::blob_shape(seed, &kind);
                self.shape.replace_text(next_shape);
                self.set_avatar(None);
                vec![]
            }
            Action::CheckReady => {
                let Some(name) = self
                    .created_chat_pending
                    .as_ref()
                    .filter(|n| self.selected.as_deref() == Some(n.as_str()))
                    .cloned()
                else {
                    return vec![];
                };
                if self.dirty() || self.confirm_model.is_some() {
                    return vec![];
                }
                self.busy = true;
                vec![(
                    "bots.ready".into(),
                    PageRequest::rpc("setup.runtime_check", json!({})).profile(&name),
                )]
            }
            Action::OpenChat(_)
            | Action::OpenCapabilities(_)
            | Action::OpenMcp(_)
            | Action::ProviderSetup => vec![],
        }
    }
    fn new_capabilities(&mut self, source: &str) -> Vec<PageEffect> {
        self.catalog_loading = true;
        vec![
            (
                "bots.create-caps".into(),
                PageRequest::rpc("profiles.describe", json!({"name":source})).profile(source),
            ),
            (
                "bots.mcp".into(),
                PageRequest::rpc("mcp.catalog", json!({})).profile(source),
            ),
            (
                "bots.catalog".into(),
                PageRequest::get("/api/model/options")
                    .query("explicit_only", 1)
                    .profile(source),
            ),
            (
                "bots.avatar-probe".into(),
                PageRequest::rpc("image.generate", json!({"probe":true})).profile(source),
            ),
        ]
    }
}
fn valid_roster(v: &Value) -> Option<Vec<Value>> {
    let rows = v["profiles"].as_array()?;
    let mut names = std::collections::BTreeSet::new();
    for row in rows {
        let name = row["name"].as_str().filter(|n| !n.trim().is_empty())?;
        if !names.insert(name) {
            return None;
        }
    }
    Some(rows.clone())
}
impl Viewer {
    pub fn bots_action(&mut self, a: Action) {
        if self.bots.demo
            && !matches!(
                a,
                Action::Select(_)
                    | Action::New
                    | Action::Tab(_)
                    | Action::Discard
                    | Action::CancelDialog
                    | Action::CancelModel
            )
        {
            return;
        }
        if matches!(a, Action::Export) {
            self.prepare_bot_export();
        }
        match &a {
            Action::OpenChat(name) => {
                self.open_bot(name.clone());
                return;
            }
            Action::OpenCapabilities(name) => {
                if self.bots.creating {
                    self.bots.error =
                        Some("Create the bot before configuring installed capabilities".into());
                    return;
                }
                self.open_capabilities_for_profile(Some(name.clone()));
                return;
            }
            Action::OpenMcp(name) => {
                if self.bots.creating {
                    self.bots.error = Some("Create the bot before running connector setup".into());
                    return;
                }
                if let Some(profile) = self.bots.selected.clone() {
                    self.open_mcp_settings(Some(profile), name.clone());
                }
                return;
            }
            Action::ProviderSetup => {
                if let Some(profile) = self.bots.selected.clone() {
                    self.settings_action(crate::pages::settings::Action::Scope(Some(
                        profile.clone(),
                    )));
                    if self.settings.scope_profile.as_deref() == Some(&profile) {
                        self.open_route(crate::pages::Route::Settings);
                        self.settings_action(crate::pages::settings::Action::Section(
                            "providers".into(),
                        ));
                    }
                }
                return;
            }
            _ => {}
        }
        let retire = match &a {
            Action::SubmitRename => self
                .bots
                .dialog
                .as_deref()
                .and_then(|d| d.strip_prefix("rename:"))
                .filter(|n| {
                    *n != "default"
                        && valid_name(self.bots.rename.text.trim())
                        && *n != self.bots.rename.text.trim()
                        && !self
                            .bots
                            .roster
                            .iter()
                            .any(|r| r["name"] == self.bots.rename.text.trim())
                })
                .map(str::to_owned),
            Action::ConfirmDelete => self
                .bots
                .dialog
                .as_deref()
                .and_then(|d| d.strip_prefix("delete:"))
                .filter(|n| n.to_lowercase() != "default")
                .map(str::to_owned),
            _ => None,
        };
        if let Some(name) = retire {
            if !self.prepare_profile_mutation(&name) {
                return;
            }
            self.invalidate_page_requests("bots.list");
            self.invalidate_page_requests("bots.detail");
            self.invalidate_page_requests("bots.soul");
            self.invalidate_page_requests("bots.avatar");
        }
        if matches!(
            a,
            Action::Select(_) | Action::New | Action::Discard | Action::CloneFrom(_)
        ) && !self.bots.busy
        {
            self.invalidate_page_requests("bots.detail");
            self.invalidate_page_requests("bots.soul");
            self.invalidate_page_requests("bots.mcp");
            self.invalidate_page_requests("bots.catalog");
            self.invalidate_page_requests("bots.avatar");
            self.invalidate_page_requests("bots.create-caps");
        }
        let effects = self.bots.act(a);
        self.page_requests(effects);
    }
}
