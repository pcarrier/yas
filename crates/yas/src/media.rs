//! YAS Media family version 1 payload codecs.

use crate::prelude::*;

use crate::codec::{
    Decode, Decoder, Encode, Error, Extensions, Result, limit_u32, put_bytes_u32, put_i64,
    put_len_u16, put_string_u16, put_string_u32, put_u16, put_u32, put_u64, read_limit_u32,
    reject_unknown_required_extensions,
};
use crate::state::{Record, RecordKind};
use crate::transfer::{Delivery, Descriptor, Direction, InlineOrTransfer, Mode};

pub const VERSION: u16 = crate::schema::media::VERSION;

pub mod request_kind {
    pub use crate::schema::media::request::*;
}

pub mod event_kind {
    pub use crate::schema::media::event::*;
}

/// Convert a backend millisecond timestamp to the canonical audio
/// sample-position timebase, rounding toward the earlier instant.
pub fn audio_sample_position_from_milliseconds(milliseconds: u64, sample_rate: u32) -> Result<u64> {
    if sample_rate == 0 {
        return Err(Error::Invalid("zero Media audio sample rate"));
    }
    u64::try_from(u128::from(milliseconds) * u128::from(sample_rate) / u128::from(1_000u16))
        .map_err(|_| Error::LengthOverflow)
}

/// Convert a canonical audio sample position to backend milliseconds, rounding
/// toward the earlier instant.
pub fn audio_milliseconds_from_sample_position(
    sample_position: u64,
    sample_rate: u32,
) -> Result<u64> {
    if sample_rate == 0 {
        return Err(Error::Invalid("zero Media audio sample rate"));
    }
    u64::try_from(u128::from(sample_position) * u128::from(1_000u16) / u128::from(sample_rate))
        .map_err(|_| Error::LengthOverflow)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_devices: u32,
    pub max_leases_per_session: u32,
    pub max_streams_per_session: u32,
    pub max_portals_per_session: u32,
    pub max_players: u32,
    pub max_formats: u32,
    pub max_inline_metadata_bytes: u32,
    pub max_inline_asset_bytes: u32,
    pub max_portal_metadata_bytes: u32,
    pub max_portal_string_bytes: u32,
    pub max_portal_body_bytes: u32,
    pub max_portal_choices: u32,
    pub max_portal_choice_options: u32,
    pub max_screencast_candidates: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_devices: crate::schema::media::MAX_DEVICES as u32,
        max_leases_per_session: crate::schema::media::MAX_LEASES_PER_SESSION as u32,
        max_streams_per_session: crate::schema::media::MAX_STREAMS_PER_SESSION as u32,
        max_portals_per_session: crate::schema::media::MAX_PORTALS_PER_SESSION as u32,
        max_players: crate::schema::media::MAX_PLAYERS as u32,
        max_formats: crate::schema::media::MAX_FORMATS as u32,
        max_inline_metadata_bytes: crate::schema::media::MAX_INLINE_METADATA_BYTES as u32,
        max_inline_asset_bytes: crate::schema::media::MAX_INLINE_ASSET_BYTES as u32,
        max_portal_metadata_bytes: crate::schema::media::MAX_PORTAL_METADATA_BYTES as u32,
        max_portal_string_bytes: crate::schema::media::MAX_PORTAL_STRING_BYTES as u32,
        max_portal_body_bytes: crate::schema::media::MAX_PORTAL_BODY_BYTES as u32,
        max_portal_choices: crate::schema::media::MAX_PORTAL_CHOICES as u32,
        max_portal_choice_options: crate::schema::media::MAX_PORTAL_CHOICE_OPTIONS as u32,
        max_screencast_candidates: crate::schema::media::MAX_SCREENCAST_CANDIDATES as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let values = [
            (self.max_devices, hard.max_devices),
            (self.max_leases_per_session, hard.max_leases_per_session),
            (self.max_streams_per_session, hard.max_streams_per_session),
            (self.max_portals_per_session, hard.max_portals_per_session),
            (self.max_players, hard.max_players),
            (self.max_formats, hard.max_formats),
            (
                self.max_inline_metadata_bytes,
                hard.max_inline_metadata_bytes,
            ),
            (self.max_inline_asset_bytes, hard.max_inline_asset_bytes),
            (
                self.max_portal_metadata_bytes,
                hard.max_portal_metadata_bytes,
            ),
            (self.max_portal_string_bytes, hard.max_portal_string_bytes),
            (self.max_portal_body_bytes, hard.max_portal_body_bytes),
            (self.max_portal_choices, hard.max_portal_choices),
            (
                self.max_portal_choice_options,
                hard.max_portal_choice_options,
            ),
            (
                self.max_screencast_candidates,
                hard.max_screencast_candidates,
            ),
        ];
        if values
            .into_iter()
            .any(|(value, maximum)| value == 0 || value > maximum)
        {
            return Err(Error::Invalid("Media family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(crate::schema::media::LIMIT_MAX_DEVICES, self.max_devices),
            limit_u32(
                crate::schema::media::LIMIT_MAX_LEASES_PER_SESSION,
                self.max_leases_per_session,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_STREAMS_PER_SESSION,
                self.max_streams_per_session,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_PORTALS_PER_SESSION,
                self.max_portals_per_session,
            ),
            limit_u32(crate::schema::media::LIMIT_MAX_PLAYERS, self.max_players),
            limit_u32(crate::schema::media::LIMIT_MAX_FORMATS, self.max_formats),
            limit_u32(
                crate::schema::media::LIMIT_MAX_INLINE_METADATA_BYTES,
                self.max_inline_metadata_bytes,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_INLINE_ASSET_BYTES,
                self.max_inline_asset_bytes,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_PORTAL_METADATA_BYTES,
                self.max_portal_metadata_bytes,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_PORTAL_STRING_BYTES,
                self.max_portal_string_bytes,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_PORTAL_BODY_BYTES,
                self.max_portal_body_bytes,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_PORTAL_CHOICES,
                self.max_portal_choices,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_PORTAL_CHOICE_OPTIONS,
                self.max_portal_choice_options,
            ),
            limit_u32(
                crate::schema::media::LIMIT_MAX_SCREENCAST_CANDIDATES,
                self.max_screencast_candidates,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::media::LIMIT_MAX_DEVICES as u16,
                crate::schema::media::LIMIT_MAX_LEASES_PER_SESSION as u16,
                crate::schema::media::LIMIT_MAX_STREAMS_PER_SESSION as u16,
                crate::schema::media::LIMIT_MAX_PORTALS_PER_SESSION as u16,
                crate::schema::media::LIMIT_MAX_PLAYERS as u16,
                crate::schema::media::LIMIT_MAX_FORMATS as u16,
                crate::schema::media::LIMIT_MAX_INLINE_METADATA_BYTES as u16,
                crate::schema::media::LIMIT_MAX_INLINE_ASSET_BYTES as u16,
                crate::schema::media::LIMIT_MAX_PORTAL_METADATA_BYTES as u16,
                crate::schema::media::LIMIT_MAX_PORTAL_STRING_BYTES as u16,
                crate::schema::media::LIMIT_MAX_PORTAL_BODY_BYTES as u16,
                crate::schema::media::LIMIT_MAX_PORTAL_CHOICES as u16,
                crate::schema::media::LIMIT_MAX_PORTAL_CHOICE_OPTIONS as u16,
                crate::schema::media::LIMIT_MAX_SCREENCAST_CANDIDATES as u16,
            ],
            "unknown required Media family limit",
        )?;
        let value = Self {
            max_devices: read_limit_u32(extensions, crate::schema::media::LIMIT_MAX_DEVICES)?,
            max_leases_per_session: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_LEASES_PER_SESSION,
            )?,
            max_streams_per_session: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_STREAMS_PER_SESSION,
            )?,
            max_portals_per_session: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_PORTALS_PER_SESSION,
            )?,
            max_players: read_limit_u32(extensions, crate::schema::media::LIMIT_MAX_PLAYERS)?,
            max_formats: read_limit_u32(extensions, crate::schema::media::LIMIT_MAX_FORMATS)?,
            max_inline_metadata_bytes: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_INLINE_METADATA_BYTES,
            )?,
            max_inline_asset_bytes: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_INLINE_ASSET_BYTES,
            )?,
            max_portal_metadata_bytes: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_PORTAL_METADATA_BYTES,
            )?,
            max_portal_string_bytes: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_PORTAL_STRING_BYTES,
            )?,
            max_portal_body_bytes: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_PORTAL_BODY_BYTES,
            )?,
            max_portal_choices: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_PORTAL_CHOICES,
            )?,
            max_portal_choice_options: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_PORTAL_CHOICE_OPTIONS,
            )?,
            max_screencast_candidates: read_limit_u32(
                extensions,
                crate::schema::media::LIMIT_MAX_SCREENCAST_CANDIDATES,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn handle(value: u64, name: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(name))
    } else {
        Ok(())
    }
}

fn revision(value: u64) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid("zero Media revision"))
    } else {
        Ok(())
    }
}

fn audio_codec(codec: u16) -> bool {
    codec == crate::schema::media::CODEC_PCM_S16LE as u16
        || codec == crate::schema::media::CODEC_PCM_F32LE as u16
        || codec == crate::schema::media::CODEC_OPUS as u16
}

fn video_codec(codec: u16) -> bool {
    codec == crate::schema::media::CODEC_H264 as u16
        || codec == crate::schema::media::CODEC_AV1 as u16
        || codec == crate::schema::media::CODEC_H264_444 as u16
        || codec == crate::schema::media::CODEC_AV1_444 as u16
        || codec == crate::schema::media::CODEC_VP9 as u16
        || codec == crate::schema::media::CODEC_MJPEG as u16
}

fn validate_asset_transfer(descriptor: &Descriptor) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Byte
        || descriptor.direction != Direction::SENDER_TO_RECEIVER
        || descriptor.content_family != crate::family::MEDIA
        || descriptor.content_kind != crate::schema::media::ASSET_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("Media asset Transfer descriptor"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFormat {
    pub codec: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate_milli: u32,
    pub extensions: Extensions,
}

impl MediaFormat {
    fn validate(&self) -> Result<()> {
        let audio = audio_codec(self.codec);
        let video = video_codec(self.codec);
        if (!audio && !video)
            || (audio
                && (self.channels == 0
                    || self.sample_rate == 0
                    || self.width != 0
                    || self.height != 0
                    || self.frame_rate_milli != 0))
            || (video
                && (self.channels != 0
                    || self.sample_rate != 0
                    || self.width == 0
                    || self.height == 0
                    || self.frame_rate_milli == 0))
        {
            return Err(Error::Invalid("Media format"));
        }
        self.extensions.validate()
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            codec: decoder.u16()?,
            channels: decoder.u16()?,
            sample_rate: decoder.u32()?,
            width: decoder.u32()?,
            height: decoder.u32()?,
            frame_rate_milli: decoder.u32()?,
            extensions: decoder.extensions()?,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for MediaFormat {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.codec);
        put_u16(out, self.channels);
        put_u32(out, self.sample_rate);
        put_u32(out, self.width);
        put_u32(out, self.height);
        put_u32(out, self.frame_rate_milli);
        self.extensions.encode_tail(out)
    }
}

impl Decode for MediaFormat {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

fn validate_formats(formats: &[MediaFormat], expected_audio: Option<bool>) -> Result<()> {
    if formats.is_empty() || formats.len() > crate::schema::media::MAX_FORMATS as usize {
        return Err(Error::Invalid("Media format count"));
    }
    let mut codecs = BTreeSet::new();
    for format in formats {
        format.validate()?;
        if !codecs.insert(format.codec)
            || expected_audio.is_some_and(|expected| expected != audio_codec(format.codec))
        {
            return Err(Error::Invalid("Media format set"));
        }
    }
    Ok(())
}

fn encode_formats(formats: &[MediaFormat], out: &mut Vec<u8>) -> Result<()> {
    put_len_u16(out, formats.len())?;
    put_u16(out, 0);
    for format in formats {
        format.encode_to(out)?;
    }
    Ok(())
}

fn decode_formats(decoder: &mut Decoder<'_>) -> Result<Vec<MediaFormat>> {
    let count = usize::from(decoder.u16()?);
    if decoder.u16()? != 0
        || count == 0
        || count > crate::schema::media::MAX_FORMATS as usize
        || count > decoder.remaining() / 24
    {
        return Err(Error::Invalid("Media format count or reserved field"));
    }
    let mut formats = Vec::with_capacity(count);
    for _ in 0..count {
        formats.push(MediaFormat::decode_from(decoder)?);
    }
    Ok(formats)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenOutput {
    pub device_handle: u64,
    pub formats: Vec<MediaFormat>,
    pub latency_target_ns: u64,
    /// Requested Opus bitrate in kilobits per second, or zero for the server default.
    pub target_bitrate_kbps: u16,
    pub extensions: Extensions,
}

impl Encode for OpenOutput {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.device_handle, "zero Media device handle")?;
        validate_formats(&self.formats, Some(true))?;
        if u64::from(self.target_bitrate_kbps) > crate::schema::media::MAX_OUTPUT_BITRATE_KBPS {
            return Err(Error::Invalid("Media output target bitrate"));
        }
        put_u64(out, self.device_handle);
        encode_formats(&self.formats, out)?;
        put_u64(out, self.latency_target_ns);
        put_u16(out, self.target_bitrate_kbps);
        put_u16(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for OpenOutput {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let device_handle = decoder.u64()?;
        let formats = decode_formats(&mut decoder)?;
        let latency_target_ns = decoder.u64()?;
        let target_bitrate_kbps = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Media output reserved field"));
        }
        let value = Self {
            device_handle,
            formats,
            latency_target_ns,
            target_bitrate_kbps,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenOutputResult {
    pub stream_handle: u64,
    pub selected_format: MediaFormat,
}

impl Encode for OpenOutputResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.stream_handle, "zero Media stream handle")?;
        if !audio_codec(self.selected_format.codec) {
            return Err(Error::Invalid("Media output format"));
        }
        put_u64(out, self.stream_handle);
        self.selected_format.encode_to(out)
    }
}

impl Decode for OpenOutputResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            stream_handle: decoder.u64()?,
            selected_format: MediaFormat::decode_from(&mut decoder)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcquireDevice {
    pub device_handle: u64,
    pub operation_id: [u8; 16],
    pub kind: u8,
    pub lease_duration_ns: u64,
    pub formats: Vec<MediaFormat>,
    pub extensions: Extensions,
}

impl Encode for AcquireDevice {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.device_handle, "zero Media device handle")?;
        if self.kind != crate::schema::media::KIND_MICROPHONE as u8
            && self.kind != crate::schema::media::KIND_CAMERA as u8
        {
            return Err(Error::Invalid("Media acquired device kind"));
        }
        validate_formats(
            &self.formats,
            Some(self.kind == crate::schema::media::KIND_MICROPHONE as u8),
        )?;
        put_u64(out, self.device_handle);
        out.extend_from_slice(&self.operation_id);
        out.push(self.kind);
        out.extend_from_slice(&[0; 3]);
        put_u64(out, self.lease_duration_ns);
        encode_formats(&self.formats, out)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for AcquireDevice {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let device_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Media ACQUIRE_DEVICE reserved bytes"));
        }
        let lease_duration_ns = decoder.u64()?;
        let formats = decode_formats(&mut decoder)?;
        let value = Self {
            device_handle,
            operation_id,
            kind,
            lease_duration_ns,
            formats,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcquireDeviceResult {
    pub lease_handle: u64,
    pub stream_handle: u64,
    pub expires_server_ns: u64,
    pub selected_format: MediaFormat,
}

impl Encode for AcquireDeviceResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.lease_handle, "zero Media lease handle")?;
        handle(self.stream_handle, "zero Media stream handle")?;
        put_u64(out, self.lease_handle);
        put_u64(out, self.stream_handle);
        put_u64(out, self.expires_server_ns);
        self.selected_format.encode_to(out)
    }
}

impl Decode for AcquireDeviceResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            lease_handle: decoder.u64()?,
            stream_handle: decoder.u64()?,
            expires_server_ns: decoder.u64()?,
            selected_format: MediaFormat::decode_from(&mut decoder)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleOperation {
    pub handle: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for HandleOperation {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.handle, "zero Media operation handle")?;
        put_u64(out, self.handle);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for HandleOperation {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        handle(value.handle, "zero Media operation handle")?;
        Ok(value)
    }
}

pub type ReleaseDevice = HandleOperation;
pub type CloseStream = HandleOperation;

fn validate_portal_text(value: &str, maximum: usize, required: bool) -> Result<()> {
    if value.len() > maximum || value.as_bytes().contains(&0) || required && value.is_empty() {
        return Err(Error::Invalid("Media portal text"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortalChoiceValue {
    pub id: String,
    pub value: String,
}

impl PortalChoiceValue {
    fn validate(&self) -> Result<()> {
        validate_portal_text(
            &self.id,
            crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            true,
        )?;
        validate_portal_text(
            &self.value,
            crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            true,
        )
    }
}

impl Encode for PortalChoiceValue {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_string_u16(out, &self.id)?;
        put_string_u16(out, &self.value)
    }
}

impl Decode for PortalChoiceValue {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            id: decoder.string_u16()?,
            value: decoder.string_u16()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalChoice {
    pub id: String,
    pub label: String,
    pub initial: String,
    pub options: Vec<PortalChoiceValue>,
}

impl Encode for PortalChoice {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_portal_text(
            &self.id,
            crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            true,
        )?;
        validate_portal_text(
            &self.label,
            crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            true,
        )?;
        validate_portal_text(
            &self.initial,
            crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            false,
        )?;
        if self.options.is_empty()
            || self.options.len() > crate::schema::media::MAX_PORTAL_CHOICE_OPTIONS as usize
        {
            return Err(Error::Invalid("Media portal choice option count"));
        }
        let mut ids = BTreeSet::new();
        for option in &self.options {
            option.validate()?;
            if !ids.insert(&option.id) {
                return Err(Error::Invalid("duplicate Media portal choice option"));
            }
        }
        if !self.initial.is_empty() && !ids.contains(&self.initial) {
            return Err(Error::Invalid("Media portal initial choice"));
        }
        put_string_u16(out, &self.id)?;
        put_string_u16(out, &self.label)?;
        put_string_u16(out, &self.initial)?;
        put_len_u16(out, self.options.len())?;
        put_u16(out, 0);
        for option in &self.options {
            put_bytes_u32(out, &option.encode()?)?;
        }
        Ok(())
    }
}

impl Decode for PortalChoice {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let id = decoder.string_u16()?;
        let label = decoder.string_u16()?;
        let initial = decoder.string_u16()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count == 0
            || count > crate::schema::media::MAX_PORTAL_CHOICE_OPTIONS as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("Media portal choice option count"));
        }
        let mut options = Vec::with_capacity(count);
        for _ in 0..count {
            options.push(PortalChoiceValue::decode(decoder.len_bytes_u32()?)?);
        }
        let value = Self {
            id,
            label,
            initial,
            options,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessRequestMetadata {
    pub deadline_server_ns: u64,
    pub parent_surface_handle: Option<u64>,
    pub app_id: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub deny_label: String,
    pub grant_label: String,
    pub icon_name: String,
    pub choices: Vec<PortalChoice>,
}

impl Encode for AccessRequestMetadata {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.deadline_server_ns == 0
            || self.choices.len() > crate::schema::media::MAX_PORTAL_CHOICES as usize
        {
            return Err(Error::Invalid("Media access portal metadata"));
        }
        if let Some(parent) = self.parent_surface_handle {
            handle(parent, "zero Media portal parent Surface handle")?;
        }
        for (value, required, maximum) in [
            (
                self.app_id.as_str(),
                true,
                crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            ),
            (
                self.title.as_str(),
                true,
                crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            ),
            (
                self.subtitle.as_str(),
                false,
                crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            ),
            (
                self.body.as_str(),
                false,
                crate::schema::media::MAX_PORTAL_BODY_BYTES as usize,
            ),
            (
                self.deny_label.as_str(),
                true,
                crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            ),
            (
                self.grant_label.as_str(),
                true,
                crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            ),
            (
                self.icon_name.as_str(),
                false,
                crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            ),
        ] {
            validate_portal_text(value, maximum, required)?;
        }
        let mut ids = BTreeSet::new();
        for choice in &self.choices {
            if !ids.insert(&choice.id) {
                return Err(Error::Invalid("duplicate Media portal choice"));
            }
            choice.encode()?;
        }
        put_u64(out, self.deadline_server_ns);
        put_u64(out, self.parent_surface_handle.unwrap_or(0));
        put_string_u16(out, &self.app_id)?;
        put_string_u16(out, &self.title)?;
        put_string_u16(out, &self.subtitle)?;
        put_string_u32(out, &self.body)?;
        put_string_u16(out, &self.deny_label)?;
        put_string_u16(out, &self.grant_label)?;
        put_string_u16(out, &self.icon_name)?;
        put_len_u16(out, self.choices.len())?;
        put_u16(out, 0);
        for choice in &self.choices {
            put_bytes_u32(out, &choice.encode()?)?;
        }
        Ok(())
    }
}

impl Decode for AccessRequestMetadata {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let deadline_server_ns = decoder.u64()?;
        let parent_surface_handle = match decoder.u64()? {
            0 => None,
            value => Some(value),
        };
        let app_id = decoder.string_u16()?;
        let title = decoder.string_u16()?;
        let subtitle = decoder.string_u16()?;
        let body = decoder.string_u32()?;
        let deny_label = decoder.string_u16()?;
        let grant_label = decoder.string_u16()?;
        let icon_name = decoder.string_u16()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count > crate::schema::media::MAX_PORTAL_CHOICES as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("Media portal choice count"));
        }
        let mut choices = Vec::with_capacity(count);
        for _ in 0..count {
            choices.push(PortalChoice::decode(decoder.len_bytes_u32()?)?);
        }
        let value = Self {
            deadline_server_ns,
            parent_surface_handle,
            app_id,
            title,
            subtitle,
            body,
            deny_label,
            grant_label,
            icon_name,
            choices,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenCastCandidate {
    pub surface_handle: u64,
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub app_id: String,
    pub thumbnail_hash: Option<[u8; 32]>,
}

impl Encode for ScreenCastCandidate {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(
            self.surface_handle,
            "zero Media screencast candidate Surface handle",
        )?;
        if self.width == 0 || self.height == 0 {
            return Err(Error::Invalid("Media screencast candidate dimensions"));
        }
        validate_portal_text(
            &self.title,
            crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            true,
        )?;
        validate_portal_text(
            &self.app_id,
            crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            true,
        )?;
        put_u64(out, self.surface_handle);
        put_u32(out, self.width);
        put_u32(out, self.height);
        put_string_u16(out, &self.title)?;
        put_string_u16(out, &self.app_id)?;
        out.push(u8::from(self.thumbnail_hash.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(hash) = self.thumbnail_hash {
            out.extend_from_slice(&hash);
        }
        Ok(())
    }
}

impl Decode for ScreenCastCandidate {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let surface_handle = decoder.u64()?;
        let width = decoder.u32()?;
        let height = decoder.u32()?;
        let title = decoder.string_u16()?;
        let app_id = decoder.string_u16()?;
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Media thumbnail hash presence"));
        }
        let value = Self {
            surface_handle,
            width,
            height,
            title,
            app_id,
            thumbnail_hash: if present != 0 {
                Some(decoder.array_32()?)
            } else {
                None
            },
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenCastRequestMetadata {
    pub deadline_server_ns: u64,
    pub parent_surface_handle: Option<u64>,
    pub app_id: String,
    pub multiple: bool,
    pub candidates: Vec<ScreenCastCandidate>,
}

impl Encode for ScreenCastRequestMetadata {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.deadline_server_ns == 0
            || self.candidates.is_empty()
            || self.candidates.len() > crate::schema::media::MAX_SCREENCAST_CANDIDATES as usize
        {
            return Err(Error::Invalid("Media screencast request metadata"));
        }
        if let Some(parent) = self.parent_surface_handle {
            handle(parent, "zero Media portal parent Surface handle")?;
        }
        validate_portal_text(
            &self.app_id,
            crate::schema::media::MAX_PORTAL_STRING_BYTES as usize,
            true,
        )?;
        let mut handles = BTreeSet::new();
        for candidate in &self.candidates {
            if !handles.insert(candidate.surface_handle) {
                return Err(Error::Invalid("duplicate Media screencast candidate"));
            }
            candidate.encode()?;
        }
        put_u64(out, self.deadline_server_ns);
        put_u64(out, self.parent_surface_handle.unwrap_or(0));
        put_string_u16(out, &self.app_id)?;
        out.push(u8::from(self.multiple));
        out.extend_from_slice(&[0; 3]);
        put_len_u16(out, self.candidates.len())?;
        put_u16(out, 0);
        for candidate in &self.candidates {
            put_bytes_u32(out, &candidate.encode()?)?;
        }
        Ok(())
    }
}

impl Decode for ScreenCastRequestMetadata {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let deadline_server_ns = decoder.u64()?;
        let parent_surface_handle = match decoder.u64()? {
            0 => None,
            value => Some(value),
        };
        let app_id = decoder.string_u16()?;
        let multiple = match decoder.u8()? {
            0 => false,
            1 => true,
            _ => return Err(Error::Invalid("Media screencast multiple value")),
        };
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Media screencast reserved bytes"));
        }
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count == 0
            || count > crate::schema::media::MAX_SCREENCAST_CANDIDATES as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("Media screencast candidate count"));
        }
        let mut candidates = Vec::with_capacity(count);
        for _ in 0..count {
            candidates.push(ScreenCastCandidate::decode(decoder.len_bytes_u32()?)?);
        }
        let value = Self {
            deadline_server_ns,
            parent_surface_handle,
            app_id,
            multiple,
            candidates,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalRequestMetadata {
    Access(AccessRequestMetadata),
    ScreenCast(ScreenCastRequestMetadata),
}

impl PortalRequestMetadata {
    fn kind(&self) -> u16 {
        match self {
            Self::Access(_) => crate::schema::media::PORTAL_KIND_ACCESS as u16,
            Self::ScreenCast(_) => crate::schema::media::PORTAL_KIND_SCREENCAST as u16,
        }
    }

    fn encode_body(&self) -> Result<Vec<u8>> {
        match self {
            Self::Access(value) => value.encode(),
            Self::ScreenCast(value) => value.encode(),
        }
    }

    fn decode_body(kind: u16, input: &[u8]) -> Result<Self> {
        match kind {
            value if value == crate::schema::media::PORTAL_KIND_ACCESS as u16 => {
                Ok(Self::Access(AccessRequestMetadata::decode(input)?))
            }
            value if value == crate::schema::media::PORTAL_KIND_SCREENCAST as u16 => {
                Ok(Self::ScreenCast(ScreenCastRequestMetadata::decode(input)?))
            }
            _ => Err(Error::Invalid("Media portal request kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessGrantMetadata {
    pub choices: Vec<PortalChoiceValue>,
}

impl Encode for AccessGrantMetadata {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.choices.len() > crate::schema::media::MAX_PORTAL_CHOICES as usize {
            return Err(Error::Invalid("Media portal grant choice count"));
        }
        let mut ids = BTreeSet::new();
        for choice in &self.choices {
            choice.validate()?;
            if !ids.insert(&choice.id) {
                return Err(Error::Invalid("duplicate Media portal grant choice"));
            }
        }
        put_len_u16(out, self.choices.len())?;
        put_u16(out, 0);
        for choice in &self.choices {
            put_bytes_u32(out, &choice.encode()?)?;
        }
        Ok(())
    }
}

impl Decode for AccessGrantMetadata {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count > crate::schema::media::MAX_PORTAL_CHOICES as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("Media portal grant choice count"));
        }
        let mut choices = Vec::with_capacity(count);
        for _ in 0..count {
            choices.push(PortalChoiceValue::decode(decoder.len_bytes_u32()?)?);
        }
        let value = Self { choices };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenCastGrantMetadata {
    pub surface_handles: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenCastGrantedStream {
    pub surface_handle: u64,
    pub stream_handle: u64,
}

impl ScreenCastGrantedStream {
    fn encode_into(self, out: &mut Vec<u8>) -> Result<()> {
        handle(
            self.surface_handle,
            "zero granted Media screencast Surface handle",
        )?;
        handle(
            self.stream_handle,
            "zero granted Media screencast stream handle",
        )?;
        put_u64(out, self.surface_handle);
        put_u64(out, self.stream_handle);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            surface_handle: decoder.u64()?,
            stream_handle: decoder.u64()?,
        };
        let mut ignored = Vec::new();
        value.encode_into(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenCastGrantedMetadata {
    pub streams: Vec<ScreenCastGrantedStream>,
}

impl Encode for ScreenCastGrantedMetadata {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.streams.is_empty()
            || self.streams.len() > crate::schema::media::MAX_SCREENCAST_CANDIDATES as usize
        {
            return Err(Error::Invalid("Media granted screencast stream count"));
        }
        let mut surfaces = BTreeSet::new();
        let mut streams = BTreeSet::new();
        for stream in &self.streams {
            if !surfaces.insert(stream.surface_handle) || !streams.insert(stream.stream_handle) {
                return Err(Error::Invalid("duplicate granted Media screencast stream"));
            }
            let mut ignored = Vec::new();
            stream.encode_into(&mut ignored)?;
        }
        put_len_u16(out, self.streams.len())?;
        put_u16(out, 0);
        for stream in &self.streams {
            stream.encode_into(out)?;
        }
        Ok(())
    }
}

impl Decode for ScreenCastGrantedMetadata {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count == 0
            || count > crate::schema::media::MAX_SCREENCAST_CANDIDATES as usize
            || count > decoder.remaining() / 16
        {
            return Err(Error::Invalid("Media granted screencast stream count"));
        }
        let mut streams = Vec::with_capacity(count);
        for _ in 0..count {
            streams.push(ScreenCastGrantedStream::decode_from(&mut decoder)?);
        }
        let value = Self { streams };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

impl Encode for ScreenCastGrantMetadata {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.surface_handles.is_empty()
            || self.surface_handles.len() > crate::schema::media::MAX_SCREENCAST_CANDIDATES as usize
        {
            return Err(Error::Invalid("Media screencast grant count"));
        }
        let mut handles = BTreeSet::new();
        for handle_value in &self.surface_handles {
            handle(*handle_value, "zero granted Media Surface handle")?;
            if !handles.insert(handle_value) {
                return Err(Error::Invalid("duplicate granted Media Surface handle"));
            }
        }
        put_len_u16(out, self.surface_handles.len())?;
        put_u16(out, 0);
        for handle_value in &self.surface_handles {
            put_u64(out, *handle_value);
        }
        Ok(())
    }
}

impl Decode for ScreenCastGrantMetadata {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count == 0
            || count > crate::schema::media::MAX_SCREENCAST_CANDIDATES as usize
            || count > decoder.remaining() / 8
        {
            return Err(Error::Invalid("Media screencast grant count"));
        }
        let mut surface_handles = Vec::with_capacity(count);
        for _ in 0..count {
            surface_handles.push(decoder.u64()?);
        }
        let value = Self { surface_handles };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalReplyMetadata {
    Empty,
    AccessGrant(AccessGrantMetadata),
    ScreenCastGrant(ScreenCastGrantMetadata),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalGrantedMetadata {
    Access(AccessGrantMetadata),
    ScreenCast(ScreenCastGrantedMetadata),
}

impl PortalGrantedMetadata {
    fn encode_for(&self, kind: u16) -> Result<Vec<u8>> {
        match (kind, self) {
            (kind, Self::Access(value))
                if kind == crate::schema::media::PORTAL_KIND_ACCESS as u16 =>
            {
                value.encode()
            }
            (kind, Self::ScreenCast(value))
                if kind == crate::schema::media::PORTAL_KIND_SCREENCAST as u16 =>
            {
                value.encode()
            }
            _ => Err(Error::Invalid("Media granted portal state metadata")),
        }
    }

    fn decode_for(kind: u16, input: &[u8]) -> Result<Self> {
        match kind {
            value if value == crate::schema::media::PORTAL_KIND_ACCESS as u16 => {
                Ok(Self::Access(AccessGrantMetadata::decode(input)?))
            }
            value if value == crate::schema::media::PORTAL_KIND_SCREENCAST as u16 => {
                Ok(Self::ScreenCast(ScreenCastGrantedMetadata::decode(input)?))
            }
            _ => Err(Error::Invalid("Media granted portal state kind")),
        }
    }
}

impl PortalReplyMetadata {
    fn encode_for(&self, kind: u16, decision: u8) -> Result<Vec<u8>> {
        let grant = decision == crate::schema::media::PORTAL_DECISION_GRANT as u8;
        match (kind, grant, self) {
            (_, false, Self::Empty) => Ok(Vec::new()),
            (kind, true, Self::AccessGrant(value))
                if kind == crate::schema::media::PORTAL_KIND_ACCESS as u16 =>
            {
                value.encode()
            }
            (kind, true, Self::ScreenCastGrant(value))
                if kind == crate::schema::media::PORTAL_KIND_SCREENCAST as u16 =>
            {
                value.encode()
            }
            _ => Err(Error::Invalid("Media portal reply metadata")),
        }
    }

    fn decode_for(kind: u16, decision: u8, input: &[u8]) -> Result<Self> {
        if decision != crate::schema::media::PORTAL_DECISION_GRANT as u8 {
            if !input.is_empty() {
                return Err(Error::Invalid("nonempty denied Media portal metadata"));
            }
            return Ok(Self::Empty);
        }
        match kind {
            value if value == crate::schema::media::PORTAL_KIND_ACCESS as u16 => {
                Ok(Self::AccessGrant(AccessGrantMetadata::decode(input)?))
            }
            value if value == crate::schema::media::PORTAL_KIND_SCREENCAST as u16 => Ok(
                Self::ScreenCastGrant(ScreenCastGrantMetadata::decode(input)?),
            ),
            _ => Err(Error::Invalid("Media portal reply kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalRecordMetadata {
    Request(PortalRequestMetadata),
    Grant(PortalGrantedMetadata),
    Empty,
}

impl PortalRecordMetadata {
    fn encode_for(&self, kind: u16, state: u16) -> Result<Vec<u8>> {
        match (state, self) {
            (state, Self::Request(value))
                if state == crate::schema::media::PORTAL_PENDING as u16 && value.kind() == kind =>
            {
                value.encode_body()
            }
            (state, Self::Grant(value)) if state == crate::schema::media::PORTAL_GRANTED as u16 => {
                value.encode_for(kind)
            }
            (state, Self::Empty)
                if state == crate::schema::media::PORTAL_DENIED as u16
                    || state == crate::schema::media::PORTAL_CANCELLED as u16
                    || state == crate::schema::media::PORTAL_WITHDRAWN as u16 =>
            {
                Ok(Vec::new())
            }
            _ => Err(Error::Invalid("Media portal state metadata")),
        }
    }

    fn decode_for(kind: u16, state: u16, input: &[u8]) -> Result<Self> {
        match state {
            value if value == crate::schema::media::PORTAL_PENDING as u16 => Ok(Self::Request(
                PortalRequestMetadata::decode_body(kind, input)?,
            )),
            value if value == crate::schema::media::PORTAL_GRANTED as u16 => {
                Ok(Self::Grant(PortalGrantedMetadata::decode_for(kind, input)?))
            }
            value
                if value == crate::schema::media::PORTAL_DENIED as u16
                    || value == crate::schema::media::PORTAL_CANCELLED as u16
                    || value == crate::schema::media::PORTAL_WITHDRAWN as u16 =>
            {
                if !input.is_empty() {
                    return Err(Error::Invalid("nonempty terminal Media portal metadata"));
                }
                Ok(Self::Empty)
            }
            _ => Err(Error::Invalid("Media portal state")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalReply {
    pub portal_handle: u64,
    pub revision: u64,
    pub operation_id: [u8; 16],
    pub kind: u16,
    pub decision: u8,
    pub metadata: PortalReplyMetadata,
    pub extensions: Extensions,
}

impl Encode for PortalReply {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.portal_handle, "zero Media portal handle")?;
        revision(self.revision)?;
        if self.operation_id == [0; 16]
            || self.kind > crate::schema::media::PORTAL_KIND_SCREENCAST as u16
            || self.decision > crate::schema::media::PORTAL_DECISION_CANCEL as u8
        {
            return Err(Error::Invalid("Media portal reply identity or decision"));
        }
        let metadata = self.metadata.encode_for(self.kind, self.decision)?;
        if metadata.len() > crate::schema::media::MAX_PORTAL_METADATA_BYTES as usize {
            return Err(Error::Invalid("Media portal reply metadata"));
        }
        put_u64(out, self.portal_handle);
        put_u64(out, self.revision);
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.kind);
        out.push(self.decision);
        out.push(0);
        put_bytes_u32(out, &metadata)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for PortalReply {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let portal_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let kind = decoder.u16()?;
        let decision = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Media PORTAL_REPLY reserved byte"));
        }
        let metadata = PortalReplyMetadata::decode_for(kind, decision, decoder.len_bytes_u32()?)?;
        let value = Self {
            portal_handle,
            revision,
            operation_id,
            kind,
            decision,
            metadata,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalClose {
    pub portal_handle: u64,
    pub revision: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for PortalClose {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.portal_handle, "zero Media portal handle")?;
        revision(self.revision)?;
        if self.operation_id == [0; 16] {
            return Err(Error::Invalid("zero Media portal-close operation ID"));
        }
        put_u64(out, self.portal_handle);
        put_u64(out, self.revision);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for PortalClose {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            portal_handle: decoder.u64()?,
            revision: decoder.u64()?,
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
pub struct PlayerAction {
    pub player_handle: u64,
    pub operation_id: [u8; 16],
    pub action: u16,
    pub value: i64,
    pub extensions: Extensions,
}

impl Encode for PlayerAction {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.player_handle, "zero Media player handle")?;
        if self.action > crate::schema::media::PLAYER_ACTION_RAISE as u16 {
            return Err(Error::Invalid("Media player action"));
        }
        put_u64(out, self.player_handle);
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.action);
        put_u16(out, 0);
        put_i64(out, self.value);
        self.extensions.encode_tail(out)
    }
}

impl Decode for PlayerAction {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let player_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let action = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Media PLAYER_ACTION reserved field"));
        }
        let value = Self {
            player_handle,
            operation_id,
            action,
            value: decoder.i64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchAsset {
    pub content_hash: [u8; 32],
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for FetchAsset {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for FetchAsset {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            content_hash: decoder.array_32()?,
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetResult(pub InlineOrTransfer);

impl AssetResult {
    fn validate(&self) -> Result<()> {
        match &self.0.delivery {
            Delivery::Inline(bytes)
                if bytes.len() <= crate::schema::media::MAX_INLINE_ASSET_BYTES as usize =>
            {
                Ok(())
            }
            Delivery::Inline(bytes) => Err(Error::LimitExceeded {
                limit: "Media inline asset bytes",
                actual: bytes.len() as u64,
                maximum: crate::schema::media::MAX_INLINE_ASSET_BYTES,
            }),
            Delivery::Transfer(descriptor) => validate_asset_transfer(descriptor),
        }
    }
}

impl Encode for AssetResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        self.0.encode_to(out)
    }
}

impl Decode for AssetResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let value = Self(InlineOrTransfer::decode(input)?);
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalRequest {
    pub portal_handle: u64,
    pub revision: u64,
    pub kind: u16,
    pub flags: u16,
    pub application_handle: u64,
    pub metadata: PortalRequestMetadata,
    pub extensions: Extensions,
}

impl Encode for PortalRequest {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.portal_handle, "zero Media portal handle")?;
        revision(self.revision)?;
        if self.kind > crate::schema::media::PORTAL_KIND_SCREENCAST as u16
            || self.flags & !(crate::schema::media::PORTAL_REQUEST_FLAGS as u16) != 0
            || self.kind != self.metadata.kind()
        {
            return Err(Error::Invalid("Media portal request"));
        }
        let metadata = self.metadata.encode_body()?;
        if metadata.len() > crate::schema::media::MAX_PORTAL_METADATA_BYTES as usize {
            return Err(Error::Invalid("Media portal request metadata"));
        }
        put_u64(out, self.portal_handle);
        put_u64(out, self.revision);
        put_u16(out, self.kind);
        put_u16(out, self.flags);
        put_u64(out, self.application_handle);
        put_bytes_u32(out, &metadata)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for PortalRequest {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let portal_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let kind = decoder.u16()?;
        let flags = decoder.u16()?;
        let application_handle = decoder.u64()?;
        let value = Self {
            portal_handle,
            revision,
            kind,
            flags,
            application_handle,
            metadata: PortalRequestMetadata::decode_body(kind, decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFrame {
    pub stream_handle: u64,
    pub sequence: u64,
    pub capture_time: u64,
    pub presentation_time: u64,
    pub codec_version: u16,
    pub flags: u16,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub complete_len: u32,
    pub payload: Vec<u8>,
}

impl MediaFrame {
    pub fn datagram_eligible(&self) -> bool {
        let forbidden = crate::schema::media::FRAME_KEYFRAME as u16
            | crate::schema::media::FRAME_CODEC_CONFIG as u16
            | crate::schema::media::FRAME_END_OF_STREAM as u16;
        self.flags & crate::schema::media::FRAME_DISCARDABLE as u16 != 0
            && self.flags & forbidden == 0
    }
}

impl Encode for MediaFrame {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.stream_handle, "zero Media stream handle")?;
        if (!audio_codec(self.codec_version) && !video_codec(self.codec_version))
            || self.flags & !(crate::schema::media::FRAME_FLAGS_MASK as u16) != 0
            || self.fragment_count == 0
            || self.fragment_index >= self.fragment_count
            || self.complete_len == 0
            || self.payload.is_empty()
            || self.payload.len() > self.complete_len as usize
            || self.payload.len() > crate::frame::HARD_MAX_BULK_CHUNK as usize
        {
            return Err(Error::Invalid("Media frame"));
        }
        put_u64(out, self.stream_handle);
        put_u64(out, self.sequence);
        put_u64(out, self.capture_time);
        put_u64(out, self.presentation_time);
        put_u16(out, self.codec_version);
        put_u16(out, self.flags);
        put_u16(out, self.fragment_index);
        put_u16(out, self.fragment_count);
        put_u32(out, self.complete_len);
        out.extend_from_slice(&self.payload);
        Ok(())
    }
}

impl Decode for MediaFrame {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            stream_handle: decoder.u64()?,
            sequence: decoder.u64()?,
            capture_time: decoder.u64()?,
            presentation_time: decoder.u64()?,
            codec_version: decoder.u16()?,
            flags: decoder.u16()?,
            fragment_index: decoder.u16()?,
            fragment_count: decoder.u16()?,
            complete_len: decoder.u32()?,
            payload: decoder.rest().to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameAck {
    pub stream_handle: u64,
    pub consumed_sequence: u64,
    pub queue_depth: u16,
    pub desired_credit_frames: u16,
}

impl Encode for FrameAck {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.stream_handle, "zero Media stream handle")?;
        put_u64(out, self.stream_handle);
        put_u64(out, self.consumed_sequence);
        put_u16(out, self.queue_depth);
        put_u16(out, self.desired_credit_frames);
        Ok(())
    }
}

impl Decode for FrameAck {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            stream_handle: decoder.u64()?,
            consumed_sequence: decoder.u64()?,
            queue_depth: decoder.u16()?,
            desired_credit_frames: decoder.u16()?,
        };
        decoder.finish()?;
        handle(value.stream_handle, "zero Media stream handle")?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamStatus {
    pub stream_handle: u64,
    pub revision: u64,
    pub status: u16,
    pub flags: u16,
    pub codec_config: Vec<u8>,
    pub extensions: Extensions,
}

impl Encode for StreamStatus {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.stream_handle, "zero Media stream handle")?;
        revision(self.revision)?;
        if self.status > crate::schema::media::STREAM_ERROR as u16
            || self.flags & !(crate::schema::media::STREAM_FLAGS_MASK as u16) != 0
            || self.codec_config.len() > crate::schema::media::MAX_INLINE_METADATA_BYTES as usize
        {
            return Err(Error::Invalid("Media stream status"));
        }
        put_u64(out, self.stream_handle);
        put_u64(out, self.revision);
        put_u16(out, self.status);
        put_u16(out, self.flags);
        put_bytes_u32(out, &self.codec_config)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for StreamStatus {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            stream_handle: decoder.u64()?,
            revision: decoder.u64()?,
            status: decoder.u16()?,
            flags: decoder.u16()?,
            codec_config: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRecord {
    pub device_handle: u64,
    pub revision: u64,
    pub kind: u8,
    pub state: u8,
    pub flags: u16,
    pub name: String,
    pub formats: Vec<MediaFormat>,
    pub extensions: Extensions,
}

impl Encode for DeviceRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.device_handle, "zero Media device handle")?;
        revision(self.revision)?;
        if self.kind > crate::schema::media::KIND_CAMERA as u8
            || self.state > crate::schema::media::DEVICE_PERMISSION_REQUIRED as u8
            || self.flags & !(crate::schema::media::DEVICE_FLAGS_MASK as u16) != 0
        {
            return Err(Error::Invalid("Media device record"));
        }
        validate_formats(
            &self.formats,
            Some(self.kind != crate::schema::media::KIND_CAMERA as u8),
        )?;
        put_u64(out, self.device_handle);
        put_u64(out, self.revision);
        out.push(self.kind);
        out.push(self.state);
        put_u16(out, self.flags);
        put_string_u16(out, &self.name)?;
        encode_formats(&self.formats, out)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for DeviceRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let device_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let kind = decoder.u8()?;
        let state = decoder.u8()?;
        let flags = decoder.u16()?;
        let name = decoder.string_u16()?;
        let formats = decode_formats(&mut decoder)?;
        let value = Self {
            device_handle,
            revision,
            kind,
            state,
            flags,
            name,
            formats,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseRecord {
    pub lease_handle: u64,
    pub revision: u64,
    pub device_handle: u64,
    pub owner_session: [u8; 16],
    pub lifecycle: u8,
    pub expires_server_ns: u64,
    pub extensions: Extensions,
}

impl Encode for LeaseRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.lease_handle, "zero Media lease handle")?;
        handle(self.device_handle, "zero Media device handle")?;
        revision(self.revision)?;
        if self.lifecycle > crate::schema::media::LEASE_RELEASED as u8 {
            return Err(Error::Invalid("Media lease lifecycle"));
        }
        put_u64(out, self.lease_handle);
        put_u64(out, self.revision);
        put_u64(out, self.device_handle);
        out.extend_from_slice(&self.owner_session);
        out.push(self.lifecycle);
        out.extend_from_slice(&[0; 7]);
        put_u64(out, self.expires_server_ns);
        self.extensions.encode_tail(out)
    }
}

impl Decode for LeaseRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let lease_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let device_handle = decoder.u64()?;
        let owner_session = decoder.array_16()?;
        let lifecycle = decoder.u8()?;
        if decoder.take(7)? != [0; 7] {
            return Err(Error::Invalid("Media lease reserved bytes"));
        }
        let value = Self {
            lease_handle,
            revision,
            device_handle,
            owner_session,
            lifecycle,
            expires_server_ns: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalRecord {
    pub portal_handle: u64,
    pub revision: u64,
    pub kind: u16,
    pub state: u16,
    pub owner_session: [u8; 16],
    pub metadata: PortalRecordMetadata,
    pub extensions: Extensions,
}

impl Encode for PortalRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.portal_handle, "zero Media portal handle")?;
        revision(self.revision)?;
        if self.kind > crate::schema::media::PORTAL_KIND_SCREENCAST as u16
            || self.state > crate::schema::media::PORTAL_WITHDRAWN as u16
        {
            return Err(Error::Invalid("Media portal record"));
        }
        let metadata = self.metadata.encode_for(self.kind, self.state)?;
        if metadata.len() > crate::schema::media::MAX_PORTAL_METADATA_BYTES as usize {
            return Err(Error::Invalid("Media portal record metadata"));
        }
        put_u64(out, self.portal_handle);
        put_u64(out, self.revision);
        put_u16(out, self.kind);
        put_u16(out, self.state);
        out.extend_from_slice(&self.owner_session);
        put_bytes_u32(out, &metadata)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for PortalRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let portal_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let kind = decoder.u16()?;
        let state = decoder.u16()?;
        let owner_session = decoder.array_16()?;
        let value = Self {
            portal_handle,
            revision,
            kind,
            state,
            owner_session,
            metadata: PortalRecordMetadata::decode_for(kind, state, decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerRecord {
    pub player_handle: u64,
    pub revision: u64,
    pub state: u16,
    pub flags: u16,
    pub position_us: i64,
    pub duration_us: i64,
    pub identity: String,
    pub desktop_entry: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub extensions: Extensions,
}

impl PlayerRecord {
    /// Exact server-selected active player state.  Absence means the peer did
    /// not publish that optional v1 extension; it must not be inferred from
    /// playback state or capabilities.
    pub fn active(&self) -> Result<Option<bool>> {
        let Some(extension) = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::media::PLAYER_ACTIVE_EXTENSION as u16
        }) else {
            return Ok(None);
        };
        match extension.value.as_slice() {
            [0] => Ok(Some(false)),
            [1] => Ok(Some(true)),
            _ => Err(Error::Invalid("Media player active state")),
        }
    }

    /// Browser-loadable album artwork URL. Embedded artwork uses the separate
    /// content-hash extension and is fetched through Media FETCH_ASSET.
    pub fn album_art_url(&self) -> Result<Option<&str>> {
        let Some(extension) = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::media::PLAYER_ALBUM_ART_URL_EXTENSION as u16
        }) else {
            return Ok(None);
        };
        if extension.value.is_empty()
            || extension.value.len() > crate::schema::media::PLAYER_ALBUM_ART_URL_MAX_BYTES as usize
        {
            return Err(Error::Invalid("Media album art URL length"));
        }
        let url = core::str::from_utf8(&extension.value)
            .map_err(|_| Error::Invalid("Media album art URL UTF-8"))?;
        let (scheme, rest) = url
            .split_once("://")
            .ok_or(Error::Invalid("Media album art URL scheme"))?;
        if (!scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http"))
            || rest.is_empty()
        {
            return Err(Error::Invalid("Media album art URL"));
        }
        Ok(Some(url))
    }
}

pub fn player_active_extension(active: bool) -> crate::codec::Extension {
    crate::codec::Extension {
        tag: crate::schema::media::PLAYER_ACTIVE_EXTENSION as u16,
        required: false,
        value: vec![u8::from(active)],
    }
}

impl Encode for PlayerRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.player_handle, "zero Media player handle")?;
        revision(self.revision)?;
        if self.state > crate::schema::media::PLAYER_PLAYING as u16
            || self.flags & !(crate::schema::media::PLAYER_FLAGS_MASK as u16) != 0
            || self.position_us < 0
            || self.duration_us < -1
        {
            return Err(Error::Invalid("Media player record"));
        }
        self.extensions.validate()?;
        let album_art_hash = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::media::PLAYER_ALBUM_ART_HASH_EXTENSION as u16
        });
        if album_art_hash.is_some_and(|art| art.value.len() != 32) {
            return Err(Error::Invalid("Media album art hash"));
        }
        let album_art_url = self.album_art_url()?;
        if album_art_hash.is_some() && album_art_url.is_some() {
            return Err(Error::Invalid("multiple Media album art sources"));
        }
        self.active()?;
        put_u64(out, self.player_handle);
        put_u64(out, self.revision);
        put_u16(out, self.state);
        put_u16(out, self.flags);
        put_i64(out, self.position_us);
        put_i64(out, self.duration_us);
        put_string_u16(out, &self.identity)?;
        put_string_u16(out, &self.desktop_entry)?;
        put_string_u16(out, &self.title)?;
        put_string_u16(out, &self.artist)?;
        put_string_u16(out, &self.album)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for PlayerRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            player_handle: decoder.u64()?,
            revision: decoder.u64()?,
            state: decoder.u16()?,
            flags: decoder.u16()?,
            position_us: decoder.i64()?,
            duration_us: decoder.i64()?,
            identity: decoder.string_u16()?,
            desktop_entry: decoder.string_u16()?,
            title: decoder.string_u16()?,
            artist: decoder.string_u16()?,
            album: decoder.string_u16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompleteEntity {
    Device(DeviceRecord),
    Lease(LeaseRecord),
    Portal(Box<PortalRecord>),
    Player(PlayerRecord),
}

impl CompleteEntity {
    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Media complete state record kind"));
        }
        let mut body = Vec::new();
        let entity = match self {
            Self::Device(_) => crate::schema::media::ENTITY_DEVICE,
            Self::Lease(_) => crate::schema::media::ENTITY_LEASE,
            Self::Portal(_) => crate::schema::media::ENTITY_PORTAL,
            Self::Player(_) => crate::schema::media::ENTITY_PLAYER,
        } as u16;
        put_u16(&mut body, entity);
        put_u16(&mut body, 0);
        match self {
            Self::Device(value) => value.encode_to(&mut body)?,
            Self::Lease(value) => value.encode_to(&mut body)?,
            Self::Portal(value) => value.encode_to(&mut body)?,
            Self::Player(value) => value.encode_to(&mut body)?,
        }
        Ok(Record {
            kind,
            required: false,
            body,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityPatch {
    pub entity_kind: u16,
    pub handle: u64,
    pub revision: u64,
    pub extensions: Extensions,
}

impl EntityPatch {
    pub fn state_record(&self) -> Result<Record> {
        validate_entity(self.entity_kind, self.handle, self.revision)?;
        let mut body = Vec::new();
        put_u16(&mut body, self.entity_kind);
        put_u16(&mut body, 0);
        put_u64(&mut body, self.handle);
        put_u64(&mut body, self.revision);
        self.extensions.encode_tail(&mut body)?;
        Ok(Record {
            kind: RecordKind::Patch,
            required: false,
            body,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedEntity {
    pub entity_kind: u16,
    pub handle: u64,
    pub revision: u64,
}

impl RemovedEntity {
    pub fn state_record(self) -> Result<Record> {
        validate_entity(self.entity_kind, self.handle, self.revision)?;
        let mut body = Vec::new();
        put_u16(&mut body, self.entity_kind);
        put_u16(&mut body, 0);
        put_u64(&mut body, self.handle);
        put_u64(&mut body, self.revision);
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body,
        })
    }
}

fn validate_entity(entity: u16, entity_handle: u64, entity_revision: u64) -> Result<()> {
    if entity > crate::schema::media::ENTITY_PLAYER as u16 {
        return Err(Error::Invalid("Media state entity"));
    }
    handle(entity_handle, "zero Media entity handle")?;
    revision(entity_revision)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateMutation {
    Complete(CompleteEntity),
    Patch(EntityPatch),
    Remove(RemovedEntity),
}

pub fn decode_state_record(record: &Record) -> Result<StateMutation> {
    let mut decoder = Decoder::new(&record.body);
    let entity_kind = decoder.u16()?;
    if decoder.u16()? != 0 {
        return Err(Error::Invalid("Media state entity reserved field"));
    }
    let payload = decoder.rest();
    decoder.finish()?;
    match record.kind {
        RecordKind::Add | RecordKind::Replace => {
            let complete = match entity_kind {
                value if value == crate::schema::media::ENTITY_DEVICE as u16 => {
                    CompleteEntity::Device(DeviceRecord::decode(payload)?)
                }
                value if value == crate::schema::media::ENTITY_LEASE as u16 => {
                    CompleteEntity::Lease(LeaseRecord::decode(payload)?)
                }
                value if value == crate::schema::media::ENTITY_PORTAL as u16 => {
                    CompleteEntity::Portal(Box::new(PortalRecord::decode(payload)?))
                }
                value if value == crate::schema::media::ENTITY_PLAYER as u16 => {
                    CompleteEntity::Player(PlayerRecord::decode(payload)?)
                }
                _ => return Err(Error::Invalid("Media state entity")),
            };
            Ok(StateMutation::Complete(complete))
        }
        RecordKind::Patch => {
            let mut value = Decoder::new(payload);
            let patch = EntityPatch {
                entity_kind,
                handle: value.u64()?,
                revision: value.u64()?,
                extensions: value.extensions()?,
            };
            value.finish()?;
            patch.state_record()?;
            Ok(StateMutation::Patch(patch))
        }
        RecordKind::Remove => {
            let mut value = Decoder::new(payload);
            let removed = RemovedEntity {
                entity_kind,
                handle: value.u64()?,
                revision: value.u64()?,
            };
            value.finish()?;
            removed.state_record()?;
            Ok(StateMutation::Remove(removed))
        }
        _ => Err(Error::Invalid("Media state record kind")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);

        let mut invalid = Limits::HARD;
        invalid.max_formats = 0;
        assert!(invalid.to_extensions().is_err());

        let mut unknown = extensions;
        unknown.0.push(crate::codec::Extension {
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
    fn audio_output_and_video_device_formats_are_typed() {
        truncations(&OpenOutput {
            device_handle: 1,
            formats: vec![MediaFormat {
                codec: crate::schema::media::CODEC_OPUS as u16,
                channels: 2,
                sample_rate: 48_000,
                width: 0,
                height: 0,
                frame_rate_milli: 0,
                extensions: Extensions::default(),
            }],
            latency_target_ns: 20_000_000,
            target_bitrate_kbps: 64,
            extensions: Extensions::default(),
        });
        let invalid_bitrate = OpenOutput {
            device_handle: 1,
            formats: vec![MediaFormat {
                codec: crate::schema::media::CODEC_OPUS as u16,
                channels: 2,
                sample_rate: 48_000,
                width: 0,
                height: 0,
                frame_rate_milli: 0,
                extensions: Extensions::default(),
            }],
            latency_target_ns: 20_000_000,
            target_bitrate_kbps: crate::schema::media::MAX_OUTPUT_BITRATE_KBPS as u16 + 1,
            extensions: Extensions::default(),
        };
        assert!(invalid_bitrate.encode().is_err());
        let camera = MediaFormat {
            codec: crate::schema::media::CODEC_H264 as u16,
            channels: 0,
            sample_rate: 0,
            width: 1280,
            height: 720,
            frame_rate_milli: 30_000,
            extensions: Extensions::default(),
        };
        assert_eq!(
            MediaFormat::decode(&camera.encode().unwrap()).unwrap(),
            camera
        );
        for codec in [
            crate::schema::media::CODEC_H264_444 as u16,
            crate::schema::media::CODEC_AV1_444 as u16,
        ] {
            let mut format = camera.clone();
            format.codec = codec;
            truncations(&format);
        }
    }

    #[test]
    fn audio_timebase_converts_milliseconds_and_samples_exactly() {
        let samples = audio_sample_position_from_milliseconds(1_234, 48_000).unwrap();
        assert_eq!(samples, 59_232);
        assert_eq!(
            audio_milliseconds_from_sample_position(samples, 48_000).unwrap(),
            1_234
        );
        assert_eq!(
            audio_sample_position_from_milliseconds(1, 44_100).unwrap(),
            44
        );
        assert_eq!(
            audio_milliseconds_from_sample_position(44, 44_100).unwrap(),
            0
        );
        assert!(audio_sample_position_from_milliseconds(1, 0).is_err());
        assert!(audio_milliseconds_from_sample_position(1, 0).is_err());
        assert!(audio_sample_position_from_milliseconds(u64::MAX, u32::MAX).is_err());
        assert!(audio_milliseconds_from_sample_position(u64::MAX, 1).is_err());
    }

    #[test]
    fn frame_and_portal_are_bounded_and_truncation_safe() {
        let frame = MediaFrame {
            stream_handle: 1,
            sequence: 2,
            capture_time: 3,
            presentation_time: 4,
            codec_version: crate::schema::media::CODEC_H264 as u16,
            flags: crate::schema::media::FRAME_KEYFRAME as u16,
            fragment_index: 0,
            fragment_count: 1,
            complete_len: 3,
            payload: vec![1, 2, 3],
        };
        let bytes = frame.encode().unwrap();
        assert_eq!(MediaFrame::decode(&bytes).unwrap(), frame);
        let mut oversized = frame.clone();
        oversized.complete_len = crate::frame::HARD_MAX_BULK_CHUNK + 1;
        oversized.payload = vec![0; oversized.complete_len as usize];
        assert!(oversized.encode().is_err());
        // Once one payload byte is present a shorter prefix is a legal frame
        // fragment, not a truncation of this value.
        for end in 0..=44 {
            assert!(MediaFrame::decode(&bytes[..end]).is_err());
        }
        truncations(&PortalReply {
            portal_handle: 5,
            revision: 6,
            operation_id: [7; 16],
            kind: crate::schema::media::PORTAL_KIND_ACCESS as u16,
            decision: crate::schema::media::PORTAL_DECISION_GRANT as u8,
            metadata: PortalReplyMetadata::AccessGrant(AccessGrantMetadata {
                choices: vec![PortalChoiceValue {
                    id: "remember".into(),
                    value: "yes".into(),
                }],
            }),
            extensions: Extensions::default(),
        });
        truncations(&PortalRequest {
            portal_handle: 5,
            revision: 6,
            kind: crate::schema::media::PORTAL_KIND_ACCESS as u16,
            flags: 0,
            application_handle: 8,
            metadata: PortalRequestMetadata::Access(AccessRequestMetadata {
                deadline_server_ns: 10,
                parent_surface_handle: Some(9),
                app_id: "app".into(),
                title: "Permission".into(),
                subtitle: String::new(),
                body: "Allow?".into(),
                deny_label: "Deny".into(),
                grant_label: "Allow".into(),
                icon_name: "app".into(),
                choices: vec![PortalChoice {
                    id: "remember".into(),
                    label: "Remember".into(),
                    initial: "yes".into(),
                    options: vec![PortalChoiceValue {
                        id: "yes".into(),
                        value: "Yes".into(),
                    }],
                }],
            }),
            extensions: Extensions::default(),
        });
        truncations(&PortalRequest {
            portal_handle: 10,
            revision: 1,
            kind: crate::schema::media::PORTAL_KIND_SCREENCAST as u16,
            flags: 0,
            application_handle: 8,
            metadata: PortalRequestMetadata::ScreenCast(ScreenCastRequestMetadata {
                deadline_server_ns: 20,
                parent_surface_handle: None,
                app_id: "meet".into(),
                multiple: true,
                candidates: vec![ScreenCastCandidate {
                    surface_handle: 11,
                    width: 800,
                    height: 600,
                    title: "Window".into(),
                    app_id: "browser".into(),
                    thumbnail_hash: Some([3; 32]),
                }],
            }),
            extensions: Extensions::default(),
        });
        truncations(&PortalClose {
            portal_handle: 10,
            revision: 2,
            operation_id: [10; 16],
            extensions: Extensions::default(),
        });
        truncations(&PortalRecord {
            portal_handle: 10,
            revision: 2,
            kind: crate::schema::media::PORTAL_KIND_SCREENCAST as u16,
            state: crate::schema::media::PORTAL_GRANTED as u16,
            owner_session: [11; 16],
            metadata: PortalRecordMetadata::Grant(PortalGrantedMetadata::ScreenCast(
                ScreenCastGrantedMetadata {
                    streams: vec![ScreenCastGrantedStream {
                        surface_handle: 11,
                        stream_handle: 12,
                    }],
                },
            )),
            extensions: Extensions::default(),
        });
        assert!(
            PortalReply {
                portal_handle: 5,
                revision: 6,
                operation_id: [7; 16],
                kind: crate::schema::media::PORTAL_KIND_SCREENCAST as u16,
                decision: crate::schema::media::PORTAL_DECISION_DENY as u8,
                metadata: PortalReplyMetadata::ScreenCastGrant(ScreenCastGrantMetadata {
                    surface_handles: vec![11],
                }),
                extensions: Extensions::default(),
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn state_entity_discriminator_round_trips() {
        let value = CompleteEntity::Player(PlayerRecord {
            player_handle: 1,
            revision: 2,
            state: crate::schema::media::PLAYER_PLAYING as u16,
            flags: crate::schema::media::PLAYER_CAN_CONTROL as u16,
            position_us: 3,
            duration_us: 4,
            identity: "player".into(),
            desktop_entry: "player".into(),
            title: "title".into(),
            artist: "artist".into(),
            album: "album".into(),
            extensions: Extensions(vec![player_active_extension(true)]),
        });
        let record = value.state_record(RecordKind::Add).unwrap();
        let decoded = decode_state_record(&record).unwrap();
        assert_eq!(decoded, StateMutation::Complete(value));
        let StateMutation::Complete(CompleteEntity::Player(player)) = decoded else {
            panic!("expected player state");
        };
        assert_eq!(player.active().unwrap(), Some(true));
    }

    #[test]
    fn player_album_art_url_is_typed_and_exclusive_with_a_hash() {
        let mut player = PlayerRecord {
            player_handle: 1,
            revision: 2,
            state: crate::schema::media::PLAYER_PLAYING as u16,
            flags: crate::schema::media::PLAYER_CAN_CONTROL as u16,
            position_us: 3,
            duration_us: 4,
            identity: "player".into(),
            desktop_entry: "player".into(),
            title: "title".into(),
            artist: "artist".into(),
            album: "album".into(),
            extensions: Extensions(vec![crate::codec::Extension {
                tag: crate::schema::media::PLAYER_ALBUM_ART_URL_EXTENSION as u16,
                required: false,
                value: b"https://i.scdn.co/image/cover".to_vec(),
            }]),
        };
        assert_eq!(
            player.album_art_url().unwrap(),
            Some("https://i.scdn.co/image/cover")
        );
        assert!(PlayerRecord::decode(&player.encode().unwrap()).is_ok());

        player.extensions.0.push(crate::codec::Extension {
            tag: crate::schema::media::PLAYER_ALBUM_ART_HASH_EXTENSION as u16,
            required: false,
            value: vec![0; 32],
        });
        assert!(player.encode().is_err());
    }
}
