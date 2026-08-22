//! YAS Surface family version 1 payload codecs.

use crate::prelude::*;

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, limit_u32, limit_u64,
    put_bytes_u16, put_bytes_u32, put_i32, put_i64, put_len_u16, put_string_u16, put_string_u32,
    put_u16, put_u32, put_u64, read_limit_u32, read_limit_u64, reject_unknown_required_extensions,
};
use crate::state::{Record, RecordKind};

pub const VERSION: u16 = crate::schema::surface::VERSION;

pub mod request_kind {
    pub use crate::schema::surface::request::*;
}

pub mod event_kind {
    pub use crate::schema::surface::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_surfaces_per_session: u32,
    pub max_views_per_session: u32,
    pub max_view_dimension: u32,
    pub max_view_pixels: u64,
    pub max_frame_rate: u32,
    pub max_inline_cursor_bytes: u32,
    pub max_remote_contacts: u32,
    pub max_app_endpoints_per_session: u32,
    pub max_app_endpoint_lifetime_ns: u64,
}

impl Limits {
    pub const HARD: Self = Self {
        max_surfaces_per_session: crate::schema::surface::MAX_SURFACES_PER_SESSION as u32,
        max_views_per_session: crate::schema::surface::MAX_VIEWS_PER_SESSION as u32,
        max_view_dimension: crate::schema::surface::MAX_VIEW_DIMENSION as u32,
        max_view_pixels: crate::schema::surface::MAX_VIEW_PIXELS,
        max_frame_rate: crate::schema::surface::MAX_FRAME_RATE as u32,
        max_inline_cursor_bytes: crate::schema::surface::MAX_INLINE_CURSOR_BYTES as u32,
        max_remote_contacts: crate::schema::surface::MAX_REMOTE_CONTACTS as u32,
        max_app_endpoints_per_session: crate::schema::surface::MAX_APP_ENDPOINTS_PER_SESSION as u32,
        max_app_endpoint_lifetime_ns: crate::schema::surface::MAX_APP_ENDPOINT_LIFETIME_NS,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid_u32 = |value: u32, maximum: u32| value != 0 && value <= maximum;
        if !valid_u32(self.max_surfaces_per_session, hard.max_surfaces_per_session)
            || !valid_u32(self.max_views_per_session, hard.max_views_per_session)
            || !valid_u32(self.max_view_dimension, hard.max_view_dimension)
            || self.max_view_pixels == 0
            || self.max_view_pixels > hard.max_view_pixels
            || !valid_u32(self.max_frame_rate, hard.max_frame_rate)
            || !valid_u32(self.max_inline_cursor_bytes, hard.max_inline_cursor_bytes)
            || !valid_u32(self.max_remote_contacts, hard.max_remote_contacts)
            || !valid_u32(
                self.max_app_endpoints_per_session,
                hard.max_app_endpoints_per_session,
            )
            || self.max_app_endpoint_lifetime_ns == 0
            || self.max_app_endpoint_lifetime_ns > hard.max_app_endpoint_lifetime_ns
        {
            return Err(Error::Invalid("Surface family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::surface::LIMIT_MAX_SURFACES_PER_SESSION,
                self.max_surfaces_per_session,
            ),
            limit_u32(
                crate::schema::surface::LIMIT_MAX_VIEWS_PER_SESSION,
                self.max_views_per_session,
            ),
            limit_u32(
                crate::schema::surface::LIMIT_MAX_VIEW_DIMENSION,
                self.max_view_dimension,
            ),
            limit_u64(
                crate::schema::surface::LIMIT_MAX_VIEW_PIXELS,
                self.max_view_pixels,
            ),
            limit_u32(
                crate::schema::surface::LIMIT_MAX_FRAME_RATE,
                self.max_frame_rate,
            ),
            limit_u32(
                crate::schema::surface::LIMIT_MAX_INLINE_CURSOR_BYTES,
                self.max_inline_cursor_bytes,
            ),
            limit_u32(
                crate::schema::surface::LIMIT_MAX_REMOTE_CONTACTS,
                self.max_remote_contacts,
            ),
            limit_u32(
                crate::schema::surface::LIMIT_MAX_APP_ENDPOINTS_PER_SESSION,
                self.max_app_endpoints_per_session,
            ),
            limit_u64(
                crate::schema::surface::LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS,
                self.max_app_endpoint_lifetime_ns,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::surface::LIMIT_MAX_SURFACES_PER_SESSION as u16,
                crate::schema::surface::LIMIT_MAX_VIEWS_PER_SESSION as u16,
                crate::schema::surface::LIMIT_MAX_VIEW_DIMENSION as u16,
                crate::schema::surface::LIMIT_MAX_VIEW_PIXELS as u16,
                crate::schema::surface::LIMIT_MAX_FRAME_RATE as u16,
                crate::schema::surface::LIMIT_MAX_INLINE_CURSOR_BYTES as u16,
                crate::schema::surface::LIMIT_MAX_REMOTE_CONTACTS as u16,
                crate::schema::surface::LIMIT_MAX_APP_ENDPOINTS_PER_SESSION as u16,
                crate::schema::surface::LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS as u16,
            ],
            "unknown required Surface family limit",
        )?;
        let value = Self {
            max_surfaces_per_session: read_limit_u32(
                extensions,
                crate::schema::surface::LIMIT_MAX_SURFACES_PER_SESSION,
            )?,
            max_views_per_session: read_limit_u32(
                extensions,
                crate::schema::surface::LIMIT_MAX_VIEWS_PER_SESSION,
            )?,
            max_view_dimension: read_limit_u32(
                extensions,
                crate::schema::surface::LIMIT_MAX_VIEW_DIMENSION,
            )?,
            max_view_pixels: read_limit_u64(
                extensions,
                crate::schema::surface::LIMIT_MAX_VIEW_PIXELS,
            )?,
            max_frame_rate: read_limit_u32(
                extensions,
                crate::schema::surface::LIMIT_MAX_FRAME_RATE,
            )?,
            max_inline_cursor_bytes: read_limit_u32(
                extensions,
                crate::schema::surface::LIMIT_MAX_INLINE_CURSOR_BYTES,
            )?,
            max_remote_contacts: read_limit_u32(
                extensions,
                crate::schema::surface::LIMIT_MAX_REMOTE_CONTACTS,
            )?,
            max_app_endpoints_per_session: read_limit_u32(
                extensions,
                crate::schema::surface::LIMIT_MAX_APP_ENDPOINTS_PER_SESSION,
            )?,
            max_app_endpoint_lifetime_ns: read_limit_u64(
                extensions,
                crate::schema::surface::LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn validate_view_geometry(width: u32, height: u32, max_fps: u16) -> Result<()> {
    if width == 0
        || height == 0
        || width > crate::schema::surface::MAX_VIEW_DIMENSION as u32
        || height > crate::schema::surface::MAX_VIEW_DIMENSION as u32
        || u64::from(width) * u64::from(height) > crate::schema::surface::MAX_VIEW_PIXELS
        || max_fps == 0
        || u32::from(max_fps) > crate::schema::surface::MAX_FRAME_RATE as u32
    {
        return Err(Error::Invalid("Surface view geometry"));
    }
    Ok(())
}

fn handle(value: u64, what: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

fn view(value: u32) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid("zero Surface view ID"))
    } else {
        Ok(())
    }
}

fn valid_codec(value: u16) -> bool {
    value == crate::schema::surface::CODEC_H264_V1 as u16
        || value == crate::schema::surface::CODEC_AV1_V1 as u16
        || value == crate::schema::surface::CODEC_PNG_V1 as u16
}

fn valid_key_code(value: u16) -> bool {
    matches!(value, 0x04..=0x31 | 0x33..=0x53 | 0xe0..=0xe7)
}

fn validate_key_state(value: u8) -> Result<()> {
    if value <= crate::schema::surface::KEY_STATE_REPEAT as u8 {
        Ok(())
    } else {
        Err(Error::Invalid("Surface key state"))
    }
}

fn validate_pointer(phase: u8, button: u8) -> Result<()> {
    let valid_phase = phase <= crate::schema::surface::POINTER_PHASE_LEAVE as u8;
    let valid_button = button <= crate::schema::surface::POINTER_BUTTON_FORWARD as u8;
    let button_phase = phase == crate::schema::surface::POINTER_PHASE_DOWN as u8
        || phase == crate::schema::surface::POINTER_PHASE_UP as u8;
    if valid_phase && valid_button && button_phase == (button != 0) {
        Ok(())
    } else {
        Err(Error::Invalid("Surface pointer phase or button"))
    }
}

fn extension(extensions: &Extensions, tag: u64) -> Option<&Extension> {
    extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceRecord {
    pub surface_handle: u64,
    pub revision: u64,
    pub parent_handle: u64,
    pub app_handle: u64,
    pub lifecycle: u8,
    pub composite_width: u32,
    pub composite_height: u32,
    pub logical_width_32_32: i64,
    pub logical_height_32_32: i64,
    pub application_id: String,
    pub title: String,
    pub extensions: Extensions,
}

impl SurfaceRecord {
    fn validate(&self) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        if self.revision == 0
            || self.composite_width == 0
            || self.composite_height == 0
            || self.logical_width_32_32 <= 0
            || self.logical_height_32_32 <= 0
        {
            return Err(Error::Invalid("Surface record geometry or revision"));
        }
        self.extensions.validate()?;
        for value in &self.extensions.0 {
            let known = value.tag
                == crate::schema::surface::STATE_ACTIVATION_REVISION_EXTENSION as u16
                || value.tag == crate::schema::surface::STATE_CURSOR_EXTENSION as u16
                || value.tag == crate::schema::surface::STATE_TEXT_INPUT_EXTENSION as u16;
            if value.required && !known {
                return Err(Error::Invalid("unknown required Surface state extension"));
            }
        }
        self.activation_revision()?;
        self.cursor_state()?;
        self.text_input_state()?;
        Ok(())
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Surface complete state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }

    pub fn activation_revision(&self) -> Result<Option<u64>> {
        extension(
            &self.extensions,
            crate::schema::surface::STATE_ACTIVATION_REVISION_EXTENSION,
        )
        .map(|extension| {
            let value = u64::from_le_bytes(
                extension
                    .value
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::Invalid("Surface activation revision extension"))?,
            );
            if value == 0 {
                return Err(Error::Invalid("zero Surface activation revision"));
            }
            Ok(value)
        })
        .transpose()
    }

    pub fn cursor_state(&self) -> Result<Option<CursorState>> {
        extension(
            &self.extensions,
            crate::schema::surface::STATE_CURSOR_EXTENSION,
        )
        .map(|extension| CursorState::decode(&extension.value))
        .transpose()
    }

    pub fn text_input_state(&self) -> Result<Option<TextInputState>> {
        extension(
            &self.extensions,
            crate::schema::surface::STATE_TEXT_INPUT_EXTENSION,
        )
        .map(|extension| TextInputState::decode(&extension.value))
        .transpose()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CursorState {
    Named(String),
    Hidden,
    Custom {
        hotspot_x: i32,
        hotspot_y: i32,
        width: u32,
        height: u32,
        scale_120: u16,
        png: Vec<u8>,
    },
}

impl CursorState {
    pub fn extension(&self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::surface::STATE_CURSOR_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for CursorState {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Named(name) => {
                if name.is_empty() {
                    return Err(Error::Invalid("empty Surface cursor name"));
                }
                out.push(crate::schema::surface::CURSOR_NAMED as u8);
                out.extend_from_slice(&[0; 3]);
                put_string_u16(out, name)?;
            }
            Self::Hidden => {
                out.push(crate::schema::surface::CURSOR_HIDDEN as u8);
                out.extend_from_slice(&[0; 3]);
            }
            Self::Custom {
                hotspot_x,
                hotspot_y,
                width,
                height,
                scale_120,
                png,
            } => {
                if *width == 0
                    || *height == 0
                    || *scale_120 == 0
                    || png.is_empty()
                    || png.len() > crate::schema::surface::MAX_INLINE_CURSOR_BYTES as usize
                {
                    return Err(Error::Invalid("Surface custom cursor"));
                }
                out.push(crate::schema::surface::CURSOR_CUSTOM as u8);
                out.extend_from_slice(&[0; 3]);
                put_i32(out, *hotspot_x);
                put_i32(out, *hotspot_y);
                put_u32(out, *width);
                put_u32(out, *height);
                put_u16(out, *scale_120);
                put_u16(out, 0);
                put_bytes_u32(out, png)?;
            }
        }
        Ok(())
    }
}

impl Decode for CursorState {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Surface cursor reserved bytes"));
        }
        let value = match kind {
            value if value == crate::schema::surface::CURSOR_NAMED as u8 => {
                Self::Named(decoder.string_u16()?)
            }
            value if value == crate::schema::surface::CURSOR_HIDDEN as u8 => Self::Hidden,
            value if value == crate::schema::surface::CURSOR_CUSTOM as u8 => {
                let hotspot_x = decoder.i32()?;
                let hotspot_y = decoder.i32()?;
                let width = decoder.u32()?;
                let height = decoder.u32()?;
                let scale_120 = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Surface custom cursor reserved field"));
                }
                Self::Custom {
                    hotspot_x,
                    hotspot_y,
                    width,
                    height,
                    scale_120,
                    png: decoder.len_bytes_u32()?.to_vec(),
                }
            }
            _ => return Err(Error::Invalid("Surface cursor kind")),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextInputState {
    pub enabled: bool,
    pub requested: bool,
    pub content_hint: u32,
    pub content_purpose: u32,
    pub cursor_rect: Option<CursorRect>,
}

impl TextInputState {
    pub fn extension(self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::surface::STATE_TEXT_INPUT_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for TextInputState {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.requested && !self.enabled {
            return Err(Error::Invalid(
                "Surface text input requested while disabled",
            ));
        }
        let flags = (u16::from(self.enabled) * crate::schema::surface::TEXT_INPUT_ENABLED as u16)
            | (u16::from(self.requested) * crate::schema::surface::TEXT_INPUT_REQUESTED as u16)
            | (u16::from(self.cursor_rect.is_some())
                * crate::schema::surface::TEXT_INPUT_HAS_CURSOR_RECT as u16);
        put_u16(out, flags);
        put_u16(out, 0);
        put_u32(out, self.content_hint);
        put_u32(out, self.content_purpose);
        if let Some(rect) = self.cursor_rect {
            if rect.width <= 0 || rect.height <= 0 {
                return Err(Error::Invalid("Surface text input cursor rectangle"));
            }
            put_i32(out, rect.x);
            put_i32(out, rect.y);
            put_i32(out, rect.width);
            put_i32(out, rect.height);
        }
        Ok(())
    }
}

impl Decode for TextInputState {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if flags & !(crate::schema::surface::TEXT_INPUT_FLAGS_MASK as u16) != 0
            || decoder.u16()? != 0
        {
            return Err(Error::Invalid("Surface text input flags or reserved field"));
        }
        let enabled = flags & crate::schema::surface::TEXT_INPUT_ENABLED as u16 != 0;
        let requested = flags & crate::schema::surface::TEXT_INPUT_REQUESTED as u16 != 0;
        let content_hint = decoder.u32()?;
        let content_purpose = decoder.u32()?;
        let cursor_rect = if flags & crate::schema::surface::TEXT_INPUT_HAS_CURSOR_RECT as u16 != 0
        {
            Some(CursorRect {
                x: decoder.i32()?,
                y: decoder.i32()?,
                width: decoder.i32()?,
                height: decoder.i32()?,
            })
        } else {
            None
        };
        decoder.finish()?;
        let value = Self {
            enabled,
            requested,
            content_hint,
            content_purpose,
            cursor_rect,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

impl Encode for SurfaceRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.surface_handle);
        put_u64(out, self.revision);
        put_u64(out, self.parent_handle);
        put_u64(out, self.app_handle);
        out.push(self.lifecycle);
        out.push(0);
        put_u32(out, self.composite_width);
        put_u32(out, self.composite_height);
        put_i64(out, self.logical_width_32_32);
        put_i64(out, self.logical_height_32_32);
        put_string_u16(out, &self.application_id)?;
        put_string_u16(out, &self.title)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for SurfaceRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let surface_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let parent_handle = decoder.u64()?;
        let app_handle = decoder.u64()?;
        let lifecycle = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Surface record reserved byte"));
        }
        let value = Self {
            surface_handle,
            revision,
            parent_handle,
            app_handle,
            lifecycle,
            composite_width: decoder.u32()?,
            composite_height: decoder.u32()?,
            logical_width_32_32: decoder.i64()?,
            logical_height_32_32: decoder.i64()?,
            application_id: decoder.string_u16()?,
            title: decoder.string_u16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfacePatch {
    pub surface_handle: u64,
    pub revision: u64,
    pub extensions: Extensions,
}

impl SurfacePatch {
    pub fn state_record(&self) -> Result<Record> {
        Ok(Record {
            kind: RecordKind::Patch,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for SurfacePatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        if self.revision == 0 {
            return Err(Error::Invalid("zero Surface revision"));
        }
        put_u64(out, self.surface_handle);
        put_u64(out, self.revision);
        self.extensions.encode_tail(out)
    }
}

impl Decode for SurfacePatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            surface_handle: decoder.u64()?,
            revision: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        handle(value.surface_handle, "zero surface handle")?;
        if value.revision == 0 {
            return Err(Error::Invalid("zero Surface revision"));
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedSurface {
    pub surface_handle: u64,
    pub revision: u64,
}

impl RemovedSurface {
    pub fn state_record(self) -> Result<Record> {
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for RemovedSurface {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        if self.revision == 0 {
            return Err(Error::Invalid("zero Surface revision"));
        }
        put_u64(out, self.surface_handle);
        put_u64(out, self.revision);
        Ok(())
    }
}

impl Decode for RemovedSurface {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            surface_handle: decoder.u64()?,
            revision: decoder.u64()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateAppEndpoint {
    pub operation_id: [u8; 16],
    pub application_id: String,
    pub extensions: Extensions,
}

impl Encode for CreateAppEndpoint {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.application_id.is_empty() {
            return Err(Error::Invalid("empty Surface application ID"));
        }
        out.extend_from_slice(&self.operation_id);
        put_string_u16(out, &self.application_id)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for CreateAppEndpoint {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            operation_id: decoder.array_16()?,
            application_id: decoder.string_u16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        if value.application_id.is_empty() {
            return Err(Error::Invalid("empty Surface application ID"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentOverride {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateAppEndpointResult {
    pub app_handle: u64,
    pub expires_server_ns: u64,
    pub environment: Vec<EnvironmentOverride>,
    pub extensions: Extensions,
}

impl Encode for CreateAppEndpointResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.app_handle, "zero app handle")?;
        if self.expires_server_ns == 0 {
            return Err(Error::Invalid("zero Surface app endpoint expiry"));
        }
        let mut keys = BTreeSet::new();
        put_u64(out, self.app_handle);
        put_u64(out, self.expires_server_ns);
        put_len_u16(out, self.environment.len())?;
        for entry in &self.environment {
            if entry.key.is_empty() || !keys.insert(entry.key.as_slice()) {
                return Err(Error::Invalid("Surface environment override key"));
            }
            put_bytes_u16(out, &entry.key)?;
            put_bytes_u32(out, &entry.value)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for CreateAppEndpointResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let app_handle = decoder.u64()?;
        let expires_server_ns = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if count > decoder.remaining() / 6 {
            return Err(Error::Invalid("Surface environment override count"));
        }
        let mut environment = Vec::with_capacity(count);
        for _ in 0..count {
            environment.push(EnvironmentOverride {
                key: decoder.len_bytes_u16()?.to_vec(),
                value: decoder.len_bytes_u32()?.to_vec(),
            });
        }
        let value = Self {
            app_handle,
            expires_server_ns,
            environment,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseAppEndpoint {
    pub app_handle: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for ReleaseAppEndpoint {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.app_handle, "zero app handle")?;
        put_u64(out, self.app_handle);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for ReleaseAppEndpoint {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            app_handle: decoder.u64()?,
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
pub struct OpenView {
    pub surface_handle: u64,
    pub width: u32,
    pub height: u32,
    pub max_fps: u16,
    pub decoder_capacity: u8,
    pub codec_versions: Vec<u16>,
    pub extensions: Extensions,
}

impl Encode for OpenView {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        validate_view_geometry(self.width, self.height, self.max_fps)?;
        if self.decoder_capacity == 0
            || self.codec_versions.is_empty()
            || self.codec_versions.len() > usize::from(u8::MAX)
            || self
                .codec_versions
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.codec_versions.iter().any(|value| !valid_codec(*value))
        {
            return Err(Error::Invalid("Surface OPEN_VIEW parameters"));
        }
        put_u64(out, self.surface_handle);
        put_u32(out, self.width);
        put_u32(out, self.height);
        put_u16(out, self.max_fps);
        out.push(self.decoder_capacity);
        out.push(self.codec_versions.len() as u8);
        for codec in &self.codec_versions {
            put_u16(out, *codec);
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for OpenView {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let surface_handle = decoder.u64()?;
        let width = decoder.u32()?;
        let height = decoder.u32()?;
        let max_fps = decoder.u16()?;
        let decoder_capacity = decoder.u8()?;
        let count = usize::from(decoder.u8()?);
        let mut codec_versions = Vec::with_capacity(count);
        for _ in 0..count {
            codec_versions.push(decoder.u16()?);
        }
        let value = Self {
            surface_handle,
            width,
            height,
            max_fps,
            decoder_capacity,
            codec_versions,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewResult {
    pub view_id: u32,
    pub codec_version: u16,
    pub max_inflight_frames: u16,
    pub max_encoded_frame: u32,
    pub max_decoded_frame: u32,
    pub first_sequence: u64,
    pub extensions: Extensions,
}

impl Encode for ViewResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        if !valid_codec(self.codec_version)
            || self.max_inflight_frames == 0
            || self.max_encoded_frame == 0
            || self.max_decoded_frame == 0
        {
            return Err(Error::Invalid("Surface view result limits"));
        }
        put_u32(out, self.view_id);
        put_u16(out, self.codec_version);
        put_u16(out, self.max_inflight_frames);
        put_u32(out, self.max_encoded_frame);
        put_u32(out, self.max_decoded_frame);
        put_u64(out, self.first_sequence);
        self.extensions.encode_tail(out)
    }
}

impl Decode for ViewResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            view_id: decoder.u32()?,
            codec_version: decoder.u16()?,
            max_inflight_frames: decoder.u16()?,
            max_encoded_frame: decoder.u32()?,
            max_decoded_frame: decoder.u32()?,
            first_sequence: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigureView {
    pub view_id: u32,
    pub width: u32,
    pub height: u32,
    pub max_fps: u16,
    pub decoder_capacity: u8,
    pub latency_target_ns: u64,
    pub extensions: Extensions,
}

impl Encode for ConfigureView {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        validate_view_geometry(self.width, self.height, self.max_fps)?;
        if self.decoder_capacity == 0 {
            return Err(Error::Invalid("Surface CONFIGURE_VIEW parameters"));
        }
        put_u32(out, self.view_id);
        put_u32(out, self.width);
        put_u32(out, self.height);
        put_u16(out, self.max_fps);
        out.push(self.decoder_capacity);
        out.push(0);
        put_u64(out, self.latency_target_ns);
        self.extensions.encode_tail(out)
    }
}

impl Decode for ConfigureView {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let width = decoder.u32()?;
        let height = decoder.u32()?;
        let max_fps = decoder.u16()?;
        let decoder_capacity = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Surface CONFIGURE_VIEW reserved byte"));
        }
        let value = Self {
            view_id,
            width,
            height,
            max_fps,
            decoder_capacity,
            latency_target_ns: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewRequest {
    pub view_id: u32,
}

impl Encode for ViewRequest {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        put_u32(out, self.view_id);
        Ok(())
    }
}

impl Decode for ViewRequest {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            view_id: decoder.u32()?,
        };
        decoder.finish()?;
        view(value.view_id)?;
        Ok(value)
    }
}

pub type ResetView = ViewRequest;
pub type CloseView = ViewRequest;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    pub surface_handle: u64,
    pub revision: u64,
    pub initial_receive_credit: u64,
    pub formats: Vec<u8>,
    pub extensions: Extensions,
}

impl Encode for Capture {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        if self.revision == 0
            || self.formats.is_empty()
            || self.formats.len() > usize::from(u8::MAX)
            || self.formats.iter().any(|format| {
                *format != crate::schema::surface::CAPTURE_PNG as u8
                    && *format != crate::schema::surface::CAPTURE_AVIF as u8
            })
            || {
                let mut unique = BTreeSet::new();
                self.formats.iter().any(|format| !unique.insert(*format))
            }
        {
            return Err(Error::Invalid("Surface CAPTURE parameters"));
        }
        put_u64(out, self.surface_handle);
        put_u64(out, self.revision);
        put_u64(out, self.initial_receive_credit);
        out.push(self.formats.len() as u8);
        out.extend_from_slice(&[0; 3]);
        out.extend_from_slice(&self.formats);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Capture {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let surface_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let initial_receive_credit = decoder.u64()?;
        let count = usize::from(decoder.u8()?);
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Surface CAPTURE reserved bytes"));
        }
        let value = Self {
            surface_handle,
            revision,
            initial_receive_credit,
            formats: decoder.take(count)?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

pub type CaptureResult = crate::transfer::InlineOrTransfer;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resize {
    pub surface_handle: u64,
    pub operation_id: [u8; 16],
    pub logical_width_32_32: i64,
    pub logical_height_32_32: i64,
    pub extensions: Extensions,
}

impl Resize {
    pub fn scale_120(&self) -> Result<Option<u16>> {
        extension(
            &self.extensions,
            crate::schema::surface::RESIZE_SCALE_120_EXTENSION,
        )
        .map(|extension| {
            let value = u16::from_le_bytes(
                extension
                    .value
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::Invalid("Surface RESIZE scale extension"))?,
            );
            if value == 0 {
                return Err(Error::Invalid("zero Surface RESIZE scale"));
            }
            Ok(value)
        })
        .transpose()
    }
}

impl Encode for Resize {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        let releases_claim = self.logical_width_32_32 == 0 && self.logical_height_32_32 == 0;
        if !releases_claim && (self.logical_width_32_32 <= 0 || self.logical_height_32_32 <= 0) {
            return Err(Error::Invalid("Surface RESIZE dimensions"));
        }
        put_u64(out, self.surface_handle);
        out.extend_from_slice(&self.operation_id);
        put_i64(out, self.logical_width_32_32);
        put_i64(out, self.logical_height_32_32);
        self.scale_120()?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Resize {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            surface_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            logical_width_32_32: decoder.i64()?,
            logical_height_32_32: decoder.i64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Focus {
    pub surface_handle: u64,
    pub operation_id: [u8; 16],
    pub focused: bool,
    pub extensions: Extensions,
}

impl Encode for Focus {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        put_u64(out, self.surface_handle);
        out.extend_from_slice(&self.operation_id);
        out.push(u8::from(self.focused));
        out.extend_from_slice(&[0; 7]);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Focus {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let surface_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let focused = match decoder.u8()? {
            0 => false,
            1 => true,
            _ => return Err(Error::Invalid("Surface FOCUS boolean")),
        };
        if decoder.take(7)? != [0; 7] {
            return Err(Error::Invalid("Surface FOCUS reserved bytes"));
        }
        let value = Self {
            surface_handle,
            operation_id,
            focused,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        handle(value.surface_handle, "zero surface handle")?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevisionResult {
    pub state_revision: u64,
}

impl Encode for RevisionResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.state_revision == 0 {
            return Err(Error::Invalid("zero Surface state revision"));
        }
        put_u64(out, self.state_revision);
        Ok(())
    }
}

impl Decode for RevisionResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            state_revision: decoder.u64()?,
        };
        decoder.finish()?;
        if value.state_revision == 0 {
            return Err(Error::Invalid("zero Surface state revision"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Close {
    pub surface_handle: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for Close {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        put_u64(out, self.surface_handle);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Close {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            surface_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        handle(value.surface_handle, "zero surface handle")?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameFeedback {
    pub presented_sequence: u64,
    pub decoder_queue_depth: u16,
    pub available_slots: u16,
}

impl FrameFeedback {
    fn encode_into(&self, out: &mut Vec<u8>) {
        put_u64(out, self.presented_sequence);
        put_u16(out, self.decoder_queue_depth);
        put_u16(out, self.available_slots);
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        Ok(Self {
            presented_sequence: decoder.u64()?,
            decoder_queue_depth: decoder.u16()?,
            available_slots: decoder.u16()?,
        })
    }
}

impl Encode for FrameFeedback {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out);
        Ok(())
    }
}

impl Decode for FrameFeedback {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub view_id: u32,
    pub feedback: FrameFeedback,
    pub client_monotonic_ns: u64,
    pub key_code: u16,
    pub state: u8,
    pub modifiers: u32,
}

impl Encode for Key {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        if !valid_key_code(self.key_code)
            || self.modifiers & !(crate::schema::surface::MODIFIER_MASK as u32) != 0
        {
            return Err(Error::Invalid("Surface key code or modifiers"));
        }
        validate_key_state(self.state)?;
        put_u32(out, self.view_id);
        self.feedback.encode_into(out);
        put_u64(out, self.client_monotonic_ns);
        put_u16(out, self.key_code);
        out.push(self.state);
        out.push(0);
        put_u32(out, self.modifiers);
        Ok(())
    }
}

impl Decode for Key {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let feedback = FrameFeedback::decode_from(&mut decoder)?;
        let client_monotonic_ns = decoder.u64()?;
        let key_code = decoder.u16()?;
        let state = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Surface KEY reserved byte"));
        }
        let value = Self {
            view_id,
            feedback,
            client_monotonic_ns,
            key_code,
            state,
            modifiers: decoder.u32()?,
        };
        decoder.finish()?;
        view(value.view_id)?;
        if !valid_key_code(value.key_code)
            || value.modifiers & !(crate::schema::surface::MODIFIER_MASK as u32) != 0
        {
            return Err(Error::Invalid("Surface key code or modifiers"));
        }
        validate_key_state(value.state)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub view_id: u32,
    pub feedback: FrameFeedback,
    pub client_monotonic_ns: u64,
    pub text: String,
}

impl Encode for Text {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        put_u32(out, self.view_id);
        self.feedback.encode_into(out);
        put_u64(out, self.client_monotonic_ns);
        put_string_u32(out, &self.text)
    }
}

impl Decode for Text {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            view_id: decoder.u32()?,
            feedback: FrameFeedback::decode_from(&mut decoder)?,
            client_monotonic_ns: decoder.u64()?,
            text: decoder.string_u32()?,
        };
        decoder.finish()?;
        view(value.view_id)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preedit {
    pub view_id: u32,
    pub client_monotonic_ns: u64,
    pub selection_start: u32,
    pub selection_end: u32,
    pub cursor: u32,
    pub text: String,
}

impl Encode for Preedit {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        if self.selection_start > self.selection_end
            || self.selection_end > self.text.len() as u32
            || self.cursor > self.text.len() as u32
        {
            return Err(Error::Invalid("Surface PREEDIT range"));
        }
        put_u32(out, self.view_id);
        put_u64(out, self.client_monotonic_ns);
        put_u32(out, self.selection_start);
        put_u32(out, self.selection_end);
        put_u32(out, self.cursor);
        put_string_u32(out, &self.text)
    }
}

impl Decode for Preedit {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            view_id: decoder.u32()?,
            client_monotonic_ns: decoder.u64()?,
            selection_start: decoder.u32()?,
            selection_end: decoder.u32()?,
            cursor: decoder.u32()?,
            text: decoder.string_u32()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointer {
    pub view_id: u32,
    pub feedback: FrameFeedback,
    pub client_monotonic_ns: u64,
    pub phase: u8,
    pub button: u8,
    pub x_32_32: i64,
    pub y_32_32: i64,
}

impl Encode for Pointer {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        validate_pointer(self.phase, self.button)?;
        put_u32(out, self.view_id);
        self.feedback.encode_into(out);
        put_u64(out, self.client_monotonic_ns);
        out.push(self.phase);
        out.push(self.button);
        put_u16(out, 0);
        put_i64(out, self.x_32_32);
        put_i64(out, self.y_32_32);
        Ok(())
    }
}

impl Decode for Pointer {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let feedback = FrameFeedback::decode_from(&mut decoder)?;
        let client_monotonic_ns = decoder.u64()?;
        let phase = decoder.u8()?;
        let button = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Surface POINTER reserved field"));
        }
        let value = Self {
            view_id,
            feedback,
            client_monotonic_ns,
            phase,
            button,
            x_32_32: decoder.i64()?,
            y_32_32: decoder.i64()?,
        };
        decoder.finish()?;
        view(value.view_id)?;
        validate_pointer(value.phase, value.button)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axis {
    pub view_id: u32,
    pub feedback: FrameFeedback,
    pub client_monotonic_ns: u64,
    pub source: u8,
    pub flags: u8,
    pub dx_32_32: i64,
    pub dy_32_32: i64,
    pub steps_x: i32,
    pub steps_y: i32,
}

impl Encode for Axis {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        if self.source > crate::schema::surface::AXIS_SOURCE_WHEEL_TILT as u8
            || self.flags & !(crate::schema::surface::AXIS_FLAGS_MASK as u8) != 0
        {
            return Err(Error::Invalid("Surface axis source or flags"));
        }
        put_u32(out, self.view_id);
        self.feedback.encode_into(out);
        put_u64(out, self.client_monotonic_ns);
        out.push(self.source);
        out.push(self.flags);
        put_u16(out, 0);
        put_i64(out, self.dx_32_32);
        put_i64(out, self.dy_32_32);
        out.extend_from_slice(&self.steps_x.to_le_bytes());
        out.extend_from_slice(&self.steps_y.to_le_bytes());
        Ok(())
    }
}

impl Decode for Axis {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let feedback = FrameFeedback::decode_from(&mut decoder)?;
        let client_monotonic_ns = decoder.u64()?;
        let source = decoder.u8()?;
        let flags = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Surface AXIS reserved field"));
        }
        let value = Self {
            view_id,
            feedback,
            client_monotonic_ns,
            source,
            flags,
            dx_32_32: decoder.i64()?,
            dy_32_32: decoder.i64()?,
            steps_x: decoder.i32()?,
            steps_y: decoder.i32()?,
        };
        decoder.finish()?;
        view(value.view_id)?;
        if value.source > crate::schema::surface::AXIS_SOURCE_WHEEL_TILT as u8
            || value.flags & !(crate::schema::surface::AXIS_FLAGS_MASK as u8) != 0
        {
            return Err(Error::Invalid("Surface axis source or flags"));
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchContact {
    pub contact_id: u32,
    pub x_32_32: i64,
    pub y_32_32: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touch {
    pub view_id: u32,
    pub client_monotonic_ns: u64,
    pub phase: u8,
    pub contacts: Vec<TouchContact>,
}

impl Encode for Touch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        let may_be_empty = self.phase == crate::schema::surface::TOUCH_PHASE_CANCEL as u8
            || self.phase == crate::schema::surface::TOUCH_PHASE_FRAME as u8;
        if self.phase > crate::schema::surface::TOUCH_PHASE_FRAME as u8
            || (!may_be_empty && self.contacts.is_empty())
            || self.contacts.len() > usize::from(u16::MAX)
        {
            return Err(Error::Invalid("Surface TOUCH contacts"));
        }
        let mut ids = BTreeSet::new();
        put_u32(out, self.view_id);
        put_u64(out, self.client_monotonic_ns);
        out.push(self.phase);
        out.push(0);
        put_len_u16(out, self.contacts.len())?;
        for contact in &self.contacts {
            if !ids.insert(contact.contact_id) {
                return Err(Error::Invalid("duplicate Surface touch contact"));
            }
            put_u32(out, contact.contact_id);
            put_i64(out, contact.x_32_32);
            put_i64(out, contact.y_32_32);
        }
        Ok(())
    }
}

impl Decode for Touch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let client_monotonic_ns = decoder.u64()?;
        let phase = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Surface TOUCH reserved byte"));
        }
        let count = usize::from(decoder.u16()?);
        let may_be_empty = phase == crate::schema::surface::TOUCH_PHASE_CANCEL as u8
            || phase == crate::schema::surface::TOUCH_PHASE_FRAME as u8;
        if phase > crate::schema::surface::TOUCH_PHASE_FRAME as u8
            || (!may_be_empty && count == 0)
            || count > decoder.remaining() / 20
        {
            return Err(Error::Invalid("Surface TOUCH contact count"));
        }
        let mut contacts = Vec::with_capacity(count);
        for _ in 0..count {
            contacts.push(TouchContact {
                contact_id: decoder.u32()?,
                x_32_32: decoder.i64()?,
                y_32_32: decoder.i64()?,
            });
        }
        decoder.finish()?;
        let value = Self {
            view_id,
            client_monotonic_ns,
            phase,
            contacts,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameAck {
    pub view_id: u32,
    pub feedback: FrameFeedback,
}

impl Encode for FrameAck {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        put_u32(out, self.view_id);
        self.feedback.encode_into(out);
        Ok(())
    }
}

impl Decode for FrameAck {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            view_id: decoder.u32()?,
            feedback: FrameFeedback::decode_from(&mut decoder)?,
        };
        decoder.finish()?;
        view(value.view_id)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceFrame {
    pub view_id: u32,
    pub sequence: u64,
    pub base_sequence: u64,
    pub capture_ns: u64,
    pub presentation_ns: u64,
    pub flags: u16,
    pub codec_version: u16,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub complete_len: u32,
    pub payload: Vec<u8>,
}

impl SurfaceFrame {
    pub fn datagram_eligible(&self) -> bool {
        let forbidden = crate::schema::surface::FRAME_KEYFRAME as u16
            | crate::schema::surface::FRAME_CODEC_CONFIG as u16
            | crate::schema::surface::FRAME_END_OF_STREAM as u16;
        self.flags & crate::schema::surface::FRAME_DATAGRAM_ELIGIBLE as u16 != 0
            && self.flags & crate::schema::surface::FRAME_DISCARDABLE as u16 != 0
            && self.flags & forbidden == 0
    }
}

impl Encode for SurfaceFrame {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        view(self.view_id)?;
        if !valid_codec(self.codec_version)
            || self.flags & !(crate::schema::surface::FRAME_FLAGS_MASK as u16) != 0
            || self.fragment_count == 0
            || self.fragment_index >= self.fragment_count
            || self.complete_len == 0
            || self.payload.is_empty()
            || self.payload.len() > self.complete_len as usize
            || self.payload.len() > crate::frame::HARD_MAX_BULK_CHUNK as usize
        {
            return Err(Error::Invalid("Surface FRAME fragments"));
        }
        put_u32(out, self.view_id);
        put_u64(out, self.sequence);
        put_u64(out, self.base_sequence);
        put_u64(out, self.capture_ns);
        put_u64(out, self.presentation_ns);
        put_u16(out, self.flags);
        put_u16(out, self.codec_version);
        put_u16(out, self.fragment_index);
        put_u16(out, self.fragment_count);
        put_u32(out, self.complete_len);
        out.extend_from_slice(&self.payload);
        Ok(())
    }
}

impl Decode for SurfaceFrame {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            view_id: decoder.u32()?,
            sequence: decoder.u64()?,
            base_sequence: decoder.u64()?,
            capture_ns: decoder.u64()?,
            presentation_ns: decoder.u64()?,
            flags: decoder.u16()?,
            codec_version: decoder.u16()?,
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
pub struct RemoteContact {
    pub contact_id: u32,
    pub x_32_32: i64,
    pub y_32_32: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RemoteInputKind {
    Pointer = crate::schema::surface::REMOTE_INPUT_POINTER as u8,
    Touch = crate::schema::surface::REMOTE_INPUT_TOUCH as u8,
}

impl TryFrom<u8> for RemoteInputKind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Pointer as u8 => Ok(Self::Pointer),
            value if value == Self::Touch as u8 => Ok(Self::Touch),
            _ => Err(Error::Invalid("Surface remote input kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteInput {
    pub surface_handle: u64,
    pub seat_handle: u64,
    pub expires_server_ns: u64,
    pub input_kind: RemoteInputKind,
    pub contacts: Vec<RemoteContact>,
}

impl Encode for RemoteInput {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.surface_handle, "zero surface handle")?;
        handle(self.seat_handle, "zero Surface seat handle")?;
        if self.contacts.len() > crate::schema::surface::MAX_REMOTE_CONTACTS as usize {
            return Err(Error::LimitExceeded {
                limit: "Surface remote contacts",
                actual: self.contacts.len() as u64,
                maximum: crate::schema::surface::MAX_REMOTE_CONTACTS,
            });
        }
        if matches!(self.input_kind, RemoteInputKind::Pointer)
            && (self.contacts.len() != 1 || self.contacts[0].contact_id != 0)
        {
            return Err(Error::Invalid("Surface remote pointer contact"));
        }
        let mut ids = BTreeSet::new();
        put_u64(out, self.surface_handle);
        put_u64(out, self.seat_handle);
        put_u64(out, self.expires_server_ns);
        out.push(self.input_kind as u8);
        out.push(0);
        put_len_u16(out, self.contacts.len())?;
        for contact in &self.contacts {
            if !ids.insert(contact.contact_id) {
                return Err(Error::Invalid("duplicate Surface remote contact"));
            }
            put_u32(out, contact.contact_id);
            put_i64(out, contact.x_32_32);
            put_i64(out, contact.y_32_32);
        }
        Ok(())
    }
}

impl Decode for RemoteInput {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let surface_handle = decoder.u64()?;
        let seat_handle = decoder.u64()?;
        let expires_server_ns = decoder.u64()?;
        let input_kind = RemoteInputKind::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Surface REMOTE_INPUT reserved byte"));
        }
        let count = usize::from(decoder.u16()?);
        if count > crate::schema::surface::MAX_REMOTE_CONTACTS as usize
            || count > decoder.remaining() / 20
        {
            return Err(Error::Invalid("Surface REMOTE_INPUT count or reserved"));
        }
        let mut contacts = Vec::with_capacity(count);
        for _ in 0..count {
            contacts.push(RemoteContact {
                contact_id: decoder.u32()?,
                x_32_32: decoder.i64()?,
                y_32_32: decoder.i64()?,
            });
        }
        let value = Self {
            surface_handle,
            seat_handle,
            expires_server_ns,
            input_kind,
            contacts,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

pub fn surface_from_state_record(record: &Record) -> Result<SurfaceRecord> {
    if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
        return Err(Error::Invalid("Surface complete state record kind"));
    }
    SurfaceRecord::decode(&record.body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_record_preserves_rounded_hidpi_composite_dimensions() {
        let record = SurfaceRecord {
            surface_handle: 1,
            revision: 1,
            parent_handle: 0,
            app_handle: 0,
            lifecycle: 0,
            composite_width: 1598,
            composite_height: 1198,
            logical_width_32_32: 800_i64 << 32,
            logical_height_32_32: 600_i64 << 32,
            application_id: "test.browser".to_owned(),
            title: "Browser".to_owned(),
            extensions: Extensions::default(),
        };

        assert_eq!(
            SurfaceRecord::decode(&record.encode().unwrap()).unwrap(),
            record
        );
    }

    #[test]
    fn resize_scale_extension_round_trips_and_rejects_zero() {
        let mut resize = Resize {
            surface_handle: 1,
            operation_id: [1; 16],
            logical_width_32_32: 800_i64 << 32,
            logical_height_32_32: 600_i64 << 32,
            extensions: Extensions(vec![Extension {
                tag: crate::schema::surface::RESIZE_SCALE_120_EXTENSION as u16,
                required: true,
                value: 240_u16.to_le_bytes().to_vec(),
            }]),
        };
        let decoded = Resize::decode(&resize.encode().unwrap()).unwrap();
        assert_eq!(decoded.scale_120().unwrap(), Some(240));

        resize.extensions.0[0].value = 0_u16.to_le_bytes().to_vec();
        assert!(resize.encode().is_err());
    }

    #[test]
    fn resize_zero_pair_releases_claim_but_mixed_zero_is_invalid() {
        let mut resize = Resize {
            surface_handle: 1,
            operation_id: [1; 16],
            logical_width_32_32: 0,
            logical_height_32_32: 0,
            extensions: Extensions::default(),
        };
        assert_eq!(Resize::decode(&resize.encode().unwrap()).unwrap(), resize);

        resize.logical_width_32_32 = 800_i64 << 32;
        assert!(resize.encode().is_err());
        resize.logical_width_32_32 = 0;
        resize.logical_height_32_32 = 600_i64 << 32;
        assert!(resize.encode().is_err());
    }

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);

        let mut invalid = Limits::HARD;
        invalid.max_view_pixels = 0;
        assert!(invalid.to_extensions().is_err());

        let mut unknown = extensions;
        unknown.0.push(Extension {
            tag: 99,
            required: true,
            value: Vec::new(),
        });
        assert!(Limits::from_extensions(&unknown).is_err());
    }

    #[test]
    fn app_endpoint_result_and_release_are_exact_and_truncation_safe() {
        let result = CreateAppEndpointResult {
            app_handle: 7,
            expires_server_ns: 9,
            environment: vec![EnvironmentOverride {
                key: b"WAYLAND_DISPLAY".to_vec(),
                value: b"wayland-yas".to_vec(),
            }],
            extensions: Extensions::default(),
        };
        let bytes = result.encode().unwrap();
        assert_eq!(CreateAppEndpointResult::decode(&bytes).unwrap(), result);
        for end in 0..bytes.len() {
            assert!(CreateAppEndpointResult::decode(&bytes[..end]).is_err());
        }

        let release = ReleaseAppEndpoint {
            app_handle: 7,
            operation_id: [3; 16],
            extensions: Extensions::default(),
        };
        let bytes = release.encode().unwrap();
        assert_eq!(ReleaseAppEndpoint::decode(&bytes).unwrap(), release);
        for end in 0..bytes.len() {
            assert!(ReleaseAppEndpoint::decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn open_view_golden_and_truncation() {
        let value = OpenView {
            surface_handle: 1,
            width: 1920,
            height: 1080,
            max_fps: 60,
            decoder_capacity: 3,
            codec_versions: vec![1, 2],
            extensions: Extensions::default(),
        };
        let bytes = value.encode().unwrap();
        assert_eq!(OpenView::decode(&bytes).unwrap(), value);
        for end in 0..bytes.len() {
            assert!(OpenView::decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn frame_and_touch_round_trip() {
        let frame = SurfaceFrame {
            view_id: 1,
            sequence: 2,
            base_sequence: 1,
            capture_ns: 3,
            presentation_ns: 4,
            flags: 0,
            codec_version: 1,
            fragment_index: 0,
            fragment_count: 1,
            complete_len: 3,
            payload: vec![1, 2, 3],
        };
        assert_eq!(
            SurfaceFrame::decode(&frame.encode().unwrap()).unwrap(),
            frame
        );
        let mut oversized = frame.clone();
        oversized.complete_len = crate::frame::HARD_MAX_BULK_CHUNK + 1;
        oversized.payload = vec![0; oversized.complete_len as usize];
        assert!(oversized.encode().is_err());
        let touch = Touch {
            view_id: 1,
            client_monotonic_ns: 2,
            phase: 0,
            contacts: vec![TouchContact {
                contact_id: 3,
                x_32_32: 4,
                y_32_32: 5,
            }],
        };
        assert_eq!(Touch::decode(&touch.encode().unwrap()).unwrap(), touch);
    }
}
