import {
  YAS_CLIENT_ACTIVE_SUBSCRIPTIONS_EXTENSION,
  YAS_CLIENT_AUXILIARY_SUBSCRIPTION_DETAILS_EXTENSION,
  YAS_CLIENT_BANDWIDTH_RATES_EXTENSION,
  YAS_CLIENT_DISCONNECT,
  YAS_CLIENT_MAX_ACTIVE_SUBSCRIPTIONS,
  YAS_CLIENT_MAX_PUBLISHED_CLIENTS,
  YAS_CLIENT_LIMIT_MAX_PUBLISHED_CLIENTS,
  YAS_CLIENT_ORIGIN_EDGE,
  YAS_CLIENT_ORIGIN_EXTENSION,
  YAS_CLIENT_ORIGIN_RELAY,
  YAS_CLIENT_ORIGIN_SSH,
  YAS_CLIENT_ORIGIN_UNIX,
  YAS_CLIENT_ORIGIN_WEBRTC,
  YAS_CLIENT_STATE,
  YAS_CLIENT_STATE_ACK,
  YAS_CLIENT_UNWATCH,
  YAS_CLIENT_VERSION,
  YAS_CLIENT_WATCH,
  YAS_FAMILY_CLIENT,
} from "./generated";
import type { YasConnection } from "./session";
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
  detachStateRetainedValue,
  estimateStateRetainedBytes,
  negotiatedStateLimitU32,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  decodeTypedRecord,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_CLIENT_ACTIVE_SUBSCRIPTIONS_EXTENSION,
  YAS_CLIENT_AUXILIARY_SUBSCRIPTION_DETAILS_EXTENSION,
  YAS_CLIENT_BANDWIDTH_RATES_EXTENSION,
  YAS_CLIENT_DISCONNECT,
  YAS_CLIENT_MAX_ACTIVE_SUBSCRIPTIONS,
  YAS_CLIENT_ORIGIN_EDGE,
  YAS_CLIENT_ORIGIN_EXTENSION,
  YAS_CLIENT_ORIGIN_RELAY,
  YAS_CLIENT_ORIGIN_SSH,
  YAS_CLIENT_ORIGIN_UNIX,
  YAS_CLIENT_ORIGIN_WEBRTC,
  YAS_CLIENT_STATE,
  YAS_CLIENT_STATE_ACK,
  YAS_CLIENT_UNWATCH,
  YAS_CLIENT_VERSION,
  YAS_CLIENT_WATCH,
  YAS_FAMILY_CLIENT,
} from "./generated";

export type YasClientOrigin =
  | {
      kind: typeof YAS_CLIENT_ORIGIN_UNIX;
      peerPid: number;
      peerUid: number;
      peerGid: number;
      socketPath: Uint8Array;
    }
  | {
      kind: typeof YAS_CLIENT_ORIGIN_SSH;
      remoteAddress: string;
      username: string;
    }
  | {
      kind: typeof YAS_CLIENT_ORIGIN_EDGE;
      subject: string;
      issuer: string;
    }
  | {
      kind: typeof YAS_CLIENT_ORIGIN_RELAY;
      routeHandle: bigint;
      generation: bigint;
      depth: number;
    }
  | { kind: typeof YAS_CLIENT_ORIGIN_WEBRTC; peerId: string }
  | {
      kind: typeof YAS_CLIENT_ORIGIN_EXTENSION;
      extensionId: bigint;
      definitionRevision: bigint;
      attempt: bigint;
      taskId: number;
      name: string;
    }
  | { kind: number; unknownOptional: true; body: Uint8Array };

export interface YasClientRecord {
  sessionId: Uint8Array;
  clientInstance: Uint8Array;
  connectedServerNs: bigint;
  idleNs: bigint;
  bytesReceived: bigint;
  bytesSent: bigint;
  name: string;
  release: string;
  label: string;
  origin: YasClientOrigin;
  extensions: readonly YasExtension[];
  activeSubscriptions: YasClientActiveSubscriptions | null;
  auxiliarySubscriptionDetails: YasClientAuxiliarySubscriptionDetails | null;
  bandwidthRates: YasClientBandwidthRates | null;
}

export interface YasClientBandwidthRates {
  receivedBytesPerSecond: bigint;
  sentBytesPerSecond: bigint;
  sampleWindowNs: bigint;
}

export interface YasClientTerminalSubscription {
  terminalHandle: bigint;
  viewId: number;
  rows: number;
  columns: number;
}

export interface YasClientSurfaceSubscription {
  surfaceHandle: bigint;
  viewId: number;
  width: number;
  height: number;
  scale120: number;
}

export interface YasClientAuxiliarySubscription {
  family: number;
  subscriptionId: number;
  resourceHandle: bigint;
}

export interface YasClientActiveSubscriptions {
  terminals: readonly YasClientTerminalSubscription[];
  surfaces: readonly YasClientSurfaceSubscription[];
  auxiliary: readonly YasClientAuxiliarySubscription[];
}

export interface YasClientAuxiliarySubscriptionDetail {
  family: number;
  stateWatchFlags: number;
  subscriptionId: number;
  requestFlags: number;
  /** Family-specific resource identity; for KV this is the namespace prefix. */
  resource: Uint8Array;
}

export interface YasClientAuxiliarySubscriptionDetails {
  entries: readonly YasClientAuxiliarySubscriptionDetail[];
}

export interface YasClientSnapshot {
  revision: bigint;
  clients: readonly YasClientRecord[];
}

export function decodeClientOrigin(cursor: YasCursor): YasClientOrigin {
  const record = decodeTypedRecord(cursor);
  const body = new YasCursor(record.body);
  let origin: YasClientOrigin;
  if (record.kind === YAS_CLIENT_ORIGIN_UNIX) {
    origin = {
      kind: record.kind,
      peerPid: body.u32("Client origin peer PID"),
      peerUid: body.u32("Client origin peer UID"),
      peerGid: body.u32("Client origin peer GID"),
      socketPath: new Uint8Array(body.bytesU32("Client origin socket path")),
    };
  } else if (record.kind === YAS_CLIENT_ORIGIN_SSH) {
    origin = {
      kind: record.kind,
      remoteAddress: body.utf8U16("Client SSH remote address"),
      username: body.utf8U16("Client SSH username"),
    };
  } else if (record.kind === YAS_CLIENT_ORIGIN_EDGE) {
    origin = {
      kind: record.kind,
      subject: body.utf8U16("Client edge subject"),
      issuer: body.utf8U16("Client edge issuer"),
    };
  } else if (record.kind === YAS_CLIENT_ORIGIN_RELAY) {
    const routeHandle = body.u64("Client Relay route handle");
    const generation = body.u64("Client Relay generation");
    const depth = body.u16("Client Relay depth");
    if (
      body.u16("Client Relay reserved") !== 0 ||
      routeHandle === 0n ||
      depth === 0
    )
      throw new YasProtocolError("invalid Client Relay origin");
    origin = { kind: record.kind, routeHandle, generation, depth };
  } else if (record.kind === YAS_CLIENT_ORIGIN_WEBRTC) {
    origin = {
      kind: record.kind,
      peerId: body.utf8U16("Client WebRTC peer ID"),
    };
  } else if (record.kind === YAS_CLIENT_ORIGIN_EXTENSION) {
    const extensionId = body.u64("Client Extension ID");
    if (extensionId === 0n)
      throw new YasProtocolError("Client Extension origin ID is zero");
    origin = {
      kind: record.kind,
      extensionId,
      definitionRevision: body.u64("Client Extension definition revision"),
      attempt: body.u64("Client Extension attempt"),
      taskId: body.u32("Client Extension task ID"),
      name: body.utf8U16("Client Extension name"),
    };
  } else {
    if (record.flags & 1)
      throw new YasProtocolError("unknown required Client origin");
    return {
      kind: record.kind,
      unknownOptional: true,
      body: new Uint8Array(record.body),
    };
  }
  body.end("Client origin");
  return origin;
}

export function decodeClientRecord(bytes: Uint8Array): YasClientRecord {
  const cursor = new YasCursor(bytes);
  const sessionId = new Uint8Array(cursor.take(16, "Client session ID"));
  const clientInstance = new Uint8Array(cursor.take(16, "Client instance ID"));
  const extensionsOffset = {
    connectedServerNs: cursor.u64("Client connected server time"),
    idleNs: cursor.u64("Client idle time"),
    bytesReceived: cursor.u64("Client received bytes"),
    bytesSent: cursor.u64("Client sent bytes"),
    name: cursor.utf8U16("Client name"),
    release: cursor.utf8U16("Client release"),
    label: cursor.utf8U16("Client label"),
    origin: decodeClientOrigin(cursor),
  };
  const extensions = decodeExtensions(cursor, undefined, "Client extensions");
  const record: YasClientRecord = {
    sessionId,
    clientInstance,
    ...extensionsOffset,
    extensions,
    activeSubscriptions: decodeClientActiveSubscriptions(extensions),
    auxiliarySubscriptionDetails:
      decodeClientAuxiliarySubscriptionDetails(extensions),
    bandwidthRates: decodeClientBandwidthRates(extensions),
  };
  cursor.end("Client record");
  requireNonzeroId(record.sessionId, "Client session ID");
  requireNonzeroId(record.clientInstance, "Client instance ID");
  if (record.name.length === 0)
    throw new YasProtocolError("Client name is empty");
  return record;
}

export function decodeClientAuxiliarySubscriptionDetails(
  extensions: readonly YasExtension[],
): YasClientAuxiliarySubscriptionDetails | null {
  const extension = extensions.find(
    (candidate) =>
      candidate.tag ===
      YAS_CLIENT_AUXILIARY_SUBSCRIPTION_DETAILS_EXTENSION,
  );
  if (!extension) return null;
  const cursor = new YasCursor(extension.value);
  const count = cursor.u16("Client auxiliary subscription detail count");
  if (cursor.u16("Client auxiliary subscription details reserved") !== 0)
    throw new YasProtocolError(
      "Client auxiliary subscription details reserved is nonzero",
    );
  if (
    count > YAS_CLIENT_MAX_ACTIVE_SUBSCRIPTIONS ||
    count > Math.floor(cursor.remaining / 14)
  )
    throw new YasProtocolError(
      "invalid Client auxiliary subscription detail count",
    );
  const entries: YasClientAuxiliarySubscriptionDetail[] = [];
  let previous: readonly [number, number] | null = null;
  for (let index = 0; index < count; index++) {
    const value: YasClientAuxiliarySubscriptionDetail = {
      family: cursor.u16("Client auxiliary subscription detail family"),
      stateWatchFlags: cursor.u16(
        "Client auxiliary subscription detail State WATCH flags",
      ),
      subscriptionId: cursor.u32(
        "Client auxiliary subscription detail ID",
      ),
      requestFlags: cursor.u32(
        "Client auxiliary subscription detail request flags",
      ),
      resource: new Uint8Array(
        cursor.bytesU16("Client auxiliary subscription detail resource"),
      ),
    };
    if (
      value.subscriptionId === 0 ||
      (previous !== null &&
        compareNumberPair(previous, [value.family, value.subscriptionId]) >= 0)
    )
      throw new YasProtocolError("invalid Client auxiliary subscription detail");
    previous = [value.family, value.subscriptionId];
    entries.push(value);
  }
  cursor.end("Client auxiliary subscription details");
  return { entries };
}

export function decodeClientActiveSubscriptions(
  extensions: readonly YasExtension[],
): YasClientActiveSubscriptions | null {
  const extension = extensions.find(
    (candidate) => candidate.tag === YAS_CLIENT_ACTIVE_SUBSCRIPTIONS_EXTENSION,
  );
  if (!extension) return null;
  const cursor = new YasCursor(extension.value);
  const terminalCount = cursor.u16("Client terminal subscription count");
  const surfaceCount = cursor.u16("Client surface subscription count");
  const auxiliaryCount = cursor.u16("Client auxiliary subscription count");
  if (cursor.u16("Client subscriptions reserved") !== 0)
    throw new YasProtocolError("Client subscriptions reserved is nonzero");
  const total = terminalCount + surfaceCount + auxiliaryCount;
  if (total > YAS_CLIENT_MAX_ACTIVE_SUBSCRIPTIONS)
    throw new YasProtocolError("too many Client active subscriptions");
  if (terminalCount > Math.floor(cursor.remaining / 16))
    throw new YasProtocolError("invalid Client terminal subscription count");
  const terminals: YasClientTerminalSubscription[] = [];
  let previousTerminal: readonly [bigint, number] | null = null;
  for (let index = 0; index < terminalCount; index++) {
    const value: YasClientTerminalSubscription = {
      terminalHandle: cursor.u64("Client terminal subscription handle"),
      viewId: cursor.u32("Client terminal subscription view ID"),
      rows: cursor.u16("Client terminal subscription rows"),
      columns: cursor.u16("Client terminal subscription columns"),
    };
    if (
      value.terminalHandle === 0n ||
      value.viewId === 0 ||
      (value.rows === 0) !== (value.columns === 0) ||
      (previousTerminal !== null &&
        comparePair(previousTerminal, [value.terminalHandle, value.viewId]) >=
          0)
    )
      throw new YasProtocolError("invalid Client terminal subscription");
    previousTerminal = [value.terminalHandle, value.viewId];
    terminals.push(value);
  }
  if (surfaceCount > Math.floor(cursor.remaining / 24))
    throw new YasProtocolError("invalid Client surface subscription count");
  const surfaces: YasClientSurfaceSubscription[] = [];
  let previousSurface: readonly [bigint, number] | null = null;
  for (let index = 0; index < surfaceCount; index++) {
    const value: YasClientSurfaceSubscription = {
      surfaceHandle: cursor.u64("Client surface subscription handle"),
      viewId: cursor.u32("Client surface subscription view ID"),
      width: cursor.u32("Client surface subscription width"),
      height: cursor.u32("Client surface subscription height"),
      scale120: cursor.u16("Client surface subscription scale"),
    };
    if (cursor.u16("Client surface subscription reserved") !== 0)
      throw new YasProtocolError(
        "Client surface subscription reserved is nonzero",
      );
    const absent =
      value.width === 0 && value.height === 0 && value.scale120 === 0;
    const present =
      value.width !== 0 && value.height !== 0 && value.scale120 !== 0;
    if (
      value.surfaceHandle === 0n ||
      value.viewId === 0 ||
      (!absent && !present) ||
      (previousSurface !== null &&
        comparePair(previousSurface, [value.surfaceHandle, value.viewId]) >= 0)
    )
      throw new YasProtocolError("invalid Client surface subscription");
    previousSurface = [value.surfaceHandle, value.viewId];
    surfaces.push(value);
  }
  if (auxiliaryCount > Math.floor(cursor.remaining / 16))
    throw new YasProtocolError("invalid Client auxiliary subscription count");
  const auxiliary: YasClientAuxiliarySubscription[] = [];
  let previousAuxiliary: readonly [number, number, bigint] | null = null;
  for (let index = 0; index < auxiliaryCount; index++) {
    const family = cursor.u16("Client auxiliary subscription family");
    if (cursor.u16("Client auxiliary subscription reserved") !== 0)
      throw new YasProtocolError(
        "Client auxiliary subscription reserved is nonzero",
      );
    const value: YasClientAuxiliarySubscription = {
      family,
      subscriptionId: cursor.u32("Client auxiliary subscription ID"),
      resourceHandle: cursor.u64("Client auxiliary subscription resource"),
    };
    if (
      value.subscriptionId === 0 ||
      (previousAuxiliary !== null &&
        compareTriple(previousAuxiliary, [
          value.family,
          value.subscriptionId,
          value.resourceHandle,
        ]) >= 0)
    )
      throw new YasProtocolError("invalid Client auxiliary subscription");
    previousAuxiliary = [
      value.family,
      value.subscriptionId,
      value.resourceHandle,
    ];
    auxiliary.push(value);
  }
  cursor.end("Client active subscriptions");
  return { terminals, surfaces, auxiliary };
}

export function decodeClientBandwidthRates(
  extensions: readonly YasExtension[],
): YasClientBandwidthRates | null {
  const extension = extensions.find(
    (candidate) => candidate.tag === YAS_CLIENT_BANDWIDTH_RATES_EXTENSION,
  );
  if (!extension) return null;
  const cursor = new YasCursor(extension.value);
  const value = {
    receivedBytesPerSecond: cursor.u64("Client received bytes per second"),
    sentBytesPerSecond: cursor.u64("Client sent bytes per second"),
    sampleWindowNs: cursor.u64("Client bandwidth sample window"),
  };
  cursor.end("Client bandwidth rates");
  if (value.sampleWindowNs === 0n)
    throw new YasProtocolError("Client bandwidth sample window is zero");
  return value;
}

export function encodeClientBandwidthRates(
  value: YasClientBandwidthRates,
): Uint8Array {
  if (value.sampleWindowNs === 0n)
    throw new YasProtocolError("Client bandwidth sample window is zero");
  return new YasWriter()
    .u64(value.receivedBytesPerSecond)
    .u64(value.sentBytesPerSecond)
    .u64(value.sampleWindowNs)
    .finish();
}

export function encodeClientDisconnect(
  sessionId: Uint8Array,
  operationId: Uint8Array,
  reason: string,
): Uint8Array {
  requireNonzeroId(sessionId, "Client DISCONNECT session ID");
  if (operationId.length !== 16)
    throw new YasProtocolError(
      "Client DISCONNECT operation ID is not 16 bytes",
    );
  return new YasWriter()
    .bytes(sessionId)
    .bytes(operationId)
    .utf8U32(reason)
    .finish();
}

export class YasClientCatalog {
  private current = new Map<string, YasClientRecord>();
  private staging: Map<string, YasClientRecord> | null = null;
  private retention: YasStateCatalogueRetention<string>;
  private stagingRetention: YasStateCatalogueRetention<string> | null = null;
  private subscription: YasStateSubscription | null = null;
  private listeners = new Set<(snapshot: YasClientSnapshot) => void>();
  private pendingFirstSnapshots = new Set<(error: unknown) => void>();
  private _revision = 0n;
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private watchEpoch = 0;
  private disposed = false;

  constructor(private readonly connection: YasConnection) {
    this.retention = YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_CLIENT) {
        this.cancelPendingWatch(
          new YasProtocolError("Client catalogue was invalidated"),
        );
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasClientSnapshot {
    return { revision: this._revision, clients: [...this.current.values()] };
  }

  subscribe(listener: (snapshot: YasClientSnapshot) => void): () => void {
    if (this.disposed) throw new Error("Client catalogue is disposed");
    this.listeners.add(listener);
    try {
      listener(this.snapshot);
    } catch {
      this.listeners.delete(listener);
    }
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions = {},
  ): Promise<YasClientSnapshot> {
    if (this.disposed) throw new Error("Client catalogue is disposed");
    if (this._revision !== 0n && this.subscription?.active)
      return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectPending!: (error: unknown) => void;
    const result = new Promise<YasClientSnapshot>((resolve, reject) => {
      rejectPending = (error) => {
        this.pendingFirstSnapshots.delete(rejectPending);
        remove?.();
        reject(error);
      };
      this.pendingFirstSnapshots.add(rejectPending);
      remove = this.subscribe((snapshot) => {
        if (snapshot.revision === 0n) return;
        this.pendingFirstSnapshots.delete(rejectPending);
        remove?.();
        resolve(snapshot);
      });
    });
    try {
      return await Promise.race([
        result,
        this.watch(options).then(() => result),
      ]);
    } finally {
      remove?.();
      this.pendingFirstSnapshots.delete(rejectPending);
    }
  }

  watch(options: YasWatchOptions = {}): Promise<void> {
    if (this.disposed)
      return Promise.reject(new Error("Client catalogue is disposed"));
    if (this.subscription?.active) return Promise.resolve();
    if (this.pendingWatch) return this.pendingWatch;
    this.subscription = null;
    this.resetLocal();
    const epoch = this.watchEpoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      YAS_FAMILY_CLIENT,
      YAS_CLIENT_WATCH,
      YAS_CLIENT_UNWATCH,
      YAS_CLIENT_STATE,
      YAS_CLIENT_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.watchEpoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.watchEpoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Client catalogue watch was cancelled");
      }
      this.subscription = subscription;
    });
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = reject;
    });
    let pending!: Promise<void>;
    pending = Promise.race([watched, cancelled]).finally(() => {
      if (this.pendingWatch !== pending) return;
      this.pendingWatch = null;
      if (this.pendingWatchCancel === cancel) this.pendingWatchCancel = null;
    });
    this.pendingWatch = pending;
    this.pendingWatchCancel = cancel;
    return pending;
  }

  async unwatch(): Promise<void> {
    this.cancelPendingWatch(
      new YasProtocolError("Client catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    if (!this.disposed) this.clearState();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const disposalError = new Error("Client catalogue is disposed");
    this.cancelPendingWatch(disposalError);
    this.removeInvalidation();
    for (const reject of [...this.pendingFirstSnapshots]) reject(disposalError);
    this.pendingFirstSnapshots.clear();
    this.listeners.clear();
    const subscription = this.subscription;
    this.subscription = null;
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.current.clear();
    this.staging = null;
    this.stagingRetention = null;
    void subscription?.unwatch().catch(() => undefined);
  }

  private apply(batch: YasStateBatch): void {
    if (this.disposed) return;
    if (batch.phase === YAS_STATE_RESET) {
      this.clearState();
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.discardStaging();
      this.staging = new Map();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Client snapshot records without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Client snapshot end without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.retention;
      this.current = this.staging;
      this.retention = this.stagingRetention;
      this.staging = null;
      this.stagingRetention = null;
      previousRetention.dispose();
      this._revision = batch.toRevision;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const retention = this.retention.clone();
      let next: Map<string, YasClientRecord>;
      try {
        next = new Map(this.current);
        this.applyRecords(next, retention, batch.records);
        this.validateCatalog(next);
      } catch (error) {
        retention.dispose();
        throw error;
      }
      const previousRetention = this.retention;
      this.current = next;
      this.retention = retention;
      previousRetention.dispose();
      this._revision = batch.toRevision;
      this.emit();
    }
  }

  private applyRecords(
    target: Map<string, YasClientRecord>,
    retention: YasStateCatalogueRetention<string>,
    records: readonly YasTypedRecord[],
  ): void {
    for (const action of records) {
      if (action.kind === YAS_STATE_ADD || action.kind === YAS_STATE_REPLACE) {
        const record = detachStateRetainedValue(
          decodeClientRecord(action.body),
        );
        const key = idKey(record.sessionId);
        const exists = target.has(key);
        if ((action.kind === YAS_STATE_ADD) === exists)
          throw new YasProtocolError("Client ADD/REPLACE precondition failed");
        if (action.kind === YAS_STATE_ADD && target.size >= this.catalogLimit())
          throw new YasProtocolError(
            "Client catalogue exceeds its negotiated client limit",
          );
        retention.upsert(key, estimateStateRetainedBytes(record));
        target.set(key, record);
      } else if (action.kind === YAS_STATE_PATCH) {
        const cursor = new YasCursor(action.body);
        const sessionId = new Uint8Array(
          cursor.take(16, "patched Client session ID"),
        );
        requireNonzeroId(sessionId, "patched Client session ID");
        const extensions = decodeExtensions(
          cursor,
          undefined,
          "Client patch extensions",
        );
        cursor.end("Client patch");
        const key = idKey(sessionId);
        const previous = target.get(key);
        if (!previous)
          throw new YasProtocolError("Client PATCH names an unknown session");
        const mergedExtensions = mergeExtensions(
          previous.extensions,
          extensions,
        );
        const next = detachStateRetainedValue({
          ...previous,
          extensions: mergedExtensions,
          activeSubscriptions:
            decodeClientActiveSubscriptions(mergedExtensions),
          auxiliarySubscriptionDetails:
            decodeClientAuxiliarySubscriptionDetails(mergedExtensions),
          bandwidthRates: decodeClientBandwidthRates(mergedExtensions),
        });
        retention.upsert(key, estimateStateRetainedBytes(next));
        target.set(key, next);
      } else if (action.kind === YAS_STATE_REMOVE) {
        const cursor = new YasCursor(action.body);
        const sessionId = new Uint8Array(
          cursor.take(16, "removed Client session ID"),
        );
        cursor.end("Client removal");
        const key = idKey(sessionId);
        if (!target.has(key))
          throw new YasProtocolError("Client REMOVE names an unknown session");
        retention.remove(key);
        target.delete(key);
      }
    }
  }

  private validateCatalog(records: ReadonlyMap<string, YasClientRecord>): void {
    if (records.size > this.catalogLimit())
      throw new YasProtocolError(
        "Client catalogue exceeds its negotiated client limit",
      );
  }

  private catalogLimit(): number {
    return negotiatedStateLimitU32(
      this.connection,
      YAS_FAMILY_CLIENT,
      YAS_CLIENT_VERSION,
      YAS_CLIENT_LIMIT_MAX_PUBLISHED_CLIENTS,
      YAS_CLIENT_MAX_PUBLISHED_CLIENTS,
    );
  }

  private emit(): void {
    const snapshot = this.snapshot;
    for (const listener of this.listeners) {
      try {
        listener(snapshot);
      } catch {
        // One observer cannot block sibling delivery or wire cleanup.
      }
    }
  }

  private resetLocal(): void {
    if (this.disposed) return;
    this.subscription = null;
    this.clearState();
  }

  private cancelPendingWatch(error: unknown): void {
    this.watchEpoch++;
    const cancel = this.pendingWatchCancel;
    this.pendingWatch = null;
    this.pendingWatchCancel = null;
    cancel?.(error);
    for (const reject of [...this.pendingFirstSnapshots]) reject(error);
    this.pendingFirstSnapshots.clear();
  }

  private clearState(): void {
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.current = new Map();
    this.staging = null;
    this.retention = YasStateCatalogueRetention.forConnection(this.connection);
    this.stagingRetention = null;
    this._revision = 0n;
    this.emit();
  }

  private discardStaging(): void {
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
  }
}

export class YasClientClient {
  readonly catalog: YasClientCatalog;

  constructor(readonly connection: YasConnection) {
    this.catalog = new YasClientCatalog(connection);
  }

  list(options: YasWatchOptions = {}): Promise<YasClientSnapshot> {
    return this.catalog.firstSnapshot(options);
  }

  async disconnect(
    sessionId: Uint8Array,
    operationId: Uint8Array,
    reason: string,
  ): Promise<void> {
    await this.connection.request(
      YAS_FAMILY_CLIENT,
      YAS_CLIENT_DISCONNECT,
      encodeClientDisconnect(sessionId, operationId, reason),
    );
  }

  dispose(): void {
    this.catalog.dispose();
  }
}

function requireNonzeroId(value: Uint8Array, name: string): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError(`${name} is zero or not 16 bytes`);
}

function idKey(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function mergeExtensions(
  previous: readonly YasExtension[],
  patch: readonly YasExtension[],
): YasExtension[] {
  const byTag = new Map(
    previous.map((extension) => [extension.tag, extension]),
  );
  for (const extension of patch) byTag.set(extension.tag, extension);
  return [...byTag.values()].sort((left, right) => left.tag - right.tag);
}

function comparePair(
  left: readonly [bigint, number],
  right: readonly [bigint, number],
): number {
  if (left[0] !== right[0]) return left[0] < right[0] ? -1 : 1;
  return left[1] - right[1];
}

function compareNumberPair(
  left: readonly [number, number],
  right: readonly [number, number],
): number {
  return left[0] !== right[0] ? left[0] - right[0] : left[1] - right[1];
}

function compareTriple(
  left: readonly [number, number, bigint],
  right: readonly [number, number, bigint],
): number {
  if (left[0] !== right[0]) return left[0] - right[0];
  if (left[1] !== right[1]) return left[1] - right[1];
  if (left[2] === right[2]) return 0;
  return left[2] < right[2] ? -1 : 1;
}
