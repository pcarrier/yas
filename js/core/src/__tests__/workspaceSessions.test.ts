import { afterEach, describe, expect, it, vi } from "vitest";

import { LAYOUT_DSL_MAX_DEPTH, LAYOUT_DSL_MAX_PANES } from "../layout/dsl";
import {
  WORKSPACE_SESSION_KEY_PREFIX,
  WORKSPACE_SESSION_MAX_CATALOG_ENTRIES,
  WORKSPACE_SESSION_MAX_DOCUMENT_BYTES,
  WORKSPACE_SESSION_MAX_RETAINED_BYTES,
  WorkspaceSessionNotFoundError,
  WorkspaceSessionKvConflictError,
  WorkspaceSessionStore,
  WorkspaceSessionValidationError,
  createDefaultStoredWorkspaceSession,
  parseStoredWorkspaceSession,
  type StoredWorkspaceSession,
  type WorkspaceSessionKv,
  type WorkspaceSessionKvDeleteOptions,
  type WorkspaceSessionKvEntry,
  type WorkspaceSessionKvMirror,
  type WorkspaceSessionKvPutOptions,
  type WorkspaceSessionKvWatchOptions,
  type WorkspaceSessionWorkspace,
} from "../workspaceSessions";
import type { YasConnection } from "../yas/session";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

interface BackendEntry {
  value: Uint8Array;
  hash: Uint8Array;
  mtimeNs: bigint;
  reportedSize: number;
}

interface BackendWatch {
  prefix: string;
  inlineMax: number;
  mirror: WorkspaceSessionKvMirror & {
    readonly live: Map<string, WorkspaceSessionKvEntry>;
  };
  options: WorkspaceSessionKvWatchOptions;
}

class FakeKvBackend {
  readonly entries = new Map<string, BackendEntry>();
  readonly watches = new Set<BackendWatch>();
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
  private nextHash = 1n;

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
        const entry: BackendEntry = {
          value: new Uint8Array(value),
          hash: testHash(backend.nextHash++),
          mtimeNs: backend.nextHash * 1_000n,
          reportedSize: value.length,
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
        const watch: BackendWatch = {
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

  setRaw(key: string, value: Uint8Array, reportedSize = value.length): void {
    this.entries.set(key, {
      value: new Uint8Array(value),
      hash: testHash(this.nextHash++),
      mtimeNs: this.nextHash * 1_000n,
      reportedSize,
    });
    this.refresh();
  }

  deleteRaw(key: string): void {
    this.entries.delete(key);
    this.refresh();
  }

  partialSnapshot(keys: readonly string[]): void {
    for (const watch of this.watches) {
      watch.mirror.live.clear();
      for (const key of keys) {
        const entry = this.entries.get(key);
        if (entry && key.startsWith(watch.prefix))
          watch.mirror.live.set(key, this.mirrorEntry(entry, watch.inlineMax));
      }
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

  private fill(watch: BackendWatch, snapshotDone: boolean): void {
    watch.mirror.live.clear();
    for (const [key, entry] of this.entries) {
      if (key.startsWith(watch.prefix))
        watch.mirror.live.set(key, this.mirrorEntry(entry, watch.inlineMax));
    }
    watch.mirror.snapshotDone = snapshotDone;
    watch.options.onUpdate?.(watch.mirror);
  }

  private mirrorEntry(entry: BackendEntry, inlineMax: number) {
    return {
      hash: new Uint8Array(entry.hash),
      size: entry.reportedSize,
      mtimeNs: entry.mtimeNs,
      value:
        entry.value.length <= inlineMax ? new Uint8Array(entry.value) : null,
    };
  }
}

function id(n: number): string {
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

function workspace(
  layout: WorkspaceSessionWorkspace["layout"] = null,
): WorkspaceSessionWorkspace {
  return {
    layout,
    assignments: {},
    focusedPaneId: null,
    main: null,
    panels: {
      leftOpen: false,
      previewOpen: false,
      expandedSections: [],
      project: null,
      musterExpanded: false,
      debugOpen: false,
    },
  };
}

function document(record: StoredWorkspaceSession): Uint8Array {
  return encoder.encode(JSON.stringify(record));
}

function backendRecord(
  backend: FakeKvBackend,
  sessionId: string,
): StoredWorkspaceSession | null {
  const entry = backend.entries.get(
    `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`,
  );
  return entry
    ? parseStoredWorkspaceSession(JSON.parse(decoder.decode(entry.value)))
    : null;
}

async function settle(): Promise<void> {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

afterEach(() => vi.useRealTimers());

/** A layout DSL big enough to push a record past the inline limit. Panes are
 * anonymous, so the bytes come from a tab label — the one place a layout still
 * carries a name. */
function largeDsl(): string {
  return `tabs("${"x".repeat(39_980)}": _, _)`;
}

describe("workspace DTO", () => {
  it("uses conservative defaults and rejects ambiguous or unsafe state", () => {
    const record = createDefaultStoredWorkspaceSession({
      id: id(1),
      nowUnixMs: 1,
    });
    expect(record.workspace).toEqual(workspace());
    expect(record.activeRemotes).toEqual([]);

    expect(() =>
      parseStoredWorkspaceSession({ ...record, activeRemotes: ["local"] }),
    ).toThrow(WorkspaceSessionValidationError);
    expect(() =>
      parseStoredWorkspaceSession({
        ...record,
        workspace: { ...record.workspace, layout: { name: "bad", dsl: "" } },
      }),
    ).toThrow(WorkspaceSessionValidationError);
    expect(() =>
      parseStoredWorkspaceSession({
        ...record,
        workspace: {
          ...record.workspace,
          assignments: JSON.parse('{"__proto__":"terminal:1"}'),
        },
      }),
    ).toThrow(WorkspaceSessionValidationError);
  });

  it("rejects structurally hostile layout DSL before accepting a record", () => {
    const record = createDefaultStoredWorkspaceSession({
      id: id(12),
      nowUnixMs: 1,
    });
    const wide = `line(${Array.from(
      { length: LAYOUT_DSL_MAX_PANES + 1 },
      (_, index) => `pane${index}`,
    ).join(",")})`;
    let deep = "leaf";
    for (let index = 0; index < LAYOUT_DSL_MAX_DEPTH; index++)
      deep = `line(side${index},${deep})`;

    for (const dsl of [wide, deep]) {
      expect(() =>
        parseStoredWorkspaceSession({
          ...record,
          workspace: {
            ...record.workspace,
            layout: { name: "hostile", dsl },
          },
        }),
      ).toThrow(WorkspaceSessionValidationError);
    }
  });
});

describe("WorkspaceSessionStore", () => {
  it("auto-names workspaces with the first available positive integer", async () => {
    const backend = new FakeKvBackend();
    let sequence = 9_000;
    const store = new WorkspaceSessionStore(backend.client(), {
      randomUUID: () => id(sequence++),
    });
    await store.start();

    const first = await store.create();
    const custom = await store.create({ name: "project" });
    const second = await store.create();
    expect([first.name, custom.name, second.name]).toEqual([
      "1",
      "project",
      "2",
    ]);

    await store.delete(first.id);
    expect((await store.create()).name).toBe("1");
  });

  it("serializes a delayed update with a following delete", async () => {
    vi.useFakeTimers();
    const backend = new FakeKvBackend();
    const base = backend.client();
    const firstCasApplied = deferred();
    let delayFirstCas = true;
    const kv: WorkspaceSessionKv = {
      ...base,
      async kvPut(key, value, options) {
        const result = await base.kvPut(key, value, options);
        if (options?.ifHash !== undefined && delayFirstCas) {
          delayFirstCas = false;
          firstCasApplied.resolve();
          await new Promise<void>((resolve) => setTimeout(resolve, 1));
        }
        return result;
      },
    };
    const sessionId = id(8_000);
    const store = new WorkspaceSessionStore(kv, {
      randomUUID: () => sessionId,
      now: () => 8_000,
    });
    await store.start();
    await store.create();

    const updating = store.rename(sessionId, "Updated before deletion");
    await firstCasApplied.promise;
    const deleting = store.delete(sessionId);
    await settle();
    await vi.runAllTimersAsync();
    await Promise.all([updating, deleting]);
    await settle();

    expect(backendRecord(backend, sessionId)).toBeNull();
    expect(store.get(sessionId)).toBeNull();
    expect(store.getPresence(sessionId)).toBe("absent");
  });

  it("does not retain a create deleted before its delayed response", async () => {
    const backend = new FakeKvBackend();
    const base = backend.client();
    const createApplied = deferred();
    const releaseCreate = deferred();
    const sessionId = id(8_001);
    const key = `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`;
    const kv: WorkspaceSessionKv = {
      ...base,
      async kvPut(keyValue, value, options) {
        const result = await base.kvPut(keyValue, value, options);
        if (options?.create) {
          createApplied.resolve();
          await releaseCreate.promise;
        }
        return result;
      },
    };
    const store = new WorkspaceSessionStore(kv);
    await store.start();

    const creating = store.create({ id: sessionId });
    await createApplied.promise;
    backend.deleteRaw(key);
    releaseCreate.resolve();
    await creating;
    await settle();

    expect(backendRecord(backend, sessionId)).toBeNull();
    expect(store.get(sessionId)).toBeNull();
    expect(store.getPresence(sessionId)).toBe("absent");
  });

  it("does not re-add a pending create beyond a full reconciled catalogue", async () => {
    const backend = new FakeKvBackend();
    const base = backend.client();
    const createApplied = deferred();
    const releaseCreate = deferred();
    const sessionId = "ffffffff-ffff-4fff-8fff-ffffffffffff";
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
    const store = new WorkspaceSessionStore(kv);
    await store.start();

    const creating = store.create({ id: sessionId });
    await createApplied.promise;
    for (
      let index = 1;
      index <= WORKSPACE_SESSION_MAX_CATALOG_ENTRIES;
      index++
    ) {
      const peerId = id(index);
      const value = document(
        createDefaultStoredWorkspaceSession({
          id: peerId,
          nowUnixMs: index,
        }),
      );
      backend.entries.set(`${WORKSPACE_SESSION_KEY_PREFIX}${peerId}`, {
        value,
        hash: testHash(BigInt(10_000 + index)),
        mtimeNs: BigInt(index) * 1_000n,
        reportedSize: value.length,
      });
    }
    backend.finishSnapshot();
    releaseCreate.resolve();
    await creating;
    const barrier = await store.attach(id(1));
    barrier.detach();

    expect(store.getSnapshot().sessions).toHaveLength(
      WORKSPACE_SESSION_MAX_CATALOG_ENTRIES,
    );
    expect(store.get(sessionId)).toBeNull();
    expect(store.getPresence(sessionId)).toBe("quarantined");
  });

  it("coalesces watch bursts behind one delayed durable mutation", async () => {
    const backend = new FakeKvBackend();
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
    const sessionId = id(8_004);
    const key = `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`;
    const store = new WorkspaceSessionStore(kv, {
      randomUUID: () => sessionId,
      now: () => 8_004,
    });
    await store.start();
    await store.create();
    await settle();

    const updating = store.rename(sessionId, "Delayed local update");
    await firstCasApplied.promise;
    backend.fetches = 0;
    for (let index = 1; index <= 64; index++) {
      backend.setRaw(
        key,
        document(
          createDefaultStoredWorkspaceSession({
            id: sessionId,
            name: `External ${index}`,
            nowUnixMs: 8_004 + index,
            workspace: workspace({
              name: "metadata-only",
              dsl: largeDsl(),
            }),
          }),
        ),
      );
    }
    releaseFirstCas.resolve();
    await updating;
    await vi.waitFor(() =>
      expect(store.get(sessionId)?.name).toBe("External 64"),
    );
    expect(backend.fetches).toBe(1);
  });

  it("serializes concurrent creates at the catalogue boundary", async () => {
    const backend = new FakeKvBackend();
    for (
      let index = 0;
      index < WORKSPACE_SESSION_MAX_CATALOG_ENTRIES - 1;
      index++
    ) {
      backend.setRaw(
        `${WORKSPACE_SESSION_KEY_PREFIX}invalid-${index}`,
        encoder.encode("null"),
      );
    }
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();

    const results = await Promise.allSettled([
      store.create({ id: id(8_002) }),
      store.create({ id: id(8_003) }),
    ]);
    await settle();

    expect(results.filter(({ status }) => status === "fulfilled")).toHaveLength(
      1,
    );
    expect(results.filter(({ status }) => status === "rejected")).toHaveLength(
      1,
    );
    expect(
      store.getSnapshot().invalidRecords.length +
        store.getSnapshot().sessions.length,
    ).toBe(WORKSPACE_SESSION_MAX_CATALOG_ENTRIES);
    expect(
      [...backend.entries.keys()].filter((key) =>
        /^ui\/workspace-sessions\/v1\/[0-9a-f-]{36}$/.test(key),
      ),
    ).toHaveLength(1);
  });

  it("merges a concurrent rename with a semantic layout/panel/remote patch", async () => {
    const backend = new FakeKvBackend();
    let clock = 10;
    const first = new WorkspaceSessionStore(backend.client(), {
      now: () => ++clock,
      randomUUID: () => id(1),
    });
    const second = new WorkspaceSessionStore(backend.client(), {
      now: () => ++clock,
    });
    await Promise.all([first.start(), second.start()]);
    const created = await first.create({
      name: "Before",
      workspace: workspace(),
    });
    await settle();

    await Promise.all([
      first.rename(created.id, "Renamed"),
      second.update(created.id, {
        activeRemotes: ["build"],
        workspace: {
          layout: { name: "split", dsl: "line(_, _)" },
          panels: { leftOpen: true },
        },
      }),
    ]);

    const final = backendRecord(backend, created.id)!;
    expect(backend.conflicts).toBeGreaterThan(0);
    expect(final.name).toBe("Renamed");
    expect(final.activeRemotes).toEqual(["build"]);
    expect(final.workspace.layout?.name).toBe("split");
    expect(final.workspace.panels.leftOpen).toBe(true);
    expect(backend.puts.every(({ options }) => options.durable)).toBe(true);
  });

  it("rebases individual remote membership changes across CAS conflicts", async () => {
    const backend = new FakeKvBackend();
    let clock = 100;
    const first = new WorkspaceSessionStore(backend.client(), {
      now: () => ++clock,
      randomUUID: () => id(11),
    });
    const second = new WorkspaceSessionStore(backend.client(), {
      now: () => ++clock,
    });
    await Promise.all([first.start(), second.start()]);
    const created = await first.create();

    const updates = await Promise.all([
      first.setRemoteActive(created.id, "build", true),
      second.setRemoteActive(created.id, "prod", true),
    ]);

    expect(backend.conflicts).toBeGreaterThan(0);
    expect(Object.isFrozen(updates[0])).toBe(true);
    expect(Object.isFrozen(updates[0].activeRemotes)).toBe(true);
    expect(Object.isFrozen(updates[0].workspace)).toBe(true);
    expect(new Set(backendRecord(backend, created.id)!.activeRemotes)).toEqual(
      new Set(["build", "prod"]),
    );

    const attachment = await first.attach(created.id);
    await Promise.all([
      attachment.setRemoteActive("build", false),
      second.setRemoteActive(created.id, "test", true),
    ]);

    expect(new Set(backendRecord(backend, created.id)!.activeRemotes)).toEqual(
      new Set(["prod", "test"]),
    );
    expect(backend.puts.every(({ options }) => options.durable)).toBe(true);
  });

  it("lets a concurrent delete win instead of recreating during update retry", async () => {
    const backend = new FakeKvBackend();
    const writerKv = backend.client();
    const deleterKv = backend.client();
    const writer = new WorkspaceSessionStore(writerKv, {
      now: () => 20,
      randomUUID: () => id(2),
    });
    await writer.start();
    const created = await writer.create({ name: "Soon gone" });
    backend.beforeCasPut = async (key) => {
      const current = await deleterKv.kvFetch(key);
      await deleterKv.kvDelete(key, {
        ifHash: current!.hash,
        durable: true,
      });
    };

    await expect(
      writer.rename(created.id, "Must not return"),
    ).rejects.toBeInstanceOf(WorkspaceSessionNotFoundError);
    expect(backendRecord(backend, created.id)).toBeNull();
    expect(backend.deletes.at(-1)?.options.durable).toBe(true);
  });

  it("keeps the last complete catalogue through a partial recovery snapshot", async () => {
    const backend = new FakeKvBackend();
    const sessionId = id(3);
    const initial = createDefaultStoredWorkspaceSession({
      id: sessionId,
      name: "Kept",
      nowUnixMs: 30,
    });
    backend.setRaw(
      `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`,
      document(initial),
    );
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();
    const attachment = await store.attach(sessionId);

    backend.partialSnapshot([]);
    await settle();
    expect(store.getSnapshot().sessions.map(({ id }) => id)).toEqual([
      sessionId,
    ]);
    expect(attachment.getSnapshot()?.name).toBe("Kept");

    backend.finishSnapshot();
    await settle();
    expect(attachment.getSnapshot()?.name).toBe("Kept");
  });

  it("fetches non-inline records and quarantines bad peers without losing valid ones", async () => {
    const backend = new FakeKvBackend();
    const validId = id(4);
    const badId = id(5);
    const large = createDefaultStoredWorkspaceSession({
      id: validId,
      name: "Large",
      nowUnixMs: 40,
      workspace: workspace({ name: "large", dsl: largeDsl() }),
    });
    backend.setRaw(
      `${WORKSPACE_SESSION_KEY_PREFIX}${validId}`,
      document(large),
    );
    backend.setRaw(
      `${WORKSPACE_SESSION_KEY_PREFIX}${badId}`,
      document({ ...large, name: "Wrong id" }),
    );
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();

    expect(backend.fetches).toBeGreaterThan(0);
    expect(store.getSnapshot().sessions.map(({ id }) => id)).toEqual([validId]);
    expect(store.getSnapshot().invalidKeys).toEqual([
      `${WORKSPACE_SESSION_KEY_PREFIX}${badId}`,
    ]);
    expect(store.getSnapshot().error).toBeInstanceOf(
      WorkspaceSessionValidationError,
    );
  });

  it("reflects external create, rename, and delete through the one prefix watch", async () => {
    const backend = new FakeKvBackend();
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();
    const sessionId = id(6);
    const created = createDefaultStoredWorkspaceSession({
      id: sessionId,
      name: "External",
      nowUnixMs: 60,
    });
    const key = `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`;

    backend.setRaw(key, document(created));
    await settle();
    expect(store.get(sessionId)?.name).toBe("External");
    const attachment = await store.attach(sessionId);

    backend.setRaw(
      key,
      document({ ...created, name: "Elsewhere", updatedAtUnixMs: 61 }),
    );
    await settle();
    expect(attachment.getSnapshot()?.name).toBe("Elsewhere");

    backend.deleteRaw(key);
    await settle();
    expect(store.get(sessionId)).toBeNull();
    expect(attachment.getSnapshot()).toBeNull();
  });

  it("keeps the last-good attachment while an exact record is quarantined", async () => {
    const backend = new FakeKvBackend();
    const sessionId = id(7);
    const key = `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`;
    const initial = createDefaultStoredWorkspaceSession({
      id: sessionId,
      name: "Last good",
      nowUnixMs: 70,
    });
    backend.setRaw(key, document(initial));
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();
    const attachment = await store.attach(sessionId);

    backend.setRaw(key, encoder.encode("{"));
    await settle();
    expect(store.getPresence(sessionId)).toBe("quarantined");
    expect(store.getSnapshot().quarantinedSessionIds).toEqual([sessionId]);
    expect(Object.isFrozen(store.getSnapshot().quarantinedSessionIds)).toBe(
      true,
    );
    expect(store.get(sessionId)?.name).toBe("Last good");
    expect(attachment.getSnapshot()?.name).toBe("Last good");
    const secondAttachment = await store.attach(sessionId);
    expect(secondAttachment.getSnapshot()?.name).toBe("Last good");
    secondAttachment.detach();

    backend.setRaw(
      key,
      document({ ...initial, name: "Repaired", updatedAtUnixMs: 71 }),
    );
    await settle();
    expect(store.getPresence(sessionId)).toBe("available");
    expect(store.getSnapshot().quarantinedSessionIds).toEqual([]);
    expect(attachment.getSnapshot()?.name).toBe("Repaired");

    backend.deleteRaw(key);
    await settle();
    expect(store.getPresence(sessionId)).toBe("absent");
    expect(attachment.getSnapshot()).toBeNull();
  });

  it("does not let direct attach bypass the catalogue entry cap", async () => {
    const backend = new FakeKvBackend();
    for (
      let index = 0;
      index < WORKSPACE_SESSION_MAX_CATALOG_ENTRIES;
      index++
    ) {
      backend.setRaw(
        `${WORKSPACE_SESSION_KEY_PREFIX}0-invalid-${String(index).padStart(4, "0")}`,
        encoder.encode("null"),
      );
    }
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();
    const quarantinedBefore = store.getSnapshot().invalidKeys;
    const sessionId = "ffffffff-ffff-4fff-8fff-ffffffffffff";
    backend.setRaw(
      `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`,
      document(
        createDefaultStoredWorkspaceSession({
          id: sessionId,
          nowUnixMs: 80,
        }),
      ),
    );

    await expect(store.attach(sessionId)).rejects.toThrow("catalogue limit");
    await expect(store.rename(sessionId, "Must not install")).rejects.toThrow(
      "catalogue limit",
    );
    expect(store.get(sessionId)).toBeNull();
    // Exact presence remains queryable even when the bounded warning list is
    // already saturated by hostile prefix keys.
    expect(store.getPresence(sessionId)).toBe("quarantined");
    expect(store.getSnapshot().invalidRecords).toHaveLength(
      WORKSPACE_SESSION_MAX_CATALOG_ENTRIES,
    );
    expect(store.getSnapshot().invalidKeys).toEqual(quarantinedBefore);
    expect(store.getSnapshot().sessions).toHaveLength(0);
    expect(backendRecord(backend, sessionId)?.name).toBe("1");
    expect(backend.puts).toHaveLength(0);
  });

  it("does not let direct attach bypass the aggregate retained-byte cap", async () => {
    const backend = new FakeKvBackend();
    const reportedSize =
      (WORKSPACE_SESSION_MAX_RETAINED_BYTES / 16 - 4_096) / 4;
    for (let index = 0; index < 16; index++) {
      const sessionId = id(1_000 + index);
      backend.setRaw(
        `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`,
        document(
          createDefaultStoredWorkspaceSession({
            id: sessionId,
            nowUnixMs: 90 + index,
          }),
        ),
        reportedSize,
      );
    }
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();
    expect(store.getSnapshot().sessions).toHaveLength(16);
    const beforeIds = store.getSnapshot().sessions.map(({ id }) => id);

    const sessionId = id(2_000);
    backend.setRaw(
      `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`,
      document(
        createDefaultStoredWorkspaceSession({
          id: sessionId,
          nowUnixMs: 110,
        }),
      ),
    );
    await expect(store.attach(sessionId)).rejects.toThrow(
      "retained-byte limit",
    );
    await expect(store.rename(sessionId, "Must not install")).rejects.toThrow(
      "retained-byte limit",
    );
    expect(store.get(sessionId)).toBeNull();
    expect(store.getPresence(sessionId)).toBe("quarantined");
    expect(store.getSnapshot().sessions.map(({ id }) => id)).toEqual(beforeIds);
    expect(backendRecord(backend, sessionId)?.name).toBe("1");
    expect(backend.puts).toHaveLength(0);
  });

  it("does not let direct attach undercount an oversized mirror record", async () => {
    const backend = new FakeKvBackend();
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();
    const sessionId = id(3_000);
    backend.setRaw(
      `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`,
      document(
        createDefaultStoredWorkspaceSession({
          id: sessionId,
          nowUnixMs: 120,
        }),
      ),
      WORKSPACE_SESSION_MAX_DOCUMENT_BYTES + 1,
    );

    await expect(store.attach(sessionId)).rejects.toThrow(
      "document exceeds its byte limit",
    );
    expect(store.get(sessionId)).toBeNull();
    expect(store.getPresence(sessionId)).toBe("quarantined");
    expect(store.getSnapshot().sessions).toHaveLength(0);
  });

  it("bounds aggregate retained catalogue bytes before fetching every value", async () => {
    const backend = new FakeKvBackend();
    for (let index = 1; index <= 17; index++) {
      const sessionId = id(100 + index);
      const record = createDefaultStoredWorkspaceSession({
        id: sessionId,
        name: `Session ${index}`,
        nowUnixMs: index,
      });
      backend.setRaw(
        `${WORKSPACE_SESSION_KEY_PREFIX}${sessionId}`,
        document(record),
        WORKSPACE_SESSION_MAX_DOCUMENT_BYTES,
      );
    }
    const store = new WorkspaceSessionStore(backend.client());
    await store.start();

    expect(store.getSnapshot().sessions.length).toBeLessThan(17);
    expect(
      store
        .getSnapshot()
        .invalidRecords.some(({ message }) =>
          message.includes("retained-byte limit"),
        ),
    ).toBe(true);
    expect(backend.fetches).toBeLessThan(17);
  });

  it("counts invalid prefix keys against create capacity", async () => {
    const backend = new FakeKvBackend();
    for (
      let index = 0;
      index < WORKSPACE_SESSION_MAX_CATALOG_ENTRIES;
      index++
    ) {
      backend.setRaw(
        `${WORKSPACE_SESSION_KEY_PREFIX}invalid-${index}`,
        encoder.encode("null"),
      );
    }
    const store = new WorkspaceSessionStore(backend.client(), {
      randomUUID: () => id(9),
    });
    await store.start();
    await expect(store.create()).rejects.toThrow("catalogue limit");
  });

  it("detach is idempotent and performs no backend mutation", async () => {
    const backend = new FakeKvBackend();
    const store = new WorkspaceSessionStore(backend.client(), {
      randomUUID: () => id(10),
    });
    await store.start();
    const record = await store.create();
    const attachment = await store.attach(record.id);
    const mutations = backend.puts.length + backend.deletes.length;
    attachment.detach();
    attachment.detach();
    expect(attachment.getSnapshot()).toBeNull();
    expect(backend.puts.length + backend.deletes.length).toBe(mutations);
  });

  it("rejects an unavailable raw-YAS first open, then recovers with a fresh adapter", async () => {
    vi.useFakeTimers();
    const backend = new FakeKvBackend();
    const good = backend.client();
    let factories = 0;
    const store = new WorkspaceSessionStore({} as YasConnection, {
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
    });

    await expect(store.start()).rejects.toThrow("KV family unavailable");
    expect(store.getSnapshot().status).toBe("loading");
    const retry = store.start();
    await vi.advanceTimersByTimeAsync(100);
    await retry;
    expect(factories).toBe(2);
    expect(store.getSnapshot().status).toBe("ready");
  });
});
