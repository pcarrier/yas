import type {
  YasTransport as BaseYasTransport,
  YasTransportMessage,
  ConnectionStatus,
} from "../types";
import {
  YAS_CORE_CANCEL,
  YAS_CORE_FAMILY_UPDATE,
  YAS_CORE_GOAWAY,
  YAS_CORE_HELLO,
  YAS_CORE_PING,
  YAS_CORE_SESSION_INFO,
  YAS_CORE_SESSION_UPDATE,
  YAS_CORE_SHUTDOWN,
  YAS_RUNTIME_UNAVAILABLE,
  YAS_DIRECTION_SERVER_ACCEPTS,
  YAS_DIRECTION_SERVER_SENDS,
  YAS_FAMILY_CORE,
  decodeCancel,
  decodeFamilyUpdate,
  decodeGoAway,
  decodePing,
  decodePingResult,
  decodeServerHello,
  decodeSessionUpdate,
  decodeSessionInfo,
  encodeCancel,
  encodeClientHello,
  encodePing,
  encodePingResult,
  encodeShutdown,
  validateServerHello,
  validateReceiveLimitUpdate,
  type YasClientHelloOptions,
  type YasFamilyDescriptor,
  type YasGoAway,
  type YasServerHello,
} from "./core";
import type { YasExtension } from "./wire";
import { YAS_FAMILY_DEPENDENCIES } from "./generated";
import { validateYasDatagramFrame } from "./datagram";
import {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_CLASS_RESULT,
  YAS_MAX_PRE_HELLO_FRAME,
  YAS_MAX_DATAGRAM,
  YAS_PREFACE,
  YAS_STATUS_CANCELLED,
  YAS_STATUS_INVALID,
  YAS_STATUS_INTERNAL,
  YAS_STATUS_NOT_FOUND,
  YAS_STATUS_OK,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YAS_STATUS_UNAVAILABLE,
  YAS_STATUS_UNSUPPORTED,
  YasDisconnectedError,
  YasProtocolError,
  YasResultError,
  YasStreamFrameDecoder,
  decodeResultPayload,
  decodeYasFrame,
  encodeResultPayload,
  encodeYasFrame,
  frameForByteStream,
  type YasFrame,
} from "./wire";

export interface YasTransport extends BaseYasTransport {
  /** Browser transports use messages; Relay tunnels expose a raw byte stream. */
  readonly yasFraming?: "message" | "stream";
}

export interface YasConnectionOptions extends Omit<
  YasClientHelloOptions,
  "clientInstance"
> {
  clientInstance?: Uint8Array;
}

export interface YasIncomingRequest {
  family: number;
  kind: number;
  requestId: number;
  sensitive: boolean;
  payload: Uint8Array;
  /** Aborted when the peer successfully cancels this admitted Request. */
  signal: AbortSignal;
}

export interface YasRequestResponse {
  status: number;
  body?: Uint8Array;
  detail?: Uint8Array;
  /** Runs only after the Result frame has been emitted. */
  afterSend?: () => void | Promise<void>;
}

/**
 * A ResultPrefix-preserving response. Most family clients use {@link request}
 * and receive a rejected {@link YasResultError} for a non-OK status. Families
 * whose public API exposes retryable or partial outcomes can use
 * {@link requestResult} instead without losing the wire status or detail.
 */
export interface YasResultEnvelope {
  status: number;
  detail: Uint8Array;
  body: Uint8Array;
}

export type YasRequestHandler = (
  request: YasIncomingRequest,
) => Uint8Array | YasRequestResponse | Promise<Uint8Array | YasRequestResponse>;

export interface YasEvent {
  family: number;
  kind: number;
  sensitive: boolean;
  payload: Uint8Array;
  /** True when the Event arrived on the optional unreliable path. */
  datagram: boolean;
}

export interface YasDatagramCounters {
  received: number;
  delivered: number;
  dropped: number;
}

type EventListener = (event: YasEvent) => void;

export interface YasInvalidation {
  /** Undefined invalidates the physical session; otherwise only this family. */
  family?: number;
  error: Error;
}

type InvalidationListener = (invalidation: YasInvalidation) => void;
type ReadyListener = (hello: YasServerHello) => void;

export interface YasCatalogChange {
  revision: bigint;
  /** Families whose selected descriptor may have changed. */
  families: readonly number[];
}

type CatalogChangeListener = (change: YasCatalogChange) => void;

interface PendingRequest {
  family: number;
  kind: number;
  decode?: (body: Uint8Array) => unknown;
  preserveResult?: boolean;
  resolve: (body: unknown) => void;
  reject: (error: unknown) => void;
}

interface IncomingRequestState {
  controller: AbortController;
  payloadLease?: YasReceiveBudgetLease;
}

const CANCELLED_INCOMING_REQUEST = Symbol("cancelled incoming YAS Request");
export const YAS_MAX_RETAINED_INCOMING_REQUESTS = 256;

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

export class YasReceiveBudgetLease {
  private released = false;
  private _bytes: bigint;

  constructor(
    private readonly budget: YasReceiveBudget,
    bytes: bigint,
  ) {
    this._bytes = bytes;
  }

  get bytes(): bigint {
    return this._bytes;
  }

  /** Resize an active reservation without releasing it between sizes. */
  resizeExact(bytes: bigint): void {
    if (this.released)
      throw new YasProtocolError("cannot resize a released receive lease");
    this.budget.resizeLeaseExact(this._bytes, bytes);
    this._bytes = bytes;
  }

  release(): void {
    if (this.released) return;
    this.released = true;
    this.budget.release(this._bytes);
  }
}

/** Conservatively reserves aggregate receive windows across State and Transfer. */
export class YasReceiveBudget {
  private leased = 0n;
  private readonly capacityListeners = new Set<() => void>();

  constructor(private limit: bigint) {}

  setLimit(limit: bigint): void {
    if (limit <= 0n)
      throw new YasProtocolError("receive budget must be positive");
    const increased = limit > this.limit;
    this.limit = limit;
    if (increased) this.notifyCapacityAvailable();
  }

  /** Subscribe to capacity releases shared by every receive-resource family. */
  onCapacityAvailable(listener: () => void): () => void {
    this.capacityListeners.add(listener);
    return () => this.capacityListeners.delete(listener);
  }

  reserve(preferred: bigint, minimum = 1n): YasReceiveBudgetLease {
    if (preferred < minimum)
      throw new YasProtocolError("invalid receive credit request");
    const available = this.limit > this.leased ? this.limit - this.leased : 0n;
    const selected = preferred < available ? preferred : available;
    if (selected < minimum)
      throw new YasResultError(
        YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(0),
        "aggregate YAS receive budget exhausted",
      );
    this.leased += selected;
    return new YasReceiveBudgetLease(this, selected);
  }

  /** Retain an already-decoded payload only when its complete size fits. */
  reserveExact(bytes: bigint): YasReceiveBudgetLease {
    if (bytes <= 0n)
      throw new YasProtocolError("exact receive reservation must be positive");
    const available = this.limit > this.leased ? this.limit - this.leased : 0n;
    if (bytes > available)
      throw new YasResultError(
        YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(0),
        "aggregate YAS receive budget exhausted",
      );
    this.leased += bytes;
    return new YasReceiveBudgetLease(this, bytes);
  }

  /** Internal backing operation for {@link YasReceiveBudgetLease.resizeExact}. */
  resizeLeaseExact(previous: bigint, next: bigint): void {
    if (previous <= 0n || next <= 0n)
      throw new YasProtocolError("exact receive reservation must be positive");
    if (previous > this.leased)
      throw new YasProtocolError("receive budget lease underflow");
    if (next > previous) {
      const growth = next - previous;
      const available =
        this.limit > this.leased ? this.limit - this.leased : 0n;
      if (growth > available)
        throw new YasResultError(
          YAS_STATUS_RESOURCE_EXHAUSTED,
          new Uint8Array(0),
          "aggregate YAS receive budget exhausted",
        );
      this.leased += growth;
    } else {
      this.leased -= previous - next;
      if (next < previous) this.notifyCapacityAvailable();
    }
  }

  release(bytes: bigint): void {
    if (bytes > this.leased)
      throw new YasProtocolError("receive budget lease underflow");
    this.leased -= bytes;
    if (bytes > 0n) this.notifyCapacityAvailable();
  }

  private notifyCapacityAvailable(): void {
    for (const listener of this.capacityListeners) {
      try {
        listener();
      } catch (error) {
        // Accounting has already been released. An observer must not turn that
        // successful cleanup into a failed resource lifecycle.
        console.error("YAS receive-budget capacity listener failed", error);
      }
    }
  }
}

/** A complete YAS v1 client session over a browser message or Relay stream transport. */
export class YasConnection {
  readonly receiveBudget: YasReceiveBudget;
  readonly options: YasClientHelloOptions;

  private readonly framing: "message" | "stream";
  private readonly streamDecoder = new YasStreamFrameDecoder();
  private readonly pending = new Map<number, PendingRequest>();
  private readonly incoming = new Map<number, IncomingRequestState>();
  private readonly eventListeners = new Map<string, Set<EventListener>>();
  private readonly requestHandlers = new Map<string, YasRequestHandler>();
  private readonly allEventListeners = new Set<EventListener>();
  private readonly invalidationListeners = new Set<InvalidationListener>();
  private readonly readyListeners = new Set<ReadyListener>();
  private readonly catalogChangeListeners = new Set<CatalogChangeListener>();
  private readonly familyLimitValidators = new Map<
    number,
    (limits: readonly YasExtension[]) => void
  >();
  private nextRequestId = 2;
  private helloDeferred: Deferred<YasServerHello> = deferred();
  /** True only while at least one caller can observe helloDeferred. */
  private helloAwaited = false;
  private handshakeStarted = false;
  private disposed = false;
  private _goAway: YasGoAway | null = null;
  private serverClockAnchor: {
    serverNs: bigint;
    clientMonotonicMs: number;
  } | null = null;
  private catalogSyncPending = false;
  private catalogGapTarget = 0n;
  private protocolFailureCause: YasProtocolError | null = null;
  private _hello: YasServerHello | null = null;
  private _families = new Map<number, YasFamilyDescriptor>();
  private readonly requestedReceiveMaxDatagram: number | undefined;
  private datagramReceived = 0;
  private datagramDelivered = 0;
  private datagramDropped = 0;

  private readonly onMessageBound = (data: YasTransportMessage) =>
    this.onMessage(data);
  private readonly onDatagramBound = (data: YasTransportMessage) =>
    this.onDatagram(data);
  private readonly onStatusBound = (status: ConnectionStatus) =>
    this.onStatus(status);

  constructor(
    readonly transport: YasTransport,
    options: YasConnectionOptions = {},
  ) {
    const clientInstance = options.clientInstance ?? randomUuidBytes();
    this.requestedReceiveMaxDatagram = options.receiveMaxDatagram;
    this.options = {
      ...options,
      clientInstance,
      minMinor: options.minMinor ?? 0,
      maxMinor: options.maxMinor ?? 0,
      receiveMaxFrame: options.receiveMaxFrame ?? 1024 * 1024,
      receiveMaxDecoded: options.receiveMaxDecoded ?? 4 * 1024 * 1024,
      receiveMaxDatagram: options.receiveMaxDatagram ?? 0,
      receiveMaxBuffered: options.receiveMaxBuffered ?? 16n * 1024n * 1024n,
    };
    this.receiveBudget = new YasReceiveBudget(this.options.receiveMaxBuffered!);
    this.framing = transport.yasFraming ?? "message";
    transport.addEventListener("message", this.onMessageBound);
    transport.addEventListener("datagram", this.onDatagramBound);
    transport.addEventListener("statuschange", this.onStatusBound);
  }

  get ready(): boolean {
    return (
      this.transport.status === "connected" &&
      this._hello !== null &&
      this._goAway === null
    );
  }

  get hello(): YasServerHello | null {
    return this._hello;
  }

  /** Set after GOAWAY while already-admitted work is still draining. */
  get goAway(): YasGoAway | null {
    return this._goAway;
  }

  /** Best local estimate for scheduling one absolute server-monotonic deadline. */
  nanosecondsUntilServerTime(deadlineServerNs: bigint): bigint {
    if (deadlineServerNs === 0n) return 0n;
    const estimatedServerNs = this.estimatedServerMonotonicNs();
    if (estimatedServerNs === null) return 0n;
    return deadlineServerNs > estimatedServerNs
      ? deadlineServerNs - estimatedServerNs
      : 0n;
  }

  /** Best local estimate of the server's current monotonic clock.
   *
   * Catalogue timestamps such as client connection age are absolute in this
   * clock. Returning the HELLO sample itself would freeze those values for the
   * entire connection, so consumers use the same anchored elapsed time as
   * deadline scheduling. */
  estimatedServerMonotonicNs(): bigint | null {
    const anchor = this.serverClockAnchor;
    if (!anchor) return null;
    const elapsedMs = Math.max(
      0,
      monotonicMilliseconds() - anchor.clientMonotonicMs,
    );
    // Millisecond clocks do not justify pretending to nanosecond precision.
    // Converting through integer microseconds also stays exact for long-lived
    // browser sessions before crossing Number's integer boundary.
    return anchor.serverNs + BigInt(Math.floor(elapsedMs * 1_000)) * 1_000n;
  }

  get families(): ReadonlyMap<number, YasFamilyDescriptor> {
    return this._families;
  }

  /** Optional-path receive accounting. Invalid datagrams never fail the session. */
  get datagramCounters(): YasDatagramCounters {
    return {
      received: this.datagramReceived,
      delivered: this.datagramDelivered,
      dropped: this.datagramDropped,
    };
  }

  async connect(): Promise<YasServerHello> {
    if (this.disposed)
      throw new YasDisconnectedError("YAS connection is closed");
    this.helloAwaited = true;
    if (this.transport.status === "connected") this.beginHandshake();
    else this.transport.connect();
    return this.helloDeferred.promise;
  }

  close(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.transport.removeEventListener("message", this.onMessageBound);
    this.transport.removeEventListener("datagram", this.onDatagramBound);
    this.transport.removeEventListener("statuschange", this.onStatusBound);
    this.failSession(new YasDisconnectedError("YAS connection closed"));
    this.transport.close();
  }

  family(family: number, version = 1): YasFamilyDescriptor {
    const descriptor = this._families.get(family);
    if (!descriptor || descriptor.version !== version)
      throw new YasResultError(
        YAS_STATUS_UNSUPPORTED,
        new Uint8Array(0),
        `YAS family 0x${family.toString(16)} version ${version} was not negotiated`,
      );
    if (descriptor.runtimeState === YAS_RUNTIME_UNAVAILABLE)
      throw new YasResultError(
        YAS_STATUS_UNAVAILABLE,
        new Uint8Array(0),
        `YAS family 0x${family.toString(16)} is unavailable`,
      );
    return descriptor;
  }

  /**
   * Register the canonical limit parser for a selected family. Registration
   * validates the current descriptor immediately and all later reconnect,
   * FAMILY_UPDATE, and SESSION_INFO replacement descriptors before use.
   */
  registerFamilyLimitValidator(
    family: number,
    validate: (limits: readonly YasExtension[]) => void,
  ): void {
    this.familyLimitValidators.set(family, validate);
    const descriptor = this._families.get(family);
    if (descriptor) validate(descriptor.limits);
  }

  operationAdvertised(
    family: number,
    frameClass: number,
    kind: number,
    serverSends = false,
  ): boolean {
    const descriptor = this._families.get(family);
    if (!descriptor || descriptor.runtimeState === YAS_RUNTIME_UNAVAILABLE)
      return false;
    const direction = serverSends
      ? YAS_DIRECTION_SERVER_SENDS
      : YAS_DIRECTION_SERVER_ACCEPTS;
    return descriptor.operations.some(
      (operation) =>
        operation.class === frameClass &&
        operation.kind === kind &&
        (operation.direction & direction) !== 0,
    );
  }

  request(
    family: number,
    kind: number,
    payload: Uint8Array = new Uint8Array(0),
    sensitive?: boolean,
  ): Promise<Uint8Array> {
    if (!this.ready)
      return Promise.reject(
        new YasDisconnectedError("YAS session is not ready"),
      );
    this.family(family);
    if (!this.operationAdvertised(family, YAS_CLASS_REQUEST, kind))
      return Promise.reject(
        new YasResultError(
          YAS_STATUS_UNSUPPORTED,
          new Uint8Array(0),
          "YAS Request was not advertised by the server",
        ),
      );
    return this.sendRequest(family, kind, payload, sensitive);
  }

  /**
   * Send a Request while preserving its complete ResultPrefix outcome.
   * Transport/protocol failures still reject, but a family status such as
   * UNAVAILABLE or CANCELLED resolves with its exact status, detail, and body.
   */
  requestResult(
    family: number,
    kind: number,
    payload: Uint8Array = new Uint8Array(0),
    sensitive?: boolean,
  ): Promise<YasResultEnvelope> {
    if (!this.ready)
      return Promise.reject(
        new YasDisconnectedError("YAS session is not ready"),
      );
    this.family(family);
    if (!this.operationAdvertised(family, YAS_CLASS_REQUEST, kind))
      return Promise.reject(
        new YasResultError(
          YAS_STATUS_UNSUPPORTED,
          new Uint8Array(0),
          "YAS Request was not advertised by the server",
        ),
      );
    return this.sendRequestResult(family, kind, payload, sensitive);
  }

  /** Decode a Result synchronously, before a following Event can be dispatched. */
  requestDecoded<T>(
    family: number,
    kind: number,
    payload: Uint8Array,
    decode: (body: Uint8Array) => T,
    sensitive?: boolean,
  ): Promise<T> {
    if (!this.ready)
      return Promise.reject(
        new YasDisconnectedError("YAS session is not ready"),
      );
    this.family(family);
    if (!this.operationAdvertised(family, YAS_CLASS_REQUEST, kind))
      return Promise.reject(
        new YasResultError(
          YAS_STATUS_UNSUPPORTED,
          new Uint8Array(0),
          "YAS Request was not advertised by the server",
        ),
      );
    return this.sendRequest(family, kind, payload, sensitive, decode);
  }

  sendEvent(
    family: number,
    kind: number,
    payload: Uint8Array = new Uint8Array(0),
    sensitive?: boolean,
  ): void {
    if (!this.ready) throw new YasDisconnectedError("YAS session is not ready");
    this.family(family);
    if (!this.operationAdvertised(family, YAS_CLASS_EVENT, kind))
      throw new YasResultError(
        YAS_STATUS_UNSUPPORTED,
        new Uint8Array(0),
        "YAS Event was not advertised by the server",
      );
    this.writeFrame(
      encodeYasFrame({
        family,
        kind,
        class: YAS_CLASS_EVENT,
        sensitive,
        payload,
      }),
    );
  }

  /**
   * Send one datagram-safe Event on the optional unreliable path.
   * Returns false when the negotiated path cannot carry the complete frame;
   * callers that selected reliable delivery can then use {@link sendEvent}.
   */
  sendDatagramEvent(
    family: number,
    kind: number,
    payload: Uint8Array = new Uint8Array(0),
    sensitive?: boolean,
  ): boolean {
    if (!this.ready) throw new YasDisconnectedError("YAS session is not ready");
    this.family(family);
    if (!this.operationAdvertised(family, YAS_CLASS_EVENT, kind))
      throw new YasResultError(
        YAS_STATUS_UNSUPPORTED,
        new Uint8Array(0),
        "YAS Event was not advertised by the server",
      );
    const send = this.transport.sendDatagram;
    const transportMaximum = this.transport.maxDatagramSize ?? 0;
    const peerMaximum = this._hello?.receiveMaxDatagram ?? 0;
    if (!send || transportMaximum === 0 || peerMaximum === 0) return false;
    const frame = encodeYasFrame({
      family,
      kind,
      class: YAS_CLASS_EVENT,
      sensitive,
      payload,
    });
    validateYasDatagramFrame(decodeYasFrame(frame, YAS_MAX_DATAGRAM));
    if (
      frame.length > Math.min(transportMaximum, peerMaximum, YAS_MAX_DATAGRAM)
    )
      return false;
    send.call(this.transport, frame);
    return true;
  }

  onEvent(family: number, kind: number, listener: EventListener): () => void {
    const key = `${family}/${kind}`;
    let listeners = this.eventListeners.get(key);
    if (!listeners) {
      listeners = new Set();
      this.eventListeners.set(key, listeners);
    }
    listeners.add(listener);
    return () => {
      listeners!.delete(listener);
      if (listeners!.size === 0) this.eventListeners.delete(key);
    };
  }

  onAnyEvent(listener: EventListener): () => void {
    this.allEventListeners.add(listener);
    return () => this.allEventListeners.delete(listener);
  }

  /** Resource clients use this to tear down session- or family-scoped state. */
  onInvalidation(listener: InvalidationListener): () => void {
    this.invalidationListeners.add(listener);
    return () => this.invalidationListeners.delete(listener);
  }

  /** Observe every successfully validated HELLO, including physical reconnects. */
  onReady(listener: ReadyListener): () => void {
    this.readyListeners.add(listener);
    if (this.ready && this._hello)
      this.notifyReadyListener(listener, this._hello);
    return () => this.readyListeners.delete(listener);
  }

  /** Observe applied FAMILY_UPDATE and SESSION_INFO catalogue replacements. */
  onCatalogChange(listener: CatalogChangeListener): () => void {
    this.catalogChangeListeners.add(listener);
    return () => this.catalogChangeListeners.delete(listener);
  }

  handleRequests(
    family: number,
    kind: number,
    handler: YasRequestHandler,
  ): () => void {
    const key = `${family}/${kind}`;
    if (this.requestHandlers.has(key))
      throw new YasProtocolError("a YAS Request handler is already registered");
    this.requestHandlers.set(key, handler);
    return () => this.requestHandlers.delete(key);
  }

  async ping(
    senderMonotonicNs: bigint,
  ): Promise<ReturnType<typeof decodePingResult>> {
    const body = await this.request(
      YAS_FAMILY_CORE,
      YAS_CORE_PING,
      encodePing(senderMonotonicNs),
    );
    return decodePingResult(body);
  }

  async sessionInfo(): Promise<ReturnType<typeof decodeSessionInfo>> {
    return decodeSessionInfo(
      await this.request(YAS_FAMILY_CORE, YAS_CORE_SESSION_INFO),
    );
  }

  async cancel(targetRequestId: number): Promise<void> {
    await this.request(
      YAS_FAMILY_CORE,
      YAS_CORE_CANCEL,
      encodeCancel(targetRequestId),
    );
  }

  /** Ask the server to shut down. Its Result resolves before GOAWAY draining. */
  async shutdown(
    operationId: Uint8Array,
    graceNs: bigint,
    reason: string,
  ): Promise<void> {
    await this.request(
      YAS_FAMILY_CORE,
      YAS_CORE_SHUTDOWN,
      encodeShutdown({ operationId, graceNs, reason }),
      true,
    );
  }

  private beginHandshake(): void {
    if (this.handshakeStarted || this.disposed) return;
    this.handshakeStarted = true;
    this._goAway = null;
    this.streamDecoder.reset();
    const transportMaximum = Math.min(
      this.transport.maxDatagramSize ?? 0,
      YAS_MAX_DATAGRAM,
    );
    this.options.receiveMaxDatagram =
      this.requestedReceiveMaxDatagram === undefined
        ? transportMaximum
        : Math.min(this.requestedReceiveMaxDatagram, transportMaximum);
    const payload = encodeClientHello(this.options);
    const helloDeferred = this.helloDeferred;
    const id = 1;
    const frame = encodeYasFrame({
      family: YAS_FAMILY_CORE,
      kind: YAS_CORE_HELLO,
      class: YAS_CLASS_REQUEST,
      requestId: id,
      sensitive: false,
      payload,
    });
    this.pending.set(id, {
      family: YAS_FAMILY_CORE,
      kind: YAS_CORE_HELLO,
      decode: (body) => {
        const hello = decodeServerHello(body);
        const families = validateServerHello(hello, this.options);
        this.validateRegisteredFamilyLimits(families);
        this._hello = hello;
        this.serverClockAnchor = {
          serverNs: hello.serverMonotonicNs,
          clientMonotonicMs: monotonicMilliseconds(),
        };
        this._families = families;
        this.receiveBudget.setLimit(this.options.receiveMaxBuffered!);
        if (this.framing === "stream")
          this.streamDecoder.setMaxFrame(this.options.receiveMaxFrame!);
        this.emitReady(hello);
        helloDeferred.resolve(hello);
        return body;
      },
      resolve: () => undefined,
      reject: (error) => helloDeferred.reject(error),
    });
    if (this.framing === "message") {
      this.transport.send(YAS_PREFACE);
      this.transport.send(frame);
    } else {
      const framed = frameForByteStream(frame);
      const startup = new Uint8Array(YAS_PREFACE.length + framed.length);
      startup.set(YAS_PREFACE);
      startup.set(framed, YAS_PREFACE.length);
      this.transport.send(startup);
    }
  }

  private sendRequest<T = Uint8Array>(
    family: number,
    kind: number,
    payload: Uint8Array,
    sensitive: boolean | undefined,
    decode?: (body: Uint8Array) => T,
  ): Promise<T> {
    const requestId = this.allocateRequestId();
    let rejectPromise!: (error: unknown) => void;
    const promise = new Promise<T>((resolve, reject) => {
      rejectPromise = reject;
      this.pending.set(requestId, {
        family,
        kind,
        decode,
        resolve: resolve as (value: unknown) => void,
        reject,
      });
    });
    try {
      this.writeFrame(
        encodeYasFrame({
          family,
          kind,
          class: YAS_CLASS_REQUEST,
          requestId,
          sensitive,
          payload,
        }),
      );
    } catch (error) {
      this.pending.delete(requestId);
      rejectPromise(error);
      this.protocolFailure(error);
    }
    return promise;
  }

  private sendRequestResult(
    family: number,
    kind: number,
    payload: Uint8Array,
    sensitive: boolean | undefined,
  ): Promise<YasResultEnvelope> {
    const requestId = this.allocateRequestId();
    let rejectPromise!: (error: unknown) => void;
    const promise = new Promise<YasResultEnvelope>((resolve, reject) => {
      rejectPromise = reject;
      this.pending.set(requestId, {
        family,
        kind,
        preserveResult: true,
        resolve: resolve as (value: unknown) => void,
        reject,
      });
    });
    try {
      this.writeFrame(
        encodeYasFrame({
          family,
          kind,
          class: YAS_CLASS_REQUEST,
          requestId,
          sensitive,
          payload,
        }),
      );
    } catch (error) {
      this.pending.delete(requestId);
      rejectPromise(error);
      this.protocolFailure(error);
    }
    return promise;
  }

  private allocateRequestId(): number {
    for (let attempts = 0; attempts < 0xffff_ffff; attempts++) {
      const id = this.nextRequestId;
      this.nextRequestId = id === 0xffff_ffff ? 1 : id + 1;
      if (id !== 0 && !this.pending.has(id)) return id;
    }
    throw new YasProtocolError("YAS request ID space exhausted");
  }

  private writeFrame(frame: Uint8Array): void {
    if (
      frame.length > (this._hello?.receiveMaxFrame ?? YAS_MAX_PRE_HELLO_FRAME)
    )
      throw new YasProtocolError(
        "outgoing YAS frame exceeds peer receive limit",
      );
    this.transport.send(
      this.framing === "stream" ? frameForByteStream(frame) : frame,
    );
  }

  private onMessage(data: YasTransportMessage): void {
    if (this.disposed) return;
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    try {
      if (this.framing === "stream") {
        for (const frame of this.streamDecoder.push(bytes))
          this.dispatchFrame(frame);
      } else {
        if (bytes.length === 0)
          throw new YasProtocolError("empty YAS transport message");
        this.dispatchFrame(bytes);
      }
    } catch (error) {
      this.protocolFailure(error);
    }
  }

  private onDatagram(data: YasTransportMessage): void {
    if (this.disposed) return;
    this.datagramReceived++;
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    try {
      const receiveMaximum = this.options.receiveMaxDatagram ?? 0;
      if (
        !this._hello ||
        receiveMaximum === 0 ||
        bytes.length > receiveMaximum ||
        bytes.length > YAS_MAX_DATAGRAM
      )
        throw new YasProtocolError("YAS datagram is outside the receive limit");
      const frame = decodeYasFrame(bytes, receiveMaximum);
      validateYasDatagramFrame(frame);
      if (!this._families.has(frame.family))
        throw new YasProtocolError(
          "datagram received for an unselected YAS family",
        );
      if (
        !this.operationAdvertised(
          frame.family,
          YAS_CLASS_EVENT,
          frame.kind,
          true,
        )
      )
        throw new YasProtocolError(
          "server sent a datagram Event that was not advertised",
        );
      this.dispatchEvent(frame, true);
      this.datagramDelivered++;
    } catch {
      // Loss includes malformed, stale, oversized, and semantically invalid
      // datagrams. The reliable session remains authoritative and alive.
      this.datagramDropped++;
    }
  }

  private dispatchFrame(bytes: Uint8Array): void {
    const frame = decodeYasFrame(
      bytes,
      this._hello ? this.options.receiveMaxFrame! : YAS_MAX_PRE_HELLO_FRAME,
    );
    if (frame.class === YAS_CLASS_RESULT) {
      const pending = this.pending.get(frame.requestId!);
      if (!pending)
        throw new YasProtocolError("unsolicited or duplicate YAS Result");
      if (pending.family !== frame.family || pending.kind !== frame.kind)
        throw new YasProtocolError(
          "YAS Result family or kind does not match its Request",
        );
      const result = decodeResultPayload(frame.payload);
      this.pending.delete(frame.requestId!);
      if (pending.preserveResult) {
        pending.resolve({
          status: result.status,
          detail: new Uint8Array(result.detail),
          body: new Uint8Array(result.body),
        } satisfies YasResultEnvelope);
      } else if (result.status === YAS_STATUS_OK) {
        const body = new Uint8Array(result.body);
        try {
          const value = pending.decode ? pending.decode(body) : body;
          pending.resolve(value);
        } catch (error) {
          pending.reject(error);
          throw error;
        }
      } else {
        pending.reject(
          new YasResultError(result.status, new Uint8Array(result.detail)),
        );
      }
      return;
    }
    if (!this._hello)
      throw new YasProtocolError(
        "non-HELLO frame received before negotiation completed",
      );
    if (!this._families.has(frame.family))
      throw new YasProtocolError("frame received for an unselected YAS family");
    if (frame.class === YAS_CLASS_EVENT) {
      if (
        !this.operationAdvertised(
          frame.family,
          YAS_CLASS_EVENT,
          frame.kind,
          true,
        )
      )
        throw new YasProtocolError(
          "server sent an Event that was not advertised",
        );
      this.handleCoreEvent(frame.family, frame.kind, frame.payload);
      this.dispatchEvent(frame, false);
      return;
    }
    if (
      !this.operationAdvertised(
        frame.family,
        YAS_CLASS_REQUEST,
        frame.kind,
        true,
      )
    )
      throw new YasProtocolError(
        "server sent a Request that was not advertised",
      );
    const requestId = frame.requestId!;
    if (this.incoming.has(requestId))
      throw new YasProtocolError("server reused an active Request ID");
    if (this.incoming.size >= YAS_MAX_RETAINED_INCOMING_REQUESTS)
      throw new YasProtocolError("too many retained incoming YAS Requests");
    let payloadLease: YasReceiveBudgetLease | undefined;
    if (frame.payload.length !== 0) {
      try {
        payloadLease = this.receiveBudget.reserveExact(
          BigInt(frame.payload.length),
        );
      } catch (error) {
        // A full receive budget is backpressure, not a protocol violation.
        // Throwing here reaches `onMessage` and kills an otherwise healthy
        // session; refuse this one Request instead and keep the rest of the
        // session running until a lease is released elsewhere.
        if (
          !(error instanceof YasResultError) ||
          error.status !== YAS_STATUS_RESOURCE_EXHAUSTED
        )
          throw error;
        this.refuseIncomingRequest(frame, YAS_STATUS_RESOURCE_EXHAUSTED);
        return;
      }
    }
    let payload: Uint8Array;
    try {
      payload = new Uint8Array(frame.payload);
    } catch (error) {
      payloadLease?.release();
      throw error;
    }
    const state: IncomingRequestState = {
      controller: new AbortController(),
      payloadLease,
    };
    this.incoming.set(requestId, state);
    void this.handleIncomingRequest(
      {
        family: frame.family,
        kind: frame.kind,
        requestId,
        sensitive: frame.sensitive,
        payload,
        signal: state.controller.signal,
      },
      state,
    ).catch((error) => this.protocolFailure(error));
  }

  private dispatchEvent(frame: YasFrame, datagram: boolean): void {
    const event: YasEvent = {
      family: frame.family,
      kind: frame.kind,
      sensitive: frame.sensitive,
      payload: new Uint8Array(frame.payload),
      datagram,
    };
    for (const listener of this.eventListeners.get(
      `${frame.family}/${frame.kind}`,
    ) ?? [])
      listener(event);
    for (const listener of this.allEventListeners) listener(event);
  }

  /**
   * Answer a Request that was never admitted.
   *
   * Nothing was retained for it: no incoming state, no receive lease, and no
   * handler ran. The peer still correlated the Request, so it gets a Result
   * rather than silence, and may retry once the aggregate budget recovers.
   */
  private refuseIncomingRequest(frame: YasFrame, status: number): void {
    if (!this._hello || this.disposed) return;
    this.writeFrame(
      encodeYasFrame({
        family: frame.family,
        kind: frame.kind,
        class: YAS_CLASS_RESULT,
        requestId: frame.requestId!,
        sensitive: frame.sensitive,
        payload: encodeResultPayload(
          status,
          new Uint8Array(0),
          new Uint8Array(0),
        ),
      }),
    );
  }

  private async handleIncomingRequest(
    request: YasIncomingRequest,
    state: IncomingRequestState,
  ): Promise<void> {
    let status: number = YAS_STATUS_UNSUPPORTED;
    let body: Uint8Array = new Uint8Array(0);
    let detail: Uint8Array = new Uint8Array(0);
    let afterSend: YasRequestResponse["afterSend"];
    try {
      if (this._goAway) {
        status = YAS_STATUS_UNAVAILABLE;
      } else if (
        request.family === YAS_FAMILY_CORE &&
        request.kind === YAS_CORE_PING
      ) {
        try {
          const receiverReceiveNs = monotonicNanoseconds();
          decodePing(request.payload);
          status = YAS_STATUS_OK;
          body = encodePingResult({
            receiverReceiveNs,
            receiverSendNs: monotonicNanoseconds(),
          });
        } catch (error) {
          if (!(error instanceof YasProtocolError)) throw error;
          status = YAS_STATUS_INVALID;
        }
      } else if (
        request.family === YAS_FAMILY_CORE &&
        request.kind === YAS_CORE_CANCEL
      ) {
        try {
          const targetRequestId = decodeCancel(request.payload);
          const target = this.incoming.get(targetRequestId);
          if (!target || target === state) status = YAS_STATUS_NOT_FOUND;
          else {
            target.controller.abort();
            status = YAS_STATUS_OK;
          }
        } catch (error) {
          if (!(error instanceof YasProtocolError)) throw error;
          status = YAS_STATUS_INVALID;
        }
      } else {
        const handler = this.requestHandlers.get(
          `${request.family}/${request.kind}`,
        );
        if (handler) {
          try {
            const response = await cancelableIncomingResponse(
              Promise.resolve(handler(request)),
              request.signal,
            );
            if (response === CANCELLED_INCOMING_REQUEST) {
              status = YAS_STATUS_CANCELLED;
            } else if (response instanceof Uint8Array) {
              status = YAS_STATUS_OK;
              body = response;
            } else {
              status = response.status;
              body = response.body ?? body;
              detail = response.detail ?? detail;
              afterSend = response.afterSend;
            }
          } catch {
            status = request.signal.aborted
              ? YAS_STATUS_CANCELLED
              : YAS_STATUS_INTERNAL;
          }
        }
      }
      // Results for Requests admitted before GOAWAY still drain until the
      // physical session closes.
      if (!this._hello || this.disposed) return;
      this.writeFrame(
        encodeYasFrame({
          family: request.family,
          kind: request.kind,
          class: YAS_CLASS_RESULT,
          requestId: request.requestId,
          sensitive: request.sensitive,
          payload: encodeResultPayload(status, body, detail),
        }),
      );
      if (afterSend) void Promise.resolve().then(afterSend);
    } finally {
      if (this.incoming.get(request.requestId) === state)
        this.incoming.delete(request.requestId);
      state.payloadLease?.release();
    }
  }

  private handleCoreEvent(
    family: number,
    kind: number,
    payload: Uint8Array,
  ): void {
    if (family !== YAS_FAMILY_CORE) return;
    if (kind === YAS_CORE_GOAWAY) {
      this._goAway = decodeGoAway(payload);
      return;
    }
    if (kind === YAS_CORE_SESSION_UPDATE) {
      const update = decodeSessionUpdate(payload);
      const revision = update.catalogRevision;
      if (revision <= this._hello!.catalogRevision)
        throw new YasProtocolError("non-increasing SESSION_UPDATE revision");
      validateReceiveLimitUpdate(update, this._hello!, "server");
      if (revision !== this._hello!.catalogRevision + 1n) {
        this.noteCatalogGap(revision);
        return;
      }
      if (this.catalogSyncPending) {
        this.noteCatalogGap(revision);
        return;
      }
      Object.assign(this._hello!, {
        catalogRevision: revision,
        receiveMaxFrame: update.receiveMaxFrame,
        receiveMaxDecoded: update.receiveMaxDecoded,
        receiveMaxDatagram: update.receiveMaxDatagram,
        receiveMaxBuffered: update.receiveMaxBuffered,
      });
      return;
    }
    if (kind === YAS_CORE_FAMILY_UPDATE) {
      const update = decodeFamilyUpdate(payload);
      const revision = update.catalogRevision;
      const descriptor = update.descriptor;
      if (revision <= this._hello!.catalogRevision)
        throw new YasProtocolError("non-increasing FAMILY_UPDATE revision");
      const previous = this._families.get(descriptor.family);
      if (!previous || previous.version !== descriptor.version)
        throw new YasProtocolError(
          "FAMILY_UPDATE changed an unselected family or version",
        );
      if (revision !== this._hello!.catalogRevision + 1n) {
        this.noteCatalogGap(revision);
        return;
      }
      if (this.catalogSyncPending) {
        this.noteCatalogGap(revision);
        return;
      }
      const offer = this.options.families?.find(
        (value) => value.family === descriptor.family,
      );
      if (
        descriptor.family !== YAS_FAMILY_CORE &&
        (!offer || !offer.versions.includes(descriptor.version))
      )
        throw new YasProtocolError(
          "FAMILY_UPDATE selected an unoffered version",
        );
      this.validateFamilyLimits(descriptor);
      this._hello!.catalogRevision = revision;
      this._families.set(descriptor.family, descriptor);
      this._hello!.families = [...this._families.values()].sort(
        (left, right) => left.family - right.family,
      );
      const previousOperations = new Set(
        previous?.operations.map(
          (operation) =>
            `${operation.direction}/${operation.class}/${operation.kind}`,
        ) ?? [],
      );
      const newOperations = new Set(
        descriptor.operations.map(
          (operation) =>
            `${operation.direction}/${operation.class}/${operation.kind}`,
        ),
      );
      const disabledOperation = [...previousOperations].some(
        (operation) => !newOperations.has(operation),
      );
      if (
        descriptor.runtimeState === YAS_RUNTIME_UNAVAILABLE ||
        descriptor.version !== previous?.version ||
        disabledOperation
      )
        this.invalidateFamilyCascade(
          [descriptor.family],
          `YAS family 0x${descriptor.family.toString(16)} became unavailable or changed incompatibly`,
        );
      this.emitCatalogChange(revision, [descriptor.family]);
    }
  }

  private noteCatalogGap(revision: bigint): void {
    if (revision > this.catalogGapTarget) this.catalogGapTarget = revision;
    if (this.catalogSyncPending) return;
    this.catalogSyncPending = true;
    void this.resyncCatalog().catch((error) => {
      if (this._hello) this.protocolFailure(error);
    });
  }

  private async resyncCatalog(): Promise<void> {
    let lastRevision = this._hello?.catalogRevision ?? 0n;
    while (this.ready) {
      const target = this.catalogGapTarget;
      const info = await this.sessionInfo();
      if (!this._hello) return;
      if (!sameBytes(info.sessionId, this._hello.sessionId))
        throw new YasProtocolError("SESSION_INFO session ID changed");
      if (info.catalogRevision < this._hello.catalogRevision)
        throw new YasProtocolError("SESSION_INFO catalogue revision regressed");
      if (info.catalogRevision <= lastRevision && info.catalogRevision < target)
        throw new YasProtocolError(
          "SESSION_INFO catalogue resynchronization made no progress",
        );
      lastRevision = info.catalogRevision;
      validateReceiveLimitUpdate(info, this._hello, "server");
      if (
        info.families.length !== this._hello.families.length ||
        info.families.some((family, index) => {
          const previous = this._hello!.families[index];
          return (
            !previous ||
            family.family !== previous.family ||
            family.version !== previous.version
          );
        })
      )
        throw new YasProtocolError(
          "SESSION_INFO changed selected families or versions",
        );
      const replacement = validateServerHello(
        {
          ...this._hello,
          catalogRevision: info.catalogRevision,
          receiveMaxFrame: info.receiveMaxFrame,
          receiveMaxDecoded: info.receiveMaxDecoded,
          receiveMaxDatagram: info.receiveMaxDatagram,
          receiveMaxBuffered: info.receiveMaxBuffered,
          serverMonotonicNs: info.serverMonotonicNs,
          families: info.families,
        },
        this.options,
      );
      this.validateRegisteredFamilyLimits(replacement);
      const previous = this._families;
      Object.assign(this._hello, {
        catalogRevision: info.catalogRevision,
        receiveMaxFrame: info.receiveMaxFrame,
        receiveMaxDecoded: info.receiveMaxDecoded,
        receiveMaxDatagram: info.receiveMaxDatagram,
        receiveMaxBuffered: info.receiveMaxBuffered,
        serverMonotonicNs: info.serverMonotonicNs,
        families: info.families,
      });
      this._families = replacement;
      this.serverClockAnchor = {
        serverNs: info.serverMonotonicNs,
        clientMonotonicMs: monotonicMilliseconds(),
      };
      const invalidatedFamilies: number[] = [];
      for (const [family, descriptor] of previous) {
        const next = replacement.get(family);
        if (!next || familyDescriptorInvalidated(descriptor, next))
          invalidatedFamilies.push(family);
      }
      this.invalidateFamilyCascade(
        invalidatedFamilies,
        "One or more YAS families or their dependencies changed incompatibly",
      );
      this.emitCatalogChange(info.catalogRevision, [
        ...new Set([...previous.keys(), ...replacement.keys()]),
      ]);
      if (info.catalogRevision >= target) break;
    }
    this.catalogSyncPending = false;
    this.catalogGapTarget = 0n;
  }

  private onStatus(status: ConnectionStatus): void {
    if (this.disposed) return;
    if (status === "connected") {
      this.beginHandshake();
      return;
    }
    if (
      status === "disconnected" ||
      status === "error" ||
      status === "closed"
    ) {
      this.failSession(
        this.protocolFailureCause ??
          new YasDisconnectedError(this.transport.lastError ?? undefined),
      );
    }
  }

  private failSession(error: unknown): void {
    const pending = [...this.pending.values()];
    const incoming = [...this.incoming.values()];
    this.pending.clear();
    this.incoming.clear();
    const previousHello = this.helloDeferred;
    const rejectUnfinishedHello = !this._hello && this.helloAwaited;
    this.helloDeferred = deferred();
    this.helloAwaited = false;
    const invalidationError =
      error instanceof Error ? error : new YasDisconnectedError(String(error));
    this._hello = null;
    this._goAway = null;
    this.serverClockAnchor = null;
    this._families = new Map();
    this.handshakeStarted = false;
    this.catalogSyncPending = false;
    this.catalogGapTarget = 0n;
    if (rejectUnfinishedHello) previousHello.reject(error);
    for (const request of pending) request.reject(error);
    // Abort dispatch is synchronous. All observable session state and maps are
    // already dead so cleanup callbacks cannot send onto the failed transport.
    for (const request of incoming) request.controller.abort();
    this.emitInvalidation({ error: invalidationError });
  }

  private emitReady(hello: YasServerHello): void {
    for (const listener of [...this.readyListeners])
      this.notifyReadyListener(listener, hello);
  }

  private notifyReadyListener(listener: ReadyListener, hello: YasServerHello) {
    try {
      listener(hello);
    } catch (error) {
      this.reportListenerError("ready", error);
    }
  }

  private emitInvalidation(invalidation: YasInvalidation): void {
    for (const listener of [...this.invalidationListeners]) {
      try {
        listener(invalidation);
      } catch (error) {
        this.reportListenerError("invalidation", error);
      }
    }
  }

  private reportListenerError(kind: string, error: unknown): void {
    const reportError = (
      globalThis as typeof globalThis & {
        reportError?: (error: unknown) => void;
      }
    ).reportError;
    if (reportError) reportError(error);
    else console.error(`YAS ${kind} listener failed`, error);
  }

  private invalidateFamilyCascade(
    roots: readonly number[],
    message: string,
  ): void {
    const invalidated = new Set(roots);
    const queue = [...roots];
    while (queue.length !== 0) {
      const dependency = queue.shift()!;
      for (const family of this._families.keys()) {
        if (
          invalidated.has(family) ||
          !(YAS_FAMILY_DEPENDENCIES[family] ?? []).includes(dependency)
        )
          continue;
        invalidated.add(family);
        queue.push(family);
      }
    }
    for (const family of invalidated) {
      const error = new YasResultError(
        YAS_STATUS_UNAVAILABLE,
        new Uint8Array(0),
        message,
      );
      this.emitInvalidation({ family, error });
    }
  }

  private emitCatalogChange(
    revision: bigint,
    families: readonly number[],
  ): void {
    const change = { revision, families } satisfies YasCatalogChange;
    for (const listener of [...this.catalogChangeListeners]) listener(change);
  }

  private validateFamilyLimits(descriptor: YasFamilyDescriptor): void {
    this.familyLimitValidators.get(descriptor.family)?.(descriptor.limits);
  }

  private validateRegisteredFamilyLimits(
    families: ReadonlyMap<number, YasFamilyDescriptor>,
  ): void {
    for (const descriptor of families.values())
      this.validateFamilyLimits(descriptor);
  }

  private protocolFailure(error: unknown): void {
    const protocolError =
      error instanceof YasProtocolError
        ? error
        : new YasProtocolError(
            error instanceof Error ? error.message : String(error),
          );
    const previousCause = this.protocolFailureCause;
    this.protocolFailureCause = protocolError;
    try {
      this.failSession(protocolError);
      // Some transports synchronously publish `closed` from close(). Keep the
      // causal protocol error available to that status callback rather than
      // replacing it with a generic disconnected error.
      this.transport.close();
    } finally {
      this.protocolFailureCause = previousCause;
    }
  }
}

function monotonicMilliseconds(): number {
  return globalThis.performance?.now() ?? Date.now();
}

function monotonicNanoseconds(): bigint {
  return BigInt(Math.floor(monotonicMilliseconds() * 1_000)) * 1_000n;
}

async function cancelableIncomingResponse(
  response: Promise<Uint8Array | YasRequestResponse>,
  signal: AbortSignal,
): Promise<
  Uint8Array | YasRequestResponse | typeof CANCELLED_INCOMING_REQUEST
> {
  if (signal.aborted) return CANCELLED_INCOMING_REQUEST;
  let onAbort!: () => void;
  const cancelled = new Promise<typeof CANCELLED_INCOMING_REQUEST>(
    (resolve) => {
      onAbort = () => resolve(CANCELLED_INCOMING_REQUEST);
      signal.addEventListener("abort", onAbort, { once: true });
    },
  );
  try {
    return await Promise.race([response, cancelled]);
  } finally {
    signal.removeEventListener("abort", onAbort);
  }
}

function familyDescriptorInvalidated(
  previous: YasFamilyDescriptor,
  next: YasFamilyDescriptor,
): boolean {
  if (
    next.runtimeState === YAS_RUNTIME_UNAVAILABLE ||
    previous.version !== next.version
  )
    return true;
  const operations = new Set(
    next.operations.map(
      (operation) =>
        `${operation.direction}/${operation.class}/${operation.kind}`,
    ),
  );
  return previous.operations.some(
    (operation) =>
      !operations.has(
        `${operation.direction}/${operation.class}/${operation.kind}`,
      ),
  );
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function randomUuidBytes(): Uint8Array {
  const bytes = new Uint8Array(16);
  const source = globalThis.crypto;
  if (!source?.getRandomValues)
    throw new YasProtocolError(
      "secure randomness is required for YAS client_instance",
    );
  source.getRandomValues(bytes);
  bytes[6] = (bytes[6]! & 0x0f) | 0x40;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  return bytes;
}
