use super::*;
fn data(v: &Value) -> Result<(&str, Vec<u8>), String> {
    let url = v
        .as_str()
        .or(v["dataUrl"].as_str())
        .or(v["data_url"].as_str())
        .or(v["data"].as_str())
        .ok_or("The agent returned no media data")?;
    if url.len() > 48 * 1024 * 1024 {
        return Err("The preview exceeds the 32 MiB safe media limit. Save or open the original file instead.".into());
    }
    let (meta, encoded) = url
        .split_once(',')
        .ok_or("The agent returned invalid media data")?;
    if !meta.starts_with("data:") {
        return Err("The agent returned an unsupported media address".into());
    }
    let mime = meta.trim_start_matches("data:").trim_end_matches(";base64");
    let bytes = if meta.ends_with(";base64") {
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "The media encoding is invalid")?
    } else {
        decode(encoded).into_bytes()
    };
    Ok((mime, bytes))
}
impl Artifacts {
    pub(super) fn ensure_thumbnails(&mut self) -> Vec<PageEffect> {
        if self.demo {
            return vec![];
        }
        let visible = self.visible();
        let images: Vec<_> = visible
            .into_iter()
            .filter(|&i| self.rows[i].kind == "image")
            .collect();
        let count = images.len().div_ceil(24).max(1);
        self.image_page = self.image_page.min(count - 1);
        let mut effects = vec![];
        for i in images.into_iter().skip(self.image_page * 24).take(24) {
            let a = self.rows[i].clone();
            let key = a.key();
            if self.thumbnails.contains_key(&key) {
                continue;
            }
            self.thumbnails.insert(
                key.clone(),
                Thumbnail {
                    image: None,
                    error: None,
                    loading: true,
                },
            );
            let op = format!("artifacts.thumb.{key}");
            if a.value.starts_with("data:") {
                self.media_reply(&op, Ok(json!({"data_url":a.value})));
            } else {
                effects.push((op, self.image_request(&a)));
            }
        }
        effects
    }
    fn image_request(&self, a: &Artifact) -> Req {
        if a.value.starts_with("http://") || a.value.starts_with("https://") {
            Req::get("/api/media/proxy")
                .query("url", &a.value)
                .profile(&a.profile)
        } else {
            Req::get("/api/fs/read-data-url")
                .query("path", &a.value)
                .query("session_id", &a.session)
                .profile(&a.profile)
        }
    }
    pub(super) fn load_preview(&mut self) -> Vec<PageEffect> {
        let Some(a) = self.selected.and_then(|i| self.rows.get(i)).cloned() else {
            return vec![];
        };
        if self.demo {
            self.preview = self.thumbnails.get(&a.key()).and_then(|t| t.image.clone());
            return vec![];
        }
        self.preview_loading = true;
        self.preview_error = None;
        self.preview = None;
        self.preview_text = None;
        self.download = None;
        if a.value.starts_with("data:") {
            return self.media_reply("artifacts.media", Ok(json!({"data_url":a.value})));
        }
        if a.kind == "link" {
            self.preview_loading = false;
            return vec![];
        }
        vec![("artifacts.media".into(), self.image_request(&a))]
    }
    pub(super) fn native_open(&mut self) -> Vec<PageEffect> {
        if self.saving {
            return vec![];
        }
        let Some(a) = self.selected.and_then(|i| self.rows.get(i)).cloned() else {
            return vec![];
        };
        if a.value.starts_with("https://") || a.value.starts_with("http://") {
            self.open_url = Some(a.value);
            return vec![];
        }
        let path = match self.temp.reserve(&a.label) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(e);
                return vec![];
            }
        };
        self.saving = true;
        if a.value.starts_with("data:") {
            match data(&json!({"data_url":a.value})) {
                Ok((_, bytes)) => self.write_bytes(path, bytes, true),
                Err(e) => {
                    self.saving = false;
                    self.error = Some(e);
                }
            }
            return vec![];
        }
        self.open_pending = Some(path.clone());
        vec![(
            "artifacts.native-open".into(),
            Req::get("/api/fs/download")
                .profile(&a.profile)
                .query("path", &a.value)
                .query("session_id", &a.session)
                .download_to(path)
                .timeout_secs(3600),
        )]
    }
    pub(super) fn media_reply(
        &mut self,
        op: &str,
        result: Result<Value, String>,
    ) -> Vec<PageEffect> {
        if op == "artifacts.download" {
            self.saving = false;
            match result {
                Ok(v) if v["ok"] == true && v["downloaded"] == true => {
                    self.notice = Some("File saved to the selected destination".into())
                }
                Ok(_) => self.error = Some("The file download was not confirmed".into()),
                Err(e) => self.error = Some(e),
            }
            return vec![];
        }
        if op == "artifacts.native-open" {
            self.saving = false;
            let path = self.open_pending.take();
            match result {
                Ok(v) if v["ok"] == true && v["downloaded"] == true => {
                    if let Some(path) = path {
                        if self.temp.contains(&path) {
                            self.open_url =
                                crate::media::local_file_url(&path.to_string_lossy()).ok();
                        }
                    }
                }
                Ok(_) => self.error = Some("The native media copy was not confirmed".into()),
                Err(e) => self.error = Some(e),
            }
            return vec![];
        }
        if let Some(key) = op.strip_prefix("artifacts.thumb.") {
            let Some(thumb) = self.thumbnails.get_mut(key) else {
                return vec![];
            };
            thumb.loading = false;
            match result.and_then(|v| data(&v).map(|(mime, bytes)| (mime.to_owned(), bytes))) {
                Ok((mime, bytes)) if mime.starts_with("image/") => {
                    if let Some(cache) = &self.cache {
                        thumb.image =
                            Some(cache.load_mime(Arc::<[u8]>::from(bytes), &mime, 360.0, 1.0));
                    }
                }
                Ok(_) => thumb.error = Some("This artifact no longer contains an image".into()),
                Err(e) => thumb.error = Some(e),
            }
            return vec![];
        }
        self.preview_loading = false;
        match result.and_then(|v| data(&v).map(|(mime, bytes)| (mime.to_owned(), bytes))) {
            Err(e) => self.preview_error = Some(e),
            Ok((mime, bytes)) => {
                self.mime = mime.clone();
                if let Some(a) = self.selected.and_then(|i| self.rows.get(i)) {
                    self.download = Some((a.label.clone(), bytes.clone()));
                }
                if mime.starts_with("image/") {
                    if let Some(cache) = &self.cache {
                        self.preview =
                            Some(cache.load_mime(Arc::<[u8]>::from(bytes), &mime, 1600.0, 1.0));
                    }
                } else if let Ok(text) = std::str::from_utf8(&bytes) {
                    if !text.contains('\0') {
                        let truncated = text.len() > 1_000_000;
                        let body = text.chars().take(1_000_000).collect::<String>();
                        self.preview_text = Some(if truncated {
                            format!(
                                "{body}\n\n[Preview truncated. Save the original file for complete content.]"
                            )
                        } else {
                            body
                        });
                    }
                }
            }
        }
        vec![]
    }
    pub fn write_bytes(&mut self, path: std::path::PathBuf, bytes: Vec<u8>, open: bool) {
        match self
            .writer
            .as_mut()
            .ok_or("The native file writer is unavailable".into())
            .and_then(|writer| writer.write(path, bytes))
        {
            Ok(id) => {
                self.writes.insert(id, open);
                self.saving = true;
            }
            Err(e) => {
                self.saving = false;
                self.error = Some(e);
            }
        }
    }
    pub fn poll(&mut self) -> bool {
        let results = self.writer.as_mut().map(|w| w.poll()).unwrap_or_default();
        let redraw = !results.is_empty();
        for (id, result) in results {
            let Some(open) = self.writes.remove(&id) else {
                continue;
            };
            self.saving = !self.writes.is_empty();
            match result {
                Ok(path) => {
                    if open && self.temp.contains(&path) {
                        self.open_url = crate::media::local_file_url(&path.to_string_lossy()).ok();
                    } else {
                        self.notice = Some("File saved to the selected destination".into());
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
        redraw
    }
    pub fn take_open_url(&mut self) -> Option<String> {
        self.open_url.take()
    }
    pub fn blocks_owner_change(&self) -> bool {
        self.saving
    }
}
