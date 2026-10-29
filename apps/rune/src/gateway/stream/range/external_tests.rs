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
    Ignored,
    NoValidator,
    Changed,
    Redirect(u16),
    Stalled,
}
struct Peer {
    url: String,
    requests: Arc<Mutex<Vec<(String, HashMap<String, String>)>>>,
    stop: Arc<AtomicBool>,
    stalled: Arc<AtomicBool>,
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
fn external_peer(length: u64, behavior: Behavior) -> Peer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let stalled = Arc::new(AtomicBool::new(false));
    let worker_stalled = stalled.clone();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let worker_requests = requests.clone();
    let join = std::thread::spawn(move || {
        while !worker_stop.load(Ordering::Acquire) {
            let (mut socket, _) = match listener.accept() {
                Ok(socket) => socket,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
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
            worker_requests
                .lock()
                .unwrap()
                .push((first.clone(), headers.clone()));
            if let Behavior::Redirect(port) = behavior {
                socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
                continue;
            }
            let etag = if matches!(behavior, Behavior::NoValidator) {
                ""
            } else {
                "ETag: \"external\"\r\n"
            };
            if first.starts_with("HEAD ") {
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\n{etag}Set-Cookie: credential=must-not-replay\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
                continue;
            }
            let range = headers
                .get("range")
                .and_then(|value| value.strip_prefix("bytes="))
                .map(|value| {
                    let (a, b) = value.split_once('-').unwrap();
                    (a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap())
                });
            let (start, end) = range.unwrap_or((0, length - 1));
            if matches!(behavior, Behavior::Stalled) && start != 0 {
                socket.write_all(format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{length}\r\n{etag}Connection: close\r\n\r\n",end-start+1).as_bytes()).unwrap();
                worker_stalled.store(true, Ordering::Release);
                while !worker_stop.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                continue;
            }
            if range.is_some() && matches!(behavior, Behavior::Ignored) {
                let _=socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n{etag}Connection: close\r\n\r\n").as_bytes());
                continue;
            }
            let etag = if matches!(behavior, Behavior::Changed) && start != 0 {
                "ETag: \"replacement\"\r\n"
            } else {
                etag
            };
            let body: Vec<u8> = (start..=end).map(|n| (n % 251) as u8).collect();
            let response = if range.is_some() {
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{length}\r\n{etag}Connection: close\r\n\r\n",
                    body.len()
                )
            } else {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{etag}Connection: close\r\n\r\n",
                    body.len()
                )
            };
            if socket.write_all(response.as_bytes()).is_ok() {
                let _ = socket.write_all(&body);
            }
        }
    });
    Peer {
        url: format!(
            "http://{address}/clips/my%20voice.wav?source=original&token=user-supplied&x=1&x=2"
        ),
        requests,
        stop,
        stalled,
        join: Some(join),
    }
}
#[test]
fn external_sparse_ranges_keep_exact_url_and_never_inherit_gateway_or_cookie_credentials() {
    let length = 2 * 1024 * 1024 * 1024 + 17;
    let peer = external_peer(length, Behavior::Ranges);
    let mut source = RangeSource::open_external(&peer.url, Cancel::default()).unwrap();
    assert!(source.seekable());
    assert_eq!(source.byte_len(), Some(length));
    source.seek(SeekFrom::End(-17)).unwrap();
    let mut bytes = [0; 17];
    source.read_exact(&mut bytes).unwrap();
    assert_eq!(
        bytes.to_vec(),
        (length - 17..length)
            .map(|n| (n % 251) as u8)
            .collect::<Vec<_>>()
    );
    for block in 1..12 {
        source.seek(SeekFrom::Start(block * BLOCK)).unwrap();
        source.read_exact(&mut bytes).unwrap();
    }
    let state = source.state.lock().unwrap();
    let Mode::Ranges { cache, .. } = &state.mode else {
        panic!()
    };
    assert_eq!(cache.len(), CACHE_BLOCKS);
    assert!(
        cache.iter().map(|(_, bytes)| bytes.len()).sum::<usize>() <= CACHE_BLOCKS * BLOCK as usize
    );
    let requests = peer.requests.lock().unwrap();
    assert_eq!(requests.len(), 14);
    for (line, headers) in requests.iter() {
        assert_eq!(
            line.split_whitespace().nth(1).unwrap(),
            "/clips/my%20voice.wav?source=original&token=user-supplied&x=1&x=2"
        );
        assert_eq!(headers["accept-encoding"], "identity");
        for forbidden in [
            "authorization",
            "proxy-authorization",
            "x-hermes-gateway-ticket",
            "cookie",
            "referer",
        ] {
            assert!(!headers.contains_key(forbidden), "{forbidden}");
        }
    }
}
#[test]
fn external_unknown_validator_and_ignored_ranges_use_single_sequential_representation() {
    for behavior in [Behavior::NoValidator, Behavior::Ignored] {
        let peer = external_peer(BLOCK + 19, behavior);
        let mut source = RangeSource::open_external(&peer.url, Cancel::default()).unwrap();
        assert!(!source.seekable());
        assert!(source.seek(SeekFrom::Start(1)).is_err());
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, BLOCK + 19);
        assert!(
            bytes
                .iter()
                .enumerate()
                .all(|(i, b)| *b == (i as u64 % 251) as u8)
        );
        let requests = peer.requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            if matches!(behavior, Behavior::Ignored) {
                3
            } else {
                2
            }
        );
        assert!(!requests.last().unwrap().1.contains_key("range"));
    }
}
#[test]
fn external_changed_etag_redirect_and_url_credentials_are_rejected() {
    let peer = external_peer(4 * BLOCK, Behavior::Changed);
    let mut source = RangeSource::open_external(&peer.url, Cancel::default()).unwrap();
    source.seek(SeekFrom::Start(BLOCK)).unwrap();
    let mut bytes = [77; 16];
    assert!(source.read(&mut bytes).is_err());
    assert_eq!(bytes, [77; 16]);
    assert!(source.seek(SeekFrom::Start(0)).is_err());
    assert_eq!(peer.requests.lock().unwrap().len(), 3);
    let trap_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    trap_listener.set_nonblocking(true).unwrap();
    let trap_port = trap_listener.local_addr().unwrap().port();
    let trap_stop = Arc::new(AtomicBool::new(false));
    let trap_seen = Arc::new(AtomicBool::new(false));
    let stop = trap_stop.clone();
    let seen = trap_seen.clone();
    let trap = std::thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            match trap_listener.accept() {
                Ok((mut socket, _)) => {
                    seen.store(true, Ordering::Release);
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut socket);
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                    }
                    let _ = socket.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("{error}"),
            }
        }
    });
    let redirect = external_peer(4 * BLOCK, Behavior::Redirect(trap_port));
    assert!(RangeSource::open_external(&redirect.url, Cancel::default()).is_err());
    assert_eq!(redirect.requests.lock().unwrap().len(), 1);
    trap_stop.store(true, Ordering::Release);
    trap.join().unwrap();
    assert!(
        !trap_seen.load(Ordering::Acquire),
        "Redirect target received a request"
    );
    for url in [
        "ftp://127.0.0.1/a",
        "file:///etc/passwd",
        "https://user:secret@example.invalid/audio",
        "https://user@example.invalid/audio",
        "data:audio/wav;base64,x",
    ] {
        assert!(RangeSource::open_external(url, Cancel::default()).is_err());
    }
}
#[test]
fn external_stalled_and_cached_cancellation_never_retries_interrupted_forever() {
    let peer = external_peer(4 * BLOCK, Behavior::Stalled);
    let cancel = Cancel::default();
    let mut source = RangeSource::open_external(&peer.url, cancel.clone()).unwrap();
    source.seek(SeekFrom::Start(BLOCK)).unwrap();
    let worker = std::thread::spawn(move || source.read_exact(&mut [0; 16]));
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !peer.stalled.load(Ordering::Acquire) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let start = std::time::Instant::now();
    cancel.cancel();
    assert!(worker.join().unwrap().is_err());
    assert!(start.elapsed() < Duration::from_millis(300));
    let cached = external_peer(BLOCK, Behavior::Ranges);
    let cancel = Cancel::default();
    let mut source = RangeSource::open_external(&cached.url, cancel.clone()).unwrap();
    cancel.cancel();
    let mut bytes = Vec::new();
    assert_eq!(
        source.read_to_end(&mut bytes).unwrap_err().kind(),
        io::ErrorKind::ConnectionAborted
    );
    assert_eq!(
        source.read_exact(&mut [0; 1]).unwrap_err().kind(),
        io::ErrorKind::ConnectionAborted
    );
}
