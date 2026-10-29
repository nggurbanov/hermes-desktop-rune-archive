//! Native profile manager. A bot remains an identity; opening it resolves Bot Chat by name.
mod actions;
mod assets;
pub use assets::decode_data as decode_avatar;
mod editor;
pub mod faces;
#[cfg(test)]
mod tests;
use crate::{
    app::Viewer,
    pages::{PageEffect, PageRequest, segment},
};
pub use actions::Action;
use rune::{Image, ImageCache, TextEdit};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub enum ProfileChange {
    Created(String),
    Renamed { from: String, to: String },
    Deleted(String),
    Updated(String),
    Reconcile(String),
}
#[derive(Clone, Debug)]
pub struct NewBotChat {
    pub name: String,
    pub kickoff: bool,
    pub editor_epoch: u64,
}
#[derive(Clone)]
pub struct Capability {
    pub name: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    pub installed: bool,
    pub catalog: bool,
    pub requires: Vec<String>,
    saved: bool,
}
pub struct Bots {
    pub roster: Vec<Value>,
    pub roster_revision: u64,
    pub selected: Option<String>,
    pub search: TextEdit,
    pub capability_search: TextEdit,
    pub editor_epoch: u64,
    pub creating: bool,
    pub tab: String,
    pub loading: bool,
    pub detail_loading: bool,
    pub busy: bool,
    pub normalizing: bool,
    pub demo: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub open: Option<String>,
    pub name: TextEdit,
    pub title: TextEdit,
    pub description: TextEdit,
    pub soul: TextEdit,
    pub provider: TextEdit,
    pub model: TextEdit,
    pub shape: TextEdit,
    pub color: TextEdit,
    pub clone_from: Option<String>,
    pub no_skills: bool,
    pub share_auth: bool,
    pub clone_all: bool,
    pub skills: Vec<Capability>,
    pub toolsets: Vec<Capability>,
    pub mcp: Vec<Capability>,
    pub catalog: Vec<Value>,
    pub catalog_loading: bool,
    pub confirm_model: Option<Value>,
    pub confirm_message: String,
    pub dialog: Option<String>,
    pub rename: TextEdit,
    pub archive_path: TextEdit,
    pub archive_name: TextEdit,
    pub export_path: TextEdit,
    pub setup_command: Option<String>,
    pub exported_archive: Option<String>,
    export_overlay: Option<(String, Value)>,
    pub imported_overlay: Option<(String, Value)>,
    pub avatar_prompt: TextEdit,
    pub avatar_data: Option<String>,
    pub avatar_preview: Option<Image>,
    pub can_generate: bool,
    pub generating: bool,
    avatar_saved: Option<String>,
    images: Option<ImageCache>,
    avatar_worker: Option<assets::AvatarWorker>,
    changes: Vec<ProfileChange>,
    baseline: Value,
    meta: Value,
    meta_revision: Option<u64>,
    last_configure: Option<Value>,
    pending_meta: Option<Value>,
    cas_attempts: u8,
    pending_create: Option<Value>,
    pending_mutation: Option<Value>,
    reconcile: Option<Value>,
    detached: TextEdit,
    server_protocol: bool,
    clock: f64,
    created_chat_pending: Option<String>,
    new_bot_chat: Option<NewBotChat>,
    save_applied: bool,
    advanced_supported: bool,
    redraw: bool,
}
impl Default for Bots {
    fn default() -> Self {
        Self::new()
    }
}
impl Bots {
    pub fn new() -> Self {
        Self {
            roster: vec![],
            roster_revision: 0,
            selected: None,
            search: TextEdit::new(""),
            capability_search: TextEdit::new(""),
            editor_epoch: 0,
            creating: false,
            tab: "identity".into(),
            loading: false,
            detail_loading: false,
            busy: false,
            normalizing: false,
            demo: false,
            error: None,
            notice: None,
            open: None,
            name: TextEdit::new(""),
            title: TextEdit::new(""),
            description: TextEdit::new(""),
            soul: TextEdit::new(""),
            provider: TextEdit::new(""),
            model: TextEdit::new(""),
            shape: TextEdit::new("circle"),
            color: TextEdit::new(""),
            clone_from: Some("default".into()),
            no_skills: false,
            share_auth: true,
            clone_all: false,
            skills: vec![],
            toolsets: vec![],
            mcp: vec![],
            catalog: vec![],
            catalog_loading: false,
            confirm_model: None,
            confirm_message: String::new(),
            dialog: None,
            rename: TextEdit::new(""),
            archive_path: TextEdit::new(""),
            archive_name: TextEdit::new(""),
            export_path: TextEdit::new(""),
            setup_command: None,
            exported_archive: None,
            export_overlay: None,
            imported_overlay: None,
            avatar_prompt: TextEdit::new(""),
            avatar_data: None,
            avatar_preview: None,
            can_generate: false,
            generating: false,
            avatar_saved: None,
            images: None,
            avatar_worker: None,
            changes: vec![],
            baseline: json!({}),
            meta: json!({}),
            meta_revision: None,
            last_configure: None,
            pending_meta: None,
            cas_attempts: 0,
            pending_create: None,
            pending_mutation: None,
            reconcile: None,
            detached: TextEdit::new(""),
            server_protocol: false,
            clock: 0.0,
            created_chat_pending: None,
            new_bot_chat: None,
            save_applied: false,
            advanced_supported: true,
            redraw: false,
        }
    }
    pub fn reset(&mut self) {
        let images = self.images.take();
        let worker = self.avatar_worker.take();
        let epoch = self.editor_epoch.wrapping_add(1);
        *self = Self::new();
        self.images = images;
        self.avatar_worker = worker;
        self.editor_epoch = epoch;
    }
    pub fn take_new_bot_chat(&mut self) -> Option<NewBotChat> {
        self.new_bot_chat.take()
    }
    pub fn awaiting_readiness(&self) -> bool {
        self.created_chat_pending.as_deref() == self.selected.as_deref()
            && self.created_chat_pending.is_some()
    }
    pub fn set_export_overlay(&mut self, profile: String, value: Value) {
        self.export_overlay = Some((profile, value));
    }
    pub fn take_imported_overlay(&mut self) -> Option<(String, Value)> {
        self.imported_overlay.take()
    }
    pub fn take_changes(&mut self) -> Vec<ProfileChange> {
        std::mem::take(&mut self.changes)
    }
    pub fn blocks_owner_change(&self) -> bool {
        self.busy
            || self.normalizing
            || self.generating
            || self.dirty()
            || self.dialog.is_some()
            || self.confirm_model.is_some()
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        if self.busy {
            return vec![];
        }
        self.loading = true;
        vec![(
            "bots.list".into(),
            PageRequest::rpc("profiles.list", json!({"include_sessions":true})),
        )]
    }
    pub fn poll(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        self.redraw |= self.poll_avatar();
        vec![]
    }
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }
    pub fn wake_after(&self) -> Option<std::time::Duration> {
        None
    }
    pub fn selected_row(&self) -> Option<&Value> {
        let name = self.selected.as_deref()?;
        self.roster
            .iter()
            .find(|r| r["name"].as_str() == Some(name))
    }
    pub fn edit_detached(&mut self) -> &mut TextEdit {
        &mut self.detached
    }
    pub fn fixture(&mut self) {
        self.demo = true;
        self.images = Some(assets::fixture_cache());
        self.apply("bots.list",Ok(json!({"bot_mode_protocol":true,"profiles":[{"name":"default","is_default":true,"description":"Your everyday agent","model":"sample-model","provider":"sample","ui_meta":{"hermes-bots":{"title":"Hermes","shape":"circle"}},"ui_meta_revisions":{"hermes-bots":1}},{"name":"research","description":"Find and organize research","model":"sample-fast","provider":"sample","ui_meta":{"hermes-bots":{"title":"Research","shape":"hexagon"}},"ui_meta_revisions":{"hermes-bots":2}}]})));
        self.selected = Some("research".into());
        self.seed_selected_metadata();
        self.apply("bots.detail",Ok(json!({"name":"research","description":"Find and organize research","soul":"# Research\n\nHelp organize evidence and explain findings clearly.","model":{"provider":"sample","default":"sample-fast"},"skills":[{"name":"research","enabled":true},{"name":"writing","enabled":false}],"toolsets":[{"name":"web","label":"Web search","enabled":true},{"name":"terminal","enabled":true}],"mcp_servers":[]})));
        self.catalog = vec![
            json!({"slug":"sample","name":"Sample provider","models":["sample-model","sample-fast"]}),
        ];
        let args = rune::extra_args();
        if args.iter().any(|a| a == "--new-bot") {
            self.begin_new();
        }
        if let Some(tab) = args
            .windows(2)
            .find(|v| v[0] == "--bot-tab")
            .map(|v| v[1].clone())
        {
            self.tab = tab;
        }
        if let Some(dialog) = args
            .windows(2)
            .find(|v| v[0] == "--bot-dialog")
            .map(|v| v[1].as_str())
        {
            self.dialog = Some(
                match dialog {
                    "rename" => "rename:research",
                    "delete" => "delete:research",
                    v => v,
                }
                .into(),
            );
            if dialog == "setup" {
                self.setup_command = Some("hermes --profile research gateway setup".into());
            }
        }
    }
}
pub fn valid_name(name: &str) -> bool {
    let s = name.trim();
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}
pub fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
