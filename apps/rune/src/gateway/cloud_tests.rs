use super::prepared::{CloudHttp, CloudRuntime, WirePolicy};
use super::*;
use reqwest::{
    Method,
    header::{HeaderMap, HeaderValue},
};
use std::{
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};
use tungstenite::client::IntoClientRequest;

struct Fixture {
    retries: AtomicUsize,
}
impl CloudRuntime for Fixture {
    fn prepare_http(&self, method: Method, url: &Url, _: Instant) -> Result<CloudHttp, String> {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("old-secret"));
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
        assert_eq!(*previous.receipt.downcast::<Method>().unwrap(), Method::GET);
        self.retries.fetch_add(1, Ordering::SeqCst);
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("new-secret"));
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
        "fixture-identity".into()
    }
}
#[test]
fn cloud_json_get_refreshes_once_post_is_not_replayed_and_redirect_is_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/prefix/", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ws = tungstenite::accept(stream).unwrap();
        for (status, expected) in [
            ("401 Unauthorized", "old-secret"),
            ("200 OK", "new-secret"),
            ("401 Unauthorized", "old-secret"),
            ("302 Found", "old-secret"),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let headers = tests::http_request(&mut stream);
            assert!(headers.contains(expected));
            let body = "{}";
            write!(stream, "HTTP/1.1 {status}\r\nLocation: http://127.0.0.1:1/escape\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let runtime = Arc::new(Fixture {
        retries: AtomicUsize::new(0),
    });
    let policy = WirePolicy::cloud(route::ProfilePolicy::identity(), runtime.clone()).unwrap();
    let cloned = policy
        .clone()
        .with_profiles(route::ProfilePolicy::identity());
    assert_eq!(policy.runtime_fingerprint(), cloned.runtime_fingerprint());
    assert!(!format!("{policy:?}").contains("secret"));
    let connection = Connection::Routed {
        actual: Box::new(Connection::Url {
            endpoint,
            token: None,
        }),
        owner: Arc::new(crate::source::RouteOwner::legacy("cloud-fixture").unwrap()),
        policy: Arc::new(policy),
    };
    let gateway = Gateway::connect(&connection).unwrap();
    let url = gateway.base.join("api/status?profile=work").unwrap();
    let deadline = Instant::now() + TIMEOUT;
    assert!(
        gateway
            .get_json("work", url.clone(), deadline, "fixture")
            .is_ok()
    );
    let post = gateway
        .prepared
        .prepare_http_method(Method::POST, url.clone(), None, deadline)
        .unwrap();
    assert!(
        gateway
            .get_json_parts(post, deadline, "fixture")
            .unwrap_err()
            .contains("401")
    );
    let redirect = gateway
        .prepared
        .prepare_http_method(Method::GET, url, None, deadline)
        .unwrap();
    assert!(
        gateway
            .get_json_parts(redirect, deadline, "fixture")
            .unwrap_err()
            .contains("302")
    );
    assert_eq!(runtime.retries.load(Ordering::SeqCst), 1);
    assert!(
        gateway
            .prepared
            .prepare_http_method(
                Method::GET,
                Url::parse("http://127.0.0.1:1/escape").unwrap(),
                None,
                deadline
            )
            .is_err()
    );
    server.join().unwrap();
}
