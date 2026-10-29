//! MCP log reads keep the selected server/source generation, never the ambient profile.
use super::*;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogSource {
    #[default]
    Stdio,
    Agent,
}
#[derive(Default)]
pub struct McpLogs {
    pub opened: bool,
    pub source: LogSource,
    pub lines: Vec<String>,
    pub error: Option<String>,
    generation: u64,
}
impl McpLogs {
    pub fn reset(&mut self) {
        self.opened = false;
        self.lines.clear();
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
    }
}
impl Capabilities {
    pub fn mcp_logs_loading(&self) -> bool {
        self.pending.contains(&format!(
            "capabilities.mcp-logs.{}",
            self.mcp_logs.generation
        ))
    }

    pub(super) fn show_mcp_logs(&mut self, source: LogSource) -> Vec<PageEffect> {
        self.mcp_logs.reset();
        self.mcp_logs.opened = true;
        self.mcp_logs.source = source;
        self.read_mcp_logs()
    }
    pub(super) fn read_mcp_logs(&mut self) -> Vec<PageEffect> {
        let Some(Selection::Mcp(name)) = &self.selection else {
            return vec![];
        };
        if !self.mcp_logs.opened {
            return vec![];
        }
        if self.demo {
            self.mcp_logs.lines = vec![format!(
                "Synthetic output from {name} · no live agent log was read."
            )];
            return vec![];
        }
        let key = format!("mcp-logs.{}", self.mcp_logs.generation);
        if self.pending.contains(&format!("capabilities.{key}")) {
            return vec![];
        }
        let request = match self.mcp_logs.source {
            LogSource::Stdio => PageRequest::get("/api/logs")
                .query("file", "mcp")
                .query("lines", 500),
            LogSource::Agent => PageRequest::get("/api/logs")
                .query("file", "agent")
                .query("lines", 300)
                .query("search", name),
        };
        vec![self.request(&key, request)]
    }
    pub(super) fn apply_mcp_logs(
        &mut self,
        op: &str,
        result: Result<Value, String>,
    ) -> Vec<PageEffect> {
        if op != format!("capabilities.mcp-logs.{}", self.mcp_logs.generation)
            || !self.mcp_logs.opened
        {
            return vec![];
        }
        let Some(Selection::Mcp(name)) = &self.selection else {
            return vec![];
        };
        match result {
            Err(error) => self.mcp_logs.error = Some(error),
            Ok(value) => {
                let Some(lines) = value["lines"]
                    .as_array()
                    .filter(|lines| lines.iter().all(Value::is_string))
                else {
                    self.mcp_logs.error = Some("The agent returned invalid MCP log lines".into());
                    return vec![];
                };
                let lines: Vec<_> = lines
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                self.mcp_logs.lines = if self.mcp_logs.source == LogSource::Stdio {
                    filter_stdio(&lines, name)
                } else {
                    lines
                };
                self.mcp_logs.error = None;
            }
        }
        vec![]
    }
}
fn filter_stdio(lines: &[String], server: &str) -> Vec<String> {
    let marker = regex::Regex::new(
        r"^(?:\d{4}-\d{2}-\d{2} [\d:,]+ )?===== (?:\[.*\] )?starting MCP server '(.+)' =====$",
    )
    .unwrap();
    let mut inside = false;
    lines
        .iter()
        .filter(|line| {
            if let Some(captured) = marker.captures(line.trim()) {
                inside = captured.get(1).is_some_and(|name| name.as_str() == server);
            }
            inside
        })
        .cloned()
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_output_never_shows_another_server_and_late_source_results_are_fenced() {
        let mut state = Capabilities::new();
        state.profile = Some("research".into());
        state.selection = Some(Selection::Mcp("calendar".into()));
        let requests = state.show_mcp_logs(LogSource::Stdio);
        assert_eq!(requests[0].1.profile.as_deref(), Some("research"));
        state.apply(&requests[0].0,Ok(json!({"lines":["unrelated","2026-10-29 16:00:00,123 ===== starting MCP server 'calendar' =====","calendar output","===== [old] starting MCP server 'mail' =====","private mail output"]})));
        assert_eq!(state.mcp_logs.lines.len(), 2);
        assert_eq!(state.mcp_logs.lines[1], "calendar output");
        let agent = state.show_mcp_logs(LogSource::Agent);
        assert!(
            agent[0]
                .1
                .query
                .contains(&("search".into(), "calendar".into()))
        );
        state.apply(&requests[0].0, Ok(json!({"lines":["old result"]})));
        assert!(state.mcp_logs.lines.is_empty());
        state.apply(&agent[0].0, Ok(json!({"lines":["current result"]})));
        assert_eq!(state.mcp_logs.lines, vec!["current result"]);
    }
}
