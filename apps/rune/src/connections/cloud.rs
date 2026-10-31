//! Native gateway OAuth and Cloud discovery. No UI or credential-store access is implicit.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{
    blocking::Client,
    header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidBase,
    InvalidHeader,
    Reauthenticate,
    Cancelled,
    Callback,
    Timeout,
    Transport,
    Http(u16, Option<String>),
    TeamSelection(Vec<Value>),
    InvalidResponse,
    Store,
    EmbeddedRequired,
}

/// Caller owns a secret-store namespace tied to the actual connection and origin.
/// Implementations must not put this value into window state or configuration JSON.
pub trait TokenStore: Send {
    fn load(&mut self) -> Result<Option<Tokens>, Error>;
    fn save(&mut self, tokens: &Tokens) -> Result<(), Error>;
    /// Atomically replace only the exact previously loaded generation.
    fn compare_and_swap(
        &mut self,
        expected: &Tokens,
        replacement: Option<&Tokens>,
    ) -> Result<bool, Error>;
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at: i64,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub user_id: String,
}
impl Tokens {
    fn valid(self) -> Result<Self, Error> {
        if self.access_token.is_empty() {
            Err(Error::InvalidResponse)
        } else {
            Ok(self)
        }
    }
}

/// Base URL and access headers are captured together; never retarget a credential.
#[derive(Clone)]
pub struct Endpoint {
    base: Url,
    headers: HeaderMap,
}
impl Endpoint {
    pub fn new(base: &str, headers: &[(String, String)]) -> Result<Self, Error> {
        let mut base = Url::parse(base).map_err(|_| Error::InvalidBase)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(Error::InvalidBase);
        }
        let path = base.path().trim_end_matches('/').to_owned();
        base.set_path(&path);
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| Error::InvalidHeader)?;
            // Authentication and routing headers belong to this service, not user overrides.
            if matches!(
                name.as_str(),
                "authorization" | "cookie" | "host" | "origin"
            ) {
                return Err(Error::InvalidHeader);
            }
            map.insert(
                name,
                HeaderValue::from_str(value).map_err(|_| Error::InvalidHeader)?,
            );
        }
        Ok(Self { base, headers: map })
    }
    fn url(&self, suffix: &str) -> Url {
        let mut url = self.base.clone();
        url.set_path(&format!(
            "{}{}",
            self.base.path().trim_end_matches('/'),
            suffix
        ));
        url
    }
    /// Public probes perform blocking I/O and must run on a worker.
    pub fn status(&self, deadline: Instant) -> Result<Value, Error> {
        request_until(
            self,
            reqwest::Method::GET,
            self.url("/api/status"),
            None,
            None,
            deadline,
        )
    }
    pub fn providers(&self, deadline: Instant) -> Result<Value, Error> {
        request_until(
            self,
            reqwest::Method::GET,
            self.url("/api/auth/providers"),
            None,
            None,
            deadline,
        )
    }
    pub fn origin(&self) -> String {
        self.base.origin().ascii_serialization()
    }
    pub fn access_headers(&self) -> HeaderMap {
        self.headers.clone()
    }
}

fn client(timeout: Duration) -> Result<Client, Error> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|_| Error::Transport)
}
fn request(
    endpoint: &Endpoint,
    method: reqwest::Method,
    url: Url,
    auth: Option<&Session>,
    body: Option<Value>,
) -> Result<Value, Error> {
    request_until(
        endpoint,
        method,
        url,
        auth,
        body,
        Instant::now() + Duration::from_secs(15),
    )
}

fn remaining(deadline: Instant) -> Result<Duration, Error> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(Error::Timeout)
}

fn request_until(
    endpoint: &Endpoint,
    method: reqwest::Method,
    url: Url,
    auth: Option<&Session>,
    body: Option<Value>,
    deadline: Instant,
) -> Result<Value, Error> {
    let mut headers = endpoint.headers.clone();
    if let Some(auth) = auth {
        let (name, value) = match auth {
            Session::Bearer(v) => (AUTHORIZATION, format!("Bearer {v}")),
            Session::Cookie(v) => (COOKIE, v.clone()),
        };
        if value.is_empty() || value == "Bearer " {
            return Err(Error::Reauthenticate);
        }
        headers.insert(
            name,
            HeaderValue::from_str(&value).map_err(|_| Error::InvalidHeader)?,
        );
    }
    let mut req = client(remaining(deadline)?)?
        .request(method, url)
        .headers(headers);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let response = req.send().map_err(|e| {
        if e.is_timeout() {
            Error::Timeout
        } else {
            Error::Transport
        }
    })?;
    let status = response.status().as_u16();
    // Bound responses; never include response text or credentials in errors/logs.
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            if Instant::now() >= deadline
                || matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                )
            {
                Error::Timeout
            } else {
                Error::Transport
            }
        })?;
    remaining(deadline)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::InvalidResponse);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        if (200..300).contains(&status) {
            Error::InvalidResponse
        } else {
            Error::Http(status, None)
        }
    })?;
    if status == 409 && value.get("error").and_then(Value::as_str) == Some("org_selection_required")
    {
        if let Some(orgs) = value.get("orgs").and_then(Value::as_array) {
            return Err(Error::TeamSelection(
                orgs.iter()
                    .filter(|o| o.get("id").and_then(Value::as_str).is_some())
                    .cloned()
                    .collect(),
            ));
        }
    }
    if !(200..300).contains(&status) {
        return Err(Error::Http(
            status,
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned),
        ));
    }
    Ok(value)
}

/// Explicitly supplied, isolated session. Cookie lifecycle and renewal belong to its owner.
#[derive(Clone)]
pub enum Session {
    Bearer(String),
    Cookie(String),
}

/// An isolated browser owner supplies cookies only for its bound portal origin.
/// The session is ephemeral and must be dropped on logout or when the owner closes.
pub struct PortalSession {
    endpoint: Endpoint,
    cookie: Option<String>,
    selected_org: Option<String>,
}
impl PortalSession {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            cookie: None,
            selected_org: None,
        }
    }
    pub fn set_cookie_from_owned_view(
        &mut self,
        origin: &str,
        cookie: String,
    ) -> Result<(), Error> {
        if origin != self.endpoint.origin()
            || cookie.is_empty()
            || HeaderValue::from_str(&cookie).is_err()
        {
            return Err(Error::InvalidHeader);
        }
        self.cookie = Some(cookie);
        Ok(())
    }
    pub fn select_team(&mut self, org: Option<String>) {
        self.selected_org = org;
    }
    pub fn discover(&self) -> Result<Discovery, Error> {
        let cookie = self.cookie.as_ref().ok_or(Error::Reauthenticate)?;
        discover(
            &self.endpoint,
            &Session::Cookie(cookie.clone()),
            self.selected_org.as_deref(),
        )
    }
    pub fn logout(&mut self) {
        self.cookie = None;
        self.selected_org = None;
    }
}

pub enum Discovery {
    Agents(Value),
    SelectTeam(Vec<Value>),
}
pub fn discover(
    portal: &Endpoint,
    session: &Session,
    org: Option<&str>,
) -> Result<Discovery, Error> {
    fn fetch(portal: &Endpoint, session: &Session, org: Option<&str>) -> Result<Value, Error> {
        let mut url = portal.url("/api/agents");
        if let Some(org) = org.filter(|s| !s.is_empty()) {
            url.query_pairs_mut().append_pair("org", org);
        }
        request(portal, reqwest::Method::GET, url, Some(session), None)
    }
    let mut result = fetch(portal, session, org);
    if org.is_some()
        && matches!(&result, Err(Error::Http(403, Some(code))) if code == "org_access_denied")
        || org.is_some()
            && matches!(&result, Err(Error::Http(404, Some(code))) if code == "org_not_found")
    {
        result = fetch(portal, session, None);
    }
    match result {
        Ok(value) => Ok(Discovery::Agents(value)),
        Err(Error::Http(401, _)) => Err(Error::Reauthenticate),
        Err(Error::TeamSelection(orgs)) => Ok(Discovery::SelectTeam(orgs)),
        Err(e) => Err(e),
    }
}

/// One active callback owns its listener, state and verifier. Dropping it cancels it.
pub struct Login {
    listener: crate::oauth::Loopback,
    endpoint: Endpoint,
    verifier: String,
    pub authorize_url: String,
    deadline: Instant,
}
impl Login {
    pub fn begin(
        endpoint: &Endpoint,
        status: &Value,
        provider: Option<&str>,
    ) -> Result<Self, Error> {
        if !status
            .get("auth_flows")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|v| v == "native_pkce"))
        {
            return Err(Error::InvalidResponse);
        }
        // A system browser cannot receive endpoint access headers.
        if !endpoint.headers.is_empty() {
            return Err(Error::EmbeddedRequired);
        }
        let mut listener = crate::oauth::Loopback::listen().map_err(|_| Error::Transport)?;
        // Two OS-random UUIDs provide 244 unpredictable bits, within RFC 7636 limits.
        let mut random = Vec::with_capacity(32);
        random.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        random.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        let verifier = URL_SAFE_NO_PAD.encode(random);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut url = endpoint.url("/auth/native/authorize");
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256")
                .append_pair("redirect_uri", &listener.redirect_uri)
                .append_pair("state", &state);
            if let Some(provider) = provider {
                q.append_pair("provider", provider);
            }
        }
        listener
            .authorize(url.as_str())
            .map_err(|_| Error::Callback)?;
        Ok(Self {
            listener,
            endpoint: endpoint.clone(),
            verifier,
            authorize_url: url.to_string(),
            deadline: Instant::now() + Duration::from_secs(360),
        })
    }
    /// Returns None while awaiting the user; consuming completion prevents code reuse.
    pub fn poll(&mut self) -> Option<Result<Tokens, Error>> {
        self.poll_until(self.deadline)
    }
    /// Poll the callback and bound the token exchange by the caller's deadline.
    pub fn poll_until(&mut self, deadline: Instant) -> Option<Result<Tokens, Error>> {
        let deadline = deadline.min(self.deadline);
        if Instant::now() >= deadline {
            return Some(Err(Error::Timeout));
        }
        let endpoint = &self.endpoint;
        let result = self.listener.poll()?;
        Some(result.map_err(|_| Error::Callback).and_then(|value| {
            let code = value
                .get("code")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or(Error::Callback)?;
            let body = request_until(
                endpoint,
                reqwest::Method::POST,
                endpoint.url("/auth/native/token"),
                None,
                Some(json!({"code":code,"code_verifier":self.verifier})),
                deadline,
            )?;
            serde_json::from_value::<Tokens>(body)
                .map_err(|_| Error::InvalidResponse)?
                .valid()
        }))
    }
}

#[path = "cloud/refresh.rs"]
mod refresh;
pub use refresh::AccessSubscription;

pub struct Gateway<S: TokenStore> {
    endpoint: Endpoint,
    identity: String,
    store: Arc<Mutex<S>>,
}
impl<S: TokenStore + 'static> Gateway<S> {
    /// Identity includes the complete source/auth namespace and backing record.
    /// Equal identities at the same endpoint must address the same atomic record.
    pub fn new(endpoint: Endpoint, identity: &str, store: S) -> Result<Self, Error> {
        if identity.is_empty() {
            return Err(Error::InvalidBase);
        }
        Ok(Self {
            endpoint,
            identity: identity.to_owned(),
            store: Arc::new(Mutex::new(store)),
        })
    }
    pub fn install(&self, tokens: Tokens) -> Result<(), Error> {
        let tokens = tokens.valid()?;
        self.store.lock().map_err(|_| Error::Store)?.save(&tokens)
    }
    /// Starts or joins a refresh without waiting for I/O. Dropping the subscription
    /// cancels delivery, not a rotation already accepted by the server.
    pub fn access_async(
        &self,
        force: bool,
        rejected: Option<&str>,
        now: i64,
    ) -> Result<AccessSubscription, Error> {
        refresh::subscribe(
            &self.endpoint,
            &self.identity,
            self.store.clone(),
            force,
            rejected.map(str::to_owned),
            now,
        )
    }
    fn access(&self, force: bool, rejected: Option<&str>, now: i64) -> Result<String, Error> {
        self.access_async(force, rejected, now)?.wait()
    }
    fn access_until(
        &self,
        force: bool,
        rejected: Option<&str>,
        now: i64,
        deadline: Instant,
    ) -> Result<String, Error> {
        remaining(deadline)?;
        self.access_async(force, rejected, now)?
            .wait_until(deadline)
    }
    pub fn prepare_http_until(
        &self,
        method: reqwest::Method,
        path: &str,
        now: i64,
        deadline: Instant,
    ) -> Result<PreparedHttp, Error> {
        validate_http_path(path)?;
        let token = self.access_until(false, None, now, deadline)?;
        remaining(deadline)?;
        self.prepared(method, path, token)
    }
    pub fn retry_http_unauthorized_until(
        &self,
        previous: &PreparedHttp,
        now: i64,
        deadline: Instant,
    ) -> Result<PreparedHttp, Error> {
        if previous.method != reqwest::Method::GET
            || previous.retried
            || previous.endpoint_key != refresh::endpoint_key(&self.endpoint, &self.identity)
        {
            return Err(Error::InvalidBase);
        }
        let token = self.access_until(true, Some(&previous.token), now, deadline)?;
        remaining(deadline)?;
        let mut prepared = self.prepared(previous.method.clone(), &previous.path, token)?;
        prepared.retried = true;
        Ok(prepared)
    }
    /// Prepare an authenticated request for streaming, multipart and range consumers.
    /// The returned request is bound to this endpoint and path; its headers are sensitive.
    pub fn prepare_http(
        &self,
        method: reqwest::Method,
        path: &str,
        now: i64,
    ) -> Result<PreparedHttp, Error> {
        validate_http_path(path)?;
        let token = self.access(false, None, now)?;
        self.prepared(method, path, token)
    }
    /// A confirmed 401 on a GET can refresh once. Unsafe writes are never retried.
    pub fn retry_http_unauthorized(
        &self,
        previous: &PreparedHttp,
        now: i64,
    ) -> Result<PreparedHttp, Error> {
        if previous.method != reqwest::Method::GET
            || previous.retried
            || previous.endpoint_key != refresh::endpoint_key(&self.endpoint, &self.identity)
        {
            return Err(Error::InvalidBase);
        }
        let token = self.access(true, Some(&previous.token), now)?;
        let mut prepared = self.prepared(previous.method.clone(), &previous.path, token)?;
        prepared.retried = true;
        Ok(prepared)
    }
    fn prepared(
        &self,
        method: reqwest::Method,
        path: &str,
        token: String,
    ) -> Result<PreparedHttp, Error> {
        let mut headers = self.endpoint.access_headers();
        let mut auth =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::InvalidHeader)?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);
        for value in headers.values_mut() {
            value.set_sensitive(true);
        }
        Ok(PreparedHttp {
            method,
            path: path.to_owned(),
            url: {
                let (route, query) = path
                    .split_once('?')
                    .map_or((path, None), |(route, query)| (route, Some(query)));
                let mut url = self.endpoint.url(route);
                url.set_query(query);
                url
            },
            headers,
            token,
            endpoint_key: refresh::endpoint_key(&self.endpoint, &self.identity),
            retried: false,
        })
    }
    pub fn rest(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        now: i64,
    ) -> Result<Value, Error> {
        validate_path(path)?;
        let token = self.access(false, None, now)?;
        let retry_safe = method == reqwest::Method::GET
            || (method == reqwest::Method::POST && path == "/api/auth/ws-ticket");
        let call = |token: String| {
            request(
                &self.endpoint,
                method.clone(),
                self.endpoint.url(path),
                Some(&Session::Bearer(token)),
                body.clone(),
            )
        };
        match call(token.clone()) {
            Err(Error::Http(401, _)) if retry_safe => {
                let rotated = self.access(true, Some(&token), now)?;
                call(rotated)
            }
            other => other,
        }
    }
    /// Each call mints a distinct ticket; caller must consume its returned URL once.
    pub fn ws_ticket(&self, now: i64) -> Result<WsTicket, Error> {
        let response = self.rest(reqwest::Method::POST, "/api/auth/ws-ticket", None, now)?;
        make_ws_ticket(&self.endpoint, response)
    }
}
/// Opaque, non-serializable request credentials scoped to one endpoint and path.
pub struct PreparedHttp {
    method: reqwest::Method,
    path: String,
    url: Url,
    headers: HeaderMap,
    token: String,
    endpoint_key: (String, String, Vec<u8>),
    retried: bool,
}
impl PreparedHttp {
    pub fn method(&self) -> &reqwest::Method {
        &self.method
    }
    pub fn url(&self) -> &Url {
        &self.url
    }
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}
fn validate_http_path(path: &str) -> Result<(), Error> {
    let route = path.split_once('?').map_or(path, |(route, _)| route);
    if path.contains('#') || route.to_ascii_lowercase().contains("%2e") {
        return Err(Error::InvalidBase);
    }
    validate_path(route)
}
fn validate_path(path: &str) -> Result<(), Error> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('?')
        || path.contains('#')
        || path.split('/').any(|p| p == ".." || p == ".")
    {
        Err(Error::InvalidBase)
    } else {
        Ok(())
    }
}
/// Mint from a caller-owned, isolated cookie or bearer session without accessing a keychain.
pub fn ws_ticket_with_session(endpoint: &Endpoint, session: &Session) -> Result<WsTicket, Error> {
    let response = request(
        endpoint,
        reqwest::Method::POST,
        endpoint.url("/api/auth/ws-ticket"),
        Some(session),
        None,
    )?;
    make_ws_ticket(endpoint, response)
}
fn make_ws_ticket(endpoint: &Endpoint, response: Value) -> Result<WsTicket, Error> {
    let ticket = response
        .get("ticket")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(Error::InvalidResponse)?;
    let mut url = endpoint.url("/api/ws");
    url.set_scheme(if endpoint.base.scheme() == "https" {
        "wss"
    } else {
        "ws"
    })
    .map_err(|_| Error::InvalidBase)?;
    url.query_pairs_mut().append_pair("ticket", ticket);
    Ok(WsTicket {
        url,
        headers: endpoint.access_headers(),
        origin: endpoint.origin(),
    })
}
/// Deliberately non-Clone: transfer into exactly one WS connection attempt.
pub struct WsTicket {
    url: Url,
    headers: HeaderMap,
    origin: String,
}
impl WsTicket {
    /// Consume the ticket when starting a single connection attempt.
    pub fn into_request(self) -> Result<tungstenite::http::Request<()>, Error> {
        use tungstenite::client::IntoClientRequest;
        let mut request = self
            .url
            .as_str()
            .into_client_request()
            .map_err(|_| Error::InvalidBase)?;
        *request.headers_mut() = {
            let mut headers = request.headers().clone();
            headers.extend(self.headers);
            headers.insert(
                tungstenite::http::header::ORIGIN,
                HeaderValue::from_str(&self.origin).map_err(|_| Error::InvalidHeader)?,
            );
            headers
        };
        Ok(request)
    }
}

#[cfg(test)]
#[path = "cloud/tests.rs"]
mod tests;

#[path = "cloud/socket.rs"]
mod socket;
pub use socket::{AgentCookie, SocketQuery, SocketTarget};
