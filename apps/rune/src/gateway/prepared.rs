//! Captured gateway request policy shared by HTTP and WebSocket consumers.
use super::{
    Auth, Connection, endpoint_url,
    route::{ProfilePolicy, ProfilePolicySnapshot},
};
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{any::Any, collections::BTreeMap, fmt, path::Path, sync::Arc, time::Instant};
use tungstenite::client::IntoClientRequest;
use url::Url;
mod selected;
use selected::SelectedContextSnapshot;
pub(crate) use selected::{BackendTarget, ProfileProvenance, SelectedContext};

/// Resolved access-proxy credentials. These are not the gateway's own auth.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct AccessHeaders(BTreeMap<String, String>);
impl fmt::Debug for AccessHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessHeaders")
            .field("names", &self.0.keys().collect::<Vec<_>>())
            .finish()
    }
}
impl AccessHeaders {
    pub(crate) fn new(raw: BTreeMap<String, String>) -> Result<Self, String> {
        let mut values = BTreeMap::new();
        for (name, value) in raw {
            let name = name.trim().to_ascii_lowercase();
            if HeaderName::from_bytes(name.as_bytes()).is_err() || forbidden(&name) {
                continue;
            }
            let value = value.replace(['\r', '\n'], "").trim().to_owned();
            if value.is_empty() {
                continue;
            }
            HeaderValue::from_str(&value).map_err(|_| "Invalid access-header value")?;
            if values.insert(name, value).is_some() {
                return Err("Access headers contain an ambiguous duplicate name".into());
            }
        }
        Ok(Self(values))
    }
    pub(crate) fn for_request(&self, base: &Url, request: &Url) -> Result<HeaderMap, String> {
        require_scope(base, request)?;
        let mut headers = HeaderMap::new();
        for (name, value) in &self.0 {
            let mut value = HeaderValue::from_str(value).expect("validated access-header value");
            value.set_sensitive(true);
            headers.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("validated access-header name"),
                value,
            );
        }
        Ok(headers)
    }
    pub(crate) fn fingerprint(&self) -> String {
        let bytes = serde_json::to_vec(&self.0).expect("string header map serializes");
        format!("{:x}", Sha256::digest(bytes))
    }
}
/// Retained Cloud authentication, owned by the connection resolver.
pub(crate) trait CloudRuntime: Send + Sync {
    fn prepare_http(
        &self,
        method: Method,
        url: &Url,
        deadline: Instant,
    ) -> Result<CloudHttp, String>;
    fn retry_http_unauthorized(
        &self,
        previous: CloudHttp,
        deadline: Instant,
    ) -> Result<CloudHttp, String>;
    fn prepare_ws(
        &self,
        url: &Url,
        deadline: Instant,
    ) -> Result<tungstenite::handshake::client::Request, String>;
    fn runtime_fingerprint(&self) -> String;
}
pub(crate) struct CloudHttp {
    pub(crate) url: Url,
    pub(crate) headers: HeaderMap,
    pub(crate) receipt: Box<dyn Any + Send>,
}
/// New resolved Legacy URL routes opt into header-based HTTP token auth.
/// Existing plain URL and Saved inputs retain their compatibility behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WireAuth {
    Preserve,
    LegacyToken,
}
#[derive(Clone)]
pub struct WirePolicy {
    auth: WireAuth,
    profiles: ProfilePolicy,
    headers: AccessHeaders,
    selected: Option<SelectedContext>,
    cloud: Option<Arc<dyn CloudRuntime>>,
}
/// Only the private parent-to-child IPC channel may carry this credential material.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WirePolicySnapshot {
    version: u8,
    auth: WireAuth,
    profiles: ProfilePolicySnapshot,
    headers: BTreeMap<String, String>,
    #[serde(default)]
    selected: Option<SelectedContextSnapshot>,
}
impl fmt::Debug for WirePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WirePolicy")
            .field("auth", &self.auth)
            .field("profiles", &self.profiles)
            .field("headers", &self.headers)
            .field("selected", &self.selected)
            .finish()
    }
}
impl WirePolicy {
    pub(crate) fn native(profiles: ProfilePolicy) -> Self {
        Self {
            auth: WireAuth::Preserve,
            profiles,
            headers: AccessHeaders::default(),
            selected: None,
            cloud: None,
        }
    }
    pub(crate) fn token(
        profiles: ProfilePolicy,
        headers: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        Ok(Self {
            auth: WireAuth::LegacyToken,
            profiles,
            headers: AccessHeaders::new(headers)?,
            selected: None,
            cloud: None,
        })
    }
    pub(crate) fn cloud(
        profiles: ProfilePolicy,
        runtime: Arc<dyn CloudRuntime>,
    ) -> Result<Self, String> {
        if runtime.runtime_fingerprint().is_empty() {
            return Err("Cloud runtime has no identity".into());
        }
        Ok(Self {
            auth: WireAuth::Preserve,
            profiles,
            headers: AccessHeaders::default(),
            selected: None,
            cloud: Some(runtime),
        })
    }
    pub(crate) fn is_cloud(&self) -> bool {
        self.cloud.is_some()
    }
    pub(crate) fn profiles(&self) -> &ProfilePolicy {
        &self.profiles
    }
    pub(crate) fn with_profiles(&self, profiles: ProfilePolicy) -> Self {
        Self {
            auth: self.auth,
            profiles,
            headers: self.headers.clone(),
            selected: None,
            cloud: self.cloud.clone(),
        }
    }
    pub(crate) fn with_selected(&self, context: SelectedContext) -> Result<Self, String> {
        if self.profiles.desktop_profile() != Some(context.window_alias()) {
            return Err("The selected backend context belongs to a different Desktop route".into());
        }
        Ok(Self {
            auth: self.auth,
            profiles: self.profiles.clone(),
            headers: self.headers.clone(),
            selected: Some(context),
            cloud: self.cloud.clone(),
        })
    }
    pub(crate) fn selected(&self) -> Option<&SelectedContext> {
        self.selected.as_ref()
    }
    pub(crate) fn private_ipc_snapshot(&self) -> WirePolicySnapshot {
        WirePolicySnapshot {
            version: if self.cloud.is_some() { 0 } else { 1 },
            auth: self.auth,
            profiles: self.profiles.private_ipc_snapshot(),
            headers: self.headers.0.clone(),
            selected: self
                .selected
                .as_ref()
                .map(SelectedContext::private_ipc_snapshot),
        }
    }
    pub(crate) fn from_private_ipc(snapshot: WirePolicySnapshot) -> Result<Self, String> {
        if snapshot.version != 1 {
            return Err("Unsupported private transport policy version".into());
        }
        let headers = AccessHeaders::new(snapshot.headers.clone())?;
        if headers.0 != snapshot.headers
            || (snapshot.auth == WireAuth::Preserve && !headers.0.is_empty())
        {
            return Err("The private transport policy contains noncanonical access headers".into());
        }
        let policy = Self {
            auth: snapshot.auth,
            profiles: ProfilePolicy::from_private_ipc(snapshot.profiles)?,
            headers,
            selected: None,
            cloud: None,
        };
        match snapshot.selected {
            Some(context) => policy.with_selected(SelectedContext::from_private_ipc(context)?),
            None => Ok(policy),
        }
    }
    pub(crate) fn fingerprint(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{}:{}:{}:{}",
                    match self.auth {
                        WireAuth::Preserve => "preserve",
                        WireAuth::LegacyToken => "legacy-token",
                    },
                    self.profiles.fingerprint(),
                    self.headers.fingerprint(),
                    self.selected
                        .as_ref()
                        .map(SelectedContext::fingerprint)
                        .unwrap_or_default()
                )
                .as_bytes()
            )
        )
    }
    pub(crate) fn runtime_fingerprint(&self) -> String {
        let stable = match &self.cloud {
            Some(cloud) => format!("{}:{}", self.fingerprint(), cloud.runtime_fingerprint()),
            None => self.fingerprint(),
        };
        match self.selected.as_ref().map(SelectedContext::target) {
            Some(BackendTarget::Root(proof)) => {
                let authority = serde_json::json!([
                    stable,
                    proof.profile_id,
                    proof.instance_id,
                    proof.authority_epoch,
                    proof.profile
                ]);
                format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&authority).expect("runtime authority serializes")
                    )
                )
            }
            _ => stable,
        }
    }
}

#[derive(Clone)]
pub(crate) struct PreparedTransport {
    base: Url,
    auth: Auth,
    initial_profile: String,
    policy: Option<Arc<WirePolicy>>,
}

#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod wire_tests;
pub(crate) struct HttpParts {
    pub url: Url,
    pub headers: HeaderMap,
    pub method: Method,
    cloud: Option<CloudHttp>,
}
pub(crate) struct WsParts {
    pub url: Url,
    pub request: tungstenite::handshake::client::Request,
    pub private_protocol: bool,
}
#[derive(Clone, Copy)]
pub(crate) enum WsPurpose {
    Interactive,
    Plugin,
}
impl PreparedTransport {
    pub(crate) fn from_connection(
        connection: &Connection,
        deadline: Instant,
    ) -> Result<Self, String> {
        let mut actual = connection;
        let mut policy = None;
        loop {
            match actual {
                Connection::Saved { inner, .. } => actual = inner,
                Connection::Routed {
                    actual: inner,
                    policy: captured,
                    ..
                } => {
                    if policy.is_some() {
                        return Err("A connection cannot apply two route policies".into());
                    }
                    policy = Some(captured.clone());
                    actual = inner;
                }
                _ => break,
            }
        }
        let (base, auth, initial_profile) = match actual {
            Connection::Url { endpoint, token } => {
                let (base, embedded) = endpoint_url(endpoint)?;
                let token = token.clone().or(embedded);
                if policy
                    .as_ref()
                    .is_some_and(|p| p.auth == WireAuth::LegacyToken)
                {
                    let token = token
                        .as_ref()
                        .filter(|token| !token.trim().is_empty())
                        .ok_or("The selected Legacy URL requires its captured session token")?;
                    HeaderValue::from_str(token)
                        .map_err(|_| "The captured session token is not a valid HTTP credential")?;
                }
                (base, Auth::Token(token), "default".into())
            }
            Connection::Local { home } => {
                if policy
                    .as_ref()
                    .is_some_and(|p| p.auth == WireAuth::LegacyToken)
                {
                    return Err(
                        "A Legacy URL policy cannot replace local gateway authentication".into(),
                    );
                }
                #[cfg(target_os = "linux")]
                {
                    let endpoint = super::local::identify_until(home, deadline)?;
                    let base =
                        Url::parse(&endpoint.origin).map_err(|_| "Invalid local gateway origin")?;
                    let profile = endpoint.profile_name();
                    (base, Auth::Native(endpoint), profile)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = (home, deadline);
                    return Err("Native local attachment is currently supported on Linux; use a gateway URL".into());
                }
            }
            Connection::Saved { .. } | Connection::Routed { .. } => {
                unreachable!("transport wrappers were resolved")
            }
        };
        Ok(Self {
            base,
            auth,
            initial_profile,
            policy,
        })
    }
    pub(crate) fn base(&self) -> &Url {
        &self.base
    }
    pub(crate) fn initial_profile(&self) -> &str {
        &self.initial_profile
    }
    pub(crate) fn policy(&self) -> Option<&WirePolicy> {
        self.policy.as_deref()
    }
    pub(super) fn auth(&self) -> &Auth {
        &self.auth
    }
    fn legacy_token(&self) -> bool {
        self.policy
            .as_ref()
            .is_some_and(|p| p.auth == WireAuth::LegacyToken)
    }
    fn access_headers(&self, url: &Url) -> Result<HeaderMap, String> {
        match &self.policy {
            Some(policy) => policy.headers.for_request(&self.base, url),
            None => {
                require_scope(&self.base, url)?;
                Ok(HeaderMap::new())
            }
        }
    }
    pub(crate) fn prepare_http(
        &self,
        url: Url,
        home: Option<&Path>,
        deadline: Instant,
    ) -> Result<HttpParts, String> {
        if self.policy.as_ref().is_some_and(|p| p.cloud.is_some()) {
            return Err("Cloud HTTP requires an explicit request method".into());
        }
        self.prepare_http_method(Method::GET, url, home, deadline)
    }
    pub(crate) fn prepare_http_method(
        &self,
        method: Method,
        mut url: Url,
        home: Option<&Path>,
        deadline: Instant,
    ) -> Result<HttpParts, String> {
        require_scope(&self.base, &url)?;
        if let Some(runtime) = self.policy.as_ref().and_then(|p| p.cloud.as_ref()) {
            let mut cloud = runtime.prepare_http(method.clone(), &url, deadline)?;
            if cloud.url != url {
                return Err("Cloud authentication changed the request URL".into());
            }
            validate_cloud_headers(&mut cloud.headers)?;
            return Ok(HttpParts {
                url: cloud.url.clone(),
                headers: cloud.headers.clone(),
                method,
                cloud: Some(cloud),
            });
        }
        let mut headers = self.access_headers(&url)?;
        match &self.auth {
            Auth::Token(Some(token)) if self.legacy_token() => {
                if url.query_pairs().any(|(key, _)| key == "token") {
                    return Err("The route cannot replace captured gateway authentication".into());
                }
                headers.insert("X-Hermes-Session-Token", sensitive(token)?);
            }
            Auth::Token(Some(token)) => {
                url.query_pairs_mut().append_pair("token", token);
            }
            Auth::Token(None) => {}
            #[cfg(target_os = "linux")]
            Auth::Native(endpoint) => {
                let ticket = endpoint.ticket_until(
                    home.unwrap_or(&endpoint.profile_id),
                    "native-http",
                    deadline,
                )?;
                headers.insert("X-Hermes-Gateway-Ticket", sensitive(&ticket)?);
            }
        }
        Ok(HttpParts {
            url,
            headers,
            method,
            cloud: None,
        })
    }
    pub(crate) fn retry_http_unauthorized(
        &self,
        parts: HttpParts,
        deadline: Instant,
    ) -> Result<HttpParts, String> {
        if parts.method != Method::GET {
            return Err("Only Cloud GET requests can be retried".into());
        }
        let runtime = self
            .policy
            .as_ref()
            .and_then(|p| p.cloud.as_ref())
            .ok_or("No Cloud runtime")?;
        let previous = parts.cloud.ok_or("No Cloud retry receipt")?;
        let mut cloud = runtime.retry_http_unauthorized(previous, deadline)?;
        if cloud.url != parts.url {
            return Err("Cloud refresh changed the request URL".into());
        }
        require_scope(&self.base, &cloud.url)?;
        validate_cloud_headers(&mut cloud.headers)?;
        Ok(HttpParts {
            url: cloud.url.clone(),
            headers: cloud.headers.clone(),
            method: Method::GET,
            cloud: Some(cloud),
        })
    }
    /// Execute the actual request and refresh only a confirmed unauthorized Cloud GET.
    pub(crate) fn send_http<F>(
        &self,
        parts: HttpParts,
        deadline: Instant,
        replay_safe: bool,
        mut send: F,
    ) -> Result<reqwest::blocking::Response, String>
    where
        F: FnMut(&HttpParts) -> Result<reqwest::blocking::Response, String>,
    {
        let response = send(&parts)?;
        if replay_safe
            && response.status() == reqwest::StatusCode::UNAUTHORIZED
            && parts.method == Method::GET
            && Self::has_cloud_receipt(&parts)
        {
            let retry = self.retry_http_unauthorized(parts, deadline)?;
            return send(&retry);
        }
        Ok(response)
    }
    pub(crate) fn has_cloud_receipt(parts: &HttpParts) -> bool {
        parts.cloud.is_some()
    }
    pub(crate) fn prepare_ws(
        &self,
        mut url: Url,
        home: Option<&Path>,
        purpose: WsPurpose,
        deadline: Instant,
    ) -> Result<WsParts, String> {
        require_scope(&self.base, &url)?;
        if let Some(runtime) = self.policy.as_ref().and_then(|p| p.cloud.as_ref()) {
            let request = runtime.prepare_ws(&url, deadline)?;
            let mut authenticated = Url::parse(&request.uri().to_string())
                .map_err(|_| "Cloud authentication returned an invalid WebSocket URL")?;
            let query = authenticated.query().unwrap_or("");
            let prefix = url
                .query()
                .filter(|q| !q.is_empty())
                .map(|q| format!("{q}&"))
                .unwrap_or_default();
            let ticket = query
                .strip_prefix(&prefix)
                .and_then(|q| q.strip_prefix("ticket="));
            if authenticated != url {
                if ticket.is_none_or(|value| value.is_empty() || value.contains('&')) {
                    return Err("Cloud authentication changed the WebSocket URL".into());
                }
                authenticated.set_query(url.query());
            }
            if authenticated != url {
                return Err("Cloud authentication changed the WebSocket URL".into());
            }
            return Ok(WsParts {
                url,
                request,
                private_protocol: false,
            });
        }
        let mut headers = self.access_headers(&url)?;
        let mut private_protocol = false;
        match &self.auth {
            Auth::Token(Some(token)) => {
                if self.legacy_token() && url.query_pairs().any(|(key, _)| key == "token") {
                    return Err("The route cannot replace captured gateway authentication".into());
                }
                url.query_pairs_mut().append_pair("token", token);
            }
            Auth::Token(None) => {}
            #[cfg(target_os = "linux")]
            Auth::Native(endpoint) => {
                let purpose = match purpose {
                    WsPurpose::Interactive => "interactive",
                    WsPurpose::Plugin => "native-http",
                };
                let ticket = endpoint.ticket_until(
                    home.unwrap_or(&endpoint.profile_id),
                    purpose,
                    deadline,
                )?;
                if purpose == "interactive" {
                    headers.insert(
                        "Sec-WebSocket-Protocol",
                        sensitive(&format!(
                            "hermes-gateway-v1, hermes-gateway-ticket.{ticket}"
                        ))?,
                    );
                    private_protocol = true;
                } else {
                    headers.insert("X-Hermes-Gateway-Ticket", sensitive(&ticket)?);
                }
            }
        }
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| "Invalid gateway WebSocket request")?;
        for (name, value) in &headers {
            request.headers_mut().insert(name.clone(), value.clone());
        }
        Ok(WsParts {
            url,
            request,
            private_protocol,
        })
    }
}
fn validate_cloud_headers(headers: &mut HeaderMap) -> Result<(), String> {
    for name in [
        "host",
        "connection",
        "content-length",
        "transfer-encoding",
        "proxy-authorization",
        "upgrade",
    ] {
        if headers.contains_key(name) {
            return Err("Cloud authentication attempted to override transport headers".into());
        }
    }
    for value in headers.values_mut() {
        value.set_sensitive(true);
    }
    Ok(())
}
fn sensitive(value: &str) -> Result<HeaderValue, String> {
    let mut value = HeaderValue::from_str(value).map_err(|_| "Invalid gateway credential")?;
    value.set_sensitive(true);
    Ok(value)
}

/// Canonical secret-bundle material; unknown config fields remain the store's responsibility.
pub(crate) fn normalize_access_headers(
    raw: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    AccessHeaders::new(raw).map(|headers| headers.0)
}
fn forbidden(name: &str) -> bool {
    matches!(
        name,
        "authorization"
            | "connection"
            | "content-length"
            | "content-type"
            | "cookie"
            | "host"
            | "origin"
            | "referer"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "x-hermes-session-token"
    )
}
fn http_scheme(url: &Url) -> Option<&str> {
    match url.scheme() {
        "http" | "ws" => Some("http"),
        "https" | "wss" => Some("https"),
        _ => None,
    }
}
/// WS and HTTP share a gateway scope; an origin sibling outside its path does not.
pub(super) fn require_scope(base: &Url, request: &Url) -> Result<(), String> {
    let base_path = base.path().trim_end_matches('/');
    let in_path = base_path.is_empty()
        || request.path() == base_path
        || request
            .path()
            .strip_prefix(base_path)
            .is_some_and(|suffix| suffix.starts_with('/'));
    if http_scheme(base).is_none()
        || http_scheme(base) != http_scheme(request)
        || base.host_str() != request.host_str()
        || base.port_or_known_default() != request.port_or_known_default()
        || !request.username().is_empty()
        || request.password().is_some()
        || request.fragment().is_some()
        || !in_path
    {
        return Err("The request changed its gateway origin or base path".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn url(value: &str) -> Url {
        Url::parse(value).unwrap()
    }
    fn routed(token: Option<&str>) -> Connection {
        Connection::Routed {
            owner: Arc::new(crate::source::RouteOwner::legacy("private-test-namespace").unwrap()),
            actual: Box::new(Connection::Url {
                endpoint: "https://gateway.test/hermes/".into(),
                token: token.map(str::to_owned),
            }),
            policy: Arc::new(
                WirePolicy::token(
                    ProfilePolicy::identity(),
                    BTreeMap::from([("X-Api-Key".into(), "private-access-fixture".into())]),
                )
                .unwrap(),
            ),
        }
    }
    #[test]
    fn captured_legacy_token_uses_http_header_and_ws_query_without_exposing_debug_values() {
        let connection = routed(Some("private-session-fixture"));
        let prepared = PreparedTransport::from_connection(&connection, Instant::now()).unwrap();
        let http = prepared
            .prepare_http(
                url("https://gateway.test/hermes/api/status?profile=work"),
                None,
                Instant::now(),
            )
            .unwrap();
        assert!(!http.url.query_pairs().any(|(key, _)| key == "token"));
        assert_eq!(
            http.headers["x-hermes-session-token"],
            "private-session-fixture"
        );
        assert_eq!(http.headers["x-api-key"], "private-access-fixture");
        assert!(http.headers["x-hermes-session-token"].is_sensitive());
        assert!(http.headers["x-api-key"].is_sensitive());
        assert!(!format!("{:?}", http.headers).contains("private-session-fixture"));
        let ws = prepared
            .prepare_ws(
                url("wss://gateway.test/hermes/api/ws"),
                None,
                WsPurpose::Interactive,
                Instant::now(),
            )
            .unwrap();
        assert_eq!(
            ws.url
                .query_pairs()
                .find(|(key, _)| key == "token")
                .unwrap()
                .1,
            "private-session-fixture"
        );
        assert_eq!(ws.request.headers()["x-api-key"], "private-access-fixture");
        assert!(!ws.request.headers().contains_key("x-hermes-session-token"));
        assert!(!ws.private_protocol);
        assert!(!format!("{connection:?}").contains("private-session-fixture"));
    }
    #[test]
    fn old_saved_url_and_anonymous_url_keep_their_existing_authentication_contract() {
        for token in [None, Some("existing-url-fixture")] {
            let connection = Connection::Saved {
                id: "existing".into(),
                revision: "revision".into(),
                inner: Box::new(Connection::Url {
                    endpoint: "https://gateway.test/hermes".into(),
                    token: token.map(str::to_owned),
                }),
            };
            let prepared = PreparedTransport::from_connection(&connection, Instant::now()).unwrap();
            let http = prepared
                .prepare_http(
                    url("https://gateway.test/hermes/api/status"),
                    None,
                    Instant::now(),
                )
                .unwrap();
            assert_eq!(
                http.url
                    .query_pairs()
                    .find(|(key, _)| key == "token")
                    .map(|(_, value)| value.into_owned()),
                token.map(str::to_owned)
            );
            assert!(http.headers.is_empty());
            let captured = Connection::Routed {
                actual: Box::new(connection),
                owner: Arc::new(crate::source::RouteOwner::legacy("private-preserve").unwrap()),
                policy: Arc::new(WirePolicy::native(ProfilePolicy::identity())),
            };
            let preserved = PreparedTransport::from_connection(&captured, Instant::now())
                .unwrap()
                .prepare_http(
                    url("https://gateway.test/hermes/api/status"),
                    None,
                    Instant::now(),
                )
                .unwrap();
            assert_eq!(preserved.url, http.url);
            assert!(preserved.headers.is_empty());
        }
        assert!(PreparedTransport::from_connection(&routed(None), Instant::now()).is_err());
        assert!(
            PreparedTransport::from_connection(&routed(Some("bad\r\ncredential")), Instant::now())
                .is_err()
        );
    }
    #[test]
    fn access_proxy_headers_sanitize_values_without_replacing_gateway_auth() {
        let headers = AccessHeaders::new(BTreeMap::from([
            ("CF-Access-Client-Id".into(), "  synthetic-id\r\n".into()),
            (
                "CF-Access-Client-Secret".into(),
                "synthetic-\r\nsecret".into(),
            ),
            ("Authorization".into(), "Bearer wrong-owner".into()),
            ("Cookie".into(), "wrong=session".into()),
            ("X-Hermes-Session-Token".into(), "wrong-token".into()),
            ("bad name".into(), "ignored".into()),
            ("X-Empty".into(), "\r\n  ".into()),
        ]))
        .unwrap();
        let wire = headers
            .for_request(
                &url("https://gateway.test/hermes"),
                &url("wss://gateway.test/hermes/api/ws"),
            )
            .unwrap();
        assert_eq!(wire.len(), 2);
        assert_eq!(wire["cf-access-client-id"], "synthetic-id");
        assert_eq!(wire["cf-access-client-secret"], "synthetic-secret");
        assert!(wire["cf-access-client-secret"].is_sensitive());
        assert!(!format!("{headers:?}").contains("synthetic-secret"));
        assert_eq!(
            headers.fingerprint(),
            AccessHeaders::new(BTreeMap::from([
                ("cf-access-client-secret".into(), "synthetic-secret".into()),
                ("cf-access-client-id".into(), "synthetic-id".into()),
            ]))
            .unwrap()
            .fingerprint()
        );
        assert!(
            AccessHeaders::new(BTreeMap::from([
                ("X-Access".into(), "first".into()),
                ("x-access".into(), "second".into()),
            ]))
            .is_err()
        );
        assert!(
            AccessHeaders::new(BTreeMap::from([(
                "X-Access".into(),
                "invalid\0value".into()
            )]))
            .is_err()
        );
    }
    #[test]
    fn access_credentials_never_leave_the_exact_scheme_host_port_and_path_scope() {
        let headers = AccessHeaders::new(BTreeMap::from([(
            "X-Api-Key".into(),
            "private-fixture".into(),
        )]))
        .unwrap();
        let base = url("https://gateway.test/hermes/");
        for target in [
            "https://gateway.test/hermes",
            "https://gateway.test/hermes/api/status?profile=work",
            "wss://gateway.test/hermes/api/ws",
        ] {
            assert_eq!(
                headers.for_request(&base, &url(target)).unwrap()["x-api-key"],
                "private-fixture"
            );
        }
        for target in [
            "http://gateway.test/hermes/api/status",
            "https://gateway.test:444/hermes/api/status",
            "https://other.test/hermes/api/status",
            "https://gateway.test/hermes-other/api/status",
            "https://gateway.test/api/status",
            "https://user@gateway.test/hermes/api/status",
            "https://gateway.test/hermes/api/status#fragment",
        ] {
            assert!(
                headers.for_request(&base, &url(target)).is_err(),
                "{target}"
            );
        }
    }
}

#[cfg(test)]
mod cloud_policy_tests {
    use super::*;
    struct Runtime;
    impl CloudRuntime for Runtime {
        fn prepare_http(&self, _: Method, url: &Url, _: Instant) -> Result<CloudHttp, String> {
            let mut headers = HeaderMap::new();
            headers.insert("host", HeaderValue::from_static("escape.invalid"));
            Ok(CloudHttp {
                url: url.clone(),
                headers,
                receipt: Box::new(()),
            })
        }
        fn retry_http_unauthorized(&self, _: CloudHttp, _: Instant) -> Result<CloudHttp, String> {
            unreachable!()
        }
        fn prepare_ws(
            &self,
            url: &Url,
            _: Instant,
        ) -> Result<tungstenite::handshake::client::Request, String> {
            url.as_str()
                .into_client_request()
                .map_err(|_| "bad URL".into())
        }
        fn runtime_fingerprint(&self) -> String {
            "fixture".into()
        }
    }
    #[test]
    fn cloud_runtime_is_retained_and_transport_header_override_is_rejected() {
        let runtime: Arc<dyn CloudRuntime> = Arc::new(Runtime);
        let profiles = ProfilePolicy::new("work", Default::default()).unwrap();
        let policy = WirePolicy::cloud(profiles.clone(), runtime.clone()).unwrap();
        let selected = policy
            .with_profiles(profiles)
            .with_selected(
                SelectedContext::new("work", BackendTarget::Named("backend".into())).unwrap(),
            )
            .unwrap();
        assert!(Arc::ptr_eq(&runtime, selected.cloud.as_ref().unwrap()));
        assert_eq!(policy.fingerprint(), policy.clone().fingerprint());
        assert!(!format!("{selected:?}").contains("escape.invalid"));
        let connection = Connection::Routed {
            actual: Box::new(Connection::Url {
                endpoint: "https://example.test/prefix/".into(),
                token: None,
            }),
            owner: Arc::new(crate::source::RouteOwner::legacy("fixture").unwrap()),
            policy: Arc::new(selected),
        };
        let transport = PreparedTransport::from_connection(&connection, Instant::now()).unwrap();
        let url = Url::parse("https://example.test/prefix/api/status").unwrap();
        assert!(
            transport
                .prepare_http(url.clone(), None, Instant::now())
                .is_err()
        );
        assert!(
            transport
                .prepare_http_method(Method::GET, url, None, Instant::now())
                .is_err()
        );
    }
}
