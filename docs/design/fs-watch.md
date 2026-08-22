# Filesystem State Sync

- **Status:** Implemented as native YAS FS family v1
- **Date:** 2026-07-21

Chosen over four sibling proposals (since removed from the tree) that
streamed filesystem _events_; their best ideas — staged snapshots,
torn-read handling, credit-based ACK, churn bounds, and content hashes — were
adopted here.

## Summary

The rejected proposals streamed _events_ and made the client cope with what
native watch APIs actually are: lossy, coalescing, platform-flavored
invalidation streams. Clients had to track generations, validate sequences,
request resyncs, pair renames, and re-fetch content on mismatch.

This proposal streams _state_. The server maintains a canonical replica of the
watched tree — names, metadata, and content — and sends each client ordered
diffs between the client's last-acknowledged view and the current view. This is
exactly YAS's terminal model applied to a filesystem: Terminal `FRAME` state
and FS `STATE` each describe the delta from what that subscription last
acknowledged.

The consequence is that loss, overflow, races, rename pairing, and recovery
stop being protocol concepts. A native queue overflow, a client that stalls
for a minute, and the initial snapshot are all the same thing on the wire: a
(possibly large) diff, delivered as a staged snapshot when incremental
delivery is not possible. The complete client obligation fits in a dozen
lines:

```text
live = {}; staging = none
on STATE(subscription_id, revision, phase, records):
    if phase is RESET or SNAPSHOT_BEGIN: staging = empty map
    for r in records:                      # into staging if active, else live
        ADD/REPLACE/PATCH → m[r.path] = complete resulting entry
        REMOVE → drop m[r.path] and every path under it
        MOVE   → rename r.from subtree to r.to
    if phase is SNAPSHOT_END: live = staging; staging = none
    send STATE_ACK(subscription_id, revision, credit)
```

The staging map keeps the visible mirror coherent while a snapshot streams in:
applications never observe a half-enumerated tree, and recovery never empties
a UI. Snapshots stream as ordinary bounded updates rather than one giant
message.

A client that does only this is always correct. Everything else — hashing,
delta encoding, rename detection, overflow rescans, snapshot retention,
non-UTF-8 names — is the server's problem, by design. Server cost is higher
than the event-stream proposals and that trade is intentional: one server
implementation, many trivially thin clients (browser panes, CLI agents,
skills, future sync).

## Goals

- The thinnest possible correct client: apply records, ack. No state machine
  beyond a map, no error recovery paths, no platform knowledge.
- Content included, not bolted on: a synced client holds the current bytes of
  every regular file under the root (up to a size limit), kept current via
  server-computed deltas.
- Identical semantics on Linux, macOS, and Windows; native event backends, no
  idle polling.
- Bounded memory on both sides regardless of client speed, without a
  client-visible desync/resync protocol.
- Fit native YAS conventions: selected versioned families, exact generated
  records, explicit State phases, per-subscription credit, and Transfer for
  large byte content.

## Non-goals

- Delivering discrete filesystem _events_. Consumers that want "a file was
  saved" derive it from map transitions (the client library can surface the
  applied records as callbacks — it just applied them). A change that leaves
  state identical (touch-then-revert within one tick) is invisible. This is a
  feature of the model, not an accident.
- Mutation semantics. Client writes are implemented by the same native FS
  family, but their staging, preconditions, idempotency, and conflict behavior
  are specified separately in [fs-write.md](fs-write.md).
- Hardlink identity, xattrs, atime, durability assertions.
- Persisting sync state across connections. Reconnect = new sync = one
  snapshot diff.

## Native YAS contract

Filesystem state is carried by FS family `0x0030`, version 1. The canonical
layouts are in
[`protocol/yas/families/fs.toml`](../../protocol/yas/families/fs.toml), and the
family contract is in [yas.md](yas.md#filesystem-family).

`OPEN` resolves an explicit platform path, a Terminal or Process cwd plus a
component-vector suffix, or the session-owned staging root. It returns a
boot-scoped `root_handle`, canonical path model, case behavior, and limits. All
later paths are root-relative vectors of raw components. A nonrecursive watch
may open a single file directly; no synthetic parent or string split is needed.

`WATCH` returns a State subscription. Its staged snapshot and later ADD,
REPLACE, PATCH, MOVE, and REMOVE records carry exact revisions, metadata,
BLAKE3-256 hashes, symlink targets, and optionally bounded inline content. The
client applies records to staging during snapshot phases, atomically promotes
the completed snapshot, and sends `STATE_ACK` only after applying a revision.
Credit bounds unacknowledged state, and coalescing or RESET recovers from native
watch loss without exposing platform events.

`FETCH` returns one file as inline bytes or a sensitive BYTE Transfer, with an
optional expected-hash precondition. `CLOSE` and `UNWATCH` are idempotent. A
watch backend failure currently ends its State stream without a family CLOSED
reason; connection recovery or an explicit reopen/resubscribe restores it.

The required family limits cap roots, watches, path shape, inline/query bytes,
query concurrency, stages, staged bytes, batch items, and catalogue entries.
There is no global feature bit, family opcode block, universal fragmentation,
or retired escaped-path form.

## Server implementation

The server does more so clients can do less. Three layers, all in a new
`yas-fssync` crate plus wiring in `yas-server`:

**Native hint backends** — inotify on Linux, FSEvents on macOS, overlapped
`ReadDirectoryChangesW` on Windows (v1 reaches all three through the
`notify` crate). They are demoted to producing exactly one thing: a
**dirty set** of paths. Rename cookies and action codes are used only as
locality hints; every native loss signal (`IN_Q_OVERFLOW`, `MustScanSubDirs`,
`ERROR_NOTIFY_ENUM_DIR`, internal channel overflow) degrades to "root is
dirty". No backend behavior is client-visible.

Read-only events are dropped before they become hints. inotify's mask
includes `IN_OPEN`, so on Linux — and only on Linux — opening a watched file
is itself an event; a watcher that reads inside its own tree (this engine
hashing a file, the git engine opening `.gitignore` and `HEAD` to recompute
status, an LSP server reading a document) would retrigger on its own reads
and turn every settle window into a spin loop. `IN_CLOSE_WRITE` and an
unspecified access stay: an extra verification pass is always cheaper than a
lost change. Since FSEvents and `ReadDirectoryChangesW` have no notion of a
read event, the filter removes a platform difference rather than adding one.

**Canonical index** — per synced root, shared and refcounted across clients:
a persistent (structurally shared) tree map `path → (type, size, mtime_ns,
mode, blake3)`. Each settle tick stats the dirty set, updates the index, and
publishes an immutable snapshot; snapshots share unchanged subtrees, so
holding several is cheap. Content lives in a content-addressed **blob store**
(BLAKE3 → bytes, LRU by total size) shared by all syncs — identical files and
unchanged-across-rename files cost one entry, and delta bases are found by
hash. "Efficient but not ultra-optimized" is the bar: a `BTreeMap` clone per
tick with a dirty-subtree copy is acceptable for v1; structural sharing is the
recommended implementation, not a wire requirement.

**Racily-clean entries** — stat cannot always tell whether a file changed.
Inode timestamps come from a clock coarser than the interval between two
writes (Linux's advances once per jiffy), so a rewrite that keeps the size —
`one` → `two`, or any editor saving twice in a millisecond — can leave type,
size, identity _and_ `mtime_ns` all untouched. The index then says
"unchanged" about a file that changed, and since the snapshot is identical
the diff has nothing to emit: the client keeps the old bytes forever.

Git's racy-index rule applies here too. A verified entry whose mtime is
within one coarse granule of now (2 s, FAT's granularity, is the portable
bound) is _unproven_ rather than unchanged, and the reconciler publishes it
as such — an unchanged snapshot plus a **recheck set**, since an unchanged
snapshot is exactly the symptom. Only content syncs can settle it, and they
can: each holds the hash of the bytes it last sent, so it re-reads the file
(just written, so in page cache) and compares. A matching hash emits
nothing; a differing one emits an ordinary content upsert. Metadata-only
syncs, and files past the inline limit, have no client-held bytes that could
be stale and skip the check entirely.

The same 2 s window keeps an unproven hash out of the shared learned-hash
map, so no other sync can serve stale bytes by a hash taken inside it.

**Per-client differ** — each client cursor is one pointer to the snapshot it
last acked, plus its in-flight updates. An update is computed by walking two
snapshots' diff: trivial where subtrees are shared pointers (skip), records
where they differ. **Move detection is a diff-time join**, not event pairing:
entries that disappeared and appeared within one tick with the same file
identity become MOVE records. The current index records `(dev, ino)` on Unix;
platforms without indexed file identity, including Windows, report renames as
Remove plus Add records. Both forms describe the same resulting tree.
Snapshot retention per client is budgeted (default 32 MiB of unshared nodes);
over budget, the cursor is dropped and the client is restarted with RESET and
a staged snapshot.

Nothing here runs under the session mutex; the differ and blob hashing run on
blocking-pool threads and deliver bounded native Events through the normal
per-session writer. State credit and fair transport scheduling keep a large
tree from starving Terminal, Surface, or Media traffic.

## Limits and defaults

The process still uses operational defaults for settle windows, the shared blob
cache, and watcher backends. The wire-visible ceilings are negotiated as FS
family limits. Canonical hard maxima include 64 roots per session, 32 watches
per root, 1,024 path components, 4,096 bytes per component, 65,535 total path
bytes, 32 KiB inline content, 8 concurrent queries, 16 stages, 64 MiB staged
bytes, 256 batch items, and 1,000,000 catalogue entries.

A root which exceeds its negotiated catalogue limit cannot publish an invalid
partial snapshot: its subscription closes with a typed resource failure. On
Linux the server also leaves headroom under `fs.inotify.max_user_watches`.

## Comparison with the rejected event-stream designs

|                    | invalidation streams                                             | verified events + deltas                       | state sync (this)                   |
| ------------------ | ---------------------------------------------------------------- | ---------------------------------------------- | ----------------------------------- |
| Wire model         | event / invalidation stream                                      | verified events + content deltas               | state diffs                         |
| Client must handle | sequences, DESYNC, resync, generations, rename pairing, re-reads | generations, sequences, hash-mismatch recovery | apply + ack                         |
| Loss recovery      | client-driven resync + barrier                                   | server rescan, synthetic events                | invisible (RESET + staged snapshot) |
| Content            | out of scope                                                     | delta stream with ack bases                    | integral, content-addressed         |
| Non-UTF-8 names    | component encoding, client compares bytes                        | lossy or WTF-8                                 | raw component vectors               |
| Server memory      | lowest (no index)                                                | index per watch                                | index + snapshots + blobs (highest) |
| Server CPU         | lowest                                                           | stat verification                              | stat + hash + diff                  |
| Event fidelity     | highest (invalidations per change)                               | high                                           | state transitions only              |

Choose the event-stream designs if consumers need change _notifications_ with
minimal server cost. Choose this design if consumers need the _tree_ — which
is what agents tailing builds, browser file views, and sync features actually
consume — and thin clients matter more than server frugality.

## Security

The server executes FS operations as its OS identity. The mandatory controls are
negotiated resource limits, strict reserved-field validation, prompt teardown on
session loss, sensitive framing for paths and content, and component-wise path
resolution. Raw path bytes are never logged as trusted text.

Every path component has exact byte and count limits and is rejected if empty,
dot, dot-dot, NUL-containing, separator-containing, or a platform prefix. This
keeps traversal out of string normalization and preserves non-UTF-8 Unix names
without an escape convention.

## Implementation

1. `protocol/yas/families/fs.toml` is the canonical native schema; Rust and
   TypeScript codecs share golden vectors and validators.
2. `yas-fssync` owns one canonical shared root per compatible watch definition,
   native dirty hints, snapshot reconciliation, blob caching, and independent
   subscription state. Property tests vary mutations and ACK timing across
   multiple watchers and require convergence on the final tree.
3. Native backends use `notify` (inotify, FSEvents, and
   `ReadDirectoryChangesW`) only as dirty hints; backend semantics never cross
   the family boundary.
4. The CLI and `YasFsRoot` expose open, catalogue/watch, fetch, read, search,
   index, grep, staged write, commit, and apply over the same native family.
