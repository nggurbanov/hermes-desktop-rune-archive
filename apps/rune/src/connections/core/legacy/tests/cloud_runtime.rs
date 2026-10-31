use super::*;
use crate::connections::cloud::{Error, Tokens};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

struct Store {
    tokens: Arc<Mutex<Option<Tokens>>>,
    reads: Arc<AtomicUsize>,
}
impl TokenStore for Store {
    fn load(&mut self) -> Result<Option<Tokens>, Error> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.tokens.lock().unwrap().clone())
    }
    fn save(&mut self, tokens: &Tokens) -> Result<(), Error> {
        *self.tokens.lock().unwrap() = Some(tokens.clone());
        Ok(())
    }
    fn compare_and_swap(&mut self, old: &Tokens, new: Option<&Tokens>) -> Result<bool, Error> {
        let mut slot = self.tokens.lock().unwrap();
        if slot.as_ref() != Some(old) {
            return Ok(false);
        }
        *slot = new.cloned();
        Ok(true)
    }
}
fn memory_store(token: &str) -> (Store, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    (
        Store {
            tokens: Arc::new(Mutex::new(Some(Tokens {
                access_token: token.into(),
                refresh_token: "refresh-secret".into(),
                expires_at: i64::MAX,
                provider: String::new(),
                user_id: String::new(),
            }))),
            reads: reads.clone(),
        },
        reads,
    )
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn read_request(stream: &mut std::net::TcpStream) -> String {
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
    String::from_utf8(bytes).unwrap()
}
fn respond(stream: &mut std::net::TcpStream, body: &str) {
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
}
#[test]
fn scope_and_receipts_reject_before_store_access() {
    let (store, reads) = memory_store("secret-a");
    let runtime = CloudRuntimeAdapter::new(
        "http://localhost:32123/prefix",
        &[],
        "source-a/credential-a",
        store,
    )
    .unwrap();
    for raw in [
        "http://other:32123/prefix/api/x",
        "http://localhost:32124/prefix/api/x",
        "http://localhost:32123/prefix-other/api/x",
        "http://user@localhost:32123/prefix/api/x",
        "http://localhost:32123/prefix/api/x#fragment",
        "http://localhost:32123/prefix/api/x?to%6ben=bad",
        "http://localhost:32123/prefix/%2e%2e/x",
    ] {
        assert!(
            runtime
                .prepare_http(Method::GET, &Url::parse(raw).unwrap(), deadline())
                .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let url = Url::parse("http://localhost:32123/prefix/api/x?q=a%20b").unwrap();
    let prepared = runtime.prepare_http(Method::GET, &url, deadline()).unwrap();
    assert_eq!(prepared.url, url);
    assert!(prepared.headers[reqwest::header::AUTHORIZATION].is_sensitive());
    let (other_store, other_reads) = memory_store("secret-b");
    let other = CloudRuntimeAdapter::new(
        "http://localhost:32123/prefix",
        &[],
        "source-b/credential-b",
        other_store,
    )
    .unwrap();
    assert_ne!(runtime.runtime_fingerprint(), other.runtime_fingerprint());
    assert!(other.retry_http_unauthorized(prepared, deadline()).is_err());
    assert_eq!(other_reads.load(Ordering::SeqCst), 0);
    let other_request = other.prepare_http(Method::GET, &url, deadline()).unwrap();
    assert_eq!(
        other_request.headers[reqwest::header::AUTHORIZATION],
        "Bearer secret-b"
    );
    let poisoned = CloudHttp {
        url,
        headers: Default::default(),
        receipt: Box::new(()),
    };
    assert!(
        runtime
            .retry_http_unauthorized(poisoned, deadline())
            .is_err()
    );
}
#[test]
fn get_retry_rotates_real_service_and_is_single_use() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/prefix", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        let request = read_request(&mut first);
        assert!(request.starts_with("GET /prefix/api/items?q=one%20two "));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer old-secret")
        );
        write!(
            first,
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        assert!(request.starts_with("POST /prefix/auth/native/refresh "));
        respond(
            &mut stream,
            r#"{"access_token":"new-secret","refresh_token":"next","expires_at":9223372036854775807}"#,
        );
        let (mut last, _) = listener.accept().unwrap();
        let request = read_request(&mut last);
        assert!(request.starts_with("GET /prefix/api/items?q=one%20two "));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer new-secret")
        );
        respond(&mut last, "{}");
    });
    let (store, _) = memory_store("old-secret");
    let runtime = CloudRuntimeAdapter::new(&base, &[], "source-a/credential-a", store).unwrap();
    let url = Url::parse(&format!("{base}/api/items?q=one%20two")).unwrap();
    let initial = runtime.prepare_http(Method::GET, &url, deadline()).unwrap();
    assert_eq!(
        initial.headers[reqwest::header::AUTHORIZATION],
        "Bearer old-secret"
    );
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(initial.url.clone())
        .headers(initial.headers.clone())
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let retry = runtime
        .retry_http_unauthorized(initial, deadline())
        .unwrap();
    assert_eq!(retry.url, url);
    assert_eq!(
        retry.headers[reqwest::header::AUTHORIZATION],
        "Bearer new-secret"
    );
    let response = client
        .get(retry.url.clone())
        .headers(retry.headers.clone())
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(runtime.retry_http_unauthorized(retry, deadline()).is_err());
    server.join().unwrap();
}
#[test]
fn websocket_mints_exact_target_and_replay_is_rejected() {
    use tungstenite::handshake::server::{ErrorResponse, Request, Response};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut mint, _) = listener.accept().unwrap();
        let req = read_request(&mut mint);
        assert!(req.starts_with("POST /prefix/api/webapp/socket-ticket?socket=host-terminal "));
        assert!(
            req.to_lowercase()
                .contains("authorization: bearer socket-secret")
        );
        respond(&mut mint, r#"{"ticket":"once"}"#);
        for index in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            let result = tungstenite::accept_hdr(
                stream,
                |req: &Request, response: Response| -> Result<Response, ErrorResponse> {
                    assert_eq!(
                        req.uri().to_string(),
                        "/prefix/api/host-terminal?profile=work&ticket=once"
                    );
                    if index == 1 {
                        return Err(tungstenite::http::Response::builder()
                            .status(401)
                            .body(Some("replay".into()))
                            .unwrap());
                    }
                    Ok(response)
                },
            );
            if let Ok(mut socket) = result {
                let _ = socket.close(None);
            }
        }
    });
    let (store, reads) = memory_store("socket-secret");
    let runtime =
        CloudRuntimeAdapter::new(&format!("http://{address}/prefix"), &[], "source", store)
            .unwrap();
    let invalid = Url::parse(&format!("ws://{address}/prefix/api/ws?ticket=old")).unwrap();
    assert!(runtime.prepare_ws(&invalid, deadline()).is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let url = Url::parse(&format!(
        "ws://{address}/prefix/api/host-terminal?profile=work"
    ))
    .unwrap();
    assert!(
        runtime
            .prepare_ws(&url, Instant::now() - Duration::from_secs(1))
            .is_err()
    );
    use crate::gateway::{
        Connection,
        prepared::{PreparedTransport, WirePolicy, WsPurpose},
        route::ProfilePolicy,
    };
    let policy = WirePolicy::cloud(ProfilePolicy::identity(), runtime.clone()).unwrap();
    let connection = Connection::Routed {
        actual: Box::new(Connection::Url {
            endpoint: format!("http://{address}/prefix"),
            token: None,
        }),
        owner: Arc::new(crate::source::RouteOwner::legacy("cloud-loopback").unwrap()),
        policy: Arc::new(policy),
    };
    let transport = PreparedTransport::from_connection(&connection, deadline()).unwrap();
    let request = transport
        .prepare_ws(url.clone(), None, WsPurpose::Interactive, deadline())
        .unwrap()
        .request;
    let replay = request.clone();
    let (mut socket, _) = tungstenite::connect(request).unwrap();
    let _ = socket.close(None);
    assert!(tungstenite::connect(replay).is_err());
    server.join().unwrap();
}

#[test]
fn retired_store_does_not_reuse_previously_loaded_auth() {
    use std::sync::atomic::AtomicBool;
    struct Retiring {
        retired: Arc<AtomicBool>,
        inner: Store,
    }
    impl TokenStore for Retiring {
        fn load(&mut self) -> Result<Option<Tokens>, Error> {
            if self.retired.load(Ordering::SeqCst) {
                return Err(Error::Store);
            }
            self.inner.load()
        }
        fn save(&mut self, tokens: &Tokens) -> Result<(), Error> {
            self.inner.save(tokens)
        }
        fn compare_and_swap(&mut self, old: &Tokens, new: Option<&Tokens>) -> Result<bool, Error> {
            self.inner.compare_and_swap(old, new)
        }
    }
    let retired = Arc::new(AtomicBool::new(false));
    let (inner, _) = memory_store("retired-secret");
    let runtime = CloudRuntimeAdapter::new(
        "http://localhost:32123/prefix",
        &[],
        "retired-source",
        Retiring {
            retired: retired.clone(),
            inner,
        },
    )
    .unwrap();
    let url = Url::parse("http://localhost:32123/prefix/api/x").unwrap();
    runtime.prepare_http(Method::GET, &url, deadline()).unwrap();
    retired.store(true, Ordering::SeqCst);
    assert!(runtime.prepare_http(Method::GET, &url, deadline()).is_err());
}

#[test]
fn socket_mint_respects_deadline_during_network_io() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        thread::sleep(Duration::from_millis(350));
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\n{{\"ticket\":\"late\"}}"
        );
    });
    let (store, _) = memory_store("secret");
    let runtime = CloudRuntimeAdapter::new(
        &format!("http://{address}/prefix"),
        &[],
        "deadline-source",
        store,
    )
    .unwrap();
    let url = Url::parse(&format!("ws://{address}/prefix/api/ws")).unwrap();
    let started = Instant::now();
    assert!(
        runtime
            .prepare_ws(&url, started + Duration::from_millis(100))
            .is_err()
    );
    assert!(started.elapsed() < Duration::from_millis(300));
    server.join().unwrap();
}
