/** Demand-driven preview socket proxy over an App-owned native YAS Net flow. */

import type { NetOpenOptions, NetStream } from "@yas-run/core";
import {
  PREVIEW_NET_ACCEPTED,
  PREVIEW_NET_CLOSE,
  PREVIEW_NET_DATA,
  PREVIEW_NET_END,
  PREVIEW_NET_ERROR,
  PREVIEW_NET_OPEN,
  PREVIEW_NET_OPENED,
  PREVIEW_NET_READ,
  PREVIEW_NET_SHUTDOWN_WRITE,
  PREVIEW_NET_WRITE,
  PREVIEW_NET_WRITE_OK,
  type PreviewNetAppMessage,
  type PreviewNetOpenMessage,
  type PreviewNetWorkerMessage,
} from "../previewNetProtocol";

const OPEN_TIMEOUT_MS = 10_000;

export type PreviewNetPortRequest = (
  request: PreviewNetOpenMessage,
) => Promise<MessagePort>;

/**
 * Opens preview sockets through a controlled top-level App. The App chooses
 * the already-authenticated home or nested Relay YAS session; this worker owns
 * neither credentials nor an edge connection.
 */
export class PreviewNetBroker {
  private nextStreamId = 1;
  private readonly streams = new Set<BrokeredNetStream>();

  constructor(private readonly requestPort: PreviewNetPortRequest) {}

  async open(
    dest: string,
    host: string,
    port: number,
    options: NetOpenOptions = {},
  ): Promise<NetStream> {
    const channel = await this.requestPort({
      type: PREVIEW_NET_OPEN,
      dest,
      host,
      port,
      options,
    });
    const stream = new BrokeredNetStream(this.allocateStreamId(), channel, () =>
      this.streams.delete(stream),
    );
    this.streams.add(stream);
    return stream;
  }

  close(): void {
    for (const stream of [...this.streams]) stream.close();
    this.streams.clear();
  }

  private allocateStreamId(): number {
    const value = this.nextStreamId;
    this.nextStreamId = value === 0xffff ? 1 : value + 1;
    return value;
  }
}

class BrokeredNetStream implements NetStream {
  readonly opened: Promise<string>;
  private resolveOpened!: (alpn: string) => void;
  private rejectOpened!: (error: Error) => void;
  private openedSettled = false;
  private nextWriteId = 1;
  private writeTail: Promise<void> = Promise.resolve();
  private readonly writes = new Map<
    number,
    { resolve: () => void; reject: (error: Error) => void }
  >();
  private readResponse:
    | {
        resolve: (value: Uint8Array | null) => void;
        reject: (error: Error) => void;
      }
    | undefined;
  private ended = false;
  private readonly timer: ReturnType<typeof setTimeout>;

  constructor(
    readonly streamId: number,
    private readonly port: MessagePort,
    private readonly onClose: () => void,
  ) {
    this.opened = new Promise<string>((resolve, reject) => {
      this.resolveOpened = resolve;
      this.rejectOpened = reject;
    });
    // A port can be accepted before the App finishes native NET OPEN. Bound
    // that wait so a stale controlled tab cannot hang a fetch forever.
    this.timer = setTimeout(
      () => this.fail(new Error("timed out opening native preview Net flow")),
      OPEN_TIMEOUT_MS,
    );
    port.onmessage = (event) => this.onMessage(event.data);
    port.onmessageerror = () =>
      this.fail(new Error("invalid preview Net broker message"));
    port.start();
  }

  write(data: Uint8Array): Promise<void> {
    const copy = new Uint8Array(data);
    const current = this.writeTail.then(async () => {
      await this.opened;
      if (this.ended) throw new Error("preview Net flow is closed");
      const id = this.allocateWriteId();
      const result = new Promise<void>((resolve, reject) => {
        this.writes.set(id, { resolve, reject });
      });
      this.port.postMessage(
        {
          type: PREVIEW_NET_WRITE,
          id,
          data: copy.buffer,
        } satisfies PreviewNetWorkerMessage,
        [copy.buffer],
      );
      await result;
    });
    this.writeTail = current.catch(() => {});
    return current;
  }

  shutdownWrite(): void {
    if (this.ended) return;
    this.port.postMessage({
      type: PREVIEW_NET_SHUTDOWN_WRITE,
    } satisfies PreviewNetWorkerMessage);
  }

  close(): void {
    if (this.ended) return;
    try {
      this.port.postMessage({
        type: PREVIEW_NET_CLOSE,
      } satisfies PreviewNetWorkerMessage);
    } catch {
      // The App side already disappeared.
    }
    this.fail(new Error("preview Net flow was closed"));
  }

  async *read(): AsyncGenerator<Uint8Array, void, void> {
    await this.opened;
    try {
      for (;;) {
        const value = await this.readOne();
        if (value === null) return;
        yield value;
      }
    } finally {
      if (!this.ended) this.close();
    }
  }

  private readOne(): Promise<Uint8Array | null> {
    if (this.ended) return Promise.resolve(null);
    if (this.readResponse)
      return Promise.reject(new Error("concurrent preview Net reads"));
    const result = new Promise<Uint8Array | null>((resolve, reject) => {
      this.readResponse = { resolve, reject };
    });
    this.port.postMessage({
      type: PREVIEW_NET_READ,
    } satisfies PreviewNetWorkerMessage);
    return result;
  }

  private onMessage(raw: unknown): void {
    if (this.ended || !raw || typeof raw !== "object") return;
    const value = raw as PreviewNetAppMessage;
    if (value.type === PREVIEW_NET_ACCEPTED) return;
    if (value.type === PREVIEW_NET_OPENED) {
      if (this.openedSettled || typeof value.alpn !== "string") {
        this.fail(new Error("duplicate or malformed preview Net OPEN result"));
        return;
      }
      this.openedSettled = true;
      clearTimeout(this.timer);
      this.resolveOpened(value.alpn);
      return;
    }
    if (value.type === PREVIEW_NET_WRITE_OK) {
      const pending = this.writes.get(value.id);
      if (!pending) {
        this.fail(new Error("unknown preview Net write acknowledgement"));
        return;
      }
      this.writes.delete(value.id);
      pending.resolve();
      return;
    }
    if (value.type === PREVIEW_NET_DATA) {
      const bytes = messageBytes(value.data);
      if (!this.readResponse || !bytes) {
        this.fail(new Error("unsolicited or malformed preview Net data"));
        return;
      }
      const pending = this.readResponse;
      this.readResponse = undefined;
      pending.resolve(bytes);
      return;
    }
    if (value.type === PREVIEW_NET_END) {
      const pending = this.readResponse;
      this.readResponse = undefined;
      pending?.resolve(null);
      const error = new Error("preview Net flow ended");
      for (const write of this.writes.values()) write.reject(error);
      this.writes.clear();
      this.finish();
      return;
    }
    if (value.type === PREVIEW_NET_ERROR) {
      const error = new Error(value.detail || "native preview Net flow failed");
      if (value.id !== undefined) {
        const pending = this.writes.get(value.id);
        if (!pending) {
          this.fail(new Error("unknown preview Net write failure"));
          return;
        }
        this.writes.delete(value.id);
        pending.reject(error);
      }
      this.fail(error);
    }
  }

  private allocateWriteId(): number {
    const value = this.nextWriteId;
    this.nextWriteId = value === 0xffff_ffff ? 1 : value + 1;
    return value;
  }

  private fail(error: Error): void {
    if (this.ended) return;
    if (!this.openedSettled) {
      this.openedSettled = true;
      clearTimeout(this.timer);
      this.rejectOpened(error);
    }
    for (const pending of this.writes.values()) pending.reject(error);
    this.writes.clear();
    this.readResponse?.reject(error);
    this.readResponse = undefined;
    this.finish();
  }

  private finish(): void {
    if (this.ended) return;
    this.ended = true;
    clearTimeout(this.timer);
    this.port.close();
    this.onClose();
  }
}

function messageBytes(value: unknown): Uint8Array | null {
  if (ArrayBuffer.isView(value))
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  if (Object.prototype.toString.call(value) === "[object ArrayBuffer]")
    return new Uint8Array(value as ArrayBuffer);
  return null;
}
