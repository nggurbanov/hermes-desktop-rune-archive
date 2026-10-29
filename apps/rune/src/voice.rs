//! Profile-owned engine status, separate from drafts and conversation reads.
use crate::{
    composer::DraftScope,
    gateway::{Connection, Gateway, VoiceStatus},
};
use rune::Waker;
use std::sync::mpsc::{self, Receiver, Sender};

type Owner = (u64, String);
fn owner(scope: &DraftScope) -> Owner {
    (scope.connection, scope.profile.clone())
}

#[derive(Default)]
pub struct Voice {
    snapshot: Option<(Owner, VoiceStatus)>,
    pending: Option<(u64, Owner)>,
    failure: Option<(Owner, String)>,
    sequence: u64,
    network: Option<Network>,
}
struct Network {
    send: Sender<Request>,
    receive: Receiver<Reply>,
}
struct Request {
    id: u64,
    owner: Owner,
    connection: Connection,
}
struct Reply {
    id: u64,
    owner: Owner,
    result: Result<Option<VoiceStatus>, String>,
}

impl Voice {
    pub fn status(&self, scope: &DraftScope) -> Option<&VoiceStatus> {
        self.snapshot
            .as_ref()
            .filter(|(key, _)| *key == owner(scope))
            .map(|(_, s)| s)
    }
    pub fn loading(&self, scope: &DraftScope) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(_, key)| *key == owner(scope))
    }
    pub fn error(&self, scope: &DraftScope) -> Option<&str> {
        self.failure
            .as_ref()
            .filter(|(key, _)| *key == owner(scope))
            .map(|(_, e)| e.as_str())
    }
    pub fn fixture(&mut self, scope: &DraftScope, value: &serde_json::Value) {
        self.snapshot = VoiceStatus::from_value(value)
            .ok()
            .flatten()
            .map(|s| (owner(scope), s));
    }
    pub fn init(&mut self, waker: Waker) {
        let (send, requests) = mpsc::channel::<Request>();
        let (replies, receive) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(mut req) = requests.recv() {
                for newer in requests.try_iter() {
                    req = newer;
                }
                let result = Gateway::connect(&req.connection)
                    .and_then(|mut g| g.voice_status(&req.owner.1));
                if replies
                    .send(Reply {
                        id: req.id,
                        owner: req.owner,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                waker();
            }
        });
        self.network = Some(Network { send, receive });
    }
    pub fn request(&mut self, connection: Connection, scope: &DraftScope) {
        self.sequence += 1;
        let key = owner(scope);
        self.failure = None;
        self.pending = Some((self.sequence, key.clone()));
        let req = Request {
            id: self.sequence,
            owner: key.clone(),
            connection,
        };
        if !self
            .network
            .as_ref()
            .is_some_and(|n| n.send.send(req).is_ok())
        {
            self.pending = None;
            self.failure = Some((key, "Voice status could not be loaded".into()));
        }
    }
    pub fn poll(&mut self, scope: &DraftScope) -> bool {
        let replies: Vec<_> = self
            .network
            .as_ref()
            .map(|n| n.receive.try_iter().collect())
            .unwrap_or_default();
        let mut changed = false;
        for reply in replies {
            changed |= self.apply(reply, scope);
        }
        changed
    }
    fn apply(&mut self, reply: Reply, scope: &DraftScope) -> bool {
        if self.pending.as_ref() != Some(&(reply.id, reply.owner.clone())) {
            return false;
        }
        self.pending = None;
        if reply.owner != owner(scope) {
            return true;
        }
        self.snapshot = None;
        self.failure = None;
        match reply.result {
            Ok(Some(status)) => self.snapshot = Some((reply.owner, status)),
            Ok(None) => {
                self.failure = Some((
                    reply.owner,
                    "This agent does not report voice engines".into(),
                ))
            }
            Err(error) => self.failure = Some((reply.owner, error)),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::VoiceEngine;
    fn scope(connection: u64, profile: &str) -> DraftScope {
        DraftScope {
            connection,
            profile: profile.into(),
            session: Some("stored".into()),
        }
    }
    fn status() -> VoiceStatus {
        VoiceStatus {
            engine: VoiceEngine::GptLive,
            live_available: true,
        }
    }

    #[test]
    fn engine_status_is_profile_owned_and_never_seeds_another_connection() {
        let a = scope(1, "work");
        let b = scope(2, "work");
        let mut voice = Voice::default();
        voice.pending = Some((1, owner(&a)));
        assert!(voice.apply(
            Reply {
                id: 1,
                owner: owner(&a),
                result: Ok(Some(status()))
            },
            &b
        ));
        assert!(voice.status(&b).is_none());
        voice.fixture(
            &a,
            &serde_json::json!({"ok":true,"mode":"gpt-live","available":true}),
        );
        let other_session = DraftScope {
            session: Some("other".into()),
            ..a.clone()
        };
        assert_eq!(
            voice.status(&other_session).unwrap().engine,
            VoiceEngine::GptLive
        );
        assert!(voice.status(&scope(1, "personal")).is_none());
    }

    #[test]
    fn a_stale_status_reply_cannot_replace_a_newer_read_or_masquerade_as_a_default() {
        let a = scope(1, "work");
        let mut voice = Voice::default();
        voice.pending = Some((2, owner(&a)));
        assert!(!voice.apply(
            Reply {
                id: 1,
                owner: owner(&a),
                result: Ok(Some(status()))
            },
            &a
        ));
        assert!(voice.loading(&a));
        assert!(voice.apply(
            Reply {
                id: 2,
                owner: owner(&a),
                result: Ok(None)
            },
            &a
        ));
        assert!(voice.status(&a).is_none());
        assert!(voice.error(&a).is_some());
    }
}
