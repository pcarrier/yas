use crate::ProducerKeys;
use crate::ice::{self, IceConfig, Transport};
use crate::signaling;
use crate::turn::{self, TurnRelay};
use futures_util::stream::{FuturesUnordered, StreamExt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::net::Receive;
use str0m::{Candidate, Event, Input, Output, Rtc};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Notify, mpsc};
use yas_wire::core::ClientHello;
use yas_wire::{Decode, Encode, Extension, FrameCodec, PREFACE};

const GATHER_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_NATIVE_CHANNELS_PER_PEER: usize = 64;
/// How much unwritten client-to-server data one reliable channel may hold.
///
/// A full queue here is fatal — the channel is closed as over-budget — so the
/// depth has to be what the protocol says a peer may have in flight, not a
/// number that merely looks safe. One message deep meant a browser sending its
/// preface and first HELLO back to back closed the share before it finished
/// opening, because the writer task had not been scheduled between the two.
///
/// Bytes are what the budget is really about, and an mpsc bounds messages, so
/// this is the recommended in-flight budget divided by the largest message
/// that can occupy a slot.
const PEER_INGRESS_PENDING_MESSAGES: usize = (yas_wire::schema::transport::RECOMMENDED_BUFFERED
    / yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME as u64)
    as usize;
const PEER_INGRESS_MAX_MESSAGE: usize =
    yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME as usize;
const DATAGRAM_INGRESS_MESSAGES: usize = 64;
const DATAGRAM_PAIR_TIMEOUT: Duration = Duration::from_secs(1);

fn enqueue_peer_ingress(
    sender: &mpsc::Sender<Vec<u8>>,
    data: &[u8],
) -> Result<(), mpsc::error::TrySendError<Vec<u8>>> {
    if data.len() > PEER_INGRESS_MAX_MESSAGE {
        return Err(mpsc::error::TrySendError::Full(Vec::new()));
    }
    sender.try_send(data.to_vec())
}

/// Client-to-server ingress for one native YAS byte stream.
///
/// Read-write shares are byte-transparent. Read-only shares buffer exactly
/// the preface and first Core HELLO, inject the required read-only-session
/// extension, and then become byte-transparent. The server, not this relay,
/// advertises and enforces the restricted descriptor catalogue; therefore a
/// forged later frame cannot acquire authority merely by confusing a relay
/// parser.
enum ClientIngress {
    ReadWrite,
    ReadOnly { pending: Vec<u8>, negotiated: bool },
}

impl ClientIngress {
    fn new(access: crate::Access) -> Self {
        match access {
            crate::Access::ReadWrite => Self::ReadWrite,
            crate::Access::ReadOnly => Self::ReadOnly {
                pending: Vec::new(),
                negotiated: false,
            },
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
        match self {
            Self::ReadWrite => Ok((!bytes.is_empty()).then(|| bytes.to_vec())),
            Self::ReadOnly {
                pending,
                negotiated,
            } if *negotiated => Ok((!bytes.is_empty()).then(|| bytes.to_vec())),
            Self::ReadOnly {
                pending,
                negotiated,
            } => {
                pending.extend_from_slice(bytes);
                let compared = pending.len().min(PREFACE.len());
                if pending[..compared] != PREFACE[..compared] {
                    return Err("read-only share received an invalid YAS preface".into());
                }
                if pending.len() < PREFACE.len() + 4 {
                    return Ok(None);
                }

                let length_offset = PREFACE.len();
                let frame_len = u32::from_le_bytes(
                    pending[length_offset..length_offset + 4]
                        .try_into()
                        .expect("bounded length prefix"),
                );
                if frame_len > yas_wire::FrameLimits::pre_hello().max_wire_frame {
                    return Err("read-only share HELLO exceeds the pre-negotiation limit".into());
                }
                let hello_end = PREFACE
                    .len()
                    .checked_add(4)
                    .and_then(|value| value.checked_add(frame_len as usize))
                    .ok_or_else(|| "read-only share HELLO length overflow".to_string())?;
                if pending.len() < hello_end {
                    return Ok(None);
                }

                let codec = FrameCodec::pre_hello();
                let (mut frame, consumed) = codec
                    .decode_stream(&pending[PREFACE.len()..hello_end])
                    .map_err(|error| format!("invalid read-only share HELLO: {error}"))?;
                if consumed != hello_end - PREFACE.len() {
                    return Err("read-only share HELLO has trailing frame bytes".into());
                }
                let mut hello = ClientHello::decode(&frame.payload)
                    .map_err(|error| format!("invalid read-only share HELLO body: {error}"))?;
                let read_only_tag =
                    yas_wire::schema::core::CLIENT_HELLO_READ_ONLY_SESSION_EXTENSION as u16;
                if !hello
                    .extensions
                    .0
                    .iter()
                    .any(|extension| extension.tag == read_only_tag)
                {
                    let position = hello
                        .extensions
                        .0
                        .partition_point(|extension| extension.tag < read_only_tag);
                    hello.extensions.0.insert(
                        position,
                        Extension {
                            tag: read_only_tag,
                            required: true,
                            value: Vec::new(),
                        },
                    );
                }
                frame.payload = hello
                    .encode()
                    .map_err(|error| format!("cannot encode read-only share HELLO: {error}"))?;

                let mut output = Vec::with_capacity(pending.len() + 8);
                output.extend_from_slice(&PREFACE);
                output.extend_from_slice(
                    &codec
                        .encode_stream(&frame)
                        .map_err(|error| format!("cannot encode read-only share frame: {error}"))?,
                );
                output.extend_from_slice(&pending[hello_end..]);
                pending.clear();
                *negotiated = true;
                Ok(Some(output))
            }
        }
    }
}

/// Result from a single parallel gather task.
enum GatherResult {
    Srflx { srflx: SocketAddr, base: SocketAddr },
    Relay(TurnRelay),
}

pub type BoxedRead = Box<dyn tokio::io::AsyncRead + Unpin + Send>;
pub type BoxedWrite = Box<dyn tokio::io::AsyncWrite + Unpin + Send>;

#[derive(Clone)]
struct PeerChannelState {
    aborts: Arc<Vec<tokio::task::AbortHandle>>,
    available: Arc<AtomicBool>,
    write_tx: mpsc::Sender<Vec<u8>>,
    datagram: bool,
    paired_with: Option<ChannelId>,
}

impl PeerChannelState {
    fn abort(&self) {
        for task in self.aborts.iter() {
            task.abort();
        }
    }
}

fn monitored_tasks(
    tasks: Vec<tokio::task::JoinHandle<()>>,
) -> (Arc<Vec<tokio::task::AbortHandle>>, Arc<AtomicBool>) {
    let aborts = tasks
        .iter()
        .map(tokio::task::JoinHandle::abort_handle)
        .collect::<Vec<_>>();
    let monitor_aborts = aborts.clone();
    let available = Arc::new(AtomicBool::new(true));
    let monitor_available = Arc::clone(&available);
    let monitor = tokio::spawn(async move {
        let mut tasks = tasks;
        if tasks.is_empty() {
            return;
        }
        // All pumps share one accepted logical link. Polling the joins in a
        // set makes the first closed half tear down every sibling promptly.
        let mut joins = futures_util::stream::FuturesUnordered::new();
        joins.extend(tasks.drain(..));
        let _ = joins.next().await;
        monitor_available.store(false, Ordering::Release);
        for task in monitor_aborts {
            task.abort();
        }
    });
    let mut all = aborts;
    all.push(monitor.abort_handle());
    (Arc::new(all), available)
}

/// Connect to the local yas-server, optionally routing through yas-proxy.
///
/// When `proxy_sock` is `Some`, the connection is established through the
/// yas-proxy daemon using the `target-yas socket:<sock_path>\n` / `ok\n`
/// handshake.  Otherwise, a direct IPC connection is made.
///
/// If the proxy connection fails and `proxy_ensure` is provided, the proxy
/// daemon is restarted and the connection is retried once.
///
/// Returns boxed (reader, writer) halves for a byte-transparent native stream.
async fn connect_to_server(
    upstream: &crate::Upstream,
) -> Result<(BoxedRead, BoxedWrite), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(open) = upstream.hosted.as_ref() {
        return open().await.map_err(std::convert::Into::into);
    }
    let sock_path = upstream.sock_path.as_str();
    if let Some(proxy) = upstream.proxy_sock.as_deref() {
        match connect_via_proxy(proxy, sock_path, upstream.proxy_uid).await {
            Ok(rw) => return Ok(rw),
            Err(first_err) => {
                // Proxy may be down — try to restart it and retry once.
                if let Some(ensure_fn) = upstream.proxy_ensure.as_ref()
                    && let Ok(new_sock) = ensure_fn().await
                {
                    verbose!("yas-proxy restarted → {new_sock}");
                    return connect_via_proxy(&new_sock, sock_path, upstream.proxy_uid).await;
                }
                return Err(first_err);
            }
        }
    }

    #[cfg(unix)]
    {
        let conn = tokio::net::UnixStream::connect(sock_path).await?;
        let (r, w) = conn.into_split();
        Ok((Box::new(r), Box::new(w)))
    }
    #[cfg(windows)]
    {
        let conn = tokio::net::windows::named_pipe::ClientOptions::new().open(sock_path)?;
        let (r, w) = tokio::io::split(conn);
        Ok((Box::new(r), Box::new(w)))
    }
}

/// Connect to `sock_path` via the yas-proxy daemon at `proxy_sock`.
/// Performs the `target-yas socket:<sock_path>\n` / `ok\n` handshake.
#[cfg(unix)]
async fn connect_via_proxy(
    proxy_sock: &str,
    sock_path: &str,
    expected_uid: Option<u32>,
) -> Result<(BoxedRead, BoxedWrite), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = tokio::net::UnixStream::connect(proxy_sock).await?;
    let expected_uid = expected_uid.ok_or("yas-proxy expected UID is missing")?;
    yas_webserver::local_ipc::verify_peer_uid_named(&stream, expected_uid, "yas-proxy")
        .map_err(|error| format!("refusing yas-proxy at {proxy_sock}: {error}"))?;
    let msg = format!("target-yas socket:{sock_path}\n");
    stream.write_all(msg.as_bytes()).await?;

    // Read the handshake response byte-by-byte to avoid consuming data
    // past `ok\n` with a buffered reader.
    let mut buf = Vec::with_capacity(64);
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).await?;
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 4096 {
            return Err("yas-proxy: handshake response too long".into());
        }
    }
    let resp = String::from_utf8_lossy(&buf);
    let resp = resp.trim_end_matches('\r');
    if resp == "ok" {
        let (r, w) = stream.into_split();
        Ok((Box::new(r), Box::new(w)))
    } else if let Some(m) = resp.strip_prefix("error ") {
        Err(format!("yas-proxy: {m}").into())
    } else {
        Err(format!("yas-proxy: unexpected response: {resp:?}").into())
    }
}

#[cfg(not(unix))]
async fn connect_via_proxy(
    _proxy_sock: &str,
    _sock_path: &str,
    _expected_uid: Option<u32>,
) -> Result<(BoxedRead, BoxedWrite), Box<dyn std::error::Error + Send + Sync>> {
    Err("yas-proxy is not supported on this platform".into())
}

async fn bridge_direct_channel(
    cid: ChannelId,
    access: crate::Access,
    upstream: &crate::Upstream,
    server_tx: mpsc::Sender<(ChannelId, Vec<u8>)>,
) -> Result<PeerChannelState, Box<dyn std::error::Error + Send + Sync>> {
    let (mut reader, mut writer) = connect_to_server(upstream).await?;
    let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(PEER_INGRESS_PENDING_MESSAGES);
    let read_task = tokio::spawn(async move {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if server_tx
                        .send((cid, buffer[..read].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    let write_task = tokio::spawn(async move {
        let mut ingress = ClientIngress::new(access);
        while let Some(data) = write_rx.recv().await {
            match ingress.push(&data) {
                Ok(Some(bytes)) if writer.write_all(&bytes).await.is_err() => break,
                Ok(Some(_)) | Ok(None) => {}
                Err(error) => {
                    verbose!("closing invalid share stream: {error}");
                    break;
                }
            }
        }
    });
    let (aborts, available) = monitored_tasks(vec![read_task, write_task]);
    Ok(PeerChannelState {
        aborts,
        available,
        write_tx,
        datagram: false,
        paired_with: None,
    })
}

#[allow(clippy::too_many_arguments)]
async fn bridge_composite_channels(
    main_cid: ChannelId,
    datagram_cid: ChannelId,
    access: crate::Access,
    upstream: &crate::Upstream,
    server_tx: mpsc::Sender<(ChannelId, Vec<u8>)>,
) -> Result<(PeerChannelState, PeerChannelState), Box<dyn std::error::Error + Send + Sync>> {
    let (mut main_reader, mut main_writer) = connect_to_server(upstream).await?;
    let (side_reader, mut side_writer) = connect_to_server(upstream).await?;

    let mut token: yas_composite_transport::Token = rand::random();
    if token == [0; yas_composite_transport::TOKEN_BYTES] {
        token[0] = 1;
    }
    let maximum = crate::MAX_DATAGRAM_SIZE as u32;
    yas_composite_transport::write_offer(
        &mut main_writer,
        yas_composite_transport::Offer::new(yas_composite_transport::Role::Main, token, maximum)?,
    )
    .await?;
    yas_composite_transport::write_offer(
        &mut side_writer,
        yas_composite_transport::Offer::new(
            yas_composite_transport::Role::Datagram,
            token,
            maximum,
        )?,
    )
    .await?;

    let (main_write_tx, mut main_write_rx) =
        mpsc::channel::<Vec<u8>>(PEER_INGRESS_PENDING_MESSAGES);
    let (datagram_write_tx, datagram_write_rx) =
        mpsc::channel::<Vec<u8>>(DATAGRAM_INGRESS_MESSAGES);

    let main_server_tx = server_tx.clone();
    let main_read_task = tokio::spawn(async move {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            match main_reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if main_server_tx
                        .send((main_cid, buffer[..read].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    let main_write_task = tokio::spawn(async move {
        let mut ingress = ClientIngress::new(access);
        while let Some(data) = main_write_rx.recv().await {
            match ingress.push(&data) {
                Ok(Some(bytes)) if main_writer.write_all(&bytes).await.is_err() => break,
                Ok(Some(_)) | Ok(None) => {}
                Err(error) => {
                    verbose!("closing invalid composite share stream: {error}");
                    break;
                }
            }
        }
    });
    let datagram_read_task = tokio::spawn(forward_server_datagrams_to_sctp(
        side_reader,
        maximum,
        datagram_cid,
        server_tx,
    ));
    let datagram_write_task = tokio::spawn(forward_sctp_datagrams_to_server(
        datagram_write_rx,
        side_writer,
        maximum,
    ));
    let (main_aborts, main_available) = monitored_tasks(vec![main_read_task, main_write_task]);
    let (datagram_aborts, datagram_available) =
        monitored_tasks(vec![datagram_read_task, datagram_write_task]);
    Ok((
        PeerChannelState {
            aborts: main_aborts,
            available: main_available,
            write_tx: main_write_tx,
            datagram: false,
            paired_with: Some(datagram_cid),
        },
        PeerChannelState {
            aborts: datagram_aborts,
            available: datagram_available,
            write_tx: datagram_write_tx,
            datagram: true,
            paired_with: Some(main_cid),
        },
    ))
}

async fn forward_server_datagrams_to_sctp<R, C>(
    mut side_reader: R,
    maximum: u32,
    datagram_cid: C,
    server_tx: mpsc::Sender<(C, Vec<u8>)>,
) where
    R: AsyncRead + Unpin,
    C: Copy + Send,
{
    loop {
        let Ok(frame) = yas_composite_transport::read_datagram(&mut side_reader, maximum).await
        else {
            break;
        };
        // SCTP congestion is datagram loss and never blocks the RTC loop.
        let _ = server_tx.try_send((datagram_cid, frame));
    }
}

async fn forward_sctp_datagrams_to_server<W>(
    mut datagram_write_rx: mpsc::Receiver<Vec<u8>>,
    mut side_writer: W,
    maximum: u32,
) where
    W: AsyncWrite + Unpin,
{
    while let Some(frame) = datagram_write_rx.recv().await {
        if yas_composite_transport::write_datagram(&mut side_writer, &frame, maximum)
            .await
            .is_err()
        {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_peer(
    peer_session_id: String,
    upstream: crate::Upstream,
    mut signal_rx: mpsc::Receiver<serde_json::Value>,
    signal_tx: mpsc::Sender<String>,
    keys: ProducerKeys,
    established: Arc<AtomicBool>,
    ice_config: Option<IceConfig>,
    shutdown: Arc<Notify>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // --- Bind sockets ---
    let udp4 = UdpSocket::bind("0.0.0.0:0")?;
    udp4.set_nonblocking(true)?;
    let port4 = udp4.local_addr()?.port();
    let tokio_udp4 = tokio::net::UdpSocket::from_std(udp4)?;

    // IPv6 socket is optional — skip silently if the OS doesn't support it.
    let udp6_result = UdpSocket::bind("[::]:0").and_then(|s| {
        s.set_nonblocking(true)?;
        Ok(s)
    });
    let (tokio_udp6, port6): (Option<tokio::net::UdpSocket>, Option<u16>) = match udp6_result
        .and_then(|s| {
            let port = s.local_addr()?.port();
            Ok((tokio::net::UdpSocket::from_std(s)?, port))
        }) {
        Ok((s, p)) => (Some(s), Some(p)),
        Err(_) => (None, None),
    };

    // --- Resolve local IPs and compute host addresses ---
    let local_ips = crate::default_local_ips();
    let host_addr4: SocketAddr = local_ips
        .iter()
        .find(|ip| ip.is_ipv4())
        .map(|ip| SocketAddr::new(*ip, port4))
        .unwrap_or_else(|| SocketAddr::new("0.0.0.0".parse::<IpAddr>().unwrap(), port4));
    let host_addr6: Option<SocketAddr> = tokio_udp6.as_ref().and(
        local_ips
            .iter()
            .find(|ip| ip.is_ipv6())
            .map(|ip| SocketAddr::new(*ip, port6.unwrap_or(0))),
    );

    // --- Build Rtc and add host candidates ---
    let mut rtc = Rtc::new(Instant::now());

    if let Ok(c) = Candidate::host(host_addr4, "udp") {
        verbose!("host candidate (IPv4): {host_addr4}");
        rtc.add_local_candidate(c);
    }
    if let Some(h6) = host_addr6
        && let Ok(c) = Candidate::host(h6, "udp")
    {
        verbose!("host candidate (IPv6): {h6}");
        rtc.add_local_candidate(c);
    }

    // --- Parallel ICE gathering ---
    let mut relay: Option<TurnRelay> = None;

    if let Some(config) = &ice_config {
        let (stun_servers, turn_servers) = ice::collect_servers(config);

        let mut tasks: FuturesUnordered<
            std::pin::Pin<Box<dyn std::future::Future<Output = Option<GatherResult>> + Send>>,
        > = FuturesUnordered::new();

        // STUN binding on IPv4 — use host_addr4 as the base so the srflx
        // candidate's base port matches the main tokio_udp4 socket used for
        // transmit routing.
        for stun_addr in stun_servers.iter().copied() {
            let base4 = host_addr4;
            tasks.push(Box::pin(async move {
                let udp = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
                    Ok(s) => s,
                    Err(_) => return None,
                };
                match turn::stun_binding(stun_addr, &udp).await {
                    Ok(srflx) => Some(GatherResult::Srflx { srflx, base: base4 }),
                    Err(e) => {
                        verbose!("STUN binding failed ({stun_addr}): {e}");
                        None
                    }
                }
            }));
        }

        // STUN binding on IPv6 — use host_addr6 as the base for the same reason.
        if let Some(base6) = host_addr6 {
            for stun_addr in stun_servers.iter().copied() {
                tasks.push(Box::pin(async move {
                    let udp = match tokio::net::UdpSocket::bind("[::]:0").await {
                        Ok(s) => s,
                        Err(_) => return None,
                    };
                    match turn::stun_binding(stun_addr, &udp).await {
                        Ok(srflx) => {
                            if !srflx.ip().is_ipv6() {
                                return None;
                            }
                            Some(GatherResult::Srflx { srflx, base: base6 })
                        }
                        Err(e) => {
                            verbose!("STUN binding (IPv6) failed ({stun_addr}): {e}");
                            None
                        }
                    }
                }));
            }
        }

        // TURN allocations — all in parallel
        for ts in turn_servers.iter().cloned() {
            tasks.push(Box::pin(async move {
                let result = match ts.transport {
                    Transport::Udp => {
                        TurnRelay::allocate_udp(ts.addr, &ts.username, &ts.credential).await
                    }
                    Transport::Tcp => {
                        TurnRelay::allocate_tcp(
                            ts.addr,
                            ts.tls,
                            &ts.hostname,
                            &ts.username,
                            &ts.credential,
                        )
                        .await
                    }
                };
                match result {
                    Ok(r) => {
                        verbose!(
                            "TURN allocated relay {} via {:?} {}",
                            r.relay_addr,
                            ts.transport,
                            ts.addr
                        );
                        Some(GatherResult::Relay(r))
                    }
                    Err(e) => {
                        verbose!("TURN allocate ({:?} {}) failed: {e}", ts.transport, ts.addr);
                        None
                    }
                }
            }));
        }

        // Drain results until timeout, stop trying TURN once we have a relay.
        let deadline = tokio::time::sleep(GATHER_TIMEOUT);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                biased;
                result = tasks.next() => {
                    match result {
                        None => break, // all tasks done
                        Some(None) => {} // this task failed, keep going
                        Some(Some(GatherResult::Srflx { srflx, base })) => {
                            if let Ok(c) = Candidate::server_reflexive(srflx, base, "udp") {
                                verbose!("srflx candidate: {srflx} (base {base})");
                                rtc.add_local_candidate(c);
                            }
                        }
                        Some(Some(GatherResult::Relay(r))) => {
                            if relay.is_none() {
                                if let Ok(c) = Candidate::relayed(r.relay_addr, host_addr4, "udp") {
                                    verbose!("relay candidate: {}", r.relay_addr);
                                    rtc.add_local_candidate(c);
                                }
                                relay = Some(r);
                                // Don't break — let STUN tasks finish, but
                                // remaining TURN tasks will just be dropped when
                                // tasks goes out of scope after the loop.
                            }
                        }
                    }
                }
                _ = &mut deadline => {
                    verbose!("ICE gathering timed out after {}s", GATHER_TIMEOUT.as_secs());
                    break;
                }
            }
        }
    }

    // Wait for the SDP offer.  Decrypt via ProducerKeys::open_sealed which
    // tries both the RW and RO consumer keys — the one that works tells us
    // the consumer's access level.
    let (offer, consumer_access): (SdpOffer, crate::Access) = loop {
        match signal_rx.recv().await {
            Some(raw) => {
                let Some((data, access)) = keys.open_sealed(&raw) else {
                    continue;
                };
                if let Some(sdp) = data.get("sdp") {
                    let offer: SdpOffer = serde_json::from_value(sdp.clone())?;
                    break (offer, access);
                }
            }
            None => return Ok(()),
        }
    };

    verbose!("consumer access: {:?}", consumer_access);

    let answer = rtc.sdp_api().accept_offer(offer)?;
    let answer_json = serde_json::to_value(&answer)?;
    let signal_data = serde_json::json!({ "sdp": answer_json });
    let bk = keys.box_keys_for(consumer_access);
    let msg = signaling::build_sealed_message(&keys.signing, &peer_session_id, &signal_data, &bk);
    signal_tx
        .send(msg)
        .await
        .map_err(|e| format!("send failed: {e}"))?;

    let mut buf4 = vec![0u8; 65535];
    let mut buf6 = vec![0u8; 65535];
    let mut signaling_alive = true;
    // Shared across all inbound paths: they all feed one DTLS engine, and a
    // retransmitted flight can arrive on a different path than the original.
    let mut dtls_dedupe = crate::dtls_dedupe::DtlsFlightDedupe::new();

    let relay_addr = relay.as_ref().map(|r| r.relay_addr);

    // Idle detection: if we receive nothing from the peer for this long,
    // assume the connection is dead and tear down.  This catches cases where
    // ICE never transitions to Disconnected (e.g. TURN relay stays alive
    // after the browser tab is closed without clean teardown).
    const PEER_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
    // Stop pushing data into the DataChannel when no incoming peer data has
    // been observed for this long.  A healthy peer sends SCTP ACKs, ICE
    // binding responses, etc. — silence means the peer is gone.  Without
    // this gate the yas-server data pump keeps filling the SCTP send buffer,
    // generating a continuous stream of retransmission Transmit outputs that
    // spin the drive loop at full CPU.  After the cutoff, SCTP retransmits
    // only what is already buffered with natural RTO backoff.
    const SEND_IDLE_CUTOFF: Duration = Duration::from_secs(10);
    // Tear down peers whose DataChannels have all closed but whose ICE
    // is still "alive" (e.g. browser crashed, tab closed without clean
    // teardown — ICE consent / SCTP SACKs keep `last_peer_activity`
    // fresh so PEER_IDLE_TIMEOUT never fires, and str0m's drive loop
    // burns CPU processing ICE/DTLS/SCTP forever).  A healthy browser
    // reopens a channel immediately after closing one, so anything
    // longer than a few seconds without any channel means the peer is
    // really gone.
    const CHANNELS_EMPTY_TIMEOUT: Duration = Duration::from_secs(5);
    let mut last_peer_activity = Instant::now();
    let mut ever_had_channel = false;
    let mut channels_empty_since: Option<Instant> = None;

    // Reusable sleep future — avoids allocating/dropping a TimerEntry on every
    // loop iteration, which was responsible for ~15% of steady-state CPU
    // (timer wheel mutex contention + entry alloc/drop).
    let sleep = tokio::time::sleep(Duration::ZERO);
    tokio::pin!(sleep);

    struct PendingDatagram {
        cid: ChannelId,
        opened: Instant,
        frames: VecDeque<Vec<u8>>,
    }
    let mut channels: HashMap<ChannelId, PeerChannelState> = HashMap::new();
    let mut keepalive_channels = HashSet::<ChannelId>::new();
    let mut pending_datagrams = VecDeque::<PendingDatagram>::new();

    // Frame parked when ch.write() returns Ok(false) — retried after
    // the next poll_output cycle processes SCTP acks and frees buffer.
    // The offset tracks how much of the frame has already been written
    // as separate SCTP messages (the browser reassembles them via its
    // readBuf accumulator).  This is critical because str0m caps the
    // SCTP send buffer at 128 KiB — a single large surface keyframe
    // (often 150-200 KiB) would permanently deadlock if sent as one
    // message since ch.write() rejects anything larger than available().
    let mut pending_send: Option<(ChannelId, Vec<u8>, usize)> = None;

    /// Maximum bytes per DataChannel write.  Must be well below str0m's
    /// MAX_BUFFERED_ACROSS_STREAMS (128 KiB) so that chunks fit even
    /// when the buffer isn't completely empty.
    const MAX_DC_CHUNK: usize = 64 * 1024;

    // yas-server → DataChannel: pump tasks send (ChannelId, framed data) here.
    // Bounded(1): when the DataChannel is congested and pending_send is
    // set, the drive loop stops reading server_rx.  The channel fills (1
    // slot), the pump task blocks on send(), which stops IPC reads, which
    // fills the kernel socket buffer, which backpressures the yas-server.
    // This prevents stale frames from piling up during congestion.
    let (server_tx, mut server_rx) = mpsc::channel::<(ChannelId, Vec<u8>)>(1);

    loop {
        // A local optional-sideband pump can end without a remote SCTP close
        // (for example when yas-server closes only its sideband socket). Tell
        // the peer that this channel is gone, while retaining the reliable
        // channel and detaching its pairing metadata.
        let failed_datagrams = channels
            .iter()
            .filter_map(|(&cid, state)| {
                (state.datagram && !state.available.load(Ordering::Acquire)).then_some(cid)
            })
            .collect::<Vec<_>>();
        for cid in failed_datagrams {
            if let Some(state) = channels.remove(&cid) {
                state.abort();
                if let Some(main_cid) = state.paired_with
                    && let Some(main) = channels.get_mut(&main_cid)
                {
                    main.paired_with = None;
                }
                rtc.direct_api().close_data_channel(cid);
            }
        }
        while pending_datagrams
            .front()
            .is_some_and(|pending| pending.opened.elapsed() >= DATAGRAM_PAIR_TIMEOUT)
        {
            if let Some(pending) = pending_datagrams.pop_front() {
                rtc.direct_api().close_data_channel(pending.cid);
            }
        }
        // Check idle timeout before doing any work.
        if last_peer_activity.elapsed() > PEER_IDLE_TIMEOUT {
            verbose!(
                "peer idle for >{}s, tearing down",
                PEER_IDLE_TIMEOUT.as_secs()
            );
            break;
        }
        if let Some(t) = channels_empty_since
            && t.elapsed() > CHANNELS_EMPTY_TIMEOUT
        {
            verbose!(
                "all data channels closed for >{}s, tearing down",
                CHANNELS_EMPTY_TIMEOUT.as_secs()
            );
            break;
        }

        let timeout = loop {
            // After every poll_output step, try to flush pending_send.
            // Transmit outputs free SCTP send-buffer space, so retrying
            // here gives the parked frame the earliest chance to go out.
            // Writes are chunked at MAX_DC_CHUNK to avoid permanently
            // stalling on frames larger than the 128 KiB SCTP buffer.
            let mut clear_pending = false;
            if let Some((ref cid, ref frame, ref mut offset)) = pending_send {
                if !channels.contains_key(cid) {
                    clear_pending = true;
                } else if let Some(mut ch) = rtc.channel(*cid) {
                    while *offset < frame.len() {
                        let end = (*offset + MAX_DC_CHUNK).min(frame.len());
                        if matches!(ch.write(true, &frame[*offset..end]), Ok(true)) {
                            *offset = end;
                        } else {
                            break;
                        }
                    }
                    if *offset >= frame.len() {
                        clear_pending = true;
                    }
                } else {
                    clear_pending = true;
                }
            }
            if clear_pending {
                pending_send = None;
            }
            match rtc.poll_output()? {
                Output::Timeout(v) => break v,
                Output::Transmit(t) => {
                    if relay_addr == Some(t.source) {
                        if let Some(r) = &relay {
                            let _ = r.send_tx.try_send((t.destination, t.contents.to_vec()));
                        }
                    } else if host_addr6.map(|h6| h6 == t.source).unwrap_or(false) {
                        if let Some(ref udp6) = tokio_udp6 {
                            let _ = udp6.send_to(&t.contents, t.destination).await;
                        }
                    } else {
                        let _ = tokio_udp4.send_to(&t.contents, t.destination).await;
                    }
                    continue;
                }
                Output::Event(ev) => {
                    match ev {
                        Event::ChannelOpen(cid, label) => {
                            verbose!("data channel opened: {label}");
                            ever_had_channel = true;
                            channels_empty_since = None;
                            if channels.len() + keepalive_channels.len() + pending_datagrams.len()
                                >= MAX_NATIVE_CHANNELS_PER_PEER
                            {
                                rtc.direct_api().close_data_channel(cid);
                            } else if label == crate::KEEPALIVE_CHANNEL_LABEL {
                                // A daemon-held consumer uses this channel to
                                // retain ICE/DTLS/SCTP between short-lived YAS
                                // sessions. It deliberately has no server
                                // socket or data path of its own.
                                keepalive_channels.insert(cid);
                            } else if label == crate::DATAGRAM_CHANNEL_LABEL {
                                // Browsers create the unordered datagram
                                // channel immediately before its reliable
                                // peer. Keep unmatched halves in a short,
                                // bounded FIFO; no local socket is opened yet.
                                pending_datagrams.push_back(PendingDatagram {
                                    cid,
                                    opened: Instant::now(),
                                    frames: VecDeque::new(),
                                });
                            } else if label == crate::DATA_CHANNEL_LABEL {
                                if let Some(pending) = pending_datagrams.pop_front() {
                                    let datagram_cid = pending.cid;
                                    match bridge_composite_channels(
                                        cid,
                                        datagram_cid,
                                        consumer_access,
                                        &upstream,
                                        server_tx.clone(),
                                    )
                                    .await
                                    {
                                        Ok((main, datagram)) => {
                                            for frame in pending.frames {
                                                // Pre-pair traffic has the
                                                // same bounded, lossy
                                                // semantics as live traffic.
                                                let _ = datagram.write_tx.try_send(frame);
                                            }
                                            channels.insert(cid, main);
                                            channels.insert(datagram_cid, datagram);
                                            established.store(true, Ordering::Relaxed);
                                        }
                                        Err(error) => {
                                            verbose!(
                                                "yas-server composite connect failed for channels \
                                                 {cid:?}/{datagram_cid:?}: {error}"
                                            );
                                            rtc.direct_api().close_data_channel(cid);
                                            rtc.direct_api().close_data_channel(datagram_cid);
                                        }
                                    }
                                } else {
                                    // Stream-only clients remain valid and
                                    // advertise max_datagram=0 at the server.
                                    match bridge_direct_channel(
                                        cid,
                                        consumer_access,
                                        &upstream,
                                        server_tx.clone(),
                                    )
                                    .await
                                    {
                                        Err(error) => {
                                            verbose!(
                                                "yas-server connect failed for channel {cid:?}: \
                                                 {error}"
                                            );
                                            rtc.direct_api().close_data_channel(cid);
                                        }
                                        Ok(state) => {
                                            channels.insert(cid, state);
                                            established.store(true, Ordering::Relaxed);
                                        }
                                    }
                                }
                            } else {
                                rtc.direct_api().close_data_channel(cid);
                            }
                        }
                        Event::ChannelData(cd) => {
                            let overflowed = channels.get(&cd.id).is_some_and(|state| {
                                if state.datagram {
                                    if !cd.data.is_empty()
                                        && cd.data.len() <= crate::MAX_DATAGRAM_SIZE
                                    {
                                        // A full sideband queue is packet
                                        // loss, never reliable-stream
                                        // backpressure.
                                        let _ = state.write_tx.try_send(cd.data.to_vec());
                                    }
                                    false
                                } else {
                                    enqueue_peer_ingress(&state.write_tx, &cd.data).is_err()
                                }
                            });
                            if !channels.contains_key(&cd.id)
                                && let Some(pending) = pending_datagrams
                                    .iter_mut()
                                    .find(|pending| pending.cid == cd.id)
                                && !cd.data.is_empty()
                                && cd.data.len() <= crate::MAX_DATAGRAM_SIZE
                                && pending.frames.len() < DATAGRAM_INGRESS_MESSAGES
                            {
                                pending.frames.push_back(cd.data.to_vec());
                            }
                            if overflowed {
                                verbose!("closing over-budget native share channel");
                                if let Some(state) = channels.remove(&cd.id) {
                                    state.abort();
                                    if let Some(paired) = state.paired_with {
                                        if let Some(peer) = channels.remove(&paired) {
                                            peer.abort();
                                        }
                                        rtc.direct_api().close_data_channel(paired);
                                    }
                                }
                                rtc.direct_api().close_data_channel(cd.id);
                                if ever_had_channel
                                    && channels.is_empty()
                                    && keepalive_channels.is_empty()
                                    && pending_datagrams.is_empty()
                                {
                                    channels_empty_since.get_or_insert_with(Instant::now);
                                }
                            }
                        }
                        Event::ChannelClose(cid) => {
                            verbose!("data channel closed");
                            keepalive_channels.remove(&cid);
                            if let Some(state) = channels.remove(&cid) {
                                state.abort();
                                if let Some(paired) = state.paired_with {
                                    if state.datagram {
                                        // The unordered channel is optional.
                                        // Closing it tears down only the local
                                        // sideband so YAS falls back to the
                                        // reliable stream.
                                        if let Some(main) = channels.get_mut(&paired) {
                                            main.paired_with = None;
                                        }
                                    } else {
                                        if let Some(peer) = channels.remove(&paired) {
                                            peer.abort();
                                        }
                                        rtc.direct_api().close_data_channel(paired);
                                    }
                                }
                            } else {
                                pending_datagrams.retain(|pending| pending.cid != cid);
                            }
                            if ever_had_channel
                                && channels.is_empty()
                                && keepalive_channels.is_empty()
                                && pending_datagrams.is_empty()
                            {
                                channels_empty_since.get_or_insert_with(Instant::now);
                            }
                        }
                        Event::Connected => {
                            // DTLS is up: from here an epoch-0 ChangeCipherSpec
                            // can only be a retransmission.
                            dtls_dedupe.dtls_connected();
                        }
                        Event::IceConnectionStateChange(state) => {
                            verbose!("ICE state: {state:?}");
                            if matches!(state, str0m::IceConnectionState::Disconnected) {
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
            }
        };

        let mut deadline = tokio::time::Instant::from_std(timeout);
        if let Some(pending) = pending_datagrams.front() {
            deadline = deadline.min(tokio::time::Instant::from_std(
                pending.opened + DATAGRAM_PAIR_TIMEOUT,
            ));
        }
        // Once the peer has gone quiet (no incoming UDP for
        // SEND_IDLE_CUTOFF), floor the sleep at 100 ms so SCTP
        // retransmit timers on already-buffered data cannot
        // busy-spin the event loop until PEER_IDLE_TIMEOUT tears
        // the connection down.
        let deadline = if last_peer_activity.elapsed() >= SEND_IDLE_CUTOFF {
            deadline.max(tokio::time::Instant::now() + Duration::from_millis(100))
        } else {
            deadline
        };
        sleep.as_mut().reset(deadline);

        tokio::select! {
            result = tokio_udp4.recv_from(&mut buf4) => {
                let (n, source) = result?;
                if dtls_dedupe.accept(&buf4[..n])
                    && let Ok(receive) = Receive::new(
                        str0m::net::Protocol::Udp,
                        source,
                        host_addr4,
                        &buf4[..n],
                    )
                {
                    last_peer_activity = Instant::now();
                    rtc.handle_input(Input::Receive(last_peer_activity, receive))?;
                }
            }
            result = async {
                if let Some(ref udp6) = tokio_udp6 {
                    udp6.recv_from(&mut buf6).await
                } else {
                    std::future::pending().await
                }
            } => {
                let (n, source) = result?;
                if let Some(h6) = host_addr6
                    && dtls_dedupe.accept(&buf6[..n])
                    && let Ok(receive) = Receive::new(
                        str0m::net::Protocol::Udp,
                        source,
                        h6,
                        &buf6[..n],
                    )
                {
                    last_peer_activity = Instant::now();
                    rtc.handle_input(Input::Receive(last_peer_activity, receive))?;
                }
            }
            _ = &mut sleep => {
                rtc.handle_input(Input::Timeout(Instant::now()))?;
            }
            turn_data = async {
                if let Some(r) = &mut relay {
                    r.recv_rx.recv().await
                } else {
                    std::future::pending::<Option<(SocketAddr, Vec<u8>)>>().await
                }
            } => {
                if let Some((peer_addr, data)) = turn_data
                    && let Some(ra) = relay_addr
                    && dtls_dedupe.accept(&data)
                    && let Ok(receive) = Receive::new(
                        str0m::net::Protocol::Udp,
                        peer_addr,
                        ra,
                        &data,
                    ) {
                    last_peer_activity = Instant::now();
                    rtc.handle_input(Input::Receive(last_peer_activity, receive))?;
                }
            }
            // yas-server → DataChannel: pump tasks forward data here.
            // Gated on recent peer activity so we don't spin the loop
            // writing into an SCTP association that can never deliver.
            // The yas-server paces frame delivery via its own goodput /
            // ACK feedback loop — the forwarder is a dumb reliable pipe.
            // If the SCTP send buffer is full, park the frame and retry
            // after the next poll_output cycle drains it.
            msg = async {
                if pending_send.is_some() {
                    return std::future::pending().await;
                }
                if last_peer_activity.elapsed() < SEND_IDLE_CUTOFF {
                    server_rx.recv().await
                } else {
                    std::future::pending().await
                }
            } => {
                if let Some((cid, frame)) = msg
                    && let Some(state) = channels.get(&cid)
                    && let Some(mut ch) = rtc.channel(cid)
                {
                    if state.datagram {
                        if !frame.is_empty() && frame.len() <= crate::MAX_DATAGRAM_SIZE {
                            // One complete YAS Event per SCTP message. A full
                            // SCTP buffer is datagram loss; do not park or
                            // retry it behind the reliable stream.
                            let _ = ch.write(true, &frame);
                        }
                    } else {
                        // Write in chunks so frames larger than the SCTP
                        // send buffer (128 KiB) make progress instead of
                        // permanently deadlocking.  The browser reassembles
                        // chunks via its readBuf length-prefix parser.
                        let mut offset = 0usize;
                        while offset < frame.len() {
                            let end = (offset + MAX_DC_CHUNK).min(frame.len());
                            if matches!(ch.write(true, &frame[offset..end]), Ok(true)) {
                                offset = end;
                            } else {
                                break;
                            }
                        }
                        if offset < frame.len() {
                            pending_send = Some((cid, frame, offset));
                        }
                    }
                }
            }
            sig = async {
                if signaling_alive {
                    signal_rx.recv().await
                } else {
                    std::future::pending::<Option<serde_json::Value>>().await
                }
            } => {
                match sig {
                    Some(raw) => {
                        let Some((data, access)) = keys.open_sealed(&raw) else {
                            continue;
                        };
                        if access != consumer_access {
                            continue;
                        }
                        if let Some(candidate) = data.get("candidate")
                            && let Ok(c) = serde_json::from_value::<Candidate>(candidate.clone())
                        {
                            rtc.add_remote_candidate(c);
                        }
                    }
                    None => {
                        signaling_alive = false;
                        if !established.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        verbose!("signaling channel closed, WebRTC connection continues");
                    }
                }
            }
            _ = shutdown.notified() => {
                // Closing the native stream is the truthful shutdown signal;
                // the forwarded YAS byte stream is never modified here.
                break;
            }
        }
    }

    // Clean up all channel pump tasks on exit.
    for (_, state) in channels {
        state.abort();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yas_wire::core::{FamilyOffer, ReceiveLimits};
    use yas_wire::{Extensions, Frame, FrameHeader};

    #[cfg(unix)]
    #[tokio::test]
    async fn proxied_server_path_is_not_sent_to_wrong_uid_listener() {
        // Root is a trusted peer for every endpoint, so a root test runner
        // cannot exercise this rejection.
        if yas_webserver::local_ipc::effective_uid() == 0 {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("prebound.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let accepted = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0u8; 1];
            stream.read(&mut byte).await.unwrap()
        });
        let expected_uid = yas_webserver::local_ipc::effective_uid() ^ 1;
        let error = match connect_via_proxy(
            socket.to_str().unwrap(),
            "/secret/server.sock",
            Some(expected_uid),
        )
        .await
        {
            Ok(_) => panic!("wrong-UID proxy prebind was accepted"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("does not match expected UID"),
            "{error}"
        );
        assert_eq!(accepted.await.unwrap(), 0, "server path reached prebind");
    }

    fn hello_bytes(extensions: Extensions) -> Vec<u8> {
        let hello = ClientHello {
            min_minor: 0,
            max_minor: 0,
            receive: ReceiveLimits {
                max_frame: 1024 * 1024,
                max_decoded: 1024 * 1024,
                max_datagram: 0,
                max_buffered: 1024 * 1024,
            },
            client_instance: [7; 16],
            client_name: "share-test".into(),
            client_release: "test".into(),
            families: vec![FamilyOffer {
                family_id: yas_wire::family::TRANSFER,
                required: false,
                versions: vec![1],
            }],
            codecs: Vec::new(),
            extensions,
        };
        let frame = Frame {
            header: FrameHeader::request(
                yas_wire::family::CORE,
                yas_wire::core::request_kind::HELLO,
                1,
            ),
            payload: hello.encode().unwrap(),
        };
        let mut bytes = PREFACE.to_vec();
        bytes.extend(FrameCodec::pre_hello().encode_stream(&frame).unwrap());
        bytes
    }

    #[test]
    fn read_write_native_ingress_is_byte_transparent() {
        let bytes = hello_bytes(Extensions::default());
        let mut ingress = ClientIngress::new(crate::Access::ReadWrite);
        assert_eq!(ingress.push(&bytes).unwrap().unwrap(), bytes);
    }

    #[test]
    fn read_only_native_ingress_injects_required_hello_extension_when_fragmented() {
        let mut bytes = hello_bytes(Extensions::default());
        bytes.extend_from_slice(b"post-hello-stream-bytes");
        let mut ingress = ClientIngress::new(crate::Access::ReadOnly);
        let mut output = Vec::new();
        for byte in bytes.chunks(1) {
            if let Some(chunk) = ingress.push(byte).unwrap() {
                output.extend_from_slice(&chunk);
            }
        }

        assert_eq!(&output[..PREFACE.len()], &PREFACE);
        let (frame, consumed) = FrameCodec::pre_hello()
            .decode_stream(&output[PREFACE.len()..])
            .unwrap();
        let hello = ClientHello::decode(&frame.payload).unwrap();
        let read_only_tag = yas_wire::schema::core::CLIENT_HELLO_READ_ONLY_SESSION_EXTENSION as u16;
        assert_eq!(
            hello
                .extensions
                .0
                .iter()
                .filter(|extension| extension.tag == read_only_tag)
                .collect::<Vec<_>>(),
            vec![&Extension {
                tag: read_only_tag,
                required: true,
                value: Vec::new(),
            }]
        );
        assert_eq!(
            &output[PREFACE.len() + consumed..],
            b"post-hello-stream-bytes"
        );
    }

    #[test]
    fn read_only_native_ingress_rejects_non_yas_and_oversized_hello() {
        let mut ingress = ClientIngress::new(crate::Access::ReadOnly);
        assert!(ingress.push(b"not-yas").is_err());

        let mut ingress = ClientIngress::new(crate::Access::ReadOnly);
        let mut bytes = PREFACE.to_vec();
        bytes.extend_from_slice(
            &(yas_wire::FrameLimits::pre_hello().max_wire_frame + 1).to_le_bytes(),
        );
        assert!(ingress.push(&bytes).is_err());
    }

    #[test]
    fn native_share_ingress_is_bounded_before_server_admission() {
        let (sender, mut receiver) = mpsc::channel(PEER_INGRESS_PENDING_MESSAGES);
        // A handshake arrives as several messages before anything drains one.
        // Closing the share there is what "closing over-budget native share
        // channel" was really reporting, so the queue has to survive a burst.
        for byte in 0..PEER_INGRESS_PENDING_MESSAGES {
            enqueue_peer_ingress(&sender, &[byte as u8]).unwrap();
        }
        assert!(matches!(
            enqueue_peer_ingress(&sender, &[0xff]),
            Err(mpsc::error::TrySendError::Full(_))
        ));
        assert_eq!(receiver.try_recv().unwrap(), vec![0]);
        while receiver.try_recv().is_ok() {}

        // Oversized is refused on its own terms, and refused *before* it can
        // occupy a slot: an empty queue stays empty.
        let oversized = vec![0x5a; PEER_INGRESS_MAX_MESSAGE + 1];
        assert!(matches!(
            enqueue_peer_ingress(&sender, &oversized),
            Err(mpsc::error::TrySendError::Full(bytes)) if bytes.is_empty()
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn optional_pump_failure_does_not_abort_reliable_pumps() {
        let main_tasks = vec![
            tokio::spawn(std::future::pending()),
            tokio::spawn(std::future::pending()),
        ];
        let (main_aborts, main_available) = monitored_tasks(main_tasks);
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let optional_tasks = vec![
            tokio::spawn(async move {
                let _ = release_rx.await;
            }),
            tokio::spawn(std::future::pending()),
        ];
        let (optional_aborts, optional_available) = monitored_tasks(optional_tasks);

        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while optional_available.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(main_available.load(Ordering::Acquire));

        for abort in main_aborts.iter().chain(optional_aborts.iter()) {
            abort.abort();
        }
    }

    #[tokio::test]
    async fn webrtc_datagram_adapter_preserves_message_boundaries_order_and_duplicates() {
        const MAXIMUM: u32 = 1024;
        let payloads = [b"third".to_vec(), b"first".to_vec(), b"first".to_vec()];

        // Server sideband -> unordered SCTP DataChannel messages.
        let (mut server_side, adapter_side) = tokio::io::duplex(4096);
        let (sctp_tx, mut sctp_rx) = mpsc::channel(8);
        let outbound = tokio::spawn(forward_server_datagrams_to_sctp(
            adapter_side,
            MAXIMUM,
            7u8,
            sctp_tx,
        ));
        for payload in &payloads {
            yas_composite_transport::write_datagram(&mut server_side, payload, MAXIMUM)
                .await
                .unwrap();
        }
        for expected in &payloads {
            let (channel, actual) = tokio::time::timeout(Duration::from_secs(1), sctp_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(channel, 7);
            assert_eq!(&actual, expected);
        }
        outbound.abort();

        // SCTP DataChannel messages -> server sideband. Each message becomes
        // exactly one composite datagram, including duplicates and reordered
        // application sequence.
        let (adapter_side, mut server_side) = tokio::io::duplex(4096);
        let (sctp_tx, sctp_rx) = mpsc::channel(8);
        let inbound = tokio::spawn(forward_sctp_datagrams_to_server(
            sctp_rx,
            adapter_side,
            MAXIMUM,
        ));
        for payload in &payloads {
            sctp_tx.send(payload.clone()).await.unwrap();
        }
        for expected in &payloads {
            let actual = tokio::time::timeout(
                Duration::from_secs(1),
                yas_composite_transport::read_datagram(&mut server_side, MAXIMUM),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(&actual, expected);
        }
        drop(sctp_tx);
        inbound.await.unwrap();
    }

    #[tokio::test]
    async fn webrtc_datagram_adapter_drops_on_sctp_congestion_without_stalling() {
        const MAXIMUM: u32 = 1024;
        let (mut server_side, adapter_side) = tokio::io::duplex(4096);
        let (sctp_tx, mut sctp_rx) = mpsc::channel(1);
        let adapter = tokio::spawn(forward_server_datagrams_to_sctp(
            adapter_side,
            MAXIMUM,
            9u8,
            sctp_tx,
        ));
        yas_composite_transport::write_datagram(&mut server_side, b"kept", MAXIMUM)
            .await
            .unwrap();
        yas_composite_transport::write_datagram(&mut server_side, b"dropped", MAXIMUM)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        assert_eq!(sctp_rx.recv().await.unwrap(), (9, b"kept".to_vec()));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), sctp_rx.recv())
                .await
                .is_err(),
            "a full SCTP queue must drop, never defer, a datagram"
        );
        adapter.abort();
    }
}
