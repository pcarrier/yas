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
    // Disposable example pair also verifies raw-seed -> PKCS#8 -> rustls import.
    let private = "3iAcr2-GUIpQfQsOaabe-eD6uK8O53exaB9pKnVfotI";
    let public = "zd5N9JFta-3KOvvXwQshnMRKNHVIiUEJSuaKMO56Uxk";
    let identity = Identity::from_base64(private).unwrap();
    assert_eq!(identity.public_key().to_string(), public);
    let old_pkcs8 = "MC4CAQAwBQYDK2VwBCIEIN4gHK9vhlCKUH0LDmmm3vng+rivDud3sWgfaSp1X6LS";
    assert!(Identity::from_base64(old_pkcs8).is_err());
}

#[tokio::test]
async fn mutual_authentication_and_exporter_agreement() {
    let server = identity();
    let client = identity();
    let (a, b) = tokio::io::duplex(64 * 1024);
    let (client, server) = tokio::join!(
        connect(a, client.client_config(server.public_key()).unwrap()),
        accept(b, server.server_config(vec![client.public_key()]).unwrap()),
    );
    let (mut client, mut server) = (client.unwrap(), server.unwrap());
    assert_eq!(
        client.get_ref().1.protocol_version(),
        Some(rustls::ProtocolVersion::TLSv1_3)
    );
    assert_eq!(
        client
            .get_ref()
            .1
            .negotiated_key_exchange_group()
            .unwrap()
            .name(),
        rustls::NamedGroup::X25519
    );
    let c = client
        .get_ref()
        .1
        .export_keying_material(
            DatagramKeyMaterial::new([0; 64]),
            DATAGRAM_EXPORTER_LABEL,
            Some(&[]),
        )
        .unwrap();
    let s = server
        .get_ref()
        .1
        .export_keying_material(
            DatagramKeyMaterial::new([0; 64]),
            DATAGRAM_EXPORTER_LABEL,
            Some(&[]),
        )
        .unwrap();
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
    // Knowing an allowed public key is insufficient: the handshake signature
    // must be made by its private key, not by the relay's own signing key.
    let forged = Identity {
        key: Arc::new(rustls::sign::CertifiedKey::new(
            vec![client.public_key().spki().into()],
            attacker.key.key.clone(),
        )),
        public: client.public_key(),
    };
    let (a, b) = tokio::io::duplex(64 * 1024);
    let (c, s) = tokio::join!(
        connect(a, forged.client_config(server.public_key()).unwrap()),
        accept(b, server_config)
    );
    assert!(c.is_err());
    assert!(s.is_err());
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
async fn client_authentication_and_alpn_are_mandatory() {
    let server = identity();
    let client = identity();
    let server_config = server.server_config(vec![client.public_key()]).unwrap();
    let mut anonymous = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedKeys(vec![server.public_key()])))
        .with_no_client_auth();
    anonymous.alpn_protocols = vec![ALPN.to_vec()];
    let mut missing_alpn = client.client_config(server.public_key()).unwrap();
    Arc::get_mut(&mut missing_alpn)
        .unwrap()
        .alpn_protocols
        .clear();
    for config in [Arc::new(anonymous), missing_alpn] {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (c, s) = tokio::join!(connect(a, config), accept(b, server_config.clone()));
        assert!(c.is_err());
        assert!(s.is_err());
    }
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
