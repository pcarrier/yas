use crate::codec::{
    Decode, Decoder, Encode, Error, Extensions, Result, put_len_u32, put_u16, put_u32, put_u64,
};
use crate::prelude::*;

pub const WATCH_RESUME: u16 = crate::schema::state::WATCH_RESUME as u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub boot_id: [u8; 16],
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub initial_credit: u64,
    pub resume: Option<Cursor>,
    pub extensions: Extensions,
}

impl Encode for Watch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        put_u16(
            out,
            if self.resume.is_some() {
                WATCH_RESUME
            } else {
                0
            },
        );
        put_u16(out, 0);
        put_u64(out, self.initial_credit);
        if let Some(cursor) = self.resume {
            if cursor.revision == 0 {
                return Err(Error::Invalid("zero state revision"));
            }
            out.extend_from_slice(&cursor.boot_id);
            put_u64(out, cursor.revision);
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Watch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if flags & !WATCH_RESUME != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("WATCH flags or reserved field"));
        }
        let initial_credit = decoder.u64()?;
        let resume = if flags & WATCH_RESUME != 0 {
            let cursor = Cursor {
                boot_id: decoder.array_16()?,
                revision: decoder.u64()?,
            };
            if cursor.revision == 0 {
                return Err(Error::Invalid("zero state revision"));
            }
            Some(cursor)
        } else {
            None
        };
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        Ok(Self {
            initial_credit,
            resume,
            extensions,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WatchMode {
    Snapshot = crate::schema::state::MODE_SNAPSHOT as u8,
    Replay = crate::schema::state::MODE_REPLAY as u8,
}

impl TryFrom<u8> for WatchMode {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::state::MODE_SNAPSHOT as u8 => Ok(Self::Snapshot),
            value if value == crate::schema::state::MODE_REPLAY as u8 => Ok(Self::Replay),
            _ => Err(Error::Invalid("WATCH result mode")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchResult {
    pub subscription_id: u32,
    pub mode: WatchMode,
    pub current_revision: u64,
    pub extensions: Extensions,
}

impl Encode for WatchResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.subscription_id == 0 || self.current_revision == 0 {
            return Err(Error::Invalid("WATCH result identity or revision"));
        }
        put_u32(out, self.subscription_id);
        out.push(self.mode as u8);
        out.extend_from_slice(&[0; 3]);
        put_u64(out, self.current_revision);
        self.extensions.encode_tail(out)
    }
}

impl Decode for WatchResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let subscription_id = decoder.u32()?;
        let mode = WatchMode::try_from(decoder.u8()?)?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("WATCH result reserved bytes"));
        }
        let current_revision = decoder.u64()?;
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            subscription_id,
            mode,
            current_revision,
            extensions,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

/// Common idempotent UNWATCH request body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unwatch {
    pub subscription_id: u32,
}

impl Encode for Unwatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.subscription_id == 0 {
            return Err(Error::Invalid("zero subscription ID"));
        }
        put_u32(out, self.subscription_id);
        Ok(())
    }
}

impl Decode for Unwatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            subscription_id: decoder.u32()?,
        };
        decoder.finish()?;
        if value.subscription_id == 0 {
            return Err(Error::Invalid("zero subscription ID"));
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    SnapshotBegin = crate::schema::state::PHASE_SNAPSHOT_BEGIN as u8,
    SnapshotRecords = crate::schema::state::PHASE_SNAPSHOT_RECORDS as u8,
    SnapshotEnd = crate::schema::state::PHASE_SNAPSHOT_END as u8,
    Delta = crate::schema::state::PHASE_DELTA as u8,
    Reset = crate::schema::state::PHASE_RESET as u8,
}

impl TryFrom<u8> for Phase {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::state::PHASE_SNAPSHOT_BEGIN as u8 => {
                Ok(Self::SnapshotBegin)
            }
            value if value == crate::schema::state::PHASE_SNAPSHOT_RECORDS as u8 => {
                Ok(Self::SnapshotRecords)
            }
            value if value == crate::schema::state::PHASE_SNAPSHOT_END as u8 => {
                Ok(Self::SnapshotEnd)
            }
            value if value == crate::schema::state::PHASE_DELTA as u8 => Ok(Self::Delta),
            value if value == crate::schema::state::PHASE_RESET as u8 => Ok(Self::Reset),
            _ => Err(Error::Invalid("STATE phase")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    Add,
    Replace,
    Patch,
    Remove,
    /// A family-reserved record kind known by the selected family version.
    Family(u16),
}

impl RecordKind {
    pub const fn wire(self) -> u16 {
        match self {
            Self::Add => crate::schema::state::RECORD_ADD as u16,
            Self::Replace => crate::schema::state::RECORD_REPLACE as u16,
            Self::Patch => crate::schema::state::RECORD_PATCH as u16,
            Self::Remove => crate::schema::state::RECORD_REMOVE as u16,
            Self::Family(kind) => kind,
        }
    }

    pub fn family(kind: u16) -> Result<Self> {
        if kind <= crate::schema::state::RECORD_REMOVE as u16 {
            Err(Error::Invalid("family state record kind"))
        } else {
            Ok(Self::Family(kind))
        }
    }

    fn known(value: u16, family_kinds: &[u16]) -> Option<Self> {
        match value {
            value if value == crate::schema::state::RECORD_ADD as u16 => Some(Self::Add),
            value if value == crate::schema::state::RECORD_REPLACE as u16 => Some(Self::Replace),
            value if value == crate::schema::state::RECORD_PATCH as u16 => Some(Self::Patch),
            value if value == crate::schema::state::RECORD_REMOVE as u16 => Some(Self::Remove),
            value if family_kinds.contains(&value) => Some(Self::Family(value)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: RecordKind,
    pub required: bool,
    pub body: Vec<u8>,
}

impl Record {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if matches!(self.kind, RecordKind::Family(kind) if kind <= crate::schema::state::RECORD_REMOVE as u16)
        {
            return Err(Error::Invalid("family state record kind"));
        }
        let body_len = 4usize
            .checked_add(self.body.len())
            .ok_or(Error::LengthOverflow)?;
        put_len_u32(out, body_len)?;
        put_u16(out, self.kind.wire());
        put_u16(out, u16::from(self.required));
        out.extend_from_slice(&self.body);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateEvent {
    pub subscription_id: u32,
    pub phase: Phase,
    /// Family-defined flags. Version-1 Relay and Font use zero.
    pub flags: u8,
    pub from_revision: u64,
    pub to_revision: u64,
    pub records: Vec<Record>,
}

impl StateEvent {
    pub fn validate(&self) -> Result<()> {
        if self.subscription_id == 0 || self.to_revision == 0 {
            return Err(Error::Invalid("STATE identity or revision"));
        }
        match self.phase {
            Phase::SnapshotBegin if self.from_revision == 0 => {}
            Phase::SnapshotRecords | Phase::SnapshotEnd
                if self.from_revision == self.to_revision => {}
            Phase::Delta | Phase::Reset
                if self.from_revision != 0 && self.from_revision < self.to_revision => {}
            _ => return Err(Error::Invalid("STATE revision transition")),
        }
        if matches!(self.phase, Phase::SnapshotBegin | Phase::Reset) && !self.records.is_empty() {
            return Err(Error::Invalid("records in marker STATE event"));
        }
        if self.records.len() > crate::schema::transport::HARD_MAX_TYPED_RECORDS {
            return Err(Error::LimitExceeded {
                limit: "typed records",
                actual: self.records.len() as u64,
                maximum: crate::schema::transport::HARD_MAX_TYPED_RECORDS as u64,
            });
        }
        Ok(())
    }

    /// Decode using the selected family's STATE flag mask and reserved record
    /// kinds. Optional unknown records are skipped; required unknown records
    /// are rejected without losing known family records.
    pub fn decode_with(
        input: &[u8],
        allowed_flags: u8,
        family_record_kinds: &[u16],
    ) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let subscription_id = decoder.u32()?;
        let phase = Phase::try_from(decoder.u8()?)?;
        let flags = decoder.u8()?;
        if flags & !allowed_flags != 0 {
            return Err(Error::Invalid("family STATE flags"));
        }
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("STATE reserved field"));
        }
        let from_revision = decoder.u64()?;
        let to_revision = decoder.u64()?;
        let record_count = decoder.u16()?;
        if usize::from(record_count) > crate::schema::transport::HARD_MAX_TYPED_RECORDS {
            return Err(Error::LimitExceeded {
                limit: "typed records",
                actual: u64::from(record_count),
                maximum: crate::schema::transport::HARD_MAX_TYPED_RECORDS as u64,
            });
        }
        let mut records = Vec::with_capacity(usize::from(record_count));
        for _ in 0..record_count {
            let record_len = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
            if record_len < 4 {
                return Err(Error::Invalid("state record length"));
            }
            let mut record = Decoder::new(decoder.take(record_len)?);
            let raw_kind = record.u16()?;
            let record_flags = record.u16()?;
            if record_flags & !1 != 0 {
                return Err(Error::Invalid("state record flags"));
            }
            let required = record_flags & 1 != 0;
            match RecordKind::known(raw_kind, family_record_kinds) {
                Some(kind) => records.push(Record {
                    kind,
                    required,
                    body: record.rest().to_vec(),
                }),
                None if !required => {
                    record.rest();
                }
                None => return Err(Error::Invalid("unknown required state record")),
            }
            record.finish()?;
        }
        decoder.finish()?;
        let value = Self {
            subscription_id,
            phase,
            flags,
            from_revision,
            to_revision,
            records,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for StateEvent {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.subscription_id);
        out.push(self.phase as u8);
        out.push(self.flags);
        put_u16(out, 0);
        put_u64(out, self.from_revision);
        put_u64(out, self.to_revision);
        put_u16(
            out,
            u16::try_from(self.records.len()).map_err(|_| Error::LengthOverflow)?,
        );
        for record in &self.records {
            record.encode_to(out)?;
        }
        Ok(())
    }
}

impl Decode for StateEvent {
    fn decode(input: &[u8]) -> Result<Self> {
        Self::decode_with(input, u8::MAX, &[])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateAck {
    pub subscription_id: u32,
    pub applied_revision: u64,
    pub cumulative_byte_limit: u64,
}

impl Encode for StateAck {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.subscription_id == 0 {
            return Err(Error::Invalid("zero subscription ID"));
        }
        put_u32(out, self.subscription_id);
        put_u64(out, self.applied_revision);
        put_u64(out, self.cumulative_byte_limit);
        Ok(())
    }
}

impl Decode for StateAck {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            subscription_id: decoder.u32()?,
            applied_revision: decoder.u64()?,
            cumulative_byte_limit: decoder.u64()?,
        };
        decoder.finish()?;
        if value.subscription_id == 0 {
            return Err(Error::Invalid("zero subscription ID"));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_resume_golden_and_truncation() {
        let value = Watch {
            initial_credit: 9,
            resume: Some(Cursor {
                boot_id: [7; 16],
                revision: 11,
            }),
            extensions: Extensions::default(),
        };
        let bytes = value.encode().unwrap();
        assert_eq!(&bytes[..12], &[1, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(Watch::decode(&bytes).unwrap(), value);
        for end in 0..bytes.len() {
            assert!(Watch::decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn state_event_records_round_trip_and_skip_unknown_optional() {
        let value = StateEvent {
            subscription_id: 1,
            phase: Phase::Delta,
            flags: 0,
            from_revision: 1,
            to_revision: 2,
            records: vec![Record {
                kind: RecordKind::Replace,
                required: false,
                body: vec![4, 5],
            }],
        };
        let bytes = value.encode().unwrap();
        assert_eq!(StateEvent::decode(&bytes).unwrap(), value);

        let mut unknown = bytes[..26].to_vec();
        unknown.extend_from_slice(&[4, 0, 0, 0, 99, 0, 0, 0]);
        assert_eq!(StateEvent::decode(&unknown).unwrap().records, Vec::new());

        unknown[32] = 1;
        assert_eq!(
            StateEvent::decode(&unknown),
            Err(Error::Invalid("unknown required state record"))
        );
    }

    #[test]
    fn family_record_kinds_and_flags_require_family_context() {
        let value = StateEvent {
            subscription_id: 1,
            phase: Phase::Delta,
            flags: 4,
            from_revision: 1,
            to_revision: 2,
            records: vec![Record {
                kind: RecordKind::family(16).unwrap(),
                required: true,
                body: vec![9],
            }],
        };
        let bytes = value.encode().unwrap();
        assert_eq!(
            StateEvent::decode_with(&bytes, 0, &[16]),
            Err(Error::Invalid("family STATE flags"))
        );
        assert_eq!(
            StateEvent::decode_with(&bytes, 4, &[]),
            Err(Error::Invalid("unknown required state record"))
        );
        assert_eq!(StateEvent::decode_with(&bytes, 4, &[16]).unwrap(), value);
    }

    #[test]
    fn snapshot_end_may_carry_final_records() {
        let record = Record {
            kind: RecordKind::Add,
            required: false,
            body: vec![1, 2, 3],
        };
        let end = StateEvent {
            subscription_id: 1,
            phase: Phase::SnapshotEnd,
            flags: 0,
            from_revision: 7,
            to_revision: 7,
            records: vec![record.clone()],
        };
        let bytes = end.encode().unwrap();
        assert_eq!(StateEvent::decode(&bytes).unwrap(), end);

        for phase in [Phase::SnapshotBegin, Phase::Reset] {
            let invalid = StateEvent {
                subscription_id: 1,
                phase,
                flags: 0,
                from_revision: if phase == Phase::SnapshotBegin { 0 } else { 6 },
                to_revision: 7,
                records: vec![record.clone()],
            };
            assert_eq!(
                invalid.encode(),
                Err(Error::Invalid("records in marker STATE event"))
            );
        }
    }
}
