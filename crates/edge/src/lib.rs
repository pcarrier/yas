use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, FromRequest, State, WebSocketUpgrade};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::serve::ListenerExt;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(unix)]
use tokio::net::UnixStream;

type BoxedReader = Box<dyn AsyncRead + Unpin + Send>;
type BoxedWriter = Box<dyn AsyncWrite + Unpin + Send>;

const WEBTRANSPORT_AUTH_TIMEOUT: Duration = Duration::from_secs(30);
const WEBTRANSPORT_PEER_TIMEOUT: Duration = Duration::from_secs(30);
const WEBTRANSPORT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
const WEBTRANSPORT_AUTH_MAX_BYTES: usize = 4 * 1024;
const WEBTRANSPORT_AUTH_OK: u8 = 1;
const WEBTRANSPORT_AUTH_REJECTED: u8 = 0;
const WEBTRANSPORT_AUTH_BUSY: u8 = 2;
const WEBTRANSPORT_PATH: &str = "/edge";
const WEBTRANSPORT_CONFIG_PATH: &str = "/edge-transport.json";

#[cfg(unix)]
type IpcStream = tokio::net::UnixStream;
#[cfg(windows)]
type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;
#[cfg(unix)]
type HomeServerUid = u32;
#[cfg(windows)]
type HomeServerUid = ();

async fn connect_ipc(path: &str) -> Result<IpcStream, String> {
    #[cfg(unix)]
    {
        UnixStream::connect(path)
            .await
            .map_err(|error| format!("cannot connect to {path}: {error}"))
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        ClientOptions::new()
            .open(path)
            .map_err(|error| format!("cannot connect to {path}: {error}"))
    }
}

/// Open the single local YAS server exposed at `/edge`.
async fn connect_home_ipc(
    path: &str,
    expected_server_uid: HomeServerUid,
) -> Result<(BoxedReader, BoxedWriter), String> {
    let stream = connect_ipc(path).await?;
    #[cfg(unix)]
    yas_webserver::local_ipc::verify_peer_uid(&stream, expected_server_uid)
        .map_err(|error| format!("refusing native home server at {path}: {error}"))?;
    #[cfg(windows)]
    let _ = expected_server_uid;
    let (reader, writer) = tokio::io::split(stream);
    Ok((Box::new(reader), Box::new(writer)))
}

const YAS_SUBPROTOCOL: &str = yas_wire::schema::transport::WEBSOCKET_SUBPROTOCOL;
const YAS_PREFACE: &[u8; 8] = &yas_wire::PREFACE;
const YAS_MAX_FRAME_SIZE: usize = yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME as usize;
const _: () = assert!(
    yas_wire::schema::transport::STREAM_LENGTH_BITS == u32::BITS as u8
        && yas_wire::schema::transport::STREAM_LENGTH_BYTES == size_of::<u32>()
);

const INDEX_HTML_BR: &[u8] = include_bytes!("../../../js/ui/dist/index.html.br");
const SW_JS_BR: &[u8] = include_bytes!("../../../js/ui/dist/sw.js.br");

static INDEX_ETAG: LazyLock<String> = LazyLock::new(|| yas_webserver::html_etag(INDEX_HTML_BR));
static SW_ETAG: LazyLock<String> = LazyLock::new(|| yas_webserver::html_etag(SW_JS_BR));

/// How a browser session reaches the home server.
///
/// Two answers, and the edge does not care which: the socket, for an edge
/// running as its own process, and a closure, for one the server hosts — where
/// the home server is this process and there is nothing to dial.
#[derive(Clone)]
pub enum Home {
    Ipc { socket: String, uid: HomeServerUid },
    Hosted(HostedHome),
}

/// Opens one session against a server in this process.
pub type HostedHome = Arc<
    dyn Fn() -> std::pin::Pin<
            Box<dyn Future<Output = Result<(BoxedReader, BoxedWriter), String>> + Send>,
        > + Send
        + Sync,
>;

impl Home {
    async fn connect(&self) -> Result<(BoxedReader, BoxedWriter), String> {
        match self {
            Self::Ipc { socket, uid } => connect_home_ipc(socket, *uid).await,
            Self::Hosted(open) => open().await,
        }
    }

    async fn connect_composite(&self, maximum: u32) -> Result<CompositeHome, String> {
        let (main_reader, mut main_writer) = self.connect().await?;
        let (datagram_reader, mut datagram_writer) = self.connect().await?;
        let mut token: yas_composite_transport::Token = rand::random();
        if token == [0; yas_composite_transport::TOKEN_BYTES] {
            token[0] = 1;
        }
        let main_offer = yas_composite_transport::Offer::new(
            yas_composite_transport::Role::Main,
            token,
            maximum,
        )
        .map_err(|error| format!("invalid composite main offer: {error}"))?;
        let datagram_offer = yas_composite_transport::Offer::new(
            yas_composite_transport::Role::Datagram,
            token,
            maximum,
        )
        .map_err(|error| format!("invalid composite datagram offer: {error}"))?;
        yas_composite_transport::write_offer(&mut main_writer, main_offer)
            .await
            .map_err(|error| format!("cannot select composite home stream: {error}"))?;
        yas_composite_transport::write_offer(&mut datagram_writer, datagram_offer)
            .await
            .map_err(|error| format!("cannot select home datagram sideband: {error}"))?;
        Ok(CompositeHome {
            main_reader,
            main_writer,
            datagram_reader,
            datagram_writer,
        })
    }
}

struct CompositeHome {
    main_reader: BoxedReader,
    main_writer: BoxedWriter,
    datagram_reader: BoxedReader,
    datagram_writer: BoxedWriter,
}

/// Native WebTransport listener paired with an edge's WebSocket listener.
///
/// The certificate and key are PEM files. When both are omitted, the edge
/// creates a short-lived self-signed certificate and advertises its SHA-256
/// hash to the UI. Production deployments should use stable certificate files
/// so open pages can reconnect after a service restart.
#[derive(Clone, Debug)]
pub struct WebTransportOptions {
    pub addr: String,
    pub public_port: u16,
    pub certificate: Option<PathBuf>,
    pub private_key: Option<PathBuf>,
    pub pin_certificate: bool,
}

/// What the edge needs to serve, however it was started.
pub struct Options {
    pub passphrase: yas_webserver::config::AuthPassphrase,
    /// Listen address. Plaintext `ws://`, and the passphrase it accepts is full
    /// authority over the home server, so anything but loopback wants a TLS
    /// reverse proxy in front of it.
    pub addr: String,
    pub home: Home,
    pub trusted_proxy_ips: HashSet<IpAddr>,
    pub web_transport: Option<WebTransportOptions>,
}

#[derive(Clone)]
struct WebTransportAdvertisement {
    public_port: u16,
    certificate_hash: Option<[u8; 32]>,
}

struct Config {
    passphrase: yas_webserver::config::AuthPassphrase,
    home: Home,
    shutdown: Arc<tokio::sync::Notify>,
    auth_throttle: yas_webserver::config::AuthThrottle,
    trusted_proxy_ips: HashSet<IpAddr>,
    web_transport: Option<WebTransportAdvertisement>,
}

type AppState = Arc<Config>;

const INTERACTIVE_TOS: u32 = 34 << 2;
#[cfg(target_os = "linux")]
const TCP_NOTSENT_LOWAT: u32 = 64 * 1024;

fn configure_browser_tcp(stream: &mut tokio::net::TcpStream) {
    let _ = stream.set_nodelay(true);
    let socket = socket2::SockRef::from(&*stream);
    let _ = socket.set_tos_v4(INTERACTIVE_TOS);
    #[cfg(unix)]
    let _ = socket.set_tclass_v6(INTERACTIVE_TOS);
    #[cfg(target_os = "linux")]
    {
        let _ = socket.set_priority(4);
        let _ = socket.set_tcp_notsent_lowat(TCP_NOTSENT_LOWAT);
    }
}

async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> Option<Vec<u8>> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length).await.ok()?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > YAS_MAX_FRAME_SIZE {
        return None;
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload).await.ok()?;
    Some(payload)
}

async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), payload: &[u8]) -> bool {
    let Ok(length) = u32::try_from(payload.len()) else {
        return false;
    };
    if payload.is_empty() || payload.len() > YAS_MAX_FRAME_SIZE {
        return false;
    }
    writer.write_all(&length.to_le_bytes()).await.is_ok() && writer.write_all(payload).await.is_ok()
}

/// Run the YAS edge. It serves the browser and exposes one local native YAS
/// server through the authenticated `/edge` WebSocket endpoint.
pub async fn run() {
    let passphrase_raw = passphrase_from_env().unwrap_or_else(|| {
        eprintln!("yas edge: YAS_PASSPHRASE environment variable required");
        std::process::exit(1);
    });
    let passphrase = yas_webserver::config::AuthPassphrase::from_env_value(passphrase_raw)
        .unwrap_or_else(|error| {
            eprintln!("yas edge: {error}");
            std::process::exit(1);
        });
    let home_socket = yas_webserver::config::default_yas_socket();
    #[cfg(unix)]
    let home_server_uid = yas_webserver::local_ipc::expected_server_uid().unwrap_or_else(|error| {
        eprintln!("yas edge: {error}");
        std::process::exit(1);
    });
    #[cfg(windows)]
    let home_server_uid = ();
    // Loopback by default. The listener is plaintext `ws://` and the browser
    // passphrase it accepts is full authority over the home server, so the
    // default must not be reachable from the network: a deployment that wants
    // that puts a TLS reverse proxy in front and says so with YAS_ADDR. Bind
    // `[::1]:3264` instead when the proxy dials IPv6 loopback.
    let addr = std::env::var("YAS_ADDR").unwrap_or_else(|_| "127.0.0.1:3264".into());
    let trusted_proxy_ips = std::env::var("YAS_TRUSTED_PROXY_IPS")
        .ok()
        .map_or_else(|| Ok(HashSet::new()), |raw| parse_trusted_proxy_ips(&raw))
        .unwrap_or_else(|error| {
            eprintln!("yas edge: invalid YAS_TRUSTED_PROXY_IPS: {error}");
            std::process::exit(1);
        });
    serve(Options {
        passphrase,
        addr,
        home: Home::Ipc {
            socket: home_socket,
            uid: home_server_uid,
        },
        trusted_proxy_ips,
        web_transport: web_transport_options_from_env().unwrap_or_else(|error| {
            eprintln!("yas edge: invalid WebTransport configuration: {error}");
            std::process::exit(1);
        }),
    })
    .await;
}

/// The browser passphrase, from the edge's own variable or the shared one.
///
/// `YAS_EDGE_PASSPHRASE` exists for the folded deployment: one process serving
/// both the browser and a WebRTC share is one process holding two secrets, and
/// they are not the same secret.
pub fn passphrase_from_env() -> Option<String> {
    std::env::var("YAS_EDGE_PASSPHRASE")
        .or_else(|_| std::env::var("YAS_PASSPHRASE"))
        .ok()
}

/// Read the optional native datagram edge from the process environment.
///
/// `YAS_WEBTRANSPORT=1` enables it. The UDP bind defaults to `YAS_ADDR`, and
/// the advertised public port defaults to the bind port. A certificate and
/// key must be supplied together; omitting both selects a short-lived pinned
/// development certificate.
pub fn web_transport_options_from_env() -> Result<Option<WebTransportOptions>, String> {
    if !std::env::var("YAS_WEBTRANSPORT").is_ok_and(|value| value == "1") {
        return Ok(None);
    }
    let addr = std::env::var("YAS_WEBTRANSPORT_ADDR")
        .or_else(|_| std::env::var("YAS_ADDR"))
        .unwrap_or_else(|_| "127.0.0.1:3264".into());
    let bind = addr
        .parse::<SocketAddr>()
        .map_err(|error| format!("YAS_WEBTRANSPORT_ADDR {addr:?}: {error}"))?;
    let public_port = match std::env::var("YAS_WEBTRANSPORT_PUBLIC_PORT") {
        Ok(value) => value
            .parse::<u16>()
            .map_err(|_| format!("YAS_WEBTRANSPORT_PUBLIC_PORT {value:?} is not a valid port"))?,
        Err(_) => bind.port(),
    };
    if public_port == 0 {
        return Err("YAS_WEBTRANSPORT_PUBLIC_PORT must not be zero".into());
    }
    let certificate = std::env::var_os("YAS_WEBTRANSPORT_CERT").map(PathBuf::from);
    let private_key = std::env::var_os("YAS_WEBTRANSPORT_KEY").map(PathBuf::from);
    if certificate.is_some() != private_key.is_some() {
        return Err(
            "YAS_WEBTRANSPORT_CERT and YAS_WEBTRANSPORT_KEY must be supplied together".into(),
        );
    }
    Ok(Some(WebTransportOptions {
        addr,
        public_port,
        pin_certificate: certificate.is_none()
            || std::env::var("YAS_WEBTRANSPORT_PIN_CERT").is_ok_and(|value| value == "1"),
        certificate,
        private_key,
    }))
}

fn load_web_transport_identity(
    options: &WebTransportOptions,
) -> Result<
    (
        Vec<rustls_pki_types::CertificateDer<'static>>,
        rustls_pki_types::PrivateKeyDer<'static>,
        bool,
    ),
    String,
> {
    match (&options.certificate, &options.private_key) {
        (Some(certificate), Some(private_key)) => {
            let mut certificates = BufReader::new(File::open(certificate).map_err(|error| {
                format!("cannot open certificate {}: {error}", certificate.display())
            })?);
            let certificates = rustls_pemfile::certs(&mut certificates)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    format!(
                        "cannot parse certificate {}: {error}",
                        certificate.display()
                    )
                })?;
            if certificates.is_empty() {
                return Err(format!(
                    "certificate {} contains no certificates",
                    certificate.display()
                ));
            }
            let mut private_key = BufReader::new(File::open(private_key).map_err(|error| {
                format!("cannot open private key {}: {error}", private_key.display())
            })?);
            let private_key = rustls_pemfile::private_key(&mut private_key)
                .map_err(|error| format!("cannot parse WebTransport private key: {error}"))?
                .ok_or_else(|| "WebTransport private-key file contains no key".to_owned())?;
            Ok((certificates, private_key, false))
        }
        (None, None) => {
            let signing_key = rcgen::KeyPair::generate()
                .map_err(|error| format!("cannot generate WebTransport key: {error}"))?;
            let mut parameters = rcgen::CertificateParams::new(vec!["localhost".into()])
                .map_err(|error| format!("cannot create WebTransport certificate: {error}"))?;
            let now = time::OffsetDateTime::now_utc();
            parameters.not_before = now - time::Duration::minutes(1);
            // Browser certificate-hash authentication permits at most two
            // weeks. Leave a full day for clock skew and boundary rounding.
            parameters.not_after = now + time::Duration::days(13);
            let certificate = parameters
                .self_signed(&signing_key)
                .map_err(|error| format!("cannot sign WebTransport certificate: {error}"))?;
            let certificate = rustls_pki_types::CertificateDer::from(certificate.der().to_vec());
            let private_key = rustls_pki_types::PrivateKeyDer::Pkcs8(
                rustls_pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()),
            );
            Ok((vec![certificate], private_key, true))
        }
        _ => Err("WebTransport certificate and key must be supplied together".into()),
    }
}

fn prepare_web_transport(
    options: Option<WebTransportOptions>,
) -> Result<
    Option<(
        web_transport_quinn::Server,
        WebTransportAdvertisement,
        String,
    )>,
    String,
> {
    let Some(options) = options else {
        return Ok(None);
    };
    let addr = options
        .addr
        .parse::<SocketAddr>()
        .map_err(|error| format!("invalid WebTransport bind {}: {error}", options.addr))?;
    // `yas-edge` is also usable as a library. Do not assume the CLI installed
    // a process-wide provider before asking web-transport-quinn for one
    // (both crypto features may be unified elsewhere in the workspace).
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certificates, private_key, ephemeral) = load_web_transport_identity(&options)?;
    let certificate_hash = (options.pin_certificate || ephemeral).then(|| {
        let digest = ring::digest::digest(&ring::digest::SHA256, certificates[0].as_ref());
        let mut hash = [0; 32];
        hash.copy_from_slice(digest.as_ref());
        hash
    });
    // The builder does not expose QUIC transport configuration. Keep probing
    // idle peers so that the receive deadline below also works for quiet CLI
    // sessions, without requiring application-level traffic.
    use web_transport_quinn::quinn;
    let mut tls = rustls::ServerConfig::builder_with_provider(
        web_transport_quinn::crypto::default_provider(),
    )
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|error| format!("cannot configure WebTransport TLS: {error}"))?
    .with_no_client_auth()
    .with_single_cert(certificates, private_key)
    .map_err(|error| format!("cannot configure WebTransport certificate: {error}"))?;
    tls.alpn_protocols = vec![web_transport_quinn::ALPN.as_bytes().to_vec()];
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)
        .map_err(|error| format!("cannot configure WebTransport QUIC: {error}"))?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_idle_timeout(Some(
            WEBTRANSPORT_PEER_TIMEOUT
                .try_into()
                .expect("valid QUIC timeout"),
        ))
        .keep_alive_interval(Some(WEBTRANSPORT_KEEP_ALIVE_INTERVAL));
    config.transport_config(Arc::new(transport));
    let endpoint = quinn::Endpoint::server(config, addr)
        .map_err(|error| format!("cannot bind WebTransport to {addr}: {error}"))?;
    let server = web_transport_quinn::Server::new(endpoint);
    Ok(Some((
        server,
        WebTransportAdvertisement {
            public_port: options.public_port,
            certificate_hash,
        },
        addr.to_string(),
    )))
}

/// Serve the edge until the process is asked to stop.
///
/// Exits the process on a failure to bind, which is the honest answer for an
/// edge that is the whole of what its process does. A server hosting an edge
/// wants its own answer, and calls [`try_serve`].
pub async fn serve(options: Options) {
    if let Err(error) = try_serve(options, None).await {
        eprintln!("yas edge: {error}");
        std::process::exit(1);
    }
}

/// Serve the edge, reporting rather than exiting on failure.
///
/// `shutdown`, when given, replaces the signal handlers: a hosted edge stops
/// when the server that hosts it stops, and must not install a second SIGTERM
/// handler racing the first.
pub async fn try_serve(
    options: Options,
    shutdown: Option<Arc<tokio::sync::Notify>>,
) -> Result<(), String> {
    let hosted = shutdown.is_some();
    let addr = options.addr;
    let prepared_web_transport = prepare_web_transport(options.web_transport)?;
    let web_transport_advertisement = prepared_web_transport
        .as_ref()
        .map(|(_, advertisement, _)| advertisement.clone());
    let shutdown = shutdown.unwrap_or_default();
    let state = Arc::new(Config {
        passphrase: options.passphrase,
        home: options.home,
        shutdown: shutdown.clone(),
        auth_throttle: yas_webserver::config::AuthThrottle::new(),
        trusted_proxy_ips: options.trusted_proxy_ips,
        web_transport: web_transport_advertisement,
    });

    let tcp = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|error| format!("cannot bind to {addr}: {error}"))?;
    let listener = tcp.tap_io(configure_browser_tcp);
    eprintln!("yas edge: listening on {addr} (WebSocket)");
    let web_transport_task = prepared_web_transport.map(|(server, _, addr)| {
        eprintln!("yas edge: listening on {addr} (WebTransport)");
        tokio::spawn(serve_web_transport(server, state.clone(), shutdown.clone()))
    });
    // The hosting server tells systemd when it is ready; two readies is one
    // more than the protocol has a meaning for.
    if !hosted {
        yas_sd_notify::notify_ready(false);
    }

    let graceful = axum::serve(
        listener,
        build_app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        if hosted {
            // Someone else owns the signals; this edge ends when they say so.
            shutdown.notified().await;
            return;
        }
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut sigterm = signal(SignalKind::terminate()).expect("signal handler");
            let mut sigint = signal(SignalKind::interrupt()).expect("signal handler");
            tokio::select! {
                _ = sigterm.recv() => {}
                _ = sigint.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        shutdown.notify_waiters();
    });
    let result = graceful
        .await
        .map_err(|error| format!("serve error: {error}"));
    if let Some(task) = web_transport_task {
        task.abort();
    }
    result
}

async fn serve_web_transport(
    mut server: web_transport_quinn::Server,
    state: AppState,
    shutdown: Arc<tokio::sync::Notify>,
) {
    loop {
        let request = tokio::select! {
            request = server.accept() => request,
            _ = shutdown.notified() => return,
        };
        let Some(request) = request else {
            return;
        };
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_web_transport(request, state).await {
                eprintln!("yas edge: WebTransport session: {error}");
            }
        });
    }
}

async fn authenticate_web_transport(
    recv: &mut web_transport_quinn::RecvStream,
    send: &mut web_transport_quinn::SendStream,
    state: &Config,
    peer: &str,
) -> bool {
    let Some(guard) = state.auth_throttle.begin(peer.to_owned()) else {
        let _ = send.write_all(&[WEBTRANSPORT_AUTH_BUSY]).await;
        return false;
    };
    let credential = tokio::time::timeout(WEBTRANSPORT_AUTH_TIMEOUT, async {
        let length = recv.read_u16_le().await? as usize;
        if length > WEBTRANSPORT_AUTH_MAX_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "WebTransport credential is too long",
            ));
        }
        let mut bytes = vec![0; length];
        recv.read_exact(&mut bytes)
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        String::from_utf8(bytes).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "non-UTF-8 credential")
        })
    })
    .await;

    match credential {
        Ok(Ok(credential)) => {
            match yas_webserver::config::verify_auth_passphrase(
                &state.passphrase,
                credential.trim(),
            )
            .await
            {
                yas_webserver::config::AuthVerification::Accepted => {
                    guard.record_success();
                    send.write_all(&[WEBTRANSPORT_AUTH_OK]).await.is_ok()
                }
                yas_webserver::config::AuthVerification::Rejected => {
                    guard.record_failure();
                    let _ = send.write_all(&[WEBTRANSPORT_AUTH_REJECTED]).await;
                    false
                }
                yas_webserver::config::AuthVerification::Busy => {
                    drop(guard);
                    let _ = send.write_all(&[WEBTRANSPORT_AUTH_BUSY]).await;
                    false
                }
            }
        }
        Err(_) => {
            guard.record_stalled();
            false
        }
        Ok(Err(_)) => false,
    }
}

async fn handle_web_transport(
    request: web_transport_quinn::Request,
    state: AppState,
) -> Result<(), String> {
    let path = request.url.path();
    if path != WEBTRANSPORT_PATH && path != "/" {
        request
            .reject(axum::http::StatusCode::NOT_FOUND)
            .await
            .map_err(|error| format!("cannot reject unknown path: {error}"))?;
        return Ok(());
    }
    let peer = request.conn().remote_address().ip().to_string();
    let session = request
        .ok()
        .await
        .map_err(|error| format!("handshake failed: {error}"))?;
    let (mut send, mut recv) = tokio::time::timeout(WEBTRANSPORT_AUTH_TIMEOUT, session.accept_bi())
        .await
        .map_err(|_| "timed out waiting for authenticated stream".to_owned())?
        .map_err(|error| format!("cannot accept authenticated stream: {error}"))?;
    if !authenticate_web_transport(&mut recv, &mut send, &state, &peer).await {
        session.close(0, b"authentication failed");
        return Ok(());
    }

    let maximum = session
        .max_datagram_size()
        .min(yas_composite_transport::HARD_MAX_DATAGRAM as usize) as u32;
    if maximum < yas_wire::schema::transport::EVENT_HEADER_BYTES as u32 {
        let (home_reader, home_writer) = state.home.connect().await?;
        bridge_reliable_web_transport(session, recv, send, home_reader, home_writer).await;
        return Ok(());
    }
    let home = state.home.connect_composite(maximum).await?;
    bridge_composite_web_transport(session, recv, send, home, maximum).await;
    Ok(())
}

async fn bridge_reliable_web_transport(
    session: web_transport_quinn::Session,
    mut recv: web_transport_quinn::RecvStream,
    mut send: web_transport_quinn::SendStream,
    mut home_reader: BoxedReader,
    mut home_writer: BoxedWriter,
) {
    let down = async {
        let _ = tokio::io::copy(&mut recv, &mut home_writer).await;
        let _ = home_writer.shutdown().await;
    };
    let up = async {
        let _ = tokio::io::copy(&mut home_reader, &mut send).await;
        let _ = send.shutdown().await;
    };
    tokio::select! {
        _ = down => {}
        _ = up => {}
        // A home write can be backpressured when the peer disappears, so
        // neither copy future is guaranteed to observe the QUIC error.
        _ = web_transport_closed(&session) => {}
    }
    session.close(0, b"YAS stream closed");
}

async fn bridge_composite_web_transport(
    session: web_transport_quinn::Session,
    mut recv: web_transport_quinn::RecvStream,
    mut send: web_transport_quinn::SendStream,
    home: CompositeHome,
    maximum: u32,
) {
    let CompositeHome {
        mut main_reader,
        mut main_writer,
        mut datagram_reader,
        mut datagram_writer,
    } = home;
    let reliable_down = async {
        let _ = tokio::io::copy(&mut recv, &mut main_writer).await;
        let _ = main_writer.shutdown().await;
    };
    let reliable_up = async {
        let _ = tokio::io::copy(&mut main_reader, &mut send).await;
        let _ = send.shutdown().await;
    };
    let datagram_session = session.clone();
    let datagram_down = async move {
        while let Ok(frame) = datagram_session.read_datagram().await {
            if frame.is_empty() || frame.len() > maximum as usize {
                continue;
            }
            if yas_composite_transport::write_datagram(&mut datagram_writer, &frame, maximum)
                .await
                .is_err()
            {
                break;
            }
        }
    };
    let datagram_session = session.clone();
    let datagram_up = async move {
        loop {
            let Ok(frame) =
                yas_composite_transport::read_datagram(&mut datagram_reader, maximum).await
            else {
                break;
            };
            // QUIC congestion is ordinary loss on this optional path.
            let _ = datagram_session.send_datagram(frame.into());
        }
    };
    tokio::select! {
        _ = reliable_down => {}
        _ = reliable_up => {}
        _ = web_transport_closed(&session) => {}
        _ = async {
            tokio::join!(datagram_down, datagram_up);
            // Losing both datagram directions does not invalidate the main
            // link. Park this future, keeping the other select branches live.
            std::future::pending::<()>().await;
        } => {}
    }
    session.close(0, b"YAS stream closed");
}

async fn web_transport_closed(session: &web_transport_quinn::Session) {
    // QUIC's idle timer may restart on a send and grow with the probe timeout.
    // Track receive progress separately: outbound traffic must not extend a
    // vanished peer's lifetime. QUIC ACKs count, even while copies are blocked
    // or the optional application datagram path has failed.
    let connection: &web_transport_quinn::quinn::Connection = session;
    let mut received = connection.stats().udp_rx.datagrams;
    let mut last_received = tokio::time::Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = session.closed() => return,
            _ = tick.tick() => {
                let current = connection.stats().udp_rx.datagrams;
                if current != received {
                    received = current;
                    last_received = tokio::time::Instant::now();
                } else if last_received.elapsed() >= WEBTRANSPORT_PEER_TIMEOUT {
                    session.close(0, b"WebTransport peer timeout");
                    return;
                }
            }
        }
    }
}

fn build_app(state: AppState) -> axum::Router {
    axum::Router::new()
        .fallback(get(root_handler))
        .with_state(state)
}

fn offers_yas_subprotocol(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get_all(axum::http::header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|value| value.trim() == YAS_SUBPROTOCOL)
}

const MAX_FORWARDED_FOR_BYTES: usize = 4 * 1024;
const MAX_FORWARDED_FOR_HOPS: usize = 32;

/// Parse `YAS_TRUSTED_PROXY_IPS`: the exact addresses whose `X-Forwarded-For`
/// this edge will believe.
pub fn parse_trusted_proxy_ips(raw: &str) -> Result<HashSet<IpAddr>, String> {
    let mut result = HashSet::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            return Err("expected a comma-separated list of exact IP addresses".into());
        }
        result.insert(
            entry
                .parse::<IpAddr>()
                .map_err(|_| format!("{entry:?} is not an exact IPv4 or IPv6 address"))?,
        );
    }
    Ok(result)
}

fn forwarded_auth_peer(
    direct: IpAddr,
    headers: &axum::http::HeaderMap,
    trusted_proxies: &HashSet<IpAddr>,
) -> IpAddr {
    if !trusted_proxies.contains(&direct) {
        return direct;
    }
    let mut bytes = 0usize;
    let mut hops = Vec::new();
    for value in headers.get_all("x-forwarded-for") {
        bytes = match bytes.checked_add(value.as_bytes().len()) {
            Some(total) if total <= MAX_FORWARDED_FOR_BYTES => total,
            _ => return direct,
        };
        let Ok(value) = value.to_str() else {
            return direct;
        };
        for entry in value.split(',') {
            if hops.len() >= MAX_FORWARDED_FOR_HOPS {
                return direct;
            }
            let Ok(ip) = entry.trim().parse::<IpAddr>() else {
                return direct;
            };
            hops.push(ip);
        }
    }
    hops.into_iter()
        .rev()
        .find(|ip| !trusted_proxies.contains(ip))
        .unwrap_or(direct)
}

async fn root_handler(State(state): State<AppState>, request: axum::extract::Request) -> Response {
    let auth_peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| {
            forwarded_auth_peer(addr.ip(), request.headers(), &state.trusted_proxy_ips).to_string()
        })
        .unwrap_or_else(|| "unknown".to_string());
    let path = request.uri().path().to_string();
    let inm = request
        .headers()
        .get(axum::http::header::IF_NONE_MATCH)
        .map(|value| value.as_bytes());
    let ae = request
        .headers()
        .get(axum::http::header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok());

    if path == WEBTRANSPORT_CONFIG_PATH {
        let body = match state.web_transport.as_ref() {
            Some(advertisement) => {
                let certificate_hash = advertisement
                    .certificate_hash
                    .map(|hash| format!(",\"certificateHash\":\"{}\"", hex(&hash)))
                    .unwrap_or_default();
                format!(
                    "{{\"webTransport\":{{\"port\":{}{certificate_hash}}}}}",
                    advertisement.public_port
                )
            }
            None => "{\"webTransport\":null}".to_owned(),
        };
        return (
            [
                (axum::http::header::CONTENT_TYPE, "application/json"),
                (axum::http::header::CACHE_CONTROL, "no-store"),
            ],
            body,
        )
            .into_response();
    }

    if let Some(response) = yas_webserver::try_ui_route(&path, SW_JS_BR, &SW_ETAG, inm, ae) {
        return response;
    }
    let is_ws = request
        .headers()
        .get(axum::http::header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if is_ws && path == "/edge" {
        if !offers_yas_subprotocol(request.headers()) {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "the /edge endpoint requires Sec-WebSocket-Protocol: yas.v1",
            )
                .into_response();
        }
        return match WebSocketUpgrade::from_request(request, &state).await {
            Ok(ws) => ws
                .protocols([YAS_SUBPROTOCOL])
                .max_message_size(YAS_MAX_FRAME_SIZE)
                .on_upgrade(move |socket| handle_edge_ws(socket, state, auth_peer)),
            Err(error) => error.into_response(),
        };
    }
    if is_ws {
        return (
            axum::http::StatusCode::NOT_FOUND,
            "unknown WebSocket endpoint",
        )
            .into_response();
    }
    yas_webserver::html_response(INDEX_HTML_BR, &INDEX_ETAG, inm, ae)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

/// Authenticate one browser and adapt YAS message framing to the local
/// server's byte stream. Family payloads remain opaque.
async fn handle_edge_ws(mut ws: WebSocket, state: AppState, auth_peer: String) {
    if !yas_webserver::config::authenticate_text_ws(
        &mut ws,
        &state.passphrase,
        &state.auth_throttle,
        &auth_peer,
        None,
    )
    .await
    {
        return;
    }
    let (mut home_reader, mut home_writer) = match state.home.connect().await {
        Ok(parts) => parts,
        Err(error) => {
            eprintln!("yas edge: cannot connect to home server: {error}");
            let _ = ws
                .send(Message::Text("error:home server unavailable".into()))
                .await;
            let _ = ws.close().await;
            return;
        }
    };
    if ws.send(Message::Text("ok".into())).await.is_err() {
        return;
    }
    loop {
        match ws.recv().await {
            Some(Ok(Message::Binary(preface))) if preface.as_ref() == YAS_PREFACE => {
                if home_writer.write_all(YAS_PREFACE).await.is_err() {
                    let _ = ws.close().await;
                    return;
                }
                break;
            }
            Some(Ok(Message::Ping(payload))) => {
                if ws.send(Message::Pong(payload)).await.is_err() {
                    return;
                }
            }
            Some(Ok(Message::Pong(_))) => {}
            Some(Ok(Message::Binary(_) | Message::Text(_) | Message::Close(_)))
            | Some(Err(_))
            | None => {
                let _ = ws.close().await;
                return;
            }
        }
    }

    let (mut ws_tx, mut ws_rx) = ws.split();
    let mut ws_to_home = tokio::spawn(async move {
        while let Some(message) = ws_rx.next().await {
            match message {
                Ok(Message::Binary(payload)) => {
                    if !write_frame(&mut home_writer, &payload).await {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) | Ok(Message::Text(_)) => break,
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
            }
        }
    });
    let shutdown = state.shutdown.clone();
    let mut home_to_ws = tokio::spawn(async move {
        loop {
            tokio::select! {
                frame = read_frame(&mut home_reader) => {
                    match frame {
                        Some(payload) => {
                            if ws_tx.send(Message::Binary(payload.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = shutdown.notified() => break,
            }
        }
        let _ = ws_tx.close().await;
    });
    tokio::select! {
        _ = &mut ws_to_home => {}
        _ = &mut home_to_ws => {}
    }
    ws_to_home.abort();
    home_to_ws.abort();
}

#[cfg(test)]
mod tests {
    mod web_transport;

    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        Arc::new(Config {
            passphrase: yas_webserver::config::AuthPassphrase::plaintext("test"),
            home: Home::Ipc {
                socket: "/nonexistent-home.sock".into(),
                #[cfg(unix)]
                uid: yas_webserver::local_ipc::effective_uid(),
                #[cfg(windows)]
                uid: (),
            },
            shutdown: Arc::new(tokio::sync::Notify::new()),
            auth_throttle: yas_webserver::config::AuthThrottle::new(),
            trusted_proxy_ips: HashSet::new(),
            web_transport: None,
        })
    }

    fn test_app() -> axum::Router {
        build_app(test_state())
    }

    #[tokio::test]
    async fn webtransport_home_opens_paired_composite_ingress() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
        let open: HostedHome = Arc::new(move || {
            let sender = sender.clone();
            Box::pin(async move {
                let (client, server) = tokio::io::duplex(4096);
                sender
                    .send(server)
                    .await
                    .map_err(|_| "test ingress closed".to_owned())?;
                let (reader, writer) = tokio::io::split(client);
                Ok((
                    Box::new(reader) as BoxedReader,
                    Box::new(writer) as BoxedWriter,
                ))
            })
        });
        let home = Home::Hosted(open);
        let composite = home.connect_composite(1200).await.unwrap();
        let main = receiver.recv().await.unwrap();
        let datagram = receiver.recv().await.unwrap();
        let main = yas_composite_transport::classify(main).await.unwrap();
        let datagram = yas_composite_transport::classify(datagram).await.unwrap();
        let (
            yas_composite_transport::Ingress::Composite {
                offer: main_offer, ..
            },
            yas_composite_transport::Ingress::Composite {
                offer: datagram_offer,
                ..
            },
        ) = (main, datagram)
        else {
            panic!("edge did not select composite ingress");
        };
        assert_eq!(main_offer.role, yas_composite_transport::Role::Main);
        assert_eq!(datagram_offer.role, yas_composite_transport::Role::Datagram);
        assert_eq!(main_offer.token, datagram_offer.token);
        assert_eq!(main_offer.max_datagram, 1200);
        assert_eq!(datagram_offer.max_datagram, 1200);
        drop(composite);
    }

    #[tokio::test]
    async fn ephemeral_webtransport_identity_binds_and_is_pinned() {
        let prepared = prepare_web_transport(Some(WebTransportOptions {
            addr: "127.0.0.1:0".into(),
            public_port: 443,
            certificate: None,
            private_key: None,
            pin_certificate: false,
        }))
        .unwrap()
        .unwrap();
        assert_ne!(prepared.0.local_addr().unwrap().port(), 0);
        assert_eq!(prepared.1.public_port, 443);
        assert!(prepared.1.certificate_hash.is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn home_connector_rejects_wrong_uid_before_protocol_bytes() {
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

    #[test]
    fn trusted_proxy_configuration_accepts_only_exact_ips() {
        assert_eq!(
            parse_trusted_proxy_ips("127.0.0.1, ::1").unwrap(),
            HashSet::from([
                "127.0.0.1".parse::<IpAddr>().unwrap(),
                "::1".parse::<IpAddr>().unwrap(),
            ])
        );
        for invalid in ["", "127.0.0.1,", "10.0.0.0/8", "proxy.local"] {
            assert!(parse_trusted_proxy_ips(invalid).is_err());
        }
    }

    #[test]
    fn untrusted_peer_cannot_spoof_auth_identity() {
        let direct: IpAddr = "203.0.113.10".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-forwarded-for", "198.51.100.7".parse().unwrap());
        let trusted = HashSet::from(["192.0.2.9".parse().unwrap()]);
        assert_eq!(forwarded_auth_peer(direct, &headers, &trusted), direct);
    }

    #[tokio::test]
    async fn edge_rejects_websocket_without_yas_subprotocol() {
        let response = test_app()
            .oneshot(
                axum::extract::Request::builder()
                    .uri("/edge")
                    .header("host", "localhost")
                    .header("upgrade", "websocket")
                    .header("connection", "Upgrade")
                    .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                    .header("sec-websocket-version", "13")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn obsolete_websocket_paths_are_not_endpoints() {
        for path in ["/config", "/mux", "/d/secret"] {
            let response = test_app()
                .oneshot(
                    axum::extract::Request::builder()
                        .uri(path)
                        .header("upgrade", "websocket")
                        .header("connection", "upgrade")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn root_and_unknown_http_paths_serve_only_the_ui() {
        for path in ["/", "/workspace"] {
            let response = test_app()
                .oneshot(
                    axum::extract::Request::builder()
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            assert!(
                response
                    .headers()
                    .get(axum::http::header::CONTENT_TYPE)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .contains("text/html")
            );
        }
    }

    #[tokio::test]
    async fn transport_metadata_reports_optional_webtransport() {
        let mut state = test_state();
        Arc::get_mut(&mut state).unwrap().web_transport = Some(WebTransportAdvertisement {
            public_port: 443,
            certificate_hash: Some([0xab; 32]),
        });
        let response = build_app(state)
            .oneshot(
                axum::extract::Request::builder()
                    .uri(WEBTRANSPORT_CONFIG_PATH)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CACHE_CONTROL)
                .unwrap(),
            "no-store"
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains("\"port\":443"));
        assert!(body.contains(&"ab".repeat(32)));
    }

    #[tokio::test]
    async fn matching_etag_returns_not_modified() {
        let app = test_app();
        let first = app
            .clone()
            .oneshot(
                axum::extract::Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = first
            .headers()
            .get(axum::http::header::ETAG)
            .unwrap()
            .clone();
        let response = app
            .oneshot(
                axum::extract::Request::builder()
                    .uri("/")
                    .header(axum::http::header::IF_NONE_MATCH, etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn ui_body_is_nonempty() {
        let response = test_app()
            .oneshot(
                axum::extract::Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(body.len() > 100);
    }
}
