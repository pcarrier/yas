use core::fmt;

use crate::prelude::*;

/// A YAS wire-codec error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Truncated,
    TrailingBytes(usize),
    Invalid(&'static str),
    InvalidUtf8,
    LengthOverflow,
    LimitExceeded {
        limit: &'static str,
        actual: u64,
        maximum: u64,
    },
    UnsupportedCodec(u16),
    Compression,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated YAS value"),
            Self::TrailingBytes(n) => write!(f, "{n} trailing YAS bytes"),
            Self::Invalid(what) => write!(f, "invalid YAS {what}"),
            Self::InvalidUtf8 => f.write_str("invalid YAS UTF-8"),
            Self::LengthOverflow => f.write_str("YAS length does not fit its wire field"),
            Self::LimitExceeded {
                limit,
                actual,
                maximum,
            } => write!(f, "YAS {limit} limit exceeded: {actual} > {maximum}"),
            Self::UnsupportedCodec(codec) => write!(f, "unsupported YAS codec {codec}"),
            Self::Compression => f.write_str("invalid YAS compressed payload"),
        }
    }
}

impl core::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

/// Encode one complete operation-specific value, without a frame header.
pub trait Encode {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()>;

    fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.encode_to(&mut out)?;
        Ok(out)
    }
}

/// Decode one complete operation-specific value, rejecting trailing bytes.
pub trait Decode: Sized {
    fn decode(input: &[u8]) -> Result<Self>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extension {
    pub tag: u16,
    pub required: bool,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extensions(pub Vec<Extension>);

impl Extensions {
    pub fn validate(&self) -> Result<()> {
        if self.0.len() > crate::schema::transport::HARD_MAX_EXTENSION_ENTRIES {
            return Err(Error::LimitExceeded {
                limit: "extension entries",
                actual: self.0.len() as u64,
                maximum: crate::schema::transport::HARD_MAX_EXTENSION_ENTRIES as u64,
            });
        }
        let mut previous = None;
        for extension in &self.0 {
            if previous.is_some_and(|tag| tag >= extension.tag) {
                return Err(Error::Invalid("extension tag order"));
            }
            previous = Some(extension.tag);
        }
        Ok(())
    }

    /// Encode the extension entries, without the enclosing `extensions_len`.
    pub fn encode_entries(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        for extension in &self.0 {
            put_u16(out, extension.tag);
            put_u16(out, u16::from(extension.required));
            put_len_u32(out, extension.value.len())?;
            out.extend_from_slice(&extension.value);
        }
        Ok(())
    }

    /// Encode an extension tail (`extensions_len` followed by its entries).
    pub fn encode_tail(&self, out: &mut Vec<u8>) -> Result<()> {
        let mut entries = Vec::new();
        self.encode_entries(&mut entries)?;
        put_len_u32(out, entries.len())?;
        out.extend_from_slice(&entries);
        Ok(())
    }

    pub fn decode_entries(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let mut entries = Vec::new();
        let mut previous = None;
        while !decoder.is_empty() {
            if entries.len() == crate::schema::transport::HARD_MAX_EXTENSION_ENTRIES {
                return Err(Error::LimitExceeded {
                    limit: "extension entries",
                    actual: entries.len() as u64 + 1,
                    maximum: crate::schema::transport::HARD_MAX_EXTENSION_ENTRIES as u64,
                });
            }
            let tag = decoder.u16()?;
            if previous.is_some_and(|old| old >= tag) {
                return Err(Error::Invalid("extension tag order"));
            }
            previous = Some(tag);
            let flags = decoder.u16()?;
            if flags & !1 != 0 {
                return Err(Error::Invalid("extension flags"));
            }
            let value = decoder.len_bytes_u32()?.to_vec();
            entries.push(Extension {
                tag,
                required: flags & 1 != 0,
                value,
            });
        }
        Ok(Self(entries))
    }
}

impl Encode for Extensions {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_tail(out)
    }
}

impl Decode for Extensions {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let entries = decoder.len_bytes_u32()?;
        decoder.finish()?;
        Self::decode_entries(entries)
    }
}

pub(crate) fn limit_u32(tag: u64, value: u32) -> Extension {
    Extension {
        tag: tag as u16,
        required: false,
        value: value.to_le_bytes().to_vec(),
    }
}

pub(crate) fn limit_u64(tag: u64, value: u64) -> Extension {
    Extension {
        tag: tag as u16,
        required: false,
        value: value.to_le_bytes().to_vec(),
    }
}

pub(crate) fn read_limit_u32(extensions: &Extensions, tag: u64) -> Result<u32> {
    let extension = extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .ok_or(Error::Invalid("missing family limit"))?;
    Ok(u32::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("family limit length"))?,
    ))
}

pub(crate) fn read_limit_u64(extensions: &Extensions, tag: u64) -> Result<u64> {
    let extension = extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .ok_or(Error::Invalid("missing family limit"))?;
    Ok(u64::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("family limit length"))?,
    ))
}

pub(crate) fn reject_unknown_required_extensions(
    extensions: &Extensions,
    known: &[u16],
    context: &'static str,
) -> Result<()> {
    extensions.validate()?;
    if extensions
        .0
        .iter()
        .any(|extension| extension.required && !known.contains(&extension.tag))
    {
        return Err(Error::Invalid(context));
    }
    Ok(())
}

pub(crate) struct Decoder<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.input.len() - self.offset
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.offset.checked_add(len).ok_or(Error::LengthOverflow)?;
        let value = self.input.get(self.offset..end).ok_or(Error::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    pub(crate) fn rest(&mut self) -> &'a [u8] {
        let value = &self.input[self.offset..];
        self.offset = self.input.len();
        value
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(crate) fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(crate) fn array_16(&mut self) -> Result<[u8; 16]> {
        Ok(self.take(16)?.try_into().unwrap())
    }

    pub(crate) fn array_32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().unwrap())
    }

    pub(crate) fn len_bytes_u16(&mut self) -> Result<&'a [u8]> {
        let len = usize::from(self.u16()?);
        self.take(len)
    }

    pub(crate) fn len_bytes_u32(&mut self) -> Result<&'a [u8]> {
        let len = usize::try_from(self.u32()?).map_err(|_| Error::LengthOverflow)?;
        self.take(len)
    }

    pub(crate) fn string_u16(&mut self) -> Result<String> {
        let bytes = self.len_bytes_u16()?;
        core::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| Error::InvalidUtf8)
    }

    pub(crate) fn string_u32(&mut self) -> Result<String> {
        let bytes = self.len_bytes_u32()?;
        core::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| Error::InvalidUtf8)
    }

    pub(crate) fn extensions(&mut self) -> Result<Extensions> {
        Extensions::decode_entries(self.len_bytes_u32()?)
    }

    pub(crate) fn finish(self) -> Result<()> {
        if self.offset == self.input.len() {
            Ok(())
        } else {
            Err(Error::TrailingBytes(self.remaining()))
        }
    }
}

pub(crate) fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_len_u16(out: &mut Vec<u8>, len: usize) -> Result<()> {
    put_u16(out, u16::try_from(len).map_err(|_| Error::LengthOverflow)?);
    Ok(())
}

pub(crate) fn put_len_u32(out: &mut Vec<u8>, len: usize) -> Result<()> {
    put_u32(out, u32::try_from(len).map_err(|_| Error::LengthOverflow)?);
    Ok(())
}

pub(crate) fn put_bytes_u16(out: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    put_len_u16(out, value.len())?;
    out.extend_from_slice(value);
    Ok(())
}

pub(crate) fn put_bytes_u32(out: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    put_len_u32(out, value.len())?;
    out.extend_from_slice(value);
    Ok(())
}

pub(crate) fn put_string_u16(out: &mut Vec<u8>, value: &str) -> Result<()> {
    put_bytes_u16(out, value.as_bytes())
}

pub(crate) fn put_string_u32(out: &mut Vec<u8>, value: &str) -> Result<()> {
    put_bytes_u32(out, value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_reject_duplicate_and_reserved_flags() {
        let duplicate = Extensions(vec![
            Extension {
                tag: 1,
                required: false,
                value: vec![],
            },
            Extension {
                tag: 1,
                required: false,
                value: vec![],
            },
        ]);
        assert_eq!(
            duplicate.encode(),
            Err(Error::Invalid("extension tag order"))
        );

        assert_eq!(
            Extensions::decode(&[8, 0, 0, 0, 1, 0, 2, 0, 0, 0, 0, 0]),
            Err(Error::Invalid("extension flags"))
        );
    }

    #[test]
    fn extension_entry_limit_bounds_decoded_allocations() {
        let maximum = crate::schema::transport::HARD_MAX_EXTENSION_ENTRIES;
        let entries = (0..maximum)
            .map(|index| Extension {
                tag: u16::try_from(index + 1).unwrap(),
                required: false,
                value: vec![],
            })
            .collect();
        let at_limit = Extensions(entries);
        assert_eq!(
            Extensions::decode(&at_limit.encode().unwrap()).unwrap(),
            at_limit
        );

        let mut too_many = Vec::new();
        put_u32(&mut too_many, ((maximum + 1) * 8) as u32);
        for tag in 1..=maximum + 1 {
            put_u16(&mut too_many, tag as u16);
            put_u16(&mut too_many, 0);
            put_u32(&mut too_many, 0);
        }
        assert!(matches!(
            Extensions::decode(&too_many),
            Err(Error::LimitExceeded {
                limit: "extension entries",
                ..
            })
        ));
    }
}
