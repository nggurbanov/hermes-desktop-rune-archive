//! Event-thread requests for ordinary windows created after the loop has started.
use crate::{AppHandle, WindowHandle, WindowSpec};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

const MAX_PENDING: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowSpawnStatus {
    Queued,
    /// The independent host is registered; a windowless app has not opened yet.
    Registered,
    /// Native creation completed. `WindowHandle::is_ready` additionally checks its first frame.
    Created,
    Failed(String),
}

#[derive(Clone)]
pub struct WindowCreation(Arc<Mutex<Receipt>>);
struct Receipt {
    status: WindowSpawnStatus,
    wake: Option<crate::Waker>,
}
impl std::fmt::Debug for WindowCreation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.status().fmt(f)
    }
}
impl WindowCreation {
    pub fn status(&self) -> WindowSpawnStatus {
        self.0.lock().unwrap().status.clone()
    }
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Receipt {
            status: WindowSpawnStatus::Queued,
            wake: None,
        })))
    }
    fn set(&self, status: WindowSpawnStatus) {
        let wake = {
            let mut s = self.0.lock().unwrap();
            s.status = status;
            s.wake.clone()
        };
        if let Some(wake) = wake {
            wake();
        }
    }
}

pub struct SpawnedWindow<S: 'static> {
    pub app: AppHandle<S>,
    pub window: WindowHandle,
    pub creation: WindowCreation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSpawnError(pub String);
impl std::fmt::Display for WindowSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for WindowSpawnError {}

impl crate::Tx {
    /// Create an independent ordinary window on this event loop. State and factories may
    /// capture `Rc` and other non-Send data. A background caller enters the event thread
    /// through `AppHandle::update` and constructs its spec there.
    ///
    /// Closing the caller does not close this window; Quit remains application-wide. The
    /// queue limit is temporary backpressure, never a fixed lifetime window pool. Dynamic
    /// windows use their own App arguments, or no tooling arguments by default. A single
    /// offscreen runtime reports Failed explicitly rather than pretending a host exists.
    pub fn spawn_window<S: 'static>(
        &mut self,
        mut spec: WindowSpec<S>,
    ) -> Result<SpawnedWindow<S>, WindowSpawnError> {
        if spec.key.is_empty() {
            return Err(WindowSpawnError(
                "A dynamic window requires a nonempty key".into(),
            ));
        }
        if self.window_spawns.len() >= MAX_PENDING {
            return Err(WindowSpawnError(
                "The window creation queue is full; try again after this event".into(),
            ));
        }
        let app = spec.app.handle();
        let window = WindowHandle::new(&spec.key);
        let creation = WindowCreation::new();
        let ready = creation.clone();
        let original = spec.on_created.take();
        spec.on_created = Some(Box::new(move |handle| {
            ready.set(WindowSpawnStatus::Created);
            if let Some(original) = original {
                original(handle);
            }
        }));
        let error_app = app.clone();
        let publish_app = app.clone();
        let request = Request {
            spec: Some(crate::AnyWindowSpec::dynamic(spec, window.clone())),
            creation: creation.clone(),
            error: Box::new(move || error_app.window_error()),
            publish_error: Box::new(move |error| {
                *publish_app.shared.window_error.lock().unwrap() = Some(error)
            }),
        };
        self.window_spawns
            .push(Call(Rc::new(RefCell::new(Some(request)))));
        Ok(SpawnedWindow {
            app,
            window,
            creation,
        })
    }
}

#[derive(Clone)]
pub(crate) struct Call(Rc<RefCell<Option<Request>>>);
impl std::fmt::Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WindowSpawn")
    }
}
impl Call {
    fn take(&self) -> Option<Request> {
        self.0.borrow_mut().take()
    }
}

pub(crate) struct Request {
    spec: Option<crate::AnyWindowSpec>,
    creation: WindowCreation,
    error: Box<dyn Fn() -> Option<String>>,
    publish_error: Box<dyn Fn(String)>,
}
impl Request {
    pub(crate) fn take_spec(&mut self) -> crate::AnyWindowSpec {
        self.spec.take().unwrap()
    }
    pub(crate) fn fail(&self, error: impl Into<String>) {
        let error = error.into();
        (self.publish_error)(error.clone());
        self.creation.set(WindowSpawnStatus::Failed(error));
    }
    pub(crate) fn registered(&self) {
        self.creation.set(WindowSpawnStatus::Registered);
    }
    pub(crate) fn check_error(&self) -> bool {
        if let Some(error) = (self.error)() {
            self.fail(error);
            true
        } else {
            false
        }
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        if self.spec.is_some() && self.creation.status() == WindowSpawnStatus::Queued {
            self.fail("The creating runtime closed before the window was registered");
        }
    }
}

#[derive(Default)]
pub(crate) struct Queue {
    pending: Vec<Request>,
    wake: Option<crate::Waker>,
}
impl Queue {
    pub(crate) fn enable(&mut self, wake: crate::Waker) {
        self.wake = Some(wake);
    }
    pub(crate) fn latch(&mut self, calls: &mut Vec<Call>) {
        for request in calls.drain(..).filter_map(|call| call.take()) {
            request.creation.0.lock().unwrap().wake = self.wake.clone();
            if self.wake.is_none() {
                request.fail("Dynamic native windows are unavailable in this offscreen runtime");
            } else if self.pending.len() >= MAX_PENDING {
                request.fail("The window creation queue is full; try again after this event");
            } else {
                self.pending.push(request);
            }
        }
    }
    pub(crate) fn take(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.pending)
    }
}
