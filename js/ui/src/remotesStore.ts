/**
 * The Relay catalogue in the server's KV store: one CAS'd `remotes` key per
 * server holding the ordered `name = uri` list (`#`-prefixed = disabled).
 *
 * Shaped exactly like {@link ./ide/rootsStore.ts}, because it is the same
 * problem: a per-server document that several clients edit. The server reads
 * this key to build its route catalogue (`crates/server/src/relay.rs`), so an
 * edit here is an edit to what that server will dial.
 *
 * The stored URIs carry credentials — a `share:` passphrase, an `ssh:`
 * identity — and every client of this server can read them. That is the
 * deliberate trade for letting clients administer remotes at all: a client
 * that can reach this store already holds full authority over the server.
 * What is *published* through the Relay family is still credential-free.
 *
 * Every mutation is read-modify-write CAS'd on the current value hash and
 * retried once on conflict, so two clients editing at once converge instead of
 * one silently losing.
 */

import { createSignal } from "solid-js";
import type {
  YasWorkspace,
  ConnectionId,
  WorkspaceSessionKvWatch,
} from "@yas-run/core";
import { WorkspaceSessionKvConflictError } from "@yas-run/core";

export interface StoredRemote {
  name: string;
  /** Connection whose catalogue holds this entry. */
  connectionId: ConnectionId;
  /** The connector URI, credentials and all. */
  uri: string;
  /** Disabled entries stay stored but are excluded from route resolution. */
  disabled: boolean;
}

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

/** Must match `yas_server::relay::REMOTES_KEY`. */
const REMOTES_KEY = "remotes";
export const REMOTES_MAX_DOCUMENT_BYTES = 1024 * 1024;
export const REMOTES_MAX_ITEMS = 1_024;
export const REMOTES_MAX_NAME_CHARS = 255;
export const REMOTES_MAX_URI_CHARS = 16 * 1024;
const WATCH_MAX_CONNECTIONS = 64;

function serialize(remotes: readonly StoredRemote[]): string {
  return remotes
    .map(
      (remote) =>
        `${remote.disabled ? "# " : ""}${remote.name} = ${remote.uri}`,
    )
    .join("\n");
}

/**
 * Parse a stored catalogue.
 *
 * Deliberately lenient in the same way the Rust parser is: a line it cannot
 * read is skipped, not fatal. A document one client cannot parse must not stop
 * that client showing the rest of the catalogue.
 */
export function parseRemotesDocument(
  text: string,
  connectionId: ConnectionId,
): StoredRemote[] {
  if (text.length > REMOTES_MAX_DOCUMENT_BYTES) return [];
  const remotes: StoredRemote[] = [];
  let lines = 0;
  for (const line of text.split("\n")) {
    if (++lines > REMOTES_MAX_ITEMS) break;
    const trimmed = line.trim();
    if (!trimmed) continue;
    const disabled = trimmed.startsWith("#");
    const body = disabled ? trimmed.slice(1).trimStart() : trimmed;
    const equals = body.indexOf("=");
    if (equals <= 0) continue;
    const name = body.slice(0, equals).trim();
    const uri = body.slice(equals + 1).trim();
    if (
      !name ||
      name.length > REMOTES_MAX_NAME_CHARS ||
      !uri ||
      uri.length > REMOTES_MAX_URI_CHARS
    ) {
      continue;
    }
    remotes.push({ name, connectionId, uri, disabled });
  }
  return remotes;
}

/**
 * Whether `name` survives a round trip through the document.
 *
 * The same rule the Rust side enforces: the format is `name = uri`, so an `=`
 * reparses as the start of the URI and a leading `#` reparses as the disabled
 * marker — an entry added as enabled would come back disabled.
 */
export function validRemoteName(name: string): boolean {
  return (
    name.length > 0 &&
    name.length <= REMOTES_MAX_NAME_CHARS &&
    !name.startsWith("#") &&
    !/[\s=]/.test(name)
  );
}

type WatchState = {
  handle: WorkspaceSessionKvWatch | null;
  generation: number;
};

const watches = new Map<ConnectionId, WatchState>();
const [storedRemotes, setStoredRemotes] = createSignal<
  Map<ConnectionId, StoredRemote[]>
>(new Map(), { equals: false });

/** Every connected server's catalogue, keyed by connection. */
export function remotesByConnection(): Map<ConnectionId, StoredRemote[]> {
  return storedRemotes();
}

/** One server's catalogue, or an empty list while it is unknown. */
export function remotesFor(connectionId: ConnectionId): StoredRemote[] {
  return storedRemotes().get(connectionId) ?? [];
}

function cache(connectionId: ConnectionId, remotes: StoredRemote[]): void {
  setStoredRemotes((byConnection) => {
    byConnection.set(connectionId, remotes);
    return byConnection;
  });
}

/** Follow one server's catalogue for as long as the connection lives. */
export function ensureStoredRemotes(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  generation: number,
): void {
  const existing = watches.get(connectionId);
  if (existing && existing.generation === generation) return;
  if (!existing && watches.size >= WATCH_MAX_CONNECTIONS) return;
  existing?.handle?.close();
  const state: WatchState = { handle: null, generation };
  watches.set(connectionId, state);
  workspace
    .watchKv(connectionId, REMOTES_KEY, {
      onUpdate: (mirror) => {
        if (watches.get(connectionId) !== state) return;
        const entry = mirror.live.get(REMOTES_KEY);
        cache(
          connectionId,
          entry?.value
            ? parseRemotesDocument(
                textDecoder.decode(entry.value),
                connectionId,
              )
            : [],
        );
      },
      onClosed: () => {
        // Connection lost: drop so the arming effect re-runs, and stop
        // showing a catalogue nobody is confirming.
        if (watches.get(connectionId) === state) watches.delete(connectionId);
        setStoredRemotes((byConnection) => {
          byConnection.delete(connectionId);
          return byConnection;
        });
      },
    })
    .then((handle) => {
      if (watches.get(connectionId) !== state) {
        handle.close(); // superseded while opening
        return;
      }
      state.handle = handle;
    })
    .catch(() => {
      if (watches.get(connectionId) === state) watches.delete(connectionId);
    });
}

/** Read-modify-write one server's catalogue under CAS; one conflict retry. */
async function mutate(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  transform: (remotes: StoredRemote[]) => StoredRemote[],
): Promise<void> {
  const attempt = async (): Promise<void> => {
    const current = await workspace.kvFetch(connectionId, REMOTES_KEY);
    const remotes = current
      ? parseRemotesDocument(textDecoder.decode(current.value), connectionId)
      : [];
    await workspace.kvPut(
      connectionId,
      REMOTES_KEY,
      textEncoder.encode(serialize(transform(remotes))),
      current ? { ifHash: current.hash } : { create: true },
    );
  };
  try {
    await attempt();
  } catch (error) {
    if (error instanceof WorkspaceSessionKvConflictError) await attempt();
    else throw error;
  }
}

/** Add a remote, or replace the URI of one that already has that name. */
export function addStoredRemote(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  name: string,
  uri: string,
): Promise<void> {
  return mutate(workspace, connectionId, (remotes) => {
    const existing = remotes.find((remote) => remote.name === name);
    if (existing) {
      return remotes.map((remote) =>
        remote.name === name ? { ...remote, uri, disabled: false } : remote,
      );
    }
    return [...remotes, { name, connectionId, uri, disabled: false }];
  });
}

export function removeStoredRemote(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  name: string,
): Promise<void> {
  return mutate(workspace, connectionId, (remotes) =>
    remotes.filter((remote) => remote.name !== name),
  );
}

export function toggleStoredRemote(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  name: string,
): Promise<void> {
  return mutate(workspace, connectionId, (remotes) =>
    remotes.map((remote) =>
      remote.name === name ? { ...remote, disabled: !remote.disabled } : remote,
    ),
  );
}
