use super::*;
use crate::gateway::{BackendRootProof, route::WireScope, valid_scope};

#[derive(Clone)]
pub(crate) enum BackendTarget {
    Named(String),
    Root(BackendRootProof),
}
impl fmt::Debug for BackendTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => f.debug_tuple("Named").field(name).finish(),
            Self::Root(proof) => f
                .debug_struct("Root")
                .field("profile", &proof.profile)
                .finish_non_exhaustive(),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct SelectedContext {
    window_alias: String,
    target: BackendTarget,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProfileProvenance {
    NativeSelf,
    Backend(String),
    Ambient(Option<String>),
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum BackendTargetSnapshot {
    Named {
        name: String,
    },
    Root {
        profile: Option<String>,
        profile_id: String,
        instance_id: String,
        authority_epoch: u64,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectedContextSnapshot {
    version: u8,
    window_alias: String,
    target: BackendTargetSnapshot,
}
impl SelectedContext {
    pub(crate) fn new(window_alias: &str, target: BackendTarget) -> Result<Self, String> {
        valid_scope(window_alias)?;
        match &target {
            BackendTarget::Named(name) => valid_scope(name)?,
            BackendTarget::Root(proof) => proof.validate()?,
        }
        Ok(Self {
            window_alias: window_alias.into(),
            target,
        })
    }
    pub(crate) fn window_alias(&self) -> &str {
        &self.window_alias
    }
    pub(crate) fn target(&self) -> &BackendTarget {
        &self.target
    }
    pub(super) fn fingerprint(&self) -> String {
        // A restarted runtime changes authority, not this durable namespace.
        let stable = match &self.target {
            BackendTarget::Named(name) => serde_json::json!([self.window_alias, "named", name]),
            BackendTarget::Root(root) => {
                serde_json::json!([self.window_alias, "root", root.profile_id])
            }
        };
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&stable).expect("string context serializes"))
        )
    }
    pub(super) fn private_ipc_snapshot(&self) -> SelectedContextSnapshot {
        let target = match &self.target {
            BackendTarget::Named(name) => BackendTargetSnapshot::Named { name: name.clone() },
            BackendTarget::Root(proof) => BackendTargetSnapshot::Root {
                profile: proof.profile.clone(),
                profile_id: proof.profile_id.clone(),
                instance_id: proof.instance_id.clone(),
                authority_epoch: proof.authority_epoch,
            },
        };
        SelectedContextSnapshot {
            version: 1,
            window_alias: self.window_alias.clone(),
            target,
        }
    }
    pub(super) fn from_private_ipc(snapshot: SelectedContextSnapshot) -> Result<Self, String> {
        if snapshot.version != 1 {
            return Err("Unsupported selected backend context".into());
        }
        let target = match snapshot.target {
            BackendTargetSnapshot::Named { name } => BackendTarget::Named(name),
            BackendTargetSnapshot::Root {
                profile,
                profile_id,
                instance_id,
                authority_epoch,
            } => BackendTarget::Root(BackendRootProof {
                profile,
                profile_id,
                instance_id,
                authority_epoch,
            }),
        };
        Self::new(&snapshot.window_alias, target)
    }
}
impl WirePolicy {
    pub(crate) fn rpc_scope(&self, provenance: &ProfileProvenance) -> Result<WireScope, String> {
        match provenance {
            ProfileProvenance::NativeSelf => match self
                .selected
                .as_ref()
                .ok_or("The Desktop route has no selected backend context")?
                .target()
            {
                BackendTarget::Named(name) => Ok(WireScope::Named(name.clone())),
                BackendTarget::Root(_) => Ok(WireScope::Omit),
            },
            ProfileProvenance::Backend(name) => {
                valid_scope(name)?;
                Ok(WireScope::Named(name.clone()))
            }
            ProfileProvenance::Ambient(original) => {
                self.profiles.ambient_rpc_scope(original.as_deref())
            }
        }
    }
    pub(crate) fn rest_route(
        &self,
        provenance: &ProfileProvenance,
        method: &str,
        path: &str,
    ) -> Result<String, String> {
        match provenance {
            ProfileProvenance::Backend(name) => {
                ProfilePolicy::identity().rest_self_query(name, method, path)
            }
            ProfileProvenance::Ambient(original) => self.profiles.rest_self_query(
                original
                    .as_deref()
                    .or(self.profiles.desktop_profile())
                    .unwrap_or(""),
                method,
                path,
            ),
            ProfileProvenance::NativeSelf => {
                let selected = self
                    .selected
                    .as_ref()
                    .ok_or("The Desktop route has no selected backend context")?;
                let mut url = Url::parse("http://scope.invalid")
                    .unwrap()
                    .join(path)
                    .map_err(|_| "Invalid scoped API path")?;
                let sidebar = url.path() == "/api/profiles/sessions/sidebar";
                let mut pairs = Vec::new();
                for (key, value) in url.query_pairs() {
                    let is_self = (key == "profile" || sidebar && key == "recents_profile")
                        && value == selected.window_alias();
                    match (is_self, selected.target()) {
                        (true, BackendTarget::Named(name)) => {
                            pairs.push((key.into_owned(), name.clone()))
                        }
                        (true, BackendTarget::Root(_)) => {}
                        _ => pairs.push((key.into_owned(), value.into_owned())),
                    }
                }
                url.set_query(None);
                if !pairs.is_empty() {
                    url.query_pairs_mut().extend_pairs(pairs);
                }
                let mut path = url.path().to_owned();
                if let Some(query) = url.query() {
                    path.push('?');
                    path.push_str(query);
                }
                match selected.target() {
                    BackendTarget::Named(name) => {
                        ProfilePolicy::identity().rest_self_query(name, method, &path)
                    }
                    BackendTarget::Root(_) => Ok(path),
                }
            }
        }
    }
}
