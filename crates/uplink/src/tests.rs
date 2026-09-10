use super::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::DuplexStream;

fn identity() -> Identity {
    let (private, _) = Identity::generate().unwrap();
    Identity::from_base64(&private).unwrap()
}

#[test]
fn base64_keys_round_trip_and_reject_noncanonical_or_wrong_lengths() {
    let (private, public) = Identity::generate().unwrap();
    assert_eq!(private.len(), 43);
    assert_eq!(public.to_string().len(), 43);
    assert_eq!(
        Identity::from_base64(&private).unwrap().public_key(),
        public
    );
    assert_eq!(public.to_string().parse::<PublicKey>().unwrap(), public);
    for bad in [
        "".to_owned(),
        "short".to_owned(),
        "a".repeat(42),
        "a".repeat(44),
        "é".repeat(32),
        format!("{}=", private.as_str()),
        "A".repeat(64),
        "/absolute/path/key".to_owned(),
        format!("{}+", "A".repeat(42)),
        format!("{}/", "A".repeat(42)),
        format!("{}B", "A".repeat(42)),
    ] {
        assert!(bad.parse::<PublicKey>().is_err());
        assert!(Identity::from_base64(&bad).is_err());
    }
}

#[test]
fn raw_seed_derives_the_expected_public_key() {
    // Disposable X25519 example; the old Ed25519 public pin must change.
    let private = "3iAcr2-GUIpQfQsOaabe-eD6uK8O53exaB9pKnVfotI";
    let public = "XRb2vVZJepyosoYqoXX24-lMYpkj9SekAq8ViT7RC1Q";
    let identity = Identity::from_base64(private).unwrap();
    assert_eq!(identity.public_key().to_string(), public);
    let old_pkcs8 = "MC4CAQAwBQYDK2VwBCIEIN4gHK9vhlCKUH0LDmmm3vng+rivDud3sWgfaSp1X6LS";
    assert!(Identity::from_base64(old_pkcs8).is_err());
}

#[tokio::test]
async fn mutual_authentication_and_datagram_key_agreement() {
    let server = identity();
    let client = identity();
    let (a, b) = tokio::io::duplex(64 * 1024);
    let (client, server) = tokio::join!(
        connect(a, client.client_config(server.public_key()).unwrap()),
        accept(b, server.server_config(vec![client.public_key()]).unwrap()),
    );
    let (mut client, mut server) = (client.unwrap(), server.unwrap());
    let c = client.datagram_key_material();
    let s = server.datagram_key_material();
    assert_eq!(c, s);
    let (mut send, _) = datagram_pair(c, [5; 16], true);
    let (_, mut receive) = datagram_pair(s, [5; 16], false);
    assert_eq!(
        receive.open(&send.seal(b"datagram").unwrap()).unwrap(),
        b"datagram"
    );
    client.write_all(b"YAS protocol bytes").await.unwrap();
    client.flush().await.unwrap();
    let mut bytes = [0; 18];
    server.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"YAS protocol bytes");
    client.shutdown().await.unwrap();
    assert_eq!(server.read(&mut bytes).await.unwrap(), 0);
    server.write_all(b"reply after FIN").await.unwrap();
    server.shutdown().await.unwrap();
    let mut reply = Vec::new();
    client.read_to_end(&mut reply).await.unwrap();
    assert_eq!(reply, b"reply after FIN");
}

#[tokio::test]
async fn unauthorized_client_wrong_server_and_forged_identity_fail() {
    let server = identity();
    let client = identity();
    let attacker = identity();
    assert!(server.server_config(vec![]).is_err());
    let server_config = server.server_config(vec![client.public_key()]).unwrap();
    for (offered, pinned) in [
        (&attacker, server.public_key()),
        (&client, attacker.public_key()),
    ] {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (c, s) = tokio::join!(
            connect(a, offered.client_config(pinned).unwrap()),
            accept(b, server_config.clone())
        );
        assert!(c.is_err());
        assert!(s.is_err());
    }
}

#[tokio::test(start_paused = true)]
async fn plaintext_and_stalled_handshakes_fail_closed() {
    let server = identity();
    let config = server.server_config(vec![identity().public_key()]).unwrap();
    let (mut a, b) = tokio::io::duplex(4096);
    a.write_all(b"YAS\x01\x00\x00\x00\x00").await.unwrap();
    a.shutdown().await.unwrap();
    assert!(accept(b, config.clone()).await.is_err());
    let (_a, b) = tokio::io::duplex(4096);
    assert_eq!(
        accept(b, config).await.unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
}

#[tokio::test]
async fn protocol_version_and_context_are_authenticated() {
    let server = identity();
    let client = identity();
    let config = server.server_config(vec![client.public_key()]).unwrap();
    let mut wrong_context = snow::Builder::new(NOISE_PROTOCOL.parse().unwrap())
        .prologue(b"YAS-UPLINK\x01")
        .unwrap()
        .local_private_key(client.private.as_ref())
        .unwrap()
        .remote_public_key(&server.public_key().0)
        .unwrap()
        .build_initiator()
        .unwrap();
    let mut message = [0; 96];
    let length = wrong_context.write_message(&[], &mut message).unwrap();
    let (mut a, b) = tokio::io::duplex(4096);
    write_handshake(&mut a, &message[..length]).await.unwrap();
    assert!(accept(b, config).await.is_err());
}

async fn forward(
    mut read: tokio::io::ReadHalf<DuplexStream>,
    mut write: tokio::io::WriteHalf<DuplexStream>,
    capture: Arc<Mutex<Vec<u8>>>,
    tamper: Arc<AtomicBool>,
) {
    let mut buffer = [0; 4096];
    while let Ok(count) = read.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        capture.lock().unwrap().extend_from_slice(&buffer[..count]);
        if tamper.swap(false, Ordering::SeqCst) {
            buffer[count - 1] ^= 1;
        }
        if write.write_all(&buffer[..count]).await.is_err() {
            break;
        }
    }
    let _ = write.shutdown().await;
}

#[tokio::test]
async fn hostile_relay_cannot_read_modify_or_replay_records() {
    let server = identity();
    let client = identity();
    let server_config = server.server_config(vec![client.public_key()]).unwrap();
    let (a, relay_a) = tokio::io::duplex(64 * 1024);
    let (relay_b, b) = tokio::io::duplex(64 * 1024);
    let (ra, wa) = tokio::io::split(relay_a);
    let (rb, wb) = tokio::io::split(relay_b);
    let capture = Arc::new(Mutex::new(Vec::new()));
    let tamper = Arc::new(AtomicBool::new(false));
    let up = tokio::spawn(forward(ra, wb, capture.clone(), tamper.clone()));
    let down = tokio::spawn(forward(
        rb,
        wa,
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(AtomicBool::new(false)),
    ));
    let (c, s) = tokio::join!(
        connect(a, client.client_config(server.public_key()).unwrap()),
        accept(b, server_config.clone())
    );
    let (mut c, mut s) = (c.unwrap(), s.unwrap());
    let replay = capture.lock().unwrap().clone();
    c.write_all(b"secret command").await.unwrap();
    c.flush().await.unwrap();
    let mut plaintext = [0; 14];
    s.read_exact(&mut plaintext).await.unwrap();
    assert_eq!(&plaintext, b"secret command");
    assert!(
        !capture
            .lock()
            .unwrap()
            .windows(14)
            .any(|w| w == b"secret command")
    );
    tamper.store(true, Ordering::SeqCst);
    c.write_all(b"another secret").await.unwrap();
    c.flush().await.unwrap();
    assert!(s.read_exact(&mut plaintext).await.is_err());
    up.abort();
    down.abort();

    // Replay a captured client handshake against fresh server ephemeral keys.
    let (mut a, b) = tokio::io::duplex(64 * 1024);
    a.write_all(&replay).await.unwrap();
    a.shutdown().await.unwrap();
    assert!(accept(b, server_config).await.is_err());
}

#[tokio::test]
async fn unauthenticated_eof_is_an_error_and_poisons_both_directions() {
    let server = identity();
    let client = identity();
    let (a, b) = tokio::io::duplex(4096);
    let (c, s) = tokio::join!(
        connect(a, client.client_config(server.public_key()).unwrap()),
        accept(b, server.server_config(vec![client.public_key()]).unwrap())
    );
    let mut s = s.unwrap();
    drop(c.unwrap());
    assert_eq!(
        s.read(&mut [0; 1]).await.unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert!(s.write_all(b"must fail").await.is_err());
}

pub(crate) fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn cacophony_interoperability_vector() {
    let vector: serde_json::Value =
        serde_json::from_str(include_str!("../test-vectors/noise-ik.json")).unwrap();
    let bytes = |key: &str| hex(vector[key].as_str().unwrap());
    let (is, ie, rs, re, pin, prologue) = (
        bytes("init_static"),
        bytes("init_ephemeral"),
        bytes("resp_static"),
        bytes("resp_ephemeral"),
        bytes("init_remote_static"),
        bytes("init_prologue"),
    );
    let mut initiator = snow::Builder::new(NOISE_PROTOCOL.parse().unwrap())
        .local_private_key(&is)
        .unwrap()
        .remote_public_key(&pin)
        .unwrap()
        .prologue(&prologue)
        .unwrap()
        .fixed_ephemeral_key_for_testing_only(&ie)
        .build_initiator()
        .unwrap();
    let mut responder = snow::Builder::new(NOISE_PROTOCOL.parse().unwrap())
        .local_private_key(&rs)
        .unwrap()
        .prologue(&prologue)
        .unwrap()
        .fixed_ephemeral_key_for_testing_only(&re)
        .build_responder()
        .unwrap();
    let messages = vector["messages"].as_array().unwrap();
    let mut out = [0; 256];
    let mut plain = [0; 256];
    for (index, message) in messages.iter().take(2).enumerate() {
        let payload = hex(message["payload"].as_str().unwrap());
        let expected = hex(message["ciphertext"].as_str().unwrap());
        let (sender, receiver) = if index == 0 {
            (&mut initiator, &mut responder)
        } else {
            (&mut responder, &mut initiator)
        };
        let len = sender.write_message(&payload, &mut out).unwrap();
        assert_eq!(out[..len], expected);
        let len = receiver.read_message(&out[..len], &mut plain).unwrap();
        assert_eq!(plain[..len], payload);
    }
    assert_eq!(initiator.get_handshake_hash(), bytes("handshake_hash"));
    let mut initiator = initiator.into_transport_mode().unwrap();
    let mut responder = responder.into_transport_mode().unwrap();
    for (index, message) in messages.iter().skip(2).enumerate() {
        let payload = hex(message["payload"].as_str().unwrap());
        let expected = hex(message["ciphertext"].as_str().unwrap());
        let (sender, receiver) = if index % 2 == 0 {
            (&mut initiator, &mut responder)
        } else {
            (&mut responder, &mut initiator)
        };
        let len = sender.write_message(&payload, &mut out).unwrap();
        assert_eq!(out[..len], expected);
        let len = receiver.read_message(&out[..len], &mut plain).unwrap();
        assert_eq!(plain[..len], payload);
    }
}

#[tokio::test]
async fn low_order_and_noncanonical_public_keys_are_rejected() {
    let mut one = [0; 32];
    one[0] = 1;
    let mut prime = [255; 32];
    prime[0] = 237;
    prime[31] = 127;
    for bad in [[0; 32], one, prime, [255; 32]] {
        assert!(URL_SAFE_NO_PAD.encode(bad).parse::<PublicKey>().is_err());
        for length in [48, 96] {
            let (mut writer, mut reader) = tokio::io::duplex(1024);
            let mut message = vec![0; length];
            message[..32].copy_from_slice(&bad);
            write_handshake(&mut writer, &message).await.unwrap();
            assert!(read_handshake(&mut reader, length).await.is_err());
        }
    }
}
