use crate::prelude::*;

use lz4_flex::block::{compress, decompress};

use crate::codec::{Decode, Decoder, Error, Result, put_len_u32, put_u16, put_u32};
use crate::family;

pub const PREFACE: [u8; 8] = crate::schema::transport::PREFACE;

pub const PRE_HELLO_MAX_FRAME: u32 = crate::schema::transport::PRE_HELLO_MAX_FRAME;
pub const HARD_MAX_WIRE_FRAME: u32 = crate::schema::transport::HARD_MAX_WIRE_FRAME;
pub const HARD_MAX_DECODED_FRAME: u32 = crate::schema::transport::HARD_MAX_DECODED_FRAME;
pub const HARD_MAX_DATAGRAM: u32 = crate::schema::transport::HARD_MAX_DATAGRAM;
pub const HARD_MAX_BUFFERED: u64 = crate::schema::transport::HARD_MAX_BUFFERED;
pub const HARD_MAX_BULK_CHUNK: u32 = crate::schema::transport::HARD_MAX_BULK_CHUNK;

const META_CLASS_MASK: u8 = crate::schema::transport::CLASS_MASK;
const META_COMPRESSED: u8 = crate::schema::transport::META_COMPRESSED;
const META_SENSITIVE: u8 = crate::schema::transport::META_SENSITIVE;
const META_RESERVED: u8 = crate::schema::transport::META_RESERVED;
pub const LZ4_CODEC: u16 = crate::schema::transport::codec::LZ4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Class {
    Event = crate::schema::transport::class::EVENT,
    Request = crate::schema::transport::class::REQUEST,
    Result = crate::schema::transport::class::RESULT,
}

impl Class {
    fn from_meta(meta: u8) -> Result<Self> {
        match meta & META_CLASS_MASK {
            crate::schema::transport::class::EVENT => Ok(Self::Event),
            crate::schema::transport::class::REQUEST => Ok(Self::Request),
            crate::schema::transport::class::RESULT => Ok(Self::Result),
            _ => Err(Error::Invalid("frame class")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub family: u16,
    pub kind: u16,
    pub class: Class,
    pub request_id: Option<u32>,
    pub compressed: bool,
    pub sensitive: bool,
}

impl FrameHeader {
    pub const fn event(family: u16, kind: u16) -> Self {
        Self {
            family,
            kind,
            class: Class::Event,
            request_id: None,
            compressed: false,
            sensitive: false,
        }
    }

    pub const fn request(family: u16, kind: u16, request_id: u32) -> Self {
        Self {
            family,
            kind,
            class: Class::Request,
            request_id: Some(request_id),
            compressed: false,
            sensitive: false,
        }
    }

    pub const fn result(family: u16, kind: u16, request_id: u32) -> Self {
        Self {
            family,
            kind,
            class: Class::Result,
            request_id: Some(request_id),
            compressed: false,
            sensitive: false,
        }
    }

    pub const fn encoded_len(&self) -> usize {
        match self.class {
            Class::Event => crate::schema::transport::EVENT_HEADER_BYTES,
            Class::Request | Class::Result => crate::schema::transport::CORRELATED_HEADER_BYTES,
        }
    }

    fn validate(&self) -> Result<()> {
        match (self.class, self.request_id) {
            (Class::Event, None) => Ok(()),
            (Class::Request | Class::Result, Some(request_id)) if request_id != 0 => Ok(()),
            _ => Err(Error::Invalid("request ID presence")),
        }
    }

    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.family);
        put_u16(out, self.kind);
        let mut meta = self.class as u8;
        if self.compressed {
            meta |= META_COMPRESSED;
        }
        if self.sensitive {
            meta |= META_SENSITIVE;
        }
        out.push(meta);
        if let Some(request_id) = self.request_id {
            put_u32(out, request_id);
        }
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let family = decoder.u16()?;
        let kind = decoder.u16()?;
        let meta = decoder.u8()?;
        if meta & META_RESERVED != 0 {
            return Err(Error::Invalid("frame meta reserved bits"));
        }
        let class = Class::from_meta(meta)?;
        let request_id = match class {
            Class::Event => None,
            Class::Request | Class::Result => Some(decoder.u32()?),
        };
        Ok(Self {
            family,
            kind,
            class,
            request_id,
            compressed: meta & META_COMPRESSED != 0,
            sensitive: meta & META_SENSITIVE != 0,
        })
    }
}

/// A decoded YAS frame. `payload` is always decompressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub family: u16,
    pub kind: u16,
    pub payload: Vec<u8>,
    pub compressed: bool,
    pub sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub family: u16,
    pub kind: u16,
    pub request_id: u32,
    pub payload: Vec<u8>,
    pub compressed: bool,
    pub sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultEnvelope {
    pub family: u16,
    pub kind: u16,
    pub request_id: u32,
    pub payload: Vec<u8>,
    pub compressed: bool,
    pub sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Envelope {
    Event(Event),
    Request(Request),
    Result(ResultEnvelope),
}

impl From<Envelope> for Frame {
    fn from(envelope: Envelope) -> Self {
        match envelope {
            Envelope::Event(value) => Self {
                header: FrameHeader {
                    family: value.family,
                    kind: value.kind,
                    class: Class::Event,
                    request_id: None,
                    compressed: value.compressed,
                    sensitive: value.sensitive,
                },
                payload: value.payload,
            },
            Envelope::Request(value) => Self {
                header: FrameHeader {
                    family: value.family,
                    kind: value.kind,
                    class: Class::Request,
                    request_id: Some(value.request_id),
                    compressed: value.compressed,
                    sensitive: value.sensitive,
                },
                payload: value.payload,
            },
            Envelope::Result(value) => Self {
                header: FrameHeader {
                    family: value.family,
                    kind: value.kind,
                    class: Class::Result,
                    request_id: Some(value.request_id),
                    compressed: value.compressed,
                    sensitive: value.sensitive,
                },
                payload: value.payload,
            },
        }
    }
}

impl From<Frame> for Envelope {
    fn from(frame: Frame) -> Self {
        match frame.header.class {
            Class::Event => Self::Event(Event {
                family: frame.header.family,
                kind: frame.header.kind,
                payload: frame.payload,
                compressed: frame.header.compressed,
                sensitive: frame.header.sensitive,
            }),
            Class::Request => Self::Request(Request {
                family: frame.header.family,
                kind: frame.header.kind,
                request_id: frame.header.request_id.unwrap(),
                payload: frame.payload,
                compressed: frame.header.compressed,
                sensitive: frame.header.sensitive,
            }),
            Class::Result => Self::Result(ResultEnvelope {
                family: frame.header.family,
                kind: frame.header.kind,
                request_id: frame.header.request_id.unwrap(),
                payload: frame.payload,
                compressed: frame.header.compressed,
                sensitive: frame.header.sensitive,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLimits {
    pub max_wire_frame: u32,
    pub max_decoded_frame: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatagramContext {
    NetNativeFlow,
    SurfaceFrame,
    MediaFrame,
}

impl FrameLimits {
    pub const fn pre_hello() -> Self {
        Self {
            max_wire_frame: PRE_HELLO_MAX_FRAME,
            max_decoded_frame: PRE_HELLO_MAX_FRAME,
        }
    }

    pub const fn recommended() -> Self {
        Self {
            max_wire_frame: crate::schema::transport::RECOMMENDED_WIRE_FRAME,
            max_decoded_frame: crate::schema::transport::RECOMMENDED_DECODED_FRAME,
        }
    }

    pub fn validate(self) -> Result<Self> {
        if self.max_wire_frame < crate::schema::transport::CORRELATED_HEADER_BYTES as u32 {
            return Err(Error::Invalid("wire frame limit"));
        }
        if self.max_wire_frame > HARD_MAX_WIRE_FRAME {
            return Err(Error::LimitExceeded {
                limit: "wire frame",
                actual: u64::from(self.max_wire_frame),
                maximum: u64::from(HARD_MAX_WIRE_FRAME),
            });
        }
        if self.max_decoded_frame < self.max_wire_frame {
            return Err(Error::Invalid("decoded frame limit"));
        }
        if self.max_decoded_frame > HARD_MAX_DECODED_FRAME {
            return Err(Error::LimitExceeded {
                limit: "decoded frame",
                actual: u64::from(self.max_decoded_frame),
                maximum: u64::from(HARD_MAX_DECODED_FRAME),
            });
        }
        Ok(self)
    }
}

/// Stateless frame encoder/decoder for one negotiated direction.
#[derive(Clone, Debug)]
pub struct FrameCodec {
    limits: FrameLimits,
    codecs: BTreeSet<u16>,
    compression_required: BTreeSet<(u16, Class, u16)>,
    compression_forbidden: BTreeSet<(u16, Class, u16)>,
    sensitive_required: BTreeSet<(u16, Class, u16)>,
    sensitive_forbidden: BTreeSet<(u16, Class, u16)>,
    hello_only: bool,
}

impl FrameCodec {
    /// Codec for the one pre-negotiation exchange. It accepts only an
    /// uncompressed Core HELLO Request or Result; the session layer enforces
    /// endpoint direction, exactly one Request, and matching correlation.
    pub fn pre_hello() -> Self {
        let mut codec = Self::new(FrameLimits::pre_hello(), []).expect("constant limits are valid");
        codec.hello_only = true;
        codec
    }

    pub fn new(
        limits: FrameLimits,
        negotiated_codecs: impl IntoIterator<Item = u16>,
    ) -> Result<Self> {
        let limits = limits.validate()?;
        let mut codecs = BTreeSet::new();
        let mut previous_codec = None;
        for codec in negotiated_codecs {
            if codec == 0 || previous_codec.is_some_and(|previous| previous >= codec) {
                return Err(Error::Invalid("negotiated codec order"));
            }
            if codec != LZ4_CODEC {
                return Err(Error::UnsupportedCodec(codec));
            }
            codecs.insert(codec);
            previous_codec = Some(codec);
        }
        let mut compression_required = BTreeSet::new();
        let mut compression_forbidden = BTreeSet::new();
        let mut sensitive_required = BTreeSet::new();
        let mut sensitive_forbidden = BTreeSet::new();
        for family in crate::schema::FAMILIES {
            for operation in family.operations {
                let class = match operation.class {
                    value if value == crate::schema::transport::class::EVENT => Class::Event,
                    value if value == crate::schema::transport::class::REQUEST => Class::Request,
                    _ => return Err(Error::Invalid("schema operation class")),
                };
                let keys = [
                    (family.id, class, operation.kind),
                    (family.id, Class::Result, operation.kind),
                ];
                // A Request policy also covers its correlated Result. This is
                // conservative for requests that return secret data.
                let keys = &keys[..if class == Class::Request { 2 } else { 1 }];
                match operation.compression {
                    value if value == crate::schema::transport::policy::REQUIRED => {
                        compression_required.extend(keys.iter().copied());
                    }
                    value if value == crate::schema::transport::policy::FORBIDDEN => {
                        compression_forbidden.extend(keys.iter().copied());
                    }
                    _ => {}
                }
                match operation.sensitive {
                    value if value == crate::schema::transport::policy::REQUIRED => {
                        sensitive_required.extend(keys.iter().copied());
                    }
                    value if value == crate::schema::transport::policy::FORBIDDEN => {
                        sensitive_forbidden.extend(keys.iter().copied());
                    }
                    _ => {}
                }
            }
        }
        Ok(Self {
            limits,
            codecs,
            compression_required,
            compression_forbidden,
            sensitive_required,
            sensitive_forbidden,
            hello_only: false,
        })
    }

    pub const fn limits(&self) -> FrameLimits {
        self.limits
    }

    /// Mark a schema kind as ineligible for generic frame compression.
    pub fn forbid_compression(&mut self, family: u16, class: Class, kind: u16) {
        self.compression_forbidden.insert((family, class, kind));
    }

    pub fn require_compression(&mut self, family: u16, class: Class, kind: u16) {
        self.compression_required.insert((family, class, kind));
    }

    /// Mark a schema kind as requiring the SENSITIVE diagnostic flag.
    pub fn require_sensitive(&mut self, family: u16, class: Class, kind: u16) {
        self.sensitive_required.insert((family, class, kind));
    }

    pub fn forbid_sensitive(&mut self, family: u16, class: Class, kind: u16) {
        self.sensitive_forbidden.insert((family, class, kind));
    }

    pub fn encode(&self, frame: &Frame) -> Result<Vec<u8>> {
        self.validate_schema_flags(&frame.header)?;
        frame.header.validate()?;
        let header_len = frame.header.encoded_len();
        self.check_decoded_len(header_len, frame.payload.len())?;

        let mut out = Vec::new();
        frame.header.encode_to(&mut out)?;
        if frame.header.compressed {
            if self.compression_forbidden.contains(&(
                frame.header.family,
                frame.header.class,
                frame.header.kind,
            )) {
                return Err(Error::Invalid("compression-forbidden frame"));
            }
            if !self.codecs.contains(&LZ4_CODEC) {
                return Err(Error::UnsupportedCodec(LZ4_CODEC));
            }
            put_u16(&mut out, LZ4_CODEC);
            put_u16(&mut out, 0);
            put_len_u32(&mut out, frame.payload.len())?;
            out.extend_from_slice(&compress(&frame.payload));
        } else {
            out.extend_from_slice(&frame.payload);
        }
        self.check_wire_len(out.len())?;
        Ok(out)
    }

    pub fn decode(&self, input: &[u8]) -> Result<Frame> {
        self.check_wire_len(input.len())?;
        let mut decoder = Decoder::new(input);
        let header = FrameHeader::decode_from(&mut decoder)?;
        header.validate()?;
        self.validate_schema_flags(&header)?;
        let header_len = header.encoded_len();
        let payload = if header.compressed {
            if self
                .compression_forbidden
                .contains(&(header.family, header.class, header.kind))
            {
                return Err(Error::Invalid("compression-forbidden frame"));
            }
            let codec = decoder.u16()?;
            let reserved = decoder.u16()?;
            if reserved != 0 {
                return Err(Error::Invalid("compressed payload reserved field"));
            }
            let decoded_len = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
            self.check_decoded_len(header_len, decoded_len)?;
            if !self.codecs.contains(&codec) {
                return Err(Error::UnsupportedCodec(codec));
            }
            match codec {
                LZ4_CODEC => {
                    decompress(decoder.rest(), decoded_len).map_err(|_| Error::Compression)?
                }
                _ => return Err(Error::UnsupportedCodec(codec)),
            }
        } else {
            let payload = decoder.rest().to_vec();
            self.check_decoded_len(header_len, payload.len())?;
            payload
        };
        decoder.finish()?;
        Ok(Frame { header, payload })
    }

    /// Encode one frame for a byte-stream link, including its `u32` length.
    pub fn encode_stream(&self, frame: &Frame) -> Result<Vec<u8>> {
        let encoded = self.encode(frame)?;
        let mut out = Vec::with_capacity(4 + encoded.len());
        put_len_u32(&mut out, encoded.len())?;
        out.extend_from_slice(&encoded);
        Ok(out)
    }

    /// Decode the first byte-stream frame and return it with bytes consumed.
    pub fn decode_stream(&self, input: &[u8]) -> Result<(Frame, usize)> {
        if input.len() < 4 {
            return Err(Error::Truncated);
        }
        let len = usize::try_from(u32::from_le_bytes(input[..4].try_into().unwrap()))
            .map_err(|_| Error::LengthOverflow)?;
        self.check_wire_len(len)?;
        let end = 4usize.checked_add(len).ok_or(Error::LengthOverflow)?;
        let bytes = input.get(4..end).ok_or(Error::Truncated)?;
        Ok((self.decode(bytes)?, end))
    }

    /// Encode one transport datagram after applying the generated operation
    /// predicate. Net additionally requires the selected flow context.
    pub fn encode_datagram(
        &self,
        frame: &Frame,
        receive_max_datagram: u32,
        context: DatagramContext,
    ) -> Result<Vec<u8>> {
        validate_datagram_header(&frame.header, receive_max_datagram)?;
        self.validate_datagram_predicate(frame, context)?;
        let encoded = self.encode(frame)?;
        check_datagram_len(encoded.len(), receive_max_datagram)?;
        Ok(encoded)
    }

    /// Decode one complete transport datagram. Malformed datagrams are
    /// returned as errors for the caller to drop and count without closing the
    /// reliable session.
    pub fn decode_datagram(
        &self,
        input: &[u8],
        receive_max_datagram: u32,
        context: DatagramContext,
    ) -> Result<Frame> {
        check_datagram_len(input.len(), receive_max_datagram)?;
        let frame = self.decode(input)?;
        validate_datagram_header(&frame.header, receive_max_datagram)?;
        self.validate_datagram_predicate(&frame, context)?;
        Ok(frame)
    }

    pub fn validate_datagram_predicate(
        &self,
        frame: &Frame,
        context: DatagramContext,
    ) -> Result<()> {
        let Some(operation) = crate::schema::FAMILIES
            .iter()
            .find(|family| family.id == frame.header.family)
            .and_then(|family| {
                family.operations.iter().find(|operation| {
                    operation.class == frame.header.class as u8
                        && operation.kind == frame.header.kind
                })
            })
        else {
            return Err(Error::Invalid("transport datagram predicate"));
        };
        let eligible = match operation.datagram {
            value if value == crate::schema::transport::datagram_predicate::NET_NATIVE_FLOW => {
                context == DatagramContext::NetNativeFlow
                    && crate::net::Datagram::decode(&frame.payload).is_ok()
            }
            value if value == crate::schema::transport::datagram_predicate::SURFACE_FRAME => {
                context == DatagramContext::SurfaceFrame
                    && crate::surface::SurfaceFrame::decode(&frame.payload)
                        .is_ok_and(|value| value.datagram_eligible())
            }
            value if value == crate::schema::transport::datagram_predicate::MEDIA_FRAME => {
                context == DatagramContext::MediaFrame
                    && crate::media::MediaFrame::decode(&frame.payload)
                        .is_ok_and(|value| value.datagram_eligible())
            }
            value if value == crate::schema::transport::datagram_predicate::FORBIDDEN => false,
            _ => return Err(Error::Invalid("unknown transport datagram predicate")),
        };
        if !eligible {
            return Err(Error::Invalid("transport datagram predicate"));
        }
        Ok(())
    }

    fn validate_schema_flags(&self, header: &FrameHeader) -> Result<()> {
        if self.hello_only
            && (header.family != family::CORE
                || header.kind != crate::core::request_kind::HELLO
                || !matches!(header.class, Class::Request | Class::Result)
                || header.compressed)
        {
            return Err(Error::Invalid("pre-HELLO frame"));
        }
        let key = (header.family, header.class, header.kind);
        if !header.compressed && self.compression_required.contains(&key) {
            return Err(Error::Invalid("missing required compression"));
        }
        if header.compressed && self.compression_forbidden.contains(&key) {
            return Err(Error::Invalid("compression-forbidden frame"));
        }
        if !header.sensitive && self.sensitive_required.contains(&key) {
            return Err(Error::Invalid("missing SENSITIVE flag"));
        }
        if header.sensitive && self.sensitive_forbidden.contains(&key) {
            return Err(Error::Invalid("forbidden SENSITIVE flag"));
        }
        Ok(())
    }

    fn check_wire_len(&self, len: usize) -> Result<()> {
        if len > self.limits.max_wire_frame as usize {
            return Err(Error::LimitExceeded {
                limit: "wire frame",
                actual: len as u64,
                maximum: u64::from(self.limits.max_wire_frame),
            });
        }
        if len < crate::schema::transport::EVENT_HEADER_BYTES {
            return Err(if len == 0 {
                Error::Invalid("empty frame")
            } else {
                Error::Truncated
            });
        }
        Ok(())
    }

    fn check_decoded_len(&self, header_len: usize, payload_len: usize) -> Result<()> {
        let decoded = header_len
            .checked_add(payload_len)
            .ok_or(Error::LengthOverflow)?;
        if decoded > self.limits.max_decoded_frame as usize {
            return Err(Error::LimitExceeded {
                limit: "decoded frame",
                actual: decoded as u64,
                maximum: u64::from(self.limits.max_decoded_frame),
            });
        }
        Ok(())
    }
}

fn validate_datagram_header(header: &FrameHeader, receive_max_datagram: u32) -> Result<()> {
    if receive_max_datagram == 0
        || receive_max_datagram > HARD_MAX_DATAGRAM
        || header.class != Class::Event
        || header.compressed
        || header.family == family::CORE
        || header.family == family::TRANSFER
    {
        return Err(Error::Invalid("transport datagram frame"));
    }
    Ok(())
}

fn check_datagram_len(len: usize, receive_max_datagram: u32) -> Result<()> {
    let maximum = receive_max_datagram.min(HARD_MAX_DATAGRAM);
    if maximum == 0 || len > maximum as usize {
        return Err(Error::LimitExceeded {
            limit: "transport datagram",
            actual: len as u64,
            maximum: u64::from(maximum),
        });
    }
    Ok(())
}

pub fn validate_preface(input: &[u8]) -> Result<()> {
    if input.len() < PREFACE.len() {
        return Err(Error::Truncated);
    }
    if input.len() != PREFACE.len() || input != PREFACE {
        return Err(Error::Invalid("preface"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Encode;

    #[test]
    fn preface_golden() {
        assert_eq!(PREFACE, [0x59, 0x41, 0x53, 0x00, 0x01, 0x00, 0x0d, 0x0a]);
        assert_eq!(validate_preface(&PREFACE), Ok(()));
        assert_eq!(validate_preface(&PREFACE[..7]), Err(Error::Truncated));
        let mut wrong = PREFACE;
        wrong[4] = 2;
        assert_eq!(validate_preface(&wrong), Err(Error::Invalid("preface")));
        let mut extra = PREFACE.to_vec();
        extra.push(0);
        assert_eq!(validate_preface(&extra), Err(Error::Invalid("preface")));
    }

    #[test]
    fn frame_limits_cover_event_headers_and_decoded_wire_bytes() {
        assert_eq!(
            FrameLimits {
                max_wire_frame: crate::schema::transport::CORRELATED_HEADER_BYTES as u32,
                max_decoded_frame: crate::schema::transport::CORRELATED_HEADER_BYTES as u32,
            }
            .validate(),
            Ok(FrameLimits {
                max_wire_frame: crate::schema::transport::CORRELATED_HEADER_BYTES as u32,
                max_decoded_frame: crate::schema::transport::CORRELATED_HEADER_BYTES as u32,
            })
        );
        assert_eq!(
            FrameLimits {
                max_wire_frame: crate::schema::transport::CORRELATED_HEADER_BYTES as u32 - 1,
                max_decoded_frame: 1024,
            }
            .validate(),
            Err(Error::Invalid("wire frame limit"))
        );
        assert_eq!(
            FrameLimits {
                max_wire_frame: 1024,
                max_decoded_frame: 1023,
            }
            .validate(),
            Err(Error::Invalid("decoded frame limit"))
        );
    }

    #[test]
    fn request_and_event_header_golden() {
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let request = Frame {
            header: FrameHeader::request(0x1234, 0x5678, 0x9abcdef0),
            payload: vec![1, 2],
        };
        assert_eq!(
            codec.encode(&request).unwrap(),
            [0x34, 0x12, 0x78, 0x56, 1, 0xf0, 0xde, 0xbc, 0x9a, 1, 2]
        );

        let event = Frame {
            header: FrameHeader::event(1, 2),
            payload: vec![3],
        };
        assert_eq!(codec.encode(&event).unwrap(), [1, 0, 2, 0, 0, 3]);
    }

    #[test]
    fn compression_round_trip_and_validation() {
        let codec = FrameCodec::new(FrameLimits::recommended(), [LZ4_CODEC]).unwrap();
        let mut header = FrameHeader::request(family::CORE, 1, 7);
        header.compressed = true;
        let frame = Frame {
            header,
            payload: vec![42; 4096],
        };
        let encoded = codec.encode(&frame).unwrap();
        assert_eq!(codec.decode(&encoded).unwrap(), frame);

        let mut reserved = encoded.clone();
        reserved[11] = 1;
        assert_eq!(
            codec.decode(&reserved),
            Err(Error::Invalid("compressed payload reserved field"))
        );

        let without_codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        assert_eq!(
            without_codec.decode(&encoded),
            Err(Error::UnsupportedCodec(1))
        );
        assert!(matches!(
            FrameCodec::new(FrameLimits::recommended(), [2]),
            Err(Error::UnsupportedCodec(2))
        ));
        assert!(matches!(
            FrameCodec::new(FrameLimits::recommended(), [LZ4_CODEC, LZ4_CODEC]),
            Err(Error::Invalid("negotiated codec order"))
        ));
    }

    #[test]
    fn transfer_data_cannot_use_generic_compression() {
        let codec = FrameCodec::new(FrameLimits::recommended(), [LZ4_CODEC]).unwrap();
        let mut header = FrameHeader::event(family::TRANSFER, crate::transfer::kind::BYTE_DATA);
        header.compressed = true;
        assert_eq!(
            codec.encode(&Frame {
                header,
                payload: vec![]
            }),
            Err(Error::Invalid("compression-forbidden frame"))
        );
    }

    #[test]
    fn generated_schema_policies_apply_on_encode_and_decode() {
        let codec = FrameCodec::new(FrameLimits::recommended(), [LZ4_CODEC]).unwrap();
        let create = Frame {
            header: FrameHeader::request(
                family::TERMINAL,
                crate::schema::terminal::request::CREATE,
                7,
            ),
            payload: vec![],
        };
        assert_eq!(
            codec.encode(&create),
            Err(Error::Invalid("missing SENSITIVE flag"))
        );
        let mut sensitive_create = create;
        sensitive_create.header.sensitive = true;
        let mut encoded = codec.encode(&sensitive_create).unwrap();
        encoded[4] &= !META_SENSITIVE;
        assert_eq!(
            codec.decode(&encoded),
            Err(Error::Invalid("missing SENSITIVE flag"))
        );

        let mut generic_header = FrameHeader::event(family::NET, 0x7777);
        generic_header.compressed = true;
        let mut compressed = codec
            .encode(&Frame {
                header: generic_header,
                payload: b"surface pixels surface pixels".to_vec(),
            })
            .unwrap();
        compressed[..2].copy_from_slice(&family::SURFACE.to_le_bytes());
        compressed[2..4].copy_from_slice(&crate::schema::surface::event::FRAME.to_le_bytes());
        assert_eq!(
            codec.decode(&compressed),
            Err(Error::Invalid("compression-forbidden frame"))
        );

        let mut hello = FrameHeader::request(family::CORE, crate::schema::core::request::HELLO, 1);
        hello.compressed = true;
        assert_eq!(
            codec.encode(&Frame {
                header: hello,
                payload: vec![],
            }),
            Err(Error::Invalid("compression-forbidden frame"))
        );

        let shutdown =
            FrameHeader::request(family::CORE, crate::schema::core::request::SHUTDOWN, 8);
        assert_eq!(
            codec.encode(&Frame {
                header: shutdown,
                payload: vec![],
            }),
            Err(Error::Invalid("missing SENSITIVE flag"))
        );
        let shutdown_result =
            FrameHeader::result(family::CORE, crate::schema::core::request::SHUTDOWN, 8);
        assert_eq!(
            codec.encode(&Frame {
                header: shutdown_result,
                payload: vec![],
            }),
            Err(Error::Invalid("missing SENSITIVE flag"))
        );
    }

    #[test]
    fn operation_policy_keys_include_class() {
        let mut codec = FrameCodec::new(FrameLimits::recommended(), [LZ4_CODEC]).unwrap();
        codec.forbid_compression(family::NET, Class::Event, 9);
        codec.require_sensitive(family::NET, Class::Request, 9);
        let mut request = FrameHeader::request(family::NET, 9, 1);
        request.compressed = true;
        request.sensitive = true;
        assert!(
            codec
                .encode(&Frame {
                    header: request,
                    payload: b"same kind, request class".to_vec(),
                })
                .is_ok()
        );
        let mut event = FrameHeader::event(family::NET, 9);
        event.compressed = true;
        assert_eq!(
            codec.encode(&Frame {
                header: event,
                payload: b"same kind, event class".to_vec(),
            }),
            Err(Error::Invalid("compression-forbidden frame"))
        );
    }

    #[test]
    fn every_truncation_is_rejected() {
        let codec = FrameCodec::new(FrameLimits::recommended(), [LZ4_CODEC]).unwrap();
        let mut header = FrameHeader::request(3, 4, 5);
        header.compressed = true;
        let encoded = codec
            .encode_stream(&Frame {
                header,
                payload: b"compressed payload".to_vec(),
            })
            .unwrap();
        for end in 0..encoded.len() {
            assert!(
                codec.decode_stream(&encoded[..end]).is_err(),
                "prefix {end}"
            );
        }
        assert_eq!(codec.decode_stream(&encoded).unwrap().1, encoded.len());
    }

    #[test]
    fn reserved_meta_and_class_are_rejected() {
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        assert_eq!(
            codec.decode(&[0, 0, 0, 0, 0x10]),
            Err(Error::Invalid("frame meta reserved bits"))
        );
        assert_eq!(
            codec.decode(&[0, 0, 0, 0, 3]),
            Err(Error::Invalid("frame class"))
        );
    }

    #[test]
    fn pre_hello_codec_accepts_only_uncompressed_core_hello_request_or_result() {
        let codec = FrameCodec::pre_hello();
        let hello = Frame {
            header: FrameHeader::request(family::CORE, crate::core::request_kind::HELLO, 1),
            payload: vec![],
        };
        assert!(codec.decode(&codec.encode(&hello).unwrap()).is_ok());
        let result = Frame {
            header: FrameHeader::result(family::CORE, crate::core::request_kind::HELLO, 1),
            payload: vec![],
        };
        assert!(codec.decode(&codec.encode(&result).unwrap()).is_ok());

        let ping = Frame {
            header: FrameHeader::request(family::CORE, crate::core::request_kind::PING, 2),
            payload: vec![],
        };
        assert_eq!(codec.encode(&ping), Err(Error::Invalid("pre-HELLO frame")));

        let zero = Frame {
            header: FrameHeader::request(family::CORE, crate::core::request_kind::HELLO, 0),
            payload: vec![],
        };
        assert_eq!(
            codec.encode(&zero),
            Err(Error::Invalid("request ID presence"))
        );
        assert_eq!(
            codec.decode(&[0, 0, 0, 0, 1, 0, 0, 0, 0]),
            Err(Error::Invalid("request ID presence"))
        );
    }

    #[test]
    fn datagram_requires_generated_operation_predicate_and_context() {
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let datagram = crate::net::Datagram {
            flow_handle: 7,
            sequence: 9,
            payload: vec![1, 2],
        };
        let mut header = FrameHeader::event(family::NET, crate::net::event_kind::DATAGRAM);
        header.sensitive = true;
        let frame = Frame {
            header,
            payload: datagram.encode().unwrap(),
        };
        let encoded = codec
            .encode_datagram(&frame, 1200, DatagramContext::NetNativeFlow)
            .unwrap();
        assert_eq!(
            codec
                .decode_datagram(&encoded, 1200, DatagramContext::NetNativeFlow)
                .unwrap(),
            frame
        );
        assert_eq!(
            codec.encode_datagram(&frame, 1200, DatagramContext::SurfaceFrame),
            Err(Error::Invalid("transport datagram predicate"))
        );

        let mut surface = crate::surface::SurfaceFrame {
            view_id: 1,
            sequence: 2,
            base_sequence: 1,
            capture_ns: 3,
            presentation_ns: 4,
            flags: crate::schema::surface::FRAME_DATAGRAM_ELIGIBLE as u16
                | crate::schema::surface::FRAME_DISCARDABLE as u16,
            codec_version: crate::schema::surface::CODEC_H264_V1 as u16,
            fragment_index: 0,
            fragment_count: 1,
            complete_len: 1,
            payload: vec![5],
        };
        let mut header = FrameHeader::event(family::SURFACE, crate::surface::event_kind::FRAME);
        header.sensitive = true;
        let mut frame = Frame {
            header,
            payload: surface.encode().unwrap(),
        };
        assert!(
            codec
                .encode_datagram(&frame, 1200, DatagramContext::SurfaceFrame)
                .is_ok()
        );
        surface.flags |= crate::schema::surface::FRAME_KEYFRAME as u16;
        frame.payload = surface.encode().unwrap();
        assert_eq!(
            codec.encode_datagram(&frame, 1200, DatagramContext::SurfaceFrame),
            Err(Error::Invalid("transport datagram predicate"))
        );

        let media = crate::media::MediaFrame {
            stream_handle: 1,
            sequence: 2,
            capture_time: 3,
            presentation_time: 4,
            codec_version: crate::schema::media::CODEC_H264 as u16,
            flags: crate::schema::media::FRAME_DISCARDABLE as u16,
            fragment_index: 0,
            fragment_count: 1,
            complete_len: 1,
            payload: vec![6],
        };
        let mut header = FrameHeader::event(family::MEDIA, crate::media::event_kind::FRAME);
        header.sensitive = true;
        let frame = Frame {
            header,
            payload: media.encode().unwrap(),
        };
        assert!(
            codec
                .encode_datagram(&frame, 1200, DatagramContext::MediaFrame)
                .is_ok()
        );
        assert_eq!(
            codec.encode_datagram(&frame, 1200, DatagramContext::SurfaceFrame),
            Err(Error::Invalid("transport datagram predicate"))
        );
    }
}
