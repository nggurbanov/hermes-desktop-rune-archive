use super::*;
use crate::connections::legacy::{
    AuthEdit, AuthMode, Edit, Global, InstalledSsh, Mode, Profile, Remote, RemoteKind, SecretEdit,
    ValueEdit,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormMode {
    Inherit,
    Url,
    Cloud,
    Ssh,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretChoice {
    Keep,
    Set,
    Clear,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Field {
    Url,
    RemoteProfile,
    Org,
    CloudName,
    Token,
    Host,
    User,
    Port,
    KeyPath,
    HermesPath,
    SshProfile,
    HeaderName(u64),
    HeaderValue(u64),
}
pub(crate) struct Header {
    pub id: u64,
    pub choice: SecretChoice,
    pub stored: bool,
    original: Option<String>,
}
// Authored secrets deliberately have no Debug or Serialize implementation.
#[derive(PartialEq, Eq)]
struct Authored {
    mode: FormMode,
    auth: AuthMode,
    token: SecretChoice,
    fields: Vec<(Field, String)>,
    headers: Vec<(u64, SecretChoice)>,
    removed: BTreeSet<String>,
}
pub(crate) struct Form {
    pub generation: u64,
    pub scope: Scope,
    expected: crate::connections::legacy::ScopeVersion,
    pub mode: FormMode,
    pub auth_mode: AuthMode,
    pub token_choice: SecretChoice,
    pub token_stored: bool,
    pub oauth_connected: bool,
    pub headers: Vec<Header>,
    fields: BTreeMap<Field, TextEdit>,
    removed: BTreeSet<String>,
    next_header: u64,
    global: Global,
    profile: Option<Profile>,
    remote: Remote,
    ssh: InstalledSsh,
    initial: Option<Authored>,
}
impl Form {
    pub(super) fn from_snapshot(scope: Scope, generation: u64, snapshot: &Snapshot) -> Self {
        let profile = match &scope {
            Scope::Global => None,
            Scope::Profile(name) => snapshot.profiles.get(name).map(|s| s.value.clone()),
        };
        let remote = match (&scope, &profile) {
            (Scope::Global, _) => snapshot.global.remote.clone(),
            (_, Some(Profile::Remote(remote))) => Some(remote.clone()),
            _ => None,
        }
        .unwrap_or_else(|| Remote::new(String::new()));
        let ssh = match &scope {
            Scope::Global => snapshot.global.ssh.clone(),
            _ => profile.as_ref().and_then(Profile::saved_ssh).cloned(),
        }
        .unwrap_or_else(|| InstalledSsh::new(String::new()));
        let mode = match &scope {
            Scope::Global => match snapshot.global.mode {
                Mode::Local => FormMode::Inherit,
                Mode::Remote => FormMode::Url,
                Mode::Cloud => FormMode::Cloud,
                Mode::Ssh => FormMode::Ssh,
            },
            _ => match &profile {
                Some(Profile::Remote(r)) if r.kind == RemoteKind::Cloud => FormMode::Cloud,
                Some(Profile::Remote(_)) => FormMode::Url,
                Some(Profile::Ssh(_)) => FormMode::Ssh,
                _ => FormMode::Inherit,
            },
        };
        let mut fields = BTreeMap::new();
        for (field, value) in [
            (Field::Url, remote.url.clone()),
            (
                Field::RemoteProfile,
                remote.remote_profile.clone().unwrap_or_default(),
            ),
            (Field::Org, remote.org.clone().unwrap_or_default()),
            (Field::CloudName, remote.name.clone().unwrap_or_default()),
            (Field::Token, String::new()),
            (Field::Host, ssh.host.clone()),
            (Field::User, ssh.user.clone()),
            (Field::Port, ssh.port.to_string()),
            (Field::KeyPath, ssh.key_path.clone().unwrap_or_default()),
            (
                Field::HermesPath,
                ssh.remote_hermes_path.clone().unwrap_or_default(),
            ),
            (
                Field::SshProfile,
                ssh.remote_profile.clone().unwrap_or_default(),
            ),
        ] {
            fields.insert(field, TextEdit::new(&value));
        }
        let headers = remote
            .header_names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let id = index as u64 + 1;
                fields.insert(Field::HeaderName(id), TextEdit::new(name));
                fields.insert(Field::HeaderValue(id), TextEdit::new(""));
                Header {
                    id,
                    choice: SecretChoice::Keep,
                    stored: remote.has_credential(),
                    original: Some(name.clone()),
                }
            })
            .collect::<Vec<_>>();
        let mut form = Self {
            generation,
            expected: snapshot.scope_version(&scope),
            scope,
            mode,
            auth_mode: remote.auth_mode,
            token_choice: SecretChoice::Keep,
            token_stored: remote.has_token(),
            oauth_connected: remote.has_oauth_credential(),
            next_header: headers.len() as u64 + 1,
            headers,
            fields,
            removed: BTreeSet::new(),
            global: snapshot.global.clone(),
            profile,
            remote,
            ssh,
            initial: None,
        };
        form.initial = Some(form.authored());
        form
    }
    fn authored(&self) -> Authored {
        Authored {
            mode: self.mode,
            auth: self.auth_mode,
            token: self.token_choice,
            fields: self
                .fields
                .iter()
                .map(|(f, t)| (*f, t.text.clone()))
                .collect(),
            headers: self.headers.iter().map(|h| (h.id, h.choice)).collect(),
            removed: self.removed.clone(),
        }
    }
    pub(super) fn expected(&self) -> crate::connections::legacy::ScopeVersion {
        self.expected.clone()
    }
    pub(crate) fn has_override(&self) -> bool {
        matches!(self.scope, Scope::Profile(_))
            && matches!(
                self.expected,
                crate::connections::legacy::ScopeVersion::Present(_)
            )
    }
    pub(crate) fn dirty(&self) -> bool {
        self.initial
            .as_ref()
            .is_none_or(|initial| *initial != self.authored())
    }
    pub(crate) fn field(&self, field: Field) -> Option<&TextEdit> {
        self.fields.get(&field)
    }
    pub(super) fn field_mut(&mut self, field: Field) -> Option<&mut TextEdit> {
        self.fields.get_mut(&field)
    }
    fn value(&self, field: Field) -> &str {
        self.fields
            .get(&field)
            .map(|v| v.text.as_str())
            .unwrap_or("")
    }
    fn trimmed(&self, field: Field) -> String {
        self.value(field).trim().to_owned()
    }
    fn optional(&self, field: Field) -> Option<String> {
        let value = self.trimmed(field);
        (!value.is_empty()).then_some(value)
    }
    pub(super) fn add_header(&mut self) {
        let id = self.next_header;
        self.next_header += 1;
        self.fields.insert(Field::HeaderName(id), TextEdit::new(""));
        self.fields
            .insert(Field::HeaderValue(id), TextEdit::new(""));
        self.headers.push(Header {
            id,
            choice: SecretChoice::Set,
            stored: false,
            original: None,
        });
    }
    pub(super) fn remove_header(&mut self, id: u64) {
        if let Some(index) = self.headers.iter().position(|h| h.id == id) {
            let header = self.headers.remove(index);
            if let Some(name) = header.original {
                self.removed.insert(name.to_ascii_lowercase());
            }
            self.fields.remove(&Field::HeaderName(id));
            self.fields.remove(&Field::HeaderValue(id));
        }
    }
    fn secret(choice: SecretChoice, value: &str) -> Result<SecretEdit, String> {
        Ok(match choice {
            SecretChoice::Keep => SecretEdit::Keep,
            SecretChoice::Clear => SecretEdit::Clear,
            SecretChoice::Set => {
                if value.trim().is_empty() {
                    return Err("Enter a secret value or choose Clear".into());
                }
                SecretEdit::Set(value.to_owned())
            }
        })
    }
    fn auth(&self) -> Result<AuthEdit, String> {
        let mut headers = self
            .removed
            .iter()
            .map(|name| (name.clone(), SecretEdit::Clear))
            .collect::<BTreeMap<_, _>>();
        let mut names = BTreeSet::new();
        for header in &self.headers {
            let name = self
                .trimmed(Field::HeaderName(header.id))
                .to_ascii_lowercase();
            if name.is_empty() || !names.insert(name.clone()) {
                return Err("Access header names must be nonempty and unique".into());
            }
            let allowed = crate::gateway::prepared::normalize_access_headers(BTreeMap::from([(
                name.clone(),
                "validate".into(),
            )]))?;
            if !allowed.contains_key(&name) {
                return Err(
                    "This access header is invalid or reserved for gateway authentication".into(),
                );
            }
            let renamed = header
                .original
                .as_ref()
                .is_some_and(|old| !old.eq_ignore_ascii_case(&name));
            if (renamed || !header.stored) && header.choice == SecretChoice::Keep {
                return Err("Enter a value for each new or renamed access header".into());
            }
            if renamed {
                headers.insert(
                    header.original.as_ref().unwrap().to_ascii_lowercase(),
                    SecretEdit::Clear,
                );
            }
            headers.insert(
                name,
                Self::secret(header.choice, self.value(Field::HeaderValue(header.id)))?,
            );
        }
        Ok(AuthEdit {
            token: Self::secret(self.token_choice, self.value(Field::Token))?,
            headers,
        })
    }
    fn remote_value(&self) -> Remote {
        let mut remote = self.remote.clone();
        remote.kind = if self.mode == FormMode::Cloud {
            RemoteKind::Cloud
        } else {
            self.remote.kind
        };
        if self.mode == FormMode::Url {
            remote.kind = RemoteKind::Url;
        }
        remote.url = self.trimmed(Field::Url);
        remote.auth_mode = self.auth_mode;
        remote.remote_profile = self.optional(Field::RemoteProfile);
        remote.org = self.optional(Field::Org);
        remote.name = self.optional(Field::CloudName);
        remote
    }
    fn ssh_value(&self) -> Result<InstalledSsh, String> {
        let mut ssh = self.ssh.clone();
        ssh.host = self.trimmed(Field::Host);
        ssh.user = self.trimmed(Field::User);
        ssh.port = self
            .trimmed(Field::Port)
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .ok_or("Enter an SSH port between 1 and 65535")?;
        ssh.key_path = self.optional(Field::KeyPath);
        ssh.remote_hermes_path = self.optional(Field::HermesPath);
        ssh.remote_profile = self.optional(Field::SshProfile);
        Ok(ssh)
    }
    pub(super) fn edit(&self, remove: bool) -> Result<Edit, String> {
        if remove {
            return Ok(Edit {
                scope: self.scope.clone(),
                expected: self.expected.clone(),
                value: ValueEdit::Remove,
                auth: AuthEdit::default(),
            });
        }
        if matches!(self.mode, FormMode::Url | FormMode::Cloud)
            && self.trimmed(Field::Url).is_empty()
        {
            return Err("Enter a gateway URL".into());
        }
        if self.mode == FormMode::Ssh && self.trimmed(Field::Host).is_empty() {
            return Err("Enter an SSH host".into());
        }
        let value = match &self.scope {
            Scope::Global => {
                let mut global = self.global.clone();
                global.mode = match self.mode {
                    FormMode::Inherit => Mode::Local,
                    FormMode::Url => Mode::Remote,
                    FormMode::Cloud => Mode::Cloud,
                    FormMode::Ssh => Mode::Ssh,
                };
                if global.remote.is_some()
                    || matches!(self.mode, FormMode::Url | FormMode::Cloud)
                    || !self.trimmed(Field::Url).is_empty()
                {
                    global.remote = Some(self.remote_value());
                }
                if global.ssh.is_some()
                    || self.mode == FormMode::Ssh
                    || !self.trimmed(Field::Host).is_empty()
                {
                    global.ssh = Some(self.ssh_value()?);
                }
                ValueEdit::Global(global)
            }
            Scope::Profile(_) => ValueEdit::Profile(match self.mode {
                FormMode::Inherit => {
                    if self.profile.as_ref().and_then(Profile::saved_ssh).is_some()
                        || !self.trimmed(Field::Host).is_empty()
                    {
                        let prior = Profile::ssh_from(self.profile.as_ref(), self.ssh_value()?);
                        Profile::inherit_from(Some(&prior))
                    } else {
                        Profile::inherit_from(self.profile.as_ref())
                    }
                }
                FormMode::Url | FormMode::Cloud => {
                    Profile::remote_from(self.profile.as_ref(), self.remote_value())
                }
                FormMode::Ssh => Profile::ssh_from(self.profile.as_ref(), self.ssh_value()?),
            }),
        };
        let auth = if matches!(&value,ValueEdit::Global(g) if g.remote.is_some())
            || matches!(self.mode, FormMode::Url | FormMode::Cloud)
        {
            self.auth()?
        } else {
            AuthEdit::default()
        };
        Ok(Edit {
            scope: self.scope.clone(),
            expected: self.expected.clone(),
            value,
            auth,
        })
    }
}
