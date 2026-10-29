use super::*;
use crate::{gateway::Gateway, pages::PageRequest};
use std::sync::{Arc, Mutex};
#[derive(Clone)]
pub(super) enum Action {
    Read,
    Kill(String),
    RewindKill(String),
}
pub(super) struct Work {
    pub scope: DraftScope,
    pub runtime: String,
    pub generation: u64,
    pub id: u64,
    pub connection: Connection,
    pub active: Arc<AtomicBool>,
    pub action: Action,
}
pub(super) struct Reply {
    pub scope: DraftScope,
    pub runtime: String,
    pub generation: u64,
    pub id: u64,
    pub action: Action,
    pub result: Result<Value, String>,
}
pub(super) fn start(waker: Waker) -> (mpsc::SyncSender<Work>, mpsc::Receiver<Reply>) {
    let (send, requests) = mpsc::sync_channel::<Work>(32);
    let requests = Arc::new(Mutex::new(requests));
    let (replies, receive) = mpsc::channel();
    for _ in 0..2 {
        let requests = requests.clone();
        let replies = replies.clone();
        let waker = waker.clone();
        std::thread::spawn(move || {
            loop {
                let work = match requests.lock().unwrap().recv() {
                    Ok(work) => work,
                    Err(_) => break,
                };
                if !work.active.load(Ordering::Acquire) {
                    continue;
                }
                let mut body = serde_json::json!({"session_id":work.runtime});
                let method = match &work.action {
                    Action::Read => "process.list",
                    Action::Kill(process) | Action::RewindKill(process) => {
                        body["process_id"] = process.clone().into();
                        "process.kill"
                    }
                };
                let request = PageRequest::rpc(method, body)
                    .profile(&work.scope.profile)
                    .timeout_secs(10);
                let result = Gateway::connect(&work.connection)
                    .and_then(|mut gateway| gateway.page_request(&work.scope.profile, &request));
                if replies
                    .send(Reply {
                        scope: work.scope,
                        runtime: work.runtime,
                        generation: work.generation,
                        id: work.id,
                        action: work.action,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                waker();
            }
        });
    }
    (send, receive)
}
