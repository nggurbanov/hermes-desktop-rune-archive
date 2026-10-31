//! Socket credentials are minted for the selected endpoint, never retargeted.
use super::*;

/// The server routes for which the pinned dashboard has an explicit ticket gate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SocketTarget {
    Gateway,
    Pty,
    Console,
    SpeakStream,
    TranscribeStream,
    HostTerminal,
}
impl SocketTarget {
    fn path(self) -> &'static str {
        match self {
            Self::Gateway => "/api/ws",
            Self::Pty => "/api/pty",
            Self::Console => "/api/console",
            Self::SpeakStream => "/api/audio/speak-stream",
            Self::TranscribeStream => "/api/audio/transcribe-stream",
            Self::HostTerminal => "/api/host-terminal",
        }
    }
    /// Resolve an already routed URL only when its complete origin and path
    /// match this endpoint and one supported socket. No credential is acquired.
    pub fn from_url(endpoint: &Endpoint, url: &Url) -> Result<(Self, SocketQuery), Error> {
        let scheme = if endpoint.base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        };
        if url.scheme() != scheme
            || url.host_str() != endpoint.base.host_str()
            || url.port_or_known_default() != endpoint.base.port_or_known_default()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::InvalidBase);
        }
        let targets = [
            Self::Gateway,
            Self::Pty,
            Self::Console,
            Self::SpeakStream,
            Self::TranscribeStream,
            Self::HostTerminal,
        ];
        let target = targets
            .into_iter()
            .find(|target| endpoint.url(target.path()).path() == url.path())
            .ok_or(Error::InvalidBase)?;
        Ok((target, SocketQuery::new(url.query().unwrap_or(""))?))
    }
    fn mint_url(self, endpoint: &Endpoint) -> Url {
        if self == Self::HostTerminal {
            let mut url = endpoint.url("/api/webapp/socket-ticket");
            url.query_pairs_mut().append_pair("socket", "host-terminal");
            url
        } else {
            endpoint.url("/api/auth/ws-ticket")
        }
    }
}

/// A relative query, validated before any credential acquisition or network I/O.
/// Authentication parameters are reserved for the service.
pub struct SocketQuery(String);
impl SocketQuery {
    pub fn new(query: &str) -> Result<Self, Error> {
        if query.contains(['#', '\r', '\n']) || query.starts_with('?') {
            return Err(Error::InvalidBase);
        }
        for (key, _) in url::form_urlencoded::parse(query.as_bytes()) {
            if matches!(
                key.to_ascii_lowercase().as_str(),
                "ticket" | "token" | "internal" | "native_dial"
            ) {
                return Err(Error::InvalidBase);
            }
        }
        Ok(Self(query.to_owned()))
    }
}

fn ticket_for(
    endpoint: &Endpoint,
    target: SocketTarget,
    query: &SocketQuery,
    response: Value,
) -> Result<WsTicket, Error> {
    let ticket = response
        .get("ticket")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(Error::InvalidResponse)?;
    let mut url = endpoint.url(target.path());
    url.set_scheme(if endpoint.base.scheme() == "https" {
        "wss"
    } else {
        "ws"
    })
    .map_err(|_| Error::InvalidBase)?;
    if !query.0.is_empty() {
        url.set_query(Some(&query.0));
    }
    url.query_pairs_mut().append_pair("ticket", ticket);
    Ok(WsTicket {
        url,
        headers: endpoint.access_headers(),
        origin: endpoint.origin(),
    })
}

impl<S: TokenStore + 'static> Gateway<S> {
    pub fn socket_ticket_until(
        &self,
        target: SocketTarget,
        query: &SocketQuery,
        now: i64,
        deadline: Instant,
    ) -> Result<WsTicket, Error> {
        let token = self.access_until(false, None, now, deadline)?;
        let call = |token: &str| {
            request_until(
                &self.endpoint,
                reqwest::Method::POST,
                target.mint_url(&self.endpoint),
                Some(&Session::Bearer(token.to_owned())),
                None,
                deadline,
            )
        };
        let response = match call(&token) {
            Err(Error::Http(401, _)) => {
                call(&self.access_until(true, Some(&token), now, deadline)?)?
            }
            other => other?,
        };
        remaining(deadline)?;
        ticket_for(&self.endpoint, target, query, response)
    }
    /// Mint once per dial. Host terminal uses its own server-scoped mint; a
    /// missing surface returns the server error and is never replaced by a generic ticket.
    pub fn socket_ticket(
        &self,
        target: SocketTarget,
        query: &SocketQuery,
        now: i64,
    ) -> Result<WsTicket, Error> {
        let token = self.access(false, None, now)?;
        let call = |token: &str| {
            request(
                &self.endpoint,
                reqwest::Method::POST,
                target.mint_url(&self.endpoint),
                Some(&Session::Bearer(token.to_owned())),
                None,
            )
        };
        let response = match call(&token) {
            Err(Error::Http(401, _)) => call(&self.access(true, Some(&token), now)?)?,
            other => other?,
        };
        ticket_for(&self.endpoint, target, query, response)
    }
}

/// Cookie read from the selected agent's isolated jar after its own sign-in.
/// The portal cookie is never an agent credential. The owner must verify the
/// selected source namespace and exact agent base when extracting it.
pub struct AgentCookie {
    endpoint: Endpoint,
    namespace: String,
    cookie: String,
}
impl AgentCookie {
    pub fn from_agent_jar(
        endpoint: Endpoint,
        namespace: &str,
        jar_base: &str,
        jar_namespace: &str,
        cookie: String,
    ) -> Result<Self, Error> {
        let jar = Endpoint::new(jar_base, &[])?;
        if namespace.is_empty()
            || namespace != jar_namespace
            || jar.base != endpoint.base
            || cookie.is_empty()
            || HeaderValue::from_str(&cookie).is_err()
        {
            return Err(Error::InvalidBase);
        }
        Ok(Self {
            endpoint,
            namespace: namespace.to_owned(),
            cookie,
        })
    }
    pub fn socket_ticket_until(
        &self,
        endpoint: &Endpoint,
        namespace: &str,
        target: SocketTarget,
        query: &SocketQuery,
        deadline: Instant,
    ) -> Result<WsTicket, Error> {
        if self.namespace != namespace
            || self.endpoint.base != endpoint.base
            || self.endpoint.headers != endpoint.headers
        {
            return Err(Error::InvalidBase);
        }
        let response = request_until(
            &self.endpoint,
            reqwest::Method::POST,
            target.mint_url(&self.endpoint),
            Some(&Session::Cookie(self.cookie.clone())),
            None,
            deadline,
        )?;
        ticket_for(&self.endpoint, target, query, response)
    }
    /// A caller must reassert its source binding before any request is sent.
    pub fn socket_ticket(
        &self,
        endpoint: &Endpoint,
        namespace: &str,
        target: SocketTarget,
        query: &SocketQuery,
    ) -> Result<WsTicket, Error> {
        if self.namespace != namespace
            || self.endpoint.base != endpoint.base
            || self.endpoint.headers != endpoint.headers
        {
            return Err(Error::InvalidBase);
        }
        let response = request(
            &self.endpoint,
            reqwest::Method::POST,
            target.mint_url(&self.endpoint),
            Some(&Session::Cookie(self.cookie.clone())),
            None,
        )?;
        ticket_for(&self.endpoint, target, query, response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn fixture() -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/prefix", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut seen = Vec::new();
            for index in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    bytes.extend_from_slice(&buf[..n]);
                    if n == 0 || bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                seen.push(String::from_utf8(bytes).unwrap());
                let body = format!("{{\"ticket\":\"t{index}\"}}");
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            seen
        });
        (base, handle)
    }

    struct MemoryStore(Option<Tokens>);
    impl TokenStore for MemoryStore {
        fn load(&mut self) -> Result<Option<Tokens>, Error> {
            Ok(self.0.clone())
        }
        fn save(&mut self, tokens: &Tokens) -> Result<(), Error> {
            self.0 = Some(tokens.clone());
            Ok(())
        }
        fn compare_and_swap(
            &mut self,
            expected: &Tokens,
            replacement: Option<&Tokens>,
        ) -> Result<bool, Error> {
            if self.0.as_ref() != Some(expected) {
                return Ok(false);
            }
            self.0 = replacement.cloned();
            Ok(true)
        }
    }
    #[test]
    fn host_terminal_401_refreshes_and_retries_its_named_mint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/prefix", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in [
                ("401 Unauthorized", r#"{"error":"expired"}"#),
                (
                    "200 OK",
                    r#"{"access_token":"new","refresh_token":"next","expires_at":9999}"#,
                ),
                ("200 OK", r#"{"ticket":"fresh"}"#),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    bytes.extend_from_slice(&buf[..n]);
                    if n == 0 {
                        break;
                    }
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&bytes[..end]);
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .and_then(|v| v.parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(bytes).unwrap());
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let endpoint = Endpoint::new(&base, &[]).unwrap();
        let service = Gateway::new(
            endpoint,
            "host-refresh",
            MemoryStore(Some(Tokens {
                access_token: "old".into(),
                refresh_token: "refresh".into(),
                expires_at: 9999,
                provider: String::new(),
                user_id: String::new(),
            })),
        )
        .unwrap();
        let ticket = service
            .socket_ticket(
                SocketTarget::HostTerminal,
                &SocketQuery::new("profile=work").unwrap(),
                1,
            )
            .unwrap();
        assert_eq!(ticket.url.path(), "/prefix/api/host-terminal");
        let requests = server.join().unwrap();
        assert!(
            requests[0].starts_with("POST /prefix/api/webapp/socket-ticket?socket=host-terminal ")
        );
        assert!(requests[1].starts_with("POST /prefix/auth/native/refresh "));
        assert!(
            requests[2].starts_with("POST /prefix/api/webapp/socket-ticket?socket=host-terminal ")
        );
        assert!(
            requests[2]
                .to_lowercase()
                .contains("authorization: bearer new")
        );
    }

    #[test]
    fn routed_url_must_match_exact_endpoint_and_supported_path() {
        let endpoint = Endpoint::new("https://agent.example:8443/prefix", &[]).unwrap();
        let good =
            Url::parse("wss://agent.example:8443/prefix/api/audio/transcribe-stream?profile=a%20b")
                .unwrap();
        let (target, query) = SocketTarget::from_url(&endpoint, &good).unwrap();
        assert!(target == SocketTarget::TranscribeStream);
        assert_eq!(query.0, "profile=a%20b");
        for invalid in [
            "ws://agent.example:8443/prefix/api/ws",
            "wss://other.example:8443/prefix/api/ws",
            "wss://agent.example:8444/prefix/api/ws",
            "wss://agent.example:8443/other/api/ws",
            "wss://agent.example:8443/prefix/api/ws/extra",
            "wss://agent.example:8443/prefix/api/plugins/test/events",
            "wss://agent.example:8443/prefix/api/ws?ticket=old",
        ] {
            assert!(SocketTarget::from_url(&endpoint, &Url::parse(invalid).unwrap()).is_err());
        }
    }

    #[test]
    fn ticket_modes_dial_exact_sockets_and_refuse_replay() {
        use tungstenite::{
            accept_hdr,
            handshake::server::{ErrorResponse, Request, Response},
        };
        for (target, mint_path, socket_path) in [
            (
                SocketTarget::Gateway,
                "/prefix/api/auth/ws-ticket",
                "/prefix/api/ws",
            ),
            (
                SocketTarget::HostTerminal,
                "/prefix/api/webapp/socket-ticket?socket=host-terminal",
                "/prefix/api/host-terminal",
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let base = format!("http://{address}/prefix");
            let expected_origin = format!("http://{address}");
            let server = thread::spawn(move || {
                let (mut mint, _) = listener.accept().unwrap();
                let mut buf = [0; 4096];
                let mut bytes = Vec::new();
                loop {
                    let n = mint.read(&mut buf).unwrap();
                    bytes.extend_from_slice(&buf[..n]);
                    if n == 0 || bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                assert!(request.starts_with(&format!("POST {mint_path} ")));
                assert!(request.to_lowercase().contains("cookie: agent=ok"));
                let body = r#"{"ticket":"once"}"#;
                write!(mint, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                let mut used = false;
                for _ in 0..2 {
                    let (stream, _) = listener.accept().unwrap();
                    let result = accept_hdr(
                        stream,
                        |request: &Request,
                         response: Response|
                         -> Result<Response, ErrorResponse> {
                            assert_eq!(
                                request.uri().to_string(),
                                format!("{socket_path}?profile=work&ticket=once")
                            );
                            assert_eq!(request.headers()["origin"], expected_origin);
                            assert!(!request.headers().contains_key("cookie"));
                            if used {
                                return Err(tungstenite::http::Response::builder()
                                    .status(401)
                                    .body(Some("replay".into()))
                                    .unwrap());
                            }
                            used = true;
                            Ok(response)
                        },
                    );
                    if let Ok(mut socket) = result {
                        let _ = socket.close(None);
                    }
                }
            });
            let endpoint = Endpoint::new(&base, &[]).unwrap();
            let cookie = AgentCookie::from_agent_jar(
                endpoint.clone(),
                "source",
                &base,
                "source",
                "agent=ok".into(),
            )
            .unwrap();
            let ws = cookie
                .socket_ticket(
                    &endpoint,
                    "source",
                    target,
                    &SocketQuery::new("profile=work").unwrap(),
                )
                .unwrap();
            let request = ws.into_request().unwrap();
            let replay = request.clone();
            let (mut socket, _) = tungstenite::connect(request).unwrap();
            let _ = socket.close(None);
            assert!(tungstenite::connect(replay).is_err());
            server.join().unwrap();
        }
    }

    #[test]
    fn selected_agent_cookie_mints_exact_targets_and_rejects_other_bindings_before_io() {
        let (base, server) = fixture();
        let endpoint = Endpoint::new(&base, &[("x-edge".into(), "selected".into())]).unwrap();
        let cookie = AgentCookie::from_agent_jar(
            endpoint.clone(),
            "source-a",
            &base,
            "source-a",
            "agent=only".into(),
        )
        .unwrap();
        let query = SocketQuery::new("profile=work&cwd=%2Ftmp%20x").unwrap();
        for (target, path) in [
            (SocketTarget::Gateway, "/prefix/api/ws"),
            (SocketTarget::Pty, "/prefix/api/pty"),
            (SocketTarget::SpeakStream, "/prefix/api/audio/speak-stream"),
            (SocketTarget::HostTerminal, "/prefix/api/host-terminal"),
        ] {
            let ws = cookie
                .socket_ticket(&endpoint, "source-a", target, &query)
                .unwrap();
            assert_eq!(ws.url.path(), path);
            assert!(
                ws.url
                    .query()
                    .unwrap()
                    .starts_with("profile=work&cwd=%2Ftmp%20x&ticket=t")
            );
            let req = ws.into_request().unwrap();
            assert_eq!(req.headers()["origin"], endpoint.origin());
            assert!(!req.headers().contains_key("cookie"));
        }
        let requests = server.join().unwrap();
        assert!(
            requests[..3]
                .iter()
                .all(|r| r.starts_with("POST /prefix/api/auth/ws-ticket "))
        );
        assert!(
            requests[3].starts_with("POST /prefix/api/webapp/socket-ticket?socket=host-terminal ")
        );
        assert!(
            requests
                .iter()
                .all(|r| r.to_lowercase().contains("cookie: agent=only"))
        );
        let other = Endpoint::new(&format!("{base}/other"), &[]).unwrap();
        assert!(matches!(
            cookie.socket_ticket(&other, "source-a", SocketTarget::Gateway, &query),
            Err(Error::InvalidBase)
        ));
        assert!(matches!(
            cookie.socket_ticket(&endpoint, "source-b", SocketTarget::Gateway, &query),
            Err(Error::InvalidBase)
        ));
        assert!(
            AgentCookie::from_agent_jar(
                endpoint.clone(),
                "source-a",
                &base,
                "source-b",
                "portal=x".into()
            )
            .is_err()
        );
        for q in [
            "ticket=x",
            "to%6ben=x",
            "internal=x",
            "native_dial=x",
            "x=y#z",
        ] {
            assert!(SocketQuery::new(q).is_err());
        }
    }
}
