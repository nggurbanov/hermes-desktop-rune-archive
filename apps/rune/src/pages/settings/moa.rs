//! Typed MoA draft; opaque routing fields survive every preset and slot edit.
use crate::pages::{PageEffect, PageRequest};
use rune::TextEdit;
use serde_json::{Value, json};
#[derive(Clone)]
pub struct Slot {
    pub provider: TextEdit,
    pub model: TextEdit,
    pub enabled: bool,
    extra: Value,
}
impl Slot {
    fn parse(v: &Value) -> Self {
        Self {
            provider: TextEdit::new(v["provider"].as_str().unwrap_or("")),
            model: TextEdit::new(v["model"].as_str().unwrap_or("")),
            enabled: v["enabled"].as_bool() != Some(false),
            extra: v.clone(),
        }
    }
    fn value(&self) -> Value {
        let mut v = self.extra.clone();
        v["provider"] = json!(self.provider.text.trim());
        v["model"] = json!(self.model.text.trim());
        v["enabled"] = json!(self.enabled);
        v
    }
}
pub struct Moa {
    pub selected: String,
    pub names: Vec<String>,
    pub default: String,
    pub active: String,
    pub new_name: TextEdit,
    pub aggregator: Slot,
    pub references: Vec<Slot>,
    pub enabled: bool,
    pub aggregator_temperature: TextEdit,
    pub reference_temperature: TextEdit,
    pub timeout: TextEdit,
    pub policy: String,
    pub open: bool,
    pub loading: bool,
    pub saving: bool,
    pub dirty: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub confirm_delete: bool,
    raw: Value,
    detached: TextEdit,
}
impl Moa {
    pub fn new() -> Self {
        Self {
            selected: String::new(),
            names: vec![],
            default: String::new(),
            active: String::new(),
            new_name: TextEdit::new(""),
            aggregator: Slot::parse(&json!({})),
            references: vec![],
            enabled: true,
            aggregator_temperature: TextEdit::new("0.7"),
            reference_temperature: TextEdit::new("0.7"),
            timeout: TextEdit::new(""),
            policy: "loud".into(),
            open: false,
            loading: false,
            saving: false,
            dirty: false,
            error: None,
            notice: None,
            confirm_delete: false,
            raw: Value::Null,
            detached: TextEdit::new(""),
        }
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        if self.dirty || self.saving {
            return vec![];
        }
        self.loading = true;
        vec![(
            "settings.moa.load".into(),
            PageRequest::get("/api/model/moa"),
        )]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                if op == "settings.moa.save" {
                    self.saving = false;
                }
                self.loading = false;
                self.error = Some(e);
                return vec![];
            }
        };
        if op == "settings.moa.save" {
            self.saving = false;
            if value["ok"].as_bool() != Some(true) {
                self.error = Some("The agent did not confirm the MoA save".into());
                return vec![];
            }
            self.dirty = false;
            self.notice = Some("MoA presets saved".into());
            if !value["presets"].is_object() {
                return self.load();
            }
        }
        self.loading = false;
        if !value["presets"].is_object()
            || value["presets"].as_object().is_none_or(|p| p.is_empty())
        {
            self.error = Some("Invalid MoA presets response".into());
            return vec![];
        }
        if self.dirty {
            return vec![];
        }
        self.raw = value;
        self.names = self.raw["presets"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        self.default = self.raw["default_preset"].as_str().unwrap_or("").into();
        self.active = self.raw["active_preset"].as_str().unwrap_or("").into();
        if !self.names.contains(&self.selected) {
            self.selected = if self.names.contains(&self.default) {
                self.default.clone()
            } else {
                self.names[0].clone()
            };
        }
        self.select_draft();
        vec![]
    }
    fn select_draft(&mut self) {
        let p = &self.raw["presets"][&self.selected];
        self.enabled = p["enabled"].as_bool() != Some(false);
        self.aggregator = Slot::parse(&p["aggregator"]);
        self.references = p["reference_models"]
            .as_array()
            .into_iter()
            .flatten()
            .map(Slot::parse)
            .collect();
        self.aggregator_temperature
            .replace_text(p["aggregator_temperature"].to_string());
        self.reference_temperature
            .replace_text(p["reference_temperature"].to_string());
        self.timeout
            .replace_text(if p["reference_timeout"].is_number() {
                p["reference_timeout"].to_string()
            } else {
                String::new()
            });
        self.policy = p["degraded_reference_policy"]
            .as_str()
            .unwrap_or("loud")
            .into();
    }
    fn flush(&mut self) -> Result<(), String> {
        if self.selected.is_empty() {
            return Err("Load MoA presets first".into());
        }
        let at = temperature(&self.aggregator_temperature.text)?;
        let rt = temperature(&self.reference_temperature.text)?;
        let timeout = if self.timeout.text.trim().is_empty() {
            Value::Null
        } else {
            let n = self
                .timeout
                .text
                .trim()
                .parse::<f64>()
                .map_err(|_| "Enter a positive reference timeout")?;
            if !n.is_finite() || n <= 0.0 {
                return Err("Enter a positive reference timeout".into());
            }
            json!(n)
        };
        let p = &mut self.raw["presets"][&self.selected];
        p["enabled"] = json!(self.enabled);
        p["aggregator"] = self.aggregator.value();
        p["reference_models"] = Value::Array(self.references.iter().map(Slot::value).collect());
        p["aggregator_temperature"] = json!(at);
        p["reference_temperature"] = json!(rt);
        p["reference_timeout"] = timeout;
        p["degraded_reference_policy"] = json!(self.policy);
        Ok(())
    }
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.saving {
            return vec![];
        }
        self.error = None;
        match a {
            Action::Load => self.load(),
            Action::Edited => {
                self.dirty = true;
                vec![]
            }
            Action::Select(name) => {
                if let Err(e) = self.flush() {
                    self.error = Some(e);
                    return vec![];
                }
                if self.names.contains(&name) {
                    self.selected = name;
                    self.select_draft();
                }
                self.open = false;
                vec![]
            }
            Action::Enabled(v) => {
                self.enabled = v;
                self.dirty = true;
                vec![]
            }
            Action::ReferenceEnabled(i, v) => {
                if let Some(slot) = self.references.get_mut(i) {
                    slot.enabled = v;
                    self.dirty = true;
                }
                vec![]
            }
            Action::Policy(v) => {
                self.policy = v;
                self.dirty = true;
                vec![]
            }
            Action::AddReference => {
                self.references.push(Slot::parse(&json!({})));
                self.dirty = true;
                vec![]
            }
            Action::RemoveReference(i) => {
                if self.references.len() > 1 && i < self.references.len() {
                    self.references.remove(i);
                    self.dirty = true;
                }
                vec![]
            }
            Action::AddPreset => {
                let name = self.new_name.text.trim().to_owned();
                if name.is_empty() || self.names.contains(&name) {
                    self.error = Some("Choose a new preset name".into());
                    return vec![];
                }
                if let Err(e) = self.flush() {
                    self.error = Some(e);
                    return vec![];
                }
                self.raw["presets"][&name] = self.raw["presets"][&self.selected].clone();
                self.names.push(name.clone());
                self.selected = name;
                self.new_name.replace_text("");
                self.dirty = true;
                self.save()
            }
            Action::Default => {
                self.default = self.selected.clone();
                self.raw["default_preset"] = json!(self.default);
                self.dirty = true;
                self.save()
            }
            Action::Delete => {
                if self.names.len() > 1 {
                    self.confirm_delete = true;
                }
                vec![]
            }
            Action::CancelDelete => {
                self.confirm_delete = false;
                vec![]
            }
            Action::ConfirmDelete => {
                self.confirm_delete = false;
                if self.names.len() <= 1 {
                    return vec![];
                }
                self.raw["presets"]
                    .as_object_mut()
                    .unwrap()
                    .remove(&self.selected);
                self.names.retain(|n| n != &self.selected);
                if self.default == self.selected {
                    self.default = self.names[0].clone();
                    self.raw["default_preset"] = json!(self.default);
                }
                if self.active == self.selected {
                    self.active.clear();
                    self.raw["active_preset"] = json!("");
                }
                self.selected = self.names[0].clone();
                self.select_draft();
                self.dirty = true;
                self.save()
            }
            Action::Save => self.save(),
            Action::Revert => {
                self.dirty = false;
                self.load()
            }
        }
    }
    fn save(&mut self) -> Vec<PageEffect> {
        if let Err(e) = self.flush() {
            self.error = Some(e);
            return vec![];
        }
        for p in self.raw["presets"]
            .as_object()
            .into_iter()
            .flat_map(|p| p.values())
        {
            let complete = |s: &Value| {
                s["provider"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty() && s != "moa")
                    && s["model"].as_str().is_some_and(|s| !s.trim().is_empty())
            };
            if !complete(&p["aggregator"])
                || !p["reference_models"]
                    .as_array()
                    .is_some_and(|slots| !slots.is_empty() && slots.iter().all(complete))
            {
                self.error=Some("Complete every provider and model in all presets before saving. MoA cannot reference itself.".into());
                return vec![];
            }
        }
        self.saving = true;
        vec![(
            "settings.moa.save".into(),
            PageRequest::put("/api/model/moa", self.raw.clone()),
        )]
    }
    pub fn edit(&mut self, slot: Option<usize>, field: &str) -> &mut TextEdit {
        let s = if let Some(i) = slot {
            self.references.get_mut(i)
        } else {
            Some(&mut self.aggregator)
        };
        match s {
            Some(s) => match field {
                "provider" => &mut s.provider,
                _ => &mut s.model,
            },
            None => &mut self.detached,
        }
    }
}
fn temperature(value: &str) -> Result<f64, String> {
    let v = value
        .trim()
        .parse::<f64>()
        .map_err(|_| "Enter a temperature from 0 to 2")?;
    if !v.is_finite() || !(0.0..=2.0).contains(&v) {
        Err("Enter a temperature from 0 to 2".into())
    } else {
        Ok(v)
    }
}
#[derive(Clone)]
pub enum Action {
    Load,
    Edited,
    Select(String),
    Enabled(bool),
    ReferenceEnabled(usize, bool),
    Policy(String),
    AddReference,
    RemoveReference(usize),
    AddPreset,
    Default,
    Delete,
    CancelDelete,
    ConfirmDelete,
    Save,
    Revert,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Moa {
        let mut s = Moa::new();
        s.apply("settings.moa.load",Ok(json!({"default_preset":"balanced","active_preset":"balanced","presets":{"balanced":{"enabled":true,"reference_models":[{"provider":"sample","model":"reference","enabled":false,"reasoning_effort":"high","routing_hint":"keep"}],"aggregator":{"provider":"sample","model":"aggregate","reasoning_effort":"ultra"},"reference_temperature":0.7,"aggregator_temperature":0.4,"reference_timeout":null,"degraded_reference_policy":"loud","fanout":"every_n:3","future_option":{"preserve":true}}}})));
        s
    }
    #[test]
    fn preset_edit_keeps_opaque_slot_and_cadence_values() {
        let mut s = state();
        s.references[0].model.replace_text("reference-new");
        s.act(Action::Edited);
        let e = s.act(Action::Save);
        let p = &e[0].1.body["presets"]["balanced"];
        assert_eq!(p["fanout"], "every_n:3");
        assert_eq!(p["future_option"]["preserve"], true);
        assert_eq!(p["reference_models"][0]["reasoning_effort"], "high");
        assert_eq!(p["reference_models"][0]["routing_hint"], "keep");
        assert_eq!(p["reference_models"][0]["model"], "reference-new");
        assert_eq!(p["reference_models"][0]["enabled"], false);
        assert_eq!(p["aggregator"]["reasoning_effort"], "ultra");
        assert!(s.dirty);
        s.apply("settings.moa.save", Ok(json!({"ok":false})));
        assert!(s.dirty);
        assert!(s.error.is_some());
    }
    #[test]
    fn incomplete_or_recursive_moa_slot_never_reaches_agent() {
        let mut s = state();
        s.act(Action::AddReference);
        assert!(s.act(Action::Save).is_empty());
        s.references[1].provider.replace_text("moa");
        s.references[1].model.replace_text("balanced");
        assert!(s.act(Action::Save).is_empty());
        s.references[1].provider.replace_text("sample");
        assert_eq!(s.act(Action::Save)[0].1.path, "/api/model/moa");
    }
    #[test]
    fn deleting_default_is_confirmed_and_keeps_valid_fallback() {
        let mut s = state();
        s.new_name.replace_text("copy");
        let e = s.act(Action::AddPreset);
        s.apply(
            "settings.moa.save",
            Ok(e[0]
                .1
                .body
                .as_object()
                .map(|v| {
                    let mut v = v.clone();
                    v.insert("ok".into(), json!(true));
                    Value::Object(v)
                })
                .unwrap()),
        );
        assert_eq!(s.names.len(), 2);
        s.act(Action::Select("balanced".into()));
        assert!(s.act(Action::Delete).is_empty());
        assert!(s.confirm_delete);
        let e = s.act(Action::ConfirmDelete);
        assert_eq!(e[0].1.body["default_preset"], "copy");
        assert_eq!(e[0].1.body["active_preset"], "");
        assert!(e[0].1.body["presets"].get("balanced").is_none());
    }
}
