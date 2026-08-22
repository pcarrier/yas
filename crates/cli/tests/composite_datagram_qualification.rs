#![cfg(unix)]

use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use yas_composite_transport::{
    Offer, Role, RouteDatagramError, RoutedDatagramRoutes, encode_routed_datagram,
};
use yas_wire::core::{
    ClientHello, FamilyOffer, Ping, ReceiveLimits, ResultPrefix, ServerHello, Status,
};
use yas_wire::frame::DatagramContext;
use yas_wire::net::{
    Address, Datagram, DatagramDelivery, DeliveryPreference, DropPolicy, Endpoint, Open,
};
use yas_wire::{
    Class, Decode, Encode, Extensions, Frame, FrameCodec, FrameHeader, FrameLimits, family,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(10);
const DATAGRAM_MAXIMUM: u32 = 1_200;
const ROUTE_QUEUE: usize = 4;
const TOKEN: yas_composite_transport::Token = [0x5a; yas_composite_transport::TOKEN_BYTES];

struct TestServer {
    child: Child,
    socket: PathBuf,
    _directory: TempDir,
}

impl TestServer {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("yas.sock");
        let state = directory.path().join("state");
        let cache = directory.path().join("cache");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_yas"))
            .args([
                "server",
                "--name",
                "composite-datagram-qualification",
                "--socket",
            ])
            .arg(&socket)
            .args(["--no-processes", "--no-persistent-extensions"])
            .env("YAS_SKIP_COMPOSITOR", "1")
            .env("YAS_AUDIO", "0")
            .env("YAS_FONTS", "0")
            .env("YAS_RELAY", "0")
            .env("YAS_EXT", "0")
            .env("YAS_CHANNEL", "0")
            .env("XDG_STATE_HOME", &state)
            .env("XDG_CACHE_HOME", &cache)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn isolated YAS server");
        let server = Self {
            child,
            socket,
            _directory: directory,
        };
        server.wait_ready().await;
        server
    }

    async fn wait_ready(&self) {
        let deadline = Instant::now() + TEST_TIMEOUT;
        while !self.socket.exists() {
            assert!(
                Instant::now() < deadline,
                "YAS server socket did not appear"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn connect(&self) -> UnixStream {
        let deadline = Instant::now() + TEST_TIMEOUT;
        loop {
            match UnixStream::connect(&self.socket).await {
                Ok(stream) => return stream,
                Err(error) => {
                    assert!(
                        Instant::now() < deadline,
                        "cannot connect to isolated YAS server: {error}"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn read_frame(
    reader: &mut (impl AsyncRead + Unpin),
    codec: &FrameCodec,
) -> io::Result<Frame> {
    let mut length = [0; 4];
    reader.read_exact(&mut length).await?;
    let length = u32::from_le_bytes(length) as usize;
    if length > codec.limits().max_wire_frame as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "YAS frame exceeds negotiated maximum",
        ));
    }
    let mut encoded = vec![0; length];
    reader.read_exact(&mut encoded).await?;
    codec
        .decode(&encoded)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), codec: &FrameCodec, frame: &Frame) {
    writer
        .write_all(&codec.encode_stream(frame).unwrap())
        .await
        .unwrap();
}

async fn handshake(main: &mut UnixStream) -> (FrameCodec, ServerHello) {
    main.write_all(&yas_wire::PREFACE).await.unwrap();
    let hello = ClientHello {
        min_minor: 1,
        max_minor: 1,
        receive: ReceiveLimits {
            max_frame: yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME,
            max_decoded: yas_wire::schema::transport::RECOMMENDED_DECODED_FRAME,
            max_datagram: DATAGRAM_MAXIMUM,
            max_buffered: 4 * 1024 * 1024,
        },
        client_instance: [0x41; 16],
        client_name: "composite-datagram-qualification".to_owned(),
        client_release: "1".to_owned(),
        families: vec![
            FamilyOffer {
                family_id: family::TRANSFER,
                versions: vec![1],
                required: true,
            },
            FamilyOffer {
                family_id: family::NET,
                versions: vec![1],
                required: true,
            },
        ],
        codecs: Vec::new(),
        extensions: Extensions::default(),
    };
    let request = Frame {
        header: FrameHeader::request(family::CORE, yas_wire::core::request_kind::HELLO, 1),
        payload: hello.encode().unwrap(),
    };
    let pre_hello = FrameCodec::pre_hello();
    write_frame(main, &pre_hello, &request).await;
    let response = read_frame(main, &pre_hello).await.unwrap();
    assert_eq!(
        response.header,
        FrameHeader::result(family::CORE, yas_wire::core::request_kind::HELLO, 1,)
    );
    let result = ResultPrefix::decode(&response.payload).unwrap();
    assert_eq!(result.status, Status::Ok);
    let server_hello = ServerHello::decode(&result.body).unwrap();
    server_hello.validate_for_client(&hello).unwrap();
    let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
    (codec, server_hello)
}

fn spawn_reliable_reader(
    mut reader: tokio::net::unix::OwnedReadHalf,
    codec: FrameCodec,
) -> mpsc::Receiver<Frame> {
    let (sender, receiver) = mpsc::channel(64);
    tokio::spawn(async move {
        while let Ok(frame) = read_frame(&mut reader, &codec).await {
            if sender.send(frame).await.is_err() {
                break;
            }
        }
    });
    receiver
}

fn spawn_sideband_reader(
    mut reader: tokio::net::unix::OwnedReadHalf,
    codec: FrameCodec,
) -> mpsc::Receiver<Frame> {
    let (sender, receiver) = mpsc::channel(64);
    tokio::spawn(async move {
        while let Ok(bytes) =
            yas_composite_transport::read_datagram(&mut reader, DATAGRAM_MAXIMUM).await
        {
            let Ok(frame) =
                codec.decode_datagram(&bytes, DATAGRAM_MAXIMUM, DatagramContext::NetNativeFlow)
            else {
                continue;
            };
            if sender.send(frame).await.is_err() {
                break;
            }
        }
    });
    receiver
}

async fn next_matching(
    frames: &mut mpsc::Receiver<Frame>,
    predicate: impl Fn(&Frame) -> bool,
) -> Frame {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let frame = frames.recv().await.expect("YAS frame reader closed");
            if predicate(&frame) {
                return frame;
            }
        }
    })
    .await
    .expect("timed out waiting for YAS frame")
}

fn net_event(flow_handle: u64, sequence: u64, payload: &[u8]) -> Frame {
    Frame {
        header: FrameHeader {
            sensitive: true,
            ..FrameHeader::event(family::NET, yas_wire::schema::net::event::DATAGRAM)
        },
        payload: Datagram {
            flow_handle,
            sequence,
            payload: payload.to_vec(),
        }
        .encode()
        .unwrap(),
    }
}

fn route_event(
    routes: &RoutedDatagramRoutes,
    codec: &FrameCodec,
    frame: &Frame,
) -> Result<(), RouteDatagramError> {
    let event = codec
        .encode_datagram(frame, DATAGRAM_MAXIMUM, DatagramContext::NetNativeFlow)
        .unwrap();
    let routed = encode_routed_datagram(TOKEN, &event, DATAGRAM_MAXIMUM).unwrap();
    routes.route(&routed)
}

async fn recv_udp(socket: &tokio::net::UdpSocket) -> (Vec<u8>, std::net::SocketAddr) {
    let mut buffer = vec![0; 2_048];
    let (count, source) = tokio::time::timeout(TEST_TIMEOUT, socket.recv_from(&mut buffer))
        .await
        .expect("timed out waiting for relayed UDP datagram")
        .unwrap();
    buffer.truncate(count);
    (buffer, source)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uplink_composite_datagram_adversity_and_reliable_fallback() {
    let server = TestServer::start().await;
    let mut main = server.connect().await;
    let mut side = server.connect().await;
    let main_offer = Offer::new(Role::Main, TOKEN, DATAGRAM_MAXIMUM).unwrap();
    let side_offer = Offer::new(Role::Datagram, TOKEN, DATAGRAM_MAXIMUM).unwrap();
    yas_composite_transport::write_offer(&mut main, main_offer)
        .await
        .unwrap();
    yas_composite_transport::write_offer(&mut side, side_offer)
        .await
        .unwrap();
    let (codec, hello) = handshake(&mut main).await;
    assert!(hello.families.iter().any(|item| {
        item.family_id == family::NET
            && item.runtime_state == yas_wire::core::RuntimeState::Available
    }));

    let (main_reader, mut main_writer) = main.into_split();
    let (side_reader, mut side_writer) = side.into_split();
    let mut main_frames = spawn_reliable_reader(main_reader, codec.clone());
    let mut side_frames = spawn_sideband_reader(side_reader, codec.clone());

    let target = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let open = Open {
        operation_id: [0x31; 16],
        address: Address::Udp {
            host: target_address.ip().to_string(),
            port: target_address.port(),
        },
        delivery_preference: DeliveryPreference::PreferNative,
        drop_policy: DropPolicy::Oldest,
        initial_receive_credit: 0,
        early_data: Vec::new(),
        tls_options: None,
        extensions: Extensions::default(),
    };
    write_frame(
        &mut main_writer,
        &codec,
        &Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::request(family::NET, yas_wire::schema::net::request::OPEN, 2)
            },
            payload: open.encode().unwrap(),
        },
    )
    .await;
    let opened = next_matching(&mut main_frames, |frame| {
        frame.header
            == FrameHeader {
                sensitive: true,
                ..FrameHeader::result(family::NET, yas_wire::schema::net::request::OPEN, 2)
            }
    })
    .await;
    let opened = ResultPrefix::decode(&opened.payload).unwrap();
    assert_eq!(opened.status, Status::Ok);
    let endpoint = Endpoint::decode(&opened.body).unwrap();
    assert_eq!(endpoint.selected_delivery, DatagramDelivery::Native);

    // This is the exact bounded route used by `yas uplink`. Delay its writer
    // to deterministically fill it: sequence 0 is lost before the route,
    // sequence 2 precedes 1, sequence 1 is duplicated, and sequence 3 loses
    // to congestion after four accepted messages fill the route.
    let routes = RoutedDatagramRoutes::new(1);
    let mut route_receiver = routes
        .register(TOKEN, DATAGRAM_MAXIMUM, ROUTE_QUEUE)
        .unwrap();
    for (sequence, payload) in [
        (2, b"two".as_slice()),
        (1, b"one"),
        (1, b"one"),
        (4, b"four"),
    ] {
        route_event(
            &routes,
            &codec,
            &net_event(endpoint.flow_handle, sequence, payload),
        )
        .unwrap();
    }
    assert_eq!(
        route_event(
            &routes,
            &codec,
            &net_event(endpoint.flow_handle, 3, b"congested"),
        ),
        Err(RouteDatagramError::Congested)
    );
    let side_writer_task = tokio::spawn(async move {
        while let Some(frame) = route_receiver.recv().await {
            if yas_composite_transport::write_datagram(&mut side_writer, &frame, DATAGRAM_MAXIMUM)
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let mut source = None;
    for expected in [b"two".as_slice(), b"one", b"one", b"four"] {
        let (received, observed_source) = recv_udp(&target).await;
        assert_eq!(received, expected);
        source = Some(observed_source);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(100), async {
            let mut byte = [0];
            target.recv_from(&mut byte).await
        })
        .await
        .is_err(),
        "a deliberately lost or congested datagram reached the peer"
    );
    let source = source.unwrap();

    // Hostile or ineligible sideband messages are dropped without damaging
    // the authoritative link. The local generated predicate rejects Core,
    // and forcing it through the transport still leaves a reliable PING live.
    let forbidden = Frame {
        header: FrameHeader::request(family::CORE, yas_wire::core::request_kind::PING, 90),
        payload: Ping {
            sender_monotonic_ns: 90,
        }
        .encode()
        .unwrap(),
    };
    assert!(
        codec
            .encode_datagram(&forbidden, DATAGRAM_MAXIMUM, DatagramContext::NetNativeFlow,)
            .is_err()
    );
    let forced =
        encode_routed_datagram(TOKEN, &codec.encode(&forbidden).unwrap(), DATAGRAM_MAXIMUM)
            .unwrap();
    routes.route(&forced).unwrap();
    let malformed = encode_routed_datagram(TOKEN, b"not-a-frame", DATAGRAM_MAXIMUM).unwrap();
    routes.route(&malformed).unwrap();

    write_frame(
        &mut main_writer,
        &codec,
        &Frame {
            header: FrameHeader::request(family::CORE, yas_wire::core::request_kind::PING, 3),
            payload: Ping {
                sender_monotonic_ns: 3,
            }
            .encode()
            .unwrap(),
        },
    )
    .await;
    let ping = next_matching(&mut main_frames, |frame| {
        frame.header == FrameHeader::result(family::CORE, yas_wire::core::request_kind::PING, 3)
    })
    .await;
    assert_eq!(
        ResultPrefix::decode(&ping.payload).unwrap().status,
        Status::Ok
    );

    target.send_to(b"native-response", source).await.unwrap();
    let native_response = next_matching(&mut side_frames, |frame| {
        frame.header.family == family::NET
            && frame.header.class == Class::Event
            && frame.header.kind == yas_wire::schema::net::event::DATAGRAM
    })
    .await;
    assert_eq!(
        Datagram::decode(&native_response.payload).unwrap().payload,
        b"native-response"
    );

    // Lower the live Uplink path MTU below this otherwise valid Event. The
    // no-wait route refuses it, and the same complete Event succeeds through
    // the required reliable fallback. Raising the MTU re-enables the lane.
    routes.set_maximum(TOKEN, 80).unwrap();
    let mtu_event = net_event(endpoint.flow_handle, 5, &[b'm'; 128]);
    assert_eq!(
        route_event(&routes, &codec, &mtu_event),
        Err(RouteDatagramError::Oversized)
    );
    write_frame(&mut main_writer, &codec, &mtu_event).await;
    assert_eq!(recv_udp(&target).await.0, vec![b'm'; 128]);
    routes.set_maximum(TOKEN, DATAGRAM_MAXIMUM).unwrap();
    route_event(
        &routes,
        &codec,
        &net_event(endpoint.flow_handle, 6, b"mtu-restored"),
    )
    .unwrap();
    assert_eq!(recv_udp(&target).await.0, b"mtu-restored");

    // Closing only the optional sideband must retain the native Net flow and
    // move peer replies to the reliable stream. Retry a bounded number of
    // times to cover the intentional race between EOF and the sideband task.
    side_writer_task.abort();
    routes.remove(TOKEN);
    let mut reliable_fallback = false;
    for attempt in 0..20u8 {
        let outbound = [b'f', attempt];
        write_frame(
            &mut main_writer,
            &codec,
            &net_event(endpoint.flow_handle, 100 + u64::from(attempt), &outbound),
        )
        .await;
        let (_, current_source) = recv_udp(&target).await;
        let response = [b'r', attempt];
        target.send_to(&response, current_source).await.unwrap();
        let observed = tokio::time::timeout(Duration::from_millis(250), async {
            loop {
                tokio::select! {
                    frame = main_frames.recv() => {
                        let frame = frame.expect("reliable reader closed after sideband EOF");
                        if frame.header.family == family::NET
                            && frame.header.class == Class::Event
                            && frame.header.kind == yas_wire::schema::net::event::DATAGRAM
                        {
                            break Some((true, Datagram::decode(&frame.payload).unwrap()));
                        }
                    }
                    frame = side_frames.recv() => {
                        let Some(frame) = frame else { continue };
                        if frame.header.family == family::NET
                            && frame.header.class == Class::Event
                            && frame.header.kind == yas_wire::schema::net::event::DATAGRAM
                        {
                            break Some((false, Datagram::decode(&frame.payload).unwrap()));
                        }
                    }
                }
            }
        })
        .await
        .ok()
        .flatten();
        if let Some((reliable, datagram)) = observed {
            assert_eq!(datagram.payload, response);
            if reliable {
                reliable_fallback = true;
                break;
            }
        }
    }
    assert!(
        reliable_fallback,
        "closed composite sideband never fell back to reliable Net delivery"
    );

    write_frame(
        &mut main_writer,
        &codec,
        &Frame {
            header: FrameHeader::request(family::CORE, yas_wire::core::request_kind::PING, 4),
            payload: Ping {
                sender_monotonic_ns: 4,
            }
            .encode()
            .unwrap(),
        },
    )
    .await;
    let final_ping = next_matching(&mut main_frames, |frame| {
        frame.header == FrameHeader::result(family::CORE, yas_wire::core::request_kind::PING, 4)
    })
    .await;
    assert_eq!(
        ResultPrefix::decode(&final_ping.payload).unwrap().status,
        Status::Ok
    );
}
