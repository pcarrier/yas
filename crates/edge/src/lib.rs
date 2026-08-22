use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, FromRequest, State, WebSocketUpgrade};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::serve::ListenerExt;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, LazyLock};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(unix)]
use tokio::net::UnixStream;

type BoxedReader = Box<dyn AsyncRead + Unpin + Send>;
type BoxedWriter = Box<dyn AsyncWrite + Unpin + Send>;

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
}

struct Config {
    passphrase: yas_webserver::config::AuthPassphrase,
    home: Home,
    shutdown: Arc<tokio::sync::Notify>,
    auth_throttle: yas_webserver::config::AuthThrottle,
    trusted_proxy_ips: HashSet<IpAddr>,
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
    let shutdown = shutdown.unwrap_or_default();
    let state = Arc::new(Config {
        passphrase: options.passphrase,
        home: options.home,
        shutdown: shutdown.clone(),
        auth_throttle: yas_webserver::config::AuthThrottle::new(),
        trusted_proxy_ips: options.trusted_proxy_ips,
    });

    let tcp = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|error| format!("cannot bind to {addr}: {error}"))?;
    let listener = tcp.tap_io(configure_browser_tcp);
    eprintln!("yas edge: listening on {addr} (WebSocket)");
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
    graceful
        .await
        .map_err(|error| format!("serve error: {error}"))
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
        })
    }

    fn test_app() -> axum::Router {
        build_app(test_state())
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
