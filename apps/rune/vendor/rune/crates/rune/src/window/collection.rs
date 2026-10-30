//! Live native host collection, dispatch and independent loop lifetimes.
use super::*;

pub(super) struct Windows {
    pub(super) factory: Option<Factory>,
    pub(super) args: Args,
    pub(super) proxy: EventLoopProxy<WakeEvent>,
    pub(super) hosts: BTreeMap<usize, Box<dyn AnyHost>>,
    pub(super) next_host: usize,
    // Weak pool preserves release_gpu_when_closed when the last GPU owner closes.
    pub(super) gpu: Rc<RefCell<Weak<Gpu>>>,
}
impl Windows {
    fn cleanup(&mut self, event_loop: &ActiveEventLoop) {
        self.spawn_pending(event_loop);
        self.hosts.retain(|_, host| {
            if host.finished() { host.cancel_menus(); false } else { true }
        });
        // Another window's quit: a menu still tracking would hold the loop open.
        if event_loop.exiting() {
            for host in self.hosts.values_mut() { host.cancel_menus(); }
        }
        if !self.hosts.values().any(|h| h.keeps_loop()) {
            // The app ends with its last main window: windows still open close as their close
            // button would. A quit already asked to exit and skips their hooks.
            if !event_loop.exiting() {
                for host in self.hosts.values_mut() { host.terminate(); }
            }
            event_loop.exit();
        }
    }

    /// The app was reopened: every `on_reopen` hook runs, then, unless one handled it or the
    /// app is an accessory, the default brings back a window when none shows (`reopen::target`).
    fn reopen(&mut self, event_loop: &ActiveEventLoop) {
        let candidates = |hosts: &BTreeMap<usize, Box<dyn AnyHost>>| -> Vec<_> {
            hosts.values().map(|h| h.reopen_candidate()).collect()
        };
        let has_visible = crate::reopen::has_visible(&candidates(&self.hosts));
        let mut handled = false;
        for host in self.hosts.values_mut() {
            handled |= host.reopen_hook(event_loop, has_visible);
        }
        if handled || event_loop.exiting() { return; }
        #[cfg(target_os = "macos")]
        if crate::reopen::accessory() { return; }
        // The default sees what the hooks did, to other windows too.
        for host in self.hosts.values_mut() { host.apply_pending(event_loop); }
        if event_loop.exiting() { return; }
        #[cfg(target_os = "macos")]
        crate::reopen::unhide();
        let target = crate::reopen::target(&candidates(&self.hosts)).and_then(|i| self.hosts.keys().nth(i).copied());
        if let Some(host) = target.and_then(|id| self.hosts.get_mut(&id)) { host.reopen(event_loop); }
    }
}


impl ApplicationHandler<WakeEvent> for Windows {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        for host in self.hosts.values_mut() { host.new_events(event_loop, cause); }
        // Before resumed there are no hosts to clean up.
        if self.factory.is_none() { self.cleanup(event_loop); }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(factory) = self.factory.take() else { return };
        let primary = event_loop.primary_monitor();
        let displays: Vec<_> = event_loop.available_monitors().map(|m| Display {
            id: Some(crate::DisplayId::from_monitor(m.clone(), is_wayland(event_loop))),
            name: m.name(), position: (m.position().x, m.position().y),
            size: (m.size().width, m.size().height), scale: m.scale_factor(), primary: primary.as_ref() == Some(&m),
        }).collect();
        let specs = crate::windows::flatten(factory(&displays));
        crate::windows::validate(&specs).expect("window keys");
        let trace_window = self.args.window.clone().or_else(|| specs.first().map(|s| s.key().to_owned()));
        let mut overlays = Vec::new();
        let wayland = is_wayland(event_loop);
        self.next_host = specs.len();
        self.hosts = start_all(specs, |slot, spec| {
            let traced = trace_window.as_deref() == Some(spec.key());
            spec.start(self.args.clone(), Start { proxy: self.proxy.clone(), slot, gpu: self.gpu.clone(), wayland, traced, overlays: &mut overlays })
        }, |host| host.resumed(event_loop)).into_iter().enumerate().filter_map(|(id, host)| host.map(|host| (id, host))).collect();
        for mut overlay in overlays {
            let (slot, proxy) = (self.allocate_host(), self.proxy.clone());
            let frame_proxy = proxy.clone();
            overlay.connect(Arc::new(move || { let _ = proxy.send_event(WakeEvent::App(slot)); }),
                Arc::new(move || { let _ = frame_proxy.send_event(WakeEvent::DisplayTick(slot)); }), self.gpu.clone());
            overlay.resumed(event_loop);
            self.hosts.insert(slot, overlay);
        }
        #[cfg(target_os = "macos")]
        {
            let proxy = self.proxy.clone();
            crate::reopen::install(move || proxy.send_event(WakeEvent::Reopen).is_ok());
        }
        crate::open_url_in::settle();
        // A window opened at launch took it; a GPU kept for a later one would sit idle until then.
        crate::render::discard_prewarm();
        self.cleanup(event_loop);
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: WakeEvent) {
        let slot = match &event {
            WakeEvent::DisplayTick(i) | WakeEvent::App(i) | WakeEvent::Live(i) | WakeEvent::Window(i, _) | WakeEvent::Focus(i)
                | WakeEvent::Position(i, _) | WakeEvent::Level(i, _) | WakeEvent::Place(i, _) | WakeEvent::Frame(i, _) => *i,
            WakeEvent::Reopen => {
                self.reopen(event_loop);
                self.cleanup(event_loop);
                return;
            }
        };
        if let Some(host) = self.hosts.get_mut(&slot) {
            host.user_event(event_loop, event);
        }
        self.cleanup(event_loop);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(host) = self.hosts.values_mut().find(|h| h.owns(id)) {
            let _input = crate::input_time::arrived(&event);
            host.window_event(event_loop, id, event);
        }
        self.cleanup(event_loop);
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        crate::os_cursor::make_pending(event_loop);
        let mut animating = false;
        for host in self.hosts.values_mut() {
            animating |= host.about_to_wait(event_loop);
        }
        if self.factory.is_none() { self.cleanup(event_loop); }
        // Layer surfaces have no redraw events: keep turning while one animates (its vsync
        // paces the loop). Hosts whose only motion is an ambient loop sleep until its next step.
        let deadline = self.hosts.values().filter_map(|host| host.wake_deadline()).min();
        event_loop.set_control_flow(if animating { ControlFlow::Poll }
            else { deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil) });
    }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        // Rune didn't ask to exit, so the OS is ending the app: macOS Quit, Cmd-Q, the Dock's
        // Quit, logout. Windows close as their close button would, before the clipboard stops.
        if !event_loop.exiting() {
            for host in self.hosts.values_mut() { host.terminate(); }
        }
        for host in self.hosts.values_mut() { host.cancel_menus(); }
        // Surfaces go while winit's display connection is still open (layer surfaces live on it).
        self.hosts.clear();
        crate::clipboard::shutdown();
    }
}

