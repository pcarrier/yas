//! YAS Selection family version 1 payload codecs.

use crate::prelude::*;

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, limit_u32, limit_u64,
    put_bytes_u32, put_i64, put_len_u16, put_len_u32, put_string_u16, put_string_u32, put_u16,
    put_u64, read_limit_u32, read_limit_u64, reject_unknown_required_extensions,
};
use crate::state::{Record, RecordKind};
use crate::transfer::{
    Delivery, Descriptor, Direction, InlineOrTransfer, Mode, Reset, UploadStage,
};

pub const VERSION: u16 = crate::schema::selection::VERSION;
pub const MAX_INLINE_BYTES: usize = crate::schema::selection::MAX_INLINE_BYTES as usize;
pub const MAX_ITEMS: usize = crate::schema::selection::MAX_ITEMS as usize;
pub const MAX_MIME_BYTES: usize = crate::schema::selection::MAX_MIME_BYTES as usize;
pub const MAX_ITEM_NAME_BYTES: usize = crate::schema::selection::MAX_ITEM_NAME_BYTES as usize;

pub mod request_kind {
    pub use crate::schema::selection::request::*;
}

pub mod event_kind {
    pub use crate::schema::selection::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_inline_bytes: u32,
    pub max_items: u32,
    pub max_mime_bytes: u32,
    pub max_item_name_bytes: u32,
    pub max_active_drags_per_session: u32,
    pub max_stages_per_session: u32,
    pub max_staged_bytes_per_stage: u64,
    pub max_mutation_replays: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_inline_bytes: crate::schema::selection::MAX_INLINE_BYTES as u32,
        max_items: crate::schema::selection::MAX_ITEMS as u32,
        max_mime_bytes: crate::schema::selection::MAX_MIME_BYTES as u32,
        max_item_name_bytes: crate::schema::selection::MAX_ITEM_NAME_BYTES as u32,
        max_active_drags_per_session: crate::schema::selection::MAX_ACTIVE_DRAGS_PER_SESSION as u32,
        max_stages_per_session: crate::schema::selection::MAX_STAGES_PER_SESSION as u32,
        max_staged_bytes_per_stage: crate::schema::selection::MAX_STAGED_BYTES_PER_STAGE,
        max_mutation_replays: crate::schema::selection::MAX_MUTATION_REPLAYS as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid = |value: u32, maximum: u32| value != 0 && value <= maximum;
        if !valid(self.max_inline_bytes, hard.max_inline_bytes)
            || !valid(self.max_items, hard.max_items)
            || !valid(self.max_mime_bytes, hard.max_mime_bytes)
            || !valid(self.max_item_name_bytes, hard.max_item_name_bytes)
            || !valid(
                self.max_active_drags_per_session,
                hard.max_active_drags_per_session,
            )
            || !valid(self.max_stages_per_session, hard.max_stages_per_session)
            || self.max_staged_bytes_per_stage == 0
            || self.max_staged_bytes_per_stage > hard.max_staged_bytes_per_stage
            || !valid(self.max_mutation_replays, hard.max_mutation_replays)
        {
            return Err(Error::Invalid("Selection family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::selection::LIMIT_MAX_INLINE_BYTES,
                self.max_inline_bytes,
            ),
            limit_u32(crate::schema::selection::LIMIT_MAX_ITEMS, self.max_items),
            limit_u32(
                crate::schema::selection::LIMIT_MAX_MIME_BYTES,
                self.max_mime_bytes,
            ),
            limit_u32(
                crate::schema::selection::LIMIT_MAX_ITEM_NAME_BYTES,
                self.max_item_name_bytes,
            ),
            limit_u32(
                crate::schema::selection::LIMIT_MAX_ACTIVE_DRAGS_PER_SESSION,
                self.max_active_drags_per_session,
            ),
            limit_u32(
                crate::schema::selection::LIMIT_MAX_STAGES_PER_SESSION,
                self.max_stages_per_session,
            ),
            limit_u64(
                crate::schema::selection::LIMIT_MAX_STAGED_BYTES_PER_STAGE,
                self.max_staged_bytes_per_stage,
            ),
            limit_u32(
                crate::schema::selection::LIMIT_MAX_MUTATION_REPLAYS,
                self.max_mutation_replays,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::selection::LIMIT_MAX_INLINE_BYTES as u16,
                crate::schema::selection::LIMIT_MAX_ITEMS as u16,
                crate::schema::selection::LIMIT_MAX_MIME_BYTES as u16,
                crate::schema::selection::LIMIT_MAX_ITEM_NAME_BYTES as u16,
                crate::schema::selection::LIMIT_MAX_ACTIVE_DRAGS_PER_SESSION as u16,
                crate::schema::selection::LIMIT_MAX_STAGES_PER_SESSION as u16,
                crate::schema::selection::LIMIT_MAX_STAGED_BYTES_PER_STAGE as u16,
                crate::schema::selection::LIMIT_MAX_MUTATION_REPLAYS as u16,
            ],
            "unknown required Selection family limit",
        )?;
        let value = Self {
            max_inline_bytes: read_limit_u32(
                extensions,
                crate::schema::selection::LIMIT_MAX_INLINE_BYTES,
            )?,
            max_items: read_limit_u32(extensions, crate::schema::selection::LIMIT_MAX_ITEMS)?,
            max_mime_bytes: read_limit_u32(
                extensions,
                crate::schema::selection::LIMIT_MAX_MIME_BYTES,
            )?,
            max_item_name_bytes: read_limit_u32(
                extensions,
                crate::schema::selection::LIMIT_MAX_ITEM_NAME_BYTES,
            )?,
            max_active_drags_per_session: read_limit_u32(
                extensions,
                crate::schema::selection::LIMIT_MAX_ACTIVE_DRAGS_PER_SESSION,
            )?,
            max_stages_per_session: read_limit_u32(
                extensions,
                crate::schema::selection::LIMIT_MAX_STAGES_PER_SESSION,
            )?,
            max_staged_bytes_per_stage: read_limit_u64(
                extensions,
                crate::schema::selection::LIMIT_MAX_STAGED_BYTES_PER_STAGE,
            )?,
            max_mutation_replays: read_limit_u32(
                extensions,
                crate::schema::selection::LIMIT_MAX_MUTATION_REPLAYS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn valid_slot(slot: u8) -> bool {
    slot == crate::schema::selection::SLOT_CLIPBOARD as u8
        || slot == crate::schema::selection::SLOT_PRIMARY as u8
}

fn validate_slot(slot: u8) -> Result<()> {
    if valid_slot(slot) {
        Ok(())
    } else {
        Err(Error::Invalid("Selection slot"))
    }
}

fn validate_handle(value: u64, name: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(name))
    } else {
        Ok(())
    }
}

fn validate_revision(value: u64) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid("zero Selection revision"))
    } else {
        Ok(())
    }
}

fn validate_operation_id(value: &[u8; 16]) -> Result<()> {
    if value.iter().all(|byte| *byte == 0) {
        Err(Error::Invalid("zero Selection operation ID"))
    } else {
        Ok(())
    }
}

fn validate_mime(mime: &str) -> Result<()> {
    if mime.is_empty() || mime.len() > MAX_MIME_BYTES || mime.as_bytes().contains(&0) {
        Err(Error::Invalid("Selection MIME"))
    } else {
        Ok(())
    }
}

fn validate_drag_name(name: &str) -> Result<()> {
    if name.len() > MAX_ITEM_NAME_BYTES || name.as_bytes().contains(&0) {
        Err(Error::Invalid("Selection drag item name"))
    } else {
        Ok(())
    }
}

fn validate_actions(actions: u16, allow_none: bool, single: bool) -> Result<()> {
    let mask = crate::schema::selection::ACTION_MASK as u16;
    if actions & !mask != 0 || (!allow_none && actions == 0) || (single && actions.count_ones() > 1)
    {
        Err(Error::Invalid("Selection drag action"))
    } else {
        Ok(())
    }
}

fn validate_mimes(mimes: &[String], allow_empty: bool) -> Result<()> {
    if mimes.len() > MAX_ITEMS || (!allow_empty && mimes.is_empty()) {
        return Err(Error::Invalid("Selection MIME count"));
    }
    let mut previous: Option<&str> = None;
    for mime in mimes {
        validate_mime(mime)?;
        if previous.is_some_and(|old| old >= mime.as_str()) {
            return Err(Error::Invalid("Selection MIME order"));
        }
        previous = Some(mime);
    }
    Ok(())
}

fn encode_mimes(mimes: &[String], out: &mut Vec<u8>) -> Result<()> {
    validate_mimes(mimes, true)?;
    put_len_u16(out, mimes.len())?;
    for mime in mimes {
        put_string_u16(out, mime)?;
    }
    Ok(())
}

fn decode_mimes(decoder: &mut Decoder<'_>, allow_empty: bool) -> Result<Vec<String>> {
    let count = usize::from(decoder.u16()?);
    if count > MAX_ITEMS || count > decoder.remaining() / 2 {
        return Err(Error::Invalid("Selection MIME count"));
    }
    let mut mimes = Vec::with_capacity(count);
    for _ in 0..count {
        mimes.push(decoder.string_u16()?);
    }
    validate_mimes(&mimes, allow_empty)?;
    Ok(mimes)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineItem {
    pub mime: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Set {
    pub slot: u8,
    pub operation_id: [u8; 16],
    pub items: Vec<InlineItem>,
    pub extensions: Extensions,
}

impl Set {
    fn validate(&self) -> Result<()> {
        validate_slot(self.slot)?;
        validate_operation_id(&self.operation_id)?;
        if self.items.is_empty() || self.items.len() > MAX_ITEMS {
            return Err(Error::Invalid("Selection SET item count"));
        }
        let mut previous: Option<&str> = None;
        let mut encoded_bytes = 0usize;
        for item in &self.items {
            validate_mime(&item.mime)?;
            if previous.is_some_and(|old| old >= item.mime.as_str()) {
                return Err(Error::Invalid("Selection MIME order"));
            }
            previous = Some(&item.mime);
            encoded_bytes = encoded_bytes
                .checked_add(2 + item.mime.len() + 4 + item.data.len())
                .ok_or(Error::LengthOverflow)?;
            if encoded_bytes > MAX_INLINE_BYTES {
                return Err(Error::LimitExceeded {
                    limit: "Selection inline item bytes",
                    actual: encoded_bytes as u64,
                    maximum: MAX_INLINE_BYTES as u64,
                });
            }
        }
        self.extensions.validate()
    }
}

impl Encode for Set {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.slot);
        out.extend_from_slice(&[0; 3]);
        out.extend_from_slice(&self.operation_id);
        put_len_u16(out, self.items.len())?;
        for item in &self.items {
            put_string_u16(out, &item.mime)?;
            put_bytes_u32(out, &item.data)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Set {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let slot = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Selection SET reserved bytes"));
        }
        let operation_id = decoder.array_16()?;
        let count = usize::from(decoder.u16()?);
        if count == 0 || count > MAX_ITEMS || count > decoder.remaining() / 6 {
            return Err(Error::Invalid("Selection SET item count"));
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(InlineItem {
                mime: decoder.string_u16()?,
                data: decoder.len_bytes_u32()?.to_vec(),
            });
        }
        let value = Self {
            slot,
            operation_id,
            items,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevisionResult {
    pub revision: u64,
}

impl Encode for RevisionResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_revision(self.revision)?;
        put_u64(out, self.revision);
        Ok(())
    }
}

impl Decode for RevisionResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            revision: decoder.u64()?,
        };
        decoder.finish()?;
        validate_revision(value.revision)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadItem {
    pub mime: String,
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub initial_receive_credit: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetBegin {
    pub slot: u8,
    pub operation_id: [u8; 16],
    pub items: Vec<UploadItem>,
    pub extensions: Extensions,
}

impl SetBegin {
    fn validate(&self) -> Result<()> {
        validate_slot(self.slot)?;
        validate_operation_id(&self.operation_id)?;
        if self.items.is_empty() || self.items.len() > MAX_ITEMS {
            return Err(Error::Invalid("Selection SET_BEGIN item count"));
        }
        let mut previous: Option<&str> = None;
        let mut total_bytes = 0u64;
        for item in &self.items {
            validate_mime(&item.mime)?;
            if previous.is_some_and(|old| old >= item.mime.as_str()) {
                return Err(Error::Invalid("Selection MIME order"));
            }
            previous = Some(&item.mime);
            total_bytes = total_bytes
                .checked_add(item.byte_len)
                .ok_or(Error::LengthOverflow)?;
            if total_bytes > crate::schema::selection::MAX_STAGED_BYTES_PER_STAGE {
                return Err(Error::LimitExceeded {
                    limit: "Selection staged bytes",
                    actual: total_bytes,
                    maximum: crate::schema::selection::MAX_STAGED_BYTES_PER_STAGE,
                });
            }
        }
        self.extensions.validate()
    }
}

impl Encode for SetBegin {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.slot);
        out.extend_from_slice(&[0; 3]);
        out.extend_from_slice(&self.operation_id);
        put_len_u16(out, self.items.len())?;
        for item in &self.items {
            put_string_u16(out, &item.mime)?;
            put_u64(out, item.byte_len);
            out.extend_from_slice(&item.content_hash);
            put_u64(out, item.initial_receive_credit);
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for SetBegin {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let slot = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Selection SET_BEGIN reserved bytes"));
        }
        let operation_id = decoder.array_16()?;
        let count = usize::from(decoder.u16()?);
        if count == 0 || count > MAX_ITEMS || count > decoder.remaining() / 50 {
            return Err(Error::Invalid("Selection SET_BEGIN item count"));
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(UploadItem {
                mime: decoder.string_u16()?,
                byte_len: decoder.u64()?,
                content_hash: decoder.array_32()?,
                initial_receive_credit: decoder.u64()?,
            });
        }
        let value = Self {
            slot,
            operation_id,
            items,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

fn validate_item_transfer(descriptor: &Descriptor, direction: Direction) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Byte
        || descriptor.direction != direction
        || descriptor.content_family != crate::family::SELECTION
        || descriptor.content_kind != crate::schema::selection::ITEM_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("Selection item Transfer descriptor"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetBeginResult {
    pub staging_handle: u64,
    pub descriptors: Vec<Descriptor>,
    pub extensions: Extensions,
}

impl SetBeginResult {
    fn validate(&self) -> Result<()> {
        validate_handle(self.staging_handle, "zero Selection staging handle")?;
        if self.descriptors.is_empty() || self.descriptors.len() > MAX_ITEMS {
            return Err(Error::Invalid("Selection upload descriptor count"));
        }
        let mut ids = BTreeSet::new();
        let mut expires_server_ns = None;
        for descriptor in &self.descriptors {
            validate_item_transfer(descriptor, Direction::RECEIVER_TO_SENDER)?;
            let stage = descriptor.require_upload_stage(self.staging_handle)?;
            if expires_server_ns
                .replace(stage.expires_server_ns)
                .is_some_and(|value| value != stage.expires_server_ns)
            {
                return Err(Error::Invalid("Selection upload-stage expiry mismatch"));
            }
            if !ids.insert(descriptor.transfer_id) {
                return Err(Error::Invalid("duplicate Selection Transfer ID"));
            }
        }
        self.extensions.validate()
    }

    pub fn upload_stage(&self) -> Result<crate::transfer::UploadStage> {
        self.validate()?;
        self.descriptors[0].require_upload_stage(self.staging_handle)
    }

    /// Return this entire Selection stage when `reset` targets any one of its
    /// item Transfers. Every sibling Transfer is retired with the stage.
    pub fn stage_discarded_by(&self, reset: &Reset) -> Result<Option<UploadStage>> {
        self.validate()?;
        reset.disposed_upload_stage_from(self.staging_handle, &self.descriptors)
    }
}

impl Encode for SetBeginResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.staging_handle);
        put_len_u16(out, self.descriptors.len())?;
        put_u16(out, 0);
        for descriptor in &self.descriptors {
            let bytes = descriptor.encode()?;
            put_len_u32(out, bytes.len())?;
            out.extend_from_slice(&bytes);
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for SetBeginResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let staging_handle = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0 || count == 0 || count > MAX_ITEMS || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("Selection upload descriptor count"));
        }
        let mut descriptors = Vec::with_capacity(count);
        for _ in 0..count {
            descriptors.push(Descriptor::decode(decoder.len_bytes_u32()?)?);
        }
        let value = Self {
            staging_handle,
            descriptors,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetCommit {
    pub staging_handle: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for SetCommit {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.staging_handle, "zero Selection staging handle")?;
        validate_operation_id(&self.operation_id)?;
        put_u64(out, self.staging_handle);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for SetCommit {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            staging_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        validate_handle(value.staging_handle, "zero Selection staging handle")?;
        validate_operation_id(&value.operation_id)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GetTarget {
    Slot {
        slot: u8,
        revision: u64,
    },
    Drag {
        drag_handle: u64,
        revision: u64,
        item_index: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Get {
    pub target: GetTarget,
    pub initial_receive_credit: u64,
    pub mime: String,
    pub extensions: Extensions,
}

impl Get {
    fn validate(&self) -> Result<()> {
        match self.target {
            GetTarget::Slot { slot, revision } => {
                validate_slot(slot)?;
                validate_revision(revision)?;
            }
            GetTarget::Drag {
                drag_handle,
                revision,
                ..
            } => {
                validate_handle(drag_handle, "zero Selection drag handle")?;
                validate_revision(revision)?;
            }
        }
        validate_mime(&self.mime)?;
        self.extensions.validate()
    }
}

impl Encode for Get {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        match self.target {
            GetTarget::Slot { slot, revision } => {
                out.push(crate::schema::selection::GET_TARGET_SLOT as u8);
                out.extend_from_slice(&[0; 3]);
                out.push(slot);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, revision);
            }
            GetTarget::Drag {
                drag_handle,
                revision,
                item_index,
            } => {
                out.push(crate::schema::selection::GET_TARGET_DRAG as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, drag_handle);
                put_u64(out, revision);
                put_u16(out, item_index);
                put_u16(out, 0);
            }
        }
        put_u64(out, self.initial_receive_credit);
        put_string_u16(out, &self.mime)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Get {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Selection GET reserved bytes"));
        }
        let target = match kind {
            value if value == crate::schema::selection::GET_TARGET_SLOT as u8 => {
                let slot = decoder.u8()?;
                if decoder.take(3)? != [0; 3] {
                    return Err(Error::Invalid("Selection slot target reserved bytes"));
                }
                GetTarget::Slot {
                    slot,
                    revision: decoder.u64()?,
                }
            }
            value if value == crate::schema::selection::GET_TARGET_DRAG as u8 => {
                let drag_handle = decoder.u64()?;
                let revision = decoder.u64()?;
                let item_index = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Selection drag target reserved field"));
                }
                GetTarget::Drag {
                    drag_handle,
                    revision,
                    item_index,
                }
            }
            _ => return Err(Error::Invalid("Selection GET target")),
        };
        let value = Self {
            target,
            initial_receive_credit: decoder.u64()?,
            mime: decoder.string_u16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetResult(pub InlineOrTransfer);

impl GetResult {
    fn validate(&self) -> Result<()> {
        match &self.0.delivery {
            Delivery::Inline(bytes) if bytes.len() <= MAX_INLINE_BYTES => Ok(()),
            Delivery::Inline(bytes) => Err(Error::LimitExceeded {
                limit: "Selection inline result bytes",
                actual: bytes.len() as u64,
                maximum: MAX_INLINE_BYTES as u64,
            }),
            Delivery::Transfer(descriptor) => {
                validate_item_transfer(descriptor, Direction::SENDER_TO_RECEIVER)
            }
        }
    }
}

impl Encode for GetResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        self.0.encode_to(out)
    }
}

impl Decode for GetResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let value = Self(InlineOrTransfer::decode(input)?);
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clear {
    pub slot: u8,
    pub observed_revision: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for Clear {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_slot(self.slot)?;
        validate_revision(self.observed_revision)?;
        validate_operation_id(&self.operation_id)?;
        out.push(self.slot);
        out.extend_from_slice(&[0; 3]);
        put_u64(out, self.observed_revision);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Clear {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let slot = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Selection CLEAR reserved bytes"));
        }
        let value = Self {
            slot,
            observed_revision: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        validate_slot(value.slot)?;
        validate_revision(value.observed_revision)?;
        validate_operation_id(&value.operation_id)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragItem {
    pub name: String,
    pub mime_types: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragBegin {
    pub operation_id: [u8; 16],
    pub source_actions: u16,
    pub items: Vec<DragItem>,
    pub extensions: Extensions,
}

impl DragBegin {
    fn validate(&self) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        validate_actions(self.source_actions, false, false)?;
        if self.items.is_empty() || self.items.len() > MAX_ITEMS {
            return Err(Error::Invalid("Selection drag item count"));
        }
        for item in &self.items {
            validate_drag_name(&item.name)?;
            validate_mimes(&item.mime_types, false)?;
        }
        self.extensions.validate()
    }
}

impl Encode for DragBegin {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.source_actions);
        put_len_u16(out, self.items.len())?;
        for item in &self.items {
            put_string_u16(out, &item.name)?;
            encode_mimes(&item.mime_types, out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for DragBegin {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let operation_id = decoder.array_16()?;
        let source_actions = decoder.u16()?;
        let count = usize::from(decoder.u16()?);
        if count == 0 || count > MAX_ITEMS || count > decoder.remaining() / 4 {
            return Err(Error::Invalid("Selection drag item count"));
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(DragItem {
                name: decoder.string_u16()?,
                mime_types: decode_mimes(&mut decoder, false)?,
            });
        }
        let value = Self {
            operation_id,
            source_actions,
            items,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragBeginResult {
    pub drag_handle: u64,
    pub revision: u64,
}

impl Encode for DragBeginResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.drag_handle, "zero Selection drag handle")?;
        validate_revision(self.revision)?;
        put_u64(out, self.drag_handle);
        put_u64(out, self.revision);
        Ok(())
    }
}

impl Decode for DragBeginResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            drag_handle: decoder.u64()?,
            revision: decoder.u64()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragDrop {
    pub drag_handle: u64,
    pub revision: u64,
    pub operation_id: [u8; 16],
    pub selected_action: u16,
    pub extensions: Extensions,
}

/// Final per-item metadata selected by the drag source at drop time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragDropItem {
    pub name: String,
    pub selected_mime: String,
}

fn decode_drag_drop_items(
    extensions: &Extensions,
    expected_items: Option<&[DragItem]>,
) -> Result<Vec<DragDropItem>> {
    extensions.validate()?;
    let mut result = None;
    for extension in &extensions.0 {
        if extension.tag != crate::schema::selection::DRAG_DROP_ITEMS_EXTENSION as u16 {
            if extension.required {
                return Err(Error::Invalid(
                    "unknown required Selection DRAG_DROP extension",
                ));
            }
            continue;
        }
        if !extension.required {
            return Err(Error::Invalid("optional Selection DRAG_DROP items"));
        }
        let mut decoder = Decoder::new(&extension.value);
        let count = usize::from(decoder.u16()?);
        if count == 0
            || count > MAX_ITEMS
            || count > decoder.remaining() / 4
            || expected_items.is_some_and(|expected| expected.len() != count)
        {
            return Err(Error::Invalid("Selection DRAG_DROP item count"));
        }
        let mut items = Vec::with_capacity(count);
        for index in 0..count {
            let name = decoder.string_u16()?;
            validate_drag_name(&name)?;
            let selected_mime = decoder.string_u16()?;
            validate_mime(&selected_mime)?;
            if let Some(expected) = expected_items {
                let offered = &expected[index];
                if (!offered.name.is_empty() && offered.name != name)
                    || offered
                        .mime_types
                        .binary_search_by(|mime| mime.as_str().cmp(&selected_mime))
                        .is_err()
                {
                    return Err(Error::Invalid("Selection DRAG_DROP item metadata"));
                }
            }
            items.push(DragDropItem {
                name,
                selected_mime,
            });
        }
        decoder.finish()?;
        result = Some(items);
    }
    result.ok_or(Error::Invalid("missing required Selection DRAG_DROP items"))
}

pub fn drag_drop_items_extension(items: &[DragDropItem]) -> Result<Extension> {
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err(Error::Invalid("Selection DRAG_DROP item count"));
    }
    let mut value = Vec::new();
    put_len_u16(&mut value, items.len())?;
    for item in items {
        validate_drag_name(&item.name)?;
        validate_mime(&item.selected_mime)?;
        put_string_u16(&mut value, &item.name)?;
        put_string_u16(&mut value, &item.selected_mime)?;
    }
    Ok(Extension {
        tag: crate::schema::selection::DRAG_DROP_ITEMS_EXTENSION as u16,
        required: true,
        value,
    })
}

impl DragDrop {
    /// Decode the required final names and exact MIME selected for each
    /// current item, rejecting reordered or unoffered metadata.
    pub fn selected_items(&self, current_items: &[DragItem]) -> Result<Vec<DragDropItem>> {
        decode_drag_drop_items(&self.extensions, Some(current_items))
    }

    fn validate(&self) -> Result<()> {
        validate_handle(self.drag_handle, "zero Selection drag handle")?;
        validate_revision(self.revision)?;
        validate_operation_id(&self.operation_id)?;
        validate_actions(self.selected_action, false, true)?;
        decode_drag_drop_items(&self.extensions, None)?;
        Ok(())
    }
}

impl Encode for DragDrop {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.drag_handle);
        put_u64(out, self.revision);
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.selected_action);
        put_u16(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for DragDrop {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let drag_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let selected_action = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Selection DRAG_DROP reserved field"));
        }
        let value = Self {
            drag_handle,
            revision,
            operation_id,
            selected_action,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragCancel {
    pub drag_handle: u64,
    pub revision: u64,
    pub operation_id: [u8; 16],
    pub reason: String,
}

impl Encode for DragCancel {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.drag_handle, "zero Selection drag handle")?;
        validate_revision(self.revision)?;
        validate_operation_id(&self.operation_id)?;
        put_u64(out, self.drag_handle);
        put_u64(out, self.revision);
        out.extend_from_slice(&self.operation_id);
        put_string_u32(out, &self.reason)
    }
}

impl Decode for DragCancel {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            drag_handle: decoder.u64()?,
            revision: decoder.u64()?,
            operation_id: decoder.array_16()?,
            reason: decoder.string_u32()?,
        };
        decoder.finish()?;
        validate_handle(value.drag_handle, "zero Selection drag handle")?;
        validate_revision(value.revision)?;
        validate_operation_id(&value.operation_id)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragPosition {
    pub drag_handle: u64,
    pub revision: u64,
    pub target_surface: u64,
    pub x_32_32: i64,
    pub y_32_32: i64,
    pub actions: u16,
}

impl Encode for DragPosition {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.drag_handle, "zero Selection drag handle")?;
        validate_revision(self.revision)?;
        validate_handle(self.target_surface, "zero Selection target surface")?;
        validate_actions(self.actions, true, false)?;
        put_u64(out, self.drag_handle);
        put_u64(out, self.revision);
        put_u64(out, self.target_surface);
        put_i64(out, self.x_32_32);
        put_i64(out, self.y_32_32);
        put_u16(out, self.actions);
        put_u16(out, 0);
        Ok(())
    }
}

impl Decode for DragPosition {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let drag_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let target_surface = decoder.u64()?;
        let x_32_32 = decoder.i64()?;
        let y_32_32 = decoder.i64()?;
        let actions = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Selection drag position reserved field"));
        }
        let value = Self {
            drag_handle,
            revision,
            target_surface,
            x_32_32,
            y_32_32,
            actions,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

pub type DragEnter = DragPosition;
pub type DragMotion = DragPosition;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragLeave {
    pub drag_handle: u64,
    pub revision: u64,
    pub target_surface: u64,
}

impl Encode for DragLeave {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.drag_handle, "zero Selection drag handle")?;
        validate_revision(self.revision)?;
        validate_handle(self.target_surface, "zero Selection target surface")?;
        put_u64(out, self.drag_handle);
        put_u64(out, self.revision);
        put_u64(out, self.target_surface);
        Ok(())
    }
}

impl Decode for DragLeave {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            drag_handle: decoder.u64()?,
            revision: decoder.u64()?,
            target_surface: decoder.u64()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionRecord {
    pub slot: u8,
    pub owner_kind: u8,
    pub owner_handle: u64,
    pub revision: u64,
    pub mime_types: Vec<String>,
    pub extensions: Extensions,
}

impl SelectionRecord {
    fn validate(&self) -> Result<()> {
        validate_slot(self.slot)?;
        let none = crate::schema::selection::OWNER_NONE as u8;
        let max = crate::schema::selection::OWNER_EXTERNAL as u8;
        if self.owner_kind > max || (self.owner_kind == none) != (self.owner_handle == 0) {
            return Err(Error::Invalid("Selection owner"));
        }
        validate_revision(self.revision)?;
        validate_mimes(&self.mime_types, true)?;
        if self.owner_kind == none && !self.mime_types.is_empty() {
            return Err(Error::Invalid("unowned Selection MIME list"));
        }
        self.extensions.validate()
    }
}

impl Encode for SelectionRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.slot);
        out.push(self.owner_kind);
        put_u16(out, 0);
        put_u64(out, self.owner_handle);
        put_u64(out, self.revision);
        encode_mimes(&self.mime_types, out)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for SelectionRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let slot = decoder.u8()?;
        let owner_kind = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Selection record reserved field"));
        }
        let value = Self {
            slot,
            owner_kind,
            owner_handle: decoder.u64()?,
            revision: decoder.u64()?,
            mime_types: decode_mimes(&mut decoder, true)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragRecord {
    pub drag_handle: u64,
    pub revision: u64,
    pub owner_session: [u8; 16],
    pub source_actions: u16,
    pub selected_action: u16,
    pub target_surface: u64,
    pub items: Vec<DragItem>,
    pub extensions: Extensions,
}

impl DragRecord {
    fn validate(&self) -> Result<()> {
        validate_handle(self.drag_handle, "zero Selection drag handle")?;
        validate_revision(self.revision)?;
        validate_actions(self.source_actions, false, false)?;
        validate_actions(self.selected_action, true, true)?;
        if self.selected_action & !self.source_actions != 0 {
            return Err(Error::Invalid("selected Selection drag action"));
        }
        if self.items.is_empty() || self.items.len() > MAX_ITEMS {
            return Err(Error::Invalid("Selection drag item count"));
        }
        for item in &self.items {
            validate_drag_name(&item.name)?;
            validate_mimes(&item.mime_types, false)?;
        }
        self.extensions.validate()
    }
}

impl Encode for DragRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.drag_handle);
        put_u64(out, self.revision);
        out.extend_from_slice(&self.owner_session);
        put_u16(out, self.source_actions);
        put_u16(out, self.selected_action);
        put_u64(out, self.target_surface);
        put_len_u16(out, self.items.len())?;
        for item in &self.items {
            put_string_u16(out, &item.name)?;
            encode_mimes(&item.mime_types, out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for DragRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let drag_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let owner_session = decoder.array_16()?;
        let source_actions = decoder.u16()?;
        let selected_action = decoder.u16()?;
        let target_surface = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if count == 0 || count > MAX_ITEMS || count > decoder.remaining() / 4 {
            return Err(Error::Invalid("Selection drag item count"));
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(DragItem {
                name: decoder.string_u16()?,
                mime_types: decode_mimes(&mut decoder, false)?,
            });
        }
        let value = Self {
            drag_handle,
            revision,
            owner_session,
            source_actions,
            selected_action,
            target_surface,
            items,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompleteEntity {
    Slot(SelectionRecord),
    Drag(DragRecord),
}

impl CompleteEntity {
    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Selection complete state record kind"));
        }
        let mut body = Vec::new();
        match self {
            Self::Slot(record) => {
                put_u16(&mut body, crate::schema::selection::ENTITY_SLOT as u16);
                put_u16(&mut body, 0);
                record.encode_to(&mut body)?;
            }
            Self::Drag(record) => {
                put_u16(&mut body, crate::schema::selection::ENTITY_DRAG as u16);
                put_u16(&mut body, 0);
                record.encode_to(&mut body)?;
            }
        }
        Ok(Record {
            kind,
            required: false,
            body,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatePatch {
    Slot {
        slot: u8,
        revision: u64,
        extensions: Extensions,
    },
    Drag {
        drag_handle: u64,
        revision: u64,
        extensions: Extensions,
    },
}

impl StatePatch {
    pub fn state_record(&self) -> Result<Record> {
        let mut body = Vec::new();
        match self {
            Self::Slot {
                slot,
                revision,
                extensions,
            } => {
                validate_slot(*slot)?;
                validate_revision(*revision)?;
                put_u16(&mut body, crate::schema::selection::ENTITY_SLOT as u16);
                put_u16(&mut body, 0);
                body.push(*slot);
                body.extend_from_slice(&[0; 3]);
                put_u64(&mut body, *revision);
                extensions.encode_tail(&mut body)?;
            }
            Self::Drag {
                drag_handle,
                revision,
                extensions,
            } => {
                validate_handle(*drag_handle, "zero Selection drag handle")?;
                validate_revision(*revision)?;
                put_u16(&mut body, crate::schema::selection::ENTITY_DRAG as u16);
                put_u16(&mut body, 0);
                put_u64(&mut body, *drag_handle);
                put_u64(&mut body, *revision);
                extensions.encode_tail(&mut body)?;
            }
        }
        Ok(Record {
            kind: RecordKind::Patch,
            required: false,
            body,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovedEntity {
    Slot { slot: u8, revision: u64 },
    Drag { drag_handle: u64, revision: u64 },
}

impl RemovedEntity {
    pub fn state_record(self) -> Result<Record> {
        let mut body = Vec::new();
        match self {
            Self::Slot { slot, revision } => {
                validate_slot(slot)?;
                validate_revision(revision)?;
                put_u16(&mut body, crate::schema::selection::ENTITY_SLOT as u16);
                put_u16(&mut body, 0);
                body.push(slot);
                body.extend_from_slice(&[0; 3]);
                put_u64(&mut body, revision);
            }
            Self::Drag {
                drag_handle,
                revision,
            } => {
                validate_handle(drag_handle, "zero Selection drag handle")?;
                validate_revision(revision)?;
                put_u16(&mut body, crate::schema::selection::ENTITY_DRAG as u16);
                put_u16(&mut body, 0);
                put_u64(&mut body, drag_handle);
                put_u64(&mut body, revision);
            }
        }
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateMutation {
    Complete(CompleteEntity),
    Patch(StatePatch),
    Remove(RemovedEntity),
}

pub fn decode_state_record(record: &Record) -> Result<StateMutation> {
    let mut decoder = Decoder::new(&record.body);
    let entity = decoder.u16()?;
    if decoder.u16()? != 0 {
        return Err(Error::Invalid("Selection state entity reserved field"));
    }
    let payload = decoder.rest();
    decoder.finish()?;
    match record.kind {
        RecordKind::Add | RecordKind::Replace => match entity {
            value if value == crate::schema::selection::ENTITY_SLOT as u16 => Ok(
                StateMutation::Complete(CompleteEntity::Slot(SelectionRecord::decode(payload)?)),
            ),
            value if value == crate::schema::selection::ENTITY_DRAG as u16 => Ok(
                StateMutation::Complete(CompleteEntity::Drag(DragRecord::decode(payload)?)),
            ),
            _ => Err(Error::Invalid("Selection state entity")),
        },
        RecordKind::Patch => {
            let mut value = Decoder::new(payload);
            let patch = match entity {
                kind if kind == crate::schema::selection::ENTITY_SLOT as u16 => {
                    let slot = value.u8()?;
                    if value.take(3)? != [0; 3] {
                        return Err(Error::Invalid("Selection slot patch reserved bytes"));
                    }
                    StatePatch::Slot {
                        slot,
                        revision: value.u64()?,
                        extensions: value.extensions()?,
                    }
                }
                kind if kind == crate::schema::selection::ENTITY_DRAG as u16 => StatePatch::Drag {
                    drag_handle: value.u64()?,
                    revision: value.u64()?,
                    extensions: value.extensions()?,
                },
                _ => return Err(Error::Invalid("Selection state entity")),
            };
            value.finish()?;
            patch.state_record()?;
            Ok(StateMutation::Patch(patch))
        }
        RecordKind::Remove => {
            let mut value = Decoder::new(payload);
            let removed = match entity {
                kind if kind == crate::schema::selection::ENTITY_SLOT as u16 => {
                    let slot = value.u8()?;
                    if value.take(3)? != [0; 3] {
                        return Err(Error::Invalid("Selection slot remove reserved bytes"));
                    }
                    RemovedEntity::Slot {
                        slot,
                        revision: value.u64()?,
                    }
                }
                kind if kind == crate::schema::selection::ENTITY_DRAG as u16 => {
                    RemovedEntity::Drag {
                        drag_handle: value.u64()?,
                        revision: value.u64()?,
                    }
                }
                _ => return Err(Error::Invalid("Selection state entity")),
            };
            value.finish()?;
            removed.state_record()?;
            Ok(StateMutation::Remove(removed))
        }
        _ => Err(Error::Invalid("Selection state mutation kind")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged_item_descriptor(transfer_id: u32, staging_handle: u64) -> Descriptor {
        let stage = UploadStage {
            staging_handle,
            expires_server_ns: 1,
        };
        Descriptor {
            transfer_id,
            mode: Mode::Byte,
            direction: Direction::RECEIVER_TO_SENDER,
            receiver_send_credit: 4096,
            sender_send_credit: 0,
            max_item_bytes: 0,
            max_chunk_bytes: 1024,
            content_family: crate::family::SELECTION,
            content_kind: crate::schema::selection::ITEM_CONTENT_KIND as u16,
            content_version: VERSION,
            extensions: Extensions(vec![
                Extension {
                    tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                    required: true,
                    value: Vec::new(),
                },
                stage.extension().unwrap(),
            ]),
        }
    }

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);

        let mut invalid = Limits::HARD;
        invalid.max_items = 0;
        assert!(invalid.to_extensions().is_err());

        let mut unknown = extensions;
        unknown.0.push(Extension {
            tag: 99,
            required: true,
            value: Vec::new(),
        });
        assert!(Limits::from_extensions(&unknown).is_err());
    }

    fn truncations<T: Encode + Decode + PartialEq + std::fmt::Debug>(value: &T) {
        let bytes = value.encode().unwrap();
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
        assert_eq!(&T::decode(&bytes).unwrap(), value);
    }

    #[test]
    fn set_and_drag_get_round_trip_and_truncate() {
        truncations(&Set {
            slot: crate::schema::selection::SLOT_CLIPBOARD as u8,
            operation_id: [1; 16],
            items: vec![InlineItem {
                mime: "text/plain".into(),
                data: vec![0xff, 0],
            }],
            extensions: Extensions::default(),
        });
        truncations(&Get {
            target: GetTarget::Drag {
                drag_handle: 7,
                revision: 3,
                item_index: 2,
            },
            initial_receive_credit: 4096,
            mime: "text/plain".into(),
            extensions: Extensions::default(),
        });

        let stage = SetBeginResult {
            staging_handle: 9,
            descriptors: vec![staged_item_descriptor(1, 9), staged_item_descriptor(3, 9)],
            extensions: Extensions::default(),
        };
        truncations(&stage);
        let reset = Reset {
            transfer_id: 3,
            status: crate::schema::core::status::CANCELLED,
            detail: Vec::new(),
        };
        assert_eq!(
            stage.stage_discarded_by(&reset).unwrap(),
            Some(stage.upload_stage().unwrap())
        );
    }

    #[test]
    fn drag_position_round_trip_preserves_both_coordinates() {
        let position = DragPosition {
            drag_handle: 7,
            revision: 3,
            target_surface: 11,
            x_32_32: -0x1020_3040_5060,
            y_32_32: 0x1122_3344_5566,
            actions: crate::schema::selection::ACTION_COPY as u16,
        };
        truncations(&position);
    }

    #[test]
    fn drag_drop_requires_the_exact_selected_mime_for_each_item() {
        let current = vec![
            DragItem {
                name: String::new(),
                mime_types: vec!["text/plain".into(), "text/uri-list".into()],
            },
            DragItem {
                name: "image.png".into(),
                mime_types: vec!["image/png".into()],
            },
        ];
        let selected = vec![
            DragDropItem {
                name: "notes.txt".into(),
                selected_mime: "text/plain".into(),
            },
            DragDropItem {
                name: "image.png".into(),
                selected_mime: "image/png".into(),
            },
        ];
        let drop = DragDrop {
            drag_handle: 7,
            revision: 3,
            operation_id: [1; 16],
            selected_action: crate::schema::selection::ACTION_COPY as u16,
            extensions: Extensions(vec![drag_drop_items_extension(&selected).unwrap()]),
        };
        truncations(&drop);
        assert_eq!(drop.selected_items(&current).unwrap(), selected);

        let missing = DragDrop {
            extensions: Extensions::default(),
            ..drop.clone()
        };
        assert!(missing.encode().is_err());

        let mut unoffered = selected;
        unoffered[0].selected_mime = "application/json".into();
        let unoffered = DragDrop {
            extensions: Extensions(vec![drag_drop_items_extension(&unoffered).unwrap()]),
            ..drop.clone()
        };
        assert!(unoffered.selected_items(&current).is_err());

        let mut renamed = drop.selected_items(&current).unwrap();
        renamed[1].name = "replacement.png".into();
        let renamed = DragDrop {
            extensions: Extensions(vec![drag_drop_items_extension(&renamed).unwrap()]),
            ..drop
        };
        assert!(renamed.selected_items(&current).is_err());
    }

    #[test]
    fn mutations_reject_zero_operation_ids() {
        assert!(
            Set {
                slot: crate::schema::selection::SLOT_CLIPBOARD as u8,
                operation_id: [0; 16],
                items: vec![InlineItem {
                    mime: "text/plain".into(),
                    data: b"x".to_vec(),
                }],
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
        assert!(
            SetBegin {
                slot: crate::schema::selection::SLOT_CLIPBOARD as u8,
                operation_id: [0; 16],
                items: vec![UploadItem {
                    mime: "text/plain".into(),
                    byte_len: 1,
                    content_hash: [1; 32],
                    initial_receive_credit: 1,
                }],
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
        assert!(
            SetCommit {
                staging_handle: 1,
                operation_id: [0; 16],
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
        assert!(
            Clear {
                slot: crate::schema::selection::SLOT_CLIPBOARD as u8,
                observed_revision: 1,
                operation_id: [0; 16],
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
        assert!(
            DragBegin {
                operation_id: [0; 16],
                source_actions: crate::schema::selection::ACTION_COPY as u16,
                items: vec![DragItem {
                    name: String::new(),
                    mime_types: vec!["text/plain".into()],
                }],
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
        assert!(
            DragDrop {
                drag_handle: 1,
                revision: 1,
                operation_id: [0; 16],
                selected_action: crate::schema::selection::ACTION_COPY as u16,
                extensions: Extensions(vec![
                    drag_drop_items_extension(&[DragDropItem {
                        name: String::new(),
                        selected_mime: "text/plain".into(),
                    }])
                    .unwrap(),
                ]),
            }
            .encode()
            .is_err()
        );
        assert!(
            DragCancel {
                drag_handle: 1,
                revision: 1,
                operation_id: [0; 16],
                reason: String::new(),
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn state_entity_discriminator_preserves_mutation_semantics() {
        let complete = CompleteEntity::Slot(SelectionRecord {
            slot: crate::schema::selection::SLOT_PRIMARY as u8,
            owner_kind: crate::schema::selection::OWNER_SESSION as u8,
            owner_handle: 9,
            revision: 2,
            mime_types: vec!["text/plain".into()],
            extensions: Extensions::default(),
        });
        let record = complete.state_record(RecordKind::Add).unwrap();
        assert_eq!(
            decode_state_record(&record).unwrap(),
            StateMutation::Complete(complete)
        );

        let removed = RemovedEntity::Drag {
            drag_handle: 8,
            revision: 4,
        };
        let record = removed.state_record().unwrap();
        assert_eq!(
            decode_state_record(&record).unwrap(),
            StateMutation::Remove(removed)
        );
    }

    #[test]
    fn inline_and_action_bounds_are_enforced() {
        assert!(
            Set {
                slot: crate::schema::selection::SLOT_CLIPBOARD as u8,
                operation_id: [0; 16],
                items: vec![InlineItem {
                    mime: "application/octet-stream".into(),
                    data: vec![0; MAX_INLINE_BYTES],
                }],
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
        assert!(
            DragDrop {
                drag_handle: 1,
                revision: 1,
                operation_id: [0; 16],
                selected_action: crate::schema::selection::ACTION_COPY as u16
                    | crate::schema::selection::ACTION_MOVE as u16,
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
    }
}
