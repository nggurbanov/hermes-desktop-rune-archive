//! Native viewer state. Network replies carry the intent and profile that own them.
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::composer::{Composer, DraftScope, Popup};
use rune::{TextEdit, Tx, Waker};
use rune_kit::PaneState;

use crate::gateway::{
    CanonicalSession, Connection, Gateway, History, HistoryMessage, Profile, Session,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Sessions,
    Bots,
}

#[derive(Clone)]
pub struct ConversationTab {
    pub profile: String,
    pub session: String,
    pub title: String,
    pub bot: bool,
}

#[derive(Clone)]
pub enum ConversationTarget {
    Session {
        profile: String,
        id: String,
        title: String,
    },
    Bot(String),
}

#[derive(Clone)]
struct BotOpen {
    profile: String,
    new_tab: bool,
    replace: Option<(String, String)>,
}

pub struct Viewer {
    pub route: crate::pages::Route,
    pub conversation: crate::conversation::Conversation,
    pub chat: crate::chat::Chat,
    pub subagents: crate::subagents::Subagents,
    pub audio: crate::audio::Audio,
    pub plugin_hosts: crate::plugin_host::PluginHosts,
    plugin_dialogs: plugin_actions::NativeDialogs,
    plugin_navigation: Vec<plugin_actions::PluginNavigation>,
    pub connections: crate::connections::Connections,
    pub files: crate::files::Files,
    pub identities: crate::identities::Identities,
    pub directories: crate::directories::Directories,
    pub terminal: crate::terminal::Terminal,
    pub review: crate::review::Review,
    review_preferences: serde_json::Value,
    review_completions: HashMap<DraftScope, u64>,
    tool_owner: Option<(DraftScope, Option<String>)>,
    workspace_cwds: HashMap<DraftScope, String>,
    process_owner: Option<DraftScope>,
    next_process: f64,
    pub operations: crate::operations::Operations,
    pub health_open: bool,
    pub chrome: crate::client_status::ClientStatus,
    pub show_statusbar: bool,
    pub show_profile_rail: bool,
    pub hidden_status_items: std::collections::BTreeSet<String>,
    pub workspace_menu: bool,
    retired_profiles: std::collections::HashSet<(u64, String)>,
    audio_owner: Option<DraftScope>,
    spoken_scope: Option<DraftScope>,
    voice_interrupted: std::collections::HashSet<DraftScope>,
    pub speak_replies: bool,
    pub wake_enabled: bool,
    voice_sends: HashMap<DraftScope, String>,
    spoken_completion: HashMap<DraftScope, u64>,
    pub compose_error: Option<String>,
    pub show_reasoning: bool,
    pub admission_mode: crate::conversation::AdmissionMode,
    pub recoverable_sends: Vec<(String, String)>,
    read_only_chats: HashMap<DraftScope, String>,
    pub discard_send_confirm: Option<String>,
    policy_sends: HashMap<String, crate::composer::Draft>,
    recovery_scope: Option<DraftScope>,
    pub prompt_responses: HashMap<(DraftScope, String), TextEdit>,
    pub prompt_choices: HashMap<(DraftScope, String, String), std::collections::BTreeSet<String>>,
    chat_attached: std::collections::HashSet<DraftScope>,
    pub client_error: Option<String>,
    pub client_notice: Option<String>,
    preferences: crate::client::Preferences,
    pub(crate) profile_preferences: crate::client::profiles::ProfilePreferences,
    pub sidebar_state: crate::sidebar::SidebarState,
    pub settings: crate::pages::settings::Settings,
    pub bots: crate::pages::bots::Bots,
    bots_revision: u64,
    new_bot_launch: Option<(crate::pages::bots::NewBotChat, u64)>,
    new_bot_intro: Option<(String, u64, u64)>,
    pub capabilities: crate::pages::capabilities::Capabilities,
    pub cron: crate::pages::cron::Cron,
    pub messaging: crate::pages::messaging::Messaging,
    pub artifacts: crate::pages::artifacts::Artifacts,
    page_network: crate::pages::PageNetwork,
    page_owner: Option<(u64, String)>,
    page_connection: Option<Connection>,
    pub composer: Composer,
    pub voice: crate::voice::Voice,
    connection_epoch: u64,
    pub profiles: Vec<Profile>,
    pub sessions: Vec<Session>,
    pub messages: Vec<HistoryMessage>,
    pub profile: String,
    pub selected_session: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub loading: bool,
    pub history_loading: bool,
    pub history_truncated: bool,
    pub endpoint_label: String,
    pub search: TextEdit,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub theme: Option<bool>,
    pub layout: crate::layout::Node,
    pub arranging: bool,
    pub connection_form: bool,
    pub connection_url: TextEdit,
    pub connection_token: TextEdit,
    pub connection_local_home: TextEdit,
    pub sidebar_tab: SidebarTab,
    pub tabs: Vec<ConversationTab>,
    pub bot_empty: bool,
    pub simple_mode: bool,
    pub profile_picker: bool,
    pub row_menu: Option<(ConversationTarget, rune::PointerPress)>,
    pending_bot: Option<BotOpen>,
    advanced_tools_open: bool,
    fixture_history: HashMap<(String, String), Vec<HistoryMessage>>,
    connection: Option<Connection>,
    intent: u64,
    network: Option<Network>,
    fixture: bool,
}

struct Network {
    send: Sender<Request>,
    receive: Receiver<Reply>,
}
struct Request {
    intent: u64,
    command: Command,
}
enum Command {
    Connect(Connection, String),
    Sessions(String),
    History(String, String),
    Bot(String),
}
struct Reply {
    intent: u64,
    result: Result<Update, String>,
}
enum Update {
    Connected {
        profiles: Vec<Profile>,
        profile: String,
        sessions: Vec<Session>,
    },
    Sessions {
        profile: String,
        sessions: Vec<Session>,
    },
    Opened {
        profile: String,
        session: String,
        sessions: Vec<Session>,
        history: History,
    },
    BotOpened {
        profiles: Vec<Profile>,
        profile: String,
        sessions: Vec<Session>,
        chat: Option<(String, History)>,
    },
}

impl Viewer {
    fn empty() -> Self {
        Self {
            route: crate::pages::Route::Chat,
            conversation: crate::conversation::Conversation::new(),
            chat: crate::chat::Chat::new(),
            subagents: crate::subagents::Subagents::new(),
            connections: crate::connections::Connections::new(),
            files: crate::files::Files::new(),
            identities: Default::default(),
            directories: Default::default(),
            terminal: crate::terminal::Terminal::new(),
            review: crate::review::Review::new(),
            review_preferences: serde_json::Value::Null,
            review_completions: HashMap::new(),
            tool_owner: None,
            workspace_cwds: HashMap::new(),
            process_owner: None,
            next_process: 0.0,
            operations: crate::operations::Operations::new(),
            health_open: false,
            chrome: Default::default(),
            show_statusbar: true,
            show_profile_rail: true,
            hidden_status_items: crate::client::default_hidden_status(),
            workspace_menu: false,
            plugin_hosts: Default::default(),
            plugin_dialogs: plugin_actions::NativeDialogs::new(),
            plugin_navigation: vec![],
            retired_profiles: Default::default(),
            audio: Default::default(),
            audio_owner: None,
            spoken_scope: None,
            voice_interrupted: Default::default(),
            speak_replies: false,
            wake_enabled: false,
            voice_sends: HashMap::new(),
            spoken_completion: HashMap::new(),
            compose_error: None,
            show_reasoning: false,
            admission_mode: Default::default(),
            recoverable_sends: vec![],
            read_only_chats: HashMap::new(),
            discard_send_confirm: None,
            policy_sends: HashMap::new(),
            recovery_scope: None,
            prompt_responses: HashMap::new(),
            prompt_choices: HashMap::new(),
            chat_attached: Default::default(),
            client_error: None,
            client_notice: None,
            preferences: crate::client::Preferences::default(),
            profile_preferences: Default::default(),
            sidebar_state: crate::sidebar::SidebarState::new(),
            settings: crate::pages::settings::Settings::new(),
            bots: crate::pages::bots::Bots::new(),
            bots_revision: 0,
            new_bot_launch: None,
            new_bot_intro: None,
            capabilities: crate::pages::capabilities::Capabilities::new(),
            cron: crate::pages::cron::Cron::new(),
            messaging: crate::pages::messaging::Messaging::new(),
            artifacts: crate::pages::artifacts::Artifacts::new(),
            page_network: crate::pages::PageNetwork::default(),
            page_owner: None,
            page_connection: None,
            composer: Composer::new(),
            voice: crate::voice::Voice::default(),
            connection_epoch: 0,
            profiles: vec![],
            sessions: vec![],
            messages: vec![],
            profile: "default".into(),
            selected_session: None,
            status: "Not connected".into(),
            error: None,
            loading: false,
            history_loading: false,
            history_truncated: false,
            endpoint_label: "Connect an agent".into(),
            search: TextEdit::new(""),
            sidebar: PaneState::new(237.0).range(190.0, 360.0),
            inspector: PaneState::new(300.0).range(240.0, 520.0).collapsed(),
            theme: None,
            layout: Default::default(),
            arranging: false,
            connection_form: true,
            connection_url: TextEdit::new(""),
            connection_token: TextEdit::new(""),
            connection_local_home: TextEdit::new(default_agent_home().to_string_lossy()),
            sidebar_tab: SidebarTab::Sessions,
            tabs: vec![],
            bot_empty: false,
            simple_mode: false,
            profile_picker: false,
            row_menu: None,
            pending_bot: None,
            advanced_tools_open: false,
            fixture_history: HashMap::new(),
            connection: None,
            intent: 0,
            network: None,
            fixture: false,
        }
    }

    pub fn new() -> Self {
        let mut s = Self::empty();
        if let Some(path) = argument("--fixture") {
            match std::fs::read_to_string(path)
                .and_then(|v| serde_json::from_str(&v).map_err(std::io::Error::other))
            {
                Ok(value) => s.load_fixture(value),
                Err(_) => s.error = Some("Unable to read the selected fixture".into()),
            }
            return s;
        }
        s.sidebar_state.load_preferences();
        match crate::client::profiles::ProfilePreferences::load() {
            Ok(p) => s.profile_preferences = p,
            Err(error) => s.client_error = Some(error),
        }
        match crate::client::Preferences::load() {
            Ok(prefs) => {
                s.speak_replies = prefs.speak_replies;
                s.review_preferences = prefs.review.clone();
                s.review.hydrate_preferences(&prefs.review);
                s.show_statusbar = prefs.show_statusbar;
                s.show_profile_rail = prefs.show_profile_rail;
                s.hidden_status_items = prefs.hidden_status_items.clone();
                s.theme = prefs.theme;
                s.layout = prefs
                    .layout
                    .clone()
                    .normalize()
                    .filter(|t| t.panes().iter().any(|p| p == "workspace"))
                    .unwrap_or_default();
                s.simple_mode = prefs.simple_mode;
                s.sidebar.set_width(prefs.sidebar_width);
                s.inspector.set_width(prefs.inspector_width);
                if !prefs.sidebar_open {
                    s.sidebar.toggle();
                }
                if prefs.inspector_open {
                    s.inspector.toggle();
                    s.advanced_tools_open = true;
                }
                s.preferences = prefs;
            }
            Err(error) => s.client_error = Some(error),
        }
        if let Some(endpoint) = argument("--gateway") {
            let token = std::env::var("HERMES_GATEWAY_TOKEN")
                .ok()
                .filter(|s| !s.is_empty());
            match url_connection(&endpoint, token) {
                Ok((connection, visible_url)) => {
                    s.connection_url.replace_text(visible_url);
                    s.connection = Some(connection);
                }
                Err(error) => s.error = Some(error),
            }
        } else if let Some(home) = argument("--local-home") {
            s.connection_local_home.replace_text(home.clone());
            s.connection = Some(Connection::Local { home: home.into() });
        }
        if let Some(profile) = argument("--profile") {
            s.profile = profile;
        }
        s.connection_form = s.connection.is_none();
        s
    }

    pub fn begin(&mut self) {
        if !self.fixture && self.connection.is_some() {
            self.reconnect();
        }
    }

    pub fn reconnect(&mut self) {
        let Some(connection) = self.connection.clone() else {
            self.change_connection();
            return;
        };
        self.endpoint_label = connection_label(&connection);
        self.loading = true;
        self.error = None;
        self.status = "Connecting…".into();
        self.dispatch(Command::Connect(connection, self.profile.clone()));
    }

    pub fn change_connection(&mut self) {
        self.connection_form = true;
    }
    pub fn can_cancel_connection(&self) -> bool {
        self.connection.is_some() || self.fixture
    }
    pub fn cancel_connection(&mut self) {
        self.connection_token.replace_text("");
        self.connection_form = self.connection.is_none() && !self.fixture;
    }
    pub fn apply_connection(&mut self) {
        if self.fixture {
            self.error = Some("Connections are disabled in fixture mode".into());
            return;
        }
        let endpoint = self.connection_url.text.trim().to_owned();
        let valid = url::Url::parse(&endpoint).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https" | "ws" | "wss")
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
                && u.query().is_none()
                && u.fragment().is_none()
        });
        if !valid {
            self.error = Some("Enter a gateway URL without credentials or query parameters; use the token field for authentication".into());
            return;
        }
        let token =
            (!self.connection_token.text.is_empty()).then(|| self.connection_token.text.clone());
        self.connection_token.replace_text("");
        self.use_connection(Connection::Url { endpoint, token });
    }
    pub fn apply_local_connection(&mut self) {
        if self.fixture {
            self.error = Some("Connections are disabled in fixture mode".into());
            return;
        }
        let home = PathBuf::from(self.connection_local_home.text.trim());
        if !home.is_absolute() {
            self.error = Some("Choose an absolute local Hermes home path".into());
            return;
        }
        self.use_connection(Connection::Local { home });
    }
    fn use_connection(&mut self, connection: Connection) {
        if !self.allow_owner_change("") {
            return;
        }
        self.chat.close_connection(self.connection_epoch);
        self.subagents.close_connection(self.connection_epoch);
        self.conversation.close_connection(self.connection_epoch);
        self.audio.close_connection(self.connection_epoch);
        self.terminal.close_connection(self.connection_epoch);
        self.connection_epoch += 1;
        self.page_network.invalidate();
        self.composer.close();
        self.connection = Some(connection);
        self.connection_form = false;
        self.profiles.clear();
        self.sessions.clear();
        self.messages.clear();
        self.selected_session = None;
        self.tabs.clear();
        self.pending_bot = None;
        self.row_menu = None;
        self.bot_empty = false;
        self.history_truncated = false;
        self.reconnect();
    }
    pub fn toggle_theme(&mut self, system_dark: bool) {
        self.theme = Some(!self.theme.unwrap_or(system_dark));
    }

    pub fn composer_scope(&self) -> DraftScope {
        DraftScope {
            connection: self.connection_epoch,
            profile: self.profile.clone(),
            session: self.selected_session.clone(),
        }
    }

    pub fn open_composer(&mut self, popup: Popup) {
        let scope = self.composer_scope();
        self.row_menu = None;
        self.composer.toggle_popup(scope.clone(), popup);
        if popup == Popup::VoiceEngine && self.composer.popup.is_some() {
            self.reload_voice_status();
        }
        if popup == Popup::Models
            && self.composer.popup.is_some()
            && self.composer.catalog_for(&scope).is_none()
            && !self.composer.catalog_loading_for(&scope)
        {
            self.reload_models();
        }
    }

    pub fn reload_models(&mut self) {
        if self.fixture {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            self.composer
                .request_catalog(connection, self.composer_scope());
        } else if !self.fixture {
            self.composer.catalog_error = Some("Connect an agent to browse its models".into());
        }
    }
    pub fn reload_voice_status(&mut self) {
        if self.fixture {
            return;
        }
        if let Some(connection) = self.connection.clone() {
            let scope = self.composer_scope();
            if !self.voice.loading(&scope) {
                self.voice.request(connection, &scope);
            }
        }
    }
    pub fn can_refresh_voice_status(&self) -> bool {
        self.connection.is_some() && !self.fixture
    }
    pub fn select_profile(&mut self, profile: String) {
        self.profile_picker = false;
        if profile == self.profile {
            return;
        }
        if !self.allow_owner_change(&profile) {
            return;
        }
        self.remember_profile_appearance();
        self.profile = profile.clone();
        self.restore_profile_appearance();
        self.pending_bot = None;
        self.bot_empty = false;
        self.selected_session = None;
        self.messages.clear();
        self.sessions.clear();
        self.history_loading = false;
        self.history_truncated = false;
        self.loading = true;
        self.error = None;
        self.search.replace_text("");
        if self.fixture {
            self.intent += 1;
            self.loading = false;
            return;
        }
        self.dispatch(Command::Sessions(profile));
    }
    pub fn open_session(&mut self, id: String) {
        self.layout.open("workspace");
        if self.route == crate::pages::Route::Settings {
            self.settings.vault.dismiss();
        }
        self.route = crate::pages::Route::Chat;
        let title = self
            .sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| "Conversation".into());
        self.open_conversation(self.profile.clone(), id, title, false, false);
    }

    pub fn open_bot(&mut self, profile: String) {
        self.layout.open("workspace");
        if self.route == crate::pages::Route::Settings {
            self.settings.vault.dismiss();
        }
        self.route = crate::pages::Route::Chat;
        self.open_bot_with_intent(profile, false);
    }

    pub fn open_target(&mut self, target: ConversationTarget, new_tab: bool) {
        self.row_menu = None;
        match target {
            ConversationTarget::Bot(profile) => self.open_bot_with_intent(profile, new_tab),
            ConversationTarget::Session { profile, id, title } => {
                self.open_conversation(profile, id, title, false, new_tab)
            }
        }
    }

    fn open_bot_with_intent(&mut self, profile: String, new_tab: bool) {
        if !self.allow_owner_change(&profile) {
            return;
        }
        let Some(bot) = self.profiles.iter().find(|p| p.name == profile).cloned() else {
            return;
        };
        if !self.fixture {
            let replace = self.foreground_slot();
            if self.profile != profile {
                self.sessions.clear();
                self.search.replace_text("");
            }
            self.profile = profile.clone();
            self.pending_bot = Some(BotOpen {
                profile: profile.clone(),
                new_tab,
                replace,
            });
            self.selected_session = None;
            self.bot_empty = false;
            self.messages.clear();
            self.error = None;
            self.history_loading = true;
            self.history_truncated = false;
            self.connection_form = false;
            self.dispatch(Command::Bot(profile));
            return;
        }
        if let Some(chat) = bot.canonical_session {
            self.open_conversation(profile, chat.id, bot.label, true, new_tab);
        } else {
            if self.profile == profile {
                self.intent += 1;
                self.loading = false;
            } else {
                self.select_profile(profile);
            }
            self.selected_session = None;
            self.messages.clear();
            self.history_loading = false;
            self.history_truncated = false;
            self.bot_empty = true;
            self.error = None;
        }
    }

    fn open_conversation(
        &mut self,
        profile: String,
        id: String,
        title: String,
        bot: bool,
        new_tab: bool,
    ) {
        if !self.allow_owner_change(&profile) {
            return;
        }
        let replace = self.foreground_slot();
        self.connection_form = false;
        self.pending_bot = None;
        self.bot_empty = false;
        if self.selected_session.as_ref() == Some(&id) && self.error.is_none() {
            if self.profile == profile {
                return;
            }
        }
        let changed_profile = self.profile != profile;
        if changed_profile {
            self.profile = profile.clone();
            self.sessions.clear();
            self.search.replace_text("");
        }
        self.place_tab(
            ConversationTab {
                profile: profile.clone(),
                session: id.clone(),
                title,
                bot,
            },
            new_tab,
            replace,
        );
        self.selected_session = Some(id.clone());
        self.messages.clear();
        self.history_loading = true;
        self.history_truncated = false;
        self.error = None;
        if self.fixture {
            self.intent += 1;
            self.history_loading = false;
            self.loading = false;
            self.messages = self
                .fixture_history
                .get(&(profile, id))
                .cloned()
                .unwrap_or_default();
            return;
        }
        // A foreground read is self-contained, so coalescing cannot discard
        // the session-list hydration required by a quick profile/tab switch.
        self.dispatch(Command::History(profile, id));
    }

    fn foreground_slot(&self) -> Option<(String, String)> {
        self.selected_session
            .as_ref()
            .map(|id| (self.profile.clone(), id.clone()))
            .or_else(|| {
                self.pending_bot
                    .as_ref()
                    .and_then(|pending| pending.replace.clone())
            })
    }

    fn place_tab(
        &mut self,
        tab: ConversationTab,
        new_tab: bool,
        replace: Option<(String, String)>,
    ) {
        if let Some(existing) = self
            .tabs
            .iter_mut()
            .find(|t| t.profile == tab.profile && (t.session == tab.session || (t.bot && tab.bot)))
        {
            *existing = tab;
        } else if !new_tab
            && let Some(index) = replace.and_then(|(profile, id)| {
                self.tabs
                    .iter()
                    .position(|t| t.profile == profile && t.session == id)
            })
        {
            self.tabs[index] = tab;
        } else {
            self.tabs.push(tab);
        }
    }

    pub fn activate_tab(&mut self, index: usize) {
        self.layout.open("workspace");
        if self.route == crate::pages::Route::Settings {
            self.settings.vault.dismiss();
        }
        self.route = crate::pages::Route::Chat;
        if let Some(tab) = self.tabs.get(index).cloned() {
            if tab.bot {
                self.open_bot_with_intent(tab.profile, true);
            } else {
                self.open_conversation(tab.profile, tab.session, tab.title, false, true);
            }
        }
    }

    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(index);
        let pending_anchor = self
            .pending_bot
            .as_ref()
            .and_then(|p| p.replace.as_ref())
            .is_some_and(|(profile, id)| profile == &tab.profile && id == &tab.session);
        if pending_anchor
            || (self.profile == tab.profile && self.selected_session.as_ref() == Some(&tab.session))
        {
            self.pending_bot = None;
            self.selected_session = None;
            self.messages.clear();
            self.history_loading = false;
            self.intent += 1;
            if !self.tabs.is_empty() {
                self.activate_tab(index.min(self.tabs.len() - 1));
            }
        }
    }

    pub fn active_tab(&self) -> Option<&ConversationTab> {
        self.tabs.iter().find(|t| {
            t.profile == self.profile && self.selected_session.as_ref() == Some(&t.session)
        })
    }

    pub fn bot_is_active(&self, name: &str) -> bool {
        self.active_tab()
            .is_some_and(|t| t.bot && t.profile == name)
            || self.pending_bot.as_ref().is_some_and(|p| p.profile == name)
            || (self.bot_empty && self.profile == name)
    }

    pub fn set_simple_mode(&mut self, simple: bool) {
        if simple == self.simple_mode {
            return;
        }
        if simple {
            self.advanced_tools_open = self.inspector.is_open();
        }
        self.simple_mode = simple;
        self.inspector.set_open(!simple && self.advanced_tools_open);
    }
    fn dispatch(&mut self, command: Command) {
        self.intent += 1;
        let sent = self.network.as_ref().is_some_and(|n| {
            n.send
                .send(Request {
                    intent: self.intent,
                    command,
                })
                .is_ok()
        });
        if !sent {
            self.loading = false;
            self.history_loading = false;
            self.error = Some("The connection worker is unavailable. Restart the client.".into());
        }
    }
    fn apply(&mut self, reply: Reply) {
        if reply.intent != self.intent {
            return;
        }
        self.loading = false;
        self.history_loading = false;
        match reply.result {
            Err(error) => {
                self.status = "Connection error".into();
                self.error = Some(error);
            }
            Ok(Update::Connected {
                profiles,
                profile,
                sessions,
            }) => {
                let reopen_bot = self
                    .pending_bot
                    .take()
                    .filter(|p| p.profile == profile)
                    .or_else(|| {
                        self.active_tab()
                            .filter(|t| t.bot && t.profile == profile)
                            .map(|t| BotOpen {
                                profile: t.profile.clone(),
                                new_tab: true,
                                replace: None,
                            })
                    });
                let retained = if self.profile == profile {
                    self.selected_session.clone().filter(|id| {
                        sessions.iter().any(|s| &s.id == id)
                            || profiles.iter().any(|p| {
                                p.name == profile
                                    && p.canonical_session
                                        .as_ref()
                                        .is_some_and(|c| &c.id == id || &c.resolved_id == id)
                            })
                    })
                } else {
                    None
                };
                self.profiles = profiles;
                self.profile = profile;
                self.sessions = sessions;
                self.selected_session = None;
                self.messages.clear();
                self.error = None;
                self.status = "Connected".into();
                if let Some(bot) = reopen_bot {
                    let profile = bot.profile.clone();
                    self.pending_bot = Some(bot);
                    self.history_loading = true;
                    self.dispatch(Command::Bot(profile));
                } else if let Some(id) = retained {
                    if let Some(tab) = self
                        .tabs
                        .iter()
                        .find(|t| t.profile == self.profile && t.session == id)
                        .cloned()
                    {
                        if tab.bot {
                            self.open_bot(tab.profile);
                        } else {
                            self.open_conversation(
                                tab.profile,
                                tab.session,
                                tab.title,
                                false,
                                true,
                            );
                        }
                    } else {
                        self.open_session(id);
                    }
                }
            }
            Ok(Update::Sessions { profile, sessions }) if profile == self.profile => {
                self.sessions = sessions;
                self.status = "Connected".into();
            }
            Ok(Update::Opened {
                profile,
                session,
                sessions,
                history,
            }) if profile == self.profile && self.selected_session.as_ref() == Some(&session) => {
                self.sessions = sessions;
                self.messages = history.messages;
                self.history_truncated = history.has_more;
                self.status = "Connected".into();
                self.error = None;
            }
            Ok(Update::BotOpened {
                profiles,
                profile,
                sessions,
                chat,
            }) if profile == self.profile
                && self
                    .pending_bot
                    .as_ref()
                    .is_some_and(|p| p.profile == profile) =>
            {
                let intent = self.pending_bot.take().expect("matching bot open");
                self.profiles = profiles;
                self.sessions = sessions;
                self.status = "Connected".into();
                self.error = None;
                if let Some((id, history)) = chat {
                    let title = self
                        .profiles
                        .iter()
                        .find(|p| p.name == profile)
                        .map(|p| p.label.clone())
                        .unwrap_or_else(|| profile.clone());
                    self.place_tab(
                        ConversationTab {
                            profile,
                            session: id.clone(),
                            title,
                            bot: true,
                        },
                        intent.new_tab,
                        intent.replace,
                    );
                    self.selected_session = Some(id);
                    self.messages = history.messages;
                    self.history_truncated = history.has_more;
                    self.bot_empty = false;
                } else {
                    self.selected_session = None;
                    self.messages.clear();
                    self.bot_empty = true;
                }
            }
            Ok(_) => {}
        }
    }
    fn load_fixture(&mut self, v: serde_json::Value) {
        self.fixture = true;
        self.chrome.fixture();
        self.connection_form = false;
        self.endpoint_label = "Sample agent · Fixture".into();
        self.status = "Fixture · Read only".into();
        self.profiles = v["profiles"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| Profile {
                name: p["name"].as_str().unwrap_or("default").into(),
                label: p["label"].as_str().unwrap_or("Default").into(),
                description: p["description"].as_str().unwrap_or("").into(),
                canonical_session: p["canonical_session"]["id"].as_str().map(|id| {
                    CanonicalSession {
                        id: id.into(),
                        resolved_id: id.into(),
                        preview: p["canonical_session"]["preview"]
                            .as_str()
                            .unwrap_or("")
                            .into(),
                    }
                }),
            })
            .collect();
        self.profile = v["profile"].as_str().unwrap_or("default").into();
        self.sessions = v["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| Session {
                id: p["id"].as_str().unwrap_or("").into(),
                title: p["title"].as_str().unwrap_or("Untitled").into(),
                updated: String::new(),
            })
            .collect();
        self.selected_session = v["selected_session"].as_str().map(String::from);
        self.messages = v["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| HistoryMessage {
                role: p["role"].as_str().unwrap_or("assistant").into(),
                content: p["content"].as_str().unwrap_or("").into(),
                raw: p.clone(),
            })
            .collect();
        if let Some(id) = &self.selected_session {
            self.fixture_history
                .insert((self.profile.clone(), id.clone()), self.messages.clone());
            let title = self
                .sessions
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.title.clone())
                .unwrap_or_else(|| "Conversation".into());
            self.tabs.push(ConversationTab {
                profile: self.profile.clone(),
                session: id.clone(),
                title,
                bot: false,
            });
        }
        for history in v["histories"].as_array().into_iter().flatten() {
            if let (Some(profile), Some(id), Some(messages)) = (
                history["profile"].as_str(),
                history["session"].as_str(),
                history["messages"].as_array(),
            ) {
                self.fixture_history.insert(
                    (profile.into(), id.into()),
                    messages
                        .iter()
                        .map(|m| HistoryMessage {
                            role: m["role"].as_str().unwrap_or("assistant").into(),
                            content: m["content"].as_str().unwrap_or("").into(),
                            raw: m.clone(),
                        })
                        .collect(),
                );
            }
        }
        if v["model_catalog"].is_object() {
            let scope = self.composer_scope();
            self.composer
                .fixture_catalog(scope, &v["model_catalog"])
                .expect("valid synthetic model catalog");
        }
    }
}

pub fn setup(s: &mut Viewer, waker: Waker) {
    s.composer.init_network(waker.clone());
    s.conversation.init(waker.clone());
    s.subagents.init(waker.clone());
    s.chat.init(waker.clone());
    s.audio.init(waker.clone());
    s.operations.init(waker.clone());
    s.files.init(waker.clone());
    s.identities.init(waker.clone());
    if !s.fixture {
        s.terminal.init(waker.clone());
    }
    s.review.init(waker.clone());
    s.plugin_dialogs.init(waker.clone());
    s.voice.init(waker.clone());
    s.page_network.init(waker.clone());
    s.messaging.init(waker.clone());
    s.bots.init(waker.clone());
    s.artifacts.init(waker.clone());
    if s.fixture {
        s.connections.fixture();
        return;
    }
    s.connections.init(waker.clone());
    let (send, requests) = mpsc::channel::<Request>();
    let (replies, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut gateway: Option<Gateway> = None;
        let mut pending = VecDeque::new();
        loop {
            let first = match pending.pop_front() {
                Some(request) => request,
                None => match requests.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                },
            };
            pending = coalesce(first, requests.try_iter());
            let request = pending.pop_front().expect("a queued read intent");
            let result = match request.command {
                Command::Connect(connection, requested_profile) => {
                    gateway = None;
                    Gateway::connect(&connection).and_then(|mut g| {
                        let profiles = g.profiles()?;
                        let profile = if profiles.iter().any(|p| p.name == requested_profile) {
                            requested_profile
                        } else {
                            profiles
                                .first()
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| "default".into())
                        };
                        let sessions = g.sessions(&profile)?;
                        gateway = Some(g);
                        Ok(Update::Connected {
                            profiles,
                            profile,
                            sessions,
                        })
                    })
                }
                Command::Sessions(profile) => gateway
                    .as_mut()
                    .ok_or_else(|| "Connect an agent first".into())
                    .and_then(|g| {
                        g.sessions(&profile)
                            .map(|sessions| Update::Sessions { profile, sessions })
                    }),
                Command::History(profile, session) => gateway
                    .as_mut()
                    .ok_or_else(|| "Connect an agent first".into())
                    .and_then(|g| {
                        let sessions = g.sessions(&profile)?;
                        let history = g.history(&profile, &session)?;
                        Ok(Update::Opened {
                            profile,
                            session,
                            sessions,
                            history,
                        })
                    }),
                Command::Bot(profile) => gateway
                    .as_mut()
                    .ok_or_else(|| "Connect an agent first".into())
                    .and_then(|g| {
                        let profiles = g.profiles()?;
                        let sessions = g.sessions(&profile)?;
                        let chat = profiles
                            .iter()
                            .find(|p| p.name == profile)
                            .and_then(|p| p.canonical_session.as_ref())
                            .map(|chat| {
                                g.history(&profile, &chat.id)
                                    .map(|history| (chat.id.clone(), history))
                            })
                            .transpose()?;
                        Ok(Update::BotOpened {
                            profiles,
                            profile,
                            sessions,
                            chat,
                        })
                    }),
            };
            if replies
                .send(Reply {
                    intent: request.intent,
                    result,
                })
                .is_err()
            {
                break;
            }
            waker();
        }
    });
    s.network = Some(Network { send, receive });
}

pub fn poll(s: &mut Viewer, now: f64, tx: &mut Tx) -> bool {
    s.save_client_preferences();
    if !s.fixture {
        match s.composer.poll_storage(now) {
            Ok(Some(delay)) => tx.wake_after(delay),
            Err(error) => s.client_error = Some(error),
            _ => {}
        }
    }
    if s.sidebar.is_open() {
        tx.wake_after(std::time::Duration::from_secs(15));
    }
    s.messaging
        .observe(s.route == crate::pages::Route::Messaging && !s.connection_form);
    s.operations
        .observe(s.route == crate::pages::Route::CommandCenter, s.health_open);
    if s.chrome.observe(s.composer_scope(), s.chat_running()) {
        s.invalidate_page_requests("chrome.context");
    }
    s.poll_new_bot_intro();
    s.settings_local_navigation();
    let timers = s.show_statusbar && !s.simple_mode;
    let timer_changed = s.chrome.timers_changed(
        timers && s.status_item_visible("turn-timer"),
        timers && s.status_item_visible("session-timer"),
    );
    let scope = s.composer_scope();
    let chat_visible = s.route == crate::pages::Route::Chat
        && s.layout.shows("workspace", &|pane| match pane {
            "workspace" => true,
            "sessions" | "hermes-bots:pane" => s.sidebar.is_open(),
            "files" | "review" | "terminal" => s.inspector.is_open(),
            _ => false,
        });
    let subagents_changed = if !s.fixture && chat_visible {
        if let Some(connection) = s.connection.as_ref() {
            s.subagents.observe(
                connection,
                scope.clone(),
                s.conversation.view(&scope),
                s.theme,
            )
        } else {
            s.subagents.poll(s.theme)
        }
    } else {
        s.subagents.poll(s.theme)
    };
    if let Some(delay) = s.subagents.wake_after() {
        tx.wake_after(delay);
    }
    let pages_changed = subagents_changed
        | timer_changed
        | s.poll_chat(tx)
        | s.poll_pages(now)
        | s.poll_conversations()
        | s.poll_audio()
        | s.poll_plugin_hosts(tx)
        | s.poll_connections()
        | s.poll_files(now)
        | s.poll_tools(now, tx);
    if s.inspector.is_open() || s.route == crate::pages::Route::FilePreview {
        tx.wake_after(std::time::Duration::from_secs(4));
    }
    if s.plugin_hosts.is_open() {
        tx.wake_after(std::time::Duration::from_millis(30));
    }
    if let Some(delay) = s.review.wake_after() {
        tx.wake_after(delay);
    }
    if s.inspector.is_open() {
        tx.wake_after(std::time::Duration::from_secs(2));
    }
    if let Some(delay) = s.messaging.wake_after() {
        tx.wake_after(delay);
    }
    if let Some(delay) = s.operations.wake_after() {
        tx.wake_after(delay);
    }
    if let Some(delay) = s.bots.wake_after() {
        tx.wake_after(delay);
    }
    if let Some(delay) = s.settings.wake_after() {
        tx.wake_after(delay);
    }
    if s.route == crate::pages::Route::Cron {
        tx.wake_after(std::time::Duration::from_secs(15));
    }
    if s.route == crate::pages::Route::Capabilities && s.capabilities.needs_poll() {
        tx.wake_after(std::time::Duration::from_secs(2));
    }
    if s.show_statusbar
        && !s.simple_mode
        && (s.status_item_visible("turn-timer") && s.chat_running()
            || s.status_item_visible("session-timer"))
    {
        tx.wake_after(std::time::Duration::from_secs(1));
    }
    if s.chrome.resources_enabled
        && s.show_statusbar
        && !s.simple_mode
        && s.status_item_visible("resources")
    {
        tx.wake_after(std::time::Duration::from_secs(5));
    }
    if s.audio_needs_tick() {
        tx.wake_after(std::time::Duration::from_millis(50));
    }
    let scope = s.composer_scope();
    let composer_changed = pages_changed
        | s.composer.poll(&scope)
        | s.composer.dismiss_if_scope_changed(&scope)
        | s.voice.poll(&scope);
    let mut replies = vec![];
    if let Some(network) = &s.network {
        while let Ok(reply) = network.receive.try_recv() {
            replies.push(reply);
        }
    }
    if replies.is_empty() {
        if composer_changed {
            tx.instant();
        }
        return composer_changed;
    }
    for reply in replies {
        s.apply(reply);
    }
    tx.instant();
    true
}

fn argument(key: &str) -> Option<String> {
    let a = rune::extra_args();
    a.iter()
        .position(|v| v == key)
        .and_then(|i| a.get(i + 1))
        .cloned()
}
fn default_agent_home() -> PathBuf {
    std::env::var_os("HERMES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".hermes")
        })
}
fn connection_label(connection: &Connection) -> String {
    match connection {
        Connection::Saved { inner, .. } => connection_label(inner),
        Connection::Local { .. } => "Local Hermes".into(),
        Connection::Url { endpoint, .. } => url::Url::parse(endpoint)
            .ok()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_else(|| "Remote Hermes".into()),
    }
}

pub fn reader_scene(s: &mut Viewer) {
    s.load_fixture(
        serde_json::from_str(include_str!("../fixtures/reader.json"))
            .expect("committed reader fixture"),
    );
    s.ensure_page_owner();
}
pub fn bots_scene(s: &mut Viewer) {
    reader_scene(s);
    s.sidebar_tab = SidebarTab::Bots;
    s.open_target(ConversationTarget::Bot("default".into()), true);
}
pub fn layout_scene(s: &mut Viewer) {
    reader_scene(s);
    s.inspector.set_open(true);
}
pub fn subagents_scene(s: &mut Viewer) {
    reader_scene(s);
    s.subagents.fixture(s.composer_scope());
}
pub fn rich_chat_scene(s: &mut Viewer) {
    reader_scene(s);
    s.rich_chat_fixture();
}
pub fn composer_scene(s: &mut Viewer) {
    reader_scene(s);
    let scope = s.composer_scope();
    s.composer.draft_mut(scope.clone()).text.replace_text("Сравни эти варианты и помоги выбрать следующий шаг.\nХочу сохранить разговор и контекст агента.");
    s.composer.draft_mut(scope.clone()).reasoning = Some("high".into());
    s.composer.draft_mut(scope.clone()).approval_mode = Some("smart".into());
    s.composer
        .toggle_favorite(scope.connection, "openai", "gpt-5");
    s.composer
        .toggle_favorite(scope.connection, "anthropic", "claude-sonnet-4-5");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/images/reader-light.png");
    let _ = s.composer.add_attachment(scope.clone(), path);
    if let Some(draft) = s.composer.draft(&scope) {
        for attachment in &draft.attachments {
            if let Some(image) = &attachment.preview {
                let _ = image.wait();
            }
        }
    }
}
pub fn models_scene(s: &mut Viewer) {
    composer_scene(s);
    s.open_composer(Popup::Models);
}
pub fn reasoning_scene(s: &mut Viewer) {
    composer_scene(s);
    s.open_composer(Popup::Settings);
}
pub fn permissions_scene(s: &mut Viewer) {
    composer_scene(s);
    s.open_composer(Popup::Settings);
}
pub fn voice_scene(s: &mut Viewer) {
    composer_scene(s);
    s.composer.voice_expanded = true;
}
pub fn voice_chat_scene(s: &mut Viewer) {
    reader_scene(s);
    s.voice.fixture(
        &s.composer_scope(),
        &serde_json::json!({"ok":true,"mode":"chained","available":true}),
    );
    s.composer.voice_expanded = true;
}
pub fn voice_engine_scene(s: &mut Viewer) {
    voice_chat_scene(s);
    s.open_composer(Popup::VoiceEngine);
}
pub fn empty_scene(s: &mut Viewer) {
    reader_scene(s);
    s.sessions.clear();
    s.messages.clear();
    s.selected_session = None;
}
pub fn error_scene(s: &mut Viewer) {
    empty_scene(s);
    s.error = Some("The agent could not be reached. Check the connection and try again.".into());
}

#[cfg(test)]
mod tests;

pub fn on_key(s: &mut Viewer, event: &rune::KeyEvent, tx: &mut Tx) -> bool {
    if crate::view::terminal::on_key(s, event, tx) {
        return true;
    }
    if let rune::Key::Command(ch) = event.key {
        match ch {
            'n' => s.new_draft(),
            ',' => s.open_route(crate::pages::Route::Settings),
            'b' => s.sidebar.toggle(),
            'j' => s.inspector.toggle(),
            'k' => s.open_route(crate::pages::Route::CommandCenter),
            's' if event.mods.shift => s.show_statusbar = !s.show_statusbar,
            _ => return crate::view::composer::on_key(s, event, tx),
        }
        tx.instant();
        return true;
    }
    if crate::view::composer::on_key(s, event, tx) {
        return true;
    }
    if matches!(&event.key, rune::Key::Escape)
        && (s.composer.popup.is_some() || s.composer.voice_expanded)
    {
        s.composer.close();
        tx.instant();
        return true;
    }
    if matches!(&event.key, rune::Key::Escape) && s.connection_form && s.can_cancel_connection() {
        s.cancel_connection();
        true
    } else {
        false
    }
}

// Only viewer reads may coalesce. A future mutation queue needs its own durable ordering.
fn coalesce(first: Request, incoming: impl IntoIterator<Item = Request>) -> VecDeque<Request> {
    let mut pending = VecDeque::new();
    for request in std::iter::once(first).chain(incoming) {
        if matches!(&request.command, Command::Connect(_, _)) {
            pending.clear();
        } else if pending
            .back()
            .is_some_and(|r: &Request| !matches!(&r.command, Command::Connect(_, _)))
        {
            pending.pop_back();
        }
        pending.push_back(request);
    }
    pending
}

fn url_connection(endpoint: &str, token: Option<String>) -> Result<(Connection, String), String> {
    let mut url = url::Url::parse(endpoint).map_err(|_| "Enter a valid gateway URL")?;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Use a gateway URL without user credentials or fragments".into());
    }
    let mut url_token = None;
    for (key, value) in url.query_pairs() {
        if key != "token" || url_token.is_some() {
            return Err("Unsupported gateway URL parameters".into());
        }
        url_token = Some(value.into_owned());
    }
    url.set_query(None);
    let visible = url.to_string();
    Ok((
        Connection::Url {
            endpoint: visible.clone(),
            token: token.or(url_token),
        },
        visible,
    ))
}

#[path = "pages/integration.rs"]
mod page_integration;

pub fn settings_scene(s: &mut Viewer) {
    reader_scene(s);
    s.open_route(crate::pages::Route::Settings);
}
pub fn capabilities_scene(s: &mut Viewer) {
    reader_scene(s);
    s.open_route(crate::pages::Route::Capabilities);
}
pub fn messaging_scene(s: &mut Viewer) {
    reader_scene(s);
    s.open_route(crate::pages::Route::Messaging);
}
pub fn artifacts_scene(s: &mut Viewer) {
    reader_scene(s);
    s.open_route(crate::pages::Route::Artifacts);
}
pub fn cron_scene(s: &mut Viewer) {
    reader_scene(s);
    s.open_route(crate::pages::Route::Cron);
}

impl Viewer {
    fn save_client_preferences(&mut self) {
        if self.fixture {
            return;
        }
        self.remember_profile_appearance();
        let next = crate::client::Preferences {
            layout: self.layout.clone(),
            review: self.review_preferences.clone(),
            show_statusbar: self.show_statusbar,
            show_profile_rail: self.show_profile_rail,
            hidden_status_items: self.hidden_status_items.clone(),
            speak_replies: self.speak_replies,
            theme: self.theme,
            simple_mode: self.simple_mode,
            sidebar_width: self.sidebar.width(),
            sidebar_open: self.sidebar.is_open(),
            inspector_width: self.inspector.width(),
            inspector_open: if self.simple_mode {
                self.advanced_tools_open
            } else {
                self.inspector.is_open()
            },
        };
        if next != self.preferences {
            self.preferences = next.clone();
            match next.save() {
                Ok(()) => self.client_error = None,
                Err(error) => self.client_error = Some(error),
            }
        }
    }
}

#[path = "app/conversation.rs"]
mod conversation_actions;

fn synthetic_conversation(s: &mut Viewer, value: serde_json::Value) {
    let scope = s.composer_scope();
    let mut value = value;
    value["messages"] = serde_json::json!(
        s.messages
            .iter()
            .map(|m| serde_json::json!({"role":m.role,"content":m.content}))
            .collect::<Vec<_>>()
    );
    if let Err(error) = s.conversation.fixture(scope, &value) {
        s.compose_error = Some(error);
    }
}
pub fn working_scene(s: &mut Viewer) {
    reader_scene(s);
    synthetic_conversation(
        s,
        serde_json::json!({"running":true,"reasoning":"Checking the requested files and comparing the implementation.","tools":["Read source files","Run targeted checks"],"pending":[{"admission_id":"queued-example","input_id":"input-example","text":"Also check the light theme","status":"queued","controls":["cancel","steer"]}]}),
    );
}
pub fn decisions_scene(s: &mut Viewer) {
    reader_scene(s);
    synthetic_conversation(
        s,
        serde_json::json!({"running":true,"prompts":[{"kind":"approval","prompt_id":"approval-example","execution_generation":1,"description":"Allow the agent to edit the selected project files?","command":"apply_patch src/view.rs","choices":["once","session","deny"]},{"kind":"clarify","prompt_id":"clarify-example","execution_generation":1,"server_request":true,"questions":[{"qid":"theme","question":"Which appearances should be checked?","choices":["Light","Dark"],"multi_select":true},{"qid":"device","question":"Which window size?","choices":["Compact","Desktop"],"multi_select":false}],"answers":{}}]}),
    );
}

#[path = "app/audio.rs"]
mod audio_actions;

#[path = "app/plugins.rs"]
mod plugin_actions;
#[path = "app/profiles.rs"]
mod profile_actions;

pub fn bots_manager_scene(s: &mut Viewer) {
    reader_scene(s);
    s.open_route(crate::pages::Route::Bots);
}
pub fn connections_scene(s: &mut Viewer) {
    reader_scene(s);
    s.open_route(crate::pages::Route::Connections);
}

#[path = "app/files.rs"]
mod file_actions;

pub fn operations_scene(s: &mut Viewer) {
    reader_scene(s);
    s.open_route(crate::pages::Route::CommandCenter);
    if let Some(section) = argument("--ops-section")
        .filter(|s| ["sessions", "system", "usage", "maintenance"].contains(&s.as_str()))
    {
        s.operations_action(crate::operations::Action::Section(section));
    }
}

impl Viewer {
    pub fn toggle_health(&mut self) {
        self.ensure_page_owner();
        self.health_open = !self.health_open;
        if self.fixture {
            self.operations.fixture();
        } else if self.health_open {
            let e = self.operations.check_health();
            self.page_requests(e);
        }
    }
}

#[path = "app/tools.rs"]
mod tool_actions;
pub fn terminal_scene(s: &mut Viewer) {
    layout_scene(s);
    s.terminal.fixture();
}
pub fn file_preview_scene(s: &mut Viewer) {
    layout_scene(s);
    s.files.fixture(s.composer_scope());
    s.open_route(crate::pages::Route::FilePreview);
}

impl Viewer {
    pub fn current_connection(&self) -> Option<&Connection> {
        self.connection.as_ref()
    }
}

impl Viewer {
    pub fn can_remember_profile_preferences(&self) -> bool {
        self.page_owner.as_ref().is_some_and(|(epoch, profile)| {
            *epoch == self.connection_epoch && profile == &self.profile
        })
    }
}

pub fn review_scene(s: &mut Viewer) {
    layout_scene(s);
    s.review.fixture(s.composer_scope());
}
