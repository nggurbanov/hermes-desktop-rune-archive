use super::*;
use crate::connections::{Connections, Resolver, Work};
use std::sync::mpsc::{self, Receiver};
#[derive(Debug)]
pub(crate) struct SaveReceipt {
    pub scope: Scope,
    pub version: ScopeVersion,
    pub snapshot: Snapshot,
    pub warning: Option<String>,
}
/// An accepted explicit write keeps its actor through the final receipt.
/// Dropping the UI does not turn an accepted write into a cancelled read.
pub(crate) struct SaveTicket {
    pub receiver: Receiver<Result<SaveReceipt, String>>,
    _resolver: Resolver,
}
impl Resolver {
    pub(crate) fn legacy_snapshot(&self) -> Result<Receiver<Result<Snapshot, String>>, String> {
        let (reply, receiver) = mpsc::channel();
        self.send
            .send(Work::LegacySnapshot { reply })
            .map_err(|_| "The Legacy settings service is unavailable")?;
        Ok(receiver)
    }
    pub(crate) fn save_legacy(&self, edit: Edit) -> Result<SaveTicket, String> {
        let (reply, receiver) = mpsc::channel();
        self.send
            .send(Work::SaveLegacy { edit, reply })
            .map_err(|_| "The Legacy settings service is unavailable")?;
        Ok(SaveTicket {
            receiver,
            _resolver: self.clone(),
        })
    }
}
impl Connections {
    pub(crate) fn legacy_snapshot(&self) -> Result<Receiver<Result<Snapshot, String>>, String> {
        if self.synthetic {
            return Err("Legacy settings are unavailable in synthetic scenes".into());
        }
        self.resolver()
            .ok_or("The connection worker is unavailable")?
            .legacy_snapshot()
    }
    pub(crate) fn save_legacy(&self, edit: Edit) -> Result<SaveTicket, String> {
        if self.synthetic {
            return Err("Legacy settings are unavailable in synthetic scenes".into());
        }
        self.resolver()
            .ok_or("The connection worker is unavailable")?
            .save_legacy(edit)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedAuth {
    version: u32,
    token: Option<String>,
    headers: BTreeMap<String, String>,
}
impl AuthBundle {
    pub(in crate::connections) fn empty() -> Self {
        Self {
            token: None,
            headers: BTreeMap::new(),
        }
    }
    pub(in crate::connections) fn decode(encoded: &str) -> Result<Self, String> {
        if encoded.len() > 1024 * 1024 {
            return Err("The Legacy credential bundle is too large".into());
        }
        let raw: EncodedAuth =
            serde_json::from_str(encoded).map_err(|_| "The Legacy credential bundle is invalid")?;
        if raw.version != 1 {
            return Err("The Legacy credential bundle requires a newer Hermes app".into());
        }
        let mut bundle = Self {
            token: raw.token,
            headers: raw.headers,
        };
        bundle.normalize()?;
        Ok(bundle)
    }
    pub(in crate::connections) fn encode(&self) -> Result<String, String> {
        serde_json::to_string(&EncodedAuth {
            version: 1,
            token: self.token.clone(),
            headers: self.headers.clone(),
        })
        .map_err(|_| "The Legacy credential bundle could not be encoded".into())
    }
    pub(in crate::connections) fn normalize(&mut self) -> Result<(), String> {
        if self
            .token
            .as_ref()
            .is_some_and(|t| t.len() > 16384 || t.chars().any(char::is_control))
        {
            return Err("The Legacy gateway token is invalid".into());
        }
        if self.headers.len() > 64 || self.headers.values().any(|v| v.len() > 16384) {
            return Err("The Legacy access headers exceed the size limit".into());
        }
        self.headers =
            crate::gateway::prepared::normalize_access_headers(std::mem::take(&mut self.headers))?;
        Ok(())
    }
    pub(in crate::connections) fn is_empty(&self) -> bool {
        self.token.is_none() && self.headers.is_empty()
    }
}
impl AuthEdit {
    pub(in crate::connections) fn unchanged(&self) -> bool {
        matches!(self.token, SecretEdit::Keep)
            && self.headers.values().all(|v| matches!(v, SecretEdit::Keep))
    }
}
