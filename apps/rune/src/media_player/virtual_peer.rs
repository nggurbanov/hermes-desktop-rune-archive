//! Actual gateway audio IO in the existing opt-in private output harness.
use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::atomic::AtomicBool,
    thread::JoinHandle,
};
struct Peer {
    connection: Connection,
    stop: Arc<AtomicBool>,
    stalled: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
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
fn peer(bytes: Vec<u8>) -> Peer {
    let bytes = Arc::new(bytes);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let stalled = Arc::new(AtomicBool::new(false));
    let began = stalled.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let ended = cancelled.clone();
    let requests = Arc::new(Mutex::new(vec![]));
    let recorded = requests.clone();
    let join = std::thread::spawn(move || {
        let mut children = vec![];
        while !stopped.load(Ordering::Acquire) {
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            let bytes = bytes.clone();
            let stopped = stopped.clone();
            let began = began.clone();
            let ended = ended.clone();
            let recorded = recorded.clone();
            children.push(std::thread::spawn(move||{
 stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();let start=Instant::now();let mut peek=[0u8;8192];loop{match stream.peek(&mut peek){Ok(n) if peek[..n].windows(4).any(|w|w==b"\r\n\r\n")=>{let headers=String::from_utf8_lossy(&peek[..n]);if headers.to_ascii_lowercase().contains("upgrade: websocket"){let mut socket=tungstenite::accept(stream).unwrap();while !stopped.load(Ordering::Acquire){if let Err(e)=socket.read(){match e{tungstenite::Error::Io(e) if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>{},_=>break,}}}return;}break;},Ok(0)=>return,Ok(_)=>{},Err(e) if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>{},Err(_)=>return}if stopped.load(Ordering::Acquire)||start.elapsed()>Duration::from_secs(3){return;}std::thread::sleep(Duration::from_millis(1));}
 let mut reader=BufReader::new(&mut stream);let mut first=String::new();reader.read_line(&mut first).unwrap();let mut range=None;loop{let mut line=String::new();reader.read_line(&mut line).unwrap();if line=="\r\n"{break;}if let Some(value)=line.to_ascii_lowercase().strip_prefix("range: "){range=Some(value.trim().to_string());}}
 assert!(first.contains("profile=sample"));assert!(first.contains("session_id=owned"));assert!(first.contains("token=synthetic-only"));recorded.lock().unwrap().push(first.clone());
 let length=bytes.len();let etag="ETag: \"owned-wav\"\r\n";
 if first.starts_with("HEAD "){let _=stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\n{etag}Connection: close\r\n\r\n").as_bytes());return;}
 let range=range.unwrap();let (a,b)=range.strip_prefix("bytes=").unwrap().split_once('-').unwrap();let a:usize=a.parse().unwrap();let b:usize=b.parse().unwrap();assert!(b<length&&b-a<262144);
 if stream.write_all(format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{length}\r\n{etag}Connection: close\r\n\r\n",b-a+1).as_bytes()).is_err(){return;}
 if a==262144&&!began.swap(true,Ordering::AcqRel){let mut byte=[0];while !stopped.load(Ordering::Acquire){match stream.read(&mut byte){Ok(0)=>{ended.store(true,Ordering::Release);break;},Ok(_)=>{},Err(e) if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>{},Err(_)=>{ended.store(true,Ordering::Release);break;}}}return;}
 let _=stream.write_all(&bytes[a..=b]);
 }));
        }
        for child in children {
            child.join().unwrap();
        }
    });
    Peer {
        connection: Connection::Url {
            endpoint: format!("http://{address}"),
            token: Some("synthetic-only".into()),
        },
        stop,
        stalled,
        cancelled,
        requests,
        join: Some(join),
    }
}
pub(super) fn seek_cancels_stalled_http_body() {
    let peer = peer(wav(1, 10.));
    let mut players = MediaPlayers::new();
    players.init(Arc::new(|| {}));
    let mut t = target(vec![]);
    t.source = crate::preview::Source::AgentPath("/owned/tone.wav".into());
    let id = players.open(peer.connection.clone(), t).unwrap();
    wait(&mut players, id, |s| s.state == State::Paused);
    players.act(id, Action::Rate(4.));
    players.act(id, Action::Play);
    let start = Instant::now();
    while !peer.stalled.load(Ordering::Acquire) {
        players.poll();
        assert!(players.get(id).unwrap().error.is_none());
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "No real later-range stall"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let source = players.players[&id]
        .shared
        .source_cancel
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    let before = Instant::now();
    players.act(id, Action::Seek(9.));
    assert!(before.elapsed() < Duration::from_millis(100));
    assert!(source.cancelled());
    assert!(players.players[&id].shared.ring.lock().unwrap().queued == 0);
    wait(&mut players, id, |s| {
        s.position >= 9. && s.state != State::Loading
    });
    wait(&mut players, id, |s| s.state == State::Ended);
    assert_eq!(players.get(id).unwrap().position, 10.);
    let start = Instant::now();
    while !peer.cancelled.load(Ordering::Acquire) {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "The cancelled HTTP body stayed open"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        peer.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.starts_with("HEAD "))
            .count()
            >= 2
    );
    players.close(id);
}
