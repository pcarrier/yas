//! Native YAS filesystem semantics.
//!
//! This module owns session roots, watches, staging files and durable
//! mutations.  It intentionally does not allocate YAS request or Transfer
//! identifiers: `yas.rs` supplies correlation and flow control around these
//! owned semantic values.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Component, Path as OsPath, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use regex::bytes::{Regex, RegexBuilder};
use tokio::sync::mpsc;
use yas_fssync as sync;
use yas_wire::Encode;
use yas_wire::codec::Extensions;
use yas_wire::core::RuntimeState;
use yas_wire::fs as wire;
use yas_wire::schema;

use super::{AppState, resolve_term_cwd};

const WATCH_QUEUE: usize = 4;
const DEFAULT_QUERY_RESULTS: usize = 256;
const MAX_REPLAYS: usize = 1_024;
const MAX_FETCH_BYTES: u64 = schema::fs::MAX_STAGED_BYTES;

#[derive(Clone)]
pub(crate) struct Runtime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    app: AppState,
    enabled: bool,
    writes_enabled: bool,
    limits: wire::Limits,
    next_root: AtomicU64,
    roots: StdMutex<HashMap<PathBuf, Weak<Root>>>,
}

#[derive(Clone)]
pub(crate) struct Session {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    runtime: Runtime,
    opened: StdMutex<HashMap<u64, OpenedRoot>>,
    watches: StdMutex<HashMap<u32, WatchControl>>,
    stages: StdMutex<HashMap<u64, Stage>>,
    next_watch: AtomicU64,
    next_stage: AtomicU64,
    staging_root: StdMutex<Option<PathBuf>>,
    replay: StdMutex<ReplayCache>,
    closed: AtomicBool,
}

struct Root {
    handle: u64,
    path: PathBuf,
    canonical_path: Vec<u8>,
    single_file: bool,
    revision: AtomicU64,
    entries: StdMutex<BTreeMap<wire::Path, EntryVersion>>,
    operation_echoes: StdMutex<BTreeMap<wire::Path, [u8; 16]>>,
}

#[derive(Clone)]
struct OpenedRoot {
    root: Arc<Root>,
    read_only: bool,
    cross_filesystem: bool,
}

#[derive(Clone, Debug)]
struct EntryVersion {
    signature: [u8; 32],
    revision: u64,
}

struct WatchControl {
    root_handle: u64,
    command: Arc<StdMutex<Option<sync::WatchHandle>>>,
}

pub(crate) struct Watch {
    id: u32,
    root_handle: u64,
    command: Arc<StdMutex<Option<sync::WatchHandle>>>,
    receiver: mpsc::Receiver<Result<WatchUpdate, Error>>,
    owner: Weak<SessionInner>,
}

#[derive(Clone, Debug)]
pub(crate) struct WatchUpdate {
    /// Native watcher update token. The YAS State revision is assigned by the
    /// session adapter; ACK this token only after that State revision is ACKed.
    pub(crate) update_id: u32,
    pub(crate) reset: bool,
    pub(crate) snapshot_end: bool,
    pub(crate) mutations: Vec<wire::StateMutation>,
}

struct Stage {
    root: Arc<Root>,
    path: wire::Path,
    precondition: wire::Precondition,
    flags: u16,
    mode: u32,
    byte_len: u64,
    content_hash: [u8; 32],
    temp_path: PathBuf,
    file: Option<File>,
    hasher: blake3::Hasher,
    received: u64,
    sealed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ReplayValue {
    Commit(wire::CommitResult),
    Apply(wire::ApplyResult),
}

#[derive(Default)]
struct ReplayCache {
    values: HashMap<[u8; 16], ([u8; 32], ReplayValue)>,
    order: VecDeque<[u8; 16]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileContent {
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_hash: [u8; 32],
    pub(crate) modified_unix_ns: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StageInfo {
    pub(crate) staging_handle: u64,
    pub(crate) byte_len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryData {
    pub(crate) records: Vec<wire::QueryRecord>,
    pub(crate) next_cursor: Vec<u8>,
    pub(crate) total_hint: u64,
    pub(crate) truncated: bool,
}

impl QueryData {
    pub(crate) fn typed_records(&self) -> Result<Vec<wire::TypedRecord>, Error> {
        self.records
            .iter()
            .map(|record| record.to_typed_record().map_err(|_| Error::Internal))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Unavailable,
    NotFound,
    Permission,
    Conflict(wire::ConflictDetail),
    ResourceExhausted,
    TooLarge,
    Unsupported,
    Invalid(&'static str),
    Io(String),
    Closed,
    Internal,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("FS family is unavailable"),
            Self::NotFound => formatter.write_str("filesystem entry not found"),
            Self::Permission => formatter.write_str("filesystem operation is not permitted"),
            Self::Conflict(_) => formatter.write_str("filesystem precondition failed"),
            Self::ResourceExhausted => formatter.write_str("filesystem resource limit reached"),
            Self::TooLarge => formatter.write_str("filesystem value is too large"),
            Self::Unsupported => formatter.write_str("filesystem operation is unsupported"),
            Self::Invalid(detail) => formatter.write_str(detail),
            Self::Io(detail) => formatter.write_str(detail),
            Self::Closed => formatter.write_str("filesystem session is closed"),
            Self::Internal => formatter.write_str("filesystem internal error"),
        }
    }
}

impl Runtime {
    pub(crate) fn new(app: AppState) -> Self {
        let enabled = !std::env::var("YAS_FS").is_ok_and(|value| value == "0");
        let writes_enabled = !std::env::var("YAS_FS_WRITE").is_ok_and(|value| value == "0");
        let mut limits = wire::Limits::HARD;
        limits.max_catalog_entries = u32::try_from(sync::SyncOptions::default().max_entries)
            .unwrap_or(u32::MAX)
            .clamp(1, wire::Limits::HARD.max_catalog_entries);
        Self {
            inner: Arc::new(RuntimeInner {
                app,
                enabled,
                writes_enabled,
                limits,
                next_root: AtomicU64::new(1),
                roots: StdMutex::new(HashMap::new()),
            }),
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.inner.enabled
    }

    pub(crate) fn runtime_state(&self) -> RuntimeState {
        if self.enabled() {
            RuntimeState::Available
        } else {
            RuntimeState::Unavailable
        }
    }

    pub(crate) fn limits(&self) -> wire::Limits {
        self.inner.limits
    }

    pub(crate) fn session(&self, owner_session: [u8; 16]) -> Result<Session, Error> {
        if !self.enabled() {
            return Err(Error::Unavailable);
        }
        if owner_session.iter().all(|byte| *byte == 0) {
            return Err(Error::Invalid("zero FS owner session"));
        }
        Ok(Session {
            inner: Arc::new(SessionInner {
                runtime: self.clone(),
                opened: StdMutex::new(HashMap::new()),
                watches: StdMutex::new(HashMap::new()),
                stages: StdMutex::new(HashMap::new()),
                next_watch: AtomicU64::new(1),
                next_stage: AtomicU64::new(1),
                staging_root: StdMutex::new(None),
                replay: StdMutex::new(ReplayCache::default()),
                closed: AtomicBool::new(false),
            }),
        })
    }

    fn root(&self, path: PathBuf) -> Result<Arc<Root>, Error> {
        let mut roots = self.inner.roots.lock().unwrap();
        roots.retain(|_, root| root.strong_count() != 0);
        if let Some(root) = roots.get(&path).and_then(Weak::upgrade) {
            return Ok(root);
        }
        let handle = next_nonzero(&self.inner.next_root).ok_or(Error::ResourceExhausted)?;
        let metadata = fs::symlink_metadata(&path).map_err(map_io)?;
        let root = Arc::new(Root {
            handle,
            canonical_path: os_path_bytes(&path),
            single_file: !metadata.is_dir(),
            path: path.clone(),
            revision: AtomicU64::new(1),
            entries: StdMutex::new(BTreeMap::new()),
            operation_echoes: StdMutex::new(BTreeMap::new()),
        });
        roots.insert(path, Arc::downgrade(&root));
        Ok(root)
    }
}

impl Session {
    pub(crate) async fn open(&self, request: &wire::Open) -> Result<wire::OpenResult, Error> {
        self.ensure_open()?;
        if self.inner.opened.lock().unwrap().len()
            >= wire::Limits::HARD.max_roots_per_session as usize
        {
            return Err(Error::ResourceExhausted);
        }
        let path = canonical_root(self.resolve_source(&request.source).await?)?;
        let root = self.inner.runtime.root(path)?;
        let read_only = request.flags & schema::fs::OPEN_READ_ONLY as u16 != 0
            || !self.inner.runtime.inner.writes_enabled;
        let opened = OpenedRoot {
            root: root.clone(),
            read_only,
            cross_filesystem: request.flags & schema::fs::OPEN_CROSS_FILESYSTEM as u16 != 0,
        };
        self.inner
            .opened
            .lock()
            .unwrap()
            .insert(root.handle, opened);
        Ok(wire::OpenResult {
            root_handle: root.handle,
            root_revision: root.revision.load(Ordering::Acquire).max(1),
            path_model: path_model(),
            case_behavior: case_behavior(),
            canonical_path: root.canonical_path.clone(),
            extensions: Extensions::default(),
        })
    }

    /// Close one opened root and return every staged upload invalidated by it.
    ///
    /// The native wire runtime owns the published Transfer descriptors for
    /// these stages. Returning the handles lets it retire those descriptors
    /// only after the correlated CLOSE Result is physically written.
    pub(crate) fn close(&self, root_handle: u64) -> Result<Vec<u64>, Error> {
        self.ensure_open()?;
        let removed = self.inner.opened.lock().unwrap().remove(&root_handle);
        if removed.is_none() {
            // CLOSE is explicitly idempotent. An already-closed or never-open
            // handle owns no resources in this session.
            return Ok(Vec::new());
        }
        let watch_ids = self
            .inner
            .watches
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(&id, watch)| (watch.root_handle == root_handle).then_some(id))
            .collect::<Vec<_>>();
        for id in watch_ids {
            self.unwatch(id);
        }
        let mut invalidated_stages = Vec::new();
        self.inner
            .stages
            .lock()
            .unwrap()
            .retain(|staging_handle, stage| {
                let keep = stage.root.handle != root_handle;
                if !keep {
                    invalidated_stages.push(*staging_handle);
                }
                keep
            });
        invalidated_stages.sort_unstable();
        Ok(invalidated_stages)
    }

    /// Resolve an existing YAS FS component path for another typed server
    /// family. The result has passed the same final-symlink confinement check
    /// as FS reads; callers must not append unvalidated components to it.
    pub(crate) fn resolved_path(
        &self,
        root_handle: u64,
        path: &wire::Path,
    ) -> Result<PathBuf, Error> {
        let opened = self.opened(root_handle)?;
        confined_existing(&opened.root, path, true)
    }

    pub(crate) async fn watch(&self, request: &wire::Watch) -> Result<Watch, Error> {
        let opened = self.opened(request.root_handle)?;
        let root = opened.root.clone();
        let flags = request.flags;
        let recursive = flags & schema::fs::WATCH_RECURSIVE as u16 != 0;
        let has_enumeration_policy = flags
            & ((schema::fs::WATCH_GITIGNORE
                | schema::fs::WATCH_DOT_IGNORE
                | schema::fs::WATCH_EXCLUDE_GIT) as u16)
            != 0
            || !request.ignore_patterns.is_empty();
        if root.single_file && (recursive || has_enumeration_policy) {
            return Err(Error::Invalid(
                "recursive or ignored WATCH on a single-file FS root",
            ));
        }
        let current_watches = self
            .inner
            .watches
            .lock()
            .unwrap()
            .values()
            .filter(|watch| watch.root_handle == request.root_handle)
            .count();
        if current_watches >= wire::Limits::HARD.max_watches_per_root as usize {
            return Err(Error::ResourceExhausted);
        }
        let mut patterns = sync::IgnoreSpec::parse_patterns(&request.ignore_patterns);
        if flags & schema::fs::WATCH_INCLUDE_HIDDEN as u16 == 0 {
            patterns.push(".*".to_owned());
            patterns.push("**/.*".to_owned());
        }
        if patterns.len() > sync::MAX_IGNORE_PATTERNS {
            return Err(Error::TooLarge);
        }
        let ignores = sync::IgnoreSpec {
            gitignore: flags & schema::fs::WATCH_GITIGNORE as u16 != 0,
            dot_ignore: flags & schema::fs::WATCH_DOT_IGNORE as u16 != 0,
            exclude_git: flags & schema::fs::WATCH_EXCLUDE_GIT as u16 != 0,
            patterns,
        };
        let shared_root = root.clone();
        let cross_filesystem = opened.cross_filesystem;
        let shared = tokio::task::spawn_blocking(move || {
            if shared_root.single_file {
                sync::open_single_root(shared_root.path.clone())
            } else {
                sync::open_root(sync::RootKey {
                    path: shared_root.path.clone(),
                    recursive,
                    cross_filesystem,
                    ignores,
                })
            }
        })
        .await
        .map_err(|_| Error::Internal)?
        .map_err(sync_open_error)?;
        let id = self.next_watch_id()?;
        let (sender, receiver) = mpsc::channel(WATCH_QUEUE);
        let inline_max = request.inline_max.min(wire::Limits::HARD.max_inline_bytes) as usize;
        let include_content = flags & schema::fs::WATCH_CONTENT as u16 != 0;
        let callback_root = root.clone();
        let sink: sync::WatchSink = Box::new(move |event| {
            let update = translate_watch_event(&callback_root, event, inline_max, include_content);
            sender.blocking_send(update).is_ok()
        });
        let mut options = sync::SyncOptions {
            recursive,
            content: include_content,
            cross_filesystem,
            inline_max: inline_max as u64,
            window_bytes: wire::MAX_QUERY_BYTES,
            max_entries: self.inner.runtime.limits().max_catalog_entries as usize,
            ..sync::SyncOptions::default()
        };
        if request.settle_ms != 0 {
            options.latency = Duration::from_millis(u64::from(request.settle_ms));
        }
        let handle = sync::start_watch(&shared, options, sink);
        let command = Arc::new(StdMutex::new(Some(handle)));
        self.inner.watches.lock().unwrap().insert(
            id,
            WatchControl {
                root_handle: request.root_handle,
                command: command.clone(),
            },
        );
        Ok(Watch {
            id,
            root_handle: request.root_handle,
            command,
            receiver,
            owner: Arc::downgrade(&self.inner),
        })
    }

    pub(crate) fn unwatch(&self, subscription_id: u32) {
        let Some(control) = self.inner.watches.lock().unwrap().remove(&subscription_id) else {
            return;
        };
        if let Some(handle) = control.command.lock().unwrap().take() {
            let _ = handle.command(sync::WatchCommand::Stop);
        }
    }

    pub(crate) async fn fetch(&self, request: &wire::Fetch) -> Result<FileContent, Error> {
        let opened = self.opened(request.root_handle)?;
        let root = opened.root;
        let path = request.path.clone();
        let expected_hash = request.expected_hash;
        tokio::task::spawn_blocking(move || {
            // Follow the final link. A caller asking for a path's content
            // means the file, not the sixty bytes of text that say where the
            // file is — and the mirror already reports a symlink as a symlink,
            // with its target, so nothing needs this to answer that question.
            // A target outside the root is still refused, as everywhere else.
            let absolute = confined_existing(&root, &path, true)?;
            let (bytes, modified_unix_ns) = read_content_bounded(&absolute, MAX_FETCH_BYTES, true)?;
            let content_hash = *blake3::hash(&bytes).as_bytes();
            if expected_hash.is_some_and(|expected| expected != content_hash) {
                return Err(conflict_for(&root, &path));
            }
            Ok(FileContent {
                bytes,
                content_hash,
                modified_unix_ns,
            })
        })
        .await
        .map_err(|_| Error::Internal)?
    }

    pub(crate) async fn read(&self, request: &wire::Read) -> Result<QueryData, Error> {
        let opened = self.opened(request.root_handle)?;
        let root = opened.root;
        let questions = request.questions.clone();
        tokio::task::spawn_blocking(move || read_questions(&root, &questions))
            .await
            .map_err(|_| Error::Internal)?
    }

    pub(crate) async fn search(&self, request: &wire::Search) -> Result<QueryData, Error> {
        let opened = self.opened(request.root_handle)?;
        let root = opened.root;
        let request = request.clone();
        tokio::task::spawn_blocking(move || search_root(&root, &request))
            .await
            .map_err(|_| Error::Internal)?
    }

    pub(crate) async fn index(&self, request: &wire::Index) -> Result<QueryData, Error> {
        let opened = self.opened(request.root_handle)?;
        let root = opened.root;
        let request = request.clone();
        tokio::task::spawn_blocking(move || index_root(&root, &request))
            .await
            .map_err(|_| Error::Internal)?
    }

    pub(crate) async fn grep(&self, request: &wire::Grep) -> Result<QueryData, Error> {
        let opened = self.opened(request.root_handle)?;
        let root = opened.root;
        let request = request.clone();
        tokio::task::spawn_blocking(move || grep_root(&root, &request))
            .await
            .map_err(|_| Error::Internal)?
    }

    pub(crate) fn begin_stage(&self, request: &wire::StageWrite) -> Result<StageInfo, Error> {
        let opened = self.opened(request.root_handle)?;
        if opened.read_only {
            return Err(Error::Permission);
        }
        let mut stages = self.inner.stages.lock().unwrap();
        if stages.len() >= wire::Limits::HARD.max_stages_per_session as usize
            || stages
                .values()
                .map(|stage| stage.byte_len)
                .sum::<u64>()
                .checked_add(request.byte_len)
                .is_none_or(|total| total > wire::Limits::HARD.max_staged_bytes)
        {
            return Err(Error::ResourceExhausted);
        }
        check_precondition(&opened.root, &request.path, &request.precondition)?;
        let staging_handle =
            next_nonzero(&self.inner.next_stage).ok_or(Error::ResourceExhausted)?;
        let directory = self.staging_root()?.join("uploads");
        fs::create_dir_all(&directory).map_err(map_io)?;
        set_private_directory(&directory)?;
        let temp_path = unique_temp_path(&directory, "upload")?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(map_io)?;
        stages.insert(
            staging_handle,
            Stage {
                root: opened.root,
                path: request.path.clone(),
                precondition: request.precondition.clone(),
                flags: request.flags,
                mode: request.mode,
                byte_len: request.byte_len,
                content_hash: request.content_hash,
                temp_path,
                file: Some(file),
                hasher: blake3::Hasher::new(),
                received: 0,
                sealed: false,
            },
        );
        Ok(StageInfo {
            staging_handle,
            byte_len: request.byte_len,
        })
    }

    pub(crate) fn append_stage(
        &self,
        staging_handle: u64,
        offset: u64,
        data: &[u8],
    ) -> Result<u64, Error> {
        self.ensure_open()?;
        let mut stages = self.inner.stages.lock().unwrap();
        let stage = stages.get_mut(&staging_handle).ok_or(Error::NotFound)?;
        if stage.sealed || offset != stage.received {
            return Err(Error::Invalid("out-of-order FS staged bytes"));
        }
        let next = offset
            .checked_add(data.len() as u64)
            .ok_or(Error::TooLarge)?;
        if next > stage.byte_len {
            return Err(Error::TooLarge);
        }
        stage
            .file
            .as_mut()
            .ok_or(Error::Closed)?
            .write_all(data)
            .map_err(map_io)?;
        stage.hasher.update(data);
        stage.received = next;
        Ok(next)
    }

    pub(crate) fn seal_stage(&self, staging_handle: u64) -> Result<(), Error> {
        self.ensure_open()?;
        let mut stages = self.inner.stages.lock().unwrap();
        let stage = stages.get_mut(&staging_handle).ok_or(Error::NotFound)?;
        if stage.sealed {
            return Ok(());
        }
        if stage.received != stage.byte_len {
            return Err(Error::Invalid("incomplete FS staged write"));
        }
        let actual = *stage.hasher.finalize().as_bytes();
        if actual != stage.content_hash {
            return Err(Error::Conflict(conflict_detail(&stage.root, &stage.path)));
        }
        if let Some(mut file) = stage.file.take() {
            file.flush().map_err(map_io)?;
        }
        stage.sealed = true;
        Ok(())
    }

    pub(crate) fn abort_stage(&self, staging_handle: u64) {
        self.inner.stages.lock().unwrap().remove(&staging_handle);
    }

    pub(crate) async fn commit(&self, request: &wire::Commit) -> Result<wire::CommitResult, Error> {
        self.ensure_open()?;
        let fingerprint = request_fingerprint(request)?;
        if let Some(value) = self.replay(&request.operation_id, fingerprint)? {
            return match value {
                ReplayValue::Commit(value) => Ok(value),
                ReplayValue::Apply(_) => Err(Error::Conflict(wire::ConflictDetail {
                    path: wire::Path::default(),
                    current_present: false,
                    current_entry_revision: 0,
                    modified_unix_ns: 0,
                    current_hash: None,
                })),
            };
        }
        let stage = {
            let mut stages = self.inner.stages.lock().unwrap();
            let stage = stages
                .remove(&request.staging_handle)
                .ok_or(Error::NotFound)?;
            if !stage.sealed {
                stages.insert(request.staging_handle, stage);
                return Err(Error::Invalid("unsealed FS staged write"));
            }
            stage
        };
        let operation_id = request.operation_id;
        let flags = request.flags;
        let result = tokio::task::spawn_blocking(move || commit_stage(stage, operation_id, flags))
            .await
            .map_err(|_| Error::Internal)??;
        self.remember_replay(
            request.operation_id,
            fingerprint,
            ReplayValue::Commit(result.clone()),
        );
        Ok(result)
    }

    pub(crate) async fn apply(&self, request: &wire::Apply) -> Result<wire::ApplyResult, Error> {
        self.ensure_open()?;
        let opened = self.opened(request.root_handle)?;
        if opened.read_only {
            return Err(Error::Permission);
        }
        if request.flags & schema::fs::APPLY_ALL_OR_NONE as u16 != 0 {
            return Err(Error::Unsupported);
        }
        let fingerprint = request_fingerprint(request)?;
        if let Some(value) = self.replay(&request.operation_id, fingerprint)? {
            return match value {
                ReplayValue::Apply(value) => Ok(value),
                ReplayValue::Commit(_) => Err(Error::Invalid("FS operation ID reused")),
            };
        }
        let root = opened.root;
        let items = request.items.clone();
        let operation_id = request.operation_id;
        let result = tokio::task::spawn_blocking(move || apply_items(&root, operation_id, &items))
            .await
            .map_err(|_| Error::Internal)??;
        self.remember_replay(
            request.operation_id,
            fingerprint,
            ReplayValue::Apply(result.clone()),
        );
        Ok(result)
    }

    async fn resolve_source(&self, source: &wire::RootSource) -> Result<PathBuf, Error> {
        match source {
            wire::RootSource::PlatformPath(path) => platform_path(path),
            wire::RootSource::TerminalCwd {
                terminal_handle,
                suffix,
            } => {
                let session = self.inner.runtime.inner.app.session.lock().await;
                let pty_id = session
                    .terminal_backend(*terminal_handle)
                    .ok_or(Error::NotFound)?;
                let pty = session.ptys.get(&pty_id).ok_or(Error::NotFound)?;
                let cwd =
                    resolve_term_cwd(pty.osc7_cwd.as_deref(), || super::pty::pty_cwd(&pty.handle))
                        .ok_or(Error::NotFound)?;
                let mut path = PathBuf::from(cwd);
                append_wire_path(&mut path, suffix)?;
                Ok(path)
            }
            wire::RootSource::ProcessCwd(process_handle) => self
                .inner
                .runtime
                .inner
                .app
                .process_server
                .native_cwd(*process_handle)
                .ok_or(Error::NotFound)
                .and_then(|bytes| platform_path(&bytes)),
            wire::RootSource::Staging => self.staging_root(),
        }
    }

    fn staging_root(&self) -> Result<PathBuf, Error> {
        let mut slot = self.inner.staging_root.lock().unwrap();
        if let Some(path) = slot.as_ref() {
            return Ok(path.clone());
        }
        let base = std::env::temp_dir();
        for attempt in 0..32u64 {
            let mut random = [0u8; 8];
            getrandom::fill(&mut random).map_err(|_| Error::Internal)?;
            let suffix = u64::from_le_bytes(random) ^ attempt ^ u64::from(std::process::id());
            let path = base.join(format!("yas-fs-{}-{suffix:016x}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    set_private_directory(&path)?;
                    *slot = Some(path.clone());
                    return Ok(path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(map_io(error)),
            }
        }
        Err(Error::ResourceExhausted)
    }

    /// Resolve a browser drag item inside this YAS session's staging root.
    /// Only already-committed regular files are accepted; absolute paths,
    /// parent traversal, and symlink escapes are rejected.
    pub(crate) fn resolve_staging_file(&self, name: &str) -> Result<PathBuf, Error> {
        let relative = OsPath::new(name);
        if name.is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(Error::Invalid("invalid FS staging child"));
        }
        let root = fs::canonicalize(self.staging_root()?).map_err(map_io)?;
        let candidate = fs::canonicalize(root.join(relative)).map_err(map_io)?;
        if !candidate.starts_with(&root) || !candidate.is_file() {
            return Err(Error::NotFound);
        }
        Ok(candidate)
    }

    fn opened(&self, root_handle: u64) -> Result<OpenedRoot, Error> {
        self.ensure_open()?;
        self.inner
            .opened
            .lock()
            .unwrap()
            .get(&root_handle)
            .cloned()
            .ok_or(Error::NotFound)
    }

    fn ensure_open(&self) -> Result<(), Error> {
        if self.inner.closed.load(Ordering::Acquire) {
            Err(Error::Closed)
        } else {
            Ok(())
        }
    }

    fn replay(&self, id: &[u8; 16], fingerprint: [u8; 32]) -> Result<Option<ReplayValue>, Error> {
        let replay = self.inner.replay.lock().unwrap();
        let Some((known, value)) = replay.values.get(id) else {
            return Ok(None);
        };
        if *known != fingerprint {
            return Err(Error::Invalid(
                "FS operation ID reused with a different request",
            ));
        }
        Ok(Some(value.clone()))
    }

    fn remember_replay(&self, id: [u8; 16], fingerprint: [u8; 32], value: ReplayValue) {
        let mut replay = self.inner.replay.lock().unwrap();
        if replay.values.contains_key(&id) {
            return;
        }
        replay.values.insert(id, (fingerprint, value));
        replay.order.push_back(id);
        while replay.order.len() > MAX_REPLAYS {
            if let Some(oldest) = replay.order.pop_front() {
                replay.values.remove(&oldest);
            }
        }
    }

    fn next_watch_id(&self) -> Result<u32, Error> {
        for _ in 0..u16::MAX {
            let candidate = next_nonzero(&self.inner.next_watch).ok_or(Error::ResourceExhausted)?;
            let candidate = u32::try_from(candidate).map_err(|_| Error::ResourceExhausted)?;
            if candidate <= u32::from(u16::MAX)
                && !self.inner.watches.lock().unwrap().contains_key(&candidate)
            {
                return Ok(candidate);
            }
        }
        Err(Error::ResourceExhausted)
    }
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        for watch in self.watches.get_mut().unwrap().values_mut() {
            if let Some(handle) = watch.command.lock().unwrap().take() {
                let _ = handle.command(sync::WatchCommand::Stop);
            }
        }
        self.stages.get_mut().unwrap().clear();
        if let Some(path) = self.staging_root.get_mut().unwrap().take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.temp_path);
    }
}

impl Watch {
    pub(crate) fn root_handle(&self) -> u64 {
        self.root_handle
    }

    pub(crate) async fn recv(&mut self) -> Option<Result<WatchUpdate, Error>> {
        self.receiver.recv().await
    }

    pub(crate) fn ack(&self, update_id: u32) -> Result<(), Error> {
        let command = self.command.lock().unwrap();
        command
            .as_ref()
            .filter(|handle| handle.command(sync::WatchCommand::Ack(update_id)))
            .map(|_| ())
            .ok_or(Error::Closed)
    }

    pub(crate) fn close(&self) {
        if let Some(handle) = self.command.lock().unwrap().take() {
            let _ = handle.command(sync::WatchCommand::Stop);
        }
        if let Some(owner) = self.owner.upgrade() {
            owner.watches.lock().unwrap().remove(&self.id);
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.close();
    }
}

fn next_nonzero(counter: &AtomicU64) -> Option<u64> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(1).filter(|next| *next != 0)
        })
        .ok()
        .filter(|value| *value != 0)
}

fn map_io(error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::NotFound => Error::NotFound,
        io::ErrorKind::PermissionDenied => Error::Permission,
        _ => Error::Io(error.to_string()),
    }
}

fn canonical_root(path: PathBuf) -> Result<PathBuf, Error> {
    let metadata = fs::symlink_metadata(&path).map_err(map_io)?;
    if metadata.file_type().is_symlink() {
        let parent = path
            .parent()
            .ok_or(Error::Invalid("FS symlink root has no parent"))?;
        let name = path
            .file_name()
            .ok_or(Error::Invalid("FS symlink root has no name"))?;
        return Ok(fs::canonicalize(parent).map_err(map_io)?.join(name));
    }
    fs::canonicalize(path).map_err(map_io)
}

#[cfg(unix)]
fn platform_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    use std::os::unix::ffi::OsStringExt;
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(Error::Invalid("invalid FS platform path"));
    }
    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(windows)]
fn platform_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    let path = std::str::from_utf8(bytes).map_err(|_| Error::Invalid("invalid UTF-8 FS path"))?;
    if path.is_empty() || path.contains('\0') {
        return Err(Error::Invalid("invalid FS platform path"));
    }
    Ok(PathBuf::from(path))
}

#[cfg(all(not(unix), not(windows)))]
fn platform_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    let path = std::str::from_utf8(bytes).map_err(|_| Error::Invalid("invalid UTF-8 FS path"))?;
    Ok(PathBuf::from(path))
}

#[cfg(unix)]
fn component_os(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}

#[cfg(windows)]
fn component_os(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}

#[cfg(all(not(unix), not(windows)))]
fn component_os(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}

#[cfg(unix)]
fn component_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn component_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

#[cfg(all(not(unix), not(windows)))]
fn component_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

fn os_path_bytes(path: &OsPath) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    path.to_string_lossy().as_bytes().to_vec()
}

const fn path_model() -> u8 {
    #[cfg(windows)]
    {
        schema::fs::PATH_WINDOWS_UTF8 as u8
    }
    #[cfg(not(windows))]
    {
        schema::fs::PATH_POSIX_BYTES as u8
    }
}

const fn case_behavior() -> u8 {
    #[cfg(windows)]
    {
        schema::fs::CASE_PRESERVING_INSENSITIVE as u8
    }
    #[cfg(not(windows))]
    {
        schema::fs::CASE_SENSITIVE as u8
    }
}

fn append_wire_path(path: &mut PathBuf, relative: &wire::Path) -> Result<(), Error> {
    for component in &relative.components {
        let value = component_os(component);
        let mut parts = OsPath::new(&value).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(part)), None) if part == value.as_os_str() => path.push(part),
            _ => return Err(Error::Invalid("invalid FS path component")),
        }
    }
    Ok(())
}

fn relative_path(root: &Root, absolute: &OsPath) -> Result<wire::Path, Error> {
    let relative = if root.single_file {
        if absolute != root.path {
            return Err(Error::Invalid("path is outside single-file FS root"));
        }
        OsPath::new("")
    } else {
        absolute
            .strip_prefix(&root.path)
            .map_err(|_| Error::Invalid("path is outside FS root"))?
    };
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => components.push(component_bytes(value)),
            _ => return Err(Error::Invalid("invalid relative FS path")),
        }
    }
    Ok(wire::Path { components })
}

fn joined_path(root: &Root, relative: &wire::Path) -> Result<PathBuf, Error> {
    if root.single_file {
        if relative.components.is_empty() {
            return Ok(root.path.clone());
        }
        return Err(Error::Invalid(
            "single-file root only accepts the empty path",
        ));
    }
    let mut path = root.path.clone();
    append_wire_path(&mut path, relative)?;
    Ok(path)
}

/// Confine an existing target without following the final symlink.
fn confined_existing(
    root: &Root,
    relative: &wire::Path,
    follow_final: bool,
) -> Result<PathBuf, Error> {
    let target = joined_path(root, relative)?;
    if target == root.path {
        return Ok(target);
    }
    let parent = target
        .parent()
        .ok_or(Error::Invalid("FS target has no parent"))?;
    let parent = fs::canonicalize(parent).map_err(map_io)?;
    if !parent.starts_with(&root.path) {
        return Err(Error::Permission);
    }
    let metadata = fs::symlink_metadata(&target).map_err(map_io)?;
    if follow_final && metadata.file_type().is_symlink() {
        let canonical = fs::canonicalize(&target).map_err(map_io)?;
        if !canonical.starts_with(&root.path) {
            return Err(Error::Permission);
        }
        return Ok(canonical);
    }
    Ok(target)
}

fn unix_ns(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_nanos()).unwrap_or(i64::MAX),
    }
}

fn metadata_time(metadata: &fs::Metadata) -> i64 {
    metadata.modified().map(unix_ns).unwrap_or(0)
}

#[cfg(unix)]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn metadata_mode(_metadata: &fs::Metadata) -> u32 {
    0
}

fn read_link_bytes(path: &OsPath) -> Result<Vec<u8>, Error> {
    let target = fs::read_link(path).map_err(map_io)?;
    Ok(os_path_bytes(&target))
}

fn read_content_bounded(
    path: &OsPath,
    maximum: u64,
    follow_final: bool,
) -> Result<(Vec<u8>, i64), Error> {
    let metadata = if follow_final {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
    .map_err(map_io)?;
    if metadata.file_type().is_symlink() && !follow_final {
        let bytes = read_link_bytes(path)?;
        if bytes.len() as u64 > maximum {
            return Err(Error::TooLarge);
        }
        return Ok((bytes, metadata_time(&metadata)));
    }
    if !metadata.is_file() {
        return Err(Error::Invalid("FS content target is not a file"));
    }
    if metadata.len() > maximum {
        return Err(Error::TooLarge);
    }
    let file = File::open(path).map_err(map_io)?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(map_io)?;
    if bytes.len() as u64 > maximum {
        return Err(Error::TooLarge);
    }
    Ok((bytes, metadata_time(&metadata)))
}

fn hash_file(path: &OsPath) -> Result<[u8; 32], Error> {
    let mut file = File::open(path).map_err(map_io)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(map_io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn entry_signature(entry: &wire::EntryRecord) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(entry.path.components.len() as u64).to_le_bytes());
    for component in &entry.path.components {
        hasher.update(&(component.len() as u64).to_le_bytes());
        hasher.update(component);
    }
    hasher.update(&[entry.flags]);
    hasher.update(&entry.mode.to_le_bytes());
    hasher.update(&entry.modified_unix_ns.to_le_bytes());
    match &entry.body {
        wire::EntryBody::File {
            byte_len,
            content_hash,
            ..
        } => {
            hasher.update(&[schema::fs::ENTRY_FILE as u8]);
            hasher.update(&byte_len.to_le_bytes());
            hasher.update(content_hash);
        }
        wire::EntryBody::Directory => {
            hasher.update(&[schema::fs::ENTRY_DIRECTORY as u8]);
        }
        wire::EntryBody::Symlink {
            content_hash,
            target,
        } => {
            hasher.update(&[schema::fs::ENTRY_SYMLINK as u8]);
            hasher.update(content_hash);
            hasher.update(&(target.len() as u64).to_le_bytes());
            hasher.update(target);
        }
    }
    *hasher.finalize().as_bytes()
}

fn observe_entry(root: &Root, mut entry: wire::EntryRecord) -> wire::EntryRecord {
    entry.entry_revision = 1;
    let signature = entry_signature(&entry);
    let mut entries = root.entries.lock().unwrap();
    let revision = match entries.get(&entry.path) {
        Some(previous) if previous.signature == signature => previous.revision,
        _ => root
            .revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1)
            .max(1),
    };
    entry.entry_revision = revision;
    entries.insert(
        entry.path.clone(),
        EntryVersion {
            signature,
            revision,
        },
    );
    entry
}

fn observe_remove(root: &Root, path: &wire::Path) -> u64 {
    let mut entries = root.entries.lock().unwrap();
    let previous = entries.remove(path).map(|entry| entry.revision);
    previous.unwrap_or_else(|| {
        root.revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1)
            .max(1)
    })
}

fn stat_entry(
    root: &Root,
    path: &wire::Path,
    inline_max: usize,
    include_content: bool,
) -> Result<wire::EntryRecord, Error> {
    let absolute = confined_existing(root, path, false)?;
    let metadata = fs::symlink_metadata(&absolute).map_err(map_io)?;
    let file_type = metadata.file_type();
    let mut flags = 0u8;
    let mode = metadata_mode(&metadata);
    if mode & 0o111 != 0 {
        flags |= schema::fs::ENTRY_EXECUTABLE as u8;
    }
    if metadata.permissions().readonly() {
        flags |= schema::fs::ENTRY_READ_ONLY as u8;
    }
    if path
        .components
        .last()
        .is_some_and(|component| component.first() == Some(&b'.'))
    {
        flags |= schema::fs::ENTRY_HIDDEN as u8;
    }
    let body = if file_type.is_file() {
        let byte_len = metadata.len();
        let content_hash = hash_file(&absolute)?;
        let inline_content = if include_content && byte_len <= inline_max as u64 {
            Some(read_content_bounded(&absolute, inline_max as u64, false)?.0)
        } else {
            None
        };
        wire::EntryBody::File {
            byte_len,
            content_hash,
            inline_content,
        }
    } else if file_type.is_dir() {
        wire::EntryBody::Directory
    } else if file_type.is_symlink() {
        let target = read_link_bytes(&absolute)?;
        if fs::metadata(&absolute).is_ok_and(|metadata| metadata.is_dir()) {
            flags |= schema::fs::ENTRY_SYMLINK_DIRECTORY as u8;
        }
        wire::EntryBody::Symlink {
            content_hash: *blake3::hash(&target).as_bytes(),
            target,
        }
    } else {
        return Err(Error::Unsupported);
    };
    Ok(observe_entry(
        root,
        wire::EntryRecord {
            path: path.clone(),
            entry_revision: 1,
            flags,
            mode,
            modified_unix_ns: metadata_time(&metadata),
            body,
            extensions: Extensions::default(),
        },
    ))
}

fn translate_watch_event(
    root: &Root,
    event: sync::WatchEvent,
    inline_max: usize,
    include_content: bool,
) -> Result<WatchUpdate, Error> {
    let sync::WatchEvent::Update(update) = event else {
        let sync::WatchEvent::Closed(reason) = event else {
            unreachable!()
        };
        return Err(match reason {
            sync::CloseReason::RootGone => Error::NotFound,
            sync::CloseReason::PermissionLost => Error::Permission,
            sync::CloseReason::ResourceExhausted => Error::ResourceExhausted,
            sync::CloseReason::BackendFailed => Error::Unavailable,
            sync::CloseReason::ProtocolViolation => Error::Internal,
        });
    };
    let update_id = update.update_id;
    if update_id == 0 {
        return Err(Error::Internal);
    }
    let mut mutations = Vec::new();
    for record in update.records {
        match record {
            sync::WatchRecord::Upsert { path } => {
                let path = watch_path(&path)?;
                match stat_entry(root, &path, inline_max, include_content) {
                    Ok(mut entry) => {
                        if let Some(operation_id) =
                            root.operation_echoes.lock().unwrap().remove(&path)
                            && let Ok(extension) = wire::operation_id_extension(operation_id)
                        {
                            entry.extensions = Extensions(vec![extension]);
                        }
                        mutations.push(wire::StateMutation::Complete(entry));
                    }
                    Err(Error::NotFound) => {
                        let operation_id = root.operation_echoes.lock().unwrap().remove(&path);
                        mutations.push(wire::StateMutation::Remove(wire::RemoveRecord {
                            path: path.clone(),
                            removed_revision: observe_remove(root, &path),
                            operation_id,
                        }));
                    }
                    Err(Error::Unsupported) => {}
                    Err(error) => return Err(error),
                }
            }
            sync::WatchRecord::Delete { path } => {
                let path = watch_path(&path)?;
                let operation_id = root.operation_echoes.lock().unwrap().remove(&path);
                mutations.push(wire::StateMutation::Remove(wire::RemoveRecord {
                    removed_revision: observe_remove(root, &path),
                    path,
                    operation_id,
                }));
            }
            sync::WatchRecord::Move { from, to } => {
                let from = watch_path(&from)?;
                let to = watch_path(&to)?;
                let operation_id = {
                    let mut echoes = root.operation_echoes.lock().unwrap();
                    echoes.remove(&to).or_else(|| echoes.remove(&from))
                };
                root.revision.fetch_add(1, Ordering::AcqRel);
                {
                    let mut entries = root.entries.lock().unwrap();
                    if let Some(version) = entries.remove(&from) {
                        entries.insert(to.clone(), version);
                    }
                }
                mutations.push(wire::StateMutation::Move(wire::MoveRecord {
                    from,
                    to,
                    operation_id,
                }));
            }
        }
    }
    Ok(WatchUpdate {
        update_id,
        reset: update.reset,
        snapshot_end: update.snapshot_end,
        mutations,
    })
}

#[cfg(unix)]
fn watch_component(component: &str) -> Result<Vec<u8>, Error> {
    sync::unescape_to_bytes(component).ok_or(Error::Internal)
}

#[cfg(windows)]
fn watch_component(component: &str) -> Result<Vec<u8>, Error> {
    let wide = sync::unescape_to_wide(component).ok_or(Error::Internal)?;
    String::from_utf16(&wide)
        .map(|value| value.into_bytes())
        .map_err(|_| Error::Internal)
}

#[cfg(all(not(unix), not(windows)))]
fn watch_component(component: &str) -> Result<Vec<u8>, Error> {
    sync::unescape_to_bytes(component).ok_or(Error::Internal)
}

fn watch_path(path: &str) -> Result<wire::Path, Error> {
    if path.is_empty() {
        return Ok(wire::Path::default());
    }
    let components = path
        .split('/')
        .map(watch_component)
        .collect::<Result<Vec<_>, _>>()?;
    if components.iter().any(|component| component.is_empty()) {
        return Err(Error::Internal);
    }
    Ok(wire::Path { components })
}

fn sync_open_error(error: sync::OpenError) -> Error {
    match error.kind {
        sync::OpenErrorKind::NotFound => Error::NotFound,
        sync::OpenErrorKind::Permission => Error::Permission,
        sync::OpenErrorKind::ResourceExhausted => Error::ResourceExhausted,
        sync::OpenErrorKind::Invalid => Error::Invalid("invalid FS watch root"),
        sync::OpenErrorKind::Io => Error::Io(error.detail),
    }
}

fn status_for(error: &Error) -> u16 {
    match error {
        Error::NotFound => schema::core::status::NOT_FOUND,
        Error::Permission => schema::core::status::IO,
        Error::Conflict(_) => schema::core::status::CONFLICT,
        Error::ResourceExhausted | Error::TooLarge => schema::core::status::RESOURCE_EXHAUSTED,
        Error::Unsupported => schema::core::status::UNSUPPORTED,
        Error::Invalid(_) => schema::core::status::INVALID,
        Error::Unavailable | Error::Closed => schema::core::status::UNAVAILABLE,
        Error::Io(_) | Error::Internal => schema::core::status::INTERNAL,
    }
}

fn read_questions(root: &Root, questions: &[wire::ReadQuestion]) -> Result<QueryData, Error> {
    let mut records = Vec::with_capacity(questions.len());
    let mut bytes_used = 0usize;
    for (index, question) in questions.iter().enumerate() {
        let answer = read_question(root, question);
        let record = match answer {
            Ok(content) => wire::QueryReadRecord {
                question_index: u16::try_from(index).map_err(|_| Error::TooLarge)?,
                status: schema::core::status::OK,
                path: Some(question.path.clone()),
                content,
            },
            Err(error) => wire::QueryReadRecord {
                question_index: u16::try_from(index).map_err(|_| Error::TooLarge)?,
                status: status_for(&error),
                path: None,
                content: Vec::new(),
            },
        };
        let encoded = wire::QueryRecord::Read(record.clone())
            .to_typed_record()
            .map_err(|_| Error::Internal)?;
        let cost = encoded.body.len().saturating_add(8);
        if bytes_used.saturating_add(cost) > wire::MAX_QUERY_BYTES {
            return Err(Error::TooLarge);
        }
        bytes_used += cost;
        records.push(wire::QueryRecord::Read(record));
    }
    Ok(QueryData {
        records,
        next_cursor: Vec::new(),
        total_hint: questions.len() as u64,
        truncated: false,
    })
}

fn read_question(root: &Root, question: &wire::ReadQuestion) -> Result<Vec<u8>, Error> {
    let no_follow = question.flags & schema::fs::READ_NO_FOLLOW as u16 != 0;
    match question.kind {
        kind if kind == schema::fs::READ_STAT as u16 => stat_entry(root, &question.path, 0, false)?
            .encode()
            .map_err(|_| Error::Internal),
        kind if kind == schema::fs::READ_HASH as u16 => {
            let absolute = confined_existing(root, &question.path, !no_follow)?;
            let metadata = fs::symlink_metadata(&absolute).map_err(map_io)?;
            let hash = if metadata.file_type().is_symlink() && no_follow {
                *blake3::hash(&read_link_bytes(&absolute)?).as_bytes()
            } else {
                hash_file(&absolute)?
            };
            Ok(hash.to_vec())
        }
        kind if kind == schema::fs::READ_LINK_TARGET as u16 => {
            let absolute = confined_existing(root, &question.path, false)?;
            read_link_bytes(&absolute)
        }
        kind if kind == schema::fs::READ_CONTENT as u16 => {
            let absolute = confined_existing(root, &question.path, !no_follow)?;
            read_content_bounded(&absolute, wire::MAX_QUERY_BYTES as u64, !no_follow)
                .map(|value| value.0)
        }
        _ => Err(Error::Invalid("unknown FS READ question")),
    }
}

fn decode_cursor(cursor: &[u8]) -> Result<usize, Error> {
    match cursor {
        [] => Ok(0),
        bytes if bytes.len() == 8 => usize::try_from(u64::from_le_bytes(bytes.try_into().unwrap()))
            .map_err(|_| Error::Invalid("FS query cursor overflow")),
        _ => Err(Error::Invalid("malformed FS query cursor")),
    }
}

fn encode_cursor(index: usize, total: usize) -> Vec<u8> {
    if index >= total {
        Vec::new()
    } else {
        (index as u64).to_le_bytes().to_vec()
    }
}

fn enumerate_paths(
    root: &Root,
    include_ignored: bool,
) -> Result<Vec<(wire::Path, bool, bool)>, Error> {
    if root.single_file {
        return Ok(vec![(wire::Path::default(), false, false)]);
    }
    let visible = if include_ignored {
        let mut builder = WalkBuilder::new(&root.path);
        builder.hidden(false).follow_links(false).require_git(false);
        let mut visible = BTreeSet::new();
        for entry in builder.build() {
            let entry = entry.map_err(|error| Error::Io(error.to_string()))?;
            if entry.path() != root.path {
                visible.insert(relative_path(root, entry.path())?);
            }
        }
        Some(visible)
    } else {
        None
    };

    let mut builder = WalkBuilder::new(&root.path);
    builder.hidden(false).follow_links(false).require_git(false);
    if include_ignored {
        builder
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false);
    }
    let mut paths = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(|error| Error::Io(error.to_string()))?;
        if entry.path() == root.path {
            continue;
        }
        let relative = relative_path(root, entry.path())?;
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
        let ignored = visible
            .as_ref()
            .is_some_and(|visible| !visible.contains(&relative));
        paths.push((relative, is_dir, ignored));
        if paths.len() > wire::Limits::HARD.max_query_records as usize * 16 {
            return Err(Error::ResourceExhausted);
        }
    }
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(paths)
}

fn path_flat(path: &wire::Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, component) in path.components.iter().enumerate() {
        if index != 0 {
            bytes.push(b'/');
        }
        bytes.extend_from_slice(component);
    }
    bytes
}

fn ascii_fold(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().map(u8::to_ascii_lowercase).collect()
}

fn fuzzy_score(path: &[u8], query: &[u8], prefix: bool) -> Option<(usize, usize)> {
    if prefix {
        return path.starts_with(query).then_some((0, path.len()));
    }
    let mut at = 0usize;
    let mut first = None;
    for wanted in query {
        let offset = path[at..].iter().position(|byte| byte == wanted)?;
        at += offset;
        first.get_or_insert(at);
        at += 1;
    }
    Some((at.saturating_sub(first.unwrap_or(0)), path.len()))
}

fn search_root(root: &Root, request: &wire::Search) -> Result<QueryData, Error> {
    let include_ignored = request.flags & schema::fs::SEARCH_INCLUDE_IGNORED as u16 != 0;
    let case_sensitive = request.flags & schema::fs::SEARCH_CASE_SENSITIVE as u16 != 0;
    let prefix = request.flags & schema::fs::SEARCH_PREFIX as u16 != 0;
    let query = if case_sensitive {
        request.query.clone()
    } else {
        ascii_fold(&request.query)
    };
    let mut candidates = enumerate_paths(root, include_ignored)?
        .into_iter()
        .filter_map(|(path, is_dir, ignored)| {
            let flat = path_flat(&path);
            let comparable = if case_sensitive {
                flat
            } else {
                ascii_fold(&flat)
            };
            fuzzy_score(&comparable, &query, prefix).map(|score| (score, path, is_dir, ignored))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    page_paths(
        candidates
            .into_iter()
            .map(|(_, path, directory, ignored)| (path, directory, ignored))
            .collect(),
        decode_cursor(&request.cursor)?,
        request.max_results,
    )
}

fn index_root(root: &Root, request: &wire::Index) -> Result<QueryData, Error> {
    let include_files = request.flags & schema::fs::INDEX_INCLUDE_FILES as u16 != 0;
    let include_directories = request.flags & schema::fs::INDEX_INCLUDE_DIRECTORIES as u16 != 0;
    let include_ignored = request.flags & schema::fs::INDEX_INCLUDE_IGNORED as u16 != 0;
    let paths = enumerate_paths(root, include_ignored)?
        .into_iter()
        .filter(|(_, directory, _)| {
            (*directory && include_directories) || (!*directory && include_files)
        })
        .collect();
    page_paths(paths, decode_cursor(&request.cursor)?, request.max_results)
}

fn page_paths(
    paths: Vec<(wire::Path, bool, bool)>,
    start: usize,
    requested: u16,
) -> Result<QueryData, Error> {
    let total = paths.len();
    let maximum = if requested == 0 {
        DEFAULT_QUERY_RESULTS
    } else {
        requested as usize
    }
    .min(wire::MAX_QUERY_RECORDS);
    let start = start.min(total);
    let mut records = Vec::new();
    let mut bytes_used = 0usize;
    for (path, directory, ignored) in paths.iter().skip(start).take(maximum) {
        let mut flags = 0u16;
        if *directory {
            flags |= schema::fs::QUERY_PATH_DIRECTORY as u16;
        }
        if *ignored {
            flags |= schema::fs::QUERY_PATH_IGNORED as u16;
        }
        let record = wire::QueryRecord::Path(wire::QueryPathRecord {
            path: path.clone(),
            flags,
        });
        let cost = query_record_size(&record)?;
        if bytes_used.saturating_add(cost) > wire::MAX_QUERY_BYTES {
            if records.is_empty() {
                return Err(Error::TooLarge);
            }
            break;
        }
        bytes_used += cost;
        records.push(record);
    }
    let end = start.saturating_add(records.len());
    Ok(QueryData {
        records,
        next_cursor: encode_cursor(end, total),
        total_hint: total as u64,
        truncated: end < total,
    })
}

fn query_record_size(record: &wire::QueryRecord) -> Result<usize, Error> {
    record
        .to_typed_record()
        .map(|record| record.body.len().saturating_add(8))
        .map_err(|_| Error::Internal)
}

fn build_grep_regex(request: &wire::Grep) -> Result<Regex, Error> {
    let pattern = if request.flags & schema::fs::GREP_REGEX as u16 != 0 {
        std::str::from_utf8(&request.query)
            .map_err(|_| Error::Invalid("FS GREP regex is not UTF-8"))?
            .to_owned()
    } else {
        regex::escape(
            std::str::from_utf8(&request.query)
                .map_err(|_| Error::Invalid("FS GREP query is not UTF-8"))?,
        )
    };
    let pattern = if request.flags & schema::fs::GREP_WORD as u16 != 0 {
        format!(r"(?-u:\b(?:{pattern})\b)")
    } else {
        pattern
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(request.flags & schema::fs::GREP_CASE_SENSITIVE as u16 == 0)
        .build()
        .map_err(|_| Error::Invalid("invalid FS GREP regular expression"))
}

fn grep_root(root: &Root, request: &wire::Grep) -> Result<QueryData, Error> {
    let regex = build_grep_regex(request)?;
    let include_ignored = request.flags & schema::fs::GREP_INCLUDE_IGNORED as u16 != 0;
    let paths = enumerate_paths(root, include_ignored)?;
    let (start_path, start_match) = decode_grep_cursor(&request.cursor)?;
    let start_path = start_path.min(paths.len());
    let max_results = if request.max_results == 0 {
        DEFAULT_QUERY_RESULTS
    } else {
        request.max_results as usize
    }
    .min(wire::MAX_QUERY_RECORDS);
    if max_results < 2 {
        return Err(Error::Invalid(
            "FS GREP max_results cannot hold a file and match record",
        ));
    }
    let max_per_file = if request.max_per_file == 0 {
        max_results
    } else {
        request.max_per_file as usize
    };
    let mut records = Vec::new();
    let mut bytes_used = 0usize;
    let mut next = None;
    for (path_index, (path, directory, ignored)) in paths.iter().enumerate().skip(start_path) {
        if *directory {
            continue;
        }
        let absolute = joined_path(root, path)?;
        let metadata = match fs::metadata(&absolute) {
            Ok(metadata)
                if metadata.is_file() && metadata.len() <= wire::MAX_QUERY_BYTES as u64 =>
            {
                metadata
            }
            _ => continue,
        };
        let _ = metadata;
        let bytes = match fs::read(&absolute) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let resume_match = if path_index == start_path {
            start_match
        } else {
            0
        };
        let mut matches = Vec::new();
        for found in regex
            .find_iter(text.as_bytes())
            .take(max_per_file)
            .skip(resume_match)
        {
            let (line, column) = byte_line_column(text.as_bytes(), found.start());
            let (end_line, end_column) = byte_line_column(text.as_bytes(), found.end());
            let display = line_display(text, line as usize);
            matches.push(wire::QueryGrepMatchRecord {
                file_index: 0,
                line,
                column,
                end_line,
                end_column,
                text: display,
            });
        }
        if matches.is_empty() {
            continue;
        }
        if records.len().saturating_add(2) > max_results {
            next = Some((path_index, resume_match));
            break;
        }
        let file_index = records
            .iter()
            .filter(|record| matches!(record, wire::QueryRecord::GrepFile(_)))
            .count() as u32;
        let mut file_record = wire::QueryRecord::GrepFile(wire::QueryGrepFileRecord {
            file_index,
            match_count: 0,
            flags: if *ignored {
                schema::fs::QUERY_GREP_FILE_IGNORED as u16
            } else {
                0
            },
            path: path.clone(),
        });
        let file_cost = query_record_size(&file_record)?;
        let mut included = Vec::new();
        let mut included_cost = 0usize;
        for mut found in matches {
            found.file_index = file_index;
            let record = wire::QueryRecord::GrepMatch(found);
            let cost = query_record_size(&record)?;
            if records
                .len()
                .saturating_add(1)
                .saturating_add(included.len())
                >= max_results
                || bytes_used
                    .saturating_add(file_cost)
                    .saturating_add(included_cost)
                    .saturating_add(cost)
                    > wire::MAX_QUERY_BYTES
            {
                break;
            }
            included_cost += cost;
            included.push(record);
        }
        if included.is_empty() {
            if records.is_empty() {
                return Err(Error::TooLarge);
            }
            next = Some((path_index, resume_match));
            break;
        }
        if let wire::QueryRecord::GrepFile(value) = &mut file_record {
            value.match_count = included.len() as u32;
        }
        bytes_used = bytes_used
            .saturating_add(file_cost)
            .saturating_add(included_cost);
        records.push(file_record);
        records.extend(included);

        let consumed = records
            .iter()
            .rev()
            .take_while(|record| matches!(record, wire::QueryRecord::GrepMatch(_)))
            .count();
        if resume_match.saturating_add(consumed) < max_per_file
            && regex
                .find_iter(text.as_bytes())
                .take(max_per_file)
                .nth(resume_match.saturating_add(consumed))
                .is_some()
        {
            next = Some((path_index, resume_match.saturating_add(consumed)));
            break;
        }
    }
    let truncated = next.is_some();
    Ok(QueryData {
        records,
        next_cursor: next.map_or_else(Vec::new, |(path, found)| encode_grep_cursor(path, found)),
        total_hint: 0,
        truncated,
    })
}

fn decode_grep_cursor(cursor: &[u8]) -> Result<(usize, usize), Error> {
    match cursor {
        [] => Ok((0, 0)),
        bytes if bytes.len() == 8 => Ok((decode_cursor(bytes)?, 0)),
        bytes if bytes.len() == 16 => {
            let path = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            let found = u64::from_le_bytes(bytes[8..].try_into().unwrap());
            Ok((
                usize::try_from(path)
                    .map_err(|_| Error::Invalid("FS GREP path cursor overflow"))?,
                usize::try_from(found)
                    .map_err(|_| Error::Invalid("FS GREP match cursor overflow"))?,
            ))
        }
        _ => Err(Error::Invalid("malformed FS GREP cursor")),
    }
}

fn encode_grep_cursor(path: usize, found: usize) -> Vec<u8> {
    let mut cursor = Vec::with_capacity(16);
    cursor.extend_from_slice(&(path as u64).to_le_bytes());
    cursor.extend_from_slice(&(found as u64).to_le_bytes());
    cursor
}

fn byte_line_column(bytes: &[u8], offset: usize) -> (u32, u32) {
    let offset = offset.min(bytes.len());
    let line = bytes[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count();
    let line_start = bytes[..offset]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    (
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(offset.saturating_sub(line_start)).unwrap_or(u32::MAX),
    )
}

fn line_display(text: &str, line: usize) -> String {
    let value = text.lines().nth(line).unwrap_or("");
    let mut end = value.len().min(schema::fs::MAX_GREP_LINE_BYTES as usize);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].to_owned()
}

static MUTATION_LOCK: StdMutex<()> = StdMutex::new(());

fn request_fingerprint(value: &impl Encode) -> Result<[u8; 32], Error> {
    let bytes = value
        .encode()
        .map_err(|_| Error::Invalid("invalid FS request"))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn unique_temp_path(directory: &OsPath, prefix: &str) -> Result<PathBuf, Error> {
    for attempt in 0..32u64 {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).map_err(|_| Error::Internal)?;
        let suffix = u64::from_le_bytes(random) ^ attempt;
        let path = directory.join(format!(".{prefix}-{suffix:016x}.tmp"));
        if fs::symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err(Error::ResourceExhausted)
}

fn conflict_detail(root: &Root, path: &wire::Path) -> wire::ConflictDetail {
    let current = stat_entry(root, path, 0, false).ok();
    wire::ConflictDetail {
        path: path.clone(),
        current_present: current.is_some(),
        current_entry_revision: current.as_ref().map_or(0, |entry| entry.entry_revision),
        modified_unix_ns: current.as_ref().map_or(0, |entry| entry.modified_unix_ns),
        current_hash: current.as_ref().and_then(entry_hash),
    }
}

fn entry_hash(entry: &wire::EntryRecord) -> Option<[u8; 32]> {
    match entry.body {
        wire::EntryBody::File { content_hash, .. }
        | wire::EntryBody::Symlink { content_hash, .. } => Some(content_hash),
        wire::EntryBody::Directory => None,
    }
}

fn check_precondition(
    root: &Root,
    path: &wire::Path,
    precondition: &wire::Precondition,
) -> Result<(), Error> {
    match precondition {
        wire::Precondition::Any => Ok(()),
        wire::Precondition::Absent => {
            let target = joined_path(root, path)?;
            match fs::symlink_metadata(&target) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                _ => Err(Error::Conflict(conflict_detail(root, path))),
            }
        }
        wire::Precondition::Revision(expected) => {
            let current = stat_entry(root, path, 0, false)
                .map_err(|_| Error::Conflict(conflict_detail(root, path)))?;
            if current.entry_revision == *expected {
                Ok(())
            } else {
                Err(Error::Conflict(conflict_detail(root, path)))
            }
        }
        wire::Precondition::Hash(expected) => {
            let current = stat_entry(root, path, 0, false)
                .map_err(|_| Error::Conflict(conflict_detail(root, path)))?;
            if entry_hash(&current).is_some_and(|hash| hash == *expected) {
                Ok(())
            } else {
                Err(Error::Conflict(conflict_detail(root, path)))
            }
        }
    }
}

fn mutation_target(root: &Root, path: &wire::Path, create_parents: bool) -> Result<PathBuf, Error> {
    let target = joined_path(root, path)?;
    if target == root.path && !root.single_file {
        return Err(Error::Invalid("cannot replace an FS directory root"));
    }
    let parent = target
        .parent()
        .ok_or(Error::Invalid("FS target has no parent"))?;
    if create_parents {
        create_parents_confined(root, parent)?;
    }
    let canonical_parent = fs::canonicalize(parent).map_err(map_io)?;
    let allowed = if root.single_file {
        root.path
            .parent()
            .is_some_and(|value| canonical_parent == value)
    } else {
        canonical_parent.starts_with(&root.path)
    };
    if !allowed {
        return Err(Error::Permission);
    }
    Ok(target)
}

fn create_parents_confined(root: &Root, parent: &OsPath) -> Result<(), Error> {
    if root.single_file {
        return Ok(());
    }
    let relative = parent
        .strip_prefix(&root.path)
        .map_err(|_| Error::Permission)?;
    let mut current = root.path.clone();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(Error::Invalid("invalid FS parent path"));
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(Error::Permission),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(map_io)?;
            }
            Err(error) => return Err(map_io(error)),
        }
    }
    let canonical = fs::canonicalize(parent).map_err(map_io)?;
    if !canonical.starts_with(&root.path) {
        return Err(Error::Permission);
    }
    Ok(())
}

fn commit_stage(
    stage: Stage,
    operation_id: [u8; 16],
    flags: u16,
) -> Result<wire::CommitResult, Error> {
    let _mutation = MUTATION_LOCK.lock().unwrap();
    check_precondition(&stage.root, &stage.path, &stage.precondition)?;
    let create_parents = stage.flags & schema::fs::STAGE_CREATE_PARENTS as u16 != 0;
    let target = mutation_target(&stage.root, &stage.path, create_parents)?;
    if fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(Error::Conflict(conflict_detail(&stage.root, &stage.path)));
    }
    let parent = target
        .parent()
        .ok_or(Error::Invalid("FS target has no parent"))?;
    let temp = unique_temp_path(parent, "yas-commit")?;
    let result = (|| {
        fs::copy(&stage.temp_path, &temp).map_err(map_io)?;
        set_file_mode(&temp, stage.mode)?;
        if flags & schema::fs::COMMIT_SYNC_DATA as u16 != 0 {
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&temp)
                .and_then(|file| file.sync_all())
                .map_err(map_io)?;
        }
        fs::rename(&temp, &target).map_err(map_io)?;
        if flags & schema::fs::COMMIT_SYNC_DIRECTORY as u16 != 0 {
            sync_directory(parent)?;
        }
        Ok::<(), Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    stage
        .root
        .operation_echoes
        .lock()
        .unwrap()
        .insert(stage.path.clone(), operation_id);
    let entry = stat_entry(&stage.root, &stage.path, 0, false)?;
    Ok(wire::CommitResult {
        root_revision: stage.root.revision.load(Ordering::Acquire).max(1),
        entry_revision: entry.entry_revision,
        modified_unix_ns: entry.modified_unix_ns,
        content_hash: entry_hash(&entry).ok_or(Error::Internal)?,
    })
}

fn apply_items(
    root: &Root,
    operation_id: [u8; 16],
    items: &[wire::ApplyItem],
) -> Result<wire::ApplyResult, Error> {
    let _mutation = MUTATION_LOCK.lock().unwrap();
    let mut results = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let outcome = apply_one(root, operation_id, item);
        let result = match outcome {
            Ok(entry) => wire::ApplyItemResult {
                index: index as u16,
                status: schema::core::status::OK,
                entry_revision: entry.entry_revision,
                modified_unix_ns: entry.modified_unix_ns,
                content_hash: entry_hash(&entry),
                detail: String::new(),
            },
            Err(Error::Conflict(detail)) => wire::ApplyItemResult {
                index: index as u16,
                status: schema::core::status::CONFLICT,
                entry_revision: detail.current_entry_revision,
                modified_unix_ns: detail.modified_unix_ns,
                content_hash: detail.current_hash,
                detail: "filesystem precondition failed".to_owned(),
            },
            Err(error) => wire::ApplyItemResult {
                index: index as u16,
                status: status_for(&error),
                entry_revision: 0,
                modified_unix_ns: 0,
                content_hash: None,
                detail: bounded_detail(&error.to_string()),
            },
        };
        results.push(result);
    }
    Ok(wire::ApplyResult {
        root_revision: root.revision.load(Ordering::Acquire).max(1),
        items: results,
        extensions: Extensions::default(),
    })
}

fn apply_one(
    root: &Root,
    operation_id: [u8; 16],
    item: &wire::ApplyItem,
) -> Result<wire::EntryRecord, Error> {
    match item {
        wire::ApplyItem::WriteInline {
            path,
            precondition,
            create_parents,
            mode,
            content,
        } => {
            check_precondition(root, path, precondition)?;
            atomic_write(root, path, *create_parents, *mode, content)?;
            mark_operation(root, path, operation_id);
            stat_entry(root, path, 0, false)
        }
        wire::ApplyItem::Mkdir {
            path,
            precondition,
            create_parents,
            mode,
        } => {
            check_precondition(root, path, precondition)?;
            let target = mutation_target(root, path, *create_parents)?;
            let mut builder = fs::DirBuilder::new();
            set_directory_builder_mode(&mut builder, *mode);
            match builder.create(&target) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists && target.is_dir() => {}
                Err(error) => return Err(map_io(error)),
            }
            mark_operation(root, path, operation_id);
            stat_entry(root, path, 0, false)
        }
        wire::ApplyItem::Remove {
            path,
            precondition,
            flags,
        } => {
            check_precondition(root, path, precondition)?;
            let target = mutation_target(root, path, false)?;
            let metadata = fs::symlink_metadata(&target).map_err(map_io)?;
            if metadata.is_dir() {
                if flags & schema::fs::REMOVE_RECURSIVE as u16 != 0 {
                    fs::remove_dir_all(&target).map_err(map_io)?;
                } else {
                    fs::remove_dir(&target).map_err(map_io)?;
                }
            } else {
                fs::remove_file(&target).map_err(map_io)?;
            }
            let revision = observe_remove(root, path);
            mark_operation(root, path, operation_id);
            Ok(removed_placeholder(path.clone(), revision))
        }
        wire::ApplyItem::Rename {
            from,
            to,
            precondition,
            create_parents,
        } => {
            check_precondition(root, from, precondition)?;
            let source = confined_existing(root, from, false)?;
            let target = mutation_target(root, to, *create_parents)?;
            fs::rename(source, target).map_err(map_io)?;
            observe_remove(root, from);
            mark_operation(root, to, operation_id);
            stat_entry(root, to, 0, false)
        }
        wire::ApplyItem::Symlink {
            path,
            target,
            precondition,
            create_parents,
        } => {
            check_precondition(root, path, precondition)?;
            let link = mutation_target(root, path, *create_parents)?;
            atomic_symlink(target, &link)?;
            mark_operation(root, path, operation_id);
            stat_entry(root, path, 0, false)
        }
        wire::ApplyItem::Hardlink {
            source,
            target,
            precondition,
            create_parents,
        } => {
            check_precondition(root, target, precondition)?;
            let source_path = confined_existing(root, source, false)?;
            if !fs::symlink_metadata(&source_path).is_ok_and(|metadata| metadata.is_file()) {
                return Err(Error::Invalid("FS hardlink source is not a file"));
            }
            let target_path = mutation_target(root, target, *create_parents)?;
            atomic_hardlink(&source_path, &target_path)?;
            mark_operation(root, target, operation_id);
            stat_entry(root, target, 0, false)
        }
    }
}

fn mark_operation(root: &Root, path: &wire::Path, operation_id: [u8; 16]) {
    root.operation_echoes
        .lock()
        .unwrap()
        .insert(path.clone(), operation_id);
}

fn removed_placeholder(path: wire::Path, revision: u64) -> wire::EntryRecord {
    wire::EntryRecord {
        path,
        entry_revision: revision.max(1),
        flags: 0,
        mode: 0,
        modified_unix_ns: 0,
        body: wire::EntryBody::Directory,
        extensions: Extensions::default(),
    }
}

fn atomic_write(
    root: &Root,
    path: &wire::Path,
    create_parents: bool,
    mode: u32,
    content: &[u8],
) -> Result<(), Error> {
    let target = mutation_target(root, path, create_parents)?;
    if fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(Error::Conflict(conflict_detail(root, path)));
    }
    let parent = target
        .parent()
        .ok_or(Error::Invalid("FS target has no parent"))?;
    let temp = unique_temp_path(parent, "yas-write")?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(map_io)?;
        file.write_all(content).map_err(map_io)?;
        file.flush().map_err(map_io)?;
        set_file_mode(&temp, mode)?;
        fs::rename(&temp, &target).map_err(map_io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn atomic_hardlink(source: &OsPath, target: &OsPath) -> Result<(), Error> {
    let parent = target
        .parent()
        .ok_or(Error::Invalid("FS target has no parent"))?;
    let temp = unique_temp_path(parent, "yas-hardlink")?;
    let result = fs::hard_link(source, &temp)
        .map_err(map_io)
        .and_then(|()| fs::rename(&temp, target).map_err(map_io));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(unix)]
fn atomic_symlink(target: &[u8], link: &OsPath) -> Result<(), Error> {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    let parent = link
        .parent()
        .ok_or(Error::Invalid("FS target has no parent"))?;
    let temp = unique_temp_path(parent, "yas-symlink")?;
    let result = symlink(OsString::from_vec(target.to_vec()), &temp)
        .map_err(map_io)
        .and_then(|()| fs::rename(&temp, link).map_err(map_io));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(windows)]
fn atomic_symlink(target: &[u8], link: &OsPath) -> Result<(), Error> {
    use std::os::windows::fs::symlink_file;
    let target =
        std::str::from_utf8(target).map_err(|_| Error::Invalid("invalid UTF-8 symlink"))?;
    let parent = link
        .parent()
        .ok_or(Error::Invalid("FS target has no parent"))?;
    let temp = unique_temp_path(parent, "yas-symlink")?;
    let result = symlink_file(target, &temp)
        .map_err(map_io)
        .and_then(|()| fs::rename(&temp, link).map_err(map_io));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(all(not(unix), not(windows)))]
fn atomic_symlink(_target: &[u8], _link: &OsPath) -> Result<(), Error> {
    Err(Error::Unsupported)
}

#[cfg(unix)]
fn set_file_mode(path: &OsPath, mode: u32) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    if mode != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(map_io)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_file_mode(_path: &OsPath, _mode: u32) -> Result<(), Error> {
    Ok(())
}

#[cfg(unix)]
fn set_directory_builder_mode(builder: &mut fs::DirBuilder, mode: u32) {
    use std::os::unix::fs::DirBuilderExt;
    if mode != 0 {
        builder.mode(mode);
    }
}

#[cfg(not(unix))]
fn set_directory_builder_mode(_builder: &mut fs::DirBuilder, _mode: u32) {}

#[cfg(unix)]
fn sync_directory(path: &OsPath) -> Result<(), Error> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(map_io)
}

#[cfg(not(unix))]
fn sync_directory(_path: &OsPath) -> Result<(), Error> {
    Ok(())
}

fn bounded_detail(detail: &str) -> String {
    let mut end = detail.len().min(4096);
    while !detail.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    detail[..end].to_owned()
}

fn conflict_for(root: &Root, path: &wire::Path) -> Error {
    Error::Conflict(conflict_detail(root, path))
}

#[cfg(unix)]
fn set_private_directory(path: &OsPath) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(map_io)
}

#[cfg(not(unix))]
fn set_private_directory(_path: &OsPath) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let base = std::env::temp_dir();
            for attempt in 0..32u64 {
                let path = base.join(format!(
                    "yas-fs-test-{}-{attempt}",
                    u64::from(std::process::id()) << 32
                        ^ SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_nanos() as u64
                ));
                if fs::create_dir(&path).is_ok() {
                    return Self(path);
                }
            }
            panic!("could not create FS test directory")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_root(directory: &TestDir) -> Root {
        let path = fs::canonicalize(&directory.0).unwrap();
        Root {
            handle: 1,
            canonical_path: os_path_bytes(&path),
            single_file: false,
            path,
            revision: AtomicU64::new(1),
            entries: StdMutex::new(BTreeMap::new()),
            operation_echoes: StdMutex::new(BTreeMap::new()),
        }
    }

    fn path(components: &[&[u8]]) -> wire::Path {
        wire::Path {
            components: components.iter().map(|value| value.to_vec()).collect(),
        }
    }

    #[test]
    fn stat_uses_full_hash_and_monotonic_entry_revisions() {
        let directory = TestDir::new();
        fs::write(directory.0.join("value"), b"first").unwrap();
        let root = test_root(&directory);
        let relative = path(&[b"value"]);
        let first = stat_entry(&root, &relative, wire::MAX_INLINE_BYTES, true).unwrap();
        assert_eq!(entry_hash(&first), Some(*blake3::hash(b"first").as_bytes()));
        assert!(matches!(
            first.body,
            wire::EntryBody::File {
                inline_content: Some(ref bytes),
                ..
            } if bytes == b"first"
        ));
        let unchanged = stat_entry(&root, &relative, 0, false).unwrap();
        assert_eq!(unchanged.entry_revision, first.entry_revision);

        fs::write(directory.0.join("value"), b"second").unwrap();
        let changed = stat_entry(&root, &relative, 0, false).unwrap();
        assert!(changed.entry_revision > first.entry_revision);
        assert_eq!(
            entry_hash(&changed),
            Some(*blake3::hash(b"second").as_bytes())
        );
    }

    #[test]
    fn path_resolution_rejects_intermediate_symlink_escape() {
        let directory = TestDir::new();
        let outside = TestDir::new();
        fs::write(outside.0.join("secret"), b"nope").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside.0, directory.0.join("escape")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside.0, directory.0.join("escape")).unwrap();
        let root = test_root(&directory);
        let error = confined_existing(&root, &path(&[b"escape", b"secret"]), false).unwrap_err();
        assert_eq!(error, Error::Permission);
    }

    #[cfg(unix)]
    #[test]
    fn raw_non_utf8_names_round_trip_without_percent_encoding() {
        use std::os::unix::ffi::OsStringExt;
        let directory = TestDir::new();
        let name = OsString::from_vec(vec![b'x', 0xff]);
        // Path encoding is independent of filesystem support for these names
        // (APFS rejects non-UTF-8 filenames).
        let root = test_root(&directory);
        let relative = relative_path(&root, &directory.0.join(&name)).unwrap();
        assert_eq!(relative.components, vec![vec![b'x', 0xff]]);
        assert_eq!(
            joined_path(&root, &relative).unwrap(),
            directory.0.join(name)
        );
    }

    #[test]
    fn native_watch_updates_become_typed_full_hash_state() {
        let directory = TestDir::new();
        fs::write(directory.0.join("watched"), b"contents").unwrap();
        let root = test_root(&directory);
        let update = translate_watch_event(
            &root,
            sync::WatchEvent::Update(sync::WatchUpdate {
                update_id: 7,
                reset: true,
                snapshot_end: true,
                records: vec![sync::WatchRecord::Upsert {
                    path: "watched".to_owned(),
                }],
            }),
            32,
            true,
        )
        .unwrap();
        assert_eq!(update.update_id, 7);
        assert!(update.reset && update.snapshot_end);
        let wire::StateMutation::Complete(entry) = &update.mutations[0] else {
            panic!("expected complete entry")
        };
        assert_eq!(
            entry_hash(entry),
            Some(*blake3::hash(b"contents").as_bytes())
        );
    }

    #[test]
    fn staged_commit_rechecks_hash_and_lands_atomically() {
        let directory = TestDir::new();
        let root = Arc::new(test_root(&directory));
        let bytes = b"staged bytes";
        let stage_path = directory.0.join("stage.tmp");
        fs::write(&stage_path, bytes).unwrap();
        let mut hasher = blake3::Hasher::new();
        hasher.update(bytes);
        let stage = Stage {
            root: root.clone(),
            path: path(&[b"landed"]),
            precondition: wire::Precondition::Absent,
            flags: 0,
            mode: 0,
            byte_len: bytes.len() as u64,
            content_hash: *blake3::hash(bytes).as_bytes(),
            temp_path: stage_path,
            file: None,
            hasher,
            received: bytes.len() as u64,
            sealed: true,
        };
        let result = commit_stage(stage, [1; 16], 0).unwrap();
        assert_eq!(fs::read(directory.0.join("landed")).unwrap(), bytes);
        assert_eq!(result.content_hash, *blake3::hash(bytes).as_bytes());
        assert!(result.root_revision >= result.entry_revision);
        assert_eq!(
            root.operation_echoes
                .lock()
                .unwrap()
                .get(&path(&[b"landed"])),
            Some(&[1; 16])
        );
    }

    #[test]
    fn apply_runs_typed_write_rename_and_remove() {
        let directory = TestDir::new();
        let root = test_root(&directory);
        let items = vec![
            wire::ApplyItem::WriteInline {
                path: path(&[b"a"]),
                precondition: wire::Precondition::Absent,
                create_parents: false,
                mode: 0,
                content: b"one".to_vec(),
            },
            wire::ApplyItem::Rename {
                from: path(&[b"a"]),
                to: path(&[b"b"]),
                precondition: wire::Precondition::Any,
                create_parents: false,
            },
            wire::ApplyItem::Remove {
                path: path(&[b"b"]),
                precondition: wire::Precondition::Hash(*blake3::hash(b"one").as_bytes()),
                flags: 0,
            },
        ];
        let result = apply_items(&root, [2; 16], &items).unwrap();
        assert_eq!(result.items.len(), 3);
        assert!(
            result
                .items
                .iter()
                .all(|item| item.status == schema::core::status::OK)
        );
        assert!(!directory.0.join("b").exists());
    }

    #[test]
    fn query_cursors_are_bounded_and_resumable() {
        let directory = TestDir::new();
        fs::write(directory.0.join("alpha.txt"), b"needle alpha\n").unwrap();
        fs::write(directory.0.join("beta.txt"), b"needle beta\n").unwrap();
        let root = test_root(&directory);
        let first = index_root(
            &root,
            &wire::Index {
                root_handle: 1,
                flags: schema::fs::INDEX_INCLUDE_FILES as u16,
                max_results: 1,
                cursor: Vec::new(),
                initial_receive_credit: 0,
                extensions: Extensions::default(),
            },
        )
        .unwrap();
        assert_eq!(first.records.len(), 1);
        assert!(first.truncated && !first.next_cursor.is_empty());
        let second = index_root(
            &root,
            &wire::Index {
                root_handle: 1,
                flags: schema::fs::INDEX_INCLUDE_FILES as u16,
                max_results: 1,
                cursor: first.next_cursor,
                initial_receive_credit: 0,
                extensions: Extensions::default(),
            },
        )
        .unwrap();
        assert_eq!(second.records.len(), 1);
        assert!(second.next_cursor.is_empty());

        let grep = grep_root(
            &root,
            &wire::Grep {
                root_handle: 1,
                flags: 0,
                max_results: 8,
                max_per_file: 2,
                query: b"needle".to_vec(),
                cursor: Vec::new(),
                initial_receive_credit: 0,
                extensions: Extensions::default(),
            },
        )
        .unwrap();
        assert_eq!(
            grep.records
                .iter()
                .filter(|record| matches!(record, wire::QueryRecord::GrepFile(_)))
                .count(),
            2
        );
    }

    #[test]
    fn include_ignored_marks_records_instead_of_losing_provenance() {
        let directory = TestDir::new();
        fs::write(directory.0.join(".gitignore"), b"ignored.txt\n").unwrap();
        fs::write(directory.0.join("visible.txt"), b"visible").unwrap();
        fs::write(directory.0.join("ignored.txt"), b"ignored").unwrap();
        let root = test_root(&directory);
        let page = index_root(
            &root,
            &wire::Index {
                root_handle: 1,
                flags: (schema::fs::INDEX_INCLUDE_FILES | schema::fs::INDEX_INCLUDE_IGNORED) as u16,
                max_results: 32,
                cursor: Vec::new(),
                initial_receive_credit: 0,
                extensions: Extensions::default(),
            },
        )
        .unwrap();
        let ignored = page.records.iter().find_map(|record| match record {
            wire::QueryRecord::Path(value) if path_flat(&value.path) == b"ignored.txt".to_vec() => {
                Some(value.flags)
            }
            _ => None,
        });
        assert_eq!(
            ignored.unwrap() & schema::fs::QUERY_PATH_IGNORED as u16,
            schema::fs::QUERY_PATH_IGNORED as u16
        );
    }

    #[test]
    fn grep_cursor_resumes_within_one_file() {
        let directory = TestDir::new();
        fs::write(
            directory.0.join("many.txt"),
            b"needle one\nneedle two\nneedle three\n",
        )
        .unwrap();
        let root = test_root(&directory);
        let mut cursor = Vec::new();
        let mut matches = 0usize;
        for page_index in 0..3 {
            let page = grep_root(
                &root,
                &wire::Grep {
                    root_handle: 1,
                    flags: 0,
                    max_results: 2,
                    max_per_file: 3,
                    query: b"needle".to_vec(),
                    cursor,
                    initial_receive_credit: 0,
                    extensions: Extensions::default(),
                },
            )
            .unwrap();
            matches += page
                .records
                .iter()
                .filter(|record| matches!(record, wire::QueryRecord::GrepMatch(_)))
                .count();
            if page_index < 2 {
                assert!(page.truncated);
                assert_eq!(page.next_cursor.len(), 16);
            } else {
                assert!(!page.truncated);
                assert!(page.next_cursor.is_empty());
            }
            cursor = page.next_cursor;
        }
        assert_eq!(matches, 3);
    }
}
