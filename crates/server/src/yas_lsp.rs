//! Native YAS LSP semantics over the daemon-owned LSP engine.
//!
//! Request correlation, common State revisions, and Transfer credit remain in
//! `yas.rs`. This module owns workspace attachments, exact buffer CAS state,
//! and conversion between engine records and typed YAS values.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use tokio::sync::{mpsc, oneshot};
use yas_wire::Encode;
use yas_wire::codec::Extensions;
use yas_wire::core::{RuntimeState, Status};
use yas_wire::fs as fs_wire;
use yas_wire::lsp as wire;

use super::{AppState, resolve_term_cwd, yas_fs};

const STREAM_QUEUE: usize = 8;
const MAX_REPLAYS: usize = 1_024;
const MAX_DIAGNOSTIC_CONTEXTS: usize = 16_384;
const MAX_DIAGNOSTIC_CONTEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Runtime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    app: AppState,
    enabled: bool,
    next_workspace: AtomicU64,
    next_buffer: AtomicU64,
    next_stage: AtomicU64,
    next_server: AtomicU64,
    next_server_generation: AtomicU64,
    servers: StdMutex<ServerBindings>,
}

#[derive(Default)]
struct ServerBindings {
    by_ref: HashMap<u16, ServerBinding>,
    by_handle: HashMap<u64, u16>,
}

#[derive(Clone)]
struct ServerBinding {
    handle: u64,
    generation: u64,
    root: Vec<u8>,
    backend_id: String,
}

#[derive(Clone)]
pub(crate) struct Session {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    runtime: Runtime,
    workspaces: StdMutex<HashMap<u64, Arc<WorkspaceInner>>>,
    buffers: StdMutex<Buffers>,
    diagnostics: StdMutex<DiagnosticContexts>,
    stages: StdMutex<HashMap<u64, BufferStage>>,
    pending_queries: StdMutex<HashSet<u16>>,
    replay: StdMutex<ReplayCache>,
    next_nonce: AtomicU32,
    closed: AtomicBool,
}

struct WorkspaceInner {
    handle: u64,
    revision: AtomicU64,
    root: PathBuf,
    attachment: Arc<yas_lsp::Attachment>,
    receiver: StdMutex<Option<mpsc::Receiver<yas_lsp::native::Event>>>,
    closed: AtomicBool,
}

#[derive(Default)]
struct Buffers {
    by_handle: HashMap<u64, Buffer>,
    by_path: BTreeMap<(u64, fs_wire::Path), u64>,
}

#[derive(Clone)]
struct Buffer {
    workspace_handle: u64,
    handle: u64,
    revision: u64,
    path: fs_wire::Path,
    bytes: Arc<[u8]>,
    hash: [u8; 32],
}

struct BufferStage {
    workspace_handle: u64,
    expected_revision: u64,
    path: fs_wire::Path,
    byte_len: u64,
    content_hash: [u8; 32],
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct DiagnosticContext {
    workspace_handle: u64,
    path: fs_wire::Path,
    value: serde_json::Value,
    logical_bytes: usize,
}

#[derive(Default)]
struct DiagnosticContexts {
    values: HashMap<u64, DiagnosticContext>,
    order: VecDeque<u64>,
    logical_bytes: usize,
}

impl DiagnosticContexts {
    fn remove_workspace(&mut self, workspace_handle: u64) {
        let removed: HashSet<u64> = self
            .values
            .iter()
            .filter_map(|(id, context)| {
                (context.workspace_handle == workspace_handle).then_some(*id)
            })
            .collect();
        if removed.is_empty() {
            return;
        }
        for id in &removed {
            if let Some(context) = self.values.remove(id) {
                self.logical_bytes = self.logical_bytes.saturating_sub(context.logical_bytes);
            }
        }
        self.order.retain(|id| !removed.contains(id));
    }

    fn insert(&mut self, id: u64, context: DiagnosticContext) {
        if context.logical_bytes > MAX_DIAGNOSTIC_CONTEXT_BYTES {
            return;
        }
        if let Some(previous) = self.values.remove(&id) {
            self.logical_bytes = self.logical_bytes.saturating_sub(previous.logical_bytes);
            self.order.retain(|prior| *prior != id);
        }
        while self.values.len() >= MAX_DIAGNOSTIC_CONTEXTS
            || self.logical_bytes.saturating_add(context.logical_bytes)
                > MAX_DIAGNOSTIC_CONTEXT_BYTES
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(previous) = self.values.remove(&oldest) {
                self.logical_bytes = self.logical_bytes.saturating_sub(previous.logical_bytes);
            }
        }
        self.logical_bytes += context.logical_bytes;
        self.order.push_back(id);
        self.values.insert(id, context);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ReplayValue {
    Buffer(wire::BufferIdentity),
    Closed(wire::RemovedEntity),
    Stopped,
}

type ReplayEntry = ([u8; 32], Result<ReplayValue, Error>);

#[derive(Default)]
struct ReplayCache {
    values: HashMap<[u8; 16], ReplayEntry>,
    order: VecDeque<[u8; 16]>,
}

pub(crate) struct Watch {
    workspace: Arc<WorkspaceInner>,
    datasets: u16,
    receiver: Option<mpsc::Receiver<yas_lsp::native::Event>>,
    state: BTreeMap<u16, yas_lsp::native::Server>,
    diagnostics: BTreeMap<PathBuf, yas_lsp::native::FileDiagnostics>,
    pending_backend: AtomicU32,
    pending_diagnostics: AtomicU32,
    owner: Weak<SessionInner>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WatchStream {
    Backend,
    Diagnostics,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WatchEvent {
    Snapshot {
        stream: WatchStream,
        update_id: u32,
        entities: Vec<wire::StateEntity>,
    },
}

pub(crate) struct PendingQuery {
    nonce: u16,
    max_records: usize,
    cursor: Option<QueryCursor>,
    workspace: Arc<WorkspaceInner>,
    owner: Weak<SessionInner>,
    receiver: oneshot::Receiver<yas_lsp::native::QueryResponse>,
    completed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryData {
    pub(crate) query_status: u16,
    pub(crate) flags: u16,
    pub(crate) detail: String,
    pub(crate) total_hint: u64,
    pub(crate) next_cursor: Vec<u8>,
    pub(crate) records: Vec<wire::QueryRecord>,
}

type QueryArguments = (
    yas_lsp::native::QueryKind,
    u8,
    u32,
    u32,
    Option<PathBuf>,
    String,
);

#[derive(Clone, Copy)]
struct QueryCursor {
    offset: usize,
    fingerprint: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Error {
    pub(crate) status: Status,
    pub(crate) detail: String,
}

impl Error {
    fn new(status: Status, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }

    fn invalid(detail: impl Into<String>) -> Self {
        Self::new(Status::Invalid, detail)
    }

    fn conflict(detail: impl Into<String>) -> Self {
        Self::new(Status::Conflict, detail)
    }

    fn exhausted(detail: impl Into<String>) -> Self {
        Self::new(Status::ResourceExhausted, detail)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Runtime {
    pub(crate) fn new(app: AppState) -> Self {
        // Neither value contains the LSP engine's u16 server reference.
        // They only seed monotonically allocated, boot-scoped opaque IDs.
        let boot_seed = app.boot_generation.max(1);
        Self {
            inner: Arc::new(RuntimeInner {
                app,
                enabled: !std::env::var("YAS_LSP").is_ok_and(|value| value == "0"),
                next_workspace: AtomicU64::new(1),
                next_buffer: AtomicU64::new(1),
                next_stage: AtomicU64::new(1),
                next_server: AtomicU64::new(0x8000_0000_0000_0000 | boot_seed.rotate_left(17)),
                next_server_generation: AtomicU64::new(
                    0x4000_0000_0000_0000 | boot_seed.rotate_left(41),
                ),
                servers: StdMutex::new(ServerBindings::default()),
            }),
        }
    }

    pub(crate) fn runtime_state(&self) -> RuntimeState {
        if self.inner.enabled {
            RuntimeState::Available
        } else {
            RuntimeState::Unavailable
        }
    }

    pub(crate) fn limits(&self) -> wire::Limits {
        wire::Limits {
            // The LSP attachment owns one ordered state/diagnostic stream.
            // Advertising more would let a second WATCH succeed at HELLO but
            // fail only after the first consumer had claimed that stream.
            max_watches_per_workspace: 1,
            ..wire::Limits::HARD
        }
    }

    fn bind_server(
        &self,
        server_ref: u16,
        root: &[u8],
        backend_id: &str,
    ) -> Result<(u64, u64), Error> {
        if server_ref == 0 || backend_id.is_empty() {
            return Err(Error::new(Status::Internal, "invalid LSP backend identity"));
        }
        let mut servers = self.inner.servers.lock().unwrap();
        if let Some(binding) = servers.by_ref.get(&server_ref)
            && binding.root == root
            && binding.backend_id == backend_id
        {
            return Ok((binding.handle, binding.generation));
        }
        if let Some(previous) = servers.by_ref.remove(&server_ref) {
            servers.by_handle.remove(&previous.handle);
        }
        let handle = loop {
            let candidate = next_global_handle(&self.inner.next_server)?;
            if !servers.by_handle.contains_key(&candidate) {
                break candidate;
            }
        };
        let generation = next_global_handle(&self.inner.next_server_generation)?;
        servers.by_ref.insert(
            server_ref,
            ServerBinding {
                handle,
                generation,
                root: root.to_vec(),
                backend_id: backend_id.to_owned(),
            },
        );
        servers.by_handle.insert(handle, server_ref);
        Ok((handle, generation))
    }

    fn resolve_server(&self, handle: u64, generation: u64) -> Result<u16, Error> {
        let servers = self.inner.servers.lock().unwrap();
        let server_ref = servers
            .by_handle
            .get(&handle)
            .copied()
            .ok_or_else(|| Error::new(Status::NotFound, "unknown LSP server"))?;
        let binding = servers
            .by_ref
            .get(&server_ref)
            .ok_or_else(|| Error::new(Status::NotFound, "unknown LSP server"))?;
        if binding.generation != generation {
            return Err(Error::new(Status::Stale, "stale LSP server generation"));
        }
        Ok(server_ref)
    }

    fn forget_server(&self, handle: u64) {
        let mut servers = self.inner.servers.lock().unwrap();
        let Some(server_ref) = servers.by_handle.remove(&handle) else {
            return;
        };
        if servers
            .by_ref
            .get(&server_ref)
            .is_some_and(|binding| binding.handle == handle)
        {
            servers.by_ref.remove(&server_ref);
        }
    }

    fn retain_servers(&self, live_refs: &HashSet<u16>) {
        let mut servers = self.inner.servers.lock().unwrap();
        let retired = servers
            .by_ref
            .iter()
            .filter_map(|(server_ref, binding)| {
                (!live_refs.contains(server_ref)).then_some((*server_ref, binding.handle))
            })
            .collect::<Vec<_>>();
        for (server_ref, handle) in retired {
            servers.by_ref.remove(&server_ref);
            servers.by_handle.remove(&handle);
        }
    }

    pub(crate) fn session(&self, owner_session: [u8; 16]) -> Result<Session, Error> {
        if !self.inner.enabled {
            return Err(Error::new(Status::Unavailable, "LSP family is disabled"));
        }
        if owner_session == [0; 16] {
            return Err(Error::new(Status::Invalid, "zero LSP owner session"));
        }
        Ok(Session {
            inner: Arc::new(SessionInner {
                runtime: self.clone(),
                workspaces: StdMutex::new(HashMap::new()),
                buffers: StdMutex::new(Buffers::default()),
                diagnostics: StdMutex::new(DiagnosticContexts::default()),
                stages: StdMutex::new(HashMap::new()),
                pending_queries: StdMutex::new(HashSet::new()),
                replay: StdMutex::new(ReplayCache::default()),
                next_nonce: AtomicU32::new(1),
                closed: AtomicBool::new(false),
            }),
        })
    }
}

impl Session {
    pub(crate) async fn open(
        &self,
        request: &wire::Open,
        fs: Option<&yas_fs::Session>,
    ) -> Result<wire::OpenResult, Error> {
        self.ensure_open()?;
        if self.inner.workspaces.lock().unwrap().len()
            >= wire::Limits::HARD.max_workspaces_per_session as usize
        {
            return Err(Error::exhausted("LSP workspace limit reached"));
        }
        let source = self.resolve_source(&request.source, fs).await?;
        let open_mode = request.open_mode;
        let language = request.language.clone();
        let profile = request.profile.clone();
        let initialization_options = request.initialization_options.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            if open_mode == yas_wire::schema::lsp::OPEN_EXPLICIT as u8 {
                yas_lsp::prepare_explicit(&source, &language, &profile, &initialization_options)
            } else {
                yas_lsp::prepare_native(&source)
            }
        })
        .await
        .map_err(|_| Error::new(Status::Internal, "LSP prepare task failed"))?
        .map_err(|failure| Error::new(native_status(failure.status), failure.detail))?;
        let (prepared, missing) = prepared;
        let workspace_handle = next_global_handle(&self.inner.runtime.inner.next_workspace)?;
        let backend_count = u16::try_from(prepared.backend_count())
            .map_err(|_| Error::exhausted("LSP backend count exceeds protocol"))?;
        let capabilities = map_capabilities(prepared.native_capabilities());
        let extensions = if backend_count == 0 {
            Extensions(vec![
                wire::open_no_backend_detail_extension(if missing.is_empty() {
                    "no language server backend was discovered"
                } else {
                    &missing
                })
                .map_err(|_| Error::new(Status::Internal, "invalid LSP discovery detail"))?,
            ])
        } else {
            Extensions::default()
        };
        let root = prepared.root().to_path_buf();
        let canonical_root = os_path_bytes(&root);
        let (sender, receiver) = mpsc::channel(STREAM_QUEUE);
        let sink: yas_lsp::native::EventSink =
            Arc::new(move |event| sender.try_send(event).is_ok());
        // Keep both engine streams attached. Native WATCH still controls what
        // is exposed and charged to the peer; this avoids a lossy late-attach
        // transition after OPEN has already published its result.
        let attachment = Arc::new(prepared.attach_native(request.diagnostics_settle_ms, sink));
        self.inner.workspaces.lock().unwrap().insert(
            workspace_handle,
            Arc::new(WorkspaceInner {
                handle: workspace_handle,
                revision: AtomicU64::new(1),
                root,
                attachment,
                receiver: StdMutex::new(Some(receiver)),
                closed: AtomicBool::new(false),
            }),
        );
        Ok(wire::OpenResult {
            workspace_handle,
            workspace_revision: 1,
            position_encoding: yas_wire::schema::lsp::POSITION_UTF8 as u8,
            backend_count,
            capabilities,
            canonical_root,
            extensions,
        })
    }

    pub(crate) fn close(&self, workspace_handle: u64) -> Result<(), Error> {
        self.ensure_open()?;
        let workspace = self
            .inner
            .workspaces
            .lock()
            .unwrap()
            .remove(&workspace_handle)
            .ok_or_else(|| Error::new(Status::NotFound, "unknown LSP workspace"))?;
        workspace.closed.store(true, Ordering::Release);
        let mut buffers = self.inner.buffers.lock().unwrap();
        let handles = buffers
            .by_handle
            .values()
            .filter(|buffer| buffer.workspace_handle == workspace_handle)
            .map(|buffer| buffer.handle)
            .collect::<Vec<_>>();
        for handle in handles {
            if let Some(buffer) = buffers.by_handle.remove(&handle) {
                buffers
                    .by_path
                    .remove(&(buffer.workspace_handle, buffer.path));
            }
        }
        self.inner
            .diagnostics
            .lock()
            .unwrap()
            .remove_workspace(workspace_handle);
        Ok(())
    }

    pub(crate) fn watch(&self, request: &wire::Watch) -> Result<Watch, Error> {
        self.ensure_open()?;
        let workspace = self.workspace(request.workspace_handle)?;
        let receiver = workspace
            .receiver
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| Error::new(Status::Busy, "LSP workspace is already watched"))?;
        Ok(Watch {
            workspace,
            datasets: request.datasets,
            receiver: Some(receiver),
            state: BTreeMap::new(),
            diagnostics: BTreeMap::new(),
            pending_backend: AtomicU32::new(0),
            pending_diagnostics: AtomicU32::new(0),
            owner: Arc::downgrade(&self.inner),
        })
    }

    async fn resolve_source(
        &self,
        source: &wire::WorkspaceSource,
        fs: Option<&yas_fs::Session>,
    ) -> Result<PathBuf, Error> {
        match source {
            wire::WorkspaceSource::PlatformPath(path) => platform_path(path),
            wire::WorkspaceSource::Fs {
                root_handle,
                root_path,
            } => fs
                .ok_or_else(|| Error::new(Status::NotFound, "unknown FS session"))?
                .resolved_path(*root_handle, root_path)
                .map_err(|error| Error::new(Status::NotFound, error.to_string())),
            wire::WorkspaceSource::TerminalCwd {
                terminal_handle,
                suffix,
            } => {
                let session = self.inner.runtime.inner.app.session.lock().await;
                let pty_id = session
                    .terminal_backend(*terminal_handle)
                    .ok_or_else(|| Error::new(Status::NotFound, "unknown terminal"))?;
                let pty = session
                    .ptys
                    .get(&pty_id)
                    .ok_or_else(|| Error::new(Status::NotFound, "unknown terminal"))?;
                let cwd =
                    resolve_term_cwd(pty.osc7_cwd.as_deref(), || super::pty::pty_cwd(&pty.handle))
                        .ok_or_else(|| {
                            Error::new(Status::NotFound, "terminal has no working directory")
                        })?;
                let mut path = PathBuf::from(cwd);
                append_path(&mut path, suffix)?;
                Ok(path)
            }
        }
    }

    fn workspace(&self, handle: u64) -> Result<Arc<WorkspaceInner>, Error> {
        self.inner
            .workspaces
            .lock()
            .unwrap()
            .get(&handle)
            .cloned()
            .ok_or_else(|| Error::new(Status::NotFound, "unknown LSP workspace"))
    }

    fn ensure_open(&self) -> Result<(), Error> {
        if self.inner.closed.load(Ordering::Acquire) {
            Err(Error::new(Status::Unavailable, "LSP session is closed"))
        } else {
            Ok(())
        }
    }
}

impl Watch {
    pub(crate) async fn next(&mut self) -> Option<Result<WatchEvent, Error>> {
        while let Some(event) = self.receiver.as_mut()?.recv().await {
            match event {
                yas_lsp::native::Event::State { update_id, servers } => {
                    self.state = servers
                        .into_iter()
                        .map(|server| (server.server_ref, server))
                        .collect();
                    if self.datasets & yas_wire::schema::lsp::WATCH_BACKEND as u16 == 0 {
                        self.workspace
                            .attachment
                            .ack_native(yas_lsp::native::Stream::State, update_id);
                        continue;
                    }
                    let entities = self
                        .state
                        .iter()
                        .map(|(server_ref, server)| {
                            Ok(wire::StateEntity::Backend(server_record(
                                &self.owner,
                                &self.workspace,
                                update_id,
                                *server_ref,
                                server,
                            )?))
                        })
                        .collect::<Result<Vec<_>, Error>>();
                    self.pending_backend.store(update_id, Ordering::Release);
                    return Some(entities.map(|entities| WatchEvent::Snapshot {
                        stream: WatchStream::Backend,
                        update_id,
                        entities,
                    }));
                }
                yas_lsp::native::Event::Diagnostics {
                    update_id,
                    full,
                    files,
                } => {
                    if full {
                        self.diagnostics.clear();
                    }
                    for file in files {
                        if file.diagnostics.is_empty() {
                            self.diagnostics.remove(&file.path);
                        } else {
                            self.diagnostics.insert(file.path.clone(), file);
                        }
                    }
                    if self.datasets & yas_wire::schema::lsp::WATCH_DIAGNOSTICS as u16 == 0 {
                        self.workspace
                            .attachment
                            .ack_native(yas_lsp::native::Stream::Diagnostics, update_id);
                        continue;
                    }
                    let entities = diagnostics_entities(
                        &self.owner,
                        &self.workspace,
                        update_id,
                        &self.diagnostics,
                    );
                    self.pending_diagnostics.store(update_id, Ordering::Release);
                    return Some(entities.map(|entities| WatchEvent::Snapshot {
                        stream: WatchStream::Diagnostics,
                        update_id,
                        entities,
                    }));
                }
            }
        }
        self.workspace.closed.store(true, Ordering::Release);
        None
    }

    pub(crate) fn acknowledge(&self, stream: WatchStream, update_id: u32) {
        let accepted = match stream {
            WatchStream::Backend => self
                .pending_backend
                .compare_exchange(update_id, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            WatchStream::Diagnostics => self
                .pending_diagnostics
                .compare_exchange(update_id, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
        };
        if !accepted {
            return;
        }
        self.workspace.attachment.ack_native(
            match stream {
                WatchStream::Backend => yas_lsp::native::Stream::State,
                WatchStream::Diagnostics => yas_lsp::native::Stream::Diagnostics,
            },
            update_id,
        );
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let backend = self.pending_backend.swap(0, Ordering::AcqRel);
        if backend != 0 {
            self.workspace
                .attachment
                .ack_native(yas_lsp::native::Stream::State, backend);
        }
        let diagnostics = self.pending_diagnostics.swap(0, Ordering::AcqRel);
        if diagnostics != 0 {
            self.workspace
                .attachment
                .ack_native(yas_lsp::native::Stream::Diagnostics, diagnostics);
        }
        if self.workspace.closed.load(Ordering::Acquire) {
            return;
        }
        let Some(receiver) = self.receiver.take() else {
            return;
        };
        let mut slot = self.workspace.receiver.lock().unwrap();
        if slot.is_none() {
            *slot = Some(receiver);
        }
    }
}

impl Session {
    pub(crate) fn query(&self, request: &wire::Query) -> Result<PendingQuery, Error> {
        self.ensure_open()?;
        let cursor = decode_query_cursor(&request.cursor)?;
        let workspace = self.workspace(request.workspace_handle)?;
        if workspace.closed.load(Ordering::Acquire) {
            return Err(Error::new(Status::Unavailable, "LSP workspace is closed"));
        }
        let (kind, flags, line, column, path, argument) =
            self.query_arguments(&workspace, &request.body)?;
        let nonce = self.allocate_nonce()?;
        let (sender, receiver) = oneshot::channel();
        let sender = StdMutex::new(Some(sender));
        let sink: yas_lsp::native::QuerySink = Arc::new(move |response| {
            sender
                .lock()
                .unwrap()
                .take()
                .is_some_and(|sender| sender.send(response).is_ok())
        });
        workspace.attachment.query_native(
            yas_lsp::native::QueryRequest {
                nonce,
                kind,
                flags,
                line,
                column,
                path: path.as_deref(),
                argument: &argument,
            },
            sink,
        );
        Ok(PendingQuery {
            nonce,
            max_records: if request.max_records == 0 {
                wire::Limits::HARD.max_query_records as usize
            } else {
                usize::from(request.max_records)
            },
            cursor,
            workspace,
            owner: Arc::downgrade(&self.inner),
            receiver,
            completed: false,
        })
    }

    fn query_arguments(
        &self,
        workspace: &WorkspaceInner,
        body: &wire::QueryBody,
    ) -> Result<QueryArguments, Error> {
        use yas_lsp::native::QueryKind;
        let target = |target: &wire::DocumentTarget| {
            self.validate_target(workspace, target)?;
            resolve_relative(&workspace.root, &target.path).map(Some)
        };
        let value = match body {
            wire::QueryBody::Definition {
                target: item,
                position,
            } => (
                QueryKind::Definition,
                0,
                position.line,
                position.byte_column,
                target(item)?,
                String::new(),
            ),
            wire::QueryBody::References {
                target: item,
                position,
                flags,
            } => (
                QueryKind::References,
                u8::try_from(*flags).map_err(|_| Error::invalid("LSP reference flags"))?,
                position.line,
                position.byte_column,
                target(item)?,
                String::new(),
            ),
            wire::QueryBody::Hover {
                target: item,
                position,
            } => (
                QueryKind::Hover,
                0,
                position.line,
                position.byte_column,
                target(item)?,
                String::new(),
            ),
            wire::QueryBody::DocumentSymbols { target: item } => (
                QueryKind::DocumentSymbols,
                0,
                0,
                0,
                target(item)?,
                String::new(),
            ),
            wire::QueryBody::WorkspaceSymbols { query } => {
                (QueryKind::WorkspaceSymbols, 0, 0, 0, None, query.clone())
            }
            wire::QueryBody::Completion {
                target: item,
                position,
                trigger_kind,
                trigger,
            } => (
                QueryKind::Completion,
                *trigger_kind,
                position.line,
                position.byte_column,
                target(item)?,
                trigger.clone(),
            ),
            wire::QueryBody::CodeActions {
                target: item,
                range,
                diagnostic_ids,
            } => {
                let path = target(item)?;
                let diagnostics = self.inner.diagnostics.lock().unwrap();
                let mut selected = Vec::with_capacity(diagnostic_ids.len());
                for id in diagnostic_ids {
                    let diagnostic = diagnostics
                        .values
                        .get(id)
                        .ok_or_else(|| Error::new(Status::Stale, "LSP diagnostic ID is stale"))?;
                    if diagnostic.workspace_handle != workspace.handle
                        || diagnostic.path != item.path
                    {
                        return Err(Error::new(
                            Status::Stale,
                            "LSP diagnostic does not belong to the query document",
                        ));
                    }
                    selected.push(diagnostic.value.clone());
                }
                let argument = serde_json::json!({
                    "range": json_range(*range),
                    "diagnostics": selected,
                })
                .to_string();
                (
                    QueryKind::CodeActions,
                    0,
                    range.start.line,
                    range.start.byte_column,
                    path,
                    argument,
                )
            }
            wire::QueryBody::Formatting {
                target: item,
                range,
                tab_width,
                flags,
            } => {
                let path = target(item)?;
                let argument = serde_json::json!({
                    "range": range.map(json_range),
                    "options": {
                        "tabSize": tab_width,
                        "insertSpaces": flags & yas_wire::schema::lsp::FORMATTING_INSERT_SPACES as u16 != 0,
                        "trimTrailingWhitespace": flags & yas_wire::schema::lsp::FORMATTING_TRIM_TRAILING_WHITESPACE as u16 != 0,
                        "insertFinalNewline": flags & yas_wire::schema::lsp::FORMATTING_INSERT_FINAL_NEWLINE as u16 != 0,
                        "trimFinalNewlines": flags & yas_wire::schema::lsp::FORMATTING_TRIM_FINAL_NEWLINES as u16 != 0,
                    },
                })
                .to_string();
                (
                    QueryKind::Formatting,
                    0,
                    range.map_or(0, |range| range.start.line),
                    range.map_or(0, |range| range.start.byte_column),
                    path,
                    argument,
                )
            }
            wire::QueryBody::Rename {
                target: item,
                position,
                new_name,
            } => (
                QueryKind::Rename,
                0,
                position.line,
                position.byte_column,
                target(item)?,
                new_name.clone(),
            ),
            wire::QueryBody::SignatureHelp {
                target: item,
                position,
            } => (
                QueryKind::SignatureHelp,
                0,
                position.line,
                position.byte_column,
                target(item)?,
                String::new(),
            ),
        };
        Ok(value)
    }

    fn validate_target(
        &self,
        workspace: &WorkspaceInner,
        target: &wire::DocumentTarget,
    ) -> Result<(), Error> {
        let buffers = self.inner.buffers.lock().unwrap();
        let buffer = buffers
            .by_path
            .get(&(workspace.handle, target.path.clone()))
            .and_then(|handle| buffers.by_handle.get(handle));
        if target.document_revision != 0 {
            let buffer = buffer.ok_or_else(|| Error::conflict("LSP buffer is not open"))?;
            if buffer.revision != target.document_revision || buffer.hash != target.content_hash {
                return Err(Error::conflict("LSP buffer revision or hash is stale"));
            }
            return Ok(());
        }
        if target.content_hash == [0; 32] {
            return Ok(());
        }
        drop(buffers);
        let bytes = std::fs::read(resolve_relative(&workspace.root, &target.path)?)
            .map_err(|error| Error::new(Status::Io, error.to_string()))?;
        if *blake3::hash(&bytes).as_bytes() != target.content_hash {
            return Err(Error::conflict("LSP disk content hash is stale"));
        }
        Ok(())
    }

    fn allocate_nonce(&self) -> Result<u16, Error> {
        let mut pending = self.inner.pending_queries.lock().unwrap();
        if pending.len() >= wire::Limits::HARD.max_concurrent_queries as usize {
            return Err(Error::exhausted("LSP concurrent query limit reached"));
        }
        for _ in 0..u16::MAX {
            let candidate = self.inner.next_nonce.fetch_add(1, Ordering::Relaxed) as u16;
            if candidate != 0 && pending.insert(candidate) {
                return Ok(candidate);
            }
        }
        Err(Error::exhausted("LSP query ID space exhausted"))
    }

    pub(crate) fn buffer_put(
        &self,
        request: &wire::BufferPut,
    ) -> Result<wire::BufferIdentity, Error> {
        self.ensure_open()?;
        let fingerprint = fingerprint(request)?;
        if let Some(value) = self.replay_lookup(request.operation_id, fingerprint)? {
            return match value {
                ReplayValue::Buffer(identity) => Ok(identity),
                _ => Err(Error::new(Status::Internal, "LSP replay type mismatch")),
            };
        }
        let result = self.apply_buffer(
            request.workspace_handle,
            request.expected_revision,
            request.path.clone(),
            request.content.clone(),
        );
        self.replay_insert(
            request.operation_id,
            fingerprint,
            result.clone().map(ReplayValue::Buffer),
        );
        result
    }

    pub(crate) fn buffer_begin(&self, request: &wire::BufferBegin) -> Result<u64, Error> {
        self.ensure_open()?;
        self.workspace(request.workspace_handle)?;
        if self.inner.stages.lock().unwrap().len()
            >= wire::Limits::HARD.max_stages_per_session as usize
        {
            return Err(Error::exhausted("LSP buffer staging limit reached"));
        }
        self.check_buffer_precondition(
            request.workspace_handle,
            &request.path,
            request.expected_revision,
        )?;
        let staging_handle = next_global_handle(&self.inner.runtime.inner.next_stage)?;
        let capacity = usize::try_from(request.byte_len)
            .map_err(|_| Error::exhausted("LSP buffer is too large"))?
            .min(yas_wire::frame::HARD_MAX_BULK_CHUNK as usize);
        self.inner.stages.lock().unwrap().insert(
            staging_handle,
            BufferStage {
                workspace_handle: request.workspace_handle,
                expected_revision: request.expected_revision,
                path: request.path.clone(),
                byte_len: request.byte_len,
                content_hash: request.content_hash,
                bytes: Vec::with_capacity(capacity),
            },
        );
        Ok(staging_handle)
    }

    pub(crate) fn buffer_stage_write(
        &self,
        staging_handle: u64,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), Error> {
        self.ensure_open()?;
        let mut stages = self.inner.stages.lock().unwrap();
        let stage = stages
            .get_mut(&staging_handle)
            .ok_or_else(|| Error::new(Status::NotFound, "unknown LSP staging handle"))?;
        if offset != stage.bytes.len() as u64
            || offset.saturating_add(bytes.len() as u64) > stage.byte_len
        {
            return Err(Error::invalid("LSP staged buffer offset or length"));
        }
        stage.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub(crate) fn buffer_stage_reset(&self, staging_handle: u64) {
        self.inner.stages.lock().unwrap().remove(&staging_handle);
    }

    pub(crate) fn buffer_commit(
        &self,
        request: &wire::BufferCommit,
    ) -> Result<wire::BufferIdentity, Error> {
        self.ensure_open()?;
        let fingerprint = fingerprint(request)?;
        if let Some(value) = self.replay_lookup(request.operation_id, fingerprint)? {
            return match value {
                ReplayValue::Buffer(identity) => Ok(identity),
                _ => Err(Error::new(Status::Internal, "LSP replay type mismatch")),
            };
        }
        let result = (|| {
            let stage = self
                .inner
                .stages
                .lock()
                .unwrap()
                .remove(&request.staging_handle)
                .ok_or_else(|| Error::new(Status::NotFound, "unknown LSP staging handle"))?;
            if stage.bytes.len() as u64 != stage.byte_len
                || *blake3::hash(&stage.bytes).as_bytes() != stage.content_hash
            {
                return Err(Error::conflict("LSP staged buffer length or hash mismatch"));
            }
            self.apply_buffer(
                stage.workspace_handle,
                stage.expected_revision,
                stage.path,
                stage.bytes,
            )
        })();
        self.replay_insert(
            request.operation_id,
            fingerprint,
            result.clone().map(ReplayValue::Buffer),
        );
        result
    }

    pub(crate) fn buffer_close(
        &self,
        request: &wire::BufferClose,
    ) -> Result<wire::RemovedEntity, Error> {
        self.ensure_open()?;
        let fingerprint = fingerprint(request)?;
        if let Some(value) = self.replay_lookup(request.operation_id, fingerprint)? {
            return match value {
                ReplayValue::Closed(removed) => Ok(removed),
                _ => Err(Error::new(Status::Internal, "LSP replay type mismatch")),
            };
        }
        let result = (|| {
            let mut buffers = self.inner.buffers.lock().unwrap();
            let buffer = buffers
                .by_handle
                .get(&request.buffer_handle)
                .cloned()
                .ok_or_else(|| Error::new(Status::NotFound, "unknown LSP buffer"))?;
            if buffer.revision != request.expected_revision {
                return Err(Error::conflict("LSP buffer revision is stale"));
            }
            let workspace = self.workspace(buffer.workspace_handle)?;
            workspace
                .attachment
                .buffer(&resolve_relative(&workspace.root, &buffer.path)?, None);
            buffers.by_handle.remove(&buffer.handle);
            buffers
                .by_path
                .remove(&(buffer.workspace_handle, buffer.path));
            let removed_revision = workspace.revision.fetch_add(1, Ordering::AcqRel) + 1;
            Ok(wire::RemovedEntity {
                key: wire::RemovedEntityKey::Buffer {
                    buffer_handle: buffer.handle,
                },
                removed_revision,
            })
        })();
        self.replay_insert(
            request.operation_id,
            fingerprint,
            result.clone().map(ReplayValue::Closed),
        );
        result
    }

    pub(crate) fn buffer_record(&self, buffer_handle: u64) -> Result<wire::BufferRecord, Error> {
        let buffers = self.inner.buffers.lock().unwrap();
        let buffer = buffers
            .by_handle
            .get(&buffer_handle)
            .ok_or_else(|| Error::new(Status::NotFound, "unknown LSP buffer"))?;
        Ok(buffer_record(buffer))
    }

    pub(crate) fn buffer_snapshot(
        &self,
        workspace_handle: u64,
    ) -> Result<Vec<wire::StateEntity>, Error> {
        self.workspace(workspace_handle)?;
        let buffers = self.inner.buffers.lock().unwrap();
        let mut records = buffers
            .by_handle
            .values()
            .filter(|buffer| buffer.workspace_handle == workspace_handle)
            .map(|buffer| wire::StateEntity::Buffer(buffer_record(buffer)))
            .collect::<Vec<_>>();
        records.sort_by_key(|entity| match entity {
            wire::StateEntity::Buffer(buffer) => buffer.buffer_handle,
            _ => 0,
        });
        Ok(records)
    }

    pub(crate) fn list_servers(
        &self,
        request: &wire::ListServers,
    ) -> Result<wire::ServerList, Error> {
        self.ensure_open()?;
        let requested_workspace = (request.workspace_handle != 0)
            .then(|| self.workspace(request.workspace_handle))
            .transpose()?;
        let workspaces = self.inner.workspaces.lock().unwrap();
        let mut servers = Vec::new();
        let mut live_refs = HashSet::new();
        for server in yas_lsp::native_servers() {
            let root_path = server
                .root
                .as_ref()
                .ok_or_else(|| Error::new(Status::Internal, "LSP server omitted its root"))?;
            let root = os_path_bytes(root_path);
            let workspace_handle = if let Some(workspace) = requested_workspace.as_ref() {
                if os_path_bytes(&workspace.root) != root {
                    continue;
                }
                workspace.handle
            } else {
                workspaces
                    .values()
                    .filter(|workspace| os_path_bytes(&workspace.root) == root)
                    .map(|workspace| workspace.handle)
                    .min()
                    .unwrap_or(0)
            };
            live_refs.insert(server.server_ref);
            let (server_handle, generation) =
                self.inner
                    .runtime
                    .bind_server(server.server_ref, &root, &server.id)?;
            servers.push(server_record_parts(
                server_handle,
                generation,
                workspace_handle,
                server.epoch.max(1),
                server.phase,
                server.progress_pct,
                server.capabilities,
                server.epoch,
                server.refused_edits,
                server.rss_bytes,
                &server.id,
                &server.message,
            )?);
        }
        drop(workspaces);
        if requested_workspace.is_none() {
            self.inner.runtime.retain_servers(&live_refs);
        }
        Ok(wire::ServerList { servers })
    }

    pub(crate) fn stop_server(&self, request: &wire::StopServer) -> Result<(), Error> {
        self.ensure_open()?;
        let fingerprint = fingerprint(request)?;
        if let Some(value) = self.replay_lookup(request.operation_id, fingerprint)? {
            return match value {
                ReplayValue::Stopped => Ok(()),
                _ => Err(Error::new(Status::Internal, "LSP replay type mismatch")),
            };
        }
        let server_ref = self
            .inner
            .runtime
            .resolve_server(request.server_handle, request.generation)?;
        let result = if yas_lsp::stop_native(server_ref) {
            self.inner.runtime.forget_server(request.server_handle);
            Ok(())
        } else {
            self.inner.runtime.forget_server(request.server_handle);
            Err(Error::new(Status::NotFound, "failed to stop LSP server"))
        };
        self.replay_insert(
            request.operation_id,
            fingerprint,
            result.clone().map(|()| ReplayValue::Stopped),
        );
        result
    }

    fn apply_buffer(
        &self,
        workspace_handle: u64,
        expected_revision: u64,
        path: fs_wire::Path,
        bytes: Vec<u8>,
    ) -> Result<wire::BufferIdentity, Error> {
        if bytes.len() > wire::Limits::HARD.max_buffer_bytes as usize {
            return Err(Error::exhausted("LSP buffer is too large"));
        }
        std::str::from_utf8(&bytes)
            .map_err(|_| Error::invalid("LSP buffer content is not UTF-8"))?;
        let workspace = self.workspace(workspace_handle)?;
        let mut buffers = self.inner.buffers.lock().unwrap();
        let existing = buffers
            .by_path
            .get(&(workspace_handle, path.clone()))
            .copied();
        let current = existing.and_then(|handle| buffers.by_handle.get(&handle));
        match (expected_revision, current) {
            (0, None) => {}
            (0, Some(_)) => return Err(Error::conflict("LSP buffer already exists")),
            (expected, Some(buffer)) if buffer.revision == expected => {}
            _ => return Err(Error::conflict("LSP buffer revision is stale")),
        }
        if existing.is_none()
            && buffers
                .by_handle
                .values()
                .filter(|buffer| buffer.workspace_handle == workspace_handle)
                .count()
                >= wire::Limits::HARD.max_buffers_per_workspace as usize
        {
            return Err(Error::exhausted("LSP workspace buffer limit reached"));
        }
        let (buffer_handle, buffer_revision) = match existing {
            Some(handle) => {
                let revision = buffers.by_handle[&handle].revision.saturating_add(1);
                (handle, revision)
            }
            None => (
                next_global_handle(&self.inner.runtime.inner.next_buffer)?,
                1,
            ),
        };
        let hash = *blake3::hash(&bytes).as_bytes();
        workspace.attachment.buffer(
            &resolve_relative(&workspace.root, &path)?,
            Some(bytes.clone()),
        );
        let workspace_revision = workspace.revision.fetch_add(1, Ordering::AcqRel) + 1;
        let byte_len = bytes.len() as u64;
        buffers
            .by_path
            .insert((workspace_handle, path.clone()), buffer_handle);
        buffers.by_handle.insert(
            buffer_handle,
            Buffer {
                workspace_handle,
                handle: buffer_handle,
                revision: buffer_revision,
                path,
                bytes: bytes.into(),
                hash,
            },
        );
        Ok(wire::BufferIdentity {
            buffer_handle,
            buffer_revision,
            workspace_revision,
            byte_len,
            content_hash: hash,
            extensions: Extensions::default(),
        })
    }

    fn check_buffer_precondition(
        &self,
        workspace_handle: u64,
        path: &fs_wire::Path,
        expected_revision: u64,
    ) -> Result<(), Error> {
        let buffers = self.inner.buffers.lock().unwrap();
        let current = buffers
            .by_path
            .get(&(workspace_handle, path.clone()))
            .and_then(|handle| buffers.by_handle.get(handle));
        match (expected_revision, current) {
            (0, None) => Ok(()),
            (0, Some(_)) => Err(Error::conflict("LSP buffer already exists")),
            (expected, Some(buffer)) if buffer.revision == expected => Ok(()),
            _ => Err(Error::conflict("LSP buffer revision is stale")),
        }
    }

    fn replay_lookup(
        &self,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
    ) -> Result<Option<ReplayValue>, Error> {
        let replay = self.inner.replay.lock().unwrap();
        let Some((known, result)) = replay.values.get(&operation_id) else {
            return Ok(None);
        };
        if *known != fingerprint {
            return Err(Error::conflict(
                "LSP operation ID reused with different request",
            ));
        }
        result.clone().map(Some)
    }

    fn replay_insert(
        &self,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
        value: Result<ReplayValue, Error>,
    ) {
        let mut replay = self.inner.replay.lock().unwrap();
        if replay.values.contains_key(&operation_id) {
            return;
        }
        replay.order.push_back(operation_id);
        replay.values.insert(operation_id, (fingerprint, value));
        while replay.order.len() > MAX_REPLAYS {
            if let Some(evicted) = replay.order.pop_front() {
                replay.values.remove(&evicted);
            }
        }
    }
}

impl PendingQuery {
    pub(crate) async fn finish(mut self) -> Result<QueryData, Error> {
        let response = (&mut self.receiver)
            .await
            .map_err(|_| Error::new(Status::Unavailable, "LSP query channel closed"))?;
        self.completed = true;
        retire_query(&self.owner, self.nonce);
        convert_query_response(
            &self.owner,
            &self.workspace,
            response,
            self.max_records,
            self.cursor,
        )
    }
}

impl Drop for PendingQuery {
    fn drop(&mut self) {
        if !self.completed {
            self.workspace.attachment.cancel(self.nonce);
            retire_query(&self.owner, self.nonce);
        }
    }
}

fn retire_query(owner: &Weak<SessionInner>, nonce: u16) {
    if let Some(owner) = owner.upgrade() {
        owner.pending_queries.lock().unwrap().remove(&nonce);
    }
}

fn server_record(
    owner: &Weak<SessionInner>,
    workspace: &WorkspaceInner,
    update_id: u32,
    server_ref: u16,
    state: &yas_lsp::native::Server,
) -> Result<wire::ServerRecord, Error> {
    let owner = owner
        .upgrade()
        .ok_or_else(|| Error::new(Status::Unavailable, "LSP session closed"))?;
    let root = os_path_bytes(&workspace.root);
    let (server_handle, generation) = owner.runtime.bind_server(server_ref, &root, &state.id)?;
    server_record_parts(
        server_handle,
        generation,
        workspace.handle,
        update_id,
        state.phase,
        state.progress_pct,
        state.capabilities,
        state.epoch,
        state.refused_edits,
        state.rss_bytes,
        &state.id,
        &state.message,
    )
}

fn buffer_record(buffer: &Buffer) -> wire::BufferRecord {
    wire::BufferRecord {
        workspace_handle: buffer.workspace_handle,
        buffer_handle: buffer.handle,
        buffer_revision: buffer.revision,
        path: buffer.path.clone(),
        byte_len: buffer.bytes.len() as u64,
        content_hash: buffer.hash,
        extensions: Extensions::default(),
    }
}

#[allow(clippy::too_many_arguments)]
fn server_record_parts(
    server_handle: u64,
    generation: u64,
    workspace_handle: u64,
    revision: u32,
    phase: u8,
    progress_pct: u8,
    capabilities: yas_lsp::native::Capabilities,
    epoch: u32,
    refused_edits: u32,
    rss_bytes: u64,
    backend_id: &str,
    last_message: &str,
) -> Result<wire::ServerRecord, Error> {
    if backend_id.is_empty() {
        return Err(Error::new(Status::Internal, "empty LSP backend ID"));
    }
    Ok(wire::ServerRecord {
        server_handle,
        generation,
        server_revision: u64::from(revision.max(1)),
        workspace_handle,
        phase: map_server_phase(phase),
        progress_pct,
        epoch,
        refused_edits,
        rss_bytes,
        capabilities: map_capabilities(capabilities),
        // The discovery table exposes one stable backend identifier;
        // preserve it in all three required identity fields instead of
        // inventing client-visible process details.
        language: backend_id.to_owned(),
        profile: "auto".to_owned(),
        backend_id: backend_id.to_owned(),
        last_message: last_message.to_owned(),
        extensions: Extensions::default(),
    })
}

fn diagnostics_entities(
    owner: &Weak<SessionInner>,
    workspace: &WorkspaceInner,
    update_id: u32,
    mirror: &BTreeMap<PathBuf, yas_lsp::native::FileDiagnostics>,
) -> Result<Vec<wire::StateEntity>, Error> {
    let mut entities = Vec::with_capacity(mirror.len());
    let mut contexts = HashMap::new();
    for (native_path, file) in mirror {
        let path = relative_wire_path(&workspace.root, native_path)?;
        let (document_revision, content_hash) =
            exact_content_identity(owner, workspace, &path, &file.hash)?;
        let mut diagnostics = Vec::with_capacity(file.diagnostics.len());
        for (index, diagnostic) in file.diagnostics.iter().enumerate() {
            let id = diagnostic_id(&path, update_id, index, diagnostic);
            let mut tags = Vec::new();
            let mut native_tags = 0;
            if diagnostic.unnecessary {
                tags.push(serde_json::Value::from(1));
                native_tags |= yas_wire::schema::lsp::DIAGNOSTIC_UNNECESSARY as u16;
            }
            if diagnostic.deprecated {
                tags.push(serde_json::Value::from(2));
                native_tags |= yas_wire::schema::lsp::DIAGNOSTIC_DEPRECATED as u16;
            }
            let logical_bytes = 96usize
                .saturating_add(path.components.iter().map(Vec::len).sum::<usize>())
                .saturating_add(diagnostic.code.len())
                .saturating_add(diagnostic.source.len())
                .saturating_add(diagnostic.message.len());
            contexts.insert(
                id,
                DiagnosticContext {
                    workspace_handle: workspace.handle,
                    path: path.clone(),
                    value: serde_json::json!({
                        "range": {
                            "start": { "line": diagnostic.line, "character": diagnostic.column },
                            "end": { "line": diagnostic.end_line, "character": diagnostic.end_column },
                        },
                        "severity": diagnostic.severity,
                        "code": diagnostic.code,
                        "source": diagnostic.source,
                        "message": diagnostic.message,
                        "tags": tags,
                    }),
                    logical_bytes,
                },
            );
            diagnostics.push(wire::Diagnostic {
                diagnostic_id: id,
                severity: diagnostic
                    .severity
                    .saturating_sub(1)
                    .min(yas_wire::schema::lsp::DIAGNOSTIC_HINT as u8),
                tags: native_tags,
                range: text_range(
                    diagnostic.line,
                    diagnostic.column,
                    diagnostic.end_line,
                    diagnostic.end_column,
                ),
                code: diagnostic.code.clone(),
                source: diagnostic.source.clone(),
                message: diagnostic.message.clone(),
            });
        }
        entities.push(wire::StateEntity::Diagnostics(wire::DiagnosticRecord {
            path,
            document_revision,
            content_hash,
            diagnostics_revision: u64::from(update_id.max(1)),
            diagnostics,
            extensions: Extensions::default(),
        }));
    }
    if let Some(owner) = owner.upgrade() {
        let mut stored = owner.diagnostics.lock().unwrap();
        stored.remove_workspace(workspace.handle);
        for (id, context) in contexts {
            stored.insert(id, context);
        }
    }
    Ok(entities)
}

fn diagnostic_id(
    path: &fs_wire::Path,
    update_id: u32,
    index: usize,
    diagnostic: &yas_lsp::native::Diagnostic,
) -> u64 {
    let mut hasher = blake3::Hasher::new();
    for component in &path.components {
        hasher.update(&(component.len() as u32).to_le_bytes());
        hasher.update(component);
    }
    hasher.update(&update_id.to_le_bytes());
    hasher.update(&(index as u64).to_le_bytes());
    hasher.update(diagnostic.message.as_bytes());
    let bytes = hasher.finalize();
    u64::from_le_bytes(bytes.as_bytes()[..8].try_into().unwrap()).max(1)
}

fn exact_content_identity(
    owner: &Weak<SessionInner>,
    workspace: &WorkspaceInner,
    path: &fs_wire::Path,
    backend_hash: &[u8; 16],
) -> Result<(u64, [u8; 32]), Error> {
    if let Some(owner) = owner.upgrade() {
        let buffers = owner.buffers.lock().unwrap();
        if let Some(buffer) = buffers
            .by_path
            .get(&(workspace.handle, path.clone()))
            .and_then(|handle| buffers.by_handle.get(handle))
            && (backend_hash == &[0; 16] || buffer.hash[..16] == backend_hash[..])
        {
            return Ok((buffer.revision, buffer.hash));
        }
    }
    let bytes = match std::fs::read(resolve_relative(&workspace.root, path)?) {
        Ok(bytes) => bytes,
        Err(_) => return Ok((0, [0; 32])),
    };
    let hash = *blake3::hash(&bytes).as_bytes();
    if backend_hash != &[0; 16] && hash[..16] != backend_hash[..] {
        // The file changed after the LSP backend took its snapshot. Never
        // mislabel an old result with the new disk hash.
        return Ok((0, [0; 32]));
    }
    Ok((0, hash))
}

fn convert_query_response(
    owner: &Weak<SessionInner>,
    workspace: &WorkspaceInner,
    response: yas_lsp::native::QueryResponse,
    max_records: usize,
    cursor: Option<QueryCursor>,
) -> Result<QueryData, Error> {
    use yas_lsp::native::QueryRecord as Native;
    let query_status = native_status(response.status).code();
    let mut flags = 0;
    if response.truncated || response.incomplete || response.status != yas_lsp::native::Status::Ok {
        flags |= yas_wire::schema::lsp::PAGE_INCOMPLETE as u16;
    }
    let detail = if response.status == yas_lsp::native::Status::Ok {
        String::new()
    } else if response.detail.is_empty() {
        format!("{:?}", response.status)
    } else {
        response.detail
    };
    let mut records = Vec::new();
    let mut pending_hover: Option<wire::LocationRecord> = None;
    let mut pending_action: Option<(wire::ActionRecord, u16)> = None;
    for record in response.records {
        if pending_action.is_some() && !matches!(&record, Native::Edit { .. }) {
            return Err(Error::new(
                Status::Internal,
                "LSP action ended before its declared edits",
            ));
        }
        match record {
            Native::Location {
                declaration,
                hash,
                line,
                column,
                end_line,
                end_column,
                path,
            } => {
                if let Some(location) = pending_hover.take() {
                    records.push(wire::QueryRecord::Location(location));
                }
                let path = relative_wire_path(&workspace.root, &path)?;
                let (document_revision, content_hash) =
                    exact_content_identity(owner, workspace, &path, &hash)?;
                pending_hover = Some(wire::LocationRecord {
                    path,
                    document_revision,
                    content_hash,
                    range: text_range(line, column, end_line, end_column),
                    flags: if declaration {
                        yas_wire::schema::lsp::LOCATION_DECLARATION as u16
                    } else {
                        0
                    },
                });
            }
            Native::Markup { markdown, text } => {
                let target = pending_hover.take().ok_or_else(|| {
                    Error::new(Status::Internal, "LSP markup has no target location")
                })?;
                records.push(wire::QueryRecord::Hover(wire::HoverRecord {
                    target,
                    markup_kind: if markdown {
                        yas_wire::schema::lsp::MARKUP_MARKDOWN as u8
                    } else {
                        yas_wire::schema::lsp::MARKUP_PLAIN_TEXT as u8
                    },
                    content: text.into_bytes(),
                }));
            }
            Native::Symbol {
                symbol_kind,
                deprecated,
                depth,
                line,
                column,
                end_line,
                end_column,
                name,
                path,
            } => {
                if let Some(location) = pending_hover.take() {
                    records.push(wire::QueryRecord::Location(location));
                }
                let path = path
                    .as_deref()
                    .map(|path| relative_wire_path(&workspace.root, path))
                    .transpose()?;
                let content_hash = path
                    .as_ref()
                    .map(|path| exact_content_identity(owner, workspace, path, &[0; 16]))
                    .transpose()?
                    .map(|(_, hash)| hash);
                let range = text_range(line, column, end_line, end_column);
                records.push(wire::QueryRecord::Symbol(wire::SymbolRecord {
                    symbol_kind: u16::from(symbol_kind.saturating_sub(1)),
                    flags: if deprecated {
                        yas_wire::schema::lsp::SYMBOL_DEPRECATED as u16
                    } else {
                        0
                    },
                    depth: u16::from(depth),
                    name,
                    detail: String::new(),
                    path,
                    content_hash,
                    range,
                    selection_range: range,
                }));
            }
            Native::Edit {
                hash,
                line,
                column,
                end_line,
                end_column,
                new_text,
                path,
            } => {
                if let Some(location) = pending_hover.take() {
                    records.push(wire::QueryRecord::Location(location));
                }
                let path = relative_wire_path(&workspace.root, &path)?;
                let (expected_revision, expected_content_hash) =
                    exact_content_identity(owner, workspace, &path, &hash)?;
                let edit = wire::EditRecord {
                    path,
                    expected_revision,
                    expected_content_hash,
                    range: text_range(line, column, end_line, end_column),
                    replacement: new_text.into_bytes(),
                };
                if let Some((action, remaining)) = pending_action.as_mut() {
                    action.edits.push(edit);
                    *remaining -= 1;
                    if *remaining == 0 {
                        let (action, _) = pending_action.take().unwrap();
                        records.push(wire::QueryRecord::Action(action));
                    }
                } else {
                    records.push(wire::QueryRecord::Edit(edit));
                }
            }
            Native::Completion {
                item_kind,
                deprecated,
                preselect,
                snippet,
                line,
                column,
                end_line,
                end_column,
                label,
                insert,
                detail,
            } => {
                if let Some(location) = pending_hover.take() {
                    records.push(wire::QueryRecord::Location(location));
                }
                let replacement_range =
                    (line != 0 || column != 0 || end_line != 0 || end_column != 0)
                        .then(|| text_range(line, column, end_line, end_column));
                records.push(wire::QueryRecord::Completion(wire::CompletionRecord {
                    item_kind: u16::from(item_kind.saturating_sub(1)),
                    flags: map_completion_flags(deprecated, preselect, snippet),
                    label,
                    detail,
                    filter_text: String::new(),
                    insert_text: insert.into_bytes(),
                    replacement_range,
                }));
            }
            Native::Signature {
                active,
                active_parameter,
                parameter_start,
                parameter_end,
                label,
                documentation,
            } => {
                if let Some(location) = pending_hover.take() {
                    records.push(wire::QueryRecord::Location(location));
                }
                records.push(wire::QueryRecord::Signature(wire::SignatureRecord {
                    flags: if active {
                        yas_wire::schema::lsp::SIGNATURE_ACTIVE as u16
                    } else {
                        0
                    },
                    active_parameter: active_parameter
                        .unwrap_or(yas_wire::schema::lsp::SIGNATURE_NO_ACTIVE_PARAMETER as u16),
                    parameter_start: u32::from(parameter_start),
                    parameter_end: u32::from(parameter_end),
                    label,
                    documentation,
                }));
            }
            Native::Action {
                preferred,
                disabled,
                edit_count,
                title,
                action_kind,
                disabled_reason,
            } => {
                if let Some(location) = pending_hover.take() {
                    records.push(wire::QueryRecord::Location(location));
                }
                let mut mapped_flags = 0;
                if preferred {
                    mapped_flags |= yas_wire::schema::lsp::ACTION_PREFERRED as u16;
                }
                if disabled {
                    mapped_flags |= yas_wire::schema::lsp::ACTION_DISABLED as u16;
                }
                let action = wire::ActionRecord {
                    title,
                    kind: action_kind,
                    flags: mapped_flags,
                    edits: Vec::with_capacity(usize::from(edit_count)),
                    disabled_reason,
                };
                if edit_count == 0 {
                    records.push(wire::QueryRecord::Action(action));
                } else {
                    pending_action = Some((action, edit_count));
                }
            }
        }
    }
    if pending_action.is_some() {
        return Err(Error::new(
            Status::Internal,
            "LSP action response is missing edits",
        ));
    }
    if let Some(location) = pending_hover {
        records.push(wire::QueryRecord::Location(location));
    }
    let total_hint = records.len() as u64;
    let fingerprint = query_records_fingerprint(query_status, flags, &detail, &records)?;
    let offset = match cursor {
        Some(cursor) if cursor.fingerprint == fingerprint && cursor.offset < records.len() => {
            cursor.offset
        }
        Some(_) => {
            return Ok(QueryData {
                query_status: Status::Stale.code(),
                flags: yas_wire::schema::lsp::PAGE_INCOMPLETE as u16,
                detail: "LSP query continuation is stale".to_owned(),
                total_hint,
                next_cursor: Vec::new(),
                records: Vec::new(),
            });
        }
        None => 0,
    };
    let end = offset.saturating_add(max_records).min(records.len());
    let next_cursor = if end < records.len() {
        flags |= yas_wire::schema::lsp::PAGE_TRUNCATED as u16;
        encode_query_cursor(end, fingerprint)
    } else {
        Vec::new()
    };
    let records = records
        .into_iter()
        .skip(offset)
        .take(end - offset)
        .collect();
    Ok(QueryData {
        query_status,
        flags,
        detail,
        total_hint,
        next_cursor,
        records,
    })
}

const QUERY_CURSOR_MAGIC: &[u8; 4] = b"YLQ1";

fn decode_query_cursor(bytes: &[u8]) -> Result<Option<QueryCursor>, Error> {
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() != 44 || &bytes[..4] != QUERY_CURSOR_MAGIC {
        return Err(Error::invalid("invalid LSP query continuation cursor"));
    }
    let offset = u64::from_le_bytes(bytes[4..12].try_into().unwrap());
    let offset = usize::try_from(offset)
        .map_err(|_| Error::invalid("LSP query cursor offset is too large"))?;
    if offset == 0 {
        return Err(Error::invalid("zero LSP query cursor offset"));
    }
    Ok(Some(QueryCursor {
        offset,
        fingerprint: bytes[12..44].try_into().unwrap(),
    }))
}

fn encode_query_cursor(offset: usize, fingerprint: [u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(44);
    out.extend_from_slice(QUERY_CURSOR_MAGIC);
    out.extend_from_slice(&(offset as u64).to_le_bytes());
    out.extend_from_slice(&fingerprint);
    out
}

fn query_records_fingerprint(
    query_status: u16,
    flags: u16,
    detail: &str,
    records: &[wire::QueryRecord],
) -> Result<[u8; 32], Error> {
    let mut hash = blake3::Hasher::new();
    hash.update(&query_status.to_le_bytes());
    hash.update(&flags.to_le_bytes());
    hash.update(&(detail.len() as u64).to_le_bytes());
    hash.update(detail.as_bytes());
    for record in records {
        let encoded = record
            .encode_typed()
            .and_then(|record| record.encode_message())
            .map_err(|_| Error::new(Status::Internal, "failed to encode LSP query cursor"))?;
        hash.update(&(encoded.len() as u64).to_le_bytes());
        hash.update(&encoded);
    }
    Ok(*hash.finalize().as_bytes())
}

fn text_range(line: u32, column: u32, end_line: u32, end_column: u32) -> wire::TextRange {
    wire::TextRange {
        start: wire::Position {
            line,
            byte_column: column,
        },
        end: wire::Position {
            line: end_line,
            byte_column: end_column,
        },
    }
}

fn json_range(range: wire::TextRange) -> serde_json::Value {
    serde_json::json!({
        "start": {
            "line": range.start.line,
            "character": range.start.byte_column,
        },
        "end": {
            "line": range.end.line,
            "character": range.end.byte_column,
        },
    })
}

fn map_capabilities(value: yas_lsp::native::Capabilities) -> u64 {
    use yas_wire::schema::lsp as native;
    let mut mapped = 0;
    if value.definition() {
        mapped |= native::CAPABILITY_DEFINITION;
    }
    if value.references() {
        mapped |= native::CAPABILITY_REFERENCES;
    }
    if value.hover() {
        mapped |= native::CAPABILITY_HOVER;
    }
    if value.document_symbols() {
        mapped |= native::CAPABILITY_DOCUMENT_SYMBOLS;
    }
    if value.workspace_symbols() {
        mapped |= native::CAPABILITY_WORKSPACE_SYMBOLS;
    }
    if value.rename() {
        mapped |= native::CAPABILITY_RENAME;
    }
    if value.completion() {
        mapped |= native::CAPABILITY_COMPLETION;
    }
    if value.signature_help() {
        mapped |= native::CAPABILITY_SIGNATURE_HELP;
    }
    if value.code_actions() {
        mapped |= native::CAPABILITY_CODE_ACTIONS;
    }
    if value.formatting() {
        mapped |= native::CAPABILITY_FORMATTING;
    }
    mapped
}

fn map_completion_flags(deprecated: bool, preselect: bool, snippet: bool) -> u16 {
    use yas_wire::schema::lsp as native;
    let mut mapped = 0;
    if deprecated {
        mapped |= native::COMPLETION_DEPRECATED as u16;
    }
    if preselect {
        mapped |= native::COMPLETION_PRESELECT as u16;
    }
    if snippet {
        mapped |= native::COMPLETION_SNIPPET_TEXT as u16;
    }
    mapped
}

fn map_server_phase(value: u8) -> u8 {
    value.min(yas_wire::schema::lsp::SERVER_FAILED as u8)
}

fn native_status(value: yas_lsp::native::Status) -> Status {
    match value {
        yas_lsp::native::Status::Ok => Status::Ok,
        yas_lsp::native::Status::NotFound => Status::NotFound,
        yas_lsp::native::Status::Unsupported => Status::Unsupported,
        yas_lsp::native::Status::Permission => Status::Unavailable,
        yas_lsp::native::Status::ResourceExhausted => Status::ResourceExhausted,
        yas_lsp::native::Status::Invalid => Status::Invalid,
        yas_lsp::native::Status::Cancelled => Status::Cancelled,
        yas_lsp::native::Status::Warming => Status::Busy,
        yas_lsp::native::Status::Other => Status::Internal,
    }
}

fn fingerprint(value: &impl Encode) -> Result<[u8; 32], Error> {
    value
        .encode()
        .map(|bytes| *blake3::hash(&bytes).as_bytes())
        .map_err(|_| Error::invalid("invalid LSP operation"))
}

fn next_global_handle(counter: &AtomicU64) -> Result<u64, Error> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| Error::exhausted("LSP boot handle space exhausted"))
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        for workspace in self.workspaces.lock().unwrap().values() {
            workspace.closed.store(true, Ordering::Release);
        }
        self.workspaces.lock().unwrap().clear();
        self.buffers.lock().unwrap().by_handle.clear();
        self.buffers.lock().unwrap().by_path.clear();
        self.stages.lock().unwrap().clear();
    }
}

fn relative_wire_path(root: &Path, absolute: &Path) -> Result<fs_wire::Path, Error> {
    if !absolute.is_absolute() {
        return Err(Error::new(
            Status::Internal,
            "relative native LSP result path",
        ));
    }
    let relative = absolute.strip_prefix(root).map_err(|_| {
        Error::new(
            Status::Unsupported,
            "native LSP result is outside the workspace",
        )
    })?;
    let mut components = Vec::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(Error::new(
                Status::Internal,
                "invalid native LSP result path",
            ));
        };
        let bytes = os_component_bytes(component);
        if bytes.is_empty() || bytes.contains(&b'/') || bytes.contains(&0) {
            return Err(Error::new(
                Status::Internal,
                "invalid native LSP path component",
            ));
        }
        components.push(bytes);
    }
    if components.is_empty() {
        return Err(Error::new(Status::Internal, "empty native LSP result path"));
    }
    Ok(fs_wire::Path { components })
}

fn resolve_relative(root: &Path, path: &fs_wire::Path) -> Result<PathBuf, Error> {
    let mut resolved = root.to_path_buf();
    append_path(&mut resolved, path)?;
    Ok(resolved)
}

fn append_path(path: &mut PathBuf, relative: &fs_wire::Path) -> Result<(), Error> {
    for component in &relative.components {
        let value = component_os(component);
        let mut parts = Path::new(&value).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(part)), None) if part == value.as_os_str() => path.push(part),
            _ => return Err(Error::invalid("invalid LSP path component")),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn platform_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    use std::os::unix::ffi::OsStringExt;
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(Error::invalid("invalid LSP platform path"));
    }
    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(unix))]
fn platform_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| Error::invalid("invalid UTF-8 LSP platform path"))?;
    Ok(PathBuf::from(value))
}

#[cfg(unix)]
fn component_os(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}

#[cfg(unix)]
fn os_component_bytes(component: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    component.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_component_bytes(component: &std::ffi::OsStr) -> Vec<u8> {
    component.to_string_lossy().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn component_os(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}

#[cfg(unix)]
fn os_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostic_context(workspace_handle: u64, logical_bytes: usize) -> DiagnosticContext {
        DiagnosticContext {
            workspace_handle,
            path: fs_wire::Path {
                components: vec![b"a.rs".to_vec()],
            },
            value: serde_json::Value::Null,
            logical_bytes,
        }
    }

    #[test]
    fn diagnostic_action_contexts_have_count_byte_and_lifecycle_bounds() {
        let mut contexts = DiagnosticContexts::default();
        for id in 1..=(MAX_DIAGNOSTIC_CONTEXTS as u64 + 1) {
            contexts.insert(id, diagnostic_context(1, 1));
        }
        assert_eq!(contexts.values.len(), MAX_DIAGNOSTIC_CONTEXTS);
        assert!(!contexts.values.contains_key(&1));
        assert!(
            contexts
                .values
                .contains_key(&(MAX_DIAGNOSTIC_CONTEXTS as u64 + 1))
        );

        contexts = DiagnosticContexts::default();
        let large = MAX_DIAGNOSTIC_CONTEXT_BYTES / 2 + 1;
        contexts.insert(1, diagnostic_context(1, large));
        contexts.insert(2, diagnostic_context(2, large));
        assert_eq!(contexts.values.len(), 1);
        assert!(!contexts.values.contains_key(&1));
        assert!(contexts.values.contains_key(&2));
        assert!(contexts.logical_bytes <= MAX_DIAGNOSTIC_CONTEXT_BYTES);

        contexts.insert(3, diagnostic_context(3, MAX_DIAGNOSTIC_CONTEXT_BYTES + 1));
        assert!(!contexts.values.contains_key(&3));
        contexts.remove_workspace(2);
        assert!(contexts.values.is_empty());
        assert_eq!(contexts.logical_bytes, 0);
        assert!(contexts.order.is_empty());
    }

    #[test]
    fn wire_paths_resolve_and_native_results_stay_in_the_workspace() {
        let path = fs_wire::Path {
            components: vec![b"src".to_vec(), b"main.rs".to_vec()],
        };
        let root = std::env::current_dir().unwrap().join("workspace");
        let resolved = resolve_relative(&root, &path).unwrap();
        assert_eq!(resolved, root.join("src").join("main.rs"));
        assert_eq!(relative_wire_path(&root, &resolved).unwrap(), path);
        let outside = root.parent().unwrap().join("outside").join("main.rs");
        assert!(relative_wire_path(&root, &outside).is_err());

        let mut appended = root.clone();
        append_path(
            &mut appended,
            &fs_wire::Path {
                components: vec![b"..".to_vec()],
            },
        )
        .unwrap_err();
        assert_eq!(appended, root);
    }

    #[cfg(unix)]
    #[test]
    fn platform_paths_preserve_non_utf8_bytes() {
        use std::os::unix::ffi::OsStrExt;

        let raw = b"/tmp/lsp-\xff";
        let path = platform_path(raw).unwrap();
        assert_eq!(path.as_os_str().as_bytes(), raw);
        assert_eq!(os_path_bytes(&path), raw);
    }

    #[test]
    fn completion_flags_are_mapped_semantically() {
        assert_eq!(
            map_completion_flags(true, false, true),
            (yas_wire::schema::lsp::COMPLETION_DEPRECATED
                | yas_wire::schema::lsp::COMPLETION_SNIPPET_TEXT) as u16
        );
    }

    #[cfg(unix)]
    #[test]
    fn server_handles_are_opaque_boot_scoped_and_generation_checked() {
        let app =
            crate::tests::process_transport::test_state(crate::process::Server::new(false, true));
        let runtime = Runtime::new(app);

        let (handle, generation) = runtime
            .bind_server(7, b"/workspace", "rust-analyzer")
            .unwrap();
        assert_ne!(handle, 7);
        assert_ne!(generation, 1);
        assert_eq!(runtime.resolve_server(handle, generation).unwrap(), 7);
        assert_eq!(
            runtime
                .resolve_server(handle, generation.wrapping_add(1))
                .unwrap_err()
                .status,
            Status::Stale
        );
        assert_eq!(
            runtime
                .bind_server(7, b"/workspace", "rust-analyzer")
                .unwrap(),
            (handle, generation)
        );

        let (replacement_handle, replacement_generation) =
            runtime.bind_server(7, b"/other", "rust-analyzer").unwrap();
        assert_ne!(replacement_handle, handle);
        assert_ne!(replacement_generation, generation);
        assert_eq!(
            runtime
                .resolve_server(handle, generation)
                .unwrap_err()
                .status,
            Status::NotFound
        );
        assert_eq!(
            runtime
                .resolve_server(replacement_handle, replacement_generation)
                .unwrap(),
            7
        );

        runtime.forget_server(replacement_handle);
        assert_eq!(
            runtime
                .resolve_server(replacement_handle, replacement_generation)
                .unwrap_err()
                .status,
            Status::NotFound
        );
    }

    #[test]
    fn query_continuation_cursor_is_opaque_exact_and_tamper_evident() {
        let records = vec![wire::QueryRecord::Completion(wire::CompletionRecord {
            item_kind: 1,
            flags: 0,
            label: "item".to_owned(),
            detail: String::new(),
            filter_text: String::new(),
            insert_text: b"item".to_vec(),
            replacement_range: None,
        })];
        let fingerprint = query_records_fingerprint(Status::Ok.code(), 0, "", &records).unwrap();
        let cursor = encode_query_cursor(1, fingerprint);
        let decoded = decode_query_cursor(&cursor).unwrap().unwrap();
        assert_eq!(decoded.offset, 1);
        assert_eq!(decoded.fingerprint, fingerprint);
        let mut malformed = cursor;
        malformed[0] ^= 1;
        assert!(decode_query_cursor(&malformed).is_err());
    }
}
