//! Server-side extension service and supervisor.

mod command_directory;
pub(crate) mod quickjs_host;
pub(crate) mod wasmi_host;

use self::command_directory::{CommandDirectory, CommandListener, CommandOwner, DiscoveryStatus};
use self::wasmi_host::{
    AttemptCancellation, AttemptFailure, AttemptOutcome, AttemptSpec as WasmiAttemptSpec,
    FailureKind, WasmiHostConfig,
};
use crate::extension_catalog::{
    BlockedState, CatalogError, ExtensionCatalog, PersistentDefinition, PersistentMutationReplay,
    mutation_replay_capacity,
};
use crate::extension_store::{
    BeginUpload, ChunkUploadCommit, ObjectHash, ObjectRead, ObjectStore, ObjectStoreConfig,
    ObjectStoreError, PreparedBeginUpload, PreparedPut, PutChunk, UploadCreationCommit,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch};

const EXT_FLAG_PERSIST: u8 = yas_wire::schema::extension::DEFINITION_PERSISTENT as u8;
const EXT_FLAG_ENABLED: u8 = yas_wire::schema::extension::DEFINITION_ENABLED as u8;
const EXT_FLAG_DESIRED_RUNNING: u8 = yas_wire::schema::extension::DEFINITION_DESIRED_RUNNING as u8;
const EXT_FLAG_DETACH: u8 = yas_wire::schema::extension::DEFINITION_DETACHED as u8;
const EXT_FLAGS: u8 = yas_wire::schema::extension::DEFINITION_FLAGS as u8;

const EXT_RESTART_ON_FAILURE: u8 = yas_wire::schema::extension::RESTART_ON_FAILURE as u8;
const EXT_RESTART_ALWAYS: u8 = yas_wire::schema::extension::RESTART_ALWAYS as u8;

const EXT_PHASE_NEED_OBJECT: u8 = yas_wire::schema::extension::PHASE_NEED_OBJECT as u8;
const EXT_PHASE_VALIDATING: u8 = yas_wire::schema::extension::PHASE_VALIDATING as u8;
const EXT_PHASE_QUEUED: u8 = yas_wire::schema::extension::PHASE_QUEUED as u8;
const EXT_PHASE_RUNNING: u8 = yas_wire::schema::extension::PHASE_RUNNING as u8;
const EXT_PHASE_BACKOFF: u8 = yas_wire::schema::extension::PHASE_BACKOFF as u8;
const EXT_PHASE_STOPPED: u8 = yas_wire::schema::extension::PHASE_STOPPED as u8;
const EXT_PHASE_BLOCKED: u8 = yas_wire::schema::extension::PHASE_BLOCKED as u8;
const EXT_PHASE_STOPPING: u8 = yas_wire::schema::extension::PHASE_STOPPING as u8;

const EXT_EXIT_RETURNED: u8 = yas_wire::schema::extension::EXIT_RETURNED as u8;
const EXT_EXIT_TRAPPED: u8 = yas_wire::schema::extension::EXIT_TRAPPED as u8;
const EXT_EXIT_CANCELLED: u8 = yas_wire::schema::extension::EXIT_CANCELLED as u8;
const EXT_EXIT_UPDATED: u8 = yas_wire::schema::extension::EXIT_UPDATED as u8;
const EXT_EXIT_SLOW_CONSUMER: u8 = yas_wire::schema::extension::EXIT_SLOW_CONSUMER as u8;
const EXT_EXIT_PROTOCOL_VIOLATION: u8 = yas_wire::schema::extension::EXIT_PROTOCOL_VIOLATION as u8;
const EXT_EXIT_HOST_FAILURE: u8 = yas_wire::schema::extension::EXIT_HOST_FAILURE as u8;
const EXT_EXIT_SERVER_SHUTDOWN: u8 = yas_wire::schema::extension::EXIT_SERVER_SHUTDOWN as u8;
const EXT_EXIT_RESOURCE_LIMIT: u8 = yas_wire::schema::extension::EXIT_RESOURCE_LIMIT as u8;

// Private backend operation/status values. They are never serialized.
const EXT_RUN_DETACH: u8 = 1 << 0;
const EXT_RUN_PERSIST: u8 = 1 << 1;
const EXT_RUN_UPDATE: u8 = 1 << 2;
const EXT_PUT_BEGIN: u8 = 1 << 0;
const EXT_PUT_FINAL: u8 = 1 << 1;
const EXT_CONTROL_CANCEL: u8 = 1;
const EXT_CONTROL_ATTACH: u8 = 2;
const EXT_CONTROL_UNFOLLOW: u8 = 3;
const EXT_CONTROL_STATUS: u8 = 4;
const EXT_CONTROL_RESTART: u8 = 5;
const EXT_CONTROL_ENABLE: u8 = 6;
const EXT_CONTROL_DISABLE: u8 = 7;
const EXT_CONTROL_REMOVE: u8 = 8;
const EXT_CONTROL_LIST: u8 = 9;
const EXT_STATUS_OK: u8 = 0;
const EXT_STATUS_UNKNOWN_ID: u8 = 1;
const EXT_STATUS_NOT_FOUND: u8 = 2;
const EXT_STATUS_PERMISSION: u8 = 3;
const EXT_STATUS_TOO_LARGE: u8 = 4;
const EXT_STATUS_BUDGET: u8 = 5;
const EXT_STATUS_INVALID: u8 = 6;
const EXT_STATUS_CANCELLED: u8 = 7;
const EXT_STATUS_OTHER: u8 = 8;
const EXT_STATUS_CONFLICT: u8 = 9;
const EXT_PUT_ALREADY_HAVE: u8 = 128;
const EXT_MAX_DETAIL: usize = 4 * 1024;
const EXT_MAX_MODULE: u64 = yas_wire::extension::MAX_OBJECT_BYTES;

const DEFAULT_MAX_TRANSIENT: usize = 128;
const DEFAULT_MAX_PERSISTENT: usize = 128;
const DEFAULT_FOLLOW_MAX_PER_ENDPOINT: usize = 128;
const DEFAULT_FOLLOW_MAX: usize = 4_096;
const DEFAULT_MAX_RUNNING: usize = 4;
const DEFAULT_MAX_VALIDATING: usize = 2;
const DEFAULT_ARGUMENT_STORE_MAX: usize = 256 * 1024 * 1024;
const DEFAULT_OUTPUT_RETAIN_MAX: usize = 64 * 1024 * 1024;
const OUTPUT_RETAIN_PER_EXTENSION: usize = 4 * 1024 * 1024;
const NATIVE_ENDPOINT_QUEUE: usize = 128;
const DEFAULT_PENDING_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const DEFAULT_TERMINAL_RETAIN: Duration = Duration::from_secs(30);
const BACKOFF_BASE: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const WASM_MAGIC: &[u8; 4] = b"\0asm";

fn is_wasm_module(object: &[u8]) -> bool {
    object.starts_with(WASM_MAGIC)
}

fn validate_extension_object(
    object: &[u8],
    config: &WasmiHostConfig,
) -> Result<(), AttemptFailure> {
    if is_wasm_module(object) {
        wasmi_host::validate_module(object, config)
    } else {
        quickjs_host::validate_source(object, config)
    }
}

/// Owned process-global definition state consumed by the native YAS adapter.
/// This deliberately contains transport-independent semantic values.
#[derive(Clone, Debug)]
pub(crate) struct NativeDefinition {
    pub(crate) extension_handle: u64,
    pub(crate) generation: u64,
    pub(crate) definition_revision: u64,
    pub(crate) phase: u8,
    pub(crate) runtime: u8,
    pub(crate) restart: u8,
    pub(crate) flags: u8,
    pub(crate) attempt: u64,
    pub(crate) last_running_attempt: u64,
    pub(crate) task_id: u32,
    pub(crate) oldest_output_sequence: u64,
    pub(crate) output_sequence: u64,
    pub(crate) next_start_unix_ms: u64,
    pub(crate) directory_revision: u64,
    pub(crate) hash: ObjectHash,
    pub(crate) name: String,
    pub(crate) last_exit: Option<NativeExit>,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeExit {
    pub(crate) kind: u8,
    pub(crate) code: i32,
    pub(crate) attempt: u64,
    pub(crate) detail: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NativeRuntimeLimits {
    pub(crate) memory_bytes: u64,
    pub(crate) stack_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NativeFamilyLimits {
    pub(crate) max_definitions: usize,
    pub(crate) max_follows_per_session: usize,
    pub(crate) max_running_attempts: usize,
    pub(crate) max_mutation_replays: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NativeRunOptions {
    /// Definition flags, including persistence/detachment and the initial
    /// enabled/desired state. They are committed before a supervisor can
    /// observe the definition.
    pub(crate) flags: u8,
    pub(crate) runtime: u8,
    pub(crate) follow_creator: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeCommandPage {
    pub(crate) directory_revision: u64,
    pub(crate) next_cursor: u64,
    pub(crate) records: Vec<NativeCommand>,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeCommand {
    pub(crate) extension_handle: u64,
    pub(crate) definition_revision: u64,
    pub(crate) content_hash: [u8; 32],
    pub(crate) name: String,
    pub(crate) listener_name: String,
    pub(crate) listener_generation: u64,
    pub(crate) descriptor: String,
}

/// A typed, owned supervisor reply exposed to the native YAS adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeStatus {
    pub(crate) extension_handle: u64,
    pub(crate) definition_revision: u64,
    pub(crate) replay_from_sequence: u64,
    pub(crate) output_sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativePutDisposition {
    Accepted { received: u64 },
    AlreadyPresent { size: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeControlAction {
    Stop,
    Restart,
    Enable,
    Disable,
    Remove,
    Attach,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeOutputKind {
    Stdout,
    Stderr,
    Log,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NativeAttemptIdentity {
    extension_handle: u64,
    generation: u64,
    definition_revision: u64,
    attempt: u64,
    task_id: u32,
}

impl From<&yas_wire::extension::AttemptContext> for NativeAttemptIdentity {
    fn from(context: &yas_wire::extension::AttemptContext) -> Self {
        Self {
            extension_handle: context.extension_handle,
            generation: context.generation,
            definition_revision: context.definition_revision,
            attempt: context.attempt,
            task_id: context.task_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeFollowItem {
    Output {
        kind: NativeOutputKind,
        sequence: u64,
        data: Vec<u8>,
    },
    Complete {
        through_sequence: u64,
    },
}

pub(crate) struct NativeFollowStream {
    pub(crate) attempt: u64,
    pub(crate) replay_from_sequence: u64,
    pub(crate) output_sequence: u64,
    extension_handle: u64,
    owner: Weak<NativeEndpointInner>,
    receiver: mpsc::Receiver<NativeFollowItem>,
}

impl NativeFollowStream {
    pub(crate) async fn next(&mut self) -> Option<NativeFollowItem> {
        self.receiver.recv().await
    }
}

impl Drop for NativeFollowStream {
    fn drop(&mut self) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        owner
            .follows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.extension_handle);
        spawn_native_unfollow(owner, self.extension_handle);
    }
}

#[derive(Clone)]
pub(crate) struct NativeEndpoint {
    inner: Arc<NativeEndpointInner>,
}

struct NativeEndpointInner {
    app: super::AppState,
    service: Arc<ExtensionService>,
    endpoint: u64,
    next_nonce: AtomicU32,
    pending: std::sync::Mutex<HashMap<u16, oneshot::Sender<NativeReply>>>,
    follows: std::sync::Mutex<HashMap<u64, NativeFollowRoute>>,
    closed: AtomicBool,
}

#[derive(Clone)]
struct NativeFollowRoute {
    attempt: u64,
    from_sequence: u64,
    sender: mpsc::Sender<NativeFollowItem>,
}

#[derive(Clone, Debug)]
enum NativeReply {
    Status(NativeStatus),
    Put(NativePutDisposition),
    Error(NativeMutationFailure),
}

#[derive(Clone, Debug)]
enum BackendEvent {
    Reply {
        nonce: u16,
        reply: NativeReply,
    },
    Retained {
        extension_handle: u64,
        sequence: u64,
        item: Arc<RetainedItem>,
    },
    ReplayDone {
        extension_handle: u64,
        through_sequence: u64,
    },
}

struct NativePendingGuard {
    inner: Weak<NativeEndpointInner>,
    nonce: u16,
}

impl Drop for NativePendingGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.nonce);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeCommandRegistration {
    pub(crate) extension_handle: u64,
    pub(crate) generation: u64,
    pub(crate) definition_revision: u64,
    pub(crate) directory_revision: u64,
    pub(crate) changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeMutationFailure {
    Unavailable,
    NotFound,
    Permission,
    Conflict,
    ResourceExhausted,
    TooLarge,
    Invalid(String),
    Unsupported(String),
    Cancelled,
    Closed,
    Internal(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeMutationSettlement {
    Success(Vec<u8>),
    Failure(NativeMutationFailure),
}

pub(crate) enum NativeMutationReplay {
    Miss,
    Conflict,
    Hit(NativeMutationSettlement),
}

#[derive(Clone)]
struct NativeMutationReplayEntry {
    fingerprint: [u8; 32],
    settlement: NativeMutationSettlement,
}

struct NativeMutationReplayCache {
    capacity: usize,
    values: HashMap<(u16, [u8; 16]), NativeMutationReplayEntry>,
    order: VecDeque<(u16, [u8; 16])>,
}

impl NativeMutationReplayCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            values: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn lookup(
        &self,
        operation_kind: u16,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
    ) -> Option<NativeMutationReplay> {
        self.values
            .get(&(operation_kind, operation_id))
            .map(|entry| {
                if entry.fingerprint == fingerprint {
                    NativeMutationReplay::Hit(entry.settlement.clone())
                } else {
                    NativeMutationReplay::Conflict
                }
            })
    }

    fn insert(
        &mut self,
        operation_kind: u16,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
        settlement: NativeMutationSettlement,
    ) {
        let key = (operation_kind, operation_id);
        if !self.values.contains_key(&key) {
            self.order.push_back(key);
        }
        self.values.insert(
            key,
            NativeMutationReplayEntry {
                fingerprint,
                settlement,
            },
        );
        while self.order.len() > self.capacity {
            if let Some(expired) = self.order.pop_front() {
                self.values.remove(&expired);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Interrupt {
    Cancelled,
    Updated,
    Restarted,
    Disabled,
    OwnerClosed,
    ServerShutdown,
}

enum ObjectProbe {
    Hit(ObjectRead),
    Miss,
    Durability(ObjectStoreError),
}

#[derive(Clone)]
struct AttemptControl {
    definition_revision: u64,
    attempt: u64,
    task_id: u32,
    host: AttemptCancellation,
    connection: super::ConnectionCancellation,
}

#[derive(Clone)]
struct RetainedRecord {
    sequence: u64,
    clock: u64,
    item: Arc<RetainedItem>,
}

#[derive(Clone, Debug)]
enum RetainedItemKind {
    Output {
        kind: NativeOutputKind,
        attempt: u64,
        data: Vec<u8>,
    },
    Exit(NativeExit),
    Status,
}

#[derive(Debug)]
struct RetainedItem {
    kind: RetainedItemKind,
    charged_bytes: usize,
    _reservation: Option<OutputReservation>,
}

impl RetainedItem {
    fn output(kind: NativeOutputKind, attempt: u64, data: Vec<u8>) -> Self {
        Self {
            charged_bytes: data.len().saturating_add(64),
            kind: RetainedItemKind::Output {
                kind,
                attempt,
                data,
            },
            _reservation: None,
        }
    }

    fn exit(exit: NativeExit) -> Self {
        Self {
            charged_bytes: exit.detail.len().saturating_add(96),
            kind: RetainedItemKind::Exit(exit),
            _reservation: None,
        }
    }

    const fn status(charged_bytes: usize) -> Self {
        Self {
            kind: RetainedItemKind::Status,
            charged_bytes,
            _reservation: None,
        }
    }
}

#[derive(Clone, Copy)]
struct FollowerCursor {
    next_sequence: u64,
    replay_through: Option<u64>,
}

#[derive(Clone)]
struct Definition {
    extension_id: u64,
    definition_revision: u64,
    flags: u8,
    restart: u8,
    /// Native YAS may fence a definition to one runtime. Unfenced definitions
    /// retain AUTO and resolve from immutable object bytes.
    native_runtime: u8,
    hash: ObjectHash,
    name: String,
    /// Arguments are resident only for transient definitions and uncommitted
    /// persistent creations. Committed persistent values stay in redb.
    args: Option<Vec<Vec<u8>>>,
    argument_bytes: usize,
    argument_reservation: Option<Arc<ArgumentReservation>>,
    owner_endpoint: Option<u64>,
    phase: u8,
    attempt: u64,
    last_running_attempt: u64,
    task_id: u32,
    next_start_unix_ms: u64,
    detail: String,
    next_output_sequence: u64,
    retained: VecDeque<RetainedRecord>,
    terminal_replay: VecDeque<RetainedRecord>,
    retained_bytes: usize,
    followers: HashMap<u64, FollowerCursor>,
    pending_deadline: Option<Instant>,
    release_deadline: Option<Instant>,
    generation: u64,
    failure_count: u32,
    interrupt: Option<Interrupt>,
    control: Option<AttemptControl>,
    object_pinned: bool,
    catalog_committed: bool,
    wake: Arc<Notify>,
}

impl Definition {
    fn persistent(&self) -> bool {
        self.flags & EXT_FLAG_PERSIST != 0
    }

    fn enabled(&self) -> bool {
        self.flags & EXT_FLAG_ENABLED != 0
    }

    fn desired(&self) -> bool {
        self.flags & EXT_FLAG_DESIRED_RUNNING != 0
    }

    fn latest_output_sequence(&self) -> u64 {
        self.next_output_sequence.saturating_sub(1)
    }

    fn set_flag(&mut self, bit: u8, value: bool) {
        if value {
            self.flags |= bit;
        } else {
            self.flags &= !bit;
        }
    }
}

struct ServiceState {
    store: Option<ObjectStore>,
    definitions: HashMap<u64, Definition>,
    endpoints: HashMap<u64, mpsc::Sender<BackendEvent>>,
    endpoint_wakes: HashMap<u64, Arc<Notify>>,
    supervisors: HashSet<u64>,
    supervisor_completions: HashMap<u64, Vec<Arc<SupervisorCompletion>>>,
    task_ids: HashSet<u32>,
    retained_bytes: usize,
    output_budget: Arc<OutputBudget>,
    retention_clock: u64,
    shutting_down: bool,
    commands: CommandDirectory,
}

#[derive(Debug)]
struct SupervisorCompletion {
    done: watch::Sender<bool>,
}

impl SupervisorCompletion {
    fn new() -> Arc<Self> {
        let (done, _) = watch::channel(false);
        Arc::new(Self { done })
    }

    fn complete(&self) {
        self.done.send_replace(true);
    }

    fn is_complete(&self) -> bool {
        *self.done.borrow()
    }

    async fn wait(&self) {
        let mut done = self.done.subscribe();
        while !*done.borrow_and_update() && done.changed().await.is_ok() {}
    }
}

struct SupervisorCompletionGuard(Arc<SupervisorCompletion>);

impl Drop for SupervisorCompletionGuard {
    fn drop(&mut self) {
        self.0.complete();
    }
}

#[derive(Debug)]
struct OutputBudget {
    max: usize,
    used: AtomicUsize,
}

impl OutputBudget {
    fn new(max: usize) -> Arc<Self> {
        Arc::new(Self {
            max,
            used: AtomicUsize::new(0),
        })
    }

    fn try_reserve(self: &Arc<Self>, bytes: usize) -> Option<OutputReservation> {
        let mut used = self.used.load(Ordering::Acquire);
        loop {
            let next = used.checked_add(bytes)?;
            if next > self.max {
                return None;
            }
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    return Some(OutputReservation {
                        budget: Arc::clone(self),
                        bytes,
                    });
                }
                Err(actual) => used = actual,
            }
        }
    }
}

#[derive(Debug)]
struct OutputReservation {
    budget: Arc<OutputBudget>,
    bytes: usize,
}

impl Drop for OutputReservation {
    fn drop(&mut self) {
        let previous = self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
        debug_assert!(previous >= self.bytes);
    }
}

struct ArgumentBudget {
    max: usize,
    used: AtomicUsize,
    notify: Notify,
}

impl ArgumentBudget {
    fn new(max: usize) -> Arc<Self> {
        Arc::new(Self {
            max,
            used: AtomicUsize::new(0),
            notify: Notify::new(),
        })
    }

    fn try_reserve(self: &Arc<Self>, bytes: usize) -> Option<Arc<ArgumentReservation>> {
        let mut used = self.used.load(Ordering::Acquire);
        loop {
            let next = used.checked_add(bytes)?;
            if next > self.max {
                return None;
            }
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    return Some(Arc::new(ArgumentReservation {
                        budget: Arc::clone(self),
                        bytes,
                    }));
                }
                Err(actual) => used = actual,
            }
        }
    }
}

struct ArgumentReservation {
    budget: Arc<ArgumentBudget>,
    bytes: usize,
}

impl Drop for ArgumentReservation {
    fn drop(&mut self) {
        let previous = self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
        debug_assert!(previous >= self.bytes);
        self.budget.notify.notify_waiters();
        // Retain one permit for a waiter which observed contention just
        // before this release but has not registered its future yet.
        self.budget.notify.notify_one();
    }
}

#[cfg(test)]
type CatalogHook = Arc<std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

/// Process-global extension storage, lifecycle registry, and fair admission.
pub(crate) struct ExtensionService {
    enabled: bool,
    available: bool,
    persist_allowed: bool,
    max_transient: usize,
    max_persistent: usize,
    follow_max_per_endpoint: usize,
    follow_max: usize,
    max_running: usize,
    argument_budget: Arc<ArgumentBudget>,
    /// Accounts request bytes retained by network-origin FINAL validation.
    validation_request_budget: Arc<ArgumentBudget>,
    output_retain_max: usize,
    pending_timeout: Duration,
    terminal_retain: Duration,
    host_config: WasmiHostConfig,
    running: Arc<Semaphore>,
    validating: Arc<Semaphore>,
    /// Serializes state transitions which temporarily move an upload or LRU
    /// victim out of the object-store owner while filesystem work is detached.
    /// Control/status paths never acquire this mutex.
    store_io: Mutex<()>,
    /// Orders durable catalog operations while their redb I/O runs on the
    /// blocking pool. The service-state mutex is never held across this lane.
    catalog_io: Mutex<()>,
    /// Serializes native DEPLOY/CONTROL replay admission across connections.
    /// It is distinct from `catalog_io` because the mutation itself acquires
    /// that durable lane internally.
    native_mutation_io: Mutex<()>,
    native_mutation_replays: std::sync::Mutex<NativeMutationReplayCache>,
    catalog: Arc<std::sync::Mutex<Option<ExtensionCatalog>>>,
    maintenance_started: AtomicBool,
    #[cfg(test)]
    validation_hook: std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    storage_hook: std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    catalog_hook: CatalogHook,
    inner: Mutex<ServiceState>,
}

impl ExtensionService {
    pub(crate) fn from_env(persist_allowed: bool, name: &crate::ServerName) -> Arc<Self> {
        let enabled = crate::extensions_enabled();
        let max_running =
            crate::deployment_usize("YAS_EXT_MAX_RUNNING", host_running_default()).clamp(1, 4);
        let max_validating =
            crate::deployment_usize("YAS_EXT_MAX_VALIDATING", DEFAULT_MAX_VALIDATING).max(1);
        let validation_request_max = usize::try_from(
            crate::deployment_u64("YAS_EXT_MODULE_MAX", EXT_MAX_MODULE).min(EXT_MAX_MODULE),
        )
        .unwrap_or(usize::MAX);
        let host_config = WasmiHostConfig {
            memory_bytes: crate::deployment_usize("YAS_EXT_MEMORY_MAX", 128 * 1024 * 1024),
            table_elements: crate::deployment_usize("YAS_EXT_TABLE_ELEMENTS_MAX", 65_536),
            value_stack_bytes: crate::deployment_usize("YAS_EXT_VALUE_STACK_MAX", 128 * 1024),
            call_depth: crate::deployment_usize("YAS_EXT_CALL_DEPTH_MAX", 1_024),
            native_stack_bytes: crate::deployment_usize("YAS_EXT_STACK_SIZE", 2 * 1024 * 1024),
            fuel_slice: crate::deployment_u64("YAS_EXT_FUEL_SLICE", 1_000_000),
        };

        let mut diagnostic = None;
        let mut store = None;
        let mut catalog = None;
        let mut definitions = HashMap::new();

        if enabled {
            let opened = ObjectStoreConfig::from_env(name)
                .ok_or_else(|| "extension cache directory is unavailable".to_owned())
                .and_then(|config| ObjectStore::open(config).map_err(|error| error.to_string()));
            match opened {
                Ok(mut opened_store) => match ExtensionCatalog::from_env(name) {
                    Ok(mut opened_catalog) => {
                        for persistent in opened_catalog.list() {
                            let mut definition = definition_from_persistent(persistent);
                            let object_block = if opened_store.pin(&definition.hash).is_ok() {
                                definition.object_pinned = true;
                                (!opened_store.is_usable(&definition.hash)).then_some(
                                    "persistent extension object exceeds the configured module limit",
                                )
                            } else {
                                Some("persistent extension object is absent from the cache")
                            };
                            if let Some(block_detail) = object_block {
                                definition.phase = EXT_PHASE_BLOCKED;
                                definition.detail = block_detail.into();
                                if let Err(error) = opened_catalog.set_lifecycle(
                                    definition.extension_id,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    Some(0),
                                    Some(BlockedState::Set(&definition.detail)),
                                ) {
                                    diagnostic = Some(error.to_string());
                                }
                            }
                            if !persist_allowed && definition.desired() && definition.enabled() {
                                definition.phase = EXT_PHASE_BLOCKED;
                                definition.detail =
                                    "persistent extensions are disabled on this server".into();
                            }
                            if definition.phase != EXT_PHASE_BACKOFF {
                                definition.next_start_unix_ms = 0;
                            }
                            definitions.insert(definition.extension_id, definition);
                        }
                        let list_fits = definitions.len()
                            <= yas_wire::schema::extension::MAX_DEFINITIONS as usize;
                        if !list_fits {
                            diagnostic = Some(
                                "persistent extension catalog exceeds the wire list ceiling".into(),
                            );
                        } else if let Err(error) = opened_store.finish_startup_gc() {
                            diagnostic = Some(error.to_string());
                        } else if host_config.validate().is_err() {
                            diagnostic =
                                Some("invalid extension runtime containment limits".into());
                        } else {
                            store = Some(opened_store);
                            catalog = Some(opened_catalog);
                        }
                    }
                    Err(error) => diagnostic = Some(error.to_string()),
                },
                Err(error) => diagnostic = Some(error),
            }
        }

        let configured_max_transient =
            crate::deployment_usize("YAS_EXT_MAX_TRANSIENT", DEFAULT_MAX_TRANSIENT);
        let configured_max_persistent =
            crate::deployment_usize("YAS_EXT_MAX_PERSISTENT", DEFAULT_MAX_PERSISTENT);
        if configured_max_transient.saturating_add(configured_max_persistent) > u16::MAX as usize {
            diagnostic.get_or_insert_with(|| {
                "extension persistent and transient caps exceed the wire list ceiling".into()
            });
        }
        let effective_max_transient =
            configured_max_transient.min((u16::MAX as usize).saturating_sub(definitions.len()));
        let output_retain_max =
            crate::deployment_usize("YAS_EXT_OUTPUT_RETAIN_MAX", DEFAULT_OUTPUT_RETAIN_MAX);

        if let Some(detail) = diagnostic.as_deref() {
            eprintln!("yas-server: extension subsystem disabled: {detail}");
        }

        let available = enabled && store.is_some() && catalog.is_some() && diagnostic.is_none();
        Arc::new(Self {
            enabled,
            available,
            persist_allowed,
            max_transient: effective_max_transient,
            max_persistent: configured_max_persistent,
            follow_max_per_endpoint: crate::deployment_usize(
                "YAS_EXT_FOLLOW_MAX_PER_CLIENT",
                DEFAULT_FOLLOW_MAX_PER_ENDPOINT,
            ),
            follow_max: crate::deployment_usize("YAS_EXT_FOLLOW_MAX", DEFAULT_FOLLOW_MAX),
            max_running,
            argument_budget: ArgumentBudget::new(crate::deployment_usize(
                "YAS_EXT_ARGUMENT_STORE_MAX",
                DEFAULT_ARGUMENT_STORE_MAX,
            )),
            validation_request_budget: ArgumentBudget::new(validation_request_max),
            output_retain_max,
            pending_timeout: Duration::from_secs(crate::deployment_u64(
                "YAS_EXT_PENDING_TIMEOUT",
                DEFAULT_PENDING_TIMEOUT.as_secs(),
            )),
            terminal_retain: Duration::from_secs(
                crate::deployment_u64("YAS_EXT_TERMINAL_RETAIN", DEFAULT_TERMINAL_RETAIN.as_secs())
                    .max(1),
            ),
            host_config,
            running: Arc::new(Semaphore::new(max_running)),
            validating: Arc::new(Semaphore::new(max_validating)),
            store_io: Mutex::new(()),
            catalog_io: Mutex::new(()),
            native_mutation_io: Mutex::new(()),
            native_mutation_replays: std::sync::Mutex::new(NativeMutationReplayCache::new(
                mutation_replay_capacity(configured_max_persistent),
            )),
            catalog: Arc::new(std::sync::Mutex::new(catalog)),
            maintenance_started: AtomicBool::new(false),
            #[cfg(test)]
            validation_hook: std::sync::Mutex::new(None),
            #[cfg(test)]
            storage_hook: std::sync::Mutex::new(None),
            #[cfg(test)]
            catalog_hook: Arc::new(std::sync::Mutex::new(None)),
            inner: Mutex::new(ServiceState {
                store,
                definitions,
                endpoints: HashMap::new(),
                endpoint_wakes: HashMap::new(),
                supervisors: HashSet::new(),
                supervisor_completions: HashMap::new(),
                task_ids: HashSet::new(),
                retained_bytes: 0,
                output_budget: OutputBudget::new(output_retain_max),
                retention_clock: 0,
                shutting_down: false,
                commands: CommandDirectory::default(),
            }),
        })
    }

    /// Isolated capable service for native protocol integration tests. Tests
    /// provide a unique root, so no process-global environment mutation or
    /// live server catalog lock is involved.
    #[cfg(test)]
    pub(crate) fn persistent_for_test(root: &std::path::Path) -> Arc<Self> {
        let output_retain_max = 8 * 1024 * 1024;
        let max_running = 2;
        let store = ObjectStore::open(ObjectStoreConfig {
            root: root.join("cache"),
            module_max: EXT_MAX_MODULE,
            cache_max: 128 * 1024 * 1024,
            entry_max: 32,
            upload_max: 4,
            upload_max_per_endpoint: 2,
            upload_timeout: Duration::from_secs(30),
            allocation_quantum: 4096,
        })
        .expect("open isolated Extension object store");
        let catalog = ExtensionCatalog::open(Some(root.join("extensions.redb")), 8)
            .expect("open isolated Extension catalog");
        Arc::new(Self {
            enabled: true,
            available: true,
            persist_allowed: true,
            max_transient: 8,
            max_persistent: 8,
            follow_max_per_endpoint: 8,
            follow_max: 32,
            max_running,
            argument_budget: ArgumentBudget::new(8 * 1024 * 1024),
            validation_request_budget: ArgumentBudget::new(
                usize::try_from(EXT_MAX_MODULE).expect("Extension module limit fits usize"),
            ),
            output_retain_max,
            pending_timeout: Duration::from_secs(30),
            terminal_retain: DEFAULT_TERMINAL_RETAIN,
            host_config: WasmiHostConfig::default(),
            running: Arc::new(Semaphore::new(max_running)),
            validating: Arc::new(Semaphore::new(1)),
            store_io: Mutex::new(()),
            catalog_io: Mutex::new(()),
            native_mutation_io: Mutex::new(()),
            native_mutation_replays: std::sync::Mutex::new(NativeMutationReplayCache::new(
                mutation_replay_capacity(8),
            )),
            catalog: Arc::new(std::sync::Mutex::new(Some(catalog))),
            maintenance_started: AtomicBool::new(false),
            validation_hook: std::sync::Mutex::new(None),
            storage_hook: std::sync::Mutex::new(None),
            catalog_hook: Arc::new(std::sync::Mutex::new(None)),
            inner: Mutex::new(ServiceState {
                store: Some(store),
                definitions: HashMap::new(),
                endpoints: HashMap::new(),
                endpoint_wakes: HashMap::new(),
                supervisors: HashSet::new(),
                supervisor_completions: HashMap::new(),
                task_ids: HashSet::new(),
                retained_bytes: 0,
                output_budget: OutputBudget::new(output_retain_max),
                retention_clock: 0,
                shutting_down: false,
                commands: CommandDirectory::new(Default::default()),
            }),
        })
    }

    pub(crate) fn advertised(&self) -> bool {
        self.enabled && self.available
    }

    pub(crate) fn native_runtime_limits(&self) -> NativeRuntimeLimits {
        NativeRuntimeLimits {
            memory_bytes: self.host_config.memory_bytes as u64,
            stack_bytes: self.host_config.native_stack_bytes as u64,
        }
    }

    pub(crate) fn native_family_limits(&self) -> NativeFamilyLimits {
        NativeFamilyLimits {
            max_definitions: self.max_transient.saturating_add(self.max_persistent),
            max_follows_per_session: self.follow_max_per_endpoint.min(self.follow_max),
            max_running_attempts: self.max_running,
            max_mutation_replays: mutation_replay_capacity(self.max_persistent),
        }
    }

    pub(crate) fn native_persist_allowed(&self) -> bool {
        self.persist_allowed
    }

    pub(crate) async fn lock_native_mutation(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.native_mutation_io.lock().await
    }

    /// Resolve a native mutation settlement from the process-global boot
    /// cache or the successful-persistent journal. A fingerprint mismatch is
    /// an explicit conflict, never a cache miss that could repeat a mutation.
    pub(crate) async fn native_mutation_replay(
        &self,
        operation_kind: u16,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
    ) -> Result<NativeMutationReplay, CatalogError> {
        if let Some(replay) = self
            .native_mutation_replays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .lookup(operation_kind, operation_id, fingerprint)
        {
            return Ok(replay);
        }
        let replay = self
            .catalog_call(move |catalog| catalog.mutation_replay(operation_kind, operation_id))
            .await?;
        let Some(replay) = replay else {
            return Ok(NativeMutationReplay::Miss);
        };
        let result = if replay.fingerprint == fingerprint {
            NativeMutationReplay::Hit(NativeMutationSettlement::Success(
                replay.result_body.clone(),
            ))
        } else {
            NativeMutationReplay::Conflict
        };
        self.native_mutation_replays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                operation_kind,
                operation_id,
                replay.fingerprint,
                NativeMutationSettlement::Success(replay.result_body),
            );
        Ok(result)
    }

    /// Record an exact successful Result. Persistent mutations are committed
    /// to redb before this returns, and callers do not expose their YAS Result
    /// until after that durability boundary.
    pub(crate) async fn record_native_mutation(
        &self,
        operation_kind: u16,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
        result_body: Vec<u8>,
        persistent: bool,
    ) -> Result<(), CatalogError> {
        let replay = PersistentMutationReplay {
            fingerprint,
            result_body,
        };
        if persistent {
            let stored = replay.clone();
            self.catalog_call(move |catalog| {
                catalog.put_mutation_replay(operation_kind, operation_id, &stored)
            })
            .await?;
        }
        self.native_mutation_replays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                operation_kind,
                operation_id,
                replay.fingerprint,
                NativeMutationSettlement::Success(replay.result_body),
            );
        Ok(())
    }

    /// Retain a settled, noncommitted native mutation failure for this server
    /// boot. Failures are never written to the persistent journal, but share
    /// the family's advertised bounded replay horizon with successful
    /// settlements so reconnecting clients cannot re-evaluate one operation
    /// ID against changed state.
    pub(crate) fn record_native_mutation_failure(
        &self,
        operation_kind: u16,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
        failure: NativeMutationFailure,
    ) {
        self.native_mutation_replays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                operation_kind,
                operation_id,
                fingerprint,
                NativeMutationSettlement::Failure(failure),
            );
    }

    pub(crate) async fn native_object_runtime(&self, hash: ObjectHash) -> Option<u8> {
        let read = self
            .inner
            .lock()
            .await
            .store
            .as_mut()?
            .reserve_read(&hash)
            .ok()?;
        if read.starts_with(WASM_MAGIC).ok()? {
            Some(yas_wire::schema::extension::RUNTIME_WASMI as u8)
        } else {
            Some(yas_wire::schema::extension::RUNTIME_QUICKJS as u8)
        }
    }

    async fn publish_native_attempt_output(
        &self,
        identity: NativeAttemptIdentity,
        kind: NativeOutputKind,
        data: &[u8],
    ) -> Result<u64, NativeMutationFailure> {
        if data.len() > yas_wire::extension::MAX_OUTPUT_RECORD_BYTES {
            return Err(NativeMutationFailure::TooLarge);
        }
        let mut inner = self.inner.lock().await;
        let definition = inner
            .definitions
            .get(&identity.extension_handle)
            .ok_or(NativeMutationFailure::NotFound)?;
        let active = definition.generation == identity.generation
            && definition.control.as_ref().is_some_and(|control| {
                control.definition_revision == identity.definition_revision
                    && control.attempt == identity.attempt
                    && control.task_id == identity.task_id
            });
        if !active {
            return Err(NativeMutationFailure::Conflict);
        }
        let sequence = definition.next_output_sequence;
        let item = RetainedItem::output(kind, identity.attempt, data.to_vec());
        retain_and_fanout(
            &mut inner,
            identity.extension_handle,
            item,
            self.output_retain_max,
        );
        Ok(sequence)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn native_run(
        self: &Arc<Self>,
        state: super::AppState,
        endpoint: u64,
        nonce: u16,
        restart: u8,
        expected_id: u64,
        expected_generation: u64,
        expected_revision: u64,
        hash: ObjectHash,
        name: &str,
        arguments: Vec<&[u8]>,
        options: NativeRunOptions,
    ) {
        let mut run_flags = 0;
        if options.flags & EXT_FLAG_DETACH != 0 {
            run_flags |= EXT_RUN_DETACH;
        }
        if options.flags & EXT_FLAG_PERSIST != 0 {
            run_flags |= EXT_RUN_PERSIST;
        }
        if expected_revision != 0 {
            run_flags |= EXT_RUN_UPDATE;
        }
        self.handle_run(
            state,
            endpoint,
            nonce,
            run_flags,
            restart,
            expected_id,
            expected_revision,
            Some(expected_generation),
            hash,
            name,
            arguments,
            Some(options),
            None,
        )
        .await;
    }

    pub(crate) async fn native_discover_commands(
        &self,
        endpoint: u64,
        directory_revision: u64,
        cursor: u64,
        max_records: usize,
    ) -> Result<NativeCommandPage, NativeMutationFailure> {
        let page = self.inner.lock().await.commands.discover_limited(
            endpoint,
            directory_revision,
            cursor,
            max_records,
            Instant::now(),
        );
        match page.status {
            DiscoveryStatus::Ok => {}
            DiscoveryStatus::Budget => return Err(NativeMutationFailure::ResourceExhausted),
            DiscoveryStatus::Conflict => return Err(NativeMutationFailure::Conflict),
        }
        Ok(NativeCommandPage {
            directory_revision: page.directory_revision,
            next_cursor: page.next_cursor,
            records: page
                .records
                .into_iter()
                .map(|record| NativeCommand {
                    extension_handle: record.extension_id,
                    definition_revision: record.definition_revision,
                    content_hash: record.hash,
                    name: record.name,
                    listener_name: record.listener_name,
                    listener_generation: u64::from_le_bytes(
                        record.listener_token[8..]
                            .try_into()
                            .expect("listener generation width"),
                    ),
                    descriptor: record.descriptor,
                })
                .collect(),
        })
    }

    /// Atomically publish the command descriptor owned by one authenticated
    /// Extension attempt and one listener on that same native YAS endpoint.
    /// The public listener handle is resolved under the Channel registry lock;
    /// neither the guest nor this API may supply an owner identity.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn native_register_command(
        &self,
        state: super::AppState,
        channel_endpoint: u64,
        extension_handle: u64,
        generation: u64,
        definition_revision: u64,
        attempt: u64,
        task_id: u32,
        listener_handle: u64,
        listener_generation: u64,
        descriptor: &str,
    ) -> Result<NativeCommandRegistration, NativeMutationFailure> {
        let identity = (extension_handle, definition_revision, attempt, task_id);
        let endpoint_generation = state.boot_generation;
        let captured_listener = if listener_handle == 0 {
            None
        } else {
            let session = state.session.lock().await;
            session.channels.native_listener_snapshot(
                channel_endpoint,
                listener_handle,
                listener_generation,
            )
        };
        let listener_id = captured_listener
            .as_ref()
            .map_or(0, |listener| listener.registry_id);
        let prepared = {
            let inner = self.inner.lock().await;
            let owner = command_owner(&inner, channel_endpoint, endpoint_generation, identity)
                .filter(|_| {
                    inner
                        .definitions
                        .get(&extension_handle)
                        .is_some_and(|definition| definition.generation == generation)
                });
            let listener = captured_listener
                .clone()
                .map(|listener| command_listener(endpoint_generation, listener));
            inner
                .commands
                .prepare_registration(owner.as_ref(), listener_id, descriptor, listener.as_ref())
                .map_err(registration_native_error)?
        };

        // Keep the Channel registry immutable while the directory rechecks
        // and commits both fences. This preserves the established global lock
        // order: Session/Channel, then Extension directory.
        let session = state.session.lock().await;
        let current_listener = if listener_handle == 0 {
            None
        } else {
            session.channels.native_listener_snapshot(
                channel_endpoint,
                listener_handle,
                listener_generation,
            )
        };
        let result = {
            let mut inner = self.inner.lock().await;
            let owner = command_owner(&inner, channel_endpoint, endpoint_generation, identity)
                .filter(|_| {
                    inner
                        .definitions
                        .get(&extension_handle)
                        .is_some_and(|definition| definition.generation == generation)
                });
            let listener =
                current_listener.map(|listener| command_listener(endpoint_generation, listener));
            inner
                .commands
                .commit_registration(prepared, owner.as_ref(), listener.as_ref())
                .map_err(registration_native_error)?
        };
        drop(session);
        Ok(NativeCommandRegistration {
            extension_handle: result.extension_id,
            generation,
            definition_revision: result.definition_revision,
            directory_revision: result.directory_revision,
            changed: result.changed,
        })
    }

    /// Snapshot the process-global extension catalogue as owned semantic values.
    /// Object prefixes are read through temporary store pins so eviction can
    /// never race runtime classification.
    pub(crate) async fn native_snapshot(&self) -> Vec<NativeDefinition> {
        let (definitions, directory_revision, reads) = {
            let mut inner = self.inner.lock().await;
            let mut definitions = inner.definitions.values().cloned().collect::<Vec<_>>();
            definitions.sort_by(|left, right| {
                left.name
                    .as_bytes()
                    .cmp(right.name.as_bytes())
                    .then(left.extension_id.cmp(&right.extension_id))
            });
            let reads = definitions
                .iter()
                .map(|definition| {
                    inner
                        .store
                        .as_mut()
                        .and_then(|store| store.reserve_read(&definition.hash).ok())
                })
                .collect::<Vec<_>>();
            (definitions, inner.commands.revision(), reads)
        };

        definitions
            .into_iter()
            .zip(reads)
            .map(|(definition, object)| {
                let runtime = if definition.native_runtime
                    != yas_wire::schema::extension::RUNTIME_AUTO as u8
                {
                    definition.native_runtime
                } else if definition.phase == EXT_PHASE_NEED_OBJECT {
                    yas_wire::schema::extension::RUNTIME_AUTO as u8
                } else if object
                    .as_ref()
                    .is_some_and(|read| read.starts_with(WASM_MAGIC).unwrap_or(false))
                {
                    yas_wire::schema::extension::RUNTIME_WASMI as u8
                } else {
                    yas_wire::schema::extension::RUNTIME_QUICKJS as u8
                };
                let last_exit = definition
                    .retained
                    .iter()
                    .chain(definition.terminal_replay.iter())
                    .rev()
                    .find_map(|record| match &record.item.kind {
                        RetainedItemKind::Exit(exit) => Some(exit.clone()),
                        _ => None,
                    });
                NativeDefinition {
                    extension_handle: definition.extension_id,
                    generation: definition.generation.max(1),
                    definition_revision: definition.definition_revision,
                    phase: definition.phase,
                    runtime,
                    restart: definition.restart,
                    flags: definition.flags,
                    attempt: definition.attempt,
                    last_running_attempt: definition.last_running_attempt,
                    task_id: definition.task_id,
                    oldest_output_sequence: oldest_replay_sequence(&definition),
                    output_sequence: definition.latest_output_sequence(),
                    next_start_unix_ms: definition.next_start_unix_ms,
                    directory_revision,
                    hash: definition.hash,
                    name: definition.name,
                    last_exit,
                }
            })
            .collect()
    }

    async fn catalog_call<R, F>(&self, operation: F) -> Result<R, CatalogError>
    where
        R: Send + 'static,
        F: FnOnce(&mut ExtensionCatalog) -> Result<R, CatalogError> + Send + 'static,
    {
        let catalog = Arc::clone(&self.catalog);
        #[cfg(test)]
        let catalog_hook = Arc::clone(&self.catalog_hook);
        tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(hook) = catalog_hook.lock().expect("catalog hook lock").clone() {
                hook();
            }
            let mut catalog = catalog
                .lock()
                .map_err(|_| CatalogError::Storage("extension catalog lock poisoned".into()))?;
            operation(catalog.as_mut().ok_or(CatalogError::Unavailable)?)
        })
        .await
        .map_err(|error| {
            CatalogError::Storage(format!("extension catalog worker failed: {error}"))
        })?
    }

    async fn definition_arguments(
        &self,
        definition: &Definition,
    ) -> Result<Vec<Vec<u8>>, CatalogError> {
        if let Some(arguments) = &definition.args {
            return Ok(arguments.clone());
        }
        let extension_id = definition.extension_id;
        let definition_revision = definition.definition_revision;
        let expected_bytes = definition.argument_bytes;
        let arguments = self
            .catalog_call(move |catalog| catalog.load_arguments(extension_id, definition_revision))
            .await?
            .into_iter()
            .map(String::into_bytes)
            .collect::<Vec<_>>();
        if encoded_argument_bytes(&arguments) != expected_bytes {
            return Err(CatalogError::Storage(
                "persistent extension argument metadata changed".into(),
            ));
        }
        Ok(arguments)
    }

    async fn commit_catalog_create(
        &self,
        definition: &Definition,
    ) -> Result<PersistentDefinition, (u8, String)> {
        let arguments = definition
            .args
            .as_ref()
            .ok_or((
                EXT_STATUS_OTHER,
                "pending extension arguments are unavailable".into(),
            ))?
            .iter()
            .map(|argument| {
                std::str::from_utf8(argument)
                    .map(str::to_owned)
                    .map_err(|_| {
                        (
                            EXT_STATUS_INVALID,
                            "persistent arguments must be UTF-8".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let extension_id = definition.extension_id;
        let hash = definition.hash;
        let name = definition.name.clone();
        let restart = definition.restart;
        let flags = definition.flags;
        {
            let mut inner = self.inner.lock().await;
            inner
                .store
                .as_mut()
                .ok_or((EXT_STATUS_OTHER, "object store is unavailable".into()))?
                .pin(&hash)
                .map_err(|error| (object_status(&error), error.to_string()))?;
        }
        let committed = self
            .catalog_call(move |catalog| {
                catalog.create_with_id_flags(extension_id, hash, name, arguments, restart, flags)
            })
            .await;
        if let Err(error) = committed {
            let mut inner = self.inner.lock().await;
            if let Some(store) = inner.store.as_mut() {
                store.unpin(&hash);
            }
            return Err((catalog_status(&error), error.to_string()));
        }
        Ok(committed.expect("checked catalog create result"))
    }

    async fn commit_catalog_update(
        &self,
        current: &Definition,
        hash: ObjectHash,
        args: Vec<Vec<u8>>,
        restart: u8,
        flags: u8,
    ) -> Result<PersistentDefinition, CatalogError> {
        let changed_hash = hash != current.hash;
        let acquired_pin = changed_hash || !current.object_pinned;
        let arguments = args
            .into_iter()
            .map(|argument| {
                String::from_utf8(argument)
                    .map_err(|_| CatalogError::Invalid("persistent arguments must be UTF-8"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if acquired_pin {
            let mut inner = self.inner.lock().await;
            inner
                .store
                .as_mut()
                .ok_or(CatalogError::Unavailable)?
                .pin(&hash)
                .map_err(|error| CatalogError::Storage(error.to_string()))?;
        }
        let extension_id = current.extension_id;
        let definition_revision = current.definition_revision;
        let name = current.name.clone();
        let updated = self
            .catalog_call(move |catalog| {
                catalog.update_with_flags(
                    extension_id,
                    definition_revision,
                    &name,
                    hash,
                    arguments,
                    restart,
                    flags,
                )
            })
            .await;
        let mut inner = self.inner.lock().await;
        match updated {
            Ok(updated) => {
                inner
                    .commands
                    .invalidate_definition(current.extension_id, current.definition_revision);
                if changed_hash
                    && current.object_pinned
                    && let Some(store) = inner.store.as_mut()
                {
                    store.unpin(&current.hash);
                }
                Ok(updated)
            }
            Err(error) => {
                if acquired_pin && let Some(store) = inner.store.as_mut() {
                    store.unpin(&hash);
                }
                Err(error)
            }
        }
    }

    async fn persist_attempt_counters_catalog(
        &self,
        extension_id: u64,
        attempt: u64,
        last_running: u64,
        persistent: bool,
    ) -> Result<(), CatalogError> {
        if !persistent {
            return Ok(());
        }
        self.catalog_call(move |catalog| {
            catalog
                .set_lifecycle(
                    extension_id,
                    None,
                    None,
                    Some(attempt),
                    Some(last_running),
                    None,
                    None,
                    None,
                )
                .map(|_| ())
        })
        .await
    }

    async fn persist_terminal_catalog(&self, definition: &Definition) -> Result<(), CatalogError> {
        if !definition.persistent() {
            return Ok(());
        }
        let extension_id = definition.extension_id;
        let enabled = definition.enabled();
        let desired = definition.desired();
        let attempt = definition.attempt;
        let last_running_attempt = definition.last_running_attempt;
        let failure_count = definition.failure_count;
        let next_start_unix_ms = definition.next_start_unix_ms;
        self.catalog_call(move |catalog| {
            catalog
                .set_lifecycle(
                    extension_id,
                    Some(enabled),
                    Some(desired),
                    Some(attempt),
                    Some(last_running_attempt),
                    Some(failure_count),
                    Some(next_start_unix_ms),
                    Some(BlockedState::Clear),
                )
                .map(|_| ())
        })
        .await
    }

    fn validate_module(&self, module: &[u8]) -> Result<(), String> {
        #[cfg(test)]
        if let Some(hook) = self
            .validation_hook
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            hook();
        }
        validate_extension_object(module, &self.host_config).map_err(|error| error.to_string())
    }

    fn before_storage_io(&self) {
        #[cfg(test)]
        if let Some(hook) = self
            .storage_hook
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            hook();
        }
    }

    async fn probe_object(&self, hash: ObjectHash, durable: bool) -> ObjectProbe {
        let reserved = {
            let mut inner = self.inner.lock().await;
            inner
                .store
                .as_mut()
                .ok_or(ObjectStoreError::NotFound)
                .and_then(|store| store.reserve_read(&hash))
        };
        let read = match reserved {
            Ok(read) => read,
            Err(_) => return ObjectProbe::Miss,
        };
        let validation = tokio::task::block_in_place(|| {
            let module = read.read_verified()?;
            self.validate_module(&module)
                .map_err(ObjectStoreError::InvalidModule)?;
            Ok::<(), ObjectStoreError>(())
        });
        match validation {
            Ok(()) => {
                {
                    let mut inner = self.inner.lock().await;
                    if let Some(store) = inner.store.as_mut() {
                        store.mark_executable(&hash);
                    }
                }
                if durable && let Err(error) = tokio::task::block_in_place(|| read.sync()) {
                    return ObjectProbe::Durability(error);
                }
                if let Err(error) = self.persist_store_lru().await {
                    return ObjectProbe::Durability(error);
                }
                ObjectProbe::Hit(read)
            }
            Err(error) => {
                let missing = matches!(
                    &error,
                    ObjectStoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound
                );
                let invalid = matches!(
                    &error,
                    ObjectStoreError::HashMismatch | ObjectStoreError::InvalidModule(_)
                );
                if missing || (invalid && read.remove_file().is_ok()) {
                    let mut inner = self.inner.lock().await;
                    if let Some(store) = inner.store.as_mut() {
                        store.forget_removed(&hash);
                    }
                    mark_hash_unpinned(&mut inner, &hash);
                }
                ObjectProbe::Miss
            }
        }
    }

    /// Open a typed endpoint to the process-global supervisor.
    pub(crate) async fn open_native_endpoint(
        self: &Arc<Self>,
        app: super::AppState,
    ) -> Result<NativeEndpoint, NativeMutationFailure> {
        if !self.advertised() {
            return Err(NativeMutationFailure::Unavailable);
        }
        let (sender, receiver) = mpsc::channel(NATIVE_ENDPOINT_QUEUE);
        let (endpoint, wake) = loop {
            let mut random = [0; 8];
            getrandom::fill(&mut random).map_err(|_| NativeMutationFailure::ResourceExhausted)?;
            let endpoint = u64::from_le_bytes(random) | (1_u64 << 63);
            if endpoint == 0 {
                continue;
            }
            let wake = Arc::new(Notify::new());
            let mut state = self.inner.lock().await;
            if state.endpoints.contains_key(&endpoint) {
                continue;
            }
            state.endpoints.insert(endpoint, sender.clone());
            state.endpoint_wakes.insert(endpoint, Arc::clone(&wake));
            break (endpoint, wake);
        };
        let inner = Arc::new(NativeEndpointInner {
            app,
            service: Arc::clone(self),
            endpoint,
            next_nonce: AtomicU32::new(1),
            pending: std::sync::Mutex::new(HashMap::new()),
            follows: std::sync::Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        });
        tokio::spawn(route_native_events(Arc::downgrade(&inner), receiver));
        let service = Arc::clone(self);
        tokio::spawn(async move {
            service.output_scheduler(endpoint, wake).await;
        });
        Ok(NativeEndpoint { inner })
    }

    async fn output_scheduler(self: Arc<Self>, endpoint: u64, wake: Arc<Notify>) {
        let mut last_extension = None;
        loop {
            let sender = {
                let inner = self.inner.lock().await;
                match (
                    inner.endpoints.get(&endpoint),
                    inner.endpoint_wakes.get(&endpoint),
                ) {
                    (Some(sender), Some(current)) if Arc::ptr_eq(current, &wake) => sender.clone(),
                    _ => return,
                }
            };
            if sender.is_closed() {
                return;
            }
            let outcome = {
                let mut inner = self.inner.lock().await;
                schedule_one_locked(&mut inner, endpoint, last_extension)
            };
            match outcome {
                ScheduleOutcome::Sent(extension_id) => {
                    last_extension = Some(extension_id);
                    tokio::task::yield_now().await;
                }
                ScheduleOutcome::Idle => wake.notified().await,
                ScheduleOutcome::Closed => return,
            }
        }
    }

    pub(crate) async fn unregister_endpoint(
        self: &Arc<Self>,
        endpoint: u64,
        endpoint_generation: u64,
    ) {
        let store_io = self.store_io.lock().await;
        let mut to_cancel = Vec::new();
        let mut to_wait = Vec::new();
        let mut upload_cleanups = Vec::new();
        {
            let mut inner = self.inner.lock().await;
            inner.endpoints.remove(&endpoint);
            if let Some(wake) = inner.endpoint_wakes.remove(&endpoint) {
                wake.notify_one();
            }
            inner.commands.close_endpoint(endpoint);
            inner
                .commands
                .invalidate_endpoint(endpoint, endpoint_generation);
            let aborted_uploads = inner.store.as_mut().map_or_else(Vec::new, |store| {
                let (hashes, cleanups) = store.take_endpoint_uploads(endpoint);
                upload_cleanups = cleanups;
                hashes
            });
            notify_need_object_locked(&mut inner, &aborted_uploads, self.output_retain_max);
            let mut changed = Vec::new();
            let mut remove_now = Vec::new();
            let mut invalidate_attempts = Vec::new();
            let mut owned_definitions = Vec::new();
            for definition in inner.definitions.values_mut() {
                definition.followers.remove(&endpoint);
                if definition.owner_endpoint == Some(endpoint) {
                    owned_definitions.push(definition.extension_id);
                    definition.set_flag(EXT_FLAG_DESIRED_RUNNING, false);
                    definition.interrupt = Some(Interrupt::OwnerClosed);
                    definition.generation = definition.generation.saturating_add(1);
                    definition.wake.notify_waiters();
                    // Preserve a wake permit when the supervisor is between
                    // checks so cleanup cannot fall through terminal retain.
                    definition.wake.notify_one();
                    if let Some(control) = definition.control.clone() {
                        definition.phase = EXT_PHASE_STOPPING;
                        definition.task_id = 0;
                        invalidate_attempts.push((
                            definition.extension_id,
                            control.definition_revision,
                            control.attempt,
                        ));
                        to_cancel.push(control);
                        changed.push(definition.extension_id);
                    } else {
                        definition.phase = EXT_PHASE_STOPPED;
                        definition.pending_deadline = None;
                        definition.release_deadline = None;
                        definition.detail = "attached owner disconnected".into();
                        changed.push(definition.extension_id);
                        remove_now.push(definition.extension_id);
                    }
                }
            }
            for extension_id in owned_definitions {
                if let Some(completions) = inner.supervisor_completions.get(&extension_id) {
                    to_wait.extend(completions.iter().cloned());
                }
            }
            for (extension_id, revision, attempt) in invalidate_attempts {
                inner
                    .commands
                    .invalidate_attempt(extension_id, revision, attempt);
            }
            for extension_id in changed {
                emit_lifecycle_locked(&mut inner, extension_id, self.output_retain_max);
            }
            for extension_id in remove_now {
                remove_definition_locked(&mut inner, extension_id);
            }
        }
        let cleanup_results = if upload_cleanups.is_empty() {
            Vec::new()
        } else {
            tokio::task::spawn_blocking(move || {
                upload_cleanups
                    .into_iter()
                    .map(|cleanup| cleanup.finish())
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default()
        };
        if !cleanup_results.is_empty() {
            let mut inner = self.inner.lock().await;
            if let Some(store) = inner.store.as_mut() {
                for cleanup in cleanup_results {
                    store.commit_upload_cleanup(cleanup);
                }
            }
        }
        drop(store_io);
        for control in to_cancel {
            control.connection.cancel();
            control.host.cancel();
        }
        // A child supervisor completes only after its runtime attempt, logical
        // connection, native jobs, and that connection's own attached
        // children have drained. Waiting outside the service lock therefore
        // forms a recursive cleanup barrier without serializing unrelated
        // extension work.
        for completion in to_wait {
            completion.wait().await;
        }
    }

    pub(crate) async fn restore(self: &Arc<Self>, state: super::AppState) {
        if self
            .maintenance_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let service = Arc::clone(self);
            tokio::spawn(async move {
                service.maintenance_loop().await;
            });
        }
        let definitions = {
            let inner = self.inner.lock().await;
            if !self.persist_allowed || !self.available || inner.store.is_none() {
                Vec::new()
            } else {
                inner
                    .definitions
                    .values()
                    .filter(|definition| {
                        definition.persistent()
                            && definition.enabled()
                            && definition.desired()
                            && definition.object_pinned
                            && definition.phase != EXT_PHASE_BLOCKED
                    })
                    .map(|definition| (definition.extension_id, definition.next_start_unix_ms))
                    .collect()
            }
        };
        let now = unix_millis_now();
        for (id, next_start_unix_ms) in definitions {
            let delay = Duration::from_millis(next_start_unix_ms.saturating_sub(now));
            if delay.is_zero() {
                self.ensure_supervisor(state.clone(), id).await;
            } else {
                let service = Arc::clone(self);
                let state = state.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    service.ensure_supervisor(state, id).await;
                });
            }
        }
    }

    async fn maintenance_loop(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let _store_io = self.store_io.lock().await;
            let (cleanups, retries, snapshot) = {
                let mut inner = self.inner.lock().await;
                if inner.shutting_down {
                    return;
                }
                let now = Instant::now();
                let (expired_uploads, cleanups) = inner
                    .store
                    .as_mut()
                    .map(|store| store.take_expired_uploads(now))
                    .unwrap_or_default();
                notify_need_object_locked(&mut inner, &expired_uploads, self.output_retain_max);
                expire_pending_locked(
                    &mut inner,
                    now,
                    self.output_retain_max,
                    self.terminal_retain,
                );
                release_expired_pending_locked(&mut inner, now);
                let snapshot = inner
                    .store
                    .as_ref()
                    .and_then(|store| store.lru_snapshot().ok().flatten());
                let retries = inner
                    .store
                    .as_ref()
                    .map(ObjectStore::cleanup_retries)
                    .unwrap_or_default();
                (cleanups, retries, snapshot)
            };
            let cleanup_results = if cleanups.is_empty() {
                Vec::new()
            } else {
                tokio::task::spawn_blocking(move || {
                    cleanups
                        .into_iter()
                        .map(|cleanup| cleanup.finish())
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default()
            };
            let retry_results = if retries.is_empty() {
                Vec::new()
            } else {
                tokio::task::spawn_blocking(move || {
                    retries
                        .into_iter()
                        .map(|retry| retry.finish())
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default()
            };
            let (snapshot, persisted) = if let Some(snapshot) = snapshot {
                tokio::task::spawn_blocking(move || {
                    let persisted = snapshot.persist().is_ok();
                    (Some(snapshot), persisted)
                })
                .await
                .unwrap_or((None, false))
            } else {
                (None, false)
            };
            if !cleanup_results.is_empty() || !retry_results.is_empty() || persisted {
                let mut inner = self.inner.lock().await;
                if let Some(store) = inner.store.as_mut() {
                    for cleanup in cleanup_results {
                        store.commit_upload_cleanup(cleanup);
                    }
                    for retry in retry_results {
                        store.commit_cleanup_retry(retry);
                    }
                    if persisted && let Some(snapshot) = snapshot.as_ref() {
                        store.acknowledge_lru_snapshot(snapshot);
                    }
                }
            }
        }
    }

    /// Publish the extension shutdown barrier before global connection
    /// cancellation. This closes the restart/accounting race while allowing
    /// the caller to cancel every connection before waiting for supervisors.
    pub(crate) async fn begin_shutdown(&self) {
        let (controls, wakes) = {
            let mut inner = self.inner.lock().await;
            inner.shutting_down = true;
            let controls = inner
                .definitions
                .values_mut()
                .filter_map(|definition| {
                    definition.interrupt = Some(Interrupt::ServerShutdown);
                    definition.control.clone()
                })
                .collect::<Vec<_>>();
            let wakes = inner
                .definitions
                .values()
                .map(|definition| Arc::clone(&definition.wake))
                .collect::<Vec<_>>();
            (controls, wakes)
        };
        for wake in wakes {
            wake.notify_waiters();
        }
        for control in controls {
            control.connection.cancel();
            control.host.cancel();
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.begin_shutdown().await;
        loop {
            if self.inner.lock().await.supervisors.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_run(
        self: &Arc<Self>,
        state: super::AppState,
        endpoint: u64,
        nonce: u16,
        run_flags: u8,
        restart: u8,
        expected_id: u64,
        expected_revision: u64,
        expected_generation: Option<u64>,
        hash: ObjectHash,
        name: &str,
        arguments: Vec<&[u8]>,
        native: Option<NativeRunOptions>,
        admitted_validation: Option<OwnedSemaphorePermit>,
    ) {
        let update = run_flags & EXT_RUN_UPDATE != 0;
        let persistent = run_flags & EXT_RUN_PERSIST != 0;
        if let Some(native) = native
            && (native.flags & !EXT_FLAGS != 0
                || native.flags & EXT_FLAG_PERSIST != u8::from(persistent) * EXT_FLAG_PERSIST
                || native.flags & EXT_FLAG_DETACH
                    != u8::from(run_flags & EXT_RUN_DETACH != 0) * EXT_FLAG_DETACH
                || native.runtime > yas_wire::schema::extension::RUNTIME_QUICKJS as u8)
        {
            self.send(
                endpoint,
                run_error_status(
                    nonce,
                    EXT_STATUS_INVALID,
                    hash,
                    "native Extension deployment options are invalid",
                ),
            )
            .await;
            return;
        }
        if persistent && !self.persist_allowed {
            self.send(
                endpoint,
                run_error_status(
                    nonce,
                    EXT_STATUS_PERMISSION,
                    hash,
                    "persistent extensions are disabled on this server",
                ),
            )
            .await;
            return;
        }
        if persistent
            && arguments
                .iter()
                .any(|arg| std::str::from_utf8(arg).is_err())
        {
            self.send(
                endpoint,
                run_error_status(
                    nonce,
                    EXT_STATUS_INVALID,
                    hash,
                    "persistent extension arguments must be UTF-8",
                ),
            )
            .await;
            return;
        }
        let argument_charge = encoded_borrowed_argument_bytes(&arguments);
        let Some(argument_reservation) = self.argument_budget.try_reserve(argument_charge) else {
            self.send(
                endpoint,
                run_error_status(
                    nonce,
                    EXT_STATUS_BUDGET,
                    hash,
                    "extension argument store is full",
                ),
            )
            .await;
            return;
        };
        // Admission precedes the only borrowed-to-owned argument copy. The
        // same guard is transferred into a resident definition, so this path
        // never reserves the argument bytes a second time. Extension-origin
        // request bytes remain charged by the service argument budget.
        let mut argument_reservation = Some(argument_reservation);
        let args = arguments
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>();
        let _validation = match admitted_validation {
            Some(permit) => permit,
            None => match self.validating.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    self.send(
                        endpoint,
                        run_error_status(
                            nonce,
                            EXT_STATUS_OTHER,
                            hash,
                            "extension validation service is unavailable",
                        ),
                    )
                    .await;
                    return;
                }
            },
        };
        let (object_read, durability_error) = match self.probe_object(hash, persistent).await {
            ObjectProbe::Hit(read) => (Some(read), None),
            ObjectProbe::Miss => (None, None),
            ObjectProbe::Durability(error) => (None, Some(error)),
        };
        let object_hit = object_read.is_some();
        let _catalog_io = if persistent || update {
            Some(self.catalog_io.lock().await)
        } else {
            None
        };

        let mut start = None;
        let mut cancel = None;
        let mut created = None;
        let mut emit_after_reply = None;
        let response;
        {
            let mut inner = self.inner.lock().await;
            if inner.shutting_down {
                response =
                    run_error_status(nonce, EXT_STATUS_OTHER, hash, "server is shutting down");
            } else if update {
                let Some(current) = inner
                    .definitions
                    .values()
                    .find(|definition| definition.persistent() && definition.name == name)
                    .cloned()
                else {
                    response = run_error_status(
                        nonce,
                        EXT_STATUS_NOT_FOUND,
                        hash,
                        "persistent extension name does not exist",
                    );
                    drop(inner);
                    self.send(endpoint, response).await;
                    return;
                };
                if current.extension_id != expected_id
                    || expected_generation.is_some_and(|value| current.generation != value)
                    || current.definition_revision != expected_revision
                {
                    response = status_event(
                        &current,
                        nonce,
                        EXT_STATUS_CONFLICT,
                        None,
                        "extension definition changed",
                    );
                } else if let Some(error) = durability_error.as_ref() {
                    response =
                        status_event(&current, nonce, EXT_STATUS_OTHER, None, &error.to_string());
                } else if !object_hit {
                    response = update_operation_status(
                        &current,
                        nonce,
                        EXT_STATUS_OK,
                        EXT_PHASE_NEED_OBJECT,
                        hash,
                        restart,
                        "module upload required",
                    );
                } else {
                    let current_arguments = if current.args.is_some() {
                        current.args.clone().ok_or(CatalogError::Unavailable)
                    } else {
                        drop(inner);
                        let loaded = self.definition_arguments(&current).await;
                        inner = self.inner.lock().await;
                        loaded
                    };
                    if let Err(error) = &current_arguments {
                        response = status_event(
                            &current,
                            nonce,
                            catalog_status(error),
                            None,
                            &error.to_string(),
                        );
                    } else if current.hash == hash
                        && current_arguments
                            .as_ref()
                            .is_ok_and(|stored| stored == &args)
                        && current.restart == restart
                        && native.is_none_or(|options| {
                            current.flags == options.flags
                                && current.native_runtime == options.runtime
                        })
                    {
                        match repair_persistent_pin(&mut inner, &current) {
                            Ok(()) => {
                                let current_id = current.extension_id;
                                drop(inner);
                                let cleared = self
                                    .catalog_call(move |catalog| {
                                        catalog.set_lifecycle(
                                            current_id,
                                            None,
                                            None,
                                            None,
                                            None,
                                            None,
                                            Some(0),
                                            Some(BlockedState::Clear),
                                        )
                                    })
                                    .await;
                                inner = self.inner.lock().await;
                                match cleared {
                                    Ok(_) => {
                                        let shutting_down = inner.shutting_down;
                                        if let Some(definition) =
                                            inner.definitions.get_mut(&current.extension_id)
                                        {
                                            if definition.phase == EXT_PHASE_BLOCKED {
                                                definition.detail.clear();
                                                definition.generation =
                                                    definition.generation.saturating_add(1);
                                                definition.wake.notify_waiters();
                                                if shutting_down {
                                                    definition.phase = EXT_PHASE_STOPPED;
                                                } else if definition.enabled()
                                                    && definition.desired()
                                                {
                                                    definition.phase = EXT_PHASE_QUEUED;
                                                    start = Some(definition.extension_id);
                                                } else {
                                                    definition.phase = EXT_PHASE_STOPPED;
                                                }
                                                emit_after_reply = Some(definition.extension_id);
                                            }
                                            response = update_operation_status(
                                                definition,
                                                nonce,
                                                EXT_STATUS_OK,
                                                0,
                                                definition.hash,
                                                definition.restart,
                                                "extension definition is unchanged",
                                            );
                                        } else {
                                            response = run_error_status(
                                                nonce,
                                                EXT_STATUS_OTHER,
                                                hash,
                                                "extension disappeared during update",
                                            );
                                        }
                                    }
                                    Err(error) => {
                                        response = status_event(
                                            &current,
                                            nonce,
                                            catalog_status(&error),
                                            None,
                                            &error.to_string(),
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                response = status_event(
                                    &current,
                                    nonce,
                                    catalog_status(&error),
                                    None,
                                    &error.to_string(),
                                );
                            }
                        }
                    } else {
                        drop(inner);
                        let updated = self
                            .commit_catalog_update(
                                &current,
                                hash,
                                args,
                                restart,
                                native.map_or(current.flags, |options| options.flags),
                            )
                            .await;
                        inner = self.inner.lock().await;
                        match updated {
                            Ok(updated) => {
                                let shutting_down = inner.shutting_down;
                                if let Some(definition) = inner.definitions.get_mut(&expected_id) {
                                    definition.definition_revision = updated.definition_revision;
                                    definition.hash = hash;
                                    release_definition_arguments(definition);
                                    definition.argument_bytes = updated.argument_bytes;
                                    definition.restart = restart;
                                    definition.flags = updated.flags;
                                    if let Some(options) = native {
                                        definition.native_runtime = options.runtime;
                                    }
                                    definition.object_pinned = true;
                                    definition.generation = definition.generation.saturating_add(1);
                                    definition.failure_count = 0;
                                    definition.interrupt = Some(Interrupt::Updated);
                                    definition.detail.clear();
                                    definition.pending_deadline = None;
                                    definition.next_start_unix_ms = 0;
                                    definition.wake.notify_waiters();
                                    cancel = definition.control.clone();
                                    if cancel.is_some() {
                                        definition.phase = EXT_PHASE_STOPPING;
                                        definition.task_id = 0;
                                    } else if shutting_down {
                                        definition.phase = EXT_PHASE_STOPPED;
                                    } else if definition.enabled() && definition.desired() {
                                        definition.phase = EXT_PHASE_QUEUED;
                                        start = Some(expected_id);
                                    } else {
                                        definition.phase = EXT_PHASE_STOPPED;
                                    }
                                    response = update_operation_status(
                                        definition,
                                        nonce,
                                        EXT_STATUS_OK,
                                        0,
                                        definition.hash,
                                        definition.restart,
                                        "",
                                    );
                                    emit_after_reply = Some(expected_id);
                                } else {
                                    response = run_error_status(
                                        nonce,
                                        EXT_STATUS_OTHER,
                                        hash,
                                        "extension disappeared during update",
                                    );
                                }
                            }
                            Err(error) => {
                                response = status_event(
                                    &current,
                                    nonce,
                                    catalog_status(&error),
                                    None,
                                    &error.to_string(),
                                );
                            }
                        }
                    }
                }
            } else {
                let transient_count = inner
                    .definitions
                    .values()
                    .filter(|definition| !definition.persistent())
                    .count();
                let persistent_count = inner
                    .definitions
                    .values()
                    .filter(|definition| definition.persistent())
                    .count();
                let name_conflict = inner.definitions.values().any(|definition| {
                    definition.name == name
                        && (native.is_some() || persistent && definition.persistent())
                });
                let follow_creator = native.is_none_or(|options| options.follow_creator);
                let follower_capacity = !follow_creator
                    || follower_capacity_available(
                        &inner,
                        endpoint,
                        self.follow_max_per_endpoint,
                        self.follow_max,
                    );
                if name_conflict {
                    response = run_error_status(
                        nonce,
                        EXT_STATUS_CONFLICT,
                        hash,
                        "persistent extension name already exists",
                    );
                } else if (!persistent && transient_count >= self.max_transient)
                    || (persistent && persistent_count >= self.max_persistent)
                {
                    response = run_error_status(
                        nonce,
                        EXT_STATUS_BUDGET,
                        hash,
                        "extension supervisor capacity exhausted",
                    );
                } else if !follower_capacity {
                    response = run_error_status(
                        nonce,
                        EXT_STATUS_BUDGET,
                        hash,
                        "extension follower capacity exhausted",
                    );
                } else if let Some(error) = durability_error.as_ref() {
                    response = run_error_status(nonce, EXT_STATUS_OTHER, hash, &error.to_string());
                } else {
                    // The reservation exists before argument cloning and ID
                    // admission. Persistent cache hits drop it again as soon
                    // as their redb transaction commits.
                    let Some(extension_id) = allocate_extension_id(&inner) else {
                        response = run_error_status(
                            nonce,
                            EXT_STATUS_BUDGET,
                            hash,
                            "could not allocate an extension ID",
                        );
                        drop(inner);
                        self.send(endpoint, response).await;
                        return;
                    };
                    let hit = object_hit;
                    let flags = native.map_or_else(
                        || {
                            (u8::from(run_flags & EXT_RUN_DETACH != 0) * EXT_FLAG_DETACH)
                                | (u8::from(persistent) * EXT_FLAG_PERSIST)
                                | EXT_FLAG_ENABLED
                                | EXT_FLAG_DESIRED_RUNNING
                        },
                        |options| options.flags,
                    );
                    let mut definition = Definition {
                        extension_id,
                        definition_revision: 1,
                        flags,
                        restart,
                        native_runtime: native
                            .map_or(yas_wire::schema::extension::RUNTIME_AUTO as u8, |options| {
                                options.runtime
                            }),
                        hash,
                        name: name.to_owned(),
                        args: Some(args),
                        argument_bytes: argument_charge,
                        argument_reservation: argument_reservation.take(),
                        owner_endpoint: (run_flags & EXT_RUN_DETACH == 0).then_some(endpoint),
                        phase: if hit
                            && flags & (EXT_FLAG_ENABLED | EXT_FLAG_DESIRED_RUNNING)
                                == EXT_FLAG_ENABLED | EXT_FLAG_DESIRED_RUNNING
                        {
                            EXT_PHASE_QUEUED
                        } else if hit {
                            EXT_PHASE_STOPPED
                        } else {
                            EXT_PHASE_NEED_OBJECT
                        },
                        attempt: 0,
                        last_running_attempt: 0,
                        task_id: 0,
                        next_start_unix_ms: 0,
                        detail: if hit {
                            String::new()
                        } else {
                            "module upload required".into()
                        },
                        next_output_sequence: 1,
                        retained: VecDeque::new(),
                        terminal_replay: VecDeque::new(),
                        retained_bytes: 0,
                        followers: HashMap::new(),
                        pending_deadline: (!hit).then_some(Instant::now() + self.pending_timeout),
                        release_deadline: None,
                        generation: 1,
                        failure_count: 0,
                        interrupt: None,
                        control: None,
                        object_pinned: false,
                        catalog_committed: false,
                        wake: Arc::new(Notify::new()),
                    };
                    if follow_creator {
                        definition.followers.insert(
                            endpoint,
                            FollowerCursor {
                                next_sequence: 1,
                                replay_through: Some(0),
                            },
                        );
                    }
                    let admitted = if hit && persistent {
                        drop(inner);
                        let committed = self.commit_catalog_create(&definition).await;
                        inner = self.inner.lock().await;
                        committed.map(|persistent| {
                            definition.definition_revision = persistent.definition_revision;
                            definition.flags = persistent.flags;
                            definition.argument_bytes = persistent.argument_bytes;
                            definition.catalog_committed = true;
                            definition.object_pinned = true;
                            release_definition_arguments(&mut definition);
                        })
                    } else if hit {
                        commit_transient_create(&mut inner, &mut definition)
                    } else {
                        Ok(())
                    };
                    match admitted {
                        Ok(()) => {
                            if hit && inner.shutting_down {
                                definition.phase = EXT_PHASE_STOPPED;
                            }
                            let should_start = hit
                                && !inner.shutting_down
                                && definition.enabled()
                                && definition.desired();
                            response = creation_status(&definition, nonce, &definition.detail);
                            created = Some(extension_id);
                            inner.definitions.insert(extension_id, definition);
                            if should_start {
                                start = Some(extension_id);
                            }
                        }
                        Err((status, detail)) => {
                            release_definition_arguments(&mut definition);
                            response = run_error_status(nonce, status, hash, &detail);
                        }
                    }
                }
            }
            try_deliver_endpoint_locked(&mut inner, endpoint, response);
            if created.is_some() {
                wake_endpoint_locked(&inner, endpoint);
            }
            if let Some(extension_id) = emit_after_reply {
                emit_lifecycle_locked(&mut inner, extension_id, self.output_retain_max);
            }
        }
        drop(_catalog_io);
        if let Some(control) = cancel {
            control.connection.cancel();
            control.host.cancel();
        }
        if let Some(extension_id) = start {
            self.ensure_supervisor(state, extension_id).await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_put(
        self: &Arc<Self>,
        state: super::AppState,
        endpoint: u64,
        nonce: u16,
        flags: u8,
        hash: ObjectHash,
        offset: u64,
        total_size: u64,
        data: &[u8],
        admitted_validation: Option<OwnedSemaphorePermit>,
    ) {
        let validation_request =
            if flags & (EXT_PUT_BEGIN | EXT_PUT_FINAL) != 0 && admitted_validation.is_none() {
                self.validation_request_budget.try_reserve(data.len())
            } else {
                None
            };
        if flags & (EXT_PUT_BEGIN | EXT_PUT_FINAL) != 0
            && admitted_validation.is_none()
            && validation_request.is_none()
        {
            let mut inner = self.inner.lock().await;
            try_deliver_endpoint_locked(
                &mut inner,
                endpoint,
                put_status(
                    nonce,
                    EXT_STATUS_BUDGET,
                    hash,
                    0,
                    "extension validation request budget exhausted",
                ),
            );
            return;
        }
        let _validation_request = validation_request;
        let _validation = if flags & (EXT_PUT_BEGIN | EXT_PUT_FINAL) != 0 {
            match admitted_validation {
                Some(permit) => Some(permit),
                None => self.validating.clone().acquire_owned().await.ok(),
            }
        } else {
            debug_assert!(admitted_validation.is_none());
            None
        };
        let _begin_read = if flags & EXT_PUT_BEGIN != 0 {
            match self.probe_object(hash, false).await {
                ObjectProbe::Hit(read) => Some(read),
                ObjectProbe::Miss | ObjectProbe::Durability(_) => None,
            }
        } else {
            None
        };
        // Upload tokens temporarily leave ObjectStore while their file work
        // runs. Serialize those transitions without occupying `inner`; status,
        // control, output, and cancellation remain independently available.
        let _store_io = self.store_io.lock().await;
        let begin = if flags & EXT_PUT_BEGIN != 0 {
            if let Err(error) = self.persist_store_lru_in_lane().await {
                Some(Err(error))
            } else {
                Some(loop {
                    let prepared = {
                        let mut inner = self.inner.lock().await;
                        let Some(store) = inner.store.as_mut() else {
                            break Err(ObjectStoreError::NotFound);
                        };
                        store.prepare_begin_upload_after_probe(
                            endpoint,
                            hash,
                            total_size,
                            Instant::now(),
                        )
                    };
                    match prepared {
                        Ok(PreparedBeginUpload::Complete(result)) => break Ok(result),
                        Ok(PreparedBeginUpload::Evict(eviction)) => {
                            let evicted = tokio::task::block_in_place(|| {
                                self.before_storage_io();
                                (*eviction).finish()
                            });
                            let committed = {
                                let mut inner = self.inner.lock().await;
                                inner
                                    .store
                                    .as_mut()
                                    .ok_or(ObjectStoreError::NotFound)
                                    .and_then(|store| store.commit_eviction(evicted))
                            };
                            if let Err(error) = committed {
                                break Err(error);
                            }
                            if let Err(error) = self.persist_store_lru_in_lane().await {
                                break Err(error);
                            }
                        }
                        Ok(PreparedBeginUpload::Create(creation)) => {
                            let created = tokio::task::block_in_place(|| {
                                self.before_storage_io();
                                (*creation).finish()
                            });
                            let committed = {
                                let mut inner = self.inner.lock().await;
                                inner
                                    .store
                                    .as_mut()
                                    .map(|store| store.commit_upload_creation(created))
                            };
                            break match committed {
                                Some(UploadCreationCommit::Complete(result)) => result,
                                Some(UploadCreationCommit::Stale(stale)) => {
                                    let cleanup =
                                        tokio::task::block_in_place(|| (*stale).cleanup());
                                    let mut inner = self.inner.lock().await;
                                    if let Some(store) = inner.store.as_mut() {
                                        store.commit_upload_cleanup(cleanup);
                                    }
                                    Err(ObjectStoreError::Conflict)
                                }
                                None => Err(ObjectStoreError::NotFound),
                            };
                        }
                        Err(error) => break Err(error),
                    }
                })
            }
        } else {
            None
        };

        let result = match begin {
            Some(Ok(BeginUpload::AlreadyHave { size })) => Ok(PutChunk::AlreadyHave { size }),
            Some(Err(error)) => Err(error),
            Some(Ok(BeginUpload::Started)) | None => {
                let prepared = {
                    let mut inner = self.inner.lock().await;
                    inner
                        .store
                        .as_mut()
                        .ok_or(ObjectStoreError::NotFound)
                        .and_then(|store| {
                            store.prepare_put_chunk(
                                endpoint,
                                hash,
                                offset,
                                total_size,
                                data,
                                flags & EXT_PUT_FINAL != 0,
                                Instant::now(),
                            )
                        })
                };
                match prepared {
                    Ok(PreparedPut::Complete(result)) => Ok(result),
                    Ok(PreparedPut::Abort(cleanup, error)) => {
                        let cleaned = tokio::task::block_in_place(|| {
                            self.before_storage_io();
                            (*cleanup).finish()
                        });
                        let mut inner = self.inner.lock().await;
                        if let Some(store) = inner.store.as_mut() {
                            store.commit_upload_cleanup(cleaned);
                        }
                        Err(error)
                    }
                    Ok(PreparedPut::Chunk(upload)) => {
                        let completed = tokio::task::block_in_place(|| {
                            self.before_storage_io();
                            (*upload).finish(data)
                        });
                        let committed = {
                            let mut inner = self.inner.lock().await;
                            inner
                                .store
                                .as_mut()
                                .map(|store| store.commit_chunk_upload(completed))
                        };
                        match committed {
                            Some(ChunkUploadCommit::Complete(result)) => result,
                            Some(ChunkUploadCommit::Stale(stale)) => {
                                let cleanup = tokio::task::block_in_place(|| (*stale).cleanup());
                                let mut inner = self.inner.lock().await;
                                if let Some(store) = inner.store.as_mut() {
                                    store.commit_upload_cleanup(cleanup);
                                }
                                Err(ObjectStoreError::Conflict)
                            }
                            None => Err(ObjectStoreError::NotFound),
                        }
                    }
                    Ok(PreparedPut::Final(upload)) => {
                        let finalized = tokio::task::block_in_place(|| {
                            self.before_storage_io();
                            (*upload).finish(data, |module| self.validate_module(module))
                        });
                        let committed = {
                            let mut inner = self.inner.lock().await;
                            inner
                                .store
                                .as_mut()
                                .ok_or(ObjectStoreError::NotFound)
                                .and_then(|store| store.commit_final_upload(finalized))
                        };
                        match committed {
                            Ok(result) => self.persist_store_lru_in_lane().await.map(|()| result),
                            Err(error) => Err(error),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
        };
        let result = match result {
            Ok(result @ PutChunk::AlreadyHave { .. }) => {
                self.persist_store_lru_in_lane().await.map(|()| result)
            }
            result => result,
        };
        let start = self.apply_put_result(endpoint, nonce, hash, result).await;
        for extension_id in start {
            self.ensure_supervisor(state.clone(), extension_id).await;
        }
    }

    async fn apply_put_result(
        &self,
        endpoint: u64,
        nonce: u16,
        hash: ObjectHash,
        result: Result<PutChunk, ObjectStoreError>,
    ) -> Vec<u64> {
        let (status, received, detail, start, transitioned, notify_need_object) = match result {
            Ok(PutChunk::Accepted { received }) => (
                EXT_STATUS_OK,
                received,
                String::new(),
                Vec::new(),
                Vec::new(),
                false,
            ),
            Ok(PutChunk::Committed { size }) => {
                let (start, transitioned) = self.complete_pending(hash).await;
                (
                    EXT_STATUS_OK,
                    size,
                    String::new(),
                    start,
                    transitioned,
                    false,
                )
            }
            Ok(PutChunk::AlreadyHave { size }) => {
                let (start, transitioned) = self.complete_pending(hash).await;
                (
                    EXT_PUT_ALREADY_HAVE,
                    size,
                    "module already exists".into(),
                    start,
                    transitioned,
                    false,
                )
            }
            Err(error) => {
                let status = object_status(&error);
                let detail = error.to_string();
                let mut inner = self.inner.lock().await;
                let transitioned = if matches!(error, ObjectStoreError::InvalidModule(_)) {
                    stop_invalid_pending_locked(&mut inner, hash, &detail, self.terminal_retain)
                } else {
                    Vec::new()
                };
                (
                    status,
                    0,
                    detail,
                    Vec::new(),
                    transitioned,
                    !matches!(
                        error,
                        ObjectStoreError::Conflict | ObjectStoreError::InvalidModule(_)
                    ),
                )
            }
        };
        let mut inner = self.inner.lock().await;
        try_deliver_endpoint_locked(
            &mut inner,
            endpoint,
            put_status(nonce, status, hash, received, &detail),
        );
        for extension_id in transitioned {
            emit_lifecycle_locked(&mut inner, extension_id, self.output_retain_max);
        }
        if notify_need_object {
            notify_need_object_locked(&mut inner, &[hash], self.output_retain_max);
        }
        start
    }

    async fn complete_pending(&self, hash: ObjectHash) -> (Vec<u64>, Vec<u64>) {
        let ids = {
            let inner = self.inner.lock().await;
            inner
                .definitions
                .values()
                .filter(|definition| {
                    definition.hash == hash && definition.phase == EXT_PHASE_NEED_OBJECT
                })
                .map(|definition| definition.extension_id)
                .collect::<Vec<_>>()
        };
        let has_persistent = {
            let inner = self.inner.lock().await;
            ids.iter().any(|extension_id| {
                inner
                    .definitions
                    .get(extension_id)
                    .is_some_and(Definition::persistent)
            })
        };
        let _catalog_io = if has_persistent {
            Some(self.catalog_io.lock().await)
        } else {
            None
        };
        let mut start = Vec::new();
        let mut changed = Vec::new();
        for extension_id in ids {
            let snapshot = {
                let inner = self.inner.lock().await;
                inner.definitions.get(&extension_id).cloned()
            };
            let Some(snapshot) = snapshot else {
                continue;
            };
            if snapshot.phase != EXT_PHASE_NEED_OBJECT || snapshot.hash != hash {
                continue;
            }
            if snapshot
                .pending_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                let mut inner = self.inner.lock().await;
                if let Some(definition) = inner.definitions.get_mut(&extension_id) {
                    definition.phase = EXT_PHASE_STOPPED;
                    definition.set_flag(EXT_FLAG_DESIRED_RUNNING, false);
                    definition.detail = "pending extension creation expired".into();
                    definition.pending_deadline = None;
                    definition.release_deadline = Some(Instant::now() + self.terminal_retain);
                    release_definition_arguments(definition);
                    changed.push(extension_id);
                }
                continue;
            }

            let admitted = if snapshot.persistent() {
                self.commit_catalog_create(&snapshot).await.map(Some)
            } else {
                let mut inner = self.inner.lock().await;
                let Some(mut definition) = inner.definitions.remove(&extension_id) else {
                    continue;
                };
                let admitted = commit_transient_create(&mut inner, &mut definition).map(|()| None);
                inner.definitions.insert(extension_id, definition);
                admitted
            };
            let mut inner = self.inner.lock().await;
            let shutting_down = inner.shutting_down;
            let Some(definition) = inner.definitions.get_mut(&extension_id) else {
                continue;
            };
            match admitted {
                Ok(persistent) => {
                    if let Some(persistent) = persistent {
                        definition.definition_revision = persistent.definition_revision;
                        definition.flags = persistent.flags;
                        definition.argument_bytes = persistent.argument_bytes;
                        definition.catalog_committed = true;
                        definition.object_pinned = true;
                        release_definition_arguments(definition);
                    }
                    definition.phase = if shutting_down {
                        EXT_PHASE_STOPPED
                    } else {
                        EXT_PHASE_QUEUED
                    };
                    definition.pending_deadline = None;
                    definition.release_deadline = None;
                    definition.detail.clear();
                    if !shutting_down {
                        start.push(extension_id);
                    }
                }
                Err((_, detail)) => {
                    definition.phase = EXT_PHASE_STOPPED;
                    definition.pending_deadline = None;
                    definition.release_deadline = Some(Instant::now() + self.terminal_retain);
                    definition.set_flag(EXT_FLAG_DESIRED_RUNNING, false);
                    definition.detail = bounded_detail(&detail);
                    release_definition_arguments(definition);
                }
            }
            changed.push(extension_id);
        }
        (start, changed)
    }

    /// Persist the latest complete LRU image without holding service state.
    /// Callers serialize this with `store_io` so eviction cannot pass a newer
    /// in-memory touch while an older snapshot is being published.
    async fn persist_store_lru(&self) -> Result<(), ObjectStoreError> {
        let _store_io = self.store_io.lock().await;
        self.persist_store_lru_in_lane().await
    }

    async fn persist_store_lru_in_lane(&self) -> Result<(), ObjectStoreError> {
        let snapshot = {
            let inner = self.inner.lock().await;
            inner
                .store
                .as_ref()
                .ok_or(ObjectStoreError::NotFound)?
                .lru_snapshot()?
        };
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        let (snapshot, persisted) = tokio::task::spawn_blocking(move || {
            let persisted = snapshot.persist();
            (snapshot, persisted)
        })
        .await
        .map_err(|error| ObjectStoreError::Io(std::io::Error::other(error.to_string())))?;
        persisted?;
        let mut inner = self.inner.lock().await;
        inner
            .store
            .as_mut()
            .ok_or(ObjectStoreError::NotFound)?
            .acknowledge_lru_snapshot(&snapshot);
        Ok(())
    }

    async fn handle_control(
        self: &Arc<Self>,
        state: super::AppState,
        endpoint: u64,
        nonce: u16,
        extension_id: u64,
        action: u8,
    ) {
        if matches!(
            action,
            EXT_CONTROL_CANCEL
                | EXT_CONTROL_RESTART
                | EXT_CONTROL_ENABLE
                | EXT_CONTROL_DISABLE
                | EXT_CONTROL_REMOVE
        ) {
            self.handle_mutating_control(state, endpoint, nonce, extension_id, action)
                .await;
            return;
        }
        let mut events = Vec::new();
        {
            let mut inner = self.inner.lock().await;
            if action != EXT_CONTROL_LIST {
                let Some(current) = inner.definitions.get(&extension_id).cloned() else {
                    let event = fixed_status(
                        nonce,
                        EXT_STATUS_UNKNOWN_ID,
                        0,
                        0,
                        extension_id,
                        0,
                        [0; 32],
                        "extension ID does not exist",
                    );
                    try_deliver_endpoint_locked(&mut inner, endpoint, event);
                    return;
                };
                match action {
                    EXT_CONTROL_STATUS => events.push(status_event(
                        &current,
                        nonce,
                        EXT_STATUS_OK,
                        None,
                        &current.detail,
                    )),
                    EXT_CONTROL_ATTACH => {
                        let already_following = current.followers.contains_key(&endpoint);
                        if !already_following
                            && !follower_capacity_available(
                                &inner,
                                endpoint,
                                self.follow_max_per_endpoint,
                                self.follow_max,
                            )
                        {
                            events.push(status_event(
                                &current,
                                nonce,
                                EXT_STATUS_BUDGET,
                                None,
                                "extension follower capacity exhausted",
                            ));
                        } else if let Some(definition) = inner.definitions.get_mut(&extension_id) {
                            let oldest = oldest_replay_sequence(definition);
                            let cursor = definition
                                .followers
                                .get(&endpoint)
                                .map(|follower| follower.next_sequence)
                                .unwrap_or(oldest)
                                .max(oldest);
                            let through = definition.latest_output_sequence();
                            let replay_from =
                                next_replay_sequence(definition, cursor, through).unwrap_or(0);
                            let replay_through = definition
                                .followers
                                .get(&endpoint)
                                .and_then(|follower| follower.replay_through)
                                .map_or(through, |pending| pending.max(through));
                            definition.followers.insert(
                                endpoint,
                                FollowerCursor {
                                    next_sequence: cursor,
                                    replay_through: Some(replay_through),
                                },
                            );
                            events.push(attach_status_event(
                                definition,
                                nonce,
                                replay_from,
                                &definition.detail,
                            ));
                        }
                    }
                    EXT_CONTROL_UNFOLLOW => {
                        if let Some(definition) = inner.definitions.get_mut(&extension_id) {
                            definition.followers.remove(&endpoint);
                        }
                        events.push(status_event(&current, nonce, EXT_STATUS_OK, None, ""));
                    }
                    _ => events.push(status_event(
                        &current,
                        nonce,
                        EXT_STATUS_INVALID,
                        None,
                        "unknown extension control action",
                    )),
                }
            } else {
                events.push(fixed_status(
                    nonce,
                    EXT_STATUS_INVALID,
                    0,
                    0,
                    0,
                    0,
                    [0; 32],
                    "catalogue listing uses the native snapshot API",
                ));
            }
            for event in events.drain(..) {
                if !try_deliver_endpoint_locked(&mut inner, endpoint, event) {
                    break;
                }
            }
            if action == EXT_CONTROL_ATTACH {
                wake_endpoint_locked(&inner, endpoint);
            }
        }
    }

    async fn handle_mutating_control(
        self: &Arc<Self>,
        state: super::AppState,
        endpoint: u64,
        nonce: u16,
        extension_id: u64,
        action: u8,
    ) {
        let initial = {
            let inner = self.inner.lock().await;
            inner.definitions.get(&extension_id).cloned()
        };
        let Some(initial) = initial else {
            self.send(
                endpoint,
                fixed_status(
                    nonce,
                    EXT_STATUS_UNKNOWN_ID,
                    0,
                    0,
                    extension_id,
                    0,
                    [0; 32],
                    "extension ID does not exist",
                ),
            )
            .await;
            return;
        };
        let serialize_catalog = initial.persistent();
        let _catalog_io = if serialize_catalog {
            Some(self.catalog_io.lock().await)
        } else {
            None
        };

        let current = {
            let mut inner = self.inner.lock().await;
            let Some(current) = inner.definitions.get(&extension_id).cloned() else {
                drop(inner);
                self.send(
                    endpoint,
                    fixed_status(
                        nonce,
                        EXT_STATUS_UNKNOWN_ID,
                        0,
                        0,
                        extension_id,
                        0,
                        [0; 32],
                        "extension ID does not exist",
                    ),
                )
                .await;
                return;
            };
            let invalid = match action {
                EXT_CONTROL_RESTART if current.persistent() && !self.persist_allowed => Some((
                    EXT_STATUS_PERMISSION,
                    "persistent extensions are disabled on this server",
                )),
                EXT_CONTROL_RESTART
                    if current
                        .owner_endpoint
                        .is_some_and(|owner| !inner.endpoints.contains_key(&owner)) =>
                {
                    Some((
                        EXT_STATUS_CONFLICT,
                        "attached extension owner is no longer connected",
                    ))
                }
                EXT_CONTROL_RESTART if !current.enabled() => {
                    Some((EXT_STATUS_CONFLICT, "extension is disabled"))
                }
                EXT_CONTROL_ENABLE if !current.persistent() || !self.persist_allowed => Some((
                    EXT_STATUS_PERMISSION,
                    "enable requires persistent-extension permission",
                )),
                EXT_CONTROL_DISABLE if !current.persistent() => Some((
                    EXT_STATUS_PERMISSION,
                    "disable requires a persistent extension",
                )),
                EXT_CONTROL_REMOVE if !current.persistent() => Some((
                    EXT_STATUS_PERMISSION,
                    "remove requires a persistent extension",
                )),
                EXT_CONTROL_REMOVE
                    if current.enabled()
                        || current.control.is_some()
                        || !matches!(current.phase, EXT_PHASE_STOPPED | EXT_PHASE_BLOCKED) =>
                {
                    Some((
                        EXT_STATUS_CONFLICT,
                        "extension must be disabled and quiescent before removal",
                    ))
                }
                _ => None,
            };
            if let Some((status, detail)) = invalid {
                let event = status_event(&current, nonce, status, None, detail);
                try_deliver_endpoint_locked(&mut inner, endpoint, event);
                return;
            }
            if matches!(action, EXT_CONTROL_RESTART | EXT_CONTROL_ENABLE)
                && current.persistent()
                && let Err(error) = repair_persistent_pin(&mut inner, &current)
            {
                let event = status_event(
                    &current,
                    nonce,
                    catalog_status(&error),
                    None,
                    &error.to_string(),
                );
                try_deliver_endpoint_locked(&mut inner, endpoint, event);
                return;
            }
            current
        };
        let write_catalog = current.catalog_committed;

        let persisted = if write_catalog {
            if action == EXT_CONTROL_REMOVE {
                self.catalog_call(move |catalog| catalog.remove(extension_id).map(|_| ()))
                    .await
            } else {
                let (enabled, desired) = match action {
                    EXT_CONTROL_CANCEL => (None, Some(false)),
                    EXT_CONTROL_RESTART => (None, Some(true)),
                    EXT_CONTROL_ENABLE => (Some(true), None),
                    EXT_CONTROL_DISABLE => (Some(false), None),
                    _ => unreachable!(),
                };
                self.catalog_call(move |catalog| {
                    catalog
                        .set_lifecycle(
                            extension_id,
                            enabled,
                            desired,
                            None,
                            None,
                            None,
                            Some(0),
                            Some(BlockedState::Clear),
                        )
                        .map(|_| ())
                })
                .await
            }
        } else {
            Ok(())
        };

        let mut cancel = None;
        let mut start = None;
        {
            let mut inner = self.inner.lock().await;
            let persisted_ok = persisted.is_ok();
            let event = match persisted {
                Err(error) => status_event(
                    &current,
                    nonce,
                    catalog_status(&error),
                    None,
                    &error.to_string(),
                ),
                Ok(()) if action == EXT_CONTROL_REMOVE => {
                    remove_definition_locked(&mut inner, extension_id);
                    fixed_status(
                        nonce,
                        EXT_STATUS_OK,
                        0,
                        0,
                        extension_id,
                        0,
                        [0; 32],
                        "removed",
                    )
                }
                Ok(()) => {
                    let (enabled, desired, interrupt) = match action {
                        EXT_CONTROL_CANCEL => (None, Some(false), Interrupt::Cancelled),
                        EXT_CONTROL_RESTART => (None, Some(true), Interrupt::Restarted),
                        EXT_CONTROL_ENABLE => (Some(true), None, Interrupt::Restarted),
                        EXT_CONTROL_DISABLE => (Some(false), None, Interrupt::Disabled),
                        _ => unreachable!(),
                    };
                    if mutate_lifecycle_locked(
                        &mut inner,
                        extension_id,
                        enabled,
                        desired,
                        interrupt,
                        self.terminal_retain,
                    )
                    .is_err()
                    {
                        fixed_status(
                            nonce,
                            EXT_STATUS_UNKNOWN_ID,
                            0,
                            0,
                            extension_id,
                            0,
                            [0; 32],
                            "extension ID does not exist",
                        )
                    } else if let Some(definition) = inner.definitions.get(&extension_id) {
                        cancel = definition.control.clone();
                        if matches!(action, EXT_CONTROL_RESTART | EXT_CONTROL_ENABLE)
                            && definition.enabled()
                            && definition.desired()
                            && cancel.is_none()
                        {
                            start = Some(extension_id);
                        }
                        status_event(definition, nonce, EXT_STATUS_OK, None, "")
                    } else {
                        fixed_status(
                            nonce,
                            EXT_STATUS_UNKNOWN_ID,
                            0,
                            0,
                            extension_id,
                            0,
                            [0; 32],
                            "extension ID does not exist",
                        )
                    }
                }
            };
            try_deliver_endpoint_locked(&mut inner, endpoint, event);
            if persisted_ok && action != EXT_CONTROL_REMOVE {
                emit_lifecycle_locked(&mut inner, extension_id, self.output_retain_max);
            }
        }
        drop(_catalog_io);
        if let Some(control) = cancel {
            control.connection.cancel();
            control.host.cancel();
        }
        if let Some(extension_id) = start {
            self.ensure_supervisor(state, extension_id).await;
        }
    }

    pub(crate) async fn invalidate_command_listener(
        &self,
        endpoint_generation: u64,
        listener: crate::channel::ListenerSnapshot,
    ) {
        self.inner
            .lock()
            .await
            .commands
            .invalidate_listener(&command_listener(endpoint_generation, listener));
    }

    async fn send(&self, endpoint: u64, event: BackendEvent) {
        let mut inner = self.inner.lock().await;
        try_deliver_endpoint_locked(&mut inner, endpoint, event);
    }
}

impl NativeEndpoint {
    pub(crate) fn endpoint(&self) -> u64 {
        self.inner.endpoint
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Publish output from the exact active attempt represented by `context`.
    /// Stale contexts cannot write into a replacement attempt's retained
    /// stream, even when an extension handle has been reused.
    pub(crate) async fn attempt_output(
        &self,
        context: &yas_wire::extension::AttemptContext,
        kind: NativeOutputKind,
        data: &[u8],
    ) -> Result<u64, NativeMutationFailure> {
        self.ensure_open()?;
        self.inner
            .service
            .publish_native_attempt_output(context.into(), kind, data)
            .await
    }

    pub(crate) async fn put(
        &self,
        hash: ObjectHash,
        offset: u64,
        total_size: u64,
        data: &[u8],
        begin: bool,
        final_chunk: bool,
    ) -> Result<NativePutDisposition, NativeMutationFailure> {
        self.ensure_open()?;
        let (nonce, receiver, _pending) = self.reserve_pending()?;
        let mut flags = 0;
        if begin {
            flags |= EXT_PUT_BEGIN;
        }
        if final_chunk {
            flags |= EXT_PUT_FINAL;
        }
        self.inner
            .service
            .handle_put(
                self.inner.app.clone(),
                self.inner.endpoint,
                nonce,
                flags,
                hash,
                offset,
                total_size,
                data,
                None,
            )
            .await;
        match receiver.await.map_err(|_| NativeMutationFailure::Closed)? {
            NativeReply::Put(disposition) => Ok(disposition),
            NativeReply::Error(error) => Err(error),
            NativeReply::Status(_) => Err(NativeMutationFailure::Internal(
                "unexpected Extension object reply".into(),
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run(
        &self,
        restart: u8,
        expected_id: u64,
        expected_generation: u64,
        expected_revision: u64,
        hash: ObjectHash,
        name: &str,
        arguments: Vec<Vec<u8>>,
        options: NativeRunOptions,
    ) -> Result<NativeStatus, NativeMutationFailure> {
        self.ensure_open()?;
        let (nonce, receiver, _pending) = self.reserve_pending()?;
        let arguments = arguments.iter().map(Vec::as_slice).collect();
        self.inner
            .service
            .native_run(
                self.inner.app.clone(),
                self.inner.endpoint,
                nonce,
                restart,
                expected_id,
                expected_generation,
                expected_revision,
                hash,
                name,
                arguments,
                options,
            )
            .await;
        match receiver.await.map_err(|_| NativeMutationFailure::Closed)? {
            NativeReply::Status(status) => Ok(status),
            NativeReply::Error(error) => Err(error),
            NativeReply::Put(_) => Err(NativeMutationFailure::Internal(
                "unexpected Extension deployment reply".into(),
            )),
        }
    }

    pub(crate) async fn control(
        &self,
        extension_handle: u64,
        action: NativeControlAction,
    ) -> Result<NativeStatus, NativeMutationFailure> {
        self.ensure_open()?;
        let (nonce, receiver, _pending) = self.reserve_pending()?;
        let action = match action {
            NativeControlAction::Stop => EXT_CONTROL_CANCEL,
            NativeControlAction::Restart => EXT_CONTROL_RESTART,
            NativeControlAction::Enable => EXT_CONTROL_ENABLE,
            NativeControlAction::Disable => EXT_CONTROL_DISABLE,
            NativeControlAction::Remove => EXT_CONTROL_REMOVE,
            NativeControlAction::Attach => EXT_CONTROL_ATTACH,
        };
        self.inner
            .service
            .handle_control(
                self.inner.app.clone(),
                self.inner.endpoint,
                nonce,
                extension_handle,
                action,
            )
            .await;
        match receiver.await.map_err(|_| NativeMutationFailure::Closed)? {
            NativeReply::Status(status) => Ok(status),
            NativeReply::Error(error) => Err(error),
            NativeReply::Put(_) => Err(NativeMutationFailure::Internal(
                "unexpected Extension control reply".into(),
            )),
        }
    }

    pub(crate) async fn follow(
        &self,
        extension_handle: u64,
        attempt: u64,
        from_sequence: u64,
        queue: usize,
    ) -> Result<NativeFollowStream, NativeMutationFailure> {
        self.ensure_open()?;
        let (sender, receiver) = mpsc::channel(queue.max(1));
        {
            let mut follows = self
                .inner
                .follows
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if follows.contains_key(&extension_handle) {
                return Err(NativeMutationFailure::Conflict);
            }
            follows.insert(
                extension_handle,
                NativeFollowRoute {
                    attempt,
                    from_sequence,
                    sender,
                },
            );
        }
        let status = match self
            .control(extension_handle, NativeControlAction::Attach)
            .await
        {
            Ok(status) => status,
            Err(error) => {
                self.inner
                    .follows
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&extension_handle);
                return Err(error);
            }
        };
        Ok(NativeFollowStream {
            attempt,
            replay_from_sequence: status.replay_from_sequence,
            output_sequence: status.output_sequence,
            extension_handle,
            owner: Arc::downgrade(&self.inner),
            receiver,
        })
    }

    pub(crate) async fn close(&self) {
        close_native_endpoint(&self.inner).await;
    }

    fn ensure_open(&self) -> Result<(), NativeMutationFailure> {
        if self.inner.closed.load(Ordering::Acquire) {
            Err(NativeMutationFailure::Closed)
        } else {
            Ok(())
        }
    }

    fn reserve_pending(
        &self,
    ) -> Result<(u16, oneshot::Receiver<NativeReply>, NativePendingGuard), NativeMutationFailure>
    {
        for _ in 0..u16::MAX {
            let raw = self.inner.next_nonce.fetch_add(1, Ordering::Relaxed);
            let nonce = (raw as u16).max(1);
            let mut pending = self
                .inner
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let std::collections::hash_map::Entry::Vacant(entry) = pending.entry(nonce) {
                let (sender, receiver) = oneshot::channel();
                entry.insert(sender);
                return Ok((
                    nonce,
                    receiver,
                    NativePendingGuard {
                        inner: Arc::downgrade(&self.inner),
                        nonce,
                    },
                ));
            }
        }
        Err(NativeMutationFailure::ResourceExhausted)
    }
}

impl Drop for NativeEndpointInner {
    fn drop(&mut self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let service = Arc::clone(&self.service);
            let endpoint = self.endpoint;
            let generation = self.app.boot_generation;
            tokio::spawn(async move {
                service.unregister_endpoint(endpoint, generation).await;
            });
        }
    }
}

async fn route_native_events(
    inner: Weak<NativeEndpointInner>,
    mut receiver: mpsc::Receiver<BackendEvent>,
) {
    while let Some(event) = receiver.recv().await {
        let Some(inner) = inner.upgrade() else {
            return;
        };
        route_native_event(&inner, event);
    }
    if let Some(inner) = inner.upgrade() {
        close_native_endpoint(&inner).await;
    }
}

fn route_native_event(inner: &Arc<NativeEndpointInner>, event: BackendEvent) {
    match event {
        BackendEvent::Reply { nonce, reply } => complete_native_pending(inner, nonce, reply),
        BackendEvent::Retained {
            extension_handle,
            sequence,
            item,
        } => match &item.kind {
            RetainedItemKind::Output {
                kind,
                attempt,
                data,
            } => {
                let route = inner
                    .follows
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&extension_handle)
                    .cloned();
                if let Some(route) = route
                    && *attempt == route.attempt
                    && sequence >= route.from_sequence
                    && route
                        .sender
                        .try_send(NativeFollowItem::Output {
                            kind: *kind,
                            sequence,
                            data: data.clone(),
                        })
                        .is_err()
                {
                    inner
                        .follows
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&extension_handle);
                    spawn_native_unfollow(Arc::clone(inner), extension_handle);
                }
            }
            RetainedItemKind::Exit(exit) => {
                let route = inner
                    .follows
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&extension_handle)
                    .cloned();
                if let Some(route) = route
                    && exit.attempt == route.attempt
                {
                    let _ = route.sender.try_send(NativeFollowItem::Complete {
                        through_sequence: sequence,
                    });
                    inner
                        .follows
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&extension_handle);
                    spawn_native_unfollow(Arc::clone(inner), extension_handle);
                }
            }
            RetainedItemKind::Status => {}
        },
        BackendEvent::ReplayDone {
            extension_handle,
            through_sequence,
        } => {
            let _ = (extension_handle, through_sequence);
        }
    }
}

fn complete_native_pending(inner: &NativeEndpointInner, nonce: u16, reply: NativeReply) {
    if let Some(sender) = inner
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&nonce)
    {
        let _ = sender.send(reply);
    }
}

fn spawn_native_unfollow(inner: Arc<NativeEndpointInner>, extension_handle: u64) {
    tokio::spawn(async move {
        inner
            .service
            .handle_control(
                inner.app.clone(),
                inner.endpoint,
                0,
                extension_handle,
                EXT_CONTROL_UNFOLLOW,
            )
            .await;
    });
}

fn clear_native_routes(
    pending: &std::sync::Mutex<HashMap<u16, oneshot::Sender<NativeReply>>>,
    follows: &std::sync::Mutex<HashMap<u64, NativeFollowRoute>>,
) {
    pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    follows
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

fn clear_native_endpoint(inner: &NativeEndpointInner) {
    clear_native_routes(&inner.pending, &inner.follows);
}

async fn close_native_endpoint(inner: &Arc<NativeEndpointInner>) {
    if !inner.closed.swap(true, Ordering::AcqRel) {
        clear_native_endpoint(inner);
        inner
            .service
            .unregister_endpoint(inner.endpoint, inner.app.boot_generation)
            .await;
    }
}

fn native_status_failure(status: u8, detail: &str) -> NativeMutationFailure {
    match status {
        EXT_STATUS_UNKNOWN_ID | EXT_STATUS_NOT_FOUND => NativeMutationFailure::NotFound,
        EXT_STATUS_PERMISSION => NativeMutationFailure::Permission,
        EXT_STATUS_TOO_LARGE => NativeMutationFailure::TooLarge,
        EXT_STATUS_BUDGET => NativeMutationFailure::ResourceExhausted,
        EXT_STATUS_INVALID => NativeMutationFailure::Invalid(detail.to_owned()),
        EXT_STATUS_CANCELLED => NativeMutationFailure::Cancelled,
        EXT_STATUS_CONFLICT => NativeMutationFailure::Conflict,
        _ => NativeMutationFailure::Internal(detail.to_owned()),
    }
}

impl ExtensionService {
    async fn acquire_running_permit(
        &self,
        extension_id: u64,
        queued_generation: u64,
        wake: Arc<Notify>,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let acquire = self.running.clone().acquire_owned();
        tokio::pin!(acquire);
        loop {
            tokio::select! {
                permit = &mut acquire => return permit.ok(),
                _ = wake.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            let inner = self.inner.lock().await;
            let still_queued = !inner.shutting_down
                && inner
                    .definitions
                    .get(&extension_id)
                    .is_some_and(|definition| {
                        definition.generation == queued_generation
                            && definition.enabled()
                            && definition.desired()
                    });
            if !still_queued {
                return None;
            }
        }
    }

    fn ensure_supervisor(
        self: &Arc<Self>,
        state: super::AppState,
        extension_id: u64,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        // A native YAS extension can deploy another native extension. Erase
        // this recursive async edge so Send auto-trait evaluation and the
        // future layout do not recurse through supervise -> drive_attempt ->
        // YAS Extension dispatch -> ensure_supervisor.
        let this = Arc::clone(self);
        Box::pin(async move {
            let completion = {
                let mut inner = this.inner.lock().await;
                let eligible = inner
                    .definitions
                    .get(&extension_id)
                    .is_some_and(|definition| {
                        definition.enabled()
                            && definition.desired()
                            && definition.phase != EXT_PHASE_NEED_OBJECT
                    });
                if !inner.shutting_down && eligible && inner.supervisors.insert(extension_id) {
                    let completion = SupervisorCompletion::new();
                    let completions = inner
                        .supervisor_completions
                        .entry(extension_id)
                        .or_default();
                    completions.retain(|completion| !completion.is_complete());
                    completions.push(Arc::clone(&completion));
                    Some(completion)
                } else {
                    None
                }
            };
            if let Some(completion) = completion {
                let service = Arc::clone(&this);
                tokio::spawn(async move {
                    let guard = SupervisorCompletionGuard(Arc::clone(&completion));
                    Arc::clone(&service).supervise(state, extension_id).await;
                    completion.complete();
                    let mut inner = service.inner.lock().await;
                    // The task boundary is the authoritative registry
                    // cleanup for every early-return path in `supervise`.
                    inner.supervisors.remove(&extension_id);
                    let remove_entry = if let Some(completions) =
                        inner.supervisor_completions.get_mut(&extension_id)
                    {
                        completions.retain(|current| !Arc::ptr_eq(current, &completion));
                        completions.is_empty()
                    } else {
                        false
                    };
                    if remove_entry {
                        inner.supervisor_completions.remove(&extension_id);
                    }
                    drop(guard);
                });
            }
        })
    }

    async fn supervise(self: Arc<Self>, state: super::AppState, extension_id: u64) {
        loop {
            let terminal = {
                let mut inner = self.inner.lock().await;
                let shutting_down = inner.shutting_down;
                let Some(definition) = inner.definitions.get_mut(&extension_id) else {
                    inner.supervisors.remove(&extension_id);
                    return;
                };
                if shutting_down
                    || !definition.enabled()
                    || !definition.desired()
                    || definition.phase == EXT_PHASE_NEED_OBJECT
                {
                    if definition.phase != EXT_PHASE_NEED_OBJECT {
                        definition.phase = EXT_PHASE_STOPPED;
                        definition.task_id = 0;
                        definition.next_start_unix_ms = 0;
                    }
                    Some((
                        shutting_down,
                        definition.persistent(),
                        definition.generation,
                        Arc::clone(&definition.wake),
                    ))
                } else {
                    definition.phase = EXT_PHASE_QUEUED;
                    definition.task_id = 0;
                    definition.next_start_unix_ms = 0;
                    None
                }
            };
            if let Some((shutting_down, persistent, generation, wake)) = terminal {
                if shutting_down {
                    let mut inner = self.inner.lock().await;
                    inner.supervisors.remove(&extension_id);
                    if !persistent {
                        remove_definition_locked(&mut inner, extension_id);
                    }
                    return;
                }
                if persistent {
                    self.inner.lock().await.supervisors.remove(&extension_id);
                    return;
                }
                tokio::select! {
                    _ = tokio::time::sleep(self.terminal_retain) => {
                        self.release_transient(extension_id, generation, true).await;
                        return;
                    }
                    _ = wake.notified() => continue,
                }
            }
            {
                let mut inner = self.inner.lock().await;
                emit_lifecycle_locked(&mut inner, extension_id, self.output_retain_max);
            }
            let Some((queued_generation, wake)) = self
                .inner
                .lock()
                .await
                .definitions
                .get(&extension_id)
                .map(|definition| (definition.generation, Arc::clone(&definition.wake)))
            else {
                break;
            };
            let permit = self
                .acquire_running_permit(extension_id, queued_generation, wake)
                .await;
            let Some(permit) = permit else {
                continue;
            };

            let validation = match self.validating.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => break,
            };
            let prepared = self.prepare_attempt(extension_id).await;
            let (
                mut attempt,
                generation,
                attempt_number,
                name,
                args,
                flags,
                revision,
                hash,
                loaded_argument_reservation,
            ) = match prepared {
                Ok(value) => value,
                Err(PrepareAttemptError::ArgumentBudget(wake)) => {
                    // Contention is admission pressure, not an attempt or a
                    // durable failure. Release execution permits before
                    // waiting so resident transient work can drain.
                    drop(validation);
                    drop(permit);
                    tokio::select! {
                        _ = self.argument_budget.notify.notified() => {}
                        _ = wake.notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                    continue;
                }
                Err(PrepareAttemptError::Superseded) => {
                    drop(validation);
                    drop(permit);
                    continue;
                }
                Err(PrepareAttemptError::Failed(error)) => {
                    self.block_definition(extension_id, error).await;
                    drop(validation);
                    drop(permit);
                    if self.wait_blocked_or_restart(extension_id).await {
                        continue;
                    }
                    return;
                }
            };

            let connection = super::ConnectionCancellation::default();
            let host = attempt.cancellation();
            let preparation_installed = {
                let mut inner = self.inner.lock().await;
                let valid = !inner.shutting_down
                    && inner
                        .definitions
                        .get(&extension_id)
                        .is_some_and(|definition| {
                            definition.generation == generation
                                && definition.definition_revision == revision
                                && definition.enabled()
                                && definition.desired()
                        });
                if valid && let Some(definition) = inner.definitions.get_mut(&extension_id) {
                    definition.control = Some(AttemptControl {
                        definition_revision: revision,
                        attempt: attempt_number,
                        task_id: 0,
                        host: host.clone(),
                        connection: connection.clone(),
                    });
                    definition.interrupt = None;
                }
                valid
            };
            if !preparation_installed {
                attempt.cancel();
                let _ = attempt.join().await;
                drop(validation);
                drop(permit);
                continue;
            }

            if let Err(error) = attempt.wait_prepared().await {
                attempt.cancel();
                let _ = attempt.join().await;
                self.block_definition(extension_id, error).await;
                drop(validation);
                drop(permit);
                if self.wait_blocked_or_restart(extension_id).await {
                    continue;
                }
                return;
            }
            if !attempt.native_yas() {
                attempt.cancel();
                let _ = attempt.join().await;
                self.block_definition(
                    extension_id,
                    AttemptFailure {
                        kind: FailureKind::Validation,
                        detail:
                            "extension module must declare the native YAS v1 ABI with `yas_wire_v1`"
                                .into(),
                    },
                )
                .await;
                drop(validation);
                drop(permit);
                return;
            }
            drop(validation);

            let task_id = {
                let mut inner = self.inner.lock().await;
                let Some(current) = inner.definitions.get(&extension_id) else {
                    attempt.cancel();
                    drop(inner);
                    let _ = attempt.join().await;
                    drop(permit);
                    break;
                };
                if inner.shutting_down
                    || current.generation != generation
                    || !current.enabled()
                    || !current.desired()
                    || !current.control.as_ref().is_some_and(|control| {
                        control.definition_revision == revision
                            && control.attempt == attempt_number
                            && control.task_id == 0
                    })
                {
                    attempt.cancel();
                    drop(inner);
                    let _ = attempt.join().await;
                    drop(permit);
                    continue;
                }
                let Some(task_id) = allocate_task_id(&inner) else {
                    attempt.cancel();
                    drop(inner);
                    let _ = attempt.join().await;
                    drop(permit);
                    self.block_definition(
                        extension_id,
                        AttemptFailure {
                            kind: FailureKind::HostFailure,
                            detail: "could not allocate a task ID".into(),
                        },
                    )
                    .await;
                    return;
                };
                inner.task_ids.insert(task_id);
                if let Some(definition) = inner.definitions.get_mut(&extension_id)
                    && let Some(control) = definition.control.as_mut()
                {
                    control.task_id = task_id;
                }
                Ok(task_id)
            };
            let task_id = match task_id {
                Ok(task_id) => task_id,
                Err(error) => {
                    attempt.cancel();
                    let _ = attempt.join().await;
                    drop(permit);
                    self.block_definition(extension_id, error).await;
                    return;
                }
            };

            let native_context = yas_wire::extension::AttemptContext {
                definition_revision: revision,
                attempt: attempt_number,
                task_id,
                extension_handle: extension_id,
                generation,
                flags: yas_definition_flags(flags),
                runtime: match &attempt {
                    RuntimeAttempt::Wasmi(_) => yas_wire::extension::Runtime::Wasmi,
                    RuntimeAttempt::QuickJs(_) => yas_wire::extension::Runtime::QuickJs,
                },
                content_hash: hash,
                name: name.clone(),
                argv: args.clone(),
                extensions: yas_wire::Extensions::default(),
            };
            drop(args);
            drop(loaded_argument_reservation);
            let publication = AttemptPublication {
                service: Arc::clone(&self),
                extension_id,
                generation,
                definition_revision: revision,
                attempt: attempt_number,
                task_id,
            };
            let driven = drive_attempt(
                state.clone(),
                attempt,
                native_context,
                connection.clone(),
                publication,
            )
            .await;
            let running_for = driven.running_for;

            let decision = self
                .finish_attempt(
                    extension_id,
                    generation,
                    revision,
                    attempt_number,
                    task_id,
                    driven,
                    running_for,
                )
                .await;
            drop(permit);

            match decision {
                NextAttempt::Stop => {
                    let terminal = {
                        let mut inner = self.inner.lock().await;
                        inner.supervisors.remove(&extension_id);
                        inner.definitions.get(&extension_id).map(|definition| {
                            (
                                definition.persistent(),
                                definition.generation,
                                Arc::clone(&definition.wake),
                            )
                        })
                    };
                    let Some((persistent, generation, wake)) = terminal else {
                        // Owner-loss teardown removes the transient definition
                        // synchronously. Never extend the recursive cleanup
                        // barrier merely to wait out its replay lease.
                        return;
                    };
                    if persistent {
                        return;
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(self.terminal_retain) => {
                            self.release_transient(extension_id, generation, true).await;
                        }
                        _ = wake.notified() => {}
                    }
                    return;
                }
                NextAttempt::Immediate => continue,
                NextAttempt::Backoff { duration, wake } => {
                    tokio::select! {
                        _ = tokio::time::sleep(duration) => {}
                        _ = wake.notified() => {}
                    }
                }
            }
        }
        self.inner.lock().await.supervisors.remove(&extension_id);
    }

    async fn prepare_attempt(
        &self,
        extension_id: u64,
    ) -> Result<PreparedAttempt, PrepareAttemptError> {
        let (snapshot, loaded_argument_reservation, attempt_number, object_read) = {
            let mut inner = self.inner.lock().await;
            let snapshot = inner
                .definitions
                .get(&extension_id)
                .cloned()
                .ok_or_else(|| AttemptFailure {
                    kind: FailureKind::HostFailure,
                    detail: "extension disappeared before validation".into(),
                })?;
            if !snapshot.enabled() || !snapshot.desired() {
                return Err(PrepareAttemptError::Superseded);
            }
            let loaded_argument_reservation = if snapshot.persistent() && snapshot.catalog_committed
            {
                if snapshot.argument_bytes > self.argument_budget.max {
                    return Err(AttemptFailure {
                        kind: FailureKind::HostFailure,
                        detail: "persistent extension arguments exceed the argument-store budget"
                            .into(),
                    }
                    .into());
                }
                Some(
                    self.argument_budget
                        .try_reserve(snapshot.argument_bytes)
                        .ok_or_else(|| {
                            PrepareAttemptError::ArgumentBudget(Arc::clone(&snapshot.wake))
                        })?,
                )
            } else {
                None
            };
            let attempt_number = snapshot
                .attempt
                .checked_add(1)
                .ok_or_else(|| AttemptFailure {
                    kind: FailureKind::HostFailure,
                    detail: "extension attempt counter exhausted".into(),
                })?;
            let object_read = inner
                .store
                .as_mut()
                .ok_or(ObjectStoreError::NotFound)
                .and_then(|store| store.reserve_read(&snapshot.hash))
                .map_err(|error| AttemptFailure {
                    kind: FailureKind::Validation,
                    detail: error.to_string(),
                })?;
            (
                snapshot,
                loaded_argument_reservation,
                attempt_number,
                object_read,
            )
        };
        let args = {
            let _catalog_io = if snapshot.args.is_none() {
                Some(self.catalog_io.lock().await)
            } else {
                None
            };
            self.definition_arguments(&snapshot)
                .await
                .map_err(|error| AttemptFailure {
                    kind: FailureKind::HostFailure,
                    detail: error.to_string(),
                })?
        };
        let module = match tokio::task::block_in_place(|| object_read.read_verified()) {
            Ok(module) => module,
            Err(error) => {
                let missing = matches!(
                    &error,
                    ObjectStoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound
                );
                let corrupt = matches!(&error, ObjectStoreError::HashMismatch);
                let removed = missing || (corrupt && object_read.remove_file().is_ok());
                let mut inner = self.inner.lock().await;
                if removed {
                    if let Some(store) = inner.store.as_mut() {
                        store.forget_removed(&snapshot.hash);
                    }
                    mark_hash_unpinned(&mut inner, &snapshot.hash);
                }
                return Err(AttemptFailure {
                    kind: FailureKind::Validation,
                    detail: error.to_string(),
                }
                .into());
            }
        };
        let actual_runtime = if is_wasm_module(&module) {
            yas_wire::schema::extension::RUNTIME_WASMI as u8
        } else {
            yas_wire::schema::extension::RUNTIME_QUICKJS as u8
        };
        if snapshot.native_runtime != yas_wire::schema::extension::RUNTIME_AUTO as u8
            && snapshot.native_runtime != actual_runtime
        {
            return Err(AttemptFailure {
                kind: FailureKind::Validation,
                detail: "extension object does not match the requested runtime".into(),
            }
            .into());
        }
        self.persist_store_lru()
            .await
            .map_err(|error| AttemptFailure {
                kind: FailureKind::Validation,
                detail: error.to_string(),
            })?;
        let _catalog_io = if snapshot.persistent() {
            Some(self.catalog_io.lock().await)
        } else {
            None
        };
        let still_current = {
            let inner = self.inner.lock().await;
            !inner.shutting_down
                && inner
                    .definitions
                    .get(&extension_id)
                    .is_some_and(|definition| {
                        definition.generation == snapshot.generation
                            && definition.definition_revision == snapshot.definition_revision
                            && definition.hash == snapshot.hash
                            && definition.attempt == snapshot.attempt
                            && definition.enabled()
                            && definition.desired()
                    })
        };
        if !still_current {
            return Err(PrepareAttemptError::Superseded);
        }
        self.persist_attempt_counters_catalog(
            extension_id,
            attempt_number,
            snapshot.last_running_attempt,
            snapshot.persistent(),
        )
        .await
        .map_err(|error| AttemptFailure {
            kind: FailureKind::HostFailure,
            detail: error.to_string(),
        })?;
        let mut inner = self.inner.lock().await;
        let still_current = !inner.shutting_down
            && inner
                .definitions
                .get(&extension_id)
                .is_some_and(|definition| {
                    definition.generation == snapshot.generation
                        && definition.definition_revision == snapshot.definition_revision
                        && definition.hash == snapshot.hash
                        && definition.attempt == snapshot.attempt
                        && definition.enabled()
                        && definition.desired()
                });
        if !still_current {
            return Err(PrepareAttemptError::Superseded);
        }
        if let Some(definition) = inner.definitions.get_mut(&extension_id) {
            definition.phase = EXT_PHASE_VALIDATING;
            definition.attempt = attempt_number;
            definition.task_id = 0;
            definition.detail.clear();
        }
        emit_lifecycle_locked(&mut inner, extension_id, self.output_retain_max);
        drop(inner);
        drop(_catalog_io);

        let label = (!snapshot.name.is_empty()).then_some(snapshot.name.clone());
        let attempt = if is_wasm_module(&module) {
            RuntimeAttempt::Wasmi(
                wasmi_host::spawn_attempt(WasmiAttemptSpec {
                    module: Arc::<[u8]>::from(module),
                    module_hash: snapshot.hash,
                    extension_id,
                    label,
                    config: self.host_config.clone(),
                })
                .map_err(|error| AttemptFailure {
                    kind: FailureKind::HostFailure,
                    detail: error.to_string(),
                })?,
            )
        } else {
            RuntimeAttempt::QuickJs(
                quickjs_host::spawn_attempt(quickjs_host::AttemptSpec {
                    source: Arc::<[u8]>::from(module),
                    module_hash: snapshot.hash,
                    extension_id,
                    label,
                    config: self.host_config.clone(),
                })
                .map_err(|error| AttemptFailure {
                    kind: FailureKind::HostFailure,
                    detail: error.to_string(),
                })?,
            )
        };
        if std::env::var_os("YAS_EXT_THREAD_DEBUG").is_some() {
            eprintln!(
                "yas-server: prepared extension thread {} ({})",
                attempt.thread_names().logical,
                attempt.thread_names().os
            );
        }
        Ok((
            attempt,
            snapshot.generation,
            attempt_number,
            snapshot.name,
            args,
            snapshot.flags,
            snapshot.definition_revision,
            snapshot.hash,
            loaded_argument_reservation,
        ))
    }

    async fn block_definition(&self, extension_id: u64, error: AttemptFailure) {
        let persistent = {
            let inner = self.inner.lock().await;
            inner
                .definitions
                .get(&extension_id)
                .is_some_and(Definition::persistent)
        };
        let _catalog_io = if persistent {
            Some(self.catalog_io.lock().await)
        } else {
            None
        };
        let (should_block, persistent) = {
            let inner = self.inner.lock().await;
            let shutting_down = inner.shutting_down;
            (
                inner
                    .definitions
                    .get(&extension_id)
                    .is_some_and(|definition| {
                        !shutting_down && definition.enabled() && definition.desired()
                    }),
                inner
                    .definitions
                    .get(&extension_id)
                    .is_some_and(Definition::persistent),
            )
        };
        let mut detail = bounded_detail(&error.detail);
        if should_block && persistent {
            let durable_detail = detail.clone();
            let persisted = self
                .catalog_call(move |catalog| {
                    catalog.set_lifecycle(
                        extension_id,
                        None,
                        None,
                        None,
                        None,
                        None,
                        Some(0),
                        Some(BlockedState::Set(&durable_detail)),
                    )
                })
                .await;
            if let Err(persist_error) = persisted {
                detail = bounded_detail(&format!(
                    "{detail}; could not persist blocked state: {persist_error}"
                ));
            }
        }
        let mut inner = self.inner.lock().await;
        if let Some(definition) = inner.definitions.get_mut(&extension_id) {
            definition.phase = if should_block {
                EXT_PHASE_BLOCKED
            } else {
                EXT_PHASE_STOPPED
            };
            definition.task_id = 0;
            definition.control = None;
            definition.next_start_unix_ms = 0;
            definition.detail = detail;
        }
        emit_lifecycle_locked(&mut inner, extension_id, self.output_retain_max);
    }

    async fn wait_blocked_or_restart(&self, extension_id: u64) -> bool {
        let Some((persistent, generation, wake, owner_endpoint)) = ({
            let mut inner = self.inner.lock().await;
            let state = inner.definitions.get(&extension_id).map(|definition| {
                (
                    definition.persistent(),
                    definition.generation,
                    Arc::clone(&definition.wake),
                    definition.owner_endpoint,
                )
            });
            if state
                .as_ref()
                .is_some_and(|(persistent, _, _, _)| *persistent)
            {
                inner.supervisors.remove(&extension_id);
            }
            state
        }) else {
            return false;
        };
        if persistent {
            return false;
        }
        if let Some(owner_endpoint) = owner_endpoint {
            let owner_live = self
                .inner
                .lock()
                .await
                .endpoints
                .contains_key(&owner_endpoint);
            if !owner_live {
                self.release_transient(extension_id, generation, false)
                    .await;
                return false;
            }
            wake.notified().await;
            let state = self
                .inner
                .lock()
                .await
                .definitions
                .get(&extension_id)
                .map(|definition| {
                    (
                        definition.enabled() && definition.desired(),
                        definition.generation,
                    )
                });
            if let Some((true, _)) = state {
                return true;
            }
            if let Some((false, current_generation)) = state {
                self.release_transient(extension_id, current_generation, false)
                    .await;
            }
            return false;
        }
        tokio::select! {
            _ = tokio::time::sleep(self.terminal_retain) => {
                self.release_transient(extension_id, generation, true).await;
                false
            }
            _ = wake.notified() => {
                let inner = self.inner.lock().await;
                let state = inner.definitions.get(&extension_id).map(|definition| {
                    (
                        definition.enabled() && definition.desired(),
                        definition.generation,
                    )
                });
                drop(inner);
                if let Some((false, current_generation)) = state {
                    self.release_transient(extension_id, current_generation, false)
                        .await;
                }
                state.is_some_and(|(eligible, _)| eligible)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish_attempt(
        &self,
        extension_id: u64,
        generation: u64,
        attempt_revision: u64,
        attempt_number: u64,
        task_id: u32,
        driven: DrivenAttempt,
        running_for: Duration,
    ) -> NextAttempt {
        let persistent = {
            let inner = self.inner.lock().await;
            inner
                .definitions
                .get(&extension_id)
                .is_some_and(Definition::persistent)
        };
        let _catalog_io = if persistent {
            Some(self.catalog_io.lock().await)
        } else {
            None
        };
        let mut inner = self.inner.lock().await;
        inner.task_ids.remove(&task_id);
        let Some(mut definition) = inner.definitions.remove(&extension_id) else {
            return NextAttempt::Stop;
        };
        let visible_definition = definition.clone();
        inner
            .commands
            .invalidate_attempt(extension_id, attempt_revision, attempt_number);
        definition.control = None;
        definition.task_id = 0;
        definition.next_start_unix_ms = 0;
        let interrupt = definition.interrupt.take();
        let (mut reason, mut code, mut detail, failure) = classify_outcome(&driven, interrupt);
        if running_for >= Duration::from_secs(60) {
            definition.failure_count = 0;
        }
        if failure {
            definition.failure_count = definition.failure_count.saturating_add(1);
        } else if reason == EXT_EXIT_RETURNED && code == 0 {
            definition.failure_count = 0;
        }

        let explicit_replace = matches!(interrupt, Some(Interrupt::Updated | Interrupt::Restarted));
        let suppressed = matches!(
            interrupt,
            Some(
                Interrupt::Cancelled
                    | Interrupt::Disabled
                    | Interrupt::OwnerClosed
                    | Interrupt::ServerShutdown
            )
        );
        let automatic = !suppressed
            && definition.enabled()
            && definition.desired()
            && (definition.restart == EXT_RESTART_ALWAYS
                || failure && definition.restart == EXT_RESTART_ON_FAILURE);
        let mut restart = explicit_replace || automatic;
        let mut backoff = restart && !explicit_replace;
        let mut duration = if backoff {
            backoff_duration(definition.failure_count.max(1))
        } else {
            Duration::ZERO
        };
        if restart {
            if backoff {
                definition.phase = EXT_PHASE_BACKOFF;
                definition.next_start_unix_ms = unix_millis_after(duration);
            } else {
                definition.phase = EXT_PHASE_QUEUED;
            }
        } else {
            definition.phase = EXT_PHASE_STOPPED;
            if !suppressed && !explicit_replace {
                definition.set_flag(EXT_FLAG_DESIRED_RUNNING, false);
            }
        }
        definition.detail = bounded_detail(&detail);
        if definition.generation == generation && explicit_replace {
            definition.generation = definition.generation.saturating_add(1);
        }
        let persisted = if persistent {
            inner.definitions.insert(extension_id, visible_definition);
            drop(inner);
            let persisted = self.persist_terminal_catalog(&definition).await;
            inner = self.inner.lock().await;
            let _ = inner.definitions.remove(&extension_id);
            persisted
        } else {
            Ok(())
        };
        if let Err(error) = persisted {
            restart = false;
            backoff = false;
            duration = Duration::ZERO;
            definition.phase = EXT_PHASE_BLOCKED;
            definition.next_start_unix_ms = 0;
            detail = error.to_string();
            definition.detail = bounded_detail(&detail);
            definition.failure_count = definition.failure_count.saturating_add(1);
            reason = EXT_EXIT_HOST_FAILURE;
            code = 0;
        }

        let owner_lost = !definition.persistent()
            && definition
                .owner_endpoint
                .is_some_and(|owner| !inner.endpoints.contains_key(&owner));
        let compact_terminal = !definition.persistent()
            && matches!(definition.phase, EXT_PHASE_STOPPED | EXT_PHASE_BLOCKED);
        let next_start = definition.next_start_unix_ms;
        inner.definitions.insert(extension_id, definition);
        let exit = RetainedItem::exit(NativeExit {
            kind: reason,
            code,
            attempt: attempt_number,
            detail: bounded_detail(&detail),
        });
        let _ = (attempt_revision, task_id, next_start);
        if compact_terminal {
            retain_terminal_and_fanout(
                &mut inner,
                extension_id,
                exit,
                self.output_retain_max,
                TerminalRecordKind::Exit,
            );
        } else {
            retain_and_fanout(&mut inner, extension_id, exit, self.output_retain_max);
        }
        emit_lifecycle_locked(&mut inner, extension_id, self.output_retain_max);
        if owner_lost {
            remove_definition_locked(&mut inner, extension_id);
            return NextAttempt::Stop;
        }
        let wake = inner
            .definitions
            .get(&extension_id)
            .map(|definition| Arc::clone(&definition.wake))
            .unwrap_or_else(|| Arc::new(Notify::new()));
        if !restart {
            NextAttempt::Stop
        } else if backoff {
            NextAttempt::Backoff { duration, wake }
        } else {
            NextAttempt::Immediate
        }
    }

    async fn release_transient(
        &self,
        extension_id: u64,
        generation: u64,
        force_terminal_replay: bool,
    ) {
        let mut inner = self.inner.lock().await;
        let removable = inner
            .definitions
            .get(&extension_id)
            .is_some_and(|definition| {
                !definition.persistent()
                    && definition.generation == generation
                    && definition.control.is_none()
                    && matches!(definition.phase, EXT_PHASE_STOPPED | EXT_PHASE_BLOCKED)
            });
        if !removable {
            return;
        }
        if force_terminal_replay {
            force_terminal_replay_locked(&mut inner, extension_id);
        }
        remove_definition_locked(&mut inner, extension_id);
    }
}

type PreparedAttempt = (
    RuntimeAttempt,
    u64,
    u64,
    String,
    Vec<Vec<u8>>,
    u8,
    u64,
    ObjectHash,
    Option<Arc<ArgumentReservation>>,
);

#[derive(Debug)]
enum RuntimeAttempt {
    Wasmi(wasmi_host::WasmiAttempt),
    QuickJs(quickjs_host::QuickJsAttempt),
}

fn yas_definition_flags(flags: u8) -> u16 {
    let mut native = 0;
    if flags & EXT_FLAG_PERSIST != 0 {
        native |= yas_wire::schema::extension::DEFINITION_PERSISTENT as u16;
    }
    if flags & EXT_FLAG_ENABLED != 0 {
        native |= yas_wire::schema::extension::DEFINITION_ENABLED as u16;
    }
    if flags & EXT_FLAG_ENABLED != 0 && flags & EXT_FLAG_DESIRED_RUNNING != 0 {
        native |= yas_wire::schema::extension::DEFINITION_DESIRED_RUNNING as u16;
    }
    if flags & EXT_FLAG_DETACH != 0 {
        native |= yas_wire::schema::extension::DEFINITION_DETACHED as u16;
    }
    native
}

impl RuntimeAttempt {
    fn native_yas(&self) -> bool {
        match self {
            Self::Wasmi(attempt) => attempt.native_yas(),
            // QuickJS is an in-process typed YAS guest; it has no Wasm export
            // table in which to publish the marker.
            Self::QuickJs(_) => true,
        }
    }

    fn thread_names(&self) -> &crate::thread_name::ThreadNames {
        match self {
            Self::Wasmi(attempt) => attempt.thread_names(),
            Self::QuickJs(attempt) => attempt.thread_names(),
        }
    }

    fn cancellation(&self) -> AttemptCancellation {
        match self {
            Self::Wasmi(attempt) => attempt.cancellation(),
            Self::QuickJs(attempt) => attempt.cancellation(),
        }
    }

    fn bridge(&self) -> wasmi_host::HostBridge {
        match self {
            Self::Wasmi(attempt) => attempt.bridge(),
            Self::QuickJs(attempt) => attempt.bridge(),
        }
    }

    async fn wait_prepared(&mut self) -> Result<(), AttemptFailure> {
        match self {
            Self::Wasmi(attempt) => attempt.wait_prepared().await,
            Self::QuickJs(attempt) => attempt.wait_prepared().await,
        }
    }

    fn start(&mut self) -> Result<(), wasmi_host::LifecycleError> {
        match self {
            Self::Wasmi(attempt) => attempt.start(),
            Self::QuickJs(attempt) => attempt.start(),
        }
    }

    fn cancel(&self) {
        match self {
            Self::Wasmi(attempt) => attempt.cancel(),
            Self::QuickJs(attempt) => attempt.cancel(),
        }
    }

    async fn join(self) -> Result<AttemptOutcome, wasmi_host::LifecycleError> {
        match self {
            Self::Wasmi(attempt) => attempt.join().await,
            Self::QuickJs(attempt) => attempt.join().await,
        }
    }
}

enum PrepareAttemptError {
    ArgumentBudget(Arc<Notify>),
    Superseded,
    Failed(AttemptFailure),
}

impl From<AttemptFailure> for PrepareAttemptError {
    fn from(error: AttemptFailure) -> Self {
        Self::Failed(error)
    }
}

enum NextAttempt {
    Stop,
    Immediate,
    Backoff {
        duration: Duration,
        wake: Arc<Notify>,
    },
}

struct AttemptPublication {
    service: Arc<ExtensionService>,
    extension_id: u64,
    generation: u64,
    definition_revision: u64,
    attempt: u64,
    task_id: u32,
}

impl AttemptPublication {
    async fn publish_running(&self) -> Result<(), AttemptFailure> {
        let persistent = {
            let inner = self.service.inner.lock().await;
            inner
                .definitions
                .get(&self.extension_id)
                .is_some_and(Definition::persistent)
        };
        let _catalog_io = if persistent {
            Some(self.service.catalog_io.lock().await)
        } else {
            None
        };
        let is_valid = |inner: &ServiceState| {
            !inner.shutting_down
                && inner
                    .definitions
                    .get(&self.extension_id)
                    .is_some_and(|definition| {
                        definition.generation == self.generation
                            && definition.definition_revision == self.definition_revision
                            && definition.enabled()
                            && definition.desired()
                            && definition.control.as_ref().is_some_and(|control| {
                                control.definition_revision == self.definition_revision
                                    && control.attempt == self.attempt
                                    && control.task_id == self.task_id
                            })
                    })
        };
        {
            let inner = self.service.inner.lock().await;
            if !is_valid(&inner) {
                return Err(AttemptFailure {
                    kind: FailureKind::HostFailure,
                    detail: "extension attempt was superseded during bootstrap".into(),
                });
            }
        }
        self.service
            .persist_attempt_counters_catalog(
                self.extension_id,
                self.attempt,
                self.attempt,
                persistent,
            )
            .await
            .map_err(|error| AttemptFailure {
                kind: FailureKind::HostFailure,
                detail: error.to_string(),
            })?;
        let mut inner = self.service.inner.lock().await;
        if !is_valid(&inner) {
            return Err(AttemptFailure {
                kind: FailureKind::HostFailure,
                detail: "extension attempt was superseded during bootstrap".into(),
            });
        }
        if let Some(definition) = inner.definitions.get_mut(&self.extension_id) {
            definition.phase = EXT_PHASE_RUNNING;
            definition.task_id = self.task_id;
            definition.last_running_attempt = self.attempt;
            definition.detail.clear();
        }
        emit_lifecycle_locked(
            &mut inner,
            self.extension_id,
            self.service.output_retain_max,
        );
        Ok(())
    }

    async fn publish_stopping(&self) {
        let mut inner = self.service.inner.lock().await;
        let matches = inner
            .definitions
            .get(&self.extension_id)
            .and_then(|definition| definition.control.as_ref())
            .is_some_and(|control| {
                control.definition_revision == self.definition_revision
                    && control.attempt == self.attempt
                    && control.task_id == self.task_id
            });
        if matches
            && let Some(definition) = inner.definitions.get_mut(&self.extension_id)
            && definition.phase == EXT_PHASE_RUNNING
        {
            definition.phase = EXT_PHASE_STOPPING;
            definition.task_id = 0;
            emit_lifecycle_locked(
                &mut inner,
                self.extension_id,
                self.service.output_retain_max,
            );
        }
    }
}

struct DrivenAttempt {
    outcome: AttemptOutcome,
    handler_closed_first: bool,
    connection_failure: Option<super::ConnectionFailure>,
    running_for: Duration,
}

async fn drive_attempt(
    state: super::AppState,
    mut attempt: RuntimeAttempt,
    native_context: yas_wire::extension::AttemptContext,
    connection: super::ConnectionCancellation,
    publication: AttemptPublication,
) -> DrivenAttempt {
    let bridge = attempt.bridge();
    let host_cancel = attempt.cancellation();
    let (server_stream, client_stream) =
        tokio::io::duplex(wasmi_host::PACKET_MAX_BYTES.saturating_add(4));
    let (mut from_server, mut to_server) = tokio::io::split(client_stream);

    let handler_state = state.clone();
    let Some(registration) = handler_state.connections.register(connection.clone()) else {
        attempt.cancel();
        return DrivenAttempt {
            outcome: attempt.join().await.unwrap_or_else(|error| {
                AttemptOutcome::Failed(AttemptFailure {
                    kind: FailureKind::HostFailure,
                    detail: error.to_string(),
                })
            }),
            handler_closed_first: true,
            connection_failure: connection.failure(),
            running_for: Duration::ZERO,
        };
    };
    let handler_connection = connection.clone();
    let endpoint: Pin<Box<dyn Future<Output = ()> + Send>> =
        Box::pin(super::yas::serve_attempt_stream(
            server_stream,
            handler_state,
            handler_connection,
            registration,
            native_context,
        ));
    let mut handler = tokio::spawn(endpoint);

    let outbound_bridge = bridge.clone();
    let mut outbound = tokio::spawn(async move {
        while let Some(lease) = outbound_bridge.recv_from_guest().await {
            let packet = lease.packet();
            if to_server.write_all(packet).await.is_err() {
                outbound_bridge.close_from_guest();
                return false;
            }
            lease.acknowledge();
        }
        to_server.shutdown().await.is_ok()
    });

    let inbound_bridge = bridge.clone();
    let mut inbound = tokio::spawn(async move {
        let mut packet = vec![0_u8; wasmi_host::PACKET_MAX_BYTES];
        loop {
            let length = match from_server.read(&mut packet).await {
                Ok(0) | Err(_) => {
                    inbound_bridge.close_to_guest();
                    return true;
                }
                Ok(length) => length,
            };
            match inbound_bridge.reserve_to_guest(length).await {
                Ok(reservation) => {
                    if reservation.commit(packet[..length].to_vec()).is_err() {
                        inbound_bridge.close_to_guest();
                        return false;
                    }
                }
                Err(wasmi_host::PacketSendError::Closed) => {}
                Err(_) => {
                    inbound_bridge.cancel();
                    return false;
                }
            }
        }
    });

    // Let the guest drain HELLO and the initial snapshot while the public
    // lifecycle remains VALIDATING. Its send ABI still traps until it has
    // actually received INIT.
    if let Err(error) = attempt.start() {
        connection.cancel();
        host_cancel.cancel();
        let outcome = attempt.join().await.unwrap_or_else(|_| {
            AttemptOutcome::Failed(AttemptFailure {
                kind: FailureKind::HostFailure,
                detail: error.to_string(),
            })
        });
        let _ = handler.await;
        let _ = outbound.await;
        let _ = inbound.await;
        return DrivenAttempt {
            outcome,
            handler_closed_first: false,
            connection_failure: connection.failure(),
            running_for: Duration::ZERO,
        };
    }

    let mut join = Box::pin(attempt.join());

    if let Err(error) = publication.publish_running().await {
        connection.cancel();
        host_cancel.cancel();
        let _ = join.await;
        if !handler.is_finished() {
            let _ = (&mut handler).await;
        }
        let _ = (&mut outbound).await;
        let _ = (&mut inbound).await;
        return DrivenAttempt {
            outcome: AttemptOutcome::Failed(error),
            handler_closed_first: false,
            connection_failure: connection.failure(),
            running_for: Duration::ZERO,
        };
    }
    let started_at = Instant::now();

    let (outcome, handler_closed_first) = tokio::select! {
        result = &mut join => {
            let outcome = result.unwrap_or_else(|error| AttemptOutcome::Failed(AttemptFailure {
                kind: FailureKind::HostFailure,
                detail: error.to_string(),
            }));
            (outcome, false)
        }
        _ = &mut handler => {
            host_cancel.cancel();
            let outcome = join.await.unwrap_or_else(|error| AttemptOutcome::Failed(AttemptFailure {
                kind: FailureKind::HostFailure,
                detail: error.to_string(),
            }));
            (outcome, true)
        }
    };
    // A normal guest return seals its send side before the thread result is
    // delivered. The handler may consequently observe the orderly EOF and win
    // this select even though it did not fail first.
    let handler_closed_first =
        handler_closed_first && !matches!(outcome, AttemptOutcome::Returned(_));
    publication.publish_stopping().await;
    let running_for = started_at.elapsed();

    if !matches!(outcome, AttemptOutcome::Returned(_)) || handler_closed_first {
        connection.cancel();
        host_cancel.cancel();
    }
    let _ = (&mut outbound).await;
    if !handler.is_finished() {
        let _ = (&mut handler).await;
    }
    let _ = (&mut inbound).await;
    DrivenAttempt {
        outcome,
        handler_closed_first,
        connection_failure: connection.failure(),
        running_for,
    }
}

fn classify_outcome(
    driven: &DrivenAttempt,
    interrupt: Option<Interrupt>,
) -> (u8, i32, String, bool) {
    if let Some(interrupt) = interrupt {
        return match interrupt {
            Interrupt::Updated | Interrupt::Restarted => (
                EXT_EXIT_UPDATED,
                0,
                "extension definition replaced".into(),
                false,
            ),
            Interrupt::ServerShutdown => (
                EXT_EXIT_SERVER_SHUTDOWN,
                0,
                "server is shutting down".into(),
                false,
            ),
            Interrupt::Cancelled | Interrupt::Disabled | Interrupt::OwnerClosed => {
                (EXT_EXIT_CANCELLED, 0, "extension cancelled".into(), false)
            }
        };
    }
    if driven.connection_failure == Some(super::ConnectionFailure::SlowConsumer) {
        return (
            EXT_EXIT_SLOW_CONSUMER,
            0,
            "extension did not drain its output".into(),
            true,
        );
    }
    if driven.connection_failure == Some(super::ConnectionFailure::ResourceLimit) {
        return (
            EXT_EXIT_RESOURCE_LIMIT,
            0,
            "extension native-job resource limit exceeded".into(),
            true,
        );
    }
    if driven.handler_closed_first {
        return (
            EXT_EXIT_PROTOCOL_VIOLATION,
            0,
            "logical client connection closed before the guest returned".into(),
            true,
        );
    }
    match &driven.outcome {
        AttemptOutcome::Returned(code) => (EXT_EXIT_RETURNED, *code, String::new(), *code != 0),
        AttemptOutcome::Cancelled => (EXT_EXIT_CANCELLED, 0, "extension cancelled".into(), false),
        AttemptOutcome::Failed(error) => match error.kind {
            FailureKind::AbiMisuse => (EXT_EXIT_PROTOCOL_VIOLATION, 0, error.detail.clone(), true),
            FailureKind::Trap => (EXT_EXIT_TRAPPED, 0, error.detail.clone(), true),
            FailureKind::Validation | FailureKind::Instantiation | FailureKind::HostFailure => {
                (EXT_EXIT_HOST_FAILURE, 0, error.detail.clone(), true)
            }
        },
    }
}

fn allocate_task_id(inner: &ServiceState) -> Option<u32> {
    for _ in 0..64 {
        let mut bytes = [0; 4];
        getrandom::fill(&mut bytes).ok()?;
        let task_id = u32::from_le_bytes(bytes);
        if task_id != 0 && !inner.task_ids.contains(&task_id) {
            return Some(task_id);
        }
    }
    None
}

fn backoff_duration(failure_count: u32) -> Duration {
    let exponent = failure_count.saturating_sub(1).min(16);
    let cap = BACKOFF_BASE
        .checked_mul(1_u32 << exponent)
        .unwrap_or(BACKOFF_MAX)
        .min(BACKOFF_MAX);
    let range = u64::try_from(cap.as_nanos())
        .unwrap_or(u64::MAX - 1)
        .saturating_add(1);
    let rejection_floor = u64::MAX - (u64::MAX % range);
    loop {
        let mut random = [0_u8; 8];
        if getrandom::fill(&mut random).is_err() {
            return cap;
        }
        let sample = u64::from_le_bytes(random);
        if sample < rejection_floor {
            return Duration::from_nanos(sample % range);
        }
    }
}

fn definition_from_persistent(value: PersistentDefinition) -> Definition {
    let waiting_for_backoff = !value.blocked
        && value.next_start_unix_ms > unix_millis_now()
        && value.flags & (EXT_FLAG_ENABLED | EXT_FLAG_DESIRED_RUNNING)
            == EXT_FLAG_ENABLED | EXT_FLAG_DESIRED_RUNNING;
    Definition {
        extension_id: value.extension_id,
        definition_revision: value.definition_revision,
        flags: value.flags,
        restart: value.restart,
        native_runtime: yas_wire::schema::extension::RUNTIME_AUTO as u8,
        hash: value.hash,
        name: value.name,
        args: None,
        argument_bytes: value.argument_bytes,
        argument_reservation: None,
        owner_endpoint: None,
        phase: if value.blocked {
            EXT_PHASE_BLOCKED
        } else if waiting_for_backoff {
            EXT_PHASE_BACKOFF
        } else {
            EXT_PHASE_STOPPED
        },
        attempt: value.attempt,
        last_running_attempt: value.last_running_attempt,
        task_id: 0,
        next_start_unix_ms: if waiting_for_backoff {
            value.next_start_unix_ms
        } else {
            0
        },
        detail: if value.blocked {
            value.blocked_detail
        } else {
            String::new()
        },
        next_output_sequence: 1,
        retained: VecDeque::new(),
        terminal_replay: VecDeque::new(),
        retained_bytes: 0,
        followers: HashMap::new(),
        pending_deadline: None,
        release_deadline: None,
        generation: 1,
        failure_count: value.failure_count,
        interrupt: None,
        control: None,
        object_pinned: false,
        catalog_committed: true,
        wake: Arc::new(Notify::new()),
    }
}

fn release_definition_arguments(definition: &mut Definition) {
    definition.args = None;
    definition.argument_reservation = None;
}

fn allocate_extension_id(inner: &ServiceState) -> Option<u64> {
    for _ in 0..64 {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).ok()?;
        let extension_id = u64::from_le_bytes(bytes);
        if extension_id != 0 && !inner.definitions.contains_key(&extension_id) {
            return Some(extension_id);
        }
    }
    None
}

fn commit_transient_create(
    inner: &mut ServiceState,
    definition: &mut Definition,
) -> Result<(), (u8, String)> {
    let store = inner
        .store
        .as_mut()
        .ok_or((EXT_STATUS_OTHER, "object store is unavailable".into()))?;
    store
        .pin(&definition.hash)
        .map_err(|error| (object_status(&error), error.to_string()))?;
    definition.object_pinned = true;
    if definition.persistent() {
        store.unpin(&definition.hash);
        definition.object_pinned = false;
        return Err((
            EXT_STATUS_OTHER,
            "persistent creation requires the catalog lane".into(),
        ));
    }
    Ok(())
}

fn repair_persistent_pin(
    inner: &mut ServiceState,
    current: &Definition,
) -> Result<(), CatalogError> {
    if current.object_pinned {
        return Ok(());
    }
    let store = inner.store.as_mut().ok_or(CatalogError::Unavailable)?;
    store
        .pin(&current.hash)
        .map_err(|error| CatalogError::Storage(error.to_string()))?;
    let definition = inner
        .definitions
        .get_mut(&current.extension_id)
        .ok_or(CatalogError::NotFound)?;
    definition.object_pinned = true;
    Ok(())
}

fn stop_invalid_pending_locked(
    inner: &mut ServiceState,
    hash: ObjectHash,
    detail: &str,
    terminal_retain: Duration,
) -> Vec<u64> {
    let ids = inner
        .definitions
        .values()
        .filter(|definition| definition.hash == hash && definition.phase == EXT_PHASE_NEED_OBJECT)
        .map(|definition| definition.extension_id)
        .collect::<Vec<_>>();
    let now = Instant::now();
    for extension_id in &ids {
        if let Some(definition) = inner.definitions.get_mut(extension_id) {
            definition.phase = EXT_PHASE_STOPPED;
            definition.set_flag(EXT_FLAG_DESIRED_RUNNING, false);
            definition.pending_deadline = None;
            definition.release_deadline = Some(now + terminal_retain);
            definition.generation = definition.generation.saturating_add(1);
            definition.detail = bounded_detail(detail);
            definition.wake.notify_waiters();
            release_definition_arguments(definition);
        }
    }
    ids
}

fn notify_need_object_locked(
    inner: &mut ServiceState,
    hashes: &[ObjectHash],
    output_retain_max: usize,
) {
    if hashes.is_empty() {
        return;
    }
    let ids = inner
        .definitions
        .values()
        .filter(|definition| {
            definition.phase == EXT_PHASE_NEED_OBJECT && hashes.contains(&definition.hash)
        })
        .map(|definition| definition.extension_id)
        .collect::<Vec<_>>();
    for extension_id in ids {
        emit_lifecycle_locked(inner, extension_id, output_retain_max);
    }
}

fn expire_pending_locked(
    inner: &mut ServiceState,
    now: Instant,
    output_retain_max: usize,
    terminal_retain: Duration,
) {
    let ids = inner
        .definitions
        .values()
        .filter(|definition| {
            definition.phase == EXT_PHASE_NEED_OBJECT
                && definition
                    .pending_deadline
                    .is_some_and(|deadline| now >= deadline)
        })
        .map(|definition| definition.extension_id)
        .collect::<Vec<_>>();
    for extension_id in ids {
        if let Some(definition) = inner.definitions.get_mut(&extension_id) {
            definition.phase = EXT_PHASE_STOPPED;
            definition.set_flag(EXT_FLAG_DESIRED_RUNNING, false);
            definition.pending_deadline = None;
            definition.release_deadline = Some(now + terminal_retain);
            definition.generation = definition.generation.saturating_add(1);
            definition.detail = "pending extension creation expired".into();
            definition.wake.notify_waiters();
            release_definition_arguments(definition);
        }
        emit_lifecycle_locked(inner, extension_id, output_retain_max);
    }
}

fn release_expired_pending_locked(inner: &mut ServiceState, now: Instant) {
    let ids = inner
        .definitions
        .values()
        .filter(|definition| {
            definition.control.is_none()
                && definition
                    .release_deadline
                    .is_some_and(|deadline| now >= deadline)
        })
        .map(|definition| definition.extension_id)
        .collect::<Vec<_>>();
    for extension_id in ids {
        force_terminal_replay_locked(inner, extension_id);
        remove_definition_locked(inner, extension_id);
    }
}

/// At the replay lease boundary, bypass the network soft production gate once
/// for the compact terminal pair and its marker. Extension-origin followers
/// still pass through their hard outbox reservation, which cancels a slow
/// consumer instead of admitting unbounded retained output.
fn force_terminal_replay_locked(inner: &mut ServiceState, extension_id: u64) {
    let endpoints = inner
        .definitions
        .get(&extension_id)
        .map(|definition| definition.followers.keys().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    for endpoint in endpoints {
        if !inner.endpoints.contains_key(&endpoint) {
            continue;
        }
        let Some((through, cursor, mut records)) =
            inner.definitions.get(&extension_id).and_then(|definition| {
                let follower = definition.followers.get(&endpoint)?;
                let through = follower.replay_through?;
                let records = definition
                    .terminal_replay
                    .iter()
                    .filter(|record| {
                        record.sequence >= follower.next_sequence && record.sequence <= through
                    })
                    .map(|record| (record.sequence, Arc::clone(&record.item)))
                    .collect::<Vec<_>>();
                Some((through, follower.next_sequence, records))
            })
        else {
            continue;
        };
        records.sort_by_key(|(sequence, _)| *sequence);
        records.dedup_by_key(|(sequence, _)| *sequence);
        let mut next_sequence = cursor;
        let mut open = true;
        for (sequence, item) in records {
            if !try_deliver_endpoint_locked(
                inner,
                endpoint,
                BackendEvent::Retained {
                    extension_handle: extension_id,
                    sequence,
                    item,
                },
            ) {
                open = false;
                break;
            }
            next_sequence = sequence.saturating_add(1);
        }
        if open {
            open = try_deliver_endpoint_locked(
                inner,
                endpoint,
                BackendEvent::ReplayDone {
                    extension_handle: extension_id,
                    through_sequence: through,
                },
            );
        }
        if open
            && let Some(follower) = inner
                .definitions
                .get_mut(&extension_id)
                .and_then(|definition| definition.followers.get_mut(&endpoint))
        {
            follower.next_sequence = next_sequence.max(through.saturating_add(1));
            follower.replay_through = None;
        }
    }
}

fn remove_definition_locked(inner: &mut ServiceState, extension_id: u64) {
    if let Some(mut definition) = inner.definitions.remove(&extension_id) {
        if definition.object_pinned
            && let Some(store) = inner.store.as_mut()
        {
            store.unpin(&definition.hash);
        }
        release_definition_arguments(&mut definition);
        inner.retained_bytes = inner
            .retained_bytes
            .saturating_sub(definition.retained_bytes);
        inner.commands.invalidate_extension(extension_id);
    }
    inner.supervisors.remove(&extension_id);
}

fn follower_capacity_available(
    inner: &ServiceState,
    endpoint: u64,
    per_endpoint_limit: usize,
    global_limit: usize,
) -> bool {
    let mut endpoint_count = 0usize;
    let mut global_count = 0usize;
    for definition in inner.definitions.values() {
        global_count = global_count.saturating_add(definition.followers.len());
        endpoint_count = endpoint_count
            .saturating_add(usize::from(definition.followers.contains_key(&endpoint)));
    }
    endpoint_count < per_endpoint_limit && global_count < global_limit
}

fn mutate_lifecycle_locked(
    inner: &mut ServiceState,
    extension_id: u64,
    enabled: Option<bool>,
    desired: Option<bool>,
    interrupt: Interrupt,
    terminal_retain: Duration,
) -> Result<(), CatalogError> {
    let Some(current) = inner.definitions.get(&extension_id).cloned() else {
        return Err(CatalogError::NotFound);
    };
    let pending_creation = current.phase == EXT_PHASE_NEED_OBJECT && !current.catalog_committed;
    if let Some(definition) = inner.definitions.get_mut(&extension_id) {
        if let Some(enabled) = enabled {
            definition.set_flag(EXT_FLAG_ENABLED, enabled);
        }
        if let Some(desired) = desired {
            definition.set_flag(EXT_FLAG_DESIRED_RUNNING, desired);
        }
        definition.generation = definition.generation.saturating_add(1);
        definition.interrupt = Some(interrupt);
        definition.next_start_unix_ms = 0;
        definition.wake.notify_waiters();
        if pending_creation {
            if !definition.enabled() || !definition.desired() {
                definition.phase = EXT_PHASE_STOPPED;
                definition.pending_deadline = None;
                definition.release_deadline = Some(Instant::now() + terminal_retain);
                definition.task_id = 0;
                release_definition_arguments(definition);
            } else {
                definition.phase = EXT_PHASE_NEED_OBJECT;
            }
        } else if definition.control.is_some() {
            definition.phase = EXT_PHASE_STOPPING;
            definition.task_id = 0;
        } else if definition.enabled() && definition.desired() {
            definition.phase = EXT_PHASE_QUEUED;
        } else {
            definition.phase = EXT_PHASE_STOPPED;
        }
    }
    if let Some(control) = current.control.as_ref() {
        inner.commands.invalidate_attempt(
            extension_id,
            control.definition_revision,
            control.attempt,
        );
    }
    if enabled == Some(false) {
        inner
            .commands
            .invalidate_definition(extension_id, current.definition_revision);
    }
    Ok(())
}

fn retain_and_fanout(
    inner: &mut ServiceState,
    extension_id: u64,
    item: RetainedItem,
    global_limit: usize,
) -> Option<RetainedRecord> {
    retain_record(inner, extension_id, item, global_limit, false)
}

fn retain_record(
    inner: &mut ServiceState,
    extension_id: u64,
    mut item: RetainedItem,
    global_limit: usize,
    compact_terminal: bool,
) -> Option<RetainedRecord> {
    debug_assert_eq!(inner.output_budget.max, global_limit);
    let bytes = item.charged_bytes;
    let (sequence, clock) = {
        let definition = inner.definitions.get_mut(&extension_id)?;
        let Some(next_sequence) = definition.next_output_sequence.checked_add(1) else {
            definition.phase = EXT_PHASE_BLOCKED;
            definition.detail = "extension output sequence exhausted".into();
            return None;
        };
        let sequence = definition.next_output_sequence;
        definition.next_output_sequence = next_sequence;
        inner.retention_clock = inner.retention_clock.saturating_add(1);
        while bytes <= OUTPUT_RETAIN_PER_EXTENSION
            && definition.retained_bytes.saturating_add(bytes) > OUTPUT_RETAIN_PER_EXTENSION
        {
            let Some(evicted) = definition.retained.pop_front() else {
                break;
            };
            definition.retained_bytes = definition
                .retained_bytes
                .saturating_sub(evicted.item.charged_bytes);
            inner.retained_bytes = inner
                .retained_bytes
                .saturating_sub(evicted.item.charged_bytes);
        }
        (sequence, inner.retention_clock)
    };

    let reservation = if bytes <= OUTPUT_RETAIN_PER_EXTENSION {
        loop {
            if let Some(reservation) = inner.output_budget.try_reserve(bytes) {
                break Some(reservation);
            }
            if !evict_oldest_history(inner) {
                break None;
            }
        }
    } else {
        None
    };
    if reservation.is_none() && !compact_terminal {
        return None;
    }
    let retained = reservation.is_some();
    item._reservation = reservation;
    let record = RetainedRecord {
        sequence,
        clock,
        item: Arc::new(item),
    };
    if retained {
        let definition = inner.definitions.get_mut(&extension_id)?;
        definition.retained.push_back(record.clone());
        definition.retained_bytes = definition.retained_bytes.saturating_add(bytes);
        inner.retained_bytes = inner.retained_bytes.saturating_add(bytes);
    }
    wake_followers_locked(inner, extension_id);
    Some(record)
}

#[derive(Clone, Copy)]
enum TerminalRecordKind {
    Exit,
    Status,
}

fn retain_terminal_and_fanout(
    inner: &mut ServiceState,
    extension_id: u64,
    item: RetainedItem,
    global_limit: usize,
    kind: TerminalRecordKind,
) -> Option<u64> {
    let persistent = inner.definitions.get(&extension_id)?.persistent();
    let record = retain_record(inner, extension_id, item, global_limit, !persistent)?;
    let definition = inner.definitions.get_mut(&extension_id)?;
    if persistent {
        return Some(record.sequence);
    }
    match kind {
        TerminalRecordKind::Exit => definition.terminal_replay.clear(),
        TerminalRecordKind::Status => definition
            .terminal_replay
            .retain(|record| matches!(record.item.kind, RetainedItemKind::Exit(_))),
    }
    definition.terminal_replay.push_back(record);
    while definition.terminal_replay.len() > 2 {
        definition.terminal_replay.pop_front();
    }
    Some(
        definition
            .terminal_replay
            .back()
            .expect("just inserted compact terminal record")
            .sequence,
    )
}

fn oldest_replay_sequence(definition: &Definition) -> u64 {
    definition
        .retained
        .iter()
        .chain(definition.terminal_replay.iter())
        .map(|record| record.sequence)
        .min()
        .unwrap_or(definition.next_output_sequence)
}

fn fanout_replay_done(inner: &mut ServiceState, extension_id: u64, through: u64) {
    if let Some(definition) = inner.definitions.get_mut(&extension_id) {
        for follower in definition.followers.values_mut() {
            follower.replay_through = Some(
                follower
                    .replay_through
                    .map_or(through, |pending| pending.max(through)),
            );
        }
    }
    wake_followers_locked(inner, extension_id);
}

fn evict_oldest_history(inner: &mut ServiceState) -> bool {
    let oldest = inner
        .definitions
        .iter()
        .filter_map(|(extension_id, definition)| {
            definition
                .retained
                .front()
                .map(|record| (*extension_id, record.clock))
        })
        .min_by_key(|(_, clock)| *clock)
        .map(|(extension_id, _)| extension_id);
    let Some(extension_id) = oldest else {
        return false;
    };
    let Some(definition) = inner.definitions.get_mut(&extension_id) else {
        return false;
    };
    let Some(evicted) = definition.retained.pop_front() else {
        return false;
    };
    definition.retained_bytes = definition
        .retained_bytes
        .saturating_sub(evicted.item.charged_bytes);
    inner.retained_bytes = inner
        .retained_bytes
        .saturating_sub(evicted.item.charged_bytes);
    true
}

fn next_replay_record(
    definition: &Definition,
    cursor: u64,
    through: u64,
) -> Option<&RetainedRecord> {
    definition
        .retained
        .iter()
        .chain(definition.terminal_replay.iter())
        .filter(|record| record.sequence >= cursor && record.sequence <= through)
        .min_by_key(|record| record.sequence)
}

fn next_replay_sequence(definition: &Definition, cursor: u64, through: u64) -> Option<u64> {
    next_replay_record(definition, cursor, through).map(|record| record.sequence)
}

/// Admit one backend event without ever waiting while the service lock is
/// held. A full endpoint is a deterministic slow-consumer failure: remove its
/// only service-owned sender and wake its scheduler. Once outstanding sender
/// clones drop, the route observes EOF, unregisters the endpoint, and clearing
/// its pending oneshots wakes every request with `Closed`.
fn try_deliver_endpoint_locked(
    inner: &mut ServiceState,
    endpoint: u64,
    event: BackendEvent,
) -> bool {
    let Some(sender) = inner.endpoints.get(&endpoint).cloned() else {
        return false;
    };
    if sender.try_send(event).is_ok() {
        return true;
    }
    inner.endpoints.remove(&endpoint);
    if let Some(wake) = inner.endpoint_wakes.remove(&endpoint) {
        wake.notify_one();
    }
    false
}

fn wake_endpoint_locked(inner: &ServiceState, endpoint: u64) {
    if let Some(wake) = inner.endpoint_wakes.get(&endpoint) {
        wake.notify_one();
    }
}

fn wake_followers_locked(inner: &ServiceState, extension_id: u64) {
    let Some(definition) = inner.definitions.get(&extension_id) else {
        return;
    };
    for endpoint in definition.followers.keys() {
        wake_endpoint_locked(inner, *endpoint);
    }
}

enum ScheduleOutcome {
    Sent(u64),
    Idle,
    Closed,
}

fn schedule_one_locked(
    inner: &mut ServiceState,
    endpoint: u64,
    last_extension: Option<u64>,
) -> ScheduleOutcome {
    if !inner.endpoints.contains_key(&endpoint) {
        return ScheduleOutcome::Closed;
    }
    let mut followed = inner
        .definitions
        .iter()
        .filter(|(_, definition)| definition.followers.contains_key(&endpoint))
        .map(|(extension_id, _)| *extension_id)
        .collect::<Vec<_>>();
    followed.sort_unstable();
    let start = last_extension
        .and_then(|last| {
            followed
                .iter()
                .position(|extension_id| *extension_id > last)
        })
        .unwrap_or(0);
    for offset in 0..followed.len() {
        let extension_id = followed[(start + offset) % followed.len()];
        let Some((event, follower)) = inner.definitions.get(&extension_id).and_then(|definition| {
            let mut follower = definition.followers.get(&endpoint).copied()?;
            follower.next_sequence = follower
                .next_sequence
                .max(oldest_replay_sequence(definition));
            let through = follower.replay_through.unwrap_or(u64::MAX);
            if let Some(record) = next_replay_record(definition, follower.next_sequence, through) {
                let sequence = record.sequence;
                follower.next_sequence = sequence.saturating_add(1);
                return Some((
                    BackendEvent::Retained {
                        extension_handle: extension_id,
                        sequence,
                        item: Arc::clone(&record.item),
                    },
                    follower,
                ));
            }
            follower.replay_through.map(|through| {
                follower.next_sequence = follower.next_sequence.max(through.saturating_add(1));
                follower.replay_through = None;
                (
                    BackendEvent::ReplayDone {
                        extension_handle: extension_id,
                        through_sequence: through,
                    },
                    follower,
                )
            })
        }) else {
            continue;
        };
        if !try_deliver_endpoint_locked(inner, endpoint, event) {
            return ScheduleOutcome::Closed;
        }
        if let Some(definition) = inner.definitions.get_mut(&extension_id) {
            definition.followers.insert(endpoint, follower);
        }
        return ScheduleOutcome::Sent(extension_id);
    }
    ScheduleOutcome::Idle
}

fn emit_lifecycle_locked(inner: &mut ServiceState, extension_id: u64, global_limit: usize) {
    let Some(definition) = inner.definitions.get(&extension_id) else {
        return;
    };
    let compact_terminal = !definition.persistent()
        && matches!(definition.phase, EXT_PHASE_STOPPED | EXT_PHASE_BLOCKED);
    let item = RetainedItem::status(
        definition
            .detail
            .len()
            .saturating_add(definition.name.len())
            .saturating_add(128),
    );
    if compact_terminal {
        if let Some(through) = retain_terminal_and_fanout(
            inner,
            extension_id,
            item,
            global_limit,
            TerminalRecordKind::Status,
        ) {
            fanout_replay_done(inner, extension_id, through);
        }
    } else {
        if let Some(definition) = inner.definitions.get_mut(&extension_id) {
            definition.terminal_replay.clear();
        }
        retain_and_fanout(inner, extension_id, item, global_limit);
    }
}

fn status_event(
    definition: &Definition,
    nonce: u16,
    status: u8,
    phase_override: Option<u8>,
    detail: &str,
) -> BackendEvent {
    status_event_with_replay(definition, nonce, status, phase_override, 0, detail)
}

fn attach_status_event(
    definition: &Definition,
    nonce: u16,
    replay_from_sequence: u64,
    detail: &str,
) -> BackendEvent {
    status_event_with_replay(
        definition,
        nonce,
        EXT_STATUS_OK,
        None,
        replay_from_sequence,
        detail,
    )
}

fn status_event_with_replay(
    definition: &Definition,
    nonce: u16,
    status: u8,
    phase_override: Option<u8>,
    replay_from_sequence: u64,
    detail: &str,
) -> BackendEvent {
    let _ = phase_override;
    let reply = if status == EXT_STATUS_OK {
        NativeReply::Status(NativeStatus {
            extension_handle: definition.extension_id,
            definition_revision: definition.definition_revision,
            replay_from_sequence,
            output_sequence: definition.latest_output_sequence(),
        })
    } else {
        NativeReply::Error(native_status_failure(status, detail))
    };
    BackendEvent::Reply { nonce, reply }
}

#[allow(clippy::too_many_arguments)]
fn fixed_status(
    nonce: u16,
    status: u8,
    flags: u8,
    restart: u8,
    extension_id: u64,
    definition_revision: u64,
    hash: ObjectHash,
    detail: &str,
) -> BackendEvent {
    let _ = (flags, restart, hash);
    let reply = if status == EXT_STATUS_OK {
        NativeReply::Status(NativeStatus {
            extension_handle: extension_id,
            definition_revision,
            replay_from_sequence: 0,
            output_sequence: 0,
        })
    } else {
        NativeReply::Error(native_status_failure(status, detail))
    };
    BackendEvent::Reply { nonce, reply }
}

fn run_error_status(nonce: u16, status: u8, hash: ObjectHash, detail: &str) -> BackendEvent {
    fixed_status(nonce, status, 0, 0, 0, 0, hash, detail)
}

fn creation_status(definition: &Definition, nonce: u16, detail: &str) -> BackendEvent {
    status_event(definition, nonce, EXT_STATUS_OK, None, detail)
}

fn update_operation_status(
    definition: &Definition,
    nonce: u16,
    status: u8,
    phase: u8,
    hash: ObjectHash,
    restart: u8,
    detail: &str,
) -> BackendEvent {
    let _ = (phase, hash, restart);
    status_event(definition, nonce, status, None, detail)
}

fn put_status(
    nonce: u16,
    status: u8,
    hash: ObjectHash,
    received: u64,
    detail: &str,
) -> BackendEvent {
    let _ = hash;
    let reply = match status {
        EXT_STATUS_OK => NativeReply::Put(NativePutDisposition::Accepted { received }),
        EXT_PUT_ALREADY_HAVE => {
            NativeReply::Put(NativePutDisposition::AlreadyPresent { size: received })
        }
        _ => NativeReply::Error(native_status_failure(status, detail)),
    };
    BackendEvent::Reply { nonce, reply }
}

fn bounded_detail(detail: &str) -> String {
    if detail.len() <= EXT_MAX_DETAIL {
        return detail.to_owned();
    }
    let mut end = EXT_MAX_DETAIL;
    while !detail.is_char_boundary(end) {
        end -= 1;
    }
    detail[..end].to_owned()
}

fn catalog_status(error: &CatalogError) -> u8 {
    match error {
        CatalogError::Unavailable => EXT_STATUS_PERMISSION,
        CatalogError::Invalid(_) => EXT_STATUS_INVALID,
        CatalogError::Conflict => EXT_STATUS_CONFLICT,
        CatalogError::NotFound => EXT_STATUS_NOT_FOUND,
        CatalogError::Budget => EXT_STATUS_BUDGET,
        CatalogError::Storage(_) => EXT_STATUS_OTHER,
    }
}

fn mark_hash_unpinned(inner: &mut ServiceState, hash: &ObjectHash) {
    for definition in inner
        .definitions
        .values_mut()
        .filter(|definition| &definition.hash == hash)
    {
        definition.object_pinned = false;
    }
}

fn object_status(error: &ObjectStoreError) -> u8 {
    match error {
        ObjectStoreError::InvalidConfig(_) | ObjectStoreError::InvalidUpload(_) => {
            EXT_STATUS_INVALID
        }
        ObjectStoreError::NotFound => EXT_STATUS_NOT_FOUND,
        ObjectStoreError::Conflict => EXT_STATUS_CONFLICT,
        ObjectStoreError::TooLarge => EXT_STATUS_TOO_LARGE,
        ObjectStoreError::Budget => EXT_STATUS_BUDGET,
        ObjectStoreError::HashMismatch | ObjectStoreError::InvalidModule(_) => EXT_STATUS_INVALID,
        ObjectStoreError::Io(_) => EXT_STATUS_OTHER,
    }
}

fn command_owner(
    inner: &ServiceState,
    endpoint: u64,
    endpoint_generation: u64,
    identity: (u64, u64, u64, u32),
) -> Option<CommandOwner> {
    let definition = inner.definitions.get(&identity.0)?;
    let control = definition.control.as_ref()?;
    Some(CommandOwner {
        endpoint_id: endpoint,
        endpoint_generation,
        extension_id: definition.extension_id,
        definition_revision: definition.definition_revision,
        attempt: identity.2,
        hash: definition.hash,
        name: definition.name.clone(),
        persistent: definition.persistent(),
        enabled: definition.enabled(),
        running: definition.phase == EXT_PHASE_RUNNING
            && definition.definition_revision == identity.1
            && control.attempt == identity.2
            && control.task_id == identity.3,
    })
}

fn command_listener(
    endpoint_generation: u64,
    listener: crate::channel::ListenerSnapshot,
) -> CommandListener {
    CommandListener {
        endpoint_id: listener.endpoint,
        endpoint_generation,
        listener_id: listener.registry_id,
        listener_generation: listener.generation,
        name: listener.name,
        token: listener.token,
    }
}

fn registration_native_error(error: command_directory::RegistrationError) -> NativeMutationFailure {
    match error {
        command_directory::RegistrationError::Permission => NativeMutationFailure::Permission,
        command_directory::RegistrationError::NotFound => NativeMutationFailure::NotFound,
        command_directory::RegistrationError::Invalid => {
            NativeMutationFailure::Invalid(error.detail().to_owned())
        }
        command_directory::RegistrationError::Conflict => NativeMutationFailure::Conflict,
        command_directory::RegistrationError::Budget => NativeMutationFailure::ResourceExhausted,
    }
}

fn host_running_default() -> usize {
    std::thread::available_parallelism()
        .map(|cpus| cpus.get().saturating_sub(1).clamp(1, DEFAULT_MAX_RUNNING))
        .unwrap_or(1)
}

fn encoded_argument_bytes(args: &[Vec<u8>]) -> usize {
    args.iter().fold(2usize, |total, argument| {
        total.saturating_add(4 + argument.len())
    })
}

fn encoded_borrowed_argument_bytes(args: &[&[u8]]) -> usize {
    args.iter().fold(2usize, |total, argument| {
        total.saturating_add(4 + argument.len())
    })
}

fn unix_millis_after(duration: Duration) -> u64 {
    unix_millis_now().saturating_add(duration.as_millis().min(u64::MAX as u128) as u64)
}

fn unix_millis_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root(label: &str) -> std::path::PathBuf {
        let mut random = [0; 8];
        getrandom::fill(&mut random).unwrap();
        std::env::temp_dir().join(format!(
            "yas-extension-service-{label}-{:016x}",
            u64::from_le_bytes(random)
        ))
    }

    #[tokio::test]
    async fn native_endpoint_flood_closes_the_route_and_wakes_pending_replies() {
        let root = temporary_root("native-endpoint-flood");
        let service = ExtensionService::persistent_for_test(&root);
        let endpoint = 0xfedc_ba98_7654_3210;
        let (sender, mut receiver) = mpsc::channel(NATIVE_ENDPOINT_QUEUE);
        {
            let mut inner = service.inner.lock().await;
            inner.endpoints.insert(endpoint, sender.clone());
            inner
                .endpoint_wakes
                .insert(endpoint, Arc::new(Notify::new()));
            for sequence in 0..NATIVE_ENDPOINT_QUEUE {
                assert!(try_deliver_endpoint_locked(
                    &mut inner,
                    endpoint,
                    BackendEvent::ReplayDone {
                        extension_handle: 0x12,
                        through_sequence: sequence as u64,
                    },
                ));
            }
            assert!(!try_deliver_endpoint_locked(
                &mut inner,
                endpoint,
                BackendEvent::ReplayDone {
                    extension_handle: 0x12,
                    through_sequence: NATIVE_ENDPOINT_QUEUE as u64,
                },
            ));
            assert!(!inner.endpoints.contains_key(&endpoint));
            assert!(!inner.endpoint_wakes.contains_key(&endpoint));
        }

        drop(sender);
        for _ in 0..NATIVE_ENDPOINT_QUEUE {
            assert!(receiver.recv().await.is_some());
        }
        assert!(receiver.recv().await.is_none());

        let (reply_sender, reply_receiver) = oneshot::channel();
        let pending = std::sync::Mutex::new(HashMap::from([(0xbeef, reply_sender)]));
        let follows = std::sync::Mutex::new(HashMap::<u64, NativeFollowRoute>::new());
        clear_native_routes(&pending, &follows);
        assert!(reply_receiver.await.is_err());

        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn native_mutation_replay_is_shared_and_survives_service_reopen() {
        let root = temporary_root("native-mutation-replay");
        let operation_id = [0x44; 16];
        let fingerprint = [0x55; 32];
        let body = vec![1, 2, 3, 4];
        let service = ExtensionService::persistent_for_test(&root);
        {
            let _lane = service.lock_native_mutation().await;
            assert!(matches!(
                service
                    .native_mutation_replay(4, operation_id, fingerprint)
                    .await
                    .unwrap(),
                NativeMutationReplay::Miss
            ));
            service
                .record_native_mutation(4, operation_id, fingerprint, body.clone(), true)
                .await
                .unwrap();
        }
        assert!(matches!(
            service
                .native_mutation_replay(4, operation_id, fingerprint)
                .await
                .unwrap(),
            NativeMutationReplay::Hit(NativeMutationSettlement::Success(value)) if value == body
        ));
        assert!(matches!(
            service
                .native_mutation_replay(4, operation_id, [0x66; 32])
                .await
                .unwrap(),
            NativeMutationReplay::Conflict
        ));
        drop(service);

        let reopened = ExtensionService::persistent_for_test(&root);
        assert!(matches!(
            reopened
                .native_mutation_replay(4, operation_id, fingerprint)
                .await
                .unwrap(),
            NativeMutationReplay::Hit(NativeMutationSettlement::Success(value)) if value == body
        ));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn native_attempt_output_is_retained_only_for_the_exact_active_attempt() {
        let root = temporary_root("native-attempt-output");
        let service = ExtensionService::persistent_for_test(&root);
        let identity = NativeAttemptIdentity {
            extension_handle: 17,
            generation: 23,
            definition_revision: 29,
            attempt: 31,
            task_id: 37,
        };
        {
            let mut definition = definition_from_persistent(PersistentDefinition {
                extension_id: identity.extension_handle,
                definition_revision: identity.definition_revision,
                flags: EXT_FLAG_ENABLED | EXT_FLAG_DESIRED_RUNNING,
                restart: 0,
                attempt: identity.attempt,
                last_running_attempt: identity.attempt,
                failure_count: 0,
                next_start_unix_ms: 0,
                blocked: false,
                blocked_detail: String::new(),
                hash: [0x42; 32],
                name: "output-test".to_owned(),
                argument_bytes: 0,
            });
            definition.generation = identity.generation;
            definition.phase = EXT_PHASE_RUNNING;
            definition.task_id = identity.task_id;
            definition.control = Some(AttemptControl {
                definition_revision: identity.definition_revision,
                attempt: identity.attempt,
                task_id: identity.task_id,
                host: AttemptCancellation {
                    inner: wasmi_host::new_attempt_shared(),
                },
                connection: super::super::ConnectionCancellation::default(),
            });
            service
                .inner
                .lock()
                .await
                .definitions
                .insert(identity.extension_handle, definition);
        }

        assert_eq!(
            service
                .publish_native_attempt_output(identity, NativeOutputKind::Stdout, b"hello")
                .await,
            Ok(1)
        );
        {
            let inner = service.inner.lock().await;
            let retained = &inner.definitions[&identity.extension_handle].retained;
            assert_eq!(retained.len(), 1);
            assert_eq!(retained[0].sequence, 1);
            assert!(matches!(
                &retained[0].item.kind,
                RetainedItemKind::Output {
                    kind: NativeOutputKind::Stdout,
                    attempt: 31,
                    data,
                } if data == b"hello"
            ));
        }

        let stale_identities = [
            NativeAttemptIdentity {
                generation: identity.generation + 1,
                ..identity
            },
            NativeAttemptIdentity {
                definition_revision: identity.definition_revision + 1,
                ..identity
            },
            NativeAttemptIdentity {
                attempt: identity.attempt + 1,
                ..identity
            },
            NativeAttemptIdentity {
                task_id: identity.task_id + 1,
                ..identity
            },
        ];
        for stale in stale_identities {
            assert_eq!(
                service
                    .publish_native_attempt_output(stale, NativeOutputKind::Stderr, b"stale")
                    .await,
                Err(NativeMutationFailure::Conflict)
            );
        }
        assert_eq!(
            service
                .publish_native_attempt_output(
                    NativeAttemptIdentity {
                        extension_handle: identity.extension_handle + 1,
                        ..identity
                    },
                    NativeOutputKind::Stderr,
                    b"missing",
                )
                .await,
            Err(NativeMutationFailure::NotFound)
        );
        let oversized = vec![0; yas_wire::extension::MAX_OUTPUT_RECORD_BYTES + 1];
        assert_eq!(
            service
                .publish_native_attempt_output(identity, NativeOutputKind::Log, &oversized)
                .await,
            Err(NativeMutationFailure::TooLarge)
        );
        assert_eq!(
            service.inner.lock().await.definitions[&identity.extension_handle].next_output_sequence,
            2
        );

        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_job_admission_failure_is_a_resource_limit_exit() {
        let driven = DrivenAttempt {
            outcome: AttemptOutcome::Cancelled,
            handler_closed_first: true,
            connection_failure: Some(super::super::ConnectionFailure::ResourceLimit),
            running_for: Duration::ZERO,
        };
        let (reason, code, detail, failure) = classify_outcome(&driven, None);
        assert_eq!(reason, EXT_EXIT_RESOURCE_LIMIT);
        assert_eq!(code, 0);
        assert!(detail.contains("resource limit"));
        assert!(failure);
    }

    #[test]
    fn native_definition_flags_preserve_enabled_desired_invariant() {
        assert_eq!(yas_definition_flags(EXT_FLAG_DESIRED_RUNNING), 0);
        assert_eq!(
            yas_definition_flags(EXT_FLAG_ENABLED | EXT_FLAG_DESIRED_RUNNING),
            (yas_wire::schema::extension::DEFINITION_ENABLED
                | yas_wire::schema::extension::DEFINITION_DESIRED_RUNNING) as u16
        );
    }
}
