import { describe, expect, it, vi } from "vitest";

import type { YasConnection } from "../yas";
import {
  YAS_LSP_MAX_INLINE_BUFFER_BYTES,
  YAS_STATUS_OK,
  YasNativeWorkspaceLsp,
  type YasLspBufferIdentity,
  type YasLspSnapshot,
  type YasLspWorkspace,
} from "../yas";

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function identity(revision: bigint, byteLength: number): YasLspBufferIdentity {
  return {
    bufferHandle: 0x8000_0000_0000_0001n,
    bufferRevision: revision,
    workspaceRevision: revision,
    byteLength: BigInt(byteLength),
    contentHash: new Uint8Array(32).fill(Number(revision & 0xffn)),
    extensions: [],
  };
}

function connection(): YasConnection {
  return {
    onInvalidation: () => () => undefined,
  } as unknown as YasConnection;
}

function nativeWorkspace(
  overrides: Partial<YasLspWorkspace> = {},
): YasLspWorkspace {
  return {
    handle: 0xf000_0000_0000_0001n,
    opened: {
      workspaceHandle: 0xf000_0000_0000_0001n,
      workspaceRevision: 1n,
      positionEncoding: 1,
      backendCount: 1,
      capabilities: 0n,
      canonicalRoot: new TextEncoder().encode("/repo"),
      extensions: [],
    },
    onClosed: vi.fn(() => () => undefined),
    close: vi.fn(async () => undefined),
    query: vi.fn(async () => ({
      queryStatus: YAS_STATUS_OK,
      flags: 0,
      detail: "",
      nextCursor: new Uint8Array(),
      totalHint: 0n,
      records: async () => [],
    })),
    ...overrides,
  } as unknown as YasLspWorkspace;
}

async function openHandle(native: YasLspWorkspace) {
  const workspace = new YasNativeWorkspaceLsp(connection(), {
    terminalHandle: () => undefined,
    client: { open: vi.fn(async () => native) },
    hashBytes: async () => new Uint8Array(32),
    operationId: () => new Uint8Array(16).fill(1),
  });
  return workspace.openLsp("/repo");
}

describe("YasNativeWorkspaceLsp buffer admission", () => {
  it("keeps one in-flight upload and only the latest rapid edit per path", async () => {
    const firstWriteStarted = deferred();
    const releaseFirstWrite = deferred();
    const writes: Uint8Array[] = [];
    let uploadCount = 0;
    let revision = 0n;
    const native = nativeWorkspace({
      bufferBegin: vi.fn(async () => {
        const current = uploadCount++;
        return {
          stagingHandle: BigInt(current + 1),
          transfer: {
            write: async (content: Uint8Array) => {
              writes.push(new Uint8Array(content));
              if (current === 0) {
                firstWriteStarted.resolve();
                await releaseFirstWrite.promise;
              }
            },
            closeWrite: vi.fn(),
            closed: Promise.resolve(),
            reset: vi.fn(),
          },
          extensions: [],
        } as never;
      }),
      bufferCommit: vi.fn(async () => {
        revision++;
        return identity(revision, writes.at(-1)?.length ?? 0);
      }),
      bufferClose: vi.fn(async () => new Uint8Array()),
    } as Partial<YasLspWorkspace>);
    const handle = await openHandle(native);
    const size = YAS_LSP_MAX_INLINE_BUFFER_BYTES + 1;

    const first = new Uint8Array(size).fill(1);
    handle.buffer("src/main.ts", first);
    await firstWriteStarted.promise;

    for (let edit = 2; edit <= 100; edit++) {
      const content = new Uint8Array(size).fill(edit);
      handle.buffer("src/main.ts", content);
    }
    releaseFirstWrite.resolve();

    await handle.hover("src/main.ts", 0, 0);

    expect(native.bufferBegin).toHaveBeenCalledTimes(2);
    expect(native.bufferCommit).toHaveBeenCalledTimes(2);
    expect(writes).toHaveLength(2);
    expect(writes[0]![0]).toBe(1);
    expect(writes[1]![0]).toBe(100);
    expect(writes[1]![size - 1]).toBe(100);
  });

  it("forgets a failed path when the buffer is released", async () => {
    const failure = new Error("buffer upload failed");
    const native = nativeWorkspace({
      bufferPut: vi.fn(async () => {
        throw failure;
      }),
      bufferClose: vi.fn(async () => new Uint8Array()),
    } as Partial<YasLspWorkspace>);
    const handle = await openHandle(native);

    handle.buffer("rotated.ts", new Uint8Array([1]));
    await expect(handle.hover("rotated.ts", 0, 0)).rejects.toBe(failure);

    handle.releaseBuffer("rotated.ts");
    await expect(handle.hover("rotated.ts", 0, 0)).resolves.toMatchObject({
      status: expect.any(Number),
      records: [],
    });
  });

  it("closes an active native workspace on facade disposal", async () => {
    vi.stubGlobal("reportError", vi.fn());
    const native = nativeWorkspace();
    const workspace = new YasNativeWorkspaceLsp(connection(), {
      terminalHandle: () => undefined,
      client: { open: vi.fn(async () => native) },
      hashBytes: async () => new Uint8Array(32),
      operationId: () => new Uint8Array(16).fill(1),
    });
    await workspace.openLsp("/repo", {
      onClosed: () => {
        throw new Error("close callback failed");
      },
    });

    workspace.dispose();
    await Promise.resolve();

    expect(native.close).toHaveBeenCalledOnce();
    vi.unstubAllGlobals();
  });

  it("self-closes an LSP workspace returned after permanent disposal", async () => {
    let resolveOpen!: (workspace: YasLspWorkspace) => void;
    const pendingOpen = new Promise<YasLspWorkspace>((resolve) => {
      resolveOpen = resolve;
    });
    const native = nativeWorkspace();
    const workspace = new YasNativeWorkspaceLsp(connection(), {
      terminalHandle: () => undefined,
      client: { open: vi.fn(() => pendingOpen) },
      hashBytes: async () => new Uint8Array(32),
      operationId: () => new Uint8Array(16).fill(1),
    });

    const pending = workspace.openLsp("/repo");
    workspace.dispose();
    resolveOpen(native);

    await expect(pending).rejects.toThrow(/OPEN was pending/);
    expect(native.close).toHaveBeenCalledOnce();
  });

  it("suppresses State callbacks after a subscriber disposes reentrantly", async () => {
    let deliver!: (snapshot: YasLspSnapshot) => void;
    const native = nativeWorkspace({
      catalog: {
        subscribe: vi.fn((listener: (snapshot: YasLspSnapshot) => void) => {
          deliver = listener;
          return () => undefined;
        }),
        watch: vi.fn(async () => undefined),
        unwatch: vi.fn(async () => undefined),
      } as never,
    });
    const onState = vi.fn();
    const onDiagnostics = vi.fn();
    const workspace = new YasNativeWorkspaceLsp(connection(), {
      terminalHandle: () => undefined,
      client: { open: vi.fn(async () => native) },
      hashBytes: async () => new Uint8Array(32),
      operationId: () => new Uint8Array(16).fill(1),
    });
    const handle = await workspace.openLsp("/repo", {
      watch: true,
      diagnostics: true,
      onState,
      onDiagnostics,
    });
    handle.subscribe(() => workspace.dispose());

    deliver({ revision: 1n, backends: [], diagnostics: [], buffers: [] });

    expect(onState).not.toHaveBeenCalled();
    expect(onDiagnostics).not.toHaveBeenCalled();
    expect(native.close).toHaveBeenCalledOnce();
  });
});
