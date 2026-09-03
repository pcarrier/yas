use std::io;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use yas_wire::core::{ClientHello, ReceiveLimits, Shutdown};
use yas_wire::{Encode, Extensions, Frame, FrameCodec, FrameHeader, FrameLimits, family};

pub(crate) async fn request_shutdown(previous: &mut (impl AsyncWrite + Unpin)) -> io::Result<()> {
    let mut client_instance = [0; 16];
    getrandom::fill(&mut client_instance).map_err(io::Error::other)?;
    if client_instance == [0; 16] {
        client_instance[15] = 1;
    }
    let hello = ClientHello {
        min_minor: 1,
        max_minor: 1,
        receive: ReceiveLimits::recommended(0),
        client_instance,
        client_name: "yas-server-replacement".to_owned(),
        client_release: env!("CARGO_PKG_VERSION").to_owned(),
        families: Vec::new(),
        codecs: Vec::new(),
        extensions: Extensions::default(),
    };
    let codec = FrameCodec::pre_hello();
    let hello = codec
        .encode_stream(&Frame {
            header: FrameHeader::request(family::CORE, yas_wire::core::request_kind::HELLO, 1),
            payload: hello.encode().map_err(io::Error::other)?,
        })
        .map_err(io::Error::other)?;
    let shutdown = Shutdown {
        operation_id: client_instance,
        grace_ns: 0,
        reason: "server replacement".to_owned(),
    };
    // SHUTDOWN follows HELLO and must use the session codec. No compression
    // codecs were offered, and this fixed-size request fits the Core limits.
    let codec = FrameCodec::new(FrameLimits::recommended(), []).map_err(io::Error::other)?;
    let shutdown = codec
        .encode_stream(&Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::request(family::CORE, yas_wire::core::request_kind::SHUTDOWN, 2)
            },
            payload: shutdown.encode().map_err(io::Error::other)?,
        })
        .map_err(io::Error::other)?;
    previous.write_all(&yas_wire::PREFACE).await?;
    previous.write_all(&hello).await?;
    previous.write_all(&shutdown).await
}
