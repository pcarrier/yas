import { describe, expect, it, vi } from "vitest";

import {
  YAS_FAMILY_KV,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YasNativeWorkspaceKv,
  type YasConnection,
  type YasKvNamespace,
  type YasKvStateUpdate,
} from "../yas";

const encoder = new TextEncoder();

function hash(seed: number): Uint8Array {
  return Uint8Array.from({ length: 32 }, (_, index) => (seed + index) & 0xff);
}

function connection(
  connect: () => Promise<unknown> = async () => undefined,
): YasConnection {
  return {
    connect: vi.fn(connect),
    onInvalidation: vi.fn(() => () => undefined),
  } as unknown as YasConnection;
}

describe("YasNativeWorkspaceKv", () => {
  it("waits for HELLO before opening the KV family", async () => {
    let finishHello!: () => void;
    const hello = new Promise<void>((resolve) => {
      finishHello = resolve;
    });
    const namespace = {
      handle: 1n,
      watch: vi.fn(async () => ({
        active: true,
        unwatch: vi.fn(async () => undefined),
      })),
      close: vi.fn(async () => undefined),
    } as unknown as YasKvNamespace;
    const client = { open: vi.fn(async () => namespace) };
    const gatedConnection = connection(() => hello);
    const kv = new YasNativeWorkspaceKv(gatedConnection, { client });

    const pending = kv.watchKv("ui/");
    await Promise.resolve();

    expect(gatedConnection.connect).toHaveBeenCalledOnce();
    expect(client.open).not.toHaveBeenCalled();

    finishHello();
    const watch = await pending;
    expect(client.open).toHaveBeenCalledOnce();
    expect(namespace.watch).toHaveBeenCalledOnce();

    watch.close();
    kv.dispose();
  });

  it("uses the exact native 32-byte hash as its CAS precondition", async () => {
    const current = hash(7);
    const next = hash(19);
    const put = vi.fn(async (..._args: Parameters<YasKvNamespace["put"]>) => ({
      status: 0,
      modificationRevision: 3n,
      modifiedUnixNs: 4n,
      contentHash: next,
      byteLength: 5n,
      extensions: [],
    }));
    const namespace = {
      handle: 1n,
      put,
      close: vi.fn(async () => undefined),
    } as unknown as YasKvNamespace;
    const client = { open: vi.fn(async () => namespace) };
    const kv = new YasNativeWorkspaceKv(connection(), { client });

    const result = await kv.kvPut("session", new Uint8Array([1, 2, 3]), {
      ifHash: current,
      durable: true,
    });

    expect(put).toHaveBeenCalledOnce();
    expect(put.mock.calls[0]?.[2]).toMatchObject({
      durable: true,
      precondition: { type: "hash", contentHash: current },
    });
    expect(result.hash).toEqual(next);
    expect(result.hash).not.toBe(next);
  });

  it("retains the opaque namespace handle and full hashes in watch state", async () => {
    const namespaceHandle = 0xffff_ffff_ffff_fffen;
    let deliver!: (update: YasKvStateUpdate) => void;
    const namespace = {
      handle: namespaceHandle,
      watch: vi.fn(async (listener: (update: YasKvStateUpdate) => void) => {
        deliver = listener;
        return { active: true, unwatch: vi.fn(async () => undefined) };
      }),
      close: vi.fn(async () => undefined),
    } as unknown as YasKvNamespace;
    const client = { open: vi.fn(async () => namespace) };
    const kv = new YasNativeWorkspaceKv(connection(), { client });
    const onUpdate = vi.fn();

    const watched = await kv.watchKv("ui/", { onUpdate });
    const contentHash = hash(41);
    deliver({
      phase: YAS_STATE_SNAPSHOT_BEGIN,
      fromRevision: 0n,
      toRevision: 0n,
      changes: [],
    });
    deliver({
      phase: YAS_STATE_SNAPSHOT_END,
      fromRevision: 0n,
      toRevision: 1n,
      changes: [
        {
          type: "add",
          entry: {
            relativeKey: encoder.encode("session"),
            contentHash,
            byteLength: 3n,
            modificationRevision: 1n,
            modifiedUnixNs: 2n,
            inlineValue: new Uint8Array([1, 2, 3]),
            extensions: [],
          },
        },
      ],
    });

    expect(watched.namespaceHandle).toBe(namespaceHandle);
    expect(typeof watched.namespaceHandle).toBe("bigint");
    expect(watched.mirror.snapshotDone).toBe(true);
    expect(watched.mirror.live.get("ui/session")).toMatchObject({
      hash: contentHash,
      size: 3,
      mtimeNs: 2n,
      value: new Uint8Array([1, 2, 3]),
    });
    expect(watched.mirror.live.get("ui/session")?.hash).not.toBe(contentHash);
    expect(onUpdate).toHaveBeenCalledTimes(2);
  });

  it("resets one invalidated generation and becomes available again", async () => {
    let invalidate!: (event: { family?: number; error: Error }) => void;
    const reusableConnection = {
      connect: vi.fn(async () => undefined),
      onInvalidation: vi.fn(
        (listener: (event: { family?: number; error: Error }) => void) => {
          invalidate = listener;
          return () => undefined;
        },
      ),
    } as unknown as YasConnection;
    const first = {
      handle: 11n,
      watch: vi.fn(async () => ({
        active: true,
        unwatch: vi.fn(async () => undefined),
      })),
      close: vi.fn(async () => undefined),
    } as unknown as YasKvNamespace;
    const second = {
      handle: 12n,
      watch: vi.fn(async () => ({
        active: true,
        unwatch: vi.fn(async () => undefined),
      })),
      close: vi.fn(async () => undefined),
    } as unknown as YasKvNamespace;
    const client = {
      open: vi
        .fn<() => Promise<YasKvNamespace>>()
        .mockResolvedValueOnce(first)
        .mockResolvedValueOnce(second),
    };
    const onClosed = vi.fn();
    const kv = new YasNativeWorkspaceKv(reusableConnection, { client });

    const initial = await kv.watchKv("ui/", { onClosed });
    invalidate({ family: YAS_FAMILY_KV, error: new Error("unavailable") });
    await Promise.resolve();

    expect(first.close).toHaveBeenCalledOnce();
    expect(onClosed).toHaveBeenCalledOnce();
    expect(initial.mirror.live.size).toBe(0);

    const recovered = await kv.watchKv("ui/");
    expect(recovered.namespaceHandle).toBe(12n);
    expect(client.open).toHaveBeenCalledTimes(2);
    recovered.close();
    kv.dispose();
  });

  it("self-closes a namespace whose WATCH resolves after invalidation", async () => {
    let invalidate!: (event: { family?: number; error: Error }) => void;
    let finishWatch!: () => void;
    const pendingWatch = new Promise<void>((resolve) => {
      finishWatch = resolve;
    });
    const reusableConnection = {
      connect: vi.fn(async () => undefined),
      onInvalidation: vi.fn(
        (listener: (event: { family?: number; error: Error }) => void) => {
          invalidate = listener;
          return () => undefined;
        },
      ),
    } as unknown as YasConnection;
    const namespace = {
      handle: 21n,
      watch: vi.fn(() => pendingWatch),
      close: vi.fn(async () => undefined),
    } as unknown as YasKvNamespace;
    const kv = new YasNativeWorkspaceKv(reusableConnection, {
      client: { open: vi.fn(async () => namespace) },
    });

    const pending = kv.watchKv("ui/");
    await vi.waitFor(() => expect(namespace.watch).toHaveBeenCalledOnce());
    invalidate({ family: YAS_FAMILY_KV, error: new Error("reconfigured") });
    finishWatch();

    await expect(pending).rejects.toThrow(/WATCH was pending/);
    expect(namespace.close).toHaveBeenCalledOnce();
    kv.dispose();
  });

  it("keeps permanent disposal terminal", async () => {
    const kv = new YasNativeWorkspaceKv(connection(), {
      client: { open: vi.fn() },
    });
    kv.dispose();

    await expect(kv.kvFetch("session")).rejects.toThrow(
      /native Workspace KV is closed/,
    );
  });

  it("closes every watched namespace despite a throwing onClosed callback", async () => {
    vi.stubGlobal("reportError", vi.fn());
    let invalidate!: (event: { family?: number; error: Error }) => void;
    const invalidating = {
      connect: vi.fn(async () => undefined),
      onInvalidation: vi.fn(
        (listener: (event: { family?: number; error: Error }) => void) => {
          invalidate = listener;
          return () => undefined;
        },
      ),
    } as unknown as YasConnection;
    const namespaces = [61n, 62n].map(
      (handle) =>
        ({
          handle,
          watch: vi.fn(async () => ({
            active: true,
            unwatch: vi.fn(async () => undefined),
          })),
          close: vi.fn(async () => undefined),
        }) as unknown as YasKvNamespace,
    );
    const client = {
      open: vi.fn(async () => namespaces[client.open.mock.calls.length - 1]!),
    };
    const secondClosed = vi.fn();
    const kv = new YasNativeWorkspaceKv(invalidating, { client });
    await kv.watchKv("one/", {
      onClosed: () => {
        throw new Error("first onClosed failed");
      },
    });
    await kv.watchKv("two/", { onClosed: secondClosed });

    invalidate({ family: YAS_FAMILY_KV, error: new Error("unavailable") });
    await Promise.resolve();

    expect(namespaces[0]!.close).toHaveBeenCalledOnce();
    expect(namespaces[1]!.close).toHaveBeenCalledOnce();
    expect(secondClosed).toHaveBeenCalledOnce();
    kv.dispose();
    vi.unstubAllGlobals();
  });
});
