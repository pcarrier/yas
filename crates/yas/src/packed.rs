//! Validators for every packed codec registered by the canonical schema.
//!
//! These checks operate on one complete codec payload. They do not decode
//! video or audio into samples; they enforce the framing and canonicality
//! rules YAS owns before a payload reaches a codec implementation.

use crate::codec::Decoder;
use crate::prelude::*;
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceColorSpace {
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
    pub range: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfacePayload<'a> {
    pub color_space: Option<SurfaceColorSpace>,
    pub damage: Option<Vec<SurfaceDamageRect>>,
    pub bitstream: &'a [u8],
}

/// Validate one Media packed payload against its negotiated codec and channel
/// count.
pub fn validate_media(codec: u16, payload: &[u8], channels: u8) -> Result<()> {
    if channels == 0 {
        return Err(Error::Invalid("Media channel count"));
    }
    match codec {
        value if value == crate::schema::packed_codec::MEDIA_CODEC_PCM_S16LE => {
            require_sample_frames(payload, channels, 2)
        }
        value if value == crate::schema::packed_codec::MEDIA_CODEC_PCM_F32LE => {
            require_sample_frames(payload, channels, 4)?;
            for sample in payload.as_chunks::<4>().0 {
                let bits = u32::from_le_bytes(*sample);
                let value = f32::from_bits(bits);
                if !value.is_finite() || !(-1.0..=1.0).contains(&value) || bits == 0x8000_0000 {
                    return Err(Error::Invalid("canonical Media f32 sample"));
                }
            }
            Ok(())
        }
        value if value == crate::schema::packed_codec::MEDIA_CODEC_OPUS => validate_opus(payload),
        crate::schema::packed_codec::MEDIA_CODEC_H264
        | crate::schema::packed_codec::MEDIA_CODEC_H264_444 => require_h264(payload),
        crate::schema::packed_codec::MEDIA_CODEC_AV1
        | crate::schema::packed_codec::MEDIA_CODEC_AV1_444 => require_av1(payload),
        value if value == crate::schema::packed_codec::MEDIA_CODEC_VP9 => {
            if payload.is_empty() {
                Err(Error::Invalid("empty VP9 frame"))
            } else {
                Ok(())
            }
        }
        value if value == crate::schema::packed_codec::MEDIA_CODEC_MJPEG => {
            if payload.len() < 4
                || !payload.starts_with(&[0xff, 0xd8])
                || !payload.ends_with(&[0xff, 0xd9])
            {
                Err(Error::Invalid("MJPEG interchange datastream"))
            } else {
                Ok(())
            }
        }
        _ => Err(Error::UnsupportedCodec(codec)),
    }
}

/// Decode and validate the metadata envelope around one Surface codec
/// bitstream.
pub fn decode_surface(codec: u16, payload: &[u8]) -> Result<SurfacePayload<'_>> {
    if !matches!(
        codec,
        crate::schema::packed_codec::SURFACE_CODEC_H264_V1
            | crate::schema::packed_codec::SURFACE_CODEC_AV1_V1
            | crate::schema::packed_codec::SURFACE_CODEC_PNG_V1
    ) {
        return Err(Error::UnsupportedCodec(codec));
    }
    let mut decoder = Decoder::new(payload);
    let count = usize::from(decoder.u8()?);
    if decoder.take(3)? != [0, 0, 0] {
        return Err(Error::Invalid("Surface metadata reserved bytes"));
    }
    let mut previous = None;
    let mut color_space = None;
    let mut damage = None;
    for _ in 0..count {
        let tag = decoder.u16()?;
        let flags = decoder.u16()?;
        let length = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
        let mut body = Decoder::new(decoder.take(length)?);
        if previous.is_some_and(|old| old >= tag) || flags & !1 != 0 {
            return Err(Error::Invalid("Surface metadata order or flags"));
        }
        previous = Some(tag);
        if u64::from(tag)
            == crate::schema::packed_codec::surface_codec_h264_v1::METADATA_COLOR_SPACE
        {
            color_space = Some(SurfaceColorSpace {
                primaries: body.u8()?,
                transfer: body.u8()?,
                matrix: body.u8()?,
                range: body.u8()?,
            });
            body.finish()?;
        } else if u64::from(tag)
            == crate::schema::packed_codec::surface_codec_h264_v1::METADATA_DAMAGE
        {
            let rect_count = usize::from(body.u16()?);
            if body.u16()? != 0
                || rect_count
                    > crate::schema::packed_codec::surface_codec_h264_v1::MAX_DAMAGE_RECTS as usize
                || rect_count > body.remaining() / 16
            {
                return Err(Error::Invalid("Surface damage count"));
            }
            let mut rectangles = Vec::with_capacity(rect_count);
            for _ in 0..rect_count {
                let rectangle = SurfaceDamageRect {
                    x: body.u32()?,
                    y: body.u32()?,
                    width: body.u32()?,
                    height: body.u32()?,
                };
                if rectangle.width == 0 || rectangle.height == 0 {
                    return Err(Error::Invalid("empty Surface damage rectangle"));
                }
                rectangles.push(rectangle);
            }
            body.finish()?;
            damage = Some(rectangles);
        } else if flags & 1 != 0 {
            return Err(Error::Invalid("required Surface metadata"));
        }
    }
    let bitstream = decoder.rest();
    match codec {
        crate::schema::packed_codec::SURFACE_CODEC_H264_V1 => require_h264(bitstream)?,
        crate::schema::packed_codec::SURFACE_CODEC_AV1_V1 => require_av1(bitstream)?,
        crate::schema::packed_codec::SURFACE_CODEC_PNG_V1 => require_png(bitstream)?,
        _ => unreachable!(),
    }
    Ok(SurfacePayload {
        color_space,
        damage,
        bitstream,
    })
}

fn require_sample_frames(payload: &[u8], channels: u8, sample_bytes: usize) -> Result<()> {
    let frame_bytes = usize::from(channels)
        .checked_mul(sample_bytes)
        .ok_or(Error::LengthOverflow)?;
    if payload.is_empty() || !payload.len().is_multiple_of(frame_bytes) {
        Err(Error::Invalid("Media PCM sample frame"))
    } else {
        Ok(())
    }
}

fn validate_opus(payload: &[u8]) -> Result<()> {
    let mut decoder = Decoder::new(payload);
    let packet_count = usize::from(decoder.u16()?);
    if decoder.u16()? != 0 || packet_count == 0 {
        return Err(Error::Invalid("Opus packet count"));
    }
    for _ in 0..packet_count {
        let packet_len = usize::from(decoder.u16()?);
        if packet_len == 0
            || packet_len > crate::schema::packed_codec::media_codec_opus::MAX_PACKET_BYTES as usize
        {
            return Err(Error::Invalid("Opus packet length"));
        }
        decoder.take(packet_len)?;
    }
    decoder.finish()
}

fn require_h264(payload: &[u8]) -> Result<()> {
    if payload.starts_with(&[0, 0, 1]) || payload.starts_with(&[0, 0, 0, 1]) {
        Ok(())
    } else {
        Err(Error::Invalid("H.264 Annex-B access unit"))
    }
}

fn require_av1(payload: &[u8]) -> Result<()> {
    if payload.first().is_some_and(|byte| (byte >> 3) & 0x0f == 2) {
        Ok(())
    } else {
        Err(Error::Invalid("AV1 temporal delimiter"))
    }
}

fn require_png(payload: &[u8]) -> Result<()> {
    if payload.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Ok(())
    } else {
        Err(Error::Invalid("PNG signature"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Decode;

    fn hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let digit = |byte: u8| match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => panic!("invalid hex"),
                };
                digit(pair[0]) << 4 | digit(pair[1])
            })
            .collect()
    }

    #[test]
    fn every_registered_packed_codec_accepts_its_golden_payload() {
        for codec in crate::schema::CODECS {
            let payload = hex(codec.golden_hex);
            match codec.family {
                crate::family::EVENTS => {
                    crate::events::EventBatch::decode(&payload).unwrap();
                }
                crate::family::MEDIA => validate_media(codec.id, &payload, 1).unwrap(),
                crate::family::SURFACE => {
                    decode_surface(codec.id, &payload).unwrap();
                }
                crate::family::TERMINAL => {
                    let flags = crate::schema::terminal::FRAME_DIMENSIONS as u16
                        | crate::schema::terminal::FRAME_CURSOR as u16
                        | crate::schema::terminal::FRAME_MODES as u16
                        | crate::schema::terminal::FRAME_SCROLLBACK as u16
                        | crate::schema::terminal::FRAME_VIEW_OFFSET as u16
                        | crate::schema::terminal::FRAME_TITLE as u16;
                    crate::terminal::Grid::decode_codec1(flags, &payload, 4096, None).unwrap();
                }
                _ => panic!("unhandled packed codec {}", codec.name),
            }
        }
    }

    #[test]
    fn malformed_codec_envelopes_are_rejected() {
        assert!(validate_media(crate::schema::packed_codec::MEDIA_CODEC_OPUS, &[1, 0], 1).is_err());
        assert!(
            validate_media(
                crate::schema::packed_codec::MEDIA_CODEC_PCM_F32LE,
                &[0, 0, 0, 128],
                1
            )
            .is_err()
        );
        assert!(
            decode_surface(crate::schema::packed_codec::SURFACE_CODEC_PNG_V1, &[0; 12]).is_err()
        );
    }
}
