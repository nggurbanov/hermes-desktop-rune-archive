//! Native workspace routes and immutable, owner-bound backend operations.
pub mod artifacts;
pub mod bots;
pub mod capabilities;
pub mod cron;
pub mod messaging;
pub mod settings;
use crate::{
    composer::DraftScope,
    gateway::{Connection, Gateway},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Route {
    #[default]
    Chat,
    Settings,
    Capabilities,
    Messaging,
    Artifacts,
    Cron,
    Bots,
    Connections,
    FilePreview,
    CommandCenter,
}
impl Route {
    pub fn title(self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Settings => "Settings",
            Self::Capabilities => "Capabilities",
            Self::Messaging => "Messaging",
            Self::Artifacts => "Artifacts",
            Self::Cron => "Scheduled jobs",
            Self::Bots => "Bots",
            Self::Connections => "Connections",
            Self::FilePreview => "Preview",
            Self::CommandCenter => "Command Center",
        }
    }
}
#[derive(Clone)]
pub enum PageMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Rpc,
    PluginRpc,
    LocalFs,
}
#[derive(Clone)]
pub struct PageUpload {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}
#[derive(Clone)]
pub struct PageRequest {
    pub method: PageMethod,
    pub path: String,
    pub body: Value,
    pub query: Vec<(String, String)>,
    pub profile: Option<String>,
    pub timeout: u64,
    pub destination: Option<std::path::PathBuf>,
    pub connection: Option<Connection>,
    pub source_lease: Option<crate::connections::Lease>,
    // Hold the originating actor through the actual I/O and final receipt.
    pub source_resolver: Option<crate::connections::Resolver>,
    pub upload: Option<PageUpload>,
    pub edit_claim: Option<Arc<crate::client::FileLock>>,
}
pub type PageEffect = (String, PageRequest);
impl PageRequest {
    fn new(method: PageMethod, path: impl Into<String>, body: Value) -> Self {
        Self {
            method,
            path: path.into(),
            body,
            query: vec![],
            profile: None,
            timeout: 30,
            destination: None,
            connection: None,
            source_lease: None,
            source_resolver: None,
            upload: None,
            edit_claim: None,
        }
    }
    pub fn get(path: impl Into<String>) -> Self {
        Self::new(PageMethod::Get, path, Value::Null)
    }
    pub fn post(path: impl Into<String>, body: Value) -> Self {
        Self::new(PageMethod::Post, path, body)
    }
    pub fn put(path: impl Into<String>, body: Value) -> Self {
        Self::new(PageMethod::Put, path, body)
    }
    pub fn patch(path: impl Into<String>, body: Value) -> Self {
        Self::new(PageMethod::Patch, path, body)
    }
    pub fn delete(path: impl Into<String>, body: Value) -> Self {
        Self::new(PageMethod::Delete, path, body)
    }
    pub fn rpc(method: impl Into<String>, body: Value) -> Self {
        Self::new(PageMethod::Rpc, method, body)
    }
    pub fn local_fs(path: impl Into<String>, body: Value) -> Self {
        Self::new(PageMethod::LocalFs, path, body)
    }
    pub fn plugin_rpc(method: impl Into<String>, body: Value) -> Self {
        Self::new(PageMethod::PluginRpc, method, body)
    }
    pub fn query(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.query.push((key.into(), value.to_string()));
        self
    }
    pub fn profile(mut self, profile: &str) -> Self {
        self.profile = Some(profile.into());
        self
    }
    pub fn upload(
        mut self,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        self.upload = Some(PageUpload {
            filename: filename.into(),
            content_type: content_type.into(),
            bytes,
        });
        self
    }
    pub fn connection(mut self, connection: Connection) -> Self {
        self.connection = Some(connection);
        self
    }
    pub fn lease(mut self, lease: crate::connections::Lease) -> Self {
        self.connection = Some(lease.connection().clone());
        self.source_lease = Some(lease);
        self
    }
    pub fn edit_claim(mut self, claim: Arc<crate::client::FileLock>) -> Self {
        self.edit_claim = Some(claim);
        self
    }
    pub fn download_to(mut self, path: std::path::PathBuf) -> Self {
        self.destination = Some(path);
        self
    }
    pub fn timeout_secs(mut self, seconds: u64) -> Self {
        self.timeout = seconds.clamp(1, 86400);
        self
    }
}
pub fn segment(value: &str) -> String {
    let mut url = url::Url::parse("http://local/").unwrap();
    url.path_segments_mut().unwrap().push(value);
    url.path().trim_start_matches('/').into()
}
#[derive(Clone, PartialEq, Eq)]
struct Owner {
    connection: u64,
    profile: String,
}
impl Owner {
    fn from(scope: &DraftScope) -> Self {
        Self {
            connection: scope.connection,
            profile: scope.profile.clone(),
        }
    }
}
struct Work {
    read_permit: Option<Arc<AtomicBool>>,
    id: u64,
    owner: Owner,
    operation: String,
    connection: Connection,
    request: PageRequest,
}
pub struct PageReply {
    pub progress: bool,
    pub operation: String,
    pub profile: String,
    pub result: Result<Value, String>,
    owner: Owner,
    id: u64,
}
#[derive(Default)]
pub struct PageNetwork {
    send: Option<Sender<Work>>,
    receive: Option<Receiver<PageReply>>,
    sequence: u64,
    pending: HashMap<String, (u64, Owner)>,
    reads: HashMap<String, Arc<AtomicBool>>,
}
impl PageNetwork {
    pub fn init(&mut self, waker: rune::Waker) {
        let (send, requests) = mpsc::channel::<Work>();
        let requests = Arc::new(Mutex::new(requests));
        let (replies, receive) = mpsc::channel();
        for _ in 0..4 {
            let requests = requests.clone();
            let replies = replies.clone();
            let waker = waker.clone();
            std::thread::spawn(move || {
                loop {
                    let work = match requests.lock().unwrap().recv() {
                        Ok(w) => w,
                        Err(_) => break,
                    };
                    let profile = work
                        .request
                        .profile
                        .clone()
                        .unwrap_or_else(|| work.owner.profile.clone());
                    let Some(result) = work.run(|event| {
                        let _ = replies.send(PageReply {
                            progress: true,
                            operation: work.operation.clone(),
                            profile: profile.clone(),
                            result: Ok(event),
                            owner: work.owner.clone(),
                            id: work.id,
                        });
                        waker();
                    }) else {
                        continue;
                    };
                    if replies
                        .send(PageReply {
                            progress: false,
                            operation: work.operation,
                            profile,
                            result,
                            owner: work.owner,
                            id: work.id,
                        })
                        .is_err()
                    {
                        break;
                    }
                    waker();
                }
            });
        }
        self.send = Some(send);
        self.receive = Some(receive);
    }
    pub fn request(
        &mut self,
        scope: &DraftScope,
        connection: Connection,
        operation: &str,
        request: PageRequest,
    ) -> Result<(), String> {
        if let Some(permit) = self.reads.remove(operation) {
            permit.store(false, Ordering::Release);
        }
        let read_permit = if request.is_read_only() {
            let permit = Arc::new(AtomicBool::new(true));
            self.reads.insert(operation.into(), permit.clone());
            Some(permit)
        } else {
            None
        };
        self.sequence += 1;
        let owner = Owner::from(scope);
        let id = self.sequence;
        self.pending.insert(operation.into(), (id, owner.clone()));
        let connection = request.connection.clone().unwrap_or(connection);
        self.send
            .as_ref()
            .ok_or("Page transport is unavailable")?
            .send(Work {
                read_permit,
                id,
                owner,
                operation: operation.into(),
                connection,
                request,
            })
            .map_err(|_| "Page transport is unavailable".into())
    }
    pub fn poll(&mut self, scope: &DraftScope) -> Vec<PageReply> {
        let mut result = vec![];
        if let Some(receive) = &self.receive {
            for reply in receive.try_iter() {
                let expected = self
                    .pending
                    .get(&reply.operation)
                    .is_some_and(|(id, owner)| *id == reply.id && *owner == reply.owner);
                if !expected {
                    continue;
                }
                if !reply.progress {
                    self.pending.remove(&reply.operation);
                    self.reads.remove(&reply.operation);
                }
                if reply.owner == Owner::from(scope) {
                    result.push(reply);
                }
            }
        }
        result
    }
    pub fn invalidate(&mut self) {
        self.pending.clear();
        for (_, permit) in self.reads.drain() {
            permit.store(false, Ordering::Release);
        }
    }
    pub fn invalidate_prefix(&mut self, prefix: &str) {
        self.pending.retain(|key, _| !key.starts_with(prefix));
        self.reads.retain(|key, permit| {
            if key.starts_with(prefix) {
                permit.store(false, Ordering::Release);
                false
            } else {
                true
            }
        });
    }
}

impl PageRequest {
    fn is_read_only(&self) -> bool {
        if self.destination.is_some() || self.upload.is_some() {
            return false;
        }
        match self.method {
            PageMethod::Get => true,
            PageMethod::Rpc | PageMethod::PluginRpc => matches!(
                self.path.as_str(),
                "profiles.list"
                    | "profiles.describe"
                    | "profiles.get_asset"
                    | "setup.status"
                    | "setup.runtime_check"
                    | "model.options"
                    | "config.get"
                    | "mcp.catalog"
                    | "process.list"
                    | "session.list"
                    | "session.context_breakdown"
                    | "pet.thumb"
            ),
            PageMethod::LocalFs => matches!(
                self.path.as_str(),
                "/api/fs/list" | "/api/fs/read-text" | "/api/fs/read-data-url"
            ),
            _ => false,
        }
    }
}
impl Work {
    fn run(&self, on_event: impl FnMut(Value)) -> Option<Result<Value, String>> {
        if self
            .read_permit
            .as_ref()
            .is_some_and(|permit| !permit.load(Ordering::Acquire))
        {
            return None;
        }
        let profile = self
            .request
            .profile
            .as_deref()
            .unwrap_or(&self.owner.profile);
        if self
            .request
            .source_lease
            .as_ref()
            .is_some_and(|lease| !lease.is_valid())
        {
            return Some(Err(
                "The saved source changed before this operation could be sent".into(),
            ));
        }
        let result = if matches!(self.request.method, PageMethod::LocalFs) {
            if matches!(self.connection.transport(), Connection::Local { .. }) {
                crate::native_fs::execute(&self.request.path, &self.request.body)
            } else {
                Err("Client filesystem operations require a local connection".into())
            }
        } else {
            Gateway::connect(&self.connection)
                .and_then(|mut g| g.page_request_events(profile, &self.request, on_event))
        };
        Some(
            if self
                .request
                .source_lease
                .as_ref()
                .is_some_and(|lease| !lease.is_valid())
            {
                Err("The saved source changed while this operation was in flight; its outcome may be unknown".into())
            } else {
                result
            },
        )
    }
}

#[cfg(test)]
mod tests;
