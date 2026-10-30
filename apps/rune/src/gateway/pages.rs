//! Shared authenticated REST/RPC operations for native workspace features.
use super::*;
use crate::pages::{PageMethod, PageRequest};
impl Gateway {
    pub fn page_request(&mut self, profile: &str, request: &PageRequest) -> Result<Value, String> {
        self.page_request_events(profile, request, |_| {})
    }
    pub fn page_request_events(
        &mut self,
        profile: &str,
        request: &PageRequest,
        mut on_event: impl FnMut(Value),
    ) -> Result<Value, String> {
        valid_scope(profile)?;
        let deadline = Instant::now() + Duration::from_secs(request.timeout);
        if matches!(request.method, PageMethod::Rpc | PageMethod::PluginRpc) {
            let mut body = request
                .body
                .as_object()
                .cloned()
                .ok_or("Workspace RPC parameters must be an object")?;
            body.insert("profile".into(), json!(profile));
            return self.page_rpc_until(
                &request.path,
                Value::Object(body),
                deadline,
                &mut on_event,
                matches!(request.method, PageMethod::PluginRpc),
            );
        }
        #[cfg(target_os = "linux")]
        if matches!(&self.auth, Auth::Native(_)) && !self.profile_homes.contains_key(profile) {
            self.profiles_until(deadline)?;
        }
        if !request.path.starts_with("/api/")
            || request.path.contains('#')
            || request.path.contains("..")
        {
            return Err("Invalid workspace API route".into());
        }
        let mut url = self
            .base
            .join(request.path.trim_start_matches('/'))
            .map_err(|_| "Invalid workspace API route")?;
        if url.origin() != self.base.origin() {
            return Err("Workspace request changed gateway owner".into());
        }
        let has_profile = url.query_pairs().any(|(key, _)| key == "profile")
            || request.query.iter().any(|(key, _)| key == "profile");
        if url.query_pairs().any(|(key, _)| key == "token") {
            return Err("Workspace route cannot contain credentials".into());
        }
        if has_profile
            && !(matches!(request.method, PageMethod::Get)
                && request.path.split('?').next() == Some("/api/profiles/sessions"))
        {
            return Err("Use the explicit request profile scope".into());
        }
        if !has_profile {
            url.query_pairs_mut().append_pair("profile", profile);
        }
        for (key, value) in &request.query {
            if key == "token" {
                return Err("Workspace query cannot replace authentication or owner".into());
            }
            url.query_pairs_mut().append_pair(key, value);
        }
        let method = match request.method {
            PageMethod::Get => reqwest::Method::GET,
            PageMethod::Post => reqwest::Method::POST,
            PageMethod::Put => reqwest::Method::PUT,
            PageMethod::Patch => reqwest::Method::PATCH,
            PageMethod::Delete => reqwest::Method::DELETE,
            PageMethod::Rpc | PageMethod::PluginRpc | PageMethod::LocalFs => unreachable!(),
        };
        let mut headers = reqwest::header::HeaderMap::new();
        match &self.auth {
            Auth::Token(Some(token)) => {
                url.query_pairs_mut().append_pair("token", token);
            }
            Auth::Token(None) => {}
            #[cfg(target_os = "linux")]
            Auth::Native(endpoint) => {
                let home = self
                    .profile_homes
                    .get(profile)
                    .ok_or("Profile is not served by this gateway")?;
                let ticket = endpoint.ticket_until(home, "native-http", deadline)?;
                headers.insert(
                    "X-Hermes-Gateway-Ticket",
                    ticket.parse().map_err(|_| "Invalid native HTTP ticket")?,
                );
            }
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Workspace request timed out")?;
        let mut operation = self
            .http
            .request(method, url)
            .headers(headers)
            .timeout(remaining);
        if let Some(upload) = &request.upload {
            if !request.body.is_null()
                || !matches!(
                    request.method,
                    PageMethod::Post | PageMethod::Put | PageMethod::Patch
                )
            {
                return Err(
                    "File upload must be a single multipart write without a JSON body".into(),
                );
            }
            if upload.bytes.len() > 256 * 1024 * 1024 {
                return Err("The uploaded file exceeds the 256 MiB transfer limit".into());
            }
            if upload.content_type.contains(['\r', '\n']) {
                return Err("Invalid upload content type".into());
            }
            let boundary = format!("----hermes-{}", uuid::Uuid::new_v4().simple());
            let filename = upload.filename.replace(['"', '\r', '\n'], "_");
            let mime = if upload.content_type.is_empty() {
                "application/octet-stream"
            } else {
                &upload.content_type
            };
            let mut bytes=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n").into_bytes();
            bytes.extend_from_slice(&upload.bytes);
            bytes.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            operation = operation
                .header(
                    "Content-Type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(bytes);
        } else if !request.body.is_null() {
            let data =
                serde_json::to_vec(&request.body).map_err(|_| "Invalid workspace request")?;
            if data.len() > MAX_BODY {
                return Err("Workspace request exceeds the size limit".into());
            }
            operation = operation
                .header("Content-Type", "application/json")
                .body(data);
        }
        let response = operation
            .send()
            .map_err(|_| "Workspace request failed (network, TLS or timeout)")?;
        if let Some(destination) = &request.destination {
            if !matches!(request.method, PageMethod::Get) || request.path != "/api/fs/download" {
                return Err("Invalid native download request".into());
            }
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                let mut fallback = PageRequest::get("/api/fs/read-data-url");
                fallback.query = request.query.clone();
                fallback.timeout = request.timeout;
                let data = self.page_request(profile, &fallback)?;
                let data = data
                    .as_str()
                    .or(data["dataUrl"].as_str())
                    .or(data["data_url"].as_str())
                    .ok_or("The agent returned invalid file data")?;
                let (meta, data) = data
                    .split_once(',')
                    .ok_or("The agent returned invalid file data")?;
                if !meta.ends_with(";base64") {
                    return Err("The agent returned unsupported file encoding".into());
                }
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| "The agent returned invalid file data")?;
                save_download(destination, &mut bytes.as_slice())?;
            } else {
                if !response.status().is_success() {
                    return Err(format!(
                        "File download refused (HTTP {})",
                        response.status().as_u16()
                    ));
                }
                save_download(destination, &mut response.take(u64::MAX))?;
            }
            return Ok(json!({"ok":true,"downloaded":true}));
        }
        if !response.status().is_success() {
            return Err(format!(
                "Workspace request refused (HTTP {})",
                response.status().as_u16()
            ));
        }
        let response_limit = if request.path == "/api/fs/read-data-url" {
            48 * 1024 * 1024
        } else {
            MAX_BODY
        };
        let mut bytes = vec![];
        response
            .take((response_limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "Workspace response could not be read")?;
        if bytes.len() > response_limit {
            return Err("Workspace response exceeds the size limit".into());
        }
        if bytes.is_empty() {
            return Ok(json!({"ok":true}));
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid workspace response")?;
        let platform_test = matches!(request.method, PageMethod::Post)
            && request.path.starts_with("/api/messaging/platforms/")
            && request.path.ends_with("/test")
            && request.path.split('/').count() == 6;
        if !platform_test && value.get("ok") == Some(&Value::Bool(false)) {
            return Err("The agent rejected this operation; no success was confirmed".into());
        }
        Ok(value)
    }
}

impl Gateway {
    fn page_rpc_until(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
        on_event: &mut impl FnMut(Value),
        plugin: bool,
    ) -> Result<Value, String> {
        let unset_profile_model = method == "cli.exec"
            && params["argv"].as_array().is_some_and(|args| {
                args.len() == 5
                    && args[0] == "--profile"
                    && args[1]
                        .as_str()
                        .is_some_and(|name| valid_scope(name).is_ok())
                    && args[2] == "config"
                    && args[3] == "unset"
                    && args[4] == "model"
            });
        let allowed = plugin
            || unset_profile_model
            || matches!(
                method,
                "setup.runtime_check"
                    | "image.generate"
                    | "plugins.manage"
                    | "connection.respond"
                    | "profiles.list"
                    | "profiles.describe"
                    | "profiles.configure"
                    | "profiles.create"
                    | "profiles.get_asset"
                    | "profiles.set_asset"
                    | "mcp.catalog"
                    | "config.get"
                    | "config.set"
                    | "process.list"
                    | "session.list"
                    | "process.kill"
            )
            || ["vault.", "billing.", "connectors.", "mcp.servers."]
                .iter()
                .any(|prefix| method.starts_with(prefix));
        if !allowed {
            return Err("This method is not part of the native workspace API".into());
        }
        self.sequence += 1;
        let id = self.sequence;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Workspace RPC timed out")?;
        socket_timeout(self.socket.get_mut(), remaining)?;
        self.socket
            .send(Message::Text(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| "Workspace RPC could not be sent")?;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("Workspace RPC timed out")?;
            socket_timeout(self.socket.get_mut(), remaining)?;
            match self
                .socket
                .read()
                .map_err(|_| "Workspace RPC failed (connection closed or timeout)")?
            {
                Message::Text(raw) => {
                    let value: Value =
                        serde_json::from_str(&raw).map_err(|_| "Invalid workspace RPC response")?;
                    if value.get("id").and_then(Value::as_u64) != Some(id) {
                        if plugin
                            || method == "billing.step_up"
                                && (value["type"] == "billing.step_up.verification"
                                    || value["method"] == "billing.step_up.verification"
                                    || value["event"] == "billing.step_up.verification")
                        {
                            on_event(value);
                        }
                        continue;
                    }
                    if let Some(error) = value.get("error") {
                        let code = error["code"].as_i64().unwrap_or(-1);
                        if matches!(method, "process.list" | "process.kill") {
                            if let Some(reason) = error.pointer("/data/reason") {
                                let token = reason
                                    .as_str()
                                    .filter(|s| {
                                        !s.is_empty()
                                            && s.len() <= 64
                                            && s.chars().all(|c| {
                                                c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
                                            })
                                    })
                                    .unwrap_or("other");
                                return Err(format!(
                                    "Process RPC refused ({code}; reason={token})"
                                ));
                            }
                        }
                        return Err(format!("Workspace RPC refused ({code})"));
                    }
                    return value
                        .get("result")
                        .cloned()
                        .ok_or_else(|| "Workspace RPC response has no result".into());
                }
                Message::Ping(_) => self
                    .socket
                    .flush()
                    .map_err(|_| "Workspace heartbeat failed")?,
                Message::Close(_) => return Err("Workspace connection closed".into()),
                _ => {}
            }
        }
    }
}

fn save_download(
    destination: &std::path::Path,
    source: &mut impl std::io::Read,
) -> Result<(), String> {
    use std::io::Write;
    let parent = destination.parent().ok_or("Invalid download destination")?;
    let temp = parent.join(format!(".hermes-download-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temp)
            .map_err(|_| "The selected download could not be opened")?;
        std::io::copy(source, &mut file)
            .map_err(|_| "The download was interrupted; the destination was not replaced")?;
        file.flush()
            .and_then(|_| file.sync_all())
            .map_err(|_| "The download could not be saved")?;
        drop(file);
        std::fs::rename(&temp, destination).map_err(|_| "The download could not be committed")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
