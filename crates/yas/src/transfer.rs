use crate::prelude::*;

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, put_len_u32, put_u16, put_u32,
    put_u64, reject_unknown_required_extensions,
};
use crate::frame::HARD_MAX_BULK_CHUNK;

pub const VERSION: u16 = crate::schema::transfer::VERSION;

pub mod kind {
    pub use crate::schema::transfer::event::*;
}

pub const MAX_UPLOAD_STAGE_LIFETIME_NS: u64 = crate::schema::transfer::MAX_UPLOAD_STAGE_LIFETIME_NS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadStage {
    pub staging_handle: u64,
    pub expires_server_ns: u64,
}

/// Family-scoped identity used when retiring every Transfer in one upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UploadStageKey {
    pub content_family: u16,
    pub staging_handle: u64,
}

impl UploadStage {
    pub fn validate(self) -> Result<()> {
        if self.staging_handle == 0 || self.expires_server_ns == 0 {
            return Err(Error::Invalid("Transfer upload-stage extension"));
        }
        Ok(())
    }

    /// Validate a newly allocated stage against the canonical maximum
    /// lifetime. Receivers need not reject a descriptor merely because it
    /// expired in transit; operations using that stage return NOT_FOUND.
    pub fn validate_at(self, created_server_ns: u64) -> Result<()> {
        self.validate()?;
        let lifetime = self
            .expires_server_ns
            .checked_sub(created_server_ns)
            .ok_or(Error::Invalid("Transfer upload-stage expiry"))?;
        if lifetime == 0 || lifetime > MAX_UPLOAD_STAGE_LIFETIME_NS {
            return Err(Error::Invalid("Transfer upload-stage expiry"));
        }
        Ok(())
    }

    /// Whether the uncommitted stage must be garbage-collected at this Core
    /// monotonic time. Expiry has the same whole-stage effect as RESET.
    pub fn is_expired_at(self, now_server_ns: u64) -> Result<bool> {
        self.validate()?;
        Ok(now_server_ns >= self.expires_server_ns)
    }

    pub fn extension(self) -> Result<Extension> {
        self.validate()?;
        let mut value = Vec::with_capacity(16);
        put_u64(&mut value, self.staging_handle);
        put_u64(&mut value, self.expires_server_ns);
        Ok(Extension {
            tag: crate::schema::transfer::UPLOAD_STAGE_EXTENSION as u16,
            required: true,
            value,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Byte = crate::schema::transfer::MODE_BYTE as u8,
    Message = crate::schema::transfer::MODE_MESSAGE as u8,
}

impl TryFrom<u8> for Mode {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::transfer::MODE_BYTE as u8 => Ok(Self::Byte),
            value if value == crate::schema::transfer::MODE_MESSAGE as u8 => Ok(Self::Message),
            _ => Err(Error::Invalid("transfer mode")),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Direction {
    pub receiver_to_sender: bool,
    pub sender_to_receiver: bool,
}

impl Direction {
    pub const RECEIVER_TO_SENDER: Self = Self {
        receiver_to_sender: true,
        sender_to_receiver: false,
    };
    pub const SENDER_TO_RECEIVER: Self = Self {
        receiver_to_sender: false,
        sender_to_receiver: true,
    };
    pub const BIDIRECTIONAL: Self = Self {
        receiver_to_sender: true,
        sender_to_receiver: true,
    };

    fn bits(self) -> u8 {
        (u8::from(self.receiver_to_sender)
            * crate::schema::transfer::DIRECTION_RECEIVER_TO_SENDER as u8)
            | (u8::from(self.sender_to_receiver)
                * crate::schema::transfer::DIRECTION_SENDER_TO_RECEIVER as u8)
    }

    fn from_bits(bits: u8) -> Result<Self> {
        let known = (crate::schema::transfer::DIRECTION_RECEIVER_TO_SENDER
            | crate::schema::transfer::DIRECTION_SENDER_TO_RECEIVER) as u8;
        if bits & !known != 0 || bits == 0 {
            return Err(Error::Invalid("transfer direction"));
        }
        Ok(Self {
            receiver_to_sender: bits & crate::schema::transfer::DIRECTION_RECEIVER_TO_SENDER as u8
                != 0,
            sender_to_receiver: bits & crate::schema::transfer::DIRECTION_SENDER_TO_RECEIVER as u8
                != 0,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub transfer_id: u32,
    pub mode: Mode,
    pub direction: Direction,
    pub receiver_send_credit: u64,
    pub sender_send_credit: u64,
    pub max_item_bytes: u64,
    pub max_chunk_bytes: u32,
    pub content_family: u16,
    pub content_kind: u16,
    pub content_version: u16,
    pub extensions: Extensions,
}

impl Descriptor {
    pub fn sensitive_content(&self) -> Result<bool> {
        let Some(extension) = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16
        }) else {
            return Ok(false);
        };
        if !extension.required || !extension.value.is_empty() {
            return Err(Error::Invalid("Transfer sensitive-content extension"));
        }
        Ok(true)
    }

    pub fn max_open_messages(&self) -> Result<u32> {
        let Some(extension) = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::transfer::MAX_OPEN_MESSAGES_EXTENSION as u16
        }) else {
            return Ok(1);
        };
        if extension.value.len() != 4 {
            return Err(Error::Invalid("Transfer max-open-messages extension"));
        }
        let value = u32::from_le_bytes(extension.value.as_slice().try_into().unwrap());
        if value == 0 {
            return Err(Error::Invalid("zero Transfer max-open-messages"));
        }
        Ok(value)
    }

    pub fn upload_stage(&self) -> Result<Option<UploadStage>> {
        let Some(extension) = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::transfer::UPLOAD_STAGE_EXTENSION as u16
        }) else {
            return Ok(None);
        };
        if !extension.required || extension.value.len() != 16 {
            return Err(Error::Invalid("Transfer upload-stage extension"));
        }
        let stage = UploadStage {
            staging_handle: u64::from_le_bytes(extension.value[..8].try_into().unwrap()),
            expires_server_ns: u64::from_le_bytes(extension.value[8..].try_into().unwrap()),
        };
        stage.validate()?;
        Ok(Some(stage))
    }

    /// Return the family-scoped upload identity used for sibling cleanup.
    pub fn upload_stage_key(&self) -> Result<Option<UploadStageKey>> {
        Ok(self.upload_stage()?.map(|stage| UploadStageKey {
            content_family: self.content_family,
            staging_handle: stage.staging_handle,
        }))
    }

    pub fn require_upload_stage(&self, staging_handle: u64) -> Result<UploadStage> {
        match self.upload_stage()? {
            Some(stage) if stage.staging_handle == staging_handle => Ok(stage),
            _ => Err(Error::Invalid("Transfer upload-stage identity")),
        }
    }

    /// Whether this descriptor requires the SENSITIVE frame flag for the
    /// exact Transfer Event kind. Session managers call this after resolving
    /// `transfer_id`; static frame policy cannot classify Transfer content.
    pub fn requires_sensitive_frame(&self, kind: u16) -> Result<bool> {
        match kind {
            kind::BYTE_DATA | kind::MESSAGE_DATA | kind::CLOSE | kind::RESET => {
                self.sensitive_content()
            }
            kind::CREDIT => Ok(false),
            _ => Err(Error::Invalid("Transfer event kind")),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.transfer_id == 0 {
            return Err(Error::Invalid("zero transfer ID"));
        }
        if !self.direction.receiver_to_sender && self.receiver_send_credit != 0 {
            return Err(Error::Invalid("credit for disallowed transfer direction"));
        }
        if !self.direction.sender_to_receiver && self.sender_send_credit != 0 {
            return Err(Error::Invalid("credit for disallowed transfer direction"));
        }
        match self.mode {
            Mode::Byte if self.max_item_bytes != 0 => {
                return Err(Error::Invalid("BYTE max item bytes"));
            }
            Mode::Message if self.max_item_bytes == 0 => {
                return Err(Error::Invalid("MESSAGE max item bytes"));
            }
            _ => {}
        }
        if self.max_chunk_bytes == 0 || self.max_chunk_bytes > HARD_MAX_BULK_CHUNK {
            return Err(Error::Invalid("transfer max chunk bytes"));
        }
        reject_unknown_required_extensions(
            &self.extensions,
            &[
                crate::schema::transfer::MAX_OPEN_MESSAGES_EXTENSION as u16,
                crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                crate::schema::transfer::UPLOAD_STAGE_EXTENSION as u16,
            ],
            "unknown required Transfer descriptor extension",
        )?;
        let has_message_extension = self.extensions.0.iter().any(|extension| {
            extension.tag == crate::schema::transfer::MAX_OPEN_MESSAGES_EXTENSION as u16
        });
        if self.mode == Mode::Byte && has_message_extension {
            return Err(Error::Invalid("BYTE max-open-messages extension"));
        }
        self.max_open_messages()?;
        let sensitive = self.sensitive_content()?;
        if self.upload_stage()?.is_some()
            && (self.mode != Mode::Byte
                || self.direction != Direction::RECEIVER_TO_SENDER
                || self.sender_send_credit != 0
                || !sensitive)
        {
            return Err(Error::Invalid("Transfer upload-stage descriptor"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    Inline(Vec<u8>),
    Transfer(Descriptor),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineOrTransfer {
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub delivery: Delivery,
}

impl Encode for InlineOrTransfer {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        match &self.delivery {
            Delivery::Inline(bytes) if bytes.len() as u64 == self.byte_len => {
                out.push(crate::schema::transfer::DELIVERY_INLINE as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, self.byte_len);
                out.extend_from_slice(&self.content_hash);
                put_len_u32(out, bytes.len())?;
                out.extend_from_slice(bytes);
            }
            Delivery::Inline(_) => return Err(Error::Invalid("inline delivery length")),
            Delivery::Transfer(descriptor)
                if descriptor.mode == Mode::Byte && descriptor.max_item_bytes == 0 =>
            {
                out.push(crate::schema::transfer::DELIVERY_TRANSFER as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, self.byte_len);
                out.extend_from_slice(&self.content_hash);
                descriptor.encode_to(out)?;
            }
            Delivery::Transfer(_) => return Err(Error::Invalid("non-BYTE delivery transfer")),
        }
        Ok(())
    }
}

impl Decode for InlineOrTransfer {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("delivery reserved bytes"));
        }
        let byte_len = decoder.u64()?;
        let content_hash = decoder.array_32()?;
        let delivery = match kind {
            value if value == crate::schema::transfer::DELIVERY_INLINE as u8 => {
                Delivery::Inline(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::transfer::DELIVERY_TRANSFER as u8 => {
                Delivery::Transfer(Descriptor::decode(decoder.rest())?)
            }
            _ => return Err(Error::Invalid("delivery kind")),
        };
        decoder.finish()?;
        let value = Self {
            byte_len,
            content_hash,
            delivery,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

impl Encode for Descriptor {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.transfer_id);
        out.push(self.mode as u8);
        out.push(self.direction.bits());
        put_u16(out, 0);
        put_u64(out, self.receiver_send_credit);
        put_u64(out, self.sender_send_credit);
        put_u64(out, self.max_item_bytes);
        put_u32(out, self.max_chunk_bytes);
        put_u16(out, self.content_family);
        put_u16(out, self.content_kind);
        put_u16(out, self.content_version);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Descriptor {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let transfer_id = decoder.u32()?;
        let mode = Mode::try_from(decoder.u8()?)?;
        let direction = Direction::from_bits(decoder.u8()?)?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("transfer descriptor flags"));
        }
        let value = Self {
            transfer_id,
            mode,
            direction,
            receiver_send_credit: decoder.u64()?,
            sender_send_credit: decoder.u64()?,
            max_item_bytes: decoder.u64()?,
            max_chunk_bytes: decoder.u32()?,
            content_family: decoder.u16()?,
            content_kind: decoder.u16()?,
            content_version: decoder.u16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteData {
    pub transfer_id: u32,
    pub offset: u64,
    pub data: Vec<u8>,
}

impl Encode for ByteData {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.transfer_id == 0
            || self.data.is_empty()
            || self.data.len() > HARD_MAX_BULK_CHUNK as usize
        {
            return Err(Error::Invalid("BYTE_DATA"));
        }
        put_u32(out, self.transfer_id);
        put_u64(out, self.offset);
        out.extend_from_slice(&self.data);
        Ok(())
    }
}

/// Stateful MESSAGE fragment validator for one receive direction.
#[derive(Clone, Debug)]
pub struct MessageReceiver {
    max_item_bytes: u64,
    max_open_messages: u32,
    open: BTreeMap<u64, u64>,
}

impl MessageReceiver {
    pub fn new(descriptor: &Descriptor) -> Result<Self> {
        descriptor.validate()?;
        if descriptor.mode != Mode::Message {
            return Err(Error::Invalid("MESSAGE receiver for BYTE transfer"));
        }
        Ok(Self {
            max_item_bytes: descriptor.max_item_bytes,
            max_open_messages: descriptor.max_open_messages()?,
            open: BTreeMap::new(),
        })
    }

    /// Accept one validated fragment. Returns true when it completed a
    /// message. Callers account `data.len()` against cumulative byte credit.
    pub fn accept(&mut self, fragment: &MessageData) -> Result<bool> {
        if fragment.data.is_empty() {
            return Err(Error::Invalid("empty MESSAGE_DATA fragment"));
        }
        let data_len = fragment.data.len() as u64;
        if fragment.start {
            if fragment.fragment_offset != 0 || self.open.contains_key(&fragment.sequence) {
                return Err(Error::Invalid("MESSAGE START"));
            }
            if self.open.len() >= self.max_open_messages as usize {
                return Err(Error::Invalid("too many open messages"));
            }
            if data_len > self.max_item_bytes {
                return Err(Error::LimitExceeded {
                    limit: "Transfer message item",
                    actual: data_len,
                    maximum: self.max_item_bytes,
                });
            }
            if fragment.end {
                return Ok(true);
            }
            self.open.insert(fragment.sequence, data_len);
            return Ok(false);
        }
        let next = self
            .open
            .get_mut(&fragment.sequence)
            .ok_or(Error::Invalid("MESSAGE fragment without START"))?;
        if fragment.fragment_offset != *next {
            return Err(Error::Invalid("MESSAGE fragment offset"));
        }
        *next = next.checked_add(data_len).ok_or(Error::LengthOverflow)?;
        if *next > self.max_item_bytes {
            return Err(Error::LimitExceeded {
                limit: "Transfer message item",
                actual: *next,
                maximum: self.max_item_bytes,
            });
        }
        if fragment.end {
            self.open.remove(&fragment.sequence);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn open_messages(&self) -> usize {
        self.open.len()
    }
}

impl Decode for ByteData {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let transfer_id = decoder.u32()?;
        let offset = decoder.u64()?;
        let data = decoder.rest().to_vec();
        decoder.finish()?;
        let value = Self {
            transfer_id,
            offset,
            data,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

pub const MESSAGE_START: u8 = crate::schema::transfer::MESSAGE_START as u8;
pub const MESSAGE_END: u8 = crate::schema::transfer::MESSAGE_END as u8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageData {
    pub transfer_id: u32,
    pub sequence: u64,
    pub fragment_offset: u64,
    pub start: bool,
    pub end: bool,
    pub data: Vec<u8>,
}

impl Encode for MessageData {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.transfer_id == 0
            || self.data.is_empty()
            || self.data.len() > HARD_MAX_BULK_CHUNK as usize
        {
            return Err(Error::Invalid("MESSAGE_DATA"));
        }
        put_u32(out, self.transfer_id);
        put_u64(out, self.sequence);
        put_u64(out, self.fragment_offset);
        out.push((u8::from(self.start) * MESSAGE_START) | (u8::from(self.end) * MESSAGE_END));
        out.extend_from_slice(&[0; 3]);
        out.extend_from_slice(&self.data);
        Ok(())
    }
}

impl Decode for MessageData {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let transfer_id = decoder.u32()?;
        let sequence = decoder.u64()?;
        let fragment_offset = decoder.u64()?;
        let flags = decoder.u8()?;
        if flags & !(MESSAGE_START | MESSAGE_END) != 0 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("MESSAGE_DATA flags or reserved bytes"));
        }
        let data = decoder.rest().to_vec();
        decoder.finish()?;
        let value = Self {
            transfer_id,
            sequence,
            fragment_offset,
            start: flags & MESSAGE_START != 0,
            end: flags & MESSAGE_END != 0,
            data,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Credit {
    pub transfer_id: u32,
    pub cumulative_limit: u64,
}

impl Encode for Credit {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.transfer_id == 0 {
            return Err(Error::Invalid("zero transfer ID"));
        }
        put_u32(out, self.transfer_id);
        put_u64(out, self.cumulative_limit);
        Ok(())
    }
}

impl Decode for Credit {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            transfer_id: decoder.u32()?,
            cumulative_limit: decoder.u64()?,
        };
        decoder.finish()?;
        if value.transfer_id == 0 {
            return Err(Error::Invalid("zero transfer ID"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Close {
    pub transfer_id: u32,
    pub final_data_bytes: u64,
    pub status: u16,
    pub detail: Vec<u8>,
}

impl Encode for Close {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.transfer_id == 0 {
            return Err(Error::Invalid("zero transfer ID"));
        }
        put_u32(out, self.transfer_id);
        put_u64(out, self.final_data_bytes);
        put_u16(out, self.status);
        put_u16(out, 0);
        put_len_u32(out, self.detail.len())?;
        out.extend_from_slice(&self.detail);
        Ok(())
    }
}

impl Decode for Close {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let transfer_id = decoder.u32()?;
        let final_data_bytes = decoder.u64()?;
        let status = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("CLOSE reserved field"));
        }
        let detail = decoder.len_bytes_u32()?.to_vec();
        decoder.finish()?;
        if transfer_id == 0 {
            return Err(Error::Invalid("zero transfer ID"));
        }
        Ok(Self {
            transfer_id,
            final_data_bytes,
            status,
            detail,
        })
    }
}

/// RESET uses CLOSE's status/reserved/detail shape without
/// `final_data_bytes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reset {
    pub transfer_id: u32,
    pub status: u16,
    pub detail: Vec<u8>,
}

impl Reset {
    /// Return the stage which must be atomically discarded by this RESET.
    /// Session managers also retire every sibling descriptor carrying the
    /// same `(content_family, staging_handle)` pair. Numeric handles are
    /// family-scoped and may overlap across different content families.
    pub fn disposed_upload_stage(&self, descriptor: &Descriptor) -> Result<Option<UploadStage>> {
        if self.transfer_id != descriptor.transfer_id {
            return Err(Error::Invalid("Transfer RESET descriptor mismatch"));
        }
        descriptor.validate()?;
        descriptor.upload_stage()
    }

    /// Return the family-scoped key which session managers use to retire
    /// siblings without colliding with another family's numeric handle.
    pub fn disposed_upload_stage_key(
        &self,
        descriptor: &Descriptor,
    ) -> Result<Option<UploadStageKey>> {
        if self.transfer_id != descriptor.transfer_id {
            return Err(Error::Invalid("Transfer RESET descriptor mismatch"));
        }
        descriptor.validate()?;
        descriptor.upload_stage_key()
    }

    /// Resolve this RESET against all descriptors owned by one family stage.
    /// A matching descriptor must carry the same upload-stage handle; callers
    /// then atomically discard the stage and retire every sibling descriptor.
    pub fn disposed_upload_stage_from<'a>(
        &self,
        staging_handle: u64,
        descriptors: impl IntoIterator<Item = &'a Descriptor>,
    ) -> Result<Option<UploadStage>> {
        let Some(descriptor) = descriptors
            .into_iter()
            .find(|descriptor| descriptor.transfer_id == self.transfer_id)
        else {
            return Ok(None);
        };
        let stage = self
            .disposed_upload_stage(descriptor)?
            .ok_or(Error::Invalid("Transfer RESET missing upload stage"))?;
        if stage.staging_handle != staging_handle {
            return Err(Error::Invalid("Transfer RESET upload-stage identity"));
        }
        Ok(Some(stage))
    }
}

impl Encode for Reset {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.transfer_id == 0 {
            return Err(Error::Invalid("zero transfer ID"));
        }
        put_u32(out, self.transfer_id);
        put_u16(out, self.status);
        put_u16(out, 0);
        put_len_u32(out, self.detail.len())?;
        out.extend_from_slice(&self.detail);
        Ok(())
    }
}

impl Decode for Reset {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let transfer_id = decoder.u32()?;
        let status = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("RESET reserved field"));
        }
        let detail = decoder.len_bytes_u32()?.to_vec();
        decoder.finish()?;
        if transfer_id == 0 {
            return Err(Error::Invalid("zero transfer ID"));
        }
        Ok(Self {
            transfer_id,
            status,
            detail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn truncations<T: Decode>(bytes: &[u8]) {
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "prefix {end}");
        }
    }

    #[test]
    fn descriptor_golden_and_truncation() {
        let value = Descriptor {
            transfer_id: 2,
            mode: Mode::Byte,
            direction: Direction::BIDIRECTIONAL,
            receiver_send_credit: 3,
            sender_send_credit: 4,
            max_item_bytes: 0,
            max_chunk_bytes: 5,
            content_family: 6,
            content_kind: 7,
            content_version: 1,
            extensions: Extensions::default(),
        };
        let bytes = value.encode().unwrap();
        assert_eq!(bytes.len(), 46);
        assert_eq!(&bytes[..8], &[2, 0, 0, 0, 0, 3, 0, 0]);
        assert_eq!(Descriptor::decode(&bytes).unwrap(), value);
        truncations::<Descriptor>(&bytes);
        assert!(!value.requires_sensitive_frame(kind::BYTE_DATA).unwrap());
        assert!(!value.requires_sensitive_frame(kind::CREDIT).unwrap());

        let mut mislabeled = value;
        mislabeled.extensions = Extensions(vec![crate::Extension {
            tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
            required: false,
            value: Vec::new(),
        }]);
        assert_eq!(
            mislabeled.encode(),
            Err(Error::Invalid("Transfer sensitive-content extension"))
        );
        mislabeled.extensions.0[0].required = true;
        assert!(
            mislabeled
                .requires_sensitive_frame(kind::BYTE_DATA)
                .unwrap()
        );
        assert!(mislabeled.requires_sensitive_frame(kind::CLOSE).unwrap());
        assert!(!mislabeled.requires_sensitive_frame(kind::CREDIT).unwrap());
    }

    #[test]
    fn data_and_lifecycle_round_trip() {
        let byte = ByteData {
            transfer_id: 2,
            offset: 7,
            data: vec![8, 9],
        };
        assert_eq!(ByteData::decode(&byte.encode().unwrap()).unwrap(), byte);
        let message = MessageData {
            transfer_id: 3,
            sequence: 0,
            fragment_offset: 0,
            start: true,
            end: true,
            data: vec![1],
        };
        assert_eq!(
            MessageData::decode(&message.encode().unwrap()).unwrap(),
            message
        );
        let close = Close {
            transfer_id: 2,
            final_data_bytes: 7,
            status: 0,
            detail: vec![],
        };
        assert_eq!(Close::decode(&close.encode().unwrap()).unwrap(), close);
        let reset = Reset {
            transfer_id: 2,
            status: 12,
            detail: vec![1],
        };
        assert_eq!(Reset::decode(&reset.encode().unwrap()).unwrap(), reset);
    }

    #[test]
    fn upload_stage_is_bounded_and_reset_discards_it() {
        let stage = UploadStage {
            staging_handle: 9,
            expires_server_ns: 10 + MAX_UPLOAD_STAGE_LIFETIME_NS,
        };
        stage.validate_at(10).unwrap();
        assert!(!stage.is_expired_at(stage.expires_server_ns - 1).unwrap());
        assert!(stage.is_expired_at(stage.expires_server_ns).unwrap());
        assert_eq!(
            stage.validate_at(9),
            Err(Error::Invalid("Transfer upload-stage expiry"))
        );
        let descriptor = Descriptor {
            transfer_id: 2,
            mode: Mode::Byte,
            direction: Direction::RECEIVER_TO_SENDER,
            receiver_send_credit: 1024,
            sender_send_credit: 0,
            max_item_bytes: 0,
            max_chunk_bytes: 1024,
            content_family: 6,
            content_kind: 7,
            content_version: 1,
            extensions: Extensions(vec![
                crate::Extension {
                    tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                    required: true,
                    value: Vec::new(),
                },
                stage.extension().unwrap(),
            ]),
        };
        let bytes = descriptor.encode().unwrap();
        assert_eq!(Descriptor::decode(&bytes).unwrap(), descriptor);
        truncations::<Descriptor>(&bytes);
        assert_eq!(descriptor.upload_stage().unwrap(), Some(stage));
        assert_eq!(
            descriptor.upload_stage_key().unwrap(),
            Some(UploadStageKey {
                content_family: 6,
                staging_handle: stage.staging_handle,
            })
        );

        let reset = Reset {
            transfer_id: descriptor.transfer_id,
            status: 10,
            detail: b"cancelled".to_vec(),
        };
        assert_eq!(
            reset.disposed_upload_stage(&descriptor).unwrap(),
            Some(stage)
        );
        assert_eq!(
            reset.disposed_upload_stage_key(&descriptor).unwrap(),
            descriptor.upload_stage_key().unwrap()
        );
        let mut same_numeric_handle_in_other_family = descriptor.clone();
        same_numeric_handle_in_other_family.content_family = 7;
        assert_ne!(
            descriptor.upload_stage_key().unwrap(),
            same_numeric_handle_in_other_family
                .upload_stage_key()
                .unwrap()
        );
        assert_eq!(
            reset
                .disposed_upload_stage_from(stage.staging_handle, [&descriptor])
                .unwrap(),
            Some(stage)
        );
        let unrelated = Reset {
            transfer_id: 99,
            status: 10,
            detail: Vec::new(),
        };
        assert_eq!(
            unrelated
                .disposed_upload_stage_from(stage.staging_handle, [&descriptor])
                .unwrap(),
            None
        );

        let mut invalid = descriptor.clone();
        invalid.direction = Direction::BIDIRECTIONAL;
        assert_eq!(
            invalid.encode(),
            Err(Error::Invalid("Transfer upload-stage descriptor"))
        );
        let mut invalid = descriptor;
        invalid.extensions.0[1].required = false;
        assert_eq!(
            invalid.encode(),
            Err(Error::Invalid("Transfer upload-stage extension"))
        );
    }

    #[test]
    fn message_limits_fragmentation_and_empty_items() {
        let descriptor = Descriptor {
            transfer_id: 3,
            mode: Mode::Message,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: 100,
            max_item_bytes: 5,
            max_chunk_bytes: 3,
            content_family: 9,
            content_kind: 1,
            content_version: 1,
            extensions: Extensions(vec![crate::Extension {
                tag: crate::schema::transfer::MAX_OPEN_MESSAGES_EXTENSION as u16,
                required: false,
                value: 2u32.to_le_bytes().to_vec(),
            }]),
        };
        assert_eq!(descriptor.max_open_messages().unwrap(), 2);
        let mut receiver = MessageReceiver::new(&descriptor).unwrap();
        let fragment = |sequence, offset, start, end, data: &[u8]| MessageData {
            transfer_id: 3,
            sequence,
            fragment_offset: offset,
            start,
            end,
            data: data.to_vec(),
        };
        assert!(
            !receiver
                .accept(&fragment(1, 0, true, false, b"ab"))
                .unwrap()
        );
        assert!(!receiver.accept(&fragment(2, 0, true, false, b"c")).unwrap());
        assert_eq!(receiver.open_messages(), 2);
        assert_eq!(
            receiver.accept(&fragment(3, 0, true, false, b"d")),
            Err(Error::Invalid("too many open messages"))
        );
        assert!(
            receiver
                .accept(&fragment(1, 2, false, true, b"ef"))
                .unwrap()
        );
        assert_eq!(receiver.open_messages(), 1);
        assert_eq!(
            fragment(4, 0, true, true, b"").encode(),
            Err(Error::Invalid("MESSAGE_DATA"))
        );
        assert_eq!(
            receiver.accept(&fragment(2, 9, false, true, b"z")),
            Err(Error::Invalid("MESSAGE fragment offset"))
        );
    }
}
