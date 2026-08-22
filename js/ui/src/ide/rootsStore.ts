/**
 * Workspace roots in the server KV store (docs/design/kv.md § Second
 * consumer): one CAS'd `roots` key per server holding the ordered
 * `name = /path` list (`#`-prefixed = disabled). The remote is implicit in
 * which server holds the key.
 *
 * Per-server scoping is the feature and the conceded cost in one: every
 * client of a host sees the same roots, and the picker lists the union over
 * connected servers.
 *
 * Every mutation is read-modify-write CAS'd on the current value hash and
 * retried once on conflict — at human edit rates the retry is invisible,
 * and two clients editing simultaneously converge instead of one side
 * silently losing (the retired HTTP store's last-writer-wins hazard).
 */

import { createSignal } from "solid-js";
import type {
  YasWorkspace,
  ConnectionId,
  WorkspaceSessionKvWatch,
} from "@yas-run/core";
import { WorkspaceSessionKvConflictError } from "@yas-run/core";

export interface Root {
  name: string;
  /** Connection that owns this root. */
  remote: string;
  /** Absolute path on that connection. */
  path: string;
  /** Disabled roots remain stored but are hidden from the picker. */
  disabled: boolean;
}

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

const ROOTS_KEY = "roots";
export const ROOT_CACHE_MAX_DOCUMENT_BYTES = 1024 * 1024;
export const ROOT_CACHE_MAX_ITEMS = 4_096;
export const ROOT_CACHE_MAX_BYTES = 16 * 1024 * 1024;
export const ROOT_WATCH_MAX_CONNECTIONS = 64;
export const ROOT_DOCUMENT_MAX_NAME_CHARS = 255;
export const ROOT_DOCUMENT_MAX_PATH_CHARS = 8 * 1024;

function serialize(roots: readonly Root[]): string {
  return roots
    .map((r) => `${r.disabled ? "# " : ""}${r.name} = ${r.path}`)
    .join("\n");
}

/** Parse a bounded server `roots` document and stamp its owning connection. */
export function parseRootDocument(
  text: string,
  connectionId: ConnectionId,
): Root[] {
  if (text.length > ROOT_CACHE_MAX_DOCUMENT_BYTES) return [];
  const roots: Root[] = [];
  let lines = 0;
  for (const line of text.split("\n")) {
    if (++lines > ROOT_CACHE_MAX_ITEMS) break;
    const trimmed = line.trim();
    if (!trimmed) continue;
    const disabled = trimmed.startsWith("#");
    const body = disabled ? trimmed.slice(1).trimStart() : trimmed;
    const eq = body.indexOf("=");
    if (eq <= 0) continue;
    const name = body.slice(0, eq).trim();
    const path = body.slice(eq + 1).trim();
    if (
      !name ||
      name.length > ROOT_DOCUMENT_MAX_NAME_CHARS ||
      !path ||
      path.length > ROOT_DOCUMENT_MAX_PATH_CHARS
    ) {
      continue;
    }
    roots.push({ name, remote: connectionId, path, disabled });
  }
  return roots;
}

type WatchState = {
  handle: WorkspaceSessionKvWatch | null;
  generation: number;
  hash: Uint8Array | null;
};

const watches = new Map<ConnectionId, WatchState>();
const [serverRoots, setServerRoots] = createSignal<Map<ConnectionId, Root[]>>(
  new Map(),
  { equals: false },
);
const serverRootCosts = new Map<
  ConnectionId,
  { items: number; bytes: number }
>();
let retainedRootItems = 0;
let retainedRootBytes = 0;

function removeCachedRoots(
  rootsByConnection: Map<ConnectionId, Root[]>,
  connectionId: ConnectionId,
): void {
  rootsByConnection.delete(connectionId);
  const cost = serverRootCosts.get(connectionId);
  if (!cost) return;
  serverRootCosts.delete(connectionId);
  retainedRootItems -= cost.items;
  retainedRootBytes -= cost.bytes;
}

function cacheRoots(connectionId: ConnectionId, roots: Root[]): void {
  setServerRoots((rootsByConnection) => {
    removeCachedRoots(rootsByConnection, connectionId);
    const bounded: Root[] = [];
    let bytes = 64;
    for (const root of roots) {
      const nextBytes =
        128 + (root.name.length + root.remote.length + root.path.length) * 2;
      if (
        bounded.length >= ROOT_CACHE_MAX_ITEMS ||
        bytes + nextBytes > ROOT_CACHE_MAX_BYTES
      ) {
        break;
      }
      bounded.push(root);
      bytes += nextBytes;
    }
    const cost = { items: bounded.length, bytes };
    rootsByConnection.set(connectionId, bounded);
    serverRootCosts.set(connectionId, cost);
    retainedRootItems += cost.items;
    retainedRootBytes += cost.bytes;

    while (
      retainedRootItems > ROOT_CACHE_MAX_ITEMS ||
      retainedRootBytes > ROOT_CACHE_MAX_BYTES
    ) {
      const oldest = rootsByConnection.keys().next().value as
        | ConnectionId
        | undefined;
      if (oldest === undefined) break;
      removeCachedRoots(rootsByConnection, oldest);
    }
    return rootsByConnection;
  });
}

/** Roots stored on connected servers, in per-server document order. */
export function allServerRoots(): Root[] {
  const out: Root[] = [];
  for (const roots of serverRoots().values()) out.push(...roots);
  return out;
}

/** True while `connectionId`'s roots come from its server (a live watch). */
export function hasServerRoots(connectionId: ConnectionId): boolean {
  return watches.has(connectionId);
}

/** Release one removed connection's watch and retained root document. */
export function dropServerRoots(connectionId: ConnectionId): void {
  const state = watches.get(connectionId);
  if (state) {
    watches.delete(connectionId);
    state.handle?.close();
  }
  setServerRoots((rootsByConnection) => {
    removeCachedRoots(rootsByConnection, connectionId);
    return rootsByConnection;
  });
}

/**
 * Idempotently watch one server's `roots` key. Re-call freely — e.g. from an
 * effect over connection snapshots with the connection generation, which
 * re-arms the watch after a re-establish (subscriptions don't survive one).
 */
export function ensureServerRoots(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  generation: number,
): void {
  const existing = watches.get(connectionId);
  if (existing && existing.generation === generation) return;
  if (!existing && watches.size >= ROOT_WATCH_MAX_CONNECTIONS) return;
  existing?.handle?.close();
  const state: WatchState = { handle: null, generation, hash: null };
  watches.set(connectionId, state);
  workspace
    .watchKv(connectionId, ROOTS_KEY, {
      onUpdate: (mirror) => {
        if (watches.get(connectionId) !== state) return;
        const entry = mirror.live.get(ROOTS_KEY);
        state.hash = entry?.hash ?? null;
        if (entry?.value) {
          const roots =
            entry.value.byteLength <= ROOT_CACHE_MAX_DOCUMENT_BYTES
              ? parseRootDocument(textDecoder.decode(entry.value), connectionId)
              : [];
          cacheRoots(connectionId, roots);
        } else {
          cacheRoots(connectionId, []);
        }
      },
      onClosed: () => {
        // Connection lost/re-established: drop so the ensure-effect re-arms,
        // and stop advertising this server's roots meanwhile.
        if (watches.get(connectionId) === state) watches.delete(connectionId);
        setServerRoots((rootsByConnection) => {
          removeCachedRoots(rootsByConnection, connectionId);
          return rootsByConnection;
        });
      },
    })
    .then((handle) => {
      const cur = watches.get(connectionId);
      if (cur !== state) {
        handle.close(); // superseded while opening
        return;
      }
      state.handle = handle;
    })
    .catch(() => {
      // Transient open failure: drop so the ensure-effect retries.
      if (watches.get(connectionId) === state) watches.delete(connectionId);
    });
}

/** Read-modify-write one server's roots under CAS; one conflict retry. */
async function mutate(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  transform: (roots: Root[]) => Root[],
): Promise<void> {
  const attempt = async (): Promise<void> => {
    const cur = await workspace.kvFetch(connectionId, ROOTS_KEY);
    const roots = cur
      ? parseRootDocument(textDecoder.decode(cur.value), connectionId)
      : [];
    const next = serialize(transform(roots));
    await workspace.kvPut(
      connectionId,
      ROOTS_KEY,
      textEncoder.encode(next),
      cur ? { ifHash: cur.hash } : { create: true },
    );
  };
  try {
    await attempt();
  } catch (e) {
    if (e instanceof WorkspaceSessionKvConflictError) await attempt();
    // Anything else: best-effort, the watch keeps the UI truthful.
  }
}

export function addServerRoot(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  name: string,
  path: string,
): void {
  void mutate(workspace, connectionId, (roots) => [
    ...roots.filter((r) => r.name !== name),
    { name, remote: connectionId, path, disabled: false },
  ]);
}

export function removeServerRoot(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  name: string,
): void {
  void mutate(workspace, connectionId, (roots) =>
    roots.filter((r) => r.name !== name),
  );
}

export function toggleServerRoot(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  name: string,
): void {
  void mutate(workspace, connectionId, (roots) =>
    roots.map((r) => (r.name === name ? { ...r, disabled: !r.disabled } : r)),
  );
}

/** Reorder this server's roots to match `names` (unknown names keep their
 *  relative order at the end — a concurrent add survives a reorder). */
export function reorderServerRoots(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  names: readonly string[],
): void {
  void mutate(workspace, connectionId, (roots) => {
    const byName = new Map(roots.map((r) => [r.name, r]));
    const ordered: Root[] = [];
    for (const name of names) {
      const r = byName.get(name);
      if (r) {
        ordered.push(r);
        byName.delete(name);
      }
    }
    return [...ordered, ...byName.values()];
  });
}
