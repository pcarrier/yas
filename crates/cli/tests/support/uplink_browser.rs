//! Local infrastructure for Playwright; stdout is a JSON readiness/report pipe.
#[cfg(unix)]
mod uplink;

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::{io::Write, path::PathBuf, process::Stdio, time::Duration};
    use tokio::io::AsyncReadExt;
    use uplink::{Fixture, cli};

    let binary = PathBuf::from(std::env::args_os().nth(1).expect("YAS binary path"));
    let mut fixture = tokio::time::timeout(Duration::from_secs(30), Fixture::start(&binary))
        .await
        .expect("uplink fixture startup timed out");
    assert!(fixture.uri.contains(&fixture.producer_public.to_string()));
    let root = fixture.directory.path();
    let home = root.join("home");
    for name in ["", "state", "cache", "config", "empty-certs"] {
        std::fs::create_dir_all(home.join(name)).unwrap();
    }
    std::fs::write(
        home.join("remotes"),
        format!("uplink-test = {}\n", fixture.uri),
    )
    .unwrap();
    let mut home_server = cli(&binary, &home, &fixture.ca)
        .env("YAS_RELAY", "1")
        .env("YAS_UPLINK_IDENTITY", &fixture.consumer_key)
        .args([
            "server",
            "--name",
            "uplink-browser-home",
            "--no-persistent-extensions",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        while tokio::net::UnixStream::connect(home.join("yas.sock"))
            .await
            .is_err()
        {
            assert!(home_server.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("home YAS server startup timed out");
    let terminal = cli(&binary, root, &fixture.ca)
        .args(["terminal", "start", "--tag", "uplink-browser", "--cwd"])
        .arg(root)
        .args(["--env", "PS1=UPLINK-READY> ", "/bin/sh", "-i"])
        .output()
        .await
        .unwrap();
    assert!(
        terminal.status.success(),
        "{}",
        String::from_utf8_lossy(&terminal.stderr)
    );
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut edge = cli(&binary, &home, &fixture.ca)
        .env("YAS_ADDR", format!("127.0.0.1:{port}"))
        .env("YAS_PASSPHRASE", "test-secret")
        .arg("edge")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    println!(
        "{}",
        serde_json::json!({
            "baseURL": format!("http://127.0.0.1:{port}"),
            "markerPath": root.join("browser-proof"),
            "homeMarkerPath": home.join("browser-proof"),
        })
    );
    std::io::stdout().flush().unwrap();
    let mut input = Vec::new();
    let mut terminate =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
    let mut stdin = tokio::io::stdin();
    tokio::select! {
        result = stdin.read_to_end(&mut input) => { result.unwrap(); }
        _ = terminate.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    let report = serde_json::json!({
        "attaches": fixture.requests.lock().unwrap().iter().filter(|&&r| r == "attach").count(),
        "capturedBytes": fixture.capture.lock().unwrap().len(),
        "plaintextLeaked": fixture.capture.lock().unwrap().windows(b"browser-uplink-proof-73b1".len())
            .any(|bytes| bytes == b"browser-uplink-proof-73b1"),
        "producerAlive": fixture.producer.try_wait().unwrap().is_none(),
    });
    edge.kill().await.unwrap();
    home_server.kill().await.unwrap();
    println!("{report}");
}

#[cfg(not(unix))]
fn main() {
    panic!("the uplink browser fixture requires Unix sockets");
}
