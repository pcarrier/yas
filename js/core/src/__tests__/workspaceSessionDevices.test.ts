import { afterEach, describe, expect, it, vi } from "vitest";

import {
  WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY,
  WORKSPACE_SESSION_DEVICE_KEY_PREFIX,
  WORKSPACE_SESSION_DEVICE_MAX_ATTACHED_SESSIONS,
  WORKSPACE_SESSION_DEVICE_MAX_QUARANTINED_KEYS,
  WorkspaceSessionDeviceStore,
  createDefaultStoredWorkspaceSessionDevice,
  createWorkspaceSessionDeviceId,
  parseStoredWorkspaceSessionDevice,
  workspaceSessionDeviceKey,
  type StoredWorkspaceSessionDevice,
} from "../workspaceSessionDevices";
import {
  WorkspaceSessionKvConflictError,
  WorkspaceSessionValidationError,
  type WorkspaceSessionKv,
  type WorkspaceSessionKvDeleteOptions,
  type WorkspaceSessionKvEntry,
  type WorkspaceSessionKvMirror,
  type WorkspaceSessionKvPutOptions,
  type WorkspaceSessionKvWatchOptions,
  workspaceSessionKey,
} from "../workspaceSessions";
import type { YasConnection } from "../yas/session";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

interface Entry {
  value: Uint8Array;
  hash: Uint8Array;
  mtimeNs: bigint;
  forceMetadata: boolean;
}

interface Watch {
  prefix: string;
  inlineMax: number;
  mirror: WorkspaceSessionKvMirror & {
    readonly live: Map<string, WorkspaceSessionKvEntry>;
  };
  options: WorkspaceSessionKvWatchOptions;
}

class Backend {
  readonly entries = new Map<string, Entry>();
  readonly watches = new Set<Watch>();
  readonly puts: Array<{
    key: string;
    options: WorkspaceSessionKvPutOptions;
  }> = [];
  readonly deletes: Array<{
    key: string;
    options: WorkspaceSessionKvDeleteOptions;
  }> = [];
  conflicts = 0;
  fetches = 0;
  beforeCasPut: ((key: string) => Promise<void>) | null = null;
  private hash = 1n;

  client(): WorkspaceSessionKv {
    const backend = this;
    return {
      async kvPut(key, value, options = {}) {
        backend.puts.push({ key, options: { ...options } });
        if (options.ifHash !== undefined && backend.beforeCasPut) {
          const hook = backend.beforeCasPut;
          backend.beforeCasPut = null;
          await hook(key);
        }
        const current = backend.entries.get(key);
        if (
          (options.create && current !== undefined) ||
          (options.ifHash !== undefined &&
            !hashesEqual(current?.hash, options.ifHash))
        ) {
          backend.conflicts++;
          throw new WorkspaceSessionKvConflictError(
            current?.hash ?? testHash(0n),
          );
        }
        const entry: Entry = {
          value: new Uint8Array(value),
          hash: testHash(backend.hash++),
          mtimeNs: backend.hash * 1_000n,
          forceMetadata: false,
        };
        backend.entries.set(key, entry);
        backend.refresh();
        return { hash: new Uint8Array(entry.hash), mtimeNs: entry.mtimeNs };
      },
      async kvDelete(key, options = {}) {
        backend.deletes.push({ key, options: { ...options } });
        const current = backend.entries.get(key);
        if (
          options.ifHash !== undefined &&
          !hashesEqual(current?.hash, options.ifHash)
        ) {
          backend.conflicts++;
          throw new WorkspaceSessionKvConflictError(
            current?.hash ?? testHash(0n),
          );
        }
        backend.entries.delete(key);
        backend.refresh();
      },
      async kvFetch(key) {
        backend.fetches++;
        const entry = backend.entries.get(key);
        return entry
          ? {
              hash: new Uint8Array(entry.hash),
              value: new Uint8Array(entry.value),
            }
          : null;
      },
      async watchKv(prefix, options = {}) {
        const watch: Watch = {
          prefix,
          inlineMax: options.inlineMax ?? 0,
          mirror: { live: new Map(), snapshotDone: false },
          options,
        };
        backend.watches.add(watch);
        backend.fill(watch, true);
        return {
          namespaceHandle: BigInt(backend.watches.size),
          mirror: watch.mirror,
          close: () => backend.watches.delete(watch),
        };
      },
    };
  }

  setRaw(
    key: string,
    record: StoredWorkspaceSessionDevice,
    forceMetadata = false,
  ): void {
    this.entries.set(key, {
      value: encode(record),
      hash: testHash(this.hash++),
      mtimeNs: this.hash * 1_000n,
      forceMetadata,
    });
    this.refresh();
  }

  setBytes(key: string, value: Uint8Array): void {
    this.entries.set(key, {
      value: new Uint8Array(value),
      hash: testHash(this.hash++),
      mtimeNs: this.hash * 1_000n,
      forceMetadata: false,
    });
    this.refresh();
  }

  partialSnapshot(): void {
    for (const watch of this.watches) {
      watch.mirror.live.clear();
      watch.mirror.snapshotDone = false;
      watch.options.onUpdate?.(watch.mirror);
    }
  }

  finishSnapshot(): void {
    this.refresh();
  }

  closeWatches(error: Error): void {
    for (const watch of [...this.watches]) {
      this.watches.delete(watch);
      watch.options.onClosed?.(error);
    }
  }

  private refresh(): void {
    for (const watch of this.watches) this.fill(watch, true);
  }

  private fill(watch: Watch, snapshotDone: boolean): void {
    watch.mirror.live.clear();
    for (const [key, entry] of this.entries) {
      if (!key.startsWith(watch.prefix)) continue;
      watch.mirror.live.set(key, {
        hash: new Uint8Array(entry.hash),
        size: entry.value.length,
        mtimeNs: entry.mtimeNs,
        value:
          !entry.forceMetadata && entry.value.length <= watch.inlineMax
            ? new Uint8Array(entry.value)
            : null,
      });
    }
    watch.mirror.snapshotDone = snapshotDone;
    watch.options.onUpdate?.(watch.mirror);
  }
}

function uuid(n: number): string {
  return `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
}

function testHash(value: bigint): Uint8Array {
  const hash = new Uint8Array(32);
  new DataView(hash.buffer).setBigUint64(0, value, true);
  return hash;
}

function hashesEqual(
  left: Uint8Array | undefined,
  right: Uint8Array | undefined,
): boolean {
  return Boolean(
    left &&
    right &&
    left.length === right.length &&
    left.every((byte, index) => byte === right[index]),
  );
}

function encode(record: StoredWorkspaceSessionDevice): Uint8Array {
  return encoder.encode(JSON.stringify(record));
}

function stored(
  backend: Backend,
  deviceId: string,
): StoredWorkspaceSessionDevice | null {
  const entry = backend.entries.get(workspaceSessionDeviceKey(deviceId));
  return entry
    ? parseStoredWorkspaceSessionDevice(JSON.parse(decoder.decode(entry.value)))
    : null;
}

async function settle(): Promise<void> {
  for (let index = 0; index < 8; index++) await Promise.resolve();
}

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

afterEach(() => vi.useRealTimers());

describe("workspace device DTO", () => {
  it("exports a stable localStorage key, UUID helpers, bounds, and frozen defaults", () => {
    const deviceId = uuid(1);
    expect(WORKSPACE_SESSION_DEVICE_ID_STORAGE_KEY).toBe(
      "yas.workspaceSessionDeviceId",
    );
    expect(createWorkspaceSessionDeviceId(() => deviceId)).toBe(deviceId);
    const record = createDefaultStoredWorkspaceSessionDevice({
      deviceId,
      nowUnixMs: 1,
    });
    expect(record.attachedSessionIds).toEqual([]);
    expect(Object.isFrozen(record)).toBe(true);
    expect(Object.isFrozen(record.attachedSessionIds)).toBe(true);
    expect(() =>
      createDefaultStoredWorkspaceSessionDevice({
        deviceId,
        attachedSessionIds: Array.from(
          { length: WORKSPACE_SESSION_DEVICE_MAX_ATTACHED_SESSIONS + 1 },
          (_, index) => uuid(index + 100),
        ),
      }),
    ).toThrow(WorkspaceSessionValidationError);
  });
});

describe("WorkspaceSessionDeviceStore", () => {
  it("serializes delayed device mutations so an older response cannot win", async () => {
    const backend = new Backend();
    const base = backend.client();
    const firstCasApplied = deferred();
    const releaseFirstCas = deferred();
    let delayFirstCas = true;
    const kv: WorkspaceSessionKv = {
      ...base,
      async kvPut(key, value, options) {
        const result = await base.kvPut(key, value, options);
        if (options?.ifHash !== undefined && delayFirstCas) {
          delayFirstCas = false;
          firstCasApplied.resolve();
          await releaseFirstCas.promise;
        }
        return result;
      },
    };
    const deviceId = uuid(100);
    const initial = uuid(101);
    const latest = uuid(102);
    const store = new WorkspaceSessionDeviceStore(kv, deviceId, {
      now: () => 100,
    });
    await store.start();
    await store.attach(initial);
    await settle();

    const detaching = store.detach(initial);
    await firstCasApplied.promise;
    const attaching = store.attach(latest);
    await settle();
    releaseFirstCas.resolve();
    await Promise.all([detaching, attaching]);
    await settle();

    expect(stored(backend, deviceId)?.attachedSessionIds).toEqual([latest]);
    expect(store.getSnapshot().device?.attachedSessionIds).toEqual([latest]);
  });

  it("does not retain an initial claim deleted before its delayed response", async () => {
    const backend = new Backend();
    const base = backend.client();
    const createApplied = deferred();
    const releaseCreate = deferred();
    const kv: WorkspaceSessionKv = {
      ...base,
      async kvPut(key, value, options) {
        const result = await base.kvPut(key, value, options);
        if (options?.create) {
          createApplied.resolve();
          await releaseCreate.promise;
        }
        return result;
      },
    };
    const deviceId = uuid(103);
    const store = new WorkspaceSessionDeviceStore(kv, deviceId);
    await store.start();

    const claiming = store.claimInitialSession(uuid(104));
    await createApplied.promise;
    backend.entries.delete(workspaceSessionDeviceKey(deviceId));
    backend.finishSnapshot();
    releaseCreate.resolve();
    await claiming;
    await settle();

    expect(stored(backend, deviceId)).toBeNull();
    expect(store.getSnapshot().device).toBeNull();
  });

  it("keeps concurrent attaches bounded at the device tab limit", async () => {
    const backend = new Backend();
    const deviceId = uuid(105);
    backend.setRaw(
      workspaceSessionDeviceKey(deviceId),
      createDefaultStoredWorkspaceSessionDevice({
        deviceId,
        attachedSessionIds: Array.from(
          { length: WORKSPACE_SESSION_DEVICE_MAX_ATTACHED_SESSIONS - 1 },
          (_, index) => uuid(1_000 + index),
        ),
        nowUnixMs: 105,
      }),
    );
    const store = new WorkspaceSessionDeviceStore(backend.client(), deviceId);
    await store.start();

    const results = await Promise.allSettled([
      store.attach(uuid(2_000)),
      store.attach(uuid(2_001)),
    ]);
    await settle();

    expect(results.filter(({ status }) => status === "fulfilled")).toHaveLength(
      1,
    );
    expect(results.filter(({ status }) => status === "rejected")).toHaveLength(
      1,
    );
    expect(store.getSnapshot().device?.attachedSessionIds).toHaveLength(
      WORKSPACE_SESSION_DEVICE_MAX_ATTACHED_SESSIONS,
    );
    expect(stored(backend, deviceId)?.attachedSessionIds).toHaveLength(
      WORKSPACE_SESSION_DEVICE_MAX_ATTACHED_SESSIONS,
    );
  });

  it("starts absent without writing and preserves an intentionally empty record", async () => {
    const backend = new Backend();
    const deviceId = uuid(2);
    const store = new WorkspaceSessionDeviceStore(backend.client(), deviceId, {
      now: () => 2,
    });
    await store.start();
    expect(store.getSnapshot().device).toBeNull();
    expect(backend.puts).toHaveLength(0);

    await store.attach(uuid(20));
    await store.detach(uuid(20));
    expect(store.getSnapshot().device?.attachedSessionIds).toEqual([]);
    const claim = await store.claimInitialSession(uuid(21));
    expect(claim.claimed).toBe(false);
    expect(claim.device.attachedSessionIds).toEqual([]);
  });

  it("allows exactly one simultaneous absent-device bootstrap claim", async () => {
    const backend = new Backend();
    const deviceId = uuid(3);
    const first = new WorkspaceSessionDeviceStore(backend.client(), deviceId, {
      now: () => 3,
    });
    const second = new WorkspaceSessionDeviceStore(backend.client(), deviceId, {
      now: () => 4,
    });
    await Promise.all([first.start(), second.start()]);

    const claims = await Promise.all([
      first.claimInitialSession(uuid(30)),
      second.claimInitialSession(uuid(31)),
    ]);
    const winner = claims.find(({ claimed }) => claimed)!;
    const loser = claims.find(({ claimed }) => !claimed)!;
    expect(claims.filter(({ claimed }) => claimed)).toHaveLength(1);
    expect(loser.device.attachedSessionIds).toEqual(
      winner.device.attachedSessionIds,
    );
    expect(stored(backend, deviceId)?.attachedSessionIds).toEqual(
      winner.device.attachedSessionIds,
    );
    expect(backend.conflicts).toBeGreaterThan(0);
    expect(backend.puts.every(({ options }) => options.durable)).toBe(true);
  });

  it("rebases concurrent attaches and reorder while synchronizing both tabs", async () => {
    const backend = new Backend();
    const deviceId = uuid(4);
    const first = new WorkspaceSessionDeviceStore(backend.client(), deviceId, {
      now: () => 10,
    });
    const second = new WorkspaceSessionDeviceStore(backend.client(), deviceId, {
      now: () => 11,
    });
    await Promise.all([first.start(), second.start()]);
    await first.attach(uuid(40));
    await Promise.all([first.attach(uuid(41)), second.attach(uuid(42))]);
    await Promise.all([
      first.reorder([uuid(42), uuid(40)]),
      second.attach(uuid(43)),
    ]);
    await settle();

    expect(stored(backend, deviceId)?.attachedSessionIds).toEqual([
      uuid(42),
      uuid(40),
      uuid(41),
      uuid(43),
    ]);
    expect(first.getSnapshot().device?.attachedSessionIds).toEqual(
      second.getSnapshot().device?.attachedSessionIds,
    );

    await Promise.all([second.detach(uuid(40)), first.attach(uuid(44))]);
    await settle();
    expect(first.getSnapshot().device?.attachedSessionIds).not.toContain(
      uuid(40),
    );
    expect(first.getSnapshot().device?.attachedSessionIds).toContain(uuid(44));
  });

  it("prunes confirmed stale IDs but aborts safely on a concurrent attachment", async () => {
    const backend = new Backend();
    const deviceId = uuid(5);
    const first = new WorkspaceSessionDeviceStore(backend.client(), deviceId, {
      now: () => 20,
    });
    const second = new WorkspaceSessionDeviceStore(backend.client(), deviceId, {
      now: () => 21,
    });
    await Promise.all([first.start(), second.start()]);
    await first.attach(uuid(50));
    await first.attach(uuid(51));
    await first.attach(uuid(52));
    await first.pruneDeleted([uuid(50), uuid(52)]);
    expect(stored(backend, deviceId)?.attachedSessionIds).toEqual([
      uuid(50),
      uuid(52),
    ]);

    await first.attach(uuid(54));
    backend.setBytes(workspaceSessionKey(uuid(54)), encoder.encode("exists"));
    await first.pruneDeleted([uuid(50), uuid(52)]);
    expect(stored(backend, deviceId)?.attachedSessionIds).toContain(uuid(54));

    await first.attach(uuid(51));
    backend.beforeCasPut = async () => {
      await second.attach(uuid(53));
    };
    const result = await first.pruneDeleted([uuid(50), uuid(52)]);
    expect(result?.attachedSessionIds).toEqual([
      uuid(50),
      uuid(52),
      uuid(54),
      uuid(51),
      uuid(53),
    ]);
  });

  it("keeps the last complete device state during a partial reconnect snapshot", async () => {
    const backend = new Backend();
    const deviceId = uuid(6);
    backend.setRaw(
      workspaceSessionDeviceKey(deviceId),
      createDefaultStoredWorkspaceSessionDevice({
        deviceId,
        attachedSessionIds: [uuid(60)],
        nowUnixMs: 30,
      }),
    );
    const store = new WorkspaceSessionDeviceStore(backend.client(), deviceId);
    await store.start();
    backend.partialSnapshot();
    await settle();
    expect(store.getSnapshot().device?.attachedSessionIds).toEqual([uuid(60)]);
    backend.finishSnapshot();
  });

  it("fetches metadata-only values and bounds exact-prefix suffix quarantine", async () => {
    const backend = new Backend();
    const deviceId = uuid(7);
    const key = workspaceSessionDeviceKey(deviceId);
    backend.setRaw(
      key,
      createDefaultStoredWorkspaceSessionDevice({
        deviceId,
        attachedSessionIds: [uuid(70)],
        nowUnixMs: 40,
      }),
      true,
    );
    for (let index = 0; index < 40; index++)
      backend.setBytes(`${key}/suffix-${index}`, encoder.encode("null"));
    const store = new WorkspaceSessionDeviceStore(backend.client(), deviceId);
    await store.start();
    expect(backend.fetches).toBeGreaterThan(0);
    expect(store.getSnapshot().device?.attachedSessionIds).toEqual([uuid(70)]);
    expect(store.getSnapshot().invalidRecords).toHaveLength(
      WORKSPACE_SESSION_DEVICE_MAX_QUARANTINED_KEYS,
    );
    expect(store.getSnapshot().error).toBeInstanceOf(
      WorkspaceSessionValidationError,
    );
    expect([...backend.watches][0]?.prefix).toBe(
      `${WORKSPACE_SESSION_DEVICE_KEY_PREFIX}${deviceId}`,
    );
  });

  it("surfaces a raw-YAS first failure and recovers with a fresh adapter", async () => {
    vi.useFakeTimers();
    const backend = new Backend();
    const good = backend.client();
    let factories = 0;
    const store = new WorkspaceSessionDeviceStore(
      {} as YasConnection,
      uuid(8),
      {
        yasKvFactory: () => {
          factories++;
          return {
            ...good,
            watchKv:
              factories === 1
                ? async () => {
                    throw new Error("KV family unavailable");
                  }
                : good.watchKv,
            dispose: () => undefined,
          };
        },
      },
    );
    await expect(store.start()).rejects.toThrow("KV family unavailable");
    const retry = store.start();
    await vi.advanceTimersByTimeAsync(100);
    await retry;
    expect(factories).toBe(2);
    expect(store.getSnapshot().status).toBe("ready");
  });

  it("reopens its exact watch and atomically replaces state after link loss", async () => {
    vi.useFakeTimers();
    const backend = new Backend();
    const good = backend.client();
    const deviceId = uuid(9);
    let factories = 0;
    let disposals = 0;
    const store = new WorkspaceSessionDeviceStore(
      {} as YasConnection,
      deviceId,
      {
        yasKvFactory: () => {
          factories++;
          return { ...good, dispose: () => disposals++ };
        },
      },
    );
    await store.start();
    await store.attach(uuid(90));
    await settle();

    backend.closeWatches(new Error("link lost"));
    expect(store.getSnapshot().status).toBe("loading");
    backend.setRaw(
      workspaceSessionDeviceKey(deviceId),
      createDefaultStoredWorkspaceSessionDevice({
        deviceId,
        attachedSessionIds: [uuid(91)],
        nowUnixMs: 91,
      }),
    );
    await vi.advanceTimersByTimeAsync(100);
    await settle();

    expect(factories).toBe(2);
    expect(disposals).toBe(1);
    expect(store.getSnapshot().status).toBe("ready");
    expect(store.getSnapshot().device?.attachedSessionIds).toEqual([uuid(91)]);
    expect([...backend.watches][0]?.prefix).toBe(
      workspaceSessionDeviceKey(deviceId),
    );
  });
});
