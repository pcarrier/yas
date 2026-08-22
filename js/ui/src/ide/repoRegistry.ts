/**
 * Shared, ref-counted git-repo registry.
 *
 * Every diff/commit tile used to call `openRepo` independently, so opening
 * several tiles over the same working tree spun up several server-side repos.
 * This registry coalesces them: acquires are keyed by the *resolved* workdir,
 * so all tiles under one repo share a single `GitRepoHandle`. The handle is
 * closed on the server only once the last consumer releases it.
 *
 * Consumers get a thin proxy whose `close()` drops their reference; every other
 * method/getter (patch, log, watchLog, state, subscribe, revision, workdir…)
 * delegates to the shared handle, so notifications fan out to all consumers via
 * the handle's own notifier.
 *
 * Scope: tiles only. The IdeSession keeps its own handle — it opens with
 * session-specific options and has its own reconnect lifecycle. Tiles, by
 * contrast, release on disconnect (their `open` factory returns null when the
 * connection drops), so a dead entry is always evicted before a reconnect
 * re-acquires.
 */

import type {
  YasWorkspace,
  YasNativeGitRepoHandle,
  YasNativeGitOpenOptions,
} from "@yas-run/core";

interface Entry {
  key: string;
  connectionId: string;
  workdir: string;
  handle: YasNativeGitRepoHandle;
  refs: number;
  /** Pending close from the linger below, cancelled by a re-acquire. */
  closing?: ReturnType<typeof setTimeout>;
}

/**
 * How long a repo with no consumers is kept before it is closed server-side.
 *
 * Moving a tile between a pane and the dock unmounts the old view and mounts a
 * new one, which drops the last reference and takes a fresh one a moment later.
 * Closing immediately made every such move a `GIT_OPEN` plus a fresh state
 * snapshot over the wire, for a repo that was about to be asked for again. A
 * few seconds of linger costs one idle server-side repo — they are per-workdir
 * and re-acquired instantly — and makes the move free.
 */
const LINGER_MS = 10_000;

const registries = new WeakMap<YasWorkspace, Map<string, Entry>>();

function mapFor(ws: YasWorkspace): Map<string, Entry> {
  let m = registries.get(ws);
  if (!m) {
    m = new Map();
    registries.set(ws, m);
  }
  return m;
}

function isUnder(path: string, dir: string): boolean {
  if (!dir) return false;
  const d = dir.endsWith("/") ? dir.slice(0, -1) : dir;
  return path === d || path.startsWith(`${d}/`);
}

// Options for the shared tile repo: watch (diffs re-run on worktree/ref changes
// via the notifier) + status (worktree-change notifications). NOT untracked —
// an untracked *diff* passes GIT_DIFF_UNTRACKED on the patch call itself, so the
// repo needn't maintain an untracked status walk (expensive on large repos).
// Commits need none of this; the flags are harmless for them.
const SHARED_OPTIONS: YasNativeGitOpenOptions = {
  watch: true,
  status: true,
};

function makeProxy(ws: YasWorkspace, entry: Entry): YasNativeGitRepoHandle {
  let released = false;
  return new Proxy(entry.handle, {
    get(target, prop) {
      if (prop === "close") {
        return () => {
          if (released) return;
          released = true;
          if (--entry.refs > 0) return;
          entry.closing = setTimeout(() => {
            // Still unreferenced, and still the registry's entry for this
            // key: an acquire in the meantime cleared `closing`.
            if (entry.refs > 0) return;
            if (mapFor(ws).get(entry.key) === entry)
              mapFor(ws).delete(entry.key);
            entry.handle.close();
          }, LINGER_MS);
        };
      }
      // Handle methods are closures (no `this`), so an unbound reference is
      // safe; getters (revision/state) read captured state, not `this`.
      return Reflect.get(target, prop);
    },
  });
}

/**
 * Acquire a shared repo containing `path`. Returns a proxy handle; call
 * `close()` on it to release. When the last holder releases, the underlying
 * server repo is closed.
 */
export async function acquireRepo(
  ws: YasWorkspace,
  connectionId: string,
  path: string,
): Promise<YasNativeGitRepoHandle> {
  const m = mapFor(ws);
  // Reuse a cached repo whose workdir contains `path` — including one that is
  // lingering with no consumers, which is the whole point of the linger.
  for (const entry of m.values()) {
    if (entry.connectionId === connectionId && isUnder(path, entry.workdir)) {
      entry.refs++;
      if (entry.closing !== undefined) {
        clearTimeout(entry.closing);
        entry.closing = undefined;
      }
      return makeProxy(ws, entry);
    }
  }
  // Open a fresh repo and key it by the resolved workdir. onClosed evicts the
  // entry: a server reset/reconnect closes the handle server-side, so it must
  // never be handed to a later acquire — that acquire opens a fresh one.
  let entryRef: Entry | null = null;
  const handle = await ws.openRepo(connectionId, path, {
    ...SHARED_OPTIONS,
    onClosed: () => {
      if (entryRef && mapFor(ws).get(entryRef.key) === entryRef) {
        mapFor(ws).delete(entryRef.key);
      }
    },
  });
  const workdir = handle.workdir;
  const key = `${connectionId}\0${workdir}`;
  const existing = m.get(key);
  if (existing) {
    // Race: a concurrent acquire opened the same workdir first — dedup.
    handle.close();
    existing.refs++;
    if (existing.closing !== undefined) {
      clearTimeout(existing.closing);
      existing.closing = undefined;
    }
    return makeProxy(ws, existing);
  }
  const entry: Entry = { key, connectionId, workdir, handle, refs: 1 };
  entryRef = entry;
  m.set(key, entry);
  return makeProxy(ws, entry);
}
