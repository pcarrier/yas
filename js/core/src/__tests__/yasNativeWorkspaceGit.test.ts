import { describe, expect, it, vi } from "vitest";

import {
  YasNativeWorkspaceGit,
  type YasConnection,
  type YasGitRepository,
} from "../yas";

function connection(): YasConnection {
  return {
    onInvalidation: vi.fn(() => () => undefined),
  } as unknown as YasConnection;
}

function nativeRepository(): YasGitRepository {
  return {
    handle: 0xf000_0000_0000_0001n,
    opened: {
      repositoryHandle: 0xf000_0000_0000_0001n,
      repositoryRevision: 1n,
      objectAlgorithm: 1,
      repositoryFlags: 0,
      canonicalWorktreePath: new TextEncoder().encode("/repo"),
      canonicalGitDir: new TextEncoder().encode("/repo/.git"),
      extensions: [],
    },
    onClosed: vi.fn(() => () => undefined),
    close: vi.fn(async () => undefined),
  } as unknown as YasGitRepository;
}

describe("YasNativeWorkspaceGit lifecycle", () => {
  it("closes active repositories on facade disposal", async () => {
    vi.stubGlobal("reportError", vi.fn());
    const native = nativeRepository();
    const workspace = new YasNativeWorkspaceGit(connection(), {
      terminalHandle: () => undefined,
      client: {
        open: vi.fn(async () => native),
        discover: vi.fn(),
      },
    });
    await workspace.openRepo("/repo", {
      onClosed: () => {
        throw new Error("close callback failed");
      },
    });

    workspace.dispose();
    await Promise.resolve();

    expect(native.close).toHaveBeenCalledOnce();
    vi.unstubAllGlobals();
  });

  it("self-closes a repository returned after permanent disposal", async () => {
    let resolveOpen!: (repository: YasGitRepository) => void;
    const pendingOpen = new Promise<YasGitRepository>((resolve) => {
      resolveOpen = resolve;
    });
    const native = nativeRepository();
    const workspace = new YasNativeWorkspaceGit(connection(), {
      terminalHandle: () => undefined,
      client: {
        open: vi.fn(() => pendingOpen),
        discover: vi.fn(),
      },
    });

    const pending = workspace.openRepo("/repo");
    workspace.dispose();
    resolveOpen(native);

    await expect(pending).rejects.toThrow(/OPEN was pending/);
    expect(native.close).toHaveBeenCalledOnce();
  });

  it("does not notify state subscribers again after onState disposes reentrantly", async () => {
    let deliver!: (snapshot: {
      revision: bigint;
      entities: readonly never[];
    }) => void;
    const native = nativeRepository();
    Object.assign(native, {
      list: vi.fn(async () => ({ revision: 1n, entities: [] })),
      catalog: {
        subscribe: vi.fn((listener) => {
          deliver = listener;
          return () => undefined;
        }),
      },
    });
    const workspace = new YasNativeWorkspaceGit(connection(), {
      terminalHandle: () => undefined,
      client: {
        open: vi.fn(async () => native),
        discover: vi.fn(),
      },
    });
    let handle: Awaited<ReturnType<typeof workspace.openRepo>> | undefined;
    handle = await workspace.openRepo("/repo", {
      watch: true,
      onState: () => {
        if (handle) workspace.dispose();
      },
    });
    const subscriber = vi.fn();
    handle.subscribe(subscriber);

    deliver({ revision: 2n, entities: [] });

    // One notification belongs to close(); applySnapshot must not emit again.
    expect(subscriber).toHaveBeenCalledOnce();
    expect(native.close).toHaveBeenCalledOnce();
  });
});
