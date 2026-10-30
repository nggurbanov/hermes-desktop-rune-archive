//! The event thread registers independent hosts before retiring their creator.
use super::*;
impl Windows {
    pub(super) fn allocate_host(&mut self) -> usize {
        let id = self.next_host;
        self.next_host = id
            .checked_add(1)
            .expect("native host routing identity exhausted");
        id
    }
    pub(super) fn spawn_pending(&mut self, event_loop: &ActiveEventLoop) {
        let requests: Vec<_> = self
            .hosts
            .values_mut()
            .flat_map(|host| host.take_new_windows())
            .collect();
        for mut request in requests {
            if event_loop.exiting() {
                request.fail("The application is quitting");
                continue;
            }
            let specs = crate::windows::flatten([request.take_spec()]);
            if let Err(error) = crate::windows::validate(&specs) {
                request.fail(error);
                continue;
            }
            if specs.iter().any(|spec| {
                self.hosts
                    .values()
                    .any(|host| !host.finished() && host.key() == Some(spec.key()))
            }) {
                request.fail("A live window already owns this key");
                continue;
            }
            // A peer is a new ordinary app, not a replay of its opener's CLI script.
            let prepared: Vec<_> = specs
                .into_iter()
                .map(|mut spec| {
                    let args = spec.resolve_args(Args {
                        fps: 60.0,
                        every: 1,
                        cols: 10,
                        ..Default::default()
                    });
                    (spec, args)
                })
                .collect();
            if prepared.iter().any(|(_, args)| args.headless()) {
                request.fail("A dynamic native window cannot use offscreen tooling arguments");
                continue;
            }
            request.registered();
            let mut overlays = Vec::new();
            let mut hosts = Vec::new();
            for (spec, inherited) in prepared {
                let slot = self.allocate_host();
                if let Some(host) = spec.start(
                    inherited,
                    Start {
                        proxy: self.proxy.clone(),
                        slot,
                        gpu: self.gpu.clone(),
                        wayland: is_wayland(event_loop),
                        traced: false,
                        overlays: &mut overlays,
                    },
                ) {
                    hosts.push((slot, host));
                }
            }
            // All shortcut/menu guards exist before any creation callback runs.
            let registered: Vec<_> = hosts.iter().map(|(slot, _)| *slot).collect();
            for (slot, mut host) in hosts {
                host.resumed(event_loop);
                self.hosts.insert(slot, host);
            }
            // Failed initial native creation must not leave an invisible host
            // retaining its state and keeping the loop alive after its opener closes.
            if request.check_error() {
                for slot in registered {
                    self.hosts.remove(&slot);
                }
                continue;
            }
            for mut overlay in overlays {
                let slot = self.allocate_host();
                let proxy = self.proxy.clone();
                let frame_proxy = proxy.clone();
                overlay.connect(
                    Arc::new(move || {
                        let _ = proxy.send_event(WakeEvent::App(slot));
                    }),
                    Arc::new(move || {
                        let _ = frame_proxy.send_event(WakeEvent::DisplayTick(slot));
                    }),
                    self.gpu.clone(),
                );
                overlay.resumed(event_loop);
                self.hosts.insert(slot, overlay);
            }
        }
    }
}
