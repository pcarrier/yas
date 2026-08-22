use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::RawFd;
use std::path::Path;
use tokio::io::unix::AsyncFd;
use tokio::net::UnixListener;

pub type IpcStream = tokio::net::UnixStream;

pub fn default_ipc_path() -> String {
    default_ipc_path_for(&crate::ServerName::default())
}

pub fn default_ipc_path_for(name: &crate::ServerName) -> String {
    yas_webserver::local_ipc::automatic_socket_path("yas", name.as_str())
}

/// Canonical automatic YAS endpoint with `{name}` in place of the server
/// name. Explicit `YAS_SOCK` values intentionally do not affect it.
pub fn default_ipc_path_template() -> String {
    yas_webserver::local_ipc::automatic_socket_path_template("yas")
}

pub struct IpcListener {
    inner: UnixListener,
    /// Held for the process lifetime so the flock is released on exit.
    _lock: Option<std::fs::File>,
}

fn open_lock_file(lock_path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(lock_path)?;
    let metadata = lock_file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "IPC lock path is not a regular file",
        ));
    }
    if metadata.nlink() != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "IPC lock file has multiple hard links",
        ));
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "IPC lock file is not owned by the effective user",
        ));
    }
    // `mode` applies only when this call created the file. Tighten a safe
    // pre-existing lock through its already-validated descriptor as well.
    lock_file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(lock_file)
}

fn validate_automatic_socket_path(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let effective_uid = unsafe { libc::geteuid() };
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "automatic IPC socket has no parent directory",
        )
    })?;
    let parent_metadata = std::fs::symlink_metadata(parent)?;
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.uid() != effective_uid
        || parent_metadata.mode() & 0o777 != 0o700
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "automatic IPC socket parent is not an effective-user-owned mode-0700 directory",
        ));
    }

    match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_socket()
                && metadata.uid() == effective_uid
                && metadata.mode() & 0o077 == 0 =>
        {
            Ok(())
        }
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "automatic IPC socket path is prebound by an unsafe filesystem object",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

impl IpcListener {
    pub fn bind(path: &str, verbose: bool, automatic_path: bool) -> Self {
        if automatic_path {
            validate_automatic_socket_path(Path::new(path)).unwrap_or_else(|error| {
                eprintln!("yas-server: refusing unsafe automatic IPC path {path}: {error}");
                std::process::exit(1);
            });
        }
        // Acquire an exclusive flock on a companion lockfile so that a
        // previous server instance is terminated before we bind.  The OS
        // releases the lock automatically when the holder exits — even on
        // SIGKILL — so there are no stale-lock issues.
        let lock_path = format!("{path}.lock");
        let lock_file = open_lock_file(Path::new(&lock_path)).unwrap_or_else(|e| {
            eprintln!("yas-server: cannot open {lock_path}: {e}");
            std::process::exit(1);
        });

        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        let mut locked = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0;

        if !locked {
            // Lock held by another server — read its PID and terminate it.
            let mut pid_str = String::new();
            if std::io::Read::read_to_string(&mut (&lock_file), &mut pid_str).is_ok()
                && let Ok(old_pid) = pid_str.trim().parse::<i32>()
            {
                eprintln!("yas-server: terminating previous server (pid {old_pid})");
                unsafe { libc::kill(old_pid, libc::SIGTERM) };
            }
            // Wait up to 3 s for the old server to release the lock.
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    locked = true;
                    break;
                }
            }
            if !locked {
                eprintln!(
                    "yas-server: cannot acquire lock {lock_path} — is another server running?"
                );
                std::process::exit(1);
            }
        }

        // Record our PID so the next server instance can terminate us.
        {
            use std::io::{Seek, Write};
            let _ = lock_file.set_len(0);
            let _ = (&lock_file).seek(std::io::SeekFrom::Start(0));
            let _ = write!(&lock_file, "{}", std::process::id());
        }

        // Revalidate after acquiring the companion lock and immediately
        // before removing a stale socket. The private parent makes a
        // cross-user replacement impossible; this second check also catches
        // accidental same-process mutation between the two steps.
        if automatic_path {
            validate_automatic_socket_path(Path::new(path)).unwrap_or_else(|error| {
                eprintln!("yas-server: refusing unsafe automatic IPC path {path}: {error}");
                std::process::exit(1);
            });
        }
        let _ = std::fs::remove_file(path);
        // Set a restrictive umask before bind so the socket is created with
        // 0700 permissions atomically, closing the race window between bind
        // and the subsequent chmod.
        let old_umask = unsafe { libc::umask(0o077) };
        let listener = UnixListener::bind(path).unwrap_or_else(|e| {
            unsafe { libc::umask(old_umask) };
            eprintln!("yas-server: cannot bind to {path}: {e}");
            std::process::exit(1);
        });
        unsafe { libc::umask(old_umask) };
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)) {
            eprintln!("yas-server: warning: cannot set socket permissions: {e}");
        }
        if verbose {
            eprintln!("listening on {path}");
        }
        Self {
            inner: listener,
            _lock: Some(lock_file),
        }
    }

    pub fn from_systemd_fd(verbose: bool) -> Option<Self> {
        let fds = std::env::var("LISTEN_FDS").ok()?;
        if fds.trim() != "1" {
            if verbose {
                eprintln!("LISTEN_FDS={fds}, expected 1; falling back to bind");
            }
            return None;
        }
        let pid = std::env::var("LISTEN_PID").ok()?;
        if pid.trim() != std::process::id().to_string() {
            if verbose {
                eprintln!(
                    "LISTEN_PID={pid} does not match our pid {}; falling back to bind",
                    std::process::id()
                );
            }
            return None;
        }
        use std::os::unix::io::FromRawFd;
        let std_listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(3) };
        std_listener.set_nonblocking(true).unwrap();
        if verbose {
            eprintln!("using socket activation (fd 3)");
        }
        Some(Self {
            inner: UnixListener::from_std(std_listener).unwrap(),
            _lock: None,
        })
    }

    pub async fn accept(&self) -> std::io::Result<IpcStream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(stream)
    }
}

enum RecvFdResult {
    Fd(RawFd),
    WouldBlock,
    Closed,
}

fn recv_fd(channel: RawFd) -> RecvFdResult {
    unsafe {
        let mut buf = [0u8; 1];
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut libc::c_void,
            iov_len: buf.len(),
        };
        let cmsg_space = libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) as usize;
        let mut cmsg_buf = vec![0u8; cmsg_space];
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = cmsg_space as _;
        let n = libc::recvmsg(channel, &mut msg, libc::MSG_DONTWAIT);
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return RecvFdResult::WouldBlock;
            }
            if err.raw_os_error() == Some(libc::EINTR) {
                return RecvFdResult::WouldBlock;
            }
            return RecvFdResult::Closed;
        }
        if n == 0 {
            return RecvFdResult::Closed;
        }
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            return RecvFdResult::Closed;
        }
        if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
            let fd_ptr = libc::CMSG_DATA(cmsg) as *const RawFd;
            RecvFdResult::Fd(std::ptr::read_unaligned(fd_ptr))
        } else {
            RecvFdResult::Closed
        }
    }
}

pub async fn run_fd_channel(channel_fd: RawFd, state: crate::AppState) {
    use std::os::unix::io::FromRawFd;
    if state.config.verbose {
        eprintln!("accepting clients via fd-channel (fd {channel_fd})");
    }
    let channel = unsafe { std::os::unix::net::UnixStream::from_raw_fd(channel_fd) };
    channel.set_nonblocking(true).unwrap();
    let async_channel = AsyncFd::new(channel).unwrap();
    loop {
        let mut guard = match async_channel.readable().await {
            Ok(g) => g,
            Err(e) => {
                eprintln!("fd-channel error: {e}");
                break;
            }
        };
        match recv_fd(channel_fd) {
            RecvFdResult::Fd(client_fd) => {
                let std_stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(client_fd) };
                std_stream.set_nonblocking(true).unwrap();
                let stream = tokio::net::UnixStream::from_std(std_stream).unwrap();
                let origin = crate::local_origin(&stream);
                crate::spawn_yas_client(stream, state.clone(), origin);
                guard.retain_ready();
            }
            RecvFdResult::WouldBlock => {
                guard.clear_ready();
            }
            RecvFdResult::Closed => {
                break;
            }
        }
    }
    if state.config.verbose {
        eprintln!("fd-channel closed, shutting down");
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, symlink};
    use std::os::unix::io::AsRawFd;

    #[test]
    fn named_socket_has_an_instance_suffix() {
        let name: crate::ServerName = "work".parse().unwrap();
        let path = default_ipc_path_for(&name);
        assert!(path.ends_with("yas-work.sock"), "{path}");
        assert_ne!(path, default_ipc_path());
    }

    #[test]
    fn default_socket_has_the_default_instance_suffix() {
        assert!(default_ipc_path().ends_with("yas-default.sock"));
    }

    #[test]
    fn canonical_socket_is_named_for_the_instance() {
        let name: crate::ServerName = "work".parse().unwrap();
        let yas = default_ipc_path_for(&name);
        assert!(yas.ends_with("yas-work.sock"), "{yas}");
    }

    #[test]
    fn lock_file_rejects_a_symlink_without_touching_its_target() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        let lock = directory.path().join("server.lock");
        std::fs::write(&target, b"do not truncate").unwrap();
        symlink(&target, &lock).unwrap();

        assert!(open_lock_file(&lock).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"do not truncate");
    }

    #[test]
    fn lock_file_rejects_a_hard_link_without_touching_its_target() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        let lock = directory.path().join("server.lock");
        std::fs::write(&target, b"do not truncate").unwrap();
        std::fs::hard_link(&target, &lock).unwrap();

        assert!(open_lock_file(&lock).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"do not truncate");
    }

    #[test]
    fn lock_file_is_owner_only_and_close_on_exec() {
        let directory = tempfile::tempdir().unwrap();
        let lock = directory.path().join("server.lock");
        let permissive = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o666)
            .open(&lock)
            .unwrap();
        permissive
            .set_permissions(std::fs::Permissions::from_mode(0o666))
            .unwrap();
        drop(permissive);

        let lock_file = open_lock_file(&lock).unwrap();
        assert_eq!(lock_file.metadata().unwrap().mode() & 0o777, 0o600);
        let descriptor_flags = unsafe { libc::fcntl(lock_file.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(descriptor_flags, -1);
        assert_ne!(descriptor_flags & libc::FD_CLOEXEC, 0);
    }

    #[test]
    fn automatic_socket_parent_must_be_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let socket = directory.path().join("yas-default.sock");
        assert!(validate_automatic_socket_path(&socket).is_err());

        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(validate_automatic_socket_path(&socket).is_ok());
    }

    #[test]
    fn automatic_socket_rejects_malicious_final_symlink() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = directory.path().join("target");
        let socket = directory.path().join("yas-default.sock");
        std::fs::write(&target, b"untouched").unwrap();
        symlink(&target, &socket).unwrap();

        assert!(validate_automatic_socket_path(&socket).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"untouched");
    }

    #[tokio::test]
    async fn automatic_listener_and_companion_lock_are_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.path().join("yas-default.sock");
        let path = socket.to_str().unwrap();
        let listener = IpcListener::bind(path, false, true);

        assert_eq!(
            std::fs::symlink_metadata(&socket).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::symlink_metadata(format!("{path}.lock"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
        drop(listener);
    }
}
