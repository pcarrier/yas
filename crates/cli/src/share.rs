//! `yas share`: publishing this machine's YAS server over WebRTC.
//!
//! Two ways in, one implementation. On its own the share is a client of the
//! server: it makes sure one is running, dials its socket per peer, and pools
//! those dials through the yas-proxy daemon so a burst of consumers does not
//! become a burst of processes. Hosted inside `yas server`, none of that
//! exists — the server is right here, and a peer's session is a channel to it.

use std::sync::Arc;

/// Where a share sends its peers, and how loudly it says so.
pub(crate) struct Options {
    pub hub: String,
    pub quiet: bool,
    pub verbose: bool,
    /// When set, peers are served by the server in this process: no socket,
    /// no proxy daemon, no second copy of the bytes.
    pub hosted: Option<yas_webrtc_forwarder::HostedConnector>,
}

/// The passphrase a share publishes under.
///
/// A configured passphrase makes a share resumable — the same URL survives a
/// restart, which is what a service unit wants. Without one, a fresh random
/// passphrase, because a share that reuses a passphrase nobody chose is a
/// share anyone who saw the old URL can still reach.
///
/// `YAS_SHARE_PASSPHRASE` exists for the folded deployment: one process
/// serving both a browser edge and a share is one process holding two secrets,
/// and they are not the same secret.
pub(crate) fn passphrase() -> String {
    std::env::var("YAS_SHARE_PASSPHRASE")
        .or_else(|_| std::env::var("YAS_PASSPHRASE"))
        .ok()
        .unwrap_or_else(|| {
            use rand::RngExt as _;
            const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
            let mut rng = rand::rng();
            let bytes: [u8; 26] = rng.random();
            bytes
                .iter()
                .map(|b| ALPHABET[(b & 0x1f) as usize] as char)
                .collect()
        })
}

pub(crate) async fn run(options: Options) {
    let signal_url = yas_webrtc_forwarder::normalize_hub(&options.hub);
    let passphrase = passphrase();
    let upstream = match options.hosted {
        Some(hosted) => yas_webrtc_forwarder::Upstream {
            hosted: Some(hosted),
            ..Default::default()
        },
        None => standalone_upstream().await,
    };

    yas_webrtc_forwarder::run(yas_webrtc_forwarder::Config {
        upstream,
        signal_url,
        passphrase,
        message_override: None,
        quiet: options.quiet,
        verbose: options.verbose,
    })
    .await;
}

/// The upstream for a share that is not the server: a socket, and the proxy
/// daemon in front of it when this deployment uses one.
async fn standalone_upstream() -> yas_webrtc_forwarder::Upstream {
    let sock_path = crate::transport::default_local_socket();
    if let Err(error) = crate::transport::ensure_local_server(&sock_path).await {
        eprintln!("yas: {error}");
        std::process::exit(1);
    }

    let proxy_sock = if crate::transport::proxy_enabled() {
        match crate::transport::ensure_proxy().await {
            Ok(sock) => Some(sock),
            Err(error) => {
                eprintln!("yas share: proxy auto-start failed: {error}");
                None
            }
        }
    } else {
        None
    };

    // Provide a callback to restart the proxy if it dies mid-session.
    let proxy_ensure: Option<yas_webrtc_forwarder::ProxyEnsureFn> = proxy_sock.as_ref().map(|_| {
        let exe = yas_proxy::yas_exe();
        Arc::new(move || {
            let exe = exe.clone();
            Box::pin(async move { yas_proxy::ensure_proxy(&exe, true).await })
                as std::pin::Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        }) as yas_webrtc_forwarder::ProxyEnsureFn
    });

    let proxy_uid = {
        #[cfg(unix)]
        {
            proxy_sock
                .as_ref()
                .map(|_| yas_proxy::expected_proxy_uid())
                .transpose()
                .unwrap_or_else(|error| {
                    eprintln!("yas share: invalid proxy UID: {error}");
                    std::process::exit(1);
                })
        }
        #[cfg(not(unix))]
        {
            None
        }
    };

    yas_webrtc_forwarder::Upstream {
        sock_path,
        proxy_sock,
        proxy_uid,
        proxy_ensure,
        hosted: None,
    }
}
