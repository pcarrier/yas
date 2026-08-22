# One-shot reads (FS `READ`)

- **Status:** Implemented as native YAS FS family v1
- **Companion to:** [fs-watch.md](fs-watch.md), [fs-search.md](fs-search.md),
  and [fs-grep.md](fs-grep.md)

Read a fixed set of files, once, without a sync session.

## Why it exists

The filesystem family was built for an editor watching a tree. Everything else
it grew — `SEARCH`, `INDEX`, and `GREP` — asks a root resource directly because
_discovery_ has nothing to watch. Reading did not get the same treatment, so
anything wanting a handful of files had two choices: watch a directory it did
not want watched, or spawn `/bin/sh -c 'cat …'`.

The session supervisor took the second one, twice — every `.desktop` file at
startup, and every icon a panel asks for — and the shell brought its own problems
with it: quoting rules for every path, base64 to get bytes back through a text
stream, and `wc -c` to enforce a size limit the protocol should own. `READ`
fills that missing operation.

It grants nothing new. A client with the selected FS family can already read
any file the server user can by opening its directory and fetching it; this is
the same authority in one Request instead of three.

## Native YAS contract

`READ` is Request kind `0x0005` in FS family `0x0030`, version 1. The
canonical layouts are in
[`protocol/yas/families/fs.toml`](../../protocol/yas/families/fs.toml) and the
family contract is in [yas.md](yas.md#filesystem-family).

A Request names an opened `root_handle`, grants initial Transfer credit, and
carries an ordered list of typed questions. Each question has a root-relative
component-vector path and independently asks for stat data, a content hash, a
symlink target, or file content. The Result is a pageable sequence of typed
READ records, inline when small and otherwise sent over a MESSAGE Transfer.
Each record repeats its question index, so partial failures cannot misalign
answers.

Each record carries its own common status, so one unreadable path does not
spoil the rest:

| Status               | Meaning                                                         |
| -------------------- | --------------------------------------------------------------- |
| `OK`                 | content follows                                                 |
| `NOT_FOUND`          | no such path                                                    |
| `IO`                 | permission denied                                               |
| `INVALID`            | the question is invalid, including a content read of a non-file |
| `RESOURCE_EXHAUSTED` | content exceeds the query-byte limit                            |
| `INTERNAL`           | another filesystem I/O failure                                  |

A Result is bounded by the negotiated query-record and query-byte limits.
Native paths are component vectors whose Unix components remain raw bytes;
there is no lossy UTF-8 or escaped-path bridge. A caller which wants a search
path asks ordered content questions and selects the first `OK` record.

### Metadata-only questions

`STAT`, `HASH`, and `LINK_TARGET` questions name the file without carrying its
contents. `STAT` returns the complete native EntryRecord, including kind, size,
mode, revision, modification time, and content hash where applicable.

This is for a caller that only needs to know _where_ something is because it will
hand the path to whoever actually wants the bytes. The session supervisor resolves
icons this way and answers the panel with a path; the panel reads it itself, which
is the difference between artwork crossing a Wasm interpreter and not. A 30 KB
icon had to be base64`d into a JSON string in there, and that cost more than
everything else the panel did.

Muster uses metadata-only reads for path readiness so a Unix socket or other
non-file node can satisfy readiness without an arbitrary delay. The session
supervisor uses ordered questions for icon search paths and reads only the
chosen file.

## `INDEX` behavior used by readers

`INDEX_INCLUDE_FILES` and `INDEX_INCLUDE_DIRECTORIES` independently select the
two result kinds, so a caller can ask for files, directories, or both.
`INDEX_INCLUDE_IGNORED` includes paths hidden by ignore rules and marks them in
the returned records. Results are path records rather than file content and are
paginated by the native query cursor.

The native index walk does not follow symlinked directories. A caller that wants
to cross a link resolves it explicitly and opens or indexes the resulting path;
this keeps a recursive index from silently escaping its root.

## Not in v1

- **No byte ranges.** Every caller so far wants whole files, and a range needs an
  offset, a length, and a rule for a file that changed under it.
- **No directory content reads.** That is `INDEX`, and conflating them would make one
  message answer two shapes.
