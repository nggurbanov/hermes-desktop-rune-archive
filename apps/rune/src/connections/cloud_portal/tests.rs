use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};
use tao::{
    event_loop::EventLoopBuilder, platform::unix::EventLoopBuilderExtUnix, window::WindowBuilder,
};

// Requires a private display and explicit ignored-test invocation. No real portal,
// profile, browser or credential store is touched.
#[test]
#[ignore = "private native WebView fixture requires an exclusive display"]
fn native_cookie_roundtrip_isolation_and_retirement() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}/fixture", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let logged = Arc::new(Mutex::new(Vec::<String>::new()));
    let counter = Arc::new(AtomicUsize::new(0));
    let discovery_delay = Arc::new(AtomicBool::new(false));
    let discovery_started = Arc::new(AtomicBool::new(false));
    let delay = discovery_delay.clone();
    let started = discovery_started.clone();
    let (st, lg, count) = (stop.clone(), logged.clone(), counter.clone());
    let worker = thread::spawn(move || {
        while !st.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut request = [0; 8192];
            let n = stream.read(&mut request).unwrap_or(0);
            let request = String::from_utf8_lossy(&request[..n]).to_string();
            if request.starts_with("GET /fixture/api/agents") {
                lg.lock().unwrap().push(request);
                started.store(true, Ordering::SeqCst);
                if delay.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(500));
                }
                let body = "{}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            } else if request.starts_with("GET /fixture/") {
                let value = count.fetch_add(1, Ordering::SeqCst) + 1;
                let body = "<html><body>private fixture</body></html>";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nSet-Cookie: nas-session=token{value}; Path=/fixture; HttpOnly\r\nSet-Cookie: nas-refresh=refresh{value}; Path=/fixture; HttpOnly\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            }
        }
    });
    let event_loop = EventLoopBuilder::<()>::with_user_event()
        .with_any_thread(true)
        .build();
    let first_window = WindowBuilder::new()
        .with_visible(false)
        .build(&event_loop)
        .unwrap();
    let second_window = WindowBuilder::new()
        .with_visible(false)
        .build(&event_loop)
        .unwrap();
    let mut first =
        PortalView::private_fixture(&first_window, Endpoint::new(&base, &[]).unwrap(), &base)
            .unwrap();
    let mut second =
        PortalView::private_fixture(&second_window, Endpoint::new(&base, &[]).unwrap(), &base)
            .unwrap();
    fn wait(view: &mut PortalView, receipt: Receipt) -> Result<Progress, Error> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            match view.poll(receipt, Instant::now())? {
                Progress::Pending => {}
                done => return Ok(done),
            }
            assert!(Instant::now() < deadline, "operation never completed");
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn finish(view: &mut PortalView, operation: Operation) {
        let receipt = view.begin(operation, Instant::now()).unwrap();
        assert!(matches!(wait(view, receipt).unwrap(), Progress::Complete));
        assert!(matches!(
            view.poll(receipt, Instant::now()).unwrap(),
            Progress::Cancelled
        ));
    }
    fn discover(view: &mut PortalView) {
        let receipt = view.begin(Operation::Discover, Instant::now()).unwrap();
        assert!(matches!(
            wait(view, receipt).unwrap(),
            Progress::Discovery(_)
        ));
    }
    // A pending GTK callback must not require a nested loop for cancellation.
    let old = first.begin(Operation::Login, Instant::now()).unwrap();
    let started = Instant::now();
    first.cancel();
    assert!(started.elapsed() < Duration::from_millis(100));
    let current = first.begin(Operation::Login, Instant::now()).unwrap();
    // Inject a late successful callback from the prior generation before GTK runs.
    assert!(
        first
            .tx
            .send(Reply::Cookies(
                old,
                Ok(vec![("nas-session".into(), "stale".into())])
            ))
            .is_ok()
    );
    assert!(matches!(
        first.poll(current, Instant::now()).unwrap(),
        Progress::Pending
    ));
    assert!(matches!(
        wait(&mut first, current).unwrap(),
        Progress::Complete
    ));
    finish(&mut second, Operation::Login);
    discover(&mut first);
    discover(&mut second);
    let before = counter.load(Ordering::SeqCst);
    let receipt = first
        .begin(Operation::Renew { force: false }, Instant::now())
        .unwrap();
    assert!(matches!(
        wait(&mut first, receipt).unwrap(),
        Progress::Complete
    ));
    assert_eq!(counter.load(Ordering::SeqCst), before);
    finish(&mut first, Operation::Renew { force: true });
    discover(&mut first);
    let requests = logged.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .enumerate()
            .all(|(i, request)| request.contains(&format!("nas-session=token{}", i + 1)))
    );
    drop(requests);
    let now = Instant::now();
    let receipt = second.begin(Operation::Renew { force: true }, now).unwrap();
    assert!(matches!(
        second.poll(receipt, now + Duration::from_secs(13)).unwrap(),
        Progress::TimedOut
    ));
    discover(&mut second);
    finish(&mut second, Operation::Renew { force: true });
    discover(&mut second);
    // Discovery is performed off the GTK thread; retirement does not wait for it.
    discovery_delay.store(true, Ordering::SeqCst);
    discovery_started.store(false, Ordering::SeqCst);
    let delayed = first.begin(Operation::Discover, Instant::now()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !discovery_started.load(Ordering::SeqCst) {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        assert!(matches!(
            first.poll(delayed, Instant::now()).unwrap(),
            Progress::Pending
        ));
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let start = Instant::now();
    first.cancel();
    assert!(start.elapsed() < Duration::from_millis(100));
    let replacement = first.begin(Operation::Discover, Instant::now()).unwrap();
    assert!(matches!(
        wait(&mut first, replacement).unwrap(),
        Progress::Discovery(_)
    ));
    assert!(matches!(
        first.poll(delayed, Instant::now()).unwrap(),
        Progress::Cancelled
    ));
    discovery_delay.store(false, Ordering::SeqCst);
    let old = first.begin(Operation::Discover, Instant::now()).unwrap();
    let started = Instant::now();
    first.retire();
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(
        first
            .tx
            .send(Reply::Cookies(
                old,
                Ok(vec![("nas-session".into(), "late".into())])
            ))
            .is_ok()
    );
    assert!(matches!(
        first.poll(old, Instant::now()).unwrap(),
        Progress::Cancelled
    ));
    assert!(matches!(
        first.begin(Operation::Discover, Instant::now()),
        Err(Error::Cancelled)
    ));
    second.retire();
    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
}
