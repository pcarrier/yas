/**
 * Client-side file search index (docs/design/fs-search.md).
 *
 * The switcher's "@" mode wants per-keystroke results, which a server round
 * trip per keystroke can't deliver. The server ships the candidate list once
 * (`FS_INDEX`, gitignore-filtered) and every keystroke scores locally.
 * Cached per (connection, root) with stale-while-revalidate. While the native
 * index is unavailable or still loading, `localFileIndex` stays null and
 * callers keep the server-side `FS_SEARCH` fallback.
 */

import { createSignal } from "solid-js";
import type { YasWorkspace, FsFileIndex } from "@yas-run/core";

type CacheEntry = {
  index: FsFileIndex | null;
  fetchedAt: number;
  inflight: boolean;
  /** Conservative retained cost, including the prepared lowercase form. */
  bytes: number;
};

const cache = new Map<string, CacheEntry>();
const TTL_MS = 60_000;
export const FILE_INDEX_CACHE_MAX_ITEMS = 16;
export const FILE_INDEX_CACHE_MAX_BYTES = 64 * 1024 * 1024;
export const FILE_INDEX_MAX_PATHS = 200_000;
export const FILE_INDEX_MAX_KEY_CHARS = 4_096;
export const FILE_INDEX_MAX_INFLIGHT = 4;
let cacheBytes = 0;
let activeFetches = 0;

function removeEntry(key: string, entry: CacheEntry): void {
  if (cache.get(key) !== entry) return;
  cache.delete(key);
  cacheBytes -= entry.bytes;
  // Release a possibly large index immediately. An in-flight closure keeps
  // only this small entry object and refuses to install its eventual result.
  entry.index = null;
  entry.bytes = 0;
}

function touchEntry(key: string, entry: CacheEntry): void {
  if (cache.get(key) !== entry) return;
  cache.delete(key);
  cache.set(key, entry);
}

function pruneCache(): void {
  while (
    cache.size > FILE_INDEX_CACHE_MAX_ITEMS ||
    cacheBytes > FILE_INDEX_CACHE_MAX_BYTES
  ) {
    // Preserve useful work when rotation adds retry tombstones faster than
    // the four network operations settle. An in-flight entry is evicted only
    // when every retained entry is in flight (the concurrency cap makes that
    // case small).
    const oldest =
      [...cache].find(([, entry]) => !entry.inflight) ??
      (cache.entries().next().value as [string, CacheEntry] | undefined);
    if (!oldest) break;
    removeEntry(oldest[0], oldest[1]);
  }
}

/** Charge both the wire-derived path list and its lazily prepared lowercase
 * arrays. The multiplier covers UTF-16 storage, array slots, and short
 * per-code-point strings without depending on an engine's object layout. */
function retainedIndexBytes(index: FsFileIndex): number {
  if (index.paths.length > FILE_INDEX_MAX_PATHS) return Infinity;
  let bytes = 128;
  for (const path of index.paths) {
    if (typeof path !== "string") return Infinity;
    bytes += 96 + path.length * 16;
    if (bytes > FILE_INDEX_CACHE_MAX_BYTES) return Infinity;
  }
  return bytes;
}

/** Drop indexes for a route that left the workspace. */
export function dropFileIndexes(connectionId: string): void {
  const prefix = `${connectionId}\0`;
  for (const [key, entry] of [...cache]) {
    if (key.startsWith(prefix)) removeEntry(key, entry);
  }
}

/** Test/diagnostic seam. */
export function fileIndexCacheStats(): {
  items: number;
  bytes: number;
  inflight: number;
} {
  return { items: cache.size, bytes: cacheBytes, inflight: activeFetches };
}

/** Test seam. In-flight replies remain detached and cannot repopulate it. */
export function resetFileIndexCache(): void {
  for (const [key, entry] of [...cache]) removeEntry(key, entry);
}

/** Bumped when any fetch lands, so effects reading `localFileIndex` re-run
 *  and pick up the fresh list. */
const [indexVersion, setIndexVersion] = createSignal(0);

/** The cached candidate list for (connection, root), kicking off a fetch in
 *  the background when missing or older than the TTL. Null until the first
 *  fetch lands. Reactive: reads a version signal, so a tracking scope
 *  re-runs when the fetch completes. */
export function localFileIndex(
  workspace: YasWorkspace,
  connectionId: string,
  root: string,
): FsFileIndex | null {
  void indexVersion();
  const key = `${connectionId}\0${root}`;
  if (key.length > FILE_INDEX_MAX_KEY_CHARS) return null;
  let entry = cache.get(key);
  if (!entry) {
    entry = { index: null, fetchedAt: 0, inflight: false, bytes: 0 };
    cache.set(key, entry);
    pruneCache();
  } else {
    touchEntry(key, entry);
  }
  if (
    cache.get(key) === entry &&
    Date.now() - entry.fetchedAt > TTL_MS &&
    !entry.inflight &&
    activeFetches < FILE_INDEX_MAX_INFLIGHT
  ) {
    entry.inflight = true;
    activeFetches++;
    workspace
      .indexFiles(connectionId, root)
      .then((index) => {
        activeFetches--;
        if (cache.get(key) !== entry) return;
        cacheBytes -= entry.bytes;
        const bytes = retainedIndexBytes(index);
        entry.index = Number.isFinite(bytes) ? index : null;
        entry.bytes = Number.isFinite(bytes) ? bytes : 0;
        cacheBytes += entry.bytes;
        entry.fetchedAt = Date.now();
        entry.inflight = false;
        touchEntry(key, entry);
        pruneCache();
        setIndexVersion((v) => v + 1);
      })
      .catch(() => {
        activeFetches--;
        if (cache.get(key) !== entry) return;
        // Refused (bad root, budget) — retry after the TTL, and leave any
        // previous list in place meanwhile.
        entry.fetchedAt = Date.now();
        entry.inflight = false;
        touchEntry(key, entry);
        setIndexVersion((v) => v + 1);
      });
  }
  return entry.index;
}

const utf8 = new TextEncoder();

/** The server folds only ASCII bytes; non-ASCII UTF-8 remains byte-exact. */
function asciiFold(bytes: Uint8Array): Uint8Array {
  const folded = new Uint8Array(bytes);
  for (let i = 0; i < folded.length; i++) {
    if (folded[i] >= 0x41 && folded[i] <= 0x5a) folded[i] += 0x20;
  }
  return folded;
}

/** One candidate with everything the scorer needs precomputed, so a
 *  keystroke's rescore allocates nothing per candidate. */
type PreparedPath = {
  path: string;
  /** ASCII-folded UTF-8 bytes, exactly matching native FS SEARCH. */
  bytes: Uint8Array;
  /** Original UTF-8 components for the server's final path tie-break. */
  components: Uint8Array[];
};

// The prepared form is derived once per index fetch (the list changes at
// most once per TTL) and keyed by the index object itself, so a fresh
// fetch naturally invalidates it.
const preparedCache = new WeakMap<FsFileIndex, PreparedPath[]>();

function prepare(index: FsFileIndex): PreparedPath[] {
  let prepared = preparedCache.get(index);
  if (!prepared) {
    prepared = index.paths.map((path) => ({
      path,
      bytes: asciiFold(utf8.encode(path)),
      components: path.split("/").map((component) => utf8.encode(component)),
    }));
    preparedCache.set(index, prepared);
  }
  return prepared;
}

/** Port of `fuzzy_score` in `crates/server/src/yas_fs.rs`. Smaller tuples are
 *  better: the span from first to last matched byte, then total path bytes. */
function fuzzyScore(
  hay: PreparedPath,
  needle: Uint8Array,
): readonly [span: number, byteLength: number] | null {
  let at = 0;
  let first: number | null = null;
  for (const wanted of needle) {
    while (at < hay.bytes.length && hay.bytes[at] !== wanted) at++;
    if (at === hay.bytes.length) return null;
    first ??= at;
    at++;
  }
  return [at - (first ?? 0), hay.bytes.length];
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const shared = Math.min(left.length, right.length);
  for (let i = 0; i < shared; i++) {
    if (left[i] !== right[i]) return left[i] - right[i];
  }
  return left.length - right.length;
}

function comparePath(left: PreparedPath, right: PreparedPath): number {
  const shared = Math.min(left.components.length, right.components.length);
  for (let i = 0; i < shared; i++) {
    const order = compareBytes(left.components[i], right.components[i]);
    if (order !== 0) return order;
  }
  return left.components.length - right.components.length;
}

/** Score the index against `query`, best first. `recencyRank` (0 = most
 *  recently touched, null = never) lifts files the user already opened
 *  above cold matches, so an empty "@" reads as a most-recent-files list. */
export function searchFileIndex(
  index: FsFileIndex,
  query: string,
  limit: number,
  recencyRank?: (relPath: string) => number | null,
): string[] {
  const needle = asciiFold(utf8.encode(query));
  const scored: Array<{
    score: readonly [number, number];
    path: PreparedPath;
    recency: number | null;
  }> = [];
  for (const p of prepare(index)) {
    const score = fuzzyScore(p, needle);
    if (score === null) continue;
    scored.push({ score, path: p, recency: recencyRank?.(p.path) ?? null });
  }
  scored.sort((a, b) => {
    const span = a.score[0] - b.score[0];
    if (span !== 0) return span;
    // Recency is an intentional UI-only tie-break. It makes an empty query a
    // recent-files list without changing native fuzzy-match quality.
    if (a.recency !== b.recency) {
      if (a.recency === null) return 1;
      if (b.recency === null) return -1;
      return a.recency - b.recency;
    }
    return a.score[1] - b.score[1] || comparePath(a.path, b.path);
  });
  if (scored.length > limit) scored.length = limit;
  return scored.map(({ path }) => path.path);
}
