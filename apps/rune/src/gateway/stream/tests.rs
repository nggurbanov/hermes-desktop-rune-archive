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
            "HTTP/1.1 200 OK\r\nContent-Length: 1234\r\nContent-Type: video/webm\r\nAccept-Ranges: bytes\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n"
        } else {
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/12\r\nContent-Type: video/webm\r\nAccept-Ranges: bytes\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n4567"
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
