use super::*;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::sync::oneshot;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

struct Peer {
    session: web_transport_quinn::Session,
    send: web_transport_quinn::SendStream,
    recv: web_transport_quinn::RecvStream,
}

struct UdpRelay {
    blackhole: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for UdpRelay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn peers() -> (Peer, Peer, UdpRelay) {
    timeout(TEST_TIMEOUT, async {
        let (mut server, advertisement, _) = prepare_web_transport(Some(WebTransportOptions {
            addr: "127.0.0.1:0".into(),
            public_port: 0,
            certificate: None,
            private_key: None,
            pin_certificate: false,
        }))
        .unwrap()
        .unwrap();
        let server_addr = server.local_addr().unwrap();
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = socket.local_addr().unwrap();
        let blackhole = Arc::new(AtomicBool::new(false));
        let drop_packets = blackhole.clone();
        let relay = UdpRelay {
            blackhole,
            task: tokio::spawn(async move {
                let mut client_addr = None;
                let mut buffer = vec![0; 65_536];
                loop {
                    let (length, from) = socket.recv_from(&mut buffer).await.unwrap();
                    if drop_packets.load(Ordering::SeqCst) {
                        continue;
                    }
                    let destination = if from == server_addr {
                        client_addr.unwrap()
                    } else {
                        client_addr = Some(from);
                        server_addr
                    };
                    socket
                        .send_to(&buffer[..length], destination)
                        .await
                        .unwrap();
                }
            }),
        };
        let url: url::Url = format!("https://{relay_addr}/edge").parse().unwrap();
        let client = web_transport_quinn::ClientBuilder::new()
            .with_server_certificate_hashes(vec![advertisement.certificate_hash.unwrap().to_vec()])
            .unwrap();
        let (client, edge) = tokio::join!(
            async {
                let session = client.connect(url).await.unwrap();
                let (mut send, recv) = session.open_bi().await.unwrap();
                // A QUIC stream is not visible to the peer until it sends data.
                send.write_all(&[0]).await.unwrap();
                Peer {
                    session,
                    send,
                    recv,
                }
            },
            async {
                let session = server.accept().await.unwrap().ok().await.unwrap();
                let (send, mut recv) = session.accept_bi().await.unwrap();
                assert_eq!(recv.read_u8().await.unwrap(), 0);
                Peer {
                    session,
                    send,
                    recv,
                }
            },
        );
        (client, edge, relay)
    })
    .await
    .expect("loopback WebTransport handshake")
}

fn start_bridge(
    edge: Peer,
    composite: bool,
    capacity: usize,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::io::DuplexStream,
    tokio::io::DuplexStream,
) {
    let (main, home) = tokio::io::duplex(capacity);
    let (main_reader, main_writer) = tokio::io::split(main);
    let (datagram, home_datagram) = tokio::io::duplex(4096);
    let (datagram_reader, datagram_writer) = tokio::io::split(datagram);
    let bridge = tokio::spawn(async move {
        if composite {
            bridge_composite_web_transport(
                edge.session,
                edge.recv,
                edge.send,
                CompositeHome {
                    main_reader: Box::new(main_reader),
                    main_writer: Box::new(main_writer),
                    datagram_reader: Box::new(datagram_reader),
                    datagram_writer: Box::new(datagram_writer),
                },
                1200,
            )
            .await;
        } else {
            bridge_reliable_web_transport(
                edge.session,
                edge.recv,
                edge.send,
                Box::new(main_reader),
                Box::new(main_writer),
            )
            .await;
        }
    });
    (bridge, home, home_datagram)
}

struct FailedDatagramWriter(Option<oneshot::Sender<()>>);

impl AsyncWrite for FailedDatagramWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Some(failed) = self.0.take() {
            let _ = failed.send(());
        }
        Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn datagram_failure_preserves_reliable_forwarding_and_eof_cleanup() {
    let (mut client, edge, _relay) = peers().await;
    let (main, mut home) = tokio::io::duplex(4096);
    let (main_reader, main_writer) = tokio::io::split(main);
    let (failed, failure) = oneshot::channel();
    let bridge = tokio::spawn(bridge_composite_web_transport(
        edge.session,
        edge.recv,
        edge.send,
        CompositeHome {
            main_reader: Box::new(main_reader),
            main_writer: Box::new(main_writer),
            datagram_reader: Box::new(tokio::io::empty()),
            datagram_writer: Box::new(FailedDatagramWriter(Some(failed))),
        },
        1200,
    ));

    client
        .session
        .send_datagram(b"lost".to_vec().into())
        .unwrap();
    timeout(TEST_TIMEOUT, failure).await.unwrap().unwrap();

    // Both optional directions have ended. The authoritative stream must
    // still forward bytes in both directions and notice its eventual EOF.
    client.send.write_all(b"request").await.unwrap();
    let mut request = [0; 7];
    timeout(TEST_TIMEOUT, home.read_exact(&mut request))
        .await
        .expect("reliable input stalled after datagram failure")
        .unwrap();
    assert_eq!(&request, b"request");
    home.write_all(b"reply").await.unwrap();
    let mut reply = [0; 5];
    timeout(TEST_TIMEOUT, client.recv.read_exact(&mut reply))
        .await
        .expect("reliable output stalled after datagram failure")
        .unwrap();
    assert_eq!(&reply, b"reply");

    client.send.shutdown().await.unwrap();
    timeout(TEST_TIMEOUT, bridge)
        .await
        .expect("stream EOF did not stop the bridge")
        .unwrap();
    assert_eq!(
        home.read_u8().await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}

#[tokio::test]
async fn session_close_releases_backpressured_home_connections() {
    backpressured_disconnect(false).await;
}

#[tokio::test]
async fn silent_peer_timeout_releases_backpressured_home_connections() {
    backpressured_disconnect(true).await;
}

async fn backpressured_disconnect(blackhole: bool) {
    for composite in [false, true] {
        let (mut client, edge, relay) = peers().await;
        let (bridge, mut home, mut home_datagram) = start_bridge(edge, composite, 1);

        // Fill the copy buffer as well as the home buffer, so copy cannot
        // read ahead and discover EOF/the connection error while blocked.
        client.send.write_all(&vec![b'b'; 64 * 1024]).await.unwrap();
        client.send.finish().unwrap();
        timeout(TEST_TIMEOUT, client.send.stopped())
            .await
            .expect("peer did not acknowledge buffered stream data")
            .unwrap();
        assert_eq!(
            timeout(TEST_TIMEOUT, home.read_u8())
                .await
                .unwrap()
                .unwrap(),
            b'b'
        );
        // Leave the one-byte home buffer full. Stream copying cannot reach
        // the next QUIC read to discover that the browser has closed.
        let deadline = if blackhole {
            relay.blackhole.store(true, Ordering::SeqCst);
            tokio::time::pause();
            Duration::from_secs(32)
        } else {
            client.session.close(0, b"browser gone");
            TEST_TIMEOUT
        };
        timeout(deadline, bridge)
            .await
            .expect("closed session retained a backpressured home connection")
            .unwrap();
        timeout(TEST_TIMEOUT, home.read_to_end(&mut Vec::new()))
            .await
            .expect("home connection was not released")
            .unwrap();
        if composite {
            assert_eq!(
                timeout(TEST_TIMEOUT, home_datagram.read_u8())
                    .await
                    .unwrap()
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::UnexpectedEof
            );
        }
        if blackhole {
            tokio::time::resume();
        }
    }
}

#[tokio::test]
async fn silent_peer_timeout_is_not_extended_by_late_server_traffic() {
    for composite in [false, true] {
        let (mut client, edge, relay) = peers().await;
        let (bridge, mut home, home_datagram) = start_bridge(edge, composite, 4096);
        // Lose the optional path before the peer disappears, too.
        drop(home_datagram);

        // Let handshake housekeeping finish, then leave the client as the
        // last sender of application data.
        tokio::time::sleep(Duration::from_millis(100)).await;
        client.send.write_all(b"last input").await.unwrap();
        let mut input = [0; 10];
        timeout(TEST_TIMEOUT, home.read_exact(&mut input))
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        // Drop packets without delivering CONNECTION_CLOSE or an ICMP error.
        relay.blackhole.store(true, Ordering::SeqCst);
        tokio::time::pause();
        tokio::time::sleep(Duration::from_secs(20)).await;
        assert!(!bridge.is_finished());
        // QUIC permits the first ack-eliciting send after receiving a packet
        // to restart its idle timer, even though the peer is already gone.
        home.write_all(b"late update").await.unwrap();
        timeout(Duration::from_secs(12), bridge)
            .await
            .expect("silent peer survived more than 30 seconds after packet loss")
            .unwrap();
        assert_eq!(
            home.read_u8().await.unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
        tokio::time::resume();
        drop(client);
    }
}

#[tokio::test]
async fn keep_alive_preserves_quiet_sessions_without_application_traffic() {
    for composite in [false, true] {
        let (client, edge, _relay) = peers().await;
        let connection = (*edge.session).clone();
        let (bridge, mut home, home_datagram) = start_bridge(edge, composite, 4096);
        drop(home_datagram);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let initial = connection.stats().frame_rx;
        for _ in 0..4 {
            let acks = connection.stats().frame_rx.acks;
            tokio::time::pause();
            tokio::time::sleep(WEBTRANSPORT_KEEP_ALIVE_INTERVAL).await;
            // Let real UDP I/O deliver the keepalive and its ACK before
            // advancing the virtual clock to the next probe.
            tokio::time::resume();
            timeout(TEST_TIMEOUT, async {
                while connection.stats().frame_rx.acks == acks {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("idle client did not acknowledge a QUIC keepalive");
            assert!(
                !bridge.is_finished(),
                "healthy idle session was disconnected"
            );
        }
        let received = connection.stats().frame_rx;
        assert_eq!(received.stream, initial.stream);
        assert_eq!(received.datagram, initial.datagram);
        client.session.close(0, b"test complete");
        timeout(TEST_TIMEOUT, bridge).await.unwrap().unwrap();
        assert_eq!(
            home.read_u8().await.unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }
}
