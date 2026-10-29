//! Raw, owner-bound file transfers. No URL supplied by a document can replace the gateway.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Default)]
pub struct Cancel(Arc<Cancellation>);
#[derive(Default)]
struct Cancellation {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}
impl Cancel {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.notify.notify_waiters();
    }
    pub fn cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    async fn wait(&self) {
        let notified = self.0.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.cancelled() {
            notified.await;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
}
#[derive(Clone, Copy)]
pub enum Route {
    Download,
    Media,
}
pub struct Request {
    pub path: String,
    pub session: Option<String>,
    pub route: Route,
    pub method: Method,
    pub range: Option<String>,
    pub if_range: Option<String>,
    pub if_none_match: Option<String>,
    pub if_modified_since: Option<String>,
    pub limit: u64,
}
impl Request {
    pub fn download(path: String, session: Option<String>, limit: u64) -> Self {
        Self {
            path,
            session,
            route: Route::Download,
            method: Method::Get,
            range: None,
            if_range: None,
            if_none_match: None,
            if_modified_since: None,
            limit,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub status: u16,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub content_range: Option<String>,
    pub accept_ranges: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub received: u64,
}
impl Gateway {
    /// The caller owns the destination and removes partial data on failure. A stalled send or
    /// body is cancellable; auth preparation retains the existing bounded gateway deadline.
    pub fn stream_file(
        &mut self,
        profile: &str,
        request: &Request,
        cancel: &Cancel,
        output: &mut impl Write,
    ) -> Result<Metadata, String> {
        valid_scope(profile)?;
        if let Some(session) = &request.session {
            valid_scope(session)?;
        }
        if request.path.is_empty() || request.path.chars().any(char::is_control) {
            return Err("Invalid agent file path".into());
        }
        if cancel.cancelled() {
            return Err("File transfer cancelled".into());
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        #[cfg(target_os = "linux")]
        if matches!(&self.auth, Auth::Native(_)) && !self.profile_homes.contains_key(profile) {
            self.profiles_until(deadline)?;
        }
        let route = match request.route {
            Route::Download => "api/fs/download",
            Route::Media => "api/files/stream",
        };
        let mut url = self.base.join(route).map_err(|_| "Invalid file route")?;
        if url.origin() != self.base.origin() {
            return Err("File route changed gateway owner".into());
        }
        url.query_pairs_mut()
            .append_pair("profile", profile)
            .append_pair("path", &request.path);
        if let Some(session) = &request.session {
            url.query_pairs_mut().append_pair("session_id", session);
        }
        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in [
            ("range", &request.range),
            ("if-range", &request.if_range),
            ("if-none-match", &request.if_none_match),
            ("if-modified-since", &request.if_modified_since),
        ] {
            if let Some(value) = value {
                headers.insert(
                    reqwest::header::HeaderName::from_static(name),
                    value
                        .parse()
                        .map_err(|_| "Invalid file negotiation header")?,
                );
            }
        }
        match &self.auth {
            Auth::Token(Some(token)) => {
                url.query_pairs_mut().append_pair("token", token);
            }
            Auth::Token(None) => {}
            #[cfg(target_os = "linux")]
            Auth::Native(endpoint) => {
                let home = self
                    .profile_homes
                    .get(profile)
                    .ok_or("Profile is not served by this gateway")?;
                let ticket = endpoint.ticket_until(home, "native-http", deadline)?;
                headers.insert(
                    "X-Hermes-Gateway-Ticket",
                    ticket.parse().map_err(|_| "Invalid file ticket")?,
                );
            }
        }
        let mut client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::none());
        #[cfg(target_os = "linux")]
        if matches!(&self.auth, Auth::Native(_)) {
            client = client.no_proxy();
        }
        if self.base.host_str().is_some_and(|h| {
            h == "localhost"
                || h.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }) {
            client = client.no_proxy();
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "The file transfer runtime could not start")?;
        runtime.block_on(async {
            let client=client.build().map_err(|_|"The file transport could not start")?;
            let method=if request.method==Method::Head { reqwest::Method::HEAD }else{reqwest::Method::GET};
            let send=client.request(method,url).headers(headers).send();
            let mut response=tokio::select! {
                biased;
                _=cancel.wait()=>return Err("File transfer cancelled".into()),
                result=send=>result.map_err(|_|"File transfer failed (network, TLS or timeout)")?,
            };
            let header=|name:&str| response.headers().get(name).and_then(|h|h.to_str().ok()).map(str::to_owned);
            let mut metadata=Metadata {
                status:response.status().as_u16(), content_type:header("content-type"),
                content_length:header("content-length").and_then(|s|s.parse().ok()), content_range:header("content-range"),
                accept_ranges:header("accept-ranges"), etag:header("etag"),
                last_modified:header("last-modified"), received:0,
            };
            // Preserve HTTP verdicts without downloading an error/redirect document into a file.
            if request.method==Method::Head || !response.status().is_success() { return Ok(metadata); }
            if metadata.content_length.is_some_and(|n|n>request.limit) {
                return Err("The file exceeds this preview’s transfer limit".into());
            }
            loop {
                let chunk=tokio::select! {
                    biased;
                    _=cancel.wait()=>return Err("File transfer cancelled".into()),
                    result=response.chunk()=>result.map_err(|_|"The file transfer was interrupted")?,
                };
                let Some(chunk)=chunk else {break};
                metadata.received=metadata.received.checked_add(chunk.len() as u64)
                    .filter(|n|*n<=request.limit).ok_or("The file exceeds this preview’s transfer limit")?;
                output.write_all(&chunk).map_err(|_|"The preview file could not be written")?;
            }
            Ok(metadata)
        })
    }
}

#[cfg(test)]
mod tests;
