use super::*;
impl Bots {
    pub fn dirty(&self) -> bool {
        if !self.creating && self.selected.is_none() {
            return false;
        }
        self.creating
            || [
                "title",
                "description",
                "soul",
                "provider",
                "model",
                "shape",
                "color",
            ]
            .iter()
            .any(|k| self.editor_value(k) != self.baseline[*k].as_str().unwrap_or(""))
            || self.avatar_data != self.avatar_saved
            || self
                .skills
                .iter()
                .chain(&self.toolsets)
                .chain(&self.mcp)
                .any(|c| c.enabled != c.saved)
    }
    pub(super) fn editor_value(&self, key: &str) -> &str {
        match key {
            "title" => &self.title.text,
            "description" => &self.description.text,
            "soul" => &self.soul.text,
            "provider" => &self.provider.text,
            "model" => &self.model.text,
            "shape" => &self.shape.text,
            "color" => &self.color.text,
            _ => "",
        }
    }
    pub(super) fn clear_editor(&mut self) {
        self.editor_epoch = self.editor_epoch.wrapping_add(1);
        self.creating = false;
        self.confirm_model = None;
        self.confirm_message.clear();
        self.dialog = None;
        self.open = None;
        self.normalizing = false;
        self.generating = false;
        self.pending_meta = None;
        self.cas_attempts = 0;
        self.last_configure = None;
        self.baseline = json!({});
        self.meta = json!({});
        self.meta_revision = None;
        self.name.replace_text("");
        self.title.replace_text("");
        self.description.replace_text("");
        self.soul.replace_text("");
        self.provider.replace_text("");
        self.model.replace_text("");
        self.shape.replace_text("circle");
        self.color.replace_text("");
        self.skills.clear();
        self.toolsets.clear();
        self.mcp.clear();
        self.avatar_prompt.replace_text("");
        self.avatar_data = None;
        self.avatar_saved = None;
        self.avatar_preview = None;
        self.setup_command = None;
        self.exported_archive = None;
        self.export_overlay = None;
        self.created_chat_pending = None;
        self.new_bot_chat = None;
        self.capability_search.replace_text("");
    }
    pub(super) fn begin_new(&mut self) {
        self.clear_editor();
        self.selected = None;
        self.creating = true;
        self.shape.replace_text("blobatar");
        self.clone_from = Some("default".into());
        self.share_auth = true;
        self.no_skills = false;
        self.clone_all = false;
        self.tab = "identity".into();
        self.error = None;
        self.notice = None;
    }
    pub(super) fn seed_selected_metadata(&mut self) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        self.name.replace_text(string(&row, "name"));
        self.description.replace_text(string(&row, "description"));
        self.read_meta(&row);
        self.title.replace_text(
            self.meta["title"]
                .as_str()
                .or_else(|| row["bot_title"].as_str())
                .unwrap_or(""),
        );
        self.shape
            .replace_text(self.meta["shape"].as_str().unwrap_or("circle"));
        self.color
            .replace_text(self.meta["color"].as_str().unwrap_or(""));
        for key in ["title", "description", "shape", "color"] {
            self.baseline[key] = json!(self.editor_value(key));
        }
    }
    pub(super) fn read_meta(&mut self, row: &Value) {
        self.meta = row["ui_meta"]["hermes-bots"]
            .as_object()
            .cloned()
            .map(Value::Object)
            .unwrap_or_else(|| json!({}));
        self.meta_revision = row["ui_meta_revisions"]["hermes-bots"].as_u64();
    }
    pub(super) fn seed_detail(&mut self, v: &Value) {
        if let Some(name) = v["name"].as_str() {
            if !self.creating && self.selected.as_deref() != Some(name) {
                return;
            }
        }
        for key in ["description", "soul", "provider", "model"] {
            let value = match key {
                "provider" => v["model"]["provider"].as_str().unwrap_or(""),
                "model" => v["model"]["default"].as_str().unwrap_or(""),
                _ => v[key].as_str().unwrap_or_else(|| {
                    if key == "description" {
                        self.baseline[key].as_str().unwrap_or("")
                    } else {
                        ""
                    }
                }),
            }
            .to_owned();
            let untouched = self.editor_value(key) == self.baseline[key].as_str().unwrap_or("");
            self.baseline[key] = json!(value);
            if untouched {
                match key {
                    "description" => self.description.replace_text(&value),
                    "soul" => self.soul.replace_text(&value),
                    "provider" => self.provider.replace_text(&value),
                    _ => self.model.replace_text(&value),
                }
            }
        }
        Self::seed_caps(&mut self.skills, &v["skills"], false);
        Self::seed_caps(&mut self.toolsets, &v["toolsets"], false);
        Self::seed_caps(&mut self.mcp, &v["mcp_servers"], true);
    }
    pub(super) fn seed_caps(target: &mut Vec<Capability>, v: &Value, mcp: bool) {
        let drafts = target
            .iter()
            .filter(|c| c.enabled != c.saved)
            .map(|c| (c.name.clone(), c.enabled))
            .collect::<BTreeMap<_, _>>();
        let extras = target
            .iter()
            .filter(|c| c.catalog)
            .cloned()
            .collect::<Vec<_>>();
        *target = v
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let name = c["name"].as_str()?;
                let saved = if mcp {
                    c["enabled"].as_bool() != Some(false)
                } else {
                    c["enabled"].as_bool() == Some(true)
                };
                Some(Capability {
                    name: name.into(),
                    label: c["label"].as_str().unwrap_or(name).into(),
                    description: c["description"].as_str().unwrap_or("").into(),
                    enabled: drafts.get(name).copied().unwrap_or(saved),
                    saved,
                    installed: true,
                    catalog: false,
                    requires: vec![],
                })
            })
            .collect();
        for c in extras {
            if !target.iter().any(|t| t.name == c.name) {
                target.push(c);
            }
        }
    }
    pub(super) fn merge_mcp_catalog(&mut self, v: &Value) {
        for c in v["servers"]
            .as_array()
            .or_else(|| v["entries"].as_array())
            .into_iter()
            .flatten()
        {
            let Some(name) = c["name"].as_str() else {
                continue;
            };
            if self.mcp.iter().any(|m| m.name == name) {
                continue;
            }
            self.mcp.push(Capability {
                name: name.into(),
                label: c["label"].as_str().unwrap_or(name).into(),
                description: c["description"].as_str().unwrap_or("").into(),
                enabled: false,
                saved: false,
                installed: c["installed"].as_bool().unwrap_or(false),
                catalog: true,
                requires: c["requires"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            });
        }
    }
    pub(super) fn look_patch(&self) -> Value {
        let mut patch = json!({});
        for key in ["title", "shape", "color"] {
            let value = self.editor_value(key).trim();
            if value != self.baseline[key].as_str().unwrap_or("").trim() {
                patch[key] = if value.is_empty() && key == "color" {
                    Value::Null
                } else {
                    json!(value)
                };
            }
        }
        if patch.get("shape").is_some() || patch.get("color").is_some() {
            patch["custom"] = json!(true);
        }
        if self.avatar_data != self.avatar_saved {
            patch["imageKind"] = json!(if self.avatar_data.is_some() {
                "photo"
            } else {
                "shape"
            });
            patch["custom"] = json!(true);
        }
        patch
    }
    pub(super) fn configure_body(&self, name: &str, include_all: bool) -> Result<Value, String> {
        let mut body = json!({"name":name});
        for key in ["description", "soul"] {
            if include_all || self.editor_value(key) != self.baseline[key].as_str().unwrap_or("") {
                let value = if key == "soul" {
                    self.soul_for(name, false)
                } else {
                    self.editor_value(key).trim().into()
                };
                body[key] = json!(value);
            }
        }
        let model_dirty = include_all
            && (!self.model.text.trim().is_empty() || !self.provider.text.trim().is_empty())
            || !include_all
                && (self.model.text != self.baseline["model"].as_str().unwrap_or("")
                    || self.provider.text != self.baseline["provider"].as_str().unwrap_or(""));
        if model_dirty {
            if self.model.text.trim().is_empty() != self.provider.text.trim().is_empty() {
                return Err("Set both model and provider, or leave both blank to inherit".into());
            }
            if !self.model.text.trim().is_empty() {
                body["model"] = json!(self.model.text.trim());
                body["provider"] = json!(self.provider.text.trim());
            }
        }
        for (kind, caps, wire) in [
            ("skills", &self.skills, "disabled_skills"),
            ("toolsets", &self.toolsets, "enabled_toolsets"),
            ("mcp", &self.mcp, "enabled_mcp_servers"),
        ] {
            if caps.iter().any(|c| c.enabled != c.saved) {
                let enabled = caps
                    .iter()
                    .filter(|c| c.enabled)
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>();
                body[wire] = if kind == "skills" {
                    json!(
                        caps.iter()
                            .filter(|c| !c.enabled)
                            .map(|c| c.name.clone())
                            .collect::<Vec<_>>()
                    )
                } else if kind == "toolsets" && (enabled.is_empty() || enabled.len() == caps.len())
                {
                    json!([])
                } else {
                    json!(enabled)
                };
            }
        }
        Ok(body)
    }
    pub(super) fn metadata_effect(&mut self, patch: Value) -> Vec<PageEffect> {
        if self.selected.is_none() {
            return vec![];
        }
        self.pending_meta = Some(patch);
        self.cas_attempts = 0;
        self.busy = true;
        vec![(
            "bots.meta-read".into(),
            PageRequest::rpc("profiles.list", json!({"include_sessions":false})),
        )]
    }
    pub(super) fn meta_write(&mut self) -> Vec<PageEffect> {
        let Some(name) = self.selected.clone() else {
            return vec![];
        };
        let Some(patch) = &self.pending_meta else {
            return vec![];
        };
        let mut meta = self.meta.clone();
        if let Some(map) = meta.as_object_mut() {
            map.remove("chat");
        }
        if let Some(fields) = patch.as_object() {
            for (key, value) in fields {
                meta[key] = value.clone();
            }
        }
        let mut body = json!({"name":name,"ui_meta":{"hermes-bots":meta}});
        if let Some(rev) = self.meta_revision {
            body["ui_meta_expected_revisions"] = json!({"hermes-bots":rev});
        }
        self.cas_attempts += 1;
        vec![(
            "bots.meta-write".into(),
            PageRequest::rpc("profiles.configure", body).profile(&name),
        )]
    }
    pub(super) fn soul_for(&self, name: &str, compose: bool) -> String {
        let value = if compose && self.soul.text.trim().is_empty() {
            let title = if self.title.text.trim().is_empty() {
                name
            } else {
                self.title.text.trim()
            };
            format!(
                "# {title}\n\n{}{}You are {title}, a persistent named agent (profile `{name}`) on this machine.\nYou keep your own memory, skills, and conversation history across sessions.",
                if self.title.text.trim().is_empty() {
                    String::new()
                } else {
                    format!("**Role:** {}\n\n", self.title.text.trim())
                },
                if self.description.text.trim().is_empty() {
                    String::new()
                } else {
                    format!("**Mission:** {}\n\n", self.description.text.trim())
                }
            )
        } else {
            self.soul.text.trim().into()
        };
        if self.server_protocol
            || value
                .lines()
                .any(|l| l.trim() == "## Messaging other agents")
        {
            return value;
        }
        let section = legacy_protocol(name, &self.roster);
        if value.is_empty() {
            section
        } else {
            format!("{value}\n\n{section}")
        }
    }
}
fn legacy_protocol(name: &str, roster: &[Value]) -> String {
    let mut section = include_str!("messaging-protocol.txt").replace("{{handle}}", name);
    let teammates = roster
        .iter()
        .filter(|r| r["name"] != name)
        .map(|r| {
            format!(
                "- `{}`{}",
                string(r, "name"),
                if string(r, "description").is_empty() {
                    String::new()
                } else {
                    format!(" — {}", string(r, "description"))
                }
            )
        })
        .collect::<Vec<_>>();
    section.push_str(if teammates.is_empty() {
        "\n- (none yet)"
    } else {
        "\n"
    });
    section.push_str(&teammates.join("\n"));
    section
}
