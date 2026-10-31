//! Every child request remains bound to the parent-selected gateway principal.
use super::*;
mod ambient;
mod notifications;
use crate::{
    composer::DraftScope,
    gateway::Connection,
    pages::{PageEffect, PageRequest},
};
pub use ambient::AmbientTarget;
use std::collections::BTreeSet;
#[derive(Clone)]
pub struct HostAction {
    pub id: String,
    pub kind: String,
    pub payload: Value,
}
#[derive(Clone)]
pub struct NavigationTarget {
    pub source: String,
    pub profile: String,
    pub connection: Connection,
    pub lease: Option<crate::connections::Lease>,
}
#[derive(Clone, PartialEq, Eq)]
struct DocumentIdentity {
    source: crate::source::SourceStamp,
    connection_generation: u64,
    foreground_profile: String,
    profile_intent: crate::source::ProfileIntent,
    target_profile: String,
    document: String,
    origin: String,
}
enum ManagedProcess {
    Native(HostProcess),
    #[cfg(test)]
    Fixture(std::sync::Arc<std::sync::Mutex<FixtureProcess>>),
}
#[cfg(test)]
#[derive(Default)]
struct FixtureProcess {
    alive: bool,
    fail_focus: bool,
    commands: Vec<Value>,
    events: Vec<Result<Value, String>>,
    state: String,
    closes: usize,
}
impl ManagedProcess {
    fn alive(&mut self) -> bool {
        match self {
            Self::Native(process) => process.alive(),
            #[cfg(test)]
            Self::Fixture(process) => process.lock().unwrap().alive,
        }
    }
    fn send(&mut self, value: Value) -> Result<(), String> {
        match self {
            Self::Native(process) => process.send(value),
            #[cfg(test)]
            Self::Fixture(process) => {
                let mut process = process.lock().unwrap();
                if !process.alive || (value["type"] == "focus" && process.fail_focus) {
                    return Err("fixture host closed".into());
                }
                process.commands.push(value);
                Ok(())
            }
        }
    }
    fn poll(&mut self) -> Vec<Result<Value, String>> {
        match self {
            Self::Native(process) => process.poll(),
            #[cfg(test)]
            Self::Fixture(process) => std::mem::take(&mut process.lock().unwrap().events),
        }
    }
    fn close(&mut self) {
        match self {
            Self::Native(process) => process.close(),
            #[cfg(test)]
            Self::Fixture(process) => {
                let mut process = process.lock().unwrap();
                process.alive = false;
                process.closes += 1;
            }
        }
    }
}
struct Running {
    identity: DocumentIdentity,
    owner_lease: Option<crate::connections::Lease>,
    window_facts: Option<(bool, bool)>,
    opened_at: std::time::Instant,
    token: String,
    source_connection: Connection,
    request_source: String,
    sources: super::sources::Sources,
    deferred: Vec<(Value, std::time::Instant)>,
    scope: DraftScope,
    request_profile: String,
    key: String,
    process: ManagedProcess,
    source: String,
    origin: String,
    owner: Value,
    allowed_profiles: BTreeSet<String>,
    bootstrap_sent: bool,
    storage_path: std::path::PathBuf,
    storage: Option<Value>,
    saved_at: std::time::Instant,
    feed: crate::gateway::plugin::PluginFeed,
    uploads: super::uploads::Uploads,
    sockets: crate::gateway::plugin::sockets::SocketBroker,
}
#[derive(Default)]
pub struct PluginHosts {
    #[cfg(test)]
    spawn_fixture: Option<Box<dyn FnMut() -> Result<ManagedProcess, String>>>,
    ambient: BTreeMap<u64, AmbientTarget>,
    running: Option<Running>,
    resolver: Option<crate::connections::Resolver>,
    pending: BTreeMap<String, String>,
    pub actions: Vec<HostAction>,
    pub error: Option<String>,
    pub notice: Option<String>,
}
impl PluginHosts {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_resolver(&mut self, resolver: Option<crate::connections::Resolver>) {
        self.resolver = resolver;
    }
    pub fn window_token(&self) -> Option<String> {
        self.running.as_ref().map(|running| running.token.clone())
    }
    pub fn reply_owned(&mut self, token: &str, id: &str, result: Result<Value, String>) {
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.token == token)
        {
            self.reply(id, result);
        }
    }
    pub fn adopt_parent_scope(
        &mut self,
        token: &str,
        scope: &DraftScope,
        request_profile: Option<&str>,
    ) -> Result<(), String> {
        let running = self.running.as_mut().ok_or("The plugin window is closed")?;
        if running.token != token || running.scope.connection != scope.connection {
            return Err("The native plugin owner changed while navigating".into());
        }
        if let Some(profile) = request_profile {
            if !running.allowed_profiles.contains(profile) {
                return Err("The plugin target profile is no longer served".into());
            }
            if running.request_profile != profile {
                for key in ["session", "runtime_session", "cwd", "model"] {
                    running.owner[key] = Value::Null;
                }
                running.owner["busy"] = false.into();
                running.request_profile = profile.into();
                running.owner["profile"] = profile.into();
                running.owner["target_profile"] = profile.into();
            }
        }
        running.scope = scope.clone();
        Ok(())
    }
    pub fn navigation_busy(&self) -> bool {
        !self.pending.is_empty()
            || self
                .running
                .as_ref()
                .is_some_and(|running| running.uploads.is_busy())
    }
    pub fn navigation_target(
        &mut self,
        token: &str,
        payload: &Value,
    ) -> Result<Option<NavigationTarget>, String> {
        let running = self.running.as_mut().ok_or("The plugin window is closed")?;
        if running.token != token
            || payload["native_owner"]["lifecycle_key"].as_str() != Some(&running.key)
        {
            return Err("The plugin owner changed while navigating".into());
        }
        let options = &payload["options"];
        let route = options
            .get("route")
            .or_else(|| payload.get("profile").filter(|v| v.is_object()));
        let source = route
            .and_then(|r| r["connectionId"].as_str())
            .or(payload["connectionId"].as_str())
            .unwrap_or(&running.request_source)
            .to_owned();
        let profile = route
            .and_then(|r| r["targetProfile"].as_str().or(r["profile"].as_str()))
            .or(options["profile"].as_str())
            .or(payload["profile"].as_str())
            .or(payload["native_owner"]["profile"].as_str())
            .unwrap_or(&running.request_profile)
            .to_owned();
        let birth = running.sources.primary.clone();
        if running.sources.ensure(&birth)?.is_none() {
            return Ok(None);
        }
        let Some(binding) = running.sources.ensure(&source)? else {
            return Ok(None);
        };
        if !binding.profiles.contains(&profile) {
            return Err("The selected source no longer serves that profile".into());
        }
        Ok(Some(NavigationTarget {
            source,
            profile,
            connection: binding.connection,
            lease: binding.lease,
        }))
    }
    pub fn adopt_parent_connection(
        &mut self,
        token: &str,
        scope: &DraftScope,
        connection: &Connection,
        target: &NavigationTarget,
        adopt_default: bool,
    ) -> Result<(), String> {
        let running = self.running.as_mut().ok_or("The plugin window is closed")?;
        if running.token != token
            || crate::client::connection_key(connection)?
                != crate::client::connection_key(&target.connection)?
            || target.lease.as_ref().is_some_and(|lease| !lease.is_valid())
        {
            return Err("The verified source changed while activating it".into());
        }
        let Some(binding) = running.sources.ensure(&target.source)? else {
            return Err("The source activation is not verified".into());
        };
        if crate::client::connection_key(&binding.connection)?
            != crate::client::connection_key(connection)?
            || !binding.profiles.contains(&target.profile)
        {
            return Err("The source route no longer matches its verified owner".into());
        }
        running.key = crate::client::connection_key(connection)?;
        running.source_connection = connection.clone();
        running.owner_lease = target.lease.clone();
        running.scope = scope.clone();
        if adopt_default {
            running.request_source = target.source.clone();
            running.request_profile = target.profile.clone();
            running.owner["connection_id"] = target.source.clone().into();
            running.owner["profile"] = target.profile.clone().into();
            running.owner["target_profile"] = target.profile.clone().into();
            running.allowed_profiles = binding.profiles;
        }
        for key in ["session", "runtime_session", "cwd", "model"] {
            running.owner[key] = Value::Null;
        }
        running.owner["busy"] = false.into();
        Ok(())
    }
    pub fn is_open(&self) -> bool {
        self.running.is_some()
    }
    pub fn launch(
        &mut self,
        source: String,
        origin: String,
        scope: DraftScope,
        connection: &Connection,
        owner_lease: Option<crate::connections::Lease>,
        mut owner: Value,
    ) -> Result<(), String> {
        if source.len() > 16 * 1024 * 1024 {
            return Err("The desktop plugin source exceeds the size limit".into());
        }
        let key = crate::client::connection_key(connection)?;
        let source_id = public_source_id(connection, &key);
        owner["connection_id"] = source_id.clone().into();
        let allowed_profiles: BTreeSet<String> = owner["profiles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["name"].as_str().map(str::to_owned))
            .chain([scope.profile.clone()])
            .collect();
        let request_profile = owner["target_profile"]
            .as_str()
            .unwrap_or(&scope.profile)
            .to_owned();
        if !allowed_profiles.contains(&request_profile) {
            return Err("The desktop plugin target profile is no longer served".into());
        }
        let identity = DocumentIdentity {
            source: crate::source::SourceStamp::from_connection(connection)?,
            connection_generation: scope.connection,
            foreground_profile: scope.profile.clone(),
            profile_intent: scope.profile_intent.clone(),
            target_profile: request_profile.clone(),
            document: source.clone(),
            origin: origin.clone(),
        };
        if owner_lease.as_ref().is_some_and(|lease| {
            !lease.is_valid()
                || crate::source::SourceStamp::from_connection(lease.connection())
                    .ok()
                    .as_ref()
                    != Some(&identity.source)
        }) {
            return Err("The plugin source lease is no longer valid".into());
        }
        if let Some(running) = self.running.as_mut() {
            running.sources.refresh()?;
            let birth = running.sources.primary.clone();
            let binding = running.sources.ensure(&birth);
            let valid = match binding {
                Ok(Some(binding)) => {
                    binding.lease.as_ref().is_none_or(|lease| lease.is_valid())
                        && crate::source::SourceStamp::from_connection(&binding.connection)
                            .ok()
                            .as_ref()
                            == Some(&running.identity.source)
                        && binding.profiles.contains(&running.identity.target_profile)
                }
                Ok(None) if running.identity == identity => {
                    return Err("The plugin source is still being verified".into());
                }
                Ok(None) => true,
                Err(_) => false,
            };
            let retained_valid = running
                .owner_lease
                .as_ref()
                .is_none_or(|lease| lease.is_valid());
            let expired_owner =
                !retained_valid && running.identity == identity && owner_lease.is_none();
            if !valid || !retained_valid || !running.process.alive() {
                self.close();
                if expired_owner {
                    return Err("The plugin source lease has retired".into());
                }
            } else if running.identity == identity
                && running.scope.connection == scope.connection
                && running.scope.profile == scope.profile
                && running.scope.profile_intent == scope.profile_intent
                && running.request_profile == request_profile
                && running.key == key
                && (running.owner_lease.is_some() == owner_lease.is_some())
            {
                if running.process.send(json!({"type":"focus"})).is_ok() {
                    return Ok(());
                }
                self.close();
            }
        }
        scoped_snapshot(&mut owner, &scope.profile, &request_profile);
        use sha2::{Digest, Sha256};
        let storage_key = format!(
            "{:x}",
            Sha256::digest(
                format!("{}\0desktop-plugins", storage_owner_key(connection, &key)?).as_bytes()
            )
        );
        let storage_path = crate::client::state_dir()?
            .join("plugin-storage")
            .join(format!("{storage_key}.json"));
        let restored = match std::fs::read(&storage_path) {
            Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
                .map_err(|_| "Saved plugin storage is invalid")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
            Err(_) => return Err("Saved plugin storage could not be read".into()),
        };
        let restore = restored.to_string().replace('<', "\\u003c");
        let assets = plugin_assets()?;
        let html = "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><meta http-equiv=Content-Security-Policy content=\"default-src 'self' blob: data:; script-src 'self' blob: 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self' https: http: wss: ws:; img-src 'self' data: https: http:; media-src 'self' blob: https: http:; frame-src 'none'; object-src 'none'; base-uri 'none'\"><link rel=stylesheet href=host.css><div id=root></div><script type=module src=host.js></script>";
        let html=html.replace("<script type=module src=host.js>",&format!("<script>for(const [k,v]of Object.entries({restore})){{if((k.startsWith('hermes.plugin.')||k==='hermes.desktop.pluginDecisions.v2'||k==='hermes.desktop.disabledPlugins.v1')&&typeof v==='string')localStorage.setItem(k,v);}}</script><script type=module src=host.js>"));
        let mut sources = super::sources::Sources::new(
            connection.clone(),
            source_id.clone(),
            owner["profiles"].as_array().cloned().unwrap_or_default(),
            self.resolver.clone(),
        )?;
        let birth = sources.primary.clone();
        sources.ensure(&birth)?;
        #[cfg(test)]
        let process = if let Some(spawn) = &mut self.spawn_fixture {
            spawn()?
        } else {
            ManagedProcess::Native(spawn_document(
                "Hermes · Desktop plugin",
                html,
                assets,
                false,
            )?)
        };
        #[cfg(not(test))]
        let process = ManagedProcess::Native(spawn_document(
            "Hermes · Desktop plugin",
            html,
            assets,
            false,
        )?);
        self.close();
        let feed = crate::gateway::plugin::PluginFeed::start(
            connection.clone(),
            allowed_profiles.clone(),
            source_id.clone(),
        );
        self.running = Some(Running {
            identity,
            owner_lease,
            window_facts: None,
            opened_at: std::time::Instant::now(),
            token: uuid::Uuid::new_v4().to_string(),
            source_connection: connection.clone(),
            request_source: source_id,
            sources,
            deferred: vec![],
            scope,
            request_profile,
            key,
            process,
            source,
            origin,
            owner,
            allowed_profiles,
            bootstrap_sent: false,
            storage_path,
            storage: None,
            saved_at: std::time::Instant::now(),
            feed,
            uploads: super::uploads::Uploads::default(),
            sockets: crate::gateway::plugin::sockets::SocketBroker::default(),
        });
        self.error = None;
        self.notice = Some("Opening the desktop plugin compatibility window…".into());
        Ok(())
    }
    pub fn close(&mut self) {
        self.save_storage();
        if let Some(running) = &mut self.running {
            running.process.close();
            for event in running.process.poll().into_iter().flatten() {
                if event["type"] == "plugin-storage" && valid_storage(&event["values"]) {
                    running.storage = Some(event["values"].clone());
                }
            }
        }
        self.save_storage();
        self.running = None;
        self.pending.clear();
        self.actions.clear();
        self.ambient.clear();
    }
    pub fn sync(&mut self, mut snapshot: Value) {
        if let Some(running) = &mut self.running {
            let ambient_source = running
                .sources
                .bindings()
                .into_iter()
                .find(|binding| {
                    crate::client::connection_key(&binding.connection)
                        .ok()
                        .as_ref()
                        == Some(&running.key)
                })
                .map(|binding| binding.id);
            if snapshot["profiles_loaded"] != false {
                if let Some(id) = &ambient_source {
                    running.sources.update_profiles(
                        id,
                        snapshot["profiles"].as_array().cloned().unwrap_or_default(),
                    );
                }
            }
            if ambient_source.as_deref() != Some(&running.request_source)
                || snapshot["profiles_loaded"] == false
            {
                for key in ["session", "runtime_session", "cwd", "model"] {
                    snapshot[key] = Value::Null;
                }
                snapshot["busy"] = false.into();
            }
            snapshot["connection_id"] = running.request_source.clone().into();
            snapshot["profiles"] = running.sources.profiles(&running.request_source).into();
            scoped_snapshot(
                &mut snapshot,
                &running.scope.profile,
                &running.request_profile,
            );
            running.allowed_profiles = snapshot["profiles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| row["name"].as_str().map(str::to_owned))
                .collect();
            let birth = running.sources.primary.clone();
            let birth_profiles = running
                .sources
                .profiles(&birth)
                .iter()
                .filter_map(|p| p["name"].as_str().map(str::to_owned))
                .collect();
            running.feed.update_profiles(birth_profiles);
            let bindings = running.sources.bindings();
            running
                .sockets
                .retain_routes(&bindings.iter().map(|binding| binding.id.clone()).collect());
            for binding in bindings {
                running
                    .sockets
                    .retain_source(&binding.id, binding.lease, &binding.profiles);
            }
            if running.owner != snapshot {
                running.owner = snapshot.clone();
                if running.bootstrap_sent {
                    if let Err(error) = running
                        .process
                        .send(json!({"type":"plugin-state","owner":snapshot}))
                    {
                        self.error = Some(error);
                    }
                }
            }
        }
    }
    pub fn poll(&mut self, scope: &DraftScope, connection: &Connection) -> Vec<PageEffect> {
        let Some(running) = &self.running else {
            return vec![];
        };
        let current = crate::client::connection_key(connection);
        if running.scope.connection != scope.connection
            || running.scope.profile != scope.profile
            || current.as_ref().ok() != Some(&running.key)
            || !running.allowed_profiles.contains(&running.request_profile)
        {
            self.close();
            return vec![];
        }
        if let Some(running) = &mut self.running {
            for event in running.feed.poll() {
                match event {
                    Ok(event) => {
                        let message = match event["type"].as_str() {
                            Some("native.plugin.state") => {
                                json!({"type":"plugin-feed-state","state":event["state"]})
                            }
                            Some("native.plugin.request") => {
                                json!({"type":"plugin-server-request","request":event["request"]})
                            }
                            _ if running
                                .allowed_profiles
                                .contains(event["profile"].as_str().unwrap_or_default()) =>
                            {
                                json!({"type":"gateway-event","event":event})
                            }
                            _ => continue,
                        };
                        let _ = running.process.send(message);
                    }
                    Err(error) => {
                        let _ = running.process.send(
                            json!({"type":"plugin-feed-state","state":"error","message":error}),
                        );
                    }
                }
            }
        }
        if let Some(running) = &mut self.running {
            for event in running.sockets.poll() {
                let _ = running.process.send(event);
            }
        }
        let running = self.running.as_mut().unwrap();
        running.sources.poll();
        let mut events = std::mem::take(&mut running.deferred)
            .into_iter()
            .map(|(mut value, deadline)| {
                value["native_deadline_ms"] = json!(
                    deadline
                        .saturating_duration_since(std::time::Instant::now())
                        .as_millis() as u64
                );
                Ok(value)
            })
            .collect::<Vec<_>>();
        events.extend(running.process.poll());
        let mut effects = vec![];
        for event in events {
            let value = match event {
                Ok(value) => value,
                Err(error) => {
                    self.error = Some(error);
                    self.close();
                    break;
                }
            };
            match value["type"].as_str().unwrap_or_default() {
                "host-window-state" if value["_native_host"] == true => {
                    if let (Some(focused), Some(visible)) =
                        (value["focused"].as_bool(), value["visible"].as_bool())
                    {
                        self.running.as_mut().unwrap().window_facts = Some((focused, visible));
                    }
                }
                "plugin-bootstrap-ready" => {
                    let running = self.running.as_mut().unwrap();
                    if !running.bootstrap_sent {
                        running.bootstrap_sent = true;
                        if let Err(error)=running.process.send(json!({"type":"plugin-load","source":running.source,"origin":running.origin,"owner":running.owner})){self.error=Some(error);}
                    }
                }
                "plugin-server-response" => {
                    if let Some(running) = &self.running {
                        if let Err(error) = running.feed.respond(value["response"].clone()) {
                            self.error = Some(error);
                        }
                    }
                }
                "plugin-feed-close" => {
                    if let Some(running) = &mut self.running {
                        running.feed.close();
                        let _ = running
                            .process
                            .send(json!({"type":"plugin-feed-state","state":"closed"}));
                    }
                }
                "plugin-request" => {
                    let id = value["id"].as_str().unwrap_or_default().to_owned();
                    if uuid::Uuid::parse_str(&id).is_err() {
                        continue;
                    }
                    let kind = value["kind"].as_str().unwrap_or_default();
                    let payload = value["payload"].clone();
                    let timeout = value["timeout_ms"]
                        .as_u64()
                        .unwrap_or(30000)
                        .div_ceil(1000)
                        .clamp(1, 86400);
                    if matches!(
                        kind,
                        "rpc"
                            | "api"
                            | "plugin-socket-prepare"
                            | "plugin-gateway-prepare"
                            | "upload-start"
                    ) {
                        let running = self.running.as_mut().unwrap();
                        let data = if kind == "upload-start" {
                            &payload["request"]
                        } else {
                            &payload
                        };
                        let selected = data["connectionId"]
                            .as_str()
                            .unwrap_or(&running.request_source)
                            .to_owned();
                        match running.sources.ensure(&selected) {
                            Ok(None) => {
                                Self::defer(running, value);
                                continue;
                            }
                            Err(error) => {
                                self.reply(&id, Err(error));
                                continue;
                            }
                            Ok(Some(_)) => {}
                        }
                    }
                    if kind == "adopt-sdk-route" {
                        let running = self.running.as_mut().unwrap();
                        let source = payload["connectionId"]
                            .as_str()
                            .unwrap_or(&running.request_source)
                            .to_owned();
                        let profile = payload["profile"]
                            .as_str()
                            .unwrap_or(&running.request_profile)
                            .to_owned();
                        let result=running.sources.ensure(&source).and_then(|binding|binding.ok_or("The source route is still connecting".into())).and_then(|binding|{
                            if !binding.profiles.contains(&profile)||crate::client::connection_key(&binding.connection)?!=running.key{return Err("The SDK activation does not match the native owner's verified source".into())}
                            running.request_source=source;running.request_profile=profile;running.allowed_profiles=binding.profiles;Ok(json!({"ok":true}))
                        });
                        self.reply(&id, result);
                    } else if kind == "registry-metadata" {
                        let result = self.running.as_mut().unwrap().sources.metadata();
                        let result = result.map(|value| {
                            current_metadata(value, &self.running.as_ref().unwrap().request_source)
                        });
                        self.reply(&id, result);
                    } else if kind == "registry-roster" {
                        let running = self.running.as_mut().unwrap();
                        match running.sources.roster() {
                            Ok(None) => {
                                Self::defer(running, value);
                                continue;
                            }
                            result => {
                                let source = self.running.as_ref().unwrap().request_source.clone();
                                self.reply(
                                    &id,
                                    result.map(|v| current_metadata(v.unwrap(), &source)),
                                );
                            }
                        }
                    } else if kind == "source-descriptor" {
                        let running = self.running.as_mut().unwrap();
                        let selected = payload["connectionId"]
                            .as_str()
                            .unwrap_or(&running.request_source)
                            .to_owned();
                        match running.sources.ensure(&selected) {
                            Ok(None) => {
                                Self::defer(running, value);
                                continue;
                            }
                            Err(error) => self.reply(&id, Err(error)),
                            Ok(Some(binding)) => {
                                let result = super::sources::Sources::descriptor(
                                    &binding,
                                    payload["profile"]
                                        .as_str()
                                        .unwrap_or(&running.request_profile),
                                );
                                self.reply(&id, result);
                            }
                        }
                    } else if kind.starts_with("plugin-socket-") || kind == "plugin-gateway-prepare"
                    {
                        let result = self.socket_action(kind, &payload, connection);
                        self.reply(&id, result);
                    } else if matches!(kind, "upload-start" | "upload-chunk" | "upload-abort") {
                        let result = self.upload_action(kind, &payload);
                        self.reply(&id, result);
                    } else if matches!(kind, "rpc" | "api") {
                        let mut metadata = payload.clone();
                        let wire = metadata
                            .as_object_mut()
                            .and_then(|object| object.remove("upload"));
                        let upload = match wire.filter(|wire| !wire.is_null()) {
                            Some(wire) => self
                                .running
                                .as_mut()
                                .unwrap()
                                .uploads
                                .take(&wire, &metadata)
                                .map(Some),
                            None => Ok(None),
                        };
                        let running = self.running.as_ref().unwrap();
                        let selected = payload["connectionId"]
                            .as_str()
                            .unwrap_or(&running.request_source)
                            .to_owned();
                        let binding = self
                            .running
                            .as_mut()
                            .unwrap()
                            .sources
                            .ensure(&selected)
                            .and_then(|binding| {
                                binding.ok_or("The source is still connecting".into())
                            });
                        let running = self.running.as_ref().unwrap();
                        let request = binding.and_then(|binding| {
                            upload.and_then(|upload| {
                                request(
                                    kind,
                                    &metadata,
                                    &running.request_profile,
                                    &binding.profiles,
                                    binding.connection.route_owner().is_some(),
                                )
                                .map(|request| {
                                    let request = binding.request(request);
                                    if let Some(upload) = upload {
                                        request.upload(
                                            upload.filename,
                                            upload.content_type,
                                            upload.bytes,
                                        )
                                    } else {
                                        request
                                    }
                                })
                            })
                        });
                        match request {
                            Ok(request) => {
                                let op = format!("plugin-host.{id}");
                                self.pending.insert(op.clone(), id);
                                effects.push((op, request.timeout_secs(timeout)));
                            }
                            Err(error) => self.reply(&id, Err(error)),
                        }
                    } else if matches!(
                        kind,
                        "open-external"
                            | "clipboard"
                            | "reveal-path"
                            | "notify"
                            | "native-notify"
                            | "claim-ambient-cue"
                            | "select-paths"
                            | "select-save-path"
                            | "open-session"
                            | "new-chat"
                            | "ensure-agent"
                    ) {
                        let running = self.running.as_ref().unwrap();
                        let mut payload = payload;
                        if let Some(object) = payload.as_object_mut() {
                            object.insert("native_owner".into(),json!({"profile":running.request_profile,"connection_id":running.request_source,"lifecycle_key":running.key,"window_token":running.token}));
                        }
                        self.actions.push(HostAction {
                            id,
                            kind: kind.into(),
                            payload,
                        });
                    } else {
                        self.reply(&id, Err("This native host operation is unavailable".into()));
                    }
                }
                "plugin-storage" => {
                    let values = &value["values"];
                    if valid_storage(values) {
                        self.running.as_mut().unwrap().storage = Some(values.clone());
                    }
                }
                "plugin-error" | "error" => {
                    self.error = Some(
                        value["message"]
                            .as_str()
                            .unwrap_or("The plugin host reported an error")
                            .chars()
                            .take(500)
                            .collect(),
                    )
                }
                "closed" if value["_native_host"] != true => {}
                "plugin-loaded" => {
                    self.notice = Some(format!(
                        "Desktop plugin {} loaded",
                        value["id"].as_str().unwrap_or_default()
                    ))
                }
                "plugin-rendered" => {
                    self.notice =
                        Some("The desktop plugin rendered in its compatibility window".into())
                }
                "plugin-navigate" => self.actions.push(HostAction {
                    id: String::new(),
                    kind: "navigate".into(),
                    payload: value["path"].clone(),
                }),
                "closed" => {
                    self.close();
                    break;
                }
                _ => {}
            }
        }
        if self.running.as_ref().is_some_and(|running| {
            running.saved_at.elapsed() >= std::time::Duration::from_millis(500)
        }) {
            self.save_storage();
        }
        effects
    }
    fn defer(running: &mut Running, value: Value) {
        let remaining = value["native_deadline_ms"]
            .as_u64()
            .unwrap_or_else(|| value["timeout_ms"].as_u64().unwrap_or(30000));
        if remaining == 0 {
            let _=running.process.send(json!({"type":"plugin-response","id":value["id"],"error":"The saved source did not connect before the request deadline"}));
            return;
        }
        running.deferred.push((
            value,
            std::time::Instant::now() + std::time::Duration::from_millis(remaining),
        ));
    }
    fn save_storage(&mut self) {
        let Some(running) = &mut self.running else {
            return;
        };
        let Some(value) = running.storage.take() else {
            return;
        };
        match serde_json::to_vec(&value)
            .map_err(|_| "Plugin storage could not be encoded".to_owned())
            .and_then(|bytes| crate::client::private_write(&running.storage_path, &bytes))
        {
            Ok(()) => running.saved_at = std::time::Instant::now(),
            Err(error) => {
                running.storage = Some(value);
                self.error = Some(error);
            }
        }
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) {
        if let Some(operation) = op.strip_suffix(".event") {
            if !self.pending.contains_key(operation) {
                return;
            }
            if let Ok(event) = result {
                if let Some(running) = &mut self.running {
                    // Per-call sockets may launch in a different profile. Only an explicit
                    // event owner can be trusted here; the persistent tap binds legacy events.
                    if let Some(event) = crate::gateway::plugin::normalize_event(
                        &event,
                        "",
                        &running.allowed_profiles,
                        &running.request_source,
                    ) {
                        let _ = running
                            .process
                            .send(json!({"type":"gateway-event","event":event}));
                    }
                }
            }
            return;
        }
        if let Some(id) = self.pending.remove(op) {
            self.reply(&id, result);
        }
    }
    pub fn reply(&mut self, id: &str, result: Result<Value, String>) {
        let Some(running) = &mut self.running else {
            return;
        };
        let value = match result {
            Ok(value) => json!({"type":"plugin-response","id":id,"value":value}),
            Err(error) => json!({"type":"plugin-response","id":id,"error":error}),
        };
        if let Err(error) = running.process.send(value) {
            self.error = Some(error);
        }
    }
    fn socket_action(
        &mut self,
        kind: &str,
        payload: &Value,
        connection: &Connection,
    ) -> Result<Value, String> {
        let running = self.running.as_mut().ok_or("The plugin window is closed")?;
        if kind == "plugin-socket-prepare" || kind == "plugin-gateway-prepare" {
            let selected = payload["connectionId"]
                .as_str()
                .unwrap_or(&running.request_source)
                .to_owned();
            let binding = running
                .sources
                .ensure(&selected)?
                .ok_or("The source is still connecting")?;
            let profile = payload["profile"]
                .as_str()
                .unwrap_or(&running.request_profile);
            if !binding.profiles.contains(profile) {
                return Err("The plugin stream profile is no longer served by its source".into());
            }
            let receipt = prepare_plugin_socket(
                &mut running.sockets,
                kind,
                &binding.connection,
                profile,
                payload["pluginId"].as_str().unwrap_or_default(),
                payload["path"].as_str().unwrap_or_default(),
            )?;
            if receipt["url"].is_null() {
                return Ok(receipt);
            }
            running.sockets.authorize_profiles(
                receipt["receipt"].as_str().unwrap_or_default(),
                &binding.profiles,
            );
            running.sockets.bind_prepared(
                receipt["receipt"].as_str().unwrap_or_default(),
                &selected,
                binding.connection,
                binding.lease,
            )?;
            Ok(receipt)
        } else {
            let receipt = payload["receipt"]
                .as_str()
                .ok_or("The plugin stream receipt is missing")?;
            match kind {
                "plugin-socket-open" => running.sockets.open(receipt, connection.clone()),
                "plugin-socket-send" => running.sockets.command(receipt, payload["frame"].clone()),
                "plugin-socket-close" => running.sockets.command(
                    receipt,
                    json!({"type":"close","code":payload["code"],"reason":payload["reason"]}),
                ),
                _ => Err("Unknown plugin stream operation".into()),
            }
        }
    }
    fn upload_action(&mut self, kind: &str, payload: &Value) -> Result<Value, String> {
        let running = self.running.as_mut().ok_or("The plugin window is closed")?;
        match kind {
            "upload-start" => {
                let metadata = &payload["request"];
                let selected = metadata["connectionId"]
                    .as_str()
                    .unwrap_or(&running.request_source)
                    .to_owned();
                let binding = running
                    .sources
                    .ensure(&selected)?
                    .ok_or("The source is still connecting")?;
                request(
                    "api",
                    metadata,
                    &running.request_profile,
                    &binding.profiles,
                    binding.connection.route_owner().is_some(),
                )?;
                running.uploads.start(payload)
            }
            "upload-chunk" => running.uploads.chunk(payload),
            _ => Ok(running
                .uploads
                .abort(payload["transfer_id"].as_str().unwrap_or_default())),
        }
    }
    pub fn take_actions(&mut self) -> Vec<HostAction> {
        std::mem::take(&mut self.actions)
    }
}
fn public_source_id(connection: &Connection, key: &str) -> String {
    if let Some(owner) = connection.route_owner() {
        return format!("routed:{}", owner.fingerprint());
    }
    match connection {
        Connection::Saved { id, revision, .. } => format!("saved:{id}:{revision}"),
        _ => format!("native:{key}"),
    }
}
fn storage_owner_key(connection: &Connection, key: &str) -> Result<String, String> {
    if connection.route_owner().is_some() {
        return crate::client::connection_storage_key(connection);
    }
    use sha2::{Digest, Sha256};
    let Connection::Saved {
        id,
        revision,
        inner,
    } = connection
    else {
        return Ok(key.into());
    };
    let credential = match inner.transport() {
        Connection::Url { endpoint, token } => {
            let (_, embedded) = crate::gateway::endpoint_url(endpoint)?;
            token
                .as_deref()
                .or(embedded.as_deref())
                .unwrap_or("")
                .to_owned()
        }
        Connection::Local { .. } => crate::client::connection_key(inner)?,
        _ => unreachable!(),
    };
    Ok(format!(
        "{:x}",
        Sha256::digest(format!("saved:{id}:{revision}\0{credential}").as_bytes())
    ))
}
fn current_metadata(mut value: Value, current: &str) -> Value {
    value["primary"] = current.into();
    if let Some(row) = value["connections"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == current))
        .cloned()
    {
        value["current"] = row;
    }
    value
}
fn valid_storage(values: &Value) -> bool {
    values.to_string().len() <= 5 * 1024 * 1024
        && values.as_object().is_some_and(|map| {
            map.iter().all(|(key, value)| {
                (key.starts_with("hermes.plugin.")
                    || matches!(
                        key.as_str(),
                        "hermes.desktop.pluginDecisions.v2" | "hermes.desktop.disabledPlugins.v1"
                    ))
                    && value.is_string()
            })
        })
}
fn scoped_snapshot(snapshot: &mut Value, ambient_profile: &str, target_profile: &str) {
    snapshot["profile"] = target_profile.into();
    snapshot["target_profile"] = target_profile.into();
    if target_profile != ambient_profile {
        for key in ["session", "runtime_session", "cwd", "model"] {
            snapshot[key] = Value::Null;
        }
        snapshot["busy"] = false.into();
    }
}
fn plugin_assets() -> Result<BTreeMap<String, String>, String> {
    let compressed = include_bytes!("../../assets/plugin-host/assets.json.gz");
    let decoder = flate2::read::GzDecoder::new(compressed.as_slice());
    let mut json = String::new();
    decoder
        .take((64 * 1024 * 1024 + 1) as u64)
        .read_to_string(&mut json)
        .map_err(|_| "The embedded plugin SDK could not be read")?;
    if json.len() > 64 * 1024 * 1024 {
        return Err("The embedded plugin SDK exceeds its size limit".into());
    }
    serde_json::from_str(&json).map_err(|_| "The embedded plugin SDK is invalid".into())
}
fn request(
    kind: &str,
    payload: &Value,
    default_profile: &str,
    allowed: &BTreeSet<String>,
    routed: bool,
) -> Result<PageRequest, String> {
    if payload
        .get("upload")
        .is_some_and(|upload| !upload.is_null())
    {
        return Err("This native plugin host cannot yet send multipart uploads".into());
    }
    if kind == "rpc" {
        let method = payload["method"]
            .as_str()
            .ok_or("The plugin request has no method")?;
        if method.is_empty()
            || method.len() > 200
            || !method
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err("Invalid plugin RPC method".into());
        }
        let params = payload.get("params").cloned().unwrap_or_else(|| json!({}));
        let original = params
            .get("profile")
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("Invalid plugin RPC profile selector")
            })
            .transpose()?;
        let profile = original
            .as_deref()
            .filter(|profile| !routed || !profile.is_empty())
            .unwrap_or(default_profile);
        if !allowed.contains(profile) {
            return Err("The plugin request names an unserved profile".into());
        }
        let request = PageRequest::plugin_rpc(method, params.clone()).profile(profile);
        return Ok(if routed {
            request.provenance(crate::gateway::prepared::ProfileProvenance::Ambient(
                original,
            ))
        } else {
            request
        });
    }
    let path = payload["path"]
        .as_str()
        .ok_or("The plugin request has no path")?;
    if !path.starts_with("/api/")
        || path.contains('#')
        || path.contains("..")
        || path.contains('\\')
    {
        return Err("The plugin API path must stay on its owning gateway".into());
    }
    let url =
        url::Url::parse(&format!("http://native{path}")).map_err(|_| "Invalid plugin API path")?;
    if url.host_str() != Some("native") {
        return Err("The plugin request changed gateway owner".into());
    }
    let query: Vec<_> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    if query.iter().any(|(key, _)| key == "token") {
        return Err("Plugins cannot replace gateway authentication".into());
    }
    let selector = query
        .iter()
        .any(|(key, value)| key == "profile" && value == "all")
        && url.path() == "/api/profiles/sessions"
        && payload["method"].as_str().unwrap_or("GET") == "GET";
    let profile = payload["profile"]
        .as_str()
        .filter(|profile| !selector || *profile != "all")
        .or_else(|| {
            query
                .iter()
                .find(|(key, value)| key == "profile" && (!selector || value != "all"))
                .map(|(_, value)| value.as_str())
        })
        .or(payload["body"]["profile"].as_str())
        .unwrap_or(default_profile);
    let profile = if routed && profile.is_empty() {
        default_profile
    } else {
        profile
    };
    if !allowed.contains(profile) {
        return Err("The plugin request names an unserved profile".into());
    }
    if query.iter().any(|(key, value)| {
        key == "profile"
            && value != profile
            && !(selector && value == "all")
            && !(routed && value.is_empty())
    }) {
        return Err("The plugin request disagrees about its query profile owner".into());
    }
    let mut body = payload.get("body").cloned().unwrap_or(Value::Null);
    if let Some(body_profile) = body["profile"].as_str() {
        if body_profile != profile && !(routed && body_profile.is_empty()) {
            return Err("The plugin request disagrees about its profile owner".into());
        }
    }
    let original = query
        .iter()
        .find(|(key, _)| key == "profile")
        .map(|(_, value)| value.clone())
        .or_else(|| payload["profile"].as_str().map(str::to_owned));
    let mut request = match payload["method"].as_str().unwrap_or("GET") {
        "GET" => PageRequest::get(url.path()),
        "POST" => PageRequest::post(url.path(), body.take()),
        "PUT" => PageRequest::put(url.path(), body.take()),
        "PATCH" => PageRequest::patch(url.path(), body.take()),
        "DELETE" => PageRequest::delete(url.path(), body.take()),
        _ => return Err("The plugin requested an unsupported HTTP method".into()),
    }
    .profile(profile);
    for (key, value) in query {
        if routed || key != "profile" || selector {
            request = request.query(key, value);
        }
    }
    Ok(if routed {
        request.provenance(crate::gateway::prepared::ProfileProvenance::Ambient(
            original,
        ))
    } else {
        request
    })
}
fn prepare_plugin_socket(
    sockets: &mut crate::gateway::plugin::sockets::SocketBroker,
    kind: &str,
    connection: &Connection,
    profile: &str,
    plugin: &str,
    path: &str,
) -> Result<Value, String> {
    if kind == "plugin-socket-prepare" && connection.wire_policy().is_some_and(|p| p.is_cloud()) {
        return Ok(json!({"url": null}));
    }
    if kind == "plugin-gateway-prepare" {
        sockets.prepare_gateway(
            profile,
            matches!(connection.transport(), Connection::Local { .. }),
        )
    } else {
        sockets.prepare(profile, plugin, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reopen_uses_owned_live_document_and_recovers_failures() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };
        let spawned = Arc::new(Mutex::new(Vec::<Arc<Mutex<FixtureProcess>>>::new()));
        let fail = Arc::new(AtomicBool::new(false));
        let mut hosts = PluginHosts::new();
        let spawned_copy = spawned.clone();
        let fail_copy = fail.clone();
        hosts.spawn_fixture = Some(Box::new(move || {
            if fail_copy.load(Ordering::Relaxed) {
                return Err("fixture spawn failed".into());
            }
            let process = Arc::new(Mutex::new(FixtureProcess {
                alive: true,
                ..Default::default()
            }));
            spawned_copy.lock().unwrap().push(process.clone());
            Ok(ManagedProcess::Fixture(process))
        }));
        let connection = Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: Some("synthetic-a".into()),
        };
        let scope = DraftScope {
            draft_origin: None,
            connection: 1,
            profile: "default".into(),
            profile_intent: crate::source::ProfileIntent::Backend,
            session: None,
        };
        let owner = json!({"profiles":[{"name":"default"}, {"name":"other"}]});
        let open =
            |hosts: &mut PluginHosts, scope: DraftScope, connection: &Connection, owner: Value| {
                hosts.launch(
                    "fixture-document".into(),
                    "fixture-origin".into(),
                    scope,
                    connection,
                    None,
                    owner,
                )
            };
        open(&mut hosts, scope.clone(), &connection, owner.clone()).unwrap();
        let token = hosts.window_token().unwrap();
        let first = spawned.lock().unwrap()[0].clone();
        first.lock().unwrap().state = "sdk-state".into();
        hosts.pending.insert("plugin-host.old".into(), "old".into());
        open(&mut hosts, scope.clone(), &connection, owner.clone()).unwrap();
        assert_eq!(hosts.window_token().as_deref(), Some(token.as_str()));
        assert_eq!(spawned.lock().unwrap().len(), 1);
        assert_eq!(first.lock().unwrap().state, "sdk-state");
        assert_eq!(
            first.lock().unwrap().commands.last().unwrap()["type"],
            "focus"
        );
        hosts.reply_owned(&token, "old", Ok(json!({"ok":true})));
        assert_eq!(first.lock().unwrap().commands.last().unwrap()["id"], "old");
        assert!(hosts.pending.contains_key("plugin-host.old"));
        let mut other = scope.clone();
        other.profile_intent = crate::source::ProfileIntent::NativeSelf;
        fail.store(true, Ordering::Relaxed);
        assert!(open(&mut hosts, other.clone(), &connection, owner.clone()).is_err());
        assert_eq!(hosts.window_token().as_deref(), Some(token.as_str()));
        assert_eq!(first.lock().unwrap().closes, 0);
        fail.store(false, Ordering::Relaxed);
        open(&mut hosts, other, &connection, owner.clone()).unwrap();
        assert_ne!(hosts.window_token().as_deref(), Some(token.as_str()));
        assert_eq!(first.lock().unwrap().closes, 1);
        let second = spawned.lock().unwrap()[1].clone();
        hosts.reply_owned(&token, "late", Ok(json!({})));
        hosts.apply(
            "plugin-host.old.event",
            Ok(json!({"type":"message.delta","profile":"default"})),
        );
        assert!(second.lock().unwrap().commands.is_empty());
        second.lock().unwrap().fail_focus = true;
        let current = hosts.running.as_ref().unwrap().scope.clone();
        open(&mut hosts, current.clone(), &connection, owner.clone()).unwrap();
        assert_eq!(second.lock().unwrap().closes, 1);
        let third = spawned.lock().unwrap()[2].clone();
        third.lock().unwrap().alive = false;
        open(&mut hosts, current, &connection, owner).unwrap();
        assert_eq!(spawned.lock().unwrap().len(), 4);
        let fourth = spawned.lock().unwrap()[3].clone();
        hosts
            .running
            .as_mut()
            .unwrap()
            .sources
            .retire_primary_for_test();
        let current = hosts.running.as_ref().unwrap().scope.clone();
        open(
            &mut hosts,
            current.clone(),
            &connection,
            json!({"profiles":[{"name":"default"},{"name":"other"}]}),
        )
        .unwrap();
        assert_eq!(fourth.lock().unwrap().closes, 1);
        let fifth = spawned.lock().unwrap()[4].clone();
        let rotated = Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: Some("synthetic-b".into()),
        };
        open(
            &mut hosts,
            current.clone(),
            &rotated,
            json!({"profiles":[{"name":"default"},{"name":"other"}]}),
        )
        .unwrap();
        assert_eq!(fifth.lock().unwrap().closes, 1);
        let sixth = spawned.lock().unwrap()[5].clone();
        open(
            &mut hosts,
            current,
            &rotated,
            json!({"profiles":[{"name":"default"},{"name":"other"}],"target_profile":"other"}),
        )
        .unwrap();
        assert_eq!(sixth.lock().unwrap().closes, 1);
        hosts.close();
    }

    #[test]
    fn cloud_plugin_namespace_uses_polling_and_gateway_keeps_native_bridge() {
        use crate::gateway::prepared::{CloudHttp, CloudRuntime, WirePolicy};
        use std::sync::Arc;
        struct NoMint;
        impl CloudRuntime for NoMint {
            fn prepare_http(
                &self,
                _: reqwest::Method,
                _: &url::Url,
                _: std::time::Instant,
            ) -> Result<CloudHttp, String> {
                panic!("unexpected HTTP")
            }
            fn retry_http_unauthorized(
                &self,
                _: CloudHttp,
                _: std::time::Instant,
            ) -> Result<CloudHttp, String> {
                panic!("unexpected retry")
            }
            fn prepare_ws(
                &self,
                _: &url::Url,
                _: std::time::Instant,
            ) -> Result<tungstenite::handshake::client::Request, String> {
                panic!("unexpected mint")
            }
            fn runtime_fingerprint(&self) -> String {
                "poll-fixture".into()
            }
        }
        let connection = Connection::Routed {
            actual: Box::new(Connection::Url {
                endpoint: "http://127.0.0.1:1/".into(),
                token: None,
            }),
            owner: Arc::new(crate::source::RouteOwner::legacy("poll-fixture").unwrap()),
            policy: Arc::new(
                WirePolicy::cloud(
                    crate::gateway::route::ProfilePolicy::identity(),
                    Arc::new(NoMint),
                )
                .unwrap(),
            ),
        };
        let mut broker = crate::gateway::plugin::sockets::SocketBroker::default();
        let polling = prepare_plugin_socket(
            &mut broker,
            "plugin-socket-prepare",
            &connection,
            "work",
            "board",
            "/events",
        )
        .unwrap();
        assert!(polling["url"].is_null());
        assert!(polling.get("receipt").is_none());
        let gateway = prepare_plugin_socket(
            &mut broker,
            "plugin-gateway-prepare",
            &connection,
            "work",
            "",
            "",
        )
        .unwrap();
        assert!(gateway["receipt"].as_str().is_some());
        assert!(
            gateway["url"]
                .as_str()
                .unwrap()
                .starts_with("ws://hermes-native-gateway.invalid/")
        );
    }
    #[test]
    fn saved_plugin_identity_and_storage_survive_forward_port_changes_but_not_auth_rotation() {
        let make = |port, token: &str| Connection::Saved {
            id: "saved-fixture".into(),
            revision: "revision-fixture".into(),
            inner: Box::new(Connection::Url {
                endpoint: format!("http://127.0.0.1:{port}"),
                token: Some(token.into()),
            }),
        };
        let first = make(9119, "synthetic-a");
        let next = make(19119, "synthetic-a");
        let rotated = make(19119, "synthetic-b");
        assert_eq!(
            public_source_id(&first, "one"),
            public_source_id(&next, "two")
        );
        assert_eq!(
            storage_owner_key(&first, "one").unwrap(),
            storage_owner_key(&next, "two").unwrap()
        );
        assert_ne!(
            storage_owner_key(&first, "one").unwrap(),
            storage_owner_key(&rotated, "two").unwrap()
        );
    }
    #[test]
    fn alternate_profile_snapshot_does_not_inherit_an_ambient_session() {
        let mut snapshot = json!({"profile":"default","session":"private","runtime_session":"runtime","model":"model","cwd":"/ambient","busy":true});
        scoped_snapshot(&mut snapshot, "default", "research");
        assert_eq!(snapshot["profile"], "research");
        assert_eq!(snapshot["target_profile"], "research");
        assert!(snapshot["session"].is_null());
        assert!(snapshot["runtime_session"].is_null());
        assert!(snapshot["cwd"].is_null());
        assert_eq!(snapshot["busy"], false);
    }
    #[test]
    fn routed_sdk_requests_keep_ambient_absence_empty_and_declared_query_owner() {
        use crate::gateway::prepared::ProfileProvenance;
        let profiles = ["default".into(), "dixie".into()].into_iter().collect();
        let absent = request(
            "rpc",
            &json!({"method":"config.get","params":{}}),
            "default",
            &profiles,
            true,
        )
        .unwrap();
        assert_eq!(absent.provenance, Some(ProfileProvenance::Ambient(None)));
        assert!(absent.body.get("profile").is_none());
        let empty = request(
            "rpc",
            &json!({"method":"config.get","params":{"profile":""}}),
            "default",
            &profiles,
            true,
        )
        .unwrap();
        assert_eq!(
            empty.provenance,
            Some(ProfileProvenance::Ambient(Some("".into())))
        );
        assert_eq!(empty.body["profile"], "");
        let declared = request(
            "api",
            &json!({"path":"/api/config?profile=default&key=model"}),
            "dixie",
            &profiles,
            true,
        )
        .unwrap();
        assert!(
            declared
                .query
                .contains(&("profile".into(), "default".into()))
        );
        assert!(declared.query.contains(&("key".into(), "model".into())));
        assert_eq!(
            declared.provenance,
            Some(ProfileProvenance::Ambient(Some("default".into())))
        );
    }
    #[test]
    fn native_plugin_requests_cannot_swap_authentication_or_implicit_owner() {
        let profiles = ["research".into(), "default".into()].into_iter().collect();
        assert!(
            request(
                "api",
                &json!({"path":"https://other.invalid/api/config"}),
                "research",
                &profiles,
                false
            )
            .is_err()
        );
        assert!(
            request(
                "api",
                &json!({"path":"/api/config?token=other"}),
                "research",
                &profiles,
                false
            )
            .is_err()
        );
        assert!(
            request(
                "rpc",
                &json!({"method":"config.set","params":{"profile":"unknown"}}),
                "research",
                &profiles,
                false
            )
            .is_err()
        );
        let req = request(
            "rpc",
            &json!({"method":"config.get","params":{}}),
            "research",
            &profiles,
            false,
        )
        .unwrap();
        assert_eq!(req.profile.as_deref(), Some("research"));
        let req = request(
            "api",
            &json!({"path":"/api/profiles/sessions?profile=all"}),
            "research",
            &profiles,
            false,
        )
        .unwrap();
        assert_eq!(req.profile.as_deref(), Some("research"));
        assert!(req.query.contains(&("profile".into(), "all".into())));
        let req = request(
            "api",
            &json!({"path":"/api/profiles/sessions?profile=all","profile":"research"}),
            "default",
            &profiles,
            false,
        )
        .unwrap();
        assert_eq!(req.profile.as_deref(), Some("research"));
        assert!(req.query.contains(&("profile".into(), "all".into())));
        assert!(
            request(
                "api",
                &json!({"path":"/api/config?profile=default","profile":"research"}),
                "research",
                &profiles,
                false
            )
            .is_err()
        );
    }
}
