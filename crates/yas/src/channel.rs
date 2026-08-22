//! YAS named message-channel family wire values.

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, put_bytes_u32, put_string_u16,
    put_u16, put_u64,
};
use crate::prelude::*;
use crate::state::{Record, RecordKind};
use crate::transfer::{Descriptor, Direction, Mode};

pub const VERSION: u16 = crate::schema::channel::VERSION;
pub const CHANNEL_CONTENT_KIND: u16 = crate::schema::channel::CHANNEL_CONTENT_KIND as u16;
pub const MAX_NAME_BYTES: usize = crate::schema::channel::MAX_NAME_BYTES as usize;
pub const MAX_METADATA_BYTES: usize = crate::schema::channel::MAX_METADATA_BYTES as usize;
pub const MAX_LISTENERS_PER_SESSION: u32 = crate::schema::channel::MAX_LISTENERS_PER_SESSION as u32;
pub const MAX_CHANNELS_PER_SESSION: u32 = crate::schema::channel::MAX_CHANNELS_PER_SESSION as u32;
pub const MAX_PENDING_CONNECTS: u32 = crate::schema::channel::MAX_PENDING_CONNECTS as u32;
pub const MAX_MESSAGE_BYTES: u64 = crate::schema::channel::MAX_MESSAGE_BYTES;
pub const MAX_OPEN_MESSAGES: u32 = crate::schema::channel::MAX_OPEN_MESSAGES as u32;
pub const CONNECT_TIMEOUT_NS: u64 = crate::schema::channel::CONNECT_TIMEOUT_NS;

pub mod request_kind {
    pub use crate::schema::channel::request::*;
}

pub mod event_kind {
    pub use crate::schema::channel::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OwnerKind {
    Session = crate::schema::channel::OWNER_SESSION as u8,
    Extension = crate::schema::channel::OWNER_EXTENSION as u8,
}

impl TryFrom<u8> for OwnerKind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::channel::OWNER_SESSION as u8 => Ok(Self::Session),
            value if value == crate::schema::channel::OWNER_EXTENSION as u8 => Ok(Self::Extension),
            _ => Err(Error::Invalid("Channel owner kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenerRecord {
    pub listener_handle: u64,
    pub generation: u64,
    pub owner_kind: OwnerKind,
    pub owner_session: [u8; 16],
    pub name: String,
    pub metadata: Vec<u8>,
    pub extensions: Extensions,
}

impl ListenerRecord {
    fn validate(&self) -> Result<()> {
        validate_identity(self.listener_handle, self.generation, "Channel listener")?;
        validate_session_id(&self.owner_session)?;
        validate_name(&self.name)?;
        validate_metadata(&self.metadata)?;
        reject_unknown_required(&self.extensions, &[])
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Channel listener state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Channel listener state record kind"));
        }
        Self::decode(&record.body)
    }
}

impl Encode for ListenerRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.listener_handle);
        put_u64(out, self.generation);
        out.push(self.owner_kind as u8);
        out.push(0);
        put_u16(out, 0);
        out.extend_from_slice(&self.owner_session);
        put_string_u16(out, &self.name)?;
        put_bytes_u32(out, &self.metadata)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for ListenerRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let listener_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let owner_kind = OwnerKind::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Channel listener reserved fields"));
        }
        let value = Self {
            listener_handle,
            generation,
            owner_kind,
            owner_session: decoder.array_16()?,
            name: decoder.string_u16()?,
            metadata: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedListener {
    pub listener_handle: u64,
    pub generation: u64,
}

impl RemovedListener {
    pub fn state_record(self) -> Result<Record> {
        validate_identity(self.listener_handle, self.generation, "Channel listener")?;
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body: self.encode()?,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if record.kind != RecordKind::Remove {
            return Err(Error::Invalid("Channel listener remove record kind"));
        }
        Self::decode(&record.body)
    }
}

impl Encode for RemovedListener {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_identity(self.listener_handle, self.generation, "Channel listener")?;
        put_u64(out, self.listener_handle);
        put_u64(out, self.generation);
        Ok(())
    }
}

impl Decode for RemovedListener {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            listener_handle: decoder.u64()?,
            generation: decoder.u64()?,
        };
        decoder.finish()?;
        validate_identity(value.listener_handle, value.generation, "Channel listener")?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listen {
    pub operation_id: [u8; 16],
    pub name: String,
    pub metadata: Vec<u8>,
    pub extensions: Extensions,
}

impl Listen {
    fn validate(&self) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        validate_name(&self.name)?;
        validate_metadata(&self.metadata)?;
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for Listen {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.extend_from_slice(&self.operation_id);
        put_string_u16(out, &self.name)?;
        put_bytes_u32(out, &self.metadata)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Listen {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            operation_id: decoder.array_16()?,
            name: decoder.string_u16()?,
            metadata: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenerIdentity {
    pub listener_handle: u64,
    pub generation: u64,
    pub extensions: Extensions,
}

impl ListenerIdentity {
    fn validate(&self) -> Result<()> {
        validate_identity(self.listener_handle, self.generation, "Channel listener")?;
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for ListenerIdentity {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.listener_handle);
        put_u64(out, self.generation);
        self.extensions.encode_tail(out)
    }
}

impl Decode for ListenerIdentity {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            listener_handle: decoder.u64()?,
            generation: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

pub type ListenResult = ListenerIdentity;
pub type CloseListener = ListenerIdentity;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connect {
    pub listener_handle: u64,
    pub generation: u64,
    pub initial_receive_credit: u64,
    pub metadata: Vec<u8>,
    pub extensions: Extensions,
}

impl Connect {
    fn validate(&self) -> Result<()> {
        validate_identity(self.listener_handle, self.generation, "Channel listener")?;
        if self.initial_receive_credit == 0 {
            return Err(Error::Invalid("zero Channel receive credit"));
        }
        validate_metadata(&self.metadata)?;
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for Connect {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.listener_handle);
        put_u64(out, self.generation);
        put_u64(out, self.initial_receive_credit);
        put_bytes_u32(out, &self.metadata)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Connect {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            listener_handle: decoder.u64()?,
            generation: decoder.u64()?,
            initial_receive_credit: decoder.u64()?,
            metadata: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelEndpoint {
    pub channel_handle: u64,
    pub peer_channel_handle: u64,
    pub peer_session: [u8; 16],
    pub listener_metadata: Vec<u8>,
    pub connector_metadata: Vec<u8>,
    pub descriptor: Descriptor,
    pub extensions: Extensions,
}

impl ChannelEndpoint {
    fn validate(&self) -> Result<()> {
        validate_handle(self.channel_handle, "Channel handle")?;
        validate_handle(self.peer_channel_handle, "peer Channel handle")?;
        validate_session_id(&self.peer_session)?;
        validate_metadata(&self.listener_metadata)?;
        validate_metadata(&self.connector_metadata)?;
        validate_channel_transfer(&self.descriptor)?;
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for ChannelEndpoint {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.channel_handle);
        put_u64(out, self.peer_channel_handle);
        out.extend_from_slice(&self.peer_session);
        put_bytes_u32(out, &self.listener_metadata)?;
        put_bytes_u32(out, &self.connector_metadata)?;
        let descriptor = self.descriptor.encode()?;
        put_bytes_u32(out, &descriptor)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for ChannelEndpoint {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let channel_handle = decoder.u64()?;
        let peer_channel_handle = decoder.u64()?;
        let peer_session = decoder.array_16()?;
        let listener_metadata = decoder.len_bytes_u32()?.to_vec();
        let connector_metadata = decoder.len_bytes_u32()?.to_vec();
        let descriptor = Descriptor::decode(decoder.len_bytes_u32()?)?;
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            channel_handle,
            peer_channel_handle,
            peer_session,
            listener_metadata,
            connector_metadata,
            descriptor,
            extensions,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accept {
    pub listener_handle: u64,
    pub generation: u64,
    pub endpoint: ChannelEndpoint,
}

impl Encode for Accept {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_identity(self.listener_handle, self.generation, "Channel listener")?;
        self.endpoint.validate()?;
        if self.endpoint.descriptor.sender_send_credit != 0 {
            return Err(Error::Invalid("Channel ACCEPT initial send credit"));
        }
        put_u64(out, self.listener_handle);
        put_u64(out, self.generation);
        self.endpoint.encode_to(out)
    }
}

impl Decode for Accept {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let listener_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let endpoint = ChannelEndpoint::decode(decoder.rest())?;
        decoder.finish()?;
        let value = Self {
            listener_handle,
            generation,
            endpoint,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_name_bytes: u32,
    pub max_metadata_bytes: u32,
    pub max_listeners_per_session: u32,
    pub max_channels_per_session: u32,
    pub max_pending_connects: u32,
    pub max_message_bytes: u64,
    pub max_open_messages: u32,
    pub connect_timeout_ns: u64,
    pub max_mutation_replays: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_name_bytes: MAX_NAME_BYTES as u32,
        max_metadata_bytes: MAX_METADATA_BYTES as u32,
        max_listeners_per_session: MAX_LISTENERS_PER_SESSION,
        max_channels_per_session: MAX_CHANNELS_PER_SESSION,
        max_pending_connects: MAX_PENDING_CONNECTS,
        max_message_bytes: MAX_MESSAGE_BYTES,
        max_open_messages: MAX_OPEN_MESSAGES,
        connect_timeout_ns: CONNECT_TIMEOUT_NS,
        max_mutation_replays: crate::schema::channel::MAX_MUTATION_REPLAYS as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        if self.max_name_bytes == 0 || self.max_name_bytes > hard.max_name_bytes {
            return Err(Error::Invalid("Channel max name bytes limit"));
        }
        if self.max_metadata_bytes > hard.max_metadata_bytes {
            return Err(Error::Invalid("Channel max metadata bytes limit"));
        }
        if self.max_listeners_per_session == 0
            || self.max_listeners_per_session > hard.max_listeners_per_session
        {
            return Err(Error::Invalid("Channel max listeners limit"));
        }
        if self.max_channels_per_session == 0
            || self.max_channels_per_session > hard.max_channels_per_session
        {
            return Err(Error::Invalid("Channel max channels limit"));
        }
        if self.max_pending_connects == 0 || self.max_pending_connects > hard.max_pending_connects {
            return Err(Error::Invalid("Channel max pending connects limit"));
        }
        if self.max_message_bytes == 0 || self.max_message_bytes > hard.max_message_bytes {
            return Err(Error::Invalid("Channel max message bytes limit"));
        }
        if self.max_open_messages == 0 || self.max_open_messages > hard.max_open_messages {
            return Err(Error::Invalid("Channel max open messages limit"));
        }
        if self.connect_timeout_ns == 0 || self.connect_timeout_ns > hard.connect_timeout_ns {
            return Err(Error::Invalid("Channel connect timeout limit"));
        }
        if self.max_mutation_replays == 0 || self.max_mutation_replays > hard.max_mutation_replays {
            return Err(Error::Invalid("Channel mutation replay limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::channel::LIMIT_MAX_NAME_BYTES,
                self.max_name_bytes,
            ),
            limit_u32(
                crate::schema::channel::LIMIT_MAX_METADATA_BYTES,
                self.max_metadata_bytes,
            ),
            limit_u32(
                crate::schema::channel::LIMIT_MAX_LISTENERS_PER_SESSION,
                self.max_listeners_per_session,
            ),
            limit_u32(
                crate::schema::channel::LIMIT_MAX_CHANNELS_PER_SESSION,
                self.max_channels_per_session,
            ),
            limit_u32(
                crate::schema::channel::LIMIT_MAX_PENDING_CONNECTS,
                self.max_pending_connects,
            ),
            limit_u64(
                crate::schema::channel::LIMIT_MAX_MESSAGE_BYTES,
                self.max_message_bytes,
            ),
            limit_u32(
                crate::schema::channel::LIMIT_MAX_OPEN_MESSAGES,
                self.max_open_messages,
            ),
            limit_u64(
                crate::schema::channel::LIMIT_CONNECT_TIMEOUT_NS,
                self.connect_timeout_ns,
            ),
            limit_u32(
                crate::schema::channel::LIMIT_MAX_MUTATION_REPLAYS,
                self.max_mutation_replays,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required(
            extensions,
            &[
                crate::schema::channel::LIMIT_MAX_NAME_BYTES as u16,
                crate::schema::channel::LIMIT_MAX_METADATA_BYTES as u16,
                crate::schema::channel::LIMIT_MAX_LISTENERS_PER_SESSION as u16,
                crate::schema::channel::LIMIT_MAX_CHANNELS_PER_SESSION as u16,
                crate::schema::channel::LIMIT_MAX_PENDING_CONNECTS as u16,
                crate::schema::channel::LIMIT_MAX_MESSAGE_BYTES as u16,
                crate::schema::channel::LIMIT_MAX_OPEN_MESSAGES as u16,
                crate::schema::channel::LIMIT_CONNECT_TIMEOUT_NS as u16,
                crate::schema::channel::LIMIT_MAX_MUTATION_REPLAYS as u16,
            ],
        )?;
        let value = Self {
            max_name_bytes: read_limit_u32(
                extensions,
                crate::schema::channel::LIMIT_MAX_NAME_BYTES,
            )?,
            max_metadata_bytes: read_limit_u32(
                extensions,
                crate::schema::channel::LIMIT_MAX_METADATA_BYTES,
            )?,
            max_listeners_per_session: read_limit_u32(
                extensions,
                crate::schema::channel::LIMIT_MAX_LISTENERS_PER_SESSION,
            )?,
            max_channels_per_session: read_limit_u32(
                extensions,
                crate::schema::channel::LIMIT_MAX_CHANNELS_PER_SESSION,
            )?,
            max_pending_connects: read_limit_u32(
                extensions,
                crate::schema::channel::LIMIT_MAX_PENDING_CONNECTS,
            )?,
            max_message_bytes: read_limit_u64(
                extensions,
                crate::schema::channel::LIMIT_MAX_MESSAGE_BYTES,
            )?,
            max_open_messages: read_limit_u32(
                extensions,
                crate::schema::channel::LIMIT_MAX_OPEN_MESSAGES,
            )?,
            connect_timeout_ns: read_limit_u64(
                extensions,
                crate::schema::channel::LIMIT_CONNECT_TIMEOUT_NS,
            )?,
            max_mutation_replays: read_limit_u32(
                extensions,
                crate::schema::channel::LIMIT_MAX_MUTATION_REPLAYS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn validate_channel_transfer(descriptor: &Descriptor) -> Result<()> {
    let sensitive = descriptor.extensions.0.iter().any(|extension| {
        extension.tag == crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16
            && extension.required
            && extension.value.is_empty()
    });
    if descriptor.mode != Mode::Message
        || descriptor.direction != Direction::BIDIRECTIONAL
        || descriptor.max_item_bytes == 0
        || descriptor.max_item_bytes > MAX_MESSAGE_BYTES
        || descriptor.max_open_messages()? > MAX_OPEN_MESSAGES
        || descriptor.content_family != crate::family::CHANNEL
        || descriptor.content_kind != CHANNEL_CONTENT_KIND
        || descriptor.content_version != VERSION
        || !sensitive
    {
        return Err(Error::Invalid("Channel message Transfer descriptor"));
    }
    descriptor.validate()
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name.as_bytes().contains(&0) {
        return Err(Error::Invalid("Channel name"));
    }
    Ok(())
}

fn validate_metadata(metadata: &[u8]) -> Result<()> {
    if metadata.len() > MAX_METADATA_BYTES {
        return Err(limit(
            "Channel metadata bytes",
            metadata.len() as u64,
            MAX_METADATA_BYTES as u64,
        ));
    }
    Ok(())
}

fn validate_operation_id(operation_id: &[u8; 16]) -> Result<()> {
    if operation_id.iter().all(|byte| *byte == 0) {
        return Err(Error::Invalid("zero Channel operation ID"));
    }
    Ok(())
}

fn validate_session_id(session_id: &[u8; 16]) -> Result<()> {
    if session_id.iter().all(|byte| *byte == 0) {
        return Err(Error::Invalid("zero Channel session ID"));
    }
    Ok(())
}

fn validate_identity(handle: u64, generation: u64, name: &'static str) -> Result<()> {
    validate_handle(handle, name)?;
    if generation == 0 {
        return Err(Error::Invalid("zero Channel generation"));
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
        return Err(Error::Invalid("unknown required Channel extension"));
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

fn read_limit_u32(extensions: &Extensions, tag: u64) -> Result<u32> {
    let extension = extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .ok_or(Error::Invalid("missing Channel family limit"))?;
    Ok(u32::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("Channel family limit length"))?,
    ))
}

fn read_limit_u64(extensions: &Extensions, tag: u64) -> Result<u64> {
    let extension = extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .ok_or(Error::Invalid("missing Channel family limit"))?;
    Ok(u64::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("Channel family limit length"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Extension;

    fn sensitive_descriptor(sender_credit: u64) -> Descriptor {
        Descriptor {
            transfer_id: 2,
            mode: Mode::Message,
            direction: Direction::BIDIRECTIONAL,
            receiver_send_credit: 65_536,
            sender_send_credit: sender_credit,
            max_item_bytes: 1024,
            max_chunk_bytes: 1024,
            content_family: crate::family::CHANNEL,
            content_kind: CHANNEL_CONTENT_KIND,
            content_version: VERSION,
            extensions: Extensions(vec![
                Extension {
                    tag: crate::schema::transfer::MAX_OPEN_MESSAGES_EXTENSION as u16,
                    required: false,
                    value: 2u32.to_le_bytes().to_vec(),
                },
                Extension {
                    tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                    required: true,
                    value: Vec::new(),
                },
            ]),
        }
    }

    fn endpoint(sender_credit: u64) -> ChannelEndpoint {
        ChannelEndpoint {
            channel_handle: 3,
            peer_channel_handle: 4,
            peer_session: [7; 16],
            listener_metadata: b"listener".to_vec(),
            connector_metadata: b"connector".to_vec(),
            descriptor: sensitive_descriptor(sender_credit),
            extensions: Extensions::default(),
        }
    }

    fn every_truncation<T>(value: &T)
    where
        T: Encode + Decode + PartialEq + std::fmt::Debug,
    {
        let bytes = value.encode().unwrap();
        assert_eq!(T::decode(&bytes).unwrap(), *value);
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
    }

    #[test]
    fn listener_record_and_remove_round_trip() {
        let record = ListenerRecord {
            listener_handle: 1,
            generation: 2,
            owner_kind: OwnerKind::Extension,
            owner_session: [3; 16],
            name: "commands.build".into(),
            metadata: vec![4, 5],
            extensions: Extensions::default(),
        };
        every_truncation(&record);
        assert_eq!(
            ListenerRecord::from_state_record(&record.state_record(RecordKind::Add).unwrap())
                .unwrap(),
            record
        );
        let removed = RemovedListener {
            listener_handle: 1,
            generation: 2,
        };
        every_truncation(&removed);
        assert_eq!(
            RemovedListener::from_state_record(&removed.state_record().unwrap()).unwrap(),
            removed
        );
    }

    #[test]
    fn requests_and_endpoint_reject_every_truncation() {
        every_truncation(&Listen {
            operation_id: [1; 16],
            name: "rpc.echo".into(),
            metadata: b"schema-v1".to_vec(),
            extensions: Extensions::default(),
        });
        every_truncation(&ListenerIdentity {
            listener_handle: 1,
            generation: 2,
            extensions: Extensions::default(),
        });
        every_truncation(&Connect {
            listener_handle: 1,
            generation: 2,
            initial_receive_credit: 4096,
            metadata: b"client".to_vec(),
            extensions: Extensions::default(),
        });
        every_truncation(&endpoint(4096));
        every_truncation(&Accept {
            listener_handle: 1,
            generation: 2,
            endpoint: endpoint(0),
        });
    }

    #[test]
    fn transfer_and_accept_policy_are_enforced() {
        let mut value = endpoint(0);
        value.descriptor.mode = Mode::Byte;
        assert!(value.encode().is_err());
        let value = Accept {
            listener_handle: 1,
            generation: 2,
            endpoint: endpoint(1),
        };
        assert!(value.encode().is_err());
    }

    #[test]
    fn hard_limits_round_trip() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
        let mut invalid = Limits::HARD;
        invalid.max_message_bytes += 1;
        assert!(invalid.to_extensions().is_err());
    }

    #[test]
    fn metadata_accepts_the_64_kib_boundary() {
        let mut listen = Listen {
            operation_id: [1; 16],
            name: "rpc.boundary".into(),
            metadata: vec![0x5a; MAX_METADATA_BYTES],
            extensions: Extensions::default(),
        };
        let bytes = listen.encode().unwrap();
        assert_eq!(Listen::decode(&bytes).unwrap(), listen);
        listen.metadata.push(0);
        assert!(matches!(
            listen.encode(),
            Err(Error::LimitExceeded {
                limit: "Channel metadata bytes",
                actual: 65_537,
                maximum: 65_536,
            })
        ));
    }
}
