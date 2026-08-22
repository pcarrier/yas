# RFC: Server KV Store (CAS)

- **Status:** Implemented as native YAS KV family v1
- **Date:** 2026-07-25
- **Companion to:** [fs-write.md](fs-write.md), [fs-watch.md](fs-watch.md),
  [../protocol.md](../protocol.md), [../ide.md](../ide.md)

## Summary

A small **host-local key→value store on the yas server**, with
compare-and-swap writes and prefix-watch subscriptions. Its first
consumer is the editor: the **list of opened files** and, for each, the
**modified buffer** when one exists — so editors become what terminals
already are: always-on server-backed state that a client _views_, not
state a client _owns_. Reload the tab, connect from another device, or
crash mid-edit: the open files and their unsaved edits are still there,
because they never lived only in the tab.

Before KV, dirty buffers and the open-file list lived only in one browser tab.
A reload depended on best-effort UI autosave, a crash lost unsaved state, and a
save conflict had no durable fallback. Native KV makes those states explicit,
CAS-guarded server data instead of encoding them in tab-local URL state.

KV deliberately remains below a shared-editing engine: it provides a durable,
CAS-guarded byte map while editor semantics stay in the client and LSP buffers
remain an independent native family resource. That keeps the primitive general
enough for layouts, tree expansion, and future bounded panel state without
pulling collaborative-editing policy into the server. The workspace-roots
registry is the second shipped consumer and proves that generality.

The design reuses native family standards wholesale: BLAKE3-256 content-hash
CAS, exact revision preconditions, the common status registry, one Result per
Request, inline-or-Transfer delivery, State subscriptions, and operation-ID
deduplication. A reader who knows the FS family already knows this one.

## Native YAS contract

KV is family `0x0033`, version 1. The canonical kinds, layouts, records, and
limits are generated from
[`protocol/yas/families/kv.toml`](../../protocol/yas/families/kv.toml); the
family contract is in [yas.md](yas.md#kv-family).

`OPEN` selects a raw-byte prefix and returns a boot-scoped namespace handle plus
the current store revision. Keys used through the handle are nonempty relative
byte strings; the full key is bounded to 256 bytes. `WATCH` returns a State
subscription whose staged snapshot and later ADD, REPLACE, and REMOVE records
carry BLAKE3-256 hashes, value length, modification revision/time, and optional
inline content. State ACK credit bounds retained delivery; re-opening and taking
a fresh snapshot is the complete recovery path.

`GET` returns one value inline or over a sensitive BYTE Transfer. `STAGE_VALUE`
returns a client-to-server BYTE Transfer and staging handle for content above
the 32 KiB inline limit. Closing that Transfer only seals the bytes; RESET,
expiry, successful consumption, or session loss retires the stage.

`PUT`, `DELETE`, and `BATCH` are mutations with nonzero 128-bit operation IDs.
Their precondition is `ANY`, `ABSENT`, exact revision, exact hash, or both hash
and revision. Values are inline or reference a sealed stage. Results always
report typed status plus current/committed revision, time, BLAKE3-256 hash, and
length. `BATCH` returns one result per item under one store revision. Durable
mutations and their deduplication outcomes survive restart.

The server advertises limits for key/value/inline bytes, entries, total store
bytes, namespaces and stages per session, staged bytes, and batch items.
`YAS_KV=0` leaves the selected family visible but settles operations with
`UNAVAILABLE`, making policy refusal distinct from a missing family.

## Storage

The KV store is YAS's first server-owned at-rest state. Terminals survive a
client disconnect but not a reboot; durable KV mutations survive both:

- **One [redb](https://github.com/cberner/redb) database** at the platform
  state path `yas/instances/NAME/kv.redb` (`NAME` defaults to `default`;
  `YAS_KV_PATH` overrides), isolating browser settings and extension state
  such as `@session` intent. The initial standalone table is the byte-keyed
  `kv_v1`, mapping raw key bytes to
  `[mtime_ns:u64le][modification_revision:u64le][value…]`; bounded
  operation replays use byte-keyed `kv_operations_v1`, whose 16-byte operation
  IDs map to
  `[YKO1:4][settlement_sequence:u64le][fingerprint:32][store_revision:u64le]`
  `[result_count:u16le][stage_witness_count:u16le][58-byte results…]`
  `[50-byte stage witnesses…]`. redb is the tree's first embedded-storage
  dependency, taken deliberately: pure Rust, actively maintained, a
  stable file format, and a copy-on-write B-tree whose atomic commits
  mean a crash sees the old store or the new, never a torn one. An
  earlier draft persisted value-per-file with percent-escaped-key
  filenames and zero dependencies; it died on arithmetic — a legal
  256-byte wire key whose escaped form exceeds `NAME_MAX` is an
  illegal filename, and a storage layer that can refuse a legal key is
  a bug wearing a design's clothes. redb has no key-encoding problem
  to solve.
- **`DURABLE`** maps to an immediate (fsynced) commit; the default
  commit is eventual-durability — the same latency-over-durability
  default as fs-write, here as a per-commit redb knob rather than a
  temp-file fsync.
- **An in-memory map** (key → value, hash, mtime) is loaded once at
  startup — hashes are not persisted; BLAKE3-256 recomputes at memory
  speed over a ≤ 256 MiB store — and is the source of truth for CAS
  and watches; redb is its write-behind, commits riding a dedicated
  writer thread fed in mutation order (queued mutations batch into one
  transaction, so a `DURABLE` commit's fsync also hardens everything
  ordered before it, and a non-`DURABLE` put is acked as soon as the
  in-memory mutation lands). All mutations serialize on one store
  lock, so the compare-hash-and-write section is trivially race-free
  server-side (the [fs-write.md](fs-write.md) yas-vs-external window
  does not exist: the server is the only writer of its own database,
  and an external mutator of it is out of contract).

Conceded cost of the engine: `ls`/`cat` no longer debug the store —
`yas kv ls|get` is the inspection tool — and the state is one file,
one basket. The copy-on-write commit discipline is what makes the
basket acceptable; a backup is one `cp` of a crash-consistent file.

Entries persist across server restarts — deliberately _more_ durable
than terminals. A parked buffer surviving reboot is the feature; a PTY
surviving reboot is impossible. There is no TTL and no eviction:
over-budget writes are **refused** (`RESOURCE_EXHAUSTED`), never silently evicted —
an evicted "unsaved buffer" is data loss wearing a cache's clothes
(§ Budgets; the honest-refusal stance of
[fs-write.md](fs-write.md) § Operation set).

## First consumer: editor state

Two key families, all values minted by the client. The store neither
parses nor validates them — these shapes are a `js/ui` convention
documented here, not wire schema.

**Keys embed the absolute path and nothing else.** The client's own
maps key on `(connectionId, path)`, but the connectionId is a
client-local remote _name_ — two clients reach the same host under
different names, and the store is already per-host. Embedding a
connection name in a key would silently shard the state this RFC
exists to share; the connection identity is implicit in which server
you asked.

**`editor/open/<abs-path>`** — presence = this file is open somewhere.
Value: small JSON `{ "at": mtime, "cursor": [line, col], "scroll":
top }`. Written with the `ANY` precondition — two tabs updating cursor metadata may race
and last-writer-wins is correct (both agree the file is open, and a
cursor is advisory). Deleted when the last view of the file closes
(dock ✕, tile close without background).

**`editor/buf/<abs-path>`** — present iff the buffer has unsaved edits.
Value: `[ver:1][base:32][content…]` — `ver` = 0, `base` = the **disk
content hash** (`FsNode.hash`) the buffer diverged from, `content` =
the full buffer bytes. Written with **CAS chained off the previous
put** (zero on first divergence — create-exclusive), debounced ~1 s
after the last edit and flushed on the autosave triggers (blur, tab
hide, teardown). Deleted (CAS'd on the last written hash) when a save
lands on disk or the user discards.

The `base` field is what makes restore honest. On editor mount, the
client fetches `editor/buf/<path>`:

- absent → load disk, clean editor (today's path).
- present, `base` == current disk hash → restore the buffer as the
  dirty content; the user is exactly where the crash/reload left them.
- present, `base` ≠ current disk hash → the disk moved under the parked
  buffer. Surface the existing conflict UI (Reload / Overwrite /
  Compare) — the same three-way the CAS save path already owns.

This closes the worst hole in the current model for free: a buffer
whose disk save keeps refusing `CONFLICT` still parks in the store
(the KV put chains on KV hashes, not disk hashes), so "the file
changed under me" stops being a countdown to data loss. The remaining
exposure is honest and small: the debounced put rides the same
fire-and-forget triggers autosave does, so a crash can lose at most
the final debounce window (~1 s) of typing.

**Two namespaces, kept orthogonal** ([fs-write.md](fs-write.md)
§ Forward compatibility, contract 2): the KV layer CASes on hashes of
**KV value bytes**; the `base` _inside_ a buffer value references
**disk** content space. They are never compared to each other. A save
is still an FS mutation with an exact hash precondition against disk — the KV store
never writes files.

**Cross-client behavior falls out.** A second client watching
`editor/open/` sees the first's open files and shows them (the
background dock is the natural landing — they arrive as parked
editors). Two clients editing the same file both put `editor/buf/<p>`;
the CAS chain makes the second put `CONFLICT`, and the client surfaces
it — crude, disclosed, and correct: this RFC parks buffers, it does not
merge them. Real co-editing is the buffer RFC's problem.

**Honest weakness — rename.** Buffer keys embed paths, so an external
`mv` orphans a parked buffer (contract 1's buffer-identity question,
still unforeclosed). A client that observes the fs `MOVE` record may
migrate the key (get → put → delete, each CAS'd); one that doesn't
leaves an orphan that restore never finds. Bounded loss, listed, and
the reason keys are a client convention: the migration needs no server
feature.

## Second consumer: workspace roots

Workspace roots live in the home server's native KV store. This replaced the
retired whole-file configuration endpoint: roots now have CAS, State watch
semantics, and the same bounded retention and durable mutation behavior as
other KV consumers. There is no configuration-WebSocket fallback.

**One key, `roots`, holding the whole ordered list** — the
`name = value` line format omits the remote prefix (a root stored _on_
a server names a path on that server; the remote name was only ever
the client's routing label, the connectionId argument again). Roots
are an _ordered_, human-edited list: per-entry keys would trade one
rare CAS retry for a rank-maintenance scheme, the wrong trade at human
edit rates. Every mutation is read-modify-write CAS'd on the previous
hash; the Roots overlay retries on `CONFLICT` by re-reading — at human
rates the retry is invisible, and two clients editing simultaneously
converge instead of silently dropping one side's edit (the last-writer-wins
hazard of the retired edge-owned scheme).

**Re-scoping is the feature and the conceded cost in one.** Stored
per-server, a root travels with the host: every client that connects
sees the same roots, with no shared browser edge required. The cost: the
picker's list becomes the union over _connected_ servers, so an
offline server's roots are invisible until it connects — defensible
(a root you cannot reach is not actionable) but a real behavior
change, stated.

There is no configuration-WebSocket or retired edge-owned fallback. On first
native initialization, a missing `roots` key simply means an empty list; later
updates use the same revision/hash preconditions as every KV consumer.

## Budgets

The KV family advertises exact limits. Canonical hard maxima are 256-byte keys,
4 MiB values, 32 KiB inline values, 16,384 entries, 256 MiB total store bytes,
16 namespaces per session, 16 stages and 64 MiB staged bytes per session, and
256 batch items. State credit bounds unacknowledged watch delivery and Transfer
credit bounds large values in either direction. Over-limit mutations settle
with `RESOURCE_EXHAUSTED`; stored state is never silently evicted.

## Security posture

The store carries file contents (buffers), so it inherits the fs
family's read posture, and it accepts writes, so it inherits the
write-side gate: **`YAS_KV=0`** leaves KV selected with
`runtime_state = UNAVAILABLE` and settles each Request with `UNAVAILABLE`. The
operator decision is therefore distinguishable from lack of implementation. No
path resolution exists to confine:
keys never touch the filesystem API (they are table entries in one
database, not filenames), so the traversal class fs-write § Path
validation fights cannot arise — the one structural safety advantage
of a flat map. The database and its parent directory are created
`0600`/`0700`. Multi-client visibility is the _point_, and the ceiling
is unchanged: every client that can open the store can already open a
PTY.

**The store is flat across sessions, and that is visible where a server
is shared.** A cloud sandbox runs one server per session, so the
question does not arise there. A local desktop server is one per
`computer.id` and serves every session on the machine, so session B can
watch or fetch session A's keys — including `editor/buf/<abs-path>`,
whose value is an unsaved buffer's contents and whose key is an absolute
path. The access boundary is the same one the already-global PTY table
draws, and both are reachable by anyone holding the connection secret;
what is new is that buffer text now sits **at rest** in a file rather
than only in a live PTY. Accepted deliberately: a per-session namespace
would take away the cross-client visibility this family exists to
provide, and the disclosure is to sessions that could already read the
same files through the fs family. An operator who needs the separation
runs separate servers, or `YAS_KV=0`.

## Client surface

`YasKvClient.open(prefix)` returns a `YasKvNamespace`. The namespace exposes
`get()`, `put()`, `delete()`, `batch()`, and `watch()`. It automatically chooses
inline or staged values, verifies BLAKE3-256 content, assigns operation IDs,
handles State ACK credit, and reports exact conflict metadata through
`YasKvConflictError`. Namespace and watch handles close idempotently.

### Workspace-session consumer

The browser stores each durable workspace as one strict versioned
JSON document under `ui/workspace-sessions/v1/<uuid>`. The document owns the
human name, active Relay route names, layout and stable pane assignments, focus,
and side-panel state. Shared URLs use `#workspace=<uuid>`; the UUID
is a locator, not an authentication capability.

`WorkspaceSessionStore` holds one prefix watch. Create uses absent-CAS, while
rename, semantic state patches, and delete use the last fetched content hash;
all four mutations request durable storage. A conflicting patch refetches and
reapplies only its named fields, so a rename cannot silently erase a concurrent
layout or panel change. A concurrent delete wins and is never converted into an
unconditional put. Oversized, malformed, key/id-mismatched, or over-budget
records are quarantined without removing valid catalogue entries.

Direct attach uses those same entry and aggregate retained-byte budgets. An
invalid replacement keeps its last-good record and attachment when it fits;
the bounded `quarantinedSessionIds` snapshot plus exact `getPresence(id)` query
distinguish quarantine from deletion until the backend record is repaired.
Layout DSL is structurally parsed at admission with a 2,048-pane and 64-level
limit shared by the UI parser.

Individual Relay membership changes use `setRemoteActive` rather than replacing
the whole `activeRemotes` array. Each toggle is reapplied to the latest document
after a CAS conflict, so devices enabling or disabling different routes merge
without losing one another's changes.

The low-level `WorkspaceSessionAttachment` remains a local subscription handle;
it does not claim ephemeral server presence or a lease. Durable frontend
membership is instead one strict device document under
`ui/workspace-session-devices/v1/<device-uuid>`. The browser keeps that UUID in
`yas.workspaceSessionDeviceId` local storage, so tabs on the same browser
share one exact-key watch and one ordered, unique `attachedSessionIds` list.
Attach, detach, and reorder are semantic CAS updates and preserve unrelated
concurrent tab changes. The selected session remains URL/tab-local.

An absent device document means the device has never initialized. Initial
bootstrap uses create-exclusive `claimInitialSession`: simultaneous tabs may
each create a candidate Default session, but exactly one claims the device;
losers select the winning ID and delete their unique orphan candidate. An
existing document with an empty attachment list is intentional and stays empty
on reload. New session documents likewise start with `activeRemotes: []` rather
than inheriting every currently visible Relay route. Pruning IDs deleted from
the session catalogue is durable but conservatively aborts on a device-record
CAS race, preserving a concurrent reattachment for the next reconciliation.

Session deletion invalidates local subscription handles. Link loss closes both
watches, and raw-YAS-backed stores recreate their KV adapters and atomically
replace state only after the next complete snapshot. A store constructed with
a fixed structural KV adapter cannot replace an invalidated transport; its
owner must construct a replacement store.

`js/ui` builds the editor consumer on top: a small `ide/serverState.ts`
owning the debounced buffer puts (hooking the existing autosave
triggers), the open-markers, and the mount-time restore; it follows the
connection-generation/retry discipline every other handle uses. After link
loss, namespace and watch handles are invalidated and reopened only after a new
native HELLO succeeds; puts in flight reject and the debouncer re-fires. Exact
operation IDs and CAS preconditions make the retry safe, and a fresh staged
snapshot replaces the mirror atomically.

CLI: `yas kv get|put|rm|ls [--prefix P] [--if-hash H] [--watch]` —
the store is also a handy host-local scratch space for scripts, which
is not a goal but falls out free.

## Out of scope (with triggers)

- **Server-side buffer engine / co-editing / OT-CRDT** — the store
  parks bytes; it never merges. Trigger: real-time co-edit product
  ([fs-write.md](fs-write.md) § Forward compatibility).
- **LSP `didOpen`-from-buffer** — diagnostics on unsaved parked
  buffers. Trigger: [lsp.md](lsp.md)'s buffer-as-byte-source line.
- **Cross-host sync** — the store is per-server by design; roaming
  state between hosts is a multi-server product question. Trigger: a
  multi-host workspace product.
- **TTL / eviction / compaction** — refusal over eviction in v1.
  Trigger: real deployments hitting `RESOURCE_EXHAUSTED` on legitimate state.
- **Value deltas** — full values only; buffers are small and frame compression
  remains available. Trigger: measured put bandwidth pain (then FS State patch
  semantics are the template).
- **Further UI-state consumers** — roots and workspaces prove the
  pattern; other bounded UI state composes the same way with zero server work.
- **Multi-key transactions** — every consumer so far is one key per
  logical unit (roots deliberately so). Trigger: a consumer whose
  invariant genuinely spans keys; redb has native transactions
  waiting, so the cost then is wire design, not storage.

## Implementation status

1. `protocol/yas/families/kv.toml` defines native Requests, State records,
   staging, mutation results, and limits; Rust and TypeScript codecs share
   golden vectors and validators.
2. `crates/server/src/kv.rs` implements exact hashes, revision/hash CAS, batches,
   watch state, durable redb persistence, operation deduplication, and recovery
   from a truncated tail without a compatibility dispatcher.
3. `YasKvClient` and `YasKvNamespace` implement native inline/Transfer values,
   watches, mutations, conflicts, and idempotent close.
4. Workspaces, device attachment records, roots, and editor state use
   versioned KV namespaces and semantic CAS updates.
5. `yas kv get|put|rm|ls` exposes the same store to scripts.

## Top risks

1. **Restore correctness.** A wrong `base` comparison on mount silently
   resurrects a stale buffer over newer disk content — data loss with a
   UI that looks intentional. The mount flow must treat "base ≠ disk"
   as conflict, never auto-apply. Highest.
2. **Debounce vs. teardown races.** A buffer put in flight while the
   editor tears down and deletes the key (save landed) can interleave;
   the CAS chain makes the outcome safe but the client must not retry a
   `CONFLICT` delete blindly. The `lastWrittenHash` discipline is
   load-bearing here exactly as in fs-write.
3. **First at-rest state, first storage dependency.** The server has
   never owned persistent data; one `kv.redb` file now carries user
   file contents and the roots registry. Deployments that treat yas
   servers as stateless (containers, ephemeral hosts) silently lose
   the durability story — worth a line in server docs, not a design
   change. And the store's health now rides a third-party format:
   mitigated by redb's stable file format and the store's small size
   (a full re-seed from clients is cheap), but a dependency bug is now
   a data bug, which zero-dependency yas has never had before.
