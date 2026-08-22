use crate::BoxKeys;
use crate::ice::{self, Transport};
use crate::signaling;
use crate::turn::{self, TurnRelay};
use futures_util::{
    SinkExt,
    stream::{FuturesUnordered, StreamExt},
};
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use str0m::channel::{ChannelConfig, ChannelId, Reliability};

/// Opaque handle to an open DataChannel on a [`Session`].
/// Returned by [`Session::open_channel`] and consumed by [`Session::close_channel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChannelHandle(ChannelId);
use str0m::net::Receive;
use str0m::{Candidate, Event, Input, Output, Rtc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

const GATHER_TIMEOUT: Duration = Duration::from_secs(4);
/// Maximum time to wait for the share producer (forwarder) to appear on the
/// signaling hub.  If the producer is offline this prevents the client from
/// blocking indefinitely.
const PEER_JOIN_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_SIGNAL_TEXT_BYTES: usize = 64 * 1024;
const MAX_BUFFERED_SIGNALS: usize = 64;
const MAX_BUFFERED_SIGNAL_BYTES: usize = 64 * 1024;

/// In-flight bytes a DuplexStream bridge will hold before the writer blocks.
/// Point where a slow native stream reader starts pushing back.
const BRIDGE_BUF: usize = 256 * 1024;
const BRIDGE_CHUNK: usize = 64 * 1024;
/// DataChannel messages waiting for the bounded byte-stream bridge.
///
/// The producer emits at most `BRIDGE_CHUNK` bytes per SCTP message. Keeping
/// seven queued messages plus the one a blocked writer may own permit ordinary
/// scheduling jitter while bounding retained peer data to 512 KiB per channel,
/// in addition to `BRIDGE_BUF`.
const BRIDGE_PENDING_MESSAGES: usize = 7;
/// Shared app-to-DataChannel handoff. The per-channel reader awaits this
/// queue, propagating a congested SCTP uplink back through the bounded duplex.
const BRIDGE_EGRESS_MESSAGES: usize = 1;
const MAX_DATA_CHANNELS: usize = 64;
const DRIVE_COMMANDS: usize = MAX_DATA_CHANNELS * 2;
const DATAGRAM_PENDING_MESSAGES: usize = 64;

fn enqueue_channel_data(
    sender: &mpsc::Sender<Vec<u8>>,
    data: &[u8],
) -> Result<(), mpsc::error::TrySendError<Vec<u8>>> {
    if data.len() > BRIDGE_CHUNK {
        return Err(mpsc::error::TrySendError::Full(Vec::new()));
    }
    sender.try_send(data.to_vec())
}

#[derive(Deserialize)]
struct ServerMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    role: Option<String>,
    data: Option<serde_json::Value>,
    message: Option<String>,
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Result from a single parallel gather task.
enum GatherResult {
    Srflx { srflx: SocketAddr, base: SocketAddr },
    Relay(TurnRelay),
}

// ---------------------------------------------------------------------------
// Commands sent into the drive task
// ---------------------------------------------------------------------------

type DatagramQueueReceiver = mpsc::Receiver<Vec<u8>>;
type DatagramOpenResult = Result<(ChannelId, DatagramQueueReceiver, Arc<AtomicBool>), String>;
type DatagramOpenReply = oneshot::Sender<DatagramOpenResult>;
type StreamOpenResult = Result<(ChannelId, tokio::io::DuplexStream), String>;
type StreamOpenReply = oneshot::Sender<StreamOpenResult>;

enum PendingOpen {
    Stream(StreamOpenReply),
    Datagram(DatagramOpenReply),
}

enum InitialChannels {
    Stream(String),
    Composite,
}

struct InitialChannelIds {
    reliable: ChannelId,
    datagram: Option<ChannelId>,
}

enum DriveCmd {
    /// Open a new DataChannel with the given label and hand back a DuplexStream.
    Open {
        label: String,
        reply: StreamOpenReply,
    },
    OpenDatagram {
        reply: DatagramOpenReply,
    },
    /// Close a channel. The ICE/DTLS session keeps running.
    Close {
        id: ChannelId,
    },
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// A live WebRTC session (ICE+DTLS+SCTP) that can open and close DataChannels
/// without re-negotiating.
#[derive(Clone)]
pub struct Session {
    inner: Arc<SessionInner>,
}

/// One unordered, zero-retransmit WebRTC DataChannel. Every send and receive
/// is exactly one complete YAS Event frame; queue pressure is observable loss.
pub struct DatagramChannel {
    handle: ChannelHandle,
    sender: mpsc::Sender<(ChannelId, Vec<u8>, bool)>,
    receiver: DatagramQueueReceiver,
    available: Arc<AtomicBool>,
}

/// Cloneable no-wait sender for a YAS datagram DataChannel.
#[derive(Clone)]
pub struct DatagramSender {
    handle: ChannelHandle,
    sender: mpsc::Sender<(ChannelId, Vec<u8>, bool)>,
    available: Arc<AtomicBool>,
}

/// Message-preserving receive half for a YAS datagram DataChannel.
pub struct DatagramReceiver {
    receiver: DatagramQueueReceiver,
    available: Arc<AtomicBool>,
}

impl DatagramChannel {
    pub fn handle(&self) -> ChannelHandle {
        self.handle
    }

    pub fn try_send(&self, frame: Vec<u8>) -> Result<(), Vec<u8>> {
        DatagramSender {
            handle: self.handle,
            sender: self.sender.clone(),
            available: Arc::clone(&self.available),
        }
        .try_send(frame)
    }

    pub fn into_parts(self) -> (DatagramSender, DatagramReceiver) {
        (
            DatagramSender {
                handle: self.handle,
                sender: self.sender,
                available: Arc::clone(&self.available),
            },
            DatagramReceiver {
                receiver: self.receiver,
                available: self.available,
            },
        )
    }

    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        let frame = self.receiver.recv().await;
        if frame.is_none() {
            self.available.store(false, Ordering::Release);
        }
        frame
    }
}

impl DatagramSender {
    pub fn handle(&self) -> ChannelHandle {
        self.handle
    }

    pub fn try_send(&self, frame: Vec<u8>) -> Result<(), Vec<u8>> {
        if frame.is_empty() || frame.len() > crate::MAX_DATAGRAM_SIZE {
            return Err(frame);
        }
        if !self.is_available() {
            return Err(frame);
        }
        self.sender
            .try_send((self.handle.0, frame, true))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full((_, frame, _))
                | mpsc::error::TrySendError::Closed((_, frame, _)) => frame,
            })
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire) && !self.sender.is_closed()
    }
}

impl DatagramReceiver {
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        let frame = self.receiver.recv().await;
        if frame.is_none() {
            self.available.store(false, Ordering::Release);
        }
        frame
    }
}

struct SessionInner {
    cmd_tx: mpsc::Sender<DriveCmd>,
    app_data_tx: mpsc::Sender<(ChannelId, Vec<u8>, bool)>,
}

impl Session {
    /// Establish the ICE+DTLS+SCTP session and open the first DataChannel
    /// labeled `"yas.v1"`.
    /// This is the expensive part; subsequent `open_channel()` calls are cheap.
    pub async fn establish(
        passphrase: &str,
        signal_url: &str,
    ) -> Result<(Session, ChannelHandle, tokio::io::DuplexStream), BoxError> {
        Self::establish_with_label(passphrase, signal_url, crate::DATA_CHANNEL_LABEL).await
    }

    /// Internal constructor for the canonical initial DataChannel.
    async fn establish_with_label(
        passphrase: &str,
        signal_url: &str,
        label: &str,
    ) -> Result<(Session, ChannelHandle, tokio::io::DuplexStream), BoxError> {
        crate::init_verbose();
        let (
            rtc,
            tokio_udp4,
            host_addr4,
            tokio_udp6,
            host_addr6,
            relay,
            ws_read,
            ws_write,
            box_keys,
            initial_ids,
        ) = setup_rtc(
            passphrase,
            signal_url,
            InitialChannels::Stream(label.to_owned()),
        )
        .await?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<DriveCmd>(DRIVE_COMMANDS);
        let (app_data_tx, app_data_rx) =
            mpsc::channel::<(ChannelId, Vec<u8>, bool)>(BRIDGE_EGRESS_MESSAGES);
        let (ready_tx, ready_rx) = oneshot::channel::<StreamOpenResult>();
        let drive_app_data_tx = app_data_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = drive(
                rtc,
                tokio_udp4,
                host_addr4,
                tokio_udp6,
                host_addr6,
                relay,
                ws_read,
                ws_write,
                vec![(initial_ids.reliable, PendingOpen::Stream(ready_tx))],
                cmd_rx,
                drive_app_data_tx,
                app_data_rx,
                box_keys,
            )
            .await
            {
                verbose!("webrtc client error: {e}");
            }
        });

        let (cid, stream) = ready_rx
            .await
            .map_err(|_| "driver task died before first channel open")??;

        let session = Session {
            inner: Arc::new(SessionInner {
                cmd_tx,
                app_data_tx,
            }),
        };
        Ok((session, ChannelHandle(cid), stream))
    }

    /// Establish one composite YAS transport. The unordered datagram channel
    /// is created first and the reliable channel immediately after it in the
    /// same SDP generation so the producer can pair the adjacent DCEP opens.
    pub async fn establish_composite(
        passphrase: &str,
        signal_url: &str,
    ) -> Result<
        (
            Session,
            ChannelHandle,
            tokio::io::DuplexStream,
            DatagramChannel,
        ),
        BoxError,
    > {
        crate::init_verbose();
        let (
            rtc,
            tokio_udp4,
            host_addr4,
            tokio_udp6,
            host_addr6,
            relay,
            ws_read,
            ws_write,
            box_keys,
            initial_ids,
        ) = setup_rtc(passphrase, signal_url, InitialChannels::Composite).await?;
        let datagram_id = initial_ids
            .datagram
            .ok_or("composite WebRTC setup omitted its datagram channel")?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<DriveCmd>(DRIVE_COMMANDS);
        let (app_data_tx, app_data_rx) =
            mpsc::channel::<(ChannelId, Vec<u8>, bool)>(BRIDGE_EGRESS_MESSAGES);
        let (stream_tx, stream_rx) = oneshot::channel::<StreamOpenResult>();
        let (datagram_tx, datagram_rx) = oneshot::channel::<DatagramOpenResult>();
        let drive_app_data_tx = app_data_tx.clone();

        tokio::spawn(async move {
            if let Err(error) = drive(
                rtc,
                tokio_udp4,
                host_addr4,
                tokio_udp6,
                host_addr6,
                relay,
                ws_read,
                ws_write,
                vec![
                    (datagram_id, PendingOpen::Datagram(datagram_tx)),
                    (initial_ids.reliable, PendingOpen::Stream(stream_tx)),
                ],
                cmd_rx,
                drive_app_data_tx,
                app_data_rx,
                box_keys,
            )
            .await
            {
                verbose!("webrtc client error: {error}");
            }
        });

        let (reliable_id, stream) = stream_rx
            .await
            .map_err(|_| "driver task died before reliable channel open")??;
        let (received_datagram_id, receiver, available) = datagram_rx
            .await
            .map_err(|_| "driver task died before datagram channel open")??;
        let session = Session {
            inner: Arc::new(SessionInner {
                cmd_tx,
                app_data_tx: app_data_tx.clone(),
            }),
        };
        let datagram = DatagramChannel {
            handle: ChannelHandle(received_datagram_id),
            sender: app_data_tx,
            receiver,
            available,
        };
        Ok((session, ChannelHandle(reliable_id), stream, datagram))
    }

    /// Open a new `"yas.v1"` DataChannel on the existing session.
    /// No ICE or SDP negotiation — just an SCTP stream open.
    pub async fn open_channel(&self) -> Result<(ChannelHandle, tokio::io::DuplexStream), String> {
        self.open_channel_with_label(crate::DATA_CHANNEL_LABEL)
            .await
    }

    /// Open the canonical unordered, zero-retransmit YAS datagram channel.
    pub async fn open_datagram_channel(&self) -> Result<DatagramChannel, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(DriveCmd::OpenDatagram { reply: reply_tx })
            .await
            .map_err(|_| "drive task has exited".to_string())?;
        let (id, receiver, available) = reply_rx
            .await
            .map_err(|_| "drive task died waiting for datagram channel".to_string())??;
        Ok(DatagramChannel {
            handle: ChannelHandle(id),
            sender: self.inner.app_data_tx.clone(),
            receiver,
            available,
        })
    }

    /// Open a lightweight keepalive DataChannel.
    ///
    /// The channel label is `"keepalive"`, which the forwarder (producer)
    /// recognises and does **not** bridge to the yas-server, so no
    /// server-side client state is created.  The channel's only purpose is
    /// to keep the ICE/DTLS/SCTP session alive while the entry sits in a
    /// connection pool.
    pub async fn open_keepalive(&self) -> Result<ChannelHandle, String> {
        let (handle, _stream) = self.open_channel_with_label("keepalive").await?;
        // _stream dropped: the per-channel pump task exits, but the SCTP
        // channel remains open in str0m, keeping the session alive.
        Ok(handle)
    }

    /// Verify the ICE/DTLS/SCTP path is alive by doing a DCEP
    /// round-trip.  Opens a lightweight channel (not bridged to
    /// yas-server) and immediately closes it.  Returns `Ok(())` if
    /// the path is healthy.
    pub async fn probe(&self) -> Result<(), String> {
        let (handle, _stream) = self.open_channel_with_label("probe").await?;
        self.close_channel(handle).await;
        Ok(())
    }

    /// Open a DataChannel with an arbitrary label.
    async fn open_channel_with_label(
        &self,
        label: &str,
    ) -> Result<(ChannelHandle, tokio::io::DuplexStream), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(DriveCmd::Open {
                label: label.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| "drive task has exited".to_string())?;
        let (cid, stream) = reply_rx
            .await
            .map_err(|_| "drive task died waiting for channel open".to_string())??;
        Ok((ChannelHandle(cid), stream))
    }

    /// Close a specific channel. The underlying ICE/DTLS session stays alive.
    pub async fn close_channel(&self, handle: ChannelHandle) {
        let _ = self
            .inner
            .cmd_tx
            .send(DriveCmd::Close { id: handle.0 })
            .await;
    }

    /// Returns `true` if the background drive task is still running.
    ///
    /// A session becomes non-alive when the drive task exits (ICE disconnect,
    /// error, etc.).  This is a cheap check (no I/O) — it just tests whether
    /// the command channel's receiver has been dropped.
    pub fn is_alive(&self) -> bool {
        !self.inner.cmd_tx.is_closed()
    }
}

// ---------------------------------------------------------------------------
// Single-channel connect convenience
// ---------------------------------------------------------------------------

/// Establish a session and open a channel. The session is dropped after this
/// call, so the drive task exits once the channel closes — same behaviour as
/// before the Session API was introduced.
pub async fn connect(
    passphrase: &str,
    signal_url: &str,
) -> Result<tokio::io::DuplexStream, BoxError> {
    let (_session, _handle, stream) = Session::establish(passphrase, signal_url).await?;
    // _session dropped: cmd_tx dropped. Drive task exits when channel closes.
    Ok(stream)
}

// ---------------------------------------------------------------------------
// Common setup: ICE gathering + SDP exchange
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
async fn setup_rtc(
    passphrase: &str,
    signal_url: &str,
    initial_channels: InitialChannels,
) -> Result<
    (
        Rtc,
        tokio::net::UdpSocket,
        SocketAddr,
        Option<tokio::net::UdpSocket>,
        Option<SocketAddr>,
        Option<TurnRelay>,
        futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        BoxKeys,
        InitialChannelIds,
    ),
    BoxError,
> {
    let consumer =
        crate::parse_consumer_secret(passphrase).map_err(|e| -> BoxError { e.into() })?;
    // Connect to the producer's channel (the passphrase-derived Ed25519 public
    // key) and sign with the matching secret key.  The hub verifies signatures
    // against the channel ID as the Ed25519 public key, so the signing key must
    // correspond to the channel we connect to.
    // Multiple consumers can coexist in the same channel; the hub gives each a
    // unique sessionId (UUID).
    let signing_key = consumer.signing.clone();
    let public_key_hex = crate::hex_encode(signing_key.verifying_key().as_bytes());
    let box_keys = consumer.box_keys();

    let ice_config = ice::fetch_ice_config(signal_url).await.ok();

    let ws_url = format!(
        "{}/channel/{}/consumer",
        signal_url.trim_end_matches('/'),
        public_key_hex,
    );
    let ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_SIGNAL_TEXT_BYTES))
        .max_frame_size(Some(MAX_SIGNAL_TEXT_BYTES));
    let (ws, _) =
        tokio_tungstenite::connect_async_with_config(&ws_url, Some(ws_config), false).await?;
    let (mut ws_write, mut ws_read) = ws.split();

    let _my_session_id = loop {
        let msg = ws_read
            .next()
            .await
            .ok_or("signaling closed before registration")??;
        if let Message::Text(t) = msg {
            if t.len() > MAX_SIGNAL_TEXT_BYTES {
                return Err("oversized signaling message".into());
            }
            if let Ok(m) = serde_json::from_str::<ServerMessage>(&t) {
                if m.msg_type == "registered" {
                    let id = m.session_id.unwrap_or_default();
                    uuid::Uuid::parse_str(&id)
                        .map_err(|_| "signaling hub returned an invalid session ID")?;
                    verbose!("registered with signaling hub (session {id})");
                    break id;
                }
                if m.msg_type == "error" {
                    return Err(format!("signaling: {}", m.message.unwrap_or_default()).into());
                }
            }
        }
    };

    verbose!("waiting for forwarder to join signaling hub...");
    let mut forwarder_session_id = tokio::time::timeout(PEER_JOIN_TIMEOUT, async {
        loop {
            let msg = ws_read
                .next()
                .await
                .ok_or("signaling closed before peer joined")?;
            let msg = msg?;
            if let Message::Text(t) = msg {
                if t.len() > MAX_SIGNAL_TEXT_BYTES {
                    return Err("oversized signaling message".into());
                }
                if let Ok(m) = serde_json::from_str::<ServerMessage>(&t) {
                    if m.msg_type == "peer_joined" {
                        // Only accept peer_joined from the producer side; ignore
                        // other consumers that may join the same channel (e.g. other
                        // relay connections to the same remote share).
                        if m.role.as_deref() == Some("consumer") {
                            verbose!("ignoring peer_joined from another consumer");
                            continue;
                        }
                        let id = m.session_id.unwrap_or_default();
                        uuid::Uuid::parse_str(&id)
                            .map_err(|_| "signaling hub returned an invalid peer ID")?;
                        verbose!("forwarder joined (session {id})");
                        return Ok::<_, BoxError>(id);
                    }
                    if m.msg_type == "error" {
                        return Err(format!("signaling: {}", m.message.unwrap_or_default()).into());
                    }
                }
            }
        }
    })
    .await
    .map_err(|_| -> BoxError {
        "timed out waiting for share producer (is `yas share` running on the remote?)".into()
    })??;

    let udp4 = UdpSocket::bind("0.0.0.0:0")?;
    udp4.set_nonblocking(true)?;
    let port4 = udp4.local_addr()?.port();
    let tokio_udp4 = tokio::net::UdpSocket::from_std(udp4)?;

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

    let mut relay: Option<TurnRelay> = None;

    if let Some(config) = &ice_config {
        let (stun_servers, turn_servers) = ice::collect_servers(config);

        let mut tasks: FuturesUnordered<
            std::pin::Pin<Box<dyn std::future::Future<Output = Option<GatherResult>> + Send>>,
        > = FuturesUnordered::new();

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

        let deadline = tokio::time::sleep(GATHER_TIMEOUT);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                biased;
                result = tasks.next() => {
                    match result {
                        None => break,
                        Some(None) => {}
                        Some(Some(GatherResult::Srflx { srflx, base })) => {
                            if let Ok(c) = Candidate::server_reflexive(srflx, base, "udp") {
                                verbose!("srflx candidate: {srflx} (base {base})");
                                rtc.add_local_candidate(c);
                            }
                        }
                        Some(Some(GatherResult::Relay(r))) => {
                            if relay.is_none() {
                                if let Ok(c) =
                                    Candidate::relayed(r.relay_addr, host_addr4, "udp")
                                {
                                    verbose!("relay candidate: {}", r.relay_addr);
                                    rtc.add_local_candidate(c);
                                }
                                relay = Some(r);
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

    // The first channel triggers the SDP offer/answer. Composite transports
    // add both DCEP channels to the same generation, datagram first, so the
    // producer can pair them without an in-band association message.
    let mut changes = rtc.sdp_api();
    let initial_ids = match initial_channels {
        InitialChannels::Stream(label) => InitialChannelIds {
            reliable: changes.add_channel(label),
            datagram: None,
        },
        InitialChannels::Composite => {
            let datagram = changes.add_channel_with_config(ChannelConfig {
                label: crate::DATAGRAM_CHANNEL_LABEL.to_owned(),
                ordered: false,
                reliability: Reliability::MaxRetransmits { retransmits: 0 },
                ..ChannelConfig::default()
            });
            InitialChannelIds {
                reliable: changes.add_channel(crate::DATA_CHANNEL_LABEL.to_owned()),
                datagram: Some(datagram),
            }
        }
    };
    let (offer, pending) = changes.apply().unwrap();

    let offer_json = serde_json::to_value(&offer)?;
    let signal_data = serde_json::json!({ "sdp": offer_json });
    let msg = signaling::build_sealed_message(
        &signing_key,
        &forwarder_session_id,
        &signal_data,
        &box_keys,
    );
    verbose!("sending SDP offer to forwarder...");
    ws_write.send(Message::Text(msg.into())).await?;

    let mut answer_pending = Some(pending);
    let mut signal_rx_buf: Vec<serde_json::Value> = Vec::new();
    let mut signal_rx_bytes = 0usize;

    loop {
        let msg = ws_read
            .next()
            .await
            .ok_or("signaling closed before answer")??;
        if let Message::Text(t) = msg {
            if t.len() > MAX_SIGNAL_TEXT_BYTES {
                return Err("oversized signaling message".into());
            }
            let signal_bytes = t.len();
            if let Ok(m) = serde_json::from_str::<ServerMessage>(&t) {
                verbose!("signaling rx: type={:?}", m.msg_type);
                if m.msg_type == "peer_joined" {
                    // Only react to producer peer_joined; ignore other consumers
                    // joining the same channel.
                    if m.role.as_deref() == Some("consumer") {
                        verbose!("ignoring peer_joined from another consumer during SDP exchange");
                        continue;
                    }
                    // The hub replaced the pairing (e.g. the ephemeral session
                    // expired while we were doing ICE gathering).  Update our
                    // target and re-send the offer so the forwarder can answer.
                    let new_id = m.session_id.unwrap_or_default();
                    uuid::Uuid::parse_str(&new_id)
                        .map_err(|_| "signaling hub returned an invalid peer ID")?;
                    if new_id != forwarder_session_id {
                        verbose!(
                            "forwarder session changed {forwarder_session_id} → {new_id}, re-sending SDP offer"
                        );
                        forwarder_session_id = new_id;
                        let offer_json = serde_json::to_value(&offer)?;
                        let signal_data = serde_json::json!({ "sdp": offer_json });
                        let msg = signaling::build_sealed_message(
                            &signing_key,
                            &forwarder_session_id,
                            &signal_data,
                            &box_keys,
                        );
                        ws_write.send(Message::Text(msg.into())).await?;
                    }
                } else if m.msg_type == "signal"
                    && let Some(raw) = m.data
                {
                    let Some(data) = signaling::open_sealed_data(&raw, &box_keys) else {
                        continue;
                    };
                    if let Some(sdp) = data.get("sdp") {
                        let sdp_type = sdp.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                        verbose!("received SDP from forwarder: type={sdp_type:?}");
                        // The hub may echo our own offer back to us as a signal;
                        // ignore any SDP that isn't an answer.
                        match serde_json::from_value(sdp.clone()) {
                            Ok(answer) => {
                                if let Some(p) = answer_pending.take() {
                                    rtc.sdp_api().accept_answer(p, answer)?;
                                }
                            }
                            Err(e) => {
                                verbose!("ignoring SDP signal that is not an answer: {e}");
                            }
                        }
                    } else if data.get("candidate").is_some() {
                        verbose!("received remote ICE candidate (pre-answer buffer)");
                        if signal_rx_buf.len() >= MAX_BUFFERED_SIGNALS
                            || signal_rx_bytes + signal_bytes > MAX_BUFFERED_SIGNAL_BYTES
                        {
                            return Err("buffered signaling budget exceeded".into());
                        }
                        signal_rx_buf.push(data);
                        signal_rx_bytes += signal_bytes;
                    } else {
                        verbose!("received unknown signal data (ignored)");
                    }
                }
            }
            if answer_pending.is_none() {
                break;
            }
        }
    }

    verbose!(
        "applying {} buffered remote ICE candidates",
        signal_rx_buf.len()
    );
    for data in signal_rx_buf.drain(..) {
        if let Some(candidate) = data.get("candidate")
            && let Ok(c) = serde_json::from_value::<Candidate>(candidate.clone())
        {
            verbose!("remote ICE candidate: {c:?}");
            rtc.add_remote_candidate(c);
        }
    }

    Ok((
        rtc,
        tokio_udp4,
        host_addr4,
        tokio_udp6,
        host_addr6,
        relay,
        ws_read,
        ws_write,
        box_keys,
        initial_ids,
    ))
}

// ---------------------------------------------------------------------------
// Drive task
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn drive(
    mut rtc: Rtc,
    tokio_udp4: tokio::net::UdpSocket,
    host_addr4: SocketAddr,
    tokio_udp6: Option<tokio::net::UdpSocket>,
    host_addr6: Option<SocketAddr>,
    mut relay: Option<TurnRelay>,
    mut ws_read: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    mut _ws_write: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    initial_pending: Vec<(ChannelId, PendingOpen)>,
    mut cmd_rx: mpsc::Receiver<DriveCmd>,
    app_data_tx: mpsc::Sender<(ChannelId, Vec<u8>, bool)>,
    mut app_data_rx: mpsc::Receiver<(ChannelId, Vec<u8>, bool)>,
    box_keys: BoxKeys,
) -> Result<(), BoxError> {
    let relay_addr = relay.as_ref().map(|r| r.relay_addr);
    let mut buf4 = vec![0u8; 65535];
    let mut buf6 = vec![0u8; 65535];
    let mut signaling_alive = true;
    // Whether the Session handle is still alive (cmd_rx not yet closed).
    // Once false we stop polling cmd_rx but keep running until all channels close.
    let mut session_alive = true;
    // Shared across all inbound paths: they all feed one DTLS engine, and a
    // retransmitted flight can arrive on a different path than the original.
    let mut dtls_dedupe = crate::dtls_dedupe::DtlsFlightDedupe::new();

    // Reusable sleep future — avoids allocating/dropping a TimerEntry on every
    // loop iteration, which was responsible for ~15% of steady-state CPU
    // (timer wheel mutex contention + entry alloc/drop).
    let sleep = tokio::time::sleep(std::time::Duration::ZERO);
    tokio::pin!(sleep);

    // Channels waiting for ChannelOpen confirmation.
    type PendingOpenMap = std::collections::HashMap<ChannelId, PendingOpen>;
    let mut pending_open: PendingOpenMap = initial_pending.into_iter().collect();

    let mut pending_send: Option<(ChannelId, Vec<u8>)> = None;

    // Active channels keep reliable stream plumbing separate from the lossy
    // message-preserving datagram path.
    enum ChannelState {
        Stream {
            abort: tokio::task::AbortHandle,
            write_tx: mpsc::Sender<Vec<u8>>,
        },
        Datagram {
            write_tx: mpsc::Sender<Vec<u8>>,
            available: Arc<AtomicBool>,
        },
    }
    let mut channel_tasks: std::collections::HashMap<ChannelId, ChannelState> =
        std::collections::HashMap::new();

    loop {
        let timeout = loop {
            if let Some((cid, ref frame)) = pending_send {
                if let Some(mut ch) = rtc.channel(cid) {
                    if matches!(ch.write(true, frame), Ok(true)) {
                        pending_send = None;
                    }
                } else {
                    return Ok(());
                }
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
                            verbose!("DataChannel opened: {label} (id {cid:?})");
                            match pending_open.remove(&cid) {
                                Some(PendingOpen::Stream(reply_tx)) => {
                                    let (app_half, driver_half) = tokio::io::duplex(BRIDGE_BUF);
                                    let (write_tx, mut write_rx) =
                                        mpsc::channel::<Vec<u8>>(BRIDGE_PENDING_MESSAGES);
                                    let app_tx = app_data_tx.clone();
                                    let (mut drv_r, mut drv_w) = tokio::io::split(driver_half);
                                    let read_handle = tokio::spawn(async move {
                                        let mut buffer = vec![0u8; BRIDGE_CHUNK];
                                        loop {
                                            match drv_r.read(&mut buffer).await {
                                                Ok(0) | Err(_) => break,
                                                Ok(read) => {
                                                    if app_tx
                                                        .send((cid, buffer[..read].to_vec(), false))
                                                        .await
                                                        .is_err()
                                                    {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    });
                                    let write_handle = tokio::spawn(async move {
                                        while let Some(data) = write_rx.recv().await {
                                            if drv_w.write_all(&data).await.is_err() {
                                                break;
                                            }
                                        }
                                    });
                                    let handle = tokio::spawn(async move {
                                        tokio::select! {
                                            _ = read_handle => {}
                                            _ = write_handle => {}
                                        }
                                    });
                                    channel_tasks.insert(
                                        cid,
                                        ChannelState::Stream {
                                            abort: handle.abort_handle(),
                                            write_tx,
                                        },
                                    );
                                    let _ = reply_tx.send(Ok((cid, app_half)));
                                }
                                Some(PendingOpen::Datagram(reply_tx)) => {
                                    let (write_tx, receiver) =
                                        mpsc::channel(DATAGRAM_PENDING_MESSAGES);
                                    let available = Arc::new(AtomicBool::new(true));
                                    channel_tasks.insert(
                                        cid,
                                        ChannelState::Datagram {
                                            write_tx,
                                            available: Arc::clone(&available),
                                        },
                                    );
                                    let _ = reply_tx.send(Ok((cid, receiver, available)));
                                }
                                None => {}
                            }
                        }
                        Event::ChannelData(cd) => {
                            let overflowed =
                                channel_tasks.get(&cd.id).is_some_and(|state| match state {
                                    ChannelState::Stream { write_tx, .. } => {
                                        enqueue_channel_data(write_tx, &cd.data).is_err()
                                    }
                                    ChannelState::Datagram { write_tx, .. } => {
                                        if cd.data.is_empty()
                                            || cd.data.len() > crate::MAX_DATAGRAM_SIZE
                                        {
                                            false
                                        } else {
                                            let _ = write_tx.try_send(cd.data.to_vec());
                                            false
                                        }
                                    }
                                });
                            if overflowed {
                                if let Some(ChannelState::Stream { abort, .. }) =
                                    channel_tasks.remove(&cd.id)
                                {
                                    abort.abort();
                                }
                                rtc.direct_api().close_data_channel(cd.id);
                            }
                        }
                        Event::ChannelClose(cid) => {
                            if let Some(state) = channel_tasks.remove(&cid) {
                                match state {
                                    ChannelState::Stream { abort, .. } => abort.abort(),
                                    ChannelState::Datagram { available, .. } => {
                                        available.store(false, Ordering::Release);
                                    }
                                }
                            }
                            if let Some(tx) = pending_open.remove(&cid) {
                                match tx {
                                    PendingOpen::Stream(tx) => {
                                        let _ = tx.send(Err("channel closed before open".into()));
                                    }
                                    PendingOpen::Datagram(tx) => {
                                        let _ = tx.send(Err("channel closed before open".into()));
                                    }
                                }
                            }
                            // If the Session was already dropped and this was
                            // the last channel, there is nothing left to do.
                            if !session_alive && channel_tasks.is_empty() && pending_open.is_empty()
                            {
                                return Ok(());
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
                                for (_, tx) in pending_open.drain() {
                                    match tx {
                                        PendingOpen::Stream(tx) => {
                                            let _ = tx.send(Err("ICE disconnected".into()));
                                        }
                                        PendingOpen::Datagram(tx) => {
                                            let _ = tx.send(Err("ICE disconnected".into()));
                                        }
                                    }
                                }
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
            }
        };

        let deadline = tokio::time::Instant::from_std(timeout);
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
                    rtc.handle_input(Input::Receive(Instant::now(), receive))?;
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
                    rtc.handle_input(Input::Receive(Instant::now(), receive))?;
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
                    rtc.handle_input(Input::Receive(Instant::now(), receive))?;
                }
            }
            cmd = async {
                if session_alive { cmd_rx.recv().await } else { std::future::pending().await }
            } => {
                match cmd {
                    Some(DriveCmd::Open { label, reply }) => {
                        if pending_open.len() + channel_tasks.len() >= MAX_DATA_CHANNELS {
                            let _ = reply.send(Err("WebRTC DataChannel limit reached".to_owned()));
                            continue;
                        }
                        let mut changes = rtc.sdp_api();
                        let cid = changes.add_channel(label);
                        // For non-first channels apply() returns None — no SDP needed.
                        let _ = changes.apply();
                        pending_open.insert(cid, PendingOpen::Stream(reply));
                    }
                    Some(DriveCmd::OpenDatagram { reply }) => {
                        if pending_open.len() + channel_tasks.len() >= MAX_DATA_CHANNELS {
                            let _ = reply.send(Err("WebRTC DataChannel limit reached".to_owned()));
                            continue;
                        }
                        let mut changes = rtc.sdp_api();
                        let cid = changes.add_channel_with_config(ChannelConfig {
                            label: crate::DATAGRAM_CHANNEL_LABEL.to_owned(),
                            ordered: false,
                            reliability: Reliability::MaxRetransmits { retransmits: 0 },
                            ..ChannelConfig::default()
                        });
                        let _ = changes.apply();
                        pending_open.insert(cid, PendingOpen::Datagram(reply));
                    }
                    Some(DriveCmd::Close { id }) => {
                        if let Some(ChannelState::Stream { abort, .. }) =
                            channel_tasks.remove(&id)
                        {
                            abort.abort();
                        }
                        rtc.direct_api().close_data_channel(id);
                    }
                    None => {
                        // All Session handles dropped — no new channels will be
                        // requested, but keep running until existing channels close
                        // so in-flight data is not cut short (`connect()` drops
                        // the Session immediately after opening its one channel).
                        session_alive = false;
                        if channel_tasks.is_empty() && pending_open.is_empty() {
                            return Ok(());
                        }
                    }
                }
            }
            // app → DataChannel: forward to SCTP.  If the send
            // buffer is full, park and retry next poll_output cycle.
            app_msg = async {
                if pending_send.is_some() {
                    return std::future::pending().await;
                }
                app_data_rx.recv().await
            } => {
                if let Some((id, data, datagram)) = app_msg
                    && let Some(mut ch) = rtc.channel(id)
                    && !matches!(ch.write(true, &data), Ok(true))
                    && !datagram
                {
                            pending_send = Some((id, data));
                }
            }
            sig = async {
                if signaling_alive {
                    ws_read.next().await
                } else {
                    std::future::pending().await
                }
            } => {
                match sig {
                    Some(Ok(Message::Text(t))) => {
                        if t.len() > MAX_SIGNAL_TEXT_BYTES {
                            return Err("oversized signaling message".into());
                        }
                        if let Ok(m) = serde_json::from_str::<ServerMessage>(&t)
                            && m.msg_type == "signal"
                            && let Some(raw) = m.data
                        {
                            let Some(data) = signaling::open_sealed_data(&raw, &box_keys) else {
                                continue;
                            };
                            if let Some(candidate) = data.get("candidate")
                                && let Ok(c) =
                                    serde_json::from_value::<Candidate>(candidate.clone())
                            {
                                verbose!("remote ICE candidate (trickle): {c:?}");
                                rtc.add_remote_candidate(c);
                            }
                        }
                    }
                    None | Some(Err(_)) => {
                        signaling_alive = false;
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod channel_admission_tests {
    use super::*;

    #[test]
    fn bounds_messages_waiting_for_the_app_bridge() {
        let (sender, mut receiver) = mpsc::channel(BRIDGE_PENDING_MESSAGES);
        for marker in 0..BRIDGE_PENDING_MESSAGES {
            enqueue_channel_data(&sender, &[marker as u8]).unwrap();
        }
        assert!(matches!(
            enqueue_channel_data(&sender, &[0xff]),
            Err(mpsc::error::TrySendError::Full(_))
        ));
        for marker in 0..BRIDGE_PENDING_MESSAGES {
            assert_eq!(receiver.try_recv().unwrap(), vec![marker as u8]);
        }
    }

    #[test]
    fn rejects_one_oversized_message_before_allocating_it() {
        let (sender, mut receiver) = mpsc::channel(BRIDGE_PENDING_MESSAGES);
        let oversized = vec![0x5a; BRIDGE_CHUNK + 1];
        assert!(matches!(
            enqueue_channel_data(&sender, &oversized),
            Err(mpsc::error::TrySendError::Full(bytes)) if bytes.is_empty()
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        let exact = vec![0x3c; BRIDGE_CHUNK];
        enqueue_channel_data(&sender, &exact).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), exact);
    }
}
