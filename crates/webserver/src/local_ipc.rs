//! Unix home-socket path and peer-identity hardening shared by the server,
//! standalone edge, and embedded `yas open` edge.

use std::ffi::OsString;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

pub const EXPECTED_SERVER_UID_ENV: &str = "YAS_SERVER_UID";

// `sockaddr_un.sun_path` is 104 bytes on Darwin and 108 on Linux, including
// the terminating NUL. Stay within the smaller native limit on every Unix we
// ship so an arbitrary long TMPDIR merely falls through to the next candidate.
const PORTABLE_SOCKET_PATH_BYTES: usize = 103;
// Match the placeholder byte length so candidate selection observes the path
// length of the template it returns. Muster rechecks the final substituted
// path for longer names.
const SOCKET_TEMPLATE_MARKER: &str = "marker";
pub const SOCKET_NAME_PLACEHOLDER: &str = "{name}";

pub fn effective_uid() -> u32 {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// Expected kernel peer UID for the fixed native YAS home server.
///
/// Same-user is the secure default. Cross-UID deployments must name the
/// expected numeric UID explicitly instead of disabling peer verification.
pub fn expected_server_uid() -> Result<u32, String> {
    parse_expected_server_uid(std::env::var_os(EXPECTED_SERVER_UID_ENV), effective_uid())
}

/// Parse a numeric expected peer UID from `environment`, defaulting to the
/// supplied UID when the variable is absent.
pub fn expected_peer_uid(environment: &str, default: u32) -> Result<u32, String> {
    parse_expected_peer_uid(environment, std::env::var_os(environment), default)
}

fn parse_expected_peer_uid(
    environment: &str,
    raw: Option<OsString>,
    default: u32,
) -> Result<u32, String> {
    let Some(raw) = raw else {
        return Ok(default);
    };
    let raw = raw
        .to_str()
        .ok_or_else(|| format!("{environment} must be a decimal numeric Unix UID"))?;
    raw.parse::<u32>()
        .map_err(|_| format!("{environment} must be a decimal numeric Unix UID"))
}

fn parse_expected_server_uid(raw: Option<OsString>, default: u32) -> Result<u32, String> {
    parse_expected_peer_uid(EXPECTED_SERVER_UID_ENV, raw, default)
}

const ROOT_UID: u32 = 0;

/// Decide whether a kernel-reported peer UID may serve `expected_uid`.
///
/// Same-user is the rule. Root is the one additional accepted identity, for
/// two reasons that both matter in practice:
///
/// * A connecting client's `SO_PEERCRED` (and `getpeereid`) reports the
///   credentials captured when the peer called `listen()`, not those of the
///   process that later `accept()`ed. Under service-manager socket activation
///   the listener is created by the manager, so a system-scope
///   `yas-server@%i.socket` handing fd 3 to a `User=%i` server reports UID 0
///   no matter which user actually runs the server. Refusing that made every
///   socket-activated system unit we ship unusable.
/// * Root can enter any UID at will, so a peer that is already root gains
///   nothing from being admitted here. The check exists to keep *other
///   unprivileged users* off the endpoint, and it still does: a listener whose
///   credentials read as UID 0 can only have been created by root.
fn peer_uid_is_trusted(actual_uid: u32, expected_uid: u32) -> bool {
    actual_uid == expected_uid || actual_uid == ROOT_UID
}

/// A connected peer's kernel-authenticated identity.
///
/// `pid` is zero where the platform does not report one: `SO_PEERCRED` carries
/// it, `getpeereid` does not. Nothing may treat a zero pid as "PID 1".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeerCredentials {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

/// Read a connected stream's peer credentials, for describing who a client is
/// rather than for deciding whether to talk to it — see [`verify_peer_uid`]
/// for the decision.
pub fn peer_credentials(stream: &impl AsRawFd) -> io::Result<PeerCredentials> {
    peer_credentials_raw(stream.as_raw_fd())
}

/// Verify a connected Unix stream before any home-server protocol bytes are
/// exchanged. The credential comes from the kernel, never the socket path.
pub fn verify_peer_uid(stream: &impl AsRawFd, expected_uid: u32) -> Result<(), String> {
    verify_peer_uid_named(stream, expected_uid, "native home-server")
}

/// Verify a connected Unix stream's kernel-authenticated peer UID before the
/// caller sends any protocol or credential bytes.
pub fn verify_peer_uid_named(
    stream: &impl AsRawFd,
    expected_uid: u32,
    peer_label: &str,
) -> Result<(), String> {
    let actual_uid = peer_uid(stream.as_raw_fd())
        .map_err(|error| format!("cannot read {peer_label} peer credentials: {error}"))?;
    if !peer_uid_is_trusted(actual_uid, expected_uid) {
        return Err(format!(
            "{peer_label} peer UID {actual_uid} does not match expected UID {expected_uid}"
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_credentials_raw(fd: RawFd) -> io::Result<PeerCredentials> {
    let credentials = peer_ucred(fd)?;
    Ok(PeerCredentials {
        pid: credentials.pid.max(0) as u32,
        uid: credentials.uid,
        gid: credentials.gid,
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_uid(fd: RawFd) -> io::Result<u32> {
    Ok(peer_ucred(fd)?.uid)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_ucred(fd: RawFd) -> io::Result<libc::ucred> {
    let mut credentials = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `credentials` points to writable storage of exactly `length`
    // bytes, and `fd` remains borrowed for the duration of the call.
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an unexpected credential size",
        ));
    }
    // SAFETY: a successful `getsockopt` initialized the complete `ucred`.
    Ok(unsafe { credentials.assume_init() })
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn peer_credentials_raw(fd: RawFd) -> io::Result<PeerCredentials> {
    let mut uid = 0;
    let mut gid = 0;
    // SAFETY: both output pointers are valid for writes and `fd` is borrowed.
    if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PeerCredentials { pid: 0, uid, gid })
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn peer_uid(fd: RawFd) -> io::Result<u32> {
    let mut uid = 0;
    let mut gid = 0;
    // These platforms expose the same authenticated peer identity through
    // `getpeereid`; Linux exposes it through `SO_PEERCRED` above.
    // SAFETY: both output pointers are valid for writes and `fd` is borrowed.
    if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
fn peer_uid(_fd: RawFd) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "kernel Unix peer credentials are unavailable on this platform",
    ))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
fn peer_credentials_raw(_fd: RawFd) -> io::Result<PeerCredentials> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "kernel Unix peer credentials are unavailable on this platform",
    ))
}

/// Resolve an automatic owner-only runtime socket path.
///
/// Explicit `YAS_SOCK` values are handled by callers and never
/// enter this function. Environment bases are only accepted when absolute,
/// component-normal, and either private to this UID or a root-owned sticky
/// temporary directory. Every accepted base receives a mode-0700 child.
pub fn automatic_socket_path(prefix: &str, server_name: &str) -> String {
    automatic_socket_path_from(
        prefix,
        server_name,
        effective_uid(),
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        std::env::var_os("TMPDIR").map(PathBuf::from),
        PathBuf::from("/run/user"),
        PathBuf::from("/tmp"),
    )
}

/// Resolve the canonical automatic socket path while leaving the server-name
/// component as an unambiguous placeholder.
///
/// This deliberately does not inspect `YAS_SOCK`: it predicts the
/// automatic endpoint for a *different* named server, even when this process
/// itself was started on an explicit socket.
pub fn automatic_socket_path_template(prefix: &str) -> String {
    automatic_socket_path_template_from(
        prefix,
        effective_uid(),
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        std::env::var_os("TMPDIR").map(PathBuf::from),
        PathBuf::from("/run/user"),
        PathBuf::from("/tmp"),
    )
}

fn automatic_socket_path_template_from(
    prefix: &str,
    uid: u32,
    xdg_runtime_dir: Option<PathBuf>,
    tmpdir: Option<PathBuf>,
    run_user_root: PathBuf,
    fallback_tmp: PathBuf,
) -> String {
    debug_assert_eq!(SOCKET_TEMPLATE_MARKER.len(), SOCKET_NAME_PLACEHOLDER.len());
    let prefix = if safe_component(prefix) {
        prefix
    } else {
        "yas"
    };
    let resolved = automatic_socket_path_from(
        prefix,
        SOCKET_TEMPLATE_MARKER,
        uid,
        xdg_runtime_dir,
        tmpdir,
        run_user_root,
        fallback_tmp,
    );
    let mut path = PathBuf::from(resolved);
    let marker_filename = format!("{prefix}-{SOCKET_TEMPLATE_MARKER}.sock");
    debug_assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some(marker_filename.as_str())
    );
    path.set_file_name(format!("{prefix}-{SOCKET_NAME_PLACEHOLDER}.sock"));
    path.to_string_lossy().into_owned()
}

/// Validate the parent and any existing final object for an automatic IPC
/// socket. The parent must be an effective-user-owned mode-0700 directory; an
/// existing final object is accepted only when it is that user's owner-only
/// Unix socket, so callers may remove a known-stale listener without touching
/// a symlink, regular file, or another user's endpoint.
pub fn validate_automatic_socket_path(path: &Path) -> io::Result<()> {
    let effective_uid = effective_uid();
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "automatic IPC socket has no parent directory",
        )
    })?;
    let parent_metadata = std::fs::symlink_metadata(parent)?;
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.uid() != effective_uid
        || parent_metadata.mode() & 0o777 != 0o700
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
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
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "automatic IPC socket path is prebound by an unsafe filesystem object",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Open a companion lock without following symlinks or accepting hard links.
/// The returned descriptor is owner-only and close-on-exec; callers retain it
/// while holding their advisory lock.
pub fn open_owner_only_lock_file(path: &Path) -> io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC lock path is not a singly-linked regular file",
        ));
    }
    if metadata.uid() != effective_uid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IPC lock file is not owned by the effective user",
        ));
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

fn automatic_socket_path_from(
    prefix: &str,
    server_name: &str,
    uid: u32,
    xdg_runtime_dir: Option<PathBuf>,
    tmpdir: Option<PathBuf>,
    run_user_root: PathBuf,
    fallback_tmp: PathBuf,
) -> String {
    let prefix = if safe_component(prefix) {
        prefix
    } else {
        "yas"
    };
    let server_name = if safe_component(server_name) {
        server_name
    } else {
        "default"
    };
    let filename = format!("{prefix}-{server_name}.sock");
    let mut bases = Vec::with_capacity(4);
    if let Some(base) = xdg_runtime_dir {
        bases.push((base, BasePolicy::PrivateOnly));
    }
    if let Some(base) = tmpdir {
        bases.push((base, BasePolicy::PrivateOrSticky));
    }
    bases.push((run_user_root.join(uid.to_string()), BasePolicy::PrivateOnly));
    bases.push((fallback_tmp.clone(), BasePolicy::PrivateOrSticky));

    for (base, policy) in bases {
        let Some(runtime_dir) = runtime_dir_for_base(&base, uid, policy) else {
            continue;
        };
        let socket = runtime_dir.join(&filename);
        if portable_socket_path(&socket) {
            return socket.to_string_lossy().into_owned();
        }
    }

    // A hostile pre-created `/tmp/yas-UID` can deny the conventional
    // fallback, but it cannot make us select a different user's listener:
    // server bind fails ownership checks and both home edges verify peer UID.
    // Keep the deterministic path so a later bind reports the real failure.
    fallback_tmp
        .join(format!("yas-{uid}"))
        .join(filename)
        .to_string_lossy()
        .into_owned()
}

#[derive(Clone, Copy)]
enum BasePolicy {
    PrivateOnly,
    PrivateOrSticky,
}

fn runtime_dir_for_base(base: &Path, uid: u32, policy: BasePolicy) -> Option<PathBuf> {
    if !normal_absolute_path(base) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(base).ok()?;
    if !metadata.file_type().is_dir() {
        return None;
    }

    let private = metadata.uid() == uid && metadata.mode() & 0o077 == 0;
    let sticky_shared = matches!(policy, BasePolicy::PrivateOrSticky)
        && metadata.uid() == 0
        && u64::from(metadata.mode()) & u64::from(libc::S_ISVTX) != 0
        && metadata.mode() & 0o002 != 0;
    let runtime_dir = if private {
        base.join("yas")
    } else if sticky_shared {
        base.join(format!("yas-{uid}"))
    } else {
        return None;
    };
    prepare_private_runtime_dir(&runtime_dir, uid).ok()?;
    Some(runtime_dir)
}

fn prepare_private_runtime_dir(path: &Path, uid: u32) -> io::Result<()> {
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }

    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC runtime path is not a directory",
        ));
    }
    if metadata.uid() != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IPC runtime directory is not owned by the effective user",
        ));
    }
    if metadata.mode() & 0o777 != 0o700 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    let checked = std::fs::symlink_metadata(path)?;
    if !checked.file_type().is_dir() || checked.uid() != uid || checked.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IPC runtime directory failed its owner-only check",
        ));
    }
    Ok(())
}

fn normal_absolute_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

fn portable_socket_path(path: &Path) -> bool {
    path.to_str().is_some() && path.as_os_str().as_bytes().len() <= PORTABLE_SOCKET_PATH_BYTES
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn assert_template_parity(
        expected_parent: &Path,
        xdg_runtime_dir: Option<PathBuf>,
        tmpdir: Option<PathBuf>,
        run_user_root: PathBuf,
        fallback_tmp: PathBuf,
    ) {
        let uid = effective_uid();
        let concrete = automatic_socket_path_from(
            "yas",
            "epic",
            uid,
            xdg_runtime_dir.clone(),
            tmpdir.clone(),
            run_user_root.clone(),
            fallback_tmp.clone(),
        );
        let template = automatic_socket_path_template_from(
            "yas",
            uid,
            xdg_runtime_dir,
            tmpdir,
            run_user_root,
            fallback_tmp,
        );
        assert_eq!(template.replace(SOCKET_NAME_PLACEHOLDER, "epic"), concrete);
        assert_eq!(Path::new(&concrete).parent().unwrap(), expected_parent);
    }

    #[test]
    fn private_runtime_child_is_created_owner_only() {
        let base = tempfile::tempdir().unwrap();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = automatic_socket_path_from(
            "yas",
            "work",
            effective_uid(),
            Some(base.path().to_path_buf()),
            None,
            base.path().join("unused-run"),
            base.path().join("unused-tmp"),
        );
        assert_eq!(Path::new(&path).file_name().unwrap(), "yas-work.sock");
        let runtime = Path::new(&path).parent().unwrap();
        assert_eq!(runtime, base.path().join("yas"));
        let metadata = std::fs::symlink_metadata(runtime).unwrap();
        assert_eq!(metadata.uid(), effective_uid());
        assert_eq!(metadata.mode() & 0o777, 0o700);
    }

    #[test]
    fn socket_template_matches_private_xdg_layout() {
        let root = tempfile::tempdir().unwrap();
        let xdg = root.path().join("xdg");
        std::fs::create_dir(&xdg).unwrap();
        std::fs::set_permissions(&xdg, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_template_parity(
            &xdg.join("yas"),
            Some(xdg),
            None,
            root.path().join("missing-run"),
            root.path().join("missing-tmp"),
        );
    }

    #[test]
    fn socket_template_matches_sticky_tmpdir_layout() {
        let root = tempfile::tempdir().unwrap();
        // macOS exposes /tmp as a symlink to the root-owned sticky directory.
        let tmp = std::fs::canonicalize("/tmp").unwrap();
        let metadata = std::fs::symlink_metadata(&tmp).unwrap();
        assert_eq!(
            metadata.uid(),
            0,
            "test requires the standard root-owned /tmp"
        );
        assert_ne!(u64::from(metadata.mode()) & u64::from(libc::S_ISVTX), 0);
        assert_template_parity(
            &tmp.join(format!("yas-{}", effective_uid())),
            None,
            Some(tmp),
            root.path().join("missing-run"),
            root.path().join("missing-fallback"),
        );
    }

    #[test]
    fn socket_template_matches_final_tmp_fallback_layout() {
        let root = tempfile::tempdir().unwrap();
        let unsafe_base = root.path().join("unsafe");
        std::fs::create_dir(&unsafe_base).unwrap();
        std::fs::set_permissions(&unsafe_base, std::fs::Permissions::from_mode(0o777)).unwrap();
        let fallback = PathBuf::from("/tmp");
        assert_template_parity(
            &fallback.join(format!("yas-{}", effective_uid())),
            Some(unsafe_base.clone()),
            Some(unsafe_base),
            root.path().join("missing-run"),
            fallback,
        );
    }

    #[test]
    fn unsafe_arbitrary_environment_bases_are_ignored() {
        let root = tempfile::tempdir().unwrap();
        let unsafe_base = root.path().join("shared-without-sticky");
        std::fs::create_dir(&unsafe_base).unwrap();
        std::fs::set_permissions(&unsafe_base, std::fs::Permissions::from_mode(0o777)).unwrap();
        let safe_base = root.path().join("safe");
        std::fs::create_dir(&safe_base).unwrap();
        std::fs::set_permissions(&safe_base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let run_user = safe_base.join(effective_uid().to_string());
        std::fs::create_dir(&run_user).unwrap();
        std::fs::set_permissions(&run_user, std::fs::Permissions::from_mode(0o700)).unwrap();

        let path = automatic_socket_path_from(
            "yas",
            "default",
            effective_uid(),
            Some(PathBuf::from("relative/xdg")),
            Some(unsafe_base),
            safe_base.clone(),
            root.path().join("unused"),
        );
        assert!(Path::new(&path).starts_with(run_user.join("yas")));
    }

    #[test]
    fn malicious_prebound_runtime_symlink_is_rejected() {
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let runtime = base.path().join("yas");
        symlink(&target, &runtime).unwrap();
        assert!(prepare_private_runtime_dir(&runtime, effective_uid()).is_err());
    }

    #[test]
    fn prebound_runtime_directory_with_wrong_expected_owner_is_rejected() {
        let base = tempfile::tempdir().unwrap();
        let runtime = base.path().join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        assert!(prepare_private_runtime_dir(&runtime, effective_uid().wrapping_add(1)).is_err());
    }

    #[test]
    fn peer_uid_acceptance_is_same_user_or_root() {
        assert!(peer_uid_is_trusted(1000, 1000));
        assert!(!peer_uid_is_trusted(1001, 1000));
        assert!(!peer_uid_is_trusted(65534, 1000));
        // Socket activation reports the service manager that called `listen()`,
        // so a `User=alice` server behind a system-scope unit authenticates as
        // root. Root is admitted; it can assume any UID regardless.
        assert!(peer_uid_is_trusted(0, 1000));
        // An unprivileged peer never satisfies a root-owned endpoint.
        assert!(!peer_uid_is_trusted(1000, 0));
    }

    #[test]
    fn same_user_listener_passes_kernel_verification() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("listener.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let client = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        verify_peer_uid(&client, effective_uid()).unwrap();
        drop(listener);
    }

    #[test]
    fn explicit_expected_uid_is_strictly_numeric() {
        assert_eq!(parse_expected_server_uid(None, 42).unwrap(), 42);
        assert_eq!(
            parse_expected_server_uid(Some(OsString::from("1001")), 42).unwrap(),
            1001
        );
        assert!(parse_expected_server_uid(Some(OsString::from("alice")), 42).is_err());
        assert!(parse_expected_server_uid(Some(OsString::from("-1")), 42).is_err());
    }

    #[test]
    fn hostile_server_name_cannot_escape_private_runtime_directory() {
        let base = tempfile::tempdir().unwrap();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = automatic_socket_path_from(
            "yas",
            "../../attacker",
            effective_uid(),
            Some(base.path().to_path_buf()),
            None,
            base.path().join("unused-run"),
            base.path().join("unused-tmp"),
        );
        assert_eq!(Path::new(&path), base.path().join("yas/yas-default.sock"));
    }
}
