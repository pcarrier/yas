#![no_main]

use libfuzzer_sys::fuzz_target;
use yas_wire::frame::{DatagramContext, FrameCodec, FrameLimits, LZ4_CODEC};

fuzz_target!(|input: &[u8]| {
    let codec = FrameCodec::new(FrameLimits::recommended(), [LZ4_CODEC]).unwrap();
    if let Ok(frame) = codec.decode(input) {
        let encoded = codec.encode(&frame).unwrap();
        assert_eq!(codec.decode(&encoded).unwrap(), frame);
    }
    if let Ok((frame, consumed)) = codec.decode_stream(input) {
        assert!(consumed <= input.len());
        let encoded = codec.encode_stream(&frame).unwrap();
        assert_eq!(codec.decode_stream(&encoded).unwrap().0, frame);
    }
    for context in [
        DatagramContext::NetNativeFlow,
        DatagramContext::SurfaceFrame,
        DatagramContext::MediaFrame,
    ] {
        if let Ok(frame) = codec.decode_datagram(input, 65_536, context) {
            let encoded = codec.encode_datagram(&frame, 65_536, context).unwrap();
            assert_eq!(
                codec.decode_datagram(&encoded, 65_536, context).unwrap(),
                frame
            );
        }
    }
});
