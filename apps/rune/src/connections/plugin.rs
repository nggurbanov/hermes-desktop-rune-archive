//! Credential-free metadata for executable desktop extensions; reads never unlock a keychain.
use super::*;
use serde_json::{Value, json};
pub(crate) struct SavedRoute {
    pub id: String,
    pub revision: String,
    pub label: String,
    pub kind: &'static str,
}
pub(crate) fn routes() -> Result<Vec<SavedRoute>, String> {
    Ok(
        core::read_registry(crate::client::state_dir()?.join("connections.json"))?
            .rows
            .iter()
            .map(|row| SavedRoute {
                id: row.id.clone(),
                revision: row.revision.clone(),
                label: row.label.clone(),
                kind: kind_of(row),
            })
            .collect(),
    )
}
pub(crate) fn metadata(connection: &Connection, opaque: &str) -> Result<Value, String> {
    let registry = core::read_registry(crate::client::state_dir()?.join("connections.json"))?;
    from_rows(connection, opaque, &registry.rows)
}
fn from_rows(connection: &Connection, opaque: &str, rows: &[Saved]) -> Result<Value, String> {
    let active = match connection {
        Connection::Saved { id, revision, .. } => {
            let row = rows
                .iter()
                .find(|row| row.id == *id)
                .ok_or("The plugin's saved source was removed; reconnect it")?;
            if row.revision != *revision {
                return Err("The plugin's saved source changed; reconnect it".into());
            }
            Some(row)
        }
        _ => None,
    };
    let local = matches!(connection.transport(), Connection::Local { .. });
    let mode = if local { "local" } else { "remote" };
    let label = active.map(|row| row.label.as_str()).unwrap_or(if local {
        "This device"
    } else {
        "Connected agent"
    });
    let kind = active.map(|row| kind(row.kind())).unwrap_or(mode);
    let current = json!({"id":opaque,"connectionId":opaque,"label":label,"kind":kind,"mode":mode});
    let mut connections: Vec<_> = rows
        .iter()
        .map(|row| {
            let id = if active.is_some_and(|active| active.id == row.id) {
                opaque
            } else {
                &row.id
            };
            json!({"id":id,"label":row.label,"kind":kind_of(row),"attached":id==opaque})
        })
        .collect();
    if !connections.iter().any(|row| row["id"] == opaque) {
        connections.push(json!({"id":opaque,"label":label,"kind":kind,"attached":true}));
    }
    Ok(json!({"current":current,"connections":connections,"primary":opaque}))
}
fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Local => "local",
        Kind::Remote => "remote",
        Kind::Ssh => "ssh",
    }
}
fn kind_of(row: &Saved) -> &'static str {
    kind(row.kind())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_redacts_endpoints_paths_and_credential_identifiers_and_keeps_the_current_alias() {
        let id = uuid::Uuid::new_v4().to_string();
        let revision = uuid::Uuid::new_v4().to_string();
        let row = Saved {
            id: id.clone(),
            revision: revision.clone(),
            label: "Synthetic device".into(),
            destination: Destination::Remote {
                url: "https://synthetic.invalid/private-endpoint".into(),
            },
            credential: Some("synthetic-secret-slot".into()),
        };
        let connection = Connection::Saved {
            id,
            revision,
            inner: Box::new(Connection::Url {
                endpoint: "https://synthetic.invalid/private-endpoint".into(),
                token: Some("synthetic-token".into()),
            }),
        };
        let public = from_rows(&connection, "native:opaque", &[row]).unwrap();
        let encoded = public.to_string();
        for secret in [
            "synthetic.invalid",
            "private-endpoint",
            "synthetic-secret-slot",
            "synthetic-token",
        ] {
            assert!(!encoded.contains(secret));
        }
        assert_eq!(public["primary"], "native:opaque");
        assert_eq!(public["connections"][0]["id"], "native:opaque");
        assert_eq!(public["current"]["label"], "Synthetic device");
    }
    #[test]
    fn a_saved_revision_change_cannot_relabel_an_old_authenticated_source() {
        let row = Saved {
            id: "synthetic".into(),
            revision: "new".into(),
            label: "New account".into(),
            destination: Destination::Remote {
                url: "https://synthetic.invalid".into(),
            },
            credential: None,
        };
        let connection = Connection::Saved {
            id: "synthetic".into(),
            revision: "old".into(),
            inner: Box::new(Connection::Url {
                endpoint: "https://synthetic.invalid".into(),
                token: None,
            }),
        };
        assert!(from_rows(&connection, "native:opaque", &[row]).is_err());
    }
}
