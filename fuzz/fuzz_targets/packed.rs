#![no_main]

use libfuzzer_sys::fuzz_target;
use yas_wire::Decode;

fuzz_target!(|input: &[u8]| {
    let Some((&selector, rest)) = input.split_first() else {
        return;
    };
    let Some((&parameters, payload)) = rest.split_first() else {
        return;
    };
    let channels = parameters.max(1);
    match selector % 14 {
        0 => {
            drop(yas_wire::events::EventBatch::decode(payload));
        }
        1 => {
            let flags = u16::from(parameters) & yas_wire::terminal::TerminalFrame::KNOWN_FLAGS;
            drop(yas_wire::terminal::Grid::decode_codec1(
                flags,
                payload,
                4 * 1024 * 1024,
                Some((24, 80)),
            ));
        }
        2 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_H264,
            payload,
            channels,
        )),
        3 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_H264_444,
            payload,
            channels,
        )),
        4 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_AV1,
            payload,
            channels,
        )),
        5 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_AV1_444,
            payload,
            channels,
        )),
        6 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_VP9,
            payload,
            channels,
        )),
        7 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_MJPEG,
            payload,
            channels,
        )),
        8 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_OPUS,
            payload,
            channels,
        )),
        9 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_PCM_F32LE,
            payload,
            channels,
        )),
        10 => drop(yas_wire::packed::validate_media(
            yas_wire::schema::packed_codec::MEDIA_CODEC_PCM_S16LE,
            payload,
            channels,
        )),
        11 => drop(yas_wire::packed::decode_surface(
            yas_wire::schema::packed_codec::SURFACE_CODEC_H264_V1,
            payload,
        )),
        12 => drop(yas_wire::packed::decode_surface(
            yas_wire::schema::packed_codec::SURFACE_CODEC_AV1_V1,
            payload,
        )),
        13 => drop(yas_wire::packed::decode_surface(
            yas_wire::schema::packed_codec::SURFACE_CODEC_PNG_V1,
            payload,
        )),
        _ => unreachable!(),
    }
});
