use super::*;
use serde_json::Value;
impl Terminal {
    fn agent_owner(&self, scope: &DraftScope) -> Option<String> {
        self.session_owners.get(scope).cloned()
    }
    pub fn agent_event(&mut self, scope: &DraftScope, kind: &str, payload: &Value) {
        let Some(key) = self.agent_owner(scope) else {
            return;
        };
        let Some(id) = payload
            .get("process_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        if kind == "terminal.close" {
            let w = self.workspaces.get_mut(&key).unwrap();
            w.closed_processes.insert(id.into());
            let removed = w
                .tabs
                .iter()
                .filter(|t| t.process.as_deref() == Some(id))
                .map(|t| t.id.clone())
                .collect::<Vec<_>>();
            w.tabs.retain(|t| t.process.as_deref() != Some(id));
            if w.active.as_ref().is_some_and(|id| removed.contains(id)) {
                w.active = w.tabs.first().map(|t| t.id.clone());
            }
            return;
        }
        if kind != "agent.terminal.output" {
            return;
        }
        let chunk = payload.get("chunk").and_then(Value::as_str).unwrap_or("");
        let Some(tab) = self.agent_tab(&key, id, "") else {
            return;
        };
        tab.grid.process(chunk.as_bytes(), true);
        tab.agent_output.push_str(chunk);
        if tab.agent_output.len() > 256000 {
            let mut cut = tab.agent_output.len() - 256000;
            while !tab.agent_output.is_char_boundary(cut) {
                cut += 1;
            }
            tab.agent_output.drain(..cut);
        }
    }
    fn agent_tab(&mut self, key: &str, id: &str, command: &str) -> Option<&mut Tab> {
        let w = self.workspaces.get_mut(key).unwrap();
        if w.closed_processes.contains(id) {
            return None;
        }
        let index =
            if let Some(index) = w.tabs.iter().position(|t| t.process.as_deref() == Some(id)) {
                index
            } else {
                let mut tab = Tab::new(
                    uuid::Uuid::new_v4().to_string(),
                    Source::Agent,
                    String::new(),
                );
                tab.process = Some(id.into());
                tab.title = if command.is_empty() {
                    format!("Agent · {id}")
                } else {
                    command.chars().take(55).collect()
                };
                tab.status = Status::Open;
                if !command.is_empty() {
                    tab.grid
                        .process(format!("$ {command}\r\n").as_bytes(), true);
                }
                w.tabs.push(tab);
                if w.active.is_none() {
                    w.active = w.tabs.last().map(|t| t.id.clone());
                }
                w.tabs.len() - 1
            };
        Some(&mut w.tabs[index])
    }
    pub fn open_process(&mut self, scope: &DraftScope, id: &str) {
        let Some(key) = self.agent_owner(scope) else {
            return;
        };
        let row = self
            .workspaces
            .get(&key)
            .and_then(|w| w.process_snapshots.get(id))
            .cloned();
        self.workspaces
            .get_mut(&key)
            .unwrap()
            .closed_processes
            .remove(id);
        if let Some(row) = row {
            self.sync_processes(scope, &serde_json::json!({"processes":[row]}));
        } else {
            self.agent_tab(&key, id, "");
        }
        let w = self.workspaces.get_mut(&key).unwrap();
        w.active = w
            .tabs
            .iter()
            .find(|tab| tab.process.as_deref() == Some(id))
            .map(|tab| tab.id.clone());
    }
    pub fn sync_processes(&mut self, scope: &DraftScope, value: &Value) {
        let Some(key) = self.agent_owner(scope) else {
            return;
        };
        let Some(rows) = value.get("processes").and_then(Value::as_array) else {
            return;
        };
        for row in rows {
            let Some(id) = row.get("session_id").and_then(Value::as_str) else {
                continue;
            };
            let command = row.get("command").and_then(Value::as_str).unwrap_or("");
            let output = row
                .get("output_tail")
                .or_else(|| row.get("output_preview"))
                .and_then(Value::as_str)
                .unwrap_or("");
            self.workspaces
                .get_mut(&key)
                .unwrap()
                .process_snapshots
                .insert(id.into(), row.clone());
            let Some(tab) = self.agent_tab(&key, id, command) else {
                continue;
            };
            if output != tab.last_snapshot {
                let current = tab.agent_output.clone();
                if output.ends_with(&current) && !current.is_empty() {
                } else if output.starts_with(&tab.last_snapshot) {
                    tab.grid
                        .process(output[tab.last_snapshot.len()..].as_bytes(), true);
                } else {
                    let (r, c) = tab.grid.screen().size();
                    tab.grid = grid::Grid::new(r, c);
                    if !command.is_empty() {
                        tab.grid
                            .process(format!("$ {command}\r\n").as_bytes(), true);
                    }
                    tab.grid.process(output.as_bytes(), true);
                }
                tab.last_snapshot = output.into();
                tab.agent_output = output.into();
            }
            if row
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|s| matches!(s, "completed" | "exited" | "failed" | "killed"))
            {
                tab.status = Status::Exited;
            }
        }
    }
}
