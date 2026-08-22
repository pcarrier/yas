# RFC: YAS wire protocol

- **Status:** Implemented in tree (version 1); release qualification pending
- **Date:** 2026-08-22
- **Protocol overview:** [../protocol.md](../protocol.md)
- **Replaces:** The retired pre-YAS wire, without wire compatibility
- **Scope:** Complete replacement for every previous protocol family
- **Companion to:** [../transports.md](../transports.md),
  [term-journal.md](term-journal.md), and
  [workspace-sessions.md](workspace-sessions.md)
- **Domain companions:** [fs-watch.md](fs-watch.md),
  [fs-write.md](fs-write.md), [fs-read.md](fs-read.md),
  [fs-search.md](fs-search.md), [fs-grep.md](fs-grep.md), [git.md](git.md),
  [lsp.md](lsp.md), [kv.md](kv.md), [processes.md](processes.md),
  [net.md](net.md), [extensions.md](extensions.md), [events.md](events.md),
  [env.md](env.md), [tray-notifications.md](tray-notifications.md), and
  [media-devices-portals.md](media-devices-portals.md)

## Summary

YAS is a new compact binary protocol. It retains direct dispatch,
purpose-built hot codecs, low startup latency, and transport simplicity while
fixing the failures that accumulated around its global opcode byte, feature
mask, parser extension rules, request correlation, and family growth model.

The first YAS release covers Core, Transfer, server relay, client control,
terminals, surfaces, selections, desktop integration, media, server fonts,
filesystem, Git, LSP, KV, native processes, network relay, channels,
extensions, server events, and the server environment. These are independent
statically identified families with shared request, state, transfer, limit,
and reconnect conventions. YAS does not pretend that nested YAS links, font
files, UDP datagrams, terminal grids, video, and state snapshots have one
universal data-plane contract.

The canonical protocol schema generates Rust and TypeScript codecs,
registries, documentation, sensitivity metadata, and golden vectors. Static
family and kind IDs identify a frame without a session-specific ID map; its
exact family version still comes from HELLO.

## Decisions

1. YAS starts at protocol major 1 and has no previous-wire compatibility path.
2. Family and kind IDs are stable `u16` values assigned by the schema. Events
   use a five-byte header; only Requests and Results carry correlation IDs.
3. The client sends the preface and HELLO without waiting for a server echo.
4. HELLO returns family versions, operations, runtime state, and limits in one
   Result. Startup does not call DESCRIBE per family.
5. Every admitted Request receives exactly one Result while the session
   remains viable.
6. Required fields use compact operation-specific layouts. Only optional tails
   use tagged extensions.
7. Byte and message transfers share credit and closure machinery. Datagram,
   media, and terminal-frame semantics remain family-specific.
8. State families share snapshot/replay/delta conventions but carry their
   records directly in their family.
9. Flow control has per-transfer or per-subscription credit and one aggregate
   receive budget.
10. Any connected endpoint can use every advertised operation and observe every
    resource exposed by the selected families.
11. Mutation IDs and state cursors make reconnect outcomes explicit.
12. Every capability the product exposes is part of the first YAS release and
    every YAS family must have generated codecs, vectors, limits, and lifecycle
    tests before release.
13. Named server routing and font delivery are YAS server capabilities. The
    browser-facing edge authenticates, adapts transport framing, and forwards
    one home-server link; it does not own either catalogue.

## Goals

- Preserve compactness and directness.
- Prevent opcode collisions and documentation drift mechanically.
- Provide explicit, uniform request completion and common statuses.
- Make every extension rule unambiguous.
- Separate static implementation support, runtime availability, and limits.
- Bound memory across all simultaneous protocol resources.
- Support reconnect replay and idempotent mutation retry.
- Keep hot terminal and media codecs packed and independently versioned.
- Make the Rust and TypeScript implementations conform to the same generated
  schema and vectors.

## Non-goals

- No previous handshake, opcode, feature-bit, or packet compatibility.
- No in-band upgrade from the retired protocol.
- No universal RPC, object, stream, state, or media model.
- No dynamic family-ID assignment.
- No wire priority field over a transport that cannot actually preempt bytes.
- No general roles, per-family ACLs, or user-defined partial-control profiles.
  A normal admitted session has full control; the one fixed
  `read_only_session` profile is reserved for explicitly attenuated sharing.

## Direct wire properties

The useful properties of the previous wire were:

- one framed binary message maps directly to one operation;
- required hot fields have compact fixed layouts;
- server and client dispatch are simple matches;
- the server can begin useful work in its first response flight;
- domain protocols can choose semantics appropriate to their data; and
- little-endian fixed-width integers are easy to inspect and implement.

YAS changes the registry, correlation, extension, state, and capability rules.
It does not replace direct messages with an object broker.

## Terminology

- **Link:** one reliable, ordered, full-duplex connection, optionally paired
  with a transport-native unreliable datagram path.
- **Session:** one accepted YAS HELLO over a link.
- **Family:** a static protocol namespace, such as Core or Terminal.
- **Kind:** a stable operation or event ID inside a family and class.
- **Frame:** one outer YAS message.
- **Request:** a frame that expects exactly one Result.
- **Result:** the terminal reply to one Request.
- **Event:** a one-way frame whose family defines its delivery semantics.
- **Transfer:** a connection-scoped byte or message flow established by a
  domain descriptor.
- **Resource:** a protocol object with an explicitly declared session, boot,
  or durable scope.

`MUST`, `MUST NOT`, `SHOULD`, and `MAY` are normative.

## Static family registry

Family IDs are stable and generated. Reuse is forbidden even after a family is
retired.

|       ID | Family          | Role                                            |
| -------: | --------------- | ----------------------------------------------- |
| `0x0000` | `yas.core`      | Session control and negotiation                 |
| `0x0001` | `yas.transfer`  | Bounded byte and message flows                  |
| `0x0002` | `yas.relay`     | Watched server routes and nested YAS links      |
| `0x0010` | `yas.terminal`  | PTYs, terminal state, views, and journal        |
| `0x0011` | `yas.client`    | Connected-client catalogue and control          |
| `0x0020` | `yas.surface`   | Compositor surfaces, input, views, and frames   |
| `0x0021` | `yas.selection` | Clipboard, primary selection, and drag/drop     |
| `0x0022` | `yas.desktop`   | Tray icons and desktop notifications            |
| `0x0023` | `yas.media`     | Audio, viewer devices, portals, and MPRIS       |
| `0x0024` | `yas.font`      | Font-family catalogue, metadata, and face bytes |
| `0x0030` | `yas.fs`        | Filesystem state, reads, search, and mutation   |
| `0x0031` | `yas.git`       | Repository state and content-addressed queries  |
| `0x0032` | `yas.lsp`       | Language-server state, diagnostics, and queries |
| `0x0033` | `yas.kv`        | Watched server key/value store                  |
| `0x0040` | `yas.process`   | Native non-PTY process lifecycle and streams    |
| `0x0041` | `yas.net`       | TCP, UDP, Unix socket, and Windows pipe relay   |
| `0x0042` | `yas.channel`   | Named bidirectional message channels            |
| `0x0043` | `yas.extension` | Wasm/JavaScript extension lifecycle             |
| `0x0044` | `yas.events`    | Server event journal and recordings             |
| `0x0045` | `yas.env`       | Server environment snapshot                     |

The registry has 65,536 entries. Exhaustion is not a design concern; collision
and reuse are CI failures. Static IDs keep captures, logs, event journals, and
crash dumps routeable and nameable without a session-specific ID map. Exact
payload decoding still uses the family version selected in HELLO.

Kinds are keyed by `(family, version, class, kind)`, so Request and Event kinds
may overlap. Existing kind semantics never change within a family version.

Core version 1 is implicit and its minor range is carried in HELLO; clients do
not offer Core in `family_count`. Every other row is an ordinary versioned
family. A server selects a family only when all of its declared dependencies
are also selected.

| Family    | Version-1 shared machinery                                   |
| --------- | ------------------------------------------------------------ |
| Relay     | Transfer and state convention                                |
| Terminal  | Transfer                                                     |
| Client    | State convention                                             |
| Surface   | Transfer and state convention                                |
| Selection | Transfer and state convention                                |
| Desktop   | Transfer and state convention                                |
| Media     | Transfer for control payloads; family-native timed frames    |
| Font      | Transfer and state convention                                |
| FS        | Transfer and state convention                                |
| Git       | Transfer and state convention                                |
| LSP       | Transfer and state convention                                |
| KV        | Transfer and state convention                                |
| Process   | Transfer                                                     |
| Net       | Transfer for reliable sockets/pipes; family-native datagrams |
| Channel   | Transfer message mode                                        |
| Extension | Transfer, Channel, and state convention                      |
| Events    | Transfer for dumps; family-native lossy live records         |
| Env       | Transfer for an oversized snapshot                           |

The state convention is generated shared structure inside each owning family,
not a negotiable family of its own. Names of actual families in the table are
negotiated dependencies; a server cannot select a dependent family without
them.

## Transport and framing

### Transport selection

The YAS transport is selected before the protocol begins:

| Transport    | YAS selector                                 |
| ------------ | -------------------------------------------- |
| Unix         | A named endpoint such as `yas-<server>.sock` |
| WebSocket    | The `yas.v1` subprotocol                     |
| WebTransport | A YAS endpoint path and token                |
| WebRTC       | A DataChannel labelled `yas.v1`              |
| SSH          | A `yas` subsystem or explicit remote command |

A YAS endpoint never sniffs for another protocol and has no compatibility
mode. An ordinary YAS session connects to one server. A home server exposes
additional servers through `yas.relay`; each successful Relay CONNECT creates
another complete YAS link carried by the outer session. Multi-server discovery
and lifecycle are therefore semantic server capabilities rather than paths,
control messages, or channel IDs owned by a browser transport adapter.

### Browser edge

The browser-facing process is called the **YAS edge**, not a gateway. Gateway
implies that it selects and connects upstream destinations; Relay moves that
job to the home server. The edge has a deliberately narrow contract:

1. serve the browser application when deployment needs an asset host;
2. authenticate the incoming browser transport before YAS bytes are accepted;
3. connect one configured home YAS server; and
4. adapt only transport envelopes, such as WebSocket messages to the home
   server's length-prefixed byte stream.

The edge does not parse family payloads, publish routes or fonts, hold remote
credentials, originate a second configuration protocol, or open one upstream
connection per destination. Apart from enforcing transport frame and queue
limits, it only adds or removes the transport length envelope; the preface and
frame bytes are unchanged. An accepted-link interface may also pair a native
datagram path with the reliable home-server link. Without that paired path the
home session advertises `receive_max_datagram = 0`, even when the browser side
uses WebTransport. Authentication is outside YAS and authorizes the resulting
full-control home-server session.

Consequently a browser opens exactly one edge transport. It watches the home
server's Relay catalogue and opens nested sessions over Relay Transfers. A
nested server can advertise Relay itself, so the model composes without adding
another multiplexing layer to the edge. The client presents each nested
session independently: server name, boot ID, session ID, selected families,
limits, resource handles, and state revisions never leak across a relay
boundary.

### Preface

The client sends this eight-byte preface:

| Offset | Type      | Value                       |
| -----: | --------- | --------------------------- |
|      0 | `[u8; 4]` | ASCII `YAS` followed by NUL |
|      4 | `u16`     | protocol major, `1`         |
|      6 | `[u8; 2]` | `0x0d 0x0a`                 |

The bytes are `59 41 53 00 01 00 0d 0a`.

The client sends Core HELLO immediately after the preface without waiting for
a response. On a byte stream both are normally one write. On a message
transport the preface and HELLO may be consecutive messages sent without a
receive between them. The server does not echo the preface; its first frame is
the HELLO Result. This makes startup one protocol RTT.

A bad preface closes the link. Before HELLO succeeds, the client may send only
one uncompressed Core HELLO Request and the server may send only its matching
uncompressed Core HELLO Result. Each frame is limited to 64 KiB.

### Transport framing

On a byte-stream transport, each frame is prefixed by its `u32` little-endian
length:

```text
[frame_len:u32][frame:frame_len]
```

`frame_len` includes the class-specific header and payload but excludes its own
four bytes. It is at least five and at most the receiver's negotiated
wire-frame limit.

After the preface on a message transport such as WebSocket, one transport
message is one YAS frame and the length prefix is omitted. Empty, split, or
concatenated YAS frames are invalid on a message transport. The WebRTC share
bridge binds its ordered `yas.v1` DataChannel as a byte stream instead: SCTP
messages are opaque chunks, the preface and `u32` stream framing remain
end-to-end, and neither peer assigns authority based on chunk boundaries.

This preserves YAS's efficient use of native message framing while the shared
frame decoder still consumes the same bytes after the optional length prefix.

### Optional transport datagrams

A transport may associate one unreliable, unordered datagram path with the
reliable YAS link. WebTransport uses its native datagrams. WebRTC uses a
separate unordered DataChannel with zero retransmits. Unix streams, SSH,
WebSocket, and other byte/message transports normally provide none.

One transport datagram contains one complete YAS frame without a length
prefix. Every family operation has a generated datagram predicate. The
predicates are `FORBIDDEN`, `NET_NATIVE_FLOW`, `SURFACE_FRAME`, and
`MEDIA_FRAME`; only Events whose predicate succeeds are legal. Requests,
Results, Transfer frames, compressed frames, and Core Events always use the
reliable link. A datagram therefore uses the five-byte Event header. Loss,
duplication, and reordering are normal. A malformed datagram is dropped and
counted rather than killing the reliable session.

Each HELLO direction advertises `receive_max_datagram`; zero means unavailable.
The sender obeys the smaller of that value and its transport path limit. A
family must still define sequencing, recovery, and reliable fallback. Terminal
frames are never datagram-safe. `NET_NATIVE_FLOW` additionally requires the
resolved flow to have selected `NATIVE_DATAGRAM`. `SURFACE_FRAME` requires both
`DATAGRAM_ELIGIBLE` and `DISCARDABLE`, and forbids `KEYFRAME`, `CODEC_CONFIG`,
and `END_OF_STREAM`. `MEDIA_FRAME` requires `DISCARDABLE` and forbids the same
three reliable-only flags. Net UDP and Unix datagrams prefer this path; when it
is absent, OPEN reports `RELIABLE_TUNNEL`, preserving message boundaries but
explicitly adding ordered delivery, retransmission, and head-of-line blocking.

### Frame header

Every frame begins with a five-byte routing header:

```text
[family:u16][kind:u16][meta:u8]
```

`meta` bits 0 and 1 encode the class:

| Value | Class    | Following field                 |
| ----: | -------- | ------------------------------- |
|     0 | EVENT    | Payload begins immediately      |
|     1 | REQUEST  | `[request_id:u32]` then payload |
|     2 | RESULT   | `[request_id:u32]` then payload |
|     3 | Reserved | Invalid                         |

Event headers are therefore five bytes. Request and Result headers are nine.
One-way traffic does not pay four zero bytes for correlation it cannot use.

Meta bit 2 is `COMPRESSED`; bit 3 is `SENSITIVE`. All other meta bits are
reserved and zero. A schema may require SENSITIVE but a sender cannot clear
schema sensitivity by omitting the bit. SENSITIVE is a logging and diagnostics
classification; it does not change wire or operation semantics.

For a Request operation, the schema's compression and sensitivity policy also
applies to its correlated Result. Event policy applies only to that Event
class. This conservative Result rule prevents an innocuous-looking Request
from causing secret response bytes to enter ordinary diagnostics.

Request IDs are allocated independently by each endpoint, need only be unique
among that endpoint's pending Requests, and may be reused after their Result.
This bounds request tracking and keeps the header compact. A library SHOULD
increment a `u32` counter and skip zero and currently pending values. Zero is
invalid in both Request and Result headers.

There is no wire lane. Senders use three bounded local queues and a fair
scheduler: urgent Core shutdown, control, and data. Control receives a bounded
burst before data must run. Transfer BYTE_DATA, MESSAGE_DATA, CLOSE, and RESET
share one FIFO data lane so a stream terminator cannot overtake its causally
prior payload. Events GAP and STREAM_STOPPED remain ordered with RECORD, Net
DATAGRAM_STATS remains ordered with reliable DATAGRAM fallback, and Media
STREAM_STATUS stays ordered with Media FRAME. Terminal, Surface, Media, and
Events bulk chunks also use the data lane and MUST be at most 64 KiB of payload,
so an already-written frame cannot impose an unbounded priority inversion.

### Frame limits and compression

HELLO negotiates receive limits independently in each direction. Initial hard
ceilings are:

| Limit                       |  Recommended default | Hard ceiling |
| --------------------------- | -------------------: | -----------: |
| Pre-HELLO frame             |               64 KiB |       64 KiB |
| Wire frame                  |                1 MiB |       16 MiB |
| Decoded frame               |                4 MiB |       64 MiB |
| Transport datagram          | transport path limit |       64 KiB |
| Bulk DATA chunk             |               64 KiB |       64 KiB |
| Total buffered receive data |               16 MiB |        1 GiB |

If COMPRESSED is set, the class-specific payload is:

```text
[codec:u16][reserved:u16=0][decoded_len:u32][compressed:N]
```

Codec 1 is an LZ4 block. Further codecs are negotiated in HELLO. The receiver
checks `decoded_len` before allocating and verifies the exact output length.

Generic frame compression is forbidden on Transfer DATA and Terminal, Surface,
and Media frame Events, including FRAME_CHUNK. Those protocols account and
schedule decoded content directly and may select a content codec appropriate
to the domain.

The negotiated wire-frame limit is at least the nine-byte correlated header,
the decoded-frame limit is at least the wire-frame limit, a nonzero datagram
limit is at least the five-byte Event header, and the aggregate buffered limit
is nonzero.

## Encoding rules

### Required fields

Each kind has a compact, generated required layout. Integers are fixed-width
little-endian. Required strings, bytes, lists, and records carry an explicit
length or count. No value is inferred from bytes accidentally left at the end
of a frame.

Changing required fields, their order, width, or semantics requires a new
family version or a new kind.

### Optional extensions

An extensible payload explicitly ends with:

```text
[extensions_len:u32][extensions:extensions_len]
```

Each extension is:

```text
[tag:u16][flags:u16][value_len:u32][value:value_len]
```

Extension flag bit 0 is REQUIRED. Unknown optional extensions are skipped.
An unknown REQUIRED extension in a Request returns `UNSUPPORTED`.

Results and Events MUST NOT contain an unknown-semantics requirement under an
already-selected family version. If correct interpretation by an older peer is
required, the sender uses a new family version or kind. Therefore an unknown
Result/Event extension is always optional and skipped; it never tears down the
session.

Tags are unique within one payload and encoded in ascending order. Known
duplicates, invalid lengths, out-of-order tags, and unconsumed extension bytes
are `INVALID` in a Request and protocol errors in a Result or Event.

### Records

Lists of typed records use:

```text
[record_len:u32][record_kind:u16][record_flags:u16][body:record_len-4]
```

Record flag bit 0 is REQUIRED. Unknown optional records are skipped. An unknown
required record in a Request returns `UNSUPPORTED`; Results and Events follow
the same versioning rule as extensions.

Packed hot records remain legal. Their version and containing record length
are explicit in the schema.

### Common scalar policy

- Human-facing names, titles, messages, hostnames, MIME types, and family names
  are UTF-8.
- Arbitrary content, paths, argv, and environment values are byte strings.
- Unix paths use `unix-bytes`; another server platform advertises a different
  path model in its family limits.
- Environment keys are non-empty bytes containing neither NUL nor `=`; values
  may not contain NUL.
- UUIDs are 16 RFC 4122 bytes.
- Durations are `u64` nanoseconds unless the field names another unit.
- Server-monotonic timestamps are scoped by `boot_id`; client-monotonic
  timestamps by `session_id`; Unix time is diagnostic only.
- Pixel dimensions are `u32`. Logical coordinates use signed 32.32 fixed point
  in `i64`, avoiding the range regression of 16.16 coordinates.
- Terminal input remains exact PTY bytes. Surface version 1 defines physical
  keys, text, and IME independently rather than reuse DOM, evdev, or Wayland
  numeric values accidentally.
- Portable process-control enums are distinct from explicitly platform-native
  signal records.

## Version and capability model

The preface selects YAS major 1. HELLO negotiates a Core minor and one family
version for each offered static family ID.

Within a family version:

- existing kinds and fields never change semantics;
- optional fields and records may be added;
- an optional new kind may be added and is listed in HELLO's operation set;
- changing required behavior or an existing kind requires a new version.

The operation set is returned once in HELLO, not through per-family startup
queries. A client may also optimistically send an unlisted kind and receive
`UNSUPPORTED`; it never waits indefinitely.

Runtime state (`AVAILABLE`, `DEGRADED`, or `UNAVAILABLE`) and typed limits are
part of the selected-family descriptor. Core FAMILY_UPDATE carries a complete
replacement descriptor when they change. Core SESSION_INFO returns the
complete current family and limit catalogue for diagnostics or after a missed
revision.

This deliberately keeps three concepts separate:

- family version: static schema and behavior;
- operation set: optional implementation support;
- runtime state and limits: mutable availability.

There is no global feature bitmap and no mandatory DESCRIBE round trip.

## HELLO

Core family ID is `0x0000`; HELLO kind is `0x0000`. The client sends it as its
first Request, conventionally request ID 1.

### Client HELLO payload

```text
[min_minor:u16][max_minor:u16]
[receive_max_frame:u32][receive_max_decoded:u32]
[receive_max_datagram:u32]
[receive_max_buffered:u64]
[client_instance:16]
[client_name_len:u16][client_name:N]
[client_release_len:u16][client_release:M]
[family_count:u16] repeated{
  [family_id:u16][version_count:u8][offer_flags:u8]
  repeated{ [version:u16] }
}
[codec_count:u8] repeated{ [codec:u16] }
[extensions_len:u32][extensions:N]
```

Family versions are explicit supported values, not a range that accidentally
claims unimplemented intermediate versions. Offer flag bit 0 REQUIRED makes
HELLO fail when no offered version is available; other bits are zero. Family
records are ordered by ID; versions within a record are unique and descending.
Codec IDs are nonzero, unique, and ascending.

HELLO extension tags are:

| Tag | Name                | Type                           |
| --: | ------------------- | ------------------------------ |
|   1 | `idle_timeout_ns`   | `u64`                          |
|   2 | `client_platform`   | typed records                  |
|   3 | `initial_watches`   | repeated family WATCH requests |
|   4 | `read_only_session` | REQUIRED, empty marker         |

`read_only_session` requests a server-enforced least-authority catalogue. It
is REQUIRED so a server that does not understand the restriction rejects the
HELLO instead of silently granting a writable session. The restricted
catalogue is authoritative: forged Requests and Events absent from it are
rejected by the normal descriptor gate.

The read-only catalogue selects only Core plus offered Transfer, Terminal,
Surface, Media, and Font families. Client-to-server authority is exactly:

- Core PING, CANCEL, and SESSION_INFO;
- Transfer CREDIT, CLOSE, and RESET (DATA is server-to-client only);
- Terminal WATCH, UNWATCH, SCROLL, view open/configure/reset/close, READ,
  SEARCH, CWD, JOURNAL, OUTPUT, WAIT, COPY_RANGE, SEARCH_CATALOG, STATE_ACK,
  and FRAME_ACK;
- Surface WATCH, UNWATCH, view open/configure/reset/close, CAPTURE, STATE_ACK,
  and FRAME_ACK;
- Media WATCH, UNWATCH, OPEN_OUTPUT, CLOSE_STREAM, FETCH_ASSET, STATE_ACK, and
  FRAME_ACK;
- Font WATCH, UNWATCH, DESCRIBE, FETCH, and STATE_ACK.

Every other family is omitted. Every other client-originated operation,
including terminal creation/input/resizing, surface input/focus/resizing,
media acquisition or consent, CLIENT_UPDATE, and SHUTDOWN, is unadvertised and
rejected. A read-only WebRTC producer injects this marker into the first HELLO;
it never attempts to reproduce the policy by filtering operation bytes.

The `initial_watches` value is:

```text
[count:u16] repeated{
  [family_id:u16][family_version:u16]
  [watch_len:u32][watch_payload:N]
}
```

The exact family version must be offered and define the state convention. If
the server selects another offered version, that embedded watch returns
UNSUPPORTED without failing HELLO. This explicitly requests initial state
during HELLO and avoids one startup RTT for terminal, client, surface, desktop,
or any other watched catalogue. It is not an implicit subscription.

### Server HELLO Result body

After the common successful Result prefix:

```text
[minor:u16][reserved:u16=0]
[boot_id:16][session_id:16]
[receive_max_frame:u32][receive_max_decoded:u32]
[receive_max_datagram:u32]
[receive_max_buffered:u64]
[server_monotonic_ns:u64][catalog_revision:u64]
[server_name_len:u16][server_name:N]
[server_release_len:u16][server_release:M]
[family_count:u16] repeated{ [family_descriptor] }
[extensions_len:u32][extensions:N]
```

A family descriptor is length-delimited:

```text
[descriptor_len:u32]
[family_id:u16][version:u16][runtime_state:u8][reserved:u8=0]
[operation_count:u16] repeated{ [direction:u8][class:u8][kind:u16] }
[limits_len:u32][family_limit_extensions:N]
```

`descriptor_len` counts the bytes after itself, as do all `*_len` fields that
prefix a record unless a layout says otherwise.

Runtime-state values are 0 AVAILABLE, 1 DEGRADED, and 2 UNAVAILABLE.
Direction bit 0 means the server accepts the operation from the client; bit 1
means the server may send it to the client. Other bits are zero. This matters
for Event kinds, which may be input, output, or symmetric.

Only Request and Event operations appear in a descriptor. A Result is implied
by its matching Request and is validated against the pending Request rather
than advertised as a separate operation. A receiver enforces the selected
descriptor's direction before dispatching a Request or Event.

Descriptors are ordered by family ID. A required family with no shared version
makes HELLO fail with `UNSUPPORTED`; an optional family is omitted.

The descriptor list includes Core version 1 followed by every selected offered
family. Core's operation set reflects the negotiated Core minor.

HELLO Result extension tag 1 is `initial_watch_results`. It begins with a
`u16` count and contains one length-delimited embedded WATCH Result prefix and
body for each request, in request order. A watch may fail without failing
HELLO. On success, its family STATE Events may follow HELLO immediately in the
same server response flight.

HELLO Result extension tag 2 is `negotiated_codecs`:

```text
[count:u8] repeated{ [codec:u16] }
```

The IDs are nonzero, unique, ascending, and a subset of the Client HELLO codec
list. The list is the symmetric intersection usable for generic compression in
either direction. Absence means no generic compression codec was negotiated.
An endpoint MUST NOT set COMPRESSED with a codec outside this list.

The HELLO Result completes the negotiation. The client may immediately send
other selected-family Requests. There is no READY message and no implicit
server catalog burst; initial state is sent only when HELLO explicitly asks for
it.

## Requests, Results, and Events

### Completion rule

Every Request that the receiver admits while the session remains viable gets
exactly one Result with the same family, kind, and request ID. Admission occurs
after the class-specific header is valid and before family parsing.

Therefore:

- unknown or unselected-family kind returns `UNSUPPORTED`;
- unknown REQUIRED extension returns `UNSUPPORTED`;
- malformed known Request returns `INVALID`;
- inactive runtime dependency returns `UNAVAILABLE`;
- overload returns `BUSY`, `RATE_LIMITED`, or `RESOURCE_EXHAUSTED`;
- cancellation returns `CANCELLED`; and
- timeout returns `TIMEOUT`.

The guarantee does not survive link loss, process death, or a fatal frame
error. Client libraries settle every pending Request locally as DISCONNECTED
when the session ends; DISCONNECTED is not a wire status.

Events are one-way and receive no Result. Unknown optional Event kinds in a
selected family version are ignored. A syntactically malformed known Event is
a protocol error; a family may drop and count semantically stale or out-of-
range input.

### Result prefix

Every Result payload begins:

```text
[status:u16][flags:u16][detail_len:u32][detail:detail_len][body:N]
```

Flags are currently zero. Detail is an extension set containing optional
domain, domain code, human message, retry delay, and structured context. A
failed Result has no operation body.

| Code | Status               | Meaning                                             |
| ---: | -------------------- | --------------------------------------------------- |
|    0 | `OK`                 | The synchronous operation contract completed        |
|    1 | `INVALID`            | Known request with invalid syntax or arguments      |
|    2 | `UNSUPPORTED`        | Required operation or semantics are not implemented |
|    3 | `NOT_FOUND`          | Resource does not exist                             |
|    4 | `CONFLICT`           | Current state prevents the operation                |
|    5 | `BUSY`               | Temporary contention                                |
|    6 | `UNAVAILABLE`        | Runtime dependency is inactive                      |
|    7 | `RESOURCE_EXHAUSTED` | Configured or negotiated resource cap reached       |
|    8 | `RATE_LIMITED`       | Caller exceeded an operation rate                   |
|    9 | `TIMEOUT`            | Operation deadline elapsed                          |
|   10 | `CANCELLED`          | Operation was cancelled before commit               |
|   11 | `STALE`              | Revision, cursor, or CAS precondition is stale      |
|   12 | `IO`                 | External I/O failed, with a domain code in detail   |
|   13 | `INTERNAL`           | Unexpected server failure                           |

An unknown nonzero status is a generic failure. `OK` may return a resource or
Transfer whose later lifecycle has an independent close status.

### Cancellation and timeout

Core CANCEL names one nonzero pending Request ID allocated by the cancelling
endpoint. It cannot target a Request allocated by the peer. CANCEL has
its own Result. If it returns OK, the original Request later returns CANCELLED.
If the operation already committed, CANCEL returns CONFLICT or NOT_FOUND and
the original Result remains authoritative.

Blocking Requests may carry a generated optional `timeout_ns`, starting when
the receiver admits them. A requester that needs a stricter local end-to-end
deadline also runs a timer and sends CANCEL.

After a Request has returned a Transfer descriptor, the Request is complete;
the consumer closes or resets the Transfer to stop remaining production.

### Idempotent mutations

Every mutation that can leave durable or user-visible state after a lost
Result defines a required 16-byte `operation_id`. In Terminal this includes
CREATE, RESTART, SIGNAL, CLOSE, and SET_DEADLINE. It also applies to process
spawn, file commit, Git mutation, extension deployment, and device lease
acquisition.

Within one server boot, retrying the same operation ID, family, kind, and
canonical arguments returns the original outcome without repeating the
mutation. Reuse with different arguments returns CONFLICT. Servers retain
deduplication entries for at least the affected resource's lifetime unless the
family explicitly advertises a shorter bounded retry horizon. Such a horizon
defines the minimum replay guarantee and the authoritative reconciliation a
client must perform after leaving it.

A family may explicitly mark an embedded result object as ephemeral. The
mutation remains deduplicated, but a duplicate whose cached success would
republish that object returns STALE instead of the original body.

For a connection-scoped mutation, the deduplication key also includes
`session_id` and is not retryable after reconnect. Boot-scoped or durable
mutations omit the session from the key so a reconnect can resolve a lost
Result.

A family whose mutation survives a server reboot must either persist the
operation ID and outcome or provide an authoritative reconciliation query.
Boot-scoped deduplication alone is insufficient for durable filesystem, Git,
or deployment mutations.

## Core family

Core is family `0x0000`, version 1.

| Class   |     Kind | Name           | Payload/body                           |
| ------- | -------: | -------------- | -------------------------------------- |
| Request | `0x0000` | HELLO          | Defined above                          |
| Request | `0x0001` | PING           | `[sender_monotonic_ns:u64]`            |
| Request | `0x0002` | CANCEL         | `[target_request_id:u32]`              |
| Request | `0x0003` | SESSION_INFO   | empty                                  |
| Request | `0x0004` | CLIENT_UPDATE  | extension set                          |
| Request | `0x0005` | SHUTDOWN       | operation ID, grace period, and reason |
| Event   | `0x0000` | GOAWAY         | status, deadline, detail               |
| Event   | `0x0001` | SESSION_UPDATE | connection-limit replacement           |
| Event   | `0x0002` | FAMILY_UPDATE  | one complete family descriptor         |

PING's successful body is:

```text
[receiver_receive_ns:u64][receiver_send_ns:u64]
```

Together with the requester's send and receive times this gives RTT and a
clock-offset estimate. `receiver_send_ns` is not earlier than
`receiver_receive_ns`. Either endpoint may PING.

SESSION_INFO's successful body is:

```text
[session_id:16][catalog_revision:u64]
[receive_max_frame:u32][receive_max_decoded:u32]
[receive_max_datagram:u32]
[receive_max_buffered:u64][server_monotonic_ns:u64]
[family_count:u16] repeated{ [family_descriptor] }
[extensions_len:u32][extensions:N]
```

Optional extension tag 1 is `ServerDiagnostics`:

```text
[active_sessions:u32][relay_active:u32][relay_pending:u32]
[reserved:u32=0][aggregate_receive_limit:u64]
[aggregate_receive_buffered:u64]
```

The counts and budget are server-wide diagnostic snapshots.
`aggregate_receive_buffered` never exceeds `aggregate_receive_limit`.

CLIENT_UPDATE changes non-authoritative connection presentation metadata such
as a label. Display size, frame rate, decoder support, and queue depth belong
to their actual Terminal or Surface view, not this connection-wide message.

SHUTDOWN payload is:

```text
[operation_id:16][grace_ns:u64][reason_len:u32][reason:N]
```

The operation ID is nonzero and `reason` is UTF-8. `grace_ns = 0` requests an
immediate orderly shutdown after the SHUTDOWN Result and GOAWAY have been sent.
An OK Result means orderly server shutdown is scheduled. The server sends the
Result before broadcasting GOAWAY to every session, drains until the grace
deadline, then terminates. Retrying the operation ID returns the same outcome.

GOAWAY payload is:

```text
[status:u16][reserved:u16=0][close_deadline_server_ns:u64]
[detail_len:u32][detail:N]
```

After GOAWAY, the sender accepts no new Requests and drains admitted work until
the deadline. The receiver stops originating new Requests as soon as it
receives GOAWAY; already pending Requests retain the normal completion rule.
Framing corruption closes immediately without GOAWAY.

SESSION_UPDATE payload is:

```text
[catalog_revision:u64]
[receive_max_frame:u32][receive_max_decoded:u32]
[receive_max_datagram:u32]
[receive_max_buffered:u64]
[extensions_len:u32][extensions:N]
```

FAMILY_UPDATE carries `[catalog_revision:u64][family_descriptor]`. The family
ID and version match an existing selected descriptor; v1 does not add, remove,
or renegotiate family versions after HELLO. Catalogue revisions are shared by
SESSION_UPDATE and FAMILY_UPDATE. The next applied Event has exactly
`current_revision + 1`; a stale or duplicate revision is a protocol error, and
a skipped revision makes the client stop applying catalogue Events and call
SESSION_INFO for one complete replacement snapshot.

A SESSION_INFO resynchronization must return the same session ID, may not
regress the last applied catalogue revision, and obeys the same nondecreasing
wire/decoded frame-limit rule. Its family list is complete, ordered, begins
with Core version 1, and retains the family IDs and versions selected by HELLO.

YAS v1 has no acknowledgement barrier for reliable frames already in flight,
so SESSION_UPDATE cannot reduce `receive_max_frame` or
`receive_max_decoded`. Increasing them is immediate. It may reduce
`receive_max_buffered` without revoking already granted Transfer credit or
other admitted buffering; no new reservation or credit may grow the
outstanding amount until it drains under the replacement cap. A reduced
datagram limit applies to subsequently received datagrams; an oversized
in-flight datagram is dropped and counted rather than closing the reliable
session.

## Transfer family

Transfer is family `0x0001`, version 1. It handles reliable byte and reliable
message delivery only. A domain Request, Result, or Event creates a Transfer by
carrying its descriptor. There is no generic open Request because the domain
owns what is being transferred and why.

Client-allocated transfer IDs are odd; server-allocated IDs are even; zero is
invalid. The endpoint that first emits the descriptor allocates the ID. IDs are
`u32`, connection-scoped, and not reused.

### Descriptor

```text
[transfer_id:u32][mode:u8][direction:u8][flags:u16]
[receiver_send_credit:u64][sender_send_credit:u64]
[max_item_bytes:u64][max_chunk_bytes:u32]
[content_family:u16][content_kind:u16][content_version:u16]
[extensions_len:u32][extensions:N]
```

Modes are 0 BYTE and 1 MESSAGE. Relative to the frame carrying the descriptor,
direction bit 0 permits descriptor receiver-to-sender data and bit 1 permits
descriptor sender-to-receiver data. Flags are initially zero. The content
family and kind determine local scheduling; priority is not a wire semantic.

Initial credit is explicit for each permitted direction; a disallowed
direction's credit is zero. There is no automatic per-stream window.
`max_chunk_bytes` cannot exceed the canonical transport bulk limit. A MESSAGE descriptor
also advertises extension tag 1 `max_open_messages`, default 1.

Required descriptor extension tag 2 `sensitive_content` has an empty value. A
domain schema that carries sensitive bytes requires this extension; making it
REQUIRED ensures an endpoint that does not understand the classification
rejects the descriptor instead of logging its content as ordinary data.
BYTE_DATA, MESSAGE_DATA, CLOSE, and RESET for such a descriptor carry the
SENSITIVE frame flag. CREDIT contains no domain content and does not inherit
the flag.

Required descriptor extension tag 3 `upload_stage` contains
`staging_handle:u64,expires_server_ns:u64`. It appears only on a sensitive
receiver-to-sender BYTE descriptor allocated by SET_BEGIN, STAGE_WRITE,
BUFFER_BEGIN, STAGE_VALUE, or OBJECT_BEGIN. A staging handle is scoped by the
descriptor's `content_family`; different families may allocate the same numeric
handle. Before the corresponding family commit succeeds, RESET of any
descriptor carrying the `(content_family, staging_handle)` pair atomically
discards the entire stage and retires all sibling descriptors carrying that
pair. The absolute Core-monotonic expiry and owning-session loss perform the
same discard. Subsequent DATA, CLOSE, or commit using the retired handle fails
with NOT_FOUND. CLOSE only seals its one byte stream; it neither commits nor
discards the family stage. No family-specific ABORT request is needed.

`receiver_send_credit` is granted by the descriptor sender to its receiver;
`sender_send_credit` is credit the receiver previously proposed for the
descriptor sender.

A domain operation that may produce a descriptor carries the peer's proposed
initial receive credit in its initiating Request or listener configuration.
The eventual descriptor selects `sender_send_credit` no larger than that
proposal and independently chooses `receiver_send_credit`. For BYTE mode,
`max_item_bytes` is zero and ignored.

### Kinds

All Transfer frames are Events and therefore use the five-byte Event header:

|     Kind | Name         | Direction                              |
| -------: | ------------ | -------------------------------------- |
| `0x0000` | BYTE_DATA    | Permitted data direction               |
| `0x0001` | MESSAGE_DATA | Permitted data direction               |
| `0x0002` | CREDIT       | Receiver to sender                     |
| `0x0003` | CLOSE        | Sender half-closes its direction       |
| `0x0004` | RESET        | Either endpoint aborts both directions |

BYTE_DATA payload:

```text
[transfer_id:u32][offset:u64][data:N]
```

Offsets are exact and contiguous. Consumers may split or coalesce bytes after
delivery.

MESSAGE_DATA payload:

```text
[transfer_id:u32][sequence:u64][fragment_offset:u64]
[flags:u8][reserved:3=0][data:N]
```

Flag bit 0 START and bit 1 END delimit one message. Sequences increase from
zero. Fragments from up to `max_open_messages` sequences may interleave;
offsets within each sequence are exact. The receiver exposes a message only at
END. It must either reserve `max_item_bytes` within its aggregate budget or
incrementally spool fragments to a bounded sink before granting credit.

CREDIT payload is:

```text
[transfer_id:u32][cumulative_limit:u64]
```

The limit counts DATA bytes, excludes framing, and only increases. A sender
never transmits beyond it. Each direction has its own cumulative byte counter;
a CREDIT frame grants the peer's send direction.

CLOSE payload is:

```text
[transfer_id:u32][final_data_bytes:u64][status:u16][reserved:u16=0]
[detail_len:u32][detail:N]
```

CLOSE half-closes the sender's permitted direction. A bidirectional Transfer
ends after both directions close. RESET has transfer ID, status, and detail and
ends both directions immediately.

### Aggregate receive budget

An endpoint's HELLO `receive_max_buffered` covers all decoded protocol data
buffered but not consumed, including Transfer data, state records, terminal
and surface frame reassembly, optional-path datagrams retained by a family,
and other family queues. The receiver grants initial credit, later CREDIT, and
view windows such that the sum of outstanding permitted bytes does not exceed
this aggregate cap. Per-transfer, per-subscription, and per-view limits do not
multiply the memory budget.

An unreliable datagram has no credit reservation. The receiver checks its
declared length and family queue capacity before copying it and drops it when
the aggregate budget has no room.

Implementations may grant less than requested and may delay new credit. They
must not revoke already granted credit. A sender that exceeds credit resets
that Transfer; repeated violations close the session.

An incoming Request carries no credit grant of its own, so a receiver whose
aggregate budget cannot admit the payload answers that Request with
RESOURCE_EXHAUSTED and retains nothing for it. A full budget is backpressure,
not a protocol violation, and MUST NOT fail the session; the bound on retained
incoming Requests is what limits abuse.

Datagrams, terminal frames, audio, and video do not use this family. Their own
family sections define loss, ordering, timing, feedback, and congestion
semantics explicitly.

## State subscription convention

State is a generated family convention, not a common wire family. A family
that supports watched state defines three kinds with these layouts:

1. a correlated WATCH Request/Result;
2. a STATE Event from producer to consumer; and
3. a STATE_ACK Event from consumer to producer.

Subscription IDs are producer-allocated nonzero `u32` values, scoped by the
session, and not reused.

WATCH Request prefix:

```text
[flags:u16][reserved:u16=0][initial_credit:u64]
if flags.RESUME {
  [boot_id:16][revision:u64]
}
[extensions_len:u32][extensions:N]
```

WATCH flag bit 0 is RESUME; other bits are zero.

WATCH Result body:

```text
[subscription_id:u32][mode:u8][reserved:3=0][current_revision:u64]
[extensions_len:u32][extensions:N]
```

Mode 0 means a full snapshot follows; mode 1 means replay begins after the
requested revision. A server chooses snapshot when the boot ID differs, the
cursor is unavailable, or replay would exceed policy.

STATE Event prefix:

```text
[subscription_id:u32][phase:u8][flags:u8][reserved:u16=0]
[from_revision:u64][to_revision:u64]
[record_count:u16] repeated{ [typed_record] }
```

| Phase | Meaning                          |
| ----: | -------------------------------- |
|     0 | SNAPSHOT_BEGIN                   |
|     1 | SNAPSHOT_RECORDS                 |
|     2 | SNAPSHOT_END                     |
|     3 | DELTA                            |
|     4 | RESET followed by a new snapshot |

Revisions are nonzero. A snapshot uses `(0, R)`, then `(R, R)` record batches
and end. A delta uses `(R, S)`. Records explicitly encode add, replace, patch,
and remove; absence never means deletion. Concurrent changes appear after
SNAPSHOT_END.

State record kinds 0 through 3 are reserved by the convention for ADD,
REPLACE, PATCH, and REMOVE respectively. A family defines each body it emits
and may reserve further kinds. SNAPSHOT_BEGIN and RESET carry zero records.
SNAPSHOT_END may carry the final snapshot records; consumers apply those
records atomically before marking the snapshot complete. Relay and Font
version 1 encode ADD and REPLACE as their
complete published route or family-summary record, encode REMOVE as
`[resource_handle:u64][generation:u64]`, and do not emit PATCH.

STATE_ACK payload is:

```text
[subscription_id:u32][applied_revision:u64][cumulative_byte_limit:u64]
```

Credit counts the complete decoded STATE payload after the YAS frame header. It
shares HELLO's aggregate buffered-data cap with Transfers. A producer that
cannot retain needed history sends RESET naming the last valid applied revision
and the target snapshot revision, then starts that snapshot. It never hides a
gap. WATCH cancellation is a correlated family UNWATCH Request, not an
overloaded ACK.

Every STATE Event independently fits the receiver's frame, decoded-frame, and
credit-window limits. A producer does not split one revision's atomic DELTA
across Events. If a valid multi-record DELTA would exceed any of those limits,
it instead sends RESET naming the last applied and target revisions, followed
by a new snapshot whose SNAPSHOT_RECORDS Events contain one record each.
Because aggregate receive reservations may reduce a WATCH proposal, the server
admits the actual State window before returning OK. WATCH returns
RESOURCE_EXHAUSTED, without creating a subscription or retaining its credit
lease, when that admitted window cannot carry the family's guaranteed
worst-case one-record SNAPSHOT_RECORDS Event or any current snapshot Event.

The native server applies one durable publication policy to the Client,
Selection, Desktop, Media, Relay, Font, LSP, Extension, Git, Git-query, and
Channel watches: the admitted limit must carry an encoded StateEvent payload
of `RECOMMENDED_WIRE_FRAME - EVENT_HEADER_BYTES` bytes (currently
1,048,571 bytes). The actual limit remains the minimum of that WATCH's
aggregate-clamped credit and the peer's wire-frame and decoded-frame payload
limits. This common policy is deliberately independent of the server's own
receive-frame setting. Snapshot records are published one per Event. Client,
Selection, Media, and Channel producers that cannot publish an atomic
multi-record DELTA use the RESET and authoritative one-record snapshot rule
above; snapshot-oriented Extension, Git, and LSP producers likewise never
batch several records into one Event. If even one projected record cannot fit,
the producer terminates instead of waiting for credit that can never make the
record legal.

For Selection and Relay, whose catalogues and live notifications are shared
across sessions, the server acquires the change receiver before cloning the
catalogue. That one exact snapshot supplies `WATCH_RESULT.current_revision`,
the admission check, and the initial snapshot Events. A notification already
represented by that snapshot is discarded when its revision is less than or
equal to the published revision; every newer notification is then published
as the next contiguous Delta or RESET plus snapshot. This ordering closes the
snapshot/live boundary without duplicating the initial revision.

## Identity and reconnect

HELLO returns a random `boot_id` and `session_id`. Server resource handles are
nonzero `u64` values that are never reused within one boot. The allocation
strategy is deliberately unspecified; clients treat handles as opaque and do
not infer creation order.

Connection-scoped request, transfer, subscription, and view IDs are invalid
after session loss. Boot-scoped resource handles may be reacquired through a
family snapshot or replay after reconnect.

Reconnect creates a new session. A client presents `(boot_id, revision)` on
each state WATCH:

- same boot and retained cursor: replay deltas;
- same boot but unavailable cursor: fresh snapshot;
- different boot: invalidate every resource handle and snapshot.

Mutation operation IDs address ambiguous lost Results independently of state
replay. Transfers do not resume generically. Filesystem upload defines a
durable upload resource and acknowledged offset in the FS family.

There are no magic resource values for newest, absent, or wildcard. Schemas use
flags, enum variants, or optional extensions.

## Trust model

A normal YAS connection is a full-control session. Every normally admitted
endpoint may use every operation the server advertises, observe every exposed
resource catalogue, start arbitrary terminal and native processes, access the
server environment, read and mutate files, open network endpoints as the
server OS identity, and connect every route in the Relay catalogue using
credentials retained by that server. The fixed `read_only_session` HELLO
profile is the sole reduced-authority exception and exposes only the exact
passive operations listed above. The SENSITIVE meta bit only keeps command
bytes, environment values, route credentials, and content out of routine
diagnostics; it is not an authorization mechanism.

The edge passphrase is therefore a bearer-style credential for that complete
home-server authority, not an account, role, or viewer token. The edge decides
only whether to admit the transport and which single home socket it reaches;
it does not attenuate a successful session. All normally admitted clients to
the same home server can list and connect all routes that server publishes.
YAS v1 has no configurable per-client, per-family, per-route, or per-font ACL
and no way to define another subset of the home session. Deployments that do
not mutually trust their users must use separate server processes/OS
identities, route catalogues, sockets, and edge credentials. Read-only sharing
uses the single fixed, server-enforced `read_only_session` catalogue; it does
not create a general permission system.

The edge authentication exchange assumes a confidential transport. A browser
sends the bearer passphrase before protocol bytes, so an edge reachable beyond
loopback must use WSS/TLS (normally through a reverse proxy). Plain `ws://` on
an untrusted network lets a passive observer recover complete home-server
authority; protocol redaction cannot repair a leaked admission credential.

Relay is a delegation by the home server. Connector URIs, SSH identities,
passphrases, uplink tokens, and other route credentials stay in the
server-owned catalogue and never appear in route state or client-visible
connector errors. Route names, labels, transport hints, and availability are
not secret and may reveal topology. A CONNECT authorizes the home process to
use the selected credential and creates a fresh upstream session; it does not
send that credential to the client. The nested server sees a connection made
by the home server's connector and applies its own complete protocol and OS
authority. Nested Relay deliberately composes that delegation, so clients
must impose their own recursion/visited-server policy in addition to the
server's link and byte budgets.

Font enumeration can fingerprint the host. Servers may disable the family
entirely; when it is enabled, catalogue metadata is visible to every connected
client. Face-byte export is a separate explicit policy decision and remains
blocked by restricted OS/2 embedding metadata. Paths never leave the server.
Clients treat the advertised BLAKE3 digest as an identifier only after hashing
the received bytes themselves; verified bytes may then be reused across
servers without trusting either server's cache claim.

Connection, queue, credit, transfer, scan, and byte limits contain resource
use and ensure cleanup on disconnect. They are denial-of-service boundaries,
not authorization boundaries, and do not protect one authenticated client from
another authenticated client with the same full-control authority.

## Relay family

Relay is family `0x0002`, version 1. It lets a home YAS server publish a
revisioned catalogue of other YAS servers and carry independent nested YAS
links to them. It replaces gateway-owned destination files, destination paths,
and transport-level multiplexing with an ordinary negotiated server family.

| Class   |     Kind | Name       |
| ------- | -------: | ---------- |
| Request | `0x0000` | WATCH      |
| Request | `0x0001` | UNWATCH    |
| Request | `0x0002` | CONNECT    |
| Request | `0x0003` | DISCONNECT |
| Event   | `0x0000` | STATE      |
| Event   | `0x0001` | STATE_ACK  |

WATCH and UNWATCH use the state convention. The snapshot contains one record
per enabled route and changes immediately when the server configuration or
observed availability changes. Route names are unique UTF-8 strings within the
server boot and are presentation identifiers, not addresses for the client to
resolve. A route record contains:

```text
[route_handle:u64][generation:u64]
[availability:u8][transport_hint:u8][flags:u16]
[name_len:u16][name:N]
[label_len:u16][label:M]
[description_len:u32][description:O]
[extensions_len:u32][extensions:P]
```

`route_handle` is boot-scoped and never reused. `generation` increases when a
route keeps its visible identity but its connector or security material is
replaced. Availability values are 0 UNKNOWN, 1 AVAILABLE, 2 DEGRADED, and 3
UNAVAILABLE. It is advisory: CONNECT remains authoritative and may succeed or
fail after any advertised state. `transport_hint` is display-only and can
distinguish local, SSH, TCP, WebRTC, uplink, or another Relay; clients never
branch connection logic on it. Flags initially contain only bit 0 DEFAULT.
Endpoint addresses, passphrases, SSH identities, tokens, and other connector
secrets are absent from records and diagnostics.

Transport-hint values are 0 OTHER, 1 LOCAL, 2 SSH, 3 TCP, 4 WEBRTC, 5 UPLINK,
and 6 RELAY. At most one route in a snapshot has DEFAULT set. These values do
not expose connector arguments or constrain future server implementations.

CONNECT payload is:

```text
[route_handle:u64][generation:u64][initial_receive_credit:u64]
[flags:u16][reserved:u16=0]
[extensions_len:u32][extensions:N]
```

The observed generation is required, so a selection cannot silently land on a
replacement route. A stale generation returns STALE. Flags are initially zero.
Extension tag 1 is `early_data`, at most the family-advertised limit and never
more than 64 KiB. It is written only after the upstream link is established;
on any connect or write failure the Request fails and none of its bytes are
reported as accepted. A client normally puts the complete YAS preface and
Core HELLO Request there, saving an inner startup flight.

After the upstream is connected and any early data is accepted, the OK Result
body is:

```text
[relay_handle:u64][route_handle:u64][generation:u64]
[transfer_descriptor]
```

`relay_handle` is a nonzero session-scoped `u64`. The descriptor is a
bidirectional sensitive BYTE Transfer whose content type is
`yas.relay.tunnel/1` (`content_family = 0x0002`, `content_kind = 0x0000`,
`content_version = 1`). `sender_send_credit` does not exceed the CONNECT
`initial_receive_credit`; the server independently grants
`receiver_send_credit` for client-to-upstream bytes. Transfer chunks are
arbitrary stream segments and never imply an inner frame boundary.

Without `early_data`, the first client-to-upstream bytes are the eight-byte YAS
preface followed by a length-prefixed Core HELLO Request. The upstream's first
bytes are the length-prefixed HELLO Result. A relayed link always uses YAS byte
stream framing even when the outer link is WebSocket or another message
transport. It has no associated native datagram path, so the nested HELLO
advertises `receive_max_datagram = 0`. Every other limit and family is
negotiated independently with the nested server.

The relay forwards bytes and half-closes without parsing inner frames. It does
not merge catalogues, translate handles, filter families, impersonate Results,
or reuse one inner session for multiple clients. Multiple CONNECTs to the same
route create independent upstream links. A nested server may expose Relay in
its own HELLO, which permits deliberate recursion without giving the home
server special knowledge of the next catalogue.

DISCONNECT payload is:

```text
[relay_handle:u64][reason_len:u32][reason:N]
```

Its Result is queued before the relay resets the Transfer and closes the
upstream link. Resetting the Transfer has the same abortive effect without a
correlated Result. An orderly client instead sends inner Core GOAWAY if
applicable and half-closes its Transfer direction; upstream half-close maps to
the opposite Transfer CLOSE. I/O failure resets the Transfer with IO detail.
Loss of the outer session closes every relay it owns; relay handles and
Transfers never resume after reconnect.

Relay family limits advertise at least maximum routes, simultaneous links per
session, pending CONNECTs, early-data bytes, connect timeout, and buffered
bytes per link. Buffered tunnel data consumes the same aggregate receive
budget as every other Transfer. A client applies its own maximum recursion
depth and visited-server policy; protocol resource limits bound an accidental
or malicious route cycle even when it ignores that policy.

The ordered Relay limit extensions and canonical hard ceilings are:

| Tag | Limit                   | Type  | Hard maximum       |
| --: | ----------------------- | ----- | ------------------ |
|   1 | max routes              | `u32` | 65,536             |
|   2 | links per session       | `u32` | 4,096              |
|   3 | pending CONNECTs        | `u32` | 1,024              |
|   4 | early-data bytes        | `u32` | 65,536             |
|   5 | connect timeout         | `u64` | 300,000,000,000 ns |
|   6 | buffered bytes per link | `u64` | 1,073,741,824      |

All except early-data bytes are nonzero. Pending CONNECTs cannot exceed links
per session. Early-data bytes may be zero to disable the extension.

## Terminal family

Terminal is family `0x0010`, version 1. This section defines its operation,
state, and `yas.terminal.grid/1` wire shape. The generated schema and golden
vectors repeat these layouts; they do not fill in wire details omitted here.

### Kinds

| Class   |     Kind | Name           |
| ------- | -------: | -------------- |
| Request | `0x0000` | WATCH          |
| Request | `0x0001` | UNWATCH        |
| Request | `0x0002` | CREATE         |
| Request | `0x0003` | RESTART        |
| Request | `0x0004` | SIGNAL         |
| Request | `0x0005` | CLOSE          |
| Request | `0x0006` | SET_DEADLINE   |
| Request | `0x0007` | RESIZE         |
| Request | `0x0008` | SET_FOCUS      |
| Request | `0x0009` | SCROLL         |
| Request | `0x000a` | OPEN_VIEW      |
| Request | `0x000b` | CONFIGURE_VIEW |
| Request | `0x000c` | RESET_VIEW     |
| Request | `0x000d` | READ           |
| Request | `0x000e` | SEARCH         |
| Request | `0x000f` | CWD            |
| Request | `0x0010` | JOURNAL        |
| Request | `0x0011` | OUTPUT         |
| Request | `0x0012` | WAIT           |
| Request | `0x0013` | COPY_RANGE     |
| Request | `0x0014` | CLOSE_VIEW     |
| Request | `0x0015` | SEARCH_CATALOG |
| Event   | `0x0000` | STATE          |
| Event   | `0x0001` | STATE_ACK      |
| Event   | `0x0010` | WRITE          |
| Event   | `0x0011` | INPUT          |
| Event   | `0x0012` | MOUSE          |
| Event   | `0x0013` | WHEEL          |
| Event   | `0x0020` | FRAME          |
| Event   | `0x0021` | FRAME_CHUNK    |
| Event   | `0x0022` | FRAME_ACK      |

WATCH, STATE, and STATE_ACK use the common state convention. UNWATCH payload is
`[subscription_id:u32]` and returns OK even when the subscription already
closed, making cleanup idempotent.

Terminal family limits are ordered optional extensions, all present in a
selected family descriptor. Tags 1 through 8 are respectively maximum
terminals per session, views per session, view rows, view columns, input bytes,
inline query bytes, query records, and hyperlink URI bytes. Every value is a
nonzero little-endian `u32` no larger than the canonical hard maximum.

### Terminal state records

Terminal STATE records have:

```text
[record_len:u32][record_kind:u16][record_flags:u16]
[terminal_handle:u64][body:N]
```

| Kind | Meaning                             |
| ---: | ----------------------------------- |
|    0 | ADD: complete terminal state        |
|    1 | REPLACE: complete terminal state    |
|    2 | PATCH: named fields change          |
|    3 | REMOVE: terminal no longer retained |

ADD and REPLACE body:

```text
[lifecycle:u8][reserved:u8=0][rows:u16][cols:u16]
[generation:u32][used_rows:u32]
[extensions_len:u32][extensions:N]
```

PATCH body is only an extension set. Extension presence means replace that
field; a zero-length string is therefore distinct from an absent change.

Initial state extension tags are:

| Tag | Name                 | Type                     |
| --: | -------------------- | ------------------------ |
|   1 | `title`              | UTF-8                    |
|   2 | `cwd`                | path bytes               |
|   3 | `command_display`    | UTF-8, presentation only |
|   4 | `exit`               | exit record              |
|   5 | `deadline_server_ns` | `u64`                    |
|   6 | `app_handle`         | `u64`                    |
|   7 | `journal_cursor`     | `u64`                    |
|   8 | `resource_tag`       | UTF-8                    |

Lifecycle values are 0 RUNNING and 1 EXITED. Exit records distinguish an exit
code, a portable termination reason, and optional platform detail. REMOVE is
used only after retention ends; EXITED terminals remain addressable until then.
Generation starts at one and increments on every successful RESTART. Journal,
output, exit, and lifecycle records carry it whenever ambiguity across restarts
would otherwise be possible.

The native server projection caps `title` and presentation-only
`command_display` at 65,536 UTF-8 bytes, `cwd` at 65,535 UTF-8 bytes, and
`resource_tag` at 4,096 UTF-8 bytes. It truncates only at a UTF-8 boundary.
These are State publication limits, not permission to reinterpret or replay a
truncated command. Before a Terminal WATCH returns OK, the server reserves the
actual aggregate-clamped State window and verifies that snapshot markers,
every current record, and a record carrying every maximum published extension
fit that window and the peer frame/decoded-frame limits. Failure returns
RESOURCE_EXHAUSTED without publishing a subscription or retaining the lease.

### Terminal launch specification

CREATE and RESTART use the same complete launch record:

```text
[command_kind:u8][cwd_kind:u8][environment_base:u8][reserved:u8=0]
[command_body:variant][cwd_body:variant]
[environment_entry_count:u16] repeated{
  [key_len:u16][key:N][entry_kind:u8]
  [value_len:u32][value:M]
}
[extensions_len:u32][extensions:N]
```

Command variants are:

| Kind | Name          | Body                                             |
| ---: | ------------- | ------------------------------------------------ |
|    0 | DEFAULT_SHELL | Empty; use the server's configured default shell |
|    1 | ARGV          | `[argc:u16] repeated{ [arg_len:u32][arg:N] }`    |
|    2 | SHELL_COMMAND | `[command_len:u32][command_utf8:N]`              |

ARGV is executed directly without a shell. It has at least one entry, preserves
empty entries, and otherwise carries exact platform argv bytes. SHELL_COMMAND
is passed as one UTF-8 command to the server's configured platform shell; it is
not split or re-quoted by YAS.

Cwd variants are:

| Kind | Name           | Body                     |
| ---: | -------------- | ------------------------ |
|    0 | SERVER_DEFAULT | Empty                    |
|    1 | PATH           | `[path_len:u32][path:N]` |
|    2 | TERMINAL       | `[source_terminal:u64]`  |

TERMINAL snapshots the source terminal's cwd when the Request is admitted. PATH
uses the server platform's advertised path model. An inaccessible, missing, or
wrong-platform path fails the Request; it never silently falls back.

Environment base 0 is the server environment plus the live session's display,
desktop-bus, and audio variables; base 1 is empty. Entry kind 0 SET assigns the
supplied value. Entry kind 1 REMOVE requires `value_len == 0` and removes the
key from the selected base. Entries are unique and sorted by the server
platform's environment-key ordering, and are applied after the session
overlay. Base EMPTY plus SET entries constructs an exact environment,
including the empty environment. The server does not inject `TERM`, a shell,
`PATH`, locale, or YAS variables implicitly.
If an `app_handle` is requested below, its documented application-endpoint
overlay is applied after these entries; omit `app_handle` when the environment
must remain byte-for-byte exact.

Launch extensions are:

| Tag | Name                | Type  |
| --: | ------------------- | ----- |
|   1 | `deadline_after_ns` | `u64` |
|   2 | `app_handle`        | `u64` |

The complete launch record is stored on the terminal resource after a
successful CREATE or replacing RESTART. It is the source of truth for later
replay; `command_display` is never parsed back into a command. CREATE and
RESTART are SENSITIVE frames because the record may contain command and
environment bytes.

### CREATE

CREATE Request payload:

```text
[rows:u16][cols:u16][reserved:u32=0][operation_id:16]
[launch_len:u32][launch:launch_len]
[extensions_len:u32][extensions:N]
```

| Tag | Name           | Type                |
| --: | -------------- | ------------------- |
|   1 | `initial_view` | view request record |

The launch record is required: defaults are represented by its explicit
DEFAULT_SHELL, SERVER_DEFAULT, and SERVER environment variants. There is no
NUL-split spelling, positional option mask, or implicit subscription.
`initial_view` is explicit and atomic with creation.

Successful body:

```text
[terminal_handle:u64][state_revision:u64]
[generation:u32][reserved:u32=0]
[extensions_len:u32][extensions:N]
```

Result extension tag 1 carries the initial-view result when requested. Every
accepted CREATE returns success or a failure status; refusal never hangs.
This extension is an ephemeral result-object exception to generic mutation
replay: the first successful Result atomically publishes the initial view, but
every same-fingerprint duplicate returns STALE and an empty body without
creating another terminal or view. Server-side view liveness is insufficient
to replay the descriptor because it cannot prove that the client retained and
installed the corresponding decoder object. CREATE without `initial_view`
continues to replay its cached Result normally.

### Lifecycle and control

RESTART payload is:

```text
[terminal_handle:u64][operation_id:16]
[launch_mode:u8][cutover_mode:u8][reserved:u16=0]
if launch_mode == REPLACE: [launch_len:u32][launch:launch_len]
[extensions_len:u32][extensions:N]
```

Launch mode 0 REPLAY starts the exact stored launch record again. Launch mode 1
REPLACE uses the supplied complete launch record and, on success, stores it for
future REPLAY operations. REPLACE does not merge with the old command, cwd,
environment, deadline, or application association. It can therefore restart a
terminal with any argv or shell command, any valid cwd, and any environment the
server OS can represent. REPLAY has no launch bytes; REPLACE requires them.

RESTART is valid for RUNNING and EXITED terminals. For an EXITED terminal both
cutover modes simply start the new generation. For a RUNNING terminal the
caller chooses:

| Mode | Name              | Semantics                                                                                                                                                   |
| ---: | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
|    0 | STOP_THEN_START   | Stop and drain the old PTY generation before launch, allowing the new command to acquire its resources; launch failure leaves the terminal EXITED           |
|    1 | START_THEN_SWITCH | Start on a private PTY and buffer output before committing; launch failure leaves the old generation running, but old and new processes may overlap briefly |

The server validates the complete launch record before either cutover.
STOP_THEN_START is the ordinary process-restart semantic; START_THEN_SWITCH is
for callers that prefer rollback safety and know the commands may overlap. A
STOP_THEN_START launch failure returns IO, leaves the stored launch record
unchanged, and exposes the stopped generation's final state through WATCH. A
START_THEN_SWITCH launch failure changes neither generation nor stored launch
record.

On success, the server increments generation, stores a REPLACE launch record,
rebinds the terminal handle and existing views, resets the grid and scrollback,
emits a terminal REPLACE state record and view keyframes, and retires the old
PTY generation. Buffered new-generation output is applied only after the reset.
Unwritten old-generation frames are discarded; frames already on the reliable
link precede the new keyframe. Old-generation output cannot enter the new grid
after the commit boundary, but remains available in generation-tagged journal
history. The Result body is:

```text
[state_revision:u64][generation:u32][reserved:u32=0]
```

SIGNAL is:

```text
[terminal_handle:u64][operation_id:16][signal:u16][reserved:u16=0]
[extensions_len:u32][extensions:N]
```

Portable values are INTERRUPT, TERMINATE, KILL, and HANGUP. A platform-native
signal is an explicit extension allowed only when Terminal family limits
advertise its platform and representation.

CLOSE is `[terminal_handle:u64][operation_id:16]`. It is idempotent: retrying
the operation ID returns the original outcome. It closes the terminal resource
according to server retention policy; watchers observe EXITED and later REMOVE.

SET_DEADLINE is:

```text
[terminal_handle:u64][operation_id:16]
[mode:u8][reserved:7=0][duration_ns:u64]
```

Mode 0 clears the deadline and mode 1 sets it relative to request admission.

RESIZE is `[terminal_handle:u64][rows:u16][cols:u16]`. Multiple resizes may be
sent as concurrent Requests; server processing order determines the final
size, and each Result returns the resulting state revision.

SET_FOCUS is `[view_id:u32][focused:u8][reserved:3=0]`. Focus is
viewer-specific and has no subscription or keyframe side effect.

SCROLL is:

```text
[view_id:u32][mode:u8][reserved:7=0][amount:i64]
```

Mode 0 applies an absolute line offset and mode 1 applies a relative delta to
the server's current offset. The Result contains the applied `i64` offset.
Relative scroll is safe without an operation ID because the view is
session-scoped: a live session resolves the Request exactly once, while a lost
session destroys the view and makes retry impossible.

### Views and terminal frames

OPEN_VIEW Request:

```text
[terminal_handle:u64][rows:u16][cols:u16][max_fps:u16]
[codec_count:u8][reserved:u8=0] repeated{ [codec_version:u16] }
[extensions_len:u32][extensions:N]
```

Successful body:

```text
[view_id:u32][codec_version:u16][max_inflight_frames:u8][reserved:u8=0]
[max_encoded_frame:u32][max_decoded_frame:u32][first_sequence:u32]
[extensions_len:u32][extensions:N]
```

View IDs are server-allocated nonzero `u32` values scoped by session. Opening a
view subscribes only that view to frames; terminal state WATCH is independent.

Across all open views, the server chooses frame counts and decoded-frame limits
whose worst-case buffered total fits the client's remaining
`receive_max_buffered` budget. `max_encoded_frame` caps the reassembled logical
body; `max_decoded_frame` caps the grid bytes after codec-local decompression.
The server reserves the complete advertised window before allocating and
publishing a view. Terminal version 1 reserves one `max_decoded_frame` window
for each open view. It releases that reservation only after the view is retired
and every previously queued reliable frame has crossed its established
lifetime boundary, or when the session is torn down.

These limits SHOULD be derived from the view's own geometry rather than set to
the session-wide `receive_max_decoded`. A client reserves the declared window
for the whole life of the view, so declaring the session ceiling charges a
24x80 preview the same buffer as a full-screen view and exhausts the aggregate
budget after a handful of views. A server that declares a geometry-derived
bound MUST also encode within it: optional grid components are dropped before
a frame is allowed to exceed the declared limit.

A view's declared limits are fixed for its lifetime -- the CONFIGURE_VIEW
Result carries no revision of them. A server MUST therefore answer
RESOURCE_EXHAUSTED, leaving the view unchanged, when a requested geometry
would need more than the bound the view declared. A client that wants the
larger geometry closes the view and opens a new one, whose declaration is
sized for it.

The successful OPEN_VIEW Result is a write barrier: the server does not make
the view's FRAME producer observable until the Result has been written. A
CREATE carrying an initial-view extension applies the same barrier to its
successful Result. This ordering applies across the Control and Data
scheduler classes, not merely within either class.

CONFIGURE_VIEW begins `[view_id:u32]` and uses extensions for rows, columns,
maximum FPS, presentation metrics, and queue target. RESET_VIEW is
`[view_id:u32]` and requests an explicit keyframe. Repeating OPEN_VIEW or
CONFIGURE_VIEW never implicitly requests one.

Frame sequences are `u32` serial numbers scoped by a view. Serial comparisons
use modulo-2^32 arithmetic with a window smaller than 2^31;
`max_inflight_frames` is at most 255. Before any frame is presented, feedback
uses `first_sequence - 1` modulo 2^32. Explicit bases and the bounded window
keep wrapping unambiguous. Sequence numbers are assigned only to emitted
logical frames and advance consecutively.

An unchunked FRAME Event is:

```text
[view_id:u32][frame_sequence:u32]
[frame_flags:u16]
if EXPLICIT_BASE: [base_sequence:u32]
[grid_payload:N]
```

`frame_flags` are:

| Bit | Name             | Additional decoded grid field                   |
| --: | ---------------- | ----------------------------------------------- |
|   0 | KEYFRAME         | Decode from blank state                         |
|   1 | FINAL_STATE      | No later frame until restart                    |
|   2 | DIMENSIONS       | `[rows:u16][cols:u16]`                          |
|   3 | CURSOR           | `[cursor_row:u16][cursor_col:u16]`              |
|   4 | MODES            | `[modes:u16]`                                   |
|   5 | SCROLLBACK       | `[scrollback_lines:u32]`                        |
|   6 | VIEW_OFFSET      | `[scroll_offset:i64]`                           |
|   7 | TITLE            | `[title_len:u16][title_utf8:N]`                 |
|   8 | COMPONENTS       | Component stream follows the cell operations    |
|   9 | CODEC_COMPRESSED | Grid payload uses the codec-local LZ4 wrapper   |
|  10 | EXPLICIT_BASE    | `[base_sequence:u32]` precedes the grid payload |

Bits 11 through 15 are zero. A delta without EXPLICIT_BASE uses
`frame_sequence - 1` modulo 2^32 as its base. EXPLICIT_BASE names another exact
decoded base and is invalid with KEYFRAME. Fields absent from `frame_flags`
retain their base values. DIMENSIONS is legal only on a keyframe. A keyframe
initializes a blank grid and default optional state before applying its fields
and operations; it requires DIMENSIONS, CURSOR, MODES, SCROLLBACK, VIEW_OFFSET,
and TITLE. Components absent from a keyframe are reset to their defaults.

Without CODEC_COMPRESSED, `grid_payload` is the decoded grid bytes directly.
With it, the payload is:

```text
[decoded_grid_len:u32][lz4_block:N]
```

The decoded length is checked against `max_decoded_frame` and the aggregate
receive budget before allocation. CODEC_COMPRESSED MUST NOT be used unless its
four-byte length plus LZ4 block is at least eight bytes smaller than the raw
grid payload. The outer COMPRESSED meta bit remains forbidden on FRAME and
FRAME_CHUNK; compression here is visible to terminal frame accounting.

The decoded grid begins with the fields selected by `frame_flags`, in the bit
order above, followed by:

```text
[operation_count:uleb128] repeated{ [opcode:u8][operation_body] }
if COMPONENTS:
  [component_count:uleb128] repeated{
    [component_kind:u8][component_flags:u8]
    [component_len:uleb128][component_body:N]
  }
```

ULEB128 values are canonical `u32` encodings: at most five bytes, no redundant
high zero group. Cell indices are row-major. Codec 1 defines these operations:

| Opcode | Name         | Body                                                                                |
| -----: | ------------ | ----------------------------------------------------------------------------------- |
| `0x00` | PATCH_RUN    | `[start_cell:uleb128][cell_count:uleb128][cells:12*count]`                          |
| `0x01` | PATCH_LIST   | `[cell_count:uleb128][first_cell:uleb128][positive_deltas:count-1][cells:12*count]` |
| `0x02` | PATCH_BITMAP | `[start_cell:uleb128][span:uleb128][bitmap:ceil(span/8)][cells:12*popcount]`        |
| `0x03` | COPY_RECT    | `[src_row:u16][src_col:u16][dst_row:u16][dst_col:u16][rows:u16][cols:u16]`          |
| `0x04` | FILL_RECT    | `[row:u16][col:u16][rows:u16][cols:u16][cell:12]`                                   |

PATCH_LIST indices are strictly increasing; each delta is from the preceding
index. PATCH_BITMAP bit `i` names cell `start_cell + i`; it begins and ends on a
changed cell, so bit zero and bit `span-1` MUST be set. It can therefore never
carry a bitmap covering unchanged grid prefixes or suffixes. Patch counts and
spans are nonzero, indices and rectangles are in bounds, and rectangle extents
are nonzero. Violating any of those rules is a codec error. For all PATCH
operations, the cell bytes are transposed by byte plane: byte 0 of every cell,
then byte 1 of every cell, and so on. Operations apply in encoded order.

Each cell is exactly 12 bytes, retaining the compact renderer-facing form:

```text
byte 0: fg_type[2] | bg_type[2] | bold | dim | italic | underline
byte 1: inverse | wide | wide_continuation | content_len[3] | link
bytes 2..4: foreground RGB or palette index
bytes 5..7: background RGB or palette index
bytes 8..11: UTF-8 content up to four bytes, or overflow-string hash
```

An encoder chooses PATCH_RUN for contiguous changes, PATCH_LIST for sparse
changes, and PATCH_BITMAP for a sufficiently dense bounded span. For the same
dirty set, its patch encoding MUST be no larger than both one PATCH_LIST and
one cropped PATCH_BITMAP encoding. COPY_RECT and FILL_RECT may replace patches
when smaller. This rule prevents frame cost from scaling with the entire grid
when one cell changes.

Components carry infrequent state that should not tax the common delta.
They are unique and ordered by kind. Component flag bit 0 is REQUIRED; other
flags are zero. Codec 1 defines:

|   Kind | Name             | Body and replacement rule                                                                                                                                                                                          |
| -----: | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `0x00` | LINE_FLAGS       | `[run_count:uleb128] repeated{ [start_row:uleb128][row_count:uleb128][flags:u8] }`; replaces all row flags, with unspecified rows zero                                                                             |
| `0x01` | OVERFLOW_STRINGS | `[entry_count:uleb128] repeated{ [cell_index:uleb128][utf8_len:uleb128][utf8:N] }`; supplies strings for cells patched to overflow content                                                                         |
| `0x02` | HYPERLINKS       | `[uri_count:uleb128] repeated{ [link_id:uleb128][uri_len:uleb128][uri:N] }[run_count:uleb128] repeated{ [start_cell:uleb128][cell_count:uleb128][link_id:uleb128] }`; replaces the complete URI table and cell map |

LINE_FLAGS runs are ordered, nonoverlapping, nonzero, and in bounds.
OVERFLOW_STRINGS indices are strictly increasing; every patched cell whose
content length is the overflow marker has exactly one entry, and patching a
cell first removes its previous overflow entry. COPY_RECT copies overflow and
hyperlink association with its cells; FILL_RECT clears both in its destination.
Whenever hyperlink association changes by means other than those operations,
HYPERLINKS is present and replaces it completely. URI bytes are UTF-8 and
limited to 4096 bytes each. Hyperlink IDs are nonzero and unique, runs are
ordered and nonoverlapping, and after applying the frame every link-marked cell
belongs to exactly one run while no unmarked cell belongs to one.

Unknown optional components are skipped and an unknown required component is a
codec error. Dynamic palettes, image placement, a new required component, or a
new cell interpretation require a new negotiated grid codec version; they are
not speculative overhead in codec 1.

### Fragmented terminal frames

The logical body of a FRAME is the bytes beginning with `frame_flags` and
ending with `grid_payload`. If it does not fit the canonical bulk chunk limit,
the server sends FRAME_CHUNK Events instead of FRAME:

```text
[view_id:u32][frame_sequence:u32]
[chunk_index:u16][chunk_count:u16][logical_frame_len:u32][chunk:N]
```

`logical_frame_len` is at most `max_encoded_frame`. Chunks are nonempty,
ordered slices no larger than the bulk chunk limit; every chunk repeats the
same view, sequence, count, and logical length. Frames for different views may
interleave. The receiver reserves aggregate budget, reassembles exactly
`chunk_count` slices, and then decodes the resulting logical body as though it
had arrived in one FRAME. Chunk coordinates and lengths never appear on the
ordinary unchunked path.

### Frame feedback

View input and FRAME_ACK share this ten-byte feedback prefix:

```text
[view_id:u32][presented_sequence:u32]
[decoder_queue_depth:u8][available_frame_slots:u8]
```

Feedback is cumulative. Before presenting any frame, `presented_sequence` is
`first_sequence - 1` modulo 2^32. Queue depth and available slots are exact
because a view has at most 255 in-flight frames. Stale feedback is ignored. The server sends no more than
`max_inflight_frames` logical frames beyond the highest presented sequence and
also obeys the most recent available-slot report.

INPUT, MOUSE, and WHEEL carry the prefix, so active interaction normally
acknowledges the preceding frame without a separate Event. FRAME_ACK contains
only the prefix. A client sends FRAME_ACK when feedback advances and no view
input is ready before useful frame credit would otherwise be withheld. It does
not manufacture an ACK for every frame when a cumulative or piggybacked update
already reports the same state.

### Input Events

WRITE performs view-independent, exact PTY input for automation and headless
clients:

```text
[terminal_handle:u64][data:N]
```

INPUT performs the same byte write through an interactive view and reports
that view's frame feedback:

```text
[view_feedback:10][data:N]
```

`data` is nonempty and limited to 16 KiB per Event. The client terminal encoder
maps keys, committed text, paste, and input-method results to bytes using the
negotiated terminal modes. The server does not reinterpret keyboard layouts or
text encodings. WRITE and INPUT differ only in view association and feedback;
their PTY writes are identical. Processing INPUT first applies its feedback,
then resets that view's scroll offset to zero, then writes the bytes. WRITE
does not change any view's offset.

MOUSE payload:

```text
[view_feedback:10][client_monotonic_ns:u64]
[action:u8][button:u8][modifiers:u16]
[column:i32][row:i32]
```

Actions, buttons, and modifiers are stable YAS terminal enums. The server
encodes recognized events according to the terminal's active mouse-reporting
mode. Unknown values are dropped and counted; they never become a left click.

WHEEL payload:

```text
[view_feedback:10][client_monotonic_ns:u64]
[source:u8][reserved:3=0][dx_32_32:i64][dy_32_32:i64]
```

Source distinguishes wheel, finger, continuous, and unknown. Values are
logical cells in signed 32.32 fixed point. The server encodes the event only
when a terminal mouse-reporting mode accepts it; otherwise it drops it. Clients
use the SCROLL Request for view scrolling.

### Character-update byte budget

The common case is normative enough to budget exactly. For one changed ASCII
cell at an index of at least 128 and one cursor move in an 80 by 24 view, with
no other state change:

| Part                                                    |  Bytes |
| ------------------------------------------------------- | -----: |
| YAS Event header                                        |      5 |
| View, frame sequence, flags                             |     10 |
| Cursor row and column                                   |      4 |
| Operation count                                         |      1 |
| PATCH_RUN opcode, two-byte worst-case cell index, count |      4 |
| Cell                                                    |     12 |
| **Server FRAME**                                        | **36** |

The corresponding 36-byte golden frame uses view 1, sequence 2, implicit base
1, cursor `(23, 79)`, and writes `x` with default style to cell 1918:

```text
10 00 20 00 00                       # Terminal, FRAME, Event
01 00 00 00  02 00 00 00  08 00     # view, sequence, CURSOR
17 00 4f 00                           # cursor
01 00 fe 0e 01                        # one PATCH_RUN, cell 1918, count 1
00 08 00 00 00 00 00 00 78 00 00 00 # 12-byte cell
```

An interactive one-byte INPUT with feedback is 16 bytes. In a typing stream,
each INPUT acknowledges the preceding frame, so the steady-state protocol cost
is 52 bytes per character. On a local byte stream the two length prefixes make
that 60 bytes; WebSocket framing also makes it 60 bytes (22 client-to-server
and 38 server-to-client). After the final character, a standalone FRAME_ACK is
15 protocol bytes only if no subsequent view input carries the feedback. Thus
one isolated character, all feedback included, is 67 protocol bytes, 79 bytes
on a local byte stream, or 81 bytes through WebSocket framing. A run of `N`
characters is `52N + 15` protocol bytes.

The byte-budget golden vector is part of codec conformance. A codec-1 encoder
that emits more than 36 YAS bytes for this exact case is nonconforming.

The engineering baseline is the previous 0.55.1 encoder: it emits 53 protocol bytes
for the same 80 by 24 update, and INPUT + update + ACK costs 58 bytes. YAS must
retain the 36-byte server-frame and 52-byte steady-state typing bounds in CI;
payload flexibility is not permission to regress the hot path.

### Queries

READ, SEARCH, CWD, JOURNAL, OUTPUT, WAIT, and COPY_RANGE are correlated
Requests. Their operation-specific prefixes use explicit cursor variants and
never magic `u64::MAX` values. COPY_RANGE takes exact grid endpoints and
returns plain text, styled records, or both through the same bounded result
representation as READ. Every one of these Requests carries
`initial_receive_credit:u64` immediately before its trailing Extensions. Zero
forces an inline Result; a returned query Transfer's `sender_send_credit` may
not exceed the proposal. CWD has no independent maximum-length field, so its
successful PATH bytes, including inline delivery, must fit
`initial_receive_credit`; otherwise the server returns RESOURCE_EXHAUSTED
without creating a Transfer or sending an OK Result.

READ cursor kind 0 is ABSOLUTE: `cursor_a` is an oldest-retained-relative row
and `cursor_b` is the maximum row count, with zero meaning through the end.
Kind 1 is TAIL: `cursor_a` is the number of rows skipped back from
one-past-tail and `cursor_b` is the maximum row count, with zero meaning
through the oldest retained row. READ flags are zero. Its next cursor is
`[kind:u8][a:u64][b:u32]` and preserves the chosen kind.

SEARCH flags are REGEX bit 0, CASE_SENSITIVE bit 1, and BACKWARD bit 2. Its
start cursor is `[kind:u8=SEARCH_CURSOR_POSITION][row:u64][column:u32]`, where
the row is oldest-retained-relative and the coordinate is inclusive. Search
matches and the next cursor use the same coordinate space, so a page can end
mid-row without skipping matches. A search range is end-exclusive.

SEARCH_CATALOG is the global terminal catalogue search. Its request carries a
nonzero bounded result count and a UTF-8 query of at most 1024 bytes. The server
trims the query and applies it as a case-insensitive regular expression to every
terminal title, visible grid, and scrollback; an empty or invalid expression
returns an empty result. Each result names the terminal handle and generation,
score, primary match source, complete matched-source bit mask, optional
oldest-retained-relative scroll offset, and at most 4096 bytes of UTF-8 context.
Results are sorted by descending score, with the lead terminal first on a tie
and terminal handle as the final ascending tie-break. The server returns the
deterministic prefix that fits both `max_results` and the negotiated frame
limit, setting `CATALOG_SEARCH_TRUNCATED` when more matches exist.

JOURNAL flag bit 0 is TAIL. Without it, `from_index` is an absolute command
index; with it, `from_index` is the number of records skipped back from
`next_index`. OUTPUT cursor kinds are COMMAND 0, LATEST_COMMAND 1, SEQUENCE 2,
and PROBE 3. COMMAND uses `a=command_index,b=column`, LATEST_COMMAND requires
`a=0`, and SEQUENCE/PROBE use `a=sequence,b=column`; OUTPUT flags are zero.
Every OUTPUT next cursor is normalized to SEQUENCE with `a=next_seq` and
`b=next_col`, including COMMAND, LATEST_COMMAND, and PROBE results.

WAIT kinds are OUTPUT 0, COMMAND 1, and LATEST_COMMAND 2, with zero flags.
OUTPUT uses `a=sequence,b=column`, requires a nonempty needle, and returns an
OutputResult. COMMAND uses `a=command_index,b=0` and an empty needle;
LATEST_COMMAND requires `a=b=0` and an empty needle. Both command waits return
a JournalResult containing exactly one command record. Timeouts and byte
limits are nonzero.

COPY_RANGE rows are inclusive, oldest-retained-relative when nonnegative, and
relative to one-past-tail when negative; `end_col` is exclusive. The server
resolves both rows against one retention snapshot, rejects a reversed resolved
range, clamps only endpoints outside the retained history, and sets TRUNCATED
when it clamps. For a one-row range `start_col` may not exceed `end_col`.

Potentially large successful Results use this common Terminal query body:

```text
[delivery:u8][content_kind:u8][encoding:u8][reserved:u8=0]
[flags:u16][reserved:u16=0]
[extensions_len:u32][extensions:N]
```

Delivery 0 carries required extension tag 1 `inline_bytes`; delivery 1 carries
required tag 2 with a BYTE, sender-to-receiver, sensitive Transfer descriptor
whose content family is Terminal, content kind is `QUERY_CONTENT_KIND` 0, and
content version is 1. Inline data is limited to 32 KiB. Optional tag 3 carries
the content-specific next cursor; absence means there is none. Optional tag 4
is `total_lines:u64`, and tag 5 is the nonzero state revision that satisfied a
WAIT. QueryBody flag bit 0 is TRUNCATED; all other bits are reserved.

Content kinds fix both the inline/Transfer bytes and encoding:

| Kind | Name            | Encoding         | Bytes / next cursor                                               |
| ---: | --------------- | ---------------- | ----------------------------------------------------------------- |
|    0 | TEXT            | UTF8             | UTF-8; READ QueryCursor                                           |
|    1 | PATH            | BYTES            | raw path bytes; no cursor                                         |
|    2 | STYLED_LINES    | TERMINAL_RECORDS | StyledLines; READ QueryCursor                                     |
|    3 | SEARCH_RESULTS  | TERMINAL_RECORDS | SearchResults; SEARCH QueryCursor                                 |
|    4 | JOURNAL         | TERMINAL_RECORDS | JournalResult; next command index `u64`                           |
|    5 | OUTPUT          | TERMINAL_RECORDS | OutputResult; normalized SEQUENCE QueryCursor                     |
|    6 | TEXT_AND_STYLED | TERMINAL_RECORDS | `plain:bytes_u32` UTF-8 then `styled:bytes_u32`; READ QueryCursor |

SearchResults is a `count:u32` followed by
`start_row:u64,start_col:u32,end_row:u64,end_col:u32,preview:string_u32`
records. StyledLines is a `line_count:u32` followed by each
`row:i64,start_col:u32,cell_count:u32`, the cell-major 12-byte cells, ordered
overflow records (`cell_offset:u32,text:string_u32`), and ordered hyperlink
records (`start_col:u32,cell_count:u32,uri:string_u16`). Styled hyperlink
columns and each line's `start_col` are absolute original grid columns;
overflow offsets are relative to the returned cell slice.

JournalResult is
`oldest_index:u64,next_index:u64,count:u32` followed by command records:
`index:u64,generation:u32,flags:u16,reserved:u16=0,exit_code:i32,start_seq:u64,end_seq:u64,started_unix_ms:u64,ended_unix_ms:u64,command:string_u32`.
Its flags are RUNNING, HAS_EXIT, NO_COMMAND, INCOMPLETE, EVICTED, and
PTY_EXITED. OutputResult is
`generation:u32,flags:u16,reserved:u16=0,start_seq:u64,start_col:u32,next_seq:u64,next_col:u32,text:bytes_u32`;
its flags are TRUNCATED, EVICTED, ALT_SCREEN, and MATCHED. Query failure is the
common Result status, never an empty payload with overloaded meaning.

WAIT accepts a relative timeout and returns the terminal state revision that
satisfied it. CWD returns path bytes. Journal and output return typed records;
their exact required layouts are generated from the Terminal schema.

## Client family

Client is family `0x0011`, version 1. It replaces CLIENT_LIST, CLIENT_LIST2,
CLIENT_WATCH, CLIENT_UNWATCH, KICK, and the origin feature bit with one watched
catalogue and one correlated control Request.

| Class   |     Kind | Name       |
| ------- | -------: | ---------- |
| Request | `0x0000` | WATCH      |
| Request | `0x0001` | UNWATCH    |
| Request | `0x0002` | DISCONNECT |
| Event   | `0x0000` | STATE      |
| Event   | `0x0001` | STATE_ACK  |

Client records are keyed by the 16-byte `session_id` and contain instance ID,
name, release, label, origin, connected time, idle time, and sampled traffic.
Origin is a length-delimited typed record, so Unix, SSH, edge, Relay, WebRTC,
extension, and future transports do not require another list shape. The first
snapshot includes the caller.

The required `bytes_received` and `bytes_sent` fields are cumulative session
counters. Optional ClientRecord/ClientPatch extension tag 2 carries current
sampled bandwidth as exactly
`received_bytes_per_second:u64,sent_bytes_per_second:u64,sample_window_ns:u64`;
the sample window is nonzero. Consumers use these rates directly and do not
synthesize a lifetime average from cumulative counters.

Optional ClientRecord/ClientPatch extension tag 1 is the connection's live
active-subscription snapshot. It starts with
`terminal_count:u16,surface_count:u16,auxiliary_count:u16,reserved:u16=0`, then
terminal entries
`terminal_handle:u64,view_id:u32,rows:u16,columns:u16`, Surface entries
`surface_handle:u64,view_id:u32,width:u32,height:u32,scale_120:u16,reserved:u16=0`,
and auxiliary State-watch entries
`family:u16,reserved:u16=0,subscription_id:u32,resource_handle:u64`. Native
sessions publish this extension even when they did not select Client. Terminal
and Surface entries appear only after successful view creation and disappear
after close, natural retirement, or session cleanup; auxiliary entries follow
successful WATCH/UNWATCH and cleanup. FS, KV, Git, Git-query, and LSP entries
carry their concrete resource handle; catalogue-wide families use zero. Native
Surface OPEN_VIEW/CONFIGURE_VIEW dimensions have no separate scale input and
are represented as the server's 1x interpretation, `scale_120=120`.

Each section is strictly sorted by its wire key. The family limit is the
maximum represented, not a limit on live resources: if the combined live total
exceeds it, the server keeps the deterministic sorted prefix in wire section
order (Terminal, then Surface, then auxiliary). A catalogue refresh observes
the snapshot transition as an ordinary Client record replacement. The active
snapshot does not replace or suppress the independent bandwidth extension.

Client family-limit tags 1 and 2 are the nonzero `u32` maximum published client
records and maximum active subscriptions represented per client. Both are
present in a selected family descriptor and cannot exceed their canonical hard
maximums.

DISCONNECT names a session ID, operation ID, and UTF-8 reason. Its Result is
queued before the target receives Core GOAWAY. Targeting the caller is valid
and becomes an orderly self-disconnect after the Result is sent. Core SHUTDOWN,
not this family, stops the whole server.

## Surface family

Surface is family `0x0020`, version 1. It owns compositor objects, application
endpoints, input seats, viewer-specific presentation, capture, and timed video.
It replaces the connection-global display-rate and codec messages with
per-view negotiation.

Surface family-limit tags 1 through 9 are respectively maximum surfaces per
session (`u32`), views per session (`u32`), view dimension (`u32`), view pixels
(`u64`), frame rate (`u32`), inline cursor bytes (`u32`), and remote contacts
(`u32`), live application endpoints per session (`u32`), and maximum
application-endpoint lifetime (`u64` nanoseconds). All are nonzero, no larger
than the canonical hard maximums, and are present in a selected family
descriptor. The endpoint hard ceilings are 1,024 live endpoints and 24 hours.
OPEN_VIEW and CONFIGURE_VIEW are also bounded by the hard dimension, pixel,
and frame-rate limits before negotiated limits are applied.

| Class                   | Kinds                                                                                                                                       |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Request                 | WATCH, UNWATCH, CREATE_APP_ENDPOINT, OPEN_VIEW, CONFIGURE_VIEW, RESET_VIEW, CLOSE_VIEW, CAPTURE, RESIZE, FOCUS, CLOSE, RELEASE_APP_ENDPOINT |
| Event, client to server | STATE_ACK, KEY, TEXT, PREEDIT, POINTER, AXIS, TOUCH, FRAME_ACK                                                                              |
| Event, server to client | STATE, FRAME, REMOTE_INPUT                                                                                                                  |

Surface state records are keyed by boot-scoped `surface_handle` and contain
parent, application handle, application ID, title, lifecycle, logical size,
buffer scale, activation request revision, cursor, and committed text-input
state. ADD, PATCH, and REMOVE replace the retired SURFACE_CREATED, TITLE,
APP_ID, ORIGIN, RESIZED, ACTIVATED, CURSOR, TEXT_INPUT, and DESTROYED messages without losing
which revision a client has applied.

CREATE_APP_ENDPOINT returns a session-owned, boot-scoped `app_handle`, an
absolute nonzero `expires_server_ns` on the Core monotonic clock, and exact
environment overrides needed to launch into that application identity. The
expiry is later than request admission and no later than admission plus the
negotiated maximum lifetime. Terminal CREATE and Process SPAWN accept the
handle directly only before that deadline and in its owning session. The
socket basename and platform mechanism are server details; surface attribution
no longer depends on a self-asserted application ID or an unrelated retired
global operation.

RELEASE_APP_ENDPOINT is kind `0x000b` and carries
`app_handle:u64,operation_id:[u8;16],Extensions`. Its OK Result has no family
body. It atomically removes the endpoint and its launch environment. Releasing
an already released or expired handle is an idempotent OK; zero is invalid.
Expiry and session loss perform the same release automatically. A released or
expired handle is never reused during the server boot and subsequent launch
requests using it return NOT_FOUND. The live-endpoint cap counts only endpoints
owned by that session which have not been released or expired.

A successful CREATE_APP_ENDPOINT Result is replayable only while that exact
endpoint remains live. Once RELEASE_APP_ENDPOINT, expiry, or session
invalidation retires it, an exact CREATE retry returns STALE instead of
republishing launch authority that no longer exists; reuse of either lifecycle
operation ID with another canonical payload remains CONFLICT. The server keeps
the first completed RELEASE settlement in the endpoint's bounded CREATE replay
record. Consequently cleanup of a live endpoint remains admissible when the
ordinary Surface mutation replay table is full, while both operation identities
and the retired CREATE tombstone remain available for the rest of the session.

The endpoint is not stored, returned, or usable for launch until the compositor
acknowledges that its listener source was installed. A failed installation
drops the listener and unlinks its pathname without creating replay state.
Release, expiry, and session cleanup likewise cannot be dropped by bounded
compositor-command pressure: the pathname is unlinked after command admission,
and settlement waits until the compositor acknowledges that the listener token
was withdrawn or already absent. Compositor disconnection is equivalent to
withdrawal because teardown drops all listener sources and file descriptors.

RESIZE and FOCUS each retain one latest-success replay slot per live
`(surface_handle,request kind)`. An exact retry of that current operation ID and
canonical payload replays its Result without applying the backend mutation a
second time. RESIZE advances Surface state when it changes an active claim;
FOCUS does not mutate the Surface catalogue and returns its current revision.
A later successful same-kind mutation of the same Surface supersedes that slot;
a rejected or failed successor does not. The replay horizon of a superseded ID
has ended: the client MUST reconcile the authoritative WATCH catalogue state
and MUST NOT retry that old ID.

RESIZE dimensions are either both positive or both zero. Positive dimensions
replace this session's logical-size and display-density claim. The zero pair
releases that claim without changing the Surface catalogue geometry; it is how
a client that remains connected stops influencing mediation after its last
resizable view disappears. A mixed zero/positive pair is invalid.

FOCUS with `focused=1` directs keyboard and text-input focus to the named
Surface. `focused=0` is accepted as a compatibility no-op. Viewer focus never
creates or clears the activation-request extension: that extension represents
application-originated `xdg_activation_v1` state published by the compositor.

CLOSE instead retains the first successful slot without supersession until the
watched Surface REMOVE (or session teardown). Its exact retry replays OK, while
a fresh CLOSE identity for that still-live or closing Surface returns CONFLICT.
REMOVE retires all three resource-scoped slots before it becomes authoritative;
a later retry is resolved against the catalogue and can return NOT_FOUND. These
slots are independent of the ordinary Surface replay-table capacity, are purged
with their Surface, and are bounded to at most three per live Surface. Retained
operation IDs conflict globally across these slots, endpoint CREATE and nested
RELEASE identities, and the ordinary Surface replay table.

OPEN_VIEW negotiates codec versions, encoded dimensions, maximum frame rate,
latency target, and decoder capacity for one client mount. When more than one
configured codec family is acceptable, the server runs its ordered host
encoder probes before completing OPEN_VIEW and returns the codec of the first
successful encoder. The chosen family is then fixed for that view: later
backend recovery cannot change the codec promised by the Result. Thus hardware
AV1 may outrank hardware H.264 while software H.264 can still outrank software
AV1, according to the configured encoder list. CONFIGURE_VIEW replaces the
supplied values without resubscription. CAPTURE returns an inline image up to
32 KiB or a byte Transfer carrying the selected PNG or AVIF object.

Before OPEN_VIEW succeeds, the server reserves
`decoder_capacity * max_decoded_frame` from the same aggregate peer-receive
budget used by State and Transfer. CONFIGURE_VIEW growth succeeds only after
the complete additional worst-case window is reserved; if that exact growth
does not fit, it returns RESOURCE_EXHAUSTED without changing the view or
backend configuration. The reservation is a per-view high-water mark: reducing
decoder capacity does not release it, because frames already written may still
be retained by the receiver. Later growth up to that high-water mark needs no
new reservation. Credit is released only after view retirement and its prior
reliable-frame boundary, natural-destruction EOS, or session teardown.

FRAME uses the same chunk identity and decoder-allocation safeguards as a
Terminal frame, plus capture timestamp, presentation timestamp, color space,
damage region, and codec version. Frames are latest-biased: the server may drop
an unencoded or queued delta when the view is behind, but it never sends a
delta whose base was dropped. FRAME_ACK reports the highest presented frame,
decoder queue depth, and available slots. Video does not use Transfer MESSAGE;
byte credit cannot express keyframes, presentation, or safe frame dropping.
Configuration, keyframes, and non-discardable dependencies use the reliable
link. Independently discardable chunks may use the optional datagram path and
are abandoned as a whole logical frame when any chunk is missing.

Surface packed codecs carry optional `DIMENSIONS` (tag 3) and
`LOGICAL_DIMENSIONS` (tag 4) metadata, each with exactly
`width:u32,height:u32`, both nonzero. DIMENSIONS describes the encoded image;
LOGICAL_DIMENSIONS describes the full surface extent represented by that frame
in application logical pixels. Encoding downscales do not change its logical
extent. Receivers retain this geometry with the frame through decoding and
presentation, and apply their own display scale and zoom. Neither the shared
composite density nor the requested encoder viewport determines presentation
size. With default zoom, one application logical pixel occupies one CSS pixel
in every live viewer. These tags have flags zero so older receivers can skip
metadata they do not recognize.

On the reliable link, CONFIGURE_VIEW, RESET_VIEW, and CLOSE_VIEW take effect
only after every frame already queued for that view has been written. Natural
Surface destruction retires every associated view and emits one reliable
keyframe-shaped FRAME with END_OF_STREAM before the watched Surface REMOVE.
Receivers discard any older optional-path fragments that arrive after that
terminal frame.

Successful OPEN_VIEW and CONFIGURE_VIEW Results are write barriers before
frames from the new or replacement configuration become observable across the
Control and Data scheduler classes.

Surface input has stable YAS enums. KEY carries a generated physical-key code,
press/release/repeat state, modifiers, and client monotonic time. The initial
registry follows USB HID positions where they exist but is a YAS registry, not
a browser or evdev number. Modifiers are the sender's current state, including
the post-event Caps Lock state; a Caps Lock press or repeat must not toggle
that snapshot again. TEXT carries committed UTF-8 with case already resolved,
independent of the seat's Shift and Caps Lock state. PREEDIT carries text,
selection, and cursor as byte offsets. POINTER positions use signed 32.32
fractions of the presented frame when both axes are in `0..=1`; for Surface v1
compatibility, a pair outside that range is interpreted in the original native
composite physical-pixel space and normalized by the server. AXIS smooth deltas
remain physical pixels. TOUCH contacts remain in the native composite physical
pixel space. AXIS separately represents discrete steps, source, and stop.
Unknown enums are dropped and counted, never coerced into a valid button or
key.

REMOTE_INPUT carries another session's transient pointer or touch marks with a
seat handle and expiry. Its exact body is
`surface_handle:u64,seat_handle:u64,expires_server_ns:u64,input_kind:u8,reserved:u8=0,contact_count:u16`
followed by `contact_id:u32,x_32_32:i64,y_32_32:i64` contacts. Input kind 0 is
POINTER and requires exactly one contact whose ID is zero; kind 1 is TOUCH and
permits zero through 64 uniquely identified contacts. It is best-effort
presentation, not surface state; a zero-contact TOUCH event retires marks
immediately and timeout retires them if a client disappears. An already expired
event (including `expires_server_ns=0`) immediately retires that surface's marks
of the named kind. Pointer leave and ownership handoff use this with the required
ID-zero contact; its coordinates are ignored. A newer event replaces the previous
deadline for that surface and kind.

## Selection family

Selection is family `0x0021`, version 1. It owns clipboard, primary selection,
and drag-and-drop data without embedding unbounded content in control frames.

| Class   | Kinds                                                                                      |
| ------- | ------------------------------------------------------------------------------------------ |
| Request | WATCH, UNWATCH, SET, SET_BEGIN, SET_COMMIT, GET, CLEAR, DRAG_BEGIN, DRAG_DROP, DRAG_CANCEL |
| Event   | STATE, STATE_ACK, DRAG_ENTER, DRAG_MOTION, DRAG_LEAVE                                      |

Selection state is keyed by slot (`CLIPBOARD` or `PRIMARY`) and reports owner
kind, owner handle, revision, and offered MIME types. SET atomically installs a
fully inline offer capped at 32 KiB total. SET_BEGIN returns a staging handle
and byte Transfers for larger items; SET_COMMIT carries the operation ID and
changes ownership only after every promised item is sealed. A failed upload or
commit leaves the previous selection intact. RESET of any item upload before
SET_COMMIT discards the handle and every sibling item upload. GET names slot, revision, and
MIME type and returns inline bytes or a Transfer. CLEAR is conditional on the
observed revision, so one client cannot erase a newer owner accidentally.

A drag is a session-scoped resource with offered MIME types, optional named
items, source actions, current target, and revision. Motion remains an Event;
begin, drop, and cancel are correlated Requests. Drop data is fetched lazily by
MIME type through Transfers instead of being copied into one potentially huge
DROP Request. The same model handles a browser-origin drag and a
compositor-origin drag by reversing producer and consumer roles.

The drag `revision` is a fixed nonzero generation token from DRAG_BEGIN through
ENTER, MOTION, LEAVE, and the final DROP or CANCEL. Target and action changes
advance the outer Selection catalogue revision but not this token, so motion
does not require a response round trip before the next Event.

Although the schema permits many maximum-length MIME strings, one complete
Drag record is still a single State record. DRAG_BEGIN and the final metadata
update in DRAG_DROP return RESOURCE_EXHAUSTED before mutating the drag when the
exact projected one-record StateEvent would exceed the common publication
policy. This prevents a valid control mutation from creating a catalogue
revision that no admitted watcher could receive.

Selection family-limit tags 1 through 6 are the nonzero `u32` maximum inline
bytes, items, MIME bytes, item-name bytes, active drags per session, and upload
stages per session. Tag 7 is the nonzero `u64` maximum staged bytes per stage.
Tag 8 is the nonzero `u32` `max_mutation_replays` bound. All are present in a
selected family descriptor and cannot exceed the canonical hard maximums.

SET_BEGIN keeps its exact successful Result, including byte-identical Transfer
descriptors, replayable while the returned stage and every sibling item upload
remain live. Reusing that operation ID with another canonical request is
CONFLICT. Successful Transfer CLOSE of any item retires replay eligibility but
preserves the sealed stage for SET_COMMIT. SET_COMMIT, RESET of any sibling
upload, expiry, or session teardown removes the stage. An identical retry of a
retained retired SET_BEGIN is STALE and cannot create a replacement stage.
Fully live stages pin their replay records.
Retired records are the oldest-first eviction candidates when a later distinct
settlement needs room, and at most the advertised `max_mutation_replays`
records are retained. Once an ID leaves that bounded horizon, the client must
reconcile Selection state and use a fresh operation ID rather than retry it.

An item name in DRAG_BEGIN may be empty, meaning UNKNOWN. DRAG_DROP extension
tag 1 is always REQUIRED. Its value is `item_count:u16` followed by that many
`name:string_u16,selected_mime:string_u16` pairs. The count must match the
current drag; a nonempty original name cannot change, and each selected MIME
must be in that item's offer. The server sends the owner exactly one GET per
item in index order for the selected MIME. It completes DROP only after every
inline or Transfer body validates, publishing all final names and delivering
all bodies atomically; any GET or Transfer failure cancels the drop without
partial delivery. Names are at most 1024 UTF-8 bytes.

For browser-origin file drags, the server materializes named items in private,
per-drag directories owned by Selection; FS need not be selected. A known name
reserves an empty file and allows its URI to be read during hover. Unknown names
are resolved at DROP. All selected bodies validate before the files are filled
and the compositor receives DROP. Successful files live until session teardown;
cancelled or failed drags discard their files. Repeated names across items or
drags refer to separate files.

## Desktop family

Desktop is family `0x0022`, version 1. It projects tray icons, menus, and active
notifications into normalized state; it does not tunnel D-Bus.

| Class   | Kinds                                                                   |
| ------- | ----------------------------------------------------------------------- |
| Request | WATCH, UNWATCH, GET_MENU, TRAY_ACTION, NOTIFICATION_ACTION, FETCH_ASSET |
| Event   | STATE, STATE_ACK                                                        |

The WATCH payload selects tray, notifications, or both. Tray and notification
records have stable boot-scoped handles and revisions. Icons and notification
images are inline when small and otherwise content-addressed Transfers. Menus
are fetched by `(tray_handle, tray_revision, menu_revision)` and returned as a
typed tree; the tuple makes stale clicks CONFLICT instead of applying them to a
different menu. Actions likewise carry the revision they were rendered from.
TRAY_ACTION distinguishes ACTIVATE, SECONDARY_ACTIVATE, SCROLL, and MENU_ITEM.
Only SCROLL carries a nonzero signed value and may set the HORIZONTAL flag;
only MENU_ITEM carries nonzero menu revision and item handle. Opening a menu is
GET_MENU, not an action.

Notification replacement, close reasons, buttons, reply fields, progress, and
resident/transient behavior are records in the state schema. A complete
notification's extensions use tag 1 for the 32-byte content-image hash, tag 2
for the 32-byte application-icon hash, tag 3 for progress
`{value:u32,maximum:u32}`, and tag 4 for reply metadata
`{placeholder:string_u16}`. Progress requires nonzero `maximum` and
`value <= maximum`. `HAS_PROGRESS` and `HAS_REPLY` are set if and only if the
corresponding extension is present. Notification PATCH uses the same tags and
codecs; a zero-byte value clears that field and otherwise the complete typed
value replaces it. Clearing or setting progress/reply also clears or sets the
derived flag.

Every viewer sees the same state and may invoke DEFAULT, ACTION, or DISMISS. An
action handle is nonzero only for ACTION, and reply text is valid only with
ACTION. Tray REMOVE is `{handle:u64,revision:u64}`. Notification REMOVE appends
`close_reason:u8,reserved:[u8;3]=0`; reasons are 1 EXPIRED, 2 DISMISSED,
3 CLOSED_BY_CALLER, and 4 UNDEFINED. State RESET handles a restarted desktop
service without inventing another subscription protocol.

The native Desktop projection bounds the presentation strings before it
advances the catalogue revision. A tray title is at most 65,535 UTF-8 bytes.
For one notification, application, summary, body, and published action labels
share a 786,432-byte UTF-8 budget; body is additionally capped at 524,288
bytes, each action label at 1,024 bytes, and at most 256 non-default actions
are published. Truncation is at UTF-8 boundaries. Only those published action
handles are retained as invokable mappings. Catalogue rebuild is
transactional: an invalid backend record cannot leave handle, asset, expiry,
action, or revision side effects.

Desktop family-limit tags 1 through 6 are the nonzero `u32` maximum tray
items, notifications, menu nodes, notification actions, inline menu bytes, and
inline asset bytes. All are present in a selected family descriptor and cannot
exceed the canonical hard maximums.

## Media family

Media is family `0x0023`, version 1. It combines the current compositor audio,
viewer microphone/camera leases, desktop portal exchanges, and MPRIS player
bridge because they share timed media streams and one session media service.
It does not merge media frames into generic Transfer messages.

| Class   | Kinds                                                                                                                             |
| ------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Request | WATCH, UNWATCH, OPEN_OUTPUT, ACQUIRE_DEVICE, RELEASE_DEVICE, PORTAL_REPLY, PLAYER_ACTION, CLOSE_STREAM, FETCH_ASSET, PORTAL_CLOSE |
| Event   | STATE, STATE_ACK, PORTAL_REQUEST, FRAME, FRAME_ACK, PLAYOUT_REPORT, STREAM_STATUS                                                 |

State records describe output devices, viewer-device leases, pending portal
requests, and normalized MPRIS players. A device lease is a boot-scoped
resource with media kind, format, owner session, lifecycle, and expiry. Browser
or OS consent remains part of acquiring the device, but it is application
state: refusal is a normal CANCELLED Result and does not change what else the
session can do.

Media family-limit tags 1 through 14 are the nonzero `u32` maximum devices,
leases per session, streams per session, portals per session, players, formats,
inline metadata bytes, inline asset bytes, portal metadata bytes, portal string
bytes, portal body bytes, portal choices, portal choice options, and screencast
candidates. All are present in a selected family descriptor and cannot exceed
the canonical hard maximums.

OPEN_OUTPUT carries the ordered acceptable audio formats, a latency target,
and `target_bitrate_kbps:u16`. A zero bitrate selects the server default;
otherwise the value is an Opus target in kilobits per second from 1 through
`MAX_OUTPUT_BITRATE_KBPS` (512). The selected format remains authoritative for
codec, channel count, and sample rate. A successful OPEN_OUTPUT Result is a
write barrier before the initial STREAM_STATUS becomes observable across the
Control and Data scheduler classes.

FRAME is direction-neutral and names a media stream, sequence, capture time,
codec version, flags, fragment index/count, and complete encoded-frame length.
Audio capture time and nonzero presentation time are sample positions at the
selected format's sample rate from the server media-session epoch; video times
are nanoseconds from that same epoch. A zero presentation time means to use the
capture time. Platform APIs that expose millisecond timestamps convert toward
the earlier instant at the native backend boundary: `samples = floor(ms *
sample_rate / 1000)` and `ms = floor(samples * 1000 / sample_rate)`;
milliseconds are never placed directly in a canonical audio timestamp.
FRAME_ACK supplies the last consumed sequence, queue depth, and desired credit
in complete frames. Live media is latest-biased and may report explicit gaps;
it never retransmits stale realtime data merely because the YAS link is
reliable. Discardable media frames may use the optional datagram path;
configuration, lease state, keyframes, and final status remain reliable. Codec
configuration and keyframe changes are STREAM_STATUS Events. On the reliable
link, FRAME and STREAM_STATUS share one FIFO so configuration changes and a
final CLOSED/ERROR status cannot overtake earlier frames. A receiver MUST
ignore a discardable frame that arrives late from an optional datagram path
after the stream's final status.

PLAYOUT_REPORT carries the output stream, its last consumed sequence, and the
client-measured extra audible-audio latency relative to the focused visible
surface's video. Other surfaces do not contribute timing samples because their
independent encode and transport paths are not the path being synchronized. It is a
reliable client-to-server Event and is only valid for an acknowledged live
audio-output stream. The server publishes the maximum active viewer report as
PipeWire `ProcessLatency` on the configured-default `yas-sink`. PipeWire derives the sink playback
ports' input `Latency` from that value and exposes it to playback applications,
which can select their own corresponding video frame. A latency published on
the downstream monitor-capture node stops at the sink's monitor ports and
never reaches its playback ports. YAS does not hold video frames to perform
A/V sync. The browser canvas is never held for this correction; interactive
surface presentation remains on its normal low-latency path.

Portal requests are stateful typed resources rather than opaque subtypes.
ACCESS request metadata contains a nonzero deadline, optional parent Surface,
application and presentation strings, and bounded choices with stable string
IDs and options. Its GRANT reply carries the chosen ID/value pairs.
SCREENCAST request metadata contains the deadline, optional parent Surface,
application ID, single/multiple selection mode, and bounded candidate Surface
records with dimensions and optional content hashes for thumbnails. Its GRANT
reply carries unique selected Surface handles. PORTAL_REPLY repeats the portal
kind as well as the request revision; DENY and CANCEL metadata is exactly empty.
Pending state records reuse request metadata. An ACCESS grant state carries the
chosen values. A SCREENCAST grant state replaces each chosen Surface handle
with an exact `(surface_handle, stream_handle)` pair; both namespaces are unique
within the portal. PORTAL_CLOSE carries the portal handle, observed revision,
and nonzero operation ID. It idempotently closes a pending or granted portal,
releases every granted stream, and advances the resource to CANCELLED; repeated
operation IDs replay the same Result. Application withdrawal advances it to
WITHDRAWN. Denied, cancelled, and withdrawn records have empty metadata.

Media asset references, including candidate thumbnails and player album art,
are BLAKE3 content hashes fetched with FETCH_ASSET. Small values may be inline;
larger values use a sensitive server-to-client BYTE Transfer and the aggregate
receive budget. MPRIS actions name a player handle and observed revision.

The packed media registry distinguishes 4:2:0 from 4:4:4 video. H.264 4:4:4 is
codec `0x0104`, version 1, and AV1 4:4:4 is codec `0x0105`, version 1. They are
not aliases for the H.264 and AV1 4:2:0 codec IDs. Their exact framing is
defined by `protocol/yas/codecs/media-h264-444-v1.toml` and
`protocol/yas/codecs/media-av1-444-v1.toml`.

## Font family

Font is family `0x0024`, version 1. It enumerates the fonts a YAS server elects
to expose, describes their browser-relevant faces and metrics, and transfers
the selected face bytes. Font discovery is scoped to one server connection;
Relay does not aggregate a remote server's catalogue into the home server's
catalogue.

| Class   |     Kind | Name      |
| ------- | -------: | --------- |
| Request | `0x0000` | WATCH     |
| Request | `0x0001` | UNWATCH   |
| Request | `0x0002` | DESCRIBE  |
| Request | `0x0003` | FETCH     |
| Event   | `0x0000` | STATE     |
| Event   | `0x0001` | STATE_ACK |

WATCH and UNWATCH use the state convention. The catalogue includes every
family permitted by server policy, not only monospace families; clients filter
on the flags appropriate to their UI. One family summary record is:

```text
[font_handle:u64][generation:u64][flags:u16][face_count:u16]
[family_len:u16][family:N]
[display_len:u16][display:M]
[extensions_len:u32][extensions:O]
```

`font_handle` is boot-scoped and never reused. `generation` increases whenever
the set of faces or any description relevant to loading them changes. Family
names are canonical UTF-8 names suitable for a CSS `font-family` descriptor;
display names may be localized. Flag bit 0 is MONOSPACE, bit 1 VARIABLE, bit 2
COLOR, and bit 3 FETCHABLE, meaning at least one face can be fetched. Remaining
bits are zero. A family can be listed but not fetchable when server policy or
the font's embedding metadata forbids export.

DESCRIBE payload is:

```text
[font_handle:u64][generation:u64][initial_receive_credit:u64]
[extensions_len:u32][extensions:N]
```

The observed generation prevents a selection from being described as a
different installed font after a catalogue race. A stale generation returns
STALE. The OK Result begins:

```text
[font_handle:u64][generation:u64][description_hash:32]
[delivery:u8][reserved:3=0]
```

Delivery 0 INLINE continues with `[description_len:u32][description:N]` and is
limited to 32 KiB. Delivery 1 TRANSFER continues with
`[description_len:u64][transfer_descriptor]`; that descriptor is a
server-to-client BYTE Transfer of `yas.font.description/1`
(`content_family = 0x0024`, `content_kind = 0x0000`, `content_version = 1`),
and its initial credit does not exceed the Request proposal.
`description_hash` is BLAKE3 over the exact description bytes in either
delivery.

A version-1 description is:

```text
[family_len:u16][family:N]
[face_count:u16] repeated{
  [face_record_len:u32][face_record]
}
[extensions_len:u32][extensions:M]
```

Each face record is:

```text
[face_handle:u64][content_hash:32][byte_len:u64]
[format:u8][style:u8][face_flags:u16]
[weight_min:u16][weight_default:u16][weight_max:u16]
[stretch_min:u16][stretch_default:u16][stretch_max:u16]
[slant_tenths_degrees:i16][units_per_em:u16]
[cell_advance:i32][ascent:i32][descent:i32][line_gap:i32]
[subfamily_len:u16][subfamily:N]
[postscript_len:u16][postscript:M]
[extensions_len:u32][extensions:O]
```

Format values are 0 SFNT_TRUETYPE, 1 SFNT_CFF, 2 WOFF, and 3 WOFF2. Style
values are 0 NORMAL, 1 ITALIC, and 2 OBLIQUE. CSS weights use 1 through 1000;
stretch values are percentages in tenths, so 1000 is normal width. Static
faces have equal minimum, default, and maximum values. Face flag bit 0 is
VARIABLE, bit 1 COLOR, and bit 2 FETCHABLE. Metrics are signed font units
except `units_per_em`; `cell_advance / units_per_em` is the exact terminal cell
advance ratio when MONOSPACE is set. A zero `cell_advance` means it is not
defined. Extension records carry variable axes, localized names, Unicode
coverage summaries, color-font capabilities, and embedding metadata without
changing the required loading fields.

`face_handle` is boot-scoped and invalidated when its served bytes or required
metadata change. `content_hash` is BLAKE3 over the bytes FETCH returns and is
stable across server boots when those bytes are identical. This separates
short-lived resource identity from a durable client cache key.

FETCH payload is:

```text
[face_handle:u64][expected_content_hash:32]
[initial_receive_credit:u64]
[extensions_len:u32][extensions:N]
```

The hash is required. A changed or removed face returns STALE or NOT_FOUND
rather than serving bytes under obsolete CSS metadata. The OK Result is:

```text
[face_handle:u64][content_hash:32][byte_len:u64]
[format:u8][reserved:3=0][transfer_descriptor]
```

The descriptor is a server-to-client BYTE Transfer of
`yas.font.face-bytes/1` (`content_family = 0x0024`,
`content_kind = 0x0001`, `content_version = 1`). It delivers one complete
browser-consumable font resource with no base64 or generated CSS wrapper. A
face stored in a TTC or OTC collection is rebuilt as a standalone SFNT
resource; collection indices and server filesystem paths are never exposed.
The client constructs its own `FontFace` from the bytes and the description's
family, style, weight, and stretch fields. Transfer CLOSE is successful only
after exactly `byte_len` bytes whose BLAKE3 is `content_hash`; otherwise the
client discards them.

Clients cache face bytes by content hash and may use cached bytes immediately
when a later description names the same hash, including after a server restart
or through another server. Catalogue deltas invalidate handles and metadata,
not already verified content-addressed cache entries. The YAS edge has no font
API. Font enumeration, metadata, and bytes are served only through the
authenticated Font family on the home server.

Font family limits advertise at least maximum families, faces per family,
description bytes, face bytes, concurrent FETCH transfers, scan duration, and
catalogue refresh interval. Descriptions and face bytes consume normal state
and Transfer credit under the aggregate receive budget. Servers bound scans,
reject malformed font tables, and hash the post-extraction bytes before
publishing them.

The ordered Font limit extensions and canonical hard ceilings are:

| Tag | Limit                    | Type  | Hard maximum          |
| --: | ------------------------ | ----- | --------------------- |
|   1 | families                 | `u32` | 65,535                |
|   2 | faces per family         | `u32` | 65,535                |
|   3 | description bytes        | `u64` | 67,108,864            |
|   4 | face bytes               | `u64` | 67,108,864            |
|   5 | concurrent FETCHes       | `u32` | 1,024                 |
|   6 | scan duration            | `u64` | 300,000,000,000 ns    |
|   7 | catalogue refresh period | `u64` | 86,400,000,000,000 ns |

All except the refresh period are nonzero. A zero refresh period means that
the catalogue is immutable for the current server boot.

## Filesystem family

FS is family `0x0030`, version 1. It combines sync, fetch, one-shot read,
search, index, grep, write, structural operations, and chunked upload around a
single root resource and one mutation model.

| Class   | Kinds                                                                                     |
| ------- | ----------------------------------------------------------------------------------------- |
| Request | OPEN, CLOSE, WATCH, UNWATCH, FETCH, READ, SEARCH, INDEX, GREP, STAGE_WRITE, COMMIT, APPLY |
| Event   | STATE, STATE_ACK                                                                          |

OPEN resolves a platform path, a terminal/process cwd, or the session-owned
drag staging directory and returns a boot-scoped `root_handle`, canonical path
model, case behavior, and limits. TERMINAL_CWD carries a root-relative
component-vector suffix which is joined after resolving the terminal's live
cwd. STAGING has no body, is created lazily, and survives closing an FS root;
the server removes it only when the owning YAS session ends.
All later paths are root-relative vectors of length-delimited components. Unix
components are bytes; Windows components use the advertised Windows path
model. Empty, dot, dot-dot, separator-containing, and platform-prefix
components are invalid, so traversal cannot hide inside string normalization
and the protocol needs no percent-escape convention. A caller opens the
platform root when it wants the server-wide view. Operations run as the server
OS identity.

A platform-path source may resolve to one regular file or symlink rather than
a directory. A non-recursive WATCH then publishes exactly one entry at the
zero-component root path. RECURSIVE and ignore-enumeration flags are invalid
for such a root. This is the canonical single-file watch; no synthetic parent
directory or platform-path split is required.

WATCH sends a staged snapshot followed by ordered ADD, REPLACE, PATCH, MOVE,
and REMOVE records. Each record carries metadata, a 32-byte BLAKE3 content
hash, and inline content only below the requested threshold. Larger content is
lazy and fetched by hash or path. Renames remain first-class; RESET is explicit
when the watcher loses history. Mutation records include their operation ID so
a caller can recognize its own echo without the server suppressing it.
Before returning OK, the server reserves the actual aggregate-clamped State
window and requires it to fit the largest valid future one-record Event as well
as snapshot markers. The maximum envelope includes PATCH's repeated maximum
path plus its complete maximum replacement entry (which is larger than one
complete snapshot entry, MOVE, or REMOVE). An insufficient window returns
RESOURCE_EXHAUSTED and closes the provisional native watch without retaining a
subscription or credit lease. Snapshot and Delta records remain one record per
Event and are checked against the admitted envelope when sent.
The required `MAX_CATALOG_ENTRIES` family limit caps each root catalogue and
has a canonical hard maximum of 1,000,000, matching the filesystem watcher.
A snapshot replacement may transiently retain the old and staged
catalogues, but each generation independently obeys that limit.

WATCH carries an exact `settle_ms:u16` (`0` selects the server default,
otherwise 1 through 1000), inline threshold, and UTF-8 gitignore-pattern text
with one rule per line, capped at 65535 bytes. Its independent flags select
recursive enumeration, inline content, hidden entries, governing `.gitignore`
rules, governing `.ignore` rules, and exact-name `.git` exclusion. Caller
patterns have highest precedence and retain gitignore last-match-wins and `!`
re-inclusion semantics. An empty pattern string adds no caller rules.

Entry flags preserve EXECUTABLE, READ_ONLY, HIDDEN, UNREADABLE, UNSTABLE,
SYMLINK_DIRECTORY, and DIRECTORY_FILTERED independently. UNREADABLE and
UNSTABLE entries have no inline content. A symlink record carries both its raw
target and `BLAKE3(target)` so link CAS does not require another read. Optional
EntryRecord extension tag 1 is exactly one nonzero 16-byte operation ID.

FETCH reads one file; READ answers grouped path questions; SEARCH performs
ranked path search; INDEX returns a pageable candidate set; GREP returns typed
matches with truncation and continuation cursors. Every potentially large
answer uses the common inline-or-Transfer representation and an explicit next
cursor. No query relies on fragmentation of an oversized logical Result.

QueryPage flags include TRUNCATED and its typed record kinds are READ, PATH,
GREP_FILE, and GREP_MATCH. A READ record identifies its question index, common
status, optional answered WirePath, and kind-specific bytes: an EntryRecord for
STAT, exactly 32 hash bytes for HASH, raw target bytes for LINK_TARGET, or raw
file bytes for CONTENT. SEARCH and INDEX return PATH records with DIRECTORY and
IGNORED flags. GREP_FILE records carry dense page-local file indices, expected
match counts, ignored state, and path; GREP_MATCH names that file index plus a
zero-based, UTF-8-byte-column, end-exclusive range and at most 512 bytes of
UTF-8 display text. Unknown optional record kinds are skipped and unknown
required kinds fail the page.

STAGE_WRITE validates path, expected base hash, mode, and size, then returns a
byte Transfer and an upload handle. Transfer CLOSE only seals the staged bytes.
RESET before COMMIT discards that handle; expiry and session loss do likewise.
COMMIT carries the operation ID, rechecks the base hash, verifies size and
BLAKE3, optionally syncs, and atomically lands the file; it is the durable
idempotent mutation. APPLY handles small inline writes and typed mkdir, remove,
rename, symlink, and hardlink operations as a length-delimited batch. Each item
has an explicit precondition and result; requested all-or-none behavior is
accepted only when the platform can provide it, otherwise UNSUPPORTED.
Disconnect drops uncommitted staging; a successful durable commit is
reconciled by operation ID after reconnect or reboot.

STAGE_WRITE flag CREATE_PARENTS applies to its eventual file. APPLY item flag
CREATE_PARENTS is valid for WRITE_INLINE, MKDIR, RENAME, SYMLINK, and HARDLINK,
and forbidden for REMOVE. COMMIT success returns root and entry revisions,
`modified_unix_ns`, and content hash. Every APPLY item likewise returns its
modification time. On an item CONFLICT its revision, modification time, and
optional hash describe the current entry (zero/absent means the entry is now
absent). A STAGE_WRITE or COMMIT Core Result with status CONFLICT has no family
body, following the common Result rule. Its ResultPrefix `detail` Extensions
contains optional FS tag `RESULT_CONFLICT_DETAIL_EXTENSION` (1), whose exact
value is ConflictDetail: WirePath, current-presence/hash-presence bits, current
entry revision and modification time, then the optional current 32-byte hash.
Unknown optional detail extensions are skipped; required Result detail
extensions remain forbidden by Core.

## Git family

Git is family `0x0031`, version 1. It preserves the useful split between small
mutable repository state is watched, while immutable content is pulled by
object ID and cached indefinitely.

| Class   | Kinds                                                                 |
| ------- | --------------------------------------------------------------------- |
| Request | OPEN, CLOSE, WATCH, UNWATCH, QUERY, WATCH_QUERY, UNWATCH_QUERY, FETCH |
| Event   | STATE, STATE_ACK, QUERY_STATE, QUERY_STATE_ACK, PROGRESS, CLOSED      |

Request kinds are assigned in the order shown (`0x0000` through `0x0007`);
Event kinds are likewise `0x0000` through `0x0005`. The generated field
layouts, enum values, limits, and Transfer content kinds in
`protocol/yas/families/git.toml` are normative.

OPEN accepts a raw platform path, FS root/path pair,
parent-repository/submodule pair, or Terminal handle plus relative FS-path
suffix. The Terminal form resolves the suffix against that Terminal's live CWD
atomically. Success returns the boot-scoped repository handle and revision,
object format, BARE/SHALLOW/SPARSE/LINKED/WRITABLE/FETCHABLE flags, canonical
raw worktree path (empty exactly for BARE), and canonical raw git-directory
path. STATE records carry HEAD, refs, remotes, in-progress operation, and
optional index/worktree status, upstream tracking, stash entries, and the
worktree-set generation. Reconnect resumes by repository revision when retained.
CLOSED is the terminal server-side lifecycle event: it carries the repository
handle, last revision, exact client-request/repository-gone/permission/backend/
resource reason, and bounded detail. A client invalidates the repository and
all repository-scoped subscriptions before dispatching it.

An object ID is `[algorithm:u8][byte_len:u8][reserved:u16=0][bytes]`: SHA-1 is
exactly 20 bytes and SHA-256 exactly 32. Zero bytes are data, not a sentinel.
Repository source is a length-delimited tagged union. Component paths reuse the
FS component-vector encoding and platform paths remain raw bytes. WATCH's State
extensions contain optional nonzero ref/status settle windows in milliseconds
and an ordered unique list of raw ref prefixes. Zero/absence selects server
defaults; an empty prefix list watches every ref.

QUERY is a generated tagged union, not an untyped bag. Its variants are
RESOLVE, MERGE_BASE, LOG, TREE, BLOB, DIFF, PATCH, INDEX, DISCOVER, BLAME,
REFLOG, and WORKTREES. Each variant has its own required packed prefix and
typed output records. Bounded enumerations return one of the exact QueryCursor
variants: LOG_FRONTIER object IDs, FS PATH, raw PLATFORM_PATH, PATCH path plus
row position, or scalar POSITION. Empty cursor bytes mean START. Large blobs,
patches, and record pages use Transfer. Object IDs carry
their algorithm and exact byte length rather than zero padding or an all-zero
sentinel. Object result records carry an explicit RESULT, TIP, or HIDE role:
RESOLVE preserves its ordered positive and negative range endpoints as TIP and
HIDE records, while MERGE_BASE carries an ordered count and two or more object
IDs and emits RESULT for the best common ancestor of the complete set. A
multi-head merge base is not defined as an associative sequence of pairwise
queries.

LOG accepts either a revision specification or explicit ordered tips and hides,
plus subtree path and FIRST_PARENT/TOPO/FULL_MESSAGE/FOLLOW/PATH_OIDS flags;
its cursor is the remaining frontier. TREE, DIFF, and INDEX page by FS path.
BLOB names object, optional path, offset, and maximum returned bytes. DIFF and
PATCH use exact EMPTY/COMMIT/TREE/INDEX/WORKTREE/MERGE_BASE endpoints, optional
path, rename threshold, and the canonical whitespace/untracked/ignored/raw flags.
MERGE_BASE is valid only on the left. PATCH additionally names context lines,
byte budget, and structured/text/span mode, and returns the patch transforming
left into right; it is not implicitly "commit versus parent." DISCOVER carries
a RepositorySource, NESTED/BARE flags, depth, and raw-platform-path cursor.
BLAME carries object, one-based start/count and rename/copy flags. REFLOG uses
the common page record limit and a POSITION cursor; WORKTREES uses the same
cursor for stable list paging.

QUERY carries `[repository_handle:u64][max_records:u16][reserved:u16=0]
[cursor:bytes_u16][initial_receive_credit:u64][body:bytes_u32][Extensions]`.
Zero `max_records` selects the server default; a nonzero value cannot exceed
the negotiated `max_query_records` limit.
Its Result is a `QueryPage`: exact next cursor, total hint, and flags followed by
either bounded typed records inline or a sensitive server-to-client MESSAGE
Transfer. MORE is set exactly when the continuation cursor is nonempty. Known
v1 record bodies are object, commit, LOG path-at, tree entry, blob, diff, raw
patch content, structured patch file/row/gap/base, index entry, discovery,
blame, reflog, and worktree. Unknown optional typed records are skipped; an
unknown required record fails the page. Blob and raw patch content records
carry total object size plus exact `offset..next_offset` windows. Inline byte
length must equal the window; a sensitive BYTE Transfer sends exactly that
window before CLOSE. Blob and patch use distinct Transfer content kinds.

Commit records preserve tree and parent object IDs, raw author/committer
name/email fields, both timestamps and timezone offsets, full message bytes,
and lossy-bridge indication. With PATH_OIDS, each commit is immediately
followed by a typed path-at record carrying kind, mode, optional object, and
the rename-adjusted FS path; absent object exactly represents deletion.
Structured PATCH responses begin file sections that preserve status,
similarity, binary/filtered classification, and old/new paths. Row records
carry one-based old/new line numbers, raw side bytes, and bounded sorted
non-overlapping byte spans; zero line means that side is absent. Gap records
preserve hunk coordinates, base records preserve an effective merge base, and
the page PATCH cursor preserves path plus delivered row position. Index records preserve stage, status,
conflicted/intent-to-add/skip-worktree flags, mode, size, modification time,
and object ID. Discovery records contain both canonical worktree and git-dir
raw paths and identify bare, linked, and submodule repositories. Blame records
carry the current and original one-based ranges, original path, commit, author,
and summary. Reflog records carry stable index, old/new IDs, timestamp/timezone,
committer, and message. Worktree records preserve main/current/locked/prunable/
detached/bare state, optional HEAD, full branch ref, and lock reason.

WATCH_QUERY carries `max_records` with the same zero-default and hard-bound
semantics as QUERY, so each initial or replacement page preserves the caller's
requested page size. It turns a LOG or other ref-dependent query into watched
state. Each reevaluation replaces one complete page at a revision and uses
ordinary state credit. LOG pages use the same MORE flag and LOG_FRONTIER cursor as one-shot
LOG, so acknowledgement/coalescing cannot lose pagination state. A QUERY_STATE
`SNAPSHOT_RECORDS` event has exactly one ADD record and a
`DELTA` event exactly one REPLACE record. The record body is
`[query_status:u16][reserved:u16=0][detail:string_u32][page:bytes_u32]`.
OK requires empty detail and one INLINE `QueryPage`; non-OK requires nonempty
detail and no page. A later REPLACE with OK reports recovery from an
asynchronous ref-resolution, budget, or backend error without terminating the
subscription. Marker phases have no records. Watched pages never contain a
Transfer descriptor: `StateWatch.initial_credit` accounts the complete encoded
QUERY_STATE event, and reusing it as Transfer credit would double-account one
budget. Servers bound the inline page to available state credit and negotiated
query limits. The native server measures the complete outer QUERY_STATE
payload, not only its embedded StateEvent. If a valid reevaluated OK page is
too large, it publishes a small RESOURCE_EXHAUSTED value at that same adapter
revision and acknowledges the adapter update only after that value is sent; a
later bounded reevaluation may REPLACE it with OK. Backend failure detail is
truncated at a UTF-8 boundary to 4,096 bytes so the recovery value itself is
always bounded. FETCH is an idempotent mutation with operation ID, remote,
refspecs, PROGRESS Events keyed by operation ID, and a final Result containing
the changed-ref revision plus one status/old/new/detail record for every remote
ref answer. FETCH accepts empty remote/refspec lists for configured defaults,
timeout, and PRUNE/NO_TAGS/ANCHOR flags. YAS does not add a generic Git command tunnel;
arbitrary Git commands already run through Process or Terminal when a typed
query is not the right interface.

WATCH selects an exact HEAD/refs/remotes/operation/status/upstreams/stashes/
worktree-generation dataset mask and
uses ordinary State framing. ADD/REPLACE carry complete discriminated entity
records, PATCH carries an observed revision plus complete replacement, and
REMOVE carries entity kind, key, and removed revision. QUERY_STATE uses a
separate subscription allocated by WATCH_QUERY but otherwise follows the same
State Event/ACK convention. Its nonzero outer `query_subscription_id` MUST
equal the embedded StateEvent `subscription_id`; QUERY_STATE_ACK is a bare
StateAck using that same ID. FETCH carries a bounded ordered refspec list and
PROGRESS carries the operation ID, phase, counters, and UTF-8 detail.

Git state entity bodies are exact typed v1 records, not implementation-owned
bytes. HEAD key `HEAD` carries detached/unborn flags, an optional object ID,
and symbolic target. REF is keyed by raw full ref name and carries resolved,
optional peeled, and optional symbolic-target data. REMOTE is keyed by remote
name and carries default-remote flags plus raw configured fetch/push URLs.
OPERATION key `operation` carries the merge/rebase/cherry-pick/revert/bisect
kind, optional head object, and UTF-8 detail. STATUS is keyed by an encoded
non-root FS WirePath and carries stable index/worktree status enums, conflict
flags, optional content object, and optional old path. Presence bits must
exactly match optional fields. UPSTREAM is keyed by the local branch ref and
preserves upstream ref, gone/count-valid flags, and ahead/behind counts. STASH
uses the little-endian `u32` stash index as key and preserves object, timestamp,
timezone, and raw message bytes. WORKTREE_GENERATION key `worktrees` carries the
order-independent count/digest generation used to refetch WORKTREES. The
layouts and enum values in `git.toml` are
normative, and every body is length-delimited by EntityRecord.

For State publication, the native server projects a STASH message to at most
524,288 raw bytes before encoding and acknowledging the snapshot. This is a
byte truncation, not UTF-8 truncation: stash messages are explicitly raw. Git
snapshot records are then emitted one per Event, and the watched adapter state
is acknowledged only after RESET, SNAPSHOT_BEGIN, every record, and
SNAPSHOT_END have all been sent.

## LSP family

LSP is family `0x0032`, version 1. The server remains the sole LSP client and
projects language intelligence into YAS-native state and query records; JSON-
RPC IDs and UTF-16 positions never cross the wire.

| Class   | Kinds                                                                                                                |
| ------- | -------------------------------------------------------------------------------------------------------------------- |
| Request | OPEN, CLOSE, WATCH, UNWATCH, QUERY, BUFFER_PUT, BUFFER_BEGIN, BUFFER_COMMIT, BUFFER_CLOSE, LIST_SERVERS, STOP_SERVER |
| Event   | STATE, STATE_ACK, CLOSED                                                                                             |

Request kinds are assigned in the order shown (`0x0000` through `0x000a`);
Event kinds are `0x0000` through `0x0002`. Exact fields, enum registries, bounds,
and Transfer content kinds are generated from
`protocol/yas/families/lsp.toml`.

OPEN selects a workspace source union: an existing FS root and relative path,
raw nonempty platform path bytes, or a Terminal handle plus relative suffix
resolved atomically from its live cwd. EXPLICIT mode requires a language and
profile and permits bounded initialization bytes. AUTO_DISCOVER mode requires
those fields empty and may start multiple backends. The request also carries a
workspace-wide diagnostics settle delay (`0` or at most 10 seconds). Its Result
returns the boot-scoped workspace handle, workspace revision, UTF-8 position
encoding, backend count, capability mask, and canonical raw root bytes. A zero
backend count requires the typed no-backend detail extension; a nonzero count
forbids it.

WATCH selects backend state, diagnostics, or buffer overlays. Backend records
publish SPAWNING, INITIALIZING, INDEXING, READY, or FAILED plus progress
(`0..100` or `255` unknown), epoch, refused edit count, RSS, stable backend ID,
capabilities, and the last status message. A stopped backend is removed rather
than represented by another phase. Diagnostics are per-path replacement
records, so a zero count explicitly clears a file. CLOSE is idempotent and
releases workspace watches. CLOSED reports server-initiated loss due to a
vanished root, permission loss, backend failure, or resource pressure; ordinary
connection loss remains a local client lifecycle reason.

Every document target contains a non-root FS path, revision, and BLAKE3-256
content hash. Revision zero selects disk bytes; with it, an all-zero request
hash means snapshot the current bytes atomically when the query is admitted,
while a nonzero hash is an exact disk precondition. A nonzero revision selects
an overlay and requires its nonzero exact hash. Locations, edits, hover targets,
symbols with a path, and diagnostics always return the actual selected hash so
stale bytes cannot be confused with an equal revision number. Edits carry both
an expected revision and expected hash and both are checked atomically.

The native State projection caps a backend's last status message at 65,535
UTF-8 bytes. One diagnostic-file record preserves all of its at most 4,096
diagnostics and their IDs, ranges, severities, and tags, while code, source,
and message strings share a deterministic 524,288-byte UTF-8 budget in record
order. The budget includes no hidden omission convention: exhausted text
fields become empty, but no diagnostic is dropped. Together with the maximum
65,535-byte FS path and fixed per-diagnostic/envelope overhead, this guarantees
that the complete one-record StateEvent fits the common publication policy.
LSP snapshots are emitted one record per Event and an adapter update is
acknowledged only after SNAPSHOT_END.

QUERY has the bounded cursor/credit prefix and a tagged body for DEFINITION,
REFERENCES, HOVER, DOCUMENT_SYMBOLS, WORKSPACE_SYMBOLS, COMPLETION,
CODE_ACTIONS, FORMATTING, RENAME, or SIGNATURE_HELP. Results are typed LOCATION,
HOVER, SYMBOL, COMPLETION, ACTION, EDIT, or SIGNATURE records. Symbol records
carry nesting depth. Signature parameter bounds are UTF-8 byte offsets within
the label. Action records contain typed edits and never an opaque server
command. Positions are zero-based lines and UTF-8 byte columns; ranges have an
exclusive end.

A QueryPage has an inner Core status, page flags, and detail before its cursor.
Non-OK inner status requires INCOMPLETE and a nonempty detail; an OK page has an
empty detail. TRUNCATED is present exactly when the continuation cursor is
nonempty. Page records are inline or use a sensitive server-to-client MESSAGE
Transfer; unknown optional records are skipped and unknown required records
fail. The outer Core Result remains OK whenever this typed QueryPage body is
present. Core CANCEL cancels the underlying LSP request when possible and still
resolves exactly one YAS Result.

BUFFER_PUT creates or replaces a boot-scoped unsaved-buffer resource using an
expected revision and at most 32 KiB of inline UTF-8 document text. BUFFER_BEGIN returns a
staging handle and byte Transfer for a larger replacement; BUFFER_COMMIT seals
it under the expected revision and operation ID only after exact size, BLAKE3,
and UTF-8 validation. Invalid UTF-8 is INVALID and publishes no overlay.
RESET before BUFFER_COMMIT discards the staging handle. Multiple clients see the same
resource and must use CAS; there is no silent last-writer-wins merge.
BUFFER_CLOSE removes that overlay. Rename returns a typed edit plan; applying
it is an FS mutation, keeping language analysis separate from filesystem
commit semantics.

BUFFER_PUT carries at most 32 KiB inline and a nonzero operation ID.
BUFFER_BEGIN carries the expected revision, path, size, hash, and initial
credit and returns a sensitive client-to-server BYTE Transfer plus staging
handle. BUFFER_COMMIT verifies and publishes the staged bytes under its
operation ID; BUFFER_CLOSE is a revision CAS. Buffer identity Results and
State records carry handle, revision, workspace revision, size, and hash.
`LIST_SERVERS(0)` is daemon/home-authority global. It returns every live
backend with opaque, nonzero, boot-scoped `server_handle` and `generation`
values. A backend whose root is open in the requesting session carries that
session's workspace handle; a foreign root carries `workspace_handle = 0`
only in this `ServerList` context. `LIST_SERVERS(nonzero)` validates the
session workspace and returns only backends for that exact root. BACKEND State
records always carry a nonzero session workspace handle.

`STOP_SERVER` requires the exact opaque handle, generation, and operation ID;
the server never exposes or accepts an internal backend reference. An unknown
handle is NOT_FOUND and a generation mismatch for a live handle is STALE.
State ADD/REPLACE carries a complete backend, diagnostics, or buffer entity,
PATCH carries an observed revision and complete replacement, and REMOVE
carries its typed key and removed revision. A BACKEND remove key is exactly
one nonzero `server_handle:u64`; a DIAGNOSTICS remove key is the raw encoding
of one non-root FS WirePath wrapped once by the common `key:bytes_u32`; and a
BUFFER remove key is exactly one nonzero `buffer_handle:u64`.

## KV family

KV is family `0x0033`, version 1. It is a watched persistent byte-key/byte-value
store for extensions and clients, not a second filesystem protocol.

| Class   | Kinds                                                             |
| ------- | ----------------------------------------------------------------- |
| Request | OPEN, CLOSE, WATCH, UNWATCH, GET, STAGE_VALUE, PUT, DELETE, BATCH |
| Event   | STATE, STATE_ACK                                                  |

OPEN selects a byte prefix and returns a namespace handle. WATCH snapshots and
then updates keys in lexical byte order. Values carry a 32-byte BLAKE3 hash,
modification revision, signed Unix-nanosecond modification time, and inline
bytes below the watch threshold. MutationResult likewise carries
`modified_unix_ns:i64`: the committed time for OK, or the current entry time
when a non-OK result publishes current metadata. GET returns inline bytes or
Transfer. STAGE_VALUE returns a byte Transfer and staging
handle for a large value. PUT takes either bounded inline bytes or a sealed
staging handle; PUT and DELETE require operation IDs and optionally an expected
hash/revision. BATCH applies a list of inline or staged values under one store
transaction and returns a result per item; a stale precondition changes
nothing. RESET before the first successful PUT or BATCH use discards the
staging handle. Durable deduplication entries survive a server restart with the store.

## Process family

Process is family `0x0040`, version 1. It starts non-PTY children, exposes the
process catalogue, and represents standard streams as Transfers instead
of defining separate data and ACK messages.

| Class   | Kinds                                        |
| ------- | -------------------------------------------- |
| Request | WATCH, UNWATCH, SPAWN, ATTACH, CONTROL, WAIT |
| Event   | STATE, STATE_ACK                             |

SPAWN executes exact argv and environment bytes without an implicit shell. It
accepts explicit cwd, inherited terminal cwd, FS root/path, session environment,
and optional Surface application handle. Its successful Result returns a
boot-scoped `process_handle`, stdin BYTE Transfer, stdout BYTE Transfer, and
either stderr BYTE Transfer or a merged-stream indication. The operation ID
prevents a lost Result from spawning the child twice.

Catalog records contain argv0, native PID for diagnostics, lifecycle, owner
session, detachable flag, stream offsets, exit record, and retention deadline.
An ordinary process is owned by its spawning session and terminated when that
session disappears. A detachable process survives without watchers and remains
discoverable until its retained exit result expires.

ATTACH returns new stdout/stderr Transfers beginning at the process's current
lifetime offsets; earlier output is explicitly reported as a gap and is not
replayed. At most one attachment owns the stdin Transfer at a time, while any
number may observe output. CONTROL provides portable signal, terminate, kill,
and detach actions with operation IDs. Closing the stdin Transfer half-closes
stdin. WAIT returns the final exit record or TIMEOUT.

The canonical v1 payloads are generated from
`protocol/yas/families/process.toml`. SPAWN carries `[operation_id, flags,
environment_kind, cwd, argv, env, stdout_receive_credit,
stderr_receive_credit, Extensions]`. `argv` and environment keys and values
are length-delimited bytes; only protocol labels are UTF-8. The cwd union is
server default, native path bytes, Terminal handle, or FS root plus byte
components. ATTACH carries the process handle, optional stdin-claim flag, and
fresh output credits. Successful SPAWN and ATTACH Results return one
`StreamBundle`: process identity, three lifetime offsets, stream flags, zero or
one sensitive BYTE descriptor per standard stream, and Extensions. A merged
stderr stream has no stderr descriptor or credit.

Process STATE ADD/REPLACE records are complete `ProcessRecord` values; REMOVE
identifies handle and generation. Exit is a portable kind/reason plus native
code and UTF-8 detail. Every mutating request uses a nonzero 128-bit operation
ID. Spawned children are therefore deduplicated independently of frame request
IDs, which are only connection-local.

Required Process family-limit tag 10 is the nonzero `u32`
`max_mutation_replays` bound. An identical SPAWN replays its byte-identical
successful `StreamBundle` only while the returned process attachment and every
descriptor in that bundle remain live. Reusing the ID for another Process kind
or canonical payload is CONFLICT. Once any returned authority retires, the
retained identical retry is STALE and cannot spawn a second child. Live SPAWN
records are pinned; retired records are evicted oldest first as later distinct
settlements enter the bounded table, whose size never exceeds the advertised
limit. Outside that horizon the client must WATCH to reconcile the catalogue
and use a fresh operation ID instead of retrying the expired SPAWN ID.

## Network family

Net is family `0x0041`, version 1. It relays TCP, UDP, Unix-domain sockets, and
Windows named pipes. It is a raw endpoint family: HTTP, WebSocket, Postgres,
DNS, and every other application protocol remain client code.

| Class   | Kinds                    |
| ------- | ------------------------ |
| Request | OPEN, CLOSE              |
| Event   | DATAGRAM, DATAGRAM_STATS |

OPEN carries one typed endpoint address:

| Address kind   | Address and resulting data mode                                   |
| -------------- | ----------------------------------------------------------------- |
| TCP            | DNS name or IP plus port; Transfer BYTE                           |
| UDP            | DNS name or IP plus port; lossy DATAGRAM Events                   |
| UNIX_STREAM    | filesystem or abstract-namespace address; Transfer BYTE           |
| UNIX_DATAGRAM  | filesystem or abstract-namespace address; lossy DATAGRAM Events   |
| UNIX_SEQPACKET | filesystem or abstract-namespace address; Transfer MESSAGE        |
| WINDOWS_PIPE   | UTF-8 pipe name; Transfer BYTE or MESSAGE after server inspection |

Unsupported address kinds return UNSUPPORTED on the current platform. Unix
filesystem paths are bytes. An abstract address is its own variant and carries
raw name bytes, never a leading-NUL convention. Windows pipe names are UTF-8
local or UNC strings converted to native UTF-16; OPEN reports byte versus
message mode, duplex direction, server instance limits, and maximum message
size.

The OPEN Result contains a session-scoped flow handle, resolved peer metadata,
and the Transfer descriptor for reliable byte/message modes. An optional
16 KiB `early_data` field lets TCP, Unix stream/seqpacket, and named-pipe
clients send the first bytes or one message with OPEN; the server writes it
only after connect succeeds and discards it on failure. TCP may additionally
carry typed TLS client options, SNI, ALPN, and verification mode. TLS is
invalid for every local or datagram endpoint.

The canonical v1 payloads are generated from `protocol/yas/families/net.toml`.
OPEN is `[operation_id, address:bytes_u32, delivery_preference, drop_policy,
reserved, initial_receive_credit, early_data:bytes_u32,
tls_options:bytes_u32, Extensions]`. Reliable endpoints require nonzero initial
receive credit and mark delivery/drop fields not applicable. Datagram endpoints
require zero credit and empty early-data/TLS fields. TCP/UDP addresses contain
a UTF-8 host and nonzero port. Each Unix address explicitly distinguishes raw
filesystem bytes from raw abstract-namespace bytes. A Windows pipe contains a
UTF-8 name and AUTO, BYTE, or MESSAGE preference. TLS contains STRICT or
INSECURE verification, optional SNI, an ordered ALPN byte-string list, and
Extensions; server policy may reject INSECURE.

The successful `NetEndpoint` Result is `[flow_handle, mode, direction,
selected_delivery, max_datagram_payload, server_instance_limit,
max_message_bytes, local_address, peer_address, negotiated_alpn, descriptor,
Extensions]`. BYTE and MESSAGE modes contain exactly one sensitive Transfer
descriptor whose content family is Net. DATAGRAM mode contains no descriptor
and reports NATIVE_DATAGRAM or RELIABLE_TUNNEL plus its maximum payload. CLOSE
carries the flow handle and a nonzero operation ID and aborts both directions;
ordinary Transfer CLOSE retains half-close semantics for reliable streams.

UDP and Unix datagram payloads use one DATAGRAM Event each and are never
coalesced, split, retransmitted by the family, or represented as Transfer
MESSAGE. OPEN reports NATIVE_DATAGRAM when they use the optional transport path
and RELIABLE_TUNNEL when they must ride the ordered link. Per-direction sequence
numbers expose relay drops. Bounded queues drop according to the endpoint's
configured latest/oldest policy, and periodic plus final DATAGRAM_STATS report
sent, received, oversized, and congestive drops.

DATAGRAM is `[flow_handle:u64, sequence:u64, payload:remaining]`, is sensitive,
and forbids generic compression. Sequence numbers are independent in each
direction and assigned before bounded-queue admission, so gaps reveal drops.
DATAGRAM_STATS has a monotonic revision, final flag, per-direction delivered,
oversized and congestive-drop counters, a transport-error counter, and
Extensions.

A datagram OPEN requests NATIVE_REQUIRED, PREFER_NATIVE, or RELIABLE_TUNNEL.
The Result reports the selected delivery and maximum payload after YAS and
transport headers. A payload over that limit is dropped and counted, never
split. This makes the path's MTU and the reliable-tunnel semantic compromise
visible instead of silently changing behavior per datagram.

For a bidirectional Unix datagram flow, the server binds a private ephemeral
local address before connecting: abstract where supported, otherwise a
server-owned temporary filesystem entry removed at close.

Transfer CLOSE half-closes a byte stream or compatible pipe direction. Net
CLOSE aborts the entire flow. Connection loss resets all flows and never
presents a truncated stream as clean EOF.

Required Net family-limit tag 13 is the nonzero `u32`
`max_mutation_replays` bound. An identical OPEN replays the byte-identical
`NetEndpoint` only while its flow handle and every returned Transfer remain
live. Reuse of that operation ID for CLOSE or another canonical OPEN is
CONFLICT. Flow retirement makes a retained identical OPEN STALE; it cannot
open a replacement flow. Live OPEN records are pinned and retired settlements
are evicted oldest first when later distinct operations need room, so the table
never exceeds the advertised bound. After an ID leaves that horizon, the
client must reconcile its flow lifecycle and use a fresh operation ID.

## Channel family

Channel is family `0x0042`, version 1. It supplies named, server-native,
reliable bidirectional messages for RPC, command invocations, actor mailboxes,
and extension coordination without pretending they are terminals.

| Class   | Kinds                                           |
| ------- | ----------------------------------------------- |
| Request | WATCH, UNWATCH, LISTEN, CLOSE_LISTENER, CONNECT |
| Event   | STATE, STATE_ACK, ACCEPT                        |

The watched name registry maps a UTF-8 name to a listener handle, generation
token, metadata, and owning session/extension. LISTEN is session-scoped and
exclusive by name. CONNECT may require the generation token it observed,
preventing a command from landing on a replacement listener after a race.

A successful CONNECT returns a bidirectional MESSAGE Transfer. ACCEPT carries
the peer's channel handle, the corresponding descriptor, and both sides'
bounded metadata. Message boundaries are exact; Transfer credit bounds bytes
and `max_open_messages` bounds incomplete messages. Closing the listener stops
new accepts but not established channels. Transfer CLOSE or RESET ends an
established channel. RPC correlation, streaming results, and application
schemas are libraries inside channel messages, not more YAS frame classes.

The canonical v1 kinds and payloads are generated from
`protocol/yas/families/channel.toml`. WATCH and UNWATCH use the common State
payloads. LISTEN is
`operation_id:[u8;16], name:string_u16, metadata:bytes_u32, Extensions` and
returns `listener_handle:u64, generation:u64, Extensions`. CLOSE_LISTENER uses
that handle and generation. CONNECT adds `initial_receive_credit:u64` and
connector metadata; its Result is a `ChannelEndpoint`. ACCEPT prefixes the
same endpoint with the listener handle and generation.

A `ChannelEndpoint` contains the local and peer channel handles, the peer's
16-byte session ID, listener and connector metadata, and a length-delimited
bidirectional MESSAGE Transfer descriptor. Channel payload Transfers use
content kind 0, version 1, require the sensitive-content extension, and allow a
negotiated nonzero `max_item_bytes` no greater than the 16 MiB hard limit, with
at most 16 incomplete messages. CONNECT's `initial_receive_credit` is a
proposal: the server clamps it through the listener's aggregate ingress budget
and the connector's aggregate egress budget. The resulting initial authority
also bounds `max_item_bytes`, and the CONNECT Result and ACCEPT descriptors
carry the identical bound. The server does not publish ACCEPT if its final
credit grant is smaller than that already-fixed item bound. This guarantees
that a sender with the advertised initial authority can complete one maximum
message, since MESSAGE credit is acknowledged only after complete delivery.

ACCEPT descriptors initially grant the server zero sender credit in the
opposite direction: the receiving client either reserves at least the
descriptor's `max_item_bytes` within its aggregate receive budget and grants
CREDIT, or sends RESET without accepting bytes. Names are nonempty UTF-8
without NUL and at most 255 bytes; each metadata value is at most 64 KiB
(65,536 bytes, carried by the `u32` metadata length). Listener state records
carry an opaque owner session and owner kind, never credentials. Family-limit
extensions advertise name and metadata limits, listeners, channels, pending
connects, message size, open messages, connect timeout, and required tag 9
`max_mutation_replays` as a nonzero `u32`.

An identical LISTEN replays its byte-identical successful listener identity
only while that handle and generation remain registered. Reuse of the
operation ID with another canonical name or metadata is CONFLICT. Listener
close or session teardown makes a retained identical LISTEN STALE and cannot
recreate the old listener. Live listeners pin their records; retired records
are evicted oldest first as later distinct settlements need room, with at most
the advertised `max_mutation_replays` retained. Outside that bounded horizon a
client must WATCH the listener catalogue and use a fresh operation ID.

## Extension family

Extension is family `0x0043`, version 1. It supervises Wasmi and QuickJS
extensions, stores immutable modules by BLAKE3, exposes desired definitions as
state, and gives every running attempt a complete in-process YAS session.

| Class   | Kinds                                                                                   |
| ------- | --------------------------------------------------------------------------------------- |
| Request | WATCH, UNWATCH, OBJECT_BEGIN, OBJECT_COMMIT, DEPLOY, CONTROL, FOLLOW, DISCOVER_COMMANDS |
| Event   | STATE, STATE_ACK, ATTEMPT_CONTEXT, ATTEMPT_OUTPUT                                       |

OBJECT_BEGIN returns an OK disposition of ALREADY_PRESENT or UPLOAD; UPLOAD
includes a byte Transfer and staging handle for the requested hash and size.
OBJECT_COMMIT seals, hashes, validates, and atomically installs the object;
Transfer CLOSE alone never installs it, while RESET before OBJECT_COMMIT
discards the stage. DEPLOY creates or replaces a named
desired definition containing runtime, hash, argv, restart policy, persistence,
and enabled state. Creation requires a zero expected handle, generation, and
definition revision. Replacement atomically matches the nonzero current name,
handle, generation, and definition revision plus operation ID, preventing a
remove/recreate ABA race and duplicate supervisors on retry.

State records expose definition revision, desired state, phase, attempt,
an exact `next_start_unix_ms` backoff deadline (`0` when absent), runtime
limits, object hash, last exit, and registered command
directory revision. Persistent definitions and their deduplication outcomes
survive server restart; runtime memory and open resources do not. More
precisely, only successful committed DEPLOY and CONTROL outcomes are durable:
the server journals the request fingerprint and exact Result body before it
replies. A retry after restart returns that body verbatim, including its
historical boot-scoped generation, even when later mutations make the identity
stale. WATCH is the authoritative way to reconcile the current definition
identity before another CAS mutation. Settled noncommitted failures are kept
in the same bounded replay horizon for the current server boot, but are not
durable and may be evaluated again after a restart.

The required `max_mutation_replays` family limit is the retry horizon for
OBJECT_BEGIN, OBJECT_COMMIT, DEPLOY, and CONTROL settlements and the durable
horizon for successful persistent mutations. If its advertised value is `N`,
an ordinary outcome remains replayable until capacity pressure from later
distinct settlements evicts it from the applicable boot or durable journal.
At capacity, the server evicts the oldest eligible outcome by commit sequence
before recording the new one; replacing an existing entry consumes no
additional slot. After an outcome leaves this horizon, a client must WATCH to
reconcile current state and use a fresh operation ID instead of retrying the
expired one.

OBJECT_BEGIN UPLOAD is resource-qualified within that horizon: an identical
request replays its byte-identical staging handle and Transfer descriptor only
while both remain live. Reuse of its operation ID with another hash or length
is CONFLICT. Successful Transfer CLOSE retires replay eligibility but preserves
the sealed stage for OBJECT_COMMIT. OBJECT_COMMIT, Transfer RESET, expiry, or
session teardown removes the stage; a retained identical retry is STALE and
cannot recreate it. A fully live UPLOAD pins its replay record, while a retired
record becomes an oldest-first eviction candidate. ALREADY_PRESENT returns no
ephemeral authority and follows the ordinary bounded replay rule. The
configured live-stage limit is below the negotiated replay capacity, so
pinning cannot make a new settlement unrecordable.

Each attempt connects through the same YAS framing and HELLO path as a network
client and receives ATTEMPT_CONTEXT after HELLO. It can call every selected
family. The host ABI remains frame send/receive, wait/deadline, clock, and
entropy. The authenticated active attempt emits sensitive client-to-server
ATTEMPT_OUTPUT Events containing one bounded stdout, stderr, or UTF-8 log
record; the server assigns the retained sequence. FOLLOW returns those records
through a MESSAGE Transfer with explicit replay start and gap records.
Extension commands are descriptors in watched state and execute through named
Channel listeners.

The canonical v1 layouts are generated from
`protocol/yas/families/extension.toml`. OBJECT_BEGIN is keyed by operation ID,
BLAKE3 hash, and exact byte length. Its Result is ALREADY_PRESENT or an upload
staging handle plus a sensitive client-to-server BYTE Transfer. OBJECT_COMMIT
is the only operation that seals, rehashes, validates, and publishes staged
bytes. Transfer CLOSE alone never installs an object.

DEPLOY carries an operation ID, the all-zero creation or complete nonzero
expected handle/generation/definition-revision CAS tuple,
definition flags, AUTO/Wasmi/QuickJS runtime, restart policy, durable name,
object hash, raw byte argv, requested runtime limits, and Extensions. CONTROL
is revision-checked and provides stop, start, restart, enable, disable, and
remove. State records contain stable handle/generation, definition revision,
phase, resolved runtime, flags, attempt identities, backoff deadline, command
directory revision, object hash, name, last exit, and applied limits; arguments
are deliberately absent from catalogue state.

FOLLOW names an attempt and output sequence, grants credit, and returns a
sensitive server-to-client MESSAGE Transfer. Each message is a consecutive
batch of stdout, stderr, log, or explicit gap records. Successful FOLLOW is the
native ATTACH decision. CLOSE or RESET of its Transfer by either endpoint,
dropping the follow/subscription, and session loss each performs native
UNFOLLOW and discards queued output without changing extension lifecycle
state. There are therefore no redundant ATTACH or UNFOLLOW request kinds.
DISCOVER_COMMANDS pages
a stable directory revision and returns definition and Channel listener
identities plus bounded UTF-8 descriptors. ATTEMPT_CONTEXT is a sensitive
server-to-client Event delivered after the embedded attempt's HELLO; it carries
the exact raw argv and immutable attempt identity. The attempt otherwise uses
the same selected native YAS families and framing as an external session.
ATTEMPT_OUTPUT is the sensitive reverse-direction Event: its stdout/stderr/log
kind and bytes are bounded by `MAX_OUTPUT_RECORD_BYTES`, LOG is UTF-8, and the
server accepts it only from that session's authenticated active attempt.

## Events family

Events is family `0x0044`, version 1. It controls the process-wide bounded
binary event journal, produces dumps, follows live records, and manages
server-file recordings.

| Class   | Kinds                                                                                                     |
| ------- | --------------------------------------------------------------------------------------------------------- |
| Request | GET_CONFIG, SET_CONFIG, DUMP, START_STREAM, STOP_STREAM, START_RECORDING, STOP_RECORDING, LIST_RECORDINGS |
| Event   | RECORD, GAP, STREAM_STOPPED                                                                               |

SET_CONFIG conditionally replaces ring size and the append-only event-ID
activation set using an expected revision and operation ID. DUMP returns a
byte Transfer containing a self-describing header, catalog, and complete
records. START_RECORDING names a server path and append policy and returns a
boot-scoped recording handle; recordings survive the requesting connection but
not a server restart.

Live records do not use Transfer because observability must never backpressure
the work being observed. RECORD batches complete sequenced records up to the
bulk chunk limit. A slow consumer receives GAP with the exact lost count and
resumes at the oldest retained live record. Ring overwrite count, sequence
gaps, stream loss, bytes written, and terminal file errors remain distinct.
Raw frame, PTY, environment, and content events are disabled by default to
avoid cost and routine secret capture.

The canonical v1 layouts are generated from
`protocol/yas/families/events.toml`. GET_CONFIG returns revision, capacity,
retained bytes and records, overwrite count, next sequence, the complete four
word activation set, and Extensions. SET_CONFIG carries a nonzero operation
ID, expected revision (`0` means unconditional), replacement capacity, and the
complete activation set; stale CAS changes nothing. DUMP grants initial credit
and returns the byte length, BLAKE3 hash, and a sensitive server-to-client BYTE
Transfer descriptor.

START_STREAM chooses retained history or live-only delivery, an optional first
history sequence, and a bounded batch target. RECORD is sensitive, forbids
generic compression, and wraps packed codec `events-v1`: consecutive records
with sequence, monotonic time, stable event ID, required bit, event-specific
`u16` flags, and opaque typed payload. The event-specific flags preserve the
EventLog flags unchanged; they are interpreted only with their event
ID. Unknown optional event IDs are retained/skipped; an unknown required ID
rejects the batch. GAP reports both the exact lost count and first sequence
still available. STREAM_STOPPED reports the final common status.

START_RECORDING uses native path bytes plus HISTORY/APPEND flags and a nonzero
operation ID. Recording records expose handle, running/stopped/failed state,
flags, written records and bytes, live loss, path, and delayed error.
STOP_RECORDING returns final counters; LIST_RECORDINGS enumerates the
process-scoped tasks. Client live streams are session-scoped, while recordings
remain server-owned until stopped, failed and collected, or server shutdown.

Required Events family-limit tag 8 is the nonzero `u32`
`max_mutation_replays` bound. An identical START_STREAM replays its
byte-identical successful stream identity only while that live stream remains
registered. Reusing the operation ID for another Events kind or canonical
payload is CONFLICT. STOP_STREAM, terminal stream failure, or session teardown
makes a retained identical START_STREAM STALE and cannot create a replacement
stream. Live stream records are pinned; retired settlements are evicted oldest
first when later distinct operations need room, and the table never exceeds
the advertised bound. Once an ID leaves the bounded horizon, the client must
use observed STREAM_STOPPED/STOP_STREAM lifecycle state and a fresh operation
ID instead of retrying it.

## Environment family

Env is family `0x0045`, version 1. GET is its only Request. It returns every
server environment entry sorted by raw byte key, plus derived session values
such as the effective server name. Keys and values are byte strings subject to
the process-platform rules; no entry is redacted. The Result uses inline typed
records up to 32 KiB and otherwise a MESSAGE Transfer of bounded record batches.
The environment is fixed for one boot, so there is no watch or write operation.

## Complete-family contract

Every family above is normative for YAS version 1. The referenced existing
domain RFCs remain the source for product behavior and edge cases where this
document does not replace them; their retired opcodes, feature gates, correlation,
and fragmentation mechanics do not carry over. Before YAS ships, each family
must have exact generated Request, Result, Event, record,
and packed-codec schemas; runtime limits; resource/reconnect state machines;
idempotency rules; cancellation and late-failure behavior; Rust/TypeScript
golden vectors; and fuzz targets. An incomplete family blocks the first YAS
release rather than being deferred behind a reserved ID.

## Product capability coverage

This table is a release checklist, not a wire-compatibility map:

| Retired or product capability                                      | YAS delivery                                                                                           |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| HELLO, feature bits, READY, global ACK, FRAGMENT                   | Core HELLO descriptors, per-resource credit, native chunks; no READY or universal fragment wrapper     |
| Terminal create/restart variants and lifecycle notifications       | Shared exact launch records; CREATE and RESTART replay/replace with explicit cutover and operation IDs |
| Terminal catalogue, title, cwd, used rows, exit                    | Terminal watched state                                                                                 |
| Terminal grid, input, mouse, scroll, resize, copy, search, journal | Terminal views, specialized frames, correlated control and bounded query Results                       |
| Client list variants, watch, kick, origin                          | Client watched state and DISCONNECT                                                                    |
| App socket and surface attribution                                 | Surface CREATE_APP_ENDPOINT and `app_handle`                                                           |
| Surface list/update/input/capture/video                            | Surface watched state, per-view controls, stable input enums, timed frames                             |
| Clipboard, primary selection, drag/drop                            | Selection revisioned state, staged MIME content, lazy Transfers                                        |
| Tray icons, menus, notifications                                   | Desktop watched state and revision-checked actions                                                     |
| Compositor audio, viewer devices, portals, MPRIS                   | Media state, leases, portal resources, and timed frames                                                |
| FS sync/fetch/read/search/index/grep/write/op/upload               | FS root/state/query model plus Transfer staging and idempotent commit                                  |
| Git repository state and queries                                   | Git state watches and typed pageable QUERY variants                                                    |
| LSP state, diagnostics, queries, buffers                           | LSP state datasets, typed QUERY, CAS buffer resources                                                  |
| KV watch/get/put                                                   | KV namespace state, staged values, transactional mutations                                             |
| Native process catalogue, stdio, attach, control                   | Process state and byte Transfers                                                                       |
| TCP and UDP relay                                                  | Net typed endpoints, Transfer streams, explicit datagrams                                              |
| Unix sockets and Windows named pipes                               | Net local endpoint variants with native byte/message mode                                              |
| Retired gateway remote list, destination routing, and mux channels | Relay watched routes and one nested YAS byte Transfer per connection                                   |
| Retired gateway font list, metrics, and generated face CSS         | Font watched families, typed face descriptions, and content-addressed byte Transfers                   |
| Browser authentication and transport adaptation                    | Fixed-home WebSocket/WebTransport edge; native read-only `yas.v1` WebRTC share                         |
| Native channels                                                    | Channel name state and bidirectional MESSAGE Transfers                                                 |
| Wasmi/QuickJS extensions and commands                              | Extension desired state, object staging, complete embedded YAS session, Channels                       |
| Binary event journal, dump, live/file recording                    | Events configuration, dump Transfer, lossy sequenced live Events                                       |
| Server environment                                                 | Env bounded snapshot with no redaction                                                                 |

## What YAS does not carry forward

YAS has no:

- global one-byte semantic opcode namespace;
- fixed-size global feature bitmap;
- CREATE/CREATE_AT/CREATE_N/CREATE2 variants;
- CREATED/CREATED_N/CREATE_FAILED variants;
- success-only or uncorrelated Requests;
- silent unknown-Request drops;
- CLIENT_LIST2 packet shape;
- positional optional tails inferred from remaining bytes;
- universal fragment wrapper;
- empty global ACK;
- implicit subscribe-on-create or focus;
- resubscribe-as-keyframe behavior;
- magic resource sentinels;
- session-assigned family IDs;
- mandatory per-family DESCRIBE exchange;
- wire priority label over one ordered link; or
- edge filtering based on numeric family or kind ranges.

## Errors and protocol violations

Connection-fatal errors include:

- bad preface;
- invalid transport or frame length;
- unknown header class or nonzero reserved meta bits;
- frame or decoded payload above the negotiated limit;
- unnegotiated compression codec;
- Event or Result on the reliable link for a family not selected in HELLO;
- malformed, duplicate, or unsolicited Result;
- syntactically malformed known Event on the reliable link; and
- framing or decompression corruption.

After HELLO, an endpoint SHOULD send GOAWAY before closing when the frame is
trustworthy enough. Request-local failures return a Result. Transfer-local
offset, credit, and fragment failures RESET that Transfer. State history gaps
produce STATE RESET, not hidden loss or connection death.

Malformed optional-path datagrams are dropped and counted; failure on the
unreliable path does not terminate the reliable session.

Servers count invalid Events on the reliable link and may close a session that
repeatedly violates resource-local rules there. Diagnostics respect schema
sensitivity and must not turn small invalid inputs into large reflected errors.

## Canonical schema

The source of truth is validated language-neutral data under `protocol/yas/`:

```text
protocol/yas/registry.toml
protocol/yas/families/*.toml
protocol/yas/codecs/terminal-grid-v1.toml
protocol/yas/codecs/surface-*.toml
protocol/yas/codecs/media-*.toml
protocol/yas/codecs/events-v1.toml
```

The schema grammar is the implemented phase-zero contract. It describes:

- static family, class, kind, status, field, extension, and record IDs;
- fixed required layouts and extension tails;
- family and packed-codec versions;
- operation direction and sensitivity metadata;
- size, count, and nesting limits; and
- compatibility constraints.

It does not attempt to encode server business logic or resource ownership.

`cargo xtask protocol` generates:

- Rust and TypeScript constants, registry/header types and codecs, plus the
  shared metadata used by the typed family payload codecs;
- dispatch registries and family catalogue metadata;
- Markdown wire tables;
- sensitivity metadata consumed by diagnostics;
- frame-inspection registry data; and
- shared golden binary vectors.

Generated files are checked in. CI regenerates them and rejects a diff. The
compatibility checker rejects ID reuse, changed required layouts, changed
existing semantics metadata, removed supported versions, and extension-tag
reuse. Semantic review remains human; the generator cannot prove compatibility
or semantic correctness.

## Testing requirements

The checked-in tooling currently enforces a deterministic baseline: schema,
Rust, TypeScript, Markdown, inspection, and vector artifacts must regenerate
byte-for-byte; the retained major-1 baseline rejects incompatible schema
changes; Rust and TypeScript consume the same full-payload vectors and exercise
their registered decoders at proper truncation boundaries; and the standalone
`fuzz/` workspace builds frame, family, and packed-codec libFuzzer entry points.
The TypeScript suite additionally routes a bounded deterministic arbitrary-byte
and valid-seed mutation corpus through every family decoder and packed codec.
Those checks run through `cargo xtask protocol --check`, `cargo test -p
yas-wire`, the TypeScript test suite, and the Nix lint and test tasks.

The following end-to-end matrix remains a release requirement. A generated
codec or vector does not by itself mark the corresponding runtime lifecycle
scenario complete:

1. Rust-to-TypeScript and TypeScript-to-Rust vectors cover every family, kind,
   state record, packed codec, and status shape.
2. Every decoder is tested at every truncation boundary.
3. Unknown optional and required extensions and records are tested.
4. Limits are tested at, below, and above every frame, decode, chunk, message,
   state-window, and aggregate-buffer boundary.
5. Request tests cover out-of-order Results, ID reuse after completion,
   cancellation races, disconnection, and idempotent retries.
6. Transfer tests cover partial chunks, interleaved messages, credit, aggregate
   budgets, half-close, reset, and late failure.
7. State tests prove snapshot/live handoff, replay, RESET, reconnect, and no
   hidden gap or duplicate.
8. Relay tests cover catalogue replacement races, unavailable routes, early
   data, independent nested HELLO negotiation, byte-stream chunking,
   half-close, forced disconnect, outer-session loss, recursive routes, and
   aggregate resource limits.
9. Terminal and Surface tests cover creation, exact input, view independence,
   frame chunks, codec changes, keyframes, ACKs, decoder limits, cross-lane
   Result-before-FRAME publication, and their shared State/Transfer/view
   receive budget: exact rejection, high-water configure growth/shrink,
   retirement reuse, and session cleanup. Terminal
   tests additionally enforce the 36-byte character vector, adaptive
   run/list/bitmap selection, compression threshold, feedback piggybacking,
   exact launch environments, and RESTART replacement and rollback for both
   running and exited generations.
10. Selection, Desktop, and Media tests cover revision races, staged content,
    lazy transfers, portal cancellation, timed-frame loss, and stale actions.
11. Font tests cover catalogue deltas, collection-face extraction, static and
    variable descriptions, exact metrics, stale handles and hashes, policy-
    denied fetches, content-addressed caching, size limits, and corrupted font
    rejection.
12. FS, Git, LSP, and KV tests cover snapshot/query handoff, cursors, CAS,
    staged commit, durable operation retry, and reconnect.
13. Process, Net, Channel, and Extension tests cover byte/message boundaries,
    half-close, detach, Unix socket address variants, Windows pipe modes,
    datagram loss, listener replacement, and supervisor restart.
14. Events and Env tests cover exact raw bytes, ring overwrite, live gaps,
    recording failure, and oversized Transfer results.
15. Frame, extension, record, Result, Transfer, state, and every family codec
    are fuzzed by the Rust libFuzzer targets and the deterministic TypeScript
    arbitrary-byte property corpus; sustained external fuzz campaigns remain a
    release gate.

Tests run over arbitrarily chunked byte streams and native message transports;
datagram-safe Events are also tested under loss, duplication, reordering, and
path-MTU changes. A latency suite verifies that a maximum-size bulk chunk,
including Relay or Font data, cannot indefinitely delay Core, Terminal,
Surface, Media, or Net control. A load suite verifies that all families
together remain within the single aggregate receive budget.

## Implementation status and release gates

There is no compatibility rollout or runtime alias. YAS is an independently
named server, CLI, browser stack, edge, extension platform, and protocol.

### Implemented in the current tree

The implementation inventory now includes:

- the validated language-neutral schema, retained major-1 compatibility
  baseline, deterministic Rust/TypeScript registries and codecs, generated wire
  Markdown and inspection data, shared vectors, truncation tests, and fuzz
  harnesses;
- native preface/HELLO, family selection and limits, Core lifecycle, Transfer,
  State, request correlation, cancellation, mutation replay, aggregate receive
  accounting, and reliable-stream transport adapters;
- native server backends for every family in the static registry, including
  Relay and Font in the home server and the fixed-home browser edge;
- matching CLI, browser, guest SDK, bundled-extension, proxy, share, uplink, and
  inspection paths using typed native YAS; and
- native workspaces whose URL contains only the durable backend session
  ID and whose device attachment order is stored separately.

This is an implementation inventory, not release sign-off. The gates below
remain open.

### Qualification inventory

The current tree closes the former implementation blockers with executable
evidence:

1. **Runtime dependency closure.** The normal workspace and server dependency
   graphs contain no retired wire crate or runtime compatibility adapter.
2. **Composite datagram path.** The public Uplink qualification exercises the
   native sideband end to end under loss, duplication, reordering, congestion,
   live path-MTU changes, sideband loss, and reliable fallback. Lower-level
   route tests enforce eligibility, queue, isolation, and MTU bounds.
3. **Server-wide diagnostics and aggregate ownership.** Core SESSION_INFO and
   `@doctor` expose real session, Relay, and receive-budget counters. The
   simultaneous all-family load gate verifies that every retaining family
   shares one live aggregate ceiling and releases ownership on lifecycle close.

One umbrella gate remains open: run and archive the complete cross-language,
family-lifecycle, transport, browser, extension, Linux/macOS/Windows,
load/latency, and sustained-fuzz matrix in
[Testing requirements](#testing-requirements). Focused and local passing suites
do not waive that final release qualification.

YAS version 1 is releasable only when that complete matrix and the generated
artifacts, dependency audit, diagnostics, and release-test evidence are checked
together.

## Rejected alternatives

### Extend the retired protocol again

A generated registry could prevent another collision, but it cannot make
unsafe existing positional layouts, success-only Requests, or the exhausted
feature mask coherent. YAS keeps the useful wire properties without inheriting
those contracts.

### Dynamic family IDs

They avoid a registry at the cost of session-dependent frame decoding and a
larger handshake state machine. A generated `u16` static registry is simpler
and effectively inexhaustible.

### Tagged fields for every scalar

They make tiny changes easy and every common message larger. YAS uses packed
required layouts and tagged optional tails; semantic changes still require a
kind or family-version change.

### One universal stream or state family

Sharing implementation is useful; erasing byte, message, datagram, media,
terminal-frame, and revision semantics is not. Transfer covers only reliable
bytes and messages. State is a direct family convention.

### Wire lanes over one ordered link

A priority label cannot preempt bytes already written. YAS instead caps bulk
chunks and requires bounded fair scheduling. A transport with real independent
streams may map scheduling classes to them without changing family messages.

### Retired gateway-owned destination routing

Transport paths and a separate mux control protocol make remotes available
only to gateway-aware clients, put the destination catalogue outside HELLO and
state replay, and force the browser-facing process to retain every connector
credential. Relay instead gives all clients one watched catalogue and carries
each destination as a complete, recursively composable YAS link.

### HTTP font routes

Serving a family as generated CSS with base64 data ties fonts to the page
origin, expands the payload, hides individual face metadata, and makes cache
identity depend on a URL rather than bytes. It also serves the edge machine's
fonts when the selected terminal belongs to another server. Font DESCRIBE and
FETCH bind metadata and content hashes to the actual YAS server connection.

### Keep the gateway name

In YAS the component has no destination-routing role. Calling it a gateway
would preserve the old ownership model in operational language and invite
features back into it. **Edge** names where authentication and browser
transport adaptation happen without implying semantic routing.
