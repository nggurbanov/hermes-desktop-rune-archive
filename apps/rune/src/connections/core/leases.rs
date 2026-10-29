//! Resolution and activation share the existing keychain, probe and SSH services.
use super::super::leases::{Entry, Lease};
use super::*;
impl Core {
    pub(in crate::connections) fn resolve_lease(
        &mut self,
        id: &str,
        expected_revision: Option<&str>,
    ) -> Result<Lease, String> {
        self.registry = self.metadata.load()?;
        self.poll_leases();
        let row = self
            .registry
            .rows
            .iter()
            .find(|row| row.id == id)
            .cloned()
            .ok_or("The saved source was removed")?;
        if expected_revision.is_some_and(|revision| revision != row.revision) {
            return Err("The saved source changed; refresh its route before connecting".into());
        }
        let key = (row.id.clone(), row.revision.clone());
        if let Some(lease) = self.leases.get(&key).and_then(Entry::acquire) {
            return Ok(lease);
        }
        self.leases.remove(&key);
        let token = row
            .credential
            .as_deref()
            .map(|id| self.secrets.get(id))
            .transpose()?;
        let promote = self
            .active
            .as_ref()
            .is_some_and(|active| active.id == row.id && active.revision == row.revision);
        let mut tunnel = None;
        let inner = match &row.destination {
            Destination::Local { home } => Connection::Local { home: home.clone() },
            Destination::Remote { url } => Connection::Url {
                endpoint: url.clone(),
                token,
            },
            Destination::Ssh(original) => {
                let mut spec = original.clone();
                if !promote {
                    spec.local_port = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                        .map_err(|_| "A local port for the retained SSH source is unavailable")?
                        .local_addr()
                        .map_err(|_| "The retained SSH port is unavailable")?
                        .port();
                    tunnel = Some(self.tunnels.open(&spec)?);
                }
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
            Ok(profiles) => profiles,
            Err(error) => {
                if let Some(mut tunnel) = tunnel {
                    tunnel.close();
                }
                return Err(error);
            }
        };
        if promote {
            tunnel = self.active.take().map(|active| active.tunnel);
        }
        let foreground = self.active_owner.as_ref() == Some(&key);
        let (entry, lease) = Entry::new(row.id, row.revision, connection, profiles, tunnel);
        if foreground {
            self.primary_lease = Some(lease.clone());
        }
        self.leases.insert(key, entry);
        Ok(lease)
    }
    pub(super) fn commit_lease(&mut self, lease: Lease) -> Result<Outcome, String> {
        self.commit_lease_for(lease, None)
    }
    pub(super) fn commit_lease_for(
        &mut self,
        lease: Lease,
        target_profile: Option<&str>,
    ) -> Result<Outcome, String> {
        if !lease.is_valid() {
            return Err(lease
                .error()
                .unwrap_or("The saved-source lease expired".into()));
        }
        let mut next = self.metadata.load()?;
        if !next
            .rows
            .iter()
            .any(|row| row.id == lease.source_id() && row.revision == lease.revision())
        {
            return Err("The saved source changed while activating it".into());
        }
        let profiles = self.probe.check(lease.connection())?;
        if target_profile.is_some_and(|profile| !profiles.iter().any(|row| row.name == profile)) {
            return Err("The selected source no longer serves that agent profile".into());
        }
        next.selected = Some(lease.source_id().into());
        self.metadata.save(&next)?;
        self.registry = next;
        self.active_owner = Some((lease.source_id().into(), lease.revision().into()));
        self.primary_lease = Some(lease.clone());
        if let Some(mut active) = self.active.take() {
            active.tunnel.close();
        }
        Ok(Outcome::Ready(
            self.registry.clone(),
            lease.connection().clone(),
        ))
    }
    pub(super) fn poll_leases(&mut self) {
        let mut retire = Vec::new();
        for (key, entry) in &mut self.leases {
            if !entry.referenced() {
                retire.push(key.clone());
                continue;
            }
            let error = if !self
                .registry
                .rows
                .iter()
                .any(|row| row.id == key.0 && row.revision == key.1)
            {
                Some("The saved source changed or was removed".into())
            } else {
                entry.exited()
            };
            if let Some(error) = error {
                entry.fail(&error);
                retire.push(key.clone());
                if self
                    .primary_lease
                    .as_ref()
                    .is_some_and(|lease| lease.source_id() == key.0 && lease.revision() == key.1)
                {
                    self.primary_lease = None;
                    if self.active_owner.as_ref() == Some(key) {
                        self.active_owner = None;
                        self.outbox.push_back((key.0.clone(), key.1.clone(), error));
                    }
                }
            }
        }
        for key in retire {
            self.leases.remove(&key);
        }
    }
}
