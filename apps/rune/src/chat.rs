//! Native transcript presentation and asynchronous reads retain exact owners.
mod actions;
mod media;
pub mod projection;
use crate::{composer::DraftScope, gateway::Connection};
pub use actions::{BranchIntent, SlashRow, catalog_rows, parse_slash};
use rune::{Image, ImageCache, TextEdit, Waker};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver, Sender},
};
#[derive(Clone)]
pub enum Action {
    Expand(String),
    Image(String),
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
}
#[derive(Default)]
pub struct Presentation {
    pub expanded: HashSet<String>,
    pub edit: Option<(u64, TextEdit)>,
    pub confirmation: Option<Confirmation>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub slash: Vec<SlashRow>,
    pub slash_loaded: bool,
    pub slash_loading: bool,
    pub slash_running: bool,
    pub branch: Option<BranchIntent>,
    pub branch_result: Option<(String, String)>,
    pub branch_busy: bool,
    pub recovered: bool,
    pub preview: Option<String>,
}
pub struct Media {
    pub image: Option<Image>,
    pub error: Option<String>,
    pub loading: bool,
}
pub enum Reply {
    MediaReady,
    Media {
        owner: DraftScope,
        source: String,
        result: Result<(String, Vec<u8>), String>,
    },
    Catalog {
        owner: DraftScope,
        result: Result<Value, String>,
    },
    Slash {
        owner: DraftScope,
        input: String,
        result: Result<Value, String>,
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
                    result,
                } => {
                    if let Some(media) = self.media.get_mut(&(owner, source)) {
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
                other => out.push(other),
            }
        }
        out
    }
    pub fn ensure_media(&mut self, connection: Connection, owner: DraftScope, source: String) {
        media::request(self, connection, owner, source);
    }
    pub fn close_connection(&mut self, epoch: u64) {
        self.views.retain(|s, _| s.connection != epoch);
        self.media.retain(|(s, _), _| s.connection != epoch);
    }
    pub fn close_profile(&mut self, epoch: u64, profile: &str) {
        self.views
            .retain(|s, _| s.connection != epoch || s.profile != profile);
        self.media
            .retain(|(s, _), _| s.connection != epoch || s.profile != profile);
    }
    pub fn blocks_profile_retirement(&self, epoch: u64, profile: Option<&str>) -> bool {
        self.views.iter().any(|(scope, view)| {
            scope.connection == epoch
                && profile.is_none_or(|p| p == scope.profile)
                && (view.slash_running
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
