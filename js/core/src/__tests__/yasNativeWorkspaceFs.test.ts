import { describe, expect, it, vi } from "vitest";

import type { YasConnection } from "../yas";
import {
  YAS_FS_MAX_BATCH_ITEMS,
  YAS_FS_MAX_STAGES_PER_SESSION,
  YAS_FAMILY_FS,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATUS_IO,
  YAS_STATUS_OK,
  YasNativeFsPermissionError,
  YasNativeWorkspaceFs,
  type YasFsApply,
  type YasFsRoot,
  type YasInvalidation,
  type YasStateBatch,
} from "../yas";

function connection(): YasConnection {
  return {
    onInvalidation: () => () => undefined,
  } as unknown as YasConnection;
}

function nativeRoot(
  apply: (value: Omit<YasFsApply, "rootHandle">) => Promise<unknown>,
): YasFsRoot {
  return {
    handle: 0xf000_0000_0000_0001n,
    opened: {
      rootHandle: 0xf000_0000_0000_0001n,
      rootRevision: 1n,
      pathModel: 1,
      caseBehavior: 1,
      canonicalPath: new TextEncoder().encode("/repo"),
      extensions: [],
    },
    catalog: {
      subscribeBatches: vi.fn(() => () => undefined),
      watch: vi.fn(async () => undefined),
    },
    close: vi.fn(async () => undefined),
    apply: vi.fn(apply),
  } as unknown as YasFsRoot;
}

async function openHandle(root: YasFsRoot) {
  const workspace = new YasNativeWorkspaceFs(connection(), {
    terminalHandle: () => undefined,
    client: { open: vi.fn(async () => root) },
    hashBytes: async () => new Uint8Array(32),
  });
  return workspace.syncFs("/repo");
}

describe("YasNativeWorkspaceFs mutation retention", () => {
  it("advances the public revision before update callbacks derive state", async () => {
    let deliver!: (batch: YasStateBatch) => void;
    const root = nativeRoot(async () => ({
      rootRevision: 1n,
      items: [],
      extensions: [],
    }));
    root.catalog.subscribeBatches = vi.fn((listener) => {
      deliver = listener;
      return () => undefined;
    });
    const workspace = new YasNativeWorkspaceFs(connection(), {
      terminalHandle: () => undefined,
      client: { open: vi.fn(async () => root) },
      hashBytes: async () => new Uint8Array(32),
    });
    let revisionSeen = -1;
    let handle!: Awaited<ReturnType<typeof workspace.syncFs>>;
    handle = await workspace.syncFs("/repo", {
      onUpdate: () => {
        revisionSeen = handle.revision;
      },
    });
    await Promise.resolve();

    deliver({
      phase: YAS_STATE_SNAPSHOT_BEGIN,
      flags: 0,
      fromRevision: 0n,
      toRevision: 1n,
      records: [],
    });
    deliver({
      phase: YAS_STATE_SNAPSHOT_END,
      flags: 0,
      fromRevision: 1n,
      toRevision: 1n,
      records: [],
    });

    expect(revisionSeen).toBe(1);
    expect(handle.revision).toBe(1);
    handle.stop();
    workspace.dispose();
  });

  it("evicts old self-echo hashes when watch delivery never arrives", async () => {
    let revision = 0n;
    const root = nativeRoot(async () => {
      revision++;
      const hashByte = Number(revision % 255n) + 1;
      return {
        rootRevision: revision,
        items: [
          {
            index: 0,
            status: YAS_STATUS_OK,
            entryRevision: revision,
            modifiedUnixNs: revision,
            contentHash: new Uint8Array(32).fill(hashByte),
            detail: "",
          },
        ],
        extensions: [],
      };
    });
    const handle = await openHandle(root);

    for (let index = 0; index <= YAS_FS_MAX_BATCH_ITEMS; index++)
      await handle.symlink("target", `link-${index}`);

    expect(handle.lastWrittenHash("link-0")).toBeUndefined();
    expect(handle.lastWrittenHash(`link-${YAS_FS_MAX_BATCH_ITEMS}`)).toEqual(
      new Uint8Array(32).fill(((YAS_FS_MAX_BATCH_ITEMS + 1) % 255) + 1),
    );
    handle.stop();
  });

  it("rejects a mutation before retaining beyond the native pending budget", async () => {
    const never = new Promise<never>(() => undefined);
    const root = nativeRoot(async () => never);
    const handle = await openHandle(root);
    const maximum = YAS_FS_MAX_BATCH_ITEMS + YAS_FS_MAX_STAGES_PER_SESSION;

    for (let index = 0; index < maximum; index++)
      void handle.mkdir(`pending-${index}`).catch(() => undefined);

    await expect(handle.mkdir("one-too-many")).rejects.toThrow(
      /pending-mutation budget is exhausted/,
    );
    expect(root.apply).toHaveBeenCalledTimes(maximum);
    handle.stop();
  });

  it("surfaces the native permission status as a typed error", async () => {
    const root = nativeRoot(async () => ({
      rootRevision: 1n,
      items: [
        {
          index: 0,
          status: YAS_STATUS_IO,
          entryRevision: 0n,
          modifiedUnixNs: 0n,
          detail: "filesystem operation is not permitted",
        },
      ],
      extensions: [],
    }));
    const handle = await openHandle(root);

    await expect(
      handle.symlink("target", "readonly-link"),
    ).rejects.toBeInstanceOf(YasNativeFsPermissionError);
    handle.stop();
  });

  it("closes active and short-lived roots instead of dropping local callbacks", async () => {
    const active = nativeRoot(async () => ({
      rootRevision: 1n,
      items: [],
      extensions: [],
    }));
    const workspace = new YasNativeWorkspaceFs(connection(), {
      terminalHandle: () => undefined,
      client: { open: vi.fn(async () => active) },
      hashBytes: async () => new Uint8Array(32),
    });
    await workspace.syncFs("/repo");

    workspace.dispose();
    await Promise.resolve();
    expect(active.close).toHaveBeenCalledOnce();

    const searchRoot = nativeRoot(async () => ({
      rootRevision: 1n,
      items: [],
      extensions: [],
    }));
    Object.assign(searchRoot, {
      search: vi.fn(async () => ({
        flags: 0,
        nextCursor: new Uint8Array(),
        records: async () => [],
      })),
    });
    const searches = new YasNativeWorkspaceFs(connection(), {
      terminalHandle: () => undefined,
      client: { open: vi.fn(async () => searchRoot) },
      hashBytes: async () => new Uint8Array(32),
    });

    await expect(searches.searchFiles("/repo", "needle")).resolves.toEqual([]);
    expect(searchRoot.close).toHaveBeenCalledOnce();
    searches.dispose();
  });

  it("self-closes an FS root returned after permanent disposal", async () => {
    let resolveOpen!: (root: YasFsRoot) => void;
    const pendingOpen = new Promise<YasFsRoot>((resolve) => {
      resolveOpen = resolve;
    });
    const root = nativeRoot(async () => ({
      rootRevision: 1n,
      items: [],
      extensions: [],
    }));
    const workspace = new YasNativeWorkspaceFs(connection(), {
      terminalHandle: () => undefined,
      client: { open: vi.fn(() => pendingOpen) },
      hashBytes: async () => new Uint8Array(32),
    });

    const pending = workspace.syncFs("/repo");
    workspace.dispose();
    resolveOpen(root);

    await expect(pending).rejects.toThrow(/OPEN was pending/);
    expect(root.close).toHaveBeenCalledOnce();
  });

  it("isolates throwing invalidation callbacks and closes every sibling handle", async () => {
    vi.stubGlobal("reportError", vi.fn());
    let invalidate!: (event: YasInvalidation) => void;
    const invalidating = {
      onInvalidation: vi.fn((listener: (event: YasInvalidation) => void) => {
        invalidate = listener;
        return () => undefined;
      }),
    } as unknown as YasConnection;
    const roots = [
      nativeRoot(async () => ({ items: [], extensions: [], rootRevision: 1n })),
      nativeRoot(async () => ({ items: [], extensions: [], rootRevision: 1n })),
    ];
    const secondClosed = vi.fn();
    const workspace = new YasNativeWorkspaceFs(invalidating, {
      terminalHandle: () => undefined,
      client: { open: vi.fn(async () => roots.shift()!) },
      hashBytes: async () => new Uint8Array(32),
    });
    await workspace.syncFs("/one", {
      onClosed: () => {
        throw new Error("first close callback failed");
      },
    });
    await workspace.syncFs("/two", { onClosed: secondClosed });

    invalidate({ family: YAS_FAMILY_FS, error: new Error("disconnected") });

    expect(secondClosed).toHaveBeenCalledOnce();
    workspace.dispose();
    vi.unstubAllGlobals();
  });

  it("stops an FS batch callback chain after reentrant handle closure", async () => {
    let deliver!: (batch: YasStateBatch) => void;
    const root = nativeRoot(async () => ({
      rootRevision: 1n,
      items: [],
      extensions: [],
    }));
    root.catalog.subscribeBatches = vi.fn((listener) => {
      deliver = listener;
      return () => undefined;
    });
    const onUpdate = vi.fn();
    const workspace = new YasNativeWorkspaceFs(connection(), {
      terminalHandle: () => undefined,
      client: { open: vi.fn(async () => root) },
      hashBytes: async () => new Uint8Array(32),
    });
    let handle!: Awaited<ReturnType<typeof workspace.syncFs>>;
    handle = await workspace.syncFs("/repo", {
      onSync: () => handle.stop(),
      onUpdate,
    });
    const subscriber = vi.fn();
    handle.subscribe(subscriber);
    await Promise.resolve();

    deliver({
      phase: YAS_STATE_SNAPSHOT_BEGIN,
      flags: 0,
      fromRevision: 0n,
      toRevision: 1n,
      records: [],
    });
    deliver({
      phase: YAS_STATE_SNAPSHOT_END,
      flags: 0,
      fromRevision: 1n,
      toRevision: 1n,
      records: [],
    });

    expect(onUpdate).not.toHaveBeenCalled();
    expect(subscriber).not.toHaveBeenCalled();
    await Promise.resolve();
    expect(root.close).toHaveBeenCalledOnce();
    workspace.dispose();
  });
});
