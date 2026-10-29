use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::atomic::AtomicBool,
};
#[test]
fn external_audio_metadata_and_original_save_use_no_gateway_credentials() {
    let bytes = Arc::new(wav(2, 1.));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let output = bytes.clone();
    let requests = Arc::new(AtomicU64::new(0));
    let recorded = requests.clone();
    let server = std::thread::spawn(move || {
        while !stopped.load(Ordering::Acquire) {
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            assert!(
                request.starts_with("HEAD /public.wav ") || request.starts_with("GET /public.wav ")
            );
            let mut range = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                let line = line.to_ascii_lowercase();
                assert!(
                    !line.starts_with("authorization:")
                        && !line.starts_with("cookie:")
                        && !line.starts_with("x-hermes-gateway-ticket:")
                );
                if let Some(value) = line.strip_prefix("range: ") {
                    range = Some(value.trim().to_string());
                }
            }
            recorded.fetch_add(1, Ordering::AcqRel);
            let len = output.len();
            if request.starts_with("HEAD ") {
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nETag: \"public-tone\"\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
            } else {
                let (a, b) = range
                    .unwrap()
                    .strip_prefix("bytes=")
                    .unwrap()
                    .split_once('-')
                    .map(|(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()))
                    .unwrap();
                stream.write_all(format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{len}\r\nETag: \"public-tone\"\r\nConnection: close\r\n\r\n",b-a+1).as_bytes()).unwrap();
                stream.write_all(&output[a..=b]).unwrap();
            }
        }
    });
    let mut players = MediaPlayers::new();
    players.init(Arc::new(|| {}));
    let mut t = target(vec![]);
    t.source = crate::preview::Source::RemoteUrl(format!("http://{address}/public.wav"));
    let connection = Connection::Url {
        endpoint: "http://127.0.0.1:1/gateway".into(),
        token: Some("must-not-leak".into()),
    };
    let id = players.open(connection, t).unwrap();
    wait(&mut players, id, |s| s.state == State::Paused);
    assert_eq!(players.get(id).unwrap().duration, Some(1.));
    let directory =
        std::env::temp_dir().join(format!("hermes-external-audio-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let destination = directory.join("saved.wav");
    players.act(id, Action::SaveOriginal(destination.clone()));
    let start = Instant::now();
    loop {
        players.poll();
        let s = players.get(id).unwrap();
        assert!(s.save_error.is_none(), "{:?}", s.save_error);
        if s.notice.as_deref() == Some("Original saved") {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read(&destination).unwrap(), *bytes);
    assert!(
        !players.players[&id].shared.cancel.cancelled(),
        "Saving must not retire the player"
    );
    assert_eq!(requests.load(Ordering::Acquire), 4);
    players.close(id);
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
