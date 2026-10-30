//! Two real worker sockets contend for one durable input, never two admissions.
use super::contract_tests::{answer, history, request, snapshot, worker};
use super::*;
use std::{
    io::ErrorKind,
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

#[test]
fn two_workers_share_one_claim_and_only_the_owner_admits_the_saved_input() {
    let root = std::env::temp_dir().join(format!("hermes-two-workers-{}", uuid::Uuid::new_v4()));
    let journal = Journal::new(root.clone()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let connection = Connection::Url {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        token: Some("synthetic-token".into()),
    };
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("stored".into()),
    };
    let entry = Prepared::new(
        &connection,
        &scope,
        SendInput {
            text: "exactly once".into(),
            ..Default::default()
        },
    )
    .unwrap();
    journal.save(&entry).unwrap();
    let id = entry.submission_id.clone();
    let admissions = Arc::new(AtomicUsize::new(0));
    let counted = admissions.clone();
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut first = tungstenite::accept(stream).unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let mut second = tungstenite::accept(stream).unwrap();
        let resume = request(&mut first, "session.resume");
        answer(&mut first, &resume, snapshot(false, 0));
        history(&listener, serde_json::json!([]));
        let submit = request(&mut first, "prompt.submit");
        assert_eq!(submit["params"]["text"], "exactly once");
        counted.fetch_add(1, Ordering::SeqCst);
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(4)).unwrap();
        match second.read() {
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            other => panic!("contending worker opened an RPC: {other:?}"),
        }
        answer(
            &mut first,
            &submit,
            serde_json::json!({"admission_id":"one-admission","ref":{"profile_id":"fixture-profile","session_id":"live"},"status":"queued","sequence":1,"execution_generation":1,"authority_epoch":7}),
        );
    });
    let (mut first, _) = worker(connection.clone(), scope.clone());
    let (mut second, _) = worker(connection, scope);
    let first_entry = entry.clone();
    let first_journal = journal.clone();
    let owner = thread::spawn(move || first.submit(first_entry, &first_journal));
    entered.recv_timeout(Duration::from_secs(4)).unwrap();
    let began = Instant::now();
    let error = second.submit(entry.clone(), &journal).unwrap_err();
    assert!(error.message.contains("Another Hermes window"));
    assert!(began.elapsed() < Duration::from_secs(1));
    assert!(journal.save(&entry).is_err());
    assert!(journal.retire(&entry).is_err());
    release.send(()).unwrap();
    owner.join().unwrap().unwrap();
    server.join().unwrap();
    assert_eq!(admissions.load(Ordering::SeqCst), 1);
    assert!(journal.load(&id).is_err());
    assert!(second.submit(entry, &journal).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
