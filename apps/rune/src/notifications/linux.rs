//! One asynchronous broker owns all native side effects across Rune windows.
use super::{
    registry::Registry,
    service::{Client, Clients, Command},
    transport::{self, Binding, Delivery, Failure, Session},
    types::{Availability, Reply, Request},
};
use futures_util::StreamExt;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinSet};

#[derive(Clone)]
pub(super) struct Config {
    pub address: Option<String>,
    pub timeout: Duration,
    pub cooldown: Duration,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            address: None,
            timeout: Duration::from_secs(5),
            cooldown: Duration::from_secs(10),
        }
    }
}
struct Duplicate {
    until: Instant,
    result: Option<Result<(), String>>,
    waiters: Vec<(u64, u64)>,
}
enum Finished {
    Probe {
        client: u64,
        result: Result<bool, String>,
    },
    Show {
        request: Request,
        result: Result<Delivery, Failure>,
    },
}
struct Broker {
    clients: Clients,
    registry: Registry,
    duplicates: HashMap<String, Duplicate>,
    session: Option<Session>,
    tasks: JoinSet<Finished>,
    pending: usize,
    probes: HashSet<u64>,
    retry: Option<(Option<Binding>, Instant)>,
    config: Config,
}
impl Broker {
    fn new(config: Config) -> Self {
        Self {
            clients: Clients::new(),
            registry: Registry::new(),
            duplicates: HashMap::new(),
            session: None,
            tasks: JoinSet::new(),
            pending: 0,
            probes: HashSet::new(),
            retry: None,
            config,
        }
    }
    fn reply(&self, client: u64, reply: Reply) {
        if let Some(client) = self.clients.get(&client) {
            client.send(reply);
        }
    }
    fn availability(&self, status: Availability) {
        for client in self.clients.values() {
            client.send(Reply::Availability(status.clone()));
        }
    }
    async fn connect(&mut self) -> Result<(), String> {
        if self.session.is_none() {
            self.session =
                Some(Session::connect(self.config.address.as_deref(), self.config.timeout).await?);
        }
        Ok(())
    }
    async fn command(&mut self, command: Command) {
        match command {
            Command::Register {
                client,
                replies,
                waker,
                primary,
                alive,
            } => {
                self.clients.insert(
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
                self.clients.remove(&client);
                if self.clients.is_empty() {
                    self.release();
                }
            }
            Command::Probe { client } => self.probe(client).await,
            Command::Show(request) => self.show(request).await,
        }
    }
    fn release(&mut self) {
        self.tasks.abort_all();
        self.pending = 0;
        self.probes.clear();
        self.duplicates.clear();
        if let Some(session) = self.session.take() {
            for (owner, id) in self.registry.live.keys().cloned().collect::<Vec<_>>() {
                transport::close(&session.connection, owner, id, self.config.timeout);
            }
        }
        self.registry = Registry::new();
    }
    async fn probe(&mut self, client: u64) {
        if self.probes.contains(&client) {
            return;
        }
        if let Err(error) = self.connect().await {
            self.reply(
                client,
                Reply::Availability(Availability::Unavailable(error)),
            );
            return;
        }
        self.probes.insert(client);
        let session = self.session.as_ref().unwrap();
        let (connection, owner, timeout) = (
            session.connection.clone(),
            session.owner.clone(),
            self.config.timeout,
        );
        self.tasks.spawn(async move {
            Finished::Probe {
                client,
                result: transport::probe(connection, owner, timeout).await,
            }
        });
    }
    fn cooling(&self, now: Instant) -> bool {
        self.retry.as_ref().is_some_and(|(binding, until)| {
            now < *until
                && match binding {
                    None => true,
                    Some(binding) => self
                        .session
                        .as_ref()
                        .is_some_and(|s| s.owner.lock().unwrap().matches(binding)),
                }
        })
    }
    fn duplicate(&mut self, request: &Request, now: Instant) -> bool {
        self.duplicates
            .retain(|_, item| item.result.is_none() || now < item.until);
        if request.test {
            return false;
        }
        let Some(item) = self.duplicates.get_mut(&request.key()) else {
            return false;
        };
        if let Some(result) = &item.result {
            let result = result.clone();
            self.reply(
                request.client,
                Reply::Delivered {
                    id: request.id,
                    result,
                },
            );
        } else if item.waiters.len() < 32 {
            item.waiters.push((request.client, request.id));
        } else {
            self.reply(
                request.client,
                Reply::Delivered {
                    id: request.id,
                    result: Err("Too many duplicate notification deliveries are pending".into()),
                },
            );
        }
        true
    }
    async fn show(&mut self, request: Request) {
        if !self.clients.contains_key(&request.client) {
            return;
        }
        let now = Instant::now();
        if self.duplicate(&request, now) {
            return;
        }
        let error = if self.pending >= 32 {
            Some("Too many desktop notifications are pending".into())
        } else if self.cooling(now) {
            Some("The desktop notification service is temporarily unavailable; no notification was retried.".into())
        } else {
            self.connect().await.err()
        };
        if let Some(error) = error {
            self.reply(
                request.client,
                Reply::Delivered {
                    id: request.id,
                    result: Err(error),
                },
            );
            return;
        }
        if !request.test {
            self.duplicates.insert(
                request.key(),
                Duplicate {
                    until: now + Duration::from_secs(1),
                    result: None,
                    waiters: vec![],
                },
            );
        }
        self.pending += 1;
        let session = self.session.as_ref().unwrap();
        let (connection, owner, timeout) = (
            session.connection.clone(),
            session.owner.clone(),
            self.config.timeout,
        );
        self.tasks.spawn(async move {
            let result = transport::deliver(connection, owner, &request, timeout).await;
            Finished::Show { request, result }
        });
    }
    fn finished(&mut self, finished: Finished) {
        match finished {
            Finished::Probe { client, result } => {
                self.probes.remove(&client);
                self.reply(
                    client,
                    Reply::Availability(match result {
                        Ok(actions) => Availability::Ready { actions },
                        Err(error) => Availability::Unavailable(error),
                    }),
                );
            }
            Finished::Show { request, result } => {
                self.pending = self.pending.saturating_sub(1);
                let outcome = match result {
                    Ok(delivery) => self.delivered(&request, delivery),
                    Err(failure) => {
                        if failure.binding.as_ref().is_none_or(|b| {
                            self.session
                                .as_ref()
                                .is_some_and(|s| s.owner.lock().unwrap().matches(b))
                        }) {
                            self.retry =
                                Some((failure.binding, Instant::now() + self.config.cooldown));
                        }
                        Err(failure.text)
                    }
                };
                self.reply(
                    request.client,
                    Reply::Delivered {
                        id: request.id,
                        result: outcome.clone(),
                    },
                );
                if !request.test
                    && let Some(item) = self.duplicates.get_mut(&request.key())
                {
                    item.result = Some(outcome.clone());
                    let waiters = std::mem::take(&mut item.waiters);
                    for (client, id) in waiters {
                        self.reply(
                            client,
                            Reply::Delivered {
                                id,
                                result: outcome.clone(),
                            },
                        );
                    }
                }
            }
        }
    }
    fn delivered(&mut self, request: &Request, delivery: Delivery) -> Result<(), String> {
        let Some(session) = self.session.as_ref() else {
            return Err("The desktop session disconnected during delivery".into());
        };
        if !session.owner.lock().unwrap().matches(&delivery.binding) {
            transport::close(
                &session.connection,
                delivery.binding.owner,
                delivery.id,
                self.config.timeout,
            );
            return Err("The desktop notification service changed during delivery".into());
        }
        self.retry = None;
        self.availability(Availability::Ready {
            actions: delivery.actions,
        });
        let close = self.registry.insert(
            delivery.binding.owner.clone(),
            delivery.id,
            request.clone(),
            delivery.actions,
            &self.clients,
            Instant::now(),
        );
        if close {
            transport::close(
                &session.connection,
                delivery.binding.owner,
                delivery.id,
                self.config.timeout,
            );
        }
        Ok(())
    }
    fn message(&mut self, message: zbus::Message) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some((owner, id, signal)) = session.signal(&message) else {
            return;
        };
        if id == 0 {
            self.registry.retire_owner(&owner);
            return;
        }
        // The registry matches the sender AND numeric id. A different peer cannot invoke an action.
        let close = self.registry.receive(
            &owner,
            id,
            signal,
            &self.clients,
            self.pending > 0,
            Instant::now(),
        );
        if close {
            transport::close(&session.connection, owner, id, self.config.timeout);
        }
    }
    fn disconnected(&mut self) {
        self.session = None;
        self.registry = Registry::new();
        self.availability(Availability::Unavailable(
            "The desktop session bus disconnected".into(),
        ));
    }
    fn expire(&mut self) {
        self.clients
            .retain(|_, client| client.alive.strong_count() > 0);
        if self.clients.is_empty() {
            self.release();
            return;
        }
        if let Some(session) = &self.session {
            for (owner, id) in self.registry.expired(Instant::now()) {
                transport::close(&session.connection, owner, id, self.config.timeout);
            }
        }
    }
}
pub(super) async fn run(receiver: mpsc::Receiver<Command>) {
    run_with(receiver, Config::default()).await;
}
pub(super) async fn run_with(mut receiver: mpsc::Receiver<Command>, config: Config) {
    let mut broker = Broker::new(config);
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        if broker.session.is_none() {
            tokio::select! {
                command = receiver.recv() => match command { Some(command) => broker.command(command).await, None => break },
                result = broker.tasks.join_next(), if !broker.tasks.is_empty() => { if let Some(Ok(finished)) = result { broker.finished(finished); } },
                _ = interval.tick() => broker.expire(),
            }
            continue;
        }
        tokio::select! {
            command = receiver.recv() => match command { Some(command) => broker.command(command).await, None => break },
            message = broker.session.as_mut().unwrap().messages.next() => match message {
                Some(Ok(message)) => broker.message(message),
                _ => broker.disconnected(),
            },
            result = broker.tasks.join_next(), if !broker.tasks.is_empty() => { if let Some(Ok(finished)) = result { broker.finished(finished); } },
            _ = interval.tick() => broker.expire(),
        }
    }
    broker.tasks.abort_all();
    if let Some(session) = broker.session.take() {
        for (owner, id) in broker.registry.live.keys().cloned().collect::<Vec<_>>() {
            transport::close(&session.connection, owner, id, broker.config.timeout);
        }
    }
}
