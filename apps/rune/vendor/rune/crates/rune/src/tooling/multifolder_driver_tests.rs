//! End-to-end script dispatch through the two offscreen driver loops.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Observed {
    answers: Vec<crate::MultiFileDialogResult>,
    busy: Vec<crate::MultiFileDialogResult>,
    keys: usize,
}
struct State {
    observed: Rc<RefCell<Observed>>,
    opened: bool,
}
thread_local! {
    static OBSERVED: RefCell<Option<Rc<RefCell<Observed>>>> = const { RefCell::new(None) };
}
fn init() -> State {
    State {
        observed: OBSERVED.with(|o| o.borrow().as_ref().unwrap().clone()),
        opened: false,
    }
}
fn view(_: &State, _: &mut Cx<State>) -> crate::El {
    crate::el().size(120.0, 80.0)
}
fn key(state: &mut State, _: &KeyEvent, tx: &mut Tx) -> bool {
    if !state.opened {
        state.opened = true;
        tx.open_folders(
            crate::FileDialogOptions::default(),
            |s: &mut State, answer, _| {
                s.observed.borrow_mut().answers.push(answer);
            },
        );
        // A second request is refused asynchronously; it must not stop the script.
        tx.open_folders(
            crate::FileDialogOptions::default(),
            |s: &mut State, answer, _| {
                s.observed.borrow_mut().busy.push(answer);
            },
        );
    } else {
        state.observed.borrow_mut().keys += 1;
    }
    true
}
fn spec() -> AppSpec<State> {
    let mut spec = AppSpec::new("Script test", (120.0, 80.0), init, view);
    spec.on_key = Some(key);
    spec
}
fn run_case(desktop: bool, cancel: bool) {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let _guard = SERIAL.lock().unwrap();
    let observed = Rc::new(RefCell::new(Observed::default()));
    OBSERVED.with(|o| *o.borrow_mut() = Some(observed.clone()));
    let dir = std::env::temp_dir().join(format!(
        "rune-dialog-driver-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let answer = if cancel {
        json!({"t": 0.2, "type": "file-dialog-cancel"})
    } else {
        json!({"t": 0.2, "type": "file-dialogs", "paths": ["/second", "/first"]})
    };
    let script = json!([
        {"t": 0.0, "type": "type", "text": "a"},
        answer,
        {"t": 0.4, "type": "type", "text": "b"}
    ]);
    let script_path = dir.join("script.json");
    std::fs::write(&script_path, script.to_string()).unwrap();
    let mut args = Args {
        script: Some(script_path),
        fps: 10.0,
        scale: Some(1.0),
        size: Some((120.0, 80.0)),
        at: Some(0.5),
        every: 1,
        ..Default::default()
    };
    if desktop {
        let backdrop = dir.join("backdrop.png");
        ::image::RgbaImage::new(120, 80).save(&backdrop).unwrap();
        args.desktop = Some(backdrop);
        crate::desktop::run(
            vec![crate::WindowSpec::new("requester", spec()).into()],
            0,
            args,
            false,
        );
    } else {
        headless_configured(crate::app::App::new(spec()), args, false);
    }
    OBSERVED.with(|o| *o.borrow_mut() = None);
    let result = observed.borrow();
    let expected = if cancel {
        None
    } else {
        Some(vec!["/second".into(), "/first".into()])
    };
    assert_eq!(result.answers, vec![Ok(expected)]);
    assert_eq!(result.busy.len(), 1);
    assert!(result.busy[0].is_err());
    assert_eq!(result.keys, 1, "script continued to its next key");
}
#[test]
fn tooling_loop_delivers_ordered_folders() {
    run_case(false, false);
}
#[test]
fn tooling_loop_cancels_and_continues() {
    run_case(false, true);
}
#[test]
fn desktop_loop_delivers_ordered_folders() {
    run_case(true, false);
}
#[test]
fn desktop_loop_cancels_and_continues() {
    run_case(true, true);
}
