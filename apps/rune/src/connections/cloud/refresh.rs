use super::*;
use std::{
    collections::HashMap,
    sync::{OnceLock, mpsc},
    thread,
};

type Key = (String, String, Vec<u8>);
type Sender = mpsc::Sender<Result<String, Error>>;
struct Flight {
    subscribers: Vec<Sender>,
}
static FLIGHTS: OnceLock<Mutex<HashMap<Key, Flight>>> = OnceLock::new();

/// Drop to cancel this subscriber. The shared operation remains alive until the
/// server response and its atomic credential-store update have completed.
pub struct AccessSubscription(mpsc::Receiver<Result<String, Error>>);
impl AccessSubscription {
    pub fn try_recv(&self) -> Result<Option<Result<String, Error>>, Error> {
        match self.0.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(Error::Transport),
        }
    }
    pub fn wait_until(self, deadline: Instant) -> Result<String, Error> {
        match self.0.recv_timeout(remaining(deadline)?) {
            Ok(result) => {
                remaining(deadline)?;
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Transport),
        }
    }
    pub fn wait(self) -> Result<String, Error> {
        self.0.recv().unwrap_or(Err(Error::Transport))
    }
}

pub(super) fn endpoint_key(endpoint: &Endpoint, identity: &str) -> Key {
    let mut entries: Vec<_> = endpoint.headers.iter().collect();
    entries.sort_by(|a, b| {
        a.0.as_str()
            .cmp(b.0.as_str())
            .then(a.1.as_bytes().cmp(b.1.as_bytes()))
    });
    let mut hash = Sha256::new();
    for (name, value) in entries {
        hash.update((name.as_str().len() as u64).to_be_bytes());
        hash.update(name.as_str().as_bytes());
        hash.update((value.as_bytes().len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    (
        endpoint.base.to_string(),
        identity.to_owned(),
        hash.finalize().to_vec(),
    )
}

pub(super) fn subscribe<S: TokenStore + 'static>(
    endpoint: &Endpoint,
    identity: &str,
    store: Arc<Mutex<S>>,
    force: bool,
    rejected: Option<String>,
    now: i64,
) -> Result<AccessSubscription, Error> {
    let key = endpoint_key(endpoint, identity);
    let (tx, rx) = mpsc::channel();
    let endpoint = endpoint.clone();
    thread::Builder::new()
        .name("cloud-access".into())
        .spawn(move || {
            let initial = store
                .lock()
                .map_err(|_| Error::Store)
                .and_then(|mut s| s.load());
            let tokens = match initial {
                Ok(Some(tokens)) => tokens,
                Ok(None) => {
                    let _ = tx.send(Err(Error::Reauthenticate));
                    return;
                }
                Err(error) => {
                    let _ = tx.send(Err(error));
                    return;
                }
            };
            let registry = FLIGHTS.get_or_init(Default::default);
            let Ok(mut flights) = registry.lock() else {
                let _ = tx.send(Err(Error::Store));
                return;
            };
            if let Some(flight) = flights.get_mut(&key) {
                flight.subscribers.push(tx);
                return;
            }
            if rejected
                .as_deref()
                .is_some_and(|r| r != tokens.access_token)
                || (!force && tokens.expires_at > now.saturating_add(60))
            {
                drop(flights);
                let _ = tx.send(Ok(tokens.access_token));
                return;
            }
            flights.insert(
                key.clone(),
                Flight {
                    subscribers: vec![tx],
                },
            );
            drop(flights);
            // Reload after admission: a previous flight may have finished between
            // our first read and registration.
            let result = perform(&endpoint, &store, force, rejected.as_deref(), now);
            let subscribers = registry
                .lock()
                .ok()
                .and_then(|mut flights| flights.remove(&key))
                .map(|f| f.subscribers)
                .unwrap_or_default();
            for subscriber in subscribers {
                let _ = subscriber.send(result.clone());
            }
        })
        .map_err(|_| Error::Transport)?;
    Ok(AccessSubscription(rx))
}

fn perform<S: TokenStore>(
    endpoint: &Endpoint,
    store: &Mutex<S>,
    force: bool,
    rejected: Option<&str>,
    now: i64,
) -> Result<String, Error> {
    let tokens = store
        .lock()
        .map_err(|_| Error::Store)?
        .load()?
        .ok_or(Error::Reauthenticate)?;
    if rejected.is_some_and(|r| r != tokens.access_token)
        || (!force && tokens.expires_at > now.saturating_add(60))
    {
        return Ok(tokens.access_token);
    }
    if tokens.refresh_token.is_empty() {
        return Err(Error::Reauthenticate);
    }
    let body = request(
        endpoint,
        reqwest::Method::POST,
        endpoint.url("/auth/native/refresh"),
        None,
        Some(json!({"refresh_token":tokens.refresh_token,"provider":tokens.provider})),
    );
    let replacement = match body {
        Err(Error::Http(401, _)) => None,
        Err(error) => return Err(error),
        Ok(value) => Some(
            serde_json::from_value::<Tokens>(value)
                .map_err(|_| Error::InvalidResponse)?
                .valid()?,
        ),
    };
    let mut store = store.lock().map_err(|_| Error::Store)?;
    if store.compare_and_swap(&tokens, replacement.as_ref())? {
        replacement
            .map(|t| t.access_token)
            .ok_or(Error::Reauthenticate)
    } else {
        store
            .load()?
            .map(|t| t.access_token)
            .ok_or(Error::Reauthenticate)
    }
}
