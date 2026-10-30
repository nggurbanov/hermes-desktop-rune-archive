//! Passive roster reads use HTTP only and one absolute source deadline.
use super::*;
use std::io::Read as _;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ProfileMetadata {
    /// Already-resolved label from the current authenticated profiles reply.
    pub resolved_label: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub title: Option<String>,
    pub ui_meta: Option<Value>,
    pub has_avatar: Option<bool>,
}
#[derive(Clone, Debug)]
pub(crate) struct RosterProfile {
    pub name: String,
    pub metadata: ProfileMetadata,
    pub canonical_session: Option<CanonicalSession>,
}
impl ProfileMetadata {
    pub fn gateway_profile(&self, name: &str) -> Profile {
        let bot = self
            .ui_meta
            .as_ref()
            .and_then(|meta| meta.get("hermes-bots"));
        let row = json!({"display_name":self.display_name,"title":self.title});
        let label = self.resolved_label.clone().unwrap_or_else(|| {
            profile_label(&row, if self.title.is_none() { bot } else { None }, name)
        });
        Profile {
            name: name.into(),
            label,
            description: self
                .description
                .clone()
                .or_else(|| {
                    bot.and_then(|value| value.get("description"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_default(),
            canonical_session: None,
        }
    }
}
impl RosterProfile {
    pub fn gateway_profile(&self) -> Profile {
        let mut profile = self.metadata.gateway_profile(&self.name);
        profile.canonical_session = self.canonical_session.clone();
        profile
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Identity {
    NotRequested,
    Observed(Option<String>),
    Unavailable,
}
pub(crate) struct Read {
    pub profiles: Vec<RosterProfile>,
    pub identity: Identity,
}
#[derive(Clone)]
struct Http {
    base: Url,
    token: Option<String>,
    profile: String,
    client: reqwest::blocking::Client,
    #[cfg(target_os = "linux")]
    native: Option<local::Endpoint>,
}
impl Http {
    fn new(connection: &Connection, deadline: Instant) -> Result<Self, String> {
        match connection {
            Connection::Saved { inner, .. } => Self::new(inner, deadline),
            Connection::Url { endpoint, token } => {
                let (base, embedded) = endpoint_url(endpoint)?;
                Self::build(base, token.clone().or(embedded), "default".into(), deadline)
            }
            Connection::Local { home } => {
                #[cfg(target_os = "linux")]
                {
                    let endpoint = local::identify_until(home, deadline)?;
                    let mut transport = Self::build(
                        Url::parse(&endpoint.origin).map_err(|_| "Invalid local gateway origin")?,
                        None,
                        endpoint.profile_name(),
                        deadline,
                    )?;
                    transport.native = Some(endpoint);
                    Ok(transport)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = home;
                    Err("Passive local attachment is not available on this platform; use an existing gateway URL".into())
                }
            }
        }
    }
    fn build(
        base: Url,
        token: Option<String>,
        profile: String,
        deadline: Instant,
    ) -> Result<Self, String> {
        let remaining = remaining(deadline)?;
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(remaining)
            .connect_timeout(remaining.min(Duration::from_secs(5)))
            .redirect(reqwest::redirect::Policy::none());
        if base.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }) {
            builder = builder.no_proxy();
        }
        Ok(Self {
            base,
            token,
            profile,
            client: builder
                .build()
                .map_err(|_| "Passive HTTP transport is unavailable")?,
            #[cfg(target_os = "linux")]
            native: None,
        })
    }
    fn get(&self, path: &str, deadline: Instant, cancelled: &AtomicBool) -> Result<Value, String> {
        check(cancelled, deadline)?;
        let mut url = self
            .base
            .join(path)
            .map_err(|_| "Invalid passive roster route")?;
        url.query_pairs_mut().append_pair("profile", &self.profile);
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = &self.token {
            url.query_pairs_mut().append_pair("token", token);
        }
        #[cfg(target_os = "linux")]
        if let Some(endpoint) = &self.native {
            let ticket = endpoint.ticket_until(&endpoint.profile_id, "native-http", deadline)?;
            headers.insert(
                "X-Hermes-Gateway-Ticket",
                ticket
                    .parse()
                    .map_err(|_| "Invalid passive native ticket")?,
            );
        }
        let response = self
            .client
            .get(url)
            .headers(headers)
            .timeout(remaining(deadline)?)
            .send()
            .map_err(|_| "Roster read failed (network, TLS or deadline)")?;
        if !response.status().is_success() {
            return Err(format!(
                "Roster read refused (HTTP {})",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        response
            .take((MAX_BODY + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "Roster response could not be read")?;
        check(cancelled, deadline)?;
        if bytes.len() > MAX_BODY {
            return Err("Roster response exceeds the size limit".into());
        }
        serde_json::from_slice(&bytes).map_err(|_| "Invalid roster response".into())
    }
}
fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or("Roster enumeration timed out".into())
}
fn check(cancelled: &AtomicBool, deadline: Instant) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Roster enumeration was superseded".into());
    }
    remaining(deadline).map(|_| ())
}

pub(crate) fn read(
    connection: &Connection,
    deadline: Instant,
    identity: bool,
    cancelled: Arc<AtomicBool>,
) -> Result<Read, String> {
    check(&cancelled, deadline)?;
    let http = Http::new(connection, deadline)?;
    let identity_limit = Instant::now() + Duration::from_secs(2).min(remaining(deadline)? / 2);
    std::thread::scope(|scope| {
        let status = identity
            .then(|| scope.spawn(|| http.get("api/status", identity_limit, &cancelled).ok()));
        let body = http.get(
            "api/profiles",
            deadline.min(Instant::now() + Duration::from_secs(8)),
            &cancelled,
        );
        let identity = match status {
            None => Identity::NotRequested,
            Some(task) => match task.join().map_err(|_| "Roster identity worker failed")? {
                Some(body) => Identity::Observed(
                    body.get("install_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|id| {
                            !id.is_empty() && id.len() <= 1024 && !id.chars().any(char::is_control)
                        })
                        .map(str::to_owned),
                ),
                None => Identity::Unavailable,
            },
        };
        let profiles = parse_profiles(&body?)?;
        check(&cancelled, deadline)?;
        Ok(Read { profiles, identity })
    })
}

pub(crate) fn parse_profiles(value: &Value) -> Result<Vec<RosterProfile>, String> {
    let rows = rows(value, "profiles")?;
    let mut profiles = std::collections::BTreeMap::new();
    for row in rows {
        let name = required_str(row, "name")?.trim();
        if !valid_profile(name) {
            return Err("The gateway returned an invalid profile name".into());
        }
        let canonical_session = row
            .get("canonical_session")
            .filter(|v| !v.is_null())
            .map(|value| {
                let id = required_str(value, "id")?.to_owned();
                let resolved_id = required_str(value, "resolved_id")?.to_owned();
                valid_scope(&id)?;
                valid_scope(&resolved_id)?;
                Ok::<_, String>(CanonicalSession {
                    id,
                    resolved_id,
                    preview: value
                        .get("preview")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                })
            })
            .transpose()?;
        profiles.entry(name.to_owned()).or_insert(RosterProfile {
            name: name.into(),
            canonical_session,
            metadata: ProfileMetadata {
                resolved_label: None,
                description: row
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                display_name: row
                    .get("display_name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
                title: row
                    .get("bot_title")
                    .or_else(|| row.get("title"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .map(str::to_owned),
                ui_meta: row.get("ui_meta").filter(|v| v.is_object()).cloned(),
                has_avatar: row.get("has_avatar").and_then(Value::as_bool),
            },
        });
    }
    // A successful older gateway can omit the root profile. Failure never seeds one.
    profiles.entry("default".into()).or_insert(RosterProfile {
        name: "default".into(),
        metadata: ProfileMetadata::default(),
        canonical_session: None,
    });
    let mut result = vec![profiles.remove("default").unwrap()];
    result.extend(profiles.into_values());
    Ok(result)
}
pub(crate) fn valid_profile(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(*c, b'_' | b'-'))
}

#[cfg(test)]
mod tests;
