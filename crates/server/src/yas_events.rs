//! Semantic adapter between the process-wide event journal and YAS Events.
//!
//! Frame correlation, operation replay, Transfer IDs and session ownership
//! stay in `yas`; this module only exposes process-owned journal operations and
//! a loss-reporting live record source.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use tokio::sync::broadcast;
use yas_wire::codec::{Extension, Extensions};
use yas_wire::events::{
    self as wire, Config, EventBatch, EventRecord, RecordingInfo, RecordingList, RecordingState,
    SetConfig, StartRecording, StartStream,
};

use super::events::{
    self, EVENT_DUMP_HEADER_LEN, EVENT_DUMP_MAGIC, EVENT_RECORD_HEADER_LEN, EVENTS_STREAM_APPEND,
    EVENTS_STREAM_HISTORY, EventLog, FileStreamInfo, FileStreamState,
};

const PACKED_RECORD_HEADER_BYTES: usize = 28;
const PACKED_BATCH_HEADER_BYTES: usize = 12;

#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestOperationGate {
    entered: AtomicUsize,
    releases: Mutex<usize>,
    released: Condvar,
    changed: tokio::sync::Notify,
}

#[cfg(test)]
impl TestOperationGate {
    fn wait_blocking(&self) {
        self.entered.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_waiters();
        let mut releases = self.releases.lock().unwrap();
        while *releases == 0 {
            let (next, wait) = self
                .released
                .wait_timeout(releases, std::time::Duration::from_secs(10))
                .unwrap();
            releases = next;
            if wait.timed_out() {
                return;
            }
        }
        *releases -= 1;
    }

    pub(crate) async fn wait_for_entered(&self, expected: usize) {
        loop {
            let changed = self.changed.notified();
            if self.entered.load(Ordering::Acquire) >= expected {
                return;
            }
            changed.await;
        }
    }

    pub(crate) fn release(&self, count: usize) {
        let mut releases = self.releases.lock().unwrap();
        *releases = releases.saturating_add(count);
        self.released.notify_all();
    }
}

#[derive(Clone)]
pub(crate) struct Runtime {
    log: Arc<EventLog>,
    #[cfg(test)]
    operation_gate: Option<Arc<TestOperationGate>>,
}

#[derive(Clone, Debug)]
pub(crate) struct DumpSnapshot {
    pub bytes: Arc<[u8]>,
    pub content_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Conflict,
    Invalid(&'static str),
    NotFound,
    Io(String),
    CorruptJournal(&'static str),
    TaskFailed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => formatter.write_str("event configuration revision changed"),
            Self::Invalid(detail) => formatter.write_str(detail),
            Self::NotFound => formatter.write_str("event resource not found"),
            Self::Io(detail) => formatter.write_str(detail),
            Self::CorruptJournal(detail) => write!(formatter, "corrupt event journal: {detail}"),
            Self::TaskFailed => formatter.write_str("event journal task failed"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StreamItem {
    Records(EventBatch),
    Gap {
        lost: u64,
        first_available_sequence: u64,
    },
}

pub(crate) struct LiveStream {
    first_sequence: u64,
    max_batch_bytes: usize,
    expected_sequence: u64,
    history: VecDeque<EventRecord>,
    receiver: broadcast::Receiver<Arc<[u8]>>,
    pending_record: Option<EventRecord>,
    pending_gap: Option<(u64, u64)>,
}

impl Runtime {
    pub(crate) fn new(log: Arc<EventLog>) -> Self {
        Self {
            log,
            #[cfg(test)]
            operation_gate: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_operation_gate(mut self, gate: Arc<TestOperationGate>) -> Self {
        self.operation_gate = Some(gate);
        self
    }

    pub(crate) fn config(&self) -> Config {
        let (stats, activations) = self.log.configuration();
        config_from_parts(stats, activations)
    }

    pub(crate) async fn set_config(&self, request: &SetConfig) -> Result<Config, Error> {
        let capacity = usize::try_from(request.capacity)
            .map_err(|_| Error::Invalid("event ring size does not fit this host"))?;
        let expected_revision = (request.expected_revision
            != yas_wire::schema::events::REVISION_ANY)
            .then_some(request.expected_revision);
        let activations = wire::ActivationSet(request.activations.0);
        let log = self.log.clone();
        let stats = tokio::task::spawn_blocking(move || {
            log.configure(capacity, activations, expected_revision)
        })
        .await
        .map_err(|_| Error::TaskFailed)?
        .map_err(|error| match error {
            events::ConfigureError::Conflict => Error::Conflict,
            events::ConfigureError::InvalidSize => Error::Invalid("invalid event ring size"),
        })?;
        Ok(config_from_parts(stats, activations))
    }

    pub(crate) async fn dump(&self) -> Result<DumpSnapshot, Error> {
        let log = self.log.clone();
        #[cfg(test)]
        let gate = self.operation_gate.clone();
        let bytes = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(gate) = gate {
                gate.wait_blocking();
            }
            log.dump()
        })
        .await
        .map_err(|_| Error::TaskFailed)?;
        let content_hash = *blake3::hash(&bytes).as_bytes();
        Ok(DumpSnapshot {
            bytes: bytes.into(),
            content_hash,
        })
    }

    pub(crate) async fn start_stream(&self, request: &StartStream) -> Result<LiveStream, Error> {
        let history = request.history;
        let log = self.log.clone();
        #[cfg(test)]
        let gate = self.operation_gate.clone();
        let (dump, next_sequence, receiver) = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(gate) = gate {
                gate.wait_blocking();
            }
            log.native_snapshot_and_subscribe(history)
        })
        .await
        .map_err(|_| Error::TaskFailed)?;

        if request.start_sequence > next_sequence {
            return Err(Error::Invalid("event start sequence is in the future"));
        }
        let mut records = if let Some(dump) = dump {
            parse_dump_records(&dump)?
        } else {
            VecDeque::new()
        };
        let oldest = records
            .front()
            .map_or(next_sequence, |record| record.sequence);
        let requested = if request.history && request.start_sequence != 0 {
            request.start_sequence
        } else {
            oldest
        };
        while records
            .front()
            .is_some_and(|record| record.sequence < requested)
        {
            records.pop_front();
        }
        let first_sequence = records
            .front()
            .map_or(next_sequence, |record| record.sequence);
        let pending_gap =
            (requested < first_sequence).then_some((first_sequence - requested, first_sequence));
        let max_batch_bytes = if request.max_batch_bytes == 0 {
            wire::MAX_LIVE_BATCH_BYTES
        } else {
            request.max_batch_bytes as usize
        };
        Ok(LiveStream {
            first_sequence,
            max_batch_bytes,
            expected_sequence: requested,
            history: records,
            receiver,
            pending_record: None,
            pending_gap,
        })
    }

    pub(crate) async fn start_recording(
        &self,
        request: &StartRecording,
    ) -> Result<RecordingInfo, Error> {
        let path = native_path(&request.path)?;
        let mut flags = 0;
        if request.history {
            flags |= EVENTS_STREAM_HISTORY;
        }
        if request.append {
            flags |= EVENTS_STREAM_APPEND;
        }
        let id = self
            .log
            .start_file_stream(&path, flags)
            .await
            .map_err(|error| Error::Io(error.to_string()))?;
        self.recording(id)
            .ok_or(Error::CorruptJournal("new recording disappeared"))
    }

    pub(crate) async fn stop_recording(
        &self,
        recording_handle: u64,
    ) -> Result<RecordingInfo, Error> {
        let id = u32::try_from(recording_handle).map_err(|_| Error::NotFound)?;
        let info = self
            .log
            .stop_file_stream_with_info(id)
            .await
            .map_err(Error::Io)?
            .ok_or(Error::NotFound)?;
        Ok(recording_info(info))
    }

    pub(crate) fn recordings(&self) -> RecordingList {
        RecordingList {
            recordings: self
                .log
                .file_streams()
                .into_iter()
                .map(recording_info)
                .collect(),
        }
    }

    pub(crate) fn limits() -> Extensions {
        Extensions(vec![
            extension_u64(
                yas_wire::schema::events::LIMIT_MIN_RING_BYTES,
                events::MIN_RING_SIZE as u64,
            ),
            extension_u64(
                yas_wire::schema::events::LIMIT_MAX_RING_BYTES,
                events::MAX_RING_SIZE as u64,
            ),
            extension_u32(
                yas_wire::schema::events::LIMIT_MAX_STREAMS_PER_SESSION,
                yas_wire::schema::events::MAX_STREAMS_PER_SESSION as u32,
            ),
            extension_u32(
                yas_wire::schema::events::LIMIT_MAX_RECORDINGS,
                yas_wire::schema::events::MAX_RECORDINGS as u32,
            ),
            extension_u32(
                yas_wire::schema::events::LIMIT_MAX_RECORDING_PATH_BYTES,
                yas_wire::schema::events::MAX_RECORDING_PATH_BYTES as u32,
            ),
            extension_u32(
                yas_wire::schema::events::LIMIT_MAX_LIVE_BATCH_BYTES,
                yas_wire::schema::events::MAX_LIVE_BATCH_BYTES as u32,
            ),
            extension_u32(
                yas_wire::schema::events::LIMIT_MAX_PENDING_DUMPS,
                yas_wire::schema::events::MAX_PENDING_DUMPS as u32,
            ),
            extension_u32(
                yas_wire::schema::events::LIMIT_MAX_MUTATION_REPLAYS,
                super::yas::MAX_EVENTS_OPERATION_REPLAYS as u32,
            ),
        ])
    }

    fn recording(&self, id: u32) -> Option<RecordingInfo> {
        self.log
            .file_streams()
            .into_iter()
            .find(|info| info.id == id)
            .map(recording_info)
    }
}

impl LiveStream {
    pub(crate) fn first_sequence(&self) -> u64 {
        self.first_sequence
    }

    pub(crate) fn max_batch_bytes(&self) -> u32 {
        self.max_batch_bytes as u32
    }

    pub(crate) async fn next(&mut self) -> Result<Option<StreamItem>, Error> {
        if let Some((lost, first_available_sequence)) = self.pending_gap.take() {
            self.expected_sequence = first_available_sequence;
            return Ok(Some(StreamItem::Gap {
                lost,
                first_available_sequence,
            }));
        }

        let first = loop {
            if let Some((lost, first_available_sequence)) = self.pending_gap.take() {
                self.expected_sequence = first_available_sequence;
                return Ok(Some(StreamItem::Gap {
                    lost,
                    first_available_sequence,
                }));
            }
            match self.next_record().await? {
                Some(record) => break record,
                None if self.pending_gap.is_some() => continue,
                None => return Ok(None),
            }
        };
        if first.sequence > self.expected_sequence {
            let lost = first.sequence - self.expected_sequence;
            let first_available_sequence = first.sequence;
            self.pending_record = Some(first);
            self.expected_sequence = first_available_sequence;
            return Ok(Some(StreamItem::Gap {
                lost,
                first_available_sequence,
            }));
        }
        if first.sequence < self.expected_sequence {
            return Err(Error::CorruptJournal("event sequence moved backwards"));
        }

        let first_sequence = first.sequence;
        let mut encoded_bytes = PACKED_BATCH_HEADER_BYTES
            .saturating_add(PACKED_RECORD_HEADER_BYTES)
            .saturating_add(first.payload.len());
        self.expected_sequence = first.sequence.saturating_add(1);
        let mut records = vec![first];
        while records.len() < u16::MAX as usize {
            let Some(record) = self.try_next_record()? else {
                break;
            };
            if record.sequence != self.expected_sequence {
                self.pending_record = Some(record);
                break;
            }
            let next_bytes = PACKED_RECORD_HEADER_BYTES.saturating_add(record.payload.len());
            if encoded_bytes.saturating_add(next_bytes) > self.max_batch_bytes {
                self.pending_record = Some(record);
                break;
            }
            encoded_bytes = encoded_bytes.saturating_add(next_bytes);
            self.expected_sequence = record.sequence.saturating_add(1);
            records.push(record);
        }
        Ok(Some(StreamItem::Records(EventBatch {
            first_sequence,
            records,
        })))
    }

    async fn next_record(&mut self) -> Result<Option<EventRecord>, Error> {
        if let Some(record) = self.pending_record.take() {
            return Ok(Some(record));
        }
        if let Some(record) = self.history.pop_front() {
            return Ok(Some(record));
        }
        match self.receiver.recv().await {
            Ok(record) => parse_live_record(&record).map(Some),
            Err(broadcast::error::RecvError::Lagged(lost)) => {
                let first_available = self.expected_sequence.saturating_add(lost);
                self.pending_gap = Some((lost, first_available));
                Ok(None)
            }
            Err(broadcast::error::RecvError::Closed) => Ok(None),
        }
    }

    fn try_next_record(&mut self) -> Result<Option<EventRecord>, Error> {
        if let Some(record) = self.pending_record.take() {
            return Ok(Some(record));
        }
        if let Some(record) = self.history.pop_front() {
            return Ok(Some(record));
        }
        match self.receiver.try_recv() {
            Ok(record) => parse_live_record(&record).map(Some),
            Err(broadcast::error::TryRecvError::Lagged(lost)) => {
                let first_available = self.expected_sequence.saturating_add(lost);
                self.pending_gap = Some((lost, first_available));
                Ok(None)
            }
            Err(broadcast::error::TryRecvError::Empty)
            | Err(broadcast::error::TryRecvError::Closed) => Ok(None),
        }
    }
}

fn config_from_parts(stats: events::EventStats, activations: wire::ActivationSet) -> Config {
    Config {
        revision: stats.revision,
        capacity: stats.capacity as u64,
        used: stats.used as u64,
        record_count: stats.records,
        dropped: stats.dropped,
        next_sequence: stats.next_sequence,
        activations: wire::ActivationSet(activations.0),
        extensions: Extensions::default(),
    }
}

fn parse_dump_records(dump: &[u8]) -> Result<VecDeque<EventRecord>, Error> {
    if dump.len() < EVENT_DUMP_HEADER_LEN
        || &dump[..EVENT_DUMP_MAGIC.len()] != EVENT_DUMP_MAGIC
        || u16::from_le_bytes([dump[8], dump[9]]) as usize != EVENT_DUMP_HEADER_LEN
    {
        return Err(Error::CorruptJournal("invalid dump header"));
    }
    let mut offset = EVENT_DUMP_HEADER_LEN;
    let mut records = VecDeque::new();
    while offset < dump.len() {
        let length = read_u32(dump, offset)? as usize;
        if length < EVENT_RECORD_HEADER_LEN || length > dump.len() - offset {
            return Err(Error::CorruptJournal("invalid retained record length"));
        }
        records.push_back(parse_live_record(&dump[offset..offset + length])?);
        offset += length;
    }
    Ok(records)
}

fn parse_live_record(record: &[u8]) -> Result<EventRecord, Error> {
    if record.len() < EVENT_RECORD_HEADER_LEN {
        return Err(Error::CorruptJournal("truncated record"));
    }
    let length = read_u32(record, 0)? as usize;
    if length != record.len() {
        return Err(Error::CorruptJournal("record length mismatch"));
    }
    let event_id = u16::from_le_bytes([record[4], record[5]]);
    if event_id > yas_wire::schema::events::EVENT_SERVER_ERROR as u16 {
        return Err(Error::CorruptJournal("unknown retained event ID"));
    }
    Ok(EventRecord {
        sequence: read_u64(record, 8)?,
        monotonic_ns: read_u64(record, 16)?,
        event_id: u32::from(event_id),
        required: true,
        event_flags: u16::from_le_bytes([record[6], record[7]]),
        payload: record[EVENT_RECORD_HEADER_LEN..].to_vec(),
    })
}

fn recording_info(info: FileStreamInfo) -> RecordingInfo {
    let state = match info.state {
        FileStreamState::Running => RecordingState::Running,
        FileStreamState::Stopped => RecordingState::Stopped,
        FileStreamState::Failed => RecordingState::Failed,
    };
    RecordingInfo {
        recording_handle: u64::from(info.id),
        state,
        history: info.flags & EVENTS_STREAM_HISTORY != 0,
        append: info.flags & EVENTS_STREAM_APPEND != 0,
        records: info.records,
        bytes: info.bytes,
        lost: info.lost,
        path: info.path_bytes,
        error: info.error,
        extensions: Extensions::default(),
    }
}

#[cfg(unix)]
fn native_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(unix))]
fn native_path(bytes: &[u8]) -> Result<PathBuf, Error> {
    let path = String::from_utf8(bytes.to_vec())
        .map_err(|_| Error::Invalid("recording path is not platform UTF-8"))?;
    Ok(PathBuf::from(path))
}

fn extension_u32(tag: u64, value: u32) -> Extension {
    Extension {
        tag: u16::try_from(tag).expect("generated Events limit tag fits u16"),
        required: false,
        value: value.to_le_bytes().to_vec(),
    }
}

fn extension_u64(tag: u64, value: u64) -> Extension {
    Extension {
        tag: u16::try_from(tag).expect("generated Events limit tag fits u16"),
        required: false,
        value: value.to_le_bytes().to_vec(),
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(Error::CorruptJournal("truncated u32"))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    bytes
        .get(offset..offset + 8)
        .and_then(|value| value.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(Error::CorruptJournal("truncated u64"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventType;
    use yas_wire::codec::Extensions;
    use yas_wire::events::ActivationSet;

    fn runtime(size: usize) -> (Runtime, Arc<EventLog>) {
        let log = EventLog::new(size, ActivationSet::all());
        (Runtime::new(log.clone()), log)
    }

    #[tokio::test]
    async fn config_cas_and_dump_are_exact() {
        let (runtime, log) = runtime(events::MIN_RING_SIZE);
        log.record(EventType::Error, 0x1234, b"boom");
        let config = runtime.config();
        assert_eq!(config.revision, 1);
        assert_eq!(config.record_count, 1);

        let changed = runtime
            .set_config(&SetConfig {
                operation_id: [1; 16],
                expected_revision: 1,
                capacity: (events::MIN_RING_SIZE * 2) as u64,
                activations: wire::ActivationSet::low_throughput(),
                extensions: Extensions::default(),
            })
            .await
            .unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(changed.capacity, (events::MIN_RING_SIZE * 2) as u64);
        assert_eq!(
            runtime
                .set_config(&SetConfig {
                    operation_id: [2; 16],
                    expected_revision: 1,
                    capacity: events::MIN_RING_SIZE as u64,
                    activations: wire::ActivationSet::all(),
                    extensions: Extensions::default(),
                })
                .await,
            Err(Error::Conflict)
        );

        let snapshot = runtime.dump().await.unwrap();
        assert_eq!(&snapshot.bytes[..8], EVENT_DUMP_MAGIC);
        assert_eq!(
            snapshot.content_hash,
            *blake3::hash(&snapshot.bytes).as_bytes()
        );
    }

    #[tokio::test]
    async fn history_and_live_handoff_preserves_flags_and_has_no_gap() {
        let (runtime, log) = runtime(events::MIN_RING_SIZE);
        log.record(EventType::Error, 0x1234, b"old");
        let mut stream = runtime
            .start_stream(&StartStream {
                operation_id: [3; 16],
                history: true,
                start_sequence: 0,
                max_batch_bytes: 0,
                extensions: Extensions::default(),
            })
            .await
            .unwrap();
        log.record(EventType::ServerStop, 0x5678, b"live");

        assert_eq!(stream.first_sequence(), 0);
        let StreamItem::Records(history) = stream.next().await.unwrap().unwrap() else {
            panic!("expected records");
        };
        assert_eq!(history.records.len(), 2);
        assert_eq!(history.records[0].event_flags, 0x1234);
        assert_eq!(history.records[1].event_flags, 0x5678);
        assert_eq!(history.records[1].payload, b"live");
    }

    #[tokio::test]
    async fn overwritten_history_reports_exact_initial_gap() {
        let (runtime, log) = runtime(events::MIN_RING_SIZE);
        for index in 0..100u64 {
            log.record(EventType::Error, 0, &index.to_le_bytes().repeat(16));
        }
        let mut stream = runtime
            .start_stream(&StartStream {
                operation_id: [4; 16],
                history: true,
                start_sequence: 1,
                max_batch_bytes: 256,
                extensions: Extensions::default(),
            })
            .await
            .unwrap();
        assert!(stream.first_sequence() > 1);
        assert_eq!(
            stream.next().await.unwrap().unwrap(),
            StreamItem::Gap {
                lost: stream.first_sequence() - 1,
                first_available_sequence: stream.first_sequence(),
            }
        );
        let StreamItem::Records(batch) = stream.next().await.unwrap().unwrap() else {
            panic!("expected retained records");
        };
        assert_eq!(batch.first_sequence, stream.first_sequence());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recording_round_trips_native_path_bytes_and_final_counters() {
        use std::os::unix::ffi::OsStrExt;

        let (runtime, log) = runtime(events::MIN_RING_SIZE);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("yas-events-{}-{unique}.bin", std::process::id()));
        let path_bytes = path.as_os_str().as_bytes().to_vec();
        let started = runtime
            .start_recording(&StartRecording {
                operation_id: [5; 16],
                history: true,
                append: false,
                path: path_bytes.clone(),
                extensions: Extensions::default(),
            })
            .await
            .unwrap();
        assert_eq!(started.path, path_bytes);
        assert_eq!(runtime.recordings().recordings.len(), 1);
        log.record(EventType::Error, 0, b"saved");
        let stopped = runtime
            .stop_recording(started.recording_handle)
            .await
            .unwrap();
        assert_eq!(stopped.state, RecordingState::Stopped);
        assert!(stopped.records >= 1);
        assert!(runtime.recordings().recordings.is_empty());
        tokio::fs::remove_file(path).await.unwrap();
    }
}
