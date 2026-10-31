//! Native terminal workspaces. Visibility is not lifecycle; captured connection,
//! profile and cwd own each tab, while reconnect never invents a new shell.
use crate::{composer::DraftScope, gateway::Connection};
use rune::Waker;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};
mod grid;
pub(crate) mod input;
mod persistence;
mod worker;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Starting,
    Open,
    Reconnecting,
    Detached,
    Exited,
    Closing,
    Error,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Local,
    Ssh,
    Remote,
    Agent,
}
pub struct Tab {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub shell: String,
    pub source: Source,
    pub status: Status,
    pub error: Option<String>,
    pub durable: Option<String>,
    pub process: Option<String>,
    pub grid: grid::Grid,
    pub generation: u64,
    pub preedit: String,
    pub(crate) handle: Option<worker::Handle>,
    pub(crate) last_snapshot: String,
    pub(crate) agent_output: String,
}
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active: Option<String>,
    pub error: Option<String>,
    pub(crate) epoch: u64,
    pub(crate) profile: String,
    pub(crate) profile_intent: crate::source::ProfileIntent,
    pub(crate) closed_processes: std::collections::HashSet<String>,
    pub(crate) process_snapshots: HashMap<String, serde_json::Value>,
}
impl Workspace {
    pub fn active(&self) -> Option<&Tab> {
        self.tabs
            .iter()
            .find(|t| Some(&t.id) == self.active.as_ref())
    }
    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        self.tabs
            .iter_mut()
            .find(|t| Some(&t.id) == self.active.as_ref())
    }
}
#[derive(Clone)]
struct Context {
    key: String,
    connection: Connection,
    scope: DraftScope,
    cwd: String,
    ssh: Option<crate::connections::Ssh>,
    ssh_windows: bool,
}
#[derive(Clone)]
pub enum Action {
    Start,
    StartLocal,
    Select(String),
    Close(String),
    Forget(String),
    Quote,
    FontSize(f32),
    Reconnect(String),
    CloseOthers(String),
    CloseAll,
    Clear,
    Scroll(i32),
    Resize(u16, u16),
    Input(Vec<u8>),
    Drop(Vec<PathBuf>),
    SshWindows(bool),
    SelectAll,
    HideMenu,
    Menu,
    Find(String),
    FindNext,
}
pub(crate) enum Reply {
    Event {
        owner: String,
        id: String,
        generation: u64,
        event: worker::Event,
    },
    Loaded(Result<Vec<persistence::SavedWorkspace>, String>),
    Saved(Result<(), String>),
}
pub struct Terminal {
    contexts: HashMap<String, Context>,
    session_owners: HashMap<DraftScope, String>,
    pub workspaces: HashMap<String, Workspace>,
    current: Option<String>,
    send: Sender<Reply>,
    receive: Receiver<Reply>,
    waker: Option<Waker>,
    persistence: Option<persistence::Worker>,
    dirty: bool,
    last_save: Instant,
    loaded: bool,
    synthetic: bool,
    pub menu: bool,
    pub menu_anchor: [f32; 2],
    pub bounds: [f32; 4],
    pub find: rune::TextEdit,
    pub find_match: Option<(u16, u16)>,
    pub font_size: f32,
}
impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}
impl Terminal {
    pub fn new() -> Self {
        let (send, receive) = mpsc::channel();
        Self {
            contexts: HashMap::new(),
            session_owners: HashMap::new(),
            workspaces: HashMap::new(),
            current: None,
            send,
            receive,
            waker: None,
            persistence: None,
            dirty: false,
            last_save: Instant::now(),
            loaded: false,
            synthetic: false,
            menu: false,
            menu_anchor: [20., 60.],
            bounds: [0., 0., 360., 600.],
            find: rune::TextEdit::new(""),
            find_match: None,
            font_size: 12.,
        }
    }
    pub fn init(&mut self, waker: Waker) {
        if self.synthetic {
            return;
        }
        self.waker = Some(waker.clone());
        self.persistence = Some(persistence::Worker::start(self.send.clone(), waker));
    }
    pub fn set_owner(
        &mut self,
        connection: Connection,
        scope: DraftScope,
        cwd: Option<String>,
        ssh: Option<crate::connections::Ssh>,
    ) -> Result<(), String> {
        if self.synthetic {
            return Ok(());
        }
        let cwd = cwd.unwrap_or_default();
        if cwd.contains(['\0', '\r', '\n']) {
            return Err("Invalid terminal working directory".into());
        }
        let fingerprint = crate::client::connection_key(&connection)?;
        let key = if scope.profile_intent.is_backend() {
            serde_json::json!([fingerprint, scope.profile, cwd]).to_string()
        } else {
            serde_json::json!([fingerprint, scope.profile, cwd, scope.profile_intent]).to_string()
        };
        self.workspaces
            .entry(key.clone())
            .or_insert_with(|| Workspace {
                tabs: vec![],
                active: None,
                error: None,
                epoch: scope.connection,
                profile: scope.profile.clone(),
                profile_intent: scope.profile_intent,
                closed_processes: Default::default(),
                process_snapshots: Default::default(),
            });
        let workspace = self.workspaces.get_mut(&key).unwrap();
        workspace.epoch = scope.connection;
        let windows = self.contexts.get(&key).is_some_and(|c| c.ssh_windows);
        self.session_owners.insert(scope.clone(), key.clone());
        self.contexts.insert(
            key.clone(),
            Context {
                key: key.clone(),
                connection,
                scope,
                cwd,
                ssh,
                ssh_windows: windows,
            },
        );
        self.current = Some(key);
        Ok(())
    }
    pub fn current(&self) -> Option<&Workspace> {
        self.current
            .as_ref()
            .and_then(|key| self.workspaces.get(key))
    }
    pub fn current_mut(&mut self) -> Option<&mut Workspace> {
        self.current
            .as_ref()
            .and_then(|key| self.workspaces.get_mut(key))
    }
    pub fn ssh(&self) -> bool {
        self.current
            .as_ref()
            .and_then(|key| self.contexts.get(key))
            .is_some_and(|c| c.ssh.is_some())
    }
    pub fn ssh_windows(&self) -> bool {
        self.current
            .as_ref()
            .and_then(|key| self.contexts.get(key))
            .is_some_and(|c| c.ssh_windows)
    }
    pub fn default_label(&self) -> &str {
        let Some(context) = self.current.as_ref().and_then(|k| self.contexts.get(k)) else {
            return "Start shell";
        };
        if context.ssh.is_some() {
            "Start SSH shell"
        } else if matches!(context.connection.transport(), Connection::Local { .. }) {
            "Start local shell"
        } else {
            "Start remote shell"
        }
    }
    pub fn action(&mut self, action: Action) {
        let result = self.perform(action);
        if let Err(error) = result {
            if let Some(workspace) = self.current_mut() {
                workspace.error = Some(error);
            }
        }
    }
    fn perform(&mut self, action: Action) -> Result<(), String> {
        match action {
            Action::Start | Action::StartLocal => {
                if self.synthetic {
                    return Err("Shell execution is disabled in synthetic scenes".into());
                }
                self.start(matches!(action, Action::StartLocal))?;
                self.menu = false;
            }
            Action::Select(id) => {
                if let Some(w) = self.current_mut() {
                    if w.tabs.iter().any(|t| t.id == id) {
                        w.active = Some(id);
                    }
                }
                self.menu = false;
            }
            Action::Close(id) => self.close(&id)?,
            Action::Forget(id) => {
                if let Some(w) = self.current_mut() {
                    if w.tabs
                        .iter()
                        .any(|t| t.id == id && t.status == Status::Open)
                    {
                        return Err("Close a connected shell before forgetting its tab".into());
                    }
                    w.tabs.retain(|t| t.id != id);
                    if w.active.as_deref() == Some(&id) {
                        w.active = w.tabs.last().map(|t| t.id.clone());
                    }
                }
            }
            Action::Quote => {}
            Action::FontSize(size) => {
                self.font_size = size.clamp(8., 28.);
                let rect = self.bounds;
                self.layout(rect);
            }
            Action::Reconnect(id) => self.reconnect(&id)?,
            Action::CloseOthers(keep) => {
                let ids = self
                    .current()
                    .map(|w| {
                        w.tabs
                            .iter()
                            .filter(|t| t.id != keep)
                            .map(|t| t.id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for id in ids {
                    self.close(&id)?;
                }
            }
            Action::CloseAll => {
                let ids = self
                    .current()
                    .map(|w| w.tabs.iter().map(|t| t.id.clone()).collect::<Vec<_>>())
                    .unwrap_or_default();
                for id in ids {
                    self.close(&id)?;
                }
            }
            Action::Clear => {
                if let Some(tab) = self.current_mut().and_then(Workspace::active_mut) {
                    let (rows, cols) = tab.grid.screen().size();
                    tab.grid = grid::Grid::new(rows, cols);
                }
            }
            Action::Scroll(lines) => {
                if let Some(tab) = self.current_mut().and_then(Workspace::active_mut) {
                    tab.grid.scroll(lines);
                }
            }
            Action::Resize(cols, rows) => {
                let (cols, rows) = (cols.clamp(2, 400), rows.clamp(2, 200));
                if let Some(tab) = self.current_mut().and_then(Workspace::active_mut) {
                    if tab.grid.screen().size() != (rows, cols) {
                        tab.grid.resize(rows, cols);
                        if let Some(handle) = &tab.handle {
                            handle
                                .send
                                .send(worker::Command::Resize(cols, rows))
                                .map_err(|_| "The terminal worker stopped")?;
                        }
                    }
                }
            }
            Action::Input(bytes) => self.write(bytes)?,
            Action::Drop(paths) => {
                if self.synthetic {
                    return Err("File drops are disabled in synthetic scenes".into());
                }
                if let Some(tab) = self.current().and_then(Workspace::active) {
                    if tab.source == Source::Agent {
                        return Err("Agent process mirrors are read-only".into());
                    }
                    if tab.status != Status::Open {
                        return Err(
                            "The shell is disconnected; the file drop was not started".into()
                        );
                    }
                    tab.handle
                        .as_ref()
                        .ok_or("The shell is disconnected")?
                        .send
                        .send(worker::Command::Drop(paths))
                        .map_err(|_| "The terminal worker stopped")?;
                }
            }
            Action::SshWindows(value) => {
                if let Some(context) = self.current.as_ref().and_then(|k| self.contexts.get_mut(k))
                {
                    context.ssh_windows = value;
                }
            }
            Action::SelectAll => {
                if let Some(tab) = self.current_mut().and_then(Workspace::active_mut) {
                    tab.grid.select_all();
                }
            }
            Action::Menu => self.menu = !self.menu,
            Action::HideMenu => self.menu = false,
            Action::Find(text) => {
                self.find.replace_text(text);
                self.find_next();
            }
            Action::FindNext => self.find_next(),
        }
        self.dirty = true;
        Ok(())
    }
    fn target(context: &Context, source: Source) -> Result<worker::Target, String> {
        Ok(match source {
            Source::Local => worker::Target::Local,
            Source::Remote => worker::Target::Remote(context.connection.clone()),
            Source::Ssh => worker::Target::Ssh {
                config: context
                    .ssh
                    .clone()
                    .ok_or("This connection has no SSH destination")?,
                windows: context.ssh_windows,
                connection: context.connection.clone(),
            },
            Source::Agent => return Err("An agent mirror cannot start a shell".into()),
        })
    }
    fn start(&mut self, local: bool) -> Result<(), String> {
        let context = self
            .current
            .as_ref()
            .and_then(|k| self.contexts.get(k))
            .cloned()
            .ok_or("Connect to an agent before opening its terminal workspace")?;
        let source = if local {
            Source::Local
        } else if context.ssh.is_some() {
            Source::Ssh
        } else if matches!(context.connection.transport(), Connection::Local { .. }) {
            Source::Local
        } else {
            Source::Remote
        };
        let id = uuid::Uuid::new_v4().to_string();
        let mut tab = Tab::new(
            id.clone(),
            source,
            if local && !matches!(context.connection.transport(), Connection::Local { .. }) {
                String::new()
            } else {
                context.cwd.clone()
            },
        );
        self.spawn_tab(&context, &mut tab)?;
        let w = self.workspaces.get_mut(&context.key).unwrap();
        w.active = Some(id);
        w.tabs.push(tab);
        w.error = None;
        Ok(())
    }
    fn spawn_tab(&self, context: &Context, tab: &mut Tab) -> Result<(), String> {
        if self.synthetic {
            return Err("Shell execution is disabled in synthetic scenes".into());
        }
        let waker = self
            .waker
            .clone()
            .ok_or("The terminal transport is not initialized")?;
        tab.generation += 1;
        let (rows, cols) = tab.grid.screen().size();
        let start = worker::Start {
            target: Self::target(context, tab.source)?,
            profile: context.scope.profile.clone(),
            profile_intent: context.scope.profile_intent,
            cwd: tab.cwd.clone(),
            rows,
            cols,
            durable: tab.durable.clone(),
        };
        tab.status = Status::Starting;
        tab.error = None;
        tab.handle = Some(worker::spawn(
            context.key.clone(),
            tab.id.clone(),
            tab.generation,
            start,
            self.send.clone(),
            waker,
        ));
        Ok(())
    }
    fn reconnect(&mut self, id: &str) -> Result<(), String> {
        if self.synthetic {
            return Err("Shell execution is disabled in synthetic scenes".into());
        }
        let context = self
            .current
            .as_ref()
            .and_then(|k| self.contexts.get(k))
            .cloned()
            .ok_or("This terminal has no active owner")?;
        let index = self
            .current()
            .and_then(|w| w.tabs.iter().position(|t| t.id == id))
            .ok_or("The terminal tab is gone")?;
        let mut tab = self
            .workspaces
            .get_mut(&context.key)
            .unwrap()
            .tabs
            .remove(index);
        if tab.source != Source::Remote || tab.durable.is_none() {
            self.workspaces
                .get_mut(&context.key)
                .unwrap()
                .tabs
                .insert(index, tab);
            return Err(
                "Start a new shell explicitly; this tab has no resumable remote identity".into(),
            );
        }
        tab.handle.take();
        let result = self.spawn_tab(&context, &mut tab);
        self.workspaces
            .get_mut(&context.key)
            .unwrap()
            .tabs
            .insert(index, tab);
        result
    }
    fn close(&mut self, id: &str) -> Result<(), String> {
        let Some(w) = self.current_mut() else {
            return Ok(());
        };
        let Some(index) = w.tabs.iter().position(|t| t.id == id) else {
            return Ok(());
        };
        let tab = &mut w.tabs[index];
        if tab.source == Source::Remote && tab.durable.is_some() {
            if tab.handle.is_none() {
                self.reconnect(id)?;
            }
            let tab = self
                .current_mut()
                .unwrap()
                .tabs
                .iter_mut()
                .find(|t| t.id == id)
                .unwrap();
            tab.status = Status::Closing;
            tab.handle
                .as_ref()
                .unwrap()
                .send
                .send(worker::Command::Close)
                .map_err(|_| "The terminal worker stopped before confirming close")?;
            return Ok(());
        }
        if let Some(process) = &tab.process {
            w.closed_processes.insert(process.clone());
        }
        if let Some(handle) = tab.handle.take() {
            let _ = handle.send.send(worker::Command::Close);
        }
        w.tabs.remove(index);
        if w.active.as_deref() == Some(id) {
            w.active = w.tabs.last().map(|t| t.id.clone());
        }
        Ok(())
    }
    fn write(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        if self.synthetic {
            return Err("Terminal input is disabled in synthetic scenes".into());
        }
        if bytes.len() > 1024 * 1024 {
            return Err("Terminal input exceeds 1 MiB".into());
        }
        let tab = self
            .current_mut()
            .and_then(Workspace::active_mut)
            .ok_or("Choose a terminal tab")?;
        if tab.source == Source::Agent {
            return Err("Agent process mirrors are read-only".into());
        }
        if tab.status != Status::Open {
            return Err("The terminal is not connected; input was not sent".into());
        }
        tab.grid.parser.screen_mut().set_scrollback(0);
        tab.grid.selection = None;
        tab.grid.selection_all = false;
        tab.handle
            .as_ref()
            .ok_or("The terminal worker is unavailable")?
            .send
            .send(worker::Command::Input(bytes))
            .map_err(|_| "The terminal worker stopped".into())
    }
    pub fn layout(&mut self, rect: [f32; 4]) {
        self.bounds = rect;
        let cols = ((rect[2] - 12.) / (self.font_size * 0.6)).floor().max(2.) as u16;
        let rows = ((rect[3] - 8.) / (self.font_size * 1.3)).floor().max(2.) as u16;
        self.action(Action::Resize(cols, rows));
    }
    pub fn selection(&self) -> String {
        self.current()
            .and_then(Workspace::active)
            .map(|t| t.grid.selected())
            .unwrap_or_default()
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(tab) = self.current().and_then(Workspace::active) {
            self.action(Action::Input(input::paste(
                text,
                tab.grid.screen().bracketed_paste(),
            )));
        }
    }
    fn find_next(&mut self) {
        let needle = self.find.text.clone();
        self.find_match = self
            .current_mut()
            .and_then(Workspace::active_mut)
            .and_then(|tab| tab.grid.find_next(&needle));
    }
    pub fn rehome(&mut self, origin: &DraftScope, target: DraftScope) {
        if origin.connection != target.connection
            || origin.profile != target.profile
            || origin.profile_intent != target.profile_intent
        {
            return;
        }
        if let Some(owner) = self.session_owners.get(origin).cloned() {
            self.session_owners.insert(target, owner);
        }
    }
    pub fn close_connection(&mut self, epoch: u64) {
        self.session_owners
            .retain(|scope, _| scope.connection != epoch);
        for w in self.workspaces.values_mut().filter(|w| w.epoch == epoch) {
            for tab in &mut w.tabs {
                tab.generation = tab.generation.wrapping_add(1);
                tab.handle.take();
                if !matches!(tab.source, Source::Agent) {
                    tab.status = Status::Detached;
                }
            }
        }
        self.dirty = true;
    }
    pub fn close_profile(&mut self, epoch: u64, profile: &str) {
        self.close_profile_intent(epoch, profile, crate::source::ProfileIntent::Backend);
    }
    pub fn close_profile_intent(
        &mut self,
        epoch: u64,
        profile: &str,
        intent: crate::source::ProfileIntent,
    ) {
        self.session_owners.retain(|scope, _| {
            !(scope.connection == epoch
                && scope.profile == profile
                && scope.profile_intent == intent)
        });
        for w in self
            .workspaces
            .values_mut()
            .filter(|w| w.epoch == epoch && w.profile == profile && w.profile_intent == intent)
        {
            for tab in &mut w.tabs {
                tab.generation = tab.generation.wrapping_add(1);
                tab.handle.take();
                tab.status = Status::Detached;
            }
        }
        self.dirty = true;
    }
    pub fn poll(&mut self) -> bool {
        if self.synthetic {
            while self.receive.try_recv().is_ok() {}
            return false;
        }
        let mut changed = false;
        for reply in self.receive.try_iter().collect::<Vec<_>>() {
            changed = true;
            match reply {
                Reply::Loaded(result) => {
                    self.loaded = result.is_ok();
                    match result {
                        Ok(saved) => self.restore(saved),
                        Err(error) => {
                            if let Some(w) = self.current_mut() {
                                w.error = Some(error);
                            }
                        }
                    }
                }
                Reply::Saved(result) => {
                    if let Err(error) = result {
                        if let Some(w) = self.current_mut() {
                            w.error = Some(error);
                        }
                    }
                }
                Reply::Event {
                    owner,
                    id,
                    generation,
                    event,
                } => {
                    let Some(w) = self.workspaces.get_mut(&owner) else {
                        continue;
                    };
                    if !w
                        .tabs
                        .iter()
                        .any(|t| t.id == id && t.generation == generation)
                    {
                        continue;
                    }
                    let tab = w.tabs.iter_mut().find(|t| t.id == id).unwrap();
                    match event {
                        worker::Event::Started {
                            shell,
                            cwd,
                            durable,
                        } => {
                            tab.shell = shell.clone();
                            tab.title = shell;
                            tab.cwd = cwd;
                            tab.durable = durable;
                            tab.status = Status::Open;
                            tab.error = None;
                        }
                        worker::Event::Data(bytes, replay) => {
                            let responses = tab.grid.process(&bytes, replay);
                            if let Some(title) = tab.grid.parser.callbacks_mut().title.take() {
                                tab.title = title;
                            }
                            if let Some(cwd) = tab.grid.parser.callbacks_mut().cwd.take() {
                                tab.cwd = cwd;
                            }
                            if let Some(handle) = &tab.handle {
                                for response in responses {
                                    let _ = handle.send.send(worker::Command::Input(response));
                                }
                            }
                        }
                        worker::Event::Reset(trimmed) => {
                            let (r, c) = tab.grid.screen().size();
                            tab.grid = grid::Grid::new(r, c);
                            if trimmed {
                                tab.grid
                                    .process(b"[Earlier terminal output was trimmed]\r\n", true);
                            }
                        }
                        worker::Event::State(status) => tab.status = status,
                        worker::Event::Exit(code) => {
                            tab.status = Status::Exited;
                            tab.handle.take();
                            if let Some(code) = code {
                                tab.grid.process(
                                    format!("\r\n[Process exited: {code}]\r\n").as_bytes(),
                                    true,
                                );
                            }
                        }
                        worker::Event::Warning(error) => tab.error = Some(error),
                        worker::Event::Error(error) => {
                            tab.status = Status::Error;
                            tab.error = Some(error);
                            tab.handle.take();
                        }
                        worker::Event::Cwd(cwd) => tab.cwd = cwd,
                        worker::Event::Closed => {
                            w.tabs.retain(|t| t.id != id);
                            if w.active.as_deref() == Some(&id) {
                                w.active = w.tabs.last().map(|t| t.id.clone());
                            }
                        }
                    }
                    self.dirty = true;
                }
            }
        }
        if self.dirty && self.loaded && self.last_save.elapsed() > Duration::from_secs(1) {
            if let Some(persist) = &self.persistence {
                persist.save(self.saved());
            }
            self.dirty = false;
            self.last_save = Instant::now();
        }
        changed
    }
    pub fn fixture(&mut self) {
        self.persistence = None;
        self.synthetic = true;
        self.loaded = true;
        let key = "synthetic".to_owned();
        let mut tab = Tab::new(
            "fixture-shell".into(),
            Source::Local,
            "/synthetic/workspace".into(),
        );
        tab.status = Status::Open;
        tab.title = "bash".into();
        tab.grid.process(b"\x1b[32mhermes\x1b[0m \x1b[34m~/workspace\x1b[0m\r\n$ cargo check\r\n\x1b[32mFinished\x1b[0m dev profile in 0.42s\r\n$ ",true);
        self.workspaces.insert(
            key.clone(),
            Workspace {
                active: Some(tab.id.clone()),
                tabs: vec![tab],
                error: None,
                epoch: 0,
                profile: "synthetic".into(),
                profile_intent: crate::source::ProfileIntent::Backend,
                closed_processes: Default::default(),
                process_snapshots: Default::default(),
            },
        );
        self.current = Some(key);
    }
}
impl Tab {
    fn new(id: String, source: Source, cwd: String) -> Self {
        Self {
            id,
            title: if source == Source::Agent {
                "Agent process"
            } else {
                "Shell"
            }
            .into(),
            cwd,
            shell: String::new(),
            source,
            status: Status::Detached,
            error: None,
            durable: None,
            process: None,
            grid: grid::Grid::new(24, 80),
            generation: 0,
            preedit: String::new(),
            handle: None,
            last_snapshot: String::new(),
            agent_output: String::new(),
        }
    }
}
impl crate::app::Viewer {
    pub fn terminal_action(&mut self, action: Action) {
        if matches!(action, Action::Quote) {
            let selection = self.terminal.selection();
            if selection.trim().is_empty() {
                return;
            }
            let label = self
                .terminal
                .current()
                .and_then(Workspace::active)
                .map(|tab| tab.title.clone())
                .unwrap_or_else(|| "Terminal".into());
            let fence = if selection.contains("```") {
                "````"
            } else {
                "```"
            };
            let quote = format!("\n\n[Terminal: {label}]\n{fence}text\n{selection}\n{fence}\n");
            let scope = self.composer_scope();
            self.composer.draft_mut(scope).text.insert(&quote);
            self.terminal.menu = false;
        } else {
            self.terminal.action(action);
        }
    }
}

mod agent;
#[cfg(test)]
mod tests;
