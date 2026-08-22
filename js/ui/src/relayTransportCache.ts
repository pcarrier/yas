import type {
  ConnectionStatus,
  YasRelayRoute,
  YasWorkspaceConnection,
} from "@yas-run/core";

export const UI_RELAY_MAX_ROUTES = 128;
export const UI_RELAY_MAX_NAME_CHARS = 255;
export const UI_RELAY_MAX_LABEL_CHARS = 512;
export const UI_RELAY_MAX_DESCRIPTION_CHARS = 4_096;

/** Bound the presentation/transport catalogue independently of the wire hard
 * maximum. A peer may advertise 65k routes with 64k names; constructing a
 * transport and reactive UI entry for every record is avoidable amplification.
 */
export function boundedRelayRoutes(
  routes: readonly YasRelayRoute[],
): YasRelayRoute[] {
  const out: YasRelayRoute[] = [];
  const names = new Set<string>();
  for (const route of routes) {
    const name = route.name.trim();
    if (
      name.length === 0 ||
      name.length > UI_RELAY_MAX_NAME_CHARS ||
      name.includes("\0") ||
      names.has(name)
    ) {
      continue;
    }
    names.add(name);
    out.push({
      ...route,
      name,
      label: route.label.slice(0, UI_RELAY_MAX_LABEL_CHARS),
      description: route.description.slice(0, UI_RELAY_MAX_DESCRIPTION_CHARS),
    });
    if (out.length >= UI_RELAY_MAX_ROUTES) break;
  }
  return out;
}

export interface RelayConnectionCacheEntry {
  routeKey: string;
  connection: YasWorkspaceConnection;
  removeStatusListener: () => void;
  retryTimer: ReturnType<typeof setTimeout> | null;
}

const DEFAULT_RECONNECT_MIN_MS = 500;
const DEFAULT_RECONNECT_MAX_MS = 10_000;

/**
 * Own typed relayed product connections by stable route name. A nested Relay
 * stream and its YAS session share one lifetime; retry replaces both instead
 * of replaying HELLO/catalogue state into a second protocol consumer.
 */
export class RelayConnectionCache {
  private readonly entriesByName = new Map<string, RelayConnectionCacheEntry>();
  private readonly retryDelays = new Map<string, number>();

  constructor(
    private readonly onRetry: () => void,
    private readonly reconnectMinMs = DEFAULT_RECONNECT_MIN_MS,
    private readonly reconnectMaxMs = DEFAULT_RECONNECT_MAX_MS,
  ) {}

  get(name: string): RelayConnectionCacheEntry | undefined {
    return this.entriesByName.get(name);
  }

  entries(): IterableIterator<[string, RelayConnectionCacheEntry]> {
    return this.entriesByName.entries();
  }

  /** Reconcile both live transports and backoff-only tombstones with the
   * current route catalogue. A failed entry removes its transport while
   * retaining retry delay, so rotating route names would otherwise grow the
   * delay map forever even though `entries()` looked empty. */
  retain(names: ReadonlySet<string>): void {
    for (const name of [...this.entriesByName.keys()]) {
      if (!names.has(name)) this.delete(name);
    }
    for (const name of [...this.retryDelays.keys()]) {
      if (!names.has(name)) this.retryDelays.delete(name);
    }
  }

  /** Test/diagnostic seam. */
  stats(): { entries: number; retryDelays: number } {
    return {
      entries: this.entriesByName.size,
      retryDelays: this.retryDelays.size,
    };
  }

  set(
    name: string,
    routeKey: string,
    connection: YasWorkspaceConnection,
  ): RelayConnectionCacheEntry {
    if (this.entriesByName.has(name)) this.delete(name);
    const entry: RelayConnectionCacheEntry = {
      routeKey,
      connection,
      removeStatusListener: () => {},
      retryTimer: null,
    };
    const onStatus = (status: ConnectionStatus) => {
      if (this.entriesByName.get(name) !== entry) return;
      if (status === "connected") {
        this.retryDelays.delete(name);
        return;
      }
      if ((status !== "closed" && status !== "error") || entry.retryTimer) {
        return;
      }
      const delay = this.retryDelays.get(name) ?? this.reconnectMinMs;
      this.retryDelays.set(name, Math.min(delay * 2, this.reconnectMaxMs));
      entry.retryTimer = setTimeout(() => {
        entry.retryTimer = null;
        if (this.entriesByName.get(name) !== entry) return;
        this.delete(name, false);
        this.onRetry();
      }, delay);
    };
    connection.transport.addEventListener("statuschange", onStatus);
    entry.removeStatusListener = () =>
      connection.transport.removeEventListener("statuschange", onStatus);
    this.entriesByName.set(name, entry);
    return entry;
  }

  delete(name: string, forgetRetry = true): void {
    const entry = this.entriesByName.get(name);
    if (!entry) {
      if (forgetRetry) this.retryDelays.delete(name);
      return;
    }
    if (entry.retryTimer !== null) clearTimeout(entry.retryTimer);
    entry.retryTimer = null;
    entry.removeStatusListener();
    this.entriesByName.delete(name);
    if (forgetRetry) this.retryDelays.delete(name);
    entry.connection.close();
    entry.connection.dispose();
  }

  clear(): void {
    for (const name of [...this.entriesByName.keys()]) this.delete(name);
    this.retryDelays.clear();
  }
}
