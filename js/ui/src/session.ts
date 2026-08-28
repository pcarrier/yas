/**
 * Client mirror for the `yas.session.v1` channel served by the session
 * supervisor extension (`extensions/session`).
 *
 * Outbound is JSON, one object per message; inbound is a bare text line
 * (`enable <id>`), because a Wasm guest has no JSON parser and the vocabulary
 * is three verbs. The extension sends complete state rather than deltas: the
 * managed set is what an operator typed, so it is small, and a panel that can
 * only ever be correct beats one that avoids resending a few hundred bytes.
 *
 * Icons are the exception to "complete state": they are asked for, one batch of
 * ids at a time, and answered one message per id. Artwork is three orders of
 * magnitude larger than everything else here — a catalog of a thousand entries
 * is a few tens of kilobytes of names and tens of megabytes of icons — so the
 * panel asks only for the rows it is about to draw.
 *
 * This is an extension protocol, not a built-in YAS family, which is why it
 * lives in the app rather than in `@yas-run/core` — the same split
 * {@link ./systemd.ts} makes.
 */

import type { ReactiveStore } from "@yas-run/core";
import { Notifier, YAS_STATUS_RESOURCE_EXHAUSTED } from "@yas-run/core";

interface ChannelMessageHandle {
  send(payload: Uint8Array | string): boolean;
  close(status?: number): void;
}

interface ChannelOpenCallbacks {
  metadata?: Uint8Array;
  onData?(payload: Uint8Array): void;
  onCredit?(available: bigint): void;
  onClosed?(status: number, detail: string): void;
}

/** The part of a connection this mirror needs: one named channel, and the
 *  one-shot read that turns a resolved icon path into bytes. */
export interface ChannelOpener {
  connectChannel(
    name: string,
    options?: ChannelOpenCallbacks,
  ): Promise<ChannelMessageHandle>;
  readFiles?(
    groups: readonly (readonly string[])[],
    options?: { flags?: number; maxBytes?: number },
  ): Promise<{ status: number; path: string; content: Uint8Array }[]>;
}

export const SESSION_CHANNEL = "yas.session.v1";

/** What the supervisor is doing about one application. */
export type SessionPhase = "running" | "backoff" | "starting" | "stopped";

/** One application the session manages. */
export interface SessionApp {
  /** Desktop-entry id — the name `@session enable <id>` takes. */
  readonly id: string;
  readonly name: string;
  readonly enabled: boolean;
  readonly phase: SessionPhase;
  /** Consecutive failed starts; reset by a run that stays up. */
  readonly failures: number;
  /**
   * Windows counted from the identity the compositor stamped on the app's
   * Wayland socket — not from the app's self-asserted `app_id`, which is why
   * this number can be trusted.
   */
  readonly windows: number;
  readonly lastExit?: number;
  /** `WAYLAND_DISPLAY` basename of the running instance, when there is one. */
  readonly socket?: string;
}

/** One installed application that could be enabled. */
export interface SessionCatalogEntry {
  readonly id: string;
  readonly name: string;
}

/**
 * How many ids ride one request; the extension refuses more than this.
 *
 * Deliberately several screens' worth. A request costs one child process on the
 * far end whatever it asks for, so the batch size is what buys throughput while
 * a list is being scrolled — twelve rows per round trip cannot keep up with a
 * wheel, and forty-eight can.
 */
const ICON_BATCH = 48;

/**
 * How long an id is considered asked before it may be asked again.
 *
 * Not a retry timer so much as a leak stopper: an answer can be lost — the
 * supervisor bounds what it will queue for a panel that is not keeping up — and
 * without this the row that lost it keeps its placeholder for the life of the
 * channel, because the id is marked asked and nothing ever asks again.
 */
const ICON_RETRY_MS = 8_000;

/**
 * How long an icon request waits for company.
 *
 * A render can reveal rows through several synchronous refs. Deferring the
 * send to the next task gathers those into one native search batch without
 * adding a human-visible wait to the first screenful.
 */
const ICON_REQUEST_COALESCE_MS = 0;
// A failed native read remains unread and gets another chance on this tick.
const ICON_ARTWORK_RETRY_MS = 120;

/**
 * Largest icon file the panel will read.
 *
 * Generous where the extension could not be: nothing base64s this or squeezes it
 * through a JSON string any more, so the only cost of a big file is the transfer
 * — which is why Steam's habit of writing 600 KB artwork into every size bucket
 * is no longer a row with no icon.
 */
export const SESSION_MAX_ARTWORK_FILE_BYTES = 1024 * 1024;

/** Retained supervisor state. The catalog is normally around a thousand rows;
 * these ceilings leave ample headroom without letting one peer own the page. */
export const SESSION_MAX_APPS = 1024;
export const SESSION_MAX_CATALOG_ENTRIES = 4096;
export const SESSION_MAX_STATE_BYTES = 2 * 1024 * 1024;
export const SESSION_MAX_ID_CHARS = 255;
export const SESSION_MAX_NAME_CHARS = 1024;
export const SESSION_MAX_SOCKET_CHARS = 255;
export const SESSION_MAX_ICON_PATH_CHARS = 4096;

/** Keep the launcher's resolved shelf, bounded primarily by actual blob bytes. */
export const SESSION_ARTWORK_MAX_ENTRIES = SESSION_MAX_CATALOG_ENTRIES - 64;
export const SESSION_ARTWORK_MAX_BYTES = 32 * 1024 * 1024;
export const SESSION_ARTWORK_LOAD_MAX_ROUNDS = Math.ceil(
  SESSION_MAX_CATALOG_ENTRIES / ICON_BATCH,
);

/** Outstanding requests are peer-derived IDs and therefore bounded too. */
export const SESSION_ICON_REQUEST_MAX_ENTRIES = SESSION_MAX_CATALOG_ENTRIES;
export const SESSION_ICON_REQUEST_MAX_BYTES = 1024 * 1024;
export const SESSION_ICON_QUEUE_MAX_ENTRIES = SESSION_MAX_CATALOG_ENTRIES;
export const SESSION_ICON_QUEUE_MAX_BYTES = 1024 * 1024;
export const SESSION_COMMAND_QUEUE_MAX_ENTRIES = 64;
export const SESSION_COMMAND_QUEUE_MAX_BYTES = 64 * 1024;

export interface SessionOptions {
  onClosed?(reason: number, detail: string): void;
}

export interface SessionHandle extends ReactiveStore {
  /** Managed applications, sorted by display name. */
  readonly apps: readonly SessionApp[];
  /** Everything installed, sorted by display name. Empty until it arrives. */
  readonly catalog: readonly SessionCatalogEntry[];
  /** False until the first state message lands. */
  readonly ready: boolean;
  /**
   * Artwork for one application: an object URL, `null` for "there is none", and
   * `undefined` for "nobody has asked yet".
   *
   * The three-way answer is what lets a row show a placeholder without either
   * flickering through it on the way to an icon or re-asking forever for an
   * application that has none.
   */
  icon(id: string): string | null | undefined;
  /** Ask for the icons of these applications, skipping any already known or
   *  already in flight. Safe to call on every render. */
  requestIcons(ids: readonly string[]): void;
  /** Run it now, and on every session start. */
  enable(id: string): void;
  /** Stop it now, and on every session start. */
  disable(id: string): void;
  /** Run it now without changing what the next session start does. */
  start(id: string): void;
  /** Stop it now without changing what the next session start does. */
  stop(id: string): void;
  /** Stop it and drop it from the managed list entirely. */
  forget(id: string): void;
  /** Ask for fresh state and a fresh catalog. */
  resync(): void;
  close(): void;
}

function phaseOf(value: unknown): SessionPhase {
  return value === "running" ||
    value === "backoff" ||
    value === "starting" ||
    value === "stopped"
    ? value
    : "stopped";
}

const retainedStringBytes = (value: string): number => value.length * 2;
const keyedBytes = (value: string): number => 32 + retainedStringBytes(value);

function validSessionId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= SESSION_MAX_ID_CHARS &&
    !value.includes("\n") &&
    !value.includes("\r") &&
    !value.includes("\0")
  );
}

function boundedText(
  value: unknown,
  fallback: string,
  maxChars: number,
): string {
  return typeof value === "string" ? value.slice(0, maxChars) : fallback;
}

function appOf(value: unknown): SessionApp | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  if (!validSessionId(record.id)) return null;
  return {
    id: record.id,
    name: boundedText(record.name, record.id, SESSION_MAX_NAME_CHARS),
    enabled: record.enabled === true,
    phase: phaseOf(record.phase),
    failures: typeof record.failures === "number" ? record.failures : 0,
    windows: typeof record.windows === "number" ? record.windows : 0,
    lastExit: typeof record.lastExit === "number" ? record.lastExit : undefined,
    socket:
      typeof record.socket === "string"
        ? record.socket.slice(0, SESSION_MAX_SOCKET_CHARS)
        : undefined,
  };
}

function entryOf(value: unknown): SessionCatalogEntry | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  if (!validSessionId(record.id)) return null;
  return {
    id: record.id,
    name: boundedText(record.name, record.id, SESSION_MAX_NAME_CHARS),
  };
}

const appBytes = (app: SessionApp): number =>
  96 +
  retainedStringBytes(app.id) +
  retainedStringBytes(app.name) +
  (app.socket === undefined ? 0 : retainedStringBytes(app.socket));

const catalogEntryBytes = (entry: SessionCatalogEntry): number =>
  48 + retainedStringBytes(entry.id) + retainedStringBytes(entry.name);

function boundedRows<T>(
  values: readonly unknown[],
  parse: (value: unknown) => T | null,
  idOf: (value: T) => string,
  bytesOf: (value: T) => number,
  maxItems: number,
  maxBytes: number,
): { rows: T[]; bytes: number } {
  const rows: T[] = [];
  const seen = new Set<string>();
  let bytes = 0;
  for (const value of values) {
    if (rows.length >= maxItems) break;
    const row = parse(value);
    if (row === null) continue;
    const id = idOf(row);
    if (seen.has(id)) continue;
    const rowBytes = bytesOf(row);
    if (rowBytes > maxBytes - bytes) continue;
    seen.add(id);
    rows.push(row);
    bytes += rowBytes;
  }
  return { rows, bytes };
}

const byName = (
  left: { name: string; id: string },
  right: { name: string; id: string },
): number =>
  left.name.localeCompare(right.name) || left.id.localeCompare(right.id);

/**
 * The message reducer, with no transport attached.
 *
 * Separate from {@link openSession} so it can be driven from a recorded
 * transcript and tested without a connection.
 */
export class SessionMirror implements ReactiveStore {
  readonly #notifier = new Notifier();
  #apps: SessionApp[] = [];
  #appsBytes = 0;
  #catalog: SessionCatalogEntry[] = [];
  #catalogBytes = 0;
  #artwork = new Map<
    string,
    {
      path: string | null | undefined;
      icon: string | null | undefined;
      blobBytes: number;
    }
  >();
  #artworkBytes = 0;
  #hasAuthoritativeIds = false;
  #knownIds = new Set<string>();
  #ready = false;

  constructor(
    private readonly options: {
      onIconEvicted?(url: string): void;
      onIdsChanged?(ids: ReadonlySet<string>): void;
    } = {},
  ) {}

  get revision(): number {
    return this.#notifier.revision;
  }

  subscribe = (listener: () => void): (() => void) =>
    this.#notifier.subscribe(listener);

  get apps(): readonly SessionApp[] {
    return this.#apps;
  }

  get catalog(): readonly SessionCatalogEntry[] {
    return this.#catalog;
  }

  get ready(): boolean {
    return this.#ready;
  }

  /** An object URL, `null` once the answer "no artwork" has arrived, `undefined`
   *  while nobody has asked. */
  icon(id: string): string | null | undefined {
    const artwork = this.#touchArtwork(id);
    return artwork?.icon;
  }

  /** The path resolved for `id`, if artwork has been located and not yet read. */
  path(id: string): string | null | undefined {
    const artwork = this.#touchArtwork(id);
    return artwork?.path;
  }

  /** Ids with a path whose bytes nobody has read yet. */
  unread(): { id: string; path: string }[] {
    const out: { id: string; path: string }[] = [];
    for (const [id, artwork] of this.#artwork) {
      if (
        artwork.path !== undefined &&
        artwork.path !== null &&
        artwork.icon === undefined
      ) {
        out.push({ id, path: artwork.path });
      }
    }
    return out;
  }

  /** Record what a path turned out to hold. */
  setIcon(id: string, url: string | null, blobBytes = 0): boolean {
    if (
      !validSessionId(id) ||
      !this.acceptsId(id) ||
      !Number.isFinite(blobBytes) ||
      blobBytes < 0 ||
      blobBytes > SESSION_MAX_ARTWORK_FILE_BYTES
    ) {
      if (url !== null) this.options.onIconEvicted?.(url);
      return false;
    }
    const previous = this.#artwork.get(id);
    if (typeof previous?.icon === "string" && previous.icon !== url) {
      this.options.onIconEvicted?.(previous.icon);
    }
    const next = {
      path: previous?.path,
      icon: url,
      blobBytes: url === null ? 0 : Math.max(0, blobBytes),
    };
    this.#storeArtwork(id, next);
    this.#notifier.emit();
    return this.#artwork.has(id);
  }

  /** Whether a peer-provided/requested id belongs to the latest complete
   * state. Before the greeting, a bounded icon reply remains accepted for
   * compatibility with supervisors that race it ahead of state. */
  acceptsId(id: string): boolean {
    if (!validSessionId(id)) return false;
    if (!this.#hasAuthoritativeIds) return true;
    return this.#knownIds.has(id);
  }

  cacheStats(): { entries: number; bytes: number } {
    return { entries: this.#artwork.size, bytes: this.#artworkBytes };
  }

  stateStats(): { apps: number; catalog: number; bytes: number } {
    return {
      apps: this.#apps.length,
      catalog: this.#catalog.length,
      bytes: this.#appsBytes + this.#catalogBytes,
    };
  }

  /** Release all object URLs. Safe to call more than once. */
  dispose(): void {
    for (const id of [...this.#artwork.keys()]) this.#dropArtwork(id);
  }

  #touchArtwork(id: string) {
    const artwork = this.#artwork.get(id);
    if (artwork === undefined) return undefined;
    this.#artwork.delete(id);
    this.#artwork.set(id, artwork);
    return artwork;
  }

  #artworkCost(
    id: string,
    artwork: {
      path: string | null | undefined;
      icon: string | null | undefined;
      blobBytes: number;
    },
  ): number {
    return (
      keyedBytes(id) +
      (typeof artwork.path === "string"
        ? retainedStringBytes(artwork.path)
        : 0) +
      (typeof artwork.icon === "string"
        ? retainedStringBytes(artwork.icon)
        : 0) +
      artwork.blobBytes
    );
  }

  #dropArtwork(id: string): void {
    const artwork = this.#artwork.get(id);
    if (artwork === undefined) return;
    this.#artwork.delete(id);
    this.#artworkBytes -= this.#artworkCost(id, artwork);
    if (typeof artwork.icon === "string") {
      this.options.onIconEvicted?.(artwork.icon);
    }
  }

  #storeArtwork(
    id: string,
    artwork: {
      path: string | null | undefined;
      icon: string | null | undefined;
      blobBytes: number;
    },
  ): void {
    const previous = this.#artwork.get(id);
    if (previous !== undefined) {
      this.#artworkBytes -= this.#artworkCost(id, previous);
      this.#artwork.delete(id);
    }
    this.#artwork.set(id, artwork);
    this.#artworkBytes += this.#artworkCost(id, artwork);
    while (
      this.#artwork.size > SESSION_ARTWORK_MAX_ENTRIES ||
      this.#artworkBytes > SESSION_ARTWORK_MAX_BYTES
    ) {
      const oldest = this.#artwork.keys().next().value as string | undefined;
      if (oldest === undefined) break;
      this.#dropArtwork(oldest);
    }
  }

  #setPath(id: string, path: string | null): void {
    if (!this.acceptsId(id)) return;
    const previous = this.#artwork.get(id);
    let icon = previous?.icon;
    let blobBytes = previous?.blobBytes ?? 0;
    if (path === null || previous?.path !== path) {
      if (typeof icon === "string") this.options.onIconEvicted?.(icon);
      icon = path === null ? null : undefined;
      blobBytes = 0;
    }
    this.#storeArtwork(id, { path, icon, blobBytes });
  }

  #reconcileArtwork(): void {
    const ids = new Set<string>();
    for (const app of this.#apps) ids.add(app.id);
    for (const entry of this.#catalog) ids.add(entry.id);
    this.#knownIds = ids;
    for (const id of [...this.#artwork.keys()]) {
      if (!ids.has(id)) this.#dropArtwork(id);
    }
    this.options.onIdsChanged?.(ids);
  }

  /** Apply one channel payload. Malformed messages are dropped, not thrown:
   *  a panel is not the place to surface a parser disagreement. */
  apply(payload: Uint8Array): void {
    let message: unknown;
    try {
      message = JSON.parse(new TextDecoder().decode(payload));
    } catch {
      return;
    }
    if (typeof message !== "object" || message === null) return;
    const record = message as Record<string, unknown>;

    // One id per message, and a missing `icon` is the answer "there is none" —
    // which has to be recorded, or the panel asks again on the next render.
    // The supervisor answers with a *path*, not the artwork: a 30 KB data URL
    // per row had to be base64‑encoded and JSON‑escaped inside a Wasm
    // interpreter, which cost more than everything else the panel does put
    // together. The bytes are fetched here instead, natively, over `FS_READ`.
    if (record.type === "icon") {
      if (!validSessionId(record.id) || !this.acceptsId(record.id)) return;
      const path = record.path;
      if (
        typeof path === "string" &&
        path.length > 0 &&
        path.length <= SESSION_MAX_ICON_PATH_CHARS &&
        !path.includes("\0")
      ) {
        this.#setPath(record.id, path);
      } else {
        // No artwork to be had, which is a final answer.
        this.#setPath(record.id, null);
      }
      this.#notifier.emit();
      return;
    }
    if (record.type !== "state") return;

    const hasApps = Array.isArray(record.apps);
    const hasCatalog = Array.isArray(record.catalog);
    if (hasApps) {
      const budget = Math.max(
        0,
        SESSION_MAX_STATE_BYTES - (hasCatalog ? 0 : this.#catalogBytes),
      );
      const parsed = boundedRows(
        record.apps as unknown[],
        appOf,
        (app) => app.id,
        appBytes,
        SESSION_MAX_APPS,
        budget,
      );
      this.#apps = parsed.rows.sort(byName);
      this.#appsBytes = parsed.bytes;
      this.#ready = true;
    }
    // Absent on an update; only a greeting or a resync carries it, because it
    // is the larger half and changes only when packages do.
    if (hasCatalog) {
      const parsed = boundedRows(
        record.catalog as unknown[],
        entryOf,
        (entry) => entry.id,
        catalogEntryBytes,
        SESSION_MAX_CATALOG_ENTRIES,
        Math.max(0, SESSION_MAX_STATE_BYTES - this.#appsBytes),
      );
      this.#catalog = parsed.rows.sort(byName);
      this.#catalogBytes = parsed.bytes;
    }
    if (hasApps || hasCatalog) {
      this.#hasAuthoritativeIds = true;
      this.#reconcileArtwork();
    }
    this.#notifier.emit();
  }
}

export async function openSession(
  connection: ChannelOpener,
  options: SessionOptions = {},
): Promise<SessionHandle> {
  let reconcileRequests: (ids: ReadonlySet<string>) => void = () => {};
  // Installed after the channel opens. A greeting can arrive during
  // connectChannel(), but an icon cannot arrive until requestIcons() is
  // available to its caller.
  let loadArtwork: () => Promise<void> = async () => {};
  const mirror = new SessionMirror({
    onIconEvicted: (url) => URL.revokeObjectURL(url),
    onIdsChanged: (ids) => reconcileRequests(ids),
  });
  let closed = false;
  let transportClosed = false;
  let cleanupLocal: (() => void) | undefined;
  let flushPending: () => void = () => {};
  let channel: ChannelMessageHandle | null = null;
  const channelHandle = await connection.connectChannel(SESSION_CHANNEL, {
    onData: (payload: Uint8Array) => {
      if (closed) return;
      mirror.apply(payload);
      // Icon replies carry paths, not bytes. Start the native read as part of
      // handling the reply instead of waiting for the retry poll below.
      void loadArtwork();
    },
    onCredit: () => flushPending(),
    onClosed: (reason: number, detail: string) => {
      transportClosed = true;
      channel = null;
      cleanupLocal?.();
      options.onClosed?.(reason, detail);
    },
  });
  if (!transportClosed) channel = channelHandle;

  // Unlike the systemd watcher, this one greets with full state and the
  // catalog, so no opening request is needed.
  const request = (line: string): boolean => channel?.send(line) ?? false;

  // Control verbs share channel credit with icon batches. A click must not be
  // discarded merely because artwork consumed the current window: retain a
  // small FIFO and give it priority when credit returns.
  const pendingCommands: string[] = [];
  let pendingCommandBytes = 0;
  const commandBytes = (line: string) => 16 + retainedStringBytes(line);
  const flushCommands = (): boolean => {
    while (pendingCommands.length > 0) {
      const line = pendingCommands[0]!;
      if (!request(line)) return false;
      pendingCommands.shift();
      pendingCommandBytes -= commandBytes(line);
    }
    return true;
  };
  const command = (line: string): void => {
    if (closed) return;
    if (pendingCommands.length === 0 && request(line)) return;
    const bytes = commandBytes(line);
    if (
      pendingCommands.length >= SESSION_COMMAND_QUEUE_MAX_ENTRIES ||
      bytes > SESSION_COMMAND_QUEUE_MAX_BYTES - pendingCommandBytes
    ) {
      return;
    }
    pendingCommands.push(line);
    pendingCommandBytes += bytes;
    flushPending();
  };

  // When each id was last asked about. Separate from what the mirror holds
  // because a request is outstanding for a round trip, and a panel re-rendering
  // in that window would otherwise ask again for every row on screen — but it
  // expires, so an answer that never came is asked for again rather than
  // leaving one row a placeholder forever.
  const asked = new Map<string, number>();
  let askedBytes = 0;
  const dropAsked = (id: string): void => {
    if (!asked.delete(id)) return;
    askedBytes -= keyedBytes(id);
  };
  const markAsked = (id: string, now: number): void => {
    dropAsked(id);
    asked.set(id, now);
    askedBytes += keyedBytes(id);
    while (
      asked.size > SESSION_ICON_REQUEST_MAX_ENTRIES ||
      askedBytes > SESSION_ICON_REQUEST_MAX_BYTES
    ) {
      const oldest = asked.keys().next().value as string | undefined;
      if (oldest === undefined) break;
      dropAsked(oldest);
    }
  };
  const worthAsking = (id: string, now: number): boolean => {
    if (!validSessionId(id) || !mirror.acceptsId(id)) return false;
    // A located icon counts as answered even before its bytes arrive: the read
    // is the panel's own job now, and asking the supervisor again would only
    // repeat an answer it already gave.
    if (mirror.path(id) !== undefined) return false;
    const at = asked.get(id);
    return at === undefined || now - at >= ICON_RETRY_MS;
  };
  // Ids waiting to be asked about, and the timer that will ask.
  const queued = new Set<string>();
  let queuedBytes = 0;
  let coalescing: ReturnType<typeof setTimeout> | undefined;
  const flushIcons = () => {
    coalescing = undefined;
    // Newline-separated, not space: a desktop-entry id is a filename, and Steam
    // alone installs hundreds with spaces in them ("3DMark Demo.desktop").
    while (queued.size > 0) {
      const batch = [...queued].slice(0, ICON_BATCH);
      // A newly opened native channel starts with no outgoing credit. Keep the
      // exact batch queued until send accepts it; onCredit schedules us again.
      if (!request(`icons ${batch.join("\n")}`)) return;
      const now = Date.now();
      for (const id of batch) {
        queued.delete(id);
        queuedBytes -= keyedBytes(id);
        markAsked(id, now);
      }
    }
  };
  const scheduleIconFlush = () => {
    if (closed || queued.size === 0 || coalescing !== undefined) return;
    coalescing = setTimeout(flushIcons, ICON_REQUEST_COALESCE_MS);
  };
  flushPending = () => {
    if (!flushCommands()) return;
    scheduleIconFlush();
  };

  reconcileRequests = (ids) => {
    for (const id of [...asked.keys()]) {
      if (!ids.has(id)) dropAsked(id);
    }
    for (const id of [...queued]) {
      if (ids.has(id)) continue;
      queued.delete(id);
      queuedBytes -= keyedBytes(id);
    }
    if (queued.size === 0 && coalescing !== undefined) {
      clearTimeout(coalescing);
      coalescing = undefined;
    }
  };

  // Artwork the panel reads for itself. Object URLs rather than data URLs: the
  // bytes arrive as bytes, and turning them into base64 to hand to an <img> is
  // the work this whole arrangement exists to avoid.
  let loading = false;
  loadArtwork = async () => {
    if (closed || loading || !connection.readFiles) return;
    loading = true;
    try {
      // A few screens at a time: a batch is one message, and the panel wants
      // the rows it is drawing to arrive before the ones it is not.
      for (let guard = 0; guard < SESSION_ARTWORK_LOAD_MAX_ROUNDS; guard += 1) {
        if (closed) return;
        const wanted = mirror.unread().slice(0, ICON_BATCH);
        if (wanted.length === 0) return;
        const records = await connection.readFiles(
          [wanted.map((entry) => entry.path)],
          {
            // FS receive credit covers the whole query, not each question.
            // Giving a 48-file batch one file's 1 MiB allowance truncated the
            // reply after the first few icons and made the rest look absent.
            maxBytes: SESSION_MAX_ARTWORK_FILE_BYTES * wanted.length,
          },
        );
        if (closed) return;
        for (const [index, record] of records.entries()) {
          const entry = wanted[index];
          if (!entry) continue;
          if (record.status === YAS_STATUS_RESOURCE_EXHAUSTED) continue;
          if (
            record.status !== 0 ||
            record.content.length === 0 ||
            record.content.byteLength > SESSION_MAX_ARTWORK_FILE_BYTES
          ) {
            mirror.setIcon(entry.id, null);
            continue;
          }
          const url = URL.createObjectURL(
            new Blob([record.content as BlobPart], {
              type: entry.path.endsWith(".svg") ? "image/svg+xml" : "image/png",
            }),
          );
          mirror.setIcon(entry.id, url, record.content.byteLength);
        }
      }
    } catch {
      // A refused or unanswered read leaves the ids unread, so the next batch
      // picks them up rather than the rows keeping a placeholder forever.
    } finally {
      loading = false;
    }
  };
  const artworkTick = setInterval(
    () => void loadArtwork(),
    ICON_ARTWORK_RETRY_MS,
  );

  cleanupLocal = () => {
    if (closed) return;
    closed = true;
    if (coalescing !== undefined) clearTimeout(coalescing);
    coalescing = undefined;
    queued.clear();
    queuedBytes = 0;
    pendingCommands.length = 0;
    pendingCommandBytes = 0;
    asked.clear();
    askedBytes = 0;
    clearInterval(artworkTick);
    // An object URL is a document-lifetime reference: without this every
    // panel that was opened keeps every icon it ever drew.
    mirror.dispose();
  };
  if (transportClosed) cleanupLocal();

  return {
    get apps() {
      return mirror.apps;
    },
    get catalog() {
      return mirror.catalog;
    },
    get ready() {
      return mirror.ready;
    },
    get revision() {
      return mirror.revision;
    },
    icon: (id: string) => mirror.icon(id),
    requestIcons: (ids: readonly string[]) => {
      if (closed) return;
      const now = Date.now();
      for (const id of ids) {
        if (queued.has(id) || !worthAsking(id, now)) continue;
        const bytes = keyedBytes(id);
        if (
          queued.size >= SESSION_ICON_QUEUE_MAX_ENTRIES ||
          bytes > SESSION_ICON_QUEUE_MAX_BYTES - queuedBytes
        ) {
          continue;
        }
        queued.add(id);
        queuedBytes += bytes;
      }
      scheduleIconFlush();
    },
    subscribe: mirror.subscribe,
    enable: (id: string) => {
      if (validSessionId(id)) command(`enable ${id}`);
    },
    disable: (id: string) => {
      if (validSessionId(id)) command(`disable ${id}`);
    },
    start: (id: string) => {
      if (validSessionId(id)) command(`start ${id}`);
    },
    stop: (id: string) => {
      if (validSessionId(id)) command(`stop ${id}`);
    },
    forget: (id: string) => {
      if (validSessionId(id)) command(`forget ${id}`);
    },
    resync: () => command("resync"),
    close: () => {
      if (closed) return;
      cleanupLocal?.();
      channelHandle.close();
      channel = null;
    },
  };
}

// Whether a supervisor serves a connection is no longer asked here: the
// question is about the server's channel registry rather than about sessions,
// and it is followed for both extension channels at once in
// `channelPresence.ts`.
