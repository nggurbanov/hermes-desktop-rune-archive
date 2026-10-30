//! Fleet facts and source-local workspaces do not follow the foreground descriptor.
use super::*;
use crate::{
    fleet::{Descriptor, Reason, Snapshot},
    source::{Binding, SourceId, SourceStamp, SourceVersion},
};
use std::collections::BTreeMap;
#[path = "fleet/chat.rs"]
mod chat;
#[path = "fleet/navigation.rs"]
mod navigation;
#[path = "fleet/sidebar.rs"]
mod sidebar;
#[path = "fleet/tabs.rs"]
mod tabs;
#[path = "fleet/workspaces.rs"]
mod workspaces;

#[derive(Default)]
pub(super) struct State {
    waker: Option<Waker>,
    registry: String,
    seeded: Option<(SourceStamp, Vec<String>)>,
    visible: bool,
    roster_enabled: bool,
    snapshot: u64,
    partitions: BTreeMap<String, crate::sidebar::fleet::SourceSidebar>,
    destination: Option<navigation::Waiting>,
    workspaces: BTreeMap<String, workspaces::Workspace>,
}
#[derive(Clone)]
pub(super) enum Destination {
    ProfilePick,
    Bot,
    Session {
        id: String,
        title: String,
        read_only: Option<bool>,
    },
    All,
    Manage(crate::view::fleet::ManageAction),
    Cron(String),
}
impl Viewer {
    pub(super) fn setup_fleet(&mut self, waker: Waker) {
        self.fleet_state.waker = Some(waker.clone());
        match crate::client::fleet::Preferences::system() {
            Ok(value) => self.fleet_preferences = value,
            Err(error) => {
                self.client_error = Some(error.clone());
                self.fleet_preferences = crate::client::fleet::Preferences::unavailable(error);
            }
        }
        if let Some(resolver) = self.connections.resolver() {
            self.fleet.setup(waker, resolver);
            self.refresh_passive_fleet(Reason::Mount);
        }
    }
    pub(in crate::app) fn inspect_fleet_sources(&self) -> serde_json::Value {
        let sidebar = |state: &crate::sidebar::SidebarState, stamp: Option<&SourceStamp>| {
            serde_json::json!({"source":stamp,"profile":state.owner_profile,"retired":state.retired,
                "rename":state.rename.is_some(),"menu":state.menu.is_some(),"busy":state.mutation_inflight,
                "notice":state.notice,"errors":state.errors,
                "rows":state.all_rows().iter().map(|row|serde_json::json!({
                    "key":row.key(),"profile":row.profile(),"id":row.id,"root":row.root_id(),
                    "title":row.title(),"status":state.status(row).label(),"pinned":row.pinned,"unread":row.unread
                })).collect::<Vec<_>>()})
        };
        let active = self
            .connection
            .as_ref()
            .and_then(|c| SourceStamp::from_connection(c).ok());
        let mut groups = vec![sidebar(&self.sidebar_state, active.as_ref())];
        groups.extend(
            self.fleet_state
                .partitions
                .values()
                .map(|p| sidebar(p.state(), Some(p.stamp()))),
        );
        serde_json::json!({"revision":self.fleet_snapshot().revision,"refreshing":self.fleet_snapshot().refreshing,
            "error":self.fleet_snapshot().error,
            "sources":self.fleet_snapshot().sources.iter().map(|s|serde_json::json!({
                "version":s.descriptor.version,"label":s.descriptor.label,"kind":format!("{:?}",s.descriptor.kind),
                "status":format!("{:?}",s.status),"evidence":format!("{:?}",s.evidence),"install_id":s.install_id,
                "principal_stamp":s.principal_stamp,"profiles":s.profiles.iter().map(|p|p.name.clone()).collect::<Vec<_>>(),
                "inventory_error":s.inventory_error
            })).collect::<Vec<_>>(),
            "sidebar_groups":groups
        })
    }
    pub fn fleet_snapshot(&self) -> &Snapshot {
        self.fleet.snapshot()
    }
    pub fn fleet_active_version(&self) -> Option<SourceVersion> {
        self.connection
            .as_ref()
            .and_then(|c| SourceStamp::from_connection(c).ok())
            .map(|s| s.version().clone())
    }
    /// Automatic inventory cannot escape a failed exact startup route.
    pub(super) fn refresh_passive_fleet(&mut self, reason: Reason) {
        self.observe_fleet_connection();
        if self.fleet_state.roster_enabled {
            self.fleet.refresh(reason);
        }
    }
    fn observe_fleet_connection(&mut self) {
        if self.connection.is_some() && self.status == "Connected" && self.error.is_none() {
            self.fleet_state.roster_enabled = true;
        }
    }
    pub fn fleet_refresh(&mut self) {
        if !self.fixture {
            self.fleet_state.roster_enabled = true;
            self.connections.observe_registry();
            self.fleet.refresh(Reason::Explicit);
        }
    }
    pub fn fleet_profile_context(
        &mut self,
        version: SourceVersion,
        profile: String,
        point: rune::PointerPress,
    ) {
        self.close_profile_route_menu();
        self.row_menu = None;
        self.fleet_ui.open_profile(version, profile, point);
    }
    fn fleet_version_exists(&self, version: &SourceVersion) -> bool {
        match &version.id {
            SourceId::Saved(id) => self
                .connections
                .rows
                .iter()
                .any(|r| &r.id == id && r.revision == version.revision),
            SourceId::Direct(_) => {
                self.fleet_active_version().as_ref() == Some(version)
                    || self.fleet.binding(version).is_some()
            }
            SourceId::Legacy => false,
        }
    }
    pub fn fleet_is_default(&self, version: &SourceVersion, profile: &str) -> bool {
        let Some(route) = self.default_profile_route() else {
            return false;
        };
        route.profile == profile
            && match &version.id {
                SourceId::Saved(id) => {
                    route.connection_id.as_ref() == Some(id)
                        || (route.connection_id.as_deref() == Some("local")
                            && self
                                .connections
                                .rows
                                .iter()
                                .find(|r| r.kind() == crate::connections::Kind::Local)
                                .is_some_and(|r| &r.id == id))
                }
                SourceId::Legacy => route.connection_id.is_none(),
                SourceId::Direct(_) => {
                    self.fleet_active_version().as_ref() == Some(version)
                        && self.is_default_profile(profile)
                }
            }
    }
    pub fn fleet_can_set_default(&self, version: &SourceVersion, profile: &str) -> bool {
        !self.fixture
            && self.fleet_version_exists(version)
            && match version.id {
                SourceId::Saved(_) => self.fleet_snapshot().sources.iter().any(|s| {
                    &s.descriptor.version == version && s.profiles.iter().any(|p| p.name == profile)
                }),
                _ => self.can_set_default_profile(profile),
            }
    }
    pub fn fleet_set_default(&mut self, version: SourceVersion, profile: String) {
        self.fleet_ui.close();
        if !self.fleet_can_set_default(&version, &profile) {
            self.client_error =
                Some("The clicked profile's source changed or is no longer available".into());
            return;
        }
        if let SourceId::Saved(id) = version.id {
            let result = (|| {
                let rows = crate::connections::plugin::routes()?;
                if !rows
                    .iter()
                    .any(|r| r.id == id && r.revision == version.revision)
                {
                    return Err(
                        "The saved source changed; refresh before setting its default".into(),
                    );
                }
                let route = crate::client::startup::Route::new(Some(id), profile.clone())?;
                self.device_startup
                    .as_mut()
                    .ok_or("The device preference store is unavailable".to_owned())?
                    .set(route)
            })();
            match result {
                Ok(()) => {
                    self.client_notice = Some(format!("New conversations will use {profile}"))
                }
                Err(e) => self.client_error = Some(e),
            }
        } else {
            self.set_default_profile(profile);
        }
    }
    pub fn fleet_open_window(&mut self, version: SourceVersion, profile: String) {
        self.fleet_ui.close();
        if !self.fleet_version_exists(&version) {
            self.client_error = Some("The clicked source changed; refresh its profiles".into());
            return;
        }
        if let Err(error) = self.open_bound_profile_window(version, profile) {
            self.client_error = Some(error);
        }
    }
    pub fn fleet_all_profiles_active(&self) -> bool {
        self.sidebar_state.prefs.all_profiles
    }
    pub fn fleet_show_all_profiles(&mut self) {
        if !self.sidebar_state.prefs.all_profiles {
            self.sidebar_action(crate::sidebar::Action::Toggle("all-profiles"));
        }
    }
    pub fn fleet_show_source_all(&mut self, version: SourceVersion) {
        let profile = self.fleet_source_profile(&version);
        self.begin_fleet_navigation(version, profile, Destination::All);
    }
    pub fn fleet_manage_source(
        &mut self,
        version: SourceVersion,
        action: crate::view::fleet::ManageAction,
    ) {
        let profile = self.fleet_source_profile(&version);
        self.begin_fleet_navigation(version, profile, Destination::Manage(action));
    }
    pub(super) fn poll_fleet(&mut self, now: f64, tx: &mut Tx) -> bool {
        let mut changed = self.fleet_ui.poll();
        if let Some(delay) = self.fleet_ui.wake_after() {
            tx.wake_after(delay);
        }
        if self.fixture {
            return changed;
        }
        self.observe_fleet_connection();
        let prior_error = self.fleet_preferences.error().map(str::to_owned);
        match self.fleet_preferences.poll_storage() {
            Ok(saved) => {
                changed |= saved;
                if saved && prior_error.is_some() && self.client_error == prior_error {
                    self.client_error = None;
                }
            }
            Err(error) => self.client_error = Some(error),
        }
        if let Some(delay) = self.fleet_preferences.wake_after() {
            tx.wake_after(delay);
        }
        if self.connections.registry_loaded() {
            let descriptors = self
                .connections
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| Descriptor::saved(r, i))
                .collect::<Vec<_>>();
            let signature = format!("{:?}|{:?}", descriptors, self.connections.primary());
            if signature != self.fleet_state.registry {
                self.fleet_state.registry = signature;
                self.fleet.update_registry(
                    descriptors,
                    self.connections
                        .primary()
                        .map(|id| SourceId::Saved(id.into())),
                );
                self.refresh_passive_fleet(Reason::RegistryChanged);
            }
        }
        if self.status == "Connected" && !self.profiles.is_empty() {
            if let Ok(binding) = self.current_source_binding() {
                let fingerprint = self
                    .profiles
                    .iter()
                    .map(|p| {
                        serde_json::json!([
                            p.name,
                            p.label,
                            p.description,
                            p.canonical_session.as_ref().map(|c| (
                                &c.id,
                                &c.resolved_id,
                                &c.preview
                            ))
                        ])
                        .to_string()
                    })
                    .collect::<Vec<_>>();
                let seed = (binding.stamp().clone(), fingerprint);
                if self.fleet_state.seeded.as_ref() != Some(&seed) {
                    self.fleet_state.seeded = Some(seed);
                    let resolver =
                        binding
                            .resolver()
                            .cloned()
                            .or_else(|| match binding.connection() {
                                Connection::Saved { id, revision, .. } => self
                                    .window_route_resolver(id, revision)
                                    .or_else(|| self.fleet.resolver(binding.stamp().version())),
                                _ => None,
                            });
                    let result = match resolver {
                        Some(resolver) => {
                            self.fleet
                                .seed_current_from(binding, &self.profiles, resolver)
                        }
                        None => self.fleet.seed_current(binding, &self.profiles),
                    };
                    if let Err(error) = result {
                        self.client_error = Some(error);
                    }
                    self.fleet.refresh(Reason::GatewayOpened);
                }
            }
        }
        let visible = self.sidebar.is_open() || self.profile_picker;
        if visible && !self.fleet_state.visible {
            self.refresh_passive_fleet(Reason::Visible);
        }
        self.fleet_state.visible = visible;
        changed |= self.fleet.poll();
        if self.fleet_state.roster_enabled
            && self.fleet_state.snapshot != self.fleet.snapshot().revision
        {
            self.fleet_state.snapshot = self.fleet.snapshot().revision;
            self.sync_fleet_partitions();
            changed = true;
        }
        for partition in self.fleet_state.partitions.values_mut() {
            changed |= partition.poll(now);
        }
        changed |= self.poll_fleet_destination();
        changed |= self.retire_expired_workspaces();
        if let Some(delay) = self.fleet.wake() {
            tx.wake_after(delay);
        }
        changed
    }
}
