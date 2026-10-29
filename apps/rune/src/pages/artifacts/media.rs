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
            if let Some(data) = self
                .safe_fixture
                .as_ref()
                .and_then(|data| data.get(&key))
                .cloned()
            {
                self.media_reply(&op, Ok(json!({"data_url":data})));
            } else if a.value.starts_with("data:") {
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
                .query("session_id", &a.session)
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
        self.cancel_detail_image();
        self.preview_loading = true;
        self.preview_error = None;
        self.preview = None;
        self.preview_text = None;
        self.download = None;
        if let Some(data) = self
            .safe_fixture
            .as_ref()
            .and_then(|data| data.get(&a.key()))
            .cloned()
        {
            return self.media_reply("artifacts.media", Ok(json!({"data_url":data})));
        }
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
            if !self.thumbnails.contains_key(key) {
                return vec![];
            }
            match result.and_then(|v| data(&v).map(|(mime, bytes)| (mime.to_owned(), bytes))) {
                Ok((mime, bytes)) => {
                    if let Err(error) = self.queue_image(key.to_owned(), false, &mime, bytes, 360) {
                        if let Some(thumb) = self.thumbnails.get_mut(key) {
                            thumb.loading = false;
                            thumb.error = Some(error);
                        }
                    }
                }
                Err(error) => {
                    if let Some(thumb) = self.thumbnails.get_mut(key) {
                        thumb.loading = false;
                        thumb.error = Some(error);
                    }
                }
            }
            return vec![];
        }
        self.preview_loading = false;
        match result.and_then(|v| data(&v).map(|(mime, bytes)| (mime.to_owned(), bytes))) {
            Err(error) => self.preview_error = Some(error),
            Ok((mime, bytes)) => {
                self.mime = mime.clone();
                if let Some(a) = self.selected.and_then(|i| self.rows.get(i)).cloned() {
                    self.download = Some((a.label.clone(), bytes.clone()));
                    if mime.starts_with("image/") || a.kind == "image" {
                        match self.queue_image(a.key(), true, &mime, bytes, 1600) {
                            Ok(()) => self.preview_loading = true,
                            Err(error) => self.preview_error = Some(error),
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
        }
        vec![]
    }
    pub(super) fn cancel_images(&mut self) {
        if let Some(images) = &mut self.safe_images {
            images.cancel_all();
        }
        self.image_jobs.clear();
        self.image_latest.clear();
    }
    pub(super) fn cancel_detail_image(&mut self) {
        let ids: Vec<_> = self
            .image_jobs
            .iter()
            .filter(|(_, (_, detail))| *detail)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(images) = &mut self.safe_images {
                images.cancel(id);
            }
            self.image_jobs.remove(&id);
        }
        self.image_latest
            .retain(|key, _| !key.starts_with("detail:"));
    }
    fn queue_image(
        &mut self,
        key: String,
        detail: bool,
        mime: &str,
        bytes: Vec<u8>,
        edge: u32,
    ) -> Result<(), String> {
        let target = format!("{}:{key}", if detail { "detail" } else { "thumb" });
        if let Some(old) = self.image_latest.remove(&target) {
            if let Some(images) = &mut self.safe_images {
                images.cancel(old);
            }
            self.image_jobs.remove(&old);
        }
        let id = self
            .safe_images
            .as_mut()
            .ok_or("The image decoder is unavailable")?
            .request(mime, Arc::from(bytes), edge)?;
        self.image_latest.insert(target, id);
        self.image_jobs.insert(id, (key, detail));
        Ok(())
    }
    fn poll_images(&mut self) -> bool {
        let results = self
            .safe_images
            .as_mut()
            .map(|images| images.poll())
            .unwrap_or_default();
        let mut changed = false;
        for decoded in results {
            let Some((key, detail)) = self.image_jobs.remove(&decoded.id) else {
                continue;
            };
            let target = format!("{}:{key}", if detail { "detail" } else { "thumb" });
            if self.image_latest.get(&target) != Some(&decoded.id) {
                continue;
            }
            self.image_latest.remove(&target);
            if detail {
                if !self
                    .selected
                    .and_then(|i| self.rows.get(i))
                    .is_some_and(|a| a.key() == key)
                {
                    continue;
                }
                self.preview_loading = false;
                match decoded.result {
                    Ok(image) => self.preview = Some(image),
                    Err(error) => self.preview_error = Some(error),
                }
                changed = true;
            } else if self.rows.iter().any(|a| a.key() == key) {
                if let Some(thumb) = self.thumbnails.get_mut(&key) {
                    thumb.loading = false;
                    match decoded.result {
                        Ok(image) => thumb.image = Some(image),
                        Err(error) => thumb.error = Some(error),
                    }
                    changed = true;
                }
            }
        }
        changed
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
        let mut redraw = self.poll_images() || !results.is_empty();
        if self.safe_fixture.is_some()
            && self.rows.len() == 24
            && self.thumbnails.len() == 24
            && self.thumbnails.values().all(|thumb| {
                thumb
                    .image
                    .as_ref()
                    .is_some_and(|image| image.size().is_some())
            })
            && self.notice.as_deref() != Some("Fixture · 24 images ready")
        {
            self.notice = Some("Fixture · 24 images ready".into());
            redraw = true;
        }
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

#[cfg(test)]
mod image_tests {
    use super::*;
    fn artifact(session: &str) -> Artifact {
        Artifact {
            kind: "image".into(),
            value: format!("/agent/{session}.png"),
            label: format!("{session}.png"),
            session: session.into(),
            profile: "research".into(),
            title: session.into(),
            timestamp: 0,
        }
    }
    fn data_url(w: u32, h: u32, color: &str) -> String {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="{color}"/></svg>"#
        );
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(svg)
        )
    }
    fn settle(state: &mut Artifacts) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.preview_loading && std::time::Instant::now() < deadline {
            state.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    #[test]
    fn production_safe_image_fixture_decodes_all_twenty_four_and_keeps_detail_original() {
        let mut state = Artifacts::new();
        state.fixture_safe_images();
        assert!(state.thumbnails.is_empty());
        state.init(Arc::new(|| {}));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while state.notice.as_deref() != Some("Fixture · 24 images ready")
            && std::time::Instant::now() < deadline
        {
            state.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(state.thumbnails.len(), 24);
        assert_eq!(state.notice.as_deref(), Some("Fixture · 24 images ready"));
        assert!(state.thumbnails.values().all(|t| {
            t.error.is_none()
                && t.image
                    .as_ref()
                    .is_some_and(|i| i.size() == Some([360, 135]))
        }));
        state.act(Action::Select(0));
        settle(&mut state);
        assert_eq!(state.preview.as_ref().unwrap().wait().unwrap(), [1600, 600]);
        let expected = state
            .safe_fixture
            .as_ref()
            .unwrap()
            .get(&state.rows[0].key())
            .unwrap();
        assert_eq!(
            state.download.as_ref().unwrap().1,
            data(&json!({"data_url":expected})).unwrap().1
        );
    }
    #[test]
    fn async_detail_and_thumbnails_use_current_request_artifact_owner_and_original_download() {
        let mut state = Artifacts::new();
        state.init(Arc::new(|| {}));
        state.rows = vec![artifact("one"), artifact("two")];
        let first = data_url(200, 100, "#c72d28");
        let second = data_url(100, 100, "#249b88");
        state.selected = Some(0);
        state.media_reply("artifacts.media", Ok(json!({"data_url":first})));
        state.act(Action::Select(1));
        state.media_reply("artifacts.media", Ok(json!({"data_url":second})));
        settle(&mut state);
        assert_eq!(
            state.preview.as_ref().unwrap().wait().unwrap(),
            [1600, 1600]
        );
        assert_eq!(state.rows[state.selected.unwrap()].session, "two");
        let expected = data(&json!({"data_url":second})).unwrap().1;
        assert_eq!(state.download.as_ref().unwrap().1, expected);
        state.ensure_thumbnails();
        let key = state.rows[0].key();
        state.media_reply(
            &format!("artifacts.thumb.{key}"),
            Ok(json!({"data_url":first})),
        );
        state.reset();
        state.rows = vec![artifact("one")];
        state.ensure_thumbnails();
        state.media_reply(
            &format!("artifacts.thumb.{key}"),
            Ok(json!({"data_url":second})),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.thumbnails[&key].image.is_none() && std::time::Instant::now() < deadline {
            state.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            state.thumbnails[&key]
                .image
                .as_ref()
                .unwrap()
                .wait()
                .unwrap(),
            [360, 360]
        );
        assert!(state.preview.is_none());
    }
}
