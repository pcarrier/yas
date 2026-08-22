#![no_main]

use libfuzzer_sys::fuzz_target;

include!(concat!(env!("OUT_DIR"), "/family_decoders.rs"));

fuzz_target!(|input: &[u8]| {
    let Some((&first, rest)) = input.split_first() else {
        return;
    };
    let Some((&second, payload)) = rest.split_first() else {
        return;
    };
    let selector = usize::from(u16::from_le_bytes([first, second]));
    DECODERS[selector % DECODERS.len()](payload);
});
