use super::{Kind, Notice};
use crate::composer::DraftScope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub enabled: bool,
    pub kinds: BTreeMap<String, bool>,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: true,
            kinds: Kind::ALL
                .into_iter()
                .map(|kind| (kind.id().into(), true))
                .collect(),
            other: BTreeMap::new(),
        }
    }
}
impl Preferences {
    pub fn kind(&self, kind: Kind) -> bool {
        self.kinds.get(kind.id()).copied().unwrap_or(true)
    }
}
pub(super) struct Policy {
    pub focused: bool,
    pub active: Option<DraftScope>,
    quiet: HashMap<(u64, String), Instant>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            focused: true,
            active: None,
            quiet: HashMap::new(),
        }
    }
}
impl Policy {
    pub fn opened(&mut self, scope: &DraftScope, now: Instant) {
        self.quiet.retain(|_, until| *until > now);
        self.quiet.insert(
            (scope.connection, scope.profile.clone()),
            now + Duration::from_secs(4),
        );
    }
    pub fn allows(&self, prefs: &Preferences, notice: &Notice, now: Instant) -> bool {
        if !prefs.enabled
            || !prefs.kind(notice.kind)
            || self
                .quiet
                .get(&(notice.scope.connection, notice.scope.profile.clone()))
                .is_some_and(|until| now < *until)
        {
            return false;
        }
        if notice.global {
            return !self.focused;
        }
        let same = self.active.as_ref() == Some(&notice.scope);
        let has_session = notice.scope.session.is_some() && !notice.runtime.is_empty();
        if matches!(notice.kind, Kind::Approval | Kind::Input) {
            !self.focused || (has_session && !same)
        } else {
            !self.focused && has_session && same
        }
    }
}
