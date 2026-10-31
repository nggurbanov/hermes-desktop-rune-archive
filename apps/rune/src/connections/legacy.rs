//! Durable Legacy routing is separate from Saved transport selection.
//! Secrets are immutable OS-keyring bundles; settings contain references only.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
mod persistence;
mod resolution;
pub(crate) use resolution::{
    CapturedPrimary, ResolveReceipt, ResolveRequest, ResolveTicket, ResolvedLegacy,
};
mod selection;
pub(crate) use persistence::{SaveReceipt, SaveTicket};
pub(crate) use selection::{
    Environment, Origin, Plan, ResolutionVersion, Selected, SelectionContext,
};
#[cfg(test)]
mod tests;
pub(in crate::connections) type Unknown = BTreeMap<String, Value>;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Scope {
    Global,
    Profile(String),
}
impl Scope {
    fn key(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Profile(p) => format!("profiles/{p}"),
        }
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
            Self::Profile(p) if !super::startup::valid_profile(p) => {
                Err("The Legacy profile scope is invalid".into())
            }
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SlotStamp {
    pub id: String,
    pub edit_revision: String,
    pub route_revision: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScopeVersion {
    Absent { revision: Option<String> },
    Present(SlotStamp),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(in crate::connections) struct Slot<T> {
    #[serde(flatten)]
    pub stamp: SlotStamp,
    pub value: T,
    #[serde(flatten)]
    pub other: Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::connections) struct Store {
    pub version: u32,
    #[serde(default)]
    pub global: Option<Slot<Global>>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Slot<Profile>>,
    #[serde(default)]
    pub tombstones: BTreeMap<String, String>,
    #[serde(flatten)]
    pub other: Unknown,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            version: 1,
            global: None,
            profiles: BTreeMap::new(),
            tombstones: BTreeMap::new(),
            other: Unknown::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    #[default]
    Local,
    Remote,
    Cloud,
    Ssh,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RemoteKind {
    Url,
    Cloud,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthMode {
    #[default]
    Token,
    Oauth,
}
// Deliberately no Debug: even secret-store references stay out of diagnostics.
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::connections) struct CredentialRef {
    id: String,
    version: u32,
    #[serde(flatten)]
    other: Unknown,
}
impl PartialEq for CredentialRef {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.version == other.version
    }
}
impl Eq for CredentialRef {}
impl CredentialRef {
    pub(in crate::connections) fn fresh() -> Self {
        Self {
            id: format!("legacy-auth-{}", uuid::Uuid::new_v4()),
            version: 1,
            other: Unknown::new(),
        }
    }
    pub(in crate::connections) fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self
                .id
                .strip_prefix("legacy-auth-")
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .is_none()
        {
            return Err("The Legacy credential reference requires repair or a newer app".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Remote {
    pub kind: RemoteKind,
    pub url: String,
    #[serde(default)]
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub remote_profile: Option<String>,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub header_names: Vec<String>,
    #[serde(default)]
    pub(in crate::connections) token_present: bool,
    #[serde(default)]
    pub(in crate::connections) credential: Option<CredentialRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::connections) oauth_credential: Option<CredentialRef>,
    #[serde(flatten)]
    pub(in crate::connections) other: Unknown,
}
impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remote")
            .field("kind", &self.kind)
            .field("url", &self.url)
            .field("auth_mode", &self.auth_mode)
            .field("remote_profile", &self.remote_profile)
            .field("has_credential", &self.has_credential())
            .field("has_oauth_credential", &self.oauth_credential.is_some())
            .finish_non_exhaustive()
    }
}
impl Remote {
    pub(crate) fn new(url: String) -> Self {
        Self {
            kind: RemoteKind::Url,
            url,
            auth_mode: AuthMode::Token,
            remote_profile: None,
            org: None,
            name: None,
            header_names: vec![],
            token_present: false,
            credential: None,
            oauth_credential: None,
            other: Unknown::new(),
        }
    }
    pub(crate) fn has_credential(&self) -> bool {
        self.credential.is_some()
    }
    pub(crate) fn has_oauth_credential(&self) -> bool {
        self.oauth_credential.is_some()
    }
    pub(crate) fn has_token(&self) -> bool {
        self.token_present && self.credential.is_some()
    }
    fn routing_eq(&self, other: &Self, include_alias: bool) -> bool {
        let headers = |r: &Self| {
            r.header_names
                .iter()
                .map(|n| n.to_ascii_lowercase())
                .collect::<BTreeSet<_>>()
        };
        self.kind == other.kind
            && self.url == other.url
            && self.auth_mode == other.auth_mode
            && self.credential == other.credential
            && self.oauth_credential == other.oauth_credential
            && (self.kind != RemoteKind::Cloud || self.org == other.org)
            && (!include_alias
                || normalize_alias(self.remote_profile.as_deref())
                    == normalize_alias(other.remote_profile.as_deref()))
            && headers(self) == headers(other)
    }
    fn validate(&self) -> Result<(), String> {
        if let Some(reference) = &self.credential {
            reference.validate()?;
        }
        if let Some(reference) = &self.oauth_credential {
            reference.validate()?;
            if self.auth_mode != AuthMode::Oauth {
                return Err("OAuth credentials require OAuth authentication".into());
            }
            if self.credential.as_ref() == Some(reference) {
                return Err(
                    "OAuth and static gateway credentials must have distinct owners".into(),
                );
            }
        }
        if !self.url.trim().is_empty() {
            let (_, embedded) = crate::gateway::endpoint_url(&self.url)?;
            if embedded.is_some() {
                return Err("Legacy settings cannot contain a gateway token in the URL".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InstalledSsh {
    pub host: String,
    #[serde(default)]
    pub user: String,
    #[serde(default = "ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub remote_hermes_path: Option<String>,
    #[serde(default)]
    pub remote_profile: Option<String>,
    #[serde(default)]
    pub(in crate::connections) credential: Option<CredentialRef>,
    #[serde(flatten)]
    pub(in crate::connections) other: Unknown,
}
impl InstalledSsh {
    pub(crate) fn new(host: String) -> Self {
        Self {
            host,
            user: String::new(),
            port: 22,
            key_path: None,
            remote_hermes_path: None,
            remote_profile: None,
            credential: None,
            other: Unknown::new(),
        }
    }
    fn routing_eq(&self, other: &Self) -> bool {
        self.host == other.host
            && self.user == other.user
            && self.port == other.port
            && self.key_path == other.key_path
            && self.remote_hermes_path == other.remote_hermes_path
            && normalize_alias(self.remote_profile.as_deref())
                == normalize_alias(other.remote_profile.as_deref())
            && self.credential == other.credential
    }
}
fn ssh_port() -> u16 {
    22
}
impl std::fmt::Debug for InstalledSsh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstalledSsh")
            .field("host", &self.host)
            .field("remote_profile", &self.remote_profile)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Global {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub remote: Option<Remote>,
    #[serde(default)]
    pub ssh: Option<InstalledSsh>,
    #[serde(flatten)]
    pub(in crate::connections) other: Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum Profile {
    #[serde(rename = "local")]
    Inherit {
        #[serde(default)]
        saved_ssh: Option<InstalledSsh>,
        #[serde(flatten)]
        other: Unknown,
    },
    Remote(Remote),
    Ssh(InstalledSsh),
}
impl Profile {
    fn other(&self) -> &Unknown {
        match self {
            Self::Inherit { other, .. } => other,
            Self::Remote(remote) => &remote.other,
            Self::Ssh(ssh) => &ssh.other,
        }
    }
    fn other_mut(&mut self) -> &mut Unknown {
        match self {
            Self::Inherit { other, .. } => other,
            Self::Remote(remote) => &mut remote.other,
            Self::Ssh(ssh) => &mut ssh.other,
        }
    }
    pub(crate) fn remote_from(previous: Option<&Self>, remote: Remote) -> Self {
        let mut value = Self::Remote(remote);
        if let Some(previous) = previous {
            preserve_unknown(previous.other(), value.other_mut());
        }
        value
    }
    pub(crate) fn ssh_from(previous: Option<&Self>, ssh: InstalledSsh) -> Self {
        let mut value = Self::Ssh(ssh);
        if let Some(previous) = previous {
            preserve_unknown(previous.other(), value.other_mut());
        }
        value
    }
    pub(crate) fn saved_ssh(&self) -> Option<&InstalledSsh> {
        match self {
            Self::Ssh(ssh) => Some(ssh),
            Self::Inherit { saved_ssh, .. } => saved_ssh.as_ref(),
            _ => None,
        }
    }
    pub(crate) fn inherit_from(previous: Option<&Self>) -> Self {
        match previous {
            Some(Self::Ssh(ssh)) => Self::Inherit {
                saved_ssh: Some(ssh.clone()),
                other: ssh.other.clone(),
            },
            Some(Self::Inherit { saved_ssh, other }) => Self::Inherit {
                saved_ssh: saved_ssh.clone(),
                other: other.clone(),
            },
            _ => Self::Inherit {
                saved_ssh: None,
                other: previous.map(|p| p.other().clone()).unwrap_or_default(),
            },
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub global: Global,
    pub global_version: ScopeVersion,
    pub profiles: BTreeMap<String, ProfileSnapshot>,
    tombstones: BTreeMap<String, String>,
}
impl Snapshot {
    pub(crate) fn scope_version(&self, scope: &Scope) -> ScopeVersion {
        match scope {
            Scope::Global => self.global_version.clone(),
            Scope::Profile(name) => self
                .profiles
                .get(name)
                .map(|s| s.version.clone())
                .unwrap_or_else(|| ScopeVersion::Absent {
                    revision: self.tombstones.get(&scope.key()).cloned(),
                }),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct ProfileSnapshot {
    pub value: Profile,
    pub version: ScopeVersion,
}
#[derive(Clone, Debug)]
pub(crate) enum ValueEdit {
    Global(Global),
    Profile(Profile),
    Remove,
}
// Authored secret intents must never derive Debug or Serialize.
pub(crate) enum SecretEdit {
    Keep,
    Clear,
    Set(String),
}
pub(crate) struct AuthEdit {
    pub token: SecretEdit,
    pub headers: BTreeMap<String, SecretEdit>,
}
impl Default for AuthEdit {
    fn default() -> Self {
        Self {
            token: SecretEdit::Keep,
            headers: BTreeMap::new(),
        }
    }
}
pub(crate) struct Edit {
    pub scope: Scope,
    pub expected: ScopeVersion,
    pub value: ValueEdit,
    pub auth: AuthEdit,
}
pub(in crate::connections) struct AuthBundle {
    pub token: Option<String>,
    pub headers: BTreeMap<String, String>,
}
impl Store {
    pub(in crate::connections) fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Legacy settings require a newer Hermes app".into());
        }
        if let Some(slot) = &self.global {
            validate_stamp(&slot.stamp)?;
            validate_global(&slot.value)?;
        }
        for (name, slot) in &self.profiles {
            Scope::Profile(name.clone()).validate()?;
            validate_stamp(&slot.stamp)?;
            validate_profile(&slot.value)?;
        }
        for (key, revision) in &self.tombstones {
            if key != "global" {
                Scope::Profile(
                    key.strip_prefix("profiles/")
                        .ok_or("The Legacy tombstone scope is invalid")?
                        .into(),
                )
                .validate()?;
            }
            valid_uuid(revision)?;
        }
        Ok(())
    }
    pub(in crate::connections) fn scope_version(&self, scope: &Scope) -> ScopeVersion {
        let stamp = match scope {
            Scope::Global => self.global.as_ref().map(|s| &s.stamp),
            Scope::Profile(p) => self.profiles.get(p).map(|s| &s.stamp),
        };
        stamp
            .map(|s| ScopeVersion::Present(s.clone()))
            .unwrap_or_else(|| ScopeVersion::Absent {
                revision: self.tombstones.get(&scope.key()).cloned(),
            })
    }
    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            global: self
                .global
                .as_ref()
                .map(|s| s.value.clone())
                .unwrap_or_default(),
            global_version: self.scope_version(&Scope::Global),
            tombstones: self.tombstones.clone(),
            profiles: self
                .profiles
                .iter()
                .map(|(name, slot)| {
                    (
                        name.clone(),
                        ProfileSnapshot {
                            value: slot.value.clone(),
                            version: ScopeVersion::Present(slot.stamp.clone()),
                        },
                    )
                })
                .collect(),
        }
    }
    pub(in crate::connections) fn apply(
        &mut self,
        scope: &Scope,
        expected: &ScopeVersion,
        value: ValueEdit,
    ) -> Result<ScopeVersion, String> {
        self.validate()?;
        scope.validate()?;
        if &self.scope_version(scope) != expected {
            return Err(
                "These Legacy settings changed in another window; reopen them before saving".into(),
            );
        }
        match (scope, value) {
            (Scope::Global, ValueEdit::Global(mut value)) => {
                if let Some(previous) = &self.global {
                    preserve_global_fields(&previous.value, &mut value);
                }
                validate_global(&value)?;
                let previous = self.global.as_ref();
                let stamp = next_stamp(
                    previous.map(|s| &s.stamp),
                    previous.is_some_and(|s| global_routing_eq(&s.value, &value)),
                );
                let other = previous.map(|s| s.other.clone()).unwrap_or_default();
                self.global = Some(Slot {
                    stamp,
                    value,
                    other,
                });
            }
            (Scope::Profile(name), ValueEdit::Profile(mut value)) => {
                if let Some(previous) = self.profiles.get(name) {
                    preserve_profile_fields(&previous.value, &mut value);
                }
                validate_profile(&value)?;
                let previous = self.profiles.get(name);
                let stamp = next_stamp(
                    previous.map(|s| &s.stamp),
                    previous.is_some_and(|s| profile_routing_eq(&s.value, &value)),
                );
                let other = previous.map(|s| s.other.clone()).unwrap_or_default();
                self.profiles.insert(
                    name.clone(),
                    Slot {
                        stamp,
                        value,
                        other,
                    },
                );
            }
            (_, ValueEdit::Remove) => {
                match scope {
                    Scope::Global => self.global = None,
                    Scope::Profile(p) => {
                        self.profiles.remove(p);
                    }
                }
                self.tombstones
                    .insert(scope.key(), uuid::Uuid::new_v4().to_string());
                return Ok(self.scope_version(scope));
            }
            _ => return Err("The Legacy edit does not match its scope".into()),
        }
        self.tombstones.remove(&scope.key());
        Ok(self.scope_version(scope))
    }
    pub(in crate::connections) fn credential_refs(&self) -> BTreeSet<String> {
        let mut refs = BTreeSet::new();
        let mut ssh_ref = |ssh: Option<&InstalledSsh>| {
            if let Some(reference) = ssh.and_then(|s| s.credential.as_ref()) {
                refs.insert(reference.id().to_owned());
            }
        };
        ssh_ref(self.global.as_ref().and_then(|s| s.value.ssh.as_ref()));
        for slot in self.profiles.values() {
            match &slot.value {
                Profile::Inherit { saved_ssh, .. } => ssh_ref(saved_ssh.as_ref()),
                Profile::Ssh(ssh) => ssh_ref(Some(ssh)),
                _ => {}
            }
        }
        for remote in self
            .global
            .iter()
            .filter_map(|s| s.value.remote.as_ref())
            .chain(self.profiles.values().filter_map(|s| match &s.value {
                Profile::Remote(r) => Some(r),
                _ => None,
            }))
        {
            if let Some(reference) = &remote.credential {
                refs.insert(reference.id().to_owned());
            }
            if let Some(reference) = &remote.oauth_credential {
                refs.insert(reference.id().to_owned());
            }
        }
        refs
    }
}
fn next_stamp(previous: Option<&SlotStamp>, same_route: bool) -> SlotStamp {
    SlotStamp {
        id: previous
            .map(|s| s.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        edit_revision: uuid::Uuid::new_v4().to_string(),
        route_revision: previous
            .filter(|_| same_route)
            .map(|s| s.route_revision.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
    }
}
fn validate_stamp(stamp: &SlotStamp) -> Result<(), String> {
    valid_uuid(&stamp.id)?;
    valid_uuid(&stamp.edit_revision)?;
    valid_uuid(&stamp.route_revision)
}
fn valid_uuid(value: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| "The Legacy settings revision is invalid".into())
}
fn validate_ssh(value: &InstalledSsh) -> Result<(), String> {
    if let Some(reference) = &value.credential {
        reference.validate()?;
    }
    Ok(())
}
fn validate_global(value: &Global) -> Result<(), String> {
    if let Some(r) = &value.remote {
        r.validate()?;
    }
    if let Some(s) = &value.ssh {
        validate_ssh(s)?;
    }
    Ok(())
}
fn validate_profile(value: &Profile) -> Result<(), String> {
    match value {
        Profile::Inherit { saved_ssh, .. } => {
            saved_ssh.as_ref().map(validate_ssh).unwrap_or(Ok(()))
        }
        Profile::Remote(r) => r.validate(),
        Profile::Ssh(s) => validate_ssh(s),
    }
}
fn global_routing_eq(a: &Global, b: &Global) -> bool {
    a.mode == b.mode
        && match a.mode {
            Mode::Local => true,
            Mode::Remote | Mode::Cloud => match (&a.remote, &b.remote) {
                (Some(a), Some(b)) => a.routing_eq(b, false),
                (None, None) => true,
                _ => false,
            },
            Mode::Ssh => match (&a.ssh, &b.ssh) {
                (Some(a), Some(b)) => a.routing_eq(b),
                (None, None) => true,
                _ => false,
            },
        }
}
fn profile_routing_eq(a: &Profile, b: &Profile) -> bool {
    match (a, b) {
        (Profile::Inherit { .. }, Profile::Inherit { .. }) => true,
        (Profile::Remote(a), Profile::Remote(b)) => a.routing_eq(b, true),
        (Profile::Ssh(a), Profile::Ssh(b)) => a.routing_eq(b),
        _ => false,
    }
}
pub(crate) fn normalize_alias(value: Option<&str>) -> Option<String> {
    let name = value?.trim();
    (super::startup::valid_profile(name)
        && !matches!(name, "hermes" | "test" | "tmp" | "root" | "sudo"))
    .then(|| name.to_owned())
}

fn preserve_unknown(previous: &Unknown, current: &mut Unknown) {
    let authored = std::mem::take(current);
    *current = previous.clone();
    current.extend(authored);
}
fn preserve_global_fields(previous: &Global, current: &mut Global) {
    preserve_unknown(&previous.other, &mut current.other);
    if let (Some(old), Some(new)) = (&previous.remote, &mut current.remote) {
        preserve_unknown(&old.other, &mut new.other);
    }
    if let (Some(old), Some(new)) = (&previous.ssh, &mut current.ssh) {
        preserve_unknown(&old.other, &mut new.other);
    }
}
fn preserve_profile_fields(previous: &Profile, current: &mut Profile) {
    preserve_unknown(previous.other(), current.other_mut());
    match (previous, current) {
        (
            Profile::Inherit {
                saved_ssh: a,
                other: _,
            },
            Profile::Inherit {
                saved_ssh: b,
                other: _,
            },
        ) => {
            if let (Some(a), Some(b)) = (a, b) {
                preserve_unknown(&a.other, &mut b.other);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
pub(crate) mod testing {
    pub(crate) use crate::connections::core::legacy::testing::*;
}
