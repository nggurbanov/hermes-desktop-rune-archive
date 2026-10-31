use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn fixture(responses: Vec<&'static str>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/prefix", listener.local_addr().unwrap());
    let join = thread::spawn(move || {
        let mut requests = vec![];
        for body in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buf = Vec::new();
            let mut part = [0; 4096];
            loop {
                let n = stream.read(&mut part).unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&part[..n]);
                if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..end]);
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(buf).unwrap());
            stream.write_all(body.as_bytes()).unwrap();
        }
        requests
    });
    (base, join)
}
fn response(status: &str, body: &str) -> &'static str {
    Box::leak(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_boxed_str())
}
#[derive(Default)]
struct Store(Option<Tokens>);
impl TokenStore for Store {
    fn load(&mut self) -> Result<Option<Tokens>, Error> {
        Ok(self.0.clone())
    }
    fn save(&mut self, value: &Tokens) -> Result<(), Error> {
        self.0 = Some(value.clone());
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
fn refresh_rotation_then_distinct_ticket_requests_preserve_prefix_and_headers() {
    let (base, server) = fixture(vec![
        response(
            "200 OK",
            r#"{"access_token":"new","refresh_token":"rotated","expires_at":9999}"#,
        ),
        response("200 OK", r#"{"ticket":"one"}"#),
        response("200 OK", r#"{"ticket":"two"}"#),
    ]);
    let endpoint = Endpoint::new(&base, &[("x-access".into(), "edge".into())]).unwrap();
    let service = Gateway::new(
        endpoint,
        "fixture",
        Store(Some(Tokens {
            access_token: "old".into(),
            refresh_token: "refresh".into(),
            expires_at: 1,
            provider: "p".into(),
            user_id: String::new(),
        })),
    )
    .unwrap();
    let first = service.ws_ticket(2).unwrap();
    let second = service.ws_ticket(2).unwrap();
    assert_eq!(first.url.path(), "/prefix/api/ws");
    assert_ne!(first.url.query(), second.url.query());
    assert_eq!(first.headers["x-access"], "edge");
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("POST /prefix/auth/native/refresh "));
    assert!(requests[0].contains("refresh_token\":\"refresh"));
    assert!(
        requests[1]
            .to_lowercase()
            .contains("authorization: bearer new")
    );
    assert!(
        requests
            .iter()
            .all(|r| r.to_lowercase().contains("x-access: edge"))
    );
}
#[test]
fn stale_team_only_retries_exact_code_and_returns_picker() {
    let (base, server) = fixture(vec![
        response("403 Forbidden", r#"{"error":"org_access_denied"}"#),
        response(
            "409 Conflict",
            r#"{"error":"org_selection_required","orgs":[{"id":"team"}]}"#,
        ),
    ]);
    let endpoint = Endpoint::new(&base, &[]).unwrap();
    assert!(
        matches!(discover(&endpoint, &Session::Cookie("session=private".into()), Some("old")), Ok(Discovery::SelectTeam(orgs)) if orgs.len() == 1)
    );
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /prefix/api/agents?org=old "));
    assert!(requests[1].starts_with("GET /prefix/api/agents "));
    assert!(requests.iter().all(|r| r.contains("session=private")));
}
#[test]
fn rejects_unadvertised_native_flow_and_unsafe_base() {
    let ep = Endpoint::new("http://127.0.0.1:1/prefix", &[]).unwrap();
    assert!(Login::begin(&ep, &json!({}), None).is_err());
    assert!(Endpoint::new("https://user:pass@example.com", &[]).is_err());
}
#[test]
fn callback_requires_state_and_redeems_matching_pkce_once() {
    let (base, server) = fixture(vec![response(
        "200 OK",
        r#"{"access_token":"private","refresh_token":"next","expires_at":9999}"#,
    )]);
    let endpoint = Endpoint::new(&base, &[]).unwrap();
    let mut login = Login::begin(&endpoint, &json!({"auth_flows":["native_pkce"]}), None).unwrap();
    let authorize = Url::parse(&login.authorize_url).unwrap();
    let params: std::collections::HashMap<_, _> = authorize.query_pairs().into_owned().collect();
    let redirect = Url::parse(&params["redirect_uri"]).unwrap();
    let port = redirect.port().unwrap();
    for state in ["wrong", params["state"].as_str()] {
        let mut stream =
            std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(
            stream,
            "GET /callback?code=fixture&state={state} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with(if state == "wrong" {
            "HTTP/1.1 400"
        } else {
            "HTTP/1.1 200"
        }));
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let tokens = loop {
        if let Some(result) = login.poll() {
            break result.unwrap();
        }
        assert!(std::time::Instant::now() < deadline);
        thread::yield_now();
    };
    assert_eq!(tokens.access_token, "private");
    let requests = server.join().unwrap();
    let body = requests[0].split("\r\n\r\n").nth(1).unwrap();
    let body: Value = serde_json::from_str(body).unwrap();
    let verifier = body["code_verifier"].as_str().unwrap();
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        params["code_challenge"]
    );
    assert_eq!(body["code"], "fixture");
}

#[derive(Clone)]
struct SharedStore(std::sync::Arc<Mutex<Option<Tokens>>>);
impl TokenStore for SharedStore {
    fn load(&mut self) -> Result<Option<Tokens>, Error> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&mut self, value: &Tokens) -> Result<(), Error> {
        *self.0.lock().unwrap() = Some(value.clone());
        Ok(())
    }
    fn compare_and_swap(
        &mut self,
        expected: &Tokens,
        replacement: Option<&Tokens>,
    ) -> Result<bool, Error> {
        let mut value = self.0.lock().unwrap();
        if value.as_ref() != Some(expected) {
            return Ok(false);
        }
        *value = replacement.cloned();
        Ok(true)
    }
}
#[test]
fn distinct_gateways_share_refresh_rotation() {
    let (base, server) = fixture(vec![
        response(
            "200 OK",
            r#"{"access_token":"new","refresh_token":"next","expires_at":9999}"#,
        ),
        response("200 OK", r#"{"ok":true}"#),
        response("200 OK", r#"{"ok":true}"#),
    ]);
    let store = SharedStore(std::sync::Arc::new(Mutex::new(Some(Tokens {
        access_token: "old".into(),
        refresh_token: "once".into(),
        expires_at: 1,
        provider: "p".into(),
        user_id: "u".into(),
    }))));
    let ep = Endpoint::new(&base, &[]).unwrap();
    let a = Gateway::new(ep.clone(), "cloud:source:user", store.clone()).unwrap();
    let b = Gateway::new(ep, "cloud:source:user", store).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let t = {
        let barrier = barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            a.rest(reqwest::Method::GET, "/api/status", None, 2)
                .unwrap()
        })
    };
    barrier.wait();
    b.rest(reqwest::Method::GET, "/api/status", None, 2)
        .unwrap();
    t.join().unwrap();
    let requests = server.join().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("POST /prefix/auth/native/refresh "))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.to_lowercase().contains("authorization: bearer new"))
            .count(),
        2
    );
}
#[test]
fn system_browser_cannot_use_private_access_headers() {
    let ep = Endpoint::new(
        "https://example.test",
        &[("x-access".into(), "secret".into())],
    )
    .unwrap();
    assert!(matches!(
        Login::begin(&ep, &json!({"auth_flows":["native_pkce"]}), None),
        Err(Error::EmbeddedRequired)
    ));
}
#[test]
fn unrelated_team_errors_do_not_retry() {
    let (base, server) = fixture(vec![response("403 Forbidden", r#"{"error":"other"}"#)]);
    let ep = Endpoint::new(&base, &[]).unwrap();
    assert!(
        matches!(discover(&ep, &Session::Cookie("private".into()), Some("stale")), Err(Error::Http(403, Some(code))) if code == "other")
    );
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn websocket_tickets_are_consumed_once_per_connection() {
    use tungstenite::{
        accept_hdr,
        handshake::server::{ErrorResponse, Request, Response},
        http::StatusCode,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut issued = 0;
        let mut consumed = std::collections::HashSet::new();
        for _ in 0..5 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut peek = [0; 4096];
            let n = stream.peek(&mut peek).unwrap();
            if String::from_utf8_lossy(&peek[..n]).starts_with("POST ") {
                let mut buf = [0; 4096];
                let _ = stream.read(&mut buf).unwrap();
                issued += 1;
                let body = format!("{{\"ticket\":\"{issued}\"}}");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            } else {
                let result = accept_hdr(
                    stream,
                    |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
                        let ticket = request
                            .uri()
                            .query()
                            .unwrap_or("")
                            .strip_prefix("ticket=")
                            .unwrap_or("");
                        if ticket.is_empty() || !consumed.insert(ticket.to_owned()) {
                            return Err(tungstenite::http::Response::builder()
                                .status(StatusCode::UNAUTHORIZED)
                                .body(Some("replay".into()))
                                .unwrap());
                        }
                        Ok(response)
                    },
                );
                if let Ok(mut ws) = result {
                    let _ = ws.close(None);
                }
            }
        }
    });
    let ep = Endpoint::new(&format!("http://{address}"), &[]).unwrap();
    let session = Session::Cookie("fixture=private".into());
    let first = ws_ticket_with_session(&ep, &session).unwrap();
    let url = first.url.to_string();
    assert!(tungstenite::connect(first.into_request().unwrap()).is_ok());
    assert!(tungstenite::connect(url.as_str()).is_err());
    let second = ws_ticket_with_session(&ep, &session).unwrap();
    assert_ne!(url, second.url.as_str());
    assert!(tungstenite::connect(second.into_request().unwrap()).is_ok());
    server.join().unwrap();
}

#[test]
fn portal_session_is_origin_bound_and_cleared_on_logout() {
    let (base, server) = fixture(vec![response("200 OK", r#"{"agents":[]}"#)]);
    let ep = Endpoint::new(&base, &[]).unwrap();
    let mut session = PortalSession::new(ep.clone());
    assert!(
        session
            .set_cookie_from_owned_view("https://other.test", "fixture=private".into())
            .is_err()
    );
    session
        .set_cookie_from_owned_view(&ep.origin(), "fixture=private".into())
        .unwrap();
    session.select_team(Some("team".into()));
    assert!(matches!(session.discover(), Ok(Discovery::Agents(_))));
    session.logout();
    assert!(matches!(session.discover(), Err(Error::Reauthenticate)));
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /prefix/api/agents?org=team "));
}
#[test]
fn login_times_out_without_redeeming() {
    let ep = Endpoint::new("http://127.0.0.1:1", &[]).unwrap();
    let mut login = Login::begin(&ep, &json!({"auth_flows":["native_pkce"]}), None).unwrap();
    login.deadline = Instant::now();
    assert!(matches!(login.poll(), Some(Err(Error::Timeout))));
}

#[test]
fn callback_rejects_wrong_host_and_drop_cancels_listener() {
    let ep = Endpoint::new("http://127.0.0.1:1", &[]).unwrap();
    let login = Login::begin(&ep, &json!({"auth_flows":["native_pkce"]}), None).unwrap();
    let url = Url::parse(&login.authorize_url).unwrap();
    let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let redirect = Url::parse(&params["redirect_uri"]).unwrap();
    let port = redirect.port().unwrap();
    let mut stream = std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        stream,
        "GET /callback?code=x&state={} HTTP/1.1\r\nHost: attacker.test\r\n\r\n",
        params["state"]
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 400"));
    drop(login);
    let deadline = Instant::now() + Duration::from_secs(2);
    while std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_ok() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}

#[test]
fn credentialed_requests_never_follow_redirects() {
    let (base, server) = fixture(vec![
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ]);
    let ep = Endpoint::new(&base, &[("x-access".into(), "private".into())]).unwrap();
    assert!(matches!(
        discover(&ep, &Session::Cookie("private".into()), None),
        Err(Error::Http(302, None))
    ));
    assert_eq!(server.join().unwrap().len(), 1);
}

fn expired() -> Tokens {
    Tokens {
        access_token: "old".into(),
        refresh_token: "once".into(),
        expires_at: 1,
        provider: "p".into(),
        user_id: "u".into(),
    }
}
fn gated_refresh(
    reply: &'static str,
) -> (
    String,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/prefix", listener.local_addr().unwrap());
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let join = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 4096];
        let n = stream.read(&mut buf).unwrap();
        assert!(
            String::from_utf8_lossy(&buf[..n]).starts_with("POST /prefix/auth/native/refresh ")
        );
        accepted_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        stream.write_all(reply.as_bytes()).unwrap();
    });
    (base, accepted_rx, release_tx, join)
}
fn shared() -> SharedStore {
    SharedStore(Arc::new(Mutex::new(Some(expired()))))
}
fn gateway(base: &str, identity: &str, store: SharedStore) -> Gateway<SharedStore> {
    Gateway::new(Endpoint::new(base, &[]).unwrap(), identity, store).unwrap()
}

#[test]
fn refresh_subscribers_cancel_independently_and_initiator_can_cancel() {
    let (base, accepted, release, server) = gated_refresh(response(
        "200 OK",
        r#"{"access_token":"new","refresh_token":"next","expires_at":9999}"#,
    ));
    let store = shared();
    let a = gateway(&base, "same-record", store.clone());
    let b = gateway(&base, "same-record", store.clone());
    let initiator = a.access_async(false, None, 2).unwrap();
    accepted.recv_timeout(Duration::from_secs(5)).unwrap();
    let cancelled = b.access_async(false, None, 2).unwrap();
    let survivor = b.access_async(false, None, 2).unwrap();
    assert!(matches!(survivor.try_recv(), Ok(None)));
    drop(cancelled);
    drop(initiator);
    release.send(()).unwrap();
    assert_eq!(survivor.wait().unwrap(), "new");
    assert_eq!(
        store.0.lock().unwrap().as_ref().unwrap().refresh_token,
        "next"
    );
    server.join().unwrap();
}

#[test]
fn all_cancelled_still_persist_consumed_rotation() {
    let (base, accepted, release, server) = gated_refresh(response(
        "200 OK",
        r#"{"access_token":"new","refresh_token":"next","expires_at":9999}"#,
    ));
    let store = shared();
    let g = gateway(&base, "all-cancel", store.clone());
    let sub = g.access_async(false, None, 2).unwrap();
    accepted.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(sub);
    release.send(()).unwrap();
    server.join().unwrap();
    // A new subscriber joins the still-running flight or observes its persisted result.
    assert_eq!(
        g.access_async(false, None, 2).unwrap().wait().unwrap(),
        "new"
    );
    assert_eq!(
        store.0.lock().unwrap().as_ref().unwrap().refresh_token,
        "next"
    );
}

#[test]
fn explicit_replacement_and_logout_win_over_accepted_rotation() {
    for logout in [false, true] {
        let (base, accepted, release, server) = gated_refresh(response(
            "200 OK",
            r#"{"access_token":"rotated","refresh_token":"next","expires_at":9999}"#,
        ));
        let store = shared();
        let g = gateway(&base, "replace", store.clone());
        let sub = g.access_async(false, None, 2).unwrap();
        accepted.recv_timeout(Duration::from_secs(5)).unwrap();
        if logout {
            *store.0.lock().unwrap() = None;
        } else {
            let mut replacement = expired();
            replacement.access_token = "login".into();
            g.install(replacement).unwrap();
        }
        release.send(()).unwrap();
        if logout {
            assert!(matches!(sub.wait(), Err(Error::Reauthenticate)));
        } else {
            assert_eq!(sub.wait().unwrap(), "login");
        }
        server.join().unwrap();
    }
}

#[test]
fn unrelated_identity_is_not_blocked_and_failed_refresh_publishes_no_token() {
    let (base, accepted, release, server) = gated_refresh(response(
        "500 Internal Server Error",
        r#"{"error":"broken"}"#,
    ));
    let g = gateway(&base, "slow", shared());
    let slow = g.access_async(false, None, 2).unwrap();
    accepted.recv_timeout(Duration::from_secs(5)).unwrap();
    let fresh = shared();
    fresh.0.lock().unwrap().as_mut().unwrap().expires_at = 9999;
    let other = gateway(&base, "other", fresh);
    assert_eq!(
        other.access_async(false, None, 2).unwrap().wait().unwrap(),
        "old"
    );
    release.send(()).unwrap();
    assert!(matches!(slow.wait(), Err(Error::Http(500, _))));
    server.join().unwrap();
}

#[test]
fn prepared_http_binds_origin_path_headers_and_only_retries_safe_get_once() {
    let (base, server) = fixture(vec![response(
        "200 OK",
        r#"{"access_token":"new","refresh_token":"next","expires_at":9999}"#,
    )]);
    let store = shared();
    store.0.lock().unwrap().as_mut().unwrap().expires_at = 9999;
    let endpoint = Endpoint::new(&base, &[("x-access".into(), "edge-secret".into())]).unwrap();
    let g = Gateway::new(endpoint, "runtime-record", store).unwrap();
    let prepared = g
        .prepare_http(reqwest::Method::GET, "/api/files/rawstream?path=a%2Fb", 2)
        .unwrap();
    assert_eq!(prepared.url().path(), "/prefix/api/files/rawstream");
    assert_eq!(prepared.url().query(), Some("path=a%2Fb"));
    assert_eq!(prepared.headers()[AUTHORIZATION], "Bearer old");
    assert!(prepared.headers()[AUTHORIZATION].is_sensitive());
    assert!(prepared.headers()["x-access"].is_sensitive());
    let retried = g.retry_http_unauthorized(&prepared, 2).unwrap();
    assert_eq!(retried.headers()[AUTHORIZATION], "Bearer new");
    assert!(g.retry_http_unauthorized(&retried, 2).is_err());
    let post = g
        .prepare_http(reqwest::Method::POST, "/api/upload", 2)
        .unwrap();
    assert!(g.retry_http_unauthorized(&post, 2).is_err());
    assert!(matches!(
        g.retry_http_unauthorized_until(&post, 2, Instant::now() + Duration::from_secs(1)),
        Err(Error::InvalidBase)
    ));
    assert!(
        g.prepare_http(reqwest::Method::GET, "/%2e%2e/private", 2)
            .is_err()
    );
    assert!(
        g.prepare_http(reqwest::Method::GET, "//other.test/path", 2)
            .is_err()
    );
    assert!(
        g.prepare_http(reqwest::Method::GET, "/path#fragment", 2)
            .is_err()
    );
    server.join().unwrap();
}

#[test]
fn public_probes_use_exact_paths_headers_and_refuse_redirects_and_large_bodies() {
    let oversized = format!("\"{}\"", "x".repeat(1024 * 1024));
    let (base, server) = fixture(vec![
        response("200 OK", r#"{"auth_flows":["native_pkce"]}"#),
        response("200 OK", r#"{"providers":[{"id":"example"}]}"#),
        response("302 Found", r#"{}"#),
        response("200 OK", &oversized),
    ]);
    let endpoint = Endpoint::new(&base, &[("x-edge".into(), "scoped".into())]).unwrap();
    let deadline = || Instant::now() + Duration::from_secs(3);
    assert_eq!(
        endpoint.status(deadline()).unwrap()["auth_flows"][0],
        "native_pkce"
    );
    assert_eq!(
        endpoint.providers(deadline()).unwrap()["providers"][0]["id"],
        "example"
    );
    assert!(matches!(
        endpoint.status(deadline()),
        Err(Error::Http(302, _))
    ));
    assert!(matches!(
        endpoint.providers(deadline()),
        Err(Error::InvalidResponse)
    ));
    let requests = server.join().unwrap();
    for (request, path) in requests.iter().zip([
        "/api/status",
        "/api/auth/providers",
        "/api/status",
        "/api/auth/providers",
    ]) {
        assert!(request.starts_with(&format!("GET /prefix{path} ")));
        let lower = request.to_lowercase();
        assert!(lower.contains("x-edge: scoped"));
        assert!(!lower.contains("authorization:") && !lower.contains("cookie:"));
    }
}

#[test]
fn deadline_bounds_socket_body_read_and_refresh_subscriber_but_rotation_survives() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/prefix", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 4096];
        let _ = stream.read(&mut buf).unwrap();
        thread::sleep(Duration::from_millis(250));
        let body = r#"{"access_token":"new","refresh_token":"next","expires_at":9999}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let endpoint = Endpoint::new(&base, &[]).unwrap();
    let service = Gateway::new(
        endpoint,
        "deadline-rotation",
        Store(Some(Tokens {
            access_token: "old".into(),
            refresh_token: "refresh".into(),
            expires_at: 1,
            provider: String::new(),
            user_id: String::new(),
        })),
    )
    .unwrap();
    let started = Instant::now();
    assert!(matches!(
        service.prepare_http_until(
            reqwest::Method::GET,
            "/api/status",
            2,
            started + Duration::from_millis(80)
        ),
        Err(Error::Timeout)
    ));
    assert!(started.elapsed() < Duration::from_millis(220));
    server.join().unwrap();
    let access = service
        .access_async(false, None, 2)
        .unwrap()
        .wait_until(Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(access, "new");
    assert_eq!(
        service
            .store
            .lock()
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .refresh_token,
        "next"
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/prefix", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 4096];
        let _ = stream.read(&mut buf).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        thread::sleep(Duration::from_millis(250));
        let _ = stream.write_all(br#"{"ticket":"slow"}"#);
    });
    let endpoint = Endpoint::new(&base, &[]).unwrap();
    let service = Gateway::new(
        endpoint,
        "deadline-mint",
        Store(Some(Tokens {
            access_token: "valid".into(),
            refresh_token: String::new(),
            expires_at: 9999,
            provider: String::new(),
            user_id: String::new(),
        })),
    )
    .unwrap();
    let started = Instant::now();
    assert!(matches!(
        service.socket_ticket_until(
            SocketTarget::Gateway,
            &SocketQuery::new("").unwrap(),
            2,
            started + Duration::from_millis(80)
        ),
        Err(Error::Timeout)
    ));
    assert!(started.elapsed() < Duration::from_millis(220));
    server.join().unwrap();
}

#[test]
fn socket_mint_wait_for_headers_obeys_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/prefix", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 4096];
        let n = stream.read(&mut buf).unwrap();
        assert!(
            String::from_utf8_lossy(&buf[..n])
                .starts_with("POST /prefix/api/webapp/socket-ticket?socket=host-terminal ")
        );
        thread::sleep(Duration::from_millis(250));
        let _ =
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"ticket\":\"ok\"}");
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
    let start = Instant::now();
    assert!(matches!(
        cookie.socket_ticket_until(
            &endpoint,
            "source",
            SocketTarget::HostTerminal,
            &SocketQuery::new("profile=x").unwrap(),
            start + Duration::from_millis(80)
        ),
        Err(Error::Timeout)
    ));
    assert!(start.elapsed() < Duration::from_millis(220));
    server.join().unwrap();
}
