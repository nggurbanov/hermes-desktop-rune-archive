//! The retained Hermes `model.options` read contract. No settings are written.
use std::collections::HashSet;

use super::{Gateway, TIMEOUT, Value, json, valid_scope};

const MAX_PROVIDERS: usize = 128;
const MAX_MODELS_PER_PROVIDER: usize = 4096;
const MAX_MODELS: usize = 16_384;
const MAX_ID_BYTES: usize = 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelCatalog {
    pub current_provider: Option<String>,
    pub current_model: Option<String>,
    pub providers: Vec<ModelProvider>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelProvider {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,
    pub authenticated: Option<bool>,
    pub models: Vec<CatalogModel>,
}

impl ModelProvider {
    pub fn matches(&self, provider: &str) -> bool {
        self.id == provider
            || self.label == provider
            || self.aliases.iter().any(|alias| alias == provider)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogModel {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub capabilities: Option<ModelCapabilities>,
}

/// Unknown is distinct from false. Hermes advertises support flags, not a
/// route-specific reasoning-level list or a current effort/service-tier value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub reasoning: Option<bool>,
    pub can_disable_reasoning: Option<bool>,
    pub fast: Option<bool>,
    pub ultrafast: Option<bool>,
}

impl ModelCatalog {
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let providers = value
            .get("providers")
            .and_then(Value::as_array)
            .ok_or("Invalid model catalog providers")?;
        if providers.len() > MAX_PROVIDERS {
            return Err("Model catalog exceeds the provider limit".into());
        }
        let mut result = Self {
            current_provider: optional_text(value.get("provider"))?,
            current_model: optional_text(value.get("model"))?,
            providers: Vec::with_capacity(providers.len()),
        };
        let mut provider_ids = HashSet::new();
        let mut total_models = 0usize;
        for provider in providers {
            let id = required_text(provider.get("slug"))?;
            if !provider_ids.insert(id.clone()) {
                return Err("Model catalog has duplicate provider identities".into());
            }
            let label = optional_text(provider.get("name"))?.unwrap_or_else(|| id.clone());
            let models = optional_strings(provider.get("models"), MAX_MODELS_PER_PROVIDER)?;
            total_models = total_models
                .checked_add(models.len())
                .ok_or("Model catalog exceeds the model limit")?;
            if total_models > MAX_MODELS {
                return Err("Model catalog exceeds the model limit".into());
            }
            let unavailable: HashSet<String> =
                optional_strings(provider.get("unavailable_models"), MAX_MODELS_PER_PROVIDER)?
                    .into_iter()
                    .collect();
            let capabilities = match provider.get("capabilities") {
                None | Some(Value::Null) => None,
                Some(Value::Object(map)) => Some(map),
                _ => return Err("Invalid model catalog capabilities".into()),
            };
            let mut seen = HashSet::new();
            let mut entries = Vec::with_capacity(models.len());
            for model in models {
                if !seen.insert(model.clone()) {
                    continue;
                }
                let caps = capabilities.and_then(|map| map.get(&model));
                let capabilities = match caps {
                    None | Some(Value::Null) => None,
                    Some(value) if value.is_object() => Some(ModelCapabilities {
                        reasoning: optional_bool(value.get("reasoning"))?,
                        can_disable_reasoning: optional_bool(value.get("can_disable_reasoning"))?,
                        fast: optional_bool(value.get("fast"))?,
                        ultrafast: optional_bool(value.get("ultrafast"))?,
                    }),
                    _ => return Err("Invalid model catalog capabilities".into()),
                };
                entries.push(CatalogModel {
                    label: model.clone(),
                    available: !unavailable.contains(&model),
                    id: model,
                    capabilities,
                });
            }
            result.providers.push(ModelProvider {
                id,
                label,
                aliases: optional_strings(provider.get("aliases"), 64)?,
                authenticated: optional_bool(provider.get("authenticated"))?,
                models: entries,
            });
        }
        Ok(result)
    }

    fn has_models(&self) -> bool {
        self.providers
            .iter()
            .any(|provider| !provider.models.is_empty())
    }
}

impl Gateway {
    /// A separate fallible read: callers keep catalog errors outside history
    /// state. The REST recovery stays on this exact gateway and profile.
    pub fn model_catalog(&mut self, profile: &str) -> Result<ModelCatalog, String> {
        valid_scope(profile)?;
        self.model_catalog_for(&super::prepared::ProfileProvenance::Backend(profile.into()))
    }
    pub(crate) fn model_catalog_for(
        &mut self,
        provenance: &super::prepared::ProfileProvenance,
    ) -> Result<ModelCatalog, String> {
        let deadline = std::time::Instant::now() + TIMEOUT;
        let params = self.rpc_params_for(provenance, json!({"explicit_only":true}))?;
        let rpc = match self.rpc_until("model.options", params, deadline) {
            Ok(value) => {
                self.validate_response_owner(provenance, &value, "profile", true)?;
                ModelCatalog::from_value(&value)
            }
            Err(error) => Err(error),
        };
        if rpc.as_ref().is_ok_and(ModelCatalog::has_models) {
            return rpc;
        }
        let path = super::audio::scoped_path(self, provenance, "/api/model/options")?;
        let path = format!("{path}&explicit_only=1");
        let url = self.api_url_for(provenance, "GET", &path)?;
        #[cfg(target_os = "linux")]
        if matches!(&self.auth, super::Auth::Native(_))
            && self
                .native_scope_profile(provenance, &url)?
                .is_some_and(|name| !self.profile_homes.contains_key(&name))
        {
            self.profiles_until(deadline)?;
        }
        let parts =
            self.http_parts_for_method(provenance, url, deadline, Some(reqwest::Method::GET))?;
        let rest = match self.get_json_parts(parts, deadline, "Model catalog") {
            Ok(value) => {
                self.validate_response_owner(provenance, &value, "profile", true)?;
                ModelCatalog::from_value(&value)
            }
            Err(error) => Err(error),
        };
        if let Ok(mut catalog) = rest
            && catalog.has_models()
        {
            if let Ok(snapshot) = &rpc {
                if snapshot.current_provider.is_some() {
                    catalog
                        .current_provider
                        .clone_from(&snapshot.current_provider);
                }
                if snapshot.current_model.is_some() {
                    catalog.current_model.clone_from(&snapshot.current_model);
                }
            }
            return Ok(catalog);
        }
        rpc
    }
}

fn required_text(value: Option<&Value>) -> Result<String, String> {
    optional_text(value)?.ok_or_else(|| "Invalid model catalog identity".into())
}

fn optional_text(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.is_empty() => Ok(None),
        Some(Value::String(text))
            if text.len() <= MAX_ID_BYTES
                && !text.trim().is_empty()
                && !text.chars().any(char::is_control) =>
        {
            Ok(Some(text.clone()))
        }
        _ => Err("Invalid model catalog identity".into()),
    }
}

fn optional_strings(value: Option<&Value>, max: usize) -> Result<Vec<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(values)) if values.len() <= max => values
            .iter()
            .map(|value| required_text(Some(value)))
            .collect(),
        _ => Err("Invalid or oversized model catalog list".into()),
    }
}

fn optional_bool(value: Option<&Value>) -> Result<Option<bool>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        _ => Err("Invalid model catalog capability flag".into()),
    }
}

#[cfg(test)]
mod tests;
