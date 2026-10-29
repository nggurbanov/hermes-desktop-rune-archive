use super::*;
impl Capabilities {
    pub(super) fn mcp_oauth_action(&mut self, action: Action) -> Vec<PageEffect> {
        if self.demo {
            return vec![];
        }
        match action {
            Action::StartMcpOAuth(name) => {
                if self.busy() {
                    return vec![];
                }
                let flow = match crate::oauth::McpOAuth::new(name.clone()) {
                    Ok(flow) => flow,
                    Err(error) => {
                        self.message = Some(error);
                        return vec![];
                    }
                };
                let redirect = flow.listener.as_ref().unwrap().redirect_uri.clone();
                self.mcp_oauth = Some(flow);
                vec![
                    self.request(
                        "mcp-oauth-start",
                        PageRequest::rpc(
                            "mcp.servers.oauth.start",
                            json!({"name":name,"client_redirect_uri":redirect}),
                        )
                        .timeout_secs(60),
                    ),
                ]
            }
            Action::CancelMcpOAuth => {
                if let Some(flow) = &mut self.mcp_oauth {
                    flow.status = if flow.session_id.is_empty()
                        && !self.pending.contains("capabilities.mcp-oauth-start")
                    {
                        "cancelled"
                    } else {
                        "cancelling"
                    }
                    .into();
                    flow.listener = None;
                }
                self.cancel_mcp_oauth()
            }
            Action::PollMcpOAuth => {
                let Some(flow) = &self.mcp_oauth else {
                    return vec![];
                };
                if flow.session_id.is_empty() || !flow.running() {
                    return vec![];
                }
                let params = flow.params();
                vec![self.request(
                    "mcp-oauth-poll",
                    PageRequest::rpc("mcp.servers.oauth.poll", params).timeout_secs(60),
                )]
            }
            _ => vec![],
        }
    }
    fn cancel_mcp_oauth(&mut self) -> Vec<PageEffect> {
        let Some(flow) = &mut self.mcp_oauth else {
            return vec![];
        };
        if flow.cancel_requested || flow.session_id.is_empty() {
            return vec![];
        }
        flow.cancel_requested = true;
        let params = flow.params();
        vec![self.request(
            "mcp-oauth-cancel",
            PageRequest::rpc("mcp.servers.oauth.cancel", params).timeout_secs(60),
        )]
    }
    pub(super) fn tick_mcp_oauth(&mut self) -> Vec<PageEffect> {
        let Some(flow) = &mut self.mcp_oauth else {
            return vec![];
        };
        if !flow.running() || flow.status == "cancelling" {
            return vec![];
        }
        let callback = flow
            .listener
            .as_mut()
            .and_then(crate::oauth::Loopback::poll);
        if let Some(callback) = callback {
            match callback {
                Ok(callback) => {
                    let mut params = flow.params().as_object().unwrap().clone();
                    params.extend(callback.as_object().unwrap().clone());
                    flow.relayed = true;
                    flow.listener = None;
                    return vec![
                        self.request(
                            "mcp-oauth-callback",
                            PageRequest::rpc("mcp.servers.oauth.callback", Value::Object(params))
                                .timeout_secs(60),
                        ),
                    ];
                }
                Err(error) => {
                    flow.fail(error);
                    return self.cancel_mcp_oauth();
                }
            }
        }
        if !self.pending.contains("capabilities.mcp-oauth-poll")
            && !self.pending.contains("capabilities.mcp-oauth-callback")
        {
            self.mcp_oauth_action(Action::PollMcpOAuth)
        } else {
            vec![]
        }
    }
    pub(super) fn apply_mcp_oauth(
        &mut self,
        key: &str,
        result: Result<Value, String>,
    ) -> Vec<PageEffect> {
        let value = match result {
            Ok(value) if value.get("ok") != Some(&Value::Bool(false)) => value,
            result => {
                let error = match result {
                    Err(error) => error,
                    Ok(value) => {
                        let message = string(&value, "error_message");
                        if message.is_empty() {
                            "The agent refused this OAuth operation".into()
                        } else {
                            message
                        }
                    }
                };
                if key == "mcp-oauth-cancel" {
                    let Some(flow) = &self.mcp_oauth else {
                        return vec![];
                    };
                    let state = url::Url::parse(&flow.authorization_url)
                        .ok()
                        .and_then(|url| {
                            url.query_pairs()
                                .find(|(key, _)| key == "state")
                                .map(|(_, value)| value.into_owned())
                        });
                    if let Some(state) = state {
                        let mut params = flow.params();
                        params["state"] = state.into();
                        params["error"] = "access_denied".into();
                        return vec![self.request(
                            "mcp-oauth-deny",
                            PageRequest::rpc("mcp.servers.oauth.callback", params).timeout_secs(60),
                        )];
                    }
                }
                if let Some(flow) = &mut self.mcp_oauth {
                    flow.fail(error);
                }
                return if key == "mcp-oauth-deny" {
                    vec![]
                } else {
                    self.cancel_mcp_oauth()
                };
            }
        };
        let Some(flow) = &mut self.mcp_oauth else {
            return vec![];
        };
        match key {
            "mcp-oauth-start" => {
                flow.session_id = string(&value, "session_id");
                flow.authorization_url = string(&value, "auth_url");
                if matches!(flow.status.as_str(), "cancelled" | "cancelling") {
                    return self.cancel_mcp_oauth();
                }
                if flow.session_id.is_empty() {
                    flow.fail("The agent did not return an OAuth flow identity".into());
                    return vec![];
                }
                let result = flow
                    .listener
                    .as_mut()
                    .ok_or_else(|| "The local callback listener is unavailable".to_owned())
                    .and_then(|listener| listener.authorize(&flow.authorization_url));
                if let Err(error) = result {
                    flow.fail(error);
                    return self.cancel_mcp_oauth();
                }
                flow.status = "pending".into();
                vec![]
            }
            "mcp-oauth-poll" => match string(&value, "status").as_str() {
                "approved" => {
                    flow.status = "approved".into();
                    flow.listener = None;
                    self.mcp_probe = json!({"ok":true,"tools":value["tools"],"oauth_needed":false});
                    self.message =
                        Some("The agent confirmed MCP authorization and stored its tokens.".into());
                    self.load()
                }
                "error" => {
                    flow.fail(string(&value, "error_message"));
                    self.cancel_mcp_oauth()
                }
                "pending" => vec![],
                _ => {
                    flow.fail("The agent returned an unknown OAuth status".into());
                    self.cancel_mcp_oauth()
                }
            },
            "mcp-oauth-callback" => self.mcp_oauth_action(Action::PollMcpOAuth),
            "mcp-oauth-cancel" | "mcp-oauth-deny" => {
                if flow.status == "cancelling" {
                    flow.status = "cancelled".into();
                }
                vec![]
            }
            _ => vec![],
        }
    }
}
