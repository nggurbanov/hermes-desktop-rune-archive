//! Sidebar slices, view preferences and session edits keep their own owner.
use crate::{
    gateway::sidebar::{SidebarSession, parse_sessions, parse_sessions_owned},
    pages::{PageEffect, PageRequest, segment},
    source::{SessionOwner, SourceStamp},
};
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
pub mod fleet;
#[cfg(test)]
mod owned_tests;
mod persistence;
mod receipt;
#[cfg(test)]
mod tests;
use rune::TextEdit;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const LOCAL_SOURCES: &str = "acp,cli,codex,desktop,gateway,kanban,local,oneshot,tui";
const MESSAGING_SOURCES: &str = "telegram,discord,slack,mattermost,matrix,signal,whatsapp,bluebubbles,photon,homeassistant,email,sms,webhook,api_server,weixin,wecom,qqbot,yuanbao,dingtalk,feishu";
#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    #[default]
    Date,
    Profile,
    Project,
    Status,
    None,
}
impl Grouping {
    pub const ALL: [Self; 5] = [
        Self::Date,
        Self::Profile,
        Self::Project,
        Self::Status,
        Self::None,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Date => "Updated",
            Self::Profile => "Profile",
            Self::Project => "Project",
            Self::Status => "Status",
            Self::None => "None",
        }
    }
}
#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Ordering {
    #[default]
    Updated,
    Created,
    Status,
    Tokens,
    Cost,
}
impl Ordering {
    pub const ALL: [Self; 5] = [
        Self::Updated,
        Self::Created,
        Self::Status,
        Self::Tokens,
        Self::Cost,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Updated => "Updated",
            Self::Created => "Created",
            Self::Status => "Status",
            Self::Tokens => "Tokens",
            Self::Cost => "Cost",
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    NeedsInput,
    Working,
    Unread,
    Draft,
    #[default]
    Idle,
}
impl Status {
    pub const ALL: [Self; 5] = [
        Self::NeedsInput,
        Self::Working,
        Self::Unread,
        Self::Draft,
        Self::Idle,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::NeedsInput => "Needs input",
            Self::Working => "Working",
            Self::Unread => "Unread",
            Self::Draft => "Draft",
            Self::Idle => "Idle",
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
    pub grouping: Grouping,
    pub ordering: Ordering,
    pub collapsed: BTreeSet<String>,
    pub all_profiles: bool,
    pub archived: bool,
    pub cards: bool,
    pub preview: bool,
    pub tokens: bool,
    pub cost: bool,
    pub profile: bool,
    pub rail: bool,
    pub status: BTreeSet<String>,
    pub profiles: BTreeSet<String>,
    pub projects: BTreeSet<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            other: BTreeMap::new(),
            grouping: Grouping::Date,
            ordering: Ordering::Updated,
            collapsed: BTreeSet::new(),
            all_profiles: false,
            archived: false,
            cards: false,
            preview: true,
            tokens: false,
            cost: false,
            profile: false,
            rail: true,
            status: BTreeSet::new(),
            profiles: BTreeSet::new(),
            projects: BTreeSet::new(),
        }
    }
}
#[derive(Default)]
pub struct Slice {
    pub rows: Vec<SidebarSession>,
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub truncated: BTreeMap<String, bool>,
}
#[derive(Clone)]
pub enum Action {
    Refresh,
    LoadMore(String),
    Collapse(String),
    Filters,
    Grouping(Grouping),
    Ordering(Ordering),
    Toggle(&'static str),
    Status(String),
    Profile(String),
    Project(String),
    ResetView,
    CollapseAll,
    MarkAllRead,
    MoreJobs,
    Open(String, String),
    Pin(String, String, bool),
    Unread(String, String, bool),
    Archive(String, String, bool),
    Rename(String, String),
    Delete(String, String),
    SaveRename,
    Confirm,
    Cancel,
    Context {
        profile: String,
        id: String,
        point: rune::PointerPress,
    },
    CloseMenu,
    RetryMutation,
    PeekJob(String),
    ManageJob(String),
    TriggerJob(String),
    PauseJob(String),
    DeleteJob(String),
    OpenRun(usize),
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Mutation {
    pub session: SidebarSession,
    pub payload: Value,
    pub delete: bool,
    pub identity: Option<Value>,
    pub owner_connection: u64,
    #[serde(default)]
    pub source: Option<SourceStamp>,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
}
#[derive(Clone, Serialize, Deserialize)]
struct SavedReceipt {
    #[serde(default)]
    version: u32,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
    owner: String,
    mutation: Mutation,
}
#[derive(Clone)]
pub enum Confirmation {
    Session(Mutation),
    Job(String),
}
pub struct SidebarState {
    pub prefs: Preferences,
    preferences_storage: persistence::Storage,
    pub owner_profile: String,
    pub owner_connection: u64,
    pub demo: bool,
    pub retired: bool,
    pub recents: Slice,
    pub messaging: Slice,
    pub cron_sessions: Slice,
    pub jobs: Vec<crate::pages::cron::Job>,
    pub jobs_error: Option<String>,
    pub jobs_loaded: bool,
    pub jobs_visible: usize,
    pub runs: Vec<crate::pages::cron::Run>,
    pub peek_job: Option<String>,
    pub filter_open: bool,
    pub open_select: Option<String>,
    pub menu: Option<(String, String, rune::PointerPress)>,
    pub live: HashMap<(String, String), Status>,
    pub errors: BTreeMap<String, String>,
    pub pending: HashSet<String>,
    pub notice: Option<String>,
    pub storage: BTreeMap<String, String>,
    pub confirm: Option<Confirmation>,
    pub rename: Option<SidebarSession>,
    pub title: TextEdit,
    pub mutation: Option<Mutation>,
    pub mutation_inflight: bool,
    pub jobs_busy: BTreeSet<String>,
    pub next_poll: f64,
    pub now: f64,
    limit: usize,
    messaging_limit: usize,
    cron_limit: usize,
    batch_missing: bool,
    live_loaded: bool,
    clock: f64,
    connection_key: String,
    recovery: Option<SavedReceipt>,
    recovery_loaded: bool,
    unread_queue: Vec<SidebarSession>,
    source: Option<SourceStamp>,
    receipt_store: Option<receipt::Store>,
    partition_preferences: bool,
    canonical_profiles: BTreeMap<String, BTreeSet<String>>,
}
impl Default for SidebarState {
    fn default() -> Self {
        Self::new()
    }
}
impl SidebarState {
    pub fn new() -> Self {
        Self {
            prefs: Preferences::default(),
            preferences_storage: Default::default(),
            owner_profile: "default".into(),
            owner_connection: 0,
            demo: false,
            retired: false,
            recents: Slice::default(),
            messaging: Slice::default(),
            cron_sessions: Slice::default(),
            jobs: vec![],
            jobs_error: None,
            jobs_loaded: false,
            jobs_visible: 3,
            runs: vec![],
            peek_job: None,
            filter_open: false,
            open_select: None,
            menu: None,
            live: HashMap::new(),
            errors: BTreeMap::new(),
            pending: HashSet::new(),
            notice: None,
            storage: BTreeMap::new(),
            confirm: None,
            rename: None,
            title: TextEdit::new(""),
            mutation: None,
            mutation_inflight: false,
            jobs_busy: BTreeSet::new(),
            next_poll: 0.0,
            now: 0.0,
            limit: 40,
            messaging_limit: 20,
            cron_limit: 20,
            batch_missing: false,
            live_loaded: false,
            clock: 0.0,
            connection_key: String::new(),
            recovery: None,
            recovery_loaded: false,
            unread_queue: vec![],
            source: None,
            receipt_store: None,
            partition_preferences: false,
            canonical_profiles: BTreeMap::new(),
        }
    }
    pub fn blocks_owner_change(&self) -> bool {
        self.mutation.is_some() || self.mutation_inflight || !self.jobs_busy.is_empty()
    }
    pub fn bind_connection(&mut self, connection: &crate::gateway::Connection, epoch: u64) {
        let directory = match crate::client::state_dir() {
            Ok(directory) => directory,
            Err(error) => {
                self.notice = Some(error);
                return;
            }
        };
        self.bind_connection_at(connection, epoch, directory);
    }
    pub fn bind_connection_at(
        &mut self,
        connection: &crate::gateway::Connection,
        epoch: u64,
        directory: std::path::PathBuf,
    ) {
        let source = match crate::source::SourceStamp::from_connection(connection) {
            Ok(source) => source,
            Err(error) => {
                self.notice = Some(error);
                return;
            }
        };
        self.bind_stamp_at(connection, source, epoch, directory);
    }
    pub fn bind_source_at(
        &mut self,
        binding: &crate::source::Binding,
        epoch: u64,
        directory: std::path::PathBuf,
    ) -> Result<(), String> {
        binding.validate()?;
        self.bind_stamp_at(
            binding.connection(),
            binding.stamp().clone(),
            epoch,
            directory,
        );
        if self.source.as_ref() != Some(binding.stamp()) {
            return Err("The sidebar source could not be rebound while an edit is pending".into());
        }
        Ok(())
    }
    fn bind_stamp_at(
        &mut self,
        connection: &crate::gateway::Connection,
        source: SourceStamp,
        epoch: u64,
        directory: std::path::PathBuf,
    ) {
        let key = match crate::client::connection_key(connection) {
            Ok(key) => key,
            Err(error) => {
                self.notice = Some(error);
                return;
            }
        };
        if self.connection_key != key {
            if self.mutation.is_some() {
                self.notice = Some("Reconcile the pending edit on its original source before rebinding the sidebar".into());
                return;
            }
            self.reset_data();
            self.connection_key = key.clone();
            self.recovery_loaded = false;
            self.receipt_store = None;
            self.recovery = None;
        }
        self.owner_connection = epoch;
        self.source = Some(source);
        if !self.recovery_loaded && !self.demo {
            self.recovery_loaded = true;
            let result = self
                .source
                .as_ref()
                .ok_or("The source identity is unavailable".to_owned())
                .and_then(|source| {
                    let mut store = receipt::Store::new(directory, source.clone());
                    let loaded = store.load();
                    self.receipt_store = Some(store);
                    loaded
                });
            match result {
                Ok(receipt) => self.recovery = receipt,
                Err(error) => self.notice = Some(error),
            }
        }
        if self.mutation.is_none()
            && let Some(receipt) = &self.recovery
        {
            if receipt.owner == key
                || receipt
                    .mutation
                    .source
                    .as_ref()
                    .zip(self.source.as_ref())
                    .is_some_and(|(old, current)| receipt::can_reconcile(old, current))
            {
                let mut mutation = receipt.mutation.clone();
                mutation.owner_connection = epoch;
                if mutation.source.is_none() {
                    mutation.source = self.source.clone();
                }
                mutation.session.provenance = mutation.source.clone();
                self.mutation = Some(mutation);
                self.notice=Some("A saved session edit needs reconciliation. Retry uses its original identity and preconditions.".into());
            } else {
                self.notice=Some("A session edit is pending on another gateway. Return to its owner to reconcile it before editing sessions.".into());
            }
        }
    }
    fn save_receipt(&mut self) -> Result<(), String> {
        if self.demo {
            return Ok(());
        }
        let mutation = self.mutation.clone().ok_or("No session edit is prepared")?;
        let receipt = SavedReceipt {
            version: 1,
            other: self
                .recovery
                .as_ref()
                .map(|receipt| receipt.other.clone())
                .unwrap_or_default(),
            owner: self.connection_key.clone(),
            mutation,
        };
        let retained = self
            .receipt_store
            .as_mut()
            .ok_or("The source-qualified edit store is unavailable")?
            .save(&receipt)?;
        if !retained {
            self.recovery = None;
            self.mutation = None;
            return Err("Another desktop window already reconciled this edit. Refresh before editing again.".into());
        }
        self.recovery = Some(receipt);
        Ok(())
    }
    fn retire_receipt(&mut self) -> Result<(), String> {
        if !self.demo {
            self.receipt_store
                .as_mut()
                .ok_or("The source-qualified edit store is unavailable")?
                .retire(
                    self.recovery
                        .as_ref()
                        .ok_or("The pending edit receipt is unavailable")?,
                )?;
        }
        self.recovery = None;
        self.mutation = None;
        Ok(())
    }
    pub fn reset_data(&mut self) {
        self.recents = Slice::default();
        self.messaging = Slice::default();
        self.cron_sessions = Slice::default();
        self.jobs.clear();
        self.jobs_loaded = false;
        self.jobs_visible = 3;
        self.jobs_error = None;
        self.runs.clear();
        self.peek_job = None;
        self.live.clear();
        self.live_loaded = false;
        self.pending.clear();
        self.errors.clear();
        self.storage.clear();
        self.confirm = None;
        self.rename = None;
        self.menu = None;
        self.next_poll = 0.0;
        self.limit = 40;
        self.messaging_limit = 20;
        self.cron_limit = 20;
        self.batch_missing = false;
    }
    fn request(&mut self, key: &str, request: PageRequest) -> PageEffect {
        let key = format!("sidebar.{key}");
        self.pending.insert(key.clone());
        self.errors.remove(&key);
        let request = if let Some(claim) = self
            .receipt_store
            .as_ref()
            .filter(|_| matches!(key.as_str(), "sidebar.snapshot" | "sidebar.mutation"))
            .and_then(receipt::Store::worker_claim)
        {
            request.edit_claim(claim)
        } else {
            request
        };
        (
            key,
            if request.profile.is_none() {
                request.profile(&self.owner_profile)
            } else {
                request
            },
        )
    }
    fn selected_profile(&self) -> &str {
        if self.prefs.all_profiles {
            "all"
        } else {
            &self.owner_profile
        }
    }
    pub fn load(&mut self, profile: &str) -> Vec<PageEffect> {
        if self.owner_profile != profile {
            self.reset_data();
            self.owner_profile = profile.into();
        }
        if self.demo {
            return vec![];
        }
        self.next_poll = self.clock + 15.0;
        self.recents.loading = true;
        self.messaging.loading = true;
        self.cron_sessions.loading = true;
        let mut effects = if self.batch_missing || self.prefs.archived {
            self.legacy_slices()
        } else {
            let selected = self.selected_profile().to_owned();
            vec![
                self.request(
                    "batch",
                    PageRequest::get("/api/profiles/sessions/sidebar")
                        .query("recents_profile", selected)
                        .query("recents_limit", self.limit)
                        .query("cron_limit", self.cron_limit)
                        .query("messaging_limit", self.messaging_limit)
                        .query("recents_exclude", format!("cron,{MESSAGING_SOURCES}"))
                        .query("messaging_exclude", format!("cron,{LOCAL_SOURCES}"))
                        .timeout_secs(60),
                ),
            ]
        };
        effects.push(self.request("jobs", PageRequest::get("/api/cron/jobs")));
        effects.extend(self.live_requests());
        effects
    }
    fn legacy_slices(&mut self) -> Vec<PageEffect> {
        let selected = self.selected_profile().to_owned();
        let archived = if self.prefs.archived {
            "include"
        } else {
            "exclude"
        };
        let base = PageRequest::get("/api/profiles/sessions")
            .query("profile", selected)
            .query("min_messages", 1)
            .query("archived", archived)
            .query("order", "recent")
            .timeout_secs(60);
        vec![
            self.request(
                "recents",
                base.clone()
                    .query("limit", self.limit)
                    .query("exclude_sources", format!("cron,{MESSAGING_SOURCES}")),
            ),
            self.request(
                "messaging",
                base.clone()
                    .query("limit", self.messaging_limit)
                    .query("exclude_sources", format!("cron,{LOCAL_SOURCES}")),
            ),
            self.request(
                "cron",
                base.query("limit", self.cron_limit).query("source", "cron"),
            ),
        ]
    }
    fn live_requests(&mut self) -> Vec<PageEffect> {
        let profiles: BTreeSet<String> = if self.prefs.all_profiles {
            self.all_rows()
                .iter()
                .map(|s| s.profile().to_owned())
                .chain([self.owner_profile.clone()])
                .collect()
        } else {
            [self.owner_profile.clone()].into_iter().collect()
        };
        profiles
            .into_iter()
            .filter(|p| !self.pending.contains(&format!("sidebar.live.{p}")))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|p| {
                self.request(
                    &format!("live.{p}"),
                    PageRequest::rpc(
                        "session.list",
                        json!({"scope":"live","limit":200,"include_hidden":false}),
                    )
                    .profile(&p),
                )
            })
            .collect()
    }
    pub fn tick(&mut self, now: f64) -> Vec<PageEffect> {
        self.clock = now;
        self.now = Local::now().timestamp() as f64;
        if self.demo
            || now < self.next_poll
            || self.mutation_inflight
            || !self.jobs_busy.is_empty()
            || self.recents.loading
            || self.messaging.loading
            || self.cron_sessions.loading
        {
            return vec![];
        }
        let profile = self.owner_profile.clone();
        self.load(&profile)
    }
    pub fn all_rows(&self) -> Vec<&SidebarSession> {
        self.recents
            .rows
            .iter()
            .chain(&self.messaging.rows)
            .chain(&self.cron_sessions.rows)
            .collect()
    }
    pub fn status(&self, row: &SidebarSession) -> Status {
        for alias in row
            .aliases()
            .into_iter()
            .filter(|_| row.provenance.as_ref() == self.source.as_ref())
        {
            if let Some(status) = self.live.get(&(row.profile().into(), alias.into())) {
                if *status != Status::Idle {
                    return *status;
                }
            }
        }
        if row.scheduler_owned == Some(true) {
            return Status::Working;
        }
        if row.unread == Some(true) {
            Status::Unread
        } else if row.message_count == 0 {
            Status::Draft
        } else {
            Status::Idle
        }
    }
    pub fn matches(&self, row: &SidebarSession, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        (query.is_empty()
            || format!(
                "{} {} {} {}",
                row.title(),
                row.preview.as_deref().unwrap_or_default(),
                row.profile(),
                source_label(row.source.as_deref().unwrap_or_default())
            )
            .to_lowercase()
            .contains(&query))
            && (!row.archived || self.prefs.archived)
            && (self.prefs.status.is_empty()
                || self.prefs.status.contains(self.status(row).label()))
            && (self.prefs.profiles.is_empty() || self.prefs.profiles.contains(row.profile()))
            && (self.prefs.projects.is_empty() || self.prefs.projects.contains(project(row)))
    }
    pub fn ordered<'a>(&self, mut rows: Vec<&'a SidebarSession>) -> Vec<&'a SidebarSession> {
        rows.sort_by(|a, b| {
            let order = match self.prefs.ordering {
                Ordering::Updated => b.recency().total_cmp(&a.recency()),
                Ordering::Created => b.started_at.total_cmp(&a.started_at),
                Ordering::Status => self.status(a).cmp(&self.status(b)),
                Ordering::Tokens => b.tokens().cmp(&a.tokens()),
                Ordering::Cost => b.cost().total_cmp(&a.cost()),
            };
            order.then_with(|| a.id.cmp(&b.id))
        });
        rows
    }
    pub fn set_canonical_profiles(&mut self, profiles: &[crate::gateway::Profile]) {
        self.canonical_profiles = profiles
            .iter()
            .filter_map(|profile| {
                profile.canonical_session.as_ref().map(|session| {
                    (
                        profile.name.clone(),
                        [session.id.clone(), session.resolved_id.clone()]
                            .into_iter()
                            .collect(),
                    )
                })
            })
            .collect();
    }
    pub fn is_canonical(&self, row: &SidebarSession) -> bool {
        row.hidden
            || row.is_internal_child
            || row.title.as_deref().map(str::trim) == Some("Bot Chat")
            || row.root_title.as_deref().map(str::trim) == Some("Bot Chat")
            || row.provenance.as_ref() == self.source.as_ref()
                && self
                    .canonical_profiles
                    .get(row.profile())
                    .is_some_and(|ids| row.aliases().iter().any(|id| ids.contains(*id)))
    }
    pub fn branch_depth(&self, row: &SidebarSession) -> usize {
        let mut current = row.clone();
        let mut seen = HashSet::new();
        let mut depth = 0;
        while seen.insert(current.key()) {
            let Some(parent) = current.fork_parent() else {
                break;
            };
            let Some(parent) = self.all_rows().into_iter().find(|candidate| {
                candidate.provenance == current.provenance
                    && candidate.profile() == current.profile()
                    && candidate.aliases().contains(&parent)
                    && candidate.key() != current.key()
            }) else {
                break;
            };
            current = parent.clone();
            depth += 1;
        }
        depth
    }
    pub fn group_key(&self, section: &str, kind: &str, value: &str) -> String {
        serde_json::json!([
            "sidebar",
            section,
            self.source.as_ref().map(|source| &source.version().id),
            kind,
            value
        ])
        .to_string()
    }
    pub fn source_stamp(&self) -> Option<&SourceStamp> {
        self.source.as_ref()
    }
    pub fn owner(&self, profile: &str) -> Option<SessionOwner> {
        self.source
            .clone()
            .map(|source| SessionOwner::new(source, profile))
    }
    pub fn find_owned(&self, owner: &SessionOwner, id: &str) -> Option<SidebarSession> {
        if self.source.as_ref() != Some(&owner.source) {
            return None;
        }
        self.find(&owner.profile, id)
    }
    pub fn set_partition_preferences(&mut self, preferences: &Preferences) {
        self.partition_preferences = true;
        self.prefs = preferences.clone();
    }
    pub fn find(&self, profile: &str, id: &str) -> Option<SidebarSession> {
        self.all_rows()
            .into_iter()
            .find(|row| {
                row.provenance.as_ref() == self.source.as_ref()
                    && row.profile() == profile
                    && row.aliases().contains(&id)
            })
            .cloned()
    }
    pub fn action(&mut self, action: Action) -> Vec<PageEffect> {
        match action {
            Action::Refresh => {
                let profile = self.owner_profile.clone();
                self.load(&profile)
            }
            Action::LoadMore(slice) => {
                match slice.as_str() {
                    "messaging" => self.messaging_limit += 20,
                    "cron" => self.cron_limit += 20,
                    _ => self.limit += 40,
                };
                let profile = self.owner_profile.clone();
                self.load(&profile)
            }
            Action::Filters => {
                self.filter_open = !self.filter_open;
                vec![]
            }
            Action::Collapse(id) => {
                toggle(&mut self.prefs.collapsed, id);
                self.save_preferences(&["collapsed"]);
                vec![]
            }
            Action::Grouping(grouping) => {
                self.prefs.grouping = grouping;
                self.save_preferences(&["grouping"]);
                vec![]
            }
            Action::Ordering(ordering) => {
                self.prefs.ordering = ordering;
                self.save_preferences(&["ordering"]);
                vec![]
            }
            Action::Status(status) => {
                toggle(&mut self.prefs.status, status);
                self.save_preferences(&["status"]);
                vec![]
            }
            Action::Profile(profile) => {
                toggle(&mut self.prefs.profiles, profile);
                self.save_preferences(&["profiles"]);
                vec![]
            }
            Action::Project(project) => {
                toggle(&mut self.prefs.projects, project);
                self.save_preferences(&["projects"]);
                vec![]
            }
            Action::Toggle(key) => {
                match key {
                    "all-profiles" => self.prefs.all_profiles = !self.prefs.all_profiles,
                    "archived" => self.prefs.archived = !self.prefs.archived,
                    "cards" => self.prefs.cards = !self.prefs.cards,
                    "preview" => self.prefs.preview = !self.prefs.preview,
                    "tokens" => self.prefs.tokens = !self.prefs.tokens,
                    "cost" => self.prefs.cost = !self.prefs.cost,
                    "profile" => self.prefs.profile = !self.prefs.profile,
                    "rail" => self.prefs.rail = !self.prefs.rail,
                    _ => return vec![],
                }
                self.save_preferences(&[if key == "all-profiles" {
                    "all_profiles"
                } else {
                    key
                }]);
                if matches!(key, "all-profiles" | "archived") {
                    let profile = self.owner_profile.clone();
                    self.load(&profile)
                } else {
                    vec![]
                }
            }
            Action::MoreJobs => {
                self.jobs_visible = (self.jobs_visible + 10).min(50);
                vec![]
            }
            Action::MarkAllRead => {
                if self.demo || self.mutation.is_some() {
                    return vec![];
                }
                let mut seen = HashSet::new();
                self.unread_queue = self
                    .all_rows()
                    .into_iter()
                    .filter(|r| {
                        r.unread == Some(true)
                            && seen.insert(r.key())
                            && (self
                                .source
                                .as_ref()
                                .is_some_and(|source| r.provenance.as_ref() == Some(source))
                                || self.source.is_none() && r.connection_id.is_none())
                    })
                    .cloned()
                    .collect();
                self.next_unread()
            }
            Action::CollapseAll => {
                let rows = self.ordered(self.recents.rows.iter().collect());
                let keys: Vec<String> = match self.prefs.grouping {
                    Grouping::Date => date_rows(rows, self.now)
                        .into_iter()
                        .filter_map(|row| match row {
                            ListRow::Divider(key, _) => {
                                Some(self.group_key("recents", "date", &key))
                            }
                            _ => None,
                        })
                        .collect(),
                    Grouping::None => vec![],
                    group => rows
                        .iter()
                        .map(|row| {
                            self.group_key(
                                "recents",
                                group.label(),
                                &match group {
                                    Grouping::Profile => row.profile().to_owned(),
                                    Grouping::Project => project(row).into(),
                                    _ => self.status(row).label().into(),
                                },
                            )
                        })
                        .collect(),
                };
                let collapse = keys.iter().any(|key| !self.prefs.collapsed.contains(key));
                for key in keys {
                    if collapse {
                        self.prefs.collapsed.insert(key);
                    } else {
                        self.prefs.collapsed.remove(&key);
                    }
                }
                self.save_preferences(&["collapsed"]);
                vec![]
            }
            Action::ResetView => {
                self.prefs = Preferences {
                    other: std::mem::take(&mut self.prefs.other),
                    ..Default::default()
                };
                self.save_preferences(persistence::KNOWN_FIELDS);
                let profile = self.owner_profile.clone();
                self.load(&profile)
            }
            Action::Pin(profile, id, enabled) => {
                self.start_flag(&profile, &id, json!({"pinned":enabled}), false)
            }
            Action::Unread(profile, id, unread) => {
                self.start_flag(&profile, &id, json!({"unread":unread}), false)
            }
            Action::Archive(profile, id, archived) => self.start_flag(
                &profile,
                &id,
                if archived {
                    json!({"archived":true,"pinned":false})
                } else {
                    json!({"archived":false})
                },
                false,
            ),
            Action::Rename(profile, id) => {
                self.rename = self.find(&profile, &id);
                if let Some(row) = &self.rename {
                    self.title.replace_text(row.title());
                }
                vec![]
            }
            Action::SaveRename => {
                let Some(row) = self.rename.clone() else {
                    return vec![];
                };
                let title = self.title.text.trim().to_owned();
                if title.is_empty() {
                    self.notice = Some("A session title cannot be empty".into());
                    return vec![];
                }
                self.rename = None;
                self.start_flag(row.profile(), &row.id, json!({"title":title}), false)
            }
            Action::Delete(profile, id) => {
                if let Some(row) = self.find(&profile, &id) {
                    self.confirm = Some(Confirmation::Session(Mutation {
                        session: row,
                        payload: json!({}),
                        delete: true,
                        identity: None,
                        owner_connection: self.owner_connection,
                        source: self.source.clone(),
                        other: BTreeMap::new(),
                    }));
                }
                vec![]
            }
            Action::Confirm => match self.confirm.take() {
                Some(Confirmation::Session(mutation)) => self.prepare(mutation),
                Some(Confirmation::Job(id)) => self.job_write(&id, "delete"),
                None => vec![],
            },
            Action::Context { profile, id, point } => {
                self.menu = Some((profile, id, point));
                vec![]
            }
            Action::CloseMenu => {
                self.menu = None;
                vec![]
            }
            Action::Cancel => {
                self.confirm = None;
                self.rename = None;
                vec![]
            }
            Action::RetryMutation => {
                if let Some(mut mutation) = self.mutation.clone() {
                    if let (Some(old), Some(current)) = (&mutation.source, &self.source) {
                        if !receipt::can_reconcile(old, current) {
                            self.notice = Some(
                                "This pending edit belongs to a different source version".into(),
                            );
                            return vec![];
                        }
                        mutation.source = Some(current.clone());
                        mutation.session.provenance = Some(current.clone());
                        self.mutation = Some(mutation.clone());
                    }
                    if mutation.owner_connection != self.owner_connection {
                        self.notice = Some(
                            "Return to the gateway that owns this pending edit before retrying it"
                                .into(),
                        );
                        vec![]
                    } else if mutation.identity.is_some() {
                        self.send_mutation()
                    } else {
                        self.mutation = None;
                        self.prepare(mutation)
                    }
                } else {
                    vec![]
                }
            }
            Action::PeekJob(id) => {
                if self.peek_job.as_ref() == Some(&id) {
                    self.peek_job = None;
                    self.runs.clear();
                    vec![]
                } else {
                    self.peek_job = Some(id.clone());
                    self.runs.clear();
                    self.runs_request(&id)
                }
            }
            Action::TriggerJob(id) => self.job_write(&id, "trigger"),
            Action::PauseJob(id) => {
                let verb = self
                    .jobs
                    .iter()
                    .find(|job| job.id == id)
                    .map(|job| {
                        if matches!(job.state(), "paused" | "disabled") {
                            "resume"
                        } else {
                            "pause"
                        }
                    })
                    .unwrap_or("pause");
                self.job_write(&id, verb)
            }
            Action::DeleteJob(id) => {
                self.confirm = Some(Confirmation::Job(id));
                vec![]
            }
            Action::Open(..) | Action::OpenRun(_) | Action::ManageJob(_) => vec![],
        }
    }
    fn next_unread(&mut self) -> Vec<PageEffect> {
        let Some(row) = self.unread_queue.pop() else {
            return vec![];
        };
        self.prepare(Mutation {
            session: row,
            payload: json!({"unread":false}),
            delete: false,
            identity: None,
            owner_connection: self.owner_connection,
            source: self.source.clone(),
            other: BTreeMap::new(),
        })
    }
    fn start_flag(
        &mut self,
        profile: &str,
        id: &str,
        payload: Value,
        delete: bool,
    ) -> Vec<PageEffect> {
        let Some(row) = self.find(profile, id) else {
            return vec![];
        };
        self.prepare(Mutation {
            session: row,
            payload,
            delete,
            identity: None,
            owner_connection: self.owner_connection,
            source: self.source.clone(),
            other: BTreeMap::new(),
        })
    }
    fn prepare(&mut self, mutation: Mutation) -> Vec<PageEffect> {
        if self.demo || self.mutation_inflight {
            return vec![];
        }
        if self.mutation.is_some() || self.recovery.is_some() {
            self.notice =
                Some("Retry the pending session edit before starting another edit".into());
            return vec![];
        }
        if mutation.source.as_ref() != self.source.as_ref()
            || mutation
                .session
                .provenance
                .as_ref()
                .is_some_and(|source| Some(source) != self.source.as_ref())
        {
            self.notice = Some("The session edit belongs to a different source incarnation".into());
            return vec![];
        }
        if self.source.is_none() && mutation.session.connection_id.is_some() {
            self.notice = Some(
                "This session names another gateway. Connect its owner before editing it".into(),
            );
            return vec![];
        }
        if !self.demo {
            let result = self
                .receipt_store
                .as_mut()
                .ok_or("The source-qualified edit store is unavailable".to_owned())
                .and_then(|store| store.begin());
            if let Err(error) = result {
                if let Some(store) = &mut self.receipt_store {
                    if let Ok(Some(receipt)) = store.load() {
                        if receipt
                            .mutation
                            .source
                            .as_ref()
                            .zip(self.source.as_ref())
                            .is_some_and(|(old, current)| receipt::can_reconcile(old, current))
                        {
                            let mut pending = receipt.mutation.clone();
                            pending.owner_connection = self.owner_connection;
                            pending.session.provenance = pending.source.clone();
                            self.mutation = Some(pending);
                            self.recovery = Some(receipt);
                        }
                    }
                }
                self.notice = Some(error);
                return vec![];
            }
        }
        let row = &mutation.session;
        let id = row.root_id();
        let request = PageRequest::get(format!("/api/sessions/{}/mutation-snapshot", segment(id)))
            .profile(row.profile());
        self.mutation = Some(mutation);
        self.mutation_inflight = true;
        vec![self.request("snapshot", request)]
    }
    fn send_mutation(&mut self) -> Vec<PageEffect> {
        let Some(mutation) = &self.mutation else {
            return vec![];
        };
        let Some(identity) = &mutation.identity else {
            return vec![];
        };
        if mutation.source.as_ref() != self.source.as_ref() {
            self.notice = Some("The pending edit belongs to a different source incarnation".into());
            return vec![];
        }
        let row = &mutation.session;
        let id = row.root_id();
        let path = format!("/api/sessions/{}", segment(id));
        let request = if mutation.delete {
            let mut request = PageRequest::delete(path, Value::Null);
            for (key, value) in identity.as_object().unwrap() {
                request = request.query(
                    key,
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string()),
                );
            }
            request
        } else {
            let mut payload = mutation.payload.as_object().cloned().unwrap_or_default();
            payload.extend(identity.as_object().unwrap().clone());
            payload.insert("profile".into(), row.profile().into());
            PageRequest::patch(path, Value::Object(payload))
        }
        .profile(row.profile());
        if self.connection_key.is_empty() && !self.demo {
            self.notice =
                Some("The immutable gateway identity is unavailable; no edit was sent".into());
            return vec![];
        }
        if let Err(error) = self.save_receipt() {
            self.notice = Some(error);
            return vec![];
        }
        self.mutation_inflight = true;
        vec![self.request("mutation", request)]
    }
    fn job_write(&mut self, id: &str, verb: &str) -> Vec<PageEffect> {
        if self.demo || self.jobs_busy.contains(id) {
            return vec![];
        }
        let Some(job) = self.jobs.iter().find(|job| job.id == id) else {
            return vec![];
        };
        let owner = job
            .profile
            .as_deref()
            .unwrap_or(&self.owner_profile)
            .to_owned();
        let path = format!("/api/cron/jobs/{}", segment(id));
        let request = if verb == "delete" {
            PageRequest::delete(path, Value::Null)
        } else {
            PageRequest::post(format!("{path}/{verb}"), Value::Null)
        }
        .profile(&owner)
        .timeout_secs(if verb == "trigger" { 86400 } else { 30 });
        self.jobs_busy.insert(id.into());
        vec![self.request(&format!("job-write.{id}"), request)]
    }
    fn runs_request(&mut self, id: &str) -> Vec<PageEffect> {
        let owner = self
            .jobs
            .iter()
            .find(|job| job.id == id)
            .and_then(|job| job.profile.clone())
            .unwrap_or_else(|| self.owner_profile.clone());
        vec![
            self.request(
                &format!("runs.{id}"),
                PageRequest::get(format!("/api/cron/jobs/{}/runs", segment(id)))
                    .query("limit", 5)
                    .profile(&owner),
            ),
        ]
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        self.pending.remove(op);
        let key = op.strip_prefix("sidebar.").unwrap_or(op);
        if key == "mutation" || key == "snapshot" {
            self.mutation_inflight = false;
        }
        if let Some(id) = key.strip_prefix("job-write.") {
            self.jobs_busy.remove(id);
        }
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                if key == "batch" && error.contains("HTTP 404") {
                    self.batch_missing = true;
                    return self.legacy_slices();
                }
                match key {
                    "batch" => {
                        self.recents.loading = false;
                        self.messaging.loading = false;
                        self.cron_sessions.loading = false;
                        self.recents.error = Some(error.clone());
                        self.messaging.error = Some(error.clone());
                        self.cron_sessions.error = Some(error.clone());
                    }
                    "recents" => {
                        self.recents.loading = false;
                        self.recents.error = Some(error.clone());
                    }
                    "messaging" => {
                        self.messaging.loading = false;
                        self.messaging.error = Some(error.clone());
                    }
                    "cron" => {
                        self.cron_sessions.loading = false;
                        self.cron_sessions.error = Some(error.clone());
                    }
                    "jobs" => self.jobs_error = Some(error.clone()),
                    "snapshot" => {
                        if let Some(store) = &mut self.receipt_store {
                            store.release_unprepared();
                        }
                        self.mutation = None;
                        self.unread_queue.clear();
                    }
                    _ => {}
                }
                if key == "mutation" {
                    self.unread_queue.clear();
                    // An explicit refusal never authorizes a new precondition automatically.
                    // It does retire this rejected intent so a later user edit may prepare afresh.
                    if [400, 401, 403, 404, 409, 410, 412, 413, 422, 428]
                        .iter()
                        .any(|code| error.contains(&format!("HTTP {code}")))
                    {
                        if let Err(retire) = self.retire_receipt() {
                            self.notice = Some(retire);
                        }
                    }
                }
                self.errors.insert(op.into(), error);
                return vec![];
            }
        };
        self.errors.remove(op);
        if key == "snapshot" {
            return self.accept_snapshot(value);
        }
        if key == "mutation" {
            if let Err(error) = self.retire_receipt() {
                self.notice = Some(format!(
                    "The edit was accepted, but its recovery receipt could not be retired: {error}"
                ));
                return vec![];
            }
            self.notice = Some("Session updated.".into());
            if !self.unread_queue.is_empty() {
                return self.next_unread();
            }
            let profile = self.owner_profile.clone();
            return self.load(&profile);
        }
        if key.starts_with("job-write.") {
            self.notice = Some("The scheduled job operation completed".into());
            let profile = self.owner_profile.clone();
            return self.load(&profile);
        }
        if let Some(profile) = key.strip_prefix("live.") {
            if let Some(source) = &self.source {
                let invalid = value["sessions"].as_array().is_some_and(|rows| {
                    rows.iter().any(|row| {
                        !crate::gateway::sidebar::source_tag_matches(
                            row["connection_id"].as_str(),
                            source,
                        ) || row["profile"]
                            .as_str()
                            .is_some_and(|owner| owner != profile)
                    })
                });
                if invalid {
                    self.errors.insert(
                        op.into(),
                        "The live session list contradicts its verified source/profile".into(),
                    );
                    return vec![];
                }
            }
            self.live.retain(|(owner, _), _| owner != profile);
            for row in value
                .get("sessions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let id = row
                    .get("id")
                    .or_else(|| row.get("session_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let status = match row["status"].as_str() {
                    Some("waiting") => Status::NeedsInput,
                    Some("working" | "streaming" | "starting" | "resuming") => Status::Working,
                    _ => {
                        if row["running"].as_bool() == Some(true) {
                            Status::Working
                        } else {
                            Status::Idle
                        }
                    }
                };
                if !id.is_empty() {
                    self.live.insert((profile.into(), id.into()), status);
                }
            }
            self.live_loaded = true;
            return vec![];
        }
        if let Some(id) = key.strip_prefix("runs.") {
            if self.peek_job.as_deref() == Some(id) {
                match serde_json::from_value::<Vec<crate::pages::cron::Run>>(value["runs"].clone())
                {
                    Ok(runs) => self.runs = runs,
                    Err(_) => {
                        self.errors
                            .insert(op.into(), "The agent returned invalid run history".into());
                    }
                }
            }
            return vec![];
        }
        match key {
            "batch" => {
                let slice = |key: &str| {
                    let mut slice = value[key].clone();
                    for shared in ["errors", "profiles_failed"] {
                        if slice.get(shared).is_none() && !value[shared].is_null() {
                            slice[shared] = value[shared].clone();
                        }
                    }
                    slice
                };
                apply_slice(
                    &mut self.recents,
                    &slice("recents"),
                    &self.owner_profile,
                    self.source.as_ref(),
                );
                apply_slice(
                    &mut self.messaging,
                    &slice("messaging"),
                    &self.owner_profile,
                    self.source.as_ref(),
                );
                apply_slice(
                    &mut self.cron_sessions,
                    &slice("cron"),
                    &self.owner_profile,
                    self.source.as_ref(),
                );
                self.storage = serde_json::from_value(value["storage"].clone()).unwrap_or_default();
            }
            "recents" => apply_slice(
                &mut self.recents,
                &value,
                &self.owner_profile,
                self.source.as_ref(),
            ),
            "messaging" => apply_slice(
                &mut self.messaging,
                &value,
                &self.owner_profile,
                self.source.as_ref(),
            ),
            "cron" => apply_slice(
                &mut self.cron_sessions,
                &value,
                &self.owner_profile,
                self.source.as_ref(),
            ),
            "jobs" => {
                match serde_json::from_value::<Vec<crate::pages::cron::Job>>(
                    value
                        .as_array()
                        .map(|_| value.clone())
                        .unwrap_or_else(|| value["jobs"].clone()),
                ) {
                    Ok(mut jobs) => {
                        jobs.sort_by(|a, b| {
                            a.next_run_at
                                .is_none()
                                .cmp(&b.next_run_at.is_none())
                                .then_with(|| a.next_run_at.cmp(&b.next_run_at))
                                .then_with(|| a.title().cmp(b.title()))
                        });
                        self.jobs = jobs;
                        self.jobs_loaded = true;
                        self.jobs_error = None;
                    }
                    Err(_) => {
                        self.jobs_error = Some("The agent returned invalid scheduled jobs".into())
                    }
                }
            }
            _ => {}
        }
        if matches!(key, "recents" | "messaging" | "cron")
            && value.get("profiles_truncated").is_none()
        {
            let (slice, cap) = match key {
                "recents" => (&mut self.recents, self.limit),
                "messaging" => (&mut self.messaging, self.messaging_limit),
                _ => (&mut self.cron_sessions, self.cron_limit),
            };
            let mut counts = BTreeMap::<String, usize>::new();
            for row in &slice.rows {
                *counts.entry(row.profile().into()).or_default() += 1;
            }
            slice.truncated = counts
                .into_iter()
                .map(|(profile, count)| (profile, count >= cap))
                .collect();
        }
        vec![]
    }
    fn accept_snapshot(&mut self, value: Value) -> Vec<PageEffect> {
        let exists = value["exists"].as_bool() == Some(true);
        let revision = value["runtime_revision"].as_u64();
        let generation = value["runtime_generation"].as_u64();
        if !exists || revision.is_none() || generation.is_none() {
            self.errors.insert("sidebar.snapshot".into(),"The agent could not confirm a revision-fenced session identity. Refresh before editing.".into());
            self.mutation = None;
            if let Some(store) = &mut self.receipt_store {
                store.release_unprepared();
            }
            return vec![];
        }
        if let Some(mutation) = &mut self.mutation {
            mutation.identity = Some(
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_revision":revision.unwrap(),"expected_generation":generation.unwrap()}),
            );
        }
        self.send_mutation()
    }
    pub fn fixture(&mut self) {
        self.demo = true;
        self.recents.loaded = true;
        self.messaging.loaded = true;
        self.cron_sessions.loaded = true;
        self.jobs_loaded = true;
        let now = chrono::Local::now().timestamp() as f64;
        self.now = now;
        let value = json!({"sessions":[{"id":"plan","profile":"default","title":"Plan the next release","source":"desktop","last_active":now-300.0,"pinned":true,"message_count":12},{"id":"draft","profile":"default","title":"A fresh idea","source":"desktop","last_active":now-7200.0,"message_count":0},{"id":"research","profile":"default","title":"Research notes","source":"desktop","last_active":now-86400.0*9.0,"message_count":18,"unread":true}]});
        self.recents.rows = parse_sessions(&value, "default").unwrap();
        self.messaging.rows=parse_sessions(&json!({"sessions":[{"id":"telegram-chat","profile":"default","title":"Weekend plans","source":"telegram","last_active":now-3600.0,"message_count":9}]}),"default").unwrap();
        self.jobs=serde_json::from_value(json!([{"id":"morning","name":"Morning brief","enabled":true,"state":"scheduled","schedule_display":"Every morning","next_run_at":(Local::now()+Duration::hours(2)).to_rfc3339()}])).unwrap();
    }
}
fn apply_slice(slice: &mut Slice, value: &Value, fallback: &str, source: Option<&SourceStamp>) {
    slice.loading = false;
    if value["failed"].as_bool() == Some(true) || value["retry"].as_bool() == Some(true) {
        slice.error = Some("This session slice failed to load. Retry to recover it.".into());
        return;
    }
    match source
        .map(|source| parse_sessions_owned(value, fallback, source))
        .unwrap_or_else(|| parse_sessions(value, fallback))
    {
        Ok(mut rows) => {
            let failed: BTreeSet<String> = value["profiles_failed"]
                .as_object()
                .into_iter()
                .flat_map(|m| m.keys().cloned())
                .chain(
                    value["errors"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e["profile"].as_str().map(str::to_owned)),
                )
                .collect();
            let mut seen: HashSet<String> = rows.iter().map(SidebarSession::key).collect();
            for row in &slice.rows {
                if failed.contains(row.profile()) && seen.insert(row.key()) {
                    rows.push(row.clone());
                }
            }
            slice.rows = rows;
            slice.loaded = true;
            slice.error = if failed.is_empty() {
                None
            } else {
                Some(format!(
                    "Some profiles could not be read: {}",
                    failed.into_iter().collect::<Vec<_>>().join(", ")
                ))
            };
            slice.truncated =
                serde_json::from_value(value["profiles_truncated"].clone()).unwrap_or_default();
        }
        Err(error) => slice.error = Some(error),
    }
}
fn toggle(set: &mut BTreeSet<String>, key: String) {
    if !set.remove(&key) {
        set.insert(key);
    }
}
pub fn cron_read_only(
    ended: Option<f64>,
    owned: Option<bool>,
    active: Option<bool>,
    last: Option<f64>,
    now: f64,
) -> bool {
    if ended.is_some() {
        return false;
    }
    if let Some(owned) = owned {
        return !owned;
    }
    if let Some(active) = active {
        return !active;
    }
    !last.is_some_and(|last| last > 0.0 && now - last < 300.0)
}
pub fn project(row: &SidebarSession) -> &str {
    row.git_repo_root
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(row.cwd.as_deref().filter(|s| !s.is_empty()))
        .unwrap_or("Home")
}
pub fn source_label(source: &str) -> String {
    match source {
        "api_server" => "API",
        "telegram" => "Telegram",
        "bluebubbles" | "photon" => "iMessage",
        "qqbot" => "QQ",
        "weixin" => "WeChat",
        "cli" => "CLI",
        "tui" => "TUI",
        "acp" => "ACP",
        _ => {
            return source
                .split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    .into()
}
pub fn relative_age(seconds: f64, now: f64) -> String {
    let elapsed = (now - seconds).max(0.0);
    if seconds <= 0.0 {
        return String::new();
    }
    if elapsed < 60.0 {
        "now".into()
    } else if elapsed < 3600.0 {
        format!("{}m", (elapsed / 60.0).floor() as u64)
    } else if elapsed < 86400.0 {
        format!("{}h", (elapsed / 3600.0).floor() as u64)
    } else {
        format!("{}d", (elapsed / 86400.0).floor() as u64)
    }
}
#[derive(Clone)]
pub enum ListRow {
    Divider(String, String),
    Session(SidebarSession),
}
pub fn lineage_rows(rows: Vec<&SidebarSession>) -> Vec<&SidebarSession> {
    // Compression aliases belong to one exact source/profile namespace; forks have a new root.
    let mut lineages: HashMap<String, Vec<&SidebarSession>> = HashMap::new();
    for row in &rows {
        lineages.entry(row.key()).or_default().push(*row);
    }
    let mut winners = HashMap::new();
    for (key, group) in lineages {
        let continued: HashSet<_> = group
            .iter()
            .filter_map(|row| row.parent_session_id.as_deref().map(|id| row.alias_key(id)))
            .collect();
        let best = group
            .iter()
            .copied()
            .reduce(|best, row| {
                let rank = |row: &SidebarSession| {
                    if continued.contains(&row.alias_key(&row.id)) {
                        f64::NEG_INFINITY
                    } else {
                        row.recency()
                    }
                };
                if rank(row) > rank(best) { row } else { best }
            })
            .unwrap();
        winners.insert(key, best);
    }
    let selected: Vec<_> = rows
        .iter()
        .copied()
        .filter(|row| std::ptr::eq(*row, winners[&row.key()]))
        .collect();
    let mut aliases = HashMap::new();
    for row in &rows {
        let winner = winners[&row.key()];
        for id in row.aliases() {
            aliases.insert(row.alias_key(id), winner);
        }
    }
    let mut children: HashMap<String, Vec<&SidebarSession>> = HashMap::new();
    let mut roots = vec![];
    for row in selected {
        let parent = row
            .fork_parent()
            .and_then(|id| aliases.get(&row.alias_key(id)).copied())
            .filter(|parent| parent.key() != row.key());
        if let Some(parent) = parent {
            children.entry(parent.key()).or_default().push(row);
        } else {
            roots.push(row);
        }
    }
    for children in children.values_mut() {
        children.sort_by(|a, b| b.recency().total_cmp(&a.recency()));
    }
    fn walk<'a>(
        row: &'a SidebarSession,
        children: &HashMap<String, Vec<&'a SidebarSession>>,
        seen: &mut HashSet<String>,
        out: &mut Vec<&'a SidebarSession>,
    ) {
        if !seen.insert(row.key()) {
            return;
        }
        out.push(row);
        if let Some(nested) = children.get(&row.key()) {
            for child in nested {
                walk(child, children, seen, out);
            }
        }
    }
    let mut output = vec![];
    let mut seen = HashSet::new();
    for root in roots {
        walk(root, &children, &mut seen, &mut output);
    }
    for group in children.values() {
        for row in group {
            walk(row, &children, &mut seen, &mut output);
        }
    }
    output
}
pub fn date_rows(rows: Vec<&SidebarSession>, now: f64) -> Vec<ListRow> {
    let branches: HashSet<_> = rows
        .iter()
        .filter(|row| {
            row.fork_parent().is_some_and(|parent| {
                rows.iter().any(|candidate| {
                    candidate.provenance == row.provenance
                        && candidate.profile() == row.profile()
                        && candidate.aliases().contains(&parent)
                        && candidate.key() != row.key()
                })
            })
        })
        .map(|row| row.key())
        .collect();
    let sorted = lineage_rows(rows);
    let mut roots: Vec<&SidebarSession> = sorted
        .iter()
        .copied()
        .filter(|r| !branches.contains(&r.key()))
        .collect();
    roots.sort_by(|a, b| b.recency().total_cmp(&a.recency()));
    let mut best = None;
    let mut score = f64::INFINITY;
    let mut ended = false;
    for index in 1..roots.len() {
        let gap = roots[index - 1].recency() - roots[index].recency();
        let end = gap > 8.0 * 3600.0;
        if gap >= 1800.0 || end {
            let next = (index as f64 / 5.0).ln().abs();
            if next < score {
                score = next;
                best = Some(index);
                ended = end;
            }
        }
        if end {
            break;
        }
    }
    let cutoff = match best {
        None => f64::NEG_INFINITY,
        Some(index)
            if ended
                && calendar_bucket(roots[0].recency(), now).0
                    == calendar_bucket(roots[index].recency(), now).0 =>
        {
            f64::INFINITY
        }
        Some(index) => roots[index - 1].recency(),
    };
    let mut output = vec![];
    let mut emitted = HashSet::new();
    let mut last = String::new();
    for row in sorted {
        if branches.contains(&row.key()) {
            output.push(ListRow::Session(row.clone()));
            continue;
        }
        if row.recency() >= cutoff {
            last = "recent".into();
            output.push(ListRow::Session(row.clone()));
            continue;
        }
        let (key, label) = calendar_bucket(row.recency(), now);
        if last != key {
            last = key.clone();
            if emitted.insert(key.clone()) && !output.is_empty() {
                output.push(ListRow::Divider(format!("date:{key}"), label));
            }
        }
        output.push(ListRow::Session(row.clone()));
    }
    output
}
fn nominal_date(seconds: f64) -> NaiveDate {
    Local
        .timestamp_opt((seconds - 4.0 * 3600.0) as i64, 0)
        .single()
        .unwrap_or_else(Local::now)
        .date_naive()
}
pub fn calendar_bucket(seconds: f64, now: f64) -> (String, String) {
    let date = nominal_date(seconds);
    let today = nominal_date(now);
    let day = (today - date).num_days();
    if day <= 0 {
        return ("today".into(), "Earlier today".into());
    }
    if day == 1 {
        return ("yesterday".into(), "Yesterday".into());
    }
    let week = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    if date >= week {
        return ("this-week".into(), "Earlier this week".into());
    }
    if date >= week - Duration::days(7) {
        return ("last-week".into(), "Last week".into());
    }
    if date.year() == today.year() && date.month() == today.month() {
        return ("this-month".into(), "Earlier this month".into());
    }
    let key = format!("{}-{}", date.year(), date.month());
    let label = if date.year() == today.year() {
        date.format("%B").to_string()
    } else {
        date.format("%B %Y").to_string()
    };
    (key, label)
}

impl crate::app::Viewer {
    pub fn sidebar_action(&mut self, action: Action) {
        if let Action::Open(profile, id) = &action {
            if let Some(row) = self.sidebar_state.find(profile, id) {
                if row.provenance.as_ref() != self.sidebar_state.source_stamp()
                    || row.provenance.is_none() && row.connection_id.is_some()
                {
                    self.sidebar_state.notice =
                        Some("Connect the gateway that owns this session before opening it".into());
                    return;
                }
                if row.source.as_deref() == Some("cron") {
                    let read_only = cron_read_only(
                        row.ended_at,
                        row.scheduler_owned,
                        row.is_active,
                        Some(row.last_active),
                        self.sidebar_state.now,
                    );
                    self.open_run_session(
                        profile.clone(),
                        row.id.clone(),
                        row.title().into(),
                        read_only,
                    );
                } else {
                    self.open_page_session(profile.clone(), row.id.clone(), row.title().into());
                }
            }
            return;
        }
        if let Action::OpenRun(index) = &action {
            if let Some(run) = self.sidebar_state.runs.get(*index) {
                let profile = run
                    .profile
                    .clone()
                    .unwrap_or_else(|| self.sidebar_state.owner_profile.clone());
                let id = run.session_id.clone();
                let title = run.title.clone().unwrap_or_else(|| "Scheduled run".into());
                if run.source.as_deref() == Some("cron") {
                    let read_only = cron_read_only(
                        run.ended_at,
                        run.scheduler_owned,
                        run.is_active,
                        run.last_active,
                        self.sidebar_state.now,
                    );
                    self.open_run_session(profile, id, title, read_only);
                }
            }
            return;
        }
        if let Action::ManageJob(id) = &action {
            let id = id.clone();
            self.open_route(crate::pages::Route::Cron);
            self.cron_action(crate::pages::cron::Action::Select(id));
            return;
        }
        let effects = self.sidebar_state.action(action);
        self.page_requests(effects);
        self.mirror_sidebar_preferences();
    }
}
