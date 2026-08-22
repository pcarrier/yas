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

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("YasWebTransportTransport", () => {
  describe("certificate refresh", () => {
    beforeEach(() => vi.useFakeTimers());

    const firstHash = "ab".repeat(32);
    const latestHash = "cd".repeat(32);
    const pinnedHash = (session: MockWebTransport) =>
      new Uint8Array(
        session.options.serverCertificateHashes![0]!.value as ArrayBuffer,
      );

    it("keeps supporting a static certificate pin", async () => {
      const transport = new YasWebTransportTransport(
        "https://example.test/yas",
        "ok",
        { serverCertificateHash: firstHash },
      );
      transport.connect();
      await vi.advanceTimersByTimeAsync(0);
      expect(transport.status).toBe("connected");
      expect(pinnedHash(MockWebTransport.instances[0]!)).toEqual(
        new Uint8Array(32).fill(0xab),
      );
      transport.close();
    });

    it.each(["automatic", "manual", "resume"])(
      "fetches the current pin on %s reconnect",
      async (mode) => {
        const resolveHash = vi
          .fn()
          .mockResolvedValueOnce(firstHash)
          .mockResolvedValueOnce(latestHash);
        const transport = new YasWebTransportTransport(
          "https://example.test/yas",
          "ok",
          { serverCertificateHash: resolveHash },
        );
        transport.connect();
        await vi.advanceTimersByTimeAsync(0);
        expect(transport.status).toBe("connected");
        expect(pinnedHash(MockWebTransport.instances[0]!)).toEqual(
          new Uint8Array(32).fill(0xab),
        );

        if (mode === "automatic") MockWebTransport.instances[0]!.close();
        else if (mode === "manual") transport.reconnect();
        else {
          transport.suspend();
          transport.connect();
        }
        await vi.advanceTimersByTimeAsync(500);

        expect(resolveHash).toHaveBeenCalledTimes(2);
        expect(MockWebTransport.instances).toHaveLength(2);
        expect(transport.status).toBe("connected");
        expect(pinnedHash(MockWebTransport.instances[1]!)).toEqual(
          new Uint8Array(32).fill(0xcd),
        );
        transport.close();
      },
    );

    it("retries failed discovery without dialing with the old pin", async () => {
      const resolveHash = vi
        .fn()
        .mockResolvedValueOnce(firstHash)
        .mockRejectedValueOnce(new Error("discovery unavailable"))
        .mockResolvedValueOnce(latestHash);
      const transport = new YasWebTransportTransport(
        "https://example.test/yas",
        "ok",
        { serverCertificateHash: resolveHash },
      );
      transport.connect();
      await vi.advanceTimersByTimeAsync(0);
      transport.reconnect();
      await vi.advanceTimersByTimeAsync(0);
      expect(transport.status).toBe("error");
      expect(transport.lastError).toBe("discovery unavailable");
      expect(transport.authRejected).toBe(false);
      expect(MockWebTransport.instances).toHaveLength(1);

      await vi.advanceTimersByTimeAsync(500);
      expect(transport.status).toBe("connected");
      expect(pinnedHash(MockWebTransport.instances[1]!)).toEqual(
        new Uint8Array(32).fill(0xcd),
      );
      transport.close();
    });

    it("bounds discovery by the connect timeout and aborts the fetch", async () => {
      const resolveHash = vi
        .fn()
        .mockImplementationOnce(() => new Promise<string>(() => {}))
        .mockResolvedValueOnce(latestHash);
      const transport = new YasWebTransportTransport(
        "https://example.test/yas",
        "ok",
        { serverCertificateHash: resolveHash, connectTimeoutMs: 50 },
      );
      transport.connect();
      const signal = resolveHash.mock.calls[0]![0] as AbortSignal;
      await vi.advanceTimersByTimeAsync(50);
      expect(transport.status).toBe("error");
      expect(signal.aborted).toBe(true);
      expect(MockWebTransport.instances).toHaveLength(0);

      await vi.advanceTimersByTimeAsync(500);
      expect(transport.status).toBe("connected");
      transport.close();
    });

    it.each(["close", "suspend"] as const)(
      "does not open a session after %s during discovery",
      async (action) => {
        let finish!: (value: string) => void;
        const resolveHash = vi.fn(
          (_signal: AbortSignal) =>
            new Promise<string>((resolve) => {
              finish = resolve;
            }),
        );
        const transport = new YasWebTransportTransport(
          "https://example.test/yas",
          "ok",
          { serverCertificateHash: resolveHash },
        );
        transport.connect();
        transport[action]();
        expect(resolveHash.mock.calls[0]![0].aborted).toBe(true);
        finish(firstHash);
        await vi.advanceTimersByTimeAsync(10_000);
        expect(MockWebTransport.instances).toHaveLength(0);
        expect(resolveHash).toHaveBeenCalledOnce();
        expect(transport.status).toBe(
          action === "close" ? "closed" : "disconnected",
        );
        transport.close();
      },
    );

    it.each(["resolve", "reject"])(
      "ignores a superseded discovery that later completes with %s",
      async (result) => {
        let finish!: (value: string) => void;
        let fail!: (error: Error) => void;
        const resolveHash = vi
          .fn()
          .mockImplementationOnce(
            () =>
              new Promise<string>((resolve, reject) => {
                finish = resolve;
                fail = reject;
              }),
          )
          .mockResolvedValueOnce(latestHash);
        const transport = new YasWebTransportTransport(
          "https://example.test/yas",
          "ok",
          { serverCertificateHash: resolveHash },
        );
        transport.connect();
        transport.reconnect();
        await vi.advanceTimersByTimeAsync(0);
        expect(transport.status).toBe("connected");

        if (result === "resolve") finish(firstHash);
        else fail(new Error("obsolete discovery"));
        await vi.advanceTimersByTimeAsync(10_000);
        expect(MockWebTransport.instances).toHaveLength(1);
        expect(resolveHash).toHaveBeenCalledTimes(2);
        expect(transport.status).toBe("connected");
        expect(transport.lastError).toBeNull();
        expect(pinnedHash(MockWebTransport.instances[0]!)).toEqual(
          new Uint8Array(32).fill(0xcd),
        );
        transport.close();
      },
    );
  });

  it("uses createWritable on browsers with the current datagram API", async () => {
    const transport = new YasWebTransportTransport(
      "https://example.test/yas",
      "ok",
      { reconnect: false },
    );
    transport.connect();
    const session = MockWebTransport.instances[0]!;
    Object.defineProperty(session.datagrams, "writable", {
      get: () => {
        throw new Error("legacy writable must not be accessed");
      },
    });
    const createWritable = vi.fn(function (
      this: WebTransportDatagramDuplexStream,
    ) {
      expect(this).toBe(session.datagrams);
      return session.datagramWritable;
    });
    Object.assign(session.datagrams, { createWritable });

    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    expect(createWritable).toHaveBeenCalledOnce();
    expect(transport.maxDatagramSize).toBe(65_536);
    transport.sendDatagram(new Uint8Array([7, 6]));
    await vi.waitFor(() =>
      expect(session.datagramWrites).toEqual([new Uint8Array([7, 6])]),
    );
    transport.close();
  });

  it.each(["missing", "throws"])(
    "keeps the reliable stream when datagram writer setup %s",
    async (failure) => {
      const transport = new YasWebTransportTransport(
        "https://example.test/yas",
        "ok",
        { reconnect: false },
      );
      transport.connect();
      const session = MockWebTransport.instances[0]!;
      Object.defineProperty(session.datagrams, "writable", {
        value: undefined,
      });
      if (failure === "throws") {
        Object.assign(session.datagrams, {
          createWritable() {
            throw new Error("datagrams unavailable");
          },
        });
      }

      await vi.waitFor(() => expect(transport.status).toBe("connected"));
      expect(transport.maxDatagramSize).toBe(0);
      expect(transport.lastError).toBeNull();
      transport.send(new Uint8Array([5, 4, 3]));
      await vi.waitFor(() => expect(session.streamWrites).toHaveLength(2));
      expect(session.streamWrites[1]).toEqual(new Uint8Array([5, 4, 3]));
      transport.close();
    },
  );

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
