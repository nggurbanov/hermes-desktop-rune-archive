//! Device presentation is independent of agent-owned display/font configuration.
mod keys;
use crate::{
    composer::DraftScope,
    pages::{PageEffect, PageRequest},
};
pub use keys::{COMMANDS, Command, binding_label, event_chord};
use rune::{Color, Family, TextEdit, TextStyle};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub chat_scale: u16,
    pub session_density: String,
    pub bubble_transparency: u8,
    pub reasoning_collapsed: bool,
    pub technical_tools: bool,
    pub bindings: BTreeMap<String, Vec<String>>,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            chat_scale: 110,
            session_density: "compact".into(),
            bubble_transparency: 0,
            reasoning_collapsed: false,
            technical_tools: false,
            bindings: BTreeMap::new(),
            other: BTreeMap::new(),
        }
    }
}
impl Preferences {
    fn normalize(&mut self) {
        if ![90, 100, 110, 125, 150, 175].contains(&self.chat_scale) {
            self.chat_scale = 110;
        }
        if !["compact", "comfortable", "detailed"].contains(&self.session_density.as_str()) {
            self.session_density = "compact".into();
        }
        self.bubble_transparency = self.bubble_transparency.min(100);
    }
}
pub enum Action {
    Scale(u16),
    Density(String),
    Bubble(u8),
    Reasoning(bool),
    Technical(bool),
    ResetDisplay,
    Capture(String),
    Clear(String),
    Reset(String),
    ResetBindings,
}
pub struct ClientSettings {
    pub prefs: Preferences,
    pub system_dark: std::cell::Cell<bool>,
    pub search: TextEdit,
    pub capture: Option<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub open: Option<String>,
    owner: Option<DraftScope>,
    config: Value,
    path: Option<PathBuf>,
    writable: bool,
    next_read: f64,
    reading: bool,
}
impl Default for ClientSettings {
    fn default() -> Self {
        Self::new()
    }
}
impl ClientSettings {
    pub fn new() -> Self {
        Self {
            prefs: Preferences::default(),
            system_dark: std::cell::Cell::new(false),
            search: TextEdit::new(""),
            capture: None,
            error: None,
            notice: None,
            open: None,
            owner: None,
            config: json!({}),
            path: None,
            writable: true,
            next_read: 0.,
            reading: false,
        }
    }
    pub fn load(&mut self) {
        match crate::client::state_dir() {
            Ok(p) => self.load_at(p.join("display.json")),
            Err(e) => {
                self.error = Some(e);
                self.writable = false;
            }
        }
    }
    fn load_at(&mut self, path: PathBuf) {
        self.path = Some(path.clone());
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(mut p) if p.version == 1 => {
                    p.normalize();
                    self.prefs = p;
                    self.writable = true;
                }
                _ => {
                    self.error=Some("Saved display preferences are invalid or newer than this client; the file is preserved.".into());
                    self.writable = false;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                self.error = Some(
                    "Saved display preferences could not be read; the file is preserved.".into(),
                );
                self.writable = false;
            }
        }
    }
    fn save(&mut self, persist: bool) {
        if !persist {
            return;
        }
        self.notice = None;
        if !self.writable {
            self.error = Some(
                "Display preferences are read-only because the saved file could not be loaded."
                    .into(),
            );
            return;
        }
        let result = (|| {
            let path = self
                .path
                .as_ref()
                .ok_or("The client state directory is unavailable")?;
            let bytes = serde_json::to_vec_pretty(&self.prefs)
                .map_err(|_| "Display preferences could not be encoded")?;
            crate::client::private_write(path, &bytes)
        })();
        match result {
            Ok(()) => {
                self.error = None;
                self.notice = Some("Saved on this device".into());
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub fn act(&mut self, a: Action, persist: bool) {
        match a {
            Action::Capture(id) => {
                if COMMANDS.iter().any(|c| c.id == id) {
                    self.capture = Some(id);
                    self.notice = None;
                }
                return;
            }
            Action::Scale(n) => self.prefs.chat_scale = n,
            Action::Density(n) => self.prefs.session_density = n,
            Action::Bubble(n) => self.prefs.bubble_transparency = n,
            Action::Reasoning(n) => self.prefs.reasoning_collapsed = n,
            Action::Technical(n) => self.prefs.technical_tools = n,
            Action::ResetDisplay => {
                let p = Preferences::default();
                self.prefs.chat_scale = p.chat_scale;
                self.prefs.session_density = p.session_density;
                self.prefs.bubble_transparency = p.bubble_transparency;
                self.prefs.reasoning_collapsed = p.reasoning_collapsed;
                self.prefs.technical_tools = p.technical_tools;
            }
            Action::Clear(id) => {
                self.set_binding(id, None, persist);
                return;
            }
            Action::Reset(id) => {
                self.prefs.bindings.remove(&id);
            }
            Action::ResetBindings => self
                .prefs
                .bindings
                .retain(|id, _| !COMMANDS.iter().any(|c| c.id == id)),
        }
        self.prefs.normalize();
        self.save(persist);
    }
    pub fn set_binding(&mut self, id: String, chord: Option<String>, persist: bool) {
        if !COMMANDS.iter().any(|c| c.id == id) {
            return;
        }
        let next = chord
            .into_iter()
            .filter_map(|s| keys::canonical(&s))
            .collect::<Vec<_>>();
        let defaults = COMMANDS
            .iter()
            .find(|c| c.id == id)
            .map(|c| {
                c.defaults
                    .iter()
                    .filter_map(|s| keys::canonical(s))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if next == defaults {
            self.prefs.bindings.remove(&id);
        } else {
            self.prefs.bindings.insert(id, next);
        }
        self.capture = None;
        self.save(persist);
    }
    pub fn bindings(&self, id: &str) -> Vec<String> {
        self.prefs
            .bindings
            .get(id)
            .map(|v| v.iter().filter_map(|s| keys::canonical(s)).collect())
            .unwrap_or_else(|| {
                COMMANDS
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| {
                        c.defaults
                            .iter()
                            .filter_map(|s| keys::canonical(s))
                            .collect()
                    })
                    .unwrap_or_default()
            })
    }
    pub fn conflicts(&self, id: &str) -> Vec<&'static str> {
        let bindings = self.bindings(id);
        COMMANDS
            .iter()
            .filter(|c| c.id != id && self.bindings(c.id).iter().any(|b| bindings.contains(b)))
            .map(|c| c.label)
            .collect()
    }
    pub fn command(&self, chord: &str, focused: bool) -> Option<&'static str> {
        if focused
            && chord
                .parse::<rune::Shortcut>()
                .ok()
                .is_none_or(|key| key.require_modifier().is_err())
        {
            return None;
        }
        COMMANDS
            .iter()
            .find(|c| self.bindings(c.id).iter().any(|b| b == chord))
            .map(|c| c.id)
    }
    pub fn bind(&mut self, mut owner: DraftScope) -> Vec<PageEffect> {
        owner.session = None;
        if self.owner.as_ref() == Some(&owner) {
            return vec![];
        }
        self.owner = Some(owner);
        self.config = json!({});
        self.reading = false;
        self.next_read = 0.;
        self.read()
    }
    pub fn refresh(&mut self) -> Vec<PageEffect> {
        // A verified write must supersede a read started before that write.
        // PageNetwork fences the older operation generation when this one is queued.
        self.reading = false;
        self.read()
    }
    fn read(&mut self) -> Vec<PageEffect> {
        if self.reading {
            return vec![];
        }
        let Some(owner) = self.owner.as_ref() else {
            return vec![];
        };
        self.reading = true;
        vec![(
            "client-display.config".into(),
            PageRequest::get("/api/config").profile(&owner.profile),
        )]
    }
    pub fn poll(&mut self, now: f64) -> Vec<PageEffect> {
        if now < self.next_read {
            return vec![];
        }
        self.next_read = now + 30.;
        self.read()
    }
    pub fn apply(&mut self, _operation: &str, result: Result<Value, String>) {
        self.reading = false;
        if let Ok(value) = result {
            if value.is_object() {
                self.config = value;
            }
        }
    }
    pub fn apply_config(&mut self, value: &Value) {
        if value.is_object() {
            self.config = value.clone();
        }
    }
    pub fn theme_kit(&self, base: &rune_kit::Kit) -> rune_kit::Kit {
        let mut k = base.clone();
        let family = self.chat_family();
        for f in [
            &mut k.ty.ui,
            &mut k.ty.ui_medium,
            &mut k.ty.ui_compact,
            &mut k.ty.small,
            &mut k.ty.small_medium,
            &mut k.ty.body,
            &mut k.ty.body_medium,
            &mut k.ty.text_small,
            &mut k.ty.text_small_medium,
            &mut k.ty.text_xsmall,
            &mut k.ty.text_xsmall_medium,
        ] {
            f.family = family;
        }
        k
    }
    pub fn prose(&self, mut style: TextStyle) -> TextStyle {
        let factor = self.prefs.chat_scale as f32 / 110.;
        style.size *= factor;
        style.line_height *= factor;
        if style.family != Family::Mono && style.family != Family::SystemMono {
            style.family = self.chat_family();
        }
        style
    }
    pub fn chat_family(&self) -> Family {
        match self
            .config
            .pointer("/desktop/font_family")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "ibm plex mono" => Family::Mono,
            "monospace" | "ui-monospace" => Family::SystemMono,
            _ => Family::Sans,
        }
    }
    pub fn terminal_family(&self) -> Family {
        match self
            .config
            .pointer("/terminal/font_family")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "monospace" | "ui-monospace" => Family::SystemMono,
            _ => Family::Mono,
        }
    }
    pub fn bubble_fill(&self, color: Color) -> Color {
        color.alpha(color.a * (100 - self.prefs.bubble_transparency) as f32 / 100.)
    }
    pub fn default_open(&self, reasoning: bool, simple_mode: bool) -> bool {
        !simple_mode
            && if reasoning {
                !self.prefs.reasoning_collapsed
            } else {
                self.prefs.technical_tools
            }
    }
    pub fn show_reasoning(&self) -> bool {
        let value = self
            .config
            .pointer("/display/platforms/cli/show_reasoning")
            .or_else(|| self.config.pointer("/display/show_reasoning"));
        !matches!(value, Some(Value::Bool(false)))
            && value != Some(&json!("false"))
            && value != Some(&json!(0))
    }
    pub fn timestamps(&self) -> bool {
        matches!(
            self.config.pointer("/display/timestamps"),
            Some(Value::Bool(true))
        ) || self.config.pointer("/display/timestamps") == Some(&json!("true"))
            || self.config.pointer("/display/timestamps") == Some(&json!(1))
    }
}
#[cfg(test)]
mod tests;
