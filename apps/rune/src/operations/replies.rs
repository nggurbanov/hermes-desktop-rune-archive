use super::*;
impl Operations {
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => return self.failed(op, e),
        };
        match op {
            "ops.status" => {
                self.status_loading = false;
                if !v["gateway_running"].is_boolean() {
                    self.system_error =
                        Some("The agent did not return an authenticated gateway status".into());
                    return vec![];
                }
                self.status = Some(v);
                self.system_error = None;
            }
            "ops.health-logs" => {
                self.health_logs_loading = false;
                if let Some(lines) = v["lines"].as_array() {
                    self.health_logs = lines
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|l| {
                            !l.trim().is_empty()
                                && ![
                                    "ws accepted",
                                    "ws closed",
                                    "ws response sent",
                                    "ws ping",
                                    "ws pong",
                                ]
                                .iter()
                                .any(|noise| l.to_lowercase().contains(noise))
                        })
                        .map(str::to_owned)
                        .collect();
                    self.health_logs_error = None;
                } else {
                    self.health_logs_error =
                        Some("The agent returned an invalid gateway activity tail".into());
                }
            }
            "ops.runtime" => {
                self.runtime = if v["ok"].is_boolean() { Some(v) } else { None };
            }
            "ops.setup" => {
                self.setup = if v["provider_configured"].is_boolean() {
                    Some(v)
                } else {
                    None
                };
            }
            "ops.update-check" => {
                self.update_loading = false;
                if !v["current_version"].is_string() && !v["update_available"].is_boolean() {
                    self.system_error =
                        Some("The agent returned an invalid update description".into());
                } else {
                    self.update = Some(v);
                }
            }
            "ops.curator" => {
                self.maintenance_loading = false;
                if !v["enabled"].is_boolean() {
                    self.maintenance_error = Some("Invalid curator status".into());
                } else {
                    self.curator = Some(v);
                }
            }
            "ops.memory" => {
                self.maintenance_loading = false;
                if !v["builtin_files"].is_object() {
                    self.maintenance_error = Some("Invalid memory layer status".into());
                } else {
                    self.memory = Some(v);
                }
            }
            "ops.curator-pause" => {
                self.curator_busy = false;
                let want = self.pending_curator.take();
                if v["ok"].as_bool() != Some(true) || v["paused"].as_bool() != want {
                    self.maintenance_error =
                        Some("The agent did not confirm the curator pause setting".into());
                    return vec![];
                }
                return vec![("ops.curator".into(), PageRequest::get("/api/curator"))];
            }
            "ops.memory-reset" => {
                self.memory_busy = false;
                if v["ok"].as_bool() != Some(true) || !v["deleted"].is_array() {
                    self.maintenance_error =
                        Some("The agent did not confirm which memory files were reset".into());
                    return vec![];
                }
                let deleted = v["deleted"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>();
                self.notice = Some(if deleted.is_empty() {
                    "No stored memory files were removed".into()
                } else {
                    format!("Reset {}", deleted.join(", "))
                });
                return vec![("ops.memory".into(), PageRequest::get("/api/memory"))];
            }
            "ops.debug-share" => {
                self.sharing = false;
                if v["ok"].as_bool() != Some(true) || !v["urls"].is_object() {
                    self.maintenance_error =
                        Some("The diagnostics service did not confirm a share result".into());
                    return vec![];
                }
                self.share = Some(v);
            }
            "ops.action-start" => {
                self.starting = false;
                let Some(kind) = self.pending_kind.take() else {
                    return vec![];
                };
                let Some(name) = v["name"].as_str().filter(|n| !n.is_empty()) else {
                    self.maintenance_error =
                        Some("The agent did not return an operation receipt".into());
                    return vec![];
                };
                if v["ok"].as_bool() != Some(true) {
                    self.maintenance_error = Some("The agent did not accept the operation".into());
                    return vec![];
                }
                let mut task = Task::new(kind);
                task.accepted = true;
                task.name = name.into();
                task.pid = v["pid"].as_u64();
                task.action_id = v["action_id"].as_str().map(str::to_owned);
                task.archive = v["archive"].as_str().map(str::to_owned);
                task.next = self.clock;
                self.task = Some(task);
                self.notice = Some(format!("{} accepted by the agent", kind.label()));
                return self.task_poll();
            }
            "ops.action-status" => return self.action_reply(v),
            "ops.export-page" => return self.export_reply(v),
            "ops.backup-download" => {
                self.downloading = false;
                if v["ok"].as_bool() == Some(true) && v["downloaded"].as_bool() == Some(true) {
                    self.notice = Some("Backup downloaded to this device".into());
                } else {
                    self.export_error = Some("The backup download was not confirmed".into());
                }
            }
            _ if op.starts_with("ops.logs.") => {
                if op != self.log_operation() {
                    return vec![];
                }
                self.logs_loading = false;
                let Some(lines) = v["lines"].as_array() else {
                    self.logs_error = Some("The agent returned an invalid log tail".into());
                    return vec![];
                };
                self.logs = lines
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                self.logs_error = None;
            }
            _ if op.starts_with("ops.usage.") => {
                if op != format!("ops.usage.{}", self.period) {
                    return vec![];
                }
                self.usage_loading = false;
                if !v["totals"].is_object() || !v["daily"].is_array() {
                    self.usage_error = Some("The agent returned invalid usage analytics".into());
                    return vec![];
                }
                self.usage = Some(v);
                self.usage_error = None;
            }
            _ => {}
        }
        vec![]
    }
    fn failed(&mut self, op: &str, error: String) -> Vec<PageEffect> {
        match op {
            "ops.status" => {
                self.status_loading = false;
                self.system_error = Some(error);
            }
            "ops.health-logs" => {
                self.health_logs_loading = false;
                self.health_logs_error = Some(error);
                self.next_health_logs = self.clock + 3.0;
            }
            "ops.setup" => {
                self.setup = None;
                self.readiness_error = Some(error);
            }
            "ops.runtime" => {
                self.runtime = None;
                self.readiness_error = Some(error);
            }
            "ops.update-check" => {
                self.update_loading = false;
                self.system_error = Some(error);
            }
            "ops.action-start" => {
                self.starting = false;
                if let Some(kind) = self.pending_kind.take() {
                    let mut t = Task::new(kind);
                    t.running = false;
                    t.error = Some(format!(
                        "The start response was lost or refused: {error}. Check the recorded action before starting it again."
                    ));
                    self.task = Some(t);
                } else {
                    self.maintenance_error = Some(error);
                }
            }
            "ops.action-status" => {
                let Some(t) = &mut self.task else {
                    return vec![];
                };
                t.polling = false;
                t.polls += 1;
                t.next = self.clock + 1.2;
                if t.kind == Kind::Restart && t.polls < 18 {
                    return vec![];
                }
                t.running = false;
                t.error = Some(if t.kind == Kind::Restart && !t.answered {
                    format!("No replacement gateway answered during recovery. {error}")
                } else {
                    format!(
                        "Status monitoring stopped: {error}. The operation may still be running on the agent."
                    )
                });
                if t.kind == Kind::Restart {
                    self.reconnect_followthrough = true;
                }
            }
            "ops.curator-pause" => {
                self.curator_busy = false;
                self.pending_curator = None;
                self.maintenance_error = Some(error);
            }
            "ops.memory-reset" => {
                self.memory_busy = false;
                self.maintenance_error = Some(error);
            }
            "ops.debug-share" => {
                self.sharing = false;
                self.maintenance_error = Some(error);
            }
            "ops.memory" | "ops.curator" => {
                self.maintenance_loading = false;
                self.maintenance_error = Some(error);
            }
            "ops.export-page" => {
                self.exporting = false;
                self.export = None;
                self.export_error = Some(error);
            }
            "ops.backup-download" => {
                self.downloading = false;
                self.export_error = Some(error);
            }
            _ if op.starts_with("ops.logs.") => {
                if op == self.log_operation() {
                    self.logs_loading = false;
                    self.logs_error = Some(error);
                    self.next_logs = self.clock + 3.0;
                }
            }
            _ if op.starts_with("ops.usage.") => {
                if op == format!("ops.usage.{}", self.period) {
                    self.usage_loading = false;
                    self.usage_error = Some(error);
                }
            }
            _ => {}
        }
        vec![]
    }
    fn action_reply(&mut self, v: Value) -> Vec<PageEffect> {
        let Some(t) = &mut self.task else {
            return vec![];
        };
        t.polling = false;
        t.polls += 1;
        t.next = self.clock + 1.2;
        if v["name"].as_str().is_some_and(|n| n != t.name) || !v["running"].is_boolean() {
            t.running = false;
            t.error = Some("The agent returned an invalid operation status".into());
            return vec![];
        }
        if t.kind == Kind::Update
            && t.action_id
                .as_ref()
                .is_some_and(|id| v["action_id"].as_str().is_some_and(|n| n != id))
        {
            t.running = false;
            t.error=Some("Another update replaced this action receipt. Refresh the agent update state before proceeding.".into());
            return vec![];
        }
        if t.kind != Kind::Restart
            && t.pid.is_some()
            && v["pid"].as_u64().is_some()
            && t.pid != v["pid"].as_u64()
        {
            t.running = false;
            t.error=Some("Another process replaced this action receipt. Its outcome cannot confirm this request.".into());
            return vec![];
        }
        t.answered = true;
        t.lines = v["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        t.running = v["running"].as_bool() == Some(true);
        t.exit_code = v["exit_code"].as_i64();
        if let Some(receipt) = v["receipt"].as_object() {
            t.receipt = Some(Value::Object(receipt.clone()));
        }
        if !t.accepted {
            t.error=Some("A recorded action was found, but it cannot confirm the lost start response. Its log is shown for inspection; no completion is claimed for this request.".into());
            if t.kind == Kind::Restart && !t.running {
                self.reconnect_followthrough = true;
            }
            return vec![];
        }
        let receipt = t
            .receipt
            .as_ref()
            .and_then(|r| r["outcome"].as_str())
            .unwrap_or("");
        if t.kind == Kind::Update && matches!(receipt, "success" | "partial" | "failed" | "refused")
        {
            t.running = false;
            t.settled = true;
            if receipt == "success" {
                t.exit_code = Some(0);
                self.notice = Some("The external Hermes agent update completed".into());
            } else if receipt == "partial" {
                t.error=Some("The agent update applied with unfinished follow-up steps. Review its receipt before retrying.".into());
            } else {
                t.error = Some(format!("Agent update {receipt}; review its operation log"));
            }
            return self.system_load();
        }
        if !t.running && t.exit_code.is_some() {
            t.settled = true;
            if t.exit_code == Some(0) {
                self.notice = Some(format!("{} completed", t.kind.label()));
            } else {
                t.error = Some(format!(
                    "{} failed with exit code {}",
                    t.kind.label(),
                    t.exit_code.unwrap()
                ));
            }
            if t.kind == Kind::Restart {
                self.reconnect_followthrough = true;
            }
            return if self.section == "maintenance" {
                self.maintenance_load()
            } else {
                self.health_load()
            };
        }
        if t.kind == Kind::Restart {
            if t.polls >= 18 {
                t.running = false;
                t.settled = true;
                t.recovered = true;
                self.reconnect_followthrough = true;
                self.notice=Some("The gateway answered during recovery. Its previous action registry may have been replaced; review gateway health.".into());
                return self.health_load();
            }
            t.running = true;
        } else if !t.running {
            t.error=Some("The agent has no recorded exit outcome for this action. Check status again before retrying it.".into());
        } else if t.polls >= 240 {
            t.running = false;
            t.error = Some(
                "The operation is still running on the agent. Resume status monitoring when ready."
                    .into(),
            );
        }
        vec![]
    }
    fn export_reply(&mut self, v: Value) -> Vec<PageEffect> {
        let Some(e) = &mut self.export else {
            return vec![];
        };
        if v["profile"].as_str().is_some_and(|p| p != e.row.profile()) {
            self.export_error = Some("The transcript response belongs to another profile".into());
            self.exporting = false;
            self.export = None;
            return vec![];
        }
        let Some(rows) = v["messages"].as_array() else {
            self.export_error = Some("The agent returned an invalid transcript page".into());
            self.exporting = false;
            self.export = None;
            return vec![];
        };
        let page = serde_json::to_string(rows).unwrap();
        e.bytes = e.bytes.saturating_add(page.len());
        e.pages += 1;
        if e.bytes > 32_000_000 || e.pages > 20_000 || !rows.is_empty() && !e.seen.insert(page) {
            self.export_error=Some("The transcript exceeded the safe export limit or the agent repeated a page. No incomplete export was saved.".into());
            self.exporting = false;
            self.export = None;
            return vec![];
        }
        if let Some(id) = v["session_id"].as_str() {
            e.resolved = id.into();
        }
        e.messages.extend(rows.iter().cloned());
        e.offset += rows.len();
        if v["pagination"].is_object()
            && !rows.is_empty()
            && rows.len()
                >= v["pagination"]["limit"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .unwrap_or(500) as usize
        {
            return self.export_page();
        }
        let e = self.export.take().unwrap();
        let value = json!({"exported_at":chrono::Utc::now().to_rfc3339(),"session_id":e.resolved,"title":e.row.title(),"session":e.row,"message_count":e.messages.len(),"messages":e.messages});
        self.save_file(e.path, files::Payload::Json(value));
        vec![]
    }
}
