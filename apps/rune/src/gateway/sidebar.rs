//! Durable sidebar rows are different from a live runtime handle.
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
    pub fn key(&self) -> String {
        format!(
            "{}\0{}",
            self.profile(),
            self._lineage_root_id.as_deref().unwrap_or(&self.id)
        )
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
