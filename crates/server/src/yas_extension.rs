//! Native YAS semantics over the process-global extension supervisor.
//!
//! The existing extension service remains the sole owner of objects,
//! definitions, attempts, output retention, and command discovery. This module
//! gives `yas` owned typed values and intentionally keeps request IDs and
//! Transfer descriptors out of that service boundary.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use yas_wire::codec::Extensions;
use yas_wire::core::RuntimeState;
use yas_wire::extension as wire;
use yas_wire::schema;
use yas_wire::{Decode, Encode};

use super::AppState;
use super::extension::{
    ExtensionService, NativeControlAction, NativeDefinition, NativeEndpoint, NativeFollowItem,
    NativeFollowStream, NativeMutationFailure, NativeMutationReplay, NativeMutationSettlement,
    NativeOutputKind, NativePutDisposition, NativeRunOptions,
};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const PUT_CHUNK_BYTES: usize = 64 * 1024;
const FOLLOW_QUEUE: usize = 64;

#[derive(Clone)]
pub(crate) struct Runtime {
    app: AppState,
    service: Arc<ExtensionService>,
    catalogue: Arc<StdMutex<CatalogueRevision>>,
}

#[derive(Default)]
struct CatalogueRevision {
    fingerprint: [u8; 32],
    revision: u64,
}

impl CatalogueRevision {
    fn observe(&mut self, fingerprint: [u8; 32]) -> Result<u64, Error> {
        if self.revision == 0 || self.fingerprint != fingerprint {
            self.revision = self.revision.checked_add(1).ok_or_else(|| {
                Error::Internal("Extension catalogue revision exhausted".to_owned())
            })?;
            self.fingerprint = fingerprint;
        }
        Ok(self.revision)
    }
}

#[derive(Clone)]
pub(crate) struct Session {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    app: AppState,
    service: Arc<ExtensionService>,
    backend: NativeEndpoint,
    replay_capacity: usize,
    next_stage: AtomicU64,
    stages: StdMutex<HashMap<u64, Stage>>,
    begin_replay: StdMutex<ReplayCache<ObjectStage>>,
    commit_replay: StdMutex<ReplayCache<()>>,
    deploy_replay: StdMutex<ReplayCache<wire::DefinitionIdentity>>,
    control_replay: StdMutex<ReplayCache<wire::DefinitionIdentity>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) revision: u64,
    pub(crate) records: Vec<wire::ExtensionRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ObjectStage {
    pub(crate) disposition: wire::ObjectDisposition,
    pub(crate) staging_handle: u64,
}

pub(crate) struct FollowStream {
    pub(crate) attempt: u64,
    pub(crate) first_sequence: u64,
    pub(crate) through_sequence: u64,
    pending: VecDeque<FollowItem>,
    backend: NativeFollowStream,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FollowItem {
    Batch(wire::OutputBatch),
    Complete { through_sequence: u64 },
}

impl FollowStream {
    pub(crate) async fn next(&mut self) -> Option<FollowItem> {
        if let Some(item) = self.pending.pop_front() {
            return Some(item);
        }
        match self.backend.next().await? {
            NativeFollowItem::Output {
                kind,
                sequence,
                data,
            } => Some(FollowItem::Batch(wire::OutputBatch {
                first_sequence: sequence,
                records: vec![wire::OutputRecord {
                    kind: match kind {
                        NativeOutputKind::Stdout => wire::OutputKind::Stdout,
                        NativeOutputKind::Stderr => wire::OutputKind::Stderr,
                        NativeOutputKind::Log => wire::OutputKind::Log,
                    },
                    sequence,
                    server_ns: 0,
                    data,
                }],
            })),
            NativeFollowItem::Complete { through_sequence } => {
                Some(FollowItem::Complete { through_sequence })
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Unavailable,
    NotFound,
    Permission,
    Conflict,
    Stale,
    ResourceExhausted,
    TooLarge,
    Invalid(String),
    Unsupported(String),
    Cancelled,
    Closed,
    Internal(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("Extension family is unavailable"),
            Self::NotFound => formatter.write_str("extension resource not found"),
            Self::Permission => formatter.write_str("extension operation is not permitted"),
            Self::Conflict => formatter.write_str("extension state conflicts with the request"),
            Self::Stale => formatter.write_str("extension operation resource is stale"),
            Self::ResourceExhausted => formatter.write_str("extension resource limit reached"),
            Self::TooLarge => formatter.write_str("extension request is too large"),
            Self::Invalid(detail) | Self::Unsupported(detail) | Self::Internal(detail) => {
                formatter.write_str(detail)
            }
            Self::Cancelled => formatter.write_str("extension operation was cancelled"),
            Self::Closed => formatter.write_str("extension session is closed"),
        }
    }
}

impl From<&Error> for NativeMutationFailure {
    fn from(error: &Error) -> Self {
        match error {
            Error::Unavailable => Self::Unavailable,
            Error::NotFound => Self::NotFound,
            Error::Permission => Self::Permission,
            Error::Conflict | Error::Stale => Self::Conflict,
            Error::ResourceExhausted => Self::ResourceExhausted,
            Error::TooLarge => Self::TooLarge,
            Error::Invalid(detail) => Self::Invalid(detail.clone()),
            Error::Unsupported(detail) => Self::Unsupported(detail.clone()),
            Error::Cancelled => Self::Cancelled,
            Error::Closed => Self::Closed,
            Error::Internal(detail) => Self::Internal(detail.clone()),
        }
    }
}

impl From<NativeMutationFailure> for Error {
    fn from(error: NativeMutationFailure) -> Self {
        match error {
            NativeMutationFailure::Unavailable => Self::Unavailable,
            NativeMutationFailure::NotFound => Self::NotFound,
            NativeMutationFailure::Permission => Self::Permission,
            NativeMutationFailure::Conflict => Self::Conflict,
            NativeMutationFailure::ResourceExhausted => Self::ResourceExhausted,
            NativeMutationFailure::TooLarge => Self::TooLarge,
            NativeMutationFailure::Invalid(detail) => Self::Invalid(detail),
            NativeMutationFailure::Unsupported(detail) => Self::Unsupported(detail),
            NativeMutationFailure::Cancelled => Self::Cancelled,
            NativeMutationFailure::Closed => Self::Closed,
            NativeMutationFailure::Internal(detail) => Self::Internal(detail),
        }
    }
}

#[derive(Clone)]
struct Stage {
    hash: [u8; 32],
    byte_len: u64,
    replayable: bool,
}

#[derive(Clone)]
struct ReplayEntry<T> {
    fingerprint: [u8; 32],
    value: Result<T, Error>,
}

struct ReplayCache<T> {
    values: HashMap<[u8; 16], ReplayEntry<T>>,
    order: VecDeque<[u8; 16]>,
}

impl<T> Default for ReplayCache<T> {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl<T: Clone> ReplayCache<T> {
    fn lookup(&self, operation_id: &[u8; 16], fingerprint: &[u8; 32]) -> Option<Result<T, Error>> {
        self.values.get(operation_id).map(|entry| {
            if &entry.fingerprint == fingerprint {
                entry.value.clone()
            } else {
                Err(Error::Conflict)
            }
        })
    }

    fn make_room(
        &mut self,
        capacity: usize,
        mut is_live: impl FnMut(&ReplayEntry<T>) -> bool,
    ) -> bool {
        while self.values.len() >= capacity {
            let Some(index) = self.order.iter().position(|operation_id| {
                self.values
                    .get(operation_id)
                    .is_some_and(|entry| !is_live(entry))
            }) else {
                return false;
            };
            let operation_id = self
                .order
                .remove(index)
                .expect("Extension replay order index remains valid");
            self.values.remove(&operation_id);
        }
        true
    }

    fn insert_with_liveness(
        &mut self,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
        value: Result<T, Error>,
        capacity: usize,
        is_live: impl FnMut(&ReplayEntry<T>) -> bool,
    ) -> bool {
        if !self.values.contains_key(&operation_id) {
            if !self.make_room(capacity, is_live) {
                return false;
            }
            self.order.push_back(operation_id);
        }
        self.values
            .insert(operation_id, ReplayEntry { fingerprint, value });
        true
    }

    fn insert(
        &mut self,
        operation_id: [u8; 16],
        fingerprint: [u8; 32],
        value: Result<T, Error>,
        capacity: usize,
    ) {
        let inserted =
            self.insert_with_liveness(operation_id, fingerprint, value, capacity, |_| false);
        debug_assert!(inserted, "non-pinned Extension replay must be admitted");
    }
}

impl Runtime {
    pub(crate) fn new(app: AppState) -> Self {
        Self {
            service: app.extensions.clone(),
            app,
            catalogue: Arc::new(StdMutex::new(CatalogueRevision::default())),
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        let limits = self.service.native_family_limits();
        self.service.advertised()
            && limits.max_definitions != 0
            && limits.max_follows_per_session != 0
            && limits.max_running_attempts != 0
    }

    pub(crate) fn runtime_state(&self) -> RuntimeState {
        if self.enabled() {
            RuntimeState::Available
        } else {
            RuntimeState::Unavailable
        }
    }

    pub(crate) fn limits(&self) -> wire::Limits {
        let configured = self.service.native_family_limits();
        let mut limits = wire::Limits::HARD;
        limits.max_definitions = u32::try_from(configured.max_definitions)
            .unwrap_or(u32::MAX)
            .min(limits.max_definitions);
        limits.max_follows_per_session = u32::try_from(configured.max_follows_per_session)
            .unwrap_or(u32::MAX)
            .min(limits.max_follows_per_session);
        limits.max_running_attempts = u32::try_from(configured.max_running_attempts)
            .unwrap_or(u32::MAX)
            .min(limits.max_running_attempts);
        limits.max_mutation_replays = u32::try_from(configured.max_mutation_replays)
            .unwrap_or(u32::MAX)
            .min(limits.max_mutation_replays);
        limits
    }

    pub(crate) async fn session(&self, owner_session: [u8; 16]) -> Result<Session, Error> {
        if !self.enabled() {
            return Err(Error::Unavailable);
        }
        if owner_session == [0; 16] {
            return Err(Error::Invalid("zero Extension owner session".to_owned()));
        }
        let backend = self
            .service
            .open_native_endpoint(self.app.clone())
            .await
            .map_err(Error::from)?;
        let inner = Arc::new(SessionInner {
            app: self.app.clone(),
            service: self.service.clone(),
            backend,
            replay_capacity: self.limits().max_mutation_replays as usize,
            next_stage: AtomicU64::new(1),
            stages: StdMutex::new(HashMap::new()),
            begin_replay: StdMutex::new(ReplayCache::default()),
            commit_replay: StdMutex::new(ReplayCache::default()),
            deploy_replay: StdMutex::new(ReplayCache::default()),
            control_replay: StdMutex::new(ReplayCache::default()),
        });
        Ok(Session { inner })
    }

    pub(crate) async fn snapshot(&self, _now_server_ns: u64) -> Result<Snapshot, Error> {
        if !self.enabled() {
            return Err(Error::Unavailable);
        }
        let defaults = self.service.native_runtime_limits();
        let records = self
            .service
            .native_snapshot()
            .await
            .into_iter()
            .map(|definition| definition_record(definition, defaults))
            .collect::<Result<Vec<_>, _>>()?;
        let fingerprint = snapshot_fingerprint(&records)?;
        let revision = {
            let mut catalogue = self
                .catalogue
                .lock()
                .expect("Extension catalogue revision lock");
            catalogue.observe(fingerprint)?
        };
        Ok(Snapshot { revision, records })
    }

    pub(crate) async fn changed(
        &self,
        revision: u64,
        now_server_ns: impl Fn() -> u64,
    ) -> Result<Snapshot, Error> {
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            let snapshot = self.snapshot(now_server_ns()).await?;
            if snapshot.revision != revision {
                return Ok(snapshot);
            }
        }
    }
}

impl Session {
    pub(crate) async fn object_begin(
        &self,
        request: &wire::ObjectBegin,
    ) -> Result<ObjectStage, Error> {
        self.ensure_open()?;
        let fingerprint = fingerprint(request)?;
        let replay = self
            .inner
            .begin_replay
            .lock()
            .expect("Extension begin replay lock")
            .lookup(&request.operation_id, &fingerprint);
        if let Some(replay) = replay {
            return match replay {
                Ok(stage)
                    if stage.disposition == wire::ObjectDisposition::Upload
                        && self
                            .inner
                            .stages
                            .lock()
                            .expect("Extension stage lock")
                            .get(&stage.staging_handle)
                            .is_none_or(|stage| !stage.replayable) =>
                {
                    Err(Error::Stale)
                }
                replay => replay,
            };
        }
        {
            let stages = self.inner.stages.lock().expect("Extension stage lock");
            if stages.len() >= wire::Limits::HARD.max_object_stages_per_session as usize {
                return Err(Error::ResourceExhausted);
            }
            let mut replay = self
                .inner
                .begin_replay
                .lock()
                .expect("Extension begin replay lock");
            if !replay.make_room(self.inner.replay_capacity, |entry| {
                matches!(
                    &entry.value,
                    Ok(ObjectStage {
                        disposition: wire::ObjectDisposition::Upload,
                        staging_handle,
                    }) if stages.get(staging_handle).is_some_and(|stage| stage.replayable)
                )
            }) {
                return Err(Error::ResourceExhausted);
            }
        }
        let result = self.begin_object_inner(request).await;
        let inserted = {
            let stages = self.inner.stages.lock().expect("Extension stage lock");
            self.inner
                .begin_replay
                .lock()
                .expect("Extension begin replay lock")
                .insert_with_liveness(
                    request.operation_id,
                    fingerprint,
                    result.clone(),
                    self.inner.replay_capacity,
                    |entry| {
                        matches!(
                            &entry.value,
                            Ok(ObjectStage {
                                disposition: wire::ObjectDisposition::Upload,
                                staging_handle,
                            }) if stages.get(staging_handle).is_some_and(|stage| stage.replayable)
                        )
                    },
                )
        };
        if !inserted {
            if let Ok(ObjectStage {
                disposition: wire::ObjectDisposition::Upload,
                staging_handle,
            }) = result
            {
                self.inner
                    .stages
                    .lock()
                    .expect("Extension stage lock")
                    .remove(&staging_handle);
            }
            return Err(Error::ResourceExhausted);
        }
        result
    }

    async fn begin_object_inner(&self, request: &wire::ObjectBegin) -> Result<ObjectStage, Error> {
        match self
            .inner
            .backend
            .put(request.content_hash, 0, request.byte_len, &[], true, false)
            .await
            .map_err(Error::from)?
        {
            NativePutDisposition::AlreadyPresent { size } if size == request.byte_len => {
                Ok(ObjectStage {
                    disposition: wire::ObjectDisposition::AlreadyPresent,
                    staging_handle: 0,
                })
            }
            NativePutDisposition::AlreadyPresent { .. } => Err(Error::Conflict),
            NativePutDisposition::Accepted { received: 0 } => {
                let staging_handle = self.allocate_stage()?;
                self.inner
                    .stages
                    .lock()
                    .expect("Extension stage lock")
                    .insert(
                        staging_handle,
                        Stage {
                            hash: request.content_hash,
                            byte_len: request.byte_len,
                            replayable: true,
                        },
                    );
                Ok(ObjectStage {
                    disposition: wire::ObjectDisposition::Upload,
                    staging_handle,
                })
            }
            NativePutDisposition::Accepted { .. } => Err(Error::Internal(
                "Extension object begin advanced the upload cursor".to_owned(),
            )),
        }
    }

    /// Abort an uncommitted OBJECT_BEGIN stage. The extension service owns the
    /// object-store reservation, which independently expires on its short
    /// upload timeout; removing the YAS handle immediately prevents COMMIT
    /// and releases this session's staging budget.
    pub(crate) fn object_reset(&self, staging_handle: u64) -> bool {
        self.inner
            .stages
            .lock()
            .expect("Extension stage lock")
            .remove(&staging_handle)
            .is_some()
    }

    /// Mark the returned receiver-to-sender Transfer terminal without
    /// discarding the sealed stage needed by OBJECT_COMMIT.
    pub(crate) fn object_seal(&self, staging_handle: u64) -> bool {
        let mut stages = self.inner.stages.lock().expect("Extension stage lock");
        let Some(stage) = stages.get_mut(&staging_handle) else {
            return false;
        };
        stage.replayable = false;
        true
    }

    /// Retire a stage which could not be published to the client and replace
    /// its provisional success with the failure that the client observed.
    /// A later identical retry therefore remains deterministic, while a
    /// different payload cannot reuse the operation ID inside the horizon.
    pub(crate) fn object_reject_unpublished(
        &self,
        operation_id: [u8; 16],
        staging_handle: u64,
        error: Error,
    ) -> bool {
        let removed = self
            .inner
            .stages
            .lock()
            .expect("Extension stage lock")
            .remove(&staging_handle)
            .is_some();
        let mut replay = self
            .inner
            .begin_replay
            .lock()
            .expect("Extension begin replay lock");
        let Some(entry) = replay.values.get_mut(&operation_id) else {
            return false;
        };
        if !matches!(
            &entry.value,
            Ok(ObjectStage {
                disposition: wire::ObjectDisposition::Upload,
                staging_handle: replay_stage,
            }) if *replay_stage == staging_handle
        ) {
            return false;
        }
        entry.value = Err(error);
        removed
    }

    pub(crate) async fn object_commit(
        &self,
        request: &wire::ObjectCommit,
        bytes: Vec<u8>,
    ) -> Result<(), Error> {
        self.ensure_open()?;
        let encoded = request
            .encode()
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let fingerprint = *blake3::hash(&encoded).as_bytes();
        if let Some(replay) = self
            .inner
            .commit_replay
            .lock()
            .expect("Extension commit replay lock")
            .lookup(&request.operation_id, &fingerprint)
        {
            return replay;
        }
        let result = self.commit_object_inner(request, bytes).await;
        self.inner
            .commit_replay
            .lock()
            .expect("Extension commit replay lock")
            .insert(
                request.operation_id,
                fingerprint,
                result.clone(),
                self.inner.replay_capacity,
            );
        result
    }

    async fn commit_object_inner(
        &self,
        request: &wire::ObjectCommit,
        bytes: Vec<u8>,
    ) -> Result<(), Error> {
        let stage = self
            .inner
            .stages
            .lock()
            .expect("Extension stage lock")
            .get(&request.staging_handle)
            .cloned()
            .ok_or(Error::NotFound)?;
        if stage.hash != request.content_hash
            || stage.byte_len != request.byte_len
            || bytes.len() as u64 != request.byte_len
            || blake3::hash(&bytes).as_bytes() != &request.content_hash
        {
            return Err(Error::Conflict);
        }
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end = (offset + PUT_CHUNK_BYTES).min(bytes.len());
            let final_chunk = end == bytes.len();
            self.inner
                .backend
                .put(
                    stage.hash,
                    offset as u64,
                    stage.byte_len,
                    &bytes[offset..end],
                    false,
                    final_chunk,
                )
                .await
                .map_err(Error::from)?;
            offset = end;
        }
        self.inner
            .stages
            .lock()
            .expect("Extension stage lock")
            .remove(&request.staging_handle);
        Ok(())
    }

    pub(crate) async fn deploy(
        &self,
        request: &wire::Deploy,
    ) -> Result<wire::DefinitionIdentity, Error> {
        self.ensure_open()?;
        let fingerprint = fingerprint(request)?;
        if let Some(replay) = self
            .inner
            .deploy_replay
            .lock()
            .expect("Extension deploy replay lock")
            .lookup(&request.operation_id, &fingerprint)
        {
            return replay;
        }
        let _mutation = self.inner.service.lock_native_mutation().await;
        match self
            .inner
            .service
            .native_mutation_replay(
                wire::request_kind::DEPLOY,
                request.operation_id,
                fingerprint,
            )
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            NativeMutationReplay::Miss => {}
            NativeMutationReplay::Conflict => return Err(Error::Conflict),
            NativeMutationReplay::Hit(NativeMutationSettlement::Success(body)) => {
                return wire::DefinitionIdentity::decode(&body).map_err(|_| {
                    Error::Internal("invalid durable Extension DEPLOY replay".into())
                });
            }
            NativeMutationReplay::Hit(NativeMutationSettlement::Failure(error)) => {
                return Err(error.into());
            }
        }
        let mut result = self.deploy_inner(request).await;
        if let Ok(identity) = &result {
            let recorded = self
                .inner
                .service
                .record_native_mutation(
                    wire::request_kind::DEPLOY,
                    request.operation_id,
                    fingerprint,
                    identity
                        .encode()
                        .map_err(|_| Error::Internal("invalid Extension DEPLOY identity".into()))?,
                    request.flags & schema::extension::DEFINITION_PERSISTENT as u16 != 0,
                )
                .await;
            if let Err(error) = recorded {
                let failure = Error::Internal(error.to_string());
                self.inner.service.record_native_mutation_failure(
                    wire::request_kind::DEPLOY,
                    request.operation_id,
                    fingerprint,
                    NativeMutationFailure::from(&failure),
                );
                result = Err(failure);
            }
        } else if let Err(error) = &result {
            self.inner.service.record_native_mutation_failure(
                wire::request_kind::DEPLOY,
                request.operation_id,
                fingerprint,
                NativeMutationFailure::from(error),
            );
        }
        self.inner
            .deploy_replay
            .lock()
            .expect("Extension deploy replay lock")
            .insert(
                request.operation_id,
                fingerprint,
                result.clone(),
                self.inner.replay_capacity,
            );
        result
    }

    async fn deploy_inner(
        &self,
        request: &wire::Deploy,
    ) -> Result<wire::DefinitionIdentity, Error> {
        validate_runtime_limits(&self.inner.service, &request.runtime_limits)?;
        if request.runtime != wire::Runtime::Auto
            && self
                .inner
                .service
                .native_object_runtime(request.content_hash)
                .await
                .is_some_and(|runtime| runtime != request.runtime as u8)
        {
            return Err(Error::Invalid(
                "Extension object does not match the requested runtime".to_owned(),
            ));
        }
        let persistent = request.flags & schema::extension::DEFINITION_PERSISTENT as u16 != 0;
        if persistent && !self.inner.service.native_persist_allowed() {
            return Err(Error::Permission);
        }

        let reply = self
            .inner
            .backend
            .run(
                request.restart_policy as u8,
                request.expected_extension_handle,
                request.expected_generation,
                request.expected_definition_revision,
                request.content_hash,
                &request.name,
                request.argv.clone(),
                NativeRunOptions {
                    flags: u8::try_from(request.flags).map_err(|_| {
                        Error::Invalid("invalid Extension definition flags".to_owned())
                    })?,
                    runtime: request.runtime as u8,
                    follow_creator: false,
                },
            )
            .await
            .map_err(Error::from)?;
        if reply.extension_handle == 0 || reply.definition_revision == 0 {
            return Err(Error::Internal(
                "Extension deploy omitted identity".to_owned(),
            ));
        }
        let current = self
            .inner
            .service
            .native_snapshot()
            .await
            .into_iter()
            .find(|definition| definition.extension_handle == reply.extension_handle)
            .ok_or(Error::NotFound)?;
        Ok(identity(&current))
    }

    pub(crate) async fn control(
        &self,
        request: &wire::Control,
    ) -> Result<wire::DefinitionIdentity, Error> {
        self.ensure_open()?;
        let fingerprint = fingerprint(request)?;
        if let Some(replay) = self
            .inner
            .control_replay
            .lock()
            .expect("Extension control replay lock")
            .lookup(&request.operation_id, &fingerprint)
        {
            return replay;
        }
        let _mutation = self.inner.service.lock_native_mutation().await;
        match self
            .inner
            .service
            .native_mutation_replay(
                wire::request_kind::CONTROL,
                request.operation_id,
                fingerprint,
            )
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            NativeMutationReplay::Miss => {}
            NativeMutationReplay::Conflict => return Err(Error::Conflict),
            NativeMutationReplay::Hit(NativeMutationSettlement::Success(body)) => {
                return wire::DefinitionIdentity::decode(&body).map_err(|_| {
                    Error::Internal("invalid durable Extension CONTROL replay".into())
                });
            }
            NativeMutationReplay::Hit(NativeMutationSettlement::Failure(error)) => {
                return Err(error.into());
            }
        }
        let persistent = self
            .inner
            .service
            .native_snapshot()
            .await
            .into_iter()
            .find(|definition| definition.extension_handle == request.extension_handle)
            .is_some_and(|definition| {
                definition.flags & schema::extension::DEFINITION_PERSISTENT as u8 != 0
            });
        let mut result = self.control_inner(request).await;
        if let Ok(identity) = &result {
            let recorded = self
                .inner
                .service
                .record_native_mutation(
                    wire::request_kind::CONTROL,
                    request.operation_id,
                    fingerprint,
                    identity.encode().map_err(|_| {
                        Error::Internal("invalid Extension CONTROL identity".into())
                    })?,
                    persistent,
                )
                .await;
            if let Err(error) = recorded {
                let failure = Error::Internal(error.to_string());
                self.inner.service.record_native_mutation_failure(
                    wire::request_kind::CONTROL,
                    request.operation_id,
                    fingerprint,
                    NativeMutationFailure::from(&failure),
                );
                result = Err(failure);
            }
        } else if let Err(error) = &result {
            self.inner.service.record_native_mutation_failure(
                wire::request_kind::CONTROL,
                request.operation_id,
                fingerprint,
                NativeMutationFailure::from(error),
            );
        }
        self.inner
            .control_replay
            .lock()
            .expect("Extension control replay lock")
            .insert(
                request.operation_id,
                fingerprint,
                result.clone(),
                self.inner.replay_capacity,
            );
        result
    }

    async fn control_inner(
        &self,
        request: &wire::Control,
    ) -> Result<wire::DefinitionIdentity, Error> {
        let current = self
            .inner
            .service
            .native_snapshot()
            .await
            .into_iter()
            .find(|definition| definition.extension_handle == request.extension_handle)
            .ok_or(Error::NotFound)?;
        if current.generation != request.generation
            || request.expected_definition_revision != 0
                && current.definition_revision != request.expected_definition_revision
        {
            return Err(Error::Conflict);
        }
        if matches!(
            request.action,
            wire::ControlAction::Disable | wire::ControlAction::Enable
        ) {
            // YAS definition flags require DESIRED_RUNNING => ENABLED. The
            // extension supervisor tracks those bits independently, so clear
            // desired before either edge of the enabled transition; both
            // externally observable states remain valid.
            self.inner
                .backend
                .control(request.extension_handle, NativeControlAction::Stop)
                .await?;
        }
        let action = match request.action {
            wire::ControlAction::Stop => NativeControlAction::Stop,
            wire::ControlAction::Start | wire::ControlAction::Restart => {
                NativeControlAction::Restart
            }
            wire::ControlAction::Enable => NativeControlAction::Enable,
            wire::ControlAction::Disable => NativeControlAction::Disable,
            wire::ControlAction::Remove => NativeControlAction::Remove,
        };
        let reply = self
            .inner
            .backend
            .control(request.extension_handle, action)
            .await?;
        if request.action == wire::ControlAction::Remove {
            return Ok(wire::DefinitionIdentity {
                extension_handle: request.extension_handle,
                generation: request.generation,
                definition_revision: current.definition_revision,
                extensions: Extensions::default(),
            });
        }
        let updated = self
            .inner
            .service
            .native_snapshot()
            .await
            .into_iter()
            .find(|definition| definition.extension_handle == request.extension_handle)
            .ok_or(Error::NotFound)?;
        if reply.definition_revision != 0
            && reply.definition_revision != updated.definition_revision
        {
            return Err(Error::Conflict);
        }
        Ok(identity(&updated))
    }

    pub(crate) async fn follow(&self, request: &wire::Follow) -> Result<FollowStream, Error> {
        self.ensure_open()?;
        let current = self
            .inner
            .service
            .native_snapshot()
            .await
            .into_iter()
            .find(|definition| definition.extension_handle == request.extension_handle)
            .ok_or(Error::NotFound)?;
        if current.generation != request.generation {
            return Err(Error::Conflict);
        }
        let attempt = if request.attempt == 0 {
            current.attempt.max(current.last_running_attempt)
        } else {
            request.attempt
        };
        if attempt == 0 || attempt > current.attempt {
            return Err(Error::NotFound);
        }
        let requested_sequence = if request.from_sequence == 0 {
            current.oldest_output_sequence
        } else {
            request.from_sequence
        };
        if requested_sequence > current.output_sequence.saturating_add(1) {
            return Err(Error::Invalid(
                "Extension follow sequence is beyond retained output".to_owned(),
            ));
        }
        let emitted_gap = requested_sequence < current.oldest_output_sequence;
        let mut pending = VecDeque::new();
        if emitted_gap {
            pending.push_back(FollowItem::Batch(wire::OutputBatch {
                first_sequence: requested_sequence,
                records: vec![wire::OutputRecord {
                    kind: wire::OutputKind::Gap,
                    sequence: requested_sequence,
                    server_ns: 0,
                    data: current
                        .oldest_output_sequence
                        .saturating_sub(requested_sequence)
                        .to_le_bytes()
                        .to_vec(),
                }],
            }));
        }
        let backend = self
            .inner
            .backend
            .follow(
                request.extension_handle,
                attempt,
                requested_sequence.max(current.oldest_output_sequence),
                FOLLOW_QUEUE,
            )
            .await
            .map_err(Error::from)?;
        let first_sequence = if emitted_gap {
            requested_sequence
        } else {
            requested_sequence.max(backend.replay_from_sequence)
        };
        Ok(FollowStream {
            attempt: backend.attempt,
            first_sequence,
            through_sequence: current.output_sequence.max(backend.output_sequence),
            pending,
            backend,
        })
    }

    pub(crate) async fn discover_commands(
        &self,
        request: &wire::DiscoverCommands,
    ) -> Result<wire::CommandPage, Error> {
        self.ensure_open()?;
        let max_records = if request.max_records == 0 {
            wire::MAX_COMMAND_RECORDS
        } else {
            usize::from(request.max_records)
        };
        let reply = self
            .inner
            .service
            .native_discover_commands(
                self.inner.backend.endpoint(),
                request.directory_revision,
                request.cursor,
                max_records,
            )
            .await
            .map_err(Error::from)?;
        let definition_generations = self
            .inner
            .service
            .native_snapshot()
            .await
            .into_iter()
            .map(|definition| (definition.extension_handle, definition.generation))
            .collect::<HashMap<_, _>>();
        let listener_handles = self
            .inner
            .app
            .session
            .lock()
            .await
            .channels
            .native_catalogue()
            .listeners
            .into_iter()
            .map(|listener| (listener.generation, listener.listener_handle))
            .collect::<HashMap<_, _>>();
        let records = reply
            .records
            .into_iter()
            .map(|record| {
                let listener_generation = record.listener_generation;
                let listener_handle = listener_handles
                    .get(&listener_generation)
                    .copied()
                    .ok_or(Error::NotFound)?;
                Ok(wire::CommandRecord {
                    extension_handle: record.extension_handle,
                    generation: definition_generations
                        .get(&record.extension_handle)
                        .copied()
                        .ok_or(Error::NotFound)?,
                    definition_revision: record.definition_revision,
                    content_hash: record.content_hash,
                    listener_handle,
                    listener_generation,
                    name: record.name,
                    listener_name: record.listener_name,
                    descriptor: record.descriptor,
                    extensions: Extensions::default(),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(wire::CommandPage {
            directory_revision: reply.directory_revision,
            next_cursor: reply.next_cursor,
            records,
        })
    }

    pub(crate) async fn register_command(
        &self,
        channel_endpoint: u64,
        context: &wire::AttemptContext,
        request: &wire::RegisterCommand,
    ) -> Result<wire::RegisterCommandResult, Error> {
        self.ensure_open()?;
        let result = self
            .inner
            .service
            .native_register_command(
                self.inner.app.clone(),
                channel_endpoint,
                context.extension_handle,
                context.generation,
                context.definition_revision,
                context.attempt,
                context.task_id,
                request.listener_handle,
                request.listener_generation,
                &request.descriptor,
            )
            .await
            .map_err(Error::from)?;
        Ok(wire::RegisterCommandResult {
            extension_handle: result.extension_handle,
            generation: result.generation,
            definition_revision: result.definition_revision,
            directory_revision: result.directory_revision,
            changed: result.changed,
            extensions: Extensions::default(),
        })
    }

    pub(crate) async fn attempt_output(
        &self,
        context: &wire::AttemptContext,
        output: &wire::AttemptOutput,
    ) -> Result<u64, Error> {
        self.ensure_open()?;
        let kind = match output.kind {
            wire::OutputKind::Stdout => NativeOutputKind::Stdout,
            wire::OutputKind::Stderr => NativeOutputKind::Stderr,
            wire::OutputKind::Log => NativeOutputKind::Log,
            wire::OutputKind::Gap => {
                return Err(Error::Invalid(
                    "attempt output cannot publish a gap record".to_owned(),
                ));
            }
        };
        self.inner
            .backend
            .attempt_output(context, kind, &output.data)
            .await
            .map_err(Error::from)
    }

    pub(crate) async fn close(&self) {
        self.inner.backend.close().await;
        self.inner
            .stages
            .lock()
            .expect("Extension stage lock")
            .clear();
    }

    fn ensure_open(&self) -> Result<(), Error> {
        if self.inner.backend.is_closed() {
            Err(Error::Closed)
        } else {
            Ok(())
        }
    }

    fn allocate_stage(&self) -> Result<u64, Error> {
        let handle = self.inner.next_stage.fetch_add(1, Ordering::Relaxed);
        if handle == 0 {
            Err(Error::ResourceExhausted)
        } else {
            Ok(handle)
        }
    }
}

fn definition_record(
    definition: NativeDefinition,
    defaults: super::extension::NativeRuntimeLimits,
) -> Result<wire::ExtensionRecord, Error> {
    let phase = wire::Phase::try_from(definition.phase)
        .map_err(|error| Error::Internal(error.to_string()))?;
    let runtime = wire::Runtime::try_from(definition.runtime)
        .map_err(|error| Error::Internal(error.to_string()))?;
    let restart_policy = wire::RestartPolicy::try_from(definition.restart)
        .map_err(|error| Error::Internal(error.to_string()))?;
    let flags = u16::from(definition.flags);
    let last_exit = definition
        .last_exit
        .map(|exit| {
            Ok::<_, Error>(wire::ExitRecord {
                kind: wire::ExitKind::try_from(exit.kind)
                    .map_err(|error| Error::Internal(error.to_string()))?,
                code: exit.code,
                attempt: exit.attempt,
                server_ns: 0,
                detail: exit.detail,
                extensions: Extensions::default(),
            })
        })
        .transpose()?;
    Ok(wire::ExtensionRecord {
        extension_handle: definition.extension_handle,
        generation: definition.generation,
        definition_revision: definition.definition_revision,
        phase,
        runtime,
        restart_policy,
        flags,
        attempt: definition.attempt,
        last_running_attempt: definition.last_running_attempt,
        task_id: definition.task_id,
        next_start_unix_ms: definition.next_start_unix_ms,
        directory_revision: definition.directory_revision,
        content_hash: definition.hash,
        name: definition.name,
        last_exit,
        runtime_limits: applied_runtime_limits(defaults),
        extensions: Extensions::default(),
    })
}

fn applied_runtime_limits(defaults: super::extension::NativeRuntimeLimits) -> wire::RuntimeLimits {
    wire::RuntimeLimits {
        memory_bytes: defaults.memory_bytes,
        stack_bytes: defaults.stack_bytes,
        max_active_jobs: schema::extension::MAX_ACTIVE_JOBS as u32,
        max_pending_jobs: schema::extension::MAX_PENDING_JOBS as u32,
        max_job_bytes: schema::extension::MAX_JOB_BYTES,
        slow_consumer_timeout_ns: schema::extension::DEFAULT_SLOW_CONSUMER_TIMEOUT_NS,
        extensions: Extensions::default(),
    }
}

fn validate_runtime_limits(
    service: &ExtensionService,
    requested: &wire::RuntimeLimits,
) -> Result<(), Error> {
    let applied = applied_runtime_limits(service.native_runtime_limits());
    let compatible = (requested.memory_bytes == 0
        || requested.memory_bytes == applied.memory_bytes)
        && (requested.stack_bytes == 0 || requested.stack_bytes == applied.stack_bytes)
        && (requested.max_active_jobs == 0 || requested.max_active_jobs == applied.max_active_jobs)
        && (requested.max_pending_jobs == 0
            || requested.max_pending_jobs == applied.max_pending_jobs)
        && (requested.max_job_bytes == 0 || requested.max_job_bytes == applied.max_job_bytes)
        && (requested.slow_consumer_timeout_ns == 0
            || requested.slow_consumer_timeout_ns == applied.slow_consumer_timeout_ns);
    if compatible {
        Ok(())
    } else {
        Err(Error::Unsupported(
            "per-definition Extension runtime limit overrides are unsupported".to_owned(),
        ))
    }
}

fn identity(definition: &NativeDefinition) -> wire::DefinitionIdentity {
    wire::DefinitionIdentity {
        extension_handle: definition.extension_handle,
        generation: definition.generation,
        definition_revision: definition.definition_revision,
        extensions: Extensions::default(),
    }
}

fn snapshot_fingerprint(records: &[wire::ExtensionRecord]) -> Result<[u8; 32], Error> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(records.len() as u64).to_le_bytes());
    for record in records {
        let encoded = record
            .encode()
            .map_err(|error| Error::Internal(error.to_string()))?;
        hasher.update(&(encoded.len() as u64).to_le_bytes());
        hasher.update(&encoded);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn fingerprint(value: &impl Encode) -> Result<[u8; 32], Error> {
    let encoded = value
        .encode()
        .map_err(|error| Error::Invalid(error.to_string()))?;
    Ok(*blake3::hash(&encoded).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_replay_rejects_conflicting_reuse_and_bounds_memory() {
        const CAPACITY: usize = 4;
        let mut replay = ReplayCache::default();
        let operation = [1; 16];
        replay.insert(operation, [2; 32], Ok(7_u64), CAPACITY);
        assert_eq!(replay.lookup(&operation, &[2; 32]), Some(Ok(7)));
        assert_eq!(
            replay.lookup(&operation, &[3; 32]),
            Some(Err(Error::Conflict))
        );
        for value in 2..=(CAPACITY + 2) {
            let mut id = [0; 16];
            id[..8].copy_from_slice(&(value as u64).to_le_bytes());
            replay.insert(id, [value as u8; 32], Ok(value as u64), CAPACITY);
        }
        assert!(replay.values.len() <= CAPACITY);
        assert!(!replay.values.contains_key(&operation));
    }

    #[test]
    fn object_begin_replay_pins_live_stage_then_retires_as_a_bounded_tombstone() {
        const CAPACITY: usize = 2;
        let live_operation = [1; 16];
        let retired_operation = [2; 16];
        let replacement = [3; 16];
        let mut live_stages = std::collections::HashSet::from([7_u64]);
        let mut replay = ReplayCache::default();
        let stage = |staging_handle| {
            Ok(ObjectStage {
                disposition: wire::ObjectDisposition::Upload,
                staging_handle,
            })
        };
        {
            let is_live = |entry: &ReplayEntry<ObjectStage>| {
                matches!(
                    &entry.value,
                    Ok(ObjectStage {
                        disposition: wire::ObjectDisposition::Upload,
                        staging_handle,
                    }) if live_stages.contains(staging_handle)
                )
            };
            assert!(replay.insert_with_liveness(
                live_operation,
                [1; 32],
                stage(7),
                CAPACITY,
                is_live,
            ));
            assert!(replay.insert_with_liveness(
                retired_operation,
                [2; 32],
                stage(8),
                CAPACITY,
                is_live,
            ));
            assert!(
                replay.insert_with_liveness(replacement, [3; 32], stage(9), CAPACITY, is_live,)
            );
        }
        assert!(replay.values.contains_key(&live_operation));
        assert!(!replay.values.contains_key(&retired_operation));
        assert_eq!(replay.values.len(), CAPACITY);

        live_stages.clear();
        let is_live = |entry: &ReplayEntry<ObjectStage>| {
            matches!(
                &entry.value,
                Ok(ObjectStage {
                    disposition: wire::ObjectDisposition::Upload,
                    staging_handle,
                }) if live_stages.contains(staging_handle)
            )
        };
        assert!(replay.insert_with_liveness([4; 16], [4; 32], stage(10), CAPACITY, is_live,));
        assert!(!replay.values.contains_key(&live_operation));
        assert_eq!(replay.values.len(), CAPACITY);
    }

    #[test]
    fn runtime_limit_policy_reports_the_applied_profile() {
        let applied = applied_runtime_limits(super::super::extension::NativeRuntimeLimits {
            memory_bytes: 64 * 1024 * 1024,
            stack_bytes: 2 * 1024 * 1024,
        });
        assert_eq!(applied.memory_bytes, 64 * 1024 * 1024);
        assert_eq!(applied.stack_bytes, 2 * 1024 * 1024);
        assert_ne!(applied.max_active_jobs, 0);
        assert_ne!(applied.slow_consumer_timeout_ns, 0);
    }

    #[test]
    fn catalogue_revision_is_monotonic_and_stable_for_equal_state() {
        let mut catalogue = CatalogueRevision::default();
        assert_eq!(catalogue.observe([1; 32]), Ok(1));
        assert_eq!(catalogue.observe([1; 32]), Ok(1));
        assert_eq!(catalogue.observe([2; 32]), Ok(2));
        assert_eq!(catalogue.observe([1; 32]), Ok(3));
    }
}
