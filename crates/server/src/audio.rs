//! Audio capture pipeline: PipeWire monitor capture → Opus encode.
//!
//! Each compositor instance gets its own PipeWire + pipewire-pulse pair.
//! Apps connect via PulseAudio; PipeWire mixes into a null sink and an
//! in-process monitor stream supplies timestamped interleaved f32 PCM. We
//! frame it into 20 ms chunks and Opus-encode it for delivery.

use opus::{Application, Channels, Encoder as OpusEncoder};
use std::collections::{HashMap, VecDeque};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Returns a closure suitable for `Command::pre_exec` that sets
/// `PR_SET_PDEATHSIG(SIGTERM)` so the child is killed when the parent (yas
/// server) dies — even via SIGKILL where Rust destructors can't run.
///
pub(crate) fn pdeathsig_hook() -> impl FnMut() -> std::io::Result<()> {
    // SAFETY: `prctl(PR_SET_PDEATHSIG, …)` is async-signal-safe and runs in
    // the child between fork and exec.
    || unsafe {
        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

/// 48 kHz, stereo, 20 ms = 960 samples per channel = 1920 interleaved samples.
const FRAME_SAMPLES: usize = 960;
const CHANNELS: usize = 2;
const FRAME_FLOATS: usize = FRAME_SAMPLES * CHANNELS;
/// Maximum Opus packet size (RFC 6716 recommends 4000 bytes as upper bound).
const MAX_OPUS_PACKET: usize = 4000;

/// Default Opus bitrate in bits/sec.
pub const DEFAULT_BITRATE: i32 = 64_000;

/// Server-side ring buffer depth: 200 ms = 10 Opus frames at 20 ms.
/// Also sizes the encoder -> fan-out channel (at `RING_CAPACITY * 2`),
/// which needs slack for a briefly-descheduled fan-out task — so keep
/// this independent of how much of the ring we actually replay.
const RING_CAPACITY: usize = 10;

/// How many of the ring's newest frames a new subscriber is sent as
/// catch-up.  Only enough to prime the client's jitter buffer past its
/// floor (3 frames / 60 ms) and cover the gap until the first live
/// frame; every frame beyond that is start-up latency the client then
/// has to spend seconds draining at its ±2 % servo rate.  Replaying the
/// full 200 ms ring put every fresh subscribe ~140 ms in the hole.
const CATCHUP_FRAMES: usize = 4;

/// Minimum interval between sub-process heal attempts.
const HEAL_COOLDOWN: Duration = Duration::from_secs(1);
/// Maximum sub-process restarts in a burst window before giving up.
const MAX_HEALS: u32 = 5;
/// Duration of the burst window for counting heal attempts.
const HEAL_WINDOW: Duration = Duration::from_secs(30);

/// An encoded Opus frame ready for wire delivery.
#[derive(Clone)]
pub struct OpusFrame {
    /// Wall-clock milliseconds since the compositor epoch — same timebase
    /// as video frame timestamps for A/V sync.
    pub timestamp: u32,
    /// Opus-encoded bytes.
    pub data: Vec<u8>,
}

/// Shared state between the per-client subscribe/unsubscribe API on
/// [`AudioPipeline`] and the fan-out task that drains encoded frames
/// from the encoder.
///
/// Lives outside the pipeline so it persists across pipeline restarts:
/// clients stay subscribed even when pw-cat or the encoder task dies and
/// is respawned.  Wrap in `Arc` at the caller.
pub struct AudioBroadcast {
    /// Native YAS subscribers receive semantic encoded frames directly.
    native_subscribers: std::sync::Mutex<HashMap<u64, mpsc::Sender<OpusFrame>>>,
    /// Requested native output bitrates, keyed by the same private backend
    /// owner as `native_subscribers`.
    native_bitrates_kbps: std::sync::Mutex<HashMap<u64, u16>>,
    /// Client-measured audible-audio latency beyond visible-video latency,
    /// keyed by backend owner. The shared graph publishes the maximum so an
    /// application remains synchronized for every active listener.
    native_playout_delays_ns: std::sync::Mutex<HashMap<u64, u64>>,
    /// Recent frames for catch-up on new subscribers.  Kept in sync with
    /// delivery: every frame delivered to subscribers is first appended
    /// here, so a late-subscribing client gets the same tail.
    ring: std::sync::Mutex<VecDeque<OpusFrame>>,
    /// Shared flag telling the encoder task whether to bother encoding.
    /// Updated atomically from subscribe/unsubscribe.  Encoder still
    /// drains pw-cat's pipe (to avoid PipeWire backpressure) but skips
    /// the Opus encode when no one is listening.
    has_listener: Arc<AtomicBool>,
}

impl AudioBroadcast {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            native_subscribers: std::sync::Mutex::new(HashMap::new()),
            native_bitrates_kbps: std::sync::Mutex::new(HashMap::new()),
            native_playout_delays_ns: std::sync::Mutex::new(HashMap::new()),
            ring: std::sync::Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
            has_listener: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Register a native YAS output stream. Catch-up and live frames use the
    /// same ring ordering as every subscriber, but remain typed `OpusFrame`
    /// values until the YAS Media encoder consumes them.
    pub fn subscribe_native(&self, id: u64, bitrate_kbps: u16, tx: mpsc::Sender<OpusFrame>) {
        let ring_guard = self.ring.lock().unwrap();
        let skip = ring_guard.len().saturating_sub(CATCHUP_FRAMES);
        for frame in ring_guard.iter().skip(skip) {
            let _ = tx.try_send(frame.clone());
        }
        self.native_subscribers.lock().unwrap().insert(id, tx);
        self.native_bitrates_kbps
            .lock()
            .unwrap()
            .insert(id, bitrate_kbps);
        self.native_playout_delays_ns.lock().unwrap().insert(id, 0);
        self.has_listener.store(true, Ordering::Release);
    }

    /// Remove a native YAS output stream. Idempotent.
    pub fn unsubscribe_native(&self, id: u64) {
        self.native_subscribers.lock().unwrap().remove(&id);
        self.native_bitrates_kbps.lock().unwrap().remove(&id);
        self.native_playout_delays_ns.lock().unwrap().remove(&id);
        if self.native_subscribers.lock().unwrap().is_empty() {
            self.has_listener.store(false, Ordering::Release);
        }
    }

    pub fn max_native_bitrate_kbps(&self) -> u16 {
        self.native_bitrates_kbps
            .lock()
            .unwrap()
            .values()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// Update one active viewer's measured extra audio latency and return the
    /// maximum that the shared PipeWire graph must advertise.
    pub fn set_native_playout_delay_ns(&self, id: u64, delay_ns: u64) -> Option<(bool, u64)> {
        let mut delays = self.native_playout_delays_ns.lock().unwrap();
        let previous_max = delays.values().copied().max().unwrap_or(0);
        let delay = delays.get_mut(&id)?;
        *delay = delay_ns;
        let maximum = delays.values().copied().max().unwrap_or(0);
        Some((maximum != previous_max, maximum))
    }

    pub fn max_native_playout_delay_ns(&self) -> u64 {
        self.native_playout_delays_ns
            .lock()
            .unwrap()
            .values()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// Publish one semantic frame into native subscribers without starting
    /// PipeWire. Integration tests use this to cover the typed Media fan-out
    /// path without encoding a transport frame.
    #[cfg(test)]
    pub(crate) fn publish_native_for_test(&self, frame: OpusFrame) {
        {
            let mut ring = self.ring.lock().unwrap();
            if ring.len() >= RING_CAPACITY {
                ring.pop_front();
            }
            ring.push_back(frame.clone());
        }
        for tx in self.native_subscribers.lock().unwrap().values() {
            let _ = tx.try_send(frame.clone());
        }
    }

    fn has_listener_flag(&self) -> Arc<AtomicBool> {
        self.has_listener.clone()
    }
}

/// Dedicated task: drains encoded Opus frames from the encoder's MPSC and
/// fans them out to every subscribed client, **off the main server tick
/// loop**.  Running independently is the whole point — on a shared-tick
/// design, long video writes or compositor work would starve audio
/// delivery and the bounded encoder channel would overflow and silently
/// drop frames, starving the client's jitter buffer below real-time.
/// Runs on its own thread for the same reason the encoder does: it carries a
/// 20 ms frame on a deadline, and as a runtime task it waited behind whatever
/// else the runtime was doing. A second viewer is enough to show it — measured
/// with one scrolling while the other listened, the listener's connection sat
/// idle for 37-75 ms with nothing on its socket at all, because the frame had
/// not reached its queue yet.
fn fanout_task(mut opus_rx: mpsc::Receiver<OpusFrame>, broadcast: Arc<AudioBroadcast>) {
    while let Some(frame) = opus_rx.blocking_recv() {
        {
            let mut ring = broadcast.ring.lock().unwrap();
            if ring.len() >= RING_CAPACITY {
                ring.pop_front();
            }
            ring.push_back(frame.clone());
        }
        let native_subs = broadcast.native_subscribers.lock().unwrap();
        for tx in native_subs.values() {
            let _ = tx.try_send(frame.clone());
        }
    }
}

/// Manages the PipeWire child processes and produces Opus frames.
pub struct AudioPipeline {
    pipewire_child: Child,
    wireplumber_child: Option<Child>,
    pipewire_pulse_child: Child,
    /// In-process PipeWire capture stream (replaces pw-cat).  `None`
    /// only transiently during construction failure paths.
    capture: Option<crate::audio_pw::Capture>,
    /// The XDG_RUNTIME_DIR used by this pipeline's PipeWire instance.
    pub runtime_dir: PathBuf,
    /// True when the pipeline is still running.
    alive: bool,
    /// Send bitrate updates to the encoder task.
    bitrate_tx: tokio::sync::watch::Sender<i32>,
    /// Shared flag set to `false` when the encoder task exits.
    encoder_alive: Arc<AtomicBool>,
    /// D-Bus session bus address for restarting sub-processes.
    dbus_address: String,
    /// Verbose logging flag.
    verbose: bool,
    /// Last sub-process heal attempt timestamp.
    last_heal: Option<Instant>,
    /// Start of the current heal burst window.
    first_heal_at: Option<Instant>,
    /// Number of heals in the current burst window.
    heals: u32,
}

/// PipeWire configuration template.
///
/// Clock quanta are sized for the remote-desktop path, not local
/// monitoring: the browser client sits behind a >= 60 ms jitter buffer,
/// so a 21 ms graph cycle (1024/48000) adds no perceptible latency while
/// giving the (possibly non-RT) graph threads 4x more scheduling slack
/// per deadline than the PipeWire default under encode-saturated CPU.
/// `min-quantum` stops any client stream from dragging the graph back
/// down to millisecond cycles it can't reliably meet.
const PIPEWIRE_CONF_TEMPLATE: &str = r#"
context.properties = {
    core.daemon          = true
    core.name            = pipewire-0
    default.clock.rate   = 48000
    default.clock.quantum     = 1024
    default.clock.min-quantum = 1024
    default.clock.max-quantum = 2048
}
context.spa-libs = {
    audio.convert.* = audioconvert/libspa-audioconvert
    support.*       = support/libspa-support
}
context.modules = [
    # RT scheduling for the graph threads (falls back to nice -11 when
    # RLIMIT_RTPRIO / RTKit aren't available).  Without this the stripped
    # config runs the timer-driven null sink at SCHED_OTHER, and heavy
    # video-encode load on the same host makes graph cycles miss their
    # deadlines — audible as capture gaps baked into the Opus stream.
    { name = libpipewire-module-rt
        args = {
            nice.level   = -11
            rt.prio      = 88
            rt.time.soft = -1
            rt.time.hard = -1
        }
        flags = [ ifexists nofail ]
    }
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-access }
    # Required for a viewer's lent camera to be visible to applications.
    #
    # The camera portal does not hand an application a view of the graph: it
    # hands over a PipeWire fd with every node hidden, and expects the session
    # manager to grant per-node access. WirePlumber's `access-portal` script is
    # that grant, but it only ever looks at `pipewire.access.portal.*`
    # properties — and *this* module is what sets them, by asking the portal
    # over D-Bus who the connecting client is. Without it those properties are
    # never set, the script never fires, and a browser enumerates zero cameras
    # while `yas-camera` sits plainly in the graph. The measured symptom is a
    # bare "Enumerating PipeWire camera devices complete." with no camera found.
    #
    # `nofail`, and no `condition`: on a host without D-Bus this is simply
    # absent, exactly like the rest of the optional chain in
    # WIREPLUMBER_CONF_TEMPLATE.
    { name = libpipewire-module-portal
        flags = [ ifexists nofail ]
    }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-adapter }
    { name = libpipewire-module-link-factory }
    { name = libpipewire-module-metadata }
    # Without this, `pw-top` and `pw-profiler` print *nothing* against a yas
    # daemon — they need the Profiler interface this module registers, and the
    # stripped config above supplies no other route to it. That makes xruns
    # unobservable in production, which is the one number worth having when a
    # listener reports dropouts: the graph either missed its deadlines or it
    # did not, and every other explanation lives downstream of that answer.
    # The cost is a registry object; the per-cycle profiling work only runs
    # while a client is actually subscribed to it.
    { name = libpipewire-module-profiler }
    { name = libpipewire-module-spa-node-factory }
]
context.objects = [
    {   factory = adapter
        args = {
            factory.name          = support.null-audio-sink
            node.name             = yas-sink
            # What applications show in their device lists. Named for its
            # direction, to pair with the "Input" the microphone lease
            # publishes: an app choosing between them should see one naming
            # scheme, not a product name on one side and an internal node
            # name on the other.
            node.nick             = Output
            node.description      = Output
            media.class           = Audio/Sink
            object.linger         = true
            audio.position        = [ FL FR ]
            audio.rate            = 48000
            monitor.channel-volumes = true
            monitor.passthrough     = true
        }
    }
]
"#;

/// Minimal WirePlumber configuration: stream linking policy, plus the portal
/// access script a lent camera needs. No ALSA, Bluetooth, host camera
/// enumeration, MPRIS, or device reservation.
///
/// `hardware.audio` MUST stay enabled (the default) — it contains
/// `policy.node`, the module that links playback streams to sinks.
/// Without it, apps like mpv hang because their audio stream is never
/// connected to yas-sink.  We disable only the sub-features we don't
/// need (ALSA monitor, device reservation).
///
/// `support.dbus` and the two features above it MUST stay enabled for a
/// viewer's lent camera to be visible to applications. The camera portal does
/// not hand an application a view of the graph — it hands it a PipeWire fd
/// with *every* node hidden (`PW_PERMISSION_INIT(PW_ID_ANY, 0)`) and leaves it
/// to the session manager to grant visibility. `script.client.access-portal`
/// is that grant, and it needs `support.portal-permissionstore`, which needs
/// `support.dbus`. Disable any link in that chain and a browser enumerates
/// zero cameras while `yas-camera` sits plainly in the graph — and Chromium
/// does not fall back to V4L2 once its PipeWire factory is live, so the
/// failure is a silent empty device list.
///
/// They are `optional` (WirePlumber's default) rather than `required`: a host
/// without a usable D-Bus still gets audio, just no lendable camera. This does
/// not race the system WirePlumber, which an earlier blanket
/// `support.dbus = disabled` was guarding against — yas runs its instance
/// against its own private bus.
const WIREPLUMBER_CONF_TEMPLATE: &str = r#"
wireplumber.profiles = {
  main = {
    support.dbus = optional
    support.portal-permissionstore = optional
    script.client.access-portal = optional
    support.reserve-device = disabled
    # hardware.audio stays enabled — its policy.node links streams to sinks.
    hardware.bluetooth = disabled
    # No host camera enumeration: the only video source on this graph is the
    # one yas publishes for a viewer's lent camera.
    hardware.video-capture = disabled
    monitor.alsa = disabled
    monitor.alsa.reserve-device = disabled
    monitor.bluez = disabled
    monitor.bluez.midi = disabled
    monitor.bluez.seat-monitoring = disabled
    monitor.libcamera = disabled
    monitor.v4l2 = disabled
  }
}
"#;

/// Resolve a program to an absolute path by searching $PATH.
fn find_program(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    for dir in path_var.split(':') {
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Check whether the required PipeWire + D-Bus binaries and
/// `libpipewire-0.3.so.0` are available.  Capture is done in-process
/// via `audio_pw`, so `pw-cat` is no longer required.
pub fn pipewire_available() -> bool {
    missing_pipewire_binaries().is_empty() && crate::audio_pw::available()
}

/// Returns the list of required PipeWire / D-Bus binaries that are not
/// found on `$PATH`.  Empty list means audio can run (provided
/// libpipewire is also loadable at runtime; see `pipewire_available`).
pub fn missing_pipewire_binaries() -> Vec<&'static str> {
    ["pipewire", "pipewire-pulse"]
        .into_iter()
        .filter(|name| find_program(name).is_none())
        .collect()
}

/// Poll for a socket file to appear, sleeping 50 ms between checks.
/// Returns `true` if the socket appeared within `timeout`, `false` otherwise.
/// Falls back gracefully on timeout — the caller proceeds with a best-effort
/// attempt rather than failing hard.
fn wait_for_socket(path: &Path, timeout: std::time::Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// Name of the private runtime directory holding instance `instance_id`'s
/// PipeWire and pulse sockets.  Scoped to this process; see the call site in
/// `spawn` for why the pid has to be in there.
fn audio_dir_name(instance_id: u16) -> String {
    format!("yas-audio-{}-{instance_id}", std::process::id())
}

/// Delete `yas-audio-<pid>-<instance>` directories whose owning yas server
/// is gone.  Restarting a pipeline inside one process reuses its own name, so
/// the only leftovers are from servers that exited without running
/// `shutdown()` — SIGKILL, or a panic that skipped the `Drop`.
///
/// Deliberately conservative, since the whole point of the pid in the name is
/// to stop one server from deleting another's live sockets.  A directory goes
/// only when its name parses, no process holds the pid, and we own it.
/// Anything else is left alone because it cannot be attributed safely.
///
/// The pid check assumes the directory's owner shares our PID namespace.
/// That holds for servers sharing a runtime dir on one host; a container that
/// bind-mounts the host's temp dir but not its PID namespace could still see
/// a live directory as stale.
fn sweep_stale_audio_dirs(runtime_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(runtime_dir) else {
        return;
    };
    // SAFETY: `getuid` is always successful and has no preconditions.
    let self_uid = unsafe { libc::getuid() };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix("yas-audio-")) else {
            continue;
        };
        let Some((pid, instance)) = rest.split_once('-') else {
            continue;
        };
        if pid.parse::<u32>().is_err() || instance.parse::<u16>().is_err() {
            continue;
        }
        if Path::new("/proc").join(pid).exists() {
            continue;
        }
        // symlink_metadata, not metadata: never follow a link planted in a
        // shared runtime dir such as /tmp.
        let path = entry.path();
        let Ok(meta) = path.symlink_metadata() else {
            continue;
        };
        if !meta.is_dir() || meta.uid() != self_uid {
            continue;
        }
        let _ = std::fs::remove_dir_all(&path);
    }
}

impl AudioPipeline {
    /// Spawn a new PipeWire instance and start capturing audio.
    ///
    /// `runtime_dir` is the XDG_RUNTIME_DIR for this compositor instance.
    /// `instance_id` names the PipeWire remote uniquely *within this
    /// process*; see the runtime-dir comment below for why that is not
    /// enough on its own.
    /// `bitrate` is the Opus encoder bitrate in bits/sec (0 = default).
    /// `broadcast` is the shared fan-out state; pass the same `Arc` across
    /// restarts so subscribed clients stay connected to the output.
    pub fn spawn(
        runtime_dir: &Path,
        instance_id: u16,
        dbus_address: &str,
        bitrate: i32,
        verbose: bool,
        broadcast: Arc<AudioBroadcast>,
    ) -> Result<Self, String> {
        // Use a private subdirectory so the PulseAudio socket doesn't
        // collide with the system's or with other yas instances.
        //
        // The pid is part of the name because `instance_id` is a per-server
        // counter that restarts at 1 in every yas server process — it is
        // unique within one server, not across servers.  Two servers whose
        // runtime dirs resolve to the same place both pick instance 1, and
        // that is the common case rather than an exotic one: a compositor
        // with no writable XDG_RUNTIME_DIR falls back to the temp dir, and
        // it exports that fallback to its PTY children, so a dev stack
        // started from inside a yas terminal inherits it too.  Both then
        // land on /tmp/yas-audio-1, and the `remove_dir_all` below deletes
        // the other server's live sockets: the loser's capture stream ends
        // up reading a daemon that nothing feeds, its pipeline restarts,
        // and the restart deletes the winner's directory in turn.  Audio
        // then ping-pongs between the two servers, silent on whichever one
        // did not spawn last.
        let audio_dir = runtime_dir.join(audio_dir_name(instance_id));

        // Remove leftovers from a previous unclean exit so we don't trip
        // over stale PipeWire/pulse sockets ("Address already in use").
        if audio_dir.exists() {
            let _ = std::fs::remove_dir_all(&audio_dir);
        }

        // Pid-scoped names are never reused, so a server that died without
        // running `shutdown()` leaves its directory behind for good unless
        // somebody collects it.
        sweep_stale_audio_dirs(runtime_dir);

        std::fs::create_dir_all(&audio_dir)
            .map_err(|e| format!("failed to create audio runtime dir: {e}"))?;

        // Write the config at $audio_dir/pipewire/pipewire.conf so that
        // setting XDG_CONFIG_HOME=$audio_dir makes PipeWire pick it up
        // from $XDG_CONFIG_HOME/pipewire/pipewire.conf — which takes
        // priority over system / nix-store configs on all versions.
        let conf_dir = audio_dir.join("pipewire");
        std::fs::create_dir_all(&conf_dir)
            .map_err(|e| format!("failed to create PipeWire config dir: {e}"))?;
        let conf_path = conf_dir.join("pipewire.conf");
        std::fs::write(&conf_path, PIPEWIRE_CONF_TEMPLATE)
            .map_err(|e| format!("failed to write PipeWire config: {e}"))?;

        if dbus_address.is_empty() {
            return Err("desktop D-Bus address is empty".into());
        }

        // 1. Start pipewire.
        //    XDG_CONFIG_HOME=$audio_dir makes PipeWire load
        //    $audio_dir/pipewire/pipewire.conf, which takes priority over
        //    system and nix-store configs on all PipeWire versions.
        let mut pipewire_child = match unsafe {
            Command::new("pipewire")
                .env("XDG_CONFIG_HOME", &audio_dir)
                .env("DBUS_SESSION_BUS_ADDRESS", dbus_address)
                .env("XDG_RUNTIME_DIR", &audio_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(if verbose {
                    Stdio::inherit()
                } else {
                    Stdio::null()
                })
                .pre_exec(pdeathsig_hook())
                .spawn()
        } {
            Ok(c) => c,
            Err(e) => return Err(format!("failed to start pipewire: {e}")),
        };

        // Wait for PipeWire to create its socket before spawning dependents.
        // Polls every 50 ms instead of a fixed 500 ms sleep — faster on fast
        // systems, more robust on slow ones (up to 2 s timeout).
        let pw_socket = audio_dir.join("pipewire-0");
        if !wait_for_socket(&pw_socket, std::time::Duration::from_secs(2)) {
            // Check that PipeWire hasn't already exited.
            if matches!(pipewire_child.try_wait(), Ok(Some(_))) {
                return Err("pipewire exited before creating its socket".into());
            }
            // Socket still missing but process alive — proceed anyway
            // (might just be slow; the next spawn will fail clearly).
        }

        // 1b. Start WirePlumber (session manager) if available.
        //     Without a session manager, pipewire-pulse can negotiate
        //     PulseAudio connections but can't create links between
        //     stream nodes and yas-sink — stream creation hangs.
        //     We use a minimal config that disables all hardware monitors
        //     (ALSA, Bluetooth, camera) to avoid conflicts with the
        //     system WirePlumber on the same D-Bus.
        let mut wireplumber_child = if find_program("wireplumber").is_some() {
            let wp_conf_dir = audio_dir.join("wireplumber").join("wireplumber.conf.d");
            let _ = std::fs::create_dir_all(&wp_conf_dir);
            let _ = std::fs::write(wp_conf_dir.join("99-yas.conf"), WIREPLUMBER_CONF_TEMPLATE);
            let child = unsafe {
                Command::new("wireplumber")
                    .env("PIPEWIRE_REMOTE", audio_dir.join("pipewire-0"))
                    .env("XDG_CONFIG_HOME", &audio_dir)
                    .env("DBUS_SESSION_BUS_ADDRESS", dbus_address)
                    .env("XDG_RUNTIME_DIR", &audio_dir)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(if verbose {
                        Stdio::inherit()
                    } else {
                        Stdio::null()
                    })
                    .pre_exec(pdeathsig_hook())
                    .spawn()
            };
            match child {
                Ok(c) => {
                    // Give WirePlumber a moment to register its policy module
                    // with PipeWire.  There's no socket to poll for here, so
                    // we use a short fixed sleep + liveness check.
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    Some(c)
                }
                Err(e) => {
                    if verbose {
                        eprintln!("[audio] failed to start wireplumber: {e}");
                    }
                    None
                }
            }
        } else {
            None
        };

        // 2. Start pipewire-pulse.
        let mut pipewire_pulse_child = match unsafe {
            Command::new("pipewire-pulse")
                .env("PIPEWIRE_REMOTE", audio_dir.join("pipewire-0"))
                .env("DBUS_SESSION_BUS_ADDRESS", dbus_address)
                .env("XDG_RUNTIME_DIR", &audio_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(if verbose {
                    Stdio::inherit()
                } else {
                    Stdio::null()
                })
                .pre_exec(pdeathsig_hook())
                .spawn()
        } {
            Ok(c) => c,
            Err(e) => {
                if let Some(ref mut wp) = wireplumber_child {
                    let _ = wp.kill();
                }
                let _ = pipewire_child.kill();
                if let Some(ref mut wp) = wireplumber_child {
                    let _ = wp.wait();
                }
                let _ = pipewire_child.wait();
                return Err(format!("failed to start pipewire-pulse: {e}"));
            }
        };

        // Wait for pipewire-pulse to create the PulseAudio socket.
        let pulse_socket = audio_dir.join("pulse").join("native");
        if !wait_for_socket(&pulse_socket, std::time::Duration::from_secs(2))
            && matches!(pipewire_pulse_child.try_wait(), Ok(Some(_)))
        {
            if let Some(ref mut wp) = wireplumber_child {
                let _ = wp.kill();
            }
            let _ = pipewire_child.kill();
            if let Some(ref mut wp) = wireplumber_child {
                let _ = wp.wait();
            }
            let _ = pipewire_child.wait();
            return Err("pipewire-pulse exited before creating its socket".into());
        }

        // 3. Open an in-process PipeWire capture stream on yas-sink's
        //    monitor.  No more pw-cat subprocess, no pipe buffer — the
        //    RT callback hands us PCM frames directly.  Target by name
        //    since the `target.object` property accepts node names.
        let (capture, capture_rx) = match crate::audio_pw::Capture::start(&audio_dir, "yas-sink") {
            Ok(pair) => pair,
            Err(e) => {
                let _ = pipewire_pulse_child.kill();
                if let Some(ref mut wp) = wireplumber_child {
                    let _ = wp.kill();
                }
                let _ = pipewire_child.kill();
                let _ = pipewire_pulse_child.wait();
                if let Some(ref mut wp) = wireplumber_child {
                    let _ = wp.wait();
                }
                let _ = pipewire_child.wait();
                return Err(format!("failed to start PipeWire capture: {e}"));
            }
        };
        let restored_playout_delay_ns = broadcast.max_native_playout_delay_ns();
        if let Err(error) = capture.set_process_latency_ns(restored_playout_delay_ns) {
            if verbose {
                eprintln!("[audio] failed to restore remote playout latency: {error}");
            }
        }

        if verbose {
            eprintln!(
                "[audio] spawned pipewire={} pipewire-pulse={} capture=in-process dir={}",
                pipewire_child.id(),
                pipewire_pulse_child.id(),
                audio_dir.display(),
            );
        }

        // Spawn the encoder on its own thread.
        let (opus_tx, opus_rx) = mpsc::channel::<OpusFrame>(RING_CAPACITY * 2);
        let bitrate = if bitrate > 0 {
            bitrate
        } else {
            DEFAULT_BITRATE
        };
        let (bitrate_tx, bitrate_rx) = tokio::sync::watch::channel(bitrate);
        let encoder_alive = Arc::new(AtomicBool::new(true));
        let encoder_alive_clone = encoder_alive.clone();
        let has_listener = broadcast.has_listener_flag();
        let has_listener_clone = has_listener.clone();
        let verbose_copy = verbose;
        // Its own thread, not the shared runtime.
        //
        // Everything in the loop except waiting for the next capture chunk
        // is CPU work — an f32 conversion and an Opus encode, every 20 ms,
        // on a deadline nothing reschedules. As a `tokio::spawn` task it
        // queued behind whatever else the runtime was doing: video encode
        // orchestration, and one writer per connected client. Measured with
        // seventeen of those, audio came out in bursts with 43-71 ms holes
        // while the capture thread itself was on time, which is a gap no
        // client-side buffer sized for a 20 ms cadence can absorb.
        //
        // PipeWire's graph thread is already SCHED_FIFO; this one is left at
        // the default. It has a whole quantum of slack and would rather be
        // preempted than compete with the thread feeding it.
        if let Err(e) = std::thread::Builder::new()
            .name("yas-audio-enc".into())
            .spawn(move || {
                let result = encoder_task(
                    capture_rx,
                    opus_tx,
                    bitrate,
                    verbose_copy,
                    bitrate_rx,
                    has_listener_clone,
                );
                encoder_alive_clone.store(false, Ordering::Release);
                if let Err(e) = result
                    && verbose_copy
                {
                    eprintln!("[audio] encoder task exited: {e}");
                }
            })
        {
            return Err(format!("failed to spawn audio encoder thread: {e}"));
        }

        // Spawn the fan-out task: drains encoded frames from the encoder
        // and pushes them to every subscribed client's mpsc, independent
        // of the main server tick loop so long video writes can't starve
        // audio delivery.
        let broadcast_for_fanout = broadcast.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("yas-audio-fan".into())
            .spawn(move || fanout_task(opus_rx, broadcast_for_fanout))
        {
            return Err(format!("failed to spawn audio fanout thread: {e}"));
        }

        Ok(Self {
            pipewire_child,
            wireplumber_child,
            pipewire_pulse_child,
            capture: Some(capture),
            runtime_dir: audio_dir,
            alive: true,
            bitrate_tx,
            encoder_alive,
            dbus_address: dbus_address.to_string(),
            verbose,
            last_heal: None,
            first_heal_at: None,
            heals: 0,
        })
    }

    /// Collect any sub-process that has exited, and nothing else.
    ///
    /// `is_alive` already reaps as a side effect of its `try_wait` calls,
    /// but it lives in the delivery tick, which only runs while a client is
    /// attached — and it heals as well as observes, which is not something to
    /// do on a timer nobody asked for.  A server sitting idle with a dead
    /// PipeWire would keep the corpse until someone connected.
    ///
    /// That used to be covered by the reaper draining `waitpid(-1)` for the
    /// whole process; once that narrowed to PTY-owned pids, these had nobody.
    /// So the supervisor calls this instead: same `try_wait`, no restart, no
    /// state change beyond releasing a zombie.
    pub fn reap_children(&mut self) {
        let _ = self.pipewire_child.try_wait();
        let _ = self.pipewire_pulse_child.try_wait();
        if let Some(ref mut wp) = self.wireplumber_child {
            let _ = wp.try_wait();
        }
    }

    /// Returns true if the pipeline is still producing (or can resume
    /// producing) audio.
    ///
    /// Automatically restarts dead sub-processes (WirePlumber,
    /// pipewire-pulse, pw-cat/encoder) without tearing down the entire
    /// pipeline. Only returns false when PipeWire dies or sub-process
    /// restarts keep failing. The compositor service bundle supervises its
    /// shared desktop D-Bus separately.
    pub fn is_alive(&mut self) -> bool {
        if !self.alive {
            return false;
        }

        // Core processes: if dead, the whole pipeline must be rebuilt.
        if matches!(self.pipewire_child.try_wait(), Ok(Some(_))) {
            self.alive = false;
            return false;
        }

        // Detect dead sub-processes.  Compute booleans first so we don't
        // hold borrows across the restart calls that take &mut self.
        let wp_dead = self
            .wireplumber_child
            .as_mut()
            .is_some_and(|wp| matches!(wp.try_wait(), Ok(Some(_))));
        let pulse_dead = matches!(self.pipewire_pulse_child.try_wait(), Ok(Some(_)));
        let encoder_dead = !self.encoder_alive.load(Ordering::Acquire);

        let needs_heal = wp_dead || pulse_dead || encoder_dead;
        if !needs_heal {
            return true;
        }

        // Rate-limit heal attempts.
        let now = Instant::now();
        let can_heal = self
            .last_heal
            .is_none_or(|t| now.duration_since(t) >= HEAL_COOLDOWN);
        if !can_heal {
            // Still in cooldown — return true so the outer code doesn't
            // trigger a full pipeline restart while we're healing.
            return true;
        }

        // Burst limiter: give up after too many restarts in a window.
        if self
            .first_heal_at
            .is_none_or(|t| now.duration_since(t) > HEAL_WINDOW)
        {
            self.first_heal_at = Some(now);
            self.heals = 0;
        }
        self.heals += 1;
        if self.heals > MAX_HEALS {
            eprintln!(
                "[audio] too many sub-process restarts ({}), giving up",
                self.heals
            );
            self.alive = false;
            return false;
        }
        self.last_heal = Some(now);

        // Restart dead sub-processes individually.

        if wp_dead {
            eprintln!("[audio] wireplumber died, restarting");
            self.restart_wireplumber();
        }

        if pulse_dead {
            eprintln!("[audio] pipewire-pulse died, restarting");
            self.restart_pipewire_pulse();
        }

        if encoder_dead {
            // The encoder task can only exit if its capture receiver
            // closed (PipeWire stream gone) or it hit an unrecoverable
            // encode error.  Restarting the in-process capture cleanly
            // is not supported yet — bail so the caller triggers a full
            // pipeline restart (which re-spawns everything).
            eprintln!("[audio] encoder died, triggering full pipeline restart");
            self.alive = false;
            return false;
        }

        true
    }

    /// Kill all child processes and clean up.
    pub fn shutdown(&mut self) {
        self.alive = false;
        // Stop the in-process capture first so the PW thread-loop has
        // joined before we tear the daemon down under it.
        self.capture.take();
        let _ = self.pipewire_pulse_child.kill();
        if let Some(ref mut wp) = self.wireplumber_child {
            let _ = wp.kill();
        }
        let _ = self.pipewire_child.kill();
        let _ = self.pipewire_pulse_child.wait();
        if let Some(ref mut wp) = self.wireplumber_child {
            let _ = wp.wait();
        }
        let _ = self.pipewire_child.wait();
        // Remove the private runtime directory and everything in it
        // (config file, PipeWire socket, pulse/native socket, etc.).
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }

    /// Restart a dead WirePlumber sub-process.
    fn restart_wireplumber(&mut self) {
        if let Some(ref mut wp) = self.wireplumber_child {
            let _ = wp.kill();
            let _ = wp.wait();
        }
        let child = unsafe {
            Command::new("wireplumber")
                .env("PIPEWIRE_REMOTE", self.runtime_dir.join("pipewire-0"))
                .env("XDG_CONFIG_HOME", &self.runtime_dir)
                .env("DBUS_SESSION_BUS_ADDRESS", &self.dbus_address)
                .env("XDG_RUNTIME_DIR", &self.runtime_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(if self.verbose {
                    Stdio::inherit()
                } else {
                    Stdio::null()
                })
                .pre_exec(pdeathsig_hook())
                .spawn()
        };
        match child {
            Ok(c) => {
                self.wireplumber_child = Some(c);
            }
            Err(e) => {
                eprintln!("[audio] failed to restart wireplumber: {e}");
                self.wireplumber_child = None;
            }
        }
    }

    /// Restart a dead pipewire-pulse sub-process.
    fn restart_pipewire_pulse(&mut self) {
        let _ = self.pipewire_pulse_child.kill();
        let _ = self.pipewire_pulse_child.wait();
        // Remove stale PulseAudio socket to avoid "Address already in use".
        let _ = std::fs::remove_dir_all(self.runtime_dir.join("pulse"));
        match unsafe {
            Command::new("pipewire-pulse")
                .env("PIPEWIRE_REMOTE", self.runtime_dir.join("pipewire-0"))
                .env("DBUS_SESSION_BUS_ADDRESS", &self.dbus_address)
                .env("XDG_RUNTIME_DIR", &self.runtime_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(if self.verbose {
                    Stdio::inherit()
                } else {
                    Stdio::null()
                })
                .pre_exec(pdeathsig_hook())
                .spawn()
        } {
            Ok(c) => {
                self.pipewire_pulse_child = c;
            }
            Err(e) => {
                eprintln!("[audio] failed to restart pipewire-pulse: {e}");
            }
        }
    }

    /// Update the Opus encoder bitrate. Takes effect on the next frame.
    pub fn set_bitrate(&self, bitrate: i32) {
        let _ = self.bitrate_tx.send(bitrate);
    }

    /// Publish remote output latency to applications without buffering YAS
    /// video. PipeWire propagates this value upstream to Pulse/PipeWire
    /// playback clients, where their own A/V scheduler consumes it.
    pub fn set_playout_delay_ns(&self, delay_ns: u64) -> Result<(), String> {
        self.capture
            .as_ref()
            .ok_or_else(|| "PipeWire capture stream is unavailable".to_string())?
            .set_process_latency_ns(delay_ns)
    }

    /// Build the `PULSE_SERVER` value for child process environments.
    pub fn pulse_server_path(&self) -> String {
        let pulse_dir = self.runtime_dir.join("pulse");
        format!("unix:{}", pulse_dir.join("native").display())
    }

    /// Build the `PIPEWIRE_REMOTE` value for child process environments.
    ///
    /// Apps that speak PipeWire natively (mpv, Firefox, etc.) look for the
    /// PipeWire socket at `$XDG_RUNTIME_DIR/pipewire-0` by default.  Since the
    /// child's XDG_RUNTIME_DIR points at the Wayland socket directory (not the
    /// audio directory), those apps can't find the socket.  Setting
    /// PIPEWIRE_REMOTE to an absolute path lets them connect directly.
    pub fn pipewire_remote_path(&self) -> String {
        self.runtime_dir
            .join("pipewire-0")
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Async task: consumes raw PCM chunks delivered by the in-process
/// PipeWire capture (`audio_pw::Capture`), frames into 20 ms windows,
/// Opus-encodes, and sends to the fan-out channel.
///
fn encoder_task(
    mut pcm_rx: mpsc::Receiver<crate::audio_pw::CapturedAudioChunk>,
    tx: mpsc::Sender<OpusFrame>,
    bitrate: i32,
    verbose: bool,
    mut bitrate_rx: tokio::sync::watch::Receiver<i32>,
    has_listener: Arc<AtomicBool>,
) -> Result<(), String> {
    // Init Opus encoder.
    let mut encoder = OpusEncoder::new(48000, Channels::Stereo, Application::Audio)
        .map_err(|e| format!("failed to create Opus encoder: {e}"))?;
    encoder
        .set_bitrate(opus::Bitrate::Bits(bitrate))
        .map_err(|e| format!("failed to set Opus bitrate: {e}"))?;
    // DTX: during silence the encoder emits tiny frames (or none at all),
    // cutting both bitrate and CPU across the CELT analysis pipeline.
    if let Err(e) = encoder.set_dtx(true)
        && verbose
    {
        eprintln!("[audio] failed to enable Opus DTX: {e}");
    }
    let mut current_bitrate = bitrate;

    if verbose {
        eprintln!("[audio] encoder ready, bitrate={bitrate} bps");
    }

    let mut pcm_buf = vec![0f32; FRAME_FLOATS];
    let mut byte_buf: Vec<u8> = Vec::with_capacity(FRAME_FLOATS * 4 * 2);
    // CLOCK_MONOTONIC PTS of byte_buf's first sample. PipeWire capture
    // quanta (1024 samples) do not align with Opus frames (960 samples), so
    // carry the first-sample timestamp across the leftover between chunks.
    let mut buffer_pts_ns: Option<i64> = None;
    let mut opus_out = vec![0u8; MAX_OPUS_PACKET];

    loop {
        // Check for bitrate updates before reading the next chunk.
        if bitrate_rx.has_changed().unwrap_or(false) {
            let new_bitrate = *bitrate_rx.borrow_and_update();
            if new_bitrate != current_bitrate {
                if let Err(e) = encoder.set_bitrate(opus::Bitrate::Bits(new_bitrate)) {
                    if verbose {
                        eprintln!("[audio] failed to update bitrate to {new_bitrate}: {e}");
                    }
                } else {
                    if verbose {
                        eprintln!(
                            "[audio] bitrate updated: {current_bitrate} -> {new_bitrate} bps"
                        );
                    }
                    current_bitrate = new_bitrate;
                }
            }
        }

        // Receive the next capture chunk.  Chunks are whatever size
        // PipeWire gave us (typically one quantum ≈ 21 ms at 48 kHz for
        // the latency we requested), which we accumulate until we have
        // a full 20 ms Opus frame's worth of bytes.
        let chunk = match pcm_rx.blocking_recv() {
            Some(c) => c,
            None => return Ok(()), // capture closed
        };
        let chunk_pts_ns = (chunk.pts_ns != i64::MIN).then_some(chunk.pts_ns);
        if byte_buf.is_empty() {
            buffer_pts_ns = chunk_pts_ns;
        } else if let (Some(start), Some(actual)) = (buffer_pts_ns, chunk_pts_ns) {
            let buffered_frames = byte_buf.len() / (2 * std::mem::size_of::<f32>());
            let expected = start.saturating_add(
                i64::try_from(buffered_frames)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(1_000_000_000)
                    / 48_000,
            );
            // A full capture quantum disappeared before the encoder. Do not
            // splice the new samples onto the old tail and assign the splice a
            // continuous PTS: that under-reports both the gap and A/V delay.
            if actual.abs_diff(expected) > 1_000_000 {
                byte_buf.clear();
                buffer_pts_ns = Some(actual);
            }
        }
        byte_buf.extend_from_slice(&chunk.data);

        // Process all complete 20 ms frames in the buffer.
        while byte_buf.len() >= FRAME_FLOATS * 4 {
            let consumed = FRAME_FLOATS * 4;

            // When no client is listening, drain samples but skip the
            // per-frame f32 conversion and Opus encode — those are the
            // expensive steps.  We still must consume the bytes so the
            // capture's bounded PCM queue keeps accepting newer samples.
            if !has_listener.load(Ordering::Acquire) {
                consume_pcm_prefix(&mut byte_buf, &mut buffer_pts_ns, consumed);
                continue;
            }

            // Convert bytes to f32 samples (little-endian).
            for (i, sample) in pcm_buf.iter_mut().enumerate() {
                let off = i * 4;
                *sample = f32::from_le_bytes([
                    byte_buf[off],
                    byte_buf[off + 1],
                    byte_buf[off + 2],
                    byte_buf[off + 3],
                ]);
            }

            // Encode.  Skip the frame on error instead of killing the
            // entire pipeline — a single dropped 20 ms frame is inaudible.
            let encoded_len = match encoder.encode_float(&pcm_buf, &mut opus_out) {
                Ok(len) => len,
                Err(e) => {
                    if verbose {
                        eprintln!("[audio] Opus encode error, skipping frame: {e}");
                    }
                    consume_pcm_prefix(&mut byte_buf, &mut buffer_pts_ns, consumed);
                    continue;
                }
            };

            let timestamp = buffer_pts_ns.map_or(0, |pts| pts.div_euclid(1_000_000) as u32);
            let frame = OpusFrame {
                // First-sample CLOCK_MONOTONIC ms from PipeWire. Surface
                // capture uses that same wrapping u32 domain, so the browser
                // can finally compare the two paths and report sink latency.
                timestamp,
                data: opus_out[..encoded_len].to_vec(),
            };

            match tx.try_send(frame) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // Channel full — drop this frame rather than blocking.
                    // A dropped 20 ms Opus frame is inaudible; blocking
                    // here would propagate backpressure into PipeWire's
                    // RT thread and hang audio-producing apps.
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    // Receiver dropped — pipeline shutting down.
                    return Ok(());
                }
            }

            consume_pcm_prefix(&mut byte_buf, &mut buffer_pts_ns, consumed);
        }
    }
}

/// Consume interleaved stereo f32 PCM while advancing its first-sample PTS.
fn consume_pcm_prefix(bytes: &mut Vec<u8>, pts_ns: &mut Option<i64>, consumed: usize) {
    bytes.drain(..consumed);
    let frames = consumed / (2 * std::mem::size_of::<f32>());
    if let Some(pts) = pts_ns.as_mut() {
        *pts = pts.saturating_add(
            i64::try_from(frames)
                .unwrap_or(i64::MAX)
                .saturating_mul(1_000_000_000)
                / 48_000,
        );
    }
    if bytes.is_empty() {
        *pts_ns = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ts: u32) -> OpusFrame {
        OpusFrame {
            timestamp: ts,
            data: vec![0u8; 8],
        }
    }

    /// A new subscriber must be primed, but only just: replaying the whole
    /// ring hands it a jitter buffer that starts already behind live, and
    /// the client can only drain that at its ±2 % servo rate.
    #[test]
    fn catch_up_replays_only_the_newest_frames() {
        let bc = AudioBroadcast::new();
        {
            let mut ring = bc.ring.lock().unwrap();
            for ts in 0..RING_CAPACITY as u32 {
                ring.push_back(frame(ts));
            }
        }

        let (tx, mut rx) = mpsc::channel(crate::AUDIO_QUEUE_MAX_FRAMES);
        bc.subscribe_native(1, 64, tx);

        let mut got = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            got.push(frame.timestamp);
        }

        assert_eq!(got.len(), CATCHUP_FRAMES);
        // The newest frames, in order — never the stalest.
        let first = RING_CAPACITY as u32 - CATCHUP_FRAMES as u32;
        assert_eq!(got, (first..RING_CAPACITY as u32).collect::<Vec<_>>());
    }

    /// A ring shorter than the catch-up window is replayed whole rather
    /// than under-delivering.
    #[test]
    fn catch_up_handles_a_partially_filled_ring() {
        let bc = AudioBroadcast::new();
        bc.ring.lock().unwrap().push_back(frame(7));

        let (tx, mut rx) = mpsc::channel(crate::AUDIO_QUEUE_MAX_FRAMES);
        bc.subscribe_native(1, 64, tx);

        let frame = rx.try_recv().expect("the one ring frame");
        assert_eq!(frame.timestamp, 7);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn shared_playout_latency_tracks_the_slowest_live_viewer() {
        let broadcast = AudioBroadcast::new();
        let (first_tx, _first_rx) = mpsc::channel(crate::AUDIO_QUEUE_MAX_FRAMES);
        let (second_tx, _second_rx) = mpsc::channel(crate::AUDIO_QUEUE_MAX_FRAMES);
        broadcast.subscribe_native(1, 64, first_tx);
        broadcast.subscribe_native(2, 64, second_tx);

        assert_eq!(
            broadcast.set_native_playout_delay_ns(1, 80_000_000),
            Some((true, 80_000_000)),
        );
        assert_eq!(
            broadcast.set_native_playout_delay_ns(2, 140_000_000),
            Some((true, 140_000_000)),
        );
        assert_eq!(
            broadcast.set_native_playout_delay_ns(1, 100_000_000),
            Some((false, 140_000_000)),
        );

        broadcast.unsubscribe_native(2);
        assert_eq!(broadcast.max_native_playout_delay_ns(), 100_000_000);
        broadcast.unsubscribe_native(1);
        assert_eq!(broadcast.max_native_playout_delay_ns(), 0);
    }

    #[test]
    fn pcm_leftover_keeps_the_first_sample_on_the_monotonic_timeline() {
        let opus_bytes = FRAME_FLOATS * std::mem::size_of::<f32>();
        let mut bytes = vec![0; opus_bytes + 8];
        let mut pts = Some(4_321_000_000_000_i64);

        consume_pcm_prefix(&mut bytes, &mut pts, opus_bytes);
        assert_eq!(bytes.len(), 8);
        assert_eq!(pts, Some(4_321_020_000_000));

        consume_pcm_prefix(&mut bytes, &mut pts, 8);
        assert!(pts.is_none());
    }

    /// A pid with no `/proc` entry.  Scanning down from `pid_max` keeps us
    /// clear of the low numbers the kernel is about to hand out.
    fn dead_pid() -> u32 {
        let max = std::fs::read_to_string("/proc/sys/kernel/pid_max")
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(32768);
        (2..max)
            .rev()
            .find(|p| !Path::new("/proc").join(p.to_string()).exists())
            .expect("no free pid to use as a dead one")
    }

    #[test]
    fn sweep_collects_dead_servers_and_spares_everyone_else() {
        let root =
            std::env::temp_dir().join(format!("yas-audio-sweep-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Ours, for a pipeline that is currently running.  This is the case
        // the pid exists to protect: another server sweeping the same
        // runtime dir must not touch it.
        let live = root.join(audio_dir_name(1));
        // A server that died without cleaning up.
        let stale = root.join(format!("yas-audio-{}-1", dead_pid()));
        // Neither of these parses as `<pid>-<instance>`.
        let unparsable = root.join("yas-audio-nope-1");
        let unrelated = root.join("wayland-0");
        for d in [&live, &stale, &unparsable, &unrelated] {
            std::fs::create_dir(d).unwrap();
        }
        // Same name as a stale directory but a plain file, so `remove_dir_all`
        // must not be pointed at it.
        let file = root.join(format!("yas-audio-{}-2", dead_pid()));
        std::fs::write(&file, b"").unwrap();

        sweep_stale_audio_dirs(&root);

        assert!(live.exists(), "swept a live pipeline's directory");
        assert!(unparsable.exists(), "swept an unparsable name");
        assert!(unrelated.exists(), "swept an unrelated entry");
        assert!(file.exists(), "swept a non-directory");
        assert!(!stale.exists(), "left a dead server's directory behind");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn audio_dir_name_is_scoped_to_this_process() {
        let name = audio_dir_name(1);
        assert!(
            name.contains(&std::process::id().to_string()),
            "{name} carries no pid, so a second server would pick the same one"
        );
        // Distinct compositor sessions still get distinct directories.
        assert_ne!(audio_dir_name(1), audio_dir_name(2));
    }
}
