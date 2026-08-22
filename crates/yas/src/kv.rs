//! YAS persistent key/value family wire values.
//!
//! Keys and values are opaque bytes. Namespace prefixes and relative keys are
//! concatenated by the server; codecs bound each component independently and
//! the server must also validate the resulting full key.

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, put_bytes_u16, put_bytes_u32,
    put_i64, put_len_u32, put_u16, put_u32, put_u64,
};
use crate::core::Status;
use crate::prelude::*;
use crate::state::{Record, RecordKind, Watch as StateWatch};
use crate::transfer::{
    Delivery as TransferDelivery, Direction, InlineOrTransfer, Mode, Reset, UploadStage,
};

pub const VERSION: u16 = crate::schema::kv::VERSION;
pub const VALUE_CONTENT_KIND: u16 = crate::schema::kv::VALUE_CONTENT_KIND as u16;
pub const MAX_KEY_BYTES: usize = crate::schema::kv::MAX_KEY_BYTES as usize;
pub const MAX_VALUE_BYTES: usize = crate::schema::kv::MAX_VALUE_BYTES as usize;
pub const MAX_INLINE_BYTES: usize = crate::schema::kv::MAX_INLINE_BYTES as usize;
pub const MAX_ENTRIES: usize = crate::schema::kv::MAX_ENTRIES as usize;
pub const MAX_STORE_BYTES: u64 = crate::schema::kv::MAX_STORE_BYTES;
pub const MAX_NAMESPACES_PER_SESSION: usize =
    crate::schema::kv::MAX_NAMESPACES_PER_SESSION as usize;
pub const MAX_STAGES_PER_SESSION: usize = crate::schema::kv::MAX_STAGES_PER_SESSION as usize;
pub const MAX_STAGED_BYTES_PER_SESSION: u64 = crate::schema::kv::MAX_STAGED_BYTES_PER_SESSION;
pub const MAX_BATCH_ITEMS: usize = crate::schema::kv::MAX_BATCH_ITEMS as usize;

pub mod request_kind {
    pub use crate::schema::kv::request::*;
}

pub mod event_kind {
    pub use crate::schema::kv::event::*;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub prefix: Vec<u8>,
    pub extensions: Extensions,
}

impl Encode for Open {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_prefix(&self.prefix)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_bytes_u16(out, &self.prefix)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Open {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            prefix: decoder.len_bytes_u16()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        validate_prefix(&value.prefix)?;
        reject_unknown_required(&value.extensions, &[])?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenResult {
    pub namespace_handle: u64,
    pub store_revision: u64,
    pub extensions: Extensions,
}

impl Encode for OpenResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_identity_revision(self.namespace_handle, self.store_revision, "KV namespace")?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.namespace_handle);
        put_u64(out, self.store_revision);
        self.extensions.encode_tail(out)
    }
}

impl Decode for OpenResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            namespace_handle: decoder.u64()?,
            store_revision: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        validate_identity_revision(value.namespace_handle, value.store_revision, "KV namespace")?;
        reject_unknown_required(&value.extensions, &[])?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Close {
    pub namespace_handle: u64,
    pub extensions: Extensions,
}

impl Encode for Close {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.namespace_handle, "KV namespace handle")?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.namespace_handle);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Close {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            namespace_handle: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        validate_handle(value.namespace_handle, "KV namespace handle")?;
        reject_unknown_required(&value.extensions, &[])?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub namespace_handle: u64,
    pub inline_max: u32,
    pub state: StateWatch,
}

impl Encode for Watch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.namespace_handle, "KV namespace handle")?;
        if self.inline_max as usize > MAX_INLINE_BYTES {
            return Err(limit(
                "KV watch inline bytes",
                self.inline_max as u64,
                MAX_INLINE_BYTES as u64,
            ));
        }
        put_u64(out, self.namespace_handle);
        put_u32(out, self.inline_max);
        put_u32(out, 0);
        let state = self.state.encode()?;
        put_len_u32(out, state.len())?;
        out.extend_from_slice(&state);
        Ok(())
    }
}

impl Decode for Watch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let namespace_handle = decoder.u64()?;
        let inline_max = decoder.u32()?;
        if decoder.u32()? != 0 {
            return Err(Error::Invalid("KV WATCH reserved field"));
        }
        let state = StateWatch::decode(decoder.len_bytes_u32()?)?;
        decoder.finish()?;
        let value = Self {
            namespace_handle,
            inline_max,
            state,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRecord {
    pub relative_key: Vec<u8>,
    pub content_hash: [u8; 32],
    pub byte_len: u64,
    pub modification_revision: u64,
    pub modified_unix_ns: i64,
    pub inline_value: Option<Vec<u8>>,
    pub extensions: Extensions,
}

impl EntryRecord {
    fn validate(&self) -> Result<()> {
        validate_relative_key(&self.relative_key)?;
        if self.byte_len > MAX_VALUE_BYTES as u64 {
            return Err(limit(
                "KV value bytes",
                self.byte_len,
                MAX_VALUE_BYTES as u64,
            ));
        }
        if self.modification_revision == 0 {
            return Err(Error::Invalid("zero KV modification revision"));
        }
        if let Some(value) = &self.inline_value
            && (value.len() > MAX_INLINE_BYTES || value.len() as u64 != self.byte_len)
        {
            return Err(Error::Invalid("KV inline value length"));
        }
        reject_unknown_required(&self.extensions, &[])
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("KV entry state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for EntryRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_bytes_u16(out, &self.relative_key)?;
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.byte_len);
        put_u64(out, self.modification_revision);
        put_i64(out, self.modified_unix_ns);
        match &self.inline_value {
            None => {
                out.push(crate::schema::kv::CONTENT_NONE as u8);
                out.extend_from_slice(&[0; 3]);
            }
            Some(value) => {
                out.push(crate::schema::kv::CONTENT_INLINE as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, value)?;
            }
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for EntryRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let relative_key = decoder.len_bytes_u16()?.to_vec();
        let content_hash = decoder.array_32()?;
        let byte_len = decoder.u64()?;
        let modification_revision = decoder.u64()?;
        let modified_unix_ns = decoder.i64()?;
        let content = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("KV entry reserved bytes"));
        }
        let inline_value = match content {
            value if value == crate::schema::kv::CONTENT_NONE as u8 => None,
            value if value == crate::schema::kv::CONTENT_INLINE as u8 => {
                Some(decoder.len_bytes_u32()?.to_vec())
            }
            _ => return Err(Error::Invalid("KV entry content kind")),
        };
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            relative_key,
            content_hash,
            byte_len,
            modification_revision,
            modified_unix_ns,
            inline_value,
            extensions,
        };
        value.validate()?;
        Ok(value)
    }
}

pub fn entry_from_state_record(record: &Record) -> Result<EntryRecord> {
    if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
        return Err(Error::Invalid("KV entry state record kind"));
    }
    EntryRecord::decode(&record.body)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedEntry {
    pub relative_key: Vec<u8>,
    pub modification_revision: u64,
}

impl RemovedEntry {
    pub fn state_record(&self) -> Result<Record> {
        validate_relative_key(&self.relative_key)?;
        if self.modification_revision == 0 {
            return Err(Error::Invalid("zero KV modification revision"));
        }
        let mut body = Vec::new();
        put_bytes_u16(&mut body, &self.relative_key)?;
        put_u64(&mut body, self.modification_revision);
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if record.kind != RecordKind::Remove {
            return Err(Error::Invalid("KV remove state record kind"));
        }
        let mut decoder = Decoder::new(&record.body);
        let value = Self {
            relative_key: decoder.len_bytes_u16()?.to_vec(),
            modification_revision: decoder.u64()?,
        };
        decoder.finish()?;
        validate_relative_key(&value.relative_key)?;
        if value.modification_revision == 0 {
            return Err(Error::Invalid("zero KV modification revision"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Get {
    pub namespace_handle: u64,
    pub relative_key: Vec<u8>,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Get {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.namespace_handle, "KV namespace handle")?;
        validate_relative_key(&self.relative_key)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.namespace_handle);
        put_bytes_u16(out, &self.relative_key)?;
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Get {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            namespace_handle: decoder.u64()?,
            relative_key: decoder.len_bytes_u16()?.to_vec(),
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetResult {
    pub modification_revision: u64,
    pub value: InlineOrTransfer,
}

impl GetResult {
    fn validate(&self) -> Result<()> {
        if self.modification_revision == 0 || self.value.byte_len > MAX_VALUE_BYTES as u64 {
            return Err(Error::Invalid("KV GET result revision or length"));
        }
        match &self.value.delivery {
            TransferDelivery::Inline(bytes) if bytes.len() <= MAX_INLINE_BYTES => {}
            TransferDelivery::Inline(_) => {
                return Err(Error::Invalid("KV inline GET value length"));
            }
            TransferDelivery::Transfer(descriptor) => {
                validate_value_transfer(descriptor, Direction::SENDER_TO_RECEIVER)?;
            }
        }
        let mut ignored = Vec::new();
        self.value.encode_to(&mut ignored)
    }
}

impl Encode for GetResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.modification_revision);
        self.value.encode_to(out)
    }
}

impl Decode for GetResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let modification_revision = decoder.u64()?;
        let value = InlineOrTransfer::decode(decoder.rest())?;
        decoder.finish()?;
        let result = Self {
            modification_revision,
            value,
        };
        result.validate()?;
        Ok(result)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageValue {
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub extensions: Extensions,
}

impl Encode for StageValue {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_value_len(self.byte_len)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.byte_len);
        out.extend_from_slice(&self.content_hash);
        self.extensions.encode_tail(out)
    }
}

impl Decode for StageValue {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            byte_len: decoder.u64()?,
            content_hash: decoder.array_32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageValueResult {
    pub staging_handle: u64,
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub transfer: crate::transfer::Descriptor,
}

impl StageValueResult {
    fn validate(&self) -> Result<()> {
        validate_handle(self.staging_handle, "KV staging handle")?;
        validate_value_len(self.byte_len)?;
        validate_value_transfer(&self.transfer, Direction::RECEIVER_TO_SENDER)?;
        self.transfer.require_upload_stage(self.staging_handle)?;
        Ok(())
    }

    /// Return the KV value stage discarded when `reset` targets its upload.
    pub fn stage_discarded_by(&self, reset: &Reset) -> Result<Option<UploadStage>> {
        self.validate()?;
        reset.disposed_upload_stage_from(self.staging_handle, core::iter::once(&self.transfer))
    }
}

impl Encode for StageValueResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.staging_handle);
        put_u64(out, self.byte_len);
        out.extend_from_slice(&self.content_hash);
        self.transfer.encode_to(out)
    }
}

impl Decode for StageValueResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let staging_handle = decoder.u64()?;
        let byte_len = decoder.u64()?;
        let content_hash = decoder.array_32()?;
        let transfer = crate::transfer::Descriptor::decode(decoder.rest())?;
        decoder.finish()?;
        let value = Self {
            staging_handle,
            byte_len,
            content_hash,
            transfer,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Precondition {
    Any,
    Absent,
    Hash([u8; 32]),
    Revision(u64),
    HashAndRevision {
        content_hash: [u8; 32],
        modification_revision: u64,
    },
}

impl Precondition {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let kind = match self {
            Self::Any => crate::schema::kv::PRECONDITION_ANY,
            Self::Absent => crate::schema::kv::PRECONDITION_ABSENT,
            Self::Hash(_) => crate::schema::kv::PRECONDITION_HASH,
            Self::Revision(_) => crate::schema::kv::PRECONDITION_REVISION,
            Self::HashAndRevision { .. } => crate::schema::kv::PRECONDITION_HASH_AND_REVISION,
        };
        out.push(kind as u8);
        out.extend_from_slice(&[0; 3]);
        match self {
            Self::Any | Self::Absent => {}
            Self::Hash(hash) => out.extend_from_slice(hash),
            Self::Revision(revision) => {
                if *revision == 0 {
                    return Err(Error::Invalid("zero KV expected revision"));
                }
                put_u64(out, *revision);
            }
            Self::HashAndRevision {
                content_hash,
                modification_revision,
            } => {
                if *modification_revision == 0 {
                    return Err(Error::Invalid("zero KV expected revision"));
                }
                out.extend_from_slice(content_hash);
                put_u64(out, *modification_revision);
            }
        }
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("KV precondition reserved bytes"));
        }
        match kind {
            value if value == crate::schema::kv::PRECONDITION_ANY as u8 => Ok(Self::Any),
            value if value == crate::schema::kv::PRECONDITION_ABSENT as u8 => Ok(Self::Absent),
            value if value == crate::schema::kv::PRECONDITION_HASH as u8 => {
                Ok(Self::Hash(decoder.array_32()?))
            }
            value if value == crate::schema::kv::PRECONDITION_REVISION as u8 => {
                let revision = decoder.u64()?;
                if revision == 0 {
                    return Err(Error::Invalid("zero KV expected revision"));
                }
                Ok(Self::Revision(revision))
            }
            value if value == crate::schema::kv::PRECONDITION_HASH_AND_REVISION as u8 => {
                let content_hash = decoder.array_32()?;
                let modification_revision = decoder.u64()?;
                if modification_revision == 0 {
                    return Err(Error::Invalid("zero KV expected revision"));
                }
                Ok(Self::HashAndRevision {
                    content_hash,
                    modification_revision,
                })
            }
            _ => Err(Error::Invalid("KV precondition kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueSource {
    Inline(Vec<u8>),
    Staged(u64),
}

impl ValueSource {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Inline(value) => {
                if value.len() > MAX_INLINE_BYTES {
                    return Err(limit(
                        "KV inline mutation bytes",
                        value.len() as u64,
                        MAX_INLINE_BYTES as u64,
                    ));
                }
                out.push(crate::schema::kv::VALUE_INLINE as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, value)
            }
            Self::Staged(handle) => {
                validate_handle(*handle, "KV staging handle")?;
                out.push(crate::schema::kv::VALUE_STAGED as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *handle);
                Ok(())
            }
        }
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("KV value source reserved bytes"));
        }
        match kind {
            value if value == crate::schema::kv::VALUE_INLINE as u8 => {
                let bytes = decoder.len_bytes_u32()?.to_vec();
                if bytes.len() > MAX_INLINE_BYTES {
                    return Err(limit(
                        "KV inline mutation bytes",
                        bytes.len() as u64,
                        MAX_INLINE_BYTES as u64,
                    ));
                }
                Ok(Self::Inline(bytes))
            }
            value if value == crate::schema::kv::VALUE_STAGED as u8 => {
                let handle = decoder.u64()?;
                validate_handle(handle, "KV staging handle")?;
                Ok(Self::Staged(handle))
            }
            _ => Err(Error::Invalid("KV value source kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Put {
    pub namespace_handle: u64,
    pub operation_id: [u8; 16],
    pub durable: bool,
    pub relative_key: Vec<u8>,
    pub precondition: Precondition,
    pub value: ValueSource,
    pub extensions: Extensions,
}

impl Encode for Put {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_mutation_header(
            self.namespace_handle,
            &self.operation_id,
            &self.relative_key,
            &self.extensions,
        )?;
        put_u64(out, self.namespace_handle);
        out.extend_from_slice(&self.operation_id);
        put_u16(
            out,
            if self.durable {
                crate::schema::kv::MUTATION_DURABLE as u16
            } else {
                0
            },
        );
        put_u16(out, 0);
        put_bytes_u16(out, &self.relative_key)?;
        self.precondition.encode_to(out)?;
        self.value.encode_to(out)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Put {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let namespace_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        if flags & !(crate::schema::kv::MUTATION_DURABLE as u16) != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("KV PUT flags or reserved field"));
        }
        let relative_key = decoder.len_bytes_u16()?.to_vec();
        let precondition = Precondition::decode_from(&mut decoder)?;
        let value = ValueSource::decode_from(&mut decoder)?;
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let result = Self {
            namespace_handle,
            operation_id,
            durable: flags != 0,
            relative_key,
            precondition,
            value,
            extensions,
        };
        let mut ignored = Vec::new();
        result.encode_to(&mut ignored)?;
        Ok(result)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delete {
    pub namespace_handle: u64,
    pub operation_id: [u8; 16],
    pub durable: bool,
    pub relative_key: Vec<u8>,
    pub precondition: Precondition,
    pub extensions: Extensions,
}

impl Encode for Delete {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_mutation_header(
            self.namespace_handle,
            &self.operation_id,
            &self.relative_key,
            &self.extensions,
        )?;
        if matches!(self.precondition, Precondition::Absent) {
            return Err(Error::Invalid("KV delete-if-absent precondition"));
        }
        put_u64(out, self.namespace_handle);
        out.extend_from_slice(&self.operation_id);
        put_u16(
            out,
            if self.durable {
                crate::schema::kv::MUTATION_DURABLE as u16
            } else {
                0
            },
        );
        put_u16(out, 0);
        put_bytes_u16(out, &self.relative_key)?;
        self.precondition.encode_to(out)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Delete {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let namespace_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        if flags & !(crate::schema::kv::MUTATION_DURABLE as u16) != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("KV DELETE flags or reserved field"));
        }
        let relative_key = decoder.len_bytes_u16()?.to_vec();
        let precondition = Precondition::decode_from(&mut decoder)?;
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let result = Self {
            namespace_handle,
            operation_id,
            durable: flags != 0,
            relative_key,
            precondition,
            extensions,
        };
        let mut ignored = Vec::new();
        result.encode_to(&mut ignored)?;
        Ok(result)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mutation {
    Put {
        relative_key: Vec<u8>,
        precondition: Precondition,
        value: ValueSource,
        extensions: Extensions,
    },
    Delete {
        relative_key: Vec<u8>,
        precondition: Precondition,
        extensions: Extensions,
    },
}

impl Encode for Mutation {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let (kind, relative_key, precondition, value, extensions) = match self {
            Self::Put {
                relative_key,
                precondition,
                value,
                extensions,
            } => (
                crate::schema::kv::MUTATION_PUT,
                relative_key,
                precondition,
                Some(value),
                extensions,
            ),
            Self::Delete {
                relative_key,
                precondition,
                extensions,
            } => {
                if matches!(precondition, Precondition::Absent) {
                    return Err(Error::Invalid("KV delete-if-absent precondition"));
                }
                (
                    crate::schema::kv::MUTATION_DELETE,
                    relative_key,
                    precondition,
                    None,
                    extensions,
                )
            }
        };
        validate_relative_key(relative_key)?;
        reject_unknown_required(extensions, &[])?;
        out.push(kind as u8);
        out.extend_from_slice(&[0; 3]);
        put_bytes_u16(out, relative_key)?;
        precondition.encode_to(out)?;
        if let Some(value) = value {
            value.encode_to(out)?;
        }
        extensions.encode_tail(out)
    }
}

impl Decode for Mutation {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("KV mutation reserved bytes"));
        }
        let relative_key = decoder.len_bytes_u16()?.to_vec();
        let precondition = Precondition::decode_from(&mut decoder)?;
        let value = match kind {
            value if value == crate::schema::kv::MUTATION_PUT as u8 => Self::Put {
                relative_key,
                precondition,
                value: ValueSource::decode_from(&mut decoder)?,
                extensions: decoder.extensions()?,
            },
            value if value == crate::schema::kv::MUTATION_DELETE as u8 => Self::Delete {
                relative_key,
                precondition,
                extensions: decoder.extensions()?,
            },
            _ => return Err(Error::Invalid("KV mutation kind")),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationResult {
    pub status: Status,
    pub modification_revision: u64,
    pub modified_unix_ns: i64,
    pub content_hash: [u8; 32],
    pub byte_len: u64,
    pub extensions: Extensions,
}

impl MutationResult {
    fn validate(&self) -> Result<()> {
        if self.byte_len > MAX_VALUE_BYTES as u64 {
            return Err(limit(
                "KV mutation result bytes",
                self.byte_len,
                MAX_VALUE_BYTES as u64,
            ));
        }
        if self.status.is_ok() && self.modification_revision == 0 {
            return Err(Error::Invalid("zero successful KV mutation revision"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for MutationResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.status.code());
        put_u16(out, 0);
        put_u64(out, self.modification_revision);
        put_i64(out, self.modified_unix_ns);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.byte_len);
        self.extensions.encode_tail(out)
    }
}

impl Decode for MutationResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let status = Status::from_code(decoder.u16()?);
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("KV mutation result reserved field"));
        }
        let value = Self {
            status,
            modification_revision: decoder.u64()?,
            modified_unix_ns: decoder.i64()?,
            content_hash: decoder.array_32()?,
            byte_len: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub namespace_handle: u64,
    pub operation_id: [u8; 16],
    pub durable: bool,
    pub mutations: Vec<Mutation>,
    pub extensions: Extensions,
}

impl Encode for Batch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.namespace_handle, "KV namespace handle")?;
        validate_operation_id(&self.operation_id)?;
        if self.mutations.is_empty() || self.mutations.len() > MAX_BATCH_ITEMS {
            return Err(Error::Invalid("KV batch item count"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.namespace_handle);
        out.extend_from_slice(&self.operation_id);
        put_u16(
            out,
            if self.durable {
                crate::schema::kv::MUTATION_DURABLE as u16
            } else {
                0
            },
        );
        put_u16(
            out,
            u16::try_from(self.mutations.len()).map_err(|_| Error::LengthOverflow)?,
        );
        for mutation in &self.mutations {
            let bytes = mutation.encode()?;
            put_bytes_u32(out, &bytes)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Batch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let namespace_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        if flags & !(crate::schema::kv::MUTATION_DURABLE as u16) != 0 {
            return Err(Error::Invalid("KV BATCH flags"));
        }
        let count = decoder.u16()? as usize;
        if count == 0 || count > MAX_BATCH_ITEMS {
            return Err(Error::Invalid("KV batch item count"));
        }
        let mut mutations = Vec::with_capacity(count);
        for _ in 0..count {
            mutations.push(Mutation::decode(decoder.len_bytes_u32()?)?);
        }
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            namespace_handle,
            operation_id,
            durable: flags != 0,
            mutations,
            extensions,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchResult {
    pub store_revision: u64,
    pub results: Vec<MutationResult>,
    pub extensions: Extensions,
}

impl Encode for BatchResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.store_revision == 0
            || self.results.is_empty()
            || self.results.len() > MAX_BATCH_ITEMS
        {
            return Err(Error::Invalid("KV batch result revision or count"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.store_revision);
        put_u16(
            out,
            u16::try_from(self.results.len()).map_err(|_| Error::LengthOverflow)?,
        );
        put_u16(out, 0);
        for result in &self.results {
            let bytes = result.encode()?;
            put_bytes_u32(out, &bytes)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for BatchResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let store_revision = decoder.u64()?;
        let count = decoder.u16()? as usize;
        if decoder.u16()? != 0 || count == 0 || count > MAX_BATCH_ITEMS {
            return Err(Error::Invalid("KV batch result reserved field or count"));
        }
        let mut results = Vec::with_capacity(count);
        for _ in 0..count {
            results.push(MutationResult::decode(decoder.len_bytes_u32()?)?);
        }
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            store_revision,
            results,
            extensions,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_key_bytes: u32,
    pub max_value_bytes: u64,
    pub max_inline_bytes: u32,
    pub max_entries: u32,
    pub max_store_bytes: u64,
    pub max_namespaces_per_session: u32,
    pub max_stages_per_session: u32,
    pub max_staged_bytes_per_session: u64,
    pub max_batch_items: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_key_bytes: MAX_KEY_BYTES as u32,
        max_value_bytes: MAX_VALUE_BYTES as u64,
        max_inline_bytes: MAX_INLINE_BYTES as u32,
        max_entries: MAX_ENTRIES as u32,
        max_store_bytes: MAX_STORE_BYTES,
        max_namespaces_per_session: MAX_NAMESPACES_PER_SESSION as u32,
        max_stages_per_session: MAX_STAGES_PER_SESSION as u32,
        max_staged_bytes_per_session: MAX_STAGED_BYTES_PER_SESSION,
        max_batch_items: MAX_BATCH_ITEMS as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid = self.max_key_bytes > 0
            && self.max_key_bytes <= hard.max_key_bytes
            && self.max_value_bytes <= hard.max_value_bytes
            && self.max_inline_bytes <= hard.max_inline_bytes
            && self.max_inline_bytes as u64 <= self.max_value_bytes
            && self.max_entries > 0
            && self.max_entries <= hard.max_entries
            && self.max_store_bytes > 0
            && self.max_store_bytes <= hard.max_store_bytes
            && self.max_namespaces_per_session > 0
            && self.max_namespaces_per_session <= hard.max_namespaces_per_session
            && self.max_stages_per_session > 0
            && self.max_stages_per_session <= hard.max_stages_per_session
            && self.max_staged_bytes_per_session > 0
            && self.max_staged_bytes_per_session <= hard.max_staged_bytes_per_session
            && self.max_batch_items > 0
            && self.max_batch_items <= hard.max_batch_items;
        if !valid {
            return Err(Error::Invalid("KV family limits"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(crate::schema::kv::LIMIT_MAX_KEY_BYTES, self.max_key_bytes),
            limit_u64(
                crate::schema::kv::LIMIT_MAX_VALUE_BYTES,
                self.max_value_bytes,
            ),
            limit_u32(
                crate::schema::kv::LIMIT_MAX_INLINE_BYTES,
                self.max_inline_bytes,
            ),
            limit_u32(crate::schema::kv::LIMIT_MAX_ENTRIES, self.max_entries),
            limit_u64(
                crate::schema::kv::LIMIT_MAX_STORE_BYTES,
                self.max_store_bytes,
            ),
            limit_u32(
                crate::schema::kv::LIMIT_MAX_NAMESPACES_PER_SESSION,
                self.max_namespaces_per_session,
            ),
            limit_u32(
                crate::schema::kv::LIMIT_MAX_STAGES_PER_SESSION,
                self.max_stages_per_session,
            ),
            limit_u64(
                crate::schema::kv::LIMIT_MAX_STAGED_BYTES_PER_SESSION,
                self.max_staged_bytes_per_session,
            ),
            limit_u32(
                crate::schema::kv::LIMIT_MAX_BATCH_ITEMS,
                self.max_batch_items,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        let known = [
            crate::schema::kv::LIMIT_MAX_KEY_BYTES as u16,
            crate::schema::kv::LIMIT_MAX_VALUE_BYTES as u16,
            crate::schema::kv::LIMIT_MAX_INLINE_BYTES as u16,
            crate::schema::kv::LIMIT_MAX_ENTRIES as u16,
            crate::schema::kv::LIMIT_MAX_STORE_BYTES as u16,
            crate::schema::kv::LIMIT_MAX_NAMESPACES_PER_SESSION as u16,
            crate::schema::kv::LIMIT_MAX_STAGES_PER_SESSION as u16,
            crate::schema::kv::LIMIT_MAX_STAGED_BYTES_PER_SESSION as u16,
            crate::schema::kv::LIMIT_MAX_BATCH_ITEMS as u16,
        ];
        reject_unknown_required(extensions, &known)?;
        let value = Self {
            max_key_bytes: read_u32(extensions, crate::schema::kv::LIMIT_MAX_KEY_BYTES)?,
            max_value_bytes: read_u64(extensions, crate::schema::kv::LIMIT_MAX_VALUE_BYTES)?,
            max_inline_bytes: read_u32(extensions, crate::schema::kv::LIMIT_MAX_INLINE_BYTES)?,
            max_entries: read_u32(extensions, crate::schema::kv::LIMIT_MAX_ENTRIES)?,
            max_store_bytes: read_u64(extensions, crate::schema::kv::LIMIT_MAX_STORE_BYTES)?,
            max_namespaces_per_session: read_u32(
                extensions,
                crate::schema::kv::LIMIT_MAX_NAMESPACES_PER_SESSION,
            )?,
            max_stages_per_session: read_u32(
                extensions,
                crate::schema::kv::LIMIT_MAX_STAGES_PER_SESSION,
            )?,
            max_staged_bytes_per_session: read_u64(
                extensions,
                crate::schema::kv::LIMIT_MAX_STAGED_BYTES_PER_SESSION,
            )?,
            max_batch_items: read_u32(extensions, crate::schema::kv::LIMIT_MAX_BATCH_ITEMS)?,
        };
        value.validate()?;
        Ok(value)
    }
}

pub fn validate_full_key(prefix: &[u8], relative_key: &[u8]) -> Result<()> {
    validate_prefix(prefix)?;
    validate_relative_key(relative_key)?;
    let len = prefix
        .len()
        .checked_add(relative_key.len())
        .ok_or(Error::LengthOverflow)?;
    if len == 0 || len > MAX_KEY_BYTES {
        return Err(Error::Invalid("KV full key length"));
    }
    Ok(())
}

fn validate_prefix(prefix: &[u8]) -> Result<()> {
    if prefix.len() > MAX_KEY_BYTES {
        return Err(limit(
            "KV namespace prefix bytes",
            prefix.len() as u64,
            MAX_KEY_BYTES as u64,
        ));
    }
    if prefix.contains(&0) {
        return Err(Error::Invalid("KV namespace prefix"));
    }
    Ok(())
}

fn validate_relative_key(key: &[u8]) -> Result<()> {
    if key.len() > MAX_KEY_BYTES {
        return Err(limit(
            "KV relative key bytes",
            key.len() as u64,
            MAX_KEY_BYTES as u64,
        ));
    }
    if key.contains(&0) {
        return Err(Error::Invalid("KV relative key"));
    }
    Ok(())
}

fn validate_mutation_header(
    namespace_handle: u64,
    operation_id: &[u8; 16],
    relative_key: &[u8],
    extensions: &Extensions,
) -> Result<()> {
    validate_handle(namespace_handle, "KV namespace handle")?;
    validate_operation_id(operation_id)?;
    validate_relative_key(relative_key)?;
    reject_unknown_required(extensions, &[])
}

fn validate_operation_id(operation_id: &[u8; 16]) -> Result<()> {
    if operation_id.iter().all(|byte| *byte == 0) {
        return Err(Error::Invalid("zero KV operation ID"));
    }
    Ok(())
}

fn validate_value_len(byte_len: u64) -> Result<()> {
    if byte_len > MAX_VALUE_BYTES as u64 {
        return Err(limit("KV value bytes", byte_len, MAX_VALUE_BYTES as u64));
    }
    Ok(())
}

fn validate_value_transfer(
    descriptor: &crate::transfer::Descriptor,
    direction: Direction,
) -> Result<()> {
    let sensitive = descriptor.extensions.0.iter().any(|extension| {
        extension.tag == crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16
            && extension.required
            && extension.value.is_empty()
    });
    if descriptor.mode != Mode::Byte
        || descriptor.direction != direction
        || descriptor.content_family != crate::family::KV
        || descriptor.content_kind != VALUE_CONTENT_KIND
        || descriptor.content_version != VERSION
        || !sensitive
    {
        return Err(Error::Invalid("KV value Transfer descriptor"));
    }
    descriptor.validate()
}

fn validate_identity_revision(handle: u64, revision: u64, name: &'static str) -> Result<()> {
    validate_handle(handle, name)?;
    if revision == 0 {
        return Err(Error::Invalid("zero KV store revision"));
    }
    Ok(())
}

fn validate_handle(handle: u64, name: &'static str) -> Result<()> {
    if handle == 0 {
        return Err(Error::Invalid(name));
    }
    Ok(())
}

fn reject_unknown_required(extensions: &Extensions, known: &[u16]) -> Result<()> {
    extensions.validate()?;
    if extensions
        .0
        .iter()
        .any(|extension| extension.required && !known.contains(&extension.tag))
    {
        return Err(Error::Invalid("unknown required KV extension"));
    }
    Ok(())
}

fn limit(name: &'static str, actual: u64, maximum: u64) -> Error {
    Error::LimitExceeded {
        limit: name,
        actual,
        maximum,
    }
}

fn limit_u32(tag: u64, value: u32) -> Extension {
    Extension {
        tag: tag as u16,
        required: false,
        value: value.to_le_bytes().to_vec(),
    }
}

fn limit_u64(tag: u64, value: u64) -> Extension {
    Extension {
        tag: tag as u16,
        required: false,
        value: value.to_le_bytes().to_vec(),
    }
}

fn extension_value(extensions: &Extensions, tag: u64) -> Result<&[u8]> {
    extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .map(|extension| extension.value.as_slice())
        .ok_or(Error::Invalid("missing KV family limit"))
}

fn read_u32(extensions: &Extensions, tag: u64) -> Result<u32> {
    Ok(u32::from_le_bytes(
        extension_value(extensions, tag)?
            .try_into()
            .map_err(|_| Error::Invalid("KV family limit length"))?,
    ))
}

fn read_u64(extensions: &Extensions, tag: u64) -> Result<u64> {
    Ok(u64::from_le_bytes(
        extension_value(extensions, tag)?
            .try_into()
            .map_err(|_| Error::Invalid("KV family limit length"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state;
    use crate::transfer::{Delivery, Descriptor};

    fn sensitive_descriptor(id: u32, direction: Direction) -> Descriptor {
        Descriptor {
            transfer_id: id,
            mode: Mode::Byte,
            direction,
            receiver_send_credit: if direction.receiver_to_sender {
                64 * 1024
            } else {
                0
            },
            sender_send_credit: if direction.sender_to_receiver {
                64 * 1024
            } else {
                0
            },
            max_item_bytes: 0,
            max_chunk_bytes: 64 * 1024,
            content_family: crate::family::KV,
            content_kind: VALUE_CONTENT_KIND,
            content_version: VERSION,
            extensions: Extensions(vec![Extension {
                tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    fn every_truncation<T>(value: &T)
    where
        T: Encode + Decode + PartialEq + std::fmt::Debug,
    {
        let bytes = value.encode().unwrap();
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
        assert_eq!(&T::decode(&bytes).unwrap(), value);
    }

    fn mutation_result(status: Status) -> MutationResult {
        MutationResult {
            status,
            modification_revision: if status.is_ok() { 9 } else { 0 },
            modified_unix_ns: if status.is_ok() {
                1_700_000_000_000_000_009
            } else {
                0
            },
            content_hash: [3; 32],
            byte_len: 3,
            extensions: Extensions::default(),
        }
    }

    #[test]
    fn namespace_and_watch_values_round_trip() {
        every_truncation(&Open {
            prefix: b"editor/buf/".to_vec(),
            extensions: Extensions::default(),
        });
        every_truncation(&OpenResult {
            namespace_handle: 7,
            store_revision: 3,
            extensions: Extensions::default(),
        });
        every_truncation(&Close {
            namespace_handle: 7,
            extensions: Extensions::default(),
        });
        every_truncation(&Watch {
            namespace_handle: 7,
            inline_max: 4096,
            state: StateWatch {
                initial_credit: 1024 * 1024,
                resume: Some(state::Cursor {
                    boot_id: [1; 16],
                    revision: 2,
                }),
                extensions: Extensions::default(),
            },
        });
    }

    #[test]
    fn state_records_round_trip_raw_relative_keys() {
        let entry = EntryRecord {
            relative_key: vec![0xff, b'/'],
            content_hash: [2; 32],
            byte_len: 3,
            modification_revision: 8,
            modified_unix_ns: 1_700_000_000_000_000_008,
            inline_value: Some(vec![1, 2, 3]),
            extensions: Extensions::default(),
        };
        every_truncation(&entry);
        let record = entry.state_record(RecordKind::Add).unwrap();
        assert_eq!(entry_from_state_record(&record).unwrap(), entry);

        let removed = RemovedEntry {
            relative_key: b"gone".to_vec(),
            modification_revision: 9,
        };
        let record = removed.state_record().unwrap();
        assert_eq!(RemovedEntry::from_state_record(&record).unwrap(), removed);
    }

    #[test]
    fn get_and_stage_values_round_trip() {
        every_truncation(&Get {
            namespace_handle: 7,
            relative_key: b"one".to_vec(),
            initial_receive_credit: 128 * 1024,
            extensions: Extensions::default(),
        });
        every_truncation(&GetResult {
            modification_revision: 4,
            value: InlineOrTransfer {
                byte_len: 3,
                content_hash: [9; 32],
                delivery: Delivery::Inline(vec![1, 2, 3]),
            },
        });
        every_truncation(&GetResult {
            modification_revision: 4,
            value: InlineOrTransfer {
                byte_len: 100_000,
                content_hash: [9; 32],
                delivery: Delivery::Transfer(sensitive_descriptor(
                    2,
                    Direction::SENDER_TO_RECEIVER,
                )),
            },
        });
        every_truncation(&StageValue {
            byte_len: 100_000,
            content_hash: [4; 32],
            extensions: Extensions::default(),
        });
        let mut upload = sensitive_descriptor(2, Direction::RECEIVER_TO_SENDER);
        upload.extensions.0.push(
            UploadStage {
                staging_handle: 11,
                expires_server_ns: 1,
            }
            .extension()
            .unwrap(),
        );
        let stage = StageValueResult {
            staging_handle: 11,
            byte_len: 100_000,
            content_hash: [4; 32],
            transfer: upload,
        };
        every_truncation(&stage);
        let reset = Reset {
            transfer_id: stage.transfer.transfer_id,
            status: crate::schema::core::status::CANCELLED,
            detail: Vec::new(),
        };
        assert_eq!(
            stage.stage_discarded_by(&reset).unwrap(),
            stage.transfer.upload_stage().unwrap()
        );
    }

    #[test]
    fn mutations_and_atomic_batch_round_trip() {
        let put = Put {
            namespace_handle: 7,
            operation_id: [1; 16],
            durable: true,
            relative_key: b"one".to_vec(),
            precondition: Precondition::HashAndRevision {
                content_hash: [2; 32],
                modification_revision: 3,
            },
            value: ValueSource::Inline(vec![4, 5]),
            extensions: Extensions::default(),
        };
        every_truncation(&put);
        every_truncation(&Delete {
            namespace_handle: 7,
            operation_id: [2; 16],
            durable: false,
            relative_key: b"two".to_vec(),
            precondition: Precondition::Revision(4),
            extensions: Extensions::default(),
        });
        let batch = Batch {
            namespace_handle: 7,
            operation_id: [3; 16],
            durable: true,
            mutations: vec![
                Mutation::Put {
                    relative_key: b"one".to_vec(),
                    precondition: Precondition::Any,
                    value: ValueSource::Staged(9),
                    extensions: Extensions::default(),
                },
                Mutation::Delete {
                    relative_key: b"two".to_vec(),
                    precondition: Precondition::Hash([4; 32]),
                    extensions: Extensions::default(),
                },
            ],
            extensions: Extensions::default(),
        };
        every_truncation(&batch);
        every_truncation(&BatchResult {
            store_revision: 10,
            results: vec![
                mutation_result(Status::Ok),
                mutation_result(Status::Conflict),
            ],
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn malformed_values_and_limits_are_rejected() {
        assert!(validate_full_key(b"", b"").is_err());
        assert!(validate_full_key(b"a", b"").is_ok());
        assert!(
            Open {
                prefix: vec![0; 1],
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
        assert!(
            Delete {
                namespace_handle: 1,
                operation_id: [1; 16],
                durable: false,
                relative_key: b"x".to_vec(),
                precondition: Precondition::Absent,
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
        let mut wrong = sensitive_descriptor(2, Direction::SENDER_TO_RECEIVER);
        wrong.content_family = crate::family::FONT;
        assert!(
            GetResult {
                modification_revision: 1,
                value: InlineOrTransfer {
                    byte_len: 1,
                    content_hash: [1; 32],
                    delivery: Delivery::Transfer(wrong),
                },
            }
            .encode()
            .is_err()
        );
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
    }
}
