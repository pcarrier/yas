//! YAS process-wide binary event-journal family wire values.

use crate::codec::{
    Decode, Decoder, Encode, Error, Extensions, Result, limit_u32, limit_u64, put_bytes_u32,
    put_len_u16, put_string_u32, put_u16, put_u32, put_u64, read_limit_u32, read_limit_u64,
};
use crate::core::Status;
use crate::prelude::*;
use crate::transfer::{Descriptor, Direction, Mode};

pub const VERSION: u16 = crate::schema::events::VERSION;
pub const ACTIVATION_WORDS: usize = crate::schema::events::ACTIVATION_WORDS as usize;
pub const MAX_RECORDING_PATH_BYTES: usize =
    crate::schema::events::MAX_RECORDING_PATH_BYTES as usize;
pub const MAX_RECORD_ERROR_BYTES: usize = crate::schema::events::MAX_RECORD_ERROR_BYTES as usize;
pub const MAX_LIVE_BATCH_BYTES: usize = crate::schema::events::MAX_LIVE_BATCH_BYTES as usize;
pub const EVENTS_CODEC_ID: u16 = crate::schema::packed_codec::EVENTS_CODEC_V1;
pub const EVENTS_CODEC_VERSION: u16 = crate::schema::packed_codec::events_codec_v1::VERSION;

pub mod request_kind {
    pub use crate::schema::events::request::*;
}

pub mod event_kind {
    pub use crate::schema::events::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub min_ring_bytes: u64,
    pub max_ring_bytes: u64,
    pub max_streams_per_session: u32,
    pub max_recordings: u32,
    pub max_recording_path_bytes: u32,
    pub max_live_batch_bytes: u32,
    pub max_pending_dumps: u32,
    pub max_mutation_replays: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        min_ring_bytes: crate::schema::events::MIN_RING_BYTES,
        max_ring_bytes: crate::schema::events::MAX_RING_BYTES,
        max_streams_per_session: crate::schema::events::MAX_STREAMS_PER_SESSION as u32,
        max_recordings: crate::schema::events::MAX_RECORDINGS as u32,
        max_recording_path_bytes: crate::schema::events::MAX_RECORDING_PATH_BYTES as u32,
        max_live_batch_bytes: crate::schema::events::MAX_LIVE_BATCH_BYTES as u32,
        max_pending_dumps: crate::schema::events::MAX_PENDING_DUMPS as u32,
        max_mutation_replays: crate::schema::events::MAX_MUTATION_REPLAYS as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        if self.min_ring_bytes < hard.min_ring_bytes
            || self.min_ring_bytes > self.max_ring_bytes
            || self.max_ring_bytes > hard.max_ring_bytes
            || [
                (self.max_streams_per_session, hard.max_streams_per_session),
                (self.max_recordings, hard.max_recordings),
                (self.max_recording_path_bytes, hard.max_recording_path_bytes),
                (self.max_live_batch_bytes, hard.max_live_batch_bytes),
                (self.max_pending_dumps, hard.max_pending_dumps),
                (self.max_mutation_replays, hard.max_mutation_replays),
            ]
            .into_iter()
            .any(|(value, maximum)| value == 0 || value > maximum)
        {
            return Err(Error::Invalid("Events family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u64(
                crate::schema::events::LIMIT_MIN_RING_BYTES,
                self.min_ring_bytes,
            ),
            limit_u64(
                crate::schema::events::LIMIT_MAX_RING_BYTES,
                self.max_ring_bytes,
            ),
            limit_u32(
                crate::schema::events::LIMIT_MAX_STREAMS_PER_SESSION,
                self.max_streams_per_session,
            ),
            limit_u32(
                crate::schema::events::LIMIT_MAX_RECORDINGS,
                self.max_recordings,
            ),
            limit_u32(
                crate::schema::events::LIMIT_MAX_RECORDING_PATH_BYTES,
                self.max_recording_path_bytes,
            ),
            limit_u32(
                crate::schema::events::LIMIT_MAX_LIVE_BATCH_BYTES,
                self.max_live_batch_bytes,
            ),
            limit_u32(
                crate::schema::events::LIMIT_MAX_PENDING_DUMPS,
                self.max_pending_dumps,
            ),
            limit_u32(
                crate::schema::events::LIMIT_MAX_MUTATION_REPLAYS,
                self.max_mutation_replays,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required(
            extensions,
            &[
                crate::schema::events::LIMIT_MIN_RING_BYTES as u16,
                crate::schema::events::LIMIT_MAX_RING_BYTES as u16,
                crate::schema::events::LIMIT_MAX_STREAMS_PER_SESSION as u16,
                crate::schema::events::LIMIT_MAX_RECORDINGS as u16,
                crate::schema::events::LIMIT_MAX_RECORDING_PATH_BYTES as u16,
                crate::schema::events::LIMIT_MAX_LIVE_BATCH_BYTES as u16,
                crate::schema::events::LIMIT_MAX_PENDING_DUMPS as u16,
                crate::schema::events::LIMIT_MAX_MUTATION_REPLAYS as u16,
            ],
        )?;
        let value = Self {
            min_ring_bytes: read_limit_u64(
                extensions,
                crate::schema::events::LIMIT_MIN_RING_BYTES,
            )?,
            max_ring_bytes: read_limit_u64(
                extensions,
                crate::schema::events::LIMIT_MAX_RING_BYTES,
            )?,
            max_streams_per_session: read_limit_u32(
                extensions,
                crate::schema::events::LIMIT_MAX_STREAMS_PER_SESSION,
            )?,
            max_recordings: read_limit_u32(
                extensions,
                crate::schema::events::LIMIT_MAX_RECORDINGS,
            )?,
            max_recording_path_bytes: read_limit_u32(
                extensions,
                crate::schema::events::LIMIT_MAX_RECORDING_PATH_BYTES,
            )?,
            max_live_batch_bytes: read_limit_u32(
                extensions,
                crate::schema::events::LIMIT_MAX_LIVE_BATCH_BYTES,
            )?,
            max_pending_dumps: read_limit_u32(
                extensions,
                crate::schema::events::LIMIT_MAX_PENDING_DUMPS,
            )?,
            max_mutation_replays: read_limit_u32(
                extensions,
                crate::schema::events::LIMIT_MAX_MUTATION_REPLAYS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActivationSet(pub [u64; ACTIVATION_WORDS]);

impl ActivationSet {
    pub const fn low_throughput() -> Self {
        Self([u16::MAX as u64, 0, 0, 0])
    }

    pub const fn all() -> Self {
        Self([u64::MAX; ACTIVATION_WORDS])
    }

    pub const fn enabled(self, event_id: u16) -> bool {
        let id = event_id as usize;
        id < ACTIVATION_WORDS * 64 && self.0[id / 64] & (1u64 << (id % 64)) != 0
    }

    pub fn set(&mut self, event_id: u16, enabled: bool) {
        let id = event_id as usize;
        if id >= ACTIVATION_WORDS * 64 {
            return;
        }
        let bit = 1u64 << (id % 64);
        if enabled {
            self.0[id / 64] |= bit;
        } else {
            self.0[id / 64] &= !bit;
        }
    }

    fn encode_to(self, out: &mut Vec<u8>) {
        for word in self.0 {
            put_u64(out, word);
        }
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let mut words = [0; ACTIVATION_WORDS];
        for word in &mut words {
            *word = decoder.u64()?;
        }
        Ok(Self(words))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetConfig {
    pub extensions: Extensions,
}

impl Encode for GetConfig {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        reject_unknown_required(&self.extensions, &[])?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for GetConfig {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        reject_unknown_required(&value.extensions, &[])?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub revision: u64,
    pub capacity: u64,
    pub used: u64,
    pub record_count: u64,
    pub dropped: u64,
    pub next_sequence: u64,
    pub activations: ActivationSet,
    pub extensions: Extensions,
}

impl Config {
    fn validate(&self) -> Result<()> {
        if self.revision == 0 {
            return Err(Error::Invalid("zero Events configuration revision"));
        }
        if !(crate::schema::events::MIN_RING_BYTES..=crate::schema::events::MAX_RING_BYTES)
            .contains(&self.capacity)
            || self.used > self.capacity
            || self.record_count > self.used
            || self.next_sequence < self.record_count
        {
            return Err(Error::Invalid("Events configuration counters"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for Config {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        for value in [
            self.revision,
            self.capacity,
            self.used,
            self.record_count,
            self.dropped,
            self.next_sequence,
        ] {
            put_u64(out, value);
        }
        self.activations.encode_to(out);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Config {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            revision: decoder.u64()?,
            capacity: decoder.u64()?,
            used: decoder.u64()?,
            record_count: decoder.u64()?,
            dropped: decoder.u64()?,
            next_sequence: decoder.u64()?,
            activations: ActivationSet::decode_from(&mut decoder)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetConfig {
    pub operation_id: [u8; 16],
    /// Zero applies unconditionally; every other value is a CAS revision.
    pub expected_revision: u64,
    pub capacity: u64,
    pub activations: ActivationSet,
    pub extensions: Extensions,
}

impl Encode for SetConfig {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        if !(crate::schema::events::MIN_RING_BYTES..=crate::schema::events::MAX_RING_BYTES)
            .contains(&self.capacity)
        {
            return Err(Error::Invalid("Events ring capacity"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        out.extend_from_slice(&self.operation_id);
        put_u64(out, self.expected_revision);
        put_u64(out, self.capacity);
        self.activations.encode_to(out);
        self.extensions.encode_tail(out)
    }
}

impl Decode for SetConfig {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            operation_id: decoder.array_16()?,
            expected_revision: decoder.u64()?,
            capacity: decoder.u64()?,
            activations: ActivationSet::decode_from(&mut decoder)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dump {
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Dump {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.initial_receive_credit == 0 {
            return Err(Error::Invalid("zero Events dump receive credit"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Dump {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
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
pub struct DumpResult {
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub descriptor: Descriptor,
    pub extensions: Extensions,
}

impl DumpResult {
    fn validate(&self) -> Result<()> {
        self.descriptor.validate()?;
        if self.descriptor.mode != Mode::Byte
            || self.descriptor.direction != Direction::SENDER_TO_RECEIVER
            || self.descriptor.receiver_send_credit != 0
            || self.descriptor.max_item_bytes != 0
            || self.descriptor.content_family != crate::family::EVENTS
            || self.descriptor.content_kind != crate::schema::events::DUMP_CONTENT_KIND as u16
            || self.descriptor.content_version != VERSION
            || !self.descriptor.sensitive_content()?
        {
            return Err(Error::Invalid("Events dump Transfer descriptor"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for DumpResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.byte_len);
        out.extend_from_slice(&self.content_hash);
        put_bytes_u32(out, &self.descriptor.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for DumpResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            byte_len: decoder.u64()?,
            content_hash: decoder.array_32()?,
            descriptor: Descriptor::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartStream {
    pub operation_id: [u8; 16],
    pub history: bool,
    /// With history: zero starts at the oldest retained record; nonzero asks
    /// for that sequence. Without history this must be zero and starts live.
    pub start_sequence: u64,
    /// Zero selects the server default.
    pub max_batch_bytes: u32,
    pub extensions: Extensions,
}

impl Encode for StartStream {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        if (!self.history && self.start_sequence != 0)
            || self.max_batch_bytes as usize > MAX_LIVE_BATCH_BYTES
        {
            return Err(Error::Invalid("Events stream history or batch options"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        out.extend_from_slice(&self.operation_id);
        put_u16(
            out,
            if self.history {
                crate::schema::events::STREAM_HISTORY as u16
            } else {
                0
            },
        );
        put_u16(out, 0);
        put_u64(out, self.start_sequence);
        put_u32(out, self.max_batch_bytes);
        put_u32(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for StartStream {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        if flags & !(crate::schema::events::STREAM_FLAGS as u16) != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Events stream flags or reserved field"));
        }
        let start_sequence = decoder.u64()?;
        let max_batch_bytes = decoder.u32()?;
        if decoder.u32()? != 0 {
            return Err(Error::Invalid("Events stream reserved field"));
        }
        let value = Self {
            operation_id,
            history: flags & crate::schema::events::STREAM_HISTORY as u16 != 0,
            start_sequence,
            max_batch_bytes,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamStarted {
    pub stream_handle: u64,
    pub first_sequence: u64,
    pub max_batch_bytes: u32,
    pub extensions: Extensions,
}

impl Encode for StreamStarted {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.stream_handle, "Events stream handle")?;
        if self.max_batch_bytes == 0 || self.max_batch_bytes as usize > MAX_LIVE_BATCH_BYTES {
            return Err(Error::Invalid("Events stream batch bytes"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.stream_handle);
        put_u64(out, self.first_sequence);
        put_u32(out, self.max_batch_bytes);
        put_u32(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for StreamStarted {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            stream_handle: decoder.u64()?,
            first_sequence: decoder.u64()?,
            max_batch_bytes: decoder.u32()?,
            extensions: {
                if decoder.u32()? != 0 {
                    return Err(Error::Invalid("Events stream-started reserved field"));
                }
                decoder.extensions()?
            },
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopStream {
    pub stream_handle: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for StopStream {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.stream_handle, "Events stream handle")?;
        validate_operation_id(&self.operation_id)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.stream_handle);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for StopStream {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            stream_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordingState {
    Running = crate::schema::events::RECORDING_RUNNING as u8,
    Stopped = crate::schema::events::RECORDING_STOPPED as u8,
    Failed = crate::schema::events::RECORDING_FAILED as u8,
}

impl TryFrom<u8> for RecordingState {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Running as u8 => Ok(Self::Running),
            value if value == Self::Stopped as u8 => Ok(Self::Stopped),
            value if value == Self::Failed as u8 => Ok(Self::Failed),
            _ => Err(Error::Invalid("Events recording state")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartRecording {
    pub operation_id: [u8; 16],
    pub history: bool,
    pub append: bool,
    pub path: Vec<u8>,
    pub extensions: Extensions,
}

impl Encode for StartRecording {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        validate_path(&self.path)?;
        reject_unknown_required(&self.extensions, &[])?;
        out.extend_from_slice(&self.operation_id);
        let flags = (u16::from(self.history) * crate::schema::events::RECORDING_HISTORY as u16)
            | (u16::from(self.append) * crate::schema::events::RECORDING_APPEND as u16);
        put_u16(out, flags);
        put_u16(out, 0);
        put_bytes_u32(out, &self.path)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for StartRecording {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        if flags & !(crate::schema::events::RECORDING_FLAGS as u16) != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Events recording flags or reserved field"));
        }
        let value = Self {
            operation_id,
            history: flags & crate::schema::events::RECORDING_HISTORY as u16 != 0,
            append: flags & crate::schema::events::RECORDING_APPEND as u16 != 0,
            path: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopRecording {
    pub recording_handle: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for StopRecording {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.recording_handle, "Events recording handle")?;
        validate_operation_id(&self.operation_id)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.recording_handle);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for StopRecording {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            recording_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingInfo {
    pub recording_handle: u64,
    pub state: RecordingState,
    pub history: bool,
    pub append: bool,
    pub records: u64,
    pub bytes: u64,
    pub lost: u64,
    pub path: Vec<u8>,
    pub error: String,
    pub extensions: Extensions,
}

impl RecordingInfo {
    fn validate(&self) -> Result<()> {
        validate_handle(self.recording_handle, "Events recording handle")?;
        validate_path(&self.path)?;
        if self.error.len() > MAX_RECORD_ERROR_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Events recording error bytes",
                actual: self.error.len() as u64,
                maximum: MAX_RECORD_ERROR_BYTES as u64,
            });
        }
        if self.state == RecordingState::Running && !self.error.is_empty() {
            return Err(Error::Invalid("running Events recording error"));
        }
        reject_unknown_required(&self.extensions, &[])
    }

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.recording_handle);
        out.push(self.state as u8);
        out.push(0);
        let flags = (u16::from(self.history) * crate::schema::events::RECORDING_HISTORY as u16)
            | (u16::from(self.append) * crate::schema::events::RECORDING_APPEND as u16);
        put_u16(out, flags);
        put_u64(out, self.records);
        put_u64(out, self.bytes);
        put_u64(out, self.lost);
        put_bytes_u32(out, &self.path)?;
        put_string_u32(out, &self.error)?;
        self.extensions.encode_tail(out)
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let recording_handle = decoder.u64()?;
        let state = RecordingState::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Events recording reserved byte"));
        }
        let flags = decoder.u16()?;
        if flags & !(crate::schema::events::RECORDING_FLAGS as u16) != 0 {
            return Err(Error::Invalid("Events recording flags"));
        }
        let value = Self {
            recording_handle,
            state,
            history: flags & crate::schema::events::RECORDING_HISTORY as u16 != 0,
            append: flags & crate::schema::events::RECORDING_APPEND as u16 != 0,
            records: decoder.u64()?,
            bytes: decoder.u64()?,
            lost: decoder.u64()?,
            path: decoder.len_bytes_u32()?.to_vec(),
            error: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for RecordingInfo {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_body(out)
    }
}

impl Decode for RecordingInfo {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRecordings {
    pub extensions: Extensions,
}

impl Encode for ListRecordings {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        reject_unknown_required(&self.extensions, &[])?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for ListRecordings {
    fn decode(input: &[u8]) -> Result<Self> {
        GetConfig::decode(input).map(|value| Self {
            extensions: value.extensions,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingList {
    pub recordings: Vec<RecordingInfo>,
}

impl Encode for RecordingList {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.recordings.len() > crate::schema::events::MAX_RECORDINGS as usize {
            return Err(Error::Invalid("Events recording count"));
        }
        put_len_u16(out, self.recordings.len())?;
        put_u16(out, 0);
        for recording in &self.recordings {
            recording.encode_body(out)?;
        }
        Ok(())
    }
}

impl Decode for RecordingList {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count > crate::schema::events::MAX_RECORDINGS as usize
            || count > decoder.remaining() / 44
        {
            return Err(Error::Invalid("Events recording count or reserved field"));
        }
        let mut recordings = Vec::with_capacity(count);
        for _ in 0..count {
            recordings.push(RecordingInfo::decode_from(&mut decoder)?);
        }
        decoder.finish()?;
        Ok(Self { recordings })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    pub sequence: u64,
    pub monotonic_ns: u64,
    pub event_id: u32,
    pub required: bool,
    /// Stable event-specific flags; their meaning is selected by `event_id`.
    pub event_flags: u16,
    pub payload: Vec<u8>,
}

impl EventRecord {
    const HEADER_BYTES: usize = 28;

    fn validate(&self) -> Result<()> {
        if self.required && !known_event(self.event_id) {
            return Err(Error::Invalid("unknown required Events record"));
        }
        let len = Self::HEADER_BYTES
            .checked_add(self.payload.len())
            .ok_or(Error::LengthOverflow)?;
        if len > crate::frame::HARD_MAX_DECODED_FRAME as usize {
            return Err(Error::LimitExceeded {
                limit: "Events record bytes",
                actual: len as u64,
                maximum: crate::frame::HARD_MAX_DECODED_FRAME as u64,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventBatch {
    pub first_sequence: u64,
    pub records: Vec<EventRecord>,
}

impl Encode for EventBatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.records.is_empty()
            || self.records.len() > crate::schema::transport::HARD_MAX_TYPED_RECORDS
        {
            return Err(Error::Invalid("Events record count"));
        }
        put_u64(out, self.first_sequence);
        put_len_u16(out, self.records.len())?;
        put_u16(out, 0);
        for (index, record) in self.records.iter().enumerate() {
            record.validate()?;
            let expected = self
                .first_sequence
                .checked_add(index as u64)
                .ok_or(Error::LengthOverflow)?;
            if record.sequence != expected {
                return Err(Error::Invalid("non-consecutive Events records"));
            }
            let len = EventRecord::HEADER_BYTES
                .checked_add(record.payload.len())
                .ok_or(Error::LengthOverflow)?;
            put_u32(out, len.try_into().map_err(|_| Error::LengthOverflow)?);
            put_u64(out, record.sequence);
            put_u64(out, record.monotonic_ns);
            put_u32(out, record.event_id);
            put_u16(
                out,
                if record.required {
                    crate::schema::packed_codec::events_codec_v1::RECORD_REQUIRED as u16
                } else {
                    0
                },
            );
            put_u16(out, record.event_flags);
            out.extend_from_slice(&record.payload);
        }
        Ok(())
    }
}

impl Decode for EventBatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let first_sequence = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count == 0
            || count > crate::schema::transport::HARD_MAX_TYPED_RECORDS
            || count > decoder.remaining() / EventRecord::HEADER_BYTES
        {
            return Err(Error::Invalid("Events record count or reserved field"));
        }
        let mut records = Vec::with_capacity(count);
        for index in 0..count {
            let len = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
            if len < EventRecord::HEADER_BYTES {
                return Err(Error::Invalid("Events record length"));
            }
            let body = decoder.take(len - 4)?;
            let mut record_decoder = Decoder::new(body);
            let sequence = record_decoder.u64()?;
            let monotonic_ns = record_decoder.u64()?;
            let event_id = record_decoder.u32()?;
            let flags = record_decoder.u16()?;
            if flags & !(crate::schema::packed_codec::events_codec_v1::RECORD_FLAGS_MASK as u16)
                != 0
            {
                return Err(Error::Invalid("Events record flags"));
            }
            let event_flags = record_decoder.u16()?;
            let required =
                flags & crate::schema::packed_codec::events_codec_v1::RECORD_REQUIRED as u16 != 0;
            let record = EventRecord {
                sequence,
                monotonic_ns,
                event_id,
                required,
                event_flags,
                payload: record_decoder.rest().to_vec(),
            };
            record.validate()?;
            if record.sequence
                != first_sequence
                    .checked_add(index as u64)
                    .ok_or(Error::LengthOverflow)?
            {
                return Err(Error::Invalid("non-consecutive Events records"));
            }
            records.push(record);
        }
        decoder.finish()?;
        Ok(Self {
            first_sequence,
            records,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordEvent {
    pub stream_handle: u64,
    pub batch: EventBatch,
}

impl Encode for RecordEvent {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.stream_handle, "Events stream handle")?;
        let batch = self.batch.encode()?;
        if batch.len() > MAX_LIVE_BATCH_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Events live batch bytes",
                actual: batch.len() as u64,
                maximum: MAX_LIVE_BATCH_BYTES as u64,
            });
        }
        put_u64(out, self.stream_handle);
        put_u16(out, EVENTS_CODEC_ID);
        put_u16(out, EVENTS_CODEC_VERSION);
        out.extend_from_slice(&batch);
        Ok(())
    }
}

impl Decode for RecordEvent {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let stream_handle = decoder.u64()?;
        if decoder.u16()? != EVENTS_CODEC_ID || decoder.u16()? != EVENTS_CODEC_VERSION {
            return Err(Error::Invalid("Events packed codec"));
        }
        if decoder.remaining() > MAX_LIVE_BATCH_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Events live batch bytes",
                actual: decoder.remaining() as u64,
                maximum: MAX_LIVE_BATCH_BYTES as u64,
            });
        }
        let value = Self {
            stream_handle,
            batch: EventBatch::decode(decoder.rest())?,
        };
        decoder.finish()?;
        validate_handle(value.stream_handle, "Events stream handle")?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    pub stream_handle: u64,
    pub lost: u64,
    pub first_available_sequence: u64,
}

impl Encode for Gap {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.stream_handle, "Events stream handle")?;
        if self.lost == 0 {
            return Err(Error::Invalid("zero Events stream gap"));
        }
        put_u64(out, self.stream_handle);
        put_u64(out, self.lost);
        put_u64(out, self.first_available_sequence);
        Ok(())
    }
}

impl Decode for Gap {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            stream_handle: decoder.u64()?,
            lost: decoder.u64()?,
            first_available_sequence: decoder.u64()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamStopped {
    pub stream_handle: u64,
    pub status: Status,
    pub detail: String,
    pub extensions: Extensions,
}

impl Encode for StreamStopped {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.stream_handle, "Events stream handle")?;
        if self.detail.len() > MAX_RECORD_ERROR_BYTES {
            return Err(Error::Invalid("Events stream-stopped detail"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.stream_handle);
        put_u16(out, self.status.code());
        put_u16(out, 0);
        put_string_u32(out, &self.detail)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for StreamStopped {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let stream_handle = decoder.u64()?;
        let status = Status::from_code(decoder.u16()?);
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Events stream-stopped reserved field"));
        }
        let value = Self {
            stream_handle,
            status,
            detail: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

fn known_event(event_id: u32) -> bool {
    event_id <= crate::schema::events::EVENT_SERVER_ERROR as u32
}

fn validate_path(path: &[u8]) -> Result<()> {
    if path.is_empty() || path.contains(&0) {
        return Err(Error::Invalid("Events recording path"));
    }
    if path.len() > MAX_RECORDING_PATH_BYTES {
        return Err(Error::LimitExceeded {
            limit: "Events recording path bytes",
            actual: path.len() as u64,
            maximum: MAX_RECORDING_PATH_BYTES as u64,
        });
    }
    Ok(())
}

fn validate_handle(value: u64, name: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(name))
    } else {
        Ok(())
    }
}

fn validate_operation_id(value: &[u8; 16]) -> Result<()> {
    if *value == [0; 16] {
        Err(Error::Invalid("zero Events operation ID"))
    } else {
        Ok(())
    }
}

fn reject_unknown_required(extensions: &Extensions, known: &[u16]) -> Result<()> {
    extensions.validate()?;
    if extensions
        .0
        .iter()
        .any(|extension| extension.required && !known.contains(&extension.tag))
    {
        return Err(Error::Invalid("unknown required Events extension"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Extension;

    fn round_trip<T>(value: T)
    where
        T: Encode + Decode + PartialEq + std::fmt::Debug,
    {
        let encoded = value.encode().unwrap();
        assert_eq!(T::decode(&encoded).unwrap(), value);
        for end in 0..encoded.len() {
            assert!(T::decode(&encoded[..end]).is_err(), "accepted prefix {end}");
        }
    }

    fn dump_descriptor() -> Descriptor {
        Descriptor {
            transfer_id: 2,
            mode: Mode::Byte,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: 65_536,
            max_item_bytes: 0,
            max_chunk_bytes: 65_536,
            content_family: crate::family::EVENTS,
            content_kind: crate::schema::events::DUMP_CONTENT_KIND as u16,
            content_version: VERSION,
            extensions: Extensions(vec![Extension {
                tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    #[test]
    fn config_and_dump_round_trip() {
        let config = Config {
            revision: 7,
            capacity: 1 << 20,
            used: 4096,
            record_count: 8,
            dropped: 2,
            next_sequence: 15,
            activations: ActivationSet([1, 2, 3, 4]),
            extensions: Extensions::default(),
        };
        round_trip(config.clone());
        round_trip(SetConfig {
            operation_id: [1; 16],
            expected_revision: config.revision,
            capacity: config.capacity,
            activations: config.activations,
            extensions: Extensions::default(),
        });
        round_trip(DumpResult {
            byte_len: 12_345,
            content_hash: [9; 32],
            descriptor: dump_descriptor(),
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn stream_and_recording_lifecycle_round_trip() {
        round_trip(StartStream {
            operation_id: [2; 16],
            history: true,
            start_sequence: 100,
            max_batch_bytes: MAX_LIVE_BATCH_BYTES as u32,
            extensions: Extensions::default(),
        });
        round_trip(StreamStarted {
            stream_handle: 10,
            first_sequence: 103,
            max_batch_bytes: MAX_LIVE_BATCH_BYTES as u32,
            extensions: Extensions::default(),
        });
        let info = RecordingInfo {
            recording_handle: 11,
            state: RecordingState::Failed,
            history: true,
            append: false,
            records: 42,
            bytes: 4096,
            lost: 3,
            path: b"/tmp/yas.events".to_vec(),
            error: "disk full".into(),
            extensions: Extensions::default(),
        };
        round_trip(StartRecording {
            operation_id: [3; 16],
            history: true,
            append: false,
            path: info.path.clone(),
            extensions: Extensions::default(),
        });
        round_trip(info.clone());
        round_trip(RecordingList {
            recordings: vec![info],
        });
    }

    #[test]
    fn event_batches_preserve_unknown_optional_records() {
        let batch = EventBatch {
            first_sequence: 50,
            records: vec![
                EventRecord {
                    sequence: 50,
                    monotonic_ns: 1_000,
                    event_id: crate::schema::events::EVENT_PTY_CREATE as u32,
                    required: true,
                    event_flags: 0x1234,
                    payload: vec![1, 2, 3],
                },
                EventRecord {
                    sequence: 51,
                    monotonic_ns: 1_001,
                    event_id: 999,
                    required: false,
                    event_flags: 0xabcd,
                    payload: vec![4, 5],
                },
            ],
        };
        round_trip(batch.clone());
        round_trip(RecordEvent {
            stream_handle: 12,
            batch,
        });
        let oversized = EventBatch {
            first_sequence: 1,
            records: vec![EventRecord {
                sequence: 1,
                monotonic_ns: 0,
                event_id: crate::schema::events::EVENT_PTY_CREATE as u32,
                required: true,
                event_flags: 0,
                payload: vec![0; MAX_LIVE_BATCH_BYTES],
            }],
        };
        assert!(
            RecordEvent {
                stream_handle: 12,
                batch: oversized.clone(),
            }
            .encode()
            .is_err()
        );
        let mut oversized_wire = Vec::new();
        put_u64(&mut oversized_wire, 12);
        put_u16(&mut oversized_wire, EVENTS_CODEC_ID);
        put_u16(&mut oversized_wire, EVENTS_CODEC_VERSION);
        oversized_wire.extend_from_slice(&oversized.encode().unwrap());
        assert!(RecordEvent::decode(&oversized_wire).is_err());
        assert!(
            EventBatch {
                first_sequence: 1,
                records: vec![EventRecord {
                    sequence: 1,
                    monotonic_ns: 0,
                    event_id: 999,
                    required: true,
                    event_flags: 0,
                    payload: Vec::new(),
                }],
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn gap_and_stop_round_trip() {
        round_trip(Gap {
            stream_handle: 13,
            lost: 7,
            first_available_sequence: 92,
        });
        round_trip(StreamStopped {
            stream_handle: 13,
            status: Status::Io,
            detail: "writer failed".into(),
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
        let mut invalid = Limits::HARD;
        invalid.min_ring_bytes = invalid.max_ring_bytes + 1;
        assert!(invalid.to_extensions().is_err());
        assert!(Limits::from_extensions(&Extensions::default()).is_err());
    }
}
