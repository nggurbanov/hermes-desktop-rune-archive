//! Background reads never unlock, prompt, create, mutate or activate a keychain.
use std::time::Instant;

pub(in crate::connections) fn read(id: &str, deadline: Instant) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        read_at(None, id, deadline)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (id, deadline);
        Err("Saved credentials are unavailable for passive reads on this platform; connect explicitly first".into())
    }
}
#[cfg(target_os = "linux")]
const SERVICE: &str = "org.freedesktop.secrets";
#[cfg(target_os = "linux")]
const ROOT: &str = "/org/freedesktop/secrets";
#[cfg(target_os = "linux")]
#[derive(serde::Deserialize, serde::Serialize, zbus::zvariant::Type)]
struct Secret {
    session: zbus::zvariant::OwnedObjectPath,
    parameters: Vec<u8>,
    value: Vec<u8>,
    content_type: String,
}

#[cfg(target_os = "linux")]
fn read_at(address: Option<&str>, id: &str, deadline: Instant) -> Result<String, String> {
    let timeout = deadline
        .checked_duration_since(Instant::now())
        .ok_or("Passive credential deadline expired")?;
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err("The saved credential identity is invalid".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Passive credential transport is unavailable")?;
    runtime.block_on(async {
        tokio::time::timeout(timeout, read_async(address,id,timeout)).await.map_err(|_| "Passive credential read timed out; connect explicitly to recover the saved credential".to_owned())?
    })
}
#[cfg(target_os = "linux")]
async fn read_async(
    address: Option<&str>,
    id: &str,
    timeout: std::time::Duration,
) -> Result<String, String> {
    use std::collections::HashMap;
    use zbus::{
        Proxy,
        zvariant::{OwnedObjectPath, OwnedValue, Value},
    };
    let builder = match address {
        Some(address) => zbus::connection::Builder::address(address),
        None => zbus::connection::Builder::session(),
    }
    .map_err(|_| "The credential session bus is unavailable")?;
    let connection = builder
        .max_queued(32)
        .method_timeout(timeout)
        .build()
        .await
        .map_err(|_| "The credential session bus is unavailable")?;
    // Pin an already-running owner. Do not activate an absent keychain service.
    let bus = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await
    .map_err(|_| "The credential session bus is unavailable")?;
    let owner: zbus::names::OwnedUniqueName = bus
        .call("GetNameOwner", &(SERVICE,))
        .await
        .map_err(|_| "The credential store is not running; connect explicitly first")?;
    let service = Proxy::new(
        &connection,
        owner.as_str(),
        ROOT,
        "org.freedesktop.Secret.Service",
    )
    .await
    .map_err(|_| "The passive credential service is unavailable")?;
    // These are exactly keyring3 SsCredential's search attributes for Entry::new.
    let attributes = HashMap::from([
        ("service", "Hermes Rune"),
        ("username", id),
        ("target", "default"),
    ]);
    let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) = service
        .call("SearchItems", &(attributes,))
        .await
        .map_err(|_| "The saved credential could not be inspected")?;
    if !locked.is_empty() {
        return Err("The saved credential is locked; connect explicitly to unlock it".into());
    }
    if unlocked.len() != 1 {
        return Err(if unlocked.is_empty() {
            "The saved credential is missing; edit the connection to recover it"
        } else {
            "The saved credential is ambiguous; edit the connection to recover it"
        }
        .into());
    }
    let (_, session): (OwnedValue, OwnedObjectPath) = service
        .call("OpenSession", &("plain", Value::from("")))
        .await
        .map_err(|_| "The passive credential session could not be opened")?;
    let items = vec![unlocked[0].clone()];
    let result: Result<HashMap<OwnedObjectPath, Secret>, _> =
        service.call("GetSecrets", &(items, session.clone())).await;
    let result = result
        .map_err(|_| "The unlocked saved credential could not be read".to_owned())
        .and_then(|mut secrets| {
            if secrets.len() != 1 {
                return Err("The credential store returned an unrelated item".into());
            }
            let secret = secrets
                .remove(&unlocked[0])
                .ok_or("The credential store changed the requested item")?;
            if secret.session != session
                || !secret.parameters.is_empty()
                || secret.value.is_empty()
                || secret.value.len() > 16384
            {
                return Err("The saved credential response is invalid".into());
            }
            let token = String::from_utf8(secret.value)
                .map_err(|_| "The saved credential is not valid text")?;
            if token.chars().any(char::is_control) {
                return Err("The saved credential contains invalid characters".into());
            }
            Ok(token)
        });
    if let Ok(session_proxy) = Proxy::new(
        &connection,
        owner.as_str(),
        session.as_str(),
        "org.freedesktop.Secret.Session",
    )
    .await
    {
        let _: Result<(), _> = session_proxy.call("Close", &()).await;
    }
    result
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
