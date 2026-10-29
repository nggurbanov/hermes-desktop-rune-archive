use super::*;
mod leases;
use std::{fs, net::TcpListener};

pub(super) trait Secrets: Send {
    fn get(&mut self, id: &str) -> Result<String, String>;
    fn set(&mut self, id: &str, value: &str) -> Result<(), String>;
    fn delete(&mut self, id: &str) -> Result<(), String>;
}
pub(super) trait Metadata: Send {
    fn load(&mut self) -> Result<Registry, String>;
    fn save(&mut self, registry: &Registry) -> Result<(), String>;
}
pub(super) trait Probe: Send {
    fn check(&mut self, connection: &Connection) -> Result<Vec<crate::gateway::Profile>, String>;
}
pub(super) trait Tunnel: Send {
    fn exited(&mut self) -> Option<String>;
    fn close(&mut self);
}
pub(super) trait Tunnels: Send {
    fn open(&mut self, spec: &Ssh) -> Result<Box<dyn Tunnel>, String>;
}
struct SystemSecrets;
impl Secrets for SystemSecrets {
    fn get(&mut self, id: &str) -> Result<String, String> {
        keyring::Entry::new("Hermes Rune",id).map_err(|_|"The OS credential store could not be opened")?.get_password().map_err(|_|"The saved gateway credential is missing or the OS credential store is locked. Edit the connection to enter it again".into())
    }
    fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new("Hermes Rune",id).map_err(|_|"The OS credential store could not be opened")?.set_password(value).map_err(|_|"The gateway token could not be saved in the OS credential store. Unlock the keychain and try again".into())
    }
    fn delete(&mut self, id: &str) -> Result<(), String> {
        match keyring::Entry::new("Hermes Rune", id)
            .map_err(|_| "The OS credential store could not be opened")?
            .delete_credential()
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(
                "An obsolete gateway token could not be removed from the OS credential store"
                    .into(),
            ),
        }
    }
}
struct JsonMetadata(PathBuf);
impl Metadata for JsonMetadata {
    fn load(&mut self) -> Result<Registry, String> {
        match fs::read(&self.0) {
            Ok(bytes) => {
                if bytes.len() > 1024 * 1024 {
                    return Err("The saved connection registry is too large".into());
                }
                let registry: Registry = serde_json::from_slice(&bytes)
                    .map_err(|_| "The saved connection registry is invalid")?;
                if registry.version != 1 {
                    return Err("This saved connection registry requires a newer Hermes app".into());
                }
                for row in &registry.rows {
                    uuid::Uuid::parse_str(&row.id)
                        .map_err(|_| "A saved connection has an invalid identity")?;
                    uuid::Uuid::parse_str(&row.revision)
                        .map_err(|_| "A saved connection has an invalid owner revision")?;
                }
                Ok(registry)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::empty()),
            Err(_) => Err("The saved connection registry could not be read".into()),
        }
    }
    fn save(&mut self, registry: &Registry) -> Result<(), String> {
        crate::client::private_write(
            &self.0,
            &serde_json::to_vec_pretty(registry).map_err(|_| "Connections could not be encoded")?,
        )
    }
}
struct GatewayProbe;
impl Probe for GatewayProbe {
    fn check(&mut self, connection: &Connection) -> Result<Vec<crate::gateway::Profile>, String> {
        let mut gateway = crate::gateway::Gateway::connect(connection)?;
        gateway.profiles()
    }
}
struct Active {
    id: String,
    revision: String,
    port: u16,
    tunnel: Box<dyn Tunnel>,
}
pub(super) struct Core {
    registry: Registry,
    secrets: Box<dyn Secrets>,
    metadata: Box<dyn Metadata>,
    probe: Box<dyn Probe>,
    tunnels: Box<dyn Tunnels>,
    active: Option<Active>,
    active_owner: Option<(String, String)>,
    outbox: std::collections::VecDeque<(String, String, String)>,
    leases: std::collections::BTreeMap<(String, String), super::leases::Entry>,
    primary_lease: Option<Lease>,
}
impl Core {
    pub fn system(path: PathBuf) -> Self {
        Self {
            registry: Registry::empty(),
            secrets: Box::new(SystemSecrets),
            metadata: Box::new(JsonMetadata(path)),
            probe: Box::new(GatewayProbe),
            tunnels: Box::new(super::ssh::OpenSsh),
            active: None,
            active_owner: None,
            outbox: std::collections::VecDeque::new(),
            leases: std::collections::BTreeMap::new(),
            primary_lease: None,
        }
    }
    pub fn execute(&mut self, command: Command) -> Result<Outcome, String> {
        match command {
            Command::Load => {
                self.registry = self.metadata.load()?;
                let warning = self.cleanup();
                Ok(Outcome::Registry(self.registry.clone(), warning))
            }
            Command::Save(input) => self.save(input),
            Command::ConnectOnce(input) => self.connect_once(input),
            Command::Forget(id) => self.forget(&id),
            Command::Select(id) => self.select(&id),
            Command::CommitLease(lease, profile) => {
                self.commit_lease_for(lease, profile.as_deref())
            }
        }
    }
    fn save(&mut self, mut input: Input) -> Result<Outcome, String> {
        let previous = input
            .id
            .as_ref()
            .and_then(|id| self.registry.rows.iter().find(|r| &r.id == id))
            .cloned();
        if input.id.is_some() && previous.is_none() {
            return Err(
                "This saved connection was removed; refresh the list before editing".into(),
            );
        }
        let embedded = normalize_destination(
            &mut input.destination,
            previous.as_ref().map(|r| &r.destination),
        )?;
        if let Some(token) = embedded {
            match input.secret {
                SecretChange::Keep => input.secret = SecretChange::Set(token),
                _ => {
                    return Err(
                        "Enter the gateway token in one place: the token field or the pasted URL"
                            .into(),
                    );
                }
            }
        }
        if matches!(input.destination, Destination::Local { .. }) {
            input.secret = SecretChange::Clear;
        }
        let target_changed = previous
            .as_ref()
            .is_some_and(|p| p.destination != input.destination);
        if target_changed
            && previous.as_ref().is_some_and(Saved::has_credential)
            && matches!(input.secret, SecretChange::Keep)
        {
            return Err("The destination changed. Enter its token or clear the saved credential before saving".into());
        }
        let old_credential = previous.as_ref().and_then(|r| r.credential.clone());
        let mut provisional = None;
        let credential = match input.secret {
            SecretChange::Keep => old_credential.clone(),
            SecretChange::Clear => None,
            SecretChange::Set(token) => {
                if matches!(input.destination, Destination::Local { .. }) {
                    return Err(
                        "Local native connections use private tickets rather than stored tokens"
                            .into(),
                    );
                }
                if token.is_empty() || token.len() > 16384 || token.chars().any(char::is_control) {
                    return Err("The gateway token is invalid".into());
                }
                let id = format!("connection-{}", uuid::Uuid::new_v4());
                self.secrets.set(&id, &token)?;
                provisional = Some(id.clone());
                Some(id)
            }
        };
        let principal_changed = target_changed || credential != old_credential;
        let id = previous
            .as_ref()
            .map(|r| r.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let revision = previous
            .as_ref()
            .filter(|_| !principal_changed)
            .map(|r| r.revision.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let saved = Saved {
            id: id.clone(),
            revision,
            label: input.label,
            destination: input.destination,
            credential,
        };
        let mut next = self.registry.clone();
        if let Some(index) = next.rows.iter().position(|r| r.id == id) {
            next.rows[index] = saved;
        } else {
            next.rows.push(saved);
        }
        if old_credential != next.rows.iter().find(|r| r.id == id).unwrap().credential {
            if let Some(old) = old_credential {
                next.cleanup.push(old);
            }
        }
        if let Err(error) = self.metadata.save(&next) {
            if let Some(id) = provisional {
                if self.secrets.delete(&id).is_err() {
                    return Err(format!(
                        "{error}. The provisional credential also needs cleanup in the OS keychain"
                    ));
                }
            }
            return Err(error);
        }
        self.registry = next;
        let warning = self.cleanup();
        Ok(Outcome::Registry(
            self.registry.clone(),
            Some(
                warning
                    .unwrap_or_else(|| "Connection saved. Connect to use this destination".into()),
            ),
        ))
    }
    fn connect_once(&mut self, mut input: Input) -> Result<Outcome, String> {
        if matches!(input.destination, Destination::Ssh(_)) {
            return Err(
                "Save an SSH destination first so its forwarded port remains stable".into(),
            );
        }
        let previous = input
            .id
            .as_ref()
            .and_then(|id| self.registry.rows.iter().find(|row| &row.id == id))
            .cloned();
        if let Some(embedded) = normalize_destination(
            &mut input.destination,
            previous.as_ref().map(|r| &r.destination),
        )? {
            if matches!(input.secret, SecretChange::Keep) {
                input.secret = SecretChange::Set(embedded);
            } else {
                return Err("Enter the token in one place".into());
            }
        }
        if previous
            .as_ref()
            .is_some_and(|row| row.destination != input.destination && row.has_credential())
            && matches!(input.secret, SecretChange::Keep)
        {
            return Err("The destination changed. Enter its token before connecting".into());
        }
        let token = match input.secret {
            SecretChange::Set(token) => Some(token),
            SecretChange::Clear => None,
            SecretChange::Keep => previous
                .as_ref()
                .and_then(|r| r.credential.as_deref())
                .map(|id| self.secrets.get(id))
                .transpose()?,
        };
        let connection = match input.destination {
            Destination::Local { home } => Connection::Local { home },
            Destination::Remote { url } => Connection::Url {
                endpoint: url,
                token,
            },
            Destination::Ssh(_) => unreachable!(),
        };
        self.probe.check(&connection)?;
        if let Some(mut active) = self.active.take() {
            active.tunnel.close();
        }
        self.active_owner = None;
        self.primary_lease = None;
        let mut visible = self.registry.clone();
        visible.selected = None;
        Ok(Outcome::Ready(visible, connection))
    }
    fn forget(&mut self, id: &str) -> Result<Outcome, String> {
        let row = self
            .registry
            .rows
            .iter()
            .find(|r| r.id == id)
            .ok_or("The connection no longer exists")?;
        let mut next = self.registry.clone();
        if let Some(credential) = &row.credential {
            next.cleanup.push(credential.clone());
        }
        next.rows.retain(|r| r.id != id);
        if next.selected.as_deref() == Some(id) {
            next.selected = None;
        }
        self.metadata.save(&next)?;
        self.registry = next;
        if self
            .active_owner
            .as_ref()
            .is_some_and(|(active, _)| active == id)
        {
            let (_, revision) = self.active_owner.take().unwrap();
            self.outbox.push_back((
                id.into(),
                revision,
                "The saved connection was forgotten".into(),
            ));
        }
        if self.active.as_ref().is_some_and(|a| a.id == id) {
            if let Some(mut active) = self.active.take() {
                active.tunnel.close();
            }
        }
        let warning = self.cleanup();
        Ok(Outcome::Registry(
            self.registry.clone(),
            Some(warning.unwrap_or_else(|| "Saved connection forgotten".into())),
        ))
    }
    fn select(&mut self, id: &str) -> Result<Outcome, String> {
        let row = self
            .registry
            .rows
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or("The selected connection no longer exists")?;
        let key = (row.id.clone(), row.revision.clone());
        if let Some(lease) = self
            .leases
            .get(&key)
            .and_then(super::leases::Entry::acquire)
        {
            return self.commit_lease(lease);
        }
        let token = row
            .credential
            .as_deref()
            .map(|id| self.secrets.get(id))
            .transpose()?;
        let mut candidate = None;
        let inner = match &row.destination {
            Destination::Local { home } => Connection::Local { home: home.clone() },
            Destination::Remote { url } => Connection::Url {
                endpoint: url.clone(),
                token,
            },
            Destination::Ssh(spec) => {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|a| a.id == id && a.revision == row.revision)
                {
                    Connection::Url {
                        endpoint: format!("http://127.0.0.1:{}{}", spec.local_port, spec.path),
                        token,
                    }
                } else {
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|a| a.port == spec.local_port)
                    {
                        let mut old = self.active.take().unwrap();
                        old.tunnel.close();
                        self.outbox.push_back((old.id,old.revision,"The previous SSH forward was closed to switch its saved port to the selected destination".into()));
                        self.active_owner = None;
                    }
                    candidate = Some(self.tunnels.open(spec)?);
                    Connection::Url {
                        endpoint: format!("http://127.0.0.1:{}{}", spec.local_port, spec.path),
                        token,
                    }
                }
            }
        };
        let connection = Connection::Saved {
            id: row.id.clone(),
            revision: row.revision.clone(),
            inner: Box::new(inner),
        };
        if let Err(error) = self.probe.check(&connection) {
            if let Some(mut tunnel) = candidate {
                tunnel.close();
            }
            return Err(error);
        }
        let mut next = self.registry.clone();
        next.selected = Some(id.into());
        if let Err(error) = self.metadata.save(&next) {
            if let Some(mut tunnel) = candidate {
                tunnel.close();
            }
            return Err(error);
        }
        self.registry = next;
        self.primary_lease = None;
        self.active_owner = Some((row.id.clone(), row.revision.clone()));
        if candidate.is_some() || !matches!(row.destination, Destination::Ssh(_)) {
            if let Some(mut old) = self.active.take() {
                old.tunnel.close();
            }
        }
        if let Some(tunnel) = candidate {
            let port = match row.destination {
                Destination::Ssh(ref spec) => spec.local_port,
                _ => 0,
            };
            self.active = Some(Active {
                id: id.into(),
                revision: row.revision,
                port,
                tunnel,
            });
        }
        Ok(Outcome::Ready(self.registry.clone(), connection))
    }
    fn cleanup(&mut self) -> Option<String> {
        let before = self.registry.cleanup.len();
        let mut failed = false;
        self.registry.cleanup.retain(|id| {
            let keep = self.secrets.delete(id).is_err();
            failed |= keep;
            keep
        });
        if self.registry.cleanup.len() != before {
            let _ = self.metadata.save(&self.registry);
        }
        failed.then(||"The connection was saved, but an obsolete token is still waiting for OS keychain cleanup".into())
    }
    pub fn poll_tunnel(&mut self) -> Option<(String, String, String)> {
        self.poll_leases();
        if let Some(event) = self.outbox.pop_front() {
            return Some(event);
        }
        let active = self.active.as_mut()?;
        let error = active.tunnel.exited()?;
        let id = active.id.clone();
        let revision = active.revision.clone();
        self.active.take();
        if self.active_owner.as_ref() == Some(&(id.clone(), revision.clone())) {
            self.active_owner = None;
        }
        Some((id, revision, error))
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        for entry in self.leases.values_mut() {
            entry.fail("The saved connection service closed");
        }
        if let Some(mut active) = self.active.take() {
            active.tunnel.close();
        }
    }
}
fn normalize_destination(
    destination: &mut Destination,
    previous: Option<&Destination>,
) -> Result<Option<String>, String> {
    match destination {
        Destination::Local { home } => {
            if !home.is_absolute() || !home.is_dir() {
                return Err("Choose an existing absolute Hermes home directory".into());
            }
            *home = fs::canonicalize(&*home)
                .map_err(|_| "The local Hermes home could not be resolved")?;
            Ok(None)
        }
        Destination::Remote { url } => {
            let raw = if url.contains("://") {
                url.clone()
            } else {
                format!("http://{url}")
            };
            let (base, token) = crate::gateway::endpoint_url(&raw)?;
            *url = base.to_string();
            Ok(token)
        }
        Destination::Ssh(spec) => {
            super::ssh::validate(spec)?;
            if let Some(Destination::Ssh(old)) = previous {
                spec.local_port = old.local_port;
            }
            if spec.local_port == 0 {
                let listener = TcpListener::bind(("127.0.0.1", 0))
                    .map_err(|_| "No local port is available for the SSH tunnel")?;
                spec.local_port = listener
                    .local_addr()
                    .map_err(|_| "The SSH local port could not be read")?
                    .port();
            }
            if let Some(path) = &spec.key_file {
                if !path.is_absolute() || !path.is_file() {
                    return Err("Choose an existing absolute SSH private-key file, or leave it blank to use your SSH agent/config".into());
                }
            }
            if !spec.path.is_empty() {
                if !spec.path.starts_with('/')
                    || spec.path.contains(['?', '#'])
                    || spec.path.split('/').any(|p| p == "..")
                {
                    return Err("The gateway path prefix must be an absolute URL path".into());
                }
                spec.path = spec.path.trim_end_matches('/').into();
            }
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;

pub(super) fn read_registry(path: PathBuf) -> Result<Registry, String> {
    JsonMetadata(path).load()
}
