//! Validators for every packed codec registered by the canonical schema.
//!
//! These checks operate on one complete codec payload. They do not decode
//! video or audio into samples; they enforce the framing and canonicality
//! rules YAS owns before a payload reaches a codec implementation.

use crate::codec::{Decoder, put_len_u32, put_u16, put_u32};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfacePayload<'a> {
    pub color_space: Option<SurfaceColorSpace>,
    pub damage: Option<Vec<SurfaceDamageRect>>,
    pub dimensions: Option<SurfaceDimensions>,
    /// Full surface extent in logical pixels, independent of encoded resolution.
    pub logical_dimensions: Option<SurfaceDimensions>,
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
    let mut dimensions = None;
    let mut logical_dimensions = None;
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
        } else if u64::from(tag)
            == crate::schema::packed_codec::surface_codec_h264_v1::METADATA_DIMENSIONS
            || u64::from(tag)
                == crate::schema::packed_codec::surface_codec_h264_v1::METADATA_LOGICAL_DIMENSIONS
        {
            let value = SurfaceDimensions {
                width: body.u32()?,
                height: body.u32()?,
            };
            if value.width == 0 || value.height == 0 {
                return Err(Error::Invalid("empty Surface dimensions"));
            }
            body.finish()?;
            if u64::from(tag)
                == crate::schema::packed_codec::surface_codec_h264_v1::METADATA_DIMENSIONS
            {
                dimensions = Some(value);
            } else {
                logical_dimensions = Some(value);
            }
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
        dimensions,
        logical_dimensions,
        bitstream,
    })
}

/// Encode one Surface metadata envelope and validate its codec bitstream.
pub fn encode_surface(codec: u16, value: &SurfacePayload<'_>) -> Result<Vec<u8>> {
    match codec {
        crate::schema::packed_codec::SURFACE_CODEC_H264_V1 => require_h264(value.bitstream)?,
        crate::schema::packed_codec::SURFACE_CODEC_AV1_V1 => require_av1(value.bitstream)?,
        crate::schema::packed_codec::SURFACE_CODEC_PNG_V1 => require_png(value.bitstream)?,
        _ => return Err(Error::UnsupportedCodec(codec)),
    }

    let count = u8::from(value.color_space.is_some())
        + u8::from(value.damage.is_some())
        + u8::from(value.dimensions.is_some())
        + u8::from(value.logical_dimensions.is_some());
    let mut out = Vec::with_capacity(value.bitstream.len().saturating_add(64));
    out.push(count);
    out.extend_from_slice(&[0; 3]);

    if let Some(color_space) = value.color_space {
        put_u16(
            &mut out,
            crate::schema::packed_codec::surface_codec_h264_v1::METADATA_COLOR_SPACE as u16,
        );
        put_u16(&mut out, 0);
        put_u32(&mut out, 4);
        out.extend_from_slice(&[
            color_space.primaries,
            color_space.transfer,
            color_space.matrix,
            color_space.range,
        ]);
    }
    if let Some(damage) = &value.damage {
        if damage.len()
            > crate::schema::packed_codec::surface_codec_h264_v1::MAX_DAMAGE_RECTS as usize
        {
            return Err(Error::Invalid("Surface damage count"));
        }
        put_u16(
            &mut out,
            crate::schema::packed_codec::surface_codec_h264_v1::METADATA_DAMAGE as u16,
        );
        put_u16(&mut out, 0);
        let body_len = 4usize
            .checked_add(damage.len().checked_mul(16).ok_or(Error::LengthOverflow)?)
            .ok_or(Error::LengthOverflow)?;
        put_len_u32(&mut out, body_len)?;
        put_u16(
            &mut out,
            u16::try_from(damage.len()).map_err(|_| Error::LengthOverflow)?,
        );
        put_u16(&mut out, 0);
        for rectangle in damage {
            if rectangle.width == 0 || rectangle.height == 0 {
                return Err(Error::Invalid("empty Surface damage rectangle"));
            }
            put_u32(&mut out, rectangle.x);
            put_u32(&mut out, rectangle.y);
            put_u32(&mut out, rectangle.width);
            put_u32(&mut out, rectangle.height);
        }
    }
    for (tag, dimensions) in [
        (
            crate::schema::packed_codec::surface_codec_h264_v1::METADATA_DIMENSIONS,
            value.dimensions,
        ),
        (
            crate::schema::packed_codec::surface_codec_h264_v1::METADATA_LOGICAL_DIMENSIONS,
            value.logical_dimensions,
        ),
    ] {
        let Some(dimensions) = dimensions else {
            continue;
        };
        if dimensions.width == 0 || dimensions.height == 0 {
            return Err(Error::Invalid("empty Surface dimensions"));
        }
        put_u16(&mut out, tag as u16);
        put_u16(&mut out, 0);
        put_u32(&mut out, 8);
        put_u32(&mut out, dimensions.width);
        put_u32(&mut out, dimensions.height);
    }
    out.extend_from_slice(value.bitstream);
    Ok(out)
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

    #[test]
    fn surface_dimensions_round_trip() {
        let bitstream = [0, 0, 0, 1, 0x65, 0x88];
        let payload = encode_surface(
            crate::schema::packed_codec::SURFACE_CODEC_H264_V1,
            &SurfacePayload {
                color_space: None,
                damage: None,
                dimensions: Some(SurfaceDimensions {
                    width: 424,
                    height: 302,
                }),
                logical_dimensions: Some(SurfaceDimensions {
                    width: 400,
                    height: 300,
                }),
                bitstream: &bitstream,
            },
        )
        .unwrap();
        let decoded =
            decode_surface(crate::schema::packed_codec::SURFACE_CODEC_H264_V1, &payload).unwrap();
        assert_eq!(
            decoded.dimensions,
            Some(SurfaceDimensions {
                width: 424,
                height: 302,
            })
        );
        assert_eq!(decoded.bitstream, bitstream);
        assert_eq!(
            decoded.logical_dimensions,
            Some(SurfaceDimensions {
                width: 400,
                height: 300
            })
        );
    }

    #[test]
    fn empty_surface_dimensions_are_rejected() {
        let bitstream = [0, 0, 0, 1, 0x65, 0x88];
        assert!(
            encode_surface(
                crate::schema::packed_codec::SURFACE_CODEC_H264_V1,
                &SurfacePayload {
                    color_space: None,
                    damage: None,
                    dimensions: Some(SurfaceDimensions {
                        width: 0,
                        height: 302,
                    }),
                    logical_dimensions: None,
                    bitstream: &bitstream,
                },
            )
            .is_err()
        );
    }

    #[test]
    fn logical_dimensions_golden_vectors_and_validation() {
        for codec in crate::schema::CODECS
            .iter()
            .filter(|c| c.family == crate::family::SURFACE)
        {
            let name = alloc::format!("packed_codec.{}.logical_dimensions.payload", codec.name);
            let vector = crate::schema::GOLDEN_VECTORS
                .iter()
                .find(|v| v.name == name)
                .unwrap();
            let payload = hex(vector.hex);
            let decoded = decode_surface(codec.id, &payload).unwrap();
            assert_eq!(
                decoded.logical_dimensions,
                Some(SurfaceDimensions {
                    width: 400,
                    height: 300
                })
            );
            assert_eq!(encode_surface(codec.id, &decoded).unwrap(), payload);
            for end in 0..20 {
                assert!(decode_surface(codec.id, &payload[..end]).is_err());
            }
            let mut malformed = payload.clone();
            malformed[12..16].fill(0);
            assert!(decode_surface(codec.id, &malformed).is_err());
            malformed = payload.clone();
            malformed[8] = 9;
            assert!(decode_surface(codec.id, &malformed).is_err());
            let mut invalid = decoded;
            invalid.logical_dimensions.as_mut().unwrap().height = 0;
            assert!(encode_surface(codec.id, &invalid).is_err());
        }
    }
}
