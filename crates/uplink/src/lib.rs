//! End-to-end Noise IK over an untrusted uplink relay.
//!
//! Both endpoints pin raw X25519 public keys out of band. A fresh-session
//! confirmation precedes local IPC; no bearer token or allocation response
//! grants authority. See docs/uplink.md for the versioned wire protocol.

mod datagram;
mod stream;
pub use datagram::{DATAGRAM_OVERHEAD, DatagramReceiver, DatagramSender, datagram_pair};
pub use stream::NoiseStream;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::rand::SecureRandom;
use std::{io, sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
pub const NOISE_PROTOCOL: &str = "Noise_IK_25519_AESGCM_SHA256";
pub const PROLOGUE: &[u8] = b"YAS-UPLINK\x02";
/// Both reliable records and datagram epochs use at most 2^20 messages/key.
pub const REKEY_INTERVAL: u64 = 1 << 20;
pub type DatagramKeyMaterial = Zeroizing<[u8; 64]>;
const CONFIRM: &[u8] = b"YAS-UPLINK\x02";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicKey([u8; 32]);

fn valid_public_key(bytes: &[u8; 32]) -> bool {
    let mut prime = [255u8; 32];
    prime[0] = 237;
    prime[31] = 127;
    let secret = x25519_dalek::StaticSecret::from([42; 32]);
    bytes.iter().rev().cmp(prime.iter().rev()) == std::cmp::Ordering::Less
        && secret
            .diffie_hellman(&x25519_dalek::PublicKey::from(*bytes))
            .was_contributory()
}

impl std::str::FromStr for PublicKey {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = decode_key(value)
            .map_err(|_| "X25519 public key must be 43 characters of unpadded base64url")?;
        // Pin one canonical encoding per key, and reject non-contributory keys.
        if !valid_public_key(&bytes) {
            return Err("invalid X25519 public key".into());
        }
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

fn decode_key(value: &str) -> Result<[u8; 32], ()> {
    if value.len() != 43 {
        return Err(());
    }
    let mut bytes = [0; 32];
    if URL_SAFE_NO_PAD
        .decode_slice(value, &mut bytes)
        .map_err(|_| ())?
        != 32
    {
        return Err(());
    }
    Ok(bytes)
}

/// A full-control secret. It never leaves the endpoint for a relay or control plane.
#[derive(Clone)]
pub struct Identity {
    private: Zeroizing<[u8; 32]>,
    public: PublicKey,
}

impl Identity {
    pub fn generate() -> Result<(Zeroizing<String>, PublicKey), String> {
        let mut private = Zeroizing::new([0; 32]);
        ring::rand::SystemRandom::new()
            .fill(private.as_mut())
            .map_err(|_| "cannot generate X25519 identity")?;
        let encoded = Zeroizing::new(URL_SAFE_NO_PAD.encode(private.as_ref()));
        Ok((encoded, Self::from_private(private).public))
    }

    pub fn from_base64(encoded: &str) -> Result<Self, String> {
        let private = decode_key(encoded)
            .map_err(|_| "X25519 private key must be 43 characters of unpadded base64url")?;
        Ok(Self::from_private(Zeroizing::new(private)))
    }

    fn from_private(private: Zeroizing<[u8; 32]>) -> Self {
        let secret = x25519_dalek::StaticSecret::from(*private);
        let public = PublicKey(x25519_dalek::PublicKey::from(&secret).to_bytes());
        Self { private, public }
    }

    pub fn public_key(&self) -> PublicKey {
        self.public
    }

    pub fn server_config(&self, allowed: Vec<PublicKey>) -> Result<Arc<ServerConfig>, String> {
        if allowed.is_empty() {
            return Err("at least one --allow-client X25519 public key is required".into());
        }
        Ok(Arc::new(ServerConfig {
            identity: self.clone(),
            allowed,
        }))
    }

    pub fn client_config(&self, server: PublicKey) -> Result<Arc<ClientConfig>, String> {
        Ok(Arc::new(ClientConfig {
            identity: self.clone(),
            server,
        }))
    }
}

pub struct ServerConfig {
    identity: Identity,
    allowed: Vec<PublicKey>,
}
pub struct ClientConfig {
    identity: Identity,
    server: PublicKey,
}

fn builder(identity: &Identity) -> snow::Builder<'_> {
    snow::Builder::new(NOISE_PROTOCOL.parse().expect("fixed Noise protocol"))
        .prologue(PROLOGUE)
        .expect("fixed prologue")
        .local_private_key(identity.private.as_ref())
        .expect("32-byte key")
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid YAS uplink Noise record",
    )
}
fn crypto_error(_: snow::Error) -> io::Error {
    invalid()
}

async fn write_handshake<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8]) -> io::Result<()> {
    stream
        .write_all(&(bytes.len() as u16).to_be_bytes())
        .await?;
    stream.write_all(bytes).await?;
    stream.flush().await
}

async fn read_handshake<S: AsyncRead + Unpin>(
    stream: &mut S,
    expected: usize,
) -> io::Result<Vec<u8>> {
    if stream.read_u16().await? as usize != expected {
        return Err(invalid());
    }
    let mut bytes = vec![0; expected];
    stream.read_exact(&mut bytes).await?;
    // Both IK messages start with the peer's ephemeral public key. Snow's
    // default Curve25519 resolver does not reject all-zero DH results itself.
    if !valid_public_key(bytes[..32].try_into().map_err(|_| invalid())?) {
        return Err(invalid());
    }
    Ok(bytes)
}

/// Completes mutual authentication and a fresh-session proof before returning.
/// In particular, replaying IK's first message cannot open a local YAS socket.
pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    config: Arc<ServerConfig>,
) -> io::Result<NoiseStream<S>> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut handshake = builder(&config.identity)
            .build_responder()
            .map_err(crypto_error)?;
        let first = read_handshake(&mut stream, 96).await?;
        let mut buffer = Zeroizing::new([0; 96]);
        if handshake
            .read_message(&first, buffer.as_mut())
            .map_err(crypto_error)?
            != 0
            || !config
                .allowed
                .iter()
                .any(|key| handshake.get_remote_static() == Some(key.0.as_slice()))
        {
            return Err(invalid());
        }
        let len = handshake
            .write_message(&[], buffer.as_mut())
            .map_err(crypto_error)?;
        write_handshake(&mut stream, &buffer[..len]).await?;
        let state = handshake.into_transport_mode().map_err(crypto_error)?;
        let mut stream = NoiseStream::new(stream, state);
        let mut confirm = [0; CONFIRM.len()];
        stream.read_exact(&mut confirm).await?;
        if confirm != CONFIRM {
            return Err(invalid());
        }
        // Independent datagram root keys travel inside the forward-secret,
        // authenticated channel. The public handshake hash is never a key.
        ring::rand::SystemRandom::new()
            .fill(stream.datagram_keys.as_mut())
            .map_err(|_| invalid())?;
        let mut ready = Zeroizing::new(Vec::with_capacity(CONFIRM.len() + 64));
        ready.extend_from_slice(CONFIRM);
        ready.extend_from_slice(stream.datagram_keys.as_ref());
        stream.write_all(&ready).await?;
        stream.flush().await?;
        Ok(stream)
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "uplink authentication timed out"))?
}

pub async fn connect<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    config: Arc<ClientConfig>,
) -> io::Result<NoiseStream<S>> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut handshake = builder(&config.identity)
            .remote_public_key(&config.server.0)
            .map_err(crypto_error)?
            .build_initiator()
            .map_err(crypto_error)?;
        let mut buffer = Zeroizing::new([0; 96]);
        let len = handshake
            .write_message(&[], buffer.as_mut())
            .map_err(crypto_error)?;
        write_handshake(&mut stream, &buffer[..len]).await?;
        let response = read_handshake(&mut stream, 48).await?;
        if handshake
            .read_message(&response, buffer.as_mut())
            .map_err(crypto_error)?
            != 0
        {
            return Err(invalid());
        }
        let state = handshake.into_transport_mode().map_err(crypto_error)?;
        let mut stream = NoiseStream::new(stream, state);
        stream.write_all(CONFIRM).await?;
        stream.flush().await?;
        let mut ready = Zeroizing::new([0; CONFIRM.len() + 64]);
        stream.read_exact(ready.as_mut()).await?;
        if &ready[..CONFIRM.len()] != CONFIRM {
            return Err(invalid());
        }
        stream
            .datagram_keys
            .copy_from_slice(&ready[CONFIRM.len()..]);
        Ok(stream)
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "uplink authentication timed out"))?
}

#[cfg(test)]
mod tests;
