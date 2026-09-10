//! Independent datagrams use TLS exporter keys, not relay transport keys.
//! Packet: outer route token (16 bytes, added by the carrier), sequence (u64
//! big-endian), ChaCha20-Poly1305 ciphertext/tag. The token and sequence are AAD.

use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};

/// Sequence and authentication tag, excluding the outer routing token.
pub const DATAGRAM_OVERHEAD: usize = 8 + 16;

/// `material` is 64 TLS exporter bytes with DATAGRAM_EXPORTER_LABEL and the
/// empty context. The first half is client->server, the second is
/// server->client. A fresh TLS handshake creates fresh keys for every stream.
pub fn datagram_pair(
    material: crate::DatagramKeyMaterial,
    token: [u8; 16],
    client: bool,
) -> (DatagramSender, DatagramReceiver) {
    let c2s = LessSafeKey::new(
        UnboundKey::new(&aead::CHACHA20_POLY1305, &material[..32]).expect("key length"),
    );
    let s2c = LessSafeKey::new(
        UnboundKey::new(&aead::CHACHA20_POLY1305, &material[32..]).expect("key length"),
    );
    let (send, receive) = if client { (c2s, s2c) } else { (s2c, c2s) };
    (
        DatagramSender {
            key: send,
            token,
            sequence: Some(0),
        },
        DatagramReceiver {
            key: receive,
            token,
            highest: None,
            window: 0,
        },
    )
}

pub struct DatagramSender {
    key: LessSafeKey,
    token: [u8; 16],
    sequence: Option<u64>,
}

pub struct DatagramReceiver {
    key: LessSafeKey,
    token: [u8; 16],
    highest: Option<u64>,
    window: u128,
}

fn nonce(sequence: u64) -> Nonce {
    let mut nonce = [0; 12];
    nonce[4..].copy_from_slice(&sequence.to_be_bytes());
    Nonce::assume_unique_for_key(nonce)
}

fn aad(token: [u8; 16], sequence: u64) -> Aad<[u8; 24]> {
    let mut aad = [0; 24];
    aad[..16].copy_from_slice(&token);
    aad[16..].copy_from_slice(&sequence.to_be_bytes());
    Aad::from(aad)
}

impl DatagramSender {
    /// Returns None on counter exhaustion; never reuses a nonce.
    pub fn seal(&mut self, plaintext: &[u8]) -> Option<Vec<u8>> {
        let sequence = self.sequence?;
        self.sequence = sequence.checked_add(1);
        let mut ciphertext = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(nonce(sequence), aad(self.token, sequence), &mut ciphertext)
            .ok()?;
        let mut packet = Vec::with_capacity(8 + ciphertext.len());
        packet.extend_from_slice(&sequence.to_be_bytes());
        packet.extend_from_slice(&ciphertext);
        Some(packet)
    }
}

impl DatagramReceiver {
    /// Authenticate before changing replay state. Allows reordering within a
    /// 128-packet window; forged, duplicate, and older packets are discarded.
    pub fn open(&mut self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() < DATAGRAM_OVERHEAD {
            return None;
        }
        let sequence = u64::from_be_bytes(packet[..8].try_into().ok()?);
        if let Some(highest) = self.highest
            && sequence <= highest
        {
            let behind = highest - sequence;
            if behind >= 128 || self.window & (1 << behind) != 0 {
                return None;
            }
        }
        let mut ciphertext = packet[8..].to_vec();
        let plaintext = self
            .key
            .open_in_place(nonce(sequence), aad(self.token, sequence), &mut ciphertext)
            .ok()?
            .to_vec();
        match self.highest {
            Some(highest) if sequence <= highest => self.window |= 1 << (highest - sequence),
            Some(highest) => {
                self.window = self
                    .window
                    .checked_shl((sequence - highest).min(128) as u32)
                    .unwrap_or(0)
                    | 1;
                self.highest = Some(sequence);
            }
            None => {
                self.highest = Some(sequence);
                self.window = 1;
            }
        }
        Some(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_reordering_replay_and_direction_separation() {
        let (mut client, mut client_rx) = datagram_pair([7; 64].into(), [3; 16], true);
        let (mut server, mut server_rx) = datagram_pair([7; 64].into(), [3; 16], false);
        // Use distinct exporter halves, as TLS does, to test reflection.
        let mut material = [7; 64];
        material[32..].fill(8);
        let (mut distinct_client, _) = datagram_pair(material.into(), [3; 16], true);
        let (_, mut reflected_rx) = datagram_pair(material.into(), [3; 16], true);
        assert!(
            reflected_rx
                .open(&distinct_client.seal(b"reflected").unwrap())
                .is_none()
        );

        let zero = client.seal(b"secret zero").unwrap();
        let one = client.seal(b"secret one").unwrap();
        assert!(!zero.windows(6).any(|w| w == b"secret"));
        let mut forged = one.clone();
        forged[0] ^= 0x80;
        assert!(server_rx.open(&forged).is_none());
        forged = one.clone();
        *forged.last_mut().unwrap() ^= 1;
        assert!(server_rx.open(&forged).is_none());
        assert_eq!(server_rx.open(&one).unwrap(), b"secret one");
        assert_eq!(server_rx.open(&zero).unwrap(), b"secret zero");
        assert!(server_rx.open(&zero).is_none());
        assert_eq!(
            client_rx.open(&server.seal(b"reply").unwrap()).unwrap(),
            b"reply"
        );

        let (_, mut wrong_route) = datagram_pair([7; 64].into(), [4; 16], false);
        let (_, mut wrong_session) = datagram_pair([8; 64].into(), [3; 16], false);
        assert!(wrong_route.open(&zero).is_none());
        assert!(wrong_session.open(&zero).is_none());
        let old = client.seal(b"old").unwrap();
        for _ in 0..128 {
            client.seal(b"lost").unwrap();
        }
        let recent = client.seal(b"recent").unwrap();
        assert_eq!(server_rx.open(&recent).unwrap(), b"recent");
        assert!(server_rx.open(&old).is_none());
        client.sequence = Some(u64::MAX);
        assert!(client.seal(b"last").is_some());
        assert!(client.seal(b"exhausted").is_none());
    }
}
