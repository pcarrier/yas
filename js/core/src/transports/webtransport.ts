import type {
  ConnectionStatus,
  YasTransport,
  YasTransportEventMap,
  YasTransportMessage,
  YasTransportOptions,
} from "../types";

const YAS_TRANSPORT_DATAGRAM_HARD_MAX = 65_536;
const AUTH_MAX_BYTES = 4_096;
const AUTH_BUSY = 2;

export interface YasWebTransportOptions extends YasTransportOptions {
  /** Exact SHA-256 certificate hash in hexadecimal for a self-signed peer. */
  serverCertificateHash?: string;
}

/** Native YAS byte stream plus WebTransport's paired unreliable datagrams. */
export class YasWebTransportTransport implements YasTransport {
  readonly yasFraming = "stream" as const;
  private session: WebTransport | null = null;
  private streamWriter: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private datagramWriter: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private _status: ConnectionStatus = "disconnected";
  private disposed = false;
  private suspended = false;
  private connectPromise: Promise<void> | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private retryDelay: number;
  private readonly messageListeners = new Set<
    (data: YasTransportMessage) => void
  >();
  private readonly datagramListeners = new Set<
    (data: YasTransportMessage) => void
  >();
  private readonly statusListeners = new Set<
    (status: ConnectionStatus) => void
  >();

  authRejected = false;
  lastError: string | null = null;

  private readonly reconnectEnabled: boolean;
  private readonly initialRetryDelay: number;
  private readonly maximumRetryDelay: number;
  private readonly retryBackoff: number;
  private readonly connectTimeoutMs: number;
  private readonly certificateHash?: Uint8Array;

  constructor(
    private readonly url: string,
    private readonly credential: string,
    options: YasWebTransportOptions = {},
  ) {
    this.reconnectEnabled = options.reconnect ?? true;
    this.initialRetryDelay = options.reconnectDelay ?? 500;
    this.maximumRetryDelay = options.maxReconnectDelay ?? 10_000;
    this.retryBackoff = options.reconnectBackoff ?? 1.5;
    this.connectTimeoutMs = options.connectTimeoutMs ?? 10_000;
    this.retryDelay = this.initialRetryDelay;
    if (options.serverCertificateHash)
      this.certificateHash = certificateHash(options.serverCertificateHash);
  }

  get status(): ConnectionStatus {
    return this._status;
  }

  get maxDatagramSize(): number {
    if (!this.session || !this.datagramWriter) return 0;
    const maximum = this.session.datagrams.maxDatagramSize;
    return Number.isSafeInteger(maximum) && maximum > 0
      ? Math.min(maximum, YAS_TRANSPORT_DATAGRAM_HARD_MAX)
      : 0;
  }

  connect(): void {
    if (this.disposed || this.connectPromise) return;
    this.suspended = false;
    const running = this.connectInternal().finally(() => {
      if (this.connectPromise === running) this.connectPromise = null;
    });
    this.connectPromise = running;
  }

  send(data: Uint8Array): void {
    const writer = this.streamWriter;
    if (!writer) return;
    void writer.write(copyBytes(data)).catch((error: unknown) => {
      if (this.streamWriter !== writer) return;
      this.lastError =
        error instanceof Error
          ? error.message
          : "WebTransport YAS stream write failed";
      this.cleanup();
      this.setStatus("disconnected");
      this.scheduleReconnect();
    });
  }

  sendDatagram(data: Uint8Array): void {
    const writer = this.datagramWriter;
    if (!writer || data.length > this.maxDatagramSize) return;
    void writer.write(copyBytes(data)).catch(() => {
      // The datagram path is optional. Stop advertising it immediately so
      // later events use the reliable YAS stream without reconnecting it.
      if (this.datagramWriter === writer) this.datagramWriter = null;
    });
  }

  reconnect(): void {
    if (this.disposed) return;
    this.suspend();
    this.suspended = false;
    this.connect();
  }

  suspend(): void {
    if (this.disposed) return;
    this.suspended = true;
    this.clearReconnect();
    this.cleanup();
    this.retryDelay = this.initialRetryDelay;
    this.setStatus("disconnected");
  }

  close(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.clearReconnect();
    this.cleanup();
    this.setStatus("closed");
  }

  addEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messageListeners.add(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "datagram")
      this.datagramListeners.add(
        listener as (data: YasTransportMessage) => void,
      );
    else
      this.statusListeners.add(listener as (status: ConnectionStatus) => void);
  }

  removeEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messageListeners.delete(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "datagram")
      this.datagramListeners.delete(
        listener as (data: YasTransportMessage) => void,
      );
    else
      this.statusListeners.delete(
        listener as (status: ConnectionStatus) => void,
      );
  }

  private async connectInternal(): Promise<void> {
    if (
      this.disposed ||
      this.suspended ||
      this._status === "connecting" ||
      this._status === "authenticating" ||
      this._status === "connected"
    )
      return;
    this.clearReconnect();
    this.setStatus("connecting");
    try {
      const options: WebTransportOptions = {};
      if (this.certificateHash)
        options.serverCertificateHashes = [
          {
            algorithm: "sha-256",
            value: copyBytes(this.certificateHash).buffer as ArrayBuffer,
          },
        ];
      const session = new WebTransport(this.url, options);
      this.session = session;
      await withTimeout(session.ready, this.connectTimeoutMs);
      if (!this.isCurrent(session)) return session.close();

      const stream = await session.createBidirectionalStream();
      const writer = stream.writable.getWriter();
      const authReader = stream.readable.getReader();
      this.setStatus("authenticating");
      const credential = new TextEncoder().encode(this.credential);
      if (credential.length > AUTH_MAX_BYTES)
        throw new Error("WebTransport credential is too long");
      const preamble = new Uint8Array(2 + credential.length);
      new DataView(preamble.buffer).setUint16(0, credential.length, true);
      preamble.set(credential, 2);
      await writer.write(preamble);
      const verdict = await readExact(authReader, 1);
      if (!verdict.data)
        throw new Error("WebTransport closed during authentication");
      if (verdict.data[0] === AUTH_BUSY) {
        this.authRejected = false;
        throw new Error("Edge authentication is temporarily busy");
      }
      if (verdict.data[0] !== 1) {
        this.authRejected = true;
        this.lastError = "Authentication failed";
        this.setStatus("error");
        session.close();
        return;
      }
      if (!this.isCurrent(session)) return session.close();

      authReader.releaseLock();
      this.streamWriter = writer;
      this.datagramWriter = session.datagrams.writable.getWriter();
      this.authRejected = false;
      this.lastError = null;
      this.retryDelay = this.initialRetryDelay;
      this.setStatus("connected");
      void this.readStream(stream.readable, session, verdict.remainder);
      void this.readDatagrams(session.datagrams.readable, session);
      void session.closed.then(
        (info) => this.closed(session, info.reason || null),
        (error: unknown) =>
          this.closed(
            session,
            error instanceof Error ? error.message : "WebTransport closed",
          ),
      );
    } catch (error) {
      if (this.disposed || this.suspended) return;
      this.lastError = error instanceof Error ? error.message : String(error);
      this.cleanup();
      this.setStatus("error");
      if (!this.authRejected) this.scheduleReconnect();
    }
  }

  private async readStream(
    readable: ReadableStream<Uint8Array>,
    session: WebTransport,
    initial: Uint8Array,
  ): Promise<void> {
    try {
      if (initial.length > 0) this.emit(this.messageListeners, initial);
      const reader = readable.getReader();
      while (this.isCurrent(session)) {
        const { value, done } = await reader.read();
        if (done) throw new Error("WebTransport YAS stream closed");
        if (value?.length) this.emit(this.messageListeners, value);
      }
    } catch (error) {
      if (!this.isCurrent(session)) return;
      this.lastError = error instanceof Error ? error.message : String(error);
      this.cleanup();
      this.setStatus("disconnected");
      this.scheduleReconnect();
    }
  }

  private async readDatagrams(
    readable: ReadableStream<Uint8Array>,
    session: WebTransport,
  ): Promise<void> {
    try {
      const reader = readable.getReader();
      while (this.isCurrent(session)) {
        const { value, done } = await reader.read();
        if (done) return;
        if (value?.length && value.length <= YAS_TRANSPORT_DATAGRAM_HARD_MAX)
          this.emit(this.datagramListeners, value);
      }
    } catch {
      // The optional path may disappear without invalidating the reliable
      // stream. A later physical reconnect renegotiates its HELLO limit.
    } finally {
      if (this.isCurrent(session)) this.datagramWriter = null;
    }
  }

  private emit(
    listeners: ReadonlySet<(data: YasTransportMessage) => void>,
    value: Uint8Array,
  ): void {
    const owned = copyBytes(value);
    for (const listener of listeners) listener(owned);
  }

  private closed(session: WebTransport, reason: string | null): void {
    if (!this.isCurrent(session)) return;
    this.lastError = reason;
    this.cleanup();
    this.setStatus("disconnected");
    this.scheduleReconnect();
  }

  private isCurrent(session: WebTransport): boolean {
    return !this.disposed && !this.suspended && this.session === session;
  }

  private cleanup(): void {
    this.streamWriter = null;
    this.datagramWriter = null;
    const session = this.session;
    this.session = null;
    try {
      session?.close();
    } catch {
      // Already closed.
    }
  }

  private setStatus(status: ConnectionStatus): void {
    if (this._status === status) return;
    this._status = status;
    for (const listener of this.statusListeners) listener(status);
  }

  private scheduleReconnect(): void {
    if (
      !this.reconnectEnabled ||
      this.disposed ||
      this.suspended ||
      this.authRejected ||
      this.reconnectTimer
    )
      return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, this.retryDelay);
    this.retryDelay = Math.min(
      this.maximumRetryDelay,
      this.retryDelay * this.retryBackoff,
    );
  }

  private clearReconnect(): void {
    if (!this.reconnectTimer) return;
    clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }
}

async function withTimeout<T>(
  promise: Promise<T>,
  milliseconds: number,
): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timeout = setTimeout(
          () => reject(new Error("WebTransport connect timeout")),
          milliseconds,
        );
      }),
    ]);
  } finally {
    if (timeout) clearTimeout(timeout);
  }
}

async function readExact(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  length: number,
): Promise<{ data: Uint8Array | null; remainder: Uint8Array }> {
  const data = new Uint8Array(length);
  let offset = 0;
  let pending: Uint8Array<ArrayBufferLike> = new Uint8Array(0);
  while (offset < length) {
    if (pending.length === 0) {
      const next = await reader.read();
      if (next.done || !next.value)
        return { data: null, remainder: new Uint8Array(0) };
      pending = next.value;
    }
    const take = Math.min(length - offset, pending.length);
    data.set(pending.subarray(0, take), offset);
    offset += take;
    pending = pending.subarray(take);
  }
  return { data, remainder: copyBytes(pending) };
}

function certificateHash(value: string): Uint8Array {
  if (!/^[0-9a-fA-F]{64}$/.test(value))
    throw new TypeError("WebTransport certificate hash must be 64 hex digits");
  const bytes = new Uint8Array(32);
  for (let index = 0; index < bytes.length; index++)
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  return bytes;
}

function copyBytes(value: Uint8Array): Uint8Array<ArrayBuffer> {
  const copy = new Uint8Array(value.length);
  copy.set(value);
  return copy;
}
