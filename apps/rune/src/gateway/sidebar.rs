//! Durable sidebar rows are different from a live runtime handle.
use crate::source::{SessionOwner, SessionRef, SourceId, SourceStamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarSession {
    #[serde(alias = "session_id")]
    pub id: String,
    pub title: Option<String>,
    pub root_title: Option<String>,
    pub profile: Option<String>,
    pub connection_id: Option<String>,
    /// Native fetch provenance. Agent JSON cannot grant a desktop source identity.
    #[serde(skip)]
    pub provenance: Option<SourceStamp>,
    pub source: Option<String>,
    pub started_at: f64,
    pub last_active: f64,
    pub ended_at: Option<f64>,
    pub pinned: Option<bool>,
    pub unread: Option<bool>,
    pub archived: bool,
    pub hidden: bool,
    pub is_internal_child: bool,
    pub message_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub actual_cost_usd: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub preview: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub git_repo_root: Option<String>,
    pub git_branch: Option<String>,
    pub _lineage_root_id: Option<String>,
    pub _lineage_ids: Option<Vec<String>>,
    pub _branched_from: Option<String>,
    pub parent_session_id: Option<String>,
    pub _reset_from: Option<String>,
    pub continuation_kind: Option<String>,
    pub scheduler_owned: Option<bool>,
    pub is_active: Option<bool>,
}

impl SidebarSession {
    pub fn title(&self) -> &str {
        self.title
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Untitled session")
    }
    pub fn profile(&self) -> &str {
        self.profile
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("default")
    }
    pub fn owner(&self) -> Option<SessionOwner> {
        self.provenance
            .clone()
            .map(|source| SessionOwner::new(source, self.profile()))
    }
    pub fn session_ref(&self) -> Option<SessionRef> {
        self.owner().map(|owner| owner.session(&self.id))
    }
    pub fn root_id(&self) -> &str {
        self._lineage_root_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or(&self.id)
    }
    pub fn alias_key(&self, id: &str) -> String {
        match self.owner() {
            Some(owner) => owner.row_key(id),
            None => serde_json::json!([null, self.profile(), id]).to_string(),
        }
    }
    pub fn fork_parent(&self) -> Option<&str> {
        if self
            ._reset_from
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
        {
            return None;
        }
        self._branched_from
            .as_deref()
            .or(self.parent_session_id.as_deref())
            .map(str::trim)
            .filter(|id| !id.is_empty())
    }
    pub fn key(&self) -> String {
        if let Some(owner) = self.owner() {
            return owner.row_key(self.root_id());
        }
        format!("{}\0{}", self.profile(), self.root_id())
    }
    pub fn recency(&self) -> f64 {
        if self.last_active > 0.0 {
            self.last_active
        } else {
            self.started_at
        }
    }
    pub fn tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
    pub fn cost(&self) -> f64 {
        self.actual_cost_usd
            .or(self.estimated_cost_usd)
            .unwrap_or(0.0)
    }
    pub fn aliases(&self) -> Vec<&str> {
        let mut ids = vec![self.id.as_str()];
        if let Some(root) = &self._lineage_root_id {
            ids.push(root);
        }
        if let Some(lineage) = &self._lineage_ids {
            ids.extend(lineage.iter().map(String::as_str));
        }
        ids
    }
}

pub fn parse_sessions(
    value: &Value,
    fallback_profile: &str,
) -> Result<Vec<SidebarSession>, String> {
    let rows = value
        .as_array()
        .or_else(|| value.get("sessions").and_then(Value::as_array))
        .ok_or("The agent returned an invalid session list")?;
    let mut sessions = Vec::new();
    for value in rows {
        let mut session: SidebarSession = serde_json::from_value(value.clone())
            .map_err(|_| "The agent returned an invalid session row")?;
        if session.id.is_empty() {
            return Err("A session has no durable identity".into());
        }
        if session.profile.is_none() {
            session.profile = Some(fallback_profile.into());
        }
        if !session.hidden && !session.is_internal_child {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

pub(crate) fn source_tag_matches(tag: Option<&str>, source: &SourceStamp) -> bool {
    match (
        &source.version().id,
        tag.map(str::trim).filter(|id| !id.is_empty()),
    ) {
        (_, None) => true,
        (SourceId::Saved(id), Some(tag)) => id == tag,
        _ => false,
    }
}

/// Attribute rows to the verified transport, never to an untrusted connection_id tag.
pub fn parse_sessions_owned(
    value: &Value,
    fallback_profile: &str,
    source: &SourceStamp,
) -> Result<Vec<SidebarSession>, String> {
    let mut rows = parse_sessions(value, fallback_profile)?;
    for row in &mut rows {
        if !source_tag_matches(row.connection_id.as_deref(), source) {
            return Err(
                "The session list names a source different from its verified transport".into(),
            );
        }
        row.provenance = Some(source.clone());
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rows_keep_numeric_time_owner_lineage_and_pins_beyond_the_window() {
        let value = json!({"sessions":[{"id":"tip","profile":"research","last_active":1234.5,"_lineage_root_id":"root","pinned":true,"source":"telegram"},{"id":"hidden","hidden":true}]});
        let rows = parse_sessions(&value, "default").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].recency(), 1234.5);
        assert_eq!(rows[0].profile(), "research");
        assert_eq!(rows[0].aliases(), ["tip", "root"]);
        assert_eq!(rows[0].pinned, Some(true));
    }
}
