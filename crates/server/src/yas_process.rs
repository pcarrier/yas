//! Typed YAS adapter for the existing non-PTY process manager.
//!
//! The process manager remains the single owner of children, admission,
//! catalog generations and stream backpressure. This module translates its
//! private endpoint packets into semantic values; no YAS packet is exposed to
//! a YAS peer. Wire request IDs, operation replay and Transfer descriptors stay
//! in `yas`.

use std::collections::{HashMap, VecDeque};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

#[cfg(test)]
use tokio::sync::{Notify, Semaphore};
use tokio::sync::{mpsc, watch};
use yas_wire::core::RuntimeState;
use yas_wire::process as wire;
use yas_wire::schema;

use super::app_env::SessionEnv;
use super::process::{self, NativeRecord, Server};

const ROUTE_EVENTS: usize = 80;
pub(crate) const MAX_EXIT_REPLAYS_PER_SESSION: usize = schema::process::MAX_PROCESSES as usize;

#[derive(Clone)]
pub(crate) struct Runtime {
    server: Server,
    #[cfg(test)]
    operation_gate: Option<Arc<TestOperationGate>>,
}

#[cfg(test)]
pub(crate) struct TestOperationGate {
    entered: AtomicUsize,
    releases: Semaphore,
    changed: Notify,
}

#[cfg(test)]
impl Default for TestOperationGate {
    fn default() -> Self {
        Self {
            entered: AtomicUsize::new(0),
            releases: Semaphore::new(0),
            changed: Notify::new(),
        }
    }
}

#[cfg(test)]
impl TestOperationGate {
    async fn enter(&self) {
        self.entered.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_waiters();
        self.releases
            .acquire()
            .await
            .expect("Process test operation gate remains open")
            .forget();
    }

    pub(crate) async fn wait_for_entered(&self, expected: usize) {
        loop {
            let changed = self.changed.notified();
            if self.entered.load(Ordering::Acquire) >= expected {
                return;
            }
            changed.await;
        }
    }

    pub(crate) fn release(&self, count: usize) {
        self.releases.add_permits(count);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub(crate) revision: u64,
    pub(crate) records: Vec<wire::ProcessRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Unavailable,
    NotFound,
    Conflict,
    Permission,
    ResourceExhausted,
    Invalid(String),
    Io(String),
    Closed(String),
    Timeout,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("Process family is unavailable"),
            Self::NotFound => formatter.write_str("process not found"),
            Self::Conflict => formatter.write_str("process state conflicts with the request"),
            Self::Permission => formatter.write_str("process operation is not permitted"),
            Self::ResourceExhausted => formatter.write_str("process resource limit reached"),
            Self::Invalid(detail) => formatter.write_str(detail),
            Self::Io(detail) | Self::Closed(detail) => formatter.write_str(detail),
            Self::Timeout => formatter.write_str("process wait timed out"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Output {
        stream: Stream,
        lifetime_offset: u64,
        data: Vec<u8>,
    },
    StdinProgress {
        consumed: u64,
        open: bool,
    },
    Exit(ExitInfo),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExitInfo {
    pub(crate) kind: wire::ExitKind,
    pub(crate) reason: u8,
    pub(crate) code: i32,
    pub(crate) detail: Vec<u8>,
}

impl ExitInfo {
    pub(crate) fn into_record(self, exited_server_ns: u64) -> wire::ExitRecord {
        wire::ExitRecord {
            kind: self.kind,
            reason: self.reason,
            code: self.code,
            exited_server_ns: exited_server_ns.max(1),
            detail: self.detail,
        }
    }
}

pub(crate) struct Attachment {
    session: Session,
    route: Arc<Route>,
    events: mpsc::Receiver<Event>,
    pub(crate) process_handle: u64,
    pub(crate) stdin_lifetime_offset: u64,
    pub(crate) stdout_lifetime_offset: u64,
    pub(crate) stderr_lifetime_offset: u64,
    pub(crate) stdin_window: u64,
    pub(crate) merged_stderr: bool,
}

/// Cloneable command half of one Process attachment.
///
/// The native YAS boundary owns the event receiver in a dedicated pump while
/// Transfer input, output acknowledgement, and half-close handling continue
/// independently on the session task. Keeping those halves separate avoids
/// holding a lock across `AttachmentEvents::next` and therefore avoids
/// blocking stdin behind an uncredited stdout stream.
#[derive(Clone)]
pub(crate) struct AttachmentControl {
    session: Session,
    route: Arc<Route>,
    pub(crate) process_handle: u64,
    pub(crate) stdin_lifetime_offset: u64,
    pub(crate) stdout_lifetime_offset: u64,
    pub(crate) stderr_lifetime_offset: u64,
    pub(crate) stdin_window: u64,
    pub(crate) merged_stderr: bool,
}

pub(crate) struct AttachmentEvents {
    events: mpsc::Receiver<Event>,
}

#[derive(Clone)]
pub(crate) struct Session {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    server: Server,
    manager: process::Manager,
    session_env: StdMutex<Option<SessionEnv>>,
    next_process_id: AtomicU32,
    routes: StdMutex<HashMap<u32, Arc<Route>>>,
    /// Ordinary children leave the global catalogue as soon as EXIT is
    /// delivered. Retain their terminal value for the owning YAS session so a
    /// later WAIT remains authoritative.
    exits: StdMutex<ExitReplays>,
    closed: watch::Sender<Option<Error>>,
    shutting_down: AtomicBool,
    #[cfg(test)]
    operation_gate: Option<Arc<TestOperationGate>>,
}

#[derive(Default)]
struct ExitReplays {
    values: HashMap<u64, ExitInfo>,
    order: VecDeque<u64>,
}

impl ExitReplays {
    fn get(&self, process_handle: u64) -> Option<&ExitInfo> {
        self.values.get(&process_handle)
    }

    fn insert(&mut self, process_handle: u64, exit: ExitInfo) {
        if self.values.insert(process_handle, exit).is_none() {
            self.order.push_back(process_handle);
        }
        while self.order.len() > MAX_EXIT_REPLAYS_PER_SESSION {
            if let Some(retired) = self.order.pop_front() {
                self.values.remove(&retired);
            }
        }
    }

    fn clear(&mut self) {
        self.values.clear();
        self.order.clear();
    }
}

struct Route {
    process_id: u32,
    process_handle: AtomicU64,
    auto_ack_output: bool,
    events: mpsc::Sender<Event>,
    exit: watch::Sender<Option<ExitInfo>>,
}

enum WatchOutcome {
    Running(Attachment),
    Exited(ExitInfo),
}

impl Runtime {
    pub(crate) fn new(server: Server) -> Self {
        Self {
            server,
            #[cfg(test)]
            operation_gate: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_operation_gate(mut self, gate: Arc<TestOperationGate>) -> Self {
        self.operation_gate = Some(gate);
        self
    }

    pub(crate) fn enabled(&self) -> bool {
        self.server.enabled()
    }

    pub(crate) fn runtime_state(&self) -> RuntimeState {
        if self.enabled() {
            RuntimeState::Available
        } else {
            RuntimeState::Unavailable
        }
    }

    pub(crate) fn limits(&self) -> wire::Limits {
        wire::Limits {
            max_mutation_replays: super::yas::MAX_PROCESS_OPERATION_REPLAYS as u32,
            ..wire::Limits::HARD
        }
    }

    pub(crate) fn session(
        &self,
        owner_session: [u8; 16],
        session_env: Option<SessionEnv>,
    ) -> Result<Session, Error> {
        if !self.enabled() {
            return Err(Error::Unavailable);
        }
        if owner_session.iter().all(|byte| *byte == 0) {
            return Err(Error::Invalid("zero Process owner session".to_owned()));
        }
        let (manager, events, endpoint_closed) = self
            .server
            .native_endpoint_with_session(owner_session, ROUTE_EVENTS);
        let (closed, _) = watch::channel(None);
        let inner = Arc::new(SessionInner {
            server: self.server.clone(),
            manager,
            session_env: StdMutex::new(session_env),
            next_process_id: AtomicU32::new(1),
            routes: StdMutex::new(HashMap::new()),
            exits: StdMutex::new(ExitReplays::default()),
            closed,
            shutting_down: AtomicBool::new(false),
            #[cfg(test)]
            operation_gate: self.operation_gate.clone(),
        });
        tokio::spawn(route_outbound(
            Arc::downgrade(&inner),
            events,
            endpoint_closed,
        ));
        Ok(Session { inner })
    }

    pub(crate) fn snapshot(&self, now_server_ns: u64) -> Result<Snapshot, Error> {
        if !self.enabled() {
            return Err(Error::Unavailable);
        }
        let snapshot = self.server.native_snapshot();
        let records = snapshot
            .records
            .into_iter()
            .map(|record| process_record(record, now_server_ns))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Snapshot {
            revision: snapshot.revision,
            records,
        })
    }

    pub(crate) async fn changed(
        &self,
        revision: u64,
        now_server_ns: impl FnOnce() -> u64,
    ) -> Result<Snapshot, Error> {
        if !self.enabled() {
            return Err(Error::Unavailable);
        }
        self.server.wait_native_catalogue_change(revision).await;
        self.snapshot(now_server_ns())
    }
}

impl Session {
    /// Install the compositor-scoped environment lazily when the first
    /// `ENV_SESSION` spawn is actually requested. Negotiating or merely
    /// watching Process must not create a compositor as a side effect.
    pub(crate) fn set_session_env(&self, session_env: SessionEnv) {
        let mut current = self.inner.session_env.lock().unwrap();
        if current.is_none() {
            *current = Some(session_env);
        }
    }

    pub(crate) async fn spawn(
        &self,
        request: &wire::Spawn,
        resolved_cwd: Option<Vec<u8>>,
    ) -> Result<Attachment, Error> {
        let cwd = resolve_cwd(&request.cwd, resolved_cwd)?;
        let flags = u8::try_from(request.flags)
            .map_err(|_| Error::Invalid("Process SPAWN flags do not fit v1".to_owned()))?;
        let process_id = self.allocate_process_id()?;
        let (route, events) = self.install_route(process_id, false)?;
        let session_env = (request.environment_kind == wire::EnvironmentKind::Session)
            .then(|| self.inner.session_env.lock().unwrap().clone())
            .flatten();
        let clear_environment = request.environment_kind == wire::EnvironmentKind::Empty;
        let preserve_residual = request
            .surface_app_handle()
            .map_err(|error| Error::Invalid(error.to_string()))?
            .is_some();
        let started = self
            .inner
            .manager
            .spawn_native(
                process::NativeSpawnRequest {
                    process_id,
                    flags,
                    preserve_residual,
                    cwd,
                    argv: request.argv.clone(),
                    env: request
                        .env
                        .iter()
                        .map(|entry| (entry.key.clone(), entry.value.clone()))
                        .collect(),
                    clear_environment,
                },
                session_env,
            )
            .await
            .map_err(backend_error);
        let started = match started {
            Ok(started) => started,
            Err(error) => {
                self.remove_route(process_id);
                return Err(error);
            }
        };
        if started.process_id != process_id {
            self.remove_route(process_id);
            return Err(Error::Closed("mismatched Process SPAWN reply".to_owned()));
        }
        route
            .process_handle
            .store(started.process_handle, Ordering::Release);
        Ok(Attachment {
            session: self.clone(),
            route,
            events,
            process_handle: started.process_handle,
            stdin_lifetime_offset: 0,
            stdout_lifetime_offset: 0,
            stderr_lifetime_offset: 0,
            stdin_window: started.stdin_window,
            merged_stderr: started.stderr_window == 0,
        })
    }

    pub(crate) async fn attach(&self, request: &wire::Attach) -> Result<Attachment, Error> {
        #[cfg(test)]
        if let Some(gate) = &self.inner.operation_gate {
            gate.enter().await;
        }
        match self
            .watch_process(
                request.process_handle,
                request.flags & schema::process::ATTACH_STDIN as u16 != 0,
                false,
            )
            .await?
        {
            WatchOutcome::Running(attachment) => Ok(attachment),
            WatchOutcome::Exited(_) => Err(Error::Conflict),
        }
    }

    pub(crate) async fn control(
        &self,
        request: &wire::Control,
    ) -> Result<wire::ControlResult, Error> {
        #[cfg(test)]
        if let Some(gate) = &self.inner.operation_gate {
            gate.enter().await;
        }
        let (process_id, temporary) = match self.route_for_handle(request.process_handle) {
            Some(process_id) => (process_id, None),
            None => match self
                .watch_process(request.process_handle, false, true)
                .await?
            {
                WatchOutcome::Running(attachment) => {
                    let process_id = attachment.route.process_id;
                    (process_id, Some(attachment))
                }
                WatchOutcome::Exited(_) => return Err(Error::Conflict),
            },
        };
        let action = native_control(request.action, request.value)?;
        self.send_control(process_id, action).await?;
        if request.action == wire::ControlAction::Detach {
            self.remove_route(process_id);
        } else if let Some(attachment) = temporary {
            let _ = self
                .send_control(process_id, process::NativeControl::Detach)
                .await;
            self.remove_route(attachment.route.process_id);
        }
        Ok(wire::ControlResult {
            state_revision: self.inner.server.native_snapshot().revision,
        })
    }

    pub(crate) async fn wait(&self, request: &wire::Wait) -> Result<ExitInfo, Error> {
        if let Some(exit) = self
            .inner
            .exits
            .lock()
            .unwrap()
            .get(request.process_handle)
            .cloned()
        {
            return Ok(exit);
        }
        let (mut exit, temporary) =
            if let Some(route) = self.route_by_handle(request.process_handle) {
                (route.exit.subscribe(), None)
            } else {
                match self
                    .watch_process(request.process_handle, false, true)
                    .await?
                {
                    WatchOutcome::Exited(exit) => return Ok(exit),
                    WatchOutcome::Running(attachment) => {
                        let exit = attachment.route.exit.subscribe();
                        (exit, Some(attachment))
                    }
                }
            };
        let wait = async {
            loop {
                if let Some(exit) = exit.borrow().clone() {
                    return Ok(exit);
                }
                exit.changed()
                    .await
                    .map_err(|_| Error::Closed("Process attachment closed".to_owned()))?;
            }
        };
        let result = if request.timeout_ns == 0 {
            wait.await
        } else {
            match tokio::time::timeout(Duration::from_nanos(request.timeout_ns), wait).await {
                Ok(result) => result,
                Err(_) => Err(Error::Timeout),
            }
        };
        if let Some(attachment) = temporary {
            let _ = self
                .send_control(attachment.route.process_id, process::NativeControl::Detach)
                .await;
            self.remove_route(attachment.route.process_id);
        }
        result
    }

    pub(crate) async fn shutdown(&self) {
        if self.inner.shutting_down.swap(true, Ordering::AcqRel) {
            return;
        }
        self.inner.manager.shutdown().await;
        close_session(
            &self.inner,
            Error::Closed("Process session closed".to_owned()),
        );
    }

    async fn watch_process(
        &self,
        process_handle: u64,
        stdin: bool,
        auto_ack_output: bool,
    ) -> Result<WatchOutcome, Error> {
        let process_id = self.allocate_process_id()?;
        let (route, events) = self.install_route(process_id, auto_ack_output)?;
        route
            .process_handle
            .store(process_handle, Ordering::Release);
        let watched = self
            .inner
            .manager
            .watch_native(process_id, process_handle, stdin)
            .map_err(backend_error);
        let watched = match watched {
            Ok(watched) => watched,
            Err(error) => {
                self.remove_route(process_id);
                return Err(error);
            }
        };
        if watched.process_id != process_id || watched.process_handle != process_handle {
            self.remove_route(process_id);
            return Err(Error::Closed("mismatched Process WATCH reply".to_owned()));
        }
        if !watched.running {
            self.remove_route(process_id);
            return Ok(WatchOutcome::Exited(
                watched
                    .exit
                    .map(native_exit_info)
                    .ok_or_else(|| Error::Closed("missing Process exit".to_owned()))?,
            ));
        }
        Ok(WatchOutcome::Running(Attachment {
            session: self.clone(),
            route,
            events,
            process_handle,
            stdin_lifetime_offset: watched.stdin_received,
            stdout_lifetime_offset: watched.stdout_next,
            stderr_lifetime_offset: watched.stderr_next,
            stdin_window: watched.stdin_window,
            merged_stderr: watched.stream_state & process::NATIVE_STREAM_MERGED_STDERR != 0,
        }))
    }

    async fn send_control(
        &self,
        process_id: u32,
        action: process::NativeControl,
    ) -> Result<(), Error> {
        self.inner
            .manager
            .control_native(process_id, action)
            .map_err(backend_error)
    }

    fn acknowledge_output(
        &self,
        route: &Route,
        stream: Stream,
        consumed_lifetime_offset: u64,
    ) -> Result<(), Error> {
        let stream = match stream {
            Stream::Stdout => process::NATIVE_STREAM_STDOUT,
            Stream::Stderr => process::NATIVE_STREAM_STDERR,
        };
        match self.inner.manager.acknowledge_output_native(
            route.process_id,
            stream,
            consumed_lifetime_offset,
        ) {
            Ok(()) => Ok(()),
            // Terminal dispatch retires the native binding. Output already
            // delivered ahead of EXIT can still be consumed afterwards, and
            // its final acknowledgement is then an idempotent no-op.
            Err(process::NativeError::NotFound) if route.exit.borrow().is_some() => Ok(()),
            Err(error) => Err(backend_error(error)),
        }
    }

    fn allocate_process_id(&self) -> Result<u32, Error> {
        for _ in 0..u32::MAX {
            let id = self
                .inner
                .next_process_id
                .fetch_add(1, Ordering::Relaxed)
                .max(1);
            if !self.inner.routes.lock().unwrap().contains_key(&id) {
                return Ok(id);
            }
        }
        Err(Error::ResourceExhausted)
    }

    fn install_route(
        &self,
        process_id: u32,
        auto_ack_output: bool,
    ) -> Result<(Arc<Route>, mpsc::Receiver<Event>), Error> {
        let (events, receiver) = mpsc::channel(ROUTE_EVENTS);
        let (exit, _) = watch::channel(None);
        let route = Arc::new(Route {
            process_id,
            process_handle: AtomicU64::new(0),
            auto_ack_output,
            events,
            exit,
        });
        if self
            .inner
            .routes
            .lock()
            .unwrap()
            .insert(process_id, route.clone())
            .is_some()
        {
            return Err(Error::Conflict);
        }
        Ok((route, receiver))
    }

    fn remove_route(&self, process_id: u32) {
        self.inner.routes.lock().unwrap().remove(&process_id);
    }

    fn route_for_handle(&self, process_handle: u64) -> Option<u32> {
        self.route_by_handle(process_handle)
            .map(|route| route.process_id)
    }

    fn route_by_handle(&self, process_handle: u64) -> Option<Arc<Route>> {
        self.inner
            .routes
            .lock()
            .unwrap()
            .values()
            .find(|route| route.process_handle.load(Ordering::Acquire) == process_handle)
            .cloned()
    }
}

impl Attachment {
    pub(crate) fn split(self) -> (AttachmentControl, AttachmentEvents) {
        let control = AttachmentControl {
            session: self.session,
            route: self.route,
            process_handle: self.process_handle,
            stdin_lifetime_offset: self.stdin_lifetime_offset,
            stdout_lifetime_offset: self.stdout_lifetime_offset,
            stderr_lifetime_offset: self.stderr_lifetime_offset,
            stdin_window: self.stdin_window,
            merged_stderr: self.merged_stderr,
        };
        (
            control,
            AttachmentEvents {
                events: self.events,
            },
        )
    }

    #[cfg(test)]
    pub(crate) async fn next(&mut self) -> Option<Event> {
        self.events.recv().await
    }

    #[cfg(test)]
    pub(crate) fn acknowledge_output(
        &self,
        stream: Stream,
        consumed_lifetime_offset: u64,
    ) -> Result<(), Error> {
        self.session
            .acknowledge_output(&self.route, stream, consumed_lifetime_offset)
    }
}

impl AttachmentEvents {
    pub(crate) async fn next(&mut self) -> Option<Event> {
        self.events.recv().await
    }
}

impl AttachmentControl {
    pub(crate) fn local_id(&self) -> u32 {
        self.route.process_id
    }

    pub(crate) fn write_stdin(&self, lifetime_offset: u64, data: &[u8]) -> Result<(), Error> {
        self.session
            .inner
            .manager
            .write_stdin_native(self.route.process_id, lifetime_offset, data)
            .map_err(backend_error)
    }

    pub(crate) fn acknowledge_output(
        &self,
        stream: Stream,
        consumed_lifetime_offset: u64,
    ) -> Result<(), Error> {
        self.session
            .acknowledge_output(&self.route, stream, consumed_lifetime_offset)
    }

    pub(crate) async fn close_stdin(&self) -> Result<(), Error> {
        self.session
            .send_control(self.route.process_id, process::NativeControl::CloseStdin)
            .await
    }

    pub(crate) async fn detach(self) -> Result<(), Error> {
        let result = self
            .session
            .send_control(self.route.process_id, process::NativeControl::Detach)
            .await;
        self.session.remove_route(self.route.process_id);
        result
    }
}

async fn route_outbound(
    inner: std::sync::Weak<SessionInner>,
    mut events: mpsc::Receiver<process::NativeEventEnvelope>,
    mut endpoint_closed: watch::Receiver<Option<String>>,
) {
    loop {
        let event = tokio::select! {
            event = events.recv() => event,
            changed = endpoint_closed.changed() => {
                let detail = if changed.is_ok() {
                    endpoint_closed.borrow().clone()
                } else {
                    None
                };
                if let Some(inner) = inner.upgrade() {
                    close_session(
                        &inner,
                        Error::Closed(detail.unwrap_or_else(|| {
                            "Process endpoint writer closed".to_owned()
                        })),
                    );
                }
                return;
            }
        };
        let Some(event) = event else {
            break;
        };
        let Some(inner) = inner.upgrade() else {
            return;
        };
        if let Err(error) = event.dispatch(|event| dispatch_outbound(&inner, event)) {
            close_session(&inner, error);
            return;
        }
    }
    if let Some(inner) = inner.upgrade() {
        close_session(
            &inner,
            Error::Closed("Process endpoint writer closed".to_owned()),
        );
    }
}

fn dispatch_outbound(inner: &Arc<SessionInner>, event: process::NativeEvent) -> Result<(), Error> {
    match event {
        process::NativeEvent::Output {
            process_id,
            stream,
            offset,
            data,
        } => {
            let route = inner
                .routes
                .lock()
                .unwrap()
                .get(&process_id)
                .cloned()
                .ok_or_else(|| Error::Closed("output for unknown Process binding".to_owned()))?;
            let semantic_stream = match stream {
                process::NATIVE_STREAM_STDOUT => Stream::Stdout,
                process::NATIVE_STREAM_STDERR => Stream::Stderr,
                _ => return Err(Error::Closed("invalid Process output stream".to_owned())),
            };
            let end = offset
                .checked_add(data.len() as u64)
                .ok_or_else(|| Error::Closed("Process output offset overflow".to_owned()))?;
            if route.auto_ack_output {
                inner
                    .manager
                    .acknowledge_output_native(process_id, stream, end)
                    .map_err(backend_error)?;
                return Ok(());
            }
            route
                .events
                .try_send(Event::Output {
                    stream: semantic_stream,
                    lifetime_offset: offset,
                    data,
                })
                .map_err(|_| Error::Closed("Process semantic stream queue overflowed".to_owned()))
        }
        process::NativeEvent::StdinProgress {
            process_id,
            consumed,
            open,
        } => {
            let route = inner
                .routes
                .lock()
                .unwrap()
                .get(&process_id)
                .cloned()
                .ok_or_else(|| Error::Closed("stdin ACK for unknown Process binding".to_owned()))?;
            route
                .events
                .try_send(Event::StdinProgress { consumed, open })
                .map_err(|_| Error::Closed("Process semantic stream queue overflowed".to_owned()))
        }
        process::NativeEvent::Exit { process_id, exit } => {
            let route = inner
                .routes
                .lock()
                .unwrap()
                .remove(&process_id)
                .ok_or_else(|| Error::Closed("exit for unknown Process binding".to_owned()))?;
            let exit = native_exit_info(exit);
            let process_handle = route.process_handle.load(Ordering::Acquire);
            if process_handle != 0 {
                inner
                    .exits
                    .lock()
                    .unwrap()
                    .insert(process_handle, exit.clone());
            }
            route.exit.send_replace(Some(exit.clone()));
            let _ = route.events.try_send(Event::Exit(exit));
            Ok(())
        }
    }
}

fn close_session(inner: &SessionInner, error: Error) {
    if inner.closed.borrow().is_some() {
        return;
    }
    let _ = inner.closed.send(Some(error));
    inner.routes.lock().unwrap().clear();
    inner.exits.lock().unwrap().clear();
}

fn resolve_cwd(cwd: &wire::Cwd, resolved: Option<Vec<u8>>) -> Result<Option<Vec<u8>>, Error> {
    match cwd {
        wire::Cwd::ServerDefault => Ok(None),
        wire::Cwd::Path(path) => Ok(Some(path.clone())),
        wire::Cwd::Terminal(_) | wire::Cwd::Fs { .. } => resolved
            .map(Some)
            .ok_or_else(|| Error::Invalid("Process cwd handle was not resolved".to_owned())),
    }
}

fn native_control(
    action: wire::ControlAction,
    value: u16,
) -> Result<process::NativeControl, Error> {
    match action {
        wire::ControlAction::Signal => Ok(process::NativeControl::Signal(native_signal(value)?)),
        wire::ControlAction::Terminate => Ok(process::NativeControl::Terminate),
        wire::ControlAction::Kill => Ok(process::NativeControl::Kill),
        wire::ControlAction::Detach => Ok(process::NativeControl::Detach),
    }
}

#[cfg(unix)]
fn native_signal(value: u16) -> Result<u32, Error> {
    let signal = match value {
        value if value == schema::process::SIGNAL_INTERRUPT as u16 => libc::SIGINT,
        value if value == schema::process::SIGNAL_TERMINATE as u16 => libc::SIGTERM,
        value if value == schema::process::SIGNAL_KILL as u16 => libc::SIGKILL,
        value if value == schema::process::SIGNAL_HANGUP as u16 => libc::SIGHUP,
        _ => return Err(Error::Invalid("unknown portable Process signal".to_owned())),
    };
    Ok(signal as u32)
}

#[cfg(windows)]
fn native_signal(value: u16) -> Result<u32, Error> {
    match value {
        value if value == schema::process::SIGNAL_INTERRUPT as u16 => Ok(2),
        value if value == schema::process::SIGNAL_TERMINATE as u16 => Ok(15),
        value if value == schema::process::SIGNAL_KILL as u16 => Ok(9),
        value if value == schema::process::SIGNAL_HANGUP as u16 => Ok(1),
        _ => Err(Error::Invalid("unknown portable Process signal".to_owned())),
    }
}

fn process_record(record: NativeRecord, now_server_ns: u64) -> Result<wire::ProcessRecord, Error> {
    let flags = u16::from(record.flags & process::NATIVE_CATALOG_FLAGS);
    let stream_state = if record.running {
        let mut state = 0u8;
        if record.stream_state
            & (process::NATIVE_STREAM_STDIN_ACCEPTING | process::NATIVE_STREAM_STDIN_CLOSING)
            != 0
        {
            state |= schema::process::STREAM_STDIN_OPEN as u8;
        }
        if record.stream_state & process::NATIVE_STREAM_STDOUT_OPEN != 0 {
            state |= schema::process::STREAM_STDOUT_OPEN as u8;
        }
        if record.stream_state & process::NATIVE_STREAM_STDERR_OPEN != 0 {
            state |= schema::process::STREAM_STDERR_OPEN as u8;
        }
        state
    } else {
        0
    };
    let exit = record
        .exit
        .map(native_exit_info)
        .map(|exit| exit.into_record(now_server_ns));
    Ok(wire::ProcessRecord {
        process_handle: record.process_handle,
        lifecycle: if record.running {
            schema::process::LIFECYCLE_RUNNING as u8
        } else {
            schema::process::LIFECYCLE_EXITED as u8
        },
        stream_state,
        flags,
        native_pid: u64::from(record.native_pid),
        owner_session: record.owner_session,
        argv0: record.argv0,
        stdin_received: record.stdin_received,
        stdout_produced: record.stdout_produced,
        stderr_produced: record.stderr_produced,
        retention_deadline_server_ns: if !record.running
            && flags & schema::process::SPAWN_DETACHABLE as u16 != 0
        {
            now_server_ns.saturating_add(schema::process::MAX_DETACHED_RETENTION_NS)
        } else {
            0
        },
        exit,
        extensions: Default::default(),
    })
}

fn native_exit_info(exit: process::NativeExit) -> ExitInfo {
    ExitInfo {
        kind: exit.kind,
        reason: exit.reason,
        code: exit.code,
        detail: exit.detail,
    }
}

fn backend_error(error: process::NativeError) -> Error {
    match error {
        process::NativeError::NotFound => Error::NotFound,
        process::NativeError::Conflict => Error::Conflict,
        process::NativeError::Permission => Error::Permission,
        process::NativeError::ResourceExhausted => Error::ResourceExhausted,
        process::NativeError::Invalid(detail) => Error::Invalid(detail),
        process::NativeError::Io(detail) => Error::Io(detail),
        process::NativeError::Closed(detail) => Error::Closed(detail),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use yas_wire::{Extension, Extensions, process::EnvEntry};

    #[test]
    fn exit_replays_are_bounded_retryable_and_fifo_evicted() {
        let mut exits = ExitReplays::default();
        for process_handle in 1..=MAX_EXIT_REPLAYS_PER_SESSION as u64 + 1 {
            exits.insert(
                process_handle,
                ExitInfo {
                    kind: wire::ExitKind::Code,
                    reason: 0,
                    code: process_handle as i32,
                    detail: Vec::new(),
                },
            );
        }
        assert_eq!(exits.values.len(), MAX_EXIT_REPLAYS_PER_SESSION);
        assert_eq!(exits.order.len(), MAX_EXIT_REPLAYS_PER_SESSION);
        assert!(exits.get(1).is_none(), "oldest terminal replay is evicted");
        let newest = MAX_EXIT_REPLAYS_PER_SESSION as u64 + 1;
        assert_eq!(exits.get(newest).unwrap().code, newest as i32);
        // WAIT has no operation ID, so successful delivery remains retryable
        // until ordinary FIFO churn evicts the replay.
        assert_eq!(exits.get(newest).unwrap().code, newest as i32);
    }

    fn spawn_request(argv: Vec<Vec<u8>>, env: Vec<EnvEntry>) -> wire::Spawn {
        wire::Spawn {
            operation_id: [7; 16],
            flags: 0,
            environment_kind: wire::EnvironmentKind::Empty,
            cwd: wire::Cwd::ServerDefault,
            argv,
            env,
            stdout_receive_credit: 1024 * 1024,
            stderr_receive_credit: 1024 * 1024,
            extensions: Extensions::default(),
        }
    }

    fn executable(name: &str) -> Vec<u8> {
        use std::os::unix::ffi::OsStrExt;
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|directory| directory.join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| panic!("{name} is not on PATH"))
            .as_os_str()
            .as_bytes()
            .to_vec()
    }

    #[tokio::test]
    async fn surface_app_launcher_keeps_its_process_group_alive() {
        let server = Server::new(false, true);
        let runtime = Runtime::new(server.clone());
        let session = runtime.session([8; 16], None).unwrap();
        let sleep = String::from_utf8(executable("sleep")).unwrap();
        let mut request = spawn_request(
            vec![
                executable("sh"),
                b"-c".to_vec(),
                format!("({sleep} 0.15; printf survived) &").into_bytes(),
            ],
            Vec::new(),
        );
        request.flags =
            (schema::process::SPAWN_DETACHABLE | schema::process::SPAWN_MERGE_STDERR) as u16;
        request.stderr_receive_credit = 0;
        request.extensions = Extensions(vec![Extension {
            tag: schema::process::SPAWN_SURFACE_APP_EXTENSION as u16,
            required: true,
            value: 1u64.to_le_bytes().to_vec(),
        }]);

        let mut attachment = session.spawn(&request, None).await.unwrap();
        let mut output = Vec::new();
        let exit = loop {
            match tokio::time::timeout(Duration::from_secs(2), attachment.next())
                .await
                .unwrap()
                .unwrap()
            {
                Event::Output { data, .. } => output.extend_from_slice(&data),
                Event::Exit(exit) => break exit,
                Event::StdinProgress { .. } => {}
            }
        };
        assert_eq!(output, b"survived");
        assert_eq!(exit.kind, wire::ExitKind::Code);
        assert_eq!(exit.code, 0);
        session.shutdown().await;
        server.shutdown().await;
    }

    #[tokio::test]
    async fn typed_spawn_stream_credit_exit_and_catalogue() {
        let server = Server::new(false, true);
        let runtime = Runtime::new(server.clone());
        let before = runtime.snapshot(1).unwrap();
        assert_eq!(before.revision, 1);
        assert!(before.records.is_empty());

        let session = runtime.session([9; 16], None).unwrap();
        let attachment = session
            .spawn(&spawn_request(vec![executable("cat")], Vec::new()), None)
            .await
            .unwrap();
        assert_ne!(attachment.process_handle, 0);
        assert_ne!(attachment.stdin_window, 0);
        let (control, mut events) = attachment.split();

        let changed = runtime.changed(before.revision, || 2).await.unwrap();
        assert_eq!(changed.records.len(), 1);
        assert_eq!(changed.records[0].owner_session, [9; 16]);
        assert!(changed.records[0].argv0.ends_with(b"/cat"));

        control.write_stdin(0, b"hello\n").unwrap();
        let mut saw_ack = false;
        let mut saw_output = false;
        while !saw_ack || !saw_output {
            match tokio::time::timeout(Duration::from_secs(2), events.next())
                .await
                .unwrap()
                .unwrap()
            {
                Event::StdinProgress { consumed, .. } => {
                    assert_eq!(consumed, 6);
                    saw_ack = true;
                }
                Event::Output {
                    stream,
                    lifetime_offset,
                    data,
                } => {
                    assert_eq!(stream, Stream::Stdout);
                    assert_eq!(lifetime_offset, 0);
                    assert_eq!(data, b"hello\n");
                    control
                        .acknowledge_output(stream, data.len() as u64)
                        .unwrap();
                    saw_output = true;
                }
                Event::Exit(exit) => panic!("cat exited early: {exit:?}"),
            }
        }
        control.close_stdin().await.unwrap();
        let exit = loop {
            if let Event::Exit(exit) = tokio::time::timeout(Duration::from_secs(2), events.next())
                .await
                .unwrap()
                .unwrap()
            {
                break exit;
            }
        };
        assert_eq!(exit.kind, wire::ExitKind::Code);
        assert_eq!(exit.code, 0);
        session.shutdown().await;
        server.shutdown().await;
    }

    #[tokio::test]
    async fn empty_environment_is_exact_and_wait_observes_exit() {
        let server = Server::new(false, true);
        let runtime = Runtime::new(server.clone());
        let session = runtime.session([3; 16], None).unwrap();
        let mut attachment = session
            .spawn(
                &spawn_request(
                    vec![b"/usr/bin/env".to_vec()],
                    vec![EnvEntry {
                        key: b"YAS_PROCESS_TEST".to_vec(),
                        value: b"exact".to_vec(),
                    }],
                ),
                None,
            )
            .await
            .unwrap();
        let handle = attachment.process_handle;
        let mut output = Vec::new();
        let mut acknowledgements = Vec::new();
        let exit = loop {
            match tokio::time::timeout(Duration::from_secs(2), attachment.next())
                .await
                .unwrap()
                .unwrap()
            {
                Event::Output {
                    stream,
                    lifetime_offset,
                    data,
                } => {
                    output.extend_from_slice(&data);
                    acknowledgements
                        .push((stream, lifetime_offset.saturating_add(data.len() as u64)));
                }
                Event::Exit(exit) => break exit,
                Event::StdinProgress { .. } => {}
            }
        };
        assert_eq!(output, b"YAS_PROCESS_TEST=exact\n");
        assert_eq!(exit.code, 0);

        // Output delivery and terminal publication run independently. Final
        // output acknowledgements remain valid even after EXIT retires the
        // native binding.
        for (stream, consumed) in acknowledgements {
            attachment.acknowledge_output(stream, consumed).unwrap();
        }

        // The route's exit watch remains usable after terminal delivery.
        let waited = session
            .wait(&wire::Wait {
                process_handle: handle,
                timeout_ns: Duration::from_secs(1).as_nanos() as u64,
                extensions: Extensions::default(),
            })
            .await
            .unwrap();
        assert_eq!(waited, exit);
        session.shutdown().await;
        server.shutdown().await;
    }
}
