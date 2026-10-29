use super::*;
impl Worker {
    pub(super) fn prepare_rewind(
        &mut self,
        entry: &mut Prepared,
        journal: &Journal,
        snapshot: Snapshot,
    ) -> Result<Snapshot, CallError> {
        let Some(rewrite) = entry.input.rewind.clone() else {
            return Ok(snapshot);
        };
        if !snapshot.canonical {
            return Err("This gateway cannot safely rewind a durable message. Its revision-fenced session mutation API is required".into());
        }
        let index = if let Some(i) = entry.mutations.iter().position(|m| m.operation == "rewind") {
            i
        } else {
            entry.mutations.push(Mutation{receipt:None,operation:"rewind".into(),complete:false,confirmation:None,refused:false,params:json!({"session_id":snapshot.runtime,"request_id":uuid::Uuid::new_v4().to_string(),"expected_revision":rewrite.revision,"expected_generation":rewrite.generation,"operation":"rewind","payload":{"target_message_id":rewrite.target}})});
            journal.save(entry)?;
            entry.mutations.len() - 1
        };
        if entry.mutations[index].complete {
            return Ok(snapshot);
        }
        if entry.mutations[index].refused {
            return Err("The conversation changed before the rewind. Discard this saved action, refresh history and choose its target again".into());
        }
        if snapshot.running {
            return Err("Stop the current turn before editing its history".into());
        }
        let result = self
            .transport()?
            .call("session.mutate", entry.mutations[index].params.clone());
        let value = match result {
            Ok(v) => v,
            Err(error) => {
                if !error.uncertain {
                    entry.mutations[index].refused = true;
                    journal.save(entry)?;
                }
                return Err(error);
            }
        };
        if value["session_id"].as_str() != Some(snapshot.runtime.as_str())
            || value["operation"] != "rewind"
            || !value["revision"].is_u64()
        {
            return Err(CallError{message:"The rewind receipt has no matching session, operation and revision; its outcome is unknown".into(),code:None,data:None,uncertain:true});
        }
        entry.mutations[index].complete = true;
        journal.save(entry)?;
        let readback = self.transport()?.resume(&snapshot.stored)?;
        self.attached(readback.clone())?;
        Ok(readback)
    }
}
