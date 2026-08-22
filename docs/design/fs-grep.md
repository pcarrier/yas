# RFC: Project-Wide Content Search

- **Status:** Implemented as native YAS FS family v1
- **Date:** 2026-07-28
- **Companion to:** [fs-search.md](fs-search.md), [fs-watch.md](fs-watch.md)

## Summary

`SEARCH` and `INDEX` find files by _name_. Nothing finds them by
_content_: the only grep in the tree is `yas terminal grep`, which
searches PTY backlog, treating each terminal as a file. This adds the
missing half — one native Request, server-side, that walks a root and returns
matching lines.

Server-side rather than client-side by construction. The `@` index works
because a path list is small enough to ship once and score locally; file
_contents_ are not, so the walk and the match stay where the bytes are
and only hits cross the wire.

The Request independently selects case sensitivity, regex versus literal
matching, whole-word matching, and whether ignored files are included. Literal
mode escapes the query before compiling it. Both modes use the Rust `regex`
crate, so lookaround and backreferences are compile errors rather than silent
mismatches.

## Ignore rules filter by default

`INDEX` prunes ignored files, and by default so does this: on a repo
with build output, the ignored half of the tree is the entire cost.
Measured on a 56 GB checkout, the same query is **11 ms** with ignore
rules and **5.0 s** without — and `rg` shows the same split (16 ms vs
5.6 s), so this is the shape of the problem, not an implementation
artifact.

But the reason to grep is often precisely to find the thing that is _not_
where expected, including generated output or a vendored tree.
`GREP_INCLUDE_IGNORED` widens the walk and marks ignored file records so a
client can dim or group them. Native v1 orders candidate component-vector paths
lexicographically; it does not promise tracked-before-ignored ranking. With the
flag clear, the walker's normal ignore rules prune ignored directories,
including `.git`. With it set, those rules are disabled, so callers that do not
want repository metadata must filter that scope themselves.

Two filters remain, both about _matchability_ rather than relevance:

- **Non-UTF-8 files are skipped.** Match ranges and display lines have UTF-8
  byte-column semantics, so arbitrary binary content is outside this query.
- **Files larger than the negotiated query-byte ceiling are skipped.** Native
  v1's hard ceiling is 4 MiB.

Neither sets `TRUNCATED`. They are _scope_ rules, exactly like pruning
`.git` — a file that cannot usefully match was never a result to clip.
Conflating the two was the first version's mistake: with `target/`
unpruned, any real repo has thousands of files over the size cap, so
every search reported itself as incomplete and the one signal that
should mean "there is more to find" became noise.

## Native YAS contract

`GREP` is Request kind `0x0008` in FS family `0x0030`, version 1. The
canonical layouts are in
[`protocol/yas/families/fs.toml`](../../protocol/yas/families/fs.toml), and the
family contract is in [yas.md](yas.md#filesystem-family).

A Request names an opened `root_handle`, selects literal or regex matching,
case and whole-word behavior, ignored-file policy, result limits, a continuation
cursor, and initial receive credit. An empty query or invalid regex settles with
`INVALID`; a rejected concurrent walk settles with `RESOURCE_EXHAUSTED`.

The Result is a `QueryPage`. It contains typed GREP_FILE and GREP_MATCH records
inline when small and otherwise uses a MESSAGE Transfer. Each file record has a
dense page-local index, root-relative component-vector path, ignored flag, and
match count. Match records name that index and carry zero-based lines, UTF-8 byte
columns, an end-exclusive range, and bounded UTF-8 display text. Multiple hits
on one line remain distinct records, and multiline regex matches retain their
full range. `next_cursor` makes incomplete searches explicit.

Native paths preserve raw platform-name bytes rather than using lossy UTF-8 or
an escaped-path compatibility form. The display text is UTF-8 because it is
presentation data, not path identity. Unknown optional typed records are
skipped; unknown required records fail the page.

## Budgets

There is deliberately no match budget. The only thing allowed to stop a
search early is running out of wire:

| Bound                                      | Native v1 hard ceiling | Result behavior                             |
| ------------------------------------------ | ---------------------- | ------------------------------------------- |
| Records in one query page                  | 8,192                  | continuation cursor and `TRUNCATED`         |
| Encoded bytes in one query page            | 4 MiB                  | continuation cursor or `RESOURCE_EXHAUSTED` |
| Matches per file requested by the caller   | 65,535                 | next cursor when more records remain        |
| Candidate catalogue entries                | 131,072                | `RESOURCE_EXHAUSTED`                        |
| Largest file considered                    | 4 MiB                  | skipped as outside grep scope               |
| Longest display line returned              | 512 B                  | clipped display text; range remains exact   |
| Concurrent FS queries per negotiated limit | 8                      | `RESOURCE_EXHAUSTED` when admission is full |

The byte budget is charged **as matches are found**, not once per file: a
pattern matching most of a large file would otherwise build its whole match
list before a between-files check looked at it, and the
pattern comes from the client.

65,535 matches per file is the Request field's `u16` ceiling. A zero request
uses the page record limit; a nonzero value is applied per candidate file.

The size check happens before a file read. A remaining file is read in full up
to the 4 MiB ceiling and then accepted only if it is UTF-8, so an unpruned walk
still scales with the readable candidate bytes and should be requested
deliberately.

Continuation is exact, as in `INDEX`: a cursor is returned only when a match
that exists
is missing from the response.

## Client behavior

`@yas-run/core` exposes `grep(root, query, opts)` beside `searchFiles`
and `indexFiles`. The UI drives it from a left-dock panel with the two
toggles, debounced per keystroke and cancelled on the next one — the
same shape the switcher's `#symbol` mode uses, because like an LSP
symbol query and unlike the `@` index, every keystroke is a round trip.

Results group by file in native path order, and each file retains its ignored
marker. A row reveals its line
through the existing `setReveal` + `editorAssignment` path, so a grep
hit opens exactly the way a diagnostic or a definition does.

## Security

Request validation (reserved flags, empty query, uncompilable regex,
duplicate request identity) answers `INVALID` before any I/O. The root is any path
the server user can read — the family's posture
([fs-watch.md](fs-watch.md) § Security), unchanged.

Not filtering by `.gitignore` widens what a search can _read_ relative
to `INDEX`, but not relative to the family: `WATCH` and `READ`
already serve any readable path, so an ignored file was never a
protected one. Worth stating explicitly all the same, because it means a
grep can surface the contents of `.env` files that the file picker hides
— which is the intended behavior for a tool searching your own machine,
and a reason not to point a yas server at a tree you would not `cat`.

## Rollout

1. Native schema, Rust codecs, TypeScript mirror, and golden vectors. ✅
2. Server walk (`ignore`-crate based) + dispatch + budgets. ✅
3. `js/core` `grep()`; `js/ui` search panel, toggles, result list. ✅
4. Deferred, with triggers: streaming partial results as the walk
   progresses (trigger: a cold large-tree search feels unresponsive
   before the single response lands); a replace-across-files mutation
   (trigger: demand — and it wants the CAS discipline
   [fs-write.md](fs-write.md) already defines, not a new one); reusing a
   warm FS catalogue to skip re-walking a root already being watched
   (trigger: measured duplicate walk cost).
