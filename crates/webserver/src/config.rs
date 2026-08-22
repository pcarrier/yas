use axum::extract::ws::{Message, WebSocket};
use futures_util::SinkExt;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

pub use crate::passphrase::AuthPassphrase;

const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
const AUTH_MAX_UNAUTHENTICATED: usize = 8;
const AUTH_MAX_UNAUTHENTICATED_PER_PEER: usize = 2;
const AUTH_MAX_PASSPHRASE_BYTES: usize = 4 * 1024;
const AUTH_MAX_ARGON2_VERIFICATIONS: usize = 2;
const AUTH_ARGON2_VERIFY_TIMEOUT: Duration = Duration::from_secs(10);
const AUTH_MAX_FAILURES: u32 = 5;
const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);
const AUTH_LOCKOUT: Duration = Duration::from_secs(60);
/// Most peers tracked for failed-auth history at once.
///
/// `prune` already drops peers whose window has passed, so the live set is
/// "distinct peers that failed inside the last `AUTH_FAILURE_WINDOW`" — which
/// is attacker-chosen, because the key is the remote address. A source with a
/// large address range (any IPv6 allocation) gets a fresh key per attempt and
/// grows this map for five minutes at a time.
const AUTH_MAX_TRACKED_PEERS: usize = 4096;

/// Argon2 is deliberately expensive and must never run on Tokio's async
/// workers. The permit moves into the blocking task so a timed-out join still
/// occupies its slot until the uncancellable blocking verification actually
/// exits.
static AUTH_ARGON2_SLOTS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(AUTH_MAX_ARGON2_VERIFICATIONS)));

/// Shared authentication throttle for WebSocket/WebTransport passphrase checks.
///
/// It limits concurrent unauthenticated handshakes globally and temporarily
/// locks out peers that repeatedly fail authentication. Peer keys are supplied
/// by callers (typically the remote IP address, or a global fallback when the
/// transport cannot expose one).
#[derive(Clone)]
pub struct AuthThrottle {
    inner: Arc<Mutex<AuthThrottleInner>>,
    max_unauthenticated: usize,
    max_unauthenticated_per_peer: usize,
    max_failures: u32,
    failure_window: Duration,
    lockout: Duration,
    max_tracked_peers: usize,
}

struct AuthThrottleInner {
    active_unauthenticated: usize,
    active_by_peer: HashMap<String, usize>,
    peers: HashMap<String, PeerAuthState>,
}

struct PeerAuthState {
    failures: u32,
    first_failure: Instant,
    locked_until: Option<Instant>,
}

/// RAII guard for one in-progress unauthenticated auth attempt.
pub struct AuthAttemptGuard {
    throttle: AuthThrottle,
    peer: String,
    released: bool,
}

impl Default for AuthThrottle {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AuthContext<'a> {
    pub throttle: &'a AuthThrottle,
    pub peer: &'a str,
}

impl AuthThrottle {
    pub fn new() -> Self {
        Self::with_limits(
            AUTH_MAX_UNAUTHENTICATED,
            AUTH_MAX_FAILURES,
            AUTH_FAILURE_WINDOW,
            AUTH_LOCKOUT,
        )
    }

    fn with_limits(
        max_unauthenticated: usize,
        max_failures: u32,
        failure_window: Duration,
        lockout: Duration,
    ) -> Self {
        Self::with_limits_and_capacity(
            max_unauthenticated,
            max_failures,
            failure_window,
            lockout,
            AUTH_MAX_TRACKED_PEERS,
        )
    }

    fn with_limits_and_capacity(
        max_unauthenticated: usize,
        max_failures: u32,
        failure_window: Duration,
        lockout: Duration,
        max_tracked_peers: usize,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AuthThrottleInner {
                active_unauthenticated: 0,
                active_by_peer: HashMap::new(),
                peers: HashMap::new(),
            })),
            max_unauthenticated,
            max_unauthenticated_per_peer: AUTH_MAX_UNAUTHENTICATED_PER_PEER
                .min(max_unauthenticated.max(1)),
            max_failures: max_failures.max(1),
            failure_window,
            lockout,
            max_tracked_peers: max_tracked_peers.max(1),
        }
    }

    pub fn begin(&self, peer: impl Into<String>) -> Option<AuthAttemptGuard> {
        let peer = peer.into();
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        inner.prune(now, self.failure_window);

        // Both refusals answer the client with AUTH_BUSY, which is deliberately
        // indistinguishable from a healthy server to anyone probing. Say why
        // here so an operator seeing "server busy" in the UI can tell a peer
        // lockout from saturation without reproducing it.
        if inner.active_unauthenticated >= self.max_unauthenticated {
            eprintln!(
                "yas: auth refused for {peer}: {} concurrent unauthenticated handshakes \
                 (limit {})",
                inner.active_unauthenticated, self.max_unauthenticated
            );
            return None;
        }
        let active_for_peer = inner.active_by_peer.get(&peer).copied().unwrap_or(0);
        if active_for_peer >= self.max_unauthenticated_per_peer {
            eprintln!(
                "yas: auth refused for {peer}: {active_for_peer} concurrent unauthenticated \
                 handshakes from that peer (limit {})",
                self.max_unauthenticated_per_peer
            );
            return None;
        }
        if let Some(until) = inner
            .peers
            .get(&peer)
            .and_then(|state| state.locked_until)
            .filter(|until| *until > now)
        {
            eprintln!(
                "yas: auth refused for {peer}: locked out for another {}s",
                until.duration_since(now).as_secs()
            );
            return None;
        }

        inner.active_unauthenticated += 1;
        *inner.active_by_peer.entry(peer.clone()).or_default() += 1;
        Some(AuthAttemptGuard {
            throttle: self.clone(),
            peer,
            released: false,
        })
    }

    fn record_success(&self, peer: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.peers.remove(peer);
    }

    fn record_failure(&self, peer: &str, stalled: bool) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        inner.prune(now, self.failure_window);
        if !inner.peers.contains_key(peer) {
            inner.make_room(self.max_tracked_peers, now, self.failure_window);
        }
        let state = inner
            .peers
            .entry(peer.to_string())
            .or_insert_with(|| PeerAuthState {
                failures: 0,
                first_failure: now,
                locked_until: None,
            });

        if now.duration_since(state.first_failure) > self.failure_window {
            state.failures = 0;
            state.first_failure = now;
            state.locked_until = None;
        }
        state.failures = state.failures.saturating_add(1);
        if state.failures >= self.max_failures {
            state.failures = 0;
            state.first_failure = now;
            state.locked_until = Some(now + self.lockout);
            eprintln!(
                "yas: auth lockout for {peer}: {} rejected or stalled handshakes within {}s — \
                 refusing for {}s",
                self.max_failures,
                self.failure_window.as_secs(),
                self.lockout.as_secs()
            );
        } else {
            eprintln!(
                "yas: {} from {peer} ({}/{} before lockout)",
                if stalled {
                    "authentication handshake timed out"
                } else {
                    "wrong passphrase"
                },
                state.failures,
                self.max_failures
            );
        }
    }

    fn release(&self, peer: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.active_unauthenticated = inner.active_unauthenticated.saturating_sub(1);
        if let Some(active) = inner.active_by_peer.get_mut(peer) {
            *active = active.saturating_sub(1);
            if *active == 0 {
                inner.active_by_peer.remove(peer);
            }
        }
    }
}

impl AuthThrottleInner {
    fn prune(&mut self, now: Instant, failure_window: Duration) {
        self.peers.retain(|_, state| {
            if state
                .locked_until
                .is_some_and(|locked_until| locked_until > now)
            {
                return true;
            }
            state.failures > 0 && now.duration_since(state.first_failure) <= failure_window
        });
    }

    /// Make room for one more tracked peer.
    ///
    /// Called only after `prune`, so everything still here is live and
    /// something useful has to go. Peers serving an active lockout are
    /// evicted last: they are the entries doing real work, and dropping one
    /// hands a locked-out peer a way back in.
    ///
    /// Deadline alone is the wrong key for that — the default lockout (60s)
    /// is *shorter* than the failure window (5 min), so a locked peer expires
    /// sooner than a peer that failed once and would be evicted first. Order
    /// on locked-ness first, then soonest expiry within each group.
    ///
    /// A peer can still in principle evict its own lockout by pushing enough
    /// other lockouts through, but that costs `AUTH_MAX_TRACKED_PEERS` ×
    /// `max_failures` handshakes against a global limit of
    /// `AUTH_MAX_UNAUTHENTICATED` concurrent attempts, each with its own
    /// timeout. The concurrency cap is the brake there, not this map.
    fn make_room(&mut self, cap: usize, now: Instant, failure_window: Duration) {
        while self.peers.len() >= cap {
            let victim = self
                .peers
                .iter()
                .min_by_key(|(_, state)| {
                    let locked = state.locked_until.filter(|until| *until > now);
                    (
                        locked.is_some(),
                        locked.unwrap_or(state.first_failure + failure_window),
                    )
                })
                .map(|(peer, _)| peer.clone());
            match victim {
                Some(peer) => {
                    self.peers.remove(&peer);
                }
                None => break,
            }
        }
    }
}

impl AuthAttemptGuard {
    pub fn record_success(mut self) {
        self.throttle.record_success(&self.peer);
        self.release();
    }

    pub fn record_failure(mut self) {
        self.throttle.record_failure(&self.peer, false);
        self.release();
    }

    pub fn record_stalled(mut self) {
        self.throttle.record_failure(&self.peer, true);
        self.release();
    }

    fn release(&mut self) {
        if !self.released {
            self.released = true;
            self.throttle.release(&self.peer);
        }
    }
}

impl Drop for AuthAttemptGuard {
    fn drop(&mut self) {
        self.release();
    }
}

/// Wire response for a passphrase the server rejected. Clients treat it as
/// "this credential is wrong" and discard it.
pub const AUTH_REJECTED: &str = "auth";

/// Wire response for an attempt the throttle refused before it could be
/// checked — a peer lockout or the global concurrent-handshake cap. The
/// credential was never examined, so clients must keep it and retry rather
/// than dropping the user at the login screen.
pub const AUTH_BUSY: &str = "busy";

/// How one authentication attempt ended.
///
/// The distinction matters to the throttle: a passphrase mismatch and a peer
/// that holds its slot for the full deadline count against the failure budget.
/// A socket that goes away promptly — a page navigation, suspended tab, or
/// abandoned fallback probe — remains an ordinary reconnect and is not
/// charged (docs/design/net.md § service worker).
enum AuthOutcome {
    Accepted,
    Rejected,
    Busy,
    Stalled,
    Abandoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthVerification {
    Accepted,
    Rejected,
    Busy,
}

/// Verify one credential without letting remote guesses run CPU- and
/// memory-heavy Argon2 work on an async runtime worker. There is intentionally
/// no queue behind the strict semaphore: clients already understand `busy`
/// and can retry, while a queue would turn the concurrency bound into an
/// attacker-controlled memory backlog.
pub async fn verify_auth_passphrase(token: &AuthPassphrase, provided: &str) -> AuthVerification {
    if !token.is_argon2() {
        return if token.verify(provided) {
            AuthVerification::Accepted
        } else {
            AuthVerification::Rejected
        };
    }

    let Ok(permit) = AUTH_ARGON2_SLOTS.clone().try_acquire_owned() else {
        return AuthVerification::Busy;
    };
    let token = token.clone();
    let provided = provided.to_owned();
    let verification = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        token.verify(&provided)
    });
    match tokio::time::timeout(AUTH_ARGON2_VERIFY_TIMEOUT, verification).await {
        Ok(Ok(true)) => AuthVerification::Accepted,
        Ok(Ok(false)) => AuthVerification::Rejected,
        // A blocking task cannot safely be cancelled. Its moved-in permit
        // remains held until it exits, so timeouts and panics never let more
        // Argon2 jobs through than the configured bound.
        Ok(Err(_)) | Err(_) => AuthVerification::Busy,
    }
}

/// Authenticate a text WebSocket passphrase with timeout, active-connection
/// limiting, and failed-attempt backoff. Sends [`AUTH_REJECTED`] and closes on
/// a wrong passphrase, [`AUTH_BUSY`] when the throttle refused the attempt.
/// When ok_message is present, it is sent after a successful authentication
/// before returning.
pub async fn authenticate_text_ws(
    ws: &mut WebSocket,
    token: &AuthPassphrase,
    throttle: &AuthThrottle,
    peer: &str,
    ok_message: Option<&str>,
) -> bool {
    let Some(guard) = throttle.begin(peer.to_string()) else {
        let _ = ws.send(Message::Text(AUTH_BUSY.into())).await;
        let _ = ws.close().await;
        return false;
    };

    let outcome = tokio::time::timeout(AUTH_TIMEOUT, async {
        loop {
            match ws.recv().await {
                Some(Ok(Message::Text(pass))) => {
                    let pass = pass.trim();
                    break if pass.len() > AUTH_MAX_PASSPHRASE_BYTES {
                        AuthOutcome::Rejected
                    } else {
                        match verify_auth_passphrase(token, pass).await {
                            AuthVerification::Accepted => AuthOutcome::Accepted,
                            AuthVerification::Rejected => AuthOutcome::Rejected,
                            AuthVerification::Busy => AuthOutcome::Busy,
                        }
                    };
                }
                Some(Ok(Message::Ping(d))) => {
                    let _ = ws.send(Message::Pong(d)).await;
                }
                _ => break AuthOutcome::Abandoned,
            }
        }
    })
    .await
    // An idle/ping-only peer consumed a scarce admission slot for the whole
    // deadline. Charge that stall so it cannot immediately renew the slot
    // forever; ordinary sockets that close before the deadline remain
    // abandoned and do not penalize page navigation or transport fallback.
    .unwrap_or(AuthOutcome::Stalled);

    match outcome {
        AuthOutcome::Accepted => {
            guard.record_success();
            if let Some(msg) = ok_message
                && ws.send(Message::Text(msg.into())).await.is_err()
            {
                return false;
            }
            true
        }
        AuthOutcome::Rejected => {
            guard.record_failure();
            let _ = ws.send(Message::Text(AUTH_REJECTED.into())).await;
            let _ = ws.close().await;
            false
        }
        AuthOutcome::Busy => {
            drop(guard);
            let _ = ws.send(Message::Text(AUTH_BUSY.into())).await;
            let _ = ws.close().await;
            false
        }
        AuthOutcome::Stalled => {
            guard.record_stalled();
            let _ = ws.send(Message::Text(AUTH_BUSY.into())).await;
            let _ = ws.close().await;
            false
        }
        // Dropping the guard releases the handshake slot without touching the
        // peer's failure count.
        AuthOutcome::Abandoned => {
            drop(guard);
            let _ = ws.close().await;
            false
        }
    }
}

fn yas_config_dir() -> PathBuf {
    #[cfg(unix)]
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            PathBuf::from(home).join(".config")
        });
    #[cfg(windows)]
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"));
    base.join("yas")
}

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("YAS_CONFIG") {
        return PathBuf::from(p);
    }
    yas_config_dir().join("yas.conf")
}

pub fn remotes_path() -> PathBuf {
    if let Ok(p) = std::env::var("YAS_REMOTES") {
        return PathBuf::from(p);
    }
    yas_config_dir().join("yas.remotes")
}

/// Resolve the canonical native YAS server IPC socket path.
#[cfg(unix)]
pub fn default_local_socket() -> String {
    if let Ok(p) = std::env::var("YAS_SOCK") {
        return p;
    }
    let name = std::env::var("YAS_SERVER_NAME").unwrap_or_else(|_| "default".into());
    local_socket_for_name(&name)
}

/// Resolve the canonical native YAS server IPC socket path.
#[cfg(unix)]
pub fn default_yas_socket() -> String {
    if let Ok(path) = std::env::var("YAS_SOCK") {
        return path;
    }
    let name = std::env::var("YAS_SERVER_NAME").unwrap_or_else(|_| "default".into());
    yas_socket_for_name(&name)
}

/// Whether a server name is a portable socket, named-pipe, and path suffix.
/// Callers accepting names from configuration use this before passing them to
/// [`local_socket_for_name`].
pub fn valid_server_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 64
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return false;
    }
    let windows_stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    !matches!(windows_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !windows_stem
            .strip_prefix("COM")
            .or_else(|| windows_stem.strip_prefix("LPT"))
            .is_some_and(|number| number.len() == 1 && matches!(number.as_bytes()[0], b'1'..=b'9'))
}

/// Resolve one named native server endpoint.
#[cfg(unix)]
pub fn local_socket_for_name(name: &str) -> String {
    packaged_system_socket(name)
        .unwrap_or_else(|| crate::local_ipc::automatic_socket_path("yas", name))
}

#[cfg(unix)]
pub fn yas_socket_for_name(name: &str) -> String {
    local_socket_for_name(name)
}

/// Probe packaged system-service sockets only when every path component has
/// the expected owner and permissions. The NixOS module uses a private
/// per-user directory because the server now binds its own socket; the legacy
/// systemd socket unit places its PID-1-created listener directly below
/// `/run/yas`.
#[cfg(unix)]
fn packaged_system_socket(name: &str) -> Option<String> {
    let user = std::env::var("USER").ok()?;
    if !valid_server_name(&user) || !valid_server_name(name) {
        return None;
    }
    packaged_system_socket_from(
        std::path::Path::new("/run/yas"),
        &user,
        name,
        0,
        crate::local_ipc::effective_uid(),
    )
}

#[cfg(unix)]
fn packaged_system_socket_from(
    parent: &std::path::Path,
    user: &str,
    name: &str,
    system_uid: u32,
    client_uid: u32,
) -> Option<String> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let parent_metadata = std::fs::symlink_metadata(parent).ok()?;
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.uid() != system_uid
        || parent_metadata.mode() & 0o022 != 0
    {
        return None;
    }
    let private = parent.join(user);
    if let Ok(private_metadata) = std::fs::symlink_metadata(&private)
        && private_metadata.file_type().is_dir()
        && private_metadata.uid() == client_uid
        && private_metadata.mode() & 0o077 == 0
    {
        let path = private.join(format!("yas-{name}.sock"));
        if let Ok(metadata) = std::fs::symlink_metadata(&path)
            && metadata.file_type().is_socket()
            && metadata.uid() == client_uid
            && metadata.mode() & 0o077 == 0
        {
            return Some(path.to_string_lossy().into_owned());
        }
    }

    let legacy = parent.join(format!("{user}-{name}.sock"));
    let metadata = std::fs::symlink_metadata(&legacy).ok()?;
    (metadata.file_type().is_socket()
        && metadata.uid() == client_uid
        && metadata.mode() & 0o077 == 0)
        .then(|| legacy.to_string_lossy().into_owned())
}

/// Resolve the canonical native YAS server IPC pipe path (Windows).
#[cfg(windows)]
pub fn default_local_socket() -> String {
    if let Ok(p) = std::env::var("YAS_SOCK") {
        return p;
    }
    let name = std::env::var("YAS_SERVER_NAME").unwrap_or_else(|_| "default".into());
    local_socket_for_name(&name)
}

#[cfg(windows)]
pub fn default_yas_socket() -> String {
    if let Ok(path) = std::env::var("YAS_SOCK") {
        return path;
    }
    let name = std::env::var("YAS_SERVER_NAME").unwrap_or_else(|_| "default".into());
    yas_socket_for_name(&name)
}

#[cfg(windows)]
pub fn local_socket_for_name(name: &str) -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".into());
    format!(r"\\.\pipe\yas-{user}-{name}")
}

#[cfg(windows)]
pub fn yas_socket_for_name(name: &str) -> String {
    local_socket_for_name(name)
}

/// Acquire an exclusive cross-process lock for the config directory.
/// Returns a `File` whose lifetime holds the lock (released on drop).
/// On non-Unix platforms this is a no-op that returns `None`.
fn lock_config_dir() -> Option<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = yas_config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let lock_path = dir.join("yas.lock");
        if let Ok(f) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock_path)
        {
            // Block until we get the lock.
            use std::os::unix::io::AsRawFd;
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return Some(f);
            }
        }
        None
    }
    #[cfg(not(unix))]
    {
        None
    }
}

pub fn read_config() -> HashMap<String, String> {
    let path = config_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(e) => {
            eprintln!("yas: could not read {}: {e}", path.display());
            return HashMap::new();
        }
    };
    parse_config_str(&contents)
}

/// A single entry in `yas.remotes`. `disabled` entries are persisted as
/// `# name = uri` and are excluded from connection resolution but preserved
/// across restarts so users can re-enable them later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub uri: String,
    pub disabled: bool,
}

/// Read `yas.remotes` and return ordered enabled `(name, uri)` pairs.
/// If the file does not exist, provisions it with `local = local` (0600).
/// Disabled entries are filtered out — use [`read_remotes_full`] to keep them.
pub fn read_remotes() -> Vec<(String, String)> {
    read_remotes_full()
        .into_iter()
        .filter(|e| !e.disabled)
        .map(|e| (e.name, e.uri))
        .collect()
}

/// Read `yas.remotes` including disabled entries.
pub fn read_remotes_full() -> Vec<RemoteEntry> {
    let path = remotes_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let default = vec![RemoteEntry {
                name: "local".to_string(),
                uri: "local".to_string(),
                disabled: false,
            }];
            write_remotes(&default);
            return default;
        }
        Err(e) => {
            eprintln!("yas: could not read {}: {e}", path.display());
            return vec![];
        }
    };
    parse_remotes_full(&contents)
}

/// Atomically read-modify-write `yas.conf` under an exclusive flock.
pub fn modify_config(f: impl FnOnce(&mut HashMap<String, String>)) {
    let _lock = lock_config_dir();
    let mut map = read_config();
    f(&mut map);
    write_config(&map);
}

/// Atomically read-modify-write `yas.remotes` under an exclusive flock.
pub fn modify_remotes(f: impl FnOnce(&mut Vec<RemoteEntry>)) {
    let _lock = lock_config_dir();
    let mut entries = read_remotes_full();
    f(&mut entries);
    write_remotes(&entries);
}

/// Parse `yas.remotes` content into ordered enabled `(name, uri)` pairs.
/// Disabled entries (`# name = uri`) are filtered out — use
/// [`parse_remotes_full`] to keep them.
pub fn parse_remotes_str(contents: &str) -> Vec<(String, String)> {
    parse_remotes_full(contents)
        .into_iter()
        .filter(|e| !e.disabled)
        .map(|e| (e.name, e.uri))
        .collect()
}

/// Whether `name` can be an entry name in `yas.remotes` / `yas.roots`.
///
/// Every rule is forced by a format the name has to survive intact:
///
/// * the file is `name = value`, so an `=` reparses as the start of the
///   value, and a leading `#` reparses as the disabled marker — an entry
///   added as enabled would come back disabled;
/// * the config-socket verbs (`remotes-add <name> <uri>`) are
///   space-delimited, so any whitespace splits the name in two;
/// * a newline splits the line itself.
///
/// One function rather than a condition per caller. There were four, they had
/// drifted apart, and the parser's was the strictest — so `yas remote add
/// 'my remote' ssh:host` reported success, wrote the line, and the next read
/// dropped it without a word.
pub fn valid_entry_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('=')
        && !name.starts_with('#')
        && !name.contains(char::is_whitespace)
}

/// Parse `name = value` lines shared by `yas.remotes` and `yas.roots`.
/// Format: `name = value` for enabled; `# name = value` (optional whitespace
/// after `#`) for disabled. Blank lines and other `#` lines are ignored.
/// Duplicate names: last wins; first-seen order is preserved.
fn parse_kv_entries(contents: &str) -> Vec<(String, String, bool)> {
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, (String, String, bool)> = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (body, disabled) = if let Some(rest) = line.strip_prefix('#') {
            (rest.trim_start(), true)
        } else {
            (line, false)
        };
        let Some((k, v)) = body.split_once('=') else {
            continue;
        };
        let name = k.trim().to_string();
        let value = v.trim().to_string();
        // Names that cannot round-trip are never materialized. Writers
        // reject them up front (see `valid_entry_name`); this is the backstop
        // for a hand-edited file.
        if !valid_entry_name(&name) || value.is_empty() {
            continue;
        }
        if !map.contains_key(&name) {
            order.push(name.clone());
        }
        map.insert(name.clone(), (name, value, disabled));
    }
    order.into_iter().map(|k| map.remove(&k).unwrap()).collect()
}

/// Parse `yas.remotes` content including disabled entries.
/// Format: `name = uri` for enabled; `# name = uri` (with optional whitespace
/// after `#`) for disabled. Other `#` lines and blank lines are ignored.
/// Duplicate names: last wins (same as yas.conf).
pub fn parse_remotes_full(contents: &str) -> Vec<RemoteEntry> {
    parse_kv_entries(contents)
        .into_iter()
        .map(|(name, uri, disabled)| RemoteEntry {
            name,
            uri,
            disabled,
        })
        .collect()
}

/// The stored catalogue document: one `name = uri` per line, `#` for a
/// disabled entry. The same shape `roots` uses, and the one the KV key holds.
pub fn serialize_remotes(entries: &[RemoteEntry]) -> String {
    let mut out = String::new();
    for e in entries {
        if e.disabled {
            out.push_str("# ");
        }
        out.push_str(&e.name);
        out.push_str(" = ");
        out.push_str(&e.uri);
        out.push('\n');
    }
    out
}

/// Write `yas.remotes` atomically with mode 0o600 (owner read/write only).
pub fn write_remotes(entries: &[RemoteEntry]) {
    let path = remotes_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let contents = serialize_remotes(entries);
    write_secret_file(&path, &contents);
}

/// Write a file with mode 0o600 (owner-only).  On Unix this is done by
/// writing to a temp file with the right mode, then atomically renaming.
/// On Windows we just write normally (ACLs are handled separately if needed).
fn write_secret_file(path: &PathBuf, contents: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Write to a sibling temp file with a unique name (pid + counter)
        // so concurrent writers don't clobber each other's temp files.
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let tmp = path.with_extension(format!("tmp.{pid}.{seq}"));
        let result = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(contents.as_bytes())
            });
        if result.is_ok() {
            let _ = std::fs::rename(&tmp, path);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::write(path, contents);
    }
}

fn serialize_config_str(map: &HashMap<String, String>) -> String {
    let mut lines: Vec<String> = map.iter().map(|(k, v)| format!("{k} = {v}")).collect();
    lines.sort();
    lines.push(String::new());
    lines.join("\n")
}

pub fn write_config(map: &HashMap<String, String>) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_secret_file(&path, &serialize_config_str(map));
}

/// Watches a single file in its parent directory and calls `on_change`
/// whenever the file is modified.  Skips access (read) events.
fn spawn_file_watcher<F>(path: PathBuf, label: &'static str, on_change: F)
where
    F: Fn() + Send + 'static,
{
    use notify::{RecursiveMode, Watcher};

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let watch_dir = path.parent().unwrap_or(&path).to_path_buf();
    let file_name = path.file_name().map(|n| n.to_os_string());

    std::thread::Builder::new()
        .name(format!("{label}-watcher"))
        .spawn(move || {
            // A config rewrite is level-triggered: one queued notification is
            // enough because the callback rereads the current file. Coalesce
            // bursts instead of retaining an unbounded notify backlog.
            let (ntx, nrx) = std::sync::mpsc::sync_channel(1);
            let mut watcher = match notify::recommended_watcher(move |event| {
                let _ = ntx.try_send(event);
            }) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("yas: {label} watcher failed: {e}");
                    return;
                }
            };
            if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
                eprintln!("yas: {label} watch failed: {e}");
                return;
            }
            loop {
                match nrx.recv() {
                    Ok(Ok(event)) => {
                        if matches!(event.kind, notify::EventKind::Access(_)) {
                            continue;
                        }
                        let matches = file_name.as_ref().is_none_or(|name| {
                            event.paths.iter().any(|p| p.file_name() == Some(name))
                        });
                        if matches {
                            on_change();
                        }
                    }
                    Ok(Err(_)) => continue,
                    Err(_) => break,
                }
            }
        })
        .expect("failed to spawn file-watcher thread");
}

// ---------------------------------------------------------------------------
// RemotesState — live-reloading yas.remotes with 0o600 permissions
// ---------------------------------------------------------------------------

/// Manages `yas.remotes`: reads/writes the file, watches for external
/// changes, and broadcasts the serialised contents to all subscribers.
///
/// The broadcast value is the raw file text (same as what `read_remotes`
/// would parse), sent as a single string so receivers can re-parse it.
#[derive(Clone)]
pub struct RemotesState {
    inner: Arc<RemotesInner>,
}

struct RemotesInner {
    /// Cached current contents (raw file text, normalized).
    contents: RwLock<String>,
    tx: broadcast::Sender<String>,

    /// False in ephemeral mode: `set` broadcasts but never touches disk.
    /// Without this, "no file I/O" was only true until the first write —
    /// which let a `yas open` session's temporary destination list
    /// overwrite the user's real config, and let the unit tests clobber
    /// `~/.config/yas/` on every `cargo test`.
    persist: bool,
}

impl RemotesState {
    /// Full persistent mode: reads `yas.remotes`, watches it for changes.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        let inner = Arc::new(RemotesInner {
            contents: RwLock::new(serialize_remotes(&read_remotes_full())),
            tx,
            persist: true,
        });
        let watcher_inner = inner.clone();
        spawn_file_watcher(remotes_path(), "remotes", move || {
            // Read directly — do not auto-provision. The file may be
            // intentionally empty (user removed all remotes).
            let text = std::fs::read_to_string(remotes_path()).unwrap_or_default();
            *watcher_inner.contents.write().unwrap() = text.clone();
            let _ = watcher_inner.tx.send(text);
        });
        Self { inner }
    }

    /// Ephemeral mode: starts with the given text, no file I/O, no watcher.
    /// Used by `yas open` to advertise the session's destinations to the
    /// browser without touching `yas.remotes`.
    pub fn ephemeral(initial: String) -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            inner: Arc::new(RemotesInner {
                contents: RwLock::new(initial),
                tx,
                persist: false,
            }),
        }
    }

    /// Returns the current serialized remotes contents.
    pub fn get(&self) -> String {
        self.inner.contents.read().unwrap().clone()
    }

    /// Overwrite `yas.remotes` with `entries` and broadcast the change.
    pub fn set(&self, entries: &[RemoteEntry]) {
        if self.inner.persist {
            write_remotes(entries);
        }
        let text = serialize_remotes(entries);
        *self.inner.contents.write().unwrap() = text.clone();
        let _ = self.inner.tx.send(text);
    }

    /// Atomically read-modify-write `yas.remotes` under an exclusive flock,
    /// then update the in-memory cache and broadcast.
    pub fn modify(&self, f: impl FnOnce(&mut Vec<RemoteEntry>)) {
        let _lock = lock_config_dir();
        let mut entries = parse_remotes_full(&self.get());
        f(&mut entries);
        self.set(&entries);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.inner.tx.subscribe()
    }
}

impl Default for RemotesState {
    fn default() -> Self {
        Self::new()
    }
}

/// One `yas.forwards` entry: a name and a port-forward spec
/// (docs/design/net.md § A named list). `yas.conf` is a flat key→value map
/// and cannot hold an ordered list, so a list of named things gets its own
/// file. `disabled`
/// entries are persisted as `# name = spec` and skipped by
/// `yas forward --all` but preserved for re-enabling. Never
/// auto-provisioned — an absent file means no declared forwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardEntry {
    pub name: String,
    pub spec: String,
    pub disabled: bool,
}

pub fn forwards_path() -> PathBuf {
    if let Ok(p) = std::env::var("YAS_FORWARDS") {
        return PathBuf::from(p);
    }
    yas_config_dir().join("yas.forwards")
}

/// Parse `yas.forwards` content including disabled entries.
pub fn parse_forwards_full(contents: &str) -> Vec<ForwardEntry> {
    parse_kv_entries(contents)
        .into_iter()
        .map(|(name, spec, disabled)| ForwardEntry {
            name,
            spec,
            disabled,
        })
        .collect()
}

/// Read `yas.forwards` including disabled entries; empty if absent.
pub fn read_forwards_full() -> Vec<ForwardEntry> {
    match std::fs::read_to_string(forwards_path()) {
        Ok(c) => parse_forwards_full(&c),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => {
            eprintln!("yas: could not read {}: {e}", forwards_path().display());
            vec![]
        }
    }
}

fn serialize_forwards(entries: &[ForwardEntry]) -> String {
    let mut out = String::new();
    for e in entries {
        if e.disabled {
            out.push_str("# ");
        }
        out.push_str(&e.name);
        out.push_str(" = ");
        out.push_str(&e.spec);
        out.push('\n');
    }
    out
}

/// Write `yas.forwards` atomically with mode 0o600.
pub fn write_forwards(entries: &[ForwardEntry]) {
    let path = forwards_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_secret_file(&path, &serialize_forwards(entries));
}

/// Atomically read-modify-write `yas.forwards` under an exclusive flock.
pub fn modify_forwards(f: impl FnOnce(&mut Vec<ForwardEntry>)) {
    let _lock = lock_config_dir();
    let mut entries = read_forwards_full();
    f(&mut entries);
    write_forwards(&entries);
}

fn parse_config_str(contents: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn packaged_system_socket_prefers_private_user_directory() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let private = root.path().join("alice");
        std::fs::create_dir(&private).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = private.join("yas-default.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o700)).unwrap();
        let uid = crate::local_ipc::effective_uid();

        assert_eq!(
            packaged_system_socket_from(root.path(), "alice", "default", uid, uid),
            Some(socket.to_string_lossy().into_owned())
        );

        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            packaged_system_socket_from(root.path(), "alice", "default", uid, uid),
            None
        );
    }

    // ── parse_config_str ──

    #[test]
    fn parse_empty_string() {
        let map = parse_config_str("");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_comments_and_blanks() {
        let map = parse_config_str("# comment\n\n  # another\n");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_key_value() {
        let map = parse_config_str("font = Menlo\ntheme = dark\n");
        assert_eq!(map.get("font").unwrap(), "Menlo");
        assert_eq!(map.get("theme").unwrap(), "dark");
    }

    #[test]
    fn parse_trims_whitespace() {
        let map = parse_config_str("  key  =  value  ");
        assert_eq!(map.get("key").unwrap(), "value");
    }

    #[test]
    fn parse_line_without_equals() {
        let map = parse_config_str("no-equals-here\nkey=val");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("key").unwrap(), "val");
    }

    #[test]
    fn parse_equals_in_value() {
        let map = parse_config_str("cmd = a=b=c");
        assert_eq!(map.get("cmd").unwrap(), "a=b=c");
    }

    #[test]
    fn parse_duplicate_keys_last_wins() {
        let map = parse_config_str("key = first\nkey = second");
        assert_eq!(map.get("key").unwrap(), "second");
    }

    #[test]
    fn parse_mixed_content() {
        let input = "# header\nfont = FiraCode\n\n# size\nsize = 14\ntheme=light";
        let map = parse_config_str(input);
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("font").unwrap(), "FiraCode");
        assert_eq!(map.get("size").unwrap(), "14");
        assert_eq!(map.get("theme").unwrap(), "light");
    }

    // ── write_config round-trip ──

    #[test]
    fn serialize_config_produces_sorted_output() {
        let mut map: HashMap<String, String> = HashMap::new();
        map.insert("z".into(), "last".into());
        map.insert("a".into(), "first".into());
        let output = serialize_config_str(&map);
        assert!(output.starts_with("a = first"));
        assert!(output.contains("z = last"));
    }

    #[test]
    fn round_trip_parse_serialize() {
        let input = "alpha = 1\nbeta = 2\ngamma = 3";
        let map = parse_config_str(input);
        let serialized = serialize_config_str(&map);
        let reparsed = parse_config_str(&serialized);
        assert_eq!(map, reparsed);
    }

    // ── RemotesState mutations (remotes-add / remotes-remove) ──

    fn entry(name: &str, uri: &str) -> RemoteEntry {
        RemoteEntry {
            name: name.to_string(),
            uri: uri.to_string(),
            disabled: false,
        }
    }

    #[test]
    fn remotes_add_new_entry() {
        let state = RemotesState::ephemeral(String::new());
        let mut entries = parse_remotes_full(&state.get());
        entries.push(entry("rabbit", "ssh:rabbit"));
        state.set(&entries);
        let got = parse_remotes_str(&state.get());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], ("rabbit".to_string(), "ssh:rabbit".to_string()));
    }

    #[test]
    fn remotes_add_updates_existing() {
        let initial = "rabbit = ssh:rabbit\n";
        let state = RemotesState::ephemeral(initial.to_string());
        let mut entries = parse_remotes_full(&state.get());
        if let Some(pos) = entries.iter().position(|e| e.name == "rabbit") {
            entries[pos].uri = "tcp:rabbit:3264".to_string();
        }
        state.set(&entries);
        let got = parse_remotes_str(&state.get());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "tcp:rabbit:3264");
    }

    #[test]
    fn remotes_remove_existing() {
        let initial = "rabbit = ssh:rabbit\nhound = ssh:hound\n";
        let state = RemotesState::ephemeral(initial.to_string());
        let mut entries = parse_remotes_full(&state.get());
        entries.retain(|e| e.name != "rabbit");
        state.set(&entries);
        let got = parse_remotes_str(&state.get());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "hound");
    }

    #[test]
    fn remotes_remove_nonexistent_is_noop() {
        let initial = "rabbit = ssh:rabbit\n";
        let state = RemotesState::ephemeral(initial.to_string());
        let mut entries = parse_remotes_full(&state.get());
        let before = entries.len();
        entries.retain(|e| e.name != "does-not-exist");
        assert_eq!(entries.len(), before);
    }

    // ── Disabled remotes (commented) ──

    #[test]
    fn parse_disabled_entry() {
        let entries = parse_remotes_full("# rabbit = ssh:rabbit\nhound = ssh:hound\n");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "rabbit");
        assert_eq!(entries[0].uri, "ssh:rabbit");
        assert!(entries[0].disabled);
        assert_eq!(entries[1].name, "hound");
        assert!(!entries[1].disabled);
    }

    #[test]
    fn parse_disabled_no_space_after_hash() {
        let entries = parse_remotes_full("#rabbit = ssh:rabbit\n");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].disabled);
    }

    #[test]
    fn parse_remotes_str_filters_disabled() {
        let active = parse_remotes_str("# rabbit = ssh:rabbit\nhound = ssh:hound\n");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].0, "hound");
    }

    #[test]
    fn parse_skips_pure_comments() {
        let entries = parse_remotes_full("# This is just a header\n# also a comment\n");
        assert!(entries.is_empty());
    }

    #[test]
    fn round_trip_disabled() {
        let initial = "rabbit = ssh:rabbit\n# hound = ssh:hound\n";
        let entries = parse_remotes_full(initial);
        let serialized = serialize_remotes(&entries);
        let reparsed = parse_remotes_full(&serialized);
        assert_eq!(entries, reparsed);
        assert!(serialized.contains("# hound = ssh:hound"));
    }

    #[test]
    fn remotes_toggle_flips_state() {
        let state = RemotesState::ephemeral("rabbit = ssh:rabbit\n".into());
        state.modify(|entries| {
            if let Some(pos) = entries.iter().position(|e| e.name == "rabbit") {
                entries[pos].disabled = !entries[pos].disabled;
            }
        });
        let entries = parse_remotes_full(&state.get());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].disabled);
        // Active view excludes it.
        assert!(parse_remotes_str(&state.get()).is_empty());
    }

    #[test]
    fn remotes_add_reenables_disabled() {
        let state = RemotesState::ephemeral("# rabbit = ssh:old\n".into());
        // Simulate the WS handler's add logic.
        state.modify(|entries| {
            let name = "rabbit".to_string();
            if let Some(pos) = entries.iter().position(|e| e.name == name) {
                entries[pos].uri = "ssh:new".to_string();
                entries[pos].disabled = false;
            } else {
                entries.push(RemoteEntry {
                    name,
                    uri: "ssh:new".to_string(),
                    disabled: false,
                });
            }
        });
        let entries = parse_remotes_full(&state.get());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uri, "ssh:new");
        assert!(!entries[0].disabled);
    }

    #[test]
    fn remotes_reorder_preserves_disabled() {
        let initial = "alpha = a\n# beta = b\ngamma = c\n";
        let entries = parse_remotes_full(initial);
        // Reorder alpha → gamma → beta.
        let desired = ["gamma", "alpha", "beta"];
        let by_name: std::collections::HashMap<String, RemoteEntry> = entries
            .iter()
            .map(|e| (e.name.clone(), e.clone()))
            .collect();
        let reordered: Vec<RemoteEntry> = desired
            .iter()
            .filter_map(|n| by_name.get(*n).cloned())
            .collect();
        let serialized = serialize_remotes(&reordered);
        let reparsed = parse_remotes_full(&serialized);
        assert_eq!(reparsed.len(), 3);
        assert_eq!(reparsed[0].name, "gamma");
        assert!(!reparsed[0].disabled);
        assert_eq!(reparsed[2].name, "beta");
        assert!(reparsed[2].disabled);
    }

    /// The rule every writer and the parser now share. These used to be four
    /// separate conditions that had drifted — and the tests here asserted the
    /// condition inline rather than calling anything, so they passed whatever
    /// the code did.
    #[test]
    fn entry_names_must_survive_both_formats() {
        for ok in ["rabbit", "prod-1", "a", "héllo", "x.y_z:1"] {
            assert!(valid_entry_name(ok), "{ok:?} should be usable");
        }
        for bad in [
            "",           // nothing to name
            "foo=bar",    // reparses as name "foo", value "bar"
            "#foo",       // reparses as a disabled entry
            "my remote",  // splits the space-delimited add verb
            "my\tremote", // ditto, and survives split_once(' ')
            "my\nremote", // splits the line
            " lead",
            "trail ",
        ] {
            assert!(!valid_entry_name(bad), "{bad:?} should be refused");
        }
    }

    /// The parser is the backstop for a hand-edited file: a name it would
    /// refuse must not come back as an entry.
    #[test]
    fn parser_drops_names_it_could_not_write() {
        let parsed =
            parse_remotes_full("good = ssh:host\nmy remote = ssh:other\n##bad = ssh:third\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "good");
    }

    // ── set-default writes yas.target key to yas.conf ──

    #[test]
    fn set_default_inserts_target_key() {
        let mut map = parse_config_str("font = Mono\n");
        map.insert("yas.target".into(), "rabbit".into());
        let serialized = serialize_config_str(&map);
        let reparsed = parse_config_str(&serialized);
        assert_eq!(
            reparsed.get("yas.target").map(|s| s.as_str()),
            Some("rabbit")
        );
        assert_eq!(reparsed.get("font").map(|s| s.as_str()), Some("Mono"));
    }

    #[test]
    fn set_default_local_removes_target_key() {
        let mut map = parse_config_str("yas.target = rabbit\nfont = Mono\n");
        // "local" or empty → remove the key
        map.remove("yas.target");
        let serialized = serialize_config_str(&map);
        let reparsed = parse_config_str(&serialized);
        assert!(!reparsed.contains_key("yas.target"));
        assert_eq!(reparsed.get("font").map(|s| s.as_str()), Some("Mono"));
    }
    #[test]
    fn auth_throttle_limits_concurrent_unauthenticated_attempts() {
        let throttle =
            AuthThrottle::with_limits(1, 5, Duration::from_secs(60), Duration::from_secs(60));
        let first = throttle.begin("peer").expect("first attempt allowed");
        assert!(throttle.begin("other").is_none());
        drop(first);
        assert!(throttle.begin("other").is_some());
    }

    #[test]
    fn auth_throttle_limits_each_peer_without_consuming_global_capacity() {
        let throttle =
            AuthThrottle::with_limits(8, 5, Duration::from_secs(60), Duration::from_secs(60));
        let first = throttle.begin("peer").expect("first peer attempt");
        let second = throttle.begin("peer").expect("second peer attempt");
        assert!(
            throttle.begin("peer").is_none(),
            "one peer must not monopolize the global pool"
        );
        let other = throttle
            .begin("other")
            .expect("another peer still has capacity");
        drop((first, second, other));
        assert!(throttle.begin("peer").is_some(), "peer slots were released");
    }

    #[test]
    fn auth_throttle_locks_out_repeated_failures_and_clears_on_success() {
        let throttle =
            AuthThrottle::with_limits(4, 2, Duration::from_secs(60), Duration::from_secs(60));
        throttle.begin("peer").unwrap().record_failure();
        let success = throttle.begin("peer").expect("not locked before threshold");
        success.record_success();
        throttle.begin("peer").unwrap().record_failure();
        assert!(
            throttle.begin("peer").is_some(),
            "success reset failure count"
        );
        throttle.begin("bad").unwrap().record_failure();
        throttle.begin("bad").unwrap().record_failure();
        assert!(throttle.begin("bad").is_none(), "bad peer is locked out");
        assert!(throttle.begin("other").is_some(), "lockout is per peer");
    }

    /// The peer key is the remote address, so it is attacker-chosen: a source
    /// with a large address range gets a fresh key per attempt and used to
    /// grow this map for a whole failure window.
    #[test]
    fn auth_throttle_bounds_the_peer_table() {
        let cap = 8;
        let throttle = AuthThrottle::with_limits_and_capacity(
            1024,
            2,
            Duration::from_secs(600),
            Duration::from_secs(600),
            cap,
        );
        for i in 0..cap * 20 {
            throttle
                .begin(format!("peer-{i}"))
                .expect("distinct peers are not locked")
                .record_failure();
        }
        let tracked = throttle.inner.lock().unwrap().peers.len();
        assert!(tracked <= cap, "peer table grew to {tracked}, cap {cap}");
    }

    /// A peer serving an active lockout outranks peers that merely failed
    /// once, or flooding the table would be a way to clear your own lockout.
    ///
    /// Uses the real shape of the defaults — lockout *shorter* than the
    /// failure window — because that is what makes this non-obvious: ordering
    /// on expiry alone evicts the locked peer first, since it expires sooner
    /// than a peer that failed once five minutes ago.
    #[test]
    fn auth_throttle_evicts_single_failures_before_active_lockouts() {
        let cap = 4;
        let throttle = AuthThrottle::with_limits_and_capacity(
            1024,
            2,
            Duration::from_secs(300),
            Duration::from_secs(60),
            cap,
        );
        throttle.begin("bad").unwrap().record_failure();
        throttle.begin("bad").unwrap().record_failure();
        assert!(throttle.begin("bad").is_none(), "locked out to begin with");

        for i in 0..cap * 20 {
            throttle
                .begin(format!("filler-{i}"))
                .expect("filler peers are not locked")
                .record_failure();
        }
        assert!(
            throttle.begin("bad").is_none(),
            "flooding the table must not clear an active lockout"
        );
    }

    /// An abandoned handshake — a page navigation, a suspended tab, or a
    /// client that disconnects before sending credentials — used to be
    /// charged as a failed authentication. Enough of them locked out a user who
    /// never typed a wrong passphrase, and the lockout then answered with the
    /// same "auth" the UI takes as "discard your stored passphrase".
    #[test]
    fn auth_throttle_ignores_handshakes_that_never_presented_a_passphrase() {
        let throttle =
            AuthThrottle::with_limits(32, 3, Duration::from_secs(60), Duration::from_secs(60));
        for _ in 0..10 {
            drop(throttle.begin("peer").expect("abandoned attempt allowed"));
        }
        assert!(
            throttle.begin("peer").is_some(),
            "abandoned handshakes must not count towards the failure budget"
        );
    }

    #[test]
    fn auth_throttle_charges_stalled_handshakes() {
        let throttle =
            AuthThrottle::with_limits(8, 2, Duration::from_secs(60), Duration::from_secs(60));
        throttle.begin("peer").unwrap().record_stalled();
        throttle.begin("peer").unwrap().record_stalled();
        assert!(
            throttle.begin("peer").is_none(),
            "a peer must not renew timed-out admission slots forever"
        );
    }

    #[test]
    fn network_argon2_verification_runs_off_runtime_and_matches() {
        let hash = crate::passphrase::hash("secret").unwrap();
        let auth = AuthPassphrase::argon2(hash);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        assert!(matches!(
            runtime.block_on(verify_auth_passphrase(&auth, "secret")),
            AuthVerification::Accepted
        ));
        assert!(matches!(
            runtime.block_on(verify_auth_passphrase(&auth, "wrong")),
            AuthVerification::Rejected
        ));
    }

    #[test]
    fn auth_throttle_releases_a_slot_exactly_once() {
        let throttle =
            AuthThrottle::with_limits(1, 5, Duration::from_secs(60), Duration::from_secs(60));
        // record_failure() releases, and the subsequent Drop must not release a
        // second time — a double decrement would let the cap drift upwards.
        throttle.begin("peer").unwrap().record_failure();
        let held = throttle.begin("other").expect("slot freed once");
        assert!(throttle.begin("third").is_none(), "cap still holds at one");
        drop(held);
    }
}
