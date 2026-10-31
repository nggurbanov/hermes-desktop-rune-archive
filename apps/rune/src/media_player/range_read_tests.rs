//! The 20-second WAV and first response reproduce the captured external-reader boundary.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

const WAV: &[u8] = include_bytes!("../../assets/test-audio/range-20s.wav");
const PATH: &str = "/tone.wav?sig=a%2Fb&mode=one%2Btwo";
const ETAG: &str = "\"43cf157dfb1c200728404187fabbc396bae2b8de715348f3959314617d557c2b\"";

#[derive(Debug)]
struct Request {
    method: String,
    path_ok: bool,
    range: Option<String>,
    if_range_ok: bool,
    other_if: bool,
    credential: bool,
}

struct Peer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Request>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
impl Peer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let (mut socket, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("peer accept: {e}"),
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(&mut socket);
                let mut first = String::new();
                if reader.read_line(&mut first).is_err() {
                    continue;
                }
                let mut words = first.split_whitespace();
                let method = words.next().unwrap_or("").to_string();
                let path_ok = words.next() == Some(PATH);
                let mut range = None;
                let mut if_range = None;
                let mut other_if = false;
                let mut credential = false;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        let name = name.trim().to_ascii_lowercase();
                        let value = value.trim();
                        match name.as_str() {
                            "range" => range = Some(value.to_string()),
                            "if-range" => if_range = Some(value.to_string()),
                            _ if name.starts_with("if-") => other_if = true,
                            _ => {}
                        }
                        // Check presence only; never retain header values.
                        credential |= matches!(
                            name.as_str(),
                            "authorization"
                                | "cookie"
                                | "x-hermes-gateway-ticket"
                                | "x-native-ticket"
                        ) || (name == "user-agent"
                            && value.contains("must-not-leak"));
                    }
                }
                let if_range_ok = if_range.as_deref() == Some(ETAG);
                recorded.lock().unwrap().push(Request {
                    method: method.clone(),
                    path_ok,
                    range: range.clone(),
                    if_range_ok,
                    other_if,
                    credential,
                });
                let common = format!(
                    "Accept-Ranges: bytes\r\nETag: {ETAG}\r\nContent-Type: audio/wav\r\nConnection: close\r\n"
                );
                if !path_ok || other_if || credential {
                    let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                } else if method == "HEAD" && range.is_none() && if_range.is_none() {
                    let _ = write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{common}\r\n",
                        WAV.len()
                    );
                } else if method == "GET" && if_range_ok {
                    let bounds = range
                        .as_deref()
                        .and_then(|r| r.strip_prefix("bytes="))
                        .and_then(|r| r.split_once('-'))
                        .and_then(|(a, b)| {
                            Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                        });
                    match bounds {
                        Some((a, b)) if a < WAV.len() && b >= a => {
                            let b = b.min(WAV.len() - 1);
                            let _ = write!(
                                socket,
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\n{common}\r\n",
                                b - a + 1,
                                WAV.len()
                            );
                            let _ = socket.write_all(&WAV[a..=b]);
                        }
                        Some((a, _)) if a >= WAV.len() => {
                            let _ = write!(
                                socket,
                                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\n{common}\r\n",
                                WAV.len()
                            );
                        }
                        _ => {
                            let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                } else {
                    let _ = socket.write_all(b"HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        });
        Self {
            address,
            stop,
            requests,
            thread: Some(thread),
        }
    }
}

#[test]
fn external_range_wav_worker_decodes_to_end_with_original_query_and_no_credentials() {
    use sha2::{Digest, Sha256};
    assert_eq!(WAV.len(), 3_840_044);
    assert_eq!(
        format!("{:x}", Sha256::digest(WAV)),
        "43cf157dfb1c200728404187fabbc396bae2b8de715348f3959314617d557c2b"
    );
    let peer = Peer::new();
    let mut players = MediaPlayers::new();
    players.use_null_output();
    players.init(Arc::new(|| {}));
    let mut target = super::tests::target(vec![]);
    target.source = crate::preview::Source::RemoteUrl(format!("http://{}{PATH}", peer.address));
    let connection = Connection::Url {
        endpoint: "http://127.0.0.1:1".into(),
        token: Some("must-not-leak".into()),
    };
    let id = players.open(connection, target).unwrap();
    let shared = players.players[&id].shared.clone();
    let start = Instant::now();
    let mut played = false;
    loop {
        players.poll();
        let snapshot = players.get(id).unwrap();
        assert!(
            snapshot.error.is_none(),
            "player error: {:?}; next_packet detail: {:?}",
            snapshot.error,
            shared.packet_error.lock().unwrap()
        );
        if snapshot.state == State::Paused && !played {
            assert_eq!(snapshot.duration, Some(20.));
            players.act(id, Action::Play);
            played = true;
        } else if snapshot.state == State::Ended {
            assert!(played);
            assert_eq!(snapshot.position, 20.);
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "worker timed out; packet detail: {:?}",
            shared.packet_error.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    players.close(id);
    let requests = peer.requests.lock().unwrap();
    assert!(requests.len() >= 2, "{requests:?}");
    assert_eq!(requests[0].method, "HEAD");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].range.as_deref(), Some("bytes=0-262143"));
    assert!(
        requests.iter().all(|r| r.path_ok
            && !r.credential
            && !r.other_if
            && (r.method != "GET" || r.if_range_ok)),
        "{requests:?}"
    );
}
