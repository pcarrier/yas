//! `yas uplink` — expose the local yas server through a relay.
//!
//! Authenticates to an HTTPS control endpoint with `YAS_UPLINK_TOKEN`,
//! receives a pool of WebTransport relays,
//! establishes a session with one, authenticates each consumer using pinned
//! Ed25519 keys over inner TLS 1.3, then bridges decrypted streams to local YAS.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use web_transport_quinn as wt;

/// Stream application error codes.
const CODE_SHUTDOWN: u32 = 2;

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const MAX_DATAGRAM_ROUTES: usize = 4_096;
const DATAGRAM_QUEUE: usize = 64;

type DatagramRoutes = yas_composite_transport::RoutedDatagramRoutes;

/// Derive only the public identity; private input never appears in diagnostics.
pub fn public_key_from_env() -> Result<yas_uplink::PublicKey, String> {
    let encoded = std::env::var("YAS_UPLINK_IDENTITY").map_err(|error| match error {
        std::env::VarError::NotPresent =>
            "YAS_UPLINK_IDENTITY is not set; load your existing private key or generate one with yas uplink-keygen --private".to_owned(),
        std::env::VarError::NotUnicode(_) =>
            "YAS_UPLINK_IDENTITY must be 43 characters of unpadded base64url".to_owned(),
    })?;
    yas_uplink::Identity::from_base64(&encoded)
        .map(|identity| identity.public_key())
        .map_err(|error| format!("YAS_UPLINK_IDENTITY: {error}"))
}

/// Build a consumer URI offline, with a locally derived server pin. The client
/// token is distinct from the producer token; only the relay can issue it.
pub fn connection_url(control: &str, client_token: &str) -> Result<String, String> {
    let mut url = url::Url::parse(control).map_err(|_| "invalid uplink control URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "uplink control URL must be HTTPS without userinfo, query parameters, or a fragment"
                .into(),
        );
    }
    if client_token.trim().is_empty() || client_token.chars().any(char::is_control) {
        return Err("client token must be nonempty and contain no control characters".into());
    }
    let public = public_key_from_env()?;
    let fragment = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", client_token)
        .append_pair("server", &public.to_string())
        .finish();
    url.set_fragment(Some(&fragment));
    Ok(format!("uplink:{url}"))
}

struct Relay {
    /// Connection URL as minted by the control plane (fragment stripped).
    /// The URL is the session credential — never log it; use `label`.
    url: url::Url,
    /// `host:port` for log messages.
    label: String,
    /// SHA-256 of the relay's certificate (DER), from the URL's `#sha256=`
    /// fragment; pins TLS verification instead of using system trust roots.
    cert_hash: Option<Vec<u8>>,
}

pub async fn cmd_uplink(
    url: String,
    identity: String,
    allow_client: Vec<String>,
) -> Result<(), String> {
    let identity = yas_uplink::Identity::from_base64(&identity)?;
    let allowed = allow_client
        .iter()
        .map(|key| key.parse())
        .collect::<Result<Vec<_>, _>>()?;
    let crypto = identity.server_config(allowed)?;
    let control = url::Url::parse(&url).map_err(|_| "invalid uplink control URL")?;
    if control.scheme() != "https"
        || !control.username().is_empty()
        || control.password().is_some()
        || control.fragment().is_some()
    {
        return Err("uplink control URL must be HTTPS without userinfo or a fragment".into());
    }
    let token = std::env::var("YAS_UPLINK_TOKEN").unwrap_or_default();
    if token.is_empty() {
        return Err("YAS_UPLINK_TOKEN is not set".into());
    }

    // The active session, shared with the ctrl-c arm so shutdown can close
    // it with CODE_SHUTDOWN instead of letting it idle out on the relay.
    let current: Arc<Mutex<Option<wt::Session>>> = Arc::new(Mutex::new(None));
    let current2 = current.clone();

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            if let Some(session) = current2.lock().unwrap().take() {
                session.close(CODE_SHUTDOWN, b"uplink shutting down");
            }
            Ok(())
        }
        result = run_loop(&url, &token, current, crypto) => result,
    }
}

async fn run_loop(
    url: &str,
    token: &str,
    current: Arc<Mutex<Option<wt::Session>>>,
    crypto: Arc<rustls::ServerConfig>,
) -> Result<(), String> {
    let http = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut backoff = INITIAL_BACKOFF;

    loop {
        let mut pool = match fetch_pool(&http, url, token).await? {
            FetchOutcome::Pool(pool) => pool,
            FetchOutcome::Retry { after, reason } => {
                let delay = after.unwrap_or(backoff);
                eprintln!("[uplink] {reason}; retrying in {}s", delay.as_secs());
                tokio::time::sleep(jittered(delay)).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }
        };

        {
            use rand::seq::SliceRandom;
            pool.shuffle(&mut rand::rng());
        }

        // Try relays in (shuffled) order; a session that was actually
        // established ends with a fresh control-plane query, per
        // docs/uplink.md.
        let mut established = false;
        for relay in &pool {
            match run_session(relay, &current, crypto.clone()).await {
                SessionEnd::Ended(reason) => {
                    eprintln!("[uplink] session ended: {reason}");
                    established = true;
                    break;
                }
                SessionEnd::NeverConnected(e) => {
                    eprintln!("[uplink] relay {}: {e}", relay.label);
                }
            }
        }

        if established {
            backoff = INITIAL_BACKOFF;
        } else {
            eprintln!(
                "[uplink] relay pool exhausted; re-querying in {}s",
                backoff.as_secs()
            );
            tokio::time::sleep(jittered(backoff)).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }
}

// ---------------------------------------------------------------------------
// Control plane
// ---------------------------------------------------------------------------

enum FetchOutcome {
    Pool(Vec<Relay>),
    Retry {
        after: Option<Duration>,
        reason: String,
    },
}

/// Query the control endpoint. `Err` is fatal (bad token); every other
/// failure is a retryable `FetchOutcome::Retry`.
async fn fetch_pool(
    http: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<FetchOutcome, String> {
    let retry = |after, reason| Ok(FetchOutcome::Retry { after, reason });

    let resp = match http
        .get(url)
        .header("authorization", format!("Bearer {token}"))
        .header("accept", "application/json")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => return retry(None, format!("control endpoint unreachable: {e}")),
    };

    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(format!(
            "control endpoint rejected YAS_UPLINK_TOKEN ({status})"
        ));
    }
    if !status.is_success() {
        let after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        return retry(after, format!("control endpoint returned {status}"));
    }

    let body = match resp.text().await {
        Ok(body) => body,
        Err(e) => return retry(None, format!("error reading relay pool: {e}")),
    };
    match parse_pool(&body) {
        Ok(pool) => Ok(FetchOutcome::Pool(pool)),
        Err(e) => retry(None, format!("bad relay pool: {e}")),
    }
}

fn parse_pool(body: &str) -> Result<Vec<Relay>, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;
    let relays = v
        .get("relays")
        .and_then(|r| r.as_array())
        .ok_or("missing \"relays\" array")?;

    let mut pool = Vec::new();
    for relay in relays {
        let s = relay.as_str().ok_or("relay entries must be URL strings")?;
        pool.push(parse_relay(s)?);
    }
    if pool.is_empty() {
        return Err("empty \"relays\" array".into());
    }
    Ok(pool)
}

fn parse_relay(s: &str) -> Result<Relay, String> {
    let mut url = url::Url::parse(s).map_err(|e| format!("bad relay URL: {e}"))?;
    if url.scheme() != "https" {
        return Err(format!(
            "relay URL scheme must be https, got {}",
            url.scheme()
        ));
    }
    let host = url.host_str().ok_or("relay URL has no host")?.to_string();
    let label = format!("{}:{}", host, url.port().unwrap_or(443));

    // A malformed pin must be an error, never a silent fall-back to system
    // roots — that would defeat the pinning.
    let cert_hash = match url.fragment() {
        None | Some("") => None,
        Some(frag) => {
            let hash = frag
                .strip_prefix("sha256=")
                .ok_or("relay URL fragment must be sha256=<base64url hash>")?;
            let bytes =
                base64url_decode(hash).ok_or("relay certificate pin is not valid base64url")?;
            if bytes.len() != 32 {
                return Err("relay certificate pin must be a SHA-256 (32 bytes)".into());
            }
            Some(bytes)
        }
    };
    // Fragments are client-side only; strip before connecting.
    url.set_fragment(None);

    Ok(Relay {
        url,
        label,
        cert_hash,
    })
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // Leftover bits must be padding zeros of a valid encoding.
    if bits > 0 && (acc & ((1 << bits) - 1)) != 0 {
        return None;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Relay session
// ---------------------------------------------------------------------------

enum SessionEnd {
    /// Handshake or CONNECT failed — try the next relay in the pool.
    NeverConnected(String),
    /// The session was established and later died — re-query the control
    /// plane for a fresh pool.
    Ended(String),
}

async fn run_session(
    relay: &Relay,
    current: &Arc<Mutex<Option<wt::Session>>>,
    crypto: Arc<rustls::ServerConfig>,
) -> SessionEnd {
    let client = match build_client(relay.cert_hash.as_deref()) {
        Ok(client) => client,
        Err(e) => return SessionEnd::NeverConnected(e),
    };
    // Careful: the URL is the credential — log `relay.label` only.
    let session = match client.connect(relay.url.clone()).await {
        Ok(session) => session,
        Err(e) => return SessionEnd::NeverConnected(format!("connect failed: {e}")),
    };
    eprintln!("[uplink] connected to relay {}", relay.label);
    *current.lock().unwrap() = Some(session.clone());

    let routes = DatagramRoutes::new(MAX_DATAGRAM_ROUTES);
    tokio::spawn(distribute_datagrams(session.clone(), routes.clone()));
    let pending = Arc::new(tokio::sync::Semaphore::new(64));
    let local_socket = crate::transport::default_local_socket();

    let reason = loop {
        match session.accept_bi().await {
            Ok((send, recv)) => {
                let Ok(permit) = pending.clone().try_acquire_owned() else {
                    continue;
                };
                tokio::spawn(bridge(
                    session.clone(),
                    routes.clone(),
                    send,
                    recv,
                    crypto.clone(),
                    permit,
                    local_socket.clone(),
                ));
            }
            Err(e) => break format!("{e}"),
        }
    };
    current.lock().unwrap().take();
    SessionEnd::Ended(reason)
}

/// Bridge one relay-initiated stream to one local yas server connection.
/// Authentication precedes both ingress classification and local IPC. The
/// relay sees only TLS records; plaintext selectors cannot bypass admission.
async fn bridge(
    session: wt::Session,
    routes: DatagramRoutes,
    send: wt::SendStream,
    recv: wt::RecvStream,
    crypto: Arc<rustls::ServerConfig>,
    permit: tokio::sync::OwnedSemaphorePermit,
    local_socket: String,
) {
    let relay = tokio::io::join(recv, send);
    let relay = match yas_uplink::accept(relay, crypto).await {
        Ok(relay) => relay,
        Err(_) => return,
    };
    // Bind each sideband's keys to its authenticated main stream. The random
    // route token remains authenticated as AEAD AAD on every datagram.
    let material = match relay.get_ref().1.export_keying_material(
        yas_uplink::DatagramKeyMaterial::new([0u8; 64]),
        yas_uplink::DATAGRAM_EXPORTER_LABEL,
        Some(&[]),
    ) {
        Ok(material) => material,
        Err(_) => return,
    };
    let ingress = tokio::time::timeout(
        Duration::from_secs(5),
        yas_composite_transport::classify(relay),
    )
    .await;
    drop(permit);
    match ingress {
        Ok(Ok(yas_composite_transport::Ingress::Direct(relay))) => {
            bridge_direct(relay, &local_socket).await
        }
        Ok(Ok(yas_composite_transport::Ingress::Composite { offer, stream }))
            if offer.role == yas_composite_transport::Role::Main =>
        {
            let (sender, receiver) = yas_uplink::datagram_pair(material, offer.token, false);
            bridge_composite(
                session,
                routes,
                offer,
                stream,
                sender,
                receiver,
                &local_socket,
            )
            .await;
        }
        _ => {}
    }
}

async fn bridge_direct<S>(relay: S, path: &str)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let transport = match crate::transport::connect_ipc(path).await {
        Ok(transport) => transport,
        Err(e) => {
            eprintln!("[uplink] local yas server unavailable at {path}: {e}");
            return;
        }
    };
    let (mut sock_read, mut sock_write) = transport.split();
    let (mut recv, mut send) = tokio::io::split(relay);

    let down = async move {
        tokio::io::copy(&mut recv, &mut sock_write).await?;
        sock_write.shutdown().await
    };
    let up = async move {
        tokio::io::copy(&mut sock_read, &mut send).await?;
        send.shutdown().await
    };
    let _ = tokio::try_join!(down, up);
}

async fn distribute_datagrams(session: wt::Session, routes: DatagramRoutes) {
    while let Ok(bytes) = session.read_datagram().await {
        let _ = routes.route(&bytes);
    }
    routes.clear();
}

async fn bridge_composite<S>(
    session: wt::Session,
    routes: DatagramRoutes,
    offer: yas_composite_transport::Offer,
    relay: S,
    mut sender: yas_uplink::DatagramSender,
    mut receiver: yas_uplink::DatagramReceiver,
    path: &str,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let physical_maximum = session
        .max_datagram_size()
        .saturating_sub(yas_composite_transport::ROUTED_DATAGRAM_HEADER)
        .saturating_sub(yas_uplink::DATAGRAM_OVERHEAD)
        .min(yas_composite_transport::HARD_MAX_DATAGRAM as usize - yas_uplink::DATAGRAM_OVERHEAD);
    if offer.max_datagram as usize > physical_maximum {
        eprintln!(
            "[uplink] rejected composite maximum {} above path maximum {physical_maximum}",
            offer.max_datagram
        );
        return;
    }

    let main = match crate::transport::connect_ipc(path).await {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("[uplink] local yas server unavailable at {path}: {error}");
            return;
        }
    };
    let side = match crate::transport::connect_ipc(path).await {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("[uplink] local YAS datagram sideband unavailable at {path}: {error}");
            return;
        }
    };
    let (mut main_read, mut main_write) = main.split();
    let (mut side_read, mut side_write) = side.split();
    let main_offer = yas_composite_transport::Offer::new(
        yas_composite_transport::Role::Main,
        offer.token,
        offer.max_datagram,
    )
    .expect("the classified offer is valid");
    let side_offer = yas_composite_transport::Offer::new(
        yas_composite_transport::Role::Datagram,
        offer.token,
        offer.max_datagram,
    )
    .expect("the classified offer is valid");
    if yas_composite_transport::write_offer(&mut main_write, main_offer)
        .await
        .is_err()
        || yas_composite_transport::write_offer(&mut side_write, side_offer)
            .await
            .is_err()
    {
        return;
    }

    let encrypted_maximum = offer.max_datagram + yas_uplink::DATAGRAM_OVERHEAD as u32;
    let Ok(mut route_rx) = routes.register(offer.token, encrypted_maximum, DATAGRAM_QUEUE) else {
        return;
    };

    let (mut relay_read, mut relay_write) = tokio::io::split(relay);
    let down = async {
        tokio::io::copy(&mut relay_read, &mut main_write).await?;
        main_write.shutdown().await
    };
    let up = async {
        tokio::io::copy(&mut main_read, &mut relay_write).await?;
        relay_write.shutdown().await
    };
    let side_routes = routes.clone();
    let side_token = offer.token;
    let side_task = tokio::spawn(async move {
        let side_out = async {
            loop {
                let Ok(frame) =
                    yas_composite_transport::read_datagram(&mut side_read, offer.max_datagram)
                        .await
                else {
                    break;
                };
                let Some(encrypted) = sender.seal(&frame) else {
                    break;
                };
                let Ok(routed) = yas_composite_transport::encode_routed_datagram(
                    offer.token,
                    &encrypted,
                    encrypted_maximum,
                ) else {
                    break;
                };
                // Congestion is ordinary datagram loss; never await reliable
                // transport capacity or fall back to the main stream here.
                let _ = session.send_datagram(routed.into());
            }
        };
        let side_in = async {
            while let Some(frame) = route_rx.recv().await {
                let Some(frame) = receiver.open(&frame) else {
                    continue;
                };
                if yas_composite_transport::write_datagram(
                    &mut side_write,
                    &frame,
                    offer.max_datagram,
                )
                .await
                .is_err()
                {
                    break;
                }
            }
        };
        tokio::select! {
            _ = side_out => {}
            _ = side_in => {}
        }
        side_routes.remove(side_token);
    });

    // The authoritative reliable splice owns the connection lifetime. An
    // optional sideband ending only removes its route and cannot drop `main`.
    let _ = tokio::try_join!(down, up);
    routes.remove(offer.token);
    side_task.abort();
}

/// Build a WebTransport client with the liveness settings from
/// docs/uplink.md (10s keepalive, 30s idle timeout) and either
/// system-root or pinned TLS verification.  `wt::ClientBuilder` doesn't expose the quinn transport
/// config, so this mirrors its setup by hand.
fn build_client(cert_hash: Option<&[u8]>) -> Result<wt::Client, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("TLS config: {e}"))?;

    let mut crypto = match cert_hash {
        Some(hash) => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedCert {
                hash: hash.to_vec(),
                provider,
            }))
            .with_no_client_auth(),
        None => {
            let mut roots = rustls::RootCertStore::empty();
            for cert in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(cert);
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    crypto.alpn_protocols = vec![wt::ALPN.as_bytes().to_vec()];

    let quic_crypto = wt::quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
        .map_err(|e| format!("QUIC TLS config: {e}"))?;
    let mut config = wt::quinn::ClientConfig::new(Arc::new(quic_crypto));

    let mut transport = wt::quinn::TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    transport.max_idle_timeout(Some(
        wt::quinn::IdleTimeout::try_from(Duration::from_secs(30))
            .expect("30s fits in an idle timeout"),
    ));
    config.transport_config(Arc::new(transport));

    let endpoint = wt::quinn::Endpoint::client("[::]:0".parse().unwrap())
        .or_else(|_| wt::quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()))
        .map_err(|e| format!("UDP socket: {e}"))?;
    Ok(wt::Client::new(endpoint, config))
}

/// Pins the relay's end-entity certificate to a SHA-256 hash from the pool
/// (the edge's `serverCertificateHashes` flow, client side). Chain and
/// expiry are deliberately not checked — the hash is the trust anchor.
#[derive(Debug)]
struct PinnedCert {
    hash: Vec<u8>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for PinnedCert {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let digest = ring::digest::digest(&ring::digest::SHA256, end_entity.as_ref());
        if digest.as_ref() == self.hash.as_slice() {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerIncompatible(
            rustls::PeerIncompatible::Tls12NotOffered,
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn jittered(base: Duration) -> Duration {
    use rand::RngExt as _;
    let ms = base.as_millis().max(1) as u64;
    // 0.75x–1.25x
    Duration::from_millis(rand::rng().random_range(ms * 3 / 4..=ms * 5 / 4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn producer_authenticates_before_ipc_and_encrypts_datagrams() {
        use tokio::io::AsyncReadExt;
        use yas_composite_transport::{Offer, Role};
        let _ = rustls::crypto::ring::default_provider().install_default();
        tokio::time::timeout(Duration::from_secs(15), async {
            let dir = tempfile::tempdir().unwrap();
            let (server_key, server_public) = yas_uplink::Identity::generate().unwrap();
            let (client_key, client_public) = yas_uplink::Identity::generate().unwrap();
            let (attacker_key, _) = yas_uplink::Identity::generate().unwrap();
            let crypto = yas_uplink::Identity::from_base64(&server_key)
                .unwrap()
                .server_config(vec![client_public])
                .unwrap();
            let client_crypto = yas_uplink::Identity::from_base64(&client_key)
                .unwrap()
                .client_config(server_public)
                .unwrap();
            let attacker_crypto = yas_uplink::Identity::from_base64(&attacker_key)
                .unwrap()
                .client_config(server_public)
                .unwrap();
            let socket = dir.path().join("local.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();

            let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
            let hash = ring::digest::digest(&ring::digest::SHA256, cert.cert.der());
            let mut worker = wt::ServerBuilder::new()
                .with_addr("127.0.0.1:0".parse().unwrap())
                .with_certificate(
                    vec![cert.cert.der().clone()],
                    rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())
                        .into(),
                )
                .unwrap();
            let outer = build_client(Some(hash.as_ref())).unwrap();
            let url: url::Url =
                format!("https://127.0.0.1:{}/", worker.local_addr().unwrap().port())
                    .parse()
                    .unwrap();
            let (local, remote) = tokio::join!(outer.connect(url), async {
                worker.accept().await.unwrap().ok().await.unwrap()
            });
            let local = local.unwrap();
            let routes = DatagramRoutes::new(16);
            let datagrams = tokio::spawn(distribute_datagrams(local.clone(), routes.clone()));

            for case in 0..4 {
                let local = local.clone();
                let routes = routes.clone();
                let crypto = crypto.clone();
                let socket = socket.to_str().unwrap().to_string();
                let producer = tokio::spawn(async move {
                    let (send, recv) = local.accept_bi().await.unwrap();
                    let permit = Arc::new(tokio::sync::Semaphore::new(1))
                        .acquire_owned()
                        .await
                        .unwrap();
                    bridge(local, routes, send, recv, crypto, permit, socket).await;
                });
                let (send, recv) = remote.open_bi().await.unwrap();
                let mut stream = tokio::io::join(recv, send);
                if case == 0 {
                    // A relay can synthesize protocol bytes, but those bytes
                    // must never open a local socket before inner TLS.
                    stream
                        .write_all(yas_wire::PREFACE.as_slice())
                        .await
                        .unwrap();
                    stream.shutdown().await.unwrap();
                    drop(stream);
                    producer.await.unwrap();
                    assert!(
                        tokio::time::timeout(Duration::from_millis(25), listener.accept())
                            .await
                            .is_err()
                    );
                    continue;
                }
                if case == 1 {
                    assert!(
                        yas_uplink::connect(stream, attacker_crypto.clone())
                            .await
                            .is_err()
                    );
                    producer.await.unwrap();
                    assert!(
                        tokio::time::timeout(Duration::from_millis(25), listener.accept())
                            .await
                            .is_err()
                    );
                    continue;
                }
                let mut stream = yas_uplink::connect(stream, client_crypto.clone())
                    .await
                    .unwrap();
                // Authentication alone also does not open IPC: the encrypted
                // YAS preface or composite selector must follow first.
                assert!(
                    tokio::time::timeout(Duration::from_millis(25), listener.accept())
                        .await
                        .is_err()
                );
                if case == 2 {
                    stream
                        .write_all(yas_wire::PREFACE.as_slice())
                        .await
                        .unwrap();
                    stream.write_all(b"command").await.unwrap();
                    stream.flush().await.unwrap();
                    let (mut ipc, _) = listener.accept().await.unwrap();
                    let mut received = vec![0; yas_wire::PREFACE.len() + 7];
                    ipc.read_exact(&mut received).await.unwrap();
                    assert_eq!(
                        received,
                        [yas_wire::PREFACE.as_slice(), b"command"].concat()
                    );
                    ipc.write_all(b"reply").await.unwrap();
                    ipc.shutdown().await.unwrap();
                    let mut reply = Vec::new();
                    stream.read_to_end(&mut reply).await.unwrap();
                    assert_eq!(reply, b"reply");
                    stream.shutdown().await.unwrap();
                    producer.await.unwrap();
                    continue;
                }

                let token = [0x35; 16];
                let maximum = 512;
                let material = stream
                    .get_ref()
                    .1
                    .export_keying_material(
                        yas_uplink::DatagramKeyMaterial::new([0; 64]),
                        yas_uplink::DATAGRAM_EXPORTER_LABEL,
                        Some(&[]),
                    )
                    .unwrap();
                let (mut sender, mut receiver) = yas_uplink::datagram_pair(material, token, true);
                yas_composite_transport::write_offer(
                    &mut stream,
                    Offer::new(Role::Main, token, maximum).unwrap(),
                )
                .await
                .unwrap();
                stream.flush().await.unwrap();
                let (mut main, _) = listener.accept().await.unwrap();
                let (mut side, _) = listener.accept().await.unwrap();
                for (socket, expected) in [(&mut main, Role::Main), (&mut side, Role::Datagram)] {
                    match yas_composite_transport::classify(socket).await.unwrap() {
                        yas_composite_transport::Ingress::Composite { offer, .. } => {
                            assert_eq!(offer.role, expected);
                            assert_eq!(offer.token, token);
                            assert_eq!(offer.max_datagram, maximum);
                        }
                        _ => panic!("missing local composite selector"),
                    }
                }
                // A reliable reply also synchronizes route registration.
                main.write_all(b"ready").await.unwrap();
                let mut ready = [0; 5];
                stream.read_exact(&mut ready).await.unwrap();
                assert_eq!(&ready, b"ready");

                let encrypted = sender.seal(b"private datagram").unwrap();
                let wire_maximum = maximum + yas_uplink::DATAGRAM_OVERHEAD as u32;
                let mut forged = encrypted.clone();
                *forged.last_mut().unwrap() ^= 1;
                for bytes in [&forged, &encrypted, &encrypted] {
                    let routed =
                        yas_composite_transport::encode_routed_datagram(token, bytes, wire_maximum)
                            .unwrap();
                    remote.send_datagram(routed.into()).unwrap();
                }
                assert_eq!(
                    yas_composite_transport::read_datagram(&mut side, maximum)
                        .await
                        .unwrap(),
                    b"private datagram"
                );
                assert!(
                    tokio::time::timeout(
                        Duration::from_millis(50),
                        yas_composite_transport::read_datagram(&mut side, maximum)
                    )
                    .await
                    .is_err()
                );

                yas_composite_transport::write_datagram(&mut side, b"private reply", maximum)
                    .await
                    .unwrap();
                let packet = remote.read_datagram().await.unwrap();
                let (received_token, ciphertext) =
                    yas_composite_transport::split_routed_datagram(&packet).unwrap();
                assert_eq!(received_token, token);
                assert!(!ciphertext.windows(13).any(|w| w == b"private reply"));
                assert_eq!(receiver.open(ciphertext).unwrap(), b"private reply");
                stream.shutdown().await.unwrap();
                main.shutdown().await.unwrap();
                producer.await.unwrap();
            }
            datagrams.abort();
            local.close(0, b"test complete");
        })
        .await
        .expect("producer uplink test stalled");
    }

    #[test]
    fn parse_pool_accepts_plain_and_pinned_relay_urls() {
        let pool = parse_pool(
            r#"{"relays":[
                "https://relay-1.indent.com:4443/t/kfV3aB",
                "https://[2001:db8::7]/session?key=x#sha256=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            ],"ttl":60}"#,
        )
        .unwrap();
        assert_eq!(pool.len(), 2);
        assert_eq!(
            pool[0].url.as_str(),
            "https://relay-1.indent.com:4443/t/kfV3aB"
        );
        assert_eq!(pool[0].label, "relay-1.indent.com:4443");
        assert!(pool[0].cert_hash.is_none());
        assert_eq!(pool[1].label, "[2001:db8::7]:443");
        assert_eq!(pool[1].cert_hash.as_ref().unwrap().len(), 32);
        // The pin must be stripped before the URL is used to connect.
        assert_eq!(pool[1].url.fragment(), None);
        assert_eq!(pool[1].url.query(), Some("key=x"));
    }

    #[test]
    fn parse_pool_rejects_bad_input() {
        assert!(parse_pool("not json").is_err());
        assert!(parse_pool(r#"{"relays":[]}"#).is_err());
        assert!(parse_pool(r#"{"relays":[{"host":"h"}]}"#).is_err());
        assert!(
            parse_relay("http://relay.example/t/x").is_err(),
            "non-https scheme must be rejected"
        );
        assert!(
            parse_relay("https://relay.example/t/x#sha256=AAAA").is_err(),
            "a pin of the wrong length must be rejected, not ignored"
        );
        assert!(
            parse_relay("https://relay.example/t/x#pin=abc").is_err(),
            "an unrecognized fragment must be rejected, not ignored"
        );
    }

    #[test]
    fn base64url_decodes() {
        assert_eq!(base64url_decode("aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64url_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64url_decode("_-8").unwrap(), vec![0xff, 0xef]);
        assert!(base64url_decode("a+b").is_none());
        assert_eq!(base64url_decode("").unwrap(), Vec::<u8>::new());
    }
}
