//! Preserve non-visual route keys while editing ordered fallback model pairs.
use super::Settings;
use rune::TextEdit;
use serde_json::{Value, json};
#[derive(Clone)]
pub struct FallbackRow {
    pub provider: TextEdit,
    pub model: TextEdit,
    extra: Value,
}
impl FallbackRow {
    pub fn empty() -> Self {
        Self {
            provider: TextEdit::new(""),
            model: TextEdit::new(""),
            extra: json!({}),
        }
    }
    fn from(value: &Value) -> Self {
        if let Some(raw) = value.as_str() {
            let (provider, model) = raw.split_once('/').unwrap_or(("", raw));
            return Self {
                provider: TextEdit::new(provider),
                model: TextEdit::new(model),
                extra: json!({}),
            };
        }
        Self {
            provider: TextEdit::new(value["provider"].as_str().unwrap_or("")),
            model: TextEdit::new(value["model"].as_str().unwrap_or("")),
            extra: if value.is_object() {
                value.clone()
            } else {
                json!({})
            },
        }
    }
    fn complete(&self) -> Option<Value> {
        let provider = self.provider.text.trim();
        let model = self.model.text.trim();
        if provider.is_empty() || model.is_empty() {
            return None;
        }
        let mut result = self.extra.clone();
        result["provider"] = json!(provider);
        result["model"] = json!(model);
        Some(result)
    }
}
impl Settings {
    pub(super) fn refresh_fallback(&mut self) {
        let Some(field) = self.fields.iter().find(|f| f.key == "fallback_providers") else {
            return;
        };
        let value = serde_json::from_str::<Value>(&field.editor.text).unwrap_or_else(|_| json!([]));
        let existing = Value::Array(
            self.fallback_rows
                .iter()
                .filter_map(FallbackRow::complete)
                .collect(),
        );
        if !self.fallback_rows.is_empty() && value == existing {
            return;
        }
        self.fallback_rows = value
            .as_array()
            .map(|a| a.iter().map(FallbackRow::from).collect())
            .unwrap_or_default();
    }
    pub fn sync_fallback(&mut self) {
        let value = Value::Array(
            self.fallback_rows
                .iter()
                .filter_map(FallbackRow::complete)
                .collect(),
        );
        if let Some(field) = self
            .fields
            .iter_mut()
            .find(|f| f.key == "fallback_providers")
        {
            field.editor.replace_text(value.to_string());
            self.changed();
        }
    }
    pub fn fallback_edit_mut(&mut self, index: usize, provider: bool) -> &mut TextEdit {
        self.fallback_rows
            .get_mut(index)
            .map(|r| {
                if provider {
                    &mut r.provider
                } else {
                    &mut r.model
                }
            })
            .unwrap_or(&mut self.detached)
    }
}
