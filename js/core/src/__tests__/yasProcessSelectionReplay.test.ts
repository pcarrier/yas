import { describe, expect, it, vi } from "vitest";

import {
  YAS_FAMILY_PROCESS,
  YAS_FAMILY_SELECTION,
  YAS_GOLDEN_VECTORS,
  YAS_PROCESS_CONTROL,
  YAS_PROCESS_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_SELECTION_ITEM_CONTENT_KIND,
  YAS_SELECTION_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_SELECTION_SET_COMMIT,
  YAS_SELECTION_SLOT_CLIPBOARD,
  YAS_SELECTION_VERSION,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YAS_STATUS_STALE,
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
  YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
  YasProcessClient,
  YasResultError,
  YasSelectionClient,
  YasWriter,
  decodeProcessStreamBundle,
  decodeProcessSpawn,
  encodeExtensions,
  encodeProcessStreamBundle,
  encodeTransferDescriptor,
  type YasProcessSpawn,
  type YasSelectionUploadItem,
  type YasTransfer,
  type YasTransferDescriptor,
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

interface PendingDecodedRequest {
  family: number;
  kind: number;
  payload: Uint8Array;
  result: Deferred<Uint8Array>;
  afterDecode?: () => void;
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

function replayConnection(maxMutationReplays = 4): {
  connection: YasConnection;
  invalidations: Set<(value: YasInvalidation) => void>;
  requests: PendingDecodedRequest[];
  setRemaining(value: bigint): void;
} {
  const invalidations = new Set<(value: YasInvalidation) => void>();
  const requests: PendingDecodedRequest[] = [];
  let remaining = 1n;
  const requestDecoded = vi.fn(
    (
      family: number,
      kind: number,
      payload: Uint8Array,
      decode: (body: Uint8Array) => unknown,
    ) => {
      const result = deferred<Uint8Array>();
      const request: PendingDecodedRequest = {
        family,
        kind,
        payload: new Uint8Array(payload),
        result,
      };
      requests.push(request);
      return result.promise.then((body) => {
        const value = decode(body);
        request.afterDecode?.();
        return value;
      });
    },
  );
  const connection = {
    options: { receiveMaxBuffered: 16n * 1024n * 1024n },
    family: vi.fn((family: number) => ({
      limits: [
        {
          tag:
            family === YAS_FAMILY_PROCESS
              ? YAS_PROCESS_LIMIT_MAX_MUTATION_REPLAYS
              : YAS_SELECTION_LIMIT_MAX_MUTATION_REPLAYS,
          required: true,
          value: new YasWriter().u32(maxMutationReplays).finish(),
        },
      ],
    })),
    registerFamilyLimitValidator: vi.fn(),
    onEvent: vi.fn(() => () => undefined),
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
        return {
          bytes,
          release: vi.fn(),
        } as unknown as YasReceiveBudgetLease;
      },
    },
    request: vi.fn(async () => new Uint8Array()),
    requestDecoded,
    sendEvent: vi.fn(),
    nanosecondsUntilServerTime: vi.fn(() => remaining),
  } as unknown as YasConnection;
  return {
    connection,
    invalidations,
    requests,
    setRemaining(value) {
      remaining = value;
    },
  };
}

class FakeTransfer {
  private readonly closedState = deferred<void>();
  private readonly terminalListeners = new Set<() => void>();
  private readonly resetListeners = new Set<() => void>();
  private terminalObserved = false;
  private resetObserved = false;
  readonly closed = this.closedState.promise;
  readonly reset: ReturnType<typeof vi.fn>;
  readonly receiveData = vi.fn((_payload: Uint8Array) => undefined);

  constructor(
    readonly descriptor: YasTransferDescriptor,
    private readonly onRetired: () => void = () => undefined,
  ) {
    this.reset = vi.fn(() => this.triggerReset());
  }

  subscribeReset(listener: () => void): () => void {
    if (this.resetObserved) {
      listener();
      return () => undefined;
    }
    this.resetListeners.add(listener);
    return () => this.resetListeners.delete(listener);
  }

  subscribeTerminal(listener: () => void): () => void {
    if (this.terminalObserved) {
      listener();
      return () => undefined;
    }
    this.terminalListeners.add(listener);
    return () => this.terminalListeners.delete(listener);
  }

  closeNormally(): void {
    this.observeTerminal();
    this.onRetired();
    this.closedState.resolve(undefined);
  }

  terminateDirection(): void {
    this.observeTerminal();
  }

  triggerReset(): void {
    if (this.resetObserved) return;
    this.observeTerminal();
    this.resetObserved = true;
    for (const listener of [...this.resetListeners]) listener();
    this.resetListeners.clear();
    this.onRetired();
    this.closedState.resolve(undefined);
  }

  private observeTerminal(): void {
    if (this.terminalObserved) return;
    this.terminalObserved = true;
    for (const listener of [...this.terminalListeners]) listener();
    this.terminalListeners.clear();
  }
}

function goldenBytes(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (entry) => entry.name === name,
  )!.hex;
  return Uint8Array.from(hex.match(/../g)!, (byte) =>
    Number.parseInt(byte, 16),
  );
}

function processSpawn(
  operationId: Uint8Array,
): Omit<YasProcessSpawn, "stdoutReceiveCredit" | "stderrReceiveCredit"> {
  const {
    stdoutReceiveCredit: _stdoutReceiveCredit,
    stderrReceiveCredit: _stderrReceiveCredit,
    ...value
  } = decodeProcessSpawn(goldenBytes("process.spawn.payload"));
  return { ...value, operationId };
}

function processBundleBody(
  processHandle: bigint,
  firstTransferId: number,
): Uint8Array {
  const bundle = decodeProcessStreamBundle(
    goldenBytes("process.stream_bundle.payload"),
  );
  return encodeProcessStreamBundle({
    ...bundle,
    processHandle,
    stdin: bundle.stdin
      ? { ...bundle.stdin, transferId: firstTransferId }
      : undefined,
    stdout: { ...bundle.stdout, transferId: firstTransferId + 2 },
    stderr: bundle.stderr
      ? { ...bundle.stderr, transferId: firstTransferId + 4 }
      : undefined,
  });
}

function processTransfers(client: YasProcessClient): {
  accepted: FakeTransfer[];
  reserves: Array<{ preferred: bigint; minimum: bigint }>;
} {
  const accepted: FakeTransfer[] = [];
  const reserves: Array<{ preferred: bigint; minimum: bigint }> = [];
  const owned = new Map<number, FakeTransfer>();
  const accept = (descriptor: YasTransferDescriptor): YasTransfer => {
    if (owned.has(descriptor.transferId))
      throw new Error(`duplicate Transfer ${descriptor.transferId}`);
    let transfer!: FakeTransfer;
    transfer = new FakeTransfer(descriptor, () => {
      if (owned.get(descriptor.transferId) === transfer)
        owned.delete(descriptor.transferId);
    });
    owned.set(descriptor.transferId, transfer);
    accepted.push(transfer);
    return transfer as unknown as YasTransfer;
  };
  const manager = {
    reserveReceiveCredit(preferred: bigint, minimum = 1n) {
      reserves.push({ preferred, minimum });
      const bytes = minimum === preferred ? preferred : preferred / 2n;
      return {
        bytes,
        release: vi.fn(),
      } as unknown as YasReceiveBudgetLease;
    },
    acceptServerUploadDescriptor(descriptor: YasTransferDescriptor) {
      return accept(descriptor);
    },
    acceptServerDescriptor(
      descriptor: YasTransferDescriptor,
      _lease: YasReceiveBudgetLease,
    ) {
      return accept(descriptor);
    },
    get(id: number) {
      return owned.get(id) as unknown as YasTransfer | undefined;
    },
  };
  Object.defineProperty(client, "transfers", { value: manager });
  return { accepted, reserves };
}

function selectionDescriptor(
  stagingHandle: bigint,
  transferId: number,
  expiresServerNs: bigint,
): YasTransferDescriptor {
  const uploadStage = { stagingHandle, expiresServerNs };
  const extensions = [
    {
      tag: YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
      required: true,
      value: new Uint8Array(),
    },
    {
      tag: YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
      required: true,
      value: new YasWriter().u64(stagingHandle).u64(expiresServerNs).finish(),
    },
  ];
  return {
    transferId,
    mode: YAS_TRANSFER_MODE_BYTE,
    direction: YAS_TRANSFER_RECEIVER_TO_SENDER,
    flags: 0,
    receiverSendCredit: 4n,
    senderSendCredit: 0n,
    maxItemBytes: 0n,
    maxChunkBytes: 1024,
    contentFamily: YAS_FAMILY_SELECTION,
    contentKind: YAS_SELECTION_ITEM_CONTENT_KIND,
    contentVersion: YAS_SELECTION_VERSION,
    extensions,
    maxOpenMessages: 1,
    sensitiveContent: true,
    uploadStage,
  };
}

function selectionBeginBody(
  stagingHandle: bigint,
  transferId: number,
  expiresServerNs = 10_000n,
): Uint8Array {
  return new YasWriter()
    .u64(stagingHandle)
    .u16(1)
    .u16(0)
    .bytesU32(
      encodeTransferDescriptor(
        selectionDescriptor(stagingHandle, transferId, expiresServerNs),
      ),
    )
    .bytes(encodeExtensions())
    .finish();
}

function selectionTransfers(client: YasSelectionClient): FakeTransfer[] {
  const accepted: FakeTransfer[] = [];
  const owned = new Map<number, FakeTransfer>();
  Object.defineProperty(client, "transfers", {
    value: {
      acceptServerUploadDescriptor(descriptor: YasTransferDescriptor) {
        if (owned.has(descriptor.transferId))
          throw new Error(`duplicate Transfer ${descriptor.transferId}`);
        let transfer!: FakeTransfer;
        transfer = new FakeTransfer(descriptor, () => {
          if (owned.get(descriptor.transferId) === transfer)
            owned.delete(descriptor.transferId);
        });
        owned.set(descriptor.transferId, transfer);
        accepted.push(transfer);
        return transfer as unknown as YasTransfer;
      },
      get(id: number) {
        return owned.get(id) as unknown as YasTransfer | undefined;
      },
    },
  });
  return accepted;
}

const uploadItems: readonly YasSelectionUploadItem[] = [
  {
    mime: "text/plain",
    byteLength: 4n,
    contentHash: new Uint8Array(32).fill(0x44),
    initialReceiveCredit: 4n,
  },
];

describe("YAS exact resource-operation replay ownership", () => {
  it("coalesces Process SPAWN and owns one live StreamBundle", async () => {
    const { connection, requests } = replayConnection();
    const client = new YasProcessClient(connection);
    const { accepted, reserves } = processTransfers(client);
    const operationId = new Uint8Array(16).fill(0x11);
    const value = processSpawn(operationId);

    const first = client.spawn(value, 1024n, 512n);
    expect(requests).toHaveLength(1);
    const concurrent = client.spawn(value, 1024n, 512n);
    expect(requests).toHaveLength(1);
    expect(reserves).toHaveLength(2);
    expect(() => client.spawn(value, 2048n, 512n)).toThrow(
      /operation ID was reused with a different payload/,
    );

    requests[0]!.result.resolve(goldenBytes("process.stream_bundle.payload"));
    const streams = await first;
    expect(await concurrent).toBe(streams);
    expect(accepted).toHaveLength(3);
    expect(await client.spawn(value, 1024n, 512n)).toBe(streams);
    expect(requests).toHaveLength(1);
    expect(accepted).toHaveLength(3);

    accepted[0]!.terminateDirection();
    const stale = client.spawn(value, 1024n, 512n);
    expect(requests).toHaveLength(2);
    expect(requests[1]!.payload).toEqual(requests[0]!.payload);
    expect(reserves.slice(2)).toEqual([
      { preferred: 512n, minimum: 512n },
      { preferred: 256n, minimum: 256n },
    ]);
    requests[1]!.result.reject(
      new YasResultError(YAS_STATUS_STALE, new Uint8Array()),
    );
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    expect(() =>
      client.spawn({ ...value, argv: [new Uint8Array([0x78])] }, 1024n, 512n),
    ).toThrow(/operation ID was reused with a different payload/);

    for (let index = 0; index < 32; index++) {
      const failed = client.spawn(
        processSpawn(new Uint8Array(16).fill(0x20 + index)),
        1024n,
        512n,
      );
      requests
        .at(-1)!
        .result.reject(
          new YasResultError(YAS_STATUS_RESOURCE_EXHAUSTED, new Uint8Array()),
        );
      await expect(failed).rejects.toMatchObject({
        status: YAS_STATUS_RESOURCE_EXHAUSTED,
      });
    }
    expect(
      (
        client as unknown as {
          spawnOperations: ReadonlyMap<string, unknown>;
        }
      ).spawnOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("bounds Process settlements, pins live ownership, and preserves tombstones across failed successors", async () => {
    const { connection, requests } = replayConnection(2);
    const client = new YasProcessClient(connection);
    processTransfers(client);
    const firstValue = processSpawn(new Uint8Array(16).fill(0x31));
    const secondValue = processSpawn(new Uint8Array(16).fill(0x32));
    const successorValue = processSpawn(new Uint8Array(16).fill(0x33));

    const first = client.spawn(firstValue, 1024n, 512n);
    requests.at(-1)!.result.resolve(processBundleBody(1n, 2));
    const firstStreams = await first;
    const second = client.spawn(secondValue, 1024n, 512n);
    requests.at(-1)!.result.resolve(processBundleBody(2n, 20));
    const secondStreams = await second;
    (secondStreams.stdout as unknown as FakeTransfer).terminateDirection();

    const failedSuccessor = client.spawn(successorValue, 1024n, 512n);
    requests
      .at(-1)!
      .result.reject(
        new YasResultError(YAS_STATUS_RESOURCE_EXHAUSTED, new Uint8Array()),
      );
    await expect(failedSuccessor).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });

    const retainedSecond = client.spawn(secondValue, 1024n, 512n);
    requests
      .at(-1)!
      .result.reject(new YasResultError(YAS_STATUS_STALE, new Uint8Array()));
    await expect(retainedSecond).rejects.toMatchObject({
      status: YAS_STATUS_STALE,
    });
    expect(await client.spawn(firstValue, 1024n, 512n)).toBe(firstStreams);

    const successfulSuccessor = client.spawn(successorValue, 1024n, 512n);
    requests.at(-1)!.result.resolve(processBundleBody(3n, 40));
    await successfulSuccessor;
    expect(
      (
        client as unknown as {
          spawnOperations: ReadonlyMap<string, unknown>;
        }
      ).spawnOperations.size,
    ).toBe(2);

    const requestCount = requests.length;
    await expect(client.spawn(secondValue, 1024n, 512n)).rejects.toThrow(
      /no evictable settlement/,
    );
    expect(requests).toHaveLength(requestCount);
    client.dispose();
  });

  it("rejects unexpected Process replay OK and only cleans a unique orphan", async () => {
    const { connection, requests } = replayConnection();
    const client = new YasProcessClient(connection);
    const { accepted } = processTransfers(client);
    const value = processSpawn(new Uint8Array(16).fill(0x41));
    const first = client.spawn(value, 1024n, 512n);
    requests[0]!.result.resolve(processBundleBody(1n, 2));
    await first;
    accepted[0]!.terminateDirection();

    const reused = client.spawn(value, 1024n, 512n);
    requests.at(-1)!.result.resolve(processBundleBody(1n, 2));
    await expect(reused).rejects.toThrow(/unexpectedly returned OK/);
    expect(accepted).toHaveLength(3);
    expect(
      accepted.every((transfer) => transfer.reset.mock.calls.length === 0),
    ).toBe(true);
    expect(connection.request).not.toHaveBeenCalled();

    const unique = client.spawn(value, 1024n, 512n);
    requests.at(-1)!.result.resolve(processBundleBody(99n, 20));
    await expect(unique).rejects.toThrow(/unexpectedly returned OK/);
    expect(accepted).toHaveLength(6);
    expect(
      accepted
        .slice(3)
        .every((transfer) => transfer.reset.mock.calls.length === 1),
    ).toBe(true);
    expect(connection.request).toHaveBeenCalledOnce();
    expect(connection.request).toHaveBeenCalledWith(
      YAS_FAMILY_PROCESS,
      YAS_PROCESS_CONTROL,
      expect.any(Uint8Array),
    );
    expect(
      accepted
        .slice(0, 3)
        .every((transfer) => !transfer.reset.mock.calls.length),
    ).toBe(true);
    client.dispose();
  });

  it("tombstones Selection replay on CLOSE while retaining its committable stage", async () => {
    const { connection, requests } = replayConnection();
    const client = new YasSelectionClient(connection);
    const accepted = selectionTransfers(client);
    const operationId = new Uint8Array(16).fill(0x51);
    const begin = () =>
      client.beginSet(YAS_SELECTION_SLOT_CLIPBOARD, operationId, uploadItems);

    const first = begin();
    expect(requests).toHaveLength(1);
    const concurrent = begin();
    expect(requests).toHaveLength(1);
    expect(() =>
      client.beginSet(YAS_SELECTION_SLOT_CLIPBOARD, operationId, [
        { ...uploadItems[0]!, byteLength: 5n },
      ]),
    ).toThrow(/operation ID was reused with a different payload/);

    requests[0]!.result.resolve(selectionBeginBody(7n, 2));
    const batch = await first;
    expect(await concurrent).toBe(batch);
    expect(accepted).toHaveLength(1);
    expect(await begin()).toBe(batch);
    expect(requests).toHaveLength(1);

    accepted[0]!.closeNormally();
    await Promise.resolve();
    const staleAfterClose = begin();
    expect(requests).toHaveLength(2);
    requests[1]!.result.reject(
      new YasResultError(YAS_STATUS_STALE, new Uint8Array()),
    );
    await expect(staleAfterClose).rejects.toMatchObject({
      status: YAS_STATUS_STALE,
    });
    expect(accepted).toHaveLength(1);

    const committing = client.commitSet(
      batch.stagingHandle,
      new Uint8Array(16).fill(0x53),
    );
    expect(requests.at(-1)!.kind).toBe(YAS_SELECTION_SET_COMMIT);
    requests.at(-1)!.result.resolve(new YasWriter().u64(9n).finish());
    await expect(committing).resolves.toBe(9n);

    const staleAfterCommit = begin();
    requests
      .at(-1)!
      .result.reject(new YasResultError(YAS_STATUS_STALE, new Uint8Array()));
    await expect(staleAfterCommit).rejects.toMatchObject({
      status: YAS_STATUS_STALE,
    });
    expect(() =>
      client.beginSet(YAS_SELECTION_SLOT_CLIPBOARD, operationId, [
        { ...uploadItems[0]!, byteLength: 6n },
      ]),
    ).toThrow(/operation ID was reused with a different payload/);

    const distinctId = new Uint8Array(16).fill(0x52);
    const distinct = client.beginSet(
      YAS_SELECTION_SLOT_CLIPBOARD,
      distinctId,
      uploadItems,
    );
    requests
      .at(-1)!
      .result.reject(
        new YasResultError(YAS_STATUS_RESOURCE_EXHAUSTED, new Uint8Array()),
      );
    await expect(distinct).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });

    const retainedAfterFailure = begin();
    requests
      .at(-1)!
      .result.reject(new YasResultError(YAS_STATUS_STALE, new Uint8Array()));
    await expect(retainedAfterFailure).rejects.toMatchObject({
      status: YAS_STATUS_STALE,
    });

    for (let index = 0; index < 32; index++) {
      const failed = client.beginSet(
        YAS_SELECTION_SLOT_CLIPBOARD,
        new Uint8Array(16).fill(0x60 + index),
        uploadItems,
      );
      requests
        .at(-1)!
        .result.reject(
          new YasResultError(YAS_STATUS_RESOURCE_EXHAUSTED, new Uint8Array()),
        );
      await expect(failed).rejects.toMatchObject({
        status: YAS_STATUS_RESOURCE_EXHAUSTED,
      });
    }
    expect(
      (
        client as unknown as {
          beginOperations: ReadonlyMap<string, unknown>;
        }
      ).beginOperations.size,
    ).toBe(1);
    client.dispose();
  });

  it("bounds Selection settlements, pins live ownership, and preserves tombstones across failed successors", async () => {
    const { connection, requests } = replayConnection(2);
    const client = new YasSelectionClient(connection);
    const accepted = selectionTransfers(client);
    const firstId = new Uint8Array(16).fill(0x81);
    const secondId = new Uint8Array(16).fill(0x82);
    const successorId = new Uint8Array(16).fill(0x83);
    const begin = (operationId: Uint8Array) =>
      client.beginSet(YAS_SELECTION_SLOT_CLIPBOARD, operationId, uploadItems);

    const first = begin(firstId);
    requests.at(-1)!.result.resolve(selectionBeginBody(21n, 2));
    const firstBatch = await first;
    const second = begin(secondId);
    requests.at(-1)!.result.resolve(selectionBeginBody(22n, 4));
    await second;
    accepted[1]!.closeNormally();

    const failedSuccessor = begin(successorId);
    requests
      .at(-1)!
      .result.reject(
        new YasResultError(YAS_STATUS_RESOURCE_EXHAUSTED, new Uint8Array()),
      );
    await expect(failedSuccessor).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    const retainedSecond = begin(secondId);
    requests
      .at(-1)!
      .result.reject(new YasResultError(YAS_STATUS_STALE, new Uint8Array()));
    await expect(retainedSecond).rejects.toMatchObject({
      status: YAS_STATUS_STALE,
    });
    expect(await begin(firstId)).toBe(firstBatch);

    const successfulSuccessor = begin(successorId);
    requests.at(-1)!.result.resolve(selectionBeginBody(23n, 6));
    await successfulSuccessor;
    expect(
      (
        client as unknown as {
          beginOperations: ReadonlyMap<string, unknown>;
        }
      ).beginOperations.size,
    ).toBe(2);

    const requestCount = requests.length;
    await expect(begin(secondId)).rejects.toThrow(/no evictable settlement/);
    expect(requests).toHaveLength(requestCount);
    client.dispose();
  });

  it("rejects unexpected Selection replay OK and only resets a unique replacement", async () => {
    const { connection, requests } = replayConnection();
    const client = new YasSelectionClient(connection);
    const accepted = selectionTransfers(client);
    const operationId = new Uint8Array(16).fill(0x91);
    const begin = () =>
      client.beginSet(YAS_SELECTION_SLOT_CLIPBOARD, operationId, uploadItems);

    const first = begin();
    requests.at(-1)!.result.resolve(selectionBeginBody(31n, 2));
    await first;
    accepted[0]!.closeNormally();

    const reused = begin();
    requests.at(-1)!.result.resolve(selectionBeginBody(31n, 2));
    await expect(reused).rejects.toThrow(/unexpectedly returned OK/);
    expect(accepted).toHaveLength(1);
    expect(accepted[0]!.reset).not.toHaveBeenCalled();

    const unique = begin();
    requests.at(-1)!.result.resolve(selectionBeginBody(32n, 4));
    await expect(unique).rejects.toThrow(/unexpectedly returned OK/);
    expect(accepted).toHaveLength(2);
    expect(accepted[0]!.reset).not.toHaveBeenCalled();
    expect(accepted[1]!.reset).toHaveBeenCalledOnce();
    client.dispose();
  });

  it("retires replay ownership on family invalidation and disposal", async () => {
    const process = replayConnection();
    const processClient = new YasProcessClient(process.connection);
    const processAccepted = processTransfers(processClient).accepted;
    const processId = new Uint8Array(16).fill(0x71);
    const processValue = processSpawn(processId);
    const spawning = processClient.spawn(processValue, 1024n, 512n);
    process.requests[0]!.result.resolve(
      goldenBytes("process.stream_bundle.payload"),
    );
    await spawning;
    for (const invalidate of [...process.invalidations])
      invalidate({ family: YAS_FAMILY_PROCESS });
    expect(
      processAccepted.every(
        (transfer) => transfer.reset.mock.calls.length === 1,
      ),
    ).toBe(true);
    const processRetry = processClient.spawn(processValue, 1024n, 512n);
    expect(process.requests).toHaveLength(2);
    processClient.dispose();
    await expect(processRetry).rejects.toThrow(/disposed/);
    process.requests[1]!.result.resolve(
      goldenBytes("process.stream_bundle.payload"),
    );
    await Promise.resolve();
    await Promise.resolve();

    const selection = replayConnection();
    const selectionClient = new YasSelectionClient(selection.connection);
    const selectionAccepted = selectionTransfers(selectionClient);
    const selectionId = new Uint8Array(16).fill(0x72);
    const selectionValue = () =>
      selectionClient.beginSet(
        YAS_SELECTION_SLOT_CLIPBOARD,
        selectionId,
        uploadItems,
      );
    const beginning = selectionValue();
    selection.requests[0]!.result.resolve(selectionBeginBody(12n, 6));
    await beginning;
    for (const invalidate of [...selection.invalidations])
      invalidate({ family: YAS_FAMILY_SELECTION });
    expect(selectionAccepted[0]!.reset).toHaveBeenCalledOnce();
    const selectionRetry = selectionValue();
    expect(selection.requests).toHaveLength(2);
    selectionClient.dispose();
    await expect(selectionRetry).rejects.toThrow(/disposed/);
    selection.requests[1]!.result.resolve(selectionBeginBody(13n, 8));
    await Promise.resolve();
    await Promise.resolve();
    expect(selectionAccepted[1]!.reset).toHaveBeenCalledOnce();
  });

  it("does not destructively clean a newer owner when an invalidated Result arrives late", async () => {
    const process = replayConnection();
    const processClient = new YasProcessClient(process.connection);
    const processAccepted = processTransfers(processClient).accepted;
    const oldSpawn = processClient.spawn(
      processSpawn(new Uint8Array(16).fill(0xa1)),
      1024n,
      512n,
    );
    for (const invalidate of [...process.invalidations])
      invalidate({ family: YAS_FAMILY_PROCESS });
    await expect(oldSpawn).rejects.toThrow(/invalidated/);

    const newSpawn = processClient.spawn(
      processSpawn(new Uint8Array(16).fill(0xa2)),
      1024n,
      512n,
    );
    process.requests[1]!.result.resolve(processBundleBody(1n, 2));
    await newSpawn;
    process.requests[0]!.result.resolve(processBundleBody(1n, 2));
    await Promise.resolve();
    await Promise.resolve();
    expect(processAccepted).toHaveLength(3);
    expect(
      processAccepted.every(
        (transfer) => transfer.reset.mock.calls.length === 0,
      ),
    ).toBe(true);
    expect(process.connection.request).not.toHaveBeenCalled();
    processClient.dispose();

    const selection = replayConnection();
    const selectionClient = new YasSelectionClient(selection.connection);
    const selectionAccepted = selectionTransfers(selectionClient);
    const oldBegin = selectionClient.beginSet(
      YAS_SELECTION_SLOT_CLIPBOARD,
      new Uint8Array(16).fill(0xb1),
      uploadItems,
    );
    for (const invalidate of [...selection.invalidations])
      invalidate({ family: YAS_FAMILY_SELECTION });
    await expect(oldBegin).rejects.toThrow(/invalidated/);

    const newBegin = selectionClient.beginSet(
      YAS_SELECTION_SLOT_CLIPBOARD,
      new Uint8Array(16).fill(0xb2),
      uploadItems,
    );
    selection.requests[1]!.result.resolve(selectionBeginBody(41n, 2));
    await newBegin;
    selection.requests[0]!.result.resolve(selectionBeginBody(41n, 2));
    await Promise.resolve();
    await Promise.resolve();
    expect(selectionAccepted).toHaveLength(1);
    expect(selectionAccepted[0]!.reset).not.toHaveBeenCalled();
    selectionClient.dispose();
  });

  it("expires Selection ownership without reusing the accepted Transfer", async () => {
    const { connection, requests, setRemaining } = replayConnection();
    const client = new YasSelectionClient(connection);
    const accepted = selectionTransfers(client);
    const operationId = new Uint8Array(16).fill(0x73);
    const begin = () =>
      client.beginSet(YAS_SELECTION_SLOT_CLIPBOARD, operationId, uploadItems);
    const first = begin();
    requests[0]!.result.resolve(selectionBeginBody(14n, 10));
    await first;
    setRemaining(0n);
    const stale = begin();
    expect(accepted[0]!.reset).toHaveBeenCalledOnce();
    expect(requests).toHaveLength(2);
    requests[1]!.result.reject(
      new YasResultError(YAS_STATUS_STALE, new Uint8Array()),
    );
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    client.dispose();
  });

  it("installs Process descriptors inside the Result decoder before following Transfer DATA", async () => {
    const { connection, requests } = replayConnection();
    const client = new YasProcessClient(connection);
    const { accepted } = processTransfers(client);
    const value = processSpawn(new Uint8Array(16).fill(0xc1));
    const first = client.spawn(value, 1024n, 512n);
    const concurrent = client.spawn(value, 1024n, 512n);
    expect(concurrent).toBe(first);
    requests[0]!.afterDecode = () => {
      const stdout = accepted[1];
      if (!stdout) throw new Error("following DATA named an unknown Transfer");
      stdout.receiveData(new Uint8Array([1, 2, 3]));
    };

    requests[0]!.result.resolve(processBundleBody(71n, 2));
    const streams = await first;
    expect(await concurrent).toBe(streams);
    expect(accepted[1]!.receiveData).toHaveBeenCalledWith(
      new Uint8Array([1, 2, 3]),
    );
    client.dispose();
  });

  it("installs Selection descriptors inside the Result decoder before a following terminal frame", async () => {
    const { connection, requests } = replayConnection();
    const client = new YasSelectionClient(connection);
    const accepted = selectionTransfers(client);
    const operationId = new Uint8Array(16).fill(0xc2);
    const begin = () =>
      client.beginSet(YAS_SELECTION_SLOT_CLIPBOARD, operationId, uploadItems);
    const first = begin();
    const concurrent = begin();
    expect(concurrent).toBe(first);
    requests[0]!.afterDecode = () => {
      const transfer = accepted[0];
      if (!transfer)
        throw new Error("following terminal frame named an unknown Transfer");
      transfer.terminateDirection();
    };

    requests[0]!.result.resolve(selectionBeginBody(72n, 2));
    const batch = await first;
    expect(await concurrent).toBe(batch);
    const stale = begin();
    expect(requests).toHaveLength(2);
    requests[1]!.result.reject(
      new YasResultError(YAS_STATUS_STALE, new Uint8Array()),
    );
    await expect(stale).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    client.dispose();
  });

  it("reserves the only evictable replay slot across concurrent Process and Selection retries", async () => {
    const process = replayConnection(2);
    const processClient = new YasProcessClient(process.connection);
    processTransfers(processClient);
    const firstProcess = processSpawn(new Uint8Array(16).fill(0xc3));
    const secondProcess = processSpawn(new Uint8Array(16).fill(0xc4));
    const reservedProcess = processSpawn(new Uint8Array(16).fill(0xc5));
    const refusedProcess = processSpawn(new Uint8Array(16).fill(0xc6));
    const firstSpawn = processClient.spawn(firstProcess, 1024n, 512n);
    process.requests.at(-1)!.result.resolve(processBundleBody(81n, 2));
    await firstSpawn;
    const secondSpawn = processClient.spawn(secondProcess, 1024n, 512n);
    process.requests.at(-1)!.result.resolve(processBundleBody(82n, 20));
    const secondStreams = await secondSpawn;
    (secondStreams.stdout as unknown as FakeTransfer).terminateDirection();

    const reservedSpawn = processClient.spawn(reservedProcess, 1024n, 512n);
    const processRequestCount = process.requests.length;
    await expect(
      processClient.spawn(refusedProcess, 1024n, 512n),
    ).rejects.toThrow(/no evictable settlement/);
    await expect(
      processClient.spawn(secondProcess, 1024n, 512n),
    ).rejects.toThrow(/no evictable settlement/);
    expect(process.requests).toHaveLength(processRequestCount);
    process.requests
      .at(-1)!
      .result.reject(
        new YasResultError(YAS_STATUS_RESOURCE_EXHAUSTED, new Uint8Array()),
      );
    await expect(reservedSpawn).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    processClient.dispose();

    const selection = replayConnection(2);
    const selectionClient = new YasSelectionClient(selection.connection);
    const selectionAccepted = selectionTransfers(selectionClient);
    const begin = (byte: number) =>
      selectionClient.beginSet(
        YAS_SELECTION_SLOT_CLIPBOARD,
        new Uint8Array(16).fill(byte),
        uploadItems,
      );
    const firstBegin = begin(0xc7);
    selection.requests.at(-1)!.result.resolve(selectionBeginBody(83n, 2));
    await firstBegin;
    const secondBegin = begin(0xc8);
    selection.requests.at(-1)!.result.resolve(selectionBeginBody(84n, 4));
    await secondBegin;
    selectionAccepted[1]!.terminateDirection();

    const reservedBegin = begin(0xc9);
    const selectionRequestCount = selection.requests.length;
    await expect(begin(0xca)).rejects.toThrow(/no evictable settlement/);
    await expect(begin(0xc8)).rejects.toThrow(/no evictable settlement/);
    expect(selection.requests).toHaveLength(selectionRequestCount);
    selection.requests
      .at(-1)!
      .result.reject(
        new YasResultError(YAS_STATUS_RESOURCE_EXHAUSTED, new Uint8Array()),
      );
    await expect(reservedBegin).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    selectionClient.dispose();
  });

  it("rejects cross-operation reuse of retained Process and Selection identities", async () => {
    const process = replayConnection();
    const processClient = new YasProcessClient(process.connection);
    const processAccepted = processTransfers(processClient).accepted;
    const firstProcess = processClient.spawn(
      processSpawn(new Uint8Array(16).fill(0xcb)),
      1024n,
      512n,
    );
    process.requests.at(-1)!.result.resolve(processBundleBody(91n, 2));
    await firstProcess;
    for (const transfer of processAccepted) transfer.reset();

    const reusedProcessHandle = processClient.spawn(
      processSpawn(new Uint8Array(16).fill(0xcc)),
      1024n,
      512n,
    );
    process.requests.at(-1)!.result.resolve(processBundleBody(91n, 20));
    await expect(reusedProcessHandle).rejects.toThrow(/owned stream authority/);
    const reusedProcessTransfer = processClient.spawn(
      processSpawn(new Uint8Array(16).fill(0xcd)),
      1024n,
      512n,
    );
    process.requests.at(-1)!.result.resolve(processBundleBody(92n, 2));
    await expect(reusedProcessTransfer).rejects.toThrow(
      /owned stream authority/,
    );
    expect(processAccepted).toHaveLength(3);
    expect(process.connection.request).not.toHaveBeenCalled();
    processClient.dispose();

    const selection = replayConnection();
    const selectionClient = new YasSelectionClient(selection.connection);
    const selectionAccepted = selectionTransfers(selectionClient);
    const firstSelection = selectionClient.beginSet(
      YAS_SELECTION_SLOT_CLIPBOARD,
      new Uint8Array(16).fill(0xce),
      uploadItems,
    );
    selection.requests.at(-1)!.result.resolve(selectionBeginBody(93n, 2));
    await firstSelection;
    selectionAccepted[0]!.reset();

    const reusedStage = selectionClient.beginSet(
      YAS_SELECTION_SLOT_CLIPBOARD,
      new Uint8Array(16).fill(0xcf),
      uploadItems,
    );
    selection.requests.at(-1)!.result.resolve(selectionBeginBody(93n, 4));
    await expect(reusedStage).rejects.toThrow(/owned stage authority/);
    const reusedTransfer = selectionClient.beginSet(
      YAS_SELECTION_SLOT_CLIPBOARD,
      new Uint8Array(16).fill(0xd0),
      uploadItems,
    );
    selection.requests.at(-1)!.result.resolve(selectionBeginBody(94n, 2));
    await expect(reusedTransfer).rejects.toThrow(/owned stage authority/);
    expect(selectionAccepted).toHaveLength(1);
    selectionClient.dispose();
  });
});
