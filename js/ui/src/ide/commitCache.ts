/**
 * Cache of a commit's message and patch, keyed by connection + repo + oid.
 *
 * A commit is immutable: its oid names its message, its author, and its patch
 * against its first parent, for as long as the object exists. The git family
 * is built on that ("oid-addressed, cache forever" — docs/design/git.md), and
 * this is where a tile takes it up.
 *
 * Without it, moving a commit tile to the dock and back refetched a log page
 * and the commit's whole patch over the wire on every move, because the tile
 * unmounts and remounts (YasTile keys on the assignment string) and its load
 * effect starts from nothing. A commit view is also the one tile whose content
 * cannot go stale, so there is nothing to trade away.
 *
 * Bounded by rows, retained bytes, and entries: one merge across a generated
 * file can outweigh a hundred ordinary commits, while a peer can also rotate
 * empty records or put enormous text in a single row. Eviction is
 * least-recently-used, since a user paging through history revisits what they
 * just looked at. A single response larger than a budget is rendered by its
 * tile but is not retained here.
 */

import type { GitPatchRecord } from "@yas-run/core";

export interface CommitInfo {
  short: string;
  message: string;
  author: string;
  email: string;
  /** Author time (git log's convention for the header line). */
  time: bigint;
  committer: string;
  committerEmail: string;
  committerTime: bigint;
  /** Full hex oids of this commit's parents — two or more for a merge,
   *  none for a root commit. Each opens as its own commit tile. */
  parents: string[];
}

export interface FileDiff {
  newPath: string;
  oldPath: string;
  rows: GitPatchRecord[];
}

export interface CachedCommit {
  commit: CommitInfo;
  files: FileDiff[];
}

/** Patch rows held across all cached commits before the oldest is dropped.
 *  ~200k rows is a few tens of MB of records and covers any realistic
 *  browsing session; the cap only stops an unbounded walk of history. */
export const COMMIT_CACHE_ROW_BUDGET = 200_000;
export const COMMIT_CACHE_BYTE_BUDGET = 32 * 1024 * 1024;
export const COMMIT_CACHE_ITEM_BUDGET = 64;

interface CacheEntry {
  readonly value: CachedCommit;
  readonly rows: number;
  readonly bytes: number;
}

const cache = new Map<string, CacheEntry>();
let rows = 0;
let bytes = 0;

function rowCount(entry: CachedCommit): number {
  let n = 0;
  for (const file of entry.files) n += file.rows.length;
  return n;
}

const stringBytes = (value: string): number => value.length * 2;

/** Conservative retained-size estimate. Typed-array payloads are charged
 * exactly; strings as UTF-16; arrays/objects get fixed overhead so a rotation
 * of empty rows is bounded too. */
function retainedBytes(key: string, entry: CachedCommit): number {
  const commit = entry.commit;
  let total =
    256 +
    stringBytes(key) +
    stringBytes(commit.short) +
    stringBytes(commit.message) +
    stringBytes(commit.author) +
    stringBytes(commit.email) +
    stringBytes(commit.committer) +
    stringBytes(commit.committerEmail);
  for (const parent of commit.parents) total += 16 + stringBytes(parent);
  for (const file of entry.files) {
    total += 128 + stringBytes(file.newPath) + stringBytes(file.oldPath);
    for (const row of file.rows) {
      total += 96;
      if (row.kind === "row") {
        total += row.oldText.byteLength + row.newText.byteLength;
        total += (row.oldSpans.length + row.newSpans.length) * 32;
      } else if (row.kind === "cursor") {
        total += stringBytes(row.after);
      } else if (row.kind === "base") {
        total += row.oid.byteLength;
      }
      if (total > COMMIT_CACHE_BYTE_BUDGET) return total;
    }
  }
  return total;
}

function removeEntry(key: string, entry: CacheEntry): void {
  if (cache.get(key) !== entry) return;
  cache.delete(key);
  rows -= entry.rows;
  bytes -= entry.bytes;
}

function prune(): void {
  while (
    cache.size > COMMIT_CACHE_ITEM_BUDGET ||
    rows > COMMIT_CACHE_ROW_BUDGET ||
    bytes > COMMIT_CACHE_BYTE_BUDGET
  ) {
    const oldest = cache.entries().next().value as
      | [string, CacheEntry]
      | undefined;
    if (!oldest) break;
    removeEntry(oldest[0], oldest[1]);
  }
}

export function commitCacheKey(
  connectionId: string,
  repoPath: string,
  oid: string,
): string {
  // NUL-separated, like every other composite key here: a repo path can
  // contain anything a filesystem allows except this byte.
  return `${connectionId}\0${repoPath}\0${oid}`;
}

/** A cached commit, promoted to most-recently-used, or undefined. */
export function getCachedCommit(key: string): CachedCommit | undefined {
  const hit = cache.get(key);
  if (!hit) return undefined;
  // Map preserves insertion order, so re-inserting is the LRU bump.
  cache.delete(key);
  cache.set(key, hit);
  return hit.value;
}

export function putCachedCommit(key: string, entry: CachedCommit): void {
  const existing = cache.get(key);
  if (existing) removeEntry(key, existing);
  const entryRows = rowCount(entry);
  const entryBytes = retainedBytes(key, entry);
  if (
    entryRows > COMMIT_CACHE_ROW_BUDGET ||
    entryBytes > COMMIT_CACHE_BYTE_BUDGET
  ) {
    return;
  }
  const cached = { value: entry, rows: entryRows, bytes: entryBytes };
  cache.set(key, cached);
  rows += entryRows;
  bytes += entryBytes;
  prune();
}

/** Drop everything for one connection — its oids stop being addressable when
 *  the connection is gone for good, and a workspace teardown should not leak
 *  patches for a box the user has closed. */
export function dropCachedCommits(connectionId: string): void {
  for (const [key, value] of [...cache]) {
    if (key.startsWith(`${connectionId}\0`)) {
      removeEntry(key, value);
    }
  }
}

/** Test seam: the row total the cache is holding. */
export function cachedCommitRows(): number {
  return rows;
}

/** Test/diagnostic seam. */
export function cachedCommitBytes(): number {
  return bytes;
}

/** Test/diagnostic seam. */
export function cachedCommitCount(): number {
  return cache.size;
}
