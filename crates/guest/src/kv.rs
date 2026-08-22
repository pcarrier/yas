//! Native KV helpers with exact hashes and bounded watch/Transfer state.

use alloc::vec::Vec;
use core::fmt;

use yas_wire::{
    Class, Decode, Encode, Extensions, Frame,
    core::Status,
    family, kv as wire,
    state::{
        Cursor, Phase, RecordKind, StateAck, StateEvent, Unwatch, Watch as StateWatch, WatchResult,
    },
};

use crate::{
    receive::{DEFAULT_STATE_WINDOW as SHARED_STATE_WINDOW, Lease as ReceiveLease},
    transfer,
    yas::{Client, Error as ClientError},
};

pub const DEFAULT_STATE_WINDOW: u64 = SHARED_STATE_WINDOW;
pub const DEFAULT_VALUE_WINDOW: u64 = wire::MAX_VALUE_BYTES as u64;

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
            Self::Wire(error) => write!(formatter, "invalid native KV value: {error}"),
            Self::Transfer(error) => write!(formatter, "native KV Transfer failed: {error}"),
            Self::FeatureMissing => formatter.write_str("native KV operation is unavailable"),
            Self::CounterOverflow => formatter.write_str("native KV credit overflow"),
            Self::Protocol(detail) => write!(formatter, "native KV protocol error: {detail}"),
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
pub struct Value {
    pub bytes: Vec<u8>,
    pub content_hash: [u8; 32],
    pub modification_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateChange {
    Upsert(wire::EntryRecord),
    Remove(wire::RemovedEntry),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateUpdate {
    pub phase: Phase,
    pub from_revision: u64,
    pub to_revision: u64,
    pub changes: Vec<StateChange>,
}

pub struct Namespace {
    handle: u64,
    prefix: Vec<u8>,
    store_revision: u64,
    closed: bool,
}

impl Namespace {
    pub fn handle(&self) -> u64 {
        self.handle
    }

    pub fn prefix(&self) -> &[u8] {
        &self.prefix
    }

    pub fn store_revision(&self) -> u64 {
        self.store_revision
    }

    pub fn get(
        &mut self,
        client: &mut Client,
        relative_key: &[u8],
    ) -> Result<Option<Value>, Error> {
        let mut receive_lease = client.receive_credit_up_to(DEFAULT_VALUE_WINDOW)?;
        let initial_receive_credit = receive_lease.bytes();
        let result: wire::GetResult = match client.request_typed_with_receive_lease(
            family::KV,
            wire::request_kind::GET,
            &wire::Get {
                namespace_handle: self.handle,
                relative_key: relative_key.to_vec(),
                initial_receive_credit,
                extensions: Extensions::default(),
            },
            true,
            &mut receive_lease,
        ) {
            Ok(result) => result,
            Err(ClientError::RequestFailed {
                status: Status::NotFound,
                ..
            }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let hash = result.value.content_hash;
        let bytes = transfer::receive_inline_or_transfer_with_lease(
            client,
            result.value,
            initial_receive_credit,
            receive_lease,
        )?;
        Ok(Some(Value {
            bytes,
            content_hash: hash,
            modification_revision: result.modification_revision,
        }))
    }

    pub fn put(
        &mut self,
        client: &mut Client,
        relative_key: &[u8],
        value: &[u8],
        precondition: wire::Precondition,
        durable: bool,
    ) -> Result<wire::MutationResult, Error> {
        let mut operation_id = [0; 16];
        client.random(&mut operation_id)?;
        let result = client.request_typed(
            family::KV,
            wire::request_kind::PUT,
            &wire::Put {
                namespace_handle: self.handle,
                operation_id,
                durable,
                relative_key: relative_key.to_vec(),
                precondition,
                value: wire::ValueSource::Inline(value.to_vec()),
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(result)
    }

    pub fn delete(
        &mut self,
        client: &mut Client,
        relative_key: &[u8],
        precondition: wire::Precondition,
        durable: bool,
    ) -> Result<wire::MutationResult, Error> {
        let mut operation_id = [0; 16];
        client.random(&mut operation_id)?;
        let result = client.request_typed(
            family::KV,
            wire::request_kind::DELETE,
            &wire::Delete {
                namespace_handle: self.handle,
                operation_id,
                durable,
                relative_key: relative_key.to_vec(),
                precondition,
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(result)
    }

    pub fn watch(
        &mut self,
        client: &mut Client,
        inline_max: u32,
        resume: Option<Cursor>,
    ) -> Result<Watch, Error> {
        let mut receive_lease = client.receive_credit_exact(DEFAULT_STATE_WINDOW)?;
        let initial_credit = receive_lease.bytes();
        let result: WatchResult = client.request_typed_with_receive_lease(
            family::KV,
            wire::request_kind::WATCH,
            &wire::Watch {
                namespace_handle: self.handle,
                inline_max,
                state: StateWatch {
                    initial_credit,
                    resume,
                    extensions: Extensions::default(),
                },
            },
            true,
            &mut receive_lease,
        )?;
        Ok(Watch {
            subscription_id: result.subscription_id,
            cumulative_credit: initial_credit,
            closed: false,
            receive_lease,
        })
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        client.request(
            family::KV,
            wire::request_kind::CLOSE,
            wire::Close {
                namespace_handle: self.handle,
                extensions: Extensions::default(),
            }
            .encode()?,
            false,
        )?;
        self.closed = true;
        Ok(())
    }
}

pub struct Watch {
    subscription_id: u32,
    cumulative_credit: u64,
    closed: bool,
    receive_lease: ReceiveLease,
}

impl Watch {
    pub fn subscription_id(&self) -> u32 {
        self.subscription_id
    }

    pub fn offer_frame(
        &mut self,
        client: &mut Client,
        frame: &Frame,
    ) -> Result<Option<StateUpdate>, Error> {
        if self.closed
            || frame.header.class != Class::Event
            || frame.header.family != family::KV
            || frame.header.kind != wire::event_kind::STATE
        {
            return Ok(None);
        }
        let event = StateEvent::decode(&frame.payload)?;
        if event.subscription_id != self.subscription_id {
            return Ok(None);
        }
        let mut changes = Vec::with_capacity(event.records.len());
        for record in &event.records {
            match record.kind {
                RecordKind::Add | RecordKind::Replace => {
                    changes.push(StateChange::Upsert(wire::entry_from_state_record(record)?));
                }
                RecordKind::Remove => {
                    changes.push(StateChange::Remove(wire::RemovedEntry::from_state_record(
                        record,
                    )?));
                }
                RecordKind::Patch | RecordKind::Family(_) => {
                    return Err(Error::Protocol("unexpected KV state record kind"));
                }
            }
        }
        self.cumulative_credit = self
            .cumulative_credit
            .checked_add(frame.payload.len() as u64)
            .ok_or(Error::CounterOverflow)?;
        client.send_typed_event(
            family::KV,
            wire::event_kind::STATE_ACK,
            &StateAck {
                subscription_id: self.subscription_id,
                applied_revision: event.to_revision,
                cumulative_byte_limit: self.cumulative_credit,
            },
            false,
        )?;
        Ok(Some(StateUpdate {
            phase: event.phase,
            from_revision: event.from_revision,
            to_revision: event.to_revision,
            changes,
        }))
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        client.request(
            family::KV,
            wire::request_kind::UNWATCH,
            Unwatch {
                subscription_id: self.subscription_id,
            }
            .encode()?,
            false,
        )?;
        self.closed = true;
        self.receive_lease.release();
        Ok(())
    }
}

impl Client {
    pub fn open_kv(&mut self, prefix: &[u8]) -> Result<Namespace, Error> {
        if !self.supports(family::KV, Class::Request, wire::request_kind::OPEN) {
            return Err(Error::FeatureMissing);
        }
        let opened: wire::OpenResult = self.request_typed(
            family::KV,
            wire::request_kind::OPEN,
            &wire::Open {
                prefix: prefix.to_vec(),
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(Namespace {
            handle: opened.namespace_handle,
            prefix: prefix.to_vec(),
            store_revision: opened.store_revision,
            closed: false,
        })
    }
}
