use std::io;
use std::time::Duration;

use super::replacement::request_shutdown;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

pub type IpcStream = NamedPipeServer;

pub fn default_ipc_path() -> String {
    default_ipc_path_for(&crate::ServerName::default())
}

pub fn default_ipc_path_for(name: &crate::ServerName) -> String {
    if let Ok(user) = std::env::var("USERNAME") {
        format!(r"\\.\pipe\yas-{user}-{}", name.as_str())
    } else {
        format!(r"\\.\pipe\yas-{}", name.as_str())
    }
}

/// Canonical automatic YAS endpoint with `{name}` in place of the server
/// name. Explicit `YAS_SOCK` values intentionally do not affect it.
pub fn default_ipc_path_template() -> String {
    if let Ok(user) = std::env::var("USERNAME") {
        format!(r"\\.\pipe\yas-{user}-{{name}}")
    } else {
        String::from(r"\\.\pipe\yas-{name}")
    }
}

pub struct IpcListener {
    pipe_name: String,
    current: NamedPipeServer,
}

impl IpcListener {
    pub async fn bind(pipe_name: &str, verbose: bool) -> Self {
        let server = bind_replacing_existing(pipe_name)
            .await
            .unwrap_or_else(|e| {
                eprintln!("yas-server: cannot create named pipe {pipe_name}: {e}");
                std::process::exit(1);
            });
        if verbose {
            eprintln!("listening on {pipe_name}");
        }
        Self {
            pipe_name: pipe_name.to_string(),
            current: server,
        }
    }

    pub async fn accept(&mut self) -> std::io::Result<IpcStream> {
        self.current.connect().await?;
        let connected = std::mem::replace(
            &mut self.current,
            ServerOptions::new().create(&self.pipe_name)?,
        );
        Ok(connected)
    }
}

/// Claim the pipe name, gracefully stopping the previous server if one owns it.
///
/// `FILE_FLAG_FIRST_PIPE_INSTANCE` reports an existing instance as
/// `PermissionDenied` (`ERROR_ACCESS_DENIED`). Ordinary CLI commands auto-start
/// a detached server, so an explicit `yas server` commonly encounters one.
/// Match the Unix listener's replacement semantics by asking that server to
/// shut down, then waiting for Windows to release the pipe name.
async fn bind_replacing_existing(pipe_name: &str) -> io::Result<NamedPipeServer> {
    const REPLACE_ATTEMPTS: usize = 30;
    const REPLACE_INTERVAL: Duration = Duration::from_millis(100);

    let mut shutdown_requested = false;

    for attempt in 0..=REPLACE_ATTEMPTS {
        let in_use = match ServerOptions::new()
            .first_pipe_instance(true)
            .create(pipe_name)
        {
            Ok(server) => return Ok(server),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => error,
            Err(error) => return Err(error),
        };
        if attempt == REPLACE_ATTEMPTS {
            return Err(in_use);
        }

        if !shutdown_requested && let Ok(mut previous) = ClientOptions::new().open(pipe_name) {
            match request_shutdown(&mut previous).await {
                Ok(()) => {
                    eprintln!("yas-server: requesting previous server shutdown");
                    shutdown_requested = true;
                    // Closing the client can discard unread pipe data. Keep it
                    // open while the server consumes SHUTDOWN and drain replies
                    // until the server closes its end, with a bounded wait.
                    let _ = tokio::time::timeout(
                        Duration::from_secs(3),
                        tokio::io::copy(&mut previous, &mut tokio::io::sink()),
                    )
                    .await;
                }
                Err(error) => {
                    eprintln!("yas-server: cannot request previous server shutdown: {error}");
                }
            }
        }

        tokio::time::sleep(REPLACE_INTERVAL).await;
    }

    unreachable!("replacement loop always returns on its final attempt")
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;
    use yas_wire::{Frame, FrameCodec, FrameHeader, FrameLimits, family};

    use super::*;

    async fn read_frame(pipe: &mut NamedPipeServer, codec: &FrameCodec) -> Frame {
        let length = pipe.read_u32_le().await.unwrap();
        assert!(length <= codec.limits().max_wire_frame);
        let mut bytes = vec![0; length as usize];
        pipe.read_exact(&mut bytes).await.unwrap();
        codec.decode(&bytes).unwrap()
    }

    #[tokio::test]
    async fn bind_replaces_an_existing_server() {
        let pipe_name = format!(r"\\.\pipe\yas-test-replace-{}", std::process::id());
        let mut previous = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .unwrap();

        let previous_task = tokio::spawn(async move {
            previous.connect().await.unwrap();
            // The replacement must retain its client even when the previous
            // server does not consume the queued request immediately.
            tokio::time::sleep(Duration::from_millis(25)).await;
            let mut preface = [0; yas_wire::PREFACE.len()];
            previous.read_exact(&mut preface).await.unwrap();
            assert_eq!(preface, yas_wire::PREFACE);
            let codec = FrameCodec::pre_hello();
            let hello = read_frame(&mut previous, &codec).await;
            assert_eq!(
                hello.header,
                FrameHeader::request(family::CORE, yas_wire::core::request_kind::HELLO, 1)
            );
            let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
            let shutdown = read_frame(&mut previous, &codec).await;
            assert_eq!(
                shutdown.header,
                FrameHeader {
                    sensitive: true,
                    ..FrameHeader::request(family::CORE, yas_wire::core::request_kind::SHUTDOWN, 2)
                }
            );
        });

        let replacement = bind_replacing_existing(&pipe_name).await.unwrap();
        previous_task.await.unwrap();
        drop(replacement);
    }
}
