use futures_util::{SinkExt, StreamExt};
use std::{
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::Command,
};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;
use web_transport_quinn as wt;

pub fn cli(binary: &Path, directory: &Path, ca: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .kill_on_drop(true)
        .env("SSL_CERT_FILE", ca)
        .env("SSL_CERT_DIR", directory.join("empty-certs"))
        .env("XDG_STATE_HOME", directory.join("state"))
        .env("XDG_CACHE_HOME", directory.join("cache"))
        .env("XDG_CONFIG_HOME", directory.join("config"))
        .env("YAS_SOCK", directory.join("yas.sock"))
        .env("YAS_KV_PATH", directory.join("state/kv.redb"))
        .env(
            "YAS_EXTENSION_PATH",
            directory.join("state/extensions.redb"),
        )
        .env("YAS_PROXY", "0")
        .env("YAS_SKIP_COMPOSITOR", "1")
        .env("YAS_AUDIO", "0")
        .env("YAS_FONTS", "0")
        .env("YAS_RELAY", "0")
        .env("YAS_EXT", "0")
        .env("YAS_CHANNEL", "0")
        .env("YAS_REMOTES", directory.join("remotes"))
        .env_remove("YAS_TARGET")
        .stdin(Stdio::null());
    command
}

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub ca: std::path::PathBuf,
    pub uri: String,
    pub producer_public: yas_uplink::PublicKey,
    pub consumer_key: String,
    pub capture: Arc<Mutex<Vec<u8>>>,
    pub requests: Arc<Mutex<Vec<&'static str>>>,
    pub producer: tokio::process::Child,
    _server: tokio::process::Child,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Fixture {
    pub async fn start(binary: &Path) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for name in ["state", "cache", "config", "empty-certs"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let ca = root.join("ca.pem");
        std::fs::write(&ca, cert.cert.pem()).unwrap();
        let key =
            || rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into();
        let tls = TlsAcceptor::from(Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert.cert.der().clone()], key())
                .unwrap(),
        ));
        let mut worker = wt::ServerBuilder::new()
            .with_addr("127.0.0.1:0".parse().unwrap())
            .with_certificate(vec![cert.cert.der().clone()], key())
            .unwrap();
        let relay_url = format!(
            "https://127.0.0.1:{}/producer",
            worker.local_addr().unwrap().port()
        );
        let control = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control_url = format!("https://{}", control.local_addr().unwrap());
        let websocket = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_url = format!("wss://{}/consumer", websocket.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let control_tls = tls.clone();
        let control_task = tokio::spawn(async move {
            loop {
                let (stream, _) = control.accept().await.unwrap();
                let tls = control_tls.clone();
                let relay_url = relay_url.clone();
                let ws_url = ws_url.clone();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let Ok(mut stream) = tls.accept(stream).await else {
                        return;
                    };
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        header.push(stream.read_u8().await.unwrap());
                        assert!(header.len() < 16384);
                    }
                    let header = String::from_utf8(header).unwrap().to_ascii_lowercase();
                    let (status, body) = if header.starts_with("get /allocate ")
                        && header.contains("authorization: bearer producer-token\r\n")
                    {
                        seen.lock().unwrap().push("allocate");
                        (
                            "200 OK",
                            serde_json::json!({"relays": [relay_url]}).to_string(),
                        )
                    } else if header.starts_with("get /attach ")
                        && header.contains("authorization: bearer consumer-token\r\n")
                    {
                        seen.lock().unwrap().push("attach");
                        ("200 OK", serde_json::json!({"ws": ws_url}).to_string())
                    } else {
                        seen.lock().unwrap().push("rejected");
                        ("403 Forbidden", "{}".into())
                    };
                    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                    stream.shutdown().await.unwrap();
                });
            }
        });

        let (producer_key, producer_public) = yas_uplink::Identity::generate().unwrap();
        // Base64url keys can begin with '-': exercise the documented spaced
        // --allow-client argument, not the equals-form workaround.
        let (consumer_key, consumer_public) = (0..4096)
            .map(|_| yas_uplink::Identity::generate().unwrap())
            .find(|(_, public)| public.to_string().starts_with('-'))
            .expect("generate a hyphen-prefixed client public key");
        let mut server = cli(binary, root, &ca)
            .args(["server", "--name", "uplink-e2e", "--socket"])
            .arg(root.join("yas.sock"))
            .arg("--no-persistent-extensions")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        while tokio::net::UnixStream::connect(root.join("yas.sock"))
            .await
            .is_err()
        {
            assert!(
                server.try_wait().unwrap().is_none(),
                "isolated YAS server exited"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut producer = cli(binary, root, &ca)
            .env("YAS_UPLINK_IDENTITY", &producer_key)
            .env("YAS_UPLINK_TOKEN", "producer-token")
            .args([
                "uplink",
                &format!("{control_url}/allocate"),
                "--allow-client",
                &consumer_public.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let session = tokio::select! {
            session = async { worker.accept().await.unwrap().ok().await.unwrap() } => session,
            status = producer.wait() => panic!("uplink producer exited before connecting: {status:?}"),
        };
        let capture = Arc::new(Mutex::new(Vec::new()));
        let captured = capture.clone();
        let relay_task = tokio::spawn(async move {
            loop {
                let (stream, _) = websocket.accept().await.unwrap();
                let tls = tls.clone();
                let session = session.clone();
                let captured = captured.clone();
                tokio::spawn(async move {
                    let stream = tls.accept(stream).await.unwrap();
                    let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                    assert_eq!(
                        ws.next().await.unwrap().unwrap(),
                        Message::Text("consumer-token".into())
                    );
                    ws.send(Message::Text("ok".into())).await.unwrap();
                    let (mut send, mut recv) = session.open_bi().await.unwrap();
                    let (mut ws_send, mut ws_recv) = ws.split();
                    let upstream = async {
                        while let Some(Ok(Message::Binary(bytes))) = ws_recv.next().await {
                            captured.lock().unwrap().extend_from_slice(&bytes);
                            if send.write_all(&bytes).await.is_err() {
                                break;
                            }
                        }
                        let _ = send.shutdown().await;
                    };
                    let downstream = async {
                        let mut buffer = [0; 16384];
                        while let Ok(Some(count)) = recv.read(&mut buffer).await {
                            captured.lock().unwrap().extend_from_slice(&buffer[..count]);
                            if count == 0 {
                                break;
                            }
                            if ws_send
                                .send(Message::Binary(buffer[..count].to_vec().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        let _ = ws_send.close().await;
                    };
                    tokio::join!(upstream, downstream);
                });
            }
        });
        // Invoke the documented URL generator; the consumer resolves /attach
        // and uses the normal WSS connector, with no test-only transport hooks.
        let generated = cli(binary, root, &ca)
            .env("YAS_UPLINK_IDENTITY", &producer_key)
            .args([
                "uplink-url",
                &control_url,
                "--client-token",
                "consumer-token",
            ])
            .output()
            .await
            .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let uri = String::from_utf8(generated.stdout)
            .unwrap()
            .trim()
            .to_owned();

        Self {
            directory,
            ca,
            uri,
            producer_public,
            consumer_key: consumer_key.to_string(),
            capture,
            requests,
            producer,
            _server: server,
            tasks: vec![control_task, relay_task],
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.producer.start_kill();
        let _ = self._server.start_kill();
        for task in &self.tasks {
            task.abort();
        }
    }
}
