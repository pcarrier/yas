# YAS protocol overview

YAS has one native, versioned protocol. It does not expose the retired
directional message-tag space, a parallel adapter wire, or an in-band upgrade
path.

This page is a navigation and implementation overview, not a second wire
manual. The canonical sources are:

- [The YAS v1 design](design/yas.md) for normative session, lifecycle,
  resource, and family behavior.
- [The generated wire registry](../protocol/yas/wire.md) for exact IDs,
  layouts, policies, limits, and packed-codec registrations.
- [The canonical schema](../protocol/yas/README.md) for source files,
  generation, schema-evolution checks, and shared vectors.

Do not copy numeric IDs or payload tables into this document. They are
generated from `protocol/yas/*.toml` into Rust, TypeScript, vectors, inspection
metadata, and the wire registry.

## Session shape

The transport selects YAS before protocol bytes begin: a named local endpoint,
the `yas.v1` WebSocket subprotocol, a WebTransport YAS endpoint, the `yas.v1`
WebRTC DataChannel, or an SSH YAS endpoint. A listener never sniffs for another
wire format.

The client sends the eight-byte YAS preface and immediately follows it with one
uncompressed Core `HELLO` Request. The server's correlated `HELLO` Result
selects family versions, limits, packed codecs, platform properties, and the
optional receive-datagram ceiling. No other operation is legal before HELLO
succeeds.

An ordinary session connects to one server. Additional servers are complete
nested YAS sessions opened through the native Relay family; browser transport
paths and channel numbers do not select destinations.

## Reliable framing

On an ordered byte stream, the preface appears once and every later frame has a
little-endian length envelope:

```text
[frame_len:u32][frame:frame_len]
```

The envelope is not part of the YAS frame. WebSocket uses one binary message
per frame after a standalone preface message, so it omits that length. The
ordered WebRTC `yas.v1` DataChannel is deliberately treated as a byte stream:
its SCTP message boundaries carry no protocol authority and the length envelope
remains end to end. WebTransport's reliable bidirectional stream uses the same
byte-stream form.

Every frame begins with the generated family, kind, class, compression, and
sensitivity header. Requests have one correlated Result. Events are one-way
and carry the sequencing or recovery semantics defined by their family.

## Optional transport datagrams

A reliable session may be paired with an unreliable datagram path:

- WebTransport uses native WebTransport datagrams.
- WebRTC uses an unordered `yas.v1.datagram` DataChannel with zero
  retransmits.

One datagram contains one complete YAS Event without a stream-length envelope.
The sender is bounded by the physical path, the peer's Core HELLO
`receive_max_datagram`, and the generated hard limit. Only Event kinds accepted
by the generated datagram predicate may use this path; datagram frames are
sensitive and uncompressed. Requests, Results, Terminal frames, keyframes,
codec configuration, and other forbidden work stay reliable.

Loss, duplication, and reordering are normal datagram behavior. Each eligible
family owns sequencing, recovery, and statistics. An oversized, malformed,
compressed, non-sensitive, or otherwise forbidden datagram is dropped and
counted without closing the reliable session. If the optional path is absent,
congested, or closed, every family retains its defined reliable fallback.

See [the transport guide](transports.md) for WebTransport URI and WebRTC
channel details.

## Families and resources

Core negotiates a static family registry. Terminal, Surface, Selection, Media,
FS, Git, LSP, KV, Process, Net, Channel, Extension, Events, Env, Font, Relay,
Client, Desktop, and Transfer remain separate semantic namespaces; there is no
global numeric message table.

Resource handles are opaque within one server boot and family. State-bearing
families use revisioned watches, bounded snapshots, patches, acknowledgements,
and explicit reset rules. Mutations that must survive a lost Result use
operation IDs. Bulk bodies use bounded Transfer descriptors rather than an
ad-hoc fragment message shared by unrelated families.

Packed codecs are independently versioned payload codecs selected during
HELLO. Terminal grid, Surface video, Media audio/video, and Events records keep
their domain-specific compact representation without becoming another session
protocol.

## Common status registry

Every Request receives exactly one Result with a generated common status and
optional detail/body. The exact status values and each operation's Result body
are in [the generated wire registry](../protocol/yas/wire.md#statuses).
Family documents define when statuses such as `INVALID`, `UNSUPPORTED`,
`CONFLICT`, `RESOURCE_EXHAUSTED`, `TIMEOUT`, and `STALE` apply; they do not
allocate private status numbers.

## Working directory tracking

The server consumes OSC 7 reports from each PTY and stores the resulting live
working directory in the Terminal resource. Terminal state patches expose cwd
changes to watchers, while the correlated Terminal `CWD` query returns the
stored path through the family's bounded query delivery. Terminal `CREATE` and
`RESTART` launch records can select the server default, exact platform path, or
a snapshot of another Terminal's cwd. Other families refer to a Terminal cwd
by its opaque handle rather than copying a transport-era terminal ID.

See [shell integration](shell-integration.md) for emitting OSC 7.

## Browser renderer boundary

The browser first validates and applies native `yas.terminal.grid/1` frames in
TypeScript. It then serializes one complete semantic grid into a private,
compressed JS-to-WASM renderer snapshot. That snapshot contains no YAS family
header, transport-era message tag, or resource handle. It is renderer-private.

Surface and Media frames likewise enter their typed native family clients
before reaching WebCodecs or audio/render workers. Transport datagrams do not
bypass family validation.

## Changing the protocol

Edit the canonical TOML schema and family implementation, then regenerate with
the workflow in [protocol/yas/README.md](../protocol/yas/README.md). Generated
Rust and TypeScript metadata, the exact wire tables, inspection data, retained
history, and shared vectors are the reviewable stability boundary.

Protocol changes evolve through the canonical schema and family versions, not
parallel public constants or hand-maintained numeric tables.
