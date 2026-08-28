import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  YasWorkspace,
  ChannelHandle,
  ChannelNamesWatch,
  ChannelOpenOptions,
  ConnectionId,
} from "@yas-run/core";
import {
  dropSessionCatalog,
  ensureSessionCatalog,
  sessionHandle,
} from "../sessionCatalogs";
import { SESSION_CHANNEL } from "../session";

const CONNECTION_ID = "catalog-test" as ConnectionId;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

type PendingOpen = {
  readonly options: ChannelOpenOptions;
  readonly result: ReturnType<typeof deferred<ChannelHandle>>;
  readonly close: ReturnType<typeof vi.fn>;
  readonly channel: ChannelHandle;
};

function fakeRemote() {
  const watches: {
    present: Set<string>;
    publish(present: boolean): void;
    stop: ReturnType<typeof vi.fn>;
  }[] = [];
  const opens: PendingOpen[] = [];
  const connection = {
    watchChannelNames(
      _names: readonly string[],
      onNames: (present: ReadonlySet<string>) => void,
    ): Promise<ChannelNamesWatch> {
      const present = new Set<string>();
      const stop = vi.fn();
      watches.push({
        present,
        publish(served) {
          present.clear();
          if (served) present.add(SESSION_CHANNEL);
          onNames(present);
        },
        stop,
      });
      return Promise.resolve({ present, stop });
    },
    connectChannel(
      _name: string,
      options: ChannelOpenOptions = {},
    ): Promise<ChannelHandle> {
      const result = deferred<ChannelHandle>();
      const close = vi.fn();
      const channel = {
        id: opens.length + 1,
        send: vi.fn(),
        close,
      } as unknown as ChannelHandle;
      opens.push({ options, result, close, channel });
      return result.promise;
    },
  };
  const workspace = {
    getConnection: (id: ConnectionId) =>
      id === CONNECTION_ID ? connection : undefined,
  } as unknown as YasWorkspace;
  return { workspace, watches, opens };
}

const settle = async (): Promise<void> => {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
};

afterEach(async () => {
  dropSessionCatalog(CONNECTION_ID);
  vi.useRealTimers();
  await settle();
});

describe("session catalog channel lifecycle", () => {
  it("keeps only one open in flight across duplicate presence updates", async () => {
    const remote = fakeRemote();
    ensureSessionCatalog(remote.workspace, CONNECTION_ID, 1);
    await settle();

    remote.watches[0]?.publish(true);
    remote.watches[0]?.publish(true);
    remote.watches[0]?.publish(true);
    expect(remote.opens).toHaveLength(1);

    remote.opens[0]?.result.resolve(remote.opens[0].channel);
    await settle();
    expect(sessionHandle(CONNECTION_ID)).not.toBeNull();
  });

  it("closes a stale pending resolution after an absent/present flap", async () => {
    const remote = fakeRemote();
    ensureSessionCatalog(remote.workspace, CONNECTION_ID, 1);
    await settle();

    remote.watches[0]?.publish(true);
    const first = remote.opens[0];
    remote.watches[0]?.publish(false);
    remote.watches[0]?.publish(true);
    const second = remote.opens[1];
    expect(first).toBeDefined();
    expect(second).toBeDefined();

    second?.result.resolve(second.channel);
    await settle();
    const replacement = sessionHandle(CONNECTION_ID);
    expect(replacement).not.toBeNull();

    first?.result.resolve(first.channel);
    await settle();
    expect(first?.close).toHaveBeenCalledTimes(1);
    expect(sessionHandle(CONNECTION_ID)).toBe(replacement);
  });

  it("ignores a late onClosed callback from the exact superseded handle", async () => {
    const remote = fakeRemote();
    ensureSessionCatalog(remote.workspace, CONNECTION_ID, 1);
    await settle();

    remote.watches[0]?.publish(true);
    const first = remote.opens[0];
    first?.result.resolve(first.channel);
    await settle();
    expect(sessionHandle(CONNECTION_ID)).not.toBeNull();

    remote.watches[0]?.publish(false);
    remote.watches[0]?.publish(true);
    const second = remote.opens[1];
    second?.result.resolve(second.channel);
    await settle();
    const replacement = sessionHandle(CONNECTION_ID);
    expect(replacement).not.toBeNull();

    first?.options.onClosed?.(0, "late close from old generation");
    expect(sessionHandle(CONNECTION_ID)).toBe(replacement);
    expect(second?.close).not.toHaveBeenCalled();
  });

  it("reopens an unexpectedly closed channel while its name stays present", async () => {
    vi.useFakeTimers();
    const remote = fakeRemote();
    ensureSessionCatalog(remote.workspace, CONNECTION_ID, 1);
    await settle();

    remote.watches[0]?.publish(true);
    const first = remote.opens[0];
    first?.result.resolve(first.channel);
    await settle();
    expect(sessionHandle(CONNECTION_ID)).not.toBeNull();

    first?.options.onClosed?.(0, "extension attempt replaced");
    expect(sessionHandle(CONNECTION_ID)).toBeNull();
    expect(remote.opens).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(50);
    expect(remote.opens).toHaveLength(2);
    const replacement = remote.opens[1];
    replacement?.result.resolve(replacement.channel);
    await settle();
    expect(sessionHandle(CONNECTION_ID)).not.toBeNull();
  });

  it("generation replacement closes the earlier pending open on resolution", async () => {
    const remote = fakeRemote();
    ensureSessionCatalog(remote.workspace, CONNECTION_ID, 1);
    await settle();
    remote.watches[0]?.publish(true);
    const first = remote.opens[0];

    ensureSessionCatalog(remote.workspace, CONNECTION_ID, 2);
    await settle();
    expect(remote.watches[0]?.stop).toHaveBeenCalledTimes(1);
    remote.watches[1]?.publish(true);
    const second = remote.opens[1];

    first?.result.resolve(first.channel);
    second?.result.resolve(second.channel);
    await settle();
    expect(first?.close).toHaveBeenCalledTimes(1);
    expect(second?.close).not.toHaveBeenCalled();
    expect(sessionHandle(CONNECTION_ID)).not.toBeNull();
  });
});
