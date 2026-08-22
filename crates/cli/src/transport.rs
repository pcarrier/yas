use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_FRAME_SIZE: usize = yas_wire::frame::HARD_MAX_WIRE_FRAME as usize;

pub enum Transport {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    #[cfg(windows)]
    NamedPipe(tokio::net::windows::named_pipe::NamedPipeClient),
    Tcp(tokio::net::TcpStream),
    Duplex(tokio::io::DuplexStream),
    WebRtc {
        stream: tokio::io::DuplexStream,
        datagram: DatagramTransport,
    },
    Web {
        reader: Box<dyn AsyncRead + Unpin + Send>,
        writer: Box<dyn AsyncWrite + Unpin + Send>,
        datagram: Option<DatagramTransport>,
    },
}

pub(crate) struct DatagramTransport {
    sender: DatagramSender,
    receiver: DatagramReceiver,
    session: DatagramSession,
    maximum: u32,
}

#[derive(Clone)]
enum DatagramSenderInner {
    WebRtc(yas_webrtc_forwarder::client::DatagramSender),
    WebTransport(Box<web_transport_quinn::Session>),
    CompositeProxy(yas_composite_transport::DatagramSender),
}

enum DatagramReceiverInner {
    WebRtc(yas_webrtc_forwarder::client::DatagramReceiver),
    WebTransport(Box<web_transport_quinn::Session>),
    CompositeProxy(yas_composite_transport::DatagramReceiver),
}

#[derive(Clone)]
pub(crate) struct DatagramSender {
    inner: DatagramSenderInner,
    available: Arc<AtomicBool>,
}

pub(crate) struct DatagramReceiver {
    inner: DatagramReceiverInner,
    available: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DatagramSend {
    Sent,
    Dropped,
    Closed,
}

pub(crate) enum DatagramSession {
    WebRtc {
        _session: yas_webrtc_forwarder::client::Session,
    },
    WebTransport {
        _session: Box<web_transport_quinn::Session>,
    },
    Proxy,
}

impl DatagramTransport {
    fn web_rtc(
        channel: yas_webrtc_forwarder::client::DatagramChannel,
        session: yas_webrtc_forwarder::client::Session,
    ) -> Self {
        let (sender, receiver) = channel.into_parts();
        let available = Arc::new(AtomicBool::new(true));
        Self {
            sender: DatagramSender {
                inner: DatagramSenderInner::WebRtc(sender),
                available: Arc::clone(&available),
            },
            receiver: DatagramReceiver {
                inner: DatagramReceiverInner::WebRtc(receiver),
                available,
            },
            session: DatagramSession::WebRtc { _session: session },
            maximum: yas_webrtc_forwarder::MAX_DATAGRAM_SIZE as u32,
        }
    }

    fn web_transport(session: web_transport_quinn::Session, maximum: u32) -> Self {
        let available = Arc::new(AtomicBool::new(true));
        Self {
            sender: DatagramSender {
                inner: DatagramSenderInner::WebTransport(Box::new(session.clone())),
                available: Arc::clone(&available),
            },
            receiver: DatagramReceiver {
                inner: DatagramReceiverInner::WebTransport(Box::new(session.clone())),
                available,
            },
            session: DatagramSession::WebTransport {
                _session: Box::new(session),
            },
            maximum,
        }
    }

    fn composite_proxy<S>(sideband: S, maximum: u32) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        const QUEUE: usize = 64;
        let (outbound, mut outbound_receiver, _) =
            yas_composite_transport::bounded_datagrams(QUEUE, maximum);
        let (inbound_sender, inbound, _) =
            yas_composite_transport::bounded_datagrams(QUEUE, maximum);
        let outbound_guard = outbound.clone();
        let inbound_guard = inbound_sender.clone();
        let (mut sideband_reader, mut sideband_writer) = tokio::io::split(sideband);
        tokio::spawn(async move {
            let outbound_pump = async {
                while let Some(frame) = outbound_receiver.recv().await {
                    if yas_composite_transport::write_datagram(
                        &mut sideband_writer,
                        &frame,
                        maximum,
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
            };
            let inbound_pump = async {
                while let Ok(frame) =
                    yas_composite_transport::read_datagram(&mut sideband_reader, maximum).await
                {
                    let _ = inbound_sender.try_send(frame);
                }
            };
            tokio::select! {
                () = outbound_pump => {}
                () = inbound_pump => {}
            }
            outbound_guard.disable();
            inbound_guard.disable();
        });
        let available = Arc::new(AtomicBool::new(true));
        Self {
            sender: DatagramSender {
                inner: DatagramSenderInner::CompositeProxy(outbound),
                available: Arc::clone(&available),
            },
            receiver: DatagramReceiver {
                inner: DatagramReceiverInner::CompositeProxy(inbound),
                available,
            },
            session: DatagramSession::Proxy,
            maximum,
        }
    }

    pub(crate) fn maximum(&self) -> u32 {
        self.maximum
    }

    pub(crate) fn into_parts(self) -> (DatagramSender, DatagramReceiver, DatagramSession) {
        (self.sender, self.receiver, self.session)
    }
}

impl DatagramSender {
    pub(crate) fn try_send(&self, frame: Vec<u8>) -> DatagramSend {
        if !self.available.load(Ordering::Acquire) {
            return DatagramSend::Closed;
        }
        match &self.inner {
            DatagramSenderInner::WebRtc(sender) => match sender.try_send(frame) {
                Ok(()) => DatagramSend::Sent,
                Err(_) if !sender.is_available() => {
                    self.available.store(false, Ordering::Release);
                    DatagramSend::Closed
                }
                Err(_) => DatagramSend::Dropped,
            },
            DatagramSenderInner::WebTransport(session) => {
                if session.send_datagram(bytes::Bytes::from(frame)).is_ok() {
                    DatagramSend::Sent
                } else {
                    self.available.store(false, Ordering::Release);
                    DatagramSend::Closed
                }
            }
            DatagramSenderInner::CompositeProxy(sender) => match sender.try_send(frame) {
                Ok(()) => DatagramSend::Sent,
                Err(_) if sender.is_closed() => {
                    self.available.store(false, Ordering::Release);
                    DatagramSend::Closed
                }
                Err(_) => DatagramSend::Dropped,
            },
        }
    }
}

impl DatagramReceiver {
    pub(crate) async fn recv(&mut self) -> Option<Vec<u8>> {
        let frame = match &mut self.inner {
            DatagramReceiverInner::WebRtc(receiver) => receiver.recv().await,
            DatagramReceiverInner::WebTransport(session) => session
                .read_datagram()
                .await
                .ok()
                .map(|bytes| bytes.to_vec()),
            DatagramReceiverInner::CompositeProxy(receiver) => receiver.recv().await,
        };
        if frame.is_none() {
            self.available.store(false, Ordering::Release);
        }
        frame
    }
}

#[cfg(unix)]
pub type HomeServerUid = u32;
#[cfg(windows)]
pub type HomeServerUid = ();

impl Transport {
    pub fn split(
        self,
    ) -> (
        Box<dyn AsyncRead + Unpin + Send>,
        Box<dyn AsyncWrite + Unpin + Send>,
    ) {
        let (reader, writer, _datagram) = self.split_with_datagram();
        (reader, writer)
    }

    pub(crate) fn split_with_datagram(
        self,
    ) -> (
        Box<dyn AsyncRead + Unpin + Send>,
        Box<dyn AsyncWrite + Unpin + Send>,
        Option<DatagramTransport>,
    ) {
        match self {
            #[cfg(unix)]
            Transport::Unix(s) => {
                let (r, w) = tokio::io::split(s);
                (Box::new(r), Box::new(w), None)
            }
            #[cfg(windows)]
            Transport::NamedPipe(s) => {
                let (r, w) = tokio::io::split(s);
                (Box::new(r), Box::new(w), None)
            }
            Transport::Tcp(s) => {
                let (r, w) = tokio::io::split(s);
                (Box::new(r), Box::new(w), None)
            }
            Transport::Duplex(s) => {
                let (r, w) = tokio::io::split(s);
                (Box::new(r), Box::new(w), None)
            }
            Transport::WebRtc { stream, datagram } => {
                let (r, w) = tokio::io::split(stream);
                (Box::new(r), Box::new(w), Some(datagram))
            }
            Transport::Web {
                reader,
                writer,
                datagram,
            } => (reader, writer, datagram),
        }
    }
}

pub use yas_webserver::config::default_local_socket;

pub async fn read_frame(r: &mut (impl AsyncRead + Unpin)) -> Option<Vec<u8>> {
    let mut hdr = [0u8; 4];
    r.read_exact(&mut hdr).await.ok()?;
    let len = u32::from_le_bytes(hdr) as usize;
    if len == 0 {
        return Some(vec![]);
    }
    if len > MAX_FRAME_SIZE {
        return None;
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await.ok()?;
    Some(buf)
}

pub fn make_frame(payload: &[u8]) -> Vec<u8> {
    debug_assert!(payload.len() <= u32::MAX as usize);
    let mut v = Vec::with_capacity(4 + payload.len());
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

pub async fn write_frame(w: &mut (impl AsyncWrite + Unpin), payload: &[u8]) -> bool {
    w.write_all(&make_frame(payload)).await.is_ok()
}

pub async fn connect_ipc(path: &str) -> Result<Transport, String> {
    #[cfg(unix)]
    {
        Ok(Transport::Unix(
            tokio::net::UnixStream::connect(path)
                .await
                .map_err(|e| format!("cannot connect to {path}: {e}"))?,
        ))
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        Ok(Transport::NamedPipe(
            ClientOptions::new()
                .open(path)
                .map_err(|e| format!("cannot connect to {path}: {e}"))?,
        ))
    }
}

/// Connect the fixed native YAS home socket and authenticate its kernel peer
/// identity before returning a transport that can carry protocol bytes.
/// Explicit `socket:` connections retain their own trust model and continue
/// to use [`connect_ipc`].
pub async fn connect_home_ipc(
    path: &str,
    expected_server_uid: HomeServerUid,
) -> Result<Transport, String> {
    let transport = connect_ipc(path).await?;
    #[cfg(unix)]
    match &transport {
        Transport::Unix(stream) => {
            yas_webserver::local_ipc::verify_peer_uid(stream, expected_server_uid)
                .map_err(|error| format!("refusing native home server at {path}: {error}"))?;
        }
        _ => unreachable!("Unix IPC connector returned a non-Unix transport"),
    }
    #[cfg(windows)]
    let _ = expected_server_uid;
    Ok(transport)
}

// ---------------------------------------------------------------------------
// yas-proxy integration
// ---------------------------------------------------------------------------

/// The socket path of the single shared yas-proxy process.
/// Matches `proxy_socket_path()` in `crates/proxy/src/lib.rs`.
pub fn proxy_socket_path() -> String {
    yas_proxy::proxy_socket_path()
}

/// Ensure a yas-proxy daemon is running.  Returns the socket/pipe path.
///
/// If no live proxy is found, re-execs the current binary as
/// `yas proxy-daemon` in a detached background process so it outlives
/// the calling CLI invocation.
pub async fn ensure_proxy() -> Result<String, String> {
    let exe = yas_proxy::yas_exe();
    yas_proxy::ensure_proxy(&exe, true).await
}

/// Send a `shutdown\n` command to a running yas-proxy, causing it to exit.
/// Silently does nothing if no proxy is running.
pub async fn stop_proxy() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[cfg(unix)]
    {
        let sock = proxy_socket_path();
        let Ok(mut stream) = yas_proxy::connect_proxy(&sock).await else {
            return;
        };
        if stream.write_all(b"shutdown\n").await.is_err() {
            return;
        }
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reader.read_line(&mut line),
        )
        .await;
    }

    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        let sock = proxy_socket_path();
        let Ok(mut stream) = ClientOptions::new().open(&sock) else {
            return;
        };
        if stream.write_all(b"shutdown\n").await.is_err() {
            return;
        }
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reader.read_line(&mut line),
        )
        .await;
    }
}

const PROXY_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

async fn read_proxy_handshake_line<S: AsyncRead + Unpin>(stream: &mut S) -> Result<String, String> {
    let mut buf = Vec::with_capacity(64);
    let mut byte = [0u8; 1];
    loop {
        stream
            .read_exact(&mut byte)
            .await
            .map_err(|error| format!("yas-proxy: handshake read: {error}"))?;
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 4096 {
            return Err("yas-proxy: handshake response too long".into());
        }
    }
    Ok(String::from_utf8_lossy(&buf)
        .trim_end_matches('\r')
        .to_string())
}

#[cfg(unix)]
async fn connect_via_native_proxy_at(
    socket: &str,
    upstream_uri: &str,
    expected_uid: u32,
) -> Result<Transport, String> {
    let mut stream = yas_proxy::connect_proxy_with_uid(socket, expected_uid).await?;
    let message = format!("target-yas {upstream_uri}\n");
    stream
        .write_all(message.as_bytes())
        .await
        .map_err(|error| format!("yas-proxy: handshake write: {error}"))?;
    let response = tokio::time::timeout(
        PROXY_HANDSHAKE_TIMEOUT,
        read_proxy_handshake_line(&mut stream),
    )
    .await
    .map_err(|_| format!("yas-proxy: timed out connecting to {upstream_uri}"))??;
    if response == "ok" {
        Ok(Transport::Unix(stream))
    } else if let Some(message) = response.strip_prefix("error ") {
        Err(format!("yas-proxy: {message}"))
    } else {
        Err(format!("yas-proxy: unexpected response: {response:?}"))
    }
}

/// Connect through the shared proxy while requiring its YAS-aware upstream
/// selector. In particular, this resolves SSH to the canonical socket and
/// negotiates `yas.v1` for WebSocket.
pub async fn connect_via_native_proxy(upstream_uri: &str) -> Result<Transport, String> {
    let socket = ensure_proxy().await?;

    #[cfg(unix)]
    {
        return connect_via_native_proxy_at(
            &socket,
            upstream_uri,
            yas_proxy::expected_proxy_uid()?,
        )
        .await;
    }

    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        let message = format!("target-yas {upstream_uri}\n");
        let mut stream = ClientOptions::new()
            .open(&socket)
            .map_err(|error| format!("yas-proxy: connect to {socket}: {error}"))?;
        stream
            .write_all(message.as_bytes())
            .await
            .map_err(|error| format!("yas-proxy: handshake write: {error}"))?;
        let response = tokio::time::timeout(
            PROXY_HANDSHAKE_TIMEOUT,
            read_proxy_handshake_line(&mut stream),
        )
        .await
        .map_err(|_| format!("yas-proxy: timed out connecting to {upstream_uri}"))??;
        if response == "ok" {
            return Ok(Transport::NamedPipe(stream));
        }
        if let Some(message) = response.strip_prefix("error ") {
            return Err(format!("yas-proxy: {message}"));
        }
        return Err(format!("yas-proxy: unexpected response: {response:?}"));
    }

    #[allow(unreachable_code)]
    Err("yas-proxy: unsupported platform".into())
}

async fn request_composite_proxy<S>(
    stream: &mut S,
    upstream_uri: &str,
) -> Result<(u32, yas_composite_transport::Token), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let message = format!("target-yas-composite {upstream_uri}\n");
    stream
        .write_all(message.as_bytes())
        .await
        .map_err(|error| format!("yas-proxy: composite handshake write: {error}"))?;
    let response = tokio::time::timeout(PROXY_HANDSHAKE_TIMEOUT, read_proxy_handshake_line(stream))
        .await
        .map_err(|_| "yas-proxy: timed out establishing composite upstream".to_owned())??;
    if let Some(fields) = response.strip_prefix("ok composite ") {
        let Some((raw_maximum, raw_token)) = fields.split_once(' ') else {
            return Err(format!(
                "yas-proxy: invalid composite response: {response:?}"
            ));
        };
        let maximum = raw_maximum
            .parse::<u32>()
            .map_err(|_| format!("yas-proxy: invalid composite response: {response:?}"))?
            .min(yas_wire::frame::HARD_MAX_DATAGRAM);
        if maximum
            < u32::try_from(yas_wire::schema::transport::EVENT_HEADER_BYTES)
                .expect("YAS Event header size fits u32")
        {
            return Err(format!(
                "yas-proxy: unusable composite datagram limit {maximum}"
            ));
        }
        let token = yas_proxy::decode_proxy_side_token(raw_token)
            .ok_or_else(|| format!("yas-proxy: invalid composite response: {response:?}"))?;
        return Ok((maximum, token));
    }
    if let Some(message) = response.strip_prefix("error ") {
        return Err(format!("yas-proxy: {message}"));
    }
    Err(format!(
        "yas-proxy: unexpected composite response: {response:?}"
    ))
}

async fn finish_composite_proxy_side<S>(
    stream: &mut S,
    token: yas_composite_transport::Token,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let message = format!(
        "target-yas-datagram {}\n",
        yas_proxy::encode_proxy_side_token(token)
    );
    stream
        .write_all(message.as_bytes())
        .await
        .map_err(|error| format!("yas-proxy: datagram handshake write: {error}"))?;
    let response = tokio::time::timeout(PROXY_HANDSHAKE_TIMEOUT, read_proxy_handshake_line(stream))
        .await
        .map_err(|_| "yas-proxy: timed out attaching datagram sideband".to_owned())??;
    if response == "ok" {
        return Ok(());
    }
    if let Some(message) = response.strip_prefix("error ") {
        return Err(format!("yas-proxy: {message}"));
    }
    Err(format!(
        "yas-proxy: unexpected datagram response: {response:?}"
    ))
}

#[cfg(unix)]
async fn connect_via_composite_proxy_at(
    socket: &str,
    upstream_uri: &str,
) -> Result<Transport, String> {
    let expected_uid = yas_proxy::expected_proxy_uid()?;
    let mut main = yas_proxy::connect_proxy_with_uid(socket, expected_uid).await?;
    let (maximum, token) = request_composite_proxy(&mut main, upstream_uri).await?;
    let mut sideband = yas_proxy::connect_proxy_with_uid(socket, expected_uid).await?;
    finish_composite_proxy_side(&mut sideband, token).await?;
    let (reader, writer) = tokio::io::split(main);
    Ok(Transport::Web {
        reader: Box::new(reader),
        writer: Box::new(writer),
        datagram: Some(DatagramTransport::composite_proxy(sideband, maximum)),
    })
}

#[cfg(windows)]
async fn connect_via_composite_proxy_at(
    socket: &str,
    upstream_uri: &str,
) -> Result<Transport, String> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let mut main = ClientOptions::new()
        .open(socket)
        .map_err(|error| format!("yas-proxy: connect to {socket}: {error}"))?;
    let (maximum, token) = request_composite_proxy(&mut main, upstream_uri).await?;
    let mut sideband = ClientOptions::new()
        .open(socket)
        .map_err(|error| format!("yas-proxy: connect to {socket}: {error}"))?;
    finish_composite_proxy_side(&mut sideband, token).await?;
    let (reader, writer) = tokio::io::split(main);
    Ok(Transport::Web {
        reader: Box::new(reader),
        writer: Box::new(writer),
        datagram: Some(DatagramTransport::composite_proxy(sideband, maximum)),
    })
}

/// Obtain a fresh reliable+datagram YAS channel pair from a persistent proxy
/// WebRTC session. Two authenticated local sockets keep the reliable byte
/// stream and lossy, message-preserving datagram lane independent.
pub async fn connect_via_composite_proxy(upstream_uri: &str) -> Result<Transport, String> {
    #[cfg(any(unix, windows))]
    {
        let socket = ensure_proxy().await?;
        let first = connect_via_composite_proxy_at(&socket, upstream_uri).await;
        let incompatible = matches!(
            first.as_ref(),
            Err(error) if error == "yas-proxy: invalid native handshake"
        );
        #[cfg(unix)]
        let replaceable =
            yas_proxy::expected_proxy_uid()? == yas_webserver::local_ipc::effective_uid();
        #[cfg(windows)]
        let replaceable = true;
        if !incompatible || !replaceable {
            return first;
        }

        // A daemon started by an older binary has no composite handshake.
        // Replace only a same-user daemon, once, and then use the current
        // executable. This is an upgrade path for the daemon, not wire
        // compatibility with old clients.
        stop_proxy().await;
        for _ in 0..100 {
            if !yas_proxy::proxy_alive(&socket).await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let socket = ensure_proxy().await?;
        return connect_via_composite_proxy_at(&socket, upstream_uri).await;
    }

    #[cfg(not(any(unix, windows)))]
    Err("yas-proxy: unsupported platform".into())
}

/// Connect to a YAS endpoint using its explicit transport selector.
///
/// SSH resolves the canonical YAS socket and WebSocket negotiates `yas.v1`.
/// Keeping that choice out of the byte stream prevents protocol sniffing.
pub async fn connect_native_uri(uri: &str, hub: &str) -> Result<Transport, String> {
    Box::pin(connect_native_uri_inner(
        uri,
        hub,
        std::collections::HashSet::new(),
    ))
    .await
}

async fn connect_native_uri_inner(
    uri: &str,
    hub: &str,
    mut visited: std::collections::HashSet<String>,
) -> Result<Transport, String> {
    if let Some(upstream) = uri.strip_prefix("proxy:") {
        return connect_via_native_proxy(upstream).await;
    }

    if let Some(rest) = uri.strip_prefix("ssh:") {
        if proxy_enabled() {
            return connect_via_native_proxy(uri).await;
        }
        let (user, host, socket) = yas_ssh::parse_ssh_uri(rest);
        let pool = yas_ssh::SshPool::new();
        let stream = pool
            .connect_yas(&host, user.as_deref(), socket.as_deref())
            .await
            .map_err(|error| format!("ssh:{rest}: {error}"))?;
        return Ok(Transport::Duplex(stream));
    }
    if let Some(rest) = uri.strip_prefix("tcp:") {
        if proxy_enabled() {
            return connect_via_native_proxy(uri).await;
        }
        let stream = tokio::net::TcpStream::connect(rest)
            .await
            .map_err(|error| format!("cannot connect to {rest}: {error}"))?;
        let _ = stream.set_nodelay(true);
        return Ok(Transport::Tcp(stream));
    }
    if uri.starts_with("wt://") {
        // The proxy's generic target-yas command is one reliable byte stream
        // and cannot retain WebTransport's independent datagrams. Connect
        // explicit WT targets in-process even when proxying is enabled.
        return connect_native_webtransport(uri).await;
    }
    if uri.starts_with("ws://") || uri.starts_with("wss://") {
        if proxy_enabled() {
            return connect_via_native_proxy(uri).await;
        }
        return connect_native_upstream(uri).await;
    }
    if uri.starts_with("uplink:") {
        return Err(format!(
            "{uri}: native YAS uplink attachment requires a YAS-aware uplink endpoint"
        ));
    }
    if let Some(path) = uri.strip_prefix("socket:") {
        return connect_ipc(path).await;
    }
    if let Some(target) = uri.strip_prefix("share:") {
        let has_explicit_hub = target
            .split_once('?')
            .is_some_and(|(_, query)| query.split('&').any(|item| item.starts_with("hub=")));
        if proxy_enabled() {
            // The proxy retains one ICE/DTLS/SCTP session and gives each CLI
            // invocation fresh paired reliable and unreliable DataChannels.
            // The local proxy connection also keeps those lanes separate.
            let proxy_uri = share_proxy_uri(target, hub);
            return connect_via_composite_proxy(&proxy_uri).await;
        }
        let (passphrase, uri_hub) = yas_proxy::parse_share_uri(target);
        let hub = if has_explicit_hub {
            uri_hub
        } else {
            yas_webrtc_forwarder::normalize_hub(hub)
        };
        let (session, _stream_handle, stream, channel) =
            yas_webrtc_forwarder::client::Session::establish_composite(&passphrase, &hub)
                .await
                .map_err(|error| format!("share connection: {error}"))?;
        return Ok(Transport::WebRtc {
            stream,
            datagram: DatagramTransport::web_rtc(channel, session),
        });
    }
    if uri == "local" {
        let path = yas_webserver::config::default_yas_socket();
        ensure_local_server(&path).await?;
        return connect_native_home(&path).await;
    }
    if let Some(raw_name) = uri.strip_prefix("local:") {
        let name: yas_server::ServerName = raw_name
            .parse()
            .map_err(|error| format!("invalid local server name: {error}"))?;
        let path = yas_webserver::config::yas_socket_for_name(name.as_str());
        ensure_local_server_with_name(&path, Some(&name)).await?;
        return connect_native_home(&path).await;
    }

    let entries = yas_webserver::config::read_remotes();
    if let Some((_, target_uri)) = entries.into_iter().find(|(name, _)| name == uri) {
        if !visited.insert(uri.to_string()) {
            return Err(format!("yas.remotes: cycle detected resolving '{uri}'"));
        }
        return Box::pin(connect_native_uri_inner(&target_uri, hub, visited)).await;
    }
    Err(format!(
        "unknown target '{uri}' \
         (expected ssh:, tcp:, ws://, wss://, wt://, socket:, share:, proxy:, local[:NAME], \
          or a name from yas.remotes)"
    ))
}

async fn connect_native_upstream(uri: &str) -> Result<Transport, String> {
    let (mut upstream_reader, mut upstream_writer) =
        yas_proxy::connect_yas_upstream_split(uri).await?;
    let (local, remote) = tokio::io::duplex(1 << 16);
    let (mut remote_reader, mut remote_writer) = tokio::io::split(remote);
    tokio::spawn(async move {
        let _ = tokio::io::copy(&mut upstream_reader, &mut remote_writer).await;
    });
    tokio::spawn(async move {
        let _ = tokio::io::copy(&mut remote_reader, &mut upstream_writer).await;
    });
    Ok(Transport::Duplex(local))
}

async fn connect_native_webtransport(uri: &str) -> Result<Transport, String> {
    let connection = yas_proxy::connect_yas_webtransport(uri).await?;
    let (reader, writer, session) = connection.into_parts();
    let maximum = session
        .max_datagram_size()
        .min(yas_wire::frame::HARD_MAX_DATAGRAM as usize);
    let datagram = if maximum >= yas_wire::schema::transport::EVENT_HEADER_BYTES {
        Some(DatagramTransport::web_transport(
            session,
            u32::try_from(maximum).expect("YAS hard maximum fits u32"),
        ))
    } else {
        None
    };
    Ok(Transport::Web {
        reader,
        writer,
        datagram,
    })
}

async fn connect_native_home(path: &str) -> Result<Transport, String> {
    #[cfg(unix)]
    let expected_uid = yas_webserver::local_ipc::expected_server_uid()?;
    #[cfg(windows)]
    let expected_uid = ();
    connect_home_ipc(path, expected_uid).await
}

/// Returns true when the proxy should be used automatically.
/// Disabled by setting `YAS_PROXY=0`.
pub fn proxy_enabled() -> bool {
    std::env::var("YAS_PROXY").ok().as_deref() != Some("0")
}

/// Preserve the CLI's selected signaling hub across the local proxy
/// handshake. The proxy is a separate process and cannot otherwise observe a
/// per-invocation `--hub`; an explicit hub in the share URI remains
/// authoritative.
fn share_proxy_uri(target: &str, hub: &str) -> String {
    let has_explicit_hub = target
        .split_once('?')
        .is_some_and(|(_, query)| query.split('&').any(|item| item.starts_with("hub=")));
    if has_explicit_hub {
        return format!("share:{target}");
    }
    let separator = if target.contains('?') { '&' } else { '?' };
    let hub = yas_webrtc_forwarder::normalize_hub(hub);
    let encoded_hub: String = url::form_urlencoded::byte_serialize(hub.as_bytes()).collect();
    format!("share:{target}{separator}hub={encoded_hub}")
}

/// Return the configured default target URI, if any.
///
/// Precedence: `YAS_TARGET` env var > `yas.target` key in `yas.conf`.
/// Returns `None` if neither is set, meaning fall back to local.
pub fn default_target() -> Option<String> {
    if let Ok(v) = std::env::var("YAS_TARGET")
        && !v.is_empty()
    {
        return Some(v);
    }
    let config = yas_webserver::config::read_config();
    config.get("yas.target").cloned()
}

/// Resolve the configured target and connect to its canonical YAS endpoint.
pub async fn connect_native(on: &Option<String>, hub: &str) -> Result<Transport, String> {
    let effective_target = on.clone().or_else(default_target);
    if let Some(uri) = effective_target {
        return connect_native_uri(&uri, hub).await;
    }

    let path = yas_webserver::config::default_yas_socket();
    ensure_local_server(&path).await?;
    connect_native_home(&path).await
}

/// Connect to the local server, spawning it as a **detached process**
/// if absent. In-process hosting (the old behavior) breaks every
/// daemon-resident feature for one-shot commands: warm LSP backends
/// (docs/design/lsp.md "Sessions and discovery"), surviving PTYs — all
/// died with each short-lived CLI invocation. The spawned `yas server`
/// outlives us and is shared by later invocations; `yas quit` shuts it
/// down.
#[cfg(any(unix, windows))]
pub async fn ensure_local_server(socket_path: &str) -> Result<(), String> {
    ensure_local_server_with_name(socket_path, None).await
}

pub(crate) async fn ensure_local_server_with_name(
    socket_path: &str,
    name: Option<&yas_server::ServerName>,
) -> Result<(), String> {
    if local_server_alive(socket_path).await {
        return Ok(());
    }
    // A socket file nobody answers on is a leftover from a dead server;
    // the fresh one must be able to bind.
    #[cfg(unix)]
    if std::path::Path::new(socket_path).exists() {
        let _ = std::fs::remove_file(socket_path);
    }
    let mut spawned = spawn_detached_server(socket_path, name)?;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Another concurrent auto-start may have won the bind while our
        // child exited. A live endpoint is success regardless of which child
        // created it, so check availability before reporting our exit.
        if local_server_alive(socket_path).await {
            return Ok(());
        }
        match spawned.try_wait() {
            Ok(Some(status)) => {
                if local_server_alive(socket_path).await {
                    return Ok(());
                }
                return Err(spawned.exit_error(status));
            }
            Ok(None) => {}
            Err(error) => return Err(format!("cannot monitor yas server startup: {error}")),
        }
    }
    if local_server_alive(socket_path).await {
        return Ok(());
    }
    match spawned.try_wait() {
        Ok(Some(status)) => Err(spawned.exit_error(status)),
        Ok(None) => Err(spawned.timeout_error()),
        Err(error) => Err(format!("cannot monitor yas server startup: {error}")),
    }
}

async fn local_server_alive(path: &str) -> bool {
    #[cfg(unix)]
    {
        tokio::net::UnixStream::connect(path).await.is_ok()
    }
    #[cfg(windows)]
    {
        connect_ipc(path).await.is_ok()
    }
}

struct SpawnedServer {
    child: Option<std::process::Child>,
}

impl SpawnedServer {
    fn monitor(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.as_mut().expect("child already taken").try_wait()
    }

    fn exit_error(self, status: std::process::ExitStatus) -> String {
        format!("server exited before accepting connections ({status})")
    }

    fn timeout_error(&self) -> String {
        "server did not accept connections within 5 seconds \
         (process was still running when last checked)"
            .to_string()
    }
}

impl Drop for SpawnedServer {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // The server is detached, but it still needs reaping if it exits
            // while this CLI remains alive.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

/// Spawn `yas server --socket <path>` detached from this process's
/// session, stdio to the void. Configuration flows through inherited
/// `YAS_*`/`SHELL` env vars, which the server command reads itself —
/// `YAS_PASSPHRASE` excepted, which is not the server's to hold.
fn spawn_detached_server(
    socket_path: &str,
    name: Option<&yas_server::ServerName>,
) -> Result<SpawnedServer, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate yas executable: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("server");
    if let Some(name) = name {
        cmd.arg("--name").arg(name.as_str());
    }
    cmd.arg("--socket")
        .arg(socket_path)
        // One-shot fs/git/lsp use never touches a surface; skip the
        // compositor/VAAPI bring-up the daemon would otherwise pay for.
        .env("YAS_SKIP_COMPOSITOR", "1")
        // The passphrase belongs to whoever authenticates browsers: the edge,
        // or `yas share`, which reads it immediately before autostarting this
        // child. No server reads it. And `ENV_GET` (docs/design/env.md) hands a
        // server's whole environment to any client that can reach the family, so
        // inheriting it would publish the credential of the process that spawned
        // it — a `yas share` link's passphrase, to everyone already through the
        // link.
        .env_remove("YAS_PASSPHRASE")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // New session: no controlling terminal, so the daemon survives
        // terminal hangup and this CLI's exit.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot start yas server: {e}"))?;
    Ok(SpawnedServer::monitor(child))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn embedded_edge_home_connector_rejects_wrong_uid_prebind_before_bytes() {
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
        let error = match connect_home_ipc(socket.to_str().unwrap(), expected_uid).await {
            Ok(_) => panic!("wrong-UID prebind was accepted"),
            Err(error) => error,
        };
        assert!(error.contains("does not match expected UID"), "{error}");
        assert_eq!(accepted.await.unwrap(), 0, "protocol bytes reached prebind");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn proxy_connector_rejects_wrong_uid_before_target_credentials() {
        // Root is a trusted peer for every endpoint, so a root test runner
        // cannot exercise this rejection.
        if yas_webserver::local_ipc::effective_uid() == 0 {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("prebound-proxy.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let accepted = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0u8; 1];
            stream.read(&mut byte).await.unwrap()
        });

        let expected_uid = yas_webserver::local_ipc::effective_uid() ^ 1;
        let error = match connect_via_native_proxy_at(
            socket.to_str().unwrap(),
            "wt://secret.example:4433/#credential",
            expected_uid,
        )
        .await
        {
            Ok(_) => panic!("wrong-UID proxy prebind was accepted"),
            Err(error) => error,
        };
        assert!(error.contains("does not match expected UID"), "{error}");
        assert_eq!(accepted.await.unwrap(), 0, "target URI reached prebind");
    }

    #[tokio::test]
    async fn monitored_child_reports_early_exit() {
        #[cfg(unix)]
        let mut command = {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "exit 23"]);
            command
        };
        #[cfg(windows)]
        let mut command = {
            let mut command = std::process::Command::new("cmd.exe");
            command.args(["/C", "exit /b 23"]);
            command
        };
        command.stderr(std::process::Stdio::null());
        let mut spawned = SpawnedServer::monitor(command.spawn().unwrap());
        let status = loop {
            if let Some(status) = spawned.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };

        let error = spawned.exit_error(status);
        assert!(error.contains("server exited before accepting connections"));
        assert!(error.contains("23"));
        assert!(!status.success());
    }

    #[test]
    fn share_proxy_target_carries_the_selected_hub() {
        assert_eq!(
            share_proxy_uri("secret", "signal.example"),
            "share:secret?hub=wss%3A%2F%2Fsignal.example"
        );
        assert_eq!(
            share_proxy_uri("secret?mode=rw", "wss://signal.example/path"),
            "share:secret?mode=rw&hub=wss%3A%2F%2Fsignal.example%2Fpath"
        );
    }

    #[test]
    fn explicit_share_hub_is_not_replaced() {
        assert_eq!(
            share_proxy_uri(
                "secret?hub=wss://chosen.example/path",
                "wss://ignored.example"
            ),
            "share:secret?hub=wss://chosen.example/path"
        );
    }

    #[tokio::test]
    async fn composite_proxy_handshake_negotiates_both_lanes() {
        let token = [0x5a; yas_composite_transport::TOKEN_BYTES];
        let encoded = yas_proxy::encode_proxy_side_token(token);
        let (mut client, mut proxy) = tokio::io::duplex(512);
        let request = async {
            request_composite_proxy(&mut client, "share:secret")
                .await
                .unwrap()
        };
        let response = async {
            assert_eq!(
                read_proxy_handshake_line(&mut proxy).await.unwrap(),
                "target-yas-composite share:secret"
            );
            proxy
                .write_all(format!("ok composite 1200 {encoded}\n").as_bytes())
                .await
                .unwrap();
        };
        let ((maximum, received_token), ()) = tokio::join!(request, response);
        assert_eq!(maximum, 1200);
        assert_eq!(received_token, token);

        let (mut client, mut proxy) = tokio::io::duplex(512);
        let request = async {
            finish_composite_proxy_side(&mut client, token)
                .await
                .unwrap()
        };
        let response = async {
            assert_eq!(
                read_proxy_handshake_line(&mut proxy).await.unwrap(),
                format!("target-yas-datagram {encoded}")
            );
            proxy.write_all(b"ok\n").await.unwrap();
        };
        let ((), ()) = tokio::join!(request, response);
    }

    #[tokio::test]
    async fn composite_proxy_datagrams_remain_message_preserving() {
        const MAXIMUM: u32 = 1200;
        let (sideband, mut proxy) = tokio::io::duplex(4096);
        let transport = DatagramTransport::composite_proxy(sideband, MAXIMUM);
        let (sender, mut receiver, _session) = transport.into_parts();

        assert_eq!(sender.try_send(b"outbound".to_vec()), DatagramSend::Sent);
        assert_eq!(
            yas_composite_transport::read_datagram(&mut proxy, MAXIMUM)
                .await
                .unwrap(),
            b"outbound"
        );

        yas_composite_transport::write_datagram(&mut proxy, b"inbound", MAXIMUM)
            .await
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap(), b"inbound");
    }

    // ── make_frame ──

    #[test]
    fn make_frame_empty_payload() {
        let frame = make_frame(&[]);
        assert_eq!(frame, vec![0, 0, 0, 0]);
    }

    #[test]
    fn make_frame_known_payload() {
        let frame = make_frame(b"hello");
        assert_eq!(frame.len(), 9);
        assert_eq!(&frame[0..4], &5u32.to_le_bytes());
        assert_eq!(&frame[4..], b"hello");
    }

    #[test]
    fn make_frame_single_byte() {
        let frame = make_frame(&[0xff]);
        assert_eq!(&frame[0..4], &1u32.to_le_bytes());
        assert_eq!(frame[4], 0xff);
    }

    // ── read_frame + make_frame round-trip ──

    #[tokio::test]
    async fn read_frame_round_trip() {
        let payload = b"yas protocol test";
        let frame = make_frame(payload);
        let mut cursor = std::io::Cursor::new(frame);
        let result = read_frame(&mut cursor).await.unwrap();
        assert_eq!(result, payload);
    }

    #[tokio::test]
    async fn read_frame_empty_payload() {
        let frame = make_frame(&[]);
        let mut cursor = std::io::Cursor::new(frame);
        let result = read_frame(&mut cursor).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn read_frame_rejects_oversized() {
        let len = (MAX_FRAME_SIZE as u32 + 1).to_le_bytes();
        let mut cursor = std::io::Cursor::new(len.to_vec());
        let result = read_frame(&mut cursor).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_frame_eof_during_header() {
        let mut cursor = std::io::Cursor::new(vec![0x01, 0x00]);
        let result = read_frame(&mut cursor).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_frame_eof_during_body() {
        let mut data = 10u32.to_le_bytes().to_vec();
        data.extend_from_slice(b"short");
        let mut cursor = std::io::Cursor::new(data);
        let result = read_frame(&mut cursor).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_frame_multiple_frames() {
        let mut data = make_frame(b"first");
        data.extend_from_slice(&make_frame(b"second"));
        let mut cursor = std::io::Cursor::new(data);
        let f1 = read_frame(&mut cursor).await.unwrap();
        let f2 = read_frame(&mut cursor).await.unwrap();
        assert_eq!(f1, b"first");
        assert_eq!(f2, b"second");
    }

    #[tokio::test]
    async fn write_frame_round_trip() {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let payload = b"write-test";
        let ok = write_frame(&mut client, payload).await;
        assert!(ok);
        drop(client);
        let result = read_frame(&mut server).await.unwrap();
        assert_eq!(result, payload);
    }
}
