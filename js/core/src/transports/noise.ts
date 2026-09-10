import type {
  ConnectionStatus,
  YasTransport,
  YasTransportEventMap,
  YasTransportMessage,
  YasTransportOptions,
} from "../types";
import { YAS_EVENT_HEADER_BYTES } from "../yas/generated";
import {
  type Bytes,
  NoiseCipher,
  NoiseDatagrams,
  NoiseInitiator,
  PROLOGUE,
  MAX_PLAINTEXT,
  DATAGRAM_OVERHEAD,
  concat,
  decodeKey,
  equal,
  importIdentity,
  publicKey,
} from "./uplink-crypto";

const QUEUE_LIMIT = 4 * 1024 * 1024;
const DATAGRAM_IN_FLIGHT = 64;
export abstract class UplinkEvents implements YasTransport {
  readonly yasFraming = "stream" as const;
  protected _status: ConnectionStatus = "disconnected";
  authRejected = false;
  lastError: string | null = null;
  get status(): ConnectionStatus {
    return this._status;
  }
  private readonly listeners: {
    [K in keyof YasTransportEventMap]: Set<
      (data: YasTransportEventMap[K]) => void
    >;
  } = {
    message: new Set(),
    datagram: new Set(),
    statuschange: new Set(),
  };
  addEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    this.listeners[type].add(listener);
  }
  removeEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    this.listeners[type].delete(listener);
  }
  protected emit<K extends keyof YasTransportEventMap>(
    type: K,
    value: YasTransportEventMap[K],
  ): void {
    for (const listener of this.listeners[type]) listener(value);
  }
  protected setStatus(status: ConnectionStatus): void {
    if (this._status !== status) {
      this._status = status;
      this.emit("statuschange", status);
    }
  }
  abstract connect(): void;
  abstract send(data: Uint8Array): void;
  abstract close(): void;
}

class TruncatedStream extends Error {
  constructor() {
    super("uplink stream truncated");
  }
}

class ByteQueue {
  private chunks: Bytes[] = [];
  private offset = 0;
  private length = 0;
  private wake: (() => void) | null = null;
  private ended = false;
  push(data: YasTransportMessage): void {
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    if (this.ended) return;
    if (bytes.length > 65536 || this.length + bytes.length > QUEUE_LIMIT)
      throw new Error("uplink receive queue exceeded");
    if (bytes.length === 0) return;
    this.chunks.push(bytes.slice());
    this.length += bytes.length;
    this.wake?.();
    this.wake = null;
  }
  async read(size: number): Promise<Bytes> {
    while (this.length < size) {
      if (this.ended) throw new TruncatedStream();
      await new Promise<void>((resolve) => {
        this.wake = resolve;
      });
    }
    const bytes = new Uint8Array(size);
    let written = 0;
    while (written < size) {
      const head = this.chunks[0]!;
      const count = Math.min(size - written, head.length - this.offset);
      bytes.set(head.subarray(this.offset, this.offset + count), written);
      written += count;
      this.offset += count;
      this.length -= count;
      if (this.offset === head.length) {
        this.chunks.shift();
        this.offset = 0;
      }
    }
    return bytes;
  }
  end(): void {
    this.ended = true;
    this.wake?.();
    this.wake = null;
  }
  close(): void {
    this.chunks = [];
    this.length = this.offset = 0;
    this.end();
  }
}
export function frameNoise(bytes: Bytes): Bytes {
  const prefix = new Uint8Array(2);
  new DataView(prefix.buffer).setUint16(0, bytes.length);
  return concat(prefix, bytes);
}

export interface YasNoiseTransportOptions extends YasTransportOptions {
  /** The carrier's datagrams must include the relay's 16-byte routing token. */
  datagrams?: boolean;
}
/** End-to-end Noise over an opaque carrier. Its "connected" event means that
 * relay routing is ready; only this wrapper's "connected" event grants YAS access.
 * Carrier reconnects always create fresh handshakes, counters, and datagram keys. */
export class YasNoiseTransport extends UplinkEvents {
  private readonly seed: Bytes;
  private readonly server: Bytes;
  private generation = 0;
  private queue: ByteQueue | null = null;
  private sendCipher: NoiseCipher | null = null;
  private datagrams: NoiseDatagrams | null = null;
  private route: Bytes | null = null;
  private maximum = 0;
  private writeQueue = Promise.resolve();
  private queuedBytes = 0;
  private datagramSends = 0;
  private datagramReads = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private finishTimer: ReturnType<typeof setTimeout> | null = null;
  private delay: number;
  private disposed = false;
  private suspended = false;
  private closing = false;
  private carrierEnded = false;
  private stoppingCarrier = false;

  constructor(
    private readonly carrier: YasTransport,
    identity: string,
    server: string,
    private readonly options: YasNoiseTransportOptions = {},
  ) {
    super();
    this.delay = options.reconnectDelay ?? 500;
    this.seed = decodeKey(identity);
    this.server = publicKey(server);
    carrier.addEventListener("message", this.onMessage);
    carrier.addEventListener("datagram", this.onDatagram);
    carrier.addEventListener("statuschange", this.onStatus);
  }
  get maxDatagramSize(): number {
    return this.maximum;
  }
  get bufferedAmount(): number {
    return this.queuedBytes + (this.carrier.bufferedAmount ?? 0);
  }
  connect(): void {
    if (!this.disposed && !this.closing) {
      this.suspended = false;
      this.clearRetry();
      if (this.queue) return;
      this.carrier.connect();
      if (this.carrier.status === "connected" && !this.queue)
        this.onStatus("connected");
    }
  }
  reconnect(): void {
    if (this.disposed) return;
    this.reset();
    this.suspended = false;
    this.clearRetry();
    this.delay = this.options.reconnectDelay ?? 500;
    this.authRejected = false;
    this.lastError = null;
    if (this.carrier.reconnect) this.carrier.reconnect();
    else this.stopCarrier();
    this.connect();
  }
  suspend(): void {
    if (this.disposed) return;
    this.suspended = true;
    this.clearRetry();
    this.reset();
    this.stopCarrier();
    this.setStatus("disconnected");
  }
  send(data: Uint8Array): void {
    if (
      this.status !== "connected" ||
      this.closing ||
      this.carrierEnded ||
      this.disposed ||
      data.length === 0
    )
      return;
    if (this.bufferedAmount + data.length > QUEUE_LIMIT)
      return this.fail("uplink send queue exceeded", false);
    const bytes = data.slice();
    const generation = this.generation;
    this.queuedBytes += bytes.length;
    this.writeQueue = this.writeQueue
      .then(async () => {
        try {
          if (generation === this.generation)
            await this.write(bytes, generation);
        } finally {
          if (generation === this.generation) this.queuedBytes -= bytes.length;
          bytes.fill(0);
        }
      })
      .catch(() => {
        if (generation === this.generation)
          this.fail("uplink encrypted write failed", false);
      });
  }
  sendDatagram(data: Uint8Array): void {
    const crypto = this.datagrams,
      route = this.route,
      generation = this.generation;
    if (
      !crypto ||
      !route ||
      data.length > this.maximum ||
      this.datagramSends >= DATAGRAM_IN_FLIGHT ||
      this.closing ||
      this.carrierEnded
    )
      return;
    this.datagramSends++;
    void crypto
      .seal(data.slice())
      .then((packet) => {
        if (
          packet &&
          generation === this.generation &&
          !this.closing &&
          !this.carrierEnded
        )
          this.carrier.sendDatagram?.(concat(route, packet));
      })
      .catch(() => {})
      .finally(() => {
        if (generation === this.generation) this.datagramSends--;
      });
  }
  close(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.seed.fill(0);
    this.clearRetry();
    if (!this.closing) this.finish();
    this.setStatus("closed");
  }
  /** Finish this session without disposing the reusable transport or identity. */
  private finish(): void {
    this.closing = true;
    this.datagrams?.close();
    this.maximum = 0;
    const cipher = this.sendCipher,
      generation = this.generation;
    const finish = async () => {
      try {
        if (cipher && generation === this.generation && !this.carrierEnded) {
          const record = frameNoise(await cipher.encrypt(new Uint8Array([1])));
          if (generation === this.generation && !this.carrierEnded)
            this.carrier.send(record);
        }
      } catch {
        /* An aborted carrier may not accept FIN. */
      }
    };
    let cleaned = false;
    const cleanup = () => {
      if (cleaned || generation !== this.generation) return;
      cleaned = true;
      this.reset();
      if (this.disposed) {
        this.carrier.close();
        this.carrier.removeEventListener("message", this.onMessage);
        this.carrier.removeEventListener("datagram", this.onDatagram);
        this.carrier.removeEventListener("statuschange", this.onStatus);
      } else {
        this.stopCarrier();
        this.lastError = null;
        this.authRejected = false;
        this.setStatus("disconnected");
        this.scheduleRetry();
      }
    };
    this.finishTimer = setTimeout(cleanup, 1000);
    void this.writeQueue.then(finish).finally(cleanup);
  }
  private stopCarrier(): void {
    this.stoppingCarrier = true;
    try {
      if (this.carrier.suspend) this.carrier.suspend();
      else this.carrier.close();
    } finally {
      this.stoppingCarrier = false;
    }
  }
  private clearRetry(): void {
    if (this.retry !== null) clearTimeout(this.retry);
    this.retry = null;
  }
  private scheduleRetry(): void {
    if (
      this.disposed ||
      this.suspended ||
      this.closing ||
      this.queue ||
      this.authRejected ||
      this.retry !== null ||
      this.options.reconnect === false
    )
      return;
    this.retry = setTimeout(() => {
      this.retry = null;
      this.connect();
    }, this.delay);
    this.delay = Math.min(
      this.options.maxReconnectDelay ?? 10000,
      this.delay * (this.options.reconnectBackoff ?? 1.5),
    );
  }
  private reset(): void {
    this.generation++;
    this.queue?.close();
    this.queue = null;
    this.sendCipher = null;
    this.datagrams?.close();
    this.datagrams = null;
    this.route = null;
    this.maximum = 0;
    this.queuedBytes = this.datagramReads = this.datagramSends = 0;
    this.writeQueue = Promise.resolve();
    this.closing = false;
    this.carrierEnded = false;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    if (this.finishTimer !== null) clearTimeout(this.finishTimer);
    this.finishTimer = null;
  }
  private fail(
    message: string,
    authentication = this.status === "authenticating",
  ): void {
    if (this.disposed) return;
    this.lastError = message;
    this.authRejected = authentication;
    this.reset();
    this.stopCarrier();
    this.setStatus("error");
    this.scheduleRetry();
  }
  private onMessage = (bytes: YasTransportMessage): void => {
    try {
      this.queue?.push(bytes);
    } catch {
      this.fail("uplink receive queue exceeded", false);
    }
  };
  private onDatagram = (data: YasTransportMessage): void => {
    const crypto = this.datagrams,
      route = this.route,
      generation = this.generation;
    const packet = data instanceof Uint8Array ? data : new Uint8Array(data);
    if (
      !crypto ||
      !route ||
      this.closing ||
      this.carrierEnded ||
      this.datagramReads >= DATAGRAM_IN_FLIGHT ||
      packet.length < 40 ||
      packet.length > this.maximum + 40 ||
      !equal(route, packet.subarray(0, 16))
    )
      return;
    this.datagramReads++;
    void crypto
      .open(packet.slice(16))
      .then((plaintext) => {
        if (
          plaintext &&
          generation === this.generation &&
          !this.closing &&
          !this.carrierEnded
        )
          this.emit("datagram", plaintext);
      })
      .finally(() => {
        if (generation === this.generation) this.datagramReads--;
      });
  };
  private onStatus = (status: ConnectionStatus): void => {
    if (this.disposed || this.suspended || this.stoppingCarrier) return;
    if (this.closing) {
      if (status !== "connected") this.carrierEnded = true;
      return;
    }
    if (status === "connected") {
      if (this.queue) return;
      this.clearRetry();
      const queue = new ByteQueue();
      this.queue = queue;
      const generation = ++this.generation;
      this.setStatus("authenticating");
      this.timer = setTimeout(() => {
        if (generation === this.generation)
          this.fail("uplink authentication timed out", false);
      }, this.options.connectTimeoutMs ?? 10000);
      void this.run(queue, generation).catch((error: unknown) => {
        if (generation === this.generation)
          this.fail(
            error instanceof TruncatedStream
              ? error.message
              : this.status === "authenticating"
                ? "uplink end-to-end authentication failed (requires WebCrypto X25519)"
                : "uplink stream integrity failure",
            this.status === "authenticating" &&
              !(error instanceof TruncatedStream),
          );
      });
    } else {
      if (this.queue) {
        // Stop carrier retries while authenticating all bytes already received.
        // EOF only rejects a read once that queue has been exhausted.
        this.carrierEnded = true;
        this.queue.end();
        this.stopCarrier();
        return;
      }
      if (this.carrier.authRejected) {
        this.authRejected = true;
        this.lastError = "uplink routing token rejected";
      }
      this.setStatus(status === "authenticating" ? "connecting" : status);
      if (
        status === "disconnected" ||
        status === "closed" ||
        status === "error"
      )
        this.scheduleRetry();
    }
  };
  private async record(
    queue: ByteQueue,
    maximum: number,
    minimum: number,
  ): Promise<Bytes> {
    const header = await queue.read(2);
    const length = new DataView(header.buffer).getUint16(0);
    if (length < minimum || length > maximum)
      throw new Error("invalid Noise record length");
    return queue.read(length);
  }
  private async write(bytes: Bytes, generation: number): Promise<void> {
    const cipher = this.sendCipher;
    if (!cipher) throw new Error("Noise handshake incomplete");
    for (let offset = 0; offset < bytes.length; offset += MAX_PLAINTEXT) {
      if (generation !== this.generation || this.carrierEnded) return;
      const record = await cipher.encrypt(
        concat(
          new Uint8Array([0]),
          bytes.subarray(offset, offset + MAX_PLAINTEXT),
        ),
      );
      if (generation !== this.generation || this.carrierEnded) return;
      this.carrier.send(frameNoise(record));
    }
  }
  private async run(queue: ByteQueue, generation: number): Promise<void> {
    const identity = await importIdentity(this.seed);
    const handshake = await NoiseInitiator.create(identity, this.server);
    const first = await handshake.first();
    if (generation !== this.generation) return;
    if (this.carrierEnded) throw new TruncatedStream();
    this.carrier.send(frameNoise(first));
    const { send, receive, payload } = await handshake.finish(
      await this.record(queue, 48, 48),
    );
    if (payload.length !== 0)
      throw new Error("invalid Noise handshake payload");
    if (generation !== this.generation) return;
    this.sendCipher = send;
    await this.write(PROLOGUE, generation);
    const ready = await receive.decrypt(
      await this.record(queue, MAX_PLAINTEXT + 17, 17),
    );
    if (
      ready.length !== 1 + PROLOGUE.length + 64 ||
      ready[0] !== 0 ||
      !equal(ready.subarray(1, 1 + PROLOGUE.length), PROLOGUE)
    )
      throw new Error("invalid Noise confirmation");
    if (generation !== this.generation) {
      ready.fill(0);
      return;
    }
    const maximum = Math.min(
      (this.carrier.maxDatagramSize ?? 0) - 16 - DATAGRAM_OVERHEAD,
      65536 - DATAGRAM_OVERHEAD,
    );
    if (
      this.options.datagrams !== false &&
      this.carrier.sendDatagram &&
      Number.isSafeInteger(maximum) &&
      maximum >= YAS_EVENT_HEADER_BYTES
    ) {
      this.route = crypto.getRandomValues(new Uint8Array(16));
      this.datagrams = new NoiseDatagrams(
        ready.slice(1 + PROLOGUE.length),
        this.route,
      );
      this.maximum = maximum;
      const offer = new Uint8Array(32);
      offer.set([0x59, 0x41, 0x53, 0x43, 0x4d, 0x50, 1, 1]);
      offer.set(this.route, 8);
      new DataView(offer.buffer).setUint32(24, maximum, true);
      await this.write(offer, generation);
    }
    ready.fill(0);
    if (generation !== this.generation) return;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    this.authRejected = false;
    this.lastError = null;
    this.delay = this.options.reconnectDelay ?? 500;
    this.setStatus("connected");
    while (generation === this.generation && !this.closing) {
      const plaintext = await receive.decrypt(
        await this.record(queue, MAX_PLAINTEXT + 17, 17),
      );
      if (generation !== this.generation || this.closing) return;
      if (plaintext[0] === 1 && plaintext.length === 1) {
        this.finish();
        return;
      }
      if (plaintext[0] !== 0 || plaintext.length < 2)
        throw new Error("invalid Noise payload");
      this.emit("message", plaintext.subarray(1));
    }
  }
}
