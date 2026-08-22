//! Human-readable rendering for the binary server event journal.

use std::fmt::Write as _;

use time::OffsetDateTime;
use yas_wire::events::ActivationSet;

const BYTE_PREVIEW: usize = 96;
const EVENT_DUMP_MAGIC: &[u8; 8] = b"YASEVT01";
const EVENT_DUMP_HEADER_LEN: usize = 84;
const EVENT_RECORD_HEADER_LEN: usize = 32;
const EVENT_TYPE_STREAM_GAP: u16 = u16::MAX;
const EVENTS_TARGET_CLIENT: u8 = 0;
const EVENTS_TARGET_FILE: u8 = 1;

#[derive(Clone, Copy)]
#[repr(u32)]
enum EventType {
    ServerStart = 0,
    ServerStop = 1,
    TaskStart = 2,
    TaskStop = 3,
    ClientConnect = 4,
    ClientDisconnect = 5,
    ClientReject = 6,
    ConfigChange = 7,
    StreamStart = 8,
    StreamStop = 9,
    ProtocolError = 10,
    PtyCreate = 11,
    PtyExit = 12,
    PtyRemove = 13,
    Deadline = 14,
    Capacity = 15,
    FrameRead = 16,
    FrameWrite = 17,
    MessageRead = 18,
    MessageWrite = 19,
    TickStart = 20,
    TickStop = 21,
    TickNudge = 22,
    SessionLock = 23,
    PtyRead = 24,
    PtyWrite = 25,
    PtyParse = 26,
    PtySnapshot = 27,
    PtyResize = 28,
    PtyInput = 29,
    CompositorEvent = 30,
    CompositorCommand = 31,
    SurfaceEncode = 32,
    SurfaceFrame = 33,
    AudioFrame = 34,
    FsRequest = 35,
    GitRequest = 36,
    LspRequest = 37,
    KvRequest = 38,
    NetRequest = 39,
    ProcessRequest = 40,
    ExtensionRequest = 41,
    ChannelRequest = 42,
    ClientControl = 43,
    OutboxQueue = 44,
    Supervisor = 45,
    ConnectionAccept = 46,
    Error = 47,
}

const EVENT_TYPES: &[EventType] = &[
    EventType::ServerStart,
    EventType::ServerStop,
    EventType::TaskStart,
    EventType::TaskStop,
    EventType::ClientConnect,
    EventType::ClientDisconnect,
    EventType::ClientReject,
    EventType::ConfigChange,
    EventType::StreamStart,
    EventType::StreamStop,
    EventType::ProtocolError,
    EventType::PtyCreate,
    EventType::PtyExit,
    EventType::PtyRemove,
    EventType::Deadline,
    EventType::Capacity,
    EventType::FrameRead,
    EventType::FrameWrite,
    EventType::MessageRead,
    EventType::MessageWrite,
    EventType::TickStart,
    EventType::TickStop,
    EventType::TickNudge,
    EventType::SessionLock,
    EventType::PtyRead,
    EventType::PtyWrite,
    EventType::PtyParse,
    EventType::PtySnapshot,
    EventType::PtyResize,
    EventType::PtyInput,
    EventType::CompositorEvent,
    EventType::CompositorCommand,
    EventType::SurfaceEncode,
    EventType::SurfaceFrame,
    EventType::AudioFrame,
    EventType::FsRequest,
    EventType::GitRequest,
    EventType::LspRequest,
    EventType::KvRequest,
    EventType::NetRequest,
    EventType::ProcessRequest,
    EventType::ExtensionRequest,
    EventType::ChannelRequest,
    EventType::ClientControl,
    EventType::OutboxQueue,
    EventType::Supervisor,
    EventType::ConnectionAccept,
    EventType::Error,
];

impl EventType {
    fn from_id(id: u32) -> Option<Self> {
        EVENT_TYPES.get(usize::try_from(id).ok()?).copied()
    }

    fn id(self) -> u16 {
        self as u16
    }

    fn name(self) -> &'static str {
        crate::yas_events::EVENT_NAMES[self as usize]
    }
}

pub(crate) fn render_dump(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < EVENT_DUMP_HEADER_LEN {
        return Err("event dump is truncated".into());
    }
    if bytes.get(..EVENT_DUMP_MAGIC.len()) != Some(EVENT_DUMP_MAGIC.as_slice()) {
        return Err("event dump has invalid magic".into());
    }
    let header_len = read_u16(bytes, 8)? as usize;
    let version = read_u16(bytes, 10)?;
    if header_len < EVENT_DUMP_HEADER_LEN || header_len > bytes.len() {
        return Err(format!("event dump has invalid header length {header_len}"));
    }
    let capacity = read_u64(bytes, 12)?;
    let used = read_u64(bytes, 20)?;
    let declared_records = read_u64(bytes, 28)?;
    let dropped = read_u64(bytes, 36)?;
    let next_sequence = read_u64(bytes, 44)?;
    let activations = activation_set(&bytes[52..84])
        .ok_or_else(|| "event dump has invalid activation bitset".to_owned())?;
    let records = &bytes[header_len..];
    if used != records.len() as u64 {
        return Err(format!(
            "event dump declares {used} retained bytes but contains {}",
            records.len()
        ));
    }

    let enabled = EVENT_TYPES
        .iter()
        .filter_map(|&kind| activations.enabled(kind.id()).then_some(kind.name()))
        .collect::<Vec<_>>()
        .join(",");
    let mut output = format!(
        "# yas.events.v{version} capacity={capacity} retained_bytes={used} retained_records={declared_records} dropped={dropped} next_sequence={next_sequence} enabled={enabled}\n"
    );
    let (rendered, actual_records) = render_record_bytes(records)?;
    if actual_records as u64 != declared_records {
        return Err(format!(
            "event dump declares {declared_records} records but contains {actual_records}"
        ));
    }
    output.push_str(&rendered);
    Ok(output)
}

#[cfg(test)]
pub(crate) fn render_records(bytes: &[u8], expected: Option<u16>) -> Result<String, String> {
    let (rendered, actual) = render_record_bytes(bytes)?;
    if let Some(expected) = expected
        && actual != usize::from(expected)
    {
        return Err(format!(
            "event batch declares {expected} records but contains {actual}"
        ));
    }
    Ok(rendered)
}

pub(crate) fn render_gap(lost: u64) -> String {
    format!("! stream.gap lost={lost}\n")
}

/// Render a canonical YAS Events packed batch. Live native batches carry the
/// server monotonic timestamp but deliberately omit a wall-clock timestamp,
/// so the output does not invent one.
pub(crate) fn render_native_batch(batch: &yas_wire::events::EventBatch) -> String {
    let mut output = String::new();
    for record in &batch.records {
        let kind = EventType::from_id(record.event_id);
        let name = kind
            .map(EventType::name)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("event.{}", record.event_id));
        write!(
            output,
            "+{} #{} {name}",
            format_duration(record.monotonic_ns),
            record.sequence
        )
        .expect("writing to String");
        if record.required {
            output.push_str(" required");
        }
        if record.event_flags != 0 {
            write!(output, " flags=0x{:04x}", record.event_flags).expect("writing to String");
        }
        if !record.payload.is_empty() {
            let detail = kind
                .and_then(|kind| describe_payload(kind, &record.payload))
                .unwrap_or_else(|| format!("payload={}", quoted_bytes(&record.payload)));
            write!(output, " {detail}").expect("writing to String");
        }
        output.push('\n');
    }
    output
}

fn render_record_bytes(mut bytes: &[u8]) -> Result<(String, usize), String> {
    let mut output = String::new();
    let mut count = 0usize;
    while !bytes.is_empty() {
        if bytes.len() < EVENT_RECORD_HEADER_LEN {
            return Err("event record header is truncated".into());
        }
        let len = read_u32(bytes, 0)? as usize;
        if len < EVENT_RECORD_HEADER_LEN || len > bytes.len() {
            return Err(format!("event record has invalid length {len}"));
        }
        render_record(&bytes[..len], &mut output)?;
        bytes = &bytes[len..];
        count += 1;
    }
    Ok((output, count))
}

fn render_record(record: &[u8], output: &mut String) -> Result<(), String> {
    let event_id = read_u16(record, 4)?;
    let flags = read_u16(record, 6)?;
    let sequence = read_u64(record, 8)?;
    let monotonic_ns = read_u64(record, 16)?;
    let unix_ns = read_u64(record, 24)?;
    let payload = &record[EVENT_RECORD_HEADER_LEN..];
    let kind = EventType::from_id(u32::from(event_id));
    let name = kind.map(EventType::name);
    let timestamp = format_timestamp(unix_ns);
    let event_name = if event_id == EVENT_TYPE_STREAM_GAP {
        "stream.gap".to_owned()
    } else {
        name.map(str::to_owned)
            .unwrap_or_else(|| format!("event.{event_id}"))
    };

    write!(
        output,
        "{timestamp} +{} #{sequence} {event_name}",
        format_duration(monotonic_ns)
    )
    .expect("writing to String");
    if flags != 0 {
        write!(output, " flags=0x{flags:04x}").expect("writing to String");
    }
    if !payload.is_empty() {
        let detail = if event_id == EVENT_TYPE_STREAM_GAP && payload.len() == 8 {
            format!("lost={}", read_u64(payload, 0)?)
        } else {
            kind.and_then(|kind| describe_payload(kind, payload))
                .unwrap_or_else(|| format!("payload={}", quoted_bytes(payload)))
        };
        write!(output, " {detail}").expect("writing to String");
    }
    output.push('\n');
    Ok(())
}

fn describe_payload(kind: EventType, payload: &[u8]) -> Option<String> {
    let mut cursor = Cursor::new(payload);
    let detail = match kind {
        EventType::ServerStart => {
            format!("version={:?} server={:?}", cursor.name()?, cursor.name()?)
        }
        EventType::TaskStart | EventType::TaskStop => format!("task={:?}", cursor.name()?),
        EventType::ClientReject => format!("reason={:?}", cursor.name()?),
        EventType::ProtocolError | EventType::Error => format!("message={:?}", cursor.name()?),
        EventType::Capacity => format!("resource={:?}", cursor.name()?),
        EventType::ClientConnect | EventType::ClientDisconnect => {
            format!("client={}", cursor.u64()?)
        }
        EventType::ConfigChange => {
            let client = cursor.u64()?;
            let size = cursor.u64()?;
            let active = activation_set(cursor.take(32)?)?;
            let names = EVENT_TYPES
                .iter()
                .filter_map(|&event| active.enabled(event.id()).then_some(event.name()))
                .collect::<Vec<_>>()
                .join(",");
            format!("client={client} capacity={size} enabled={names}")
        }
        EventType::StreamStart => describe_stream_start(&mut cursor)?,
        EventType::StreamStop => {
            format!("client={} stream={}", cursor.u64()?, cursor.u32()?)
        }
        EventType::PtyCreate => {
            let client = cursor.u64()?;
            let nonce = cursor.u16()?;
            let stage = cursor.u8()?;
            let status = cursor.u8()?;
            let pty = cursor.u16()?;
            format!(
                "client={client} nonce={nonce} stage={} status={} pty={pty}",
                pty_create_stage(stage),
                status_text(status),
            )
        }
        EventType::PtyExit => {
            let pty = cursor.u16()?;
            let status = cursor.i32()?;
            let reason = cursor.u8()?;
            format!(
                "pty={pty} status={status} reason={:?}",
                exit_reason_text(reason)
            )
        }
        EventType::PtyRemove => {
            let pty = cursor.u16()?;
            let source = cursor
                .u8_optional()
                .map(|value| if value == 1 { "close" } else { "unknown" })
                .unwrap_or("retention");
            format!("pty={pty} source={source}")
        }
        EventType::Deadline => {
            let pty = cursor.u16()?;
            let stage = match cursor.u8()? {
                1 => "term",
                2 => "kill",
                _ => "unknown",
            };
            format!("pty={pty} stage={stage}")
        }
        EventType::FrameRead
        | EventType::FrameWrite
        | EventType::SurfaceFrame
        | EventType::AudioFrame => describe_frame(&mut cursor)?,
        EventType::MessageRead
        | EventType::MessageWrite
        | EventType::FsRequest
        | EventType::GitRequest
        | EventType::LspRequest
        | EventType::KvRequest
        | EventType::NetRequest
        | EventType::ProcessRequest
        | EventType::ExtensionRequest
        | EventType::ChannelRequest
        | EventType::ClientControl
        | EventType::CompositorCommand => format!(
            "client={} opcode=0x{:02x} bytes={}",
            cursor.u64()?,
            cursor.u8()?,
            cursor.u32()?
        ),
        EventType::TickStop => format!(
            "elapsed={} clients={} ptys={}",
            format_duration(cursor.u64()?),
            cursor.u32()?,
            cursor.u32()?
        ),
        EventType::SessionLock => format!(
            "owner={:?} waited={}",
            cursor.name()?,
            format_duration(cursor.u64()?)
        ),
        EventType::PtyRead | EventType::PtyWrite => {
            let pty = cursor.u16()?;
            let declared = cursor.u32()? as usize;
            let bytes = cursor.take(declared)?;
            format!("pty={pty} bytes={declared} data={}", quoted_bytes(bytes))
        }
        EventType::PtyParse => format!(
            "pty={} bytes={} elapsed={}",
            cursor.u16()?,
            cursor.u32()?,
            format_duration(cursor.u64()?)
        ),
        EventType::PtySnapshot => format!("pty={}", cursor.u16()?),
        EventType::PtyResize => format!(
            "client={} pty={} rows={} cols={}",
            cursor.u64()?,
            cursor.u16()?,
            cursor.u16()?,
            cursor.u16()?
        ),
        EventType::PtyInput => {
            let client = cursor.u64()?;
            let pty = cursor.u16()?;
            format!(
                "client={client} pty={pty} data={}",
                quoted_bytes(cursor.remaining())
            )
        }
        EventType::CompositorEvent => describe_compositor_event(&mut cursor)?,
        EventType::SurfaceEncode => format!(
            "surface={} client={} size={}x{} bytes={} codec={} keyframe={}",
            cursor.u16()?,
            cursor.u64()?,
            cursor.u32()?,
            cursor.u32()?,
            cursor.u32()?,
            cursor.u8()?,
            cursor.u8()? != 0
        ),
        EventType::OutboxQueue => {
            format!("client={} bytes={}", cursor.u64()?, cursor.u32()?)
        }
        EventType::Supervisor => match cursor.u8()? {
            1 => "stage=start".to_owned(),
            2 => format!("stage=stop elapsed={}", format_duration(cursor.u64()?)),
            stage => format!("stage={stage}"),
        },
        EventType::ServerStop
        | EventType::TickStart
        | EventType::TickNudge
        | EventType::ConnectionAccept => return None,
    };
    cursor.finished().then_some(detail)
}

fn describe_stream_start(cursor: &mut Cursor<'_>) -> Option<String> {
    let payload = cursor.remaining();
    let mut request = Cursor::new(payload);
    if payload
        .get(12)
        .is_some_and(|target| matches!(*target, EVENTS_TARGET_CLIENT | EVENTS_TARGET_FILE))
    {
        let client = request.u64()?;
        let stream = request.u32()?;
        let target = match request.u8()? {
            EVENTS_TARGET_CLIENT => "client",
            EVENTS_TARGET_FILE => "file",
            _ => unreachable!("checked above"),
        };
        let path = if request.is_empty() {
            String::new()
        } else {
            format!(" path={:?}", request.name()?)
        };
        if request.finished() {
            return Some(format!(
                "client={client} stream={stream} target={target}{path}"
            ));
        }
    }

    let mut startup = Cursor::new(payload);
    let stream = startup.u32()?;
    let detail = format!("stream={stream} target=file path={:?}", startup.name()?);
    startup.finished().then_some(detail)
}

fn describe_frame(cursor: &mut Cursor<'_>) -> Option<String> {
    let client = cursor.u64()?;
    let declared = cursor.u32()? as usize;
    let frame = cursor.take(declared)?;
    let opcode = frame.first().copied();
    Some(match opcode {
        Some(opcode) => format!(
            "client={client} bytes={declared} opcode=0x{opcode:02x} data={}",
            quoted_bytes(frame)
        ),
        None => format!("client={client} bytes=0 data=b\"\""),
    })
}

fn describe_compositor_event(cursor: &mut Cursor<'_>) -> Option<String> {
    match cursor.u8()? {
        1 => Some(format!(
            "kind=created surface={} parent={} size={}x{} title={:?} app_id={:?}",
            cursor.u16()?,
            cursor.u16()?,
            cursor.u16()?,
            cursor.u16()?,
            cursor.name()?,
            cursor.name()?
        )),
        2 => Some(format!("kind=destroyed surface={}", cursor.u16()?)),
        3 => Some(format!(
            "kind=commit surface={} size={}x{} timestamp_ms={} timestamp_sub_us={} encoder_skip={}",
            cursor.u16()?,
            cursor.u32()?,
            cursor.u32()?,
            cursor.u32()?,
            cursor.u16()?,
            cursor.u8()? != 0
        )),
        kind => Some(format!(
            "kind={kind} payload={}",
            quoted_bytes(cursor.remaining())
        )),
    }
}

fn pty_create_stage(stage: u8) -> &'static str {
    match stage {
        1 => "request-received",
        2 => "session-acquired",
        3 => "spawn-begin",
        4 => "spawn-end",
        5 => "registered",
        6 => "refused",
        7 => "reply-written",
        _ => "unknown",
    }
}

fn activation_set(bytes: &[u8]) -> Option<ActivationSet> {
    if bytes.len() != 32 {
        return None;
    }
    let mut words = [0u64; 4];
    for (index, word) in words.iter_mut().enumerate() {
        *word = u64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().ok()?);
    }
    Some(ActivationSet(words))
}

fn status_text(status: u8) -> &'static str {
    match status {
        0 => "ok",
        1 => "unknown id",
        2 => "not found",
        3 => "wrong type",
        4 => "permission denied",
        5 => "too large",
        6 => "budget exhausted",
        7 => "invalid request",
        8 => "cancelled",
        9 => "backend error",
        10 => "warming up",
        11 => "conflict",
        12 => "no merge base",
        _ => "unknown status",
    }
}

fn exit_reason_text(reason: u8) -> &'static str {
    match reason {
        0 => "exited",
        1 => "killed by deadline",
        2 => "killed by lease expiry",
        3 => "evicted",
        4 => "stopped by unit",
        _ => "unknown reason",
    }
}

fn quoted_bytes(bytes: &[u8]) -> String {
    let shown = bytes.len().min(BYTE_PREVIEW);
    let mut output = String::from("b\"");
    for &byte in &bytes[..shown] {
        for escaped in std::ascii::escape_default(byte) {
            output.push(char::from(escaped));
        }
    }
    output.push('"');
    if bytes.len() > shown {
        write!(output, "...(+{} bytes)", bytes.len() - shown).expect("writing to String");
    }
    output
}

/// The wall clock, to the nanosecond, always nine digits of it.
///
/// RFC 3339 permits any number of subsecond digits and the formatter drops the
/// trailing zeros, so a timestamp's precision would depend on its own value —
/// `.221884495Z` one line, `.2218Z` the next.
fn format_timestamp(unix_ns: u64) -> String {
    let Ok(value) = OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_ns)) else {
        return format!("unix-ns:{unix_ns}");
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
        value.nanosecond(),
    )
}

/// Exactly what the clock said, to the nanosecond, always the same width.
///
/// Trailing zeros are digits like any other: a rounded `+9796.711854s` cannot
/// be subtracted from the record above it, and a column that changes unit
/// every few lines cannot be read down.
fn format_duration(nanos: u64) -> String {
    format!("{}.{:09}s", nanos / 1_000_000_000, nanos % 1_000_000_000)
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at + 2)
            .ok_or_else(|| "event data is truncated".to_owned())?
            .try_into()
            .expect("slice length checked"),
    ))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or_else(|| "event data is truncated".to_owned())?
            .try_into()
            .expect("slice length checked"),
    ))
}

fn read_u64(bytes: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        bytes
            .get(at..at + 8)
            .ok_or_else(|| "event data is truncated".to_owned())?
            .try_into()
            .expect("slice length checked"),
    ))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn is_empty(&self) -> bool {
        self.at == self.bytes.len()
    }

    fn finished(&self) -> bool {
        self.is_empty()
    }

    fn remaining(&mut self) -> &'a [u8] {
        let remaining = &self.bytes[self.at..];
        self.at = self.bytes.len();
        remaining
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let value = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(value)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u8_optional(&mut self) -> Option<u8> {
        (!self.is_empty()).then(|| self.u8()).flatten()
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn name(&mut self) -> Option<String> {
        let len = usize::from(self.u16()?);
        Some(std::str::from_utf8(self.take(len)?).ok()?.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: u16, sequence: u64, payload: &[u8]) -> Vec<u8> {
        let len = EVENT_RECORD_HEADER_LEN + payload.len();
        let mut record = Vec::with_capacity(len);
        record.extend_from_slice(&(len as u32).to_le_bytes());
        record.extend_from_slice(&kind.to_le_bytes());
        record.extend_from_slice(&0u16.to_le_bytes());
        record.extend_from_slice(&sequence.to_le_bytes());
        record.extend_from_slice(&1_250_000u64.to_le_bytes());
        record.extend_from_slice(&1_700_000_000_123_456_789u64.to_le_bytes());
        record.extend_from_slice(payload);
        record
    }

    fn name(value: &str) -> Vec<u8> {
        let mut payload = (value.len() as u16).to_le_bytes().to_vec();
        payload.extend_from_slice(value.as_bytes());
        payload
    }

    #[test]
    fn renders_dump_header_and_typed_payload() {
        let mut payload = name("0.55.1");
        payload.extend_from_slice(&name("default"));
        let records = record(EventType::ServerStart.id(), 7, &payload);
        let activations = ActivationSet::low_throughput();
        let mut dump = Vec::new();
        dump.extend_from_slice(EVENT_DUMP_MAGIC);
        dump.extend_from_slice(&(EVENT_DUMP_HEADER_LEN as u16).to_le_bytes());
        dump.extend_from_slice(&1u16.to_le_bytes());
        dump.extend_from_slice(&4096u64.to_le_bytes());
        dump.extend_from_slice(&(records.len() as u64).to_le_bytes());
        dump.extend_from_slice(&1u64.to_le_bytes());
        dump.extend_from_slice(&3u64.to_le_bytes());
        dump.extend_from_slice(&8u64.to_le_bytes());
        for word in activations.0 {
            dump.extend_from_slice(&word.to_le_bytes());
        }
        dump.extend_from_slice(&records);

        let output = render_dump(&dump).unwrap();
        assert!(output.contains("retained_records=1 dropped=3 next_sequence=8"));
        assert!(output.contains("#7 server.start version=\"0.55.1\" server=\"default\""));
        // Nine digits, trailing zeros and all: 1_250_000 ns is 0.001250000s.
        assert!(output.contains("+0.001250000s"), "{output}");
    }

    #[test]
    fn unknown_events_have_bounded_escaped_payloads() {
        let payload = vec![b'a', b'\n', 0, 0xff];
        let output = render_records(&record(500, 9, &payload), Some(1)).unwrap();
        assert!(output.contains("#9 event.500 payload=b\"a\\n\\x00\\xff\""));
    }

    #[test]
    fn rejects_declared_record_count_mismatch() {
        let error =
            render_records(&record(EventType::TickNudge.id(), 1, &[]), Some(2)).unwrap_err();
        assert!(error.contains("declares 2 records but contains 1"));
    }

    #[test]
    fn renders_stream_gap() {
        assert_eq!(render_gap(12), "! stream.gap lost=12\n");
        let gap = record(EVENT_TYPE_STREAM_GAP, 0, &12u64.to_le_bytes());
        assert!(
            render_records(&gap, Some(1))
                .unwrap()
                .contains("stream.gap")
        );
    }

    #[test]
    fn native_schema_event_catalog_is_dense_and_aligned() {
        assert_eq!(EVENT_TYPES.len(), crate::yas_events::EVENT_NAMES.len());
        for (id, &kind) in EVENT_TYPES.iter().enumerate() {
            assert_eq!(usize::from(kind.id()), id);
            assert_eq!(kind.name(), crate::yas_events::EVENT_NAMES[id]);
        }
        assert_eq!(
            u64::from(EventType::Error.id()),
            yas_wire::schema::events::EVENT_SERVER_ERROR
        );
    }
}
