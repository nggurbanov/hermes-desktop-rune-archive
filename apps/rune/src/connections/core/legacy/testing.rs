//! Private real actor/storage fixture. It never uses a user directory or OS credentials.
use super::super::*;
use crate::connections::{Connections, Network, Resolver, worker};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct StateInner {
    root: Arc<Root>,
    fail_saves: AtomicBool,
    exited: AtomicBool,
    secrets: Mutex<std::collections::BTreeMap<String, String>>,
    secret_calls: Mutex<Vec<&'static str>>,
}
#[derive(Clone)]
pub(crate) struct State(Arc<StateInner>);
impl State {
    pub(crate) fn path(&self) -> PathBuf {
        self.0.root.0.join("connections.json")
    }
    pub(crate) fn read_json(&self) -> Value {
        serde_json::from_slice(&fs::read(self.path()).unwrap()).unwrap()
    }
    pub(crate) fn fail_saves(&self, fail: bool) {
        self.0.fail_saves.store(fail, Ordering::Release);
    }
    pub(crate) fn worker_exited(&self) -> bool {
        self.0.exited.load(Ordering::Acquire)
    }
    pub(crate) fn secret_operations(&self) -> Vec<&'static str> {
        self.0.secret_calls.lock().unwrap().clone()
    }
    pub(crate) fn credential_count(&self) -> usize {
        self.0.secrets.lock().unwrap().len()
    }
}
struct PrivateMetadata {
    actual: JsonMetadata,
    state: State,
}
impl Metadata for PrivateMetadata {
    fn transaction(&mut self) -> Result<Option<crate::client::FileLock>, String> {
        self.actual.transaction()
    }
    fn load(&mut self) -> Result<Registry, String> {
        self.actual.load()
    }
    fn save(&mut self, value: &Registry) -> Result<(), String> {
        if self.state.0.fail_saves.load(Ordering::Acquire) {
            return Err("The private fixture refused its atomic save".into());
        }
        self.actual.save(value)
    }
}
struct PrivateSecrets(State);
impl Secrets for PrivateSecrets {
    fn get(&mut self, id: &str) -> Result<String, String> {
        self.0.0.secret_calls.lock().unwrap().push("get");
        self.0
            .0
            .secrets
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("Private fixture credential missing".into())
    }
    fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        self.0.0.secret_calls.lock().unwrap().push("set");
        self.0
            .0
            .secrets
            .lock()
            .unwrap()
            .insert(id.into(), value.into());
        Ok(())
    }
    fn delete(&mut self, id: &str) -> Result<(), String> {
        self.0.0.secret_calls.lock().unwrap().push("delete");
        self.0.0.secrets.lock().unwrap().remove(id);
        Ok(())
    }
}
struct NoProbe;
impl Probe for NoProbe {
    fn check(&mut self, _: &Connection) -> Result<Vec<crate::gateway::Profile>, String> {
        Err("This fixture forbids gateway probes".into())
    }
}
struct NoTunnels;
impl Tunnels for NoTunnels {
    fn open(&mut self, _: &Ssh) -> Result<Box<dyn Tunnel>, String> {
        Err("This fixture forbids SSH".into())
    }
}
pub(crate) struct Rig {
    pub(crate) connections: Connections,
    pub(crate) state: State,
}
impl Rig {
    pub(crate) fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("hermes-legacy-settings-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let state = State(Arc::new(StateInner {
            root: Arc::new(Root(root)),
            fail_saves: AtomicBool::new(false),
            exited: AtomicBool::new(false),
            secrets: Mutex::new(Default::default()),
            secret_calls: Mutex::new(vec![]),
        }));
        let mut metadata = JsonMetadata(state.path());
        metadata.save(&Registry::empty()).unwrap();
        let mut core = Core::system(state.path());
        core.metadata = Box::new(PrivateMetadata {
            actual: metadata,
            state: state.clone(),
        });
        core.secrets = Box::new(PrivateSecrets(state.clone()));
        core.probe = Box::new(NoProbe);
        core.tunnels = Box::new(NoTunnels);
        let (sender, requests) = mpsc::channel();
        let (replies, receiver) = mpsc::channel();
        let worker_state = state.clone();
        std::thread::spawn(move || {
            worker::run(Ok(core), requests, replies, Arc::new(|| {}));
            worker_state.0.exited.store(true, Ordering::Release);
        });
        let mut connections = Connections::new();
        connections.network = Some(Network::new(sender, receiver));
        connections.load();
        Self { connections, state }
    }
    pub(crate) fn resolver(&self) -> Resolver {
        self.connections.resolver().unwrap()
    }
}
