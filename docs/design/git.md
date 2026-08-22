# RFC: Git Introspection

- **Status:** Implemented as native YAS Git family v1
- **Date:** 2026-07-21, revised 2026-07-30 (second pass)
- **Companion to:** [fs-watch.md](fs-watch.md), [fs-write.md](fs-write.md)

## Summary

Clients want to see repositories the way tools see them: which refs exist and
where they point, what happened between two commits, what is staged, what
differs between any two of {commit, tree, index, worktree}, and the bytes of
any object — without shipping a Git implementation to every client or a
`.git` directory over the wire.

The design splits along Git's own grain:

- **Mutable and small** — HEAD, refs, in-progress operation, index/worktree
  status — is _pushed_ as whole-state snapshots, the same philosophy as
  [fs-watch.md](fs-watch.md): the server watches, settles, and streams; the
  client holds a map current by construction.
- **Immutable and large** — commits, trees, blobs, patches — is _pulled_ by
  content address. An oid names its bytes forever, so every response is
  cacheable client-side without invalidation, and nothing needs to stream.

A ref snapshot is a few KiB; the object store is unbounded. Pushing the
first and pulling the second is the only split that bounds both directions.

**Second pass.** The first version shipped and a consumer built on it (the
Review panel in `indent-com/neo#3248`), which surfaced one recurring
failure: the server knew something and the wire did not carry it. A
bounded response could not say where it stopped, a rejection could not say
why, a rename that was not byte-identical read as delete + add, and a
binary file's patch record could not say whether it was added or deleted.
Pass one bounded the wire and got that right; what it under-delivered was
**legibility**. The sharpened rule, which the rest of this document
follows:

> Every bounded response says where it stopped, and every stopping point
> is resumable. Every rejection carries its code. Nothing the server
> computed is dropped between the engine and the consumer.

That pass reshaped the native family before release rather than appending
compatibility tails. HELLO selects an exact Git family version; unsupported
versions are refused before any Git body is decoded. YAS ships server, codecs,
and clients from one version number, and SSH remotes auto-install on first
connection, so skew is bounded by construction.

Two conveniences ride on that split. The server _resolves_ revision
expressions — `main`, `v1.0^`, `HEAD~3`, ranges like `dev..HEAD` — to the
commit oids a walk needs, so clients express intent in Git syntax without
parsing it. And a commit log can be _watched_: the server re-resolves and
re-walks a spec whenever the refs it names move, pushing the fresh page
under the same settle-and-coalesce pacing as state. Watching `main..HEAD`
updates live as either endpoint advances.

## Goals

- Traverse refs, walk commit ranges (`hide..tip`), enumerate trees and the
  index, and fetch blobs — with pagination that keeps the server stateless
  between requests.
- Resolve revision expressions (refs, oids, `HEAD~3`, `A..B`, `A...B`) to
  commit oids server-side, so clients never carry a rev-parser.
- Live-watch a commit log: subscribe to a spec and receive a fresh page
  whenever the refs it names move — no polling, no client-side rev-walk.
- Diff any two of commit / tree / index / worktree: file-level records
  first, render-ready hunk rows on demand — clients display diffs
  without carrying a diff parser.
- Live state: ref moves, HEAD changes, merge/rebase progress, and (opt-in)
  worktree status arrive without polling.
- Thin clients: apply records, cache by oid. No revwalk, no pack access, no
  rename detection client-side.
- Fit native YAS conventions: an exact selected family version, generated
  typed records, explicit pagination, Transfer credit, State ACKs, and
  negotiated budgets.

## Non-goals

Each of these is refused for a reason, not merely unlisted. Where the
second pass took one on, it says so.

- **Mutation**: staging, committing, checkout, and branching remain out.
- **Push and `ls-remote`.** Fetch is in (see [`FETCH`](#native-yas-contract))—
  it was the one remote operation whose absence pushed real work back on
  every consumer, in the form of `git fetch` in a PTY with its exit codes
  screen-scraped off the terminal grid. Push and `ls-remote` are genuinely
  lower value for a read-oriented client.
- **Credentials.** yas stores, parses, and transmits no secret. A fetch
  runs the box's own `git`, which picks up whatever `credential.helper`
  the box's config names — the same thing the PTY workaround relied on.
  Remote URLs go out as configured; see Security.
- **Filter/smudge execution.** Running a `filter.<driver>.clean` program
  means spawning an arbitrary configured binary as a side effect of a
  read, and the read side is deliberately a pure function of the object
  store and the worktree. The two sides of a filtered path are instead
  _flagged_ incomparable (`FILTERED`), which removes the actively
  misleading whole-file rewrite without crossing that line. `text`/`eol`
  normalization, which needs no external program, is applied.
- **Hook execution** and general config access. Two specific config
  values are exposed because nothing else can answer the questions they
  answer: remote names/URLs (Remote State records) and a symbolic ref's target
  (Ref State records). Neither is a key/value surface.
- **Submodule recursion.** Submodules are still separate repositories —
  but a client no longer has to guess where one lives:
  the `OPEN` parent-repository source names it by `(parent, path)` and the server
  resolves the gitdir.

## Native YAS contract

Git is family `0x0031`, version 1. The canonical kinds, payloads, records,
limits, and Transfer content kinds are generated from
[`protocol/yas/families/git.toml`](../../protocol/yas/families/git.toml); the
family-level contract is in [yas.md](yas.md#git-family).

`OPEN` accepts a platform path, an FS root/path pair, a parent
repository/submodule pair, or a Terminal handle plus relative path. It returns a
boot-scoped repository handle, object format, exact worktree and git-directory
paths, revision, and capability flags. `CLOSE` is idempotent. A terminal
`CLOSED` Event reports server-side loss with the last revision and exact reason.

`WATCH` subscribes to selected HEAD, refs, remotes, in-progress operation,
index/worktree status, upstreams, stashes, and worktree-generation state. The
server publishes typed State records with revision/credit semantics; clients do
not parse `.git` or rebuild state from a lossy event log. Reconnect can resume
from a retained repository revision and otherwise receives a staged snapshot.

`QUERY` is a generated tagged union covering `RESOLVE`, `MERGE_BASE`, `LOG`,
`TREE`, `BLOB`, `DIFF`, `PATCH`, `INDEX`, `DISCOVER`, `BLAME`, `REFLOG`, and
`WORKTREES`. Each variant has its own typed input, result records, and cursor.
Bounded pages return records inline or through a sensitive MESSAGE Transfer; a
nonempty continuation cursor is the only indication that more remains. Object
IDs carry their algorithm and exact byte length, so SHA-1 and SHA-256 are both
unambiguous and zero bytes are never a sentinel.

`WATCH_QUERY` turns a ref-dependent query into replacement state under the same
page size and State credit rules. A failed reevaluation publishes typed failure
detail without destroying the subscription, and a later successful replacement
reports recovery. `FETCH` is an idempotent mutation keyed by a nonzero operation
ID; progress Events and its final Result preserve one outcome per remote ref.
Arbitrary Git commands continue to use Process or Terminal rather than an
untyped command tunnel.

Paths reuse FS component vectors and raw platform paths; arbitrary Unix bytes
never pass through a percent-escaped compatibility representation. Required
family limits bound repositories, subscriptions, query records/bytes, concurrent
queries/fetches, ref prefixes, patch spans, and retained revisions.

## Server implementation

A new `yas-git` crate wired into `yas-server`, on **gitoxide** (`gix`):
pure Rust, no C dependency, fits the static and Nix builds; pack access is
mmap-based and fast enough that requests are served directly from
blocking-pool threads. `git2`/libgit2 would work but drags a C toolchain
into every target; shelling out to `git` costs a spawn per request and a
porcelain-parsing layer that this protocol exists to avoid.

Per opened repo, one engine (thread + inbox, the [fs-watch.md](fs-watch.md)
engine shape) owns the Git `STATE` stream. It reuses `yas-fssync`'s
backend hints: a watch on the gitdir (HEAD, `refs/`, `packed-refs`,
`index`, `logs/refs/stash`, `config` (upstream mapping), `MERGE_HEAD`,
`rebase-merge/`, `sequencer/`, `info/`, and the linked worktree's private
dir) drives ref/op/upstream/stash snapshots; with `STATUS`, a watch on the
worktree drives status recomputation through gix's stat-cache-aware
status. Ahead/behind counts memoize by `(tip, upstream)` oid pair,
accelerated by commit-graph generation numbers, bounded by
`YAS_GIT_WALK_MAX` (over budget: `COUNTS_VALID` cleared, never a stall).
One-shot `QUERY` variants do not go through the engine — they are stateless
reads against the object store and index, answered concurrently. `WATCH_QUERY`
is the exception: it registers a subscription
on the engine, which re-resolves the spec and re-walks on each settled ref
change (sharing the gitdir watch above) and pushes Git query state under the
same one-in-flight coalescing pacing as Git `STATE`. A repo opened for
watched logs alone starts a log-only engine — the same thread, with the
Git `STATE` snapshot suppressed.

Every ignore source the status walk reads is watched, wherever it lives —
what counts as untracked is decided by rules, and a rule change that raises
no event leaves the view showing the old answer with nothing to correct it.
In-tree `.gitignore` files ride the worktree watch; `$GIT_DIR/info/exclude`
rides the gitdir watch (`info/` is armed for it, and is redundant only
while the worktree watch already covers a `.git` inside the tree); and the
user's global ignore file — `core.excludesFile`, defaulting to
`$XDG_CONFIG_HOME/git/ignore` — is outside every root, so its _parent
directory_ is armed on its own (a watch on a file follows its inode past
the rename-over an editor performs, the same reason
[fs-watch.md](fs-watch.md) watches parents). That directory is armed for
one file: its siblings are ignored rather than falling into the
"unclassifiable, recompute anyway" case. A `config` change re-resolves the
path and moves the watch with it.

`PATCH` rows come from a plain line diff (`imara-diff`, already in
the tree via gix) with intraline span refinement on modified line pairs —
word- or character-granular, over raw or whitespace-normalized text, per
request flags; binary detection short-circuits to `BINARY`. The row
records are engine-agnostic by design, so a syntax-aware engine can
replace the alignment later, purely server-side.

Nothing runs under the session mutex; Results and Events interleave with
Terminal, Surface, Media, and FS traffic through the per-session writer.
Transfer and State credit bound large answers fairly.

## Relation to filesystem sync

Complementary, and designed to compose: an IDE pane fs-syncs the worktree
for bytes-on-screen, git-watches the repo for decorations, `DIFF`
INDEX×WORKTREE names the dirty files, `BLOB` fetches the base for a
3-way view — each layer answering the question it is authoritative for.
Neither includes the other's data: git state never carries file content;
fs sync never interprets `.git`. The one lockstep piece is on the fs
side: FS `WATCH`'s `WATCH_EXCLUDE_GIT` flag ([fs-watch.md](fs-watch.md), landing
with Git family selection), so a worktree sync doesn't mirror object-store
churn. It is a pure name filter — fs sync still never reads git data.

## Security

Read-only by construction with one named exception (`FETCH`): no other
message mutates the repository, runs a program, or reaches the network.
Discovery honors standard Git layout only; the authority model is
[fs-watch.md](fs-watch.md)'s — the server already hands clients a shell,
so this adds denial-of-service surface, not privilege, and the mitigations
are the budget table, request validation (unknown flags/kinds, NULs,
oversized paths, bad oids rejected), prompt teardown on disconnect, and
never logging raw names as trusted text.

Four specifics worth naming:

- **Remote URLs are emitted as configured**, userinfo included. This is
  deliberate and follows the family's authority model rather than
  defecting from it: the server already hands this caller a shell, so a
  value they can `cat .git/config` for is not a secret the wire is
  keeping, and stripping it would only stop them reproducing the remote.
  The place to be careful is server-side logging, which the rule below
  already covers.
- **Cursors are untrusted.** A path-bearing cursor is decoded with the same
  component, NUL, length, and traversal validation as every Request path. It
  carries no hidden server authority, so a forged cursor can at worst name a
  different valid starting point.
- **Discovery** reveals only what FS `WATCH` plus a loop already reveals,
  bounded by depth, result, and scan caps, and does not follow symlinks
  out of the tree.
- **Fetch** reaches the network and may execute a credential helper — the
  one the box's git config already names, run by a subprocess that is
  exactly the `git fetch` the user's own shell would run, with prompting
  disabled so a missing credential fails reportably instead of hanging.
  Operators who do not want server-initiated egress set
  `YAS_GIT_FETCH=0`, which clears `FETCHABLE` on every open and refuses
  the Request. `ANCHOR` writes refs under `refs/yas/fetch/`, a namespace
  no other tool uses; nothing else in the repository is modified.

## Implementation status

The native migration is complete across:

1. `protocol/yas/families/git.toml` and generated Rust/TypeScript constants,
   with schema validation, packed-record validators, and golden vectors.
2. `crates/git`, whose gitoxide-backed engine consumes and produces native
   semantic types directly. It retains arbitrary-byte paths internally and
   converts only at the explicit FS component boundary.
3. `crates/server`, which adapts native Requests, State subscriptions, query
   pages, progress, and lifecycle Events without a retired-wire adapter.
4. `js/core/src/yas/git.ts`, `YasGitClient`, and the CLI surfaces for status,
   log, diff, show, tree/file listing, merge-base, fetch, and watches.

Git unit and integration suites cover repositories, pagination, state, and
queries; all-target/all-feature clippy and formatting are release gates.

Server-side implementation choices remain replaceable without changing the
family: rename similarity uses YAS's flattened-map scorer; path-follow is exact
rename following; very skewed cross-page topology follows the documented
cursor order; SHA-256 repository use remains subject to gitoxide backend
support even though the native object-ID encoding already supports it.
