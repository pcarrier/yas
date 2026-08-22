//! Native non-PTY child processes.
//!
//! The server owns admission and a public catalog. Each logical endpoint owns
//! its pending IDs, subscriptions, and ordinary children. Output offsets and
//! accepted stdin belong to the child generation and are shared by watchers.

use rustc_hash::FxHashMap;
use std::collections::VecDeque;
#[cfg(unix)]
use std::ffi::{OsStr, OsString};
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};
#[cfg(any(unix, windows))]
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, Semaphore, mpsc, oneshot, watch};
use tokio::task::AbortHandle;
use yas_wire::process as wire;
use yas_wire::schema::process as process_schema;

#[cfg(unix)]
use crate::pty;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
#[cfg(windows)]
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
#[cfg(windows)]
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

const DEFAULT_MAX_PER_CLIENT: usize = 16;
const DEFAULT_MAX_GLOBAL: usize = 64;
const DEFAULT_MAX_SPAWNING: usize = 8;
const DEFAULT_MAX_WATCHERS_PER_GENERATION: usize = 64;
const DEFAULT_REQUEST_MAX_PER_CLIENT: usize = 16 * 1024 * 1024;
const DEFAULT_REQUEST_MAX: usize = 64 * 1024 * 1024;
const DEFAULT_BUFFER_MAX: usize = 192 * 1024 * 1024;
/// Keep one process frame from occupying an entire ordinary bulk-writer turn.
/// The protocol accepts larger packets, but the server emits at most this much
/// stdout or stderr data before the fair scheduler can choose another queue.
const OUTPUT_FRAME_PAYLOAD: usize = 32 * 1024;
const DEFAULT_KILL_GRACE: Duration = Duration::from_secs(2);
const DEFAULT_FINAL_TTL: Duration = Duration::from_secs(5 * 60);

const PENDING_QUEUED: u8 = 0;
const PENDING_ACTIVE: u8 = 1;
const PENDING_DONE: u8 = 2;

type ProcessId = u32;
type ProcessRef = u64;

// Private engine state is semantic, not an encoding of the retired packet
// protocol.  The adapter maps this state to the public `yas.process` types.
const PROCESS_SPAWN_MERGE_STDERR: u8 = process_schema::SPAWN_MERGE_STDERR as u8;
const PROCESS_SPAWN_DETACHABLE: u8 = process_schema::SPAWN_DETACHABLE as u8;
const PROCESS_STREAM_STDOUT: u8 = process_schema::STREAM_STDOUT_CONTENT_KIND as u8;
const PROCESS_STREAM_STDERR: u8 = process_schema::STREAM_STDERR_CONTENT_KIND as u8;
const PROCESS_STREAM_STDIN_ACCEPTING: u8 = 1 << 0;
const PROCESS_STREAM_STDIN_CLOSING: u8 = 1 << 1;
const PROCESS_STREAM_STDIN_CLOSED: u8 = 1 << 2;
const PROCESS_STREAM_STDOUT_OPEN: u8 = 1 << 3;
const PROCESS_STREAM_STDERR_OPEN: u8 = 1 << 4;
const PROCESS_STREAM_MERGED_STDERR: u8 = 1 << 5;
const PROCESS_STREAM_STDIN_WRITABLE: u8 = 1 << 6;
const PROCESS_STDIN_ACCEPTING: u8 = 1;
const PROCESS_STDIN_CLOSING: u8 = 2;
const PROCESS_STDIN_CLOSED: u8 = 3;
const PROCESS_EXIT_RETURNED: u8 = wire::ExitKind::Code as u8;
const PROCESS_EXIT_SIGNALLED: u8 = wire::ExitKind::Signal as u8;
const PROCESS_EXIT_KILLED: u8 = wire::ExitKind::Killed as u8;
const PROCESS_EXIT_PROTOCOL_VIOLATION: u8 = wire::ExitKind::Other as u8;
const PROCESS_EXIT_HOST_FAILURE: u8 = wire::ExitKind::Other as u8;
const PROCESS_KILL_CLIENT: u8 = process_schema::EXIT_REASON_CLIENT as u8;
const PROCESS_KILL_OWNER_LOST: u8 = process_schema::EXIT_REASON_OWNER_LOST as u8;
const PROCESS_KILL_TERMINATE_TIMEOUT: u8 = process_schema::EXIT_REASON_TERMINATE_TIMEOUT as u8;
const PROCESS_KILL_SERVER_SHUTDOWN: u8 = process_schema::EXIT_REASON_SERVER_SHUTDOWN as u8;
const PROCESS_MAX_UNACKED_PACKETS: usize = 1_024;
const PROCESS_DEFAULT_STREAM_WINDOW: u64 = 1024 * 1024;

pub(crate) const NATIVE_STREAM_STDOUT: u8 = PROCESS_STREAM_STDOUT;
pub(crate) const NATIVE_STREAM_STDERR: u8 = PROCESS_STREAM_STDERR;
pub(crate) const NATIVE_STREAM_MERGED_STDERR: u8 = PROCESS_STREAM_MERGED_STDERR;
pub(crate) const NATIVE_STREAM_STDIN_ACCEPTING: u8 = PROCESS_STDIN_ACCEPTING;
pub(crate) const NATIVE_STREAM_STDIN_CLOSING: u8 = PROCESS_STDIN_CLOSING;
pub(crate) const NATIVE_STREAM_STDOUT_OPEN: u8 = PROCESS_STREAM_STDOUT_OPEN;
pub(crate) const NATIVE_STREAM_STDERR_OPEN: u8 = PROCESS_STREAM_STDERR_OPEN;
pub(crate) const NATIVE_CATALOG_FLAGS: u8 = PROCESS_SPAWN_MERGE_STDERR | PROCESS_SPAWN_DETACHABLE;
#[cfg(windows)]
struct JobHandle(HANDLE);

#[cfg(windows)]
unsafe impl Send for JobHandle {}
#[cfg(windows)]
unsafe impl Sync for JobHandle {}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_duration(name: &str, default: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_duration(&value))
        .unwrap_or(default)
}

fn parse_duration(value: &str) -> Option<Duration> {
    value
        .parse::<f64>()
        .ok()
        .and_then(|value| Duration::try_from_secs_f64(value).ok())
}

#[derive(Clone)]
struct Policy {
    enabled: bool,
    max_per_endpoint: usize,
    max_generations: usize,
    max_watchers: usize,
    max_watchers_per_generation: usize,
    max_request_per_endpoint: usize,
    max_request: usize,
    max_buffer: usize,
    kill_grace: Duration,
    final_ttl: Duration,
}

impl Policy {
    fn from_env(enabled: bool) -> Self {
        let max_per_endpoint = env_usize("YAS_PROCESS_MAX_PER_CLIENT", DEFAULT_MAX_PER_CLIENT);
        let max_generations = env_usize("YAS_PROCESS_MAX", DEFAULT_MAX_GLOBAL);
        let default_max_watchers = max_per_endpoint.saturating_mul(max_generations).max(1);
        Self {
            enabled,
            max_per_endpoint,
            max_generations,
            max_watchers: env_usize("YAS_PROCESS_MAX_WATCHERS", default_max_watchers).max(1),
            max_watchers_per_generation: env_usize(
                "YAS_PROCESS_MAX_WATCHERS_PER_CHILD",
                DEFAULT_MAX_WATCHERS_PER_GENERATION,
            )
            .max(1),
            max_request_per_endpoint: env_usize(
                "YAS_PROCESS_REQUEST_MAX_PER_CLIENT",
                DEFAULT_REQUEST_MAX_PER_CLIENT,
            ),
            max_request: env_usize("YAS_PROCESS_REQUEST_MAX", DEFAULT_REQUEST_MAX),
            max_buffer: env_usize("YAS_PROCESS_BUFFER_MAX", DEFAULT_BUFFER_MAX),
            kill_grace: env_duration("YAS_PROCESS_KILL_GRACE", DEFAULT_KILL_GRACE),
            final_ttl: env_duration("YAS_PROCESS_DETACHED_RESULT_TTL", DEFAULT_FINAL_TTL),
        }
    }
}

struct WriterGuard {
    action: Option<Box<dyn FnOnce() + Send>>,
}

impl WriterGuard {
    fn new(f: impl FnOnce() + Send + 'static) -> Self {
        Self {
            action: Some(Box::new(f)),
        }
    }
}

impl Drop for WriterGuard {
    fn drop(&mut self) {
        if let Some(f) = self.action.take() {
            f();
        }
    }
}

#[derive(Default)]
struct ServerState {
    accepting: bool,
    catalog_revision: u64,
    generations: usize,
    request_bytes: usize,
    buffer_bytes: usize,
    pending: FxHashMap<u64, Weak<Pending>>,
    live: FxHashMap<u64, Weak<Record>>,
    finals: FxHashMap<u64, Arc<FinalRecord>>,
}

#[derive(Clone)]
struct FinalRecord {
    generation: u64,
    pid: ProcessId,
    flags: u8,
    owner_session: [u8; 16],
    argv0: Vec<u8>,
    /// Absolute launch cwd retained for FS PROCESS_CWD after exit.
    cwd: Vec<u8>,
    buffer_bytes: usize,
    stdin_received: u64,
    stdin_acked: u64,
    stdout_next: u64,
    stderr_next: u64,
    stream_state: u8,
    reason: u8,
    kill_cause: u8,
    code: u32,
    detail: &'static str,
}

/// Transport-neutral process catalogue snapshot used by the YAS adapter.
#[derive(Clone, Debug)]
pub(crate) struct NativeSnapshot {
    pub(crate) revision: u64,
    pub(crate) records: Vec<NativeRecord>,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeRecord {
    pub(crate) process_handle: u64,
    pub(crate) running: bool,
    pub(crate) stream_state: u8,
    pub(crate) flags: u8,
    pub(crate) native_pid: u32,
    pub(crate) owner_session: [u8; 16],
    pub(crate) argv0: Vec<u8>,
    pub(crate) stdin_received: u64,
    pub(crate) stdout_produced: u64,
    pub(crate) stderr_produced: u64,
    pub(crate) exit: Option<NativeExit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeExit {
    pub(crate) kind: wire::ExitKind,
    pub(crate) reason: u8,
    pub(crate) code: i32,
    pub(crate) detail: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeSpawnRequest {
    pub(crate) process_id: u32,
    pub(crate) flags: u8,
    /// A surface application launcher may exit after handing ownership to a
    /// process-group descendant. Keep that group alive and represent it as the
    /// running Process until the group is actually empty.
    pub(crate) preserve_residual: bool,
    pub(crate) cwd: Option<Vec<u8>>,
    pub(crate) argv: Vec<Vec<u8>>,
    pub(crate) env: Vec<(Vec<u8>, Vec<u8>)>,
    pub(crate) clear_environment: bool,
}

#[derive(Clone, Debug)]
struct SpawnRequestOwned {
    process_id: u32,
    flags: u8,
    cwd: Option<Vec<u8>>,
    argv: Vec<Vec<u8>>,
    env: Vec<(Vec<u8>, Vec<u8>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeStarted {
    pub(crate) process_id: u32,
    pub(crate) process_handle: u64,
    pub(crate) stdin_window: u64,
    pub(crate) stdout_window: u64,
    pub(crate) stderr_window: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeWatched {
    pub(crate) process_id: u32,
    pub(crate) process_handle: u64,
    pub(crate) running: bool,
    pub(crate) stream_state: u8,
    pub(crate) stdin_received: u64,
    pub(crate) stdin_acked: u64,
    pub(crate) stdout_next: u64,
    pub(crate) stderr_next: u64,
    pub(crate) stdin_window: u64,
    pub(crate) exit: Option<NativeExit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeEvent {
    Output {
        process_id: u32,
        stream: u8,
        offset: u64,
        data: Vec<u8>,
    },
    StdinProgress {
        process_id: u32,
        consumed: u64,
        open: bool,
    },
    Exit {
        process_id: u32,
        exit: NativeExit,
    },
}

pub(crate) struct NativeEventEnvelope {
    pub(crate) event: NativeEvent,
    _guard: Option<WriterGuard>,
}

impl NativeEventEnvelope {
    /// Dispatch the event before releasing its writer guard.
    ///
    /// Terminal guards retire the endpoint binding. Keeping the guard through
    /// adapter dispatch ensures the adapter can publish the terminal value
    /// before a concurrent final output acknowledgement observes retirement.
    pub(crate) fn dispatch<T>(self, dispatch: impl FnOnce(NativeEvent) -> T) -> T {
        let result = dispatch(self.event);
        drop(self._guard);
        result
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeError {
    NotFound,
    Conflict,
    Permission,
    ResourceExhausted,
    Invalid(String),
    Io(String),
    Closed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeControl {
    CloseStdin,
    Signal(u32),
    Terminate,
    Kill,
    Detach,
}

#[derive(Clone)]
struct EndpointOutput {
    events: mpsc::Sender<NativeEventEnvelope>,
    closed: watch::Sender<Option<String>>,
}

impl EndpointOutput {
    fn kick(&self, reason: &str) {
        if self.closed.borrow().is_none() {
            let _ = self.closed.send(Some(reason.to_owned()));
        }
    }

    fn send_native(&self, event: NativeEvent, guard: Option<WriterGuard>) -> bool {
        self.events
            .try_send(NativeEventEnvelope {
                event,
                _guard: guard,
            })
            .is_ok()
    }

    fn send_stdin_progress(&self, process_id: u32, consumed: u64, stdin_state: u8) -> bool {
        self.send_native(
            NativeEvent::StdinProgress {
                process_id,
                consumed,
                open: stdin_state == PROCESS_STDIN_ACCEPTING,
            },
            None,
        )
    }

    fn send_output(&self, process_id: u32, stream: u8, offset: u64, data: &[u8]) -> bool {
        self.send_native(
            NativeEvent::Output {
                process_id,
                stream,
                offset,
                data: data.to_vec(),
            },
            None,
        )
    }

    fn send_exit(&self, process_id: u32, exit: NativeExit, guard: WriterGuard) -> bool {
        self.send_native(NativeEvent::Exit { process_id, exit }, Some(guard))
    }
}

struct ServerInner {
    policy: Policy,
    verbose: bool,
    next_generation: AtomicU64,
    next_endpoint: AtomicU64,
    state: StdMutex<ServerState>,
    catalog_changed: Notify,
    spawn_slots: Semaphore,
    #[cfg(test)]
    terminate_timeout_tasks: AtomicUsize,
}

#[derive(Clone)]
pub(crate) struct Server(Arc<ServerInner>);

impl Server {
    pub(crate) fn new(verbose: bool, enabled: bool) -> Self {
        let policy = Policy::from_env(enabled);
        let max_spawning = env_usize("YAS_PROCESS_MAX_SPAWNING", DEFAULT_MAX_SPAWNING).max(1);
        Self(Arc::new(ServerInner {
            policy,
            verbose,
            next_generation: AtomicU64::new(1),
            next_endpoint: AtomicU64::new(1),
            state: StdMutex::new(ServerState {
                accepting: true,
                // YAS State revisions are nonzero.  Starting at one also
                // gives an empty catalogue a stable snapshot revision.
                catalog_revision: 1,
                ..ServerState::default()
            }),
            catalog_changed: Notify::new(),
            spawn_slots: Semaphore::new(max_spawning),
            #[cfg(test)]
            terminate_timeout_tasks: AtomicUsize::new(0),
        }))
    }

    pub(crate) fn enabled(&self) -> bool {
        self.0.policy.enabled
    }

    #[cfg(test)]
    pub(crate) fn active_terminate_timeout_tasks(&self) -> usize {
        self.0.terminate_timeout_tasks.load(Ordering::Acquire)
    }

    pub(crate) fn native_endpoint_with_session(
        &self,
        session_id: [u8; 16],
        event_capacity: usize,
    ) -> (
        Manager,
        mpsc::Receiver<NativeEventEnvelope>,
        watch::Receiver<Option<String>>,
    ) {
        debug_assert!(session_id.iter().any(|byte| *byte != 0));
        let id = self.0.next_endpoint.fetch_add(1, Ordering::Relaxed);
        let (events, receiver) = mpsc::channel(event_capacity.max(1));
        let (closed, closed_receiver) = watch::channel(None);
        (
            self.endpoint_with_id(EndpointOutput { events, closed }, id, session_id),
            receiver,
            closed_receiver,
        )
    }

    fn endpoint_with_id(&self, out: EndpointOutput, id: u64, session_id: [u8; 16]) -> Manager {
        Manager {
            server: self.clone(),
            endpoint: Arc::new(Endpoint {
                id,
                session_id,
                state: StdMutex::new(EndpointState {
                    accepting: true,
                    ..EndpointState::default()
                }),
            }),
            out,
        }
    }

    pub(crate) fn native_snapshot(&self) -> NativeSnapshot {
        let state = self.0.state.lock().unwrap();
        let mut records = Vec::with_capacity(state.live.len().saturating_add(state.finals.len()));
        for record in state.live.values().filter_map(Weak::upgrade) {
            let inner = record.inner.lock().unwrap();
            records.push(NativeRecord {
                process_handle: record.generation,
                running: true,
                stream_state: stream_state(&inner, record.merged),
                flags: record_flags(&record),
                native_pid: record.pid,
                owner_session: record.owner_session,
                argv0: record.argv0.clone(),
                stdin_received: inner.stdin_received,
                stdout_produced: inner.stdout.next,
                stderr_produced: inner.stderr.as_ref().map_or(0, |stream| stream.next),
                exit: None,
            });
        }
        for record in state.finals.values() {
            records.push(NativeRecord {
                process_handle: record.generation,
                running: false,
                stream_state: 0,
                flags: record.flags,
                native_pid: record.pid,
                owner_session: record.owner_session,
                argv0: record.argv0.clone(),
                stdin_received: record.stdin_received,
                stdout_produced: record.stdout_next,
                stderr_produced: record.stderr_next,
                exit: Some(native_exit(
                    record.reason,
                    record.kill_cause,
                    record.code,
                    record.detail.as_bytes(),
                )),
            });
        }
        records.sort_unstable_by_key(|record| record.process_handle);
        NativeSnapshot {
            revision: state.catalog_revision.max(1),
            records,
        }
    }

    /// Resolve a Process handle to a platform path for FS PROCESS_CWD. A
    /// caller cannot use this as a PID oracle: the opaque generation must be
    /// present in the bounded process catalogue first.
    pub(crate) fn native_cwd(&self, process_handle: u64) -> Option<Vec<u8>> {
        let state = self.0.state.lock().unwrap();
        if let Some(record) = state.live.get(&process_handle).and_then(Weak::upgrade) {
            #[cfg(target_os = "linux")]
            if let Ok(path) = std::fs::read_link(format!("/proc/{}/cwd", record.pid)) {
                use std::os::unix::ffi::OsStrExt;
                return Some(path.as_os_str().as_bytes().to_vec());
            }
            return Some(record.cwd.clone());
        }
        state
            .finals
            .get(&process_handle)
            .map(|record| record.cwd.clone())
    }

    pub(crate) async fn wait_native_catalogue_change(&self, revision: u64) {
        loop {
            let notified = self.0.catalog_changed.notified();
            if self.0.state.lock().unwrap().catalog_revision != revision {
                return;
            }
            notified.await;
        }
    }

    fn reserve_buffer(&self, bytes: usize) -> bool {
        let mut state = self.0.state.lock().unwrap();
        let Some(next) = state.buffer_bytes.checked_add(bytes) else {
            return false;
        };
        if next > self.0.policy.max_buffer {
            return false;
        }
        state.buffer_bytes = next;
        true
    }

    fn release_buffer(&self, bytes: usize) {
        let mut state = self.0.state.lock().unwrap();
        state.buffer_bytes = state.buffer_bytes.saturating_sub(bytes);
    }

    fn finish_detached(&self, record: Arc<Record>, final_record: Arc<FinalRecord>) {
        if record.released.load(Ordering::Acquire) {
            return;
        }
        let installed = {
            let mut state = self.0.state.lock().unwrap();
            let current = state.live.get(&record.generation);
            if !matches!(current, Some(live) if live.ptr_eq(&Arc::downgrade(&record))) {
                false
            } else {
                state.live.remove(&record.generation);
                state.finals.insert(record.generation, final_record.clone());
                state.catalog_revision = state.catalog_revision.wrapping_add(1);
                self.0.catalog_changed.notify_waiters();
                true
            }
        };
        if !installed {
            return;
        }
        if !record.buffer_released.swap(true, Ordering::AcqRel) {
            self.release_buffer(
                record
                    .buffer_bytes
                    .saturating_sub(final_record.buffer_bytes),
            );
        }
        let server = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(server.0.policy.final_ttl).await;
            let mut state = server.0.state.lock().unwrap();
            let matches = state
                .finals
                .get(&final_record.generation)
                .is_some_and(|current| Arc::ptr_eq(current, &final_record));
            if matches {
                state.finals.remove(&final_record.generation);
                state.generations = state.generations.saturating_sub(1);
                state.buffer_bytes = state.buffer_bytes.saturating_sub(final_record.buffer_bytes);
                state.catalog_revision = state.catalog_revision.wrapping_add(1);
                server.0.catalog_changed.notify_waiters();
            }
        });
    }

    fn release_record(&self, record: &Record) {
        if record.released.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut state = self.0.state.lock().unwrap();
        if !record.buffer_released.swap(true, Ordering::AcqRel) {
            state.buffer_bytes = state.buffer_bytes.saturating_sub(record.buffer_bytes);
        }
        let remove = matches!(
            state.live.get(&record.generation),
            Some(live) if std::ptr::eq(live.as_ptr(), record)
        );
        if remove {
            state.live.remove(&record.generation);
            state.generations = state.generations.saturating_sub(1);
            state.catalog_revision = state.catalog_revision.wrapping_add(1);
            self.0.catalog_changed.notify_waiters();
        }
        drop(state);
        if let Some(owner) = record.owner.upgrade() {
            owner.state.lock().unwrap().owned.remove(&record.generation);
        }
    }

    pub(crate) async fn shutdown(&self) {
        let pending = {
            let mut state = self.0.state.lock().unwrap();
            state.accepting = false;
            state
                .pending
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        for pending in &pending {
            // Exactly one spawn task waits on this notification. `notify_one`
            // stores a permit if that task has not reached its select yet.
            pending.cancel.notify_one();
            if pending
                .phase
                .compare_exchange(
                    PENDING_QUEUED,
                    PENDING_DONE,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                release_pending(pending, false);
                pending.mark_completed();
            }
        }
        for pending in &pending {
            while !pending.completed.load(Ordering::Acquire) {
                let done = pending.done.notified();
                if pending.completed.load(Ordering::Acquire) {
                    break;
                }
                done.await;
            }
        }
        let live = {
            let state = self.0.state.lock().unwrap();
            state
                .live
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        // A server-wide shutdown intentionally discards all subscriptions.
        // Remove them before aborting pipes so terminal publication cannot
        // enqueue replies into endpoints which are shutting down too.
        for record in &live {
            release_record_endpoint_slots(record);
        }
        for record in &live {
            terminate_record(
                record,
                PROCESS_KILL_SERVER_SHUTDOWN,
                self.0.policy.kill_grace,
            )
            .await;
        }
        for record in &live {
            self.release_record(record);
        }
        let mut state = self.0.state.lock().unwrap();
        let finals = state.finals.len();
        let final_bytes = state
            .finals
            .values()
            .map(|record| record.buffer_bytes)
            .sum::<usize>();
        if !state.finals.is_empty() {
            state.catalog_revision = state.catalog_revision.wrapping_add(1);
            self.0.catalog_changed.notify_waiters();
        }
        state.finals.clear();
        state.generations = state.generations.saturating_sub(finals);
        state.buffer_bytes = state.buffer_bytes.saturating_sub(final_bytes);
    }
}

#[derive(Default)]
struct EndpointState {
    accepting: bool,
    request_bytes: usize,
    slots: FxHashMap<u32, EndpointSlot>,
    /// Ordinary processes remain owned after their creator unsubscribes.
    owned: FxHashMap<u64, Weak<Record>>,
}

enum EndpointSlot {
    Pending(Arc<Pending>),
    Bound(Arc<Record>),
}

struct Endpoint {
    id: u64,
    session_id: [u8; 16],
    state: StdMutex<EndpointState>,
}

fn endpoint_usage(state: &EndpointState) -> usize {
    let unbound_owned = state.owned.keys().filter(|generation| {
        !state.slots.values().any(
            |slot| matches!(slot, EndpointSlot::Bound(record) if record.generation == **generation),
        )
    });
    state.slots.len().saturating_add(unbound_owned.count())
}

/// Project usage after adding a watch, once the caller has established that
/// this endpoint does not already watch the generation.
fn endpoint_usage_after_watch(state: &EndpointState, process_ref: ProcessRef) -> usize {
    endpoint_usage(state).saturating_add(usize::from(!state.owned.contains_key(&process_ref)))
}

fn active_watcher_count(state: &ServerState) -> usize {
    let pending = state
        .pending
        .values()
        .filter(|pending| pending.strong_count() != 0)
        .count();
    state
        .live
        .values()
        .filter_map(Weak::upgrade)
        .fold(pending, |count, record| {
            count.saturating_add(record.inner.lock().unwrap().bindings.len())
        })
}

struct Pending {
    generation: u64,
    process_id: u32,
    detachable: bool,
    preserve_residual: bool,
    request_bytes: usize,
    endpoint: Weak<Endpoint>,
    server: Weak<ServerInner>,
    out: EndpointOutput,
    completion: StdMutex<Option<SpawnCompletion>>,
    phase: AtomicU8,
    endpoint_lost: AtomicBool,
    request_released: AtomicBool,
    /// `phase = DONE` prevents duplicate completion. This separate flag is
    /// published only after registry/accounting transition is fully visible.
    completed: AtomicBool,
    cancel: Notify,
    done: Notify,
}

enum SpawnCompletion {
    Native(oneshot::Sender<Result<NativeStarted, NativeError>>),
}

impl Pending {
    fn mark_completed(&self) {
        self.completed.store(true, Ordering::Release);
        self.done.notify_waiters();
    }
}

#[derive(Clone)]
struct BindingStream {
    floor: u64,
    acked: u64,
    frames: VecDeque<u64>,
}

struct Binding {
    endpoint_id: u64,
    process_id: u32,
    endpoint: Weak<Endpoint>,
    out: EndpointOutput,
    stdout: BindingStream,
    stderr: Option<BindingStream>,
}

struct StreamState {
    next: u64,
}

#[derive(Clone, Copy)]
enum ChildOutcome {
    Returned(u32),
    #[cfg(unix)]
    Signalled(u32),
    HostFailure,
}

#[derive(Clone, Copy)]
struct ExitOverride {
    reason: u8,
    kill_cause: u8,
}

struct InputChunk {
    end: u64,
    data: Vec<u8>,
}

struct Spawned {
    child: Child,
    pid: ProcessId,
    #[cfg(windows)]
    job: JobHandle,
}

struct RecordInner {
    bindings: Vec<Binding>,
    /// At most one endpoint may advance the generation-wide stdin cursor.
    /// This is an openly reacquirable writer role, not an authorization token.
    stdin_controller: Option<u64>,
    stdin_tx: Option<mpsc::Sender<InputChunk>>,
    stdin_received: u64,
    stdin_acked: u64,
    stdin_frames: VecDeque<u64>,
    stdin_state: u8,
    stdin_closed_by_child: bool,
    stdin_writer_done: bool,
    stdout: StreamState,
    stderr: Option<StreamState>,
    stdout_readers: u8,
    stderr_readers: u8,
    child_outcome: Option<ChildOutcome>,
    tree_cleanup_done: bool,
    exit_override: Option<ExitOverride>,
    terminate_timeout_armed: bool,
    terminal_queued: bool,
    cleanup_detail: &'static str,
    output_aborts: Vec<AbortHandle>,
    stdin_abort: Option<AbortHandle>,
}

struct Record {
    generation: u64,
    detachable: bool,
    preserve_residual: bool,
    pid: ProcessId,
    argv0: Vec<u8>,
    /// Absolute launch cwd. Linux PROCESS_CWD prefers the child's live cwd
    /// and falls back to this value after exit.
    cwd: Vec<u8>,
    owner: Weak<Endpoint>,
    owner_session: [u8; 16],
    #[cfg(windows)]
    job: JobHandle,
    merged: bool,
    buffer_bytes: usize,
    server: Server,
    inner: StdMutex<RecordInner>,
    changed: Notify,
    reaped: AtomicBool,
    reaped_notify: Notify,
    terminal_notify: Notify,
    buffer_released: AtomicBool,
    released: AtomicBool,
}

impl Record {
    fn mark_reaped(&self) {
        self.reaped.store(true, Ordering::Release);
        self.reaped_notify.notify_waiters();
    }

    async fn wait_reaped(&self) {
        while !self.reaped.load(Ordering::Acquire) {
            let notified = self.reaped_notify.notified();
            if self.reaped.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    async fn wait_terminal(&self) {
        loop {
            let notified = self.terminal_notify.notified();
            if self.inner.lock().unwrap().terminal_queued {
                return;
            }
            notified.await;
        }
    }

    async fn wait_tree_cleanup(&self) {
        loop {
            let changed = self.changed.notified();
            if self.inner.lock().unwrap().tree_cleanup_done {
                return;
            }
            changed.await;
        }
    }
}

#[derive(Clone)]
pub(crate) struct Manager {
    server: Server,
    endpoint: Arc<Endpoint>,
    out: EndpointOutput,
}

impl Manager {
    pub(crate) async fn spawn_native(
        &self,
        request: NativeSpawnRequest,
        session_env: Option<crate::app_env::SessionEnv>,
    ) -> Result<NativeStarted, NativeError> {
        if request.process_id == 0
            || request.argv.is_empty()
            || request.argv[0].is_empty()
            || request.flags & !(PROCESS_SPAWN_MERGE_STDERR | PROCESS_SPAWN_DETACHABLE) != 0
        {
            return Err(NativeError::Invalid("invalid Process spawn".to_owned()));
        }
        #[cfg(windows)]
        {
            let strings_valid = request
                .argv
                .iter()
                .chain(request.cwd.iter())
                .all(|value| std::str::from_utf8(value).is_ok())
                && request.env.iter().all(|(key, value)| {
                    std::str::from_utf8(key).is_ok() && std::str::from_utf8(value).is_ok()
                });
            if !strings_valid {
                return Err(NativeError::Invalid(
                    "process strings must be valid Windows UTF-8".to_owned(),
                ));
            }
        }
        let request_bytes = request
            .argv
            .iter()
            .map(Vec::len)
            .chain(request.cwd.iter().map(Vec::len))
            .chain(
                request
                    .env
                    .iter()
                    .map(|(key, value)| key.len().saturating_add(value.len())),
            )
            .try_fold(32usize, usize::checked_add)
            .ok_or(NativeError::ResourceExhausted)?;
        let clear_environment = request.clear_environment;
        let owned = SpawnRequestOwned {
            process_id: request.process_id,
            flags: request.flags,
            cwd: request.cwd,
            argv: request.argv,
            env: request.env,
        };
        let detachable = owned.flags & PROCESS_SPAWN_DETACHABLE != 0;
        let generation = self
            .server
            .0
            .next_generation
            .fetch_add(1, Ordering::Relaxed);
        let (completion, receiver) = oneshot::channel();
        let pending = Arc::new(Pending {
            generation,
            process_id: owned.process_id,
            detachable,
            preserve_residual: request.preserve_residual,
            request_bytes,
            endpoint: Arc::downgrade(&self.endpoint),
            server: Arc::downgrade(&self.server.0),
            out: self.out.clone(),
            completion: StdMutex::new(Some(SpawnCompletion::Native(completion))),
            phase: AtomicU8::new(PENDING_QUEUED),
            endpoint_lost: AtomicBool::new(false),
            request_released: AtomicBool::new(false),
            completed: AtomicBool::new(false),
            cancel: Notify::new(),
            done: Notify::new(),
        });
        {
            let mut server = self.server.0.state.lock().unwrap();
            let mut endpoint = self.endpoint.state.lock().unwrap();
            if !server.accepting || !endpoint.accepting {
                return Err(NativeError::Permission);
            }
            if endpoint.slots.contains_key(&owned.process_id) {
                return Err(NativeError::Conflict);
            }
            let request_next_server = server.request_bytes.checked_add(request_bytes);
            let request_next_endpoint = endpoint.request_bytes.checked_add(request_bytes);
            let budget = endpoint_usage(&endpoint) >= self.server.0.policy.max_per_endpoint
                || server.generations >= self.server.0.policy.max_generations
                || active_watcher_count(&server) >= self.server.0.policy.max_watchers
                || request_next_server.is_none_or(|next| next > self.server.0.policy.max_request)
                || request_next_endpoint
                    .is_none_or(|next| next > self.server.0.policy.max_request_per_endpoint);
            if budget {
                return Err(NativeError::ResourceExhausted);
            }
            server.generations += 1;
            server.request_bytes = request_next_server.unwrap();
            server.pending.insert(generation, Arc::downgrade(&pending));
            endpoint.request_bytes = request_next_endpoint.unwrap();
            endpoint
                .slots
                .insert(owned.process_id, EndpointSlot::Pending(pending.clone()));
        }
        let manager = self.clone();
        tokio::spawn(async move {
            manager
                .run_spawn(pending, owned, session_env, clear_environment)
                .await;
        });
        receiver
            .await
            .map_err(|_| NativeError::Closed("Process spawn response closed".to_owned()))?
    }

    async fn run_spawn(
        &self,
        pending: Arc<Pending>,
        request: SpawnRequestOwned,
        session_env: Option<crate::app_env::SessionEnv>,
        clear_environment: bool,
    ) {
        let permit = tokio::select! {
            permit = self.server.0.spawn_slots.acquire() => permit.ok(),
            _ = pending.cancel.notified() => None,
        };
        let Some(permit) = permit else {
            return;
        };
        if pending
            .phase
            .compare_exchange(
                PENDING_QUEUED,
                PENDING_ACTIVE,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }
        let req = &request;
        let merged = req.flags & PROCESS_SPAWN_MERGE_STDERR != 0;
        let streams = if merged { 2 } else { 3 };
        let buffer_bytes =
            (PROCESS_DEFAULT_STREAM_WINDOW as usize * streams).saturating_add(req.argv[0].len());
        if !self.server.reserve_buffer(buffer_bytes) {
            drop(permit);
            complete_spawn_failure(&pending, NativeError::ResourceExhausted);
            return;
        }
        let process_cwd = process_launch_cwd(req);
        let mut command = command_for(req, session_env.as_ref(), clear_environment);
        let merged_reader = if merged {
            match configure_merged_output(&mut command) {
                Ok(reader) => Some(reader),
                Err(error) => {
                    self.server.release_buffer(buffer_bytes);
                    drop(permit);
                    complete_spawn_failure(&pending, NativeError::Io(error.to_string()));
                    return;
                }
            }
        } else {
            None
        };
        let spawned = spawn_child(&mut command);
        drop(permit);
        let spawned = match spawned {
            Ok(spawned) => spawned,
            Err(error) => {
                self.server.release_buffer(buffer_bytes);
                let failure = match error.kind() {
                    io::ErrorKind::NotFound => NativeError::NotFound,
                    io::ErrorKind::PermissionDenied => NativeError::Permission,
                    io::ErrorKind::InvalidInput => NativeError::Invalid(error.to_string()),
                    _ => NativeError::Io(error.to_string()),
                };
                complete_spawn_failure(&pending, failure);
                return;
            }
        };
        let Spawned {
            mut child,
            pid,
            #[cfg(windows)]
            job,
        } = spawned;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = (!merged).then(|| child.stdout.take().expect("piped stdout"));
        let stderr = (!merged).then(|| child.stderr.take().expect("piped stderr"));
        let (stdin_tx, stdin_rx) = mpsc::channel(PROCESS_MAX_UNACKED_PACKETS);
        let bindings = pending
            .endpoint
            .upgrade()
            .filter(|endpoint| endpoint.state.lock().unwrap().accepting)
            .map(|endpoint| {
                vec![Binding::new(
                    endpoint.id,
                    pending.process_id,
                    Arc::downgrade(&endpoint),
                    pending.out.clone(),
                    merged,
                    0,
                    0,
                )]
            })
            .unwrap_or_default();
        let stdin_controller = bindings.first().map(|binding| binding.endpoint_id);
        let record = Arc::new(Record {
            generation: pending.generation,
            detachable: pending.detachable,
            preserve_residual: pending.preserve_residual,
            pid,
            argv0: req.argv[0].to_vec(),
            cwd: process_cwd,
            owner: pending.endpoint.clone(),
            owner_session: pending
                .endpoint
                .upgrade()
                .map_or([0xff; 16], |endpoint| endpoint.session_id),
            #[cfg(windows)]
            job,
            merged,
            buffer_bytes,
            server: self.server.clone(),
            inner: StdMutex::new(RecordInner {
                bindings,
                stdin_controller,
                stdin_tx: Some(stdin_tx),
                stdin_received: 0,
                stdin_acked: 0,
                stdin_frames: VecDeque::new(),
                stdin_state: PROCESS_STDIN_ACCEPTING,
                stdin_closed_by_child: false,
                stdin_writer_done: false,
                stdout: StreamState { next: 0 },
                stderr: (!merged).then_some(StreamState { next: 0 }),
                stdout_readers: 1,
                stderr_readers: if merged { 0 } else { 1 },
                child_outcome: None,
                tree_cleanup_done: false,
                exit_override: None,
                terminate_timeout_armed: false,
                terminal_queued: false,
                cleanup_detail: "",
                output_aborts: Vec::new(),
                stdin_abort: None,
            }),
            changed: Notify::new(),
            reaped: AtomicBool::new(false),
            reaped_notify: Notify::new(),
            terminal_notify: Notify::new(),
            buffer_released: AtomicBool::new(false),
            released: AtomicBool::new(false),
        });
        // Install every task and abort handle before publishing the live
        // record. The semaphore keeps them from emitting output or observing
        // exit until STARTED has been queued, while its stored permits make
        // publication safe even if a task has not been polled yet.
        let task_start = Arc::new(Semaphore::new(0));
        let stdin_start = task_start.clone();
        let stdin_record = record.clone();
        let stdin_task = tokio::spawn(async move {
            let permit = stdin_start
                .acquire()
                .await
                .expect("spawn task gate remains open");
            permit.forget();
            stdin_writer(stdin_record, stdin, stdin_rx).await;
        });
        let stdout_start = task_start.clone();
        let stdout_record = record.clone();
        let stdout_task = if let Some(reader) = merged_reader {
            tokio::spawn(async move {
                let permit = stdout_start
                    .acquire()
                    .await
                    .expect("spawn task gate remains open");
                permit.forget();
                output_reader(stdout_record, PROCESS_STREAM_STDOUT, reader).await;
            })
        } else {
            tokio::spawn(async move {
                let permit = stdout_start
                    .acquire()
                    .await
                    .expect("spawn task gate remains open");
                permit.forget();
                output_reader(
                    stdout_record,
                    PROCESS_STREAM_STDOUT,
                    stdout.expect("separate stdout pipe"),
                )
                .await;
            })
        };
        let stderr_task = stderr.map(|reader| {
            let start = task_start.clone();
            let record = record.clone();
            tokio::spawn(async move {
                let permit = start.acquire().await.expect("spawn task gate remains open");
                permit.forget();
                output_reader(record, PROCESS_STREAM_STDERR, reader).await;
            })
        });
        let task_count = 3 + usize::from(stderr_task.is_some());
        {
            let mut inner = record.inner.lock().unwrap();
            inner.stdin_abort = Some(stdin_task.abort_handle());
            inner.output_aborts = vec![stdout_task.abort_handle()];
            if let Some(stderr_task) = &stderr_task {
                inner.output_aborts.push(stderr_task.abort_handle());
            }
        }
        let wait_start = task_start.clone();
        let wait_record = record.clone();
        tokio::spawn(async move {
            let permit = wait_start
                .acquire()
                .await
                .expect("spawn task gate remains open");
            permit.forget();
            wait_child(wait_record, child).await;
        });

        let installed_bound = transfer_pending_to_record(&pending, &record);
        if !installed_bound && !pending.detachable {
            let mut inner = record.inner.lock().unwrap();
            if graceful_terminate(&record).is_ok() {
                inner.exit_override = Some(ExitOverride {
                    reason: PROCESS_EXIT_KILLED,
                    kill_cause: PROCESS_KILL_OWNER_LOST,
                });
            }
            drop(inner);
            schedule_terminate_timeout(record.clone(), PROCESS_KILL_OWNER_LOST);
        }
        if installed_bound {
            complete_spawn_success(&pending, record.generation, merged);
        }
        task_start.add_permits(task_count);
        if self.server.0.verbose {
            eprintln!(
                "Process Spawn: generation={} process_id={} pid={} argv0={:?}",
                record.generation,
                req.process_id,
                pid,
                String::from_utf8_lossy(&req.argv[0])
            );
        }
    }

    pub(crate) fn write_stdin_native(
        &self,
        process_id: u32,
        offset: u64,
        data: &[u8],
    ) -> Result<(), NativeError> {
        let record = self.get(process_id).ok_or(NativeError::NotFound)?;
        let mut inner = record.inner.lock().unwrap();
        if binding_index(&inner, self.endpoint.id, process_id).is_none()
            || inner.child_outcome.is_some()
            || inner.terminal_queued
        {
            return Err(NativeError::NotFound);
        }
        if inner.stdin_controller != Some(self.endpoint.id)
            || inner.stdin_state != PROCESS_STDIN_ACCEPTING
        {
            send_stdin_ack_to(&inner, self.endpoint.id, process_id);
            return Ok(());
        }
        let Some(end) = offset.checked_add(data.len() as u64) else {
            return Err(NativeError::Invalid(
                "Process stdin offset overflow".to_owned(),
            ));
        };
        let Some(limit) = inner.stdin_acked.checked_add(PROCESS_DEFAULT_STREAM_WINDOW) else {
            return Err(NativeError::Invalid(
                "Process stdin window overflow".to_owned(),
            ));
        };
        if offset != inner.stdin_received
            || end > limit
            || inner.stdin_frames.len() >= PROCESS_MAX_UNACKED_PACKETS
        {
            send_stdin_ack_to(&inner, self.endpoint.id, process_id);
            return Ok(());
        }
        let Some(tx) = inner.stdin_tx.as_ref() else {
            send_stdin_ack_to(&inner, self.endpoint.id, process_id);
            return Ok(());
        };
        match tx.try_send(InputChunk {
            end,
            data: data.to_vec(),
        }) {
            Ok(()) => {
                inner.stdin_received = end;
                inner.stdin_frames.push_back(end);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                inner.stdin_state = PROCESS_STDIN_CLOSED;
                inner.stdin_closed_by_child = true;
                inner.stdin_tx = None;
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                send_stdin_ack_to(&inner, self.endpoint.id, process_id);
            }
        }
        Ok(())
    }

    pub(crate) fn acknowledge_output_native(
        &self,
        process_id: u32,
        stream: u8,
        consumed: u64,
    ) -> Result<(), NativeError> {
        let record = self.get(process_id).ok_or(NativeError::NotFound)?;
        let mut inner = record.inner.lock().unwrap();
        let next = match stream {
            PROCESS_STREAM_STDOUT => inner.stdout.next,
            PROCESS_STREAM_STDERR if !record.merged => inner.stderr.as_ref().unwrap().next,
            _ => {
                return Err(NativeError::Invalid(
                    "invalid Process output stream".to_owned(),
                ));
            }
        };
        // All output has already been emitted when terminal publication begins.
        // A consumer may still acknowledge one of those queued frames while the
        // EXIT event is in flight; that acknowledgement is harmless and must not
        // turn a successful attachment into a protocol failure.
        if inner.terminal_queued {
            return if consumed <= next {
                Ok(())
            } else {
                Err(NativeError::Invalid(
                    "invalid Process output cursor".to_owned(),
                ))
            };
        }
        let Some(index) = binding_index(&inner, self.endpoint.id, process_id) else {
            return Err(NativeError::NotFound);
        };
        let binding = &mut inner.bindings[index];
        let credit = if stream == PROCESS_STREAM_STDOUT {
            &mut binding.stdout
        } else {
            binding.stderr.as_mut().expect("separate stderr binding")
        };
        if consumed < credit.floor || consumed < credit.acked || consumed > next {
            return Err(NativeError::Invalid(
                "invalid Process output cursor".to_owned(),
            ));
        }
        credit.acked = consumed;
        while credit.frames.front().is_some_and(|end| *end <= consumed) {
            credit.frames.pop_front();
        }
        drop(inner);
        record.changed.notify_waiters();
        Ok(())
    }

    pub(crate) fn control_native(
        &self,
        process_id: u32,
        action: NativeControl,
    ) -> Result<(), NativeError> {
        let record = self.get(process_id).ok_or(NativeError::NotFound)?;
        let mut timeout_cause = None;
        let mut detached = false;
        {
            let mut inner = record.inner.lock().unwrap();
            let Some(binding) = binding_index(&inner, self.endpoint.id, process_id) else {
                return Err(NativeError::NotFound);
            };
            let residual_running = record.preserve_residual
                && inner.child_outcome.is_some()
                && !inner.tree_cleanup_done;
            if inner.terminal_queued || (inner.child_outcome.is_some() && !residual_running) {
                return Err(NativeError::Conflict);
            }
            match action {
                NativeControl::CloseStdin => {
                    if inner.stdin_state == PROCESS_STDIN_ACCEPTING {
                        inner.stdin_state = PROCESS_STDIN_CLOSING;
                        inner.stdin_tx.take();
                        send_stdin_ack(&inner, inner.stdin_acked, PROCESS_STDIN_CLOSING);
                    }
                }
                NativeControl::Terminate => {
                    graceful_terminate(&record)
                        .map_err(|error| NativeError::Io(error.to_string()))?;
                    timeout_cause = Some(PROCESS_KILL_TERMINATE_TIMEOUT);
                }
                NativeControl::Kill => {
                    force_kill(&record).map_err(|error| NativeError::Io(error.to_string()))?;
                    inner.exit_override = Some(ExitOverride {
                        reason: PROCESS_EXIT_KILLED,
                        kill_cause: PROCESS_KILL_CLIENT,
                    });
                }
                NativeControl::Signal(signal) => {
                    control_signal(&record, signal)?;
                }
                NativeControl::Detach => {
                    inner.bindings.swap_remove(binding);
                    if inner.stdin_controller == Some(self.endpoint.id) {
                        inner.stdin_controller = None;
                    }
                    detached = true;
                }
            }
        }
        if detached {
            remove_bound_slot(&self.endpoint, process_id, &record);
            record.changed.notify_waiters();
        }
        if let Some(cause) = timeout_cause {
            schedule_terminate_timeout(record, cause);
        }
        Ok(())
    }

    pub(crate) fn watch_native(
        &self,
        process_id: u32,
        process_handle: u64,
        stdin: bool,
    ) -> Result<NativeWatched, NativeError> {
        if process_id == 0 || process_handle == 0 {
            return Err(NativeError::Invalid("invalid Process WATCH".to_owned()));
        }
        let server = self.server.0.state.lock().unwrap();
        let mut endpoint = self.endpoint.state.lock().unwrap();
        if !server.accepting || !endpoint.accepting {
            return Err(NativeError::Permission);
        }
        if endpoint.slots.contains_key(&process_id) {
            return Err(NativeError::Conflict);
        }
        if let Some(record) = server.live.get(&process_handle).and_then(Weak::upgrade) {
            let global_full = active_watcher_count(&server) >= self.server.0.policy.max_watchers;
            let mut inner = record.inner.lock().unwrap();
            if inner
                .bindings
                .iter()
                .any(|binding| binding.endpoint_id == self.endpoint.id)
            {
                return Err(NativeError::Conflict);
            }
            if global_full
                || inner.bindings.len() >= self.server.0.policy.max_watchers_per_generation
                || endpoint_usage_after_watch(&endpoint, process_handle)
                    > self.server.0.policy.max_per_endpoint
            {
                return Err(NativeError::ResourceExhausted);
            }
            if inner.terminal_queued {
                return Err(NativeError::Conflict);
            }
            if stdin
                && (inner.child_outcome.is_some()
                    || inner.stdin_controller.is_some()
                    || inner.stdin_state != PROCESS_STDIN_ACCEPTING)
            {
                return Err(NativeError::Conflict);
            }
            let stdout_next = inner.stdout.next;
            let stderr_next = inner.stderr.as_ref().map_or(0, |stream| stream.next);
            let mut streams = stream_state(&inner, record.merged);
            if stdin && streams & PROCESS_STREAM_STDIN_ACCEPTING != 0 {
                streams |= PROCESS_STREAM_STDIN_WRITABLE;
            }
            if stdin {
                inner.stdin_controller = Some(self.endpoint.id);
            }
            inner.bindings.push(Binding::new(
                self.endpoint.id,
                process_id,
                Arc::downgrade(&self.endpoint),
                self.out.clone(),
                record.merged,
                stdout_next,
                stderr_next,
            ));
            endpoint
                .slots
                .insert(process_id, EndpointSlot::Bound(record.clone()));
            let watched = NativeWatched {
                process_id,
                process_handle,
                running: true,
                stream_state: streams,
                stdin_received: inner.stdin_received,
                stdin_acked: inner.stdin_acked,
                stdout_next,
                stderr_next,
                stdin_window: if streams & PROCESS_STREAM_STDIN_WRITABLE != 0 {
                    PROCESS_DEFAULT_STREAM_WINDOW
                } else {
                    0
                },
                exit: None,
            };
            drop(inner);
            drop(endpoint);
            drop(server);
            record.changed.notify_waiters();
            Ok(watched)
        } else if let Some(record) = server.finals.get(&process_handle) {
            if endpoint_usage(&endpoint) >= self.server.0.policy.max_per_endpoint {
                return Err(NativeError::ResourceExhausted);
            }
            if stdin {
                return Err(NativeError::Conflict);
            }
            Ok(NativeWatched {
                process_id,
                process_handle,
                running: false,
                stream_state: record.stream_state,
                stdin_received: record.stdin_received,
                stdin_acked: record.stdin_acked,
                stdout_next: record.stdout_next,
                stderr_next: record.stderr_next,
                stdin_window: 0,
                exit: Some(native_exit(
                    record.reason,
                    record.kill_cause,
                    record.code,
                    record.detail.as_bytes(),
                )),
            })
        } else {
            Err(NativeError::NotFound)
        }
    }

    fn get(&self, process_id: u32) -> Option<Arc<Record>> {
        match self.endpoint.state.lock().unwrap().slots.get(&process_id) {
            Some(EndpointSlot::Bound(record)) => Some(record.clone()),
            _ => None,
        }
    }

    pub(crate) async fn shutdown(&self) {
        let (slots, owned) = {
            let mut endpoint = self.endpoint.state.lock().unwrap();
            endpoint.accepting = false;
            (
                std::mem::take(&mut endpoint.slots),
                std::mem::take(&mut endpoint.owned),
            )
        };
        let mut ordinary = owned
            .into_iter()
            .filter_map(|(generation, record)| record.upgrade().map(|record| (generation, record)))
            .collect::<FxHashMap<_, _>>();
        let mut active_pending = Vec::new();
        for (process_id, slot) in slots {
            match slot {
                EndpointSlot::Pending(pending) => {
                    pending.endpoint_lost.store(true, Ordering::Release);
                    // Store cancellation even if run_spawn has not polled its
                    // semaphore/cancel select yet.
                    pending.cancel.notify_one();
                    if pending
                        .phase
                        .compare_exchange(
                            PENDING_QUEUED,
                            PENDING_DONE,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        release_pending(&pending, false);
                        pending.mark_completed();
                    } else if !pending.completed.load(Ordering::Acquire) {
                        active_pending.push(pending);
                    }
                }
                EndpointSlot::Bound(record) => {
                    let mut inner = record.inner.lock().unwrap();
                    if let Some(index) = binding_index(&inner, self.endpoint.id, process_id) {
                        inner.bindings.swap_remove(index);
                        if inner.stdin_controller == Some(self.endpoint.id) {
                            inner.stdin_controller = None;
                        }
                    }
                    drop(inner);
                    record.changed.notify_waiters();
                }
            }
        }
        // A native spawn call which had already acquired its semaphore cannot
        // be canceled safely. Let it finish installing its unbound result,
        // then collect ordinary children here before the connection returns.
        // Detachable children deliberately remain in the server registry.
        for pending in active_pending {
            while !pending.completed.load(Ordering::Acquire) {
                let done = pending.done.notified();
                if pending.completed.load(Ordering::Acquire) {
                    break;
                }
                done.await;
            }
            if pending.detachable {
                continue;
            }
            let record = self
                .server
                .0
                .state
                .lock()
                .unwrap()
                .live
                .get(&pending.generation)
                .and_then(Weak::upgrade);
            if let Some(record) = record {
                ordinary.insert(record.generation, record);
            }
        }
        let ordinary = ordinary.into_values().collect::<Vec<_>>();
        for record in &ordinary {
            let mut inner = record.inner.lock().unwrap();
            inner.stdin_tx.take();
            if inner.child_outcome.is_none()
                && inner.exit_override.is_none()
                && cleanup_terminate(record).is_ok()
            {
                inner.exit_override = Some(ExitOverride {
                    reason: PROCESS_EXIT_KILLED,
                    kill_cause: PROCESS_KILL_OWNER_LOST,
                });
            }
        }
        wait_and_force(
            &ordinary,
            PROCESS_KILL_OWNER_LOST,
            self.server.0.policy.kill_grace,
        )
        .await;
        for record in &ordinary {
            abort_pipes(record);
        }
        // Pipe abortion makes terminal publication eligible. Keep shutdown
        // bounded, but leave any unusually slow record live so its own waiter
        // can publish the eventual EXIT instead of orphaning peer watchers.
        let terminals = async {
            for record in &ordinary {
                record.wait_terminal().await;
            }
        };
        let _ = tokio::time::timeout(
            self.server
                .0
                .policy
                .kill_grace
                .max(Duration::from_millis(100)),
            terminals,
        )
        .await;
        self.endpoint.state.lock().unwrap().request_bytes = 0;
    }
}

impl Binding {
    fn new(
        endpoint_id: u64,
        process_id: u32,
        endpoint: Weak<Endpoint>,
        out: EndpointOutput,
        merged: bool,
        stdout_floor: u64,
        stderr_floor: u64,
    ) -> Self {
        Self {
            endpoint_id,
            process_id,
            endpoint,
            out,
            stdout: BindingStream {
                floor: stdout_floor,
                acked: stdout_floor,
                frames: VecDeque::new(),
            },
            stderr: (!merged).then(|| BindingStream {
                floor: stderr_floor,
                acked: stderr_floor,
                frames: VecDeque::new(),
            }),
        }
    }
}

fn record_flags(record: &Record) -> u8 {
    let mut flags = 0;
    if record.merged {
        flags |= PROCESS_SPAWN_MERGE_STDERR;
    }
    if record.detachable {
        flags |= PROCESS_SPAWN_DETACHABLE;
    }
    flags
}

fn binding_index(inner: &RecordInner, endpoint_id: u64, process_id: u32) -> Option<usize> {
    inner
        .bindings
        .iter()
        .position(|binding| binding.endpoint_id == endpoint_id && binding.process_id == process_id)
}

fn remove_binding_at(inner: &mut RecordInner, index: usize) -> Binding {
    let endpoint_id = inner.bindings[index].endpoint_id;
    let binding = inner.bindings.swap_remove(index);
    if inner.stdin_controller == Some(endpoint_id) {
        inner.stdin_controller = None;
    }
    binding
}

fn remove_bound_slot(endpoint: &Endpoint, process_id: u32, record: &Arc<Record>) {
    let mut state = endpoint.state.lock().unwrap();
    if matches!(state.slots.get(&process_id), Some(EndpointSlot::Bound(current)) if Arc::ptr_eq(current, record))
    {
        state.slots.remove(&process_id);
    }
}

fn release_record_endpoint_slots(record: &Arc<Record>) {
    let bindings = {
        let mut inner = record.inner.lock().unwrap();
        inner.stdin_controller = None;
        std::mem::take(&mut inner.bindings)
    };
    for binding in bindings {
        if let Some(endpoint) = binding.endpoint.upgrade() {
            remove_bound_slot(&endpoint, binding.process_id, record);
        }
    }
    record.changed.notify_waiters();
}

fn release_pending(pending: &Arc<Pending>, keep_generation: bool) {
    let Some(server) = pending.server.upgrade() else {
        return;
    };
    let mut server_state = server.state.lock().unwrap();
    let endpoint = pending.endpoint.upgrade();
    let mut endpoint_state = endpoint
        .as_ref()
        .map(|endpoint| endpoint.state.lock().unwrap());
    server_state.pending.remove(&pending.generation);
    let release_request = !pending.request_released.swap(true, Ordering::AcqRel);
    if release_request {
        server_state.request_bytes = server_state
            .request_bytes
            .saturating_sub(pending.request_bytes);
    }
    if !keep_generation {
        server_state.generations = server_state.generations.saturating_sub(1);
    }
    if let Some(endpoint_state) = endpoint_state.as_mut() {
        if release_request {
            endpoint_state.request_bytes = endpoint_state
                .request_bytes
                .saturating_sub(pending.request_bytes);
        }
        if !keep_generation
            && matches!(endpoint_state.slots.get(&pending.process_id), Some(EndpointSlot::Pending(current)) if current.generation == pending.generation)
        {
            endpoint_state.slots.remove(&pending.process_id);
        }
    }
}

fn complete_spawn_success(pending: &Arc<Pending>, process_handle: u64, merged: bool) {
    let Some(completion) = pending.completion.lock().unwrap().take() else {
        return;
    };
    let SpawnCompletion::Native(sender) = completion;
    let _ = sender.send(Ok(NativeStarted {
        process_id: pending.process_id,
        process_handle,
        stdin_window: PROCESS_DEFAULT_STREAM_WINDOW,
        stdout_window: PROCESS_DEFAULT_STREAM_WINDOW,
        stderr_window: if merged {
            0
        } else {
            PROCESS_DEFAULT_STREAM_WINDOW
        },
    }));
}

fn complete_spawn_failure(pending: &Arc<Pending>, error: NativeError) {
    if pending.phase.swap(PENDING_DONE, Ordering::AcqRel) == PENDING_DONE {
        return;
    }
    let endpoint_alive = pending
        .endpoint
        .upgrade()
        .is_some_and(|endpoint| endpoint.state.lock().unwrap().accepting);
    if !endpoint_alive {
        release_pending(pending, false);
        pending.completion.lock().unwrap().take();
        pending.mark_completed();
        return;
    }
    let completion = pending.completion.lock().unwrap().take();
    release_pending(pending, false);
    if let Some(SpawnCompletion::Native(sender)) = completion {
        let _ = sender.send(Err(error));
    }
    pending.mark_completed();
}

fn transfer_pending_to_record(pending: &Arc<Pending>, record: &Arc<Record>) -> bool {
    if pending.phase.swap(PENDING_DONE, Ordering::AcqRel) == PENDING_DONE {
        return false;
    }
    let Some(server) = pending.server.upgrade() else {
        pending.mark_completed();
        return false;
    };
    let mut server_state = server.state.lock().unwrap();
    let endpoint = pending.endpoint.upgrade();
    let mut endpoint_state = endpoint
        .as_ref()
        .map(|endpoint| endpoint.state.lock().unwrap());
    server_state.pending.remove(&pending.generation);
    let release_request = !pending.request_released.swap(true, Ordering::AcqRel);
    if release_request {
        server_state.request_bytes = server_state
            .request_bytes
            .saturating_sub(pending.request_bytes);
    }
    server_state
        .live
        .insert(pending.generation, Arc::downgrade(record));
    server_state.catalog_revision = server_state.catalog_revision.wrapping_add(1);
    server.catalog_changed.notify_waiters();
    let mut installed_bound = false;
    if let Some(endpoint_state) = endpoint_state.as_mut() {
        if release_request {
            endpoint_state.request_bytes = endpoint_state
                .request_bytes
                .saturating_sub(pending.request_bytes);
        }
        let owns_pending = matches!(endpoint_state.slots.get(&pending.process_id), Some(EndpointSlot::Pending(current)) if current.generation == pending.generation);
        if server_state.accepting
            && endpoint_state.accepting
            && owns_pending
            && !pending.endpoint_lost.load(Ordering::Acquire)
        {
            endpoint_state
                .slots
                .insert(pending.process_id, EndpointSlot::Bound(record.clone()));
            if !pending.detachable {
                endpoint_state
                    .owned
                    .insert(pending.generation, Arc::downgrade(record));
            }
            installed_bound = true;
        } else if owns_pending {
            endpoint_state.slots.remove(&pending.process_id);
        }
    }
    if !installed_bound {
        let mut inner = record.inner.lock().unwrap();
        inner.bindings.clear();
        inner.stdin_controller = None;
    }
    pending.mark_completed();
    installed_bound
}

#[cfg(unix)]
fn process_launch_cwd(req: &SpawnRequestOwned) -> Vec<u8> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let path = req
        .cwd
        .as_ref()
        .map(|cwd| PathBuf::from(OsString::from_vec(cwd.clone())))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    std::fs::canonicalize(&absolute)
        .unwrap_or(absolute)
        .as_os_str()
        .as_bytes()
        .to_vec()
}

#[cfg(windows)]
fn process_launch_cwd(req: &SpawnRequestOwned) -> Vec<u8> {
    let path = req
        .cwd
        .as_deref()
        .and_then(|cwd| std::str::from_utf8(cwd).ok())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    std::fs::canonicalize(&absolute)
        .unwrap_or(absolute)
        .to_string_lossy()
        .as_bytes()
        .to_vec()
}

#[cfg(unix)]
fn command_for(
    req: &SpawnRequestOwned,
    session_env: Option<&crate::app_env::SessionEnv>,
    clear_environment: bool,
) -> Command {
    let mut command = Command::new(OsStr::from_bytes(&req.argv[0]));
    command.args(req.argv[1..].iter().map(|arg| OsStr::from_bytes(arg)));
    if clear_environment {
        command.env_clear();
    }
    // The session environment goes on first so the client's own entries still
    // win, matching the documented "explicit entries replace inherited ones".
    if let Some(session) = session_env {
        for key in &session.remove {
            command.env_remove(key);
        }
        for (key, value) in &session.set {
            command.env(key, value);
        }
    }
    for (key, value) in &req.env {
        command.env(
            OsString::from_vec(key.clone()),
            OsString::from_vec(value.clone()),
        );
    }
    if let Some(cwd) = &req.cwd {
        command.current_dir(PathBuf::from(OsString::from_vec(cwd.clone())));
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.as_std_mut().process_group(0);
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let apple_fd_directory_available = apple_fd_directory_available();
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        not(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "solaris",
            target_os = "illumos",
        )),
    ))]
    let inherited_fd_limit = inherited_fd_limit();
    // SAFETY: this runs after fork in the child and only invokes libc calls.
    unsafe {
        command.as_std_mut().pre_exec(move || {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            // Enumerate the forked child's actual descriptor table rather than
            // taking a racy parent-side snapshot. `FD_CLOEXEC` leaves Rust's
            // private exec-error pipe usable until exec while preventing every
            // descriptor at or above 3 from reaching the requested program.
            #[cfg(any(
                target_os = "linux",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "solaris",
                target_os = "illumos",
            ))]
            close_fds::set_fds_cloexec(3, &[]);
            // close_fds enumerates /dev/fd on Apple. Check it in the parent so
            // unusual chroots retain a complete, if slower, numeric fallback.
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if apple_fd_directory_available {
                close_fds::set_fds_cloexec(3, &[]);
            } else {
                mark_fd_range_cloexec(inherited_fd_limit);
            }
            // Keep generic Unix builds correct even when close_fds has no
            // native descriptor-table iterator for the target. This path is
            // intentionally slower; supported server platforms use the fast
            // directory or close-range implementations above.
            #[cfg(not(any(
                target_os = "linux",
                target_os = "macos",
                target_os = "ios",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "solaris",
                target_os = "illumos",
            )))]
            mark_fd_range_cloexec(inherited_fd_limit);
            Ok(())
        });
    }
    command
}

#[cfg(windows)]
fn command_for(
    req: &SpawnRequestOwned,
    session_env: Option<&crate::app_env::SessionEnv>,
    clear_environment: bool,
) -> Command {
    let argv = req
        .argv
        .iter()
        .map(|value| std::str::from_utf8(value).expect("Windows spawn validated UTF-8"))
        .collect::<Vec<_>>();
    let mut command = Command::new(argv[0]);
    command.args(&argv[1..]);
    if clear_environment {
        command.env_clear();
    }
    // There is no Wayland session to join on Windows, so the resolver hands back
    // nothing; the parameter exists to keep one signature across platforms.
    if let Some(session) = session_env {
        for key in &session.remove {
            command.env_remove(key);
        }
        for (key, value) in &session.set {
            command.env(key, value);
        }
    }
    for (key, value) in &req.env {
        command.env(
            std::str::from_utf8(key).expect("Windows env key validated UTF-8"),
            std::str::from_utf8(value).expect("Windows env value validated UTF-8"),
        );
    }
    if let Some(cwd) = req.cwd.as_deref() {
        command.current_dir(std::str::from_utf8(cwd).expect("Windows cwd validated UTF-8"));
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Suspension closes the otherwise unavoidable race between CreateProcess
    // and assigning the child to its kill-on-close job.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
    command
}

#[cfg(all(
    unix,
    any(
        target_os = "macos",
        target_os = "ios",
        not(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "solaris",
            target_os = "illumos",
        )),
    )
))]
fn inherited_fd_limit() -> libc::c_int {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    let hard = unsafe {
        if libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) == 0 {
            Some(limit.assume_init().rlim_max)
        } else {
            None
        }
    };
    if let Some(hard) = hard.filter(|value| *value != libc::RLIM_INFINITY) {
        return hard.min(i32::MAX as libc::rlim_t) as libc::c_int;
    }
    let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
    if open_max > 0 {
        open_max.min(i32::MAX as libc::c_long) as libc::c_int
    } else {
        65_536
    }
}

#[cfg(all(
    unix,
    any(
        target_os = "macos",
        target_os = "ios",
        not(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "solaris",
            target_os = "illumos",
        )),
    )
))]
fn mark_fd_range_cloexec(limit: libc::c_int) {
    for fd in 3..limit {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
            unsafe {
                libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn apple_fd_directory_available() -> bool {
    let directory = unsafe {
        libc::open(
            c"/dev/fd".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if directory < 0 {
        return false;
    }
    unsafe {
        libc::close(directory);
    }
    true
}

fn configure_merged_output(command: &mut Command) -> io::Result<tokio::fs::File> {
    let (reader, writer) = os_pipe::pipe()?;
    let stderr = writer.try_clone()?;
    command.stdout(Stdio::from(writer));
    command.stderr(Stdio::from(stderr));
    #[cfg(unix)]
    let reader = std::fs::File::from(std::os::fd::OwnedFd::from(reader));
    #[cfg(windows)]
    let reader = std::fs::File::from(std::os::windows::io::OwnedHandle::from(reader));
    Ok(tokio::fs::File::from_std(reader))
}

#[cfg(unix)]
fn spawn_child(command: &mut Command) -> io::Result<Spawned> {
    pty::spawn_registered_child(|| {
        let child = command.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("spawn returned no child pid"))?;
        let registered_pid = libc::pid_t::try_from(pid)
            .map_err(|_| io::Error::other("child pid exceeds native pid_t"))?;
        Ok((registered_pid, Spawned { child, pid }))
    })
}

#[cfg(windows)]
fn spawn_child(command: &mut Command) -> io::Result<Spawned> {
    let job = create_kill_on_close_job()?;
    let mut child = command.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("spawn returned no child pid"))?;
    let process = child
        .raw_handle()
        .ok_or_else(|| io::Error::other("spawn returned no process handle"))?
        as HANDLE;
    if unsafe { AssignProcessToJobObject(job.0, process) } == 0 {
        let error = io::Error::last_os_error();
        let _ = child.start_kill();
        return Err(error);
    }
    if let Err(error) = resume_primary_thread(pid) {
        let _ = child.start_kill();
        return Err(error);
    }
    Ok(Spawned { child, pid, job })
}

#[cfg(windows)]
fn resume_primary_thread(pid: u32) -> io::Result<()> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut found = Thread32First(snapshot, &mut entry) != 0;
        while found {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread.is_null() {
                    let error = io::Error::last_os_error();
                    CloseHandle(snapshot);
                    return Err(error);
                }
                let resumed = ResumeThread(thread);
                CloseHandle(thread);
                CloseHandle(snapshot);
                return if resumed == u32::MAX {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                };
            }
            found = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        Err(io::Error::other("spawned process has no primary thread"))
    }
}

#[cfg(windows)]
fn create_kill_on_close_job() -> io::Result<JobHandle> {
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let handle = JobHandle(job);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            handle.0,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(handle)
    }
}

#[cfg(windows)]
fn windows_env_keys_equal(left: &str, right: &str) -> bool {
    let left = left.encode_utf16().collect::<Vec<_>>();
    let right = right.encode_utf16().collect::<Vec<_>>();
    unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        ) == CSTR_EQUAL
    }
}

fn send_stdin_ack(inner: &RecordInner, bytes: u64, stdin_state: u8) {
    for binding in &inner.bindings {
        binding
            .out
            .send_stdin_progress(binding.process_id, bytes, stdin_state);
    }
}

fn send_stdin_ack_to(inner: &RecordInner, endpoint_id: u64, process_id: u32) {
    let Some(index) = binding_index(inner, endpoint_id, process_id) else {
        return;
    };
    let binding = &inner.bindings[index];
    binding
        .out
        .send_stdin_progress(process_id, inner.stdin_acked, inner.stdin_state);
}

async fn stdin_writer(
    record: Arc<Record>,
    mut stdin: tokio::process::ChildStdin,
    mut input: mpsc::Receiver<InputChunk>,
) {
    while let Some(chunk) = input.recv().await {
        if stdin.write_all(&chunk.data).await.is_err() {
            {
                let mut inner = record.inner.lock().unwrap();
                let changed = inner.stdin_state != PROCESS_STDIN_CLOSED;
                inner.stdin_state = PROCESS_STDIN_CLOSED;
                inner.stdin_closed_by_child = true;
                inner.stdin_writer_done = true;
                inner.stdin_tx.take();
                if changed {
                    send_stdin_ack(&inner, inner.stdin_acked, PROCESS_STDIN_CLOSED);
                }
            }
            record.changed.notify_waiters();
            try_queue_terminal(&record);
            return;
        }
        {
            let mut inner = record.inner.lock().unwrap();
            inner.stdin_acked = chunk.end;
            while inner
                .stdin_frames
                .front()
                .is_some_and(|end| *end <= chunk.end)
            {
                inner.stdin_frames.pop_front();
            }
            send_stdin_ack(&inner, inner.stdin_acked, inner.stdin_state);
        }
    }
    drop(stdin);
    {
        let mut inner = record.inner.lock().unwrap();
        let changed = inner.stdin_state != PROCESS_STDIN_CLOSED;
        inner.stdin_state = PROCESS_STDIN_CLOSED;
        inner.stdin_writer_done = true;
        if changed {
            send_stdin_ack(&inner, inner.stdin_acked, PROCESS_STDIN_CLOSED);
        }
    }
    record.changed.notify_waiters();
    try_queue_terminal(&record);
}

async fn output_reader(record: Arc<Record>, stream: u8, mut reader: impl AsyncRead + Unpin) {
    let mut buffer = vec![0u8; OUTPUT_FRAME_PAYLOAD];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Err(_) => {
                host_failure(&record, "process output pipe read failed");
                break;
            }
            Ok(n) => {
                let mut inner = record.inner.lock().unwrap();
                let state = if stream == PROCESS_STREAM_STDOUT {
                    &mut inner.stdout
                } else {
                    inner.stderr.as_mut().expect("separate stderr")
                };
                let offset = state.next;
                let Some(next) = offset.checked_add(n as u64) else {
                    drop(inner);
                    protocol_violation(&record);
                    return;
                };
                state.next = next;
                let mut evicted = Vec::new();
                let mut index = 0;
                while index < inner.bindings.len() {
                    let has_credit = {
                        let binding = &inner.bindings[index];
                        let credit = if stream == PROCESS_STREAM_STDOUT {
                            &binding.stdout
                        } else {
                            binding.stderr.as_ref().expect("separate stderr binding")
                        };
                        let available = offset
                            .checked_sub(credit.acked)
                            .and_then(|debt| PROCESS_DEFAULT_STREAM_WINDOW.checked_sub(debt));
                        available.is_some_and(|bytes| bytes >= n as u64)
                            && credit.frames.len() < PROCESS_MAX_UNACKED_PACKETS
                    };
                    if !has_credit {
                        evicted.push(remove_binding_at(&mut inner, index));
                        continue;
                    }
                    let process_id = inner.bindings[index].process_id;
                    let sent = inner.bindings[index].out.send_output(
                        process_id,
                        stream,
                        offset,
                        &buffer[..n],
                    );
                    if sent {
                        let binding = &mut inner.bindings[index];
                        let credit = if stream == PROCESS_STREAM_STDOUT {
                            &mut binding.stdout
                        } else {
                            binding.stderr.as_mut().expect("separate stderr binding")
                        };
                        credit.frames.push_back(next);
                        index += 1;
                    } else {
                        evicted.push(remove_binding_at(&mut inner, index));
                    }
                }
                drop(inner);
                for binding in evicted {
                    binding
                        .out
                        .kick("native process watcher exceeded its output window");
                }
            }
        }
    }
    stream_closed(&record, stream);
}

fn stream_closed(record: &Arc<Record>, stream: u8) {
    {
        let mut inner = record.inner.lock().unwrap();
        if stream == PROCESS_STREAM_STDOUT {
            inner.stdout_readers = inner.stdout_readers.saturating_sub(1);
        } else {
            inner.stderr_readers = inner.stderr_readers.saturating_sub(1);
        }
    }
    record.changed.notify_waiters();
    try_queue_terminal(record);
}

async fn wait_child(record: Arc<Record>, mut child: Child) {
    let result = child.wait().await;
    #[cfg(unix)]
    let outcome = match result {
        Ok(status) => {
            pty::deregister_child_pid(record.pid as libc::pid_t);
            if let Some(code) = status.code() {
                ChildOutcome::Returned(code as u32)
            } else if let Some(signal) = status.signal() {
                ChildOutcome::Signalled(signal as u32)
            } else {
                ChildOutcome::HostFailure
            }
        }
        Err(_) => match pty::take_reaped_child_status(record.pid as libc::pid_t) {
            Some(status) if status >= 0 => ChildOutcome::Returned(status as u32),
            Some(status) => ChildOutcome::Signalled(status.unsigned_abs()),
            None => ChildOutcome::HostFailure,
        },
    };
    #[cfg(windows)]
    let outcome = match result {
        Ok(status) => status
            .code()
            .map(|code| ChildOutcome::Returned(code as u32))
            .unwrap_or(ChildOutcome::HostFailure),
        Err(_) => ChildOutcome::HostFailure,
    };
    {
        let mut inner = record.inner.lock().unwrap();
        inner.child_outcome = Some(outcome);
        inner.terminate_timeout_armed = false;
        if inner.stdin_state == PROCESS_STDIN_ACCEPTING {
            inner.stdin_state = PROCESS_STDIN_CLOSING;
            send_stdin_ack(&inner, inner.stdin_acked, PROCESS_STDIN_CLOSING);
        }
        inner.stdin_tx.take();
    }
    record.mark_reaped();
    #[cfg(unix)]
    if !record.preserve_residual {
        let _ = graceful_terminate(&record);
    }
    schedule_residual_cleanup(record.clone());
    try_queue_terminal(&record);
}

fn schedule_residual_cleanup(record: Arc<Record>) {
    tokio::spawn(async move {
        #[cfg(unix)]
        if record.preserve_residual {
            let mut poll = tokio::time::interval(Duration::from_millis(50));
            poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                let changed = record.changed.notified();
                let io_done = io_tasks_done(&record.inner.lock().unwrap());
                let group_absent = process_group_absent(&record);
                if io_done && group_absent {
                    break;
                }
                tokio::select! {
                    _ = changed => {}
                    _ = poll.tick() => {}
                }
            }
            {
                let mut inner = record.inner.lock().unwrap();
                if inner.tree_cleanup_done {
                    return;
                }
                inner.tree_cleanup_done = true;
                inner.terminate_timeout_armed = false;
            }
            record.changed.notify_waiters();
            try_queue_terminal(&record);
            return;
        }

        let deadline = tokio::time::sleep(record.server.0.policy.kill_grace);
        tokio::pin!(deadline);
        loop {
            let changed = record.changed.notified();
            if io_tasks_done(&record.inner.lock().unwrap()) {
                break;
            }
            tokio::select! {
                _ = changed => continue,
                _ = &mut deadline => break,
            }
        }
        // The direct child is already reaped, so this targets only residual
        // group/job members. Running it as soon as their inherited pipes close
        // also avoids a Unix process-group-ID reuse window.
        let cleanup_failed = force_kill(&record)
            .err()
            .is_some_and(|error| !process_tree_already_absent(&error));
        let (stdin_abort, output_aborts) = {
            let mut inner = record.inner.lock().unwrap();
            if inner.tree_cleanup_done {
                return;
            }
            inner.tree_cleanup_done = true;
            if cleanup_failed {
                inner.exit_override = Some(ExitOverride {
                    reason: PROCESS_EXIT_HOST_FAILURE,
                    kill_cause: 0,
                });
                inner.cleanup_detail = "residual process tree force-kill failed";
            }
            if io_tasks_done(&inner) {
                (None, Vec::new())
            } else {
                if !cleanup_failed {
                    inner.cleanup_detail = "residual process tree required forceful cleanup";
                }
                let stdin_changed = inner.stdin_state != PROCESS_STDIN_CLOSED;
                inner.stdin_tx.take();
                inner.stdin_state = PROCESS_STDIN_CLOSED;
                inner.stdin_writer_done = true;
                inner.stdout_readers = 0;
                inner.stderr_readers = 0;
                if stdin_changed {
                    send_stdin_ack(&inner, inner.stdin_acked, PROCESS_STDIN_CLOSED);
                }
                (
                    inner.stdin_abort.take(),
                    std::mem::take(&mut inner.output_aborts),
                )
            }
        };
        if let Some(abort) = stdin_abort {
            abort.abort();
        }
        for abort in output_aborts {
            abort.abort();
        }
        record.changed.notify_waiters();
        try_queue_terminal(&record);
    });
}

fn io_tasks_done(inner: &RecordInner) -> bool {
    inner.stdin_writer_done && inner.stdout_readers == 0 && inner.stderr_readers == 0
}

fn schedule_terminate_timeout(record: Arc<Record>, cause: u8) {
    {
        let mut inner = record.inner.lock().unwrap();
        let residual_running =
            record.preserve_residual && inner.child_outcome.is_some() && !inner.tree_cleanup_done;
        if (inner.child_outcome.is_some() && !residual_running)
            || inner.terminal_queued
            || inner.terminate_timeout_armed
        {
            return;
        }
        inner.terminate_timeout_armed = true;
    }
    #[cfg(test)]
    record
        .server
        .0
        .terminate_timeout_tasks
        .fetch_add(1, Ordering::AcqRel);
    tokio::spawn(async move {
        let finished = tokio::select! {
            _ = async {
                if record.preserve_residual {
                    record.wait_tree_cleanup().await;
                } else {
                    record.wait_reaped().await;
                }
            } => true,
            _ = tokio::time::sleep(record.server.0.policy.kill_grace) => false,
        };
        if !finished {
            let mut inner = record.inner.lock().unwrap();
            let process_tree_running = inner.child_outcome.is_none()
                || (record.preserve_residual && !inner.tree_cleanup_done);
            if process_tree_running && !inner.terminal_queued {
                if force_kill(&record).is_ok() {
                    inner.exit_override = Some(ExitOverride {
                        reason: PROCESS_EXIT_KILLED,
                        kill_cause: cause,
                    });
                } else {
                    // A failed escalation may be retried by a later explicit
                    // TERMINATE rather than permanently suppressing its
                    // deadline task.
                    inner.terminate_timeout_armed = false;
                }
            } else {
                inner.terminate_timeout_armed = false;
            }
        }
        #[cfg(test)]
        record
            .server
            .0
            .terminate_timeout_tasks
            .fetch_sub(1, Ordering::AcqRel);
    });
}

fn outcome_fields(outcome: ChildOutcome, override_: Option<ExitOverride>) -> (u8, u8, u32) {
    if matches!(outcome, ChildOutcome::HostFailure) {
        return (PROCESS_EXIT_HOST_FAILURE, 0, 0);
    }
    if let Some(override_) = override_ {
        return (override_.reason, override_.kill_cause, 0);
    }
    match outcome {
        ChildOutcome::Returned(code) => (PROCESS_EXIT_RETURNED, 0, code),
        #[cfg(unix)]
        ChildOutcome::Signalled(signal) => (PROCESS_EXIT_SIGNALLED, 0, signal),
        ChildOutcome::HostFailure => (PROCESS_EXIT_HOST_FAILURE, 0, 0),
    }
}

fn stream_state(inner: &RecordInner, merged: bool) -> u8 {
    let mut state = match inner.stdin_state {
        PROCESS_STDIN_ACCEPTING => PROCESS_STREAM_STDIN_ACCEPTING,
        PROCESS_STDIN_CLOSING => PROCESS_STREAM_STDIN_CLOSING,
        _ => PROCESS_STREAM_STDIN_CLOSED,
    };
    if inner.stdout_readers > 0 {
        state |= PROCESS_STREAM_STDOUT_OPEN;
    }
    if inner.stderr_readers > 0 {
        state |= PROCESS_STREAM_STDERR_OPEN;
    }
    if merged {
        state |= PROCESS_STREAM_MERGED_STDERR;
    }
    state
}

fn try_queue_terminal(record: &Arc<Record>) {
    let terminal = {
        let mut inner = record.inner.lock().unwrap();
        let Some(outcome) = inner.child_outcome else {
            return;
        };
        if !inner.tree_cleanup_done
            || !inner.stdin_writer_done
            || inner.stdout_readers != 0
            || inner.stderr_readers != 0
            || inner.terminal_queued
        {
            return;
        }
        inner.terminal_queued = true;
        let (reason, kill_cause, code) = outcome_fields(outcome, inner.exit_override);
        let final_record = Arc::new(FinalRecord {
            generation: record.generation,
            pid: record.pid,
            flags: record_flags(record),
            owner_session: record.owner_session,
            argv0: record.argv0.clone(),
            cwd: record.cwd.clone(),
            buffer_bytes: record.argv0.len(),
            stdin_received: inner.stdin_received,
            stdin_acked: inner.stdin_acked,
            stdout_next: inner.stdout.next,
            stderr_next: inner.stderr.as_ref().map_or(0, |stream| stream.next),
            stream_state: stream_state(&inner, record.merged),
            reason,
            kill_cause,
            code,
            detail: inner.cleanup_detail,
        });
        inner.stdin_controller = None;
        (std::mem::take(&mut inner.bindings), final_record)
    };
    record.terminal_notify.notify_waiters();
    let (bindings, final_record) = terminal;
    if bindings.is_empty() {
        finish_terminal(record.clone(), final_record);
        return;
    }
    let remaining = Arc::new(AtomicUsize::new(bindings.len()));
    for binding in bindings {
        let endpoint = binding.endpoint.upgrade();
        let record_for_guard = record.clone();
        let final_for_guard = final_record.clone();
        let remaining_for_guard = remaining.clone();
        let process_id = binding.process_id;
        let guard = WriterGuard::new(move || {
            if let Some(endpoint) = endpoint {
                remove_bound_slot(&endpoint, process_id, &record_for_guard);
            }
            if remaining_for_guard.fetch_sub(1, Ordering::AcqRel) == 1 {
                finish_terminal(record_for_guard, final_for_guard);
            }
        });
        let _ = binding.out.send_exit(
            binding.process_id,
            native_exit(
                final_record.reason,
                final_record.kill_cause,
                final_record.code,
                final_record.detail.as_bytes(),
            ),
            guard,
        );
    }
}

fn native_exit(reason: u8, kill_cause: u8, code: u32, detail: &[u8]) -> NativeExit {
    match reason {
        PROCESS_EXIT_RETURNED => NativeExit {
            kind: wire::ExitKind::Code,
            reason: process_schema::EXIT_REASON_UNKNOWN as u8,
            code: i32::try_from(code).unwrap_or(i32::MAX),
            detail: detail.to_vec(),
        },
        PROCESS_EXIT_SIGNALLED => NativeExit {
            kind: wire::ExitKind::Signal,
            reason: portable_signal_reason(code),
            code: i32::try_from(code).unwrap_or(i32::MAX),
            detail: detail.to_vec(),
        },
        PROCESS_EXIT_KILLED => NativeExit {
            kind: wire::ExitKind::Killed,
            reason: match kill_cause {
                PROCESS_KILL_CLIENT
                | PROCESS_KILL_OWNER_LOST
                | PROCESS_KILL_TERMINATE_TIMEOUT
                | PROCESS_KILL_SERVER_SHUTDOWN => kill_cause,
                _ => process_schema::EXIT_REASON_UNKNOWN as u8,
            },
            code: 0,
            detail: detail.to_vec(),
        },
        _ => NativeExit {
            kind: wire::ExitKind::Other,
            reason: process_schema::EXIT_REASON_UNKNOWN as u8,
            code: 0,
            detail: if detail.is_empty() {
                b"process host failure".to_vec()
            } else {
                detail.to_vec()
            },
        },
    }
}

#[cfg(unix)]
fn portable_signal_reason(signal: u32) -> u8 {
    match signal as i32 {
        libc::SIGINT => process_schema::EXIT_REASON_INTERRUPT as u8,
        libc::SIGTERM => process_schema::EXIT_REASON_TERMINATE as u8,
        libc::SIGKILL => process_schema::EXIT_REASON_KILL as u8,
        libc::SIGHUP => process_schema::EXIT_REASON_HANGUP as u8,
        _ => process_schema::EXIT_REASON_TERMINATE as u8,
    }
}

#[cfg(windows)]
fn portable_signal_reason(signal: u32) -> u8 {
    match signal {
        2 => process_schema::EXIT_REASON_INTERRUPT as u8,
        9 => process_schema::EXIT_REASON_KILL as u8,
        1 => process_schema::EXIT_REASON_HANGUP as u8,
        _ => process_schema::EXIT_REASON_TERMINATE as u8,
    }
}

fn finish_terminal(record: Arc<Record>, final_record: Arc<FinalRecord>) {
    if record.detachable {
        let server = record.server.clone();
        server.finish_detached(record, final_record);
    } else {
        record.server.release_record(&record);
    }
}

fn protocol_violation(record: &Arc<Record>) {
    {
        let mut inner = record.inner.lock().unwrap();
        if inner.exit_override.is_none() {
            inner.exit_override = Some(ExitOverride {
                reason: PROCESS_EXIT_PROTOCOL_VIOLATION,
                kill_cause: 0,
            });
        }
        inner.stdin_tx.take();
    }
    let _ = force_kill(record);
}

fn host_failure(record: &Arc<Record>, detail: &'static str) {
    {
        let mut inner = record.inner.lock().unwrap();
        inner.exit_override = Some(ExitOverride {
            reason: PROCESS_EXIT_HOST_FAILURE,
            kill_cause: 0,
        });
        inner.cleanup_detail = detail;
        inner.stdin_tx.take();
    }
    let _ = force_kill(record);
}

async fn terminate_record(record: &Arc<Record>, cause: u8, grace: Duration) {
    {
        let mut inner = record.inner.lock().unwrap();
        inner.stdin_tx.take();
        let process_tree_running =
            inner.child_outcome.is_none() || (record.preserve_residual && !inner.tree_cleanup_done);
        if process_tree_running
            && inner.exit_override.is_none()
            && cleanup_terminate(record).is_ok()
        {
            inner.exit_override = Some(ExitOverride {
                reason: PROCESS_EXIT_KILLED,
                kill_cause: cause,
            });
        }
    }
    record.changed.notify_waiters();
    let graceful = async {
        if record.preserve_residual {
            record.wait_tree_cleanup().await;
        } else {
            record.wait_reaped().await;
        }
    };
    if tokio::time::timeout(grace, graceful).await.is_err() {
        {
            let mut inner = record.inner.lock().unwrap();
            let process_tree_running = inner.child_outcome.is_none()
                || (record.preserve_residual && !inner.tree_cleanup_done);
            if process_tree_running && force_kill(record).is_ok() {
                inner.exit_override = Some(ExitOverride {
                    reason: PROCESS_EXIT_KILLED,
                    kill_cause: cause,
                });
            }
        }
        let forced = async {
            if record.preserve_residual {
                record.wait_tree_cleanup().await;
            } else {
                record.wait_reaped().await;
            }
        };
        let _ = tokio::time::timeout(grace.max(Duration::from_millis(100)), forced).await;
    }
    abort_pipes(record);
    let _ = tokio::time::timeout(
        grace.max(Duration::from_millis(100)),
        record.wait_tree_cleanup(),
    )
    .await;
}

async fn wait_and_force(records: &[Arc<Record>], cause: u8, grace: Duration) {
    let graceful = async {
        for record in records {
            record.wait_reaped().await;
        }
    };
    if tokio::time::timeout(grace, graceful).await.is_err() {
        for record in records {
            if !record.reaped.load(Ordering::Acquire) {
                let mut inner = record.inner.lock().unwrap();
                if inner.child_outcome.is_none() && force_kill(record).is_ok() {
                    inner.exit_override = Some(ExitOverride {
                        reason: PROCESS_EXIT_KILLED,
                        kill_cause: cause,
                    });
                }
            }
        }
        let forced = async {
            for record in records {
                record.wait_reaped().await;
            }
        };
        let _ = tokio::time::timeout(grace.max(Duration::from_millis(100)), forced).await;
    }
}

fn abort_pipes(record: &Arc<Record>) {
    let (stdin_abort, output_aborts) = {
        let mut inner = record.inner.lock().unwrap();
        inner.stdin_tx.take();
        inner.stdin_state = PROCESS_STDIN_CLOSED;
        inner.stdin_writer_done = true;
        inner.stdout_readers = 0;
        inner.stderr_readers = 0;
        (
            inner.stdin_abort.take(),
            std::mem::take(&mut inner.output_aborts),
        )
    };
    if let Some(abort) = stdin_abort {
        abort.abort();
    }
    for abort in output_aborts {
        abort.abort();
    }
    record.changed.notify_waiters();
    try_queue_terminal(record);
}

#[cfg(unix)]
fn signal_group(pid: ProcessId, signal: libc::c_int) -> io::Result<()> {
    let pid = libc::pid_t::try_from(pid).map_err(|_| io::Error::other("invalid process id"))?;
    if unsafe { libc::kill(-pid, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn graceful_terminate(record: &Record) -> io::Result<()> {
    signal_group(record.pid, libc::SIGTERM)
}

#[cfg(windows)]
fn graceful_terminate(record: &Record) -> io::Result<()> {
    if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, record.pid) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn force_kill(record: &Record) -> io::Result<()> {
    signal_group(record.pid, libc::SIGKILL)
}

#[cfg(windows)]
fn force_kill(record: &Record) -> io::Result<()> {
    if unsafe { TerminateJobObject(record.job.0, 1) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn process_tree_already_absent(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ESRCH)
}

#[cfg(unix)]
fn process_group_absent(record: &Record) -> bool {
    signal_group(record.pid, 0)
        .err()
        .is_some_and(|error| process_tree_already_absent(&error))
}

#[cfg(windows)]
fn process_tree_already_absent(_error: &io::Error) -> bool {
    false
}

#[cfg(unix)]
fn cleanup_terminate(record: &Record) -> io::Result<()> {
    graceful_terminate(record)
}

#[cfg(windows)]
fn cleanup_terminate(record: &Record) -> io::Result<()> {
    force_kill(record)
}

#[cfg(unix)]
fn control_signal(record: &Record, value: u32) -> Result<(), NativeError> {
    let signal = i32::try_from(value).ok().filter(|signal| *signal > 0);
    match signal {
        Some(signal) => signal_group(record.pid, signal).map_err(|error| {
            if error.raw_os_error() == Some(libc::EINVAL) {
                NativeError::Invalid("invalid signal".to_owned())
            } else {
                NativeError::Io(os_error_detail(error).to_owned())
            }
        }),
        None => Err(NativeError::Invalid("invalid signal".to_owned())),
    }
}

#[cfg(windows)]
fn control_signal(record: &Record, value: u32) -> Result<(), NativeError> {
    if value != CTRL_BREAK_EVENT {
        return Err(NativeError::Invalid(
            "signal is unsupported on Windows".to_owned(),
        ));
    }
    graceful_terminate(record)
        .map_err(|_| NativeError::Io("console control is unavailable".to_owned()))
}

#[cfg(unix)]
fn os_error_detail(error: io::Error) -> &'static str {
    match error.raw_os_error() {
        Some(libc::ESRCH) => "process already exited",
        Some(libc::EPERM) => "permission denied signaling process group",
        Some(libc::EINVAL) => "invalid signal",
        _ => "process control failed",
    }
}

#[cfg(windows)]
fn os_error_detail(_error: io::Error) -> &'static str {
    "process control failed"
}
