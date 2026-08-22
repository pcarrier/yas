import { afterEach, describe, expect, it, vi } from "vitest";

import {
  YAS_CHANNEL_CLOSE_LISTENER,
  YAS_CHANNEL_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_CHANNEL_LISTEN,
  YAS_EVENTS_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_EVENTS_START_STREAM,
  YAS_EVENTS_STOP_STREAM,
  YAS_EXTENSION_ATTEMPT_CONTEXT,
  YAS_EXTENSION_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_EXTENSION_OBJECT_UPLOAD,
  YAS_FAMILY_CHANNEL,
  YAS_FAMILY_EVENTS,
  YAS_FAMILY_EXTENSION,
  YAS_FAMILY_NET,
  YAS_FAMILY_RELAY,
  YAS_NET_CLOSE,
  YAS_NET_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_NET_DELIVERY_PREFER_NATIVE,
  YAS_NET_DELIVERY_RELIABLE_TUNNEL,
  YAS_NET_DIRECTION_DUPLEX,
  YAS_NET_DROP_OLDEST,
  YAS_NET_MODE_DATAGRAM,
  YAS_GOLDEN_VECTORS,
  YAS_RELAY_TUNNEL_CONTENT_KIND,
  YAS_RELAY_VERSION,
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
  YasChannelCatalogue,
  YasChannelClient,
  YasEventsClient,
  YasExtensionCatalog,
  YasExtensionClient,
  YasNetClient,
  YasProcessCatalog,
  YasRelayClient,
  YasRelayRoutes,
  YasStateSubscription,
  YasWriter,
  decodeChannelIdentity,
  encodeTransferDescriptor,
} from "../yas";
import type {
  YasConnection,
  YasInvalidation,
  YasReceiveBudgetLease,
} from "../yas/session";

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(error: unknown): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function catalogConnection(): {
  connection: YasConnection;
  invalidations: Set<(value: YasInvalidation) => void>;
} {
  const invalidations = new Set<(value: YasInvalidation) => void>();
  const connection = {
    options: {},
    onInvalidation(listener: (value: YasInvalidation) => void) {
      invalidations.add(listener);
      return () => invalidations.delete(listener);
    },
  } as unknown as YasConnection;
  return { connection, invalidations };
}

function productConnection() {
  const events = new Set<() => void>();
  const eventHandlers = new Map<
    number,
    Set<(event: { payload: Uint8Array }) => void>
  >();
  const invalidations = new Set<(value: YasInvalidation) => void>();
  const statusListeners = new Set<(status: string) => void>();
  const request = vi.fn(async () => new Uint8Array());
  const connection = {
    options: { receiveMaxBuffered: 16n * 1024n * 1024n },
    family: vi.fn((family: number) => {
      const tags = new Map([
        [YAS_FAMILY_CHANNEL, YAS_CHANNEL_LIMIT_MAX_MUTATION_REPLAYS],
        [YAS_FAMILY_EVENTS, YAS_EVENTS_LIMIT_MAX_MUTATION_REPLAYS],
        [YAS_FAMILY_EXTENSION, YAS_EXTENSION_LIMIT_MAX_MUTATION_REPLAYS],
        [YAS_FAMILY_NET, YAS_NET_LIMIT_MAX_MUTATION_REPLAYS],
      ]);
      const tag = tags.get(family);
      return {
        limits:
          tag === undefined
            ? []
            : [
                {
                  tag,
                  required: true,
                  value: new YasWriter().u32(64).finish(),
                },
              ],
      };
    }),
    registerFamilyLimitValidator: vi.fn(),
    onEvent: vi.fn(
      (
        _family: number,
        kind: number,
        handler: (event: { payload: Uint8Array }) => void,
      ) => {
        const marker = () => undefined;
        events.add(marker);
        let handlers = eventHandlers.get(kind);
        if (!handlers) {
          handlers = new Set();
          eventHandlers.set(kind, handlers);
        }
        handlers.add(handler);
        return () => {
          events.delete(marker);
          handlers!.delete(handler);
        };
      },
    ),
    onInvalidation(listener: (value: YasInvalidation) => void) {
      invalidations.add(listener);
      return () => invalidations.delete(listener);
    },
    transport: {
      addEventListener(_type: string, listener: (status: string) => void) {
        statusListeners.add(listener);
      },
      removeEventListener(_type: string, listener: (status: string) => void) {
        statusListeners.delete(listener);
      },
    },
    receiveBudget: {
      reserve(bytes: bigint): YasReceiveBudgetLease {
        return { bytes, release: vi.fn() } as unknown as YasReceiveBudgetLease;
      },
    },
    request,
    requestDecoded: vi.fn(),
    sendEvent: vi.fn(),
    sendDatagramEvent: vi.fn(() => true),
    nanosecondsUntilServerTime: vi.fn(() => 1n),
  } as unknown as YasConnection;
  return { connection, events, eventHandlers, invalidations, request };
}

function goldenBytes(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (entry) => entry.name === name,
  )!.hex;
  return Uint8Array.from(hex.match(/../g)!, (byte) =>
    Number.parseInt(byte, 16),
  );
}

afterEach(() => vi.restoreAllMocks());

describe("YAS product-family lifecycle", () => {
  it("promptly cancels pending catalogue WATCH and UNWATCHes a late Result", async () => {
    const cases = [
      (connection: YasConnection) => new YasProcessCatalog(connection),
      (connection: YasConnection) => new YasChannelCatalogue(connection),
      (connection: YasConnection) => new YasExtensionCatalog(connection),
      (connection: YasConnection) =>
        new YasRelayRoutes(connection, () => ({
          maxRoutes: 4,
          maxLinksPerSession: 4,
          maxPendingConnects: 4,
          maxEarlyData: 0,
          connectTimeoutNs: 1n,
          maxBufferedPerLink: 1024n,
        })),
    ];

    for (const create of cases) {
      const watchResult = deferred<YasStateSubscription>();
      const unwatch = vi.fn(async () => undefined);
      vi.spyOn(YasStateSubscription, "watch").mockReturnValueOnce(
        watchResult.promise,
      );
      const { connection, invalidations } = catalogConnection();
      const catalog = create(connection);
      const subscriber = vi.fn();
      catalog.subscribe(subscriber);
      expect(invalidations.size).toBe(1);

      const watch = catalog.watch();
      const snapshot = catalog.firstSnapshot();
      const callsBeforeDispose = subscriber.mock.calls.length;
      catalog.dispose();

      await expect(watch).rejects.toThrow(/disposed/);
      await expect(snapshot).rejects.toThrow(/disposed/);
      expect(subscriber).toHaveBeenCalledTimes(callsBeforeDispose);
      expect(invalidations.size).toBe(0);
      expect(unwatch).not.toHaveBeenCalled();

      watchResult.resolve({
        active: true,
        unwatch,
      } as unknown as YasStateSubscription);
      await vi.waitFor(() => expect(unwatch).toHaveBeenCalledOnce());
      expect(() => catalog.subscribe(() => undefined)).toThrow(/disposed/);
    }
  });

  it("isolates throwing catalogue subscribers and still sends UNWATCH", async () => {
    const cases = [
      (connection: YasConnection) => new YasProcessCatalog(connection),
      (connection: YasConnection) => new YasChannelCatalogue(connection),
      (connection: YasConnection) => new YasExtensionCatalog(connection),
      (connection: YasConnection) =>
        new YasRelayRoutes(connection, () => ({
          maxRoutes: 4,
          maxLinksPerSession: 4,
          maxPendingConnects: 4,
          maxEarlyData: 0,
          connectTimeoutNs: 1n,
          maxBufferedPerLink: 1024n,
        })),
    ];

    for (const create of cases) {
      const unwatch = vi.fn(async () => undefined);
      vi.spyOn(YasStateSubscription, "watch").mockResolvedValueOnce({
        active: true,
        unwatch,
      } as unknown as YasStateSubscription);
      const { connection } = catalogConnection();
      const catalog = create(connection);
      const sibling = vi.fn();
      catalog.subscribe(() => {
        throw new Error("observer failed");
      });
      catalog.subscribe(sibling);

      await catalog.watch();
      await catalog.unwatch();

      expect(sibling.mock.calls.length).toBeGreaterThanOrEqual(3);
      expect(unwatch).toHaveBeenCalledOnce();
      catalog.dispose();
    }
  });

  it("stops a late Events stream without publishing it after disposal", async () => {
    const started = deferred<{
      streamHandle: bigint;
      firstSequence: bigint;
      maxBatchBytes: number;
      extensions: never[];
    }>();
    const { connection, events, invalidations, request } = productConnection();
    const requestDecoded = connection.requestDecoded as ReturnType<
      typeof vi.fn
    >;
    requestDecoded.mockImplementation((_family: number, kind: number) => {
      if (kind === YAS_EVENTS_START_STREAM) return started.promise;
      throw new Error(`unexpected Events request ${kind}`);
    });
    const client = new YasEventsClient(connection);
    const eventListenerCount = events.size;
    const invalidationListenerCount = invalidations.size;

    const pending = client.startStream();
    client.dispose();
    await expect(pending).rejects.toThrow(/disposed/);
    expect(events.size).toBe(eventListenerCount - 3);
    expect(invalidations.size).toBe(invalidationListenerCount - 1);

    started.resolve({
      streamHandle: 0xfeedn,
      firstSequence: 1n,
      maxBatchBytes: 4096,
      extensions: [],
    });
    await vi.waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        YAS_FAMILY_EVENTS,
        YAS_EVENTS_STOP_STREAM,
        expect.any(Uint8Array),
      ),
    );
  });

  it("closes a late Net flow without publishing it after disposal", async () => {
    const endpoint = deferred<{
      flowHandle: bigint;
      mode: number;
      direction: number;
      selectedDelivery: number;
      maxDatagramPayload: number;
      serverInstanceLimit: number;
      maxMessageBytes: bigint;
      peerAddress: { kind: "udp"; host: string; port: number };
      negotiatedAlpn: Uint8Array;
      extensions: never[];
    }>();
    const { connection, request } = productConnection();
    const requestDecoded = connection.requestDecoded as ReturnType<
      typeof vi.fn
    >;
    requestDecoded.mockReturnValue(endpoint.promise);
    const client = new YasNetClient(connection);
    const pending = client.open({
      operationId: new Uint8Array(16).fill(1),
      address: { kind: "udp", host: "127.0.0.1", port: 53 },
      deliveryPreference: YAS_NET_DELIVERY_PREFER_NATIVE,
      dropPolicy: YAS_NET_DROP_OLDEST,
    });

    client.dispose();
    await expect(pending).rejects.toThrow(/disposed/);
    endpoint.resolve({
      flowHandle: 0xbeefn,
      mode: YAS_NET_MODE_DATAGRAM,
      direction: YAS_NET_DIRECTION_DUPLEX,
      selectedDelivery: YAS_NET_DELIVERY_RELIABLE_TUNNEL,
      maxDatagramPayload: 1200,
      serverInstanceLimit: 8,
      maxMessageBytes: 0n,
      peerAddress: { kind: "udp", host: "127.0.0.1", port: 53 },
      negotiatedAlpn: new Uint8Array(),
      extensions: [],
    });
    await vi.waitFor(() =>
      expect(request).toHaveBeenCalledWith(
        YAS_FAMILY_NET,
        YAS_NET_CLOSE,
        expect.any(Uint8Array),
      ),
    );
  });

  it("resets an established direct Channel connection on client disposal", async () => {
    const { connection } = productConnection();
    const reset = vi.fn();
    const requestDecoded = connection.requestDecoded as ReturnType<
      typeof vi.fn
    >;
    requestDecoded.mockResolvedValue({
      channelHandle: 11n,
      peerChannelHandle: 12n,
      peerSession: new Uint8Array(16).fill(1),
      listenerMetadata: new Uint8Array(),
      connectorMetadata: new Uint8Array(),
      descriptor: {},
      extensions: [],
      transfer: {
        reset,
        closed: new Promise<void>(() => undefined),
      },
    });
    const client = new YasChannelClient(connection);

    await client.connect({ listenerHandle: 1n, generation: 2n });
    client.dispose();

    expect(reset).toHaveBeenCalledOnce();
  });

  it("keeps the original Channel listener when a Result reuses its handle", async () => {
    const { connection } = productConnection();
    const requestDecoded = connection.requestDecoded as ReturnType<
      typeof vi.fn
    >;
    const identities = [
      { listenerHandle: 0x44n, generation: 1n },
      { listenerHandle: 0x44n, generation: 2n },
    ];
    requestDecoded.mockImplementation((_family: number, kind: number) => {
      if (kind === YAS_CHANNEL_LISTEN)
        return Promise.resolve(identities.shift()!);
      if (kind === YAS_CHANNEL_CLOSE_LISTENER)
        return Promise.resolve(undefined);
      throw new Error(`unexpected Channel request ${kind}`);
    });
    const client = new YasChannelClient(connection);

    await client.listen("yas.first.v1", { onAccept: vi.fn() });
    await expect(
      client.listen("yas.second.v1", { onAccept: vi.fn() }),
    ).rejects.toThrow(/reused/);
    await vi.waitFor(() =>
      expect(
        requestDecoded.mock.calls.filter(
          (call) => call[1] === YAS_CHANNEL_CLOSE_LISTENER,
        ),
      ).toHaveLength(0),
    );
    client.dispose();
    await vi.waitFor(() =>
      expect(
        requestDecoded.mock.calls
          .filter((call) => call[1] === YAS_CHANNEL_CLOSE_LISTENER)
          .map((call) => decodeChannelIdentity(call[2] as Uint8Array)),
      ).toEqual([{ listenerHandle: 0x44n, generation: 1n, extensions: [] }]),
    );
  });

  it("resets a duplicate Relay result without overwriting the live link", async () => {
    const { connection } = productConnection();
    const firstReset = vi.fn();
    const secondReset = vi.fn();
    const transfers = [
      {
        reset: firstReset,
        closed: new Promise<void>(() => undefined),
      },
      {
        reset: secondReset,
        closed: new Promise<void>(() => undefined),
      },
    ];
    const route = {
      handle: 7n,
      generation: 3n,
      availability: 0,
      transportHint: 0,
      flags: 0,
      name: "work",
      label: "Work",
      description: "remote",
      extensions: [],
    };
    let resultIndex = 0;
    const requestDecoded = connection.requestDecoded as ReturnType<
      typeof vi.fn
    >;
    requestDecoded.mockImplementation(
      (
        _family: number,
        _kind: number,
        _payload: Uint8Array,
        decode: (body: Uint8Array) => unknown,
      ) => {
        const descriptor = encodeTransferDescriptor({
          transferId: 2 + resultIndex * 2,
          mode: YAS_TRANSFER_MODE_BYTE,
          direction:
            YAS_TRANSFER_RECEIVER_TO_SENDER | YAS_TRANSFER_SENDER_TO_RECEIVER,
          flags: 0,
          receiverSendCredit: 64n * 1024n,
          senderSendCredit: 64n * 1024n,
          maxItemBytes: 0n,
          maxChunkBytes: 64 * 1024,
          contentFamily: YAS_FAMILY_RELAY,
          contentKind: YAS_RELAY_TUNNEL_CONTENT_KIND,
          contentVersion: YAS_RELAY_VERSION,
          extensions: [
            {
              tag: YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
              required: true,
              value: new Uint8Array(),
            },
          ],
          maxOpenMessages: 1,
          sensitiveContent: true,
        });
        resultIndex++;
        return Promise.resolve(
          decode(
            new YasWriter()
              .u64(0x99n)
              .u64(route.handle)
              .u64(route.generation)
              .bytes(descriptor)
              .finish(),
          ),
        );
      },
    );
    const client = new YasRelayClient(connection);
    Object.defineProperty(client, "limits", {
      value: {
        maxRoutes: 4,
        maxLinksPerSession: 4,
        maxPendingConnects: 4,
        maxEarlyData: 0,
        connectTimeoutNs: 1n,
        maxBufferedPerLink: 1024n,
      },
    });
    Object.defineProperty(client, "transfers", {
      value: {
        reserveReceiveCredit: vi.fn(() => ({
          bytes: 64n * 1024n,
          release: vi.fn(),
        })),
        acceptServerDescriptor: vi.fn(() => transfers.shift()!),
      },
    });

    await client.connect(route);
    await expect(client.connect(route)).rejects.toThrow(/reused/);
    expect(secondReset).toHaveBeenCalledOnce();
    expect(firstReset).not.toHaveBeenCalled();

    client.dispose();
    expect(firstReset).toHaveBeenCalledOnce();
  });

  it("resets a direct Extension staging upload on client disposal", async () => {
    const { connection, request } = productConnection();
    const reset = vi.fn();
    const transfer = {
      reset,
      closeWrite: vi.fn(),
      closed: Promise.resolve(),
      subscribeTerminal: vi.fn(() => () => undefined),
      subscribeReset: vi.fn(() => () => undefined),
    };
    const requestDecoded = connection.requestDecoded as ReturnType<
      typeof vi.fn
    >;
    requestDecoded.mockResolvedValue({
      disposition: YAS_EXTENSION_OBJECT_UPLOAD,
      stagingHandle: 0x1234n,
      descriptor: {
        uploadStage: {
          stagingHandle: 0x1234n,
          expiresServerNs: 10_000n,
        },
      },
      extensions: [],
    });
    const client = new YasExtensionClient(connection);
    Object.defineProperty(client, "transfers", {
      value: {
        acceptServerUploadDescriptor: vi.fn(() => transfer),
      },
    });

    const upload = await client.beginObject({
      operationId: new Uint8Array(16).fill(1),
      contentHash: new Uint8Array(32).fill(2),
      byteLength: 1n,
    });
    upload!.transfer.closeWrite();
    await Promise.resolve();
    const commitResult = deferred<Uint8Array>();
    request.mockReturnValueOnce(commitResult.promise);
    const committing = client.commitObject({
      stagingHandle: 0x1234n,
      operationId: new Uint8Array(16).fill(3),
      contentHash: new Uint8Array(32).fill(2),
      byteLength: 1n,
    });
    client.dispose();

    expect(reset).toHaveBeenCalledOnce();
    commitResult.resolve(new Uint8Array());
    await committing;
  });

  it("isolates Extension attempt observers and removes both on disposal", () => {
    const { connection, eventHandlers } = productConnection();
    const client = new YasExtensionClient(connection);
    const sibling = vi.fn();
    const baseline =
      eventHandlers.get(YAS_EXTENSION_ATTEMPT_CONTEXT)?.size ?? 0;
    client.onAttemptContext(() => {
      throw new Error("observer failed");
    });
    client.onAttemptContext(sibling);
    const handlers = eventHandlers.get(YAS_EXTENSION_ATTEMPT_CONTEXT)!;

    for (const handler of [...handlers])
      handler({ payload: goldenBytes("extension.attempt_context.payload") });
    expect(sibling).toHaveBeenCalledOnce();

    client.dispose();
    expect(handlers.size).toBe(baseline);
  });
});
