//! TCP and UDP relay (docs/design/net.md).
//! Connection-scoped sockets: the client names a host and port, the server opens a socket and copies payload.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::{Mutex, Notify};

use yas_wire::net as yas_net;

pub(crate) trait NativeIo:
    tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send
{
}
impl<T> NativeIo for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}

/// Connect and TLS-handshake timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TLS_TIMEOUT: Duration = Duration::from_secs(10);

/// Per-flow queue byte cap, alongside the datagram count cap: 256 maximum sized datagrams would be 16 MiB, which is not a bound worth having.
const DGRAM_QUEUE_BYTES: usize = 1024 * 1024;

/// What the relay may reach, and whether it may skip TLS verification
/// (docs/design/net.md § Target policy).
///
/// **Unrestricted by default.** With no pattern the relay reaches whatever
/// the host reaches, which is the useful default for a server you run on
/// your own machines. Patterns turn it into an allowlist, for an operator
/// exposing a server to clients they do not fully trust — without them the
/// only control is `YAS_NET=0`, which turns the family off entirely.
#[derive(Clone, Debug, Default)]
pub struct Policy {
    insecure_allowed: bool,
    /// Empty = unrestricted, but only when `restricted` is false.
    allow: Vec<TargetRule>,
    /// An allowlist was asked for. Kept separate from `allow` being
    /// non-empty so that patterns which all fail to parse cannot widen the
    /// policy back to unrestricted — an operator who typed
    /// `--allow-forward` and mistyped it should get loopback, not the
    /// internet.
    restricted: bool,
}

impl Policy {
    /// `allow` are `host[:ports]` patterns; unparsable ones are reported and
    /// dropped, and patterns that all fail to parse leave loopback only
    /// rather than widening back to unrestricted.
    pub fn new(insecure_allowed: bool, allow: &[String]) -> Self {
        let env = std::env::var("YAS_ALLOW_FORWARD").unwrap_or_default();
        let patterns = allow
            .iter()
            .map(String::as_str)
            .chain(env.split(',').filter(|p| !p.trim().is_empty()));
        let mut rules = Vec::new();
        let mut restricted = false;
        for pattern in patterns {
            restricted = true;
            match TargetRule::parse(pattern.trim()) {
                Some(rule) => rules.push(rule),
                None => eprintln!("yas: ignoring unparsable --allow-forward {pattern:?}"),
            }
        }
        if restricted && rules.is_empty() {
            eprintln!("yas: no --allow-forward pattern parsed; the relay reaches loopback only");
        }
        Self {
            insecure_allowed: insecure_allowed
                || std::env::var("YAS_ALLOW_FORWARD_INSECURE").is_ok_and(|v| v == "1"),
            allow: rules,
            restricted,
        }
    }

    fn insecure_allowed(&self) -> bool {
        self.insecure_allowed
    }

    /// Whether the requested `host` may be reached on `port`.
    ///
    /// Checked against the requested *name* before resolution, so a name rule
    /// authorizes whatever that name resolves to — precisely the grant an
    /// operator writing `*.svc.internal` is asking for. Address and CIDR
    /// rules are checked again against the resolved addresses by
    /// [`Self::permits_addr`], which is what the connect actually uses: the
    /// gap between check and connect is where DNS rebinding lives, and the
    /// only way to close it is to check the address you are about to dial.
    fn permits_host(&self, host: &str, port: u16) -> bool {
        if !self.restricted {
            return true;
        }
        // Loopback always works, so a dev server does not need a rule
        // (docs/design/net.md § Target policy).
        if is_loopback_host(host) {
            return true;
        }
        self.allow.iter().any(|r| r.matches_host(host, port))
    }

    /// Whether a resolved address may be dialed on `port`. A name rule that
    /// already matched the requested host authorizes its addresses; address
    /// and CIDR rules are matched here.
    fn permits_addr(&self, host: &str, addr: SocketAddr) -> bool {
        if !self.restricted || addr.ip().is_loopback() || is_loopback_host(host) {
            return true;
        }
        self.allow
            .iter()
            .any(|r| r.matches_addr(host, addr.ip(), addr.port()))
    }

    /// Open a YAS TCP flow through the shared name/address policy and
    /// DNS-rebinding checks.
    pub(crate) async fn connect_native_tcp(
        &self,
        host: &str,
        port: u16,
        tls_options: Option<&yas_net::TlsOptions>,
    ) -> Result<NativeTcp, NativeConnectError> {
        if !self.permits_host(host, port) {
            return Err(NativeConnectError::Permission);
        }
        let resolved = resolve_target(host, port).await?;
        let mut last_error = None;
        let mut denied = false;
        for addr in resolved {
            if !self.permits_addr(host, addr) {
                denied = true;
                continue;
            }
            match tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(addr)).await
            {
                Ok(Ok(stream)) => {
                    let _ = stream.set_nodelay(true);
                    let local_address = stream
                        .local_addr()
                        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
                    let peer_address = stream
                        .peer_addr()
                        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
                    let (stream, negotiated_alpn): (Box<dyn NativeIo>, Vec<u8>) =
                        if let Some(options) = tls_options {
                            if options.verification == yas_net::TlsVerification::Insecure
                                && !self.insecure_allowed()
                            {
                                return Err(NativeConnectError::Permission);
                            }
                            let sni = if options.sni.is_empty() {
                                host
                            } else {
                                options.sni.as_str()
                            };
                            let (tls, alpn) = native_tls(stream, sni, options).await?;
                            (Box::new(tls), alpn)
                        } else {
                            (Box::new(stream), Vec::new())
                        };
                    return Ok(NativeTcp {
                        stream,
                        local_address,
                        peer_address,
                        negotiated_alpn,
                    });
                }
                Ok(Err(error)) => last_error = Some(error.to_string()),
                Err(_) => last_error = Some("connect timed out".to_owned()),
            }
        }
        if denied {
            Err(NativeConnectError::Permission)
        } else {
            Err(NativeConnectError::Refused(
                last_error.unwrap_or_else(|| "no route".to_owned()),
            ))
        }
    }

    /// Open a connected native YAS UDP flow. Connecting is important: it
    /// pins both send and receive to the authorized peer and prevents the
    /// relay from becoming a general-purpose reflector.
    pub(crate) async fn connect_native_udp(
        &self,
        host: &str,
        port: u16,
    ) -> Result<NativeUdp, NativeConnectError> {
        if !self.permits_host(host, port) {
            return Err(NativeConnectError::Permission);
        }
        let resolved = resolve_target(host, port).await?;
        let mut denied = false;
        let mut last_error = None;
        for addr in resolved {
            if !self.permits_addr(host, addr) {
                denied = true;
                continue;
            }
            let bind: SocketAddr = if addr.is_ipv4() {
                "0.0.0.0:0".parse().expect("valid IPv4 wildcard")
            } else {
                "[::]:0".parse().expect("valid IPv6 wildcard")
            };
            let socket = match tokio::net::UdpSocket::bind(bind).await {
                Ok(socket) => socket,
                Err(error) => {
                    last_error = Some(error.to_string());
                    continue;
                }
            };
            if let Err(error) = socket.connect(addr).await {
                last_error = Some(error.to_string());
                continue;
            }
            let local_address = socket
                .local_addr()
                .map_err(|error| NativeConnectError::Io(error.to_string()))?;
            let peer_address = socket
                .peer_addr()
                .map_err(|error| NativeConnectError::Io(error.to_string()))?;
            return Ok(NativeUdp {
                socket: Arc::new(socket),
                local_address,
                peer_address,
            });
        }
        if denied {
            Err(NativeConnectError::Permission)
        } else {
            Err(NativeConnectError::Refused(
                last_error.unwrap_or_else(|| "no route".to_owned()),
            ))
        }
    }

    #[cfg(windows)]
    pub(crate) async fn connect_native_windows_pipe(
        &self,
        name: &str,
        requested_mode: yas_net::PipeMode,
    ) -> Result<NativeWindowsPipe, NativeConnectError> {
        connect_windows_pipe(name, requested_mode).await
    }

    #[cfg(unix)]
    pub(crate) async fn connect_native_unix_stream(
        &self,
        name: &yas_net::UnixName,
    ) -> Result<NativeUnixStream, NativeConnectError> {
        let stream = connect_unix_stream(name).await?;
        Ok(NativeUnixStream {
            stream: Box::new(stream),
            peer: name.clone(),
        })
    }

    #[cfg(unix)]
    pub(crate) async fn connect_native_unix_datagram(
        &self,
        name: &yas_net::UnixName,
    ) -> Result<NativeUnixDatagram, NativeConnectError> {
        connect_unix_datagram(name).await
    }

    #[cfg(unix)]
    pub(crate) async fn connect_native_unix_seqpacket(
        &self,
        name: &yas_net::UnixName,
    ) -> Result<NativeUnixSeqpacket, NativeConnectError> {
        connect_unix_seqpacket(name).await
    }
}

#[derive(Debug)]
pub(crate) enum NativeConnectError {
    Permission,
    NotFound(String),
    Refused(String),
    Io(String),
}

pub(crate) struct NativeTcp {
    pub(crate) stream: Box<dyn NativeIo>,
    pub(crate) local_address: SocketAddr,
    pub(crate) peer_address: SocketAddr,
    pub(crate) negotiated_alpn: Vec<u8>,
}

pub(crate) struct NativeUdp {
    pub(crate) socket: Arc<tokio::net::UdpSocket>,
    pub(crate) local_address: SocketAddr,
    pub(crate) peer_address: SocketAddr,
}

#[cfg(windows)]
pub(crate) enum NativeWindowsPipe {
    Byte {
        stream: Box<dyn NativeIo>,
        server_instance_limit: u32,
    },
    Message {
        socket: Arc<NativeWindowsMessagePipe>,
        server_instance_limit: u32,
        max_message_bytes: u64,
    },
}

#[cfg(windows)]
pub(crate) struct NativeWindowsMessagePipe {
    pipe: tokio::net::windows::named_pipe::NamedPipeClient,
}

#[cfg(windows)]
impl NativeWindowsMessagePipe {
    pub(crate) async fn send(&self, payload: &[u8]) -> std::io::Result<usize> {
        loop {
            self.pipe.writable().await?;
            match self.pipe.try_write(payload) {
                Ok(written) => return Ok(written),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) async fn recv(&self, payload: &mut [u8]) -> std::io::Result<usize> {
        loop {
            self.pipe.readable().await?;
            match self.pipe.try_read(payload) {
                Ok(read) => return Ok(read),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn shutdown_write(&self) -> std::io::Result<()> {
        // Windows named pipes do not expose an independent client write-half
        // shutdown. The Net owner retains the handle until the peer direction
        // also closes or the flow is reset.
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) struct NativeUnixStream {
    pub(crate) stream: Box<dyn NativeIo>,
    pub(crate) peer: yas_net::UnixName,
}

#[cfg(unix)]
pub(crate) struct NativeUnixDatagram {
    pub(crate) socket: Arc<tokio::net::UnixDatagram>,
    pub(crate) local: yas_net::UnixName,
    pub(crate) peer: yas_net::UnixName,
    _filesystem_cleanup: Option<std::path::PathBuf>,
}

#[cfg(unix)]
pub(crate) struct NativeUnixSeqpacket {
    socket: tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>,
}

#[cfg(unix)]
impl NativeUnixSeqpacket {
    pub(crate) async fn send(&self, payload: &[u8]) -> std::io::Result<usize> {
        use std::os::fd::AsRawFd;
        loop {
            let mut ready = self.socket.writable().await?;
            match ready.try_io(|socket| {
                let written = unsafe {
                    libc::send(
                        socket.get_ref().as_raw_fd(),
                        payload.as_ptr().cast(),
                        payload.len(),
                        libc::MSG_NOSIGNAL,
                    )
                };
                if written < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(written as usize)
                }
            }) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    pub(crate) async fn recv(&self, payload: &mut [u8]) -> std::io::Result<usize> {
        use std::os::fd::AsRawFd;
        loop {
            let mut ready = self.socket.readable().await?;
            match ready.try_io(|socket| {
                let read = unsafe {
                    libc::recv(
                        socket.get_ref().as_raw_fd(),
                        payload.as_mut_ptr().cast(),
                        payload.len(),
                        0,
                    )
                };
                if read < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(read as usize)
                }
            }) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    pub(crate) fn shutdown_write(&self) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        if unsafe { libc::shutdown(self.socket.get_ref().as_raw_fd(), libc::SHUT_WR) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

async fn native_tls(
    stream: tokio::net::TcpStream,
    sni: &str,
    options: &yas_net::TlsOptions,
) -> Result<
    (
        tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
        Vec<u8>,
    ),
    NativeConnectError,
> {
    let server_name = rustls::pki_types::ServerName::try_from(sni.to_owned())
        .map_err(|_| NativeConnectError::Io("invalid TLS server name".to_owned()))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
    let mut config = if options.verification == yas_net::TlsVerification::Insecure {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify { provider }))
            .with_no_client_auth()
    } else {
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        if roots.is_empty() {
            return Err(NativeConnectError::Io(
                "no system TLS trust roots available".to_owned(),
            ));
        }
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    config.alpn_protocols.clone_from(&options.alpn);
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tls = tokio::time::timeout(TLS_TIMEOUT, connector.connect(server_name, stream))
        .await
        .map_err(|_| NativeConnectError::Io("TLS handshake timed out".to_owned()))?
        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
    let alpn = tls
        .get_ref()
        .1
        .alpn_protocol()
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    Ok((tls, alpn))
}

/// Certificate verifier used only when the operator explicitly enables
/// insecure forwarding and the native request selects it.
#[derive(Debug)]
struct NoVerify {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
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

#[cfg(unix)]
fn unix_sockaddr(
    name: &yas_net::UnixName,
) -> Result<(libc::sockaddr_un, libc::socklen_t), NativeConnectError> {
    use std::mem::offset_of;
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let offset = offset_of!(libc::sockaddr_un, sun_path);
    match name.kind {
        yas_net::UnixNameKind::Filesystem => {
            if name.name.len() >= address.sun_path.len() {
                return Err(NativeConnectError::Io("Unix path is too long".to_owned()));
            }
            for (target, source) in address.sun_path.iter_mut().zip(&name.name) {
                *target = *source as libc::c_char;
            }
            let length = offset + name.name.len() + 1;
            Ok((address, length as libc::socklen_t))
        }
        yas_net::UnixNameKind::Abstract => {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                if name.name.len() + 1 > address.sun_path.len() {
                    return Err(NativeConnectError::Io(
                        "Unix abstract name is too long".to_owned(),
                    ));
                }
                for (target, source) in address.sun_path[1..].iter_mut().zip(&name.name) {
                    *target = *source as libc::c_char;
                }
                Ok((address, (offset + 1 + name.name.len()) as libc::socklen_t))
            }
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            {
                let _ = size_of::<libc::sockaddr_un>();
                Err(NativeConnectError::Io(
                    "Unix abstract sockets are unsupported".to_owned(),
                ))
            }
        }
    }
}

#[cfg(unix)]
fn raw_unix_socket(kind: libc::c_int) -> Result<std::os::fd::OwnedFd, NativeConnectError> {
    use std::os::fd::FromRawFd;
    let fd = unsafe { libc::socket(libc::AF_UNIX, kind | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(NativeConnectError::Io(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
    let flags = unsafe { libc::fcntl(std::os::fd::AsRawFd::as_raw_fd(&fd), libc::F_GETFL) };
    if flags < 0
        || unsafe {
            libc::fcntl(
                std::os::fd::AsRawFd::as_raw_fd(&fd),
                libc::F_SETFL,
                flags | libc::O_NONBLOCK,
            )
        } < 0
    {
        return Err(NativeConnectError::Io(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(fd)
}

#[cfg(unix)]
async fn finish_unix_connect(
    fd: std::os::fd::OwnedFd,
) -> Result<std::os::fd::OwnedFd, NativeConnectError> {
    use std::os::fd::AsRawFd;
    let async_fd = tokio::io::unix::AsyncFd::new(fd)
        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
    let mut ready = tokio::time::timeout(CONNECT_TIMEOUT, async_fd.writable())
        .await
        .map_err(|_| NativeConnectError::Refused("connect timed out".to_owned()))?
        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
    ready.clear_ready();
    drop(ready);
    let mut value = 0i32;
    let mut length = std::mem::size_of::<i32>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            async_fd.get_ref().as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&mut value as *mut i32).cast(),
            &mut length,
        )
    } != 0
    {
        return Err(NativeConnectError::Io(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    if value != 0 {
        return Err(NativeConnectError::Refused(
            std::io::Error::from_raw_os_error(value).to_string(),
        ));
    }
    Ok(async_fd.into_inner())
}

#[cfg(unix)]
async fn connect_unix_stream(
    name: &yas_net::UnixName,
) -> Result<tokio::net::UnixStream, NativeConnectError> {
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
    let mut fd = raw_unix_socket(libc::SOCK_STREAM)?;
    let (address, length) = unix_sockaddr(name)?;
    let connected = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length,
        )
    };
    if connected != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(NativeConnectError::Refused(error.to_string()));
        }
        fd = finish_unix_connect(fd).await?;
    }
    let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd.into_raw_fd()) };
    stream
        .set_nonblocking(true)
        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
    tokio::net::UnixStream::from_std(stream)
        .map_err(|error| NativeConnectError::Io(error.to_string()))
}

#[cfg(unix)]
async fn connect_unix_seqpacket(
    name: &yas_net::UnixName,
) -> Result<NativeUnixSeqpacket, NativeConnectError> {
    use std::os::fd::AsRawFd;
    let mut fd = raw_unix_socket(libc::SOCK_SEQPACKET)?;
    let (address, length) = unix_sockaddr(name)?;
    let connected = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length,
        )
    };
    if connected != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(NativeConnectError::Refused(error.to_string()));
        }
        fd = finish_unix_connect(fd).await?;
    }
    let socket = tokio::io::unix::AsyncFd::new(fd)
        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
    Ok(NativeUnixSeqpacket { socket })
}

#[cfg(unix)]
async fn connect_unix_datagram(
    peer: &yas_net::UnixName,
) -> Result<NativeUnixDatagram, NativeConnectError> {
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
    let fd = raw_unix_socket(libc::SOCK_DGRAM)?;
    let (local, filesystem_cleanup) = private_unix_datagram_name()?;
    let (local_address, local_length) = unix_sockaddr(&local)?;
    if unsafe {
        libc::bind(
            fd.as_raw_fd(),
            (&local_address as *const libc::sockaddr_un).cast(),
            local_length,
        )
    } != 0
    {
        return Err(NativeConnectError::Io(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let (peer_address, peer_length) = unix_sockaddr(peer)?;
    if unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&peer_address as *const libc::sockaddr_un).cast(),
            peer_length,
        )
    } != 0
    {
        return Err(NativeConnectError::Refused(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let socket = unsafe { std::os::unix::net::UnixDatagram::from_raw_fd(fd.into_raw_fd()) };
    socket
        .set_nonblocking(true)
        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
    let socket = tokio::net::UnixDatagram::from_std(socket)
        .map_err(|error| NativeConnectError::Io(error.to_string()))?;
    Ok(NativeUnixDatagram {
        socket: Arc::new(socket),
        local,
        peer: peer.clone(),
        _filesystem_cleanup: filesystem_cleanup,
    })
}

#[cfg(unix)]
fn private_unix_datagram_name()
-> Result<(yas_net::UnixName, Option<std::path::PathBuf>), NativeConnectError> {
    let mut random = [0u8; 12];
    getrandom::fill(&mut random).map_err(|error| NativeConnectError::Io(error.to_string()))?;
    let suffix = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        Ok((
            yas_net::UnixName {
                kind: yas_net::UnixNameKind::Abstract,
                name: format!("yas-net-{}-{suffix}", std::process::id()).into_bytes(),
            },
            None,
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        use std::os::unix::ffi::OsStrExt;
        let path =
            std::env::temp_dir().join(format!("yas-net-{}-{suffix}.sock", std::process::id()));
        Ok((
            yas_net::UnixName {
                kind: yas_net::UnixNameKind::Filesystem,
                name: path.as_os_str().as_bytes().to_vec(),
            },
            Some(path),
        ))
    }
}

#[cfg(unix)]
impl Drop for NativeUnixDatagram {
    fn drop(&mut self) {
        if let Some(path) = &self._filesystem_cleanup {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(windows)]
async fn connect_windows_pipe(
    name: &str,
    requested_mode: yas_net::PipeMode,
) -> Result<NativeWindowsPipe, NativeConnectError> {
    use std::os::windows::io::AsRawHandle;
    use tokio::net::windows::named_pipe::ClientOptions;
    use windows_sys::Win32::System::Pipes::{
        GetNamedPipeInfo, PIPE_READMODE_BYTE, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE,
        SetNamedPipeHandleState,
    };

    let path = if name.starts_with(r"\\") {
        name.to_owned()
    } else {
        format!(r"\\.\pipe\{name}")
    };
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    let pipe = loop {
        match ClientOptions::new().read(true).write(true).open(&path) {
            Ok(pipe) => break pipe,
            Err(error)
                if matches!(error.raw_os_error(), Some(2) | Some(53) | Some(231))
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) if matches!(error.raw_os_error(), Some(2) | Some(53)) => {
                return Err(NativeConnectError::NotFound(error.to_string()));
            }
            Err(error) => return Err(NativeConnectError::Refused(error.to_string())),
        }
    };
    let handle = pipe.as_raw_handle().cast();
    let mut flags = 0u32;
    let mut outbound = 0u32;
    let mut inbound = 0u32;
    let mut instances = 0u32;
    if unsafe {
        GetNamedPipeInfo(
            handle,
            &mut flags,
            &mut outbound,
            &mut inbound,
            &mut instances,
        )
    } == 0
    {
        return Err(NativeConnectError::Io(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let native_message = flags & PIPE_TYPE_MESSAGE != 0;
    let selected_message = match requested_mode {
        yas_net::PipeMode::Auto => native_message,
        yas_net::PipeMode::Byte => false,
        yas_net::PipeMode::Message if native_message => true,
        yas_net::PipeMode::Message => {
            return Err(NativeConnectError::Io(
                "byte-mode Windows pipe cannot preserve messages".to_owned(),
            ));
        }
    };
    let mode = if selected_message {
        PIPE_READMODE_MESSAGE
    } else {
        PIPE_READMODE_BYTE
    };
    if unsafe { SetNamedPipeHandleState(handle, &mode, std::ptr::null(), std::ptr::null()) } == 0 {
        return Err(NativeConnectError::Io(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    if selected_message {
        let configured = u64::from(inbound.max(outbound)).max(1);
        Ok(NativeWindowsPipe::Message {
            socket: Arc::new(NativeWindowsMessagePipe { pipe }),
            server_instance_limit: instances,
            max_message_bytes: configured.min(16 * 1024 * 1024),
        })
    } else {
        Ok(NativeWindowsPipe::Byte {
            stream: Box::new(pipe),
            server_instance_limit: instances,
        })
    }
}

async fn resolve_target(host: &str, port: u16) -> Result<Vec<SocketAddr>, NativeConnectError> {
    let target = format!("{host}:{port}");
    let resolved =
        match tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host(&target)).await {
            Ok(Ok(addrs)) => addrs.collect::<Vec<_>>(),
            Ok(Err(error)) => return Err(NativeConnectError::NotFound(error.to_string())),
            Err(_) => {
                return Err(NativeConnectError::NotFound(
                    "resolution timed out".to_owned(),
                ));
            }
        };
    if resolved.is_empty() {
        return Err(NativeConnectError::NotFound("no addresses".to_owned()));
    }
    Ok(resolved)
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// One `--allow-forward` pattern: `host[:ports]`.
#[derive(Clone, Debug)]
struct TargetRule {
    host: HostRule,
    /// Empty = every port.
    ports: Vec<(u16, u16)>,
}

#[derive(Clone, Debug)]
enum HostRule {
    Any,
    /// `*.suffix` — matches the suffix itself and anything under it.
    Suffix(String),
    Exact(String),
    Addr(std::net::IpAddr),
    /// An address and a prefix length.
    Cidr(std::net::IpAddr, u8),
}

impl TargetRule {
    fn parse(pattern: &str) -> Option<Self> {
        if pattern.is_empty() {
            return None;
        }
        // Split host from ports on the *last* colon, but only when what
        // follows looks like a port list — otherwise `::1` and `2001:db8::/32`
        // would lose their tails.
        let (host_part, port_part) = match pattern.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.starts_with(|c: char| c.is_ascii_digit()) => {
                // A bare IPv6 literal ends in digits too; brackets or a
                // remaining colon in the host tell them apart.
                if h.ends_with(']') || !h.contains(':') {
                    (h, Some(p))
                } else {
                    (pattern, None)
                }
            }
            _ => (pattern, None),
        };
        let host_part = host_part.trim_start_matches('[').trim_end_matches(']');
        let host = if host_part == "*" {
            HostRule::Any
        } else if let Some(suffix) = host_part.strip_prefix("*.") {
            if suffix.is_empty() {
                return None;
            }
            HostRule::Suffix(suffix.to_ascii_lowercase())
        } else if let Some((addr, bits)) = host_part.split_once('/') {
            let ip: std::net::IpAddr = addr.parse().ok()?;
            let bits: u8 = bits.parse().ok()?;
            let max = if ip.is_ipv4() { 32 } else { 128 };
            if bits > max {
                return None;
            }
            HostRule::Cidr(ip, bits)
        } else if let Ok(ip) = host_part.parse::<std::net::IpAddr>() {
            HostRule::Addr(ip)
        } else if host_part.is_empty() || host_part.contains(':') {
            // A leftover colon means the port list did not parse as one, and
            // no hostname contains a colon: `host:notaport` is a typo, not a
            // machine named "host:notaport".
            return None;
        } else {
            HostRule::Exact(host_part.to_ascii_lowercase())
        };

        let mut ports = Vec::new();
        if let Some(list) = port_part {
            for item in list.split(',') {
                let item = item.trim();
                let (lo, hi) = match item.split_once('-') {
                    Some((lo, hi)) => (lo.parse::<u16>().ok()?, hi.parse::<u16>().ok()?),
                    None => {
                        let p = item.parse::<u16>().ok()?;
                        (p, p)
                    }
                };
                if lo > hi {
                    return None;
                }
                ports.push((lo, hi));
            }
            if ports.is_empty() {
                return None;
            }
        }
        Some(Self { host, ports })
    }

    fn port_ok(&self, port: u16) -> bool {
        self.ports.is_empty() || self.ports.iter().any(|(lo, hi)| port >= *lo && port <= *hi)
    }

    fn matches_host(&self, host: &str, port: u16) -> bool {
        if !self.port_ok(port) {
            return false;
        }
        let lower = host.to_ascii_lowercase();
        match &self.host {
            HostRule::Any => true,
            HostRule::Suffix(suffix) => lower == *suffix || lower.ends_with(&format!(".{suffix}")),
            HostRule::Exact(name) => lower == *name,
            // An address rule matches a requested literal directly, and
            // otherwise waits for resolution.
            HostRule::Addr(ip) => lower.parse::<std::net::IpAddr>().is_ok_and(|h| h == *ip),
            HostRule::Cidr(net, bits) => lower
                .parse::<std::net::IpAddr>()
                .is_ok_and(|h| in_cidr(h, *net, *bits)),
        }
    }

    fn matches_addr(&self, host: &str, ip: std::net::IpAddr, port: u16) -> bool {
        if !self.port_ok(port) {
            return false;
        }
        match &self.host {
            HostRule::Any => true,
            HostRule::Addr(want) => ip == *want,
            HostRule::Cidr(net, bits) => in_cidr(ip, *net, *bits),
            // A name rule authorizes what that name resolves to; it already
            // matched the requested host or we would not be here.
            HostRule::Suffix(_) | HostRule::Exact(_) => self.matches_host(host, port),
        }
    }
}

/// Whether `ip` falls in `net/bits`. Mixed families never match: a v4 rule
/// does not silently cover a v4-mapped v6 address, which would be a way to
/// slip past an allowlist.
fn in_cidr(ip: std::net::IpAddr, net: std::net::IpAddr, bits: u8) -> bool {
    fn masked(bytes: &[u8], bits: u8) -> Vec<u8> {
        let mut out = bytes.to_vec();
        let full = (bits / 8) as usize;
        for (i, b) in out.iter_mut().enumerate() {
            if i < full {
                continue;
            }
            if i == full {
                let rest = bits % 8;
                *b &= if rest == 0 { 0 } else { !0u8 << (8 - rest) };
            } else {
                *b = 0;
            }
        }
        out
    }
    match (ip, net) {
        (std::net::IpAddr::V4(a), std::net::IpAddr::V4(b)) => {
            masked(&a.octets(), bits) == masked(&b.octets(), bits)
        }
        (std::net::IpAddr::V6(a), std::net::IpAddr::V6(b)) => {
            masked(&a.octets(), bits) == masked(&b.octets(), bits)
        }
        _ => false,
    }
}

// --------------------------------------------------------------------------- Datagram queue ---------------------------------------------------------------------------

/// A bounded datagram queue that drops the **oldest** when full: for nearly every UDP protocol the newest datagram is the useful one, and a stale queue is latency with no payoff (docs/design/net.md § UDP flows).
pub(crate) struct DgramQueue {
    inner: Mutex<VecDeque<QueuedDatagram>>,
    bytes: AtomicU64,
    dropped: AtomicU64,
    notify: Notify,
    closed: AtomicU64,
    drop_oldest: bool,
}

struct QueuedDatagram {
    payload: Vec<u8>,
    // Keep the decoded Event charged to the session-wide receive budget until
    // the native socket consumes it or queue pressure drops it.
    _credit: Option<super::yas::CreditLease>,
}

impl DgramQueue {
    #[cfg(test)]
    fn new() -> Self {
        Self::new_with_drop_oldest(true)
    }

    pub(crate) fn new_with_drop_oldest(drop_oldest: bool) -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
            bytes: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            notify: Notify::new(),
            closed: AtomicU64::new(0),
            drop_oldest,
        }
    }

    pub(crate) async fn push(&self, payload: Vec<u8>, credit: super::yas::CreditLease) {
        self.push_inner(payload, Some(credit)).await;
    }

    #[cfg(test)]
    async fn push_untracked(&self, payload: Vec<u8>) {
        self.push_inner(payload, None).await;
    }

    async fn push_inner(&self, payload: Vec<u8>, credit: Option<super::yas::CreditLease>) {
        let mut q = self.inner.lock().await;
        let mut bytes = self.bytes.load(Ordering::Relaxed) as usize;
        if !self.drop_oldest
            && (q.len() >= yas_net::MAX_DATAGRAM_QUEUE as usize
                || bytes + payload.len() > DGRAM_QUEUE_BYTES)
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        while q.len() >= yas_net::MAX_DATAGRAM_QUEUE as usize
            || bytes + payload.len() > DGRAM_QUEUE_BYTES
        {
            match q.pop_front() {
                Some(old) => {
                    bytes = bytes.saturating_sub(old.payload.len());
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
                None => break,
            }
        }
        bytes += payload.len();
        self.bytes.store(bytes as u64, Ordering::Relaxed);
        q.push_back(QueuedDatagram {
            payload,
            _credit: credit,
        });
        drop(q);
        self.notify.notify_one();
    }

    /// Next datagram, or `None` once the queue is closed and drained.
    pub(crate) async fn pop(&self) -> Option<Vec<u8>> {
        loop {
            {
                let mut q = self.inner.lock().await;
                if let Some(queued) = q.pop_front() {
                    self.bytes
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                            Some(v.saturating_sub(queued.payload.len() as u64))
                        })
                        .ok();
                    return Some(queued.payload);
                }
                if self.closed.load(Ordering::Relaxed) != 0 {
                    return None;
                }
            }
            self.notify.notified().await;
        }
    }

    pub(crate) fn close(&self) {
        self.closed.store(1, Ordering::Relaxed);
        self.notify.notify_one();
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn policy(patterns: &[&str]) -> Policy {
        unsafe {
            std::env::remove_var("YAS_ALLOW_FORWARD_INSECURE");
            std::env::remove_var("YAS_ALLOW_FORWARD");
        }
        let owned: Vec<String> = patterns.iter().map(|p| p.to_string()).collect();
        Policy::new(false, &owned)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_unix_seqpacket_preserves_message_boundaries_and_half_close() {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        use std::os::unix::ffi::OsStrExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("net-seqpacket.sock");
        let name = yas_net::UnixName {
            kind: yas_net::UnixNameKind::Filesystem,
            name: path.as_os_str().as_bytes().to_vec(),
        };
        let raw =
            unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
        assert!(raw >= 0, "socket: {}", std::io::Error::last_os_error());
        let listener = unsafe { OwnedFd::from_raw_fd(raw) };
        let (address, length) = unix_sockaddr(&name).unwrap();
        assert_eq!(
            unsafe {
                libc::bind(
                    listener.as_raw_fd(),
                    (&address as *const libc::sockaddr_un).cast(),
                    length,
                )
            },
            0,
            "bind: {}",
            std::io::Error::last_os_error()
        );
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);

        let peer = tokio::task::spawn_blocking(move || {
            let accepted = unsafe {
                libc::accept(
                    listener.as_raw_fd(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            assert!(accepted >= 0, "accept: {}", std::io::Error::last_os_error());
            let accepted = unsafe { OwnedFd::from_raw_fd(accepted) };
            let mut buffer = [0u8; 32];
            let read = unsafe {
                libc::recv(
                    accepted.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    0,
                )
            };
            assert_eq!(read, 4);
            assert_eq!(&buffer[..4], b"ping");
            assert_eq!(
                unsafe {
                    libc::send(
                        accepted.as_raw_fd(),
                        b"pong".as_ptr().cast(),
                        4,
                        libc::MSG_NOSIGNAL,
                    )
                },
                4
            );
            assert_eq!(
                unsafe {
                    libc::recv(
                        accepted.as_raw_fd(),
                        buffer.as_mut_ptr().cast(),
                        buffer.len(),
                        0,
                    )
                },
                0
            );
        });

        let socket = policy(&[])
            .connect_native_unix_seqpacket(&name)
            .await
            .unwrap();
        assert_eq!(socket.send(b"ping").await.unwrap(), 4);
        let mut response = [0u8; 8];
        assert_eq!(socket.recv(&mut response).await.unwrap(), 4);
        assert_eq!(&response[..4], b"pong");
        socket.shutdown_write().unwrap();
        tokio::time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn native_tls_negotiates_requested_alpn_and_carries_stream_bytes() {
        use rcgen::{CertificateParams, KeyPair};
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

        let key = KeyPair::generate().unwrap();
        let certificate = CertificateParams::new(vec!["localhost".to_owned()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut server_config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(certificate.der().to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )
            .unwrap();
        server_config.alpn_protocols = vec![b"yas-test".to_vec()];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            let mut request = [0; 4];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(&request, b"ping");
            stream.write_all(b"pong").await.unwrap();
        });

        let options = yas_net::TlsOptions {
            verification: yas_net::TlsVerification::Insecure,
            sni: "localhost".to_owned(),
            alpn: vec![b"yas-test".to_vec()],
            extensions: Default::default(),
        };
        let mut socket = Policy::new(true, &[])
            .connect_native_tcp("127.0.0.1", port, Some(&options))
            .await
            .unwrap();
        assert_eq!(socket.negotiated_alpn, b"yas-test");
        socket.stream.write_all(b"ping").await.unwrap();
        let mut response = [0; 4];
        socket.stream.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"pong");
        peer.await.unwrap();
    }

    /// The egress allowlist docs/design/net.md § Target policy specifies.
    /// Unrestricted by default; loopback always reachable; names, globs,
    /// addresses, CIDRs and port lists all bounded.
    #[test]
    fn allow_forward_patterns() {
        // No patterns: everything.
        let p = policy(&[]);
        assert!(p.permits_host("example.com", 443));
        assert!(p.permits_addr("example.com", "93.184.216.34:443".parse().unwrap()));

        // Loopback needs no rule, so a dev server always works.
        let p = policy(&["example.com"]);
        assert!(p.permits_host("localhost", 3000));
        assert!(p.permits_host("127.0.0.1", 3000));
        assert!(p.permits_host("::1", 3000));
        assert!(!p.permits_host("elsewhere.com", 3000));
        assert!(
            p.permits_host("EXAMPLE.COM", 3000),
            "names are ASCII-case-insensitive"
        );

        // Ports narrow a rule, singly and by range.
        let p = policy(&["build.internal:8080,9000-9010"]);
        assert!(p.permits_host("build.internal", 8080));
        assert!(p.permits_host("build.internal", 9005));
        assert!(!p.permits_host("build.internal", 8081));
        assert!(!p.permits_host("build.internal", 9011));

        // A suffix glob covers the suffix itself and anything under it,
        // and nothing that merely ends with the same letters.
        let p = policy(&["*.svc.internal"]);
        assert!(p.permits_host("svc.internal", 80));
        assert!(p.permits_host("api.svc.internal", 80));
        assert!(!p.permits_host("notsvc.internal", 80));
        assert!(!p.permits_host("svc.internal.evil.com", 80));

        // `*` is everything, which is the default said out loud.
        assert!(policy(&["*"]).permits_host("anything.example", 1));

        // An address rule matches a requested literal and a resolved one.
        let p = policy(&["10.1.2.3:80"]);
        assert!(p.permits_host("10.1.2.3", 80));
        assert!(!p.permits_host("10.1.2.4", 80));
        assert!(p.permits_addr("db.internal", "10.1.2.3:80".parse().unwrap()));
        assert!(!p.permits_addr("db.internal", "10.9.9.9:80".parse().unwrap()));

        // CIDR, including a non-byte-aligned prefix.
        let p = policy(&["10.0.0.0/9"]);
        assert!(p.permits_addr("h", "10.0.0.1:80".parse().unwrap()));
        assert!(p.permits_addr("h", "10.127.255.255:80".parse().unwrap()));
        assert!(!p.permits_addr("h", "10.128.0.1:80".parse().unwrap()));
        // A v4 rule must not cover a v4-mapped v6 address: that would be a
        // way around the allowlist.
        assert!(!p.permits_addr("h", "[::ffff:10.0.0.1]:80".parse().unwrap()));

        // IPv6 rules keep their colons; the port split must not eat them.
        let p = policy(&["[2001:db8::1]:443"]);
        assert!(p.permits_addr("h", "[2001:db8::1]:443".parse().unwrap()));
        assert!(!p.permits_addr("h", "[2001:db8::2]:443".parse().unwrap()));
        assert!(policy(&["2001:db8::/32"]).permits_addr("h", "[2001:db8:1::5]:9".parse().unwrap()));

        // A name rule authorizes whatever that name resolves to — the grant
        // an operator writing a glob is asking for (net.md is explicit that
        // a stricter reading needs a CIDR).
        let p = policy(&["*.svc.internal"]);
        assert!(p.permits_addr("api.svc.internal", "203.0.113.5:80".parse().unwrap()));
        assert!(!p.permits_addr("evil.com", "203.0.113.5:80".parse().unwrap()));

        // Every one of these is unparsable, so the allowlist is empty — and
        // an empty allowlist that was *asked for* must not read as
        // unrestricted. An operator who mistyped the flag gets loopback.
        let p = policy(&["*.", "host:", "host:notaport", "host:9-8", "10.0.0.0/33"]);
        assert!(!p.permits_host("host", 9), "a dropped rule permits nothing");
        assert!(
            !p.permits_host("example.com", 443),
            "all-unparsable patterns must not widen the policy"
        );
        assert!(p.permits_host("localhost", 9), "loopback still works");
    }

    #[tokio::test]
    async fn dgram_queue_drops_oldest_when_full() {
        let q = DgramQueue::new();
        for i in 0..yas_net::MAX_DATAGRAM_QUEUE + 5 {
            q.push_untracked(vec![i as u8]).await;
        }
        assert_eq!(q.dropped(), 5);
        // The survivors are the newest: the first popped is not payload 0.
        let first = q.pop().await.unwrap();
        assert_eq!(first, vec![5u8]);
    }

    #[tokio::test]
    async fn dgram_queue_drops_on_byte_cap() {
        let q = DgramQueue::new();
        let big = vec![0u8; DGRAM_QUEUE_BYTES / 2 + 1];
        q.push_untracked(big.clone()).await;
        q.push_untracked(big.clone()).await;
        assert_eq!(q.dropped(), 1);
    }

    #[tokio::test]
    async fn dgram_queue_pop_ends_after_close() {
        let q = DgramQueue::new();
        q.push_untracked(vec![1]).await;
        q.close();
        assert_eq!(q.pop().await, Some(vec![1]));
        assert_eq!(q.pop().await, None);
    }
}
