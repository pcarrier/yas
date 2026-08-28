import { describe, expect, it, vi } from "vitest";
import { YasNativeChannelFacade } from "../yas/nativeChannelFacade";
import { YasChannelCatalogue, type YasChannelConnection } from "../yas/channel";
import { YasConnection } from "../yas/session";
import { YasStateSubscription } from "../yas/state";
import type { YasTransfer } from "../yas/transfer";
import { MockYasTransport } from "./mock-yas-transport";

describe("YasNativeChannelFacade", () => {
  it("wakes queued adapters when an asynchronous send releases its reservation", async () => {
    let finishSend!: () => void;
    const sending = new Promise<void>((resolve) => {
      finishSend = resolve;
    });
    const credit = vi.fn();
    const transfer = {
      descriptor: { maxItemBytes: 1024n },
      outgoingCreditOutstanding: 5n,
      subscribeOutgoingCredit: vi.fn(() => () => undefined),
      sendMessage: vi.fn(() => sending),
      readMessage: vi.fn(() => new Promise<null>(() => undefined)),
      closeWrite: vi.fn(),
      reset: vi.fn(),
    };
    const client = {
      catalogue: {
        firstSnapshot: vi.fn().mockResolvedValue({
          revision: 1n,
          listeners: [
            {
              listenerHandle: 1n,
              generation: 1n,
              ownerKind: 0,
              ownerSession: new Uint8Array(16),
              name: "yas.test.v1",
              metadata: new Uint8Array(),
              extensions: [],
            },
          ],
        }),
        subscribe: vi.fn(() => () => undefined),
      },
      connect: vi.fn().mockResolvedValue({
        channelHandle: 2n,
        peerChannelHandle: 3n,
        peerSession: new Uint8Array(16),
        listenerMetadata: new Uint8Array(),
      connectorMetadata: new Uint8Array(),
      transfer,
      }),
    };
    const connection = new YasConnection(
      new MockYasTransport("disconnected"),
    );
    const facade = new YasNativeChannelFacade(connection, client);
    const handle = await facade.connectChannel("yas.test.v1", {
      onCredit: credit,
    });

    expect(handle.send("five!".slice(0, 5))).toBe(true);
    expect(handle.availableCredit).toBe(0n);
    finishSend();
    await sending;
    await Promise.resolve();

    expect(credit).toHaveBeenCalledWith(5n);
    expect(handle.availableCredit).toBe(5n);
    facade.dispose();
    connection.close();
  });

  it("keeps high-bit listener and channel handles opaque", async () => {
    const listenerHandle = 0xfedc_ba98_7654_3210n;
    const generation = 0x8000_0000_0000_0011n;
    const channelHandle = 0x9000_0000_0000_0022n;
    const sent: Uint8Array[] = [];
    const transfer = {
      descriptor: { maxItemBytes: 1024n },
      outgoingCreditOutstanding: 1024n,
      subscribeOutgoingCredit: vi.fn(() => () => undefined),
      sendMessage: vi.fn(async (message: Uint8Array) => {
        sent.push(message);
      }),
      readMessage: vi.fn(() => new Promise<null>(() => undefined)),
      closeWrite: vi.fn(),
      reset: vi.fn(),
    } as unknown as YasTransfer;
    const endpoint = {
      channelHandle,
      peerChannelHandle: 0xa000_0000_0000_0033n,
      peerSession: Uint8Array.from({ length: 16 }, (_, index) => index + 1),
      listenerMetadata: new Uint8Array([1]),
      connectorMetadata: new Uint8Array([2]),
      transfer,
    } as YasChannelConnection;
    const connect = vi.fn().mockResolvedValue(endpoint);
    const client = {
      catalogue: {
        firstSnapshot: vi.fn().mockResolvedValue({
          revision: 1n,
          listeners: [
            {
              listenerHandle,
              generation,
              ownerKind: 0,
              ownerSession: new Uint8Array(16).fill(1),
              name: "yas.test.v1",
              metadata: new Uint8Array(),
              extensions: [],
            },
          ],
        }),
        subscribe: vi.fn(() => () => undefined),
      },
      connect,
    };
    const connection = {
      ready: true,
      onInvalidation: vi.fn(() => () => undefined),
    } as unknown as YasConnection;
    const facade = new YasNativeChannelFacade(connection, client);

    const handle = await facade.connectChannel("yas.test.v1", {
      expectedListener: { listenerHandle, generation },
    });
    expect(connect).toHaveBeenCalledWith(
      expect.objectContaining({ listenerHandle, generation }),
      { metadata: expect.any(Uint8Array) },
    );
    expect(handle.channelHandle).toBe(channelHandle);
    expect(handle.send("native")).toBe(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(new TextDecoder().decode(sent[0])).toBe("native");
    expect(transfer.sendMessage).toHaveBeenCalledTimes(1);
  });

  it("filters the typed listener catalogue without allocating watch IDs", async () => {
    const snapshot = {
      revision: 4n,
      listeners: [
        {
          listenerHandle: 0xffff_0000_0000_0001n,
          generation: 0xffff_0000_0000_0002n,
          ownerKind: 0,
          ownerSession: new Uint8Array(16).fill(1),
          name: "yas.session.v1",
          metadata: new Uint8Array(),
          extensions: [],
        },
      ],
    };
    const client = {
      catalogue: {
        firstSnapshot: vi.fn().mockResolvedValue(snapshot),
        subscribe: vi.fn((listener: (value: typeof snapshot) => void) => {
          listener(snapshot);
          return () => undefined;
        }),
      },
      connect: vi.fn(),
    };
    const connection = {
      ready: true,
      onInvalidation: vi.fn(() => () => undefined),
    } as unknown as YasConnection;
    const facade = new YasNativeChannelFacade(connection, client);

    const watch = await facade.watchChannelNames(
      ["yas.session.v1", "yas.systemd.v1"],
      vi.fn(),
    );
    expect([...watch.present]).toEqual(["yas.session.v1"]);
  });

  it("promptly cancels shared-catalog operations without disposing the client", async () => {
    const invalidations = new Set<(value: never) => void>();
    const connection = {
      ready: true,
      options: {},
      onInvalidation: vi.fn((listener: (value: never) => void) => {
        invalidations.add(listener);
        return () => invalidations.delete(listener);
      }),
    } as unknown as YasConnection;
    const catalogue = new YasChannelCatalogue(connection);
    const rawWatch = new Promise<YasStateSubscription>(() => undefined);
    const watchSpy = vi
      .spyOn(YasStateSubscription, "watch")
      .mockReturnValue(rawWatch);
    const disposeClient = vi.fn();
    const client = {
      catalogue,
      connect: vi.fn(),
      dispose: disposeClient,
    };
    const facade = new YasNativeChannelFacade(connection, client);
    const internal = catalogue as unknown as {
      listeners: Set<unknown>;
      snapshotRejectors: Set<unknown>;
    };

    const connecting = facade.connectChannel("yas.shared.v1");
    const watching = facade.watchChannelNames(["yas.shared.v1"], vi.fn());
    expect(internal.listeners.size).toBe(2);
    expect(internal.snapshotRejectors.size).toBe(2);
    facade.dispose();

    await expect(connecting).rejects.toThrow(/closed/);
    await expect(watching).rejects.toThrow(/closed/);
    expect(disposeClient).not.toHaveBeenCalled();
    expect(client.connect).not.toHaveBeenCalled();
    expect(internal.listeners.size).toBe(0);
    expect(internal.snapshotRejectors.size).toBe(0);

    const removeSharedSubscriber = catalogue.subscribe(() => undefined);
    removeSharedSubscriber();
    catalogue.dispose();
    watchSpy.mockRestore();
  });
});
