use crate::events::EventType;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::BuildHasher;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex, Notify, mpsc, watch};
use yas_compositor::{CompositorCommand, CompositorEvent, CompositorHandle};
use yas_terminal_driver::{
    SearchResult as AlacrittySearchResult, SeqText, TerminalDriver as AlacrittyDriver,
};
#[cfg(test)]
use yas_terminal_model::MAX_CELL_COUNT;
use yas_terminal_model::{
    DEADLINE_STOP_GRACE, EXIT_REASON_DEADLINE, EXIT_REASON_NORMAL, FrameState, TERM_CWD_MAX,
};
const CODEC_SUPPORT_H264: u8 = 1 << 0;
const CODEC_SUPPORT_AV1: u8 = 1 << 1;
const SURFACE_FRAME_FLAG_KEYFRAME: u8 = 1 << 0;
const SURFACE_FRAME_CODEC_MASK: u8 = 0b110;
const SURFACE_FRAME_CODEC_H264: u8 = 0;
const SURFACE_FRAME_CODEC_AV1: u8 = 1 << 1;
const REMOTE_INPUT_POINTER: u8 = 0;
const REMOTE_INPUT_TOUCH: u8 = 1;

mod app_env;
#[cfg(target_os = "linux")]
mod audio;
#[cfg(target_os = "linux")]
mod audio_pw;
mod capacity_diagnostics;
mod channel;
mod composite_link;
#[cfg(target_os = "linux")]
mod desktop_bus;
mod events;
mod extension;
pub mod extension_catalog;
pub mod extension_store;
mod font;
mod gpu_libs;
mod ipc;
mod journal;
mod kv;
#[cfg(target_os = "linux")]
mod media_input;
mod media_policy;
mod net;
#[cfg(target_os = "linux")]
mod nvdec_decode;
mod nvenc_encode;
#[cfg(any(unix, windows))]
mod process;
mod pty;
mod relay;
mod server_name;
#[cfg(target_os = "linux")]
mod software_decode;
mod surface_encoder;
pub mod thread_name;
#[cfg(target_os = "linux")]
mod vaapi_decode;
#[cfg(target_os = "linux")]
mod vaapi_encode;
#[cfg(target_os = "linux")]
mod video_decode;
#[cfg(target_os = "linux")]
mod video_decode_vulkan;
#[cfg(target_os = "linux")]
mod xwayland;
mod yas;
mod yas_events;
mod yas_extension;
mod yas_fs;
#[path = "yas_git.rs"]
mod yas_git_adapter;
#[path = "yas_lsp.rs"]
mod yas_lsp_adapter;
mod yas_process;
mod yas_shutdown;
mod yas_surface_backend;
mod yas_terminal_backend;
mod yas_terminal_queries;

pub use ipc::{
    IpcListener, IpcStream, default_ipc_path, default_ipc_path_for, default_ipc_path_template,
};
pub use media_policy::MediaCodecPolicy;
use pty::{PtyHandle, PtyWriteTarget};
pub use server_name::ServerName;
pub use surface_encoder::ChromaSubsampling;
use surface_encoder::SurfaceEncoder;
pub use surface_encoder::SurfaceEncoderPreference;
pub use surface_encoder::{SurfaceBandwidth, SurfaceEncoding, SurfaceSpeed};

type PtyFds = Arc<std::sync::RwLock<FxHashMap<u16, PtyWriteTarget>>>;

/// Conditional event emission. `$payload` is evaluated only while the event's
/// activation bit is set, which is the invariant that keeps full byte capture
/// free when operators leave the low-throughput default enabled.
macro_rules! yas_event {
    ($log:expr, $kind:expr) => {{
        let log = &$log;
        let kind = $kind;
        if log.enabled(kind) {
            log.record(kind, 0, &[]);
        }
    }};
    ($log:expr, $kind:expr, $payload:expr) => {{
        let log = &$log;
        let kind = $kind;
        if log.enabled(kind) {
            let payload = $payload;
            log.record(kind, 0, &payload);
        }
    }};
}

tokio::task_local! {
    static EVENT_WRITE_CONTEXT: (Arc<events::EventLog>, Arc<AtomicU64>);
}

/// Command-line overrides for extension and channel deployment policy.
///
/// The CLI fills this without mutating the process environment. Calling
/// [`configure_deployment`] merges it over the corresponding environment
/// variables and freezes the result for the lifetime of the server process.
#[derive(Clone, Debug, Default)]
pub struct DeploymentOverrides {
    extensions_disabled: bool,
    channels_disabled: bool,
    values: HashMap<&'static str, u64>,
}

impl DeploymentOverrides {
    pub fn disable_extensions(&mut self) {
        self.extensions_disabled = true;
    }

    pub fn disable_channels(&mut self) {
        self.channels_disabled = true;
    }

    /// Override one documented extension/channel server setting.
    pub fn set(&mut self, name: &'static str, value: u64) -> Result<(), String> {
        if !DEPLOYMENT_SETTING_NAMES.contains(&name) {
            return Err(format!("unknown deployment setting {name}"));
        }
        self.values.insert(name, value);
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct DeploymentSettings {
    extensions_enabled: bool,
    channels_enabled: bool,
    values: HashMap<&'static str, u64>,
}

const DEPLOYMENT_SETTING_NAMES: &[&str] = &[
    "YAS_EXT_MAX_RUNNING",
    "YAS_EXT_MAX_PERSISTENT",
    "YAS_EXT_MAX_TRANSIENT",
    "YAS_EXT_FOLLOW_MAX_PER_CLIENT",
    "YAS_EXT_FOLLOW_MAX",
    "YAS_EXT_ARGUMENT_STORE_MAX",
    "YAS_EXT_MODULE_MAX",
    "YAS_EXT_OBJECT_CACHE_MAX",
    "YAS_EXT_OBJECT_CACHE_MAX_ENTRIES",
    "YAS_EXT_UPLOAD_MAX_PER_CLIENT",
    "YAS_EXT_UPLOAD_MAX_ACTIVE",
    "YAS_EXT_UPLOAD_TIMEOUT",
    "YAS_EXT_PENDING_TIMEOUT",
    "YAS_EXT_MAX_VALIDATING",
    "YAS_EXT_MEMORY_MAX",
    "YAS_EXT_OUTBOX_MAX",
    "YAS_EXT_OUTBOX_MESSAGES_MAX",
    "YAS_EXT_OUTBOX_TIMEOUT",
    "YAS_EXT_JOB_MAX_PER_CLIENT",
    "YAS_EXT_JOB_MAX",
    "YAS_EXT_JOB_PENDING_MAX_PER_CLIENT",
    "YAS_EXT_JOB_PENDING_MAX",
    "YAS_EXT_JOB_BYTES_MAX_PER_CLIENT",
    "YAS_EXT_JOB_BYTES_MAX",
    "YAS_EXT_OUTPUT_RETAIN_MAX",
    "YAS_EXT_TERMINAL_RETAIN",
    "YAS_EXT_COMMAND_STORE_MAX",
    "YAS_EXT_COMMAND_SNAPSHOT_MAX",
    "YAS_EXT_TABLE_ELEMENTS_MAX",
    "YAS_EXT_VALUE_STACK_MAX",
    "YAS_EXT_CALL_DEPTH_MAX",
    "YAS_EXT_STACK_SIZE",
    "YAS_EXT_FUEL_SLICE",
    "YAS_CHANNEL_MAX_LISTEN_PER_CLIENT",
    "YAS_CHANNEL_MAX_LISTENERS",
    "YAS_CHANNEL_MAX_PER_CLIENT",
    "YAS_CHANNEL_MAX_CONNECTED",
    "YAS_CHANNEL_BUFFER_MAX",
];

const U64_DEPLOYMENT_SETTINGS: &[&str] = &[
    "YAS_EXT_MODULE_MAX",
    "YAS_EXT_OBJECT_CACHE_MAX",
    "YAS_EXT_UPLOAD_TIMEOUT",
    "YAS_EXT_PENDING_TIMEOUT",
    "YAS_EXT_OUTBOX_TIMEOUT",
    "YAS_EXT_TERMINAL_RETAIN",
    "YAS_EXT_FUEL_SLICE",
    "YAS_CHANNEL_BUFFER_MAX",
];

static DEPLOYMENT_SETTINGS: std::sync::OnceLock<DeploymentSettings> = std::sync::OnceLock::new();

impl DeploymentSettings {
    fn resolve(overrides: DeploymentOverrides) -> Result<Self, String> {
        Self::resolve_with(overrides, |name| {
            let Some(value) = std::env::var_os(name) else {
                return Ok(None);
            };
            value
                .into_string()
                .map(Some)
                .map_err(|_| format!("{name} must be valid UTF-8"))
        })
    }

    fn resolve_with<F>(overrides: DeploymentOverrides, read_env: F) -> Result<Self, String>
    where
        F: Fn(&str) -> Result<Option<String>, String>,
    {
        let mut values = HashMap::with_capacity(DEPLOYMENT_SETTING_NAMES.len());
        for &name in DEPLOYMENT_SETTING_NAMES {
            let value = if let Some(value) = overrides.values.get(name) {
                Some(*value)
            } else {
                read_env(name)?
                    .map(|value| {
                        value
                            .parse::<u64>()
                            .map_err(|_| format!("{name} must be a non-negative integer"))
                    })
                    .transpose()?
            };
            let Some(value) = value else {
                continue;
            };
            if !U64_DEPLOYMENT_SETTINGS.contains(&name) && usize::try_from(value).is_err() {
                return Err(format!("{name} exceeds this platform's usize range"));
            }
            values.insert(name, value);
        }

        if let Some(max_running) = values.get("YAS_EXT_MAX_RUNNING")
            && !(1..=4).contains(max_running)
        {
            return Err("YAS_EXT_MAX_RUNNING must be in 1..=4".to_owned());
        }
        if let Some(module_max) = values.get("YAS_EXT_MODULE_MAX")
            && !(1..=64 * 1024 * 1024).contains(module_max)
        {
            return Err("YAS_EXT_MODULE_MAX must be in 1..=64 MiB".to_owned());
        }
        if let Some(outbox_max) = values.get("YAS_EXT_OUTBOX_MAX")
            && *outbox_max != u64::from(yas_wire::frame::HARD_MAX_DECODED_FRAME)
        {
            return Err(format!(
                "YAS_EXT_OUTBOX_MAX must equal the {}-byte logical-message ceiling",
                yas_wire::frame::HARD_MAX_DECODED_FRAME
            ));
        }
        for name in [
            "YAS_EXT_OUTBOX_MESSAGES_MAX",
            "YAS_EXT_OUTBOX_TIMEOUT",
            "YAS_EXT_MAX_VALIDATING",
            "YAS_EXT_MEMORY_MAX",
            "YAS_EXT_TABLE_ELEMENTS_MAX",
            "YAS_EXT_VALUE_STACK_MAX",
            "YAS_EXT_CALL_DEPTH_MAX",
            "YAS_EXT_STACK_SIZE",
            "YAS_EXT_FUEL_SLICE",
        ] {
            if values.get(name) == Some(&0) {
                return Err(format!("{name} must be positive"));
            }
        }

        let extensions_enabled = !overrides.extensions_disabled
            && !read_env("YAS_EXT")?.is_some_and(|value| value == "0");
        let channels_enabled = !overrides.channels_disabled
            && !read_env("YAS_CHANNEL")?.is_some_and(|value| value == "0");
        Ok(Self {
            extensions_enabled,
            channels_enabled,
            values,
        })
    }
}

/// Resolve deployment settings once, with command-line values taking
/// precedence over their environment equivalents.
pub fn configure_deployment(overrides: DeploymentOverrides) -> Result<(), String> {
    let settings = DeploymentSettings::resolve(overrides)?;
    DEPLOYMENT_SETTINGS
        .set(settings)
        .map_err(|_| "server deployment settings were already configured".to_owned())
}

fn ensure_deployment_settings() -> &'static DeploymentSettings {
    if let Some(settings) = DEPLOYMENT_SETTINGS.get() {
        return settings;
    }
    let settings = DeploymentSettings::resolve(DeploymentOverrides::default())
        .unwrap_or_else(|error| panic!("invalid server deployment configuration: {error}"));
    let _ = DEPLOYMENT_SETTINGS.set(settings);
    DEPLOYMENT_SETTINGS
        .get()
        .expect("deployment settings were just initialized")
}

pub(crate) fn extensions_enabled() -> bool {
    DEPLOYMENT_SETTINGS
        .get()
        .map(|settings| settings.extensions_enabled)
        .unwrap_or_else(|| !std::env::var("YAS_EXT").is_ok_and(|value| value == "0"))
}

pub(crate) fn channels_enabled() -> bool {
    DEPLOYMENT_SETTINGS
        .get()
        .map(|settings| settings.channels_enabled)
        .unwrap_or_else(|| !std::env::var("YAS_CHANNEL").is_ok_and(|value| value == "0"))
}

pub(crate) fn deployment_usize(name: &str, default: usize) -> usize {
    if let Some(settings) = DEPLOYMENT_SETTINGS.get() {
        return settings
            .values
            .get(name)
            .and_then(|value| usize::try_from(*value).ok())
            .unwrap_or(default);
    }
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

pub(crate) fn deployment_u64(name: &str, default: u64) -> u64 {
    if let Some(settings) = DEPLOYMENT_SETTINGS.get() {
        return settings.values.get(name).copied().unwrap_or(default);
    }
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// How many exited-but-retained terminals to keep, oldest evicted first.
/// `YAS_MAX_EXITED` overrides; 0 disables the bound.
///
/// A terminal's output stays readable after its command exits, and consumers
/// legitimately read it back long afterwards, so this is generous — it exists
/// to stop an orchestrator that never closes terminals from growing the map
/// without limit, not to reclaim memory promptly.
pub const DEFAULT_MAX_EXITED: usize = 1024;

/// Evict exited terminals this long after they exit.  `YAS_EXITED_LINGER`
/// overrides, in seconds.
///
/// Off by default, deliberately: a time bound throws away output someone may
/// still want, and "how long is a result interesting" is a policy question
/// the server has no way to answer. The count bound alone keeps the map
/// bounded without ever discarding anything an active consumer is likely to
/// come back for.
pub const DEFAULT_EXITED_LINGER: Duration = Duration::ZERO;

fn max_exited() -> usize {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var("YAS_MAX_EXITED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_EXITED)
    })
}

fn exited_linger() -> Duration {
    static V: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var("YAS_EXITED_LINGER")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_EXITED_LINGER)
    })
}

pub struct Config {
    /// Identity used to isolate this server's socket and persistent state from
    /// other yas servers owned by the same user.
    pub name: ServerName,
    pub shell: String,
    pub shell_flags: String,
    pub scrollback: usize,
    pub ipc_path: String,
    /// Whether `ipc_path` came from secure automatic runtime resolution. Such
    /// paths are revalidated as owner-only immediately before bind/removal;
    /// explicit configured paths retain their caller-selected location.
    pub ipc_path_is_automatic: bool,
    /// Canonical automatic YAS endpoint with one literal `{name}` server-name
    /// placeholder. It is exported to extensions as `YAS_SOCKET_TEMPLATE`
    /// so they can predict a named server before its socket exists. This must
    /// be derived from [`default_ipc_path_template`], independently of an
    /// explicit `ipc_path`.
    pub automatic_ipc_template: String,
    pub surface_encoders: Vec<SurfaceEncoderPreference>,
    pub surface_encoding: SurfaceEncoding,
    pub chroma: ChromaSubsampling,
    /// Which codecs viewers may send inbound (camera, microphone). Narrows
    /// what this host can decode; never widens it.
    pub media_codecs: MediaCodecPolicy,
    pub vaapi_device: String,
    #[cfg(unix)]
    pub fd_channel: Option<std::os::unix::io::RawFd>,
    pub verbose: bool,
    /// Advertise and accept the native non-PTY process family.
    pub processes: bool,
    /// Maximum number of concurrent client connections (0 = unlimited).
    pub max_connections: usize,
    /// Maximum number of PTYs across all clients (0 = unlimited).  Counts
    /// exited-but-retained terminals too, since those still hold an id and a
    /// scrollback.
    pub max_ptys: usize,
    /// Application-level ping interval. The server sends a Core Ping to every
    /// client at this cadence so that transports without native keepalive
    /// (WebRTC data channels) can detect dead connections.  0 = disabled.
    pub ping_interval: Duration,
    /// Skip compositor initialization (e.g. for share-only mode).
    pub skip_compositor: bool,
    /// Export the server's IPC path as `YAS_SOCK` in spawned terminals so
    /// `yas` invocations inside them target this server.  Off by default:
    /// `YAS_*` is otherwise stripped from child environments.
    pub export_sock: bool,
    /// Append the directory holding the running server binary to `PATH` in
    /// spawned terminals, so `yas` is callable inside them (Unix only; the
    /// Windows PTY inherits the server's environment wholesale).  Off by
    /// default, and worth leaving off when the server is embedded in a host
    /// binary whose directory holds no `yas`.
    pub inject_path: bool,
    /// Permit relayed streams to skip TLS certificate verification
    /// (`NET_OPEN_INSECURE`). Right for a self-signed dev server on loopback,
    /// wrong for anything reached across a network.
    /// `--allow-forward` egress patterns (docs/design/net.md § Target
    /// policy). Empty = unrestricted, the default.
    pub allow_forward: Vec<String>,
    pub allow_forward_insecure: bool,
    /// Permit durable extension create/update/control and startup restore.
    /// True by default; `--no-persistent-extensions` turns it off, which is
    /// how a bad definition gets repaired. Transient extensions remain
    /// available when this gate is false.
    pub allow_persistent_extensions: bool,
}

trait PtyDriver: Send {
    fn size(&self) -> (u16, u16);
    fn resize(&mut self, rows: u16, cols: u16);
    fn process(&mut self, data: &[u8]);
    fn title(&self) -> &str;
    fn search_result(&self, query: &str) -> Option<PtySearchResult>;
    fn take_title_dirty(&mut self) -> bool;
    fn take_clipboard_stores(&mut self) -> Vec<String>;
    fn used_rows(&self) -> u16;
    fn take_used_rows_dirty(&mut self) -> bool;
    fn cursor_position(&self) -> (u16, u16);
    fn synced_output(&self) -> bool;
    fn alt_screen(&self) -> bool;
    fn snapshot(&mut self, echo: bool, icanon: bool) -> FrameState;
    fn scrollback_frame(&mut self, offset: usize) -> FrameState;
    fn mouse_event(
        &self,
        type_: u8,
        button: u8,
        col: u16,
        row: u16,
        echo: bool,
        icanon: bool,
    ) -> Option<Vec<u8>>;
    fn total_lines(&self) -> u32;
    /// Absolute sequence and column of the cursor
    /// (docs/design/term-journal.md § Sequences).
    fn cursor_seq(&self) -> (u64, u16);
    /// Oldest sequence the scrollback still holds.
    fn oldest_seq(&self) -> u64;
    fn seq_text(
        &self,
        from_seq: u64,
        from_col: u16,
        end_seq: Option<u64>,
        max_bytes: usize,
    ) -> SeqText;
}

struct PtySearchResult {
    score: u32,
    primary_source: u8,
    matched_sources: u8,
    context: String,
    scroll_offset: Option<usize>,
}

impl PtyDriver for AlacrittyDriver {
    fn size(&self) -> (u16, u16) {
        AlacrittyDriver::size(self)
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        AlacrittyDriver::resize(self, rows, cols);
    }

    fn process(&mut self, data: &[u8]) {
        AlacrittyDriver::process(self, data);
    }

    fn title(&self) -> &str {
        AlacrittyDriver::title(self)
    }

    fn search_result(&self, query: &str) -> Option<PtySearchResult> {
        AlacrittyDriver::search_result(self, query).map(|result: AlacrittySearchResult| {
            PtySearchResult {
                score: result.score,
                primary_source: result.primary_source as u8,
                matched_sources: result.matched_sources,
                context: result.context,
                scroll_offset: result.scroll_offset,
            }
        })
    }

    fn take_title_dirty(&mut self) -> bool {
        AlacrittyDriver::take_title_dirty(self)
    }

    fn take_clipboard_stores(&mut self) -> Vec<String> {
        AlacrittyDriver::take_clipboard_stores(self)
    }

    fn used_rows(&self) -> u16 {
        AlacrittyDriver::used_rows(self)
    }

    fn take_used_rows_dirty(&mut self) -> bool {
        AlacrittyDriver::take_used_rows_dirty(self)
    }

    fn cursor_position(&self) -> (u16, u16) {
        AlacrittyDriver::cursor_position(self)
    }

    fn synced_output(&self) -> bool {
        AlacrittyDriver::synced_output(self)
    }

    fn alt_screen(&self) -> bool {
        AlacrittyDriver::alt_screen(self)
    }

    fn snapshot(&mut self, echo: bool, icanon: bool) -> FrameState {
        AlacrittyDriver::snapshot(self, echo, icanon)
    }

    fn scrollback_frame(&mut self, offset: usize) -> FrameState {
        AlacrittyDriver::scrollback_frame(self, offset)
    }

    fn mouse_event(
        &self,
        type_: u8,
        button: u8,
        col: u16,
        row: u16,
        echo: bool,
        icanon: bool,
    ) -> Option<Vec<u8>> {
        AlacrittyDriver::mouse_event(self, type_, button, col, row, echo, icanon)
    }

    fn total_lines(&self) -> u32 {
        AlacrittyDriver::total_lines(self)
    }

    fn cursor_seq(&self) -> (u64, u16) {
        AlacrittyDriver::cursor_seq(self)
    }

    fn oldest_seq(&self) -> u64 {
        AlacrittyDriver::oldest_seq(self)
    }

    fn seq_text(
        &self,
        from_seq: u64,
        from_col: u16,
        end_seq: Option<u64>,
        max_bytes: usize,
    ) -> SeqText {
        AlacrittyDriver::seq_text(self, from_seq, from_col, end_seq, max_bytes)
    }
}

#[cfg(test)]
const PREVIEW_FRAME_RESERVE: usize = 1;
const READY_FRAME_QUEUE_CAP: usize = 4;
const PTY_CHANNEL_CAPACITY: usize = 64;
/// Relay workers feed the ordinary tracked outbox through this bounded lane.
/// Catalogue snapshots can approach the protocol's multi-megabyte packet
/// limit, so retain at most one packet behind the one awaiting writer drain.
/// Alternate-screen TUIs commonly emit one repaint as hundreds of writes with
/// tens of microseconds between them. Consider a burst complete after this
/// much quiet instead of exposing its clear/redraw intermediates. Ordinary
/// shell output is never held for this heuristic.
const PTY_OUTPUT_QUIET: Duration = Duration::from_millis(1);
/// A low-refresh viewer should not make a continuously writing TUI disappear
/// indefinitely. The effective ceiling is the smaller of this and its fastest
/// viewer's display interval, so high-refresh displays remain uncapped.
const PTY_OUTPUT_COALESCE_MAX: Duration = Duration::from_millis(8);
/// Max bytes of PTY output parsed from one PTY in a delivery tick.
const PTY_PARSE_BUDGET_PER_TICK: usize = 256 * 1024;
/// Max bytes parsed across every PTY in one delivery tick.
///
/// Parsing holds the session mutex. A per-PTY cap alone lets a broken stack
/// multiply the critical section by its unit count, delaying browser input,
/// ACKs, and every other terminal even when the host has idle CPUs. The
/// session cap bounds that delay; round-robin traversal gives the deferred
/// terminals the front of the next tick. Bounded reader channels propagate
/// the backpressure to the producers.
const PTY_PARSE_BUDGET_PER_SESSION_TICK: usize = 256 * 1024;
const SYNC_OUTPUT_END: &[u8] = b"\x1b[?2026l";

/// Number of surface frames to send at wire speed after a keyframe request
/// (subscribe, resubscribe, or error recovery).  During this burst window
/// only outbox backpressure gates delivery — the time-based pacing interval
/// is skipped.  This lets bandwidth estimates ramp up quickly on high-latency
/// links instead of starving the pipeline with conservative initial rates.
const SURFACE_BURST_FRAMES: u8 = 4;

/// A chunk of data from the PTY reader, sent through a lock-free channel
/// so the reader never contends with the delivery tick for the Session mutex.
enum PtyInput {
    /// Raw bytes from the PTY, with the reader's sync-scan tail for boundary
    /// detection. The tick task calls `process()` + `respond_to_queries()`.
    Data(Vec<u8>),
    /// Data up to and including a sync-output-close (`\x1b[?2026l`).
    /// Process `before` and then take a snapshot.  Any bytes following the
    /// boundary are sent in a subsequent `Data` or `SyncBoundary` event —
    /// the reader's loop re-scans them, so this event must not try to
    /// process them itself.
    SyncBoundary { before: Vec<u8> },
    /// The PTY fd hit EOF or an error — the child likely exited.
    Eof,
}

/// Shared, level-triggered cancellation for one logical connection.
#[derive(Clone, Debug)]
struct ConnectionCancellation {
    state: watch::Sender<bool>,
    failure: Arc<AtomicU8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionFailure {
    SlowConsumer,
    ResourceLimit,
}

const CONNECTION_FAILURE_NONE: u8 = 0;
const CONNECTION_FAILURE_SLOW_CONSUMER: u8 = 1;
const CONNECTION_FAILURE_RESOURCE_LIMIT: u8 = 2;

impl Default for ConnectionCancellation {
    fn default() -> Self {
        let (state, _) = watch::channel(false);
        Self {
            state,
            failure: Arc::new(AtomicU8::new(CONNECTION_FAILURE_NONE)),
        }
    }
}

impl ConnectionCancellation {
    fn cancel(&self) {
        self.state.send_replace(true);
    }

    fn is_cancelled(&self) -> bool {
        *self.state.borrow()
    }

    pub(crate) fn failure(&self) -> Option<ConnectionFailure> {
        match self.failure.load(Ordering::Relaxed) {
            CONNECTION_FAILURE_SLOW_CONSUMER => Some(ConnectionFailure::SlowConsumer),
            CONNECTION_FAILURE_RESOURCE_LIMIT => Some(ConnectionFailure::ResourceLimit),
            _ => None,
        }
    }

    async fn cancelled(&self) {
        let mut state = self.state.subscribe();
        if *state.borrow() {
            return;
        }
        while state.changed().await.is_ok() {
            if *state.borrow() {
                return;
            }
        }
    }
}

/// Process-wide registry for logical connections during orderly shutdown.
///
/// Network sockets and in-process extension endpoints use the same
/// cancellation path. The shutdown latch is set before the accept loop is
/// woken, so a connection racing with shutdown is either registered and
/// cancelled by the snapshot or refused by `register`.
struct ConnectionRegistry {
    shutting_down: AtomicBool,
    cleanup_started: AtomicBool,
    next_registration: AtomicU64,
    cancellations: std::sync::Mutex<HashMap<u64, ConnectionCancellation>>,
    drained: Notify,
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self {
            shutting_down: AtomicBool::new(false),
            cleanup_started: AtomicBool::new(false),
            next_registration: AtomicU64::new(1),
            cancellations: std::sync::Mutex::new(HashMap::new()),
            drained: Notify::new(),
        }
    }
}

impl ConnectionRegistry {
    fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    fn register(
        self: &Arc<Self>,
        cancellation: ConnectionCancellation,
    ) -> Option<ConnectionRegistration> {
        self.register_inner(cancellation, false)
    }

    /// Admit only a native YAS reconnect which will inherit an already
    /// scheduled GOAWAY and may resolve the boot-scoped SHUTDOWN Result. It
    /// cannot become a normal session and is refused once cleanup starts.
    fn register_shutdown_retry(
        self: &Arc<Self>,
        cancellation: ConnectionCancellation,
    ) -> Option<ConnectionRegistration> {
        self.register_inner(cancellation, true)
    }

    fn register_inner(
        self: &Arc<Self>,
        cancellation: ConnectionCancellation,
        shutdown_retry: bool,
    ) -> Option<ConnectionRegistration> {
        let mut connections = self.cancellations.lock().unwrap();
        if self.is_shutting_down()
            && (!shutdown_retry || self.cleanup_started.load(Ordering::Acquire))
        {
            cancellation.cancel();
            return None;
        }
        let registration = self.next_registration.fetch_add(1, Ordering::Relaxed);
        let previous = connections.insert(registration, cancellation);
        assert!(previous.is_none(), "connection registration id wrapped");
        Some(ConnectionRegistration {
            registration,
            registry: Arc::clone(self),
        })
    }

    /// Seal admission. Graceful YAS shutdown may do this before the one
    /// caller which later claims process cleanup at its drain deadline.
    fn seal_shutdown(&self) -> bool {
        self.shutting_down
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Claim the one teardown sequence after admission may already have been
    /// sealed by a graceful native YAS drain.
    fn begin_cleanup(&self) -> bool {
        self.cleanup_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn cancel_all(&self) {
        let cancellations = self
            .cancellations
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for cancellation in cancellations {
            cancellation.cancel();
        }
    }

    async fn wait_empty(&self) {
        loop {
            let drained = self.drained.notified();
            if self.cancellations.lock().unwrap().is_empty() {
                return;
            }
            drained.await;
        }
    }
}

struct ConnectionRegistration {
    registration: u64,
    registry: Arc<ConnectionRegistry>,
}

impl Drop for ConnectionRegistration {
    fn drop(&mut self) {
        self.registry
            .cancellations
            .lock()
            .unwrap()
            .remove(&self.registration);
        // Unlike `notify_waiters`, this retains a permit if teardown races
        // just ahead of `wait_empty` polling its `Notified` future.
        self.registry.drained.notify_one();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ConnectionOrigin {
    /// A transport this build cannot describe. Every native session arrives on
    /// the local IPC endpoint today, so this is the Windows named-pipe case and
    /// the one where the kernel refused to name the peer.
    Network,
    /// The local IPC peer, as the kernel reports it. This is what the browser's
    /// edge, the CLI, and every other local client actually are, and saying so
    /// beats publishing "some connection" for all of them.
    Local(yas_webserver::local_ipc::PeerCredentials),
    Extension {
        extension_id: u64,
        definition_revision: u64,
        attempt: u64,
        task_id: u32,
        /// The durable name of a persistent definition, the label a transient
        /// `ext run` carried, or empty when it had neither.
        name: String,
    },
}

impl ConnectionOrigin {
    const fn label(&self) -> &'static str {
        match self {
            Self::Network => "network client",
            Self::Local(_) => "local client",
            Self::Extension { .. } => "extension client",
        }
    }
}

#[derive(Clone, Debug)]
struct NativeClientIdentity {
    session_id: [u8; 16],
    client_instance: [u8; 16],
    name: String,
    release: String,
}

/// A YAS session registered directly with the shared backend. This is the
/// authoritative Client-family identity used for catalogue projection and
/// targeted disconnects.
struct NativeYasClient {
    identity: NativeClientIdentity,
    origin: ConnectionOrigin,
    connected_at: Instant,
    disconnect: mpsc::Sender<()>,
    inbound_bytes: Arc<AtomicU64>,
    outbound_bytes: Arc<AtomicU64>,
    active_subscriptions: Arc<NativeYasSubscriptions>,
}

/// The latest connection-owned subscriptions projected into the Client
/// catalogue. Native sessions publish this without taking the shared backend
/// mutex so catalogue readers never invert an async lock with a session lock.
#[derive(Default)]
struct NativeYasSubscriptions {
    snapshot: std::sync::RwLock<NativeYasSubscriptionSnapshot>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NativeYasSubscriptionSnapshot {
    active: yas_wire::client::ActiveSubscriptions,
    auxiliary_details: yas_wire::client::AuxiliarySubscriptionDetails,
}

impl NativeYasSubscriptions {
    fn snapshot(&self) -> NativeYasSubscriptionSnapshot {
        self.snapshot.read().map_or_else(
            |_| NativeYasSubscriptionSnapshot::default(),
            |value| value.clone(),
        )
    }

    fn replace(&self, snapshot: NativeYasSubscriptionSnapshot) -> bool {
        snapshot
            .active
            .extension()
            .expect("server-built native Client subscriptions are valid");
        snapshot
            .auxiliary_details
            .extension()
            .expect("server-built native Client subscription details are valid");
        let Ok(mut current) = self.snapshot.write() else {
            return false;
        };
        if *current == snapshot {
            return false;
        }
        *current = snapshot;
        true
    }

    fn clear(&self) {
        if let Ok(mut current) = self.snapshot.write() {
            *current = NativeYasSubscriptionSnapshot::default();
        }
    }
}

#[cfg(test)]
const AUDIO_QUEUE_MAX_FRAMES: usize = 25;

struct Pty {
    handle: PtyHandle,
    driver: Box<dyn PtyDriver>,
    /// Client-chosen tag set at creation time.
    tag: String,
    dirty: bool,
    /// Earliest time an ordinary (non-synchronized) output burst should be
    /// snapshotted. `None` means the dirty state may be sent immediately.
    snapshot_not_before: Option<Instant>,
    /// Hard end of the current coalescing burst. Unlike
    /// `snapshot_not_before`, later chunks do not move it.
    snapshot_by: Option<Instant>,
    ready_frames: VecDeque<FrameState>,
    /// Receives raw byte chunks from the PTY reader task without mutex contention.
    byte_rx: mpsc::Receiver<PtyInput>,
    reader_handle: std::thread::JoinHandle<()>,
    /// Cached (echo, icanon) from tcgetattr; refreshed every ~250ms.
    lflag_cache: (bool, bool),
    lflag_last: Instant,
    /// When we last broadcast a title update for this PTY.
    last_title_send: Instant,
    /// Title changed but not yet sent (debounced).
    title_pending: bool,
    /// Last used visible rows value broadcast for this PTY.
    last_used_rows_sent: u16,
    /// The driver's `scrolled_lines` as of the last re-anchor pass, so the
    /// tick can turn a monotonic counter into "lines that moved since we
    /// last looked" for every scrolled-back client at once.
    last_scrolled_lines: u64,
    /// When the server should stop this terminal, if a client armed a
    /// deadline.  Absent means unbounded, which stays the default — a
    /// multiplexer whose sessions expire on their own would be useless.
    deadline: Option<Instant>,
    /// Set once the deadline has fired and SIGTERM has gone out; when it
    /// passes, the group gets SIGKILL.
    stop_deadline: Option<Instant>,
    /// Fallback deadline armed when the direct child exits. Reader EOF finalizes sooner.
    exit_drain_deadline: Option<Instant>,
    /// Attributed cause, moved onto the Terminal Exited event by `cleanup_pty_internal`.
    exit_reason: u8,
    /// The subprocess has exited but the terminal state is retained for reading.
    exited: bool,
    /// When it exited, for the retention bound.  `None` while live.
    exited_at: Option<Instant>,
    /// Bumped every time this slot gets a fresh child, so work queued against
    /// one generation cannot land on the next. Terminal Restart reuses the id
    /// and the driver in place, which makes the id alone useless as identity.
    generation: u64,
    /// Exit status: WEXITSTATUS if normal exit, negative signal number if signalled,
    /// EXIT_STATUS_UNKNOWN if not yet collected.
    exit_status: i32,
    /// The command field of a Terminal List record: what this terminal is running, as a
    /// human reads it.  `None` for a plain shell.  For an argv terminal this
    /// is a *rendering*, not something to run — restart reads `spec`.  It is
    /// computed by the create path rather than derived here, because the same
    /// string has to clear `list_refusal`'s size guard before the terminal
    /// exists, and a second rendering would be a second answer.
    command: Option<String>,
    /// What was actually started, replayed verbatim by Terminal Restart. Without
    /// this a terminal created with an argv, an environment, or both came back
    /// from a restart as a bare login shell.
    spec: pty::OwnedChildSpec,
    /// Working directory last reported by the shell via OSC 7, already
    /// validated by `parse_osc7_url` (docs/protocol.md, "Working directory
    /// tracking").  Last write wins; None until shell integration first
    /// reports (then Terminal Cwd falls back to the kernel's view).
    osc7_cwd: Option<String>,
    /// Commands this PTY's shell announced through OSC 133
    /// (docs/design/term-journal.md).  Empty and free for every shell
    /// without integration.
    journal: journal::CommandJournal,
    /// An OSC left unterminated at the end of the last chunk, held so the
    /// scan of the next one sees the whole sequence.  A PTY read boundary
    /// falls wherever the kernel put it, and a marker split across one would
    /// otherwise be lost — silently mis-attributing a command's output.
    osc_carry: Vec<u8>,
}

impl Pty {
    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn mark_output_dirty(&mut self, now: Instant, coalesce_cap: Duration) {
        self.dirty = true;
        if self.driver.alt_screen() {
            arm_pty_output_coalesce(
                &mut self.snapshot_not_before,
                &mut self.snapshot_by,
                now,
                coalesce_cap,
            );
        } else {
            self.snapshot_not_before = None;
            self.snapshot_by = None;
        }
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
        self.snapshot_not_before = None;
        self.snapshot_by = None;
    }
}

fn pty_output_coalesce_cap(display_fps: f32) -> Duration {
    let display_interval = Duration::from_secs_f64(1.0 / display_fps.max(1.0) as f64);
    PTY_OUTPUT_COALESCE_MAX.min(display_interval)
}

fn arm_pty_output_coalesce(
    snapshot_not_before: &mut Option<Instant>,
    snapshot_by: &mut Option<Instant>,
    now: Instant,
    coalesce_cap: Duration,
) {
    let hard_deadline = *snapshot_by.get_or_insert(now + coalesce_cap);
    *snapshot_not_before = Some((now + PTY_OUTPUT_QUIET).min(hard_deadline));
}

/// A surface's stamped application identity, mirroring the compositor's
/// `AppIdentity`. Absent for anything on the shared Wayland socket.
#[derive(Debug, Clone)]
struct SurfaceOrigin {
    app_id: String,
    instance_id: String,
}

struct CachedSurfaceInfo {
    surface_id: u16,
    parent_id: u16,
    /// Stamped identity, as opposed to the self-asserted `app_id` below.
    origin: Option<SurfaceOrigin>,
    width: u16,
    height: u16,
    /// The composited size in surface-logical pixels, as last reported by
    /// the compositor.  Kept so a client attaching mid-session learns the
    /// surface's scale from its first Surface Resized event instead of
    /// assuming its own — see [`msg_surface_resized`].  Zero until the
    /// compositor has reported one, in which case the physical size is
    /// sent in its place (scale 1x is the right guess for a surface no
    /// high-DPI viewer has resized yet).
    logical_width: u16,
    logical_height: u16,
    title: String,
    app_id: String,
}

#[derive(Clone, Copy)]
struct CachedSurfaceTextInput {
    hint: u32,
    purpose: u32,
    /// Last caret rectangle the app named, so a client that joins mid-edit
    /// puts its IME popup in the same place as everyone else.
    cursor_rect: Option<(i16, i16, i16, i16)>,
}

/// Last committed pixel buffer for a surface, kept so we can re-encode a
/// keyframe for late-joining clients without going back to the compositor.
struct LastPixels {
    width: u32,
    height: u32,
    pixels: yas_compositor::PixelData,
    /// Monotonically increasing counter bumped on every SurfaceCommit.
    /// Used to skip re-encoding when the pixel data hasn't changed.
    generation: u64,
    /// CLOCK_MONOTONIC milliseconds captured at compositor commit time.
    /// Used as the surface frame timestamp so the client sees the source's
    /// presentation timing rather than the (jittery) encode-delivery clock.
    timestamp_ms: u32,
    timestamp_sub_us: u16,
    /// Pixel-cache only: an on-demand BGRA readback published while an
    /// NV12 zero-copy stream owns this key.  The encode tick must not
    /// feed it to an encoder — the stream already carries this frame,
    /// and re-encoding it through NVENC's ARGB conversion (whose
    /// rounding differs from the zero-copy shader's) shifts the picture
    /// for one frame.  Raw-paint consumers (initial paint, capture) use
    /// it freely.
    encoder_skip: bool,
}

/// Cache one compositor output without treating its dimensions as the
/// surface's native dimensions.
///
/// The compositor publishes the native image and every registered
/// per-client target through the same `SurfaceCommit` event.  Keeping this
/// helper limited to the pixel cache makes it impossible for a downscale
/// commit to overwrite `CachedSurfaceInfo`'s native physical/logical pair.
fn cache_surface_commit(
    last_pixels: &mut HashMap<(u16, u32, u32), LastPixels>,
    pixel_generation: &mut u64,
    key: (u16, u32, u32),
    pixels: yas_compositor::PixelData,
    timestamp_ms: u32,
    timestamp_sub_us: u16,
    encoder_skip: bool,
) {
    let (_, width, height) = key;
    *pixel_generation += 1;
    last_pixels.insert(
        key,
        LastPixels {
            width,
            height,
            pixels,
            generation: *pixel_generation,
            timestamp_ms,
            timestamp_sub_us,
            encoder_skip,
        },
    );
}

/// The most recent bitstream a compositor-resident encoder produced for
/// one `(surface, client)` pair.
///
/// Kept apart from `last_pixels` because Vulkan Video owns one encoder per
/// subscribing client: the bytes belong to exactly one client and must
/// never be handed to another, which is what sharing them by target size
/// used to do.
struct LastEncoded {
    width: u32,
    height: u32,
    data: Arc<Vec<u8>>,
    is_keyframe: bool,
    codec_flag: u8,
    generation: u64,
    timestamp_ms: u32,
    timestamp_sub_us: u16,
}

/// Drop every `last_pixels` entry belonging to `sid`, regardless of
/// per-target size.  Used when the surface is destroyed/resized/created
/// to avoid serving stale frames to encoders that were sized against
/// the prior composite.
fn last_pixels_remove_for_sid(last_pixels: &mut HashMap<(u16, u32, u32), LastPixels>, sid: u16) {
    let keys: Vec<(u16, u32, u32)> = last_pixels.keys().filter(|k| k.0 == sid).copied().collect();
    for k in keys {
        last_pixels.remove(&k);
    }
}

/// Drop every compositor-encoded frame belonging to `sid`, for every
/// client.  Paired with `last_pixels_remove_for_sid`: a surface that was
/// destroyed or resized invalidates both.
fn last_encoded_remove_for_sid(last_encoded: &mut HashMap<(u16, u64), LastEncoded>, sid: u16) {
    last_encoded.retain(|k, _| k.0 != sid);
}

/// Immutable metadata used by one delivery pass. Pixel buffers stay in
/// `last_pixels`; this compact index is shared across ticks until the cache
/// changes, avoiding a full map walk for PTY-only wakeups.
type PixelSnapshot = (u16, u32, u32, u64, u32, u16);

/// Authoritative compositor native dims for `sid`, preferring the value
/// stored from the most recent `SurfaceResized` event.  Falls back to the
/// largest entry in the per-target pixel snapshot when the resized event
/// hasn't been received yet (very first render after `SurfaceCreated`).
///
/// Native MUST NOT be derived from `pixel_snapshot.max_by_key(area)` once
/// the `SurfaceResized` value exists: the renderer can copy into stale
/// `external_outputs` / `downscale_outputs` entries (registered for prior
/// per-client targets) and those produce extra pixel snapshots at the
/// old, possibly-larger sizes.  The largest-area pick then yields a
/// stale value, mis-clamping `per_client_encode_target` and triggering
/// avoidable encoder rebuilds — and on aspect-ratio mismatches, freezing
/// visible frames at the stale target until the entry is cleared.
fn compositor_native_for_sid<S: BuildHasher>(
    native_sizes: &HashMap<u16, (u32, u32), S>,
    pixel_snapshot: &[PixelSnapshot],
    sid: u16,
) -> Option<(u32, u32)> {
    if let Some(&dims) = native_sizes.get(&sid) {
        return Some(dims);
    }
    pixel_snapshot
        .iter()
        .filter(|&&(s, _, _, _, _, _)| s == sid)
        .max_by_key(|&&(_, w, h, _, _, _)| (w as u64) * (h as u64))
        .map(|&(_, w, h, _, _, _)| (w, h))
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DesktopBackendState {
    tray: HashMap<u32, yas_desktop::TrayItem>,
    notifications: HashMap<u32, yas_desktop::Notification>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MprisBackendState {
    players: HashMap<u32, yas_desktop::MprisPlayer>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MediaBackendState {
    pipewire_available: bool,
    microphone_available: bool,
    camera_available: bool,
    screencasts: Vec<MediaBackendScreencast>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct MediaBackendScreencast {
    session_id: u32,
    app_id: String,
    surface_ids: Vec<u16>,
}

struct SharedCompositor {
    handle: CompositorHandle,
    /// Current compositor clipboard authority, replayed to every new web
    /// client before READY so its first paste takes the correct path.
    wayland_clipboard_owned: bool,
    surfaces: FxHashMap<u16, CachedSurfaceInfo>,
    /// Enabled text-input state by toplevel. Disabled entries are removed;
    /// the remaining state is replayed after `SURFACE_CREATED` to viewers
    /// joining an existing compositor session.
    surface_text_inputs: FxHashMap<u16, CachedSurfaceTextInput>,
    /// Last semantic cursor per surface, retained for native Surface WATCH
    /// snapshots without parsing a broadcast packet.
    surface_cursors: FxHashMap<u16, yas_wire::surface::CursorState>,
    /// Most recent activation request and its monotonic compositor revision.
    surface_activation: Option<(u16, u64)>,
    surface_activation_revision: u64,
    /// Latest pixel snapshot per `(surface_id, width, height)`.  The
    /// compositor renders one surface into multiple per-target buffers
    /// (one per registered per-client encoder size) plus a native BGRA
    /// staging readback, so the same surface produces several entries
    /// here — one per distinct size.  The encode loop picks the entry
    /// matching its client's per-client encode target; CPU encoders
    /// without a registered external fall back to the largest entry
    /// (the native composite) and downscale themselves.
    last_pixels: HashMap<(u16, u32, u32), LastPixels>,
    /// Opaque NV12/NV24 alternatives for targets that simultaneously
    /// publish CPU-readable BGRA. Kept separate so one representation does
    /// not overwrite the other under the shared `(surface, w, h)` key.
    last_opaque_pixels: HashMap<(u16, u32, u32), LastPixels>,
    /// Copy-on-write metadata view of `last_pixels`. Rebuilt only after a
    /// pixel-cache mutation, then shared by pointer across delivery ticks.
    pixel_snapshot: Arc<Vec<PixelSnapshot>>,
    /// Exact metadata for `last_opaque_pixels`; NVENC clients prefer this
    /// generation while other clients use `pixel_snapshot`.
    opaque_pixel_snapshot: Arc<Vec<PixelSnapshot>>,
    pixel_snapshot_dirty: bool,
    /// Latest compositor-encoded bitstream per `(surface_id, client_id)`.
    last_encoded: HashMap<(u16, u64), LastEncoded>,
    /// Display-rate clocks currently installed in the compositor.  Kept here
    /// only to avoid sending unchanged clock configuration every tick.
    frame_clock_intervals: FxHashMap<u16, Duration>,
    /// Subscription/display/surface topology changed since the last clock
    /// reconciliation. Keeps unchanged delivery ticks out of the client ×
    /// subscription scan.
    frame_clocks_dirty: bool,
    #[cfg(target_os = "linux")]
    created_at: Instant,
    /// Monotonically increasing counter for pixel generations.
    pixel_generation: u64,
    /// Last time we sent blanket RequestFrame for all surfaces (including
    /// those without subscribers).  Throttled to prevent hot-looping when
    /// apps commit at high rates without any client consuming frames.
    last_blanket_frame_request: Instant,
    /// Last dimensions sent to the compositor via `CompositorCommand::SurfaceResize`.
    /// Used to dedup resize commands — the composited output size
    /// (`info.width`/`info.height`) may differ from the requested size
    /// when the Wayland client sets `xdg_geometry` (e.g. excluding a
    /// title bar), so we compare against the actually-requested values.
    last_configured_size: FxHashMap<u16, (u16, u16, u16)>,
    /// Instant of the last resize actually handed to the compositor, per
    /// surface.  Opens that surface's settle window; see
    /// `SURFACE_RESIZE_SETTLE`.
    last_resize_at: FxHashMap<u16, Instant>,
    /// Vulkan Video 4:4:4 profiles this device has built and then failed to
    /// encode with, by the name the selection uses (`h264-vulkan 4:4:4` and
    /// friends).
    ///
    /// The per-subscription `vulkan_refused` bits are the right memory for a
    /// session that could not be built: that can be about this frame's size.
    /// A built 4:4:4 profile that cannot encode is a driver capability lie,
    /// and re-learning it costs a session setup plus a dozen failing encodes
    /// — ~90 ms — every time a viewer subscribes or a pane changes size. On
    /// NVIDIA, H.264 4:4:4 is advertised, initialises, and then fails every
    /// encode, so remembering that profile saves real work.
    ///
    /// Do not put 4:2:0 here. It is the baseline profile, and an encode can
    /// fail because of one surface, extent, or transient synchronization
    /// problem. Permanently declining it device-wide after that one failure
    /// sends every later surface to a lower encoder even when a fresh Vulkan
    /// session works.
    ///
    /// Lives here rather than in the config: it describes the device this
    /// compositor came up on, and a new compositor gets to find out afresh.
    declined_vulkan_444_encoders: HashSet<&'static str>,
    /// Surfaces with a configure on the wire whose new size the compositor
    /// has not reported back yet, and when it went out.  An encoder built
    /// against the size the surface is *leaving* is finished work nobody
    /// wants: see `RESIZE_ENCODER_GRACE`.
    resize_inflight: FxHashMap<u16, Instant>,
    /// The most recent size requested for a surface while its settle window
    /// was still open.  Dispatched by `flush_due_resizes` once the window
    /// closes; overwritten (not queued) by every further request, so a drag
    /// costs one configure per window rather than one per frame.
    pending_resize: FxHashMap<u16, (u16, u16, u16)>,
    /// Authoritative compositor native (physical) size per surface, set from
    /// `CompositorEvent::SurfaceResized`.  Used by the per-client encode
    /// target computation as the `(native_w, native_h)` clamp.
    ///
    /// Why not derive native from `last_pixels.max_by_key((w, h))`?  The
    /// renderer can copy into stale `external_outputs` / `downscale_outputs`
    /// entries (registered for prior per-client targets that no longer match
    /// the current native).  Those produce extra `last_pixels` entries at
    /// the old, possibly-larger sizes.  Picking the largest entry as
    /// "native" then yields a stale value, which mis-clamps
    /// `per_client_encode_target` and triggers an avoidable encoder
    /// rebuild — and on aspect-ratio mismatches between old downscale
    /// targets and new compositor native, the encoder ends up sized for
    /// the stale target, freezing visible frames at the wrong size until
    /// the stale entry is cleared.
    native_sizes: FxHashMap<u16, (u32, u32)>,
    /// Audio capture pipeline (PipeWire daemon → in-process libpipewire capture → Opus encode).
    /// `None` when PipeWire is not available or `YAS_AUDIO=0`.
    #[cfg(target_os = "linux")]
    audio_pipeline: Option<audio::AudioPipeline>,
    /// Private session bus whose activation environment points at this
    /// compositor. Desktop portals spawned through it map inside yas rather
    /// than escaping to the host compositor.
    #[cfg(target_os = "linux")]
    desktop_bus: Option<desktop_bus::DesktopBus>,
    /// The X11 bridge, when one is installed. Owns the `DISPLAY` that PTYs
    /// and D-Bus activation export, and dies with the session.
    #[cfg(target_os = "linux")]
    xwayland: Option<xwayland::Xwayland>,
    /// Canonical semantic tray/notification state replayed to late subscribers.
    #[cfg(target_os = "linux")]
    desktop_state: DesktopBackendState,
    /// Latest semantic menu trees retained for native Desktop GET_MENU.
    #[cfg(target_os = "linux")]
    desktop_menus: HashMap<u32, yas_desktop::TrayMenu>,
    /// Semantic Desktop command sink used by native integration tests. The
    /// production backend is the private D-Bus service; tests capture the
    /// exact command here so they do not need to encode or decode private
    /// backend packets to exercise the native path.
    #[cfg(all(target_os = "linux", test))]
    native_desktop_commands: Vec<yas_desktop::Command>,
    /// Delete metadata is not present in the live state after removal but is
    /// required for truthful native Desktop REMOVE records.
    #[cfg(target_os = "linux")]
    desktop_removed_notifications: HashMap<u32, (u32, u8)>,
    /// Canonical normalized MPRIS state replayed to per-connection subscribers.
    #[cfg(target_os = "linux")]
    mpris_state: MprisBackendState,
    /// Deterministic semantic Media capability fixture. Real sessions always
    /// derive this state from the live compositor services.
    #[cfg(all(target_os = "linux", test))]
    native_media_state_override: Option<MediaBackendState>,
    /// When each cached MPRIS position was observed by the server. Wire
    /// records carry a position but no anchor, so late-subscriber replay must
    /// retain this separately to advance playing tracks from monotonic time.
    #[cfg(target_os = "linux")]
    mpris_position_observed_at: HashMap<u32, Instant>,
    /// Results for native requesters. Each native session drains its semantic
    /// result queue.
    #[cfg(target_os = "linux")]
    native_mpris_results: HashMap<u64, VecDeque<yas_desktop::MprisActionResult>>,
    /// Credit/revocation completions produced outside a native session turn.
    #[cfg(target_os = "linux")]
    native_media_input_events: HashMap<u64, VecDeque<NativeMediaInputEvent>>,
    /// Connection-bound viewer microphone/camera leases and their ephemeral
    /// PipeWire source nodes.
    #[cfg(target_os = "linux")]
    media_input: media_input::MediaInput,
    /// Granted portal window streams and their short-lived PipeWire nodes.
    #[cfg(target_os = "linux")]
    screencasts: HashMap<u32, ScreenCastSession>,
    /// Shared fan-out state for audio — subscribers, catch-up ring,
    /// listener flag.  Persistent across pipeline restarts so clients
    /// stay subscribed even when the pipeline is restarted.  Always present on Linux;
    /// subscribe/unsubscribe succeeds even when the pipeline itself is
    /// absent (frames just don't flow until it's back).
    #[cfg(target_os = "linux")]
    audio_broadcast: Arc<audio::AudioBroadcast>,
    /// Compositor instance ID passed to `AudioPipeline::spawn()` so restarts
    /// reuse the same audio runtime directory.
    #[cfg(target_os = "linux")]
    audio_session_id: u16,
    /// When the last audio pipeline restart was attempted.  Used to enforce a
    /// cooldown so we don't spin on persistent failures.
    #[cfg(target_os = "linux")]
    last_audio_restart: Option<Instant>,
    /// A live runtime died and should be retried after the cooldown. Separate
    /// from `audio_pipeline == None`, which also represents operator-disabled
    /// or unavailable-at-start configurations.
    #[cfg(target_os = "linux")]
    audio_restart_needed: bool,
    #[cfg(target_os = "linux")]
    audio_restart_inflight: bool,
    /// When the pipeline was last checked for liveness.  `AudioPipeline::is_alive`
    /// costs up to four `waitpid` syscalls, and `tick` runs on every PTY output
    /// chunk, so polling it per tick charged terminal throughput for audio
    /// supervision — 4-6% of server CPU with no audio in use.
    #[cfg(target_os = "linux")]
    last_audio_liveness_check: Option<Instant>,
}

fn native_surface_cursor(
    cursor: &yas_compositor::CursorImage,
) -> Option<yas_wire::surface::CursorState> {
    match cursor {
        yas_compositor::CursorImage::Named(name) => {
            Some(yas_wire::surface::CursorState::Named(name.clone()))
        }
        yas_compositor::CursorImage::Hidden => Some(yas_wire::surface::CursorState::Hidden),
        yas_compositor::CursorImage::Custom {
            hotspot_x,
            hotspot_y,
            width,
            height,
            scale,
            rgba,
        } => {
            let mut png = Vec::new();
            {
                let mut encoder =
                    png::Encoder::new(&mut png, u32::from(*width), u32::from(*height));
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                let mut writer = encoder.write_header().ok()?;
                writer.write_image_data(rgba).ok()?;
            }
            if png.len() > yas_wire::schema::surface::MAX_INLINE_CURSOR_BYTES as usize {
                return None;
            }
            Some(yas_wire::surface::CursorState::Custom {
                hotspot_x: i32::from(*hotspot_x),
                hotspot_y: i32::from(*hotspot_y),
                width: u32::from(*width),
                height: u32::from(*height),
                scale_120: scale.saturating_mul(120).max(120),
                png,
            })
        }
    }
}

/// How long a surface's resize settle window stays open.  The first resize
/// of a window is dispatched immediately — a lone resize, and the start of a
/// drag, react at RTT rather than waiting out a timer — and everything that
/// arrives while the window is open is coalesced into a single configure at
/// the end of it.
///
/// This bounds compositor configure cycles (and the encoder recreation, hence
/// keyframe, that a size change forces) to one per surface per window, no
/// matter how fast sizes arrive.  It has to live here rather than only in the
/// client: the mediated size is shared across *all* subscribers, so concurrent
/// viewer resizes still have to be coalesced, and non-browser clients reach
/// Surface Resize requests with no debounce at all.
const SURFACE_RESIZE_SETTLE: Duration = Duration::from_millis(100);

/// How long a viewer's size claim outlives its subscription.
///
/// Only long enough to ride out churn, not absence. A view being remounted —
/// a pane handed between two places in the UI, a page briefly hidden — drops
/// its subscription and takes it straight back, and letting the claim die in
/// that gap resizes the window for every other viewer twice for nothing. The
/// client's own deferred unsubscribe is 250ms, so this covers a remount
/// several times over.
///
/// Anything longer is absence, and absence has to be believed quickly:
/// closing an iPad unsubscribes without ever dropping the socket, and until
/// the claim expires a pane on the other side of the world is still the width
/// of a tablet nobody is looking at.
///
/// This used to be seconds, to protect against a viewer hiding its tab and
/// dragging every *other* surface with it. That was the global output scale
/// doing the dragging; with density per surface, a viewer leaving now only
/// touches the surfaces it was actually watching, so the window can be as
/// short as the churn it exists to absorb.
#[cfg(test)]
const SURFACE_CLAIM_GRACE: Duration = Duration::from_millis(750);

/// How long a subscriber holds out for the `OPAQUE_FD` publish it asked for
/// before re-registering the shared target.
///
/// `RegisterDownscaleTarget { want_nv12_opaque, .. }` is a request, not a
/// commitment: the renderer falls back to BGRA on its own when the fd export
/// fails, and a later subscriber lifecycle transition can invalidate the
/// allocation. NVENC never consumes that BGRA on the CPU; after this grace
/// the server repairs the registration and asks the compositor to render the
/// GPU representation again.
const OPAQUE_PUBLISH_GRACE: Duration = Duration::from_millis(250);

/// How long a dispatched configure may hold off building an encoder for the
/// size the surface is leaving.
///
/// Restoring a parked thumbnail into a pane sends both at once: a subscribe
/// that wants the surface at pane resolution and a resize that moves the
/// surface there.  Building for the old native in between costs a full
/// encoder creation (~150-250 ms of NVENC init here) whose every frame is
/// the wrong size, and it delays the one that isn't — the encoders queue on
/// the same blocking pool.  Waiting for the configure to land is strictly
/// faster whenever it does land.
///
/// The cap is what makes it safe to wait: a client that ignores its
/// configure (or acks it at a size of its own choosing) would otherwise
/// hold its viewers on a frozen picture forever.  Past this, build for
/// whatever the surface actually is.
const RESIZE_ENCODER_GRACE: Duration = Duration::from_millis(400);

/// What to do with a requested surface size.  Split out from
/// `Session::resize_surface` so the policy is testable without a live
/// compositor.
#[derive(Debug, PartialEq, Eq)]
enum ResizeAction {
    /// Already the size we last asked for — nothing to send.
    Ignore,
    /// Inside the settle window: keep it and let `tick` send it later.
    Hold,
    /// Send it now and open a new settle window.
    Dispatch,
}

/// A DOM `MouseEvent.button` as the evdev code a Wayland client expects.
///
/// The thumb buttons are the ones worth explaining. Linux mice report them
/// as `BTN_SIDE` and `BTN_EXTRA`, and that is what every toolkit binds to
/// history navigation — GTK surfaces them as buttons 8 and 9, Chromium reads
/// them as back and forward. `BTN_BACK` and `BTN_FORWARD` exist and read
/// like the obvious choice, but almost no hardware emits them and almost
/// nothing listens for them, so sending those would be a press that lands
/// nowhere.
///
/// An unknown number becomes a left click, which is what this did before
/// back and forward had codes of their own.
fn evdev_button(dom_button: u8) -> u32 {
    match dom_button {
        1 => 0x112, // BTN_MIDDLE
        2 => 0x111, // BTN_RIGHT
        3 => 0x113, // BTN_SIDE, "back"
        4 => 0x114, // BTN_EXTRA, "forward"
        _ => 0x110, // BTN_LEFT
    }
}

fn resize_action(
    last_configured: Option<(u16, u16, u16)>,
    last_resize_at: Option<Instant>,
    now: Instant,
    requested: (u16, u16, u16),
) -> ResizeAction {
    // Compare against the last *requested* dimensions, not the composited
    // output dimensions (`info.width`/`info.height`).  The composited output
    // may be smaller when the Wayland client sets xdg_geometry (e.g. Chromium
    // excludes the title bar), so comparing against it would make every
    // resize look like a change, flooding the compositor with redundant
    // configures and re-creating the encoder (keyframe) on every tick during
    // a drag-resize.
    if let Some((lw, lh, ls)) = last_configured {
        let (rw, rh, rs) = requested;
        // Within a couple of pixels is BSP settle noise, not a resize: the
        // browser's pane measurement and the compositor's physical↔logical
        // rounding disagree by ±2px and re-request each other's answer.
        // Configuring the surface for that reconfigures the client, tears
        // down every compositor-resident encode session on it, and opens
        // each rebuilt one with a multi-megabyte keyframe — per nudge.
        // The viewer letterboxes the difference invisibly instead
        // (`per_client_encode_target` snaps such views to native).
        if ls == rs && lw.abs_diff(rw) <= 2 && lh.abs_diff(rh) <= 2 {
            return ResizeAction::Ignore;
        }
    }
    match last_resize_at {
        Some(t) if now.duration_since(t) < SURFACE_RESIZE_SETTLE => ResizeAction::Hold,
        _ => ResizeAction::Dispatch,
    }
}

impl SharedCompositor {
    #[cfg(target_os = "linux")]
    fn media_state(&self) -> MediaBackendState {
        #[cfg(test)]
        if let Some(state) = &self.native_media_state_override {
            return state.clone();
        }
        let microphone = self.audio_pipeline.is_some()
            && std::env::var("YAS_MEDIA_INPUT").map_or(true, |value| value != "0")
            && std::env::var("YAS_MEDIA_MICROPHONE").map_or(true, |value| value != "0");
        let camera = self.audio_pipeline.is_some()
            && std::env::var("YAS_MEDIA_INPUT").map_or(true, |value| value != "0")
            && std::env::var("YAS_MEDIA_CAMERA").map_or(true, |value| value != "0");
        let mut screencasts = self
            .screencasts
            .values()
            .map(|session| MediaBackendScreencast {
                session_id: session.session_id,
                app_id: session.app_id.clone(),
                surface_ids: session
                    .streams
                    .iter()
                    .map(|stream| stream.surface_id)
                    .collect(),
            })
            .collect::<Vec<_>>();
        screencasts.sort_unstable_by_key(|session| session.session_id);
        MediaBackendState {
            pipewire_available: self.audio_pipeline.is_some(),
            microphone_available: microphone,
            camera_available: camera,
            screencasts,
        }
    }

    fn mark_pixel_snapshot_dirty(&mut self) {
        self.pixel_snapshot_dirty = true;
    }

    fn pixel_snapshots(&mut self) -> (Arc<Vec<PixelSnapshot>>, Arc<Vec<PixelSnapshot>>) {
        if self.pixel_snapshot_dirty {
            let snapshot = Arc::make_mut(&mut self.pixel_snapshot);
            snapshot.clear();
            snapshot.extend(self.last_pixels.iter().map(|(&(sid, _, _), lp)| {
                (
                    sid,
                    lp.width,
                    lp.height,
                    lp.generation,
                    lp.timestamp_ms,
                    lp.timestamp_sub_us,
                )
            }));
            // Opaque-only targets have no entry in `last_pixels`; add them to
            // the general index so the encode loop still sees the target.
            snapshot.extend(
                self.last_opaque_pixels
                    .iter()
                    .filter_map(|(&(sid, w, h), lp)| {
                        (!self.last_pixels.contains_key(&(sid, w, h))).then_some((
                            sid,
                            lp.width,
                            lp.height,
                            lp.generation,
                            lp.timestamp_ms,
                            lp.timestamp_sub_us,
                        ))
                    }),
            );
            let opaque = Arc::make_mut(&mut self.opaque_pixel_snapshot);
            opaque.clear();
            opaque.extend(self.last_opaque_pixels.iter().map(|(&(sid, _, _), lp)| {
                (
                    sid,
                    lp.width,
                    lp.height,
                    lp.generation,
                    lp.timestamp_ms,
                    lp.timestamp_sub_us,
                )
            }));
            self.pixel_snapshot_dirty = false;
        }
        (
            Arc::clone(&self.pixel_snapshot),
            Arc::clone(&self.opaque_pixel_snapshot),
        )
    }

    /// Hand a resize to the compositor and open a fresh settle window.
    fn dispatch_resize(
        &mut self,
        surface_id: u16,
        width: u16,
        height: u16,
        scale_120: u16,
        now: Instant,
    ) {
        self.pending_resize.remove(&surface_id);
        self.last_configured_size
            .insert(surface_id, (width, height, scale_120));
        self.last_resize_at.insert(surface_id, now);
        self.resize_inflight.insert(surface_id, now);
        let _ = self
            .handle
            .command_tx
            .send(CompositorCommand::SurfaceResize {
                surface_id,
                width,
                height,
                scale_120,
            });
        // Commands are only drained at the top of the compositor's event
        // loop, which is otherwise parked in `dispatch()` for up to a
        // second.  Every other command site wakes it; this one did not, so
        // a configure sat in the queue until something unrelated ran the
        // loop — a Wayland event, the next blanket `RequestFrame`, or a
        // pointer/key event from the very surface being resized.  That last
        // one is why a resize looked like it only took effect once you
        // interacted with the window.
        self.handle.wake();
    }

    /// Where a size already decided on is taking this surface, while that is
    /// still worth waiting for.
    ///
    /// Either a resize held for its settle window or one on the wire the
    /// compositor hasn't answered yet — both say the surface is leaving its
    /// current size, which is all an encoder needs to know not to be built
    /// for it.  `None` once the surface is where it was told to go, and
    /// `None` again after `RESIZE_ENCODER_GRACE`: a client that never acks
    /// its configure must not be able to hold its viewers on a frozen
    /// picture.
    fn resize_destination(&self, surface_id: u16, now: Instant) -> Option<(u16, u16, u16)> {
        // Both cases follow a dispatch — a held resize is one that arrived
        // inside another's window — so the window's opening is the clock for
        // both.
        let opened = *self.last_resize_at.get(&surface_id)?;
        if now.duration_since(opened) >= RESIZE_ENCODER_GRACE {
            return None;
        }
        if let Some(&held) = self.pending_resize.get(&surface_id) {
            return Some(held);
        }
        if !self.resize_inflight.contains_key(&surface_id) {
            return None;
        }
        self.last_configured_size.get(&surface_id).copied()
    }

    /// Dispatch every held-back resize whose settle window has closed.
    /// Returns the earliest instant at which a still-held resize comes due,
    /// so the delivery loop can park until exactly then.
    fn flush_due_resizes(&mut self, now: Instant) -> Option<Instant> {
        if self.pending_resize.is_empty() {
            return None;
        }
        let mut next: Option<Instant> = None;
        let sids: Vec<u16> = self.pending_resize.keys().copied().collect();
        for sid in sids {
            let due_at = self
                .last_resize_at
                .get(&sid)
                .map(|&t| t + SURFACE_RESIZE_SETTLE);
            match due_at {
                Some(due) if due > now => {
                    next = Some(next.map_or(due, |n: Instant| n.min(due)));
                }
                _ => {
                    if let Some(&(w, h, s)) = self.pending_resize.get(&sid) {
                        self.dispatch_resize(sid, w, h, s, now);
                    }
                }
            }
        }
        next
    }
}

fn encode_rgba_to_png(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let expected = (width as usize) * (height as usize) * 4;
        let actual = pixels.len();
        if actual != expected {
            // Size mismatch — return a 1×1 red pixel PNG rather than panicking.
            let mut encoder = png::Encoder::new(&mut buf, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 0, 0, 255]).unwrap();
            eprintln!(
                "[capture] pixel buffer size mismatch: {width}x{height} expected {expected} got {actual}"
            );
        } else {
            let mut encoder = png::Encoder::new(&mut buf, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
    }
    buf
}

#[cfg(target_os = "linux")]
fn screencast_thumbnail(frame: &LastPixels) -> Vec<u8> {
    const MAX_THUMBNAIL_BYTES: usize = 64 * 1024;
    let rgba = frame.pixels.to_rgba(frame.width, frame.height);
    let Some(image) = image::RgbaImage::from_raw(frame.width, frame.height, rgba) else {
        return Vec::new();
    };
    for &(max_width, max_height) in &[(256, 144), (192, 108), (128, 72), (96, 54), (64, 36)] {
        let resized = image::imageops::thumbnail(&image, max_width, max_height);
        let png = encode_rgba_to_png(&resized, resized.width(), resized.height());
        if png.len() <= MAX_THUMBNAIL_BYTES {
            return png;
        }
    }
    Vec::new()
}

#[cfg(target_os = "linux")]
fn max_screencast_streams() -> usize {
    static VALUE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("YAS_SCREENCAST_MAX_STREAMS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(4)
            .min(4)
    })
}

#[cfg(target_os = "linux")]
fn bounded_portal_text(value: &str) -> String {
    let maximum = yas_desktop::MPRIS_STRING_MAX;
    let mut out = String::with_capacity(value.len().min(maximum));
    for ch in value.chars().filter(|ch| !ch.is_control()) {
        if out.len() + ch.len_utf8() > maximum {
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(target_os = "linux")]
fn screencast_candidates(compositor: &SharedCompositor) -> Vec<yas_desktop::ScreenCastCandidate> {
    const THUMBNAIL_SOURCE_PIXELS_MAX: u64 = 1920 * 1080;
    const THUMBNAIL_COUNT_MAX: usize = 8;
    let mut surfaces = compositor.surfaces.values().collect::<Vec<_>>();
    surfaces.sort_unstable_by_key(|surface| surface.surface_id);
    // Leave one MiB for candidate identities, dimensions, length fields, and
    // common prompt data so the complete wire message remains below 4 MiB.
    let mut thumbnail_budget =
        yas_wire::media::Limits::HARD.max_portal_metadata_bytes as usize - 1024 * 1024;
    let mut thumbnail_count = 0usize;
    surfaces
        .into_iter()
        .filter(|surface| compositor.native_sizes.contains_key(&surface.surface_id))
        .take(yas_wire::media::Limits::HARD.max_screencast_candidates as usize)
        .filter_map(|surface| {
            // SurfaceCreated precedes the first mapped buffer and carries a
            // 0x0 placeholder. Only a native-size event proves this is a
            // mapped toplevel eligible for the portal chooser.
            let (width, height) = compositor.native_sizes.get(&surface.surface_id).copied()?;
            let width = u16::try_from(width).ok()?.max(1);
            let height = u16::try_from(height).ok()?.max(1);
            // Thumbnail conversion and PNG encoding happen while the shared
            // compositor snapshot is borrowed. Bound both source work and
            // count so a chooser containing many huge windows cannot stall
            // every viewer. Candidates without thumbnails remain selectable.
            let frame = (thumbnail_count < THUMBNAIL_COUNT_MAX
                && u64::from(width) * u64::from(height) <= THUMBNAIL_SOURCE_PIXELS_MAX)
                .then(|| {
                    compositor.last_pixels.get(&(
                        surface.surface_id,
                        u32::from(width),
                        u32::from(height),
                    ))
                })
                .flatten();
            let thumbnail_png = frame
                .map(|frame| {
                    thumbnail_count += 1;
                    screencast_thumbnail(frame)
                })
                .filter(|png| png.len() <= thumbnail_budget)
                .unwrap_or_default();
            thumbnail_budget = thumbnail_budget.saturating_sub(thumbnail_png.len());
            Some(yas_desktop::ScreenCastCandidate {
                surface_id: surface.surface_id,
                width,
                height,
                title: bounded_portal_text(&surface.title),
                app_id: bounded_portal_text(&surface.app_id),
                thumbnail_png,
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
struct ScreenCastRetirement {
    closed_sessions: Vec<u32>,
    state_changed: bool,
}

#[cfg(target_os = "linux")]
fn retire_screencast_surface(
    compositor: &mut SharedCompositor,
    surface_id: u16,
) -> ScreenCastRetirement {
    let mut removed_stream = false;
    for session in compositor.screencasts.values_mut() {
        let before = session.streams.len();
        session
            .streams
            .retain(|stream| stream.surface_id != surface_id);
        removed_stream |= session.streams.len() != before;
    }
    let closed = compositor
        .screencasts
        .iter()
        .filter_map(|(&session_id, session)| session.streams.is_empty().then_some(session_id))
        .collect::<Vec<_>>();
    for session_id in &closed {
        compositor.screencasts.remove(session_id);
    }
    if removed_stream {
        let _ = compositor
            .handle
            .command_tx
            .send(CompositorCommand::SetScreenCastActive {
                surface_id,
                active: false,
            });
        compositor.handle.wake();
        compositor.frame_clocks_dirty = true;
    }
    ScreenCastRetirement {
        closed_sessions: closed,
        state_changed: removed_stream,
    }
}

/// Encode RGBA pixels to AVIF.  `quality` 0 = lossless, 1–100 = lossy.
fn encode_rgba_to_avif(pixels: &[u8], width: u32, height: u32, quality: u8) -> Vec<u8> {
    let rgba: Vec<rgb::RGBA8> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| rgb::RGBA8::new(c[0], c[1], c[2], c[3]))
        .collect();
    let img = ravif::Img::new(&rgba[..], width as usize, height as usize);
    let q = if quality == 0 { 100.0 } else { quality as f32 };
    let encoder = ravif::Encoder::new()
        .with_quality(q)
        .with_alpha_quality(q)
        .with_speed(6)
        .with_alpha_color_mode(ravif::AlphaColorMode::UnassociatedClean)
        .with_num_threads(None);
    let result = encoder.encode_rgba(img).expect("AVIF encoding failed");
    result.avif_file
}

/// Encode RGBA pixels to the requested capture format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureEncoding {
    Png,
    Avif,
}

fn encode_capture(
    pixels: &[u8],
    width: u32,
    height: u32,
    format: CaptureEncoding,
    quality: u8,
) -> Vec<u8> {
    match format {
        CaptureEncoding::Avif => encode_rgba_to_avif(pixels, width, height, quality),
        CaptureEncoding::Png => encode_rgba_to_png(pixels, width, height),
    }
}

/// Whether a target may be published as a GPU-only NV12 `OPAQUE_FD`
/// buffer.
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DownscaleTargetMode {
    /// At least one NVENC reader can consume this opaque layout.
    want_nv12_opaque: bool,
    /// At least one reader needs host-visible BGRA pixels as well.
    want_cpu_pixels: bool,
    /// Layout shared by the opaque readers, when `want_nv12_opaque`.
    opaque_is_444: bool,
}

/// Representations the compositor must publish for one `(surface, w, h)`.
///
/// CPU and NVENC readers can coexist: the compositor keeps the BGRA staging
/// copy for the former and additionally publishes its GPU-converted opaque
/// NV12/NV24 buffer for the latter. A split between 4:2:0 and 4:4:4 NVENC
/// sessions still has no single opaque layout, so that rarer combination
/// stays on the CPU-compatible representation.
///
/// `others` yields each *other* subscriber's `(target, can_take_nv12,
/// wants_444, needs_cpu_pixels)`. A subscriber at a different size is
/// irrelevant. Vulkan Video needs neither published representation: it reads
/// the shared BGRA scratch inside the compositor.
fn downscale_target_mode(
    this_wants: bool,
    this_444: bool,
    this_needs_cpu: bool,
    target: (u32, u32),
    others: impl Iterator<Item = (Option<(u32, u32)>, bool, bool, bool)>,
) -> DownscaleTargetMode {
    let mut opaque_layout = this_wants.then_some(this_444);
    let mut want_cpu_pixels = this_needs_cpu;
    let mut split_opaque_layout = false;

    for (their_target, they_want, their_444, they_need_cpu) in others {
        if their_target != Some(target) {
            continue;
        }
        if they_need_cpu {
            want_cpu_pixels = true;
        }
        if !they_want {
            continue;
        }
        match opaque_layout {
            Some(layout) if layout != their_444 => split_opaque_layout = true,
            None => opaque_layout = Some(their_444),
            _ => {}
        }
    }

    if split_opaque_layout {
        DownscaleTargetMode {
            want_nv12_opaque: false,
            want_cpu_pixels: true,
            opaque_is_444: false,
        }
    } else {
        DownscaleTargetMode {
            want_nv12_opaque: opaque_layout.is_some(),
            want_cpu_pixels,
            opaque_is_444: opaque_layout.unwrap_or(false),
        }
    }
}

async fn request_surface_capture_with_timeout(
    command_tx: std::sync::mpsc::SyncSender<CompositorCommand>,
    surface_id: u16,
    scale_120: u16,
    timeout: Duration,
) -> Option<(u32, u32, Vec<u8>)> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    command_tx
        .try_send(CompositorCommand::Capture {
            surface_id,
            scale_120,
            reply: tx,
        })
        .ok()?;

    // The compositor replies through a blocking std::sync::mpsc channel.
    // Wait for it off the async runtime so this request never stalls the
    // tokio worker thread or holds the Session mutex while blocked.
    tokio::task::spawn_blocking(move || rx.recv_timeout(timeout))
        .await
        .ok()?
        .ok()
        .flatten()
}

/// Per-surface bookkeeping for an active subscription.  Every field
/// defaults to "no-op" so a fresh `entry(sid).or_default()` is safe
/// even before any other state has been recorded.
struct PendingSurfaceEncode {
    target_w: u32,
    target_h: u32,
    pixels: yas_compositor::PixelData,
    needs_keyframe: bool,
    /// Unlike keyframe debt, a still-image refinement is a separate explicit
    /// refresh request and must not be silently settled by an earlier frame.
    force_quality_refresh: bool,
    generation: u64,
    timestamp_ms: u32,
    timestamp_sub_us: u16,
}

fn chained_encode_needs_keyframe(
    pending_needs_keyframe: bool,
    force_quality_refresh: bool,
    completed_is_keyframe: bool,
) -> bool {
    pending_needs_keyframe && (!completed_is_keyframe || force_quality_refresh)
}

fn pending_generation_is_newer(
    in_flight_generation: Option<u64>,
    pending_generation: Option<u64>,
    candidate_generation: u64,
) -> bool {
    in_flight_generation.is_none_or(|generation| candidate_generation > generation)
        && pending_generation.is_none_or(|generation| candidate_generation > generation)
}

fn source_generation_is_still(sub: &mut SurfaceSubState, generation: u64, now: Instant) -> bool {
    if sub.observed_source_generation != Some(generation) {
        sub.observed_source_generation = Some(generation);
        sub.source_generation_changed_at = Some(now);
        return false;
    }
    sub.source_generation_changed_at
        .is_some_and(|changed_at| now.duration_since(changed_at) >= STILL_REFRESH_INTERVAL)
}

/// Minimum spacing between keyframe requests made by repeating an otherwise
/// identical surface subscription. A decoder recovery loop can repeat the
/// request continuously, so let the first request create the debt and then
/// coalesce repeats until the client has had enough time to receive and try
/// the resulting keyframe.
const SURFACE_KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_secs(2);

/// Record a new keyframe episode for one subscription.
///
/// `force` is used for first subscriptions and preference changes, which
/// always invalidate the existing reference chain.  Otherwise the request is
/// a same-preference decoder recovery and is both idempotent while a keyframe
/// is already owed and rate-limited after one was requested recently.
fn request_surface_keyframe(sub: &mut SurfaceSubState, now: Instant, force: bool) -> bool {
    if !force
        && (!sub.has_keyframe
            || sub
                .last_keyframe_request_at
                .is_some_and(|at| now.duration_since(at) < SURFACE_KEYFRAME_REQUEST_INTERVAL))
    {
        return false;
    }

    sub.last_keyframe_request_at = Some(now);
    sub.has_keyframe = false;
    sub.observed_source_generation = None;
    sub.source_generation_changed_at = None;
    // A fresh decoder/keyframe episode must not inherit pressure reported by
    // the decoder it replaces.  Suppressed duplicate requests, however,
    // leave that signal intact so a spammy client cannot disable pacing.
    sub.decoder_queue_depth = 0;
    sub.decoder_pressure_depth = 0;
    sub.decoder_queue_high_since = None;
    true
}

#[derive(Default)]
struct SurfaceSubState {
    /// Active encoder for this surface.  `None` between encode jobs
    /// while the encoder is temporarily owned by the spawn_blocking
    /// task (see `encode_in_flight`) or before the first encode.
    encoder: Option<SurfaceEncoder>,
    /// Whether this subscriber's encoder can read a GPU-only NV12
    /// `OPAQUE_FD` buffer (i.e. is NVENC).
    ///
    /// Recorded rather than asked of `encoder` on demand, because that
    /// field is `None` while an encode task owns it — and a subscriber
    /// missed during that window would be read as "can take NV12" and
    /// handed a buffer it cannot map.
    wants_nv12_opaque: bool,
    /// The OPAQUE_FD layout this subscriber's session consumes (444 =
    /// planar YUV444).  Meaningful only with `wants_nv12_opaque`; recorded
    /// for the same reason.
    wants_opaque_444: bool,
    /// Next tick this surface may send a frame (pacing deadline).
    next_send_at: Option<Instant>,
    /// Actual shared compositor clock for this surface. Delivery needs a
    /// second cadence gate only when another viewer drives the source faster
    /// than this subscription requested.
    source_interval: Option<Duration>,
    /// Frames remaining in the post-subscribe burst window that
    /// bypass time-based pacing so bandwidth estimates ramp up fast
    /// on high-latency links.
    burst_remaining: u8,
    /// WebCodecs decodeQueueSize reported with the latest ACK. This is the
    /// decoder-pressure signal; aggregate ACKed bytes separately control the
    /// client-wide transport credit without treating callback age as RTT.
    decoder_queue_depth: u8,
    /// Queue depth admitted as sustained decoder pressure.  A raw
    /// `decodeQueueSize` spike can be a batch of `decode()` calls issued
    /// after the JavaScript event loop wakes; pacing from that instantaneous
    /// value needlessly drops frames on an otherwise idle local link.
    decoder_pressure_depth: u8,
    /// Start of the current continuously-high decoder-queue episode.
    decoder_queue_high_since: Option<Instant>,
    /// Maximum complete frames this subscriber permits in flight. Native
    /// Surface views set this from their negotiated decoder window; `None`
    /// leaves legacy subscribers governed only by byte credit.
    max_inflight_frames: Option<usize>,
    /// True while an encoder-creation spawn_blocking task is running
    /// for this surface.  Prevents dispatching a second creation in
    /// parallel and (via the `needs_new_encoder` path) skips encode
    /// dispatch until the creation task lands its result.
    creation_in_flight: bool,
    /// True while this surface's encoder is in an encode spawn_blocking
    /// task.  Prevents dispatching a parallel encode for the same
    /// surface (the encoder has been moved into the task).
    encode_in_flight: bool,
    /// Estimated wire bytes reserved by the active encode. Credit is taken
    /// before the blocking task starts so several surfaces becoming ready in
    /// one delivery tick cannot all overshoot the shared client window.
    reserved_encode_bytes: usize,
    /// Pixel generation owned by the current encode task. The delivery loop
    /// can revisit the same compositor snapshot many times before that task
    /// finishes; without this marker it queues the same pixels for an
    /// immediate second encode.
    in_flight_generation: Option<u64>,
    /// Newest full-rate source frame that arrived while the ordered encoder
    /// was busy. On completion the same encoder consumes this immediately,
    /// avoiding a round trip through the large delivery tick between every
    /// pair of frames. One slot is intentional: when an encoder genuinely
    /// cannot sustain the source rate, freshest wins.
    pending_encode: Option<PendingSurfaceEncode>,
    /// Set if the in-flight encoder was invalidated by a codec /
    /// bandwidth / speed change (resubscribe) while encoding — the completion
    /// handler must drop the stale encoder instead of reinserting it.
    encoder_invalidated: bool,
    /// When this subscriber first declined BGRA while waiting for its
    /// `OPAQUE_FD` publish. After `OPAQUE_PUBLISH_GRACE` this becomes the
    /// rate limit for re-registering and recompositing a missing target.
    /// Cleared when the GPU representation arrives.
    opaque_wait_since: Option<Instant>,
    /// This client holds a decodable keyframe for this surface, so a delta
    /// frame is safe to send.  Cleared whenever the reference chain breaks
    /// or becomes unknown: encoder rebuilt or lost, surface resized,
    /// resubscribe with changed preferences, a send that failed, a Vulkan
    /// session withdrawn.  `false` — the default — means the next frame
    /// this surface sends must be a keyframe, which is right for a
    /// subscription that has never been sent one: it cannot decode a delta.
    ///
    /// Per surface, not per client.  A client watching several surfaces has
    /// an independent reference chain for each, and one surface's keyframe
    /// says nothing about another's.
    has_keyframe: bool,
    /// When a first/preference-change/recovery subscribe most recently
    /// created keyframe debt.  Same-preference repeats are coalesced against
    /// this so a broken client cannot turn every frame into an IDR.
    last_keyframe_request_at: Option<Instant>,
    /// Pixel generation that was last encoded; used to skip re-
    /// encoding identical pixel data on subsequent ticks.
    last_encoded_gen: Option<u64>,
    /// Latest compositor pixel generation observed for stillness detection,
    /// and when it first became current. The delivery loop revisits each
    /// generation several times at high refresh rates, so equality with
    /// `last_encoded_gen` alone does not mean the application stopped.
    observed_source_generation: Option<u64>,
    source_generation_changed_at: Option<Instant>,
    /// Consecutive `nal_data=None` encodes.  After too many, the
    /// encoder is dropped so a fresh one is created on the next tick
    /// (bounds runaway encoder-recreation loops).
    nal_none_streak: u32,
    /// When the streak last latched (hit the drop threshold).  Auto-
    /// clears after a backoff so a freshly-created encoder can retry
    /// without needing a user-driven resize/resubscribe.
    nal_none_latched_at: Option<Instant>,
    /// Consecutive encoder creations that came back with nothing, cleared
    /// by the first that succeeds.
    ///
    /// A failure at a size some backend *could* have carried is retried at
    /// that size rather than shrinking the surface, since the usual cause
    /// is momentary.  This counts how long "momentary" has gone on: past
    /// [`CREATE_FAILURES_BEFORE_DEGRADE`] the surface comes down to what
    /// the whole chain clears, because a smaller picture beats none.
    create_failures: u32,
    /// Per-surface codec support override from Surface Subscribe.
    /// (bitmask of CODEC_SUPPORT_*).  0 = defer to client-wide
    /// `surface_codec_support`.
    codec_override: u8,
    /// Per-surface bandwidth override.  `None` = use server default.
    bandwidth_override: Option<SurfaceBandwidth>,
    /// Per-surface speed override.  `None` = use server default.
    speed_override: Option<SurfaceSpeed>,
    /// Fixed encode size this client asked for on Surface Subscribe.
    ///
    /// `Some` opts the subscription out of surface-size mediation entirely:
    /// the compositor surface keeps whatever size the *mediated* viewers
    /// want, and this client is served a server-side downscale of it.  That
    /// is the whole point — a side-panel thumbnail can ask for a card-sized
    /// stream without dragging the Wayland window down to a card for
    /// everyone watching it full size.
    ///
    /// `None` — the default — means the client participates in mediation via
    /// Surface Resize like any other viewer.
    scaled_target: Option<(u16, u16)>,
    /// `scaled_target` is normally literal (for previews), but native Surface
    /// views also use it to name their independent physical viewport. Those
    /// views opt into transport downscaling while preserving that requested
    /// box as the upper bound.
    allow_adaptive_scale: bool,
    /// Explicit cadence ceiling from Surface Subscribe. `None` uses the
    /// client's display rate; thumbnails set a lower value without changing
    /// the cadence of a full-size view of another surface.
    max_fps: Option<f32>,
    /// EWMA of this surface's encoded frame size in bytes.  Per surface
    /// (unlike `avg_surface_frame_bytes`) so a client watching two
    /// surfaces can split its bandwidth budget between them.  0 = no
    /// frame measured yet.
    frame_bytes: f32,
    /// Quantizer the adaptive controller is currently asking for.  `None`
    /// = run at the ceiling (`bandwidth_override` / server default).
    adaptive_quantizer: Option<u8>,
    /// When the controller last moved `adaptive_quantizer`, for hysteresis.
    rate_stepped_at: Option<Instant>,
    /// Most recent direct pressure signal for this surface.  Keep the
    /// backed-off quantizer latched briefly after a blocked write clears;
    /// otherwise a saturated link alternates between one expensive stall
    /// and an immediate walk back to maximum quality.
    congested_at: Option<Instant>,
    /// Power-of-two server-side downscale applied after the ordinary
    /// per-viewer target is chosen. This is transport adaptation only: it
    /// never changes the compositor's logical size or the coordinate space
    /// used for pointer input.
    adaptive_scale_shift: u8,
    /// Last time adaptive delivery changed the encoded extent.
    scale_stepped_at: Option<Instant>,
    /// Most recent direct transport or decoder pressure. Resolution recovery
    /// probes are measured from this; an ACK window merely being full is flow
    /// control, not evidence that the path or decoder is overloaded.
    adaptive_pressure_at: Option<Instant>,
    /// Bit per Vulkan Video encoder whose 4:2:0 profile the compositor has
    /// refused for this client and surface (see
    /// [`SurfaceEncoderPreference::vulkan_refusal_bit`]). Latched at one
    /// encoded extent so selection stops offering that codec after both
    /// chroma profiles are exhausted. A different extent gets a fresh try:
    /// profile creation and encode can fail solely because the old extent was
    /// below the device minimum, above its maximum, or otherwise unsupported.
    ///
    /// Per encoder rather than one flag for the tier: with `av1-vulkan` ahead
    /// of `h264-vulkan` in the default list, a single flag let an AV1 refusal
    /// disqualify H.264 too, losing a path that works.
    vulkan_refused: u8,
    /// Bit per Vulkan Video encoder whose 4:4:4 profile the compositor refused
    /// for this subscription.  Unlike `vulkan_refused`, this does not reject
    /// the backend: selection retries the same codec at 4:2:0 before advancing
    /// to the next preference.
    vulkan_444_refused: u8,
    /// Encoded extent the two Vulkan refusal masks describe. Selection clears
    /// both masks before trying a different extent; without this, a tiny
    /// thumbnail that is below the device minimum permanently condemns the
    /// later full-size pane to a server-side fallback encoder.
    vulkan_refused_extent: Option<(u32, u32)>,
    /// Extent at which every server-side encoder ranked above Vulkan was
    /// attempted and failed. This admits Vulkan on the next tick even when a
    /// predecessor works generally but refused this particular frame shape.
    vulkan_predecessors_exhausted_extent: Option<(u32, u32)>,
    /// Last per-client downscale target dims registered with the
    /// compositor.  Used to send `ClearDownscaleTarget` for the old
    /// dims when the encoder is recreated at a new size, so stale
    /// downscale outputs don't accumulate in the compositor.  `None`
    /// = no target registered yet (or the encoder was an external
    /// GBM path that uses `external_outputs` instead).
    last_registered_target: Option<(u32, u32)>,
    /// The compositor native size `last_registered_target` was inscribed
    /// into, as the compositor has it stamped.  The compositor refuses to
    /// fill a target whose stamp no longer matches what it is compositing,
    /// so this has to be refreshed whenever the native moves — including
    /// when the target itself lands on the same numbers as before and the
    /// encoder is therefore not rebuilt.  Without that, a surface nudged a
    /// pixel by *another* viewer's resize would leave this one's target
    /// stamped for a size that will never come back, and it would stop
    /// receiving frames entirely.
    last_registered_native: Option<(u32, u32)>,
    /// Which preference won the fallback chain for this surface, once one
    /// has.  Sizing prefers it over guessing: before an encoder exists we
    /// size for the most capable backend the client could decode, and this
    /// replaces that guess with the answer.  `None` = no encoder built yet.
    selected_encoder: Option<SurfaceEncoderPreference>,
    /// Latched when a creation attempt was refused for being too large.
    /// Sizes the next attempt to the ceiling *every* backend in the chain
    /// clears, so a surface no wide-format encoder can carry still gets a
    /// picture instead of retrying the same oversized request forever.
    ///
    /// Cleared on a prefs-changed resubscribe, deliberately *not* on the
    /// smaller creation that follows the refusal: that creation can be won by
    /// a backend whose own ceiling is wider than the size just refused, and
    /// clearing here would size the surface straight back up into it.  See
    /// the creation completion handler.
    encoder_cap_degraded: bool,
    /// When we last asked the compositor to recomposite because the pixel
    /// cache had no entry at this subscription's encode target.  Throttles
    /// the request — the dispatch loop retries at frame rate, and one
    /// recomposite is enough to refill the cache.
    recomposite_requested_at: Option<Instant>,
}

/// The codec bitmask in force for one (client, surface) pair: the
/// per-surface override from Surface Subscribe when set, else the
/// client-wide value.  0 means "accept anything".
fn surface_codec_support(client: &ClientState, surface_id: u16) -> u8 {
    client
        .surface_subs
        .get(&surface_id)
        .map(|s| s.codec_override)
        .filter(|&c| c != 0)
        .unwrap_or(client.surface_codec_support)
}

/// How large a frame this client may be served for `surface_id`.
///
/// The encoder ceiling is not a property of the chain as a whole — H.264
/// stops at 3840x2160 and hardware AV1 goes to 8192x4352 — so taking the
/// tightest cap across every configured preference would hold an AV1 viewer
/// to H.264's limit purely because H.264 is in the list as a fallback.
/// Instead:
///
///   - Once the chain has resolved, the winner's own ceiling is the truth.
///   - Before that, size for the most capable backend the client can decode
///     and let `SurfaceEncoder::new` skip the ones that can't carry it.
///   - If that request was refused for size, fall back to the ceiling every
///     eligible backend clears.  This is the one case that costs a round
///     trip, and it converges after exactly one.
///
/// The result is then intersected with what the client said its decoder can
/// handle, because a ceiling the encoder clears is worthless if the browser
/// refuses the bitstream.
///
/// `None` (empty or fully-ineligible preference list) means no cap.
fn surface_encode_cap(
    prefs: &[SurfaceEncoderPreference],
    client: &ClientState,
    surface_id: u16,
) -> Option<(u16, u16)> {
    let codec_support = surface_codec_support(client, surface_id);
    let sub = client.surface_subs.get(&surface_id);
    let eligible: Vec<_> = prefs
        .iter()
        .copied()
        .filter(|p| p.supported_by_client(codec_support))
        .collect();
    let selected = sub.and_then(|s| s.selected_encoder);
    let (dw, dh) = match client.surface_max_decode {
        // Undeclared: hold at the H.264 ceiling. This is the conservative
        // limit for any session that omits an explicit decoder maximum.
        (0, 0) => SurfaceEncoderPreference::H264Software.max_dimensions(),
        declared => declared,
    };
    let (cw, ch) = if sub.is_some_and(|s| s.encoder_cap_degraded) {
        SurfaceEncoderPreference::tightest_for_list(&eligible)
    } else if let Some(pref) = selected {
        // Software AV1's performance limit is a pixel budget, not a 16:9
        // rectangle. Preserve an already-declared non-16:9 target when its
        // total work fits that budget instead of forcing it through 2160p.
        if pref == SurfaceEncoderPreference::AV1Software && pref.fits(u32::from(dw), u32::from(dh))
        {
            Some((dw, dh))
        } else {
            Some(pref.max_dimensions())
        }
    } else {
        SurfaceEncoderPreference::widest_for_list(&eligible)
    }?;
    Some((cw.min(dw), ch.min(dh)))
}

/// How many creations in a row may come back empty at a size some backend
/// could have carried before the surface is brought down anyway.
///
/// Failures there are usually momentary — an allocation, a busy engine, a
/// compositor buffer not imported yet — and retrying at the same size keeps
/// the viewer's resolution.  But a backend can also fail only at scale (VRAM
/// for a 5K frame, a per-resolution driver limit the reported maximum does
/// not admit to) and go on doing it, and then holding out for the large size
/// means holding out forever.  Retries are spaced by
/// `NAL_NONE_RETRY_BACKOFF`, so this is a few seconds of black at worst.
const CREATE_FAILURES_BEFORE_DEGRADE: u32 = 3;

/// Whether a failed encoder creation should narrow this surface's ceiling
/// rather than simply be tried again.
///
/// True only when nothing is left that could have carried the frame: every
/// backend the client can decode and this host can run is too small for it.
/// Then a smaller surface is the only way to a picture, and the caller
/// latches `encoder_cap_degraded`.
///
/// The distinction matters because that latch does not clear until the
/// client resubscribes.  If a backend fits the frame and works on this host,
/// its failure was a momentary one — an allocation, a busy engine — and
/// another attempt at the same size is the right answer; treating it as a
/// size problem would pin the viewer to 2160p for the rest of the session.
/// A backend that goes on failing anyway is caught by
/// [`CREATE_FAILURES_BEFORE_DEGRADE`] instead, so "momentary" cannot mean
/// "forever".
///
/// `available` reports whether a backend has ever built an encoder here; it
/// is a parameter so this stays a decision about the arguments rather than
/// about process-global state.
fn refused_for_size(
    prefs: &[SurfaceEncoderPreference],
    codec_support: u8,
    width: u32,
    height: u32,
    available: impl Fn(SurfaceEncoderPreference) -> bool,
) -> bool {
    !prefs
        .iter()
        .copied()
        .filter(|p| p.supported_by_client(codec_support))
        .filter(|p| available(*p))
        .any(|p| p.fits(width, height))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VulkanVideoSurfaceState {
    encoder_name: &'static str,
    codec_flag: u8,
    width: u32,
    height: u32,
    /// The profile requested from the compositor.  A refusal of 4:4:4 still
    /// leaves the same codec's 4:2:0 profile worth trying.
    is_444: bool,
}

struct ClientState {
    /// Microseconds the writer task has spent blocked inside a socket
    /// write, accumulated.  A blocked write is the earliest and cheapest
    /// congestion signal available; the bandwidth controller samples the
    /// delta between its steps rather than the absolute value.
    write_blocked_us: Arc<AtomicU64>,
    /// `write_blocked_us` as of the controller's last step, so it can read a
    /// delta out of a monotonically growing counter.
    write_blocked_us_seen: u64,
    /// Total length-prefixed bytes successfully written to this connection.
    outbound_bytes: Arc<AtomicU64>,
    /// Counter/timestamp pair used to derive actual recent server→client
    /// bandwidth for the live client catalog.
    outbound_bytes_seen: u64,
    outbound_sampled_at: Instant,
    outbound_bytes_per_sec: u64,
    /// Total length-prefixed bytes read *from* this connection, and the same
    /// counter/timestamp pair for the other direction of the catalog's
    /// bandwidth pair. The read loop owns the counter, so this is the only
    /// per-client accounting of what a client sends: a CLI reports nothing
    /// about itself, and neither does a browser.
    inbound_bytes: Arc<AtomicU64>,
    inbound_bytes_seen: u64,
    inbound_sampled_at: Instant,
    inbound_bytes_per_sec: u64,
    connected_at: Instant,
    /// What opened this connection, kept so the catalog can say so. Captured
    /// at accept time: an attempt is named by the definition it started from,
    /// not by whatever that definition says a minute later.
    origin: ConnectionOrigin,
    /// Whether this backend connection contributes a user-facing Client
    /// catalogue record.
    catalog_visible: bool,
    /// Exact Core HELLO identity when this backend connection owns a YAS
    /// session identity.
    native_identity: Option<NativeClientIdentity>,
    /// Direct typed YAS Surface delivery. Native views reuse the shared
    /// encoder and pacing state below.
    native_surface: Option<yas_surface_backend::Sink>,
    lead: Option<u16>,
    subscriptions: FxHashSet<u16>,
    /// Active surface subscriptions for this client.
    surface_subscriptions: FxHashSet<u16>,
    view_sizes: FxHashMap<u16, (u16, u16)>,
    scroll_offsets: FxHashMap<u16, usize>,
    scroll_caches: FxHashMap<u16, FrameState>,
    last_sent: FxHashMap<u16, FrameState>,
    last_used_rows_sent: FxHashMap<u16, u16>,
    preview_next_send_at: FxHashMap<u16, Instant>,
    /// EWMA RTT estimate in milliseconds.
    rtt_ms: f32,
    /// Minimum-path RTT estimate in milliseconds, excluding queue growth.
    min_rtt_ms: f32,
    /// Client's measured display refresh rate in frames per second.
    display_fps: f32,
    /// EWMA of delivered payload rate in bytes/sec.
    delivery_bps: f32,
    /// EWMA of actual ACKed goodput in bytes/sec, based on ACK cadence rather than RTT.
    goodput_bps: f32,
    /// EWMA of absolute goodput sample-to-sample jitter in bytes/sec.
    goodput_jitter_bps: f32,
    /// Decaying peak goodput jitter in bytes/sec.
    max_goodput_jitter_bps: f32,
    /// Last sampled ACK goodput for jitter estimation.
    last_goodput_sample_bps: f32,
    /// EWMA of acknowledged frame payload size in bytes.
    avg_frame_bytes: f32,
    /// EWMA of acknowledged lead/paced frame payload size in bytes.
    avg_paced_frame_bytes: f32,
    /// EWMA of acknowledged preview/unpaced frame payload size in bytes.
    avg_preview_frame_bytes: f32,
    /// EWMA of surface (video) frame payload size in bytes.  Tracked
    /// separately from terminal frame sizes so surface pacing uses
    /// `goodput_bps / avg_surface_frame_bytes` without polluting
    /// terminal congestion control estimates.
    avg_surface_frame_bytes: f32,
    /// Payload bytes currently in flight (sent, not yet ACKed).
    #[cfg(test)]
    inflight_bytes: usize,
    /// Oldest in-flight frame first; ACKs arrive in order.
    #[cfg(test)]
    inflight_frames: VecDeque<InFlightFrame>,
    /// Earliest time the next visual update should be sent for smooth pacing.
    next_send_at: Instant,
    /// Temporary additive window growth used to probe for more throughput after
    /// a conservative backoff. Decays when queue delay grows.
    probe_frames: f32,
    /// Diagnostics.
    frames_sent: u32,
    acks_recv: u32,
    acked_bytes_since_log: usize,
    browser_backlog_frames: u16,
    browser_ack_ahead_frames: u16,
    browser_apply_ms: f32,
    last_log: Instant,
    /// Throttle timestamp for "[surface-gate] blocked" diagnostic logs.
    last_window_blocked_log: Instant,
    /// Throttle timestamp for "[encode-skip]" diagnostic logs.
    last_skip_log: Instant,
    /// Counters for silent encode-skip paths, reset each pacing log tick.
    skip_same_gen_count: u32,
    skip_in_flight_count: u32,
    skip_pacing_count: u32,
    skip_vulkan_await_count: u32,
    /// Client had no subscriptions when encode pass ran.
    skip_no_subs_count: u32,
    /// Client not subscribed to a given sid in pixel_snapshot.
    skip_not_subbed_count: u32,
    /// last_pixels entry missing / dimensions mismatched pixel_snapshot.
    skip_last_pixels_mismatch_count: u32,
    /// Iterations through pixel_snapshot for this client (sanity check).
    encode_loop_iters: u32,
    goodput_window_bytes: usize,
    goodput_window_start: Instant,
    /// Conservative aggregate surface ACK rate. Unlike `goodput_bps`, this
    /// uses a longer window and fast-decrease/slow-increase smoothing because
    /// it directly controls how much video may be outstanding.
    surface_goodput_bps: f32,
    surface_goodput_window_bytes: usize,
    surface_goodput_window_start: Instant,
    /// Per-surface encode/pacing/override state.  Holds every piece of
    /// bookkeeping the encode loop maintains between frames for a
    /// subscribed surface.  Entries are created lazily via
    /// `entry(sid).or_default()` on first touch and dropped wholesale
    /// on UNSUBSCRIBE / SurfaceDestroyed.
    surface_subs: FxHashMap<u16, SurfaceSubState>,
    /// Surfaces that use Vulkan Video encoding in the compositor rather than
    /// a local SurfaceEncoder.
    vulkan_video_surfaces: FxHashMap<u16, VulkanVideoSurfaceState>,
    /// Surface frames in flight — separate from terminal inflight so surface
    /// ACKs feed delivery/goodput without corrupting terminal frame-size
    /// averages or probe_frames.
    surface_inflight_frames: VecDeque<SurfaceInFlightFrame>,
    /// Sum of `surface_inflight_frames[*].bytes`, kept separately because the
    /// delivery gate is hot and the tracking queue can be deep on WAN links.
    surface_inflight_bytes: usize,
    /// Last surface placed first in this client's delivery pass. The next
    /// pass starts after it so one busy surface cannot monopolise shared
    /// transport credit.
    surface_schedule_cursor: Option<u16>,
    /// Per-client desired surface sizes (surface_id → (width, height, scale_120, codec_support)).
    /// Unlike PTY grids, video surfaces can be downscaled per client: the
    /// server composites for the largest active logical view and serves
    /// smaller viewers from their own encode targets.
    /// `scale_120` is the requested presentation scale in 1/120th units:
    /// 60 = 0.5×, 120 = 1×, 240 = 2×. It may be the viewer's DPR or
    /// an exact scale selected independently of DPR.
    surface_view_sizes: FxHashMap<u16, (u16, u16, u16)>,
    /// When each unsubscribed claim in `surface_view_sizes` stops counting.
    /// Absent means the claim is live — the viewer is watching, or is within
    /// `SURFACE_CLAIM_GRACE` of having stopped.
    surface_claim_lapses: FxHashMap<u16, Instant>,
    /// Intersection of codec support across all surfaces for this client.
    /// Used to pick an encoder the client can decode.  0 = accept anything.
    surface_codec_support: u8,
    /// Largest frame this client's video decoder reported it can handle,
    /// from Core negotiation. `(0, 0)` = not declared, which covers
    /// every client predating the field; those are held to the H.264
    /// ceiling, the most they could have been served anyway.
    ///
    /// Separate from `surface_codec_support` because the two answer
    /// different questions: the bitmask says *which* codecs decode, this
    /// says *how large* they decode.  A browser that reports AV1 support
    /// from a 1080p probe has said nothing about 5K.
    surface_max_decode: (u16, u16),
    /// Evdev keycodes currently held down by this client on compositor
    /// surfaces.  On disconnect we send synthetic key-up events for each
    /// so modifiers don't stay stuck and keys don't auto-repeat forever.
    pressed_surface_keys: HashSet<u32>,
    /// This viewer requested direct-touch delivery rather than the browser's
    /// pointer/gesture emulation.
    direct_touch_enabled: bool,
    /// Browser contact identifiers currently down for this connection, each with
    /// its latest position in composited-frame pixels.  The positions are what
    /// the shared-input mirror draws on the other viewers, so they have to be the
    /// whole live set and not just the contacts a message changed.
    surface_touch_ids: HashMap<i32, TouchMark>,
}

#[cfg(target_os = "linux")]
fn mpris_player_at(
    player: &yas_desktop::MprisPlayer,
    position_observed_at: Instant,
    now: Instant,
) -> yas_desktop::MprisPlayer {
    let mut replay = player.clone();
    if replay.playback_status == yas_desktop::PlaybackStatus::Playing {
        let elapsed_us = now
            .saturating_duration_since(position_observed_at)
            .as_micros()
            .min(i64::MAX as u128) as i128;
        let delta = elapsed_us.saturating_mul(i128::from(replay.rate_ppm)) / 1_000_000;
        replay.position_us =
            (i128::from(replay.position_us) + delta).clamp(0, i128::from(i64::MAX)) as i64;
    } else {
        replay.position_us = replay.position_us.max(0);
    }
    if replay.length_us >= 0 {
        replay.position_us = replay.position_us.min(replay.length_us);
    }
    replay
}

fn clamp_cursor_rect((x, y, width, height): (i32, i32, i32, i32)) -> (i16, i16, i16, i16) {
    let clamp = |value: i32| value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    (
        clamp(x),
        clamp(y),
        clamp(width.max(0)),
        clamp(height.max(0)),
    )
}

/// Percent-encode every byte of `path` outside the RFC 3986 unreserved set
/// (plus `/`, which separates path segments) for use in a `file://` URI.
fn percent_encode_uri_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The `file://` URI (RFC 2483) for a staged drop file.
fn drag_file_uri(path: &std::path::Path) -> String {
    format!(
        "file://{}",
        percent_encode_uri_path(&path.to_string_lossy())
    )
}

#[cfg(test)]
struct InFlightFrame {
    sent_at: Instant,
    bytes: usize,
    paced: bool,
}

/// A surface frame handed to the writer, awaiting its Surface Ack.
/// Carries the surface id so an ack is matched to the frame it actually
/// acknowledges: with two surfaces subscribed the acks interleave, and
/// popping blindly would credit one surface's bytes with the other's
/// delivery time.
struct SurfaceInFlightFrame {
    sent_at: Instant,
    bytes: usize,
    surface_id: u16,
}

/// Floor on the unacked-surface-frame cap.  Also the whole cap on any
/// ordinary link: at 20 ms RTT and 60 Hz the window is about four frames,
/// so 64 is already enormous headroom.
const SURFACE_INFLIGHT_MIN: usize = 64;

/// Ceiling regardless of bandwidth-delay product, so a client reporting a
/// nonsense display rate or a wildly inflated RTT cannot grow the queue
/// without bound.  Each entry is a few dozen bytes, so even this is
/// kilobytes, not megabytes.
const SURFACE_INFLIGHT_HARD_MAX: usize = 8_192;

/// Queueing allowance above the measured minimum path RTT. Surface bytes are
/// reliable and ordered, so a larger window directly delays audio and input
/// once the link is saturated.
const SURFACE_CREDIT_QUEUE_SECS: f32 = 0.1;
/// Every fresh path gets a two-frame bootstrap so measured goodput can grow
/// beyond stop-and-wait. Restricting a frame larger than the initial measured
/// window to one in flight made that estimate self-fulfilling: one ACK per RTT
/// could never demonstrate that the path had room for a pipeline.
const SURFACE_CREDIT_BOOTSTRAP_FRAMES: usize = 2;
/// Surface ACK callbacks arrive in batches. Aggregate long enough to smooth
/// the JavaScript scheduling noise before changing the client-wide window.
const SURFACE_GOODPUT_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
/// First frames and recovery frames are normally much larger than deltas.
/// Reserving a realistic floor prevents all newly visible surfaces from
/// launching large keyframes in the same delivery tick.
const SURFACE_KEYFRAME_ESTIMATE_MIN_BYTES: usize = 256 * 1024;

/// Terminal previews are rendered as small thumbnails.  Driving every
/// background PTY at a high-refresh display's full cadence wastes snapshot,
/// compression, transport, and browser paint work without improving the menu.
const TERMINAL_PREVIEW_MAX_FPS: f32 = 15.0;

/// Cap on unacked surface frames tracked per client.  A frame can go
/// unacked forever (client teardown mid-flight, a transport that drops
/// it), and every orphan permanently offsets the queue — so the oldest
/// entries are evicted rather than trusted.
///
/// Derived from the full per-surface ACK-tracking window rather than fixed, and
/// multiplied by the number of subscribed surfaces.  Otherwise two 240 Hz
/// streams share a queue sized for one, evict each other's still-live ACK
/// records, corrupting their goodput samples.  Twice the window preserves
/// ordinary browser scheduling stalls while still bounding orphaned entries.
fn surface_inflight_cap(client: &ClientState) -> usize {
    let per_surface = surface_frame_window(client)
        .saturating_add(surface_ack_tracking_frames(client.display_fps.max(1.0)));
    let surfaces = client.surface_subscriptions.len().max(1);
    per_surface
        .saturating_mul(2)
        .saturating_mul(surfaces)
        .clamp(SURFACE_INFLIGHT_MIN, SURFACE_INFLIGHT_HARD_MAX)
}

/// Frames to keep in flight: enough to cover one RTT at the client's reported
/// display rate. High-latency links need many frames in flight to avoid
/// devolving into stop-and-wait.
fn frame_window(rtt_ms: f32, display_fps: f32) -> usize {
    let frame_ms = 1_000.0 / display_fps.max(1.0);
    let base_frames = (rtt_ms / frame_ms).ceil().max(0.0) as usize;
    let slack_frames = ((base_frames as f32) * 0.125).ceil() as usize + 2;
    base_frames.saturating_add(slack_frames).max(2)
}

fn path_rtt_ms(client: &ClientState) -> f32 {
    if client.min_rtt_ms > 0.0 {
        client.min_rtt_ms
    } else {
        client.rtt_ms
    }
}

fn display_need_bps(client: &ClientState) -> f32 {
    client.avg_paced_frame_bytes.max(256.0) * client.display_fps.max(1.0)
}

/// Pacing quantities derived from one immutable snapshot of a `ClientState`.
///
/// Every function in this cluster is a pure read of `ClientState`, and they
/// fan out heavily into each other: `target_byte_window` alone used to
/// evaluate `throughput_limited` six times, and one `window_open` about nine.
/// The delivery loop then evaluated the whole window four times per client
/// per tick on unchanged state.  Since `tick` is notify-driven — every PTY
/// output chunk wakes it — that constant cost scaled with terminal
/// throughput, and measured ~10% of server CPU under a firehose.
///
/// Computing the shared roots once and threading them through fixes the
/// blow-up without changing any result: nothing here mutates, so a snapshot
/// taken at the top of a window evaluation is exactly what each nested call
/// would have recomputed for itself.
///
/// The free functions below remain as thin wrappers that build a snapshot on
/// the spot, so cold callers and unit tests read the same as before.
#[derive(Clone, Copy)]
struct Pacing {
    throughput_limited: bool,
    browser_pacing_fps: f32,
    bandwidth_floor_bps: f32,
    path_rtt_ms: f32,
    /// `1_000.0 / browser_pacing_fps.max(1.0)` — used by nearly every
    /// consumer below.
    frame_ms: f32,
}

impl Pacing {
    fn new(client: &ClientState) -> Self {
        let browser_pacing_fps = browser_pacing_fps(client);
        let bandwidth_floor_bps = bandwidth_floor_bps(client);
        // Inline of `throughput_limited`, reusing the two roots above.
        let lead_bps = client.avg_paced_frame_bytes.max(256.0) * browser_pacing_fps;
        let preview_bps = client.avg_preview_frame_bytes.max(256.0)
            * client.display_fps.clamp(1.0, TERMINAL_PREVIEW_MAX_FPS);
        Self {
            throughput_limited: (lead_bps + preview_bps) > bandwidth_floor_bps * 0.9,
            browser_pacing_fps,
            bandwidth_floor_bps,
            path_rtt_ms: path_rtt_ms(client),
            frame_ms: 1_000.0 / browser_pacing_fps.max(1.0),
        }
    }

    fn effective_rtt_ms(&self, client: &ClientState) -> f32 {
        let queue_allowance = self.frame_ms * if self.throughput_limited { 4.0 } else { 12.0 };
        client
            .rtt_ms
            .clamp(self.path_rtt_ms, self.path_rtt_ms + queue_allowance)
    }

    fn window_rtt_ms(&self, client: &ClientState) -> f32 {
        let effective = self.effective_rtt_ms(client);
        if !self.throughput_limited {
            effective
        } else {
            client.rtt_ms.clamp(effective, effective * 2.0)
        }
    }

    fn pacing_fps(&self, client: &ClientState) -> f32 {
        let frame_bytes = client.avg_paced_frame_bytes.max(256.0);
        let sustainable = self.bandwidth_floor_bps / frame_bytes;
        sustainable.min(self.browser_pacing_fps)
    }

    fn target_frame_window(&self, client: &ClientState) -> usize {
        let window_fps = if self.throughput_limited {
            self.pacing_fps(client)
        } else {
            self.browser_pacing_fps
        };
        frame_window(self.window_rtt_ms(client), window_fps)
            .saturating_add(client.probe_frames.round().max(0.0) as usize)
    }

    fn base_queue_ms(&self) -> f32 {
        self.frame_ms * if self.throughput_limited { 2.0 } else { 8.0 }
    }

    fn target_queue_ms(&self, client: &ClientState) -> f32 {
        let probe_scale = if self.throughput_limited { 0.25 } else { 1.0 };
        self.base_queue_ms() + client.probe_frames.max(0.0) * self.frame_ms * probe_scale
    }

    fn byte_budget_for(&self, client: &ClientState, budget_ms: f32) -> usize {
        let budget_bps = if self.throughput_limited {
            self.bandwidth_floor_bps
        } else {
            client.goodput_bps.max(self.bandwidth_floor_bps)
        };
        let bytes = budget_bps * budget_ms.max(1.0) / 1_000.0;
        bytes.ceil().max(client.avg_frame_bytes.max(256.0)) as usize
    }

    fn target_byte_window(&self, client: &ClientState) -> usize {
        let budget = self.byte_budget_for(client, self.path_rtt_ms + self.target_queue_ms(client));
        let frame_bytes = client.avg_paced_frame_bytes.max(256.0).ceil() as usize;
        let target_frames = self.target_frame_window(client);
        let pipeline_bytes = frame_bytes.saturating_mul(target_frames);
        // For small pipelines (e.g. idle terminals with 1KB frames), allow the
        // full frame window worth of bytes so we pipeline across the RTT instead
        // of stop-and-wait.  For large pipelines (e.g. 50KB frames × 5 frames =
        // 250KB), the budget (BDP-based) is the binding constraint; fall back to
        // a one-frame floor so we don't pile up many RTTs worth of large frames.
        const PIPELINE_FLOOR_LIMIT: usize = 32_768; // 32 KB
        let floor = if pipeline_bytes <= PIPELINE_FLOOR_LIMIT {
            pipeline_bytes
        } else {
            frame_bytes // one-frame floor for large pipelines
        };
        budget.max(floor)
    }

    #[cfg(test)]
    fn preview_fps(&self, client: &ClientState) -> f32 {
        let mut fps = client.display_fps.clamp(1.0, TERMINAL_PREVIEW_MAX_FPS);
        if client.lead.is_some() && self.throughput_limited {
            // Only budget preview bandwidth when the link is actually saturated.
            // Without this, large preview frames (e.g. 12 KB) at 30 fps consume
            // 360 KB/s, starving the lead even when lead frames are tiny.
            // On fast links (localhost, LAN), previews run at their thumbnail
            // cap rather than consuming one full display-rate stream per PTY.
            let avail = self.bandwidth_floor_bps;
            let lead_bps = client.avg_paced_frame_bytes.max(256.0) * self.browser_pacing_fps;
            let preview_budget = (avail - lead_bps).max(avail * 0.25).max(0.0);
            let bw_cap = preview_budget / client.avg_preview_frame_bytes.max(256.0);
            fps = fps.min(bw_cap.max(1.0));
        }
        fps.max(1.0)
    }

    #[cfg(test)]
    fn send_interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.browser_pacing_fps.max(1.0) as f64)
    }

    /// The lead send gate, minus the deadline check.  Shares one snapshot
    /// across both limbs — they used to cost a full recomputation each.
    #[cfg(test)]
    fn window_open(&self, client: &ClientState) -> bool {
        !browser_backlog_blocked(client)
            && client.inflight_frames.len() < self.target_frame_window(client)
            && client.inflight_bytes < self.target_byte_window(client)
    }

    #[cfg(test)]
    fn lead_window_open(&self, client: &ClientState, reserve_preview_slot: bool) -> bool {
        if !reserve_preview_slot || client.lead.is_none() {
            return self.window_open(client);
        }
        if browser_backlog_blocked(client) {
            return false;
        }
        let target_frames = self.target_frame_window(client);
        let reserve_frames = PREVIEW_FRAME_RESERVE.min(target_frames.saturating_sub(1));
        let frame_limit = target_frames.saturating_sub(reserve_frames).max(1);
        let reserve_bytes = client.avg_preview_frame_bytes.max(256.0).ceil() as usize;
        let byte_limit = self
            .target_byte_window(client)
            .saturating_sub(reserve_bytes)
            .max(client.avg_paced_frame_bytes.max(256.0).ceil() as usize);
        client.inflight_frames.len() < frame_limit && client.inflight_bytes < byte_limit
    }
}

fn effective_rtt_ms(client: &ClientState) -> f32 {
    Pacing::new(client).effective_rtt_ms(client)
}

fn window_rtt_ms(client: &ClientState) -> f32 {
    Pacing::new(client).window_rtt_ms(client)
}

fn target_frame_window(client: &ClientState) -> usize {
    Pacing::new(client).target_frame_window(client)
}

fn browser_ready(client: &ClientState) -> bool {
    client.browser_ack_ahead_frames <= 1 && client.browser_apply_ms <= 1.0
}

fn bandwidth_floor_bps(client: &ClientState) -> f32 {
    let browser_ready = browser_ready(client);
    let backlog_scale = match client.browser_backlog_frames {
        0..=2 => 0.9,
        3..=8 => 0.8,
        _ => 0.65,
    };
    let penalty = client
        .goodput_jitter_bps
        .max(client.max_goodput_jitter_bps * 0.5)
        .min(client.goodput_bps * if browser_ready { 0.75 } else { 0.9 });
    let goodput_floor = (client.goodput_bps - penalty)
        .max(client.goodput_bps * if browser_ready { 0.35 } else { 0.2 });
    // On a browser-ready path, the per-frame delivery estimate is already
    // end-to-end and reacts much faster than ACK-window goodput. Halving it
    // leaves large-frame local links chronically underpaced.
    let delivery_floor = client.delivery_bps * if browser_ready { 1.0 } else { 0.5 };
    let recent_sample_floor = if browser_ready && client.last_goodput_sample_bps > 0.0 {
        client.last_goodput_sample_bps * backlog_scale
    } else {
        0.0
    };
    goodput_floor.max(recent_sample_floor).max(delivery_floor)
}

fn pacing_fps(client: &ClientState) -> f32 {
    Pacing::new(client).pacing_fps(client)
}

/// Whether total demand — lead at cadence rate plus previews at their cap —
/// exceeds what the link will carry.
///
/// The old check (`pacing_fps < cadence * 0.9`) only saw lead bandwidth,
/// which is often tiny, so previews could starve the lead undetected.
///
/// Kept as a free function for cold callers and tests; the hot path reads
/// `Pacing::throughput_limited`, which is where the real inlining happens.
#[cfg(test)]
fn throughput_limited(client: &ClientState) -> bool {
    Pacing::new(client).throughput_limited
}

fn browser_pacing_fps(client: &ClientState) -> f32 {
    let mut fps = client.display_fps.max(1.0);

    // Backlog and ack-ahead are direct signals from the browser about
    // whether it's keeping up.  No predictive apply-time bound — it
    // consistently underestimates capacity and causes 30fps death spirals.
    //
    // The backoff is steep: at the block threshold (backlog>8) we've
    // already dropped to display_fps/4.  A gentler schedule (4/backlog)
    // held 48fps at backlog=10 for software-encoded 1080p, which is
    // faster than the browser can decode → backlog never drains, the
    // hard block stays latched, and encoding stalls entirely.
    //
    // Trigger threshold (backlog > 4) gives a few frames of transient
    // headroom before backoff engages — at 120 Hz, a 30 fps source naturally
    // queues 1-2 frames during decoder hiccups, and triggering backoff there
    // chops the rate just to absorb normal jitter.
    let backlog = client.browser_backlog_frames as f32;
    if backlog > 4.0 {
        fps = fps.min(fps * (2.0 / backlog));
    }

    if client.browser_ack_ahead_frames > 4 {
        fps = fps.min(client.display_fps.max(1.0) * 0.5);
    }
    if client.browser_ack_ahead_frames > 8 {
        fps = fps.min(client.display_fps.max(1.0) * 0.25);
    }

    fps.max(1.0)
}

#[cfg(test)]
fn browser_backlog_blocked(client: &ClientState) -> bool {
    client.browser_backlog_frames > 8
}

#[cfg(test)]
fn byte_budget_for(client: &ClientState, budget_ms: f32) -> usize {
    Pacing::new(client).byte_budget_for(client, budget_ms)
}

fn target_byte_window(client: &ClientState) -> usize {
    Pacing::new(client).target_byte_window(client)
}

#[cfg(test)]
fn send_interval(client: &ClientState) -> Duration {
    Pacing::new(client).send_interval()
}

#[cfg(test)]
fn preview_fps(client: &ClientState) -> f32 {
    Pacing::new(client).preview_fps(client)
}

#[cfg(test)]
fn preview_send_interval(client: &ClientState) -> Duration {
    Duration::from_secs_f64(1.0 / preview_fps(client) as f64)
}

/// Frames one surface normally has awaiting ACK at the client's display rate
/// and RTT. This sizes ACK/goodput accounting; aggregate byte depth, rather
/// than this frame count, controls transport admission. At 100 ms RTT and
/// 60 Hz, six unacked frames are ordinary distance.
fn surface_frame_window(client: &ClientState) -> usize {
    frame_window(effective_rtt_ms(client), client.display_fps.max(1.0))
}

fn estimated_surface_frame_bytes(client: &ClientState, surface_id: u16, keyframe: bool) -> usize {
    let delta = client
        .surface_subs
        .get(&surface_id)
        .map(|sub| sub.frame_bytes)
        .filter(|bytes| *bytes > 0.0)
        .unwrap_or(client.avg_surface_frame_bytes)
        .max(1_024.0)
        .ceil() as usize;
    if keyframe {
        delta
            .saturating_mul(4)
            .max(SURFACE_KEYFRAME_ESTIMATE_MIN_BYTES)
    } else {
        delta
    }
}

fn surface_reserved_encode_bytes(client: &ClientState) -> usize {
    client
        .surface_subs
        .values()
        .map(|sub| sub.reserved_encode_bytes)
        .fold(0usize, usize::saturating_add)
}

fn surface_credit_used_bytes(client: &ClientState) -> usize {
    client
        .surface_inflight_bytes
        .saturating_add(surface_reserved_encode_bytes(client))
}

fn surface_credit_limit_bytes(client: &ClientState, next_frame_bytes: usize) -> usize {
    let window_secs = path_rtt_ms(client).max(0.0) / 1_000.0 + SURFACE_CREDIT_QUEUE_SECS;
    let measured = (client.surface_goodput_bps.max(1.0) * window_secs).ceil() as usize;
    let next = next_frame_bytes.max(1_024);
    measured.max(next.saturating_mul(SURFACE_CREDIT_BOOTSTRAP_FRAMES))
}

/// Admit one more surface frame against a client-wide byte window. An empty
/// window always admits one frame, even a keyframe larger than its estimate;
/// its bytes still count in full, so ordinary successors remain blocked until
/// enough of that oversized frame is ACKed.
fn surface_credit_open_for(client: &ClientState, next_frame_bytes: usize) -> bool {
    let used = surface_credit_used_bytes(client);
    used == 0
        || used.saturating_add(next_frame_bytes)
            <= surface_credit_limit_bytes(client, next_frame_bytes)
}

/// Admit a frame only when both the shared byte window and this surface's
/// negotiated decoder window have room. The latter must be enforced before
/// encoding: producing a delta which the protocol view then rejects advances
/// the encoder reference chain without advancing the decoder's, forcing the
/// next admitted frame to be another expensive keyframe.
fn surface_frame_credit_open_for(
    client: &ClientState,
    surface_id: u16,
    next_frame_bytes: usize,
) -> bool {
    let slot_open = client
        .surface_subs
        .get(&surface_id)
        .and_then(|sub| sub.max_inflight_frames)
        .is_none_or(|maximum| {
            client
                .surface_inflight_frames
                .iter()
                .filter(|frame| frame.surface_id == surface_id)
                .count()
                < maximum
        });
    slot_open && surface_credit_open_for(client, next_frame_bytes)
}

fn surface_work_order(client: &mut ClientState) -> SmallVec<[u16; 4]> {
    let mut surfaces: SmallVec<[u16; 4]> = client.surface_subscriptions.iter().copied().collect();
    surfaces.sort_unstable();
    if let Some(cursor) = client.surface_schedule_cursor {
        let start = surfaces.partition_point(|surface_id| *surface_id <= cursor);
        if start < surfaces.len() {
            surfaces.rotate_left(start);
        }
    }
    client.surface_schedule_cursor = surfaces.first().copied();
    surfaces
}

/// Browser decoder ACKs arrive on the JS event loop.  Keep enough history to
/// match a burst of delayed ACKs without evicting live records and attributing
/// their bytes/timestamps to newer frames. Individual ACK age is accounting
/// only; aggregate ACKed bytes over time control the shared surface credit.
const SURFACE_ACK_TRACKING_ALLOWANCE: Duration = Duration::from_millis(250);

fn surface_ack_tracking_frames(fps: f32) -> usize {
    (fps.max(1.0) * SURFACE_ACK_TRACKING_ALLOWANCE.as_secs_f32()).ceil() as usize
}

/// A few accepted chunks are normal WebCodecs pipeline depth.  Beyond this,
/// the decoder may be falling behind, but the report must persist long
/// enough to exclude one JavaScript callback batch before it affects pacing.
const SURFACE_DECODE_QUEUE_ALLOWANCE: u8 = 4;
const SURFACE_DECODE_PRESSURE_GRACE: Duration = Duration::from_millis(50);

fn update_surface_decoder_queue(sub: &mut SurfaceSubState, depth: u8, now: Instant) {
    sub.decoder_queue_depth = depth;
    if depth <= SURFACE_DECODE_QUEUE_ALLOWANCE {
        sub.decoder_queue_high_since = None;
        sub.decoder_pressure_depth = 0;
        return;
    }

    let high_since = sub.decoder_queue_high_since.get_or_insert(now);
    if now.duration_since(*high_since) >= SURFACE_DECODE_PRESSURE_GRACE {
        sub.decoder_pressure_depth = depth;
    }
}

/// Surface frame rate: always the client's display cadence.
///
/// Deliberately *not* `browser_pacing_fps`.  That function's inputs are
/// terminal metrics: `browser_backlog_frames` carries the client's
/// `pendingAppliedFrames`, which counts applied-but-unpainted *terminal*
/// frames and is cleared when a terminal paints
/// (`TerminalStore.noteFrameRendered`).  Pacing video off it meant a burst
/// of shell output throttled an unrelated video surface — steeply, since
/// the terminal schedule quarters the rate by a backlog of 8 — and because
/// the client only reports every 250 ms, the cut outlived the burst that
/// caused it.
///
/// WebCodecs `decodeQueueSize` is still useful as sustained pressure for the
/// adaptive quality controller, but not as a rate signal: a healthy hardware
/// decoder may keep 5–6 requests accepted while running at full throughput.
/// Cutting cadence from that standing pipeline depth creates the very misses
/// the controller is meant to prevent. Real transport overload is bounded by
/// the aggregate surface credit and socket outbox; decoder pressure buys
/// cheaper frames, never fewer frames.
fn surface_pacing_fps(client: &ClientState, surface_id: u16) -> f32 {
    client
        .surface_subs
        .get(&surface_id)
        .and_then(|s| s.max_fps)
        .map_or(client.display_fps, |limit| client.display_fps.min(limit))
        .max(1.0)
}

fn surface_send_interval(client: &ClientState, surface_id: u16) -> Duration {
    Duration::from_secs_f64(1.0 / surface_pacing_fps(client, surface_id).max(1.0) as f64)
}

/// Surface delivery never installs a second cadence limiter.  At full display
/// rate the compositor's fixed clock is already the metronome; another timer
/// in the busier delivery loop can only discard fresh generations when that
/// loop wakes a fraction late.
fn surface_delivery_is_throttled(client: &ClientState, surface_id: u16) -> bool {
    let desired = surface_send_interval(client, surface_id);
    client
        .surface_subs
        .get(&surface_id)
        .and_then(|sub| sub.source_interval)
        .is_some_and(|source| desired > source)
}

/// Wayland source cadence is a display property, not a transport property.
/// Encoding and delivery may pace down under congestion, but feeding that
/// decision back into `wl_surface.frame` slows the application itself and
/// makes its rAF clock depend on network RTT.
fn surface_source_interval(client: &ClientState, surface_id: u16) -> Duration {
    Duration::from_secs_f64(1.0 / surface_pacing_fps(client, surface_id) as f64)
}

/// Slowest surface pacing across this client, for the metrics line.
fn slowest_surface_pacing_fps(client: &ClientState) -> f32 {
    client
        .surface_subs
        .keys()
        .map(|&sid| surface_pacing_fps(client, sid))
        .fold(f32::INFINITY, f32::min)
        .min(client.display_fps.max(1.0))
}

/// Whether the next frame sent to `client` for `sid` must be a keyframe.
///
/// Per surface: a client watching several surfaces keeps an independent
/// decoder reference chain for each, so one surface's keyframe says nothing
/// about another's.  A surface with no sub state yet has been sent nothing
/// and cannot decode a delta, so it owes one.
fn owes_keyframe(client: &ClientState, sid: u16) -> bool {
    !client
        .surface_subs
        .get(&sid)
        .is_some_and(|s| s.has_keyframe)
}

/// What an encode result leaves in the sub's `last_encoded_gen`.
///
/// That field is the "already shown to this client" mark the encode loop's
/// `unchanged` gate reads, so only a generation that actually produced a
/// bitstream may advance it.  `encode_pixels` returns `None` as ordinary
/// control flow — rav1e asking for more data before it emits anything, a
/// DMA-BUF that could not be mapped, a zero-size x264 output — and marking
/// one of those as encoded makes the gate skip that generation forever.
/// While the surface keeps painting, the next generation covers for it.
/// When the surface goes still on exactly that frame — a video reaching its
/// last frame, an app settling after its final repaint — nothing covers for
/// it, and the client is left holding the frame before it.
fn encoded_generation(
    previous: Option<u64>,
    generation: u64,
    produced_output: bool,
) -> Option<u64> {
    if produced_output {
        Some(generation)
    } else {
        previous
    }
}

/// Apply an encode completion to its subscription.
///
/// A resubscribe can invalidate an encode while it is running.  Its output
/// belongs to the old codec or dimensions, so it must neither be delivered
/// nor advance the generation mark used by the delivery gate.  In
/// particular, an old-size keyframe must not satisfy the new encoder's
/// keyframe debt.
fn accept_completed_encode(
    state: &mut SurfaceSubState,
    generation: u64,
    produced_output: bool,
) -> bool {
    state.encode_in_flight = false;
    state.reserved_encode_bytes = 0;
    state.in_flight_generation = None;
    if std::mem::replace(&mut state.encoder_invalidated, false) {
        state.pending_encode = None;
        return false;
    }
    state.last_encoded_gen =
        encoded_generation(state.last_encoded_gen, generation, produced_output);
    true
}

/// Retire an encoder-creation task at the subscription boundary.
///
/// Creation allocates compositor targets as well as the encoder itself.  A
/// dock/undock handoff can change the requested target several times while
/// that blocking work runs, so a stale completion must be rejected before
/// it registers buffers or clears the current pixel cache.  Registering it
/// first can leave only thumbnail-sized pixels cached while the final native
/// subscription waits for native pixels before dispatching its own creation;
/// a later surface resize happens to break that deadlock by recompositing.
fn accept_completed_creation(state: &mut SurfaceSubState) -> bool {
    state.creation_in_flight = false;
    !std::mem::replace(&mut state.encoder_invalidated, false)
}

// ---------------------------------------------------------------------------
// Adaptive bandwidth
//
// The configured bandwidth is a CEILING, not an operating point: a surface
// never spends more than it was granted, but the server spends less when the
// link cannot carry it.  The controller compares what frames actually cost
// against what the measured goodput affords at the current pacing rate, and
// walks the AV1 quantizer between the ceiling and a floor.  Every input is
// already measured per client (goodput from surface ACKs, blocked writes from
// the writer task), so no new wire messages or client cooperation are needed.
// ---------------------------------------------------------------------------

/// Worst quantizer the controller will fall back to.  Past this the picture
/// is not worth sending; dropping frame rate is the better trade and pacing
/// already does that.
const ADAPTIVE_MAX_QUANTIZER: u8 = 200;
/// Fraction of measured goodput a surface may budget for.  The remainder is
/// headroom: aiming at 100% of an estimate that is itself derived from what
/// was sent guarantees a standing queue.
const ADAPTIVE_GOODPUT_SHARE: f32 = 0.8;
/// Minimum gap between steps, so the loop settles instead of oscillating.
const ADAPTIVE_STEP_INTERVAL: Duration = Duration::from_millis(250);
/// A socket becoming writable only proves that the queue drained, not that
/// the quality which filled it is sustainable.  Hold the cheaper operating
/// point before probing upward again.
const ADAPTIVE_CONGESTION_HOLD: Duration = Duration::from_secs(2);
/// Once the hold expires, recover quality conservatively.  Direct pressure
/// still backs off at `ADAPTIVE_STEP_INTERVAL`; this only slows probes toward
/// the configured ceiling after proven congestion.
const ADAPTIVE_RECOVERY_STEP_INTERVAL: Duration = Duration::from_secs(1);
/// Quantizer step when merely off-budget.
const ADAPTIVE_STEP: u8 = 6;
/// A backend that cannot retarget in place has to be rebuilt, which costs a
/// keyframe — only worth it past this much accumulated drift.
const ADAPTIVE_REBUILD_STEP: u8 = 24;
/// Maximum linear downscale is 8x (64x fewer pixels). A 3400x2424 remote
/// desktop can therefore prefer a roughly 424x302 stream at 120 Hz to a
/// full-size stream that reaches the viewer only three times per second.
const ADAPTIVE_MAX_SCALE_SHIFT: u8 = 3;
/// A failed resolution probe can return to its sustainable target quickly,
/// but not quickly enough to rebuild the encoder repeatedly on batched ACKs.
const ADAPTIVE_SCALE_BACKOFF_INTERVAL: Duration = Duration::from_millis(750);
/// Spare path capacity is unknowable from an app-limited stream. Probe one
/// resolution step upward occasionally; pressure returns it to the prior
/// extent if the link cannot carry it.
const ADAPTIVE_SCALE_RECOVERY_INTERVAL: Duration = Duration::from_secs(1);
/// A full delivery window is congestion evidence only when a frame is much
/// larger than its share at the requested cadence. Filling an ordinary BDP
/// with small frames is healthy pipelining, not a reason to degrade them.
const ADAPTIVE_DELIVERY_PRESSURE_RATIO: f32 = 1.5;
/// Blocked-write time within one step interval that counts as congestion.
/// A write that blocks for a tenth of the interval means the socket, not the
/// encoder, is setting the pace.
const WRITE_BLOCKED_CONGESTED_US: u64 = 25_000;
/// Gap between refinement steps on a surface that has stopped changing.
/// Longer than `ADAPTIVE_STEP_INTERVAL` because each step costs a keyframe
/// and there is no deadline to meet — nothing is moving.
const STILL_REFRESH_INTERVAL: Duration = Duration::from_millis(400);
/// Smallest quantizer improvement worth spending a keyframe on.
const STILL_REFINE_MIN_STEP: u8 = 16;

/// Next quantizer when refining a frozen picture back toward the ceiling.
///
/// Halves the remaining distance, with a floor on the step size so a wide
/// gap does not cost a long tail of barely-better keyframes.  The last step
/// always lands exactly on the ceiling.
fn refine_toward_ceiling(current: u8, ceiling: u8) -> u8 {
    let gap = current.saturating_sub(ceiling);
    if gap == 0 {
        return ceiling;
    }
    let step = gap.div_ceil(2).max(STILL_REFINE_MIN_STEP);
    current.saturating_sub(step).max(ceiling)
}

/// One surface's view of the link, as the controller sees it.
#[derive(Clone, Copy, Debug)]
struct RateSample {
    /// Best (lowest) quantizer allowed: the configured ceiling.
    ceiling: u8,
    /// Quantizer currently in effect.
    current: u8,
    /// Bytes per frame the link affords this surface.
    budget_bytes: f32,
    /// Bytes per frame this surface is actually producing.
    observed_bytes: f32,
    /// The transport told us it could not keep up (blocked write or a
    /// backed-up outbox) since the last step.
    congested: bool,
    /// Nothing on the path is straining: writes aren't blocking, the outbox
    /// is open, and the decoder queue is shallow.  Goodput measured in this
    /// state describes our own send rate, not link capacity, so a budget
    /// derived from it is not evidence of anything.
    app_limited: bool,
}

/// Next quantizer for a surface, clamped to `[ceiling, ADAPTIVE_MAX_QUANTIZER]`.
///
/// Multiplicative decrease on congestion, additive otherwise, and additive
/// increase back toward the ceiling only when frames are comfortably inside
/// budget — a surface that is exactly on budget is left alone.
fn next_quantizer(sample: RateSample) -> u8 {
    let ceiling = sample.ceiling.min(ADAPTIVE_MAX_QUANTIZER);
    let clamp = |q: i32| q.clamp(ceiling as i32, ADAPTIVE_MAX_QUANTIZER as i32) as u8;
    if sample.congested {
        // Back off hard: the queue is already forming, and the frames that
        // caused it are still in flight.
        return clamp(sample.current as i32 + (sample.current as i32 / 8).max(12));
    }
    // An unstrained link never justifies getting worse, whatever the budget
    // comparison below would say: on an app-limited link goodput converges
    // to whatever we are currently sending, so "over budget" is
    // self-fulfilling — smaller frames drag the measurement down, which
    // shrinks the budget, which asks for smaller frames again, all the way
    // to the floor (a lone spinner animation used to ride this spiral to
    // quantizer 200).  Spend the idle link on walking back to the
    // configured quality instead; if that turns out to be more than the
    // path can carry, the pressure signals return and the backoff above
    // answers them.
    if sample.app_limited {
        return clamp(sample.current as i32 - ADAPTIVE_STEP as i32);
    }
    // No usable budget yet (no goodput estimate, or no frame measured):
    // hold position rather than guess.
    if sample.budget_bytes <= 0.0 || sample.observed_bytes <= 0.0 {
        return clamp(sample.current as i32);
    }
    if sample.observed_bytes > sample.budget_bytes * 1.25 {
        clamp(sample.current as i32 + ADAPTIVE_STEP as i32)
    } else if sample.observed_bytes < sample.budget_bytes * 0.75 {
        clamp(sample.current as i32 - ADAPTIVE_STEP as i32)
    } else {
        clamp(sample.current as i32)
    }
}

/// Per-frame byte budget for one surface: its share of the client's measured
/// goodput at the current pacing rate.  A client watching two surfaces
/// splits by how many bytes each is actually producing, so a big active
/// window is not starved by a small idle one.
fn surface_budget_bytes(client: &ClientState, surface_id: u16) -> f32 {
    let fps = surface_pacing_fps(client, surface_id).max(1.0);
    let total: f32 = client.surface_subs.values().map(|s| s.frame_bytes).sum();
    let own = client
        .surface_subs
        .get(&surface_id)
        .map_or(0.0, |s| s.frame_bytes);
    let share = if total > 0.0 && own > 0.0 {
        own / total
    } else {
        let subs = client.surface_subs.len().max(1) as f32;
        1.0 / subs
    };
    client.surface_goodput_bps * ADAPTIVE_GOODPUT_SHARE * share / fps
}

/// Bandwidth a surface should encode at right now: the configured ceiling,
/// lowered by whatever the controller has decided the link can carry.
fn resolve_bandwidth(
    client: &ClientState,
    default: SurfaceBandwidth,
    surface_id: u16,
) -> SurfaceBandwidth {
    let sub = client.surface_subs.get(&surface_id);
    let ceiling = sub.and_then(|s| s.bandwidth_override).unwrap_or(default);
    match sub.and_then(|s| s.adaptive_quantizer) {
        Some(q) if q > ceiling.av1_quantizer() as u8 => SurfaceBandwidth::Custom { quantizer: q },
        _ => ceiling,
    }
}

/// Run one step of the controller for a surface and report whether the live
/// encoder now needs rebuilding (the backend could not retarget in place and
/// the drift is large enough to be worth a keyframe).
/// Outcome of one adaptive step for a surface.
struct AdaptiveStep {
    /// The quantizer moved, and the encoder in hand could not take the new
    /// rate in place, so it has to be rebuilt (paying a keyframe).
    rebuild: bool,
    /// The quantizer the surface should now encode at, when it moved.
    /// A compositor-resident encoder is retargeted with this; a local one
    /// has already been retargeted in place.
    quantizer: Option<u8>,
    /// Delivery pressure changed this client's encoded extent. The caller
    /// must retire the old encoder and wait for the next tick, which derives
    /// and registers the new target.
    target_changed: bool,
}

/// Number of additional 2x linear downscale steps needed for the observed
/// frame to fit its per-frame path budget. Each step quarters pixel count and
/// therefore approximates a 4x byte reduction at a fixed quantizer.
fn adaptive_scale_steps(observed_bytes: f32, budget_bytes: f32) -> u8 {
    if observed_bytes <= 0.0 || budget_bytes <= 0.0 {
        return 1;
    }
    let mut ratio = observed_bytes / budget_bytes;
    let mut steps = 0;
    while ratio > ADAPTIVE_DELIVERY_PRESSURE_RATIO && steps < ADAPTIVE_MAX_SCALE_SHIFT {
        ratio /= 4.0;
        steps += 1;
    }
    steps.max(1)
}

/// Apply transport adaptation to an already aspect-preserving encode target.
/// Both axes use the same divisor; even rounding is required by every video
/// backend and differs from the exact aspect by at most one source pixel.
fn adaptive_surface_target(width: u32, height: u32, shift: u8) -> (u32, u32) {
    let divisor = 1u32 << shift.min(ADAPTIVE_MAX_SCALE_SHIFT);
    (
        ((width / divisor) & !1).max(2),
        ((height / divisor) & !1).max(2),
    )
}

///
/// `unchanged` says the surface is showing a frame the client already has.
/// In that mode the controller stops rate-controlling — the budget it would
/// judge against describes motion that has stopped — and instead walks the
/// quantizer back toward the ceiling so a picture that is going to sit on
/// screen ends up as good as the configuration allows.
fn step_adaptive_bandwidth(
    client: &mut ClientState,
    default: SurfaceBandwidth,
    surface_id: u16,
    now: Instant,
    unchanged: bool,
) -> AdaptiveStep {
    let blocked_us = client.write_blocked_us.load(Ordering::Relaxed);
    let decoder_backlogged = client
        .surface_subs
        .get(&surface_id)
        .is_some_and(|sub| sub.decoder_pressure_depth > SURFACE_DECODE_QUEUE_ALLOWANCE);
    let budget_bytes = surface_budget_bytes(client, surface_id);
    let congested = blocked_us.saturating_sub(client.write_blocked_us_seen)
        > WRITE_BLOCKED_CONGESTED_US
        || decoder_backlogged;
    let previous_congestion = client
        .surface_subs
        .get(&surface_id)
        .and_then(|sub| sub.congested_at);
    let recovering = previous_congestion.is_some();
    let recovery_hold = !congested
        && previous_congestion.is_some_and(|at| now.duration_since(at) < ADAPTIVE_CONGESTION_HOLD);
    // Pressure evidence: the writer blocking on the socket, or this surface's
    // explicit WebCodecs queue growing. Raw ACK age, callback timing, and a
    // full delivery window are deliberately absent: all include ordinary
    // scheduling/flow-control delay and produced false quality backoff on
    // otherwise healthy links. In particular, adapting to a window derived
    // from measured goodput was circular: stop-and-wait lowered measured
    // throughput, which shrank the window and then degraded the stream again.
    //
    // Deliberately *not* `browser_backlog_frames`, for the reason
    // `surface_pacing_fps` spells out: that counter is `pendingAppliedFrames`,
    // applied-but-unpainted *terminal* frames, cleared only when a terminal
    // paints.  Reading it here let a burst of shell output — or a terminal
    // that simply never repaints — say "the path is strained" about an
    // unrelated video surface.  Worse than it was for pacing: pacing is a
    // pure function of live state and recovers when the burst ends, while
    // `adaptive_quantizer` is latched, so the backoff outlived its cause
    // forever.  The budget arm cannot undo it (see
    // `a_self_measured_budget_can_never_ask_for_better`), which made this the
    // only way back up.
    // With no direct pressure, goodput describes our own send rate rather
    // than capacity, regardless of path RTT or how many ACK callbacks the
    // browser has temporarily batched.  Treat that link as app-limited so
    // the self-measured budget cannot walk quality down by itself.
    let app_limited = !congested && !recovery_hold;
    let ceiling = client
        .surface_subs
        .get(&surface_id)
        .and_then(|s| s.bandwidth_override)
        .unwrap_or(default);
    let ceiling_q = ceiling.av1_quantizer().min(255) as u8;

    let held = AdaptiveStep {
        rebuild: false,
        quantizer: None,
        target_changed: false,
    };
    let Some(sub) = client.surface_subs.get_mut(&surface_id) else {
        return held;
    };
    if congested {
        sub.adaptive_pressure_at = Some(now);
    }
    let interval = if unchanged {
        STILL_REFRESH_INTERVAL
    } else if recovering && !congested {
        ADAPTIVE_RECOVERY_STEP_INTERVAL
    } else {
        ADAPTIVE_STEP_INTERVAL
    };
    if sub
        .rate_stepped_at
        .is_some_and(|at| now.duration_since(at) < interval)
    {
        return held;
    }
    let current = sub.adaptive_quantizer.unwrap_or(ceiling_q).max(ceiling_q);
    if congested {
        sub.congested_at = Some(now);
    }
    let next = if unchanged {
        // A frozen picture is exactly when the link is idle and the bits
        // are affordable — unless the backlog says otherwise, in which
        // case leave it alone rather than pile a keyframe onto a queue.
        if congested {
            current
        } else {
            refine_toward_ceiling(current, ceiling_q)
        }
    } else if recovery_hold {
        current
    } else if sub.adaptive_scale_shift > 0 && current == ADAPTIVE_MAX_QUANTIZER {
        // While resolution is adapted, spend spare bytes on probing a larger
        // picture, not on making the small picture more expensive. If the
        // source stops, the `unchanged` arm may still refine the image that
        // will remain on screen.
        current
    } else {
        next_quantizer(RateSample {
            ceiling: ceiling_q,
            current,
            budget_bytes,
            observed_bytes: sub.frame_bytes,
            congested,
            app_limited,
        })
    };
    sub.rate_stepped_at = Some(now);
    client.write_blocked_us_seen = blocked_us;
    if !congested && !recovery_hold && next <= ceiling_q {
        sub.congested_at = None;
    }

    let can_scale = sub.scaled_target.is_none() || sub.allow_adaptive_scale;
    let pressure_scale_ready = sub
        .scale_stepped_at
        .is_none_or(|at| now.duration_since(at) >= ADAPTIVE_SCALE_BACKOFF_INTERVAL);
    let recovery_scale_ready = sub
        .scale_stepped_at
        .is_none_or(|at| now.duration_since(at) >= ADAPTIVE_SCALE_RECOVERY_INTERVAL);
    let mut target_changed = false;
    let previous_shift = sub.adaptive_scale_shift;
    if can_scale
        && next == ADAPTIVE_MAX_QUANTIZER
        && congested
        && pressure_scale_ready
        && previous_shift < ADAPTIVE_MAX_SCALE_SHIFT
    {
        let additional = adaptive_scale_steps(sub.frame_bytes, budget_bytes);
        sub.adaptive_scale_shift = previous_shift
            .saturating_add(additional)
            .min(ADAPTIVE_MAX_SCALE_SHIFT);
        target_changed = sub.adaptive_scale_shift != previous_shift;
    } else if can_scale
        && previous_shift > 0
        && !congested
        && !recovery_hold
        && recovery_scale_ready
        && sub
            .adaptive_pressure_at
            .is_some_and(|at| now.duration_since(at) >= ADAPTIVE_SCALE_RECOVERY_INTERVAL)
    {
        sub.adaptive_scale_shift -= 1;
        target_changed = true;
    }
    if target_changed {
        sub.scale_stepped_at = Some(now);
        let shift_delta = sub.adaptive_scale_shift.abs_diff(previous_shift);
        let area_factor = 4f32.powi(i32::from(shift_delta));
        if sub.adaptive_scale_shift > previous_shift {
            sub.frame_bytes = (sub.frame_bytes / area_factor).max(1.0);
        } else {
            sub.frame_bytes *= area_factor;
        }
        sub.has_keyframe = false;
        sub.last_encoded_gen = None;
        sub.pending_encode = None;
    }

    if next == current && !target_changed {
        // Nothing moved.  Reporting a step anyway would be harmless for a
        // live surface (a redundant set to the rate already in effect) but
        // a still one reads it as "the picture improved" and spends a
        // keyframe on it, every interval, forever.
        return held;
    }
    sub.adaptive_quantizer = if next > ceiling_q { Some(next) } else { None };

    // Retarget the live encoder in place if it can be; otherwise ask for a
    // rebuild, but only once the drift is big enough to pay for a keyframe.
    let target = SurfaceBandwidth::Custom { quantizer: next };
    let rebuild = match sub.encoder.as_mut() {
        Some(enc) => {
            if enc.set_bandwidth(target) {
                false
            } else {
                let running = enc.encoding().bandwidth.av1_quantizer() as i32;
                (next as i32 - running).abs() >= ADAPTIVE_REBUILD_STEP as i32
            }
        }
        // No encoder in hand (between jobs, in flight, or owned by the
        // compositor): the next creation picks the new bandwidth up from
        // `resolve_bandwidth`, and the caller retargets a Vulkan session.
        None => false,
    };
    AdaptiveStep {
        rebuild,
        quantizer: Some(next),
        target_changed,
    }
}

/// Emit a pacing-metrics line for this client if 10s have elapsed since
/// the last one.  Called both from the ACK handler and from `tick()` so
/// an idle client (no ACK traffic) still gets periodic metrics.
fn maybe_log_pacing_metrics(sess: &mut Session, client_id: u64, verbose: bool) {
    let Some(c) = sess.clients.get_mut(&client_id) else {
        return;
    };
    if c.last_log.elapsed().as_secs_f32() < 10.0 {
        return;
    }
    let log_elapsed = c.last_log.elapsed().as_secs_f32().max(1.0e-3);
    let paced_fps = pacing_fps(c);
    let display_need_bps_v = display_need_bps(c);
    let surface_fps = slowest_surface_pacing_fps(c);
    let frames_sent = c.frames_sent;
    let acks_recv = c.acks_recv;
    let rtt_ms = c.rtt_ms;
    let min_rtt_ms = path_rtt_ms(c);
    let eff_rtt_ms = window_rtt_ms(c);
    let delivery_bps = c.delivery_bps;
    let goodput_ewma_bps = c.goodput_bps;
    let goodput_jitter_bps = c.goodput_jitter_bps;
    let max_goodput_jitter_bps = c.max_goodput_jitter_bps;
    let avg_frame_bytes = c.avg_frame_bytes;
    let avg_paced_frame_bytes = c.avg_paced_frame_bytes;
    let avg_preview_frame_bytes = c.avg_preview_frame_bytes;
    let display_fps = c.display_fps;
    let probe_frames = c.probe_frames;
    let goodput_bps = c.acked_bytes_since_log as f32 / log_elapsed;
    let window_frames = target_frame_window(c);
    let window_bytes = target_byte_window(c);
    let browser_backlog_frames = c.browser_backlog_frames;
    let browser_ack_ahead_frames = c.browser_ack_ahead_frames;
    let browser_apply_ms = c.browser_apply_ms;
    let avg_surface_frame_bytes = c.avg_surface_frame_bytes;
    let skip_same_gen = c.skip_same_gen_count;
    let skip_in_flight = c.skip_in_flight_count;
    let skip_pacing = c.skip_pacing_count;
    let skip_vk_await = c.skip_vulkan_await_count;
    let skip_no_subs = c.skip_no_subs_count;
    let skip_not_subbed = c.skip_not_subbed_count;
    let skip_mismatch = c.skip_last_pixels_mismatch_count;
    let loop_iters = c.encode_loop_iters;
    let own_subs: usize = c.surface_subscriptions.len();
    let vk_surfs = c.vulkan_video_surfaces.len();
    let in_flight_set_len = c
        .surface_subs
        .values()
        .filter(|s| s.encode_in_flight)
        .count();
    let surface_burst: u8 = c
        .surface_subs
        .values()
        .map(|s| s.burst_remaining)
        .max()
        .unwrap_or(0);
    let surface_decode_q: u8 = c
        .surface_subs
        .values()
        .map(|s| s.decoder_queue_depth)
        .max()
        .unwrap_or(0);
    let surface_decode_pressure: u8 = c
        .surface_subs
        .values()
        .map(|s| s.decoder_pressure_depth)
        .max()
        .unwrap_or(0);
    // Worst (highest) quantizer the adaptive controller has fallen back to
    // across this client's surfaces; absent = every surface is at its
    // configured ceiling.
    let adaptive_q = c
        .surface_subs
        .values()
        .filter_map(|s| s.adaptive_quantizer)
        .max();
    let adaptive_q_log = adaptive_q.map_or(-1i32, |q| q as i32);
    let adaptive_scale_divisor = 1u16
        << c.surface_subs
            .values()
            .map(|s| s.adaptive_scale_shift)
            .max()
            .unwrap_or(0);
    let encode_jobs = sess.surface_encode_jobs.max(1) as u64;
    let encode_queue_avg_us = sess.surface_encode_queue_us / encode_jobs;
    let encode_work_avg_us = sess.surface_encode_work_us / encode_jobs;
    let encode_handoff_avg_us = sess.surface_encode_handoff_us / encode_jobs;

    c.frames_sent = 0;
    c.acks_recv = 0;
    c.acked_bytes_since_log = 0;
    c.skip_same_gen_count = 0;
    c.skip_in_flight_count = 0;
    c.skip_pacing_count = 0;
    c.skip_vulkan_await_count = 0;
    c.skip_no_subs_count = 0;
    c.skip_not_subbed_count = 0;
    c.skip_last_pixels_mismatch_count = 0;
    c.encode_loop_iters = 0;
    c.last_log = Instant::now();

    if verbose {
        let surf_info = sess.compositor.as_ref().map(|cs| {
            let surfaces = cs.surfaces.len();
            let pending = 0usize;
            let subs: usize = sess
                .clients
                .values()
                .map(|c| c.surface_subscriptions.len())
                .sum();
            (surfaces, pending, subs)
        });
        let (surf_count, surf_pending, surf_subs) = surf_info.unwrap_or((0, 0, 0));
        eprintln!(
            "client {client_id}: sent={frames_sent} acks={acks_recv} rtt={rtt_ms:.0}ms min_rtt={min_rtt_ms:.0}ms eff_rtt={eff_rtt_ms:.0}ms window={window_frames}f/{window_bytes}B probe={probe_frames:.0}f goodput={goodput_bps:.0}B/s goodput_ewma={goodput_ewma_bps:.0}B/s jitter={goodput_jitter_bps:.0}/{max_goodput_jitter_bps:.0}B/s rate={delivery_bps:.0}B/s avg_frame={avg_frame_bytes:.0}B lead_frame={avg_paced_frame_bytes:.0}B preview_frame={avg_preview_frame_bytes:.0}B need={display_need_bps_v:.0}B/s display_fps={display_fps:.0} paced_fps={paced_fps:.0} surface_fps={surface_fps:.0} surface_frame={avg_surface_frame_bytes:.0}B backlog={browser_backlog_frames} ack_ahead={browser_ack_ahead_frames} apply={browser_apply_ms:.1}ms surface_decode_q={surface_decode_q} surface_decode_pressure={surface_decode_pressure} | tick_fires={} tick_snaps={} frame_req={} | surfaces={surf_count} subs={surf_subs} own_subs={own_subs} pending_req={surf_pending} commits={} encodes={} enc_bytes={} surf_sent={} enc_queue={encode_queue_avg_us}/{}us enc_work={encode_work_avg_us}/{}us enc_handoff={encode_handoff_avg_us}/{}us px_empty_ticks={} px_snap_len={} loop_iters={loop_iters} skip_same_gen={skip_same_gen} skip_in_flight={skip_in_flight} skip_pacing={skip_pacing} skip_vk_await={skip_vk_await} skip_no_subs={skip_no_subs} skip_not_subbed={skip_not_subbed} skip_mismatch={skip_mismatch} vk_surfs={vk_surfs} enc_in_flight_set={in_flight_set_len} burst={surface_burst} adaptive_q={adaptive_q_log} adaptive_scale=1/{adaptive_scale_divisor}",
            sess.tick_fires,
            sess.tick_snaps,
            sess.frame_requests,
            sess.surface_commits,
            sess.surface_encodes,
            sess.surface_encode_bytes,
            sess.surface_frames_sent,
            sess.surface_encode_queue_max_us,
            sess.surface_encode_work_max_us,
            sess.surface_encode_handoff_max_us,
            sess.ticks_pixel_snapshot_empty,
            sess.pixel_snapshot_len,
        );
    }
    sess.tick_fires = 0;
    sess.tick_snaps = 0;
    sess.frame_requests = 0;
    sess.surface_commits = 0;
    sess.surface_encodes = 0;
    sess.surface_encode_bytes = 0;
    sess.surface_frames_sent = 0;
    sess.surface_encode_jobs = 0;
    sess.surface_encode_queue_us = 0;
    sess.surface_encode_queue_max_us = 0;
    sess.surface_encode_work_us = 0;
    sess.surface_encode_work_max_us = 0;
    sess.surface_encode_handoff_us = 0;
    sess.surface_encode_handoff_max_us = 0;
    sess.ticks_pixel_snapshot_empty = 0;
}

fn advance_deadline(deadline: &mut Instant, now: Instant, interval: Duration) {
    let _ = consume_deadline(deadline, now, interval);
}

/// Consume the most recent due point on a fixed-rate timeline and leave the
/// deadline at the first point strictly after `now`.
///
/// A late wake must skip missed refreshes instead of issuing several catch-up
/// callbacks back-to-back. Current Chromium deliberately coalesces callbacks
/// that arrive in one vsync, so catch-up bursts spend deadlines without
/// producing frames and systematically lower the effective rate.
fn consume_deadline(deadline: &mut Instant, now: Instant, interval: Duration) -> Instant {
    if interval.is_zero() {
        *deadline = now;
        return now;
    }
    if *deadline > now {
        let consumed = *deadline;
        *deadline = deadline.checked_add(interval).unwrap_or(now + interval);
        return consumed;
    }
    let intervals_elapsed = now.duration_since(*deadline).as_nanos() / interval.as_nanos();
    let intervals_elapsed = u32::try_from(intervals_elapsed).unwrap_or(u32::MAX);
    let consumed = deadline
        .checked_add(interval.saturating_mul(intervals_elapsed))
        .unwrap_or(now);
    *deadline = consumed.checked_add(interval).unwrap_or(now + interval);
    consumed
}

fn should_snapshot_pty(
    dirty: bool,
    needful: bool,
    synced_output: bool,
    snapshot_not_before: Option<Instant>,
    now: Instant,
) -> bool {
    dirty && needful && !synced_output && snapshot_not_before.is_none_or(|deadline| deadline <= now)
}

fn enqueue_ready_frame(queue: &mut VecDeque<FrameState>, frame: FrameState) -> bool {
    if queue.len() >= READY_FRAME_QUEUE_CAP {
        return false;
    }
    queue.push_back(frame);
    true
}

fn charge_pty_parse_budgets(per_pty: &mut usize, per_session: &mut usize, bytes: usize) {
    *per_pty = per_pty.saturating_sub(bytes);
    *per_session = per_session.saturating_sub(bytes);
}

fn advance_pty_parse_cursor(start: usize, visited: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    let advance = if visited == total { 1 } else { visited };
    (start + advance) % total
}

/// Find the first `\x1b[?2026l` in `bytes`, handling sequences that span
/// the `prefix`/`bytes` boundary. Uses SIMD-accelerated memchr for the
/// initial ESC scan.
fn find_sync_output_end(prefix: &[u8], bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }
    let needle = SYNC_OUTPUT_END;
    let nlen = needle.len();

    // Check for a match straddling the prefix/bytes boundary.
    if !prefix.is_empty() {
        let tail = if prefix.len() >= nlen - 1 {
            &prefix[prefix.len() - (nlen - 1)..]
        } else {
            prefix
        };
        let combined_len = tail.len() + bytes.len().min(nlen);
        if combined_len >= nlen {
            // Small stack buffer to check the boundary region.
            let mut buf = [0u8; 32]; // SYNC_OUTPUT_END is 8 bytes, so 32 is plenty
            let blen = combined_len.min(buf.len());
            let tlen = tail.len().min(blen);
            buf[..tlen].copy_from_slice(&tail[..tlen]);
            let rest = (blen - tlen).min(bytes.len());
            buf[tlen..tlen + rest].copy_from_slice(&bytes[..rest]);
            for i in 0..=(blen.saturating_sub(nlen)) {
                if &buf[i..i + nlen] == needle {
                    let end_in_bytes = (i + nlen).saturating_sub(tail.len());
                    if end_in_bytes > 0 && end_in_bytes <= bytes.len() {
                        return Some(end_in_bytes);
                    }
                }
            }
        }
    }

    // SIMD-scan for ESC (0x1b) then verify the full sequence.
    let mut offset = 0;
    while let Some(pos) = memchr::memchr(0x1b, &bytes[offset..]) {
        let abs = offset + pos;
        if abs + nlen <= bytes.len() && &bytes[abs..abs + nlen] == needle {
            return Some(abs + nlen);
        }
        offset = abs + 1;
    }
    None
}

fn update_sync_scan_tail(tail: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    tail.extend_from_slice(bytes);
    let keep = SYNC_OUTPUT_END.len().saturating_sub(1);
    if tail.len() > keep {
        let drop = tail.len() - keep;
        tail.drain(..drop);
    }
}

#[cfg(test)]
fn preview_deadline(client: &ClientState, pid: u16, now: Instant) -> Instant {
    client
        .preview_next_send_at
        .get(&pid)
        .copied()
        .unwrap_or(now)
}

/// A composited-frame position as the unsigned wire fields carry it.
///
/// Touch contacts arrive as signed sub-pixel floats — the transport encodes them
/// ×100 as `i32` — but the shared-input mirror is `u16`, so a contact in the
/// letterbox margin has to be clamped rather than wrapped.
fn frame_point(x: f64, y: f64) -> (u16, u16) {
    let clamp = |v: f64| v.round().clamp(0.0, f64::from(u16::MAX)) as u16;
    (clamp(x), clamp(y))
}

#[allow(clippy::too_many_arguments)]
fn enqueue_surface_frame(
    client: &ClientState,
    surface_id: u16,
    timestamp_ms: u32,
    timestamp_sub_us: u16,
    flags: u8,
    keyframe: bool,
    data: Vec<u8>,
) -> Result<usize, ()> {
    let codec = match flags & SURFACE_FRAME_CODEC_MASK {
        SURFACE_FRAME_CODEC_H264 => yas_surface_backend::Codec::H264,
        SURFACE_FRAME_CODEC_AV1 => yas_surface_backend::Codec::Av1,
        _ => return Err(()),
    };
    yas_surface_backend::enqueue_frame(
        client,
        yas_surface_backend::EncodedFrame {
            surface_id,
            timestamp_ms,
            timestamp_sub_us,
            codec,
            keyframe,
            data,
        },
    )
}

fn enqueue_surface_remote_input(
    client: &ClientState,
    surface_id: u16,
    seat_handle: u64,
    kind: u8,
    points: &[(u16, u16)],
) -> Result<(), ()> {
    // Native pointer events always carry their one live contact. The client
    // retires a stale overlay through its short expiry.
    if kind == REMOTE_INPUT_POINTER && points.is_empty() {
        return Ok(());
    }
    let kind = match kind {
        REMOTE_INPUT_POINTER => yas_surface_backend::RemoteInputKind::Pointer,
        REMOTE_INPUT_TOUCH => yas_surface_backend::RemoteInputKind::Touch,
        _ => return Err(()),
    };
    yas_surface_backend::enqueue_remote_input(client, surface_id, seat_handle, kind, points)
}

#[cfg(test)]
fn can_send_preview(client: &ClientState, pid: u16, now: Instant) -> bool {
    window_open(client) && now >= preview_deadline(client, pid, now)
}

#[cfg(test)]
fn record_preview_send(client: &mut ClientState, pid: u16, now: Instant) {
    let mut deadline = client
        .preview_next_send_at
        .get(&pid)
        .copied()
        .unwrap_or(now);
    advance_deadline(&mut deadline, now, preview_send_interval(client));
    client.preview_next_send_at.insert(pid, deadline);
}

#[cfg(test)]
fn window_open(client: &ClientState) -> bool {
    Pacing::new(client).window_open(client)
}

/// Client-wide surface gate. Cadence remains per surface, while all surface
/// bytes share one ACK-derived transport window so aggregate demand cannot
/// grow a reliable ordered queue in front of latency-sensitive traffic.
fn surface_window_open(client: &ClientState) -> bool {
    let estimate = client.avg_surface_frame_bytes.max(1_024.0).ceil() as usize;
    surface_credit_open_for(client, estimate)
}

#[cfg(test)]
fn lead_window_open(client: &ClientState, reserve_preview_slot: bool) -> bool {
    Pacing::new(client).lead_window_open(client, reserve_preview_slot)
}

#[cfg(test)]
fn can_send_frame(client: &ClientState, now: Instant, reserve_preview_slot: bool) -> bool {
    lead_window_open(client, reserve_preview_slot) && now >= client.next_send_at
}

#[cfg(test)]
fn record_send(client: &mut ClientState, bytes: usize, now: Instant, paced: bool) {
    client.inflight_bytes += bytes;
    client.inflight_frames.push_back(InFlightFrame {
        sent_at: now,
        bytes,
        paced,
    });
    if paced {
        let interval = send_interval(client);
        advance_deadline(&mut client.next_send_at, now, interval);
    }
}

fn ewma_with_direction(old: f32, sample: f32, rise_alpha: f32, fall_alpha: f32) -> f32 {
    let alpha = if sample > old { rise_alpha } else { fall_alpha };
    old * (1.0 - alpha) + sample * alpha
}

fn surface_goodput_ewma(old: f32, sample: f32) -> f32 {
    ewma_with_direction(old, sample, 0.125, 0.5)
}

#[cfg(test)]
fn window_saturated(client: &ClientState, inflight_frames: usize, inflight_bytes: usize) -> bool {
    let target_frames = target_frame_window(client);
    let target_bytes = target_byte_window(client);
    inflight_frames.saturating_mul(10) >= target_frames.saturating_mul(9)
        || inflight_bytes.saturating_mul(10) >= target_bytes.saturating_mul(9)
}

#[cfg(test)]
fn record_ack(client: &mut ClientState) {
    if let Some(frame) = client.inflight_frames.pop_front() {
        let prev_inflight_frames = client.inflight_frames.len() + 1;
        let prev_inflight_bytes = client.inflight_bytes;
        client.inflight_bytes = client.inflight_bytes.saturating_sub(frame.bytes);
        client.acked_bytes_since_log = client.acked_bytes_since_log.saturating_add(frame.bytes);
        let sample_ms = frame.sent_at.elapsed().as_secs_f32() * 1_000.0;
        client.rtt_ms = ewma_with_direction(client.rtt_ms, sample_ms, 0.125, 0.25);
        if client.min_rtt_ms > 0.0 {
            // Only update downward: min_rtt tracks the unloaded path RTT and
            // must not drift upward during congestion (queued RTT ≠ path RTT).
            client.min_rtt_ms = client.min_rtt_ms.min(sample_ms);
        } else {
            client.min_rtt_ms = sample_ms;
        }
        client.min_rtt_ms = client.min_rtt_ms.max(0.5);
        let sample_bps = frame.bytes as f32 / sample_ms.max(1.0e-3) * 1_000.0;
        client.delivery_bps = ewma_with_direction(client.delivery_bps, sample_bps, 0.5, 0.125);
        client.avg_frame_bytes =
            ewma_with_direction(client.avg_frame_bytes, frame.bytes as f32, 0.5, 0.125);
        if frame.paced {
            client.avg_paced_frame_bytes =
                ewma_with_direction(client.avg_paced_frame_bytes, frame.bytes as f32, 0.5, 0.125);
        } else {
            client.avg_preview_frame_bytes = ewma_with_direction(
                client.avg_preview_frame_bytes,
                frame.bytes as f32,
                0.5,
                0.125,
            );
        }
        let frame_ms = 1_000.0 / browser_pacing_fps(client).max(1.0);
        let path_rtt = path_rtt_ms(client);
        let likely_window_limited =
            window_saturated(client, prev_inflight_frames, prev_inflight_bytes);
        client.goodput_window_bytes = client.goodput_window_bytes.saturating_add(frame.bytes);
        let now = Instant::now();
        let goodput_elapsed = now
            .duration_since(client.goodput_window_start)
            .as_secs_f32();
        if goodput_elapsed >= 0.02 {
            let sample_goodput = client.goodput_window_bytes as f32 / goodput_elapsed.max(1.0e-3);
            if likely_window_limited || client.browser_backlog_frames > 0 {
                let prev_goodput_sample = if client.last_goodput_sample_bps > 0.0 {
                    client.last_goodput_sample_bps
                } else {
                    sample_goodput
                };
                let jitter_sample = (sample_goodput - prev_goodput_sample).abs();
                client.goodput_bps =
                    ewma_with_direction(client.goodput_bps, sample_goodput, 0.5, 0.125);
                // Only update jitter from windows with at least 2 frames.
                // Single-frame windows are pure measurement noise (0 or 1
                // frame per 25 ms is a Bernoulli trial, not a congestion
                // signal) and inflate jitter_bps, which in turn depresses
                // bandwidth_floor_bps and causes pacing to stall.
                let min_reliable = (client.avg_paced_frame_bytes.max(256.0) * 2.0) as usize;
                if client.goodput_window_bytes >= min_reliable {
                    client.goodput_jitter_bps =
                        ewma_with_direction(client.goodput_jitter_bps, jitter_sample, 0.5, 0.125);
                    let jitter_decay = if browser_ready(client) && sample_ms < path_rtt * 3.0 {
                        0.90
                    } else {
                        0.98
                    };
                    client.max_goodput_jitter_bps =
                        (client.max_goodput_jitter_bps * jitter_decay).max(jitter_sample);
                    // Cap jitter at 45% of goodput so jitter_ratio can never
                    // exceed 0.45 from measurement noise alone.  Real congestion
                    // will still drive goodput_bps down and widen the window.
                    client.max_goodput_jitter_bps =
                        client.max_goodput_jitter_bps.min(client.goodput_bps * 0.45);
                } else {
                    // Thin sample: gently decay jitter rather than updating it.
                    client.goodput_jitter_bps *= 0.9;
                    client.max_goodput_jitter_bps *= 0.95;
                }
                // Sticky-high: never let last_goodput_sample_bps drop abruptly.
                // A sudden drop (e.g. 1-frame window following a 2-frame window)
                // inflates jitter_sample on the next cycle, collapsing probe_frames.
                client.last_goodput_sample_bps =
                    (client.last_goodput_sample_bps * 0.99).max(sample_goodput);
            } else {
                // When the path is underfilled, ACK cadence mostly measures our
                // own pacing rather than network capacity.  Use a fall alpha
                // proportional to estimation error: when the estimate is 10x+
                // the sample, converge aggressively; when close, stay gentle.
                let ratio = client.goodput_bps / sample_goodput.max(1.0);
                let fall_alpha = if ratio > 10.0 {
                    0.5
                } else if ratio > 3.0 {
                    0.25
                } else {
                    0.03
                };
                client.goodput_bps =
                    ewma_with_direction(client.goodput_bps, sample_goodput, 0.5, fall_alpha);
                client.goodput_jitter_bps *= 0.5;
                client.max_goodput_jitter_bps *= 0.9;
                client.last_goodput_sample_bps =
                    (client.last_goodput_sample_bps * 0.99).max(sample_goodput);
            }
            client.goodput_window_bytes = 0;
            client.goodput_window_start = now;
        }
        let queue_baseline_ms = if throughput_limited(client) {
            window_rtt_ms(client)
        } else {
            path_rtt
        };
        let queue_delay_ms = (sample_ms - queue_baseline_ms).max(0.0);
        let max_probe_frames = (browser_pacing_fps(client) * 0.125).max(4.0);
        let jitter_ratio = client.max_goodput_jitter_bps / client.goodput_bps.max(1.0);
        let low_delay_frames = if throughput_limited(client) { 2.0 } else { 8.0 };
        let high_delay_frames = if throughput_limited(client) {
            4.0
        } else {
            12.0
        };
        if likely_window_limited
            && queue_delay_ms <= frame_ms * low_delay_frames
            && jitter_ratio < 0.25
        {
            client.probe_frames = (client.probe_frames + 1.0).min(max_probe_frames);
        } else if !likely_window_limited
            && browser_ready(client)
            && queue_delay_ms <= frame_ms * 2.0
            && jitter_ratio < 0.25
        {
            client.probe_frames = (client.probe_frames + 0.25).min(max_probe_frames * 0.5);
        } else if queue_delay_ms > frame_ms * high_delay_frames || jitter_ratio > 0.5 {
            client.probe_frames = (client.probe_frames * 0.5).max(1.0);
        } else if queue_delay_ms > frame_ms * 2.0 || !browser_ready(client) {
            client.probe_frames = (client.probe_frames - 0.5).max(0.0);
        }
    } else {
        client.inflight_bytes = 0;
    }
}

/// Process a surface ACK.  Feeds delivery_bps and goodput estimates (same
/// pipe) from the surface inflight queue.  Does NOT update rtt_ms / min_rtt_ms
/// — surface frames are large and their wall-clock delivery time is dominated
/// by serialization and wire transfer, not network latency.  Feeding those
/// samples into the shared RTT inflates it by orders of magnitude and
/// destabilises terminal pacing and congestion control.
/// Record a surface frame handed to the writer: queue it for ack matching
/// (evicting the oldest orphans past the cap) and fold its size into the
/// per-surface EWMA the bandwidth controller budgets against.
fn record_surface_frame_sent(
    client: &mut ClientState,
    surface_id: u16,
    bytes: usize,
    is_keyframe: bool,
    now: Instant,
) {
    // Computed before the mutable borrow below; the cap tracks the link's
    // bandwidth-delay product, so it is not a constant.
    let cap = surface_inflight_cap(client);
    while client.surface_inflight_frames.len() >= cap {
        if let Some(orphan) = client.surface_inflight_frames.pop_front() {
            client.surface_inflight_bytes =
                client.surface_inflight_bytes.saturating_sub(orphan.bytes);
        }
    }
    client.surface_inflight_bytes = client.surface_inflight_bytes.saturating_add(bytes);
    client
        .surface_inflight_frames
        .push_back(SurfaceInFlightFrame {
            sent_at: now,
            bytes,
            surface_id,
        });
    if let Some(sub) = client.surface_subs.get_mut(&surface_id) {
        // Keyframes are 5-10× a P-frame; budgeting against them would
        // starve the steady stream.  Seed from one anyway (÷4) so an
        // all-intra encoder doesn't leave the estimate at zero forever.
        sub.frame_bytes = if sub.frame_bytes <= 0.0 {
            if is_keyframe {
                (bytes as f32 / 4.0).max(4_096.0)
            } else {
                bytes as f32
            }
        } else if is_keyframe {
            ewma_with_direction(sub.frame_bytes, bytes as f32, 0.05, 0.05)
        } else {
            ewma_with_direction(sub.frame_bytes, bytes as f32, 0.5, 0.125)
        };
    }
}

fn record_surface_ack(client: &mut ClientState, surface_id: u16) {
    let matched = client
        .surface_inflight_frames
        .iter()
        .position(|f| f.surface_id == surface_id);
    if let Some(frame) = matched.and_then(|i| client.surface_inflight_frames.remove(i)) {
        client.surface_inflight_bytes = client.surface_inflight_bytes.saturating_sub(frame.bytes);
        client.acked_bytes_since_log = client.acked_bytes_since_log.saturating_add(frame.bytes);

        let sample_ms = frame.sent_at.elapsed().as_secs_f32() * 1_000.0;

        // Shared delivery rate (bandwidth, not latency — safe to update).
        let sample_bps = frame.bytes as f32 / sample_ms.max(1.0e-3) * 1_000.0;
        client.delivery_bps = ewma_with_direction(client.delivery_bps, sample_bps, 0.5, 0.125);

        // Shared goodput window — accumulate bytes, flush periodically.
        // Surface traffic at display_fps is sustained, so always use the
        // window-limited EWMA parameters (rise 0.5, fall 0.125).  No
        // jitter tracking — jitter is a terminal congestion-control signal
        // and large keyframe/P-frame variance would poison it.
        client.goodput_window_bytes = client.goodput_window_bytes.saturating_add(frame.bytes);
        let now = Instant::now();
        let goodput_elapsed = now
            .duration_since(client.goodput_window_start)
            .as_secs_f32();
        if goodput_elapsed >= 0.02 {
            let sample_goodput = client.goodput_window_bytes as f32 / goodput_elapsed.max(1.0e-3);
            client.goodput_bps =
                ewma_with_direction(client.goodput_bps, sample_goodput, 0.5, 0.125);
            client.last_goodput_sample_bps =
                (client.last_goodput_sample_bps * 0.99).max(sample_goodput);
            client.goodput_window_bytes = 0;
            client.goodput_window_start = now;
        }

        if client.surface_goodput_window_bytes == 0 {
            // Do not charge a newly opened or long-idle surface stream for
            // time in which it sent no surface traffic.
            client.surface_goodput_window_start = now;
        }
        client.surface_goodput_window_bytes = client
            .surface_goodput_window_bytes
            .saturating_add(frame.bytes);
        let surface_elapsed = now
            .duration_since(client.surface_goodput_window_start)
            .as_secs_f32();
        if surface_elapsed >= SURFACE_GOODPUT_SAMPLE_INTERVAL.as_secs_f32() {
            let sample = client.surface_goodput_window_bytes as f32 / surface_elapsed.max(1.0e-3);
            // Congestion must shrink the reliable byte window promptly;
            // available capacity is probed conservatively to avoid rebuilding
            // the queue immediately after it drains.
            client.surface_goodput_bps = surface_goodput_ewma(client.surface_goodput_bps, sample);
            client.surface_goodput_window_bytes = 0;
            client.surface_goodput_window_start = now;
        }
    }
}

/// Retire a backend frame which the native protocol view did not admit.
///
/// The compositor delivery client accounts a frame when it enters the native
/// event queue. A view can still reject that event when its decoder window is
/// full or it is waiting for a replacement keyframe. Such a frame can never
/// receive a browser ACK, so leaving it in the byte window permanently
/// throttles unrelated future frames.
fn discard_surface_frame(client: &mut ClientState, surface_id: u16) {
    let matched = client
        .surface_inflight_frames
        .iter()
        .rposition(|frame| frame.surface_id == surface_id);
    if let Some(frame) = matched.and_then(|index| client.surface_inflight_frames.remove(index)) {
        client.surface_inflight_bytes = client.surface_inflight_bytes.saturating_sub(frame.bytes);
    }
}

/// Forget every unacked frame for `surface_id`.
///
/// A surface that has gone away (unsubscribed, destroyed, resized) will
/// never be acked, and Wayland reuses surface ids: a later frame on the
/// recycled id would match a minutes-old entry, report an absurd RTT, and
/// drag the goodput estimate — and so the adaptive controller — down.
fn forget_surface_inflight(client: &mut ClientState, surface_id: u16) {
    let forgotten_bytes = client
        .surface_inflight_frames
        .iter()
        .filter(|frame| frame.surface_id == surface_id)
        .map(|frame| frame.bytes)
        .fold(0usize, usize::saturating_add);
    client
        .surface_inflight_frames
        .retain(|f| f.surface_id != surface_id);
    client.surface_inflight_bytes = client
        .surface_inflight_bytes
        .saturating_sub(forgotten_bytes);
}

/// Invalidate one client's encoder state after a compositor surface event.
///
/// Resizes keep the logical subscription and its requested encode target so
/// delivery can rebuild against the new composite.  Destruction is
/// authoritative: Wayland can reuse the id, so every client-side claim keyed
/// by the old id must be retired rather than recreated as an empty encoder
/// state for a surface that no longer exists.
fn invalidate_client_surface(client: &mut ClientState, surface_id: u16, destroyed: bool) -> bool {
    if destroyed {
        client.surface_subscriptions.remove(&surface_id);
        client.surface_view_sizes.remove(&surface_id);
        client.surface_claim_lapses.remove(&surface_id);
    }

    let still_subscribed = client.surface_subscriptions.contains(&surface_id);
    let previous = client.surface_subs.remove(&surface_id);
    if still_subscribed && let Some(previous) = previous {
        let state = client.surface_subs.entry(surface_id).or_default();
        state.scaled_target = previous.scaled_target;
        state.allow_adaptive_scale = previous.allow_adaptive_scale;
        state.max_fps = previous.max_fps;
        state.max_inflight_frames = previous.max_inflight_frames;
        state.frame_bytes = previous.frame_bytes;
        state.adaptive_quantizer = previous.adaptive_quantizer;
        state.rate_stepped_at = previous.rate_stepped_at;
        state.congested_at = previous.congested_at;
        state.adaptive_scale_shift = previous.adaptive_scale_shift;
        state.scale_stepped_at = previous.scale_stepped_at;
        state.adaptive_pressure_at = previous.adaptive_pressure_at;
    }

    let had_vulkan = client.vulkan_video_surfaces.remove(&surface_id).is_some();
    forget_surface_inflight(client, surface_id);
    had_vulkan
}

/// Let a replaced encoder go without paying its teardown here.
///
/// NVENC's destructor unregisters every buffer it imported and destroys the
/// encoder and its CUDA context — 120 ms on this hardware — and each caller
/// below sits on a loop that owes somebody a frame.  The worst is the
/// client's message loop: the time lands on whatever the client sent next,
/// and what follows a re-subscribe is the pane's resize, i.e. the configure
/// the whole restore is waiting for.  Moving a thumbnail back into a pane
/// paid that 120 ms twice over, once in the delayed configure and again in
/// the encoder rebuild it delayed.
///
/// The slot is `None` the moment this is called, so nothing races the
/// teardown for the encoder itself; the buffers it unregisters are kept
/// alive by the imported fds until it does.
/// What a compositor-resident encoder is called, for the client that
/// configures its decoder from it and for the refusals kept against it.
///
/// The chroma is part of the name because it is part of the profile — High
/// 4:4:4 Predictive for H.264, High for AV1 — and promising one while
/// encoding the other misconfigures the decoder.  It is also the reason a
/// device can decline half of a codec: the 4:4:4 profile is the one NVIDIA
/// advertises and cannot encode.
fn vulkan_encoder_name(pref: SurfaceEncoderPreference, is_444: bool) -> &'static str {
    match (pref, is_444) {
        (SurfaceEncoderPreference::VulkanVideoH264, true) => "h264-vulkan 4:4:4",
        (SurfaceEncoderPreference::VulkanVideoH264, false) => "h264-vulkan",
        (SurfaceEncoderPreference::VulkanVideoAV1, true) => "av1-vulkan 4:4:4",
        (SurfaceEncoderPreference::VulkanVideoAV1, false) => "av1-vulkan",
        _ => "vulkan",
    }
}

/// Whether the Vulkan Video tier may be tried for this client's coded extent.
///
/// The extent is deliberately the per-client target, not the compositor's
/// native surface size. Scaling is part of the compositor-resident path.
fn vulkan_video_tier_eligible(
    preferences: &[SurfaceEncoderPreference],
    codec_support: u8,
    width: u32,
    height: u32,
) -> bool {
    !surface_encoder::outranking_encoder_pending(preferences, codec_support, width, height)
}

/// Preferences the server-side creation task may walk without jumping over
/// Vulkan Video. The boolean records that a failure exhausts the predecessors
/// at this extent and should admit Vulkan immediately on the next tick.
fn server_creation_preferences(
    preferences: &[SurfaceEncoderPreference],
    vulkan_eligible: bool,
) -> (Vec<SurfaceEncoderPreference>, bool) {
    let probing_vulkan_predecessors =
        !vulkan_eligible && preferences.iter().any(|pref| pref.is_vulkan_video());
    if probing_vulkan_predecessors {
        (
            preferences
                .iter()
                .copied()
                .take_while(|pref| !pref.is_vulkan_video())
                .collect(),
            true,
        )
    } else {
        (preferences.to_vec(), false)
    }
}

/// Choose the chroma profile for the next attempt at one Vulkan codec.
///
/// A 4:4:4 refusal is profile-specific, not a reason to discard the backend:
/// AV1 High is much rarer than AV1 Main, and the latter still keeps the frame
/// on the compositor's GPU.  A refused 4:2:0 profile exhausts this codec.
fn vulkan_encoder_chroma(
    pref: SurfaceEncoderPreference,
    want_444: bool,
    refused_444: u8,
    declined_444: &HashSet<&'static str>,
) -> bool {
    let refusal_bit = pref.vulkan_refusal_bit();
    if want_444
        && refused_444 & refusal_bit == 0
        && !declined_444.contains(vulkan_encoder_name(pref, true))
    {
        return true;
    }
    false
}

fn vulkan_refusals_for_extent(sub: &mut SurfaceSubState, width: u32, height: u32) -> (u8, u8) {
    let extent = (width, height);
    if sub.vulkan_refused_extent != Some(extent) {
        sub.vulkan_refused = 0;
        sub.vulkan_444_refused = 0;
        sub.vulkan_refused_extent = Some(extent);
    }
    (sub.vulkan_refused, sub.vulkan_444_refused)
}

fn latch_vulkan_refusal(
    sub: &mut SurfaceSubState,
    pref: SurfaceEncoderPreference,
    is_444: bool,
    width: u32,
    height: u32,
) {
    // The compositor reply belongs to the session at this exact extent. If
    // the target moved while the reply was in flight, do not let its refusal
    // poison the new target's attempt.
    let _ = vulkan_refusals_for_extent(sub, width, height);
    if is_444 {
        sub.vulkan_444_refused |= pref.vulkan_refusal_bit();
    } else {
        sub.vulkan_refused |= pref.vulkan_refusal_bit();
    }
}

fn retire_encoder(encoder: Option<SurfaceEncoder>) {
    let Some(encoder) = encoder else { return };
    // Outside a runtime (tests) there is nowhere to hand it to, and no
    // loop it would be holding up either.
    match tokio::runtime::Handle::try_current() {
        Ok(rt) => drop(rt.spawn_blocking(move || drop(encoder))),
        Err(_) => drop(encoder),
    }
}

fn reset_inflight(client: &mut ClientState) {
    // Surface frames sent before the reset will never be acked either;
    // leaving them queued permanently offsets every later ack.
    client.surface_inflight_frames.clear();
    client.surface_inflight_bytes = 0;
    client.surface_goodput_window_bytes = 0;
    client.surface_goodput_window_start = Instant::now();
    client.next_send_at = Instant::now();
    client.browser_backlog_frames = 0;
    client.browser_ack_ahead_frames = 0;
}

#[cfg(test)]
fn is_unset_view_size(rows: u16, cols: u16) -> bool {
    rows == 0 && cols == 0
}

/// This client's say in the size of `surface_id`, if it still has one.
///
/// A size claim is made by Surface Resize. It is released by its 0×0
/// unset, by the client going away, or by `SURFACE_CLAIM_GRACE` elapsing
/// after the viewer stopped watching — but *not* by the unsubscribe itself.
/// A viewer that stops streaming for a moment still has the pane it sized: a
/// hidden page unsubscribes from every surface at once and resubscribes on
/// the way back, and taking its say away for that window resized the Wayland
/// window for every *other* viewer, then again when it returned.
///
/// A scaled subscriber asked to be served a downscale of whatever the
/// surface happens to be, so it gets no say in how big that is. Counting it
/// would defeat the isolation: a fixed encode box is a transport request,
/// not a request to reconfigure the Wayland window the mediated viewers
/// watch.
fn surface_mediation_size(
    client: &ClientState,
    surface_id: u16,
    now: Instant,
) -> Option<(u16, u16, u16)> {
    if client
        .surface_subs
        .get(&surface_id)
        .is_some_and(|s| s.scaled_target.is_some())
    {
        return None;
    }
    if client
        .surface_claim_lapses
        .get(&surface_id)
        .is_some_and(|&lapses_at| lapses_at <= now)
    {
        return None;
    }
    client
        .surface_view_sizes
        .get(&surface_id)
        .copied()
        .filter(|&(width, height, scale_120)| width > 0 && height > 0 && scale_120 > 0)
}

/// Longest Terminal Search query accepted, in bytes.
///
/// The query is a regex, compiled once per PTY on every search while the
/// session lock is held, so its cost is multiplied by the terminal count.
/// The regex engines bound their own compiled size — alacritty sets an NFA
/// size limit and `regex` defaults to 10 MB — but nothing bounded the input,
/// and a frame can carry 16 MiB of it.
const MAX_SEARCH_QUERY: usize = 1024;

/// Largest view dimension a client may ask for, per axis.
///
/// An 8K display at a 4px font is ~540 rows and ~3840 columns, so this is
/// past any real viewport. It exists because Terminal Resize carries two raw
/// `u16`s and only rejected zero: a single client asking for 65535x65535
/// became the mediated size — the minimum across clients, which is its own
/// when it is the only one — and the terminal grid was allocated at that.
#[cfg(test)]
const MAX_VIEW_DIM: u16 = 4096;

/// Clamp a client-supplied view size to something a frame can describe.
///
/// Both bounds matter: the per-axis cap keeps a single absurd dimension out,
/// and the cell budget is the wire's own limit — a grid past
/// [`MAX_CELL_COUNT`] produces frames every receiver rejects, so
/// sizing one is strictly worse than clamping.
#[cfg(test)]
fn clamp_view_size(rows: u16, cols: u16) -> (u16, u16) {
    let rows = rows.min(MAX_VIEW_DIM);
    let mut cols = cols.min(MAX_VIEW_DIM);
    let budget = yas_terminal_model::MAX_CELL_COUNT / (rows as usize).max(1);
    if (cols as usize) > budget {
        cols = budget.max(1) as u16;
    }
    (rows, cols)
}

fn unsubscribe_client_from(client: &mut ClientState, pty_id: u16) -> bool {
    let removed_sub = client.subscriptions.remove(&pty_id);
    client.last_sent.remove(&pty_id);
    client.last_used_rows_sent.remove(&pty_id);
    client.preview_next_send_at.remove(&pty_id);
    client.scroll_offsets.remove(&pty_id);
    client.scroll_caches.remove(&pty_id);
    let removed_view = client.view_sizes.remove(&pty_id).is_some();
    if client.lead == Some(pty_id) {
        client.lead = None;
    }
    removed_sub || removed_view
}

#[cfg(test)]
fn update_client_scroll_state(client: &mut ClientState, pty_id: u16, next_offset: usize) -> bool {
    let prev_offset = client.scroll_offsets.get(&pty_id).copied().unwrap_or(0);
    if prev_offset == next_offset {
        return false;
    }

    if prev_offset == 0 && next_offset > 0 {
        client.scroll_caches.insert(
            pty_id,
            client.last_sent.get(&pty_id).cloned().unwrap_or_default(),
        );
    } else if prev_offset > 0
        && next_offset == 0
        && let Some(cache) = client.scroll_caches.remove(&pty_id)
    {
        if cache.rows() > 0 && cache.cols() > 0 {
            client.last_sent.insert(pty_id, cache);
        } else {
            client.last_sent.remove(&pty_id);
        }
    }

    if next_offset > 0 {
        client.scroll_offsets.insert(pty_id, next_offset);
    } else {
        client.scroll_offsets.remove(&pty_id);
    }
    reset_inflight(client);
    true
}

/// Hold every scrolled-back client still while the app keeps printing.
///
/// A scroll offset is a distance from the live bottom, so lines leaving the
/// viewport slide the text a parked client is reading upward.  A shell is
/// quiet enough for that to pass unnoticed; an agent streaming output is
/// not — the page crawls out from under the reader.  Grow each offset by
/// however many lines actually scrolled and the content stays put, then
/// tell the client so both ends keep naming the same rows.
/// Apply a client's relative Terminal Scroll request against the
/// offset we hold for it right now — which is what makes the request immune
/// to a re-anchor that crossed it on the wire.  Unlike a re-anchor this may
/// start a live client scrolling, since a wheel notch on a live view is how
/// scrolling back begins.
///
/// Returns the new offset only when the client has to be *told* it, which is
/// not the same as "when it changed".  A relative request the client can
/// predict the outcome of needs no answer: it applied the same delta to the
/// same offset before it sent one.  Answering anyway is actively wrong, and
/// wrong in a way that compounds — the answer is absolute, it arrives a round
/// trip late, and a wheel notch is several requests long, so by the time the
/// first lands the client has already moved past it.  Adopting it drags the
/// view back, and the next delta, measured from the position it was dragged
/// back to, comes out too big.  A twelve-row notch went out as 2, 2, 4, 4, 2
/// and landed fourteen rows down; three notches landed forty rows instead of
/// thirty-six, with the view lurching the whole way.
///
/// Clamping is the one outcome the client cannot predict — its own idea of the
/// scrollback's depth is a frame old and never counts the rows the same way —
/// so that is exactly when the answer is worth its round trip.
#[cfg(test)]
fn scroll_client_by(
    client: &mut ClientState,
    pid: u16,
    delta: i64,
    max_offset: usize,
) -> Option<usize> {
    let current = client.scroll_offsets.get(&pid).copied().unwrap_or(0) as i64;
    let requested = current.saturating_add(delta);
    let next = requested.clamp(0, max_offset as i64);
    let changed = update_client_scroll_state(client, pid, next as usize);
    (changed && requested != next).then_some(next as usize)
}

/// Move one client's parked view down by `delta` lines, bounded by the
/// deepest offset that still has content.  Returns the new offset when it
/// changed, which is exactly when the client has to be told.
#[cfg(test)]
fn reanchor_client(
    client: &mut ClientState,
    pid: u16,
    delta: u64,
    max_offset: usize,
) -> Option<usize> {
    let offset = client.scroll_offsets.get(&pid).copied()?;
    let next = offset.saturating_add(delta as usize).min(max_offset);
    if next == offset {
        return None;
    }
    if next == 0 {
        // The whole scrollback scrolled away under the reader (the app
        // cleared its history).  Same hand-back of the cached pre-scroll
        // frame as a client scrolling home itself.
        update_client_scroll_state(client, pid, 0);
    } else {
        client.scroll_offsets.insert(pid, next);
    }
    Some(next)
}

/// Boot-owned opaque resource handles. Backend identifiers may be narrow and
/// reusable; public YAS handles are monotonically allocated and never reused.
struct OpaqueHandleRegistry<T> {
    next_handle: Option<u64>,
    by_backend: HashMap<T, u64>,
    by_handle: HashMap<u64, T>,
}

impl<T> Default for OpaqueHandleRegistry<T> {
    fn default() -> Self {
        Self {
            next_handle: Some(1),
            by_backend: HashMap::new(),
            by_handle: HashMap::new(),
        }
    }
}

impl<T> OpaqueHandleRegistry<T>
where
    T: Copy + Eq + std::hash::Hash,
{
    fn get_or_insert(&mut self, backend: T) -> Option<u64> {
        if let Some(&handle) = self.by_backend.get(&backend) {
            return Some(handle);
        }
        let handle = self.next_handle?;
        self.next_handle = handle.checked_add(1);
        self.by_backend.insert(backend, handle);
        self.by_handle.insert(handle, backend);
        Some(handle)
    }

    fn handle(&self, backend: T) -> Option<u64> {
        self.by_backend.get(&backend).copied()
    }

    fn backend(&self, handle: u64) -> Option<T> {
        self.by_handle.get(&handle).copied()
    }

    fn remove_backend(&mut self, backend: T) -> Option<u64> {
        let handle = self.by_backend.remove(&backend)?;
        self.by_handle.remove(&handle);
        Some(handle)
    }

    fn retain_backends(&mut self, mut keep: impl FnMut(T) -> bool) {
        let removed = self
            .by_backend
            .keys()
            .copied()
            .filter(|backend| !keep(*backend))
            .collect::<Vec<_>>();
        for backend in removed {
            self.remove_backend(backend);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalLifecycle {
    Created {
        terminal_handle: u64,
        backend_id: u16,
        generation: u64,
    },
    Restarted {
        terminal_handle: u64,
        backend_id: u16,
        generation: u64,
    },
    Closed {
        terminal_handle: u64,
        backend_id: u16,
        generation: u64,
    },
}

impl TerminalLifecycle {
    const fn terminal_handle(self) -> u64 {
        match self {
            Self::Created {
                terminal_handle, ..
            }
            | Self::Restarted {
                terminal_handle, ..
            }
            | Self::Closed {
                terminal_handle, ..
            } => terminal_handle,
        }
    }

    const fn backend_id(self) -> u16 {
        match self {
            Self::Created { backend_id, .. }
            | Self::Restarted { backend_id, .. }
            | Self::Closed { backend_id, .. } => backend_id,
        }
    }

    const fn generation(self) -> u64 {
        match self {
            Self::Created { generation, .. }
            | Self::Restarted { generation, .. }
            | Self::Closed { generation, .. } => generation,
        }
    }
}

struct Session {
    ptys: FxHashMap<u16, Pty>,
    terminal_handles: OpaqueHandleRegistry<u16>,
    surface_handles: OpaqueHandleRegistry<u16>,
    native_terminal_views: yas_terminal_backend::Registry,
    native_yas_clients: HashMap<[u8; 16], NativeYasClient>,
    /// Typed YAS Surface resize claims, keyed by connection identity and
    /// compositor surface. The dimensions are physical pixels at scale_120.
    /// Keeping these outside encoder subscriptions lets a temporarily closed
    /// view retain its layout claim and makes cross-connection mediation
    /// independent of request arrival order.
    native_surface_claims: HashMap<([u8; 16], u16), (u16, u16, u16)>,
    compositor: Option<SharedCompositor>,
    /// Wayland sockets minted for one application each, by app id: the instance
    /// currently listening, and the path this side owns and must unlink.
    ///
    /// Keyed by application rather than by instance because that is the bound
    /// worth holding: an application gets at most one live socket, so a
    /// crash-looping one — which mints a fresh instance per backoff retry —
    /// cannot walk the session toward fd exhaustion. Every entry is withdrawn
    /// and unlinked when the session goes.
    /// When the live client catalog last sampled age and bandwidth. Session
    /// scoped, not per client: staggered per-client deadlines would rebuild
    /// every watcher's snapshot once per client per second instead of once.
    catalog_sampled_at: Instant,
    next_client_id: u64,
    next_compositor_id: u16,
    next_pty_id: u16,
    /// Sorted PTY index that gets first use of the session-wide parse budget.
    pty_parse_cursor: usize,
    #[cfg(target_os = "linux")]
    next_screencast_id: u32,
    tick_fires: u32,
    tick_snaps: u32,
    frame_requests: u32,
    surface_commits: u32,
    surface_encodes: u32,
    surface_encode_bytes: u64,
    surface_frames_sent: u32,
    /// Wall-time breakdown for server-side encode jobs.  The worker queue
    /// and post-encode handoff are tracked separately from the encoder call
    /// so high-refresh misses can be assigned to scheduling or to the codec.
    surface_encode_jobs: u32,
    surface_encode_queue_us: u64,
    surface_encode_queue_max_us: u64,
    surface_encode_work_us: u64,
    surface_encode_work_max_us: u64,
    surface_encode_handoff_us: u64,
    surface_encode_handoff_max_us: u64,
    /// Ticks where pixel_snapshot was empty → entire encode loop skipped.
    ticks_pixel_snapshot_empty: u32,
    /// Number of (sid,w,h) tuples in the most recent non-empty pixel_snapshot.
    pixel_snapshot_len: usize,
    clients: HashMap<u64, ClientState>,
    /// Process-global native listener registry and connected channel pairs.
    channels: channel::ChannelFabric,
    /// The compositor has one seat, so one browser drives it at a time. That
    /// browser's marks are hidden from itself — its own cursor and fingers are
    /// already on its screen — and mirrored to every other subscribed viewer.
    surface_inputs: HashMap<(u16, u8), SharedSurfaceInput>,
    /// Direct touch is an implicit-grab sequence. Only this connection may
    /// extend it until all of its contacts are up or it cancels.
    surface_touch_owner: Option<u64>,
    #[cfg(target_os = "linux")]
    pending_portals: HashMap<u32, PendingPortal>,
}

#[cfg(target_os = "linux")]
struct PendingPortal {
    request: yas_desktop::PortalRequest,
    native_authority: Option<[u8; 16]>,
}

#[cfg(target_os = "linux")]
enum NativeMediaInputEvent {
    Credit(media_input::InputCredit),
    Revoked(media_input::InputRevoked),
}

#[cfg(target_os = "linux")]
struct ScreenCastSession {
    session_id: u32,
    app_id: String,
    streams: Vec<ScreenCastStream>,
}

#[cfg(target_os = "linux")]
struct ScreenCastStream {
    surface_id: u16,
    width: u16,
    height: u16,
    source: audio_pw::RawVideoSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SharedSurfaceInput {
    owner: u64,
    /// `REMOTE_INPUT_POINTER` or `REMOTE_INPUT_TOUCH`.  Also the map key, kept
    /// here so a held entry can be compared and mirrored without it.
    kind: u8,
    /// One entry for a pointer, one per live contact for a touchscreen. Inline
    /// capacity covers a five-finger gesture without allocating on the input
    /// path.
    points: SmallVec<[(u16, u16); 5]>,
}

/// A live browser contact: where it is, and which surface it landed on.
///
/// The surface matters because Wayland binds a contact to the surface it went
/// down on, so one viewer's fingers can be spread across two panes — and marks
/// have to be grouped by the surface they are actually on, or one pane's ring
/// gets drawn in another pane's coordinate space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TouchMark {
    surface_id: u16,
    at: (u16, u16),
}

struct TickOutcome {
    next_deadline: Option<Instant>,
}

impl Session {
    /// Re-decide a downscale target whose subscriber set just changed.
    ///
    /// Re-registers it for whoever is left — which re-evaluates whether the
    /// NV12 `OPAQUE_FD` shape is safe, so an NVENC reader gets the zero-copy
    /// path back once a subscriber that needed CPU pixels has gone — or
    /// clears it when nobody is left.
    ///
    /// Clearing unconditionally would be wrong on both counts: it pulls the
    /// buffer out from under clients still registered at that size, and it
    /// leaves survivors on BGRA until something unrelated re-registers them.
    fn resettle_downscale_target(&mut self, surface_id: u16, tw: u32, th: u32) {
        let survivors: Vec<(bool, bool, bool, (u32, u32))> = self
            .clients
            .values()
            .filter_map(|c| {
                let s = c.surface_subs.get(&surface_id)?;
                (s.last_registered_target == Some((tw, th))).then(|| {
                    let is_vulkan = c.vulkan_video_surfaces.contains_key(&surface_id);
                    (
                        s.wants_nv12_opaque,
                        s.wants_opaque_444,
                        !is_vulkan && !s.wants_nv12_opaque,
                        s.last_registered_native.unwrap_or((tw, th)),
                    )
                })
            })
            .collect();
        let Some(cs) = self.compositor.as_mut() else {
            return;
        };
        if let Some(&(first_wants, first_444, first_cpu, (native_w, native_h))) = survivors.first()
        {
            let mode = downscale_target_mode(
                first_wants,
                first_444,
                first_cpu,
                (tw, th),
                survivors
                    .iter()
                    .skip(1)
                    .map(|(wants, is_444, cpu, _)| (Some((tw, th)), *wants, *is_444, *cpu)),
            );
            let _ = cs.handle.command_tx.try_send(
                yas_compositor::CompositorCommand::RegisterDownscaleTarget {
                    surface_id: surface_id as u32,
                    target_w: tw,
                    target_h: th,
                    native_w,
                    native_h,
                    want_nv12_opaque: mode.want_nv12_opaque,
                    want_cpu_pixels: mode.want_cpu_pixels,
                    opaque_is_444: mode.opaque_is_444,
                },
            );
            // Re-registration may replace or remove the Vulkan allocation.
            // Never leave its old exported fd available to a later NVENC
            // subscriber while the compositor builds the new shape.
            cs.last_opaque_pixels.remove(&(surface_id, tw, th));
            cs.mark_pixel_snapshot_dirty();
        } else {
            let _ = cs.handle.command_tx.try_send(
                yas_compositor::CompositorCommand::ClearDownscaleTarget {
                    surface_id: surface_id as u32,
                    target_w: tw,
                    target_h: th,
                },
            );
            cs.last_pixels.remove(&(surface_id, tw, th));
            cs.last_opaque_pixels.remove(&(surface_id, tw, th));
            cs.mark_pixel_snapshot_dirty();
        }
        cs.handle.wake();
    }

    #[cfg(test)]
    fn new() -> Self {
        Self::new_with_boot_generation(0)
    }

    fn new_with_boot_generation(boot_generation: u64) -> Self {
        Self {
            ptys: FxHashMap::default(),
            terminal_handles: OpaqueHandleRegistry::default(),
            surface_handles: OpaqueHandleRegistry::default(),
            native_terminal_views: yas_terminal_backend::Registry::default(),
            native_yas_clients: HashMap::new(),
            native_surface_claims: HashMap::new(),
            compositor: None,
            catalog_sampled_at: Instant::now(),
            next_client_id: 1,
            next_compositor_id: 1,
            next_pty_id: 1,
            pty_parse_cursor: 0,
            #[cfg(target_os = "linux")]
            next_screencast_id: 1,
            clients: HashMap::new(),
            channels: channel::ChannelFabric::new(boot_generation),
            tick_fires: 0,
            tick_snaps: 0,
            frame_requests: 0,
            surface_commits: 0,
            surface_encodes: 0,
            surface_encode_bytes: 0,
            surface_encode_jobs: 0,
            surface_encode_queue_us: 0,
            surface_encode_queue_max_us: 0,
            surface_encode_work_us: 0,
            surface_encode_work_max_us: 0,
            surface_encode_handoff_us: 0,
            surface_encode_handoff_max_us: 0,
            ticks_pixel_snapshot_empty: 0,
            pixel_snapshot_len: 0,
            surface_frames_sent: 0,
            surface_inputs: HashMap::new(),
            surface_touch_owner: None,
            #[cfg(target_os = "linux")]
            pending_portals: HashMap::new(),
        }
    }

    fn ensure_compositor(
        &mut self,
        verbose: bool,
        event_notify: Arc<dyn Fn() + Send + Sync>,
        gpu_device: &str,
    ) -> &str {
        if self.compositor.is_none() {
            #[cfg(target_os = "linux")]
            let session_id = self.next_compositor_id;
            self.next_compositor_id = self.next_compositor_id.wrapping_add(1);
            // Create the epoch before spawning anything so audio and video
            // share the same time origin for A/V sync.
            #[cfg(target_os = "linux")]
            let created_at = Instant::now();
            #[cfg(target_os = "linux")]
            let media_notify = event_notify.clone();
            #[cfg(target_os = "linux")]
            let desktop_notify = event_notify.clone();
            let handle = yas_compositor::spawn_compositor(verbose, event_notify, gpu_device);
            // Ahead of the desktop bus and of every PTY, because both export
            // DISPLAY at spawn and an app that starts without it has no X at
            // all.  The compositor is told whose connection to expect, so the
            // X session lands on one screen instead of one per window.
            #[cfg(target_os = "linux")]
            let xwayland = if cfg!(test) {
                None
            } else {
                xwayland::Xwayland::spawn(&handle.socket_name, verbose)
            };
            #[cfg(target_os = "linux")]
            if let Some(bridge) = xwayland.as_ref() {
                let _ = handle
                    .command_tx
                    .send(yas_compositor::CompositorCommand::SetXwaylandPid { pid: bridge.pid() });
                handle.wake();
            }
            #[cfg(target_os = "linux")]
            let mut desktop_bus = if cfg!(test) {
                None
            } else {
                match desktop_bus::DesktopBus::spawn(
                    &handle.socket_name,
                    xwayland.as_ref().map(xwayland::Xwayland::display),
                    verbose,
                    desktop_notify,
                ) {
                    Ok(bus) => Some(bus),
                    Err(e) => {
                        if verbose {
                            eprintln!("[desktop-bus] {e}");
                        }
                        None
                    }
                }
            };
            #[cfg(target_os = "linux")]
            let audio_broadcast = audio::AudioBroadcast::new();
            #[cfg(target_os = "linux")]
            let audio_pipeline = {
                let desktop_bus_address = desktop_bus.as_ref().map(|bus| bus.address().to_owned());
                let audio_disabled = std::env::var("YAS_AUDIO")
                    .map(|v| v == "0")
                    .unwrap_or(false);
                let media_input_enabled = std::env::var("YAS_MEDIA_INPUT")
                    .map_or(true, |value| value != "0")
                    && (std::env::var("YAS_MEDIA_MICROPHONE").map_or(true, |value| value != "0")
                        || std::env::var("YAS_MEDIA_CAMERA").map_or(true, |value| value != "0"));
                let screencast_enabled =
                    std::env::var("YAS_PORTALS").map_or(true, |value| value != "0");
                if (!audio_disabled || media_input_enabled || screencast_enabled)
                    && audio::pipewire_available()
                    && desktop_bus_address.is_some()
                {
                    let runtime_dir = std::path::Path::new(&handle.socket_name)
                        .parent()
                        .unwrap_or(std::path::Path::new("/tmp"));
                    let bitrate = std::env::var("YAS_AUDIO_BITRATE")
                        .ok()
                        .and_then(|v| v.parse::<i32>().ok())
                        .unwrap_or(0);
                    // Wrap in block_in_place so the thread::sleep calls
                    // inside spawn() don't stall the tokio runtime.
                    let broadcast = audio_broadcast.clone();
                    tokio::task::block_in_place(|| {
                        match audio::AudioPipeline::spawn(
                            runtime_dir,
                            session_id,
                            desktop_bus_address.as_deref().unwrap(),
                            bitrate,
                            verbose,
                            created_at,
                            broadcast,
                        ) {
                            Ok(pipeline) => {
                                if verbose {
                                    eprintln!(
                                        "[audio] pipeline started, PULSE_SERVER={}",
                                        pipeline.pulse_server_path(),
                                    );
                                }
                                Some(pipeline)
                            }
                            Err(e) => {
                                eprintln!("[audio] failed to start pipeline: {e}");
                                None
                            }
                        }
                    })
                } else {
                    if verbose && (!audio_disabled || media_input_enabled || screencast_enabled) {
                        let missing = audio::missing_pipewire_binaries();
                        let load_err = audio_pw::load_error();
                        if !missing.is_empty() {
                            eprintln!(
                                "[audio] audio disabled: missing binaries on $PATH: {}",
                                missing.join(", ")
                            );
                        }
                        if !load_err.is_empty() {
                            eprintln!("[audio] audio disabled: {load_err}");
                        }
                        if missing.is_empty() && load_err.is_empty() {
                            eprintln!(
                                "[audio] audio disabled (reason not recorded; call pipewire_available() logged above)"
                            );
                        }
                    }
                    None
                }
            };

            // Only now: the portal frontend gets one shot at connecting to
            // PipeWire, and the socket it needs belongs to the pipeline above.
            #[cfg(target_os = "linux")]
            if let Some(bus) = desktop_bus.as_mut() {
                let remote = audio_pipeline
                    .as_ref()
                    .map(audio::AudioPipeline::pipewire_remote_path);
                if verbose {
                    match remote.as_deref() {
                        Some(path) => eprintln!("[portal] starting with PIPEWIRE_REMOTE={path}"),
                        None => eprintln!("[portal] starting without PipeWire (no audio pipeline)"),
                    }
                }
                bus.start_portal(remote.as_deref(), verbose);
            }

            self.compositor = Some(SharedCompositor {
                handle,
                wayland_clipboard_owned: false,
                surfaces: FxHashMap::default(),
                surface_text_inputs: FxHashMap::default(),
                surface_cursors: FxHashMap::default(),
                surface_activation: None,
                surface_activation_revision: 0,
                last_pixels: HashMap::new(),
                last_opaque_pixels: HashMap::new(),
                pixel_snapshot: Arc::new(Vec::new()),
                opaque_pixel_snapshot: Arc::new(Vec::new()),
                pixel_snapshot_dirty: false,
                last_encoded: HashMap::new(),
                frame_clock_intervals: FxHashMap::default(),
                frame_clocks_dirty: true,
                #[cfg(target_os = "linux")]
                created_at,
                pixel_generation: 0,
                last_blanket_frame_request: Instant::now(),
                last_configured_size: FxHashMap::default(),
                last_resize_at: FxHashMap::default(),
                declined_vulkan_444_encoders: HashSet::new(),
                resize_inflight: FxHashMap::default(),
                pending_resize: FxHashMap::default(),
                native_sizes: FxHashMap::default(),
                #[cfg(target_os = "linux")]
                audio_pipeline,
                #[cfg(target_os = "linux")]
                desktop_bus,
                #[cfg(target_os = "linux")]
                xwayland,
                #[cfg(target_os = "linux")]
                desktop_state: DesktopBackendState::default(),
                #[cfg(target_os = "linux")]
                desktop_menus: HashMap::new(),
                #[cfg(all(target_os = "linux", test))]
                native_desktop_commands: Vec::new(),
                #[cfg(target_os = "linux")]
                desktop_removed_notifications: HashMap::new(),
                #[cfg(target_os = "linux")]
                mpris_state: MprisBackendState::default(),
                #[cfg(all(target_os = "linux", test))]
                native_media_state_override: None,
                #[cfg(target_os = "linux")]
                mpris_position_observed_at: HashMap::new(),
                #[cfg(target_os = "linux")]
                native_mpris_results: HashMap::new(),
                #[cfg(target_os = "linux")]
                native_media_input_events: HashMap::new(),
                #[cfg(target_os = "linux")]
                media_input: media_input::MediaInput::with_notify(media_notify),
                #[cfg(target_os = "linux")]
                screencasts: HashMap::new(),
                #[cfg(target_os = "linux")]
                audio_broadcast,
                #[cfg(target_os = "linux")]
                audio_session_id: session_id,
                #[cfg(target_os = "linux")]
                last_audio_restart: None,
                #[cfg(target_os = "linux")]
                audio_restart_needed: false,
                #[cfg(target_os = "linux")]
                audio_restart_inflight: false,
                #[cfg(target_os = "linux")]
                last_audio_liveness_check: None,
            });
            // A compositor started for this new client comes up with touch off,
            // so re-assert whatever the existing viewers asked for.
            if self.wants_direct_touch() {
                self.sync_touch_capability();
            }
            // Clients can report their display rates before the first GUI
            // request starts the compositor.  Seed the new output from the
            // current cross-client maximum instead of its hard-coded 60 Hz
            // default.
            self.sync_compositor_refresh_rate();
        }
        &self.compositor.as_ref().unwrap().handle.socket_name
    }

    /// Returns the `PULSE_SERVER` path if the audio pipeline is active.
    #[cfg(target_os = "linux")]
    fn pulse_server_path(&self) -> Option<String> {
        self.compositor
            .as_ref()
            .and_then(|cs| cs.audio_pipeline.as_ref())
            .map(|ap| ap.pulse_server_path())
    }

    /// Returns the `PIPEWIRE_REMOTE` path if the audio pipeline is active.
    #[cfg(target_os = "linux")]
    fn pipewire_remote_path(&self) -> Option<String> {
        self.compositor
            .as_ref()
            .and_then(|cs| cs.audio_pipeline.as_ref())
            .map(|ap| ap.pipewire_remote_path())
    }

    /// Returns the compositor-scoped private session bus address.
    #[cfg(target_os = "linux")]
    fn desktop_bus_address(&self) -> Option<String> {
        self.compositor
            .as_ref()
            .and_then(|cs| cs.desktop_bus.as_ref())
            .map(|bus| bus.address().to_string())
    }

    #[cfg(not(target_os = "linux"))]
    fn desktop_bus_address(&self) -> Option<String> {
        None
    }

    /// The `DISPLAY` X11 apps in this session should use, when a bridge is
    /// running. `None` means this session has no X at all, and no app is
    /// told otherwise.
    #[cfg(target_os = "linux")]
    fn x_display(&self) -> Option<String> {
        self.compositor
            .as_ref()
            .and_then(|cs| cs.xwayland.as_ref())
            .map(|bridge| bridge.display().to_string())
    }

    #[cfg(not(target_os = "linux"))]
    fn x_display(&self) -> Option<String> {
        None
    }

    fn live_ptys(&self) -> usize {
        self.ptys.values().filter(|pty| !pty.exited).count()
    }

    fn note_terminal_created(
        &mut self,
        backend_id: u16,
        generation: u64,
    ) -> Option<TerminalLifecycle> {
        Some(TerminalLifecycle::Created {
            terminal_handle: self.terminal_handles.get_or_insert(backend_id)?,
            backend_id,
            generation,
        })
    }

    fn note_terminal_restarted(
        &mut self,
        backend_id: u16,
        generation: u64,
    ) -> Option<TerminalLifecycle> {
        self.native_terminal_views.restart_backend(backend_id);
        Some(TerminalLifecycle::Restarted {
            terminal_handle: self.terminal_handles.handle(backend_id)?,
            backend_id,
            generation,
        })
    }

    fn note_terminal_closed(
        &mut self,
        backend_id: u16,
        generation: u64,
    ) -> Option<TerminalLifecycle> {
        self.native_terminal_views.remove_backend(backend_id);
        Some(TerminalLifecycle::Closed {
            terminal_handle: self.terminal_handles.remove_backend(backend_id)?,
            backend_id,
            generation,
        })
    }

    fn terminal_handle(&self, backend_id: u16) -> Option<u64> {
        self.terminal_handles.handle(backend_id)
    }

    fn terminal_backend(&self, terminal_handle: u64) -> Option<u16> {
        self.terminal_handles.backend(terminal_handle)
    }

    fn surface_handle(&self, backend_id: u16) -> Option<u64> {
        self.surface_handles.handle(backend_id)
    }

    fn surface_backend(&self, surface_handle: u64) -> Option<u16> {
        self.surface_handles.backend(surface_handle)
    }

    fn allocate_pty_id(&mut self, max_ptys: usize) -> Option<u16> {
        // Live terminals only.  Counting exited-but-retained ones would let a
        // client that runs 256 short commands hit a cap of 256 with nothing
        // actually running; those are bounded separately, by retention.
        if max_ptys > 0 && self.live_ptys() >= max_ptys {
            // A status-reporting Terminal Create caller receives `BUDGET`.
            // Keep the server-side diagnostic as well so the exhausted limit
            // is visible without inspecting a peer response.
            eprintln!("yas-server: refusing CREATE, YAS_MAX_PTYS ({max_ptys}) reached");
            return None;
        }
        let start = self.next_pty_id;
        let mut id = start;
        loop {
            if !self.ptys.contains_key(&id) {
                self.next_pty_id = if id == u16::MAX { 1 } else { id + 1 };
                return Some(id);
            }
            id = if id == u16::MAX { 1 } else { id + 1 };
            if id == start {
                return None;
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn start_screencast(
        &mut self,
        request: &yas_desktop::PortalScreenCastRequest,
        selected: &[u16],
    ) -> Result<(u32, Vec<yas_desktop::PortalStream>), String> {
        let allowed = request
            .candidates
            .iter()
            .map(|candidate| candidate.surface_id)
            .collect::<HashSet<_>>();
        if selected.is_empty()
            || selected.len() > if request.multiple { 4 } else { 1 }
            || selected.iter().copied().collect::<HashSet<_>>().len() != selected.len()
            || selected
                .iter()
                .any(|surface_id| !allowed.contains(surface_id))
        {
            return Err("invalid ScreenCast surface selection".into());
        }
        let current_streams = self
            .compositor
            .as_ref()
            .map(|compositor| {
                compositor
                    .screencasts
                    .values()
                    .map(|session| session.streams.len())
                    .sum::<usize>()
            })
            .unwrap_or(0);
        if current_streams.saturating_add(selected.len()) > max_screencast_streams() {
            return Err("ScreenCast stream budget exhausted".into());
        }
        let start_id = self.next_screencast_id.max(1);
        let mut session_id = start_id;
        loop {
            if !self
                .compositor
                .as_ref()
                .is_some_and(|compositor| compositor.screencasts.contains_key(&session_id))
            {
                self.next_screencast_id = session_id.wrapping_add(1).max(1);
                break;
            }
            session_id = session_id.wrapping_add(1).max(1);
            if session_id == start_id {
                return Err("ScreenCast session IDs exhausted".into());
            }
        }
        let compositor = self
            .compositor
            .as_mut()
            .ok_or_else(|| "compositor unavailable".to_string())?;
        let runtime_dir = compositor
            .audio_pipeline
            .as_ref()
            .map(|pipeline| pipeline.runtime_dir.clone())
            .ok_or_else(|| "PipeWire runtime unavailable".to_string())?;
        let mut streams = Vec::with_capacity(selected.len());
        for &surface_id in selected {
            let candidate = request
                .candidates
                .iter()
                .find(|candidate| candidate.surface_id == surface_id)
                .ok_or_else(|| "ScreenCast surface disappeared".to_string())?;
            let current = compositor
                .surfaces
                .get(&surface_id)
                .ok_or_else(|| "ScreenCast surface disappeared".to_string())?;
            let current_dimensions = compositor
                .native_sizes
                .get(&surface_id)
                .copied()
                .unwrap_or((u32::from(current.width), u32::from(current.height)));
            if current_dimensions != (u32::from(candidate.width), u32::from(candidate.height)) {
                return Err("ScreenCast surface changed after selection".into());
            }
            let source = tokio::task::block_in_place(|| {
                audio_pw::RawVideoSource::start(
                    &runtime_dir,
                    session_id,
                    surface_id,
                    candidate.width,
                    candidate.height,
                    30,
                )
            })?;
            streams.push(ScreenCastStream {
                surface_id,
                width: candidate.width,
                height: candidate.height,
                source,
            });
        }
        let portal_streams = streams
            .iter()
            .map(|stream| yas_desktop::PortalStream {
                surface_id: stream.surface_id,
                node_id: stream.source.node_id(),
                pipewire_serial: stream.source.serial(),
                width: stream.width,
                height: stream.height,
            })
            .collect::<Vec<_>>();
        let newly_active = streams
            .iter()
            .filter(|stream| {
                !compositor.screencasts.values().any(|session| {
                    session
                        .streams
                        .iter()
                        .any(|known| known.surface_id == stream.surface_id)
                })
            })
            .map(|stream| stream.surface_id)
            .collect::<Vec<_>>();
        compositor.screencasts.insert(
            session_id,
            ScreenCastSession {
                session_id,
                app_id: request.app_id.clone(),
                streams,
            },
        );
        for surface_id in newly_active {
            let _ = compositor
                .handle
                .command_tx
                .send(CompositorCommand::SetScreenCastActive {
                    surface_id,
                    active: true,
                });
        }
        compositor.handle.wake();
        compositor.frame_clocks_dirty = true;
        Ok((session_id, portal_streams))
    }

    #[cfg(target_os = "linux")]
    fn stop_screencast(&mut self, session_id: u32) -> bool {
        let Some(compositor) = self.compositor.as_mut() else {
            return false;
        };
        let Some(session) = compositor.screencasts.remove(&session_id) else {
            return false;
        };
        let removed_surfaces = session
            .streams
            .iter()
            .map(|stream| stream.surface_id)
            .collect::<HashSet<_>>();
        for surface_id in removed_surfaces {
            let still_active = compositor.screencasts.values().any(|known| {
                known
                    .streams
                    .iter()
                    .any(|stream| stream.surface_id == surface_id)
            });
            if !still_active {
                let _ = compositor
                    .handle
                    .command_tx
                    .send(CompositorCommand::SetScreenCastActive {
                        surface_id,
                        active: false,
                    });
            }
        }
        compositor.handle.wake();
        compositor.frame_clocks_dirty = true;
        true
    }

    /// Make `owner` the latest driver of `surface_id`'s pointer mark.
    fn update_surface_pointer(&mut self, owner: u64, surface_id: u16, x: u16, y: u16) {
        self.update_surface_input(
            owner,
            surface_id,
            REMOTE_INPUT_POINTER,
            std::iter::once((x, y)).collect(),
        );
    }

    /// Mirror `owner`'s live marks of one kind on one surface to its peers.
    ///
    /// The owner is told to draw nothing: its own cursor and its own fingers are
    /// already on its screen.
    ///
    /// Marks are held per `(surface, kind)`. Per surface because one user's
    /// fingers can span two panes, and a single slot made each pane's motion
    /// retire the other's — flicker at touch-move rate. Per kind because a
    /// touchscreen laptop drives both at once, and a single slot made lifting a
    /// finger erase that same viewer's live cursor.
    fn update_surface_input(
        &mut self,
        owner: u64,
        surface_id: u16,
        kind: u8,
        points: SmallVec<[(u16, u16); 5]>,
    ) {
        if points.is_empty() {
            self.retire_surface_input(owner, surface_id, kind);
            return;
        }
        let next = SharedSurfaceInput {
            owner,
            kind,
            points,
        };
        let key = (surface_id, kind);
        if self.surface_inputs.get(&key) == Some(&next) {
            return;
        }

        // This runs on every forwarded input event — an unthrottled `mousemove`
        // or `touchmove` rate — and each `send_outbox` counts against the frame
        // budget that gates surface video and paced terminal output, so send only
        // what actually changed.  The owner's own message is constant (draw
        // nothing), so it needs sending once per hand-off, not once per motion.
        let owner_changed = self.surface_inputs.get(&key).map(|held| held.owner) != Some(owner);
        for (&client_id, client) in &self.clients {
            if !client.surface_subscriptions.contains(&surface_id) {
                continue;
            }
            if client_id == owner {
                if owner_changed {
                    let _ = enqueue_surface_remote_input(client, surface_id, owner, kind, &[]);
                }
                continue;
            }
            let _ = enqueue_surface_remote_input(client, surface_id, owner, kind, &next.points);
        }
        self.surface_inputs.insert(key, next);
    }

    /// Withdraw one kind of mark from one surface, if `owner` still holds it.
    fn retire_surface_input(&mut self, owner: u64, surface_id: u16, kind: u8) {
        let key = (surface_id, kind);
        if self.surface_inputs.get(&key).map(|held| held.owner) != Some(owner) {
            return;
        }
        self.surface_inputs.remove(&key);
        self.hide_surface_input(owner, surface_id, kind);
    }

    /// A retire message names its kind, so it withdraws only those marks and
    /// leaves the same viewer's other kind alone.
    fn hide_surface_input(&self, owner: u64, surface_id: u16, kind: u8) {
        for client in self.clients.values() {
            if client.surface_subscriptions.contains(&surface_id) {
                let _ = enqueue_surface_remote_input(client, surface_id, owner, kind, &[]);
            }
        }
    }

    /// Every mark on a surface is gone — it was destroyed.
    fn clear_surface_pointer(&mut self, surface_id: u16) {
        for kind in [REMOTE_INPUT_POINTER, REMOTE_INPUT_TOUCH] {
            if let Some(input) = self.surface_inputs.remove(&(surface_id, kind)) {
                self.hide_surface_input(input.owner, surface_id, kind);
            }
        }
    }

    /// Every mark this owner holds anywhere — it disconnected, or its pointer
    /// left for somewhere this view cannot speak for.
    fn clear_surface_pointer_owner(&mut self, owner: u64) {
        let held: Vec<(u16, u8)> = self
            .surface_inputs
            .iter()
            .filter(|(_, input)| input.owner == owner)
            .map(|(&key, _)| key)
            .collect();
        for (surface_id, kind) in held {
            self.surface_inputs.remove(&(surface_id, kind));
            self.hide_surface_input(owner, surface_id, kind);
        }
    }

    /// Re-mirror every surface this viewer has contacts on, and retire the ones
    /// it no longer does.
    ///
    /// Derived from the live contact set rather than from the event's surface: a
    /// finger can lift on one pane while another stays down on a second, and
    /// keying the retire off the incoming event left the first pane's rings on
    /// screen with nothing on the glass.
    ///
    /// The whole live set per surface is sent every time, not just the contacts a
    /// message changed: peers draw what is currently down, and a viewer that
    /// joined mid-gesture has never seen the others.
    fn mirror_owner_touch(&mut self, owner: u64) {
        let mut per_surface: HashMap<u16, SmallVec<[(u16, u16); 5]>> = HashMap::new();
        if let Some(client) = self.clients.get(&owner) {
            for mark in client.surface_touch_ids.values() {
                per_surface
                    .entry(mark.surface_id)
                    .or_default()
                    .push(mark.at);
            }
        }
        // A HashMap has no order, and an unstable one would make every motion
        // event look like a change and defeat the dedup in `update_surface_input`.
        for points in per_surface.values_mut() {
            points.sort_unstable();
        }
        // Surfaces this owner still holds touch marks on but has no contacts on.
        let stale: Vec<u16> = self
            .surface_inputs
            .iter()
            .filter(|((surface_id, kind), held)| {
                *kind == REMOTE_INPUT_TOUCH
                    && held.owner == owner
                    && !per_surface.contains_key(surface_id)
            })
            .map(|((surface_id, _), _)| *surface_id)
            .collect();
        for surface_id in stale {
            self.retire_surface_input(owner, surface_id, REMOTE_INPUT_TOUCH);
        }
        for (surface_id, points) in per_surface {
            self.update_surface_input(owner, surface_id, REMOTE_INPUT_TOUCH, points);
        }
    }

    /// Whether any viewer still wants the seat's touch capability.
    fn wants_direct_touch(&self) -> bool {
        self.clients
            .values()
            .any(|client| client.direct_touch_enabled)
    }

    /// Push the seat capability that the current viewer set implies.
    ///
    /// One predicate instead of a hand-rolled refcount per call site: the
    /// compositor's `set_touch_enabled` already early-returns on an unchanged
    /// value, so this is safe to call unconditionally.
    fn sync_touch_capability(&mut self) {
        let enabled = self.wants_direct_touch();
        if let Some(compositor) = self.compositor.as_mut() {
            let _ = compositor
                .handle
                .command_tx
                .send(CompositorCommand::SetTouchEnabled { enabled });
            compositor.handle.wake();
        }
    }

    /// The compositor retired a direct-touch sequence by itself (target
    /// unmapped, touch disabled).  Drop the matching server-side ownership so
    /// another viewer is not locked out until the old owner's fingers lift.
    fn forget_touch_sequence(&mut self, owner_id: Option<u64>) {
        let owners: Vec<u64> = match owner_id {
            Some(owner) => vec![owner],
            None => self.clients.keys().copied().collect(),
        };
        for owner in owners {
            if self.surface_touch_owner == Some(owner) {
                self.surface_touch_owner = None;
            }
            if let Some(client) = self.clients.get_mut(&owner) {
                client.surface_touch_ids.clear();
            }
            // The fingers are gone as far as the compositor is concerned, so the
            // peers must stop drawing them.
            self.clear_surface_pointer_owner(owner);
        }
    }

    /// Seed a newly subscribed view with the marks already on this surface,
    /// instead of waiting for the remote user to move again — which for a touch
    /// gesture already in progress could be never.
    fn send_surface_pointer_to(&self, client_id: u64, surface_id: u16) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };
        // Both kinds: a viewer can be pointing with a mouse and touching at the
        // same time, and each is a separate mark set.
        for kind in [REMOTE_INPUT_POINTER, REMOTE_INPUT_TOUCH] {
            let Some(input) = self.surface_inputs.get(&(surface_id, kind)) else {
                continue;
            };
            // The driver draws nothing: its own cursor and fingers are already
            // on its own screen.
            if input.owner == client_id {
                continue;
            }
            let _ =
                enqueue_surface_remote_input(client, surface_id, input.owner, kind, &input.points);
        }
    }

    fn mediated_size_for_pty(&self, pty_id: u16) -> Option<(u16, u16)> {
        let mut mediated = self.native_terminal_views.mediated_size(pty_id);
        for c in self.clients.values() {
            if let Some((r, cols)) = c.view_sizes.get(&pty_id).copied() {
                mediated = Some(mediated.map_or((r, cols), |(min_rows, min_cols)| {
                    (min_rows.min(r), min_cols.min(cols))
                }));
            }
        }
        mediated.map(|(rows, cols)| (rows.max(1), cols.max(1)))
    }

    fn resize_pty(&mut self, pty_id: u16, rows: u16, cols: u16) -> bool {
        let pty = match self.ptys.get_mut(&pty_id) {
            Some(p) => p,
            None => return false,
        };
        let (cur_rows, cur_cols) = pty.driver.size();
        if cur_rows == rows && cur_cols == cols {
            return false;
        }
        pty.ready_frames.clear();
        pty.driver.resize(rows, cols);
        pty.mark_dirty();
        pty.last_used_rows_sent = pty.last_used_rows_sent.min(rows);
        for c in self.clients.values_mut() {
            if c.subscriptions.contains(&pty_id) {
                c.last_sent.remove(&pty_id);
                c.last_used_rows_sent.remove(&pty_id);
            }
            if c.scroll_caches.remove(&pty_id).is_some() {
                reset_inflight(c);
            }
        }
        if !pty.exited {
            pty::resize_pty_os(&pty.handle, rows, cols);
        }
        true
    }

    // ------------------------------------------------------------------
    // Surface sizing — same consumer-tracking model as PTY sizing.
    // Each client reports how large it can display a surface; the server picks
    // the largest logical size that fits every active viewer, then composites
    // it at the highest density any active viewer requested.
    // ------------------------------------------------------------------

    /// Returns the compositor's mediated (width, height, scale_120) for
    /// `surface_id`, mediated across every client subscribed to it.
    ///
    /// Mediation rule: fit every viewer at the highest density any viewer has.
    ///
    /// - **Smallest logical bound wins** on each axis.  That is the largest
    ///   rectangular logical size that fits every viewer.
    ///   Each client reports its viewport in *physical*
    ///   pixels along with its requested scale (`scale_120`), so we convert each
    ///   client's report to logical pixels (`physical * 120 / scale`)
    ///   before taking the min.  Otherwise a 1× client and a 2× client
    ///   reporting the same logical size would mediate at half the
    ///   intended logical area.
    /// - **Highest scale across the compositor wins** so the densest client
    ///   gets native pixels.  The Wayland output has one global scale, so a
    ///   high-scale viewer of another surface must raise this surface too.
    ///   Lower-scale clients get the same logical size at higher density;
    ///   the per-client encoder then downscales to their physical
    ///   viewport.
    ///
    /// A lower-scale viewer is not served this density: its encode target is
    /// capped at what it can display (`per_client_encode_target` rule 3),
    /// so it gets the window's logical size at its own scale rather than 9x
    /// the pixels it has anywhere to put.  That gives two subscribers two
    /// different targets, which until 2026-08-07 meant the lower-scale one
    /// never received a frame at all — see `OPAQUE_PUBLISH_GRACE`.
    ///
    /// The returned `(width, height)` is in *physical* pixels at the
    /// returned `scale_120` (i.e. `min_logical * max_scale_120 / 120`),
    /// so the existing compositor handler — which converts physical →
    /// logical with the same scale — sees the correct logical surface
    /// size.  `max` clamps the physical size to the encoder's limits.
    /// Pick the per-client encoder source dimensions for one
    /// (client, surface) pair.  This is the size each viewer's bitstream
    /// is encoded at — the encode pipeline downscales from
    /// `(native_w, native_h)` (the compositor's mediated size) into
    /// these dimensions before handing pixels to the encoder.
    ///
    /// Clamping rules (in order):
    ///   1. Preserve native aspect ratio.  The viewport gives us a max
    ///      box; we inscribe a `native_w × native_h`-shaped box inside
    ///      it.  Stretching to fill the viewport would distort the
    ///      frame because the JS canvas copies the encoded image at its
    ///      intrinsic aspect (object-fit: contain) — any aspect
    ///      mismatch we encode is locked into the bitstream and
    ///      letterboxed by the browser.
    ///   2. Cap at `(native_w, native_h)` so we never upscale —
    ///      asking for a larger encoder just wastes bandwidth.
    ///   3. Cap at what this viewer can actually put on screen:
    ///      `native_logical × its own requested scale`. Mediation composites
    ///      at the *highest* scale any viewer asked for, so a 1x viewer
    ///      watching a surface a 3x viewer sized would otherwise be served a frame
    ///      with three times the pixels it has anywhere to put — it draws
    ///      the window at its logical size
    ///      (`YasSurfaceCanvas.presentationBox`) and throws the rest
    ///      away.  `None` disables this: a scaled subscription named its
    ///      box in encoder pixels, not as a pane at a DPR, so
    ///      reinterpreting it would shrink a thumbnail that asked for a
    ///      specific size.
    ///   4. Cap at `max` — this viewer's encoder ceiling, from
    ///      `surface_encode_cap` — preserving aspect across the cap.
    ///      This is per-viewer, not per-surface: on a 5K surface an AV1
    ///      viewer encodes at 5120×2880 while an H.264 viewer watching
    ///      the same surface gets a 3840×2160 downscale of it.
    ///   5. Floor at 2×2 (and even) so the encoder doesn't reject the
    ///      dimensions and chroma subsampling has a valid grid.
    ///
    /// `view_size` is `Some((physical_w, physical_h, scale_120))` when
    /// the client has sent at least one Surface Resize; the
    /// fallback (`None` or zero dimensions) is the compositor's native
    /// size, matching how the surface looked to the very first
    /// subscriber.
    ///
    /// `native_logical` is `(native_w, native_h)` expressed in
    /// surface-logical pixels — the pair the compositor reports alongside
    /// the physical size.  Equal to it whenever the surface sits at 1x,
    /// which makes rule 3 a no-op for every session without a high-DPI
    /// viewer in it.
    fn per_client_encode_target(
        view_size: Option<(u16, u16, u16)>,
        native_w: u32,
        native_h: u32,
        native_logical: Option<(u32, u32)>,
        max: Option<(u16, u16)>,
    ) -> (u32, u32) {
        // Largest box no larger than `(box_w, box_h)` that has the
        // same aspect ratio as `(native_w, native_h)`.
        let inscribe = |box_w: u32, box_h: u32| -> (u32, u32) {
            if native_w == 0 || native_h == 0 || box_w == 0 || box_h == 0 {
                return (box_w, box_h);
            }
            // Use u64 to avoid overflow on the cross-multiply.
            let nw = native_w as u64;
            let nh = native_h as u64;
            let bw = box_w as u64;
            let bh = box_h as u64;
            // Two candidates: width-bound (w=box_w, h=box_w*nh/nw) and
            // height-bound (h=box_h, w=box_h*nw/nh).  Pick whichever
            // fits inside the box.
            let h_for_full_w = (bw * nh) / nw;
            if h_for_full_w <= bh {
                (box_w, h_for_full_w as u32)
            } else {
                let w_for_full_h = (bh * nw) / nh;
                (w_for_full_h as u32, box_h)
            }
        };

        // What this viewer can put on screen, in its own physical pixels:
        // the window's logical size at its requested scale. `div_ceil` so rounding
        // never lands the stream a pixel short of the box it will be drawn
        // into — that pixel would show as a letterbox line.
        let display_cap = |scale_120: u16| -> (u32, u32) {
            match native_logical {
                Some((lw, lh)) if lw > 0 && lh > 0 => {
                    let s = u32::from(scale_120);
                    ((lw * s).div_ceil(120), (lh * s).div_ceil(120))
                }
                // Unknown (or a scaled subscription, which names encoder
                // pixels outright): native is the only ceiling.
                _ => (native_w, native_h),
            }
        };
        let (w, h) = view_size
            .filter(|&(w, h, scale_120)| w > 0 && h > 0 && scale_120 > 0)
            // Cap viewport box to native (no upscale) and to what this
            // viewer can display, before inscribing.
            .map(|(w, h, s)| {
                let (cap_w, cap_h) = display_cap(s);
                (
                    (w as u32).min(native_w).min(cap_w),
                    (h as u32).min(native_h).min(cap_h),
                )
            })
            .map(|(w, h)| inscribe(w, h))
            .unwrap_or((native_w, native_h));
        // Encoder-family cap, also aspect-preserving.
        let (w, h) = match max {
            Some((mw, mh)) if w > mw as u32 || h > mh as u32 => inscribe(mw as u32, mh as u32),
            _ => (w, h),
        };
        // Round to even and floor at 2 — H.264/H.265/AV1 NV12 sampling
        // grids and most encoder APIs (NVENC, VAAPI) require even
        // dimensions.
        let w = (w & !1).max(2);
        let h = (h & !1).max(2);
        // A view within a hair of native is the mediated size plus BSP
        // settle noise (`resize_action` ignores ≤2px nudges) or the even
        // rounding above.  Serve it the native stream: the viewer scales
        // the difference invisibly, and compositor-resident sessions —
        // which compare target to native exactly — stay eligible instead
        // of falling to a server-side downscale over 2px.  Only when
        // native is itself even, so the parity guarantee holds — always
        // true of a mediated native (`mediated_size_for_surface` rounds),
        // so an odd native here means the app picked its own odd size.
        if native_w & 1 == 0
            && native_h & 1 == 0
            && w.abs_diff(native_w) <= 3
            && h.abs_diff(native_h) <= 3
        {
            return (native_w, native_h);
        }
        (w, h)
    }

    /// The size the compositor should render this surface at, given every
    /// client holding a claim on it.
    ///
    /// `prefs` is the configured encoder chain. Each viewer's encode ceiling
    /// is translated from its requested presentation scale to the
    /// compositor's output scale before it limits the source. A low-scale
    /// viewer can therefore drive a source larger than its encoder ceiling
    /// and receive the downscale it asked for. Across viewers the loosest
    /// translated ceiling wins: clamping to the tightest would drag a 5K AV1
    /// viewer down to 4K because another tab only speaks H.264.
    fn mediated_size_for_surface(
        &self,
        surface_id: u16,
        prefs: &[SurfaceEncoderPreference],
    ) -> Option<(u16, u16, u16)> {
        // Per axis: the tightest logical bound, plus the exact
        // physical extent and scale of the client that asked for it.
        let now = Instant::now();
        let compositor_scale = u32::from(self.surface_scale_120(surface_id));
        let mut min_w: Option<(u32, u32, u16)> = None;
        let mut min_h: Option<(u32, u32, u16)> = None;
        let mut source_max: Option<(u16, u16)> = None;
        for c in self.clients.values() {
            let Some((pw, ph, s)) = surface_mediation_size(c, surface_id, now) else {
                continue;
            };
            let s_eff = u32::from(s);
            // Round-half-up so a 1× client and a 2× client both reporting
            // the same logical size land on the same logical integer.
            let lw = ((pw as u32) * 120 + s_eff / 2) / s_eff;
            let lh = ((ph as u32) * 120 + s_eff / 2) / s_eff;
            if min_w.is_none_or(|(m, _, _)| lw < m) {
                min_w = Some((lw, pw as u32, s));
            }
            if min_h.is_none_or(|(m, _, _)| lh < m) {
                min_h = Some((lh, ph as u32, s));
            }
            // Widen the source ceiling to whatever this viewer can be served
            // after its requested downscale. Read from the same clients that
            // get a say in the size — a scaled subscriber already skipped
            // above, and letting a thumbnail's ceiling raise the composite
            // would be as wrong as letting its size influence it.
            if let Some((cw, ch)) = surface_encode_cap(prefs, c, surface_id) {
                // The cap is in this viewer's encoded pixels, while the
                // mediated surface is in compositor pixels at the global
                // output scale. Translate between the two before limiting
                // the source. This is essential below 1x: a lone 1920x1080
                // viewer at 0.25x needs a 7680x4320 1x surface, but still
                // receives only a 1920x1080 downscale. Applying a 4K encoder
                // cap directly to that source shrinks the logical window and
                // leaves the picture filling only half of the pane.
                let source_cap = |cap: u16| {
                    ((u32::from(cap) * compositor_scale) / s_eff).min(u32::from(u16::MAX)) as u16
                };
                let (cw, ch) = (source_cap(cw), source_cap(ch));
                source_max = Some(match source_max {
                    Some((mw, mh)) => (mw.max(cw), mh.max(ch)),
                    None => (cw, ch),
                });
            }
        }
        for (&(_, sid), &(pw, ph, scale_120)) in &self.native_surface_claims {
            if sid != surface_id || pw == 0 || ph == 0 || scale_120 == 0 {
                continue;
            }
            let scale = u32::from(scale_120.max(120));
            let lw = (u32::from(pw) * 120 + scale / 2) / scale;
            let lh = (u32::from(ph) * 120 + scale / 2) / scale;
            if min_w.is_none_or(|(minimum, _, _)| lw < minimum) {
                min_w = Some((lw, u32::from(pw), scale_120));
            }
            if min_h.is_none_or(|(minimum, _, _)| lh < minimum) {
                min_h = Some((lh, u32::from(ph), scale_120));
            }
        }
        let (min_w, min_h) = match (min_w, min_h) {
            (Some(w), Some(h)) => (w, h),
            _ => return None,
        };
        let s = compositor_scale;
        // Back to physical at the chosen (highest) scale — but take the
        // selecting client's own physical extent verbatim when it is
        // already at that scale, because the logical round trip does not
        // return what it was given: at 2x an odd physical extent comes back
        // one pixel *larger* (1001 → 501 → 1002). The surface is then a pixel
        // bigger than the pane that asked for it, `per_client_encode_target`
        // inscribes the native aspect into the smaller viewport, and the
        // difference shows up as a letterbox bar on an otherwise exact fit.
        // Fractional CSS pane widths — what a tiled split produces — make odd
        // physical extents the common case, not the corner one.
        let exact = |(lw, pw, cs): (u32, u32, u16)| -> u32 {
            if u32::from(cs) == s {
                pw
            } else {
                (lw.max(1) * s) / 120
            }
        };
        let pw = exact(min_w).clamp(1, u16::MAX as u32) as u16;
        let ph = exact(min_h).clamp(1, u16::MAX as u32) as u16;
        let (pw, ph) = if let Some((mw, mh)) = source_max {
            (pw.min(mw), ph.min(mh))
        } else {
            (pw, ph)
        };
        // Negotiate onto the 4:2:0 grid instead of configuring a size no
        // encoder can carry.  An odd extent here became an odd native, the
        // per-client target rounded it even, and the two could never agree
        // again: compositor-resident sessions (eligibility is target ==
        // native, exactly) fell to a permanent 1px server-side downscale,
        // and every consumer downstream had to tolerate the off-by-one.
        // H.264 4:2:0 cannot even express an odd display width — its crop
        // units are two luma samples — so the only honest answer to an odd
        // request is the even size below it: the surface stays within the
        // pane (a 1px letterbox, never a crop), and the client learns the
        // real size from SurfaceResized rather than receiving a stream
        // that silently disagrees with what it asked for.
        let pw = (pw & !1).max(2);
        let ph = (ph & !1).max(2);
        Some((pw, ph, s as u16))
    }

    /// Drop claims whose grace has run out, and hand the surfaces they were
    /// constraining back to the viewers still watching.  Returns when the
    /// next claim comes due, so the delivery loop parks until then instead of
    /// waiting for unrelated traffic to notice.
    fn expire_surface_claims(
        &mut self,
        now: Instant,
        prefs: &[SurfaceEncoderPreference],
        verbose: bool,
    ) -> Option<Instant> {
        let mut next: Option<Instant> = None;
        let mut expired: Vec<u16> = Vec::new();
        for c in self.clients.values_mut() {
            c.surface_claim_lapses.retain(|&sid, &mut lapses_at| {
                if lapses_at > now {
                    next = Some(next.map_or(lapses_at, |n: Instant| n.min(lapses_at)));
                    return true;
                }
                c.surface_view_sizes.remove(&sid);
                expired.push(sid);
                false
            });
        }
        if !expired.is_empty() {
            expired.sort_unstable();
            expired.dedup();
            // A surface whose last claim just lapsed has no mediated size at
            // all, so ask about every surface still being mediated too — the
            // one that lapsed may have been holding down a scale others share
            // nothing with, but the viewers left behind still need their own
            // sizes applied.
            expired.extend(self.mediated_surface_ids());
            self.resize_surfaces_to_mediated_sizes(expired, prefs, verbose);
        }
        next
    }

    /// The density to composite one surface at: the highest any of *its*
    /// viewers will actually display.
    ///
    /// Each toplevel is alone on its own `wl_output`, so this is a per-surface
    /// question. Folding it across the session made every window follow the
    /// densest viewer of any *other* window — and because the answer fed the
    /// physical size of every surface, one viewer switching panes moved them
    /// all, twice.
    ///
    /// Fixed-size scaled subscriptions are transport-only downscales and have
    /// no say here, same as in `mediated_size_for_surface`.
    fn surface_scale_120(&self, surface_id: u16) -> u16 {
        let now = Instant::now();
        self.clients
            .values()
            .filter_map(|c| surface_mediation_size(c, surface_id, now))
            .map(|(_, _, scale_120)| scale_120)
            .chain(
                self.native_surface_claims
                    .iter()
                    .filter(move |&(&(_, sid), _)| sid == surface_id)
                    .map(|(_, &(_, _, scale_120))| scale_120),
            )
            .max()
            .unwrap_or(120)
            .max(120)
    }

    /// Refresh is an output property, not a per-stream pacing decision.  Run
    /// Wayland applications at the fastest connected display's cadence; each
    /// client still receives frames at its own independently paced rate.
    fn compositor_refresh_mhz(&self) -> u32 {
        let fps = self
            .clients
            .values()
            .map(|c| c.display_fps)
            .reduce(f32::max)
            .unwrap_or(60.0)
            .max(1.0);
        (fps * 1000.0).round() as u32
    }

    fn sync_compositor_refresh_rate(&self) {
        let Some(cs) = self.compositor.as_ref() else {
            return;
        };
        let _ = cs
            .handle
            .command_tx
            .send(CompositorCommand::SetRefreshRate {
                mhz: self.compositor_refresh_mhz(),
            });
        cs.handle.wake();
    }

    /// Every surface whose logical size participates in mediation.  A global
    /// output-density change requires all of these to be reconfigured at the
    /// new physical size, even when the triggering client watches only one.
    fn mediated_surface_ids(&self) -> Vec<u16> {
        let now = Instant::now();
        self.clients
            .values()
            .flat_map(|c| {
                c.surface_view_sizes
                    .keys()
                    .copied()
                    .filter(move |&sid| surface_mediation_size(c, sid, now).is_some())
            })
            .chain(
                self.native_surface_claims
                    .keys()
                    .map(|&(_, surface_id)| surface_id),
            )
            .collect()
    }

    fn set_native_surface_claim(
        &mut self,
        owner: [u8; 16],
        surface_id: u16,
        (width, height, scale_120): (u16, u16, u16),
        encoder_preferences: &[SurfaceEncoderPreference],
        verbose: bool,
    ) {
        self.native_surface_claims
            .insert((owner, surface_id), (width, height, scale_120.max(120)));
        self.resize_surfaces_to_mediated_sizes([surface_id], encoder_preferences, verbose);
    }

    fn remove_native_surface_claims(
        &mut self,
        owner: [u8; 16],
        encoder_preferences: &[SurfaceEncoderPreference],
        verbose: bool,
    ) -> bool {
        let affected = self
            .native_surface_claims
            .keys()
            .filter_map(|&(claim_owner, surface_id)| (claim_owner == owner).then_some(surface_id))
            .collect::<Vec<_>>();
        if affected.is_empty() {
            return false;
        }
        self.native_surface_claims
            .retain(|&(claim_owner, _), _| claim_owner != owner);
        self.resize_surfaces_to_mediated_sizes(affected, encoder_preferences, verbose);
        true
    }

    /// Ask the compositor for a new surface size, subject to the settle
    /// window in `SURFACE_RESIZE_SETTLE`.  Returns true if the compositor was
    /// told right away; a false return may still mean the size was recorded
    /// and will be dispatched by `flush_due_resizes`.
    fn resize_surface(&mut self, surface_id: u16, width: u16, height: u16, scale_120: u16) -> bool {
        let now = Instant::now();
        let cs = match self.compositor.as_mut() {
            Some(cs) => cs,
            None => return false,
        };
        match resize_action(
            cs.last_configured_size.get(&surface_id).copied(),
            cs.last_resize_at.get(&surface_id).copied(),
            now,
            (width, height, scale_120),
        ) {
            ResizeAction::Ignore => {
                // A drag that ends back where it started leaves nothing to
                // do.  Drop the held size rather than replaying it later,
                // which would configure the surface to a stale intermediate.
                cs.pending_resize.remove(&surface_id);
                false
            }
            ResizeAction::Hold => {
                // Keep only the latest size; the delivery loop dispatches it
                // when the window closes.
                cs.pending_resize
                    .insert(surface_id, (width, height, scale_120));
                false
            }
            ResizeAction::Dispatch => {
                cs.dispatch_resize(surface_id, width, height, scale_120, now);
                true
            }
        }
    }

    /// Returns true if any surface is left holding a resize for its settle
    /// window.  Those are dispatched only by `tick`, so a caller outside the
    /// native delivery loop must nudge it after a view change.
    fn resize_surfaces_to_mediated_sizes<I>(
        &mut self,
        surface_ids: I,
        encoder_preferences: &[SurfaceEncoderPreference],
        verbose: bool,
    ) -> bool
    where
        I: IntoIterator<Item = u16>,
    {
        let mut seen = HashSet::new();
        for sid in surface_ids {
            if !seen.insert(sid) {
                continue;
            }
            if let Some((w, h, scale_120)) =
                self.mediated_size_for_surface(sid, encoder_preferences)
            {
                let dispatched = self.resize_surface(sid, w, h, scale_120);
                if verbose {
                    // The subscribers' own view sizes are the inputs to the
                    // mediation and exist only at runtime, so when a surface
                    // comes out an unexpected size in a shared session this is
                    // the line that says which viewer pinned it there.
                    //
                    // Report which of the three outcomes it was, not just
                    // whether a configure went out: `resize_surface` returns
                    // false for a settle-window hold as well as for a no-op,
                    // and during a drag those mean opposite things — one is
                    // parked for `tick` to send, the other is nothing at all.
                    let outcome = if dispatched {
                        "dispatched"
                    } else if self
                        .compositor
                        .as_ref()
                        .is_some_and(|cs| cs.pending_resize.contains_key(&sid))
                    {
                        "held"
                    } else {
                        "unchanged"
                    };
                    let views = self
                        .clients
                        .values()
                        .filter(|c| c.surface_subscriptions.contains(&sid))
                        .filter_map(|c| c.surface_view_sizes.get(&sid))
                        .map(|&(w, h, s)| format!("{w}x{h}@{s}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!(
                        "mediate-resize: sid={sid} -> {w}x{h} scale={scale_120} {outcome} (views: {views})"
                    );
                }
            }
        }
        self.compositor
            .as_ref()
            .is_some_and(|cs| !cs.pending_resize.is_empty())
    }
}

#[derive(Clone, Copy)]
enum ServerDiagnosticsCount {
    RelayActive,
    RelayPending,
}

#[derive(Default)]
pub(crate) struct ServerDiagnosticsRegistry {
    relay_active: AtomicU64,
    relay_pending: AtomicU64,
    receive: std::sync::Mutex<ReceiveDiagnostics>,
}

#[derive(Default)]
struct ReceiveDiagnostics {
    active_sessions: u64,
    aggregate_limit: u64,
    aggregate_buffered: u64,
}

pub(crate) struct ServerDiagnosticsCountGuard {
    registry: Arc<ServerDiagnosticsRegistry>,
    count: ServerDiagnosticsCount,
}

impl ServerDiagnosticsRegistry {
    pub(crate) fn snapshot(&self) -> yas_wire::core::ServerDiagnostics {
        // These fields form one invariant and must come from one mutation
        // epoch. Independent atomics could pair a stale limit with a newer
        // reservation and make a valid live session fail SESSION_INFO.
        let receive = self
            .receive
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        yas_wire::core::ServerDiagnostics {
            active_sessions: diagnostic_count(receive.active_sessions),
            relay_active: diagnostic_count(self.relay_active.load(Ordering::Acquire)),
            relay_pending: diagnostic_count(self.relay_pending.load(Ordering::Acquire)),
            aggregate_receive_limit: receive.aggregate_limit,
            // Report the counter verbatim. Clamping would conceal a broken
            // receive-budget invariant from SESSION_INFO and `@doctor`.
            aggregate_receive_buffered: receive.aggregate_buffered,
        }
    }

    pub(crate) fn relay_active(self: &Arc<Self>) -> ServerDiagnosticsCountGuard {
        self.relay_active.fetch_add(1, Ordering::AcqRel);
        ServerDiagnosticsCountGuard {
            registry: Arc::clone(self),
            count: ServerDiagnosticsCount::RelayActive,
        }
    }

    pub(crate) fn relay_pending(self: &Arc<Self>) -> ServerDiagnosticsCountGuard {
        self.relay_pending.fetch_add(1, Ordering::AcqRel);
        ServerDiagnosticsCountGuard {
            registry: Arc::clone(self),
            count: ServerDiagnosticsCount::RelayPending,
        }
    }

    pub(crate) fn register_receive_session(&self, limit: u64) {
        let mut receive = self
            .receive
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        receive.active_sessions = receive.active_sessions.saturating_add(1);
        receive.aggregate_limit = receive.aggregate_limit.saturating_add(limit);
    }

    pub(crate) fn reserve_receive(&self, bytes: u64) {
        let mut receive = self
            .receive
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        receive.aggregate_buffered = receive.aggregate_buffered.saturating_add(bytes);
        debug_assert!(receive.aggregate_buffered <= receive.aggregate_limit);
    }

    pub(crate) fn release_receive(&self, bytes: u64) {
        let mut receive = self
            .receive
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        debug_assert!(receive.aggregate_buffered >= bytes);
        receive.aggregate_buffered = receive.aggregate_buffered.saturating_sub(bytes);
    }

    pub(crate) fn unregister_receive_session(&self, limit: u64, buffered: u64) {
        let mut receive = self
            .receive
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        debug_assert!(receive.aggregate_buffered >= buffered);
        debug_assert!(receive.aggregate_limit >= limit);
        debug_assert!(receive.active_sessions != 0);
        receive.aggregate_buffered = receive.aggregate_buffered.saturating_sub(buffered);
        receive.aggregate_limit = receive.aggregate_limit.saturating_sub(limit);
        receive.active_sessions = receive.active_sessions.saturating_sub(1);
        debug_assert!(receive.aggregate_buffered <= receive.aggregate_limit);
    }
}

impl Drop for ServerDiagnosticsCountGuard {
    fn drop(&mut self) {
        let count = match self.count {
            ServerDiagnosticsCount::RelayActive => &self.registry.relay_active,
            ServerDiagnosticsCount::RelayPending => &self.registry.relay_pending,
        };
        atomic_saturating_sub(count, 1);
    }
}

fn diagnostic_count(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn atomic_saturating_sub(value: &AtomicU64, amount: u64) {
    let mut current = value.load(Ordering::Acquire);
    loop {
        debug_assert!(current >= amount);
        let next = current.saturating_sub(amount);
        match value.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

#[cfg(test)]
mod server_diagnostics_registry_tests {
    use super::*;

    #[test]
    fn concurrent_snapshots_preserve_receive_budget_invariant() {
        let registry = Arc::new(ServerDiagnosticsRegistry::default());
        let running = Arc::new(AtomicBool::new(true));
        let sampler_registry = Arc::clone(&registry);
        let sampler_running = Arc::clone(&running);
        let sampler = std::thread::spawn(move || {
            while sampler_running.load(Ordering::Acquire) {
                let snapshot = sampler_registry.snapshot();
                assert!(
                    snapshot.aggregate_receive_buffered <= snapshot.aggregate_receive_limit,
                    "inconsistent diagnostics snapshot: {snapshot:?}"
                );
                if snapshot.active_sessions == 0 {
                    assert_eq!(snapshot.aggregate_receive_limit, 0);
                    assert_eq!(snapshot.aggregate_receive_buffered, 0);
                }
            }
        });
        let workers = (0..4)
            .map(|_| {
                let registry = Arc::clone(&registry);
                std::thread::spawn(move || {
                    for _ in 0..5_000 {
                        registry.register_receive_session(1_024);
                        registry.reserve_receive(512);
                        registry.unregister_receive_session(1_024, 512);
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        running.store(false, Ordering::Release);
        sampler.join().unwrap();
        assert_eq!(
            registry.snapshot(),
            yas_wire::core::ServerDiagnostics {
                active_sessions: 0,
                relay_active: 0,
                relay_pending: 0,
                aggregate_receive_limit: 0,
                aggregate_receive_buffered: 0,
            }
        );
    }
}

struct AppStateInner {
    config: Config,
    events: Arc<events::EventLog>,
    #[cfg(any(unix, windows))]
    process_server: process::Server,
    /// Opaque identifier shared by every connection to this server process.
    boot_generation: u64,
    session: Mutex<Session>,
    pty_fds: PtyFds,
    delivery_notify: Arc<Notify>,
    /// Signalled when a client sends a Shutdown request.
    shutdown_notify: Arc<Notify>,
    /// Broadcast to everything this process hosts alongside the server, so a
    /// hosted edge stops serving browsers when the server it fronts stops.
    hosted_shutdown: Arc<Notify>,
    /// Seals logical endpoint admission, cancels every live connection, and
    /// provides the cleanup barrier used by ordinary server teardown.
    connections: Arc<ConnectionRegistry>,
    /// Live native sessions and the boot-scoped Core SHUTDOWN outcome.
    yas_shutdown: Arc<yas_shutdown::Coordinator>,
    /// Wakes the supervisor loop.  Separate from `delivery_notify` because
    /// the two have opposite duty cycles: delivery only runs while a client
    /// is attached, and lifecycle work is exactly what has to keep running
    /// when none is.
    supervisor_notify: Arc<Notify>,
    /// Tracks the number of currently connected clients for enforcing
    /// `config.max_connections`.
    active_connections: std::sync::atomic::AtomicUsize,
    extensions: Arc<extension::ExtensionService>,
    fonts: font::Service,
    relay: relay::Service,
    selection: yas::SelectionStore,
    diagnostics: Arc<ServerDiagnosticsRegistry>,
}

type AppState = Arc<AppStateInner>;

/// Enter the common shutdown path used by signals, Shutdown requests, fd-channel EOF,
/// and ordinary listener teardown. Admission is sealed before any await.
async fn begin_server_shutdown(state: &AppState) {
    state.connections.seal_shutdown();
    if !state.connections.begin_cleanup() {
        return;
    }
    // Attribute every attempt cancellation to shutdown before cancelling any
    // logical connection. This also seals extension restart admission.
    state.extensions.begin_shutdown().await;
    state.session.lock().await.channels.begin_shutdown();
    state.connections.cancel_all();
    // `notify_one` retains a permit if the accept loop has not entered its
    // select yet; `notify_waiters` would lose that edge. It comes last so an
    // awakened main task observes the complete broadcast/cancellation step.
    state.shutdown_notify.notify_one();
    // Waiters, not one: every hosted service is owed the news.
    state.hosted_shutdown.notify_waiters();
}

fn new_boot_generation() -> u64 {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).expect("failed to generate boot generation");
    u64::from_le_bytes(bytes)
}

#[cfg(unix)]
#[allow(dead_code)]
fn spawn_compositor_child(
    command: &str,
    argv: Option<&[&str]>,
    wayland_socket: &str,
    dir: Option<&str>,
) -> libc::pid_t {
    use std::ffi::CString;
    let pid = pty::fork_child();
    if pid == 0 {
        if let Some(d) = dir {
            let c_dir = CString::new(d).unwrap();
            unsafe {
                libc::chdir(c_dir.as_ptr());
            }
        }
        unsafe {
            let wd_path = std::path::Path::new(wayland_socket);
            if let Some(dir) = wd_path.parent() {
                let xdg = std::env::var_os("XDG_RUNTIME_DIR");
                let needs_update = match &xdg {
                    Some(x) => std::path::Path::new(x) != dir,
                    None => true,
                };
                if needs_update {
                    std::env::set_var("XDG_RUNTIME_DIR", dir);
                }
            }
            std::env::set_var("WAYLAND_DISPLAY", wayland_socket);
            // This helper deliberately exposes only the Wayland socket and
            // removes DISPLAY below, so steer GUI toolkits to their Wayland
            // backends. Without these, Electron/Chromium (Cursor), Firefox,
            // GTK and Qt default to X11 and come up with no window. Only set
            // when unset so an explicit caller/environment override still wins.
            for (k, v) in [
                ("NIXOS_OZONE_WL", "1"),
                ("ELECTRON_OZONE_PLATFORM_HINT", "wayland"),
                ("MOZ_ENABLE_WAYLAND", "1"),
                ("GDK_BACKEND", "wayland"),
                ("QT_QPA_PLATFORM", "wayland"),
                ("SDL_VIDEODRIVER", "wayland"),
            ] {
                if std::env::var_os(k).is_none() {
                    std::env::set_var(k, v);
                }
            }
            std::env::remove_var("DISPLAY");
            std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
            std::env::remove_var("DBUS_SYSTEM_BUS_ADDRESS");
        }
        if let Some(args) = argv {
            let prog = CString::new(args[0]).unwrap();
            let c_args: Vec<CString> = args.iter().map(|a| CString::new(*a).unwrap()).collect();
            let c_ptrs: Vec<*const libc::c_char> = c_args
                .iter()
                .map(|a| a.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect();
            unsafe {
                libc::execvp(prog.as_ptr(), c_ptrs.as_ptr());
            }
        } else {
            let prog = CString::new(command).unwrap();
            let c_ptrs = [prog.as_ptr(), std::ptr::null()];
            unsafe {
                libc::execvp(prog.as_ptr(), c_ptrs.as_ptr());
                libc::_exit(1);
            }
        }
    }
    pid
}

/// Map xterm-256 color index to (r, g, b) in 16-bit per channel.
fn xterm256_color(idx: u8) -> (u16, u16, u16) {
    // Standard 16 colors (0-15)
    const BASE16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let (r8, g8, b8) = if idx < 16 {
        BASE16[idx as usize]
    } else if idx < 232 {
        // 6x6x6 color cube (indices 16-231)
        let n = idx - 16;
        let ri = n / 36;
        let gi = (n % 36) / 6;
        let bi = n % 6;
        let to_val = |v: u8| if v == 0 { 0u8 } else { 55 + 40 * v };
        (to_val(ri), to_val(gi), to_val(bi))
    } else {
        // Grayscale ramp (indices 232-255)
        let v = 8 + 10 * (idx - 232);
        (v, v, v)
    };
    // Scale 8-bit to 16-bit (0xFF -> 0xFFFF)
    let scale = |v: u8| (v as u16) << 8 | v as u16;
    (scale(r8), scale(g8), scale(b8))
}
/// Result of scanning a PTY output chunk in `parse_terminal_queries`.
struct TerminalScan {
    /// Query responses to write back into the PTY (DA1, DSR, OSC color
    /// queries, ...).
    responses: Vec<String>,
    /// Last valid OSC 7 working-directory report in the chunk
    /// (docs/protocol.md, "Working directory tracking"): a percent-decoded
    /// absolute local path of at most [`TERM_CWD_MAX`] bytes.
    osc7_cwd: Option<String>,
    /// OSC 133/633 semantic-prompt markers in the chunk, in order, each
    /// carrying the offset it ended at (docs/design/term-journal.md). Empty
    /// for every shell without integration, which is the common case and
    /// costs one failed prefix comparison per OSC.
    marks: Vec<journal::SemanticMark>,
}

/// This machine's hostname, for filtering OSC 7 host components.  Cached
/// because the scan runs on every PTY output chunk.
fn local_hostname() -> &'static str {
    static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HOST.get_or_init(|| {
        #[cfg(unix)]
        {
            // Reserve the last byte: gethostname need not NUL-terminate on
            // truncation.
            let mut buf = [0u8; 256];
            if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len() - 1) } == 0 {
                let end = buf.iter().position(|&b| b == 0).unwrap_or(0);
                return String::from_utf8_lossy(&buf[..end]).into_owned();
            }
            String::new()
        }
        #[cfg(not(unix))]
        {
            // No gethostname off unix; COMPUTERNAME covers Windows.  Empty
            // just narrows accepted OSC 7 hosts to ""/"localhost".
            std::env::var("COMPUTERNAME").unwrap_or_default()
        }
    })
}

/// Parse an OSC 7 URL (`file://<host><path>`) into a local absolute cwd.
/// Rejects rather than guesses:
/// - non-`file://` payloads;
/// - hosts other than this machine (empty, "localhost", or `local_host`,
///   ASCII-case-insensitively) — a remote-ssh shell's OSC 7 names the
///   remote host, and its path is not a local path;
/// - non-absolute paths (nothing after the host, or no literal `/`);
/// - malformed percent-escapes, embedded NUL, or invalid UTF-8 after
///   decoding;
/// - decoded paths longer than [`TERM_CWD_MAX`] (longer than
///   any kernel-accepted cwd; keeps the pushed message bounded).
fn parse_osc7_url(url: &[u8], local_host: &str) -> Option<String> {
    let rest = url.strip_prefix(b"file://")?;
    // The path starts at the first literal '/'; a percent-encoded slash
    // does not make a path absolute.
    let slash = rest.iter().position(|&b| b == b'/')?;
    let (host, raw_path) = rest.split_at(slash);
    let host_ok = host.is_empty()
        || host.eq_ignore_ascii_case(b"localhost")
        || (!local_host.is_empty() && host.eq_ignore_ascii_case(local_host.as_bytes()));
    if !host_ok {
        return None;
    }
    // Percent-decode: shell integrations encode non-ASCII and reserved
    // bytes as %XX (two hex digits).
    let mut decoded = Vec::with_capacity(raw_path.len());
    let mut i = 0;
    while i < raw_path.len() {
        if raw_path[i] == b'%' {
            let hex = raw_path.get(i + 1..i + 3)?;
            let hi = (hex[0] as char).to_digit(16)?;
            let lo = (hex[1] as char).to_digit(16)?;
            decoded.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            decoded.push(raw_path[i]);
            i += 1;
        }
    }
    if decoded.len() > TERM_CWD_MAX || decoded.contains(&0) {
        return None;
    }
    String::from_utf8(decoded).ok()
}

fn parse_terminal_queries(data: &[u8], size: (u16, u16), cursor: (u16, u16)) -> TerminalScan {
    const DA1_RESPONSE: &[u8] = b"\x1b[?64;1;2;6;9;15;18;21;22c";

    let mut results = Vec::new();
    let mut osc7_cwd = None;
    let mut marks = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] != 0x1b || i + 1 >= data.len() {
            i += 1;
            continue;
        }

        // Handle OSC sequences: \x1b] ... (ST or BEL)
        if data[i + 1] == b']' {
            let osc_start = i + 2;
            // Find the terminator: BEL (\x07) or ST (\x1b\\)
            let mut end = osc_start;
            while end < data.len() {
                if data[end] == 0x07 {
                    break;
                }
                if data[end] == 0x1b && end + 1 < data.len() && data[end + 1] == b'\\' {
                    break;
                }
                end += 1;
            }
            if end < data.len() {
                let payload = &data[osc_start..end];
                // OSC 11 ; ? — query background color
                if payload == b"11;?" {
                    // Respond with dark background (rgb:0000/0000/0000)
                    results.push("\x1b]11;rgb:0000/0000/0000\x1b\\".into());
                }
                // OSC 10 ; ? — query foreground color
                else if payload == b"10;?" {
                    results.push("\x1b]10;rgb:ffff/ffff/ffff\x1b\\".into());
                }
                // OSC 4 ; N ; ? — query palette color N
                else if payload.starts_with(b"4;") && payload.ends_with(b";?") {
                    let idx_bytes = &payload[2..payload.len() - 2];
                    if let Ok(idx_str) = std::str::from_utf8(idx_bytes)
                        && let Ok(idx) = idx_str.parse::<u8>()
                    {
                        let (r, g, b) = xterm256_color(idx);
                        results.push(format!("\x1b]4;{idx};rgb:{r:04x}/{g:04x}/{b:04x}\x1b\\"));
                    }
                }
                // OSC 7 — shell integration reports its working directory
                // as a file:// URL at every prompt (docs/protocol.md,
                // "Working directory tracking").  Last valid report in the
                // chunk wins.
                else if let Some(url) = payload.strip_prefix(b"7;")
                    && let Some(cwd) = parse_osc7_url(url, local_hostname())
                {
                    osc7_cwd = Some(cwd);
                }
                i = end + if data[end] == 0x07 { 1 } else { 2 };
                // OSC 133/633 — shell integration says where each command
                // begins and ends (docs/design/term-journal.md). The offset
                // matters: a marker means whatever the cursor is when the
                // bytes *before* it have been drawn, so the caller replays
                // the chunk in segments split here.
                if journal::enabled()
                    && let Some((kind, dialect)) = journal::parse_mark(payload)
                {
                    marks.push(journal::SemanticMark {
                        kind,
                        dialect,
                        at: i,
                    });
                }
                continue;
            }
            i = end;
            continue;
        }

        // Handle CSI sequences: \x1b[ ...
        if i + 2 >= data.len() || data[i + 1] != b'[' {
            i += 1;
            continue;
        }
        i += 2;
        let has_q = i < data.len() && data[i] == b'?';
        if has_q {
            i += 1;
        }
        let param_start = i;
        while i < data.len() && (data[i].is_ascii_digit() || data[i] == b';') {
            i += 1;
        }
        if i >= data.len() {
            break;
        }
        let final_byte = data[i];
        let params = &data[param_start..i];
        i += 1;
        if has_q {
            continue;
        }
        let resp: Option<String> = match final_byte {
            b'c' if params.is_empty() || params == b"0" => {
                Some(String::from_utf8_lossy(DA1_RESPONSE).into_owned())
            }
            b'n' if params == b"6" => Some(format!("\x1b[{};{}R", cursor.0 + 1, cursor.1 + 1)),
            b'n' if params == b"5" => Some("\x1b[0n".into()),
            b't' if params == b"18" => {
                let (rows, cols) = size;
                Some(format!("\x1b[8;{rows};{cols}t"))
            }
            b't' if params == b"14" => {
                let (rows, cols) = size;
                // Widen to u32 so the cell-size multiply cannot overflow for any
                // u16 terminal dimension (max 65535*16 = 1_048_560, fits in u32).
                // Previously `rows * 16` / `cols * 8` were u16*u16 and panicked
                // (debug) or wrapped (release) for large terminals.
                Some(format!("\x1b[4;{};{}t", rows as u32 * 16, cols as u32 * 8))
            }
            _ => None,
        };
        if let Some(r) = resp {
            results.push(r);
        }
    }
    TerminalScan {
        responses: results,
        osc7_cwd,
        marks,
    }
}

/// Feed one PTY output chunk to the terminal model: answer the queries in it,
/// note any OSC 7 report, and apply any shell-integration markers.
///
/// Markers are positional. `OSC 133 ; C` means "output starts *here*", and
/// here is wherever the cursor lands once every byte before the marker has
/// been drawn — so a chunk carrying markers is handed to the driver in
/// segments split at each one. A chunk carrying none, which is every chunk
/// for every shell without integration, takes the single-call path it always
/// took; the only added cost is a failed prefix comparison per OSC.
///
fn feed_pty_chunk(pty: &mut Pty, data: &[u8]) {
    // A sequence split across a read boundary is invisible to a scan of
    // either half, so the unterminated tail of the last chunk goes back in
    // front of this one. It has already been drawn, so only the scan sees
    // it; the driver still gets `data` and nothing twice.
    let carry_len = pty.osc_carry.len();
    let scanned: std::borrow::Cow<[u8]> = if carry_len == 0 {
        std::borrow::Cow::Borrowed(data)
    } else {
        let mut buf = std::mem::take(&mut pty.osc_carry);
        buf.extend_from_slice(data);
        std::borrow::Cow::Owned(buf)
    };

    let scan = pty::respond_to_queries(
        &pty.handle,
        &scanned,
        pty.driver.size(),
        pty.driver.cursor_position(),
    );

    if journal::enabled() {
        pty.osc_carry.clear();
        if let Some(tail) = journal::unterminated_osc_tail(&scanned) {
            pty.osc_carry.extend_from_slice(&scanned[tail..]);
        }
    }

    if scan.marks.is_empty() {
        pty.driver.process(data);
    } else {
        let mut fed = 0usize;
        for mark in &scan.marks {
            let split = mark.at.saturating_sub(carry_len).min(data.len());
            if split > fed {
                pty.driver.process(&data[fed..split]);
                fed = split;
            }
            let Pty {
                driver, journal, ..
            } = pty;
            let (cursor_seq, cursor_col) = driver.cursor_seq();
            journal.apply(
                mark,
                &journal::MarkContext {
                    cursor_seq,
                    cursor_col,
                    read: &|seq, col, end_seq| {
                        driver
                            .seq_text(seq, col, Some(end_seq + 1), journal::command_max())
                            .text
                    },
                },
            );
        }
        if fed < data.len() {
            pty.driver.process(&data[fed..]);
        }
    }

    let _ = note_osc7_cwd(&mut pty.osc7_cwd, scan.osc7_cwd);
}

/// Ceiling on one Terminal Journal page, well under `MAX_FRAME_SIZE`.
/// The ring bound already caps how many records exist; this caps how much
/// command text a pathological set of them can add up to.
const JOURNAL_REPLY_MAX: usize = 1 << 20;

/// Record an OSC 7 report against a PTY's semantic cwd, returning whether it
/// changed. Shells re-emit OSC 7 at every prompt, so identical repeats must
/// not publish another native state update.
fn note_osc7_cwd(stored: &mut Option<String>, cwd: Option<String>) -> bool {
    let Some(cwd) = cwd else {
        return false;
    };
    if stored.as_deref() == Some(cwd.as_str()) {
        return false;
    }
    *stored = Some(cwd);
    true
}

/// Working-directory precedence for Terminal Cwd (docs/protocol.md,
/// "Working directory tracking"): prefer the cwd the shell itself reported
/// via OSC 7 — it is fresher (re-emitted at every prompt by the interactive
/// shell, not whatever the kernel tracks for the immediate PTY child) and
/// costs nothing, while the kernel fallback (`pty::pty_cwd`: /proc readlink
/// on Linux, proc_pidinfo on macOS) is a per-request syscall that only sees
/// the direct child.  Shells without OSC 7 integration never populate the
/// report, so the kernel path remains the fallback.
fn resolve_term_cwd(osc7: Option<&str>, kernel: impl FnOnce() -> Option<String>) -> Option<String> {
    match osc7 {
        Some(cwd) => Some(cwd.to_owned()),
        None => kernel(),
    }
}

/// The timer state a Terminal Deadline request puts a terminal into: `(deadline,
/// stop_deadline, exit_reason)`.
///
/// Split out from the handler so the stand-down rule is pinned by a test
/// rather than by three assignments that are easy to get half-right: the
/// pending SIGKILL has to be cancelled whether the message re-arms or clears,
/// or a refresh that lands inside the grace kills the terminal it was sent to
/// save.
fn armed_deadline(now: Instant, ms: u32) -> (Option<Instant>, Option<Instant>, u8) {
    let deadline = (ms > 0).then(|| now + Duration::from_millis(ms as u64));
    (deadline, None, EXIT_REASON_NORMAL)
}

/// How often the supervisor sweeps when nothing has woken it.
///
/// On Unix this is a pure backstop — SIGCHLD wakes it the moment a child
/// dies, and the sweep only covers a missed signal (they coalesce, so two
/// children dying together deliver one).  Windows has no SIGCHLD and this is
/// the actual detection latency.
const SUPERVISOR_SWEEP_INTERVAL: Duration = Duration::from_secs(5);
/// Maximum wait for reader EOF after the direct child exits. A descendant can keep the slave open.
const PTY_EXIT_DRAIN_GRACE: Duration = Duration::from_millis(50);

/// Reactive lifecycle loop, deliberately not part of the delivery tick.
///
/// The tick only schedules itself while a client is attached —
/// `blanket_frame_interval` returns `None` on an empty client map and every
/// other deadline it computes is client-gated — so a server with nobody
/// watching parks on `delivery_notify` indefinitely.  That is precisely when
/// a runaway command needs supervising, so lifecycle work gets its own loop.
async fn supervisor_loop(state: AppState) {
    loop {
        // Wake at whichever comes first: something asked us to recompute, an
        // armed deadline or a pending hangup escalation is due, or the backstop
        // sweep comes round.
        let next = {
            let sess = state.session.lock().await;
            earliest_armed_deadline(&sess)
        };
        let next = match (next, pty::next_abandoned_kill()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let sweep = Instant::now() + SUPERVISOR_SWEEP_INTERVAL;
        let wake = next.map_or(sweep, |d| d.min(sweep));
        tokio::select! {
            _ = state.supervisor_notify.notified() => {}
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(wake)) => {}
        }
        supervise(&state).await;
    }
}

/// The soonest instant the supervisor has work to do, or `None` when nothing
/// is armed.
fn earliest_armed_deadline(sess: &Session) -> Option<Instant> {
    sess.ptys
        .values()
        .filter(|pty| !pty.exited)
        .filter_map(|pty| {
            pty.deadline
                .into_iter()
                .chain(pty.stop_deadline)
                .chain(pty.exit_drain_deadline)
                .min()
        })
        .min()
}

/// Terminals to evict to stay inside the retention bounds, oldest first.
///
/// Pure so the policy is testable without a real PTY: it only needs when
/// each exited terminal exited.
fn slots_to_evict(
    mut exited: Vec<(u16, Instant)>,
    now: Instant,
    max_exited: usize,
    linger: Duration,
) -> Vec<u16> {
    exited.sort_by_key(|&(_, at)| at);
    let mut doomed: Vec<u16> = Vec::new();
    if !linger.is_zero() {
        let expired = exited
            .iter()
            .filter(|&&(_, at)| now.duration_since(at) >= linger);
        doomed.extend(expired.map(|&(id, _)| id));
    }
    if max_exited > 0 && exited.len() > max_exited {
        let over = exited.len() - max_exited;
        doomed.extend(exited.iter().take(over).map(|&(id, _)| id));
    }
    doomed.sort_unstable();
    doomed.dedup();
    doomed
}

/// Drop exited terminals that have fallen outside the retention bounds.
///
/// `cleanup_pty_internal` marks a terminal exited and keeps its entry so the
/// output stays readable; nothing but an explicit Terminal Close ever removed
/// one, so a client that creates a terminal per task and never closes it grew
/// the map until the id space ran out. Eviction takes the same path a
/// Terminal Close would and broadcasts the same Terminal Closed event, so clients need no
/// new message to understand it.
///
/// Only ever touches terminals whose command has already exited.
async fn evict_exited(state: &AppState) {
    let now = Instant::now();
    let mut sess = state.session.lock().await;
    let exited: Vec<(u16, Instant)> = sess
        .ptys
        .iter()
        .filter_map(|(&id, pty)| pty.exited_at.map(|at| (id, at)))
        .collect();
    let doomed = slots_to_evict(exited, now, max_exited(), exited_linger());
    for id in doomed {
        let Some(pty) = sess.ptys.remove(&id) else {
            continue;
        };
        let generation = pty.generation;
        // Already exited by construction, so the fd and the child are gone;
        // this is only dropping the retained terminal state.
        drop(pty);
        let _ = sess.note_terminal_closed(id, generation);
        yas_event!(
            state.events,
            EventType::PtyRemove,
            id.to_le_bytes().to_vec()
        );
        state.pty_fds.write().unwrap().remove(&id);
        for client in sess.clients.values_mut() {
            unsubscribe_client_from(client, id);
        }
    }
}

/// Signal numbers for the stop sequence.  Spelled out rather than taken from
/// `libc` because this code is shared with Windows, where `kill_pty` treats
/// the number as an opaque "not SIGINT" and terminates the job.
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;

/// `TimeoutStopSec`: how long a first, polite signal has before the stop
/// sequence escalates to SIGKILL.  One value for both sequences that have one
/// — a deadline's SIGTERM and Terminal Close's SIGHUP — because it answers the
/// same question either time.
fn stop_grace() -> Duration {
    DEADLINE_STOP_GRACE
}

/// Act on terminals whose deadline has come due.
///
/// Expiry is a two-step stop: SIGTERM to the group, then SIGKILL once the
/// grace elapses, so a command that handles SIGTERM gets to unwind. The
/// attribution is recorded now and travels to Terminal Exited later, because the
/// terminal does not finish exiting until the child actually dies.
async fn enforce_deadlines(state: &AppState) {
    let now = Instant::now();
    let mut sess = state.session.lock().await;
    for (&pty_id, pty) in &mut sess.ptys {
        if pty.exited {
            continue;
        }
        if pty.stop_deadline.is_some_and(|d| now >= d) {
            pty.stop_deadline = None;
            yas_event!(state.events, EventType::Deadline, {
                let mut payload = pty_id.to_le_bytes().to_vec();
                payload.push(2);
                payload
            });
            pty::kill_pty(&pty.handle, SIGKILL, true);
        } else if pty.deadline.is_some_and(|d| now >= d) {
            pty.deadline = None;
            pty.exit_reason = EXIT_REASON_DEADLINE;
            pty.stop_deadline = Some(now + stop_grace());
            yas_event!(state.events, EventType::Deadline, {
                let mut payload = pty_id.to_le_bytes().to_vec();
                payload.push(1);
                payload
            });
            pty::kill_pty(&pty.handle, SIGTERM, true);
        }
    }
}

/// One supervisor pass: arm a bounded fallback when the direct child exits.
///
/// Ordered reader EOF normally finalizes first. A descendant can keep the slave open, so the
/// deadline forces one unpaced drain before cleanup instead of waiting forever.
async fn supervise(state: &AppState) {
    let pass_started = Instant::now();
    yas_event!(state.events, EventType::Supervisor, vec![1]);
    let now = Instant::now();
    let fallback_due = {
        let mut sess = state.session.lock().await;
        for pty in sess.ptys.values_mut().filter(|pty| !pty.exited) {
            if pty.exit_drain_deadline.is_none() && pty::poll_child_exited(&pty.handle) {
                pty.exit_drain_deadline = Some(now + PTY_EXIT_DRAIN_GRACE);
            }
        }
        sess.ptys.values().any(|pty| {
            !pty.exited
                && pty
                    .exit_drain_deadline
                    .is_some_and(|deadline| now >= deadline)
        })
    };
    if fallback_due {
        tick(state).await;
    }
    let ready: Vec<(u16, u64)> = {
        let sess = state.session.lock().await;
        sess.ptys
            .iter()
            .filter(|(_, pty)| {
                !pty.exited
                    && pty
                        .exit_drain_deadline
                        .is_some_and(|deadline| now >= deadline)
            })
            .map(|(&id, pty)| (id, pty.generation))
            .collect()
    };
    for (id, generation) in ready {
        cleanup_pty_internal(id, Some(generation), state).await;
    }
    // After the exit scan, never before it: `reap_zombies` waits a child
    // without marking its terminal exited, so between that wait and the next
    // scan the pty is `!exited` with its pid already freed.  Signalling first
    // would aim the stop sequence's `kill(-pid)` at a released process group.
    enforce_deadlines(state).await;
    // Same ordering argument for the sequence Terminal Close starts: the pid is
    // ours to signal only until `reap_zombies` waits it, so escalate first and
    // let the sweep below collect whatever the SIGKILL just killed.
    pty::escalate_abandoned(Instant::now());
    // The backstop still runs, now targeted at owned pids only, so a child
    // whose SIGCHLD we missed cannot linger as a zombie.
    pty::reap_zombies();
    // The audio pipeline's children are nobody else's to collect on this
    // cadence: the health check that reaps them as a side effect lives in
    // the delivery tick, which is asleep whenever no client is attached.
    #[cfg(target_os = "linux")]
    {
        let mut sess = state.session.lock().await;
        let mut stopped_audio = None;
        let mut stopped_media = Vec::new();
        if let Some(cs) = sess.compositor.as_mut()
            && let Some(ap) = cs.audio_pipeline.as_mut()
        {
            ap.reap_children();
        }
        let expired_media = sess
            .compositor
            .as_mut()
            .map(|cs| cs.media_input.expire(Instant::now()))
            .unwrap_or_default();
        for (owner, revoked) in &expired_media {
            if let Some(cs) = sess.compositor.as_mut() {
                let queue = cs.native_media_input_events.entry(*owner).or_default();
                if queue.len() >= 64 {
                    queue.pop_front();
                }
                queue.push_back(NativeMediaInputEvent::Revoked(*revoked));
            }
        }
        let portal_frontend_exited = sess
            .compositor
            .as_mut()
            .and_then(|cs| cs.desktop_bus.as_mut())
            .is_some_and(desktop_bus::DesktopBus::take_portal_frontend_exit);
        if portal_frontend_exited {
            eprintln!("[portal] xdg-desktop-portal exited");
            let pending = std::mem::take(&mut sess.pending_portals);
            for (request_id, _) in pending {
                if let Some(bus) = sess
                    .compositor
                    .as_ref()
                    .and_then(|cs| cs.desktop_bus.as_ref())
                {
                    let _ = bus.try_command(yas_desktop::Command::NativePortal(
                        yas_desktop::PortalResponse {
                            request_id,
                            decision: yas_desktop::PortalResponseDecision::Cancelled,
                            surface_ids: Vec::new(),
                            choices: Vec::new(),
                        },
                    ));
                }
            }
            let screencast_ids = sess
                .compositor
                .as_ref()
                .map(|compositor| compositor.screencasts.keys().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            for session_id in screencast_ids {
                sess.stop_screencast(session_id);
                if let Some(bus) = sess
                    .compositor
                    .as_ref()
                    .and_then(|cs| cs.desktop_bus.as_ref())
                {
                    let _ = bus.try_command(yas_desktop::Command::PortalSessionClosed(session_id));
                }
            }
        }
        // A dead bridge cannot be repaired for the X clients already on it,
        // but it must stop being advertised: the next app to start would
        // otherwise inherit a DISPLAY nothing is listening on, which is worse
        // than having no X — a toolkit given a broken display can fail
        // instead of falling back to Wayland.
        #[cfg(target_os = "linux")]
        {
            let bridge_exited = sess
                .compositor
                .as_mut()
                .and_then(|cs| cs.xwayland.as_mut())
                .is_some_and(|bridge| !bridge.is_alive());
            if bridge_exited && let Some(cs) = sess.compositor.as_mut() {
                eprintln!("[xwayland] bridge exited; X11 applications are unavailable");
                cs.xwayland = None;
            }
        }
        let desktop_bus_exited = sess
            .compositor
            .as_mut()
            .and_then(|cs| cs.desktop_bus.as_mut())
            .is_some_and(|bus| !bus.is_alive());
        if desktop_bus_exited {
            eprintln!("[desktop-bus] private session bus exited");
            let screencast_ids = sess
                .compositor
                .as_ref()
                .map(|compositor| compositor.screencasts.keys().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            for session_id in screencast_ids {
                sess.stop_screencast(session_id);
            }
            if let Some(cs) = sess.compositor.as_mut() {
                cs.desktop_bus = None;
                cs.desktop_state = DesktopBackendState::default();
                cs.desktop_menus.clear();
                cs.desktop_removed_notifications.clear();
                cs.mpris_state = MprisBackendState::default();
                cs.mpris_position_observed_at.clear();
                // PipeWire, WirePlumber, pulse, portals, and the desktop
                // bridge are one compositor service bundle. The old private
                // bus address cannot be repaired for already-running apps.
                stopped_audio = cs.audio_pipeline.take();
                cs.audio_restart_needed = false;
                cs.audio_restart_inflight = false;
                stopped_media = cs
                    .media_input
                    .revoke_all(media_input::InputRevokeReason::BackendFailed);
            }
            for (owner, revoked) in &stopped_media {
                if let Some(cs) = sess.compositor.as_mut() {
                    let queue = cs.native_media_input_events.entry(*owner).or_default();
                    if queue.len() >= 64 {
                        queue.pop_front();
                    }
                    queue.push_back(NativeMediaInputEvent::Revoked(*revoked));
                }
            }
        }
        drop(sess);
        if let Some(audio) = stopped_audio {
            tokio::task::spawn_blocking(move || drop(audio));
        }
    }
    evict_exited(state).await;
    yas_event!(state.events, EventType::Supervisor, {
        let mut payload = vec![2];
        payload.extend_from_slice(
            &(pass_started.elapsed().as_nanos().min(u64::MAX as u128) as u64).to_le_bytes(),
        );
        payload
    });
}

/// Run a terminal's exit path.
///
/// `generation` is the child this cleanup was decided for. Exit detection and reader EOF race, and a
/// client can restart the terminal as soon as it sees Terminal Exited; without this check, stale cleanup
/// would drop the new child's fd and broadcast a second exit for the replacement. `None` means
/// "whatever is there now", for callers that just looked.
async fn cleanup_pty_internal(pty_id: u16, generation: Option<u64>, state: &AppState) {
    let mut sess = state.session.lock().await;
    if let Some(pty) = sess.ptys.get_mut(&pty_id) {
        if generation.is_some_and(|g| g != pty.generation) {
            return;
        }
        if pty.exited {
            return;
        }
        state.pty_fds.write().unwrap().remove(&pty_id);
        pty.exited = true;
        pty.exited_at = Some(Instant::now());
        pty.deadline = None;
        pty.stop_deadline = None;
        pty::close_pty(&pty.handle);
        pty.exit_status = pty::collect_exit_status(&pty.handle);
        yas_event!(state.events, EventType::PtyExit, {
            let mut payload = pty_id.to_le_bytes().to_vec();
            payload.extend_from_slice(&pty.exit_status.to_le_bytes());
            payload.push(pty.exit_reason);
            payload
        });
        pty.mark_dirty();
        // A command still running when the shell dies never gets its `D`
        // marker; closing it here is what stops a waiter hanging until its
        // timeout for output that is never coming.
        let end_seq = pty.driver.cursor_seq().0;
        pty.journal.note_pty_exit(end_seq);
    }
}

fn take_snapshot(pty: &mut Pty) -> FrameState {
    if pty.lflag_last.elapsed() >= Duration::from_millis(250) {
        pty.lflag_cache = pty::pty_lflag(&pty.handle);
        pty.lflag_last = Instant::now();
    }
    let (echo, icanon) = pty.lflag_cache;
    pty.driver.snapshot(echo, icanon)
}

/// How much one in-process session may buffer per direction.
///
/// One wire frame's worth: the reader takes a frame at a time, so a writer that
/// is a frame ahead has said everything the reader can act on.
const LOCAL_SESSION_BUFFER: usize = yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME as usize;

/// A door into this server for a service the server is hosting itself.
///
/// The browser edge and the WebRTC share were separate processes that dialled
/// the IPC socket to reach this one. Hosted here they ask for a session
/// directly and get the same thing an external client gets — admission,
/// registration, cancellation, connection accounting — with the second process
/// and the kernel round trip taken out.
#[derive(Clone)]
pub struct LocalEndpoint {
    state: AppState,
    ingress: mpsc::Sender<tokio::io::DuplexStream>,
}

impl LocalEndpoint {
    /// Notified when the server begins shutting down, so a hosted service can
    /// stop taking work before the process goes.
    pub fn shutdown(&self) -> Arc<Notify> {
        self.state.hosted_shutdown.clone()
    }

    /// Open one in-process session.
    ///
    /// The stream is classified exactly as a socket is, because a caller may
    /// be about to write a composite offer rather than a preface: a share
    /// opens two of these and asks the server to pair them into one session
    /// with a datagram sideband.
    ///
    /// `None` means what it means at the accept loop: this server is not
    /// taking the connection, and the caller should say so to whoever asked.
    pub fn connect(&self) -> Option<tokio::io::DuplexStream> {
        let (client, server) = tokio::io::duplex(LOCAL_SESSION_BUFFER);
        self.ingress.try_send(server).ok().map(|()| client)
    }
}

/// Classify and admit in-process streams, as the accept loop does for sockets.
async fn run_hosted_ingress(
    mut incoming: mpsc::Receiver<tokio::io::DuplexStream>,
    state: AppState,
) {
    let mut pairing =
        yas_composite_transport::Pairing::new(MAX_PENDING_INGRESS, INGRESS_PAIR_TIMEOUT);
    let (classified_tx, mut classified_rx) = mpsc::channel(MAX_PENDING_INGRESS);
    let mut classifying = 0usize;
    let mut expiry = tokio::time::interval(Duration::from_millis(100));
    expiry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            stream = incoming.recv() => {
                let Some(stream) = stream else { return };
                if classifying.saturating_add(pairing.len()) >= MAX_PENDING_INGRESS {
                    continue;
                }
                classifying += 1;
                let classified_tx = classified_tx.clone();
                tokio::spawn(async move {
                    let classified = tokio::time::timeout(
                        INGRESS_CLASSIFY_TIMEOUT,
                        yas_composite_transport::classify(stream),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok);
                    let _ = classified_tx.send(classified).await;
                });
            }
            classified = classified_rx.recv(), if classifying != 0 => {
                classifying -= 1;
                let Some(Some(classified)) = classified else { continue };
                match classified {
                    yas_composite_transport::Ingress::Direct(stream) => {
                        spawn_yas_client(stream, state.clone(), embedded_origin());
                    }
                    yas_composite_transport::Ingress::Composite { offer, stream } => {
                        if let Ok(Some(pair)) = pairing.insert(offer, stream, Instant::now()) {
                            spawn_yas_composite_client(
                                pair.main,
                                pair.datagram,
                                pair.max_datagram,
                                state.clone(),
                                embedded_origin(),
                            );
                        }
                    }
                }
            }
            _ = expiry.tick() => {
                drop(pairing.expire(Instant::now()));
            }
        }
    }
}

/// Who an in-process session's peer is: this process, truthfully.
fn embedded_origin() -> ConnectionOrigin {
    #[cfg(unix)]
    {
        ConnectionOrigin::Local(yas_webserver::local_ipc::PeerCredentials {
            pid: std::process::id(),
            uid: unsafe { libc::geteuid() },
            gid: unsafe { libc::getegid() },
        })
    }
    #[cfg(not(unix))]
    {
        ConnectionOrigin::Network
    }
}

/// Called once the server can serve, with the door to it.
///
/// Anything the server hosts starts here rather than at the top of `run`: a
/// hosted service that opened a session before the delivery loop existed would
/// be talking to a server that cannot answer.
pub type HostedServices = Box<dyn FnOnce(LocalEndpoint) + Send>;

pub async fn run(config: Config) {
    run_hosted(config, None).await;
}

pub async fn run_hosted(config: Config, hosted: Option<HostedServices>) {
    // Embedders may not call `configure_deployment`; in that case freeze the
    // environment now, before any feature mask or service is constructed.
    let _ = ensure_deployment_settings();
    kv::configure_server_name(&config.name);
    let (event_log, startup_event_file) = events::EventLog::from_env();
    yas_event!(event_log, EventType::ServerStart, {
        let mut payload = events::payload_name(env!("CARGO_PKG_VERSION"));
        payload.extend_from_slice(&events::payload_name(config.name.as_str()));
        payload
    });
    #[cfg(any(unix, windows))]
    let process_server = process::Server::new(config.verbose, config.processes);
    let boot_generation = new_boot_generation();
    let logical_cpus = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    eprintln!(
        "{}",
        capacity_diagnostics::sampled_diagnostic(
            extensions_enabled(),
            channels_enabled(),
            logical_cpus,
            deployment_u64,
        )
    );
    let extensions =
        extension::ExtensionService::from_env(config.allow_persistent_extensions, &config.name);
    let fonts = font::Service::from_env().await;
    let relay = relay::Service::from_env();
    let state: AppState = Arc::new(AppStateInner {
        config,
        events: event_log.clone(),
        #[cfg(any(unix, windows))]
        process_server,
        boot_generation,
        session: Mutex::new(Session::new_with_boot_generation(boot_generation)),
        pty_fds: Arc::new(std::sync::RwLock::new(FxHashMap::default())),
        delivery_notify: Arc::new(Notify::new()),
        shutdown_notify: Arc::new(Notify::new()),
        hosted_shutdown: Arc::new(Notify::new()),
        connections: Arc::new(ConnectionRegistry::default()),
        yas_shutdown: Arc::new(yas_shutdown::Coordinator::default()),
        supervisor_notify: Arc::new(Notify::new()),
        active_connections: std::sync::atomic::AtomicUsize::new(0),
        extensions: extensions.clone(),
        fonts,
        relay,
        selection: yas::SelectionStore::new(),
        diagnostics: Arc::new(ServerDiagnosticsRegistry::default()),
    });
    if let Some(file) = startup_event_file {
        match event_log.start_file_stream(&file.path, file.flags).await {
            Ok(stream_id) => yas_event!(event_log, EventType::StreamStart, {
                let mut payload = stream_id.to_le_bytes().to_vec();
                payload.extend_from_slice(&events::payload_name(&file.path));
                payload
            }),
            Err(error) => {
                eprintln!("yas-server: event file stream: {error}");
                yas_event!(
                    event_log,
                    EventType::Error,
                    events::payload_name(&error.to_string())
                );
            }
        }
    }
    extensions.restore(state.clone()).await;

    // Start the compositor eagerly so it is ready before any client
    // connects or any terminal is created.
    if !state.config.skip_compositor {
        let notify = state.delivery_notify.clone();
        let event_notify = Arc::new(move || notify.notify_one()) as Arc<dyn Fn() + Send + Sync>;
        let mut sess = state.session.lock().await;
        sess.ensure_compositor(
            state.config.verbose,
            event_notify,
            &state.config.vaapi_device,
        );
    }

    let delivery_state = state.clone();
    // EXPERIMENT (YAS_TICK_FLOOR_US): minimum spacing between ticks.
    // The delivery loop is notify-driven with no floor, so under a PTY
    // firehose it re-ticks per output chunk and its constant per-tick cost
    // (session lock, pacing, supervision) is billed at whatever rate the
    // producer runs.  0 = current behaviour.
    let tick_floor = std::env::var("YAS_TICK_FLOOR_US")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .map(Duration::from_micros);
    tokio::spawn(async move {
        yas_event!(
            delivery_state.events,
            EventType::TaskStart,
            events::payload_name("delivery")
        );
        let mut next_deadline: Option<Instant> = None;
        let mut last_tick: Option<Instant> = None;
        loop {
            if let Some(deadline) = next_deadline {
                tokio::select! {
                    _ = delivery_state.delivery_notify.notified() => {}
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {}
                }
            } else {
                delivery_state.delivery_notify.notified().await;
            }
            if let (Some(floor), Some(last)) = (tick_floor, last_tick) {
                let since = last.elapsed();
                if since < floor {
                    tokio::time::sleep(floor - since).await;
                }
            }
            last_tick = Some(Instant::now());
            let outcome = tick(&delivery_state).await;
            next_deadline = outcome.next_deadline;
        }
    });

    let supervisor_state = state.clone();
    tokio::spawn(async move {
        yas_event!(
            supervisor_state.events,
            EventType::TaskStart,
            events::payload_name("supervisor")
        );
        supervisor_loop(supervisor_state).await;
    });

    // SIGCHLD is what makes exit detection prompt without polling.  The
    // handler does nothing but wake the supervisor: reaping from a signal
    // context would race the session mutex, and the supervisor already knows
    // which pids it owns.
    #[cfg(unix)]
    {
        let sigchld_state = state.clone();
        tokio::spawn(async move {
            let Ok(mut sigchld) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::child())
            else {
                eprintln!("[supervisor] SIGCHLD unavailable; falling back to the poll");
                return;
            };
            loop {
                sigchld.recv().await;
                sigchld_state.supervisor_notify.notify_one();
            }
        });
    }

    // Warm the KV store off the serving paths (docs/design/kv.md
    // § Storage): the load+hash of the whole database happens now, in the
    // background, instead of inline in the first connection's first KV
    // message. YAS_KV=0 disables the family, so nothing to warm.
    if !std::env::var("YAS_KV").is_ok_and(|v| v == "0") {
        kv::warm();
    }

    // Everything this process hosts alongside the server starts here: the
    // delivery and supervisor loops are running, so a session opened now is a
    // session that can be answered.
    if let Some(hosted) = hosted {
        let (ingress, incoming) = mpsc::channel(MAX_PENDING_INGRESS);
        tokio::spawn(run_hosted_ingress(incoming, state.clone()));
        hosted(LocalEndpoint {
            state: state.clone(),
            ingress,
        });
    }

    // Broadcast Shutdown on SIGTERM / SIGINT so clients can reconnect promptly
    // instead of waiting for a transport-level timeout.
    {
        let state = state.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigterm = signal(SignalKind::terminate()).expect("signal handler");
                let mut sigint = signal(SignalKind::interrupt()).expect("signal handler");
                tokio::select! {
                    _ = sigterm.recv() => {}
                    _ = sigint.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            begin_server_shutdown(&state).await;
        });
    }

    #[cfg(unix)]
    if let Some(channel_fd) = state.config.fd_channel {
        let listener = IpcListener::bind(
            &state.config.ipc_path,
            state.config.verbose,
            state.config.ipc_path_is_automatic,
        );
        let yas_accept = tokio::spawn(run_yas_accept_loop(listener, state.clone()));
        yas_sd_notify::notify_ready(state.config.verbose);
        let shutdown = state.shutdown_notify.clone();
        tokio::select! {
            _ = ipc::run_fd_channel(channel_fd, state.clone()) => {}
            _ = shutdown.notified() => {}
        }
        begin_server_shutdown(&state).await;
        let _ = yas_accept.await;
        state.extensions.shutdown().await;
        state.process_server.shutdown().await;
        state.connections.wait_empty().await;
        yas_event!(state.events, EventType::ServerStop);
        state.events.shutdown_file_streams().await;
        return;
    }

    #[cfg(unix)]
    let listener = {
        if let Some(l) = IpcListener::from_systemd_fd(state.config.verbose) {
            l
        } else {
            IpcListener::bind(
                &state.config.ipc_path,
                state.config.verbose,
                state.config.ipc_path_is_automatic,
            )
        }
    };
    #[cfg(not(unix))]
    let listener = IpcListener::bind(&state.config.ipc_path, state.config.verbose).await;

    yas_sd_notify::notify_ready(state.config.verbose);
    run_yas_accept_loop(listener, state.clone()).await;
    begin_server_shutdown(&state).await;
    #[cfg(any(unix, windows))]
    state.process_server.shutdown().await;
    state.extensions.shutdown().await;
    state.connections.wait_empty().await;
    yas_event!(state.events, EventType::ServerStop);
    state.events.shutdown_file_streams().await;
}

/// Who a freshly accepted local peer is, as the kernel reports it.
///
/// Best-effort by design: a platform without peer credentials, or a kernel that
/// refuses them, yields the undescribed origin rather than failing the accept.
/// This names a client; it never admits one.
fn local_origin(stream: &IpcStream) -> ConnectionOrigin {
    #[cfg(unix)]
    {
        yas_webserver::local_ipc::peer_credentials(stream)
            .map_or(ConnectionOrigin::Network, ConnectionOrigin::Local)
    }
    #[cfg(not(unix))]
    {
        let _ = stream;
        ConnectionOrigin::Network
    }
}

/// Streams waiting to be classified or paired, per ingress path.
const MAX_PENDING_INGRESS: usize = 64;
/// How long a stream has to say whether it is a preface or a composite offer.
const INGRESS_CLASSIFY_TIMEOUT: Duration = Duration::from_secs(1);
/// How long half a composite transport waits for the other half.
const INGRESS_PAIR_TIMEOUT: Duration = Duration::from_secs(2);

#[allow(unused_mut)] // Windows' named-pipe listener advances through &mut self.
async fn run_yas_accept_loop(mut listener: IpcListener, state: AppState) {
    let shutdown = state.shutdown_notify.clone();
    let (classified_tx, mut classified_rx) = mpsc::channel(MAX_PENDING_INGRESS);
    let mut classifying = 0usize;
    let mut pairing =
        yas_composite_transport::Pairing::new(MAX_PENDING_INGRESS, INGRESS_PAIR_TIMEOUT);
    let mut expiry = tokio::time::interval(Duration::from_millis(100));
    expiry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = listener.accept() => match result {
                Ok(stream) => {
                    if classifying.saturating_add(pairing.len()) >= MAX_PENDING_INGRESS {
                        continue;
                    }
                    classifying += 1;
                    // Read the peer while the stream is still a socket: after
                    // `classify` it is an opaque byte source, and after pairing
                    // the two halves it is two of them.
                    let origin = local_origin(&stream);
                    let classified_tx = classified_tx.clone();
                    tokio::spawn(async move {
                        let classified = tokio::time::timeout(
                            INGRESS_CLASSIFY_TIMEOUT,
                            yas_composite_transport::classify(stream),
                        )
                        .await
                        .ok()
                        .and_then(Result::ok);
                        let _ = classified_tx.send((origin, classified)).await;
                    });
                }
                Err(error) => {
                    eprintln!("YAS accept error: {error}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            _ = shutdown.notified() => return,
            classified = classified_rx.recv(), if classifying != 0 => {
                classifying -= 1;
                let Some((origin, Some(classified))) = classified else { continue };
                match classified {
                    yas_composite_transport::Ingress::Direct(stream) => {
                        spawn_yas_client(stream, state.clone(), origin);
                    }
                    yas_composite_transport::Ingress::Composite { offer, stream } => {
                        // The pair's identity is the half that completes it;
                        // both halves came from the same peer, and refusing a
                        // mismatch is the pairing token's job, not this one's.
                        if let Ok(Some(pair)) = pairing.insert(offer, stream, Instant::now()) {
                            spawn_yas_composite_client(
                                pair.main,
                                pair.datagram,
                                pair.max_datagram,
                                state.clone(),
                                origin,
                            );
                        }
                    }
                }
            }
            _ = expiry.tick() => {
                drop(pairing.expire(Instant::now()));
            }
        }
    }
}

/// Minimum interval between blanket RequestFrame rounds.  Keeps video
/// players (mpv) and browsers ticking even when no client is consuming
/// frames.  Also used as the maximum tick-loop sleep so the loop never
/// blocks longer than this.
///
/// Unwatched applications receive only 4 Hz even while another surface is
/// active. A watched surface owns an independent fixed-rate clock; raising
/// every other application to 16 Hz spends application and compositor CPU
/// on frames no client consumes.
///
/// This is a floor on liveness, not the frame rate: a subscribed surface is
/// paced by its subscription clock. The blanket round only exists so an app
/// nobody is watching still makes progress.
const BLANKET_FRAME_INTERVAL: Duration = Duration::from_millis(250);

/// Returns the interval at which the tick loop must send blanket
/// `RequestFrame` events to keep Wayland apps (mpv, browsers, etc.)
/// making progress. Returns `None` when no clients are connected — in
/// that state the loop can sleep purely on event notifications, and
/// apps pause until a viewer reconnects (resuming within SURFACE).
fn blanket_frame_interval(sess: &Session) -> Option<Duration> {
    if sess.clients.is_empty() {
        return None;
    }
    Some(BLANKET_FRAME_INTERVAL)
}

async fn tick(state: &AppState) -> TickOutcome {
    let tick_started = Instant::now();
    yas_event!(state.events, EventType::TickStart);
    let lock_started = Instant::now();
    let mut sess = state.session.lock().await;
    yas_event!(state.events, EventType::SessionLock, {
        let mut payload = events::payload_name("tick");
        payload.extend_from_slice(
            &(lock_started.elapsed().as_nanos().min(u64::MAX as u128) as u64).to_le_bytes(),
        );
        payload
    });
    sess.tick_fires += 1;
    let mut next_deadline: Option<Instant> = None;
    let now = Instant::now();

    // Emit pacing metrics every 10s for each client, even when no ACKs
    // are flowing (idle session): the ACK handler also calls this so the
    // first client with traffic still owns the tick-counter reset.
    if sess
        .clients
        .values()
        .any(|client| client.last_log.elapsed() >= Duration::from_secs(10))
    {
        let log_client_ids: SmallVec<[u64; 4]> = sess.clients.keys().copied().collect();
        for cid in log_client_ids {
            maybe_log_pacing_metrics(&mut sess, cid, state.config.verbose);
        }
    }

    // Live client-catalog bandwidth is actual framed bytes written by the
    // connection writer, sampled at a human-scale cadence. Only a crossed
    // sample boundary can move age or bandwidth, so publishing is bound to
    // one — the tick loop has no floor, and rebuilding every watcher's
    // snapshot per tick would bill that at the PTY producer's rate. Topology
    // changes publish from their own handlers instead.
    const CLIENT_CATALOG_BANDWIDTH_INTERVAL: Duration = Duration::from_secs(1);
    if now.duration_since(sess.catalog_sampled_at) >= CLIENT_CATALOG_BANDWIDTH_INTERVAL {
        sess.catalog_sampled_at = now;
        for client in sess.clients.values_mut() {
            // Each client still measures over its own window: one that
            // connected mid-interval has less elapsed than the session epoch.
            let elapsed = now.duration_since(client.outbound_sampled_at);
            if elapsed.is_zero() {
                continue;
            }
            let total = client.outbound_bytes.load(Ordering::Relaxed);
            let bytes = total.saturating_sub(client.outbound_bytes_seen);
            client.outbound_bytes_per_sec = (bytes as f64 / elapsed.as_secs_f64()) as u64;
            client.outbound_bytes_seen = total;
            client.outbound_sampled_at = now;

            // Both directions share the window: they are sampled from the same
            // tick, so the pair the catalog reports covers one interval.
            let elapsed = now.duration_since(client.inbound_sampled_at);
            if elapsed.is_zero() {
                continue;
            }
            let total = client.inbound_bytes.load(Ordering::Relaxed);
            let bytes = total.saturating_sub(client.inbound_bytes_seen);
            client.inbound_bytes_per_sec = (bytes as f64 / elapsed.as_secs_f64()) as u64;
            client.inbound_bytes_seen = total;
            client.inbound_sampled_at = now;
        }
    }

    // Surface IDs whose per-client encoders need to be invalidated.
    let mut invalidate_client_encoders: Vec<u16> = Vec::new();
    // `(surface, client, the session had been built and could not encode)`.
    let mut vulkan_unavailable: Vec<(u16, u64, bool)> = Vec::new();
    // (sid, cid, overwritten generation) for compositor bitstreams replaced
    // in `last_encoded` by a non-keyframe.  Resolved against each sub's
    // delivered generation below: an undelivered overwrite broke the chain.
    let mut encoded_overwrites: Vec<(u16, u64, u64)> = Vec::new();
    // Surface IDs resized by the compositor this tick.  After the
    // compositor borrow is released we wake pacing for every client
    // subscribed to each sid so the first post-resize frame bypasses
    // the per-surface time gate.
    let mut resized_surface_ids: Vec<u16> = Vec::new();
    // Destroyed surfaces also retire any shared-pointer overlay after the
    // compositor borrow below is released.
    let mut destroyed_surface_ids: Vec<u16> = Vec::new();
    // Touch sequences the compositor retired by itself, applied to the
    // server-side ownership after that same borrow ends.
    let mut cancelled_touch_owners: Vec<Option<u64>> = Vec::new();

    let mut surface_commit_count = 0u32;
    #[cfg(target_os = "linux")]
    let mut mpris_results: Vec<(u64, yas_desktop::MprisActionResult)> = Vec::new();
    #[cfg(target_os = "linux")]
    let mut portal_events: Vec<yas_desktop::Event> = Vec::new();
    #[cfg(target_os = "linux")]
    let mut closed_screencast_sessions = Vec::new();
    #[cfg(target_os = "linux")]
    let mut screencast_state_changed = false;
    if let Some(cs) = sess.compositor.as_mut() {
        let mut events = Vec::new();
        while let Ok(event) = cs.handle.event_rx.try_recv() {
            events.push(event);
        }
        for event in events {
            match event {
                CompositorEvent::SurfaceCreated {
                    surface_id,
                    title,
                    app_id,
                    parent_id,
                    width,
                    height,
                } => {
                    yas_event!(state.events, EventType::CompositorEvent, {
                        let mut payload = vec![1];
                        payload.extend_from_slice(&surface_id.to_le_bytes());
                        payload.extend_from_slice(&parent_id.to_le_bytes());
                        payload.extend_from_slice(&width.to_le_bytes());
                        payload.extend_from_slice(&height.to_le_bytes());
                        payload.extend_from_slice(&events::payload_name(&title));
                        payload.extend_from_slice(&events::payload_name(&app_id));
                        payload
                    });
                    cs.surfaces.insert(
                        surface_id,
                        CachedSurfaceInfo {
                            surface_id,
                            parent_id,
                            // Filled by the SurfaceOrigin that follows
                            // immediately when the client is on a per-app
                            // socket; stays None for the shared one.
                            origin: None,
                            width,
                            height,
                            logical_width: 0,
                            logical_height: 0,
                            title,
                            app_id,
                        },
                    );
                    last_pixels_remove_for_sid(&mut cs.last_pixels, surface_id);
                    last_pixels_remove_for_sid(&mut cs.last_opaque_pixels, surface_id);
                    cs.mark_pixel_snapshot_dirty();
                    last_encoded_remove_for_sid(&mut cs.last_encoded, surface_id);
                    cs.frame_clocks_dirty = true;
                    invalidate_client_encoders.push(surface_id);
                }
                CompositorEvent::SurfaceDestroyed { surface_id } => {
                    yas_event!(state.events, EventType::CompositorEvent, {
                        let mut payload = vec![2];
                        payload.extend_from_slice(&surface_id.to_le_bytes());
                        payload
                    });
                    #[cfg(target_os = "linux")]
                    {
                        let retired = retire_screencast_surface(cs, surface_id);
                        closed_screencast_sessions.extend(retired.closed_sessions);
                        screencast_state_changed |= retired.state_changed;
                    }
                    cs.surfaces.remove(&surface_id);
                    cs.surface_text_inputs.remove(&surface_id);
                    cs.surface_cursors.remove(&surface_id);
                    if cs
                        .surface_activation
                        .is_some_and(|(active, _)| active == surface_id)
                    {
                        cs.surface_activation = None;
                    }
                    last_pixels_remove_for_sid(&mut cs.last_pixels, surface_id);
                    last_pixels_remove_for_sid(&mut cs.last_opaque_pixels, surface_id);
                    cs.mark_pixel_snapshot_dirty();
                    last_encoded_remove_for_sid(&mut cs.last_encoded, surface_id);
                    cs.last_configured_size.remove(&surface_id);
                    cs.last_resize_at.remove(&surface_id);
                    cs.pending_resize.remove(&surface_id);
                    cs.resize_inflight.remove(&surface_id);
                    cs.native_sizes.remove(&surface_id);
                    cs.frame_clock_intervals.remove(&surface_id);
                    cs.frame_clocks_dirty = true;
                    cs.handle.set_frame_interval(surface_id, None);
                    invalidate_client_encoders.push(surface_id);
                    destroyed_surface_ids.push(surface_id);
                }
                CompositorEvent::SurfaceCommit {
                    surface_id,
                    width,
                    height,
                    pixels,
                    timestamp_ms,
                    timestamp_sub_us,
                    encoder_skip,
                } => {
                    yas_event!(state.events, EventType::CompositorEvent, {
                        let mut payload = vec![3];
                        payload.extend_from_slice(&surface_id.to_le_bytes());
                        payload.extend_from_slice(&width.to_le_bytes());
                        payload.extend_from_slice(&height.to_le_bytes());
                        payload.extend_from_slice(&timestamp_ms.to_le_bytes());
                        payload.extend_from_slice(&timestamp_sub_us.to_le_bytes());
                        payload.push(encoder_skip as u8);
                        payload
                    });
                    surface_commit_count += 1;
                    #[cfg(target_os = "linux")]
                    let screencast_frame = {
                        let wanted = cs.screencasts.values().any(|session| {
                            session.streams.iter().any(|stream| {
                                stream.surface_id == surface_id
                                    && u32::from(stream.width) == width
                                    && u32::from(stream.height) == height
                            })
                        });
                        wanted
                            .then(|| pixels.to_rgba(width, height))
                            .filter(|rgba| rgba.len() == width as usize * height as usize * 4)
                    };
                    // A commit is for one `(surface, target size)`, not
                    // necessarily the native composite.  In particular,
                    // multiple subscribers make native and per-client
                    // downscale commits alternate.  `CachedSurfaceInfo`
                    // carries the authoritative native physical/logical
                    // pair from `SurfaceResized`; overwriting only its
                    // physical half here made that pair alternate between
                    // valid and mismatched.  The encode target then
                    // oscillated between the client's display-capped size
                    // and native, rebuilding every encoder on every cycle.
                    // Keep all target dimensions solely in `last_pixels`.
                    if matches!(&pixels, yas_compositor::PixelData::Nv12OpaqueFd { .. }) {
                        cache_surface_commit(
                            &mut cs.last_opaque_pixels,
                            &mut cs.pixel_generation,
                            (surface_id, width, height),
                            pixels,
                            timestamp_ms,
                            timestamp_sub_us,
                            encoder_skip,
                        );
                    } else {
                        cache_surface_commit(
                            &mut cs.last_pixels,
                            &mut cs.pixel_generation,
                            (surface_id, width, height),
                            pixels,
                            timestamp_ms,
                            timestamp_sub_us,
                            encoder_skip,
                        );
                    }
                    cs.mark_pixel_snapshot_dirty();
                    #[cfg(target_os = "linux")]
                    if let Some(rgba) = screencast_frame {
                        for stream in cs
                            .screencasts
                            .values()
                            .flat_map(|session| session.streams.iter())
                            .filter(|stream| {
                                stream.surface_id == surface_id
                                    && u32::from(stream.width) == width
                                    && u32::from(stream.height) == height
                            })
                        {
                            let _ = stream.source.push_timed(
                                rgba.clone(),
                                timestamp_ms,
                                timestamp_sub_us,
                            );
                        }
                    }
                }
                CompositorEvent::SurfaceEncoded {
                    frame,
                    timestamp_ms,
                    timestamp_sub_us,
                } => {
                    yas_event!(state.events, EventType::SurfaceEncode, {
                        let mut payload = frame.surface_id.to_le_bytes().to_vec();
                        payload.extend_from_slice(&frame.client_id.to_le_bytes());
                        payload.extend_from_slice(&frame.width.to_le_bytes());
                        payload.extend_from_slice(&frame.height.to_le_bytes());
                        payload.extend_from_slice(&(frame.data.len() as u32).to_le_bytes());
                        payload.push(frame.codec_flag);
                        payload.push(frame.is_keyframe as u8);
                        payload
                    });
                    surface_commit_count += 1;
                    cs.pixel_generation += 1;
                    let new_is_keyframe = frame.is_keyframe;
                    let key = (frame.surface_id, frame.client_id);
                    let prev = cs.last_encoded.insert(
                        key,
                        LastEncoded {
                            width: frame.width,
                            height: frame.height,
                            data: frame.data,
                            is_keyframe: frame.is_keyframe,
                            codec_flag: frame.codec_flag,
                            generation: cs.pixel_generation,
                            timestamp_ms,
                            timestamp_sub_us,
                        },
                    );
                    // Overwriting a frame the subscriber never received
                    // removes a link from its delta chain: every later
                    // delta references reconstructions the decoder now
                    // lacks, and the picture visibly tears until the next
                    // keyframe.  An overwrite *by* a keyframe restarts the
                    // chain and is fine; anything else must force one —
                    // including an overwritten undelivered keyframe, whose
                    // following deltas reference a frame the client never
                    // saw.  (Checked against the sub's delivered generation
                    // after this event loop — the compositor borrow is live
                    // here.)
                    if !new_is_keyframe && let Some(prev) = prev {
                        encoded_overwrites.push((key.0, key.1, prev.generation));
                    }
                }
                CompositorEvent::VulkanEncoderUnavailable {
                    surface_id,
                    client_id,
                    after_encode_failures,
                } => {
                    // The compositor could not give this client the requested
                    // profile (driver refusal, or we are at the session cap).
                    // Drop the tracking entry so the next tick can retry this
                    // Vulkan codec at 4:2:0 or advance to another encoder.
                    vulkan_unavailable.push((surface_id, client_id, after_encode_failures));
                }
                CompositorEvent::SurfaceTitle { surface_id, title } => {
                    if let Some(info) = cs.surfaces.get_mut(&surface_id) {
                        info.title = title.clone();
                    }
                }
                CompositorEvent::SurfaceAppId { surface_id, app_id } => {
                    if let Some(info) = cs.surfaces.get_mut(&surface_id) {
                        info.app_id = app_id.clone();
                    }
                }
                CompositorEvent::SurfaceOrigin {
                    surface_id,
                    sandbox_engine: _,
                    app_id,
                    instance_id,
                } => {
                    // Cached so a client attaching later learns it too: the
                    // compositor sends this once, at creation, and identity
                    // cannot change while the connection lives.
                    if let Some(info) = cs.surfaces.get_mut(&surface_id) {
                        info.origin = Some(SurfaceOrigin {
                            app_id: app_id.clone(),
                            instance_id: instance_id.clone(),
                        });
                    }
                }
                CompositorEvent::SurfaceActivated { surface_id } => {
                    cs.surface_activation_revision =
                        cs.surface_activation_revision.saturating_add(1).max(1);
                    cs.surface_activation = Some((surface_id, cs.surface_activation_revision));
                }
                CompositorEvent::SurfaceMaximizeRequested { .. } => {
                    // Published to native Surface clients once the state
                    // extension is wired through the catalogue. Keep the
                    // compositor event exhaustive in the meantime.
                }
                CompositorEvent::SurfaceTextInput {
                    surface_id,
                    enabled,
                    requested: _,
                    hint,
                    purpose,
                    cursor_rect,
                } => {
                    let cursor_rect = cursor_rect.map(clamp_cursor_rect);
                    if enabled {
                        cs.surface_text_inputs.insert(
                            surface_id,
                            CachedSurfaceTextInput {
                                hint,
                                purpose,
                                cursor_rect,
                            },
                        );
                    } else {
                        cs.surface_text_inputs.remove(&surface_id);
                    }
                }
                CompositorEvent::SurfaceResized {
                    surface_id,
                    width,
                    height,
                    logical_width,
                    logical_height,
                } => {
                    // A resize that only moved the *logical* size — the
                    // surface kept compositing the same pixels, but the
                    // window they represent changed size because the
                    // mediated output scale did — is a presentation
                    // change, not a pipeline one.  Every cached frame and
                    // every live encoder is still valid for it, so it must
                    // not take the teardown below: emptying the pixel
                    // cache for an idle app leaves nothing to refill it
                    // and the surface goes black until something commits
                    // again (which for an idle app may be never).
                    let resolution_changed = cs
                        .native_sizes
                        .insert(surface_id, (width as u32, height as u32))
                        != Some((width as u32, height as u32));
                    if let Some(info) = cs.surfaces.get_mut(&surface_id) {
                        info.width = width;
                        info.height = height;
                        info.logical_width = logical_width;
                        info.logical_height = logical_height;
                    }
                    // The configure this answers (or one the client resized
                    // past on its own) has landed: encoder creation for this
                    // surface is unblocked.  Whether or not the resolution
                    // moved — the surface is no longer on its way anywhere,
                    // and a latch left standing would hold builds off for the
                    // whole grace window.
                    cs.resize_inflight.remove(&surface_id);
                    if resolution_changed {
                        #[cfg(target_os = "linux")]
                        {
                            let retired = retire_screencast_surface(cs, surface_id);
                            closed_screencast_sessions.extend(retired.closed_sessions);
                            screencast_state_changed |= retired.state_changed;
                        }
                        last_pixels_remove_for_sid(&mut cs.last_pixels, surface_id);
                        last_pixels_remove_for_sid(&mut cs.last_opaque_pixels, surface_id);
                        cs.mark_pixel_snapshot_dirty();
                        last_encoded_remove_for_sid(&mut cs.last_encoded, surface_id);
                        // Don't eagerly invalidate client encoders here.  The
                        // encode path already checks for dimension mismatches
                        // (source_dimensions != pixel size) and recreates the
                        // encoder on demand.  Eagerly destroying encoders on
                        // every intermediate size during a drag-resize causes
                        // expensive encoder teardown+creation cycles for sizes
                        // that may never actually be encoded (because a newer
                        // SurfaceCommit arrives before the next encode tick).
                        // Compositor-resident Vulkan sessions are the
                        // exception — nothing recreates those on demand, so
                        // the `resized_surface_ids` pass below tears them
                        // down explicitly.
                        resized_surface_ids.push(surface_id);
                    }
                }
                CompositorEvent::ClipboardContent { .. } => {}
                CompositorEvent::ClipboardOwner { wayland } => {
                    cs.wayland_clipboard_owned = wayland;
                }
                CompositorEvent::SurfaceCursor { surface_id, cursor } => {
                    if let Some(cursor) = native_surface_cursor(&cursor) {
                        cs.surface_cursors.insert(surface_id, cursor);
                    } else {
                        cs.surface_cursors.remove(&surface_id);
                    }
                }
                CompositorEvent::TouchCancelled { owner_id } => {
                    cancelled_touch_owners.push(owner_id);
                }
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(bus) = cs.desktop_bus.as_mut() {
            while let Some(event) = bus.try_recv() {
                match event {
                    yas_desktop::Event::Tray(record) => match &record {
                        yas_desktop::TrayRecord::Upsert(item) => {
                            cs.desktop_state.tray.insert(item.tray_id, item.clone());
                        }
                        yas_desktop::TrayRecord::Delete { tray_id } => {
                            cs.desktop_state.tray.remove(tray_id);
                            cs.desktop_menus.remove(tray_id);
                        }
                    },
                    yas_desktop::Event::TrayMenu(menu) => {
                        cs.desktop_menus.insert(menu.tray_id, menu.clone());
                    }
                    yas_desktop::Event::Notification(record) => match &record {
                        yas_desktop::NotificationRecord::Upsert(item) => {
                            cs.desktop_state
                                .notifications
                                .insert(item.notification_id, item.clone());
                            cs.desktop_removed_notifications
                                .remove(&item.notification_id);
                        }
                        yas_desktop::NotificationRecord::Delete {
                            notification_id,
                            revision,
                            reason,
                        } => {
                            cs.desktop_state.notifications.remove(notification_id);
                            cs.desktop_removed_notifications
                                .insert(*notification_id, (*revision, *reason));
                        }
                    },
                    yas_desktop::Event::Mpris(records) => {
                        let observed_at = Instant::now();
                        for record in &records {
                            match record {
                                yas_desktop::MprisRecord::Upsert(player) => {
                                    cs.mpris_state
                                        .players
                                        .insert(player.player_id, player.clone());
                                    cs.mpris_position_observed_at
                                        .insert(player.player_id, observed_at);
                                }
                                yas_desktop::MprisRecord::Delete { player_id } => {
                                    cs.mpris_state.players.remove(player_id);
                                    cs.mpris_position_observed_at.remove(player_id);
                                }
                            }
                        }
                    }
                    yas_desktop::Event::MprisAction { requester, result } => {
                        mpris_results.push((requester, result));
                    }
                    event @ yas_desktop::Event::Portal { .. }
                    | event @ yas_desktop::Event::PortalCancel(_)
                    | event @ yas_desktop::Event::PortalSessionClosed(_) => {
                        portal_events.push(event)
                    }
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    if screencast_state_changed
        && let Some(bus) = sess
            .compositor
            .as_ref()
            .and_then(|compositor| compositor.desktop_bus.as_ref())
    {
        for session_id in closed_screencast_sessions {
            let _ = bus.try_command(yas_desktop::Command::PortalSessionClosed(session_id));
        }
    }
    #[cfg(target_os = "linux")]
    for (requester, result) in mpris_results {
        if let Some(cs) = sess.compositor.as_mut() {
            let queue = cs.native_mpris_results.entry(requester).or_default();
            if queue.len() >= 32 {
                queue.pop_front();
            }
            queue.push_back(result);
        }
    }
    #[cfg(target_os = "linux")]
    for event in portal_events {
        match event {
            yas_desktop::Event::Portal {
                mut request,
                parent_window,
            } => {
                let parent_surface_id = sess
                    .compositor
                    .as_ref()
                    .and_then(|cs| cs.handle.resolve_foreign_parent(&parent_window));
                match &mut request {
                    yas_desktop::PortalRequest::Access(request) => {
                        request.parent_surface_id = parent_surface_id;
                    }
                    yas_desktop::PortalRequest::ScreenCast(request) => {
                        request.parent_surface_id = parent_surface_id;
                        request.candidates = sess
                            .compositor
                            .as_ref()
                            .map(screencast_candidates)
                            .unwrap_or_default();
                    }
                }
                let request_id = match &request {
                    yas_desktop::PortalRequest::Access(request) => request.request_id,
                    yas_desktop::PortalRequest::ScreenCast(request) => request.request_id,
                };
                if sess.pending_portals.len() >= 32 {
                    if let Some(bus) = sess
                        .compositor
                        .as_ref()
                        .and_then(|cs| cs.desktop_bus.as_ref())
                    {
                        let _ = bus.try_command(yas_desktop::Command::NativePortal(
                            yas_desktop::PortalResponse {
                                request_id,
                                decision: yas_desktop::PortalResponseDecision::Cancelled,
                                surface_ids: Vec::new(),
                                choices: Vec::new(),
                            },
                        ));
                    }
                    continue;
                }
                sess.pending_portals.insert(
                    request_id,
                    PendingPortal {
                        request,
                        native_authority: None,
                    },
                );
            }
            yas_desktop::Event::PortalCancel(request_id) => {
                sess.pending_portals.remove(&request_id);
            }
            yas_desktop::Event::PortalSessionClosed(session_id) => {
                sess.stop_screencast(session_id);
            }
            _ => unreachable!(),
        }
    }
    sess.surface_commits += surface_commit_count;

    for &surface_id in &destroyed_surface_ids {
        sess.surface_handles.remove_backend(surface_id);
        sess.clear_surface_pointer(surface_id);
        sess.native_surface_claims
            .retain(|&(_, claimed_surface), _| claimed_surface != surface_id);
    }

    for owner_id in cancelled_touch_owners {
        sess.forget_touch_sequence(owner_id);
    }

    // Apply deferred per-client encoder invalidation (couldn't mutate
    // sess.clients while sess.compositor was borrowed above).  Any
    // surface event (resize, destroy, reconfigure) invalidates every
    // encoder bound to that sid's pixel stream.
    for sid in invalidate_client_encoders {
        let mut had_vulkan = false;
        for c in sess.clients.values_mut() {
            had_vulkan |= invalidate_client_surface(c, sid, destroyed_surface_ids.contains(&sid));
        }
        // The compositor's sessions are sized against the old composite,
        // so drop every client's encoder for this surface.  Selection will
        // rebuild them at the new size.
        if had_vulkan && let Some(cs) = sess.compositor.as_ref() {
            let _ = cs.handle.command_tx.try_send(
                yas_compositor::CompositorCommand::DestroyVulkanEncoder {
                    surface_id: sid as u32,
                    client_id: None,
                },
            );
            cs.handle.wake();
        }
    }

    // A compositor bitstream overwritten before this client fetched it is a
    // hole in the delta chain — the decoder would silently mispredicts every
    // frame after it (the picture "jumps back and forth" between the stale
    // and fresh reference slots) until a keyframe.  Owe one: the delivery
    // loop then withholds deltas and asks the session for an IDR. The
    // one-frame token makes this rare; this is the backstop that keeps it
    // invisible.
    for (sid, cid, prev_gen) in encoded_overwrites {
        if let Some(c) = sess.clients.get_mut(&cid)
            && c.vulkan_video_surfaces.contains_key(&sid)
            && let Some(sub) = c.surface_subs.get_mut(&sid)
            && sub.last_encoded_gen != Some(prev_gen)
            && sub.has_keyframe
        {
            sub.has_keyframe = false;
        }
    }

    // A client the compositor could not give a session to retries 4:2:0 when
    // the refused profile was 4:4:4, then falls through when that also fails.
    // The exact refusal has to be latched or the next tick re-selects the
    // same profile forever.
    for (sid, cid, after_encode_failures) in vulkan_unavailable {
        let mut declined_name = None;
        if let Some(c) = sess.clients.get_mut(&cid)
            && let Some(vulkan) = c.vulkan_video_surfaces.remove(&sid)
        {
            // Cache only the optional 4:4:4 profile device-wide. A 4:2:0
            // encode failure can be surface-, extent-, or synchronization-
            // specific; the per-subscription refusal below is enough to
            // fall this client back without poisoning every later surface.
            if after_encode_failures && vulkan.is_444 {
                declined_name = Some(vulkan.encoder_name);
            }
            // Latch only the encoder that was actually refused.  The entry we
            // just removed says which one was in flight; anything else in the
            // Vulkan tier is still worth trying on the next tick.
            let refused =
                if vulkan.codec_flag == SurfaceEncoderPreference::VulkanVideoAV1.codec_flag() {
                    SurfaceEncoderPreference::VulkanVideoAV1
                } else {
                    SurfaceEncoderPreference::VulkanVideoH264
                };
            // Keep the rest of the subscription: it carries this client's
            // bandwidth/speed/codec overrides, which a refusal is no
            // reason to reset.  Clearing the encoder is enough to make the
            // next tick retry Vulkan 4:2:0 or build the next encoder.
            let sub = c.surface_subs.entry(sid).or_default();
            latch_vulkan_refusal(sub, refused, vulkan.is_444, vulkan.width, vulkan.height);
            sub.selected_encoder = None;
            retire_encoder(sub.encoder.take());
            sub.has_keyframe = false;
            if sub.encode_in_flight || sub.creation_in_flight {
                sub.encoder_invalidated = true;
            }
            forget_surface_inflight(c, sid);
            if vulkan.is_444 {
                eprintln!(
                    "[vulkan-video] cid={cid} sid={sid}: compositor declined the 4:4:4 \
                     profile, retrying the same Vulkan codec at 4:2:0",
                );
            } else {
                eprintln!(
                    "[vulkan-video] cid={cid} sid={sid}: compositor declined a 4:2:0 \
                     session, falling back to the next encoder",
                );
            }
        }
        if let Some(name) = declined_name
            && let Some(cs) = sess.compositor.as_mut()
            && cs.declined_vulkan_444_encoders.insert(name)
        {
            eprintln!(
                "[vulkan-video] {name}: built a 4:4:4 session this device could not encode \
                 with; not offering that profile again",
            );
        }
        if let Some(cs) = sess.compositor.as_mut() {
            cs.last_encoded.remove(&(sid, cid));
        }
    }

    // Wake pacing for every subscriber of a compositor-resized surface.
    // Reset the burst window and clear next_send_at so the first frame
    // at the new dimensions flows at wire speed instead of waiting for
    // the per-surface time gate (up to ~1/fps), and force a keyframe
    // so decoders recover cleanly after the dimension change.
    for sid in resized_surface_ids {
        let mut had_vulkan = false;
        for c in sess.clients.values_mut() {
            // A compositor-resident session is bound to the size it was
            // built at and nothing recreates it on demand: the delivery
            // path skips on `vulkan_await` while the compositor, whose
            // encode image now sits at a size no composite is produced
            // at, never emits another bitstream — the surface freezes.
            // Drop the tracking so the next tick re-selects at the new
            // native size.  Deliberately not `vulkan_refused`: nothing
            // was declined, and latching would bar the rebuild.
            //
            // For every client, not just the subscribed ones: the teardown
            // below names no client id, so it destroys sessions a client
            // that has since unsubscribed still holds an entry for, and
            // that entry would route it to a session that no longer exists
            // if it resubscribed.
            had_vulkan |= c.vulkan_video_surfaces.remove(&sid).is_some();
            if !c.surface_subscriptions.contains(&sid) {
                continue;
            }
            let s = c.surface_subs.entry(sid).or_default();
            s.burst_remaining = SURFACE_BURST_FRAMES;
            s.next_send_at = None;
            s.nal_none_streak = 0;
            s.nal_none_latched_at = None;
            s.has_keyframe = false;
        }
        if had_vulkan && let Some(cs) = sess.compositor.as_mut() {
            // A bitstream the old session emitted after the resize event
            // was drained outlives the session that made it.  Delivery
            // reads it next tick, finds it stamped with the pre-resize
            // size, and tears down the session selection has just built —
            // a create/destroy cycle that repeats for as long as the entry
            // is there.
            last_encoded_remove_for_sid(&mut cs.last_encoded, sid);
            let _ = cs.handle.command_tx.try_send(
                yas_compositor::CompositorCommand::DestroyVulkanEncoder {
                    surface_id: sid as u32,
                    client_id: None,
                },
            );
            cs.handle.wake();
            eprintln!(
                "[vulkan-video] teardown sid={sid}: surface resized; \
                 sessions rebuild at the new size",
            );
        }
    }

    // Per-client surface encode + deliver.
    // Each client has its own encoder per surface.  We encode from
    // shared last_pixels into each client's encoder and deliver.
    //
    // Share the cached pixel metadata index so each client's per-surface
    // encoder can draw from the latest pixels without holding the compositor
    // borrow through the (lengthy) encoder-dispatch loop below. PTY-only and
    // deadline wakeups reuse the same allocation until a surface commit
    // changes the cache.
    // (sid, width, height, generation, timestamp_ms, timestamp_sub_us) per target
    // entry.  One sid can appear several times — once for each
    // distinct (width, height) the renderer produced (per-encoder
    // target plus the native composite).
    let (pixel_snapshot, opaque_pixel_snapshot): (
        Arc<Vec<PixelSnapshot>>,
        Arc<Vec<PixelSnapshot>>,
    ) = sess
        .compositor
        .as_mut()
        .map(SharedCompositor::pixel_snapshots)
        .unwrap_or_default();
    if pixel_snapshot.is_empty() {
        sess.ticks_pixel_snapshot_empty = sess.ticks_pixel_snapshot_empty.saturating_add(1);
    } else {
        sess.pixel_snapshot_len = pixel_snapshot.len();
    }

    // ---- Surface encode (off main thread) + deliver ----
    //
    // Collect encode jobs, drop the session lock, run encodes in
    // spawn_blocking, re-acquire the lock, and deliver.

    struct EncodeJob {
        cid: u64,
        sid: u16,
        /// The encoder's source dimensions, equal to this client's
        /// physical viewport.  Pixels arrive at this size from the
        /// compositor — either zero-copy via NV12/VA-Surface DMA-BUFs
        /// (VAAPI GBM-backed externals) or a server-allocated BGRA
        /// staging buffer that the compositor GPU-copies into at this
        /// size (NVENC, software encoders).  These dims go on the
        /// wire as the frame `width`/`height` so each viewer sizes
        /// its `<canvas>` to its own bitstream.
        target_w: u32,
        target_h: u32,
        /// Pixel data to encode (already at target size).
        pixels: yas_compositor::PixelData,
        needs_keyframe: bool,
        encoder: SurfaceEncoder,
        generation: u64,
        /// CLOCK_MONOTONIC ms captured at compositor commit time.
        timestamp_ms: u32,
        timestamp_sub_us: u16,
        /// When the async loop handed this frame to the blocking pool.
        queued_at: Instant,
    }
    struct EncoderCreateParams {
        preferences: Vec<SurfaceEncoderPreference>,
        probing_vulkan_predecessors: bool,
        vaapi_device: String,
        encoding: SurfaceEncoding,
        verbose: bool,
        codec_support: u8,
        chroma: ChromaSubsampling,
    }
    /// A creation task runs `SurfaceEncoder::new` + GBM-buffer
    /// allocation on a blocking thread, then hands back the encoder
    /// and its external buffers to the main loop to register with the
    /// compositor.  No encoding happens here — the first encode runs
    /// on a subsequent tick after the compositor has committed into
    /// the new buffers.
    struct CreateJob {
        cid: u64,
        sid: u16,
        /// Encoder source dimensions = this client's physical viewport.
        /// The compositor may render larger; the encode pipeline
        /// downscales per-client into these dimensions.
        target_w: u32,
        target_h: u32,
        /// The compositor native size `(target_w, target_h)` was inscribed
        /// into.  Handed back to the compositor with the target so it can
        /// tell, without re-deriving our arithmetic, whether the composite
        /// has since moved and the target can no longer be filled without
        /// squashing the picture.
        native_w: u32,
        native_h: u32,
        params: EncoderCreateParams,
    }
    struct CreateResult {
        cid: u64,
        sid: u16,
        /// The compositor native size the target was inscribed into, carried
        /// through so the registration below can stamp it on the target.
        /// Only the compositor-backed (Linux) path registers targets.
        #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
        native_w: u32,
        #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
        native_h: u32,
        /// None when `SurfaceEncoder::new` failed; the completion
        /// handler logs and latches a backoff so the tick loop doesn't
        /// spin on retries.
        encoder: Option<SurfaceEncoder>,
        fresh: Option<FreshEncoder>,
        /// Creation failed with at least one eligible backend skipped for
        /// being unable to carry a frame this large.  Asking for less will
        /// reach those backends, so the completion handler degrades the cap
        /// and lets the next tick retry immediately instead of spending the
        /// failure backoff on a request that is merely too big.
        oversized: bool,
        /// Set only when creation stopped at the Vulkan boundary. If every
        /// predecessor failed, the next tick should try Vulkan rather than
        /// back off or let software jump the tier.
        vulkan_predecessors_exhausted: Option<(u32, u32)>,
    }
    /// Metadata shipped with an encode result when the encoder was
    /// created this tick (deferred to spawn_blocking).  `Some` = the
    /// main loop should send a Surface Encoder event and register external
    /// GBM buffers with the compositor, and accept the encoder back.
    struct FreshEncoder {
        name: &'static str,
        codec_string: String,
        #[cfg(target_os = "linux")]
        external_bufs: Vec<yas_compositor::ExternalOutputBuffer>,
    }
    struct EncodeResult {
        cid: u64,
        sid: u16,
        /// Encoded frame dimensions (what goes on the wire).  Equal to
        /// the encoder's source dimensions, i.e. this client's physical
        /// viewport — not the compositor's native size.
        target_w: u32,
        target_h: u32,
        generation: u64,
        encoder: SurfaceEncoder,
        nal_data: Option<(Vec<u8>, bool)>, // (data, is_keyframe)
        codec_flag: u8,
        /// CLOCK_MONOTONIC ms from compositor commit time.
        timestamp_ms: u32,
        timestamp_sub_us: u16,
        queued_at: Instant,
        worker_started_at: Instant,
        worker_finished_at: Instant,
    }

    let mut encode_jobs: Vec<EncodeJob> = Vec::new();
    let mut create_jobs: Vec<CreateJob> = Vec::new();
    // Surfaces that had encode jobs dispatched this tick.  Used below to
    // eagerly pre-request the next frame so the compositor renders in
    // parallel with the in-flight encode (pipeline overlap).

    // Collect (cid, subs) for clients that are due, then build encode jobs
    // in a second pass to avoid overlapping borrows.  `subs` is the set of
    // surface ids this client subscribes to.  Whether a keyframe is owed is
    // read per surface inside that second pass, from the sub's own
    // `has_keyframe` — it is not a property of the client.
    struct ClientWork {
        cid: u64,
        subs: SmallVec<[u16; 4]>,
    }
    let mut client_work: SmallVec<[ClientWork; 4]> = SmallVec::new();

    if !pixel_snapshot.is_empty() {
        for (&cid, client) in sess.clients.iter_mut() {
            if !surface_window_open(client) {
                // Log persistent blockage so hangs are visible.
                let now_inst = Instant::now();
                if now_inst
                    .duration_since(client.last_window_blocked_log)
                    .as_secs_f32()
                    > 5.0
                {
                    client.last_window_blocked_log = now_inst;
                    let max_burst: u8 = client
                        .surface_subs
                        .values()
                        .map(|s| s.burst_remaining)
                        .max()
                        .unwrap_or(0);
                    eprintln!(
                        "[surface-gate] cid={cid} surface_window_open=false surface_credit={}/{}B burst={max_burst}",
                        surface_credit_used_bytes(client),
                        surface_credit_limit_bytes(
                            client,
                            client.avg_surface_frame_bytes.max(1_024.0).ceil() as usize,
                        ),
                    );
                }
            }
            // Per-surface pacing is checked in the inner loop below so
            // that each surface can run at full frame rate independently.
            if client.surface_subscriptions.is_empty() {
                client.skip_no_subs_count = client.skip_no_subs_count.saturating_add(1);
                continue;
            }
            let subs = surface_work_order(client);
            client_work.push(ClientWork { cid, subs });
            // Don't advance the deadline here — wait until we know an
            // encode job was actually collected (see below).  Advancing
            // eagerly wastes time slots when the encode is skipped due
            // to in-flight limits or unchanged pixel data.
        }

        // Track which (client, surface) pairs actually had encode jobs
        // collected so we can advance per-surface deadlines afterwards.
        let mut encoded_client_surfaces: HashSet<(u64, u16)> = HashSet::new();

        // Pre-extract compositor Vulkan Video capabilities so we don't
        // need to borrow sess.compositor inside the client-mutation loop.
        let vk_encode_available = sess
            .compositor
            .as_ref()
            .is_some_and(|cs| cs.handle.vulkan_video_encode);
        let vk_encode_av1_available = sess
            .compositor
            .as_ref()
            .is_some_and(|cs| cs.handle.vulkan_video_encode_av1);
        // Same reason, and at most a handful of names.
        let declined_vulkan_444_encoders = sess
            .compositor
            .as_ref()
            .map(|cs| cs.declined_vulkan_444_encoders.clone())
            .unwrap_or_default();

        // `(surface, client)` pairs whose Vulkan Video encoder should be
        // torn down after the client loop because its per-client coded
        // extent changed.
        // Deferred so we can mutate the client map and the compositor
        // without holding the per-client mutable borrow used inside the
        // loop.  Only the affected client is torn down; ownership is per
        // pair, so a smaller viewport no longer costs everyone else their
        // hardware encoder.
        let mut vulkan_teardown: Vec<(u16, u64)> = Vec::new();

        // Vulkan Video encoder setup commands to send after the client loop.
        struct VulkanEncoderSetup {
            surface_id: u32,
            client_id: u64,
            codec: u8,
            qp: u8,
            width: u32,
            height: u32,
            native_w: u32,
            native_h: u32,
            is_444: bool,
        }
        let mut pending_vulkan_encoder_setups: Vec<VulkanEncoderSetup> = Vec::new();
        let mut pending_vulkan_frame_requests: Vec<(u32, u64)> = Vec::new();
        let mut pending_vulkan_keyframe_requests: Vec<(u32, u64)> = Vec::new();
        // Downscale targets registered by a server-side encoder this tick is
        // replacing with a Vulkan Video session: `(surface, target_w,
        // target_h)`. Re-settle each after the client loop: an orphaned
        // interim target is waste, but another client's NVENC encoder may
        // legitimately still own the same target.
        let mut pending_vulkan_clear_targets: Vec<(u32, u32, u32)> = Vec::new();
        let mut pending_vulkan_qp_updates: Vec<(u32, u64, u8)> = Vec::new();

        for work in &client_work {
            for &sid in &work.subs {
                // Native dims come from the authoritative `native_sizes`
                // map (see `compositor_native_for_sid` for why the
                // historical "largest pixel snapshot" pick is wrong
                // after a resize).
                let Some((native_w, native_h)) = sess.compositor.as_ref().and_then(|cs| {
                    compositor_native_for_sid(&cs.native_sizes, pixel_snapshot.as_slice(), sid)
                }) else {
                    let client = sess.clients.get_mut(&work.cid).unwrap();
                    client.skip_last_pixels_mismatch_count =
                        client.skip_last_pixels_mismatch_count.saturating_add(1);
                    continue;
                };
                // The logical half of that same size, for the per-viewer
                // display cap.  Taken only when the cached physical size
                // still matches what we resolved above: the two travel
                // together in one `SurfaceResized`, and pairing a logical
                // size with a native it was never measured against would
                // scale every viewer's stream by a wrong ratio.
                let native_logical = sess
                    .compositor
                    .as_ref()
                    .and_then(|cs| cs.surfaces.get(&sid))
                    .filter(|info| (info.width as u32, info.height as u32) == (native_w, native_h))
                    .filter(|info| info.logical_width > 0 && info.logical_height > 0)
                    .map(|info| (info.logical_width as u32, info.logical_height as u32));
                // The size this surface is on its way to, if it is on its way
                // anywhere.  An encoder built for the size it is leaving is
                // born stale; see `RESIZE_ENCODER_GRACE`.
                let resize_destination = sess
                    .compositor
                    .as_ref()
                    .and_then(|cs| cs.resize_destination(sid, now));
                // Generation / timestamp for the same-gen skip and the
                // Vulkan-Video fast-path fallback come from the
                // matching native pixel entry when present, else from
                // the largest entry for this surface (best effort).
                // These are only consulted when no exact-target snapshot
                // exists, in which case the dispatch loop skips with
                // `(px_w, px_h) != (target_w, target_h)` anyway, so the
                // values are not safety-critical.
                let (native_gen, native_ts, native_sub_us) = pixel_snapshot
                    .iter()
                    .find(|&&(s, w, h, _, _, _)| s == sid && (w, h) == (native_w, native_h))
                    .or_else(|| {
                        pixel_snapshot
                            .iter()
                            .filter(|&&(s, _, _, _, _, _)| s == sid)
                            .max_by_key(|&&(_, w, h, _, _, _)| (w as u64) * (h as u64))
                    })
                    .map(|&(_, _, _, g, t, sub_us)| (g, t, sub_us))
                    .unwrap_or((0, 0, 0));
                {
                    let client = sess.clients.get_mut(&work.cid).unwrap();
                    client.encode_loop_iters = client.encode_loop_iters.saturating_add(1);
                }
                let encoded_generation = sess
                    .compositor
                    .as_ref()
                    .and_then(|cs| cs.last_encoded.get(&(sid, work.cid)))
                    .map(|e| e.generation);
                let client = sess.clients.get_mut(&work.cid).unwrap();

                // Per-surface pacing gate. At full display rate the source
                // clock already produces at exactly the desired cadence, so
                // do not put a second, tick-loop timer in series with it.
                // The deadline becomes active only after transport pressure
                // lowers this surface below the source rate. Burst-start also
                // bypasses it so initial frames flow at wire speed.
                {
                    let (burst, deadline) = client.surface_subs.get(&sid).map_or((0, now), |s| {
                        (s.burst_remaining, s.next_send_at.unwrap_or(now))
                    });
                    let throttled = surface_delivery_is_throttled(client, sid);
                    if !throttled {
                        client.surface_subs.entry(sid).or_default().next_send_at = None;
                    } else if burst == 0 && deadline > now {
                        // Safety clamp: the deadline should never be more
                        // than 2× the send interval ahead.  If it is, snap
                        // back to now so encoding doesn't stall permanently.
                        let interval = surface_send_interval(client, sid);
                        if deadline > now + interval + interval {
                            client.surface_subs.entry(sid).or_default().next_send_at = Some(now);
                        } else {
                            next_deadline = Some(match next_deadline {
                                Some(existing) => existing.min(deadline),
                                None => deadline,
                            });
                            client.skip_pacing_count = client.skip_pacing_count.saturating_add(1);
                            continue;
                        }
                    }
                }

                // A scaled subscription names its own encode box and ignores
                // the mediated view size.  Scale 120 because the size is
                // already in the pixels the client wants out of the encoder
                // — and for the same reason it opts out of the display cap
                // (passing no logical size below): those pixels are a
                // literal request, not a pane at a DPR to be reinterpreted.
                let scaled = client.surface_subs.get(&sid).and_then(|s| s.scaled_target);
                let adaptive_scale_shift = client
                    .surface_subs
                    .get(&sid)
                    .map_or(0, |s| s.adaptive_scale_shift);
                let view = scaled
                    .map(|(w, h)| (w, h, 120))
                    .or_else(|| client.surface_view_sizes.get(&sid).copied());
                let target = Session::per_client_encode_target(
                    view,
                    native_w,
                    native_h,
                    if scaled.is_some() {
                        None
                    } else {
                        native_logical
                    },
                    surface_encode_cap(&state.config.surface_encoders, client, sid),
                );
                let (target_w, target_h) =
                    adaptive_surface_target(target.0, target.1, adaptive_scale_shift);
                // A target under a hardware encoder's minimum extent would
                // fall through the whole chain to the compositor-resident
                // tier.  Grow it to the floor instead: a sidebar preview is
                // then encoded by the same engine as the pane, and the
                // Vulkan Video sessions stay for the extents that need them.
                let (target_w, target_h) = surface_encoder::grown_to_hardware_floor(
                    &state.config.surface_encoders,
                    surface_codec_support(client, sid),
                    target_w,
                    target_h,
                    native_w,
                    native_h,
                );
                let (enc_w, enc_h) = (target_w, target_h);

                // A Vulkan Video session is per client and per encoded size,
                // just like every server-side encoder. A viewport change
                // replaces only this client's session; other viewers keep
                // their independently sized streams.
                let has_vulkan_enc = match client.vulkan_video_surfaces.get(&sid) {
                    Some(vulkan) if (vulkan.width, vulkan.height) == (enc_w, enc_h) => true,
                    Some(_) => {
                        client.vulkan_video_surfaces.remove(&sid);
                        if !vulkan_teardown.contains(&(sid, work.cid)) {
                            vulkan_teardown.push((sid, work.cid));
                        }
                        if let Some((tw, th)) = client
                            .surface_subs
                            .entry(sid)
                            .or_default()
                            .last_registered_target
                            .take()
                        {
                            client
                                .surface_subs
                                .entry(sid)
                                .or_default()
                                .last_registered_native = None;
                            pending_vulkan_clear_targets.push((sid as u32, tw, th));
                        }
                        false
                    }
                    None => false,
                };

                // The target the compositor holds is stamped with the native
                // it was inscribed into, and it refuses to fill one whose
                // stamp has gone stale — otherwise the composite moves
                // first and the frame comes out squashed into the previous
                // aspect.  When the native moves the target usually moves
                // with it and the rebuild below re-stamps; but the
                // inscription can land on the same numbers as before (a
                // one-pixel native change, say, from another viewer nudging
                // the mediated size), and then nothing would ever refresh
                // the stamp and this client would stop receiving frames.
                // The buffers are still the right ones — only the record of
                // what they were sized against is behind.
                let restamp = client.surface_subs.get(&sid).and_then(|s| {
                    let registered = s.last_registered_target?;
                    (registered == (target_w, target_h)
                        && s.last_registered_native != Some((native_w, native_h)))
                    .then_some(registered)
                });
                if let Some((tw, th)) = restamp {
                    client
                        .surface_subs
                        .entry(sid)
                        .or_default()
                        .last_registered_native = Some((native_w, native_h));
                    if let Some(cs) = sess.compositor.as_ref() {
                        let _ = cs.handle.command_tx.try_send(
                            yas_compositor::CompositorCommand::RestampTarget {
                                surface_id: sid as u32,
                                target_w: tw,
                                target_h: th,
                                native_w,
                                native_h,
                            },
                        );
                        cs.handle.wake();
                    }
                }
                let client = sess.clients.get_mut(&work.cid).unwrap();

                if state.config.verbose {
                    static EDB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                    let n = EDB.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if n < 30 || n.is_multiple_of(500) {
                        eprintln!(
                            "[encode-target #{n}] cid={} sid={sid} view={view:?} native={native_w}x{native_h} target={target_w}x{target_h}",
                            work.cid,
                        );
                    }
                }

                // The compositor produces one snapshot per (sid,
                // target) once the per-client encoder has registered
                // either an external buffer (VAAPI GBM) or a downscale
                // target (NVENC, software).  Find it.  On the very
                // first tick after encoder install the snapshot may not
                // exist yet; we use native as the source for the
                // Vulkan-Video / generation gate below, but the pixels
                // lookup further down requires an exact (sid, w, h)
                // match — feeding mis-sized pixels to a target-sized
                // encoder garbles content (the encoder reads at
                // `source_dimensions` stride into a different-sized
                // buffer, which wraps rows).
                let wants_opaque_pixels = client
                    .surface_subs
                    .get(&sid)
                    .is_some_and(|s| s.wants_nv12_opaque);
                let preferred_snapshot = if wants_opaque_pixels {
                    opaque_pixel_snapshot.as_slice()
                } else {
                    pixel_snapshot.as_slice()
                };
                let target_snapshot = preferred_snapshot
                    .iter()
                    .find(|&&(s, w, h, _, _, _)| s == sid && (w, h) == (target_w, target_h))
                    .or_else(|| {
                        pixel_snapshot
                            .iter()
                            .find(|&&(s, w, h, _, _, _)| s == sid && (w, h) == (target_w, target_h))
                    })
                    .copied();
                let (px_w, px_h, px_gen, px_timestamp_ms, px_timestamp_sub_us) = target_snapshot
                    .map(|(_, w, h, g, t, sub_us)| (w, h, g, t, sub_us))
                    .unwrap_or((native_w, native_h, native_gen, native_ts, native_sub_us));

                // Has anything changed since the frame this client already
                // has?  Answered before the controller runs, because a still
                // surface must not be judged on `frame_bytes` — that EWMA
                // describes motion that has already stopped.
                //
                // A client on a compositor-resident encoder is served from
                // the bitstream stream, not the pixel snapshot, and the two
                // carry independent generations; ask the one it is actually
                // fed from.
                let latest_gen = if has_vulkan_enc {
                    // The session exists but may not have produced anything
                    // yet; in that case there is nothing to hold still.
                    encoded_generation.unwrap_or(u64::MAX)
                } else {
                    px_gen
                };
                let owes_keyframe = owes_keyframe(client, sid);
                let already_encoded = !owes_keyframe
                    && client
                        .surface_subs
                        .get(&sid)
                        .and_then(|s| s.last_encoded_gen)
                        == Some(latest_gen);
                let source_is_still = {
                    let sub = client.surface_subs.entry(sid).or_default();
                    source_generation_is_still(sub, px_gen, now)
                };
                let actually_still = already_encoded && source_is_still;

                // Adaptive bandwidth: one step per surface per tick, after
                // the pacing gate so an idle surface neither steps nor is
                // judged on a stale frame size.  A `true` return means the
                // backend cannot retarget in place and the drift now
                // justifies paying for a rebuild + keyframe.
                let step = step_adaptive_bandwidth(
                    client,
                    state.config.surface_encoding.bandwidth,
                    sid,
                    now,
                    actually_still,
                );
                if step.rebuild || step.target_changed {
                    let sub = client.surface_subs.entry(sid).or_default();
                    retire_encoder(sub.encoder.take());
                    if sub.encode_in_flight || sub.creation_in_flight {
                        sub.encoder_invalidated = true;
                    }
                }
                if step.target_changed {
                    // `target_w` above belongs to the previous scale. Let the
                    // next tick derive the new extent and perform the normal
                    // compositor target / Vulkan-session replacement once.
                    continue;
                }
                // A compositor-resident encoder takes the new rate from the
                // next frame on — no rebuild, no keyframe.  This is only
                // meaningful because sessions are owned per `(surface,
                // client)`: one viewer's backoff no longer degrades
                // everyone else's stream.
                if step.quantizer.is_some() && has_vulkan_enc {
                    // Through `resolve_bandwidth`, not the raw step: the
                    // controller floors at `ADAPTIVE_MAX_QUANTIZER`, so a
                    // ceiling set cheaper than that would otherwise be
                    // overshot into spending more bits than allowed.
                    // Mapped to the session's own QP scale — an H.264
                    // session takes 0–51, and feeding it the controller's
                    // 0–255 walk would pin it at its worst quality.
                    let bw =
                        resolve_bandwidth(client, state.config.surface_encoding.bandwidth, sid);
                    let q = match client.vulkan_video_surfaces.get(&sid) {
                        Some(vulkan) if vulkan.codec_flag == SURFACE_FRAME_CODEC_H264 => {
                            bw.h264_qp()
                        }
                        _ => bw.av1_qp_for_vulkan(),
                    };
                    pending_vulkan_qp_updates.push((sid as u32, work.cid, q));
                }

                // The picture has not changed.  Normally that means there is
                // nothing to send — but the frame the client is looking at
                // was encoded at whatever quantizer the controller had
                // backed off to, and it is about to stay on screen.  If the
                // step above bought an improvement, spend it; otherwise
                // there is nothing to gain.
                let still_refresh = actually_still && step.quantizer.is_some();
                if already_encoded {
                    if !still_refresh {
                        client.skip_same_gen_count = client.skip_same_gen_count.saturating_add(1);
                        continue;
                    }
                    if has_vulkan_enc {
                        // Nothing to re-send here: the bitstream in hand is
                        // the one the client already has.  The qp update
                        // above is staged, and the keyframe request forces
                        // the recomposite that makes the compositor encode
                        // at it.  Delivery happens next tick, on the new
                        // generation.
                        pending_vulkan_keyframe_requests.push((sid as u32, work.cid));
                        continue;
                    }
                }

                // Fast path: this client owns a compositor-resident
                // encoder for this surface, so its bitstream is waiting in
                // `last_encoded` under its own client id.  Nothing here is
                // shared with any other subscriber — a second viewer has
                // its own session, its own GOP and its own quantizer.
                if client.vulkan_video_surfaces.contains_key(&sid) {
                    let encoded = sess
                        .compositor
                        .as_ref()
                        .and_then(|cs| cs.last_encoded.get(&(sid, work.cid)))
                        .map(|e| {
                            (
                                e.width,
                                e.height,
                                e.data.clone(),
                                e.is_keyframe,
                                e.codec_flag,
                                e.generation,
                                e.timestamp_ms,
                                e.timestamp_sub_us,
                            )
                        });
                    let client = sess.clients.get_mut(&work.cid).unwrap();
                    if let Some((
                        ew,
                        eh,
                        data,
                        is_keyframe,
                        codec_flag,
                        frame_gen,
                        ts,
                        timestamp_sub_us,
                    )) = encoded
                    {
                        // `last_encoded` holds only the newest frame per
                        // (surface, client), so the session's opening IDR
                        // survives there for one frame period — 16.6ms at
                        // 60fps.  A tick that arrives after it has been
                        // overwritten used to forward the P frame sitting
                        // there to a subscriber that had never received a
                        // keyframe, and never asked for another: the client
                        // then had no SPS/PPS and no recovery point, so the
                        // whole stream was undecodable until something else
                        // happened to force an IDR.  Ask for one and wait.
                        if owes_keyframe && !is_keyframe {
                            pending_vulkan_keyframe_requests.push((sid as u32, work.cid));
                            client.skip_vulkan_await_count =
                                client.skip_vulkan_await_count.saturating_add(1);
                            continue;
                        }
                        if !owes_keyframe
                            && client
                                .surface_subs
                                .get(&sid)
                                .and_then(|s| s.last_encoded_gen)
                                == Some(frame_gen)
                        {
                            client.skip_same_gen_count =
                                client.skip_same_gen_count.saturating_add(1);
                            continue;
                        }
                        if (target_w, target_h) != (ew, eh) {
                            // A size replacement can leave the old session's
                            // final frame in the event cache until the new
                            // opening keyframe arrives. Never stamp those old
                            // dimensions onto the new decoder stream.
                            client.skip_vulkan_await_count =
                                client.skip_vulkan_await_count.saturating_add(1);
                            continue;
                        }
                        let flags = codec_flag
                            | if is_keyframe {
                                SURFACE_FRAME_FLAG_KEYFRAME
                            } else {
                                0
                            };
                        let estimated_bytes = data.len().saturating_add(64);
                        if !surface_frame_credit_open_for(client, sid, estimated_bytes) {
                            client.skip_pacing_count = client.skip_pacing_count.saturating_add(1);
                            continue;
                        }
                        match enqueue_surface_frame(
                            client,
                            sid,
                            ts,
                            timestamp_sub_us,
                            flags,
                            is_keyframe,
                            data.to_vec(),
                        ) {
                            Err(()) => {
                                client.surface_subs.entry(sid).or_default().has_keyframe = false;
                            }
                            Ok(bytes) => {
                                record_surface_frame_sent(client, sid, bytes, is_keyframe, now);
                                if !is_keyframe {
                                    client.avg_surface_frame_bytes = ewma_with_direction(
                                        client.avg_surface_frame_bytes,
                                        bytes as f32,
                                        0.5,
                                        0.125,
                                    );
                                }
                                client.frames_sent = client.frames_sent.wrapping_add(1);
                                let s = client.surface_subs.entry(sid).or_default();
                                if is_keyframe {
                                    s.has_keyframe = true;
                                }
                                s.burst_remaining = s.burst_remaining.saturating_sub(1);
                                // Match the server-side one-in-flight encoder
                                // discipline. Vulkan may produce one successor
                                // only after this frame entered the client's
                                // delivery path; if the outbox blocks now, that
                                // successor is the sole frame allowed to wait.
                                pending_vulkan_frame_requests.push((sid as u32, work.cid));
                            }
                        }
                        encoded_client_surfaces.insert((work.cid, sid));
                        client.surface_subs.entry(sid).or_default().last_encoded_gen =
                            Some(frame_gen);
                        continue;
                    }
                    // The session exists but has not produced a frame yet.
                    if owes_keyframe {
                        pending_vulkan_keyframe_requests.push((sid as u32, work.cid));
                    }
                    client.skip_vulkan_await_count =
                        client.skip_vulkan_await_count.saturating_add(1);
                    let now_inst = Instant::now();
                    if now_inst.duration_since(client.last_skip_log).as_secs_f32() > 5.0 {
                        client.last_skip_log = now_inst;
                        eprintln!(
                            "[encode-skip] cid={} sid={sid} reason=vulkan_await \
                             (compositor has not produced a bitstream yet) count={}",
                            work.cid, client.skip_vulkan_await_count,
                        );
                    }
                    continue;
                }

                let cached: Option<(yas_compositor::PixelData, bool)> = {
                    let cs = sess.compositor.as_ref().unwrap();
                    let key = (sid, px_w, px_h);
                    let pixels = if wants_opaque_pixels {
                        cs.last_opaque_pixels
                            .get(&key)
                            .or_else(|| cs.last_pixels.get(&key))
                    } else {
                        cs.last_pixels.get(&key)
                    };
                    pixels
                        // A GPU-only commit carries no CPU pixels: the
                        // compositor skipped the readback because a Vulkan
                        // Video encoder owned the surface and nothing had
                        // registered a target needing them.  This client
                        // wants a server-side encoder, so treat it as a miss
                        // — the recomposite below asks for the native BGRA
                        // publish that fills the entry for real.
                        .filter(|lp| !matches!(lp.pixels, yas_compositor::PixelData::GpuOnly))
                        .map(|lp| (lp.pixels.clone(), lp.encoder_skip))
                };
                // A cache-only entry (on-demand BGRA readback over a live
                // NV12 zero-copy stream) must not be encoded — the stream
                // already carries this frame.  Don't advance
                // last_encoded_gen: the next zero-copy publish supersedes
                // this entry within a frame period.
                //
                // Except for a subscriber that has no picture at all — one
                // that just joined, or one whose keyframe request is still
                // outstanding.  There is no next publish for it to wait for:
                // an idle app repaints on nothing, so a capture or thumbnail
                // readback latches the entry until something unrelated makes
                // the app paint.  Treat it as absent instead, so the
                // recomposite request below asks the compositor for the
                // zero-copy frame this subscriber is missing.
                let mut cached = match cached {
                    Some((_, true)) if !owes_keyframe => continue,
                    Some((_, true)) => None,
                    other => other.map(|(p, _)| p),
                };
                // CPU-origin pixels for a registered NVENC target are never
                // an encode input. They mean its independent OPAQUE_FD
                // representation has not arrived. Give an in-flight
                // registration one short grace period, then re-register the
                // shared target and let that command's recomposite refill it.
                let cached_is_cpu_pixels = cached.as_ref().is_some_and(|p| {
                    matches!(
                        p,
                        yas_compositor::PixelData::Bgra(_) | yas_compositor::PixelData::Rgba(_)
                    )
                });
                let registered_nvenc_target = cached_is_cpu_pixels
                    && sess
                        .clients
                        .get(&work.cid)
                        .and_then(|c| c.surface_subs.get(&sid))
                        .is_some_and(|s| {
                            s.wants_nv12_opaque && s.last_registered_target == Some((enc_w, enc_h))
                        });
                let mut repair_opaque_target = false;
                {
                    let now_inst = Instant::now();
                    let sub = sess
                        .clients
                        .get_mut(&work.cid)
                        .and_then(|c| c.surface_subs.get_mut(&sid));
                    if let Some(sub) = sub {
                        if !cached_is_cpu_pixels {
                            // The GPU representation arrived. A later loss
                            // gets a fresh grace period and repair attempt.
                            sub.opaque_wait_since = None;
                        } else if registered_nvenc_target {
                            let since = *sub.opaque_wait_since.get_or_insert(now_inst);
                            if now_inst.duration_since(since) < OPAQUE_PUBLISH_GRACE {
                                continue;
                            }
                            // Rate-limit repairs to the same interval. A
                            // successful registration publishes OPAQUE_FD
                            // and clears this above; a failed export never
                            // falls through to host conversion.
                            sub.opaque_wait_since = Some(now_inst);
                            repair_opaque_target = true;
                        }
                    } else if registered_nvenc_target {
                        continue;
                    }
                }
                if repair_opaque_target {
                    sess.resettle_downscale_target(sid, enc_w, enc_h);
                    continue;
                }
                let client = sess.clients.get_mut(&work.cid).unwrap();

                // Skip if an encode or creation job is already in
                // flight for this surface.  Creations also block encode
                // dispatch: the encoder is None while creation runs,
                // and we don't want to re-queue another creation until
                // the first one completes.
                if client
                    .surface_subs
                    .get(&sid)
                    .is_some_and(|s| s.encode_in_flight || s.creation_in_flight)
                {
                    let full_rate = !surface_delivery_is_throttled(client, sid);
                    let sub = client.surface_subs.entry(sid).or_default();
                    let pending_generation = sub.pending_encode.as_ref().map(|p| p.generation);
                    if full_rate
                        && sub.encode_in_flight
                        && !sub.creation_in_flight
                        && !sub.encoder_invalidated
                        && cached.is_some()
                        && (px_w, px_h) == (target_w, target_h)
                        && (still_refresh
                            || pending_generation_is_newer(
                                sub.in_flight_generation,
                                pending_generation,
                                px_gen,
                            ))
                    {
                        // Preserve only the newest frame. NVENC must remain
                        // one ordered session (alternating parallel sessions
                        // would split the delta chain), but it need not sit
                        // idle while completion travels through the session
                        // loop and the next delivery tick.
                        sub.pending_encode = Some(PendingSurfaceEncode {
                            target_w: enc_w,
                            target_h: enc_h,
                            pixels: cached.take().unwrap(),
                            needs_keyframe: owes_keyframe || still_refresh,
                            force_quality_refresh: still_refresh,
                            generation: px_gen,
                            timestamp_ms: px_timestamp_ms,
                            timestamp_sub_us: px_timestamp_sub_us,
                        });
                    } else if !full_rate || sub.encoder_invalidated {
                        sub.pending_encode = None;
                    }
                    client.skip_in_flight_count = client.skip_in_flight_count.saturating_add(1);
                    let now_inst = Instant::now();
                    if now_inst.duration_since(client.last_skip_log).as_secs_f32() > 5.0 {
                        client.last_skip_log = now_inst;
                        let burst = client
                            .surface_subs
                            .get(&sid)
                            .map_or(0, |s| s.burst_remaining);
                        eprintln!(
                            "[encode-skip] cid={} sid={sid} reason=in_flight same_gen={} in_flight={} burst={burst}",
                            work.cid, client.skip_same_gen_count, client.skip_in_flight_count,
                        );
                    }
                    continue;
                }

                let needs_new_encoder = if has_vulkan_enc {
                    false
                } else {
                    client
                        .surface_subs
                        .get(&sid)
                        .and_then(|s| s.encoder.as_ref())
                        .is_none_or(|e| e.source_dimensions() != (enc_w, enc_h))
                };

                // If the encoder was dropped due to persistent nal_data=None,
                // back off for a short window before retrying.  Each retry
                // allocates GBM fds, so we don't want a genuinely broken
                // encoder (GPU lost) to recreate at tick rate and exhaust
                // the process fd limit — but a warm-up burst (compositor
                // hasn't imported the freshly-allocated external output
                // buffers yet) should recover within seconds without
                // requiring a user-driven resize/resubscribe.
                const NAL_NONE_RETRY_BACKOFF: Duration = Duration::from_secs(2);
                if needs_new_encoder
                    && client
                        .surface_subs
                        .get(&sid)
                        .is_some_and(|s| s.nal_none_streak >= 10)
                {
                    let ready_to_retry = client
                        .surface_subs
                        .get(&sid)
                        .and_then(|s| s.nal_none_latched_at)
                        .is_some_and(|t| now.duration_since(t) >= NAL_NONE_RETRY_BACKOFF);
                    if ready_to_retry {
                        if let Some(s) = client.surface_subs.get_mut(&sid) {
                            s.nal_none_streak = 0;
                            s.nal_none_latched_at = None;
                        }
                    } else {
                        continue;
                    }
                }

                // Hold off on a build the configure in flight is about to
                // invalidate.  Only when it would actually land somewhere
                // else: a configure that leaves this client's target where
                // it is (another viewer nudging the mediated size, a
                // one-pixel move) is no reason to withhold a frame.
                // Only for a build that is actually pending: this is two
                // target derivations, and the tick loop runs it per client
                // per surface.
                let destination_target =
                    resize_destination
                        .filter(|_| needs_new_encoder)
                        .map(|(cw, ch, cs120)| {
                            let target = Session::per_client_encode_target(
                                view,
                                cw as u32,
                                ch as u32,
                                // The destination carries the scale it will be
                                // configured at, so its logical size is exact
                                // — no need to wait for the compositor to
                                // report it.
                                if scaled.is_some() {
                                    None
                                } else {
                                    let s = (cs120 as u32).max(120);
                                    Some((
                                        (cw as u32 * 120).div_ceil(s),
                                        (ch as u32 * 120).div_ceil(s),
                                    ))
                                },
                                surface_encode_cap(&state.config.surface_encoders, client, sid),
                            );
                            let (w, h) =
                                adaptive_surface_target(target.0, target.1, adaptive_scale_shift);
                            // Grown against the native the configure is
                            // heading for, exactly as the live target above
                            // was grown against the current one.  Comparing a
                            // grown target with an ungrown projection would
                            // read every thumbnail as "the configure will move
                            // this" and withhold its frames.
                            surface_encoder::grown_to_hardware_floor(
                                &state.config.surface_encoders,
                                surface_codec_support(client, sid),
                                w,
                                h,
                                cw as u32,
                                ch as u32,
                            )
                        });
                if let Some(destination) = destination_target
                    && destination != (target_w, target_h)
                {
                    client.skip_last_pixels_mismatch_count =
                        client.skip_last_pixels_mismatch_count.saturating_add(1);
                    continue;
                }

                // --- Try Vulkan Video at its configured rank ---
                if needs_new_encoder {
                    let codec_support = surface_codec_support(client, sid);
                    let encoding = SurfaceEncoding {
                        bandwidth: resolve_bandwidth(
                            client,
                            state.config.surface_encoding.bandwidth,
                            sid,
                        ),
                        speed: client
                            .surface_subs
                            .get(&sid)
                            .and_then(|s| s.speed_override)
                            .unwrap_or(state.config.surface_encoding.speed),
                    };

                    // The tier ranks below any dedicated encode engines
                    // (see `SurfaceEncoderPreference::defaults`), which this
                    // block is what enforces: selection here runs ahead of
                    // the fallback chain, so without the check a listed
                    // Vulkan encoder would win no matter where it sits.
                    let predecessors_exhausted = client.surface_subs.get(&sid).is_some_and(|sub| {
                        sub.vulkan_predecessors_exhausted_extent == Some((enc_w, enc_h))
                    });
                    let vulkan_eligible = predecessors_exhausted
                        || vulkan_video_tier_eligible(
                            &state.config.surface_encoders,
                            codec_support,
                            enc_w,
                            enc_h,
                        );
                    let (refused_bits, refused_444_bits) = vulkan_refusals_for_extent(
                        client.surface_subs.entry(sid).or_default(),
                        enc_w,
                        enc_h,
                    );

                    let mut vulkan_selected = false;
                    for &pref in &state.config.surface_encoders {
                        if !pref.is_vulkan_video() {
                            continue;
                        }
                        if !vulkan_eligible {
                            continue;
                        }
                        // Refusals are per encoder: one the compositor has
                        // already turned down is skipped, but the rest of the
                        // tier still gets its turn.
                        if refused_bits & pref.vulkan_refusal_bit() != 0 {
                            continue;
                        }
                        if !pref.supported_by_client(codec_support) {
                            continue;
                        }
                        if !pref.fits(enc_w, enc_h) {
                            continue;
                        }
                        // Check compositor capability (pre-extracted above).
                        let available = match pref {
                            SurfaceEncoderPreference::VulkanVideoH264 => vk_encode_available,
                            SurfaceEncoderPreference::VulkanVideoAV1 => vk_encode_av1_available,
                            _ => false,
                        };
                        if !available {
                            continue;
                        }
                        // Would this client actually be served 4:4:4?  Both
                        // the server's configuration and the client's own
                        // announcement have to say so.
                        let want_444 = state.config.chroma.is_444()
                            && pref.supports_444_by_client(codec_support);

                        // Each codec carries 4:4:4 as its own profile — High
                        // 4:4:4 Predictive for H.264, High for AV1 — and the
                        // compositor asks the driver for that profile
                        // directly.  Whether it gets one is a per-device
                        // answer nothing here can predict (the 4090 does
                        // H.264 4:4:4, the Raphael iGPU does not, and AV1
                        // High is rarer than either), so the request goes out
                        // as asked: a caps query that refuses declines the
                        // session. Selection remembers that exact profile and
                        // retries the same codec at 4:2:0 on the next tick.
                        // A refusal is profile-specific. If 4:4:4 failed,
                        // retry this codec at 4:2:0 before walking on to the
                        // next backend. A 4:4:4 profile that built and then
                        // failed to encode is remembered device-wide; all
                        // 4:2:0 failures and setup refusals stay scoped to
                        // this subscription.
                        let is_444 = vulkan_encoder_chroma(
                            pref,
                            want_444,
                            refused_444_bits,
                            &declined_vulkan_444_encoders,
                        );
                        let qp = match pref {
                            SurfaceEncoderPreference::VulkanVideoAV1 => {
                                encoding.bandwidth.av1_qp_for_vulkan()
                            }
                            _ => encoding.bandwidth.h264_qp(),
                        };
                        // The name and codec string are what the client
                        // configures its decoder from, so they have to state
                        // the chroma actually being encoded — promising High
                        // 4:4:4 Predictive for a High 4:2:0 stream (or the
                        // reverse) misconfigures it.
                        let enc_name = vulkan_encoder_name(pref, is_444);
                        // Queue commands to send after the client loop. The
                        // server's ordinary per-client surface gate owns
                        // cadence; the compositor receives one encode token
                        // only after the prior bitstream enters that client's
                        // delivery path.
                        pending_vulkan_encoder_setups.push(VulkanEncoderSetup {
                            surface_id: sid as u32,
                            client_id: work.cid,
                            codec: pref.vulkan_codec(),
                            qp,
                            width: enc_w,
                            height: enc_h,
                            native_w,
                            native_h,
                            is_444,
                        });
                        pending_vulkan_keyframe_requests.push((sid as u32, work.cid));
                        if let Some(s) = client.surface_subs.get_mut(&sid) {
                            retire_encoder(s.encoder.take());
                            // A server-side creation still in flight would
                            // land after this selection and clobber the
                            // compositor's ownership — mark it stale, the
                            // same way the decline path does.
                            if s.encode_in_flight || s.creation_in_flight {
                                s.encoder_invalidated = true;
                            }
                            // Reconcile any target its interim server-side
                            // encoder registered. Vulkan keeps the same BGRA
                            // scratch at a non-native size but no longer needs
                            // that target's CPU/OPAQUE publication.
                            if let Some((tw, th)) = s.last_registered_target.take() {
                                pending_vulkan_clear_targets.push((sid as u32, tw, th));
                            }
                            s.last_registered_native = None;
                            s.selected_encoder = Some(pref);
                        }
                        client.vulkan_video_surfaces.insert(
                            sid,
                            VulkanVideoSurfaceState {
                                encoder_name: enc_name,
                                codec_flag: pref.codec_flag(),
                                width: enc_w,
                                height: enc_h,
                                is_444,
                            },
                        );
                        if (enc_w, enc_h) != (native_w, native_h) {
                            let sub = client.surface_subs.entry(sid).or_default();
                            sub.last_registered_target = Some((enc_w, enc_h));
                            sub.last_registered_native = Some((native_w, native_h));
                            sub.wants_nv12_opaque = false;
                            sub.wants_opaque_444 = false;
                        }
                        if state.config.verbose {
                            eprintln!(
                                "[surface-encoder] cid={} sid={sid} {enc_w}x{enc_h}: using {enc_name}",
                                work.cid,
                            );
                        }
                        vulkan_selected = true;
                        break;
                    }

                    // The compositor owns this subscription's encoder now.
                    // Falling through would queue a server-side one for the
                    // same (client, surface): a second encoder that never
                    // encodes a frame, because the delivery path takes the
                    // Vulkan bitstream and skips on `skip_vulkan_await`
                    // until it arrives.  It is not a fallback either — a
                    // refusal comes back asynchronously as
                    // `VulkanEncoderUnavailable`, which latches
                    // `vulkan_refused` so a later tick retries the tier
                    // below with this encoder skipped. An interim NVENC
                    // encoder for this same subscription must not survive
                    // the takeover; another client's independently tracked
                    // encoder is allowed to coexist.
                    if vulkan_selected {
                        continue;
                    }

                    // If Vulkan is waiting behind a server-side backend, only
                    // probe the entries ranked above it. Walking past Vulkan
                    // to software here would make software win even when the
                    // compositor can provide the higher-ranked Vulkan tier.
                    // Once the preceding probes are known unavailable, the
                    // next tick admits Vulkan; if Vulkan itself is unavailable,
                    // a later creation uses the full list and reaches software.
                    let (creation_preferences, probing_vulkan_predecessors) =
                        server_creation_preferences(
                            &state.config.surface_encoders,
                            vulkan_eligible,
                        );

                    // Defer encoder creation to spawn_blocking so the
                    // tick loop isn't blocked by slow VA-API init.
                    // The creation task allocates GBM buffers and
                    // returns the encoder; the first encode runs on a
                    // subsequent tick, after the main loop forwards
                    // the buffers to the compositor and the compositor
                    // commits a new frame through them.
                    {
                        let state = client.surface_subs.entry(sid).or_default();
                        retire_encoder(state.encoder.take());
                        state.creation_in_flight = true;
                    }
                    create_jobs.push(CreateJob {
                        cid: work.cid,
                        sid,
                        target_w: enc_w,
                        target_h: enc_h,
                        native_w,
                        native_h,
                        params: EncoderCreateParams {
                            preferences: creation_preferences,
                            probing_vulkan_predecessors,
                            vaapi_device: state.config.vaapi_device.clone(),
                            encoding,
                            verbose: state.config.verbose,
                            codec_support,
                            chroma: state.config.chroma,
                        },
                    });
                    continue;
                }

                let Some(pixels) = cached else {
                    // Encoder creation above is deliberately independent of
                    // this cache lookup.  Installing its target is what makes
                    // the compositor publish pixels at a newly-restored
                    // native size; gating creation on those pixels deadlocks
                    // after a thumbnail target was the last one registered.
                    // Once an encoder already exists, a recomposite of the
                    // current committed state is enough to fill its target.
                    let now_inst = Instant::now();
                    client.skip_last_pixels_mismatch_count =
                        client.skip_last_pixels_mismatch_count.saturating_add(1);
                    let recomposite_due = client.surface_subs.get_mut(&sid).is_some_and(|sub| {
                        let due = sub
                            .recomposite_requested_at
                            .is_none_or(|at| now_inst.duration_since(at).as_millis() >= 250);
                        if due {
                            sub.recomposite_requested_at = Some(now_inst);
                        }
                        due
                    });
                    if recomposite_due && let Some(cs) = sess.compositor.as_ref() {
                        let _ = cs.handle.command_tx.try_send(
                            yas_compositor::CompositorCommand::Recomposite { surface_id: sid },
                        );
                        cs.handle.wake();
                    }
                    continue;
                };

                // The per-client encoder reads pixels at its
                // `source_dimensions` stride.  If the only available
                // snapshot is at native dims (e.g. the compositor
                // hasn't copied into the freshly registered downscale
                // target yet), feeding it would read at the wrong
                // stride and garble content (rows wrap horizontally,
                // looking like the encoded frame is letterboxed AND
                // stretched).  Skip — the next tick after the
                // compositor commits a target-sized frame will
                // pick it up.
                if (px_w, px_h) != (target_w, target_h) {
                    client.skip_last_pixels_mismatch_count =
                        client.skip_last_pixels_mismatch_count.saturating_add(1);
                    continue;
                }

                // A refresh has to be an IDR: a P-frame against an identical
                // reference codes as skip blocks and refines nothing, however
                // much finer the quantizer is.
                let needs_kf = owes_keyframe || needs_new_encoder || still_refresh;
                let reserved_bytes = estimated_surface_frame_bytes(client, sid, needs_kf);
                if !surface_frame_credit_open_for(client, sid, reserved_bytes) {
                    client.skip_pacing_count = client.skip_pacing_count.saturating_add(1);
                    continue;
                }
                let encoder = client
                    .surface_subs
                    .get_mut(&sid)
                    .and_then(|s| s.encoder.take())
                    .unwrap();
                let sub = client.surface_subs.entry(sid).or_default();
                sub.encode_in_flight = true;
                sub.reserved_encode_bytes = reserved_bytes;
                sub.in_flight_generation = Some(px_gen);
                sub.pending_encode = None;
                encoded_client_surfaces.insert((work.cid, sid));
                encode_jobs.push(EncodeJob {
                    cid: work.cid,
                    sid,
                    target_w: enc_w,
                    target_h: enc_h,
                    pixels,
                    needs_keyframe: needs_kf,
                    encoder,
                    generation: px_gen,
                    timestamp_ms: px_timestamp_ms,
                    timestamp_sub_us: px_timestamp_sub_us,
                    queued_at: Instant::now(),
                });
            }
        }

        // Tear down only the superseded per-client Vulkan sessions. A new
        // target may already be queued below; command ordering destroys the
        // old coded extent before installing its replacement.
        for &(sid, cid) in &vulkan_teardown {
            if let Some(c) = sess.clients.get_mut(&cid)
                && c.vulkan_video_surfaces.remove(&sid).is_some()
            {
                c.surface_subs.entry(sid).or_default().has_keyframe = false;
            }
            if let Some(cs) = sess.compositor.as_mut() {
                cs.last_encoded.remove(&(sid, cid));
                let _ = cs.handle.command_tx.try_send(
                    yas_compositor::CompositorCommand::DestroyVulkanEncoder {
                        surface_id: sid as u32,
                        client_id: Some(cid),
                    },
                );
                cs.handle.wake();
                eprintln!(
                    "[vulkan-video] teardown sid={sid} cid={cid}: replacing per-client target",
                );
            }
        }

        // Reconcile targets this tick's Vulkan takeovers left. The taking
        // client cleared `last_registered_target` above, so survivors alone
        // decide whether the target stays and which representations it has.
        for (surface_id, tw, th) in pending_vulkan_clear_targets {
            sess.resettle_downscale_target(surface_id as u16, tw, th);
        }

        // Send Vulkan Video encoder commands to compositor.
        if (!pending_vulkan_encoder_setups.is_empty()
            || !pending_vulkan_frame_requests.is_empty()
            || !pending_vulkan_keyframe_requests.is_empty()
            || !pending_vulkan_qp_updates.is_empty())
            && let Some(cs) = sess.compositor.as_ref()
        {
            for setup in pending_vulkan_encoder_setups {
                eprintln!(
                    "[vulkan-video] sending SetVulkanEncoder sid={} cid={} codec={} {}x{} qp={}",
                    setup.surface_id,
                    setup.client_id,
                    setup.codec,
                    setup.width,
                    setup.height,
                    setup.qp,
                );
                let _ = cs.handle.command_tx.try_send(
                    yas_compositor::CompositorCommand::SetVulkanEncoder {
                        surface_id: setup.surface_id,
                        client_id: setup.client_id,
                        codec: setup.codec,
                        qp: setup.qp,
                        width: setup.width,
                        height: setup.height,
                        native_w: setup.native_w,
                        native_h: setup.native_h,
                        is_444: setup.is_444,
                    },
                );
            }
            for (surface_id, client_id, qp) in pending_vulkan_qp_updates {
                let _ = cs.handle.command_tx.try_send(
                    yas_compositor::CompositorCommand::SetVulkanEncoderQp {
                        surface_id,
                        client_id,
                        qp,
                    },
                );
            }
            for (surface_id, client_id) in pending_vulkan_frame_requests {
                let _ = cs.handle.command_tx.try_send(
                    yas_compositor::CompositorCommand::RequestVulkanFrame {
                        surface_id,
                        client_id,
                    },
                );
            }
            for (surface_id, client_id) in pending_vulkan_keyframe_requests {
                let _ = cs.handle.command_tx.try_send(
                    yas_compositor::CompositorCommand::RequestVulkanKeyframe {
                        surface_id,
                        client_id,
                    },
                );
            }
            cs.handle.wake();
        }

        // Advance per-surface pacing deadlines only for surfaces that
        // actually had an encode job collected.  Surfaces skipped due to
        // in-flight limits or unchanged pixels keep their current
        // deadline so the next tick retries without burning a time slot.
        for work in &client_work {
            if let Some(client) = sess.clients.get_mut(&work.cid) {
                for &sid in &work.subs {
                    if encoded_client_surfaces.contains(&(work.cid, sid)) {
                        // Per surface: pacing now reads that surface's own
                        // inflight depth, so one congested surface no longer
                        // sets the cadence for its neighbours.
                        if surface_delivery_is_throttled(client, sid) {
                            let interval = surface_send_interval(client, sid);
                            let deadline = client
                                .surface_subs
                                .entry(sid)
                                .or_default()
                                .next_send_at
                                .get_or_insert(now);
                            advance_deadline(deadline, now, interval);
                        } else {
                            client.surface_subs.entry(sid).or_default().next_send_at = None;
                        }
                    }
                }
            }
        }
    }

    if !encode_jobs.is_empty() {
        // Fire-and-forget, with completions consumed in finish order.
        //
        // Encodes themselves have always run concurrently, but their
        // handles used to be awaited as one batch before *any* result was
        // returned to its subscription.  One slow background surface then
        // kept a fast active surface's encoder in `encode_in_flight` past its
        // next display-rate slot.  Deliver each result independently so one
        // surface's encode time cannot set another surface's cadence.
        let state2 = state.clone();
        tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            let mut job_ids = HashMap::new();
            let spawn_encode_job = |tasks: &mut tokio::task::JoinSet<EncodeResult>,
                                    job_ids: &mut HashMap<tokio::task::Id, (u64, u16)>,
                                    job: EncodeJob| {
                let job_id = (job.cid, job.sid);
                let handle = tasks.spawn_blocking(move || {
                    let worker_started_at = Instant::now();
                    let mut encoder = job.encoder;
                    if job.needs_keyframe {
                        encoder.request_keyframe();
                    }
                    // The compositor produces a target-sized PixelData per
                    // registered (sid, target) — either a zero-copy
                    // NV12/VA-Surface DMA-BUF (VAAPI GBM-backed) or a
                    // server-allocated BGRA staging buffer filled by a GPU
                    // copy (NVENC, software). Both arrive at the encoder's
                    // source dimensions, so no CPU resize is required.
                    let nal_data = encoder.encode_pixels(&job.pixels);
                    let worker_finished_at = Instant::now();
                    let codec_flag = encoder.codec_flag();
                    EncodeResult {
                        cid: job.cid,
                        sid: job.sid,
                        target_w: job.target_w,
                        target_h: job.target_h,
                        generation: job.generation,
                        encoder,
                        nal_data,
                        codec_flag,
                        timestamp_ms: job.timestamp_ms,
                        timestamp_sub_us: job.timestamp_sub_us,
                        queued_at: job.queued_at,
                        worker_started_at,
                        worker_finished_at,
                    }
                });
                job_ids.insert(handle.id(), job_id);
            };
            for job in encode_jobs {
                spawn_encode_job(&mut tasks, &mut job_ids, job);
            }

            // Timeout: if a hardware encoder hangs (e.g. vaSyncSurface on
            // AMD), don't leave its subscription permanently in flight.
            // Finished siblings are delivered while this clock is running;
            // only jobs still pending after a quiet five seconds are lost.
            const ENCODE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

            loop {
                let mut results = Vec::with_capacity(1);
                let mut failed: Vec<(u64, u16)> = Vec::new();
                let mut timed_out = false;
                match tokio::time::timeout(ENCODE_TIMEOUT, tasks.join_next_with_id()).await {
                    Ok(Some(Ok((task_id, result)))) => {
                        job_ids.remove(&task_id);
                        results.push(result);
                    }
                    Ok(Some(Err(join_err))) => {
                        // spawn_blocking panicked — encoder is lost.
                        if let Some((cid, sid)) = job_ids.remove(&join_err.id()) {
                            eprintln!(
                                "[surface-encoder] encode task panicked: cid={cid} sid={sid}",
                            );
                            failed.push((cid, sid));
                        }
                    }
                    Ok(None) => break,
                    Err(_timeout) => {
                        // Blocking tasks cannot be forcibly stopped once
                        // running, so their encoders are lost.  Clear every
                        // remaining subscription and let it rebuild.
                        for (_, (cid, sid)) in job_ids.drain() {
                            eprintln!(
                                "[surface-encoder] encode timed out ({}s): cid={cid} sid={sid}",
                                ENCODE_TIMEOUT.as_secs(),
                            );
                            failed.push((cid, sid));
                        }
                        tasks.abort_all();
                        timed_out = true;
                    }
                }

                // Deliver this surface as soon as its own encode completes.
                let mut sess = state2.session.lock().await;
                let now = Instant::now();
                let mut local_encodes = 0u32;
                let mut local_encode_bytes = 0u64;
                let mut local_frames_sent = 0u32;
                let mut chained_jobs = Vec::new();
                // A successful completion delivers its frame here and either
                // chains the newest pending generation or returns the encoder
                // to the subscription.  In both cases there is no work for
                // the delivery tick: the next compositor commit will wake it
                // when that returned encoder is needed.  Only exceptional
                // paths that need to retry cached state have to nudge it.
                let mut needs_delivery_nudge = !failed.is_empty() || timed_out;

                // Clean up in-flight tracking for panicked/timed-out encodes.
                // Without this, the surface is permanently blocked from
                // future encode jobs and frame delivery stops for it.
                for (cid, sid) in failed {
                    if let Some(client) = sess.clients.get_mut(&cid) {
                        // The encoder was moved into the spawn_blocking closure
                        // and is now lost.  A fresh encoder will be created on
                        // the next tick when the sub's encoder is None.  Force
                        // a keyframe so the new encoder starts with a clean
                        // reference chain.
                        let s = client.surface_subs.entry(sid).or_default();
                        s.encode_in_flight = false;
                        s.reserved_encode_bytes = 0;
                        s.in_flight_generation = None;
                        s.pending_encode = None;
                        s.has_keyframe = false;
                    }
                }

                for result in results {
                    let queue_us = result
                        .worker_started_at
                        .duration_since(result.queued_at)
                        .as_micros()
                        .min(u64::MAX as u128) as u64;
                    let work_us = result
                        .worker_finished_at
                        .duration_since(result.worker_started_at)
                        .as_micros()
                        .min(u64::MAX as u128) as u64;
                    let handoff_us = now
                        .duration_since(result.worker_finished_at)
                        .as_micros()
                        .min(u64::MAX as u128) as u64;
                    sess.surface_encode_jobs = sess.surface_encode_jobs.saturating_add(1);
                    sess.surface_encode_queue_us =
                        sess.surface_encode_queue_us.saturating_add(queue_us);
                    sess.surface_encode_queue_max_us =
                        sess.surface_encode_queue_max_us.max(queue_us);
                    sess.surface_encode_work_us =
                        sess.surface_encode_work_us.saturating_add(work_us);
                    sess.surface_encode_work_max_us = sess.surface_encode_work_max_us.max(work_us);
                    sess.surface_encode_handoff_us =
                        sess.surface_encode_handoff_us.saturating_add(handoff_us);
                    sess.surface_encode_handoff_max_us =
                        sess.surface_encode_handoff_max_us.max(handoff_us);

                    // Return the encoder unless a resubscribe invalidated
                    // it mid-encode.  Don't compare against `last_pixels`
                    // here — it races with concurrent ticks.  The next
                    // tick's `needs_new_encoder` check rebuilds the
                    // encoder before any encode at the new size.
                    let mut returned_encoder = Some(result.encoder);
                    let accepted = if let Some(client) = sess.clients.get_mut(&result.cid) {
                        let state = client.surface_subs.entry(result.sid).or_default();
                        accept_completed_encode(state, result.generation, result.nal_data.is_some())
                    } else {
                        continue;
                    };
                    if !accepted {
                        // This output belongs to the pre-resubscribe encoder.
                        // Dropping it preserves the keyframe debt set by the
                        // subscription change, so the replacement encoder
                        // starts a decoder-compatible reference chain.
                        retire_encoder(returned_encoder.take());
                        needs_delivery_nudge = true;
                        continue;
                    }

                    let Some((nal_data, is_keyframe)) = result.nal_data else {
                        needs_delivery_nudge = true;
                        if let Some(client) = sess.clients.get_mut(&result.cid) {
                            let state = client.surface_subs.entry(result.sid).or_default();
                            state.encoder = returned_encoder.take();
                            state.pending_encode = None;
                            state.nal_none_streak += 1;
                            let streak = state.nal_none_streak;
                            if streak == 10 {
                                retire_encoder(state.encoder.take());
                                state.nal_none_latched_at = Some(now);
                                state.has_keyframe = false;
                                eprintln!(
                                    "[encode] nal_data=None x{streak} sid={} cid={} {}x{} — dropping encoder, backing off retry",
                                    result.sid, result.cid, result.target_w, result.target_h,
                                );
                            } else if streak < 10 {
                                eprintln!(
                                    "[encode] nal_data=None sid={} cid={} {}x{}",
                                    result.sid, result.cid, result.target_w, result.target_h,
                                );
                            }
                            // streak >= 10: suppress the log spam
                        }
                        continue;
                    };
                    // Encoder produced output — reset the None streak.
                    if let Some(client) = sess.clients.get_mut(&result.cid)
                        && let Some(s) = client.surface_subs.get_mut(&result.sid)
                    {
                        s.nal_none_streak = 0;
                    }

                    {
                        static EC: std::sync::atomic::AtomicU64 =
                            std::sync::atomic::AtomicU64::new(0);
                        let n = EC.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if n < 5 || n.is_multiple_of(1000) {
                            eprintln!(
                                "[encode #{n}] sid={} {}x{} kf={is_keyframe} bytes={}",
                                result.sid,
                                result.target_w,
                                result.target_h,
                                nal_data.len(),
                            );
                        }
                    }

                    local_encodes += 1;
                    local_encode_bytes += nal_data.len() as u64;

                    let flags = result.codec_flag
                        | if is_keyframe {
                            SURFACE_FRAME_FLAG_KEYFRAME
                        } else {
                            0
                        };
                    let Some(client) = sess.clients.get_mut(&result.cid) else {
                        continue;
                    };
                    // Don't check window_open here — we already checked before
                    // starting the encode job.  Dropping an encoded P-frame
                    // breaks the decoder's reference chain and causes glitches.
                    // With the per-sub `encode_in_flight` flag limiting to 1
                    // concurrent encode per surface, at most 1 frame arrives
                    // after the window closes, which is acceptable.
                    let sent = match enqueue_surface_frame(
                        client,
                        result.sid,
                        result.timestamp_ms,
                        result.timestamp_sub_us,
                        flags,
                        is_keyframe,
                        nal_data,
                    ) {
                        Err(()) => {
                            // Receiver dropped (client disconnected during encode).
                            // Request keyframe so the next encoder starts clean.
                            client
                                .surface_subs
                                .entry(result.sid)
                                .or_default()
                                .has_keyframe = false;
                            false
                        }
                        Ok(bytes) => {
                            // Track surface frames in their own inflight queue
                            // so surface ACKs feed shared goodput / RTT without
                            // polluting terminal frame-size averages or probing.
                            record_surface_frame_sent(client, result.sid, bytes, is_keyframe, now);
                            // Prefer updating avg_surface_frame_bytes from delta
                            // (non-keyframe) frames — keyframes are 5-10× larger
                            // than P-frames and would inflate the average, dragging
                            // surface_pacing_fps below the sustainable rate.
                            //
                            // However, we must still update from keyframes with a
                            // very slow alpha: all-intra encoders (e.g. AV1 VAAPI
                            // before P-frame support) only produce keyframes, so
                            // skipping them entirely leaves the average stuck at
                            // the 8 KB initial value, causing the pacer to wildly
                            // overshoot the send rate and saturate the transport.
                            if !is_keyframe {
                                client.avg_surface_frame_bytes = ewma_with_direction(
                                    client.avg_surface_frame_bytes,
                                    bytes as f32,
                                    0.5,
                                    0.125,
                                );
                            } else if client.avg_surface_frame_bytes <= 16_384.0 {
                                // First keyframe while the estimate is still at or
                                // near the initial 8 KB seed.  No P-frame data has
                                // been seen yet, so the seed is pure fiction.  Use a
                                // realistic P-frame estimate: keyframes are typically
                                // 3-8× larger than P-frames, so divide by 4.  This
                                // prevents surface_pacing_fps from being wildly
                                // optimistic (8 KB → 32 fps at 256 KB/s) when the
                                // actual frames are 50-200 KB keyframes.
                                client.avg_surface_frame_bytes = (bytes as f32 / 4.0).max(4_096.0);
                            } else {
                                // Slow convergence so one keyframe doesn't wreck
                                // the estimate for dozens of subsequent P-frames.
                                client.avg_surface_frame_bytes = ewma_with_direction(
                                    client.avg_surface_frame_bytes,
                                    bytes as f32,
                                    0.05,
                                    0.05,
                                );
                            }
                            client.frames_sent = client.frames_sent.wrapping_add(1);
                            local_frames_sent += 1;
                            let s = client.surface_subs.entry(result.sid).or_default();
                            if is_keyframe {
                                s.has_keyframe = true;
                            }
                            s.burst_remaining = s.burst_remaining.saturating_sub(1);
                            true
                        }
                    };

                    let pending = client
                        .surface_subs
                        .entry(result.sid)
                        .or_default()
                        .pending_encode
                        .take();
                    let had_pending = pending.is_some();
                    let chained_needs_keyframe = pending.as_ref().is_some_and(|pending| {
                        chained_encode_needs_keyframe(
                            pending.needs_keyframe,
                            pending.force_quality_refresh,
                            is_keyframe,
                        )
                    });
                    let reserved_bytes =
                        estimated_surface_frame_bytes(client, result.sid, chained_needs_keyframe);
                    let can_chain = sent
                        && !surface_delivery_is_throttled(client, result.sid)
                        && surface_frame_credit_open_for(client, result.sid, reserved_bytes);
                    let state = client.surface_subs.entry(result.sid).or_default();
                    if can_chain
                        && let Some(pending) = pending
                        && returned_encoder.as_ref().is_some_and(|encoder| {
                            encoder.source_dimensions() == (pending.target_w, pending.target_h)
                        })
                    {
                        state.encode_in_flight = true;
                        state.reserved_encode_bytes = reserved_bytes;
                        state.in_flight_generation = Some(pending.generation);
                        chained_jobs.push(EncodeJob {
                            cid: result.cid,
                            sid: result.sid,
                            target_w: pending.target_w,
                            target_h: pending.target_h,
                            pixels: pending.pixels,
                            // A keyframe that just satisfied startup or
                            // recovery debt also makes the queued frame's
                            // reference chain decodable. Do not emit a
                            // redundant second IDR. A quality refresh is
                            // a separate explicit refresh request.
                            needs_keyframe: chained_needs_keyframe,
                            generation: pending.generation,
                            encoder: returned_encoder.take().unwrap(),
                            timestamp_ms: pending.timestamp_ms,
                            timestamp_sub_us: pending.timestamp_sub_us,
                            queued_at: Instant::now(),
                        });
                    } else {
                        state.encoder = returned_encoder.take();
                        // A pending frame that could not be chained (size
                        // change or fresh backpressure) has been dropped
                        // intentionally.  Revisit the authoritative cache
                        // so it can be rebuilt or sent when the gate opens.
                        needs_delivery_nudge |= had_pending;
                    }
                }
                sess.surface_encodes += local_encodes;
                sess.surface_encode_bytes += local_encode_bytes;
                sess.surface_frames_sent += local_frames_sent;
                drop(sess);
                for job in chained_jobs {
                    spawn_encode_job(&mut tasks, &mut job_ids, job);
                }
                if needs_delivery_nudge {
                    state2.delivery_notify.notify_one();
                }
                if timed_out {
                    break;
                }
            }
        });
    }

    if !create_jobs.is_empty() {
        // Encoder creation runs on spawn_blocking so VA-API device open
        // and context allocation don't stall the tick loop.  When the
        // task lands, the main loop installs the encoder into the sub's
        // `encoder` slot, forwards the GBM buffers to the compositor
        // (`SetExternalOutputBuffers`), and sends a Surface Encoder event
        // to the client.  Encoding starts on the NEXT tick — once the
        // compositor has committed a frame through the new buffers.
        let state2 = state.clone();
        tokio::spawn(async move {
            // Track (cid, sid) for each job so we can clear
            // `creation_in_flight` if a task panics or times out.
            let job_ids: Vec<(u64, u16)> = create_jobs.iter().map(|j| (j.cid, j.sid)).collect();

            let handles: Vec<_> = create_jobs
                .into_iter()
                .map(|job| {
                    tokio::task::spawn_blocking(move || {
                        let params = job.params;
                        #[allow(unused_mut)]
                        let mut encoder = match SurfaceEncoder::new(
                            &params.preferences,
                            job.target_w,
                            job.target_h,
                            &params.vaapi_device,
                            params.encoding,
                            params.verbose,
                            params.codec_support,
                            params.chroma,
                        ) {
                            Ok(enc) => enc,
                            Err(err) => {
                                if params.verbose {
                                    eprintln!(
                                        "[surface-encoder] cid={} sid={} {}x{}: {err}",
                                        job.cid, job.sid, job.target_w, job.target_h,
                                    );
                                }
                                // Families are eliminated at 4:2:0, the chroma
                                // every attempt falls back to, so one missing
                                // there is missing outright.
                                let oversized = refused_for_size(
                                    &params.preferences,
                                    params.codec_support,
                                    job.target_w,
                                    job.target_h,
                                    |p| {
                                        !surface_encoder::known_unavailable(
                                            p,
                                            surface_encoder::ChromaSubsampling::Cs420,
                                        )
                                    },
                                );
                                return CreateResult {
                                    cid: job.cid,
                                    sid: job.sid,
                                    native_w: job.native_w,
                                    native_h: job.native_h,
                                    encoder: None,
                                    fresh: None,
                                    oversized,
                                    vulkan_predecessors_exhausted: params
                                        .probing_vulkan_predecessors
                                        .then_some((job.target_w, job.target_h)),
                                };
                            }
                        };

                        #[cfg(target_os = "linux")]
                        let external_bufs = {
                            {
                                let drm_fd = encoder.drm_fd_raw();
                                let count = encoder.gbm_buffers().len();
                                if count > 0 {
                                    encoder.allocate_nv12_buffers(drm_fd, count);
                                }
                            }
                            let gbm_bufs = encoder.gbm_buffers();
                            if gbm_bufs.is_empty() {
                                Vec::new()
                            } else {
                                let nv12_bufs = encoder.gbm_nv12_buffers();
                                let (enc_w, enc_h) = encoder.encoder_dimensions();
                                let bufs: Result<Vec<_>, std::io::Error> = gbm_bufs
                                    .iter()
                                    .enumerate()
                                    .map(|(i, b)| {
                                        let nv12 = nv12_bufs.get(i);
                                        Ok(yas_compositor::ExternalOutputBuffer {
                                            fd: std::sync::Arc::new(b.fd.try_clone()?),
                                            fourcc: 0x34325241,
                                            modifier: 0,
                                            stride: b.stride,
                                            offset: 0,
                                            width: b.width,
                                            height: b.height,
                                            va_surface_id: 0,
                                            va_display: 0,
                                            planes: vec![yas_compositor::ExternalOutputPlane {
                                                offset: 0,
                                                pitch: b.stride,
                                            }],
                                            nv12_fd: nv12.map(|n| n.fd.clone()),
                                            nv12_stride: nv12.map_or(0, |n| n.stride),
                                            nv12_uv_offset: nv12.map_or(0, |n| n.uv_offset),
                                            nv12_modifier: nv12.map_or(0, |n| n.modifier),
                                            nv12_width: enc_w,
                                            nv12_height: enc_h,
                                        })
                                    })
                                    .collect();
                                match bufs {
                                    Ok(b) => b,
                                    Err(e) => {
                                        eprintln!("[encode] dup gbm fd failed: {e}");
                                        Vec::new()
                                    }
                                }
                            }
                        };
                        let fresh = FreshEncoder {
                            name: encoder.encoder_name(),
                            codec_string: encoder.webcodecs_codec_string(),
                            #[cfg(target_os = "linux")]
                            external_bufs,
                        };
                        CreateResult {
                            cid: job.cid,
                            sid: job.sid,
                            native_w: job.native_w,
                            native_h: job.native_h,
                            encoder: Some(encoder),
                            fresh: Some(fresh),
                            oversized: false,
                            vulkan_predecessors_exhausted: None,
                        }
                    })
                })
                .collect();

            const CREATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
            let mut results: Vec<CreateResult> = Vec::with_capacity(handles.len());
            let mut failed: Vec<(u64, u16)> = Vec::new();
            for (i, h) in handles.into_iter().enumerate() {
                let wrapper =
                    tokio::spawn(async move { tokio::time::timeout(CREATE_TIMEOUT, h).await });
                match wrapper.await {
                    Ok(Ok(Ok(r))) => results.push(r),
                    Ok(Ok(Err(_))) | Ok(Err(_)) => {
                        let (cid, sid) = job_ids[i];
                        eprintln!("[surface-encoder] create task failed: cid={cid} sid={sid}",);
                        failed.push(job_ids[i]);
                    }
                    Err(_) => return,
                }
            }

            let mut sess = state2.session.lock().await;
            let now = Instant::now();

            // Clear creation_in_flight for failed tasks; latch a brief
            // backoff so the next tick doesn't immediately retry.
            for (cid, sid) in failed {
                if let Some(client) = sess.clients.get_mut(&cid)
                    && let Some(s) = client.surface_subs.get_mut(&sid)
                {
                    s.creation_in_flight = false;
                    s.nal_none_streak = 10;
                    s.nal_none_latched_at = Some(now);
                }
            }

            // Surfaces whose ceiling moved as a result of these creations —
            // either a backend resolved to something other than what sizing
            // assumed, or a request was refused for size.  Re-mediated after
            // the loop, because the composite was sized against a guess: left
            // alone it renders every frame at a resolution no subscriber can
            // actually be sent.
            let mut receilinged_surfaces: Vec<u16> = Vec::new();

            for result in results {
                // The compositor took this (surface, client) over while the
                // encoder was being built: a Vulkan Video session now owns
                // delivery, and installing this one would run two encoders
                // against one decoder — the enc_msg below would reconfigure
                // the client back to the server-side codec, and the target
                // registration would redirect the composite the Vulkan
                // session encodes from.  Drop the freshly built encoder.
                if sess
                    .clients
                    .get(&result.cid)
                    .is_some_and(|c| c.vulkan_video_surfaces.contains_key(&result.sid))
                {
                    if let Some(client) = sess.clients.get_mut(&result.cid)
                        && let Some(s) = client.surface_subs.get_mut(&result.sid)
                    {
                        s.creation_in_flight = false;
                        s.encoder_invalidated = false;
                    }
                    continue;
                }

                // Reject a creation invalidated by a resubscribe before it
                // changes compositor targets or pixel caches.  The final
                // target will be built on the next delivery tick.
                let accepted = if let Some(client) = sess.clients.get_mut(&result.cid)
                    && let Some(state) = client.surface_subs.get_mut(&result.sid)
                {
                    accept_completed_creation(state)
                } else {
                    false
                };
                if !accepted {
                    continue;
                }

                let Some(encoder) = result.encoder else {
                    if let Some(client) = sess.clients.get_mut(&result.cid)
                        && let Some(s) = client.surface_subs.get_mut(&result.sid)
                    {
                        if let Some(extent) = result.vulkan_predecessors_exhausted {
                            s.vulkan_predecessors_exhausted_extent = Some(extent);
                            s.nal_none_streak = 0;
                            s.nal_none_latched_at = None;
                            continue;
                        }
                        s.create_failures = s.create_failures.saturating_add(1);
                        // Bring the surface down to what the whole chain
                        // clears when the size is what stands in the way.
                        // Either it plainly is — nothing eligible could have
                        // carried the frame — or the backends that could have
                        // keep failing, and after enough tries a smaller
                        // picture beats none.  The counter is what separates
                        // the two from a momentary failure, which must not
                        // cost the viewer its resolution: this only clears on
                        // a resubscribe.
                        let narrow =
                            result.oversized || s.create_failures >= CREATE_FAILURES_BEFORE_DEGRADE;
                        if narrow && !s.encoder_cap_degraded {
                            // Retry at once rather than serving the backoff:
                            // the smaller size may simply work, and waiting
                            // stalls the first picture by seconds on every
                            // AV1-less host with a >4K display.
                            s.encoder_cap_degraded = true;
                            receilinged_surfaces.push(result.sid);
                        } else {
                            s.nal_none_streak = 10;
                            s.nal_none_latched_at = Some(now);
                        }
                    }
                    continue;
                };

                // Move the external buffers (and register them with the
                // compositor) BEFORE stashing the encoder, so subsequent
                // ticks see the encoder only once its buffers are live.
                let fresh = result.fresh;
                #[cfg(target_os = "linux")]
                {
                    if let Some(f) = &fresh
                        && !f.external_bufs.is_empty()
                        && let Some(cs) = sess.compositor.as_mut()
                    {
                        // Drop every cached snapshot for this surface so
                        // the next compositor frame re-fills with the
                        // newly-registered NV12 DMA-BUF target.  Stale
                        // entries (e.g. native BGRA from a previous
                        // tick) will be re-added by SurfaceCommit.
                        last_pixels_remove_for_sid(&mut cs.last_pixels, result.sid);
                        last_pixels_remove_for_sid(&mut cs.last_opaque_pixels, result.sid);
                        cs.mark_pixel_snapshot_dirty();
                    }
                }
                #[cfg(target_os = "linux")]
                let (fresh_meta, external_bufs) = match fresh {
                    Some(f) => (Some((f.name, f.codec_string)), Some(f.external_bufs)),
                    None => (None, None),
                };
                #[cfg(not(target_os = "linux"))]
                let fresh_meta = fresh.map(|f| (f.name, f.codec_string));

                #[cfg(target_os = "linux")]
                {
                    let (tw, th) = encoder.source_dimensions();
                    // Clear the previously-registered downscale target
                    // for this client/surface (if any) so stale entries
                    // don't accumulate when the per-client target dims
                    // change.  Externals replace by key in the renderer
                    // (`set_external_output_buffers`) so they don't
                    // need an explicit clear, but downscale targets do.
                    let prev_target = sess
                        .clients
                        .get(&result.cid)
                        .and_then(|c| c.surface_subs.get(&result.sid))
                        .and_then(|s| s.last_registered_target);
                    if let Some((pw, ph)) = prev_target
                        && (pw, ph) != (tw, th)
                    {
                        // Ownership is shared by target key, not by client.
                        // Remove this subscriber from the old key first, then
                        // let the surviving subscribers decide whether it is
                        // re-registered or actually cleared. Clearing it
                        // directly strands every survivor on BGRA while
                        // their state still says the opaque target exists.
                        if let Some(s) = sess
                            .clients
                            .get_mut(&result.cid)
                            .and_then(|c| c.surface_subs.get_mut(&result.sid))
                        {
                            s.last_registered_target = None;
                            s.last_registered_native = None;
                        }
                        sess.resettle_downscale_target(result.sid, pw, ph);
                    }
                    // Resolve both representations for this target. Mixed
                    // CPU/NVENC subscribers get BGRA and opaque NV12/NV24;
                    // matching NVENC-only subscribers keep the no-readback
                    // path. A 4:2:0/4:4:4 split still falls back to BGRA
                    // because one opaque allocation cannot have both shapes.
                    //
                    // Computed before the compositor borrow below, which
                    // takes `sess` mutably.
                    let encoder_wants_nv12_opaque = encoder.wants_nv12_opaque_fd();
                    let encoder_opaque_444 = encoder.opaque_wants_444();
                    let target_mode = downscale_target_mode(
                        encoder_wants_nv12_opaque,
                        encoder_opaque_444,
                        !encoder_wants_nv12_opaque,
                        (tw, th),
                        sess.clients
                            .iter()
                            .filter(|(cid, _)| **cid != result.cid)
                            .map(|(_, c)| {
                                c.surface_subs
                                    .get(&result.sid)
                                    .map(|s| {
                                        let is_vulkan =
                                            c.vulkan_video_surfaces.contains_key(&result.sid);
                                        (
                                            s.last_registered_target,
                                            s.wants_nv12_opaque,
                                            s.wants_opaque_444,
                                            !is_vulkan && !s.wants_nv12_opaque,
                                        )
                                    })
                                    .unwrap_or((None, true, encoder_opaque_444, false))
                            }),
                    );
                    if let Some(bufs) = external_bufs
                        && !bufs.is_empty()
                        && let Some(cs) = sess.compositor.as_mut()
                    {
                        let _ = cs.handle.command_tx.try_send(
                            yas_compositor::CompositorCommand::SetExternalOutputBuffers {
                                surface_id: result.sid as u32,
                                target_w: tw,
                                target_h: th,
                                native_w: result.native_w,
                                native_h: result.native_h,
                                buffers: bufs,
                            },
                        );
                        cs.handle.wake();
                    } else if let Some(cs) = sess.compositor.as_mut() {
                        // No GBM externals — register a server-allocated
                        // downscale target so the compositor can GPU-copy
                        // the native composite into target-sized pixels for
                        // this encoder.  Idempotent in the renderer.
                        //
                        // NVENC additionally asks for the NV12 OPAQUE_FD
                        // shape, which converts on the GPU and hands over a
                        // handle CUDA can import — skipping the readback
                        // into staging and the Vec that used to carry it.
                        // Every other backend needs pixels on the CPU and
                        // takes the BGRA path. The renderer falls back to
                        // BGRA on its own if the export fails, so this
                        // stays a request rather than a commitment, and it
                        // reconciles a `false` here by dropping an NV12
                        // target it had already built.
                        // The command can replace the opaque allocation
                        // (layout change, failed export, or Vulkan takeover).
                        // Its cached fd must not outlive that allocation.
                        cs.last_opaque_pixels.remove(&(result.sid, tw, th));
                        cs.mark_pixel_snapshot_dirty();
                        let _ = cs.handle.command_tx.try_send(
                            yas_compositor::CompositorCommand::RegisterDownscaleTarget {
                                surface_id: result.sid as u32,
                                target_w: tw,
                                target_h: th,
                                native_w: result.native_w,
                                native_h: result.native_h,
                                want_nv12_opaque: target_mode.want_nv12_opaque,
                                want_cpu_pixels: target_mode.want_cpu_pixels,
                                opaque_is_444: target_mode.opaque_is_444,
                            },
                        );
                        cs.handle.wake();
                    }
                    if let Some(client) = sess.clients.get_mut(&result.cid) {
                        let s = client.surface_subs.entry(result.sid).or_default();
                        s.last_registered_target = Some((tw, th));
                        s.last_registered_native = Some((result.native_w, result.native_h));
                        // This encoder's own capability, not the resolved
                        // decision above: a later subscriber asks whether
                        // *we* could take NV12, and must not inherit a
                        // "no" we only arrived at because of a third party
                        // that has since gone away.
                        s.wants_nv12_opaque = encoder_wants_nv12_opaque;
                        s.wants_opaque_444 = encoder_opaque_444;
                    }
                }
                #[cfg(not(target_os = "linux"))]
                let _ = &encoder;

                if let Some(client) = sess.clients.get_mut(&result.cid) {
                    let state = client.surface_subs.entry(result.sid).or_default();
                    // Sizing has been guessing which backend would win; now it
                    // knows.  A surface that came up on AV1 can grow past the
                    // H.264 ceiling, and one that came up on H.264 stops
                    // being composited as if it might not — but only after a
                    // re-mediation, so note it when the answer is new.
                    //
                    // `encoder_cap_degraded` is deliberately *not* cleared
                    // here.  It latches only when a request was refused for
                    // size, and clearing it on the smaller creation that
                    // followed would let the next winner's wider ceiling
                    // raise the surface straight back into the size that was
                    // just refused.  A resubscribe clears it; that is the
                    // point at which retrying is a fresh question.
                    let winner = Some(encoder.preference());
                    if state.selected_encoder != winner {
                        state.selected_encoder = winner;
                        receilinged_surfaces.push(result.sid);
                    }
                    state.encoder = Some(encoder);
                    state.nal_none_streak = 0;
                    state.nal_none_latched_at = None;
                    state.create_failures = 0;
                    let _ = fresh_meta;
                }
            }
            if !receilinged_surfaces.is_empty() {
                sess.resize_surfaces_to_mediated_sizes(
                    receilinged_surfaces,
                    &state2.config.surface_encoders,
                    state2.config.verbose,
                );
            }
            drop(sess);
            state2.delivery_notify.notify_one();
        });
    }

    // Keep Wayland source clocks independent of the delivery loop.  The
    // compositor owns the actual fixed-rate timer; this tick only reconciles
    // its configuration when subscriptions or display rates change.  A slow
    // encoder, a closed transport window, or 200 ms of RTT can therefore
    // discard stream frames without slowing the application/rAF clock.
    let reconcile_cadence = sess
        .compositor
        .as_ref()
        .is_some_and(|cs| cs.frame_clocks_dirty);
    {
        let mut desired_clocks = reconcile_cadence.then(|| {
            let mut clocks: FxHashMap<u16, Duration> = FxHashMap::default();
            for client in sess.clients.values() {
                for &sid in &client.surface_subscriptions {
                    let interval = surface_source_interval(client, sid);
                    clocks
                        .entry(sid)
                        .and_modify(|current| *current = (*current).min(interval))
                        .or_insert(interval);
                }
            }
            #[cfg(target_os = "linux")]
            if let Some(compositor) = sess.compositor.as_ref() {
                for stream in compositor
                    .screencasts
                    .values()
                    .flat_map(|session| session.streams.iter())
                {
                    clocks
                        .entry(stream.surface_id)
                        .and_modify(|current| {
                            *current = (*current).min(Duration::from_millis(33));
                        })
                        .or_insert(Duration::from_millis(33));
                }
            }
            clocks
        });
        if let Some(clocks) = desired_clocks.as_ref() {
            for client in sess.clients.values_mut() {
                for &sid in &client.surface_subscriptions {
                    client.surface_subs.entry(sid).or_default().source_interval =
                        clocks.get(&sid).copied();
                }
            }
        }

        let blanket_interval = blanket_frame_interval(&sess);
        if let Some(cs) = sess.compositor.as_mut() {
            if let Some(desired_clocks) = desired_clocks.as_mut() {
                // A stale subscription can briefly outlive its Wayland
                // surface; do not leave a useless high-rate clock installed.
                desired_clocks.retain(|sid, _| cs.surfaces.contains_key(sid));

                let removed: Vec<u16> = cs
                    .frame_clock_intervals
                    .keys()
                    .filter(|sid| !desired_clocks.contains_key(sid))
                    .copied()
                    .collect();
                for sid in removed {
                    cs.handle.set_frame_interval(sid, None);
                    cs.frame_clock_intervals.remove(&sid);
                }
                for (&sid, &interval) in desired_clocks.iter() {
                    if cs.frame_clock_intervals.get(&sid) != Some(&interval) {
                        cs.handle.set_frame_interval(sid, Some(interval));
                        cs.frame_clock_intervals.insert(sid, interval);
                    }
                }
                cs.frame_clocks_dirty = false;
            }

            // Unwatched surfaces retain the low-rate liveness callback.  Do
            // not mix it into actively clocked surfaces: an off-phase extra
            // callback would make Chromium's BeginFrame cadence irregular.
            let mut blanket_requests = 0u32;
            if let Some(interval) = blanket_interval
                && now.duration_since(cs.last_blanket_frame_request) >= interval
            {
                for &sid in cs.surfaces.keys() {
                    if cs.frame_clock_intervals.contains_key(&sid) {
                        continue;
                    }
                    if cs
                        .handle
                        .command_tx
                        .send(CompositorCommand::RequestFrame {
                            surface_id: sid,
                            presentation_at: now,
                        })
                        .is_ok()
                    {
                        blanket_requests = blanket_requests.saturating_add(1);
                    }
                }
                cs.last_blanket_frame_request = now;
                if blanket_requests > 0 {
                    cs.handle.wake();
                }
            }
            let clock_requests = cs.handle.take_frame_clock_requests();
            sess.frame_requests = sess
                .frame_requests
                .saturating_add(clock_requests)
                .saturating_add(blanket_requests);
        }
    }

    // Yield the session lock briefly so pending encode deliveries from
    // previous ticks can acquire the lock and send their frames without
    // waiting for terminal processing to complete.  This reduces the
    // latency between encode completion and frame-on-wire.
    drop(sess);
    tokio::task::yield_now().await;
    sess = state.session.lock().await;

    let max_display_fps = sess
        .clients
        .values()
        .map(|client| client.display_fps)
        .fold(1.0_f32, f32::max);
    let output_coalesce_cap = pty_output_coalesce_cap(max_display_fps);
    let mut ids: SmallVec<[u16; 8]> = sess.ptys.keys().copied().collect();
    ids.sort_unstable();
    for &id in &ids {
        let Some(pty) = sess.ptys.get_mut(&id) else {
            continue;
        };
        if pty.driver.take_title_dirty() || pty.driver.take_used_rows_dirty() {
            pty.mark_dirty();
        }
        drop(pty.driver.take_clipboard_stores());
    }

    // Drain bytes from PTY reader channels. This is the only place
    // process() is called, so there is no contention with the readers.
    //
    // End-to-end flow control, two brakes on the same chain (`byte_rx`
    // fills to its bounded capacity → the reader task's
    // `byte_tx.blocking_send` blocks → the kernel's PTY master buffer
    // fills → the child process's `write(stdout, ...)` blocks):
    //
    // 1. When at least one client is subscribed to a PTY and its
    //    `ready_frames` queue is full, stop draining that PTY.
    //    Sync-bracketed frames are never silently dropped; the producer
    //    is slowed instead.
    // 2. The per-PTY and per-session parse budgets, for output that never
    //    emits a sync boundary (so brake 1 never engages — `ready_frames`
    //    only fills on SyncBoundary) and for PTYs with no subscriber at all.
    //    Without both, one flooding PTY can run this loop indefinitely, or a
    //    stack can multiply the mutex hold by its number of flooding units.
    let ptys_with_subscribers: FxHashSet<u16> = sess
        .clients
        .values()
        .flat_map(|c| c.subscriptions.iter().copied())
        .collect();
    let mut eof_ptys: Vec<(u16, u64, bool)> = Vec::with_capacity(ids.len());
    let mut parse_budget_hit = false;
    let parse_start = if ids.is_empty() {
        0
    } else {
        sess.pty_parse_cursor % ids.len()
    };
    let mut parse_ids = ids.clone();
    parse_ids.rotate_left(parse_start);
    let mut visited = 0usize;
    let mut session_budget = PTY_PARSE_BUDGET_PER_SESSION_TICK;
    for &id in &parse_ids {
        if session_budget == 0 {
            parse_budget_hit = true;
            break;
        }
        visited += 1;
        let Some(pty) = sess.ptys.get_mut(&id) else {
            continue;
        };
        let has_subscriber = ptys_with_subscribers.contains(&id);
        let mut budget = PTY_PARSE_BUDGET_PER_TICK;
        loop {
            if has_subscriber && pty.ready_frames.len() >= READY_FRAME_QUEUE_CAP {
                break;
            }
            if budget == 0 || session_budget == 0 {
                parse_budget_hit = true;
                break;
            }
            let Ok(input) = pty.byte_rx.try_recv() else {
                break;
            };
            match input {
                PtyInput::Data(data) => {
                    yas_event!(state.events, EventType::PtyRead, {
                        let mut payload = id.to_le_bytes().to_vec();
                        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
                        payload.extend_from_slice(&data);
                        payload
                    });
                    charge_pty_parse_budgets(&mut budget, &mut session_budget, data.len());
                    let parse_started = Instant::now();
                    feed_pty_chunk(pty, &data);
                    yas_event!(state.events, EventType::PtyParse, {
                        let mut payload = id.to_le_bytes().to_vec();
                        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
                        payload.extend_from_slice(
                            &(parse_started.elapsed().as_nanos().min(u64::MAX as u128) as u64)
                                .to_le_bytes(),
                        );
                        payload
                    });
                    pty.mark_output_dirty(now, output_coalesce_cap);
                }
                PtyInput::SyncBoundary { before } => {
                    yas_event!(state.events, EventType::PtyRead, {
                        let mut payload = id.to_le_bytes().to_vec();
                        payload.extend_from_slice(&(before.len() as u32).to_le_bytes());
                        payload.extend_from_slice(&before);
                        payload
                    });
                    charge_pty_parse_budgets(&mut budget, &mut session_budget, before.len());
                    if !before.is_empty() {
                        let parse_started = Instant::now();
                        feed_pty_chunk(pty, &before);
                        yas_event!(state.events, EventType::PtyParse, {
                            let mut payload = id.to_le_bytes().to_vec();
                            payload.extend_from_slice(&(before.len() as u32).to_le_bytes());
                            payload.extend_from_slice(
                                &(parse_started.elapsed().as_nanos().min(u64::MAX as u128) as u64)
                                    .to_le_bytes(),
                            );
                            payload
                        });
                        pty.mark_output_dirty(now, output_coalesce_cap);
                    }
                    if !pty.driver.synced_output() {
                        yas_event!(
                            state.events,
                            EventType::PtySnapshot,
                            id.to_le_bytes().to_vec()
                        );
                        let frame = take_snapshot(pty);
                        enqueue_ready_frame(&mut pty.ready_frames, frame);
                        pty.clear_dirty();
                    }
                }
                PtyInput::Eof => {
                    let child_exited =
                        pty.exit_drain_deadline.is_some() || pty::poll_child_exited(&pty.handle);
                    if child_exited && pty.exit_drain_deadline.is_none() {
                        pty.exit_drain_deadline = Some(now + PTY_EXIT_DRAIN_GRACE);
                    }
                    eof_ptys.push((id, pty.generation, child_exited));
                }
            }
        }
    }
    if !ids.is_empty() {
        // If the aggregate budget stopped the round, resume at the first PTY
        // not visited. A complete/idle round still rotates by one so a newly
        // noisy high id does not always sit at the back.
        sess.pty_parse_cursor = advance_pty_parse_cursor(parse_start, visited, ids.len());
    }
    if parse_budget_hit {
        // Leftover output is already queued, so re-tick right after this
        // round instead of waiting on the reader's notify — the permit for
        // the bytes we just budgeted away was consumed when this tick woke.
        // The tick loop releases the session mutex between rounds, and the
        // mutex is fair, so handlers that queued behind this round run
        // before the next one.
        state.delivery_notify.notify_one();
    }
    // Data and EOF share one ordered channel, so consuming EOF means every preceding byte is already
    // in the terminal model. If EOF beat child exit, retain the old bounded grace before cleanup.
    drop(sess);
    for (id, generation, child_exited) in eof_ptys {
        if child_exited {
            cleanup_pty_internal(id, Some(generation), state).await;
        } else {
            let state = state.clone();
            tokio::spawn(async move {
                tokio::time::sleep(PTY_EXIT_DRAIN_GRACE).await;
                cleanup_pty_internal(id, Some(generation), &state).await;
            });
        }
    }
    let mut sess = state.session.lock().await;

    // Only snapshot PTYs that have at least one client ready to consume a fresh
    // frame right now. This avoids burning CPU on snapshot+diff+compress work
    // while the lead is merely waiting for its next pacing deadline.
    let needful_ptys = sess.native_terminal_ptys_due(now);

    let mut snapshots: FxHashMap<u16, FrameState> = FxHashMap::default();
    for &id in &ids {
        let Some(pty) = sess.ptys.get_mut(&id) else {
            continue;
        };
        let needful = needful_ptys.contains(&id);
        let synced_output = pty.driver.synced_output();
        // A deferred burst may be the event that woke this tick, with no more
        // PTY bytes forthcoming. Keep the delivery loop armed for the point at
        // which it becomes snapshot-safe instead of waiting for unrelated IO.
        if pty.dirty
            && needful
            && !synced_output
            && let Some(deadline) = pty.snapshot_not_before
            && deadline > now
        {
            next_deadline = Some(next_deadline.map_or(deadline, |d| d.min(deadline)));
        }
        if needful && let Some(frame) = pty.ready_frames.pop_front() {
            // Only one state per PTY is sent in a tick. If ordinary output
            // followed this synchronized frame, make sure its already-due
            // snapshot gets another turn even when no more bytes arrive.
            if pty.dirty
                && !synced_output
                && pty
                    .snapshot_not_before
                    .is_none_or(|deadline| deadline <= now)
            {
                next_deadline = Some(next_deadline.map_or(now, |d| d.min(now)));
            }
            snapshots.insert(id, frame);
            sess.tick_snaps += 1;
            continue;
        }
        if !should_snapshot_pty(
            pty.dirty,
            needful,
            synced_output,
            pty.snapshot_not_before,
            now,
        ) {
            continue;
        }
        // DEC synchronized output supplies exact frame boundaries. Without
        // it, ordinary shell output is immediate and alternate-screen output
        // reaches here after either a short quiet period or the display-rate
        // liveness ceiling.
        yas_event!(
            state.events,
            EventType::PtySnapshot,
            id.to_le_bytes().to_vec()
        );
        snapshots.insert(id, take_snapshot(pty));
        pty.clear_dirty();
        sess.tick_snaps += 1;
    }

    if let Some(due) = sess.publish_native_terminal_frames(&snapshots, now) {
        next_deadline = Some(next_deadline.map_or(due, |known| known.min(due)));
    }

    // -- Audio frame delivery -----------------------------------------------
    //
    // Audio is no longer delivered from the tick loop — a dedicated
    // fan-out task (spawned in `AudioPipeline::spawn`) drains encoded
    // frames from the encoder mpsc and pushes them to each subscribed
    // client's `audio_tx` independently of compositor/video work.  This
    // keeps audio flowing at a steady 20 ms cadence even when a tick is
    // blocked by a long video write, and keeps the encoder's bounded
    // mpsc from overflowing into silent frame drops.
    //
    // Audio bytes are intentionally excluded from `goodput_window_bytes`:
    // at ~8 KB/s they're negligible next to video (MB/s) and keeping the
    // accounting on the tick loop would defeat the whole point of the
    // off-tick fan-out.  The has_listener flag is now managed by the
    // subscribe/unsubscribe API on `AudioBroadcast`.

    // -- Audio pipeline auto-restart ----------------------------------------
    // If the pipeline died (encoder crashed, PipeWire gone, capture stream dropped),
    // drop it, wait for a cooldown, and respawn.  This avoids permanent
    // audio loss that previously required a full client reconnect.
    //
    // The actual shutdown + respawn runs in a spawned task: shutdown() waits
    // on child processes and spawn() starts new ones, both of which can block
    // for seconds and must not hold the session mutex or park the tick loop.
    //
    #[cfg(target_os = "linux")]
    let audio_restart_bitrate: i32 = i32::from(
        sess.compositor
            .as_ref()
            .map_or(0, |cs| cs.audio_broadcast.max_native_bitrate_kbps()),
    ) * 1_000;
    #[cfg(target_os = "linux")]
    {
        let camera_results = sess
            .compositor
            .as_mut()
            .map(|cs| cs.media_input.poll())
            .unwrap_or_default();
        let mut state_changed = false;
        for result in camera_results {
            match result {
                media_input::InputDataResult::Credit { owner, credit } => {
                    if let Some(cs) = sess.compositor.as_mut() {
                        let queue = cs.native_media_input_events.entry(owner).or_default();
                        if queue.len() >= 64 {
                            queue.pop_front();
                        }
                        queue.push_back(NativeMediaInputEvent::Credit(credit));
                    }
                }
                media_input::InputDataResult::Revoked { owner, revoked } => {
                    state_changed = true;
                    if let Some(cs) = sess.compositor.as_mut() {
                        let queue = cs.native_media_input_events.entry(owner).or_default();
                        if queue.len() >= 64 {
                            queue.pop_front();
                        }
                        queue.push_back(NativeMediaInputEvent::Revoked(revoked));
                    }
                }
                media_input::InputDataResult::Ignored => {}
            }
        }
        let _ = state_changed;
    }
    #[cfg(target_os = "linux")]
    let mut audio_screencasts_closed = false;
    #[cfg(target_os = "linux")]
    let mut audio_media_revoked = Vec::new();
    #[cfg(target_os = "linux")]
    let mut audio_runtime_changed = false;
    #[cfg(target_os = "linux")]
    if let Some(ref mut cs) = sess.compositor {
        // Poll for liveness on a timer, not per tick.  `is_alive` costs up to
        // four `waitpid` syscalls and `tick` is notify-driven, so an unguarded
        // check billed every PTY output chunk for audio supervision.  A second
        // of detection latency is free here: the restart it feeds is already
        // rate-limited to once per RESTART_COOLDOWN below.
        const LIVENESS_CHECK_INTERVAL: Duration = Duration::from_secs(1);
        let due = cs
            .last_audio_liveness_check
            .is_none_or(|t| now.duration_since(t) >= LIVENESS_CHECK_INTERVAL);
        let pipeline_dead = due && {
            cs.last_audio_liveness_check = Some(now);
            cs.audio_pipeline.as_mut().is_some_and(|ap| !ap.is_alive())
        };
        let mut dead_pipeline = None;
        if pipeline_dead {
            // Clear the slot immediately. A dead process must never keep
            // PipeWire/device/ScreenCast runtime bits advertised during the
            // restart cooldown.
            dead_pipeline = cs.audio_pipeline.take();
            audio_runtime_changed = dead_pipeline.is_some();
            cs.audio_restart_needed = true;
            audio_media_revoked = cs
                .media_input
                .revoke_all(media_input::InputRevokeReason::BackendFailed);
            // The published node IDs belong to this PipeWire instance. They
            // cannot survive its loss, so revoke every session before a new
            // pipeline is installed rather than leaving consumers attached
            // to stale nodes.
            let sessions = std::mem::take(&mut cs.screencasts);
            if !sessions.is_empty() {
                let mut surfaces = HashSet::new();
                for session in sessions.into_values() {
                    surfaces.extend(session.streams.iter().map(|stream| stream.surface_id));
                    if let Some(bus) = cs.desktop_bus.as_ref() {
                        let _ = bus.try_command(yas_desktop::Command::PortalSessionClosed(
                            session.session_id,
                        ));
                    }
                }
                for surface_id in surfaces {
                    let _ = cs
                        .handle
                        .command_tx
                        .send(CompositorCommand::SetScreenCastActive {
                            surface_id,
                            active: false,
                        });
                }
                cs.handle.wake();
                cs.frame_clocks_dirty = true;
                audio_screencasts_closed = true;
            }
        }
        const RESTART_COOLDOWN: Duration = Duration::from_secs(5);
        let can_restart = cs.audio_restart_needed
            && !cs.audio_restart_inflight
            && cs.audio_pipeline.is_none()
            && cs.desktop_bus.is_some()
            && cs
                .last_audio_restart
                .is_none_or(|t| now.duration_since(t) >= RESTART_COOLDOWN);
        if can_restart {
            cs.last_audio_restart = Some(now);
            cs.audio_restart_inflight = true;
            let runtime_dir = std::path::Path::new(&cs.handle.socket_name)
                .parent()
                .unwrap_or(std::path::Path::new("/tmp"))
                .to_path_buf();
            let session_id = cs.audio_session_id;
            let epoch = cs.created_at;
            let dbus_address = cs
                .desktop_bus
                .as_ref()
                .map(|bus| bus.address().to_owned())
                .unwrap();
            let verbose = state.config.verbose;
            // Reuse the existing broadcast so currently-subscribed clients
            // pick up frames from the restarted pipeline without re-subscribing.
            let broadcast = cs.audio_broadcast.clone();
            let state = state.clone();
            eprintln!("[audio] pipeline unavailable, restarting...");
            tokio::spawn(async move {
                // Drop the old runtime before recreating its directory. This
                // happens off the delivery lock because teardown waits on
                // child processes.
                drop(dead_pipeline);
                let pipeline = tokio::task::block_in_place(|| {
                    audio::AudioPipeline::spawn(
                        &runtime_dir,
                        session_id,
                        &dbus_address,
                        audio_restart_bitrate,
                        verbose,
                        epoch,
                        broadcast,
                    )
                });
                let mut sess = state.session.lock().await;
                let Some(cs) = sess.compositor.as_mut() else {
                    return;
                };
                // Only install if this is still the same live service bundle.
                if cs.audio_session_id != session_id || cs.created_at != epoch {
                    return;
                }
                cs.audio_restart_inflight = false;
                if cs.desktop_bus.is_none() {
                    cs.audio_restart_needed = false;
                    drop(sess);
                    drop(pipeline);
                    return;
                }
                match pipeline {
                    Ok(p) => {
                        eprintln!(
                            "[audio] pipeline restarted, PULSE_SERVER={}",
                            p.pulse_server_path(),
                        );
                        cs.audio_pipeline = Some(p);
                        cs.audio_restart_needed = false;
                    }
                    Err(e) => {
                        cs.audio_restart_needed = true;
                        eprintln!("[audio] failed to restart pipeline: {e}");
                    }
                }
                drop(sess);
                state.delivery_notify.notify_one();
            });
        } else {
            if let Some(dead_pipeline) = dead_pipeline {
                tokio::task::spawn_blocking(move || drop(dead_pipeline));
            }
            if cs.audio_restart_needed && !cs.audio_restart_inflight {
                let retry_at = cs
                    .last_audio_restart
                    .map_or(now, |last| last + RESTART_COOLDOWN);
                next_deadline = Some(next_deadline.map_or(retry_at, |due| due.min(retry_at)));
            }
        }
    }
    #[cfg(target_os = "linux")]
    if !audio_media_revoked.is_empty() {
        for (owner, revoked) in &audio_media_revoked {
            if let Some(cs) = sess.compositor.as_mut() {
                let queue = cs.native_media_input_events.entry(*owner).or_default();
                if queue.len() >= 64 {
                    queue.pop_front();
                }
                queue.push_back(NativeMediaInputEvent::Revoked(*revoked));
            }
        }
    }
    #[cfg(target_os = "linux")]
    let _ = (audio_screencasts_closed, audio_runtime_changed);

    // Retire size claims whose grace ran out, and re-mediate what they were
    // holding.  Nothing else would notice: the viewer that left is silent by
    // definition, and the ones still watching have no reason to re-offer a
    // size their own pane never changed.
    if let Some(due) =
        sess.expire_surface_claims(now, &state.config.surface_encoders, state.config.verbose)
    {
        next_deadline = Some(next_deadline.map_or(due, |d: Instant| d.min(due)));
    }

    // Dispatch resizes whose settle window closed, and park until the next
    // one comes due.  Done last so sizes armed earlier in this same tick —
    // `receilinged_surfaces` after an encoder is created — are accounted for.
    if let Some(cs) = sess.compositor.as_mut()
        && let Some(due) = cs.flush_due_resizes(now)
    {
        next_deadline = Some(next_deadline.map_or(due, |d: Instant| d.min(due)));
    }

    // Guarantee the tick loop wakes up at least every blanket interval
    // even when other time-based work isn't pending.  When no client is
    // connected the interval is `None` and the loop sleeps purely on
    // delivery_notify, so a truly-idle server consumes ~zero CPU until
    // a client connects or the compositor emits an event.
    if let Some(interval) = blanket_frame_interval(&sess) {
        let blanket_deadline = now + interval;
        next_deadline = Some(next_deadline.map_or(blanket_deadline, |d| d.min(blanket_deadline)));
    }

    yas_event!(state.events, EventType::TickStop, {
        let mut payload = (tick_started.elapsed().as_nanos().min(u64::MAX as u128) as u64)
            .to_le_bytes()
            .to_vec();
        payload.extend_from_slice(&(sess.clients.len() as u32).to_le_bytes());
        payload.extend_from_slice(&(sess.ptys.len() as u32).to_le_bytes());
        payload
    });
    TickOutcome { next_deadline }
}

/// Admit a connection on the YAS listener. Connection limits and orderly
/// shutdown are process-wide; each connection owns its protocol state.
fn spawn_yas_client<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    state: AppState,
    origin: ConnectionOrigin,
) -> bool {
    let cancellation = ConnectionCancellation::default();
    spawn_yas_session(stream, state, cancellation, None, origin)
}

fn spawn_yas_composite_client<Main, Datagram>(
    main: Main,
    datagram: Datagram,
    max_datagram: u32,
    state: AppState,
    origin: ConnectionOrigin,
) -> bool
where
    Main: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    Datagram: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let cancellation = ConnectionCancellation::default();
    let datagram = composite_link::DatagramLink::open(datagram, max_datagram, cancellation.clone());
    spawn_yas_session(main, state, cancellation, Some(datagram), origin)
}

fn spawn_yas_session<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    state: AppState,
    cancellation: ConnectionCancellation,
    datagram: Option<composite_link::DatagramLink>,
    origin: ConnectionOrigin,
) -> bool {
    let registration = state
        .connections
        .register(cancellation.clone())
        .or_else(|| {
            state
                .yas_shutdown
                .is_scheduled()
                .then(|| {
                    state
                        .connections
                        .register_shutdown_retry(cancellation.clone())
                })
                .flatten()
        });
    let Some(registration) = registration else {
        return false;
    };
    let max = state.config.max_connections;
    let admitted = state
        .active_connections
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (max == 0 || current < max).then_some(current + 1)
        })
        .is_ok();
    if !admitted {
        eprintln!("max connections ({max}) reached, rejecting YAS session");
        return false;
    }
    tokio::spawn(async move {
        yas::serve_stream(
            stream,
            state.clone(),
            cancellation.clone(),
            registration,
            datagram,
            origin,
        )
        .await;
        cancellation.cancel();
        state.active_connections.fetch_sub(1, Ordering::AcqRel);
    });
    true
}

#[cfg(test)]
mod tests {
    use super::{DeploymentOverrides, DeploymentSettings};

    #[cfg(unix)]
    pub(crate) mod process_transport {
        use super::super::*;

        /// Minimal process-wide state for native YAS integration tests. This
        /// deliberately constructs no retired packet endpoint.
        pub(crate) fn test_state(process_server: process::Server) -> AppState {
            let name = ServerName::default();
            Arc::new(AppStateInner {
                config: Config {
                    name: name.clone(),
                    shell: "/bin/sh".into(),
                    shell_flags: String::new(),
                    scrollback: 100,
                    ipc_path: String::new(),
                    ipc_path_is_automatic: false,
                    automatic_ipc_template: default_ipc_path_template(),
                    surface_encoders: Vec::new(),
                    surface_encoding: SurfaceEncoding::default(),
                    chroma: ChromaSubsampling::default(),
                    media_codecs: MediaCodecPolicy::default(),
                    vaapi_device: String::new(),
                    fd_channel: None,
                    verbose: false,
                    processes: true,
                    max_connections: 0,
                    max_ptys: 0,
                    ping_interval: Duration::ZERO,
                    skip_compositor: true,
                    export_sock: false,
                    inject_path: false,
                    allow_forward: Vec::new(),
                    allow_forward_insecure: false,
                    allow_persistent_extensions: false,
                },
                events: events::EventLog::new(
                    events::DEFAULT_RING_SIZE,
                    yas_wire::events::ActivationSet::default(),
                ),
                process_server,
                boot_generation: 1,
                session: Mutex::new(Session::new_with_boot_generation(1)),
                pty_fds: Arc::new(std::sync::RwLock::new(FxHashMap::default())),
                delivery_notify: Arc::new(Notify::new()),
                shutdown_notify: Arc::new(Notify::new()),
                hosted_shutdown: Arc::new(Notify::new()),
                connections: Arc::new(ConnectionRegistry::default()),
                yas_shutdown: Arc::new(yas_shutdown::Coordinator::default()),
                supervisor_notify: Arc::new(Notify::new()),
                active_connections: AtomicUsize::new(0),
                extensions: extension::ExtensionService::from_env(false, &name),
                fonts: font::Service::disabled_for_test(),
                relay: relay::Service::disabled_for_test(),
                selection: yas::SelectionStore::new(),
                diagnostics: Arc::new(ServerDiagnosticsRegistry::default()),
            })
        }
    }

    fn deployment_env<'a>(
        entries: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Result<Option<String>, String> + 'a {
        move |name| {
            Ok(entries
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| (*value).to_owned())))
        }
    }

    #[test]
    fn deployment_cli_capacity_overrides_invalid_environment_value() {
        let mut overrides = DeploymentOverrides::default();
        overrides.set("YAS_EXT_MAX_RUNNING", 3).unwrap();
        let settings = DeploymentSettings::resolve_with(
            overrides,
            deployment_env(&[("YAS_EXT_MAX_RUNNING", "99")]),
        )
        .unwrap();
        assert_eq!(settings.values["YAS_EXT_MAX_RUNNING"], 3);
    }

    #[test]
    fn deployment_family_flags_and_environment_are_resolved_once() {
        let mut overrides = DeploymentOverrides::default();
        overrides.disable_extensions();
        overrides.disable_channels();
        let settings = DeploymentSettings::resolve_with(
            overrides,
            deployment_env(&[("YAS_EXT", "1"), ("YAS_CHANNEL", "1")]),
        )
        .unwrap();
        assert!(!settings.extensions_enabled);
        assert!(!settings.channels_enabled);

        let settings = DeploymentSettings::resolve_with(
            DeploymentOverrides::default(),
            deployment_env(&[("YAS_EXT", "0"), ("YAS_CHANNEL", "0")]),
        )
        .unwrap();
        assert!(!settings.extensions_enabled);
        assert!(!settings.channels_enabled);
    }

    #[test]
    fn deployment_max_running_is_strictly_validated() {
        for invalid in [0, 5, u64::MAX] {
            let mut overrides = DeploymentOverrides::default();
            overrides.set("YAS_EXT_MAX_RUNNING", invalid).unwrap();
            let error =
                DeploymentSettings::resolve_with(overrides, deployment_env(&[])).unwrap_err();
            assert!(error.contains("YAS_EXT_MAX_RUNNING must be in 1..=4"));
        }
        for valid in 1..=4 {
            let mut overrides = DeploymentOverrides::default();
            overrides.set("YAS_EXT_MAX_RUNNING", valid).unwrap();
            DeploymentSettings::resolve_with(overrides, deployment_env(&[])).unwrap();
        }
    }

    mod audio_queue_bound {
        use super::super::AUDIO_QUEUE_MAX_FRAMES;
        use tokio::sync::mpsc;

        #[test]
        fn a_stalled_client_retains_at_most_the_hard_frame_cap() {
            let (tx, mut rx) = mpsc::channel(AUDIO_QUEUE_MAX_FRAMES);
            for index in 0..AUDIO_QUEUE_MAX_FRAMES {
                tx.try_send(vec![index as u8]).unwrap();
            }
            for index in 0..100_000 {
                assert!(tx.try_send(vec![index as u8]).is_err());
            }
            assert_eq!(rx.len(), AUDIO_QUEUE_MAX_FRAMES);
            for index in 0..AUDIO_QUEUE_MAX_FRAMES {
                assert_eq!(rx.try_recv().unwrap(), vec![index as u8]);
            }
        }
    }

    /// Nothing between the browser and the app inspects the button code —
    /// the compositor forwards the `u32` to `wl_pointer` verbatim — so this
    /// table is the only place a wrong number can be caught. A mistake here
    /// is silent: the press still arrives, as the wrong button.
    mod evdev_buttons {
        use super::super::evdev_button;

        #[test]
        fn the_three_common_buttons_keep_their_codes() {
            assert_eq!(evdev_button(0), 0x110);
            assert_eq!(evdev_button(1), 0x112);
            assert_eq!(evdev_button(2), 0x111);
        }

        #[test]
        fn back_and_forward_are_the_codes_a_real_mouse_sends() {
            // Not BTN_BACK/BTN_FORWARD (0x116/0x115): toolkits bind the
            // thumb buttons through BTN_SIDE/BTN_EXTRA.
            assert_eq!(evdev_button(3), 0x113);
            assert_eq!(evdev_button(4), 0x114);
        }

        #[test]
        fn an_unknown_button_falls_back_to_left() {
            assert_eq!(evdev_button(5), 0x110);
            assert_eq!(evdev_button(255), 0x110);
        }
    }

    /// Pin the representation arbitration for one downscale target. Mixed
    /// CPU/NVENC readers receive both BGRA and opaque NV12; NVENC readers
    /// must still agree on the opaque chroma layout.
    mod nv12_opaque_target {
        use super::super::{DownscaleTargetMode, downscale_target_mode};

        const T: (u32, u32) = (1280, 720);
        const OPAQUE_420: DownscaleTargetMode = DownscaleTargetMode {
            want_nv12_opaque: true,
            want_cpu_pixels: false,
            opaque_is_444: false,
        };
        const OPAQUE_444: DownscaleTargetMode = DownscaleTargetMode {
            want_nv12_opaque: true,
            want_cpu_pixels: false,
            opaque_is_444: true,
        };
        const MIXED_420: DownscaleTargetMode = DownscaleTargetMode {
            want_nv12_opaque: true,
            want_cpu_pixels: true,
            opaque_is_444: false,
        };
        const CPU_ONLY: DownscaleTargetMode = DownscaleTargetMode {
            want_nv12_opaque: false,
            want_cpu_pixels: true,
            opaque_is_444: false,
        };

        #[test]
        fn sole_nvenc_subscriber_gets_it() {
            assert_eq!(
                downscale_target_mode(true, false, false, T, std::iter::empty()),
                OPAQUE_420
            );
        }

        #[test]
        fn mixed_cpu_and_nvenc_subscribers_get_both_representations() {
            assert_eq!(
                downscale_target_mode(
                    true,
                    false,
                    false,
                    T,
                    [(Some(T), false, false, true)].into_iter()
                ),
                MIXED_420
            );
        }

        #[test]
        fn all_nvenc_subscribers_keep_it() {
            assert_eq!(
                downscale_target_mode(
                    true,
                    false,
                    false,
                    T,
                    [(Some(T), true, false, false), (Some(T), true, false, false)].into_iter()
                ),
                OPAQUE_420
            );
        }

        #[test]
        fn a_dissenter_at_another_size_is_irrelevant() {
            // It reads its own (sid, w, h) key, which still carries BGRA.
            assert_eq!(
                downscale_target_mode(
                    true,
                    false,
                    false,
                    T,
                    [
                        (Some((640, 360)), false, false, true),
                        (None, false, false, true)
                    ]
                    .into_iter()
                ),
                OPAQUE_420
            );
        }

        #[test]
        fn one_cpu_reader_among_many_keeps_both_representations() {
            assert_eq!(
                downscale_target_mode(
                    true,
                    false,
                    false,
                    T,
                    [
                        (Some(T), true, false, false),
                        (Some(T), false, false, true),
                        (Some(T), true, false, false)
                    ]
                    .into_iter()
                ),
                MIXED_420
            );
        }

        #[test]
        fn arbitration_is_independent_of_which_subscriber_triggered_it() {
            assert_eq!(
                downscale_target_mode(
                    false,
                    false,
                    true,
                    T,
                    [(Some(T), true, false, false)].into_iter()
                ),
                MIXED_420
            );
        }

        #[test]
        fn cpu_only_subscribers_need_no_opaque_buffer() {
            assert_eq!(
                downscale_target_mode(
                    false,
                    false,
                    true,
                    T,
                    [(Some(T), false, false, true)].into_iter()
                ),
                CPU_ONLY
            );
        }

        #[test]
        fn a_chroma_format_split_rules_it_out() {
            // One session needs NV12, the other planar YUV444 — one
            // shared buffer cannot serve both layouts.
            assert_eq!(
                downscale_target_mode(
                    true,
                    true,
                    false,
                    T,
                    [(Some(T), true, false, false)].into_iter()
                ),
                CPU_ONLY
            );
        }

        #[test]
        fn matching_444_subscribers_keep_it() {
            assert_eq!(
                downscale_target_mode(
                    true,
                    true,
                    false,
                    T,
                    [(Some(T), true, true, false)].into_iter()
                ),
                OPAQUE_444
            );
        }

        #[test]
        fn vulkan_only_target_needs_no_published_pixels() {
            assert_eq!(
                downscale_target_mode(false, false, false, T, std::iter::empty()),
                DownscaleTargetMode {
                    want_nv12_opaque: false,
                    want_cpu_pixels: false,
                    opaque_is_444: false,
                }
            );
        }
    }
    use super::*;

    #[test]
    fn vulkan_video_accepts_a_scaled_client_target() {
        let preferences = [SurfaceEncoderPreference::VulkanVideoAV1];
        assert!(vulkan_video_tier_eligible(&preferences, 0, 1280, 720));
    }

    #[test]
    fn server_creation_does_not_jump_from_dedicated_engines_over_vulkan() {
        use SurfaceEncoderPreference as P;
        let preferences = [P::NvencAV1, P::AV1Vaapi, P::VulkanVideoAV1, P::AV1Software];
        assert_eq!(
            server_creation_preferences(&preferences, false),
            (vec![P::NvencAV1, P::AV1Vaapi], true)
        );
        assert_eq!(
            server_creation_preferences(&preferences, true),
            (preferences.to_vec(), false)
        );
    }

    #[test]
    fn vulkan_444_refusal_retries_the_same_codec_at_420() {
        let pref = SurfaceEncoderPreference::VulkanVideoAV1;
        let mut declined = HashSet::new();
        assert!(vulkan_encoder_chroma(pref, true, 0, &declined));

        let mut sub = SurfaceSubState::default();
        latch_vulkan_refusal(&mut sub, pref, true, 102, 64);
        assert_eq!(sub.vulkan_refused, 0, "4:4:4 must not reject AV1 Vulkan");
        assert_ne!(sub.vulkan_444_refused, 0);
        assert!(!vulkan_encoder_chroma(
            pref,
            true,
            sub.vulkan_444_refused,
            &declined
        ));

        // A device-wide 4:4:4 encode failure has the same profile-specific
        // fallback behavior.
        declined.insert(vulkan_encoder_name(pref, true));
        assert!(!vulkan_encoder_chroma(pref, true, 0, &declined));

        // A 4:2:0 refusal exhausts the codec only for this subscription; it
        // is deliberately never inserted into the device-wide set.
        latch_vulkan_refusal(&mut sub, pref, false, 102, 64);
        assert_ne!(sub.vulkan_refused, 0);
        assert!(!declined.contains(vulkan_encoder_name(pref, false)));
    }

    #[test]
    fn vulkan_refusal_is_scoped_to_the_encoded_extent() {
        let pref = SurfaceEncoderPreference::VulkanVideoAV1;
        let mut sub = SurfaceSubState::default();

        latch_vulkan_refusal(&mut sub, pref, false, 102, 64);
        assert_ne!(
            vulkan_refusals_for_extent(&mut sub, 102, 64).0,
            0,
            "the failed thumbnail extent must not retry every tick"
        );
        assert_eq!(
            vulkan_refusals_for_extent(&mut sub, 684, 1064),
            (0, 0),
            "a full-size target must not inherit the thumbnail's refusal"
        );
        assert_eq!(sub.vulkan_refused_extent, Some((684, 1064)));
    }

    fn test_client_with_capacity(
        _capacity: usize,
    ) -> (ClientState, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (_raw_tx, rx) = mpsc::unbounded_channel();
        let client = ClientState {
            write_blocked_us: Arc::new(AtomicU64::new(0)),
            write_blocked_us_seen: 0,
            outbound_bytes: Arc::new(AtomicU64::new(0)),
            outbound_bytes_seen: 0,
            outbound_sampled_at: Instant::now(),
            outbound_bytes_per_sec: 0,
            inbound_bytes: Arc::new(AtomicU64::new(0)),
            inbound_bytes_seen: 0,
            inbound_sampled_at: Instant::now(),
            inbound_bytes_per_sec: 0,
            connected_at: Instant::now(),
            origin: ConnectionOrigin::Network,
            catalog_visible: true,
            native_identity: None,
            native_surface: None,
            lead: None,
            subscriptions: FxHashSet::default(),
            surface_subscriptions: FxHashSet::default(),
            view_sizes: FxHashMap::default(),
            scroll_offsets: FxHashMap::default(),
            scroll_caches: FxHashMap::default(),
            last_sent: FxHashMap::default(),
            last_used_rows_sent: FxHashMap::default(),
            preview_next_send_at: FxHashMap::default(),
            rtt_ms: 50.0,
            min_rtt_ms: 50.0,
            display_fps: 60.0,
            delivery_bps: 262_144.0,
            goodput_bps: 262_144.0,
            goodput_jitter_bps: 0.0,
            max_goodput_jitter_bps: 0.0,
            last_goodput_sample_bps: 0.0,
            avg_frame_bytes: 1_024.0,
            avg_paced_frame_bytes: 1_024.0,
            avg_preview_frame_bytes: 1_024.0,
            avg_surface_frame_bytes: 8_192.0,
            inflight_bytes: 0,
            inflight_frames: VecDeque::new(),
            next_send_at: Instant::now(),
            probe_frames: 0.0,
            frames_sent: 0,
            acks_recv: 0,
            acked_bytes_since_log: 0,
            browser_backlog_frames: 0,
            browser_ack_ahead_frames: 0,
            browser_apply_ms: 0.0,
            last_log: Instant::now(),
            last_window_blocked_log: Instant::now(),
            last_skip_log: Instant::now(),
            skip_same_gen_count: 0,
            skip_in_flight_count: 0,
            skip_pacing_count: 0,
            skip_vulkan_await_count: 0,
            skip_no_subs_count: 0,
            skip_not_subbed_count: 0,
            skip_last_pixels_mismatch_count: 0,
            encode_loop_iters: 0,
            goodput_window_bytes: 0,
            goodput_window_start: Instant::now(),
            surface_goodput_bps: 262_144.0,
            surface_goodput_window_bytes: 0,
            surface_goodput_window_start: Instant::now(),
            surface_subs: FxHashMap::default(),
            surface_inflight_frames: VecDeque::new(),
            surface_inflight_bytes: 0,
            surface_schedule_cursor: None,
            vulkan_video_surfaces: FxHashMap::default(),
            surface_view_sizes: FxHashMap::default(),
            surface_claim_lapses: FxHashMap::default(),
            surface_codec_support: 0,
            surface_max_decode: (0, 0),
            pressed_surface_keys: HashSet::new(),
            direct_touch_enabled: false,
            surface_touch_ids: HashMap::new(),
        };
        (client, rx)
    }

    fn test_client() -> ClientState {
        let (client, _rx) = test_client_with_capacity(0);
        client
    }

    #[cfg(target_os = "linux")]
    fn mpris_player(position_us: i64, length_us: i64) -> yas_desktop::MprisPlayer {
        yas_desktop::MprisPlayer {
            player_id: 1,
            revision: 1,
            track_revision: 1,
            active: true,
            playback_status: yas_desktop::PlaybackStatus::Playing,
            loop_status: yas_desktop::LoopStatus::None,
            shuffle: false,
            capability_flags: 0,
            rate_ppm: 1_000_000,
            minimum_rate_ppm: 1_000_000,
            maximum_rate_ppm: 1_000_000,
            volume_ppm: 1_000_000,
            position_us,
            length_us,
            identity: String::new(),
            desktop_entry: String::new(),
            title: String::new(),
            album: String::new(),
            artists: Vec::new(),
            artwork: yas_desktop::MprisArtwork::None,
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cached_mpris_replay_advances_from_observation_and_clamps() {
        let now = Instant::now();
        let observed_at = now - Duration::from_secs(2);
        let mut player = mpris_player(1_000_000, 4_000_000);
        player.rate_ppm = 2_000_000;

        let replay = mpris_player_at(&player, observed_at, now);
        assert_eq!(replay.position_us, 4_000_000);
        assert_eq!(player.position_us, 1_000_000);

        player.rate_ppm = -1_000_000;
        let replay = mpris_player_at(&player, observed_at, now);
        assert_eq!(replay.position_us, 0);

        player.playback_status = yas_desktop::PlaybackStatus::Paused;
        player.position_us = 5_000_000;
        assert_eq!(
            mpris_player_at(&player, observed_at, now).position_us,
            4_000_000
        );
    }

    fn force_decoder_pressure(client: &mut ClientState, surface_id: u16, depth: u8) {
        let sub = client.surface_subs.entry(surface_id).or_default();
        sub.decoder_queue_depth = depth;
        sub.decoder_pressure_depth = depth;
    }

    /// URI encoding used by native Surface drag/drop.
    mod drag {
        use super::super::percent_encode_uri_path;

        #[test]
        fn uri_path_encoding_leaves_unreserved_and_slashes() {
            assert_eq!(
                percent_encode_uri_path("/tmp/yas_drag_1_2/plain-~ok.txt"),
                "/tmp/yas_drag_1_2/plain-~ok.txt"
            );
        }

        #[test]
        fn uri_path_encoding_escapes_spaces_and_non_ascii() {
            assert_eq!(percent_encode_uri_path("/tmp/a b.png"), "/tmp/a%20b.png");
            assert_eq!(
                percent_encode_uri_path("/tmp/fötö 🚀.png"),
                "/tmp/f%C3%B6t%C3%B6%20%F0%9F%9A%80.png"
            );
            assert_eq!(percent_encode_uri_path("/tmp/#?%.x"), "/tmp/%23%3F%25.x");
        }
    }

    fn fill_inflight(client: &mut ClientState, frames: usize, bytes_per_frame: usize) {
        let now = Instant::now();
        client.inflight_bytes = frames.saturating_mul(bytes_per_frame);
        client.inflight_frames = (0..frames)
            .map(|_| InFlightFrame {
                sent_at: now,
                bytes: bytes_per_frame,
                paced: true,
            })
            .collect();
    }

    fn sample_frame(text: &str) -> FrameState {
        let mut frame = FrameState::new(2, 8);
        frame.write_text(0, 0, text, yas_terminal_model::CellStyle::default());
        frame
    }

    #[test]
    fn unset_view_size_accepts_zero_pair_only() {
        assert!(is_unset_view_size(0, 0));
        assert!(!is_unset_view_size(0, 80));
        assert!(!is_unset_view_size(u16::MAX, u16::MAX));
    }

    #[test]
    fn unsubscribe_client_from_clears_view_size() {
        let mut client = test_client();
        client.subscriptions.insert(7);
        client.view_sizes.insert(7, (24, 80));
        assert!(unsubscribe_client_from(&mut client, 7));
        assert!(!client.subscriptions.contains(&7));
        assert!(!client.view_sizes.contains_key(&7));
    }

    #[test]
    fn mediated_size_uses_per_pty_view_sizes_without_lead() {
        let mut session = Session::new();
        let mut c1 = test_client();
        let mut c2 = test_client();
        c1.view_sizes.insert(7, (30, 120));
        c2.view_sizes.insert(7, (24, 100));
        session.clients.insert(1, c1);
        session.clients.insert(2, c2);
        assert_eq!(session.mediated_size_for_pty(7), Some((24, 100)));
    }

    /// The first resize of a surface goes out immediately. Waiting out the
    /// settle window first would add its full latency to every isolated
    /// resize — a pane opening, a one-shot `yas surface capture --width`,
    /// the first frame of a drag — for no coalescing benefit, since there is
    /// nothing yet to coalesce with.
    #[test]
    fn first_surface_resize_dispatches_immediately() {
        assert_eq!(
            resize_action(None, None, Instant::now(), (800, 600, 120)),
            ResizeAction::Dispatch
        );
    }

    /// Everything arriving while the window is open is held, so a drag costs
    /// one configure (and one encoder rebuild, hence one keyframe) per
    /// window instead of one per frame.
    #[test]
    fn surface_resize_inside_the_settle_window_is_held() {
        let t0 = Instant::now();
        assert_eq!(
            resize_action(
                Some((800, 600, 120)),
                Some(t0),
                t0 + SURFACE_RESIZE_SETTLE / 2,
                (810, 600, 120),
            ),
            ResizeAction::Hold
        );
    }

    /// A couple of pixels is BSP settle noise, not a resize: the browser
    /// and the compositor round each other by ±2px, and configuring for
    /// it would tear down every compositor-resident session on the
    /// surface per nudge.
    #[test]
    fn surface_resize_nudge_is_ignored() {
        let t0 = Instant::now();
        assert_eq!(
            resize_action(
                Some((800, 600, 120)),
                Some(t0),
                t0 + SURFACE_RESIZE_SETTLE,
                (802, 598, 120),
            ),
            ResizeAction::Ignore
        );
        // A scale change is never a nudge.
        assert_eq!(
            resize_action(
                Some((800, 600, 120)),
                Some(t0),
                t0 + SURFACE_RESIZE_SETTLE,
                (802, 598, 240),
            ),
            ResizeAction::Dispatch
        );
    }

    /// Once the window closes the next resize dispatches on arrival, so a
    /// sustained drag tracks at one configure per window rather than
    /// freezing until the user lets go.
    #[test]
    fn surface_resize_after_the_settle_window_dispatches() {
        let t0 = Instant::now();
        assert_eq!(
            resize_action(
                Some((800, 600, 120)),
                Some(t0),
                t0 + SURFACE_RESIZE_SETTLE,
                (810, 600, 120),
            ),
            ResizeAction::Dispatch
        );
    }

    /// The same thing against a live compositor rather than the policy
    /// function: the leading resize reaches it on arrival, a burst behind it
    /// collapses to a single held size rather than a queue, and closing the
    /// window delivers that one size.
    ///
    /// Multi-threaded because the compositor itself owns worker threads.
    #[tokio::test(flavor = "multi_thread")]
    async fn surface_resize_burst_collapses_to_one_configure() {
        let mut session = Session::new();
        session.ensure_compositor(false, Arc::new(|| {}), "");

        #[cfg(target_os = "linux")]
        {
            let compositor = session.compositor.as_ref().unwrap();
            assert!(compositor.xwayland.is_none());
            assert!(compositor.desktop_bus.is_none());
            assert!(compositor.audio_pipeline.is_none());
        }

        // Leading edge: out at once, nothing held.
        assert!(session.resize_surface(1, 800, 600, 120));
        let cs = session.compositor.as_mut().unwrap();
        assert_eq!(cs.last_configured_size.get(&1), Some(&(800, 600, 120)));
        assert!(cs.pending_resize.is_empty());

        // A drag's worth of sizes behind it, all inside the window.
        for w in 801..=850 {
            assert!(!session.resize_surface(1, w, 600, 120));
        }
        let cs = session.compositor.as_mut().unwrap();
        assert_eq!(cs.pending_resize.get(&1), Some(&(850, 600, 120)));
        assert_eq!(
            cs.last_configured_size.get(&1),
            Some(&(800, 600, 120)),
            "the compositor must still be on the leading-edge size"
        );

        // Still inside the window: nothing goes out, and the caller is told
        // when to come back.
        let opened_at = cs.last_resize_at[&1];
        let due = cs.flush_due_resizes(opened_at);
        assert_eq!(due, Some(opened_at + SURFACE_RESIZE_SETTLE));
        assert_eq!(cs.last_configured_size.get(&1), Some(&(800, 600, 120)));

        // Window closed: the last size of the drag goes out, once.
        assert_eq!(
            cs.flush_due_resizes(opened_at + SURFACE_RESIZE_SETTLE),
            None
        );
        assert_eq!(cs.last_configured_size.get(&1), Some(&(850, 600, 120)));
        assert!(cs.pending_resize.is_empty());
    }

    /// A surface on its way to another size says so, so the encode path can
    /// decline to build for the size it is leaving.  Restoring a parked
    /// thumbnail into a pane is the case that hurts: the subscribe that wants
    /// pane resolution and the resize that gets the surface there arrive
    /// together, and an encoder for the old native in between is ~150 ms of
    /// NVENC init whose every frame is the wrong size.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_surface_awaiting_a_configure_names_where_it_is_going() {
        let mut session = Session::new();
        session.ensure_compositor(false, Arc::new(|| {}), "");

        let cs = session.compositor.as_ref().unwrap();
        let now = Instant::now();
        assert_eq!(
            cs.resize_destination(1, now),
            None,
            "a surface nobody has resized is already where it belongs"
        );

        // On the wire, unanswered: that is where the surface is going.
        assert!(session.resize_surface(1, 800, 600, 120));
        let cs = session.compositor.as_ref().unwrap();
        let sent_at = cs.last_resize_at[&1];
        assert_eq!(cs.resize_destination(1, sent_at), Some((800, 600, 120)));

        // Held for the settle window counts the same, and supersedes the one
        // on the wire — it is the newer answer.
        assert!(!session.resize_surface(1, 640, 480, 120));
        let cs = session.compositor.as_ref().unwrap();
        assert_eq!(
            cs.resize_destination(1, sent_at + SURFACE_RESIZE_SETTLE / 2),
            Some((640, 480, 120))
        );

        // A client that never acks its configure must not be able to hold
        // its viewers on a frozen picture.
        assert_eq!(
            cs.resize_destination(1, sent_at + RESIZE_ENCODER_GRACE),
            None
        );
    }

    /// And once the compositor reports the new size, waiting is over —
    /// otherwise every surface would stall for the whole grace window after
    /// each resize.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_resized_surface_stops_naming_a_destination() {
        let mut session = Session::new();
        session.ensure_compositor(false, Arc::new(|| {}), "");
        assert!(session.resize_surface(1, 800, 600, 120));

        let cs = session.compositor.as_mut().unwrap();
        let sent_at = cs.last_resize_at[&1];
        assert!(cs.resize_destination(1, sent_at).is_some());
        // What the SurfaceResized arm does when the compositor answers.
        cs.resize_inflight.remove(&1);
        assert_eq!(cs.resize_destination(1, sent_at), None);
    }

    /// Asking for the size the compositor was last given is a no-op whether
    /// or not the window is open — and it must beat the window check, so a
    /// drag that returns to its starting size clears the held intermediate
    /// instead of configuring to it after the fact.
    #[test]
    fn surface_resize_to_the_current_size_is_ignored() {
        let t0 = Instant::now();
        for now in [t0 + SURFACE_RESIZE_SETTLE / 2, t0 + SURFACE_RESIZE_SETTLE] {
            assert_eq!(
                resize_action(Some((800, 600, 120)), Some(t0), now, (800, 600, 120)),
                ResizeAction::Ignore
            );
        }
    }

    /// A lone viewer must get back the size it asked for, rounded only onto
    /// the even 4:2:0 grid — never grown. The logical round trip does not
    /// give that: at 2× an odd physical extent comes back one pixel *larger*
    /// (1001 → 501 → 1002), so the surface was a pixel bigger than the pane,
    /// `per_client_encode_target` inscribed the native aspect into the
    /// smaller viewport, and the leftover showed as a letterbox bar. Tiled
    /// panes have fractional CSS widths, so odd physical extents are the
    /// common case rather than the corner one.
    #[test]
    fn mediated_surface_size_is_exact_for_one_viewer() {
        for &(w, h) in &[(1001u16, 563u16), (1000, 562), (1003, 999), (777, 1155)] {
            for &scale in &[120u16, 180, 240, 300] {
                let mut session = Session::new();
                let mut c = test_client();
                c.surface_subscriptions.insert(1);
                c.surface_view_sizes.insert(1, (w, h, scale));
                session.clients.insert(1, c);
                assert_eq!(
                    session.mediated_size_for_surface(1, &[]),
                    Some((w & !1, h & !1, scale.max(120))),
                    "one viewer at {w}x{h} scale={scale} must get its own size back on the even grid"
                );
            }
        }
    }

    /// Wayland outputs cannot advertise less than 1×. A sub-1× viewer is
    /// therefore represented by a larger logical window at the 1× output
    /// floor; its per-client encoder downsamples that window back into the
    /// viewer's physical pane.
    #[test]
    fn mediated_surface_size_supports_sub_1x_viewers() {
        let mut session = Session::new();
        let mut c = test_client();
        c.surface_subscriptions.insert(1);
        c.surface_view_sizes.insert(1, (800, 600, 60));
        session.clients.insert(1, c);

        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1600, 1200, 120))
        );
        assert_eq!(
            Session::per_client_encode_target(
                Some((800, 600, 60)),
                1600,
                1200,
                Some((1600, 1200)),
                None,
            ),
            (800, 600)
        );
    }

    /// The encoder ceiling limits the downscaled frame sent to the viewer,
    /// not the larger 1x source needed to give a sub-1x window its logical
    /// area. At 25%, a lone full-HD pane should therefore stay full-HD on
    /// screen while seeing a full 7680x4320 logical window.
    #[test]
    fn sub_1x_lone_viewer_is_not_shrunk_by_the_encoder_ceiling() {
        let mut session = Session::new();
        let mut c = test_client();
        c.surface_subscriptions.insert(1);
        c.surface_view_sizes.insert(1, (1920, 1080, 30));
        session.clients.insert(1, c);

        assert_eq!(
            session.mediated_size_for_surface(1, &[SurfaceEncoderPreference::H264Software]),
            Some((7680, 4320, 120))
        );
        assert_eq!(
            Session::per_client_encode_target(
                Some((1920, 1080, 30)),
                7680,
                4320,
                Some((7680, 4320)),
                Some((3840, 2160)),
            ),
            (1920, 1080)
        );
    }

    /// Mixed scales still go through logical space — there is no single
    /// physical size to preserve — but the client that set the minimum on an
    /// axis is the one whose pixels are honoured when it is at the chosen
    /// scale.
    #[test]
    fn mediated_surface_size_keeps_the_constraining_viewer_exact() {
        let mut session = Session::new();
        let mut c1 = test_client();
        let mut c2 = test_client();
        // c1 is smaller on both axes, so it is the largest logical box that
        // fits both clients.  Its odd physical extent is then rounded down to
        // the encoder grid.
        c1.surface_subscriptions.insert(1);
        c1.surface_view_sizes.insert(1, (1001, 563, 240));
        c2.surface_subscriptions.insert(1);
        c2.surface_view_sizes.insert(1, (2000, 1200, 240));
        session.clients.insert(1, c1);
        session.clients.insert(2, c2);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1000, 562, 240))
        );
    }

    #[test]
    fn mediated_surface_size_picks_fitting_dimensions_max_scale() {
        let mut session = Session::new();
        let mut c1 = test_client();
        let mut c2 = test_client();
        // Client 1: 1920×1080 physical at 2× ⇒ 960×540 logical.
        // Client 2: 1280×720 physical at 1× ⇒ 1280×720 logical.
        // min logical = 960×540, max scale = 240 ⇒ 1920×1080 physical at 240.
        c1.surface_view_sizes.insert(1, (1920, 1080, 240));
        c1.surface_subscriptions.insert(1);
        c2.surface_view_sizes.insert(1, (1280, 720, 120));
        c2.surface_subscriptions.insert(1);
        session.clients.insert(1, c1);
        session.clients.insert(2, c2);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1920, 1080, 240))
        );
    }

    #[test]
    fn mediated_surface_size_same_logical_different_dpr_keeps_logical() {
        // Regression: with the old implementation that took
        // `min(physical), max(scale)` directly, two clients reporting the
        // SAME logical size at different DPRs produced a surface that was
        // half the intended logical size for the lower-DPR client.
        let mut session = Session::new();
        let mut c1 = test_client();
        let mut c2 = test_client();
        // Both clients want the surface at 800×600 logical.
        c1.surface_view_sizes.insert(1, (800, 600, 120)); // 1×
        c1.surface_subscriptions.insert(1);
        c2.surface_view_sizes.insert(1, (1600, 1200, 240)); // 2×
        c2.surface_subscriptions.insert(1);
        session.clients.insert(1, c1);
        session.clients.insert(2, c2);
        // Compositor must render at 800×600 logical (preserved across DPRs).
        // Highest scale wins (240) ⇒ 1600×1200 physical at scale 240.
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1600, 1200, 240))
        );
    }

    #[test]
    fn typed_surface_claims_negotiate_logical_extent_and_density_independently() {
        let mut session = Session::new();
        // 800x600 logical at 2x.
        session
            .native_surface_claims
            .insert(([1; 16], 1), (1600, 1200, 240));
        // 1280x720 logical at 1x. Width comes from the first viewer, height
        // from it too, while density remains the cross-view maximum.
        session
            .native_surface_claims
            .insert(([2; 16], 1), (1280, 720, 120));

        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1600, 1200, 240)),
        );
        assert_eq!(session.surface_scale_120(1), 240);

        session.native_surface_claims.remove(&([1; 16], 1));
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1280, 720, 120)),
        );
    }

    #[test]
    fn compositor_refresh_uses_fastest_connected_client() {
        let mut session = Session::new();
        assert_eq!(session.compositor_refresh_mhz(), 60_000);

        let mut slow = test_client();
        slow.display_fps = 30.0;
        let mut fast = test_client();
        fast.display_fps = 144.0;
        session.clients.insert(1, slow);
        session.clients.insert(2, fast);
        assert_eq!(session.compositor_refresh_mhz(), 144_000);

        session.clients.remove(&2);
        assert_eq!(session.compositor_refresh_mhz(), 30_000);
        session.clients.remove(&1);
        assert_eq!(session.compositor_refresh_mhz(), 60_000);
    }

    #[test]
    fn mediated_surface_size_none_when_no_clients() {
        let session = Session::new();
        assert_eq!(session.mediated_size_for_surface(1, &[]), None);
    }

    #[test]
    fn mediated_surface_size_single_client() {
        let mut session = Session::new();
        let mut c1 = test_client();
        c1.surface_view_sizes.insert(3, (800, 600, 120));
        c1.surface_subscriptions.insert(3);
        session.clients.insert(1, c1);
        assert_eq!(
            session.mediated_size_for_surface(3, &[]),
            Some((800, 600, 120))
        );
    }

    /// Density belongs to the surface, not the session.  A window watched
    /// only at 1× is composited at 1× however many HiDPI viewers are looking
    /// at *other* windows — each toplevel has its own `wl_output`, so there
    /// is no shared number left to drag it around.
    #[test]
    fn mediated_surface_size_keeps_density_per_surface() {
        let mut session = Session::new();
        let mut hidpi = test_client();
        let mut lodpi = test_client();
        hidpi.surface_view_sizes.insert(1, (1920, 1080, 240));
        hidpi.surface_subscriptions.insert(1);
        lodpi.surface_view_sizes.insert(2, (640, 480, 120));
        lodpi.surface_subscriptions.insert(2);
        session.clients.insert(1, hidpi);
        session.clients.insert(2, lodpi);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1920, 1080, 240))
        );
        assert_eq!(
            session.mediated_size_for_surface(2, &[]),
            Some((640, 480, 120)),
            "the 2x viewer of surface 1 has no say in surface 2's density"
        );
        // …and the 1× surface does not change when the HiDPI client leaves,
        // which is what used to move every window in the session.
        session.clients.remove(&1);
        assert_eq!(
            session.mediated_size_for_surface(2, &[]),
            Some((640, 480, 120))
        );
        assert_eq!(session.mediated_size_for_surface(3, &[]), None);
    }

    /// Two viewers of the *same* surface still negotiate: the densest one
    /// wins, because that is the density this window will be displayed at.
    #[test]
    fn surface_density_is_the_highest_among_its_own_viewers() {
        let mut session = Session::new();
        let mut hidpi = test_client();
        let mut lodpi = test_client();
        hidpi.surface_view_sizes.insert(1, (1920, 1080, 240));
        hidpi.surface_subscriptions.insert(1);
        lodpi.surface_view_sizes.insert(1, (1280, 720, 120));
        lodpi.surface_subscriptions.insert(1);
        session.clients.insert(1, hidpi);
        session.clients.insert(2, lodpi);
        assert_eq!(session.surface_scale_120(1), 240);
        assert_eq!(session.surface_scale_120(2), 120, "no viewers, no density");
    }

    #[test]
    fn mediated_surface_size_clamped_to_encoder_max() {
        let mut session = Session::new();
        let mut c1 = test_client();
        c1.surface_view_sizes.insert(1, (5000, 3000, 240));
        c1.surface_subscriptions.insert(1);
        session.clients.insert(1, c1);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((5000, 3000, 240))
        );
        assert_eq!(
            session.mediated_size_for_surface(1, &[SurfaceEncoderPreference::H264Software]),
            Some((3840, 2160, 240))
        );
    }

    /// The default chain carries H.264 as a fallback, and folding it into a
    /// single ceiling used to hold every surface to 3840×2160 no matter what
    /// the viewer could actually decode.  An AV1 client on a 5K panel gets
    /// composited at 5K.
    #[test]
    fn mediated_surface_size_is_not_held_to_h264_by_a_fallback_in_the_chain() {
        let mut session = Session::new();
        let mut c1 = test_client();
        c1.surface_view_sizes.insert(1, (5120, 2880, 240));
        c1.surface_subscriptions.insert(1);
        c1.surface_codec_support = CODEC_SUPPORT_AV1 | CODEC_SUPPORT_H264;
        c1.surface_max_decode = (8192, 4352);
        session.clients.insert(1, c1);
        assert_eq!(
            session.mediated_size_for_surface(1, &SurfaceEncoderPreference::defaults()),
            Some((5120, 2880, 240))
        );
    }

    /// …but a client that only speaks H.264 still composites at 3840×2160,
    /// so nothing renders larger than it can possibly be sent.
    #[test]
    fn mediated_surface_size_stays_at_h264_ceiling_for_an_h264_only_client() {
        let mut session = Session::new();
        let mut c1 = test_client();
        c1.surface_view_sizes.insert(1, (5120, 2880, 240));
        c1.surface_subscriptions.insert(1);
        c1.surface_codec_support = CODEC_SUPPORT_H264;
        session.clients.insert(1, c1);
        assert_eq!(
            session.mediated_size_for_surface(1, &SurfaceEncoderPreference::defaults()),
            Some((3840, 2160, 240))
        );
    }

    /// Two viewers of one surface, one AV1 and one H.264-only, both asking
    /// for 5K.  The composite serves the more capable of them; the H.264
    /// viewer takes a downscale rather than dragging the surface to 4K.
    #[test]
    fn mediated_surface_size_composites_for_the_most_capable_subscriber() {
        let mut session = Session::new();
        let prefs = SurfaceEncoderPreference::defaults();
        let mut av1 = test_client();
        av1.surface_view_sizes.insert(1, (5120, 2880, 240));
        av1.surface_subscriptions.insert(1);
        av1.surface_codec_support = CODEC_SUPPORT_AV1;
        av1.surface_max_decode = (8192, 4352);
        let mut h264 = test_client();
        h264.surface_view_sizes.insert(1, (5120, 2880, 240));
        h264.surface_subscriptions.insert(1);
        h264.surface_codec_support = CODEC_SUPPORT_H264;
        h264.surface_max_decode = (3840, 2160);
        session.clients.insert(1, av1);
        session.clients.insert(2, h264);
        assert_eq!(
            session.mediated_size_for_surface(1, &prefs),
            Some((5120, 2880, 240))
        );
        // And the H.264 viewer is served an aspect-preserving downscale of
        // that composite, not a stream its decoder would reject.
        let h264 = &session.clients[&2];
        assert_eq!(
            encode_target_at_1x(
                Some((5120, 2880, 240)),
                5120,
                2880,
                surface_encode_cap(&prefs, h264, 1),
            ),
            (3840, 2160)
        );
    }

    /// A client subscribed to `surface_id` with the given codec support and
    /// declared decode ceiling.
    fn decoder_client(codec_support: u8, max_decode: (u16, u16)) -> ClientState {
        let mut c = test_client();
        c.surface_subscriptions.insert(1);
        c.surface_codec_support = codec_support;
        c.surface_max_decode = max_decode;
        c
    }

    #[test]
    fn surface_encode_cap_prefers_the_widest_eligible_backend_before_selection() {
        let prefs = SurfaceEncoderPreference::defaults();
        // Nothing selected yet: size for the best backend the client could
        // land on, and let `SurfaceEncoder::new` skip the ones that can't
        // carry it.
        assert_eq!(
            surface_encode_cap(&prefs, &decoder_client(CODEC_SUPPORT_AV1, (8192, 4352)), 1),
            Some(SurfaceEncoderPreference::NvencAV1.max_dimensions())
        );
        assert_eq!(
            surface_encode_cap(&prefs, &decoder_client(CODEC_SUPPORT_H264, (8192, 4352)), 1),
            Some((3840, 2160))
        );
        // An empty chain means no cap at all.
        assert_eq!(surface_encode_cap(&[], &decoder_client(0, (0, 0)), 1), None);
    }

    #[test]
    fn surface_encode_cap_follows_the_backend_that_actually_won() {
        let prefs = SurfaceEncoderPreference::defaults();
        let mut c = decoder_client(CODEC_SUPPORT_AV1, (8192, 4352));
        c.surface_subs.entry(1).or_default().selected_encoder =
            Some(SurfaceEncoderPreference::H264Vaapi);
        // The chain fell back to H.264 despite the client speaking AV1, so
        // the surface is sized for H.264 rather than for the AV1 it didn't
        // get.
        assert_eq!(surface_encode_cap(&prefs, &c, 1), Some((3840, 2160)));
        c.surface_subs.entry(1).or_default().selected_encoder =
            Some(SurfaceEncoderPreference::AV1Vaapi);
        assert_eq!(
            surface_encode_cap(&prefs, &c, 1),
            Some(SurfaceEncoderPreference::AV1Vaapi.max_dimensions())
        );
    }

    #[test]
    fn surface_encode_cap_preserves_non_widescreen_software_av1_within_4k_pixels() {
        let prefs = SurfaceEncoderPreference::defaults();
        let mut c = decoder_client(CODEC_SUPPORT_AV1, (3400, 2424));
        c.surface_subs.entry(1).or_default().selected_encoder =
            Some(SurfaceEncoderPreference::AV1Software);

        assert_eq!(surface_encode_cap(&prefs, &c, 1), Some((3400, 2424)));
    }

    /// After a creation refused for size, the retry must be sized to what
    /// every eligible backend clears — otherwise the surface asks for the
    /// same impossible frame forever and never shows a picture.
    #[test]
    fn surface_encode_cap_degrades_to_the_tightest_after_an_oversized_refusal() {
        let prefs = SurfaceEncoderPreference::defaults();
        let mut c = decoder_client(CODEC_SUPPORT_AV1, (8192, 4352));
        {
            let sub = c.surface_subs.entry(1).or_default();
            // Degraded is set alongside a stale winner to pin the
            // precedence: the degrade wins, so a backend that has started
            // failing can't strand the surface at a size nothing accepts.
            sub.selected_encoder = Some(SurfaceEncoderPreference::AV1Vaapi);
            sub.encoder_cap_degraded = true;
        }
        assert_eq!(
            surface_encode_cap(&prefs, &c, 1),
            Some(SurfaceEncoderPreference::AV1Software.max_dimensions())
        );
    }

    /// Narrowing the ceiling is for frames nothing can carry.  A backend
    /// that fits and works gets another attempt at the same size instead —
    /// the degrade latches until the client resubscribes, so spending it on
    /// a momentary failure costs the viewer 5K for the rest of the session.
    #[test]
    fn only_a_frame_no_working_backend_fits_is_refused_for_size() {
        let prefs = SurfaceEncoderPreference::defaults();
        let av1 = CODEC_SUPPORT_AV1;
        let all_work = |_| true;

        // 5K on a host where hardware AV1 works: NvencAV1 could have taken
        // it, so this failure was not about the size.
        assert!(!refused_for_size(&prefs, av1, 5120, 2880, all_work));

        // Same frame once hardware AV1 is gone.  Only AV1Software is left
        // for an AV1 client, and it stops at 4K — so the surface has to
        // come down before anything can encode it.  `av1-vulkan` counts as
        // hardware AV1 and carries the same 8K ceiling, so leaving it in
        // would mean a backend still fits and the frame is not a size
        // problem.
        let no_hw_av1 = |p| {
            !matches!(
                p,
                SurfaceEncoderPreference::NvencAV1
                    | SurfaceEncoderPreference::AV1Vaapi
                    | SurfaceEncoderPreference::VulkanVideoAV1
            )
        };
        assert!(refused_for_size(&prefs, av1, 5120, 2880, no_hw_av1));

        // A frame everything clears is never a size problem, however much
        // of the chain is missing.
        assert!(!refused_for_size(&prefs, av1, 1920, 1080, no_hw_av1));

        // An H.264-only client is held to 3840x2160 by its own decoder, not
        // by which backends happen to be present.
        let h264 = CODEC_SUPPORT_H264;
        assert!(refused_for_size(&prefs, h264, 5120, 2880, all_work));
        assert!(!refused_for_size(&prefs, h264, 3840, 2160, all_work));
    }

    /// A backend can pass the 640x480 probe and still fail at 5K — VRAM for
    /// the frame buffers, a per-resolution driver limit the reported maximum
    /// doesn't admit to.  `refused_for_size` says no every time (the backend
    /// fits, and the host has seen it work), so without a second way down the
    /// surface would hold out for a size that never arrives and the viewer
    /// would watch black instead of the 4K it could have had.
    #[test]
    fn a_backend_that_keeps_failing_at_size_eventually_narrows_anyway() {
        let prefs = SurfaceEncoderPreference::defaults();
        let av1 = CODEC_SUPPORT_AV1;
        assert!(
            !refused_for_size(&prefs, av1, 5120, 2880, |_| true),
            "the size alone never explains this failure — hence the counter"
        );

        // What the creation loop does with that verdict, run out.
        let mut sub = SurfaceSubState::default();
        let mut narrowed_after = None;
        for attempt in 1..=CREATE_FAILURES_BEFORE_DEGRADE + 2 {
            sub.create_failures = sub.create_failures.saturating_add(1);
            let narrow = sub.create_failures >= CREATE_FAILURES_BEFORE_DEGRADE;
            if narrow && !sub.encoder_cap_degraded {
                sub.encoder_cap_degraded = true;
                narrowed_after.get_or_insert(attempt);
            }
        }
        assert_eq!(narrowed_after, Some(CREATE_FAILURES_BEFORE_DEGRADE));

        // And a run of failures that a success interrupts never gets there —
        // the resolution survives a momentary fault, which is the whole
        // reason the first failure doesn't narrow.
        let mut sub = SurfaceSubState::default();
        for _ in 0..CREATE_FAILURES_BEFORE_DEGRADE * 3 {
            sub.create_failures = sub.create_failures.saturating_add(1);
            assert!(sub.create_failures < CREATE_FAILURES_BEFORE_DEGRADE);
            sub.create_failures = 0; // the next creation succeeds
        }
        assert!(!sub.encoder_cap_degraded);
    }

    /// The decoder ceiling is a hard intersection: advertising AV1 says
    /// nothing about how large a frame the browser will actually accept, so
    /// a client that never declared one stays at 4K however capable the
    /// encoder is.
    #[test]
    fn surface_encode_cap_never_exceeds_the_declared_decoder_ceiling() {
        let prefs = SurfaceEncoderPreference::defaults();
        let av1 = CODEC_SUPPORT_AV1;
        assert_eq!(
            surface_encode_cap(&prefs, &decoder_client(av1, (0, 0)), 1),
            Some((3840, 2160)),
            "undeclared decode ceiling must not unlock >4K"
        );
        assert_eq!(
            surface_encode_cap(&prefs, &decoder_client(av1, (5120, 2880)), 1),
            Some((5120, 2880)),
            "a declared ceiling below the encoder's wins"
        );
        assert_eq!(
            surface_encode_cap(&prefs, &decoder_client(av1, (16384, 8704)), 1),
            Some(SurfaceEncoderPreference::NvencAV1.max_dimensions()),
            "a declared ceiling above the encoder's does not raise it"
        );
    }

    /// Hiding a tab unsubscribes from every surface and resubscribes on the
    /// way back.  The pane it sized is still there, so its say has to survive
    /// the gap: releasing it resized the window for every *other* viewer, and
    /// again a moment later when the tab returned.
    #[test]
    fn mediated_surface_size_survives_an_unsubscribe() {
        let mut session = Session::new();
        let mut watching = test_client();
        let mut hidden = test_client();
        watching.surface_view_sizes.insert(1, (1920, 1080, 120));
        watching.surface_subscriptions.insert(1);
        // Subscription gone, claim kept on a countdown — exactly what the
        // unsubscribe handler leaves behind.
        hidden.surface_view_sizes.insert(1, (1000, 700, 240));
        hidden
            .surface_claim_lapses
            .insert(1, Instant::now() + SURFACE_CLAIM_GRACE);
        session.clients.insert(1, watching);
        session.clients.insert(2, hidden);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1000, 700, 240)),
            "a hidden viewer keeps the surface at the size its pane needs"
        );
        // The global output scale is what made this move *every* surface.
        assert_eq!(session.surface_scale_120(1), 240);
    }

    /// A viewer that stays away is not coming back on this pane: an iPad
    /// whose lid is shut unsubscribes and then goes silent forever, holding
    /// the surface at a tablet's width for everyone still watching.  Once the
    /// grace elapses the claim stops counting, and expiring it hands the
    /// surface to whoever is left.
    #[test]
    fn a_lapsed_claim_stops_constraining_the_surface() {
        let mut session = Session::new();
        let mut watching = test_client();
        let mut gone = test_client();
        watching.surface_view_sizes.insert(1, (1920, 1080, 120));
        watching.surface_subscriptions.insert(1);
        gone.surface_view_sizes.insert(1, (1000, 700, 240));
        // Unsubscribed longer ago than the grace allows.
        let lapsed_at = Instant::now() - Duration::from_millis(1);
        gone.surface_claim_lapses.insert(1, lapsed_at);
        session.clients.insert(1, watching);
        session.clients.insert(2, gone);

        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1920, 1080, 120)),
            "the viewer still watching gets its own size back"
        );
        assert_eq!(session.surface_scale_120(1), 120);

        // Expiry is what actually retires the claim, and reports that there
        // is nothing further to wait for.
        assert_eq!(
            session.expire_surface_claims(Instant::now(), &[], false),
            None
        );
        let gone = session.clients.get(&2).unwrap();
        assert!(gone.surface_view_sizes.is_empty());
        assert!(gone.surface_claim_lapses.is_empty());
    }

    /// A claim still inside its grace is left alone, and the deadline comes
    /// back so the delivery loop parks until it comes due rather than
    /// discovering it by accident.
    #[test]
    fn a_claim_inside_its_grace_is_kept_and_scheduled() {
        let mut session = Session::new();
        let mut hidden = test_client();
        hidden.surface_view_sizes.insert(1, (1000, 700, 240));
        let due = Instant::now() + SURFACE_CLAIM_GRACE;
        hidden.surface_claim_lapses.insert(1, due);
        session.clients.insert(1, hidden);

        assert_eq!(
            session.expire_surface_claims(Instant::now(), &[], false),
            Some(due)
        );
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1000, 700, 240)),
            "a viewer mid-tab-switch keeps its say"
        );
    }

    /// …and closing the pane releases it without waiting.  That is the 0×0
    /// unset in Surface Resize, which removes the entry outright.
    #[test]
    fn mediated_surface_size_is_released_by_the_unset() {
        let mut session = Session::new();
        let mut watching = test_client();
        let mut leaving = test_client();
        watching.surface_view_sizes.insert(1, (1920, 1080, 120));
        watching.surface_subscriptions.insert(1);
        leaving.surface_view_sizes.insert(1, (1000, 700, 240));
        session.clients.insert(1, watching);
        session.clients.insert(2, leaving);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1000, 700, 240))
        );

        session
            .clients
            .get_mut(&2)
            .unwrap()
            .surface_view_sizes
            .remove(&1);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1920, 1080, 120)),
            "the viewer that closed its pane stops constraining the surface"
        );
        assert_eq!(session.surface_scale_120(1), 120);
    }

    /// The whole point of a scaled subscription: a card-sized thumbnail must
    /// not drag the Wayland window down to a card for the viewer watching it
    /// full size.  It is subscribed and it has a view size, so both existing
    /// guards let it through — only the scaled target excludes it.
    #[test]
    fn mediated_surface_size_ignores_scaled_subscriber() {
        let mut session = Session::new();
        let mut full = test_client();
        let mut thumb = test_client();
        full.surface_subscriptions.insert(1);
        full.surface_view_sizes.insert(1, (1920, 1080, 120));
        thumb.surface_subscriptions.insert(1);
        thumb.surface_view_sizes.insert(1, (314, 176, 120));
        thumb.surface_subs.entry(1).or_default().scaled_target = Some((314, 176));
        session.clients.insert(1, full);
        session.clients.insert(2, thumb);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1920, 1080, 120))
        );
    }

    /// With no mediated viewer left there is nothing to mediate, so the
    /// surface keeps its last configured size rather than collapsing to the
    /// thumbnail's box.  `resize_surfaces_to_mediated_sizes` sends no resize
    /// for `None`, which is what leaves the compositor alone.
    #[test]
    fn mediated_surface_size_none_when_every_subscriber_is_scaled() {
        let mut session = Session::new();
        let mut thumb = test_client();
        thumb.surface_subscriptions.insert(1);
        thumb.surface_view_sizes.insert(1, (314, 176, 120));
        thumb.surface_subs.entry(1).or_default().scaled_target = Some((314, 176));
        session.clients.insert(1, thumb);
        assert_eq!(session.mediated_size_for_surface(1, &[]), None);
    }

    /// `per_client_encode_target` for a surface sitting at 1x, where its
    /// logical size and its native size are the same numbers.  The display
    /// cap is then never the binding constraint — which is the world every
    /// test that predates it was written in, and still the world of any
    /// session without a high-DPI viewer in it.
    fn encode_target_at_1x(
        view_size: Option<(u16, u16, u16)>,
        native_w: u32,
        native_h: u32,
        max: Option<(u16, u16)>,
    ) -> (u32, u32) {
        Session::per_client_encode_target(
            view_size,
            native_w,
            native_h,
            Some((native_w, native_h)),
            max,
        )
    }

    /// The bandwidth half of the mixed-DPI story.  A 1x viewer draws the
    /// window at its logical size, so every pixel past that is encoded,
    /// sent, and thrown away — and it is 9x the pixels at 3x, on the viewer
    /// least likely to have the bandwidth for them.
    #[test]
    fn per_client_encode_target_caps_at_what_the_viewer_can_display() {
        // 400x300 window composited at 3x = 1200x900, watched by a 1x pane
        // with room for all of it.  It can only show 400x300 of it.
        assert_eq!(
            Session::per_client_encode_target(
                Some((1600, 1200, 120)),
                1200,
                900,
                Some((400, 300)),
                None
            ),
            (400, 300)
        );
        // The 3x viewer that sized the surface still gets every pixel.
        assert_eq!(
            Session::per_client_encode_target(
                Some((1200, 900, 360)),
                1200,
                900,
                Some((400, 300)),
                None
            ),
            (1200, 900)
        );
        // A 2x viewer in between gets 2x the logical size, not 3x.
        assert_eq!(
            Session::per_client_encode_target(
                Some((1600, 1200, 240)),
                1200,
                900,
                Some((400, 300)),
                None
            ),
            (800, 600)
        );
        // A pane smaller than the cap is still the binding constraint.
        assert_eq!(
            Session::per_client_encode_target(
                Some((200, 150, 120)),
                1200,
                900,
                Some((400, 300)),
                None
            ),
            (200, 150)
        );
    }

    /// A scaled subscription names encoder pixels outright — the caller
    /// passes no logical size for exactly this reason.  Read as a 1x pane
    /// over a 3x surface instead, this 800-wide request would come back at
    /// the 400-wide display cap: a thumbnail served half the pixels it
    /// asked for, with nothing in its own request to explain why.
    #[test]
    fn per_client_encode_target_leaves_a_scaled_target_uncapped() {
        assert_eq!(
            Session::per_client_encode_target(Some((800, 600, 120)), 1200, 900, None, None),
            (800, 600)
        );
        // Same request, mistakenly read as a pane at 1x:
        assert_eq!(
            Session::per_client_encode_target(
                Some((800, 600, 120)),
                1200,
                900,
                Some((400, 300)),
                None
            ),
            (400, 300)
        );
    }

    /// A scaled target is just a view box, so it inherits the same clamps —
    /// native aspect preserved, never upscaled past native.
    #[test]
    fn per_client_encode_target_honours_a_scaled_target() {
        // 314-wide box against a 16:9 native ⇒ width-bound, even-rounded.
        assert_eq!(
            encode_target_at_1x(Some((314, 176, 120)), 1920, 1080, None),
            (314, 176)
        );
        // A thumbnail asking for more than native still gets native.
        assert_eq!(
            encode_target_at_1x(Some((4000, 4000, 120)), 640, 480, None),
            (640, 480)
        );
    }

    #[test]
    fn per_client_encode_target_uses_view_size() {
        // 1280×720 viewport, 1920×1080 native (both 16:9) ⇒ 1280×720.
        assert_eq!(
            encode_target_at_1x(Some((1280, 720, 120)), 1920, 1080, None),
            (1280, 720)
        );
    }

    #[test]
    fn per_client_encode_target_clamps_to_native() {
        // Viewport 4000×3000 but native is only 1920×1080 — encoding bigger
        // would just upscale, so the encoder runs at native.
        assert_eq!(
            encode_target_at_1x(Some((4000, 3000, 240)), 1920, 1080, None),
            (1920, 1080)
        );
    }

    #[test]
    fn per_client_encode_target_clamps_to_encoder_max() {
        // Viewport 8000×4500 and native 8000×4500, but H.264 caps at
        // 3840×2160 — same 16:9 aspect, picks (3840, 2160).
        assert_eq!(
            encode_target_at_1x(Some((8000, 4500, 240)), 8000, 4500, Some((3840, 2160))),
            (3840, 2160)
        );
    }

    #[test]
    fn per_client_encode_target_falls_back_to_native_without_view_size() {
        // Client hasn't sent Surface Resize yet — encode at native size.
        assert_eq!(encode_target_at_1x(None, 800, 600, None), (800, 600));
        // Zero-dim viewport (cleared by client) ⇒ also fall back.
        assert_eq!(
            encode_target_at_1x(Some((0, 0, 120)), 800, 600, None),
            (800, 600)
        );
    }

    #[test]
    fn per_client_encode_target_preserves_native_aspect_landscape() {
        // Native 1920×1080 (16:9).  Client viewport 1000×1000 (square).
        // Width-bound at 1000 keeps height at 1000*1080/1920 = 562 →
        // round even = 562 (already even).
        assert_eq!(
            encode_target_at_1x(Some((1000, 1000, 120)), 1920, 1080, None),
            (1000, 562)
        );
    }

    #[test]
    fn per_client_encode_target_preserves_native_aspect_portrait_client() {
        // Native 1920×1080 (16:9).  Client viewport 500×1000 (1:2).
        // Width-bound at 500 keeps height at 500*1080/1920 = 281,
        // rounded even = 280.
        assert_eq!(
            encode_target_at_1x(Some((500, 1000, 120)), 1920, 1080, None),
            (500, 280)
        );
    }

    #[test]
    fn per_client_encode_target_preserves_native_aspect_landscape_client_portrait_native() {
        // Native 1080×1920 (9:16).  Client viewport 1000×500 (2:1).
        // Height-bound at 500 keeps width at 500*1080/1920 = 281,
        // rounded even = 280.
        assert_eq!(
            encode_target_at_1x(Some((1000, 500, 120)), 1080, 1920, None),
            (280, 500)
        );
    }

    #[test]
    fn per_client_encode_target_rounds_to_even() {
        // Native 101×51 — odd dimensions.  Same-shape viewport rounds
        // down to even.
        assert_eq!(
            encode_target_at_1x(Some((101, 51, 120)), 101, 51, None),
            (100, 50)
        );
    }

    #[test]
    fn per_client_encode_target_floors_at_two() {
        // Tiny viewport on a tall native — height-bound at 1 → width 0
        // → floor to 2.  Encoders reject 0-dim and most reject 1-dim
        // because chroma subsampling needs at least a 2×2 grid.
        assert_eq!(
            encode_target_at_1x(Some((1, 1, 120)), 100, 1000, None),
            (2, 2)
        );
    }

    /// Regression: after a resize-shrink, stale per-client downscale
    /// targets (registered for the prior, larger native) can still
    /// produce `last_pixels` entries at sizes larger than the actual
    /// new native.  `compositor_native_for_sid` MUST consult the
    /// authoritative `native_sizes` map first so
    /// `per_client_encode_target` is computed against the real native,
    /// not the stale entry.  Without this, the encoder rebuilds at the
    /// wrong size and visible frames freeze until the stale target is
    /// cleared.
    #[test]
    fn compositor_native_for_sid_prefers_resize_event_over_stale_pixel_snapshot() {
        let mut native_sizes = HashMap::new();
        native_sizes.insert(1u16, (640u32, 360u32));
        // Renderer just copied into a stale 1920x1080 downscale target
        // and a fresh 640x360 native composite, so `last_pixels` (and
        // `pixel_snapshot`) carry both sizes.  The 1920x1080 entry is
        // larger, so a width-first `max_by_key((w, h))` pick would mis-
        // identify it as native.
        let pixel_snapshot: Vec<(u16, u32, u32, u64, u32, u16)> =
            vec![(1, 640, 360, 10, 0, 0), (1, 1920, 1080, 9, 0, 0)];
        assert_eq!(
            compositor_native_for_sid(&native_sizes, &pixel_snapshot, 1),
            Some((640, 360)),
        );
    }

    /// First render after `SurfaceCreated` may arrive before the
    /// `SurfaceResized` event, so `native_sizes` is empty.  Falling
    /// back to the largest pixel-snapshot entry keeps the encode loop
    /// from skipping forever in that bootstrap window.
    #[test]
    fn compositor_native_for_sid_falls_back_to_largest_snapshot_entry() {
        let native_sizes = HashMap::new();
        let pixel_snapshot: Vec<(u16, u32, u32, u64, u32, u16)> =
            vec![(1, 320, 240, 5, 0, 0), (1, 800, 600, 6, 0, 0)];
        assert_eq!(
            compositor_native_for_sid(&native_sizes, &pixel_snapshot, 1),
            Some((800, 600)),
        );
    }

    #[test]
    fn compositor_native_for_sid_returns_none_for_unknown_sid() {
        let native_sizes = HashMap::new();
        let pixel_snapshot: Vec<(u16, u32, u32, u64, u32, u16)> = vec![(2, 640, 360, 1, 0, 0)];
        assert_eq!(
            compositor_native_for_sid(&native_sizes, &pixel_snapshot, 1),
            None,
        );
    }

    /// Native and per-client downscale commits alternate when two viewers
    /// request different encode sizes.  They must coexist in the pixel cache
    /// without changing the authoritative physical/logical surface pair;
    /// otherwise the smaller viewer's display cap appears and disappears and
    /// its encoder is rebuilt at both sizes forever.
    #[test]
    fn surface_commits_at_multiple_targets_do_not_redefine_native_surface_info() {
        let info = CachedSurfaceInfo {
            surface_id: 1,
            parent_id: 0,
            origin: None,
            width: 1919,
            height: 942,
            logical_width: 1515,
            logical_height: 744,
            title: String::new(),
            app_id: String::new(),
        };
        let mut last_pixels = HashMap::new();
        let mut generation = 0;

        cache_surface_commit(
            &mut last_pixels,
            &mut generation,
            (1, 1919, 942),
            yas_compositor::PixelData::GpuOnly,
            1,
            0,
            false,
        );
        cache_surface_commit(
            &mut last_pixels,
            &mut generation,
            (1, 1514, 742),
            yas_compositor::PixelData::GpuOnly,
            2,
            0,
            false,
        );

        assert_eq!((info.width, info.height), (1919, 942));
        assert_eq!((info.logical_width, info.logical_height), (1515, 744));
        assert!(last_pixels.contains_key(&(1, 1919, 942)));
        assert!(last_pixels.contains_key(&(1, 1514, 742)));
        assert_eq!(generation, 2);
    }

    #[test]
    fn mediated_surface_size_picks_largest_box_that_fits_all_clients() {
        let mut session = Session::new();
        let mut c1 = test_client();
        let mut c2 = test_client();
        c1.surface_view_sizes.insert(1, (1920, 1080, 120));
        c2.surface_view_sizes.insert(1, (640, 360, 120));
        c1.surface_subscriptions.insert(1);
        c2.surface_subscriptions.insert(1);
        session.clients.insert(1, c1);
        session.clients.insert(2, c2);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((640, 360, 120))
        );
    }

    #[test]
    fn mediated_surface_size_fits_cross_axis_constraints() {
        let mut session = Session::new();
        let mut wide = test_client();
        let mut tall = test_client();
        wide.surface_view_sizes.insert(1, (1920, 600, 120));
        tall.surface_view_sizes.insert(1, (800, 1080, 120));
        wide.surface_subscriptions.insert(1);
        tall.surface_subscriptions.insert(1);
        session.clients.insert(1, wide);
        session.clients.insert(2, tall);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((800, 600, 120))
        );
    }

    #[test]
    fn adding_or_removing_a_smaller_viewer_shrinks_then_restores_the_size() {
        let mut session = Session::new();
        let mut large = test_client();
        let mut small = test_client();
        large.surface_view_sizes.insert(1, (1934, 1224, 120));
        small.surface_view_sizes.insert(1, (1920, 942, 120));
        large.surface_subscriptions.insert(1);
        small.surface_subscriptions.insert(1);
        session.clients.insert(1, large);
        session.clients.insert(2, small);

        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1920, 942, 120))
        );
        session.clients.remove(&2);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1934, 1224, 120))
        );
    }

    #[test]
    fn removing_high_density_viewer_restores_remaining_viewers_scale() {
        let mut session = Session::new();
        let mut full_hd = test_client();
        let mut high_density_900p = test_client();
        full_hd.surface_view_sizes.insert(1, (1920, 1080, 120));
        high_density_900p
            .surface_view_sizes
            .insert(1, (4800, 2700, 360));
        full_hd.surface_subscriptions.insert(1);
        high_density_900p.surface_subscriptions.insert(1);
        session.clients.insert(1, full_hd);
        session.clients.insert(2, high_density_900p);

        // The 900p logical bound is the largest size that fits both viewers;
        // the 3× viewer still supplies the compositor density.
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((4800, 2700, 360))
        );
        session.clients.remove(&2);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1920, 1080, 120))
        );
    }

    /// A pane measuring an odd pixel extent must not become an odd surface:
    /// no 4:2:0 encoder can carry it. Mediation negotiates down to the even
    /// grid and the client learns the real size from SurfaceResized.
    #[test]
    fn mediated_surface_size_rounds_odd_extents_to_even() {
        let mut session = Session::new();
        let mut c = test_client();
        c.surface_view_sizes.insert(1, (1237, 843, 120));
        c.surface_subscriptions.insert(1);
        session.clients.insert(1, c);
        assert_eq!(
            session.mediated_size_for_surface(1, &[]),
            Some((1236, 842, 120))
        );
    }

    /// The even-rounded mediated size round-trips through the per-client
    /// target unchanged: once the surface is configured at it, the view
    /// that asked for the odd size gets exactly the native stream.
    #[test]
    fn odd_view_over_even_mediated_native_targets_native_exactly() {
        let target = encode_target_at_1x(Some((1237, 843, 120)), 1236, 842, None);
        assert_eq!(target, (1236, 842));
    }

    #[test]
    fn due_preview_reserves_the_last_lead_slot() {
        let mut client = test_client();
        client.lead = Some(1);
        client.subscriptions.insert(1);
        client.subscriptions.insert(2);

        let target_frames = target_frame_window(&client);
        let lead_limit = target_frames.saturating_sub(1).max(1);
        fill_inflight(&mut client, lead_limit, 512);

        assert!(window_open(&client));
        assert!(lead_window_open(&client, false));
        assert!(!lead_window_open(&client, true));
        assert!(can_send_preview(&client, 2, Instant::now()));
    }

    #[test]
    fn entering_scrollback_uses_current_visible_frame_as_baseline() {
        let mut client = test_client();
        let live = sample_frame("live");
        client.lead = Some(7);
        client.subscriptions.insert(7);
        client.last_sent.insert(7, live.clone());

        assert!(update_client_scroll_state(&mut client, 7, 12));
        assert_eq!(client.scroll_offsets.get(&7), Some(&12));
        assert_eq!(client.scroll_caches.get(&7), Some(&live));
    }

    #[test]
    fn leaving_scrollback_seeds_live_diff_from_scrollback_view() {
        let mut client = test_client();
        let history = sample_frame("hist");
        client.lead = Some(7);
        client.subscriptions.insert(7);
        client.scroll_offsets.insert(7, 12);
        client.scroll_caches.insert(7, history.clone());

        assert!(update_client_scroll_state(&mut client, 7, 0));
        assert_eq!(client.scroll_offsets.get(&7), None);
        assert_eq!(client.last_sent.get(&7), Some(&history));
        assert_eq!(client.scroll_caches.get(&7), None);
    }

    #[test]
    fn output_moves_a_parked_view_with_the_text_it_is_reading() {
        // The offset counts up from the live bottom, so three lines of
        // output have to push it three deeper for the reader to stay on the
        // same rows.
        let mut client = test_client();
        client.subscriptions.insert(7);
        client.scroll_offsets.insert(7, 12);

        assert_eq!(reanchor_client(&mut client, 7, 3, 500), Some(15));
        assert_eq!(client.scroll_offsets.get(&7), Some(&15));
    }

    #[test]
    fn a_parked_view_stops_at_the_oldest_line_it_still_has() {
        let mut client = test_client();
        client.subscriptions.insert(7);
        client.scroll_offsets.insert(7, 98);

        // Scrollback full at 100: the text really is scrolling away now, and
        // the deepest offset is as far back as the reader can follow it.
        assert_eq!(reanchor_client(&mut client, 7, 5, 100), Some(100));
        assert_eq!(reanchor_client(&mut client, 7, 5, 100), None);
    }

    #[test]
    fn losing_the_whole_scrollback_returns_a_parked_view_to_the_live_tail() {
        let mut client = test_client();
        let history = sample_frame("hist");
        client.lead = Some(7);
        client.subscriptions.insert(7);
        client.scroll_offsets.insert(7, 12);
        client.scroll_caches.insert(7, history.clone());

        assert_eq!(reanchor_client(&mut client, 7, 4, 0), Some(0));
        assert_eq!(client.scroll_offsets.get(&7), None);
        assert_eq!(client.last_sent.get(&7), Some(&history));
    }

    /// The point of the relative form: the client asked to go back three
    /// lines from what it was looking at, and three lines of output landed
    /// while it asked.  The gesture and the re-anchor compose.
    #[test]
    fn a_relative_scroll_lands_on_top_of_a_re_anchor() {
        let mut client = test_client();
        client.subscriptions.insert(7);
        client.scroll_offsets.insert(7, 12);

        // In flight: three lines scroll away and we hold the view still.
        assert_eq!(reanchor_client(&mut client, 7, 3, 500), Some(15));
        // The request was computed against offset 12 and still means "three
        // more lines back", not "offset 15".  Nothing to answer: the client
        // took the re-anchor to 15 and applied the same 3 to it.
        assert_eq!(scroll_client_by(&mut client, 7, 3, 500), None);
        assert_eq!(client.scroll_offsets.get(&7), Some(&18));
    }

    #[test]
    fn a_relative_scroll_starts_and_ends_a_scrollback_visit() {
        let mut client = test_client();
        client.subscriptions.insert(7);

        // Going back needs no answer — the client predicted 3 itself — but
        // coming home overshot the live tail, and a clamp it cannot predict
        // is exactly what an answer is for.
        assert_eq!(scroll_client_by(&mut client, 7, 3, 500), None);
        assert_eq!(client.scroll_offsets.get(&7), Some(&3));
        assert_eq!(scroll_client_by(&mut client, 7, -9, 500), Some(0));
        assert_eq!(client.scroll_offsets.get(&7), None);
    }

    #[test]
    fn a_relative_scroll_stops_at_the_ends() {
        let mut client = test_client();
        client.subscriptions.insert(7);
        client.scroll_offsets.insert(7, 98);

        assert_eq!(scroll_client_by(&mut client, 7, i64::MAX, 100), Some(100));
        assert_eq!(scroll_client_by(&mut client, 7, 5, 100), None);
    }

    #[test]
    fn a_wheel_notch_is_never_answered_back() {
        // A notch is several requests long and the answer is absolute, so an
        // answer to any of them is stale before it lands.  The client that
        // adopts one gets dragged back and then over-sends the next delta:
        // twelve rows of wheel went out as 2, 2, 4, 4, 2 and landed on 14.
        let mut client = test_client();
        client.subscriptions.insert(7);

        for _ in 0..6 {
            assert_eq!(scroll_client_by(&mut client, 7, 2, 500), None);
        }
        assert_eq!(client.scroll_offsets.get(&7), Some(&12));
    }

    #[test]
    fn a_live_client_is_left_alone() {
        let mut client = test_client();
        client.subscriptions.insert(7);

        assert_eq!(reanchor_client(&mut client, 7, 9, 500), None);
        assert!(client.scroll_offsets.is_empty());
    }

    #[tokio::test]
    async fn request_surface_capture_returns_pixels_from_compositor() {
        let (command_tx, command_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("test-capture-reply".into())
            .spawn(move || {
                let CompositorCommand::Capture {
                    surface_id,
                    scale_120: _,
                    reply,
                } = command_rx.recv().unwrap()
                else {
                    panic!("expected capture command");
                };
                assert_eq!(surface_id, 7);
                let _ = reply.send(Some((2, 3, vec![1, 2, 3, 4])));
            })
            .unwrap();

        let result =
            request_surface_capture_with_timeout(command_tx, 7, 0, Duration::from_millis(50)).await;

        assert_eq!(result, Some((2, 3, vec![1, 2, 3, 4])));
    }

    #[tokio::test]
    async fn request_surface_capture_returns_none_when_compositor_disconnects() {
        let (command_tx, command_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("test-capture-drop".into())
            .spawn(move || {
                let _ = command_rx.recv().unwrap();
            })
            .unwrap();

        let result =
            request_surface_capture_with_timeout(command_tx, 7, 0, Duration::from_millis(50)).await;

        assert_eq!(result, None);
    }

    // ── frame_window ──

    #[test]
    fn frame_window_minimum_is_two() {
        assert!(frame_window(0.0, 60.0) >= 2);
    }

    #[test]
    fn frame_window_scales_with_rtt() {
        let low = frame_window(10.0, 60.0);
        let high = frame_window(200.0, 60.0);
        assert!(high > low, "higher RTT should need more frames in flight");
    }

    #[test]
    fn frame_window_scales_with_fps() {
        let slow = frame_window(100.0, 10.0);
        let fast = frame_window(100.0, 120.0);
        assert!(fast > slow, "higher fps should need more frames in flight");
    }

    #[test]
    fn frame_window_zero_rtt() {
        assert!(frame_window(0.0, 120.0) >= 2);
    }

    // ── path_rtt_ms ──

    #[test]
    fn path_rtt_ms_uses_min_when_positive() {
        let mut client = test_client();
        client.rtt_ms = 100.0;
        client.min_rtt_ms = 30.0;
        assert_eq!(path_rtt_ms(&client), 30.0);
    }

    #[test]
    fn path_rtt_ms_falls_back_to_rtt_when_min_zero() {
        let mut client = test_client();
        client.rtt_ms = 80.0;
        client.min_rtt_ms = 0.0;
        assert_eq!(path_rtt_ms(&client), 80.0);
    }

    // ── ewma_with_direction ──

    #[test]
    fn ewma_rising_uses_rise_alpha() {
        let result = ewma_with_direction(100.0, 200.0, 0.5, 0.1);
        // rise: 100 * 0.5 + 200 * 0.5 = 150
        assert!((result - 150.0).abs() < 0.01);
    }

    #[test]
    fn ewma_falling_uses_fall_alpha() {
        let result = ewma_with_direction(200.0, 100.0, 0.5, 0.1);
        // fall: 200 * 0.9 + 100 * 0.1 = 190
        assert!((result - 190.0).abs() < 0.01);
    }

    #[test]
    fn ewma_same_value_unchanged() {
        let result = ewma_with_direction(50.0, 50.0, 0.5, 0.5);
        assert!((result - 50.0).abs() < 0.01);
    }

    // ── advance_deadline ──

    #[test]
    fn advance_deadline_steps_forward() {
        let now = Instant::now();
        let mut deadline = now;
        let interval = Duration::from_millis(16);
        advance_deadline(&mut deadline, now, interval);
        assert!(deadline > now);
        assert!(deadline <= now + interval + Duration::from_micros(100));
    }

    #[test]
    fn advance_deadline_does_not_accumulate_timer_jitter() {
        let base = Instant::now();
        let interval = Duration::from_millis(8);
        let mut deadline = base + interval;
        let late_wakeup = deadline + Duration::from_millis(2);
        advance_deadline(&mut deadline, late_wakeup, interval);
        assert_eq!(deadline, base + interval * 2);
    }

    #[test]
    fn advance_deadline_resets_when_far_behind() {
        let now = Instant::now();
        // deadline is way in the past (more than 2 intervals ago)
        let mut deadline = now - Duration::from_secs(10);
        let interval = Duration::from_millis(16);
        advance_deadline(&mut deadline, now, interval);
        // Should snap to now + interval since scheduled + interval < now
        assert!(deadline >= now);
    }

    #[test]
    fn consume_deadline_skips_missed_ticks_without_changing_phase() {
        let base = Instant::now();
        let interval = Duration::from_millis(8);
        let mut deadline = base + interval;
        let late_wakeup = base + Duration::from_millis(21);

        let consumed = consume_deadline(&mut deadline, late_wakeup, interval);

        assert_eq!(consumed, base + interval * 2);
        assert_eq!(deadline, base + interval * 3);
        assert!(consumed <= late_wakeup);
        assert!(deadline > late_wakeup);
    }

    #[test]
    fn should_snapshot_pty_requires_dirty_and_needful() {
        let now = Instant::now();
        assert!(should_snapshot_pty(true, true, false, None, now));
        assert!(!should_snapshot_pty(false, true, false, None, now));
        assert!(!should_snapshot_pty(true, false, false, None, now));
    }

    #[test]
    fn should_snapshot_pty_defers_synced_output() {
        let now = Instant::now();
        assert!(!should_snapshot_pty(true, true, true, None, now));
        assert!(should_snapshot_pty(true, true, false, None, now));
    }

    #[test]
    fn should_snapshot_pty_waits_for_output_coalescing() {
        let now = Instant::now();
        let deadline = now + PTY_OUTPUT_QUIET;
        assert!(!should_snapshot_pty(true, true, false, Some(deadline), now,));
        assert!(should_snapshot_pty(
            true,
            true,
            false,
            Some(deadline),
            deadline,
        ));
    }

    #[test]
    fn output_coalescing_slides_until_its_hard_deadline() {
        let now = Instant::now();
        let mut deadline = None;
        let mut hard_deadline = None;
        arm_pty_output_coalesce(
            &mut deadline,
            &mut hard_deadline,
            now,
            PTY_OUTPUT_COALESCE_MAX,
        );
        assert_eq!(deadline, Some(now + PTY_OUTPUT_QUIET));
        arm_pty_output_coalesce(
            &mut deadline,
            &mut hard_deadline,
            now + Duration::from_millis(4),
            PTY_OUTPUT_COALESCE_MAX,
        );
        assert_eq!(deadline, Some(now + Duration::from_millis(5)));
        arm_pty_output_coalesce(
            &mut deadline,
            &mut hard_deadline,
            now + PTY_OUTPUT_COALESCE_MAX,
            PTY_OUTPUT_COALESCE_MAX,
        );
        assert_eq!(deadline, Some(now + PTY_OUTPUT_COALESCE_MAX));
        assert_eq!(hard_deadline, Some(now + PTY_OUTPUT_COALESCE_MAX));
    }

    #[test]
    fn output_coalescing_ceiling_tracks_high_refresh_displays() {
        assert_eq!(pty_output_coalesce_cap(60.0), PTY_OUTPUT_COALESCE_MAX);
        assert_eq!(pty_output_coalesce_cap(1_000.0), Duration::from_millis(1),);
        assert!(pty_output_coalesce_cap(2_000.0) < Duration::from_millis(1));
    }

    #[test]
    fn pty_parse_budget_is_aggregate_across_terminals() {
        let half = PTY_PARSE_BUDGET_PER_SESSION_TICK / 2;
        let mut session = PTY_PARSE_BUDGET_PER_SESSION_TICK;
        let mut first = PTY_PARSE_BUDGET_PER_TICK;
        charge_pty_parse_budgets(&mut first, &mut session, half);
        let mut second = PTY_PARSE_BUDGET_PER_TICK;
        charge_pty_parse_budgets(&mut second, &mut session, half);

        assert_eq!(session, 0);
        assert!(first > 0 && second > 0, "neither per-PTY cap was reached");
    }

    #[test]
    fn pty_parse_cursor_resumes_after_the_last_visited_terminal() {
        assert_eq!(advance_pty_parse_cursor(0, 2, 4), 2);
        assert_eq!(advance_pty_parse_cursor(2, 1, 4), 3);
        assert_eq!(advance_pty_parse_cursor(2, 4, 4), 3);
        assert_eq!(advance_pty_parse_cursor(0, 0, 0), 0);
    }

    #[test]
    fn enqueue_ready_frame_refuses_new_frames_when_capped() {
        let mut queue = VecDeque::new();
        for cols in 1..=(READY_FRAME_QUEUE_CAP as u16) {
            assert!(enqueue_ready_frame(&mut queue, FrameState::new(1, cols)));
        }
        assert!(!enqueue_ready_frame(
            &mut queue,
            FrameState::new(1, READY_FRAME_QUEUE_CAP as u16 + 1),
        ));
        assert_eq!(queue.len(), READY_FRAME_QUEUE_CAP);
        assert_eq!(queue.front().map(FrameState::cols), Some(1));
        assert_eq!(
            queue.back().map(FrameState::cols),
            Some(READY_FRAME_QUEUE_CAP as u16),
        );
    }

    #[test]
    fn find_sync_output_end_returns_end_of_first_close_sequence() {
        let bytes = b"abc\x1b[?2026lrest\x1b[?2026l";
        assert_eq!(find_sync_output_end(&[], bytes), Some(11));
    }

    #[test]
    fn find_sync_output_end_returns_none_without_close_sequence() {
        assert_eq!(find_sync_output_end(&[], b"\x1b[?2026hpartial"), None);
    }

    #[test]
    fn find_sync_output_end_detects_boundary_split_across_reads() {
        assert_eq!(find_sync_output_end(b"abc\x1b[?20", b"26lrest"), Some(3));
    }

    #[test]
    fn update_sync_scan_tail_keeps_recent_suffix_only() {
        let mut tail = Vec::new();
        update_sync_scan_tail(&mut tail, b"123456789");
        assert_eq!(tail, b"3456789");
    }

    // ── window_saturated ──

    #[test]
    fn window_saturated_at_90_percent_frames() {
        let client = test_client();
        let target = target_frame_window(&client);
        let frames_90 = (target * 9).div_ceil(10); // ceil(target * 0.9)
        assert!(window_saturated(&client, frames_90, 0));
    }

    #[test]
    fn window_saturated_not_at_low_usage() {
        let client = test_client();
        assert!(!window_saturated(&client, 1, 0));
    }

    #[test]
    fn window_saturated_at_90_percent_bytes() {
        let client = test_client();
        let target_bytes = target_byte_window(&client);
        let bytes_90 = (target_bytes * 9).div_ceil(10);
        assert!(window_saturated(&client, 0, bytes_90));
    }

    // ── adaptive bandwidth ──

    fn sample(current: u8, budget: f32, observed: f32) -> RateSample {
        RateSample {
            ceiling: 120,
            current,
            budget_bytes: budget,
            observed_bytes: observed,
            congested: false,
            app_limited: false,
        }
    }

    #[test]
    fn an_app_limited_link_recovers_instead_of_degrading() {
        // Over budget on paper, but nothing on the path is straining.  The
        // budget is self-measured from our own traffic, so acting on it
        // would be the spinner death spiral: walk back toward the ceiling
        // instead.
        let mut s = sample(180, 1_000.0, 30_000.0);
        s.app_limited = true;
        assert_eq!(next_quantizer(s), 180 - ADAPTIVE_STEP);
        // Already at the ceiling: nothing to buy, hold.
        s.current = 120;
        assert_eq!(next_quantizer(s), 120);
        // Congestion outranks app-limited (they are mutually exclusive in
        // the caller, but the sample must not be trusted to be coherent).
        s.current = 180;
        s.congested = true;
        assert!(next_quantizer(s) > 180);
    }

    #[test]
    fn adaptive_bandwidth_never_spends_above_the_ceiling() {
        // Deep inside budget: the controller wants to improve, but the
        // configured ceiling is the best it may ever ask for.
        assert_eq!(next_quantizer(sample(120, 100_000.0, 1_000.0)), 120);
        // A current value below the ceiling (stale state) is pulled back up.
        assert_eq!(next_quantizer(sample(40, 100_000.0, 1_000.0)), 120);
    }

    #[test]
    fn adaptive_bandwidth_backs_off_when_over_budget_and_returns_when_under() {
        let over = next_quantizer(sample(140, 10_000.0, 30_000.0));
        assert_eq!(over, 140 + ADAPTIVE_STEP);
        let under = next_quantizer(sample(140, 30_000.0, 10_000.0));
        assert_eq!(under, 140 - ADAPTIVE_STEP);
        // On budget: hold, so the loop settles instead of hunting.
        assert_eq!(next_quantizer(sample(140, 10_000.0, 10_000.0)), 140);
    }

    #[test]
    fn adaptive_bandwidth_decreases_multiplicatively_when_congested() {
        let mut s = sample(160, 10_000.0, 1_000.0);
        s.congested = true;
        // Congestion outranks "comfortably inside budget": the queue is
        // already forming, so back off rather than improve.
        assert!(next_quantizer(s) > 160 + ADAPTIVE_STEP);
        // And never past the floor of usable picture.
        s.current = ADAPTIVE_MAX_QUANTIZER;
        assert_eq!(next_quantizer(s), ADAPTIVE_MAX_QUANTIZER);
    }

    #[test]
    fn blocked_write_backoff_is_held_before_quality_recovers() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 240.0;
        let sid = 1;
        let ceiling = SurfaceBandwidth::Medium.av1_quantizer() as u8;
        client.surface_subs.entry(sid).or_default().frame_bytes = 200_000.0;

        let started = Instant::now();
        client
            .write_blocked_us
            .store(WRITE_BLOCKED_CONGESTED_US + 1, Ordering::Relaxed);
        let backed_off =
            step_adaptive_bandwidth(&mut client, SurfaceBandwidth::Medium, sid, started, false)
                .quantizer
                .expect("a blocked write must back off");
        assert!(backed_off > ceiling);

        // Even if the controller is allowed to run immediately, a drained
        // socket must not be mistaken for permission to refill the queue.
        client.surface_subs.entry(sid).or_default().rate_stepped_at = None;
        let held = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            sid,
            started + ADAPTIVE_CONGESTION_HOLD / 2,
            false,
        );
        assert!(held.quantizer.is_none());
        assert_eq!(
            client.surface_subs[&sid].adaptive_quantizer,
            Some(backed_off)
        );

        // After the hold, probe quality upward by one ordinary step.  The
        // recovery cadence is deliberately slower than congestion backoff.
        client.surface_subs.entry(sid).or_default().rate_stepped_at = None;
        let recovered = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            sid,
            started + ADAPTIVE_CONGESTION_HOLD + Duration::from_millis(1),
            false,
        )
        .quantizer
        .expect("quality should recover after the hold");
        assert_eq!(recovered, backed_off - ADAPTIVE_STEP);

        let too_soon = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            sid,
            started + ADAPTIVE_CONGESTION_HOLD + ADAPTIVE_STEP_INTERVAL + Duration::from_millis(1),
            false,
        );
        assert!(too_soon.quantizer.is_none());
    }

    #[test]
    fn adaptive_bandwidth_holds_without_measurements() {
        // No goodput estimate yet, or no frame measured: guessing here would
        // degrade a link that may be perfectly healthy.
        assert_eq!(next_quantizer(sample(150, 0.0, 10_000.0)), 150);
        assert_eq!(next_quantizer(sample(150, 10_000.0, 0.0)), 150);
    }

    #[test]
    fn a_self_measured_budget_can_never_ask_for_better() {
        // `surface_budget_bytes` is `goodput * ADAPTIVE_GOODPUT_SHARE / fps`,
        // and on a link that is not the bottleneck `goodput` converges to
        // exactly what we sent: `frame_bytes * fps`.  So observed/budget is
        // pinned at `1/ADAPTIVE_GOODPUT_SHARE` = 1.25 however bad the picture
        // gets — the ratio carries no information about quality at all.
        // 1.25 is neither `> 1.25` nor `< 0.75`, so from a steady state the
        // budget arm can only hold.  The improve arm needs the ratio under
        // 0.75, i.e. sending 1.67x faster than the pacer allows: unreachable.
        for q in [130u8, 170, ADAPTIVE_MAX_QUANTIZER] {
            let observed = 20_000.0;
            assert_eq!(
                next_quantizer(RateSample {
                    ceiling: 120,
                    current: q,
                    budget_bytes: observed * ADAPTIVE_GOODPUT_SHARE,
                    observed_bytes: observed,
                    congested: false,
                    app_limited: false,
                }),
                q,
                "self-measured budget moved the rate at q={q}",
            );
        }
    }

    #[test]
    fn a_busy_terminal_strands_video_quality_at_the_floor() {
        // `browser_backlog_frames` is a *terminal* metric: it counts
        // applied-but-unpainted terminal frames and is cleared only when a
        // terminal paints.  `surface_pacing_fps` documents at length why
        // video must not be paced off it.  The quality controller reads it
        // anyway, through `app_limited` — and unlike pacing, which is a pure
        // function of live state, the quantizer is latched, so the
        // contamination does not wash out when the burst ends.
        let settled = |backlog: u16| -> u8 {
            let (mut client, _rx) = test_client_with_capacity(64);
            client.display_fps = 60.0;
            client.browser_backlog_frames = backlog;
            // Self-consistent quiet link: goodput is what this surface sends.
            let frame_bytes = 20_000.0;
            client.goodput_bps = frame_bytes * client.display_fps;
            let sub = client.surface_subs.entry(1).or_default();
            sub.frame_bytes = frame_bytes;
            sub.adaptive_quantizer = Some(ADAPTIVE_MAX_QUANTIZER);
            // Far more steps than the ~14 it takes to walk 200 -> 120 at
            // ADAPTIVE_STEP, and the picture is moving throughout.
            for _ in 0..200 {
                client.surface_subs.entry(1).or_default().rate_stepped_at = None;
                step_adaptive_bandwidth(
                    &mut client,
                    SurfaceBandwidth::Medium,
                    1,
                    Instant::now(),
                    false,
                );
            }
            client.surface_subs[&1]
                .adaptive_quantizer
                .unwrap_or(SurfaceBandwidth::Medium.av1_quantizer() as u8)
        };
        let ceiling = SurfaceBandwidth::Medium.av1_quantizer() as u8;
        assert_eq!(settled(0), ceiling, "quiet terminal: walks back up");
        assert_eq!(
            settled(20),
            ceiling,
            "a busy terminal must not strand an unrelated video surface at \
             the quality floor",
        );
    }

    /// Walk a degraded surface for long enough to reach the ceiling if it
    /// ever can, and report where it settled.
    fn settle_quantizer(client: &mut ClientState, sid: u16) -> u8 {
        for _ in 0..200 {
            client.surface_subs.entry(sid).or_default().rate_stepped_at = None;
            step_adaptive_bandwidth(client, SurfaceBandwidth::Medium, sid, Instant::now(), false);
        }
        client.surface_subs[&sid]
            .adaptive_quantizer
            .unwrap_or(SurfaceBandwidth::Medium.av1_quantizer() as u8)
    }

    /// A surface degraded to the floor on a 1 s / 60 Hz link, holding the
    /// given multiple of its own bandwidth-delay product in ACK accounting
    /// and with no direct congestion signal anywhere.
    ///
    /// `frame_bytes` is set well over budget deliberately: that is the arm
    /// which can only hold or degrade, so anything that walks back up here
    /// did so through `app_limited` and nothing else.
    fn deep_link_holding(windows_in_flight: f32) -> (ClientState, usize) {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 60.0;
        client.rtt_ms = 1_000.0;
        client.min_rtt_ms = 1_000.0;
        let window = surface_frame_window(&client);
        let now = Instant::now();
        for _ in 0..(window as f32 * windows_in_flight).round() as usize {
            record_surface_frame_sent(&mut client, 1, 1_000, false, now);
        }
        client.goodput_bps = 20_000.0 * client.display_fps;
        let sub = client.surface_subs.entry(1).or_default();
        sub.frame_bytes = 60_000.0;
        sub.adaptive_quantizer = Some(ADAPTIVE_MAX_QUANTIZER);
        (client, window)
    }

    #[test]
    fn a_deep_but_healthy_link_recovers() {
        // 1 s RTT at 60 Hz legitimately parks ~60 frames in flight.  Against
        // the old flat threshold of 32 that read as strained forever, so
        // `app_limited` never fired — and the budget arm can only hold or
        // degrade, so the quantizer was stranded at the floor for the life
        // of the subscription.  Distance is not congestion.
        let (mut client, window) = deep_link_holding(0.85);
        assert!(
            window > SURFACE_INFLIGHT_MIN / 2,
            "fixture must be deeper than the old flat threshold, window={window}",
        );
        assert_eq!(
            settle_quantizer(&mut client, 1),
            SurfaceBandwidth::Medium.av1_quantizer() as u8,
            "a healthy link at its bandwidth-delay product must walk back up",
        );
    }

    #[test]
    fn batched_acks_past_the_bandwidth_delay_product_do_not_degrade_quality() {
        // ACK callbacks can batch behind a JavaScript long task by more than
        // a whole path BDP.  With no writer/outbox/decoder pressure that is
        // still not evidence that lowering video quality will help.
        let (mut client, _) = deep_link_holding(2.0);
        assert_eq!(
            settle_quantizer(&mut client, 1),
            SurfaceBandwidth::Medium.av1_quantizer() as u8,
            "ACK batching alone must not strand quality at the floor",
        );
    }

    #[test]
    fn decoder_backlog_is_real_surface_pressure() {
        let (mut client, _) = deep_link_holding(0.0);
        force_decoder_pressure(&mut client, 1, SURFACE_DECODE_QUEUE_ALLOWANCE + 1);
        assert_eq!(
            settle_quantizer(&mut client, 1),
            ADAPTIVE_MAX_QUANTIZER,
            "an explicitly backlogged decoder must not probe quality upward",
        );
    }

    #[test]
    fn a_full_surface_delivery_window_does_not_fake_congestion() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 120.0;
        client.rtt_ms = 50.0;
        client.min_rtt_ms = 50.0;
        client.surface_goodput_bps = 3_000_000.0;
        let sid = 1;
        let sub = client.surface_subs.entry(sid).or_default();
        sub.frame_bytes = 800_000.0;
        sub.max_inflight_frames = Some(3);
        for _ in 0..2 {
            record_surface_frame_sent(&mut client, sid, 800_000, false, Instant::now());
        }
        assert!(!surface_frame_credit_open_for(&client, sid, 800_000));

        let ceiling = SurfaceBandwidth::Medium.av1_quantizer() as u8;
        let step = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            sid,
            Instant::now(),
            false,
        );
        assert!(step.quantizer.is_none());
        assert_eq!(client.surface_subs[&sid].adaptive_quantizer, None);
        assert_eq!(
            resolve_bandwidth(&client, SurfaceBandwidth::Medium, sid).av1_quantizer(),
            usize::from(ceiling),
        );
    }

    #[test]
    fn oversized_frames_adapt_resolution_at_the_quantizer_floor() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 120.0;
        client.rtt_ms = 50.0;
        client.min_rtt_ms = 50.0;
        client.surface_goodput_bps = 130_000.0;
        let sid = 1;
        let sub = client.surface_subs.entry(sid).or_default();
        sub.frame_bytes = 40_000.0;
        sub.max_inflight_frames = Some(8);
        sub.adaptive_quantizer = Some(ADAPTIVE_MAX_QUANTIZER);
        sub.scaled_target = Some((3_400, 2_424));
        sub.allow_adaptive_scale = true;
        client
            .write_blocked_us
            .store(WRITE_BLOCKED_CONGESTED_US + 1, Ordering::Relaxed);

        let step = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            sid,
            Instant::now(),
            false,
        );

        assert!(step.target_changed);
        assert_eq!(
            client.surface_subs[&sid].adaptive_scale_shift,
            ADAPTIVE_MAX_SCALE_SHIFT
        );
        assert_eq!(client.surface_subs[&sid].last_encoded_gen, None);
        assert!(!client.surface_subs[&sid].has_keyframe);
    }

    #[test]
    fn a_still_surface_recovers_its_full_resolution() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 120.0;
        client.surface_goodput_bps = 10_000_000.0;
        let sid = 1;
        let started = Instant::now();
        let sub = client.surface_subs.entry(sid).or_default();
        sub.frame_bytes = 1_000.0;
        sub.adaptive_quantizer = Some(ADAPTIVE_MAX_QUANTIZER);
        sub.adaptive_scale_shift = ADAPTIVE_MAX_SCALE_SHIFT;
        sub.scale_stepped_at = Some(started);
        sub.adaptive_pressure_at = Some(started);
        sub.scaled_target = Some((3_400, 2_424));
        sub.allow_adaptive_scale = true;

        for step in 1..=ADAPTIVE_MAX_SCALE_SHIFT {
            let result = step_adaptive_bandwidth(
                &mut client,
                SurfaceBandwidth::Medium,
                sid,
                started + ADAPTIVE_SCALE_RECOVERY_INTERVAL * u32::from(step),
                true,
            );
            assert!(result.target_changed);
            assert_eq!(
                client.surface_subs[&sid].adaptive_scale_shift,
                ADAPTIVE_MAX_SCALE_SHIFT - step,
            );
        }
    }

    #[test]
    fn literal_scaled_subscriptions_do_not_adapt_their_requested_extent() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 120.0;
        client.rtt_ms = 50.0;
        client.min_rtt_ms = 50.0;
        client.surface_goodput_bps = 130_000.0;
        let sid = 1;
        let sub = client.surface_subs.entry(sid).or_default();
        sub.frame_bytes = 40_000.0;
        sub.max_inflight_frames = Some(8);
        sub.adaptive_quantizer = Some(ADAPTIVE_MAX_QUANTIZER);
        sub.scaled_target = Some((314, 176));
        record_surface_frame_sent(&mut client, sid, 40_000, false, Instant::now());

        let step = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            sid,
            Instant::now(),
            false,
        );

        assert!(!step.target_changed);
        assert_eq!(client.surface_subs[&sid].adaptive_scale_shift, 0);
    }

    #[test]
    fn a_healthy_full_decoder_window_does_not_adapt_resolution() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 120.0;
        client.rtt_ms = 50.0;
        client.min_rtt_ms = 50.0;
        client.surface_goodput_bps = 150_000.0;
        let sid = 1;
        let sub = client.surface_subs.entry(sid).or_default();
        sub.frame_bytes = 1_000.0;
        sub.max_inflight_frames = Some(8);
        sub.adaptive_quantizer = Some(ADAPTIVE_MAX_QUANTIZER);
        for _ in 0..8 {
            record_surface_frame_sent(&mut client, sid, 1_000, false, Instant::now());
        }

        let step = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            sid,
            Instant::now(),
            false,
        );

        assert!(!step.target_changed);
        assert_eq!(client.surface_subs[&sid].adaptive_scale_shift, 0);
    }

    #[test]
    fn adaptive_target_reduces_both_axes_and_keeps_encoder_parity() {
        assert_eq!(adaptive_surface_target(3_400, 2_424, 0), (3_400, 2_424));
        assert_eq!(adaptive_surface_target(3_400, 2_424, 1), (1_700, 1_212));
        assert_eq!(adaptive_surface_target(3_400, 2_424, 3), (424, 302));
        assert_eq!(adaptive_surface_target(2, 2, 3), (2, 2));
    }

    #[test]
    fn surface_budget_splits_by_measured_share() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.goodput_bps = 1_000_000.0;
        client.display_fps = 10.0;
        client.surface_subs.entry(1).or_default().frame_bytes = 30_000.0;
        client.surface_subs.entry(2).or_default().frame_bytes = 10_000.0;
        let big = surface_budget_bytes(&client, 1);
        let small = surface_budget_bytes(&client, 2);
        assert!(big > small, "big={big} small={small}");
        assert!(
            (big / small - 3.0).abs() < 0.01,
            "3:1 split, got {big}/{small}"
        );
    }

    // ── surface pacing is independent of terminal backlog ──

    #[test]
    fn surface_pacing_ignores_terminal_backlog() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 60.0;
        client.surface_subs.entry(1).or_default();

        let clean = surface_pacing_fps(&client, 1);

        // A burst of shell output backs the terminal's paint loop up.  That
        // is what `browser_pacing_fps` reads, and it must not reach video.
        client.browser_backlog_frames = 20;
        client.browser_ack_ahead_frames = 20;
        assert!(
            browser_pacing_fps(&client) < clean,
            "precondition: terminal pacing should back off here"
        );
        assert_eq!(surface_pacing_fps(&client, 1), clean);
    }

    #[test]
    fn surface_pacing_ignores_ack_depth() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 60.0;
        client.rtt_ms = 1.0;
        client.min_rtt_ms = 1.0;
        client.surface_subs.entry(1).or_default();

        let clean = surface_pacing_fps(&client, 1);
        let now = Instant::now();
        for _ in 0..surface_inflight_cap(&client) {
            record_surface_frame_sent(&mut client, 1, 1_000, false, now);
        }
        assert_eq!(surface_pacing_fps(&client, 1), clean);
        assert!(!surface_delivery_is_throttled(&client, 1));
    }

    #[test]
    fn full_rate_surface_delivery_uses_the_source_clock() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 240.0;
        client.rtt_ms = 8.0;
        client.min_rtt_ms = 8.0;
        client.surface_subs.entry(1).or_default();

        assert_eq!(surface_pacing_fps(&client, 1), 240.0);
        assert!(!surface_delivery_is_throttled(&client, 1));
    }

    #[test]
    fn transient_decoder_queue_batch_does_not_throttle() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 240.0;
        let now = Instant::now();
        let high = SURFACE_DECODE_QUEUE_ALLOWANCE + 1;

        let sub = client.surface_subs.entry(1).or_default();
        update_surface_decoder_queue(sub, high, now);
        update_surface_decoder_queue(
            sub,
            high,
            now + SURFACE_DECODE_PRESSURE_GRACE - Duration::from_millis(1),
        );
        assert_eq!(sub.decoder_queue_depth, high);
        assert_eq!(sub.decoder_pressure_depth, 0);
        assert_eq!(surface_pacing_fps(&client, 1), 240.0);

        update_surface_decoder_queue(
            client.surface_subs.get_mut(&1).unwrap(),
            high,
            now + SURFACE_DECODE_PRESSURE_GRACE,
        );
        assert_eq!(
            client.surface_subs.get(&1).unwrap().decoder_pressure_depth,
            high
        );
        assert_eq!(surface_pacing_fps(&client, 1), 240.0);

        update_surface_decoder_queue(
            client.surface_subs.get_mut(&1).unwrap(),
            SURFACE_DECODE_QUEUE_ALLOWANCE,
            now + SURFACE_DECODE_PRESSURE_GRACE,
        );
        assert_eq!(surface_pacing_fps(&client, 1), 240.0);
    }

    #[test]
    fn surface_pacing_ignores_browser_ack_batching_at_any_refresh_rate() {
        let (mut client, _rx) = test_client_with_capacity(64);
        for fps in [60.0, 145.0, 240.0, 1_000.0] {
            client.surface_inflight_frames.clear();
            client.surface_inflight_bytes = 0;
            client.display_fps = fps;
            client.rtt_ms = 6.0;
            client.min_rtt_ms = 6.0;
            client.surface_subs.entry(1).or_default();

            let now = Instant::now();
            for _ in 0..surface_inflight_cap(&client) {
                record_surface_frame_sent(&mut client, 1, 1_000, false, now);
            }
            assert_eq!(surface_pacing_fps(&client, 1), fps);
        }
    }

    #[test]
    fn decoder_pressure_does_not_lower_surface_cadence() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 145.0;
        client.rtt_ms = 200.0;
        client.min_rtt_ms = 200.0;
        client.surface_subs.entry(1).or_default();

        force_decoder_pressure(&mut client, 1, SURFACE_DECODE_QUEUE_ALLOWANCE * 2);
        assert_eq!(surface_pacing_fps(&client, 1), client.display_fps);
        assert!(!surface_delivery_is_throttled(&client, 1));
        assert_eq!(
            surface_source_interval(&client, 1),
            Duration::from_secs_f64(1.0 / 145.0)
        );
    }

    #[test]
    fn per_surface_cadence_caps_source_and_delivery() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 120.0;
        let sub = client.surface_subs.entry(1).or_default();
        sub.max_fps = Some(15.0);
        sub.source_interval = Some(Duration::from_secs_f64(1.0 / 15.0));

        assert_eq!(surface_pacing_fps(&client, 1), 15.0);
        assert_eq!(
            surface_source_interval(&client, 1),
            Duration::from_secs_f64(1.0 / 15.0)
        );
        assert!(!surface_delivery_is_throttled(&client, 1));

        // A second viewer can drive the shared application clock at 120 Hz;
        // this 15 Hz subscriber must then pace its own delivery.
        client.surface_subs.get_mut(&1).unwrap().source_interval =
            Some(Duration::from_secs_f64(1.0 / 120.0));
        assert!(surface_delivery_is_throttled(&client, 1));
    }

    #[test]
    fn surface_pacing_tolerates_a_high_rtt_link() {
        // 100 ms RTT at 60 Hz legitimately keeps ~6 frames in flight.  A
        // constant threshold would read that as congestion and halve the
        // rate on a link that is behaving perfectly.
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 60.0;
        client.rtt_ms = 100.0;
        client.min_rtt_ms = 100.0;
        client.surface_subs.entry(1).or_default();

        let now = Instant::now();
        for _ in 0..6 {
            record_surface_frame_sent(&mut client, 1, 1_000, false, now);
        }
        assert_eq!(surface_pacing_fps(&client, 1), 60.0);
    }

    #[test]
    fn decoder_pressure_on_one_surface_does_not_limit_any_cadence() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 60.0;
        client.rtt_ms = 1.0;
        client.min_rtt_ms = 1.0;
        client.surface_subs.entry(1).or_default();
        client.surface_subs.entry(2).or_default();

        force_decoder_pressure(&mut client, 1, SURFACE_DECODE_QUEUE_ALLOWANCE * 2);
        // Pressure affects surface 1's quality, not either cadence.
        assert_eq!(surface_pacing_fps(&client, 1), 60.0);
        assert_eq!(surface_pacing_fps(&client, 2), 60.0);
    }

    #[test]
    fn surface_pacing_never_reaches_zero() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.display_fps = 60.0;
        client.surface_subs.entry(1).or_default();
        force_decoder_pressure(&mut client, 1, u8::MAX);
        assert_eq!(surface_pacing_fps(&client, 1), 60.0);
    }

    #[test]
    fn surface_inflight_cap_stays_above_the_window() {
        // The backoff compares against BDP plus browser scheduling slack, so
        // the tracking queue has to hold more than that whole window or the
        // controller is silently inert.  This is what a flat cap of 64 got
        // wrong at high refresh rates and long RTTs.
        for (rtt, fps) in [
            (1.0f32, 60.0f32),
            (100.0, 60.0),
            (500.0, 60.0),
            (500.0, 240.0),
            (1000.0, 60.0),
            (1000.0, 120.0),
            (1000.0, 1000.0),
            (2000.0, 1000.0),
        ] {
            let (mut client, _rx) = test_client_with_capacity(64);
            client.rtt_ms = rtt;
            client.min_rtt_ms = rtt;
            client.display_fps = fps;
            let window =
                surface_frame_window(&client) + surface_ack_tracking_frames(client.display_fps);
            let cap = surface_inflight_cap(&client);
            assert!(
                cap > window,
                "rtt={rtt} fps={fps}: cap {cap} must exceed window {window}"
            );
        }
    }

    #[test]
    fn surface_inflight_cap_scales_with_subscribed_surfaces() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.rtt_ms = 1.0;
        client.min_rtt_ms = 1.0;
        client.display_fps = 240.0;
        let one = surface_inflight_cap(&client);
        client.surface_subscriptions.extend([1, 2]);
        let two = surface_inflight_cap(&client);
        assert_eq!(two, one * 2);
    }

    #[test]
    fn ack_tracking_window_moves_with_rtt() {
        // ACK history is accounting storage, not a pacing threshold.  It
        // still has to grow with RTT so a deep healthy link can match the
        // acknowledgements it receives.
        let deep = {
            let (mut c, _rx) = test_client_with_capacity(64);
            c.rtt_ms = 1000.0;
            c.min_rtt_ms = 1000.0;
            c.display_fps = 60.0;
            c
        };
        let near = {
            let (mut c, _rx) = test_client_with_capacity(64);
            c.rtt_ms = 1.0;
            c.min_rtt_ms = 1.0;
            c.display_fps = 60.0;
            c
        };
        assert!(surface_frame_window(&deep) > surface_frame_window(&near) * 4);
        assert!(surface_inflight_cap(&deep) > surface_inflight_cap(&near));
    }

    #[test]
    fn surface_inflight_cap_is_bounded() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.rtt_ms = 60_000.0;
        client.min_rtt_ms = 60_000.0;
        client.display_fps = 480.0;
        assert_eq!(surface_inflight_cap(&client), SURFACE_INFLIGHT_HARD_MAX);
    }

    #[test]
    fn surface_cadence_is_independent_of_one_second_rtt_and_decoder_depth() {
        // Neither distance nor a standing decoder pipeline is a reason to
        // discard frames. Decoder pressure is handled by quality instead.
        let (mut client, _rx) = test_client_with_capacity(64);
        client.rtt_ms = 1000.0;
        client.min_rtt_ms = 1000.0;
        client.display_fps = 60.0;
        client.surface_subs.entry(1).or_default();

        let now = Instant::now();
        for _ in 0..surface_frame_window(&client) * 2 {
            record_surface_frame_sent(&mut client, 1, 1_000, false, now);
        }
        assert_eq!(surface_pacing_fps(&client, 1), 60.0);

        force_decoder_pressure(&mut client, 1, SURFACE_DECODE_QUEUE_ALLOWANCE * 2);
        assert_eq!(surface_pacing_fps(&client, 1), 60.0);
        assert!(!surface_delivery_is_throttled(&client, 1));
    }

    #[test]
    fn surface_acks_are_matched_to_their_own_surface() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.surface_subs.entry(1).or_default();
        client.surface_subs.entry(2).or_default();
        let now = Instant::now();
        record_surface_frame_sent(&mut client, 1, 1_000, false, now);
        record_surface_frame_sent(&mut client, 2, 2_000, false, now);
        // Surface 2 acks first (its frame is smaller on the wire, or its
        // decoder is faster).  The queue must give up surface 2's entry, not
        // the one at the front.
        record_surface_ack(&mut client, 2);
        assert_eq!(client.surface_inflight_frames.len(), 1);
        assert_eq!(client.surface_inflight_frames[0].surface_id, 1);
    }

    #[test]
    fn rejected_native_surface_frame_returns_credit_without_goodput() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.surface_subs.entry(1).or_default();
        client.surface_subs.entry(2).or_default();
        let now = Instant::now();
        record_surface_frame_sent(&mut client, 1, 1_000, false, now);
        record_surface_frame_sent(&mut client, 2, 2_000, false, now);
        record_surface_frame_sent(&mut client, 1, 3_000, false, now);

        discard_surface_frame(&mut client, 1);

        assert_eq!(client.surface_inflight_bytes, 3_000);
        assert_eq!(client.acked_bytes_since_log, 0);
        assert_eq!(client.surface_goodput_window_bytes, 0);
        assert_eq!(client.surface_inflight_frames.len(), 2);
        assert_eq!(client.surface_inflight_frames[0].surface_id, 1);
        assert_eq!(client.surface_inflight_frames[0].bytes, 1_000);
        assert_eq!(client.surface_inflight_frames[1].surface_id, 2);
    }

    #[test]
    fn adaptive_step_reports_a_quantizer_with_no_local_encoder() {
        // A Vulkan surface has no `SurfaceEncoder` on the server side, so
        // the step used to fall out silently.  It must still report where
        // the rate moved to, because that number is what gets forwarded to
        // the compositor's session for this client.
        let (mut client, _rx) = test_client_with_capacity(64);
        client.goodput_bps = 10_000.0;
        client.display_fps = 30.0;
        // Degrading needs direct pressure evidence, not just a budget
        // verdict.  Use the decoder's own reported queue depth.
        let sub = client.surface_subs.entry(4).or_default();
        sub.frame_bytes = 60_000.0;
        sub.decoder_queue_depth = SURFACE_DECODE_QUEUE_ALLOWANCE + 1;
        sub.decoder_pressure_depth = SURFACE_DECODE_QUEUE_ALLOWANCE + 1;
        assert!(sub.encoder.is_none());
        let step = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            4,
            Instant::now(),
            false,
        );
        let q = step
            .quantizer
            .expect("over budget by 20x must move the rate");
        assert!(
            q > SurfaceBandwidth::Medium.av1_quantizer() as u8,
            "cheaper than the ceiling, got {q}",
        );
        // Nothing to rebuild: the compositor retargets in place.
        assert!(!step.rebuild);
    }

    #[test]
    fn a_lone_animation_on_a_quiet_link_is_not_walked_to_the_floor() {
        // The spinner case: tiny frames, forever changing, link otherwise
        // idle.  Goodput has collapsed to the spinner's own send rate, so
        // every frame reads as "over budget" — but nothing is congested,
        // nothing is backlogged, nothing is in flight.  The controller
        // must walk back toward the ceiling, not away from it.
        let (mut client, _rx) = test_client_with_capacity(64);
        client.goodput_bps = 50_000.0;
        client.display_fps = 60.0;
        let ceiling = SurfaceBandwidth::Medium.av1_quantizer() as u8;
        let sub = client.surface_subs.entry(7).or_default();
        sub.frame_bytes = 1_700.0;
        sub.adaptive_quantizer = Some(ADAPTIVE_MAX_QUANTIZER);

        let mut q = ADAPTIVE_MAX_QUANTIZER;
        for _ in 0..40 {
            client.surface_subs.entry(7).or_default().rate_stepped_at = None;
            let step = step_adaptive_bandwidth(
                &mut client,
                SurfaceBandwidth::Medium,
                7,
                Instant::now(),
                false,
            );
            match step.quantizer {
                Some(next) => {
                    assert!(next < q, "must only improve, {q} -> {next}");
                    q = next;
                }
                None => break,
            }
        }
        assert_eq!(q, ceiling, "must recover all the way to the ceiling");
        assert_eq!(
            resolve_bandwidth(&client, SurfaceBandwidth::Medium, 7).av1_quantizer(),
            ceiling as usize,
        );
    }

    #[test]
    fn a_frozen_picture_is_refined_back_to_the_ceiling() {
        // Whatever the controller backed off to during motion is what the
        // client is left staring at once the screen stops.  Walk it back.
        let ceiling = SurfaceBandwidth::Medium.av1_quantizer() as u8;
        let mut q = ADAPTIVE_MAX_QUANTIZER;
        let mut steps = 0;
        while q > ceiling {
            let next = refine_toward_ceiling(q, ceiling);
            assert!(next < q, "must improve, {q} -> {next}");
            assert!(next >= ceiling, "must not overshoot the ceiling: {next}");
            q = next;
            steps += 1;
            assert!(
                steps < 12,
                "converging too slowly, every step is a keyframe"
            );
        }
        assert_eq!(q, ceiling);
        // At the ceiling there is nothing left to buy.
        assert_eq!(refine_toward_ceiling(ceiling, ceiling), ceiling);
    }

    #[test]
    fn a_still_surface_ignores_a_stale_frame_size() {
        // `frame_bytes` still describes the motion that just stopped.  Judged
        // against it, a surface that had been over budget would keep getting
        // worse while nothing at all is being sent.
        let (mut client, _rx) = test_client_with_capacity(64);
        client.goodput_bps = 10_000.0;
        client.display_fps = 30.0;
        // The decoder is explicitly backlogged, so the moving half of the
        // contrast is genuinely strained (an unstrained link would recover).
        let ceiling = SurfaceBandwidth::Medium.av1_quantizer() as u8;
        let sub = client.surface_subs.entry(9).or_default();
        sub.frame_bytes = 60_000.0;
        sub.adaptive_quantizer = Some(150);
        sub.decoder_queue_depth = SURFACE_DECODE_QUEUE_ALLOWANCE + 1;
        sub.decoder_pressure_depth = SURFACE_DECODE_QUEUE_ALLOWANCE + 1;

        let moving = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            9,
            Instant::now(),
            false,
        );
        assert!(
            moving.quantizer.is_some_and(|q| q > 150),
            "over budget while moving: get cheaper, got {:?}",
            moving.quantizer,
        );

        client.surface_subs.entry(9).or_default().adaptive_quantizer = Some(150);
        client.surface_subs.entry(9).or_default().rate_stepped_at = None;
        client
            .surface_subs
            .entry(9)
            .or_default()
            .decoder_queue_depth = 0;
        client
            .surface_subs
            .entry(9)
            .or_default()
            .decoder_pressure_depth = 0;
        let still = step_adaptive_bandwidth(
            &mut client,
            SurfaceBandwidth::Medium,
            9,
            Instant::now(),
            true,
        );
        let q = still.quantizer.expect("a frozen picture must be refined");
        assert!(
            q < 150 && q >= ceiling,
            "same stale bytes, opposite direction: {q}"
        );
    }

    #[test]
    fn a_surface_that_was_sent_nothing_owes_a_keyframe() {
        // A subscription with no state yet, and one whose state exists but
        // has never carried a keyframe, are the same thing to a decoder:
        // there is no reference frame, so a delta is undecodable.
        let (mut client, _rx) = test_client_with_capacity(64);
        assert!(owes_keyframe(&client, 3), "no sub state at all");
        client.surface_subs.entry(3).or_default();
        assert!(owes_keyframe(&client, 3), "sub state, no keyframe yet");
        client.surface_subs.entry(3).or_default().has_keyframe = true;
        assert!(!owes_keyframe(&client, 3));
    }

    #[test]
    fn duplicate_keyframe_requests_are_coalesced() {
        let start = Instant::now();
        let mut sub = SurfaceSubState::default();

        assert!(request_surface_keyframe(&mut sub, start, true));
        assert!(!sub.has_keyframe, "the accepted request creates debt");

        let mut sub = SurfaceSubState {
            has_keyframe: true,
            ..Default::default()
        };
        assert!(request_surface_keyframe(&mut sub, start, false));
        sub.has_keyframe = true; // the requested IDR reached the client
        sub.decoder_queue_depth = 7;
        assert!(!request_surface_keyframe(
            &mut sub,
            start + SURFACE_KEYFRAME_REQUEST_INTERVAL - Duration::from_millis(1),
            false,
        ));
        assert_eq!(
            sub.decoder_queue_depth, 7,
            "a suppressed duplicate must not erase decoder pressure",
        );
        assert!(request_surface_keyframe(
            &mut sub,
            start + SURFACE_KEYFRAME_REQUEST_INTERVAL,
            false,
        ));
    }

    #[test]
    fn meaningful_subscribe_bypasses_keyframe_cooldown() {
        let start = Instant::now();
        let mut sub = SurfaceSubState {
            has_keyframe: true,
            last_keyframe_request_at: Some(start),
            ..SurfaceSubState::default()
        };

        assert!(request_surface_keyframe(
            &mut sub,
            start + Duration::from_millis(1),
            true,
        ));
        assert!(!sub.has_keyframe);
    }

    #[test]
    fn completed_keyframe_settles_queued_recovery_debt() {
        assert!(chained_encode_needs_keyframe(true, false, false));
        assert!(
            !chained_encode_needs_keyframe(true, false, true),
            "the queued frame must not become a redundant second IDR",
        );
        assert!(
            chained_encode_needs_keyframe(true, true, true),
            "an explicit still-image quality refresh remains distinct",
        );
        assert!(!chained_encode_needs_keyframe(false, false, true));
    }

    #[test]
    fn handoff_only_queues_a_strictly_newer_generation() {
        assert!(pending_generation_is_newer(Some(10), None, 11));
        assert!(
            !pending_generation_is_newer(Some(10), None, 10),
            "the in-flight pixels must not be encoded twice",
        );
        assert!(
            !pending_generation_is_newer(Some(10), Some(12), 11),
            "an older candidate must not replace the freshest pending frame",
        );
        assert!(!pending_generation_is_newer(Some(10), Some(12), 12));
        assert!(pending_generation_is_newer(Some(10), Some(12), 13));
    }

    #[test]
    fn revisiting_one_display_frame_does_not_make_a_surface_still() {
        let mut sub = SurfaceSubState::default();
        let start = Instant::now();
        assert!(!source_generation_is_still(&mut sub, 10, start));
        assert!(!source_generation_is_still(
            &mut sub,
            10,
            start + STILL_REFRESH_INTERVAL - Duration::from_millis(1),
        ));

        let changed = start + STILL_REFRESH_INTERVAL;
        assert!(
            !source_generation_is_still(&mut sub, 11, changed),
            "a fresh compositor generation resets the stillness clock",
        );
        assert!(!source_generation_is_still(
            &mut sub,
            11,
            changed + STILL_REFRESH_INTERVAL - Duration::from_millis(1),
        ));
        assert!(source_generation_is_still(
            &mut sub,
            11,
            changed + STILL_REFRESH_INTERVAL,
        ));
    }

    #[test]
    fn one_surfaces_keyframe_does_not_settle_anothers_debt() {
        // The flag used to live on the client, so the first surface to
        // deliver a keyframe cleared it for every other surface still
        // waiting on one — those surfaces then got deltas against a
        // reference their decoder never received.
        let (mut client, _rx) = test_client_with_capacity(64);
        for sid in [1u16, 2] {
            client.surface_subs.entry(sid).or_default();
        }
        assert!(owes_keyframe(&client, 1) && owes_keyframe(&client, 2));

        // Surface 1 gets its keyframe.  Surface 2 is untouched by that.
        client.surface_subs.entry(1).or_default().has_keyframe = true;
        assert!(!owes_keyframe(&client, 1));
        assert!(
            owes_keyframe(&client, 2),
            "surface 2 never received a keyframe of its own",
        );

        // And the reverse: breaking surface 2's chain leaves surface 1's
        // intact, so one surface resizing does not cost every other surface
        // an unnecessary keyframe.
        client.surface_subs.entry(2).or_default().has_keyframe = true;
        client.surface_subs.entry(2).or_default().has_keyframe = false;
        assert!(!owes_keyframe(&client, 1), "surface 1 still has its own");
        assert!(owes_keyframe(&client, 2));
    }

    #[test]
    fn dropping_a_subscription_drops_its_keyframe_standing() {
        // `surface_subs` entries are removed wholesale on UNSUBSCRIBE and
        // SurfaceDestroyed.  A later resubscribe reuses the id against a
        // fresh encoder, so it must not inherit the old chain's standing.
        let (mut client, _rx) = test_client_with_capacity(64);
        client.surface_subs.entry(8).or_default().has_keyframe = true;
        assert!(!owes_keyframe(&client, 8));
        client.surface_subs.remove(&8);
        assert!(owes_keyframe(&client, 8), "a reused id starts over");
    }

    #[test]
    fn destroyed_surface_retires_every_client_claim_for_reused_id() {
        let (mut client, _rx) = test_client_with_capacity(64);
        let now = Instant::now();
        for sid in [8u16, 9] {
            client.surface_subscriptions.insert(sid);
            client.surface_subs.entry(sid).or_default().has_keyframe = true;
            client.surface_view_sizes.insert(sid, (800, 600, 120));
            client.surface_claim_lapses.insert(sid, now);
            record_surface_frame_sent(&mut client, sid, 1024, false, now);
        }
        client.vulkan_video_surfaces.insert(
            8,
            VulkanVideoSurfaceState {
                encoder_name: "test",
                codec_flag: 1,
                width: 800,
                height: 600,
                is_444: false,
            },
        );

        assert!(invalidate_client_surface(&mut client, 8, true));

        assert!(!client.surface_subscriptions.contains(&8));
        assert!(!client.surface_subs.contains_key(&8));
        assert!(!client.surface_view_sizes.contains_key(&8));
        assert!(!client.surface_claim_lapses.contains_key(&8));
        assert!(!client.vulkan_video_surfaces.contains_key(&8));
        assert!(
            client
                .surface_inflight_frames
                .iter()
                .all(|frame| frame.surface_id != 8)
        );

        assert!(client.surface_subscriptions.contains(&9));
        assert!(client.surface_subs.contains_key(&9));
        assert!(client.surface_view_sizes.contains_key(&9));
        assert!(client.surface_claim_lapses.contains_key(&9));
        assert!(
            client
                .surface_inflight_frames
                .iter()
                .any(|frame| frame.surface_id == 9)
        );
        assert_eq!(client.surface_inflight_bytes, 1024);
    }

    #[test]
    fn a_generation_that_encoded_to_nothing_is_not_marked_sent() {
        // `unchanged` reads `last_encoded_gen` as "the client already has
        // this".  An encode that produced no bitstream sent nothing, so
        // claiming its generation strands that frame: the gate skips it on
        // every later tick, and only new pixels ever dislodge it.
        assert_eq!(encoded_generation(Some(4), 5, true), Some(5));
        assert_eq!(
            encoded_generation(Some(4), 5, false),
            Some(4),
            "an empty encode must leave the mark where it was",
        );
        // The first generation on a fresh sub is the one that matters most:
        // there is no earlier frame on screen to fall back to.
        assert_eq!(encoded_generation(None, 5, false), None);

        // The failure this guards, played out.  A surface paints, its last
        // encode comes back empty, and then it goes still — a video reaching
        // its final frame.  The generation must stay re-encodable.
        let unchanged = |mark: Option<u64>, latest: u64| mark == Some(latest);
        let mut mark = Some(11u64);
        mark = encoded_generation(mark, 12, false);
        assert!(
            !unchanged(mark, 12),
            "the last frame must still be owed to the client",
        );
        mark = encoded_generation(mark, 12, true);
        assert!(unchanged(mark, 12), "and settle once it is actually sent");
    }

    #[test]
    fn an_invalidated_encode_completion_is_not_accepted() {
        let mut sub = SurfaceSubState {
            encode_in_flight: true,
            reserved_encode_bytes: 8_192,
            in_flight_generation: Some(12),
            encoder_invalidated: true,
            has_keyframe: false,
            last_encoded_gen: Some(11),
            ..Default::default()
        };

        assert!(
            !accept_completed_encode(&mut sub, 12, true),
            "old-size output must be dropped after a resubscribe",
        );
        assert!(!sub.encode_in_flight);
        assert_eq!(sub.reserved_encode_bytes, 0);
        assert_eq!(sub.in_flight_generation, None);
        assert_eq!(
            sub.last_encoded_gen,
            Some(11),
            "stale output must remain owed to the replacement encoder",
        );
        assert!(!sub.has_keyframe, "stale keyframe cannot pay new debt");
        assert!(!sub.encoder_invalidated);
    }

    #[test]
    fn a_current_encode_completion_advances_the_generation() {
        let mut sub = SurfaceSubState {
            encode_in_flight: true,
            reserved_encode_bytes: 8_192,
            in_flight_generation: Some(12),
            last_encoded_gen: Some(11),
            ..Default::default()
        };

        assert!(accept_completed_encode(&mut sub, 12, true));
        assert!(!sub.encode_in_flight);
        assert_eq!(sub.reserved_encode_bytes, 0);
        assert_eq!(sub.in_flight_generation, None);
        assert_eq!(sub.last_encoded_gen, Some(12));
    }

    #[test]
    fn an_invalidated_creation_is_rejected_before_registration() {
        let mut sub = SurfaceSubState {
            creation_in_flight: true,
            encoder_invalidated: true,
            last_registered_target: Some((256, 184)),
            ..Default::default()
        };

        assert!(!accept_completed_creation(&mut sub));
        assert!(!sub.creation_in_flight);
        assert!(!sub.encoder_invalidated);
        assert_eq!(
            sub.last_registered_target,
            Some((256, 184)),
            "rejection itself must not mutate compositor registration state",
        );
    }

    #[test]
    fn a_current_creation_is_accepted() {
        let mut sub = SurfaceSubState {
            creation_in_flight: true,
            ..Default::default()
        };

        assert!(accept_completed_creation(&mut sub));
        assert!(!sub.creation_in_flight);
    }

    #[test]
    fn a_vulkan_still_is_judged_on_its_own_generation_stream() {
        // A client on a compositor-resident encoder is fed bitstreams, not
        // the pixel snapshot, and the two carry independent generations.
        // Comparing against the wrong one leaves `unchanged` permanently
        // false, so the picture it is left staring at is never refined.
        let mut encoded: HashMap<(u16, u64), u64> = HashMap::new();
        encoded.insert((5, 77), 42);

        let latest = |has_vulkan: bool, px_gen: u64| -> u64 {
            if has_vulkan {
                encoded.get(&(5, 77)).copied().unwrap_or(u64::MAX)
            } else {
                px_gen
            }
        };

        // The pixel stream has moved on past the bitstream this client holds;
        // that says nothing about whether its picture changed.
        assert_eq!(latest(true, 99), 42);
        assert_eq!(latest(false, 99), 99);
        // A session with nothing produced yet must never read as "still":
        // there is no picture on screen to refine.
        assert_eq!(
            HashMap::<(u16, u64), u64>::new()
                .get(&(5, 77))
                .copied()
                .unwrap_or(u64::MAX),
            u64::MAX,
        );
    }

    #[test]
    fn a_refined_still_stops_refining_once_it_is_clean() {
        // The refresh costs a keyframe per step.  Once the picture is at the
        // ceiling there is nothing left to buy, and a controller that keeps
        // reporting a step would spend one every interval forever.
        let (mut client, _rx) = test_client_with_capacity(64);
        client.surface_subs.entry(11).or_default();
        let mut sent = 0;
        for _ in 0..40 {
            client.surface_subs.entry(11).or_default().rate_stepped_at = None;
            let step = step_adaptive_bandwidth(
                &mut client,
                SurfaceBandwidth::Medium,
                11,
                Instant::now(),
                true,
            );
            if step.quantizer.is_none() {
                break;
            }
            sent += 1;
        }
        assert!(sent < 40, "never settled: {sent} keyframes and counting");
        // And it settled at the ceiling, not short of it.
        assert_eq!(
            resolve_bandwidth(&client, SurfaceBandwidth::Medium, 11).av1_quantizer(),
            SurfaceBandwidth::Medium.av1_quantizer(),
        );
    }

    #[test]
    fn a_ceiling_cheaper_than_the_controller_floor_is_still_the_ceiling() {
        // The controller floors at ADAPTIVE_MAX_QUANTIZER, so a surface
        // configured cheaper than that (quantizer 255 = minimum bandwidth)
        // must not be pulled back up to 200 and spend more than allowed.
        let (mut client, _rx) = test_client_with_capacity(64);
        let sub = client.surface_subs.entry(6).or_default();
        sub.bandwidth_override = Some(SurfaceBandwidth::Custom { quantizer: 255 });
        sub.adaptive_quantizer = Some(ADAPTIVE_MAX_QUANTIZER);
        let resolved = resolve_bandwidth(&client, SurfaceBandwidth::Medium, 6);
        assert_eq!(resolved.av1_quantizer(), 255);
    }

    #[test]
    fn a_gone_surface_leaves_no_frames_to_be_acked_later() {
        // Surface ids are recycled, so a stale entry would be matched by a
        // frame minutes later and report a garbage RTT.
        let (mut client, _rx) = test_client_with_capacity(64);
        let now = Instant::now();
        record_surface_frame_sent(&mut client, 1, 1_000, false, now);
        record_surface_frame_sent(&mut client, 2, 1_000, false, now);
        forget_surface_inflight(&mut client, 1);
        assert_eq!(client.surface_inflight_frames.len(), 1);
        assert_eq!(client.surface_inflight_frames[0].surface_id, 2);
    }

    #[test]
    fn compositor_bitstreams_are_dropped_per_surface_not_per_client() {
        let mut last_encoded: HashMap<(u16, u64), LastEncoded> = HashMap::new();
        for key in [(1u16, 10u64), (1, 11), (2, 10)] {
            last_encoded.insert(
                key,
                LastEncoded {
                    width: 8,
                    height: 8,
                    data: Arc::new(Vec::new()),
                    is_keyframe: true,
                    codec_flag: 0,
                    generation: 1,
                    timestamp_ms: 0,
                    timestamp_sub_us: 0,
                },
            );
        }
        // Surface 1 was resized, so every viewer's bitstream for it is
        // stale — but surface 2 is untouched.
        last_encoded_remove_for_sid(&mut last_encoded, 1);
        assert_eq!(last_encoded.len(), 1);
        assert!(last_encoded.contains_key(&(2, 10)));
    }

    #[test]
    fn surface_inflight_queue_is_bounded() {
        let (mut client, _rx) = test_client_with_capacity(64);
        let now = Instant::now();
        let cap = surface_inflight_cap(&client);
        for _ in 0..(cap * 2) {
            record_surface_frame_sent(&mut client, 7, 1_000, false, now);
        }
        assert_eq!(client.surface_inflight_frames.len(), cap);
        assert_eq!(client.surface_inflight_bytes, cap * 1_000);
    }

    #[test]
    fn reset_inflight_clears_unacked_surface_frames() {
        let (mut client, _rx) = test_client_with_capacity(64);
        record_surface_frame_sent(&mut client, 3, 1_000, false, Instant::now());
        reset_inflight(&mut client);
        assert!(client.surface_inflight_frames.is_empty());
        assert_eq!(client.surface_inflight_bytes, 0);
    }

    #[test]
    fn surface_credit_is_shared_across_surfaces() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.rtt_ms = 0.0;
        client.min_rtt_ms = 0.0;
        client.surface_goodput_bps = 100_000.0;

        assert!(surface_credit_open_for(&client, 10_000));
        record_surface_frame_sent(&mut client, 1, 10_000, false, Instant::now());
        assert!(
            !surface_credit_open_for(&client, 1_000),
            "surface 2 must not spend credit already occupied by surface 1",
        );

        record_surface_ack(&mut client, 1);
        assert!(surface_credit_open_for(&client, 1_000));
    }

    #[test]
    fn surface_frame_credit_stops_at_the_negotiated_decoder_window() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.surface_goodput_bps = 1_000_000_000.0;
        client
            .surface_subs
            .entry(1)
            .or_default()
            .max_inflight_frames = Some(3);

        for _ in 0..3 {
            assert!(surface_frame_credit_open_for(&client, 1, 1_000));
            record_surface_frame_sent(&mut client, 1, 1_000, false, Instant::now());
        }
        assert!(!surface_frame_credit_open_for(&client, 1, 1_000));

        record_surface_ack(&mut client, 1);
        assert!(surface_frame_credit_open_for(&client, 1, 1_000));
    }

    #[test]
    fn surface_credit_reserves_two_parallel_encodes_before_they_finish() {
        let mut client = test_client();
        client.rtt_ms = 0.0;
        client.min_rtt_ms = 0.0;
        client.surface_goodput_bps = 100_000.0;
        client
            .surface_subs
            .entry(1)
            .or_default()
            .reserved_encode_bytes = 6_000;

        assert!(surface_credit_open_for(&client, 6_000));
        client
            .surface_subs
            .entry(2)
            .or_default()
            .reserved_encode_bytes = 6_000;
        assert!(!surface_credit_open_for(&client, 6_000));
    }

    #[test]
    fn surface_credit_bootstraps_two_frames_that_fit_the_measured_window() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.rtt_ms = 0.0;
        client.min_rtt_ms = 0.0;
        client.surface_goodput_bps = 200_000.0;

        record_surface_frame_sent(&mut client, 1, 2_000, false, Instant::now());
        assert!(surface_credit_open_for(&client, 10_000));
        record_surface_frame_sent(&mut client, 1, 10_000, false, Instant::now());
        assert!(!surface_credit_open_for(&client, 10_000));
    }

    #[test]
    fn surface_credit_bootstraps_two_oversized_keyframes() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.rtt_ms = 0.0;
        client.min_rtt_ms = 0.0;
        client.surface_goodput_bps = 100_000.0;

        assert!(surface_credit_open_for(&client, 300_000));
        record_surface_frame_sent(&mut client, 1, 300_000, true, Instant::now());
        assert!(surface_credit_open_for(&client, 300_000));
        record_surface_frame_sent(&mut client, 1, 300_000, true, Instant::now());
        assert!(!surface_credit_open_for(&client, 300_000));
        assert!(!surface_credit_open_for(&client, 1_000));
    }

    #[test]
    fn fresh_keyframes_reserve_more_than_delta_frames() {
        let mut client = test_client();
        client.surface_subs.entry(1).or_default().frame_bytes = 8_192.0;

        assert_eq!(estimated_surface_frame_bytes(&client, 1, false), 8_192);
        assert_eq!(
            estimated_surface_frame_bytes(&client, 1, true),
            SURFACE_KEYFRAME_ESTIMATE_MIN_BYTES,
        );
    }

    #[test]
    fn surface_goodput_decreases_faster_than_it_increases() {
        assert_eq!(surface_goodput_ewma(100.0, 200.0), 112.5);
        assert_eq!(surface_goodput_ewma(200.0, 100.0), 150.0);
    }

    #[test]
    fn surface_goodput_window_excludes_idle_time_before_first_ack() {
        let (mut client, _rx) = test_client_with_capacity(64);
        client.surface_goodput_bps = 100_000.0;
        let idle_start = Instant::now() - Duration::from_secs(10);
        client.surface_goodput_window_start = idle_start;

        record_surface_frame_sent(&mut client, 1, 1_000, false, Instant::now());
        record_surface_ack(&mut client, 1);

        assert_eq!(client.surface_goodput_bps, 100_000.0);
        assert_eq!(client.surface_goodput_window_bytes, 1_000);
        assert!(client.surface_goodput_window_start > idle_start);
    }

    #[test]
    fn surface_work_rotates_across_subscriptions() {
        let mut client = test_client();
        for surface_id in [3, 1, 2] {
            client.surface_subscriptions.insert(surface_id);
        }

        assert_eq!(surface_work_order(&mut client).as_slice(), &[1, 2, 3]);
        assert_eq!(surface_work_order(&mut client).as_slice(), &[2, 3, 1]);
        assert_eq!(surface_work_order(&mut client).as_slice(), &[3, 1, 2]);
        assert_eq!(surface_work_order(&mut client).as_slice(), &[1, 2, 3]);
    }

    // ── browser_pacing_fps baseline ──

    #[test]
    fn browser_pacing_fps_matches_display_fps_when_browser_ready() {
        let mut client = test_client();
        client.rtt_ms = 1.0;
        client.min_rtt_ms = 1.0;
        client.browser_backlog_frames = 0;
        client.browser_ack_ahead_frames = 0;
        client.browser_apply_ms = 0.0;
        client.goodput_bps = 1_000_000.0;
        client.delivery_bps = 1_000_000.0;
        client.display_fps = 144.0;
        assert!((browser_pacing_fps(&client) - 144.0).abs() < 0.01);
    }

    #[test]
    fn browser_pacing_fps_drops_below_display_fps_when_backlogged() {
        let mut client = test_client();
        client.browser_backlog_frames = 20;
        let fps = browser_pacing_fps(&client);
        assert!(fps >= 1.0);
        assert!(fps < client.display_fps);
    }

    // ── effective_rtt_ms ──

    #[test]
    fn effective_rtt_ms_equals_path_when_queue_is_empty() {
        let mut client = test_client();
        client.rtt_ms = 1.0;
        client.min_rtt_ms = 1.0;
        client.browser_backlog_frames = 0;
        client.browser_ack_ahead_frames = 0;
        client.browser_apply_ms = 0.0;
        client.goodput_bps = 1_000_000.0;
        client.delivery_bps = 1_000_000.0;
        assert!((effective_rtt_ms(&client) - 1.0).abs() < 0.01);
    }

    #[test]
    fn effective_rtt_ms_at_least_path_rtt() {
        let client = test_client();
        assert!(effective_rtt_ms(&client) >= path_rtt_ms(&client));
    }

    // ── target_frame_window ──

    #[test]
    fn target_frame_window_at_least_two() {
        let client = test_client();
        assert!(target_frame_window(&client) >= 2);
    }

    #[test]
    fn target_frame_window_grows_with_probe() {
        let mut client = test_client();
        let base = target_frame_window(&client);
        client.probe_frames = 10.0;
        let probed = target_frame_window(&client);
        assert!(probed > base, "probe_frames should grow the window");
    }

    // ── bandwidth_floor_bps ──

    #[test]
    fn bandwidth_floor_bps_at_least_16k() {
        let mut client = test_client();
        client.goodput_bps = 0.0;
        client.delivery_bps = 0.0;
        assert_eq!(bandwidth_floor_bps(&client), 0.0);
    }

    #[test]
    fn bandwidth_floor_bps_scales_with_goodput() {
        let mut client = test_client();
        client.goodput_bps = 1_000_000.0;
        client.delivery_bps = 1_000_000.0;
        let floor = bandwidth_floor_bps(&client);
        assert!(floor > 0.0);
    }

    #[test]
    fn browser_ready_delivery_floor_can_drive_large_frames_to_display_fps() {
        let mut client = test_client();
        client.display_fps = 60.0;
        client.browser_backlog_frames = 0;
        client.browser_ack_ahead_frames = 0;
        client.browser_apply_ms = 0.2;
        client.goodput_bps = 3_000_000.0;
        client.delivery_bps = 9_500_000.0;
        client.last_goodput_sample_bps = 3_000_000.0;
        client.avg_paced_frame_bytes = 150_000.0;
        client.avg_preview_frame_bytes = 1_024.0;
        client.avg_frame_bytes = 150_000.0;

        assert!(
            (pacing_fps(&client) - client.display_fps).abs() < 0.01,
            "browser-ready delivery floor should let large frames reach display_fps on a fast path",
        );
    }

    // ── pacing_fps ──

    #[test]
    fn pacing_fps_zero_when_no_bandwidth() {
        let mut client = test_client();
        client.goodput_bps = 0.0;
        client.delivery_bps = 0.0;
        client.last_goodput_sample_bps = 0.0;
        assert!(
            pacing_fps(&client) == 0.0,
            "pacing_fps should be 0 with zero bandwidth"
        );
    }

    #[test]
    fn pacing_fps_reaches_display_fps_when_not_bandwidth_limited() {
        let mut client = test_client();
        client.rtt_ms = 1.0;
        client.min_rtt_ms = 1.0;
        client.browser_backlog_frames = 0;
        client.browser_ack_ahead_frames = 0;
        client.browser_apply_ms = 0.0;
        client.goodput_bps = 1_000_000.0;
        client.delivery_bps = 1_000_000.0;
        client.display_fps = 60.0;
        assert!((pacing_fps(&client) - 60.0).abs() < 0.01);
    }

    // ── throughput_limited ──

    #[test]
    fn throughput_limited_when_low_bandwidth() {
        let mut client = test_client();
        client.goodput_bps = 1_000.0;
        client.delivery_bps = 1_000.0;
        client.last_goodput_sample_bps = 0.0;
        assert!(throughput_limited(&client));
    }

    #[test]
    fn throughput_not_limited_with_high_bandwidth() {
        let mut client = test_client();
        client.goodput_bps = 100_000_000.0;
        client.delivery_bps = 100_000_000.0;
        assert!(!throughput_limited(&client));
    }

    #[test]
    fn throughput_demand_uses_terminal_preview_cap() {
        let mut client = test_client();
        client.display_fps = 144.0;
        client.avg_paced_frame_bytes = 256.0;
        client.avg_preview_frame_bytes = 12_000.0;
        client.goodput_bps = 400_000.0;
        client.delivery_bps = 400_000.0;
        assert!(!throughput_limited(&client));
    }

    // ── browser_pacing_fps ──

    #[test]
    fn browser_pacing_fps_at_least_one() {
        let client = test_client();
        assert!(browser_pacing_fps(&client) >= 1.0);
    }

    #[test]
    fn browser_pacing_fps_reduced_by_high_backlog() {
        let mut client = test_client();
        let normal = browser_pacing_fps(&client);
        client.browser_backlog_frames = 20;
        let backlogged = browser_pacing_fps(&client);
        assert!(backlogged < normal, "high backlog should reduce pacing fps");
    }

    #[test]
    fn browser_pacing_fps_reduced_by_high_ack_ahead() {
        let mut client = test_client();
        let normal = browser_pacing_fps(&client);
        client.browser_ack_ahead_frames = 10;
        let ahead = browser_pacing_fps(&client);
        assert!(ahead < normal, "high ack_ahead should reduce pacing fps");
    }

    // ── browser_backlog_blocked ──

    #[test]
    fn browser_backlog_blocked_over_threshold() {
        let mut client = test_client();
        client.browser_backlog_frames = 9;
        assert!(browser_backlog_blocked(&client));
    }

    #[test]
    fn browser_backlog_not_blocked_under_threshold() {
        let mut client = test_client();
        client.browser_backlog_frames = 8;
        assert!(!browser_backlog_blocked(&client));
    }

    // ── byte_budget_for ──

    #[test]
    fn byte_budget_for_at_least_one_frame() {
        let client = test_client();
        let budget = byte_budget_for(&client, 10.0);
        assert!(budget >= client.avg_frame_bytes.max(256.0) as usize);
    }

    #[test]
    fn byte_budget_for_grows_with_time() {
        let client = test_client();
        let short = byte_budget_for(&client, 10.0);
        let long = byte_budget_for(&client, 1000.0);
        assert!(long >= short);
    }

    // ── target_byte_window ──

    #[test]
    fn target_byte_window_positive() {
        let client = test_client();
        assert!(target_byte_window(&client) > 0);
    }

    #[test]
    fn target_byte_window_covers_frame_window() {
        let client = test_client();
        let byte_win = target_byte_window(&client);
        let frame_win = target_frame_window(&client);
        let min_bytes =
            (client.avg_paced_frame_bytes.max(256.0) * frame_win.max(2) as f32).ceil() as usize;
        assert!(
            byte_win >= min_bytes,
            "byte window should cover at least frame_window worth of paced frames"
        );
    }

    // ── send_interval ──

    #[test]
    fn send_interval_matches_browser_pacing() {
        let client = test_client();
        let interval = send_interval(&client);
        let expected = Duration::from_secs_f64(1.0 / browser_pacing_fps(&client) as f64);
        let diff = interval.abs_diff(expected);
        assert!(diff < Duration::from_micros(10));
    }

    // ── preview_fps ──

    #[test]
    fn preview_fps_at_least_one() {
        let client = test_client();
        assert!(preview_fps(&client) >= 1.0);
    }

    #[test]
    fn preview_fps_is_capped_at_thumbnail_rate() {
        let mut client = test_client();
        client.display_fps = 144.0;
        client.goodput_bps = 100_000_000.0;
        client.delivery_bps = 100_000_000.0;
        assert_eq!(preview_fps(&client), TERMINAL_PREVIEW_MAX_FPS);
    }

    #[test]
    fn preview_fps_preserves_lower_display_rate() {
        let mut client = test_client();
        client.display_fps = 10.0;
        client.goodput_bps = 100_000_000.0;
        client.delivery_bps = 100_000_000.0;
        assert_eq!(preview_fps(&client), 10.0);
    }

    // ── window_open ──

    #[test]
    fn window_open_initially() {
        let client = test_client();
        assert!(window_open(&client));
    }

    #[test]
    fn window_open_false_when_browser_blocked() {
        let mut client = test_client();
        client.browser_backlog_frames = 20;
        assert!(!window_open(&client));
    }

    #[test]
    fn window_open_false_when_inflight_full() {
        let mut client = test_client();
        let target = target_frame_window(&client);
        fill_inflight(&mut client, target + 10, 1024);
        assert!(!window_open(&client));
    }

    // ── lead_window_open ──

    #[test]
    fn lead_window_open_no_reserve_same_as_window_open() {
        let client = test_client();
        assert_eq!(lead_window_open(&client, false), window_open(&client));
    }

    #[test]
    fn lead_window_open_reserves_preview_slot() {
        let mut client = test_client();
        client.lead = Some(1);
        client.subscriptions.insert(1);
        let target = target_frame_window(&client);
        // Fill to just under target minus reserve
        fill_inflight(&mut client, target.saturating_sub(1), 512);
        // Without reserve: may still be open
        // With reserve: should be closed
        assert!(!lead_window_open(&client, true));
    }

    // ── can_send_frame ──

    #[test]
    fn can_send_frame_when_window_open_and_time_due() {
        let mut client = test_client();
        client.next_send_at = Instant::now() - Duration::from_millis(100);
        assert!(can_send_frame(&client, Instant::now(), false));
    }

    #[test]
    fn can_send_frame_false_when_not_due() {
        let mut client = test_client();
        client.next_send_at = Instant::now() + Duration::from_secs(10);
        assert!(!can_send_frame(&client, Instant::now(), false));
    }

    #[test]
    fn can_send_frame_false_when_window_closed() {
        let mut client = test_client();
        client.browser_backlog_frames = 20; // triggers browser_backlog_blocked
        client.next_send_at = Instant::now() - Duration::from_millis(100);
        assert!(!can_send_frame(&client, Instant::now(), false));
    }

    // ── record_send / record_ack state transitions ──

    #[test]
    fn record_send_increases_inflight() {
        let mut client = test_client();
        let now = Instant::now();
        assert_eq!(client.inflight_bytes, 0);
        assert_eq!(client.inflight_frames.len(), 0);

        record_send(&mut client, 1000, now, true);
        assert_eq!(client.inflight_bytes, 1000);
        assert_eq!(client.inflight_frames.len(), 1);

        record_send(&mut client, 500, now, false);
        assert_eq!(client.inflight_bytes, 1500);
        assert_eq!(client.inflight_frames.len(), 2);
    }

    #[test]
    fn record_send_paced_advances_deadline() {
        let mut client = test_client();
        let now = Instant::now();
        client.next_send_at = now;
        record_send(&mut client, 1000, now, true);
        assert!(client.next_send_at > now);
    }

    #[test]
    fn record_send_unpaced_does_not_advance_deadline() {
        let mut client = test_client();
        let now = Instant::now();
        let before = client.next_send_at;
        record_send(&mut client, 1000, now, false);
        assert_eq!(client.next_send_at, before);
    }

    #[test]
    fn record_ack_decreases_inflight() {
        let mut client = test_client();
        let now = Instant::now();
        record_send(&mut client, 1000, now, true);
        record_send(&mut client, 500, now, true);
        assert_eq!(client.inflight_frames.len(), 2);

        record_ack(&mut client);
        assert_eq!(client.inflight_frames.len(), 1);
        assert_eq!(client.inflight_bytes, 500);
    }

    #[test]
    fn record_ack_on_empty_clears_bytes() {
        let mut client = test_client();
        client.inflight_bytes = 999; // stale state
        record_ack(&mut client);
        assert_eq!(client.inflight_bytes, 0);
    }

    #[test]
    fn record_ack_updates_rtt_estimate() {
        let mut client = test_client();
        let now = Instant::now();
        client.inflight_frames.push_back(InFlightFrame {
            sent_at: now - Duration::from_millis(20),
            bytes: 512,
            paced: true,
        });
        client.inflight_bytes = 512;
        let old_rtt = client.rtt_ms;
        record_ack(&mut client);
        // RTT should have been updated (moved toward ~20ms from the default 50ms)
        assert!(
            (client.rtt_ms - old_rtt).abs() > 0.01,
            "rtt_ms should be updated after ack"
        );
    }

    #[test]
    fn record_ack_paced_updates_avg_paced_frame_bytes() {
        let mut client = test_client();
        let now = Instant::now();
        client.inflight_frames.push_back(InFlightFrame {
            sent_at: now - Duration::from_millis(10),
            bytes: 4096,
            paced: true,
        });
        client.inflight_bytes = 4096;
        let old_avg = client.avg_paced_frame_bytes;
        record_ack(&mut client);
        // Should move toward 4096 from 1024
        assert!(client.avg_paced_frame_bytes > old_avg);
    }

    #[test]
    fn record_ack_unpaced_updates_avg_preview_frame_bytes() {
        let mut client = test_client();
        let now = Instant::now();
        client.inflight_frames.push_back(InFlightFrame {
            sent_at: now - Duration::from_millis(10),
            bytes: 8192,
            paced: false,
        });
        client.inflight_bytes = 8192;
        let old_avg = client.avg_preview_frame_bytes;
        record_ack(&mut client);
        assert!(client.avg_preview_frame_bytes > old_avg);
    }

    #[test]
    fn can_send_preview_true_when_due() {
        let mut client = test_client();
        let now = Instant::now();
        client
            .preview_next_send_at
            .insert(5, now - Duration::from_millis(100));
        assert!(can_send_preview(&client, 5, now));
    }

    #[test]
    fn can_send_preview_false_when_not_due() {
        let mut client = test_client();
        let now = Instant::now();
        client
            .preview_next_send_at
            .insert(5, now + Duration::from_secs(10));
        assert!(!can_send_preview(&client, 5, now));
    }

    #[test]
    fn can_send_preview_false_when_window_closed() {
        let mut client = test_client();
        client.browser_backlog_frames = 20;
        let now = Instant::now();
        assert!(!can_send_preview(&client, 5, now));
    }

    #[test]
    fn can_send_preview_true_for_unseen_pid() {
        let client = test_client();
        let now = Instant::now();
        // No entry in preview_next_send_at means deadline defaults to now
        assert!(can_send_preview(&client, 99, now));
    }

    #[test]
    fn record_preview_send_sets_future_deadline() {
        let mut client = test_client();
        let now = Instant::now();
        record_preview_send(&mut client, 5, now);
        let deadline = client.preview_next_send_at.get(&5).unwrap();
        assert!(*deadline > now);
    }

    #[test]
    fn record_preview_send_successive_calls_advance() {
        let mut client = test_client();
        let now = Instant::now();
        record_preview_send(&mut client, 5, now);
        let first = *client.preview_next_send_at.get(&5).unwrap();
        record_preview_send(&mut client, 5, first);
        let second = *client.preview_next_send_at.get(&5).unwrap();
        assert!(second > first, "successive sends should advance deadline");
    }

    // ── congestion control end-to-end properties ──
    //
    // These tests encode the two goals of the congestion controller:
    //   1. Browser-ready, well-provisioned path → full display FPS, minimal added latency
    //   2. Bottleneck                           → lowest sustainable FPS, fast recovery when pipe clears
    //
    // Some tests assert desired future behaviour and currently FAIL due to
    // known issues (min_rtt contamination, lead_floor dominating byte window).
    // They are marked with a comment so they are easy to find when fixing.

    /// Return a client in ideal low-latency, high-bandwidth conditions:
    /// browser ready, abundant bandwidth, and tiny RTT. The normal pacing path
    /// should still reach display_fps.
    fn browser_ready_high_bandwidth_client() -> ClientState {
        let mut c = test_client();
        c.display_fps = 120.0;
        c.rtt_ms = 1.0;
        c.min_rtt_ms = 1.0;
        c.goodput_bps = 50_000_000.0;
        c.delivery_bps = 50_000_000.0;
        c.last_goodput_sample_bps = 50_000_000.0;
        c.avg_paced_frame_bytes = 30_000.0;
        c.avg_preview_frame_bytes = 1_024.0;
        c.avg_frame_bytes = 30_000.0;
        c.browser_apply_ms = 0.3;
        c
    }

    /// Return a client that has converged to a clearly congested state:
    /// ~10× min_rtt inflation, low goodput.
    fn congested_client() -> ClientState {
        let mut c = test_client();
        c.display_fps = 120.0;
        c.rtt_ms = 500.0;
        c.min_rtt_ms = 40.0;
        c.goodput_bps = 200_000.0;
        c.delivery_bps = 150_000.0;
        c.last_goodput_sample_bps = 200_000.0;
        c.avg_paced_frame_bytes = 50_000.0;
        c.avg_preview_frame_bytes = 1_024.0;
        c.avg_frame_bytes = 50_000.0;
        c.goodput_jitter_bps = 50_000.0;
        c.max_goodput_jitter_bps = 200_000.0;
        c.browser_apply_ms = 1.0;
        c
    }

    /// Simulate one ACK: insert a frame with the given RTT into inflight and
    /// call record_ack.  Forces a goodput-window sample each call so that
    /// goodput estimates respond within a few calls.
    fn sim_ack(client: &mut ClientState, bytes: usize, rtt_ms: f32) {
        let sent_at = Instant::now() - Duration::from_millis(rtt_ms as u64);
        client.inflight_bytes += bytes;
        client.inflight_frames.push_back(InFlightFrame {
            sent_at,
            bytes,
            paced: true,
        });
        // Age the goodput window so record_ack always emits a sample.
        client.goodput_window_start = Instant::now() - Duration::from_millis(25);
        record_ack(client);
    }

    fn sim_acks(client: &mut ClientState, n: usize, bytes: usize, rtt_ms: f32) {
        for _ in 0..n {
            sim_ack(client, bytes, rtt_ms);
        }
    }

    // ── property: full FPS on a browser-ready path ──

    #[test]
    fn browser_ready_high_bandwidth_client_uses_full_display_fps() {
        let client = browser_ready_high_bandwidth_client();
        assert!(
            (pacing_fps(&client) - client.display_fps).abs() < 0.01,
            "pacing_fps {} should equal display_fps {} when browser is ready and bandwidth is abundant",
            pacing_fps(&client),
            client.display_fps,
        );
    }

    #[test]
    fn browser_ready_high_bandwidth_client_send_interval_within_one_frame() {
        let client = browser_ready_high_bandwidth_client();
        let interval_ms = send_interval(&client).as_secs_f32() * 1000.0;
        let frame_ms = 1000.0 / client.display_fps;
        assert!(
            interval_ms <= frame_ms + 0.1,
            "send_interval {interval_ms:.2}ms exceeds one frame ({frame_ms:.2}ms) when browser is ready"
        );
    }

    // ── property: degraded FPS when bottlenecked ──

    #[test]
    fn congested_pipe_reduces_pacing_fps_substantially() {
        let client = congested_client();
        let fps = pacing_fps(&client);
        assert!(
            fps < client.display_fps * 0.5,
            "pacing_fps {fps:.0} should be well below display_fps {} when congested",
            client.display_fps,
        );
    }

    #[test]
    fn congested_pipe_is_throughput_limited() {
        let client = congested_client();
        assert!(
            throughput_limited(&client),
            "congested client must be recognised as throughput-limited"
        );
    }

    // ── property: byte window should stay near BDP ──
    //
    // KNOWN FAILING: lead_floor in target_byte_window overrides the BDP
    // budget when avg_paced_frame_bytes is large.  Fix: cap lead_floor.

    #[test]
    fn byte_window_bounded_near_bdp_when_congested() {
        let client = congested_client();
        // BDP at the unloaded path RTT.
        let bdp = client.goodput_bps * (path_rtt_ms(&client) / 1_000.0);
        let window = target_byte_window(&client);
        assert!(
            window < bdp as usize * 8,
            "byte window {window}B is {:.1}× BDP ({bdp:.0}B); \
             expected ≤ 8× — lead_floor may be dominating",
            window as f32 / bdp.max(1.0),
        );
    }

    // ── property: min_rtt must not drift upward under congestion ──
    //
    // KNOWN FAILING: the `min_rtt_ms * 0.999 + rtt_ms * 0.001` update
    // bleeds queued RTT into min_rtt.

    #[test]
    fn min_rtt_not_contaminated_by_congested_rtts() {
        let mut client = test_client();
        client.display_fps = 120.0;
        client.rtt_ms = 40.0;
        client.min_rtt_ms = 40.0;
        client.goodput_bps = 2_000_000.0;
        client.delivery_bps = 2_000_000.0;
        client.avg_paced_frame_bytes = 30_000.0;
        client.avg_preview_frame_bytes = 1_024.0;
        let original_min = client.min_rtt_ms;

        // 200 ACKs arriving with 500ms RTT (severe congestion).
        sim_acks(&mut client, 200, 30_000, 500.0);

        assert!(
            client.min_rtt_ms < original_min * 2.0,
            "min_rtt drifted from {original_min}ms to {:.1}ms after 200 congested ACKs",
            client.min_rtt_ms,
        );
    }

    // ── property: fast recovery when congestion clears ──

    #[test]
    fn delivery_bps_rises_quickly_when_congestion_clears() {
        let mut client = congested_client();
        let before = client.delivery_bps;

        // 10 ACKs at low latency / high throughput.
        sim_acks(&mut client, 10, 30_000, 40.0);

        assert!(
            client.delivery_bps > before * 2.0,
            "delivery_bps {:.0} should more than double from {before:.0} after 10 fast ACKs",
            client.delivery_bps,
        );
    }

    #[test]
    fn pacing_fps_recovers_after_congestion_clears() {
        let mut client = congested_client();

        // Use window-saturated rounds: fill the window with frames, age the
        // goodput window once, then ACK all.  The first ACK each round emits
        // a sample; the remaining target-1 ACKs carry over into the next
        // window, so sample throughput grows as target grows — mimicking a
        // real link where the sender keeps the pipe full across one RTT.
        for _ in 0..40 {
            let target = target_frame_window(&client).max(2);
            for _ in 0..target {
                let sent_at = Instant::now() - Duration::from_millis(40);
                client.inflight_bytes += 30_000;
                client.inflight_frames.push_back(InFlightFrame {
                    sent_at,
                    bytes: 30_000,
                    paced: true,
                });
            }
            client.goodput_window_start = Instant::now() - Duration::from_millis(25);
            for _ in 0..target {
                record_ack(&mut client);
            }
        }

        let fps = pacing_fps(&client);
        assert!(
            fps > client.display_fps * 0.7,
            "pacing_fps {fps:.0} didn't recover toward display_fps {} \
             after window-saturated rounds at low RTT",
            client.display_fps,
        );
    }

    #[test]
    fn rtt_estimate_drops_quickly_when_congestion_clears() {
        let mut client = test_client();
        client.rtt_ms = 500.0;
        client.min_rtt_ms = 40.0;
        client.goodput_bps = 2_000_000.0;
        client.avg_paced_frame_bytes = 30_000.0;
        client.avg_preview_frame_bytes = 1_024.0;

        // The asymmetric EWMA uses rise=0.125, fall=0.25, so rtt_ms drops
        // at fall_alpha=0.25 per sample toward the new low.
        sim_acks(&mut client, 10, 30_000, 40.0);

        assert!(
            client.rtt_ms < 300.0,
            "rtt_ms {:.0}ms did not fall fast enough after congestion cleared",
            client.rtt_ms,
        );
    }

    // ── property: probing ──

    #[test]
    fn probe_collapses_immediately_on_queue_delay() {
        let mut client = test_client();
        client.display_fps = 120.0;
        client.rtt_ms = 40.0;
        client.min_rtt_ms = 40.0;
        client.goodput_bps = 5_000_000.0;
        client.delivery_bps = 5_000_000.0;
        client.last_goodput_sample_bps = 5_000_000.0;
        client.avg_paced_frame_bytes = 10_000.0;
        client.avg_preview_frame_bytes = 1_024.0;
        client.probe_frames = 10.0;

        // ACKs arriving with high RTT signal queue buildup.
        sim_acks(&mut client, 5, 10_000, 600.0);

        assert!(
            client.probe_frames < 5.0,
            "probe_frames {:.1} should have collapsed on queue delay signal",
            client.probe_frames,
        );
    }

    #[test]
    fn probe_grows_when_window_saturated_with_clean_rtt() {
        let mut client = test_client();
        client.display_fps = 120.0;
        client.rtt_ms = 40.0;
        client.min_rtt_ms = 40.0;
        client.goodput_bps = 5_000_000.0;
        client.delivery_bps = 5_000_000.0;
        client.last_goodput_sample_bps = 5_000_000.0;
        client.avg_paced_frame_bytes = 10_000.0;
        client.avg_preview_frame_bytes = 1_024.0;
        client.goodput_jitter_bps = 0.0;
        client.max_goodput_jitter_bps = 0.0;
        client.probe_frames = 0.0;

        // Saturate inflight so window_saturated returns true during acks.
        let target = target_frame_window(&client);
        for _ in 0..target {
            let sent_at = Instant::now() - Duration::from_millis(40);
            client.inflight_bytes += 10_000;
            client.inflight_frames.push_back(InFlightFrame {
                sent_at,
                bytes: 10_000,
                paced: true,
            });
        }

        // Ack one frame with clean RTT.  One saturated ACK is sufficient to
        // verify the property: as probe_frames increments, target_frame_window
        // grows, so the remaining (target-1) frames would fall below the 90%
        // threshold and trigger gentle decay.  The property under test is that
        // *receiving an ACK while window-saturated* increments probe_frames —
        // not that it stays incremented across subsequent unsaturated ACKs.
        // Also: do NOT age the goodput window — that would emit a per-frame
        // sample far below goodput_bps, spiking jitter and collapsing probe.
        record_ack(&mut client);

        assert!(
            client.probe_frames > 0.0,
            "probe_frames should grow when window-saturated with clean RTT"
        );
    }

    // ── property: frame window larger on high-latency links ──

    #[test]
    fn frame_window_larger_on_high_latency_link() {
        let mut lo = test_client();
        lo.display_fps = 120.0;
        lo.rtt_ms = 10.0;
        lo.min_rtt_ms = 10.0;
        lo.goodput_bps = 5_000_000.0;
        lo.delivery_bps = 5_000_000.0;
        lo.avg_paced_frame_bytes = 10_000.0;
        lo.avg_preview_frame_bytes = 1_024.0;

        let mut hi = test_client();
        hi.display_fps = 120.0;
        hi.rtt_ms = 200.0;
        hi.min_rtt_ms = 200.0;
        hi.goodput_bps = 5_000_000.0;
        hi.delivery_bps = 5_000_000.0;
        hi.avg_paced_frame_bytes = 10_000.0;
        hi.avg_preview_frame_bytes = 1_024.0;

        let lo_win = target_frame_window(&lo);
        let hi_win = target_frame_window(&hi);
        assert!(
            hi_win > lo_win,
            "high-latency link ({hi_win}f) should need more frames in flight \
             than low-latency ({lo_win}f)"
        );
    }

    // ── property: small-frame byte window allows pipelining ──

    #[test]
    fn small_frame_byte_window_enables_pipelining() {
        // Tiny terminal frames (~1KB) with a stale congested RTT and low
        // goodput estimate (stop-and-wait artifact): byte window must be at
        // least target_frame_window × frame_bytes so the sender can pipeline
        // rather than stay stuck in stop-and-wait.
        let mut client = test_client();
        client.display_fps = 120.0;
        client.rtt_ms = 165.0;
        client.min_rtt_ms = 8.0;
        client.goodput_bps = 11_000.0; // stop-and-wait artifact
        client.delivery_bps = 6_800.0;
        client.last_goodput_sample_bps = 11_000.0;
        client.avg_paced_frame_bytes = 1_120.0;
        client.avg_preview_frame_bytes = 1_024.0;
        client.goodput_jitter_bps = 4_300.0;
        client.max_goodput_jitter_bps = 6_500.0;

        let window = target_byte_window(&client);
        let frames = target_frame_window(&client);
        let pipeline = frames * 1_120;

        assert!(
            window >= pipeline,
            "byte window {window}B should be >= pipeline ({frames}f × 1120B = {pipeline}B) \
             so small frames can pipeline across the RTT"
        );
    }

    #[test]
    fn large_frame_byte_window_bounded_by_one_frame_floor() {
        // With large frames (50KB), pipelining the full frame window (5×50KB=250KB)
        // would be many multiples of BDP.  Byte window should fall back to
        // the one-frame floor so the BDP budget governs.
        let mut client = test_client();
        client.display_fps = 120.0;
        client.rtt_ms = 165.0;
        client.min_rtt_ms = 8.0;
        client.goodput_bps = 11_000.0;
        client.delivery_bps = 6_800.0;
        client.last_goodput_sample_bps = 11_000.0;
        client.avg_paced_frame_bytes = 50_000.0; // large frame
        client.avg_preview_frame_bytes = 1_024.0;
        client.goodput_jitter_bps = 0.0;
        client.max_goodput_jitter_bps = 0.0;

        let window = target_byte_window(&client);
        let frames = target_frame_window(&client);
        let pipeline = frames.saturating_mul(50_000);

        assert!(
            window < pipeline,
            "byte window {window}B should be < full pipeline {pipeline}B \
             ({frames}f × 50KB) — large frames must use one-frame floor"
        );
        assert!(
            window >= 50_000,
            "byte window {window}B must be at least one frame (50KB)"
        );
    }

    // ── property: preview reservation applies uniformly ──

    #[test]
    fn preview_reservation_applies_even_on_low_latency_high_bandwidth_links() {
        let mut client = browser_ready_high_bandwidth_client();
        client.lead = Some(1);
        client.subscriptions.insert(1);
        let target = target_frame_window(&client);
        fill_inflight(&mut client, target.saturating_sub(1), 512);
        assert!(
            !lead_window_open(&client, true),
            "preview reservation should apply uniformly for lead clients"
        );
    }

    // ── property: blip recovery on healthy paths ──

    #[test]
    fn probe_recovers_on_healthy_path_after_blip() {
        let mut client = browser_ready_high_bandwidth_client();
        client.probe_frames = 8.0;

        // Blip: 3 ACKs with inflated RTT crush probes.
        sim_acks(&mut client, 3, 30_000, 200.0);
        let post_blip = client.probe_frames;
        assert!(
            post_blip < 4.0,
            "probe_frames {post_blip:.1} should have dropped after blip"
        );

        // Reset browser metrics to healthy (browser cleared backlog).
        client.browser_backlog_frames = 0;
        client.browser_ack_ahead_frames = 0;
        client.browser_apply_ms = 0.3;

        // Recovery: 20 healthy ACKs at low RTT on an underfilled path.
        sim_acks(&mut client, 20, 30_000, 1.0);

        assert!(
            client.probe_frames > post_blip,
            "probe_frames {:.1} should have recovered from {post_blip:.1} after healthy ACKs",
            client.probe_frames,
        );
    }

    #[test]
    fn jitter_decays_fast_on_browser_ready_path() {
        let mut client = browser_ready_high_bandwidth_client();

        // Inject elevated jitter (simulating post-blip state).
        client.max_goodput_jitter_bps = client.goodput_bps * 0.4;
        client.goodput_jitter_bps = client.goodput_bps * 0.3;
        let initial_jitter = client.max_goodput_jitter_bps;

        // 10 healthy ACKs on a browser-ready path.
        sim_acks(&mut client, 10, 30_000, 1.0);

        assert!(
            client.max_goodput_jitter_bps < initial_jitter * 0.5,
            "max_goodput_jitter_bps {:.0} should have decayed below {:.0} \
             (50% of initial {initial_jitter:.0}) after 10 healthy ACKs on a ready path",
            client.max_goodput_jitter_bps,
            initial_jitter * 0.5,
        );
    }

    #[test]
    fn byte_budget_uses_floor_when_goodput_depressed() {
        let mut client = browser_ready_high_bandwidth_client();
        client.goodput_bps = 100_000.0;

        let budget = byte_budget_for(&client, 100.0);
        let floor_budget = (bandwidth_floor_bps(&client) * 100.0 / 1_000.0).ceil() as usize;

        assert!(
            budget >= floor_budget,
            "byte_budget {budget} should be at least bandwidth_floor-based {floor_budget} \
             when goodput_bps is depressed but delivery_bps is high"
        );
    }

    #[test]
    fn probe_floor_maintained_under_congestion_signal() {
        let mut client = test_client();
        client.display_fps = 120.0;
        client.rtt_ms = 40.0;
        client.min_rtt_ms = 40.0;
        client.goodput_bps = 5_000_000.0;
        client.delivery_bps = 5_000_000.0;
        client.last_goodput_sample_bps = 5_000_000.0;
        client.avg_paced_frame_bytes = 10_000.0;
        client.avg_preview_frame_bytes = 1_024.0;
        client.probe_frames = 10.0;

        // Many ACKs with high RTT: probes should not drop below the floor.
        sim_acks(&mut client, 20, 10_000, 600.0);

        assert!(
            client.probe_frames >= 1.0,
            "probe_frames {:.1} should not drop below the floor of 1.0",
            client.probe_frames,
        );
    }

    // ── parse_terminal_queries ──

    #[test]
    fn parse_tq_da1_bare() {
        let results = parse_terminal_queries(b"\x1b[c", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert!(results[0].starts_with("\x1b[?64;"));
    }

    #[test]
    fn parse_tq_da1_with_zero_param() {
        let results = parse_terminal_queries(b"\x1b[0c", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert!(results[0].starts_with("\x1b[?64;"));
    }

    #[test]
    fn parse_tq_dsr_cursor_position() {
        let results = parse_terminal_queries(b"\x1b[6n", (24, 80), (5, 10)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b[6;11R");
    }

    #[test]
    fn parse_tq_dsr_status() {
        let results = parse_terminal_queries(b"\x1b[5n", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b[0n");
    }

    #[test]
    fn parse_tq_window_size_cells() {
        let results = parse_terminal_queries(b"\x1b[18t", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b[8;24;80t");
    }

    #[test]
    fn parse_tq_window_size_pixels() {
        let results = parse_terminal_queries(b"\x1b[14t", (30, 100), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b[4;480;800t");
    }

    #[test]
    fn parse_tq_multiple_queries() {
        let data = b"\x1b[c\x1b[6n\x1b[5n";
        let results = parse_terminal_queries(data, (24, 80), (2, 3)).responses;
        assert_eq!(results.len(), 3);
        assert!(results[0].starts_with("\x1b[?64;"));
        assert_eq!(results[1], "\x1b[3;4R");
        assert_eq!(results[2], "\x1b[0n");
    }

    #[test]
    fn parse_tq_question_mark_sequences_skipped() {
        let results = parse_terminal_queries(b"\x1b[?1h", (24, 80), (0, 0)).responses;
        assert!(results.is_empty());
    }

    #[test]
    fn parse_tq_unknown_final_byte_ignored() {
        let results = parse_terminal_queries(b"\x1b[42z", (24, 80), (0, 0)).responses;
        assert!(results.is_empty());
    }

    #[test]
    fn parse_tq_empty_input() {
        let results = parse_terminal_queries(b"", (24, 80), (0, 0)).responses;
        assert!(results.is_empty());
    }

    #[test]
    fn parse_tq_plain_text_no_csi() {
        let results = parse_terminal_queries(b"hello world", (24, 80), (0, 0)).responses;
        assert!(results.is_empty());
    }

    #[test]
    fn parse_tq_interleaved_with_text() {
        let results = parse_terminal_queries(b"abc\x1b[cdef\x1b[6n", (24, 80), (1, 2)).responses;
        assert_eq!(results.len(), 2);
    }

    // ── parse_terminal_queries: OSC ──

    #[test]
    fn parse_tq_osc11_background_color_bel() {
        let results = parse_terminal_queries(b"\x1b]11;?\x07", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b]11;rgb:0000/0000/0000\x1b\\");
    }

    #[test]
    fn parse_tq_osc11_background_color_st() {
        let results = parse_terminal_queries(b"\x1b]11;?\x1b\\", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b]11;rgb:0000/0000/0000\x1b\\");
    }

    #[test]
    fn parse_tq_osc10_foreground_color() {
        let results = parse_terminal_queries(b"\x1b]10;?\x07", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b]10;rgb:ffff/ffff/ffff\x1b\\");
    }

    #[test]
    fn parse_tq_osc4_palette_color_0() {
        let results = parse_terminal_queries(b"\x1b]4;0;?\x07", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b]4;0;rgb:0000/0000/0000\x1b\\");
    }

    #[test]
    fn parse_tq_osc4_palette_color_1() {
        let results = parse_terminal_queries(b"\x1b]4;1;?\x07", (24, 80), (0, 0)).responses;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "\x1b]4;1;rgb:8080/0000/0000\x1b\\");
    }

    #[test]
    fn parse_tq_osc_mixed_with_csi() {
        let results =
            parse_terminal_queries(b"\x1b]11;?\x07\x1b[c\x1b]4;0;?\x07", (24, 80), (0, 0))
                .responses;
        assert_eq!(results.len(), 3);
        assert!(results[0].starts_with("\x1b]11;"));
        assert!(results[1].starts_with("\x1b[?64;"));
        assert!(results[2].starts_with("\x1b]4;0;"));
    }

    // ── OSC 7 working-directory reports ──

    #[test]
    fn osc7_plain_bel_terminated() {
        let scan =
            parse_terminal_queries(b"\x1b]7;file:///home/user/project\x07", (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd.as_deref(), Some("/home/user/project"));
        // A cwd report is not a query — nothing goes back into the PTY.
        assert!(scan.responses.is_empty());
    }

    #[test]
    fn osc7_st_terminated_localhost() {
        let scan = parse_terminal_queries(b"\x1b]7;file://localhost/tmp\x1b\\", (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn osc7_percent_decoded() {
        let scan =
            parse_terminal_queries(b"\x1b]7;file:///a%20dir/caf%C3%A9\x07", (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd.as_deref(), Some("/a dir/café"));
    }

    #[test]
    fn osc7_own_hostname_accepted() {
        let host = local_hostname();
        if host.is_empty() {
            return;
        }
        let payload = format!("\x1b]7;file://{host}/srv\x07");
        let scan = parse_terminal_queries(payload.as_bytes(), (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd.as_deref(), Some("/srv"));
    }

    #[test]
    fn osc7_foreign_host_ignored() {
        // A remote-ssh shell reports the remote host; its path is not local.
        let scan =
            parse_terminal_queries(b"\x1b]7;file://elsewhere.example/tmp\x07", (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd, None);
    }

    #[test]
    fn osc7_non_absolute_rejected() {
        // No path after the host at all.
        let scan = parse_terminal_queries(b"\x1b]7;file://localhost\x07", (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd, None);
        // Percent-encoded slash is not a literal path separator.
        let scan = parse_terminal_queries(b"\x1b]7;file://%2Ftmp\x07", (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd, None);
        // Not a file:// URL.
        let scan = parse_terminal_queries(b"\x1b]7;http://localhost/x\x07", (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd, None);
    }

    #[test]
    fn osc7_malformed_escapes_rejected() {
        for payload in [
            &b"\x1b]7;file:///a%GGb\x07"[..],    // non-hex escape
            &b"\x1b]7;file:///a%2\x07"[..],      // truncated escape
            &b"\x1b]7;file:///a%00b\x07"[..],    // embedded NUL
            &b"\x1b]7;file:///a%FF\x07"[..],     // invalid UTF-8 after decode
            &b"\x1b]7;file:///unterminated"[..], // no BEL/ST terminator
        ] {
            let scan = parse_terminal_queries(payload, (24, 80), (0, 0));
            assert_eq!(scan.osc7_cwd, None, "payload {payload:?}");
        }
    }

    #[test]
    fn osc7_oversize_dropped() {
        let max = "/".to_owned() + &"a".repeat(TERM_CWD_MAX - 1);
        let ok = format!("\x1b]7;file://{max}\x07");
        let scan = parse_terminal_queries(ok.as_bytes(), (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd.as_deref(), Some(max.as_str()));

        let over = format!("\x1b]7;file://{max}a\x07");
        let scan = parse_terminal_queries(over.as_bytes(), (24, 80), (0, 0));
        assert_eq!(scan.osc7_cwd, None);
    }

    #[test]
    fn osc7_last_report_in_chunk_wins() {
        let scan = parse_terminal_queries(
            b"\x1b]7;file:///first\x07output\x1b]7;file:///second\x07",
            (24, 80),
            (0, 0),
        );
        assert_eq!(scan.osc7_cwd.as_deref(), Some("/second"));
    }

    #[test]
    fn osc7_dedupe_same_cwd_one_push() {
        let mut stored = None;
        assert!(note_osc7_cwd(&mut stored, Some("/tmp".into())));
        // Shells re-emit per prompt: an identical repeat pushes nothing.
        assert!(!note_osc7_cwd(&mut stored, Some("/tmp".into())));
        // A change pushes again (last write wins).
        assert!(note_osc7_cwd(&mut stored, Some("/var".into())));
        // Chunks without a report leave the store untouched.
        assert!(!note_osc7_cwd(&mut stored, None));
        assert_eq!(stored.as_deref(), Some("/var"));
    }

    #[test]
    fn poll_prefers_osc7_over_kernel() {
        let mut kernel_called = false;
        let cwd = resolve_term_cwd(Some("/from-osc7"), || {
            kernel_called = true;
            Some("/from-kernel".into())
        });
        assert_eq!(cwd.as_deref(), Some("/from-osc7"));
        assert!(!kernel_called, "OSC 7 hit must not touch the kernel");

        let cwd = resolve_term_cwd(None, || Some("/from-kernel".into()));
        assert_eq!(cwd.as_deref(), Some("/from-kernel"));
        assert_eq!(resolve_term_cwd(None, || None), None);
    }

    // ── client-supplied view sizes ──

    /// An ordinary viewport passes through untouched — the clamp must not
    /// quietly reshape real terminals.
    #[test]
    fn clamp_view_size_leaves_real_viewports_alone() {
        for (rows, cols) in [(24, 80), (60, 200), (1, 1), (540, 900)] {
            assert_eq!(clamp_view_size(rows, cols), (rows, cols), "{rows}x{cols}");
        }
    }

    /// Terminal Resize carries two raw u16s and only rejected zero, so one
    /// client could name a grid of 4.29 billion cells and — being the
    /// minimum across clients when it is the only one — have the terminal
    /// allocated at that size.
    #[test]
    fn clamp_view_size_bounds_a_hostile_resize() {
        let (rows, cols) = clamp_view_size(u16::MAX, u16::MAX);
        assert!(
            rows <= MAX_VIEW_DIM && cols <= MAX_VIEW_DIM,
            "{rows}x{cols}"
        );
        assert!(
            rows as usize * cols as usize <= MAX_CELL_COUNT,
            "{rows}x{cols} is past what a frame can describe"
        );
    }

    /// The cell budget binds before the per-axis cap: 4096x4096 is under both
    /// dimension limits but 16.7M cells, which no receiver would accept.
    #[test]
    fn clamp_view_size_respects_the_frame_cell_budget() {
        let (rows, cols) = clamp_view_size(MAX_VIEW_DIM, MAX_VIEW_DIM);
        assert_eq!(rows, MAX_VIEW_DIM);
        assert!(
            rows as usize * cols as usize <= MAX_CELL_COUNT,
            "{rows}x{cols}"
        );
        assert!(cols >= 1, "never clamps a dimension to zero");
    }

    /// A tall, narrow ask keeps its width rather than being squared off.
    #[test]
    fn clamp_view_size_never_yields_a_zero_dimension() {
        for rows in [1u16, 2, 1000, MAX_VIEW_DIM] {
            let (r, c) = clamp_view_size(rows, u16::MAX);
            assert!(r >= 1 && c >= 1, "{rows} -> {r}x{c}");
            assert!(r as usize * c as usize <= MAX_CELL_COUNT);
        }
    }

    // ── allocate_pty_id ──

    #[test]
    fn opaque_handles_are_bidirectional_and_never_reused() {
        let mut handles = OpaqueHandleRegistry::default();
        let first = handles.get_or_insert(7u16).unwrap();
        assert_eq!(handles.handle(7), Some(first));
        assert_eq!(handles.backend(first), Some(7));
        assert_eq!(handles.remove_backend(7), Some(first));
        assert_eq!(handles.backend(first), None);

        let replacement = handles.get_or_insert(7).unwrap();
        assert!(replacement > first);
        assert_eq!(handles.backend(replacement), Some(7));
    }

    #[test]
    fn opaque_handle_exhaustion_does_not_wrap() {
        let mut handles = OpaqueHandleRegistry {
            next_handle: Some(u64::MAX),
            ..OpaqueHandleRegistry::default()
        };
        assert_eq!(handles.get_or_insert(1u16), Some(u64::MAX));
        assert_eq!(handles.get_or_insert(2), None);
        assert_eq!(handles.backend(u64::MAX), Some(1));
    }

    #[test]
    fn allocate_pty_id_empty_session() {
        let mut sess = Session::new();
        assert_eq!(sess.allocate_pty_id(0), Some(1));
    }

    #[test]
    fn allocate_pty_id_rotates() {
        let mut sess = Session::new();
        // Sequential allocations return increasing IDs (not always 1).
        assert_eq!(sess.allocate_pty_id(0), Some(1));
        assert_eq!(sess.allocate_pty_id(0), Some(2));
        assert_eq!(sess.allocate_pty_id(0), Some(3));
    }

    #[test]
    fn allocate_pty_id_wraps_at_max() {
        let mut sess = Session::new();
        sess.next_pty_id = u16::MAX;
        assert_eq!(sess.allocate_pty_id(0), Some(u16::MAX));
        // Next allocation wraps to 1.
        assert_eq!(sess.allocate_pty_id(0), Some(1));
    }

    // ── retention ──

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn eviction_keeps_the_newest_when_over_the_count_bound() {
        let base = Instant::now();
        let exited = vec![(1, at(base, 0)), (2, at(base, 10)), (3, at(base, 20))];
        // Room for two: the oldest goes.
        assert_eq!(
            slots_to_evict(exited.clone(), at(base, 30), 2, Duration::ZERO),
            vec![1]
        );
        assert_eq!(
            slots_to_evict(exited.clone(), at(base, 30), 1, Duration::ZERO),
            vec![1, 2]
        );
        // Under the bound, nothing goes.
        assert!(slots_to_evict(exited, at(base, 30), 8, Duration::ZERO).is_empty());
    }

    #[test]
    fn eviction_count_bound_is_off_at_zero() {
        let base = Instant::now();
        let exited = vec![(1, at(base, 0)), (2, at(base, 1))];
        assert!(slots_to_evict(exited, at(base, 999), 0, Duration::ZERO).is_empty());
    }

    #[test]
    fn eviction_linger_is_off_by_default() {
        // The default has to leave old output alone — someone reading a
        // result back an hour later is a supported thing to do.
        let base = Instant::now();
        let exited = vec![(1, at(base, 0))];
        assert!(
            slots_to_evict(
                exited,
                at(base, 100_000),
                DEFAULT_MAX_EXITED,
                DEFAULT_EXITED_LINGER
            )
            .is_empty()
        );
    }

    #[test]
    fn eviction_applies_the_linger_bound_when_set() {
        let base = Instant::now();
        let exited = vec![(1, at(base, 0)), (2, at(base, 50)), (3, at(base, 90))];
        // At t=100 with a 60s linger only 1 is old enough: 2 has been gone
        // 50s and 3 only 10s.
        assert_eq!(
            slots_to_evict(exited.clone(), at(base, 100), 0, Duration::from_secs(60)),
            vec![1]
        );
        // Push `now` out far enough and 2 crosses the line too.
        assert_eq!(
            slots_to_evict(exited, at(base, 120), 0, Duration::from_secs(60)),
            vec![1, 2]
        );
    }

    #[test]
    fn eviction_does_not_repeat_an_id_caught_by_both_bounds() {
        let base = Instant::now();
        let exited = vec![(1, at(base, 0)), (2, at(base, 1)), (3, at(base, 2))];
        // 1 is both the oldest over the count bound and past the linger.
        let doomed = slots_to_evict(exited, at(base, 100), 2, Duration::from_secs(50));
        let mut unique = doomed.clone();
        unique.dedup();
        assert_eq!(doomed, unique);
        assert_eq!(doomed, vec![1, 2, 3]);
    }

    #[test]
    fn arming_a_deadline_stands_down_a_pending_kill() {
        let now = Instant::now();
        // The case that matters: a refresh arriving inside the
        // SIGTERM→SIGKILL grace. It must cancel the pending kill, or the
        // terminal dies anyway a few seconds after the client said keep it.
        let (deadline, stop, reason) = armed_deadline(now, 30_000);
        assert_eq!(deadline, Some(now + Duration::from_secs(30)));
        assert_eq!(stop, None);
        assert_eq!(reason, EXIT_REASON_NORMAL);
    }

    #[test]
    fn clearing_a_deadline_disarms_everything() {
        let now = Instant::now();
        let (deadline, stop, reason) = armed_deadline(now, 0);
        assert_eq!(deadline, None);
        assert_eq!(stop, None);
        assert_eq!(reason, EXIT_REASON_NORMAL);
    }
}
