//! Plugin schema settings use Capabilities' shared editor and credential transaction.
use crate::pages::{PageEffect, PageRequest};
use serde_json::{Value, json};
pub struct Plugin {
    pub key: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub fields: usize,
}
pub struct Plugins {
    pub rows: Vec<Plugin>,
    pub loading: bool,
    pub error: Option<String>,
}
impl Plugins {
    pub fn new() -> Self {
        Self {
            rows: vec![],
            loading: false,
            error: None,
        }
    }
    pub fn load(&mut self) -> Vec<PageEffect> {
        self.loading = true;
        self.error = None;
        vec![(
            "settings.plugins.list".into(),
            PageRequest::rpc("plugins.manage", json!({"action":"list"})),
        )]
    }
    pub fn apply(&mut self, result: Result<Value, String>) -> Vec<PageEffect> {
        self.loading = false;
        match result {
            Err(e) => self.error = Some(e),
            Ok(v) => {
                let Some(rows) = v["plugins"].as_array() else {
                    self.error = Some("Invalid agent plugin list".into());
                    return vec![];
                };
                self.rows = rows
                    .iter()
                    .filter_map(|p| {
                        let fields = p["settings_schema"].as_array()?.len();
                        if fields == 0 {
                            return None;
                        }
                        Some(Plugin {
                            key: p["key"].as_str().or_else(|| p["name"].as_str())?.into(),
                            name: p["name"].as_str().unwrap_or("").into(),
                            version: p["version"].as_str().unwrap_or("").into(),
                            description: p["description"].as_str().unwrap_or("").into(),
                            fields,
                        })
                    })
                    .collect();
            }
        }
        vec![]
    }
}
