use super::*;
use crate::{
    connections::passive::PassiveSource,
    gateway::{Connection, roster},
    source::SourceStamp,
};
use std::sync::atomic::AtomicBool;
#[derive(Clone)]
pub(super) struct Job {
    pub generation: u64,
    pub descriptor: Descriptor,
    pub binding: Option<Binding>,
    pub resolver: Option<Resolver>,
    pub identity: bool,
    pub inventory: bool,
    pub cancelled: Arc<AtomicBool>,
}
pub(super) enum Outcome {
    Gateway {
        binding: Option<Binding>,
        stamp: SourceStamp,
        read: roster::Read,
    },
    Inventory(Result<inventory::ssh::Inventory, String>),
    Deferred,
    Failed(String),
}
pub(super) trait Runner: Send + Sync {
    fn run(&self, job: &Job) -> Outcome;
}
pub(super) struct System {
    pub children: Arc<inventory::ssh::Children>,
}
impl Runner for System {
    fn run(&self, job: &Job) -> Outcome {
        match run(job, &self.children) {
            Ok(result) => result,
            Err(error) => Outcome::Failed(error),
        }
    }
}
fn run(job: &Job, children: &Arc<inventory::ssh::Children>) -> Result<Outcome, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let read_deadline = deadline - Duration::from_millis(200);
    let source = if let Some(binding) = &job.binding {
        binding.validate()?;
        PassiveSource::Attached(binding.clone())
    } else {
        job.resolver
            .as_ref()
            .ok_or("The passive connection service is unavailable")?
            .passive_source(&job.descriptor.version, deadline)?
    };
    let result = match source {
        PassiveSource::Attached(binding) => {
            binding.validate()?;
            let read = roster::read(
                binding.connection(),
                read_deadline,
                job.identity,
                job.cancelled.clone(),
            )?;
            binding.validate()?;
            Outcome::Gateway {
                stamp: binding.stamp().clone(),
                binding: Some(binding),
                read,
            }
        }
        PassiveSource::Http { connection } => {
            let read = roster::read(
                &connection,
                read_deadline,
                job.identity,
                job.cancelled.clone(),
            )?;
            Outcome::Gateway {
                stamp: SourceStamp::from_connection(&connection)?,
                binding: None,
                read,
            }
        }
        PassiveSource::SshInventory(spec) => {
            if job.inventory {
                Outcome::Inventory(inventory::ssh::read(
                    &spec,
                    read_deadline,
                    job.cancelled.clone(),
                    children,
                ))
            } else {
                Outcome::Deferred
            }
        }
        PassiveSource::DeferredLocal => Outcome::Deferred,
    };
    if let SourceId::Saved(_) = job.descriptor.version.id {
        job.resolver
            .as_ref()
            .ok_or("The passive connection service is unavailable")?
            .validate_passive(&job.descriptor.version, deadline)?;
    }
    if job.cancelled.load(Ordering::Acquire) {
        return Err("The fleet read was superseded".into());
    }
    Ok(result)
}
