import { describe, expect, it, vi } from "vitest";

import {
  YAS_CHANNEL_ACCEPT,
  YAS_CHANNEL_CHANNEL_CONTENT_KIND,
  YAS_CHANNEL_CLOSE_LISTENER,
  YAS_CHANNEL_LISTEN,
  YAS_CHANNEL_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_CHANNEL_VERSION,
  YAS_EVENTS_RECORD,
  YAS_EVENTS_START_STREAM,
  YAS_EVENTS_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_EVENTS_STOP_STREAM,
  YAS_EVENTS_STREAM_STOPPED,
  YAS_EXTENSION_OBJECT_BEGIN,
  YAS_EXTENSION_OBJECT_ALREADY_PRESENT,
  YAS_EXTENSION_OBJECT_CONTENT_KIND,
  YAS_EXTENSION_OBJECT_UPLOAD,
  YAS_EXTENSION_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_EXTENSION_VERSION,
  YAS_FAMILY_CHANNEL,
  YAS_FAMILY_EVENTS,
  YAS_FAMILY_EXTENSION,
  YAS_FAMILY_NET,
  YAS_NET_CLOSE,
  YAS_NET_DELIVERY_NOT_APPLICABLE,
  YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
  YAS_NET_DIRECTION_DUPLEX,
  YAS_NET_DROP_NOT_APPLICABLE,
  YAS_NET_FLOW_CONTENT_KIND,
  YAS_NET_MODE_BYTE,
  YAS_NET_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_NET_OPEN,
  YAS_NET_VERSION,
  YAS_STATUS_OK,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YAS_STATUS_STALE,
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
  YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
  YasChannelClient,
  YasEventsClient,
  YasExtensionClient,
  YasNetClient,
  YasResultError,
  YasWriter,
  encodeChannelAccept,
  encodeChannelIdentity,
  encodeEventsRecordEvent,
  encodeEventsStreamStarted,
  encodeEventsStreamStopped,
  encodeExtensionObjectBeginResult,
  encodeNetEndpoint,
  type YasChannelEndpoint,
  type YasConnection,
  type YasInvalidation,
  type YasNetEndpoint,
  type YasReceiveBudgetLease,
  type YasTransfer,
  type YasTransferDescriptor,
} from "../yas";

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(error: unknown): void;
}

interface PendingDecodedRequest {
  family: number;
  kind: number;
  payload: Uint8Array;
  decode(body: Uint8Array): unknown;
  result: Deferred<unknown>;
  settled: boolean;
}

interface PlainRequest {
  family: number;
  kind: number;
  payload: Uint8Array;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

function replayConnection(replayLimit = 64): {
  connection: YasConnection;
  decoded: PendingDecodedRequest[];
  plain: PlainRequest[];
  invalidations: Set<(value: YasInvalidation) => void>;
  dispatch(family: number, kind: number, payload: Uint8Array): void;
  setRemaining(value: bigint): void;
} {
  const decoded: PendingDecodedRequest[] = [];
  const plain: PlainRequest[] = [];
  const invalidations = new Set<(value: YasInvalidation) => void>();
  const eventHandlers = new Map<
    string,
    Set<(event: { payload: Uint8Array; datagram: boolean }) => void>
  >();
  let remaining = 1n;
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
                  value: new YasWriter().u32(replayLimit).finish(),
                },
              ],
      };
    }),
    registerFamilyLimitValidator: vi.fn(),
    onEvent(
      family: number,
      kind: number,
      listener: (event: { payload: Uint8Array; datagram: boolean }) => void,
    ) {
      const key = `${family}:${kind}`;
      let listeners = eventHandlers.get(key);
      if (!listeners) {
        listeners = new Set();
        eventHandlers.set(key, listeners);
      }
      listeners.add(listener);
      return () => listeners!.delete(listener);
    },
    onInvalidation(listener: (value: YasInvalidation) => void) {
      invalidations.add(listener);
      return () => invalidations.delete(listener);
    },
    transport: {
      status: "connected",
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    },
    receiveBudget: {
      reserve(bytes: bigint): YasReceiveBudgetLease {
        return { bytes, release: vi.fn() } as unknown as YasReceiveBudgetLease;
      },
    },
    request: vi.fn(
      async (family: number, kind: number, payload: Uint8Array) => {
        plain.push({ family, kind, payload: new Uint8Array(payload) });
        return new Uint8Array();
      },
    ),
    requestDecoded: vi.fn(
      (
        family: number,
        kind: number,
        payload: Uint8Array,
        decode: (body: Uint8Array) => unknown,
      ) => {
        const result = deferred<unknown>();
        decoded.push({
          family,
          kind,
          payload: new Uint8Array(payload),
          decode,
          result,
          settled: false,
        });
        return result.promise;
      },
    ),
    sendEvent: vi.fn(),
    sendDatagramEvent: vi.fn(() => true),
    nanosecondsUntilServerTime: vi.fn(() => remaining),
  } as unknown as YasConnection;
  return {
    connection,
    decoded,
    plain,
    invalidations,
    dispatch(family, kind, payload) {
      for (const listener of eventHandlers.get(`${family}:${kind}`) ?? [])
        listener({ payload, datagram: false });
    },
    setRemaining(value) {
      remaining = value;
    },
  };
}

function settleDecoded(
  request: PendingDecodedRequest,
  body: Uint8Array,
  afterDecode?: (value: unknown) => void,
): unknown {
  const value = request.decode(body);
  afterDecode?.(value);
  request.settled = true;
  request.result.resolve(value);
  return value;
}

function rejectDecoded(request: PendingDecodedRequest, status: number): void {
  request.settled = true;
  request.result.reject(new YasResultError(status, new Uint8Array()));
}

function settleDecodedWithError(
  request: PendingDecodedRequest,
  body: Uint8Array,
): unknown {
  request.settled = true;
  try {
    const value = request.decode(body);
    request.result.resolve(value);
    return undefined;
  } catch (error) {
    request.result.reject(error);
    return error;
  }
}

function invalidate(
  state: ReturnType<typeof replayConnection>,
  family: number,
): void {
  for (const listener of [...state.invalidations]) listener({ family });
}

function rejectNextPlain(
  state: ReturnType<typeof replayConnection>,
  status: number,
): void {
  const request = state.connection.request as unknown as {
    mockImplementationOnce(
      implementation: (
        family: number,
        kind: number,
        payload: Uint8Array,
      ) => Promise<Uint8Array>,
    ): void;
  };
  request.mockImplementationOnce(async (family, kind, payload) => {
    state.plain.push({ family, kind, payload: new Uint8Array(payload) });
    throw new YasResultError(status, new Uint8Array());
  });
}

function requestsOf(
  requests: readonly PendingDecodedRequest[],
  family: number,
  kind: number,
): PendingDecodedRequest[] {
  return requests.filter(
    (request) => request.family === family && request.kind === kind,
  );
}

class FakeTransfer {
  private readonly closedState = deferred<void>();
  private readonly terminalListeners = new Set<() => void>();
  private readonly resetListeners = new Set<() => void>();
  private terminalObserved = false;
  private resetObserved = false;
  readonly closed = this.closedState.promise;
  readonly reset = vi.fn(() => {
    if (this.resetObserved) return;
    this.resetObserved = true;
    this.closeDirection();
    for (const listener of [...this.resetListeners]) listener();
    this.resetListeners.clear();
    this.closedState.resolve(undefined);
  });

  constructor(readonly descriptor: YasTransferDescriptor) {}

  readonly closeWrite = vi.fn(() => this.closeDirection());

  closeDirection(): void {
    if (this.terminalObserved) return;
    this.terminalObserved = true;
    for (const listener of [...this.terminalListeners]) listener();
    this.terminalListeners.clear();
  }

  subscribeTerminal(listener: () => void): () => void {
    if (this.terminalObserved) {
      listener();
      return () => undefined;
    }
    this.terminalListeners.add(listener);
    return () => this.terminalListeners.delete(listener);
  }

  subscribeReset(listener: () => void): () => void {
    if (this.resetObserved) {
      listener();
      return () => undefined;
    }
    this.resetListeners.add(listener);
    return () => this.resetListeners.delete(listener);
  }
}

function fakeTransfers(client: object): {
  accepted: FakeTransfer[];
  reserves: Array<{ preferred: bigint; minimum: bigint }>;
} {
  const accepted: FakeTransfer[] = [];
  const reserves: Array<{ preferred: bigint; minimum: bigint }> = [];
  Object.defineProperty(client, "transfers", {
    value: {
      reserveReceiveCredit(preferred: bigint, minimum = 1n) {
        reserves.push({ preferred, minimum });
        const bytes = preferred === minimum ? preferred : preferred / 2n;
        return {
          bytes,
          release: vi.fn(),
        } as unknown as YasReceiveBudgetLease;
      },
      acceptServerDescriptor(descriptor: YasTransferDescriptor) {
        const transfer = new FakeTransfer(descriptor);
        accepted.push(transfer);
        return transfer as unknown as YasTransfer;
      },
      acceptServerUploadDescriptor(descriptor: YasTransferDescriptor) {
        const transfer = new FakeTransfer(descriptor);
        accepted.push(transfer);
        return transfer as unknown as YasTransfer;
      },
    },
  });
  return { accepted, reserves };
}

function operationId(byte: number): Uint8Array {
  return new Uint8Array(16).fill(byte);
}

function sensitiveExtension() {
  return {
    tag: YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
    required: true,
    value: new Uint8Array(),
  } as const;
}

function channelDescriptor(transferId = 2): YasTransferDescriptor {
  return {
    transferId,
    mode: YAS_TRANSFER_MODE_MESSAGE,
    direction:
      YAS_TRANSFER_RECEIVER_TO_SENDER | YAS_TRANSFER_SENDER_TO_RECEIVER,
    flags: 0,
    receiverSendCredit: 1024n,
    senderSendCredit: 0n,
    maxItemBytes: 1024n,
    maxChunkBytes: 1024,
    contentFamily: YAS_FAMILY_CHANNEL,
    contentKind: YAS_CHANNEL_CHANNEL_CONTENT_KIND,
    contentVersion: YAS_CHANNEL_VERSION,
    extensions: [sensitiveExtension()],
    maxOpenMessages: 1,
    sensitiveContent: true,
  };
}

function channelEndpoint(): YasChannelEndpoint {
  return {
    channelHandle: 11n,
    peerChannelHandle: 12n,
    peerSession: new Uint8Array(16).fill(0x33),
    listenerMetadata: new Uint8Array(),
    connectorMetadata: new Uint8Array(),
    descriptor: channelDescriptor(),
    extensions: [],
  };
}

function netDescriptor(transferId = 4): YasTransferDescriptor {
  return {
    transferId,
    mode: YAS_TRANSFER_MODE_BYTE,
    direction: YAS_NET_DIRECTION_DUPLEX,
    flags: 0,
    receiverSendCredit: 1024n,
    senderSendCredit: 0n,
    maxItemBytes: 0n,
    maxChunkBytes: 1024,
    contentFamily: YAS_FAMILY_NET,
    contentKind: YAS_NET_FLOW_CONTENT_KIND,
    contentVersion: YAS_NET_VERSION,
    extensions: [sensitiveExtension()],
    maxOpenMessages: 1,
    sensitiveContent: true,
  };
}

function netEndpoint(): YasNetEndpoint {
  return {
    flowHandle: 21n,
    mode: YAS_NET_MODE_BYTE,
    direction: YAS_NET_DIRECTION_DUPLEX,
    selectedDelivery: YAS_NET_DELIVERY_NOT_APPLICABLE,
    maxDatagramPayload: 0,
    serverInstanceLimit: 1,
    maxMessageBytes: 0n,
    peerAddress: { kind: "tcp", host: "127.0.0.1", port: 443 },
    negotiatedAlpn: new Uint8Array(),
    descriptor: netDescriptor(),
    extensions: [],
  };
}

function extensionDescriptor(
  stagingHandle: bigint,
  expiresServerNs: bigint,
  transferId = 6,
): YasTransferDescriptor {
  const uploadStage = { stagingHandle, expiresServerNs };
  return {
    transferId,
    mode: YAS_TRANSFER_MODE_BYTE,
    direction: YAS_TRANSFER_RECEIVER_TO_SENDER,
    flags: 0,
    receiverSendCredit: 4n,
    senderSendCredit: 0n,
    maxItemBytes: 0n,
    maxChunkBytes: 1024,
    contentFamily: YAS_FAMILY_EXTENSION,
    contentKind: YAS_EXTENSION_OBJECT_CONTENT_KIND,
    contentVersion: YAS_EXTENSION_VERSION,
    extensions: [
      sensitiveExtension(),
      {
        tag: YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
        required: true,
        value: new YasWriter().u64(stagingHandle).u64(expiresServerNs).finish(),
      },
    ],
    maxOpenMessages: 1,
    sensitiveContent: true,
    uploadStage,
  };
}

describe("remaining client exact-operation replay ownership", () => {
  it("coalesces Channel LISTEN, installs before ACCEPT, and tombstones close", async () => {
    const state = replayConnection();
    const client = new YasChannelClient(state.connection);
    const { accepted } = fakeTransfers(client);
    const id = operationId(0x11);
    const onAccept = vi.fn();
    const listen = () =>
      client.listen("yas.replay.v1", {
        operationId: id,
        metadata: new Uint8Array([1]),
        onAccept,
      });

    const first = listen();
    const concurrent = listen();
    expect(concurrent).toBe(first);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_CHANNEL, YAS_CHANNEL_LISTEN),
    ).toHaveLength(1);
    expect(() =>
      client.listen("yas.replay.v1", {
        operationId: id,
        metadata: new Uint8Array([2]),
        onAccept,
      }),
    ).toThrow(/different payload/);

    const listenRequest = requestsOf(
      state.decoded,
      YAS_FAMILY_CHANNEL,
      YAS_CHANNEL_LISTEN,
    )[0]!;
    settleDecoded(listenRequest, encodeChannelIdentity(7n, 9n), () =>
      state.dispatch(
        YAS_FAMILY_CHANNEL,
        YAS_CHANNEL_ACCEPT,
        encodeChannelAccept(7n, 9n, channelEndpoint()),
      ),
    );
    expect(onAccept).toHaveBeenCalledOnce();
    expect(accepted).toHaveLength(1);
    const listener = await first;
    expect(await concurrent).toBe(listener);
    expect(await listen()).toBe(listener);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_CHANNEL, YAS_CHANNEL_LISTEN),
    ).toHaveLength(1);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_CHANNEL, YAS_CHANNEL_CLOSE_LISTENER),
    ).toHaveLength(0);

    const closing = listener.close();
    const closeRequest = requestsOf(
      state.decoded,
      YAS_FAMILY_CHANNEL,
      YAS_CHANNEL_CLOSE_LISTENER,
    )[0]!;
    settleDecoded(closeRequest, new Uint8Array());
    await closing;
    const stale = listen();
    const staleRequest = requestsOf(
      state.decoded,
      YAS_FAMILY_CHANNEL,
      YAS_CHANNEL_LISTEN,
    )[1]!;
    rejectDecoded(staleRequest, YAS_STATUS_STALE);
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    expect(
      (client as unknown as { activeListeners: ReadonlyMap<bigint, unknown> })
        .activeListeners.size,
    ).toBe(0);

    for (let index = 0; index < 8; index++) {
      const failed = client.listen(`yas.failed-${index}.v1`, {
        operationId: operationId(0x20 + index),
        onAccept,
      });
      rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_RESOURCE_EXHAUSTED);
      await expect(failed).rejects.toMatchObject({
        status: YAS_STATUS_RESOURCE_EXHAUSTED,
      });
    }
    expect(
      (client as unknown as { listenOperations: ReadonlyMap<string, unknown> })
        .listenOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("coalesces Events START_STREAM, installs before RECORD, and retires naturally", async () => {
    const state = replayConnection();
    const client = new YasEventsClient(state.connection);
    const id = operationId(0x41);
    const start = () =>
      client.startStream({
        operationId: id,
        history: true,
        startSequence: 5n,
        maxBatchBytes: 4096,
      });

    const first = start();
    const concurrent = start();
    expect(concurrent).toBe(first);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_EVENTS, YAS_EVENTS_START_STREAM),
    ).toHaveLength(1);
    expect(() =>
      client.startStream({
        operationId: id,
        history: true,
        startSequence: 6n,
        maxBatchBytes: 4096,
      }),
    ).toThrow(/different payload/);

    settleDecoded(
      requestsOf(state.decoded, YAS_FAMILY_EVENTS, YAS_EVENTS_START_STREAM)[0]!,
      encodeEventsStreamStarted({
        streamHandle: 31n,
        firstSequence: 5n,
        maxBatchBytes: 4096,
        extensions: [],
      }),
      () =>
        state.dispatch(
          YAS_FAMILY_EVENTS,
          YAS_EVENTS_RECORD,
          encodeEventsRecordEvent({
            streamHandle: 31n,
            batch: {
              firstSequence: 5n,
              records: [
                {
                  sequence: 5n,
                  monotonicNs: 1n,
                  eventId: 0,
                  required: false,
                  eventFlags: 0,
                  payload: new Uint8Array([1]),
                },
              ],
            },
          }),
        ),
    );
    const stream = await first;
    expect(await concurrent).toBe(stream);
    expect((await stream.next())?.type).toBe("records");
    expect(await start()).toBe(stream);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_EVENTS, YAS_EVENTS_START_STREAM),
    ).toHaveLength(1);
    expect(
      state.plain.filter((request) => request.kind === YAS_EVENTS_STOP_STREAM),
    ).toHaveLength(0);

    state.dispatch(
      YAS_FAMILY_EVENTS,
      YAS_EVENTS_STREAM_STOPPED,
      encodeEventsStreamStopped({
        streamHandle: 31n,
        status: YAS_STATUS_OK,
        detail: "done",
        extensions: [],
      }),
    );
    const stale = start();
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_STALE);
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    expect(
      (client as unknown as { streams: ReadonlyMap<bigint, unknown> }).streams
        .size,
    ).toBe(0);

    for (let index = 0; index < 8; index++) {
      const failed = client.startStream({
        operationId: operationId(0x50 + index),
        maxBatchBytes: 4096,
      });
      rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_RESOURCE_EXHAUSTED);
      await expect(failed).rejects.toMatchObject({
        status: YAS_STATUS_RESOURCE_EXHAUSTED,
      });
    }
    expect(
      (client as unknown as { startOperations: ReadonlyMap<string, unknown> })
        .startOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("coalesces Net OPEN with one lease/Transfer and tombstones CLOSE", async () => {
    const state = replayConnection();
    const client = new YasNetClient(state.connection);
    const { accepted, reserves } = fakeTransfers(client);
    const id = operationId(0x71);
    const value = {
      operationId: id,
      address: { kind: "tcp" as const, host: "127.0.0.1", port: 443 },
      deliveryPreference: YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
      dropPolicy: YAS_NET_DROP_NOT_APPLICABLE,
    };
    const open = () => client.open(value, 1024n);

    const first = open();
    const concurrent = open();
    expect(concurrent).toBe(first);
    expect(reserves).toEqual([{ preferred: 1024n, minimum: 1n }]);
    expect(() =>
      client.open({ ...value, earlyData: new Uint8Array([1]) }, 1024n),
    ).toThrow(/different payload/);
    settleDecoded(
      requestsOf(state.decoded, YAS_FAMILY_NET, YAS_NET_OPEN)[0]!,
      encodeNetEndpoint(netEndpoint()),
      (flow) => {
        expect(
          (
            client as unknown as { flows: ReadonlyMap<bigint, unknown> }
          ).flows.get(21n),
        ).toBe(flow);
        expect(accepted).toHaveLength(1);
      },
    );
    const flow = await first;
    expect(await concurrent).toBe(flow);
    expect(await open()).toBe(flow);
    expect(accepted).toHaveLength(1);
    expect(reserves).toHaveLength(1);
    expect(
      state.plain.filter((request) => request.kind === YAS_NET_CLOSE),
    ).toHaveLength(0);
    expect(accepted[0]!.reset).not.toHaveBeenCalled();

    await flow.close(operationId(0x72));
    expect(accepted[0]!.reset).toHaveBeenCalledOnce();
    expect(
      state.plain.filter((request) => request.kind === YAS_NET_CLOSE),
    ).toHaveLength(1);
    const stale = open();
    expect(reserves).toEqual([
      { preferred: 1024n, minimum: 1n },
      { preferred: 512n, minimum: 512n },
    ]);
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_STALE);
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    expect(accepted).toHaveLength(1);

    for (let index = 0; index < 8; index++) {
      const failed = client.open(
        { ...value, operationId: operationId(0x80 + index) },
        1024n,
      );
      rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_RESOURCE_EXHAUSTED);
      await expect(failed).rejects.toMatchObject({
        status: YAS_STATUS_RESOURCE_EXHAUSTED,
      });
    }
    expect(
      (client as unknown as { openOperations: ReadonlyMap<string, unknown> })
        .openOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("coalesces Extension OBJECT_BEGIN and tombstones an expired stage", async () => {
    const state = replayConnection();
    const client = new YasExtensionClient(state.connection);
    const { accepted } = fakeTransfers(client);
    const id = operationId(0xa1);
    const value = {
      operationId: id,
      contentHash: new Uint8Array(32).fill(0x22),
      byteLength: 4n,
    };
    const begin = () => client.beginObject(value);

    const first = begin();
    const concurrent = begin();
    expect(concurrent).toBe(first);
    expect(
      requestsOf(
        state.decoded,
        YAS_FAMILY_EXTENSION,
        YAS_EXTENSION_OBJECT_BEGIN,
      ),
    ).toHaveLength(1);
    expect(() => client.beginObject({ ...value, byteLength: 5n })).toThrow(
      /different payload/,
    );
    settleDecoded(
      requestsOf(
        state.decoded,
        YAS_FAMILY_EXTENSION,
        YAS_EXTENSION_OBJECT_BEGIN,
      )[0]!,
      encodeExtensionObjectBeginResult({
        disposition: YAS_EXTENSION_OBJECT_UPLOAD,
        stagingHandle: 41n,
        descriptor: extensionDescriptor(41n, 10_000n),
        extensions: [],
      }),
      () => expect(accepted).toHaveLength(1),
    );
    const upload = await first;
    expect(upload).not.toBeNull();
    expect(await concurrent).toBe(upload);
    expect(await begin()).toBe(upload);
    expect(accepted).toHaveLength(1);
    expect(accepted[0]!.reset).not.toHaveBeenCalled();

    state.setRemaining(0n);
    const stale = begin();
    expect(accepted[0]!.reset).toHaveBeenCalledOnce();
    expect(
      requestsOf(
        state.decoded,
        YAS_FAMILY_EXTENSION,
        YAS_EXTENSION_OBJECT_BEGIN,
      ),
    ).toHaveLength(2);
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_STALE);
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    expect(accepted).toHaveLength(1);
    state.setRemaining(1n);

    for (let index = 0; index < 8; index++) {
      const failed = client.beginObject({
        ...value,
        operationId: operationId(0xb0 + index),
      });
      rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_RESOURCE_EXHAUSTED);
      await expect(failed).rejects.toMatchObject({
        status: YAS_STATUS_RESOURCE_EXHAUSTED,
      });
    }
    expect(
      (
        client as unknown as {
          objectBeginOperations: ReadonlyMap<string, unknown>;
        }
      ).objectBeginOperations.size,
    ).toBe(1);
    expect(
      (client as unknown as { stagingUploads: ReadonlyMap<bigint, unknown> })
        .stagingUploads.size,
    ).toBe(0);
    client.dispose();
  });

  it("bounds Channel settlements, pins live listeners, and preserves a tombstone across a failed successor", async () => {
    const state = replayConnection(1);
    const client = new YasChannelClient(state.connection);
    const onAccept = vi.fn();
    const firstId = operationId(0xc1);
    const secondId = operationId(0xc2);
    const firstPending = client.listen("yas.bound-first.v1", {
      operationId: firstId,
      onAccept,
    });
    settleDecoded(
      requestsOf(state.decoded, YAS_FAMILY_CHANNEL, YAS_CHANNEL_LISTEN)[0]!,
      encodeChannelIdentity(101n, 1n),
    );
    const first = await firstPending;

    expect(() =>
      client.listen("yas.bound-second.v1", {
        operationId: secondId,
        onAccept,
      }),
    ).toThrow(YasResultError);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_CHANNEL, YAS_CHANNEL_LISTEN),
    ).toHaveLength(1);

    const closing = first.close();
    settleDecoded(
      requestsOf(
        state.decoded,
        YAS_FAMILY_CHANNEL,
        YAS_CHANNEL_CLOSE_LISTENER,
      )[0]!,
      new Uint8Array(),
    );
    await closing;
    const failed = client.listen("yas.bound-second.v1", {
      operationId: secondId,
      onAccept,
    });
    expect(
      (client as unknown as { listenOperations: ReadonlyMap<string, unknown> })
        .listenOperations.size,
    ).toBe(1);
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_RESOURCE_EXHAUSTED);
    await expect(failed).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    expect(() =>
      client.listen("yas.changed-first.v1", {
        operationId: firstId,
        onAccept,
      }),
    ).toThrow(/different payload/);

    const replacementPending = client.listen("yas.bound-second.v1", {
      operationId: secondId,
      onAccept,
    });
    settleDecoded(state.decoded.at(-1)!, encodeChannelIdentity(102n, 1n));
    await replacementPending;
    expect(
      (client as unknown as { listenOperations: ReadonlyMap<string, unknown> })
        .listenOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("bounds Events settlements, pins live streams, and preserves a tombstone across a failed successor", async () => {
    const state = replayConnection(1);
    const client = new YasEventsClient(state.connection);
    const firstId = operationId(0xc3);
    const secondId = operationId(0xc4);
    const firstPending = client.startStream({
      operationId: firstId,
      history: true,
      startSequence: 3n,
      maxBatchBytes: 64,
    });
    settleDecoded(
      state.decoded.at(-1)!,
      encodeEventsStreamStarted({
        streamHandle: 103n,
        firstSequence: 3n,
        maxBatchBytes: 64,
        extensions: [],
      }),
    );
    await firstPending;

    expect(() =>
      client.startStream({
        operationId: secondId,
        maxBatchBytes: 64,
      }),
    ).toThrow(YasResultError);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_EVENTS, YAS_EVENTS_START_STREAM),
    ).toHaveLength(1);

    state.dispatch(
      YAS_FAMILY_EVENTS,
      YAS_EVENTS_STREAM_STOPPED,
      encodeEventsStreamStopped({
        streamHandle: 103n,
        status: YAS_STATUS_OK,
        detail: "done",
        extensions: [],
      }),
    );
    const failed = client.startStream({
      operationId: secondId,
      maxBatchBytes: 64,
    });
    expect(
      (client as unknown as { startOperations: ReadonlyMap<string, unknown> })
        .startOperations.size,
    ).toBe(1);
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_RESOURCE_EXHAUSTED);
    await expect(failed).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    expect(() =>
      client.startStream({
        operationId: firstId,
        history: true,
        startSequence: 4n,
        maxBatchBytes: 64,
      }),
    ).toThrow(/different payload/);

    const replacementPending = client.startStream({
      operationId: secondId,
      maxBatchBytes: 64,
    });
    settleDecoded(
      state.decoded.at(-1)!,
      encodeEventsStreamStarted({
        streamHandle: 104n,
        firstSequence: 0n,
        maxBatchBytes: 64,
        extensions: [],
      }),
    );
    await replacementPending;
    expect(
      (client as unknown as { startOperations: ReadonlyMap<string, unknown> })
        .startOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("bounds Net settlements, pins live flows, and preserves a tombstone across a failed successor", async () => {
    const state = replayConnection(1);
    const client = new YasNetClient(state.connection);
    fakeTransfers(client);
    const firstId = operationId(0xc5);
    const secondId = operationId(0xc6);
    const openValue = (operationId: Uint8Array) => ({
      operationId,
      address: { kind: "tcp" as const, host: "127.0.0.1", port: 443 },
      deliveryPreference: YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
      dropPolicy: YAS_NET_DROP_NOT_APPLICABLE,
    });
    const firstPending = client.open(openValue(firstId), 1024n);
    settleDecoded(state.decoded.at(-1)!, encodeNetEndpoint(netEndpoint()));
    const first = await firstPending;

    expect(() => client.open(openValue(secondId), 1024n)).toThrow(
      YasResultError,
    );
    expect(
      requestsOf(state.decoded, YAS_FAMILY_NET, YAS_NET_OPEN),
    ).toHaveLength(1);

    await first.close(operationId(0xc7));
    const failed = client.open(openValue(secondId), 1024n);
    expect(
      (client as unknown as { openOperations: ReadonlyMap<string, unknown> })
        .openOperations.size,
    ).toBe(1);
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_RESOURCE_EXHAUSTED);
    await expect(failed).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    expect(() =>
      client.open(
        { ...openValue(firstId), earlyData: new Uint8Array([1]) },
        1024n,
      ),
    ).toThrow(/different payload/);

    const replacementPending = client.open(openValue(secondId), 1024n);
    settleDecoded(
      state.decoded.at(-1)!,
      encodeNetEndpoint({
        ...netEndpoint(),
        flowHandle: 105n,
        descriptor: netDescriptor(8),
      }),
    );
    await replacementPending;
    expect(
      (client as unknown as { openOperations: ReadonlyMap<string, unknown> })
        .openOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("bounds Extension settlements, pins live stages, and preserves a tombstone across a failed successor", async () => {
    const state = replayConnection(1);
    const client = new YasExtensionClient(state.connection);
    const { accepted } = fakeTransfers(client);
    const firstId = operationId(0xc8);
    const secondId = operationId(0xc9);
    const objectValue = (operationId: Uint8Array) => ({
      operationId,
      contentHash: new Uint8Array(32).fill(0x44),
      byteLength: 4n,
    });
    const firstPending = client.beginObject(objectValue(firstId));
    settleDecoded(
      state.decoded.at(-1)!,
      encodeExtensionObjectBeginResult({
        disposition: YAS_EXTENSION_OBJECT_UPLOAD,
        stagingHandle: 106n,
        descriptor: extensionDescriptor(106n, 10_000n, 9),
        extensions: [],
      }),
    );
    await firstPending;

    expect(() => client.beginObject(objectValue(secondId))).toThrow(
      YasResultError,
    );
    expect(
      requestsOf(
        state.decoded,
        YAS_FAMILY_EXTENSION,
        YAS_EXTENSION_OBJECT_BEGIN,
      ),
    ).toHaveLength(1);

    accepted[0]!.reset();
    const failed = client.beginObject(objectValue(secondId));
    expect(
      (
        client as unknown as {
          objectBeginOperations: ReadonlyMap<string, unknown>;
        }
      ).objectBeginOperations.size,
    ).toBe(1);
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_RESOURCE_EXHAUSTED);
    await expect(failed).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    expect(() =>
      client.beginObject({ ...objectValue(firstId), byteLength: 5n }),
    ).toThrow(/different payload/);

    const replacementPending = client.beginObject(objectValue(secondId));
    settleDecoded(
      state.decoded.at(-1)!,
      encodeExtensionObjectBeginResult({
        disposition: YAS_EXTENSION_OBJECT_ALREADY_PRESENT,
        stagingHandle: 0n,
        extensions: [],
      }),
    );
    await expect(replacementPending).resolves.toBeNull();
    expect(
      (
        client as unknown as {
          objectBeginOperations: ReadonlyMap<string, unknown>;
        }
      ).objectBeginOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("keeps an Events stream live after failed STOP and retires queue overflow", async () => {
    const state = replayConnection();
    const client = new YasEventsClient(state.connection);
    const id = operationId(0xd1);
    const start = () =>
      client.startStream({ operationId: id, maxBatchBytes: 1 });
    const pending = start();
    settleDecoded(
      state.decoded.at(-1)!,
      encodeEventsStreamStarted({
        streamHandle: 201n,
        firstSequence: 0n,
        maxBatchBytes: 1,
        extensions: [],
      }),
    );
    const stream = await pending;

    rejectNextPlain(state, YAS_STATUS_RESOURCE_EXHAUSTED);
    await expect(stream.stop(operationId(0xd2))).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    expect(await start()).toBe(stream);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_EVENTS, YAS_EVENTS_START_STREAM),
    ).toHaveLength(1);

    for (let sequence = 0; sequence < 7; sequence++)
      state.dispatch(
        YAS_FAMILY_EVENTS,
        YAS_EVENTS_RECORD,
        encodeEventsRecordEvent({
          streamHandle: 201n,
          batch: {
            firstSequence: BigInt(sequence),
            records: [
              {
                sequence: BigInt(sequence),
                monotonicNs: BigInt(sequence + 1),
                eventId: 0,
                required: false,
                eventFlags: 0,
                payload: new Uint8Array(),
              },
            ],
          },
        }),
      );
    await expect(stream.next()).rejects.toThrow(/queue limit/);
    expect(
      state.plain.filter((request) => request.kind === YAS_EVENTS_STOP_STREAM),
    ).toHaveLength(2);
    const stale = start();
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_STALE);
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    client.dispose();
  });

  it("retires a Net flow coherently after failed CLOSE and cleans an unexpected exact OPEN replay", async () => {
    const state = replayConnection();
    const client = new YasNetClient(state.connection);
    const { accepted } = fakeTransfers(client);
    const id = operationId(0xd3);
    const value = {
      operationId: id,
      address: { kind: "tcp" as const, host: "127.0.0.1", port: 443 },
      deliveryPreference: YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
      dropPolicy: YAS_NET_DROP_NOT_APPLICABLE,
    };
    const pending = client.open(value, 1024n);
    settleDecoded(state.decoded.at(-1)!, encodeNetEndpoint(netEndpoint()));
    const flow = await pending;

    rejectNextPlain(state, YAS_STATUS_RESOURCE_EXHAUSTED);
    await expect(flow.close(operationId(0xd4))).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    expect(accepted[0]!.reset).toHaveBeenCalledOnce();
    expect(
      (client as unknown as { flows: ReadonlyMap<bigint, unknown> }).flows.size,
    ).toBe(0);

    const replay = client.open(value, 1024n);
    const error = settleDecodedWithError(
      state.decoded.at(-1)!,
      encodeNetEndpoint(netEndpoint()),
    );
    expect(error).toBeInstanceOf(Error);
    await expect(replay).rejects.toThrow(/retired flow/);
    expect(accepted).toHaveLength(1);
    expect(
      state.plain.filter((request) => request.kind === YAS_NET_CLOSE),
    ).toHaveLength(2);
    client.dispose();
  });

  it("tombstones Extension replay identity on directional CLOSE while preserving commit authority", async () => {
    const state = replayConnection(1);
    const client = new YasExtensionClient(state.connection);
    const { accepted } = fakeTransfers(client);
    const value = {
      operationId: operationId(0xd5),
      contentHash: new Uint8Array(32).fill(0x55),
      byteLength: 4n,
    };
    const pending = client.beginObject(value);
    settleDecoded(
      state.decoded.at(-1)!,
      encodeExtensionObjectBeginResult({
        disposition: YAS_EXTENSION_OBJECT_UPLOAD,
        stagingHandle: 202n,
        descriptor: extensionDescriptor(202n, 10_000n, 10),
        extensions: [],
      }),
    );
    await pending;

    accepted[0]!.closeDirection();
    expect(
      (client as unknown as { stagingUploads: ReadonlyMap<bigint, unknown> })
        .stagingUploads.size,
    ).toBe(1);
    const stale = client.beginObject(value);
    rejectDecoded(state.decoded.at(-1)!, YAS_STATUS_STALE);
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    expect(accepted[0]!.reset).not.toHaveBeenCalled();

    const successor = client.beginObject({
      ...value,
      operationId: operationId(0xd7),
    });
    settleDecoded(
      state.decoded.at(-1)!,
      encodeExtensionObjectBeginResult({
        disposition: YAS_EXTENSION_OBJECT_ALREADY_PRESENT,
        stagingHandle: 0n,
        extensions: [],
      }),
    );
    await expect(successor).resolves.toBeNull();
    expect(
      (
        client as unknown as {
          objectBeginOperations: ReadonlyMap<string, unknown>;
        }
      ).objectBeginOperations.size,
    ).toBe(1);
    expect(
      (client as unknown as { stagingUploads: ReadonlyMap<bigint, unknown> })
        .stagingUploads.size,
    ).toBe(1);

    await client.commitObject({
      stagingHandle: 202n,
      operationId: operationId(0xd6),
      contentHash: value.contentHash,
      byteLength: value.byteLength,
    });
    expect(
      (client as unknown as { stagingUploads: ReadonlyMap<bigint, unknown> })
        .stagingUploads.size,
    ).toBe(0);
    client.dispose();
  });

  it("cancels Channel LISTEN promptly and never closes a newer reused handle for a late Result", async () => {
    const state = replayConnection();
    const client = new YasChannelClient(state.connection);
    const oldPending = client.listen("yas.late-old.v1", {
      operationId: operationId(0xd7),
      onAccept: vi.fn(),
    });
    const oldRequest = state.decoded.at(-1)!;
    invalidate(state, YAS_FAMILY_CHANNEL);
    await expect(oldPending).rejects.toThrow(/invalidated/);

    const newerPending = client.listen("yas.late-new.v1", {
      operationId: operationId(0xd8),
      onAccept: vi.fn(),
    });
    settleDecoded(state.decoded.at(-1)!, encodeChannelIdentity(203n, 2n));
    await newerPending;
    expect(
      settleDecodedWithError(oldRequest, encodeChannelIdentity(203n, 1n)),
    ).toBeInstanceOf(Error);
    expect(
      requestsOf(state.decoded, YAS_FAMILY_CHANNEL, YAS_CHANNEL_CLOSE_LISTENER),
    ).toHaveLength(0);
    client.dispose();

    const uniqueState = replayConnection();
    const uniqueClient = new YasChannelClient(uniqueState.connection);
    const uniquePending = uniqueClient.listen("yas.late-unique.v1", {
      operationId: operationId(0xd9),
      onAccept: vi.fn(),
    });
    const uniqueRequest = uniqueState.decoded.at(-1)!;
    invalidate(uniqueState, YAS_FAMILY_CHANNEL);
    await expect(uniquePending).rejects.toThrow(/invalidated/);
    settleDecodedWithError(uniqueRequest, encodeChannelIdentity(204n, 1n));
    expect(
      requestsOf(
        uniqueState.decoded,
        YAS_FAMILY_CHANNEL,
        YAS_CHANNEL_CLOSE_LISTENER,
      ),
    ).toHaveLength(1);
    uniqueClient.dispose();
  });

  it("cancels Events START_STREAM promptly and only stops a unique late handle", async () => {
    const state = replayConnection();
    const client = new YasEventsClient(state.connection);
    const oldPending = client.startStream({
      operationId: operationId(0xda),
      maxBatchBytes: 4096,
    });
    const oldRequest = state.decoded.at(-1)!;
    invalidate(state, YAS_FAMILY_EVENTS);
    await expect(oldPending).rejects.toThrow(/invalidated/);

    const newerPending = client.startStream({
      operationId: operationId(0xdb),
      maxBatchBytes: 4096,
    });
    settleDecoded(
      state.decoded.at(-1)!,
      encodeEventsStreamStarted({
        streamHandle: 205n,
        firstSequence: 0n,
        maxBatchBytes: 4096,
        extensions: [],
      }),
    );
    await newerPending;
    expect(
      settleDecodedWithError(
        oldRequest,
        encodeEventsStreamStarted({
          streamHandle: 205n,
          firstSequence: 0n,
          maxBatchBytes: 4096,
          extensions: [],
        }),
      ),
    ).toBeInstanceOf(Error);
    expect(
      state.plain.filter((request) => request.kind === YAS_EVENTS_STOP_STREAM),
    ).toHaveLength(0);
    client.dispose();

    const uniqueState = replayConnection();
    const uniqueClient = new YasEventsClient(uniqueState.connection);
    const uniquePending = uniqueClient.startStream({
      operationId: operationId(0xdc),
      maxBatchBytes: 4096,
    });
    const uniqueRequest = uniqueState.decoded.at(-1)!;
    invalidate(uniqueState, YAS_FAMILY_EVENTS);
    await expect(uniquePending).rejects.toThrow(/invalidated/);
    settleDecodedWithError(
      uniqueRequest,
      encodeEventsStreamStarted({
        streamHandle: 206n,
        firstSequence: 0n,
        maxBatchBytes: 4096,
        extensions: [],
      }),
    );
    expect(
      uniqueState.plain.filter(
        (request) => request.kind === YAS_EVENTS_STOP_STREAM,
      ),
    ).toHaveLength(1);
    uniqueClient.dispose();
  });

  it("cancels Net OPEN promptly and only closes a unique late handle", async () => {
    const state = replayConnection();
    const client = new YasNetClient(state.connection);
    const { accepted } = fakeTransfers(client);
    const openValue = (operationId: Uint8Array) => ({
      operationId,
      address: { kind: "tcp" as const, host: "127.0.0.1", port: 443 },
      deliveryPreference: YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
      dropPolicy: YAS_NET_DROP_NOT_APPLICABLE,
    });
    const oldPending = client.open(openValue(operationId(0xdd)), 1024n);
    const oldRequest = state.decoded.at(-1)!;
    invalidate(state, YAS_FAMILY_NET);
    await expect(oldPending).rejects.toThrow(/invalidated/);

    const newerPending = client.open(openValue(operationId(0xde)), 1024n);
    settleDecoded(
      state.decoded.at(-1)!,
      encodeNetEndpoint({
        ...netEndpoint(),
        flowHandle: 207n,
        descriptor: netDescriptor(11),
      }),
    );
    await newerPending;
    expect(accepted).toHaveLength(1);
    expect(
      settleDecodedWithError(
        oldRequest,
        encodeNetEndpoint({
          ...netEndpoint(),
          flowHandle: 207n,
          descriptor: netDescriptor(12),
        }),
      ),
    ).toBeInstanceOf(Error);
    expect(
      state.plain.filter((request) => request.kind === YAS_NET_CLOSE),
    ).toHaveLength(0);
    client.dispose();

    const uniqueState = replayConnection();
    const uniqueClient = new YasNetClient(uniqueState.connection);
    fakeTransfers(uniqueClient);
    const uniquePending = uniqueClient.open(
      openValue(operationId(0xdf)),
      1024n,
    );
    const uniqueRequest = uniqueState.decoded.at(-1)!;
    invalidate(uniqueState, YAS_FAMILY_NET);
    await expect(uniquePending).rejects.toThrow(/invalidated/);
    settleDecodedWithError(
      uniqueRequest,
      encodeNetEndpoint({
        ...netEndpoint(),
        flowHandle: 208n,
        descriptor: netDescriptor(13),
      }),
    );
    expect(
      uniqueState.plain.filter((request) => request.kind === YAS_NET_CLOSE),
    ).toHaveLength(1);
    uniqueClient.dispose();
  });
});
