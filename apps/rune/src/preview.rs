//! Shared native file previews. Targets freeze the agent owner before any asynchronous work.
pub(crate) mod image;
mod loader;
mod proof;
#[cfg(test)]
mod tests;
use crate::{
    composer::DraftScope,
    gateway::{Connection, stream::Cancel},
    media::TempMedia,
};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
};

pub use proof::run_if_proof_mode;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Pdf,
    Audio,
    Video,
    Html,
}
impl Kind {
    pub fn for_path(path: &str) -> Option<Self> {
        let path = path.split(['?', '#']).next().unwrap_or(path);
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" => Self::Image,
            "pdf" => Self::Pdf,
            "wav" | "mp3" | "m4a" | "ogg" | "flac" | "aac" => Self::Audio,
            "mp4" | "webm" | "mov" | "mkv" | "m4v" => Self::Video,
            "html" | "htm" => Self::Html,
            _ => return None,
        })
    }
    pub fn available(self) -> bool {
        self == Self::Image
    }
}
#[derive(Clone)]
pub enum Source {
    AgentPath(String),
    RemoteUrl(String),
    Inline { mime: String, bytes: Arc<[u8]> },
}
#[derive(Clone)]
pub struct Target {
    pub owner: DraftScope,
    pub source: Source,
    pub label: String,
    pub kind: Kind,
}
pub struct Document {
    pub id: u64,
    pub target: Target,
    pub loading: bool,
    pub saving: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub image: Option<rune::Image>,
    pub camera: image::Camera,
    pub original: Option<Arc<Original>>,
    cancel: Cancel,
}
pub struct Original {
    pub path: PathBuf,
    pub size: u64,
    _directory: TempMedia,
}
#[derive(Clone)]
pub enum Action {
    Close,
    Retry,
    Fit,
    ActualSize,
    Zoom(f32),
    SaveOriginal(PathBuf),
    SaveDialogError,
}
struct Task {
    id: u64,
    connection: Connection,
    target: Target,
    cancel: Cancel,
}
enum Reply {
    Loaded {
        id: u64,
        owner: DraftScope,
        result: Result<loader::Loaded, String>,
    },
    Saved {
        id: u64,
        owner: DraftScope,
        result: Result<(), String>,
    },
}
pub struct Service {
    active: Option<Document>,
    connection: Option<Connection>,
    pending: Option<Task>,
    running: Vec<Cancel>,
    inflight: usize,
    sequence: u64,
    send: mpsc::SyncSender<Reply>,
    receive: mpsc::Receiver<Reply>,
    cache: Option<rune::ImageCache>,
    waker: Option<rune::Waker>,
}
impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}
impl Service {
    pub fn new() -> Self {
        let (send, receive) = mpsc::sync_channel(3);
        Self {
            active: None,
            connection: None,
            pending: None,
            running: vec![],
            inflight: 0,
            sequence: 0,
            send,
            receive,
            cache: None,
            waker: None,
        }
    }
    pub fn init(&mut self, waker: rune::Waker) {
        self.cache = Some(rune::ImageCache::new(waker.clone()));
        self.waker = Some(waker);
    }
    pub fn active(&self) -> Option<&Document> {
        self.active.as_ref()
    }
    pub fn active_mut(&mut self, id: u64) -> Option<&mut Document> {
        self.active.as_mut().filter(|d| d.id == id)
    }
    pub fn open(&mut self, connection: Connection, target: Target) {
        self.close();
        self.sequence = self.sequence.saturating_add(1);
        let cancel = Cancel::default();
        let id = self.sequence;
        let available = target.kind.available();
        self.active=Some(Document {id,target:target.clone(),loading:available,saving:false,
            error:(!available).then(||match target.kind {
                Kind::Pdf=>"PDF preview needs the packaged PDF runtime. Download the original to open it.",
                Kind::Audio|Kind::Video=>"Playback needs the packaged media runtime. Download the original to open it.",
                Kind::Html=>"Interactive HTML preview is awaiting an isolated native guest host. Source remains available in Files.",
                Kind::Image=>unreachable!(),
            }.into()),notice:None,image:None,camera:image::Camera::default(),original:None,cancel:cancel.clone()});
        self.connection = Some(connection.clone());
        if available {
            self.pending = Some(Task {
                id,
                connection,
                target,
                cancel,
            });
            self.start_pending();
        }
    }
    fn start_pending(&mut self) {
        if self.inflight >= 2 {
            return;
        }
        let Some(waker) = self.waker.clone() else {
            return;
        };
        let Some(task) = self.pending.take() else {
            return;
        };
        self.inflight += 1;
        self.running.push(task.cancel.clone());
        let send = self.send.clone();
        std::thread::spawn(move || {
            let result = loader::load(&task.connection, &task.target, &task.cancel);
            if send
                .send(Reply::Loaded {
                    id: task.id,
                    owner: task.target.owner,
                    result,
                })
                .is_ok()
            {
                waker();
            }
        });
    }
    pub fn close(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancel.cancel();
        }
        self.pending = None;
        self.connection = None;
        self.running.retain(|c| !c.cancelled());
    }
    pub fn close_owner(&mut self, owner: &DraftScope) {
        if self
            .active
            .as_ref()
            .is_some_and(|d| &d.target.owner == owner)
        {
            self.close();
        }
    }
    pub fn close_profile(&mut self, connection: u64, profile: &str) {
        if self.active.as_ref().is_some_and(|d| {
            d.target.owner.connection == connection && d.target.owner.profile == profile
        }) {
            self.close();
        }
    }
    pub fn close_connection(&mut self, connection: u64) {
        if self
            .active
            .as_ref()
            .is_some_and(|d| d.target.owner.connection == connection)
        {
            self.close();
        }
    }
    pub fn action(&mut self, id: u64, action: Action) {
        if !self.active.as_ref().is_some_and(|d| d.id == id) {
            return;
        }
        if matches!(action, Action::Close) {
            self.close();
            return;
        }
        if matches!(action, Action::Retry) {
            if let (Some(connection), Some(document)) =
                (self.connection.clone(), self.active.as_ref())
            {
                self.open(connection, document.target.clone());
            }
            return;
        }
        let document = self.active.as_mut().unwrap();
        match action {
            Action::Fit => document.camera.fit(),
            Action::ActualSize => document.camera.actual_size(),
            Action::Zoom(factor) => document.camera.zoom(factor, None),
            Action::SaveDialogError => {
                document.error = Some("The save dialog is unavailable".into())
            }
            Action::SaveOriginal(destination) => {
                let Some(original) = document.original.clone() else {
                    return;
                };
                let Some(waker) = self.waker.clone() else {
                    return;
                };
                if document.saving || self.inflight >= 2 {
                    return;
                }
                document.saving = true;
                let owner = document.target.owner.clone();
                let cancel = document.cancel.clone();
                let send = self.send.clone();
                self.inflight += 1;
                std::thread::spawn(move || {
                    let result = loader::save(&original.path, &destination, &cancel);
                    if send.send(Reply::Saved { id, owner, result }).is_ok() {
                        waker();
                    }
                });
            }
            _ => {}
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(reply) = self.receive.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            let (id, owner) = match &reply {
                Reply::Loaded { id, owner, .. } | Reply::Saved { id, owner, .. } => (*id, owner),
            };
            let Some(document) = self
                .active
                .as_mut()
                .filter(|d| d.id == id && &d.target.owner == owner)
            else {
                continue;
            };
            changed = true;
            match reply {
                Reply::Loaded { result, .. } => {
                    document.loading = false;
                    match result {
                        Ok(loaded) => {
                            document.original = Some(Arc::new(loaded.original));
                            match loaded.image {
                                Ok(image) => {
                                    if let Some(cache) = &self.cache {
                                        document.image = Some(match image {
                                            loader::Prepared::Raster {
                                                width,
                                                height,
                                                bytes,
                                            } => cache.rgba(width, height, bytes),
                                        });
                                    }
                                }
                                Err(error) => document.error = Some(error),
                            }
                        }
                        Err(error) => document.error = Some(error),
                    }
                }
                Reply::Saved { result, .. } => {
                    document.saving = false;
                    match result {
                        Ok(()) => document.notice = Some("Original saved".into()),
                        Err(e) => document.error = Some(e),
                    }
                }
            }
        }
        if let Some(document) = self.active.as_mut() {
            if let Some(image) = &document.image {
                if let Some(size) = image.size() {
                    changed |= document.camera.set_image(size);
                }
                if let Some(error) = image.error() {
                    if document.error.is_none() {
                        document.error = Some(error.to_string());
                        changed = true;
                    }
                }
            }
        }
        self.running.retain(|c| !c.cancelled());
        self.start_pending();
        changed
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.close();
        for cancel in &self.running {
            cancel.cancel();
        }
    }
}
