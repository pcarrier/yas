//! Transport-neutral filesystem watch engine.
//!
//! Native backends only produce unreliable hints. A shared reconciler verifies
//! those hints by scanning the filesystem and publishes immutable snapshots;
//! each subscriber derives a bounded, acknowledged stream of typed changes.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod backend;
pub mod ignores;

pub use ignores::{IgnoreSpec, MAX_PATTERNS as MAX_IGNORE_PATTERNS};

const DEFAULT_LATENCY: Duration = Duration::from_millis(20);
const DEFAULT_WINDOW_BYTES: usize = 1024 * 1024;
const DEFAULT_BATCH_TARGET: usize = 64 * 1024;
const DEFAULT_MAX_ENTRIES: usize = 1_000_000;
const ROOT_MESSAGE_QUEUE: usize = 64;
const ROOT_SUBSCRIBER_QUEUE: usize = 1;
const WATCH_ENGINE_QUEUE: usize = 64;

#[derive(Clone, Debug)]
pub struct SyncOptions {
    pub recursive: bool,
    pub content: bool,
    pub cross_filesystem: bool,
    pub latency: Duration,
    pub inline_max: u64,
    pub window_bytes: usize,
    pub batch_target: usize,
    pub max_entries: usize,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            content: false,
            cross_filesystem: false,
            latency: env_duration_ms("YAS_FS_LATENCY_MS", DEFAULT_LATENCY),
            inline_max: env_u64("YAS_FS_INLINE_MAX", 16 * 1024 * 1024),
            window_bytes: env_u64("YAS_FS_WINDOW", DEFAULT_WINDOW_BYTES as u64) as usize,
            batch_target: DEFAULT_BATCH_TARGET,
            max_entries: env_u64("YAS_FS_MAX_ENTRIES", DEFAULT_MAX_ENTRIES as u64) as usize,
        }
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_duration_ms(name: &str, default: Duration) -> Duration {
    Duration::from_millis(env_u64(name, default.as_millis() as u64).clamp(1, 1_000))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hint {
    Dirty(PathBuf),
    Rescan,
}

pub trait BackendHandle: Send {
    fn add_dir(&self, _dir: &Path) -> bool {
        true
    }

    fn watch_outside(&self, _dir: &Path) {}

    fn remove_dir(&self, _dir: &Path) {}

    fn retain_dirs(&self, _keep: &dyn Fn(&Path) -> bool) {}
}

pub struct NoopBackend;

impl BackendHandle for NoopBackend {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootKey {
    pub path: PathBuf,
    pub recursive: bool,
    pub cross_filesystem: bool,
    pub ignores: IgnoreSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenErrorKind {
    NotFound,
    Permission,
    ResourceExhausted,
    Invalid,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenError {
    pub kind: OpenErrorKind,
    pub detail: String,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for OpenError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    RootGone,
    PermissionLost,
    ResourceExhausted,
    BackendFailed,
    ProtocolViolation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchRecord {
    Upsert { path: String },
    Delete { path: String },
    Move { from: String, to: String },
}

impl WatchRecord {
    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Upsert { path } | Self::Delete { path } => path.len().saturating_add(24),
            Self::Move { from, to } => from.len().saturating_add(to.len()).saturating_add(32),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchUpdate {
    pub update_id: u32,
    pub reset: bool,
    pub snapshot_end: bool,
    pub records: Vec<WatchRecord>,
}

impl WatchUpdate {
    fn estimated_bytes(&self) -> usize {
        self.records.iter().fold(24usize, |total, record| {
            total.saturating_add(record.estimated_bytes())
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    Update(WatchUpdate),
    Closed(CloseReason),
}

pub type WatchSink = Box<dyn FnMut(WatchEvent) -> bool + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchCommand {
    Ack(u32),
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    kind: EntryKind,
    size: u64,
    mode: u32,
    modified_ns: i128,
    changed_ns: i128,
    identity: Option<(u64, u64)>,
    link_hash: Option<[u8; 16]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

type Index = BTreeMap<String, Fingerprint>;

#[derive(Clone)]
enum RootUpdate {
    Snapshot(Arc<Index>),
    Closed(CloseReason),
}

enum RootMsg {
    Hint(Hint),
    Subscribe {
        id: u64,
        sender: SyncSender<RootUpdate>,
        latency: Duration,
    },
    Unsubscribe(u64),
}

pub struct SharedRootHandle {
    key: RootKey,
    single: bool,
    sender: SyncSender<RootMsg>,
    dirty_signal: Arc<AtomicBool>,
    closed: Arc<OnceLock<CloseReason>>,
    _backend: Option<backend::WatchBackend>,
}

impl SharedRootHandle {
    pub fn key(&self) -> &RootKey {
        &self.key
    }

    pub fn is_single(&self) -> bool {
        self.single
    }

    pub fn hint_sender(&self) -> HintSender {
        HintSender {
            tx: self.sender.clone(),
            dirty_signal: self.dirty_signal.clone(),
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.get().is_some()
    }
}

#[derive(Clone)]
pub struct HintSender {
    tx: SyncSender<RootMsg>,
    dirty_signal: Arc<AtomicBool>,
}

impl HintSender {
    pub fn send(&self, hint: Hint) -> bool {
        self.dirty_signal.store(true, Ordering::Release);
        match self.tx.try_send(RootMsg::Hint(hint)) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RegistryKey {
    root: RootKey,
    single: bool,
}

type Registry = HashMap<RegistryKey, Weak<SharedRootHandle>>;

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

pub fn open_root(key: RootKey) -> Result<Arc<SharedRootHandle>, OpenError> {
    open_root_inner(key, false, true)
}

pub fn open_root_unwatched(key: RootKey) -> Arc<SharedRootHandle> {
    open_root_inner(key, false, false).expect("unwatched root creation cannot fail")
}

pub fn open_single_root(path: PathBuf) -> Result<Arc<SharedRootHandle>, OpenError> {
    open_root_inner(single_root_key(path), true, true)
}

pub fn open_single_root_unwatched(path: PathBuf) -> Arc<SharedRootHandle> {
    open_root_inner(single_root_key(path), true, false)
        .expect("unwatched single-root creation cannot fail")
}

fn single_root_key(path: PathBuf) -> RootKey {
    RootKey {
        path,
        recursive: false,
        cross_filesystem: false,
        ignores: IgnoreSpec::default(),
    }
}

fn open_root_inner(
    key: RootKey,
    single: bool,
    watched: bool,
) -> Result<Arc<SharedRootHandle>, OpenError> {
    if key.path.as_os_str().is_empty() {
        return Err(OpenError {
            kind: OpenErrorKind::Invalid,
            detail: "empty filesystem root".to_owned(),
        });
    }
    let registry_key = RegistryKey {
        root: key.clone(),
        single,
    };
    {
        let mut roots = registry().lock().unwrap();
        roots.retain(|_, root| root.strong_count() != 0);
        if let Some(existing) = roots
            .get(&registry_key)
            .and_then(Weak::upgrade)
            .filter(|root| !root.is_closed())
        {
            return Ok(existing);
        }
    }

    let metadata = fs::symlink_metadata(&key.path).map_err(open_io_error)?;
    if single && metadata.is_dir() {
        return Err(OpenError {
            kind: OpenErrorKind::Invalid,
            detail: "single-file watch root is a directory".to_owned(),
        });
    }
    if !single && !metadata.is_dir() {
        return Err(OpenError {
            kind: OpenErrorKind::Invalid,
            detail: "filesystem watch root is not a directory".to_owned(),
        });
    }

    let (sender, receiver) = mpsc::sync_channel(ROOT_MESSAGE_QUEUE);
    let dirty_signal = Arc::new(AtomicBool::new(false));
    let backend = if watched {
        let hints = HintSender {
            tx: sender.clone(),
            dirty_signal: dirty_signal.clone(),
        };
        let watch_path = if single {
            key.path.parent().ok_or_else(|| OpenError {
                kind: OpenErrorKind::Invalid,
                detail: "single-file watch root has no parent".to_owned(),
            })?
        } else {
            &key.path
        };
        let recursive = key.recursive && !single;
        let per_dir = backend::per_dir_watching_pays(recursive, single, !key.ignores.is_empty());
        Some(
            backend::watch(watch_path, recursive, per_dir, hints)
                .map_err(|error| open_notify_error(&error))?,
        )
    } else {
        None
    };
    let registrar: Box<dyn BackendHandle> = backend.as_ref().map_or_else(
        || Box::new(NoopBackend) as Box<dyn BackendHandle>,
        backend::WatchBackend::registrar,
    );

    let mut roots = registry().lock().unwrap();
    roots.retain(|_, root| root.strong_count() != 0);
    if let Some(existing) = roots
        .get(&registry_key)
        .and_then(Weak::upgrade)
        .filter(|root| !root.is_closed())
    {
        return Ok(existing);
    }
    let closed = Arc::new(OnceLock::new());
    let handle = Arc::new(SharedRootHandle {
        key: key.clone(),
        single,
        sender,
        dirty_signal: dirty_signal.clone(),
        closed: closed.clone(),
        _backend: backend,
    });
    std::thread::Builder::new()
        .name("yas-fs-watch-root".to_owned())
        .spawn(move || {
            Reconciler::new(key, single, receiver, registrar, closed, dirty_signal).run()
        })
        .expect("spawn filesystem watch reconciler");
    roots.insert(registry_key, Arc::downgrade(&handle));
    Ok(handle)
}

fn open_io_error(error: io::Error) -> OpenError {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => OpenErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => OpenErrorKind::Permission,
        _ => OpenErrorKind::Io,
    };
    OpenError {
        kind,
        detail: error.to_string(),
    }
}

fn open_notify_error(error: &notify::Error) -> OpenError {
    let kind = match &error.kind {
        notify::ErrorKind::MaxFilesWatch => OpenErrorKind::ResourceExhausted,
        notify::ErrorKind::PathNotFound => OpenErrorKind::NotFound,
        notify::ErrorKind::Io(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            OpenErrorKind::Permission
        }
        notify::ErrorKind::Io(error) if matches!(error.raw_os_error(), Some(23 | 24 | 28)) => {
            OpenErrorKind::ResourceExhausted
        }
        _ => OpenErrorKind::Io,
    };
    OpenError {
        kind,
        detail: error.to_string(),
    }
}

struct Subscriber {
    sender: SyncSender<RootUpdate>,
    latency: Duration,
}

struct Reconciler {
    key: RootKey,
    single: bool,
    receiver: Receiver<RootMsg>,
    registrar: Box<dyn BackendHandle>,
    closed: Arc<OnceLock<CloseReason>>,
    dirty_signal: Arc<AtomicBool>,
    subscribers: HashMap<u64, Subscriber>,
    current: Arc<Index>,
}

impl Reconciler {
    fn new(
        key: RootKey,
        single: bool,
        receiver: Receiver<RootMsg>,
        registrar: Box<dyn BackendHandle>,
        closed: Arc<OnceLock<CloseReason>>,
        dirty_signal: Arc<AtomicBool>,
    ) -> Self {
        Self {
            key,
            single,
            receiver,
            registrar,
            closed,
            dirty_signal,
            subscribers: HashMap::new(),
            current: Arc::new(Index::new()),
        }
    }

    fn run(mut self) {
        match scan_root(&self.key, self.single, self.registrar.as_ref()) {
            Ok(index) => self.current = Arc::new(index),
            Err(reason) => {
                let _ = self.closed.set(reason);
                self.drain_initial_close(reason);
                return;
            }
        }
        let mut dirty = false;
        loop {
            if self.dirty_signal.swap(false, Ordering::AcqRel) {
                dirty = true;
            }
            let message = if dirty {
                self.receiver.recv_timeout(self.settle_latency())
            } else {
                self.receiver
                    .recv()
                    .map_err(|_| RecvTimeoutError::Disconnected)
            };
            match message {
                Ok(RootMsg::Hint(hint)) => {
                    // Hints are deliberately not trusted; both a dirty path and
                    // an overflow rescan trigger the same verified snapshot.
                    let _ = hint;
                    dirty = true;
                }
                Ok(RootMsg::Subscribe {
                    id,
                    sender,
                    latency,
                }) => {
                    if sender
                        .send(RootUpdate::Snapshot(self.current.clone()))
                        .is_ok()
                    {
                        self.subscribers.insert(
                            id,
                            Subscriber {
                                sender,
                                latency: latency.max(Duration::from_millis(1)),
                            },
                        );
                    }
                }
                Ok(RootMsg::Unsubscribe(id)) => {
                    self.subscribers.remove(&id);
                }
                Err(RecvTimeoutError::Timeout) => {
                    dirty = false;
                    match scan_root(&self.key, self.single, self.registrar.as_ref()) {
                        Ok(index) if index != *self.current => {
                            self.current = Arc::new(index);
                            let snapshot = RootUpdate::Snapshot(self.current.clone());
                            self.subscribers.retain(|_, subscriber| {
                                subscriber.sender.send(snapshot.clone()).is_ok()
                            });
                        }
                        Ok(_) => {}
                        Err(reason) => {
                            let _ = self.closed.set(reason);
                            self.broadcast_closed(reason);
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn settle_latency(&self) -> Duration {
        self.subscribers
            .values()
            .map(|subscriber| subscriber.latency)
            .min()
            .unwrap_or(DEFAULT_LATENCY)
    }

    fn broadcast_closed(&mut self, reason: CloseReason) {
        let closed = RootUpdate::Closed(reason);
        self.subscribers
            .retain(|_, subscriber| subscriber.sender.send(closed.clone()).is_ok());
    }

    fn drain_initial_close(&mut self, reason: CloseReason) {
        while let Ok(message) = self.receiver.recv() {
            if let RootMsg::Subscribe { sender, .. } = message {
                let _ = sender.send(RootUpdate::Closed(reason));
            }
        }
    }
}

fn scan_root(
    key: &RootKey,
    single: bool,
    registrar: &dyn BackendHandle,
) -> Result<Index, CloseReason> {
    if single {
        let metadata = fs::symlink_metadata(&key.path).map_err(close_io_error)?;
        return Ok(BTreeMap::from([(
            String::new(),
            fingerprint(&key.path, &metadata),
        )]));
    }
    let root_metadata = fs::symlink_metadata(&key.path).map_err(close_io_error)?;
    if !root_metadata.is_dir() {
        return Err(CloseReason::RootGone);
    }
    let root_device = device(&root_metadata);
    let max_entries = env_u64("YAS_FS_MAX_ENTRIES", DEFAULT_MAX_ENTRIES as u64) as usize;
    let mut index = BTreeMap::new();
    index.insert(String::new(), fingerprint(&key.path, &root_metadata));
    let mut pending = VecDeque::from([(key.path.clone(), String::new())]);
    let mut ignores =
        (!key.ignores.is_empty()).then(|| ignores::Ignores::new(&key.path, &key.ignores));
    if let Some(ignores) = ignores.as_ref() {
        for directory in ignores.external_watch_dirs() {
            registrar.watch_outside(&directory);
        }
    }
    let mut watched = BTreeSet::new();
    while let Some((directory, relative)) = pending.pop_front() {
        if !registrar.add_dir(&directory) {
            return Err(CloseReason::ResourceExhausted);
        }
        watched.insert(directory.clone());
        let entries = fs::read_dir(&directory).map_err(close_io_error)?;
        let mut entries = entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(close_io_error)?;
        entries.sort_unstable_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let name = os_to_wire(&entry.file_name());
            let child_relative = join_wire(&relative, &name);
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(close_io_error(error)),
            };
            let is_directory = metadata.is_dir();
            if ignores
                .as_mut()
                .is_some_and(|ignores| ignores.matched(&child_relative, is_directory))
            {
                continue;
            }
            index.insert(child_relative.clone(), fingerprint(&path, &metadata));
            if index.len() > max_entries {
                return Err(CloseReason::ResourceExhausted);
            }
            if is_directory
                && key.recursive
                && (key.cross_filesystem || device(&metadata) == root_device)
            {
                pending.push_back((path, child_relative));
            }
        }
    }
    registrar.retain_dirs(&|path| watched.contains(path));
    Ok(index)
}

fn close_io_error(error: io::Error) -> CloseReason {
    match error.kind() {
        io::ErrorKind::NotFound => CloseReason::RootGone,
        io::ErrorKind::PermissionDenied => CloseReason::PermissionLost,
        _ if matches!(error.raw_os_error(), Some(23 | 24 | 28)) => CloseReason::ResourceExhausted,
        _ => CloseReason::BackendFailed,
    }
}

fn fingerprint(path: &Path, metadata: &fs::Metadata) -> Fingerprint {
    let file_type = metadata.file_type();
    let kind = if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    };
    let link_hash = (kind == EntryKind::Symlink)
        .then(|| fs::read_link(path).ok())
        .flatten()
        .map(|target| {
            let digest = blake3::hash(os_bytes(target.as_os_str()));
            digest.as_bytes()[..16].try_into().expect("BLAKE3 prefix")
        });
    Fingerprint {
        kind,
        size: metadata.len(),
        mode: mode(metadata),
        modified_ns: metadata.modified().map(system_time_ns).unwrap_or_default(),
        changed_ns: changed_ns(metadata),
        identity: identity(metadata),
        link_hash,
    }
}

fn system_time_ns(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    }
}

#[cfg(unix)]
fn mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode()
}

#[cfg(not(unix))]
fn mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(unix)]
fn device(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.dev()
}

#[cfg(not(unix))]
fn device(_metadata: &fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn identity(metadata: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn identity(_metadata: &fs::Metadata) -> Option<(u64, u64)> {
    None
}

#[cfg(unix)]
fn changed_ns(metadata: &fs::Metadata) -> i128 {
    use std::os::unix::fs::MetadataExt;
    i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec())
}

#[cfg(not(unix))]
fn changed_ns(_metadata: &fs::Metadata) -> i128 {
    0
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes()
}

#[cfg(not(unix))]
fn os_bytes(value: &OsStr) -> &[u8] {
    value.to_str().unwrap_or_default().as_bytes()
}

pub struct WatchHandle {
    sender: SyncSender<EngineMsg>,
    done: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    // Keep the shared root—and therefore its native watcher—alive for the
    // full subscription lifetime. Callers only need the per-watch handle
    // after construction, so relying on their temporary `Arc` would let the
    // backend disappear while the initial scan was still running.
    _shared: Arc<SharedRootHandle>,
}

impl WatchHandle {
    pub fn command(&self, command: WatchCommand) -> bool {
        if command == WatchCommand::Stop {
            self.stop.store(true, Ordering::Release);
        }
        match self.sender.try_send(EngineMsg::Command(command)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => command == WatchCommand::Stop,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.sender.try_send(EngineMsg::Command(WatchCommand::Stop));
    }
}

enum EngineMsg {
    Command(WatchCommand),
    Root(RootUpdate),
}

pub fn start_watch(
    shared: &Arc<SharedRootHandle>,
    options: SyncOptions,
    sink: WatchSink,
) -> WatchHandle {
    static SUBSCRIPTION_IDS: AtomicU64 = AtomicU64::new(1);
    let subscription_id = SUBSCRIPTION_IDS.fetch_add(1, Ordering::Relaxed).max(1);
    let (engine_sender, engine_receiver) = mpsc::sync_channel(WATCH_ENGINE_QUEUE);
    let (root_sender, root_receiver) = mpsc::sync_channel(ROOT_SUBSCRIBER_QUEUE);
    let _ = shared.sender.send(RootMsg::Subscribe {
        id: subscription_id,
        sender: root_sender,
        latency: options.latency,
    });
    let forward_sender = engine_sender.clone();
    std::thread::Builder::new()
        .name("yas-fs-watch-forward".to_owned())
        .spawn(move || {
            while let Ok(update) = root_receiver.recv() {
                if forward_sender.send(EngineMsg::Root(update)).is_err() {
                    break;
                }
            }
        })
        .expect("spawn filesystem watch forwarder");
    let shared_sender = shared.sender.clone();
    let done = Arc::new(AtomicBool::new(false));
    let done_thread = done.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    std::thread::Builder::new()
        .name("yas-fs-watch-stream".to_owned())
        .spawn(move || {
            WatchEngine::new(options, engine_receiver, sink, stop_thread).run();
            let _ = shared_sender.send(RootMsg::Unsubscribe(subscription_id));
            done_thread.store(true, Ordering::Release);
        })
        .expect("spawn filesystem watch stream");
    WatchHandle {
        sender: engine_sender,
        done,
        stop,
        _shared: Arc::clone(shared),
    }
}

struct WatchEngine {
    options: SyncOptions,
    receiver: Receiver<EngineMsg>,
    sink: WatchSink,
    shadow: Arc<Index>,
    initial: bool,
    next_update_id: u32,
    highest_sent: u32,
    unacked: VecDeque<(u32, usize)>,
    unacked_bytes: usize,
    pending: Option<Arc<Index>>,
    stop: Arc<AtomicBool>,
}

impl WatchEngine {
    fn new(
        options: SyncOptions,
        receiver: Receiver<EngineMsg>,
        sink: WatchSink,
        stop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            options,
            receiver,
            sink,
            shadow: Arc::new(Index::new()),
            initial: true,
            next_update_id: 1,
            highest_sent: 0,
            unacked: VecDeque::new(),
            unacked_bytes: 0,
            pending: None,
            stop,
        }
    }

    fn run(mut self) {
        while !self.stop.load(Ordering::Acquire) {
            let message = match self.receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            match message {
                EngineMsg::Command(WatchCommand::Ack(update_id)) => {
                    if self.acknowledge(update_id).is_err() {
                        let _ = (self.sink)(WatchEvent::Closed(CloseReason::ProtocolViolation));
                        return;
                    }
                }
                EngineMsg::Command(WatchCommand::Stop) => return,
                EngineMsg::Root(RootUpdate::Closed(reason)) => {
                    let _ = (self.sink)(WatchEvent::Closed(reason));
                    return;
                }
                EngineMsg::Root(RootUpdate::Snapshot(snapshot)) => {
                    self.pending = Some(snapshot);
                    if !self.flush_pending() {
                        return;
                    }
                }
            }
        }
    }

    fn flush_pending(&mut self) -> bool {
        while let Some(snapshot) = self.pending.take() {
            if !self.emit_snapshot(snapshot.clone()) {
                return false;
            }
            self.shadow = snapshot;
            self.initial = false;
        }
        true
    }

    fn emit_snapshot(&mut self, snapshot: Arc<Index>) -> bool {
        let records = diff(&self.shadow, &snapshot, self.initial);
        if records.is_empty() && !self.initial {
            return true;
        }
        let mut batches = Vec::<Vec<WatchRecord>>::new();
        let mut batch = Vec::new();
        let mut batch_bytes = 0usize;
        for record in records {
            let cost = record.estimated_bytes();
            if !batch.is_empty()
                && batch_bytes.saturating_add(cost) > self.options.batch_target.max(1)
            {
                batches.push(std::mem::take(&mut batch));
                batch_bytes = 0;
            }
            batch_bytes = batch_bytes.saturating_add(cost);
            batch.push(record);
        }
        if !batch.is_empty() || batches.is_empty() {
            batches.push(batch);
        }
        let batch_count = batches.len();
        for (index, records) in batches.into_iter().enumerate() {
            if !self.wait_for_credit() {
                return false;
            }
            let update = WatchUpdate {
                update_id: self.allocate_update_id(),
                reset: self.initial && index == 0,
                snapshot_end: self.initial && index + 1 == batch_count,
                records,
            };
            let bytes = update.estimated_bytes();
            self.highest_sent = update.update_id;
            self.unacked.push_back((update.update_id, bytes));
            self.unacked_bytes = self.unacked_bytes.saturating_add(bytes);
            if !(self.sink)(WatchEvent::Update(update)) {
                return false;
            }
        }
        true
    }

    fn allocate_update_id(&mut self) -> u32 {
        let id = self.next_update_id.max(1);
        self.next_update_id = id.wrapping_add(1).max(1);
        id
    }

    fn wait_for_credit(&mut self) -> bool {
        let window = self.options.window_bytes.max(1);
        while self.unacked_bytes >= window {
            if self.stop.load(Ordering::Acquire) {
                return false;
            }
            match self.receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(EngineMsg::Command(WatchCommand::Ack(update_id))) => {
                    if self.acknowledge(update_id).is_err() {
                        let _ = (self.sink)(WatchEvent::Closed(CloseReason::ProtocolViolation));
                        return false;
                    }
                }
                Ok(EngineMsg::Command(WatchCommand::Stop))
                | Err(RecvTimeoutError::Disconnected) => return false,
                Err(RecvTimeoutError::Timeout) => continue,
                Ok(EngineMsg::Root(RootUpdate::Closed(reason))) => {
                    let _ = (self.sink)(WatchEvent::Closed(reason));
                    return false;
                }
                Ok(EngineMsg::Root(RootUpdate::Snapshot(snapshot))) => {
                    self.pending = Some(snapshot);
                }
            }
        }
        true
    }

    fn acknowledge(&mut self, update_id: u32) -> Result<(), ()> {
        if update_id == 0 || update_id > self.highest_sent {
            return Err(());
        }
        let Some(position) = self.unacked.iter().position(|(id, _)| *id == update_id) else {
            return Err(());
        };
        for _ in 0..=position {
            if let Some((_, bytes)) = self.unacked.pop_front() {
                self.unacked_bytes = self.unacked_bytes.saturating_sub(bytes);
            }
        }
        Ok(())
    }
}

fn diff(previous: &Index, current: &Index, initial: bool) -> Vec<WatchRecord> {
    if initial {
        return current
            .keys()
            .cloned()
            .map(|path| WatchRecord::Upsert { path })
            .collect();
    }
    let mut deleted = previous
        .iter()
        .filter(|(path, _)| !current.contains_key(*path))
        .map(|(path, fingerprint)| (path.clone(), fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut added = current
        .iter()
        .filter(|(path, _)| !previous.contains_key(*path))
        .map(|(path, fingerprint)| (path.clone(), fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut records = Vec::new();
    let mut old_by_identity = HashMap::<(u64, u64), String>::new();
    for (path, fingerprint) in &deleted {
        if let Some(identity) = fingerprint.identity {
            old_by_identity
                .entry(identity)
                .or_insert_with(|| path.clone());
        }
    }
    let moves = added
        .iter()
        .filter_map(|(to, fingerprint)| {
            fingerprint
                .identity
                .and_then(|identity| old_by_identity.get(&identity))
                .map(|from| (from.clone(), to.clone()))
        })
        .collect::<Vec<_>>();
    for (from, to) in moves {
        deleted.remove(&from);
        added.remove(&to);
        records.push(WatchRecord::Move { from, to });
    }
    records.extend(
        deleted
            .into_keys()
            .rev()
            .map(|path| WatchRecord::Delete { path }),
    );
    records.extend(
        current
            .iter()
            .filter(|(path, fingerprint)| {
                added.contains_key(*path)
                    || previous
                        .get(*path)
                        .is_some_and(|before| before != *fingerprint)
            })
            .map(|(path, _)| WatchRecord::Upsert { path: path.clone() }),
    );
    records
}

pub fn validate_root(path: &str) -> Result<PathBuf, OpenError> {
    if path.is_empty() || path.contains('\0') {
        return Err(OpenError {
            kind: OpenErrorKind::Invalid,
            detail: "invalid filesystem root".to_owned(),
        });
    }
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(literal_error)
            if literal_error.kind() == io::ErrorKind::NotFound && path.contains('%') =>
        {
            let decoded = wire_to_os(path).ok_or_else(|| open_io_error(literal_error))?;
            fs::canonicalize(decoded).map_err(open_io_error)
        }
        Err(error) => Err(open_io_error(error)),
    }
}

pub fn validate_single_root(path: &str) -> Result<PathBuf, OpenError> {
    let canonical = validate_root(path)?;
    if fs::symlink_metadata(&canonical)
        .map_err(open_io_error)?
        .is_dir()
    {
        return Err(OpenError {
            kind: OpenErrorKind::Invalid,
            detail: "single-file watch root is a directory".to_owned(),
        });
    }
    Ok(canonical)
}

pub fn escape_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                push_escaping_percent(&mut output, text);
                return output;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                push_escaping_percent(
                    &mut output,
                    // SAFETY: `valid_up_to` identifies a valid UTF-8 prefix.
                    unsafe { std::str::from_utf8_unchecked(valid) },
                );
                let invalid = error.error_len().unwrap_or(after.len());
                for byte in &after[..invalid] {
                    use std::fmt::Write as _;
                    let _ = write!(output, "%{byte:02X}");
                }
                rest = &after[invalid..];
            }
        }
    }
}

fn push_escaping_percent(output: &mut String, text: &str) {
    for character in text.chars() {
        if character == '%' {
            output.push_str("%25");
        } else {
            output.push(character);
        }
    }
}

pub fn unescape_to_bytes(value: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let encoded = bytes.get(index + 1..index + 3)?;
            let high = (encoded[0] as char).to_digit(16)?;
            let low = (encoded[1] as char).to_digit(16)?;
            output.push((high * 16 + low) as u8);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    Some(output)
}

pub fn escape_wide(units: &[u16]) -> String {
    let mut output = String::with_capacity(units.len());
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok('%') => output.push_str("%25"),
            Ok(character) => output.push(character),
            Err(error) => {
                use std::fmt::Write as _;
                let _ = write!(output, "%u{:04X}", error.unpaired_surrogate());
            }
        }
    }
    output
}

pub fn unescape_to_wide(value: &str) -> Option<Vec<u16>> {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if bytes.get(index + 1) == Some(&b'u') {
                output.push(u16::from_str_radix(value.get(index + 2..index + 6)?, 16).ok()?);
                index += 6;
            } else {
                output.push(u16::from(
                    u8::from_str_radix(value.get(index + 1..index + 3)?, 16).ok()?,
                ));
                index += 3;
            }
        } else {
            let character = value[index..].chars().next()?;
            let mut encoded = [0u16; 2];
            output.extend_from_slice(character.encode_utf16(&mut encoded));
            index += character.len_utf8();
        }
    }
    Some(output)
}

#[cfg(unix)]
pub fn escape_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    escape_bytes(path.as_os_str().as_bytes())
}

#[cfg(windows)]
pub fn escape_path(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    escape_wide(&path.as_os_str().encode_wide().collect::<Vec<_>>())
}

#[cfg(all(not(unix), not(windows)))]
pub fn escape_path(path: &Path) -> String {
    escape_bytes(path.to_string_lossy().as_bytes())
}

#[cfg(unix)]
fn os_to_wire(value: &OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    escape_bytes(value.as_bytes())
}

#[cfg(windows)]
fn os_to_wire(value: &OsStr) -> String {
    use std::os::windows::ffi::OsStrExt;
    escape_wide(&value.encode_wide().collect::<Vec<_>>())
}

#[cfg(all(not(unix), not(windows)))]
fn os_to_wire(value: &OsStr) -> String {
    escape_bytes(value.to_string_lossy().as_bytes())
}

#[cfg(unix)]
fn wire_to_os(value: &str) -> Option<OsString> {
    use std::os::unix::ffi::OsStringExt;
    Some(OsString::from_vec(unescape_to_bytes(value)?))
}

#[cfg(windows)]
fn wire_to_os(value: &str) -> Option<OsString> {
    use std::os::windows::ffi::OsStringExt;
    Some(OsString::from_wide(&unescape_to_wide(value)?))
}

#[cfg(all(not(unix), not(windows)))]
fn wire_to_os(value: &str) -> Option<OsString> {
    Some(String::from_utf8(unescape_to_bytes(value)?).ok()?.into())
}

pub fn resolve_wire_path(root: &Path, wire: &str) -> Option<PathBuf> {
    let mut absolute = root.to_path_buf();
    if wire.is_empty() {
        return Some(absolute);
    }
    for component in wire.split('/') {
        let decoded = wire_to_os(component)?;
        let mut components = Path::new(&decoded).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(part)), None) if part == decoded.as_os_str() => {
                absolute.push(part);
            }
            _ => return None,
        }
    }
    Some(absolute)
}

fn join_wire(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent}/{child}")
    }
}

fn parent_wire(path: &str) -> Option<&str> {
    if path.is_empty() {
        None
    } else {
        Some(path.rsplit_once('/').map_or("", |(parent, _)| parent))
    }
}

pub fn blake3_128(data: &[u8]) -> u128 {
    u128::from_le_bytes(
        blake3::hash(data).as_bytes()[..16]
            .try_into()
            .expect("BLAKE3 prefix"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_hint_bursts_are_coalesced_into_a_bounded_wake() {
        let (tx, rx) = mpsc::sync_channel(1);
        let dirty_signal = Arc::new(AtomicBool::new(false));
        let hints = HintSender {
            tx,
            dirty_signal: dirty_signal.clone(),
        };

        for index in 0..10_000 {
            assert!(hints.send(Hint::Dirty(PathBuf::from(index.to_string()))));
        }

        assert!(dirty_signal.load(Ordering::Acquire));
        assert!(matches!(rx.try_recv(), Ok(RootMsg::Hint(_))));
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn a_saturated_watch_command_lane_still_records_stop() {
        let root = temporary_directory("saturated-command");
        let (sender, _receiver) = mpsc::sync_channel(1);
        sender
            .try_send(EngineMsg::Command(WatchCommand::Ack(1)))
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = WatchHandle {
            sender,
            done: Arc::new(AtomicBool::new(false)),
            stop: stop.clone(),
            _shared: open_root_unwatched(RootKey {
                path: root.clone(),
                recursive: true,
                cross_filesystem: false,
                ignores: IgnoreSpec::default(),
            }),
        };

        assert!(handle.command(WatchCommand::Stop));
        assert!(stop.load(Ordering::Acquire));
        drop(handle);
        fs::remove_dir_all(root).unwrap();
    }

    fn temporary_directory(label: &str) -> PathBuf {
        static IDS: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "yas-fssync-{label}-{}-{}",
            std::process::id(),
            IDS.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn escaping_round_trips_and_rejects_traversal() {
        let bytes = b"hello%\xff";
        let encoded = escape_bytes(bytes);
        assert_eq!(
            unescape_to_bytes(&encoded).as_deref(),
            Some(bytes.as_slice())
        );
        assert!(resolve_wire_path(Path::new("/tmp"), "%2E%2E/escape").is_none());
        assert!(resolve_wire_path(Path::new("/tmp"), "%2Fetc").is_none());
    }

    #[test]
    fn typed_watch_reports_initial_and_incremental_changes() {
        let root = temporary_directory("typed-watch");
        fs::write(root.join("before"), b"one").unwrap();
        let shared = open_root_unwatched(RootKey {
            path: root.clone(),
            recursive: true,
            cross_filesystem: false,
            ignores: IgnoreSpec::default(),
        });
        let hint = shared.hint_sender();
        let (sender, receiver) = mpsc::channel();
        let handle = start_watch(
            &shared,
            SyncOptions::default(),
            Box::new(move |event| sender.send(event).is_ok()),
        );
        let shared_weak = Arc::downgrade(&shared);
        drop(shared);
        assert!(shared_weak.upgrade().is_some());
        let first = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let WatchEvent::Update(first) = first else {
            panic!("initial watch event was not an update");
        };
        assert!(first.reset && first.snapshot_end);
        assert!(
            first
                .records
                .iter()
                .any(|record| matches!(record, WatchRecord::Upsert { path } if path == "before"))
        );
        assert!(handle.command(WatchCommand::Ack(first.update_id)));

        fs::rename(root.join("before"), root.join("after")).unwrap();
        assert!(hint.send(Hint::Rescan));
        let second = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let WatchEvent::Update(second) = second else {
            panic!("incremental watch event was not an update");
        };
        assert!(second.records.iter().any(|record| matches!(
            record,
            WatchRecord::Move { from, to } if from == "before" && to == "after"
        )));
        drop(handle);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn credit_window_blocks_until_ack() {
        let root = temporary_directory("credit");
        for index in 0..8 {
            fs::write(root.join(format!("entry-{index}")), b"x").unwrap();
        }
        let shared = open_root_unwatched(RootKey {
            path: root.clone(),
            recursive: true,
            cross_filesystem: false,
            ignores: IgnoreSpec::default(),
        });
        let (sender, receiver) = mpsc::channel();
        let handle = start_watch(
            &shared,
            SyncOptions {
                window_bytes: 1,
                batch_target: 1,
                ..SyncOptions::default()
            },
            Box::new(move |event| sender.send(event).is_ok()),
        );
        let WatchEvent::Update(first) = receiver.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("initial watch event was not an update");
        };
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(handle.command(WatchCommand::Ack(first.update_id)));
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            WatchEvent::Update(_)
        ));
        drop(handle);
        fs::remove_dir_all(root).unwrap();
    }
}
