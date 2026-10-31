use super::*;
mod leases;
pub(super) mod legacy;
pub(super) mod passive;
mod registry;
mod startup;
use std::{fs, net::TcpListener};

pub(super) trait Secrets: Send {
    fn get_passive(&mut self, _id: &str, _deadline: std::time::Instant) -> Result<String, String> {
        Err("Passive saved credentials are unavailable".into())
    }
    fn get(&mut self, id: &str) -> Result<String, String>;
    fn get_optional(&mut self, id: &str) -> Result<Option<String>, String> {
        self.get(id).map(Some)
    }
    fn set(&mut self, id: &str, value: &str) -> Result<(), String>;
    fn delete(&mut self, id: &str) -> Result<(), String>;
}
pub(super) trait Metadata: Send {
    fn transaction(&mut self) -> Result<Option<crate::client::FileLock>, String>;
    fn load(&mut self) -> Result<Registry, String>;
    fn save(&mut self, registry: &Registry) -> Result<(), String>;
    fn storage_path(&self) -> Option<&std::path::Path> {
        None
    }
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
    fn get_passive(&mut self, id: &str, deadline: std::time::Instant) -> Result<String, String> {
        super::passive::credentials::read(id, deadline)
    }
    fn get(&mut self, id: &str) -> Result<String, String> {
        keyring::Entry::new("Hermes Rune",id).map_err(|_|"The OS credential store could not be opened")?.get_password().map_err(|_|"The saved gateway credential is missing or the OS credential store is locked. Edit the connection to enter it again".into())
    }
    fn get_optional(&mut self, id: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new("Hermes Rune", id)
            .map_err(|_| "The OS credential store could not be opened")?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("The OS credential store is unavailable or locked".into()),
        }
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
    fn storage_path(&self) -> Option<&std::path::Path> {
        Some(&self.0)
    }
    fn transaction(&mut self) -> Result<Option<crate::client::FileLock>, String> {
        registry::transaction(&self.0).map(Some)
    }
    fn load(&mut self) -> Result<Registry, String> {
        match fs::read(&self.0) {
            Ok(bytes) => {
                if bytes.len() > 1024 * 1024 {
                    return Err("The saved connection registry is too large".into());
                }
                registry::decode(&bytes)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::empty()),
            Err(_) => Err("The saved connection registry could not be read".into()),
        }
    }
    fn save(&mut self, registry: &Registry) -> Result<(), String> {
        crate::client::private_write(&self.0, &registry::encode(registry)?)
    }
}
struct GatewayProbe;
impl Probe for GatewayProbe {
    fn check(&mut self, connection: &Connection) -> Result<Vec<crate::gateway::Profile>, String> {
        let mut gateway = crate::gateway::Gateway::connect(connection)?;
        gateway.profiles()
    }
}
pub(super) struct Core {
    registry: Registry,
    secrets: Box<dyn Secrets>,
    metadata: Box<dyn Metadata>,
    probe: Box<dyn Probe>,
    tunnels: Box<dyn Tunnels>,
    active_owner: Option<(String, String)>,
    outbox: std::collections::VecDeque<(String, String, String)>,
    leases: std::collections::BTreeMap<(String, String), super::leases::Entry>,
    primary_lease: Option<Lease>,
    route_leases: std::collections::BTreeMap<(String, String), legacy::RouteEntry>,
    ssh_pool: super::installed_ssh::Pool,
    ssh_binary: PathBuf,
}
impl Core {
    pub fn system(path: PathBuf) -> Self {
        Self {
            registry: Registry::empty(),
            secrets: Box::new(SystemSecrets),
            metadata: Box::new(JsonMetadata(path)),
            probe: Box::new(GatewayProbe),
            tunnels: Box::new(super::ssh::OpenSsh),
            active_owner: None,
            outbox: std::collections::VecDeque::new(),
            leases: std::collections::BTreeMap::new(),
            primary_lease: None,
            route_leases: std::collections::BTreeMap::new(),
            ssh_pool: super::installed_ssh::Pool::default(),
            ssh_binary: PathBuf::from("ssh"),
        }
    }
    pub fn execute(&mut self, command: Command) -> Result<Outcome, String> {
        if let Command::CommitRoute(_, _, alive) = &command {
            if !alive.load(std::sync::atomic::Ordering::Acquire) {
                return Err("The route activation was superseded".into());
            }
        }
        let _transaction = if matches!(&command, Command::ConnectOnce(_)) {
            None
        } else {
            self.metadata.transaction()?
        };
        if !matches!(
            &command,
            Command::ConnectOnce(_) | Command::CommitLease(_, _)
        ) {
            self.refresh_registry()?;
        }
        match command {
            Command::Load => {
                let warning = self.cleanup();
                Ok(Outcome::Registry(self.visible_registry(), warning))
            }
            Command::Save(input) => self.save(input),
            Command::ConnectOnce(input) => self.connect_once(input),
            Command::Forget(id) => self.forget(&id),
            Command::Select(id) => self.select(&id),
            Command::CommitLease(lease, profile) => {
                self.commit_lease_for(lease, profile.as_deref())
            }
            Command::CommitRoute(lease, profile, alive) => {
                self.commit_route(lease, &profile, &alive)
            }
            Command::MakePrimary(id) => self.make_primary(&id),
            Command::LaunchMode(mode) => self.launch_mode(mode),
            Command::RememberProfile {
                id,
                revision,
                profile,
            } => self.remember_profile(&id, &revision, &profile),
            Command::ProfileChanged {
                id,
                revision,
                old,
                new,
            } => self.profile_changed(&id, &revision, &old, new.as_deref()),
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
        if let Some(previous) = &previous {
            match input.expected_revision.as_deref() {
                Some(revision) if revision == previous.revision => {}
                Some(_) => {
                    return Err(
                        "This saved connection changed in another window. Reopen it before saving"
                            .into(),
                    );
                }
                None => {
                    return Err("Reopen this saved connection before saving".into());
                }
            }
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
        let destination_other = previous
            .as_ref()
            .filter(|row| {
                std::mem::discriminant(&row.destination)
                    == std::mem::discriminant(&input.destination)
            })
            .map(|row| row.destination_other.clone())
            .unwrap_or_default();
        let saved = Saved {
            id: id.clone(),
            revision,
            label: input.label,
            destination: input.destination,
            credential,
            other: previous
                .as_ref()
                .map(|row| row.other.clone())
                .unwrap_or_default(),
            destination_other,
        };
        let was_empty = self.registry.rows.is_empty();
        let mut next = self.registry.clone();
        if let Some(index) = next.rows.iter().position(|r| r.id == id) {
            next.rows[index] = saved;
        } else {
            next.rows.push(saved);
        }
        if next.primary.is_none() {
            next.primary = next
                .primary_fallback()
                .or_else(|| was_empty.then(|| id.clone()));
        }
        if next.last_used.is_none() {
            next.last_used = next.primary.clone();
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
            self.visible_registry(),
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
        self.active_owner = None;
        self.primary_lease = None;
        let mut visible = self.registry.clone();
        visible.selected = None;
        Ok(Outcome::Ready(visible, connection, None))
    }
    fn forget(&mut self, id: &str) -> Result<Outcome, String> {
        let Some(row) = self.registry.rows.iter().find(|r| r.id == id) else {
            return Ok(Outcome::Forgot(self.visible_registry(), id.into(), None));
        };
        if row.kind() == Kind::Local {
            return Err("The local connection cannot be removed".into());
        }
        let mut next = self.registry.clone();
        if let Some(credential) = &row.credential {
            next.cleanup.push(credential.clone());
        }
        next.rows.retain(|r| r.id != id);
        next.last_profiles.remove(id);
        if next.primary.as_deref() == Some(id) {
            next.primary = next.primary_fallback();
        }
        if next.last_used.as_deref() == Some(id) {
            next.last_used = next.primary.clone();
        }
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
        let warning = self.cleanup();
        Ok(Outcome::Forgot(
            self.visible_registry(),
            id.into(),
            Some(warning.unwrap_or_else(|| "Saved connection forgotten".into())),
        ))
    }
    fn select(&mut self, id: &str) -> Result<Outcome, String> {
        self.poll_leases();
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
        let profile = self.target_profile(id, None)?;
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
                candidate = Some(self.tunnels.open(spec)?);
                Connection::Url {
                    endpoint: format!("http://127.0.0.1:{}{}", spec.local_port, spec.path),
                    token,
                }
            }
        };
        let connection = Connection::Saved {
            id: row.id.clone(),
            revision: row.revision.clone(),
            inner: Box::new(inner),
        };
        let profiles = match self.probe.check(&connection) {
            Ok(profiles) if profiles.iter().any(|row| row.name == profile) => profiles,
            Ok(_) => {
                if let Some(mut tunnel) = candidate {
                    tunnel.close();
                }
                return Err("The selected source no longer serves that agent profile".into());
            }
            Err(error) => {
                if let Some(mut tunnel) = candidate {
                    tunnel.close();
                }
                return Err(error);
            }
        };
        let mut next = self.registry.clone();
        next.selected = Some(id.into());
        Self::remember_activation(&mut next, id, &profile);
        if let Err(error) = self.metadata.save(&next) {
            if let Some(mut tunnel) = candidate {
                tunnel.close();
            }
            return Err(error);
        }
        self.registry = next;
        let tunnel = candidate;
        let (entry, lease) = super::leases::Entry::new(
            row.id.clone(),
            row.revision.clone(),
            connection.clone(),
            profiles,
            tunnel,
        );
        self.leases
            .insert((row.id.clone(), row.revision.clone()), entry);
        self.primary_lease = Some(lease.clone());
        self.active_owner = Some((row.id, row.revision));
        self.poll_leases();
        Ok(Outcome::Ready(
            self.visible_registry(),
            connection,
            Some(lease),
        ))
    }
    fn cleanup(&mut self) -> Option<String> {
        let reachable = match self.registry.credential_refs() {
            Ok(refs) => refs,
            Err(error) => return Some(error),
        };
        let before = self.registry.cleanup.len();
        let mut failed = false;
        self.registry.cleanup.retain(|id| {
            if reachable.contains(id) {
                return false;
            }
            let keep = self.secrets.delete(id).is_err();
            failed |= keep;
            keep
        });
        let persistence_failed =
            self.registry.cleanup.len() != before && self.metadata.save(&self.registry).is_err();
        match (failed, persistence_failed) {
            (true, _) => Some("The connection was saved, but an obsolete credential is still waiting for OS keychain cleanup".into()),
            (false, true) => Some("Credential cleanup progress could not be saved; its durable ledger will be retried".into()),
            _ => None,
        }
    }
    pub fn poll_tunnel(&mut self) -> Option<(String, String, String)> {
        self.poll_leases();
        self.outbox.pop_front()
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        self.retire_route_sources("The resolved connection service closed");
        for entry in self.leases.values_mut() {
            entry.fail("The saved connection service closed");
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
