/** Durable per-browser-device workspace attachment ordering. */

import {
  WORKSPACE_SESSION_MAX_CATALOG_ENTRIES,
  WorkspaceSessionValidationError,
  type WorkspaceSessionInvalidRecord,
  type WorkspaceSessionKv,
  type WorkspaceSessionOwnedKv,
  type WorkspaceSessionStoreStatus,
  isWorkspaceSessionId,
  workspaceSessionKey,
} from "./workspaceSessions";
import {
  WorkspaceSessionKvConflictError,
  copyWorkspaceSessionHash,
  workspaceSessionHashesEqual,
  type WorkspaceSessionHash,
  type WorkspaceSessionKvEntry,
  type WorkspaceSessionKvWatch,
} from "./workspaceSessionKv";
import { YasNativeWorkspaceKv } from "./yas/nativeWorkspaceKv";
import type { YasConnection } from "./yas/session";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

export const WORKSPACE_SESSION_DEVICE_VERSION = 1 as const;
export const WORKSPACE_SESSION_DEVICE_KEY_PREFIX =
  "ui/workspace-session-devices/v1/";
export const WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY =
  "yas.workspaceSessionDeviceId";
export const WORKSPACE_SESSION_DEVICE_MAX_ATTACHED_SESSIONS = 256;
export const WORKSPACE_SESSION_DEVICE_MAX_DOCUMENT_BYTES = 64 * 1024;
export const WORKSPACE_SESSION_DEVICE_MAX_QUARANTINED_KEYS = 32;

const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const WATCH_INLINE_MAX = 32 * 1024;
const DEFAULT_CAS_RETRIES = 3;

export interface StoredWorkspaceSessionDevice {
  version: 1;
  deviceId: string;
  /** Ordered, unique durable workspace IDs attached on this device. */
  attachedSessionIds: string[];
  createdAtUnixMs: number;
  updatedAtUnixMs: number;
}

export interface CreateStoredWorkspaceSessionDeviceOptions {
  deviceId: string;
  nowUnixMs?: number;
  attachedSessionIds?: readonly string[];
}

export interface WorkspaceSessionDeviceStoreSnapshot {
  status: WorkspaceSessionStoreStatus;
  device: StoredWorkspaceSessionDevice | null;
  error: Error | null;
  invalidRecords: readonly WorkspaceSessionInvalidRecord[];
}

export interface WorkspaceSessionDeviceStoreOptions {
  now?: () => number;
  casRetries?: number;
  onInvalidRecord?: (record: WorkspaceSessionInvalidRecord) => void;
  /** @internal Deterministic seam for YAS reconnect tests. */
  yasKvFactory?: (connection: YasConnection) => WorkspaceSessionOwnedKv;
}

export interface WorkspaceSessionDeviceAttachOptions {
  /** Insert/move immediately before this attached ID; append if it is absent. */
  beforeSessionId?: string;
}

export interface WorkspaceSessionDeviceClaimResult {
  device: StoredWorkspaceSessionDevice;
  claimed: boolean;
}

export function isWorkspaceSessionDeviceId(value: unknown): value is string {
  return typeof value === "string" && UUID_RE.test(value);
}

export function parseWorkspaceSessionDeviceId(value: unknown): string {
  if (!isWorkspaceSessionDeviceId(value))
    throw new WorkspaceSessionValidationError(
      "workspace device id is not a canonical lowercase UUID",
    );
  return value;
}

export function createWorkspaceSessionDeviceId(
  randomUUID: () => string = defaultRandomUUID,
): string {
  return parseWorkspaceSessionDeviceId(randomUUID());
}

export function workspaceSessionDeviceKey(deviceId: string): string {
  return `${WORKSPACE_SESSION_DEVICE_KEY_PREFIX}${parseWorkspaceSessionDeviceId(deviceId)}`;
}

export function createDefaultStoredWorkspaceSessionDevice(
  options: CreateStoredWorkspaceSessionDeviceOptions,
): StoredWorkspaceSessionDevice {
  const now = timestamp(options.nowUnixMs ?? Date.now(), "current time");
  return parseStoredWorkspaceSessionDevice({
    version: WORKSPACE_SESSION_DEVICE_VERSION,
    deviceId: options.deviceId,
    attachedSessionIds: options.attachedSessionIds ?? [],
    createdAtUnixMs: now,
    updatedAtUnixMs: now,
  });
}

/** Parse, bound, copy, and freeze an untrusted device attachment record. */
export function parseStoredWorkspaceSessionDevice(
  value: unknown,
): StoredWorkspaceSessionDevice {
  const input = strictObject(
    value,
    [
      "version",
      "deviceId",
      "attachedSessionIds",
      "createdAtUnixMs",
      "updatedAtUnixMs",
    ],
    "workspace device",
  );
  if (input.version !== WORKSPACE_SESSION_DEVICE_VERSION)
    invalid("workspace device version is unsupported");
  const deviceId = parseWorkspaceSessionDeviceId(input.deviceId);
  const createdAtUnixMs = timestamp(input.createdAtUnixMs, "createdAtUnixMs");
  const updatedAtUnixMs = timestamp(input.updatedAtUnixMs, "updatedAtUnixMs");
  if (updatedAtUnixMs < createdAtUnixMs)
    invalid("updatedAtUnixMs predates createdAtUnixMs");
  return deepFreeze({
    version: WORKSPACE_SESSION_DEVICE_VERSION,
    deviceId,
    attachedSessionIds: sessionIds(input.attachedSessionIds),
    createdAtUnixMs,
    updatedAtUnixMs,
  });
}

export function isStoredWorkspaceSessionDevice(
  value: unknown,
): value is StoredWorkspaceSessionDevice {
  try {
    parseStoredWorkspaceSessionDevice(value);
    return true;
  } catch {
    return false;
  }
}

function sessionIds(value: unknown): string[] {
  if (!Array.isArray(value)) invalid("attachedSessionIds is not an array");
  if (value.length > WORKSPACE_SESSION_DEVICE_MAX_ATTACHED_SESSIONS)
    invalid("attached workspace count exceeds its limit");
  const result = value.map((id) => {
    if (!isWorkspaceSessionId(id))
      invalid("attached workspace id is not a canonical UUID");
    return id;
  });
  if (new Set(result).size !== result.length)
    invalid("attachedSessionIds contains a duplicate");
  return result;
}

function timestamp(value: unknown, name: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0)
    invalid(`${name} is not a non-negative safe integer`);
  return value as number;
}

function strictObject(
  value: unknown,
  keys: readonly string[],
  name: string,
): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    invalid(`${name} is not an object`);
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null)
    invalid(`${name} does not have a plain object prototype`);
  const object = value as Record<string, unknown>;
  const allowed = new Set(keys);
  for (const key of Object.keys(object)) {
    if (!allowed.has(key)) invalid(`${name} has unknown field ${key}`);
  }
  for (const key of keys) {
    if (!Object.prototype.hasOwnProperty.call(object, key))
      invalid(`${name} is missing field ${key}`);
  }
  return object;
}

function invalid(message: string): never {
  throw new WorkspaceSessionValidationError(message);
}

interface IndexedDevice {
  record: StoredWorkspaceSessionDevice;
  hash: WorkspaceSessionHash;
  mtimeNs: bigint;
  byteLength: number;
}

interface PendingDevice {
  indexed: IndexedDevice;
  previousHash: WorkspaceSessionHash | null;
}

interface DeviceReconcileRequest {
  generation: number;
  kv: WorkspaceSessionKv;
  mirror: {
    readonly live: ReadonlyMap<string, WorkspaceSessionKvEntry>;
    snapshotDone: boolean;
  };
}

/**
 * Exact-watched durable attachment order for one browser device. The active
 * workspace selection remains URL/tab-local; this record only owns membership
 * and ordering shared by tabs carrying the same device UUID. Raw YasConnection
 * sources recreate their native KV facades; fixed structural KV sources
 * require owner replacement after permanent transport invalidation.
 */
export class WorkspaceSessionDeviceStore {
  private kv: WorkspaceSessionKv | null;
  private readonly yasConnection: YasConnection | null;
  private ownedKv: WorkspaceSessionOwnedKv | null = null;
  private readonly yasKvFactory: (
    connection: YasConnection,
  ) => WorkspaceSessionOwnedKv;
  private readonly now: () => number;
  private readonly casRetries: number;
  private readonly onInvalidRecord?: (
    record: WorkspaceSessionInvalidRecord,
  ) => void;
  private readonly key: string;
  private readonly listeners = new Set<() => void>();
  private indexed: IndexedDevice | null = null;
  private pending: PendingDevice | null = null;
  private watch: WorkspaceSessionKvWatch | null = null;
  private started = false;
  private startPromise: Promise<void> | null = null;
  private resolveStart: (() => void) | null = null;
  private rejectStart: ((error: Error) => void) | null = null;
  private openGeneration = 0;
  private openInFlight = false;
  private retryAttempt = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  /** Serializes exact-watch reconciliation with every local state mutation. */
  private stateOperationTail: Promise<void> = Promise.resolve();
  private pendingReconcile: DeviceReconcileRequest | null = null;
  private reconcileQueued = false;
  private revisionValue = 0;
  private snapshot: WorkspaceSessionDeviceStoreSnapshot = freezeSnapshot({
    status: "idle",
    device: null,
    error: null,
    invalidRecords: [],
  });

  readonly deviceId: string;

  constructor(
    source: WorkspaceSessionKv | YasConnection,
    deviceId: string,
    options: WorkspaceSessionDeviceStoreOptions = {},
  ) {
    this.deviceId = parseWorkspaceSessionDeviceId(deviceId);
    this.key = workspaceSessionDeviceKey(this.deviceId);
    if (isKv(source)) {
      this.kv = source;
      this.yasConnection = null;
    } else {
      this.kv = null;
      this.yasConnection = source;
    }
    this.now = options.now ?? Date.now;
    const retries = options.casRetries ?? DEFAULT_CAS_RETRIES;
    if (!Number.isInteger(retries) || retries < 0 || retries > 10)
      invalid("workspace device CAS retry count is invalid");
    this.casRetries = retries;
    this.onInvalidRecord = options.onInvalidRecord;
    this.yasKvFactory =
      options.yasKvFactory ??
      ((connection) => new YasNativeWorkspaceKv(connection));
  }

  get revision(): number {
    return this.revisionValue;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): WorkspaceSessionDeviceStoreSnapshot => this.snapshot;

  start(): Promise<void> {
    if (this.snapshot.status === "closed")
      return Promise.reject(new Error("Workspace device store is closed"));
    if (this.snapshot.status === "ready") return Promise.resolve();
    if (this.startPromise) return this.startPromise;
    this.startPromise = new Promise<void>((resolve, reject) => {
      this.resolveStart = resolve;
      this.rejectStart = reject;
    });
    if (!this.started) {
      this.started = true;
      this.setStatus("loading", null);
      this.openWatch();
    } else if (!this.openInFlight && this.retryTimer === null) {
      this.openWatch();
    }
    return this.startPromise;
  }

  close(): void {
    if (this.snapshot.status === "closed") return;
    this.started = false;
    this.openGeneration++;
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.watch?.close();
    this.watch = null;
    this.ownedKv?.dispose();
    this.ownedKv = null;
    const error = new Error("Workspace device store is closed");
    this.rejectStart?.(error);
    this.resolveStart = null;
    this.rejectStart = null;
    this.startPromise = null;
    this.setStatus("closed", null);
  }

  dispose(): void {
    this.close();
  }

  async attach(
    sessionId: string,
    options: WorkspaceSessionDeviceAttachOptions = {},
  ): Promise<StoredWorkspaceSessionDevice> {
    assertSessionId(sessionId);
    const before = options.beforeSessionId;
    if (before !== undefined) assertSessionId(before);
    const result = await this.mutate((current) => {
      const ids = current ? [...current.attachedSessionIds] : [];
      const existing = ids.indexOf(sessionId);
      if (existing >= 0 && before === undefined) return current;
      if (existing >= 0 && before === sessionId) return current;
      if (existing >= 0) ids.splice(existing, 1);
      if (
        existing < 0 &&
        ids.length >= WORKSPACE_SESSION_DEVICE_MAX_ATTACHED_SESSIONS
      )
        invalid("attached workspace count exceeds its limit");
      const beforeIndex = before === undefined ? -1 : ids.indexOf(before);
      ids.splice(beforeIndex < 0 ? ids.length : beforeIndex, 0, sessionId);
      return this.nextRecord(current, ids);
    });
    if (!result) throw new Error("Workspace device attach failed");
    return result;
  }

  /**
   * Atomically initialize an absent device record with one workspace. Existing
   * records, including an intentionally empty one, are never auto-populated.
   */
  async claimInitialSession(
    sessionId: string,
  ): Promise<WorkspaceSessionDeviceClaimResult> {
    assertSessionId(sessionId);
    // Initial readiness is itself published by a reconciliation on the state
    // tail, so establish it before enqueueing the mutation.
    const kv = await this.operationalKv();
    return this.enqueueStateOperation(async () => {
      let lastConflict: WorkspaceSessionKvConflictError | null = null;
      for (let attempt = 0; attempt <= this.casRetries; attempt++) {
        const current = await this.fetchIndexed(kv);
        if (current) {
          this.cacheFetched(current);
          return { device: current.record, claimed: false };
        }
        const candidate = createDefaultStoredWorkspaceSessionDevice({
          deviceId: this.deviceId,
          attachedSessionIds: [sessionId],
          nowUnixMs: timestamp(this.now(), "current time"),
        });
        const bytes = encodeDocument(candidate);
        try {
          const result = await kv.kvPut(this.key, bytes, {
            create: true,
            durable: true,
          });
          this.installLocal(
            indexed(candidate, result.hash, result.mtimeNs, bytes.length),
            null,
          );
          return { device: candidate, claimed: true };
        } catch (error) {
          if (!(error instanceof WorkspaceSessionKvConflictError)) throw error;
          lastConflict = error;
        }
      }
      const latest = await this.fetchIndexed(kv);
      if (latest) {
        this.cacheFetched(latest);
        return { device: latest.record, claimed: false };
      }
      throw lastConflict ?? new Error("Workspace device claim conflicted");
    });
  }

  detach(sessionId: string): Promise<StoredWorkspaceSessionDevice | null> {
    assertSessionId(sessionId);
    return this.mutate((current) => {
      if (!current || !current.attachedSessionIds.includes(sessionId))
        return current;
      return this.nextRecord(
        current,
        current.attachedSessionIds.filter((id) => id !== sessionId),
      );
    });
  }

  /**
   * Reorder currently attached IDs. Unknown IDs are ignored and concurrently
   * attached IDs absent from `sessionIds` retain their relative order at end.
   */
  reorder(
    orderedSessionIds: readonly string[],
  ): Promise<StoredWorkspaceSessionDevice | null> {
    const requested = validateRequestedIds(orderedSessionIds);
    return this.mutate((current) => {
      if (!current) return null;
      const currentSet = new Set(current.attachedSessionIds);
      const requestedSet = new Set(requested);
      const reordered = [
        ...requested.filter((id) => currentSet.has(id)),
        ...current.attachedSessionIds.filter((id) => !requestedSet.has(id)),
      ];
      return arraysEqual(reordered, current.attachedSessionIds)
        ? current
        : this.nextRecord(current, reordered);
    });
  }

  /**
   * Remove IDs absent from a complete authoritative workspace catalogue.
   * A CAS race aborts this prune and returns the newer record unchanged; that
   * conservatively preserves a concurrent detach/reattach or new attachment.
   */
  async pruneDeleted(
    validSessionIds: ReadonlySet<string> | readonly string[],
  ): Promise<StoredWorkspaceSessionDevice | null> {
    const valid = validateValidIds(validSessionIds);
    const kv = await this.operationalKv();
    return this.enqueueStateOperation(async () => {
      const current = await this.fetchIndexed(kv);
      if (!current) return null;
      const candidates = current.record.attachedSessionIds.filter(
        (id) => !valid.has(id),
      );
      const deleted = await confirmedMissingSessionIds(kv, candidates);
      const remaining = current.record.attachedSessionIds.filter(
        (id) => !deleted.has(id),
      );
      if (arraysEqual(remaining, current.record.attachedSessionIds))
        return this.cacheFetched(current).record;
      const next = this.nextRecord(current.record, remaining);
      const bytes = encodeDocument(next);
      try {
        const result = await kv.kvPut(this.key, bytes, {
          ifHash: current.hash,
          durable: true,
        });
        this.installLocal(
          indexed(next, result.hash, result.mtimeNs, bytes.length),
          current.hash,
        );
        return next;
      } catch (error) {
        if (!(error instanceof WorkspaceSessionKvConflictError)) throw error;
        const latest = await this.fetchIndexed(kv);
        return latest ? this.cacheFetched(latest).record : null;
      }
    });
  }

  private nextRecord(
    current: StoredWorkspaceSessionDevice | null,
    attachedSessionIds: readonly string[],
  ): StoredWorkspaceSessionDevice {
    if (!current)
      return createDefaultStoredWorkspaceSessionDevice({
        deviceId: this.deviceId,
        attachedSessionIds,
        nowUnixMs: timestamp(this.now(), "current time"),
      });
    const updatedAtUnixMs = Math.max(
      timestamp(this.now(), "current time"),
      current.updatedAtUnixMs + 1,
    );
    if (!Number.isSafeInteger(updatedAtUnixMs))
      invalid("workspace device timestamp overflowed");
    return parseStoredWorkspaceSessionDevice({
      ...current,
      attachedSessionIds,
      updatedAtUnixMs,
    });
  }

  private async mutate(
    apply: (
      current: StoredWorkspaceSessionDevice | null,
    ) => StoredWorkspaceSessionDevice | null,
  ): Promise<StoredWorkspaceSessionDevice | null> {
    const kv = await this.operationalKv();
    return this.enqueueStateOperation(async () => {
      let lastConflict: WorkspaceSessionKvConflictError | null = null;
      for (let attempt = 0; attempt <= this.casRetries; attempt++) {
        const current = await this.fetchIndexed(kv);
        const next = apply(current?.record ?? null);
        if (!next) return null;
        if (current && sameRecord(current.record, next))
          return this.cacheFetched(current).record;
        const bytes = encodeDocument(next);
        try {
          const result = await kv.kvPut(this.key, bytes, {
            ...(current ? { ifHash: current.hash } : { create: true }),
            durable: true,
          });
          this.installLocal(
            indexed(next, result.hash, result.mtimeNs, bytes.length),
            current?.hash ?? null,
          );
          return next;
        } catch (error) {
          if (!(error instanceof WorkspaceSessionKvConflictError)) throw error;
          lastConflict = error;
        }
      }
      throw lastConflict ?? new Error("Workspace device update conflicted");
    });
  }

  private async operationalKv(): Promise<WorkspaceSessionKv> {
    await this.start();
    if (this.snapshot.status !== "ready") await this.waitUntilReady();
    if (!this.kv) throw new Error("Workspace device KV is unavailable");
    return this.kv;
  }

  private enqueueStateOperation<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.stateOperationTail.then(operation);
    this.stateOperationTail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  private waitUntilReady(): Promise<void> {
    if (this.snapshot.status === "ready") return Promise.resolve();
    if (this.snapshot.status === "error" || this.snapshot.status === "closed")
      return Promise.reject(
        this.snapshot.error ??
          new Error("Workspace device store is unavailable"),
      );
    return new Promise((resolve, reject) => {
      const unsubscribe = this.subscribe(() => {
        if (this.snapshot.status === "ready") {
          unsubscribe();
          resolve();
        } else if (
          this.snapshot.status === "error" ||
          this.snapshot.status === "closed"
        ) {
          unsubscribe();
          reject(
            this.snapshot.error ??
              new Error("Workspace device store is unavailable"),
          );
        }
      });
    });
  }

  private openWatch(): void {
    if (!this.started || this.openInFlight || this.retryTimer !== null) return;
    this.openInFlight = true;
    const generation = ++this.openGeneration;
    let kv: WorkspaceSessionKv;
    try {
      if (this.yasConnection) {
        this.ownedKv?.dispose();
        this.ownedKv = this.yasKvFactory(this.yasConnection);
        this.kv = this.ownedKv;
      }
      if (!this.kv) throw new Error("Workspace device KV is unavailable");
      kv = this.kv;
    } catch (error) {
      this.openInFlight = false;
      this.watchFailed(generation, asError(error));
      return;
    }
    void kv
      .watchKv(this.key, {
        inlineMax: WATCH_INLINE_MAX,
        onUpdate: (mirror) => this.enqueueReconcile(generation, kv, mirror),
        onClosed: (error) => this.watchFailed(generation, error),
      })
      .then((watch) => {
        if (!this.started || generation !== this.openGeneration) {
          watch.close();
          return;
        }
        this.openInFlight = false;
        this.watch = watch;
        this.enqueueReconcile(generation, kv, watch.mirror);
      })
      .catch((error: unknown) => this.watchFailed(generation, asError(error)));
  }

  private watchFailed(generation: number, error: Error): void {
    if (!this.started || generation !== this.openGeneration) return;
    this.openGeneration++;
    this.openInFlight = false;
    this.watch?.close();
    this.watch = null;
    this.rejectPendingStart(error);
    if (!this.yasConnection) {
      this.started = false;
      this.setStatus("error", error);
      return;
    }
    this.ownedKv?.dispose();
    this.ownedKv = null;
    this.kv = null;
    this.setStatus("loading", error);
    const delay = Math.min(5_000, 100 * 2 ** Math.min(this.retryAttempt++, 6));
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.openWatch();
    }, delay);
  }

  private enqueueReconcile(
    generation: number,
    kv: WorkspaceSessionKv,
    mirror: {
      readonly live: ReadonlyMap<string, WorkspaceSessionKvEntry>;
      snapshotDone: boolean;
    },
  ): void {
    if (!this.started || generation !== this.openGeneration) return;
    this.pendingReconcile = { generation, kv, mirror };
    this.scheduleReconcile();
  }

  private scheduleReconcile(): void {
    if (this.reconcileQueued || !this.pendingReconcile) return;
    this.reconcileQueued = true;
    void this.enqueueStateOperation(async () => {
      const request = this.pendingReconcile;
      this.pendingReconcile = null;
      if (!request) return;
      try {
        await this.reconcile(request.generation, request.kv, request.mirror);
      } catch (error) {
        this.watchFailed(request.generation, asError(error));
      }
    }).finally(() => {
      this.reconcileQueued = false;
      if (this.pendingReconcile) this.scheduleReconcile();
    });
  }

  private async reconcile(
    generation: number,
    kv: WorkspaceSessionKv,
    mirror: {
      readonly live: ReadonlyMap<string, WorkspaceSessionKvEntry>;
      snapshotDone: boolean;
    },
  ): Promise<void> {
    if (!this.started || generation !== this.openGeneration) return;
    // Fresh snapshots replace atomically; never publish a partial recovery.
    if (!mirror.snapshotDone) return;
    const invalidRecords: WorkspaceSessionInvalidRecord[] = [];
    for (const key of mirror.live.keys()) {
      if (key === this.key) continue;
      if (invalidRecords.length < WORKSPACE_SESSION_DEVICE_MAX_QUARANTINED_KEYS)
        invalidRecords.push({
          key,
          message: "device watch returned a non-exact suffix key",
        });
    }

    const entry = mirror.live.get(this.key);
    let next: IndexedDevice | null = null;
    if (!entry) {
      if (this.pending?.previousHash === null) {
        const pending = this.pending;
        // Watch delivery can lag a just-completed first claim, but another tab
        // can delete the record before that lagging update is reconciled. An
        // exact GET distinguishes the two and prevents a permanent ghost tab
        // record after the authoritative key is gone.
        try {
          const fetched = await this.fetchIndexed(kv);
          if (!fetched) {
            this.pending = null;
          } else if (
            workspaceSessionHashesEqual(fetched.hash, pending.indexed.hash)
          ) {
            next = pending.indexed;
          } else {
            next = fetched;
            this.pending = { indexed: fetched, previousHash: null };
          }
        } catch (error) {
          if (!(error instanceof WorkspaceSessionValidationError)) throw error;
          invalidRecords.unshift({ key: this.key, message: error.message });
          invalidRecords.splice(WORKSPACE_SESSION_DEVICE_MAX_QUARANTINED_KEYS);
          next = pending.indexed;
        }
      } else this.pending = null;
    } else if (this.pending) {
      if (workspaceSessionHashesEqual(entry.hash, this.pending.indexed.hash)) {
        next = this.pending.indexed;
        this.pending = null;
      } else if (
        this.pending.previousHash !== null &&
        workspaceSessionHashesEqual(entry.hash, this.pending.previousHash)
      ) {
        next = this.pending.indexed;
      } else {
        this.pending = null;
      }
    }

    if (entry && !next) {
      try {
        next = await this.loadEntry(kv, entry);
      } catch (error) {
        invalidRecords.unshift({
          key: this.key,
          message: asError(error).message,
        });
        invalidRecords.splice(WORKSPACE_SESSION_DEVICE_MAX_QUARANTINED_KEYS);
        // Quarantine a bad replacement without discarding the last good state.
        next = this.indexed;
      }
    }
    if (!this.started || generation !== this.openGeneration) return;
    this.commit(next, invalidRecords);
    this.retryAttempt = 0;
    this.setStatus(
      "ready",
      invalidRecords.length === 0
        ? null
        : new WorkspaceSessionValidationError(
            `${invalidRecords.length} workspace device record(s) quarantined`,
          ),
    );
    this.resolveStart?.();
    this.resolveStart = null;
    this.rejectStart = null;
    this.startPromise = null;
  }

  private async loadEntry(
    kv: WorkspaceSessionKv,
    entry: WorkspaceSessionKvEntry,
  ): Promise<IndexedDevice | null> {
    if (entry.size > WORKSPACE_SESSION_DEVICE_MAX_DOCUMENT_BYTES)
      invalid("workspace device document exceeds its byte limit");
    let bytes: Uint8Array;
    let hash = entry.hash;
    if (entry.value !== null) bytes = new Uint8Array(entry.value);
    else {
      const fetched = await kv.kvFetch(this.key);
      if (!fetched) return null;
      bytes = new Uint8Array(fetched.value);
      hash = fetched.hash;
      if (!workspaceSessionHashesEqual(hash, entry.hash)) {
        const fetched = indexed(
          decodeDocument(bytes, this.deviceId),
          hash,
          entry.mtimeNs,
          bytes.length,
        );
        this.pending = {
          indexed: fetched,
          previousHash: copyWorkspaceSessionHash(entry.hash),
        };
        return fetched;
      }
    }
    return indexed(
      decodeDocument(bytes, this.deviceId),
      hash,
      entry.mtimeNs,
      bytes.length,
    );
  }

  private async fetchIndexed(
    kv: WorkspaceSessionKv,
  ): Promise<IndexedDevice | null> {
    const fetched = await kv.kvFetch(this.key);
    if (!fetched) return null;
    const bytes = new Uint8Array(fetched.value);
    const record = decodeDocument(bytes, this.deviceId);
    const mirrorEntry = this.watch?.mirror.live.get(this.key);
    return indexed(
      record,
      fetched.hash,
      mirrorEntry && workspaceSessionHashesEqual(mirrorEntry.hash, fetched.hash)
        ? mirrorEntry.mtimeNs
        : (this.indexed?.mtimeNs ?? 0n),
      bytes.length,
    );
  }

  private installLocal(
    indexedValue: IndexedDevice,
    previousHash: WorkspaceSessionHash | null,
  ): void {
    this.pending = { indexed: indexedValue, previousHash };
    this.commit(indexedValue, []);
  }

  private cacheFetched(value: IndexedDevice): IndexedDevice {
    if (!workspaceSessionHashesEqual(this.indexed?.hash, value.hash))
      this.installLocal(value, this.indexed?.hash ?? null);
    return value;
  }

  private commit(
    next: IndexedDevice | null,
    invalidRecordsValue: readonly WorkspaceSessionInvalidRecord[],
  ): void {
    const recordChanged = !workspaceSessionHashesEqual(
      this.indexed?.hash,
      next?.hash,
    );
    const invalidChanged = !invalidRecordsEqual(
      this.snapshot.invalidRecords,
      invalidRecordsValue,
    );
    if (!recordChanged && !invalidChanged) return;
    const previousInvalid = new Map(
      this.snapshot.invalidRecords.map((record) => [
        record.key,
        record.message,
      ]),
    );
    this.indexed = next;
    this.snapshot = freezeSnapshot({
      ...this.snapshot,
      device: next?.record ?? null,
      invalidRecords: [...invalidRecordsValue],
    });
    this.emit();
    for (const record of invalidRecordsValue) {
      if (previousInvalid.get(record.key) !== record.message)
        this.onInvalidRecord?.(record);
    }
  }

  private setStatus(
    status: WorkspaceSessionStoreStatus,
    error: Error | null,
  ): void {
    if (this.snapshot.status === status && this.snapshot.error === error)
      return;
    this.snapshot = freezeSnapshot({ ...this.snapshot, status, error });
    this.emit();
  }

  private rejectPendingStart(error: Error): void {
    this.rejectStart?.(error);
    this.resolveStart = null;
    this.rejectStart = null;
    this.startPromise = null;
  }

  private emit(): void {
    this.revisionValue++;
    for (const listener of [...this.listeners]) listener();
  }
}

function validateRequestedIds(value: readonly string[]): string[] {
  return sessionIds(value);
}

function validateValidIds(
  value: ReadonlySet<string> | readonly string[],
): ReadonlySet<string> {
  const ids = Array.isArray(value) ? [...value] : [...value];
  if (ids.length > WORKSPACE_SESSION_MAX_CATALOG_ENTRIES)
    invalid("valid workspace catalogue exceeds its limit");
  for (const id of ids) assertSessionId(id);
  if (new Set(ids).size !== ids.length)
    invalid("valid workspace catalogue contains a duplicate");
  return new Set(ids);
}

async function confirmedMissingSessionIds(
  kv: WorkspaceSessionKv,
  candidates: readonly string[],
): Promise<ReadonlySet<string>> {
  const missing = new Set<string>();
  // Bound in-flight GETs independently of the device attachment bound.
  for (let offset = 0; offset < candidates.length; offset += 16) {
    const chunk = candidates.slice(offset, offset + 16);
    const results = await Promise.all(
      chunk.map(async (id) => ({
        id,
        value: await kv.kvFetch(workspaceSessionKey(id)),
      })),
    );
    for (const { id, value } of results) {
      if (value === null) missing.add(id);
    }
  }
  return missing;
}

function assertSessionId(value: unknown): asserts value is string {
  if (!isWorkspaceSessionId(value))
    invalid("workspace id is not a canonical lowercase UUID");
}

function arraysEqual(
  left: readonly string[],
  right: readonly string[],
): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function sameRecord(
  left: StoredWorkspaceSessionDevice,
  right: StoredWorkspaceSessionDevice,
): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function encodeDocument(record: StoredWorkspaceSessionDevice): Uint8Array {
  const canonical = parseStoredWorkspaceSessionDevice(record);
  const bytes = encoder.encode(JSON.stringify(canonical));
  if (bytes.length > WORKSPACE_SESSION_DEVICE_MAX_DOCUMENT_BYTES)
    invalid("workspace device document exceeds its byte limit");
  return bytes;
}

function decodeDocument(
  bytesValue: Uint8Array,
  expectedDeviceId: string,
): StoredWorkspaceSessionDevice {
  const bytes = new Uint8Array(bytesValue);
  if (bytes.length > WORKSPACE_SESSION_DEVICE_MAX_DOCUMENT_BYTES)
    invalid("workspace device document exceeds its byte limit");
  let value: unknown;
  try {
    value = JSON.parse(decoder.decode(bytes));
  } catch (error) {
    throw new WorkspaceSessionValidationError(
      `workspace device JSON is invalid: ${asError(error).message}`,
    );
  }
  const record = parseStoredWorkspaceSessionDevice(value);
  if (record.deviceId !== expectedDeviceId)
    invalid("workspace device key and document id do not match");
  return record;
}

function indexed(
  record: StoredWorkspaceSessionDevice,
  hash: WorkspaceSessionHash,
  mtimeNs: bigint,
  byteLength: number,
): IndexedDevice {
  if (
    !Number.isSafeInteger(byteLength) ||
    byteLength < 0 ||
    byteLength > WORKSPACE_SESSION_DEVICE_MAX_DOCUMENT_BYTES
  )
    invalid("workspace device byte length is invalid");
  return {
    record,
    hash: copyWorkspaceSessionHash(hash),
    mtimeNs,
    byteLength,
  };
}

function invalidRecordsEqual(
  left: readonly WorkspaceSessionInvalidRecord[],
  right: readonly WorkspaceSessionInvalidRecord[],
): boolean {
  return (
    left.length === right.length &&
    left.every(
      (record, index) =>
        record.key === right[index]?.key &&
        record.message === right[index]?.message,
    )
  );
}

function isKv(value: unknown): value is WorkspaceSessionKv {
  if (value === null || typeof value !== "object") return false;
  const candidate = value as Partial<WorkspaceSessionKv>;
  return (
    typeof candidate.kvPut === "function" &&
    typeof candidate.kvDelete === "function" &&
    typeof candidate.kvFetch === "function" &&
    typeof candidate.watchKv === "function"
  );
}

function defaultRandomUUID(): string {
  const randomUUID = globalThis.crypto?.randomUUID;
  if (!randomUUID)
    throw new Error("crypto.randomUUID is unavailable for device creation");
  return randomUUID.call(globalThis.crypto);
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function deepFreeze<T>(value: T): T {
  if (value === null || typeof value !== "object" || Object.isFrozen(value))
    return value;
  for (const child of Object.values(value as Record<string, unknown>))
    deepFreeze(child);
  return Object.freeze(value) as T;
}

function freezeSnapshot(
  value: WorkspaceSessionDeviceStoreSnapshot,
): WorkspaceSessionDeviceStoreSnapshot {
  for (const record of value.invalidRecords) Object.freeze(record);
  Object.freeze(value.invalidRecords);
  return Object.freeze(value);
}
