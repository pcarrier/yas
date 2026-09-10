//! Independent AES-GCM datagrams. Root keys are exchanged inside Noise.
//! Route token + explicit counter are authenticated; loss cannot desynchronize
//! key rotation because each epoch is a function of the packet counter.
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    hkdf,
};
use zeroize::Zeroizing;

pub const DATAGRAM_OVERHEAD: usize = 8 + 16;
const KEY_LABEL: &[u8] = b"YAS-UPLINK-v2-datagram";
const MAX_PLAINTEXT: usize = 65_536 - DATAGRAM_OVERHEAD;

pub fn datagram_pair(
    material: crate::DatagramKeyMaterial,
    token: [u8; 16],
    client: bool,
) -> (DatagramSender, DatagramReceiver) {
    let c2s = KeyState::new(material[..32].try_into().expect("key length"), token, 0);
    let s2c = KeyState::new(material[32..].try_into().expect("key length"), token, 1);
    let (send, receive) = if client { (c2s, s2c) } else { (s2c, c2s) };
    (
        DatagramSender {
            keys: send,
            sequence: Some(0),
        },
        DatagramReceiver {
            keys: receive,
            highest: None,
            window: 0,
        },
    )
}

struct KeyState {
    root: Zeroizing<[u8; 32]>,
    token: [u8; 16],
    direction: u8,
    cached: Option<(u64, LessSafeKey)>,
}
impl KeyState {
    fn new(root: [u8; 32], token: [u8; 16], direction: u8) -> Self {
        Self {
            root: Zeroizing::new(root),
            token,
            direction,
            cached: None,
        }
    }
    fn derive(&self, epoch: u64) -> LessSafeKey {
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, &self.token).extract(self.root.as_ref());
        let mut key = Zeroizing::new([0u8; 32]);
        prk.expand(
            &[KEY_LABEL, &[self.direction], &epoch.to_be_bytes()],
            hkdf::HKDF_SHA256,
        )
        .expect("fixed HKDF length")
        .fill(key.as_mut())
        .expect("32-byte key");
        LessSafeKey::new(UnboundKey::new(&aead::AES_256_GCM, key.as_ref()).expect("key length"))
    }
}

pub struct DatagramSender {
    keys: KeyState,
    sequence: Option<u64>,
}
pub struct DatagramReceiver {
    keys: KeyState,
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
    pub fn seal(&mut self, plaintext: &[u8]) -> Option<Vec<u8>> {
        if plaintext.len() > MAX_PLAINTEXT {
            return None;
        }
        let sequence = self.sequence?;
        self.sequence = sequence.checked_add(1);
        let epoch = sequence / crate::REKEY_INTERVAL;
        if self
            .keys
            .cached
            .as_ref()
            .is_none_or(|(cached, _)| *cached != epoch)
        {
            self.keys.cached = Some((epoch, self.keys.derive(epoch)));
        }
        let key = &self.keys.cached.as_ref()?.1;
        let mut ciphertext = plaintext.to_vec();
        key.seal_in_place_append_tag(
            nonce(sequence),
            aad(self.keys.token, sequence),
            &mut ciphertext,
        )
        .ok()?;
        let mut packet = Vec::with_capacity(8 + ciphertext.len());
        packet.extend_from_slice(&sequence.to_be_bytes());
        packet.extend_from_slice(&ciphertext);
        Some(packet)
    }
}

impl DatagramReceiver {
    /// Authenticate before changing replay or epoch state. A forged high counter
    /// cannot flush the window. Lost, duplicate, or late packets never block audio.
    pub fn open(&mut self, packet: &[u8]) -> Option<Vec<u8>> {
        if !(DATAGRAM_OVERHEAD..=65_536).contains(&packet.len()) {
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
        let epoch = sequence / crate::REKEY_INTERVAL;
        let candidate = if self
            .keys
            .cached
            .as_ref()
            .is_none_or(|(cached, _)| *cached != epoch)
        {
            Some(self.keys.derive(epoch))
        } else {
            None
        };
        let key = candidate
            .as_ref()
            .or_else(|| self.keys.cached.as_ref().map(|(_, key)| key))?;
        let mut ciphertext = packet[8..].to_vec();
        let plaintext = key
            .open_in_place(
                nonce(sequence),
                aad(self.keys.token, sequence),
                &mut ciphertext,
            )
            .ok()?
            .to_vec();
        if let Some(key) = candidate {
            self.keys.cached = Some((epoch, key));
        }
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
        // Distinct directional root keys also protect against reflection.
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

#[cfg(test)]
mod interoperability {
    use super::*;
    #[test]
    fn aes_hkdf_vectors_and_reordering_across_epochs() {
        let vector: serde_json::Value =
            serde_json::from_str(include_str!("../test-vectors/datagrams.json")).unwrap();
        let bytes = |key: &str| crate::tests::hex(vector[key].as_str().unwrap());
        let material: [u8; 64] = bytes("material").try_into().unwrap();
        let token: [u8; 16] = bytes("token").try_into().unwrap();
        for packet in vector["packets"].as_array().unwrap() {
            let client = packet["direction"].as_u64().unwrap() == 0;
            let (mut sender, _) = datagram_pair(material.into(), token, client);
            sender.sequence = Some(packet["counter"].as_str().unwrap().parse().unwrap());
            let expected = crate::tests::hex(packet["ciphertext"].as_str().unwrap());
            assert_eq!(sender.seal(&bytes("plaintext")).unwrap(), expected);
            let (_, mut receiver) = datagram_pair(material.into(), token, !client);
            assert_eq!(receiver.open(&expected).unwrap(), bytes("plaintext"));
        }
        let (mut sender, _) = datagram_pair(material.into(), token, true);
        let (_, mut receiver) = datagram_pair(material.into(), token, false);
        sender.sequence = Some(crate::REKEY_INTERVAL - 1);
        let before = sender.seal(b"before").unwrap();
        sender.seal(b"lost").unwrap();
        let after = sender.seal(b"after").unwrap();
        let mut forged = after.clone();
        forged[0] = 127;
        assert!(receiver.open(&forged).is_none());
        assert_eq!(receiver.open(&after).unwrap(), b"after");
        assert_eq!(receiver.open(&before).unwrap(), b"before");
        assert!(receiver.open(&before).is_none());
    }
}
