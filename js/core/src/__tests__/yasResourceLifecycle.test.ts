import { afterEach, describe, expect, it, vi } from "vitest";

import {
  YAS_FAMILY_FS,
  YAS_FAMILY_GIT,
  YAS_FAMILY_LSP,
  YasFsClient,
  YasFsCatalog,
  YasGitCatalog,
  YasGitRepository,
  YasGitWatchedQuery,
  YasGitClient,
  YasKvNamespace,
  YasLspCatalog,
  YasLspClient,
  YasLspWorkspace,
  YasStateSubscription,
  type YasConnection,
  type YasInvalidation,
  type YasKvClient,
  type YasStateBatch,
} from "../yas";

interface TestConnection {
  readonly connection: YasConnection;
  readonly invalidations: Set<(event: YasInvalidation) => void>;
  eventCount(family: number): number;
}

function testConnection(): TestConnection {
  const invalidations = new Set<(event: YasInvalidation) => void>();
  const events = new Map<string, Set<(event: never) => void>>();
  const connection = {
    options: { receiveMaxBuffered: 16n * 1024n * 1024n },
    family: vi.fn(() => ({ version: 1 })),
    onInvalidation: vi.fn((listener: (event: YasInvalidation) => void) => {
      invalidations.add(listener);
      return () => invalidations.delete(listener);
    }),
    onEvent: vi.fn(
      (family: number, kind: number, listener: (event: never) => void) => {
        const key = `${family}/${kind}`;
        const listeners = events.get(key) ?? new Set();
        listeners.add(listener);
        events.set(key, listeners);
        return () => {
          listeners.delete(listener);
          if (listeners.size === 0) events.delete(key);
        };
      },
    ),
    request: vi.fn(async () => new Uint8Array()),
    transport: {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    },
  } as unknown as YasConnection;
  return {
    connection,
    invalidations,
    eventCount: (family) =>
      [...events]
        .filter(([key]) => key.startsWith(`${family}/`))
        .reduce((count, [, listeners]) => count + listeners.size, 0),
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("typed YAS family resource lifecycle", () => {
  it("self-closes a late FS OPEN and removes every FS-owned invalidation listener", async () => {
    const harness = testConnection();
    const opened = deferred<unknown>();
    Object.assign(harness.connection, {
      requestDecoded: vi.fn(async () => opened.promise),
    });
    const client = new YasFsClient(harness.connection);
    const pending = client.open({
      flags: 0,
      source: { kind: "platform-path", path: new Uint8Array([47]) },
    });

    client.dispose();
    opened.resolve({
      rootHandle: 9n,
      rootRevision: 1n,
      pathModel: 1,
      caseBehavior: 1,
      canonicalPath: new Uint8Array([47]),
      extensions: [],
    });

    await expect(pending).rejects.toThrow(/OPEN was pending/);
    expect(harness.connection.request).toHaveBeenCalledOnce();
    // The shared Transfer manager retains one connection listener; the FS
    // client and the late root catalogue both removed theirs.
    expect(harness.invalidations).toHaveLength(1);
  });

  it("removes Git and LSP family event/invalidation callbacks on dispose", () => {
    const gitHarness = testConnection();
    const git = new YasGitClient(gitHarness.connection);
    expect(gitHarness.eventCount(YAS_FAMILY_GIT)).toBe(2);
    expect(gitHarness.invalidations.size).toBe(2);
    git.dispose();
    expect(gitHarness.eventCount(YAS_FAMILY_GIT)).toBe(0);
    expect(gitHarness.invalidations.size).toBe(1);

    const lspHarness = testConnection();
    const lsp = new YasLspClient(lspHarness.connection);
    expect(lspHarness.eventCount(YAS_FAMILY_LSP)).toBe(1);
    expect(lspHarness.invalidations.size).toBe(2);
    lsp.dispose();
    expect(lspHarness.eventCount(YAS_FAMILY_LSP)).toBe(0);
    expect(lspHarness.invalidations.size).toBe(1);
  });

  it("promptly cancels every catalogue WATCH and later self-unwatches its Result", async () => {
    const factories = [
      (connection: YasConnection) => new YasFsCatalog(connection, 5n),
      (connection: YasConnection) => new YasGitCatalog(connection, 6n),
      (connection: YasConnection) => new YasLspCatalog(connection, 7n),
    ];
    for (const makeCatalog of factories) {
      const harness = testConnection();
      const watched = deferred<YasStateSubscription>();
      const unwatch = vi.fn(async () => undefined);
      const spy = vi
        .spyOn(YasStateSubscription, "watch")
        .mockReturnValue(watched.promise);
      const catalog = makeCatalog(harness.connection);
      const pending = catalog.watch();
      const settlement = pending.then(
        () => "resolved" as const,
        () => "rejected" as const,
      );

      await catalog.dispose();
      const prompt = await Promise.race([
        settlement,
        new Promise<"tick">((resolve) => setTimeout(() => resolve("tick"), 0)),
      ]);
      expect(prompt).toBe("rejected");
      expect(harness.invalidations.size).toBe(0);

      watched.resolve({
        active: true,
        unwatch,
      } as unknown as YasStateSubscription);
      await vi.waitFor(() => expect(unwatch).toHaveBeenCalledOnce());
      spy.mockRestore();
    }
  });

  it("promptly rejects every first-snapshot wait after WATCH is active", async () => {
    const factories = [
      (connection: YasConnection) => new YasFsCatalog(connection, 15n),
      (connection: YasConnection) => new YasGitCatalog(connection, 16n),
      (connection: YasConnection) => new YasLspCatalog(connection, 17n),
    ];
    for (const makeCatalog of factories) {
      const harness = testConnection();
      const unwatch = vi.fn(async () => undefined);
      const spy = vi.spyOn(YasStateSubscription, "watch").mockResolvedValue({
        active: true,
        unwatch,
      } as unknown as YasStateSubscription);
      const catalog = makeCatalog(harness.connection);
      const pending = catalog.firstSnapshot();
      const settlement = pending.then(
        () => "resolved" as const,
        () => "rejected" as const,
      );
      await vi.waitFor(() => expect(spy).toHaveBeenCalledOnce());
      await Promise.resolve();

      await catalog.dispose();
      const prompt = await Promise.race([
        settlement,
        new Promise<"tick">((resolve) => setTimeout(() => resolve("tick"), 0)),
      ]);

      expect(prompt).toBe("rejected");
      expect(unwatch).toHaveBeenCalledOnce();
      spy.mockRestore();
    }
  });

  it("isolates throwing catalogue subscribers and still sends UNWATCH", async () => {
    vi.stubGlobal("reportError", vi.fn());
    const factories = [
      (connection: YasConnection) => new YasFsCatalog(connection, 25n),
      (connection: YasConnection) => new YasGitCatalog(connection, 26n),
      (connection: YasConnection) => new YasLspCatalog(connection, 27n),
    ];
    for (const makeCatalog of factories) {
      const harness = testConnection();
      const unwatch = vi.fn(async () => undefined);
      const spy = vi.spyOn(YasStateSubscription, "watch").mockResolvedValue({
        active: true,
        unwatch,
      } as unknown as YasStateSubscription);
      const catalog = makeCatalog(harness.connection);
      await catalog.watch();
      const sibling = vi.fn();
      catalog.subscribe(() => {
        throw new Error("subscriber failed");
      });
      catalog.subscribe(sibling);
      sibling.mockClear();

      await catalog.unwatch();

      expect(sibling).toHaveBeenCalledOnce();
      expect(unwatch).toHaveBeenCalledOnce();
      await catalog.dispose();
      spy.mockRestore();
    }
  });

  it("sends Git and LSP CLOSE despite throwing close listeners", async () => {
    vi.stubGlobal("reportError", vi.fn());
    const gitHarness = testConnection();
    const gitClient = {
      connection: gitHarness.connection,
      release: vi.fn(),
    } as unknown as YasGitClient;
    const repository = new YasGitRepository(gitClient, {
      repositoryHandle: 51n,
      repositoryRevision: 1n,
      objectAlgorithm: 1,
      repositoryFlags: 0,
      canonicalWorktreePath: new Uint8Array([47]),
      canonicalGitDir: new Uint8Array([47]),
      extensions: [],
    });
    repository.onClosed(() => {
      throw new Error("Git close listener failed");
    });

    await repository.close();
    expect(gitHarness.connection.request).toHaveBeenCalledOnce();

    const lspHarness = testConnection();
    const lspClient = {
      connection: lspHarness.connection,
      releaseWorkspace: vi.fn(),
    } as unknown as YasLspClient;
    const workspace = new YasLspWorkspace(lspClient, {
      workspaceHandle: 52n,
      workspaceRevision: 1n,
      positionEncoding: 1,
      backendCount: 1,
      capabilities: 0n,
      canonicalRoot: new Uint8Array([47]),
      extensions: [],
    });
    workspace.onClosed(() => {
      throw new Error("LSP close listener failed");
    });

    await workspace.close();
    expect(lspHarness.connection.request).toHaveBeenCalledOnce();
  });

  it("promptly rejects a pending Git WATCH QUERY and self-closes its late Result", async () => {
    const harness = testConnection();
    const late = deferred<YasGitWatchedQuery>();
    const close = vi.fn(async () => undefined);
    vi.spyOn(YasGitWatchedQuery, "open").mockReturnValue(late.promise);
    const client = {
      connection: harness.connection,
      release: vi.fn(),
    } as unknown as YasGitClient;
    const repository = new YasGitRepository(client, {
      repositoryHandle: 31n,
      repositoryRevision: 1n,
      objectAlgorithm: 1,
      repositoryFlags: 0,
      canonicalWorktreePath: new Uint8Array([47]),
      canonicalGitDir: new Uint8Array([47]),
      extensions: [],
    });
    const pending = repository.watchQuery(
      { kind: "resolve", spec: new Uint8Array() },
      () => undefined,
    );
    const settlement = pending.then(
      () => "resolved" as const,
      () => "rejected" as const,
    );

    repository.invalidate();
    const prompt = await Promise.race([
      settlement,
      new Promise<"tick">((resolve) => setTimeout(() => resolve("tick"), 0)),
    ]);
    expect(prompt).toBe("rejected");

    late.resolve({ close } as unknown as YasGitWatchedQuery);
    await vi.waitFor(() => expect(close).toHaveBeenCalledOnce());
  });

  it("promptly rejects a pending KV WATCH and self-unwatches its late Result", async () => {
    const harness = testConnection();
    const late = deferred<YasStateSubscription>();
    const unwatch = vi.fn(async () => undefined);
    vi.spyOn(YasStateSubscription, "watch").mockReturnValue(late.promise);
    const client = {
      connection: harness.connection,
      hashBytes: vi.fn(async () => new Uint8Array(32)),
      release: vi.fn(),
    } as unknown as YasKvClient;
    const namespace = new YasKvNamespace(client, new Uint8Array(), {
      namespaceHandle: 41n,
      storeRevision: 1n,
      extensions: [],
    });
    const pending = namespace.watch(() => undefined);
    const settlement = pending.then(
      () => "resolved" as const,
      () => "rejected" as const,
    );

    await namespace.close();
    const prompt = await Promise.race([
      settlement,
      new Promise<"tick">((resolve) => setTimeout(() => resolve("tick"), 0)),
    ]);
    expect(prompt).toBe("rejected");

    late.resolve({ active: true, unwatch } as unknown as YasStateSubscription);
    await vi.waitFor(() => expect(unwatch).toHaveBeenCalledOnce());
  });

  it("suppresses KV State delivery after namespace close", async () => {
    const harness = testConnection();
    let deliver!: (batch: YasStateBatch) => void;
    const unwatch = vi.fn(async () => undefined);
    vi.spyOn(YasStateSubscription, "watch").mockImplementation(
      async (...args) => {
        deliver = args[7];
        return {
          active: true,
          unwatch,
        } as unknown as YasStateSubscription;
      },
    );
    const client = {
      connection: harness.connection,
      hashBytes: vi.fn(async () => new Uint8Array(32)),
      release: vi.fn(),
    } as unknown as YasKvClient;
    const namespace = new YasKvNamespace(client, new Uint8Array(), {
      namespaceHandle: 5n,
      storeRevision: 1n,
      extensions: [],
    });
    const onUpdate = vi.fn();

    await namespace.watch(onUpdate);
    await namespace.close();
    deliver({
      phase: 4,
      flags: 0,
      fromRevision: 1n,
      toRevision: 2n,
      records: [],
    });

    expect(onUpdate).not.toHaveBeenCalled();
    expect(namespace.storeRevision).toBe(1n);
    expect(unwatch).toHaveBeenCalledOnce();
  });
});
