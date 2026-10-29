use super::*;
#[derive(Clone)]
pub enum Action {
    Section(String),
    Refresh,
    LogFile(String),
    LogLevel(String),
    UsagePeriod(u16),
    FollowLogs,
    CheckUpdate,
    Start(Kind),
    Confirm,
    CancelConfirm,
    PollAction,
    StopWatching,
    DebugShare,
    ToggleCurator,
    ResetMemory(String),
    SaveLogs(PathBuf),
    Export(SidebarSession, PathBuf),
    DownloadBackup(PathBuf),
    SessionOpen(String, String),
    SessionPin(String, String, bool),
    SessionDelete(String, String),
    SessionLoadMore,
    Reconnect,
    OpenSettings,
    OpenSystem,
}
impl Operations {
    pub fn act(&mut self, a: Action) -> Vec<PageEffect> {
        if self.demo
            && !matches!(
                a,
                Action::Section(_)
                    | Action::Refresh
                    | Action::LogFile(_)
                    | Action::LogLevel(_)
                    | Action::UsagePeriod(_)
                    | Action::FollowLogs
                    | Action::CancelConfirm
            )
        {
            return vec![];
        }
        match a {
            Action::Section(s) => {
                if !["sessions", "system", "usage", "maintenance"].contains(&s.as_str()) {
                    return vec![];
                }
                self.section = s;
                self.select = None;
                self.load()
            }
            Action::Refresh => self.load(),
            Action::LogFile(v) | Action::LogLevel(v) => {
                if ["agent", "errors", "gateway", "desktop"].contains(&v.as_str()) {
                    self.log_file = v;
                } else if ["ALL", "INFO", "WARNING", "ERROR"].contains(&v.as_str()) {
                    self.log_level = v;
                } else {
                    return vec![];
                }
                self.select = None;
                self.logs_loading = false;
                self.logs.clear();
                if self.demo { vec![] } else { self.logs_load() }
            }
            Action::UsagePeriod(days) => {
                if ![7, 30, 90].contains(&days) {
                    return vec![];
                }
                self.period = days;
                self.select = None;
                if self.demo { vec![] } else { self.usage_load() }
            }
            Action::FollowLogs => {
                self.log_follow = !self.log_follow;
                vec![]
            }
            Action::CheckUpdate => self.update_load(true),
            Action::Start(kind) => {
                if self.starting || self.task.as_ref().is_some_and(|t| t.running) {
                    return vec![];
                }
                if kind == Kind::Update
                    && self
                        .update
                        .as_ref()
                        .is_some_and(|u| u["can_apply"].as_bool() == Some(false))
                {
                    self.system_error=Some(self.update.as_ref().and_then(|u|u["message"].as_str()).unwrap_or("This external installation cannot apply this update from the client").into());
                    return vec![];
                }
                if kind == Kind::Restart
                    && self
                        .status
                        .as_ref()
                        .and_then(|v| v["gateway_shared_with"].as_array())
                        .is_some_and(|n| {
                            n.iter()
                                .filter_map(Value::as_str)
                                .collect::<BTreeSet<_>>()
                                .len()
                                > 1
                        })
                {
                    self.confirm = Some(Confirmation::Action(kind));
                    return vec![];
                }
                self.launch(kind)
            }
            Action::Confirm => match self.confirm.take() {
                Some(Confirmation::Action(k)) => self.launch(k),
                Some(Confirmation::Memory(target)) => {
                    if self.memory_busy {
                        return vec![];
                    }
                    self.memory_busy = true;
                    self.maintenance_error = None;
                    vec![(
                        "ops.memory-reset".into(),
                        PageRequest::post("/api/memory/reset", json!({"target":target})),
                    )]
                }
                None => vec![],
            },
            Action::CancelConfirm => {
                self.confirm = None;
                vec![]
            }
            Action::ResetMemory(target) => {
                if ["memory", "user", "all"].contains(&target.as_str()) && !self.memory_busy {
                    self.confirm = Some(Confirmation::Memory(target));
                }
                vec![]
            }
            Action::PollAction => {
                if let Some(t) = &mut self.task {
                    t.polls = 0;
                    t.error = None;
                    t.running = true;
                }
                self.task_poll()
            }
            Action::StopWatching => {
                if let Some(t) = &mut self.task {
                    t.running = false;
                    t.polling = false;
                    t.error = Some(
                        "Stopped watching. The operation may still be running on the agent.".into(),
                    );
                }
                vec![]
            }
            Action::DebugShare => {
                if self.sharing {
                    return vec![];
                }
                self.sharing = true;
                self.share = None;
                self.maintenance_error = None;
                vec![(
                    "ops.debug-share".into(),
                    PageRequest::post("/api/ops/debug-share", json!({})).timeout_secs(120),
                )]
            }
            Action::ToggleCurator => {
                if self.curator_busy {
                    return vec![];
                }
                let Some(v) = &self.curator else {
                    return vec![];
                };
                if v["enabled"].as_bool() != Some(true) {
                    return vec![];
                }
                let paused = v["paused"].as_bool() != Some(true);
                self.pending_curator = Some(paused);
                self.curator_busy = true;
                vec![(
                    "ops.curator-pause".into(),
                    PageRequest::put("/api/curator/paused", json!({"paused":paused})),
                )]
            }
            Action::SaveLogs(path) => {
                if self.saving_file {
                    return vec![];
                }
                let text = self.visible_logs().join("\n");
                self.save_file(path, files::Payload::Text(text));
                vec![]
            }
            Action::Export(row, path) => {
                if self.exporting || self.saving_file {
                    return vec![];
                }
                if row.id.is_empty() || row.connection_id.is_some() {
                    self.export_error =
                        Some("Connect the gateway owning this session before exporting it".into());
                    return vec![];
                }
                let id = row.id.clone();
                self.export_error = None;
                self.exporting = true;
                self.export = Some(Export {
                    row,
                    path,
                    messages: vec![],
                    resolved: id,
                    offset: 0,
                    bytes: 0,
                    pages: 0,
                    seen: BTreeSet::new(),
                });
                self.export_page()
            }
            Action::DownloadBackup(path) => {
                if self.downloading {
                    return vec![];
                }
                let Some(task) = &self.task else {
                    return vec![];
                };
                let Some(archive) = task.archive.as_ref().filter(|_| {
                    task.kind == Kind::Backup && task.settled && task.exit_code == Some(0)
                }) else {
                    return vec![];
                };
                self.downloading = true;
                vec![(
                    "ops.backup-download".into(),
                    PageRequest::get("/api/fs/download")
                        .query("path", archive.clone())
                        .download_to(path)
                        .timeout_secs(3600),
                )]
            }
            _ => vec![],
        }
    }
    fn launch(&mut self, kind: Kind) -> Vec<PageEffect> {
        if self.starting {
            return vec![];
        }
        self.starting = true;
        self.pending_kind = Some(kind);
        self.system_error = None;
        self.maintenance_error = None;
        self.notice = None;
        vec![(
            "ops.action-start".into(),
            PageRequest::post(kind.endpoint(), json!({})).timeout_secs(60),
        )]
    }
    pub(super) fn export_page(&self) -> Vec<PageEffect> {
        let Some(e) = &self.export else {
            return vec![];
        };
        vec![(
            "ops.export-page".into(),
            PageRequest::get(format!("/api/sessions/{}/messages", segment(&e.row.id)))
                .query("limit", 500)
                .query("offset", e.offset)
                .query("order", "oldest")
                .query("include_compacted", true)
                .profile(e.row.profile())
                .timeout_secs(60),
        )]
    }
}
impl crate::app::Viewer {
    pub fn operations_action(&mut self, a: Action) {
        match &a {
            Action::OpenSystem => {
                self.open_route(crate::pages::Route::CommandCenter);
                self.operations_action(Action::Section("system".into()));
                return;
            }
            Action::Refresh if self.operations.section == "sessions" => {
                self.sidebar_action(crate::sidebar::Action::Refresh);
                return;
            }
            Action::Reconnect => {
                self.reconnect();
                return;
            }
            Action::OpenSettings => {
                if self.settings.scope_profile.is_some() && self.settings.blocks_owner_change() {
                    self.operations.system_error = Some(
                        "Save or discard the scoped Settings draft before configuring this profile"
                            .into(),
                    );
                    return;
                }
                self.settings_action(crate::pages::settings::Action::Scope(None));
                self.open_route(crate::pages::Route::Settings);
                self.settings_action(crate::pages::settings::Action::Section("model".into()));
                return;
            }
            Action::SessionOpen(p, id) => {
                self.sidebar_action(crate::sidebar::Action::Open(p.clone(), id.clone()));
                return;
            }
            Action::SessionPin(p, id, next) => {
                self.sidebar_action(crate::sidebar::Action::Pin(p.clone(), id.clone(), *next));
                return;
            }
            Action::SessionDelete(p, id) => {
                self.sidebar_action(crate::sidebar::Action::Delete(p.clone(), id.clone()));
                return;
            }
            Action::SessionLoadMore => {
                self.sidebar_action(crate::sidebar::Action::LoadMore("recents".into()));
                self.sidebar_action(crate::sidebar::Action::LoadMore("messaging".into()));
                self.sidebar_action(crate::sidebar::Action::LoadMore("cron".into()));
                return;
            }
            Action::LogFile(_) | Action::LogLevel(_) => self.invalidate_page_requests("ops.logs."),
            Action::UsagePeriod(_) => self.invalidate_page_requests("ops.usage."),
            Action::StopWatching => self.invalidate_page_requests("ops.action-status"),
            _ => {}
        }
        let effects = self.operations.act(a);
        self.page_requests(effects);
    }
}
