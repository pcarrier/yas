use super::*;

fn services_with_heartbeat() -> Services {
    let mut services = test_services(None, unavailable_connector(), None);
    services.ping_interval = Duration::from_secs(1);
    services
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn peer_expiry_releases_only_its_client_record_and_surface_claims() {
    let state = super::super::super::tests::process_transport::test_state(
        super::super::super::process::Server::new(false, true),
    );
    let mut services = Services::from_state(&state);
    services.ping_interval = Duration::from_millis(100);
    let (mut client, server) = tokio::io::duplex(2 * 1024 * 1024);
    let cancellation = ConnectionCancellation::default();
    let registration = state.connections.register(cancellation.clone()).unwrap();
    let task = tokio::spawn(serve_registered(
        server,
        services,
        cancellation,
        Some(registration),
        None,
        None,
        ConnectionOrigin::Network,
    ));
    let (_, hello) = handshake(&mut client, &[]).await;
    timeout(TEST_TIMEOUT, async {
        while !state
            .session
            .lock()
            .await
            .native_yas_clients
            .contains_key(&hello.session_id)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let other_owner = [0xab; 16];
    {
        let mut shared = state.session.lock().await;
        assert!(shared.native_yas_clients.contains_key(&hello.session_id));
        shared
            .native_surface_claims
            .insert((hello.session_id, 1), (800, 600, 240));
        shared
            .native_surface_claims
            .insert((other_owner, 1), (1920, 1080, 120));
    }
    timeout(TEST_TIMEOUT, task).await.unwrap().unwrap();
    timeout(TEST_TIMEOUT, async {
        loop {
            let shared = state.session.lock().await;
            if !shared.native_yas_clients.contains_key(&hello.session_id) {
                assert!(
                    !shared
                        .native_surface_claims
                        .contains_key(&(hello.session_id, 1))
                );
                assert_eq!(
                    shared.mediated_size_for_surface(1, &[]),
                    Some((1920, 1080, 120))
                );
                break;
            }
            drop(shared);
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(client);
}

#[tokio::test(start_paused = true)]
async fn pings_preserve_an_otherwise_idle_session() {
    let (mut client, codec, hello, task) = start_session(services_with_heartbeat(), &[]).await;
    assert!(
        hello
            .families
            .iter()
            .find(|family| family.family_id == family::CORE)
            .unwrap()
            .operations
            .iter()
            .any(
                |operation| operation.kind == yas_wire::core::request_kind::PING
                    && operation.server_sends
            )
    );
    for _ in 0..5 {
        let ping = next_frame(&mut client, &codec).await;
        assert_eq!(
            ping.header,
            FrameHeader::request(
                family::CORE,
                yas_wire::core::request_kind::PING,
                heartbeat::REQUEST_ID
            )
        );
        Ping::decode(&ping.payload).unwrap();
        let reply = Frame {
            header: FrameHeader::result(
                family::CORE,
                yas_wire::core::request_kind::PING,
                heartbeat::REQUEST_ID,
            ),
            payload: ResultPrefix {
                status: Status::Ok,
                detail: Extensions::default(),
                body: PingResult {
                    receiver_receive_ns: 1,
                    receiver_send_ns: 2,
                }
                .encode()
                .unwrap(),
            }
            .encode()
            .unwrap(),
        };
        client
            .write_all(&codec.encode_stream(&reply).unwrap())
            .await
            .unwrap();
    }
    // Client-originated pings still work on the same live session.
    write_request(
        &mut client,
        &codec,
        family::CORE,
        yas_wire::core::request_kind::PING,
        42,
        &Ping {
            sender_monotonic_ns: 3,
        },
    )
    .await;
    assert_eq!(
        next_result(
            &mut client,
            &codec,
            family::CORE,
            yas_wire::core::request_kind::PING,
            42
        )
        .await
        .status,
        Status::Ok
    );
    assert!(!task.is_finished());
    drop(client);
    timeout(TEST_TIMEOUT, task).await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn silent_peer_expires_even_while_it_keeps_reading() {
    let (mut client, codec, _, task) = start_session(services_with_heartbeat(), &[]).await;
    let start = tokio::time::Instant::now();
    let ping = next_frame(&mut client, &codec).await;
    assert_eq!(ping.header.class, Class::Request);
    timeout(TEST_TIMEOUT, task).await.unwrap().unwrap();
    assert_eq!(start.elapsed(), Duration::from_secs(3));
    assert!(read_frame(&mut client, &codec).await.unwrap().is_none());
}

#[tokio::test(start_paused = true)]
async fn peer_expiry_interrupts_a_blocked_writer_and_handler() {
    let gate = Arc::new(OutboundWriterGate::result(
        family::CORE,
        yas_wire::core::request_kind::PING,
        42,
        false,
    ));
    let mut services = services_with_heartbeat();
    services.outbound_writer_gate = Some(Arc::clone(&gate));
    let (mut client, codec, _, task) = start_session(services, &[]).await;
    // Park the writer and saturate its control queue, forcing the dispatcher
    // itself to await an outbound send. Neither can service a timeout branch.
    for request_id in 42..42 + OUTBOUND_CONTROL_QUEUE as u32 + 20 {
        write_request(
            &mut client,
            &codec,
            family::CORE,
            yas_wire::core::request_kind::PING,
            request_id,
            &Ping {
                sender_monotonic_ns: 0,
            },
        )
        .await;
    }
    gate.wait_until_reached().await;
    timeout(TEST_TIMEOUT, task).await.unwrap().unwrap();
    assert!(read_frame(&mut client, &codec).await.unwrap().is_none());
}

#[tokio::test(start_paused = true)]
async fn zero_interval_disables_peer_expiry() {
    let services = test_services(None, unavailable_connector(), None);
    let (client, _, _, task) = start_session(services, &[]).await;
    tokio::time::advance(Duration::from_secs(120)).await;
    assert!(!task.is_finished());
    drop(client);
    timeout(TEST_TIMEOUT, task).await.unwrap().unwrap();
}
