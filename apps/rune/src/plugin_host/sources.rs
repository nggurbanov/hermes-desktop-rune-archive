//! Verified source routes reuse the client's credential/tunnel service, never renderer credentials.
use super::*;
use crate::{
    connections::{Lease, Resolver},
    gateway::Connection,
};
use std::{collections::BTreeSet, sync::mpsc::TryRecvError};
#[derive(Clone)]
pub(super) struct Binding {
    pub id: String,
    pub connection: Connection,
    pub lease: Option<Lease>,
    pub profiles: BTreeSet<String>,
}
impl Binding {
    pub fn request(&self, request: crate::pages::PageRequest) -> crate::pages::PageRequest {
        match &self.lease {
            Some(lease) => request.lease(lease.clone()),
            None => request.connection(self.connection.clone()),
        }
    }
}
struct Source {
    label: String,
    kind: &'static str,
    saved: Option<(String, String)>,
    profiles: Vec<Value>,
    connection: Option<Connection>,
    lease: Option<Lease>,
    pending: Option<Receiver<Result<Lease, String>>>,
    error: Option<String>,
    retired: bool,
    attempted: std::time::Instant,
}
pub(super) struct Sources {
    pub primary: String,
    rows: BTreeMap<String, Source>,
    resolver: Option<Resolver>,
}
impl Sources {
    pub fn new(
        connection: Connection,
        id: String,
        profiles: Vec<Value>,
        resolver: Option<Resolver>,
    ) -> Result<Self, String> {
        let saved = match &connection {
            Connection::Saved { id, revision, .. } => Some((id.clone(), revision.clone())),
            _ => None,
        };
        let kind = if matches!(connection.transport(), Connection::Local { .. }) {
            "local"
        } else {
            "remote"
        };
        let mut result = Self {
            primary: id.clone(),
            rows: BTreeMap::from([(
                id,
                Source {
                    label: if kind == "local" {
                        "This device"
                    } else {
                        "Connected agent"
                    }
                    .into(),
                    kind,
                    saved,
                    profiles,
                    connection: Some(connection),
                    lease: None,
                    pending: None,
                    error: None,
                    retired: false,
                    attempted: std::time::Instant::now(),
                },
            )]),
            resolver,
        };
        result.refresh()?;
        // Retaining the current saved source promotes its owned foreground SSH forward.
        // It must finish before a cross-source commit can release the foreground pin.
        let primary = result.primary.clone();
        let _ = result.ensure(&primary);
        Ok(result)
    }
    pub fn refresh(&mut self) -> Result<(), String> {
        let saved = crate::connections::plugin::routes()?;
        let valid = saved
            .iter()
            .map(|row| (row.id.clone(), row.revision.clone()))
            .collect::<BTreeSet<_>>();
        for source in self.rows.values_mut() {
            if source
                .saved
                .as_ref()
                .is_some_and(|key| !valid.contains(key))
            {
                source.retired = true;
                source.error = Some("The saved source route changed or was removed".into());
                source.connection = None;
                source.pending = None;
            }
        }
        for row in saved {
            if let Some((_, source)) = self.rows.iter_mut().find(|(_, source)| {
                source.saved.as_ref() == Some(&(row.id.clone(), row.revision.clone()))
            }) {
                source.label = row.label;
                source.kind = row.kind;
                continue;
            }
            let id = format!("saved:{}:{}", row.id, row.revision);
            self.rows.entry(id).or_insert(Source {
                label: row.label,
                kind: row.kind,
                saved: Some((row.id, row.revision)),
                profiles: vec![],
                connection: None,
                lease: None,
                pending: None,
                error: None,
                retired: false,
                attempted: std::time::Instant::now(),
            });
        }
        Ok(())
    }
    pub fn poll(&mut self) {
        for source in self.rows.values_mut() {
            if let Some(pending) = &source.pending {
                let result = match pending.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Disconnected) => {
                        Some(Err("The saved source resolver closed".into()))
                    }
                    Err(TryRecvError::Empty) => None,
                };
                if let Some(result) = result {
                    source.pending = None;
                    match result {
                        Ok(lease) => {
                            source.profiles=lease.profiles().iter().map(|p|json!({"name":p.name,"display_name":p.label,"description":p.description})).collect();
                            source.connection = Some(lease.connection().clone());
                            source.lease = Some(lease);
                            source.error = None;
                        }
                        Err(error) => source.error = Some(error),
                    }
                }
            }
            if let Some(lease) = &source.lease {
                if !lease.is_valid() {
                    source.retired = true;
                    source.error = Some(lease.error().unwrap_or("The saved source changed".into()));
                    source.connection = None;
                }
            }
        }
    }
    pub fn ensure(&mut self, id: &str) -> Result<Option<Binding>, String> {
        self.poll();
        let source = self
            .rows
            .get_mut(id)
            .ok_or("The plugin selected an unknown saved-source route")?;
        if let Some(error) = &source.error {
            if source.retired || source.attempted.elapsed() < std::time::Duration::from_secs(30) {
                return Err(error.clone());
            }
            source.error = None;
        }
        if source.pending.is_some() {
            return Ok(None);
        }
        if source.saved.is_some() && source.lease.is_none() {
            let (saved, revision) = source.saved.as_ref().unwrap();
            source.attempted = std::time::Instant::now();
            source.pending = Some(
                self.resolver
                    .as_ref()
                    .ok_or("The saved connection service is unavailable")?
                    .resolve(saved, Some(revision.clone()))?,
            );
            return Ok(None);
        }
        Ok(Some(Binding {
            id: id.into(),
            connection: source
                .connection
                .clone()
                .ok_or("The source is disconnected")?,
            lease: source.lease.clone(),
            profiles: source
                .profiles
                .iter()
                .filter_map(|p| p["name"].as_str().map(str::to_owned))
                .collect(),
        }))
    }
    pub fn update_profiles(&mut self, id: &str, profiles: Vec<Value>) {
        if let Some(source) = self.rows.get_mut(id) {
            source.profiles = profiles;
        }
    }
    pub fn profiles(&self, id: &str) -> Vec<Value> {
        self.rows
            .get(id)
            .map(|source| source.profiles.clone())
            .unwrap_or_default()
    }
    pub fn metadata(&mut self) -> Result<Value, String> {
        self.refresh()?;
        self.poll();
        let rows:Vec<_>=self.rows.iter().map(|(id,source)|json!({"id":id,"label":source.label,"kind":source.kind,"mode":if source.kind=="local" {"local"} else {"remote"},"attached":source.connection.is_some()&&source.error.is_none(),"profiles":source.profiles,"error":source.error})).collect();
        let current = rows
            .iter()
            .find(|row| row["id"] == self.primary)
            .cloned()
            .ok_or("The primary plugin route was retired")?;
        Ok(json!({"current":current,"connections":rows,"primary":self.primary}))
    }
    pub fn roster(&mut self) -> Result<Option<Value>, String> {
        self.refresh()?;
        let ids = self
            .rows
            .iter()
            .filter(|(_, s)| s.kind != "ssh")
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut pending = false;
        for id in ids {
            if matches!(self.ensure(&id), Ok(None)) {
                pending = true;
            }
        }
        if pending {
            Ok(None)
        } else {
            self.metadata().map(Some)
        }
    }
    pub fn descriptor(binding: &Binding, profile: &str) -> Result<Value, String> {
        if !binding.profiles.contains(profile) {
            return Err("The selected source does not serve that profile".into());
        }
        Ok(
            json!({"baseUrl":"http://native-host.invalid","token":"","authMode":"native","mode":if matches!(binding.connection.transport(),Connection::Local{..}) {"local"} else {"remote"},"profile":profile,"connectionId":binding.id}),
        )
    }
    pub fn bindings(&mut self) -> Vec<Binding> {
        self.poll();
        self.rows
            .iter()
            .filter_map(|(id, s)| {
                (s.error.is_none() && s.pending.is_none())
                    .then(|| {
                        s.connection.clone().map(|connection| Binding {
                            id: id.clone(),
                            connection,
                            lease: s.lease.clone(),
                            profiles: s
                                .profiles
                                .iter()
                                .filter_map(|p| p["name"].as_str().map(str::to_owned))
                                .collect(),
                        })
                    })
                    .flatten()
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source(label: &str, endpoint: &str, token: &str) -> Source {
        Source {
            label: label.into(),
            kind: "remote",
            saved: None,
            profiles: vec![json!({"name":"same-name"})],
            connection: Some(Connection::Url {
                endpoint: endpoint.into(),
                token: Some(token.into()),
            }),
            lease: None,
            pending: None,
            error: None,
            retired: false,
            attempted: std::time::Instant::now(),
        }
    }
    #[test]
    fn duplicate_names_keep_exact_sources_and_never_fall_back_to_the_primary() {
        let mut sources = Sources {
            primary: "source-a".into(),
            rows: BTreeMap::from([
                (
                    "source-a".into(),
                    source("A", "http://a.invalid/private", "synthetic-a"),
                ),
                (
                    "source-b".into(),
                    source("B", "http://b.invalid/private", "synthetic-b"),
                ),
            ]),
            resolver: None,
        };
        assert!(sources.ensure("unknown").is_err());
        let target = sources.ensure("source-b").unwrap().unwrap();
        let request =
            target.request(crate::pages::PageRequest::get("/api/config").profile("same-name"));
        assert!(
            matches!(request.connection,Some(Connection::Url{ref endpoint,ref token})if endpoint=="http://b.invalid/private"&&token.as_deref()==Some("synthetic-b"))
        );
        let descriptor = Sources::descriptor(&target, "same-name").unwrap();
        assert_eq!(descriptor["connectionId"], "source-b");
        assert!(!descriptor.to_string().contains("b.invalid"));
        assert!(!descriptor.to_string().contains("synthetic-b"));
        assert!(Sources::descriptor(&target, "unserved").is_err());
    }
    #[test]
    fn an_unverified_saved_source_cannot_use_its_cached_connection_or_another_principal() {
        let mut stale = source("Saved", "http://cached.invalid", "synthetic-token");
        stale.saved = Some(("saved-id".into(), "revision".into()));
        let mut sources = Sources {
            primary: "source-a".into(),
            rows: BTreeMap::from([
                ("source-a".into(), source("A", "http://a.invalid", "a")),
                ("source-b".into(), stale),
            ]),
            resolver: None,
        };
        assert!(sources.ensure("source-b").is_err());
        sources.rows.get_mut("source-b").unwrap().retired = true;
        sources.rows.get_mut("source-b").unwrap().error = Some("The route changed".into());
        sources.rows.get_mut("source-b").unwrap().attempted =
            std::time::Instant::now() - std::time::Duration::from_secs(100);
        assert!(matches!(sources.ensure("source-b"),Err(error)if error.contains("route changed")));
        assert!(sources.ensure("source-a").unwrap().is_some());
    }
}
