import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { YasWebTransportTransport } from "../transports/webtransport";

class MockWebTransport {
  static instances: MockWebTransport[] = [];
  static nextAuthVerdict = 1;

  readonly ready = Promise.resolve();
  readonly streamWrites: Uint8Array[] = [];
  readonly datagramWrites: Uint8Array[] = [];
  readonly streamReadable: ReadableStream<Uint8Array>;
  readonly streamWritable: WritableStream<Uint8Array>;
  readonly datagramReadable: ReadableStream<Uint8Array>;
  readonly datagramWritable: WritableStream<Uint8Array>;
  readonly datagrams: WebTransportDatagramDuplexStream;
  readonly closed: Promise<{ closeCode: number; reason: string }>;
  streamWriteError: Error | null = null;
  datagramWriteError: Error | null = null;
  streamController!: ReadableStreamDefaultController<Uint8Array>;
  datagramController!: ReadableStreamDefaultController<Uint8Array>;
  private resolveClosed!: (info: { closeCode: number; reason: string }) => void;

  constructor(
    readonly url: string,
    readonly options: WebTransportOptions,
  ) {
    MockWebTransport.instances.push(this);
    this.closed = new Promise((resolve) => {
      this.resolveClosed = resolve;
    });
    this.streamReadable = new ReadableStream({
      start: (controller) => {
        this.streamController = controller;
        controller.enqueue(
          new Uint8Array([MockWebTransport.nextAuthVerdict, 9, 8]),
        );
      },
    });
    this.streamWritable = new WritableStream({
      write: (value) => {
        if (this.streamWriteError) return Promise.reject(this.streamWriteError);
        this.streamWrites.push(new Uint8Array(value));
      },
    });
    this.datagramReadable = new ReadableStream({
      start: (controller) => {
        this.datagramController = controller;
      },
    });
    this.datagramWritable = new WritableStream({
      write: (value) => {
        if (this.datagramWriteError)
          return Promise.reject(this.datagramWriteError);
        this.datagramWrites.push(new Uint8Array(value));
      },
    });
    this.datagrams = {
      readable: this.datagramReadable,
      writable: this.datagramWritable,
      maxDatagramSize: 70_000,
      incomingHighWaterMark: 1,
      incomingMaxAge: null,
      outgoingHighWaterMark: 1,
      outgoingMaxAge: null,
    };
  }

  createBidirectionalStream(): Promise<WebTransportBidirectionalStream> {
    return Promise.resolve({
      readable: this.streamReadable,
      writable: this.streamWritable,
    });
  }

  close(): void {
    this.resolveClosed({ closeCode: 0, reason: "closed" });
  }
}

beforeEach(() => {
  MockWebTransport.instances = [];
  MockWebTransport.nextAuthVerdict = 1;
  vi.stubGlobal("WebTransport", MockWebTransport);
});

afterEach(() => vi.unstubAllGlobals());

describe("YasWebTransportTransport", () => {
  it("carries raw YAS stream bytes and complete native datagrams", async () => {
    const transport = new YasWebTransportTransport(
      "https://example.test/yas",
      "ok",
      { reconnect: false },
    );
    const messages: Uint8Array[] = [];
    const datagrams: Uint8Array[] = [];
    transport.addEventListener("message", (value) =>
      messages.push(new Uint8Array(value)),
    );
    transport.addEventListener("datagram", (value) =>
      datagrams.push(new Uint8Array(value)),
    );

    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    const session = MockWebTransport.instances[0]!;
    expect(session.url).toBe("https://example.test/yas");
    expect(session.streamWrites[0]).toEqual(new Uint8Array([2, 0, 0x6f, 0x6b]));
    expect(messages).toEqual([new Uint8Array([9, 8])]);
    expect(transport.yasFraming).toBe("stream");
    expect(transport.maxDatagramSize).toBe(65_536);

    transport.send(new Uint8Array([5, 4, 3]));
    transport.sendDatagram(new Uint8Array([7, 6]));
    await vi.waitFor(() => expect(session.streamWrites).toHaveLength(2));
    expect(session.streamWrites[1]).toEqual(new Uint8Array([5, 4, 3]));
    expect(session.datagramWrites).toEqual([new Uint8Array([7, 6])]);

    session.streamController.enqueue(new Uint8Array([2, 1]));
    session.datagramController.enqueue(new Uint8Array([4, 5]));
    await vi.waitFor(() => expect(messages).toHaveLength(2));
    await vi.waitFor(() => expect(datagrams).toHaveLength(1));
    expect(messages[1]).toEqual(new Uint8Array([2, 1]));
    expect(datagrams[0]).toEqual(new Uint8Array([4, 5]));

    transport.sendDatagram(new Uint8Array(65_537));
    session.datagramController.enqueue(new Uint8Array(65_537));
    await Promise.resolve();
    expect(session.datagramWrites).toHaveLength(1);
    expect(datagrams).toHaveLength(1);

    session.datagramController.error(new Error("optional path lost"));
    await vi.waitFor(() => expect(transport.maxDatagramSize).toBe(0));
    expect(transport.status).toBe("connected");
    transport.close();
  });

  it("falls back after clean optional-datagram EOF", async () => {
    const transport = new YasWebTransportTransport(
      "https://example.test/yas",
      "ok",
      { reconnect: false },
    );
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    const session = MockWebTransport.instances[0]!;
    expect(transport.maxDatagramSize).toBe(65_536);

    session.datagramController.close();

    await vi.waitFor(() => expect(transport.maxDatagramSize).toBe(0));
    expect(transport.status).toBe("connected");
    transport.close();
  });

  it("retries a busy authentication without discarding the credential", async () => {
    MockWebTransport.nextAuthVerdict = 2;
    const transport = new YasWebTransportTransport(
      "https://example.test/yas",
      "ok",
      { reconnect: false },
    );
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("error"));
    expect(transport.authRejected).toBe(false);
    expect(transport.lastError).toMatch(/temporarily busy/);
    transport.close();
  });

  it("falls back after an optional-datagram write rejection", async () => {
    const transport = new YasWebTransportTransport(
      "https://example.test/yas",
      "ok",
      { reconnect: false },
    );
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    const session = MockWebTransport.instances[0]!;
    session.datagramWriteError = new Error("datagram path lost");

    transport.sendDatagram(new Uint8Array([1]));

    await vi.waitFor(() => expect(transport.maxDatagramSize).toBe(0));
    expect(transport.status).toBe("connected");
    transport.close();
  });

  it("disconnects after an authoritative stream write rejection", async () => {
    const transport = new YasWebTransportTransport(
      "https://example.test/yas",
      "ok",
      { reconnect: false },
    );
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    const session = MockWebTransport.instances[0]!;
    session.streamWriteError = new Error("reliable path lost");

    transport.send(new Uint8Array([1]));

    await vi.waitFor(() => expect(transport.status).toBe("disconnected"));
    expect(transport.lastError).toBe("reliable path lost");
    expect(transport.maxDatagramSize).toBe(0);
    transport.close();
  });
});
