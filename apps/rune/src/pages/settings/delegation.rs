//! Named model and identity routes use the backend's dedicated plugin contract.
use crate::pages::{PageEffect, PageRequest};
use rune::TextEdit;
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub const FIELDS: [(&str, &str); 6] = [
    ("name", "Route name"),
    ("description", "Description"),
    ("provider", "Provider"),
    ("model", "Model"),
    ("reasoning_effort", "Reasoning effort (blank: inherit)"),
    ("service_tier", "Service tier (blank: inherit)"),
];
pub struct ProfileRoute {
    pub name: String,
    pub available: bool,
    pub enabled: bool,
    pub description: TextEdit,
    extra: Value,
}
pub struct Delegation {
    pub data: Value,
    pub loading: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub editing: bool,
    pub original: Option<String>,
    pub fields: BTreeMap<String, TextEdit>,
    pub profiles: Vec<ProfileRoute>,
    pub profile_default: Option<String>,
    pub confirm_delete: Option<String>,
    pending: Option<(String, Value)>,
    detached: TextEdit,
}
impl Delegation {
    pub fn new() -> Self {
        Self {
            data: Value::Null,
            loading: false,
            busy: false,
            error: None,
            notice: None,
            editing: false,
            original: None,
            fields: FIELDS
                .iter()
                .map(|(k, _)| (k.to_string(), TextEdit::new("")))
                .collect(),
            profiles: vec![],
            profile_default: None,
            confirm_delete: None,
            pending: None,
            detached: TextEdit::new(""),
        }
    }
    pub fn dirty(&self) -> bool {
        self.editing || self.data.is_object() && self.profile_value() != self.data["profile_routes"]
    }
    pub fn edit(&mut self, key: &str) -> &mut TextEdit {
        self.fields.get_mut(key).unwrap_or(&mut self.detached)
    }
    pub fn profile_edit(&mut self, name: &str) -> &mut TextEdit {
        self.profiles
            .iter_mut()
            .find(|p| p.name == name)
            .map(|p| &mut p.description)
            .unwrap_or(&mut self.detached)
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        vec![(
            "settings.delegation.load".into(),
            PageRequest::get("/api/delegation/routing"),
        )]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                self.loading = false;
                if op == "settings.delegation.write" {
                    self.busy = false;
                }
                self.error = Some(e);
                return vec![];
            }
        };
        if op == "settings.delegation.write" {
            self.busy = false;
            if v["ok"].as_bool() != Some(true) {
                self.error = Some("The agent did not confirm the delegation change".into());
                return vec![];
            }
            if let Some((kind, body)) = self.pending.take() {
                if kind == "model_map" {
                    self.editing = false;
                    self.original = None;
                    self.data["model_map"] = body;
                } else if kind == "profile_routes" {
                    self.data["profile_routes"] = body;
                }
            }
            self.notice = Some("Delegation routing saved".into());
            return self.load();
        }
        if !v["backends"].is_array()
            || !v["profiles"].is_array()
            || !v["model_map"].is_object()
            || !v["profile_routes"].is_object()
        {
            self.loading = false;
            self.error = Some("Invalid delegation routing response".into());
            return vec![];
        }
        self.loading = false;
        let dirty_profiles =
            self.data.is_object() && self.profile_value() != self.data["profile_routes"];
        self.data = v;
        if !dirty_profiles {
            self.seed_profiles();
        }
        vec![]
    }
    fn seed_profiles(&mut self) {
        let configured = self.data["profile_routes"]["profiles"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut names = self.data["profiles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for p in &configured {
            if let Some(name) = p["name"].as_str() {
                if !names.iter().any(|n| n == name) {
                    names.push(name.into());
                }
            }
        }
        self.profiles = names
            .into_iter()
            .map(|name| {
                let existing = configured.iter().find(|p| p["name"] == name);
                ProfileRoute {
                    available: self.data["profiles"]
                        .as_array()
                        .is_some_and(|p| p.iter().any(|p| p == &name)),
                    enabled: existing.is_some(),
                    description: TextEdit::new(
                        existing
                            .and_then(|p| p["description"].as_str())
                            .unwrap_or(""),
                    ),
                    extra: existing.cloned().unwrap_or_else(|| json!({"name":name})),
                    name,
                }
            })
            .collect();
        self.profile_default = self.data["profile_routes"]["default"]
            .as_str()
            .map(str::to_owned);
    }
    fn profile_value(&self) -> Value {
        let mut v = self.data["profile_routes"]
            .as_object()
            .cloned()
            .map(Value::Object)
            .unwrap_or_else(|| json!({}));
        v["profiles"] = Value::Array(
            self.profiles
                .iter()
                .filter(|p| p.enabled)
                .map(|p| {
                    let mut v = p.extra.clone();
                    v["name"] = json!(p.name);
                    if p.description.text != p.extra["description"].as_str().unwrap_or("") {
                        if p.description.text.trim().is_empty() {
                            v.as_object_mut().unwrap().remove("description");
                        } else {
                            v["description"] = json!(p.description.text.trim());
                        }
                    }
                    v
                })
                .collect(),
        );
        v["default"] = self
            .profile_default
            .as_ref()
            .map(|name| json!(name))
            .unwrap_or(Value::Null);
        v
    }
    fn write(&mut self, kind: &str, path: &str, body: Value) -> Vec<PageEffect> {
        self.busy = true;
        self.pending = Some((kind.into(), body.clone()));
        vec![(
            "settings.delegation.write".into(),
            PageRequest::put(path, body),
        )]
    }
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.busy {
            return vec![];
        }
        self.error = None;
        match a {
            Action::Load => self.load(),
            Action::Backend(name) => {
                if self.dirty() {
                    self.error = Some(
                        "Save or cancel routing edits before changing the delegation backend"
                            .into(),
                    );
                    return vec![];
                }
                if !self.data["backends"].as_array().is_some_and(|b| {
                    b.iter()
                        .any(|b| b["name"] == name && b["available"].as_bool() == Some(true))
                }) {
                    self.error = Some("This delegation backend is not available".into());
                    return vec![];
                }
                self.write(
                    "backend",
                    "/api/delegation/backend",
                    json!({"backend":name}),
                )
            }
            Action::New => {
                self.editing = true;
                self.original = None;
                for e in self.fields.values_mut() {
                    e.replace_text("");
                }
                vec![]
            }
            Action::Edit(name) => {
                let Some(route) = self.data["model_map"]["options"]
                    .as_array()
                    .and_then(|o| o.iter().find(|r| r["name"] == name))
                else {
                    return vec![];
                };
                for (key, editor) in &mut self.fields {
                    editor.replace_text(route[key].as_str().unwrap_or(""));
                }
                self.original = Some(name);
                self.editing = true;
                vec![]
            }
            Action::Cancel => {
                self.editing = false;
                self.original = None;
                vec![]
            }
            Action::SaveRoute => {
                let name = self.fields["name"].text.trim();
                if name.is_empty()
                    || name.chars().any(char::is_whitespace)
                    || self.data["model_map"]["options"]
                        .as_array()
                        .is_some_and(|r| {
                            r.iter().any(|r| {
                                r["name"] == name && r["name"].as_str() != self.original.as_deref()
                            })
                        })
                {
                    self.error = Some("Use a unique route name without spaces".into());
                    return vec![];
                }
                let previous = self.data["model_map"]["options"]
                    .as_array()
                    .and_then(|o| {
                        o.iter()
                            .find(|r| r["name"].as_str() == self.original.as_deref())
                    })
                    .cloned();
                if self.fields["description"].text.trim().is_empty()
                    || self.fields["model"].text.trim().is_empty()
                    || self.fields["provider"].text.trim().is_empty()
                        && previous
                            .as_ref()
                            .is_none_or(|p| p["base_url"].as_str().is_none_or(str::is_empty))
                {
                    self.error=Some("Description, model and provider are required (existing endpoint routes may inherit their URL)".into());
                    return vec![];
                }
                let effort = self.fields["reasoning_effort"].text.trim();
                let tier = self.fields["service_tier"].text.trim();
                if !matches!(
                    effort,
                    "" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
                ) || !matches!(tier, "" | "normal" | "fast" | "ultrafast")
                {
                    self.error =
                        Some("Choose a supported reasoning effort and service tier".into());
                    return vec![];
                }
                let mut route = previous.unwrap_or_else(|| json!({}));
                if let Some(original) = &self.original {
                    route["original_name"] = json!(original);
                }
                for (key, editor) in &self.fields {
                    let text = editor.text.trim();
                    if text.is_empty() {
                        route.as_object_mut().unwrap().remove(key);
                    } else {
                        route[key] = json!(text);
                    }
                }
                let mut map = self.data["model_map"].clone();
                let mut options = map["options"].as_array().cloned().unwrap_or_default();
                if let Some(index) = options
                    .iter()
                    .position(|r| r["name"].as_str() == self.original.as_deref())
                {
                    options[index] = route;
                } else {
                    options.push(route);
                }
                let preferred = if self.original.is_some()
                    && map["default"].as_str() == self.original.as_deref()
                {
                    json!(name)
                } else {
                    map["default"].clone()
                };
                let default = if options.iter().any(|r| r["name"] == preferred) {
                    preferred
                } else {
                    options
                        .first()
                        .map(|r| r["name"].clone())
                        .unwrap_or(Value::Null)
                };
                map["options"] = json!(options);
                map["default"] = default;
                self.write("model_map", "/api/delegation/backends/model_map", map)
            }
            Action::Delete(name) => {
                self.confirm_delete = Some(name);
                vec![]
            }
            Action::CancelDelete => {
                self.confirm_delete = None;
                vec![]
            }
            Action::ConfirmDelete => {
                let Some(name) = self.confirm_delete.take() else {
                    return vec![];
                };
                let mut map = self.data["model_map"].clone();
                let options = map["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|r| r["name"] != name)
                    .cloned()
                    .collect::<Vec<_>>();
                if map["default"] == name {
                    map["default"] = options
                        .first()
                        .map(|r| r["name"].clone())
                        .unwrap_or(Value::Null);
                }
                map["options"] = json!(options);
                self.write("model_map", "/api/delegation/backends/model_map", map)
            }
            Action::DefaultRoute(name) => {
                if !self.data["model_map"]["options"]
                    .as_array()
                    .is_some_and(|r| r.iter().any(|r| r["name"] == name))
                {
                    return vec![];
                }
                let mut map = self.data["model_map"].clone();
                map["default"] = json!(name);
                self.write("model_map", "/api/delegation/backends/model_map", map)
            }
            Action::ProfileToggle(name, enabled) => {
                let Some(p) = self.profiles.iter_mut().find(|p| p.name == name) else {
                    return vec![];
                };
                if enabled && !p.available {
                    return vec![];
                }
                p.enabled = enabled;
                if self
                    .profile_default
                    .as_ref()
                    .is_none_or(|n| !self.profiles.iter().any(|p| p.enabled && &p.name == n))
                {
                    self.profile_default = self
                        .profiles
                        .iter()
                        .find(|p| p.enabled)
                        .map(|p| p.name.clone());
                }
                let body = self.profile_value();
                self.write(
                    "profile_routes",
                    "/api/delegation/backends/profile_routes",
                    body,
                )
            }
            Action::ProfileDefault(name) => {
                if !self.profiles.iter().any(|p| p.name == name && p.enabled) {
                    return vec![];
                }
                self.profile_default = Some(name);
                let body = self.profile_value();
                self.write(
                    "profile_routes",
                    "/api/delegation/backends/profile_routes",
                    body,
                )
            }
            Action::SaveProfiles => {
                let body = self.profile_value();
                self.write(
                    "profile_routes",
                    "/api/delegation/backends/profile_routes",
                    body,
                )
            }
            Action::RevertProfiles => {
                self.seed_profiles();
                vec![]
            }
        }
    }
}
#[derive(Clone)]
pub enum Action {
    Load,
    Backend(String),
    New,
    Edit(String),
    Cancel,
    SaveRoute,
    Delete(String),
    CancelDelete,
    ConfirmDelete,
    DefaultRoute(String),
    ProfileToggle(String, bool),
    ProfileDefault(String),
    SaveProfiles,
    RevertProfiles,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Delegation {
        let mut d = Delegation::new();
        d.apply("settings.delegation.load",Ok(json!({"configured":"model_map","blocked_reason":null,"backends":[{"name":"default","available":true},{"name":"model_map","available":true}],"model_map":{"default":"old","options":[{"name":"old","description":"Existing route","provider":"sample","model":"model","base_url":"https://example.invalid","api_key":"masked-preview","fallback_providers":[{"provider":"fallback","model":"other","key_env":"FALLBACK_KEY"}],"future":true}]},"profile_routes":{"default":null,"profiles":[{"name":"default","description":""}]},"profiles":["default","other"]})));
        d
    }
    #[test]
    fn route_rename_keeps_default_secret_marker_and_opaque_routes() {
        let mut d = state();
        assert!(!d.dirty());
        d.act(Action::Edit("old".into()));
        d.edit("name").replace_text("new");
        let e = d.act(Action::SaveRoute);
        let map = &e[0].1.body;
        assert_eq!(map["default"], "new");
        let route = &map["options"][0];
        assert_eq!(route["original_name"], "old");
        assert_eq!(route["api_key"], "masked-preview");
        assert_eq!(route["fallback_providers"][0]["key_env"], "FALLBACK_KEY");
        assert_eq!(route["future"], true);
    }
    #[test]
    fn invalid_route_cannot_persist_or_switch_away_from_dirty_draft() {
        let mut d = state();
        d.act(Action::New);
        d.edit("name").replace_text("bad name");
        assert!(d.act(Action::SaveRoute).is_empty());
        assert!(d.act(Action::Backend("default".into())).is_empty());
        assert!(d.error.is_some());
        assert!(!d.busy);
    }
    #[test]
    fn profile_routing_preserves_empty_description_and_selects_valid_default() {
        let mut d = state();
        assert!(!d.dirty());
        let e = d.act(Action::ProfileToggle("other".into(), true));
        assert_eq!(e[0].1.body["default"], "default");
        assert_eq!(e[0].1.body["profiles"][0]["description"], "");
        d.apply("settings.delegation.write", Ok(json!({"ok":true})));
        assert!(!d.dirty());
        d.profile_edit("other").replace_text("Research tasks");
        assert!(d.dirty());
        let e = d.act(Action::SaveProfiles);
        assert_eq!(e[0].1.body["profiles"][1]["description"], "Research tasks");
    }
}
