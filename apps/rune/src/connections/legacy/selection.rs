use super::*;
use crate::connections::{Destination, Kind, Saved};
// Captured by the process/bootstrap caller; never reads mutable process env.
#[derive(Clone)]
pub(crate) struct Environment {
    pub url: Option<String>,
    pub token: Option<String>,
    pub generation: String,
}
impl Environment {
    /// Capture supplied values only. The namespace is stable across restarts;
    /// a random process epoch would strand otherwise-identical Legacy drafts.
    pub(crate) fn captured(url: Option<String>, token: Option<String>) -> Self {
        use sha2::{Digest, Sha256};
        let url = url.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let token = token.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let canonical_url = url.as_deref().map(|s| {
            crate::gateway::endpoint_url(s)
                .map(|(u, _)| u.to_string())
                .unwrap_or_else(|_| s.into())
        });
        let effective_token = url.as_ref().and(token.as_ref());
        let identity = serde_json::to_vec(&(canonical_url, effective_token))
            .expect("captured environment identity serializes");
        let generation = format!(
            "{:x}",
            Sha256::digest([b"hermes-legacy-environment-v1\0".as_slice(), &identity].concat())
        );
        Self {
            url,
            token,
            generation,
        }
    }
}
#[derive(Clone)]
pub(crate) struct SelectionContext {
    pub environment: Environment,
    pub primary: Option<Saved>,
    pub primary_profile: Option<String>,
    pub primary_remote_active: bool,
    pub isolated_backend: bool,
    pub local_home: Option<std::path::PathBuf>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Profile,
    Environment,
    Global,
    RegistryPrimary,
    Local,
}
#[derive(Clone)]
pub(crate) enum Selected {
    Remote(Remote),
    Ssh(InstalledSsh),
    Environment { url: String, token: String },
    RegistryPrimary(Saved),
    Local,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) enum RouteSlot {
    Absent(Option<String>),
    Present { id: String, revision: String },
}
impl From<ScopeVersion> for RouteSlot {
    fn from(value: ScopeVersion) -> Self {
        match value {
            ScopeVersion::Absent { revision } => Self::Absent(revision),
            ScopeVersion::Present(s) => Self::Present {
                id: s.id,
                revision: s.route_revision,
            },
        }
    }
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ResolutionVersion {
    pub profile: String,
    profile_slot: RouteSlot,
    global_slot: Option<RouteSlot>,
    environment: Option<String>,
    primary: Option<(String, String)>,
    global_remote: bool,
    native_saved: Option<(String, String)>,
}
#[derive(Clone)]
pub(crate) struct Plan {
    pub selected: Selected,
    pub origin: Origin,
    pub desktop_profile: String,
    pub backend_profile: Option<String>,
    pub own_entry: bool,
    pub profile_remote_override: bool,
    pub global_remote: bool,
    pub primary_profile: Option<String>,
    pub primary_remote_active: bool,
    pub isolated_backend: bool,
    pub version: ResolutionVersion,
}
impl Store {
    pub(in crate::connections) fn select(
        &self,
        profile: &str,
        context: &SelectionContext,
    ) -> Result<Plan, String> {
        self.validate()?;
        Scope::Profile(profile.into()).validate()?;
        let own = self.profiles.get(profile).map(|slot| &slot.value);
        let global = self
            .global
            .as_ref()
            .map(|s| &s.value)
            .cloned()
            .unwrap_or_default();
        let env_url = context.environment.url.as_deref().unwrap_or("").trim();
        let global_remote = !env_url.is_empty()
            || global.mode != Mode::Local
            || context
                .primary
                .as_ref()
                .is_some_and(|p| p.kind() != Kind::Local);
        let selected = match own {
            Some(Profile::Ssh(s)) if !s.host.trim().is_empty() => Some((
                Selected::Ssh(s.clone()),
                Origin::Profile,
                normalize_alias(s.remote_profile.as_deref()),
            )),
            Some(Profile::Remote(r)) if !r.url.trim().is_empty() => Some((
                Selected::Remote(r.clone()),
                Origin::Profile,
                normalize_alias(r.remote_profile.as_deref()),
            )),
            _ => None,
        };
        let (selected, origin, alias) = if let Some(picked) = selected {
            picked
        } else if !env_url.is_empty() {
            let token = context.environment.token.as_deref().unwrap_or("").trim();
            if token.is_empty() {
                return Err("A Legacy environment URL requires its environment token; no fallback was attempted".into());
            }
            (
                Selected::Environment {
                    url: env_url.into(),
                    token: token.into(),
                },
                Origin::Environment,
                None,
            )
        } else {
            match global.mode {
                Mode::Ssh => {
                    let ssh = global
                        .ssh
                        .as_ref()
                        .filter(|s| !s.host.trim().is_empty())
                        .ok_or("Legacy global SSH mode has no host configured")?;
                    (
                        Selected::Ssh(ssh.clone()),
                        Origin::Global,
                        normalize_alias(ssh.remote_profile.as_deref()),
                    )
                }
                Mode::Remote | Mode::Cloud => {
                    let remote = global
                        .remote
                        .as_ref()
                        .ok_or("The selected Legacy global URL is not configured")?;
                    if (global.mode == Mode::Cloud) != (remote.kind == RemoteKind::Cloud) {
                        return Err("Legacy global mode and remote kind disagree".into());
                    }
                    if remote.url.trim().is_empty() {
                        return Err(
                            "The selected Legacy global URL is empty; no fallback was attempted"
                                .into(),
                        );
                    }
                    (
                        Selected::Remote(remote.clone()),
                        Origin::Global,
                        normalize_alias(remote.remote_profile.as_deref()),
                    )
                }
                Mode::Local => match &context.primary {
                    Some(saved) if !matches!(saved.destination, Destination::Local { .. }) => (
                        Selected::RegistryPrimary(saved.clone()),
                        Origin::RegistryPrimary,
                        None,
                    ),
                    _ => (Selected::Local, Origin::Local, None),
                },
            }
        };
        let version = ResolutionVersion {
            profile: profile.into(),
            profile_slot: self.scope_version(&Scope::Profile(profile.into())).into(),
            global_slot: matches!(
                origin,
                Origin::Global | Origin::RegistryPrimary | Origin::Local
            )
            .then(|| self.scope_version(&Scope::Global).into()),
            environment: (origin != Origin::Profile)
                .then(|| context.environment.generation.clone()),
            primary: (origin == Origin::RegistryPrimary)
                .then(|| {
                    context
                        .primary
                        .as_ref()
                        .map(|p| (p.id.clone(), p.revision.clone()))
                })
                .flatten(),
            global_remote,
            native_saved: None,
        };
        Ok(Plan {
            selected,
            origin,
            desktop_profile: profile.into(),
            // REST/RPC alias policy is authored by the profile override.
            // A global SSH target keeps its separate spawn-profile metadata.
            backend_profile: (origin == Origin::Profile).then_some(alias).flatten(),
            own_entry: own.is_some(),
            profile_remote_override: origin == Origin::Profile,
            global_remote,
            primary_profile: context.primary_profile.clone(),
            primary_remote_active: context.primary_remote_active,
            isolated_backend: context.isolated_backend,
            version,
        })
    }
    pub(in crate::connections) fn validates_resolution(
        &self,
        version: &ResolutionVersion,
        context: &SelectionContext,
    ) -> bool {
        self.select(&version.profile, context)
            .is_ok_and(|latest| latest.version == *version)
    }
}

impl ResolutionVersion {
    pub(crate) fn native_saved(profile: &str, id: &str, revision: &str) -> Result<Self, String> {
        Scope::Profile(profile.into()).validate()?;
        valid_uuid(id)?;
        valid_uuid(revision)?;
        Ok(Self {
            profile: profile.into(),
            profile_slot: RouteSlot::Absent(None),
            global_slot: None,
            environment: None,
            primary: None,
            global_remote: false,
            native_saved: Some((id.into(), revision.into())),
        })
    }
    pub(in crate::connections) fn native_identity(&self) -> Option<(&str, &str)> {
        self.native_saved
            .as_ref()
            .map(|(id, revision)| (id.as_str(), revision.as_str()))
    }
    pub(crate) fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let encoded =
            serde_json::to_vec(self).expect("Legacy resolution versions contain only metadata");
        format!(
            "{:x}",
            Sha256::digest([b"hermes-legacy-resolution-v1\0".as_slice(), &encoded].concat())
        )
    }
}
impl Plan {
    pub(in crate::connections) fn profile_policy(
        &self,
    ) -> Result<crate::gateway::route::ProfilePolicy, String> {
        use crate::gateway::route::{OverrideKind, ProfilePolicy, RouteOptions};
        let override_kind = if self.origin != Origin::Profile {
            OverrideKind::None
        } else {
            match self.selected {
                Selected::Ssh(_) => OverrideKind::Ssh,
                _ => OverrideKind::Url,
            }
        };
        ProfilePolicy::new(
            &self.desktop_profile,
            RouteOptions {
                primary_profile: self.primary_profile.clone(),
                global_remote: self.global_remote,
                profile_remote_override: self.profile_remote_override,
                primary_remote_active: self.primary_remote_active,
                own_entry: self.own_entry,
                isolated_backend: self.isolated_backend,
                backend_profile: self.backend_profile.clone(),
                override_kind,
            },
        )
    }
}
