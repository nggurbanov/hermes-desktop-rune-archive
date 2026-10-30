//! Passive roster snapshots never resolve a forward or activate a source.
use super::*;
pub(super) mod credentials;
use crate::source::{Binding, SourceVersion};
use std::time::Instant;

#[derive(Clone)]
pub(crate) enum PassiveSource {
    Attached(Binding),
    Http { connection: Connection },
    SshInventory(Ssh),
    DeferredLocal,
}

pub(super) enum Plan {
    Attached(Binding),
    Saved(Saved),
    DeferredLocal,
}

impl Resolver {
    /// Called on the roster worker. Only registry/lease inspection runs on the
    /// connection actor; keychain and network work never occupy that actor.
    pub(crate) fn passive_source(
        &self,
        version: &SourceVersion,
        deadline: Instant,
    ) -> Result<PassiveSource, String> {
        let (reply, receive) = mpsc::channel();
        self.send
            .send(Work::Passive {
                version: version.clone(),
                reply,
            })
            .map_err(|_| "The saved connection service is unavailable")?;
        let plan = receive
            .recv_timeout(remaining(deadline)?)
            .map_err(|_| "The saved source snapshot timed out")??;
        let source = core::passive::materialize(plan, deadline)?;
        remaining(deadline)?;
        if let PassiveSource::Http { connection } = source {
            let (reply, receive) = mpsc::channel();
            self.send
                .send(Work::RetainPassive {
                    version: version.clone(),
                    connection,
                    reply,
                })
                .map_err(|_| "The passive source could not be retained")?;
            let lease = receive
                .recv_timeout(remaining(deadline)?)
                .map_err(|_| "The passive lease timed out")??;
            return Binding::new(lease.connection().clone(), Some(lease))
                .map(|binding| PassiveSource::Attached(binding.retaining_resolver(self.clone())));
        }
        Ok(match source {
            PassiveSource::Attached(binding) => {
                PassiveSource::Attached(binding.retaining_resolver(self.clone()))
            }
            other => other,
        })
    }

    pub(crate) fn validate_passive(
        &self,
        version: &SourceVersion,
        deadline: Instant,
    ) -> Result<(), String> {
        let (reply, receive) = mpsc::channel();
        self.send
            .send(Work::ValidatePassive {
                version: version.clone(),
                reply,
            })
            .map_err(|_| "The saved connection service is unavailable")?;
        receive
            .recv_timeout(remaining(deadline)?)
            .map_err(|_| "The saved source validation timed out")?
    }
}

fn remaining(deadline: Instant) -> Result<std::time::Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or("The passive source deadline expired".into())
}
