//! Private synthetic native proof: no backend, provider, audio or user files.
use rune::{
    App, AppHandle, AppSpec, Cx, El, SpawnedWindow, Tx, WindowHandle, WindowSpawnStatus, WindowSpec,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

struct World {
    output: PathBuf,
    root: Option<AppHandle<State>>,
    current: Option<SpawnedWindow<State>>,
    duplicate: Option<SpawnedWindow<State>>,
    stale: Option<AppHandle<State>>,
    grandchild: Option<SpawnedWindow<State>>,
    phase: u32,
    cycles: u32,
    vetoes: u32,
    root_close_hooks: u32,
    initial_geometry: bool,
    parent_survived_veto: bool,
    peer_survived_parent: bool,
    duplicate_rejected: bool,
    since: Instant,
    start: Instant,
}
struct State {
    world: Rc<RefCell<World>>,
    id: u32,
    ready: bool,
    allow_close: bool,
    native: Option<WindowHandle>,
}
fn unused() -> State {
    panic!("proof always supplies its state explicitly")
}
fn view(state: &State, _: &mut Cx<State>) -> El {
    let world = state.world.borrow();
    rune::col()
        .w_full()
        .h_full()
        .padding(20.)
        .gap(14.)
        .fill(rune::oklch(0.25, 0.015, 250.))
        .child(
            rune::text(format!("Native dynamic window {}", state.id))
                .color(rune::oklch(0.91, 0.012, 250.)),
        )
        .child(
            rune::text(format!(
                "Independent Rc state · phase {} · cycle {}",
                world.phase, world.cycles
            ))
            .color(rune::oklch(0.73, 0.025, 250.)),
        )
}
fn guard(state: &mut State, _: &mut Tx) -> bool {
    if state.id == 0 && !state.allow_close {
        state.world.borrow_mut().vetoes += 1;
        false
    } else {
        true
    }
}
fn write(world: &World, phase: &str) {
    let data = serde_json::json!({"phase":phase,"cycles":world.cycles,"vetoes":world.vetoes,"root_close_hooks":world.root_close_hooks,"initial_geometry":world.initial_geometry,"duplicate_rejected":world.duplicate_rejected,"parent_survived_veto":world.parent_survived_veto,"peer_survived_parent":world.peer_survived_parent,"pid":std::process::id()});
    std::fs::write(
        world.output.join("inspection.json"),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
}
fn window(world: Rc<RefCell<World>>, id: u32, key: &str) -> WindowSpec<State> {
    let app = App::new(AppSpec {
        poll: Some(poll),
        fonts: &[include_bytes!(
            "../../../themes/opencode/assets/fonts/InterVariable.ttf"
        )],
        families: ("Inter", "Inter"),
        ..AppSpec::new("Rune dynamic-window proof", (420., 260.), unused, view)
    })
    .args(Vec::<String>::new())
    .init(move || State {
        world,
        id,
        ready: false,
        allow_close: id != 0,
        native: None,
    })
    .on_close_requested(guard)
    .on_close(|state, _| {
        if state.id == 0 {
            state.world.borrow_mut().root_close_hooks += 1;
        }
    });
    let handle = app.handle();
    WindowSpec::from_app(key, app)
        .options(rune::WindowOptions {
            position: Some((40 + (id as i32 % 2) * 460, 60)),
            ..Default::default()
        })
        .on_ready(move |native| {
            let native = native.clone();
            handle.update(move |state, tx| {
                state.ready = true;
                state.native = Some(native);
                tx.instant();
            });
        })
}
fn poll(state: &mut State, _: f64, tx: &mut Tx) -> bool {
    tx.wake_after(Duration::from_millis(20));
    let mut world = state.world.borrow_mut();
    assert!(
        world.start.elapsed() < Duration::from_secs(75),
        "native lifecycle proof timed out at phase {} / cycle {}",
        world.phase,
        world.cycles
    );
    if !state.ready {
        return false;
    }
    if state.id == 0 {
        if let Some(current) = &world.current {
            if let WindowSpawnStatus::Failed(error) = current.creation.status() {
                panic!("dynamic peer creation failed: {error}");
            }
        }
        match world.phase {
            0 => {
                world.initial_geometry = state
                    .native
                    .as_ref()
                    .and_then(WindowHandle::geometry)
                    .is_some_and(|g| {
                        g.inner_size.0 > 0 && g.scale_factor > 0. && g.monitor.is_some()
                    });
                let spec = window(state.world.clone(), 1, "reused-peer");
                world.current = Some(tx.spawn_window(spec).unwrap());
                world.phase = 1;
            }
            1 if world.current.as_ref().unwrap().window.is_ready() => {
                assert!(matches!(
                    world.current.as_ref().unwrap().creation.status(),
                    WindowSpawnStatus::Created
                ));
                world.duplicate = Some(
                    tx.spawn_window(window(state.world.clone(), 999, "reused-peer"))
                        .unwrap(),
                );
                world.phase = 2;
                world.since = Instant::now();
                write(&world, "two-native-windows");
            }
            2 if world.since.elapsed() > Duration::from_millis(500) => {
                assert!(matches!(
                    world.duplicate.as_ref().unwrap().creation.status(),
                    WindowSpawnStatus::Failed(_)
                ));
                world.duplicate_rejected = true;
                world.current.as_ref().unwrap().app.close();
                world.phase = 3;
            }
            3 if world.current.as_ref().unwrap().window.geometry().is_none() => {
                let old = world.current.take().unwrap();
                world.stale = Some(old.app);
                world.cycles += 1;
                let spec = window(state.world.clone(), world.cycles + 1, "reused-peer");
                world.current = Some(tx.spawn_window(spec).unwrap());
                world.phase = 4;
            }
            4 if world.current.as_ref().unwrap().window.is_ready() => {
                world.stale.as_ref().unwrap().close();
                world.since = Instant::now();
                world.phase = 5;
            }
            5 if world.since.elapsed() > Duration::from_millis(25) => {
                assert!(
                    world.current.as_ref().unwrap().app.is_visible(),
                    "an old route closed its replacement"
                );
                if world.cycles < 80 {
                    world.current.as_ref().unwrap().app.close();
                    world.phase = 3;
                } else {
                    world.phase = 6;
                    tx.close_window();
                }
            }
            6 if world.vetoes == 1 => {
                world.parent_survived_veto = world.root.as_ref().unwrap().is_visible()
                    && state.native.as_ref().unwrap().geometry().is_some();
                assert!(world.parent_survived_veto);
                assert_eq!(
                    world.root_close_hooks, 0,
                    "veto must run before the existing close hook"
                );
                state.allow_close = true;
                world.phase = 7;
                world.since = Instant::now();
                tx.close_window();
            }
            _ => return false,
        }
    } else if state.id == world.cycles + 1
        && world.phase == 7
        && world.since.elapsed() > Duration::from_millis(500)
    {
        world.peer_survived_parent =
            !world.root.as_ref().unwrap().is_visible() && state.native.as_ref().unwrap().is_ready();
        assert!(
            world.peer_survived_parent,
            "root_visible={} peer_ready={}",
            world.root.as_ref().unwrap().is_visible(),
            state.native.as_ref().unwrap().is_ready()
        );
        assert_eq!(world.root_close_hooks, 1);
        write(&world, "peer-survived-parent");
        world.grandchild = Some(
            tx.spawn_window(window(state.world.clone(), 1000, "after-parent"))
                .unwrap(),
        );
        world.phase = 8;
        world.since = Instant::now();
    } else if state.id == 1000
        && world.phase == 8
        && state.ready
        && world.since.elapsed() > Duration::from_millis(500)
    {
        assert!(world.current.as_ref().unwrap().app.is_visible());
        assert!(world.grandchild.as_ref().unwrap().window.is_ready());
        write(&world, "complete");
        world.phase = 9;
        tx.quit();
    } else {
        return false;
    }
    tx.instant();
    true
}
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--failure") {
        failure::run(
            std::env::args_os()
                .nth(2)
                .map(PathBuf::from)
                .expect("private failure output directory"),
        );
        return;
    }
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("absolute private proof output directory");
    assert!(output.is_absolute());
    std::fs::create_dir_all(&output).unwrap();
    let world = Rc::new(RefCell::new(World {
        output,
        root: None,
        current: None,
        duplicate: None,
        stale: None,
        grandchild: None,
        phase: 0,
        cycles: 0,
        vetoes: 0,
        root_close_hooks: 0,
        initial_geometry: false,
        parent_survived_veto: false,
        peer_survived_parent: false,
        duplicate_rejected: false,
        since: Instant::now(),
        start: Instant::now(),
    }));
    let root = window(world.clone(), 0, "root");
    world.borrow_mut().root = Some(root.app.handle());
    root.app.run();
    assert_eq!(
        world.borrow().phase,
        9,
        "native loop ended before independent peer finished"
    );
}

// A windowless driver can observe a real native creation failure when the caller
// starts this separate process with no Vulkan ICD. No test-only runtime API exists.
mod failure {
    use super::*;
    use std::cell::Cell;
    struct Token {
        drops: Rc<Cell<u32>>,
        setup: Rc<Cell<bool>>,
    }
    impl Drop for Token {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    struct Driver {
        output: PathBuf,
        child: Option<SpawnedWindow<Token>>,
        drops: Rc<Cell<u32>>,
        setup: Rc<Cell<bool>>,
        start: Instant,
    }
    fn unused_driver() -> Driver {
        panic!("driver supplies captured state")
    }
    fn unused_token() -> Token {
        panic!("child supplies captured state")
    }
    fn poll(driver: &mut Driver, _: f64, tx: &mut Tx) -> bool {
        assert!(
            driver.start.elapsed() < Duration::from_secs(20),
            "native failure did not settle"
        );
        tx.wake_after(Duration::from_millis(20));
        if driver.child.is_none() {
            let token = Token {
                drops: driver.drops.clone(),
                setup: driver.setup.clone(),
            };
            let spec = WindowSpec::new(
                "failed-peer",
                AppSpec {
                    setup: Some(|token: &mut Token, _| token.setup.set(true)),
                    ..AppSpec::new("failed native peer", (420., 400.), unused_token, |_, _| {
                        rune::el()
                    })
                },
            )
            .state(token);
            driver.child = Some(tx.spawn_window(spec).unwrap());
            return true;
        }
        let child = driver.child.as_ref().unwrap();
        match child.creation.status() {
            WindowSpawnStatus::Failed(error) => {
                assert!(
                    driver.setup.get(),
                    "failure must follow actual host initialization"
                );
                assert_eq!(
                    driver.drops.get(),
                    1,
                    "failed native creation retained its owned state"
                );
                assert!(child.window.geometry().is_none());
                assert!(child.app.window_error().is_some());
                let data = serde_json::json!({"native_creation_failed":true,"host_initialized":true,"owned_state_dropped":true,"error":error});
                std::fs::write(
                    driver.output.join("failure.json"),
                    serde_json::to_vec(&data).unwrap(),
                )
                .unwrap();
                tx.quit();
                true
            }
            _ => false,
        }
    }
    pub(super) fn run(output: PathBuf) {
        assert!(output.is_absolute());
        std::fs::create_dir_all(&output).unwrap();
        let app = App::new(AppSpec {
            setup: Some(|_: &mut Driver, wake| wake()),
            poll: Some(poll),
            ..AppSpec::new(
                "windowless failure driver",
                (420., 400.),
                unused_driver,
                |_, _| rune::el(),
            )
        })
        .args(Vec::<String>::new())
        .init(move || Driver {
            output,
            child: None,
            drops: Rc::new(Cell::new(0)),
            setup: Rc::new(Cell::new(false)),
            start: Instant::now(),
        })
        .windowless();
        app.run();
    }
}
