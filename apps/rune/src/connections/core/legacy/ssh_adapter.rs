//! Installed SSH resources are retained by the route lease, not by the UI.
use super::*;
use crate::connections::{
    installed_ssh,
    legacy::{Plan, ResolveRequest},
};
use std::sync::{Arc, atomic::AtomicBool};

impl Tunnel for installed_ssh::Resource {
    fn exited(&mut self) -> Option<String> {
        installed_ssh::Resource::exited(self)
    }
    fn close(&mut self) {
        if let Err(error) = installed_ssh::Resource::close(self) {
            eprintln!("Installed SSH cleanup could not be proved: {error}");
        }
    }
}
impl Core {
    pub(super) fn open_installed_ssh(
        &self,
        ssh: &domain::InstalledSsh,
        request: &ResolveRequest,
        plan: &Plan,
        alive: &Arc<AtomicBool>,
    ) -> Result<installed_ssh::Ready, String> {
        let path = self
            .metadata
            .storage_path()
            .ok_or("Installed SSH needs persistent client state")?
            .to_path_buf();
        let installation = super::installation::load_or_create(&path)?;
        let owner = installed_ssh::ownership_id(&installation, request.desktop_profile.trim())?;
        let configured = ssh
            .remote_profile
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let profile = configured.unwrap_or(request.desktop_profile.trim());
        let profile = (!profile.is_empty() && profile != "default").then(|| profile.to_owned());
        let spec = installed_ssh::Spec {
            host: ssh.host.trim().into(),
            user: ssh.user.trim().into(),
            port: ssh.port,
            key_path: ssh.key_path.as_ref().map(PathBuf::from),
            remote_hermes_path: ssh.remote_hermes_path.clone(),
            remote_profile: profile,
        };
        let client = installed_ssh::Ssh::new(self.ssh_binary.clone(), spec)?;
        let version = plan.version.clone();
        let context = request.context.clone();
        let door = request.door.clone();
        let validate_path = path.clone();
        let validate = Arc::new(move || {
            let guard = super::super::registry::transaction(&validate_path)?;
            let registry = JsonMetadata(validate_path.clone()).load()?;
            context_for_validation(&mut context.clone(), &registry, &version, &door)?;
            Ok(guard)
        });
        let root = path
            .parent()
            .ok_or("Invalid client state path")?
            .join("ssh-generations");
        let store =
            super::ssh_store::SshCredentialStore::new(root, Box::new(SystemSecrets), validate);
        self.ssh_pool.connect_with_store(
            client,
            &owner,
            Arc::new(store),
            &installed_ssh::Cancellation::from_live(alive.clone()),
        )
    }
}
fn context_for_validation(
    context: &mut crate::connections::legacy::SelectionContext,
    registry: &Registry,
    version: &crate::connections::legacy::ResolutionVersion,
    door: &crate::source::RouteDoor,
) -> Result<(), String> {
    context.primary = registry
        .primary
        .as_ref()
        .and_then(|id| registry.rows.iter().find(|row| &row.id == id))
        .cloned();
    if let crate::source::RouteDoor::Saved { id, revision } = door {
        if !registry
            .rows
            .iter()
            .any(|row| row.id == *id && row.revision == *revision)
        {
            return Err("The requested source changed while connecting".into());
        }
    }
    if !registry
        .legacy
        .clone()
        .unwrap_or_default()
        .validates_resolution(version, context)
    {
        return Err("The selected SSH route changed while connecting".into());
    }
    Ok(())
}
