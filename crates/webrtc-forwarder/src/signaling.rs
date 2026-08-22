use base64::Engine;
use crypto_box::SalsaBox;
use crypto_box::aead::{Aead, AeadCore, OsRng};
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::BoxKeys;

#[derive(Debug)]
pub enum Event {
    Registered {
        session_id: String,
    },
    PeerJoined {
        session_id: String,
    },
    PeerLeft {
        session_id: String,
    },
    Signal {
        from: String,
        data: serde_json::Value,
    },
    Error {
        message: String,
    },
}

#[derive(Deserialize)]
struct ServerMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    from: Option<String>,
    data: Option<serde_json::Value>,
    message: Option<String>,
}

#[derive(Serialize)]
struct ClientMessage {
    signed: String,
    target: String,
}

pub fn sign_payload(key: &SigningKey, payload: &[u8]) -> String {
    let sig = key.sign(payload);
    let mut envelope = Vec::with_capacity(64 + payload.len());
    envelope.extend_from_slice(&sig.to_bytes());
    envelope.extend_from_slice(payload);
    base64::engine::general_purpose::STANDARD.encode(&envelope)
}

/// Build a signed message with the `data` encrypted inside a NaCl crypto_box.
/// The signed inner payload is `{"box":"<base64(nonce||ciphertext)>"}` so the
/// hub can still parse it as JSON while the actual SDP/ICE data is opaque.
pub fn build_sealed_message(
    signing_key: &SigningKey,
    target: &str,
    data: &serde_json::Value,
    box_keys: &BoxKeys,
) -> String {
    let plaintext = serde_json::to_vec(data).unwrap();
    let salsa = SalsaBox::new(&box_keys.their_public, &box_keys.our_secret);
    let nonce = SalsaBox::generate_nonce(&mut OsRng);
    let ciphertext = salsa.encrypt(&nonce, plaintext.as_ref()).expect("encrypt");
    let mut sealed = Vec::with_capacity(24 + ciphertext.len());
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    let sealed_b64 = base64::engine::general_purpose::STANDARD.encode(&sealed);
    // The inner payload the hub sees after signature verification:
    let inner = serde_json::json!({ "box": sealed_b64 });
    let inner_bytes = serde_json::to_vec(&inner).unwrap();
    let signed = sign_payload(signing_key, &inner_bytes);
    serde_json::to_string(&ClientMessage {
        signed,
        target: target.to_owned(),
    })
    .unwrap()
}

/// Try to open a NaCl crypto_box sealed payload. Returns the decrypted JSON
/// value, or `None` if the data doesn't contain a `"box"` field or decryption
/// fails.
pub fn open_sealed_data(data: &serde_json::Value, box_keys: &BoxKeys) -> Option<serde_json::Value> {
    let sealed_b64 = data.get("box")?.as_str()?;
    let sealed = base64::engine::general_purpose::STANDARD
        .decode(sealed_b64)
        .ok()?;
    if sealed.len() < 24 {
        return None;
    }
    let nonce: &crypto_box::Nonce = (&sealed[..24]).into();
    let ciphertext = &sealed[24..];
    let salsa = SalsaBox::new(&box_keys.their_public, &box_keys.our_secret);
    let plaintext = salsa.decrypt(nonce, ciphertext).ok()?;
    serde_json::from_slice(&plaintext).ok()
}

pub async fn connect(
    url: String,
    key: SigningKey,
    box_keys: Option<BoxKeys>,
    event_tx: mpsc::Sender<Event>,
    mut outgoing_rx: mpsc::Receiver<String>,
) {
    loop {
        match try_connect(&url, &key, box_keys.as_ref(), &event_tx, &mut outgoing_rx).await {
            Ok(()) => {
                verbose!("signaling connection closed, reconnecting...");
            }
            Err(e) => {
                verbose!("signaling connection error: {e}, reconnecting...");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn try_connect(
    url: &str,
    _key: &SigningKey,
    box_keys: Option<&BoxKeys>,
    tx: &mpsc::Sender<Event>,
    outgoing_rx: &mut mpsc::Receiver<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    const MAX_SIGNAL_TEXT_BYTES: usize = 64 * 1024;
    let ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_SIGNAL_TEXT_BYTES))
        .max_frame_size(Some(MAX_SIGNAL_TEXT_BYTES));
    let (ws, _) = tokio_tungstenite::connect_async_with_config(url, Some(ws_config), false).await?;
    let (mut write, mut read) = ws.split();

    loop {
        tokio::select! {
            msg = read.next() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => return Err(e.into()),
                    None => break,
                };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Close(_) => break,
                    _ => continue,
                };
                if text.len() > MAX_SIGNAL_TEXT_BYTES {
                    return Err("oversized signaling message".into());
                }

                let parsed: ServerMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let event = match parsed.msg_type.as_str() {
                    "registered" => Event::Registered {
                        session_id: parsed.session_id.unwrap_or_default(),
                    },
                    "peer_joined" => Event::PeerJoined {
                        session_id: parsed.session_id.unwrap_or_default(),
                    },
                    "peer_left" => Event::PeerLeft {
                        session_id: parsed.session_id.unwrap_or_default(),
                    },
                    "signal" => {
                        let raw = parsed.data.unwrap_or(serde_json::Value::Null);
                        // Consumers require authenticated encryption. The
                        // producer passes `None` here because it must retain
                        // the sealed envelope until it determines whether the
                        // RW or RO consumer key opens it.
                        let data = match box_keys {
                            Some(keys) => match open_sealed_data(&raw, keys) {
                                Some(data) => data,
                                None => continue,
                            },
                            None => raw,
                        };
                        Event::Signal {
                            from: parsed.from.unwrap_or_default(),
                            data,
                        }
                    },
                    "error" => Event::Error {
                        message: parsed.message.unwrap_or_default(),
                    },
                    _ => continue,
                };

                match tx.try_send(event) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Closed(_)) => break,
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        return Err("signaling event budget exceeded".into());
                    }
                }
            }
            msg = outgoing_rx.recv() => {
                match msg {
                    Some(text) => {
                        write.send(Message::Text(text.into())).await?;
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Access, ConsumerKeys, ProducerKeys};

    #[test]
    fn signaling_payloads_are_sealed_and_round_trip() {
        let producer = ProducerKeys::derive("sealed-signaling-test");
        let consumer = ConsumerKeys::derive_rw("sealed-signaling-test");
        let payload = serde_json::json!({
            "type": "offer",
            "sdp": "peer-private-session-description",
        });

        let wire = build_sealed_message(
            &consumer.signing,
            "producer-session",
            &payload,
            &consumer.box_keys(),
        );
        let envelope: serde_json::Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(envelope["target"], "producer-session");

        let signed = base64::engine::general_purpose::STANDARD
            .decode(envelope["signed"].as_str().unwrap())
            .unwrap();
        assert!(signed.len() > 64);
        let sealed: serde_json::Value = serde_json::from_slice(&signed[64..]).unwrap();
        assert!(sealed.get("box").is_some());
        assert_eq!(sealed.get("type"), None);
        assert_eq!(sealed.get("sdp"), None);

        let (opened, access) = producer.open_sealed(&sealed).unwrap();
        assert_eq!(access, Access::ReadWrite);
        assert_eq!(opened, payload);
    }

    #[test]
    fn unsealed_or_tampered_signaling_payloads_are_rejected() {
        let producer = ProducerKeys::derive("sealed-signaling-test");
        let consumer = ConsumerKeys::derive_rw("sealed-signaling-test");

        assert!(
            producer
                .open_sealed(&serde_json::json!({ "type": "offer", "sdp": "plaintext" }))
                .is_none()
        );

        let payload = serde_json::json!({ "type": "candidate", "candidate": "secret" });
        let wire = build_sealed_message(
            &consumer.signing,
            "producer-session",
            &payload,
            &consumer.box_keys(),
        );
        let envelope: serde_json::Value = serde_json::from_str(&wire).unwrap();
        let signed = base64::engine::general_purpose::STANDARD
            .decode(envelope["signed"].as_str().unwrap())
            .unwrap();
        let mut sealed: serde_json::Value = serde_json::from_slice(&signed[64..]).unwrap();
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(sealed["box"].as_str().unwrap())
            .unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        sealed["box"] =
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(bytes));

        assert!(producer.open_sealed(&sealed).is_none());
    }
}
