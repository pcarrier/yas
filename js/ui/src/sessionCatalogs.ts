/**
 * Every connected server's application catalog, held open for the page.
 *
 * The Manage panel opens a `yas.session.v1` channel when a viewer expands a
 * remote and closes it when they leave, which is right for a panel: it costs
 * nothing while nobody is looking. The switcher cannot work that way. It has to
 * filter a thousand applications from the first keystroke instead of fetching
 * one when it opens.
 *
 * So this holds one channel per connected server for the life of the page. The
 * standing cost is small and one-sided: the catalog rides the greeting once,
 * and after that the supervisor only speaks when an application's state
 * changes. Icons are still asked for a screenful at a time, by whoever is
 * drawing them.
 *
 * Shaped like {@link ./ide/rootsStore.ts}: a module-scope map plus a version
 * signal. It is armed from an effect over the connection snapshots, but it
 * also follows `CHANNEL_WATCH` for `yas.session.v1` so that installing the
 * session extension after connect (or uninstalling and reinstalling it) opens
 * or closes the catalog without requiring a reconnect.
 */

import { createSignal } from "solid-js";
import type { YasWorkspace, ConnectionId } from "@yas-run/core";
import { followChannelNames } from "./channelPresence";
import {
  openSession,
  SESSION_CHANNEL,
  type SessionApp,
  type SessionCatalogEntry,
  type SessionHandle,
} from "./session";

/** One server's applications: what it manages, and what it could run. */
export interface RemoteApplications {
  readonly connectionId: ConnectionId;
  /** Applications the supervisor is managing, running or not. */
  readonly apps: readonly SessionApp[];
  /** Everything installed there, sorted by display name. */
  readonly catalog: readonly SessionCatalogEntry[];
}

type OpenState = {
  handle: SessionHandle | null;
  stopHandleSubscription: (() => void) | null;
  generation: number;
  present: boolean;
  nextAttempt: number;
  opening: { id: number; cancelled: boolean; closed: boolean } | null;
  /** Stops the `CHANNEL_WATCH` follow for this connection. */
  stopChannelWatch: (() => void) | null;
};

const opens = new Map<ConnectionId, OpenState>();
/** Catalogs are proactive (opened before the switcher is shown), so cap the
 * standing channels independently of the route protocol's much larger hard
 * maximum. Route order is stable and the home/primary servers come first. */
export const SESSION_CATALOG_MAX_CONNECTIONS = 32;

/** Bumped on every message from any supervisor. Readers touch it to become
 *  reactive, exactly as the file index's consumers touch its version. */
const [version, setVersion] = createSignal(0);
const bump = () => setVersion((n) => n + 1);

/** Detach before closing: a synchronous onClosed callback must never mistake
 * this deliberately closed handle for the current one. */
const closeInstalled = (state: OpenState): boolean => {
  const handle = state.handle;
  if (!handle) return false;
  state.handle = null;
  state.stopHandleSubscription?.();
  state.stopHandleSubscription = null;
  handle.close();
  return true;
};

/**
 * Idempotently hold one server's supervisor channel open.
 *
 * Re-call freely — from an effect over the connection snapshots, passing the
 * connection generation, which re-arms after a re-establish. The channel itself
 * is opened only while the server's registry reports `yas.session.v1` as
 * present, so uninstalling the session extension closes the catalog and
 * reinstalling it reopens it without requiring a reconnect.
 */
export function ensureSessionCatalog(
  workspace: YasWorkspace,
  connectionId: ConnectionId,
  generation: number,
): void {
  const existing = opens.get(connectionId);
  if (existing && existing.generation === generation) return;
  if (!existing && opens.size >= SESSION_CATALOG_MAX_CONNECTIONS) return;
  // Superseded by a new generation, or the first call for this connection.
  if (existing) {
    existing.stopChannelWatch?.();
    bump();
  }

  const connection = workspace.getConnection(connectionId);
  if (!connection) return;
  const state: OpenState = {
    handle: null,
    stopHandleSubscription: null,
    generation,
    present: false,
    nextAttempt: 0,
    opening: null,
    stopChannelWatch: null,
  };
  opens.set(connectionId, state);

  let live = true;
  let stopWatch: (() => void) | null = null;
  state.stopChannelWatch = () => {
    live = false;
    state.present = false;
    if (state.opening) state.opening.cancelled = true;
    state.opening = null;
    stopWatch?.();
    closeInstalled(state);
    if (opens.get(connectionId) === state) opens.delete(connectionId);
  };

  const beginOpen = (): void => {
    if (
      !live ||
      !state.present ||
      state.handle !== null ||
      state.opening !== null ||
      opens.get(connectionId) !== state
    ) {
      return;
    }
    const attempt = {
      id: state.nextAttempt++,
      cancelled: false,
      closed: false,
    };
    state.opening = attempt;
    let opened: SessionHandle | null = null;
    void openSession(connection, {
      onClosed: () => {
        attempt.closed = true;
        if (state.opening === attempt) state.opening = null;
        // Compare the exact handle. A delayed close from an earlier open must
        // not clear the replacement installed after a channel flap.
        if (opened !== null && state.handle === opened) {
          state.handle = null;
          state.stopHandleSubscription?.();
          state.stopHandleSubscription = null;
          opened.close();
          bump();
        }
      },
    })
      .then((handle) => {
        opened = handle;
        if (
          !live ||
          !state.present ||
          attempt.cancelled ||
          attempt.closed ||
          state.opening !== attempt ||
          opens.get(connectionId) !== state
        ) {
          if (state.opening === attempt) state.opening = null;
          handle.close();
          return;
        }
        state.opening = null;
        state.handle = handle;
        state.stopHandleSubscription = handle.subscribe(bump);
        bump();
      })
      .catch(() => {
        if (state.opening === attempt) state.opening = null;
        // A refused open is transient while the channel is flapping; the next
        // presence update will retry.
      });
  };

  void followChannelNames(connection, [SESSION_CHANNEL], (present) => {
    if (!live) return;
    if (present.has(SESSION_CHANNEL)) {
      state.present = true;
      beginOpen();
    } else {
      state.present = false;
      if (state.opening) state.opening.cancelled = true;
      state.opening = null;
      if (closeInstalled(state)) bump();
    }
  }).then((release) => {
    if (live) stopWatch = release;
    else release();
  });
}

/** Close a server's channel and stop watching its presence. */
export function dropSessionCatalog(connectionId: ConnectionId): void {
  const state = opens.get(connectionId);
  if (!state) return;
  state.stopChannelWatch?.();
  // stopChannelWatch deletes the state and closes the handle.
  bump();
}

const ready = (connectionId: ConnectionId): SessionHandle | null =>
  opens.get(connectionId)?.handle ?? null;

/**
 * The live supervisor channel for one server, for a caller that needs the
 * verbs and not just the lists.
 *
 * The Manage panel used to open a channel of its own. Sharing this one is not
 * only fewer channels: each mirror carries its own icon cache, and two of them
 * for the same server means the same artwork fetched and held twice. Callers
 * must not close it — it belongs to the store, and outlives any one panel.
 *
 * Reactive: null until the greeting has been asked for, and again if the
 * connection drops or the channel is not currently served.
 */
export function sessionHandle(
  connectionId: ConnectionId,
): SessionHandle | null {
  version();
  return ready(connectionId);
}

/** One server's applications, or null while it has no supervisor attached. */
export function sessionCatalog(
  connectionId: ConnectionId,
): RemoteApplications | null {
  version();
  const handle = ready(connectionId);
  if (!handle) return null;
  return { connectionId, apps: handle.apps, catalog: handle.catalog };
}

/** Every attached server's applications, in the order asked for. */
export function sessionCatalogs(
  connectionIds: readonly ConnectionId[],
): RemoteApplications[] {
  version();
  const out: RemoteApplications[] = [];
  for (const connectionId of connectionIds) {
    const found = sessionCatalog(connectionId);
    if (found) out.push(found);
  }
  return out;
}

/** Artwork for one application: an object URL, `null` for none, `undefined`
 *  while nobody has asked. Reactive — it lands long after the row is drawn. */
export function applicationIcon(
  connectionId: ConnectionId,
  id: string,
): string | null | undefined {
  version();
  return ready(connectionId)?.icon(id);
}

/** Ask one server for artwork; ids already known or in flight are dropped. */
export function requestApplicationIcons(
  connectionId: ConnectionId,
  ids: readonly string[],
): void {
  ready(connectionId)?.requestIcons(ids);
}

/**
 * Run one application now, without adopting it.
 *
 * `start`, not `enable`: launching something from the switcher is trying it,
 * not choosing it for every session from here on. It appears in the Manage
 * panel as a running row that is not enabled, which is where it can be kept or
 * discarded.
 */
export function startApplication(
  connectionId: ConnectionId,
  id: string,
): boolean {
  const handle = ready(connectionId);
  if (!handle) return false;
  handle.start(id);
  return true;
}
