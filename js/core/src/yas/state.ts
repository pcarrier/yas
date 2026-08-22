import type { YasConnection, YasReceiveBudgetLease } from "./session";
import type { ConnectionStatus } from "../types";
import {
  YAS_STATE_MODE_REPLAY,
  YAS_STATE_MODE_SNAPSHOT,
  YAS_STATE_PHASE_DELTA,
  YAS_STATE_PHASE_RESET,
  YAS_STATE_PHASE_SNAPSHOT_BEGIN,
  YAS_STATE_PHASE_SNAPSHOT_END,
  YAS_STATE_PHASE_SNAPSHOT_RECORDS,
  YAS_STATE_RECORD_ADD,
  YAS_STATE_RECORD_PATCH,
  YAS_STATE_RECORD_REMOVE,
  YAS_STATE_RECORD_REPLACE,
  YAS_STATE_WATCH_RESUME,
} from "./generated";
import {
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  decodeTypedRecord,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export const YAS_STATE_ADD = YAS_STATE_RECORD_ADD;
export const YAS_STATE_REPLACE = YAS_STATE_RECORD_REPLACE;
export const YAS_STATE_PATCH = YAS_STATE_RECORD_PATCH;
export const YAS_STATE_REMOVE = YAS_STATE_RECORD_REMOVE;
export const YAS_STATE_SNAPSHOT_BEGIN = YAS_STATE_PHASE_SNAPSHOT_BEGIN;
export const YAS_STATE_SNAPSHOT_RECORDS = YAS_STATE_PHASE_SNAPSHOT_RECORDS;
export const YAS_STATE_SNAPSHOT_END = YAS_STATE_PHASE_SNAPSHOT_END;
export const YAS_STATE_DELTA = YAS_STATE_PHASE_DELTA;
export const YAS_STATE_RESET = YAS_STATE_PHASE_RESET;
export const YAS_WATCH_RESUME = YAS_STATE_WATCH_RESUME;
export const YAS_WATCH_MODE_SNAPSHOT = YAS_STATE_MODE_SNAPSHOT;
export const YAS_WATCH_MODE_REPLAY = YAS_STATE_MODE_REPLAY;

/** Read one required negotiated u32 family limit for catalogue admission. */
export function negotiatedStateLimitU32(
  connection: YasConnection,
  family: number,
  version: number,
  tag: number,
  hardMaximum: number,
): number {
  const extension = connection
    .family(family, version)
    .limits.find((candidate) => candidate.tag === tag);
  if (!extension)
    throw new YasProtocolError("required STATE catalogue limit is absent");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u32("STATE catalogue limit");
  cursor.end("STATE catalogue limit");
  if (value === 0 || value > hardMaximum)
    throw new YasProtocolError("invalid STATE catalogue limit");
  return value;
}

const STATE_CATALOGUE_MIN_RETAINED_BYTES = 256 * 1024 * 1024;
const STATE_CATALOGUE_ENTRY_OVERHEAD = 64;

class YasStateCatalogueRetentionPool {
  private retainedBytes = 0;
  private retainedItems = 0;
  private readonly maximumItems: number;

  constructor(readonly maximumBytes: number) {
    this.maximumItems = Math.max(1, Math.floor(maximumBytes / 128));
  }

  reserve(bytes: number, items = 0): void {
    if (
      !Number.isSafeInteger(bytes) ||
      bytes < 0 ||
      !Number.isSafeInteger(items) ||
      items < 0
    )
      throw new YasProtocolError("invalid STATE catalogue reservation");
    if (this.retainedBytes + bytes > this.maximumBytes)
      throw new YasProtocolError(
        "STATE catalogues exceed the connection retained byte limit",
      );
    if (this.retainedItems + items > this.maximumItems)
      throw new YasProtocolError(
        "STATE catalogues exceed the connection retained item limit",
      );
    this.retainedBytes += bytes;
    this.retainedItems += items;
  }

  release(bytes: number, items = 0): void {
    if (
      !Number.isSafeInteger(bytes) ||
      bytes < 0 ||
      bytes > this.retainedBytes ||
      !Number.isSafeInteger(items) ||
      items < 0 ||
      items > this.retainedItems
    )
      throw new Error("invalid STATE catalogue release");
    this.retainedBytes -= bytes;
    this.retainedItems -= items;
  }
}

const stateCatalogueRetentionPools = new WeakMap<
  YasConnection,
  YasStateCatalogueRetentionPool
>();

function stateCatalogueRetentionPool(
  connection: YasConnection,
): YasStateCatalogueRetentionPool {
  const existing = stateCatalogueRetentionPools.get(connection);
  if (existing) return existing;
  const configured = connection.options?.receiveMaxBuffered ?? 0n;
  const minimum = BigInt(STATE_CATALOGUE_MIN_RETAINED_BYTES);
  const selected = configured > minimum ? configured : minimum;
  const pool = new YasStateCatalogueRetentionPool(
    Number(
      selected > BigInt(Number.MAX_SAFE_INTEGER)
        ? BigInt(Number.MAX_SAFE_INTEGER)
        : selected,
    ),
  );
  stateCatalogueRetentionPools.set(connection, pool);
  return pool;
}

/** Conservative, cycle-safe decoded-object estimate for catalogue admission. */
export function estimateStateRetainedBytes(value: unknown): number {
  const seen = new Set<object>();
  const visit = (item: unknown): number => {
    if (item === null || item === undefined) return 0;
    if (typeof item === "string") return 16 + item.length * 2;
    if (typeof item === "bigint" || typeof item === "number") return 8;
    if (typeof item === "boolean") return 4;
    if (typeof item !== "object") return 0;
    if (item instanceof Uint8Array) return 32 + item.byteLength;
    if (seen.has(item)) return 0;
    seen.add(item);
    if (Array.isArray(item))
      return 32 + item.reduce((total, entry) => total + visit(entry), 0);
    let total = 64;
    for (const [key, entry] of Object.entries(item))
      total += 16 + key.length * 2 + visit(entry);
    return total;
  };
  return visit(value);
}

/** Copy peer-decoded views so retained records cannot pin a larger frame. */
export function detachStateRetainedValue<T>(value: T): T {
  const seen = new Map<object, unknown>();
  const clone = (item: unknown): unknown => {
    if (item === null || typeof item !== "object") return item;
    if (item instanceof Uint8Array) return new Uint8Array(item);
    const previous = seen.get(item);
    if (previous !== undefined) return previous;
    if (Array.isArray(item)) {
      const output: unknown[] = [];
      seen.set(item, output);
      for (const entry of item) output.push(clone(entry));
      return output;
    }
    const output: Record<string, unknown> = {};
    seen.set(item, output);
    for (const [key, entry] of Object.entries(item)) output[key] = clone(entry);
    return output;
  };
  return clone(value) as T;
}

/**
 * Incremental admission for decoded State catalogue generations. All ledgers
 * for one connection share a fixed pool, including concurrent watches and the
 * current/staging generations needed for atomic catalogue replacement. The
 * pool is intentionally separate from the rolling wire-credit window.
 */
export class YasStateCatalogueRetention<Key> {
  private readonly sizes = new Map<Key, number>();
  private _bytes = 0;
  private active = true;
  private readonly pool: YasStateCatalogueRetentionPool;

  constructor(
    maximumBytes: number,
    pool = new YasStateCatalogueRetentionPool(maximumBytes),
  ) {
    this.pool = pool;
  }

  static forConnection<Key>(
    connection: YasConnection,
  ): YasStateCatalogueRetention<Key> {
    return new YasStateCatalogueRetention<Key>(
      0,
      stateCatalogueRetentionPool(connection),
    );
  }

  get bytes(): number {
    return this._bytes;
  }

  clone(): YasStateCatalogueRetention<Key> {
    this.assertActive();
    this.pool.reserve(this._bytes, this.sizes.size);
    try {
      const copy = new YasStateCatalogueRetention<Key>(0, this.pool);
      copy._bytes = this._bytes;
      for (const [key, size] of this.sizes) copy.sizes.set(key, size);
      return copy;
    } catch (error) {
      this.pool.release(this._bytes, this.sizes.size);
      throw error;
    }
  }

  upsert(key: Key, encodedBytes: number): void {
    this.assertActive();
    if (!Number.isSafeInteger(encodedBytes) || encodedBytes < 0)
      throw new YasProtocolError("invalid STATE catalogue retained size");
    const size = encodedBytes + STATE_CATALOGUE_ENTRY_OVERHEAD;
    const previousEntry = this.sizes.get(key);
    const previous = previousEntry ?? 0;
    const next = this._bytes - previous + size;
    const increase = next - this._bytes;
    const newItem = previousEntry === undefined ? 1 : 0;
    this.pool.reserve(Math.max(0, increase), newItem);
    try {
      this.sizes.set(key, size);
    } catch (error) {
      this.pool.release(Math.max(0, increase), newItem);
      throw error;
    }
    this._bytes = next;
    if (increase < 0) this.pool.release(-increase);
  }

  remove(key: Key): void {
    this.assertActive();
    const previous = this.sizes.get(key);
    if (previous === undefined) return;
    this.sizes.delete(key);
    this._bytes -= previous;
    this.pool.release(previous, 1);
  }

  move(from: Key, to: Key, encodedBytes: number): void {
    this.assertActive();
    if (!Number.isSafeInteger(encodedBytes) || encodedBytes < 0)
      throw new YasProtocolError("invalid STATE catalogue retained size");
    if (Object.is(from, to)) {
      this.upsert(to, encodedBytes);
      return;
    }
    const previous = this.sizes.get(from) ?? 0;
    const replaced = this.sizes.get(to) ?? 0;
    const removedItems = this.sizes.has(from) ? 1 : 0;
    const addedItems = this.sizes.has(to) ? 0 : 1;
    const size = encodedBytes + STATE_CATALOGUE_ENTRY_OVERHEAD;
    const next = this._bytes - previous - replaced + size;
    const increase = next - this._bytes;
    const itemIncrease = Math.max(0, addedItems - removedItems);
    this.pool.reserve(Math.max(0, increase), itemIncrease);
    try {
      this.sizes.set(to, size);
      this.sizes.delete(from);
    } catch (error) {
      this.pool.release(Math.max(0, increase), itemIncrease);
      throw error;
    }
    this._bytes = next;
    if (increase < 0) this.pool.release(-increase);
    if (removedItems > addedItems)
      this.pool.release(0, removedItems - addedItems);
  }

  dispose(): void {
    if (!this.active) return;
    this.active = false;
    this.pool.release(this._bytes, this.sizes.size);
    this._bytes = 0;
    this.sizes.clear();
  }

  private assertActive(): void {
    if (!this.active)
      throw new Error("disposed STATE catalogue retention ledger");
  }
}

export interface YasWatchOptions {
  initialCredit?: bigint;
  resume?: { bootId: Uint8Array; revision: bigint };
  extensions?: readonly YasExtension[];
}

export type YasWatchPayloadBuilder = (
  encodedWatch: Uint8Array,
  initialCredit: bigint,
) => Uint8Array;

export interface YasWatchResult {
  subscriptionId: number;
  mode: number;
  currentRevision: bigint;
  extensions: readonly YasExtension[];
}

export interface YasStateAck {
  subscriptionId: number;
  appliedRevision: bigint;
  cumulativeByteLimit: bigint;
}

export function encodeUnwatch(subscriptionId: number): Uint8Array {
  if (
    !Number.isInteger(subscriptionId) ||
    subscriptionId <= 0 ||
    subscriptionId > 0xffff_ffff
  )
    throw new YasProtocolError("subscription ID is invalid");
  return new YasWriter().u32(subscriptionId).finish();
}

export function decodeUnwatch(bytes: Uint8Array): number {
  const cursor = new YasCursor(bytes);
  const subscriptionId = cursor.u32("subscription ID");
  cursor.end("UNWATCH");
  encodeUnwatch(subscriptionId);
  return subscriptionId;
}

export function encodeStateAck(value: YasStateAck): Uint8Array {
  if (
    !Number.isInteger(value.subscriptionId) ||
    value.subscriptionId <= 0 ||
    value.subscriptionId > 0xffff_ffff
  )
    throw new YasProtocolError("subscription ID is invalid");
  if (value.appliedRevision === 0n)
    throw new YasProtocolError("STATE_ACK applied revision is zero");
  return new YasWriter()
    .u32(value.subscriptionId)
    .u64(value.appliedRevision)
    .u64(value.cumulativeByteLimit)
    .finish();
}

export function decodeStateAck(bytes: Uint8Array): YasStateAck {
  const cursor = new YasCursor(bytes);
  const value = {
    subscriptionId: cursor.u32("subscription ID"),
    appliedRevision: cursor.u64("applied revision"),
    cumulativeByteLimit: cursor.u64("cumulative byte limit"),
  };
  cursor.end("STATE_ACK");
  encodeStateAck(value);
  return value;
}

export interface YasStateBatch {
  phase: number;
  flags: number;
  fromRevision: bigint;
  toRevision: bigint;
  records: readonly YasTypedRecord[];
}

export interface YasStateEventSchema {
  /** Family-defined STATE flag bits accepted by this family version. */
  allowedFlags?: number;
  /** Record kinds understood by this family, including common kinds 0..3. */
  knownRecordKinds?: ReadonlySet<number>;
}

const commonStateRecordKinds = new Set([
  YAS_STATE_ADD,
  YAS_STATE_REPLACE,
  YAS_STATE_PATCH,
  YAS_STATE_REMOVE,
]);

export function encodeWatch(
  options: YasWatchOptions,
  initialCredit: bigint,
): Uint8Array {
  const flags = options.resume ? YAS_WATCH_RESUME : 0;
  const writer = new YasWriter().u16(flags).u16(0).u64(initialCredit);
  if (options.resume) {
    if (options.resume.bootId.length !== 16)
      throw new YasProtocolError("WATCH resume boot ID must contain 16 bytes");
    if (options.resume.revision === 0n)
      throw new YasProtocolError("WATCH resume revision must be nonzero");
    writer.bytes(options.resume.bootId).u64(options.resume.revision);
  }
  return writer.bytes(encodeExtensions(options.extensions)).finish();
}

export function decodeWatchResult(body: Uint8Array): YasWatchResult {
  const cursor = new YasCursor(body);
  const subscriptionId = cursor.u32("subscription ID");
  const mode = cursor.u8("WATCH mode");
  if (subscriptionId === 0)
    throw new YasProtocolError("subscription ID zero is invalid");
  if (mode !== YAS_WATCH_MODE_SNAPSHOT && mode !== YAS_WATCH_MODE_REPLAY)
    throw new YasProtocolError("unknown WATCH mode");
  if (cursor.take(3, "WATCH reserved").some((value) => value !== 0))
    throw new YasProtocolError("WATCH Result reserved bytes are nonzero");
  const currentRevision = cursor.u64("current revision");
  if (currentRevision === 0n)
    throw new YasProtocolError("WATCH Result revision is zero");
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "WATCH Result extensions",
  );
  cursor.end("WATCH Result");
  return { subscriptionId, mode, currentRevision, extensions };
}

export function decodeStateEvent(
  payload: Uint8Array,
  schema: YasStateEventSchema = {},
): {
  subscriptionId: number;
  batch: YasStateBatch;
} {
  const cursor = new YasCursor(payload);
  const subscriptionId = cursor.u32("subscription ID");
  const phase = cursor.u8("STATE phase");
  const flags = cursor.u8("STATE flags");
  if (subscriptionId === 0)
    throw new YasProtocolError("subscription ID zero is invalid");
  if (phase > YAS_STATE_RESET)
    throw new YasProtocolError("unknown STATE phase");
  if (flags & ~(schema.allowedFlags ?? 0))
    throw new YasProtocolError("reserved STATE flags are nonzero");
  if (cursor.u16("STATE reserved") !== 0)
    throw new YasProtocolError("STATE reserved field is nonzero");
  const fromRevision = cursor.u64("STATE from revision");
  const toRevision = cursor.u64("STATE to revision");
  if (toRevision === 0n)
    throw new YasProtocolError("STATE target revision is zero");
  const count = cursor.u16("STATE record count");
  const records: YasTypedRecord[] = [];
  const knownRecordKinds = schema.knownRecordKinds ?? commonStateRecordKinds;
  for (let i = 0; i < count; i++) {
    const record = decodeTypedRecord(cursor);
    if (knownRecordKinds.has(record.kind)) records.push(record);
    else if (record.flags & 1)
      throw new YasProtocolError("unknown required STATE record kind");
  }
  if (
    count !== 0 &&
    (phase === YAS_STATE_SNAPSHOT_BEGIN || phase === YAS_STATE_RESET)
  )
    throw new YasProtocolError("STATE marker phase contains records");
  cursor.end("STATE Event");
  return {
    subscriptionId,
    batch: { phase, flags, fromRevision, toRevision, records },
  };
}

/** One negotiated family state subscription with explicit replay and credit. */
export class YasStateSubscription {
  private appliedRevision = 0n;
  private cumulativeByteLimit: bigint;
  private closed = false;
  private leaseReleased = false;
  private snapshotTarget: bigint | null = null;
  private resetTarget: bigint | null = null;
  private removeEventListener: (() => void) | null = null;
  private removeInvalidationListener: (() => void) | null = null;
  private readonly onStatus = (status: ConnectionStatus) => {
    if (
      status === "connected" ||
      status === "connecting" ||
      status === "authenticating"
    )
      return;
    this.closeLocal();
  };

  private constructor(
    private readonly connection: YasConnection,
    readonly family: number,
    private readonly unwatchKind: number,
    private readonly stateAckKind: number,
    readonly result: YasWatchResult,
    private readonly lease: YasReceiveBudgetLease,
    private readonly onBatch: (batch: YasStateBatch) => void,
    initialAppliedRevision: bigint,
    private readonly eventSchema: YasStateEventSchema,
  ) {
    this.cumulativeByteLimit = lease.bytes;
    this.appliedRevision = initialAppliedRevision;
    connection.transport.addEventListener("statuschange", this.onStatus);
    this.removeInvalidationListener = connection.onInvalidation(
      ({ family: invalidatedFamily }) => {
        if (
          invalidatedFamily === undefined ||
          invalidatedFamily === this.family
        )
          this.closeLocal();
      },
    );
  }

  static async watch(
    connection: YasConnection,
    family: number,
    watchKind: number,
    unwatchKind: number,
    stateKind: number,
    stateAckKind: number,
    options: YasWatchOptions,
    onBatch: (batch: YasStateBatch) => void,
    eventSchema: YasStateEventSchema = {},
    buildPayload: YasWatchPayloadBuilder = (payload) => payload,
  ): Promise<YasStateSubscription> {
    const preferred = options.initialCredit ?? 1024n * 1024n;
    const lease = connection.receiveBudget.reserve(preferred, 1024n);
    try {
      return await connection.requestDecoded(
        family,
        watchKind,
        buildPayload(encodeWatch(options, lease.bytes), lease.bytes),
        (body) => {
          const result = decodeWatchResult(body);
          const initialRevision =
            result.mode === YAS_WATCH_MODE_REPLAY
              ? (options.resume?.revision ?? 0n)
              : 0n;
          if (result.mode === YAS_WATCH_MODE_REPLAY && initialRevision === 0n)
            throw new YasProtocolError(
              "server selected replay without a resume cursor",
            );
          const subscription = new YasStateSubscription(
            connection,
            family,
            unwatchKind,
            stateAckKind,
            result,
            lease,
            onBatch,
            initialRevision,
            eventSchema,
          );
          subscription.removeEventListener = connection.onEvent(
            family,
            stateKind,
            ({ payload }) => subscription.handle(payload),
          );
          return subscription;
        },
      );
    } catch (error) {
      lease.release();
      throw error;
    }
  }

  get revision(): bigint {
    return this.appliedRevision;
  }

  get active(): boolean {
    return !this.closed;
  }

  async unwatch(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    try {
      await this.connection.request(
        this.family,
        this.unwatchKind,
        new YasWriter().u32(this.result.subscriptionId).finish(),
      );
    } finally {
      this.closeLocal();
    }
  }

  private closeLocal(): void {
    this.closed = true;
    this.removeEventListener?.();
    this.removeEventListener = null;
    this.connection.transport.removeEventListener(
      "statuschange",
      this.onStatus,
    );
    this.removeInvalidationListener?.();
    this.removeInvalidationListener = null;
    if (!this.leaseReleased) {
      this.leaseReleased = true;
      this.lease.release();
    }
  }

  private handle(payload: Uint8Array): void {
    const { subscriptionId, batch } = decodeStateEvent(
      payload,
      this.eventSchema,
    );
    if (subscriptionId !== this.result.subscriptionId) return;
    this.validateSequence(batch);
    this.onBatch(batch);
    if (
      batch.phase === YAS_STATE_SNAPSHOT_END ||
      batch.phase === YAS_STATE_DELTA
    )
      this.appliedRevision = batch.toRevision;
    this.cumulativeByteLimit += BigInt(payload.length);
    this.connection.sendEvent(
      this.family,
      this.stateAckKind,
      new YasWriter()
        .u32(this.result.subscriptionId)
        .u64(this.appliedRevision)
        .u64(this.cumulativeByteLimit)
        .finish(),
    );
  }

  private validateSequence(batch: YasStateBatch): void {
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      if (
        batch.fromRevision !== 0n ||
        this.snapshotTarget !== null ||
        (this.resetTarget !== null && batch.toRevision !== this.resetTarget)
      )
        throw new YasProtocolError("invalid STATE snapshot begin");
      this.snapshotTarget = batch.toRevision;
      this.resetTarget = null;
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (
        this.snapshotTarget === null ||
        batch.fromRevision !== this.snapshotTarget ||
        batch.toRevision !== this.snapshotTarget
      )
        throw new YasProtocolError(
          "STATE snapshot records do not match their target",
        );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (
        this.snapshotTarget === null ||
        batch.fromRevision !== this.snapshotTarget ||
        batch.toRevision !== this.snapshotTarget
      )
        throw new YasProtocolError(
          "STATE snapshot end does not match its target",
        );
      this.snapshotTarget = null;
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      if (
        this.snapshotTarget !== null ||
        this.resetTarget !== null ||
        batch.fromRevision !== this.appliedRevision ||
        batch.toRevision <= batch.fromRevision
      )
        throw new YasProtocolError("STATE delta has a gap or invalid revision");
      return;
    }
    // RESET identifies the last valid cursor and the next snapshot revision.
    if (batch.fromRevision !== this.appliedRevision || batch.toRevision === 0n)
      throw new YasProtocolError(
        "STATE reset does not match the applied cursor",
      );
    this.snapshotTarget = null;
    this.resetTarget = batch.toRevision;
  }
}
