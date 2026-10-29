use super::*;
#[derive(Serialize, Deserialize)]
pub(crate) struct SavedWorkspace {
    pub key: String,
    pub profile: String,
    pub tabs: Vec<SavedTab>,
    pub active: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct SavedTab {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub shell: String,
    pub source: Source,
    pub durable: Option<String>,
    pub rows: u16,
    pub cols: u16,
    pub snapshot: Vec<u8>,
}
enum Command {
    Save(Vec<SavedWorkspace>),
}
pub(crate) struct Worker {
    send: Sender<Command>,
}
impl Worker {
    pub fn start(replies: Sender<Reply>, waker: Waker) -> Self {
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let path = crate::client::state_dir().map(|p| p.join("terminal-workspaces.json"));
            let load = path.as_ref().map_err(Clone::clone).and_then(|p| read(p));
            let _ = replies.send(Reply::Loaded(load));
            waker();
            while let Ok(Command::Save(mut saved)) = receive.recv() {
                while let Ok(Command::Save(newer)) = receive.try_recv() {
                    saved = newer;
                }
                let result = path.as_ref().map_err(Clone::clone).and_then(|p| {
                    serde_json::to_vec(&saved)
                        .map_err(|_| "Terminal state could not be serialized".into())
                        .and_then(|bytes| crate::client::private_write(p, &bytes))
                });
                let _ = replies.send(Reply::Saved(result));
                waker();
            }
        });
        Self { send }
    }
    pub fn save(&self, saved: Vec<SavedWorkspace>) {
        let _ = self.send.send(Command::Save(saved));
    }
}
fn read(path: &std::path::Path) -> Result<Vec<SavedWorkspace>, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.len() > 16 * 1024 * 1024 {
                return Err("Saved terminal state is too large".into());
            }
            serde_json::from_slice(&bytes)
                .map_err(|_| "Saved terminal state is invalid; it was not overwritten".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(_) => Err("Saved terminal state could not be read".into()),
    }
}
impl Terminal {
    pub(crate) fn saved(&self) -> Vec<SavedWorkspace> {
        self.workspaces
            .iter()
            .map(|(key, w)| SavedWorkspace {
                key: key.clone(),
                profile: w.profile.clone(),
                active: w.active.clone(),
                tabs: w
                    .tabs
                    .iter()
                    .filter(|t| t.source != Source::Agent)
                    .map(|t| {
                        let (rows, cols) = t.grid.screen().size();
                        let snapshot = if t.source == Source::Remote {
                            vec![]
                        } else {
                            t.grid.snapshot()
                        };
                        SavedTab {
                            id: t.id.clone(),
                            title: t.title.clone(),
                            cwd: t.cwd.clone(),
                            shell: t.shell.clone(),
                            source: t.source,
                            durable: t.durable.clone(),
                            rows,
                            cols,
                            snapshot: if t.source == Source::Remote {
                                vec![]
                            } else {
                                snapshot
                            },
                        }
                    })
                    .collect(),
            })
            .collect()
    }
    pub(crate) fn restore(&mut self, saved: Vec<SavedWorkspace>) {
        for saved in saved {
            let w = self
                .workspaces
                .entry(saved.key)
                .or_insert_with(|| Workspace {
                    tabs: vec![],
                    active: None,
                    error: None,
                    epoch: 0,
                    profile: saved.profile,
                    closed_processes: Default::default(),
                    process_snapshots: Default::default(),
                });
            for saved in saved.tabs {
                if w.tabs.iter().any(|t| t.id == saved.id)
                    || saved.source == Source::Agent
                    || saved.rows > 200
                    || saved.cols > 400
                    || saved.rows < 2
                    || saved.cols < 2
                    || saved.snapshot.len() > 1024 * 1024
                {
                    continue;
                }
                let mut tab = Tab::new(saved.id, saved.source, saved.cwd);
                tab.title = saved.title;
                tab.shell = saved.shell;
                tab.durable = saved.durable;
                tab.grid = grid::Grid::new(saved.rows, saved.cols);
                tab.grid.process(&saved.snapshot, true);
                w.tabs.push(tab);
            }
            if w.active.is_none() {
                w.active = saved
                    .active
                    .filter(|id| w.tabs.iter().any(|t| &t.id == id))
                    .or_else(|| w.tabs.first().map(|t| t.id.clone()));
            }
        }
    }
}
