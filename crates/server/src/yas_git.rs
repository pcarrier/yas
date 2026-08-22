//! Native YAS Git semantics.
//!
//! This module deliberately stops at owned, typed family values.  The YAS
//! session dispatcher owns request ids, State subscriptions, Transfer credit,
//! and frame emission. Git work remains on the semantic `yas-git` engine.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use yas_wire::codec::Extensions;
use yas_wire::core::{RuntimeState, Status};
use yas_wire::fs as fs_wire;
use yas_wire::git as wire;

use super::{AppState, resolve_term_cwd, yas_fs};

#[derive(Clone)]
pub(crate) struct Runtime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    app: AppState,
    enabled: bool,
    /// Boot-owned opaque handle allocator. Handles are never derived from
    /// backend identifiers and are never reused during this server lifetime.
    next_repository: AtomicU64,
}

#[derive(Clone)]
pub(crate) struct Session {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    runtime: Runtime,
    repositories: Mutex<HashMap<u64, Repository>>,
    closed: AtomicBool,
}

struct Repository {
    handle: yas_git::RepoHandle,
    object_algorithm: u8,
    revision: AtomicU64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryData {
    pub(crate) records: Vec<QueryItem>,
    pub(crate) next_cursor: wire::QueryCursor,
    pub(crate) total_hint: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum QueryItem {
    Record(wire::QueryRecord),
    /// Content whose inline-vs-Transfer representation is selected by the
    /// session dispatcher after it reserves aggregate peer credit.
    Content(OwnedContent),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnedContent {
    pub(crate) kind: ContentKind,
    pub(crate) object: wire::ObjectId,
    pub(crate) byte_len: u64,
    pub(crate) offset: u64,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContentKind {
    Blob,
    Patch,
}

pub(crate) struct Watch {
    repository_handle: u64,
    repository_revision: u64,
    datasets: u16,
    object_algorithm: u8,
    handle: yas_git::StateHandle,
    receiver: tokio::sync::mpsc::Receiver<yas_git::native::StateEvent>,
}

/// One live, ref-dependent Git query backed by the repository state
/// engine.  The outer YAS session owns the common State subscription and
/// credit; this object only owns Git's semantic reevaluation stream.
pub(crate) struct QueryWatch {
    object_algorithm: u8,
    repository: yas_git::RepoHandle,
    request: wire::Query,
    discover_path: Option<PathBuf>,
    handle: yas_git::StateHandle,
    receiver: tokio::sync::mpsc::Receiver<yas_git::native::StateEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryWatchUpdate {
    pub(crate) update_id: u32,
    /// Non-OK reevaluations are recoverable values. The outer session wraps
    /// either branch in `WatchedQueryValue` and still waits for its ACK.
    pub(crate) result: Result<QueryData, Error>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WatchEvent {
    Snapshot {
        state_id: u32,
        records: Vec<wire::EntityRecord>,
    },
    Closed(wire::Closed),
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

    fn unavailable() -> Self {
        Self::new(Status::Unavailable, "Git family is unavailable")
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
        let enabled = !std::env::var("YAS_GIT").is_ok_and(|value| value == "0");
        Self {
            inner: Arc::new(RuntimeInner {
                app,
                enabled,
                next_repository: AtomicU64::new(1),
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
        wire::Limits::HARD
    }

    pub(crate) fn session(&self) -> Result<Session, Error> {
        if !self.enabled() {
            return Err(Error::unavailable());
        }
        Ok(Session {
            inner: Arc::new(SessionInner {
                runtime: self.clone(),
                repositories: Mutex::new(HashMap::new()),
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
        if self.inner.repositories.lock().unwrap().len()
            >= wire::Limits::HARD.max_repositories_per_session as usize
        {
            return Err(Error::exhausted("Git repository limit reached"));
        }

        let opened = match &request.source {
            wire::RepositorySource::Submodule {
                parent_repository,
                path,
            } => {
                let parent = self.repository(*parent_repository)?.handle.clone();
                let path = relative_path(path)?;
                tokio::task::spawn_blocking(move || {
                    yas_git::native::open_submodule_path(&parent, &path)
                })
                .await
                .map_err(|_| Error::new(Status::Internal, "Git open task failed"))?
            }
            source => {
                let path = self.resolve_source(source, fs).await?;
                tokio::task::spawn_blocking(move || yas_git::native::open_path(&path))
                    .await
                    .map_err(|_| Error::new(Status::Internal, "Git open task failed"))?
            }
        }
        .map_err(native_error)?;

        let (handle, info) = opened;
        let repository_handle = self.next_repository_handle()?;
        let object_algorithm = match info.object_format {
            yas_git::native::ObjectFormat::Sha1 => yas_wire::schema::git::OBJECT_SHA1 as u8,
            yas_git::native::ObjectFormat::Sha256 => yas_wire::schema::git::OBJECT_SHA256 as u8,
        };
        let mut repository_flags = 0;
        for (present, flag) in [
            (info.bare, yas_wire::schema::git::REPOSITORY_BARE),
            (info.shallow, yas_wire::schema::git::REPOSITORY_SHALLOW),
            (info.sparse, yas_wire::schema::git::REPOSITORY_SPARSE),
            (info.linked, yas_wire::schema::git::REPOSITORY_LINKED),
            (info.writable, yas_wire::schema::git::REPOSITORY_WRITABLE),
            (info.fetchable, yas_wire::schema::git::REPOSITORY_FETCHABLE),
        ] {
            if present {
                repository_flags |= flag as u16;
            }
        }
        let canonical_worktree_path = info
            .worktree_path
            .as_deref()
            .map(platform_path_bytes)
            .transpose()?
            .unwrap_or_default();
        let canonical_git_dir = platform_path_bytes(&info.git_dir)?;
        let revision = 1;
        self.inner.repositories.lock().unwrap().insert(
            repository_handle,
            Repository {
                handle,
                object_algorithm,
                revision: AtomicU64::new(revision),
            },
        );
        Ok(wire::OpenResult {
            repository_handle,
            repository_revision: revision,
            object_algorithm,
            repository_flags,
            canonical_worktree_path,
            canonical_git_dir,
            extensions: Extensions::default(),
        })
    }

    pub(crate) fn close(&self, repository_handle: u64) -> Result<(), Error> {
        self.ensure_open()?;
        self.inner
            .repositories
            .lock()
            .unwrap()
            .remove(&repository_handle);
        Ok(())
    }

    pub(crate) fn repository_revision(&self, repository_handle: u64) -> Result<u64, Error> {
        Ok(self
            .repository(repository_handle)?
            .revision
            .load(Ordering::Acquire)
            .max(1))
    }

    pub(crate) fn cancel_token(&self) -> yas_git::Cancel {
        yas_git::Cancel::default()
    }

    pub(crate) fn watch(
        &self,
        repository_handle: u64,
        datasets: u16,
        options: &wire::WatchOptions,
    ) -> Result<Watch, Error> {
        self.ensure_open()?;
        if datasets == 0 || datasets & !(yas_wire::schema::git::WATCH_DATASETS as u16) != 0 {
            return Err(Error::invalid("invalid Git watch datasets"));
        }
        options
            .validate()
            .map_err(|_| Error::invalid("invalid Git watch options"))?;
        let repository = self.repository(repository_handle)?;
        let ref_prefixes = options
            .ref_prefixes
            .iter()
            .map(|value| {
                std::str::from_utf8(value)
                    .map(str::to_owned)
                    .map_err(|_| Error::invalid("Git ref prefix is not UTF-8"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let duration = |value: u16, default_ms: u64| {
            std::time::Duration::from_millis(if value == 0 {
                default_ms
            } else {
                u64::from(value)
            })
        };
        let state_options = yas_git::StateOptions {
            wants_state: true,
            status: datasets & yas_wire::schema::git::WATCH_STATUS as u16 != 0,
            // Native STATUS is a complete status dataset, not only tracked
            // paths. The repository engine already bounds these walks.
            untracked: true,
            ignored: true,
            tracking: datasets & yas_wire::schema::git::WATCH_UPSTREAMS as u16 != 0,
            remotes: datasets & yas_wire::schema::git::WATCH_REMOTES as u16 != 0,
            ref_prefixes,
            refs_latency: duration(options.refs_settle_ms, 50),
            status_latency: duration(options.status_settle_ms, 500),
        };
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let outbox: yas_git::native::StateSink =
            Box::new(move |event| sender.try_send(event).is_ok());
        let handle = repository.handle.start_native_state(state_options, outbox);
        Ok(Watch {
            repository_handle,
            repository_revision: repository.revision.load(Ordering::Acquire).max(1),
            datasets,
            object_algorithm: repository.object_algorithm,
            handle,
            receiver,
        })
    }

    /// Start a watched typed query. The repository-state engine is the
    /// lossless invalidation source; after each acknowledged state change we
    /// re-evaluate the original typed query from its START cursor. This keeps
    /// WATCH_QUERY's page exactly aligned with one-shot QUERY for every query
    /// kind instead of maintaining a second set of Git readers.
    pub(crate) async fn watch_query(
        &self,
        request: &wire::WatchQuery,
        fs: Option<&yas_fs::Session>,
    ) -> Result<QueryWatch, Error> {
        self.ensure_open()?;
        let (object_algorithm, repository_handle) = {
            let repository = self.repository(request.repository_handle)?;
            (repository.object_algorithm, repository.handle.clone())
        };
        let discover_path = if let wire::QueryBody::Discover { source, .. } = &request.body {
            Some(self.resolve_source(source, fs).await?)
        } else {
            None
        };
        let query = wire::Query {
            repository_handle: request.repository_handle,
            max_records: request.max_records,
            cursor: wire::QueryCursor::Start,
            // Watched pages are always inline and state credit accounts for
            // the complete event, so this value is deliberately unused.
            initial_receive_credit: 0,
            body: request.body.clone(),
            extensions: Extensions::default(),
        };
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let outbox: yas_git::native::StateSink =
            Box::new(move |event| sender.try_send(event).is_ok());
        let handle = repository_handle.start_native_state(
            yas_git::StateOptions {
                // Request the complete mutable repository projection. Its
                // byte-identical suppression is precisely the coalescing we
                // need: a watched query is re-run only when refs, index,
                // worktree, remotes, tracking, or operation state changes.
                wants_state: true,
                status: true,
                untracked: true,
                ignored: true,
                tracking: true,
                remotes: true,
                ..Default::default()
            },
            outbox,
        );
        Ok(QueryWatch {
            object_algorithm,
            repository: repository_handle,
            request: query,
            discover_path,
            handle,
            receiver,
        })
    }

    pub(crate) async fn query(
        &self,
        request: &wire::Query,
        fs: Option<&yas_fs::Session>,
        cancel: yas_git::Cancel,
    ) -> Result<QueryData, Error> {
        self.ensure_open()?;
        let (handle, object_algorithm) = if request.repository_handle == 0 {
            if !matches!(request.body, wire::QueryBody::Discover { .. }) {
                return Err(Error::invalid("Git query requires a repository"));
            }
            (None, yas_wire::schema::git::OBJECT_SHA1 as u8)
        } else {
            let repository = self.repository(request.repository_handle)?;
            (Some(repository.handle.clone()), repository.object_algorithm)
        };
        let discover_path = if let wire::QueryBody::Discover { source, .. } = &request.body {
            Some(self.resolve_source(source, fs).await?)
        } else {
            None
        };
        let request = request.clone();
        tokio::task::spawn_blocking(move || {
            run_query(
                handle.as_ref(),
                object_algorithm,
                &request,
                discover_path.as_deref(),
                &cancel,
            )
        })
        .await
        .map_err(|_| Error::new(Status::Internal, "Git query task failed"))?
    }

    pub(crate) async fn fetch(
        &self,
        request: &wire::Fetch,
        cancel: yas_git::Cancel,
    ) -> Result<wire::FetchResult, Error> {
        self.ensure_open()?;
        if !yas_git::native::fetch_available() {
            return Err(Error::new(Status::Unavailable, "Git fetch is disabled"));
        }
        let (handle, object_algorithm, revision) = {
            let repository = self.repository(request.repository_handle)?;
            (
                repository.handle.clone(),
                repository.object_algorithm,
                repository.revision.load(Ordering::Acquire).max(1),
            )
        };
        let remote = std::str::from_utf8(&request.remote)
            .map_err(|_| Error::invalid("Git remote is not UTF-8"))?
            .to_owned();
        let refspecs = request
            .refspecs
            .iter()
            .map(|value| {
                std::str::from_utf8(value)
                    .map(str::to_owned)
                    .map_err(|_| Error::invalid("Git refspec is not UTF-8"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let flags = map_u16_flags(request.flags, yas_wire::schema::git::FETCH_FLAGS, "FETCH")?;
        let timeout_ms = request.timeout_ms;
        let response = tokio::task::spawn_blocking(move || {
            handle.native_fetch(&remote, &refspecs, flags, timeout_ms, &cancel)
        })
        .await
        .map_err(|_| Error::new(Status::Internal, "Git fetch task failed"))?
        .map_err(native_error)?;
        let mut refs = Vec::new();
        for record in response.refs {
            let yas_git::native::FetchRef {
                forced,
                pruned,
                new_ref,
                tag_update,
                status,
                old_object,
                new_object,
                name,
                detail,
            } = record;
            refs.push(wire::FetchRefResult {
                flags: map_fetch_ref_flags(forced, pruned, new_ref, tag_update),
                status: native_status(status).code(),
                old: optional_oid(object_algorithm, old_object)?,
                new: optional_oid(object_algorithm, new_object)?,
                name,
                detail,
            });
        }
        let repository_revision = {
            let repository = self.repository(request.repository_handle)?;
            repository
                .revision
                .fetch_add(1, Ordering::AcqRel)
                .max(revision)
                .saturating_add(1)
        };
        Ok(wire::FetchResult {
            repository_revision,
            refs,
            extensions: Extensions::default(),
        })
    }

    fn repository(&self, repository_handle: u64) -> Result<RepositoryRef<'_>, Error> {
        let repositories = self.inner.repositories.lock().unwrap();
        if !repositories.contains_key(&repository_handle) {
            return Err(Error::new(Status::NotFound, "unknown Git repository"));
        }
        Ok(RepositoryRef {
            repositories,
            repository_handle,
        })
    }

    async fn resolve_source(
        &self,
        source: &wire::RepositorySource,
        fs: Option<&yas_fs::Session>,
    ) -> Result<PathBuf, Error> {
        match source {
            wire::RepositorySource::PlatformPath(path) => platform_path(path),
            wire::RepositorySource::Fs { root_handle, path } => fs
                .ok_or_else(|| Error::new(Status::NotFound, "unknown FS session"))?
                .resolved_path(*root_handle, path)
                .map_err(|error| Error::new(Status::NotFound, error.to_string())),
            wire::RepositorySource::Submodule { .. } => {
                Err(Error::invalid("nested Git submodule source"))
            }
            wire::RepositorySource::TerminalCwd {
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

    fn ensure_open(&self) -> Result<(), Error> {
        if self.inner.closed.load(Ordering::Acquire) {
            Err(Error::new(Status::Unavailable, "Git session is closed"))
        } else {
            Ok(())
        }
    }

    fn next_repository_handle(&self) -> Result<u64, Error> {
        let candidate = self
            .inner
            .runtime
            .inner
            .next_repository
            .fetch_add(1, Ordering::Relaxed);
        if candidate == 0 || candidate == u64::MAX {
            return Err(Error::exhausted("Git repository handle space exhausted"));
        }
        Ok(candidate)
    }
}

impl Watch {
    pub(crate) async fn next(&mut self) -> Option<Result<WatchEvent, Error>> {
        match self.receiver.recv().await? {
            yas_git::native::StateEvent::Snapshot { state_id, records } => Some(
                convert_state_snapshot(self.datasets, self.object_algorithm, state_id, records)
                    .map(|records| WatchEvent::Snapshot { state_id, records }),
            ),
            yas_git::native::StateEvent::Closed(reason) => {
                Some(Ok(WatchEvent::Closed(wire::Closed {
                    repository_handle: self.repository_handle,
                    repository_revision: self.repository_revision,
                    reason: map_closed_reason(reason),
                    detail: "Git repository watch closed".to_string(),
                })))
            }
        }
    }

    pub(crate) fn acknowledge(&self, state_id: u32) {
        self.handle.ack(state_id);
    }
}

impl QueryWatch {
    pub(crate) async fn next(&mut self) -> Option<QueryWatchUpdate> {
        let yas_git::native::StateEvent::Snapshot { state_id, .. } = self.receiver.recv().await?
        else {
            return None;
        };
        let repository = self.repository.clone();
        let object_algorithm = self.object_algorithm;
        let request = self.request.clone();
        let discover_path = self.discover_path.clone();
        let converted = tokio::task::spawn_blocking(move || {
            run_query(
                Some(&repository),
                object_algorithm,
                &request,
                discover_path.as_deref(),
                &yas_git::Cancel::default(),
            )
        })
        .await
        .unwrap_or_else(|_| {
            Err(Error::new(
                Status::Internal,
                "Git watched query task failed",
            ))
        })
        .and_then(inline_watched_content);
        Some(QueryWatchUpdate {
            update_id: state_id,
            result: converted,
        })
    }

    pub(crate) fn acknowledge(&self, update_id: u32) {
        self.handle.ack(update_id);
    }

    pub(crate) fn stop(&self) {
        self.handle.stop();
    }
}

/// WATCH_QUERY pages cannot contain Transfer descriptors because their state
/// credit accounts for the complete event. Convert content windows to inline
/// typed records, or surface a recoverable page error when the requested
/// window exceeds the negotiated v1 inline bound.
fn inline_watched_content(mut data: QueryData) -> Result<QueryData, Error> {
    let mut records = Vec::with_capacity(data.records.len());
    for item in data.records {
        match item {
            QueryItem::Record(record) => records.push(QueryItem::Record(record)),
            QueryItem::Content(content) => {
                if content.bytes.len() > yas_wire::schema::git::MAX_INLINE_BYTES as usize {
                    return Err(Error::exhausted(
                        "Git watched-query content exceeds the inline page limit",
                    ));
                }
                let next_offset = content
                    .offset
                    .checked_add(content.bytes.len() as u64)
                    .ok_or_else(|| Error::new(Status::Internal, "Git content offset overflow"))?;
                let record = wire::ContentRecord {
                    object: content.object,
                    byte_len: content.byte_len,
                    offset: content.offset,
                    next_offset,
                    delivery: wire::ContentDelivery::Inline(content.bytes),
                };
                records.push(QueryItem::Record(match content.kind {
                    ContentKind::Blob => wire::QueryRecord::Blob(record),
                    ContentKind::Patch => wire::QueryRecord::PatchContent(record),
                }));
            }
        }
    }
    data.records = records;
    Ok(data)
}

impl Drop for QueryWatch {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.repositories.lock().unwrap().clear();
    }
}

struct RepositoryRef<'a> {
    repositories: std::sync::MutexGuard<'a, HashMap<u64, Repository>>,
    repository_handle: u64,
}

impl std::ops::Deref for RepositoryRef<'_> {
    type Target = Repository;

    fn deref(&self) -> &Self::Target {
        &self.repositories[&self.repository_handle]
    }
}

fn native_error(error: yas_git::native::Failure) -> Error {
    Error::new(
        match error.status {
            yas_git::native::Status::Ok => Status::Ok,
            yas_git::native::Status::NotFound => Status::NotFound,
            yas_git::native::Status::WrongType | yas_git::native::Status::Invalid => {
                Status::Invalid
            }
            yas_git::native::Status::Permission => Status::Unavailable,
            yas_git::native::Status::ResourceExhausted => Status::ResourceExhausted,
            yas_git::native::Status::Cancelled => Status::Cancelled,
            yas_git::native::Status::Conflict => Status::Conflict,
            yas_git::native::Status::Other => Status::Internal,
        },
        error.detail,
    )
}

fn native_status(status: yas_git::native::Status) -> Status {
    match status {
        yas_git::native::Status::Ok => Status::Ok,
        yas_git::native::Status::NotFound => Status::NotFound,
        yas_git::native::Status::WrongType | yas_git::native::Status::Invalid => Status::Invalid,
        yas_git::native::Status::Permission => Status::Unavailable,
        yas_git::native::Status::ResourceExhausted => Status::ResourceExhausted,
        yas_git::native::Status::Cancelled => Status::Cancelled,
        yas_git::native::Status::Conflict => Status::Conflict,
        yas_git::native::Status::Other => Status::Internal,
    }
}

fn run_query(
    repository: Option<&yas_git::RepoHandle>,
    object_algorithm: u8,
    request: &wire::Query,
    discover_path: Option<&Path>,
    cancel: &yas_git::Cancel,
) -> Result<QueryData, Error> {
    use wire::QueryBody;

    match &request.body {
        QueryBody::Resolve { spec } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let spec = std::str::from_utf8(spec)
                .map_err(|_| Error::invalid("Git revision specification is not UTF-8"))?;
            let response = repository
                .native_resolve(spec, cancel)
                .map_err(native_error)?;
            let records = response
                .tips
                .into_iter()
                .map(|object| {
                    object_item(
                        object_algorithm,
                        object,
                        yas_wire::schema::git::OBJECT_ROLE_TIP,
                    )
                })
                .chain(response.hides.into_iter().map(|object| {
                    object_item(
                        object_algorithm,
                        object,
                        yas_wire::schema::git::OBJECT_ROLE_HIDE,
                    )
                }))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(query_page(records, wire::QueryCursor::Start))
        }
        QueryBody::MergeBase { objects } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let objects = objects
                .iter()
                .map(engine_oid)
                .collect::<Result<Vec<_>, _>>()?;
            let bases = repository
                .native_merge_base(&objects, cancel)
                .map_err(native_error)?;
            let records = bases
                .into_iter()
                .map(|object| {
                    object_item(
                        object_algorithm,
                        object,
                        yas_wire::schema::git::OBJECT_ROLE_RESULT,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(query_page(records, wire::QueryCursor::Start))
        }
        QueryBody::Log {
            spec,
            tips,
            hides,
            path,
            flags,
        } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let mut resolved_tips = tips.iter().map(engine_oid).collect::<Result<Vec<_>, _>>()?;
            let resolved_hides = hides
                .iter()
                .map(engine_oid)
                .collect::<Result<Vec<_>, _>>()?;
            if !spec.is_empty() {
                let spec = std::str::from_utf8(spec)
                    .map_err(|_| Error::invalid("Git revision specification is not UTF-8"))?;
                let response = repository
                    .native_resolve(spec, cancel)
                    .map_err(native_error)?;
                resolved_tips = response.tips;
                if !response.hides.is_empty() {
                    return run_log(
                        repository,
                        object_algorithm,
                        request,
                        LogInputs {
                            tips: resolved_tips,
                            hides: response.hides,
                            path,
                            flags: *flags,
                        },
                        cancel,
                    );
                }
            }
            run_log(
                repository,
                object_algorithm,
                request,
                LogInputs {
                    tips: resolved_tips,
                    hides: resolved_hides,
                    path,
                    flags: *flags,
                },
                cancel,
            )
        }
        QueryBody::Tree { tree, path } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let after = match &request.cursor {
                wire::QueryCursor::Start => Vec::new(),
                wire::QueryCursor::Path(path) => path.components.clone(),
                _ => return Err(Error::invalid("invalid Git TREE cursor")),
            };
            let response = repository
                .native_tree(engine_oid(tree)?, &path.components, &after, cancel)
                .map_err(native_error)?;
            let mut out = Vec::new();
            let mut next = wire::QueryCursor::Start;
            for record in response.records {
                match record {
                    yas_git::native::TreeRecord::Entry {
                        kind,
                        mode,
                        object,
                        name,
                    } => out.push(QueryItem::Record(wire::QueryRecord::TreeEntry(
                        wire::TreeEntryRecord {
                            entry_kind: tree_kind(kind),
                            mode,
                            name,
                            object: required_oid(object_algorithm, object)?,
                        },
                    ))),
                    yas_git::native::TreeRecord::Cursor { after } => {
                        next = wire::QueryCursor::Path(fs_wire::Path { components: after });
                    }
                }
            }
            Ok(query_page(out, next))
        }
        QueryBody::Blob {
            object,
            path,
            offset,
            max_bytes,
            flags,
        } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let response = repository
                .native_blob(
                    engine_oid(object)?,
                    path.as_ref().map(|path| path.components.as_slice()),
                    *offset,
                    *max_bytes,
                    map_blob_flags(*flags)?,
                )
                .map_err(native_error)?;
            let item = QueryItem::Content(OwnedContent {
                kind: ContentKind::Blob,
                object: object.clone(),
                byte_len: response.byte_len,
                offset: *offset,
                bytes: response.bytes,
            });
            Ok(query_page(vec![item], wire::QueryCursor::Start))
        }
        QueryBody::Diff {
            left,
            right,
            path,
            rename_threshold,
            flags,
        } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let after = match &request.cursor {
                wire::QueryCursor::Start => Vec::new(),
                wire::QueryCursor::Path(path) => path.components.clone(),
                _ => return Err(Error::invalid("invalid Git DIFF cursor")),
            };
            let response = repository
                .native_diff(
                    &yas_git::native::DiffRequest {
                        flags: map_u16_flags(*flags, yas_wire::schema::git::DIFF_FLAGS, "DIFF")?,
                        rename_threshold: *rename_threshold,
                        old: native_endpoint(left)?,
                        new: native_endpoint(right)?,
                        path: path
                            .as_ref()
                            .map(|path| path.components.clone())
                            .unwrap_or_default(),
                        after,
                    },
                    cancel,
                )
                .map_err(native_error)?;
            let mut out = Vec::new();
            let mut next = wire::QueryCursor::Start;
            for record in response.records {
                match record {
                    yas_git::native::DiffRecord::Entry {
                        status,
                        similarity_percent,
                        binary,
                        submodule,
                        filtered,
                        old_mode,
                        new_mode,
                        old_object,
                        new_object,
                        old_path,
                        new_path,
                    } => out.push(QueryItem::Record(wire::QueryRecord::Diff(
                        wire::DiffRecord {
                            status: diff_status(status)?,
                            similarity_percent,
                            flags: map_diff_record_flags(binary, submodule, filtered),
                            old_path: old_path.map(|components| fs_wire::Path { components }),
                            new_path: new_path.map(|components| fs_wire::Path { components }),
                            old_mode,
                            new_mode,
                            old_object: optional_oid(object_algorithm, old_object)?,
                            new_object: optional_oid(object_algorithm, new_object)?,
                        },
                    ))),
                    yas_git::native::DiffRecord::Base(object) => out.push(object_item(
                        object_algorithm,
                        object,
                        yas_wire::schema::git::OBJECT_ROLE_RESULT,
                    )?),
                    yas_git::native::DiffRecord::Cursor(after) => {
                        next = wire::QueryCursor::Path(fs_wire::Path { components: after });
                    }
                }
            }
            Ok(query_page(out, next))
        }
        QueryBody::Patch {
            left,
            right,
            path,
            context_lines,
            rename_threshold,
            max_bytes,
            flags,
        } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let (after, after_pos) = match &request.cursor {
                wire::QueryCursor::Start => (Vec::new(), 0),
                wire::QueryCursor::Patch { path, position } => (path.components.clone(), *position),
                _ => return Err(Error::invalid("invalid Git PATCH cursor")),
            };
            let response = repository
                .native_patch(
                    &yas_git::native::PatchRequest {
                        flags: *flags,
                        context_lines: *context_lines,
                        rename_threshold: *rename_threshold,
                        old: native_endpoint(left)?,
                        new: native_endpoint(right)?,
                        path: path
                            .as_ref()
                            .map(|path| path.components.clone())
                            .unwrap_or_default(),
                        max_bytes: *max_bytes,
                        after,
                        after_position: after_pos,
                    },
                    cancel,
                )
                .map_err(native_error)?;
            let records = match response {
                yas_git::native::PatchResult::Text(data) => {
                    let object = endpoint_content_object(right)
                        .or_else(|| endpoint_content_object(left))
                        .ok_or_else(|| {
                            Error::new(
                                Status::Unsupported,
                                "text patch between mutable endpoints has no content object",
                            )
                        })?;
                    let byte_len = data.len() as u64;
                    return Ok(query_page(
                        vec![QueryItem::Content(OwnedContent {
                            kind: ContentKind::Patch,
                            object,
                            byte_len,
                            offset: 0,
                            bytes: data,
                        })],
                        wire::QueryCursor::Start,
                    ));
                }
                yas_git::native::PatchResult::Structured(records) => records,
            };
            let mut out = Vec::new();
            let mut next = wire::QueryCursor::Start;
            for record in records {
                match record {
                    yas_git::native::PatchRecord::File {
                        status,
                        similarity_percent,
                        binary,
                        filtered,
                        old_path,
                        new_path,
                    } => out.push(QueryItem::Record(wire::QueryRecord::PatchFile(
                        wire::PatchFileRecord {
                            status: diff_status(status)?,
                            similarity_percent,
                            flags: map_patch_file_flags(binary, filtered),
                            old_path: old_path.map(|components| fs_wire::Path { components }),
                            new_path: new_path.map(|components| fs_wire::Path { components }),
                        },
                    ))),
                    yas_git::native::PatchRecord::Row {
                        old_line,
                        new_line,
                        old_text,
                        new_text,
                        old_spans,
                        new_spans,
                    } => out.push(QueryItem::Record(wire::QueryRecord::PatchRow(
                        wire::PatchRowRecord {
                            old_line,
                            new_line,
                            old_text,
                            new_text,
                            old_spans: patch_spans(old_spans),
                            new_spans: patch_spans(new_spans),
                        },
                    ))),
                    yas_git::native::PatchRecord::Gap { old_line, new_line } => {
                        out.push(QueryItem::Record(wire::QueryRecord::PatchGap(
                            wire::PatchGapRecord { old_line, new_line },
                        )))
                    }
                    yas_git::native::PatchRecord::Base(object) => out.push(QueryItem::Record(
                        wire::QueryRecord::PatchBase(wire::PatchBaseRecord {
                            object: required_oid(object_algorithm, object)?,
                        }),
                    )),
                    yas_git::native::PatchRecord::Cursor { after, position } => {
                        next = wire::QueryCursor::Patch {
                            path: fs_wire::Path { components: after },
                            position,
                        };
                    }
                }
            }
            Ok(query_page(out, next))
        }
        QueryBody::Index { path, flags } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            // The repository engine enumerates the index. Staged is its natural
            // dataset; zero preserves its historical default.
            if *flags & !(yas_wire::schema::git::INDEX_STAGED as u16) != 0 {
                return Err(Error::new(
                    Status::Unsupported,
                    "unstaged, untracked, and ignored INDEX datasets are unavailable",
                ));
            }
            let after = match &request.cursor {
                wire::QueryCursor::Start => Vec::new(),
                wire::QueryCursor::Path(path) => path.components.clone(),
                _ => return Err(Error::invalid("invalid Git INDEX cursor")),
            };
            let response = repository
                .native_index(
                    path.as_ref()
                        .map(|path| path.components.as_slice())
                        .unwrap_or_default(),
                    &after,
                    cancel,
                )
                .map_err(native_error)?;
            let mut out = Vec::new();
            let mut next = wire::QueryCursor::Start;
            for record in response.records {
                match record {
                    yas_git::native::IndexRecord::Entry {
                        stage,
                        intent_to_add,
                        skip_worktree,
                        mode,
                        size,
                        modified_unix_ns,
                        object,
                        path,
                    } => out.push(QueryItem::Record(wire::QueryRecord::IndexEntry(
                        wire::IndexEntryRecord {
                            stage,
                            status: yas_wire::schema::git::INDEX_STATUS_UNMODIFIED as u8,
                            flags: map_index_flags(stage, intent_to_add, skip_worktree),
                            path: fs_wire::Path { components: path },
                            mode,
                            size,
                            modified_unix_ns: i64::try_from(modified_unix_ns).unwrap_or(i64::MAX),
                            object: required_oid(object_algorithm, object)?,
                        },
                    ))),
                    yas_git::native::IndexRecord::Cursor(after) => {
                        next = wire::QueryCursor::Path(fs_wire::Path { components: after });
                    }
                }
            }
            Ok(query_page(out, next))
        }
        QueryBody::Discover {
            max_depth, flags, ..
        } => {
            let path = discover_path.ok_or_else(|| Error::invalid("missing discovery source"))?;
            let after = match &request.cursor {
                wire::QueryCursor::Start => None,
                wire::QueryCursor::PlatformPath(path) => Some(platform_path(path)?),
                _ => return Err(Error::invalid("invalid Git DISCOVER cursor")),
            };
            let response = yas_git::native::discover_path(
                map_u16_flags(
                    *flags,
                    yas_wire::schema::git::DISCOVER_QUERY_FLAGS,
                    "DISCOVER",
                )?,
                u8::try_from(*max_depth).unwrap_or(u8::MAX),
                path,
                after.as_deref(),
                cancel,
            )
            .map_err(native_error)?;
            let mut out = Vec::new();
            let mut next = wire::QueryCursor::Start;
            for record in response.records {
                match record {
                    yas_git::native::DiscoveryRecord::Repository {
                        bare,
                        linked,
                        submodule,
                        object_format,
                        worktree_path,
                        git_dir,
                    } => {
                        let object_algorithm = match object_format {
                            yas_git::native::ObjectFormat::Sha1 => {
                                yas_wire::schema::git::OBJECT_SHA1 as u8
                            }
                            yas_git::native::ObjectFormat::Sha256 => {
                                yas_wire::schema::git::OBJECT_SHA256 as u8
                            }
                        };
                        out.push(QueryItem::Record(wire::QueryRecord::Discovery(
                            wire::DiscoveryRecord {
                                flags: map_discovery_flags(bare, linked, submodule),
                                object_algorithm,
                                worktree_path: worktree_path
                                    .as_deref()
                                    .map(platform_path_bytes)
                                    .transpose()?
                                    .unwrap_or_default(),
                                git_dir: platform_path_bytes(&git_dir)?,
                            },
                        )));
                    }
                    yas_git::native::DiscoveryRecord::Cursor(after) => {
                        next = wire::QueryCursor::PlatformPath(platform_path_bytes(&after)?);
                    }
                }
            }
            Ok(query_page(out, next))
        }
        QueryBody::Blame {
            object,
            path,
            start_line,
            line_count,
            flags,
        } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let start_line = match request.cursor {
                wire::QueryCursor::Start => *start_line,
                wire::QueryCursor::Position(position) => u32::try_from(position)
                    .unwrap_or(u32::MAX)
                    .saturating_add(1),
                _ => return Err(Error::invalid("invalid Git BLAME cursor")),
            };
            let response = repository
                .native_blame(
                    engine_oid(object)?,
                    &path.components,
                    start_line,
                    *line_count,
                    map_u16_flags(*flags, yas_wire::schema::git::BLAME_FLAGS, "BLAME")?,
                    cancel,
                )
                .map_err(native_error)?;
            let mut out = Vec::new();
            let mut next = wire::QueryCursor::Start;
            for record in response.records {
                match record {
                    yas_git::native::BlameRecord::Range {
                        flags,
                        commit,
                        start_line,
                        line_count,
                        original_start_line,
                        original_path,
                    } => out.push(QueryItem::Record(wire::QueryRecord::Blame(
                        wire::BlameRecord {
                            flags: u16::from(flags),
                            start_line,
                            end_line: start_line.saturating_add(line_count),
                            original_start_line,
                            commit: required_oid(object_algorithm, commit)?,
                            original_path: original_path
                                .map(|components| fs_wire::Path { components }),
                            author: Vec::new(),
                            summary: Vec::new(),
                        },
                    ))),
                    yas_git::native::BlameRecord::Cursor(position) => {
                        next = wire::QueryCursor::Position(position);
                    }
                }
            }
            Ok(query_page(out, next))
        }
        QueryBody::Reflog { name, flags } => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let name = std::str::from_utf8(name)
                .map_err(|_| Error::invalid("Git reflog name is not UTF-8"))?;
            let after_pos = match request.cursor {
                wire::QueryCursor::Start => 0,
                wire::QueryCursor::Position(position) => position,
                _ => return Err(Error::invalid("invalid Git REFLOG cursor")),
            };
            let response = repository
                .native_reflog(
                    name,
                    map_u16_flags(*flags, yas_wire::schema::git::REFLOG_FLAGS, "REFLOG")?,
                    request.max_records,
                    after_pos,
                    cancel,
                )
                .map_err(native_error)?;
            let mut out = Vec::new();
            let mut next = wire::QueryCursor::Start;
            let mut index = after_pos;
            for record in response.records {
                match record {
                    yas_git::native::ReflogRecord::Entry {
                        flags,
                        old_object,
                        new_object,
                        committed_unix_seconds,
                        timezone_minutes,
                        message,
                    } => {
                        out.push(QueryItem::Record(wire::QueryRecord::Reflog(
                            wire::ReflogRecord {
                                flags: u16::from(flags),
                                index,
                                old_object: wire_oid(object_algorithm, old_object)?,
                                new_object: wire_oid(object_algorithm, new_object)?,
                                committer: Vec::new(),
                                committed_unix_seconds,
                                timezone_minutes,
                                message,
                            },
                        )));
                        index = index.saturating_add(1);
                    }
                    yas_git::native::ReflogRecord::Cursor(position) => {
                        next = wire::QueryCursor::Position(position);
                    }
                }
            }
            Ok(query_page(out, next))
        }
        QueryBody::Worktrees => {
            let repository = repository.ok_or_else(|| Error::invalid("missing repository"))?;
            let after_pos = match request.cursor {
                wire::QueryCursor::Start => 0,
                wire::QueryCursor::Position(position) => position,
                _ => return Err(Error::invalid("invalid Git WORKTREES cursor")),
            };
            let response = repository
                .native_worktrees(after_pos, cancel)
                .map_err(native_error)?;
            let mut out = Vec::new();
            let mut next = wire::QueryCursor::Start;
            for record in response.records {
                match record {
                    yas_git::native::WorktreeRecord::Worktree {
                        bare,
                        main,
                        current,
                        locked,
                        prunable,
                        detached,
                        head,
                        path,
                        branch,
                        lock_reason,
                    } => out.push(QueryItem::Record(wire::QueryRecord::Worktree(
                        wire::WorktreeRecord {
                            flags: map_worktree_flags(
                                bare, main, current, locked, prunable, detached,
                            ),
                            path: path
                                .as_deref()
                                .map(platform_path_bytes)
                                .transpose()?
                                .unwrap_or_default(),
                            head: optional_oid(object_algorithm, head)?,
                            branch,
                            lock_reason,
                        },
                    ))),
                    yas_git::native::WorktreeRecord::Cursor(position) => {
                        next = wire::QueryCursor::Position(position);
                    }
                }
            }
            Ok(query_page(out, next))
        }
    }
}

struct LogInputs<'a> {
    tips: Vec<yas_git::native::Oid>,
    hides: Vec<yas_git::native::Oid>,
    path: &'a Option<fs_wire::Path>,
    flags: u16,
}

fn run_log(
    repository: &yas_git::RepoHandle,
    object_algorithm: u8,
    request: &wire::Query,
    inputs: LogInputs<'_>,
    cancel: &yas_git::Cancel,
) -> Result<QueryData, Error> {
    let LogInputs {
        mut tips,
        hides,
        path,
        flags,
    } = inputs;
    if let wire::QueryCursor::LogFrontier(frontier) = &request.cursor {
        tips = frontier
            .iter()
            .map(engine_oid)
            .collect::<Result<Vec<_>, _>>()?;
    } else if !matches!(request.cursor, wire::QueryCursor::Start) {
        return Err(Error::invalid("invalid Git LOG cursor"));
    }
    let page = repository
        .native_log(
            &yas_git::native::LogRequest {
                flags: map_log_flags(flags)?,
                limit: request.max_records,
                path: path
                    .as_ref()
                    .map(|path| path.components.clone())
                    .unwrap_or_default(),
                tips,
                hides,
            },
            cancel,
        )
        .map_err(native_error)?;
    convert_log_records(object_algorithm, page)
}

fn convert_log_records(
    object_algorithm: u8,
    page: yas_git::native::LogPage,
) -> Result<QueryData, Error> {
    let mut records = Vec::new();
    for record in page.records {
        match record {
            yas_git::native::LogRecord::Commit {
                lossy_encoding,
                object,
                tree,
                parents,
                authored_unix_seconds,
                author_timezone_minutes,
                committed_unix_seconds,
                committer_timezone_minutes,
                author_name,
                author_email,
                committer_name,
                committer_email,
                message,
            } => records.push(QueryItem::Record(wire::QueryRecord::Commit(
                wire::CommitRecord {
                    flags: if lossy_encoding {
                        yas_wire::schema::git::COMMIT_LOSSY_ENCODING as u16
                    } else {
                        0
                    },
                    object: required_oid(object_algorithm, object)?,
                    tree: required_oid(object_algorithm, tree)?,
                    parents: parents
                        .into_iter()
                        .map(|parent| required_oid(object_algorithm, parent))
                        .collect::<Result<Vec<_>, _>>()?,
                    authored_unix_seconds,
                    author_timezone_minutes,
                    committed_unix_seconds,
                    committer_timezone_minutes,
                    author_name,
                    author_email,
                    committer_name,
                    committer_email,
                    message,
                },
            ))),
            yas_git::native::LogRecord::PathAt {
                kind,
                mode,
                object,
                path,
            } => records.push(QueryItem::Record(wire::QueryRecord::LogPath(
                wire::LogPathRecord {
                    entry_kind: tree_kind(kind),
                    mode,
                    object: optional_oid(object_algorithm, object)?,
                    path: fs_wire::Path { components: path },
                },
            ))),
        }
    }
    let next = if page.more {
        wire::QueryCursor::LogFrontier(
            page.frontier
                .into_iter()
                .map(|object| required_oid(object_algorithm, object))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        wire::QueryCursor::Start
    };
    Ok(query_page(records, next))
}

fn query_page(records: Vec<QueryItem>, next_cursor: wire::QueryCursor) -> QueryData {
    QueryData {
        records,
        next_cursor,
        total_hint: 0,
    }
}

fn convert_state_snapshot(
    datasets: u16,
    object_algorithm: u8,
    state_id: u32,
    records: Vec<yas_git::native::StateRecord>,
) -> Result<Vec<wire::EntityRecord>, Error> {
    use yas_wire::schema::git as native;
    let revision = u64::from(state_id.max(1));
    let mut converted = Vec::new();
    for record in records {
        let (key, body) = match record {
            yas_git::native::StateRecord::Head {
                detached,
                unborn,
                object,
                symbolic_target,
            } if datasets & native::WATCH_HEAD as u16 != 0 => {
                let mut mapped = 0;
                if detached {
                    mapped |= native::HEAD_DETACHED as u16;
                }
                if unborn {
                    mapped |= native::HEAD_UNBORN as u16;
                }
                (
                    b"HEAD".to_vec(),
                    wire::EntityBody::Head(wire::HeadEntityBody {
                        flags: mapped,
                        object: optional_oid(object_algorithm, object)?,
                        symbolic_target,
                    }),
                )
            }
            yas_git::native::StateRecord::Ref {
                peeled_valid,
                symbolic,
                object,
                peeled,
                name,
                symbolic_target,
            } if datasets & native::WATCH_REFS as u16 != 0 => {
                let mut mapped = 0;
                if peeled_valid {
                    mapped |= native::REF_PEELED as u16;
                }
                if symbolic {
                    mapped |= native::REF_SYMBOLIC as u16;
                }
                (
                    name,
                    wire::EntityBody::Ref(wire::RefEntityBody {
                        flags: mapped,
                        object: required_oid(object_algorithm, object)?,
                        peeled: if peeled_valid {
                            Some(required_oid(object_algorithm, peeled)?)
                        } else {
                            None
                        },
                        symbolic_target,
                    }),
                )
            }
            yas_git::native::StateRecord::Operation { kind, head, detail }
                if datasets & native::WATCH_OPERATION as u16 != 0 =>
            {
                let head = optional_oid(object_algorithm, head)?;
                (
                    b"operation".to_vec(),
                    wire::EntityBody::Operation(wire::OperationEntityBody {
                        operation_kind: kind,
                        flags: if head.is_some() {
                            native::OPERATION_HEAD_PRESENT as u8
                        } else {
                            0
                        },
                        head,
                        detail: detail.to_string(),
                    }),
                )
            }
            yas_git::native::StateRecord::Status {
                index_status,
                worktree_status,
                conflicted,
                object,
                old_path,
                path,
            } if datasets & native::WATCH_STATUS as u16 != 0 => {
                let content = optional_oid(object_algorithm, object)?;
                let old_path = old_path.map(|components| fs_wire::Path { components });
                let mut mapped = 0;
                if conflicted {
                    mapped |= native::STATE_STATUS_CONFLICTED as u16;
                }
                if content.is_some() {
                    mapped |= native::STATE_STATUS_CONTENT_PRESENT as u16;
                }
                if old_path.is_some() {
                    mapped |= native::STATE_STATUS_OLD_PATH_PRESENT as u16;
                }
                let path = fs_wire::Path { components: path };
                (
                    yas_wire::Encode::encode(&path)
                        .map_err(|_| Error::new(Status::Internal, "invalid Git status path"))?,
                    wire::EntityBody::Status(wire::StatusEntityBody {
                        index_status: status_kind(index_status)?,
                        worktree_status: status_kind(worktree_status)?,
                        flags: mapped,
                        content,
                        old_path,
                    }),
                )
            }
            yas_git::native::StateRecord::Upstream {
                gone,
                counts_valid,
                ahead,
                behind,
                name,
                upstream,
            } if datasets & native::WATCH_UPSTREAMS as u16 != 0 => {
                let mut mapped = 0;
                if gone {
                    mapped |= native::UPSTREAM_GONE as u16;
                }
                if counts_valid {
                    mapped |= native::UPSTREAM_COUNTS_VALID as u16;
                }
                (
                    name,
                    wire::EntityBody::Upstream(wire::UpstreamEntityBody {
                        flags: mapped,
                        ahead,
                        behind,
                        upstream,
                    }),
                )
            }
            yas_git::native::StateRecord::Stash {
                index,
                object,
                created_unix_seconds,
                timezone_minutes,
                message,
            } if datasets & native::WATCH_STASHES as u16 != 0 => (
                u32::from(index).to_le_bytes().to_vec(),
                wire::EntityBody::Stash(wire::StashEntityBody {
                    object: required_oid(object_algorithm, object)?,
                    created_unix_seconds,
                    timezone_minutes,
                    message,
                }),
            ),
            yas_git::native::StateRecord::Remote {
                default,
                name,
                fetch_url,
                push_url,
            } if datasets & native::WATCH_REMOTES as u16 != 0 => (
                name,
                wire::EntityBody::Remote(wire::RemoteEntityBody {
                    flags: if default {
                        native::REMOTE_DEFAULT as u16
                    } else {
                        0
                    },
                    fetch_url,
                    push_url,
                }),
            ),
            yas_git::native::StateRecord::WorktreeGeneration { count, digest }
                if datasets & native::WATCH_WORKTREE_GENERATION as u16 != 0 =>
            {
                (
                    b"worktrees".to_vec(),
                    wire::EntityBody::WorktreeGeneration(wire::WorktreeGenerationEntityBody {
                        count,
                        digest,
                    }),
                )
            }
            _ => continue,
        };
        converted.push(wire::EntityRecord {
            entity_kind: body.entity_kind(),
            key,
            revision,
            body,
            extensions: Extensions::default(),
        });
    }
    Ok(converted)
}

fn status_kind(status: u8) -> Result<u8, Error> {
    use yas_wire::schema::git as native;
    match status {
        b' ' | 0 => Ok(native::WORKTREE_STATUS_NONE as u8),
        b'A' => Ok(native::WORKTREE_STATUS_ADDED as u8),
        b'M' => Ok(native::WORKTREE_STATUS_MODIFIED as u8),
        b'D' => Ok(native::WORKTREE_STATUS_DELETED as u8),
        b'R' => Ok(native::WORKTREE_STATUS_RENAMED as u8),
        b'C' => Ok(native::WORKTREE_STATUS_COPIED as u8),
        b'T' => Ok(native::WORKTREE_STATUS_TYPE_CHANGED as u8),
        b'U' => Ok(native::WORKTREE_STATUS_UNMERGED as u8),
        b'?' => Ok(native::WORKTREE_STATUS_UNTRACKED as u8),
        b'!' => Ok(native::WORKTREE_STATUS_IGNORED as u8),
        _ => Err(Error::new(Status::Internal, "unknown Git status code")),
    }
}

fn map_closed_reason(reason: yas_git::native::ClosedReason) -> u8 {
    use yas_wire::schema::git as native;
    match reason {
        yas_git::native::ClosedReason::ClientRequest => native::CLOSED_CLIENT_REQUEST as u8,
        yas_git::native::ClosedReason::RepositoryGone => native::CLOSED_REPOSITORY_GONE as u8,
        yas_git::native::ClosedReason::PermissionLost => native::CLOSED_PERMISSION_LOST as u8,
        yas_git::native::ClosedReason::ResourceLimit => native::CLOSED_RESOURCE_LIMIT as u8,
        yas_git::native::ClosedReason::BackendFailed => native::CLOSED_BACKEND_FAILED as u8,
    }
}

fn object_item(algorithm: u8, object: yas_git::native::Oid, role: u64) -> Result<QueryItem, Error> {
    Ok(QueryItem::Record(wire::QueryRecord::Object(
        wire::ObjectRecord {
            role: role as u8,
            object: required_oid(algorithm, object)?,
        },
    )))
}

fn engine_oid(object: &wire::ObjectId) -> Result<yas_git::native::Oid, Error> {
    let expected = match object.algorithm {
        value if value == yas_wire::schema::git::OBJECT_SHA1 as u8 => 20,
        value if value == yas_wire::schema::git::OBJECT_SHA256 as u8 => 32,
        _ => return Err(Error::invalid("unsupported Git object algorithm")),
    };
    if object.bytes.len() != expected {
        return Err(Error::invalid("invalid Git object length"));
    }
    let mut value = [0; 32];
    value[..expected].copy_from_slice(&object.bytes);
    Ok(value)
}

fn required_oid(algorithm: u8, object: yas_git::native::Oid) -> Result<wire::ObjectId, Error> {
    optional_oid(algorithm, object)?
        .ok_or_else(|| Error::new(Status::Internal, "missing Git object ID"))
}

fn wire_oid(algorithm: u8, object: yas_git::native::Oid) -> Result<wire::ObjectId, Error> {
    let length = match algorithm {
        value if value == yas_wire::schema::git::OBJECT_SHA1 as u8 => 20,
        value if value == yas_wire::schema::git::OBJECT_SHA256 as u8 => 32,
        _ => {
            return Err(Error::new(
                Status::Internal,
                "unsupported Git object algorithm",
            ));
        }
    };
    if object[length..].iter().any(|byte| *byte != 0) {
        return Err(Error::new(Status::Internal, "malformed Git object ID"));
    }
    Ok(wire::ObjectId {
        algorithm,
        bytes: object[..length].to_vec(),
    })
}

fn optional_oid(
    algorithm: u8,
    object: yas_git::native::Oid,
) -> Result<Option<wire::ObjectId>, Error> {
    if object.iter().all(|byte| *byte == 0) {
        return Ok(None);
    }
    let length = match algorithm {
        value if value == yas_wire::schema::git::OBJECT_SHA1 as u8 => 20,
        value if value == yas_wire::schema::git::OBJECT_SHA256 as u8 => 32,
        _ => {
            return Err(Error::new(
                Status::Internal,
                "unsupported Git object algorithm",
            ));
        }
    };
    if object[length..].iter().any(|byte| *byte != 0) {
        return Err(Error::new(Status::Internal, "malformed Git object ID"));
    }
    Ok(Some(wire::ObjectId {
        algorithm,
        bytes: object[..length].to_vec(),
    }))
}

fn map_log_flags(flags: u16) -> Result<u8, Error> {
    let known = yas_wire::schema::git::LOG_FLAGS as u16;
    if flags & !known != 0 {
        return Err(Error::invalid("unknown Git LOG flags"));
    }
    u8::try_from(flags).map_err(|_| Error::invalid("Git LOG flags overflow"))
}

fn map_blob_flags(flags: u16) -> Result<u8, Error> {
    let known = yas_wire::schema::git::BLOB_FLAGS as u16;
    if flags & !known != 0 {
        return Err(Error::invalid("unknown Git BLOB flags"));
    }
    u8::try_from(flags).map_err(|_| Error::invalid("Git BLOB flags overflow"))
}

fn map_u16_flags(flags: u16, known: u64, operation: &str) -> Result<u8, Error> {
    if flags & !(known as u16) != 0 {
        return Err(Error::invalid(format!("unknown Git {operation} flags")));
    }
    u8::try_from(flags).map_err(|_| Error::invalid(format!("Git {operation} flags overflow")))
}

fn native_endpoint(endpoint: &wire::QueryEndpoint) -> Result<yas_git::native::Endpoint, Error> {
    Ok(match endpoint {
        wire::QueryEndpoint::Empty => yas_git::native::Endpoint::Empty,
        wire::QueryEndpoint::Commit(object) => {
            yas_git::native::Endpoint::Commit(engine_oid(object)?)
        }
        wire::QueryEndpoint::Tree(object) => yas_git::native::Endpoint::Tree(engine_oid(object)?),
        wire::QueryEndpoint::Index => yas_git::native::Endpoint::Index,
        wire::QueryEndpoint::Worktree => yas_git::native::Endpoint::Worktree,
        wire::QueryEndpoint::MergeBase(object) => {
            yas_git::native::Endpoint::MergeBase(engine_oid(object)?)
        }
    })
}

fn endpoint_content_object(endpoint: &wire::QueryEndpoint) -> Option<wire::ObjectId> {
    match endpoint {
        wire::QueryEndpoint::Commit(object)
        | wire::QueryEndpoint::Tree(object)
        | wire::QueryEndpoint::MergeBase(object) => Some(object.clone()),
        wire::QueryEndpoint::Empty | wire::QueryEndpoint::Index | wire::QueryEndpoint::Worktree => {
            None
        }
    }
}

fn diff_status(status: u8) -> Result<u8, Error> {
    use yas_wire::schema::git as native;
    match status {
        b'A' => Ok(native::DIFF_ADDED as u8),
        b'M' | b'T' | b'U' => Ok(native::DIFF_MODIFIED as u8),
        b'D' => Ok(native::DIFF_DELETED as u8),
        b'R' => Ok(native::DIFF_RENAMED as u8),
        b'C' => Ok(native::DIFF_COPIED as u8),
        _ => Err(Error::new(Status::Internal, "unknown Git diff status")),
    }
}

fn map_diff_record_flags(binary: bool, submodule: bool, filtered: bool) -> u16 {
    use yas_wire::schema::git as native;
    let mut mapped = 0;
    if binary {
        mapped |= native::DIFF_BINARY_RECORD as u16;
    }
    if submodule {
        mapped |= native::DIFF_SUBMODULE_RECORD as u16;
    }
    if filtered {
        mapped |= native::DIFF_FILTERED_RECORD as u16;
    }
    mapped
}

fn map_patch_file_flags(binary: bool, filtered: bool) -> u16 {
    use yas_wire::schema::git as native;
    let mut mapped = 0;
    if binary {
        mapped |= native::PATCH_FILE_BINARY as u16;
    }
    if filtered {
        mapped |= native::PATCH_FILE_FILTERED as u16;
    }
    mapped
}

fn patch_spans(spans: Vec<(u32, u32)>) -> Vec<wire::PatchSpan> {
    spans
        .into_iter()
        .map(|(start, length)| wire::PatchSpan { start, length })
        .collect()
}

fn map_index_flags(stage: u8, intent_to_add: bool, skip_worktree: bool) -> u16 {
    use yas_wire::schema::git as native;
    let mut mapped = 0;
    if stage != 0 {
        mapped |= native::INDEX_CONFLICTED as u16;
    }
    if intent_to_add {
        mapped |= native::INDEX_INTENT_TO_ADD as u16;
    }
    if skip_worktree {
        mapped |= native::INDEX_SKIP_WORKTREE as u16;
    }
    mapped
}

fn map_discovery_flags(bare: bool, linked: bool, submodule: bool) -> u16 {
    use yas_wire::schema::git as native;
    let mut mapped = 0;
    if bare {
        mapped |= native::DISCOVERY_BARE as u16;
    }
    if linked {
        mapped |= native::DISCOVERY_LINKED as u16;
    }
    if submodule {
        mapped |= native::DISCOVERY_SUBMODULE as u16;
    }
    mapped
}

fn map_worktree_flags(
    bare: bool,
    main: bool,
    current: bool,
    locked: bool,
    prunable: bool,
    detached: bool,
) -> u16 {
    use yas_wire::schema::git as native;
    let mut mapped = 0;
    for (present, flag) in [
        (bare, native::WORKTREE_BARE),
        (main, native::WORKTREE_MAIN),
        (current, native::WORKTREE_CURRENT),
        (locked, native::WORKTREE_LOCKED),
        (prunable, native::WORKTREE_PRUNABLE),
        (detached, native::WORKTREE_DETACHED),
    ] {
        if present {
            mapped |= flag as u16;
        }
    }
    mapped
}

fn map_fetch_ref_flags(forced: bool, pruned: bool, new_ref: bool, tag_update: bool) -> u16 {
    use yas_wire::schema::git as native;
    let mut mapped = 0;
    if forced {
        mapped |= native::FETCH_REF_FORCED as u16;
    }
    if pruned {
        mapped |= native::FETCH_REF_PRUNED as u16;
    }
    if new_ref {
        mapped |= native::FETCH_REF_NEW as u16;
    }
    if tag_update {
        mapped |= native::FETCH_REF_TAG_UPDATE as u16;
    }
    mapped
}

fn tree_kind(kind: yas_git::native::TreeKind) -> u8 {
    use yas_wire::schema::git as native;
    match kind {
        yas_git::native::TreeKind::Blob => native::TREE_BLOB as u8,
        yas_git::native::TreeKind::Tree => native::TREE_TREE as u8,
        yas_git::native::TreeKind::Commit => native::TREE_COMMIT as u8,
    }
}

fn platform_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(Error::invalid("invalid Git platform path"));
    }
    Ok(PathBuf::from(platform_os(bytes)))
}

fn relative_path(path: &fs_wire::Path) -> Result<PathBuf, Error> {
    let mut result = PathBuf::new();
    append_path(&mut result, path)?;
    if result.as_os_str().is_empty() {
        return Err(Error::invalid("empty Git relative path"));
    }
    Ok(result)
}

fn append_path(path: &mut PathBuf, relative: &fs_wire::Path) -> Result<(), Error> {
    for component in &relative.components {
        let value = platform_os(component);
        let mut parts = Path::new(&value).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(part)), None) if part == value.as_os_str() => path.push(part),
            _ => return Err(Error::invalid("invalid Git path component")),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn platform_os(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}

#[cfg(unix)]
fn platform_path_bytes(path: &Path) -> Result<Vec<u8>, Error> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(Error::new(Status::Internal, "invalid Git platform path"));
    }
    Ok(bytes.to_vec())
}

#[cfg(windows)]
fn platform_os(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}

#[cfg(windows)]
fn platform_path_bytes(path: &Path) -> Result<Vec<u8>, Error> {
    let value = path.to_string_lossy().into_owned().into_bytes();
    if value.is_empty() || value.contains(&0) {
        return Err(Error::new(Status::Internal, "invalid Git platform path"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(path: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .current_dir(path)
            .args(args)
            .env("GIT_AUTHOR_NAME", "YAS Test")
            .env("GIT_AUTHOR_EMAIL", "yas@example.invalid")
            .env("GIT_COMMITTER_NAME", "YAS Test")
            .env("GIT_COMMITTER_EMAIL", "yas@example.invalid")
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn query(body: wire::QueryBody) -> wire::Query {
        wire::Query {
            repository_handle: 1,
            max_records: 128,
            cursor: wire::QueryCursor::Start,
            initial_receive_credit: 1024 * 1024,
            body,
            extensions: Extensions::default(),
        }
    }

    #[test]
    fn component_paths_are_raw_and_traversal_safe() {
        let path = fs_wire::Path {
            components: vec![b"src".to_vec(), b"odd%name".to_vec()],
        };
        assert_eq!(relative_path(&path).unwrap(), PathBuf::from("src/odd%name"));
        for invalid in [b"..".as_slice(), b"a/b".as_slice(), b"".as_slice()] {
            assert!(
                relative_path(&fs_wire::Path {
                    components: vec![invalid.to_vec()],
                })
                .is_err()
            );
        }
    }

    #[test]
    fn typed_queries_cover_repository_reads_without_exposing_legacy_packets() {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), &["init", "-q"]);
        std::fs::write(directory.path().join("hello.txt"), b"hello\n").unwrap();
        git(directory.path(), &["add", "hello.txt"]);
        git(directory.path(), &["commit", "-qm", "initial"]);

        let (repository, info) = yas_git::native::open_path(directory.path()).unwrap();
        let algorithm = match info.object_format {
            yas_git::native::ObjectFormat::Sha1 => yas_wire::schema::git::OBJECT_SHA1 as u8,
            yas_git::native::ObjectFormat::Sha256 => yas_wire::schema::git::OBJECT_SHA256 as u8,
        };
        let cancel = yas_git::Cancel::default();

        let log = run_query(
            Some(&repository),
            algorithm,
            &query(wire::QueryBody::Log {
                spec: b"HEAD".to_vec(),
                tips: Vec::new(),
                hides: Vec::new(),
                path: None,
                flags: 0,
            }),
            None,
            &cancel,
        )
        .unwrap();
        let commit = log
            .records
            .iter()
            .find_map(|record| match record {
                QueryItem::Record(wire::QueryRecord::Commit(commit)) => Some(commit.clone()),
                _ => None,
            })
            .expect("commit record");

        let tree = run_query(
            Some(&repository),
            algorithm,
            &query(wire::QueryBody::Tree {
                tree: commit.tree,
                path: fs_wire::Path {
                    components: Vec::new(),
                },
            }),
            None,
            &cancel,
        )
        .unwrap();
        let blob = tree
            .records
            .iter()
            .find_map(|record| match record {
                QueryItem::Record(wire::QueryRecord::TreeEntry(entry))
                    if entry.name == b"hello.txt" =>
                {
                    Some(entry.object.clone())
                }
                _ => None,
            })
            .expect("tree entry");
        let blob_page = run_query(
            Some(&repository),
            algorithm,
            &query(wire::QueryBody::Blob {
                object: blob,
                path: None,
                offset: 0,
                max_bytes: 1024,
                flags: 0,
            }),
            None,
            &cancel,
        )
        .unwrap();
        assert!(matches!(
            blob_page.records.as_slice(),
            [QueryItem::Content(OwnedContent { bytes, .. })] if bytes == b"hello\n"
        ));

        std::fs::write(directory.path().join("hello.txt"), b"hello native YAS\n").unwrap();
        let patch = run_query(
            Some(&repository),
            algorithm,
            &query(wire::QueryBody::Patch {
                left: wire::QueryEndpoint::Commit(commit.object.clone()),
                right: wire::QueryEndpoint::Worktree,
                path: None,
                context_lines: 3,
                rename_threshold: 50,
                max_bytes: 1024 * 1024,
                flags: 0,
            }),
            None,
            &cancel,
        )
        .unwrap();
        assert!(
            patch
                .records
                .iter()
                .any(|record| matches!(record, QueryItem::Record(wire::QueryRecord::PatchRow(_))))
        );

        for body in [
            wire::QueryBody::Index {
                path: None,
                flags: yas_wire::schema::git::INDEX_STAGED as u16,
            },
            wire::QueryBody::Blame {
                object: commit.object.clone(),
                path: fs_wire::Path {
                    components: vec![b"hello.txt".to_vec()],
                },
                start_line: 1,
                line_count: 1,
                flags: 0,
            },
            wire::QueryBody::Reflog {
                name: b"HEAD".to_vec(),
                flags: 0,
            },
            wire::QueryBody::Worktrees,
        ] {
            let page =
                run_query(Some(&repository), algorithm, &query(body), None, &cancel).unwrap();
            assert!(!page.records.is_empty());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn watched_non_log_query_re_evaluates_after_acknowledged_state_change() {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), &["init", "-q"]);
        std::fs::write(directory.path().join("hello.txt"), b"hello\n").unwrap();
        git(directory.path(), &["add", "hello.txt"]);
        git(directory.path(), &["commit", "-qm", "initial"]);

        let (repository, info) = yas_git::native::open_path(directory.path()).unwrap();
        let object_algorithm = match info.object_format {
            yas_git::native::ObjectFormat::Sha1 => yas_wire::schema::git::OBJECT_SHA1 as u8,
            yas_git::native::ObjectFormat::Sha256 => yas_wire::schema::git::OBJECT_SHA256 as u8,
        };
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let outbox: yas_git::native::StateSink =
            Box::new(move |event| sender.try_send(event).is_ok());
        let handle = repository.start_native_state(
            yas_git::StateOptions {
                wants_state: true,
                status: true,
                untracked: true,
                ignored: true,
                tracking: true,
                remotes: true,
                ..Default::default()
            },
            outbox,
        );
        let request = query(wire::QueryBody::Index {
            path: None,
            flags: yas_wire::schema::git::INDEX_STAGED as u16,
        });
        let mut watch = QueryWatch {
            object_algorithm,
            repository,
            request,
            discover_path: None,
            handle,
            receiver,
        };
        let update = tokio::time::timeout(std::time::Duration::from_secs(5), watch.next())
            .await
            .expect("watched log timeout")
            .expect("watched log closed");
        assert!(update.update_id > 0);
        let page = update.result.expect("watched log result");
        assert!(
            page.records.iter().any(|record| matches!(
                record,
                QueryItem::Record(wire::QueryRecord::IndexEntry(_))
            ))
        );
        watch.acknowledge(update.update_id);

        std::fs::write(directory.path().join("hello.txt"), b"changed\n").unwrap();
        git(directory.path(), &["add", "hello.txt"]);
        let replacement = tokio::time::timeout(std::time::Duration::from_secs(5), watch.next())
            .await
            .expect("watched index replacement timeout")
            .expect("watched index closed");
        assert!(replacement.update_id > update.update_id);
        assert!(replacement.result.is_ok());
        watch.acknowledge(replacement.update_id);
        watch.stop();
    }

    #[test]
    fn state_snapshot_maps_every_native_dataset() {
        let oid = {
            let mut value = [0; 32];
            value[..20].fill(7);
            value
        };
        let records = vec![
            yas_git::native::StateRecord::Head {
                detached: false,
                unborn: false,
                object: oid,
                symbolic_target: b"refs/heads/main".to_vec(),
            },
            yas_git::native::StateRecord::Ref {
                peeled_valid: false,
                symbolic: false,
                object: oid,
                peeled: [0; 32],
                name: b"refs/heads/main".to_vec(),
                symbolic_target: Vec::new(),
            },
            yas_git::native::StateRecord::Operation {
                kind: 4,
                head: oid,
                detail: "onto main".to_owned(),
            },
            yas_git::native::StateRecord::Status {
                index_status: b'M',
                worktree_status: b' ',
                conflicted: false,
                object: oid,
                old_path: None,
                path: vec![b"hello.txt".to_vec()],
            },
            yas_git::native::StateRecord::Upstream {
                gone: false,
                counts_valid: true,
                ahead: 1,
                behind: 2,
                name: b"refs/heads/main".to_vec(),
                upstream: b"refs/remotes/origin/main".to_vec(),
            },
            yas_git::native::StateRecord::Stash {
                index: 0,
                object: oid,
                created_unix_seconds: 123,
                timezone_minutes: 60,
                message: b"wip".to_vec(),
            },
            yas_git::native::StateRecord::Remote {
                default: true,
                name: b"origin".to_vec(),
                fetch_url: b"ssh://example.invalid/repo".to_vec(),
                push_url: Vec::new(),
            },
            yas_git::native::StateRecord::WorktreeGeneration {
                count: 1,
                digest: 2,
            },
        ];
        let mapped = convert_state_snapshot(
            yas_wire::schema::git::WATCH_DATASETS as u16,
            yas_wire::schema::git::OBJECT_SHA1 as u8,
            7,
            records,
        )
        .unwrap();
        assert_eq!(mapped.len(), 8);
        assert!(mapped.iter().all(|record| record.revision == 7));
    }

    #[cfg(unix)]
    #[test]
    fn native_platform_paths_round_trip_non_utf8() {
        let bytes = b"/tmp/git-\xff";
        let path = platform_path(bytes).unwrap();
        assert_eq!(platform_path_bytes(&path).unwrap(), bytes);
    }
}

#[cfg(all(not(unix), not(windows)))]
fn platform_os(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}
