# RFC: Filesystem Writes

- **Status:** Implemented as native YAS FS family v1, including the CodeMirror
  6 browser editor
- **Date:** 2026-07-23
- **Companion to:** [fs-watch.md](fs-watch.md), [git.md](git.md), [lsp.md](lsp.md)

## Summary

This is the mutation companion to [fs-watch.md](fs-watch.md) and the write
primitive used by [lsp.md](lsp.md) edit plans. Its goal is a
credible browser IDE: enough to build a CodeMirror 6 editor and a file
explorer on top of yas — content writes with conflict detection, plus
the directory operations a file tree needs.

It deliberately **narrows** fs-watch's suggested shape. Writes are _not_
client-pushed State replacements — that reintroduces
the N-writer version-ownership hazard [lsp.md](lsp.md) closes by never
having it. Writes are Request/Result operations against **disk**: the pull side
of `FETCH` inverted. The server stays
the sole author of every mirror. A write lands on disk, the reconciler
re-indexes, and the change re-enters _all_ mirrors — including the
writer's own — through the existing echo path (fs-watch's per-client
differ). The thin-client invariant holds: a client that only applies
and acks needs zero new code; only a client that _writes_ learns the new
messages.

The model is last-writer-wins on disk, guarded by compare-and-swap on
the content hash fs-sync already maintains. No operational transform, no
CRDT, no client-side buffers, no multi-file transaction — each has an
explicit trigger for a later RFC (§ Out of scope).

## Native YAS contract

Filesystem writes are Request kinds `STAGE_WRITE`, `COMMIT`, and `APPLY` in FS
family `0x0030`, version 1. The canonical layouts are in
[`protocol/yas/families/fs.toml`](../../protocol/yas/families/fs.toml), and the
family contract is in [yas.md](yas.md#filesystem-family).

`STAGE_WRITE` names an opened root and component-vector path, declares the exact
byte length and BLAKE3-256 hash, selects a precondition, and returns a sensitive
BYTE Transfer plus a staging handle. Closing the Transfer only seals the bytes.
`COMMIT` rechecks the precondition, length, and hash, then atomically lands the
file under a nonzero 128-bit operation ID. Reset, expiry, or session loss
discards an uncommitted stage. The operation ID makes a durable retry idempotent
and is echoed through watched state so the writer can recognize its mutation.

Closing an FS root invalidates every uncommitted stage owned by that root. The
server first confirms the correlated `CLOSE` Result on the reliable link, then
emits a sensitive `RESET` for each published upload Transfer and releases its
stage mapping and receive credit. Late Transfer traffic is ignored; immutable
downloads that were already published remain independent of the closed root.

`APPLY` handles bounded inline writes plus `MKDIR`, `REMOVE`, `RENAME`,
`SYMLINK`, and `HARDLINK` as typed batch items. Each item carries its own
precondition and result. Requested all-or-none behavior is accepted only where
the platform can provide it; otherwise the Request settles with `UNSUPPORTED`
rather than pretending to provide a filesystem transaction. `CREATE_PARENTS`
is explicit and valid only for operation kinds which can create a destination.

Preconditions are `ANY`, `ABSENT`, exact entry revision, or exact BLAKE3-256
content hash. A conflict reports the current entry revision, modification time,
and optional hash in a typed Result detail. Paths are root-relative vectors of
raw components; empty, dot, dot-dot, separator-containing, and platform-prefix
components are invalid. Native YAS has no percent-escaped path form.

`YAS_FS_WRITE=0` remains a dispatch gate: mutation Requests settle with `IO`,
while read and watch Requests remain available in the selected FS
family.

### Links

A symlink record contains its raw target plus `BLAKE3(target)`, so target CAS
does not require following the link. A symlink target is stored verbatim and may
be relative, absolute, or dangling. A hardlink source must be a regular file;
platform-specific failures map to common statuses. Neither operation grants
authority outside what the server OS identity already has.

## Conflict model

The server compares an explicit native precondition under the filesystem engine
lock immediately before mutation. `HASH` uses the current BLAKE3-256 content
hash, `REVISION` uses the current entry revision, `ABSENT` provides
create-exclusive behavior, and `ANY` is the deliberate unconditional escape
hatch. A mismatch returns `CONFLICT` with typed current-entry detail; a caller
never has to interpret an all-zero hash sentinel.

mtime-and-size etags can miss a same-size edit inside timestamp granularity. A
content hash is self-verifying, while an entry revision is cheaper when the
caller already holds watched state.

The **yas-vs-external-writer** window is irreducible: no OS offers an atomic
compare-hash-and-rename. YAS closes its own cross-session race by serializing the
check-and-mutate region on the canonical target, including when two roots reach
the same file. Distinct files still proceed independently.

## Atomicity and durability

A **server implementation detail, best-effort per platform, not a wire
guarantee.** This RFC upgrades fs-watch's durability disclaimer to
"atomic-replace best-effort": the wire promises only that a reader sees
the old bytes or the new, never a torn write. A `write_atomic(path,
bytes, mode)` helper lands beside the read primitives in `crates/fssync`
(pure platform code, composing with `resolve_wire_path` as `handle_fetch`
does): temp file in the **same directory** (same filesystem ⇒ atomic
`rename`), write, then rename over the target.

- **Unix:** `O_EXCL` temp, `write`, `rename`; with `DURABLE`,
  `sync_all` then fsync the parent directory, and `F_FULLFSYNC` on macOS
  (plain `fsync` does not flush the drive cache).
- **Windows:** `ReplaceFileW`, or `MoveFileExW(REPLACE_EXISTING |
WRITE_THROUGH)`, same-dir temp, **retrying on sharing violations**
  (indexers and AV hold handles without `FILE_SHARE_DELETE`), falling
  back to in-place truncate only as a last resort.

Conceded cost: rename swaps the inode and breaks hardlinks on every
platform. Acceptable — fs-watch disclaims hardlink identity, and the
watcher watches by path. This is the one place fs-watch's "identical
semantics on three platforms" is genuinely hard; it is kept at the
**wire** level (identical statuses) while the server absorbs the
per-platform divergence.

## Echo and attribution

A successful mutation is reconciled back through FS `STATE` for every watcher,
including its origin. Each mutation carries a nonzero operation ID, and the
resulting ADD, REPLACE, MOVE, or REMOVE record repeats it. This is stronger than
hash-only attribution: byte-identical external writes cannot be mistaken for an
echo, and clients need not retain implementation-owned node identities.

A writer chains rapid mutations from the exact revision or hash returned by the
previous Result, not from a settle-lagging catalogue. Other sessions continue to
see the same authoritative state change.

## Operation set (scope, both directions)

| Op                             | Verdict         | Why                                                                                                                                                                           |
| ------------------------------ | --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| write (CAS)                    | **in**          | the core primitive                                                                                                                                                            |
| mkdir (+ mode)                 | **in**          | empty folders are real fs-sync entries; explorer "New Folder"; mode for `0700`                                                                                                |
| remove (subtree, CAS)          | **in**          | explorer delete; mirrors the REMOVE record; an optional precondition means "delete iff unchanged"                                                                             |
| rename (subtree)               | **in**          | rename _and_ drag-move are one op; surfaces as a `MOVE` record                                                                                                                |
| symlink (create/retarget, CAS) | **in**          | dotfile/workspace layouts are symlink-shaped; content = target bytes makes retarget an ordinary CAS (§ Links)                                                                 |
| hardlink                       | **in**          | `link(2)` is one op the client cannot compose from writes; source must be a regular file (§ Links)                                                                            |
| create-parents                 | **in** (flag)   | drag-move into a fresh path                                                                                                                                                   |
| delete-to-trash                | **out → shell** | XDG/Recycle/`~/.Trash` semantics diverge; a synced trash dir churns. Compose via rename                                                                                       |
| copy / duplicate               | **out (v1)**    | the weakest cut; subtree copy cannot compose client-side without shipping bytes both ways. A future typed `COPY` item is cheap server-side — trigger: duplicate latency hurts |
| touch                          | **out**         | create-empty is a zero-byte write with an `ABSENT` precondition; mtime-touch has no IDE use                                                                                   |
| save-all / txn                 | **out**         | N independent item results, per-file `CONFLICT`; all-or-none is accepted only when the platform supports it                                                                   |

**Multi-file operations get no wire transaction** — the deliberate
stance. Save-all, and applying an [lsp.md](lsp.md) rename plan's `EDIT`
records, are orchestrated as typed APPLY items, each checked against its own
precondition; a non-atomic batch reports exactly which items were applied.
No filesystem offers multi-file atomicity on any of the three platforms,
so a fake commit-or-rollback we cannot honor is worse than the honest
partial-failure UX every editor already shows. A half-applied refactor
is recoverable (re-run or undo per file); a fake transaction is a lie.

## Path validation and security posture

Native FS paths are vectors of length-delimited platform components. The codec
rejects an empty component, dot, dot-dot, NUL, embedded separators, and platform
prefixes before path resolution. There is no decode-order ambiguity and no
percent-escape layer. The server resolves and rechecks the parent beneath the
opened root before mutation; symlink targets are data unless an operation
explicitly acts on a link.

The authority remains the server OS identity, but a root-scoped write API has a
larger accidental blast radius than a typed shell command. That makes component
validation, canonical-parent confinement, bounded stages, and exact
preconditions release requirements. `YAS_FS_WRITE=0` preserves a read-only
deployment by settling mutations with `IO` before staging or filesystem
work.

## Budgets

The server advertises exact FS family limits for roots, watches, path shape,
inline/query bytes, query concurrency, stages, total staged bytes, batch items,
and catalogue entries. Canonical hard maxima include 64 MiB per staged object,
16 stages per session, 256 APPLY items, and 1,000,000 catalogue entries.
Transfer credit bounds staged upload memory; session loss and RESET release it.

## Client surface

`YasFsClient.open()` returns a `YasFsRoot`. The root exposes `stageWrite()`,
`commit()`, and `apply()` beside `fetch()`, `read()`, `search()`, `index()`,
`grep()`, and its watched catalogue. The browser editor stages large content,
commits it with a fresh operation ID, and uses exact conflict detail for
Overwrite, Compare, or Revert. Small structural and inline mutations use
`apply()`. The client never needs to invent or decode a platform path string.

## Buffer and collaboration boundary

Disk truth and LSP overlay truth remain separate. FS preconditions are entry
revisions or content hashes; LSP buffer revisions belong to an opened workspace.
A buffer may survive a path rename, and saving one composes through FS
`STAGE_WRITE`/`COMMIT` or an inline APPLY item. Real-time co-editing and CRDT or
OT state remain outside the filesystem family.

## Out of scope (with triggers)

- **Client buffers / `didOpen`-from-buffer** — disk-truth only. Trigger:
  a browser editor wanting unsaved-buffer diagnostics ([lsp.md](lsp.md)
  names the buffer as an alternate byte source into its single-writer
  projection).
- **OT/CRDT collaborative editing** — last-writer-wins via CAS here; a
  separate buffer-sync bit layered above. Trigger: a real-time co-edit
  product.
- **LSP completion / `workspace/applyEdit`** — stays [lsp.md](lsp.md)'s;
  this RFC supplies the write primitive its rename-apply composes on.
- **Chunked/append write, subtree copy** — triggers in § Operation set.

## Implementation map

1. `protocol/yas/families/fs.toml` defines the native kinds, records, limits, and
   packed layouts; Rust and TypeScript codecs share golden vectors.
2. `crates/fssync` provides canonical component-path resolution, atomic writes,
   live CAS checks, and reconciler hints.
3. `crates/server` dispatches native FS Requests and publishes watched state with
   operation attribution.
4. `YasFsRoot` exposes staged and inline mutation APIs; the CodeMirror pane uses
   the same native path and conflict model.

## Top risks

1. **Path confinement.** A decode-order or intermediate-symlink gap would turn
   a root-scoped write into an arbitrary-path write. Native component
   validation, parent confinement, and the symlink-escape tests are therefore
   release-critical invariants, not optional hardening.
2. **Echo ordering under rapid saves.** A wrong `lastWrittenHash` chain
   yields `CONFLICT` storms or cursor/undo flashes — the IDE's whole
   feel rides on it. The mitigations (chain CAS off the reply, never
   `setValue`) are load-bearing, not decorative.
3. **Windows atomic-replace.** No documented atomic rename-replace;
   sharing violations from AV/indexers; inode/hardlink break. The one
   place three-platform parity is genuinely hard — degrade to documented
   best-effort, keep the wire statuses identical.
