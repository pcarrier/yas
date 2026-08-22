//! Process-wide bounded binary event journal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, oneshot};
use yas_wire::events::{ACTIVATION_WORDS, ActivationSet};

pub(crate) const DEFAULT_RING_SIZE: usize = 1024 * 1024;
pub(crate) const MIN_RING_SIZE: usize = 4 * 1024;
pub(crate) const MAX_RING_SIZE: usize = yas_wire::schema::events::MAX_RING_BYTES as usize;
const LIVE_CHANNEL_RECORDS: usize = 4096;
pub(crate) const EVENT_DUMP_MAGIC: &[u8; 8] = b"YASEVT01";
pub(crate) const EVENT_DUMP_HEADER_LEN: usize = 84;
pub(crate) const EVENT_RECORD_HEADER_LEN: usize = 32;
const EVENT_TYPE_STREAM_GAP: u16 = u16::MAX;
pub(crate) const EVENTS_STREAM_HISTORY: u8 = yas_wire::schema::events::RECORDING_HISTORY as u8;
pub(crate) const EVENTS_STREAM_APPEND: u8 = yas_wire::schema::events::RECORDING_APPEND as u8;

/// Stable semantic event identifiers stored by the process journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum EventType {
    ServerStart = yas_wire::schema::events::EVENT_SERVER_START as u16,
    ServerStop = yas_wire::schema::events::EVENT_SERVER_STOP as u16,
    TaskStart = yas_wire::schema::events::EVENT_TASK_START as u16,
    TaskStop = yas_wire::schema::events::EVENT_TASK_STOP as u16,
    ClientConnect = yas_wire::schema::events::EVENT_CLIENT_CONNECT as u16,
    ClientDisconnect = yas_wire::schema::events::EVENT_CLIENT_DISCONNECT as u16,
    ClientReject = yas_wire::schema::events::EVENT_CLIENT_REJECT as u16,
    ConfigChange = yas_wire::schema::events::EVENT_CONFIG_CHANGE as u16,
    StreamStart = yas_wire::schema::events::EVENT_STREAM_START as u16,
    StreamStop = yas_wire::schema::events::EVENT_STREAM_STOP as u16,
    ProtocolError = yas_wire::schema::events::EVENT_PROTOCOL_ERROR as u16,
    PtyCreate = yas_wire::schema::events::EVENT_PTY_CREATE as u16,
    PtyExit = yas_wire::schema::events::EVENT_PTY_EXIT as u16,
    PtyRemove = yas_wire::schema::events::EVENT_PTY_REMOVE as u16,
    Deadline = yas_wire::schema::events::EVENT_PTY_DEADLINE as u16,
    Capacity = yas_wire::schema::events::EVENT_SERVER_CAPACITY as u16,
    FrameRead = yas_wire::schema::events::EVENT_FRAME_READ as u16,
    FrameWrite = yas_wire::schema::events::EVENT_FRAME_WRITE as u16,
    MessageRead = yas_wire::schema::events::EVENT_MESSAGE_READ as u16,
    MessageWrite = yas_wire::schema::events::EVENT_MESSAGE_WRITE as u16,
    TickStart = yas_wire::schema::events::EVENT_TICK_START as u16,
    TickStop = yas_wire::schema::events::EVENT_TICK_STOP as u16,
    TickNudge = yas_wire::schema::events::EVENT_TICK_NUDGE as u16,
    SessionLock = yas_wire::schema::events::EVENT_SESSION_LOCK as u16,
    PtyRead = yas_wire::schema::events::EVENT_PTY_READ as u16,
    PtyWrite = yas_wire::schema::events::EVENT_PTY_WRITE as u16,
    PtyParse = yas_wire::schema::events::EVENT_PTY_PARSE as u16,
    PtySnapshot = yas_wire::schema::events::EVENT_PTY_SNAPSHOT as u16,
    PtyResize = yas_wire::schema::events::EVENT_PTY_RESIZE as u16,
    PtyInput = yas_wire::schema::events::EVENT_PTY_INPUT as u16,
    CompositorEvent = yas_wire::schema::events::EVENT_COMPOSITOR_EVENT as u16,
    CompositorCommand = yas_wire::schema::events::EVENT_COMPOSITOR_COMMAND as u16,
    SurfaceEncode = yas_wire::schema::events::EVENT_SURFACE_ENCODE as u16,
    SurfaceFrame = yas_wire::schema::events::EVENT_SURFACE_FRAME as u16,
    AudioFrame = yas_wire::schema::events::EVENT_AUDIO_FRAME as u16,
    FsRequest = yas_wire::schema::events::EVENT_FS_REQUEST as u16,
    GitRequest = yas_wire::schema::events::EVENT_GIT_REQUEST as u16,
    LspRequest = yas_wire::schema::events::EVENT_LSP_REQUEST as u16,
    KvRequest = yas_wire::schema::events::EVENT_KV_REQUEST as u16,
    NetRequest = yas_wire::schema::events::EVENT_NET_REQUEST as u16,
    ProcessRequest = yas_wire::schema::events::EVENT_PROCESS_REQUEST as u16,
    ExtensionRequest = yas_wire::schema::events::EVENT_EXTENSION_REQUEST as u16,
    ChannelRequest = yas_wire::schema::events::EVENT_CHANNEL_REQUEST as u16,
    ClientControl = yas_wire::schema::events::EVENT_CLIENT_CONTROL as u16,
    OutboxQueue = yas_wire::schema::events::EVENT_OUTBOX_QUEUE as u16,
    Supervisor = yas_wire::schema::events::EVENT_SUPERVISOR as u16,
    ConnectionAccept = yas_wire::schema::events::EVENT_CONNECTION_ACCEPT as u16,
    Error = yas_wire::schema::events::EVENT_SERVER_ERROR as u16,
}

impl EventType {
    pub(crate) const fn id(self) -> u16 {
        self as u16
    }

    fn from_name(name: &str) -> Option<Self> {
        EVENT_TYPE_CATALOG
            .iter()
            .find_map(|&(kind, candidate)| (candidate == name).then_some(kind))
    }
}

const EVENT_TYPE_CATALOG: &[(EventType, &str)] = &[
    (EventType::ServerStart, "server.start"),
    (EventType::ServerStop, "server.stop"),
    (EventType::TaskStart, "task.start"),
    (EventType::TaskStop, "task.stop"),
    (EventType::ClientConnect, "client.connect"),
    (EventType::ClientDisconnect, "client.disconnect"),
    (EventType::ClientReject, "client.reject"),
    (EventType::ConfigChange, "config.change"),
    (EventType::StreamStart, "stream.start"),
    (EventType::StreamStop, "stream.stop"),
    (EventType::ProtocolError, "protocol.error"),
    (EventType::PtyCreate, "pty.create"),
    (EventType::PtyExit, "pty.exit"),
    (EventType::PtyRemove, "pty.remove"),
    (EventType::Deadline, "pty.deadline"),
    (EventType::Capacity, "server.capacity"),
    (EventType::FrameRead, "frame.read"),
    (EventType::FrameWrite, "frame.write"),
    (EventType::MessageRead, "message.read"),
    (EventType::MessageWrite, "message.write"),
    (EventType::TickStart, "tick.start"),
    (EventType::TickStop, "tick.stop"),
    (EventType::TickNudge, "tick.nudge"),
    (EventType::SessionLock, "session.lock"),
    (EventType::PtyRead, "pty.read"),
    (EventType::PtyWrite, "pty.write"),
    (EventType::PtyParse, "pty.parse"),
    (EventType::PtySnapshot, "pty.snapshot"),
    (EventType::PtyResize, "pty.resize"),
    (EventType::PtyInput, "pty.input"),
    (EventType::CompositorEvent, "compositor.event"),
    (EventType::CompositorCommand, "compositor.command"),
    (EventType::SurfaceEncode, "surface.encode"),
    (EventType::SurfaceFrame, "surface.frame"),
    (EventType::AudioFrame, "audio.frame"),
    (EventType::FsRequest, "fs.request"),
    (EventType::GitRequest, "git.request"),
    (EventType::LspRequest, "lsp.request"),
    (EventType::KvRequest, "kv.request"),
    (EventType::NetRequest, "net.request"),
    (EventType::ProcessRequest, "process.request"),
    (EventType::ExtensionRequest, "extension.request"),
    (EventType::ChannelRequest, "channel.request"),
    (EventType::ClientControl, "client.control"),
    (EventType::OutboxQueue, "outbox.queue"),
    (EventType::Supervisor, "supervisor.event"),
    (EventType::ConnectionAccept, "connection.accept"),
    (EventType::Error, "server.error"),
];

fn activation_enabled(set: ActivationSet, kind: EventType) -> bool {
    set.enabled(kind.id())
}

fn activation_bytes(set: ActivationSet) -> [u8; ACTIVATION_WORDS * 8] {
    let mut bytes = [0; ACTIVATION_WORDS * 8];
    for (index, word) in set.0.into_iter().enumerate() {
        bytes[index * 8..index * 8 + 8].copy_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn parse_activation_spec(spec: &str) -> Result<ActivationSet, String> {
    let first = spec.split(',').map(str::trim).find(|part| !part.is_empty());
    let mut set = if first.is_some_and(|part| part.starts_with(['+', '-'])) {
        ActivationSet::low_throughput()
    } else {
        ActivationSet::default()
    };
    for raw in spec
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let (enabled, selector) = match raw.as_bytes().first() {
            Some(b'+') => (true, &raw[1..]),
            Some(b'-') => (false, &raw[1..]),
            _ => (true, raw),
        };
        match selector {
            "all" => {
                set = if enabled {
                    ActivationSet::all()
                } else {
                    ActivationSet::default()
                };
            }
            "none" if enabled => set = ActivationSet::default(),
            "none" => {}
            "default" if enabled => set = ActivationSet::low_throughput(),
            "default" => {
                for &(kind, _) in EVENT_TYPE_CATALOG {
                    if activation_enabled(ActivationSet::low_throughput(), kind) {
                        set.set(kind.id(), false);
                    }
                }
            }
            _ => {
                let mut matched = false;
                if let Some(prefix) = selector.strip_suffix(".*") {
                    for &(kind, name) in EVENT_TYPE_CATALOG {
                        if name
                            .strip_prefix(prefix)
                            .is_some_and(|tail| tail.starts_with('.'))
                        {
                            set.set(kind.id(), enabled);
                            matched = true;
                        }
                    }
                } else if let Some(kind) = EventType::from_name(selector) {
                    set.set(kind.id(), enabled);
                    matched = true;
                }
                if !matched {
                    return Err(format!("unknown event selector {selector:?}"));
                }
            }
        }
    }
    Ok(set)
}

#[derive(Clone, Debug)]
pub(crate) struct StartupFile {
    pub path: String,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EventStats {
    pub revision: u64,
    pub capacity: usize,
    pub used: usize,
    pub records: u64,
    pub dropped: u64,
    pub next_sequence: u64,
}

struct Ring {
    bytes: Box<[u8]>,
    head: usize,
    used: usize,
    records: u64,
    dropped: u64,
}

impl Ring {
    fn new(capacity: usize) -> Self {
        Self {
            bytes: vec![0; capacity].into_boxed_slice(),
            head: 0,
            used: 0,
            records: 0,
            dropped: 0,
        }
    }

    fn capacity(&self) -> usize {
        self.bytes.len()
    }

    fn copy_out(&self, at: usize, len: usize, out: &mut Vec<u8>) {
        let first = len.min(self.capacity() - at);
        out.extend_from_slice(&self.bytes[at..at + first]);
        if first < len {
            out.extend_from_slice(&self.bytes[..len - first]);
        }
    }

    fn bytes_at(&self, at: usize, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        self.copy_out(at, len, &mut out);
        out
    }

    fn read_u32(&self, at: usize) -> u32 {
        let mut bytes = [0; 4];
        let first = bytes.len().min(self.capacity() - at);
        bytes[..first].copy_from_slice(&self.bytes[at..at + first]);
        if first < bytes.len() {
            let remaining = bytes.len() - first;
            bytes[first..].copy_from_slice(&self.bytes[..remaining]);
        }
        u32::from_le_bytes(bytes)
    }

    fn write_at(&mut self, at: usize, data: &[u8]) {
        let first = data.len().min(self.capacity() - at);
        self.bytes[at..at + first].copy_from_slice(&data[..first]);
        if first < data.len() {
            self.bytes[..data.len() - first].copy_from_slice(&data[first..]);
        }
    }

    fn oldest_len(&self) -> Option<usize> {
        if self.used < 4 {
            return None;
        }
        let len = self.read_u32(self.head) as usize;
        (len >= EVENT_RECORD_HEADER_LEN && len <= self.used).then_some(len)
    }

    fn evict_oldest(&mut self) {
        let Some(len) = self.oldest_len() else {
            // A corrupt in-memory prefix cannot be recovered record-by-record.
            self.head = 0;
            self.used = 0;
            self.records = 0;
            self.dropped = self.dropped.saturating_add(1);
            return;
        };
        self.head = (self.head + len) % self.capacity();
        self.used -= len;
        self.records = self.records.saturating_sub(1);
        self.dropped = self.dropped.saturating_add(1);
    }

    fn append(&mut self, record: &[u8]) -> bool {
        self.append_parts(record, &[])
    }

    fn append_parts(&mut self, header: &[u8], payload: &[u8]) -> bool {
        let Some(len) = header.len().checked_add(payload.len()) else {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        };
        if len > self.capacity() {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        while self.capacity() - self.used < len {
            self.evict_oldest();
        }
        let tail = (self.head + self.used) % self.capacity();
        self.write_at(tail, header);
        self.write_at((tail + header.len()) % self.capacity(), payload);
        self.used += len;
        self.records = self.records.saturating_add(1);
        true
    }

    fn record_vecs(&self) -> Vec<Vec<u8>> {
        let mut result = Vec::with_capacity(self.records.min(usize::MAX as u64) as usize);
        let mut at = self.head;
        let mut left = self.used;
        while left >= EVENT_RECORD_HEADER_LEN {
            let len = self.read_u32(at) as usize;
            if len < EVENT_RECORD_HEADER_LEN || len > left {
                break;
            }
            result.push(self.bytes_at(at, len));
            at = (at + len) % self.capacity();
            left -= len;
        }
        result
    }

    fn resize(&mut self, capacity: usize) {
        if capacity == self.capacity() {
            return;
        }
        let records = self.record_vecs();
        let mut replacement = Ring::new(capacity);
        replacement.dropped = self.dropped;
        for record in records {
            replacement.append(&record);
        }
        *self = replacement;
    }
}

pub(crate) struct EventLog {
    activations: [AtomicU64; ACTIVATION_WORDS],
    ring: Mutex<Ring>,
    config_revision: AtomicU64,
    next_sequence: AtomicU64,
    next_stream_id: AtomicU32,
    started: Instant,
    started_unix_ns: u64,
    live_tx: broadcast::Sender<Arc<[u8]>>,
    file_streams: Mutex<HashMap<u32, FileStreamTask>>,
}

struct FileStreamTask {
    stop: oneshot::Sender<()>,
    join: tokio::task::JoinHandle<Result<(), String>>,
    path: PathBuf,
    flags: u8,
    progress: Arc<FileStreamProgress>,
}

#[cfg(test)]
type ClientEventStream = (u32, Option<Vec<u8>>, broadcast::Receiver<Arc<[u8]>>);
type EventSubscription = (Option<Vec<u8>>, u64, broadcast::Receiver<Arc<[u8]>>);

struct FileStreamProgress {
    state: AtomicU8,
    records: AtomicU64,
    bytes: AtomicU64,
    lost: AtomicU64,
    error: Mutex<Option<String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum FileStreamState {
    Running = yas_wire::schema::events::RECORDING_RUNNING as u8,
    Stopped = yas_wire::schema::events::RECORDING_STOPPED as u8,
    Failed = yas_wire::schema::events::RECORDING_FAILED as u8,
}

impl FileStreamProgress {
    fn new(records: u64, bytes: u64) -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU8::new(FileStreamState::Running as u8),
            records: AtomicU64::new(records),
            bytes: AtomicU64::new(bytes),
            lost: AtomicU64::new(0),
            error: Mutex::new(None),
        })
    }

    fn fail(&self, error: String) -> String {
        self.state
            .store(FileStreamState::Failed as u8, Ordering::Release);
        *self.error.lock().expect("event file progress poisoned") = Some(error.clone());
        error
    }

    fn stop(&self) {
        self.state
            .store(FileStreamState::Stopped as u8, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigureError {
    InvalidSize,
    Conflict,
}

impl std::fmt::Display for ConfigureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidSize => "ring size is outside the supported range",
            Self::Conflict => "event configuration revision changed",
        })
    }
}

#[derive(Debug)]
pub(crate) enum StartFileStreamError {
    Io(String),
}

impl std::fmt::Display for StartFileStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => f.write_str(error),
        }
    }
}

pub(crate) struct FileStreamInfo {
    pub id: u32,
    pub state: FileStreamState,
    pub flags: u8,
    pub records: u64,
    pub bytes: u64,
    pub lost: u64,
    #[cfg(test)]
    pub path: String,
    pub path_bytes: Vec<u8>,
    pub error: String,
}

impl EventLog {
    pub(crate) fn from_env() -> (Arc<Self>, Option<StartupFile>) {
        let size = std::env::var("YAS_EVENTS_SIZE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|size| (MIN_RING_SIZE..=MAX_RING_SIZE).contains(size))
            .unwrap_or(DEFAULT_RING_SIZE);
        let activations = match std::env::var("YAS_EVENTS") {
            Ok(spec) => match parse_activation_spec(&spec) {
                Ok(set) => set,
                Err(error) => {
                    eprintln!("yas-server: ignoring invalid YAS_EVENTS: {error}");
                    ActivationSet::low_throughput()
                }
            },
            Err(_) => ActivationSet::low_throughput(),
        };
        let file = std::env::var("YAS_EVENTS_FILE")
            .ok()
            .filter(|path| !path.is_empty())
            .map(|path| {
                let mut flags = 0;
                if !std::env::var("YAS_EVENTS_FILE_HISTORY").is_ok_and(|value| value == "0") {
                    flags |= EVENTS_STREAM_HISTORY;
                }
                if std::env::var("YAS_EVENTS_FILE_APPEND").is_ok_and(|value| value == "1") {
                    flags |= EVENTS_STREAM_APPEND;
                }
                StartupFile { path, flags }
            });
        let (live_tx, _) = broadcast::channel(LIVE_CHANNEL_RECORDS);
        let started_unix_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        let log = Arc::new(Self {
            activations: std::array::from_fn(|index| AtomicU64::new(activations.0[index])),
            ring: Mutex::new(Ring::new(size)),
            config_revision: AtomicU64::new(1),
            next_sequence: AtomicU64::new(0),
            next_stream_id: AtomicU32::new(1),
            started: Instant::now(),
            started_unix_ns,
            live_tx,
            file_streams: Mutex::new(HashMap::new()),
        });
        (log, file)
    }

    #[cfg(test)]
    pub(crate) fn new(size: usize, activations: ActivationSet) -> Arc<Self> {
        let (live_tx, _) = broadcast::channel(16);
        Arc::new(Self {
            activations: std::array::from_fn(|index| AtomicU64::new(activations.0[index])),
            ring: Mutex::new(Ring::new(size)),
            config_revision: AtomicU64::new(1),
            next_sequence: AtomicU64::new(0),
            next_stream_id: AtomicU32::new(1),
            started: Instant::now(),
            started_unix_ns: 0,
            live_tx,
            file_streams: Mutex::new(HashMap::new()),
        })
    }

    #[inline]
    pub(crate) fn enabled(&self, kind: EventType) -> bool {
        let id = usize::from(kind.id());
        self.activations[id / 64].load(Ordering::Relaxed) & (1u64 << (id % 64)) != 0
    }

    pub(crate) fn activations(&self) -> ActivationSet {
        ActivationSet(std::array::from_fn(|index| {
            self.activations[index].load(Ordering::Acquire)
        }))
    }

    pub(crate) fn configure(
        &self,
        size: usize,
        activations: ActivationSet,
        expected_revision: Option<u64>,
    ) -> Result<EventStats, ConfigureError> {
        if !(MIN_RING_SIZE..=MAX_RING_SIZE).contains(&size) {
            return Err(ConfigureError::InvalidSize);
        }
        let mut ring = self.ring.lock().expect("event ring poisoned");
        let revision = self.config_revision.load(Ordering::Acquire);
        if expected_revision.is_some_and(|expected| expected != revision) {
            return Err(ConfigureError::Conflict);
        }
        ring.resize(size);
        for (word, value) in self.activations.iter().zip(activations.0) {
            word.store(value, Ordering::Release);
        }
        let revision = if revision == u64::MAX {
            1
        } else {
            revision + 1
        };
        self.config_revision.store(revision, Ordering::Release);
        Ok(self.stats_locked(&ring, revision))
    }

    #[cfg(test)]
    pub(crate) fn stats(&self) -> EventStats {
        let ring = self.ring.lock().expect("event ring poisoned");
        let revision = self.config_revision.load(Ordering::Acquire);
        self.stats_locked(&ring, revision)
    }

    fn stats_locked(&self, ring: &Ring, revision: u64) -> EventStats {
        EventStats {
            revision,
            capacity: ring.capacity(),
            used: ring.used,
            records: ring.records,
            dropped: ring.dropped,
            next_sequence: self.next_sequence.load(Ordering::Acquire),
        }
    }

    pub(crate) fn configuration(&self) -> (EventStats, ActivationSet) {
        let ring = self.ring.lock().expect("event ring poisoned");
        let revision = self.config_revision.load(Ordering::Acquire);
        (self.stats_locked(&ring, revision), self.activations())
    }

    pub(crate) fn record(&self, kind: EventType, flags: u16, payload: &[u8]) {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let monotonic_ns = self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let unix_ns = self.started_unix_ns.saturating_add(monotonic_ns);
        let max_payload = MAX_RING_SIZE
            .saturating_sub(EVENT_RECORD_HEADER_LEN)
            .saturating_sub(9);
        let payload = &payload[..payload.len().min(max_payload)];
        let len = EVENT_RECORD_HEADER_LEN + payload.len();
        let mut header = [0; EVENT_RECORD_HEADER_LEN];
        header[0..4].copy_from_slice(&(len as u32).to_le_bytes());
        header[4..6].copy_from_slice(&kind.id().to_le_bytes());
        header[6..8].copy_from_slice(&flags.to_le_bytes());
        header[8..16].copy_from_slice(&sequence.to_le_bytes());
        header[16..24].copy_from_slice(&monotonic_ns.to_le_bytes());
        header[24..32].copy_from_slice(&unix_ns.to_le_bytes());
        let mut ring = self.ring.lock().expect("event ring poisoned");
        ring.append_parts(&header, payload);
        // Live streams still see a record that is larger than the configured
        // ring. The ring's dropped counter reports that it was not retained.
        if self.live_tx.receiver_count() != 0 {
            let mut record = Vec::with_capacity(len);
            record.extend_from_slice(&header);
            record.extend_from_slice(payload);
            let _ = self.live_tx.send(record.into());
        }
    }

    fn dump_locked(&self, ring: &Ring) -> Vec<u8> {
        let activations = self.activations();
        let mut dump = Vec::with_capacity(EVENT_DUMP_HEADER_LEN + ring.used);
        dump.extend_from_slice(EVENT_DUMP_MAGIC);
        dump.extend_from_slice(&(EVENT_DUMP_HEADER_LEN as u16).to_le_bytes());
        dump.extend_from_slice(&yas_wire::events::VERSION.to_le_bytes());
        dump.extend_from_slice(&(ring.capacity() as u64).to_le_bytes());
        dump.extend_from_slice(&(ring.used as u64).to_le_bytes());
        dump.extend_from_slice(&ring.records.to_le_bytes());
        dump.extend_from_slice(&ring.dropped.to_le_bytes());
        dump.extend_from_slice(&self.next_sequence.load(Ordering::Acquire).to_le_bytes());
        dump.extend_from_slice(&activation_bytes(activations));
        debug_assert_eq!(dump.len(), EVENT_DUMP_HEADER_LEN);
        for record in ring.record_vecs() {
            dump.extend_from_slice(&record);
        }
        dump
    }

    pub(crate) fn dump(&self) -> Vec<u8> {
        let ring = self.ring.lock().expect("event ring poisoned");
        self.dump_locked(&ring)
    }

    fn empty_dump_locked(&self, ring: &Ring) -> Vec<u8> {
        let mut dump = Vec::with_capacity(EVENT_DUMP_HEADER_LEN);
        dump.extend_from_slice(EVENT_DUMP_MAGIC);
        dump.extend_from_slice(&(EVENT_DUMP_HEADER_LEN as u16).to_le_bytes());
        dump.extend_from_slice(&yas_wire::events::VERSION.to_le_bytes());
        dump.extend_from_slice(&(ring.capacity() as u64).to_le_bytes());
        dump.extend_from_slice(&0u64.to_le_bytes());
        dump.extend_from_slice(&0u64.to_le_bytes());
        dump.extend_from_slice(&ring.dropped.to_le_bytes());
        dump.extend_from_slice(&self.next_sequence.load(Ordering::Acquire).to_le_bytes());
        dump.extend_from_slice(&activation_bytes(self.activations()));
        debug_assert_eq!(dump.len(), EVENT_DUMP_HEADER_LEN);
        dump
    }

    pub(crate) fn snapshot_and_subscribe(
        &self,
        history: bool,
        empty_header: bool,
    ) -> EventSubscription {
        let ring = self.ring.lock().expect("event ring poisoned");
        let receiver = self.live_tx.subscribe();
        let records = if history { ring.records } else { 0 };
        let dump = if history {
            Some(self.dump_locked(&ring))
        } else if empty_header {
            Some(self.empty_dump_locked(&ring))
        } else {
            None
        };
        (dump, records, receiver)
    }

    /// Atomically captures retained history and subscribes to everything
    /// appended after that snapshot. The sequence cursor is sampled while the
    /// ring lock is held so native YAS delivery has no snapshot/live race.
    pub(crate) fn native_snapshot_and_subscribe(&self, history: bool) -> EventSubscription {
        let ring = self.ring.lock().expect("event ring poisoned");
        let receiver = self.live_tx.subscribe();
        let next_sequence = self.next_sequence.load(Ordering::Acquire);
        let dump = history.then(|| self.dump_locked(&ring));
        (dump, next_sequence, receiver)
    }

    fn allocate_stream_id(&self) -> u32 {
        loop {
            let id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
            if id != 0 {
                return id;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn client_stream(&self, history: bool) -> ClientEventStream {
        let id = self.allocate_stream_id();
        let (dump, _, receiver) = self.snapshot_and_subscribe(history, true);
        (id, dump, receiver)
    }

    pub(crate) async fn start_file_stream<P: AsRef<Path> + ?Sized>(
        self: &Arc<Self>,
        path: &P,
        flags: u8,
    ) -> Result<u32, StartFileStreamError> {
        let path = path.as_ref();
        let append = flags & EVENTS_STREAM_APPEND != 0;
        let history = flags & EVENTS_STREAM_HISTORY != 0;
        let mut options = tokio::fs::OpenOptions::new();
        options
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append);
        let mut file = options.open(path).await.map_err(|error| {
            StartFileStreamError::Io(format!("cannot open {}: {error}", path.display()))
        })?;
        let id = self.allocate_stream_id();
        // Every recording invocation starts a self-describing segment, even
        // when appending and/or starting from now. This is also the initial
        // write whose flush gates the successful START response.
        let (dump, history_records, mut receiver) = self.snapshot_and_subscribe(history, true);
        let initial_bytes = dump.as_ref().map_or(0, Vec::len) as u64;
        if let Some(dump) = dump {
            file.write_all(&dump).await.map_err(|error| {
                StartFileStreamError::Io(format!("cannot initialize {}: {error}", path.display()))
            })?;
            file.flush().await.map_err(|error| {
                StartFileStreamError::Io(format!(
                    "cannot flush initial data to {}: {error}",
                    path.display()
                ))
            })?;
        }
        let progress = FileStreamProgress::new(history_records, initial_bytes);
        let task_progress = progress.clone();
        let task_path = path.to_owned();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let result = async {
                loop {
                    let next = tokio::select! {
                        biased;
                        _ = &mut stopped => break,
                        next = receiver.recv() => next,
                    };
                    match next {
                        Ok(record) => {
                            write_stream_bytes(&mut file, &record, 0, &task_progress, &task_path)
                                .await?;
                        }
                        Err(broadcast::error::RecvError::Lagged(lost)) => {
                            let record = gap_record(lost);
                            write_stream_bytes(
                                &mut file,
                                &record,
                                lost,
                                &task_progress,
                                &task_path,
                            )
                            .await?;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                loop {
                    match receiver.try_recv() {
                        Ok(record) => {
                            write_stream_bytes(&mut file, &record, 0, &task_progress, &task_path)
                                .await?;
                        }
                        Err(broadcast::error::TryRecvError::Lagged(lost)) => {
                            write_stream_bytes(
                                &mut file,
                                &gap_record(lost),
                                lost,
                                &task_progress,
                                &task_path,
                            )
                            .await?;
                        }
                        Err(broadcast::error::TryRecvError::Empty)
                        | Err(broadcast::error::TryRecvError::Closed) => break,
                    }
                }
                file.flush()
                    .await
                    .map_err(|error| format!("cannot flush {}: {error}", task_path.display()))?;
                Ok(())
            }
            .await;
            match result {
                Ok(()) => task_progress.stop(),
                Err(error) => {
                    return Err(task_progress.fail(error));
                }
            }
            Ok(())
        });
        self.file_streams
            .lock()
            .expect("event file streams poisoned")
            .insert(
                id,
                FileStreamTask {
                    stop,
                    join: task,
                    path: path.to_owned(),
                    flags,
                    progress,
                },
            );
        Ok(id)
    }

    pub(crate) fn file_streams(&self) -> Vec<FileStreamInfo> {
        let streams = self
            .file_streams
            .lock()
            .expect("event file streams poisoned");
        let mut result = streams
            .iter()
            .map(|(&id, task)| file_stream_info(id, task.flags, &task.path, &task.progress))
            .collect::<Vec<_>>();
        result.sort_unstable_by_key(|stream| stream.id);
        result
    }

    pub(crate) async fn stop_file_stream_with_info(
        &self,
        id: u32,
    ) -> Result<Option<FileStreamInfo>, String> {
        let task = self
            .file_streams
            .lock()
            .expect("event file streams poisoned")
            .remove(&id);
        if let Some(task) = task {
            let FileStreamTask {
                stop,
                join,
                path,
                flags,
                progress,
            } = task;
            let _ = stop.send(());
            match join.await {
                Ok(Ok(())) => Ok(Some(file_stream_info(id, flags, &path, &progress))),
                Ok(Err(error)) => Err(error),
                Err(error) => Err(format!("event recording task failed: {error}")),
            }
        } else {
            Ok(None)
        }
    }

    #[cfg(test)]
    pub(crate) async fn stop_file_stream(&self, id: u32) -> Result<bool, String> {
        self.stop_file_stream_with_info(id)
            .await
            .map(|info| info.is_some())
    }

    pub(crate) async fn shutdown_file_streams(&self) {
        let tasks: Vec<_> = self
            .file_streams
            .lock()
            .expect("event file streams poisoned")
            .drain()
            .map(|(_, task)| task)
            .collect();
        let mut joins = Vec::with_capacity(tasks.len());
        for task in tasks {
            let _ = task.stop.send(());
            joins.push(task.join);
        }
        for join in joins {
            let _ = join.await;
        }
    }
}

async fn write_stream_bytes(
    file: &mut tokio::fs::File,
    record: &[u8],
    lost: u64,
    progress: &FileStreamProgress,
    path: &Path,
) -> Result<(), String> {
    file.write_all(record)
        .await
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    progress.records.fetch_add(1, Ordering::Relaxed);
    progress
        .bytes
        .fetch_add(record.len() as u64, Ordering::Relaxed);
    progress.lost.fetch_add(lost, Ordering::Relaxed);
    Ok(())
}

fn file_stream_info(
    id: u32,
    flags: u8,
    path: &Path,
    progress: &FileStreamProgress,
) -> FileStreamInfo {
    FileStreamInfo {
        id,
        state: match progress.state.load(Ordering::Acquire) {
            value if value == FileStreamState::Running as u8 => FileStreamState::Running,
            value if value == FileStreamState::Stopped as u8 => FileStreamState::Stopped,
            _ => FileStreamState::Failed,
        },
        flags,
        records: progress.records.load(Ordering::Acquire),
        bytes: progress.bytes.load(Ordering::Acquire),
        lost: progress.lost.load(Ordering::Acquire),
        #[cfg(test)]
        path: path.to_string_lossy().into_owned(),
        path_bytes: path_bytes(path),
        error: progress
            .error
            .lock()
            .expect("event file progress poisoned")
            .clone()
            .unwrap_or_default(),
    }
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

fn gap_record(lost: u64) -> Vec<u8> {
    let len = EVENT_RECORD_HEADER_LEN + 8;
    let mut record = Vec::with_capacity(len);
    record.extend_from_slice(&(len as u32).to_le_bytes());
    record.extend_from_slice(&EVENT_TYPE_STREAM_GAP.to_le_bytes());
    record.extend_from_slice(&0u16.to_le_bytes());
    record.extend_from_slice(&0u64.to_le_bytes());
    record.extend_from_slice(&0u64.to_le_bytes());
    record.extend_from_slice(&0u64.to_le_bytes());
    record.extend_from_slice(&lost.to_le_bytes());
    record
}

pub(crate) fn payload_name(name: &str) -> Vec<u8> {
    let bytes = name.as_bytes();
    let len = bytes.len().min(u16::MAX as usize);
    let mut payload = Vec::with_capacity(2 + len);
    payload.extend_from_slice(&(len as u16).to_le_bytes());
    payload.extend_from_slice(&bytes[..len]);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_wrap_preserves_complete_newest_records() {
        let log = EventLog::new(160, ActivationSet::all());
        for value in 0..8u8 {
            log.record(EventType::Error, 0, &[value; 8]);
        }
        let dump = log.dump();
        assert_eq!(&dump[..8], EVENT_DUMP_MAGIC);
        let records = &dump[EVENT_DUMP_HEADER_LEN..];
        assert_eq!(records.len() % (EVENT_RECORD_HEADER_LEN + 8), 0);
        assert!(records.len() <= 160);
        assert_eq!(records.last().copied(), Some(7));
        assert!(log.stats().dropped > 0);
    }

    #[test]
    fn disabled_event_building_can_be_guarded() {
        let log = EventLog::new(4096, ActivationSet::default());
        assert!(!log.enabled(EventType::FrameRead));
        assert_eq!(log.stats().records, 0);
    }

    #[test]
    fn oversized_records_are_live_even_when_not_retained() {
        let log = EventLog::new(64, ActivationSet::all());
        let (_, _, mut receiver) = log.client_stream(false);
        log.record(EventType::Error, 0, &[7; 64]);
        let record = receiver.try_recv().unwrap();
        assert_eq!(record.last().copied(), Some(7));
        assert_eq!(log.stats().records, 0);
        assert_eq!(log.stats().dropped, 1);
    }

    #[test]
    fn shrinking_keeps_the_newest_records() {
        let log = EventLog::new(MIN_RING_SIZE * 2, ActivationSet::all());
        for value in 0..100u8 {
            log.record(EventType::Error, 0, &[value; 64]);
        }
        log.configure(MIN_RING_SIZE, ActivationSet::all(), None)
            .unwrap();
        let dump = log.dump();
        assert_eq!(dump.last().copied(), Some(99));
        assert!(log.stats().used <= MIN_RING_SIZE);
    }

    #[test]
    fn configuration_revision_rejects_a_stale_replace() {
        let log = EventLog::new(MIN_RING_SIZE, ActivationSet::low_throughput());
        assert_eq!(log.stats().revision, 1);
        let changed = log
            .configure(MIN_RING_SIZE * 2, ActivationSet::all(), Some(1))
            .unwrap();
        assert_eq!(changed.revision, 2);
        assert!(matches!(
            log.configure(MIN_RING_SIZE, ActivationSet::default(), Some(1)),
            Err(ConfigureError::Conflict)
        ));
        let (current, activations) = log.configuration();
        assert_eq!(current.revision, 2);
        assert_eq!(current.capacity, MIN_RING_SIZE * 2);
        assert_eq!(activations, ActivationSet::all());
    }

    #[tokio::test]
    async fn file_stream_flushes_history_and_live_records() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("yas-events-{}-{unique}.bin", std::process::id()));
        let path_text = path.to_str().unwrap();
        let log = EventLog::new(MIN_RING_SIZE, ActivationSet::all());
        log.record(EventType::ServerStart, 0, &[1]);
        let stream_id = log
            .start_file_stream(path_text, EVENTS_STREAM_HISTORY)
            .await
            .unwrap();
        let initialized = tokio::fs::read(&path).await.unwrap();
        assert_eq!(&initialized[..EVENT_DUMP_MAGIC.len()], EVENT_DUMP_MAGIC);
        assert_eq!(initialized.last().copied(), Some(1));
        let streams = log.file_streams();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, stream_id);
        assert_eq!(streams[0].state, FileStreamState::Running);
        assert_eq!(streams[0].flags, EVENTS_STREAM_HISTORY);
        assert_eq!(streams[0].path, path_text);
        log.record(EventType::Error, 0, &[9]);
        assert!(log.stop_file_stream(stream_id).await.unwrap());
        assert!(log.file_streams().is_empty());

        let bytes = tokio::fs::read(&path).await.unwrap();
        tokio::fs::remove_file(&path).await.unwrap();
        assert_eq!(&bytes[..EVENT_DUMP_MAGIC.len()], EVENT_DUMP_MAGIC);
        assert_eq!(bytes.last().copied(), Some(9));
    }

    #[tokio::test]
    async fn from_now_file_stream_flushes_its_header_before_start_returns() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "yas-events-now-{}-{unique}.bin",
            std::process::id()
        ));
        let path_text = path.to_str().unwrap();
        let log = EventLog::new(MIN_RING_SIZE, ActivationSet::all());

        let stream_id = log.start_file_stream(path_text, 0).await.unwrap();
        let initialized = tokio::fs::read(&path).await.unwrap();
        assert_eq!(initialized.len(), EVENT_DUMP_HEADER_LEN);
        assert_eq!(&initialized[..EVENT_DUMP_MAGIC.len()], EVENT_DUMP_MAGIC);
        assert!(log.stop_file_stream(stream_id).await.unwrap());

        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn file_stream_status_and_stop_preserve_write_failure() {
        let log = EventLog::new(MIN_RING_SIZE, ActivationSet::all());
        let (stop, stopped) = oneshot::channel();
        drop(stopped);
        let progress = FileStreamProgress::new(7, 99);
        progress.lost.store(2, Ordering::Relaxed);
        progress.fail("disk full".into());
        let join = tokio::spawn(async { Err::<(), String>("disk full".into()) });
        log.file_streams.lock().unwrap().insert(
            42,
            FileStreamTask {
                stop,
                join,
                path: "/tmp/failed.bin".into(),
                flags: EVENTS_STREAM_HISTORY,
                progress,
            },
        );

        let streams = log.file_streams();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].state, FileStreamState::Failed);
        assert_eq!(streams[0].records, 7);
        assert_eq!(streams[0].bytes, 99);
        assert_eq!(streams[0].lost, 2);
        assert_eq!(streams[0].error, "disk full");
        assert_eq!(log.stop_file_stream(42).await, Err("disk full".into()));
        assert!(log.file_streams().is_empty());
    }

    #[tokio::test]
    async fn file_streams_have_no_process_wide_admission_cap() {
        let log = EventLog::new(MIN_RING_SIZE, ActivationSet::all());
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut streams = Vec::new();
        for index in 0..9 {
            let path = std::env::temp_dir().join(format!(
                "yas-events-uncapped-{}-{unique}-{index}.bin",
                std::process::id()
            ));
            let id = log
                .start_file_stream(path.to_str().unwrap(), 0)
                .await
                .unwrap();
            streams.push((id, path));
        }
        assert_eq!(log.file_streams().len(), 9);
        for (id, path) in streams {
            assert!(log.stop_file_stream(id).await.unwrap());
            tokio::fs::remove_file(path).await.unwrap();
        }
    }
}
