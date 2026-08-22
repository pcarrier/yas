//! YAS Environment family wire values.
//!
//! Environment keys and values are deliberately raw bytes. An Env result is
//! always sensitive: it is a complete, unredacted snapshot of the server
//! process environment for one boot.

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, put_bytes_u16, put_bytes_u32,
    put_len_u32, put_u16, put_u32, put_u64,
};
use crate::prelude::*;
use crate::transfer::{Descriptor, Direction, Mode};

pub const VERSION: u16 = crate::schema::env::VERSION;
pub const SNAPSHOT_CONTENT_KIND: u16 = crate::schema::env::SNAPSHOT_CONTENT_KIND as u16;

pub const MAX_KEY_BYTES: usize = crate::schema::env::MAX_KEY_BYTES as usize;
pub const MAX_VALUE_BYTES: usize = crate::schema::env::MAX_VALUE_BYTES as usize;
pub const MAX_ENTRIES: usize = crate::schema::env::MAX_ENTRIES as usize;
pub const MAX_TOTAL_DATA_BYTES: usize = crate::schema::env::MAX_TOTAL_DATA_BYTES as usize;
pub const MAX_INLINE_BYTES: usize = crate::schema::env::MAX_INLINE_BYTES as usize;
pub const MAX_BATCH_BYTES: usize = crate::schema::env::MAX_BATCH_BYTES as usize;

pub mod request_kind {
    pub use crate::schema::env::request::*;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Get {
    /// Initial sender-to-receiver byte credit for a possible MESSAGE Transfer.
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Get {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Get {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        reject_unknown_required(&value.extensions, &[])?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

impl Entry {
    fn validate(&self) -> Result<()> {
        if self.key.is_empty() {
            return Err(Error::Invalid("empty environment key"));
        }
        if self.key.len() > MAX_KEY_BYTES {
            return Err(limit(
                "environment key bytes",
                self.key.len(),
                MAX_KEY_BYTES,
            ));
        }
        if self.value.len() > MAX_VALUE_BYTES {
            return Err(limit(
                "environment value bytes",
                self.value.len(),
                MAX_VALUE_BYTES,
            ));
        }
        if self.key.contains(&0) || self.key.contains(&b'=') {
            return Err(Error::Invalid("environment key"));
        }
        if self.value.contains(&0) {
            return Err(Error::Invalid("environment value"));
        }
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            key: decoder.len_bytes_u16()?.to_vec(),
            value: decoder.len_bytes_u32()?.to_vec(),
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for Entry {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_bytes_u16(out, &self.key)?;
        put_bytes_u32(out, &self.value)
    }
}

impl Decode for Entry {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    Inline(Vec<Entry>),
    Transfer(Descriptor),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetResult {
    pub entry_count: u32,
    /// Sum of raw key and value lengths, excluding wire record overhead.
    pub total_data_bytes: u64,
    pub delivery: Delivery,
    pub extensions: Extensions,
}

impl GetResult {
    pub fn inline(entries: Vec<Entry>, extensions: Extensions) -> Result<Self> {
        let summary = validate_entries(&entries, true)?;
        let value = Self {
            entry_count: summary.count,
            total_data_bytes: summary.total_data_bytes,
            delivery: Delivery::Inline(entries),
            extensions,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn transfer(
        entry_count: u32,
        total_data_bytes: u64,
        transfer: Descriptor,
        extensions: Extensions,
    ) -> Result<Self> {
        let value = Self {
            entry_count,
            total_data_bytes,
            delivery: Delivery::Transfer(transfer),
            extensions,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<()> {
        if self.entry_count as usize > MAX_ENTRIES {
            return Err(limit(
                "environment entries",
                self.entry_count as usize,
                MAX_ENTRIES,
            ));
        }
        if self.total_data_bytes > MAX_TOTAL_DATA_BYTES as u64 {
            return Err(Error::LimitExceeded {
                limit: "environment total data bytes",
                actual: self.total_data_bytes,
                maximum: MAX_TOTAL_DATA_BYTES as u64,
            });
        }
        reject_unknown_required(&self.extensions, &[])?;
        match &self.delivery {
            Delivery::Inline(entries) => {
                let summary = validate_entries(entries, true)?;
                if summary.count != self.entry_count
                    || summary.total_data_bytes != self.total_data_bytes
                {
                    return Err(Error::Invalid("environment result summary"));
                }
            }
            Delivery::Transfer(transfer) => {
                if self.entry_count == 0 || self.total_data_bytes == 0 {
                    return Err(Error::Invalid("empty transferred environment"));
                }
                validate_snapshot_transfer(transfer)?;
            }
        }
        Ok(())
    }
}

impl Encode for GetResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        match &self.delivery {
            Delivery::Inline(entries) => {
                out.push(crate::schema::env::DELIVERY_INLINE as u8);
                out.extend_from_slice(&[0; 3]);
                put_u32(out, self.entry_count);
                put_u64(out, self.total_data_bytes);
                for entry in entries {
                    entry.encode_to(out)?;
                }
            }
            Delivery::Transfer(transfer) => {
                out.push(crate::schema::env::DELIVERY_TRANSFER as u8);
                out.extend_from_slice(&[0; 3]);
                put_u32(out, self.entry_count);
                put_u64(out, self.total_data_bytes);
                let descriptor = transfer.encode()?;
                put_len_u32(out, descriptor.len())?;
                out.extend_from_slice(&descriptor);
            }
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for GetResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let delivery = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Env result reserved bytes"));
        }
        let entry_count = decoder.u32()?;
        let total_data_bytes = decoder.u64()?;
        if entry_count as usize > MAX_ENTRIES {
            return Err(limit(
                "environment entries",
                entry_count as usize,
                MAX_ENTRIES,
            ));
        }
        if total_data_bytes > MAX_TOTAL_DATA_BYTES as u64 {
            return Err(Error::LimitExceeded {
                limit: "environment total data bytes",
                actual: total_data_bytes,
                maximum: MAX_TOTAL_DATA_BYTES as u64,
            });
        }
        let delivery = match delivery {
            value if value == crate::schema::env::DELIVERY_INLINE as u8 => {
                let mut entries = Vec::with_capacity(entry_count as usize);
                for _ in 0..entry_count {
                    entries.push(Entry::decode_from(&mut decoder)?);
                }
                Delivery::Inline(entries)
            }
            value if value == crate::schema::env::DELIVERY_TRANSFER as u8 => {
                Delivery::Transfer(Descriptor::decode(decoder.len_bytes_u32()?)?)
            }
            _ => return Err(Error::Invalid("Env result delivery")),
        };
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            entry_count,
            total_data_bytes,
            delivery,
            extensions,
        };
        value.validate()?;
        Ok(value)
    }
}

/// One nonempty MESSAGE item in an out-of-line environment snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotBatch {
    pub first_index: u32,
    pub entries: Vec<Entry>,
}

impl SnapshotBatch {
    fn validate(&self) -> Result<()> {
        if self.entries.is_empty() {
            return Err(Error::Invalid("empty environment snapshot batch"));
        }
        if self.entries.len() > u16::MAX as usize
            || self.entries.len() > MAX_ENTRIES
            || self
                .first_index
                .checked_add(self.entries.len() as u32)
                .is_none()
        {
            return Err(Error::Invalid("environment snapshot batch count"));
        }
        let summary = validate_entries(&self.entries, false)?;
        let encoded_len = 8usize
            .checked_add(summary.encoded_entry_bytes)
            .ok_or(Error::LengthOverflow)?;
        if encoded_len > MAX_BATCH_BYTES {
            return Err(limit(
                "environment snapshot batch bytes",
                encoded_len,
                MAX_BATCH_BYTES,
            ));
        }
        Ok(())
    }
}

impl Encode for SnapshotBatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.first_index);
        put_u16(
            out,
            u16::try_from(self.entries.len()).map_err(|_| Error::LengthOverflow)?,
        );
        put_u16(out, 0);
        for entry in &self.entries {
            entry.encode_to(out)?;
        }
        Ok(())
    }
}

impl Decode for SnapshotBatch {
    fn decode(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_BATCH_BYTES {
            return Err(limit(
                "environment snapshot batch bytes",
                input.len(),
                MAX_BATCH_BYTES,
            ));
        }
        let mut decoder = Decoder::new(input);
        let first_index = decoder.u32()?;
        let count = decoder.u16()? as usize;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Env snapshot batch reserved bytes"));
        }
        if count == 0 || count > MAX_ENTRIES {
            return Err(Error::Invalid("environment snapshot batch count"));
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(Entry::decode_from(&mut decoder)?);
        }
        decoder.finish()?;
        let value = Self {
            first_index,
            entries,
        };
        value.validate()?;
        Ok(value)
    }
}

/// Validates ordering, indices, totals, and completion across MESSAGE items.
#[derive(Clone, Debug)]
pub struct SnapshotAssembler {
    expected_entry_count: u32,
    expected_total_data_bytes: u64,
    entries: Vec<Entry>,
    total_data_bytes: u64,
}

impl SnapshotAssembler {
    pub fn new(expected_entry_count: u32, expected_total_data_bytes: u64) -> Result<Self> {
        if expected_entry_count == 0 || expected_entry_count as usize > MAX_ENTRIES {
            return Err(Error::Invalid("transferred environment entry count"));
        }
        if expected_total_data_bytes > MAX_TOTAL_DATA_BYTES as u64 {
            return Err(Error::LimitExceeded {
                limit: "environment total data bytes",
                actual: expected_total_data_bytes,
                maximum: MAX_TOTAL_DATA_BYTES as u64,
            });
        }
        if expected_total_data_bytes == 0 {
            return Err(Error::Invalid("transferred environment total data bytes"));
        }
        Ok(Self {
            expected_entry_count,
            expected_total_data_bytes,
            entries: Vec::with_capacity(expected_entry_count as usize),
            total_data_bytes: 0,
        })
    }

    pub fn push(&mut self, batch: SnapshotBatch) -> Result<()> {
        batch.validate()?;
        if batch.first_index as usize != self.entries.len() {
            return Err(Error::Invalid("environment snapshot batch index"));
        }
        let new_count = self
            .entries
            .len()
            .checked_add(batch.entries.len())
            .ok_or(Error::LengthOverflow)?;
        if new_count > self.expected_entry_count as usize {
            return Err(Error::Invalid("environment snapshot entry overrun"));
        }
        if let (Some(previous), Some(first)) = (self.entries.last(), batch.entries.first())
            && previous.key >= first.key
        {
            return Err(Error::Invalid("environment key order"));
        }
        for entry in &batch.entries {
            self.total_data_bytes = self
                .total_data_bytes
                .checked_add((entry.key.len() + entry.value.len()) as u64)
                .ok_or(Error::LengthOverflow)?;
            if self.total_data_bytes > self.expected_total_data_bytes {
                return Err(Error::Invalid("environment snapshot byte overrun"));
            }
        }
        self.entries.extend(batch.entries);
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<Entry>> {
        if self.entries.len() != self.expected_entry_count as usize
            || self.total_data_bytes != self.expected_total_data_bytes
        {
            return Err(Error::Invalid("incomplete environment snapshot"));
        }
        Ok(self.entries)
    }
}

/// Typed Env entries carried in a family descriptor's limit extensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_key_bytes: u32,
    pub max_value_bytes: u32,
    pub max_entries: u32,
    pub max_total_data_bytes: u64,
    pub max_inline_bytes: u32,
    pub max_batch_bytes: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_key_bytes: MAX_KEY_BYTES as u32,
        max_value_bytes: MAX_VALUE_BYTES as u32,
        max_entries: MAX_ENTRIES as u32,
        max_total_data_bytes: MAX_TOTAL_DATA_BYTES as u64,
        max_inline_bytes: MAX_INLINE_BYTES as u32,
        max_batch_bytes: MAX_BATCH_BYTES as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        if self.max_key_bytes == 0 || self.max_key_bytes > hard.max_key_bytes {
            return Err(Error::Invalid("Env max key bytes limit"));
        }
        if self.max_value_bytes > hard.max_value_bytes {
            return Err(Error::Invalid("Env max value bytes limit"));
        }
        if self.max_entries == 0 || self.max_entries > hard.max_entries {
            return Err(Error::Invalid("Env max entries limit"));
        }
        if self.max_total_data_bytes == 0 || self.max_total_data_bytes > hard.max_total_data_bytes {
            return Err(Error::Invalid("Env max total data bytes limit"));
        }
        if self.max_inline_bytes == 0 || self.max_inline_bytes > hard.max_inline_bytes {
            return Err(Error::Invalid("Env max inline bytes limit"));
        }
        if self.max_batch_bytes == 0 || self.max_batch_bytes > hard.max_batch_bytes {
            return Err(Error::Invalid("Env max batch bytes limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(crate::schema::env::LIMIT_MAX_KEY_BYTES, self.max_key_bytes),
            limit_u32(
                crate::schema::env::LIMIT_MAX_VALUE_BYTES,
                self.max_value_bytes,
            ),
            limit_u32(crate::schema::env::LIMIT_MAX_ENTRIES, self.max_entries),
            limit_u64(
                crate::schema::env::LIMIT_MAX_TOTAL_DATA_BYTES,
                self.max_total_data_bytes,
            ),
            limit_u32(
                crate::schema::env::LIMIT_MAX_INLINE_BYTES,
                self.max_inline_bytes,
            ),
            limit_u32(
                crate::schema::env::LIMIT_MAX_BATCH_BYTES,
                self.max_batch_bytes,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required(
            extensions,
            &[
                crate::schema::env::LIMIT_MAX_KEY_BYTES as u16,
                crate::schema::env::LIMIT_MAX_VALUE_BYTES as u16,
                crate::schema::env::LIMIT_MAX_ENTRIES as u16,
                crate::schema::env::LIMIT_MAX_TOTAL_DATA_BYTES as u16,
                crate::schema::env::LIMIT_MAX_INLINE_BYTES as u16,
                crate::schema::env::LIMIT_MAX_BATCH_BYTES as u16,
            ],
        )?;
        let value = Self {
            max_key_bytes: read_limit_u32(extensions, crate::schema::env::LIMIT_MAX_KEY_BYTES)?,
            max_value_bytes: read_limit_u32(extensions, crate::schema::env::LIMIT_MAX_VALUE_BYTES)?,
            max_entries: read_limit_u32(extensions, crate::schema::env::LIMIT_MAX_ENTRIES)?,
            max_total_data_bytes: read_limit_u64(
                extensions,
                crate::schema::env::LIMIT_MAX_TOTAL_DATA_BYTES,
            )?,
            max_inline_bytes: read_limit_u32(
                extensions,
                crate::schema::env::LIMIT_MAX_INLINE_BYTES,
            )?,
            max_batch_bytes: read_limit_u32(extensions, crate::schema::env::LIMIT_MAX_BATCH_BYTES)?,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy)]
struct EntrySummary {
    count: u32,
    total_data_bytes: u64,
    encoded_entry_bytes: usize,
}

fn validate_entries(entries: &[Entry], inline: bool) -> Result<EntrySummary> {
    if entries.len() > MAX_ENTRIES {
        return Err(limit("environment entries", entries.len(), MAX_ENTRIES));
    }
    let mut previous: Option<&[u8]> = None;
    let mut total_data_bytes = 0usize;
    let mut encoded_entry_bytes = 0usize;
    for entry in entries {
        entry.validate()?;
        if previous.is_some_and(|key| key >= entry.key.as_slice()) {
            return Err(Error::Invalid("environment key order"));
        }
        previous = Some(&entry.key);
        total_data_bytes = total_data_bytes
            .checked_add(entry.key.len())
            .and_then(|value| value.checked_add(entry.value.len()))
            .ok_or(Error::LengthOverflow)?;
        if total_data_bytes > MAX_TOTAL_DATA_BYTES {
            return Err(limit(
                "environment total data bytes",
                total_data_bytes,
                MAX_TOTAL_DATA_BYTES,
            ));
        }
        encoded_entry_bytes = encoded_entry_bytes
            .checked_add(2 + entry.key.len() + 4 + entry.value.len())
            .ok_or(Error::LengthOverflow)?;
    }
    if inline && encoded_entry_bytes > MAX_INLINE_BYTES {
        return Err(limit(
            "inline environment bytes",
            encoded_entry_bytes,
            MAX_INLINE_BYTES,
        ));
    }
    Ok(EntrySummary {
        count: entries.len() as u32,
        total_data_bytes: total_data_bytes as u64,
        encoded_entry_bytes,
    })
}

fn validate_snapshot_transfer(transfer: &Descriptor) -> Result<()> {
    if transfer.mode != Mode::Message
        || transfer.direction != Direction::SENDER_TO_RECEIVER
        || transfer.content_family != crate::family::ENV
        || transfer.content_kind != SNAPSHOT_CONTENT_KIND
        || transfer.content_version != VERSION
        || transfer.max_item_bytes == 0
        || transfer.max_item_bytes > MAX_BATCH_BYTES as u64
        || !transfer.sensitive_content()?
        || !transfer.extensions.0.iter().any(|extension| {
            extension.tag == crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16
                && extension.required
        })
    {
        return Err(Error::Invalid("Env snapshot Transfer descriptor"));
    }
    transfer.validate()
}

fn reject_unknown_required(extensions: &Extensions, known: &[u16]) -> Result<()> {
    extensions.validate()?;
    if extensions
        .0
        .iter()
        .any(|extension| extension.required && !known.contains(&extension.tag))
    {
        return Err(Error::Invalid("unknown required Env extension"));
    }
    Ok(())
}

fn limit(name: &'static str, actual: usize, maximum: usize) -> Error {
    Error::LimitExceeded {
        limit: name,
        actual: actual as u64,
        maximum: maximum as u64,
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
    let value = unique_extension(extensions, tag)?;
    Ok(u32::from_le_bytes(
        value
            .try_into()
            .map_err(|_| Error::Invalid("Env family limit length"))?,
    ))
}

fn read_limit_u64(extensions: &Extensions, tag: u64) -> Result<u64> {
    let value = unique_extension(extensions, tag)?;
    Ok(u64::from_le_bytes(
        value
            .try_into()
            .map_err(|_| Error::Invalid("Env family limit length"))?,
    ))
}

fn unique_extension(extensions: &Extensions, tag: u64) -> Result<&[u8]> {
    extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .map(|extension| extension.value.as_slice())
        .ok_or(Error::Invalid("missing Env family limit"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Extension;

    fn entries() -> Vec<Entry> {
        vec![
            Entry {
                key: b"EMPTY".to_vec(),
                value: Vec::new(),
            },
            Entry {
                key: b"HOME".to_vec(),
                value: b"/home/example".to_vec(),
            },
            Entry {
                key: vec![0xff],
                value: vec![0xfe, b'='],
            },
        ]
    }

    fn descriptor(id: u32) -> Descriptor {
        Descriptor {
            transfer_id: id,
            mode: Mode::Message,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: 64 * 1024,
            max_item_bytes: MAX_BATCH_BYTES as u64,
            max_chunk_bytes: 64 * 1024,
            content_family: crate::family::ENV,
            content_kind: SNAPSHOT_CONTENT_KIND,
            content_version: VERSION,
            extensions: Extensions(vec![Extension {
                tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    fn every_truncation<T: Decode + Encode + PartialEq + std::fmt::Debug>(value: &T) {
        let bytes = value.encode().unwrap();
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
        assert_eq!(&T::decode(&bytes).unwrap(), value);
    }

    #[test]
    fn get_round_trips_and_rejects_required_unknown_extensions() {
        every_truncation(&Get {
            initial_receive_credit: 1 << 20,
            extensions: Extensions::default(),
        });
        assert!(
            Get {
                initial_receive_credit: 0,
                extensions: Extensions(vec![Extension {
                    tag: 77,
                    required: true,
                    value: vec![],
                }]),
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn inline_result_round_trips_raw_bytes_deterministically() {
        let result = GetResult::inline(entries(), Extensions::default()).unwrap();
        every_truncation(&result);
        assert_eq!(result.entry_count, 3);
        assert_eq!(result.encode().unwrap(), result.encode().unwrap());
    }

    #[test]
    fn transfer_result_and_batches_round_trip() {
        let result = GetResult::transfer(3, 32, descriptor(2), Extensions::default()).unwrap();
        every_truncation(&result);

        let first = SnapshotBatch {
            first_index: 0,
            entries: entries()[..2].to_vec(),
        };
        let second = SnapshotBatch {
            first_index: 2,
            entries: entries()[2..].to_vec(),
        };
        every_truncation(&first);
        every_truncation(&second);
        let total = entries()
            .iter()
            .map(|entry| entry.key.len() + entry.value.len())
            .sum::<usize>() as u64;
        let mut assembler = SnapshotAssembler::new(3, total).unwrap();
        assembler.push(first).unwrap();
        assembler.push(second).unwrap();
        assert_eq!(assembler.finish().unwrap(), entries());
    }

    #[test]
    fn snapshots_reject_reordering_gaps_and_summary_mismatch() {
        let mut reversed = entries();
        reversed.swap(0, 1);
        assert!(GetResult::inline(reversed, Extensions::default()).is_err());

        let mut assembler = SnapshotAssembler::new(3, 999).unwrap();
        assert!(
            assembler
                .push(SnapshotBatch {
                    first_index: 1,
                    entries: entries()[..1].to_vec(),
                })
                .is_err()
        );

        let mut encoded = GetResult::inline(entries(), Extensions::default())
            .unwrap()
            .encode()
            .unwrap();
        encoded[4] = 2;
        assert!(GetResult::decode(&encoded).is_err());
    }

    #[test]
    fn entries_enforce_process_and_allocation_limits() {
        for invalid in [
            Entry {
                key: Vec::new(),
                value: Vec::new(),
            },
            Entry {
                key: b"A=B".to_vec(),
                value: Vec::new(),
            },
            Entry {
                key: b"A".to_vec(),
                value: b"x\0y".to_vec(),
            },
        ] {
            assert!(invalid.encode().is_err());
        }
        assert!(
            Entry {
                key: b"A".to_vec(),
                value: vec![0; MAX_VALUE_BYTES + 1],
            }
            .encode()
            .is_err()
        );

        let mut oversized_count = Vec::new();
        oversized_count.push(crate::schema::env::DELIVERY_INLINE as u8);
        oversized_count.extend_from_slice(&[0; 3]);
        oversized_count.extend_from_slice(&((MAX_ENTRIES + 1) as u32).to_le_bytes());
        oversized_count.extend_from_slice(&0u64.to_le_bytes());
        assert!(matches!(
            GetResult::decode(&oversized_count),
            Err(Error::LimitExceeded { .. })
        ));
    }

    #[test]
    fn transfer_must_be_sensitive_message_data_for_env() {
        let mut invalid = descriptor(2);
        invalid.mode = Mode::Byte;
        invalid.max_item_bytes = 0;
        assert!(GetResult::transfer(1, 1, invalid, Extensions::default()).is_err());

        let mut invalid = descriptor(2);
        invalid.extensions = Extensions::default();
        assert!(GetResult::transfer(1, 1, invalid, Extensions::default()).is_err());
    }

    #[test]
    fn family_limits_round_trip() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
        let mut missing = extensions;
        missing.0.pop();
        assert!(Limits::from_extensions(&missing).is_err());
    }
}
