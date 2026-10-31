use super::*;
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
};
fn peer(response: &'static str) -> (Connection, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _socket = tungstenite::accept(stream).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut request = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            request.push_str(&line);
        }
        stream.write_all(response.as_bytes()).unwrap();
        request
    });
    (
        Connection::Url {
            endpoint: format!("http://{address}/hermes/"),
            token: Some("synthetic".into()),
        },
        thread,
    )
}
#[test]
fn raw_range_and_head_preserve_auth_owner_headers_and_http_verdict() {
    for head in [false, true] {
        let (connection, thread) = peer(if head {
            "HTTP/1.1 200 OK\r\nContent-Length: 1234\r\nContent-Type: video/webm\r\nContent-Encoding: gzip\r\nAccept-Ranges: bytes\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n"
        } else {
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/12\r\nContent-Type: video/webm\r\nContent-Encoding: gzip\r\nAccept-Ranges: bytes\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n4567"
        });
        let mut gateway = Gateway::connect(&connection).unwrap();
        let mut request = Request::download(
            "/agent/a b.webm?profile=wrong".into(),
            Some("stored-session".into()),
            100,
        );
        request.route = Route::Media;
        request.range = Some("bytes=4-7".into());
        request.if_range = Some("\"fixture\"".into());
        request.method = if head { Method::Head } else { Method::Get };
        let mut output = Vec::new();
        let metadata = gateway
            .stream_file("research", &request, &Cancel::default(), &mut output)
            .unwrap();
        assert_eq!(metadata.status, if head { 200 } else { 206 });
        assert_eq!(metadata.etag.as_deref(), Some("\"fixture\""));
        assert_eq!(metadata.accept_ranges.as_deref(), Some("bytes"));
        assert_eq!(metadata.content_length, Some(if head { 1234 } else { 4 }));
        if head {
            assert!(output.is_empty());
        } else {
            assert_eq!(output, b"4567");
            assert_eq!(metadata.content_range.as_deref(), Some("bytes 4-7/12"));
        }
        let request = thread.join().unwrap();
        assert!(
            request
                .to_ascii_lowercase()
                .contains("accept-encoding: identity\r\n")
        );
        let target = request
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        let url = Url::parse(&format!("http://localhost{target}")).unwrap();
        let query: HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(url.path(), "/hermes/api/files/stream");
        assert_eq!(query["profile"], "research");
        assert_eq!(query["session_id"], "stored-session");
        assert_eq!(query["token"], "synthetic");
        assert_eq!(query["path"], "/agent/a b.webm?profile=wrong");
        assert_eq!(query.len(), 4);
        assert!(
            request
                .to_ascii_lowercase()
                .contains("range: bytes=4-7\r\n")
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("if-range: \"fixture\"\r\n")
        );
    }
}
#[test]
fn stalled_body_is_cancelled_and_redirect_never_reaches_other_origin() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (ready_send, ready) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _socket = tungstenite::accept(stream).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&mut stream);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n").unwrap();
        ready_send.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(900));
    });
    let cancel = Cancel::default();
    let task_cancel = cancel.clone();
    let task = std::thread::spawn(move || {
        let mut gateway = Gateway::connect(&Connection::Url {
            endpoint: format!("http://{address}"),
            token: None,
        })
        .unwrap();
        let mut output = Vec::new();
        let result = gateway.stream_file(
            "work",
            &Request::download("/large.png".into(), Some("session".into()), 1024),
            &task_cancel,
            &mut output,
        );
        result
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    cancel.cancel();
    assert!(task.join().unwrap().unwrap_err().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_millis(300));
    server.join().unwrap();
    let (connection, thread) = peer(
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/steal\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope",
    );
    let mut gateway = Gateway::connect(&connection).unwrap();
    let mut output = Vec::new();
    let metadata = gateway
        .stream_file(
            "work",
            &Request::download("/a.png".into(), None, 100),
            &Cancel::default(),
            &mut output,
        )
        .unwrap();
    assert_eq!(metadata.status, 302);
    assert!(output.is_empty());
    thread.join().unwrap();
}
#[test]
fn transfer_limits_and_cache_not_modified_never_write_untrusted_error_bodies() {
    for (response, limit, expected) in [
        (
            "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n",
            10,
            0,
        ),
        (
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n0\r\n\r\n",
            4,
            0,
        ),
        (
            "HTTP/1.1 304 Not Modified\r\nETag: \"cached\"\r\nConnection: close\r\n\r\n",
            100,
            304,
        ),
    ] {
        let (connection, thread) = peer(response);
        let mut gateway = Gateway::connect(&connection).unwrap();
        let mut output = Vec::new();
        let mut request = Request::download("/a.png".into(), None, limit);
        request.if_none_match = Some("\"cached\"".into());
        let result = gateway.stream_file("work", &request, &Cancel::default(), &mut output);
        if expected == 0 {
            assert!(result.unwrap_err().contains("limit"));
        } else {
            assert_eq!(result.unwrap().status, expected);
        }
        assert!(output.is_empty());
        thread.join().unwrap();
    }
}

#[test]
fn cloud_actual_methods_range_refresh_and_multipart_are_bound_to_the_request() {
    use crate::gateway::prepared::{CloudHttp, CloudRuntime, WirePolicy};
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use tungstenite::client::IntoClientRequest;
    struct Runtime {
        methods: Mutex<Vec<reqwest::Method>>,
        retries: AtomicUsize,
    }
    impl CloudRuntime for Runtime {
        fn prepare_http(
            &self,
            method: reqwest::Method,
            url: &Url,
            _: Instant,
        ) -> Result<CloudHttp, String> {
            self.methods.lock().unwrap().push(method.clone());
            let mut headers = HeaderMap::new();
            headers.insert("authorization", HeaderValue::from_static("old-fixture"));
            Ok(CloudHttp {
                url: url.clone(),
                headers,
                receipt: Box::new(method),
            })
        }
        fn retry_http_unauthorized(
            &self,
            previous: CloudHttp,
            _: Instant,
        ) -> Result<CloudHttp, String> {
            assert_eq!(
                *previous.receipt.downcast::<reqwest::Method>().unwrap(),
                reqwest::Method::GET
            );
            self.retries.fetch_add(1, Ordering::SeqCst);
            let mut headers = HeaderMap::new();
            headers.insert("authorization", HeaderValue::from_static("new-fixture"));
            Ok(CloudHttp {
                url: previous.url,
                headers,
                receipt: Box::new(()),
            })
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
            "stream-fixture".into()
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/prefix/", listener.local_addr().unwrap());
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    other.set_nonblocking(true).unwrap();
    let other_address = other.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        let _ws = tungstenite::accept(socket).unwrap();
        let mut requests = Vec::new();
        for response in [
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/12\r\nContent-Type: video/webm\r\nConnection: close\r\n\r\n4567",
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        ] {
            let (mut socket, _) = listener.accept().unwrap();
            requests.push(crate::gateway::tests::http_request(&mut socket));
            socket.write_all(response.as_bytes()).unwrap();
        }
        let (mut roster, _) = listener.accept().unwrap();
        requests.push(crate::gateway::tests::http_request(&mut roster));
        let body = r#"{"profiles":[{"name":"work"}]}"#;
        write!(
            roster,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let (socket, _) = listener.accept().unwrap();
        let _audio_ws = tungstenite::accept(socket).unwrap();
        let (mut audio, _) = listener.accept().unwrap();
        requests.push(crate::gateway::tests::http_request(&mut audio));
        let body = r#"{"ok":true,"transcript":"fixture"}"#;
        write!(
            audio,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let (mut redirect, _) = listener.accept().unwrap();
        requests.push(crate::gateway::tests::http_request(&mut redirect));
        write!(redirect, "HTTP/1.1 302 Found\r\nLocation: http://{other_address}/escape\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        requests
    });
    let runtime = Arc::new(Runtime {
        methods: Mutex::new(Vec::new()),
        retries: AtomicUsize::new(0),
    });
    let connection = Connection::Routed {
        actual: Box::new(Connection::Url {
            endpoint,
            token: None,
        }),
        owner: Arc::new(crate::source::RouteOwner::legacy("cloud-stream-test").unwrap()),
        policy: Arc::new(
            WirePolicy::cloud(
                crate::gateway::route::ProfilePolicy::identity(),
                runtime.clone(),
            )
            .unwrap(),
        ),
    };
    let mut gateway = Gateway::connect(&connection).unwrap();
    let mut request = Request::download("/file name".into(), Some("session".into()), 32);
    request.route = Route::Media;
    request.range = Some("bytes=4-7".into());
    request.method = Method::Head;
    let mut output = Vec::new();
    assert_eq!(
        gateway
            .stream_file("work", &request, &Cancel::default(), &mut output)
            .unwrap()
            .status,
        401
    );
    request.method = Method::Get;
    let metadata = gateway
        .stream_file("work", &request, &Cancel::default(), &mut output)
        .unwrap();
    assert_eq!(metadata.content_range.as_deref(), Some("bytes 4-7/12"));
    assert_eq!(output, b"4567");
    let upload = crate::pages::PageRequest::post("/api/fs/upload", serde_json::Value::Null).upload(
        "file.txt",
        "text/plain",
        b"payload".to_vec(),
    );
    assert!(gateway.page_request("work", &upload).is_err());
    let roster = crate::gateway::roster::read(
        &connection,
        Instant::now() + Duration::from_secs(5),
        false,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    assert!(roster.profiles.iter().any(|profile| profile.name == "work"));
    let mut audio =
        crate::gateway::audio::AudioGateway::connect(&connection, "voice-owner").unwrap();
    assert_eq!(
        audio.transcribe(&[1, 2], "audio/wav").unwrap().transcript,
        "fixture"
    );
    let mut redirect_output = Vec::new();
    assert_eq!(
        gateway
            .stream_file("work", &request, &Cancel::default(), &mut redirect_output)
            .unwrap()
            .status,
        302
    );
    assert!(redirect_output.is_empty());
    assert!(other.accept().is_err());
    assert_eq!(runtime.retries.load(Ordering::SeqCst), 1);
    assert_eq!(
        *runtime.methods.lock().unwrap(),
        vec![
            reqwest::Method::HEAD,
            reqwest::Method::GET,
            reqwest::Method::POST,
            reqwest::Method::GET,
            reqwest::Method::POST,
            reqwest::Method::GET
        ]
    );
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("HEAD /prefix/api/files/stream?"));
    for request in &requests[1..3] {
        assert!(request.starts_with("GET /prefix/api/files/stream?"));
        assert!(
            request.contains("profile=work")
                && request.contains("path=%2Ffile+name")
                && request.contains("session_id=session")
        );
        assert!(request.to_ascii_lowercase().contains("range: bytes=4-7"));
    }
    assert!(requests[1].contains("old-fixture"));
    assert!(requests[2].contains("new-fixture"));
    assert!(requests[3].starts_with("POST /prefix/api/fs/upload?profile=work"));
    assert!(
        requests[3]
            .to_ascii_lowercase()
            .contains("multipart/form-data")
    );
    assert!(requests[4].starts_with("GET /prefix/api/profiles?profile="));
    assert!(requests[4].contains("old-fixture"));
    assert!(requests[5].starts_with("POST /prefix/api/audio/transcribe?profile=voice-owner"));
    assert!(requests[5].contains("old-fixture"));
}

#[test]
fn streaming_delivers_first_chunk_before_response_finishes() {
    struct Observed(std::sync::mpsc::Sender<Vec<u8>>);
    impl std::io::Write for Observed {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.send(bytes.to_vec()).unwrap();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (finish, wait) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        let _ws = tungstenite::accept(socket).unwrap();
        let (mut socket, _) = listener.accept().unwrap();
        crate::gateway::tests::http_request(&mut socket);
        socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n").unwrap();
        wait.recv_timeout(Duration::from_secs(3)).unwrap();
    });
    let cancel = Cancel::default();
    let cancel_worker = cancel.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut gateway = Gateway::connect(&Connection::Url {
            endpoint: format!("http://{address}/"),
            token: None,
        })
        .unwrap();
        gateway.stream_file(
            "work",
            &Request::download("/large".into(), None, 1024),
            &cancel_worker,
            &mut Observed(sender),
        )
    });
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        b"hello"
    );
    cancel.cancel();
    assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
    finish.send(()).unwrap();
    server.join().unwrap();
}
