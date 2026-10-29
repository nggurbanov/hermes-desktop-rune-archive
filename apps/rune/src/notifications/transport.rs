//! Native Linux delivery binds every numeric notification id to a daemon's unique bus owner.
use super::{registry::Signal, types::Request};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use zbus::{Connection, Message, MessageStream, message::Sequence, zvariant::Value};
pub(super) const SERVICE: &str = "org.freedesktop.Notifications";
const DBUS: &str = "org.freedesktop.DBus";
const PATH: &str = "/org/freedesktop/Notifications";
#[derive(Clone, Debug)]
pub(super) struct Binding {
    pub owner: String,
    pub generation: u64,
}
#[derive(Default)]
pub(super) struct Owner {
    owner: String,
    sequence: Sequence,
    generation: u64,
}
impl Owner {
    pub fn observe(&mut self, owner: String, sequence: Sequence) {
        if sequence < self.sequence {
            return;
        }
        self.sequence = sequence;
        if self.owner != owner {
            self.generation += 1;
            self.owner = owner;
        }
    }
    pub fn binding(&self, owner: &str) -> Option<Binding> {
        (self.owner == owner).then(|| Binding {
            owner: owner.into(),
            generation: self.generation,
        })
    }
    pub fn matches(&self, binding: &Binding) -> bool {
        self.owner == binding.owner && self.generation == binding.generation
    }
}
pub(super) struct Session {
    pub connection: Connection,
    pub messages: MessageStream,
    pub owner: Arc<Mutex<Owner>>,
}
pub(super) struct Delivery {
    pub id: u32,
    pub binding: Binding,
    pub actions: bool,
}
pub(super) struct Failure {
    pub text: String,
    pub binding: Option<Binding>,
}
impl Session {
    pub async fn connect(address: Option<&str>, timeout: Duration) -> Result<Self, String> {
        let connect = async {
            let builder = match address {
                Some(address) => zbus::connection::Builder::address(address),
                None => zbus::connection::Builder::session(),
            }
            .map_err(|_| "The desktop session bus is unavailable")?;
            let connection = builder
                .max_queued(256)
                .method_timeout(timeout)
                .build()
                .await
                .map_err(|_| "Could not connect to the desktop session bus")?;
            let messages = MessageStream::from(&connection);
            for rule in [
                format!("type='signal',interface='{SERVICE}',path='{PATH}'"),
                format!(
                    "type='signal',sender='{DBUS}',interface='{DBUS}',member='NameOwnerChanged',arg0='{SERVICE}'"
                ),
            ] {
                connection
                    .call_method(
                        Some(DBUS),
                        "/org/freedesktop/DBus",
                        Some(DBUS),
                        "AddMatch",
                        &(rule,),
                    )
                    .await
                    .map_err(|_| "Could not subscribe to desktop notification actions")?;
            }
            Ok(Self {
                connection,
                messages,
                owner: Arc::default(),
            })
        };
        tokio::time::timeout(timeout, connect)
            .await
            .map_err(|_| "The desktop session bus timed out".to_string())?
    }
    pub fn signal(&self, message: &Message) -> Option<(String, u32, Signal)> {
        let header = message.header();
        if header.message_type() != zbus::message::Type::Signal {
            return None;
        }
        if header.sender().is_some_and(|s| s.as_str() == DBUS)
            && header.interface().is_some_and(|s| s.as_str() == DBUS)
            && header
                .member()
                .is_some_and(|s| s.as_str() == "NameOwnerChanged")
        {
            if let Ok((name, old, new)) = message.body().deserialize::<(String, String, String)>()
                && name == SERVICE
            {
                self.owner
                    .lock()
                    .unwrap()
                    .observe(new, message.recv_position());
                return Some((old, 0, Signal::Closed));
            }
            return None;
        }
        if header.path().is_none_or(|p| p.as_str() != PATH)
            || header.interface().is_none_or(|s| s.as_str() != SERVICE)
        {
            return None;
        }
        let sender = header.sender()?.to_string();
        let member = header.member()?.as_str();
        let (id, signal) = match member {
            "ActionInvoked" => {
                let (id, action) = message.body().deserialize::<(u32, String)>().ok()?;
                (id, Signal::Action(action))
            }
            "ActivationToken" => {
                let (id, token) = message.body().deserialize::<(u32, String)>().ok()?;
                (id, Signal::Token(token))
            }
            "NotificationClosed" => {
                let (id, _) = message.body().deserialize::<(u32, u32)>().ok()?;
                (id, Signal::Closed)
            }
            _ => return None,
        };
        Some((sender, id, signal))
    }
}
async fn resolve(connection: &Connection, owner: &Arc<Mutex<Owner>>) -> Result<Binding, String> {
    let get = || {
        connection.call_method(
            Some(DBUS),
            "/org/freedesktop/DBus",
            Some(DBUS),
            "GetNameOwner",
            &(SERVICE,),
        )
    };
    let response = match get().await {
        Ok(value) => value,
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.DBus.Error.NameHasNoOwner" =>
        {
            connection
                .call_method(
                    Some(DBUS),
                    "/org/freedesktop/DBus",
                    Some(DBUS),
                    "StartServiceByName",
                    &(SERVICE, 0u32),
                )
                .await
                .map_err(|_| "No desktop notification service is available")?;
            get()
                .await
                .map_err(|_| "The desktop notification service did not start")?
        }
        Err(_) => return Err("Could not discover the desktop notification service".into()),
    };
    let name = response
        .body()
        .deserialize::<String>()
        .map_err(|_| "The desktop returned an invalid notification service owner")?;
    let mut owner = owner.lock().unwrap();
    owner.observe(name.clone(), response.recv_position());
    owner
        .binding(&name)
        .ok_or_else(|| "The desktop notification service changed during delivery".into())
}
async fn capabilities(connection: &Connection, binding: &Binding) -> Result<bool, String> {
    let response = connection
        .call_method(
            Some(binding.owner.as_str()),
            PATH,
            Some(SERVICE),
            "GetCapabilities",
            &(),
        )
        .await
        .map_err(|_| "The desktop notification service did not report its capabilities")?;
    let caps = response
        .body()
        .deserialize::<Vec<String>>()
        .map_err(|_| "The desktop notification service returned invalid capabilities")?;
    Ok(caps.iter().any(|s| s == "actions"))
}
pub(super) async fn probe(
    connection: Connection,
    owner: Arc<Mutex<Owner>>,
    timeout: Duration,
) -> Result<bool, String> {
    tokio::time::timeout(timeout, async {
        let binding = resolve(&connection, &owner).await?;
        capabilities(&connection, &binding).await
    })
    .await
    .map_err(|_| "The desktop notification service timed out".to_string())?
}
pub(super) async fn deliver(
    connection: Connection,
    owner: Arc<Mutex<Owner>>,
    request: &Request,
    timeout: Duration,
) -> Result<Delivery, Failure> {
    let mut addressed = None;
    let result = tokio::time::timeout(timeout, async {
        let binding = resolve(&connection, &owner).await?;
        addressed = Some(binding.clone());
        let actions = capabilities(&connection, &binding).await?;
        if !owner.lock().unwrap().matches(&binding) {
            return Err("The desktop notification service changed during delivery".into());
        }
        let mut buttons = vec![];
        if actions {
            buttons.extend(["default".to_string(), "View".into()]);
            if let Some(plugin) = &request.notice.plugin {
                for (index, button) in plugin.actions.iter().enumerate() {
                    buttons.extend([index.to_string(), button.label.clone()]);
                }
            } else {
                for (index, button) in request.notice.buttons.iter().enumerate() {
                    buttons.extend([index.to_string(), button.label.clone()]);
                }
            }
        }
        let hints: HashMap<&str, Value<'_>> = HashMap::from([
            ("urgency", Value::U8(1)),
            ("desktop-entry", Value::Str("hermes".into())),
            ("suppress-sound", Value::Bool(request.notice.silent)),
        ]);
        let response = connection
            .call_method(
                Some(binding.owner.as_str()),
                PATH,
                Some(SERVICE),
                "Notify",
                &(
                    "Hermes",
                    0u32,
                    "",
                    &request.notice.title,
                    &request.notice.body,
                    buttons,
                    hints,
                    -1i32,
                ),
            )
            .await
            .map_err(|error| match error {
                zbus::Error::MethodError(_, _, _) => {
                    "The desktop notification service rejected the notification"
                }
                _ => "Native delivery is unconfirmed and will not be retried.",
            })?;
        let id = response
            .body()
            .deserialize::<u32>()
            .map_err(|_| "The desktop returned an invalid notification id")?;
        if id == 0 {
            return Err("The desktop returned an invalid notification id".into());
        }
        Ok(Delivery {
            id,
            binding,
            actions,
        })
    })
    .await;
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(text)) => Err(Failure { text, binding: addressed }),
        Err(_) => Err(Failure { text: "The desktop notification service timed out; delivery is unconfirmed and will not be retried.".into(), binding: addressed }),
    }
}
pub(super) fn close(connection: &Connection, owner: String, id: u32, timeout: Duration) {
    let connection = connection.clone();
    tokio::spawn(async move {
        let _ = tokio::time::timeout(
            timeout,
            connection.call_method(
                Some(owner.as_str()),
                PATH,
                Some(SERVICE),
                "CloseNotification",
                &(id,),
            ),
        )
        .await;
    });
}
