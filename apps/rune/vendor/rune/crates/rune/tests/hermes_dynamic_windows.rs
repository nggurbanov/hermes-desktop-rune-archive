//! Public contracts against the production library; positive hosts use the native example.
use rune::{AppSpec, Env, Runtime, Tx, WindowCreation, WindowSpawnStatus, WindowSpec};
use std::rc::Rc;
fn spec(key: &str, captured: Rc<String>) -> WindowSpec<Rc<String>> {
    WindowSpec::new(
        key,
        AppSpec::new(
            "local-state",
            (200., 200.),
            || Rc::new(String::new()),
            |_, _| rune::el(),
        ),
    )
    .state(captured)
}
fn failed_offscreen(creation: &WindowCreation) {
    assert!(
        matches!(creation.status(), WindowSpawnStatus::Failed(ref reason) if reason.contains("offscreen")),
        "{:?}",
        creation.status()
    );
}
#[test]
fn runtime_updates_report_unsupported_instead_of_silently_dropping_the_request() {
    let mut runtime = Runtime::new(
        None,
        |_: &Option<WindowCreation>, _| rune::el(),
        (200., 200.),
        Env::new(false, 1.),
    );
    runtime.update(|state, tx| {
        *state = Some(
            tx.spawn_window(spec("child", Rc::new(String::new())))
                .unwrap()
                .creation,
        )
    });
    failed_offscreen(runtime.state.as_ref().unwrap());
}
#[test]
fn poll_actions_propagate_their_request_through_the_actual_frame_transaction() {
    let mut runtime = Runtime::new(
        None,
        |_: &Option<WindowCreation>, _| rune::el(),
        (200., 200.),
        Env::new(false, 1.),
    );
    runtime.poll = Some(|state, _, tx| {
        if state.is_some() {
            return false;
        }
        *state = Some(
            tx.spawn_window(spec("poll-child", Rc::new(String::new())))
                .unwrap()
                .creation,
        );
        true
    });
    runtime.frame(0.);
    // Without the Tx merge seam this is an owner-dropped error instead.
    failed_offscreen(runtime.state.as_ref().unwrap());
}
#[test]
fn a_cloned_transaction_releases_its_non_send_factory_once_and_backpressure_recovers() {
    let captured = Rc::new("owned local state".to_owned());
    let mut pending = Tx::default();
    let mut creations = Vec::new();
    let mut full = false;
    for i in 0..1024 {
        match pending.spawn_window(spec(&format!("peer-{i}"), captured.clone())) {
            Ok(spawned) => creations.push(spawned.creation),
            Err(error) => {
                assert!(error.to_string().contains("queue"));
                full = true;
                break;
            }
        }
    }
    assert!(
        full && !creations.is_empty(),
        "pending creation work has a finite bound"
    );
    assert!(Rc::strong_count(&captured) > 1);
    let duplicate = pending.clone();
    let mut runtime = Runtime::new((), |_, _| rune::el(), (200., 200.), Env::new(false, 1.));
    runtime.update(|_, tx| *tx = pending);
    assert_eq!(
        Rc::strong_count(&captured),
        1,
        "even a retained Tx clone cannot keep consumed factories alive"
    );
    for creation in &creations {
        failed_offscreen(creation);
    }
    runtime.update(|_, tx| *tx = duplicate);
    assert_eq!(Rc::strong_count(&captured), 1);
    runtime.update(|_, tx| {
        let spawned = tx
            .spawn_window(spec("next-event", captured.clone()))
            .unwrap();
        assert_eq!(spawned.creation.status(), WindowSpawnStatus::Queued);
    });
    assert_eq!(
        Rc::strong_count(&captured),
        1,
        "backpressure does not become a lifetime limit"
    );
}
#[test]
fn cancelling_before_registration_is_observable_and_releases_owned_state() {
    let captured = Rc::new("cancelled local factory".to_owned());
    let spawned = {
        let mut tx = Tx::default();
        tx.spawn_window(spec("orphan", captured.clone())).unwrap()
    };
    assert_eq!(Rc::strong_count(&captured), 1);
    assert!(
        matches!(spawned.creation.status(),WindowSpawnStatus::Failed(ref reason) if reason.contains("before the window was registered"))
    );
    assert!(
        spawned
            .app
            .window_error()
            .unwrap()
            .contains("before the window was registered")
    );
    assert!(spawned.window.geometry().is_none());
    assert!(!spawned.window.is_ready());
    assert!(spawned.window.set_maximized(true).is_err());
}
#[test]
fn an_empty_key_is_rejected_without_retaining_the_factory() {
    let captured = Rc::new("invalid local factory".to_owned());
    let mut tx = Tx::default();
    assert!(tx.spawn_window(spec("", captured.clone())).is_err());
    assert_eq!(Rc::strong_count(&captured), 1);
}
