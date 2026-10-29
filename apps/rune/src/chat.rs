//! Native transcript presentation and asynchronous reads retain exact owners.
mod actions;
mod completion;
pub(crate) mod media;
pub mod projection;
mod slash;
use crate::{composer::DraftScope, gateway::Connection};
pub use actions::{BranchIntent, SlashRow, catalog_rows, parse_slash};
use rune::{Image, ImageCache, TextEdit, Waker};
use serde_json::Value;
pub use slash::SlashIntent;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::mpsc::{self, Receiver, Sender},
};
#[derive(Clone)]
pub enum Action {
    Expand(String),
    SetDisclosure(String, bool),
    Image(String),
    RetryMedia(String),
    Quote(String),
    Speak(String),
    File(String),
    Process(String),
    Edit(u64),
    Retry(u64),
    Restore(u64),
    SaveEdit,
    Cancel,
    Confirm,
    Branch(u64),
    RetryBranch,
    OpenBranch,
    Earlier,
    ChooseSlash(String),
    Dismiss,
    UseRestored,
    RetrySlash,
    ConfirmSlash,
    ResolveSlashUnknown,
    AppendPrefill,
}
#[derive(Clone)]
pub enum RewriteKind {
    Edit,
    Retry,
    Restore,
}
#[derive(Clone)]
pub struct Confirmation {
    pub row: u64,
    pub kind: RewriteKind,
    pub text: String,
    pub later: usize,
    pub revision: u64,
    pub generation: u64,
    pub authority: u64,
    pub replay: Option<crate::conversation::ReplayBundle>,
}
#[derive(Default)]
pub struct Presentation {
    pub expanded: HashSet<String>,
    pub collapsed: HashSet<String>,
    pub edit: Option<(u64, TextEdit)>,
    pub confirmation: Option<Confirmation>,
    pub restored_candidate: Option<crate::conversation::ReplayBundle>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub failed_command: Option<String>,
    pub prefill_candidate: Option<String>,
    pub slash: Vec<SlashRow>,
    pub slash_loaded: bool,
    pub slash_loading: bool,
    pub slash_running: bool,
    pub slash_highlight: usize,
    pub slash_dismissed: Option<String>,
    pub slash_catalog: Value,
    pub catalog_cwd: Option<String>,
    pub slash_intent: Option<SlashIntent>,
    pub slash_confirmation: Option<(String, String)>,
    pub slash_recovered: bool,
    pub completion_query: String,
    pub completion_ready: String,
    pub completion_due: Option<std::time::Instant>,
    pub completion_pending: bool,
    pub completion_items: Vec<SlashRow>,
    pub branch: Option<BranchIntent>,
    pub branch_result: Option<(String, String)>,
    pub branch_busy: bool,
    pub recovered: bool,
    pub preview: Option<String>,
}
pub struct Media {
    request_id: u64,
    decoded: bool,
    pub image: Option<Image>,
    pub error: Option<String>,
    pub loading: bool,
}
pub enum Reply {
    MediaReady,
    Completions {
        owner: DraftScope,
        query: String,
        result: Result<Value, String>,
    },
    Media {
        owner: DraftScope,
        source: String,
        request_id: u64,
        result: Result<(String, Vec<u8>), String>,
    },
    Catalog {
        owner: DraftScope,
        result: Result<Value, String>,
    },
    Slash {
        owner: DraftScope,
        input: String,
        intent: SlashIntent,
        result: Result<Value, crate::gateway::conversation::CallError>,
    },
    Branch {
        owner: DraftScope,
        result: Result<(String, String), crate::gateway::conversation::CallError>,
    },
}
pub struct Chat {
    pub views: HashMap<DraftScope, Presentation>,
    pub media: HashMap<(DraftScope, String), Media>,
    send: Sender<Reply>,
    receive: Receiver<Reply>,
    cache: Option<ImageCache>,
    media_order: VecDeque<(DraftScope, String)>,
    media_inflight: usize,
    media_sequence: u64,
    completion_inflight: usize,
    waker: Option<Waker>,
}
impl Default for Chat {
    fn default() -> Self {
        let (send, receive) = mpsc::channel();
        Self {
            views: HashMap::new(),
            media: HashMap::new(),
            send,
            receive,
            cache: None,
            media_order: VecDeque::new(),
            media_inflight: 0,
            media_sequence: 0,
            completion_inflight: 0,
            waker: None,
        }
    }
}
impl Chat {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn init(&mut self, waker: Waker) {
        self.cache = Some(ImageCache::new(waker.clone()));
        self.waker = Some(waker);
    }
    pub fn presentation(&mut self, owner: DraftScope) -> &mut Presentation {
        self.views.entry(owner).or_default()
    }
    pub fn poll(&mut self) -> Vec<Reply> {
        let mut out = vec![];
        for reply in self.receive.try_iter().take(128) {
            match reply {
                Reply::Media {
                    owner,
                    source,
                    request_id,
                    result,
                } => {
                    self.media_inflight = self.media_inflight.saturating_sub(1);
                    if let Some(media) = self
                        .media
                        .get_mut(&(owner, source))
                        .filter(|media| media.request_id == request_id)
                    {
                        media.loading = false;
                        match result {
                            Ok((mime, bytes)) => {
                                media.image = self.cache.as_ref().map(|cache| {
                                    cache.load_mime(
                                        std::sync::Arc::<[u8]>::from(bytes),
                                        &mime,
                                        1000.,
                                        1.,
                                    )
                                })
                            }
                            Err(error) => media.error = Some(error),
                        }
                    }
                    out.push(Reply::MediaReady);
                }
                Reply::Completions {
                    owner,
                    query,
                    result,
                } => {
                    self.completion_inflight = self.completion_inflight.saturating_sub(1);
                    out.push(Reply::Completions {
                        owner,
                        query,
                        result,
                    });
                }
                other => out.push(other),
            }
        }
        let mut decoded = false;
        for media in self.media.values_mut() {
            if !media.decoded && media.image.as_ref().is_some_and(|image| image.is_ready()) {
                media.decoded = true;
                decoded = true;
            }
        }
        if decoded {
            out.push(Reply::MediaReady);
        }
        out
    }
    pub fn ensure_media(&mut self, connection: Connection, owner: DraftScope, source: String) {
        media::request(self, connection, owner, source);
    }
    /// Bound background previews; a selected older image stays pinned until closed.
    pub fn prepare_media(
        &mut self,
        connection: Connection,
        owner: DraftScope,
        sources: Vec<String>,
    ) {
        let mut sources: Vec<_> = sources.into_iter().rev().take(24).collect();
        sources.reverse();
        if let Some(source) = self.views.get(&owner).and_then(|v| v.preview.clone()) {
            sources.retain(|s| s != &source);
            if sources.len() >= 24 {
                sources.remove(0);
            }
            sources.push(source);
        }
        let keep: HashSet<_> = sources.iter().cloned().collect();
        self.media
            .retain(|(scope, source), _| scope != &owner || keep.contains(source));
        self.media_order.retain(|key| self.media.contains_key(key));
        for source in sources {
            self.ensure_media(connection.clone(), owner.clone(), source);
        }
    }
    pub fn retry_media(&mut self, connection: Connection, owner: DraftScope, source: String) {
        let key = (owner.clone(), source.clone());
        if self.media.get(&key).is_some_and(|m| m.loading) {
            return;
        }
        self.media.remove(&key);
        self.media_order.retain(|item| item != &key);
        self.ensure_media(connection, owner, source);
    }
    pub fn close_connection(&mut self, epoch: u64) {
        self.views.retain(|s, _| s.connection != epoch);
        self.media.retain(|(s, _), _| s.connection != epoch);
        self.media_order.retain(|(s, _)| s.connection != epoch);
    }
    pub fn close_profile(&mut self, epoch: u64, profile: &str) {
        self.views
            .retain(|s, _| s.connection != epoch || s.profile != profile);
        self.media
            .retain(|(s, _), _| s.connection != epoch || s.profile != profile);
        self.media_order
            .retain(|(s, _)| s.connection != epoch || s.profile != profile);
    }
    pub fn blocks_profile_retirement(&self, epoch: u64, profile: Option<&str>) -> bool {
        self.views.iter().any(|(scope, view)| {
            scope.connection == epoch
                && profile.is_none_or(|p| p == scope.profile)
                && (view.slash_running
                    || view.slash_intent.is_some()
                    || view.branch_busy
                    || view.branch.is_some() && view.branch_result.is_none())
        })
    }
    pub fn rehome(&mut self, origin: &DraftScope, target: DraftScope) {
        if origin != &target {
            if let Some(view) = self.views.remove(origin) {
                self.views.entry(target).or_insert(view);
            }
        }
    }
}
