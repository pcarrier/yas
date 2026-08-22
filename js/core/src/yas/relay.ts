import type {
  YasTransportEventMap,
  YasTransportMessage,
  ConnectionStatus,
} from "../types";
import { YAS_FAMILY_RELAY } from "./core";
import {
  YAS_RELAY_AVAILABILITY_AVAILABLE,
  YAS_RELAY_AVAILABILITY_DEGRADED,
  YAS_RELAY_AVAILABILITY_UNAVAILABLE,
  YAS_RELAY_AVAILABILITY_UNKNOWN,
  YAS_RELAY_CONNECT,
  YAS_RELAY_CONNECT_TIMEOUT_NS,
  YAS_RELAY_DISCONNECT,
  YAS_RELAY_EARLY_DATA_EXTENSION,
  YAS_RELAY_ROUTE_DEFAULT,
  YAS_RELAY_LIMIT_CONNECT_TIMEOUT_NS,
  YAS_RELAY_LIMIT_MAX_BUFFERED_PER_LINK,
  YAS_RELAY_LIMIT_MAX_EARLY_DATA,
  YAS_RELAY_LIMIT_MAX_LINKS_PER_SESSION,
  YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS,
  YAS_RELAY_LIMIT_MAX_ROUTES,
  YAS_RELAY_MAX_BUFFERED_PER_LINK,
  YAS_RELAY_MAX_EARLY_DATA,
  YAS_RELAY_MAX_LINKS_PER_SESSION,
  YAS_RELAY_MAX_PENDING_CONNECTS,
  YAS_RELAY_MAX_ROUTES,
  YAS_RELAY_STATE,
  YAS_RELAY_STATE_ACK,
  YAS_RELAY_TUNNEL_CONTENT_KIND,
  YAS_RELAY_TRANSPORT_LOCAL,
  YAS_RELAY_TRANSPORT_OTHER,
  YAS_RELAY_TRANSPORT_RELAY,
  YAS_RELAY_TRANSPORT_SSH,
  YAS_RELAY_TRANSPORT_TCP,
  YAS_RELAY_TRANSPORT_UPLINK,
  YAS_RELAY_TRANSPORT_WEBRTC,
  YAS_RELAY_UNWATCH,
  YAS_RELAY_VERSION,
  YAS_RELAY_WATCH,
} from "./generated";
import {
  YasConnection,
  type YasConnectionOptions,
  type YasTransport,
} from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_PATCH,
  YAS_STATE_REMOVE,
  YAS_STATE_REPLACE,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YasStateCatalogueRetention,
  YasStateSubscription,
  estimateStateRetainedBytes,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeTransferDescriptor,
  transfersFor,
  type YasTransfer,
} from "./transfer";
import {
  YAS_STATUS_CANCELLED,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_RELAY_AVAILABILITY_AVAILABLE,
  YAS_RELAY_AVAILABILITY_DEGRADED,
  YAS_RELAY_AVAILABILITY_UNAVAILABLE,
  YAS_RELAY_AVAILABILITY_UNKNOWN,
  YAS_RELAY_CONNECT,
  YAS_RELAY_DISCONNECT,
  YAS_RELAY_ROUTE_DEFAULT,
  YAS_RELAY_STATE,
  YAS_RELAY_STATE_ACK,
  YAS_RELAY_TRANSPORT_LOCAL,
  YAS_RELAY_TRANSPORT_OTHER,
  YAS_RELAY_TRANSPORT_RELAY,
  YAS_RELAY_TRANSPORT_SSH,
  YAS_RELAY_TRANSPORT_TCP,
  YAS_RELAY_TRANSPORT_UPLINK,
  YAS_RELAY_TRANSPORT_WEBRTC,
  YAS_RELAY_UNWATCH,
  YAS_RELAY_VERSION,
  YAS_RELAY_WATCH,
} from "./generated";

export interface YasRelayRoute {
  handle: bigint;
  generation: bigint;
  availability: number;
  transportHint: number;
  flags: number;
  name: string;
  label: string;
  description: string;
  extensions: readonly YasExtension[];
}

export interface YasRelaySnapshot {
  revision: bigint;
  routes: readonly YasRelayRoute[];
}

export interface YasRelayConnectOptions {
  initialReceiveCredit?: bigint;
  earlyData?: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasRelayLink {
  relayHandle: bigint;
  route: YasRelayRoute;
  transfer: YasTransfer;
  transport: YasRelayTunnelTransport;
}

export interface YasRelayLimits {
  maxRoutes: number;
  maxLinksPerSession: number;
  maxPendingConnects: number;
  maxEarlyData: number;
  connectTimeoutNs: bigint;
  maxBufferedPerLink: bigint;
}

export function relayLimitsFromExtensions(
  extensions: readonly YasExtension[],
): YasRelayLimits {
  rejectUnknownRequiredLimits(
    extensions,
    new Set([
      YAS_RELAY_LIMIT_MAX_ROUTES,
      YAS_RELAY_LIMIT_MAX_LINKS_PER_SESSION,
      YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS,
      YAS_RELAY_LIMIT_MAX_EARLY_DATA,
      YAS_RELAY_LIMIT_CONNECT_TIMEOUT_NS,
      YAS_RELAY_LIMIT_MAX_BUFFERED_PER_LINK,
    ]),
    "Relay",
  );
  const value = {
    maxRoutes: relayLimitU32(extensions, YAS_RELAY_LIMIT_MAX_ROUTES),
    maxLinksPerSession: relayLimitU32(
      extensions,
      YAS_RELAY_LIMIT_MAX_LINKS_PER_SESSION,
    ),
    maxPendingConnects: relayLimitU32(
      extensions,
      YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS,
    ),
    maxEarlyData: relayLimitU32(extensions, YAS_RELAY_LIMIT_MAX_EARLY_DATA),
    connectTimeoutNs: relayLimitU64(
      extensions,
      YAS_RELAY_LIMIT_CONNECT_TIMEOUT_NS,
    ),
    maxBufferedPerLink: relayLimitU64(
      extensions,
      YAS_RELAY_LIMIT_MAX_BUFFERED_PER_LINK,
    ),
  };
  validateRelayLimits(value);
  return value;
}

export function relayLimitsExtensions(value: YasRelayLimits): YasExtension[] {
  validateRelayLimits(value);
  return [
    limit32(YAS_RELAY_LIMIT_MAX_ROUTES, value.maxRoutes),
    limit32(YAS_RELAY_LIMIT_MAX_LINKS_PER_SESSION, value.maxLinksPerSession),
    limit32(YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS, value.maxPendingConnects),
    limit32(YAS_RELAY_LIMIT_MAX_EARLY_DATA, value.maxEarlyData),
    limit64(YAS_RELAY_LIMIT_CONNECT_TIMEOUT_NS, value.connectTimeoutNs),
    limit64(YAS_RELAY_LIMIT_MAX_BUFFERED_PER_LINK, value.maxBufferedPerLink),
  ];
}

export const YAS_RELAY_HARD_LIMITS: YasRelayLimits = {
  maxRoutes: YAS_RELAY_MAX_ROUTES,
  maxLinksPerSession: YAS_RELAY_MAX_LINKS_PER_SESSION,
  maxPendingConnects: YAS_RELAY_MAX_PENDING_CONNECTS,
  maxEarlyData: YAS_RELAY_MAX_EARLY_DATA,
  connectTimeoutNs: BigInt(YAS_RELAY_CONNECT_TIMEOUT_NS),
  maxBufferedPerLink: BigInt(YAS_RELAY_MAX_BUFFERED_PER_LINK),
};

export function decodeRelayRoute(body: Uint8Array): YasRelayRoute {
  const cursor = new YasCursor(body);
  const handle = cursor.u64("route handle");
  const generation = cursor.u64("route generation");
  const availability = cursor.u8("route availability");
  const transportHint = cursor.u8("route transport hint");
  const flags = cursor.u16("route flags");
  const name = cursor.utf8U16("route name");
  const label = cursor.utf8U16("route label");
  const description = cursor.utf8U32("route description");
  const extensions = decodeExtensions(cursor, undefined, "route extensions");
  cursor.end("Relay route record");
  if (handle === 0n || generation === 0n)
    throw new YasProtocolError("Relay route handle or generation is zero");
  if (
    availability > YAS_RELAY_AVAILABILITY_UNAVAILABLE ||
    transportHint > YAS_RELAY_TRANSPORT_RELAY
  )
    throw new YasProtocolError("Relay route enum is invalid");
  if (flags & ~YAS_RELAY_ROUTE_DEFAULT)
    throw new YasProtocolError("reserved Relay route flags are nonzero");
  return {
    handle,
    generation,
    availability,
    transportHint,
    flags,
    name,
    label,
    description,
    extensions,
  };
}

function encodeRelayRouteRecord(value: YasRelayRoute): Uint8Array {
  return new YasWriter()
    .u64(value.handle)
    .u64(value.generation)
    .u8(value.availability)
    .u8(value.transportHint)
    .u16(value.flags)
    .utf8U16(value.name)
    .utf8U16(value.label)
    .utf8U32(value.description)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

/** Materializes the Relay state convention into a stable route catalogue. */
export class YasRelayRoutes {
  private current = new Map<bigint, YasRelayRoute>();
  private currentRetention: YasStateCatalogueRetention<bigint>;
  private staging: Map<bigint, YasRelayRoute> | null = null;
  private stagingRetention: YasStateCatalogueRetention<bigint> | null = null;
  private subscription: YasStateSubscription | null = null;
  private listeners = new Set<(snapshot: YasRelaySnapshot) => void>();
  private readonly snapshotRejectors = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private epoch = 0;
  private disposed = false;
  private _revision = 0n;

  constructor(
    private readonly connection: YasConnection,
    private readonly limits: () => YasRelayLimits = () => YAS_RELAY_HARD_LIMITS,
  ) {
    this.currentRetention =
      YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_RELAY) {
        this.epoch++;
        this.subscription = null;
        const error = new YasProtocolError("Relay catalogue was invalidated");
        this.pendingWatchCancel?.(error);
        this.cancelSnapshots(error);
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasRelaySnapshot {
    return {
      revision: this._revision,
      routes: [...this.current.values()].sort((left, right) =>
        left.name.localeCompare(right.name),
      ),
    };
  }

  subscribe(listener: (snapshot: YasRelaySnapshot) => void): () => void {
    this.assertOpen();
    this.listeners.add(listener);
    try {
      listener(this.snapshot);
    } catch {
      // One observer cannot block catalogue delivery or cleanup.
    }
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions = {},
  ): Promise<YasRelaySnapshot> {
    this.assertOpen();
    if (this._revision !== 0n && this.subscription?.active)
      return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectSnapshot: ((error: unknown) => void) | undefined;
    const result = new Promise<YasRelaySnapshot>((resolve, reject) => {
      let settled = false;
      const finish = (snapshot?: YasRelaySnapshot, error?: unknown) => {
        if (settled) return;
        settled = true;
        remove?.();
        if (rejectSnapshot) this.snapshotRejectors.delete(rejectSnapshot);
        if (error !== undefined) reject(error);
        else resolve(snapshot!);
      };
      rejectSnapshot = (error) => finish(undefined, error);
      this.snapshotRejectors.add(rejectSnapshot);
      remove = this.subscribe((snapshot) => {
        if (snapshot.revision === 0n) return;
        finish(snapshot);
      });
    });
    try {
      return await Promise.race([
        result,
        this.watch(options).then(() => result),
      ]);
    } catch (error) {
      remove?.();
      if (rejectSnapshot) this.snapshotRejectors.delete(rejectSnapshot);
      throw error;
    }
  }

  async watch(options: YasWatchOptions = {}): Promise<void> {
    this.assertOpen();
    if (this.subscription?.active) return;
    if (this.pendingWatch) return this.pendingWatch;
    this.subscription = null;
    this.resetLocal();
    const epoch = this.epoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      YAS_FAMILY_RELAY,
      YAS_RELAY_WATCH,
      YAS_RELAY_UNWATCH,
      YAS_RELAY_STATE,
      YAS_RELAY_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.epoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.epoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Relay catalogue watch was cancelled");
      }
      this.subscription = subscription;
    });
    const cancelled = new Promise<never>((_, reject) => {
      this.pendingWatchCancel = reject;
    });
    const pending = Promise.race([watched, cancelled]);
    this.pendingWatch = pending;
    try {
      await pending;
    } finally {
      if (this.pendingWatch === pending) this.pendingWatch = null;
      if (this.pendingWatchCancel) this.pendingWatchCancel = null;
    }
  }

  async unwatch(): Promise<void> {
    this.assertOpen();
    this.epoch++;
    this.pendingWatchCancel?.(
      new YasProtocolError("Relay catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    this.resetLocal();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    this.removeInvalidation();
    const subscription = this.subscription;
    this.subscription = null;
    const error = new YasProtocolError("Relay catalogue is disposed");
    this.pendingWatchCancel?.(error);
    this.cancelSnapshots(error);
    this.resetLocal();
    this.listeners.clear();
    void subscription?.unwatch().catch(() => undefined);
  }

  private apply(batch: YasStateBatch): void {
    if (this.disposed) return;
    if (batch.phase === YAS_STATE_RESET) {
      this.currentRetention.dispose();
      this.stagingRetention?.dispose();
      this.staging = null;
      this.stagingRetention = null;
      this.current = new Map();
      this.currentRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      this._revision = 0n;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.stagingRetention?.dispose();
      this.staging = new Map();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Relay snapshot records without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Relay snapshot end without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.currentRetention;
      this.current = this.staging;
      this.currentRetention = this.stagingRetention;
      this.staging = null;
      this.stagingRetention = null;
      previousRetention.dispose();
      this._revision = batch.toRevision;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const nextRetention = this.currentRetention.clone();
      let next: Map<bigint, YasRelayRoute>;
      try {
        next = new Map(this.current);
        this.applyRecords(next, nextRetention, batch.records);
      } catch (error) {
        nextRetention.dispose();
        throw error;
      }
      const previousRetention = this.currentRetention;
      this.current = next;
      this.currentRetention = nextRetention;
      previousRetention.dispose();
      this._revision = batch.toRevision;
      this.emit();
    }
  }

  private validateCatalog(routes: ReadonlyMap<bigint, YasRelayRoute>): void {
    if (routes.size > this.limits().maxRoutes)
      throw new YasProtocolError("Relay catalogue exceeds negotiated limit");
    const names = new Set<string>();
    let defaults = 0;
    for (const route of routes.values()) {
      if (names.has(route.name))
        throw new YasProtocolError("Relay route names are not unique");
      names.add(route.name);
      if (route.flags & YAS_RELAY_ROUTE_DEFAULT) defaults++;
    }
    if (defaults > 1)
      throw new YasProtocolError(
        "Relay catalogue has more than one default route",
      );
  }

  private applyRecords(
    target: Map<bigint, YasRelayRoute>,
    retention: YasStateCatalogueRetention<bigint>,
    records: readonly YasTypedRecord[],
  ): void {
    const originals = new Map<bigint, YasRelayRoute | null>();
    const remember = (key: bigint) => {
      if (!originals.has(key)) originals.set(key, target.get(key) ?? null);
    };
    try {
      for (const record of records) {
        if (
          record.kind === YAS_STATE_ADD ||
          record.kind === YAS_STATE_REPLACE
        ) {
          const decoded = decodeRelayRoute(record.body);
          const encoded = encodeRelayRouteRecord(decoded);
          const route = decodeRelayRoute(encoded);
          const exists = target.has(route.handle);
          if ((record.kind === YAS_STATE_ADD) === exists)
            throw new YasProtocolError(
              "Relay state ADD/REPLACE precondition failed",
            );
          remember(route.handle);
          retention.upsert(
            route.handle,
            Math.max(encoded.length, estimateStateRetainedBytes(route)),
          );
          target.set(route.handle, route);
        } else if (record.kind === YAS_STATE_REMOVE) {
          const cursor = new YasCursor(record.body);
          const handle = cursor.u64("removed route handle");
          const generation = cursor.u64("removed route generation");
          cursor.end("Relay REMOVE record");
          const route = target.get(handle);
          if (!route || route.generation !== generation)
            throw new YasProtocolError(
              "Relay REMOVE names an unknown generation",
            );
          remember(handle);
          retention.remove(handle);
          target.delete(handle);
        } else if (record.kind === YAS_STATE_PATCH) {
          throw new YasProtocolError("Relay v1 does not define PATCH records");
        } else if (record.flags & 1) {
          throw new YasProtocolError("unknown required Relay state record");
        }
      }
      this.validateCatalog(target);
    } catch (error) {
      for (const key of originals.keys()) retention.remove(key);
      for (const [key, original] of originals) {
        if (original) {
          retention.upsert(
            key,
            Math.max(
              encodeRelayRouteRecord(original).length,
              estimateStateRetainedBytes(original),
            ),
          );
          target.set(key, original);
        } else target.delete(key);
      }
      throw error;
    }
  }

  private emit(): void {
    if (this.disposed) return;
    const snapshot = this.snapshot;
    for (const listener of this.listeners) {
      try {
        listener(snapshot);
      } catch {
        // One observer cannot block catalogue delivery or cleanup.
      }
    }
  }

  private resetLocal(): void {
    this.subscription = null;
    this.currentRetention.dispose();
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
    this.current = new Map();
    this.currentRetention = YasStateCatalogueRetention.forConnection(
      this.connection,
    );
    this._revision = 0n;
    this.emit();
  }

  private discardStaging(): void {
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
  }

  private cancelSnapshots(error: unknown): void {
    for (const reject of [...this.snapshotRejectors]) reject(error);
    this.snapshotRejectors.clear();
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("Relay catalogue is disposed");
  }
}

/** A raw nested YAS byte stream carried by a bidirectional Relay Transfer. */
export class YasRelayTunnelTransport implements YasTransport {
  readonly yasFraming = "stream" as const;
  readonly authRejected = false;
  readonly maxDatagramSize = 0;
  lastError: string | null = null;
  private _status: ConnectionStatus = "disconnected";
  private messageListeners = new Set<(data: YasTransportMessage) => void>();
  private statusListeners = new Set<(status: ConnectionStatus) => void>();
  private writeChain = Promise.resolve();
  private queuedBytes = 0;
  private started = false;

  constructor(readonly transfer: YasTransfer) {}

  get status(): ConnectionStatus {
    return this._status;
  }

  get bufferedAmount(): number {
    return this.queuedBytes;
  }

  connect(): void {
    if (this.started || this._status === "closed") return;
    this.started = true;
    this.setStatus("connected");
    void this.pump();
  }

  send(data: Uint8Array): void {
    if (this._status !== "connected")
      throw new YasProtocolError("Relay tunnel is not connected");
    const copy = new Uint8Array(data);
    if (
      copy.length >
      this.transfer.outboundQueueHighWaterMark - this.queuedBytes
    ) {
      const error = new YasProtocolError(
        "Relay tunnel send queue exceeded its negotiated high-water mark",
      );
      this.transfer.reset(
        YAS_STATUS_RESOURCE_EXHAUSTED,
        new TextEncoder().encode(error.message),
      );
      this.fail(error);
      throw error;
    }
    this.queuedBytes += copy.length;
    this.writeChain = this.writeChain
      .then(() => this.transfer.write(copy))
      .then(
        () => {
          this.queuedBytes -= copy.length;
        },
        (error) => {
          this.queuedBytes -= copy.length;
          this.fail(error);
        },
      );
  }

  close(): void {
    if (this._status === "closed") return;
    this.transfer.reset(YAS_STATUS_CANCELLED);
    this.setStatus("closed");
  }

  suspend(): void {
    this.close();
  }

  addEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messageListeners.add(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "statuschange")
      this.statusListeners.add(listener as (status: ConnectionStatus) => void);
  }

  removeEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messageListeners.delete(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "statuschange")
      this.statusListeners.delete(
        listener as (status: ConnectionStatus) => void,
      );
  }

  private async pump(): Promise<void> {
    try {
      while (this._status === "connected") {
        const chunk = await this.transfer.read();
        if (chunk === null) {
          this.transfer.closeWrite();
          this.setStatus("closed");
          return;
        }
        for (const listener of this.messageListeners) {
          try {
            listener(chunk);
          } catch {
            // One observer cannot fail tunnel delivery for its siblings.
          }
        }
      }
    } catch (error) {
      this.fail(error);
    }
  }

  private fail(error: unknown): void {
    this.lastError = error instanceof Error ? error.message : String(error);
    this.setStatus("error");
  }

  private setStatus(status: ConnectionStatus): void {
    if (this._status === status) return;
    this._status = status;
    for (const listener of this.statusListeners) {
      try {
        listener(status);
      } catch {
        // One observer cannot fail status delivery for its siblings.
      }
    }
  }
}

export class YasRelayClient {
  readonly routes: YasRelayRoutes;
  private readonly transfers;
  private readonly links = new Map<bigint, YasRelayLink>();
  private readonly pendingCancels = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private pendingConnects = 0;
  private epoch = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    connection.family(YAS_FAMILY_RELAY, YAS_RELAY_VERSION);
    connection.registerFamilyLimitValidator(
      YAS_FAMILY_RELAY,
      relayLimitsFromExtensions,
    );
    this.transfers = transfersFor(connection);
    this.routes = new YasRelayRoutes(connection, () => this.limits);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_RELAY) {
        this.epoch++;
        // A full-session failure already rejects every pending wire Request
        // with its root protocol/transport error. Only a live family-scoped
        // invalidation needs this local cancellation race.
        if (family !== undefined) {
          const error = new YasProtocolError(
            "YAS Relay client was invalidated",
          );
          for (const cancel of [...this.pendingCancels]) cancel(error);
          this.pendingCancels.clear();
        }
        this.closeLinks();
      }
    });
  }

  get limits(): YasRelayLimits {
    return relayLimitsFromExtensions(
      this.connection.family(YAS_FAMILY_RELAY, YAS_RELAY_VERSION).limits,
    );
  }

  listRoutes(options: YasWatchOptions = {}): Promise<YasRelaySnapshot> {
    return this.routes.firstSnapshot(options);
  }

  connect(
    route: YasRelayRoute,
    options: YasRelayConnectOptions = {},
  ): Promise<YasRelayLink> {
    this.assertOpen();
    const epoch = this.epoch;
    return this.runOwned(this.performConnect(route, options, epoch));
  }

  private async performConnect(
    route: YasRelayRoute,
    options: YasRelayConnectOptions,
    epoch: number,
  ): Promise<YasRelayLink> {
    const limits = this.limits;
    const earlyData = options.earlyData ?? new Uint8Array(0);
    if (earlyData.length > limits.maxEarlyData)
      throw new YasProtocolError("Relay early_data exceeds negotiated limit");
    if (this.links.size >= limits.maxLinksPerSession)
      throw new YasProtocolError("Relay link limit is exhausted");
    if (this.pendingConnects >= limits.maxPendingConnects)
      throw new YasProtocolError("Relay pending-connect limit is exhausted");
    this.pendingConnects++;
    const lease = this.transfers.reserveReceiveCredit(
      options.initialReceiveCredit ?? 1024n * 1024n,
      1024n,
    );
    const extensions = [...(options.extensions ?? [])];
    if (earlyData.length !== 0)
      extensions.push({
        tag: YAS_RELAY_EARLY_DATA_EXTENSION,
        value: earlyData,
      });
    extensions.sort((left, right) => left.tag - right.tag);
    const payload = new YasWriter()
      .u64(route.handle)
      .u64(route.generation)
      .u64(lease.bytes)
      .u16(0)
      .u16(0)
      .bytes(encodeExtensions(extensions))
      .finish();
    let transferAccepted = false;
    try {
      return await this.connection.requestDecoded(
        YAS_FAMILY_RELAY,
        YAS_RELAY_CONNECT,
        payload,
        (body) => {
          const cursor = new YasCursor(body);
          const relayHandle = cursor.u64("relay handle");
          const routeHandle = cursor.u64("connected route handle");
          const generation = cursor.u64("connected route generation");
          const descriptor = decodeTransferDescriptor(cursor);
          cursor.end("Relay CONNECT Result");
          if (
            relayHandle === 0n ||
            routeHandle !== route.handle ||
            generation !== route.generation
          )
            throw new YasProtocolError(
              "Relay CONNECT Result does not match its request",
            );
          if (
            descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
            descriptor.direction !==
              (YAS_TRANSFER_RECEIVER_TO_SENDER |
                YAS_TRANSFER_SENDER_TO_RECEIVER) ||
            descriptor.contentFamily !== YAS_FAMILY_RELAY ||
            descriptor.contentKind !== YAS_RELAY_TUNNEL_CONTENT_KIND ||
            descriptor.contentVersion !== YAS_RELAY_VERSION
          )
            throw new YasProtocolError(
              "Relay CONNECT returned the wrong Transfer content type",
            );
          if (!descriptor.sensitiveContent)
            throw new YasProtocolError(
              "Relay CONNECT returned a nonsensitive tunnel Transfer",
            );
          const transfer = this.transfers.acceptServerDescriptor(
            descriptor,
            lease,
          );
          transferAccepted = true;
          if (this.disposed || epoch !== this.epoch) {
            transfer.reset(YAS_STATUS_CANCELLED);
            throw new YasProtocolError(
              "Relay CONNECT completed after disposal",
            );
          }
          if (this.links.has(relayHandle)) {
            transfer.reset(YAS_STATUS_CANCELLED);
            throw new YasProtocolError("Relay handle was reused");
          }
          const transport = new YasRelayTunnelTransport(transfer);
          const link = { relayHandle, route, transfer, transport };
          this.links.set(relayHandle, link);
          void transfer.closed.then(() => this.links.delete(relayHandle));
          return link;
        },
      );
    } catch (error) {
      if (!transferAccepted) lease.release();
      throw error;
    } finally {
      this.pendingConnects--;
    }
  }

  async connectYas(
    route: YasRelayRoute,
    yasOptions: YasConnectionOptions,
    relayOptions: YasRelayConnectOptions = {},
  ): Promise<YasRelayLink & { connection: YasConnection }> {
    if (relayOptions.earlyData?.length)
      throw new YasProtocolError(
        "connectYas owns the nested preface; use connect() for caller-built early_data",
      );
    const link = await this.connect(route, relayOptions);
    const connection = new YasConnection(link.transport, yasOptions);
    try {
      await connection.connect();
      return { ...link, connection };
    } catch (error) {
      link.transport.close();
      throw error;
    }
  }

  async disconnect(
    relayHandle: bigint,
    reason = "client disconnect",
  ): Promise<void> {
    this.assertOpen();
    await this.requestDisconnect(relayHandle, reason);
    this.links.get(relayHandle)?.transport.close();
    this.links.delete(relayHandle);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    const error = new YasProtocolError("YAS Relay client was disposed");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    this.removeInvalidation();
    this.routes.dispose();
    for (const relayHandle of this.links.keys())
      void this.requestDisconnect(relayHandle, "client disposed").catch(
        () => undefined,
      );
    this.closeLinks();
  }

  private requestDisconnect(
    relayHandle: bigint,
    reason: string,
  ): Promise<Uint8Array> {
    const reasonBytes = new TextEncoder().encode(reason);
    return this.connection.request(
      YAS_FAMILY_RELAY,
      YAS_RELAY_DISCONNECT,
      new YasWriter().u64(relayHandle).bytesU32(reasonBytes).finish(),
    );
  }

  private closeLinks(): void {
    for (const link of this.links.values()) {
      try {
        link.transport.close();
      } catch {
        // The Transfer registry may already have been invalidated.
      }
    }
    this.links.clear();
  }

  private runOwned<T>(operation: Promise<T>): Promise<T> {
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = reject;
    });
    this.pendingCancels.add(cancel);
    return Promise.race([operation, cancelled]).finally(() => {
      this.pendingCancels.delete(cancel);
    });
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("Relay client is disposed");
  }
}

function rejectUnknownRequiredLimits(
  extensions: readonly YasExtension[],
  known: ReadonlySet<number>,
  family: string,
): void {
  if (
    extensions.some(
      (extension) => extension.required && !known.has(extension.tag),
    )
  )
    throw new YasProtocolError(`unknown required ${family} family limit`);
}

function relayLimitU32(
  extensions: readonly YasExtension[],
  tag: number,
): number {
  const extension = extensions.find((value) => value.tag === tag);
  if (!extension) throw new YasProtocolError("missing Relay family limit");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u32("Relay family limit");
  cursor.end("Relay family limit");
  return value;
}

function relayLimitU64(
  extensions: readonly YasExtension[],
  tag: number,
): bigint {
  const extension = extensions.find((value) => value.tag === tag);
  if (!extension) throw new YasProtocolError("missing Relay family limit");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u64("Relay family limit");
  cursor.end("Relay family limit");
  return value;
}

function validateRelayLimits(value: YasRelayLimits): void {
  if (
    value.maxRoutes <= 0 ||
    value.maxRoutes > YAS_RELAY_MAX_ROUTES ||
    value.maxLinksPerSession <= 0 ||
    value.maxLinksPerSession > YAS_RELAY_MAX_LINKS_PER_SESSION ||
    value.maxPendingConnects <= 0 ||
    value.maxPendingConnects > YAS_RELAY_MAX_PENDING_CONNECTS ||
    value.maxPendingConnects > value.maxLinksPerSession ||
    value.maxEarlyData < 0 ||
    value.maxEarlyData > YAS_RELAY_MAX_EARLY_DATA ||
    value.connectTimeoutNs <= 0n ||
    value.connectTimeoutNs > BigInt(YAS_RELAY_CONNECT_TIMEOUT_NS) ||
    value.maxBufferedPerLink <= 0n ||
    value.maxBufferedPerLink > BigInt(YAS_RELAY_MAX_BUFFERED_PER_LINK)
  )
    throw new YasProtocolError("invalid Relay family limits");
}

function limit32(tag: number, value: number): YasExtension {
  return { tag, value: new YasWriter().u32(value).finish() };
}

function limit64(tag: number, value: bigint): YasExtension {
  return { tag, value: new YasWriter().u64(value).finish() };
}
