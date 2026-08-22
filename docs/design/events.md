# Binary server event journal

- **Status:** Implemented as native YAS Events family v1
- **Date:** 2026-08-21
- **Companion to:** [../protocol.md](../protocol.md),
  [../server.md](../server.md)

## Summary

The server owns one process-wide binary event journal. It is intended for
post-mortem diagnosis and short, targeted captures of hot paths that are too
expensive or too noisy for ordinary stderr logging.

The journal defaults to a 1 MiB contiguous byte ring and the low-throughput
lifecycle set. Event call sites use `yas_event!`; the macro checks one atomic
activation bit before it evaluates the payload expression. Disabled hot events
therefore do not allocate, format, lock the ring, or copy inspected bytes.

The same retained history can be dumped on demand, followed live through a
client connection, or written by a persistent server-side task. The history
and live handoff is ordered: for a history-enabled stream, a concurrent record
is in exactly one of the initial dump or the live stream, never silently
between them.

## Goals

- Keep a bounded, always-available diagnostic history in every server process.
- Make the disabled cost of high-volume events one atomic bit test, with no
  payload construction or shared lock acquisition.
- Activate individual event types, whole categories, the safe default set, or
  the complete catalog without restarting the server.
- Preserve raw protocol and PTY bytes when explicitly enabled, while keeping
  those expensive and sensitive events off by default.
- Support post-mortem dumps and live client/file capture through one stable,
  self-describing binary representation.
- Keep event production independent of slow clients and files. Observability
  must not become backpressure on the terminal, compositor, or network paths.
- Expose loss explicitly through counters, sequence gaps, and stream gap
  records instead of presenting an incomplete capture as complete.

## Non-goals

- **Not a replacement for stderr, metrics, or tracing spans.** The journal is
  a bounded forensic record, not the primary operator log or an aggregation
  system.
- **Not lossless under arbitrary load.** The ring overwrites old records and a
  slow live consumer can lag. Both cases are detectable.
- **Not durable unless a file stream is configured.** Ring contents disappear
  with the process and configuration changes are not persisted by the
  protocol.
- **Not a new authorization boundary.** Direct clients and extensions already
  have server-side filesystem/process authority and may inspect events. A
  transport that deliberately exposes a read-only subset still withholds the
  family.
- **Not a universal payload schema.** The common record header is stable;
  event-specific payloads remain compact binary structures owned by their
  event types.

## Architecture

```mermaid
flowchart LR
    Site["yas_event! call site"] -->|"atomic bit enabled"| Encode["Build binary payload"]
    Encode --> Record["EventLog::record"]
    Record --> Ring["Bounded byte ring"]
    Record --> Broadcast["Bounded live broadcast"]
    Ring --> Dump["Dedicated dump task"]
    Ring --> Handoff["Atomic history/live handoff"]
    Broadcast --> Handoff
    Handoff --> Client["Connection-scoped client task"]
    Handoff --> File["Process-scoped file task"]
    Native["Events family config"] --> Bits["4 atomic u64 words"]
    Native --> Resize["Ring resize task"]
    Bits --> Site
    Resize --> Ring
```

`EventLog` is process-wide and stored in `AppState`. It owns four independent
activation words, the ring, a monotonic sequence allocator, one bounded Tokio
broadcast channel, and the registry of persistent file tasks. Detached file
recordings and connection-local client stream tasks have no arbitrary admission
cap. Client tasks remain owned by the connection handler, which aborts all of
them on disconnect so none can retain a dead outbox.

### Storage model

The ring is one fixed-size byte allocation rather than a queue of heap-owned
event objects. Every record begins with its complete length. Before an append,
whole oldest records are evicted until the new record fits; neither wrapping
nor shrinking can leave a partial retained record. Resizing builds a new ring
from oldest to newest, allowing its ordinary eviction rule to preserve the
newest records that fit.

Each accepted event receives a sequence before retention is attempted. An
oversized event therefore advances `next_sequence` and `dropped`, even though
it cannot enter the ring. If live receivers exist, that event is still sent to
them: retention capacity does not impose a smaller live-record limit.

Timestamps contain both process-monotonic nanoseconds and an approximate Unix
nanosecond value derived from the process-start wall clock. Sequence and
monotonic time define ordering; wall time exists for correlation with external
logs and may inherit ordinary wall-clock error.

### Activation and hot-path cost

An event id is also its bit index. `yas_event!` reads exactly one atomic word
and evaluates its payload expression only when the bit is set. This ordering is
intentional: full frame/PTY capture often allocates and copies more bytes than
the event header itself. `EventLog::record` then serializes the common header,
takes the ring mutex once, appends, and performs a non-awaiting broadcast send.

Configuration stores and event reads are allowed to race. A concurrent event
may observe either activation set, which is preferable to placing a global
configuration lock on every call site. Ring resizing is serialized by the same
mutex used for append and dump. Configuration replacement has a process-wide
revision and may be conditional on that revision; the comparison and mutation
share this same mutex.

The common 32-byte record header is encoded in a stack array and header plus
payload are copied directly into the wrapping ring. No heap-owned broadcast
record is built when the broadcast channel has no receivers. With receivers,
one `Arc<[u8]>` is allocated after retention and shared by all followers.

### Snapshot and live-stream ordering

Creating a stream locks the ring, subscribes to the broadcast channel, and
builds its history header before releasing the lock. Producers append and
broadcast while holding that lock; the broadcast operation itself never
awaits. Consequently, a concurrent record is either appended before the
snapshot and absent from the new receiver, or appended afterward and delivered
live. This is the central no-gap/no-duplicate handoff invariant.

The channel is bounded by record count. A receiver that falls behind gets an
explicit lost-record count and resumes at the oldest still-available live
record. Client streams encode that as a `GAP` Event; file streams insert the
synthetic record type `65535`. Stream I/O happens only in dedicated tasks and
never while holding the ring mutex.

Client tasks drain already-queued records into one `RECORD` message up to a
256 KiB soft byte limit (an individual larger record is sent alone). This
amortizes protocol envelopes and outbox entries without adding latency before
the first available record.

Standalone dump construction and configuration resize run on blocking tasks
because they can copy the entire configured capacity. Starting a stream builds
its bounded initial snapshot under the ring lock before handing subsequent I/O
to the stream task. File tasks receive a one-shot stop, drain already-queued
live records (including a final gap marker if necessary), and flush before
shutdown returns.

## Core invariants

1. Retained bytes contain only complete, length-valid records.
2. `used` never exceeds `capacity`; capacity is bounded from 4 KiB to below the
   maximum logical protocol message.
3. Event sequence numbers are process-wide, monotonic, and never reused.
4. A disabled macro call does not construct its payload or lock the ring.
5. A stream's history/live boundary neither drops nor duplicates a concurrent
   event.
6. A slow stream cannot block an event producer; any resulting loss is
   observable.
7. Live Events `RECORD` delivery is not itself logged, preventing recursive
   record generation. Other event-control replies remain inspectable.
8. Stable event ids are never renumbered or reused with a different meaning.

## Configuration

`yas events config` reads the current configuration revision, capacity,
retained byte/record counts, overwrite count, next sequence, and complete
256-bit activation set.

```bash
yas events config
yas events set --size 8388608 --events 'default,+frame.*,+pty.*'
yas events set --events 'all,-frame.write'
yas events set --if-revision 12 --events default --size 1048576
```

Selectors are evaluated left-to-right. They are `default`, `all`, `none`, an
exact catalog name, or `category.*`; `+` and `-` enable and disable. A spec
whose first selector has a sign starts from the low-throughput default.

Every successful replacement advances `revision`. `--if-revision X` applies a
replacement only if the revision is still `X`; otherwise it returns common
status `CONFLICT` and leaves both size and activations unchanged. A temporary
capture can therefore save configuration at revision `X`, enable/resize it,
then restore the saved values only if the capture's own returned revision is
still current. A successful set returns the exact revision created by that
replacement even if another update races with response delivery. The restore
therefore cannot erase a concurrent operator's change.

All settings are also available at startup:

| Variable                  | Default   | Meaning                                           |
| ------------------------- | --------- | ------------------------------------------------- |
| `YAS_EVENTS_SIZE`         | `1048576` | Ring bytes; 4 KiB through just under 64 MiB       |
| `YAS_EVENTS`              | `default` | Activation selector expression                    |
| `YAS_EVENTS_FILE`         | unset     | Start a persistent server-side binary file stream |
| `YAS_EVENTS_FILE_HISTORY` | `1`       | `0` starts the startup file at the next event     |
| `YAS_EVENTS_FILE_APPEND`  | `0`       | `1` appends instead of truncating                 |

Invalid startup sizes fall back to 1 MiB. Invalid activation expressions are
reported and fall back to `default`.

## Event catalog

The stable event id is its activation-bit index. IDs 0–15 are the default
low-throughput set.

| IDs   | Names                                                                                                                                                      | Payload intent                                  |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| 0–15  | `server.*`, `task.*`, `client.*`, `config.change`, `stream.*`, `protocol.error`, `pty.create`, `pty.exit`, `pty.remove`, `pty.deadline`, `server.capacity` | Lifecycle and failures                          |
| 16–19 | `frame.read`, `frame.write`, `message.read`, `message.write`                                                                                               | Exact framed payloads and family/kind summaries |
| 20–23 | `tick.*`, `session.lock`                                                                                                                                   | Scheduler passes, wakeups, and lock wait time   |
| 24–29 | `pty.read`, `pty.write`, `pty.parse`, `pty.snapshot`, `pty.resize`, `pty.input`                                                                            | PTY byte flow and terminal processing           |
| 30–34 | `compositor.*`, `surface.encode`, `surface.frame`, `audio.frame`                                                                                           | Compositor and media pipeline activity          |
| 35–43 | `fs.request`, `git.request`, `lsp.request`, `kv.request`, `net.request`, `process.request`, `extension.request`, `channel.request`, `client.control`       | Protocol-family dispatch                        |
| 44–47 | `outbox.queue`, `supervisor.event`, `connection.accept`, `server.error`                                                                                    | Delivery and server internals                   |

`pty.create` records the session ID, native request ID, stage, status, and
Terminal handle. Stages cover Request admission, session-lock acquisition,
spawn begin/end, registration, refusal, and physical Result write. This makes
the complete native CREATE latency visible without decoding the Terminal body.

The full names and ids live in `crates/server/src/events.rs`. Unknown activation
bits round-trip through the protocol, allowing a new server catalog to be
configured by a generic client.

## Security and access

`frame.read`, `frame.write`, `pty.read`, and `pty.write` contain the inspected
bytes, not a text rendering. They can include terminal contents, clipboard
data, paths, environment values, and other secrets. The event family has the
same authority model as the rest of a direct server connection; read-only
read-only edges do not select Events. In-process extension sessions already
have filesystem and process authority, so they select the family under the same
native HELLO policy instead of pretending the recorder is another sandbox.

## Dump format

A dump is self-describing and uses little-endian fields:

```text
[magic:"YASEVT1":8]
[header_len:2 = 84][version:2 = 1]
[capacity:8][used:8][record_count:8][dropped:8][next_sequence:8]
[activations:4 * u64]
[records...]
```

Each retained record is complete even after wrapping:

```text
[record_len:4][event_type:2][flags:2]
[sequence:8][monotonic_ns:8][unix_ns:8][type_payload:N]
```

`record_len` includes the 32-byte record header. `sequence` increments for
every attempted record, including a record too large for the configured ring.
`dropped` counts oversize records and records overwritten or discarded during
a shrink. `unix_ns` is the process-start wall clock plus the monotonic offset;
ordering should use `sequence` or `monotonic_ns`.

A live file stream begins with a dump header. With history it contains the
retained records too; without history the header has zero used bytes and zero
records, followed by new records. Append mode starts another self-describing
header/history segment rather than appending unframed records to an old
segment. A lagged file task inserts synthetic type `65535` with `[lost:8]`.
Client streams report the same condition in a `GAP` Event.

## Native YAS contract

Events is family `0x0044`, version 1. The canonical Requests, Events, limits,
record-batch codec, and recording records are generated from
[`protocol/yas/families/events.toml`](../../protocol/yas/families/events.toml);
the family contract is in [yas.md](yas.md#events-family).

`GET_CONFIG` returns the current ring revision, capacity, retained bytes/records,
overwrite count, next sequence, and complete four-word activation set.
`SET_CONFIG` conditionally replaces capacity and activation under a nonzero
operation ID. `DUMP` returns a self-describing sensitive BYTE Transfer with exact
length and BLAKE3-256 hash.

`START_STREAM` chooses retained history or live-only delivery and an optional
first sequence. `RECORD` batches complete sequenced records under the
`events-v1` packed codec and never uses Transfer: observability must not
backpressure observed work. A slow consumer gets `GAP` with exact lost count and
resumes at the oldest available sequence. `STOP_STREAM` is session-scoped.

A session admits at most four concurrent `DUMP` snapshots and sixteen combined
pending/live client streams. The admission permit follows the actual blocking
journal operation through completion; cancelling its wire request cannot turn
uncancellable work into an unbounded detached-task queue.

`START_RECORDING`, `STOP_RECORDING`, and `LIST_RECORDINGS` manage process-owned
server-file tasks under boot-scoped recording handles. A recording survives its
requesting session but not server restart. State records expose running/stopped/
failed state, path, flags, counters, live loss, and delayed file errors. Unknown
optional event IDs are retained or skipped; an unknown required ID rejects the
batch. Raw frame, PTY, environment, and content events remain disabled by
default and every Event frame is sensitive.

## Failure behavior

| Condition                                                      | Behavior visible to the operator                                                                                                                 |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Ring lacks space                                               | Complete oldest records are evicted and `dropped` increases.                                                                                     |
| One record exceeds ring capacity                               | It is not retained, `dropped` increases, and active live streams still receive it.                                                               |
| Live receiver falls behind                                     | Client gets a `GAP` Event; file gets synthetic type `65535`, both with the lost count.                                                           |
| Resize shrinks below retained use                              | Newest complete records survive and every eviction increments `dropped`.                                                                         |
| Conditional config revision is stale                           | The replacement returns `CONFLICT`; size and activations remain unchanged.                                                                       |
| Invalid protocol request                                       | A common-status Result is returned when the native request ID can be recovered.                                                                  |
| Invalid startup size                                           | Capacity falls back to 1 MiB.                                                                                                                    |
| Invalid startup activation expression                          | The error is written to stderr and activation falls back to `default`.                                                                           |
| Server-file open, header/history write, or initial flush fails | Protocol start returns an error and no recording id; startup configuration reports stderr and records `server.error` when enabled.               |
| File write or final flush fails after start                    | `record list` reports `failed`, successful record/byte counters, live loss, and the error; `record stop` removes the task but returns the error. |
| Client disconnects                                             | Its stream tasks are aborted and removed; process-scoped file streams continue.                                                                  |
| Server shuts down                                              | `server.stop` is recorded, file tasks drain queued records, flush, and join.                                                                     |

Loss has two independent measures. The dump header's `dropped` count describes
retention loss in the ring. A stream gap describes delivery loss for one live
receiver. A capture can have either without the other, so consumers must not
merge the counters.

## Evolution

- HELLO selects Events family version 1 before any family body is decoded.
- Event IDs are append-only. Decoders retain or skip unknown optional IDs and
  reject unknown required IDs; activation words round-trip unknown bits.
- `header_len` lets a future dump version add common metadata without moving the
  record section for a reader which honors the length.
- Record flags and typed payloads provide bounded per-event growth. Changing an
  existing ID or common field requires a new packed-codec or family version.
- Append-mode files contain consecutive self-describing dump segments; readers
  accept another `YASEVT01` header at a record boundary.

## Alternatives considered

**Text or JSON events.** Rejected for the hot path: formatting and escaping are
paid before retention, raw binary frames inflate substantially, and parsing
cost obscures the timings being diagnosed.

**A `VecDeque<Vec<u8>>` of records.** Simpler wrapping semantics, but it adds an
allocation per enabled event and makes the configured byte bound include
allocator behavior rather than one exact reservation.

**A fixed-record or lock-free ring.** Not adopted without measurements. The
variable-length byte ring preserves exact raw frames and compact richer
payloads under one byte budget. V1 first removes avoidable header and idle-live
allocations; a benchmark must demonstrate that another layout improves the
real hot path before trading those semantics away.

**Synchronous writes to every stream.** Rejected because a slow filesystem or
client would add latency and failure modes to PTY, compositor, and protocol
processing.

**An unbounded live channel.** Rejected because diagnostics must not turn a
slow consumer into unbounded server memory. The bounded broadcast plus explicit
gaps makes the tradeoff visible.

**A memory-mapped persistent ring.** Rejected for v1: it adds crash-consistency,
permissions, cleanup, and cross-platform concerns to the always-on path. An
ordinary file stream supplies opt-in durability without changing ring
semantics.

**One global activation lock.** Rejected because disabled events are the common
case. Four atomic `u64` words cover 256 stable ids on every supported target
without requiring wide atomic support.

## Validation

Tests cover ring wrapping, shrink preservation, disabled-event gating,
oversized live delivery, conditional configuration conflicts, client/file
streams beyond the former admission counts, file initialization and
delayed-write status, activation
selectors, strict batched-record codecs, extension-session access, and
correlated Terminal `CREATE` stages through physical Result write. Framed
connections select Events v1, change configuration, and retrieve dumps. The
wire, server, JavaScript, and CLI unit suites and strict Clippy are part of the
implementation verification.

## Operator workflow

`dump` and `tail` render readable records by default. `dump --binary` writes
the self-describing journal dump bytes. `tail --binary` writes concatenated
`events-v1` `EventBatch` encodings: each batch carries its first monotonic
sequence and self-delimiting record lengths, but it is not a journal dump and
does not add wall-clock timestamps. Detached recordings use the journal dump
format.

```bash
yas events dump
yas events tail
yas events tail --from-now
yas events dump --binary --output /tmp/yas.events
yas events tail --binary --output /tmp/live.events
ID=$(yas events record start /var/log/yas.events)
yas events record list
yas events record stop "$ID"
```

`dump` and `tail` always deliver bytes to the invoking client and default to
stdout; `--output` is therefore always a local path. `record` exclusively
manages detached server-side file tasks, making both path locality and client
lifetime explicit in the grammar. `record start` does not print an id until the
file header and requested history are written and flushed; `record list`
exposes later failures and counters, and `record stop` reports a delayed write
or final-flush failure.
