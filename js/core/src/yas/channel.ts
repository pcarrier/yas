import {
  YAS_CHANNEL_ACCEPT,
  YAS_CHANNEL_CHANNEL_CONTENT_KIND,
  YAS_CHANNEL_CLOSE_LISTENER,
  YAS_CHANNEL_CONNECT,
  YAS_CHANNEL_LISTEN,
  YAS_CHANNEL_MAX_MESSAGE_BYTES,
  YAS_CHANNEL_MAX_LISTENERS_PER_SESSION,
  YAS_CHANNEL_MAX_MUTATION_REPLAYS,
  YAS_CHANNEL_MAX_METADATA_BYTES,
  YAS_CHANNEL_MAX_NAME_BYTES,
  YAS_CHANNEL_LIMIT_MAX_LISTENERS_PER_SESSION,
  YAS_CHANNEL_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_CHANNEL_OWNER_EXTENSION,
  YAS_CHANNEL_OWNER_SESSION,
  YAS_CHANNEL_STATE,
  YAS_CHANNEL_STATE_ACK,
  YAS_CHANNEL_UNWATCH,
  YAS_CHANNEL_VERSION,
  YAS_CHANNEL_WATCH,
  YAS_FAMILY_CHANNEL,
  YAS_FAMILY_TRANSFER,
  YAS_TRANSFER_RESET,
} from "./generated";
import type { YasConnection } from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_REMOVE,
  YAS_STATE_REPLACE,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YasStateCatalogueRetention,
  YasStateSubscription,
  estimateStateRetainedBytes,
  negotiatedStateLimitU32,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeTransferDescriptor,
  encodeTransferDescriptor,
  transfersFor,
  type YasTransfer,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YAS_STATUS_CANCELLED,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YasCursor,
  YasDisconnectedError,
  YasProtocolError,
  YasResultError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_CHANNEL_ACCEPT,
  YAS_CHANNEL_CLOSE_LISTENER,
  YAS_CHANNEL_CONNECT,
  YAS_CHANNEL_LISTEN,
  YAS_CHANNEL_OWNER_EXTENSION,
  YAS_CHANNEL_OWNER_SESSION,
  YAS_CHANNEL_STATE,
  YAS_CHANNEL_STATE_ACK,
  YAS_CHANNEL_UNWATCH,
  YAS_CHANNEL_VERSION,
  YAS_CHANNEL_WATCH,
} from "./generated";

export interface YasChannelListenerRecord {
  listenerHandle: bigint;
  generation: bigint;
  ownerKind: number;
  ownerSession: Uint8Array;
  name: string;
  metadata: Uint8Array;
  extensions: readonly YasExtension[];
}

export interface YasChannelEndpoint {
  channelHandle: bigint;
  peerChannelHandle: bigint;
  peerSession: Uint8Array;
  listenerMetadata: Uint8Array;
  connectorMetadata: Uint8Array;
  descriptor: YasTransferDescriptor;
  extensions: readonly YasExtension[];
}

export interface YasChannelConnection extends YasChannelEndpoint {
  transfer: YasTransfer;
}

export interface YasChannelSnapshot {
  revision: bigint;
  listeners: readonly YasChannelListenerRecord[];
}

export interface YasChannelListenOptions {
  metadata?: Uint8Array;
  operationId?: Uint8Array;
  acceptReceiveCredit?: bigint;
  extensions?: readonly YasExtension[];
  onAccept: (channel: YasChannelConnection) => void | Promise<void>;
}

export interface YasChannelConnectOptions {
  metadata?: Uint8Array;
  initialReceiveCredit?: bigint;
  extensions?: readonly YasExtension[];
}

function validateHandle(value: bigint, field: string): void {
  if (value === 0n) throw new YasProtocolError(`${field} is zero`);
}

function validateIdentity(handle: bigint, generation: bigint): void {
  validateHandle(handle, "Channel listener handle");
  if (generation === 0n)
    throw new YasProtocolError("Channel listener generation is zero");
}

function validateSession(value: Uint8Array, field: string): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError(`${field} is invalid`);
}

function validateName(name: string): void {
  const bytes = new TextEncoder().encode(name);
  if (
    bytes.length === 0 ||
    bytes.length > YAS_CHANNEL_MAX_NAME_BYTES ||
    [...name].some((character) => /\p{Cc}/u.test(character))
  )
    throw new YasProtocolError("Channel name is invalid");
}

function validateMetadata(metadata: Uint8Array): void {
  if (metadata.length > YAS_CHANNEL_MAX_METADATA_BYTES)
    throw new YasProtocolError("Channel metadata exceeds its limit");
}

function validateOperationId(operationId: Uint8Array): void {
  if (operationId.length !== 16 || operationId.every((byte) => byte === 0))
    throw new YasProtocolError("Channel operation ID is invalid");
}

function validateDescriptor(descriptor: YasTransferDescriptor): void {
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_MESSAGE ||
    descriptor.direction !==
      (YAS_TRANSFER_RECEIVER_TO_SENDER | YAS_TRANSFER_SENDER_TO_RECEIVER) ||
    descriptor.maxItemBytes === 0n ||
    descriptor.maxItemBytes > BigInt(YAS_CHANNEL_MAX_MESSAGE_BYTES) ||
    descriptor.contentFamily !== YAS_FAMILY_CHANNEL ||
    descriptor.contentKind !== YAS_CHANNEL_CHANNEL_CONTENT_KIND ||
    descriptor.contentVersion !== YAS_CHANNEL_VERSION ||
    !descriptor.sensitiveContent
  )
    throw new YasProtocolError("invalid Channel Transfer descriptor");
}

export function encodeChannelListenerRecord(
  value: YasChannelListenerRecord,
): Uint8Array {
  validateIdentity(value.listenerHandle, value.generation);
  if (
    value.ownerKind !== YAS_CHANNEL_OWNER_SESSION &&
    value.ownerKind !== YAS_CHANNEL_OWNER_EXTENSION
  )
    throw new YasProtocolError("invalid Channel owner kind");
  validateSession(value.ownerSession, "Channel owner session");
  validateName(value.name);
  validateMetadata(value.metadata);
  return new YasWriter()
    .u64(value.listenerHandle)
    .u64(value.generation)
    .u8(value.ownerKind)
    .u8(0)
    .u16(0)
    .bytes(value.ownerSession)
    .utf8U16(value.name)
    .bytesU32(value.metadata)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeChannelListenerRecord(
  payload: Uint8Array,
): YasChannelListenerRecord {
  const cursor = new YasCursor(payload);
  const listenerHandle = cursor.u64("Channel listener handle");
  const generation = cursor.u64("Channel listener generation");
  const ownerKind = cursor.u8("Channel owner kind");
  if (
    cursor.u8("Channel listener reserved") !== 0 ||
    cursor.u16("Channel listener flags") !== 0
  )
    throw new YasProtocolError("Channel listener reserved fields are nonzero");
  const ownerSession = cursor.take(16, "Channel owner session");
  const name = cursor.utf8U16("Channel name");
  const metadata = cursor.bytesU32("Channel listener metadata");
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "Channel listener extensions",
  );
  cursor.end("Channel listener record");
  const value = {
    listenerHandle,
    generation,
    ownerKind,
    ownerSession,
    name,
    metadata,
    extensions,
  };
  encodeChannelListenerRecord(value);
  return value;
}

export function encodeChannelListen(
  name: string,
  operationId: Uint8Array,
  metadata: Uint8Array,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  validateName(name);
  validateOperationId(operationId);
  validateMetadata(metadata);
  return new YasWriter()
    .bytes(operationId)
    .utf8U16(name)
    .bytesU32(metadata)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeChannelListen(payload: Uint8Array): {
  operationId: Uint8Array;
  name: string;
  metadata: Uint8Array;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(payload);
  const operationId = cursor.take(16, "Channel operation ID");
  const name = cursor.utf8U16("Channel name");
  const metadata = cursor.bytesU32("Channel listener metadata");
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "Channel LISTEN extensions",
  );
  cursor.end("Channel LISTEN");
  encodeChannelListen(name, operationId, metadata, extensions);
  return { operationId, name, metadata, extensions };
}

export function encodeChannelIdentity(
  listenerHandle: bigint,
  generation: bigint,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  validateIdentity(listenerHandle, generation);
  return new YasWriter()
    .u64(listenerHandle)
    .u64(generation)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeChannelIdentity(payload: Uint8Array): {
  listenerHandle: bigint;
  generation: bigint;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(payload);
  const listenerHandle = cursor.u64("Channel listener handle");
  const generation = cursor.u64("Channel listener generation");
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "Channel identity extensions",
  );
  cursor.end("Channel listener identity");
  validateIdentity(listenerHandle, generation);
  return { listenerHandle, generation, extensions };
}

export function encodeChannelConnect(
  listenerHandle: bigint,
  generation: bigint,
  initialReceiveCredit: bigint,
  metadata: Uint8Array,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  validateIdentity(listenerHandle, generation);
  if (initialReceiveCredit === 0n)
    throw new YasProtocolError("Channel receive credit is zero");
  validateMetadata(metadata);
  return new YasWriter()
    .u64(listenerHandle)
    .u64(generation)
    .u64(initialReceiveCredit)
    .bytesU32(metadata)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeChannelConnect(payload: Uint8Array): {
  listenerHandle: bigint;
  generation: bigint;
  initialReceiveCredit: bigint;
  metadata: Uint8Array;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(payload);
  const listenerHandle = cursor.u64("Channel listener handle");
  const generation = cursor.u64("Channel listener generation");
  const initialReceiveCredit = cursor.u64("Channel receive credit");
  const metadata = cursor.bytesU32("Channel connector metadata");
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "Channel CONNECT extensions",
  );
  cursor.end("Channel CONNECT");
  encodeChannelConnect(
    listenerHandle,
    generation,
    initialReceiveCredit,
    metadata,
    extensions,
  );
  return {
    listenerHandle,
    generation,
    initialReceiveCredit,
    metadata,
    extensions,
  };
}

export function encodeChannelEndpoint(value: YasChannelEndpoint): Uint8Array {
  validateHandle(value.channelHandle, "Channel handle");
  validateHandle(value.peerChannelHandle, "peer Channel handle");
  validateSession(value.peerSession, "Channel peer session");
  validateMetadata(value.listenerMetadata);
  validateMetadata(value.connectorMetadata);
  validateDescriptor(value.descriptor);
  return new YasWriter()
    .u64(value.channelHandle)
    .u64(value.peerChannelHandle)
    .bytes(value.peerSession)
    .bytesU32(value.listenerMetadata)
    .bytesU32(value.connectorMetadata)
    .bytesU32(encodeTransferDescriptor(value.descriptor))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeChannelEndpoint(payload: Uint8Array): YasChannelEndpoint {
  const cursor = new YasCursor(payload);
  const channelHandle = cursor.u64("Channel handle");
  const peerChannelHandle = cursor.u64("peer Channel handle");
  const peerSession = cursor.take(16, "Channel peer session");
  const listenerMetadata = cursor.bytesU32("Channel listener metadata");
  const connectorMetadata = cursor.bytesU32("Channel connector metadata");
  const descriptorBytes = cursor.bytesU32("Channel Transfer descriptor");
  const descriptorCursor = new YasCursor(descriptorBytes);
  const descriptor = decodeTransferDescriptor(descriptorCursor);
  descriptorCursor.end("Channel Transfer descriptor");
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "Channel endpoint extensions",
  );
  cursor.end("Channel endpoint");
  const value = {
    channelHandle,
    peerChannelHandle,
    peerSession,
    listenerMetadata,
    connectorMetadata,
    descriptor,
    extensions,
  };
  encodeChannelEndpoint(value);
  return value;
}

export function encodeChannelAccept(
  listenerHandle: bigint,
  generation: bigint,
  endpoint: YasChannelEndpoint,
): Uint8Array {
  validateIdentity(listenerHandle, generation);
  if (endpoint.descriptor.senderSendCredit !== 0n)
    throw new YasProtocolError("Channel ACCEPT has initial peer send credit");
  return new YasWriter()
    .u64(listenerHandle)
    .u64(generation)
    .bytes(encodeChannelEndpoint(endpoint))
    .finish();
}

export function decodeChannelAccept(payload: Uint8Array): {
  listenerHandle: bigint;
  generation: bigint;
  endpoint: YasChannelEndpoint;
} {
  const cursor = new YasCursor(payload);
  const listenerHandle = cursor.u64("Channel listener handle");
  const generation = cursor.u64("Channel listener generation");
  const endpoint = decodeChannelEndpoint(
    cursor.take(cursor.remaining, "Channel endpoint"),
  );
  cursor.end("Channel ACCEPT");
  encodeChannelAccept(listenerHandle, generation, endpoint);
  return { listenerHandle, generation, endpoint };
}

/** Materializes the watched server-wide listener name registry. */
export class YasChannelCatalogue {
  private current = new Map<bigint, YasChannelListenerRecord>();
  private currentRetention: YasStateCatalogueRetention<bigint>;
  private staging: Map<bigint, YasChannelListenerRecord> | null = null;
  private stagingRetention: YasStateCatalogueRetention<bigint> | null = null;
  private subscription: YasStateSubscription | null = null;
  private listeners = new Set<(snapshot: YasChannelSnapshot) => void>();
  private readonly snapshotRejectors = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private epoch = 0;
  private disposed = false;
  private revision = 0n;

  constructor(
    private readonly connection: YasConnection,
    private readonly maxListeners: () => number = () =>
      YAS_CHANNEL_MAX_LISTENERS_PER_SESSION,
  ) {
    this.currentRetention =
      YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_CHANNEL) {
        this.epoch++;
        this.subscription = null;
        const error = new YasProtocolError("Channel catalogue was invalidated");
        this.pendingWatchCancel?.(error);
        this.cancelSnapshots(error);
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasChannelSnapshot {
    return {
      revision: this.revision,
      listeners: [...this.current.values()].sort((left, right) =>
        left.name.localeCompare(right.name),
      ),
    };
  }

  subscribe(listener: (snapshot: YasChannelSnapshot) => void): () => void {
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
    signal?: AbortSignal,
  ): Promise<YasChannelSnapshot> {
    this.assertOpen();
    if (signal?.aborted)
      throw new YasProtocolError("Channel snapshot wait was cancelled");
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let removeAbort: (() => void) | undefined;
    let rejectSnapshot: ((error: unknown) => void) | undefined;
    const result = new Promise<YasChannelSnapshot>((resolve, reject) => {
      let settled = false;
      const finish = (snapshot?: YasChannelSnapshot, error?: unknown) => {
        if (settled) return;
        settled = true;
        remove?.();
        removeAbort?.();
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
    if (signal) {
      const abort = () =>
        rejectSnapshot?.(
          new YasProtocolError("Channel snapshot wait was cancelled"),
        );
      signal.addEventListener("abort", abort, { once: true });
      removeAbort = () => signal.removeEventListener("abort", abort);
    }
    try {
      return await Promise.race([
        result,
        this.watch(options).then(() => result),
      ]);
    } catch (error) {
      remove?.();
      removeAbort?.();
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
      YAS_FAMILY_CHANNEL,
      YAS_CHANNEL_WATCH,
      YAS_CHANNEL_UNWATCH,
      YAS_CHANNEL_STATE,
      YAS_CHANNEL_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.epoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.epoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Channel catalogue watch was cancelled");
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
      new YasProtocolError("Channel catalogue watch was cancelled"),
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
    const error = new YasProtocolError("Channel catalogue is disposed");
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
      this.current = new Map();
      this.currentRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      this.staging = null;
      this.stagingRetention = null;
      this.revision = 0n;
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
        throw new YasProtocolError("Channel snapshot records without begin");
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
        throw new YasProtocolError("Channel snapshot end without begin");
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
      this.revision = batch.toRevision;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const nextRetention = this.currentRetention.clone();
      let next: Map<bigint, YasChannelListenerRecord>;
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
      this.revision = batch.toRevision;
      this.emit();
    }
  }

  private applyRecords(
    target: Map<bigint, YasChannelListenerRecord>,
    retention: YasStateCatalogueRetention<bigint>,
    records: readonly YasTypedRecord[],
  ): void {
    const originals = new Map<bigint, YasChannelListenerRecord | null>();
    const remember = (key: bigint) => {
      if (!originals.has(key)) originals.set(key, target.get(key) ?? null);
    };
    try {
      for (const record of records) {
        if (
          record.kind === YAS_STATE_ADD ||
          record.kind === YAS_STATE_REPLACE
        ) {
          const decoded = decodeChannelListenerRecord(record.body);
          const encoded = encodeChannelListenerRecord(decoded);
          const listener = decodeChannelListenerRecord(encoded);
          const exists = target.has(listener.listenerHandle);
          if ((record.kind === YAS_STATE_ADD) === exists)
            throw new YasProtocolError(
              "Channel listener ADD/REPLACE precondition failed",
            );
          remember(listener.listenerHandle);
          retention.upsert(
            listener.listenerHandle,
            Math.max(encoded.length, estimateStateRetainedBytes(listener)),
          );
          target.set(listener.listenerHandle, listener);
        } else if (record.kind === YAS_STATE_REMOVE) {
          const cursor = new YasCursor(record.body);
          const handle = cursor.u64("removed Channel listener handle");
          const generation = cursor.u64("removed Channel listener generation");
          cursor.end("Channel listener REMOVE");
          const current = target.get(handle);
          if (!current || current.generation !== generation)
            throw new YasProtocolError("Channel listener REMOVE is stale");
          remember(handle);
          retention.remove(handle);
          target.delete(handle);
        } else {
          throw new YasProtocolError("unsupported Channel state record kind");
        }
      }
      this.validateCatalogue(target);
    } catch (error) {
      for (const key of originals.keys()) retention.remove(key);
      for (const [key, original] of originals) {
        if (original) {
          retention.upsert(
            key,
            Math.max(
              encodeChannelListenerRecord(original).length,
              estimateStateRetainedBytes(original),
            ),
          );
          target.set(key, original);
        } else target.delete(key);
      }
      throw error;
    }
  }

  private validateCatalogue(
    listeners: ReadonlyMap<bigint, YasChannelListenerRecord>,
  ): void {
    if (listeners.size > this.maxListeners())
      throw new YasProtocolError(
        "Channel catalogue exceeds negotiated listener limit",
      );
    const names = new Set<string>();
    for (const listener of listeners.values()) {
      if (names.has(listener.name))
        throw new YasProtocolError("duplicate Channel listener name");
      names.add(listener.name);
    }
  }

  private resetLocal(): void {
    this.currentRetention.dispose();
    this.stagingRetention?.dispose();
    this.current = new Map();
    this.currentRetention = YasStateCatalogueRetention.forConnection(
      this.connection,
    );
    this.staging = null;
    this.stagingRetention = null;
    this.revision = 0n;
    this.emit();
  }

  private discardStaging(): void {
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
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

  private cancelSnapshots(error: unknown): void {
    for (const reject of [...this.snapshotRejectors]) reject(error);
    this.snapshotRejectors.clear();
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("Channel catalogue is disposed");
  }
}

interface ActiveListener {
  value: YasChannelListener;
  onAccept: YasChannelListenOptions["onAccept"];
  acceptReceiveCredit: bigint;
  operationKey: string;
}

interface YasChannelListenOperation {
  payloadKey: string;
  pending: Promise<YasChannelListener> | null;
  listener: YasChannelListener | null;
  retainPayload: boolean;
}

export class YasChannelListener {
  private closed = false;

  constructor(
    private readonly client: YasChannelClient,
    readonly listenerHandle: bigint,
    readonly generation: bigint,
    readonly name: string,
  ) {}

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.client.forgetListener(this.listenerHandle, this.generation);
    await this.client.connection.requestDecoded(
      YAS_FAMILY_CHANNEL,
      YAS_CHANNEL_CLOSE_LISTENER,
      encodeChannelIdentity(this.listenerHandle, this.generation),
      (body) => {
        if (body.length !== 0)
          throw new YasProtocolError(
            "Channel CLOSE_LISTENER Result has a body",
          );
      },
    );
  }

  /** Mark a listener closed when its owning client loses the session. */
  invalidate(): void {
    this.closed = true;
  }
}

/** Native Channel client: catalogue, exclusive listeners, and MESSAGE links. */
export class YasChannelClient {
  readonly catalogue: YasChannelCatalogue;
  private readonly transfers;
  private readonly activeListeners = new Map<bigint, ActiveListener>();
  private readonly listenOperations = new Map<
    string,
    YasChannelListenOperation
  >();
  private readonly pendingListenOperations = new Map<
    string,
    YasChannelListenOperation
  >();
  private readonly activeChannels = new Set<YasTransfer>();
  private readonly pendingCancels = new Set<(error: unknown) => void>();
  private readonly removeAcceptListener: () => void;
  private readonly removeInvalidation: () => void;
  private epoch = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    this.catalogue = new YasChannelCatalogue(connection, () =>
      negotiatedStateLimitU32(
        connection,
        YAS_FAMILY_CHANNEL,
        YAS_CHANNEL_VERSION,
        YAS_CHANNEL_LIMIT_MAX_LISTENERS_PER_SESSION,
        YAS_CHANNEL_MAX_LISTENERS_PER_SESSION,
      ),
    );
    this.transfers = transfersFor(connection);
    this.removeAcceptListener = connection.onEvent(
      YAS_FAMILY_CHANNEL,
      YAS_CHANNEL_ACCEPT,
      ({ payload }) => this.handleAccept(payload),
    );
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_CHANNEL) {
        this.epoch++;
        for (const listener of this.activeListeners.values()) {
          listener.value.invalidate();
          this.tombstoneListener(listener.operationKey, listener.value);
        }
        this.activeListeners.clear();
        this.retirePendingListenOperations();
        this.cancelPending(
          new YasDisconnectedError("YAS Channel client was invalidated"),
        );
        this.resetChannels();
      }
    });
  }

  listen(
    name: string,
    options: YasChannelListenOptions,
  ): Promise<YasChannelListener> {
    this.assertOpen();
    const operationId = options.operationId ?? randomOperationId();
    const payload = encodeChannelListen(
      name,
      operationId,
      options.metadata ?? new Uint8Array(0),
      options.extensions,
    );
    const operationKey = byteKey(operationId);
    const payloadKey = byteKey(payload);
    let operation =
      this.listenOperations.get(operationKey) ??
      this.pendingListenOperations.get(operationKey);
    if (operation) {
      if (operation.payloadKey !== payloadKey)
        throw new YasProtocolError(
          "Channel LISTEN operation ID was reused with a different payload",
        );
      if (operation.pending) return operation.pending;
      if (operation.listener) {
        const active = this.activeListeners.get(
          operation.listener.listenerHandle,
        );
        if (active?.value === operation.listener)
          return Promise.resolve(operation.listener);
        operation.listener = null;
      }
    } else {
      this.ensureListenReplaySlot(operationKey);
      operation = {
        payloadKey,
        pending: null,
        listener: null,
        retainPayload: false,
      };
      this.pendingListenOperations.set(operationKey, operation);
    }
    if (operation.retainPayload) this.ensureListenReplaySlot(operationKey);
    const epoch = this.epoch;
    let request: Promise<YasChannelListener>;
    try {
      request = this.connection.requestDecoded(
        YAS_FAMILY_CHANNEL,
        YAS_CHANNEL_LISTEN,
        payload,
        (body) =>
          this.installListener(
            decodeChannelIdentity(body),
            name,
            options,
            operationKey,
            operation,
            epoch,
          ),
      );
    } catch (error) {
      if (!operation.listener && !operation.retainPayload)
        this.pendingListenOperations.delete(operationKey);
      throw error;
    }
    let pending!: Promise<YasChannelListener>;
    pending = this.runOwned(request)
      .then((result) => {
        // Test and embedding transports may provide an already-decoded Result.
        const listener =
          result instanceof YasChannelListener
            ? result
            : this.installListener(
                result as ReturnType<typeof decodeChannelIdentity>,
                name,
                options,
                operationKey,
                operation,
                epoch,
              );
        if (this.disposed || epoch !== this.epoch)
          throw new YasProtocolError(
            "Channel LISTEN completed after disposal or family invalidation",
          );
        return listener;
      })
      .finally(() => {
        if (operation.pending !== pending) return;
        operation.pending = null;
        if (
          !operation.listener &&
          !operation.retainPayload &&
          this.pendingListenOperations.get(operationKey) === operation
        )
          this.pendingListenOperations.delete(operationKey);
      });
    operation.pending = pending;
    return pending;
  }

  async connect(
    listener: Pick<YasChannelListenerRecord, "listenerHandle" | "generation">,
    options: YasChannelConnectOptions = {},
  ): Promise<YasChannelConnection> {
    this.assertOpen();
    const epoch = this.epoch;
    const preferred = options.initialReceiveCredit ?? 1024n * 1024n;
    const lease = this.connection.receiveBudget.reserve(preferred, 1n);
    let accepted = false;
    try {
      const connected = await this.connection.requestDecoded(
        YAS_FAMILY_CHANNEL,
        YAS_CHANNEL_CONNECT,
        encodeChannelConnect(
          listener.listenerHandle,
          listener.generation,
          lease.bytes,
          options.metadata ?? new Uint8Array(0),
          options.extensions,
        ),
        (body) => {
          const endpoint = decodeChannelEndpoint(body);
          const transfer = this.transfers.acceptServerDescriptor(
            endpoint.descriptor,
            lease,
          );
          accepted = true;
          return { ...endpoint, transfer };
        },
      );
      if (this.disposed || epoch !== this.epoch) {
        connected.transfer.reset(
          YAS_STATUS_CANCELLED,
          new TextEncoder().encode("Channel client closed during CONNECT"),
        );
        throw new YasProtocolError("Channel CONNECT completed after disposal");
      }
      this.trackChannel(connected.transfer);
      return connected;
    } catch (error) {
      if (!accepted) lease.release();
      throw error;
    }
  }

  forgetListener(listenerHandle: bigint, generation: bigint): void {
    const current = this.activeListeners.get(listenerHandle);
    if (current?.value.generation === generation) {
      this.activeListeners.delete(listenerHandle);
      this.tombstoneListener(current.operationKey, current.value);
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    this.removeAcceptListener();
    this.removeInvalidation();
    for (const listener of this.activeListeners.values()) {
      listener.value.invalidate();
      this.tombstoneListener(listener.operationKey, listener.value);
      void this.closeListenerRemote(
        listener.value.listenerHandle,
        listener.value.generation,
      ).catch(() => undefined);
    }
    this.activeListeners.clear();
    this.retirePendingListenOperations();
    this.cancelPending(
      new YasDisconnectedError("YAS Channel client was disposed"),
    );
    this.listenOperations.clear();
    this.pendingListenOperations.clear();
    this.resetChannels();
    this.catalogue.dispose();
  }

  private installListener(
    result: ReturnType<typeof decodeChannelIdentity>,
    name: string,
    options: YasChannelListenOptions,
    operationKey: string,
    operation: YasChannelListenOperation,
    epoch: number,
  ): YasChannelListener {
    if (this.disposed || epoch !== this.epoch) {
      void this.closeListenerIfUnowned(
        result.listenerHandle,
        result.generation,
      ).catch(() => undefined);
      throw new YasProtocolError(
        "Channel LISTEN completed after disposal or family invalidation",
      );
    }
    const current = this.activeListeners.get(result.listenerHandle);
    if (current) {
      throw new YasProtocolError("Channel listener handle was reused");
    }
    if (operation.retainPayload && !operation.listener) {
      void this.closeListenerIfUnowned(
        result.listenerHandle,
        result.generation,
      ).catch(() => undefined);
      throw new YasProtocolError(
        "Channel LISTEN replayed a retired listener instead of STALE",
      );
    }
    const listener = new YasChannelListener(
      this,
      result.listenerHandle,
      result.generation,
      name,
    );
    this.activeListeners.set(result.listenerHandle, {
      value: listener,
      onAccept: options.onAccept,
      acceptReceiveCredit: options.acceptReceiveCredit ?? 1024n * 1024n,
      operationKey,
    });
    operation.listener = listener;
    operation.retainPayload = true;
    if (!this.retainListenOperation(operationKey, operation)) {
      this.activeListeners.delete(result.listenerHandle);
      operation.listener = null;
      void this.closeListenerIfUnowned(
        result.listenerHandle,
        result.generation,
      ).catch(() => undefined);
      throw new YasProtocolError("Channel LISTEN replay ledger overflowed");
    }
    return listener;
  }

  private tombstoneListener(
    operationKey: string,
    listener: YasChannelListener,
  ): void {
    const operation = this.listenOperations.get(operationKey);
    if (operation?.listener !== listener) return;
    operation.listener = null;
    operation.retainPayload = true;
  }

  private retirePendingListenOperations(): void {
    for (const operation of this.listenOperations.values()) {
      if (!operation.pending) continue;
      operation.pending = null;
      operation.listener = null;
      operation.retainPayload = true;
    }
    for (const [operationKey, operation] of this.pendingListenOperations) {
      operation.pending = null;
      operation.listener = null;
      operation.retainPayload = true;
      this.retainListenOperation(operationKey, operation);
    }
    this.pendingListenOperations.clear();
  }

  private closeListenerRemote(
    listenerHandle: bigint,
    generation: bigint,
  ): Promise<void> {
    return this.connection.requestDecoded(
      YAS_FAMILY_CHANNEL,
      YAS_CHANNEL_CLOSE_LISTENER,
      encodeChannelIdentity(listenerHandle, generation),
      (body) => {
        if (body.length !== 0)
          throw new YasProtocolError(
            "Channel CLOSE_LISTENER Result has a body",
          );
      },
    );
  }

  private closeListenerIfUnowned(
    listenerHandle: bigint,
    generation: bigint,
  ): Promise<void> {
    if (this.activeListeners.has(listenerHandle)) return Promise.resolve();
    return this.closeListenerRemote(listenerHandle, generation);
  }

  private ensureListenReplaySlot(operationKey: string): void {
    let pinned = 0;
    for (const [key, operation] of this.listenOperations) {
      if (key === operationKey) continue;
      if (operation.pending || operation.listener) pinned++;
    }
    for (const key of this.pendingListenOperations.keys())
      if (key !== operationKey) pinned++;
    if (pinned + 1 > this.listenReplayLimit())
      throw new YasResultError(
        YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(0),
        "Channel LISTEN replay ledger is full",
      );
  }

  private retainListenOperation(
    operationKey: string,
    operation: YasChannelListenOperation,
  ): boolean {
    if (this.listenOperations.get(operationKey) === operation) return true;
    const limit = this.listenReplayLimit();
    let needed = this.listenOperations.size - limit + 1;
    for (const [key, operation] of this.listenOperations) {
      if (needed <= 0) break;
      if (
        !operation.pending &&
        !operation.listener &&
        operation.retainPayload
      ) {
        this.listenOperations.delete(key);
        needed--;
      }
    }
    if (needed > 0) return false;
    this.pendingListenOperations.delete(operationKey);
    this.listenOperations.set(operationKey, operation);
    return true;
  }

  private listenReplayLimit(): number {
    const extension = this.connection
      .family(YAS_FAMILY_CHANNEL, YAS_CHANNEL_VERSION)
      .limits.find(
        (candidate) => candidate.tag === YAS_CHANNEL_LIMIT_MAX_MUTATION_REPLAYS,
      );
    if (!extension)
      throw new YasProtocolError(
        "required Channel mutation replay limit is absent",
      );
    const cursor = new YasCursor(extension.value);
    const value = cursor.u32("Channel mutation replay limit");
    cursor.end("Channel mutation replay limit");
    if (value === 0 || value > YAS_CHANNEL_MAX_MUTATION_REPLAYS)
      throw new YasProtocolError("invalid Channel mutation replay limit");
    return value;
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

  private cancelPending(error: unknown): void {
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("Channel client is disposed");
  }

  private handleAccept(payload: Uint8Array): void {
    const accepted = decodeChannelAccept(payload);
    const listener = this.activeListeners.get(accepted.listenerHandle);
    if (!listener || listener.value.generation !== accepted.generation) {
      this.rejectDescriptor(
        accepted.endpoint.descriptor,
        YAS_STATUS_CANCELLED,
        "Channel listener is no longer active",
      );
      return;
    }
    let lease;
    try {
      lease = this.connection.receiveBudget.reserve(
        listener.acceptReceiveCredit,
        1n,
      );
    } catch {
      this.rejectDescriptor(
        accepted.endpoint.descriptor,
        YAS_STATUS_RESOURCE_EXHAUSTED,
        "Channel receive budget exhausted",
      );
      return;
    }
    let transfer: YasTransfer;
    try {
      transfer = this.transfers.acceptServerDescriptor(
        accepted.endpoint.descriptor,
        lease,
      );
    } catch (error) {
      lease.release();
      throw error;
    }
    const channel = { ...accepted.endpoint, transfer };
    this.trackChannel(transfer);
    try {
      const handled = listener.onAccept(channel);
      if (handled instanceof Promise)
        void handled.catch(() =>
          transfer.reset(
            YAS_STATUS_CANCELLED,
            new TextEncoder().encode("Channel accept handler failed"),
          ),
        );
    } catch {
      transfer.reset(
        YAS_STATUS_CANCELLED,
        new TextEncoder().encode("Channel accept handler failed"),
      );
    }
  }

  private rejectDescriptor(
    descriptor: YasTransferDescriptor,
    status: number,
    detail: string,
  ): void {
    validateDescriptor(descriptor);
    this.connection.sendEvent(
      YAS_FAMILY_TRANSFER,
      YAS_TRANSFER_RESET,
      new YasWriter()
        .u32(descriptor.transferId)
        .u16(status)
        .u16(0)
        .bytesU32(new TextEncoder().encode(detail))
        .finish(),
    );
  }

  private trackChannel(transfer: YasTransfer): void {
    this.activeChannels.add(transfer);
    void transfer.closed.then(
      () => this.activeChannels.delete(transfer),
      () => this.activeChannels.delete(transfer),
    );
  }

  private resetChannels(): void {
    for (const transfer of this.activeChannels) {
      try {
        transfer.reset(YAS_STATUS_CANCELLED);
      } catch {
        // The shared Transfer registry may already be invalidated.
      }
    }
    this.activeChannels.clear();
  }
}

function randomOperationId(): Uint8Array {
  const value = new Uint8Array(16);
  globalThis.crypto.getRandomValues(value);
  value[6] = (value[6]! & 0x0f) | 0x40;
  value[8] = (value[8]! & 0x3f) | 0x80;
  return value;
}

function byteKey(value: Uint8Array): string {
  let key = "";
  for (const byte of value) key += byte.toString(16).padStart(2, "0");
  return key;
}
