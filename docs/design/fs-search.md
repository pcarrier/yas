# RFC: File Search and the Client-Side Index

- **Status:** Implemented as native YAS FS family v1
- **Date:** 2026-07-27
- **Companion to:** [fs-watch.md](fs-watch.md), [fs-write.md](fs-write.md)

## Summary

The switcher's `@query` mode wants a result list that updates per
keystroke. Two Requests serve it, both one-shot operations on an opened root
with **no watch**:

- `SEARCH` — server-side: walk, fuzzy-score, return the top matches. One round
  trip per query.
- `INDEX` — client-side: the server pages the candidate list once; the client
  scores every keystroke locally with
  zero round trips, caches per (connection, root), and re-pulls on a
  staleness TTL. The fast path, and the only way `@` can use client-local
  recency to break otherwise equal matches.

Both use the same `ignore`-crate walk. Normal ignore rules filter candidates by
default; request flags can include ignored paths. Dotfiles are included,
symlinked directories are not followed, and each PATH record says whether it
is a directory and whether it was ignored. `INDEX` independently selects files
and directories; the TypeScript `indexFiles` facade requests files only.

## Native YAS contract

`SEARCH` and `INDEX` are Request kinds `0x0006` and `0x0007` in FS family
`0x0030`, version 1. The canonical layouts are in
[`protocol/yas/families/fs.toml`](../../protocol/yas/families/fs.toml), and the
family contract is in [yas.md](yas.md#filesystem-family).

Both Requests name a boot-scoped `root_handle`, carry an explicit page cursor,
grant initial receive credit, and return a `QueryPage`. Small pages contain
typed PATH records inline; larger pages use a MESSAGE Transfer. `next_cursor`
and the page flags make truncation and continuation explicit. Paths are
root-relative component vectors and preserve raw Unix filename bytes; no lossy
path bridge is part of native YAS.

`SEARCH` returns best matches first, at most the requested and negotiated page
limits. It supports case-sensitive or ASCII-folded matching and subsequence or
prefix mode. Subsequence ranking prefers the narrowest matched span and then
the shorter path; ties use component-vector path order. `INDEX` returns a
stable path-sorted candidate page suitable for local scoring. PATH records
identify directories and ignored entries explicitly.

An unreadable walk returns common `IO`, rather than an authoritative-looking
empty result. Native v1 does not retry an empty filtered walk without ignore
rules; callers that want ignored paths select the explicit include flag.

## Budgets

| Bound                           | Native v1 hard ceiling |
| ------------------------------- | ---------------------- |
| Records in one page             | 8,192                  |
| Encoded bytes in one page       | 4 MiB                  |
| Enumerated candidates per query | 131,072                |
| Concurrent FS queries/session   | 8                      |
| Client file-index retention     | 200,000 paths / 64 MiB |

Page bounds return a continuation cursor and `TRUNCATED`; malformed or
unrepresentable input returns `INVALID`, and candidate or concurrency limits
settle with `RESOURCE_EXHAUSTED`. The concurrency cap is shared by FS `READ`,
`SEARCH`, `INDEX`, and `GREP` Requests in one session.

## Client behavior

`@yas-run/core` exposes `indexFiles(root)` beside `searchFiles`. The UI
(`js/ui/src/ide/fileIndex.ts`) caches one list per (connection, root),
serves every keystroke from it synchronously, and refreshes it in the
background when a lookup finds it older than 60 s — stale-while-
revalidate, so a fresh file appears on the next switcher open without
ever blocking one. The index is the _only_ `@` path: until the list
lands, `@` simply shows nothing, and a continued prefix is served
best-effort. The local scorer mirrors the server's ASCII-folded UTF-8-byte
matching and span/path ordering. Recency is only a tie-break, so equivalent
matches with remembered editor positions rank above cold matches and an empty
`@` is a most-recently-touched list.

## Security

Request validation (reserved flags and duplicate request IDs) answers `INVALID`;
the walk runs off-thread so the connection loop never blocks; teardown
drops the in-flight set with the connection. The root is any path the
server user can read — the family's posture ([fs-watch.md](fs-watch.md)
§ Security), unchanged by these messages.

## Rollout

1. Native schema, Rust codecs, TypeScript mirror, and golden vectors. ✅
2. Server walk (`ignore`-crate based, shared by both Requests) +
   dispatch + native limits. ✅
3. `js/core` `indexFiles`; `js/ui` cache, local scorer, recency boost,
   switcher wiring. ✅
4. Deferred, with triggers: a generation echo for cheap revalidation
   (trigger: re-pull bandwidth shows up in practice on big trees);
   watcher-driven invalidation via the FS shared-root registry
   (trigger: the TTL demonstrably misses fresh files in real use);
   precomputed-lowercase or worker-thread scoring (trigger:
   measured keystroke jank on ≥100k-file indexes); a dirty-input guard
   on the log-spec restore (trigger: a restore observed clobbering
   mid-typing on a slow link).
