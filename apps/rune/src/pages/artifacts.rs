//! Index actual assistant/tool deliveries across recent conversations, one page at a time.
use super::{PageEffect, PageRequest as Req, segment};
use crate::app::Viewer;
mod index;
mod media;
use base64::Engine;
use regex::Regex;
use rune::{Image, ImageCache, TextEdit, Waker};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
};
#[derive(Clone, Debug)]
pub struct Artifact {
    pub kind: String,
    pub value: String,
    pub label: String,
    pub session: String,
    pub profile: String,
    pub title: String,
    pub timestamp: i64,
}
pub struct Artifacts {
    pub rows: Vec<Artifact>,
    pub search: TextEdit,
    pub filter: String,
    pub selected: Option<usize>,
    pub page: usize,
    pub image_page: usize,
    pub file_page: usize,
    pub thumbnails: HashMap<String, Thumbnail>,
    pub preview_loading: bool,
    pub preview_error: Option<String>,
    pub preview_text: Option<String>,
    pub mime: String,
    pub zoom: f32,
    pub demo: bool,
    pub failed_sessions: Vec<(Value, String)>,
    session_rows: Vec<Artifact>,
    session_bytes: usize,
    temp: crate::media::TempMedia,
    writer: Option<crate::media::FileWriter>,
    writes: HashMap<u64, bool>,
    open_pending: Option<std::path::PathBuf>,
    open_url: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub failures: usize,
    pub preview: Option<Image>,
    pub download: Option<(String, Vec<u8>)>,
    pub data_url: Option<String>,
    pub notice: Option<String>,
    pub saving: bool,
    sessions: Vec<Value>,
    cursor: usize,
    offset: usize,
    seen: HashSet<String>,
    page_keys: HashSet<String>,
    cache: Option<ImageCache>,
    safe_images: Option<crate::image_previews::SafeImages>,
    image_jobs: HashMap<u64, (String, bool)>,
    image_latest: HashMap<String, u64>,
    fixture_images: Vec<(String, Vec<u8>)>,
    safe_fixture: Option<HashMap<String, String>>,
}
#[derive(Clone)]
pub enum Action {
    Refresh,
    RetryFailed,
    RetrySession(usize),
    SearchChanged,
    ImagePage(usize),
    FilePage(usize),
    RetryThumbnail(usize),
    NativeOpen,
    Inspect,
    Zoom(f32),
    ClosePreview,
    Filter(String),
    Page(usize),
    Select(usize),
    LoadFile,
    OpenChat(usize),
}
static IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(png|jpe?g|gif|webp|svg|bmp)(\?.*)?$").unwrap());
static FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(png|jpe?g|gif|webp|svg|bmp|pdf|txt|json|md|csv|xlsx?|docx?|pptx?|html|zip|tar|gz|avi|flac|m4a|mkv|mp3|ogg|opus|wav|webm|mp4|mov)(\?.*)?$").unwrap()
});
static LINKS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"!?\[[^\]]*\]\(([^)\s]+)\)|https?://[^\s<>"')`]+|(?:^|[\s("'`])((?:/|~[/\\]|\.{1,2}[/\\]|[A-Za-z]:[/\\]|\\\\)[^\s"'`<>]+)"#).unwrap()
});
static MEDIA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?im)MEDIA:\s*(?:"([^"]+)"|'([^']+)'|`([^`]+)`|([^\r\n<]+))"#).unwrap()
});
static PRODUCER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|_)(?:creat(?:e|ion)|download|export|generat(?:e|ion)|render|save|speech|tts|write)(?:_|$)").unwrap()
});
static STRONG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:artifact_(?:file|image|path|url)|files?_(?:created|modified|written)|generated_(?:file|image|path|url)|media_tag|output_(?:file|path|url)|result_(?:file|path|url)|saved_to|screenshot_path)$").unwrap()
});
static PRODUCER_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:artifacts?|artifact_(?:file|image|path|url)|attachments?|attachment_(?:file|path|url)|downloads?|download_(?:file|path|url)|audio|image|video|(?:audio|image|video)_(?:file|path|url)|file_path|local_path|media|media_(?:file|path|url)|path)$").unwrap()
});
fn decode(value: &str) -> String {
    url::form_urlencoded::parse(format!("v={}", value.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}
fn normalize(value: &str) -> String {
    let mut v = value.trim().to_owned();
    for _ in 0..3 {
        if v.len() > 1 && v.chars().next() == v.chars().last() && v.starts_with(['"', '\'', '`']) {
            v = v[1..v.len() - 1].trim().into();
        } else {
            break;
        }
    }
    v = v
        .trim_end_matches(['`', '*', ')', ',', '.', ';'])
        .to_owned();
    if let Some(path) = v.strip_prefix("#media:") {
        v = decode(path);
    }
    v
}
fn is_path(v: &str) -> bool {
    v.starts_with(['/', '~'])
        || v.starts_with("./")
        || v.starts_with("../")
        || v.starts_with("\\\\")
        || v.starts_with("file://")
        || (v.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && v.as_bytes().get(1) == Some(&b':')
            && v.as_bytes()
                .get(2)
                .is_some_and(|b| *b == b'/' || *b == b'\\'))
}
fn valid(v: &str, explicit: bool) -> bool {
    if v.is_empty() {
        return false;
    }
    let lower = v.to_lowercase().replace('\\', "/");
    if !explicit
        && (lower.contains("/.cache/pip/")
            || lower.contains("/library/caches/pip/")
            || lower.ends_with(".whl")
            || lower.ends_with(".whl.metadata")
            || (lower.starts_with("http")
                && lower.contains("/packages/")
                && [".tar.gz", ".zip", ".egg"]
                    .iter()
                    .any(|s| lower.ends_with(s))))
    {
        return false;
    }
    v.starts_with("https://")
        || v.starts_with("http://")
        || v.starts_with("data:image/")
        || (is_path(v) && (explicit || FILE.is_match(v)))
}
fn from_text(value: &str, out: &mut Vec<(String, bool)>) {
    for c in MEDIA.captures_iter(value) {
        if let Some(v) = (1..=4).find_map(|i| c.get(i)) {
            out.push((v.as_str().trim_end_matches("**").trim().into(), true));
        }
    }
    for c in LINKS.captures_iter(value) {
        let v = c.get(1).or(c.get(2)).or(c.get(0)).unwrap().as_str().trim();
        out.push((v.into(), false));
    }
}
fn strings(
    value: &Value,
    path: &mut Vec<String>,
    producer: bool,
    terminal: bool,
    out: &mut Vec<(String, bool)>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    match value {
        Value::String(v) => {
            let shell = terminal
                && path
                    .iter()
                    .any(|k| ["output", "stdout", "path"].contains(&k.as_str()));
            if shell {
                from_text(v, out);
            } else if path
                .iter()
                .any(|k| STRONG.is_match(k) || (producer && PRODUCER_KEY.is_match(k)))
            {
                for c in MEDIA.captures_iter(v) {
                    if let Some(v) = (1..=4).find_map(|i| c.get(i)) {
                        out.push((v.as_str().into(), true));
                    }
                }
                out.push((v.into(), false));
            }
        }
        Value::Array(a) => {
            for v in a {
                strings(v, path, producer, terminal, out, depth + 1);
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                path.push(k.clone());
                strings(v, path, producer, terminal, out, depth + 1);
                path.pop();
            }
        }
        _ => {}
    }
}
pub fn collect(session: &Value, message: &Value) -> Vec<Artifact> {
    let role = message["role"].as_str().unwrap_or("");
    if role != "assistant" && role != "tool" {
        return vec![];
    }
    let content = ["content", "text", "context"]
        .into_iter()
        .find_map(|k| message[k].as_str().filter(|s| !s.trim().is_empty()))
        .unwrap_or("");
    let name = message["tool_name"]
        .as_str()
        .or(message["name"].as_str())
        .unwrap_or("")
        .to_lowercase();
    let producer = PRODUCER.is_match(&name) || name.starts_with("bfl_flux3_");
    let terminal = name == "terminal";
    let mut values = vec![];
    if role == "assistant" {
        from_text(content, &mut values);
    } else {
        if producer || terminal {
            from_text(content, &mut values);
        }
        if name == "browser_vision" {
            for line in content.lines() {
                if let Some((_, v)) = line.split_once("Screenshot path:") {
                    values.push((v.trim().into(), false));
                }
            }
        }
        if let Ok(v) = serde_json::from_str::<Value>(content) {
            strings(&v, &mut vec![], producer, terminal, &mut values, 0);
        }
        if let Some(start) = content.find("<untrusted_tool_result") {
            if let Some((_, wrapped)) = content[start..].split_once('>') {
                let wrapped = wrapped
                    .split("</untrusted_tool_result>")
                    .next()
                    .unwrap_or("")
                    .trim();
                let payload = wrapped
                    .split_once("\n\n")
                    .map(|(_, s)| s)
                    .unwrap_or(wrapped);
                if let Ok(v) = serde_json::from_str::<Value>(payload) {
                    strings(&v, &mut vec![], producer, terminal, &mut values, 0);
                }
            }
        }
        let mut structured = &message["content"];
        if structured["_multimodal"] == true {
            structured = &structured["meta"];
        }
        if structured.is_object() || structured.is_array() {
            strings(structured, &mut vec![], producer, terminal, &mut values, 0);
        }
    }
    let sid = session["id"]
        .as_str()
        .or(session["session_id"].as_str())
        .unwrap_or("");
    let t = message["timestamp"]
        .as_f64()
        .or(session["last_active"].as_f64())
        .or(session["started_at"].as_f64())
        .unwrap_or(0.0);
    let timestamp = if t < 1e10 {
        (t * 1000.0) as i64
    } else {
        t as i64
    };
    values
        .into_iter()
        .filter_map(|(v, explicit)| {
            let value = normalize(&v);
            if !valid(&value, explicit) {
                return None;
            }
            let kind = if value.starts_with("data:image/") || IMAGE.is_match(&value) {
                "image"
            } else if is_path(&value) {
                "file"
            } else {
                "link"
            };
            let label = if value.starts_with("data:") {
                "Inline image".into()
            } else {
                value.rsplit(['/', '\\']).next().unwrap_or(&value).into()
            };
            Some(Artifact {
                kind: kind.into(),
                label,
                value,
                session: sid.into(),
                profile: session["profile"].as_str().unwrap_or("default").into(),
                title: session["title"]
                    .as_str()
                    .unwrap_or("Untitled session")
                    .into(),
                timestamp,
            })
        })
        .collect()
}
impl Default for Artifacts {
    fn default() -> Self {
        Self::new()
    }
}
impl Artifacts {
    pub fn new() -> Self {
        Self {
            rows: vec![],
            search: TextEdit::new(""),
            filter: "all".into(),
            selected: None,
            page: 0,
            image_page: 0,
            file_page: 0,
            thumbnails: HashMap::new(),
            preview_loading: false,
            preview_error: None,
            preview_text: None,
            mime: String::new(),
            zoom: 1.0,
            demo: false,
            failed_sessions: vec![],
            session_rows: vec![],
            session_bytes: 0,
            temp: crate::media::TempMedia::new(),
            writer: None,
            writes: HashMap::new(),
            open_pending: None,
            open_url: None,
            loading: false,
            error: None,
            failures: 0,
            preview: None,
            download: None,
            data_url: None,
            notice: None,
            saving: false,
            sessions: vec![],
            cursor: 0,
            offset: 0,
            seen: HashSet::new(),
            page_keys: HashSet::new(),
            cache: None,
            safe_images: None,
            image_jobs: HashMap::new(),
            image_latest: HashMap::new(),
            fixture_images: vec![],
            safe_fixture: None,
        }
    }
    pub fn reset(&mut self) {
        self.cancel_images();
        let images = self.safe_images.take();
        let cache = self.cache.take();
        let writer = self.writer.take();
        let temp = std::mem::replace(&mut self.temp, crate::media::TempMedia::new());
        *self = Self::new();
        self.cache = cache;
        self.safe_images = images;
        self.writer = writer;
        self.temp = temp;
    }
    pub fn init(&mut self, waker: Waker) {
        self.safe_images = Some(crate::image_previews::SafeImages::new(waker.clone()));
        self.cache = Some(ImageCache::new(waker.clone()));
        self.writer = Some(crate::media::FileWriter::new(waker));
        self.load_fixture_images();
        if self.safe_fixture.is_some() {
            self.ensure_thumbnails();
        }
    }
    pub fn visible(&self) -> Vec<usize> {
        let q = self.search.text.to_lowercase();
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                (self.filter == "all" || self.filter == r.kind)
                    && format!("{} {} {}", r.label, r.value, r.title)
                        .to_lowercase()
                        .contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }
    pub fn fixture(&mut self) {
        self.rows.clear();
        self.thumbnails.clear();
        self.fixture_images.clear();
        let session = json!({"id":"sample","profile":"default","title":"Example deliverables","started_at":1793278800});
        for m in [
            json!({"role":"assistant","content":"Report: [notes](https://example.com/report). MEDIA: /srv/hermes/report.pdf"}),
            json!({"role":"tool","tool_name":"image_generate","content":{"output_path":"/srv/hermes/chart.png"}}),
        ] {
            self.rows.extend(collect(&session, &m));
        }
        {
            for (index, (label, color)) in [
                ("Architecture diagram", "#688b94"),
                ("Launch concept", "#a98d72"),
                ("Weekly trends", "#8c83a1"),
            ]
            .into_iter()
            .enumerate()
            {
                let svg = format!(
                    "<svg xmlns='http://www.w3.org/2000/svg' width='640' height='360'><rect width='640' height='360' fill='{color}'/><circle cx='495' cy='110' r='100' fill='#ffffff' opacity='.16'/><path d='M65 280 L165 190 L265 240 L365 125 L485 165 L575 65' fill='none' stroke='#ffffff' stroke-width='8'/><text x='48' y='62' font-family='sans-serif' font-size='25' fill='#ffffff'>{label}</text></svg>"
                );
                let artifact = Artifact {
                    kind: "image".into(),
                    value: format!("/srv/hermes/deliverables/sample-{index}.png"),
                    label: format!("{label}.png"),
                    session: "sample".into(),
                    profile: "default".into(),
                    title: "Example deliverables".into(),
                    timestamp: 1793278800000 + index as i64 * 60000,
                };
                self.fixture_images.push((artifact.key(), svg.into_bytes()));
                self.rows.push(artifact);
            }
        }
        let mut seen = HashSet::new();
        self.rows.retain(|a| seen.insert(a.key()));
        self.demo = true;
        self.load_fixture_images();
    }
    /// Production gallery acceptance: source-like paths with deferred synthetic transport replies.
    /// Scene construction precedes init; no image decode runs while building the scene.
    pub fn fixture_safe_images(&mut self) {
        self.cancel_images();
        self.rows.clear();
        self.thumbnails.clear();
        self.fixture_images.clear();
        self.selected = None;
        self.preview = None;
        self.download = None;
        self.demo = false;
        self.loading = false;
        self.notice = None;
        let mut data = HashMap::new();
        for i in 0..24 {
            let artifact = Artifact {
                kind: "image".into(),
                value: format!("/fixtures/diagram-{:02}.svg", i + 1),
                label: format!("Diagram {:02}.svg", i + 1),
                session: "sample".into(),
                profile: "default".into(),
                title: "Image preview fixtures".into(),
                timestamp: 1793278800000,
            };
            let svg = format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="480" height="180"><rect width="480" height="180" fill="#d7eee8"/><rect x="16" y="16" width="120" height="148" rx="8" fill="#509985"/><rect x="152" y="20" width="310" height="62" rx="8" fill="#91baae"/><rect x="152" y="100" width="250" height="60" rx="8" fill="#bd9984"/><text x="168" y="59" font-family="Inter" font-size="24" fill="#223c34">Hermes diagram {:02}</text><text x="168" y="138" font-family="monospace" font-size="20" fill="#223c34">bot-{:02}: connected</text></svg>"##,
                i + 1,
                i + 1
            );
            data.insert(
                artifact.key(),
                format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(svg)
                ),
            );
            self.rows.push(artifact);
        }
        self.safe_fixture = Some(data);
    }
    fn load_fixture_images(&mut self) {
        if let Some(cache) = &self.cache {
            for (key, bytes) in &self.fixture_images {
                self.thumbnails.insert(
                    key.clone(),
                    Thumbnail {
                        image: Some(cache.load_mime(
                            Arc::<[u8]>::from(bytes.clone()),
                            "image/svg+xml",
                            360.0,
                            1.0,
                        )),
                        error: None,
                        loading: false,
                    },
                );
            }
        }
    }
}
impl Viewer {
    pub fn artifacts_action(&mut self, action: Action) {
        if self.artifacts.demo
            && matches!(
                action,
                Action::LoadFile
                    | Action::NativeOpen
                    | Action::Inspect
                    | Action::Refresh
                    | Action::RetryFailed
                    | Action::RetrySession(_)
                    | Action::RetryThumbnail(_)
            )
        {
            self.artifacts.notice =
                Some("Demonstration data · live artifact reads are disabled".into());
            return;
        }
        if matches!(
            action,
            Action::Select(_) | Action::Refresh | Action::ClosePreview
        ) {
            self.invalidate_page_requests("artifacts.media");
            if matches!(action, Action::Refresh) {
                self.invalidate_page_requests("artifacts.thumb.");
            }
        }
        if matches!(action, Action::Inspect) {
            if let Some(artifact) = self
                .artifacts
                .selected
                .and_then(|i| self.artifacts.rows.get(i))
                .cloned()
            {
                self.preview_artifact(artifact);
            }
            return;
        }
        if let Action::OpenChat(i) = action {
            if let Some(a) = self.artifacts.rows.get(i).cloned() {
                self.open_page_session(a.profile, a.session, a.title);
            }
            return;
        }
        if matches!(action, Action::NativeOpen) && self.is_local_connection() {
            if let Some(a) = self
                .artifacts
                .selected
                .and_then(|i| self.artifacts.rows.get(i))
            {
                if is_path(&a.value) {
                    if let Ok(url) = crate::media::local_file_url(&a.value) {
                        self.artifacts.open_url = Some(url);
                        return;
                    }
                }
            }
        }
        let effects = self.artifacts.act(action);
        self.page_requests(effects);
    }
}

impl Viewer {
    pub fn download_artifact(
        &mut self,
        artifact: Artifact,
        destination: std::path::PathBuf,
        owner: crate::composer::DraftScope,
    ) {
        if self.composer_scope().connection != owner.connection || self.profile != owner.profile {
            self.artifacts.error = Some(
                "The connection changed while the file dialog was open. Choose the file again."
                    .into(),
            );
            return;
        }
        if self.artifacts.demo {
            return;
        }
        self.artifacts.saving = true;
        self.page_request(
            "artifacts.download",
            Req::get("/api/fs/download")
                .profile(&artifact.profile)
                .query("path", &artifact.value)
                .query("session_id", &artifact.session)
                .download_to(destination)
                .timeout_secs(3600),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_collects_real_deliveries_excludes_user_inputs_and_cache_noise() {
        let session = json!({"id":"s","profile":"work","title":"Report"});
        assert!(
            collect(
                &session,
                &json!({"role":"user","content":"MEDIA: /home/upload.pdf"})
            )
            .is_empty()
        );
        let artifacts = collect(
            &session,
            &json!({"role":"assistant","content":"[report](/srv/final.pdf) and /home/.cache/pip/a.whl. MEDIA: \"/srv/final chart.png\""}),
        );
        assert!(artifacts.iter().any(|a| a.value == "/srv/final.pdf"));
        assert!(artifacts.iter().any(|a| a.value == "/srv/final chart.png"));
        assert!(!artifacts.iter().any(|a| a.value.contains(".whl")));
        assert!(
            artifacts
                .iter()
                .all(|a| a.profile == "work" && a.session == "s")
        );
        assert!(!is_path("x:relative.txt"));
    }
    #[test]
    fn history_index_refuses_foreign_owner_and_stops_repeated_backend_pages() {
        let mut state = Artifacts::new();
        state.apply(
            "artifacts.sessions",
            Ok(json!({"sessions":[{"id":"s","profile":"work"}]})),
        );
        let effects=state.apply("artifacts.messages",Ok(json!({"profile":"other","session_id":"s","messages":[{"role":"assistant","content":"MEDIA: /foreign.pdf"}]})));
        assert!(effects.is_empty());
        assert!(state.rows.is_empty());
        assert_eq!(state.failures, 1);
        state = Artifacts::new();
        state.apply(
            "artifacts.sessions",
            Ok(json!({"sessions":[{"id":"s","profile":"work"}]})),
        );
        let page = json!({"profile":"work","session_id":"s","messages":[{"role":"assistant","content":"MEDIA: /report.pdf"}],"pagination":{"limit":1}});
        assert_eq!(state.apply("artifacts.messages", Ok(page.clone())).len(), 1);
        assert!(state.apply("artifacts.messages", Ok(page)).is_empty());
        assert!(!state.loading);
        assert_eq!(state.rows.len(), 0);
        assert_eq!(state.failures, 1);
    }
}

pub struct Thumbnail {
    pub image: Option<Image>,
    pub error: Option<String>,
    pub loading: bool,
}
impl Artifact {
    pub fn key(&self) -> String {
        use sha2::{Digest, Sha256};
        format!(
            "{:x}",
            Sha256::digest(
                format!("{}\0{}\0{}", self.profile, self.session, self.value).as_bytes()
            )
        )
    }
    pub fn time(&self) -> String {
        chrono::DateTime::from_timestamp_millis(self.timestamp)
            .map(|t| t.format("%b %e, %Y · %H:%M").to_string())
            .unwrap_or_else(|| "Unknown date".into())
    }
}
#[cfg(test)]
mod parity_tests {
    use super::*;
    #[test]
    fn failed_session_discards_partial_artifacts_and_individual_retry_restarts_exact_owner() {
        let mut a = Artifacts::new();
        a.apply("artifacts.sessions",Ok(json!({"sessions":[{"id":"failed","profile":"work","title":"Failed"},{"id":"good","profile":"other"}]})));
        a.apply("artifacts.messages",Ok(json!({"messages":[{"role":"assistant","content":"MEDIA: /partial.txt"}],"pagination":{"limit":1}})));
        a.apply("artifacts.messages", Err("offline".into()));
        assert!(a.rows.is_empty());
        a.apply(
            "artifacts.messages",
            Ok(json!({"messages":[{"role":"assistant","content":"MEDIA: /complete.txt"}]})),
        );
        assert_eq!(a.rows.len(), 1);
        assert_eq!(a.failures, 1);
        let e = a.act(Action::RetrySession(0));
        assert_eq!(e[0].1.profile.as_deref(), Some("work"));
        assert_eq!(
            e[0].1.query.iter().find(|(k, _)| k == "offset").unwrap().1,
            "0"
        );
        a.apply(
            "artifacts.messages",
            Ok(json!({"messages":[{"role":"assistant","content":"MEDIA: /recovered.txt"}]})),
        );
        assert_eq!(a.rows.len(), 2);
        assert_eq!(a.failures, 0);
    }
    #[test]
    fn thumbnail_and_native_copy_keep_artifact_profile_and_session() {
        let mut a = Artifacts::new();
        a.rows.push(Artifact {
            kind: "image".into(),
            value: "~/output/image.png".into(),
            label: "image.png".into(),
            session: "source-session".into(),
            profile: "origin".into(),
            title: "Source".into(),
            timestamp: 0,
        });
        let e = a.ensure_thumbnails();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].1.profile.as_deref(), Some("origin"));
        assert!(
            e[0].1
                .query
                .contains(&("session_id".into(), "source-session".into()))
        );
        a.selected = Some(0);
        let e = a.act(Action::NativeOpen);
        assert_eq!(e[0].1.profile.as_deref(), Some("origin"));
        assert_eq!(e[0].1.path, "/api/fs/download");
        assert!(
            e[0].1
                .query
                .contains(&("path".into(), "~/output/image.png".into()))
        );
        assert!(a.take_open_url().is_none());
        a.apply("artifacts.native-open", Err("refused".into()));
        assert!(a.take_open_url().is_none());
        assert!(!a.saving);
    }
    #[test]
    fn all_filter_retains_independent_image_and_file_pages_and_native_ack_is_required() {
        let mut a = Artifacts::new();
        a.act(Action::FilePage(3));
        assert_eq!(a.file_page, 3);
        a.act(Action::ImagePage(1));
        assert_eq!(a.file_page, 3);
        a.apply("artifacts.download", Ok(json!({"ok":true})));
        assert!(a.notice.is_none());
        assert!(a.error.is_some());
    }
    #[test]
    fn inline_preview_retains_binary_data_and_decodes_text_without_guessing_image() {
        let mut a = Artifacts::new();
        a.rows.push(Artifact {
            kind: "file".into(),
            value: "/report.txt".into(),
            label: "report.txt".into(),
            session: "s".into(),
            profile: "p".into(),
            title: "t".into(),
            timestamp: 0,
        });
        a.selected = Some(0);
        a.apply(
            "artifacts.media",
            Ok(json!({"data_url":"data:text/plain;base64,aGVsbG8="})),
        );
        assert_eq!(a.preview_text.as_deref(), Some("hello"));
        assert_eq!(a.download.as_ref().unwrap().1, b"hello");
        assert!(a.preview.is_none());
    }
}
#[cfg(test)]
mod fixture_lifecycle_test {
    use super::*;
    #[test]
    fn gallery_fixture_created_before_native_init_keeps_images_and_unique_row_keys() {
        let mut a = Artifacts::new();
        a.fixture();
        let count = a.rows.len();
        assert!(count > 3);
        let keys: HashSet<_> = a.rows.iter().map(Artifact::key).collect();
        assert_eq!(keys.len(), count);
        assert!(a.thumbnails.is_empty());
        a.init(std::sync::Arc::new(|| {}));
        assert_eq!(a.rows.len(), count);
        assert!(a.thumbnails.values().all(|t| t.image.is_some()));
        assert_eq!(a.thumbnails.len(), 3);
    }
}
