//! Explicit same-socket launch-home evidence, separate from UI profile labels.
use super::*;

#[derive(Clone)]
pub(crate) struct BackendRootProof {
    /// A name exists only when exactly one raw roster path identifies this home.
    pub profile: Option<String>,
    pub profile_id: String,
    pub instance_id: String,
    pub authority_epoch: u64,
}
impl BackendRootProof {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !absolute_remote_home(&self.profile_id) {
            return Err("The backend did not prove an absolute running profile home".into());
        }
        valid_scope(&self.instance_id)?;
        if let Some(profile) = &self.profile {
            valid_scope(profile)?;
        }
        Ok(())
    }
}

impl Gateway {
    /// Called only by an explicit source resolution. Passive inventory never
    /// asks for runtime.describe, which may maintain backend configuration.
    pub(crate) fn backend_root_proof(&mut self) -> Result<BackendRootProof, String> {
        let homes = self
            .raw_backend_homes
            .as_ref()
            .ok_or("Read the backend's raw profile inventory before requesting root evidence")?
            .clone();
        let value = self.rpc_until("runtime.describe", json!({}), Instant::now() + TIMEOUT)?;
        let profile_id = required_str(&value, "profile_id")?;
        if !absolute_remote_home(profile_id) {
            return Err("The backend did not prove an absolute running profile home".into());
        }
        let instance_id = required_str(&value, "instance_id")?;
        valid_scope(instance_id)?;
        let authority_epoch = value
            .get("authority_epoch")
            .and_then(Value::as_u64)
            .ok_or("The backend did not prove its runtime authority epoch")?;
        let mut matched = homes.iter().filter(|(_, path)| path == profile_id);
        let profile = match (matched.next(), matched.next()) {
            (Some((name, _)), None) => Some(name.clone()),
            _ => None,
        };
        Ok(BackendRootProof {
            profile,
            profile_id: profile_id.into(),
            instance_id: instance_id.into(),
            authority_epoch,
        })
    }
}

fn absolute_remote_home(value: &str) -> bool {
    if value.len() > 16 * 1024 || value.chars().any(char::is_control) {
        return false;
    }
    // The remote host's syntax is relevant; never canonicalize a remote home
    // through this machine's filesystem or derive a name from its basename.
    let bytes = value.as_bytes();
    value.starts_with('/')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
        || value.strip_prefix("\\\\").is_some_and(|unc| {
            let mut parts = unc.split('\\');
            parts.next().is_some_and(|part| !part.is_empty())
                && parts.next().is_some_and(|part| !part.is_empty())
        })
}
