use crate::codec::{
    Decode, Decoder, Encode, Error, Extensions, Result, limit_u32, limit_u64, put_i16, put_i32,
    put_len_u32, put_string_u16, put_u16, put_u64, read_limit_u32, read_limit_u64,
    reject_unknown_required_extensions,
};
use crate::prelude::*;
use crate::state::{Record, RecordKind};
use crate::transfer::{Descriptor, Direction, Mode};

pub const VERSION: u16 = crate::schema::font::VERSION;
pub const DESCRIPTION_CONTENT_KIND: u16 = crate::schema::font::DESCRIPTION_CONTENT_KIND as u16;
pub const FACE_BYTES_CONTENT_KIND: u16 = crate::schema::font::FACE_BYTES_CONTENT_KIND as u16;
pub const MAX_INLINE_DESCRIPTION: usize = 32 * 1024;

pub mod request_kind {
    pub use crate::schema::font::request::*;
}

pub mod event_kind {
    pub use crate::schema::font::event::*;
}

pub const FAMILY_MONOSPACE: u16 = crate::schema::font::FAMILY_MONOSPACE as u16;
pub const FAMILY_VARIABLE: u16 = crate::schema::font::FAMILY_VARIABLE as u16;
pub const FAMILY_COLOR: u16 = crate::schema::font::FAMILY_COLOR as u16;
pub const FAMILY_FETCHABLE: u16 = crate::schema::font::FAMILY_FETCHABLE as u16;
const FAMILY_FLAGS: u16 = FAMILY_MONOSPACE | FAMILY_VARIABLE | FAMILY_COLOR | FAMILY_FETCHABLE;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyRecord {
    pub font_handle: u64,
    pub generation: u64,
    pub flags: u16,
    pub face_count: u16,
    pub family: String,
    pub display: String,
    pub extensions: Extensions,
}

impl FamilyRecord {
    fn validate(&self) -> Result<()> {
        if self.font_handle == 0 || self.family.is_empty() || self.flags & !FAMILY_FLAGS != 0 {
            return Err(Error::Invalid("Font family record"));
        }
        self.extensions.validate()
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Font family state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for FamilyRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.font_handle);
        put_u64(out, self.generation);
        put_u16(out, self.flags);
        put_u16(out, self.face_count);
        put_string_u16(out, &self.family)?;
        put_string_u16(out, &self.display)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for FamilyRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            font_handle: decoder.u64()?,
            generation: decoder.u64()?,
            flags: decoder.u16()?,
            face_count: decoder.u16()?,
            family: decoder.string_u16()?,
            display: decoder.string_u16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedFamily {
    pub font_handle: u64,
    pub generation: u64,
}

impl RemovedFamily {
    pub fn state_record(self) -> Result<Record> {
        if self.font_handle == 0 {
            return Err(Error::Invalid("Font removed family identity"));
        }
        let mut body = Vec::with_capacity(16);
        put_u64(&mut body, self.font_handle);
        put_u64(&mut body, self.generation);
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if record.kind != RecordKind::Remove {
            return Err(Error::Invalid("Font remove state record kind"));
        }
        let mut decoder = Decoder::new(&record.body);
        let value = Self {
            font_handle: decoder.u64()?,
            generation: decoder.u64()?,
        };
        decoder.finish()?;
        if value.font_handle == 0 {
            return Err(Error::Invalid("Font removed family identity"));
        }
        Ok(value)
    }
}

pub fn family_from_state_record(record: &Record) -> Result<FamilyRecord> {
    if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
        return Err(Error::Invalid("Font family state record kind"));
    }
    FamilyRecord::decode(&record.body)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Describe {
    pub font_handle: u64,
    pub generation: u64,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Describe {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.font_handle == 0 {
            return Err(Error::Invalid("Font DESCRIBE identity"));
        }
        put_u64(out, self.font_handle);
        put_u64(out, self.generation);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Describe {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            font_handle: decoder.u64()?,
            generation: decoder.u64()?,
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        if value.font_handle == 0 {
            return Err(Error::Invalid("Font DESCRIBE identity"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DescriptionDelivery {
    Inline(Vec<u8>),
    Transfer {
        description_len: u64,
        transfer: Descriptor,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescribeResult {
    pub font_handle: u64,
    pub generation: u64,
    pub description_hash: [u8; 32],
    pub delivery: DescriptionDelivery,
}

impl DescribeResult {
    fn validate(&self) -> Result<()> {
        if self.font_handle == 0 {
            return Err(Error::Invalid("Font DESCRIBE result identity"));
        }
        match &self.delivery {
            DescriptionDelivery::Inline(bytes) if bytes.len() <= MAX_INLINE_DESCRIPTION => Ok(()),
            DescriptionDelivery::Inline(bytes) => Err(Error::LimitExceeded {
                limit: "inline font description",
                actual: bytes.len() as u64,
                maximum: MAX_INLINE_DESCRIPTION as u64,
            }),
            DescriptionDelivery::Transfer {
                description_len,
                transfer,
            } => validate_font_transfer(transfer, DESCRIPTION_CONTENT_KIND, *description_len),
        }
    }
}

impl Encode for DescribeResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.font_handle);
        put_u64(out, self.generation);
        out.extend_from_slice(&self.description_hash);
        match &self.delivery {
            DescriptionDelivery::Inline(bytes) => {
                out.push(crate::schema::font::DELIVERY_INLINE as u8);
                out.extend_from_slice(&[0; 3]);
                put_len_u32(out, bytes.len())?;
                out.extend_from_slice(bytes);
            }
            DescriptionDelivery::Transfer {
                description_len,
                transfer,
            } => {
                out.push(crate::schema::font::DELIVERY_TRANSFER as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *description_len);
                transfer.encode_to(out)?;
            }
        }
        Ok(())
    }
}

impl Decode for DescribeResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let font_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let description_hash = decoder.array_32()?;
        let delivery_kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Font DESCRIBE delivery reserved bytes"));
        }
        let delivery = match delivery_kind {
            value if value == crate::schema::font::DELIVERY_INLINE as u8 => {
                DescriptionDelivery::Inline(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::font::DELIVERY_TRANSFER as u8 => {
                let description_len = decoder.u64()?;
                let transfer = Descriptor::decode(decoder.rest())?;
                DescriptionDelivery::Transfer {
                    description_len,
                    transfer,
                }
            }
            _ => return Err(Error::Invalid("Font DESCRIBE delivery")),
        };
        decoder.finish()?;
        let value = Self {
            font_handle,
            generation,
            description_hash,
            delivery,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Format {
    SfntTrueType = crate::schema::font::FORMAT_SFNT_TRUETYPE as u8,
    SfntCff = crate::schema::font::FORMAT_SFNT_CFF as u8,
    Woff = crate::schema::font::FORMAT_WOFF as u8,
    Woff2 = crate::schema::font::FORMAT_WOFF2 as u8,
}

impl TryFrom<u8> for Format {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::font::FORMAT_SFNT_TRUETYPE as u8 => {
                Ok(Self::SfntTrueType)
            }
            value if value == crate::schema::font::FORMAT_SFNT_CFF as u8 => Ok(Self::SfntCff),
            value if value == crate::schema::font::FORMAT_WOFF as u8 => Ok(Self::Woff),
            value if value == crate::schema::font::FORMAT_WOFF2 as u8 => Ok(Self::Woff2),
            _ => Err(Error::Invalid("Font format")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Style {
    Normal = crate::schema::font::STYLE_NORMAL as u8,
    Italic = crate::schema::font::STYLE_ITALIC as u8,
    Oblique = crate::schema::font::STYLE_OBLIQUE as u8,
}

impl TryFrom<u8> for Style {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::font::STYLE_NORMAL as u8 => Ok(Self::Normal),
            value if value == crate::schema::font::STYLE_ITALIC as u8 => Ok(Self::Italic),
            value if value == crate::schema::font::STYLE_OBLIQUE as u8 => Ok(Self::Oblique),
            _ => Err(Error::Invalid("Font style")),
        }
    }
}

pub const FACE_VARIABLE: u16 = crate::schema::font::FACE_VARIABLE as u16;
pub const FACE_COLOR: u16 = crate::schema::font::FACE_COLOR as u16;
pub const FACE_FETCHABLE: u16 = crate::schema::font::FACE_FETCHABLE as u16;
const FACE_FLAGS: u16 = FACE_VARIABLE | FACE_COLOR | FACE_FETCHABLE;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceRecord {
    pub face_handle: u64,
    pub content_hash: [u8; 32],
    pub byte_len: u64,
    pub format: Format,
    pub style: Style,
    pub flags: u16,
    pub weight_min: u16,
    pub weight_default: u16,
    pub weight_max: u16,
    pub stretch_min: u16,
    pub stretch_default: u16,
    pub stretch_max: u16,
    pub slant_tenths_degrees: i16,
    pub units_per_em: u16,
    pub cell_advance: i32,
    pub ascent: i32,
    pub descent: i32,
    pub line_gap: i32,
    pub subfamily: String,
    pub postscript: String,
    pub extensions: Extensions,
}

impl FaceRecord {
    fn validate(&self) -> Result<()> {
        if self.face_handle == 0
            || self.flags & !FACE_FLAGS != 0
            || !(1..=1000).contains(&self.weight_min)
            || self.weight_max > 1000
            || !(self.weight_min..=self.weight_max).contains(&self.weight_default)
            || self.stretch_min == 0
            || !(self.stretch_min..=self.stretch_max).contains(&self.stretch_default)
            || self.units_per_em == 0
        {
            return Err(Error::Invalid("Font face record"));
        }
        self.extensions.validate()
    }

    fn encode_record(&self, out: &mut Vec<u8>) -> Result<()> {
        let mut body = Vec::new();
        self.encode_to(&mut body)?;
        put_len_u32(out, body.len())?;
        out.extend_from_slice(&body);
        Ok(())
    }

    fn decode_record(decoder: &mut Decoder<'_>) -> Result<Self> {
        Self::decode(decoder.len_bytes_u32()?)
    }
}

impl Encode for FaceRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.face_handle);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.byte_len);
        out.push(self.format as u8);
        out.push(self.style as u8);
        put_u16(out, self.flags);
        for value in [self.weight_min, self.weight_default, self.weight_max] {
            put_u16(out, value);
        }
        for value in [self.stretch_min, self.stretch_default, self.stretch_max] {
            put_u16(out, value);
        }
        put_i16(out, self.slant_tenths_degrees);
        put_u16(out, self.units_per_em);
        for value in [self.cell_advance, self.ascent, self.descent, self.line_gap] {
            put_i32(out, value);
        }
        put_string_u16(out, &self.subfamily)?;
        put_string_u16(out, &self.postscript)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for FaceRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            face_handle: decoder.u64()?,
            content_hash: decoder.array_32()?,
            byte_len: decoder.u64()?,
            format: Format::try_from(decoder.u8()?)?,
            style: Style::try_from(decoder.u8()?)?,
            flags: decoder.u16()?,
            weight_min: decoder.u16()?,
            weight_default: decoder.u16()?,
            weight_max: decoder.u16()?,
            stretch_min: decoder.u16()?,
            stretch_default: decoder.u16()?,
            stretch_max: decoder.u16()?,
            slant_tenths_degrees: decoder.i16()?,
            units_per_em: decoder.u16()?,
            cell_advance: decoder.i32()?,
            ascent: decoder.i32()?,
            descent: decoder.i32()?,
            line_gap: decoder.i32()?,
            subfamily: decoder.string_u16()?,
            postscript: decoder.string_u16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Description {
    pub family: String,
    pub faces: Vec<FaceRecord>,
    pub extensions: Extensions,
}

impl Encode for Description {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        put_string_u16(out, &self.family)?;
        put_u16(
            out,
            u16::try_from(self.faces.len()).map_err(|_| Error::LengthOverflow)?,
        );
        for face in &self.faces {
            face.encode_record(out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Description {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let family = decoder.string_u16()?;
        let face_count = decoder.u16()?;
        let mut faces = Vec::with_capacity(usize::from(face_count));
        for _ in 0..face_count {
            faces.push(FaceRecord::decode_record(&mut decoder)?);
        }
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        Ok(Self {
            family,
            faces,
            extensions,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetch {
    pub face_handle: u64,
    pub expected_content_hash: [u8; 32],
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Fetch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.face_handle == 0 {
            return Err(Error::Invalid("zero Font face handle"));
        }
        put_u64(out, self.face_handle);
        out.extend_from_slice(&self.expected_content_hash);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Fetch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            face_handle: decoder.u64()?,
            expected_content_hash: decoder.array_32()?,
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        if value.face_handle == 0 {
            return Err(Error::Invalid("zero Font face handle"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchResult {
    pub face_handle: u64,
    pub content_hash: [u8; 32],
    pub byte_len: u64,
    pub format: Format,
    pub transfer: Descriptor,
}

impl FetchResult {
    fn validate(&self) -> Result<()> {
        if self.face_handle == 0 {
            return Err(Error::Invalid("zero Font face handle"));
        }
        validate_font_transfer(&self.transfer, FACE_BYTES_CONTENT_KIND, self.byte_len)
    }
}

impl Encode for FetchResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.face_handle);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.byte_len);
        out.push(self.format as u8);
        out.extend_from_slice(&[0; 3]);
        self.transfer.encode_to(out)
    }
}

impl Decode for FetchResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let face_handle = decoder.u64()?;
        let content_hash = decoder.array_32()?;
        let byte_len = decoder.u64()?;
        let format = Format::try_from(decoder.u8()?)?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Font FETCH reserved bytes"));
        }
        let transfer = Descriptor::decode(decoder.rest())?;
        decoder.finish()?;
        let value = Self {
            face_handle,
            content_hash,
            byte_len,
            format,
            transfer,
        };
        value.validate()?;
        Ok(value)
    }
}

fn validate_font_transfer(transfer: &Descriptor, content_kind: u16, byte_len: u64) -> Result<()> {
    if transfer.mode != Mode::Byte
        || transfer.direction != Direction::SENDER_TO_RECEIVER
        || transfer.content_family != crate::family::FONT
        || transfer.content_kind != content_kind
        || transfer.content_version != VERSION
        || byte_len == 0
    {
        return Err(Error::Invalid("Font Transfer descriptor"));
    }
    transfer.validate()
}

/// Typed Font entries carried in a family descriptor's limit extensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_families: u32,
    pub max_faces_per_family: u32,
    pub max_description_bytes: u64,
    pub max_face_bytes: u64,
    pub max_concurrent_fetches: u32,
    pub max_scan_duration_ns: u64,
    pub refresh_interval_ns: u64,
}

impl Limits {
    pub const HARD: Self = Self {
        max_families: crate::schema::font::MAX_FAMILIES as u32,
        max_faces_per_family: crate::schema::font::MAX_FACES_PER_FAMILY as u32,
        max_description_bytes: crate::schema::font::MAX_DESCRIPTION_BYTES,
        max_face_bytes: crate::schema::font::MAX_FACE_BYTES,
        max_concurrent_fetches: crate::schema::font::MAX_CONCURRENT_FETCHES as u32,
        max_scan_duration_ns: crate::schema::font::MAX_SCAN_DURATION_NS,
        refresh_interval_ns: crate::schema::font::MAX_REFRESH_INTERVAL_NS,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid_u32 = |value: u32, maximum: u32| value != 0 && value <= maximum;
        if !valid_u32(self.max_families, hard.max_families)
            || !valid_u32(self.max_faces_per_family, hard.max_faces_per_family)
            || self.max_description_bytes == 0
            || self.max_description_bytes > hard.max_description_bytes
            || self.max_face_bytes == 0
            || self.max_face_bytes > hard.max_face_bytes
            || !valid_u32(self.max_concurrent_fetches, hard.max_concurrent_fetches)
            || self.max_scan_duration_ns == 0
            || self.max_scan_duration_ns > hard.max_scan_duration_ns
            || self.refresh_interval_ns > hard.refresh_interval_ns
        {
            return Err(Error::Invalid("Font family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(crate::schema::font::LIMIT_MAX_FAMILIES, self.max_families),
            limit_u32(
                crate::schema::font::LIMIT_MAX_FACES_PER_FAMILY,
                self.max_faces_per_family,
            ),
            limit_u64(
                crate::schema::font::LIMIT_MAX_DESCRIPTION_BYTES,
                self.max_description_bytes,
            ),
            limit_u64(
                crate::schema::font::LIMIT_MAX_FACE_BYTES,
                self.max_face_bytes,
            ),
            limit_u32(
                crate::schema::font::LIMIT_MAX_CONCURRENT_FETCHES,
                self.max_concurrent_fetches,
            ),
            limit_u64(
                crate::schema::font::LIMIT_MAX_SCAN_DURATION_NS,
                self.max_scan_duration_ns,
            ),
            limit_u64(
                crate::schema::font::LIMIT_REFRESH_INTERVAL_NS,
                self.refresh_interval_ns,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::font::LIMIT_MAX_FAMILIES as u16,
                crate::schema::font::LIMIT_MAX_FACES_PER_FAMILY as u16,
                crate::schema::font::LIMIT_MAX_DESCRIPTION_BYTES as u16,
                crate::schema::font::LIMIT_MAX_FACE_BYTES as u16,
                crate::schema::font::LIMIT_MAX_CONCURRENT_FETCHES as u16,
                crate::schema::font::LIMIT_MAX_SCAN_DURATION_NS as u16,
                crate::schema::font::LIMIT_REFRESH_INTERVAL_NS as u16,
            ],
            "unknown required Font family limit",
        )?;
        let value = Self {
            max_families: read_limit_u32(extensions, crate::schema::font::LIMIT_MAX_FAMILIES)?,
            max_faces_per_family: read_limit_u32(
                extensions,
                crate::schema::font::LIMIT_MAX_FACES_PER_FAMILY,
            )?,
            max_description_bytes: read_limit_u64(
                extensions,
                crate::schema::font::LIMIT_MAX_DESCRIPTION_BYTES,
            )?,
            max_face_bytes: read_limit_u64(extensions, crate::schema::font::LIMIT_MAX_FACE_BYTES)?,
            max_concurrent_fetches: read_limit_u32(
                extensions,
                crate::schema::font::LIMIT_MAX_CONCURRENT_FETCHES,
            )?,
            max_scan_duration_ns: read_limit_u64(
                extensions,
                crate::schema::font::LIMIT_MAX_SCAN_DURATION_NS,
            )?,
            refresh_interval_ns: read_limit_u64(
                extensions,
                crate::schema::font::LIMIT_REFRESH_INTERVAL_NS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Extension;

    fn face() -> FaceRecord {
        FaceRecord {
            face_handle: 1,
            content_hash: [2; 32],
            byte_len: 3,
            format: Format::SfntTrueType,
            style: Style::Normal,
            flags: FACE_FETCHABLE,
            weight_min: 400,
            weight_default: 400,
            weight_max: 400,
            stretch_min: 1000,
            stretch_default: 1000,
            stretch_max: 1000,
            slant_tenths_degrees: 0,
            units_per_em: 1000,
            cell_advance: 600,
            ascent: 800,
            descent: -200,
            line_gap: 0,
            subfamily: "Regular".into(),
            postscript: "Demo-Regular".into(),
            extensions: Extensions::default(),
        }
    }

    #[test]
    fn description_round_trip_and_truncation() {
        let value = Description {
            family: "Demo".into(),
            faces: vec![face()],
            extensions: Extensions::default(),
        };
        let bytes = value.encode().unwrap();
        assert_eq!(Description::decode(&bytes).unwrap(), value);
        for end in 0..bytes.len() {
            assert!(Description::decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn family_record_golden() {
        let value = FamilyRecord {
            font_handle: 1,
            generation: 2,
            flags: FAMILY_MONOSPACE | FAMILY_FETCHABLE,
            face_count: 1,
            family: "Demo".into(),
            display: "Demo Font".into(),
            extensions: Extensions::default(),
        };
        let bytes = value.encode().unwrap();
        assert_eq!(
            &bytes[..20],
            &[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 9, 0, 1, 0]
        );
        assert_eq!(FamilyRecord::decode(&bytes).unwrap(), value);
    }

    #[test]
    fn family_limits_are_hard_bounded_and_typed() {
        let limits = Limits::HARD;
        let extensions = limits.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), limits);

        let mut invalid = limits;
        invalid.max_face_bytes += 1;
        assert_eq!(invalid.validate(), Err(Error::Invalid("Font family limit")));
        let mut immutable = limits;
        immutable.refresh_interval_ns = 0;
        immutable.validate().unwrap();

        let mut unknown = extensions;
        unknown.0.push(Extension {
            tag: 99,
            required: true,
            value: Vec::new(),
        });
        assert_eq!(
            Limits::from_extensions(&unknown),
            Err(Error::Invalid("unknown required Font family limit"))
        );
    }
}
