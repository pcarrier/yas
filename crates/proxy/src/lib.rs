/// yas-proxy library — all proxy logic, usable in-process or as a binary.
///
/// Call [`proxy_socket_path`] to find the socket, then [`run`] to start the
/// proxy on the current thread (blocking, runs its own tokio runtime).
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

static VERBOSE: AtomicBool = AtomicBool::new(false);

macro_rules! log {
    ($($arg:tt)*) => {
        if VERBOSE.load(Ordering::Relaxed) {
            eprintln!($($arg)*);
        }
    };
}

type BoxRead = Box<dyn AsyncRead + Unpin + Send>;
type BoxWrite = Box<dyn AsyncWrite + Unpin + Send>;

// ---------------------------------------------------------------------------
// Proxy socket path (single stable path for the whole process)
// ---------------------------------------------------------------------------

pub const EXPECTED_PROXY_UID_ENV: &str = "YAS_PROXY_UID";

#[derive(Clone, Debug)]
struct ProxySocketSpec {
    path: String,
    #[cfg(unix)]
    automatic: bool,
    #[cfg(unix)]
    expected_uid: u32,
}

fn proxy_socket_spec() -> Result<ProxySocketSpec, String> {
    let explicit = std::env::var_os("YAS_PROXY_SOCK")
        .map(|path| {
            path.into_string()
                .map_err(|_| "YAS_PROXY_SOCK must be valid UTF-8".to_owned())
        })
        .transpose()?;
    let path = explicit.clone().unwrap_or_else(automatic_proxy_socket_path);
    if path.is_empty() {
        return Err("YAS_PROXY_SOCK must not be empty".to_owned());
    }
    #[cfg(unix)]
    let expected_uid = expected_proxy_uid()?;
    #[cfg(unix)]
    if explicit.is_none() && expected_uid != yas_webserver::local_ipc::effective_uid() {
        return Err(format!(
            "{EXPECTED_PROXY_UID_ENV} requires an explicit YAS_PROXY_SOCK"
        ));
    }
    Ok(ProxySocketSpec {
        path,
        #[cfg(unix)]
        automatic: explicit.is_none(),
        #[cfg(unix)]
        expected_uid,
    })
}

fn automatic_proxy_socket_path() -> String {
    #[cfg(unix)]
    {
        yas_webserver::local_ipc::automatic_socket_path("yas", "proxy")
    }
    #[cfg(windows)]
    {
        r"\\.\pipe\yas-proxy".into()
    }
    #[cfg(not(any(unix, windows)))]
    {
        "yas-proxy".into()
    }
}

pub fn proxy_socket_path() -> String {
    if let Ok(p) = std::env::var("YAS_PROXY_SOCK") {
        return p;
    }
    automatic_proxy_socket_path()
}

/// Kernel UID expected on an explicit proxy socket. Same-user is the default;
/// cross-UID clients must set both `YAS_PROXY_SOCK` and `YAS_PROXY_UID`.
#[cfg(unix)]
pub fn expected_proxy_uid() -> Result<u32, String> {
    yas_webserver::local_ipc::expected_peer_uid(
        EXPECTED_PROXY_UID_ENV,
        yas_webserver::local_ipc::effective_uid(),
    )
}

/// Connect without sending bytes and authenticate the proxy using kernel peer
/// credentials before returning the stream to a caller carrying target URIs.
#[cfg(unix)]
pub async fn connect_proxy_with_uid(
    path: &str,
    expected_uid: u32,
) -> Result<tokio::net::UnixStream, String> {
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|error| format!("yas-proxy: connect to {path}: {error}"))?;
    yas_webserver::local_ipc::verify_peer_uid_named(&stream, expected_uid, "yas-proxy")
        .map_err(|error| format!("refusing yas-proxy at {path}: {error}"))?;
    Ok(stream)
}

#[cfg(unix)]
pub async fn connect_proxy(path: &str) -> Result<tokio::net::UnixStream, String> {
    connect_proxy_with_uid(path, expected_proxy_uid()?).await
}

// ---------------------------------------------------------------------------
// Auto-start helpers (shared by yas-cli and yas-edge)
// ---------------------------------------------------------------------------

/// Returns true if a proxy is already listening at `path`.
pub async fn proxy_alive(path: &str) -> bool {
    #[cfg(unix)]
    {
        let Ok(expected_uid) = expected_proxy_uid() else {
            return false;
        };
        connect_proxy_with_uid(path, expected_uid).await.is_ok()
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        ClientOptions::new().open(path).is_ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        false
    }
}

/// Resolve the yas binary path for re-exec.
pub fn yas_exe() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_default()
}

/// Ensure a yas-proxy daemon is running, spawning one if necessary.
///
/// `proxy_bin` is the path to the binary that accepts a `proxy-daemon`
/// subcommand (typically [`yas_exe()`]).  When the binary is the
/// standalone `yas-proxy` it should be invoked without arguments;
/// when it is the `yas` CLI it needs the `proxy-daemon` subcommand.
///
/// Returns the socket path on success.
pub async fn ensure_proxy(
    proxy_bin: &std::path::Path,
    use_subcommand: bool,
) -> Result<String, String> {
    let spec = proxy_socket_spec()?;
    ensure_proxy_at(proxy_bin, use_subcommand, spec).await
}

async fn ensure_proxy_at(
    proxy_bin: &std::path::Path,
    use_subcommand: bool,
    spec: ProxySocketSpec,
) -> Result<String, String> {
    let sock = spec.path.clone();

    #[cfg(unix)]
    {
        if spec.automatic {
            yas_webserver::local_ipc::validate_automatic_socket_path(std::path::Path::new(&sock))
                .map_err(|error| {
                format!("yas-proxy: refusing unsafe automatic socket {sock}: {error}")
            })?;
        }
        if probe_proxy(&sock, spec.expected_uid).await? {
            return Ok(sock);
        }
        let effective_uid = yas_webserver::local_ipc::effective_uid();
        if spec.expected_uid != effective_uid {
            return Err(format!(
                "yas-proxy: explicit socket {sock} expects UID {}, but no such proxy is running; refusing to auto-start a UID {effective_uid} daemon",
                spec.expected_uid,
            ));
        }
    }

    #[cfg(windows)]
    if proxy_alive(&sock).await {
        return Ok(sock);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new(proxy_bin);
        if use_subcommand {
            cmd.arg("proxy-daemon");
        }
        // SAFETY: pre_exec runs in the child between fork and exec.
        // setsid() is async-signal-safe per POSIX.
        unsafe {
            cmd.env("YAS_PROXY_IDLE", "300")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .pre_exec(|| {
                    libc::setsid();
                    Ok(())
                })
                .spawn()
                .map_err(|e| format!("yas-proxy: spawn failed: {e}"))?;
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = std::process::Command::new(proxy_bin);
        if use_subcommand {
            cmd.arg("proxy-daemon");
        }
        cmd.env("YAS_PROXY_IDLE", "300")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("yas-proxy: spawn failed: {e}"))?;
    }

    #[cfg(not(any(unix, windows)))]
    return Err("yas-proxy auto-start is not supported on this platform".into());

    // Wait up to 5 s for the socket/pipe to appear.
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        #[cfg(unix)]
        match probe_proxy(&sock, spec.expected_uid).await {
            Ok(true) => return Ok(sock),
            Ok(false) => {}
            Err(error) => return Err(error),
        }
        #[cfg(windows)]
        if proxy_alive(&sock).await {
            return Ok(sock);
        }
    }
    Err(format!("yas-proxy did not become ready at {sock} in time"))
}

#[cfg(unix)]
async fn probe_proxy(path: &str, expected_uid: u32) -> Result<bool, String> {
    match tokio::net::UnixStream::connect(path).await {
        Ok(stream) => {
            yas_webserver::local_ipc::verify_peer_uid_named(&stream, expected_uid, "yas-proxy")
                .map_err(|error| format!("refusing yas-proxy at {path}: {error}"))?;
            Ok(true)
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(format!("yas-proxy: connect to {path}: {error}")),
    }
}

// ---------------------------------------------------------------------------
// Upstream connection
// ---------------------------------------------------------------------------

struct UpstreamConn {
    reader: BoxRead,
    writer: BoxWrite,
}

/// One directly connected YAS WebTransport session. Unlike the proxy daemon's
/// byte-stream socket, this retains the WebTransport session so callers can
/// use its independent unreliable datagram path.
pub struct YasWebTransportConnection {
    reader: BoxRead,
    writer: BoxWrite,
    session: web_transport_quinn::Session,
}

impl YasWebTransportConnection {
    pub fn into_parts(
        self,
    ) -> (
        Box<dyn AsyncRead + Unpin + Send>,
        Box<dyn AsyncWrite + Unpin + Send>,
        web_transport_quinn::Session,
    ) {
        (self.reader, self.writer, self.session)
    }
}

// ---------------------------------------------------------------------------
// Proxy activity
// ---------------------------------------------------------------------------

/// Tracks daemon use without caching protocol sessions. Every downstream gets
/// one explicitly selected native YAS upstream; no compatibility transport or
/// private multiplexing layer is reachable through the proxy socket.
struct Activity {
    active: AtomicUsize,
    last_activity: AtomicI64,
}

impl Activity {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            active: AtomicUsize::new(0),
            last_activity: AtomicI64::new(now_secs()),
        })
    }

    fn connected(&self) {
        self.active.fetch_add(1, Ordering::Relaxed);
        self.last_activity.store(i64::MAX, Ordering::Relaxed);
    }

    fn disconnected(&self) {
        let previous = self.active.fetch_sub(1, Ordering::Relaxed);
        if previous == 1 {
            self.last_activity.store(now_secs(), Ordering::Relaxed);
        }
    }

    fn latest(&self) -> i64 {
        self.last_activity.load(Ordering::Relaxed)
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ---------------------------------------------------------------------------
// Upstream transport implementations
// ---------------------------------------------------------------------------

async fn connect_upstream(uri: &str) -> Result<UpstreamConn, String> {
    if let Some(rest) = uri.strip_prefix("share:") {
        return connect_share(rest).await;
    }

    if let Some(rest) = uri.strip_prefix("ssh:") {
        return connect_ssh(rest).await;
    }

    if let Some(rest) = uri.strip_prefix("uplink:") {
        return connect_uplink(rest).await;
    }

    // Extract query parameters from URIs that support them.
    let (base_uri, passphrase, cert_hash) = extract_uri_params(uri);

    if let Some(path) = base_uri.strip_prefix("socket:") {
        return connect_socket(path).await;
    }
    if let Some(addr) = base_uri.strip_prefix("tcp:") {
        return connect_tcp(addr).await;
    }
    if base_uri.starts_with("ws://") || base_uri.starts_with("wss://") {
        return connect_ws(&base_uri, passphrase.as_deref()).await;
    }
    if let Some(rest) = base_uri.strip_prefix("wt://") {
        let cert_bytes = cert_hash.as_deref().and_then(parse_hex);
        return connect_wt(rest, passphrase.as_deref(), &cert_bytes).await;
    }
    Err(format!(
        "unknown upstream URI scheme in '{uri}' \
         (expected socket:, tcp:, ws://, wss://, wt://, share:, uplink:, or ssh:)"
    ))
}

/// Connect to any upstream URI supported by the proxy and return its raw byte
/// stream halves. This performs no YAS protocol handshake or message parsing;
/// callers can transparently carry a complete downstream protocol session.
pub async fn connect_upstream_split(
    uri: &str,
) -> Result<
    (
        Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        Box<dyn tokio::io::AsyncWrite + Unpin + Send>,
    ),
    String,
> {
    let conn = connect_upstream(uri).await?;
    Ok((conn.reader, conn.writer))
}

/// Connect to a native YAS endpoint without ever falling back to a YAS
/// transport. WebSocket routes select `yas.v1` and preserve the standalone
/// preface message; SSH routes use the SSH pool's YAS socket discovery.
pub async fn connect_yas_upstream_split(
    uri: &str,
) -> Result<
    (
        Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        Box<dyn tokio::io::AsyncWrite + Unpin + Send>,
    ),
    String,
> {
    if let Some(rest) = uri.strip_prefix("share:") {
        let conn = connect_share(rest).await?;
        return Ok((conn.reader, conn.writer));
    }
    let (base_uri, passphrase, cert_hash) = extract_uri_params(uri);
    let conn = if let Some(path) = base_uri.strip_prefix("socket:") {
        connect_socket(path).await?
    } else if let Some(addr) = base_uri.strip_prefix("tcp:") {
        connect_tcp(addr).await?
    } else if base_uri.starts_with("ws://") || base_uri.starts_with("wss://") {
        connect_ws_yas(&base_uri, passphrase.as_deref()).await?
    } else if let Some(rest) = base_uri.strip_prefix("wt://") {
        let cert_bytes = cert_hash.as_deref().and_then(parse_hex);
        connect_wt(rest, passphrase.as_deref(), &cert_bytes).await?
    } else if let Some(rest) = base_uri.strip_prefix("ssh:") {
        connect_ssh_yas(rest).await?
    } else {
        return Err(format!(
            "unsupported native YAS upstream scheme in '{uri}' \
             (expected socket:, tcp:, ws://, wss://, wt://, share:, or ssh:)"
        ));
    };
    Ok((conn.reader, conn.writer))
}

/// Connect an explicit `wt://` YAS target without tunnelling it through the
/// stream-only proxy daemon. The returned session owns the datagram path.
pub async fn connect_yas_webtransport(uri: &str) -> Result<YasWebTransportConnection, String> {
    let (base_uri, passphrase, cert_hash) = extract_uri_params(uri);
    let rest = base_uri
        .strip_prefix("wt://")
        .ok_or_else(|| format!("expected a wt:// target, got '{uri}'"))?;
    let cert_bytes = cert_hash.as_deref().and_then(parse_hex);
    let (connection, session) =
        connect_wt_with_session(rest, passphrase.as_deref(), &cert_bytes).await?;
    Ok(YasWebTransportConnection {
        reader: connection.reader,
        writer: connection.writer,
        session,
    })
}

/// Split `share:` URI rest into (passphrase, hub_url).
///
/// Accepted forms:
///   `myphrase`                       — use default hub
///   `myphrase?hub=wss://custom.hub`  — use specific hub
pub fn parse_share_uri(rest: &str) -> (String, String) {
    let (passphrase_raw, hub) = if let Some(q_pos) = rest.find('?') {
        let phrase = &rest[..q_pos];
        let query = &rest[q_pos + 1..];
        let hub = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("hub=").map(percent_decode))
            .unwrap_or_else(|| yas_webrtc_forwarder::DEFAULT_HUB_URL.to_string());
        (phrase.to_string(), hub)
    } else {
        (
            rest.to_string(),
            yas_webrtc_forwarder::DEFAULT_HUB_URL.to_string(),
        )
    };
    let passphrase = percent_decode(&passphrase_raw);
    let hub = yas_webrtc_forwarder::normalize_hub(&hub);
    (passphrase, hub)
}

async fn connect_share(rest: &str) -> Result<UpstreamConn, String> {
    let (passphrase, hub) = parse_share_uri(rest);
    let stream = yas_webrtc_forwarder::client::connect(&passphrase, &hub)
        .await
        .map_err(|e| format!("share:{rest}: {e}"))?;
    let (r, w) = tokio::io::split(stream);
    Ok(UpstreamConn {
        reader: Box::new(r),
        writer: Box::new(w),
    })
}

// ---------------------------------------------------------------------------
// Uplink relay consumers
// ---------------------------------------------------------------------------

/// Split `uplink:` URI rest into (token, control_url).
///
/// Accepted forms:
///   `https://relay.example#TOKEN`
fn parse_uplink_uri(rest: &str) -> Result<(String, String), String> {
    let (control_raw, token_raw) = rest
        .rsplit_once('#')
        .ok_or("uplink: remote requires uplink:<control-url>#<token>")?;
    let control = percent_decode(control_raw)
        .trim_end_matches('/')
        .to_string();
    if control.is_empty() {
        return Err("uplink: remote requires a control URL".into());
    }
    let token = percent_decode(token_raw);
    if token.is_empty() {
        return Err("uplink: remote requires a token after #".into());
    }
    Ok((token, control))
}

/// Resolve an `uplink:` remote: ask the control plane where the
/// session's worker is (`GET /attach`, which blocks server-side until the
/// uplink is connected), then attach over WebSocket with the token as the
/// auth passphrase.  The token is a credential — never log it.
async fn connect_uplink(rest: &str) -> Result<UpstreamConn, String> {
    let (token, control) = parse_uplink_uri(rest)?;
    if token.is_empty() {
        return Err("uplink: remote requires a token".into());
    }
    let attach = format!("{control}/attach");
    let resp = reqwest::Client::new()
        .get(&attach)
        .header("authorization", format!("Bearer {token}"))
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("{attach}: {e}"))?;
    match resp.status().as_u16() {
        200 => {}
        401 | 403 => return Err(format!("{attach}: token rejected ({})", resp.status())),
        404 => return Err(format!("{attach}: session has no connected uplink")),
        s => return Err(format!("{attach}: HTTP {s}")),
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("{attach}: bad response: {e}"))?;
    let ws = body
        .get("ws")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{attach}: response missing \"ws\""))?;
    connect_ws(ws, Some(&token)).await
}

/// Public entry point for direct (non-daemon) `uplink:` connections, used by
/// yas-cli and yas-edge when the proxy daemon is disabled.  Accepts the
/// URI with or without the `uplink:` prefix and returns the connected byte
/// stream halves.
pub async fn connect_uplink_split(
    uri: &str,
) -> Result<
    (
        Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        Box<dyn tokio::io::AsyncWrite + Unpin + Send>,
    ),
    String,
> {
    let rest = uri.strip_prefix("uplink:").unwrap_or(uri);
    let conn = connect_uplink(rest).await?;
    Ok((conn.reader, conn.writer))
}

// ---------------------------------------------------------------------------
// SSH via embedded client (cross-platform)
// ---------------------------------------------------------------------------

/// Shared SSH connection pool for the proxy.  Connections are multiplexed
/// over a single TCP+SSH session per host, matching Edge uplink behaviour.
fn ssh_pool() -> &'static yas_ssh::SshPool {
    static POOL: std::sync::OnceLock<yas_ssh::SshPool> = std::sync::OnceLock::new();
    POOL.get_or_init(yas_ssh::SshPool::new)
}

/// Connect to a remote yas-server via the embedded SSH client.
///
/// Uses `direct-streamlocal` channel forwarding (no external `ssh`, `nc`, or
/// `socat` required).  The connection is multiplexed and pooled so subsequent
/// calls to the same host reuse the TCP+SSH session.
async fn connect_ssh(rest: &str) -> Result<UpstreamConn, String> {
    if rest.is_empty() {
        return Err("ssh: destination requires a host".into());
    }
    let (user, host, socket) = yas_ssh::parse_ssh_uri(rest);
    let stream = ssh_pool()
        .connect(&host, user.as_deref(), socket.as_deref())
        .await
        .map_err(|e| format!("ssh:{rest}: {e}"))?;
    let (r, w) = tokio::io::split(stream);
    Ok(UpstreamConn {
        reader: Box::new(r),
        writer: Box::new(w),
    })
}

async fn connect_ssh_yas(rest: &str) -> Result<UpstreamConn, String> {
    if rest.is_empty() {
        return Err("ssh: destination requires a host".into());
    }
    let (user, host, socket) = yas_ssh::parse_ssh_uri(rest);
    let stream = ssh_pool()
        .connect_yas(&host, user.as_deref(), socket.as_deref())
        .await
        .map_err(|e| format!("ssh:{rest}: {e}"))?;
    let (reader, writer) = tokio::io::split(stream);
    Ok(UpstreamConn {
        reader: Box::new(reader),
        writer: Box::new(writer),
    })
}

/// Split a URI into (base, passphrase, certHash).
///
/// The passphrase is the fragment — `ws://host/edge#secret` — because a
/// fragment is the one part of a URI that never leaves the client: it is not
/// in the request line, so it is not in an access log, a proxy log, or a
/// `Referer`. It was a query parameter, which is all of those things. The
/// certificate pin stays in the query, being a hash of a public certificate
/// rather than a secret.
///
/// Percent-decoded, so a passphrase containing `#` or a space can be written.
/// Only applies to ws://, wss://, wt:// — socket: and tcp: are returned as-is.
fn extract_uri_params(uri: &str) -> (String, Option<String>, Option<String>) {
    if !uri.starts_with("ws://") && !uri.starts_with("wss://") && !uri.starts_with("wt://") {
        return (uri.to_string(), None, None);
    }
    let (rest, passphrase) = match uri.split_once('#') {
        Some((rest, fragment)) if !fragment.is_empty() => (rest, Some(percent_decode(fragment))),
        Some((rest, _)) => (rest, None),
        None => (uri, None),
    };
    let (base, query) = match rest.find('?') {
        Some(pos) => (&rest[..pos], Some(&rest[pos + 1..])),
        None => (rest, None),
    };
    let mut cert_hash = None;
    if let Some(q) = query {
        for param in q.split('&') {
            if let Some(v) = param.strip_prefix("certHash=") {
                cert_hash = Some(v.to_string());
            }
        }
    }
    (base.to_string(), passphrase, cert_hash)
}

fn percent_decode(s: &str) -> String {
    // Minimal %XX decoder sufficient for passphrases.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            if let Ok(b) = u8::from_str_radix(&format!("{h1}{h2}"), 16) {
                out.push(b as char);
                continue;
            }
        }
        out.push(c);
    }
    out
}

async fn connect_socket(path: &str) -> Result<UpstreamConn, String> {
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|e| format!("socket:{path}: {e}"))?;
        let (r, w) = tokio::io::split(stream);
        Ok(UpstreamConn {
            reader: Box::new(r),
            writer: Box::new(w),
        })
    }
    #[cfg(not(unix))]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        let pipe = ClientOptions::new()
            .open(path)
            .map_err(|e| format!("socket:{path}: {e}"))?;
        let (r, w) = tokio::io::split(pipe);
        Ok(UpstreamConn {
            reader: Box::new(r),
            writer: Box::new(w),
        })
    }
}

async fn connect_tcp(addr: &str) -> Result<UpstreamConn, String> {
    let stream = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| format!("tcp:{addr}: {e}"))?;
    let _ = stream.set_nodelay(true);
    let (r, w) = tokio::io::split(stream);
    Ok(UpstreamConn {
        reader: Box::new(r),
        writer: Box::new(w),
    })
}

async fn connect_ws(uri: &str, passphrase: Option<&str>) -> Result<UpstreamConn, String> {
    connect_ws_mode(uri, passphrase, false).await
}

async fn connect_ws_yas(uri: &str, passphrase: Option<&str>) -> Result<UpstreamConn, String> {
    connect_ws_mode(uri, passphrase, true).await
}

async fn connect_ws_mode(
    uri: &str,
    passphrase: Option<&str>,
    yas: bool,
) -> Result<UpstreamConn, String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

    let mut request = uri
        .into_client_request()
        .map_err(|error| format!("{uri}: {error}"))?;
    if yas {
        request.headers_mut().insert(
            "sec-websocket-protocol",
            yas_wire::schema::transport::WEBSOCKET_SUBPROTOCOL
                .parse()
                .expect("generated WebSocket protocol is a valid header value"),
        );
    }
    let (mut ws, response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("{uri}: {e}"))?;
    if yas
        && response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok())
            != Some(yas_wire::schema::transport::WEBSOCKET_SUBPROTOCOL)
    {
        return Err(format!(
            "{uri}: server did not select {}",
            yas_wire::schema::transport::WEBSOCKET_SUBPROTOCOL
        ));
    }

    let pass = passphrase.unwrap_or("");
    ws.send(Message::Text(pass.into()))
        .await
        .map_err(|e| format!("{uri}: auth send: {e}"))?;
    match ws.next().await {
        Some(Ok(Message::Text(t))) if t.trim() == "ok" => {}
        Some(Ok(Message::Text(t))) => {
            return Err(format!("{uri}: auth rejected: {}", t.trim()));
        }
        other => {
            return Err(format!("{uri}: unexpected auth response: {other:?}"));
        }
    }

    let (ws_write, ws_read) = ws.split();
    Ok(UpstreamConn {
        reader: Box::new(WsFrameReader {
            inner: ws_read,
            buf: bytes::Bytes::new(),
        }),
        writer: Box::new(if yas {
            WsFrameWriter::new_yas(ws_write)
        } else {
            WsFrameWriter::new(ws_write)
        }),
    })
}

async fn connect_wt(
    rest: &str,
    passphrase: Option<&str>,
    cert_hash: &Option<Vec<u8>>,
) -> Result<UpstreamConn, String> {
    connect_wt_with_session(rest, passphrase, cert_hash)
        .await
        .map(|(connection, _)| connection)
}

async fn connect_wt_with_session(
    rest: &str,
    passphrase: Option<&str>,
    cert_hash: &Option<Vec<u8>>,
) -> Result<(UpstreamConn, web_transport_quinn::Session), String> {
    use web_transport_quinn as wt;

    // Build the URL for the WT session (must use https: scheme).
    let (host, port) = parse_wt_host_port(rest)?;
    let url: url::Url = format!("https://{host}:{port}/")
        .parse()
        .map_err(|e| format!("wt: url: {e}"))?;

    // Build the client with appropriate certificate verification.
    let client: wt::Client = if let Some(hash) = cert_hash {
        wt::ClientBuilder::new()
            .with_server_certificate_hashes(vec![hash.clone()])
            .map_err(|e| format!("wt: client build: {e}"))?
    } else {
        wt::ClientBuilder::new()
            .with_system_roots()
            .map_err(|e| format!("wt: client build: {e}"))?
    };

    let session = client
        .connect(url)
        .await
        .map_err(|e| format!("wt: connect {host}:{port}: {e}"))?;

    let (mut send, mut recv) = session
        .open_bi()
        .await
        .map_err(|e| format!("wt: open_bi: {e}"))?;

    // Auth: 2-byte-LE passphrase length + passphrase bytes, then read 1-byte response.
    let pass = passphrase.unwrap_or("").as_bytes();
    let mut auth_buf = Vec::with_capacity(2 + pass.len());
    auth_buf.extend_from_slice(&(pass.len() as u16).to_le_bytes());
    auth_buf.extend_from_slice(pass);
    send.write_all(&auth_buf)
        .await
        .map_err(|e| format!("wt: auth send: {e}"))?;

    let mut resp = [0u8; 1];
    recv.read_exact(&mut resp)
        .await
        .map_err(|e| format!("wt: auth recv: {e}"))?;
    if resp[0] != 1 {
        return Err(format!(
            "wt: auth rejected (response byte {:#04x})",
            resp[0]
        ));
    }

    Ok((
        UpstreamConn {
            reader: Box::new(recv),
            writer: Box::new(send),
        },
        session,
    ))
}

fn parse_wt_host_port(rest: &str) -> Result<(String, u16), String> {
    let without_path = rest.split('/').next().unwrap_or(rest);
    if let Some(colon) = without_path.rfind(':') {
        let host = without_path[..colon].to_string();
        let port: u16 = without_path[colon + 1..]
            .parse()
            .map_err(|_| format!("wt: invalid port in '{rest}'"))?;
        Ok((host, port))
    } else {
        Ok((without_path.to_string(), 443))
    }
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// WS ↔ raw yas-frame adapters
// ---------------------------------------------------------------------------

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
type WsStream = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

struct WsFrameReader {
    inner: WsStream,
    buf: bytes::Bytes,
}

impl AsyncRead for WsFrameReader {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        out: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        loop {
            if !self.buf.is_empty() {
                let n = out.remaining().min(self.buf.len());
                out.put_slice(&self.buf[..n]);
                self.buf = self.buf.slice(n..);
                return std::task::Poll::Ready(Ok(()));
            }
            match self.inner.poll_next_unpin(cx) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(None) => return std::task::Poll::Ready(Ok(())),
                std::task::Poll::Ready(Some(Err(e))) => {
                    return std::task::Poll::Ready(Err(std::io::Error::other(e)));
                }
                std::task::Poll::Ready(Some(Ok(msg))) => {
                    let data = match msg {
                        Message::Binary(d) => d,
                        Message::Close(_) => return std::task::Poll::Ready(Ok(())),
                        _ => continue,
                    };
                    let len = data.len() as u32;
                    let mut framed = Vec::with_capacity(4 + data.len());
                    framed.extend_from_slice(&len.to_le_bytes());
                    framed.extend_from_slice(&data);
                    self.buf = bytes::Bytes::from(framed);
                }
            }
        }
    }
}

struct WsFrameWriter {
    inner: WsSink,
    /// Raw YAS stream bytes not yet assembled into a complete WebSocket
    /// message. `AsyncWrite` callers may split the four-byte length prefix or
    /// payload at any byte boundary (the relay does so at 64 KiB), so frame
    /// boundaries cannot be inferred from individual `poll_write` calls.
    buf: bytes::BytesMut,
    /// YAS message transports carry the eight-byte preface as one unframed
    /// message before their ordinary length-prefixed stream frames.
    yas_preface_pending: bool,
}

const WS_FRAME_MAX_BYTES: usize = yas_wire::schema::transport::HARD_MAX_WIRE_FRAME as usize;
const _: () = assert!(
    yas_wire::schema::transport::STREAM_LENGTH_BITS == u32::BITS as u8
        && yas_wire::schema::transport::STREAM_LENGTH_BYTES == size_of::<u32>()
);
const WS_FRAME_BUFFER_MAX_BYTES: usize = WS_FRAME_MAX_BYTES + 4;

impl WsFrameWriter {
    fn new(inner: WsSink) -> Self {
        Self {
            inner,
            buf: bytes::BytesMut::new(),
            yas_preface_pending: false,
        }
    }

    fn new_yas(inner: WsSink) -> Self {
        Self {
            inner,
            buf: bytes::BytesMut::new(),
            yas_preface_pending: true,
        }
    }

    /// Send every complete buffered frame for which the WebSocket sink has
    /// credit. A partial frame remains buffered across writes and flushes;
    /// only end-of-stream makes it an error.
    fn poll_send_buffered(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        loop {
            if self.yas_preface_pending {
                if self.buf.len() < yas_wire::PREFACE.len() {
                    return std::task::Poll::Ready(Ok(()));
                }
                if &self.buf[..yas_wire::PREFACE.len()] != yas_wire::PREFACE.as_slice() {
                    return std::task::Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "YAS WebSocket stream has an invalid preface",
                    )));
                }
                match self.inner.poll_ready_unpin(cx) {
                    std::task::Poll::Pending => return std::task::Poll::Pending,
                    std::task::Poll::Ready(Err(error)) => {
                        return std::task::Poll::Ready(Err(std::io::Error::other(error)));
                    }
                    std::task::Poll::Ready(Ok(())) => {}
                }
                let preface = self.buf.split_to(yas_wire::PREFACE.len()).freeze();
                if let Err(error) = self.inner.start_send_unpin(Message::Binary(preface)) {
                    return std::task::Poll::Ready(Err(std::io::Error::other(error)));
                }
                self.yas_preface_pending = false;
                continue;
            }
            if self.buf.len() < 4 {
                return std::task::Poll::Ready(Ok(()));
            }
            let len =
                u32::from_le_bytes(self.buf[..4].try_into().expect("checked header")) as usize;
            if len > WS_FRAME_MAX_BYTES {
                return std::task::Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("ws frame writer: frame payload too large ({len} bytes)"),
                )));
            }
            let frame_len = 4 + len;
            if self.buf.len() < frame_len {
                return std::task::Poll::Ready(Ok(()));
            }

            match self.inner.poll_ready_unpin(cx) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(Err(error)) => {
                    return std::task::Poll::Ready(Err(std::io::Error::other(error)));
                }
                std::task::Poll::Ready(Ok(())) => {}
            }
            let mut frame = self.buf.split_to(frame_len);
            let payload = frame.split_off(4).freeze();
            if let Err(error) = self.inner.start_send_unpin(Message::Binary(payload)) {
                return std::task::Poll::Ready(Err(std::io::Error::other(error)));
            }
        }
    }

    fn incomplete_frame_error(&self) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!(
                "ws frame writer: stream ended with {} incomplete frame bytes",
                self.buf.len()
            ),
        )
    }
}

impl AsyncWrite for WsFrameWriter {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if buf.is_empty() {
            return std::task::Poll::Ready(Ok(0));
        }
        match self.poll_send_buffered(cx) {
            std::task::Poll::Pending => return std::task::Poll::Pending,
            std::task::Poll::Ready(Err(error)) => return std::task::Poll::Ready(Err(error)),
            std::task::Poll::Ready(Ok(())) => {}
        }

        let buffer_limit = WS_FRAME_BUFFER_MAX_BYTES
            + usize::from(self.yas_preface_pending) * yas_wire::PREFACE.len();
        let available = buffer_limit.saturating_sub(self.buf.len());
        if available == 0 {
            // A complete maximum-sized frame would have been sent (or
            // returned Pending) above, so a full buffer here can only be an
            // invalid declared length.
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ws frame writer: frame buffer limit exceeded",
            )));
        }
        let consumed = available.min(buf.len());
        self.buf.extend_from_slice(&buf[..consumed]);

        // Validate a newly completed header before acknowledging these bytes.
        // A complete frame may also be handed to the sink immediately, but
        // sink backpressure does not undo bytes already accepted here.
        if !self.yas_preface_pending && self.buf.len() >= 4 {
            let len =
                u32::from_le_bytes(self.buf[..4].try_into().expect("checked header")) as usize;
            if len > WS_FRAME_MAX_BYTES {
                return std::task::Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("ws frame writer: frame payload too large ({len} bytes)"),
                )));
            }
        }
        if let std::task::Poll::Ready(Err(error)) = self.poll_send_buffered(cx) {
            return std::task::Poll::Ready(Err(error));
        }
        std::task::Poll::Ready(Ok(consumed))
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.poll_send_buffered(cx) {
            std::task::Poll::Pending => return std::task::Poll::Pending,
            std::task::Poll::Ready(Err(error)) => return std::task::Poll::Ready(Err(error)),
            std::task::Poll::Ready(Ok(())) => {}
        }
        // `tokio::io::copy` may flush whenever its raw input temporarily
        // returns Pending, including halfway through a length-prefixed frame.
        // Flush complete WebSocket messages while retaining that partial
        // application frame for the next write.
        self.inner
            .poll_flush_unpin(cx)
            .map_err(std::io::Error::other)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.poll_send_buffered(cx) {
            std::task::Poll::Pending => return std::task::Poll::Pending,
            std::task::Poll::Ready(Err(error)) => return std::task::Poll::Ready(Err(error)),
            std::task::Poll::Ready(Ok(())) => {}
        }
        if self.yas_preface_pending || !self.buf.is_empty() {
            return std::task::Poll::Ready(Err(self.incomplete_frame_error()));
        }
        self.inner
            .poll_close_unpin(cx)
            .map_err(std::io::Error::other)
    }
}

// ---------------------------------------------------------------------------
// Downstream listener
// ---------------------------------------------------------------------------

#[cfg(unix)]
struct BoundProxyListener {
    listener: std::os::unix::net::UnixListener,
    _lock: std::fs::File,
}

#[cfg(unix)]
fn validate_explicit_socket_object(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_socket()
                && metadata.uid() == yas_webserver::local_ipc::effective_uid() =>
        {
            Ok(())
        }
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "explicit proxy socket path is prebound by an unsafe filesystem object",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn validate_proxy_socket_object(path: &std::path::Path, automatic: bool) -> std::io::Result<()> {
    if automatic {
        yas_webserver::local_ipc::validate_automatic_socket_path(path)
    } else {
        validate_explicit_socket_object(path)
    }
}

#[cfg(unix)]
fn bind_proxy_listener(
    socket_path: &std::path::Path,
    automatic: bool,
) -> std::io::Result<BoundProxyListener> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;

    validate_proxy_socket_object(socket_path, automatic)?;
    let lock_path = std::path::PathBuf::from(format!("{}.lock", socket_path.display()));
    let lock = yas_webserver::local_ipc::open_owner_only_lock_file(&lock_path)?;
    // SAFETY: `lock` owns a valid descriptor for the duration of this call.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error());
    }

    // Revalidate only after serializing daemon startup. Never unlink an
    // arbitrary final object, and never replace a live same-UID daemon.
    validate_proxy_socket_object(socket_path, automatic)?;
    match std::os::unix::net::UnixStream::connect(socket_path) {
        Ok(stream) => {
            yas_webserver::local_ipc::verify_peer_uid_named(
                &stream,
                yas_webserver::local_ipc::effective_uid(),
                "existing yas-proxy",
            )
            .map_err(std::io::Error::other)?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "yas-proxy socket already has a live listener",
            ));
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            if std::fs::symlink_metadata(socket_path).is_ok() {
                validate_proxy_socket_object(socket_path, automatic)?;
                std::fs::remove_file(socket_path)?;
            }
        }
        Err(error) => return Err(error),
    }

    // The automatic parent is already mode 0700. The restrictive umask also
    // makes explicit paths owner-only at the instant bind creates them.
    // SAFETY: `umask` accepts every mode value and returns the prior mask.
    let old_umask = unsafe { libc::umask(0o077) };
    let listener = std::os::unix::net::UnixListener::bind(socket_path);
    // SAFETY: restore the exact mask returned by the preceding call.
    unsafe { libc::umask(old_umask) };
    let listener = listener?;
    listener.set_nonblocking(true)?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o700))?;
    validate_proxy_socket_object(socket_path, automatic)?;
    Ok(BoundProxyListener {
        listener,
        _lock: lock,
    })
}

/// Read one line from the downstream socket (up to 4 KiB).
async fn read_line<S>(stream: &mut S) -> Option<String>
where
    S: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte).await {
            Ok(0) | Err(_) => return None,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
                if buf.len() > 4096 {
                    return None;
                }
            }
        }
    }
    Some(
        String::from_utf8_lossy(&buf)
            .trim_end_matches('\r')
            .to_string(),
    )
}

#[cfg(unix)]
async fn run_listener(
    activity: Arc<Activity>,
    sock_path: &str,
    bound: BoundProxyListener,
) -> std::io::Result<()> {
    let BoundProxyListener { listener, _lock } = bound;
    let listener = tokio::net::UnixListener::from_std(listener)?;
    log!("yas-proxy: listening on {sock_path}");
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let activity = activity.clone();
                tokio::spawn(async move {
                    handle_downstream(activity, stream).await;
                });
            }
            Err(e) => log!("yas-proxy: accept error: {e}"),
        }
    }
}

#[cfg(windows)]
async fn run_listener(
    activity: Arc<Activity>,
    pipe_path: &str,
    first: tokio::net::windows::named_pipe::NamedPipeServer,
) {
    use tokio::net::windows::named_pipe::ServerOptions;
    log!("yas-proxy: listening on {pipe_path}");
    // Use the pre-created first instance (created with first_pipe_instance(true)
    // by the caller) to avoid a race window where the pipe name is unowned.
    let mut next_server = Some(first);
    loop {
        // Prepare the next server instance before awaiting the current connection,
        // so there is always a waiting server end after handoff.
        let server = match next_server.take() {
            Some(s) => s,
            None => match ServerOptions::new()
                .first_pipe_instance(false)
                .create(pipe_path)
            {
                Ok(s) => s,
                Err(e) => {
                    log!("yas-proxy: create pipe instance: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
            },
        };
        // Create the *next* instance before connecting, so the pipe name is
        // never unowned between connections.
        let upcoming = ServerOptions::new()
            .first_pipe_instance(false)
            .create(pipe_path);
        if let Err(e) = server.connect().await {
            log!("yas-proxy: pipe connect: {e}");
            // Put the upcoming server back for the next iteration.
            if let Ok(u) = upcoming {
                next_server = Some(u);
            }
            continue;
        }
        if let Ok(u) = upcoming {
            next_server = Some(u);
        }
        let activity = activity.clone();
        tokio::spawn(async move {
            handle_downstream(activity, server).await;
        });
    }
}

fn parse_target_command(line: &str) -> Option<&str> {
    let uri = line.strip_prefix("target-yas ")?;
    (!uri.is_empty()).then_some(uri)
}

async fn handle_downstream<S>(activity: Arc<Activity>, mut downstream: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // Handshake: read `target-yas <uri>` or `shutdown`. Endpoint selection
    // is explicit and complete before any YAS bytes are forwarded.
    let line = match read_line(&mut downstream).await {
        Some(line) => line,
        None => return,
    };
    if line == "shutdown" {
        let _ = downstream.write_all(b"ok\n").await;
        log!("yas-proxy: shutdown requested, exiting");
        std::process::exit(0);
    }
    let Some(uri) = parse_target_command(&line) else {
        let _ = downstream
            .write_all(b"error invalid native handshake\n")
            .await;
        return;
    };

    let (mut upstream_reader, mut upstream_writer) = match connect_yas_upstream_split(uri).await {
        Ok(upstream) => upstream,
        Err(error) => {
            let message = format!("error {error}\n");
            let _ = downstream.write_all(message.as_bytes()).await;
            return;
        }
    };
    if downstream.write_all(b"ok\n").await.is_err() {
        return;
    }

    activity.connected();
    let (mut downstream_reader, mut downstream_writer) = tokio::io::split(downstream);
    let mut client_to_server =
        tokio::spawn(
            async move { tokio::io::copy(&mut downstream_reader, &mut upstream_writer).await },
        );
    let mut server_to_client =
        tokio::spawn(
            async move { tokio::io::copy(&mut upstream_reader, &mut downstream_writer).await },
        );
    tokio::select! {
        _ = &mut client_to_server => {
            server_to_client.abort();
            let _ = server_to_client.await;
        },
        _ = &mut server_to_client => {
            client_to_server.abort();
            let _ = client_to_server.await;
        },
    }
    activity.disconnected();
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the proxy on the current thread (blocks until the process exits).
///
/// Reads `YAS_PROXY_SOCK` and `YAS_PROXY_IDLE` from the
/// environment. When called from within the `yas` binary instead of the
/// standalone `yas-proxy` binary, `verbose` should be `false`.
pub fn run(verbose: bool) {
    if verbose {
        VERBOSE.store(true, Ordering::Relaxed);
    }

    let socket = proxy_socket_spec().unwrap_or_else(|error| {
        eprintln!("yas-proxy: invalid socket configuration: {error}");
        std::process::exit(1);
    });
    let sock_path = socket.path.clone();

    #[cfg(unix)]
    let bound = bind_proxy_listener(std::path::Path::new(&sock_path), socket.automatic)
        .unwrap_or_else(|error| {
            eprintln!("yas-proxy: cannot bind to {sock_path}: {error}");
            std::process::exit(1);
        });

    let idle_secs: Option<u64> = std::env::var("YAS_PROXY_IDLE")
        .ok()
        .and_then(|v| v.parse().ok());

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("yas-proxy: tokio runtime")
        .block_on(async move {
            rustls::crypto::ring::default_provider()
                .install_default()
                .ok(); // may already be installed by the CLI's runtime

            let activity = Activity::new();

            // Idle-timeout watcher.
            if let Some(idle) = idle_secs {
                let activity = activity.clone();
                let sock = sock_path.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        let last = activity.latest();
                        if last == i64::MAX {
                            continue;
                        }
                        let elapsed = now_secs().saturating_sub(last) as u64;
                        if elapsed >= idle {
                            log!("yas-proxy: idle for {elapsed}s (limit {idle}s), exiting");
                            let _ = std::fs::remove_file(&sock);
                            std::process::exit(0);
                        }
                    }
                });
            }

            #[cfg(unix)]
            {
                let sock_cleanup = sock_path.clone();
                tokio::spawn(async move {
                    let _ = tokio::signal::ctrl_c().await;
                    let _ = std::fs::remove_file(&sock_cleanup);
                    std::process::exit(0);
                });
                if let Err(error) = run_listener(activity, &sock_path, bound).await {
                    eprintln!("yas-proxy: listener failed at {sock_path}: {error}");
                    std::process::exit(1);
                }
            }
            #[cfg(windows)]
            {
                // Create the first pipe instance before signalling readiness
                // so that clients polling the pipe path see it immediately.
                use tokio::net::windows::named_pipe::ServerOptions;
                let first = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&sock_path)
                    .unwrap_or_else(|e| {
                        eprintln!("yas-proxy: cannot create pipe {sock_path}: {e}");
                        std::process::exit(1);
                    });
                // Pass `first` into run_listener so the pipe name is never
                // unowned between creation and the first client connection.
                run_listener(activity, &sock_path, first).await;
            }
        });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_target_selection_is_explicit_and_never_sniffed() {
        assert_eq!(
            parse_target_command("target-yas ssh:host"),
            Some("ssh:host")
        );
        assert_eq!(parse_target_command("target ssh:host"), None);
        assert_eq!(parse_target_command("target-yas "), None);
        assert_eq!(parse_target_command("target-yas"), None);
        assert_eq!(parse_target_command("YAS1\\r\\n"), None);
    }

    #[tokio::test]
    async fn ws_frame_writer_reassembles_arbitrarily_chunked_stream_frames() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let first = b"short frame".to_vec();
        let second = (0..96 * 1024)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let expected_first = first.clone();
        let expected_second = second.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let one = ws.next().await.unwrap().unwrap();
            let two = ws.next().await.unwrap().unwrap();
            assert_eq!(one.into_data().as_ref(), expected_first);
            assert_eq!(two.into_data().as_ref(), expected_second);
        });

        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        let (sink, _source) = ws.split();
        let mut writer = WsFrameWriter::new(sink);
        let mut stream = Vec::new();
        stream.extend_from_slice(&(first.len() as u32).to_le_bytes());
        stream.extend_from_slice(&first);
        stream.extend_from_slice(&(second.len() as u32).to_le_bytes());
        stream.extend_from_slice(&second);

        // Split both the first length prefix and the second payload. The
        // 64-KiB cut mirrors relay DATA chunking for a large nested frame.
        let chunk_sizes = [1usize, 2, 13, 64 * 1024, 7, 4096];
        let mut offset = 0;
        let mut chunk = 0;
        while offset < stream.len() {
            let end = (offset + chunk_sizes[chunk % chunk_sizes.len()]).min(stream.len());
            writer.write_all(&stream[offset..end]).await.unwrap();
            // `tokio::io::copy` is allowed to flush between arbitrary source
            // reads, including the one-byte prefix fragment above.
            writer.flush().await.unwrap();
            offset = end;
            chunk += 1;
        }
        writer.flush().await.unwrap();
        writer.shutdown().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn yas_ws_writer_preserves_preface_then_adapts_stream_frames() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload = b"native YAS frame".to_vec();
        let expected = payload.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert_eq!(
                ws.next().await.unwrap().unwrap().into_data().as_ref(),
                yas_wire::PREFACE
            );
            assert_eq!(
                ws.next().await.unwrap().unwrap().into_data().as_ref(),
                expected
            );
        });

        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        let (sink, _source) = ws.split();
        let mut writer = WsFrameWriter::new_yas(sink);
        let mut stream = yas_wire::PREFACE.to_vec();
        stream.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        stream.extend_from_slice(&payload);
        for chunk in stream.chunks(3) {
            writer.write_all(chunk).await.unwrap();
            writer.flush().await.unwrap();
        }
        writer.shutdown().await.unwrap();
        server.await.unwrap();
    }

    #[test]
    fn parse_uplink_uri_forms() {
        assert!(parse_uplink_uri("eyJhbGciOi.eyJzaWQi.sig").is_err());

        let (token, control) = parse_uplink_uri("https://relay.example/#tok123").unwrap();
        assert_eq!(token, "tok123");
        assert_eq!(control, "https://relay.example");

        assert!(parse_uplink_uri("https://relay.example#").is_err());
        assert!(parse_uplink_uri("#tok123").is_err());
    }

    #[test]
    fn proxy_socket_path_default() {
        let p = automatic_proxy_socket_path();
        assert!(!p.is_empty());
        assert!(p.ends_with("yas-proxy.sock"));
    }

    #[cfg(unix)]
    #[test]
    fn automatic_proxy_path_has_an_owner_only_runtime_parent() {
        use std::os::unix::fs::MetadataExt;

        let path = automatic_proxy_socket_path();
        let parent = std::path::Path::new(&path).parent().unwrap();
        let metadata = std::fs::symlink_metadata(parent).unwrap();
        assert!(metadata.file_type().is_dir());
        assert_eq!(metadata.uid(), yas_webserver::local_ipc::effective_uid());
        assert_eq!(metadata.mode() & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn automatic_bind_refuses_prebound_object_and_sets_private_modes() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.path().join("yas-proxy.sock");
        let target = directory.path().join("credential-target");
        std::fs::write(&target, b"untouched").unwrap();
        symlink(&target, &socket).unwrap();
        assert!(bind_proxy_listener(&socket, true).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"untouched");

        std::fs::remove_file(&socket).unwrap();
        let bound = bind_proxy_listener(&socket, true).unwrap();
        assert_eq!(
            std::fs::symlink_metadata(&socket).unwrap().mode() & 0o777,
            0o700
        );
        let lock = std::path::PathBuf::from(format!("{}.lock", socket.display()));
        let lock_metadata = std::fs::symlink_metadata(lock).unwrap();
        assert_eq!(
            lock_metadata.uid(),
            yas_webserver::local_ipc::effective_uid()
        );
        assert_eq!(lock_metadata.mode() & 0o777, 0o600);
        drop(bound);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wrong_uid_proxy_is_rejected_before_target_bytes_and_autostart() {
        // Root is a trusted peer for every endpoint, so a root test runner
        // cannot exercise this rejection.
        if yas_webserver::local_ipc::effective_uid() == 0 {
            return;
        }
        use tokio::io::AsyncReadExt;

        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("prebound.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let accepted = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0u8; 1];
            stream.read(&mut byte).await.unwrap()
        });
        let expected_uid = yas_webserver::local_ipc::effective_uid() ^ 1;
        let spec = ProxySocketSpec {
            path: socket.to_string_lossy().into_owned(),
            automatic: false,
            expected_uid,
        };
        let error = ensure_proxy_at(
            std::path::Path::new("/definitely-not-a-proxy-binary"),
            false,
            spec,
        )
        .await
        .unwrap_err();
        assert!(error.contains("does not match expected UID"), "{error}");
        assert_eq!(accepted.await.unwrap(), 0, "target bytes reached prebind");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn missing_cross_uid_proxy_is_never_replaced_by_local_autostart() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("missing.sock");
        let expected_uid = yas_webserver::local_ipc::effective_uid() ^ 1;
        let spec = ProxySocketSpec {
            path: socket.to_string_lossy().into_owned(),
            automatic: false,
            expected_uid,
        };
        let error = ensure_proxy_at(
            std::path::Path::new("/definitely-not-a-proxy-binary"),
            false,
            spec,
        )
        .await
        .unwrap_err();
        assert!(error.contains("refusing to auto-start"), "{error}");
        assert!(!socket.exists());
    }

    #[test]
    fn parse_hex_valid() {
        assert_eq!(parse_hex("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(parse_hex(""), Some(vec![]));
    }

    #[test]
    fn parse_hex_odd_length() {
        assert_eq!(parse_hex("abc"), None);
    }

    #[test]
    fn parse_hex_invalid_char() {
        assert_eq!(parse_hex("zz"), None);
    }

    #[test]
    fn extract_uri_params_no_query() {
        let (base, pass, cert) = extract_uri_params("wss://host:3264/");
        assert_eq!(base, "wss://host:3264/");
        assert_eq!(pass, None);
        assert_eq!(cert, None);
    }

    #[test]
    fn extract_uri_params_passphrase() {
        let (base, pass, cert) = extract_uri_params("wss://host:3264/#secret");
        assert_eq!(base, "wss://host:3264/");
        assert_eq!(pass, Some("secret".into()));
        assert_eq!(cert, None);
    }

    #[test]
    fn extract_uri_params_both() {
        let (base, pass, cert) = extract_uri_params("wt://host:4433/?certHash=deadbeef#abc");
        assert_eq!(base, "wt://host:4433/");
        assert_eq!(pass, Some("abc".into()));
        assert_eq!(cert, Some("deadbeef".into()));
    }

    #[test]
    fn extract_uri_params_passphrase_is_percent_decoded() {
        let (base, pass, _) = extract_uri_params("ws://host/edge#two%20words%23");
        assert_eq!(base, "ws://host/edge");
        assert_eq!(pass, Some("two words#".into()));
    }

    #[test]
    fn extract_uri_params_ignores_a_query_passphrase() {
        // The old spelling put the secret in the request line. It is not
        // quietly honoured: a URI carrying it authenticates with nothing, and
        // the server says so, rather than the secret working and still leaking.
        let (base, pass, _) = extract_uri_params("ws://host/edge?passphrase=secret");
        assert_eq!(base, "ws://host/edge");
        assert_eq!(pass, None);
    }

    #[test]
    fn extract_uri_params_socket_unchanged() {
        let (base, pass, cert) = extract_uri_params("socket:/tmp/yas.sock");
        assert_eq!(base, "socket:/tmp/yas.sock");
        assert_eq!(pass, None);
        assert_eq!(cert, None);
    }

    #[test]
    fn parse_wt_host_port_with_port() {
        assert_eq!(parse_wt_host_port("host:4433"), Ok(("host".into(), 4433)));
    }

    #[test]
    fn parse_wt_host_port_default() {
        assert_eq!(parse_wt_host_port("host"), Ok(("host".into(), 443)));
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn parse_share_uri_no_hub() {
        let (pass, hub) = parse_share_uri("myphrase");
        assert_eq!(pass, "myphrase");
        assert_eq!(
            hub,
            yas_webrtc_forwarder::normalize_hub(yas_webrtc_forwarder::DEFAULT_HUB_URL)
        );
    }

    #[test]
    fn parse_share_uri_with_hub() {
        let (pass, hub) = parse_share_uri("myphrase?hub=wss://custom.example.com");
        assert_eq!(pass, "myphrase");
        assert_eq!(hub, "wss://custom.example.com");
    }

    #[test]
    fn parse_share_uri_hub_normalized() {
        let (pass, hub) = parse_share_uri("myphrase?hub=custom.example.com");
        assert_eq!(pass, "myphrase");
        assert_eq!(hub, "wss://custom.example.com");
    }

    #[test]
    fn parse_share_uri_percent_encoded_passphrase() {
        let (pass, hub) = parse_share_uri("my%3Fphrase");
        assert_eq!(pass, "my?phrase");
        assert_eq!(
            hub,
            yas_webrtc_forwarder::normalize_hub(yas_webrtc_forwarder::DEFAULT_HUB_URL)
        );
    }
}
