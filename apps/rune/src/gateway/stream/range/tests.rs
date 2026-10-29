use super::*;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
#[derive(Clone, Copy)]
enum Behavior {
    Ranges,
    NoValidator,
    SequentialTruncated,
    WeakValidator,
    Ignored,
    WrongRange,
    Changed,
    Truncated,
    Redirect,
    Stalled,
}
struct Peer {
    connection: Connection,
    requests: Arc<Mutex<Vec<(String, HashMap<String, String>)>>>,
    stop: Arc<AtomicBool>,
    stalled_headers: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            join.join().unwrap();
        }
    }
}
fn fixture(length: u64, behavior: Behavior) -> Peer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let stalled_headers = Arc::new(AtomicBool::new(false));
    let headers_ready = stalled_headers.clone();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let join = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _socket = tungstenite::accept(stream).unwrap();
        listener.set_nonblocking(true).unwrap();
        while !worker_stop.load(Ordering::Acquire) {
            let (mut stream, _) = match listener.accept() {
                Ok(stream) => stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut headers = HashMap::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                let (name, value) = line.split_once(':').unwrap();
                headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
            }
            recorded
                .lock()
                .unwrap()
                .push((first.clone(), headers.clone()));
            let etag = match behavior {
                Behavior::NoValidator | Behavior::SequentialTruncated => "",
                Behavior::WeakValidator => "ETag: W/\"fixture\"\r\n",
                _ => "ETag: \"fixture\"\r\n",
            };
            if first.starts_with("HEAD ") {
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\n{etag}Connection: close\r\n\r\n"
                );
                stream.write_all(head.as_bytes()).unwrap();
                continue;
            }
            if matches!(behavior, Behavior::Stalled)
                && headers
                    .get("range")
                    .is_some_and(|range| !range.starts_with("bytes=0-"))
            {
                let range = headers["range"].strip_prefix("bytes=").unwrap();
                stream.write_all(format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {BLOCK}\r\nContent-Range: bytes {range}/{length}\r\n{etag}Connection: close\r\n\r\n").as_bytes()).unwrap();
                headers_ready.store(true, Ordering::Release);
                while !worker_stop.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                continue;
            }
            if matches!(behavior, Behavior::Redirect)
                && headers
                    .get("range")
                    .is_some_and(|range| !range.starts_with("bytes=0-"))
            {
                stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/steal\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope").unwrap();
                continue;
            }
            if let Some(range) = headers.get("range") {
                let (start, end) = range
                    .strip_prefix("bytes=")
                    .unwrap()
                    .split_once('-')
                    .unwrap();
                let start: u64 = start.parse().unwrap();
                let end: u64 = end.parse().unwrap();
                let body: Vec<u8> = (start..=end).map(|n| (n % 251) as u8).collect();
                let response = if matches!(behavior, Behavior::Ignored) {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n{etag}Connection: close\r\n\r\n"
                    )
                } else {
                    let range_start = if matches!(behavior, Behavior::WrongRange) && start != 0 {
                        start + 1
                    } else {
                        start
                    };
                    let etag = if matches!(behavior, Behavior::Changed) && start != 0 {
                        "ETag: \"replacement\"\r\n"
                    } else {
                        etag
                    };
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {range_start}-{end}/{length}\r\n{etag}Connection: close\r\n\r\n",
                        body.len()
                    )
                };
                if stream.write_all(response.as_bytes()).is_ok() {
                    let count = if matches!(behavior, Behavior::Truncated) && start != 0 {
                        body.len() / 2
                    } else {
                        body.len()
                    };
                    let _ = stream.write_all(&body[..count]);
                }
            } else {
                assert!(length <= 2 * BLOCK);
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n{etag}Connection: close\r\n\r\n").as_bytes()).unwrap();
                let body: Vec<u8> = (0..length).map(|n| (n % 251) as u8).collect();
                let count = if matches!(behavior, Behavior::SequentialTruncated) {
                    body.len() / 2
                } else {
                    body.len()
                };
                let _ = stream.write_all(&body[..count]);
            }
        }
    });
    Peer {
        connection: Connection::Url {
            endpoint: format!("http://{address}/owned/"),
            token: Some("synthetic-token".into()),
        },
        requests,
        stop,
        stalled_headers,
        join: Some(join),
    }
}
fn open(peer: &Peer, cancel: Cancel) -> RangeSource {
    RangeSource::open(
        &peer.connection,
        "research",
        "/sandbox/audio.wav?profile=other",
        Some("session-7".into()),
        cancel,
    )
    .unwrap()
}
#[test]
fn sparse_multigigabyte_media_seeks_with_bounded_lru_and_frozen_authenticated_owner() {
    fn assert_traits<T: Read + Seek + Send + Sync>() {}
    assert_traits::<RangeSource>();
    let length = 2 * 1024 * 1024 * 1024 + 13;
    let peer = fixture(length, Behavior::Ranges);
    let mut source = open(&peer, Cancel::default());
    assert_eq!(source.byte_len(), Some(length));
    assert!(source.seekable());
    source.seek(SeekFrom::End(-13)).unwrap();
    let mut bytes = [0u8; 13];
    source.read_exact(&mut bytes).unwrap();
    assert_eq!(
        bytes.to_vec(),
        (length - 13..length)
            .map(|n| (n % 251) as u8)
            .collect::<Vec<_>>()
    );
    for block in 0..12 {
        source.seek(SeekFrom::Start(block * BLOCK)).unwrap();
        source.read_exact(&mut bytes).unwrap();
    }
    let State {
        mode: Mode::Ranges { cache, .. },
        ..
    } = &*source.state.lock().unwrap()
    else {
        panic!()
    };
    assert_eq!(cache.len(), CACHE_BLOCKS);
    assert!(cache.iter().map(|(_, b)| b.len()).sum::<usize>() <= CACHE_BLOCKS * BLOCK as usize);
    let requests = peer.requests.lock().unwrap();
    assert_eq!(requests.len(), 14);
    for (line, headers) in requests.iter() {
        let path = line.split_whitespace().nth(1).unwrap();
        let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
        assert_eq!(url.path(), "/owned/api/files/stream");
        let query: HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["profile"], "research");
        assert_eq!(query["session_id"], "session-7");
        assert_eq!(query["path"], "/sandbox/audio.wav?profile=other");
        assert_eq!(query["token"], "synthetic-token");
        assert_eq!(query.len(), 4);
        if line.starts_with("GET ") {
            assert_eq!(headers["if-range"], "\"fixture\"");
            assert!(headers["range"].starts_with("bytes="));
        }
    }
}
#[test]
fn missing_or_weak_validator_uses_one_sequential_representation_without_false_seek() {
    for behavior in [Behavior::NoValidator, Behavior::WeakValidator] {
        let peer = fixture(BLOCK + 31, behavior);
        let mut source = open(&peer, Cancel::default());
        assert!(!source.seekable());
        assert_eq!(source.seek(SeekFrom::Current(0)).unwrap(), 0);
        assert_eq!(
            source.seek(SeekFrom::Start(100)).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, BLOCK + 31);
        assert!(
            bytes
                .iter()
                .enumerate()
                .all(|(i, b)| *b == (i as u64 % 251) as u8)
        );
        let requests = peer.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(!requests[1].1.contains_key("range"));
        assert!(!requests[1].1.contains_key("if-range"));
    }
}
#[test]
fn initial_ignored_range_falls_back_to_one_full_sequential_response() {
    let peer = fixture(2 * BLOCK, Behavior::Ignored);
    let mut source = open(&peer, Cancel::default());
    assert!(!source.seekable());
    let mut bytes = Vec::new();
    source.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes.len() as u64, 2 * BLOCK);
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(i, b)| *b == (i as u64 % 251) as u8)
    );
    let requests = peer.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].1.contains_key("range"));
    assert!(!requests[2].1.contains_key("range"));
}

#[test]
fn ignored_wrong_changed_truncated_and_redirect_responses_expose_no_range_bytes_and_latch_failure()
{
    for behavior in [
        Behavior::WrongRange,
        Behavior::Changed,
        Behavior::Truncated,
        Behavior::Redirect,
    ] {
        let peer = fixture(4 * BLOCK, behavior);
        let mut source = open(&peer, Cancel::default());
        source.seek(SeekFrom::Start(BLOCK)).unwrap();
        let mut bytes = [77u8; 16];
        assert!(source.read(&mut bytes).is_err());
        assert_eq!(bytes, [77; 16]);
        assert!(source.read(&mut bytes).is_err());
        assert!(source.seek(SeekFrom::Start(0)).is_err());
        assert_eq!(
            peer.requests.lock().unwrap().len(),
            3,
            "failed source must not fetch again"
        );
    }
}
#[test]
fn cancellation_interrupts_a_stalled_range_and_a_retired_source_cannot_read_cached_bytes() {
    let peer = fixture(4 * BLOCK, Behavior::Stalled);
    let cancel = Cancel::default();
    let mut source = open(&peer, cancel.clone());
    source.seek(SeekFrom::Start(BLOCK)).unwrap();
    let requests = peer.requests.clone();
    let worker = std::thread::spawn(move || source.read(&mut [0u8; 1]));
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while requests.lock().unwrap().len() < 3 || !peer.stalled_headers.load(Ordering::Acquire) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let start = std::time::Instant::now();
    cancel.cancel();
    assert!(worker.join().unwrap().is_err());
    assert!(start.elapsed() < Duration::from_millis(300));
    let peer = fixture(BLOCK, Behavior::Ranges);
    let cancel = Cancel::default();
    let mut source = open(&peer, cancel.clone());
    source.read_exact(&mut [0u8; 4]).unwrap();
    cancel.cancel();
    assert_eq!(
        source.read(&mut [0u8; 4]).unwrap_err().kind(),
        io::ErrorKind::ConnectionAborted
    );
    assert!(source.seek(SeekFrom::Start(0)).is_err());
}

#[test]
fn sequential_truncation_never_reports_clean_eof_and_paused_retirement_is_cancellable() {
    let peer = fixture(BLOCK + 31, Behavior::SequentialTruncated);
    let mut source = open(&peer, Cancel::default());
    let mut bytes = Vec::new();
    assert!(source.read_to_end(&mut bytes).is_err());
    assert!(bytes.len() as u64 <= (BLOCK + 31) / 2);
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(i, b)| *b == (i as u64 % 251) as u8)
    );
    assert!(source.read(&mut [0; 1]).is_err());
    assert_eq!(peer.requests.lock().unwrap().len(), 2);
    let peer = fixture(2 * BLOCK, Behavior::NoValidator);
    let cancel = Cancel::default();
    let mut source = open(&peer, cancel.clone());
    std::thread::sleep(Duration::from_millis(40));
    cancel.cancel();
    assert_eq!(
        source.read(&mut [0; 1]).unwrap_err().kind(),
        io::ErrorKind::ConnectionAborted
    );
    drop(source);
}

#[cfg(target_os = "linux")]
#[test]
fn expired_native_http_ticket_is_renewed_once_without_bearer_or_owner_leakage() {
    use serde_json::{Value, json};
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    for remains_expired in [false, true] {
        let home =
            std::env::temp_dir().join(format!("hermes-range-native-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&home).unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = std::fs::canonicalize(home).unwrap();
        let control = UnixListener::bind(home.join("gateway.sock")).unwrap();
        std::fs::set_permissions(
            home.join("gateway.sock"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let control_home = home.clone();
        let private = std::thread::spawn(move || {
            let mut issued = Vec::new();
            for index in 0..5 {
                let (mut socket, _) = control.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(&mut socket).read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let result = if index == 0 {
                    assert_eq!(request["verb"], "identify");
                    json!({"pid":std::process::id(),"runtime_protocol":1,"state":"ready","authority_epoch":1,"instance_id":"range-owner","capabilities":["session-authority-v1"],"api_origin":origin,"served_profiles":[{"home":control_home,"profile_id":control_home}]})
                } else {
                    assert_eq!(request["verb"], "session-ticket");
                    assert_eq!(request["params"]["instance_id"], "range-owner");
                    assert_eq!(
                        request["params"]["profile_id"],
                        control_home.to_str().unwrap()
                    );
                    let purpose = request["params"]["purpose"].as_str().unwrap();
                    assert_eq!(
                        purpose,
                        if index == 1 {
                            "interactive"
                        } else {
                            "native-http"
                        }
                    );
                    let ticket = format!("range-ticket-{index}");
                    issued.push(ticket.clone());
                    json!({"profile_id":control_home,"instance_id":"range-owner","ticket":ticket})
                };
                writeln!(
                    socket,
                    "{}",
                    json!({"protocol":1,"id":request["id"],"ok":true,"result":result})
                )
                .unwrap();
            }
            issued
        });
        let public_home = home.clone();
        let public = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept_hdr(
                socket,
                |request: &tungstenite::handshake::server::Request,
                 mut response: tungstenite::handshake::server::Response| {
                    assert_eq!(
                        request.headers()["Sec-WebSocket-Protocol"],
                        "hermes-gateway-v1, hermes-gateway-ticket.range-ticket-1"
                    );
                    response.headers_mut().insert(
                        "Sec-WebSocket-Protocol",
                        "hermes-gateway-v1".parse().unwrap(),
                    );
                    Ok(response)
                },
            )
            .unwrap();
            let request: Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(request["method"], "profiles.list");
            ws.send(tungstenite::Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"result":{"profiles":[{"name":"default","path":public_home}]}}).to_string().into())).unwrap();
            let mut seen = Vec::new();
            for index in 0..3 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut socket);
                let mut header = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    header.push_str(&line);
                }
                assert!(!header.lines().next().unwrap().contains("ticket"));
                assert!(!header.to_lowercase().contains("authorization:"));
                assert!(header.to_lowercase().contains(&format!(
                    "x-hermes-gateway-ticket: range-ticket-{}",
                    index + 2
                )));
                seen.push(header);
                let response = if index == 0 {
                    "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nAccept-Ranges: bytes\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n"
                } else if index == 1 || remains_expired {
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope"
                } else {
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 0-3/4\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\nwave"
                };
                let _ = socket.write_all(response.as_bytes());
            }
            seen
        });
        let opened = RangeSource::open(
            &Connection::Local { home: home.clone() },
            "default",
            "/voice.wav",
            Some("owned-session".into()),
            Cancel::default(),
        );
        if remains_expired {
            assert!(opened.is_err());
        } else {
            let mut source = opened.unwrap();
            let mut bytes = [42u8; 4];
            source.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"wave");
        }
        assert_eq!(public.join().unwrap().len(), 3);
        assert_eq!(
            private.join().unwrap(),
            vec![
                "range-ticket-1",
                "range-ticket-2",
                "range-ticket-3",
                "range-ticket-4"
            ]
        );
        std::fs::remove_dir_all(home).unwrap();
    }
}
