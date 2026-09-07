//! Native YAS Git repository, state, query, and fetch helpers.

use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt;

use yas_wire::{
    Class, Decode, Encode, Extensions, Frame, family, git as wire,
    state::{Phase, RecordKind, StateAck, StateEvent, Unwatch, Watch as StateWatch, WatchResult},
};

use crate::{
    receive::{DEFAULT_STATE_WINDOW as SHARED_STATE_WINDOW, Lease as ReceiveLease},
    transfer,
    yas::{Client, Error as ClientError},
};

/// Complete encoded query bytes plus one MESSAGE envelope per possible row.
pub const DEFAULT_QUERY_WINDOW: u64 =
    wire::MAX_QUERY_BYTES as u64 + (wire::MAX_QUERY_RECORDS as u64 * 12);
pub const DEFAULT_STATE_WINDOW: u64 = SHARED_STATE_WINDOW;

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    Transfer(transfer::Error),
    FeatureMissing,
    CounterOverflow,
    Protocol(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid native Git value: {error}"),
            Self::Transfer(error) => write!(formatter, "native Git Transfer failed: {error}"),
            Self::FeatureMissing => formatter.write_str("native Git operation is unavailable"),
            Self::CounterOverflow => formatter.write_str("native Git state credit overflow"),
            Self::Protocol(detail) => write!(formatter, "native Git protocol error: {detail}"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::error::Error for Error {}

impl From<ClientError> for Error {
    fn from(value: ClientError) -> Self {
        Self::Client(value)
    }
}

impl From<yas_wire::Error> for Error {
    fn from(value: yas_wire::Error) -> Self {
        Self::Wire(value)
    }
}

impl From<transfer::Error> for Error {
    fn from(value: transfer::Error) -> Self {
        Self::Transfer(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryResult {
    pub records: Vec<wire::QueryRecord>,
    pub total_hint: u64,
}

/// One server-owned repository handle.
pub struct Repository {
    handle: u64,
    revision: u64,
    object_algorithm: u8,
    flags: u16,
    canonical_worktree_path: Vec<u8>,
    canonical_git_dir: Vec<u8>,
    closed: bool,
}

impl Repository {
    pub fn handle(&self) -> u64 {
        self.handle
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn object_algorithm(&self) -> u8 {
        self.object_algorithm
    }

    pub fn flags(&self) -> u16 {
        self.flags
    }

    pub fn canonical_worktree_path(&self) -> &[u8] {
        &self.canonical_worktree_path
    }

    pub fn canonical_git_dir(&self) -> &[u8] {
        &self.canonical_git_dir
    }

    /// Run a complete one-shot query, following every server cursor.
    pub fn query(&self, client: &mut Client, body: wire::QueryBody) -> Result<QueryResult, Error> {
        if self.closed {
            return Err(Error::Protocol("Git repository is closed"));
        }
        if !client.supports(family::GIT, Class::Request, wire::request_kind::QUERY) {
            return Err(Error::FeatureMissing);
        }
        let mut cursor = wire::QueryCursor::Start;
        let mut records = Vec::new();
        let mut total_hint = 0;
        loop {
            let previous = cursor.clone();
            let mut receive_lease = client.receive_credit_exact(DEFAULT_QUERY_WINDOW)?;
            let page: wire::QueryPage = client.request_typed_with_receive_lease(
                family::GIT,
                wire::request_kind::QUERY,
                &wire::Query {
                    repository_handle: self.handle,
                    max_records: 0,
                    cursor,
                    initial_receive_credit: DEFAULT_QUERY_WINDOW,
                    body: body.clone(),
                    extensions: Extensions::default(),
                },
                true,
                &mut receive_lease,
            )?;
            total_hint = total_hint.max(page.total_hint);
            records.extend(collect_page(client, page.delivery, receive_lease)?);
            cursor = page.next_cursor;
            if matches!(cursor, wire::QueryCursor::Start) {
                break;
            }
            if cursor == previous {
                return Err(Error::Protocol("Git query cursor did not advance"));
            }
        }
        Ok(QueryResult {
            records,
            total_hint,
        })
    }

    /// Resolve one revision specification to its first tip object.
    pub fn resolve(
        &self,
        client: &mut Client,
        spec: impl Into<Vec<u8>>,
    ) -> Result<Option<wire::ObjectId>, Error> {
        let result = self.query(client, wire::QueryBody::Resolve { spec: spec.into() })?;
        Ok(result.records.into_iter().find_map(|record| match record {
            wire::QueryRecord::Object(value) => Some(value.object),
            _ => None,
        }))
    }

    /// Collect a point-in-time state snapshot for the requested datasets.
    /// The temporary watch is closed before this function returns.
    pub fn state_snapshot(
        &self,
        client: &mut Client,
        datasets: u16,
    ) -> Result<Vec<wire::EntityRecord>, Error> {
        if self.closed {
            return Err(Error::Protocol("Git repository is closed"));
        }
        if !client.supports(family::GIT, Class::Request, wire::request_kind::WATCH) {
            return Err(Error::FeatureMissing);
        }
        let mut receive_lease = client.receive_credit_exact(DEFAULT_STATE_WINDOW)?;
        let opened: WatchResult = client.request_typed_with_receive_lease(
            family::GIT,
            wire::request_kind::WATCH,
            &wire::Watch {
                repository_handle: self.handle,
                datasets,
                state: StateWatch {
                    initial_credit: DEFAULT_STATE_WINDOW,
                    resume: None,
                    extensions: wire::WatchOptions::default().to_extensions()?,
                },
            },
            true,
            &mut receive_lease,
        )?;
        let result = collect_snapshot(client, opened.subscription_id);
        let close = client.request(
            family::GIT,
            wire::request_kind::UNWATCH,
            Unwatch {
                subscription_id: opened.subscription_id,
            }
            .encode()?,
            true,
        );
        if close.is_ok() {
            receive_lease.release();
        }
        match (result, close) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
            (Ok(records), Ok(_)) => Ok(records),
        }
    }

    /// Fetch through the native Git family using a caller-owned replay ID.
    pub fn fetch(
        &mut self,
        client: &mut Client,
        operation_id: [u8; 16],
        flags: u16,
        timeout_ms: u32,
        remote: Vec<u8>,
        refspecs: Vec<Vec<u8>>,
    ) -> Result<wire::FetchResult, Error> {
        if self.closed {
            return Err(Error::Protocol("Git repository is closed"));
        }
        if !client.supports(family::GIT, Class::Request, wire::request_kind::FETCH) {
            return Err(Error::FeatureMissing);
        }
        let result: wire::FetchResult = client.request_typed(
            family::GIT,
            wire::request_kind::FETCH,
            &wire::Fetch {
                repository_handle: self.handle,
                operation_id,
                flags,
                timeout_ms,
                remote,
                refspecs,
                extensions: Extensions::default(),
            },
            true,
        )?;
        self.revision = result.repository_revision;
        Ok(result)
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        client.request(
            family::GIT,
            wire::request_kind::CLOSE,
            wire::Close {
                repository_handle: self.handle,
                extensions: Extensions::default(),
            }
            .encode()?,
            true,
        )?;
        self.closed = true;
        Ok(())
    }
}

fn collect_page(
    client: &mut Client,
    delivery: wire::PageDelivery,
    receive_lease: ReceiveLease,
) -> Result<Vec<wire::QueryRecord>, Error> {
    let typed = match delivery {
        wire::PageDelivery::Inline(records) => {
            let mut receive_lease = receive_lease;
            receive_lease.release();
            records
        }
        wire::PageDelivery::Transfer(descriptor) => {
            let messages = transfer::receive_message_transfer_with_lease(
                client,
                &descriptor,
                DEFAULT_QUERY_WINDOW,
                wire::MAX_QUERY_RECORDS,
                receive_lease,
            )?;
            messages
                .iter()
                .map(|message| wire::TypedRecord::decode_message(message))
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    typed
        .iter()
        .filter_map(|record| wire::QueryRecord::decode_typed(record).transpose())
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn collect_snapshot(
    client: &mut Client,
    subscription_id: u32,
) -> Result<Vec<wire::EntityRecord>, Error> {
    let mut cumulative_credit = DEFAULT_STATE_WINDOW;
    let mut entities = BTreeMap::<(u16, Vec<u8>), wire::EntityRecord>::new();
    loop {
        let frame = client.next_matching_event(|frame| snapshot_frame(frame, subscription_id))?;
        let event = StateEvent::decode(&frame.payload)?;
        match event.phase {
            Phase::SnapshotBegin | Phase::Reset => entities.clear(),
            Phase::SnapshotRecords | Phase::Delta => {
                for record in &event.records {
                    match record.kind {
                        RecordKind::Add | RecordKind::Replace => {
                            let entity = wire::EntityRecord::decode(&record.body)?;
                            entities.insert((entity.entity_kind, entity.key.clone()), entity);
                        }
                        RecordKind::Patch => {
                            let patch = wire::EntityPatch::decode(&record.body)?;
                            entities.insert(
                                (patch.replacement.entity_kind, patch.replacement.key.clone()),
                                patch.replacement,
                            );
                        }
                        RecordKind::Remove => {
                            let removed = wire::RemovedEntity::decode(&record.body)?;
                            entities.remove(&(removed.entity_kind, removed.key));
                        }
                        RecordKind::Family(_) if !record.required => {}
                        RecordKind::Family(_) => {
                            return Err(Error::Protocol("unknown required Git state record"));
                        }
                    }
                }
            }
            Phase::SnapshotEnd => {}
        }
        cumulative_credit = cumulative_credit
            .checked_add(frame.payload.len() as u64)
            .ok_or(Error::CounterOverflow)?;
        client.send_typed_event(
            family::GIT,
            wire::event_kind::STATE_ACK,
            &StateAck {
                subscription_id,
                applied_revision: event.to_revision,
                cumulative_byte_limit: cumulative_credit,
            },
            false,
        )?;
        if event.phase == Phase::SnapshotEnd {
            return Ok(entities.into_values().collect());
        }
    }
}

fn snapshot_frame(frame: &Frame, subscription_id: u32) -> bool {
    frame.header.class == Class::Event
        && frame.header.family == family::GIT
        && frame.header.kind == wire::event_kind::STATE
        && frame.payload.get(..4).is_some_and(|prefix| {
            u32::from_le_bytes(prefix.try_into().expect("four-byte subscription ID"))
                == subscription_id
        })
}

impl Client {
    pub fn open_git(&mut self, source: wire::RepositorySource) -> Result<Repository, Error> {
        if !self.supports(family::GIT, Class::Request, wire::request_kind::OPEN) {
            return Err(Error::FeatureMissing);
        }
        let opened: wire::OpenResult = self.request_typed(
            family::GIT,
            wire::request_kind::OPEN,
            &wire::Open {
                source,
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(Repository {
            handle: opened.repository_handle,
            revision: opened.repository_revision,
            object_algorithm: opened.object_algorithm,
            flags: opened.repository_flags,
            canonical_worktree_path: opened.canonical_worktree_path,
            canonical_git_dir: opened.canonical_git_dir,
            closed: false,
        })
    }
}
