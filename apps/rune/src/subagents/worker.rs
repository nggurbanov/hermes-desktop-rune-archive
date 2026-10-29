//! A spectator has no prompt, admission, approval, stop or mutation channel.
use super::*;
use crate::gateway::{
    History, HistoryMessage,
    conversation::{Event, Snapshot},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(crate) struct Projection {
    pub messages: Vec<HistoryMessage>,
    pub tools: Vec<String>,
    pub reasoning: String,
    pub running: bool,
    pub error: Option<String>,
    pub connected: bool,
}
impl Projection {
    fn from_view(view: &ConversationView, connected: bool) -> Self {
        Self {
            messages: view.messages.clone(),
            tools: view.tools.clone(),
            reasoning: view.reasoning.clone(),
            running: view.running,
            error: view.error.clone(),
            connected,
        }
    }
}
pub(super) struct Watch {
    pub receive: Receiver<Projection>,
    stop: Arc<AtomicBool>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
pub(super) fn start(connection: Connection, profile: String, child: String, waker: Waker) -> Watch {
    let (send, receive) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::spawn(move || run(connection, profile, child, send, waker, stopped));
    Watch { receive, stop }
}
fn publish(
    send: &Sender<Projection>,
    waker: &Waker,
    view: &ConversationView,
    connected: bool,
) -> bool {
    if send.send(Projection::from_view(view, connected)).is_err() {
        return false;
    }
    waker();
    true
}
fn install(
    view: &mut ConversationView,
    transport: &mut SessionTransport,
    snapshot: Snapshot,
) -> Result<(), String> {
    let messages = snapshot.messages.clone();
    let history = if messages.is_empty() {
        transport.history(&snapshot.stored).map_err(|e| e.message)?
    } else {
        let cursor = messages.len();
        History {
            messages,
            has_more: false,
            cursor,
        }
    };
    view.install(snapshot, history);
    Ok(())
}
fn resume(
    view: &mut ConversationView,
    transport: &mut SessionTransport,
    child: &str,
) -> Result<(), String> {
    let snapshot = transport.watch(child).map_err(|e| e.message)?;
    if view.canonical
        && !view.runtime.is_empty()
        && view.runtime == snapshot.runtime
        && view.replay_epoch == snapshot.epoch
        && view.authority == snapshot.authority
    {
        if let Some(mut events) = transport
            .replay(&view.runtime, &view.replay_epoch, view.last_sequence)
            .map_err(|e| e.message)?
        {
            events.extend(transport.take_queued());
            events.sort_by_key(|e| e.sequence);
            events.dedup_by_key(|e| e.sequence);
            let mut last = view.last_sequence;
            let mut contiguous = true;
            for event in &events {
                if event.session == view.runtime
                    && event.epoch.as_deref() == Some(view.replay_epoch.as_str())
                    && event.sequence.is_some_and(|n| n > last)
                {
                    if event.sequence != Some(last + 1) {
                        contiguous = false;
                        break;
                    }
                    last += 1;
                }
            }
            if contiguous && last >= snapshot.sequence {
                for event in events {
                    view.apply_event(&event);
                }
                view.running = snapshot.running;
                view.metadata = snapshot.metadata;
                view.error = None;
                return Ok(());
            }
        }
    }
    install(view, transport, snapshot)
}
fn deliver(view: &mut ConversationView, event: Event) -> bool {
    view.apply_event(&event)
}
fn run(
    connection: Connection,
    profile: String,
    child: String,
    send: Sender<Projection>,
    waker: Waker,
    stop: Arc<AtomicBool>,
) {
    let mut view = ConversationView::default();
    let mut failures = 0u32;
    while !stop.load(Ordering::Acquire) {
        let attached = SessionTransport::connect(&connection, &profile)
            .map_err(|_| "Unable to connect to the subagent gateway".to_owned())
            .and_then(|mut transport| {
                resume(&mut view, &mut transport, &child)?;
                Ok(transport)
            });
        let mut transport = match attached {
            Ok(value) => {
                failures = 0;
                value
            }
            Err(message) => {
                failures += 1;
                view.error = Some(message);
                if !publish(&send, &waker, &view, false) || failures >= 4 {
                    break;
                }
                if pause(&stop, Duration::from_millis(250 * 2u64.pow(failures))) {
                    break;
                }
                continue;
            }
        };
        if !publish(&send, &waker, &view, true) {
            break;
        }
        loop {
            if stop.load(Ordering::Acquire) {
                return;
            }
            match transport.read_event() {
                Ok(Some(event)) => {
                    if deliver(&mut view, event) && !publish(&send, &waker, &view, true) {
                        return;
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    view.error = Some("Reconnecting to this subagent…".into());
                    if !publish(&send, &waker, &view, false) {
                        return;
                    }
                    break;
                }
            }
        }
        if pause(&stop, Duration::from_millis(500)) {
            break;
        }
    }
}
fn pause(stop: &AtomicBool, duration: Duration) -> bool {
    let until = Instant::now() + duration;
    while !stop.load(Ordering::Acquire) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(25));
    }
    stop.load(Ordering::Acquire)
}
