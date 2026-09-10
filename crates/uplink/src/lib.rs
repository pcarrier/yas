//! End-to-end TLS 1.3 over an untrusted uplink relay.
//!
//! RFC 7250 raw Ed25519 public keys are pinned out of band on both sides.
//! X25519 supplies ephemeral key agreement. No CA, DNS name, control-plane
//! response, resumption ticket, or bearer token can grant local authority.

mod datagram;
pub use datagram::{DATAGRAM_OVERHEAD, DatagramReceiver, DatagramSender, datagram_pair};

use std::io;
use std::sync::Arc;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{rand::SecureRandom, signature::KeyPair};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, Error, SignatureScheme};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
pub const DATAGRAM_EXPORTER_LABEL: &[u8] = b"EXPORTER-YAS-UPLINK-v1-datagrams";
pub type DatagramKeyMaterial = zeroize::Zeroizing<[u8; 64]>;
const ALPN: &[u8] = b"yas-uplink/1";
const READY: &[u8] = b"YAS-UPLINK\x01";
// RFC 8410 Ed25519 SubjectPublicKeyInfo, followed by the 32-byte public key.
const SPKI_PREFIX: &[u8] = &[
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicKey([u8; 32]);

impl std::str::FromStr for PublicKey {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = decode_key(value).map_err(|_| {
            "Ed25519 public key must be 43 characters of unpadded base64url".to_string()
        })?;
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl PublicKey {
    fn spki(self) -> Vec<u8> {
        [SPKI_PREFIX, &self.0].concat()
    }
}

/// Decode exactly one canonical 32-byte key without echoing invalid input.
fn decode_key(value: &str) -> Result<[u8; 32], ()> {
    if value.len() != 43 {
        return Err(());
    }
    let mut bytes = [0; 32];
    let count = URL_SAFE_NO_PAD
        .decode_slice(value, &mut bytes)
        .map_err(|_| ())?;
    if count != bytes.len() {
        return Err(());
    }
    Ok(bytes)
}

/// An Ed25519 signing identity. Encoded private seeds are full-control secrets
/// and must never be forwarded to the control plane or relay.
pub struct Identity {
    key: Arc<rustls::sign::CertifiedKey>,
    public: PublicKey,
}

impl Identity {
    /// Generate an encoded 32-byte private seed and its public key, without I/O.
    pub fn generate() -> Result<(zeroize::Zeroizing<String>, PublicKey), String> {
        let mut seed = zeroize::Zeroizing::new([0; 32]);
        ring::rand::SystemRandom::new()
            .fill(seed.as_mut())
            .map_err(|_| "cannot generate Ed25519 identity")?;
        let encoded = zeroize::Zeroizing::new(URL_SAFE_NO_PAD.encode(seed.as_ref()));
        let identity = Self::from_base64(&encoded)?;
        Ok((encoded, identity.public))
    }

    /// Import a raw Ed25519 seed in canonical unpadded base64url (43 characters).
    /// The public key is derived from the seed; no file paths are accepted.
    pub fn from_base64(encoded: &str) -> Result<Self, String> {
        let seed = zeroize::Zeroizing::new(
            decode_key(encoded)
                .map_err(|_| "Ed25519 private key must be 43 characters of unpadded base64url")?,
        );
        let pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(seed.as_ref())
            .map_err(|_| "invalid Ed25519 private seed")?;
        let public = PublicKey(
            pair.public_key()
                .as_ref()
                .try_into()
                .expect("Ed25519 key length"),
        );
        // RFC 8410 PKCS#8 v1 wrapper, solely for rustls's signing-key loader.
        // Configuration and keygen expose only the raw 32-byte seed.
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        der.extend_from_slice(seed.as_ref());
        let private = PrivatePkcs8KeyDer::from(der);
        let signing =
            rustls::crypto::ring::sign::any_supported_type(&PrivateKeyDer::Pkcs8(private))
                .map_err(|_| "cannot load Ed25519 signing key")?;
        Ok(Self {
            key: Arc::new(rustls::sign::CertifiedKey::new(
                vec![public.spki().into()],
                signing,
            )),
            public,
        })
    }

    pub fn public_key(&self) -> PublicKey {
        self.public
    }

    pub fn server_config(
        &self,
        allowed: Vec<PublicKey>,
    ) -> Result<Arc<rustls::ServerConfig>, String> {
        if allowed.is_empty() {
            return Err("at least one --allow-client Ed25519 public key is required".into());
        }
        let mut config = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| e.to_string())?
            .with_client_cert_verifier(Arc::new(PinnedKeys(allowed)))
            .with_cert_resolver(Arc::new(
                rustls::server::AlwaysResolvesServerRawPublicKeys::new(self.key.clone()),
            ));
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.send_tls13_tickets = 0;
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        Ok(Arc::new(config))
    }

    pub fn client_config(&self, server: PublicKey) -> Result<Arc<rustls::ClientConfig>, String> {
        let mut config = rustls::ClientConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedKeys(vec![server])))
            .with_client_cert_resolver(Arc::new(
                rustls::client::AlwaysResolvesClientRawPublicKeys::new(self.key.clone()),
            ));
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.enable_sni = false;
        config.resumption = rustls::client::Resumption::disabled();
        Ok(Arc::new(config))
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    let mut provider = rustls::crypto::ring::default_provider();
    provider.kx_groups = vec![rustls::crypto::ring::kx_group::X25519];
    provider.cipher_suites =
        vec![rustls::crypto::ring::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256];
    Arc::new(provider)
}

/// Only returns after client proof of possession has been verified. Callers
/// must not connect to the local YAS socket before this succeeds.
pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    config: Arc<rustls::ServerConfig>,
) -> io::Result<tokio_rustls::server::TlsStream<S>> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut stream = tokio_rustls::TlsAcceptor::from(config)
            .accept(stream)
            .await?;
        if stream.get_ref().1.alpn_protocol() != Some(ALPN) {
            return Err(io::Error::other("missing YAS uplink ALPN"));
        }
        // TLS 1.3 has no server Finished after client authentication. This
        // encrypted confirmation ensures connect() observes client rejection.
        stream.write_all(READY).await?;
        stream.flush().await?;
        Ok(stream)
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "uplink authentication timed out"))?
}

pub async fn connect<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    config: Arc<rustls::ClientConfig>,
) -> io::Result<tokio_rustls::client::TlsStream<S>> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        // A syntactic placeholder only: the verifier pins the raw public key,
        // and SNI is disabled. No hostname or system trust roots grant access.
        let name = ServerName::try_from("uplink.invalid").expect("fixed DNS name");
        let mut stream = tokio_rustls::TlsConnector::from(config)
            .connect(name, stream)
            .await?;
        if stream.get_ref().1.alpn_protocol() != Some(ALPN) {
            return Err(io::Error::other("missing YAS uplink ALPN"));
        }
        let mut ready = [0; READY.len()];
        stream.read_exact(&mut ready).await?;
        if ready != READY {
            return Err(io::Error::other(
                "invalid uplink authentication confirmation",
            ));
        }
        Ok(stream)
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "uplink authentication timed out"))?
}

#[derive(Debug)]
struct PinnedKeys(Vec<PublicKey>);

impl PinnedKeys {
    fn verify_key(
        &self,
        key: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
    ) -> Result<(), Error> {
        if intermediates.is_empty() && self.0.iter().any(|allowed| allowed.spki() == key.as_ref()) {
            Ok(())
        } else {
            Err(Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_signature(
        &self,
        message: &[u8],
        key: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        if dss.scheme != SignatureScheme::ED25519 {
            return Err(Error::General("uplink requires Ed25519 signatures".into()));
        }
        rustls::crypto::verify_tls13_signature_with_raw_key(
            message,
            &key.as_ref().into(),
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
}

impl ServerCertVerifier for PinnedKeys {
    fn verify_server_cert(
        &self,
        key: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.verify_key(key, intermediates)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("uplink requires TLS 1.3".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        key: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.verify_signature(message, key, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

impl ClientCertVerifier for PinnedKeys {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        key: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        self.verify_key(key, intermediates)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("uplink requires TLS 1.3".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        key: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.verify_signature(message, key, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
