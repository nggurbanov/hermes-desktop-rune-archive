use super::super::passive::{PassiveSource, Plan};
use super::*;
use crate::source::{Binding, SourceId, SourceVersion};

impl Core {
    pub(in crate::connections) fn retain_passive(
        &mut self,
        version: &SourceVersion,
        connection: Connection,
    ) -> Result<super::super::Lease, String> {
        let (row, _) = self.passive_row(version)?;
        if matches!(row.destination, Destination::Ssh(_)) {
            return Err("Passive inventory cannot create an SSH forward".into());
        }
        let crate::gateway::Connection::Saved { id, revision, .. } = &connection else {
            return Err("A passive retained source must be saved".into());
        };
        if id != &row.id || revision != &row.revision {
            return Err("The passive source owner changed".into());
        }
        let key = (row.id.clone(), row.revision.clone());
        if let Some(lease) = self
            .leases
            .get(&key)
            .and_then(super::super::leases::Entry::acquire)
        {
            return Ok(lease);
        }
        let (entry, lease) = super::super::leases::Entry::passive(row.id, row.revision, connection);
        self.leases.insert(key, entry);
        Ok(lease)
    }
    pub(in crate::connections) fn validate_passive(
        &mut self,
        version: &SourceVersion,
    ) -> Result<(), String> {
        self.passive_row(version).map(|_| ())
    }
    fn passive_row(&mut self, version: &SourceVersion) -> Result<(Saved, Registry), String> {
        let SourceId::Saved(id) = &version.id else {
            return Err("Only a saved source can be read through the registry".into());
        };
        self.refresh_registry()?;
        let registry = self.registry.clone();
        let row = registry
            .rows
            .iter()
            .find(|row| row.id == *id && row.revision == version.revision)
            .cloned()
            .ok_or("The saved source changed or was removed")?;
        Ok((row, registry))
    }
    pub(in crate::connections) fn passive_plan(
        &mut self,
        version: &SourceVersion,
    ) -> Result<Plan, String> {
        let (row, registry) = self.passive_row(version)?;
        if let Some(lease) = self
            .leases
            .get(&(row.id.clone(), row.revision.clone()))
            .and_then(super::super::leases::Entry::acquire)
        {
            return Binding::new(lease.connection().clone(), Some(lease)).map(Plan::Attached);
        }
        if matches!(row.destination, Destination::Local { .. })
            && registry.primary.as_deref() != Some(row.id.as_str())
        {
            return Ok(Plan::DeferredLocal);
        }
        Ok(Plan::Saved(row))
    }
}

pub(in crate::connections) fn materialize(
    plan: Plan,
    deadline: std::time::Instant,
) -> Result<PassiveSource, String> {
    materialize_with_deadline(plan, &mut SystemSecrets, deadline)
}
fn materialize_with_deadline(
    plan: Plan,
    secrets: &mut dyn Secrets,
    deadline: std::time::Instant,
) -> Result<PassiveSource, String> {
    match plan {
        Plan::Attached(binding) => {
            binding.validate()?;
            Ok(PassiveSource::Attached(binding))
        }
        Plan::DeferredLocal => Ok(PassiveSource::DeferredLocal),
        Plan::Saved(row) => {
            if let Destination::Ssh(spec) = row.destination {
                return Ok(PassiveSource::SshInventory(spec));
            }
            let inner = match row.destination {
                Destination::Local { home } => Connection::Local { home },
                Destination::Remote { url } => {
                    let token = row
                        .credential
                        .as_deref()
                        .map(|key| secrets.get_passive(key, deadline))
                        .transpose()?;
                    Connection::Url {
                        endpoint: url,
                        token,
                    }
                }
                Destination::Ssh(_) => unreachable!(),
            };
            Ok(PassiveSource::Http {
                connection: Connection::Saved {
                    id: row.id,
                    revision: row.revision,
                    inner: Box::new(inner),
                },
            })
        }
    }
}

#[cfg(test)]
mod tests;
