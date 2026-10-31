//! Loopback proof of the consumer-to-retained-runtime WebSocket boundary.
use super::prepared::{CloudHttp, CloudRuntime, WirePolicy};
use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tungstenite::client::IntoClientRequest;

struct Tickets {
    next: AtomicUsize,
    targets: Mutex<Vec<String>>,
}
impl CloudRuntime for Tickets {
    fn prepare_http(&self, _: reqwest::Method, _: &Url, _: Instant) -> Result<CloudHttp, String> {
        Err("not used".into())
    }
    fn retry_http_unauthorized(&self, _: CloudHttp, _: Instant) -> Result<CloudHttp, String> {
        Err("not used".into())
    }
    fn prepare_ws(
        &self,
        url: &Url,
        _: Instant,
    ) -> Result<tungstenite::handshake::client::Request, String> {
        self.targets.lock().unwrap().push(url.as_str().to_owned());
        let mut url = url.clone();
        url.query_pairs_mut().append_pair(
            "ticket",
            &self.next.fetch_add(1, Ordering::SeqCst).to_string(),
        );
        url.as_str()
            .into_client_request()
            .map_err(|_| "invalid fixture URL".into())
    }
    fn runtime_fingerprint(&self) -> String {
        "loopback-runtime".into()
    }
}

#[test]
fn cloud_consumers_mint_fresh_tickets_for_final_routed_urls() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/prefix/", listener.local_addr().unwrap());
    let (done, finish) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let mut seen = std::collections::HashSet::new();
        let mut paths = Vec::new();
        let mut sockets = Vec::new();
        for _ in 0..7 {
            let (stream, _) = listener.accept().unwrap();
            let socket = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request,
                 response: tungstenite::handshake::server::Response| {
                    let uri = request.uri().to_string();
                    let parsed = Url::parse(&format!("ws://localhost{uri}")).unwrap();
                    let ticket = parsed
                        .query_pairs()
                        .find(|(k, _)| k == "ticket")
                        .unwrap()
                        .1
                        .into_owned();
                    assert!(seen.insert(ticket), "ticket replayed");
                    paths.push(uri);
                    Ok(response)
                },
            )
            .unwrap();
            sockets.push(socket);
        }
        let _ = finish.recv_timeout(Duration::from_secs(5));
        paths
    });
    let runtime = Arc::new(Tickets {
        next: AtomicUsize::new(0),
        targets: Mutex::new(Vec::new()),
    });
    let policy = WirePolicy::cloud(route::ProfilePolicy::identity(), runtime.clone()).unwrap();
    let connection = Connection::Routed {
        actual: Box::new(Connection::Url {
            endpoint,
            token: None,
        }),
        owner: Arc::new(crate::source::RouteOwner::legacy("ws-fixture").unwrap()),
        policy: Arc::new(policy),
    };
    drop(Gateway::connect(&connection).unwrap());
    drop(Gateway::connect(&connection.clone()).unwrap());
    drop(
        terminal::HostTerminal::connect(
            &connection,
            "work",
            "/tmp/a b",
            80,
            24,
            Some("shell"),
            false,
        )
        .unwrap(),
    );
    let mut audio = audio::AudioGateway::connect(&connection, "work").unwrap();
    drop(audio.stream(audio::AudioStream::Speak, 16000).unwrap());
    drop(audio.stream(audio::AudioStream::Transcribe, 16000).unwrap());
    done.send(()).unwrap();
    let paths = server.join().unwrap();
    assert_eq!(paths.len(), 7);
    assert!(
        paths
            .iter()
            .any(|p| p.contains("/prefix/api/host-terminal?")
                && p.contains("profile=work")
                && p.contains("attach=shell")
                && p.contains("cwd=%2Ftmp%2Fa+b"))
    );
    assert!(
        paths
            .iter()
            .any(|p| p.contains("/prefix/api/audio/speak-stream?profile=work"))
    );
    assert!(
        paths
            .iter()
            .any(|p| p.contains("/prefix/api/audio/transcribe-stream?profile=work"))
    );
    let targets = runtime.targets.lock().unwrap();
    assert_eq!(targets.len(), 7);
    assert!(targets.iter().all(|u| !u.contains("ticket=")));
}
