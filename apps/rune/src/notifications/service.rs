#[cfg(not(target_os = "linux"))]
use super::types::Availability;
use super::types::{Reply, Request};
use rune::Waker;
use std::{
    collections::BTreeMap,
    sync::{OnceLock, Weak, mpsc::Sender},
};
use tokio::sync::mpsc;

pub(super) enum Command {
    Register {
        client: u64,
        replies: Sender<Reply>,
        waker: Option<Waker>,
        primary: bool,
        alive: Weak<()>,
    },
    Unregister {
        client: u64,
    },
    Probe {
        client: u64,
    },
    Show(Request),
}
#[derive(Clone)]
pub(super) struct Client {
    pub replies: Sender<Reply>,
    pub waker: Option<Waker>,
    pub primary: bool,
    pub alive: Weak<()>,
}
impl Client {
    pub fn send(&self, reply: Reply) -> bool {
        let sent = self.replies.send(reply).is_ok();
        if sent && let Some(waker) = &self.waker {
            waker();
        }
        sent
    }
}
pub(super) type Clients = BTreeMap<u64, Client>;
static SHARED: OnceLock<mpsc::Sender<Command>> = OnceLock::new();
pub(super) fn send(command: Command) -> Result<(), Command> {
    let sender = SHARED.get_or_init(|| {
        let (sender, receiver) = mpsc::channel(32);
        std::thread::Builder::new()
            .name("hermes-notifications".into())
            .spawn(move || run(receiver))
            .expect("notification worker thread");
        sender
    });
    sender.try_send(command).map_err(|error| error.into_inner())
}
#[allow(unused_mut)]
fn run(mut receiver: mpsc::Receiver<Command>) {
    #[cfg(target_os = "linux")]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("notification runtime");
        runtime.block_on(super::linux::run(receiver));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut clients = Clients::new();
        while let Some(command) = receiver.blocking_recv() {
            match command {
                Command::Register {
                    client,
                    replies,
                    waker,
                    primary,
                    alive,
                } => {
                    clients.insert(
                        client,
                        Client {
                            replies,
                            waker,
                            primary,
                            alive,
                        },
                    );
                }
                Command::Unregister { client } => {
                    clients.remove(&client);
                }
                Command::Probe { client } => {
                    if let Some(target) = clients.get(&client) {
                        target.send(Reply::Availability(Availability::Unavailable("Native notification delivery has not been implemented on this platform.".into())));
                    }
                }
                Command::Show(request) => {
                    if let Some(target) = clients.get(&request.client) {
                        target.send(Reply::Delivered { id: request.id, result: Err("Native notification delivery has not been implemented on this platform.".into()) });
                    }
                }
            }
        }
    }
}
