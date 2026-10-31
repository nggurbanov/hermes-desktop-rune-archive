//! Retained, endpoint-bound Cloud transport for resolved Legacy routes.
use crate::connections::cloud::{self, Endpoint, Gateway, PreparedHttp, SocketTarget, TokenStore};
use crate::gateway::prepared::{CloudHttp, CloudRuntime};
use reqwest::Method;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;

pub(super) struct CloudRuntimeAdapter<S: TokenStore> {
    endpoint: Endpoint,
    base: Url,
    gateway: Gateway<S>,
    fingerprint: String,
    binding: Arc<()>,
}
struct Receipt {
    binding: Arc<()>,
    prepared: PreparedHttp,
}
impl<S: TokenStore + 'static> CloudRuntimeAdapter<S> {
    pub(super) fn new(
        base: &str,
        headers: &[(String, String)],
        identity: &str,
        store: S,
    ) -> Result<Arc<dyn CloudRuntime>, String> {
        let endpoint = Endpoint::new(base, headers).map_err(error)?;
        if identity.is_empty() {
            return Err("Cloud runtime has no identity".into());
        }
        let mut base = Url::parse(base).map_err(|_| "Invalid Cloud endpoint")?;
        let path = base.path().trim_end_matches('/').to_owned();
        base.set_path(&path);
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(base.as_str(), identity))
                    .map_err(|_| "Invalid Cloud identity")?
            )
        );
        let gateway = Gateway::new(endpoint.clone(), identity, store).map_err(error)?;
        Ok(Arc::new(Self {
            endpoint,
            base,
            gateway,
            fingerprint,
            binding: Arc::new(()),
        }))
    }
    fn relative(&self, url: &Url) -> Result<String, String> {
        if url.scheme() != self.base.scheme()
            || url.host() != self.base.host()
            || url.port_or_known_default() != self.base.port_or_known_default()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("Cloud request is outside its endpoint".into());
        }
        if url.query_pairs().any(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "token" | "ticket" | "internal" | "native_dial"
            )
        }) {
            return Err("Cloud request contains a reserved query parameter".into());
        }
        let prefix = self.base.path().trim_end_matches('/');
        let path = url
            .path()
            .strip_prefix(prefix)
            .filter(|path| path.starts_with('/') && !path.starts_with("//"))
            .ok_or("Cloud request is outside its endpoint")?;
        if path.to_ascii_lowercase().contains("%2e") {
            return Err("Invalid Cloud request path".into());
        }
        let mut relative = path.to_owned();
        if let Some(query) = url.query() {
            relative.push('?');
            relative.push_str(query);
        }
        Ok(relative)
    }
    fn wrap(&self, prepared: PreparedHttp) -> CloudHttp {
        CloudHttp {
            url: prepared.url().clone(),
            headers: prepared.headers().clone(),
            receipt: Box::new(Receipt {
                binding: self.binding.clone(),
                prepared,
            }),
        }
    }
}
fn now() -> Result<i64, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "System clock is invalid")?
        .as_secs();
    i64::try_from(seconds).map_err(|_| "System clock is invalid".into())
}
fn error(error: cloud::Error) -> String {
    if let cloud::Error::Http(status, _) = error {
        return format!("Cloud service rejected the request (HTTP {status})");
    }
    match error {
        cloud::Error::Timeout => "Cloud request timed out",
        cloud::Error::Reauthenticate => "Cloud authentication needs renewal",
        cloud::Error::Store => "Cloud credentials are unavailable or their route has changed",
        _ => "Cloud request failed",
    }
    .into()
}
impl<S: TokenStore + 'static> CloudRuntime for CloudRuntimeAdapter<S> {
    fn prepare_http(
        &self,
        method: Method,
        url: &Url,
        deadline: Instant,
    ) -> Result<CloudHttp, String> {
        let path = self.relative(url)?;
        let prepared = self
            .gateway
            .prepare_http_until(method, &path, now()?, deadline)
            .map_err(error)?;
        if prepared.url() != url {
            return Err("Cloud request URL changed during preparation".into());
        }
        Ok(self.wrap(prepared))
    }
    fn retry_http_unauthorized(
        &self,
        previous: CloudHttp,
        deadline: Instant,
    ) -> Result<CloudHttp, String> {
        let receipt = previous
            .receipt
            .downcast::<Receipt>()
            .map_err(|_| "Invalid Cloud retry receipt")?;
        if !Arc::ptr_eq(&receipt.binding, &self.binding)
            || previous.url != *receipt.prepared.url()
            || previous.headers != *receipt.prepared.headers()
            || receipt.prepared.method() != Method::GET
        {
            return Err("Invalid Cloud retry binding".into());
        }
        self.relative(&previous.url)?;
        let prepared = self
            .gateway
            .retry_http_unauthorized_until(&receipt.prepared, now()?, deadline)
            .map_err(error)?;
        if prepared.url() != &previous.url || prepared.method() != receipt.prepared.method() {
            return Err("Cloud retry changed its request".into());
        }
        Ok(self.wrap(prepared))
    }
    fn prepare_ws(
        &self,
        url: &Url,
        deadline: Instant,
    ) -> Result<tungstenite::handshake::client::Request, String> {
        let (target, query) = SocketTarget::from_url(&self.endpoint, url).map_err(error)?;
        self.gateway
            .socket_ticket_until(target, &query, now()?, deadline)
            .map_err(error)?
            .into_request()
            .map_err(error)
    }
    fn runtime_fingerprint(&self) -> String {
        self.fingerprint.clone()
    }
}
#[cfg(test)]
#[path = "tests/cloud_runtime.rs"]
mod tests;
