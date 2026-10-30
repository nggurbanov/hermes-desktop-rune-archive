//! Device presentation for the fleet. Profile names intentionally share color/order.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

mod color;
pub use color::hue_color;
#[cfg(test)]
mod tests;

#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct Group {
    alias: Option<String>,
    collapsed: bool,
    #[serde(flatten)]
    other: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct Document {
    version: u32,
    profile_order: Vec<String>,
    profile_colors: BTreeMap<String, color::Authored>,
    source_order: Vec<String>,
    groups: BTreeMap<String, Group>,
    #[serde(flatten)]
    other: BTreeMap<String, serde_json::Value>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            profile_order: vec![],
            profile_colors: BTreeMap::new(),
            source_order: vec![],
            groups: BTreeMap::new(),
            other: BTreeMap::new(),
        }
    }
}
impl Document {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(
                "Fleet preferences require a newer Hermes app; their file was retained".into(),
            );
        }
        if self.profile_order.len() > 4096
            || self.source_order.len() > 4096
            || self.groups.len() > 4096
            || self.profile_colors.len() > 4096
        {
            return Err(
                "Fleet preferences exceed their size limit; their file was retained".into(),
            );
        }
        if self
            .profile_order
            .iter()
            .chain(self.profile_colors.keys())
            .any(|name| !valid_name(name))
            || self
                .source_order
                .iter()
                .chain(self.groups.keys())
                .any(|key| !valid_key(key))
            || self.profile_colors.values().any(|value| !value.valid())
            || self.groups.values().any(|group| {
                group
                    .alias
                    .as_ref()
                    .is_some_and(|s| s.trim().is_empty() || s.chars().any(char::is_control))
            })
        {
            return Err("Fleet preferences are invalid; their file was retained".into());
        }
        Ok(())
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}
fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 512 && !key.chars().any(char::is_control)
}

#[derive(Clone)]
enum Intent {
    Color(String, Option<color::Authored>),
    ProfileOrder(Vec<String>),
    SourceOrder(Vec<String>),
    ReorderSources(Vec<String>),
    Alias(String, Option<String>),
    Collapse(String, bool),
}
impl Intent {
    fn apply(&self, doc: &mut Document) -> Vec<String> {
        match self {
            Self::Color(name, hue) => {
                match hue {
                    Some(hue) => {
                        doc.profile_colors.insert(name.clone(), hue.clone());
                    }
                    None => {
                        doc.profile_colors.remove(name);
                    }
                }
                vec!["profile_colors".into(), name.clone()]
            }
            Self::ProfileOrder(names) => {
                doc.profile_order = names.clone();
                vec!["profile_order".into()]
            }
            Self::SourceOrder(keys) => {
                doc.source_order = keys.clone();
                vec!["source_order".into()]
            }
            Self::ReorderSources(visible) => {
                let mut all = doc.source_order.clone();
                for key in visible {
                    if !all.contains(key) {
                        all.push(key.clone());
                    }
                }
                let mut replacements = visible.iter();
                doc.source_order = all
                    .into_iter()
                    .map(|key| {
                        if visible.contains(&key) {
                            replacements.next().cloned().unwrap_or(key)
                        } else {
                            key
                        }
                    })
                    .collect();
                vec!["source_order".into()]
            }
            Self::Alias(key, alias) => {
                doc.groups.entry(key.clone()).or_default().alias = alias.clone();
                vec!["groups".into(), key.clone(), "alias".into()]
            }
            Self::Collapse(key, value) => {
                doc.groups.entry(key.clone()).or_default().collapsed = *value;
                vec!["groups".into(), key.clone(), "collapsed".into()]
            }
        }
    }
}

pub struct Preferences {
    path: Option<PathBuf>,
    unavailable: Option<String>,
    document: Document,
    pending: VecDeque<Intent>,
    error: Option<String>,
    retry_at: Option<Instant>,
    next_read: Instant,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            path: None,
            unavailable: None,
            document: Document::default(),
            pending: VecDeque::new(),
            error: None,
            retry_at: None,
            next_read: Instant::now() + Duration::from_secs(1),
        }
    }
}
impl Preferences {
    pub fn unavailable(error: String) -> Self {
        Self {
            unavailable: Some(error.clone()),
            error: Some(error),
            ..Default::default()
        }
    }
    pub fn explicit_profile_hue(&self, name: &str) -> Option<u16> {
        if name.is_empty() || name == "default" {
            return None;
        }
        self.document
            .profile_colors
            .get(name)
            .and_then(color::Authored::hue)
    }
    pub fn system() -> Result<Self, String> {
        Ok(Self::at_path(
            super::state_dir()?.join("fleet-preferences.json"),
        ))
    }
    pub fn at_path(path: PathBuf) -> Self {
        let mut this = Self {
            path: Some(path),
            ..Default::default()
        };
        if let Err(error) = this.observe() {
            this.error = Some(error);
        }
        this
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn explicit_profile_color(&self, name: &str) -> Option<String> {
        if name.is_empty() || name == "default" {
            return None;
        }
        self.document
            .profile_colors
            .get(name)
            .map(color::Authored::css)
    }
    pub fn profile_color(&self, name: &str) -> Result<Option<rune::Color>, &'static str> {
        if name.is_empty() || name == "default" {
            return Ok(None);
        }
        if let Some(value) = self.explicit_profile_color(name) {
            return color::parse(&value).map(Some).ok_or(
                "This CSS color is retained, but cannot yet be displayed by the native renderer",
            );
        }
        Ok(Some(color::hue_color(self.profile_hue(name).unwrap())))
    }
    pub fn profile_hue(&self, name: &str) -> Option<u16> {
        if name.is_empty() || name == "default" {
            return None;
        }
        Some(self.explicit_profile_hue(name).unwrap_or_else(|| {
            name.encode_utf16()
                .fold(0_u32, |h, c| h.wrapping_mul(31).wrapping_add(u32::from(c)))
                .wrapping_rem(360) as u16
        }))
    }
    pub fn profile_order(&self) -> &[String] {
        &self.document.profile_order
    }
    pub fn source_order(&self) -> &[String] {
        &self.document.source_order
    }
    pub fn group_alias(&self, key: &str) -> Option<&str> {
        self.document
            .groups
            .get(key)
            .and_then(|g| g.alias.as_deref())
    }
    pub fn collapsed(&self, key: &str) -> bool {
        self.document.groups.get(key).is_some_and(|g| g.collapsed)
    }
    pub fn set_color(&mut self, name: String, hue: Option<u16>) -> Result<(), String> {
        if hue.is_some_and(|h| h >= 360) {
            return Err("The profile hue is invalid".into());
        }
        self.enqueue(Intent::Color(
            name,
            hue.map(|h| color::Authored::Css(color::hue_css(h))),
        ))
    }
    pub fn set_profile_color(&mut self, name: String, value: Option<String>) -> Result<(), String> {
        self.enqueue(Intent::Color(name, value.map(color::Authored::Css)))
    }
    pub fn set_profile_order(&mut self, names: Vec<String>) -> Result<(), String> {
        self.enqueue(Intent::ProfileOrder(unique(names)))
    }
    pub fn set_source_order(&mut self, keys: Vec<String>) -> Result<(), String> {
        self.enqueue(Intent::SourceOrder(unique(keys)))
    }
    pub fn reorder_sources(&mut self, visible: Vec<String>) -> Result<(), String> {
        self.enqueue(Intent::ReorderSources(unique(visible)))
    }
    pub fn set_alias(&mut self, key: String, alias: Option<String>) -> Result<(), String> {
        self.enqueue(Intent::Alias(
            key,
            alias.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()),
        ))
    }
    pub fn set_collapsed(&mut self, key: String, value: bool) -> Result<(), String> {
        self.enqueue(Intent::Collapse(key, value))
    }
    fn enqueue(&mut self, intent: Intent) -> Result<(), String> {
        let mut projected = self.document.clone();
        intent.apply(&mut projected);
        projected.validate()?;
        self.pending.push_back(intent);
        self.flush_storage()
    }
    pub fn flush_storage(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let result = self.flush();
        match result {
            Ok(()) => {
                self.error = None;
                self.retry_at = None;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.clone());
                self.retry_at = Some(Instant::now() + Duration::from_secs(2));
                Err(error)
            }
        }
    }
    fn flush(&mut self) -> Result<(), String> {
        if let Some(error) = &self.unavailable {
            return Err(error.clone());
        }
        while let Some(intent) = self.pending.front() {
            let base = match &self.path {
                Some(path) => super::store::load(path, Document::validate)?,
                None => self.document.clone(),
            };
            let mut next = base.clone();
            let forced = intent.apply(&mut next);
            next.validate()?;
            self.document = match &self.path {
                Some(path) => super::store::save_patch_forced(
                    path,
                    &next,
                    &next,
                    &[forced],
                    Document::validate,
                )?,
                None => next,
            };
            self.pending.pop_front();
        }
        Ok(())
    }
    /// Observe only presentation metadata. This never asks the fleet worker to dial.
    pub fn observe(&mut self) -> Result<bool, String> {
        if let Some(error) = &self.unavailable {
            return Err(error.clone());
        }
        self.next_read = Instant::now() + Duration::from_secs(1);
        if !self.pending.is_empty() {
            self.flush_storage()?;
        }
        let Some(path) = &self.path else {
            return Ok(false);
        };
        match super::store::load(path, Document::validate) {
            Ok(document) => {
                let changed = self.document != document;
                self.document = document;
                self.error = None;
                Ok(changed)
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
    pub fn poll_storage(&mut self) -> Result<bool, String> {
        if !self.pending.is_empty() {
            if self.retry_at.is_some_and(|at| at <= Instant::now()) {
                self.flush_storage()?;
                return Ok(true);
            }
            return Ok(false);
        }
        if self.next_read <= Instant::now() {
            return self.observe();
        }
        Ok(false)
    }
    pub fn wake_after(&self) -> Option<Duration> {
        self.path.as_ref()?;
        let deadline = if self.pending.is_empty() {
            self.next_read
        } else {
            self.retry_at?
        };
        Some(deadline.saturating_duration_since(Instant::now()))
    }
}
fn unique(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    values
        .into_iter()
        .filter(|v| seen.insert(v.clone()))
        .collect()
}

pub fn ordered<'a, T>(
    items: impl IntoIterator<Item = &'a T>,
    order: &[String],
    name: impl Fn(&T) -> &str,
) -> Vec<&'a T> {
    let ranks: BTreeMap<_, _> = order
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let mut rows: Vec<_> = items.into_iter().collect();
    rows.sort_by(
        |a, b| match (ranks.get(name(a)).copied(), ranks.get(name(b)).copied()) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => name(a)
                .to_lowercase()
                .cmp(&name(b).to_lowercase())
                .then_with(|| name(a).cmp(name(b))),
        },
    );
    rows
}

/// Stable case-insensitive natural order for source labels and opaque source ids.
/// Native locale-specific accent collation remains a platform acceptance item.
pub fn compare_labels(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let left = left.to_lowercase();
    let right = right.to_lowercase();
    let mut a = left.chars().peekable();
    let mut b = right.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut x = String::new();
                let mut y = String::new();
                while a.peek().is_some_and(char::is_ascii_digit) {
                    x.push(a.next().unwrap());
                }
                while b.peek().is_some_and(char::is_ascii_digit) {
                    y.push(b.next().unwrap());
                }
                let x = x.trim_start_matches('0');
                let y = y.trim_start_matches('0');
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                a.next();
                b.next();
                let order = x.cmp(&y);
                if order != Ordering::Equal {
                    return order;
                }
            }
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}
