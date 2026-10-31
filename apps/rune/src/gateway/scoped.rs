//! Explicit request provenance keeps routing aliases out of raw backend owners.
use super::*;
use prepared::{BackendTarget, ProfileProvenance};
use route::WireScope;
#[cfg(test)]
mod tests;

impl Gateway {
    pub(crate) fn selected_backend_profile(&self) -> Option<&str> {
        match self.prepared.policy()?.selected()?.target() {
            BackendTarget::Named(name) => Some(name),
            BackendTarget::Root(proof) => proof.profile.as_deref(),
        }
    }
    pub(crate) fn ensure_profile_scope(
        &mut self,
        provenance: &ProfileProvenance,
    ) -> Result<(), String> {
        if !matches!(provenance, ProfileProvenance::NativeSelf) || self.selected_root_verified {
            return Ok(());
        }
        let proof = match self
            .prepared
            .policy()
            .and_then(|policy| policy.selected())
            .map(|context| context.target())
        {
            Some(BackendTarget::Root(proof)) => proof.clone(),
            Some(BackendTarget::Named(_)) => return Ok(()),
            None => return Err("The Desktop route has no selected backend context".into()),
        };
        let value = self.rpc_until("runtime.describe", json!({}), Instant::now() + TIMEOUT)?;
        if value.get("profile_id").and_then(Value::as_str) != Some(&proof.profile_id)
            || value.get("instance_id").and_then(Value::as_str) != Some(&proof.instance_id)
            || value.get("authority_epoch").and_then(Value::as_u64) != Some(proof.authority_epoch)
        {
            return Err("The selected backend authority changed; resolve this source again".into());
        }
        self.selected_root_verified = true;
        Ok(())
    }
    pub(crate) fn rpc_params_for(
        &mut self,
        provenance: &ProfileProvenance,
        mut params: Value,
    ) -> Result<Value, String> {
        if !params.is_object() {
            return Err("Scoped RPC parameters must be an object".into());
        }
        let scope = match self.prepared.policy() {
            Some(policy) => policy.rpc_scope(provenance)?,
            None => match provenance {
                ProfileProvenance::Backend(name) => {
                    valid_scope(name)?;
                    WireScope::Named(name.clone())
                }
                ProfileProvenance::Ambient(Some(name)) => WireScope::Named(name.clone()),
                ProfileProvenance::Ambient(None) => WireScope::Omit,
                ProfileProvenance::NativeSelf => {
                    return Err("NativeSelf requires a captured selected backend context".into());
                }
            },
        };
        self.ensure_profile_scope(provenance)?;
        let object = params.as_object_mut().unwrap();
        match scope {
            WireScope::Named(name) => {
                object.insert("profile".into(), name.into());
            }
            WireScope::Omit => {
                object.remove("profile");
            }
        }
        Ok(params)
    }
    pub(crate) fn api_url_for(
        &self,
        provenance: &ProfileProvenance,
        method: &str,
        raw_path: &str,
    ) -> Result<Url, String> {
        // Validate original authority before a pure route helper serializes it.
        if !raw_path.starts_with("/api/") || raw_path.contains(['#', '\\']) {
            return Err("Invalid scoped gateway API route".into());
        }
        let original = self
            .base
            .join(raw_path.trim_start_matches('/'))
            .map_err(|_| "Invalid scoped gateway API route")?;
        prepared::require_scope(&self.base, &original)?;
        let api_prefix = self
            .base
            .join("api/")
            .map_err(|_| "Invalid gateway API prefix")?;
        if !original.path().starts_with(api_prefix.path()) {
            return Err("The scoped request escaped its API prefix".into());
        }
        let path = match self.prepared.policy() {
            Some(policy) => policy.rest_route(provenance, method, raw_path)?,
            None => match provenance {
                ProfileProvenance::Backend(name) => {
                    route::ProfilePolicy::identity().rest_self_query(name, method, raw_path)?
                }
                _ => return Err("The scoped request has no captured transport policy".into()),
            },
        };
        let url = self
            .base
            .join(path.trim_start_matches('/'))
            .map_err(|_| "Invalid scoped gateway API route")?;
        prepared::require_scope(&self.base, &url)?;
        Ok(url)
    }
    pub(crate) fn http_parts_for(
        &self,
        provenance: &ProfileProvenance,
        url: Url,
        deadline: Instant,
    ) -> Result<prepared::HttpParts, String> {
        self.http_parts_for_method(provenance, url, deadline, None)
    }
    pub(crate) fn http_parts_for_method(
        &self,
        provenance: &ProfileProvenance,
        url: Url,
        deadline: Instant,
        method: Option<reqwest::Method>,
    ) -> Result<prepared::HttpParts, String> {
        #[cfg(target_os = "linux")]
        let home = if matches!(self.auth, Auth::Native(_)) {
            match provenance {
                ProfileProvenance::Backend(name) => Some(
                    self.profile_homes
                        .get(name)
                        .ok_or("Refresh the owning backend profile")?
                        .as_path(),
                ),
                ProfileProvenance::NativeSelf => match self
                    .prepared
                    .policy()
                    .and_then(|policy| policy.selected())
                    .map(|context| context.target())
                {
                    Some(BackendTarget::Named(name)) => Some(
                        self.profile_homes
                            .get(name)
                            .ok_or("Refresh the selected backend profile")?
                            .as_path(),
                    ),
                    Some(BackendTarget::Root(proof)) => {
                        Some(std::path::Path::new(&proof.profile_id))
                    }
                    None => return Err("The Desktop route has no selected backend context".into()),
                },
                ProfileProvenance::Ambient(_) => {
                    match self.native_scope_profile(provenance, &url)? {
                        Some(name) => Some(
                            self.profile_homes
                                .get(&name)
                                .ok_or("Refresh the owning backend profile")?
                                .as_path(),
                        ),
                        None => None,
                    }
                }
            }
        } else {
            None
        };
        #[cfg(not(target_os = "linux"))]
        let home = {
            let _ = provenance;
            None
        };
        match method {
            Some(method) => self
                .prepared
                .prepare_http_method(method, url, home, deadline),
            None => self.prepared.prepare_http(url, home, deadline),
        }
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn native_scope_profile(
        &self,
        provenance: &ProfileProvenance,
        url: &Url,
    ) -> Result<Option<String>, String> {
        match provenance {
            ProfileProvenance::Backend(name) => Ok(Some(name.clone())),
            ProfileProvenance::NativeSelf => match self
                .prepared
                .policy()
                .and_then(|policy| policy.selected())
                .map(|context| context.target())
            {
                Some(BackendTarget::Named(name)) => Ok(Some(name.clone())),
                Some(BackendTarget::Root(_)) => Ok(None),
                None => Err("The Desktop route has no selected backend context".into()),
            },
            ProfileProvenance::Ambient(_) => Ok(url
                .query_pairs()
                .find(|(key, value)| {
                    key == "profile" && !value.is_empty() && value != "all" && value != "unfiltered"
                })
                .map(|(_, value)| value.into_owned())),
        }
    }
    /// Only known owner fields are checked; the response remains backend-labelled.
    pub(crate) fn validate_response_owner(
        &self,
        provenance: &ProfileProvenance,
        owner: &Value,
        name_key: &str,
        allow_unowned: bool,
    ) -> Result<(), String> {
        let name = optional_string(owner, name_key)?;
        let home = optional_string(owner, "profile_id")?;
        let instance = optional_string(owner, "instance_id")?;
        let epoch = match owner.get("authority_epoch") {
            None | Some(Value::Null) => None,
            Some(value) => Some(value.as_u64().ok_or("Invalid response authority epoch")?),
        };
        let target = match provenance {
            ProfileProvenance::Backend(name) => BackendTarget::Named(name.clone()),
            ProfileProvenance::NativeSelf => self
                .prepared
                .policy()
                .and_then(|policy| policy.selected())
                .ok_or("The Desktop route has no selected backend context")?
                .target()
                .clone(),
            ProfileProvenance::Ambient(_) => {
                return Err("An ambient request cannot assert a typed response owner".into());
            }
        };
        match target {
            BackendTarget::Named(expected) => {
                route::ProfilePolicy::identity().validate_response_owner(
                    name,
                    &expected,
                    allow_unowned,
                )?;
            }
            BackendTarget::Root(proof) => {
                if !self.selected_root_verified {
                    return Err("The selected root has not been verified on this socket".into());
                }
                if home.is_some_and(|home| home != proof.profile_id)
                    || instance.is_some_and(|instance| instance != proof.instance_id)
                    || epoch.is_some_and(|epoch| epoch != proof.authority_epoch)
                    || proof
                        .profile
                        .as_deref()
                        .zip(name)
                        .is_some_and(|(expected, actual)| expected != actual)
                {
                    return Err(
                        "The response contradicts the selected backend root authority".into(),
                    );
                }
            }
        }
        Ok(())
    }
}
fn optional_string<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err("Invalid response owner field".into()),
    }
}
