//! Immutable wire scope policy ported from the retained Desktop routing table.
//! Routing labels, declared wire selectors and response ownership are distinct.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverrideKind {
    #[default]
    None,
    Url,
    Ssh,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteOptions {
    pub primary_profile: Option<String>,
    pub global_remote: bool,
    pub profile_remote_override: bool,
    pub primary_remote_active: bool,
    pub own_entry: bool,
    pub isolated_backend: bool,
    pub backend_profile: Option<String>,
    pub override_kind: OverrideKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Primary,
    Pool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendRoute {
    pub backend: Backend,
    pub descriptor_profile: Option<String>,
    pub scope_path: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireScope {
    Named(String),
    Omit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerEvidence {
    Named,
    Unowned,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Legacy {
    desktop: String,
    options: RouteOptions,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProfilePolicy {
    legacy: Option<Legacy>,
}
/// Only the inherited private parent/child control pipe carries this representation.
/// Import must validate the captured policy rather than accepting normalisation as a repair.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfilePolicySnapshot {
    version: u32,
    legacy: Option<Legacy>,
}
impl ProfilePolicy {
    /// Existing Saved/Direct transports already carry an explicit wire owner.
    pub fn identity() -> Self {
        Self { legacy: None }
    }
    pub fn new(desktop: &str, mut options: RouteOptions) -> Result<Self, String> {
        let desktop = desktop.trim();
        if !valid_profile(desktop) {
            return Err("Invalid Desktop profile routing label".into());
        }
        let primary = options
            .primary_profile
            .as_deref()
            .unwrap_or("default")
            .trim();
        let primary = if primary.is_empty() {
            "default"
        } else {
            primary
        };
        if !valid_profile(primary) {
            return Err("Invalid pinned primary profile".into());
        }
        options.primary_profile = Some(primary.into());
        options.backend_profile = options
            .backend_profile
            .as_deref()
            .map(str::trim)
            .filter(|name| valid_mapping(name))
            .map(str::to_owned);
        Ok(Self {
            legacy: Some(Legacy {
                desktop: desktop.into(),
                options,
            }),
        })
    }
    pub fn desktop_profile(&self) -> Option<&str> {
        self.legacy.as_ref().map(|p| p.desktop.as_str())
    }
    pub(crate) fn private_ipc_snapshot(&self) -> ProfilePolicySnapshot {
        ProfilePolicySnapshot {
            version: 1,
            legacy: self.legacy.clone(),
        }
    }
    pub(crate) fn from_private_ipc(snapshot: ProfilePolicySnapshot) -> Result<Self, String> {
        if snapshot.version != 1 {
            return Err("Unsupported inherited profile policy".into());
        }
        let Some(legacy) = snapshot.legacy else {
            return Ok(Self::identity());
        };
        let validated = Self::new(&legacy.desktop, legacy.options.clone())?;
        if validated.legacy.as_ref() != Some(&legacy) {
            return Err("Invalid inherited profile policy".into());
        }
        Ok(validated)
    }
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let encoded = serde_json::to_vec(self).expect("profile policy is serializable");
        format!(
            "{:x}",
            Sha256::digest([b"hermes-profile-policy-v1\0".as_slice(), &encoded].concat())
        )
    }
    /// No path is descriptor/socket resolution, not an unscopable REST write.
    pub fn backend_route(&self, method: &str, path: &str) -> BackendRoute {
        let Some(p) = &self.legacy else {
            return BackendRoute {
                backend: Backend::Primary,
                descriptor_profile: None,
                scope_path: true,
            };
        };
        let o = &p.options;
        let named = || BackendRoute {
            backend: Backend::Primary,
            descriptor_profile: Some(p.desktop.clone()),
            scope_path: true,
        };
        let pooled = || BackendRoute {
            backend: Backend::Pool,
            descriptor_profile: None,
            scope_path: false,
        };
        let local_scope = local_request_scope(method, path);
        if Some(p.desktop.as_str()) == o.primary_profile.as_deref() {
            if o.global_remote {
                return named();
            }
            return BackendRoute {
                backend: Backend::Primary,
                descriptor_profile: local_scope.filter(|v| *v).map(|_| p.desktop.clone()),
                scope_path: local_scope == Some(true),
            };
        }
        if o.profile_remote_override {
            return pooled();
        }
        if o.global_remote {
            return named();
        }
        if o.primary_remote_active {
            return if o.own_entry { pooled() } else { named() };
        }
        if let Some(scope_path) = local_scope {
            return BackendRoute {
                backend: Backend::Primary,
                descriptor_profile: scope_path.then(|| p.desktop.clone()),
                scope_path,
            };
        }
        let unscopable_write =
            !path.is_empty() && !matches!(method_name(method).as_str(), "GET" | "HEAD" | "OPTIONS");
        if !o.isolated_backend && !unscopable_write {
            named()
        } else {
            pooled()
        }
    }
    /// Legacy generic RPC wrapper: shared-primary scope overwrites the original
    /// field; primary and pooled sockets preserve both its presence and value.
    /// Explicit native-self/backend requests use their captured target instead.
    pub fn ambient_rpc_scope(&self, original: Option<&str>) -> Result<WireScope, String> {
        if let WireScope::Named(profile) = self.rpc_scope(None)? {
            return Ok(WireScope::Named(profile));
        }
        match original {
            Some(value) => {
                valid_selector(value)?;
                Ok(WireScope::Named(value.to_owned()))
            }
            None => Ok(WireScope::Omit),
        }
    }
    /// `None` is ambient wrapper injection; `Some` is an already-declared field.
    /// An explicit RPC `default` is named, even though SSH argv omits it.
    pub fn rpc_scope(&self, selector: Option<&str>) -> Result<WireScope, String> {
        if let Some(selector) = selector {
            let selector = selector.trim();
            valid_selector(selector)?;
            if selector.is_empty() {
                return Ok(WireScope::Omit);
            }
            return Ok(WireScope::Named(self.map_self(selector).into()));
        }
        let Some(p) = &self.legacy else {
            return Ok(WireScope::Omit);
        };
        // gatewayForProfile's primary-socket fast path does not add an owner.
        if Some(p.desktop.as_str()) == p.options.primary_profile.as_deref() {
            return Ok(WireScope::Omit);
        }
        Ok(self
            .backend_route("", "")
            .descriptor_profile
            .map(WireScope::Named)
            .unwrap_or(WireScope::Omit))
    }
    /// Changes only declared profile query keys. Bodies/text are not inputs.
    pub fn rest_self_query(
        &self,
        requested: &str,
        method: &str,
        path: &str,
    ) -> Result<String, String> {
        let requested = requested.trim();
        valid_selector(requested)?;
        if path.is_empty() {
            return Ok(path.into());
        }
        let mut url = parse_path(path).ok_or("Invalid profile request path")?;
        if let Some(p) = &self.legacy {
            if let Some(backend) = p.options.backend_profile.as_deref() {
                let mut pairs = url.query_pairs().into_owned().collect::<Vec<_>>();
                let mut changed = replace_self_query(&mut pairs, "profile", &p.desktop, backend);
                if url.path() == "/api/profiles/sessions/sidebar" {
                    changed |=
                        replace_self_query(&mut pairs, "recents_profile", &p.desktop, backend);
                }
                if changed {
                    url.query_pairs_mut().clear().extend_pairs(pairs);
                    return Ok(relative_path(&url));
                }
            }
        }
        if requested.is_empty()
            || !self.backend_route(method, path).scope_path
            || url.query_pairs().any(|(key, _)| key == "profile")
            || profile_target_in_path(url.path(), method)
        {
            return Ok(path.into());
        }
        url.query_pairs_mut().append_pair("profile", requested);
        Ok(relative_path(&url))
    }
    /// Pre-REST selector used by fetchRemoteProfileSessions. Apply REST policy
    /// afterwards, as fetchJsonForProfile does; URL aliases are handled there.
    pub fn session_aggregate_scope(&self, requested: &str) -> Result<String, String> {
        let requested = requested.trim();
        valid_selector(requested)?;
        if let Some(p) = &self.legacy {
            if requested == p.desktop && p.options.override_kind == OverrideKind::Ssh {
                if let Some(backend) = p
                    .options
                    .backend_profile
                    .as_deref()
                    .filter(|v| *v != "default")
                {
                    return Ok(backend.into());
                }
            }
        }
        Ok(requested.into())
    }
    /// Eligibility only: the caller retains the exact transport/guard and may
    /// retry once. No auth/network/5xx or individual-session fallback exists.
    pub fn aggregate_get_retry_unscoped(&self, method: &str, path: &str, status: u16) -> bool {
        let Some(p) = &self.legacy else { return false };
        if !p.options.profile_remote_override
            || method_name(method) != "GET"
            || !matches!(status, 400 | 404)
        {
            return false;
        }
        let Some(url) = parse_path(path) else {
            return false;
        };
        if url.path() != "/api/sessions" {
            return false;
        }
        let Some(scope) = url
            .query_pairs()
            .find(|(key, _)| key == "profile")
            .map(|(_, value)| value.into_owned())
        else {
            return false;
        };
        scope == p.desktop || p.options.backend_profile.as_ref() == Some(&scope)
    }
    pub fn validate_response_owner(
        &self,
        actual: Option<&str>,
        expected_backend: &str,
        allow_unowned_legacy: bool,
    ) -> Result<OwnerEvidence, String> {
        let expected_backend = expected_backend.trim();
        valid_selector(expected_backend)?;
        if expected_backend.is_empty() {
            return Err("A proved backend profile is required before projecting an owner".into());
        }
        match actual.map(str::trim).filter(|v| !v.is_empty()) {
            Some(actual) if actual == expected_backend => Ok(OwnerEvidence::Named),
            Some(_) => Err("The response belongs to another backend profile".into()),
            None if allow_unowned_legacy => Ok(OwnerEvidence::Unowned),
            None => Err("The response did not prove its backend profile owner".into()),
        }
    }
    pub fn ssh_profile_arg(&self) -> Option<String> {
        let p = self.legacy.as_ref()?;
        let name = p.options.backend_profile.as_deref().unwrap_or(&p.desktop);
        (name != "default").then(|| name.into())
    }
    fn map_self<'a>(&'a self, selector: &'a str) -> &'a str {
        self.legacy
            .as_ref()
            .filter(|p| p.desktop == selector)
            .and_then(|p| p.options.backend_profile.as_deref())
            .unwrap_or(selector)
    }
}
fn valid_selector(value: &str) -> Result<(), String> {
    if value.len() > 1024 || value.chars().any(char::is_control) {
        Err("Invalid profile selector".into())
    } else {
        Ok(())
    }
}
fn valid_profile(value: &str) -> bool {
    let b = value.as_bytes();
    (1..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(*c, b'_' | b'-'))
}
fn valid_mapping(value: &str) -> bool {
    valid_profile(value) && !matches!(value, "hermes" | "test" | "tmp" | "root" | "sudo")
}
fn method_name(method: &str) -> String {
    if method.is_empty() {
        "GET".into()
    } else {
        method.to_ascii_uppercase()
    }
}
fn parse_path(path: &str) -> Option<url::Url> {
    url::Url::parse("http://hermes.invalid")
        .ok()?
        .join(path)
        .ok()
}
fn relative_path(url: &url::Url) -> String {
    let mut result = url.path().to_owned();
    if let Some(query) = url.query() {
        result.push('?');
        result.push_str(query);
    }
    if let Some(fragment) = url.fragment() {
        result.push('#');
        result.push_str(fragment);
    }
    result
}
fn replace_self_query(
    pairs: &mut Vec<(String, String)>,
    key: &str,
    desktop: &str,
    backend: &str,
) -> bool {
    if backend == desktop
        || !pairs
            .iter()
            .find(|(k, _)| k == key)
            .is_some_and(|(_, value)| value.trim() == desktop)
    {
        return false;
    }
    let mut replaced = false;
    pairs.retain_mut(|(k, value)| {
        if k != key {
            return true;
        }
        if replaced {
            return false;
        }
        *value = backend.into();
        replaced = true;
        true
    });
    true
}
fn profile_target_in_path(path: &str, method: &str) -> bool {
    let path = path.trim_end_matches('/');
    let Some(suffix) = path
        .strip_prefix("/api/profiles/")
        .filter(|v| !v.is_empty())
    else {
        return false;
    };
    let methods: &[&str] = match suffix {
        "active" => &["GET", "POST"],
        "import" => &["POST"],
        "projects/tree" => &["GET"],
        "sessions" => &["GET"],
        "sessions/pull-requests" => &["POST"],
        "sessions/sidebar" => &["GET"],
        _ => return true,
    };
    !method.is_empty() && !methods.contains(&method_name(method).as_str())
}
fn local_request_scope(method: &str, path: &str) -> Option<bool> {
    if path.is_empty() {
        return None;
    }
    let parsed = parse_path(path)?;
    let path = parsed.path();
    let method = method_name(method);
    if LOCAL_SCOPED
        .iter()
        .any(|(verb, route)| *verb == method && *route == path)
        || path.starts_with("/api/actions/")
        || (method == "GET" && (path == "/api/sessions" || path.starts_with("/api/sessions/")))
        || path == "/api/tools"
        || path.starts_with("/api/tools/")
        || path == "/api/webhooks"
        || path.starts_with("/api/webhooks/")
        || path.starts_with("/api/ops/")
    {
        return Some(true);
    }
    if path == "/api/profiles"
        || path.starts_with("/api/profiles/")
        || (method != "GET" && (path == "/api/sessions" || path.starts_with("/api/sessions/")))
    {
        return Some(false);
    }
    None
}
const LOCAL_SCOPED: &[(&str, &str)] = &[
    ("GET", "/api/config"),
    ("PUT", "/api/config"),
    ("GET", "/api/config/raw"),
    ("PUT", "/api/config/raw"),
    ("GET", "/api/config/schema"),
    ("DELETE", "/api/env"),
    ("GET", "/api/env"),
    ("PUT", "/api/env"),
    ("POST", "/api/env/reveal"),
    ("GET", "/api/model/auxiliary"),
    ("GET", "/api/model/info"),
    ("GET", "/api/model/moa"),
    ("PUT", "/api/model/moa"),
    ("GET", "/api/model/options"),
    ("POST", "/api/model/set"),
    ("GET", "/api/skills"),
    ("GET", "/api/skills/content"),
    ("PUT", "/api/skills/toggle"),
    ("POST", "/api/skills/hub/install"),
    ("GET", "/api/skills/hub/official"),
    ("GET", "/api/skills/hub/preview"),
    ("GET", "/api/skills/hub/scan"),
    ("GET", "/api/skills/hub/search"),
    ("GET", "/api/skills/hub/sources"),
    ("POST", "/api/skills/hub/uninstall"),
    ("POST", "/api/skills/hub/update"),
    ("POST", "/api/mcp/catalog/install"),
    ("POST", "/api/gateway/restart"),
    ("POST", "/api/gateway/start"),
    ("POST", "/api/gateway/stop"),
    ("GET", "/api/memory"),
    ("PUT", "/api/memory/provider"),
    ("POST", "/api/memory/reset"),
    ("GET", "/api/curator"),
    ("PUT", "/api/curator/paused"),
    ("POST", "/api/curator/run"),
    ("GET", "/api/logs"),
    ("GET", "/api/portal"),
    ("GET", "/api/hermes/update/check"),
    ("POST", "/api/local-models/activate"),
    ("GET", "/api/dashboard/themes"),
    ("PUT", "/api/dashboard/theme"),
    ("GET", "/api/dashboard/font"),
    ("PUT", "/api/dashboard/font"),
    ("GET", "/api/dashboard/plugins"),
    ("GET", "/api/webapp-access/status"),
    ("POST", "/api/webapp-access/start"),
    ("POST", "/api/webapp-access/stop"),
    ("POST", "/api/webapp-access/restart"),
];
#[cfg(test)]
mod tests;
