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
pub(crate) fn passphrase() -> Result<String, String> {
    passphrase_from_env(|name| std::env::var(name))
}

fn passphrase_from_env(
    mut get: impl FnMut(&str) -> Result<String, std::env::VarError>,
) -> Result<String, String> {
    for name in ["YAS_SHARE_PASSPHRASE", "YAS_PASSPHRASE"] {
        match get(name) {
            Ok(value) if value.trim().is_empty() => {
                return Err(format!("{name} must not be empty or whitespace-only"));
            }
            Ok(value) => return Ok(value),
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(format!("{name} must contain valid Unicode"));
            }
        }
    }

    use rand::RngExt as _;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut rng = rand::rng();
    let bytes: [u8; 26] = rng.random();
    Ok(bytes
        .iter()
        .map(|b| ALPHABET[(b & 0x1f) as usize] as char)
        .collect())
}

pub(crate) async fn run(options: Options) {
    let signal_url = yas_webrtc_forwarder::normalize_hub(&options.hub);
    let passphrase = passphrase().unwrap_or_else(|error| {
        eprintln!("yas share: {error}");
        std::process::exit(1);
    });
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
    .await
    .unwrap_or_else(|error| {
        eprintln!("yas share: {error}");
        std::process::exit(1);
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::VarError;

    fn configured(share: Option<&str>, fallback: Option<&str>) -> Result<String, String> {
        passphrase_from_env(|name| {
            match name {
                "YAS_SHARE_PASSPHRASE" => share,
                "YAS_PASSPHRASE" => fallback,
                _ => panic!("unexpected variable: {name}"),
            }
            .map(str::to_string)
            .ok_or(VarError::NotPresent)
        })
    }

    #[test]
    fn blank_share_override_is_rejected_without_falling_back() {
        for blank in ["", " ", "\t\r\n", "\u{a0}\u{2003}"] {
            for fallback in [None, Some("strong-fallback-secret")] {
                assert_eq!(
                    configured(Some(blank), fallback),
                    Err("YAS_SHARE_PASSPHRASE must not be empty or whitespace-only".to_string())
                );
            }
        }
    }

    #[test]
    fn blank_fallback_is_rejected() {
        for blank in ["", " ", "\t\r\n", "\u{a0}\u{2003}"] {
            assert_eq!(
                configured(None, Some(blank)),
                Err("YAS_PASSPHRASE must not be empty or whitespace-only".to_string())
            );
        }
    }

    #[test]
    fn nonblank_passphrases_keep_their_bytes_and_precedence() {
        for secret in ["secret", "  secret\t\n", "\u{2003}秘密\u{a0}"] {
            for fallback in [None, Some(""), Some("other-secret")] {
                assert_eq!(configured(Some(secret), fallback).unwrap(), secret);
            }
            assert_eq!(configured(None, Some(secret)).unwrap(), secret);
        }
    }

    #[test]
    fn absent_configuration_generates_fresh_random_passphrases() {
        let first = configured(None, None).unwrap();
        let second = configured(None, None).unwrap();
        for value in [&first, &second] {
            assert_eq!(value.len(), 26);
            assert!(
                value
                    .bytes()
                    .all(|b| b"abcdefghijklmnopqrstuvwxyz234567".contains(&b))
            );
        }
        assert_ne!(first, second);
    }

    #[test]
    fn invalid_unicode_is_not_treated_as_absent() {
        for invalid_name in ["YAS_SHARE_PASSPHRASE", "YAS_PASSPHRASE"] {
            let result = passphrase_from_env(|name| {
                if name == invalid_name {
                    Err(VarError::NotUnicode("redacted".into()))
                } else if name == "YAS_SHARE_PASSPHRASE" {
                    Err(VarError::NotPresent)
                } else {
                    panic!("invalid override must not consult the fallback");
                }
            });
            assert_eq!(
                result,
                Err(format!("{invalid_name} must contain valid Unicode"))
            );
        }
    }
}
