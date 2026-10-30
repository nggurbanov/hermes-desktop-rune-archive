//! Complete native windows own their input and source even after their opener closes.
use super::*;
use crate::client::windows::{
    BootstrapKind, ClaimError, Geometry, WindowId, WindowLease, WindowRoute, WindowStore,
};
use rune::{AppHandle, WindowCreation, WindowHandle, WindowSpawnStatus, WindowSpec};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "windows/proof.rs"]
mod proof;
pub(crate) use proof::inspect_native_frame;
#[path = "windows/source.rs"]
mod source;
use source::{Pinned, Retained, Source};
static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
pub(super) fn source_epoch() -> u64 {
    NEXT_SOURCE.fetch_add(1, Ordering::Relaxed)
}
thread_local! { static WORKSPACE: RefCell<Weak<RefCell<Registry>>> = const { RefCell::new(Weak::new()) }; }
#[derive(Default)]
struct Registry {
    proof: Option<PathBuf>,
    legacy: Option<Connection>,
    entries: BTreeMap<WindowId, Entry>,
}
struct Entry {
    kind: BootstrapKind,
    app: AppHandle<Viewer>,
    host: WindowHandle,
    target: Option<FocusTarget>,
}
#[derive(Clone, PartialEq, Eq)]
struct FocusTarget {
    principal: String,
    profile: String,
    session: Option<String>,
    bot: bool,
}
impl Registry {
    fn shared() -> Rc<RefCell<Self>> {
        WORKSPACE.with(|slot| {
            if let Some(shared) = slot.borrow().upgrade() {
                return shared;
            }
            let shared = Rc::new(RefCell::new(Self::default()));
            *slot.borrow_mut() = Rc::downgrade(&shared);
            shared
        })
    }
}
pub(super) struct State {
    lease: Option<WindowLease>,
    menu: Option<rune::PointerPress>,
    quit_requested: bool,
    close_requested: bool,
    source_retired: bool,
    native: Option<WindowHandle>,
    registry: Rc<RefCell<Registry>>,
    requested: VecDeque<Open>,
    creations: Vec<(WindowId, WindowCreation)>,
    source: Option<Source>,
    retained: Option<Retained>,
    pinned: Option<Pinned>,
    pending_fresh: Option<source::Fresh>,
    boot_target: Option<WindowRoute>,
    started: bool,
    storage_error: Option<String>,
    last_unhandled_key: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            lease: None,
            menu: None,
            quit_requested: false,
            close_requested: false,
            source_retired: false,
            native: None,
            registry: Registry::shared(),
            requested: VecDeque::new(),
            creations: vec![],
            source: None,
            retained: None,
            pinned: None,
            pending_fresh: None,
            boot_target: None,
            started: false,
            storage_error: None,
            last_unhandled_key: None,
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(lease) = &self.lease {
            self.registry.borrow_mut().entries.remove(lease.id());
        }
    }
}
struct Open {
    kind: BootstrapKind,
    route: WindowRoute,
    source: Option<Source>,
    target: Option<FocusTarget>,
    presentation: crate::client::Preferences,
    geometry: Geometry,
}
/// Unmounted construction owns rollback. Once mounted, the Viewer owns crash recovery.
pub(crate) struct Bootstrap {
    lease: Option<WindowLease>,
    source: Option<Source>,
    registry: Rc<RefCell<Registry>>,
}
impl Drop for Bootstrap {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            let _ = lease.close();
        }
    }
}
impl Viewer {
    pub(crate) fn from_window_bootstrap(mut boot: Bootstrap) -> Self {
        let mut s = Self::empty();
        s.load_client_state();
        if let Some(lease) = boot.lease.take() {
            let record = lease.record().clone();
            if let Some(prefs) = record.presentation {
                s.hydrate_client_preferences(prefs);
            }
            s.profile = record.route.profile.clone();
            s.exact_connection_profile = true;
            s.window.boot_target = Some(record.route);
            s.window.lease = Some(lease);
        }
        s.window.source = boot.source.take();
        s.window.registry = boot.registry.clone();
        s.connection_form = false;
        s.loading = true;
        s.status = "Opening captured agent…".into();
        s
    }
    pub(super) fn init_window_storage(&mut self) {
        if self.fixture {
            return;
        }
        self.connection_epoch = source_epoch();
        self.configure_window_inspection();
        if self.window.lease.is_none() {
            let result = WindowStore::system().and_then(|store| match store.claim_primary() {
                Ok(lease) => Ok(lease),
                Err(ClaimError::Busy(_)) => store
                    .mint_peer(BootstrapKind::PeerInherited, WindowRoute::default())
                    .map_err(|e| e.to_string()),
                Err(error) => Err(error.to_string()),
            });
            match result {
                Ok(lease) => {
                    let record = lease.record().clone();
                    // Explicit CLI routes take precedence over launch restoration.
                    self.restore_launch_route(record.route);
                    if let Some(prefs) = record.presentation {
                        self.hydrate_client_preferences(prefs);
                    }
                    self.window.lease = Some(lease);
                }
                Err(error) => {
                    self.client_error = Some(error);
                    self.draft_storage_available = false;
                    return;
                }
            }
        }
        if let Some(lease) = &self.window.lease {
            self.composer = Composer::with_editor(lease.editor_id().clone());
        }
    }
    fn restore_launch_route(&mut self, mut route: WindowRoute) {
        if self.connection.is_some() {
            return;
        }
        if self.exact_connection_profile {
            route.profile = self.profile.clone();
            route.session = None;
            route.canonical_bot = false;
        } else {
            self.profile = route.profile.clone();
            self.exact_connection_profile = true;
        }
        self.window.boot_target = Some(route);
    }
    pub(crate) fn window_ready(&mut self, app: AppHandle<Viewer>, host: WindowHandle) {
        self.native_window = Some(app.clone());
        if let Some(lease) = &self.window.lease {
            let geometry = &lease.record().geometry;
            host.resize_instant((geometry.size.0 as f32, geometry.size.1 as f32));
            let _ = host.set_maximized(geometry.maximized);
            if let Some(position) = geometry.position {
                host.set_position(position);
            }
            let target = self
                .window
                .registry
                .borrow()
                .entries
                .get(lease.id())
                .and_then(|entry| entry.target.clone());
            self.window.registry.borrow_mut().entries.insert(
                lease.id().clone(),
                Entry {
                    kind: lease.record().kind,
                    app,
                    host: host.clone(),
                    target,
                },
            );
        }
        self.window.native = Some(host);
        self.remember_legacy_window_source();
        if !self.begin_device_startup() {
            self.start_window_source();
        }
    }
    pub fn window_menu(&self) -> Option<rune::PointerPress> {
        self.window.menu
    }
    pub fn open_window_menu(&mut self, point: rune::PointerPress) {
        self.window.menu = Some(point);
        self.row_menu = None;
        self.sidebar_state.menu = None;
    }
    pub fn close_window_menu(&mut self) {
        self.window.menu = None;
    }
    pub fn request_window_close(&mut self) {
        self.window.close_requested = true;
        self.window.menu = None;
    }
    pub(super) fn switching_window_source(&mut self) {
        self.window.source = None;
        self.window.pending_fresh = None;
        self.window.retained = None;
        self.window.boot_target = None;
        self.window.started = true;
        self.window.source_retired = false;
    }
    pub fn request_application_quit(&mut self) {
        self.window.quit_requested = true;
        self.window.menu = None;
    }
    pub fn can_open_window(&self) -> bool {
        !self.fixture && self.window.lease.is_some()
    }
    pub fn request_new_window(&mut self) {
        self.request_window_target(None);
    }
    pub fn request_window_target(&mut self, target: Option<ConversationTarget>) {
        if !self.can_open_window() {
            self.client_error =
                Some("A native window with writable client state is required".into());
            return;
        }
        self.window.menu = None;
        self.row_menu = None;
        self.sidebar_state.menu = None;
        let mut route = WindowRoute {
            profile: self.profile.clone(),
            ..WindowRoute::default()
        };
        let kind = match target {
            Some(ConversationTarget::Session { profile, id, .. }) => {
                route.profile = profile;
                route.session = Some(id);
                BootstrapKind::InteractiveSession
            }
            Some(ConversationTarget::Bot(profile)) => {
                route.profile = profile;
                route.canonical_bot = true;
                BootstrapKind::ProfilePinned
            }
            None => BootstrapKind::PeerInherited,
        };
        let resolver = self
            .window
            .retained
            .as_ref()
            .map(|retained| retained._resolver.clone())
            .or_else(|| self.connections.resolver());
        let source = match Source::capture(self.connection.as_ref(), resolver) {
            Ok(source) => source,
            Err(error) => {
                self.client_error = Some(error);
                return;
            }
        };
        route.source = match self
            .connection
            .as_ref()
            .map(crate::client::windows::SourceDescriptor::from_connection)
            .transpose()
        {
            Ok(source) => source,
            Err(error) => {
                self.client_error = Some(error);
                return;
            }
        };
        let focus = self
            .connection
            .as_ref()
            .and_then(|c| crate::client::connection_key(c).ok())
            .map(|principal| FocusTarget {
                principal,
                profile: route.profile.clone(),
                session: route.session.clone(),
                bot: route.canonical_bot,
            });
        if kind != BootstrapKind::PeerInherited {
            if let Some(entry) = self
                .window
                .registry
                .borrow()
                .entries
                .values()
                .find(|entry| {
                    matches!(
                        entry.kind,
                        BootstrapKind::InteractiveSession | BootstrapKind::ProfilePinned
                    ) && entry.target == focus
                        && focus.is_some()
                })
            {
                entry.host.open();
                entry.app.open();
                return;
            }
        }
        let geometry = self.cascade_geometry();
        self.window.requested.push_back(Open {
            kind,
            route,
            source,
            target: focus,
            presentation: self.client_preferences_snapshot(),
            geometry,
        });
    }
    fn cascade_geometry(&self) -> Geometry {
        let fallback = self
            .window
            .lease
            .as_ref()
            .map(|l| l.record().geometry.clone())
            .unwrap_or_default();
        let Some(frame) = self.window.native.as_ref().and_then(WindowHandle::geometry) else {
            return fallback;
        };
        let scale = frame.scale_factor.max(0.1);
        let size = (
            (frame.inner_size.0 as f64 / scale).round() as u32,
            (frame.inner_size.1 as f64 / scale).round() as u32,
        );
        let position = frame.position.map(|(x, y)| {
            (
                x.saturating_add((32. * scale) as i32),
                y.saturating_add((32. * scale) as i32),
            )
        });
        Geometry {
            position,
            size,
            maximized: false,
        }
    }
    pub(super) fn poll_windows(&mut self, tx: &mut Tx) -> bool {
        if self.fixture {
            return false;
        }
        if self.window.quit_requested {
            self.window.quit_requested = false;
            tx.quit();
            return true;
        }
        if self.window.close_requested {
            self.window.close_requested = false;
            tx.close_window();
            return true;
        }
        if self.window.retained.is_some() {
            tx.wake_after(std::time::Duration::from_secs(1));
        }
        let mut changed = self.poll_window_source();
        while let Some(request) = self.window.requested.pop_front() {
            let result = self.spawn_window(request, tx);
            if let Err(error) = result {
                self.client_error = Some(error);
            }
            changed = true;
        }
        self.window
            .creations
            .retain(|(id, receipt)| match receipt.status() {
                WindowSpawnStatus::Queued | WindowSpawnStatus::Registered => true,
                WindowSpawnStatus::Created => false,
                WindowSpawnStatus::Failed(error) => {
                    self.window.registry.borrow_mut().entries.remove(id);
                    self.client_error = Some(error);
                    changed = true;
                    false
                }
            });
        if !self.window.creations.is_empty()
            || self.window.source.as_ref().is_some_and(Source::pending)
        {
            tx.wake_after(std::time::Duration::from_millis(40));
        }
        changed |= self.apply_window_target();
        if let Some(lease) = &mut self.window.lease {
            if let Err(error) = lease.poll_storage() {
                self.client_error = Some(error.clone());
                self.window.storage_error = Some(error);
            }
            if let Some(delay) = lease.wake_after() {
                tx.wake_after(delay);
            }
        }
        if !self
            .window
            .lease
            .as_ref()
            .is_some_and(|lease| lease.wake_after().is_some_and(|delay| !delay.is_zero()))
        {
            if let Err(error) = self.save_window_record(false) {
                self.client_error = Some(error);
            }
        }
        changed
    }
    fn spawn_window(&mut self, request: Open, tx: &mut Tx) -> Result<(), String> {
        if request.kind != BootstrapKind::PeerInherited {
            if let Some(entry) = self
                .window
                .registry
                .borrow()
                .entries
                .values()
                .find(|entry| {
                    matches!(
                        entry.kind,
                        BootstrapKind::InteractiveSession | BootstrapKind::ProfilePinned
                    ) && entry.target == request.target
                        && request.target.is_some()
                })
            {
                entry.host.open();
                return Ok(());
            }
        }
        let mut lease = WindowStore::system()?
            .mint_peer(request.kind, request.route)
            .map_err(|e| e.to_string())?;
        let mut record = lease.record().clone();
        record.geometry = request.geometry;
        record.presentation = Some(request.presentation);
        lease.save(record)?;
        let id = lease.id().clone();
        let boot = Bootstrap {
            lease: Some(lease),
            source: request.source,
            registry: self.window.registry.clone(),
        };
        let app = crate::desktop_window::application(Some(boot));
        let spawned = tx
            .spawn_window(WindowSpec::from_app(format!("hermes-{}", id.as_str()), app))
            .map_err(|e| e.to_string())?;
        self.window.registry.borrow_mut().entries.insert(
            id.clone(),
            Entry {
                kind: request.kind,
                app: spawned.app,
                host: spawned.window,
                target: request.target,
            },
        );
        self.window.creations.push((id, spawned.creation));
        Ok(())
    }
    fn apply_window_target(&mut self) -> bool {
        if self.loading
            || self.error.is_some()
            || self.connection.is_none()
            || self.status != "Connected"
        {
            return false;
        }
        let Some(route) = self.window.boot_target.take() else {
            return false;
        };
        if !self
            .profiles
            .iter()
            .any(|profile| profile.name == route.profile)
        {
            self.error = Some("The captured profile is no longer served by this agent".into());
            return true;
        }
        if route.canonical_bot {
            self.open_bot(route.profile);
        } else if let Some(id) = route.session {
            self.open_conversation(route.profile, id, "Conversation".into(), false, true);
        }
        true
    }
    fn save_window_record(&mut self, closed: bool) -> Result<(), String> {
        let presentation = self.client_preferences_snapshot();
        let source = self
            .connection
            .as_ref()
            .map(crate::client::windows::SourceDescriptor::from_connection)
            .transpose()?;
        let bot = self.bot_is_active(&self.profile);
        let route = WindowRoute {
            source,
            profile: self.profile.clone(),
            session: if bot {
                None
            } else {
                self.selected_session.clone()
            },
            canonical_bot: bot,
        };
        let frame = self.window.native.as_ref().and_then(WindowHandle::geometry);
        let Some(lease) = &mut self.window.lease else {
            return Ok(());
        };
        let mut record = lease.record().clone();
        if self.window.started && !self.window.source_retired && self.window.boot_target.is_none() {
            record.route = route.clone();
        }
        record.presentation = Some(presentation);
        record.closed = closed;
        if let Some(frame) = frame {
            record.geometry.position = frame.position;
            record.geometry.size = (
                (frame.inner_size.0 as f64 / frame.scale_factor).round() as u32,
                (frame.inner_size.1 as f64 / frame.scale_factor).round() as u32,
            );
            record.geometry.maximized = frame.maximized;
        }
        if record != *lease.record() {
            lease.save(record)?;
        }
        if let Some(entry) = self
            .window
            .registry
            .borrow_mut()
            .entries
            .get_mut(lease.id())
        {
            let route = &lease.record().route;
            entry.target = self
                .connection
                .as_ref()
                .and_then(|c| crate::client::connection_key(c).ok())
                .map(|principal| FocusTarget {
                    principal,
                    profile: route.profile.clone(),
                    session: route.session.clone(),
                    bot: route.canonical_bot,
                });
        }
        Ok(())
    }
}
pub(crate) fn may_close(s: &mut Viewer, _tx: &mut Tx) -> bool {
    if !s.allow_owner_change("") {
        return false;
    }
    let result = s
        .flush_window_editors()
        .and_then(|_| s.flush_window_preferences())
        .and_then(|_| s.save_window_record(false));
    if let Err(error) = result {
        s.client_error = Some(error);
        return false;
    }
    true
}
impl Viewer {
    fn flush_window_preferences(&mut self) -> Result<(), String> {
        if self.preferences_writable {
            self.preferences_retry_at = None;
            self.save_client_preferences()?;
        } else if self.preferences_read_only_baseline.as_ref()
            != Some(&self.client_preferences_snapshot())
        {
            return Err(
                "Client preference changes could not be saved; the original file was retained"
                    .into(),
            );
        }
        if let Some(store) = &mut self.device_startup {
            store.flush()?;
        }
        self.client_settings.flush_preferences()?;
        self.sidebar_state.flush_storage()?;
        self.profile_preferences.flush_storage()
    }
}
pub(crate) fn closed(s: &mut Viewer, _tx: &mut Tx) {
    let _ = s.flush_window_editors();
    if let Err(error) = s.save_window_record(true) {
        eprintln!("Window state: {error}");
    }
}

#[cfg(test)]
#[path = "windows/tests.rs"]
mod tests;

#[derive(Clone)]
pub(super) struct WindowSource {
    pub connection: Connection,
}
impl Viewer {
    pub(super) fn has_pinned_window_default(&self) -> bool {
        self.window.pinned.is_some()
            || self
                .window
                .lease
                .as_ref()
                .is_some_and(|lease| lease.record().kind == BootstrapKind::ProfilePinned)
    }
    pub(super) fn is_primary_window(&self) -> bool {
        self.window
            .lease
            .as_ref()
            .is_some_and(|lease| lease.record().kind == BootstrapKind::PrimaryLaunch)
    }
    pub(super) fn legacy_window_source(&self) -> Option<WindowSource> {
        self.window
            .registry
            .borrow()
            .legacy
            .clone()
            .map(|connection| WindowSource { connection })
    }
    pub(super) fn remember_legacy_window_source(&mut self) {
        if !self.is_primary_window()
            || matches!(self.connection, Some(Connection::Saved { .. }) | None)
        {
            return;
        }
        let mut registry = self.window.registry.borrow_mut();
        if registry.legacy.is_none() {
            registry.legacy = self.connection.clone();
        }
    }
    pub(super) fn window_route_resolver(
        &self,
        id: &str,
        revision: &str,
    ) -> Option<crate::connections::Resolver> {
        self.window
            .retained
            .as_ref()
            .filter(|r| {
                r.lease.is_valid() && r.lease.source_id() == id && r.lease.revision() == revision
            })
            .map(|r| r._resolver.clone())
            .or_else(|| self.connections.resolver())
    }
    pub(super) fn retain_window_route(
        &mut self,
        resolver: crate::connections::Resolver,
        lease: crate::connections::Lease,
    ) {
        if !lease.is_valid()
            || self
                .connection
                .as_ref()
                .and_then(|c| crate::client::connection_key(c).ok())
                != crate::client::connection_key(lease.connection()).ok()
        {
            self.client_error = Some("The activated source lease is no longer valid".into());
            return;
        }
        self.window.retained = Some(Retained {
            _resolver: resolver,
            lease,
        });
    }
    pub fn window_source_valid(&self) -> bool {
        if self.fixture || !matches!(self.connection, Some(Connection::Saved { .. })) {
            return true;
        }
        self.window
            .retained
            .as_ref()
            .is_some_and(|retained| retained.lease.is_valid())
    }
}
