use super::*;
impl Artifacts {
    pub fn load(&mut self) -> Vec<PageEffect> {
        if self.demo || self.loading || self.saving {
            return vec![];
        }
        self.rows.clear();
        self.seen.clear();
        self.page_keys.clear();
        self.sessions.clear();
        self.session_rows.clear();
        self.session_bytes = 0;
        self.cursor = 0;
        self.offset = 0;
        self.loading = true;
        self.error = None;
        self.failures = 0;
        self.failed_sessions.clear();
        self.selected = None;
        self.image_page = 0;
        self.file_page = 0;
        self.preview = None;
        self.thumbnails.clear();
        vec![(
            "artifacts.sessions".into(),
            Req::get("/api/profiles/sessions")
                .query("profile", "all")
                .query("limit", 30)
                .query("offset", 0)
                .query("min_messages", 1)
                .query("archived", "exclude")
                .query("order", "recent"),
        )]
    }
    fn next(&mut self) -> Vec<PageEffect> {
        if let Some(s) = self.sessions.get(self.cursor) {
            let id = s["id"].as_str().or(s["session_id"].as_str()).unwrap_or("");
            vec![(
                "artifacts.messages".into(),
                Req::get(format!("/api/sessions/{}/messages", segment(id)))
                    .profile(s["profile"].as_str().unwrap_or("default"))
                    .query("limit", 100)
                    .query("offset", self.offset)
                    .query("include_compacted", "true")
                    .query("order", "oldest")
                    .query("inline_images", "false"),
            )]
        } else {
            self.loading = false;
            self.rows.sort_by_key(|r| std::cmp::Reverse(r.timestamp));
            self.ensure_thumbnails()
        }
    }
    fn failed_current(&mut self, error: String) -> Vec<PageEffect> {
        if let Some(s) = self.sessions.get(self.cursor) {
            self.failed_sessions
                .retain(|(previous, _)| !same_session(previous, s));
            self.failed_sessions.push((s.clone(), error));
        }
        self.failures = self.failed_sessions.len();
        self.cursor += 1;
        self.offset = 0;
        self.session_bytes = 0;
        self.session_rows.clear();
        self.next()
    }
    fn finish_current(&mut self) {
        if let Some(s) = self.sessions.get(self.cursor) {
            self.failed_sessions
                .retain(|(previous, _)| !same_session(previous, s));
        }
        self.failures = self.failed_sessions.len();
        for a in self.session_rows.drain(..) {
            if self.seen.insert(a.key()) {
                self.rows.push(a);
            }
        }
        self.cursor += 1;
        self.offset = 0;
        self.session_bytes = 0;
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        if op.starts_with("artifacts.thumb.")
            || matches!(
                op,
                "artifacts.media" | "artifacts.native-open" | "artifacts.download"
            )
        {
            return self.media_reply(op, result);
        }
        match result {
            Err(e) => {
                if op == "artifacts.messages" {
                    return self.failed_current(e);
                }
                self.error = Some(e);
                self.loading = false;
            }
            Ok(v) => match op {
                "artifacts.sessions" => {
                    let Some(sessions) = v["sessions"].as_array() else {
                        self.error =
                            Some("The agent returned an invalid recent conversation list".into());
                        self.loading = false;
                        return vec![];
                    };
                    self.sessions = sessions.clone();
                    self.loading = true;
                    return self.next();
                }
                "artifacts.messages" => {
                    let Some(messages) = v["messages"].as_array() else {
                        return self.failed_current(
                            "The agent returned an invalid transcript page".into(),
                        );
                    };
                    let Some(s) = self.sessions.get(self.cursor).cloned() else {
                        return vec![];
                    };
                    let owner = s["id"].as_str().or(s["session_id"].as_str()).unwrap_or("");
                    let profile = s["profile"].as_str().unwrap_or("default");
                    if v["profile"].as_str().is_some_and(|p| p != profile)
                        || v["session_id"].as_str().is_some_and(|id| {
                            id != owner
                                && s["resolved_id"] != id
                                && s["_lineage_root_id"] != id
                                && !s["_lineage_ids"]
                                    .as_array()
                                    .is_some_and(|a| a.iter().any(|v| v == id))
                        })
                    {
                        return self.failed_current(
                            "Artifact history changed its profile or session owner".into(),
                        );
                    }
                    let encoded = serde_json::to_vec(messages).unwrap_or_default();
                    self.session_bytes = self.session_bytes.saturating_add(encoded.len());
                    if encoded.len() > 32_000_000 || self.session_bytes > 256_000_000 {
                        return self.failed_current("This transcript exceeds the desktop safe-load limit; export it from the agent before indexing.".into());
                    }
                    let key = {
                        use sha2::{Digest, Sha256};
                        format!("{profile}:{owner}:{:x}", Sha256::digest(&encoded))
                    };
                    if !messages.is_empty() && !self.page_keys.insert(key) {
                        return self.failed_current("The agent repeated a transcript page. Retry this conversation after refreshing the agent.".into());
                    }
                    for m in messages {
                        self.session_rows.extend(collect(&s, m));
                    }
                    if v["pagination"].is_object()
                        && !messages.is_empty()
                        && messages.len()
                            >= v["pagination"]["limit"].as_u64().unwrap_or(100).max(1) as usize
                    {
                        self.offset += messages.len();
                    } else {
                        self.finish_current();
                    }
                    return self.next();
                }
                _ => {}
            },
        }
        vec![]
    }
    pub fn act(&mut self, action: Action) -> Vec<PageEffect> {
        match action {
            Action::Refresh => return self.load(),
            Action::RetryFailed | Action::RetrySession(_) => {
                if self.loading || self.demo {
                    return vec![];
                }
                self.sessions = match action {
                    Action::RetrySession(i) => self
                        .failed_sessions
                        .get(i)
                        .map(|(s, _)| vec![s.clone()])
                        .unwrap_or_default(),
                    _ => self
                        .failed_sessions
                        .iter()
                        .map(|(s, _)| s.clone())
                        .collect(),
                };
                self.cursor = 0;
                self.offset = 0;
                self.session_rows.clear();
                self.session_bytes = 0;
                self.page_keys.clear();
                self.loading = true;
                return self.next();
            }
            Action::Filter(f) => {
                self.filter = f;
                self.page = 0;
                self.image_page = 0;
                self.file_page = 0;
                return self.ensure_thumbnails();
            }
            Action::SearchChanged => {
                self.page = 0;
                self.image_page = 0;
                self.file_page = 0;
                return self.ensure_thumbnails();
            }
            Action::ImagePage(p) => {
                self.image_page = p;
                return self.ensure_thumbnails();
            }
            Action::FilePage(p) => self.file_page = p,
            Action::Page(p) => self.page = p,
            Action::ClosePreview => {
                self.selected = None;
                self.preview = None;
                self.preview_error = None;
                self.preview_loading = false;
                self.download = None;
                self.data_url = None;
                self.preview_text = None;
            }
            Action::Zoom(z) => self.zoom = z.clamp(0.25, 4.0),
            Action::Select(i) => {
                if self.rows.get(i).is_none() {
                    return vec![];
                }
                self.selected = Some(i);
                self.preview = None;
                self.download = None;
                self.data_url = None;
                self.preview_text = None;
                self.preview_error = None;
                self.zoom = 1.0;
                if self.rows[i].kind == "image" {
                    return self.load_preview();
                }
            }
            Action::LoadFile => return self.load_preview(),
            Action::RetryThumbnail(i) => {
                if let Some(a) = self.rows.get(i) {
                    self.thumbnails.remove(&a.key());
                }
                return self.ensure_thumbnails();
            }
            Action::NativeOpen => return self.native_open(),
            Action::OpenChat(_) | Action::Inspect => {}
        }
        vec![]
    }
}
fn same_session(a: &Value, b: &Value) -> bool {
    a["profile"] == b["profile"]
        && a["id"].as_str().or(a["session_id"].as_str())
            == b["id"].as_str().or(b["session_id"].as_str())
}
