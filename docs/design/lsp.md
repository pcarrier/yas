# RFC: Language Intelligence

- **Status:** Implemented as native YAS LSP family v1
- **Date:** 2026-07-23
- **Companion to:** [fs-watch.md](fs-watch.md), [git.md](git.md)

Chosen over two sibling shapes — a raw JSON-RPC tunnel and an LSP-aware
passthrough multiplexer — evaluated against the same criteria; see
§ Alternatives for the comparison.

## Summary

Clients want what tools with a language server see: the errors in a tree
as they appear, and answers at a position — definition, references,
hover, symbols — without shipping an LSP client, JSON-RPC, or UTF-16
position math to every client, and without paying a language server's
multi-minute warmup per query.

The design terminates LSP at the yas server. The server hosts language
server processes the way it hosts PTYs — spawned lazily, shared, warm
across client connections — and is the **sole LSP client** of each: it
owns `initialize`, document synchronization, and every server→client
request. What clients see is a projection into yas-native records along
the family grain established by [git.md](git.md):

- **Mutable and small** — server phase, warmup progress, capabilities —
  is _pushed_ as whole-state snapshots under one-in-flight coalescing.
- **Diagnostics** — mutable and per-file — are _pushed_ as per-file
  replacement sets from a bounded server-held cache, replayed in full to every
  fresh subscriber. Crossing a cache bound advances its generation, discards
  the prior cache, and sends another `FULL` reset before incrementals resume;
  bounded memory therefore cannot leave evicted diagnostics stale at a client.
- **Point-in-time answers** — definition, references, hover, symbols,
  rename plans — are _pulled_ by typed Request/Result.

Every documented failure of LSP sharing — request-id rewriting,
capability intersection, dropped server→client requests, `didChange`
version corruption — exists only when N clients share one LSP stream.
Here N is 1 by construction: yas terminates the protocol, and yas
clients speak records. This is the shape every successful sharer
converged on (Zed, JetBrains Gateway, Live Share); every raw multiplexer
(lspmux, lspd, ra-multiplex) documents the wall this design never hits.

The contract is semantic ("definition at position"), not passthrough: as
Git `PATCH` query rows are a presentation, not a contract with a diff
algorithm, a language could later be answered by tree-sitter or a SCIP
index with no protocol or client change.

## Goals

- One-shot CLI queries against warm shared sessions: every `yas lsp`
  invocation is a fresh connection, so language servers must be
  daemon-owned, keyed by workspace, surviving disconnects — rust-analyzer's
  warmup is paid once, amortized across every future query from every
  client.
- The thinnest possible correct client: apply records, ack. No JSON-RPC,
  no capability negotiation, no position-encoding awareness.
- Diagnostics as reliable state, not a stream: the edit → errors → fix
  loop is the highest-measured-value agent primitive, and it only works
  if a fresh subscriber always receives complete current state.
- Zero config: discover installed servers by root markers and PATH,
  degrade silently when absent, never download anything.
- Read-only by construction: no message mutates the worktree. Rename
  returns its edit plan as data; applying it is the client's business.
- Fit native YAS conventions: an exact selected family version, generated
  typed records, State ACKs, Transfer credit, explicit pagination, and budgets
  that degrade rather than surprise.

## Non-goals

- Applying edits remains outside LSP. Code-action, formatting, and rename
  queries return typed edit plans whose expected revisions and hashes are
  applied through the FS mutation family.
- Semantic tokens, inlay hints, code lens. Continuous decoration
  streams with their own sync and invalidation protocols; worth
  revisiting as the browser editor matures. (Completion and signature
  help, originally deferred alongside them, landed with native buffer overlays —
  § Buffer overlays — once the browser editor existed to consume them.)
- Installing or updating language servers.
- Debuggers and non-LSP tools. A generic supervised-stdio family (DAP,
  REPLs) is a plausible later RFC and should reuse this family's process
  supervisor, but this protocol carries LSP projections only.

## Native YAS contract

LSP is family `0x0032`, version 1. The canonical Requests, Events, records,
limits, and Transfer content kinds are generated from
[`protocol/yas/families/lsp.toml`](../../protocol/yas/families/lsp.toml); the
family contract is in [yas.md](yas.md#lsp-family).

`OPEN` selects a workspace from an FS root/path pair, raw platform path, or a
Terminal handle plus relative path. Explicit mode names a language/profile;
auto-discovery may start several backends. The Result returns a boot-scoped
workspace handle, revision, UTF-8 position encoding, backend count, capability
mask, and canonical root. `CLOSE` is idempotent; `CLOSED` reports server-side
loss with an exact reason.

`WATCH` subscribes independently to backend lifecycle, diagnostics, and buffer
overlays. Backend records carry phase, progress, epoch, capabilities, stable
identity, resource data, and failure detail. Diagnostic records are complete
per-path replacements, including an explicit empty set to clear a file. State
credit and staged snapshots make reconnect and slow-consumer recovery exact.

Every document target combines a component-vector path, revision, and
BLAKE3-256 content hash. Revision zero selects disk truth; a nonzero revision
selects a session-owned overlay and requires its exact hash. Locations, edits,
hovers, symbols, and diagnostics return the hash of the bytes they describe, so
an equal revision number can never hide stale content.

`QUERY` is a typed union for definition, references, hover, document/workspace
symbols, completion, code actions, formatting, rename, and signature help. It
returns bounded typed records inline or through a sensitive MESSAGE Transfer,
with explicit continuation. LSP JSON-RPC IDs, server-private URIs, and UTF-16
position math never cross the family.

`BUFFER_PUT`, `BUFFER_BEGIN`, `BUFFER_COMMIT`, and `BUFFER_CLOSE` manage exact
session-owned overlays with operation IDs, CAS revisions/hashes, staged BYTE
Transfers, and full-document server synchronization. `LIST_SERVERS` and
`STOP_SERVER` expose bounded backend lifecycle using opaque handles and
generations. Required limits cap workspaces, backends, queries, buffers, bytes,
records, edits, locations, completions, symbols, diagnostics, and retained
revisions.

## Sessions and discovery

Backends are **daemon-owned, keyed by `(canonical_root, server_id)`** —
the PTY model, not the fs/git model. Connection-scoped sessions are
absurd against multi-minute warmup; the entire point is that a fresh
one-shot CLI connection attaches to a warm backend in milliseconds. The
registry lives beside the PTY table; attachments hold strong refs, and
a backend with zero attachments starts an idle timer
(`YAS_LSP_IDLE_SECS`) before `shutdown`/`exit` (escalating to kill) —
a deliberate third lifecycle, between fssync's drop-on-last-ref (too
eager for warmth) and the PTY's explicit-close (leak-prone for
processes this heavy).

Discovery is a compiled-in table (~10 entries: `Cargo.toml` →
`rust-analyzer`, `go.mod` → `gopls`, `tsconfig.json`/`package.json` →
`typescript-language-server --stdio`, `pyproject.toml` →
`pyright-langserver --stdio`, `compile_commands.json` → `clangd`, …).
Each entry declares its **root policy** — how the upward marker walk
chooses among nested matches, always bounded above by the git root
(existing gix discovery), which is also the fallback when no marker
matches:

- `outermost` — rust-analyzer: the outermost `Cargo.toml` is the cargo
  workspace; nearest would spawn a backend per member and lose
  cross-crate analysis (Zed's manifest providers make the same call).
- `nearest` — clangd (`compile_commands.json` is per build tree),
  typescript-language-server, pyright.
- gopls — outermost `go.work`, else nearest `go.mod`.

The policy decides `canonical_root`, and therefore backend identity and
sharing. Binaries are probed on PATH at open; absent means absent,
silently — the PipeWire/GPU-dlopen precedent. yas never downloads a
server. Escape hatch: `lsp.<id>.command` / `.args` / `.roots` /
`.root_policy` / `.init` / `.settings` keys in `yas.conf` shadow or
extend the table. `.init` and `.settings` hold **verbatim JSON**,
handed unread to `initializationOptions` and `workspace/configuration`
respectively — yas never validates, interprets, or documents
individual server settings, so per-server schema churn stays outside
yas forever (helix's `config` pass-through is the precedent). The
zero-config default remains empty configuration, which every server in
the table accepts.

**Commands come only from the compiled table or the user's config.**
Repository contents select which entry applies; they never define what
runs. `initializationOptions` and `workspace/configuration` answers
come from config alone.

## Document truth

Disk is the default truth, overridden per document only by an explicit
native buffer overlay (§ native buffer) — and even then versions are
minted by the engine, the single writer, so N-writer version
corruption remains impossible rather than forbidden.

The engine reuses the fssync shared-root watcher (one native watcher
per tree, [fs-watch.md](fs-watch.md)) to feed
`workspace/didChangeWatchedFiles`, honoring dynamic watcher
registrations. Events carry the notify event kind mapped to the LSP
`FileChangeType` — `Created`, `Changed`, or `Deleted` — so a server
that adds a file to its project only on creation (gopls) sees new files
appear; a gone path is always `Deleted`. (FSEvents can coalesce a
create-then-write into one modify, an unavoidable macOS imprecision.)
Because several major servers diagnose only open
documents (typescript-language-server, pyright, clangd), the engine
maintains an **open set** from day one, admitted by three signals:
files recently changed on disk (watcher-dirty), files a subscriber
requested diagnostics for, and files recently queried — LRU-capped
(`YAS_LSP_MAX_DOCS`), `didOpen`ed with disk bytes and re-`didChange`d
(full text) on settled watcher hints, versions minted by the engine.
Dirty-driven admission makes the primary loop work by construction: the
file an agent just saved is opened and diagnosed without ceremony. What
remains partial on open-doc-only servers is cold coverage — a file
never touched in the daemon's lifetime carries no diagnostics, and the
absence of a diagnostics State record means unknown, not clean.

A settled disk write to a file a backend handles is also announced as
`textDocument/didSave`, and the client capability advertises it. This is
load-bearing rather than ceremonial: the servers that diagnose a whole
project do it from an **external checker** (rust-analyzer's flycheck,
gopls' `cargo`/`go build` equivalent) which reruns only on save.
`didChangeWatchedFiles` refreshes their VFS but publishes nothing, and
those servers are deliberately not admitted to the open set (above), so
without `didSave` their diagnostics would freeze at whatever the startup
check produced and never move again for the life of the backend — the
save→errors→fix loop, silently dead. Disk is yas's document truth, so a
settled write is exactly the event `didSave` names. Servers debounce or
coalesce bursts of saves as they see fit; yas's settle window already
batches, and a client holding an native buffer overlay flushes it before
writing so the checker never races the bytes.

Whole-project coverage for these servers has no clean LSP answer, and
that is inherent, not a yas gap. The 3.17 `workspace/diagnostic` pull
would fill cold files without opening them, and the engine can adopt it
where a server advertises `diagnosticProvider.workspaceDiagnostics` — but
the dominant open-doc-only server, typescript-language-server, supports
neither the workspace nor the document pull, and its maintainer closed
the workspace-diagnostics POC as architecturally unworkable (a
full-project tsserver compute "could take minutes and block all other
functionality"). Cycling the whole tree through didOpen/didClose to
force coverage is the same thrash from the client side and was rejected
for the same reason. So whole-project TypeScript diagnostics are the
build tool's job (`tsc --noEmit`), exactly as in VS Code, whose Problems
panel shows only open-file tsserver errors by default. pyright is the
one open-doc-only server that _can_ go whole-project — its
`diagnosticMode: "workspace"` setting, reachable through yas's
`lsp.<id>.settings` pass-through — at the documented cost. Servers that
already diagnose the whole project by construction (rust-analyzer and
gopls via check-on-save) need none of this; `yas lsp diag` is complete
for them once ready — provided the save that drives check-on-save
actually reaches them, which is why the engine sends `didSave`
(§ Document truth).

Intelligence therefore reflects **saved state** for every document
without an overlay — exactly what agents (who write disk) and every
read-only viewer see — and **the editor's live buffer** where one
holds an native buffer overlay: an alternate byte source into the same
single-writer projection, versions engine-minted, the wire carrying
`(path, line, col)` throughout, exactly as this section anticipated
before the browser editor existed.

Every server→client LSP request terminates in yas:
`workspace/configuration` from config (empty defaults);
`client/registerCapability`/`unregister` into an internal table, epoch
bumped in `SERVER`; `window/workDoneProgress/create` + `$/progress`
into phase/percent; `workspace/workspaceFolders` from the root;
`window/showMessage`\* into the `SERVER` msg field and server log;
`workspace/applyEdit` answered `applied:false` and counted — read-only
by construction, [git.md](git.md)'s stance.

## Limits and defaults

HELLO advertises exact LSP family limits. Canonical hard maxima include 64
workspaces per session, 32 watches per workspace, 4,096 query records, 4 MiB
query bytes, 64 MiB per buffer, 1,024 buffers per workspace, 16 upload stages,
4,096 diagnostics per file, 256 server records, and 8 concurrent queries.
Inline and staged buffer bytes retain their exact share of the session-wide
aggregate receive budget after commit. Replacement transfers ownership to the
new revision; buffer close, workspace close, and session teardown release it.

Operational controls additionally bound daemon backends, idle shutdown, spawn
and restart rates, initialization/query timeouts, diagnostic settle time, and
backend RSS visibility. The defaults admit 256 queued engine commands and 256
server-pending queries per backend. Watcher hints coalesce by path behind a
4,096-path default; buffer hints coalesce by attachment/path behind the
64-overlay default. Successful query projections use an 8-job queue by default,
and child stdin uses a 32-frame queue. These operational bounds are finite and
environment-tunable. A full projection queue settles the query with
`RESOURCE_EXHAUSTED`; a full child-writer queue treats the child as wedged and
runs the ordinary bounded restart path.

The corresponding controls are `YAS_LSP_ENGINE_QUEUE_MAX`,
`YAS_LSP_PENDING_QUERIES_MAX`, `YAS_LSP_INGRESS_PATHS_MAX`,
`YAS_LSP_PROJECTION_QUEUE_MAX`, and `YAS_LSP_WRITER_QUEUE_MAX`.

Each backend's diagnostics cache defaults to 4,096 files, 16,384 diagnostics,
and 16 MiB of decoded logical ownership, with the canonical hard limit of 4,096
diagnostics per file. The first three are finite environment-tunable controls.
Cold compression never creates admission room because accounting remains based
on decoded size. Cache overflow advances a generation and forces a
`FULL` resnapshot, including an empty reset when nothing fits. The native
adapter separately retains at most 16,384 diagnostic action contexts and 16
MiB; an evicted diagnostic ID fails a later code-action query as stale.
Cache controls are `YAS_LSP_DIAG_FILES_MAX`, `YAS_LSP_DIAG_ENTRIES_MAX`,
`YAS_LSP_DIAG_BYTES_MAX`, and `YAS_LSP_DIAG_ENTRIES_PER_FILE` (the last still
clamps to the protocol hard maximum).
Exhaustion otherwise settles with `RESOURCE_EXHAUSTED`, publishes a bounded
state failure, or leaves a backend warming; it never silently drops a Result.

## Server implementation

A new `yas-lsp` crate wired into `yas-server`, on **async-lsp** (the
one maintained crate designed for the client role; tower-based, typed
via `lsp-types`) over stdio pipes — no PTY. Per backend, one engine
(thread + inbox, the family shape) owns the LSP session, the open set,
the diagnostics cache, and per-attachment subscriber cursors with their
own outboxes and ack pacing (fssync's reconciler/subscriber split, with
strong refs). Queries route through the engine — LSP sessions are
ordered — but transcoding and record encoding run off the session
mutex; responses interleave with terminal, surface, audio, fs, and git
traffic through the existing per-client writer and Transfer credit
fairness.

Two implementation traps, named now:

- **Reaping.** The engine owns its child outright: it `wait()`s, and
  kills on timeout, on every path. That used to be a race — the daemon's
  5-second backstop drained `waitpid(-1)` and would steal the status from
  under a supervisor doing its own `wait()` (`ECHILD`). The backstop no
  longer touches what it does not own: it sweeps PTY-owned pids only
  (`register_pty_pid` in `yas-server`), so there is no race left to win.
  The corollary is that a subsystem which does not wait its own children
  now leaks zombies rather than being quietly mopped up, which makes the
  engine's every-path `wait()` load-bearing rather than belt-and-braces.
  Windows needs kill-on-drop job objects — the one platform shim.
- **Non-blocking child I/O.** Stdin writes go through a dedicated writer
  thread fed by a 32-frame bounded queue, so a language server that stops
  draining stdin blocks only that thread. Filling the queue marks the session
  failed and kills/restarts the child under the ordinary restart budget; the
  engine loop keeps expiring queries and honoring `STOP_SERVER` throughout.
- **The quirk matrix is the product.** Terminating LSP means every
  server's spec deviation — open-doc-only diagnostics, encoding
  preferences, dynamic-registration timing, nonstandard progress — is
  yas's bug, per server, forever. The mitigations are structural: a
  per-server adapter table in `yas-lsp`, and a scripted fake-LSP-server
  harness so quirk handling is tested deterministically, not against
  whatever rust-analyzer does today. The small projected surface keeps
  the tax bounded; it never disappears.

Platform story: full parity. Pipes, spawn, and the `notify` watcher
work on Linux/macOS/Windows; language servers are cross-platform
binaries the user already has; nothing touches the compositor.

## Alternatives

|                       | raw tunnel (byte channels)             | LSP-aware passthrough                 | projection (this)              |
| --------------------- | -------------------------------------- | ------------------------------------- | ------------------------------ |
| Wire payload          | raw JSON-RPC                           | LSP JSON in yas frames                | yas records                    |
| Client carries        | full LSP client (×2: TS and Rust)      | JSON-RPC + UTF-16 math + URI building | apply records, ack             |
| Sharing               | by convention; one raw attach corrupts | id rewriting, capability intersection | N=1 by construction            |
| One-shot `lsp diag`   | missed-forever notifications           | cache replay works                    | cache replay, `FULL`           |
| Positions             | client's problem                       | UTF-16 on the wire                    | UTF-8 bytes, server transcodes |
| LSP spec churn lands  | in every client                        | in the wire contract                  | in the server engine           |
| Non-LSP backend later | impossible                             | must forge LSP JSON                   | invisible                      |

The tunnel re-runs the documented multiplexer graveyard and breaks the
single most valuable primitive (diagnostics for a client that was not
attached at publish time). The passthrough terminates the right session
layer but makes LSP JSON the wire contract on a protocol whose identity
is no-JSON, and taxes the CLI with UTF-16. Both were rejected; the
passthrough's diagnostics cache and hash-correlation ideas were kept.

## Relation to fs and git

Complementary, composing on hashes and roots: an agent or pane fs-syncs
the worktree for bytes, git-watches for decorations, lsp-subscribes for
squiggles; Location and DiagnosticRecord content hashes join against FS State
content hashes for staleness; root discovery reuses gix; the LSP engine
reuses the fssync shared-root watcher rather than arming a second one.
No family carries another's data.

## Security

Read-only by construction: no message mutates the worktree, applies
edits, or runs repo-defined commands — executables come from the
compiled table or the user's own config, never from repository
contents. The authority model is [fs-watch.md](fs-watch.md)'s: the
server already hands clients a shell, so this family adds
denial-of-service surface, not privilege. Mitigations are the budget
table (including spawn-rate and restart caps against respawn storms),
request validation, prompt teardown of attachments on disconnect, idle
shutdown of backends, and never logging raw paths or server-supplied
text as trusted.

## Implementation status

The native migration is complete across:

1. `protocol/yas/families/lsp.toml` and generated Rust/TypeScript constants,
   with schema validation, packed-record validators, and golden vectors.
2. `crates/lsp`, whose async-LSP supervisor, discovery, JSON-RPC client, text
   model, diagnostics cache, and query engine consume and produce semantic
   native types. Paths remain `Path`/`PathBuf` internally and cross the family
   boundary as raw workspace roots plus FS component vectors.
3. `crates/server`, which adapts native Requests, State subscriptions, buffers,
   query pages, backend catalogues, and lifecycle Events directly.
4. `js/core/src/yas/lsp.ts`, `YasLspClient`, and the CLI surfaces for
   definitions, references, hover, completion, signature help, symbols,
   diagnostics, rename plans, backend listing, and stop.

The scripted fake-server suite makes LSP quirks deterministic; unit tests cover
discovery, text/position conversion, overlays, diagnostics, queries, lifecycle,
and errors. All-target/all-feature clippy and formatting are release gates.

Implementation refinements can remain server-side: identical queries need not
be coalesced, workspace symbols may initially use the first capable backend,
dynamic file-watch registrations may receive a conservative superset, and
completion-item resolve or server-specific trigger characters can be added as
typed record fields without exposing JSON-RPC.
