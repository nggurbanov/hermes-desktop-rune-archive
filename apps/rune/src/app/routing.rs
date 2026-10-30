//! Device defaults choose future workspace intents; live mirrors never navigate an existing one.
use super::*;
use crate::{
    client::startup::Route,
    composer::EditorId,
    connections::{Lease, Resolver, RouteTicket},
};
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "routing/tests.rs"]
mod tests;

#[derive(Default)]
pub(super) struct State {
    pending: Option<Pending>,
    sequence: u64,
    initial_considered: bool,
    waiting_initial: bool,
    next_observe: Option<Instant>,
    notice: Option<String>,
    preference_error: Option<String>,
    pub menu: Option<ProfileMenu>,
}
pub(crate) struct ProfileMenu {
    pub point: rune::PointerPress,
    pub profile: String,
    pub principal: String,
}
#[derive(Clone, PartialEq)]
pub(super) struct Owner {
    scope: DraftScope,
    editor: EditorId,
    page: crate::pages::Route,
    intent: u64,
    input: (String, Vec<(PathBuf, String)>),
}
#[derive(Clone, Copy, PartialEq)]
enum Reason {
    NewSession,
    Initial,
}
struct Pending {
    id: u64,
    target: Route,
    owner: Owner,
    reason: Reason,
    stage: Stage,
}
enum Stage {
    Registry,
    Resolve {
        receive: Receiver<Result<Lease, String>>,
        resolver: Resolver,
    },
    Commit {
        ticket: RouteTicket,
        lease: Lease,
        resolver: Resolver,
    },
    Direct(Receiver<Result<Connection, String>>),
}
impl Viewer {
    pub(super) fn route_owner(&self) -> Owner {
        let scope = self.composer_scope();
        let draft = self.composer.draft(&scope);
        Owner {
            scope,
            editor: self.composer.editor_id().clone(),
            page: self.route,
            intent: self.intent,
            input: (
                draft.map(|d| d.text.text.clone()).unwrap_or_default(),
                draft
                    .map(|d| {
                        d.attachments
                            .iter()
                            .map(|a| (a.path.clone(), a.name.clone()))
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
        }
    }
    pub(crate) fn cancel_device_route(&mut self) {
        self.cancel_window_fresh();
        self.routing.pending = None;
        self.routing.waiting_initial = false;
        self.routing.initial_considered = true;
        self.routing.menu = None;
        if self.client_notice == self.routing.notice {
            self.client_notice = None;
        }
        self.routing.notice = None;
    }
    pub fn device_route_pending(&self) -> bool {
        self.routing.pending.is_some() || self.routing.waiting_initial
    }
    pub fn default_profile_route(&self) -> Option<Route> {
        self.device_startup
            .as_ref()
            .and_then(|store| store.get().ok())
            .flatten()
    }
    pub fn device_default_error(&self) -> Option<&str> {
        match &self.device_startup {
            Some(store) => store.error(),
            None if !self.fixture => Some("The device preference store is unavailable"),
            None => None,
        }
    }
    pub fn clear_default_profile(&mut self) {
        if self.fixture {
            return;
        }
        self.routing.menu = None;
        match self
            .device_startup
            .as_mut()
            .ok_or("The device preference store is unavailable".to_owned())
            .and_then(|store| store.clear())
        {
            Ok(()) => {
                self.client_notice = Some("New conversations will use the current workspace".into())
            }
            Err(error) => self.client_error = Some(error),
        }
    }
    fn current_logical_route(&self, profile: &str) -> Result<Route, String> {
        if !self.profiles.iter().any(|p| p.name == profile) {
            return Err("This profile is no longer in the connected agent roster".into());
        }
        let connection = self.connection.as_ref().ok_or("Connect an agent first")?;
        let source = match connection {
            Connection::Saved { id, revision, .. } => {
                let rows = crate::connections::plugin::routes()?;
                if !rows.iter().any(|r| &r.id == id && &r.revision == revision) {
                    return Err(
                        "This saved source was changed or removed; refresh its connection".into(),
                    );
                }
                Some(id.clone())
            }
            _ => {
                let legacy = self
                    .legacy_window_source()
                    .ok_or("Save this connection before choosing a device default")?;
                if crate::client::connection_key(&legacy.connection)?
                    != crate::client::connection_key(connection)?
                {
                    return Err("Save this connection before choosing a device default".into());
                }
                None
            }
        };
        Route::new(source, profile.to_owned())
    }
    pub fn is_default_profile(&self, profile: &str) -> bool {
        let Some(route) = self.default_profile_route() else {
            return false;
        };
        if route.profile != profile {
            return false;
        }
        match (&route.connection_id, &self.connection) {
            (Some(id), Some(Connection::Saved { id: current, .. })) => {
                id == current
                    || (id == "local"
                        && self
                            .connections
                            .rows
                            .iter()
                            .find(|r| r.kind() == crate::connections::Kind::Local)
                            .is_some_and(|r| &r.id == current))
            }
            (None, Some(connection)) => self.legacy_window_source().is_some_and(|legacy| {
                crate::client::connection_key(&legacy.connection).ok()
                    == crate::client::connection_key(connection).ok()
            }),
            _ => false,
        }
    }
    pub fn open_profile_route_menu(&mut self, profile: String, point: rune::PointerPress) {
        let Some(principal) = self
            .connection
            .as_ref()
            .and_then(|c| crate::client::connection_key(c).ok())
        else {
            return;
        };
        self.routing.menu = Some(ProfileMenu {
            point,
            profile,
            principal,
        });
        self.row_menu = None;
    }
    pub fn profile_route_menu(&self) -> Option<&ProfileMenu> {
        self.routing.menu.as_ref()
    }
    pub fn close_profile_route_menu(&mut self) {
        self.routing.menu = None;
    }
    pub fn can_set_default_profile(&self, profile: &str) -> bool {
        !self.fixture
            && self.profiles.iter().any(|p| p.name == profile)
            && match &self.connection {
                Some(Connection::Saved { id, revision, .. }) => self
                    .connections
                    .rows
                    .iter()
                    .any(|r| r.id == *id && r.revision == *revision),
                Some(connection) => self.legacy_window_source().is_some_and(|legacy| {
                    crate::client::connection_key(&legacy.connection).ok()
                        == crate::client::connection_key(connection).ok()
                }),
                None => false,
            }
    }
    fn profile_menu_target(&mut self) -> Option<String> {
        let menu = self.routing.menu.take()?;
        let actual = self
            .connection
            .as_ref()
            .and_then(|c| crate::client::connection_key(c).ok());
        (actual.as_deref() == Some(&menu.principal)).then_some(menu.profile)
    }
    pub fn set_default_from_profile_menu(&mut self) {
        if let Some(profile) = self.profile_menu_target() {
            self.set_default_profile(profile);
        }
    }
    pub fn open_window_from_profile_menu(&mut self) {
        if let Some(profile) = self.profile_menu_target() {
            self.request_window_target(Some(ConversationTarget::Bot(profile)));
        }
    }
    pub fn set_default_profile(&mut self, profile: String) {
        if self.fixture {
            return;
        }
        self.routing.menu = None;
        let result = self.current_logical_route(&profile).and_then(|route| {
            self.device_startup
                .as_mut()
                .ok_or("The device preference store is unavailable".to_owned())
                .and_then(|store| store.set(route))
        });
        match result {
            Ok(()) => self.client_notice = Some(format!("New conversations will use {profile}")),
            Err(error) => self.client_error = Some(error),
        }
    }
    pub(super) fn remember_default_profile_change(&mut self, old: &str, new: Option<&str>) {
        if self.fixture {
            return;
        }
        let owner = match &self.connection {
            Some(Connection::Saved { id, revision, .. }) => {
                let current=crate::connections::plugin::routes().and_then(|rows| {
                    rows.iter().any(|row| &row.id==id && &row.revision==revision).then_some(()).ok_or("The changed profile's source was replaced; its device default was retained".into())
                });
                if let Err(error) = current {
                    self.client_error = Some(error);
                    return;
                }
                self.connections
                    .profile_changed(id, revision, old, new.map(str::to_owned));
                Some(Some(id.clone()))
            }
            Some(connection) => self
                .legacy_window_source()
                .filter(|legacy| {
                    crate::client::connection_key(&legacy.connection).ok()
                        == crate::client::connection_key(connection).ok()
                })
                .map(|_| None),
            None => None,
        };
        if let (Some(connection_id), Some(store)) = (owner, self.device_startup.as_mut()) {
            if let Err(error) =
                store.after_profile(connection_id, old.to_owned(), new.map(str::to_owned), true)
            {
                self.client_error = Some(error);
            }
        }
    }
    /// Called only after the foreground worker accepts a source-qualified current reply.
    pub(super) fn remember_connected_profile(&mut self, profile: &str) {
        if !self.fixture && profile == self.profile {
            if let Some(Connection::Saved { id, revision, .. }) = &self.connection {
                self.connections
                    .remember_confirmed_profile(id, revision, profile);
            }
        }
    }
    pub(super) fn begin_device_startup(&mut self) -> bool {
        if self.fixture || self.routing.initial_considered {
            return false;
        }
        self.routing.initial_considered = true;
        if !self.is_primary_window()
            || ["--gateway", "--local-home", "--profile"]
                .iter()
                .any(|arg| argument(arg).is_some())
        {
            return false;
        }
        self.routing.waiting_initial = true;
        self.connection_form = false;
        self.loading = true;
        self.status = "Opening the startup workspace…".into();
        true
    }
    /// Only the generic UI/keyboard New Session door consults the device preference.
    pub(super) fn prepare_device_new_session(&mut self) -> bool {
        if self.fixture {
            return false;
        }
        let route = match self.device_startup.as_ref().map(|store| store.get()) {
            Some(Ok(route)) => route,
            Some(Err(error)) => {
                self.client_error = Some(error);
                return true;
            }
            None => {
                self.client_error = Some("The device preference store is unavailable".into());
                return true;
            }
        };
        let Some(route) = route else { return false };
        self.cancel_device_route();
        if let Err(error) = self.queue_default_route(route, Reason::NewSession) {
            self.client_error = Some(error);
        }
        true
    }
    fn queue_default_route(&mut self, target: Route, reason: Reason) -> Result<(), String> {
        if !self.allow_owner_change("") {
            return Err("Finish the pending workspace change before opening this profile".into());
        }
        let stage = if target.connection_id.is_some() {
            self.resolve_saved_default(&target)?
        } else {
            let source=self.legacy_window_source().ok_or("The legacy profile source is unavailable. Choose its connection before opening this default")?;
            let profile = target.profile.clone();
            let (send, receive) = mpsc::channel();
            std::thread::spawn(move || {
                let result = Gateway::connect(&source.connection).and_then(|mut gateway| {
                    if !gateway.profiles()?.iter().any(|p| p.name == profile) {
                        return Err("The default profile is no longer served by its source".into());
                    }
                    Ok(source.connection)
                });
                let _ = send.send(result);
            });
            Stage::Direct(receive)
        };
        self.routing.sequence = self.routing.sequence.wrapping_add(1);
        let pending = Pending {
            id: self.routing.sequence,
            target,
            owner: self.route_owner(),
            reason,
            stage,
        };
        let notice = format!("Opening {}…", pending.target.profile);
        self.routing.notice = Some(notice.clone());
        self.client_notice = Some(notice);
        self.routing.pending = Some(pending);
        Ok(())
    }
    fn resolve_saved_default(&self, target: &Route) -> Result<Stage, String> {
        if !self.connections.registry_loaded() {
            if !self.connections.busy {
                if let Some(error) = &self.connections.error {
                    return Err(error.clone());
                }
            }
            return Ok(Stage::Registry);
        }
        let id = target
            .connection_id
            .as_ref()
            .ok_or("The target has no saved source")?;
        let row = if id == "local" {
            self.connections
                .rows
                .iter()
                .find(|row| row.kind() == crate::connections::Kind::Local)
        } else {
            self.connections.rows.iter().find(|row| &row.id == id)
        }
        .ok_or("The default profile's saved source is unavailable")?;
        let resolver = self
            .window_route_resolver(&row.id, &row.revision)
            .ok_or("The saved connection service is unavailable")?;
        let receive = resolver.resolve(row.id.clone(), Some(row.revision.clone()))?;
        Ok(Stage::Resolve { receive, resolver })
    }
    pub(super) fn poll_device_routes(&mut self, tx: &mut Tx) -> bool {
        if self.fixture {
            return false;
        }
        let mut changed = false;
        if let Some(store) = &mut self.device_startup {
            match store.poll() {
                Ok(value) => {
                    changed |= value;
                    if store.error().is_none() {
                        if self.client_error == self.routing.preference_error
                            && self.routing.preference_error.is_some()
                        {
                            self.client_error = None;
                            changed = true;
                        }
                        self.routing.preference_error = None;
                    }
                }
                Err(error) => {
                    changed |= self.client_error.as_ref() != Some(&error);
                    self.routing.preference_error = Some(error.clone());
                    self.client_error = Some(error);
                }
            }
            if let Some(delay) = store.wake_after() {
                tx.wake_after(delay);
            }
        }
        if self
            .routing
            .next_observe
            .is_none_or(|at| at <= Instant::now())
            && (self.routing.waiting_initial
                || self.routing.pending.is_some()
                || self.profile_picker
                || self.route == crate::pages::Route::Connections)
        {
            self.connections.observe_registry();
            self.routing.next_observe = Some(Instant::now() + Duration::from_secs(1));
        }
        tx.wake_after(Duration::from_secs(1));
        if self.routing.waiting_initial {
            if !self.connections.registry_loaded() {
                tx.wake_after(Duration::from_millis(40));
                if !self.connections.busy {
                    if let Some(error) = self.connections.error.clone() {
                        self.route_open_failed(error, Reason::Initial);
                    }
                }
                return changed;
            }
            self.routing.waiting_initial = false;
            let target = self
                .device_startup
                .as_ref()
                .ok_or("The device preference store is unavailable".to_owned())
                .and_then(|store| store.get())
                .and_then(|route| match route {
                    Some(route) => Ok(Some(route)),
                    None => self.connections.startup_candidate().map(|value| {
                        value.map(|c| Route {
                            connection_id: Some(c.id),
                            profile: c.profile,
                        })
                    }),
                });
            match target {
                Ok(Some(target)) => {
                    if let Err(error) = self.queue_default_route(target, Reason::Initial) {
                        self.route_open_failed(error, Reason::Initial);
                    }
                }
                Ok(None) => self.start_window_source(),
                Err(error) => self.route_open_failed(error, Reason::Initial),
            }
            changed = true;
        }
        let Some(mut pending) = self.routing.pending.take() else {
            return changed;
        };
        if self.route_owner() != pending.owner {
            self.cancel_device_route();
            return true;
        }
        if !matches!(pending.stage, Stage::Registry) && !self.allow_owner_change("") {
            self.route_open_failed(
                "The workspace changed while its target was opening".into(),
                pending.reason,
            );
            return true;
        }
        let next = match pending.stage {
            Stage::Registry => self.resolve_saved_default(&pending.target),
            Stage::Resolve { receive, resolver } => match receive.try_recv() {
                Ok(Ok(lease)) => {
                    if !lease.is_valid()
                        || !lease
                            .profiles()
                            .iter()
                            .any(|p| p.name == pending.target.profile)
                    {
                        Err("The requested profile is no longer served by its source".into())
                    } else {
                        self.connections
                            .commit_route(lease.clone(), pending.target.profile.clone(), pending.id)
                            .map(|ticket| Stage::Commit {
                                ticket,
                                lease,
                                resolver,
                            })
                    }
                }
                Ok(Err(error)) => Err(error),
                Err(mpsc::TryRecvError::Empty) => Ok(Stage::Resolve { receive, resolver }),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("The saved source service stopped".into())
                }
            },
            Stage::Commit {
                ticket,
                lease,
                resolver,
            } => match ticket.receiver.try_recv() {
                Ok(receipt) => {
                    if receipt.intent != pending.id || receipt.profile != pending.target.profile {
                        Err("The source returned a receipt for another workspace intent".into())
                    } else {
                        match receipt.result {
                            Ok(connection) => {
                                self.accept_device_route(
                                    connection,
                                    Some((resolver, lease)),
                                    pending.target.profile,
                                    pending.reason,
                                );
                                return true;
                            }
                            Err(error) => Err(error),
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => Ok(Stage::Commit {
                    ticket,
                    lease,
                    resolver,
                }),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("The route activation service stopped".into())
                }
            },
            Stage::Direct(receive) => match receive.try_recv() {
                Ok(Ok(connection)) => {
                    self.accept_device_route(
                        connection,
                        None,
                        pending.target.profile,
                        pending.reason,
                    );
                    return true;
                }
                Ok(Err(error)) => Err(error),
                Err(mpsc::TryRecvError::Empty) => Ok(Stage::Direct(receive)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("The profile connection check stopped".into())
                }
            },
        };
        match next {
            Ok(stage) => {
                pending.stage = stage;
                self.routing.pending = Some(pending);
                tx.wake_after(Duration::from_millis(40));
            }
            Err(error) => self.route_open_failed(error, pending.reason),
        }
        true
    }
    fn accept_device_route(
        &mut self,
        connection: Connection,
        retained: Option<(Resolver, Lease)>,
        profile: String,
        reason: Reason,
    ) {
        if !self.allow_owner_change("") {
            self.route_open_failed(
                "The workspace changed while its target was opening".into(),
                reason,
            );
            return;
        }
        if retained
            .as_ref()
            .is_some_and(|(_, lease)| !lease.is_valid())
        {
            self.route_open_failed(
                "The requested source expired before activation".into(),
                reason,
            );
            return;
        }
        let same = self
            .connection
            .as_ref()
            .and_then(|c| crate::client::connection_key(c).ok())
            == crate::client::connection_key(&connection).ok();
        let outgoing = self.connection.as_ref().and_then(|c| {
            if let Connection::Saved { id, revision, .. } = c {
                Some((id.clone(), revision.clone()))
            } else {
                None
            }
        });
        self.cancel_device_route();
        if same {
            self.select_profile(profile.clone());
            if self.profile != profile {
                return;
            }
        } else if !self.use_connection_for_profile(connection, Some(profile)) {
            return;
        }
        if let Some((resolver, lease)) = retained {
            self.retain_window_route(resolver, lease);
        }
        self.connections
            .note_active_source(self.connection.as_ref());
        if !same {
            if let Some((id, revision)) = outgoing {
                self.connections.release_foreground(&id, &revision);
            }
        }
        self.new_draft_on_current_source();
    }
    fn route_open_failed(&mut self, error: String, reason: Reason) {
        self.cancel_device_route();
        self.client_error = Some(error);
        if reason == Reason::Initial {
            self.loading = false;
            self.connection_form = true;
            self.status = "Choose a connection".into();
        }
    }
}
